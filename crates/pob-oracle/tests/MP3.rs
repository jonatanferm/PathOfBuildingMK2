//! Unit MP3 acceptance (calc doc §11.1, row MP3: specialModList, ModParser.lua 2130-3083).
//!
//! * `list_shape`: `special_a::entries()` are the literal keys of ModParser.lua 2130-3083 in
//!   source order, each with its defining line; an entry is a closure exactly when the Lua value
//!   is a `function` (204 closures, 209 static tables); building the entries hits no
//!   `unported!` site.
//! * `mp3_oracle` (PoB booted under LuaJIT with the JIT off, ModParser.lua instrumented in
//!   memory to hand out its finished `specialModList`, whose keys are wrapped in `^…$`):
//!   - every static entry equals PoB's mod list (names, types, values, flags, sources, tags);
//!   - every closure is called in the driver's call shape `f(tonumber(cap[1]), unpack(cap))`
//!     (ModParser.lua:6662-6663) with sample captures: every distinct capture tuple PoB's own
//!     scans recorded for that pattern in `data/dump/parse_oracle.jsonl`, plus synthetic tuples
//!     built from the pattern's capture classes. The Lua result and the Rust result must be
//!     equal; when the Lua raises, the Rust must raise the same message. Jewel closures
//!     (`ExtraJewelFunc`, 2622-2627 and 2630-2634) are compared as data: the same source range,
//!     `type` and `radius` (the upvalues are not dumped; MP6's `whole_parser_l1` checks the
//!     captured `args` against the oracle's captures);
//!   - L1 for this range: every parse-oracle line whose parse used a specialModList entry of
//!     2130-3083 (and no other unit's specialModList / jewel / cluster / unsupported entry) is
//!     parsed by `parseModNative` (MP0's driver over the merged lists) and must give PoB's mods
//!     and remainder;
//!   - coverage: the entries no parse-oracle line reaches are listed (calc doc §9.4 L1).
//!
//! Known parse-level differences (calc doc §6, `docs/port/known_diffs.toml` MP3-1): a tag field
//! that holds a digits-only capture string in Lua (`div`, `limit`) is `tonumber` of it in Rust;
//! the comparison accepts that difference (and counts it) only when the string is the number's
//! own text.
//!
//! Inputs: `data/dump` and the PoB checkout; missing inputs fail
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips).
#![allow(non_snake_case)]
#![allow(clippy::type_complexity, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Once;

use pob_gamedata::{Mod, Tag, Value};
use pob_lua::LuaError;
use pob_modparser::special_a;
use pob_modparser::types::{Caps, PatternEntry, SpecialVal};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};
use serde_json::Value as J;

