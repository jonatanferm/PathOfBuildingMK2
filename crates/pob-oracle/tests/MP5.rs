//! Unit MP5 acceptance (calc doc §11.1, row MP3-MP6: specialModList, ModParser.lua 4037-4988,
//! `pob-modparser/src/special_c.rs`).
//!
//! * `list_shape`: `special_c::entries()` holds one entry per literal key of ModParser.lua
//!   4037-4988, in source order, each with the line that defines it (618 entries: 344 static
//!   tables, 274 closures); building it hits no `unported!` site.
//! * `mp5_oracle` (PoB booted under LuaJIT with the JIT off, ModParser.lua instrumented in
//!   memory to hand out a copy of the literal `specialModList` table before the keystone entries
//!   and the `^…$` anchoring are added, 5943-5953):
//!   - every static entry equals PoB's table (mod by mod: name, type, value, flags,
//!     keywordFlags, source, tags);
//!   - every one of the 274 closures is called in the parse driver's shape
//!     `f(tonumber(cap[1]), unpack(cap))` (ModParser.lua:6662-6663) with sample captures: every
//!     distinct capture tuple PoB's own scans produced for that pattern in
//!     `data/dump/parse_oracle.jsonl`, plus synthetic captures derived from the pattern's
//!     capture classes. The Lua result and the Rust `Option<Vec<Mod>>` must be equal; when the
//!     Lua raises, the Rust must raise the same message;
//!   - the known parse-level differences (calc doc §6, CCR-0006 §7, `docs/port/known_diffs.toml`
//!     MP5-1 / MP5-2) are the only accepted differences, and only at their lines: tags that
//!     follow a nil `mod[1]` (ipairs never sees them; Rust drops them) and tag fields holding a
//!     digits-only capture string in Lua (Rust holds `tonumber` of it);
//!   - L1 for this range: every parse-oracle line whose parse used a specialModList entry of
//!     4037-4988 is parsed by MP0's driver (`parseModPassWith`, both orders as `parseMod`)
//!     over the real lists (formList/modNameList from MP1, the flag/tag lists from MP2, the
//!     generated lists from MP0, these entries) and must give PoB's mods and remainder. The
//!     other specialModList ranges (MP3, MP4, MP6) are used when ported; while they are stubs
//!     their keys are present with a sentinel value, and lines that reach them are left out.
//!
//! Inputs: `data/dump` and the PoB checkout; missing inputs fail
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips). `MP5_L1_STRIDE=n` checks every n-th L1 record.
#![allow(non_snake_case)]
#![allow(clippy::type_complexity, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Once;

use pob_gamedata::{Mod, ModType, Name, Tag, Value, ValueTable};
use pob_lua::LuaError;
use pob_modparser::generated_lists as gl;
use pob_modparser::scan::{parseModPassWith, ParserLists};
use pob_modparser::types::{Caps, PatternEntry, PatternList, SpecialVal};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};
use serde_json::Value as J;

const RANGE: (usize, usize) = (4037, 4988);
/// The other literal specialModList ranges (calc doc §11.1 MP3, MP4, MP6).
const OTHER_RANGES: [(&str, usize, usize); 3] = [
    ("MP3", 2130, 3083),
    ("MP4", 3084, 4036),
    ("MP6", 4989, 5942),
];

/// Known diff MP5-1: `mod(…, nil, {tag})` (createMod stores nil at `mod[1]`; ipairs never
/// reaches the tag). CCR-0006 §7, calc doc §6.
const HOLEY_TAG_LINES: [u32; 5] = [4576, 4577, 4578, 4579, 4580];
/// Known diff MP5-2: a tag field that holds a digits-only capture string in Lua
/// (`percent = percent`, `div = 1 / percent` coerces) and `tonumber` of it in Rust.
const STRING_TAG_FIELD_LINES: [u32; 2] = [4216, 4222];

fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        return p.join("data.bin").is_file().then_some(p);
    }
    find_up("data/dump/data.bin").map(|p| p.parent().unwrap().to_path_buf())
}

fn init_data() -> Option<PathBuf> {
    static INIT: Once = Once::new();
    let dir = dump_dir()?;
    INIT.call_once(|| {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    });
    Some(dir)
}

fn modparser_src() -> Option<String> {
    let src = find_pob_src()?;
    std::fs::read_to_string(src.join("Modules/ModParser.lua")).ok()
}

