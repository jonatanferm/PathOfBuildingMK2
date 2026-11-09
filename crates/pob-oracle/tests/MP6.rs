//! Unit MP6 acceptance (calc doc §11.1, row MP3-MP6: specialModList, ModParser.lua 4989-5942,
//! `pob-modparser/src/special_d.rs`).
//!
//! * `list_shape`: `special_d::entries()` holds the literal keys of ModParser.lua 4989-5942 in
//!   source order, each with the line of its last definition (Lua constructor semantics for a
//!   duplicated key); an entry is a closure exactly when its Lua value is a `function`; building
//!   the entries and calling every closure on synthetic captures hits no `unported!` site.
//! * `mp6_oracle` (PoB booted under LuaJIT with the JIT off; ModParser.lua instrumented in memory,
//!   line numbers unchanged, to hand out a copy of the literal `specialModList` table before the
//!   keystone loop of 5943 writes into it):
//!   - every static entry of the range equals PoB's table (mods field by field, tags, values);
//!   - every closure is called in the driver's call shape `f(tonumber(cap[1]), unpack(cap))`
//!     (ModParser.lua:6662-6663) with every distinct capture tuple PoB's own scans recorded for it
//!     in `data/dump/parse_oracle.jsonl` plus synthetic captures derived from the pattern's
//!     capture classes (including inputs on which the Lua raises). The Lua result and ours must
//!     be equal (nil, the mods, or the same error message);
//!   - L1 for this range: every parse-oracle record whose parse picked a specialModList entry of
//!     4989-5942 is parsed by MP0's driver (`parseModPassWith`, both passes as `parseMod` runs
//!     them, the ModCache tier bypassed) and must give PoB's mods and remainder. The other
//!     ranges' entries (MP3-MP5) are used when ported, else their keys are rebuilt from the Lua
//!     source with a placeholder value that fails the comparison if it ever wins the scan.
//!   - every entry of the range is hit by >= 1 oracle line, or is listed (printed) as unhit; the
//!     unhit ones are still covered by the direct comparison above.
//! * `whole_parser_l1` (the parser's L1 gate, calc doc §11.1 MP0-MP6): `parseModNative` (the
//!   ModCache tier bypassed) over every one of the 80,993 parse-oracle records must give PoB's
//!   mods and remainder for >= 99.95% of them. Jewel funcs are compared as far as the dump
//!   records them (the closure's source range; see `jewel_to_oracle_shape`), and the accepted
//!   parse-level differences below only at the lines known_diffs.toml lists for them.
//!
//! Accepted parse-level differences (calc doc §6 "Known parse-level differences",
//! docs/port/known_diffs.toml MP6-*): a tag PoB stores at `mod[2]` behind a nil `mod[1]`
//! (`mod(…, nil, {tag})`, 5686/5689/5692; `ipairs` never reaches it) is absent in ours; a numeric
//! tag field holding a digits-only capture string in Lua holds `tonumber` of it in ours; the
//! `{ SkillType.Instant, SkillType.Totem (nil), SkillType.Triggered }` list of 5509 is the list
//! `pairs` sees (its only reader, ModStore.lua:866). Each accepted difference is counted, and
//! the test fails if one occurs at a line not listed for it.
//!
//! Inputs: `data/dump` and the PoB checkout (`pob_testpaths`; missing fails unless
//! `POB_ALLOW_SKIP=1`). `MP6_L1_STRIDE=n` checks every n-th L1 record.
#![allow(non_snake_case)]
#![allow(clippy::type_complexity, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::sync::{Arc, Once};

use pob_gamedata::{n, JewelFuncSpec, Mod, Name, Tag, Value};
use pob_lua::LuaError;
use pob_modparser::jewel_func_list::{callCaptureClosure, jewelFuncListSource};
use pob_modparser::scan::{parseModPassWith, ParserLists};
use pob_modparser::types::{Caps, PatternEntry, PatternList, SpecialVal};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use serde_json::Value as J;

const LO: usize = 4989;
const HI: usize = 5942;
const OTHER_RANGES: [(&str, usize, usize); 3] = [
    ("MP3", 2130, 3083),
    ("MP4", 3084, 4036),
    ("MP5", 4037, 4988),
];
/// Lines whose closures call `mod(…, nil, {tag})` (calc doc §6).
const NIL_HOLE_LINES: [u32; 3] = [5686, 5689, 5692];
/// The `SkillType.Totem` (nil) list.
const COMPACTED_LIST_LINES: [u32; 1] = [5509];
/// Lines whose closures store a digits-only `(%d+)` capture string as a tag `div` (calc doc §6).
const DIGIT_STRING_LINES: [u32; 3] = [5733, 5734, 5741];

fn init_data() -> bool {
    static INIT: Once = Once::new();
    let Some(dir) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump (pob-dump)")
    else {
        return false;
    };
    INIT.call_once(|| {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    });
    true
}

fn modparser_src() -> Option<String> {
    let src = pob_testpaths::require(
        pob_testpaths::pob_src(),
        "PoB checkout (.PathOfBuilding/src)",
    )?;
    Some(std::fs::read_to_string(src.join("Modules/ModParser.lua")).unwrap())
}

