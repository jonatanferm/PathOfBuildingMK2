//! Unit CO-B acceptance (calc doc §11.1, row CO-B): the ConfigOptions `apply` closures of
//! Modules/ConfigOptions.lua 1189-2380 (`pob_calc::modules::config_options::apply_b`) against
//! PoB's own under LuaJIT (the oracle, `jit.off()`).
//!
//! * `apply_probe_matches_oracle` — every option whose `apply` is defined in lines 1189-2380 is
//!   called with its default and non-default values (`corpus/CO-B/apply_probe.lua` `values`:
//!   for a check `true` plus three values the UI never sends; for a list every list value plus
//!   two that are not in the list; for numbers the default and seven others including a numeric
//!   string and a boolean; for text four strings). The two closures that drive UI controls
//!   (`enemyIsBoss`, `presetBossSkills`) also run under 48 scenarios each (boss selection ×
//!   enemy level input × roll range input × `enemyDamageType.enabled`). PoB records, per call,
//!   the pre-state (the controls and config-set entries those closures read or write, the
//!   enemy level, `buildFlag`), calls the Lua closure on a fresh `ModList` pair and records the
//!   result. Rust restores the pre-state into a model `Build`, calls `configApply(&opt.apply)`
//!   (the registry path BuildModList uses) on a fresh pair, and must reproduce the error status,
//!   the `modList` / `enemyModList` (mods in order, values, tags, sources) and the post-state.
//!   The `presetBossSkills` enemy mods built from `additionalStats` come from a `pairs` loop
//!   (random order in PoB), so that list is compared as a multiset.
//! * `apply_b_rows_cover_range` — `apply_b::ROWS` is exactly the closures.json `config:apply`
//!   ids defined in lines 1189-2380, with the same hashes, and no body is a stub.
//!
//! A missing input (data/dump, the PoB checkout) fails (`pob_testpaths::require`;
//! `POB_ALLOW_SKIP=1` skips instead). `COB_VERBOSE=1` prints every mismatch.
#![allow(non_snake_case, clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use pob_calc::model::{Build, BuildRef};
use pob_calc::modules::config_options::{apply_b, configApply};
use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up, QUIET_PRELUDE};
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

const PROBE: &str = include_str!("../corpus/CO-B/apply_probe.lua");
const EMPTY_XML: &str = include_str!("../corpus/CO-B/empty.xml");
/// CO-B's line range of Modules/ConfigOptions.lua.
const RANGE: (u32, u32) = (1189, 2380);
/// Options whose control-driving closures run under the probe's scenarios.
const SCENARIO_VARS: [&str; 2] = ["enemyIsBoss", "presetBossSkills"];

fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        return p.join("data.bin").is_file().then_some(p);
    }
    find_up("data/dump/data.bin").map(|p| p.parent().unwrap().to_path_buf())
}

fn init_data() -> Option<&'static Data> {
    let dir = dump_dir()?;
    if !pob_gamedata::is_initialised() {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    }
    Some(pob_gamedata::data())
}

/// `Modules/ConfigOptions.lua:<a>-<b>` → `a`.
fn src_start(src: &str) -> u32 {
    let (file, lines) = src.rsplit_once(':').expect("src has a line range");
    assert_eq!(file, "Modules/ConfigOptions.lua", "{src}");
    lines
        .split('-')
        .next()
        .unwrap()
        .parse()
        .expect("line number")
}

fn in_range(src: &str) -> bool {
    let a = src_start(src);
    RANGE.0 <= a && a <= RANGE.1
}

/// 1-based ConfigOptions indices whose `apply` is defined in CO-B's range.
fn probed_indices(data: &Data) -> Vec<usize> {
    data.configOptions
        .iter()
        .enumerate()
        .filter(|(_, o)| o.apply.as_ref().is_some_and(|f| in_range(&f.src)))
        .map(|(i, _)| i + 1)
        .collect()
}

