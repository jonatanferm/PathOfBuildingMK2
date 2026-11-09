//! Unit MP0 acceptance (calc doc §11.1, row MP0: ModParser core).
//!
//! * `modcache_tier_serves_cached_lines`: `parseMod` returns, for every line of
//!   `Data/ModCache.lua` (`data().modCache`, 23,207 lines), exactly the cached mods and remainder
//!   (fresh copies); a line outside the cache is parsed natively once and memoised.
//! * `scan_matches_parse_oracle`: every `scan()` step PoB recorded in
//!   `data/dump/parse_oracle.jsonl` (both passes of every record) is replayed through
//!   `pob_modparser::scan` and must pick the same pattern (same defining Lua line) with the same
//!   captures; full ties (none at the pin) accept any recorded candidate. MP0's generated lists
//!   (specialModList keystones/anchoring/skill entries, skillNameList, preSkillNameList,
//!   suffix/pen/cost/flag types) are the real ones; the literal lists of MP1-MP6 (still stubs)
//!   are rebuilt from the keys of ModParser.lua. `MP0_STRIDE=n` replays every n-th record.
//! * `generated_list_sizes`: the generated tables have PoB's sizes (the parse oracle manifest's
//!   `listSizes`).
//! * `helpers_match_parse_oracle`: `explodeFunc`, `grantedExtraSkill`, `triggerExtraSkill`,
//!   `extraSupport` and `dealNoNonDamageType`, called as the specialModList closures that use
//!   them call them (captures from the oracle), against the mods PoB produced; and the generated
//!   keystone / skill specialModList entries through `parseModNative`. Mod names, types, flags
//!   and counts are always compared; values and tags only once `createMod` (unit CT) is
//!   ported (until then it is a stub that drops them).
//!
//! Inputs: `data/dump` (data.bin, manifest.json, parse_oracle.jsonl) and the PoB checkout;
//! a test fails when they are absent (`pob_testpaths::require`; `POB_ALLOW_SKIP=1` skips).

use std::collections::BTreeMap;
use std::path::PathBuf;
use std::sync::Once;

use pob_gamedata::{data, n, Mod, ModType, Name, Tag, Value};
use pob_modparser::generated_lists as gl;
use pob_modparser::helpers;
use pob_modparser::scan::ScanCaps;
use pob_modparser::types::{PatternEntry, PatternList, SpecialVal};
use pob_oracle::oracle::{find_pob_src, find_up};

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
        let t = std::time::Instant::now();
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
        eprintln!("data.bin load: {:.2}s", t.elapsed().as_secs_f64());
    });
    Some(dir)
}

// ---------------------------------------------------------------------------
// Value comparison (Lua tables have no key order; numbers within 1e-9 relative)
// ---------------------------------------------------------------------------

fn num_eq(a: f64, b: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan()) || (a - b).abs() <= 1e-9 * a.abs().max(b.abs()).max(1.0)
}

fn value_eq(a: &Value, b: &Value) -> bool {
    match (a, b) {
        (Value::Num(x), Value::Num(y)) => num_eq(*x, *y),
        (Value::Mod(x), Value::Mod(y)) => mod_eq(x, y, true),
        (Value::Table(x), Value::Table(y)) => {
            x.array.len() == y.array.len()
                && x.array.iter().zip(&y.array).all(|(p, q)| value_eq(p, q))
                && x.fields.len() == y.fields.len()
                && x.fields.iter().all(|(k, v)| value_eq(v, y.get(*k)))
        }
        _ => a == b,
    }
}

fn tag_eq(a: &Tag, b: &Tag) -> bool {
    // tags hold parsed literals and captures, so they compare exactly (`Tag: PartialEq`)
    a == b
}

/// `strict`: everything; else name, type, flags and keywordFlags only.
fn mod_eq(a: &Mod, b: &Mod, strict: bool) -> bool {
    let base = a.name == b.name
        && a.r#type == b.r#type
        && a.flags == b.flags
        && a.keywordFlags == b.keywordFlags;
    if !strict {
        return base;
    }
    base && value_eq(&a.value, &b.value)
        && a.source == b.source
        && a.tags.len() == b.tags.len()
        && a.tags.iter().zip(&b.tags).all(|(x, y)| tag_eq(x, y))
}