/// `["key"] = …` entries at the top level of a table constructor in lines `a..=b`, in source
/// order of first appearance, each with the line of its last definition and whether that
/// definition's value is a `function`.
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<(String, u32, bool)> {
    let mut by_key: BTreeMap<String, (u32, bool)> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for ln in a..=b {
        let s = src[ln - 1];
        // one tab, or (5680) spaces
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
        let is_fn = val.starts_with("function");
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

/// Sample values for one capture group (lowercase: the driver scans `line:lower()`).
fn samples_for(group: &str) -> Vec<&'static str> {
    match group {
        "%d+" | "%d" => vec!["1", "10", "250"],
        "[%d%.]+" => vec!["1.5", "25", ".", "1.2.3"],
        "%-%d+" => vec!["-5", "-1"],
        "%+%d+" => vec!["+5", "+1"],
        "[%+%-]%d" | "[%+%-]%d+" => vec!["+1", "-2", "-10"],
        "%a+" => vec![
            "fire",
            "physical",
            "cold",
            "lightning",
            "chaos",
            "rare",
            "x",
        ],
        "%D+" => vec!["fire", "x"],
        ".+" | ".-" => vec![
            "fireball",
            "flammability skill",
            "xibaqua",
            "arctic armour",
            "a1",
        ],
        "[%a%s]+" | "[%a%s']+" => vec!["gloves", "body armour", "zealot's oath"],
        "m?r?ag?r?i?e?c?" => vec!["", "magic", "rare"],
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

/// The Rust call in the driver's shape: `num = tonumber(cap[1])`, `cap` as captured.
fn rust_call(f: &SpecialVal, cap: &[String]) -> Result<Option<Vec<Mod>>, String> {
    let mut caps = Caps::default();
    for (i, c) in cap.iter().take(5).enumerate() {
        caps.cap[i] = Some(c.as_str());
    }
    caps.num = cap.first().and_then(|c| pob_lua::tonumber(c));
    match f {
        SpecialVal::Fn(f) => pcall(|| f(&caps)),
        SpecialVal::Dyn(f) => pcall(|| f(&caps)),
        SpecialVal::Mods(m) => Ok(Some(m.clone())),
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
// parse_oracle.jsonl
// ---------------------------------------------------------------------------------------------

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

fn load_oracle() -> Option<Vec<Record>> {
    let dir = pob_testpaths::dump_dir()?;
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

fn mods_from_json(v: &J, what: &str) -> Vec<Mod> {
    let seq = match v {
        J::Object(o) if o.is_empty() => J::Array(Vec::new()),
        J::Object(o) => {
            assert!(o.keys().all(|k| k == "$seq"), "{what}: mods {v}");
            o.get("$seq").cloned().unwrap_or(J::Array(Vec::new()))
        }
        other => other.clone(),
    };
    serde_json::from_value::<Vec<Mod>>(seq).unwrap_or_else(|e| panic!("{what}: mods {v}: {e}"))
}

fn oracle_mods(r: &Record) -> Option<Vec<Mod>> {
    r.mods.as_ref().map(|v| mods_from_json(v, &r.line))
}

/// The specialModList step that decided the record (its entry's line), if any.
fn special_line(r: &Record) -> Option<u32> {
    r.passes
        .iter()
        .flat_map(|p| p.steps.iter())
        .find(|s| s.list == "specialModList" && s.pattern.is_some())
        .and_then(|s| s.line)
}

// ---------------------------------------------------------------------------------------------
// Comparison (Lua's mods deserialised in the dump shape vs ours)
// ---------------------------------------------------------------------------------------------

/// Accepted differences seen during one comparison.
#[derive(Default)]
struct Notes {
    nil_hole: usize,
    digit_string: usize,
}

fn num_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan())
}

fn value_eq(a: &Value, b: &Value, n: &mut Notes) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => num_eq(*x, *y),
        (Value::Mod(x), Value::Mod(y)) => mod_eq(x, y, n),
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.array.iter().zip(&y.array).all(|(p, q)| value_eq(p, q, n))
                && x.fields.len() == y.fields.len()
                && x.fields.iter().all(|(k, v)| value_eq(v, y.get(*k), n))
        }
        _ => a == b,
    }
}

/// `lua` (PoB's tag, deserialised) against `ours`. A numeric field PoB holds as a digits-only
/// string is deserialised as the number plus the string in `extra` under the same key.
fn tag_eq(lua: &Tag, ours: &Tag, n: &mut Notes) -> bool {
    if lua == ours {
        return true;
    }
    let Some(ex) = &lua.extra else {
        return false;
    };
    let mut t = lua.clone();
    let mut ex = (**ex).clone();
    let before = ex.fields.len();
    ex.fields.retain(|(_, v)| {
        !matches!(v, Value::Str(s) if s.bytes().all(|c| c.is_ascii_digit()) && !s.is_empty())
    });
    if ex.fields.len() == before {
        return false;
    }
    t.extra = (!ex.fields.is_empty() || !ex.array.is_empty()).then(|| Box::new(ex));
    if t == *ours {
        n.digit_string += 1;
        true
    } else {
        false
    }
}

fn mod_eq(lua: &Mod, ours: &Mod, n: &mut Notes) -> bool {
    if !(lua.name == ours.name
        && lua.r#type == ours.r#type
        && lua.flags == ours.flags
        && lua.keywordFlags == ours.keywordFlags
        && lua.source == ours.source
        && value_eq(&lua.value, &ours.value, n)
        && lua.tags.len() == ours.tags.len()
        && lua
            .tags
            .iter()
            .zip(&ours.tags)
            .all(|(x, y)| tag_eq(x, y, n)))
    {
        return false;
    }
    match (&lua.extra, &ours.extra) {
        (None, None) => true,
        (Some(x), Some(y)) => value_eq(
            &Value::Table(std::sync::Arc::new((**x).clone())),
            &Value::Table(std::sync::Arc::new((**y).clone())),
            n,
        ),
        // `mod[2] = tag` behind a nil `mod[1]`: the deserialiser keeps it as extra "#2"
        (Some(x), None)
            if x.array.is_empty()
                && x.fields
                    .iter()
                    .all(|(k, v)| k.as_str().starts_with('#') && matches!(v, Value::Table(_))) =>
        {
            n.nil_hole += 1;
            true
        }
        _ => false,
    }
}

fn mods_eq(lua: &Option<Vec<Mod>>, ours: &Option<Vec<Mod>>, n: &mut Notes) -> bool {
    match (lua, ours) {
        (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| mod_eq(p, q, n)),
        (None, None) => true,
        _ => false,
    }
}

// ---------------------------------------------------------------------------------------------
// list_shape
// ---------------------------------------------------------------------------------------------

#[test]
fn list_shape() {
    let Some(src) = modparser_src() else {
        return;
    };
    let src: Vec<&str> = src.lines().collect();
    if !init_data() {
        return;
    }
    pob_gamedata::stub::stub_reset();
    let ours = pob_modparser::special_d::entries();
    let keys = literal_keys(&src, LO, HI);
    assert_eq!(ours.len(), keys.len(), "entry count");
    let mut nFn = 0;
    for (e, (k, l, f)) in ours.iter().zip(&keys) {
        assert_eq!(e.pat, k, "order");
        assert_eq!(e.lua_line, *l, "{k:?}: line of the last definition");
        let is_fn = !matches!(e.val, SpecialVal::Mods(_));
        assert_eq!(is_fn, *f, "{k:?} (line {l}): closure vs table");
        assert!(
            !matches!(e.val, SpecialVal::Dyn(_)),
            "{k:?}: no generated entry in this range"
        );
        nFn += usize::from(is_fn);
    }
    // every closure on synthetic captures: no unported! site, no non-Lua panic
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let mut nCalls = 0;
    for e in &ours {
        if let SpecialVal::Mods(_) = e.val {
            continue;
        }
        for cap in synthetic(e.pat, 30) {
            nCalls += 1;
            let _ = rust_call(&e.val, &cap);
        }
    }
    std::panic::set_hook(hook);
    let stubs = pob_gamedata::stub::stub_report();
    let mine: Vec<_> = stubs
        .iter()
        .filter(|(s, _)| {
            s.strip_prefix("Modules/ModParser.lua:")
                .and_then(|r| r.split('-').next())
                .and_then(|a| a.parse::<usize>().ok())
                .is_some_and(|a| (LO..=HI).contains(&a))
        })
        .collect();
    assert!(mine.is_empty(), "unported sites hit: {mine:?}");
    let dups = (LO..=HI)
        .filter(|ln| {
            let s = src[ln - 1];
            let t = s.trim_start();
            let indent = &s[..s.len() - t.len()];
            (indent == "\t" || (!indent.is_empty() && indent.bytes().all(|b| b == b' ')))
                && (t.starts_with("[\"") || t.starts_with("['"))
        })
        .count()
        - keys.len();
    eprintln!(
        "special_d: {} entries ({nFn} closures, {} static), {dups} overwritten duplicate key(s); \
         {nCalls} synthetic closure calls",
        ours.len(),
        ours.len() - nFn
    );
}

// ---------------------------------------------------------------------------------------------
// LuaJIT side
// ---------------------------------------------------------------------------------------------

/// Installed before PoB boots: ModParser.lua (loaded through `loadfile`) gets a copy of the
/// literal `specialModList` taken at the start of line 5943 (before the keystone loop writes
/// into the table). Line numbers are unchanged.
const PRE: &str = r#"
local orig_loadfile = loadfile
loadfile = function(path, ...)
    if type(path) == "string" and path:match("Modules/ModParser%.lua$") then
        local orig_loadstring = loadstring
        local hit = false
        loadstring = function(src, name)
            loadstring = orig_loadstring
            local n
            src, n = src:gsub("\nfor _, name in ipairs%(data%.keystones%) do\n", "\n__mp6_lit = { } for k, v in pairs(specialModList) do __mp6_lit[k] = v end for _, name in ipairs(data.keystones) do\n", 1)
            hit = n == 1
            return orig_loadstring(src, name)
        end
        local f, e = orig_loadfile(path, ...)
        loadstring = orig_loadstring
        assert(hit, "MP6: ModParser.lua instrumentation point (keystone loop) not found")
        return f, e
    end
    return orig_loadfile(path, ...)
end
"#;

/// The JSON encoder (the dump shape of crates/pob-dump/FORMAT.md: `$seq` for the array part
/// `ipairs` sees, `$num` for other numeric keys, `$float` for non-finite numbers and -0) and
/// the runner.
const LUA_LIB: &str = r##"
local fmt, t_sort, t_concat = string.format, table.sort, table.concat
local function estr(s)
    return '"' .. s:gsub('[%c"\\]', function(c) return fmt("\\u%04x", c:byte()) end) .. '"'
end
local compacted = 0
local function encnum(v)
    if v ~= v then return '{"$float":"nan"}' end
    if v == math.huge then return '{"$float":"inf"}' end
    if v == -math.huge then return '{"$float":"-inf"}' end
    if v == 0 and 1 / v < 0 then return '{"$float":"-0"}' end
    if v == math.floor(v) and math.abs(v) < 2^53 then return fmt("%d", v) end
    return fmt("%.17g", v)
end
local enc
enc = function(v, key)
    local t = type(v)
    if t == "string" then return estr(v)
    elseif t == "number" then return encnum(v)
    elseif t == "boolean" then return tostring(v)
    elseif t == "nil" then return "null"
    elseif t == "function" then return '{"$fn":"?"}'
    elseif t ~= "table" then return estr("$" .. t)
    end
    if key == "skillTypeList" then
        -- read with pairs only (ModStore.lua:866): the values pairs sees, in key order
        local ks = { }
        for k in pairs(v) do ks[#ks + 1] = k end
        t_sort(ks)
        local out = { }
        for i, k in ipairs(ks) do
            if k ~= i then compacted = compacted + 1 end
            out[i] = enc(v[k])
        end
        return "[" .. t_concat(out, ",") .. "]"
    end
    local n = 0
    while v[n + 1] ~= nil do n = n + 1 end
    local cnt = 0
    for _ in pairs(v) do cnt = cnt + 1 end
    local seq = { }
    for i = 1, n do seq[i] = enc(v[i]) end
    if n > 0 and cnt == n then return "[" .. t_concat(seq, ",") .. "]" end
    local skeys, nkeys = { }, { }
    for k in pairs(v) do
        if type(k) == "string" then skeys[#skeys + 1] = k
        elseif type(k) == "number" then
            if not (k >= 1 and k <= n and k == math.floor(k)) then nkeys[#nkeys + 1] = k end
        else error("MP6 encoder: key of type " .. type(k))
        end
    end
    t_sort(skeys)
    t_sort(nkeys)
    local out = { }
    if n > 0 then out[#out + 1] = '"$seq":[' .. t_concat(seq, ",") .. "]" end
    for _, k in ipairs(skeys) do out[#out + 1] = estr(k) .. ":" .. enc(v[k], k) end
    if #nkeys > 0 then
        local o = { }
        for _, k in ipairs(nkeys) do o[#o + 1] = '"' .. fmt("%.14g", k) .. '":' .. enc(v[k]) end
        out[#out + 1] = '"$num":{' .. t_concat(o, ",") .. "}"
    end
    return "{" .. t_concat(out, ",") .. "}"
end
-- samples: { [key] = { {cap1, …}, … } } → { [key] = { kind, v | calls, compacted } }
function __mp6_run(samples)
    local out = { }
    for k, tuples in pairs(samples) do
        local v = __mp6_lit[k]
        compacted = 0
        local r
        if v == nil then
            r = '{"kind":"missing"}'
        elseif type(v) == "function" then
            local calls = { }
            for i, cap in ipairs(tuples) do
                local ok, res = pcall(v, tonumber(cap[1]), unpack(cap))
                if not ok then
                    calls[i] = '{"ok":false,"err":' .. estr(tostring(res)) .. '}'
                elseif res == nil then
                    calls[i] = '{"ok":true,"nil":true}'
                else
                    calls[i] = '{"ok":true,"v":' .. enc(res) .. '}'
                end
            end
            r = '{"kind":"function","calls":[' .. t_concat(calls, ",") .. ']'
        else
            r = '{"kind":' .. estr(type(v)) .. ',"v":' .. enc(v)
        end
        if v ~= nil then r = r .. ',"compacted":' .. compacted .. '}' end
        out[#out + 1] = estr(k) .. ":" .. r
    end
    local cnt = 0
    for _ in pairs(__mp6_lit) do cnt = cnt + 1 end
    out[#out + 1] = '"$literalCount":' .. cnt
    return "{" .. t_concat(out, ",") .. "}"
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

fn lua_eval(pob: &mut JitPob, code: &str) -> String {
    let r = pob.run(code).unwrap_or_else(|e| panic!("lua: {e}"));
    match r.first() {
        Some(Ret::Str(s)) => s.clone(),
        other => panic!("lua returned {other:?}"),
    }
}

// ---------------------------------------------------------------------------------------------
// mp6_oracle
// ---------------------------------------------------------------------------------------------

#[test]
fn mp6_oracle() {
    let Some(src_text) = modparser_src() else {
        return;
    };
    let srcl: Vec<&str> = src_text.lines().collect();
    if !init_data() {
        return;
    }
    let Some(pob_src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout") else {
        return;
    };
    let Some(records) = pob_testpaths::require(load_oracle(), "data/dump/parse_oracle.jsonl")
    else {
        return;
    };
    let ours = pob_modparser::special_d::entries();
    let by_line: BTreeMap<u32, usize> = ours
        .iter()
        .enumerate()
        .map(|(i, e)| (e.lua_line, i))
        .collect();

    // -- sample captures per closure: PoB's own scans first, then synthetic ones
    let mut samples: Vec<Vec<Vec<String>>> = vec![Vec::new(); ours.len()];
    let mut hits = vec![0usize; ours.len()];
    let mut oracle_caps = vec![BTreeSet::<Vec<String>>::new(); ours.len()];
    for r in &records {
        for p in &r.passes {
            for st in &p.steps {
                if st.list != "specialModList" || st.pattern.is_none() {
                    continue;
                }
                let Some(&i) = st.line.and_then(|l| by_line.get(&l)) else {
                    continue;
                };
                hits[i] += 1;
                if oracle_caps[i].len() < 40 {
                    oracle_caps[i].insert(st.caps.iter().map(cap_text).collect());
                }
            }
        }
    }
    for (i, e) in ours.iter().enumerate() {
        if let SpecialVal::Mods(_) = e.val {
            continue;
        }
        let v = &mut samples[i];
        v.extend(oracle_caps[i].iter().cloned());
        for s in synthetic(e.pat, 60) {
            if !v.contains(&s) {
                v.push(s);
            }
        }
    }

    // -- the Lua side
    let _lock = pob_oracle::runner::OracleLock::acquire().unwrap_or_else(|e| panic!("{e}"));
    let t0 = std::time::Instant::now();
    let mut pob = JitPob::boot_oracle(&pob_src, Some(PRE)).unwrap_or_else(|e| panic!("boot: {e}"));
    eprintln!("PoB booted in {:.1}s", t0.elapsed().as_secs_f64());
    pob.run(LUA_LIB).unwrap_or_else(|e| panic!("lua lib: {e}"));
    let mut code = String::from("local S = {\n");
    for (i, e) in ours.iter().enumerate() {
        code.push_str(&format!("[{}] = {{", lua_lit(e.pat)));
        for t in &samples[i] {
            code.push('{');
            for c in t {
                code.push_str(&lua_lit(c));
                code.push(',');
            }
            code.push_str("},");
        }
        code.push_str("},\n");
    }
    code.push_str("}\nreturn __mp6_run(S)\n");
    let out: J = serde_json::from_str(&lua_eval(&mut pob, &code)).expect("lua json");

    // the literal table's size equals the sum of the four ranges' keys
    let total_keys = literal_keys(&srcl, 2130, HI).len();
    assert_eq!(
        out["$literalCount"].as_u64().unwrap() as usize,
        total_keys,
        "PoB's literal specialModList size vs the keys of ModParser.lua 2130-5942"
    );

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // -- compare every entry
    let mut bad: Vec<String> = Vec::new();
    let mut accepted: BTreeMap<u32, (usize, usize, usize)> = BTreeMap::new();
    let (mut nStatic, mut nFn, mut nCalls, mut nErr, mut nNil) = (0usize, 0, 0, 0, 0);
    let mut minCalls = usize::MAX;
    for (i, e) in ours.iter().enumerate() {
        let l = &out[e.pat];
        let kind = l["kind"].as_str().unwrap();
        let what = format!("{} {:?}", e.lua_line, e.pat);
        let compacted = l["compacted"].as_u64().unwrap_or(0) as usize;
        let mut notes = Notes::default();
        match &e.val {
            SpecialVal::Mods(m) => {
                nStatic += 1;
                if kind != "table" {
                    bad.push(format!("{what}: PoB has a {kind}, ours is a table"));
                    continue;
                }
                let want = Some(mods_from_json(&l["v"], &what));
                if !mods_eq(&want, &Some(m.clone()), &mut notes) {
                    bad.push(format!("{what}: PoB {want:?} / ours {m:?}"));
                }
            }
            val => {
                nFn += 1;
                if kind != "function" {
                    bad.push(format!("{what}: PoB has a {kind}, ours is a closure"));
                    continue;
                }
                let calls = l["calls"].as_array().unwrap();
                assert_eq!(calls.len(), samples[i].len(), "{what}: call count");
                minCalls = minCalls.min(calls.len());
                for (cap, c) in samples[i].iter().zip(calls) {
                    nCalls += 1;
                    let got = rust_call(val, cap);
                    match (c["ok"].as_bool().unwrap(), got) {
                        (true, Ok(m)) => {
                            let want = if c["nil"].as_bool() == Some(true) {
                                nNil += 1;
                                None
                            } else {
                                Some(mods_from_json(&c["v"], &what))
                            };
                            if !mods_eq(&want, &m, &mut notes) {
                                bad.push(format!("{what} caps {cap:?}: PoB {want:?} / ours {m:?}"));
                            }
                        }
                        (false, Err(err)) => {
                            nErr += 1;
                            let le = c["err"].as_str().unwrap();
                            if strip_pos(le) != err {
                                bad.push(format!(
                                    "{what} caps {cap:?}: PoB raises {le:?} / ours {err:?}"
                                ));
                            }
                        }
                        (true, Err(err)) => bad.push(format!(
                            "{what} caps {cap:?}: PoB returns {} / ours raises {err:?}",
                            c.get("v").unwrap_or(&J::Null)
                        )),
                        (false, Ok(m)) => bad.push(format!(
                            "{what} caps {cap:?}: PoB raises {} / ours {m:?}",
                            c["err"]
                        )),
                    }
                }
            }
        }
        if notes.nil_hole + notes.digit_string + compacted > 0 {
            accepted.insert(e.lua_line, (notes.nil_hole, notes.digit_string, compacted));
        }
    }
    for (line, (nh, ds, cp)) in &accepted {
        if *nh > 0 && !NIL_HOLE_LINES.contains(line) {
            bad.push(format!(
                "{line}: nil-hole tag difference at an unlisted line"
            ));
        }
        if *cp > 0 && !COMPACTED_LIST_LINES.contains(line) {
            bad.push(format!("{line}: holey skillTypeList at an unlisted line"));
        }
        if *ds > 0 && !DIGIT_STRING_LINES.contains(line) {
            bad.push(format!(
                "{line}: digit-string tag field difference at an unlisted line"
            ));
        }
    }
    let unhit: Vec<u32> = ours
        .iter()
        .zip(&hits)
        .filter(|(_, h)| **h == 0)
        .map(|(e, _)| e.lua_line)
        .collect();
    eprintln!(
        "entries: {nStatic} static, {nFn} closures; {nCalls} closure calls ({nErr} raise in both, \
         {nNil} return nil in both), >= {minCalls} per closure"
    );
    eprintln!(
        "accepted parse-level differences (line: nil-hole tags, digit-string tag fields, \
         compacted skillTypeList): {accepted:?}"
    );
    eprintln!(
        "oracle coverage: {} of {} entries hit by >= 1 parse-oracle line; unhit (covered by the \
         direct comparison only): {unhit:?}",
        ours.len() - unhit.len(),
        ours.len()
    );

    // -- L1 over the records whose parse picked an entry of this range
    l1(&srcl, &records, &by_line, &mut bad);

    std::panic::set_hook(hook);
    for b in bad.iter().take(80) {
        eprintln!("MISMATCH {b}");
    }
    assert!(bad.is_empty(), "{} mismatches", bad.len());
}

/// The whole specialModList: MP3-MP5's entries when ported, else their keys with a placeholder
/// value; this range's real entries; then MP0's keystones, anchoring and generated entries.
fn special_list(src: &[&str]) -> (&'static PatternList<SpecialVal>, Vec<&'static str>) {
    let ported: [Vec<PatternEntry<SpecialVal>>; 3] = [
        pob_modparser::special_a::entries(),
        pob_modparser::special_b::entries(),
        pob_modparser::special_c::entries(),
    ];
    let mut literal: Vec<PatternEntry<SpecialVal>> = Vec::new();
    let mut which = Vec::new();
    for ((unit, a, b), real) in OTHER_RANGES.iter().zip(ported) {
        if real.is_empty() {
            which.push(*unit);
            for (k, l, _) in literal_keys(src, *a, *b) {
                literal.push(PatternEntry {
                    pat: Box::leak(k.into_boxed_str()),
                    val: SpecialVal::Mods(vec![pob_modparser::helpers::flag(
                        Name::from("MP6:placeholder"),
                        None,
                        0,
                        0,
                        &[],
                    )]),
                    lua_line: l,
                });
            }
        } else {
            literal.extend(real);
        }
    }
    literal.extend(pob_modparser::special_d::entries());
    let list = pob_modparser::generated_lists::complete_special_mod_list(literal);
    (Box::leak(Box::new(list)), which)
}

fn parse_with(
    lists: &ParserLists,
    line: &str,
) -> Result<(Option<Vec<Mod>>, Option<String>), String> {
    pcall(|| {
        let (m, e) = parseModPassWith(lists, line, 1);
        if m.is_some() && e.is_some() {
            parseModPassWith(lists, line, 2)
        } else {
            (m, e)
        }
    })
}

fn l1(src: &[&str], recs: &[Record], by_line: &BTreeMap<u32, usize>, bad: &mut Vec<String>) {
    let (special, placeholders) = special_list(src);
    let lists = ParserLists {
        specialModList: special,
        ..ParserLists::statics()
    };
    let stride: usize = std::env::var("MP6_L1_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let sel: Vec<&Record> = recs
        .iter()
        .filter(|r| {
            special_line(r).is_some_and(|l| by_line.contains_key(&l))
                && !r.passes.iter().flat_map(|p| p.steps.iter()).any(|s| {
                    s.exact
                        || (s.pattern.is_some()
                            && matches!(s.list.as_str(), "jewelFuncList" | "clusterJewelSkills"))
                })
        })
        .step_by(stride)
        .collect();
    let t = std::time::Instant::now();
    let (mut ok, mut nbad) = (0usize, 0usize);
    let mut accepted = 0usize;
    for r in &sel {
        let want = oracle_mods(r);
        let mut notes = Notes::default();
        match parse_with(&lists, &r.line) {
            Ok((m, e)) if mods_eq(&want, &m, &mut notes) && e.as_deref() == r.extra.as_deref() => {
                ok += 1;
                accepted += usize::from(notes.nil_hole + notes.digit_string > 0);
                let line = special_line(r).unwrap_or(0);
                if notes.nil_hole > 0 && !NIL_HOLE_LINES.contains(&line) {
                    bad.push(format!(
                        "L1 {:?} (entry line {line}): nil-hole tag difference at an unlisted line",
                        r.line
                    ));
                }
                if notes.digit_string > 0 && !DIGIT_STRING_LINES.contains(&line) {
                    bad.push(format!(
                        "L1 {:?} (entry line {line}): digit-string tag field difference at an unlisted line",
                        r.line
                    ));
                }
            }
            other => {
                nbad += 1;
                bad.push(format!(
                    "L1 {:?} (entry line {:?}): PoB {:?} / {:?}; ours {:?}",
                    r.line,
                    special_line(r),
                    want,
                    r.extra,
                    other
                ));
            }
        }
    }
    eprintln!(
        "L1 (MP6 range; placeholders for {placeholders:?}): {} records whose parse picked an entry \
         of 4989-5942 (stride {stride}): {ok} identical ({accepted} via an accepted parse-level \
         difference), {nbad} differ, {:.1}s",
        sel.len(),
        t.elapsed().as_secs_f64()
    );
}

// ---------------------------------------------------------------------------------------------
// whole_parser_l1
// ---------------------------------------------------------------------------------------------

/// specialModList lines whose `mod(…, nil, {tag})` leaves a tag at `mod[2]` (known_diffs.toml
/// MP5-1, MP6-1; calc doc §6).
const ALL_NIL_HOLE_LINES: [u32; 8] = [4576, 4577, 4578, 4579, 4580, 5686, 5689, 5692];
/// specialModList lines whose closures store a digits-only capture string in a numeric tag field
/// (known_diffs.toml MP3-1, MP4-1, MP5-2, MP6-2; calc doc §6).
const ALL_DIGIT_STRING_LINES: [u32; 10] =
    [2325, 2571, 2862, 2922, 3535, 4216, 4222, 5733, 5734, 5741];

/// The Lua closures a jewel func spec can stand for, by their `debug.getinfo` source range (what
/// the parse oracle records for a function value, pob-dump walker.lua:182): the closures the
/// helpers return (getSimpleConv 18-39, getPerStat 6369-6377, getThreshold 6455-6479) and the
/// jewelFuncList wrappers (6551-6593). `check_closure_ranges` checks each against the source.
const SIMPLE_CONV_SRC: (usize, usize) = (19, 38);
const PER_STAT_SRC: (usize, usize) = (6370, 6376);
const THRESHOLD_SRC: (usize, usize) = (6457, 6478);
/// `{ func = function(cap1, …, cap5) … end, type = "Other" }` (6555-6570) and the closure it
/// returns (6564-6569).
const OTHER_WRAPPER_SRC: (usize, usize) = (6555, 6570);
const OTHER_INNER_SRC: (usize, usize) = (6564, 6569);
/// The threshold-table wrapper (6582-6589).
const THRESHOLD_LIST_SRC: (usize, usize) = (6582, 6589);
/// The two inline `ExtraJewelFunc` closures: which captures of their specialModList entry
/// (2621 / 2629) are the closure's upvalues, in `args` order (CCR-0039 §3).
const EXTRA_JEWEL_UPVALUES: [(&str, &[usize]); 2] = [
    ("Modules/ModParser.lua:2622-2627", &[1]),
    ("Modules/ModParser.lua:2630-2634", &[1, 2]),
];

fn src_key((a, b): (usize, usize)) -> String {
    format!("Modules/ModParser.lua:{a}-{b}")
}

/// Each range above starts with a `function(` and ends with the `end` closing it.
fn check_closure_ranges(src: &[&str]) {
    for (a, b) in [
        SIMPLE_CONV_SRC,
        PER_STAT_SRC,
        THRESHOLD_SRC,
        OTHER_WRAPPER_SRC,
        OTHER_INNER_SRC,
        THRESHOLD_LIST_SRC,
        (2622, 2627),
        (2630, 2634),
    ] {
        assert!(
            src[a - 1].contains("function("),
            "ModParser.lua:{a}: {:?}",
            src[a - 1]
        );
        assert!(
            src[b - 1].trim_start().starts_with("end"),
            "ModParser.lua:{b}: {:?}",
            src[b - 1]
        );
    }
}

/// The Lua closure a helper / inline spec stands for (`jewel_func_list.rs` module docs).
fn closure_src(spec: &JewelFuncSpec) -> Option<String> {
    match spec.key.as_str() {
        "$getSimpleConv" => Some(src_key(SIMPLE_CONV_SRC)),
        "$getPerStat" => Some(src_key(PER_STAT_SRC)),
        "$getThreshold" => Some(src_key(THRESHOLD_SRC)),
        k if k.starts_with("Modules/ModParser.lua:") => Some(k.to_owned()),
        _ => None,
    }
}

/// The source range of the Lua closure that `jewelFuncList[lineLower].func` (an exact line,
/// `args` empty, 6649-6651) or `patternVal.func(cap1, …, cap5)` (a pattern match, 6644-6646) is.
fn list_func_src(o: &JewelFuncSpec) -> Option<String> {
    if o.r#type.as_deref() == Some("Other") {
        return Some(src_key(if o.args.is_empty() {
            OTHER_WRAPPER_SRC
        } else {
            OTHER_INNER_SRC
        }));
    }
    let v = jewelFuncListSource.get(o.key.as_str())?;
    if v.key.as_str() == "$list" {
        return o.args.is_empty().then(|| src_key(THRESHOLD_LIST_SRC));
    }
    if o.args.is_empty() {
        closure_src(v)
    } else {
        closure_src(&callCaptureClosure(v, &o.args)?)
    }
}

/// PoB's jewel closures reach the parse oracle as their source range only (the walker does not
/// dump upvalues), and the dump decoder folds `{ func = <closure>, type = …[, radius = …] }` into
/// one `Value::JewelFunc` keyed by that range (pob_gamedata `gen_to_jewel_func`). Ours carries
/// the closure as data (CCR-0014 §3, CCR-0039 §3). This checks what L1 can see against the
/// record's own winning step, then rewrites our value into the oracle's shape:
/// * a jewelFuncList value `{ func = spec, type }`: `spec.key` is the step's pattern (the
///   lowercased key); an exact step gives no `args`, a pattern step `args` = its captures;
///   `spec.type` is the table's `type`; and the spec stands for the closure range PoB recorded;
/// * an inline `ExtraJewelFunc` spec (2621 / 2629): `args` are the step's upvalue captures.
///
/// The closures' behaviour is MP7's L2 (`MP7.rs`: every func on every node of the 3_26 tree).
fn jewel_to_oracle_shape(r: &Record, ours: &mut Option<Vec<Mod>>) -> Result<usize, String> {
    let Some(ms) = ours else {
        return Ok(0);
    };
    let steps = || {
        r.passes
            .iter()
            .flat_map(|p| p.steps.iter())
            .filter(|s| s.pattern.is_some())
    };
    let caps_of = |s: &Step| -> Vec<Value> {
        s.caps
            .iter()
            .map(|c| Value::from(cap_text(c).as_str()))
            .collect()
    };
    let mut n = 0;
    for m in ms.iter_mut() {
        let new = match &m.value {
            Value::Table(t) if matches!(t.get(n!("func")), Value::JewelFunc(_)) => {
                let Value::JewelFunc(o) = t.get(n!("func")) else {
                    unreachable!()
                };
                let ty = match t.get(n!("type")) {
                    Value::Str(s) => Name::from(&**s),
                    other => return Err(format!("jewel func table type {other:?}")),
                };
                let radius = match t.get(n!("radius")) {
                    Value::Nil => None,
                    Value::Str(s) => Some(Name::from(&**s)),
                    other => return Err(format!("jewel func table radius {other:?}")),
                };
                let nfields = 2 + usize::from(radius.is_some());
                if t.fields.len() != nfields || !t.array.is_empty() || o.r#type != Some(ty) {
                    return Err(format!("jewel func table {t:?}"));
                }
                let step = steps()
                    .find(|s| s.list == "jewelFuncList")
                    .ok_or("a jewelFuncList value without a jewelFuncList step")?;
                if step.pattern.as_deref() != Some(o.key.as_str()) {
                    return Err(format!(
                        "jewelFuncList key {:?}, PoB {:?}",
                        o.key, step.pattern
                    ));
                }
                let want = if step.exact {
                    Vec::new()
                } else {
                    caps_of(step)
                };
                if o.args.as_slice() != want.as_slice() {
                    return Err(format!("jewel func args {:?}, captures {want:?}", o.args));
                }
                let key = list_func_src(o).ok_or_else(|| format!("no Lua closure for {o:?}"))?;
                JewelFuncSpec {
                    key: Name::from(key.as_str()),
                    kind: o.kind,
                    args: smallvec::SmallVec::new(),
                    r#type: Some(ty),
                    radius,
                }
            }
            Value::JewelFunc(o) => {
                let Some((_, ups)) = EXTRA_JEWEL_UPVALUES
                    .iter()
                    .find(|(k, _)| *k == o.key.as_str())
                else {
                    return Err(format!("jewel func value {o:?}"));
                };
                let step = steps()
                    .find(|s| s.list == "specialModList")
                    .ok_or("an inline jewel func without a specialModList step")?;
                let caps = caps_of(step);
                let want: Vec<Value> = ups
                    .iter()
                    .map(|&i| caps.get(i).cloned().unwrap_or(Value::Nil))
                    .collect();
                if o.args.as_slice() != want.as_slice() {
                    return Err(format!(
                        "{} upvalues {:?}, captures {want:?}",
                        o.key, o.args
                    ));
                }
                JewelFuncSpec {
                    args: smallvec::SmallVec::new(),
                    ..(**o).clone()
                }
            }
            _ => continue,
        };
        m.value = Value::JewelFunc(Arc::new(new));
        n += 1;
    }
    Ok(n)
}

/// The specialModList entry lines of the record's steps.
fn special_lines(r: &Record) -> BTreeSet<u32> {
    r.passes
        .iter()
        .flat_map(|p| p.steps.iter())
        .filter(|s| s.list == "specialModList" && s.pattern.is_some())
        .filter_map(|s| s.line)
        .collect()
}

#[test]
fn whole_parser_l1() {
    if !init_data() {
        return;
    }
    let Some(src) = modparser_src() else {
        return;
    };
    let src: Vec<&str> = src.lines().collect();
    check_closure_ranges(&src);
    let Some(records) = pob_testpaths::require(load_oracle(), "data/dump/parse_oracle.jsonl")
    else {
        return;
    };
    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));
    let (mut ok, mut nbad) = (0usize, 0usize);
    let (mut nJewel, mut nNilHole, mut nDigit) = (0usize, 0usize, 0usize);
    let mut first: Vec<String> = Vec::new();
    for r in &records {
        let want = oracle_mods(r);
        let mut notes = Notes::default();
        let got = pcall(|| pob_modparser::parseModNative(&r.line)).and_then(|(mut m, e)| {
            let nj = jewel_to_oracle_shape(r, &mut m)?;
            Ok((m, e, nj))
        });
        let lines = special_lines(r);
        match got {
            Ok((m, e, nj))
                if mods_eq(&want, &m, &mut notes)
                    && e.as_deref() == r.extra.as_deref()
                    // an accepted difference only where known_diffs.toml lists it
                    && (notes.nil_hole == 0
                        || lines.iter().any(|l| ALL_NIL_HOLE_LINES.contains(l)))
                    && (notes.digit_string == 0
                        || lines.iter().any(|l| ALL_DIGIT_STRING_LINES.contains(l))) =>
            {
                ok += 1;
                nJewel += nj;
                nNilHole += notes.nil_hole;
                nDigit += notes.digit_string;
            }
            other => {
                nbad += 1;
                if first.len() < 40 {
                    first.push(format!(
                        "{:?}: PoB {want:?} / {:?}; ours {other:?}",
                        r.line, r.extra
                    ));
                }
            }
        }
    }
    std::panic::set_hook(hook);
    let pct = 100.0 * ok as f64 / (ok + nbad) as f64;
    for f in &first {
        eprintln!("DIFF {f}");
    }
    eprintln!(
        "whole-parser L1 (cache disabled): {ok}/{} identical = {pct:.3}% ({nJewel} jewel funcs \
         compared by entry, captures and closure range; accepted known diffs: {nNilHole} nil-hole \
         tags, {nDigit} digit-string tag fields)",
        ok + nbad
    );
    assert_eq!(
        records.len(),
        PARSE_ORACLE_RECORDS,
        "parse-oracle record count"
    );
    assert!(pct >= 99.95, "whole-parser L1 {pct:.3}% < 99.95%");
}

/// `data/dump/parse_oracle.jsonl` record count.
const PARSE_ORACLE_RECORDS: usize = 80_993;