/// A LuaError panic as `Err(message)` (other panics propagate).
fn catch_lua<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => match p.downcast::<LuaError>() {
            Ok(e) => Err(e.0.clone()),
            Err(other) => std::panic::resume_unwind(other),
        },
    }
}

/// LuaErrors are expected (the probe sends values the UI never sends): keep them off stderr.
fn quiet_lua_errors() {
    let prev = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        if info.payload().downcast_ref::<LuaError>().is_none() {
            prev(info);
        }
    }));
}

/// A probe scalar (`val`, state entries).
fn val_from_json(v: &J) -> Val {
    match v {
        J::Null => Val::Nil,
        J::Bool(b) => Val::Bool(*b),
        J::Number(n) => Val::Num(n.as_f64().unwrap()),
        J::String(s) => Val::from(s.as_str()),
        J::Object(o) => match o.get("$float").and_then(J::as_str) {
            Some("nan") => Val::Num(f64::NAN),
            Some("inf") => Val::Num(f64::INFINITY),
            Some("-inf") => Val::Num(f64::NEG_INFINITY),
            Some("-0") => Val::Num(-0.0),
            _ => panic!("probe value {v}"),
        },
        J::Array(_) => panic!("probe value {v}"),
    }
}

/// `{"mod": {…}, "marks": {…}}` → `Mod` (the probe never writes marks into a fresh ModList).
fn decode_mod(e: &J) -> Mod {
    let marks = e["marks"].as_object().expect("marks");
    assert!(marks.is_empty(), "unexpected mod marks {e}");
    serde_json::from_value(e["mod"].clone()).unwrap_or_else(|x| panic!("mod {e}: {x}"))
}

fn decode_list(v: &J) -> Vec<Mod> {
    v.as_array()
        .expect("mod list")
        .iter()
        .map(decode_mod)
        .collect()
}

fn store_mods(ml: &ModList) -> Vec<Mod> {
    ml.ipairs().iter().map(|m| (**m).clone()).collect()
}

fn sorted_by_debug(mut v: Vec<Mod>) -> Vec<Mod> {
    v.sort_by_cached_key(|m| format!("{m:?}"));
    v
}

