//! Unit CO-A acceptance (calc doc §11.1, row CO-A): the ConfigOptions `apply` closures of
//! Modules/ConfigOptions.lua 1-1188 (`pob_calc::modules::config_options::apply_a`) against PoB's
//! own under LuaJIT (the oracle, `jit.off()`).
//!
//! * `apply_probe_matches_oracle` — every option whose `apply` is defined in lines 1-1188
//!   (including the eight map affix options that share `mapAffixDropDownFunction`) is called
//!   with its default and non-default values (`corpus/CO-A/apply_probe.lua` `values`: for a
//!   check `true` plus three values the UI never sends; for a list every list value plus one
//!   that is not in the list; for numbers the default and seven others including a numeric
//!   string; for text four strings). PoB calls the Lua closure on a fresh `ModList` pair;
//!   Rust calls `configApply(&opt.apply)` (the registry path BuildModList uses) on a fresh
//!   pair. The resulting `modList` / `enemyModList` (mods in order, values, tags, sources), the
//!   error status and the `varControls.enemyRadius.placeholder` the closure leaves must be
//!   identical. A case whose Rust run hit an `unported!` stub of another unit (the map mod
//!   bodies, `Data/ModMap.lua`, are CO-C's) is reported as *pending* instead of failing.
//! * `apply_a_rows_cover_range` — `apply_a::ROWS` is exactly the closures.json `config:apply`
//!   ids defined in lines 1-1188, with the same hashes, and no body is a stub.
//!
//! A missing input (data/dump, the PoB checkout) fails (`pob_testpaths::require`;
//! `POB_ALLOW_SKIP=1` skips instead). `COA_VERBOSE=1` prints every mismatch.
#![allow(non_snake_case, clippy::pedantic)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::PathBuf;

use pob_calc::model::{Build, BuildRef};
use pob_calc::modules::config_options::{apply_a, configApply};
use pob_calc::prelude::*;
use pob_oracle::headless::{Engine, Ret};
use pob_oracle::jit::JitPob;
use pob_oracle::oracle::{find_pob_src, find_up, QUIET_PRELUDE};
use pob_oracle::runner::OracleLock;
use serde_json::Value as J;

const PROBE: &str = include_str!("../corpus/CO-A/apply_probe.lua");
const EMPTY_XML: &str = include_str!("../corpus/CO-A/empty.xml");
/// CO-A's line range of Modules/ConfigOptions.lua.
const RANGE: (u32, u32) = (1, 1188);

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

/// 1-based ConfigOptions indices whose `apply` is defined in CO-A's range.
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

/// The probe's `val` (scalars only).
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

