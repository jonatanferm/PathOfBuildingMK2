//! U0 smoke (calc doc §11.1 row U0): `buildOutput` runs end to end for every
//! slice5 build (`corpus/U0/slice5.txt`) in MAIN, CALCS and CALCULATOR modes,
//! and every run leaves a non-empty player output table.
//!
//! The orchestrators run in Lua order: initEnv (CS1 → CS2 → CS3 → finish) and
//! perform (PF-C → PF-D → PF-E → PF-F → PF-G); `buildOutput`'s own tail is CA's (ported).
//! CALCS mode reads `build.calcsTab.mainEnv` (Calcs.lua 635-650), so a CALCS run first runs
//! MAIN on the same build as `CalcsTab:BuildOutput` does (CalcsTab.lua 459-461).
//! A run may raise a Lua error only while it hits an `unported!` stub (a stub's default value,
//! e.g. a nil charge count from PF-B's `doActorCharges`, is then indexed or compared by ported
//! code, as Lua would raise on it); such runs are counted and bounded by `MAX_STUB_ERRORS`
//! (docs/port/FOLLOWUPS.md "HYG2 exposed"). The test deliberately does not assert which stub
//! sites are hit (HYG2/R1): the stub report is printed for information only.

use std::path::PathBuf;

use pob_calc::env::Mode;

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let Some(dir) = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump (run pob-dump)")
    else {
        return false; // POB_ALLOW_SKIP=1
    };
    if dir.join("data.bin").is_file() {
        pob_gamedata::init_from_dir(&dir).unwrap_or_else(|e| panic!("data.bin: {e}"));
    } else {
        let tree = pob_gamedata::load_dump_dir(&dir).unwrap_or_else(|e| panic!("{e}"));
        pob_gamedata::init_with(
            pob_gamedata::Data::from_tree(&tree).unwrap_or_else(|e| panic!("{e}")),
        );
    }
    true
}

/// The slice5 entries (`corpus/U0/slice5.txt`: one per line, `#` starts a comment).
fn slice5() -> Vec<String> {
    let list = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/U0/slice5.txt");
    std::fs::read_to_string(list)
        .unwrap()
        .lines()
        .map(|l| l.split('#').next().unwrap_or("").trim())
        .filter(|l| !l.is_empty())
        .map(str::to_owned)
        .collect()
}

/// A slice5 entry's XML: `corpus/U0/slice5/<entry>.xml` (the derived builds, CCR-0012), else
/// `.parity/builds/<entry>.xml`.
fn slice5_xml(entry: &str) -> Option<String> {
    let derived = PathBuf::from(env!("CARGO_MANIFEST_DIR"))
        .join("corpus/U0/slice5")
        .join(format!("{entry}.xml"));
    if derived.is_file() {
        return std::fs::read_to_string(derived).ok();
    }
    let xml = pob_testpaths::require(
        pob_testpaths::parity_build(entry),
        &format!("slice5 build {entry} (corpus/U0/slice5 or .parity/builds)"),
    )?;
    std::fs::read_to_string(xml).ok()
}

