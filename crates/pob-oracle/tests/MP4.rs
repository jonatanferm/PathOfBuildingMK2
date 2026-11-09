//! Unit MP4 acceptance (calc doc §11.1, row MP4: specialModList entries of ModParser.lua
//! 3084-4036, `pob-modparser/src/special_b.rs`).
//!
//! * `list_shape`: `special_b::entries()` holds one entry per literal key of ModParser.lua
//!   3084-4036, in source order, each with its defining line (508 entries, no duplicate keys
//!   in the range); building them hits no `unported!` site.
//! * `mp4_oracle` (PoB booted under LuaJIT with the JIT off, ModParser.lua instrumented in
//!   memory to hand out a copy of its literal `specialModList` table before the keystone and
//!   anchoring loops of 5943-5953):
//!   - every key of the range is a key of PoB's table and has the same kind (table/function);
//!   - every static entry equals PoB's mod list (mods compared field by field, tags and values
//!     included);
//!   - every closure is called in the parse driver's call shape
//!     `f(tonumber(cap[1]), unpack(cap))` (ModParser.lua:6662-6663) with sample captures: every
//!     distinct capture tuple PoB's own scans produced for that pattern in
//!     `data/dump/parse_oracle.jsonl`, plus synthetic captures derived from the pattern's
//!     capture classes (including inputs on which the Lua raises or returns nil). Results must
//!     be equal; when the Lua raises, the Rust must raise the same message;
//!   - L1 for this range: every parse-oracle line whose parse took a specialModList entry of
//!     3084-4036 (and no jewel / cluster / unsupported entry and no specialModList entry of
//!     another unit's range) is parsed by MP0's driver, the cache tier bypassed, over
//!     `complete_special_mod_list(special_b::entries())` and the merged MP1/MP2 lists, and must
//!     give PoB's mods and remainder. The entries of the range that no oracle line hits are
//!     listed.
//!
//! Inputs: `data/dump` and the PoB checkout; missing inputs skip (fail with
//! `POB_ORACLE_REQUIRE=1` or `CI`). `MP4_L1_STRIDE=n` checks every n-th L1 record.
#![allow(non_snake_case)]
#![allow(clippy::type_complexity, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::PathBuf;
use std::sync::Once;

use pob_gamedata::{Mod, Value};
use pob_lua::LuaError;
use pob_modparser::generated_lists::complete_special_mod_list;
use pob_modparser::scan::{parseModPassWith, ParserLists};
use pob_modparser::special_b;
use pob_modparser::types::{Caps, PatternEntry, PatternList, SpecialVal};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};
use serde_json::Value as J;

