//! Unit MP2 acceptance (calc doc §11.1, row MP2: modFlagList, preFlagList, modTagList,
//! ModParser.lua 913-2010).
//!
//! * `list_shape`: entry count equals PoB's (`listSizes` of the parse oracle manifest:
//!   modFlagList 181, preFlagList 190, modTagList 665); the entries are the literal keys of
//!   ModParser.lua in source order, each with the line of its last definition (Lua constructor
//!   semantics for duplicate keys); preFlagList has 7 closures and modTagList 136 (143 in all);
//!   building the lists hits no `unported!` site.
//! * `mp2_oracle` (PoB booted under LuaJIT with the JIT off, ModParser.lua instrumented in
//!   memory to hand out its three local tables):
//!   - every static entry equals PoB's table (all keys, tags field by field);
//!   - every one of the 143 closures is called, in the parse driver's call shape
//!     (ModParser.lua:6680-6682 for preFlagList, 6698-6712 for modTagList), with sample
//!     captures: every distinct capture tuple PoB's own scans produced for that pattern in
//!     `data/dump/parse_oracle.jsonl`, plus synthetic captures derived from the pattern's
//!     capture classes (digits, words, dotted numbers, …, including inputs on which the Lua
//!     raises). The Lua result table and the Rust `ModSpec` must be equal; when the Lua raises,
//!     the Rust must raise the same message;
//!   - L1 for this range: every parse-oracle line whose parse used one of the three lists (and
//!     no specialModList / jewel / cluster / unsupported entry, whose lists are other units'
//!     stubs) is parsed by MP0's driver over these lists and must give PoB's mods and remainder.
//!     formList and modNameList come from MP1 when it is merged, else they are rebuilt from
//!     PoB's tables (test-only, read from the instrumented LuaJIT state).
//!
//! Inputs: `data/dump` and the PoB checkout; missing inputs fail
//! (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips). `MP2_L1_STRIDE=n` checks every n-th L1 record.
#![allow(non_snake_case)]
#![allow(clippy::type_complexity, clippy::too_many_lines)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Once;

use pob_gamedata::{Mod, Name, Tag, Value};
use pob_lua::LuaError;
use pob_modparser::mod_flag_list::modFlagList;
use pob_modparser::mod_tag_list::modTagList;
use pob_modparser::pre_flag_list::preFlagList;
use pob_modparser::scan::{parseModPassWith, ParserLists};
use pob_modparser::types::{Caps, ModSpec, PatternEntry, PatternList, TagVal};
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up};
use serde_json::{Map, Value as J};

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

fn is_fn(v: &TagVal) -> bool {
    matches!(v, TagVal::Fn(_))
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
    let lists: [(&str, Vec<(&str, u32)>, (usize, usize)); 3] = [
        (
            "modFlagList",
            modFlagList.0.iter().map(|e| (e.pat, e.lua_line)).collect(),
            (913, 1099),
        ),
        (
            "preFlagList",
            preFlagList.0.iter().map(|e| (e.pat, e.lua_line)).collect(),
            (1102, 1317),
        ),
        (
            "modTagList",
            modTagList.0.iter().map(|e| (e.pat, e.lua_line)).collect(),
            (1320, 2009),
        ),
    ];
    let hits: Vec<_> = pob_gamedata::stub::stub_report()
        .into_iter()
        .filter(|(s, _)| s.starts_with("Modules/ModParser.lua"))
        .collect();
    assert!(hits.is_empty(), "unported sites hit: {hits:?}");

    let sizes: Option<J> = dump_dir().and_then(|d| {
        let m: J =
            serde_json::from_str(&std::fs::read_to_string(d.join("manifest.json")).ok()?).ok()?;
        Some(m["parseOracle"]["listSizes"].clone())
    });
    for (name, ours, (a, b)) in &lists {
        // PoB's own count (the parse oracle manifest), else the frozen numbers
        let want = sizes.as_ref().and_then(|s| s[*name].as_u64()).map_or(
            match *name {
                "modFlagList" => 181,
                "preFlagList" => 190,
                _ => 665,
            },
            |n| n as usize,
        );
        assert_eq!(ours.len(), want, "{name}: entry count");
        // source order: the (last-definition) lines are strictly increasing
        assert!(
            ours.windows(2).all(|w| w[0].1 < w[1].1),
            "{name}: entries not in source order"
        );
        let mut lit = literal_keys(&src, *a, *b);
        lit.sort_by_key(|(_, l)| *l);
        let lit: Vec<(&str, u32)> = lit.iter().map(|(k, l)| (k.as_str(), *l)).collect();
        assert_eq!(ours, &lit, "{name}: keys/lines differ from ModParser.lua");
        eprintln!(
            "{name}: {} entries, keys and order equal ModParser.lua {a}-{b}",
            ours.len()
        );
    }
    let nPre = preFlagList.0.iter().filter(|e| is_fn(&e.val)).count();
    let nTag = modTagList.0.iter().filter(|e| is_fn(&e.val)).count();
    assert_eq!((nPre, nTag), (7, 136), "closure counts");
    eprintln!(
        "closures: preFlagList {nPre}, modTagList {nTag} (= {})",
        nPre + nTag
    );
}