const RANGE: (u32, u32) = (2130, 3083);
const N_ENTRIES: usize = 413;
const N_CLOSURES: usize = 204;
/// The entries of the range no line of `parse_oracle.jsonl` reaches (no item, tree node or mod
/// at the pin has that text; calc doc §9.4 L1 "or listed"). Their static values and closures
/// are still checked against PoB's table.
const NOT_IN_ORACLE: [u32; 15] = [
    2183, 2203, 2223, 2228, 2231, 2232, 2233, 2234, 2293, 2400, 2428, 2561, 2564, 2772, 2929,
];

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
/// is a `function`.
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<(String, u32, bool)> {
    let mut by_key: BTreeMap<String, (u32, bool)> = BTreeMap::new();
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
        let after = body[end + 1..].trim_start();
        let after = after
            .strip_prefix(']')
            .unwrap_or_else(|| panic!("ModParser.lua:{ln}"))
            .trim_start();
        let after = after
            .strip_prefix('=')
            .unwrap_or_else(|| panic!("ModParser.lua:{ln}"))
            .trim_start();
        let is_fn = after.starts_with("function");
        if by_key.insert(key.to_owned(), (ln as u32, is_fn)).is_none() {
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

fn entries() -> &'static [PatternEntry<SpecialVal>] {
    static E: std::sync::OnceLock<Vec<PatternEntry<SpecialVal>>> = std::sync::OnceLock::new();
    E.get_or_init(special_a::entries)
}

// ---------------------------------------------------------------------------------------------
// list_shape
// ---------------------------------------------------------------------------------------------

#[test]
fn list_shape() {
    let Some(src) = pob_testpaths::require(modparser_src(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    // the closures that read `data` need it; the static entries do not
    let _ = init_data();
    let src: Vec<&str> = src.lines().collect();
    pob_gamedata::stub::stub_reset();
    let ours: Vec<(&str, u32, bool)> = special_a::entries()
        .iter()
        .map(|e| (e.pat, e.lua_line, !matches!(e.val, SpecialVal::Mods(_))))
        .collect();
    let hits: Vec<_> = pob_gamedata::stub::stub_report()
        .into_iter()
        .filter(|(s, _)| s.starts_with("Modules/ModParser.lua"))
        .collect();
    assert!(hits.is_empty(), "unported sites hit: {hits:?}");
    assert_eq!(ours.len(), N_ENTRIES, "entry count");
    assert!(
        ours.windows(2).all(|w| w[0].1 < w[1].1),
        "entries not in source order"
    );
    let mut lit = literal_keys(&src, RANGE.0 as usize, RANGE.1 as usize);
    lit.sort_by_key(|(_, l, _)| *l);
    let lit: Vec<(&str, u32, bool)> = lit.iter().map(|(k, l, f)| (k.as_str(), *l, *f)).collect();
    assert_eq!(ours, lit, "keys/lines/kinds differ from ModParser.lua");
    // no key of the range is redefined later in the constructor (Lua keeps the last value)
    let later: BTreeSet<String> = literal_keys(&src, RANGE.1 as usize + 1, 5942)
        .into_iter()
        .map(|(k, _, _)| k)
        .collect();
    let redefined: Vec<&str> = ours
        .iter()
        .map(|(k, _, _)| *k)
        .filter(|k| later.contains(*k))
        .collect();
    assert!(
        redefined.is_empty(),
        "keys redefined after 3083: {redefined:?}"
    );
    let nFn = ours.iter().filter(|(_, _, f)| *f).count();
    assert_eq!(nFn, N_CLOSURES, "closure count");
    eprintln!(
        "special_a: {} entries ({} closures, {} static), keys/order/kinds equal ModParser.lua {}-{}",
        ours.len(),
        nFn,
        ours.len() - nFn,
        RANGE.0,
        RANGE.1
    );
}

// ---------------------------------------------------------------------------------------------
// LuaJIT side
// ---------------------------------------------------------------------------------------------

/// Installed before PoB boots: ModParser.lua (loaded through `loadfile` by `LoadModule`) gets
/// ` __mp3_special = specialModList` appended to its `local cache = { }` line (6994), after the
/// keystone / anchoring / skill statements have finished the table. Line numbers are unchanged.
const PRE: &str = r#"
local orig_loadfile = loadfile
loadfile = function(path, ...)
    if type(path) == "string" and path:match("Modules/ModParser%.lua$") then
        local orig_loadstring = loadstring
        local hit = false
        loadstring = function(src, name)
            loadstring = orig_loadstring
            local n
            src, n = src:gsub("\nlocal cache = { }\n", "\nlocal cache = { } __mp3_special = specialModList\n", 1)
            hit = n == 1
            return orig_loadstring(src, name)
        end
        local f, e = orig_loadfile(path, ...)
        loadstring = orig_loadstring
        assert(hit, "MP3: ModParser.lua instrumentation point `local cache = { }` not found")
        return f, e
    end
    return orig_loadfile(path, ...)
end
"#;

/// The JSON encoder (pob-dump's table encoding: arrays for `1..n` tables, else objects with
/// string keys, `$seq` and `$num`; `$float` for non-finite numbers and `-0`; functions as
/// `{"$fn": "f", "src": "?"}`) and the call shape (Lua side).
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
    elseif t == "function" then
        -- the safe mlua state has no debug library: the closure's source range comes from the
        -- parse oracle (filled in on the Rust side)
        return '{"$fn":"f","src":"?"}'
    elseif t == "table" then
        local n = 0
        while v[n + 1] ~= nil do n = n + 1 end
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
            elseif type(k) == "number" and not (k >= 1 and k <= n and k == math.floor(k)) then nkeys[#nkeys + 1] = k
            elseif type(k) ~= "number" then error("MP3: unsupported key type " .. type(k)) end
        end
        t_sort(skeys)
        t_sort(nkeys)
        local out = {}
        for _, k in ipairs(skeys) do
            local ks = k:sub(1, 1) == "$" and ("$" .. k) or k
            out[#out + 1] = estr(ks) .. ":" .. enc(v[k])
        end
        if n > 0 then
            local seq = {}
            for i = 1, n do seq[i] = enc(v[i]) end
            out[#out + 1] = '"$seq":[' .. t_concat(seq, ",") .. "]"
        end
        if #nkeys > 0 then
            local num = {}
            for _, k in ipairs(nkeys) do num[#num + 1] = estr(tostring(k)) .. ":" .. enc(v[k]) end
            out[#out + 1] = '"$num":{' .. t_concat(num, ",") .. "}"
        end
        return "{" .. t_concat(out, ",") .. "}"
    else
        return estr("$" .. t)
    end
end
local function callSpecial(f, cap) return f(tonumber(cap[1]), unpack(cap)) end
-- keys: { pat, … }; samples: { [pat] = { {cap1, …}, … } }
-- → [ { kind = "missing" } | { kind = "table", v = … } | { kind = "function", calls = [ … ] } ]
function __mp3_run(keys, samples)
    local out = {}
    for i, k in ipairs(keys) do
        local v = __mp3_special["^" .. k .. "$"]
        if v == nil then
            out[i] = '{"kind":"missing"}'
        elseif type(v) == "function" then
            local calls = {}
            for j, cap in ipairs(samples[k] or {}) do
                local ok, r = pcall(callSpecial, v, cap)
                if ok then
                    if r == nil then
                        calls[j] = '{"ok":true,"nil":true}'
                    else
                        local eok, e = pcall(enc, r)
                        calls[j] = eok and ('{"ok":true,"v":' .. e .. '}') or ('{"ok":false,"encode":' .. estr(tostring(e)) .. '}')
                    end
                else
                    calls[j] = '{"ok":false,"err":' .. estr(tostring(r)) .. '}'
                end
            end
            out[i] = '{"kind":"function","calls":[' .. t_concat(calls, ",") .. ']}'
        else
            out[i] = '{"kind":' .. estr(type(v)) .. ',"v":' .. enc(v) .. '}'
        end
    end
    return "[" .. t_concat(out, ",") .. "]"
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

/// Sample values for one capture group (texts the group can match, plus a few edge cases).
fn samples_for(group: &str) -> Vec<&'static str> {
    match group {
        "%d+" => vec!["1", "10", "250"],
        "%d?" => vec!["", "3"],
        "[%d%.]+" => vec!["1.5", "25", "."],
        "%a+" => vec!["fire", "large", "strength", "shield", "right"],
        "%w+" => vec!["life", "mana", "energy"],
        "%-%d+" => vec!["-10", "-1"],
        "%+%d+" => vec!["+10", "+1"],
        "%-?%d+" => vec!["-5", "5"],
        "[%-%+]%d+" | "[%+%-]%d+" => vec!["+10", "-5"],
        ".+" => vec!["fire", "cold", "a quarter", "fireball", "body armour"],
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
    gen: bool,
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

/// A mod list in the dump encoding (an array, `{}` or `{"$seq": …}`).
fn mods_from_json(v: &J, what: &str) -> Result<Vec<Mod>, String> {
    let seq = match v {
        J::Object(o) => {
            if !o.keys().all(|k| k == "$seq") {
                return Err(format!("{what}: not a mod sequence: {v}"));
            }
            o.get("$seq").cloned().unwrap_or(J::Array(Vec::new()))
        }
        other => other.clone(),
    };
    serde_json::from_value::<Vec<Mod>>(seq).map_err(|e| format!("{what}: mods: {e}: {v}"))
}

/// The `src` of every `{"$fn": …}` object of a JSON value, in document order.
fn lua_fns(v: &J, out: &mut Vec<String>) {
    match v {
        J::Object(o) => {
            if o.contains_key("$fn") {
                out.push(o["src"].as_str().unwrap_or_default().to_owned());
                return;
            }
            for x in o.values() {
                lua_fns(x, out);
            }
        }
        J::Array(a) => a.iter().for_each(|x| lua_fns(x, out)),
        _ => {}
    }
}

/// `v` with the `src` of every `{"$fn": …, "src": "?"}` object (the closure-call side, which has
/// no debug library) set to `src`.
fn fill_src(v: &J, src: &str) -> J {
    match v {
        J::Object(o) => {
            let mut o = o.clone();
            if o.contains_key("$fn") && o.get("src").and_then(J::as_str) == Some("?") {
                o.insert("src".to_owned(), J::String(src.to_owned()));
                return J::Object(o);
            }
            for x in o.values_mut() {
                *x = fill_src(x, src);
            }
            J::Object(o)
        }
        J::Array(a) => J::Array(a.iter().map(|x| fill_src(x, src)).collect()),
        other => other.clone(),
    }
}

/// The jewel funcs of a Rust mod list, in order.
fn rust_fns(mods: &[Mod]) -> Vec<&pob_gamedata::JewelFuncSpec> {
    fn walk<'a>(v: &'a Value, out: &mut Vec<&'a pob_gamedata::JewelFuncSpec>) {
        match v {
            Value::JewelFunc(j) => out.push(j),
            Value::Mod(m) => walk(&m.value, out),
            Value::Table(t) => {
                t.array.iter().for_each(|x| walk(x, out));
                t.fields.iter().for_each(|(_, x)| walk(x, out));
            }
            _ => {}
        }
    }
    let mut out = Vec::new();
    for m in mods {
        walk(&m.value, &mut out);
    }
    out
}

// ---------------------------------------------------------------------------------------------
// Comparison
// ---------------------------------------------------------------------------------------------

fn num_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

/// Known diff MP3-1 (calc doc §6): a numeric tag field Lua holds as a capture string is a number
/// in Rust. The dump decoder keeps the string in `Tag.extra`; drop it when it is exactly the
/// number's text (and count it).
fn norm_tag(t: &Tag, quirks: &mut usize) -> Tag {
    let Some(ex) = &t.extra else {
        return t.clone();
    };
    let num_of = |k: &str| -> Option<f64> {
        match k {
            "div" => t.div,
            "limit" => t.limit,
            "percent" => t.percent,
            "globalLimit" => t.globalLimit,
            "base" => t.base,
            "threshold" => t.threshold,
            _ => None,
        }
    };
    let mut fields = Vec::new();
    for (k, v) in &ex.fields {
        if let (Some(n), Value::Str(s)) = (num_of(k.as_str()), v) {
            if s.bytes().all(|b| b.is_ascii_digit()) && pob_lua::tonumber(s) == Some(n) {
                *quirks += 1;
                continue;
            }
        }
        fields.push((*k, v.clone()));
    }
    let mut t2 = t.clone();
    t2.extra = if fields.is_empty() && ex.array.is_empty() {
        None
    } else {
        Some(Box::new(pob_gamedata::ValueTable {
            array: ex.array.clone(),
            fields,
        }))
    };
    t2
}

fn value_eq(a: &Value, b: &Value, q: &mut usize) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => num_eq(*x, *y),
        (Value::Mod(x), Value::Mod(y)) => mod_eq(x, y, q),
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.array.iter().zip(&y.array).all(|(p, r)| value_eq(p, r, q))
                && x.fields.len() == y.fields.len()
                && x.fields.iter().all(|(k, v)| value_eq(v, y.get(*k), q))
        }
        // the closure itself is compared through `lua_fns` / `rust_fns` (upvalues vs args)
        (Value::JewelFunc(x), Value::JewelFunc(y)) => {
            x.key == y.key && x.kind == y.kind && x.r#type == y.r#type && x.radius == y.radius
        }
        _ => a == b,
    }
}

/// `a` is ours, `b` PoB's.
fn mod_eq(a: &Mod, b: &Mod, q: &mut usize) -> bool {
    a.name == b.name
        && a.r#type == b.r#type
        && a.flags == b.flags
        && a.keywordFlags == b.keywordFlags
        && value_eq(&a.value, &b.value, q)
        && a.source == b.source
        && a.extra == b.extra
        && a.tags.len() == b.tags.len()
        && a.tags
            .iter()
            .zip(&b.tags)
            .all(|(x, y)| *x == norm_tag(y, q))
}

fn mods_eq(a: &Option<Vec<Mod>>, b: &Option<Vec<Mod>>, q: &mut usize) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, r)| mod_eq(p, r, q)),
        (None, None) => true,
        _ => false,
    }
}

/// Compare one PoB result (`v`: the dump-encoded mod list) with ours; jewel closures also by
/// closure source range == key (args are checked in MP6).
fn compare_mods(
    ours: &Option<Vec<Mod>>,
    v: Option<&J>,
    what: &str,
    q: &mut usize,
) -> Option<String> {
    let theirs = match v {
        None => None,
        Some(v) => match mods_from_json(v, what) {
            Ok(m) => Some(m),
            Err(e) => return Some(e),
        },
    };
    if !mods_eq(ours, &theirs, q) {
        return Some(format!("{what}: PoB {theirs:?} / ours {ours:?}"));
    }
    if let (Some(o), Some(v)) = (ours, v) {
        let mut lf = Vec::new();
        lua_fns(v, &mut lf);
        let rf = rust_fns(o);
        if lf.len() != rf.len() {
            return Some(format!(
                "{what}: {} Lua closures / {} jewel funcs",
                lf.len(),
                rf.len()
            ));
        }
        for (src, j) in lf.iter().zip(&rf) {
            if src != j.key.as_str() {
                return Some(format!("{what}: closure {src} / key {}", j.key));
            }
        }
    }
    None
}

/// The Rust call in the driver's shape `f(tonumber(cap[1]), unpack(cap))`.
fn rust_call(val: &SpecialVal, cap: &[String]) -> Result<Option<Vec<Mod>>, String> {
    let mut caps = Caps::default();
    for (i, c) in cap.iter().take(5).enumerate() {
        caps.cap[i] = Some(c.as_str());
    }
    caps.num = cap.first().and_then(|c| pob_lua::tonumber(c));
    pcall(|| match val {
        SpecialVal::Fn(f) => f(&caps),
        SpecialVal::Dyn(f) => f(&caps),
        SpecialVal::Mods(m) => Some(m.clone()),
    })
}

// ---------------------------------------------------------------------------------------------
// mp3_oracle
// ---------------------------------------------------------------------------------------------

#[test]
fn mp3_oracle() {
    let Some(dir) = pob_testpaths::require(init_data(), "data/dump not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(pob_src) = pob_testpaths::require(find_pob_src(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
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
    let ents = entries();
    let in_range = |l: u32| (RANGE.0..=RANGE.1).contains(&l);

    // -- sample captures per closure; oracle coverage per entry
    let by_pat: BTreeMap<&'static str, &PatternEntry<SpecialVal>> =
        ents.iter().map(|e| (e.pat, e)).collect();
    let mut samples: BTreeMap<&'static str, Vec<Vec<String>>> = BTreeMap::new();
    let mut hit: BTreeMap<u32, usize> = BTreeMap::new();
    // the source ranges of the closures inside the mods PoB returned, per entry (the parse
    // oracle records them; the closure calls below cannot, see LUA_LIB)
    let mut fn_src: BTreeMap<u32, BTreeSet<String>> = BTreeMap::new();
    let mut nOracleTuples = 0usize;
    if let Some(recs) = &records {
        let mut seen: BTreeMap<&'static str, BTreeSet<Vec<String>>> = BTreeMap::new();
        for r in recs {
            for p in &r.passes {
                for st in &p.steps {
                    if st.list != "specialModList" || st.gen {
                        continue;
                    }
                    let (Some(pat), Some(line)) = (st.pattern.as_deref(), st.line) else {
                        continue;
                    };
                    if !in_range(line) {
                        continue;
                    }
                    let raw = pat
                        .strip_prefix('^')
                        .and_then(|p| p.strip_suffix('$'))
                        .unwrap_or(pat);
                    let e = by_pat.get(raw).unwrap_or_else(|| {
                        panic!("oracle pattern {pat:?} (line {line}) is not an entry")
                    });
                    assert_eq!(e.lua_line, line, "{pat:?}: line");
                    *hit.entry(line).or_default() += 1;
                    if let Some(m) = &r.mods {
                        let mut srcs = Vec::new();
                        lua_fns(m, &mut srcs);
                        fn_src.entry(line).or_default().extend(srcs);
                    }
                    if matches!(e.val, SpecialVal::Mods(_)) {
                        continue;
                    }
                    let s = seen.entry(e.pat).or_default();
                    if s.len() < 40 {
                        s.insert(st.caps.iter().map(cap_text).collect());
                    }
                }
            }
        }
        for (k, s) in seen {
            nOracleTuples += s.len();
            samples.entry(k).or_default().extend(s);
        }
    }
    for e in ents {
        if matches!(e.val, SpecialVal::Mods(_)) {
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
    for e in ents {
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
    code.push_str("}\nreturn __mp3_run(K, S)\n");
    let out: J = serde_json::from_str(&lua.eval(&code)).expect("lua json");
    let out = out.as_array().expect("lua result array");
    assert_eq!(out.len(), ents.len());

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // -- compare every entry
    let mut bad: Vec<String> = Vec::new();
    let mut quirks = 0usize;
    let (mut nStatic, mut nFn, mut nCalls, mut nErrCalls, mut nNil) = (0usize, 0usize, 0, 0, 0);
    let mut minCalls = usize::MAX;
    for (e, l) in ents.iter().zip(out) {
        let kind = l["kind"].as_str().unwrap();
        let what = format!("{}:{:?}", e.lua_line, e.pat);
        if kind == "missing" {
            bad.push(format!("{what}: not a key of PoB's specialModList"));
            continue;
        }
        match &e.val {
            SpecialVal::Mods(m) => {
                nStatic += 1;
                if kind != "table" {
                    bad.push(format!("{what}: PoB has a {kind}, ours is a table"));
                    continue;
                }
                if let Some(d) = compare_mods(&Some(m.clone()), Some(&l["v"]), &what, &mut quirks) {
                    bad.push(d);
                }
            }
            val => {
                nFn += 1;
                if kind != "function" {
                    bad.push(format!("{what}: PoB has a {kind}, ours is a closure"));
                    continue;
                }
                let tuples = &samples[e.pat];
                let calls = l["calls"].as_array().map(Vec::as_slice).unwrap_or(&[]);
                assert_eq!(calls.len(), tuples.len(), "{what}: call count");
                minCalls = minCalls.min(calls.len());
                for (cap, c) in tuples.iter().zip(calls) {
                    nCalls += 1;
                    let wc = format!("{what} caps {cap:?}");
                    let ours = rust_call(val, cap);
                    if let Some(enc) = c.get("encode") {
                        bad.push(format!("{wc}: PoB result not encodable: {enc}"));
                        continue;
                    }
                    match (c["ok"].as_bool().unwrap(), ours) {
                        (true, Ok(m)) => {
                            let v = if c.get("nil").is_some() {
                                nNil += 1;
                                None
                            } else {
                                match fn_src
                                    .get(&e.lua_line)
                                    .map(|s| s.iter().collect::<Vec<_>>())
                                {
                                    Some(s) if s.len() == 1 => Some(fill_src(&c["v"], s[0])),
                                    _ => Some(c["v"].clone()),
                                }
                            };
                            if let Some(d) = compare_mods(&m, v.as_ref(), &wc, &mut quirks) {
                                bad.push(d);
                            }
                        }
                        (false, Err(err)) => {
                            nErrCalls += 1;
                            let le = c["err"].as_str().unwrap();
                            if strip_pos(le) != err {
                                bad.push(format!("{wc}: PoB raises {le:?} / ours {err:?}"));
                            }
                        }
                        (true, Err(err)) => {
                            bad.push(format!(
                                "{wc}: PoB returns {} / ours raises {err:?}",
                                c["v"]
                            ));
                        }
                        (false, Ok(m)) => {
                            bad.push(format!("{wc}: PoB raises {} / ours {m:?}", c["err"]));
                        }
                    }
                }
            }
        }
    }
    eprintln!(
        "entries: {nStatic} static, {nFn} closures; {nCalls} closure calls ({nErrCalls} raise in both, \
         {nNil} return nil in both), >= {minCalls} per closure; {nOracleTuples} capture tuples from PoB's \
         own scans; {quirks} tag fields accepted as known diff MP3-1 (capture string vs number)"
    );
    assert_eq!(nFn, N_CLOSURES, "closure count");
    assert!(minCalls > 0, "a closure was never called");

    // -- coverage: entries no parse-oracle line reaches
    if records.is_some() {
        let unhit: Vec<String> = ents
            .iter()
            .filter(|e| !hit.contains_key(&e.lua_line))
            .map(|e| format!("{} {:?}", e.lua_line, e.pat))
            .collect();
        eprintln!(
            "coverage: {} of {} entries hit by parse-oracle lines; not hit ({}): {unhit:#?}",
            ents.len() - unhit.len(),
            ents.len(),
            unhit.len()
        );
        let unhitLines: Vec<u32> = ents
            .iter()
            .filter(|e| !hit.contains_key(&e.lua_line))
            .map(|e| e.lua_line)
            .collect();
        if unhitLines != NOT_IN_ORACLE {
            bad.push(format!(
                "coverage: entries not hit {unhitLines:?} differ from the listed {NOT_IN_ORACLE:?}"
            ));
        }
    }

    // -- L1 over the lines whose parse used these entries
    if let Some(recs) = &records {
        l1(recs, &mut bad, &mut quirks);
    }
    std::panic::set_hook(hook);
    for b in bad.iter().take(80) {
        eprintln!("MISMATCH {b}");
    }
    assert!(bad.is_empty(), "{} mismatches", bad.len());
    // the known diff MP3-1 (capture string vs number in a tag field) is bounded (HYG2/V2)
    assert!(
        quirks <= MP3_MAX_QUIRKS,
        "{quirks} tag fields accepted as MP3-1, bound {MP3_MAX_QUIRKS}"
    );
}

fn l1(recs: &[Record], bad: &mut Vec<String>, quirks: &mut usize) {
    let other_units = ["jewelFuncList", "clusterJewelSkills", "unsupportedModList"];
    let sel: Vec<&Record> = recs
        .iter()
        .filter(|r| {
            let steps = || r.passes.iter().flat_map(|p| p.steps.iter());
            let special = || steps().filter(|s| s.list == "specialModList" && s.pattern.is_some());
            special().any(|s| !s.gen && s.line.is_some_and(|l| (RANGE.0..=RANGE.1).contains(&l)))
                && special()
                    .all(|s| !s.gen && s.line.is_some_and(|l| (RANGE.0..=RANGE.1).contains(&l)))
                && !steps().any(|s| {
                    s.exact || (other_units.contains(&s.list.as_str()) && s.pattern.is_some())
                })
        })
        .collect();
    let t = std::time::Instant::now();
    let q0 = *quirks;
    let (mut ok, mut nbad) = (0usize, 0usize);
    for r in &sel {
        let got = pcall(|| pob_modparser::parseModNative(&r.line));
        let want = r.mods.as_ref();
        match got {
            Ok((m, e)) => {
                let d = compare_mods(&m, want, &format!("L1 {:?}", r.line), quirks);
                if d.is_none() && e.as_deref() == r.extra.as_deref() {
                    ok += 1;
                } else {
                    nbad += 1;
                    bad.push(d.unwrap_or_else(|| {
                        format!(
                            "L1 {:?}: remainder PoB {:?} / ours {:?}",
                            r.line, r.extra, e
                        )
                    }));
                }
            }
            Err(e) => {
                nbad += 1;
                bad.push(format!("L1 {:?}: ours raises {e:?}", r.line));
            }
        }
    }
    eprintln!(
        "L1: {} parse-oracle lines whose parse used specialModList {}-{}: {ok} identical, {nbad} differ \
         ({} tag fields accepted as MP3-1), {:.1}s",
        sel.len(),
        RANGE.0,
        RANGE.1,
        *quirks - q0,
        t.elapsed().as_secs_f64()
    );
    // coverage (HYG2/V2): the checked share cannot shrink (MPFIX owns the selection)
    assert!(
        sel.len() >= MP3_MIN_L1,
        "{} L1 lines checked, expected at least {MP3_MIN_L1}",
        sel.len()
    );
}

/// `data/dump/parse_oracle.jsonl` record count.
const PARSE_ORACLE_RECORDS: usize = 80_993;
/// L1 lines checked; measured by HYG2 on rust-port 746b103.
const MP3_MIN_L1: usize = 1048;
/// Tag fields accepted as the known diff MP3-1 (captures and L1); measured by HYG2 on rust-port
/// 746b103.
const MP3_MAX_QUIRKS: usize = 82;