const RANGE: (u32, u32) = (3084, 4036);

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

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
/// order of first appearance, each with the line of its last definition and whether its value
/// is a function.
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<(String, u32, bool)> {
    let mut by_key: BTreeMap<String, (u32, bool)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for ln in a..=b {
        let s = src[ln - 1];
        let indent = &s[..s.len() - s.trim_start().len()];
        if !(indent == "\t" || (!indent.is_empty() && indent.bytes().all(|b| b == b' '))) {
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
        let after = body[end + 1..].trim_start();
        assert!(after.starts_with(']'), "ModParser.lua:{ln}");
        let val = after[1..]
            .trim_start()
            .strip_prefix('=')
            .unwrap()
            .trim_start();
        let isFn = val.starts_with("function");
        if by_key.insert(key.to_owned(), (ln as u32, isFn)).is_none() {
            order.push(key.to_owned());
        }
    }
    order
        .into_iter()
        .map(|k| {
            let (l, f) = by_key[&k];
            (k, l, f)
        })
        .collect()
}

fn is_fn(v: &SpecialVal) -> bool {
    !matches!(v, SpecialVal::Mods(_))
}

// ---------------------------------------------------------------------------------------------
// list_shape
// ---------------------------------------------------------------------------------------------

#[test]
fn list_shape() {
    let Some(src) = modparser_src() else {
        return skip("PoB checkout not found");
    };
    if init_data().is_none() {
        return skip("data/dump not found");
    }
    let src: Vec<&str> = src.lines().collect();
    pob_gamedata::stub::stub_reset();
    let ours = special_b::entries();
    let hits: Vec<_> = pob_gamedata::stub::stub_report()
        .into_iter()
        .filter(|(s, _)| s.starts_with("Modules/ModParser.lua"))
        .collect();
    assert!(hits.is_empty(), "unported sites hit: {hits:?}");
    let got: Vec<(&str, u32, bool)> = ours
        .iter()
        .map(|e| (e.pat, e.lua_line, is_fn(&e.val)))
        .collect();
    assert!(
        got.windows(2).all(|w| w[0].1 < w[1].1),
        "entries not in source order"
    );
    let mut lit = literal_keys(&src, RANGE.0 as usize, RANGE.1 as usize);
    lit.sort_by_key(|(_, l, _)| *l);
    let lit: Vec<(&str, u32, bool)> = lit.iter().map(|(k, l, f)| (k.as_str(), *l, *f)).collect();
    assert_eq!(got.len(), lit.len(), "entry count");
    for (g, w) in got.iter().zip(&lit) {
        assert_eq!(g, w, "entry differs from ModParser.lua");
    }
    // no key of the range is redefined elsewhere in the literal constructor (2130-5942)
    let all = literal_keys(&src, 2130, 5942);
    let mine: BTreeSet<&str> = got.iter().map(|g| g.0).collect();
    for (k, l, _) in &all {
        if mine.contains(k.as_str()) {
            assert!(
                (RANGE.0..=RANGE.1).contains(l),
                "{k:?}: last defined at ModParser.lua:{l}, outside the range"
            );
        }
    }
    let nFn = got.iter().filter(|g| g.2).count();
    eprintln!(
        "special_b: {} entries ({} static, {nFn} closures), keys/lines/kinds equal ModParser.lua {}-{}",
        got.len(),
        got.len() - nFn,
        RANGE.0,
        RANGE.1
    );
    assert_eq!(got.len(), 508);
}

// ---------------------------------------------------------------------------------------------
// LuaJIT side
// ---------------------------------------------------------------------------------------------

/// Installed before PoB boots: ModParser.lua (loaded through `loadfile` by `LoadModule`) gets a
/// shallow copy of the literal `specialModList` stored in `__mp4_literal` at the start of line
/// 5943 (before the keystone entries and the anchoring). Line numbers are unchanged.
const PRE: &str = r#"
local orig_loadfile = loadfile
loadfile = function(path, ...)
    if type(path) == "string" and path:match("Modules/ModParser%.lua$") then
        local orig_loadstring = loadstring
        local hit = false
        loadstring = function(src, name)
            loadstring = orig_loadstring
            local n
            src, n = src:gsub("\nfor _, name in ipairs%(data%.keystones%) do\n", "\n__mp4_literal = { } for k, v in pairs(specialModList) do __mp4_literal[k] = v end for _, name in ipairs(data.keystones) do\n", 1)
            hit = n == 1
            return orig_loadstring(src, name)
        end
        local f, e = orig_loadfile(path, ...)
        loadstring = orig_loadstring
        assert(hit, "MP4: ModParser.lua instrumentation point (keystone loop) not found")
        return f, e
    end
    return orig_loadfile(path, ...)
end
"#;

/// The JSON encoder (the pob-dump walker's shape for mods: a table whose keys are exactly 1..n
/// is an array; any other table an object whose 1..n part is `$seq`) and the call shape.
const LUA_LIB: &str = r##"
local fmt, t_sort, t_concat = string.format, table.sort, table.concat
local function estr(s)
    return '"' .. s:gsub('[%c"\\]', function(c) return fmt("\\u%04x", c:byte()) end) .. '"'
end
local enc
enc = function(v)
    local t = type(v)
    if t == "string" then return estr(v)
    elseif t == "number" then
        if v ~= v then return '{"$float":"nan"}' end
        if v == math.huge then return '{"$float":"inf"}' end
        if v == -math.huge then return '{"$float":"-inf"}' end
        if v == 0 and 1 / v < 0 then return '{"$float":"-0"}' end
        if v == math.floor(v) and math.abs(v) < 2^53 then return fmt("%d", v) end
        return fmt("%.17g", v)
    elseif t == "boolean" then return tostring(v)
    elseif t == "nil" then return "null"
    elseif t == "table" then
        local n = 0
        while rawget(v, n + 1) ~= nil do n = n + 1 end
        local cnt = 0
        for _ in pairs(v) do cnt = cnt + 1 end
        if n > 0 and cnt == n then
            local out = {}
            for i = 1, n do out[i] = enc(v[i]) end
            return "[" .. t_concat(out, ",") .. "]"
        end
        local keys = {}
        for k in pairs(v) do
            if not (type(k) == "number" and k >= 1 and k <= n and k == math.floor(k)) then
                keys[#keys + 1] = k
            end
        end
        t_sort(keys, function(a, b) return tostring(a) < tostring(b) end)
        local out = {}
        for _, k in ipairs(keys) do
            local ks = type(k) == "number" and ("#" .. fmt("%.17g", k)) or tostring(k)
            out[#out + 1] = estr(ks) .. ":" .. enc(v[k])
        end
        if n > 0 then
            local seq = {}
            for i = 1, n do seq[i] = enc(v[i]) end
            out[#out + 1] = '"$seq":[' .. t_concat(seq, ",") .. "]"
        end
        return "{" .. t_concat(out, ",") .. "}"
    else
        return estr("$" .. t)
    end
end
local function call(f, cap)
    return f(tonumber(cap[1]), unpack(cap))
end
-- { key = { kind = "table", v = … } | { kind = "function", calls = { { ok, v | err } } } | { kind = "absent" } }
function __mp4_run(keys, samples)
    local out = {}
    for _, k in ipairs(keys) do
        local v = __mp4_literal[k]
        if type(v) == "function" then
            local calls = {}
            for i, cap in ipairs(samples[k] or {}) do
                local ok, r = pcall(call, v, cap)
                if ok then
                    calls[i] = { ok = true, v = r == nil and "$nil" or r }
                else
                    calls[i] = { ok = false, err = tostring(r) }
                end
            end
            out[k] = { kind = "function", calls = calls }
        elseif v == nil then
            out[k] = { kind = "absent" }
        else
            out[k] = { kind = type(v), v = v }
        end
    end
    local n = 0
    for _ in pairs(__mp4_literal) do n = n + 1 end
    out["$size"] = n
    return enc(out)
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
            _ => {
                let _ = write!(o, "\\{b}");
            }
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

/// Sample values for one capture group.
fn samples_for(group: &str) -> Vec<&'static str> {
    match group {
        "%d+" | "%d" => vec!["1", "10", "250"],
        "[%d%.]+" => vec!["1.5", "25", ".", "1.2.3"],
        "[%+%-]%d+" => vec!["+1", "-2", "+30"],
        "%a+" => vec![
            "fire",
            "cold",
            "lightning",
            "chaos",
            "physical",
            "elemental",
            "bleeding",
            "ignite",
            "poison",
            "level",
            "quality",
            "x",
        ],
        "%w+" => vec!["fire", "bow", "1", "2x"],
        "%D+" => vec!["fire", "cold", "x y"],
        ".+" | ".-" => vec![
            "fireball",
            "frostbite",
            "fire",
            "spell skill",
            "searching",
            "a1",
            "blue",
        ],
        "[%a%s]+" => vec!["gloves", "body armour", "rings"],
        "[%a%- ]*" => vec!["", "fire", "non-vaal", "support"],
        _ => vec!["x", "1"],
    }
}

/// Synthetic capture tuples: the product of every group's samples (last group fastest),
/// at most `cap` tuples.
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
    #[serde(default)]
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

/// Known parse-level differences of this range (`docs/port/known_diffs.toml`, calc doc §6,
/// CCR-0006 §7): entries whose tag field holds a digits-only capture string in Lua and
/// `tonumber` of it in Rust. (ModParser.lua line, tag field.)
const KNOWN_TAG_STR: [(u32, &str); 1] = [(3535, "div")]; // MP4-1

/// Applies [`KNOWN_TAG_STR`] to PoB's JSON: the digits-only string becomes the number.
/// Returns how many fields it rewrote.
fn known_diffs(v: &mut J, line: u32) -> usize {
    fn walk(v: &mut J, fields: &[&str]) -> usize {
        let mut n = 0;
        match v {
            J::Object(o) => {
                let isTag = o.get("type").is_some_and(J::is_string) && !o.contains_key("name");
                for (k, x) in o.iter_mut() {
                    if isTag && fields.contains(&k.as_str()) {
                        if let J::String(s) = x {
                            if !s.is_empty() && s.bytes().all(|b| b.is_ascii_digit()) {
                                *x = J::from(s.parse::<f64>().unwrap());
                                n += 1;
                                continue;
                            }
                        }
                    }
                    n += walk(x, fields);
                }
            }
            J::Array(a) => {
                for x in a {
                    n += walk(x, fields);
                }
            }
            _ => {}
        }
        n
    }
    let fields: Vec<&str> = KNOWN_TAG_STR
        .iter()
        .filter(|(l, _)| *l == line)
        .map(|(_, f)| *f)
        .collect();
    if fields.is_empty() {
        return 0;
    }
    walk(v, &fields)
}

/// A mod list in the walker's JSON shape (`[...]`, `{}` for an empty list, or `{"$seq": …}`),
/// with the known differences of the entry at `line` applied.
fn mods_from_json(v: &J, what: &str, line: u32, nKnown: &mut usize) -> Vec<Mod> {
    let mut seq = match v {
        J::Object(o) => {
            assert!(o.keys().all(|k| k == "$seq"), "{what}: mods {v}");
            o.get("$seq").cloned().unwrap_or(J::Array(Vec::new()))
        }
        other => other.clone(),
    };
    *nKnown += known_diffs(&mut seq, line);
    serde_json::from_value::<Vec<Mod>>(seq).unwrap_or_else(|e| panic!("{what}: mods {v}: {e}"))
}

/// PoB's mods for a parse-oracle line; `line` is the specialModList entry its parse took.
fn oracle_mods(r: &Record, line: u32, nKnown: &mut usize) -> Option<Vec<Mod>> {
    r.mods
        .as_ref()
        .map(|v| mods_from_json(v, &r.line, line, nKnown))
}

fn num_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
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
}

fn mods_eq(a: Option<&Vec<Mod>>, b: Option<&Vec<Mod>>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| mod_eq(p, q)),
        (None, None) => true,
        _ => false,
    }
}