// ---------------------------------------------------------------------------------------------
// LuaJIT side
// ---------------------------------------------------------------------------------------------

/// Installed before PoB boots: ModParser.lua (loaded through `loadfile` by `LoadModule`) gets
/// ` __mp2_lists = {…}` appended to its `local cache = { }` line (6994), which hands its local
/// pattern tables to the test. Line numbers are unchanged.
const PRE: &str = r#"
local orig_loadfile = loadfile
loadfile = function(path, ...)
    if type(path) == "string" and path:match("Modules/ModParser%.lua$") then
        local orig_loadstring = loadstring
        local hit = false
        loadstring = function(src, name)
            loadstring = orig_loadstring
            local n
            src, n = src:gsub("\nlocal cache = { }\n", "\nlocal cache = { } __mp2_lists = { formList = formList, modNameList = modNameList, modFlagList = modFlagList, preFlagList = preFlagList, modTagList = modTagList }\n", 1)
            hit = n == 1
            return orig_loadstring(src, name)
        end
        local f, e = orig_loadfile(path, ...)
        loadstring = orig_loadstring
        assert(hit, "MP2: ModParser.lua instrumentation point `local cache = { }` not found")
        return f, e
    end
    return orig_loadfile(path, ...)
end
"#;

/// The JSON encoder and the call shapes (Lua side).
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
        if v ~= v then return '"$nan"' end
        if v == math.huge then return '"$inf"' end
        if v == -math.huge then return '"$-inf"' end
        if v == math.floor(v) and math.abs(v) < 2^53 then return fmt("%d", v) end
        return fmt("%.17g", v)
    elseif t == "boolean" then return tostring(v)
    elseif t == "nil" then return "null"
    elseif t == "table" then
        local n, cnt = #v, 0
        for _ in pairs(v) do cnt = cnt + 1 end
        if n > 0 and cnt == n then
            local out = {}
            for i = 1, n do out[i] = enc(v[i]) end
            return "[" .. t_concat(out, ",") .. "]"
        end
        local keys = {}
        for k in pairs(v) do keys[#keys + 1] = k end
        t_sort(keys, function(a, b) return tostring(a) < tostring(b) end)
        local out = {}
        for _, k in ipairs(keys) do
            local ks = type(k) == "number" and ("#" .. fmt("%d", k)) or tostring(k)
            out[#out + 1] = estr(ks) .. ":" .. enc(v[k])
        end
        return "{" .. t_concat(out, ",") .. "}"
    else
        return estr("$" .. t)
    end
end
__mp2_enc = enc
local function callPre(f, cap) return f(unpack(cap)) end
local function callTag(f, cap)
    if cap[1]:match("%d+") then
        return f(tonumber(cap[1]), unpack(cap))
    else
        return f(cap[1], unpack(cap))
    end
end
-- { list = { key = { kind = "table", v = … } | { kind = "function", calls = { { ok, v | err } } } } }
function __mp2_run(samples)
    local out = {}
    for _, name in ipairs({ "modFlagList", "preFlagList", "modTagList" }) do
        local list = __mp2_lists[name]
        local res = {}
        for k, v in pairs(list) do
            if type(v) == "function" then
                local calls = {}
                for i, cap in ipairs((samples[name] or {})[k] or {}) do
                    local ok, r = pcall(name == "preFlagList" and callPre or callTag, v, cap)
                    if ok then
                        calls[i] = { ok = true, v = r == nil and "$nil" or r }
                    else
                        calls[i] = { ok = false, err = tostring(r) }
                    end
                end
                res[k] = { kind = "function", calls = calls }
            else
                res[k] = { kind = type(v), v = v }
            end
        end
        out[name] = res
    end
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

// ---------------------------------------------------------------------------------------------
// ModSpec <-> the Lua table, as JSON
// ---------------------------------------------------------------------------------------------

/// Numbers as f64 (Lua has one number type); everything else unchanged.
fn norm(v: &J) -> J {
    match v {
        J::Number(n) => J::Number(serde_json::Number::from_f64(n.as_f64().unwrap()).unwrap()),
        J::Array(a) => J::Array(a.iter().map(norm).collect()),
        J::Object(o) => J::Object(o.iter().map(|(k, v)| (k.clone(), norm(v))).collect()),
        other => other.clone(),
    }
}

fn num(x: f64) -> J {
    J::Number(serde_json::Number::from_f64(x).unwrap())
}

fn tag_json(t: &Tag) -> J {
    serde_json::to_value(t).unwrap()
}

/// The Lua table a `ModSpec` stands for (calc doc §6 `ModSpec`), in the encoder's JSON shape.
fn spec_json(s: &ModSpec) -> J {
    let mut m = Map::new();
    let mut put = |k: &str, v: J| {
        m.insert(k.to_owned(), v);
    };
    if s.flags != 0 {
        put("flags", num(f64::from(s.flags)));
    }
    if s.keywordFlags != 0 {
        put("keywordFlags", num(f64::from(s.keywordFlags)));
    }
    let tags = |l: &[Tag]| J::Array(l.iter().map(tag_json).collect());
    if let Some(t) = &s.tag {
        put("tag", tag_json(t));
    }
    if !s.tagList.is_empty() {
        put("tagList", tags(&s.tagList));
    }
    for (k, b) in [
        ("addToMinion", s.addToMinion),
        ("addToAura", s.addToAura),
        ("onlyAddToBanners", s.onlyAddToBanners),
        ("applyToEnemy", s.applyToEnemy),
        ("actorEnemy", s.actorEnemy),
        ("newAura", s.newAura),
        ("newAuraOnlyAllies", s.newAuraOnlyAllies),
        ("affectedByAura", s.affectedByAura),
    ] {
        if b {
            put(k, J::Bool(true));
        }
    }
    for (k, t) in [
        ("addToMinionTag", &s.addToMinionTag),
        ("addToSkill", &s.addToSkill),
        ("playerTag", &s.playerTag),
    ] {
        if let Some(t) = t {
            put(k, tag_json(t));
        }
    }
    if !s.playerTagList.is_empty() {
        put("playerTagList", tags(&s.playerTagList));
    }
    if let Some(x) = s.modSuffix {
        put("modSuffix", J::String(x.as_str().to_owned()));
    }
    if let Some(x) = s.modType {
        put("modType", J::String(x.as_str().to_owned()));
    }
    if let Some(names) = &s.modName {
        put(
            "modName",
            J::Array(
                names
                    .iter()
                    .map(|n| J::String(n.as_str().to_owned()))
                    .collect(),
            ),
        );
    }
    for (i, e) in s.entries.iter().enumerate() {
        put(&format!("#{}", i + 1), spec_json(e));
    }
    for (k, v) in &s.extra {
        put(k.as_str(), serde_json::to_value(v).unwrap());
    }
    norm(&J::Object(m))
}

fn tag_from(v: &J, what: &str) -> Tag {
    serde_json::from_value(v.clone()).unwrap_or_else(|e| panic!("{what}: tag {v}: {e}"))
}

/// A Lua formList/modNameList value as the Rust list value (test-only rebuild of MP1's lists).
fn spec_from_json(v: &J, what: &str) -> ModSpec {
    let mut s = ModSpec::default();
    match v {
        J::String(n) => {
            s.modName = Some(vec![Name::from(n.as_str())]);
        }
        J::Array(a) => {
            s.modName = Some(
                a.iter()
                    .map(|x| Name::from(x.as_str().unwrap_or_else(|| panic!("{what}: {v}"))))
                    .collect(),
            );
            s.modNameIsTable = true;
        }
        J::Object(o) => {
            let mut names: BTreeMap<usize, Name> = BTreeMap::new();
            for (k, x) in o {
                if let Some(i) = k.strip_prefix('#') {
                    names.insert(i.parse().unwrap(), Name::from(x.as_str().unwrap()));
                    continue;
                }
                match k.as_str() {
                    "flags" => s.flags = x.as_f64().unwrap() as u32,
                    "keywordFlags" => s.keywordFlags = x.as_f64().unwrap() as u32,
                    "tag" => s.tag = Some(tag_from(x, what)),
                    "tagList" => {
                        s.tagList = x
                            .as_array()
                            .unwrap()
                            .iter()
                            .map(|t| tag_from(t, what))
                            .collect();
                    }
                    "addToMinion" => s.addToMinion = x.as_bool().unwrap(),
                    "addToSkill" => s.addToSkill = Some(tag_from(x, what)),
                    "addToMinionTag" => s.addToMinionTag = Some(tag_from(x, what)),
                    "modSuffix" => s.modSuffix = Some(Name::from(x.as_str().unwrap())),
                    _ => panic!("{what}: unexpected key {k} in {v}"),
                }
            }
            if !names.is_empty() {
                assert!(
                    names.keys().copied().eq(1..=names.len()),
                    "{what}: holey names"
                );
                s.modName = Some(names.into_values().collect());
                s.modNameIsTable = true;
            }
        }
        _ => panic!("{what}: unexpected value {v}"),
    }
    s
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
                // a `]` right after `[` / `[^` is literal
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
        "%a+" => vec!["fire", "right", "left", "rare", "shocked"],
        "%w+" => vec!["fire", "bow", "1", "2x"],
        ".+" => vec!["blue", "a1", "searching"],
        "[%a%s]+" => vec!["gloves", "body armour", "rings"],
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

fn oracle_mods(r: &Record) -> Option<Vec<Mod>> {
    r.mods.as_ref().map(|v| {
        let seq = match v {
            J::Object(o) => {
                assert!(o.keys().all(|k| k == "$seq"), "{}: mods {v}", r.line);
                o.get("$seq").cloned().unwrap_or(J::Array(Vec::new()))
            }
            other => other.clone(),
        };
        serde_json::from_value::<Vec<Mod>>(seq).unwrap_or_else(|e| panic!("{}: mods: {e}", r.line))
    })
}

const MP2_LISTS: [&str; 3] = ["modFlagList", "preFlagList", "modTagList"];

fn tag_list(name: &str) -> &'static PatternList<TagVal> {
    match name {
        "preFlagList" => &preFlagList,
        _ => &modTagList,
    }
}

/// The Rust call in the driver's shape: preFlagList `f(unpack(preFlagCap))` gets the scan
/// captures as they are (scan sets `num = tonumber(cap1)`, unused by these closures);
/// modTagList gets `num = tonumber(c1)` only when `c1` holds a digit (CCR-0006 §5).
fn rust_call(name: &str, f: fn(&Caps) -> ModSpec, cap: &[String]) -> Result<ModSpec, String> {
    let mut caps = Caps::default();
    for (i, c) in cap.iter().take(5).enumerate() {
        caps.cap[i] = Some(c.as_str());
    }
    let c1 = cap.first().map(String::as_str);
    caps.num = match name {
        "preFlagList" => c1.and_then(pob_lua::tonumber),
        _ => c1
            .filter(|c| pob_lua::pattern::r#match(c, "%d+", None).is_some())
            .and_then(pob_lua::tonumber),
    };
    if name == "modTagList" && c1.is_none() {
        // the driver's `tagCap[1]:match` raises before the call
        return Err("attempt to index field '?' (a nil value)".to_owned());
    }
    pcall(|| f(&caps))
}

/// The Lua error text without its `chunk:line: ` position prefix.
fn strip_pos(e: &str) -> &str {
    match e.find(": ") {
        Some(i) if e[..i].contains(':') => &e[i + 2..],
        _ => e,
    }
}

// ---------------------------------------------------------------------------------------------
// mp2_oracle
// ---------------------------------------------------------------------------------------------

#[test]
fn mp2_oracle() {
    let Some(dir) = pob_testpaths::require(init_data(), "data/dump not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(pob_src) = pob_testpaths::require(find_pob_src(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let modparser = std::fs::read_to_string(pob_src.join("Modules/ModParser.lua")).unwrap();
    let srcl: Vec<&str> = modparser.lines().collect();
    // HYG2: a missing parse oracle used to print and drop L1 silently
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

    // -- sample captures per closure
    let mut samples: BTreeMap<&str, BTreeMap<&'static str, Vec<Vec<String>>>> = BTreeMap::new();
    let mut from_oracle: BTreeMap<(&str, &'static str), usize> = BTreeMap::new();
    for name in ["preFlagList", "modTagList"] {
        let l = tag_list(name);
        let m = samples.entry(name).or_default();
        let fns: BTreeSet<&'static str> =
            l.0.iter()
                .filter(|e| is_fn(&e.val))
                .map(|e| e.pat)
                .collect();
        if let Some(recs) = &records {
            let mut seen: BTreeMap<&'static str, BTreeSet<Vec<String>>> = BTreeMap::new();
            for r in recs {
                for p in &r.passes {
                    for st in &p.steps {
                        if st.list != name {
                            continue;
                        }
                        let Some(pat) = st.pattern.as_deref() else {
                            continue;
                        };
                        let Some(&k) = fns.get(pat) else {
                            continue;
                        };
                        let s = seen.entry(k).or_default();
                        if s.len() < 40 {
                            s.insert(st.caps.iter().map(cap_text).collect());
                        }
                    }
                }
            }
            for (k, s) in seen {
                from_oracle.insert((name, k), s.len());
                m.entry(k).or_default().extend(s);
            }
        }
        for k in &fns {
            let v = m.entry(k).or_default();
            for s in synthetic(k, 60) {
                if !v.contains(&s) {
                    v.push(s);
                }
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
    let mut code = String::from("local S = {\n");
    for (name, m) in &samples {
        code.push_str(&format!("{name} = {{\n"));
        for (k, tuples) in m {
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
        code.push_str("},\n");
    }
    code.push_str("}\nreturn __mp2_run(S)\n");
    let out: J = serde_json::from_str(&lua.eval(&code)).expect("lua json");

    let hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(|_| {}));

    // -- compare every entry
    let mut bad: Vec<String> = Vec::new();
    let (mut nStatic, mut nFn, mut nCalls, mut nErrCalls) = (0usize, 0usize, 0usize, 0usize);
    let mut perFnCalls: Vec<(String, usize)> = Vec::new();
    for name in MP2_LISTS {
        let lo = out[name].as_object().unwrap();
        let ours: Vec<(
            &'static str,
            Option<&'static TagVal>,
            Option<&'static ModSpec>,
        )> = if name == "modFlagList" {
            modFlagList
                .0
                .iter()
                .map(|e| (e.pat, None, Some(&e.val)))
                .collect()
        } else {
            tag_list(name)
                .0
                .iter()
                .map(|e| (e.pat, Some(&e.val), None))
                .collect()
        };
        assert_eq!(lo.len(), ours.len(), "{name}: PoB has {} keys", lo.len());
        for (pat, tv, spec) in ours {
            let Some(l) = lo.get(pat) else {
                bad.push(format!("{name} {pat:?}: not a key of PoB's table"));
                continue;
            };
            let kind = l["kind"].as_str().unwrap();
            let fnv = match (tv, spec) {
                (Some(TagVal::Fn(f)), _) => Some(*f),
                _ => None,
            };
            let st = match (tv, spec) {
                (Some(TagVal::Spec(s)), _) | (None, Some(s)) => Some(s),
                _ => None,
            };
            if let Some(s) = st {
                nStatic += 1;
                if kind != "table" {
                    bad.push(format!("{name} {pat:?}: PoB has a {kind}, ours is a table"));
                    continue;
                }
                let want = norm(&l["v"]);
                let got = spec_json(s);
                if want != got {
                    bad.push(format!("{name} {pat:?}: PoB {want} / ours {got}"));
                }
                continue;
            }
            let f = fnv.unwrap();
            nFn += 1;
            if kind != "function" {
                bad.push(format!(
                    "{name} {pat:?}: PoB has a {kind}, ours is a closure"
                ));
                continue;
            }
            let tuples = &samples[name][pat];
            let calls = l["calls"].as_array().map(Vec::as_slice).unwrap_or(&[]);
            assert_eq!(calls.len(), tuples.len(), "{name} {pat:?}: call count");
            perFnCalls.push((format!("{name}:{pat}"), calls.len()));
            for (cap, c) in tuples.iter().zip(calls) {
                nCalls += 1;
                let ours = rust_call(name, f, cap);
                match (c["ok"].as_bool().unwrap(), ours) {
                    (true, Ok(s)) => {
                        let want = norm(&c["v"]);
                        let got = spec_json(&s);
                        if want != got {
                            bad.push(format!(
                                "{name} {pat:?} caps {cap:?}: PoB {want} / ours {got}"
                            ));
                        }
                    }
                    (false, Err(e)) => {
                        nErrCalls += 1;
                        let le = c["err"].as_str().unwrap();
                        if strip_pos(le) != e {
                            bad.push(format!(
                                "{name} {pat:?} caps {cap:?}: PoB raises {le:?} / ours {e:?}"
                            ));
                        }
                    }
                    (true, Err(e)) => bad.push(format!(
                        "{name} {pat:?} caps {cap:?}: PoB returns {} / ours raises {e:?}",
                        c["v"]
                    )),
                    (false, Ok(s)) => bad.push(format!(
                        "{name} {pat:?} caps {cap:?}: PoB raises {} / ours {}",
                        c["err"],
                        spec_json(&s)
                    )),
                }
            }
        }
    }
    let minCalls = perFnCalls.iter().map(|(_, n)| *n).min().unwrap_or(0);
    let oracleFns = from_oracle.len();
    eprintln!(
        "entries: {nStatic} static tables, {nFn} closures; {nCalls} closure calls ({nErrCalls} raise in both), \
         >= {minCalls} per closure; {oracleFns} closures have captures recorded by PoB's own scans"
    );
    assert_eq!(nFn, 143, "closure count");
    assert!(minCalls > 0, "a closure was never called");

    // -- L1 over the lines whose parse used these lists
    if let Some(recs) = &records {
        l1(&mut lua, &srcl, recs, &mut bad);
    }
    std::panic::set_hook(hook);
    for b in bad.iter().take(60) {
        eprintln!("MISMATCH {b}");
    }
    assert!(bad.is_empty(), "{} mismatches", bad.len());
}

/// MP1's lists when merged, else PoB's (read from the LuaJIT state; test-only).
fn form_and_name_lists(
    lua: &mut Lua,
    src: &[&str],
) -> (
    &'static PatternList<Name>,
    &'static PatternList<ModSpec>,
    &'static str,
) {
    let f = &pob_modparser::form_list::formList;
    let n = &pob_modparser::mod_name_list::modNameList;
    if !f.is_empty() && !n.is_empty() {
        return (f, n, "MP1's");
    }
    let j: J = serde_json::from_str(&lua.eval(
        "return __mp2_enc({ formList = __mp2_lists.formList, modNameList = __mp2_lists.modNameList })",
    ))
    .unwrap();
    let mk = |lines: Vec<(String, u32)>| -> Vec<(&'static str, u32)> {
        lines
            .into_iter()
            .map(|(k, l)| (&*Box::leak(k.into_boxed_str()), l))
            .collect()
    };
    let fk = mk(literal_keys(src, 72, 158));
    let nk = mk(literal_keys(src, 161, 910));
    let fo = j["formList"].as_object().unwrap();
    let no = j["modNameList"].as_object().unwrap();
    assert_eq!(
        (fk.len(), nk.len()),
        (fo.len(), no.len()),
        "formList/modNameList sizes"
    );
    let form: PatternList<Name> = PatternList(
        fk.iter()
            .map(|(k, l)| PatternEntry {
                pat: k,
                val: Name::from(fo[*k].as_str().unwrap()),
                lua_line: *l,
            })
            .collect(),
    );
    let names: PatternList<ModSpec> = PatternList(
        nk.iter()
            .map(|(k, l)| PatternEntry {
                pat: k,
                val: spec_from_json(&norm(&no[*k]), k),
                lua_line: *l,
            })
            .collect(),
    );
    (
        Box::leak(Box::new(form)),
        Box::leak(Box::new(names)),
        "PoB's (rebuilt)",
    )
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

fn mods_eq(a: &Option<Vec<Mod>>, b: &Option<Vec<Mod>>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| mod_eq(p, q)),
        (None, None) => true,
        _ => false,
    }
}

fn l1(lua: &mut Lua, src: &[&str], recs: &[Record], bad: &mut Vec<String>) {
    let (formList, modNameList, which) = form_and_name_lists(lua, src);
    let statics = ParserLists::statics();
    let lists = ParserLists {
        formList,
        modNameList,
        ..statics
    };
    let stride: usize = std::env::var("MP2_L1_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let other_units = [
        "specialModList",
        "jewelFuncList",
        "clusterJewelSkills",
        "unsupportedModList",
    ];
    let sel: Vec<&Record> = recs
        .iter()
        .filter(|r| {
            let steps = || r.passes.iter().flat_map(|p| p.steps.iter());
            steps().any(|s| MP2_LISTS.contains(&s.list.as_str()) && s.pattern.is_some())
                && !steps().any(|s| {
                    s.exact || (other_units.contains(&s.list.as_str()) && s.pattern.is_some())
                })
        })
        .step_by(stride)
        .collect();
    let mut per_list: BTreeMap<&str, usize> = BTreeMap::new();
    for r in &sel {
        let used: BTreeSet<&str> = r
            .passes
            .iter()
            .flat_map(|p| p.steps.iter())
            .filter(|s| s.pattern.is_some())
            .filter_map(|s| MP2_LISTS.iter().find(|l| **l == s.list).copied())
            .collect();
        for l in used {
            *per_list.entry(l).or_default() += 1;
        }
    }
    let t = std::time::Instant::now();
    let (mut ok, mut nbad) = (0usize, 0usize);
    for r in &sel {
        let got = pcall(|| {
            let (m, e) = parseModPassWith(&lists, &r.line, 1);
            if m.is_some() && e.is_some() {
                parseModPassWith(&lists, &r.line, 2)
            } else {
                (m, e)
            }
        });
        let want = oracle_mods(r);
        match got {
            Ok((m, e)) if mods_eq(&m, &want) && e.as_deref() == r.extra.as_deref() => ok += 1,
            other => {
                nbad += 1;
                bad.push(format!(
                    "L1 {:?}: PoB {:?} / {:?}; ours {:?}",
                    r.line, want, r.extra, other
                ));
            }
        }
    }
    eprintln!(
        "L1 (formList/modNameList: {which}): {} lines whose parse used modFlagList/preFlagList/modTagList \
         (stride {stride}; per list {per_list:?}): {ok} identical, {nbad} differ, {:.1}s",
        sel.len(),
        t.elapsed().as_secs_f64()
    );
    // coverage (HYG2/V2): the checked share cannot shrink (MPFIX owns the exclusions above and
    // raises the floor)
    if stride == 1 {
        assert!(
            sel.len() >= MP2_MIN_L1,
            "{} L1 lines checked, expected at least {MP2_MIN_L1}",
            sel.len()
        );
    }
}

/// `data/dump/parse_oracle.jsonl` record count.
const PARSE_ORACLE_RECORDS: usize = 80_993;
/// L1 lines checked at stride 1; measured by HYG2 on rust-port 746b103.
const MP2_MIN_L1: usize = 26_904;