/// `["key"] = …` entries at the top level of a table constructor in lines `a..=b`, in source
/// order of first appearance, each with the line of its last definition.
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<(String, u32)> {
    let mut by_key: BTreeMap<String, u32> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for ln in a..=b {
        let s = src[ln - 1];
        let indent = &s[..s.len() - s.trim_start().len()];
        if indent != "\t" {
            continue;
        }
        let Some(rest) = s.trim_start().strip_prefix('[') else {
            continue;
        };
        let q = rest.as_bytes()[0];
        if q != b'"' && q != b'\'' {
            continue;
        }
        let body = &rest[1..];
        let end = body.find(q as char).unwrap();
        let key = &body[..end];
        assert!(!key.contains('\\'), "ModParser.lua:{ln}: escaped key");
        assert!(
            body[end + 1..].trim_start().starts_with(']'),
            "ModParser.lua:{ln}"
        );
        if by_key.insert(key.to_owned(), ln as u32).is_none() {
            order.push(key.to_owned());
        }
    }
    order
        .into_iter()
        .map(|k| {
            let l = by_key[&k];
            (k, l)
        })
        .collect()
}

fn ours() -> &'static [PatternEntry<SpecialVal>] {
    static L: std::sync::LazyLock<Vec<PatternEntry<SpecialVal>>> =
        std::sync::LazyLock::new(pob_modparser::special_c::entries);
    &L
}

// ---------------------------------------------------------------------------------------------
// list_shape
// ---------------------------------------------------------------------------------------------