/// Sites hit since `before` (stub counters are global).
fn stub_hits_since(before: &[(&'static str, u64)]) -> BTreeSet<&'static str> {
    let before: BTreeMap<&str, u64> = before.iter().copied().collect();
    pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(k, n)| before.get(k).map_or(true, |b| n > b))
        .map(|(k, _)| k)
        .collect()
}

/// `varData.list` as the control's `list` (a sequence of `{ val = …, label = … }`).
fn list_table(data: &Data, var: &str) -> Table {
    let opt = data
        .configOptions
        .iter()
        .rev()
        .find(|o| o.var.is_some_and(|v| v.as_str() == var))
        .unwrap_or_else(|| panic!("no option {var}"));
    let t = Table::new();
    for item in opt.list.as_ref().expect("list option") {
        let e = Table::new();
        e.set(n!("val"), Val::from_value(&item.val));
        e.set(n!("label"), &*item.label);
        t.push(e);
    }
    t
}

/// A model build with the control-state tables the probe's PoB build has (CCR-0037: the model
/// constructor does not create them yet).
fn new_build(data: &'static Data, pre: &J) -> BuildRef {
    let build = Build::new(data);
    build.configTab.ctor_model();
    for var in pre["controls"].as_object().expect("controls").keys() {
        let c = Table::new();
        if var == "enemyIsBoss" || var == "enemyDamageType" {
            c.set(n!("list"), list_table(data, var));
        }
        build.configTab.varControls.set(var.as_str(), c);
    }
    build
}

/// Set the probe's state (`state()` in apply_probe.lua) into the model.
fn restore(build: &BuildRef, st: &J) {
    let ct = &build.configTab;
    for (var, c) in st["controls"].as_object().unwrap() {
        let t = ct.varControls.table(var.as_str()).expect("control");
        for f in ["placeholder", "selIndex", "enabled"] {
            t.set(f, val_from_json(&c[f]));
        }
    }
    let set = ct.activeConfigSet();
    let ph = set.table(n!("placeholder")).unwrap();
    for (var, v) in st["placeholder"].as_object().unwrap() {
        ph.set(var.as_str(), val_from_json(v));
    }
    let input = set.table(n!("input")).unwrap();
    for (var, v) in st["input"].as_object().unwrap() {
        input.set(var.as_str(), val_from_json(v));
    }
    ct.enemyLevel
        .set(val_from_json(&st["enemyLevel"]).opt_num());
    build
        .buildFlag
        .set(val_from_json(&st["buildFlag"]).truthy());
    build
        .characterLevel
        .set(val_from_json(&st["characterLevel"]).num());
}

/// Differences between the probe's post-state and the model's.
fn state_diffs(build: &BuildRef, st: &J) -> Vec<String> {
    let mut out = Vec::new();
    let ct = &build.configTab;
    let mut cmp = |what: String, lua: &J, rust: Val| {
        let l = val_from_json(lua);
        let same = l.raw_eq(&rust)
            || matches!((&l, &rust), (Val::Num(a), Val::Num(b)) if a.is_nan() && b.is_nan());
        if !same {
            out.push(format!("{what}: lua {l:?}, rust {rust:?}"));
        }
    };
    for (var, c) in st["controls"].as_object().unwrap() {
        let t = ct.varControls.table(var.as_str()).expect("control");
        for f in ["placeholder", "selIndex", "enabled"] {
            cmp(format!("varControls.{var}.{f}"), &c[f], t.get(f));
        }
    }
    let set = ct.activeConfigSet();
    let ph = set.table(n!("placeholder")).unwrap();
    for (var, v) in st["placeholder"].as_object().unwrap() {
        cmp(format!("placeholder.{var}"), v, ph.get(var.as_str()));
    }
    let input = set.table(n!("input")).unwrap();
    for (var, v) in st["input"].as_object().unwrap() {
        cmp(format!("input.{var}"), v, input.get(var.as_str()));
    }
    cmp(
        "enemyLevel".to_owned(),
        &st["enemyLevel"],
        Val::from(ct.enemyLevel.get()),
    );
    cmp(
        "buildFlag".to_owned(),
        &st["buildFlag"],
        Val::Bool(build.buildFlag.get()),
    );
    out
}

#[derive(Default)]
struct Tally {
    cases: usize,
    matched: usize,
    both_error: usize,
    pending: usize,
    mismatched: usize,
    state_changing: usize,
    options: BTreeSet<usize>,
    values_per_option: BTreeMap<usize, BTreeSet<String>>,
    pending_sites: BTreeSet<&'static str>,
    mismatches: Vec<String>,
}

#[test]
fn apply_probe_matches_oracle() {
    let Some(data) = pob_testpaths::require(init_data(), "data/dump/data.bin not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let Some(src) = pob_testpaths::require(
        find_pob_src(),
        "PoB checkout (.PathOfBuilding/src) not found",
    ) else {
        return; // POB_ALLOW_SKIP=1
    };
    quiet_lua_errors();
    let want = probed_indices(data);
    assert!(want.len() > 250, "probed options: {}", want.len());
    let want_lua = format!(
        "__cob_want = {{ {} }}",
        want.iter()
            .map(|i| format!("[{i}] = true"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let json = {
        let _lock = OracleLock::acquire().expect("oracle lock");
        let mut pob = JitPob::boot_oracle(&src, Some(QUIET_PRELUDE))
            .unwrap_or_else(|e| panic!("PoB boot: {e}"));
        pob.load_build_xml(EMPTY_XML, "cob_empty")
            .unwrap_or_else(|e| panic!("load: {e}"));
        pob.run(&want_lua).unwrap_or_else(|e| panic!("want: {e}"));
        match pob
            .run(PROBE)
            .unwrap_or_else(|e| panic!("probe: {e}"))
            .first()
        {
            Some(Ret::Str(s)) => s.clone(),
            other => panic!("probe returned {other:?}"),
        }
    };
    let doc: J = serde_json::from_str(&json).unwrap_or_else(|e| panic!("probe json: {e}"));
    let cases = doc["cases"].as_array().expect("cases");
    assert!(!cases.is_empty(), "no probe cases");
    let verbose = std::env::var_os("COB_VERBOSE").is_some();
    let build = new_build(data, &cases[0]["pre"]);
    let mut t = Tally::default();
    for c in cases {
        let i = c["i"].as_u64().unwrap() as usize;
        let opt = &data.configOptions[i - 1];
        assert_eq!(
            opt.var.map(|v| v.as_str().to_owned()),
            c["var"].as_str().map(str::to_owned),
            "ConfigOptions[{i}] var differs between data.bin and PoB"
        );
        let apply = opt.apply.as_ref().expect("probed option has apply");
        let val = val_from_json(&c["val"]);
        restore(&build, &c["pre"]);
        let modList = ModStore::ModList(None);
        let enemyModList = ModStore::ModList(None);
        let before = pob_calc::stub::stub_report();
        let r = catch_lua(|| configApply(apply)(&val, &modList, &enemyModList, &build));
        let hits = stub_hits_since(&before);
        t.cases += 1;
        t.options.insert(i);
        t.values_per_option
            .entry(i)
            .or_default()
            .insert(c["val"].to_string());
        if c["pre"] != c["post"] {
            t.state_changing += 1;
        }
        let lua_ok = c["ok"].as_bool().unwrap();
        let mut diffs = Vec::new();
        if lua_ok != r.is_ok() {
            diffs.push(format!(
                "status: lua {} ({}), rust {:?}",
                lua_ok,
                c["err"].as_str().unwrap_or(""),
                r
            ));
        }
        let lua_mods = decode_list(&c["mods"]);
        let rust_mods = store_mods(&modList);
        if lua_mods != rust_mods {
            diffs.push(format!(
                "modList:\n    lua  {lua_mods:?}\n    rust {rust_mods:?}"
            ));
        }
        let mut lua_enemy = decode_list(&c["enemy"]);
        let mut rust_enemy = store_mods(&enemyModList);
        if c["var"] == "presetBossSkills" {
            // LUA-ORDER: `pairs(additionalStats)` (ConfigOptions.lua:2335)
            lua_enemy = sorted_by_debug(lua_enemy);
            rust_enemy = sorted_by_debug(rust_enemy);
        }
        if lua_enemy != rust_enemy {
            diffs.push(format!(
                "enemyModList:\n    lua  {lua_enemy:?}\n    rust {rust_enemy:?}"
            ));
        }
        // after a Lua error the state is whatever the statements before it left; compare it too
        diffs.extend(state_diffs(&build, &c["post"]));
        let label = format!("ConfigOptions[{i}] ({}) val={}", c["var"], c["val"]);
        if diffs.is_empty() {
            t.matched += 1;
            if !lua_ok {
                t.both_error += 1;
            }
        } else if !hits.is_empty() {
            t.pending += 1;
            t.pending_sites.extend(hits.iter().copied());
        } else {
            t.mismatched += 1;
            t.mismatches.push(format!("{label}: {}", diffs.join("; ")));
        }
        if verbose && !diffs.is_empty() {
            eprintln!("  {label}: {}", diffs.join("; "));
        }
    }
    let min_values = t
        .values_per_option
        .values()
        .map(BTreeSet::len)
        .min()
        .unwrap_or(0);
    let scenario_cases: usize = cases
        .iter()
        .filter(|c| SCENARIO_VARS.iter().any(|v| c["var"] == *v))
        .count();
    eprintln!(
        "CO-B apply probe: {} options, {} cases (min {} distinct values per option; {} scenario cases of enemyIsBoss/presetBossSkills; {} cases change the probed state): {} identical ({} both raise), {} pending (stubs: {:?}), {} mismatched",
        t.options.len(),
        t.cases,
        min_values,
        scenario_cases,
        t.state_changing,
        t.matched,
        t.both_error,
        t.pending,
        t.pending_sites,
        t.mismatched
    );
    for m in t.mismatches.iter().take(40) {
        eprintln!("  MISMATCH {m}");
    }
    let missing: Vec<usize> = want
        .iter()
        .copied()
        .filter(|i| !t.options.contains(i))
        .collect();
    assert!(missing.is_empty(), "options PoB did not probe: {missing:?}");
    assert!(min_values >= 4, "an option has fewer than 4 probed values");
    assert!(
        t.state_changing > 0,
        "no probed case changed the control state"
    );
    assert!(
        t.pending_sites
            .iter()
            .all(|s| !s.starts_with("Modules/ConfigOptions.lua")),
        "apply_b stub hit: {:?}",
        t.pending_sites
    );
    assert_eq!(t.mismatched, 0, "apply results differ from PoB");
    // coverage and the tolerated buckets (HYG2/V2). With every stub this range reaches ported
    // (measured: 0 pending), a pending case is a regression: one may only come from a stub site
    // of another unit listed in COB_PENDING_SITES (none today), and none are allowed. Both-raise
    // matches are bounded by the measured count.
    assert_eq!(
        (want.len(), t.cases),
        COB_COVERAGE,
        "probed options and cases"
    );
    let unlisted: Vec<&str> = t
        .pending_sites
        .iter()
        .copied()
        .filter(|s| !COB_PENDING_SITES.contains(s))
        .collect();
    assert!(
        unlisted.is_empty(),
        "pending cases hit stub sites outside COB_PENDING_SITES: {unlisted:?}"
    );
    assert_eq!(
        t.pending, 0,
        "pending cases (a stub reached and the result differs)"
    );
    assert!(
        t.both_error <= COB_MAX_BOTH_ERROR,
        "{} cases raise on both sides, bound {}",
        t.both_error,
        COB_MAX_BOTH_ERROR
    );
}

/// `(probed options, probed cases)`; measured by HYG2 on rust-port 746b103.
const COB_COVERAGE: (usize, usize) = (273, 2407);
/// The other units' `unported!` sites a pending case may hit (HYG2/V2): none.
const COB_PENDING_SITES: &[&str] = &[];
/// Cases where PoB and the port both raise; measured by HYG2 on rust-port 746b103.
const COB_MAX_BOTH_ERROR: usize = 108;

#[test]
fn apply_b_rows_cover_range() {
    let Some(dir) = pob_testpaths::require(dump_dir(), "data/dump not found") else {
        return; // POB_ALLOW_SKIP=1
    };
    let text = std::fs::read_to_string(dir.join("closures.json")).expect("closures.json");
    let j: J = serde_json::from_str(&text).unwrap();
    let mut want = BTreeMap::new();
    for c in j["closures"].as_array().unwrap() {
        if c["family"] == "config:apply" && in_range(c["src"].as_str().unwrap()) {
            want.insert(
                c["id"].as_str().unwrap().to_owned(),
                c["hash"].as_str().unwrap().to_owned(),
            );
        }
    }
    let have: BTreeMap<String, String> = apply_b::ROWS
        .iter()
        .map(|r| (r.id.to_owned(), r.hash.to_owned()))
        .collect();
    assert_eq!(
        have.len(),
        apply_b::ROWS.len(),
        "duplicate ids in apply_b::ROWS"
    );
    assert_eq!(
        want, have,
        "apply_b::ROWS vs closures.json (lines 1189-2380)"
    );
    assert_eq!(have.len(), 273);
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../pob-calc-config/src/modules/config_options/apply_b.rs"),
    )
    .unwrap();
    assert!(
        !src.contains("unported!("),
        "apply_b.rs still has an unported body"
    );
}