fn mods_eq(a: &Option<Vec<Mod>>, b: &Option<Vec<Mod>>, strict: bool) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| mod_eq(p, q, strict))
        }
        (None, None) => true,
        _ => false,
    }
}

/// Whether unit CT's `createMod` is ported (the U0 stub drops value, source and tags).
fn create_mod_ported() -> bool {
    let m = pob_gamedata::mod_tools::createMod(n!("X"), ModType::BASE, 1.0, None, 0, 0, &[]);
    m.value == Value::Num(1.0)
}

// ---------------------------------------------------------------------------
// The parse oracle
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
    #[allow(dead_code)]
    order: u8,
    steps: Vec<Step>,
}

#[derive(serde::Deserialize, Clone)]
struct Step {
    list: String,
    pattern: Option<String>,
    line: Option<u32>,
    #[serde(default)]
    caps: Vec<serde_json::Value>,
    #[serde(default)]
    exact: bool,
    #[serde(default)]
    ties: Vec<Tie>,
}

#[derive(serde::Deserialize, Clone)]
struct Tie {
    pattern: String,
    #[allow(dead_code)]
    line: u32,
}

fn load_oracle(dir: &std::path::Path) -> Option<Vec<Record>> {
    let text = std::fs::read_to_string(dir.join("parse_oracle.jsonl")).ok()?;
    Some(
        text.lines()
            .map(|l| serde_json::from_str::<Record>(l).unwrap_or_else(|e| panic!("{e}: {l}")))
            .collect(),
    )
}

