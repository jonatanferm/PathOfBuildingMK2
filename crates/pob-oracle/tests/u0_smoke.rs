//! U0 smoke (calc doc §11.1 row U0): `buildOutput` runs end to end for every
//! slice5 build (`corpus/U0/slice5.txt`) in MAIN, CALCS and CALCULATOR modes
//! without panicking, and every run that goes through no orchestrator stub leaves a non-empty
//! player output table.
//!
//! The orchestrators run in Lua order: initEnv (CS1 → CS2 → CS3 → finish) and
//! perform (PF-C → PF-D → PF-E → PF-F → PF-G). The test deliberately does not
//! assert which `unported!` stub sites are hit (HYG2/R1): that set shrinks as
//! the CS/PF/CA units land, so the stub report is printed for information only.

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
    pob_calc::stub::stub_reset();
    let builds = slice5();
    assert_eq!(builds.len(), 5);
    let mut runs = 0;
    for stem in &builds {
        let Some(xml) = slice5_xml(stem) else {
            return; // POB_ALLOW_SKIP=1
        };
        for mode in [Mode::MAIN, Mode::CALCS, Mode::CALCULATOR] {
            let build = pob_calc::model::build_xml::loadBuildFromXML(&xml, pob_gamedata::data());
            let before = pob_calc::stub::stub_report();
            let env = pob_calc::try_buildOutput(&build, mode)
                .unwrap_or_else(|e| panic!("{stem} {mode:?}: Lua error {e}"));
            assert_eq!(env.mode, mode);
            // outputs present: a run that went through no orchestrator stub (initEnv, perform,
            // buildOutput's own tail) must leave a non-empty player output table. While such a
            // stub is still hit the output may be empty (HYG2: on rust-port 746b103 every run
            // hits CalcSetup.lua:488-1973, CalcPerform.lua:1289-3990 and Calcs.lua:384-870 and
            // leaves 0 output keys; docs/port/FOLLOWUPS.md "HYG2 exposed").
            let keys = env.player.output.get().pairs().count();
            let stubbed: Vec<&str> = stub_hits_since(&before)
                .into_iter()
                .filter(|s| ORCHESTRATOR_STUBS.iter().any(|p| s.starts_with(p)))
                .collect();
            assert!(
                keys > 0 || !stubbed.is_empty(),
                "{stem} {mode:?}: empty player output with no orchestrator stub hit"
            );
            eprintln!(
                "u0_smoke: {stem} {mode:?}: {keys} player output keys; orchestrator stubs hit: {}",
                stubbed.len()
            );
            runs += 1;
        }
    }
    assert_eq!(runs, 15, "5 slice5 builds x 3 modes");
    let report = pob_calc::stub::stub_report();
    eprintln!(
        "u0_smoke: {runs} buildOutput runs, {} distinct unported sites hit (information only):",
        report.len()
    );
    for (s, n) in &report {
        eprintln!("  {n:>4}  {s}");
    }
}

/// The orchestrator segments whose stubs may leave the output empty (calc doc §11.1 rows
/// CS1-CS3, PF-C..PF-G, CA; `pob_calc::stub` site prefixes).
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
