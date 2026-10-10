//! Unit MP1 acceptance (calc doc §11.1, row MP1: ModParser.lua 72-912, `formList` and
//! `modNameList`).
//!
//! * `list_sizes_and_source_order`: both lists have PoB's sizes (the parse oracle manifest's
//!   `listSizes`: 85 and 724) and hold exactly the literal `["key"]` entries of ModParser.lua
//!   72-158 / 161-910, in source order, each with the line that defines it.
//! * `values_match_luajit`: the two table constructors are evaluated by LuaJIT (the vendored
//!   one pob-oracle links; `Data/Global.lua` supplies `ModFlag`/`KeywordFlag`/`SkillType`) and
//!   every value is compared with the Rust entry: a form string with the `Name`; a modNameList
//!   string or table (array part, `flags`, `keywordFlags`, `tag`, `tagList`, `addToMinion`,
//!   `addToSkill`, …) with the `ModSpec` (`modNameIsTable` = the value is a table).
//! * `l1_form_and_name_lines`: L1 on the parse-oracle lines whose parse goes through these two
//!   lists and no list of a unit that is still a stub (MP2-MP7: no `specialModList`,
//!   `preFlagList`, `modTagList` or `modFlagList` match, no jewel/cluster/unsupported exact
//!   key): `parseModNative` must return PoB's mods and remainder. Also reports which entries
//!   of the two lists no oracle line reaches. `MP1_STRIDE=n` checks every n-th line.
//!
//! Inputs: the PoB checkout, `data/dump` (data.bin, manifest.json, parse_oracle.jsonl); a test
//! skips when they are absent (fails with `POB_ORACLE_REQUIRE=1` or `CI`).
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;
use std::sync::Once;

use pob_gamedata::modifier::for_each_tag_field;
use pob_gamedata::{Mod, Tag, Value};
use pob_modparser::form_list::formList;
use pob_modparser::mod_name_list::modNameList;
use pob_modparser::types::ModSpec;
use pob_oracle::oracle::{find_pob_src, find_up};

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

/// Initialise `data()` once per test binary.
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

