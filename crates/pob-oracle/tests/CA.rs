//! Unit CA acceptance (calc doc §11.1 row CA, §9.4 L5/L6): Modules/Calcs.lua 1-872
//! (`buildOutput`, `calcFullDPS`, `getMiscCalculator`, `buildActiveSkill`).
//!
//! * `ca_l5_post` (L5, against PoB's own state): `calcs.buildOutput`'s post-processing blocks run
//!   on PoB's state, injected from line probes inside `buildOutput` (`corpus/CA/stages.toml`):
//!   - cost warnings (Calcs.lua 400-435): inject `ca:399` of the MAIN env, then the MAIN
//!     GlobalCache entries the loop reads as PoB had them after the loop (`corpus/CA/ca.lua`
//!     `costs`: each cached env's cost outputs and main-skill `<pool>ReservedPercent`; the probe
//!     dumps only hold other envs as placeholders); diff the warning keys against `ca:437`;
//!   - spec sums and used-tracking (437-600): inject `ca:437`, diff `ExtraPoints`, `Spec:*` and
//!     every `env.*Used` table (per key, the multiset of mods) against `ca:869`;
//!   - CALCS lists (601-866): inject the CALCS env's `ca:399`, with `calcsTab.mainEnv`'s
//!     `multipliersUsed` keys from PoB, diff `BuffList` / `CombatList` / `CurseList` (player and
//!     minion) and the `SkillBuffs` / `SkillDebuffs` breakdown rows against its `ca:869`.
//!   Gate: no mismatch and no Rust error in any compared block, over every corpus build whose
//!   dumps inject.
//! * `ca_l6_corpus` (L6, end to end): the Rust build model from the XML, then `CalcsTab:
//!   BuildOutput` as PoB's load runs it (once for `Build:Init`, plus once per `Build:OnFrame`
//!   rebuild PoB made, CalcsTab.lua 441-472 / Build.lua 1258-1273; PWR owns that function, the
//!   test carries a scaffold copy), against PoB's `calcsTab.mainOutput`, `calcsOutput`, the main
//!   env's used-tracking and the misc calculator. Reported per build (headline keys) and in total;
//!   gated by floors at the measured values while CS2 (initEnv items, CalcSetup.lua 800-1450) and
//!   PF-B (CalcPerform.lua 636-1272) are stubs (docs/port/FOLLOWUPS.md "CA").
//! * `ca_d13_fixed_point` (L5 D13, Rust only): build → buildOutput → buildOutput gives identical
//!   outputs and model state (`Build:SaveDB`), on slice5 and the corpus. A build PoB itself does
//!   not reproduce on a warm GlobalCache (`D13_POB_WARM_CACHE`) gets its warm run checked against
//!   PoB's warm values and its fixed point checked after a `wipeGlobalCache()`.
//!
//! PoB runs (probe dumps + `ca.lua`) are cached in `target/tmp/ca-probe/<stem>/`. Knobs:
//! `CA_BUILDS=stem1,stem2`, `CA_LIMIT=n`, `CA_VERBOSE=1`. A missing input skips with a message;
//! with `POB_ORACLE_REQUIRE=1` (or `CI`) it fails instead.
#![allow(non_snake_case)]

use std::collections::{BTreeMap, BTreeSet};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::sync::{Arc, Mutex};

use pob_calc::modules::calcs::{buildOutput, getMiscCalculator};
use pob_calc::modules::common::wipeGlobalCache;
use pob_calc::prelude::*;
use pob_gamedata::timeless_lut as lut;
use pob_oracle::oracle::{find_parity_dir, find_pob_src, find_up, Leaf};
use serde_json::Value as J;

static SERIAL: Mutex<()> = Mutex::new(());

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn verbose() -> bool {
    std::env::var_os("CA_VERBOSE").is_some()
}

fn stem(p: &Path) -> &str {
    p.file_stem().and_then(|s| s.to_str()).unwrap_or_default()
}

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = if let Some(p) = std::env::var_os("POB_DUMP_DIR").map(PathBuf::from) {
        p
    } else if let Some(m) = find_up("data/dump/data.bin") {
        m.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        return false;
    };
    pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    true
}

/// Lua errors are expected results here (reported, not printed as panics).
fn quiet_lua_errors() {
    static ONCE: std::sync::Once = std::sync::Once::new();
    if std::env::var_os("CA_TRACE").is_some() {
        return; // print Lua errors with their backtrace (RUST_BACKTRACE=1)
    }
    ONCE.call_once(|| {
        let prev = std::panic::take_hook();
        std::panic::set_hook(Box::new(move |info| {
            if info.payload().downcast_ref::<LuaError>().is_none() {
                prev(info);
            }
        }));
    });
}

/// Run `f`; a Lua error (or any other panic) becomes `Err(message)`.
fn catch<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    match std::panic::catch_unwind(std::panic::AssertUnwindSafe(f)) {
        Ok(r) => Ok(r),
        Err(p) => Err(p
            .downcast_ref::<LuaError>()
            .map(|e| format!("Lua error: {}", e.0))
            .or_else(|| p.downcast_ref::<String>().map(|s| format!("panic: {s}")))
            .or_else(|| p.downcast_ref::<&str>().map(|s| format!("panic: {s}")))
            .unwrap_or_else(|| "panic".to_owned())),
    }
}