#[test]
fn build_output_on_stubs() {
    if !init_data() {
        return; // POB_ALLOW_SKIP=1
    }
    // Timeless jewel LUTs load from the PoB checkout (PS-B builds jewel node data through them;
    // slice5_allflame_exp_030 has a timeless jewel), as BM-A / PS-B register them.
    let Some(src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout (.PathOfBuilding/src)")
    else {
        return; // POB_ALLOW_SKIP=1
    };
    let src = std::fs::canonicalize(&src).unwrap_or(src);
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(pob_gamedata::timeless_lut::StdJewelFs {
        script_path: src.to_string_lossy().into_owned(),
        writable: false,
    }));
    pob_gamedata::timeless_lut::set_con_printf(|_| {});
    pob_calc::stub::stub_reset();
    let builds = slice5();
    assert_eq!(builds.len(), 5);
    let mut runs = 0;
    let mut stub_errors = Vec::new();
    for stem in &builds {
        let Some(xml) = slice5_xml(stem) else {
            return; // POB_ALLOW_SKIP=1
        };
        for mode in [Mode::MAIN, Mode::CALCS, Mode::CALCULATOR] {
            let build = pob_calc::model::build_xml::loadBuildFromXML(&xml, pob_gamedata::data());
            let before = pob_calc::stub::stub_report();
            if mode == Mode::CALCS {
                // CalcsTab:BuildOutput: self.mainEnv = buildOutput(MAIN) before the CALCS run
                let main = pob_calc::try_buildOutput(&build, Mode::MAIN)
                    .unwrap_or_else(|e| panic!("{stem} MAIN (for CALCS): Lua error {e}"));
                build.calcsTab.mainEnv.set(Some(main));
            }
            runs += 1;
            let stubbed = |before: &[(&'static str, u64)]| -> Vec<&'static str> {
                stub_hits_since(before)
                    .into_iter()
                    .filter(|s| ORCHESTRATOR_STUBS.iter().any(|p| s.starts_with(p)))
                    .collect()
            };
            let env = match pob_calc::try_buildOutput(&build, mode) {
                Ok(env) => env,
                Err(e) => {
                    // a Lua error is a stub gap only while the run hits a stub (module doc)
                    let hit = stubbed(&before);
                    assert!(!hit.is_empty(), "{stem} {mode:?}: Lua error {e} with no stub hit");
                    eprintln!("u0_smoke: {stem} {mode:?}: Lua error {e} (stubs hit: {hit:?})");
                    stub_errors.push(format!("{stem} {mode:?}: {e}"));
                    continue;
                }
            };
            assert_eq!(env.mode, mode);
            // outputs present: every run that completes leaves a non-empty player output table
            // (HYG2 measured 0 keys on rust-port 746b103, when initEnv, perform and buildOutput's
            // tail were stubs; docs/port/FOLLOWUPS.md "HYG2 exposed").
            let keys = env.player.output.get().pairs().count();
            assert!(keys > 0, "{stem} {mode:?}: empty player output");
            eprintln!(
                "u0_smoke: {stem} {mode:?}: {keys} player output keys; orchestrator stubs hit: {}",
                stubbed(&before).len()
            );
        }
    }
    assert_eq!(runs, 15, "5 slice5 builds x 3 modes");
    eprintln!("u0_smoke: {} of {runs} runs raised on a stub gap", stub_errors.len());
    assert!(
        stub_errors.len() <= MAX_STUB_ERRORS,
        "{} runs raised (bound {MAX_STUB_ERRORS}): {stub_errors:#?}",
        stub_errors.len()
    );
    let report = pob_calc::stub::stub_report();
    eprintln!(
        "u0_smoke: {runs} buildOutput runs, {} distinct unported sites hit (information only):",
        report.len()
    );
    for (s, n) in &report {
        eprintln!("  {n:>4}  {s}");
    }
}

/// Runs that may raise on a stub gap: none. History: on port/CA (rust-port f275edb + CA,
/// 2026-11-10) the 5 CALCS runs raised in `buildOutput`'s CALCS lists (Calcs.lua:605,
/// `output.PowerCharges > 0` with `PowerCharges` nil while PF-B's `doActorCharges` was a stub);
/// FUP2 measured 0 of 15 runs raising and lowered the bound from 5 to 0
/// (docs/port/FOLLOWUPS.md "HYG2 exposed").
const MAX_STUB_ERRORS: usize = 0;

/// The calc segments whose stubs may leave a value nil that ported code then raises on (calc
/// doc §11.1 rows CS1-CS3, PF-A..PF-G, CA; `pob_calc::stub` site prefixes).
const ORCHESTRATOR_STUBS: &[&str] = &[
    "Modules/CalcSetup.lua:",
    "Modules/CalcPerform.lua:",
    "Modules/Calcs.lua:",
    "Modules/Build.lua:",
];

/// The stub sites whose hit count grew since `before`.
fn stub_hits_since(before: &[(&'static str, u64)]) -> Vec<&'static str> {
    pob_calc::stub::stub_report()
        .into_iter()
        .filter(|(k, n)| !before.iter().any(|(b, m)| b == k && m >= n))
        .map(|(k, _)| k)
        .collect()
}