#[test]
fn list_shape() {
    let Some(src) = pob_testpaths::require(modparser_src(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let src: Vec<&str> = src.lines().collect();
    pob_gamedata::stub::stub_reset();
    let got: Vec<(&str, u32)> = pob_modparser::special_c::entries()
        .iter()
        .map(|e| (e.pat, e.lua_line))
        .collect();
    let hits: Vec<_> = pob_gamedata::stub::stub_report()
        .into_iter()
        .filter(|(s, _)| {
            s.starts_with("Modules/ModParser.lua:4") && {
                let n: usize = s["Modules/ModParser.lua:".len()..]
                    .split(['-', ':'])
                    .next()
                    .and_then(|x| x.parse().ok())
                    .unwrap_or(0);
                (RANGE.0..=RANGE.1).contains(&n)
            }
        })
        .collect();
    assert!(hits.is_empty(), "unported sites hit: {hits:?}");
    let lit = literal_keys(&src, RANGE.0, RANGE.1);
    let lit: Vec<(&str, u32)> = lit.iter().map(|(k, l)| (k.as_str(), *l)).collect();
    assert_eq!(got.len(), 618, "entry count");
    assert!(
        got.windows(2).all(|w| w[0].1 < w[1].1),
        "entries not in source order"
    );
    assert_eq!(got, lit, "keys/lines differ from ModParser.lua");
    let nFn = ours()
        .iter()
        .filter(|e| !matches!(e.val, SpecialVal::Mods(_)))
        .count();
    assert_eq!(
        (got.len() - nFn, nFn),
        (344, 274),
        "static / closure counts"
    );
    eprintln!(
        "specialModList {}-{}: {} entries ({} static, {nFn} closures), keys and order equal ModParser.lua",
        RANGE.0,
        RANGE.1,
        got.len(),
        got.len() - nFn
    );
}

// ---------------------------------------------------------------------------------------------
// LuaJIT side
// ---------------------------------------------------------------------------------------------

/// Installed before PoB boots: ModParser.lua (loaded through `loadfile` by `LoadModule`) gets a
/// shallow copy of its literal `specialModList` stored in `__mp5_lit`, prepended to the line
/// `for _, name in ipairs(data.keystones) do` (5943). Line numbers are unchanged.
const PRE: &str = r#"
local orig_loadfile = loadfile
loadfile = function(path, ...)
    if type(path) == "string" and path:match("Modules/ModParser%.lua$") then
        local orig_loadstring = loadstring
        local hit = false
        loadstring = function(src, name)
            loadstring = orig_loadstring
            local n
            src, n = src:gsub("\nfor _, name in ipairs%(data%.keystones%) do\n", "\n__mp5_lit = { } for k, v in pairs(specialModList) do __mp5_lit[k] = v end for _, name in ipairs(data.keystones) do\n", 1)
            hit = n == 1
            return orig_loadstring(src, name)
        end
        local f, e = orig_loadfile(path, ...)
        loadstring = orig_loadstring
        assert(hit, "MP5: ModParser.lua instrumentation point (keystone loop) not found")
        return f, e
    end
    return orig_loadfile(path, ...)
end
"#;

/// The encoder (pob-dump FORMAT.md shape: arrays for 1..n tables, else objects with string keys,
/// `$seq` and `$num`) and the call shape (Lua side).
const LUA_LIB: &str = r##"
local fmt, t_sort, t_concat = string.format, table.sort, table.concat
local function estr(s)
    return '"' .. s:gsub('[%c"\\]', function(c) return fmt("\\u%04x", c:byte()) end) .. '"'
end
local function enum(v)
    if v ~= v then return '{"$float":"nan"}' end
    if v == math.huge then return '{"$float":"inf"}' end
    if v == -math.huge then return '{"$float":"-inf"}' end
    if v == 0 and 1 / v < 0 then return '{"$float":"-0"}' end
    if v == math.floor(v) and math.abs(v) < 2^53 then return fmt("%d", v) end
    return fmt("%.17g", v)
end
local enc
enc = function(v)
    local t = type(v)
    if t == "string" then return estr(v)
    elseif t == "number" then return enum(v)
    elseif t == "boolean" then return tostring(v)
    elseif t == "nil" then return "null"
    elseif t == "table" then
        local n = 0
        while rawget(v, n + 1) ~= nil do n = n + 1 end
        local cnt = 0
        for _ in pairs(v) do cnt = cnt + 1 end
        if n > 0 and cnt == n and getmetatable(v) == nil then
            local out = {}
            for i = 1, n do out[i] = enc(v[i]) end
            return "[" .. t_concat(out, ",") .. "]"
        end
        local skeys, nkeys = {}, {}
        for k in pairs(v) do
            if type(k) == "string" then skeys[#skeys + 1] = k
            elseif type(k) == "number" and not (k == math.floor(k) and k >= 1 and k <= n) then nkeys[#nkeys + 1] = k
            elseif type(k) ~= "number" then error("MP5 encoder: key of type " .. type(k)) end
        end
        t_sort(skeys)
        t_sort(nkeys)
        local out = {}
        for _, k in ipairs(skeys) do
            out[#out + 1] = estr(k:sub(1, 1) == "$" and ("$" .. k) or k) .. ":" .. enc(v[k])
        end
        if n > 0 then
            local s = {}
            for i = 1, n do s[i] = enc(v[i]) end
            out[#out + 1] = '"$seq":[' .. t_concat(s, ",") .. "]"
        end
        if #nkeys > 0 then
            local s = {}
            for _, k in ipairs(nkeys) do s[#s + 1] = estr(fmt("%.17g", k)) .. ":" .. enc(v[k]) end
            out[#out + 1] = '"$num":{' .. t_concat(s, ",") .. "}"
        end
        return "{" .. t_concat(out, ",") .. "}"
    else
        return estr("$" .. t)
    end
end
__mp5_enc = enc
-- keys: { key, … }; samples: { [key] = { {cap…}, … } }
-- → { [key] = { kind = "table", v = … } | { kind = "function", calls = { { ok, v | err } } } }
function __mp5_run(keys, samples)
    local res = {}
    for _, k in ipairs(keys) do
        local v = __mp5_lit[k]
        if type(v) == "function" then
            local calls = {}
            for i, cap in ipairs(samples[k] or {}) do
                local ok, r = pcall(function() return v(tonumber(cap[1]), unpack(cap)) end)
                if ok then
                    calls[i] = { ok = true, v = r == nil and "$nil" or r }
                else
                    calls[i] = { ok = false, err = tostring(r) }
                end
            end
            res[k] = { kind = "function", calls = calls }
        elseif v == nil then
            res[k] = { kind = "missing" }
        else
            res[k] = { kind = type(v), v = v }
        end
    end
    return enc(res)
end
"##;

/// A Lua string literal (decimal escapes for anything but printable ASCII).
fn lua_lit(s: &str) -> String {
    let mut o = String::from("\"");
    for b in s.bytes() {
        match b {
            b'"' | b'\\' => {
                o.push('\\');
                o.push(b as char);
            }
            0x20..=0x7e => o.push(b as char),
            _ => o.push_str(&format!("\\{b}")),
        }
    }
    o.push('"');
    o
}

struct Lua(JitPob);

impl Lua {
    fn eval(&mut self, code: &str) -> String {
        let r = self.0.run(code).unwrap_or_else(|e| panic!("lua: {e}"));
        match r.first() {
            Some(Ret::Str(s)) => s.clone(),
            other => panic!("lua returned {other:?}"),
        }
    }
}

/// Lua `pcall`: a `lua_error!` becomes `Err(message)`.
fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(e.0),
            Err(p) => std::panic::resume_unwind(p),
        },
    }
}

/// The Lua error text without its `chunk:line: ` position prefix.
fn strip_pos(e: &str) -> &str {
    match e.find(": ") {
        Some(i) if e[..i].contains(':') => &e[i + 2..],
        _ => e,
    }
}

// ---------------------------------------------------------------------------------------------
// Mod lists: Lua JSON -> Mod, comparison, known differences
// ---------------------------------------------------------------------------------------------

/// A Lua mod list (the encoder's JSON) as `Vec<Mod>`: an array, or the empty table `{}`.
fn lua_mods(v: &J, what: &str) -> Vec<Mod> {
    let seq = match v {
        J::Object(o) => {
            assert!(o.keys().all(|k| k == "$seq"), "{what}: mod list {v}");
            o.get("$seq").cloned().unwrap_or(J::Array(Vec::new()))
        }
        other => other.clone(),
    };
    serde_json::from_value::<Vec<Mod>>(seq).unwrap_or_else(|e| panic!("{what}: mods {v}: {e}"))
}

/// Tag fields that hold a number in Rust (`Option<f64>` fields of [`Tag`] that a Lua string
/// capture can reach).
const NUM_TAG_FIELDS: [&str; 6] = [
    "div",
    "limit",
    "percent",
    "globalLimit",
    "base",
    "threshold",
];

/// Removes the two known parse-level differences from a Lua-side mod (the deserialiser keeps
/// them in `extra`, modifier.rs module docs): tags at non-sequence indices (MP5-1) and the
/// original string of a coerced numeric tag field (MP5-2). Returns (holes, strings) removed.
fn strip_known(m: &mut Mod) -> (usize, usize) {
    let mut holes = 0;
    let mut strs = 0;
    if let Some(x) = m.extra.take() {
        let mut x = *x;
        let before = x.fields.len();
        x.fields.retain(|(k, _)| !k.as_str().starts_with('#'));
        holes += before - x.fields.len();
        if !(x.fields.is_empty() && x.array.is_empty()) {
            m.extra = Some(Box::new(x));
        }
    }
    for t in m.tags.iter_mut() {
        strs += strip_tag(t);
    }
    if let Value::Table(t) = &m.value {
        let mut t2: ValueTable = (**t).clone();
        let mut changed = false;
        for (_, v) in t2.fields.iter_mut() {
            if let Value::Mod(inner) = v {
                let mut im = (**inner).clone();
                let (h, s) = strip_known(&mut im);
                if h + s > 0 {
                    holes += h;
                    strs += s;
                    *v = Value::Mod(std::sync::Arc::new(im));
                    changed = true;
                }
            }
        }
        if changed {
            m.value = Value::Table(std::sync::Arc::new(t2));
        }
    }
    (holes, strs)
}

fn strip_tag(t: &mut Tag) -> usize {
    let Some(x) = t.extra.take() else {
        return 0;
    };
    let mut x = *x;
    let before = x.fields.len();
    x.fields.retain(|(k, v)| {
        !(NUM_TAG_FIELDS.contains(&k.as_str())
            && matches!(v, Value::Str(s) if pob_lua::tonumber(s).is_some()))
    });
    let n = before - x.fields.len();
    if !(x.fields.is_empty() && x.array.is_empty()) {
        t.extra = Some(Box::new(x));
    }
    n
}

fn num_eq(a: f64, b: f64) -> bool {
    a.to_bits() == b.to_bits()
        || (a == b && a != 0.0)
        || (a.is_nan() && b.is_nan())
        || (a == 0.0 && b == 0.0) // ±0: host artefact (CCR-0002), checked separately below
}

fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => num_eq(*x, *y),
        (Value::Mod(x), Value::Mod(y)) => mod_eq(x, y),
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.array.iter().zip(&y.array).all(|(p, q)| value_eq(p, q))
                && x.fields.len() == y.fields.len()
                && x.fields.iter().all(|(k, v)| value_eq(v, y.get(*k)))
        }
        _ => a == b,
    }
}

fn mod_eq(a: &Mod, b: &Mod) -> bool {
    a.name == b.name
        && a.r#type == b.r#type
        && a.flags == b.flags
        && a.keywordFlags == b.keywordFlags
        && value_eq(&a.value, &b.value)
        && a.source == b.source
        && a.tags.len() == b.tags.len()
        && a.tags.iter().zip(&b.tags).all(|(x, y)| x == y)
        && a.extra == b.extra
}

fn mods_eq(a: &[Mod], b: &[Mod]) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(p, q)| mod_eq(p, q))
}