/// `["key"] = …` entries at the top level of a table constructor (one tab of indentation) in
/// lines `a..=b`, in source order; a duplicate key keeps its last line (Lua constructor
/// semantics) and its first position.
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<(String, u32)> {
    let mut by_key: BTreeMap<String, u32> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for ln in a..=b {
        let s = src[ln - 1];
        if !s.starts_with("\t[") || s.starts_with("\t\t") {
            continue;
        }
        let rest = &s[2..];
        let q = rest.as_bytes()[0];
        assert!(
            q == b'"' || q == b'\'',
            "ModParser.lua:{ln}: unexpected key {s}"
        );
        let body = &rest[1..];
        let end = body.find(q as char).unwrap();
        let key = &body[..end];
        assert!(!key.contains('\\'), "ModParser.lua:{ln}: escaped key");
        assert!(
            body[end + 1..].trim_start().starts_with(']'),
            "ModParser.lua:{ln}: {s}"
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

#[test]
fn list_sizes_and_source_order() {
    let Some(src) = modparser_src() else {
        return skip("PoB checkout not found");
    };
    let src: Vec<&str> = src.lines().collect();
    // the table constructors these ranges hold
    assert_eq!(src[72 - 1], "local formList = {");
    assert_eq!(src[158 - 1], "}");
    assert_eq!(src[161 - 1], "local modNameList = {");
    assert_eq!(src[910 - 1], "}");
    let want_form = literal_keys(&src, 73, 158);
    let want_name = literal_keys(&src, 162, 910);
    let got_form: Vec<(String, u32)> = formList
        .0
        .iter()
        .map(|e| (e.pat.to_owned(), e.lua_line))
        .collect();
    let got_name: Vec<(String, u32)> = modNameList
        .0
        .iter()
        .map(|e| (e.pat.to_owned(), e.lua_line))
        .collect();
    assert_eq!(got_form, want_form, "formList entries / order / lines");
    assert_eq!(got_name, want_name, "modNameList entries / order / lines");
    for l in [&got_form, &got_name] {
        assert!(l.windows(2).all(|w| w[0].1 < w[1].1), "source order");
    }
    assert_eq!(formList.len(), 85);
    assert_eq!(modNameList.len(), 724);
    if let Some(dir) = dump_dir() {
        let manifest: serde_json::Value =
            serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap())
                .unwrap();
        let sizes = &manifest["parseOracle"]["listSizes"];
        assert_eq!(sizes["formList"].as_u64(), Some(formList.len() as u64));
        assert_eq!(
            sizes["modNameList"].as_u64(),
            Some(modNameList.len() as u64)
        );
    } else {
        eprintln!("manifest.json not found: sizes checked against the literal keys only");
    }
    eprintln!(
        "formList {} entries, modNameList {} entries: keys, order and lines equal ModParser.lua",
        formList.len(),
        modNameList.len()
    );
}

// ---------------------------------------------------------------------------
// Values against LuaJIT
// ---------------------------------------------------------------------------

/// Canonical rendering shared by both sides: strings quoted, integers `%d`, booleans,
/// tables as `{array;key=value,...}` with the keys sorted.
const LUA_SER: &str = r#"
local function ser(v)
    local t = type(v)
    if t == "string" then
        assert(not v:find('"', 1, true))
        return '"' .. v .. '"'
    elseif t == "number" then
        assert(v == math.floor(v), "non-integer number")
        return string.format("%d", v)
    elseif t == "boolean" then
        return tostring(v)
    elseif t == "table" then
        local arr = { }
        for i = 1, #v do arr[i] = ser(v[i]) end
        local keys = { }
        for k in pairs(v) do
            if not (type(k) == "number" and k >= 1 and k <= #v and k == math.floor(k)) then
                assert(type(k) == "string", "non-string key")
                keys[#keys + 1] = k
            end
        end
        table.sort(keys)
        local kv = { }
        for _, k in ipairs(keys) do kv[#kv + 1] = k .. "=" .. ser(v[k]) end
        return "{" .. table.concat(arr, ",") .. ";" .. table.concat(kv, ",") .. "}"
    end
    error("unexpected " .. t)
end
return ser
"#;

fn ser_table(arr: Vec<String>, mut kv: Vec<(String, String)>) -> String {
    kv.sort();
    let kv: Vec<String> = kv.into_iter().map(|(k, v)| format!("{k}={v}")).collect();
    format!("{{{};{}}}", arr.join(","), kv.join(","))
}

fn ser_value(v: &Value) -> String {
    match v {
        Value::Str(s) => format!("\"{s}\""),
        Value::Num(x) => {
            assert_eq!(*x, x.floor(), "non-integer number");
            format!("{}", *x as i64)
        }
        Value::Bool(b) => b.to_string(),
        Value::Table(t) => ser_table(
            t.array.iter().map(ser_value).collect(),
            t.fields
                .iter()
                .map(|(k, v)| (k.to_string(), ser_value(v)))
                .collect(),
        ),
        other => panic!("unexpected value {other:?}"),
    }
}

fn ser_tag(t: &Tag) -> String {
    let mut kv = Vec::new();
    for_each_tag_field(t, |k, v| kv.push((k.to_owned(), ser_value(&v))));
    ser_table(Vec::new(), kv)
}

fn ser_name(n: pob_gamedata::Name) -> String {
    format!("\"{n}\"")
}

/// A modNameList value as Lua sees it: a string for a one-name string entry, else the table.
fn ser_spec(s: &ModSpec) -> String {
    let names = s.modName.as_deref().unwrap_or(&[]);
    if !s.modNameIsTable {
        assert_eq!(names.len(), 1, "string entry with {} names", names.len());
        let mut bare = s.clone();
        bare.modName = None;
        assert_eq!(
            ser_spec_fields(&bare),
            Vec::<(String, String)>::new(),
            "string entry with other fields"
        );
        return ser_name(names[0]);
    }
    ser_table(
        names.iter().map(|n| ser_name(*n)).collect(),
        ser_spec_fields(s),
    )
}

fn ser_spec_fields(s: &ModSpec) -> Vec<(String, String)> {
    let mut kv: Vec<(String, String)> = Vec::new();
    let mut put = |k: &str, v: String| kv.push((k.to_owned(), v));
    if s.flags != 0 {
        put("flags", s.flags.to_string());
    }
    if s.keywordFlags != 0 {
        put("keywordFlags", s.keywordFlags.to_string());
    }
    if let Some(t) = &s.tag {
        put("tag", ser_tag(t));
    }
    if !s.tagList.is_empty() {
        put(
            "tagList",
            ser_table(s.tagList.iter().map(ser_tag).collect(), Vec::new()),
        );
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
            put(k, "true".to_owned());
        }
    }
    if let Some(t) = &s.addToMinionTag {
        put("addToMinionTag", ser_tag(t));
    }
    if let Some(t) = &s.addToSkill {
        put("addToSkill", ser_tag(t));
    }
    if let Some(t) = &s.playerTag {
        put("playerTag", ser_tag(t));
    }
    if !s.playerTagList.is_empty() {
        put(
            "playerTagList",
            ser_table(s.playerTagList.iter().map(ser_tag).collect(), Vec::new()),
        );
    }
    if let Some(n) = &s.modSuffix {
        put("modSuffix", ser_name(*n));
    }
    if let Some(t) = &s.modType {
        put("modType", format!("\"{}\"", t.as_str()));
    }
    assert!(s.entries.is_empty(), "modNameList entry with `entries`");
    for (k, v) in &s.extra {
        put(k.as_str(), ser_value(v));
    }
    kv
}

#[test]
fn values_match_luajit() {
    use pob_oracle::mlua::{Function, Lua, Table, Value as LV};
    let Some(src_dir) = find_pob_src() else {
        return skip("PoB checkout not found");
    };
    let modparser = std::fs::read_to_string(src_dir.join("Modules/ModParser.lua")).unwrap();
    let lines: Vec<&str> = modparser.lines().collect();
    let global = std::fs::read_to_string(src_dir.join("Data/Global.lua")).unwrap();

    let lua = Lua::new();
    // Common.lua's copyTable is the only helper Global.lua calls while loading
    lua.load(
        "function copyTable(t) local o = { } for k, v in pairs(t) do o[k] = v end return o end",
    )
    .exec()
    .unwrap();
    lua.load(&global)
        .set_name("@Data/Global.lua")
        .exec()
        .unwrap();
    // lines 1-910: the file's local aliases (bor, ...) and the two table constructors;
    // nothing else in that range runs code
    let chunk = format!(
        "{}\nreturn formList, modNameList\n",
        lines[..910].join("\n")
    );
    let (lform, lname): (Table, Table) = lua
        .load(&chunk)
        .set_name("@Modules/ModParser.lua")
        .eval()
        .unwrap();
    let ser: Function = lua.load(LUA_SER).eval().unwrap();

    let count = |t: &Table| t.pairs::<LV, LV>().count();
    assert_eq!(count(&lform), formList.len(), "formList key count");
    assert_eq!(count(&lname), modNameList.len(), "modNameList key count");

    let mut bad = Vec::new();
    for e in &formList.0 {
        let v: Option<String> = lform.get(e.pat).unwrap();
        if v.as_deref() != Some(e.val.as_str()) {
            bad.push(format!(
                "formList[{:?}] (line {}): Lua {v:?}, Rust {}",
                e.pat, e.lua_line, e.val
            ));
        }
    }
    let (mut tables, mut strings) = (0usize, 0usize);
    for e in &modNameList.0 {
        let v: LV = lname.get(e.pat).unwrap();
        if matches!(v, LV::Table(_)) {
            tables += 1;
        } else {
            strings += 1;
        }
        let want: String = ser.call(v).unwrap();
        let got = ser_spec(&e.val);
        if want != got {
            bad.push(format!(
                "modNameList[{:?}] (line {}):\n  Lua  {want}\n  Rust {got}",
                e.pat, e.lua_line
            ));
        }
    }
    for b in &bad {
        eprintln!("MISMATCH {b}");
    }
    eprintln!(
        "values vs LuaJIT: formList {} entries, modNameList {} entries ({strings} strings, {tables} tables), {} mismatches",
        formList.len(),
        modNameList.len(),
        bad.len()
    );
    assert!(
        bad.is_empty(),
        "{} values differ from ModParser.lua",
        bad.len()
    );
}

// ---------------------------------------------------------------------------
// L1 on the lines whose form and name come from these lists
// ---------------------------------------------------------------------------

#[derive(serde::Deserialize)]
struct Record {
    line: String,
    mods: Option<serde_json::Value>,
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
    exact: bool,
}

fn oracle_mods(r: &Record) -> Option<Vec<Mod>> {
    r.mods.as_ref().map(|v| {
        let seq = match v {
            serde_json::Value::Object(o) => {
                assert!(
                    o.keys().all(|k| k == "$seq"),
                    "{}: mods table with extra keys",
                    r.line
                );
                o.get("$seq")
                    .cloned()
                    .unwrap_or(serde_json::Value::Array(Vec::new()))
            }
            other => other.clone(),
        };
        serde_json::from_value::<Vec<Mod>>(seq).unwrap_or_else(|e| panic!("{}: mods: {e}", r.line))
    })
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
        && a.tags == b.tags
}

fn mods_eq(a: Option<&[Mod]>, b: Option<&[Mod]>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => x.len() == y.len() && x.iter().zip(y).all(|(p, q)| mod_eq(p, q)),
        (None, None) => true,
        _ => false,
    }
}

/// Lists of units that are still stubs (or whose closures this test cannot reach).
const STUB_LISTS: [&str; 4] = ["specialModList", "preFlagList", "modTagList", "modFlagList"];
const STUB_EXACT: [&str; 3] = ["jewelFuncList", "clusterJewelSkills", "unsupportedModList"];

#[test]
fn l1_form_and_name_lines() {
    let Some(dir) = init_data() else {
        return skip("data/dump/data.bin not found");
    };
    let Ok(text) = std::fs::read_to_string(dir.join("parse_oracle.jsonl")) else {
        return skip("data/dump/parse_oracle.jsonl not found");
    };
    let stride: usize = std::env::var("MP1_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let t = std::time::Instant::now();
    let mut hit_form: BTreeSet<u32> = BTreeSet::new();
    let mut hit_name: BTreeSet<u32> = BTreeSet::new();
    let (mut total, mut checked, mut ok) = (0usize, 0usize, 0usize);
    let mut per_entry_checked: (BTreeSet<u32>, BTreeSet<u32>) = Default::default();
    let mut bad = Vec::new();
    // one thread, in the oracle's (sorted) line order: the DOUBLED form's write into a shared
    // modNameList table persists across parses here as it does in PoB's one Lua state
    for (i, l) in text.lines().enumerate() {
        let r: Record = serde_json::from_str(l).unwrap_or_else(|e| panic!("{e}: {l}"));
        total += 1;
        let mut uses_stub = false;
        let mut lines_f = Vec::new();
        let mut lines_n = Vec::new();
        for p in &r.passes {
            for s in &p.steps {
                if STUB_EXACT.contains(&s.list.as_str()) || s.exact {
                    uses_stub = true;
                }
                if STUB_LISTS.contains(&s.list.as_str()) && s.pattern.is_some() {
                    uses_stub = true;
                }
                if s.pattern.is_some() {
                    match s.list.as_str() {
                        "formList" => lines_f.push(s.line.unwrap()),
                        "modNameList" => lines_n.push(s.line.unwrap()),
                        _ => {}
                    }
                }
            }
        }
        hit_form.extend(&lines_f);
        hit_name.extend(&lines_n);
        if uses_stub || (lines_f.is_empty() && lines_n.is_empty()) || i % stride != 0 {
            continue;
        }
        checked += 1;
        per_entry_checked.0.extend(&lines_f);
        per_entry_checked.1.extend(&lines_n);
        let want = oracle_mods(&r);
        let (mods, extra) = pob_modparser::parseModNative(&r.line);
        if mods_eq(mods.as_deref(), want.as_deref()) && extra == r.extra {
            ok += 1;
        } else if bad.len() < 30 {
            bad.push(format!(
                "{:?}:\n  PoB  {want:?} / {:?}\n  Rust {mods:?} / {extra:?}",
                r.line, r.extra
            ));
        } else {
            bad.push(String::new());
        }
    }
    for b in bad.iter().filter(|b| !b.is_empty()) {
        eprintln!("MISMATCH {b}");
    }
    let unhit_form: Vec<u32> = formList
        .0
        .iter()
        .map(|e| e.lua_line)
        .filter(|l| !hit_form.contains(l))
        .collect();
    let unhit_name: Vec<u32> = modNameList
        .0
        .iter()
        .map(|e| e.lua_line)
        .filter(|l| !hit_name.contains(l))
        .collect();
    eprintln!(
        "L1 (form/name lines without stub lists): {checked} of {total} oracle lines checked (stride {stride}), {ok} identical, {} differ, {:.1}s",
        checked - ok,
        t.elapsed().as_secs_f64()
    );
    eprintln!(
        "entries reached by the checked lines: formList {}/{}, modNameList {}/{}",
        per_entry_checked.0.len(),
        formList.len(),
        per_entry_checked.1.len(),
        modNameList.len()
    );
    eprintln!(
        "entries reached by any oracle line: formList {}/{}, modNameList {}/{}",
        formList.len() - unhit_form.len(),
        formList.len(),
        modNameList.len() - unhit_name.len(),
        modNameList.len()
    );
    eprintln!("formList entries no oracle line reaches (ModParser.lua lines): {unhit_form:?}");
    eprintln!("modNameList entries no oracle line reaches (ModParser.lua lines): {unhit_name:?}");
    assert_eq!(ok, checked, "{} L1 lines differ from PoB", checked - ok);
}