/// Sites hit since `before` (stub counters are global).
fn stub_hits_since(before: &[(&'static str, u64)]) -> BTreeSet<&'static str> {
    let before: BTreeMap<&str, u64> = before.iter().copied().collect();
    pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(k, n)| before.get(k).map_or(true, |b| n > b))
        .map(|(k, _)| k)
        .collect()
}

/// The control state the probe fixes (apply_probe.lua header).
fn new_build(data: &'static Data) -> BuildRef {
    let build = Build::new(data);
    let ct = &build.configTab;
    ct.input.set(n!("multiplierMapModEffect"), 30.0);
    let tier = Table::new();
    tier.set(n!("selIndex"), 2.0);
    ct.varControls.set(n!("multiplierMapModTier"), tier);
    ct.varControls.set(n!("enemyRadius"), Table::new());
    build
}

#[derive(Default)]
struct Tally {
    cases: usize,
    matched: usize,
    both_error: usize,
    pending: usize,
    mismatched: usize,
    options: BTreeSet<usize>,
    cases_per_option: BTreeMap<usize, usize>,
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
        "__coa_want = {{ {} }}",
        want.iter()
            .map(|i| format!("[{i}] = true"))
            .collect::<Vec<_>>()
            .join(", ")
    );
    let json = {
        let _lock = OracleLock::acquire().expect("oracle lock");
        let mut pob = JitPob::boot_oracle(&src, Some(QUIET_PRELUDE))
            .unwrap_or_else(|e| panic!("PoB boot: {e}"));
        pob.load_build_xml(EMPTY_XML, "coa_empty")
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
    let verbose = std::env::var_os("COA_VERBOSE").is_some();
    let build = new_build(data);
    let mut t = Tally::default();
    for c in doc["cases"].as_array().expect("cases") {
        let i = c["i"].as_u64().unwrap() as usize;
        let opt = &data.configOptions[i - 1];
        assert_eq!(
            opt.var.map(|v| v.as_str().to_owned()),
            c["var"].as_str().map(str::to_owned),
            "ConfigOptions[{i}] var differs between data.bin and PoB"
        );
        let apply = opt.apply.as_ref().expect("probed option has apply");
        let val = val_from_json(&c["val"]);
        let modList = ModStore::ModList(None);
        let enemyModList = ModStore::ModList(None);
        build
            .configTab
            .varControls
            .table(n!("enemyRadius"))
            .unwrap()
            .set(n!("placeholder"), "<unset>");
        let before = pob_calc::stub::stub_report();
        let r = catch_lua(|| configApply(apply)(&val, &modList, &enemyModList, &build));
        let hits = stub_hits_since(&before);
        t.cases += 1;
        t.options.insert(i);
        *t.cases_per_option.entry(i).or_default() += 1;
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
        let lua_enemy = decode_list(&c["enemy"]);
        let rust_enemy = store_mods(&enemyModList);
        if lua_enemy != rust_enemy {
            diffs.push(format!(
                "enemyModList:\n    lua  {lua_enemy:?}\n    rust {rust_enemy:?}"
            ));
        }
        let rust_ph = build
            .configTab
            .varControls
            .table(n!("enemyRadius"))
            .unwrap()
            .get(n!("placeholder"));
        let lua_ph = val_from_json(&c["radiusPlaceholder"]);
        if !lua_ph.raw_eq(&rust_ph) {
            diffs.push(format!(
                "enemyRadius placeholder: lua {lua_ph:?}, rust {rust_ph:?}"
            ));
        }
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
    let min_cases = t.cases_per_option.values().copied().min().unwrap_or(0);
    eprintln!(
        "CO-A apply probe: {} options, {} cases (min {} per option): {} identical ({} both raise), {} pending (stubs: {:?}), {} mismatched",
        t.options.len(),
        t.cases,
        min_cases,
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
    assert!(min_cases >= 4, "an option has fewer than 4 probed values");
    // every pending case must come from another unit's stub, never from apply_a itself
    assert!(
        t.pending_sites
            .iter()
            .all(|s| !s.starts_with("Modules/ConfigOptions.lua")),
        "apply_a stub hit: {:?}",
        t.pending_sites
    );
    assert_eq!(t.mismatched, 0, "apply results differ from PoB");
    // coverage and the tolerated buckets (HYG2/V2). With every stub this range reaches ported
    // (measured: 0 pending), a pending case is a regression: one may only come from a stub site
    // of another unit listed in COA_PENDING_SITES (none today), and none are allowed. Both-raise
    // matches are bounded by the measured count.
    assert_eq!(
        (want.len(), t.cases),
        COA_COVERAGE,
        "probed options and cases"
    );
    let unlisted: Vec<&str> = t
        .pending_sites
        .iter()
        .copied()
        .filter(|s| !COA_PENDING_SITES.contains(s))
        .collect();
    assert!(
        unlisted.is_empty(),
        "pending cases hit stub sites outside COA_PENDING_SITES: {unlisted:?}"
    );
    assert_eq!(
        t.pending, 0,
        "pending cases (a stub reached and the result differs)"
    );
    assert!(
        t.both_error <= COA_MAX_BOTH_ERROR,
        "{} cases raise on both sides, bound {}",
        t.both_error,
        COA_MAX_BOTH_ERROR
    );
}

/// `(probed options, probed cases)`; measured by HYG2 on rust-port 746b103.
const COA_COVERAGE: (usize, usize) = (266, 1741);
/// The other units' `unported!` sites a pending case may hit (HYG2/V2): none.
const COA_PENDING_SITES: &[&str] = &[];
/// Cases where PoB and the port both raise; measured by HYG2 on rust-port 746b103.
const COA_MAX_BOTH_ERROR: usize = 18;

#[test]
fn apply_a_rows_cover_range() {
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
    let have: BTreeMap<String, String> = apply_a::ROWS
        .iter()
        .map(|r| (r.id.to_owned(), r.hash.to_owned()))
        .collect();
    assert_eq!(
        have.len(),
        apply_a::ROWS.len(),
        "duplicate ids in apply_a::ROWS"
    );
    assert_eq!(want, have, "apply_a::ROWS vs closures.json (lines 1-1188)");
    assert_eq!(have.len(), 259);
    let src = std::fs::read_to_string(
        PathBuf::from(env!("CARGO_MANIFEST_DIR"))
            .join("../pob-calc/src/modules/config_options/apply_a.rs"),
    )
    .unwrap();
    assert!(
        !src.contains("unported!(\"Modules/ConfigOptions.lua"),
        "apply_a.rs still has an unported ConfigOptions body"
    );
}