fn oracle_mods(r: &Record) -> Option<Vec<Mod>> {
    r.mods.as_ref().map(|v| {
        // a table that is not a plain 1..n sequence is an object: `{}` (no mods) or `$seq` + keys
        let seq = match v {
            serde_json::Value::Object(o) => {
                assert!(
                    o.keys().all(|k| k == "$seq"),
                    "{}: mods table with extra keys: {v}",
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

fn cap_text(v: &serde_json::Value) -> String {
    match v {
        serde_json::Value::String(s) => s.clone(),
        other => other.to_string(),
    }
}

// ---------------------------------------------------------------------------
// The literal pattern lists of MP1-MP6, rebuilt from ModParser.lua's keys
// ---------------------------------------------------------------------------

/// `["key"] = …` entries at the top level of a table constructor (one tab of indentation) in
/// lines `a..=b`; a duplicate key keeps its last line (Lua constructor semantics).
fn literal_keys(src: &[&str], a: usize, b: usize) -> Vec<(String, u32)> {
    let mut by_key: BTreeMap<String, u32> = BTreeMap::new();
    let mut order: Vec<String> = Vec::new();
    for ln in a..=b {
        let s = src[ln - 1];
        // one tab of indentation (ModParser.lua:5680 is indented with spaces instead)
        let indent = &s[..s.len() - s.trim_start().len()];
        if !(indent == "\t" || (!indent.is_empty() && indent.bytes().all(|b| b == b' '))) {
            continue;
        }
        let Some(rest) = s.trim_start().strip_prefix('[') else {
            continue;
        };
        if !rest.starts_with('"') && !rest.starts_with('\'') {
            continue;
        }
        let q = rest.as_bytes()[0];
        assert!(
            q == b'"' || q == b'\'',
            "ModParser.lua:{ln}: unexpected key {s}"
        );
        let body = &rest[1..];
        let end = body.find(q as char).unwrap();
        let key = &body[..end];
        assert!(!key.contains('\\'), "ModParser.lua:{ln}: escaped key");
        let after = body[end + 1..].trim_start();
        assert!(after.starts_with(']'), "ModParser.lua:{ln}: {s}");
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

fn leak_list<T: Send + Sync + 'static>(
    keys: &[(String, u32)],
    mut val: impl FnMut() -> T,
) -> &'static PatternList<T> {
    Box::leak(Box::new(PatternList(
        keys.iter()
            .map(|(k, l)| PatternEntry {
                pat: Box::leak(k.clone().into_boxed_str()),
                val: val(),
                lua_line: *l,
            })
            .collect(),
    )))
}

/// One scannable list: the entries' (pattern, line) plus `plain`.
struct TestList {
    scan: Box<dyn Fn(&str) -> Option<(&'static str, u32, String, ScanCaps)> + Send + Sync>,
    len: usize,
}

fn test_list<T: Send + Sync + 'static>(l: &'static PatternList<T>, plain: bool) -> TestList {
    // pattern + line of the winner: `scan` returns the value; find its entry by address
    let index: std::collections::HashMap<usize, (&'static str, u32)> =
        l.0.iter()
            .map(|e| (&e.val as *const T as usize, (e.pat, e.lua_line)))
            .collect();
    TestList {
        scan: Box::new(move |line: &str| {
            let (v, rest, caps) = pob_modparser::scan(line, l, plain)?;
            let (p, ln) = index[&(v as *const T as usize)];
            Some((p, ln, rest, caps))
        }),
        len: l.len(),
    }
}

fn build_lists(src: &[&str]) -> BTreeMap<&'static str, TestList> {
    let mut m = BTreeMap::new();
    m.insert(
        "formList",
        test_list(leak_list(&literal_keys(src, 72, 158), || ()), false),
    );
    m.insert(
        "modNameList",
        test_list(leak_list(&literal_keys(src, 161, 910), || ()), true),
    );
    m.insert(
        "modFlagList",
        test_list(leak_list(&literal_keys(src, 913, 1099), || ()), true),
    );
    m.insert(
        "preFlagList",
        test_list(leak_list(&literal_keys(src, 1102, 1317), || ()), false),
    );
    m.insert(
        "modTagList",
        test_list(leak_list(&literal_keys(src, 1320, 2009), || ()), false),
    );
    // specialModList: the literal keys (2130-5942) through MP0's keystone/anchoring code, then
    // MP0's generated skill entries
    let literal: Vec<PatternEntry<SpecialVal>> = literal_keys(src, 2130, 5942)
        .into_iter()
        .map(|(k, l)| PatternEntry {
            pat: Box::leak(k.into_boxed_str()),
            val: SpecialVal::Mods(Vec::new()),
            lua_line: l,
        })
        .collect();
    let mut t = gl::build_special_mod_list(literal);
    for (k, l) in gl::generatedSkillSpecialKeys() {
        t.set(k, SpecialVal::Mods(Vec::new()), l);
    }
    let special: &'static PatternList<SpecialVal> = Box::leak(Box::new(t.finish()));
    m.insert("specialModList", test_list(special, false));
    m.insert("preSkillNameList", test_list(*gl::preSkillNameList, false));
    m.insert("skillNameList", test_list(*gl::skillNameList, false));
    m.insert("penTypes", test_list(&gl::penTypes, true));
    m.insert("costTypes", test_list(&gl::costTypes, true));
    m.insert("baseCostTypes", test_list(&gl::baseCostTypes, true));
    m.insert("flagTypes", test_list(&gl::flagTypes, false));
    m.insert("suffixTypes", test_list(&gl::suffixTypes, true));
    m
}

fn pob_src_lines() -> Option<String> {
    let src = find_pob_src()?;
    std::fs::read_to_string(src.join("Modules/ModParser.lua")).ok()
}

// ---------------------------------------------------------------------------
// Tests
// ---------------------------------------------------------------------------

#[test]
fn modcache_tier_serves_cached_lines() {
    let Some(dir) = pob_testpaths::require(
        init_data(),
        "data/dump/data.bin not found (run pob-dump --compile)",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    let cache = &data().modCache;
    assert_eq!(cache.len(), 23207, "ModCache lines");
    let (mut ok, mut bad) = (0usize, 0usize);
    let mut fails = Vec::new();
    for (line, e) in cache {
        let want_mods: Option<Vec<Mod>> = e
            .mods
            .as_ref()
            .map(|v| v.iter().map(|m| (**m).clone()).collect());
        let want_extra = e.extra.as_deref().map(str::to_owned);
        let (mods, extra) = pob_modparser::parseMod(line, false);
        // exact equality: the tier hands back the cached values unchanged
        if mods == want_mods && extra == want_extra {
            ok += 1;
        } else {
            bad += 1;
            if fails.len() < 10 {
                fails.push(format!("{line:?}: got {mods:?} / {extra:?}"));
            }
        }
    }
    // fresh copies: mutating a result does not reach the cache
    let line = cache
        .iter()
        .find(|(_, e)| e.mods.as_ref().is_some_and(|m| !m.is_empty()))
        .map(|(l, _)| l.to_string())
        .unwrap();
    let (mut first, _) = pob_modparser::parseMod(&line, false);
    first.as_mut().unwrap()[0].name = n!("Mutated");
    let (again, _) = pob_modparser::parseMod(&line, false);
    assert_ne!(
        again.unwrap()[0].name,
        n!("Mutated"),
        "parseMod must return copies"
    );
    // a line outside the cache is parsed natively and memoised
    let novel = "this line is not in the mod cache";
    assert!(!pob_modparser::mod_cache::parseModCacheContains(novel));
    let r1 = pob_modparser::parseMod(novel, false);
    assert!(pob_modparser::mod_cache::parseModCacheContains(novel));
    let r2 = pob_modparser::parseMod(novel, false);
    assert_eq!(r1, r2);

    // informational: the cache against PoB's native parse of the same lines
    if let Some(records) = load_oracle(&dir) {
        let (mut same, mut differ) = (0usize, 0usize);
        for r in &records {
            let Some(e) = cache.get(r.line.as_str()) else {
                continue;
            };
            let want: Option<Vec<Mod>> = e
                .mods
                .as_ref()
                .map(|v| v.iter().map(|m| (**m).clone()).collect());
            if mods_eq(&want, &oracle_mods(r), true) && e.extra.as_deref() == r.extra.as_deref() {
                same += 1;
            } else {
                differ += 1;
                if differ <= 10 {
                    eprintln!(
                        "  ModCache entry differs from PoB's native parse: {:?}",
                        r.line
                    );
                }
            }
        }
        eprintln!("ModCache vs native PoB parse: {same} identical, {differ} differ");
    }
    for f in &fails {
        eprintln!("{f}");
    }
    eprintln!(
        "ModCache tier: {ok}/{} lines served exactly, {bad} differ",
        ok + bad
    );
    assert_eq!(bad, 0, "ModCache tier must serve 100% of cached lines");
}

#[test]
fn generated_list_sizes() {
    let Some(dir) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(src) = pob_testpaths::require(pob_src_lines(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let src: Vec<&str> = src.lines().collect();
    let manifest: serde_json::Value =
        serde_json::from_str(&std::fs::read_to_string(dir.join("manifest.json")).unwrap()).unwrap();
    let sizes = &manifest["parseOracle"]["listSizes"];
    let lists = build_lists(&src);
    let mut checked = 0;
    for (name, l) in &lists {
        let want = sizes[*name]
            .as_u64()
            .unwrap_or_else(|| panic!("no size for {name}")) as usize;
        assert_eq!(l.len, want, "{name} size");
        checked += 1;
    }
    let want = |k: &str| sizes[k].as_u64().unwrap() as usize;
    assert_eq!(gl::resourceTypes.len(), want("resourceTypes"));
    assert_eq!(gl::regenTypes.len(), want("regenTypes"));
    assert_eq!(gl::degenTypes.len(), want("degenTypes"));
    assert_eq!(gl::unsupportedModListLen, want("unsupportedModList"));
    assert_eq!(
        ["physical", "lightning", "cold", "fire", "chaos"]
            .iter()
            .filter(|k| helpers::dmgTypes(k).is_some())
            .count(),
        want("dmgTypes")
    );
    assert_eq!(helpers::conquerorList.len(), 28);
    eprintln!(
        "generated list sizes: {} lists + 5 tables match the oracle manifest",
        checked
    );
}

#[test]
fn scan_matches_parse_oracle() {
    let Some(dir) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(src) = pob_testpaths::require(pob_src_lines(), "PoB checkout not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(records) =
        pob_testpaths::require(load_oracle(&dir), "data/dump/parse_oracle.jsonl not found")
    else {
        return; // POB_ALLOW_SKIP=1
    };
    let src: Vec<&str> = src.lines().collect();
    let lists = build_lists(&src);
    let stride: usize = std::env::var("MP0_STRIDE")
        .ok()
        .and_then(|s| s.parse().ok())
        .unwrap_or(1)
        .max(1);
    let recs: Vec<&Record> = records.iter().step_by(stride).collect();
    let t = std::time::Instant::now();

    #[derive(Default)]
    struct Tally {
        steps: usize,
        matched: usize,
        no_match_ok: usize,
        ties: usize,
        bad: Vec<String>,
        per_list: BTreeMap<String, usize>,
    }
    let threads = 3usize;
    let chunk = recs.len().div_ceil(threads);
    let tallies: Vec<Tally> = std::thread::scope(|s| {
        let hs: Vec<_> = recs
            .chunks(chunk.max(1))
            .map(|part| {
                let lists = &lists;
                s.spawn(move || {
                    let mut t = Tally::default();
                    for r in part {
                        for p in &r.passes {
                            let mut line = r.line.clone();
                            let n = p.steps.len();
                            for (si, st) in p.steps.iter().enumerate() {
                                if st.exact || st.list == "jewelFuncList" {
                                    continue;
                                }
                                let Some(l) = lists.get(st.list.as_str()) else {
                                    t.bad.push(format!("{:?}: unknown list {}", r.line, st.list));
                                    break;
                                };
                                t.steps += 1;
                                *t.per_list.entry(st.list.clone()).or_default() += 1;
                                let got = (l.scan)(&line);
                                let ok = match (&got, &st.pattern) {
                                    (None, None) => {
                                        t.no_match_ok += 1;
                                        true
                                    }
                                    (Some((pat, ln, _, caps)), Some(want)) => {
                                        let want_caps: Vec<String> =
                                            st.caps.iter().map(cap_text).collect();
                                        let got_caps: Vec<String> =
                                            caps.cap.iter().flatten().cloned().collect();
                                        if !st.ties.is_empty() {
                                            t.ties += 1;
                                            st.ties.iter().any(|c| c.pattern == *pat)
                                        } else {
                                            *pat == want.as_str()
                                                && Some(*ln) == st.line
                                                && got_caps == want_caps
                                        }
                                    }
                                    _ => false,
                                };
                                if !ok {
                                    t.bad.push(format!(
                                        "{:?} [{}] on {:?}: oracle {:?} (line {:?}, caps {:?}), got {:?}",
                                        r.line,
                                        st.list,
                                        line,
                                        st.pattern,
                                        st.line,
                                        st.caps,
                                        got.as_ref().map(|(p, l, _, c)| (p.to_string(), *l, c.cap.clone()))
                                    ));
                                    break;
                                }
                                t.matched += 1;
                                if st.list == "specialModList" {
                                    // the driver goes on with `line .. " "` (6675)
                                    if si + 1 < n {
                                        line.push(' ');
                                    }
                                } else if let Some((_, _, rest, _)) = got {
                                    line = rest;
                                }
                            }
                        }
                    }
                    t
                })
            })
            .collect();
        hs.into_iter().map(|h| h.join().unwrap()).collect()
    });
    let mut total = Tally::default();
    for t in tallies {
        total.steps += t.steps;
        total.matched += t.matched;
        total.no_match_ok += t.no_match_ok;
        total.ties += t.ties;
        total.bad.extend(t.bad);
        for (k, v) in t.per_list {
            *total.per_list.entry(k).or_default() += v;
        }
    }
    for b in total.bad.iter().take(40) {
        eprintln!("MISMATCH {b}");
    }
    eprintln!(
        "scan vs parse oracle: {} records (stride {stride}), {} scan steps, {} identical ({} no-match), {} full ties, {} mismatches, {:.1}s",
        recs.len(),
        total.steps,
        total.matched,
        total.no_match_ok,
        total.ties,
        total.bad.len(),
        t.elapsed().as_secs_f64()
    );
    eprintln!("steps per list: {:?}", total.per_list);
    assert!(
        total.bad.is_empty(),
        "{} scan steps differ from PoB",
        total.bad.len()
    );
    // coverage (HYG2/V2): the whole oracle was replayed
    assert_eq!(
        records.len(),
        PARSE_ORACLE_RECORDS,
        "parse_oracle.jsonl records"
    );
    if stride == 1 {
        assert!(
            total.steps >= MP0_MIN_SCAN_STEPS,
            "{} scan steps, expected at least {MP0_MIN_SCAN_STEPS}",
            total.steps
        );
    }
}

/// `data/dump/parse_oracle.jsonl` record count.
const PARSE_ORACLE_RECORDS: usize = 80_993;
/// Scan steps replayed at stride 1; measured by HYG2 on rust-port 746b103.
const MP0_MIN_SCAN_STEPS: usize = 594_306;
/// `helpers_match_parse_oracle`: `(specialModList helper lines, generated keystone/skill lines)`
/// compared; measured by HYG2 on rust-port 746b103.
const MP0_MIN_HELPERS: (usize, usize) = (2129, 103);

/// A specialModList closure that calls a helper, as `(key line, call)`.
type HelperCall = fn(&ScanCaps) -> Option<Vec<Mod>>;

fn c(caps: &ScanCaps, i: usize) -> &str {
    caps.cap[i].as_deref().unwrap_or("")
}

fn tn(s: &str) -> Option<f64> {
    pob_lua::tonumber(s)
}

fn helper_calls() -> Vec<(u32, HelperCall)> {
    use helpers::{
        dealNoNonDamageType, explodeFunc, extraSupport, grantedExtraSkill, triggerExtraSkill,
        LuaArg, TriggerExtraSkillOptions,
    };
    use pob_gamedata::tag;
    // function(chance, _, amount, type) return explodeFunc(chance, amount, type, …) end
    macro_rules! ex4 {
        ($line:expr $(, $t:expr)*) => {
            ($line, (|k: &ScanCaps| explodeFunc(k.num, LuaArg::Str(c(k, 1)), c(k, 2), &[$($t),*])) as HelperCall)
        };
    }
    // function(amount, _, type) return explodeFunc(100, amount, type, …) end
    macro_rules! ex3 {
        ($line:expr $(, $t:expr)*) => {
            ($line, (|k: &ScanCaps| explodeFunc(Some(100.0), k.num.map_or(LuaArg::Nil, LuaArg::Num), c(k, 1), &[$($t),*])) as HelperCall)
        };
    }
    // function(num, _, skill) return triggerExtraSkill(skill, num) end
    macro_rules! trig {
        ($line:expr) => {
            (
                $line,
                (|k: &ScanCaps| Some(triggerExtraSkill(c(k, 1), k.num, None))) as HelperCall,
            )
        };
    }
    let mut v: Vec<(u32, HelperCall)> = vec![
        ex4!(2132),
        ex4!(2135),
        ex4!(2138),
        ex4!(2141, tag!({ type = Condition, var = "AffectedByPride" })),
        (2144, |k| {
            explodeFunc(
                k.num,
                LuaArg::Str(c(k, 1)),
                "randomElement",
                &[tag!({ type = Condition, var = "UsingFlask" })],
            )
        }),
        ex4!(
            2147,
            tag!({ type = Condition, var = "AffectedByGloriousMadness" })
        ),
        ex4!(2150),
        ex4!(2153, tag!({ type = Condition, var = "UsingWand" })),
        ex4!(
            2156,
            tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })
        ),
        ex3!(2159),
        ex3!(2162),
        ex3!(
            2165,
            tag!({ type = ActorCondition, actor = "enemy", var = "OnFungalGround" })
        ),
        ex3!(2168),
        ex3!(
            2171,
            tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })
        ),
        ex3!(
            2174,
            tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })
        ),
        ex3!(
            2177,
            tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })
        ),
        ex3!(
            2180,
            tag!({ type = ActorCondition, actor = "enemy", var = "Burning" })
        ),
        ex3!(
            2185,
            tag!({ type = MultiplierThreshold, var = "NearbyCorpse", threshold = 1.0 }),
            tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })
        ),
        ex3!(
            2188,
            tag!({ type = ActorCondition, actor = "enemy", var = "Taunted" }),
            tag!({ type = Condition, var = "UsedWarcryRecently" })
        ),
        ex3!(2191),
        ex3!(2194),
        (3320, |k| grantedExtraSkill(c(k, 1), k.num, None)),
        (3330, |k| {
            Some(triggerExtraSkill(
                c(k, 2),
                tn(c(k, 1)),
                Some(TriggerExtraSkillOptions {
                    sourceSkill: Some(c(k, 0).to_owned()),
                    ..Default::default()
                }),
            ))
        }),
        (3352, |k| {
            Some(triggerExtraSkill(
                c(k, 1),
                k.num,
                Some(TriggerExtraSkillOptions {
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        (3415, |k| extraSupport(c(k, 1), k.num, None, None)),
        (3416, |k| extraSupport(c(k, 1), k.num, None, None)),
        (3417, |k| {
            extraSupport(c(k, 2), tn(c(k, 1)), Some(c(k, 0)), None)
        }),
        (3419, |k| {
            extraSupport(c(k, 2), tn(c(k, 1)), Some(c(k, 0)), Some(true))
        }),
        (3420, |k| {
            extraSupport(c(k, 1), tn(c(k, 0)), Some("passive tree"), Some(true))
        }),
        (4478, |k| dealNoNonDamageType(c(k, 0), true)),
        (5302, |k| dealNoNonDamageType(c(k, 0), false)),
        (5303, |k| dealNoNonDamageType(c(k, 0), false)),
    ];
    for l in (3322..=3329).chain(3331..=3338).chain(3340..=3351) {
        v.push(trig!(l));
    }
    v
}

#[test]
fn helpers_match_parse_oracle() {
    let Some(dir) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(records) =
        pob_testpaths::require(load_oracle(&dir), "data/dump/parse_oracle.jsonl not found")
    else {
        return; // POB_ALLOW_SKIP=1
    };
    let strict = create_mod_ported();
    if !strict {
        eprintln!("createMod (unit CT) is still a stub: comparing mod names, types, flags and counts only");
    }
    let calls: std::collections::HashMap<u32, HelperCall> = helper_calls().into_iter().collect();
    let gen_special: std::collections::HashSet<u32> = [
        5944, 5947, 6125, 6128, 6131, 6132, 6133, 6136, 6137, 6140, 6141, 6144, 6145, 6146,
    ]
    .into_iter()
    .collect();
    let (mut ok, mut bad, mut gen_ok, mut gen_bad) = (0usize, 0usize, 0usize, 0usize);
    let mut per_line: BTreeMap<u32, usize> = BTreeMap::new();
    let mut fails = Vec::new();
    for r in &records {
        // a one-pass record whose specialModList match consumed the whole line
        let [p] = r.passes.as_slice() else { continue };
        let Some(st) = p.steps.iter().find(|s| s.list == "specialModList") else {
            continue;
        };
        if p.steps.last().map(|s| s.list.as_str()) != Some("specialModList") {
            continue;
        }
        let Some(ln) = st.line else { continue };
        let want = oracle_mods(r);
        if let Some(call) = calls.get(&ln) {
            let mut k = ScanCaps::default();
            for (i, cap) in st.caps.iter().take(5).enumerate() {
                k.cap[i] = Some(cap_text(cap));
            }
            k.num = k.cap[0].as_deref().and_then(pob_lua::tonumber);
            let got = call(&k);
            *per_line.entry(ln).or_default() += 1;
            if mods_eq(&got, &want, strict) {
                ok += 1;
            } else {
                bad += 1;
                if fails.len() < 20 {
                    fails.push(format!(
                        "{:?} (L{ln}): got {got:?}\n   want {want:?}",
                        r.line
                    ));
                }
            }
        } else if gen_special.contains(&ln) {
            let (got, extra) = pob_modparser::parseModNative(&r.line);
            if mods_eq(&got, &want, strict) && extra.as_deref() == r.extra.as_deref() {
                gen_ok += 1;
            } else {
                gen_bad += 1;
                if fails.len() < 20 {
                    fails.push(format!(
                        "{:?} (L{ln}, generated): got {got:?} / {extra:?}\n   want {want:?}",
                        r.line
                    ));
                }
            }
        }
    }
    for f in &fails {
        eprintln!("{f}");
    }
    eprintln!(
        "helpers vs parse oracle ({}): {ok}/{} identical over {} specialModList entries; generated keystone/skill entries: {gen_ok}/{} identical",
        if strict { "strict" } else { "names/types/flags" },
        ok + bad,
        per_line.len(),
        gen_ok + gen_bad
    );
    assert!(
        ok > 0 && gen_ok > 0,
        "no oracle lines exercised the helpers"
    );
    assert_eq!(bad + gen_bad, 0, "helper output differs from PoB");
    // coverage (HYG2/V2)
    assert_eq!(
        records.len(),
        PARSE_ORACLE_RECORDS,
        "parse_oracle.jsonl records"
    );
    assert!(
        ok + bad >= MP0_MIN_HELPERS.0 && gen_ok + gen_bad >= MP0_MIN_HELPERS.1,
        "{} helper / {} generated lines compared, expected at least {MP0_MIN_HELPERS:?}",
        ok + bad,
        gen_ok + gen_bad
    );
}

#[test]
fn appendmod_and_lookups() {
    if pob_testpaths::require(init_data(), "data/dump/data.bin not found").is_none() {
        return; // POB_ALLOW_SKIP=1
    }
    // appendMod: strings get the suffix, arrays get it per element
    let out = gl::appendMod(
        &[
            (n!("life"), Value::from("Life")),
            (
                n!("life and mana"),
                Value::Table(std::sync::Arc::new(pob_gamedata::ValueTable {
                    array: vec![Value::from("Life"), Value::from("Mana")],
                    fields: vec![],
                })),
            ),
        ],
        "Regen",
    );
    assert_eq!(out[0], (n!("life"), Value::from("LifeRegen")));
    assert_eq!(
        out[1].1.as_table().unwrap().array,
        vec![Value::from("LifeRegen"), Value::from("ManaRegen")]
    );
    // regenTypes["maximum energy shield"] (6026-6034 then 6049)
    let v = gl::lookup(&gl::regenTypes, Some("maximum energy shield")).unwrap();
    assert_eq!(*v, Value::from("EnergyShieldRegen"));
    assert!(gl::lookup(&gl::regenTypes, None).is_none());
    // gemIdLookup / dmgTypes
    assert_eq!(
        helpers::gemIdLookup("power charge on critical strike"),
        Some(Name::from("SupportPowerChargeOnCritical"))
    );
    assert_eq!(helpers::gemIdLookup("fireball"), Some(n!("Fireball")));
    assert_eq!(helpers::dmgTypes("cold"), Some(n!("Cold")));
    assert_eq!(helpers::dmgTypes("Cold"), None);
    // unsupportedModList
    assert!(gl::unsupportedModList("mirrored"));
    assert!(!gl::unsupportedModList("Mirrored"));
    // the jewel-func helper as data
    let spec = helpers::getSimpleConv(&["Str"], "Dex", ModType::BASE, true, None, None);
    assert_eq!(spec.key, n!("$getSimpleConv"));
    assert_eq!(spec.args.len(), 6);
    // parseModNative on the unsupported list
    let (m, e) = pob_modparser::parseModNative("Mirrored");
    assert_eq!(m, Some(vec![]));
    assert_eq!(e.as_deref(), Some("Mirrored"));
}