fn show(m: &[Mod]) -> String {
    serde_json::to_string(m).unwrap()
}

// ---------------------------------------------------------------------------------------------
// Sample captures
// ---------------------------------------------------------------------------------------------

/// The capture groups of a Lua pattern (their source text), in order.
fn capture_groups(pat: &str) -> Vec<String> {
    let b = pat.as_bytes();
    let mut out = Vec::new();
    let mut stack: Vec<usize> = Vec::new();
    let mut i = 0;
    while i < b.len() {
        match b[i] {
            b'%' => i += 1,
            b'[' => {
                i += 1;
                if i < b.len() && b[i] == b'^' {
                    i += 1;
                }
                if i < b.len() && b[i] == b']' {
                    i += 1;
                }
                while i < b.len() && b[i] != b']' {
                    if b[i] == b'%' {
                        i += 1;
                    }
                    i += 1;
                }
            }
            b'(' => stack.push(i),
            b')' => {
                let s = stack.pop().expect("unbalanced pattern");
                out.push((s, pat[s + 1..i].to_owned()));
            }
            _ => {}
        }
        i += 1;
    }
    out.sort_by_key(|(s, _)| *s);
    out.into_iter().map(|(_, g)| g).collect()
}

/// Sample values for one capture group (including inputs on which some closures raise).
fn samples_for(group: &str) -> Vec<&'static str> {
    match group {
        "%d+" => vec!["1", "10", "250"],
        "%d" => vec!["1", "5"],
        "[%d%.]+" => vec!["1.5", "25", ".", "1.2.3"],
        "%a+" => vec![
            "fire",
            "cold",
            "lightning",
            "chaos",
            "physical",
            "red",
            "shaper",
        ],
        "%w+" => vec!["fire", "cold", "1", "2x"],
        ".+" => vec!["fire", "cold", "blue", "a1"],
        ".-" => vec!["enfeeble", "fire", ""],
        "[%+%-][%d%.]+" => vec!["+1.5", "-2", "+."],
        "%-%d+" => vec!["-10", "-1"],
        "%-?%d+" => vec!["-5", "10"],
        "[%+%-]%d+" => vec!["+5", "-3"],
        "%D+" => vec!["vulnerability", "enfeeble", "x"],
        "[%a%s]+" => vec!["gloves", "body armour", "rings"],
        "%a-" => vec!["freeze", "ignite", ""],
        _ => vec!["x", "1"],
    }
}