fn rust_call(
    f: &dyn Fn(&Caps) -> Option<Vec<Mod>>,
    cap: &[String],
) -> Result<Option<Vec<Mod>>, String> {
    let mut caps = Caps::default();
    for (i, c) in cap.iter().take(5).enumerate() {
        caps.cap[i] = Some(c.as_str());
    }
    caps.num = cap.first().and_then(|c| pob_lua::tonumber(c));
    pcall(|| f(&caps))
}

/// The pattern of an oracle step without the `^…$` anchoring of 5949-5953.
fn unanchor(p: &str) -> &str {
    p.strip_prefix('^')
        .and_then(|p| p.strip_suffix('$'))
        .unwrap_or(p)
}

fn in_range(l: Option<u32>) -> bool {
    l.is_some_and(|l| (RANGE.0..=RANGE.1).contains(&l))
}

// ---------------------------------------------------------------------------------------------
// mp4_oracle
// ---------------------------------------------------------------------------------------------

#[test]
fn mp4_oracle() {
    let Some(dir) = init_data() else {
        return skip("data/dump not found");
    };
    let Some(pob_src) = find_pob_src() else {
        return skip("PoB checkout not found");
    };
    let records = load_oracle(&dir);
    if records.is_none() {
        eprintln!("parse_oracle.jsonl not found: synthetic captures only, no L1");
    }
    let ours = special_b::entries();
    let keys: BTreeSet<&'static str> = ours.iter().map(|e| e.pat).collect();

    // -- sample captures per closure
    let mut samples: BTreeMap<&'static str, Vec<Vec<String>>> = BTreeMap::new();
    let mut from_oracle = 0usize;
    if let Some(recs) = &records {
        let mut seen: BTreeMap<&'static str, BTreeSet<Vec<String>>> = BTreeMap::new();
        for r in recs {
            for p in &r.passes {
                for st in &p.steps {
                    if st.list != "specialModList" || !in_range(st.line) {
                        continue;
                    }
                    let Some(pat) = st.pattern.as_deref() else {
                        continue;
                    };
                    let Some(&k) = keys.get(unanchor(pat)) else {
                        continue;
                    };
                    let s = seen.entry(k).or_default();
                    if s.len() < 40 {
                        s.insert(st.caps.iter().map(cap_text).collect());
                    }
                }
            }
        }
        for e in &ours {
            if let Some(s) = seen.remove(e.pat) {
                if is_fn(&e.val) {
                    from_oracle += 1;
                    samples.entry(e.pat).or_default().extend(s);
                }
            }
        }
    }
    for e in &ours {
        if !is_fn(&e.val) {
            continue;
        }
        let v = samples.entry(e.pat).or_default();
        for s in synthetic(e.pat, 60) {
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
    for e in &ours {
        code.push_str(&lua_lit(e.pat));
        code.push_str(",\n");
    }
    code.push_str("}\nlocal S = {\n");
    for (k, tuples) in &samples {
        let _ = write!(code, "[{}] = {{", lua_lit(k));
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
    code.push_str("}\nreturn __mp4_run(K, S)\n");
    let out: J = serde_json::from_str(&lua.eval(&code)).expect("lua json");
    eprintln!(
        "PoB's literal specialModList has {} keys",
        out["$size"].as_u64().unwrap_or(0)
    );

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // -- compare every entry
    let mut bad: Vec<String> = Vec::new();
    let (mut nStatic, mut nFn, mut nCalls, mut nErrCalls, mut nNilCalls) =
        (0usize, 0usize, 0usize, 0usize, 0usize);
    let mut minCalls = usize::MAX;
    let mut nKnown = 0usize;
    for e in &ours {
        let pat = e.pat;
        let l = &out[pat];
        let kind = l["kind"].as_str().unwrap_or("?");
        match &e.val {
            SpecialVal::Mods(m) => {
                nStatic += 1;
                if kind != "table" {
                    bad.push(format!("{pat:?}: PoB has a {kind}, ours is a table"));
                    continue;
                }
                let want = Some(mods_from_json(&l["v"], pat, e.lua_line, &mut nKnown));
                let got = Some(m.clone());
                if !mods_eq(want.as_ref(), got.as_ref()) {
                    bad.push(format!("{pat:?}: PoB {want:?} / ours {got:?}"));
                }
            }
            SpecialVal::Fn(_) | SpecialVal::Dyn(_) => {
                nFn += 1;
                if kind != "function" {
                    bad.push(format!("{pat:?}: PoB has a {kind}, ours is a closure"));
                    continue;
                }
                let f: Box<dyn Fn(&Caps) -> Option<Vec<Mod>>> = match &e.val {
                    SpecialVal::Fn(f) => {
                        let f = *f;
                        Box::new(move |c: &Caps| f(c))
                    }
                    SpecialVal::Dyn(f) => {
                        let f = f.clone();
                        Box::new(move |c: &Caps| f(c))
                    }
                    SpecialVal::Mods(_) => unreachable!(),
                };
                let tuples = &samples[pat];
                let calls = l["calls"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                assert_eq!(calls.len(), tuples.len(), "{pat:?}: call count");
                minCalls = minCalls.min(calls.len());
                for (cap, c) in tuples.iter().zip(calls) {
                    nCalls += 1;
                    let ours = rust_call(&*f, cap);
                    match (c["ok"].as_bool().unwrap(), ours) {
                        (true, Ok(got)) => {
                            let want = if c["v"] == "$nil" {
                                nNilCalls += 1;
                                None
                            } else {
                                Some(mods_from_json(&c["v"], pat, e.lua_line, &mut nKnown))
                            };
                            if !mods_eq(want.as_ref(), got.as_ref()) {
                                bad.push(format!(
                                    "{pat:?} caps {cap:?}: PoB {want:?} / ours {got:?}"
                                ));
                            }
                        }
                        (false, Err(err)) => {
                            nErrCalls += 1;
                            let le = c["err"].as_str().unwrap();
                            if strip_pos(le) != err {
                                bad.push(format!(
                                    "{pat:?} caps {cap:?}: PoB raises {le:?} / ours {err:?}"
                                ));
                            }
                        }
                        (true, Err(err)) => bad.push(format!(
                            "{pat:?} caps {cap:?}: PoB returns {} / ours raises {err:?}",
                            c["v"]
                        )),
                        (false, Ok(got)) => bad.push(format!(
                            "{pat:?} caps {cap:?}: PoB raises {} / ours {got:?}",
                            c["err"]
                        )),
                    }
                }
            }
        }
    }
    eprintln!(
        "entries: {nStatic} static mod lists, {nFn} closures; {nCalls} closure calls \
         ({nErrCalls} raise in both, {nNilCalls} return nil in both), >= {minCalls} per closure; \
         {from_oracle} closures have captures recorded by PoB's own scans; \
         {nKnown} known-diff fields (MP4-1) normalised"
    );
    assert_eq!(nStatic + nFn, 508, "entry count");
    assert!(minCalls > 0, "a closure was never called");

    // -- L1 over the lines whose parse used an entry of this range
    if let Some(recs) = &records {
        l1(recs, &ours, &mut bad);
    }
    std::panic::set_hook(hook);
    for b in bad.iter().take(60) {
        eprintln!("MISMATCH {b}");
    }
    assert!(bad.is_empty(), "{} mismatches", bad.len());
}

fn l1(recs: &[Record], ours: &[PatternEntry<SpecialVal>], bad: &mut Vec<String>) {
    let special: &'static PatternList<SpecialVal> =
        Box::leak(Box::new(complete_special_mod_list(special_b::entries())));
    let lists = ParserLists {
        specialModList: special,
        ..ParserLists::statics()
    };
    let stride: usize = std::env::var("MP4_L1_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let other_units = ["jewelFuncList", "clusterJewelSkills", "unsupportedModList"];
    let sel: Vec<&Record> = recs
        .iter()
        .filter(|r| {
            let steps = || r.passes.iter().flat_map(|p| p.steps.iter());
            steps().any(|s| s.list == "specialModList" && s.pattern.is_some() && in_range(s.line))
                && !steps().any(|s| {
                    s.exact
                        || (other_units.contains(&s.list.as_str()) && s.pattern.is_some())
                        || (s.list == "specialModList"
                        && s.pattern.is_some()
                        && !in_range(s.line)
                        // the generated entries (keystones 5943-5948, skill loops 6124-6147)
                        // are MP0's and present in `special`
                        && !s.line.is_some_and(|l| (5943..=6150).contains(&l)))
                })
        })
        .step_by(stride)
        .collect();
    let mut hit: BTreeSet<&str> = BTreeSet::new();
    let mut nKnown = 0usize;
    let t = std::time::Instant::now();
    let (mut ok, mut nbad) = (0usize, 0usize);
    for r in &sel {
        let mut took = 0u32;
        for s in r.passes.iter().flat_map(|p| p.steps.iter()) {
            if s.list == "specialModList" && in_range(s.line) {
                if let Some(p) = &s.pattern {
                    hit.insert(unanchor(p));
                    took = s.line.unwrap();
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
        let want = oracle_mods(r, took, &mut nKnown);
        match got {
            Ok((m, e))
                if mods_eq(m.as_ref(), want.as_ref()) && e.as_deref() == r.extra.as_deref() =>
            {
                ok += 1;
            }
            other => {
                nbad += 1;
                bad.push(format!(
                    "L1 {:?}: PoB {:?} / {:?}; ours {:?}",
                    r.line, want, r.extra, other
                ));
            }
        }
    }
    let unhit: Vec<(u32, &str)> = ours
        .iter()
        .filter(|e| !hit.contains(e.pat))
        .map(|e| (e.lua_line, e.pat))
        .collect();
    eprintln!(
        "L1: {} parse-oracle lines whose parse took a specialModList entry of {}-{} (stride {stride}): \
         {ok} identical, {nbad} differ ({nKnown} known-diff fields normalised), {:.1}s; \
         {} of {} entries hit, {} not hit by any oracle line",
        sel.len(),
        RANGE.0,
        RANGE.1,
        t.elapsed().as_secs_f64(),
        ours.len() - unhit.len(),
        ours.len(),
        unhit.len()
    );
    for (l, p) in &unhit {
        eprintln!("  not hit: ModParser.lua:{l} {p:?}");
    }
}