/// Timeless jewel LUTs load from the PoB checkout (as BM-A / PS-B do).
fn setup_luts() -> bool {
    let Some(src) = find_pob_src() else {
        return false;
    };
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    lut::set_jewel_fs(Arc::new(lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    lut::set_con_printf(|_| {});
    true
}

/// `.parity/builds/*.xml`, filtered by `CA_BUILDS` / `CA_LIMIT`.
fn corpus(parity: &Path) -> Vec<PathBuf> {
    let mut v: Vec<PathBuf> = std::fs::read_dir(parity.join("builds"))
        .map(|r| {
            r.filter_map(Result::ok)
                .map(|e| e.path())
                .filter(|p| p.extension().is_some_and(|x| x == "xml"))
                .collect()
        })
        .unwrap_or_default();
    v.sort();
    if let Ok(f) = std::env::var("CA_BUILDS") {
        let want: Vec<&str> = f.split(',').map(str::trim).collect();
        v.retain(|p| want.iter().any(|w| stem(p) == *w));
    }
    if let Some(n) = std::env::var("CA_LIMIT").ok().and_then(|s| s.parse().ok()) {
        v.truncate(n);
    }
    v
}

// ---- values ------------------------------------------------------------------------------------

/// The §9.2 numeric tolerance: `|a-b| <= 1e-9 * max(1, |a|)` (±0 and NaN = NaN pass).
fn num_eq(a: f64, b: f64) -> bool {
    (a.is_nan() && b.is_nan()) || a == b || (a - b).abs() <= 1e-9 * a.abs().max(1.0)
}

fn leaf_eq(a: &Leaf, b: &Leaf) -> bool {
    match (a, b) {
        (Leaf::Num(x), Leaf::Num(y)) => num_eq(*x, *y),
        _ => a == b,
    }
}

/// A Rust output table flattened as `pob_oracle::oracle::flatten` names the oracle's keys
/// (`.`-joined, list positions 1-based). Calc objects in the table are skipped.
fn flat_table(t: &Table) -> BTreeMap<String, Leaf> {
    fn walk(prefix: &str, t: &Table, out: &mut BTreeMap<String, Leaf>, seen: &mut Vec<*const ()>) {
        if seen.contains(&t.as_ptr()) || seen.len() > 16 {
            return;
        }
        seen.push(t.as_ptr());
        for (k, v) in t.pairs() {
            let ks = match k {
                Key::S(n) => n.to_string(),
                Key::I(i) => i.to_string(),
                Key::B(b) => b.to_string(),
            };
            let path = if prefix.is_empty() { ks } else { format!("{prefix}.{ks}") };
            match v {
                Val::Num(x) => {
                    out.insert(path, Leaf::Num(x));
                }
                Val::Bool(b) => {
                    out.insert(path, Leaf::Bool(b));
                }
                Val::Str(s) => {
                    out.insert(path, Leaf::Str(s.to_string()));
                }
                Val::Table(c) => walk(&path, &c, out, seen),
                _ => {}
            }
        }
        seen.pop();
    }
    let mut out = BTreeMap::new();
    walk("", t, &mut out, &mut Vec::new());
    out
}

/// An oracle JSON subtree flattened (marker objects and `$` members dropped).
fn flat_json(j: &J) -> BTreeMap<String, Leaf> {
    let m = match j {
        J::Object(m) => m.clone(),
        _ => serde_json::Map::new(),
    };
    pob_oracle::oracle::flatten(&m)
        .into_iter()
        .filter(|(k, _)| !k.split('.').any(|p| p.starts_with('$')))
        .collect()
}

/// A JSON value with every number as an f64 (so `1` and `1.0` compare equal) and object keys
/// sorted, as a string.
fn canon(j: &J) -> String {
    fn norm(j: &J) -> J {
        match j {
            J::Number(n) => {
                let f = n.as_f64().unwrap_or(f64::NAN);
                J::String(format!("#{f:?}"))
            }
            J::Array(a) => J::Array(a.iter().map(norm).collect()),
            J::Object(m) => {
                let s: BTreeMap<String, J> = m
                    .iter()
                    .filter(|(k, _)| k.as_str() != "$id")
                    .map(|(k, v)| (k.clone(), norm(v)))
                    .collect();
                J::Object(s.into_iter().collect())
            }
            other => other.clone(),
        }
    }
    norm(j).to_string()
}

fn val_json(v: &Val) -> J {
    match v {
        Val::Nil => J::Null,
        Val::Bool(b) => J::Bool(*b),
        Val::Num(x) => serde_json::Number::from_f64(*x).map_or_else(|| J::String(format!("{x}")), J::Number),
        Val::Str(s) => J::String(s.to_string()),
        Val::Data(d) => pob_calc::probe::value_json(d),
        Val::Mod(m) => pob_calc::probe::mod_json(m, None),
        other => J::String(format!("<{}>", other.lua_type())),
    }
}

/// `{ var = { mod, ... } }` (a used-tracking table) as var → sorted canonical mods.
fn used_rust(t: &Option<Table>) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(t) = t else { return out };
    for (k, v) in t.pairs() {
        let Key::S(k) = k else { continue };
        let mut mods: Vec<String> = match v {
            Val::Table(l) => l.ipairs().into_iter().map(|(_, m)| canon(&val_json(&m))).collect(),
            Val::Bool(true) => Vec::new(),
            other => vec![format!("<{}>", other.lua_type())],
        };
        mods.sort();
        out.insert(k.to_string(), mods);
    }
    out
}

fn used_pob(j: Option<&J>) -> BTreeMap<String, Vec<String>> {
    let mut out = BTreeMap::new();
    let Some(J::Object(m)) = j else { return out };
    for (k, v) in m {
        if k.starts_with('$') {
            continue;
        }
        let mut mods: Vec<String> = match v {
            J::Array(a) => a.iter().map(canon).collect(),
            J::Object(o) if o.is_empty() || o.keys().all(|k| k.starts_with('$')) => Vec::new(),
            J::Bool(true) => Vec::new(),
            other => vec![canon(other)],
        };
        mods.sort();
        out.insert(k.clone(), mods);
    }
    out
}

thread_local! {
    // PoB values compared since the last `take_compared` (keys, mods, rows).
    static COMPARED: std::cell::Cell<usize> = const { std::cell::Cell::new(0) };
}

fn bump(n: usize) {
    COMPARED.with(|c| c.set(c.get() + n));
}

fn take_compared() -> usize {
    COMPARED.with(|c| c.replace(0))
}

/// Differences between two used-tracking maps (key sets and per-key mod multisets).
fn used_diff(name: &str, ours: &BTreeMap<String, Vec<String>>, pob: &BTreeMap<String, Vec<String>>, out: &mut Vec<String>) {
    bump(pob.values().map(|v| v.len() + 1).sum());
    for (k, pv) in pob {
        match ours.get(k) {
            None => out.push(format!("{name}[{k}]: missing in Rust ({} mods in PoB)", pv.len())),
            Some(ov) if ov != pv => {
                let extra: Vec<&String> = ov.iter().filter(|m| !pv.contains(m)).take(1).collect();
                let miss: Vec<&String> = pv.iter().filter(|m| !ov.contains(m)).take(1).collect();
                out.push(format!(
                    "{name}[{k}]: {} mods vs PoB {}; Rust-only {:?}; PoB-only {:?}",
                    ov.len(),
                    pv.len(),
                    extra.first().map(|s| clip(s)),
                    miss.first().map(|s| clip(s))
                ));
            }
            _ => {}
        }
    }
    for k in ours.keys() {
        if !pob.contains_key(k) {
            out.push(format!("{name}[{k}]: only in Rust"));
        }
    }
}

fn clip(s: &str) -> String {
    if s.len() > 220 {
        format!("{}…", &s[..s.char_indices().take_while(|(i, _)| *i < 220).last().map_or(0, |(i, c)| i + c.len_utf8())])
    } else {
        s.to_owned()
    }
}

fn leaves_diff(name: &str, ours: &BTreeMap<String, Leaf>, pob: &BTreeMap<String, Leaf>, out: &mut Vec<String>) {
    bump(pob.len());
    for (k, pv) in pob {
        match ours.get(k) {
            Some(ov) if leaf_eq(ov, pv) => {}
            other => out.push(format!("{name} {k}: Rust {other:?} vs PoB {pv:?}")),
        }
    }
    for (k, ov) in ours {
        if !pob.contains_key(k) {
            out.push(format!("{name} {k}: Rust {ov:?}, absent in PoB"));
        }
    }
}

// ---- CalcsTab:BuildOutput scaffold (PWR owns CalcsTab.lua 441-472) -----------------------------

/// `CalcsTab:BuildOutput()` (CalcsTab.lua 441-472), test scaffold until PWR lands. In PoB a
/// Lua error aborts the call; here each of its three calcs is caught on its own so that a
/// CALCS-mode error (an upstream stub gap) still leaves the MAIN output to compare.
/// → the errors as `"<stage>: <error>"`.
fn calcsTab_BuildOutput(build: &BuildRef) -> Vec<String> {
    let mut errors = Vec::new();
    build.calcsTab.powerBuildFlag.set(true);
    // Set default final mod list for all nodes; some may not be set during the main pass
    let spec = build.spec.get();
    for (_, node) in spec.nodes.with(|n| n.pairs()) {
        node.finalModList.set(Some(node.modList.get()));
    }
    match catch(|| buildOutput(build, Mode::MAIN)) {
        Ok(mainEnv) => {
            build.calcsTab.mainOutput.set(Some(mainEnv.player.output.get()));
            build.calcsTab.mainEnv.set(Some(mainEnv));
        }
        Err(e) => errors.push(format!("MAIN: {e}")),
    }
    match catch(|| buildOutput(build, Mode::CALCS)) {
        Ok(calcsEnv) => {
            build.calcsTab.calcsOutput.set(Some(calcsEnv.player.output.get()));
            build.calcsTab.calcsEnv.set(Some(calcsEnv));
        }
        Err(e) => errors.push(format!("CALCS: {e}")),
    }
    match catch(|| getMiscCalculator(build)) {
        Ok((f, _base)) => build.calcsTab.miscCalculator.set(Some(f)),
        Err(e) => errors.push(format!("getMiscCalculator: {e}")),
    }
    errors
}

/// The XML's build model with PoB's load-time calc passes: `Build:Init`'s BuildOutput, then
/// `rounds - 1` `Build:OnFrame` rebuilds (wipeGlobalCache, UpdateSocketGroups, BuildOutput,
/// configTab's GetMiscCalculator; Build.lua 1258-1273). → (build, stage errors of the last
/// round).
fn rust_load(xml: &str, rounds: usize) -> Result<(BuildRef, Vec<String>), String> {
    let build = catch(|| {
        pob_calc::model::build_xml::loadBuildModelFromXML(xml, pob_gamedata::data())
    })?
    .map_err(|e| format!("load: {e}"))?;
    // Build:Init (Build.lua 693-697)
    wipeGlobalCache();
    build.outputRevision.set(1.0);
    let mut errors = calcsTab_BuildOutput(&build);
    for _ in 1..rounds.max(1) {
        wipeGlobalCache();
        build.outputRevision.set(build.outputRevision.get() + 1.0);
        catch(|| build.skillsTab.UpdateSocketGroups(&build))?;
        errors = calcsTab_BuildOutput(&build);
        if let Err(e) = catch(|| getMiscCalculator(&build)) {
            errors.push(format!("configTab GetMiscCalculator: {e}"));
        }
    }
    Ok((build, errors))
}

// ---- D13 fixed point (Rust only) ---------------------------------------------------------------

fn slice5_xmls() -> Vec<(String, String)> {
    let dir = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/U0");
    let list = std::fs::read_to_string(dir.join("slice5.txt")).unwrap_or_default();
    list.lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .filter_map(|e| {
            let p = dir.join("slice5").join(format!("{e}.xml"));
            std::fs::read_to_string(p).ok().map(|x| (e.to_owned(), x))
        })
        .collect()
}

#[test]
fn ca_d13_fixed_point() {
    let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    if !init_data() {
        return skip("data/dump/data.bin not found");
    }
    quiet_lua_errors();
    setup_luts();
    let mut builds = slice5_xmls();
    if let Some(parity) = find_parity_dir() {
        for p in corpus(&parity) {
            if let Ok(x) = std::fs::read_to_string(&p) {
                builds.push((stem(&p).to_owned(), x));
            }
        }
    }
    if builds.is_empty() {
        return skip("no builds (corpus/U0/slice5, .parity/builds)");
    }
    let (mut ran, mut equal, mut errored) = (0, 0, Vec::new());
    let mut failures = Vec::new();
    let mut warm_checked = 0;
    for (name, xml) in &builds {
        let warm = D13_POB_WARM_CACHE.iter().find(|(b, _)| b == name).map(|(_, v)| *v);
        let run = catch(|| {
            let build = pob_calc::model::build_xml::loadBuildModelFromXML(xml, pob_gamedata::data())
                .unwrap_or_else(|e| lua_error!("load: {e}"));
            wipeGlobalCache();
            let e1 = buildOutput(&build, Mode::MAIN);
            let o1 = flat_table(&e1.player.output.get());
            let s1 = build.SaveDB("ca-d13");
            // a build PoB itself does not reproduce on a warm GlobalCache: the warm run is
            // checked against PoB's warm values, the fixed point after a wipe
            let ow = warm.map(|_| {
                let ew = buildOutput(&build, Mode::MAIN);
                let o = flat_table(&ew.player.output.get());
                wipeGlobalCache();
                o
            });
            let e2 = buildOutput(&build, Mode::MAIN);
            let o2 = flat_table(&e2.player.output.get());
            let s2 = build.SaveDB("ca-d13");
            (o1, o2, s1, s2, ow)
        });
        lut::clear_timeless_jewel_luts(None);
        let (o1, o2, s1, s2, ow) = match run {
            Ok(r) => r,
            Err(e) => {
                eprintln!("D13 {name}: {e}");
                errored.push(format!("{name}: {e}"));
                continue;
            }
        };
        ran += 1;
        let mut d = Vec::new();
        if let (Some(want), Some(ow)) = (warm, ow.as_ref()) {
            warm_checked += 1;
            for (k, v) in want {
                match ow.get(*k) {
                    Some(Leaf::Num(x)) if num_eq(*x, *v) => {}
                    other => d.push(format!("warm-cache run {k}: Rust {other:?} vs PoB {v}")),
                }
            }
        }
        leaves_diff("output", &o2, &o1, &mut d);
        if s1 != s2 {
            d.push("model: SaveDB differs after the second buildOutput".to_owned());
        }
        if o1.is_empty() {
            d.push("output: empty".to_owned());
        }
        if d.is_empty() {
            equal += 1;
            eprintln!("D13 {name}: ok ({} output keys)", o1.len());
        } else {
            eprintln!("D13 {name}: {} differences", d.len());
            for x in d.iter().take(10) {
                eprintln!("    {x}");
            }
            failures.push(format!("{name}: {} differences", d.len()));
        }
    }
    eprintln!(
        "CA D13: {} builds, {ran} ran, {equal} fixed points, {} differ, {} Lua errors; {warm_checked} warm-cache runs checked against PoB's",
        builds.len(),
        failures.len(),
        errored.len()
    );
    assert!(failures.is_empty(), "D13 fixed point broken: {failures:#?}");
    assert!(ran > 0, "no build ran");
    // Runs that raise are upstream-stub gaps (CS2 / PF-B leave nil outputs that Lua code then
    // indexes); bounded at the measured value (docs/port/FOLLOWUPS.md "CA").
    assert!(
        errored.len() <= D13_MAX_ERRORS,
        "{} D13 runs raised (bound {D13_MAX_ERRORS}): {errored:#?}",
        errored.len()
    );
}

/// Measured on port/CA (rust-port f275edb + CA): see docs/port/FOLLOWUPS.md "CA". With CS2:
/// 1 (upstream_Dual_Savior, the mirage-skill `Weak` actor of the L6 floors' note; FOLLOWUPS.md
/// "CA" 6).
const D13_MAX_ERRORS: usize = 1;

/// Builds whose second `buildOutput` on a warm GlobalCache differs from the first in PoB itself
/// (D13 is then not a property of the Lua; docs/port/FOLLOWUPS.md "CA" 5), with values of PoB's
/// warm run. Measured with `pob_headless` (POB_ENGINE=jit, rust-port d8a4407): wipe,
/// `calcs.buildOutput(build, "MAIN")` twice: allflame_exp_050 differs in 26 output keys
/// (BleedStacks 0.5875 -> 1.25, CombinedDPS 7,411,955.92 -> 15,755,502.09, ...); with
/// `wipeGlobalCache()` between the runs, 0 of 261,774 keys differ. For these builds the test
/// checks the warm run against these values and the fixed point after a wipe.
const D13_POB_WARM_CACHE: &[(&str, &[(&str, f64)])] = &[(
    "allflame_exp_050",
    &[
        ("BleedStacks", 1.25),
        ("BleedDPS", 80_571.900_725_94),
        ("LifeLeechInstances", 5.0),
        ("TotalDPS", 10_934_096.884_62),
        ("CombinedDPS", 15_755_502.093_183),
    ],
)];

#[cfg(feature = "luajit")]
mod oracle_side {
    use super::*;

    use pob_calc::global_cache::{set_cachedData, CachedData};
    use pob_calc::modules::calcs::{
        buildOutput_calcs, buildOutput_costWarnings, buildOutput_main,
    };
    use pob_calc::modules::common::cacheSkillUUID;
    use pob_oracle::jit::JitPob;
    use pob_oracle::oracle::ORACLE_LUA;
    use pob_oracle::runner::{probe_prelude, OracleLock, ProbeOptions};
    use pob_oracle::schema::StateDump;

    const CA_LUA: &str = include_str!("../corpus/CA/ca.lua");

    fn stages_file() -> PathBuf {
        PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/CA/stages.toml")
    }

    /// PoB's run of a build: the `ca:*` probe dumps and `ca.lua`'s JSON, cached in
    /// `target/tmp/ca-probe/<stem>/`.
    pub(super) struct PobRun {
        pub dir: PathBuf,
        pub hits: J,
        pub ca: J,
    }

    pub(super) fn pob_run(xml: &Path) -> Result<PobRun, String> {
        let stem = stem(xml);
        let dir = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("ca-probe").join(stem);
        let read = |dir: &Path| -> Result<PobRun, String> {
            let j = |f: &str| -> Result<J, String> {
                let t = std::fs::read_to_string(dir.join(f)).map_err(|e| format!("{f}: {e}"))?;
                serde_json::from_str(&t).map_err(|e| format!("{f}: {e}"))
            };
            Ok(PobRun { dir: dir.to_path_buf(), hits: j("hits.json")?, ca: j("ca.json")? })
        };
        if dir.join("ca.json").is_file() && dir.join("hits.json").is_file() {
            return read(&dir);
        }
        if let Ok(e) = std::fs::read_to_string(dir.join("error.txt")) {
            return Err(e);
        }
        let src = find_pob_src().ok_or("no PoB checkout")?;
        let root = PathBuf::from(env!("CARGO_TARGET_TMPDIR")).join("oracle-src");
        let patched = pob_oracle::patcher::patch(&src, Some(&stages_file()), &root)
            .map_err(|e| format!("patch: {e}"))?;
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(dir.join("state")).map_err(|e| e.to_string())?;
        let xml_text = std::fs::read_to_string(xml).map_err(|e| e.to_string())?;
        let result = (|| -> Result<(), String> {
            let _lock = OracleLock::acquire().map_err(|e| format!("oracle lock: {e}"))?;
            let out_dir = std::fs::canonicalize(&dir).map_err(|e| e.to_string())?;
            let src_root = std::fs::canonicalize(&patched.src).map_err(|e| e.to_string())?;
            let mut root_s = src_root.to_string_lossy().replace('\\', "/");
            if !root_s.ends_with('/') {
                root_s.push('/');
            }
            let opts = ProbeOptions { select: vec!["ca:*".to_owned()], max_dumps: 40, provenance: false };
            let pre = probe_prelude(&root_s, &out_dir, stem, &opts);
            let mut pob = JitPob::boot_oracle(&src_root, Some(&pre))?;
            pob.run(ORACLE_LUA)?;
            use pob_oracle::headless::Engine;
            pob.load_build_xml(&xml_text, stem)?;
            pob.run("return __probe_finish(__mainObject__.main.modes['BUILD'].calcsTab.mainEnv)")?;
            let ca = pob.run(CA_LUA)?;
            let text = ca.first().map(|r| r.text().to_owned()).unwrap_or_default();
            pob_oracle::runner::write_atomic(&out_dir.join("ca.json"), &text).map_err(|e| e.to_string())?;
            Ok(())
        })();
        if let Err(e) = result {
            let _ = std::fs::write(dir.join("error.txt"), &e);
            return Err(e);
        }
        read(&dir)
    }

    /// The `ca:*` dumps of PoB's kept BuildOutput: the MAIN env (`calcsTab.mainEnv`) and the
    /// CALCS env that follows it. → (main: probe → file, calcs: probe → file).
    pub(super) fn kept_hits(hits: &J) -> (BTreeMap<String, String>, BTreeMap<String, String>) {
        let main_env = hits["mainEnv"].as_str().unwrap_or_default();
        let dumps: Vec<(String, String, String)> = hits["dumps"]
            .as_array()
            .map(|a| {
                a.iter()
                    .map(|d| {
                        let s = |k: &str| d[k].as_str().unwrap_or_default().to_owned();
                        (s("hit"), s("env"), s("file"))
                    })
                    .collect()
            })
            .unwrap_or_default();
        let probe = |hit: &str| hit.split('#').next().unwrap_or_default().to_owned();
        let mut main = BTreeMap::new();
        let mut calcs = BTreeMap::new();
        let mut last_main = None;
        for (i, (hit, env, file)) in dumps.iter().enumerate() {
            if env == main_env {
                main.insert(probe(hit), file.clone());
                last_main = Some(i);
            }
        }
        if let Some(i) = last_main {
            let calcs_env = dumps[i + 1..]
                .iter()
                .find(|(h, _, _)| probe(h) == "ca:399")
                .map(|(_, e, _)| e.clone());
            if let Some(ce) = calcs_env {
                for (hit, env, file) in &dumps[i + 1..] {
                    if *env == ce {
                        calcs.insert(probe(hit), file.clone());
                    }
                }
            }
        }
        (main, calcs)
    }

    fn load(run: &PobRun, file: &str) -> Result<StateDump, String> {
        StateDump::load(&run.dir.join(file)).map_err(|e| e.to_string())
    }

    /// `"$id" → "$uuid"` of every active skill in a dump.
    fn skill_uuids(j: &J, out: &mut BTreeMap<u64, String>) {
        match j {
            J::Object(m) => {
                if let (Some(id), Some(u)) = (m.get("$id").and_then(J::as_u64), m.get("$uuid").and_then(J::as_str)) {
                    out.insert(id, u.to_owned());
                }
                for v in m.values() {
                    skill_uuids(v, out);
                }
            }
            J::Array(a) => a.iter().for_each(|v| skill_uuids(v, out)),
            _ => {}
        }
    }

    /// `cacheSkillUUID` reads `build.skillsTab.socketGroupList` (Common.lua 913-918), which a
    /// pob-state dump does not carry (the build is a marker): rebuild it with every injected
    /// skill's socket group at the index PoB's uuid names (other slots are empty groups).
    /// A skill the dump carries no `$uuid` for (PoB had not cached it before the loop: the loop
    /// builds it) takes its uuid from `known` (PoB's GlobalCache keys after the load, ca.json
    /// `costs`) when exactly one unused key has its `name_SLOT_slotIndx_` prefix, which does not
    /// depend on the group list.
    /// → the skills whose Rust uuid still differs from PoB's.
    fn fill_socket_groups(dump: &StateDump, env: &EnvRef, known: &[String]) -> Vec<String> {
        let mut uuids = BTreeMap::new();
        skill_uuids(&dump.root, &mut uuids);
        let list_json = dump.root["env"]["player"]["activeSkillList"].as_array().cloned().unwrap_or_default();
        let skills = env.player.activeSkillList.get().snapshot();
        let dumped = |i: usize| -> Option<String> {
            let e = list_json.get(i)?;
            let id = e.get("$id").or_else(|| e.get("$ref")).and_then(J::as_u64)?;
            uuids.get(&id).cloned()
        };
        let used: BTreeSet<String> = (0..skills.len()).filter_map(dumped).collect();
        let mut fallback: BTreeMap<usize, String> = BTreeMap::new();
        for (i, s) in skills.iter().enumerate() {
            if dumped(i).is_some() || i >= list_json.len() {
                continue;
            }
            let ours = cacheSkillUUID(s, env).to_string();
            let Some((prefix, _)) = ours.rsplit_once('_') else { continue };
            let prefix = format!("{prefix}_");
            let cands: Vec<&String> = known
                .iter()
                .filter(|k| !used.contains(*k))
                .filter(|k| k.strip_prefix(&prefix).is_some_and(|x| x.parse::<usize>().is_ok()))
                .collect();
            if let [one] = cands[..] {
                if !fallback.values().any(|v| v == one) {
                    fallback.insert(i, one.clone());
                }
            }
        }
        let pob_uuid = |i: usize| -> Option<String> { dumped(i).or_else(|| fallback.get(&i).cloned()) };
        let mut slots: BTreeMap<usize, SocketGroupRef> = BTreeMap::new();
        for (i, s) in skills.iter().enumerate() {
            let (Some(u), Some(g)) = (pob_uuid(i), s.socketGroup.get()) else { continue };
            if let Some(idx) = u.rsplit('_').next().and_then(|x| x.parse::<usize>().ok()) {
                slots.entry(idx).or_insert(g);
            }
        }
        let n = slots.keys().max().copied().unwrap_or(0);
        let list: Vec<SocketGroupRef> = (1..=n)
            .map(|i| slots.get(&i).cloned().unwrap_or_else(|| Rc::new(SocketGroup::default())))
            .collect();
        env.build.skillsTab.socketGroupList.set(Shared::new(list));
        let mut bad = Vec::new();
        for (i, s) in skills.iter().enumerate() {
            if let Some(u) = pob_uuid(i) {
                let ours = cacheSkillUUID(s, env);
                if ours.as_str() != u {
                    bad.push(format!("skill {}: Rust uuid {ours} vs PoB {u}", i + 1));
                }
            }
        }
        bad
    }

    struct Injected {
        build: BuildRef,
        env: EnvRef,
        output: Output,
    }

    fn inject(dump: &StateDump) -> Result<Injected, String> {
        let build = pob_oracle::testkit::empty_build();
        let inj = pob_oracle::inject::inject(dump, &build)?;
        let env = inj.env.clone().ok_or("no env in the dump")?;
        let output = inj
            .locals
            .get("output")
            .and_then(Val::table)
            .ok_or("no `output` local in the dump")?;
        Ok(Injected { build, env, output })
    }

    static NULL: J = J::Null;

    /// `$ref` resolution along a path of a dump (the referenced tables by `$id`, borrowed).
    struct Res<'a> {
        root: &'a J,
        ids: std::collections::HashMap<u64, &'a J>,
    }

    impl<'a> Res<'a> {
        fn new(root: &'a J) -> Self {
            fn collect<'a>(v: &'a J, ids: &mut std::collections::HashMap<u64, &'a J>) {
                match v {
                    J::Object(m) => {
                        if let Some(id) = m.get("$id").and_then(J::as_u64) {
                            ids.entry(id).or_insert(v);
                        }
                        m.values().for_each(|c| collect(c, ids));
                    }
                    J::Array(a) => a.iter().for_each(|c| collect(c, ids)),
                    _ => {}
                }
            }
            let mut ids = std::collections::HashMap::new();
            collect(root, &mut ids);
            Res { root, ids }
        }

        fn deref(&self, mut v: &'a J) -> &'a J {
            for _ in 0..8 {
                let Some(id) = v
                    .as_object()
                    .filter(|m| m.len() == 1)
                    .and_then(|m| m.get("$ref"))
                    .and_then(J::as_u64)
                else {
                    break;
                };
                match self.ids.get(&id) {
                    Some(t) => v = t,
                    None => break,
                }
            }
            v
        }

        fn at(&self, path: &[&str]) -> &'a J {
            let mut v = self.deref(self.root);
            for p in path {
                v = self.deref(v.get(*p).unwrap_or(&NULL));
            }
            v
        }

        /// `at(path)` with its members (and their list elements / members) dereferenced once.
        fn shallow(&self, path: &[&str]) -> J {
            let one = |c: &'a J| -> J {
                match self.deref(c) {
                    J::Array(a) => J::Array(a.iter().map(|x| self.deref(x).clone()).collect()),
                    J::Object(m) => J::Object(m.iter().map(|(k, x)| (k.clone(), self.deref(x).clone())).collect()),
                    other => other.clone(),
                }
            };
            match self.at(path) {
                J::Object(m) => J::Object(m.iter().map(|(k, c)| (k.clone(), one(c))).collect()),
                other => other.clone(),
            }
        }
    }

    fn is_warning_key(k: &str) -> bool {
        let top = k.split('.').next().unwrap_or_default();
        top == "EnergyShieldProtectsMana" || top.ends_with("Warning") || top.ends_with("WarningList")
    }

    /// GlobalCache.cachedData.MAIN as the cost-warning loop reads it after PoB's loop
    /// (`ca.json` costs). Entries whose env is the injected env are kept as injected.
    fn fill_cost_cache(inj: &Injected, costs: &J) -> usize {
        let Some(costs) = costs.as_object() else { return 0 };
        let skills = inj.env.player.activeSkillList.get().snapshot();
        let mut n = 0;
        for (uuid, c) in costs {
            let uuid = Name::from(uuid.as_str());
            let existing = pob_calc::global_cache::cachedData(Mode::MAIN, uuid);
            if existing.as_ref().is_some_and(|e| Rc::ptr_eq(&e.Env, &inj.env)) {
                continue;
            }
            let Some(any) = existing
                .as_ref()
                .map(|e| e.ActiveSkill.clone())
                .or_else(|| skills.iter().find(|s| cacheSkillUUID(s, &inj.env) == uuid).cloned())
                .or_else(|| skills.first().cloned())
            else {
                continue;
            };
            let env = Env::new(&inj.build, Mode::MAIN, Rc::new(Override::default()), inj.env.spec.get());
            let out = Table::new();
            if let Some(o) = c["output"].as_object() {
                for (k, v) in o {
                    if let Some(x) = pob_oracle::oracle::json_num(v) {
                        out.set(Name::from(k.as_str()), x);
                    }
                }
            }
            env.player.output.set(out);
            if c["hasMainSkill"].as_bool() == Some(true) {
                let ms = ActiveSkill::new(any.activeEffect.get(), Shared::new(Vec::new()), Weak::new());
                let sd = Table::new();
                if let Some(o) = c["skillData"].as_object() {
                    for (k, v) in o {
                        if let Some(x) = pob_oracle::oracle::json_num(v) {
                            sd.set(Name::from(k.as_str()), x);
                        }
                    }
                }
                ms.skillData.set(sd);
                env.player.mainSkill.set(Some(ms));
            }
            set_cachedData(
                Mode::MAIN,
                uuid,
                Some(Rc::new(CachedData {
                    Name: any.activeEffect.get().grantedEffect.get().name,
                    Speed: Val::Nil,
                    HitSpeed: Val::Nil,
                    ManaCost: Val::Nil,
                    LifeCost: Val::Nil,
                    ESCost: Val::Nil,
                    RageCost: Val::Nil,
                    HitChance: Val::Nil,
                    AccuracyHitChance: Val::Nil,
                    PreEffectiveCritChance: Val::Nil,
                    CritChance: Val::Nil,
                    TotalDPS: Val::Nil,
                    ActiveSkill: any,
                    Env: env,
                })),
            );
            n += 1;
        }
        n
    }

    fn cache_len() -> usize {
        pob_calc::global_cache::GlobalCache.with(|g| g.borrow().cachedData[Mode::MAIN.index()].len())
    }

    #[derive(Default)]
    struct Block {
        compared: usize,
        clean: usize,
        not_run: Vec<String>,
        limited: Vec<String>,
        failed: Vec<String>,
        /// PoB values (output keys, used-tracking keys + mods, breakdown rows) compared.
        values: usize,
    }

    /// Prefix of an `Err` that is a limit of the injection harness, not a CA result.
    const LIMITED: &str = "limited: ";

    impl Block {
        fn record(&mut self, stem: &str, what: &str, r: Result<Vec<String>, String>) {
            let n = take_compared();
            if r.is_ok() {
                self.values += n;
            }
            match r {
                Err(e) if e.starts_with(LIMITED) => {
                    eprintln!("LIMITED {stem} {what}: {}", &e[LIMITED.len()..]);
                    self.limited.push(format!("{stem}: {}", &e[LIMITED.len()..]));
                }
                Ok(d) => {
                    self.compared += 1;
                    if d.is_empty() {
                        self.clean += 1;
                    } else {
                        eprintln!("FAIL {stem} {what}: {} differences", d.len());
                        for x in d.iter().take(if verbose() { 200 } else { 8 }) {
                            eprintln!("       {x}");
                        }
                        self.failed.push(format!("{stem}: {} differences", d.len()));
                    }
                }
                Err(e) => {
                    eprintln!("FAIL {stem} {what}: {e}");
                    self.failed.push(format!("{stem}: {e}"));
                }
            }
        }
    }

    /// Cost warnings: inject ca:399 (MAIN), PoB's cache, run, diff ca:437's warning keys.
    fn cost_block(run: &PobRun, main: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
        let din = load(run, main.get("ca:399").ok_or("no MAIN ca:399 dump")?)?;
        let dout = load(run, main.get("ca:437").ok_or("no MAIN ca:437 dump")?)?;
        let inj = inject(&din)?;
        let known: Vec<String> =
            run.ca["costs"].as_object().map(|m| m.keys().cloned().collect()).unwrap_or_default();
        let bad = fill_socket_groups(&din, &inj.env, &known);
        if !bad.is_empty() {
            return Err(format!("{LIMITED}skill uuids not reproducible from the dump: {bad:?}"));
        }
        let mut d = Vec::new();
        fill_cost_cache(&inj, &run.ca["costs"]);
        // A skill PoB leaves uncached after its loop makes the Rust loop call buildActiveSkill,
        // i.e. the full calc on the injected placeholder build: not reproducible here. PoB does
        // this for The Saviour's Reflection: its mirage postCalcFunc (CalcMirages.lua:154-159)
        // makes the mirrored attack `env.player.mainSkill`, so `cacheData` (CalcPerform.lua:3989)
        // files the nested calc under that skill's uuid (docs/port/FOLLOWUPS.md "CA" 4).
        let missing: Vec<String> = inj
            .env
            .player
            .activeSkillList
            .get()
            .snapshot()
            .iter()
            .map(|s| cacheSkillUUID(s, &inj.env))
            .filter(|u| pob_calc::global_cache::cachedData(Mode::MAIN, *u).is_none())
            .map(|u| u.to_string())
            .collect();
        if !missing.is_empty() {
            return Err(format!("{LIMITED}no MAIN cache entry in PoB after the loop for {missing:?}"));
        }
        let before = cache_len();
        catch(|| buildOutput_costWarnings(&inj.env, Mode::MAIN, &inj.output))?;
        if cache_len() != before {
            d.push(format!("the loop built {} skills PoB had cached (harness gap)", cache_len() - before));
        }
        let ours: BTreeMap<String, Leaf> =
            flat_table(&inj.output).into_iter().filter(|(k, _)| is_warning_key(k)).collect();
        let pob: BTreeMap<String, Leaf> = flat_json(&Res::new(&dout.root).shallow(&["locals", "output"]))
            .into_iter()
            .filter(|(k, _)| is_warning_key(k))
            .collect();
        leaves_diff("output", &ours, &pob, &mut d);
        Ok(d)
    }

    const USED: [&str; 9] = [
        "conditionsUsed",
        "enemyConditionsUsed",
        "minionConditionsUsed",
        "multipliersUsed",
        "enemyMultipliersUsed",
        "perStatsUsed",
        "enemyPerStatsUsed",
        "tagTypesUsed",
        "modsUsed",
    ];

    fn env_used(env: &EnvRef, name: &str) -> Option<Table> {
        match name {
            "skillsUsed" => env.skillsUsed.get(),
            "conditionsUsed" => env.conditionsUsed.get(),
            "enemyConditionsUsed" => env.enemyConditionsUsed.get(),
            "minionConditionsUsed" => env.minionConditionsUsed.get(),
            "multipliersUsed" => env.multipliersUsed.get(),
            "enemyMultipliersUsed" => env.enemyMultipliersUsed.get(),
            "perStatsUsed" => env.perStatsUsed.get(),
            "enemyPerStatsUsed" => env.enemyPerStatsUsed.get(),
            "tagTypesUsed" => env.tagTypesUsed.get(),
            "modsUsed" => env.modsUsed.get(),
            _ => None,
        }
    }

    fn is_spec_key(k: &str) -> bool {
        k == "ExtraPoints" || k.starts_with("Spec:")
    }

    /// Spec sums + used-tracking: inject ca:437 (MAIN), run, diff ca:869.
    fn main_block(run: &PobRun, main: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
        let din = load(run, main.get("ca:437").ok_or("no MAIN ca:437 dump")?)?;
        let dout = load(run, main.get("ca:869").ok_or("no MAIN ca:869 dump")?)?;
        let inj = inject(&din)?;
        catch(|| buildOutput_main(&inj.env, &inj.output))?;
        let res = Res::new(&dout.root);
        let mut d = Vec::new();
        let ours: BTreeMap<String, Leaf> =
            flat_table(&inj.output).into_iter().filter(|(k, _)| is_spec_key(k)).collect();
        let theirs: BTreeMap<String, Leaf> = flat_json(&res.shallow(&["locals", "output"]))
            .into_iter()
            .filter(|(k, _)| is_spec_key(k))
            .collect();
        leaves_diff("output", &ours, &theirs, &mut d);
        // An `ItemCondition` tag with `searchCond` reads the item's mod lines
        // (Item:FindModifierSubstring), which pob-state item dumps do not carry: a `Spec:*` sum
        // over such a Tree mod is not reproducible from the dump.
        let db = inj.env.modDB.get();
        let item_cond = |names: &[&str]| {
            db.all_mods().iter().any(|m| {
                names.contains(&m.name.as_str())
                    && m.source_category() == Some("Tree")
                    && m.tags.iter().any(|t| t.r#type == TagType::ItemCondition && t.searchCond.is_some())
            })
        };
        let mut limited = Vec::new();
        d.retain(|x| {
            let names: &[&str] = match x.split(' ').nth(1).unwrap_or_default().trim_end_matches(':') {
                "Spec:LifeInc" => &["Life"],
                "Spec:ManaInc" => &["Mana"],
                "Spec:ArmourInc" => &["Armour", "ArmourAndEvasion"],
                "Spec:EvasionInc" => &["Evasion", "ArmourAndEvasion"],
                "Spec:EnergyShieldInc" => &["EnergyShield"],
                _ => &[],
            };
            if !names.is_empty() && item_cond(names) {
                limited.push(x.clone());
                false
            } else {
                true
            }
        });
        for name in std::iter::once("skillsUsed").chain(USED) {
            let pob = res.shallow(&["env", name]);
            used_diff(name, &used_rust(&env_used(&inj.env, name)), &used_pob(Some(&pob)), &mut d);
        }
        if d.is_empty() && !limited.is_empty() {
            return Err(format!("{LIMITED}item-condition Tree mods (injected items carry no mod lines): {limited:?}"));
        }
        Ok(d)
    }

    fn rows_rust(bd: Option<Breakdown>, key: &str) -> Vec<String> {
        let Some(bd) = bd else { return vec!["<no breakdown>".to_owned()] };
        let Some(t) = bd.table(Name::from(key)) else { return Vec::new() };
        let Some(l) = t.table(n!("modList")) else { return vec!["<no modList>".to_owned()] };
        l.ipairs()
            .into_iter()
            .map(|(_, r)| match r {
                Val::Table(r) => canon(&serde_json::json!({
                    "mod": val_json(&r.get(n!("mod"))),
                    "value": val_json(&r.get(n!("value"))),
                })),
                other => format!("<{}>", other.lua_type()),
            })
            .collect()
    }

    fn rows_diff(name: &str, ours: &[String], pob: &[String], out: &mut Vec<String>) {
        bump(pob.len());
        if ours != pob {
            let i = ours.iter().zip(pob).position(|(a, b)| a != b).unwrap_or(ours.len().min(pob.len()));
            out.push(format!(
                "{name}: {} rows vs PoB {}; first difference at row {}: Rust {:?} / PoB {:?}",
                ours.len(),
                pob.len(),
                i + 1,
                ours.get(i).map(|s| clip(s)),
                pob.get(i).map(|s| clip(s))
            ));
        }
    }

    fn is_list_key(k: &str) -> bool {
        matches!(
            k,
            "BuffList" | "CombatList" | "CurseList" | "Minion.BuffList" | "Minion.CombatList" | "Minion.CurseList"
        )
    }

    /// CALCS lists: inject the CALCS env's ca:399 (+ mainEnv.multipliersUsed), run, diff ca:869.
    fn calcs_block(run: &PobRun, calcs: &BTreeMap<String, String>) -> Result<Vec<String>, String> {
        let din = load(run, calcs.get("ca:399").ok_or("no CALCS ca:399 dump")?)?;
        let dout = load(run, calcs.get("ca:869").ok_or("no CALCS ca:869 dump")?)?;
        let inj = inject(&din)?;
        // calcsTab.mainEnv (Calcs.lua 635-650 read its multipliersUsed)
        let mainEnv = Env::new(&inj.build, Mode::MAIN, Rc::new(Override::default()), inj.env.spec.get());
        let mu = Table::new();
        if let Some(m) = run.ca["mainUsed"]["multipliersUsed"].as_object() {
            for k in m.keys() {
                mu.set(Name::from(k.as_str()), Table::new());
            }
        }
        mainEnv.multipliersUsed.set(Some(mu));
        inj.build.calcsTab.mainEnv.set(Some(mainEnv));
        catch(|| buildOutput_calcs(&inj.build, &inj.env, &inj.output))?;
        let res = Res::new(&dout.root);
        let mut d = Vec::new();
        let ours: BTreeMap<String, Leaf> =
            flat_table(&inj.output).into_iter().filter(|(k, _)| is_list_key(k)).collect();
        let mut out = res.shallow(&["locals", "output"]);
        if let J::Object(o) = &mut out {
            if o.contains_key("Minion") {
                o.insert("Minion".to_owned(), res.shallow(&["locals", "output", "Minion"]));
            }
        }
        let theirs: BTreeMap<String, Leaf> =
            flat_json(&out).into_iter().filter(|(k, _)| is_list_key(k)).collect();
        leaves_diff("output", &ours, &theirs, &mut d);
        let rows = |actor: &str, key: &str| -> Vec<String> {
            let bd = res.at(&["env", actor, "breakdown"]);
            if bd.get(key).is_none() {
                return Vec::new();
            }
            match res.at(&["env", actor, "breakdown", key, "modList"]) {
                J::Array(a) => a
                    .iter()
                    .map(|r| {
                        let r = res.deref(r);
                        canon(&serde_json::json!({
                            "mod": res.deref(r.get("mod").unwrap_or(&NULL)),
                            "value": res.deref(r.get("value").unwrap_or(&NULL)),
                        }))
                    })
                    .collect(),
                // an empty Lua table is dumped as `{}`
                J::Object(m) if m.keys().all(|k| k.starts_with('$')) => Vec::new(),
                _ => vec!["<no modList>".to_owned()],
            }
        };
        for key in ["SkillBuffs", "SkillDebuffs"] {
            rows_diff(&format!("player.breakdown.{key}"), &rows_rust(inj.env.player.breakdown.get(), key), &rows("player", key), &mut d);
        }
        if let Some(m) = inj.env.minion.get() {
            for key in ["SkillBuffs", "SkillDebuffs"] {
                rows_diff(&format!("minion.breakdown.{key}"), &rows_rust(m.breakdown.get(), key), &rows("minion", key), &mut d);
            }
        }
        Ok(d)
    }

    #[test]
    fn ca_l5_post() {
        let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(parity) = find_parity_dir() else { return skip(".parity not found") };
        if find_pob_src().is_none() {
            return skip("PoB checkout (.PathOfBuilding/src) not found");
        }
        if !init_data() {
            return skip("data/dump/data.bin not found");
        }
        quiet_lua_errors();
        let builds = corpus(&parity);
        if builds.is_empty() {
            return skip(".parity/builds/*.xml not found");
        }
        let (mut cost, mut main, mut calcs) = (Block::default(), Block::default(), Block::default());
        let mut pob_failed = Vec::new();
        for xml in &builds {
            let stem = stem(xml);
            let run = match pob_run(xml) {
                Ok(r) => r,
                Err(e) => {
                    eprintln!("PoB {stem}: {e}");
                    pob_failed.push(format!("{stem}: {e}"));
                    continue;
                }
            };
            let (mh, ch) = kept_hits(&run.hits);
            if mh.contains_key("ca:437") {
                cost.record(stem, "cost warnings", cost_block(&run, &mh));
                main.record(stem, "used-tracking", main_block(&run, &mh));
            } else {
                cost.not_run.push(stem.to_owned());
                main.not_run.push(stem.to_owned());
            }
            if ch.contains_key("ca:399") {
                calcs.record(stem, "CALCS lists", calcs_block(&run, &ch));
            } else {
                calcs.not_run.push(stem.to_owned());
            }
            pob_calc::global_cache::clear_all();
            pob_calc::probe::registry_reset();
        }
        for (name, b) in [("cost warnings (400-435)", &cost), ("used-tracking (437-600)", &main), ("CALCS lists (601-866)", &calcs)] {
            eprintln!(
                "CA L5 {name}: {} compared ({} PoB values), {} clean, {} failed, {} harness-limited, {} without dumps",
                b.compared,
                b.values,
                b.clean,
                b.failed.len(),
                b.limited.len(),
                b.not_run.len()
            );
        }
        eprintln!("CA L5: {} builds, {} PoB runs failed", builds.len(), pob_failed.len());
        assert!(pob_failed.is_empty(), "PoB runs failed: {pob_failed:#?}");
        let full = std::env::var_os("CA_BUILDS").is_none()
            && std::env::var_os("CA_LIMIT").is_none()
            && !pob_testpaths::quick_corpus();
        for ((name, b), max_limited) in
            [("cost warnings", &cost), ("used-tracking", &main), ("CALCS lists", &calcs)].into_iter().zip(L5_MAX_LIMITED)
        {
            assert!(b.failed.is_empty(), "CA L5 {name}: {:#?}", b.failed);
            assert!(b.compared > 0, "CA L5 {name}: nothing compared");
            if full {
                assert!(
                    b.limited.len() <= max_limited,
                    "CA L5 {name}: {} harness-limited builds (bound {max_limited}): {:#?}",
                    b.limited.len(),
                    b.limited
                );
            }
        }
    }

    /// Harness-limited builds per block on the all57 corpus (measured; see the reasons printed
    /// by the test): cost warnings: 1 build where PoB's loop leaves a skill uncached
    /// (upstream_Dual_Savior Reflection x2: the mirage re-keys the cache, `cost_block`), so the
    /// Rust loop would run the full calc; used-tracking: 2 builds (allflame_exp_016, _049) whose `Spec:LifeInc` sums a Tree mod
    /// with an `ItemCondition` searchCond tag (injected items carry no mod lines).
    const L5_MAX_LIMITED: [usize; 3] = [1, 2, 0];

    // ---- L6 -------------------------------------------------------------------------------------

    const HEADLINE: [&str; 13] = [
        "Life",
        "EnergyShield",
        "Mana",
        "Ward",
        "TotalEHP",
        "TotalDPS",
        "CombinedDPS",
        "FullDPS",
        "FullDotDPS",
        "FireResist",
        "ColdResist",
        "LightningResist",
        "ChaosResist",
    ];

    fn fmt_num(l: Option<&Leaf>) -> String {
        match l {
            Some(Leaf::Num(x)) => format!("{x:.6}"),
            Some(other) => format!("{other:?}"),
            None => "nil".to_owned(),
        }
    }

    #[derive(Default)]
    struct L6 {
        builds: usize,
        rust_errors: Vec<String>,
        /// stage (MAIN / CALCS / getMiscCalculator / load …) → builds whose stage raised.
        stage_errors: BTreeMap<String, usize>,
        headline: usize,
        headline_exact: usize,
        builds_headline_exact: usize,
        keys: usize,
        keys_exact: usize,
        calcs_lists: usize,
        calcs_lists_exact: usize,
        used_tables: usize,
        used_tables_equal: usize,
        misc: usize,
        misc_exact: usize,
    }

    #[test]
    fn ca_l6_corpus() {
        let _serial = SERIAL.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let Some(parity) = find_parity_dir() else { return skip(".parity not found") };
        if find_pob_src().is_none() {
            return skip("PoB checkout (.PathOfBuilding/src) not found");
        }
        if !init_data() {
            return skip("data/dump/data.bin not found");
        }
        quiet_lua_errors();
        setup_luts();
        let builds = corpus(&parity);
        if builds.is_empty() {
            return skip(".parity/builds/*.xml not found");
        }
        let mut t = L6::default();
        let mut pob_failed = Vec::new();
        for xml in &builds {
            let stem = stem(xml);
            let run = match pob_run(xml) {
                Ok(r) => r,
                Err(e) => {
                    pob_failed.push(format!("{stem}: {e}"));
                    continue;
                }
            };
            t.builds += 1;
            let rounds = (run.hits["counts"]["ca:399"].as_u64().unwrap_or(2) / 2) as usize;
            let text = std::fs::read_to_string(xml).unwrap_or_default();
            pob_calc::global_cache::clear_all();
            let r = rust_load(&text, rounds);
            lut::clear_timeless_jewel_luts(None);
            let build = match r {
                Ok((b, errors)) => {
                    for e in errors {
                        eprintln!("L6 {stem}: Rust {e}");
                        let stage = e.split(':').next().unwrap_or_default().to_owned();
                        *t.stage_errors.entry(stage).or_default() += 1;
                        t.rust_errors.push(format!("{stem}: {e}"));
                    }
                    b
                }
                Err(e) => {
                    eprintln!("L6 {stem}: Rust {e}");
                    *t.stage_errors.entry("load".to_owned()).or_default() += 1;
                    t.rust_errors.push(format!("{stem}: {e}"));
                    t.headline += HEADLINE.len();
                    continue;
                }
            };
            let ours = build.calcsTab.mainOutput.get().map(|o| flat_table(&o)).unwrap_or_default();
            let pob = flat_json(&run.ca["main"]);
            // headline keys
            let mut line = Vec::new();
            let mut all = true;
            for k in HEADLINE {
                let (o, p) = (ours.get(k), pob.get(k));
                t.headline += 1;
                let eq = match (o, p) {
                    (Some(a), Some(b)) => leaf_eq(a, b),
                    (None, None) => true,
                    _ => false,
                };
                if eq {
                    t.headline_exact += 1;
                } else {
                    all = false;
                    line.push(format!("{k} {} vs PoB {}", fmt_num(o), fmt_num(p)));
                }
            }
            if all {
                t.builds_headline_exact += 1;
            }
            // every key
            let mut exact = 0;
            for (k, pv) in &pob {
                if ours.get(k).is_some_and(|ov| leaf_eq(ov, pv)) {
                    exact += 1;
                }
            }
            let extra = ours.keys().filter(|k| !pob.contains_key(*k)).count();
            t.keys += pob.len() + extra;
            t.keys_exact += exact;
            // CALCS lists
            let oc = build.calcsTab.calcsOutput.get().map(|o| flat_table(&o)).unwrap_or_default();
            let pc = flat_json(&run.ca["calcs"]);
            for k in ["BuffList", "CombatList", "CurseList"] {
                t.calcs_lists += 1;
                if oc.get(k).zip(pc.get(k)).is_some_and(|(a, b)| leaf_eq(a, b)) {
                    t.calcs_lists_exact += 1;
                }
            }
            // used-tracking key sets
            if let Some(me) = build.calcsTab.mainEnv.get() {
                for name in USED {
                    t.used_tables += 1;
                    let o: BTreeSet<String> = env_used(&me, name)
                        .map(|t| t.pairs().filter_map(|(k, _)| k.name().map(|n| n.to_string())).collect())
                        .unwrap_or_default();
                    let p: BTreeSet<String> = run.ca["mainUsed"][name]
                        .as_object()
                        .map(|m| m.keys().cloned().collect())
                        .unwrap_or_default();
                    if o == p {
                        t.used_tables_equal += 1;
                    }
                }
            }
            // misc calculator
            if let Some(mc) = build.calcsTab.miscCalculator.get() {
                let calls = catch(|| {
                    (
                        flat_table(&mc.call(Rc::new(Override::default()), None)),
                        flat_table(&mc.call(Rc::new(Override::default()), Some(false))),
                    )
                });
                lut::clear_timeless_jewel_luts(None);
                // a raising call counts as a Rust error (stage "misc call") and its PoB
                // values as not exact
                let (a, b) = match calls {
                    Ok((a, b)) => (Some(a), Some(b)),
                    Err(e) => {
                        eprintln!("L6 {stem}: Rust misc call: {e}");
                        *t.stage_errors.entry("misc call".to_owned()).or_default() += 1;
                        t.rust_errors.push(format!("{stem}: misc call: {e}"));
                        (None, None)
                    }
                };
                for (which, ours) in [("callDefault", a), ("callNoFullDPS", b)] {
                    let p = flat_json(&run.ca["misc"][which]);
                    for (k, pv) in &p {
                        t.misc += 1;
                        if ours.as_ref().and_then(|o| o.get(k)).is_some_and(|ov| leaf_eq(ov, pv)) {
                            t.misc_exact += 1;
                        }
                    }
                }
            }
            eprintln!(
                "L6 {stem}: keys {exact}/{} exact ({extra} Rust-only); headline {}",
                pob.len(),
                if line.is_empty() { "exact".to_owned() } else { line.join("; ") }
            );
            pob_calc::global_cache::clear_all();
        }
        let pct = |a: usize, b: usize| if b == 0 { 0.0 } else { 100.0 * a as f64 / b as f64 };
        eprintln!(
            "CA L6: {} builds ({} PoB runs failed), {} Rust runs raised; headline {}/{} exact ({:.1}%), \
             {} builds with every headline key exact; all keys {}/{} exact ({:.2}%); CALCS lists {}/{}; \
             used-tracking key sets {}/{}; misc calculator {}/{}",
            t.builds,
            pob_failed.len(),
            t.rust_errors.len(),
            t.headline_exact,
            t.headline,
            pct(t.headline_exact, t.headline),
            t.builds_headline_exact,
            t.keys_exact,
            t.keys,
            pct(t.keys_exact, t.keys),
            t.calcs_lists_exact,
            t.calcs_lists,
            t.used_tables_equal,
            t.used_tables,
            t.misc_exact,
            t.misc
        );
        let mut messages: BTreeMap<String, usize> = BTreeMap::new();
        for e in &t.rust_errors {
            let msg = e.split_once(": ").map_or(e.as_str(), |(_, m)| m);
            *messages.entry(msg.to_owned()).or_default() += 1;
        }
        eprintln!("CA L6 Rust errors by stage: {:?}", t.stage_errors);
        for (m, n) in &messages {
            eprintln!("  {n:>3} x {m}");
        }
        assert!(pob_failed.is_empty(), "PoB runs failed: {pob_failed:#?}");
        // L6 is not met while CS2 / PF-B are stubs: floors at the measured values keep the
        // suite from getting worse (docs/port/FOLLOWUPS.md "CA"); full corpus only.
        if std::env::var_os("CA_BUILDS").is_none()
            && std::env::var_os("CA_LIMIT").is_none()
            && !pob_testpaths::quick_corpus()
        {
            assert!(t.headline_exact >= L6_MIN_HEADLINE_EXACT, "headline exact {} < floor {L6_MIN_HEADLINE_EXACT}", t.headline_exact);
            assert!(t.keys_exact >= L6_MIN_KEYS_EXACT, "keys exact {} < floor {L6_MIN_KEYS_EXACT}", t.keys_exact);
            assert!(t.rust_errors.len() <= L6_MAX_RUST_ERRORS, "{} Rust errors > bound {L6_MAX_RUST_ERRORS}", t.rust_errors.len());
            // per stage: a MAIN-stage error replacing a CALCS-stage one must not pass
            for (stage, n) in &t.stage_errors {
                let max = L6_MAX_STAGE_ERRORS.iter().find(|(s, _)| s == stage).map_or(0, |(_, m)| *m);
                assert!(*n <= max, "{n} Rust errors in stage {stage:?} > bound {max}");
            }
        }
    }

    /// Floors re-measured with CS2 (CalcSetup.lua 800-1450, rust-port c4628cb + CS2, 2026-11-10,
    /// full corpus): headline 728/741 exact (56 of 57 builds with every headline key exact), all
    /// keys 44,908/45,954 exact, CALCS lists 168/171, used-tracking 504/504, misc calculator
    /// 1226/1226. Before CS2: headline 194/741, keys 24,381/48,958, no Rust errors. The four
    /// Rust errors are all upstream_Dual_Savior (The Saviour's Reflection mirage, now reachable
    /// with items): `attempt to index field 'actor'` at calc_active_skill.rs:713, the mirage skill
    /// kept by `env.player.mainSkill = newSkill` (CalcMirages.lua:155) outlives its env, whose
    /// actor is a `Weak` back-pointer, and CALCS then has no `mainEnv` (docs/port/FOLLOWUPS.md
    /// "CA" 6, owners MI/AS).
    const L6_MIN_HEADLINE_EXACT: usize = 728;
    const L6_MIN_KEYS_EXACT: usize = 44_908;
    const L6_MAX_RUST_ERRORS: usize = 4;
    /// The per-stage split of [`L6_MAX_RUST_ERRORS`] (measured the same way); a stage not
    /// listed (misc call, load …) is bounded by 0.
    const L6_MAX_STAGE_ERRORS: &[(&str, usize)] = &[
        ("MAIN", 1),
        ("CALCS", 1),
        ("getMiscCalculator", 1),
        ("configTab GetMiscCalculator", 1),
    ];
}