/// Synthetic capture tuples: the product of every group's samples (last group fastest), at most
/// `cap` tuples.
fn synthetic(pat: &str, cap: usize) -> Vec<Vec<String>> {
    let groups: Vec<Vec<&str>> = capture_groups(pat).iter().map(|g| samples_for(g)).collect();
    if groups.is_empty() {
        return vec![Vec::new()];
    }
    let mut out = Vec::new();
    let mut idx = vec![0usize; groups.len()];
    'outer: loop {
        out.push(
            idx.iter()
                .zip(&groups)
                .map(|(i, g)| g[*i].to_owned())
                .collect(),
        );
        if out.len() >= cap {
            break;
        }
        let mut k = groups.len();
        loop {
            if k == 0 {
                break 'outer;
            }
            k -= 1;
            idx[k] += 1;
            if idx[k] < groups[k].len() {
                break;
            }
            idx[k] = 0;
        }
    }
    out
}

#[derive(serde::Deserialize)]
struct Record {
    line: String,
    mods: Option<J>,
    extra: Option<String>,
    passes: Vec<Pass>,
}

#[derive(serde::Deserialize)]
struct Pass {
    steps: Vec<Step>,
}

#[derive(serde::Deserialize)]
struct Step {
    list: String,
    pattern: Option<String>,
    line: Option<u32>,
    #[serde(default)]
    caps: Vec<J>,
    #[serde(default)]
    exact: bool,
}

fn load_oracle(dir: &std::path::Path) -> Option<Vec<Record>> {
    let text = std::fs::read_to_string(dir.join("parse_oracle.jsonl")).ok()?;
    Some(
        text.lines()
            .map(|l| serde_json::from_str::<Record>(l).unwrap_or_else(|e| panic!("{e}: {l}")))
            .collect(),
    )
}

fn cap_text(v: &J) -> String {
    match v {
        J::String(s) => s.clone(),
        other => other.to_string(),
    }
}

fn in_range(l: u32) -> bool {
    (RANGE.0 as u32..=RANGE.1 as u32).contains(&l)
}

/// The Rust call in the driver's shape (ModParser.lua:6663, scan.rs `ScanCaps`):
/// `num = tonumber(cap[1])`, `cap[k] = cap k+1`.
fn rust_call(f: &SpecialVal, cap: &[String]) -> Result<Option<Vec<Mod>>, String> {
    let mut caps = Caps::default();
    for (i, c) in cap.iter().take(5).enumerate() {
        caps.cap[i] = Some(c.as_str());
    }
    caps.num = cap.first().and_then(|c| pob_lua::tonumber(c));
    pcall(|| match f {
        SpecialVal::Fn(f) => f(&caps),
        SpecialVal::Dyn(f) => f(&caps),
        SpecialVal::Mods(m) => Some(m.clone()),
    })
}

/// Counts of the known differences actually seen, per Lua line.
#[derive(Default)]
struct Known {
    holes: BTreeMap<u32, usize>,
    strs: BTreeMap<u32, usize>,
}

/// Compares PoB's mods (`want`, the Lua JSON) with ours for the entry at `line`.
fn compare(line: u32, want: &J, got: &[Mod], what: &str, known: &mut Known, bad: &mut Vec<String>) {
    let mut w = lua_mods(want, what);
    for m in &mut w {
        let (h, s) = strip_known(m);
        if h > 0 {
            *known.holes.entry(line).or_default() += h;
        }
        if s > 0 {
            *known.strs.entry(line).or_default() += s;
        }
    }
    if !mods_eq(&w, got) {
        bad.push(format!("{what}: PoB {} / ours {}", show(&w), show(got)));
        return;
    }
    // ±0 is accepted above (host artefact, CCR-0002) but counted
    let signs = |m: &[Mod]| -> Vec<bool> {
        m.iter()
            .filter_map(|x| match x.value {
                Value::Num(v) => Some(v.is_sign_negative() && v == 0.0),
                _ => None,
            })
            .collect()
    };
    if signs(&w) != signs(got) {
        eprintln!("note: {what}: sign of zero differs (CCR-0002 host artefact)");
    }
}

// ---------------------------------------------------------------------------------------------
// mp5_oracle
// ---------------------------------------------------------------------------------------------

#[test]
fn mp5_oracle() {
    let Some(dir) = pob_testpaths::require(init_data(), "data/dump not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(pob_src) = pob_testpaths::require(find_pob_src(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let modparser = std::fs::read_to_string(pob_src.join("Modules/ModParser.lua")).unwrap();
    let srcl: Vec<&str> = modparser.lines().collect();
    // HYG2: a missing parse oracle used to print and drop L1 and coverage silently
    let Some(records) = pob_testpaths::require(load_oracle(&dir), "data/dump/parse_oracle.jsonl")
    else {
        return; // POB_ALLOW_SKIP=1
    };
    assert_eq!(
        records.len(),
        PARSE_ORACLE_RECORDS,
        "parse_oracle.jsonl records"
    );
    let records = Some(records);
    let entries = ours();

    // -- sample captures per closure
    let fns: BTreeSet<&'static str> = entries
        .iter()
        .filter(|e| !matches!(e.val, SpecialVal::Mods(_)))
        .map(|e| e.pat)
        .collect();
    let mut samples: BTreeMap<&'static str, Vec<Vec<String>>> = BTreeMap::new();
    let mut fromOracle = 0usize;
    if let Some(recs) = &records {
        let mut seen: BTreeMap<&'static str, BTreeSet<Vec<String>>> = BTreeMap::new();
        for r in recs {
            for p in &r.passes {
                for st in &p.steps {
                    if st.list != "specialModList" {
                        continue;
                    }
                    let Some(pat) = st.pattern.as_deref() else {
                        continue;
                    };
                    let Some(k) = pat.strip_prefix('^').and_then(|p| p.strip_suffix('$')) else {
                        continue;
                    };
                    let Some(&k) = fns.get(k) else {
                        continue;
                    };
                    let s = seen.entry(k).or_default();
                    if s.len() < 40 {
                        s.insert(st.caps.iter().map(cap_text).collect());
                    }
                }
            }
        }
        fromOracle = seen.len();
        for (k, s) in seen {
            samples.entry(k).or_default().extend(s);
        }
    }
    for k in &fns {
        let v = samples.entry(k).or_default();
        for s in synthetic(k, 60) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }

    // -- run the Lua side
    let _lock = pob_oracle::runner::OracleLock::acquire().unwrap_or_else(|e| panic!("{e}"));
    let t0 = std::time::Instant::now();
    let pob = JitPob::boot_oracle(&pob_src, Some(PRE)).unwrap_or_else(|e| panic!("boot: {e}"));
    let mut lua = Lua(pob);
    eprintln!("PoB booted in {:.1}s", t0.elapsed().as_secs_f64());
    lua.0
        .run(LUA_LIB)
        .unwrap_or_else(|e| panic!("lua lib: {e}"));
    let mut code = String::from("local K = {\n");
    for e in entries {
        code.push_str(&lua_lit(e.pat));
        code.push_str(",\n");
    }
    code.push_str("}\nlocal S = {\n");
    for (k, tuples) in &samples {
        code.push_str(&format!("[{}] = {{", lua_lit(k)));
        for t in tuples {
            code.push('{');
            for c in t {
                code.push_str(&lua_lit(c));
                code.push(',');
            }
            code.push_str("},");
        }
        code.push_str("},\n");
    }
    code.push_str("}\nreturn __mp5_run(K, S)\n");
    let out: J = serde_json::from_str(&lua.eval(&code)).expect("lua json");
    let lo = out.as_object().unwrap();

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // -- compare every entry
    let mut bad: Vec<String> = Vec::new();
    let mut known = Known::default();
    let (mut nStatic, mut nFn, mut nCalls, mut nErrCalls, mut nNil) = (0, 0, 0, 0, 0);
    let mut minCalls = usize::MAX;
    for e in entries {
        let pat = e.pat;
        let Some(l) = lo.get(pat) else {
            bad.push(format!("{pat:?}: no PoB result"));
            continue;
        };
        let kind = l["kind"].as_str().unwrap();
        match &e.val {
            SpecialVal::Mods(m) => {
                nStatic += 1;
                if kind != "table" {
                    bad.push(format!("{pat:?}: PoB has a {kind}, ours is a table"));
                    continue;
                }
                compare(
                    e.lua_line,
                    &l["v"],
                    m,
                    &format!("L{} {pat:?}", e.lua_line),
                    &mut known,
                    &mut bad,
                );
            }
            f => {
                nFn += 1;
                if kind != "function" {
                    bad.push(format!("{pat:?}: PoB has a {kind}, ours is a closure"));
                    continue;
                }
                let tuples = &samples[pat];
                let calls = l["calls"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                assert_eq!(calls.len(), tuples.len(), "{pat:?}: call count");
                minCalls = minCalls.min(calls.len());
                for (cap, c) in tuples.iter().zip(calls) {
                    nCalls += 1;
                    let what = format!("L{} {pat:?} caps {cap:?}", e.lua_line);
                    let ours = rust_call(f, cap);
                    match (c["ok"].as_bool().unwrap(), ours) {
                        (true, Ok(got)) => match (&c["v"], got) {
                            (J::String(s), None) if s == "$nil" => nNil += 1,
                            (J::String(s), Some(g)) if s == "$nil" => {
                                bad.push(format!("{what}: PoB nil / ours {}", show(&g)));
                            }
                            (v, None) => bad.push(format!("{what}: PoB {v} / ours nil")),
                            (v, Some(g)) => compare(e.lua_line, v, &g, &what, &mut known, &mut bad),
                        },
                        (false, Err(err)) => {
                            nErrCalls += 1;
                            let le = c["err"].as_str().unwrap();
                            if strip_pos(le) != err {
                                bad.push(format!("{what}: PoB raises {le:?} / ours {err:?}"));
                            }
                        }
                        (true, Err(err)) => bad.push(format!(
                            "{what}: PoB returns {} / ours raises {err:?}",
                            c["v"]
                        )),
                        (false, Ok(g)) => bad.push(format!(
                            "{what}: PoB raises {} / ours {:?}",
                            c["err"],
                            g.as_deref().map(show)
                        )),
                    }
                }
            }
        }
    }
    eprintln!(
        "entries: {nStatic} static tables, {nFn} closures; {nCalls} closure calls ({nErrCalls} raise in both, \
         {nNil} return nil in both), >= {minCalls} per closure; {fromOracle} closures have captures recorded by PoB's own scans"
    );
    assert_eq!((nStatic, nFn), (344, 274), "entry kinds");
    assert!(
        minCalls > 0 && minCalls != usize::MAX,
        "a closure was never called"
    );

    // -- the known differences occur exactly at their lines
    let holeLines: Vec<u32> = known.holes.keys().copied().collect();
    let strLines: Vec<u32> = known.strs.keys().copied().collect();
    eprintln!(
        "known diffs: MP5-1 (tags after a nil mod[1]) at {:?}, MP5-2 (string tag fields) at {:?}",
        known.holes, known.strs
    );
    if holeLines != HOLEY_TAG_LINES {
        bad.push(format!("MP5-1 lines {holeLines:?} != {HOLEY_TAG_LINES:?}"));
    }
    if strLines != STRING_TAG_FIELD_LINES {
        bad.push(format!(
            "MP5-2 lines {strLines:?} != {STRING_TAG_FIELD_LINES:?}"
        ));
    }

    // -- L1 over the lines whose parse used these entries
    if let Some(recs) = &records {
        l1(&srcl, recs, &mut bad);
    }
    std::panic::set_hook(hook);
    for b in bad.iter().take(80) {
        eprintln!("MISMATCH {b}");
    }
    assert!(bad.is_empty(), "{} mismatches", bad.len());
}

/// PoB's mods of a record (the parse oracle's JSON), with the known differences removed.
fn oracle_mods(r: &Record) -> Option<Vec<Mod>> {
    r.mods.as_ref().map(|v| {
        let mut m = lua_mods(v, &r.line);
        for x in &mut m {
            strip_known(x);
        }
        m
    })
}

fn l1(src: &[&str], recs: &[Record], bad: &mut Vec<String>) {
    // the literal specialModList: MP3/MP4/MP6 when ported, else their keys with a sentinel
    let sentinel = || {
        SpecialVal::Mods(vec![pob_gamedata::mod_tools::createMod(
            Name::from("MP5:otherUnitStub"),
            ModType::FLAG,
            true,
            None,
            0,
            0,
            &[],
        )])
    };
    let parts: [(&str, Vec<PatternEntry<SpecialVal>>); 3] = [
        ("MP3", pob_modparser::special_a::entries()),
        ("MP4", pob_modparser::special_b::entries()),
        ("MP6", pob_modparser::special_d::entries()),
    ];
    let mut stubbed: Vec<(usize, usize)> = Vec::new();
    let mut literal: Vec<PatternEntry<SpecialVal>> = Vec::new();
    let mut push_part = |literal: &mut Vec<PatternEntry<SpecialVal>>, i: usize| {
        let (unit, part) = &parts[i];
        let (_, a, b) = OTHER_RANGES[i];
        if part.is_empty() {
            stubbed.push((a, b));
            for (k, l) in literal_keys(src, a, b) {
                literal.push(PatternEntry {
                    pat: Box::leak(k.into_boxed_str()),
                    val: sentinel(),
                    lua_line: l,
                });
            }
            eprintln!(
                "L1: {unit} ({a}-{b}) is a stub: keys only, lines that reach it are left out"
            );
        } else {
            for e in part {
                literal.push(PatternEntry {
                    pat: e.pat,
                    val: e.val.clone_val(),
                    lua_line: e.lua_line,
                });
            }
        }
    };
    push_part(&mut literal, 0);
    push_part(&mut literal, 1);
    for e in pob_modparser::special_c::entries() {
        literal.push(e);
    }
    push_part(&mut literal, 2);
    let special: &'static PatternList<SpecialVal> =
        Box::leak(Box::new(gl::complete_special_mod_list(literal)));
    let lists = ParserLists {
        specialModList: special,
        ..ParserLists::statics()
    };
    let jewelStub = lists.jewelFuncList.is_empty();
    let stride: usize = std::env::var("MP5_L1_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let stubbedLine = |l: u32| {
        stubbed
            .iter()
            .any(|&(a, b)| (a as u32..=b as u32).contains(&l))
    };
    let sel: Vec<&Record> = recs
        .iter()
        .filter(|r| {
            let steps = || r.passes.iter().flat_map(|p| p.steps.iter());
            steps().any(|s| {
                s.list == "specialModList" && s.pattern.is_some() && s.line.is_some_and(in_range)
            }) && !steps().any(|s| {
                s.exact
                    || (s.pattern.is_some()
                        && ((s.list == "specialModList" && s.line.is_some_and(stubbedLine))
                            || (jewelStub && s.list == "jewelFuncList")))
            })
        })
        .step_by(stride)
        .collect();
    let t = std::time::Instant::now();
    let (mut ok, mut nbad) = (0usize, 0usize);
    let mut entriesHit: BTreeSet<u32> = BTreeSet::new();
    for r in &sel {
        for s in r.passes.iter().flat_map(|p| p.steps.iter()) {
            if s.list == "specialModList" && s.pattern.is_some() {
                if let Some(l) = s.line.filter(|l| in_range(*l)) {
                    entriesHit.insert(l);
                }
            }
        }
        let got = pcall(|| {
            let (m, e) = parseModPassWith(&lists, &r.line, 1);
            if m.is_some() && e.is_some() {
                parseModPassWith(&lists, &r.line, 2)
            } else {
                (m, e)
            }
        });
        let want = oracle_mods(r);
        let same = match (&got, &want) {
            (Ok((m, e)), w) => {
                e.as_deref() == r.extra.as_deref()
                    && match (m, w) {
                        (Some(a), Some(b)) => mods_eq(b, a),
                        (None, None) => true,
                        _ => false,
                    }
            }
            _ => false,
        };
        if same {
            ok += 1;
        } else {
            nbad += 1;
            bad.push(format!(
                "L1 {:?}: PoB {:?} / {:?}; ours {:?}",
                r.line,
                want.as_deref().map(show),
                r.extra,
                got.map(|(m, e)| (m.as_deref().map(show), e))
            ));
        }
    }
    eprintln!(
        "L1: {} lines whose parse used specialModList {}-{} (stride {stride}; {} distinct entries hit): \
         {ok} identical, {nbad} differ, {:.1}s",
        sel.len(),
        RANGE.0,
        RANGE.1,
        entriesHit.len(),
        t.elapsed().as_secs_f64()
    );
    // coverage (HYG2/V2); MPFIX owns the selection and raises the floors
    if stride == 1 {
        assert!(
            sel.len() >= MP5_MIN_L1.0 && entriesHit.len() >= MP5_MIN_L1.1,
            "{} L1 lines / {} entries hit, expected at least {MP5_MIN_L1:?}",
            sel.len(),
            entriesHit.len()
        );
    }
}

/// `data/dump/parse_oracle.jsonl` record count.
const PARSE_ORACLE_RECORDS: usize = 80_993;
/// `(L1 lines checked, distinct entries hit)` at stride 1; measured by HYG2 on rust-port 746b103.
const MP5_MIN_L1: (usize, usize) = (1974, 541);
