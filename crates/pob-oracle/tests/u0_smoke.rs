//! U0 smoke (calc doc §11.1 row U0): `buildOutput` runs end to end on the
//! stubs for every slice5 build (`corpus/U0/slice5.txt`) in MAIN, CALCS and
//! CALCULATOR modes without panicking, and the stub report names the unported
//! sites it went through.
//!
//! On stubs the build loader (BM-A) yields an empty build, initEnv's CS1
//! skeleton creates the env, every segment returns `Flow::Normal`, and the
//! orchestrators run in Lua order: initEnv (CS1 → CS2 → CS3 → finish) and
//! perform (PF-C → PF-D → PF-E → PF-F → PF-G).

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
            let env = pob_calc::try_buildOutput(&build, mode)
                .unwrap_or_else(|e| panic!("{stem} {mode:?}: Lua error {e}"));
            // the pipeline produced an env with a player output table
            assert_eq!(env.mode, mode);
            let _ = env.player.output.get();
            runs += 1;
        }
    }
    let report = pob_calc::stub::stub_report();
    let sites: Vec<&str> = report.iter().map(|(s, _)| *s).collect();
    for expect in [
        "Modules/CalcSetup.lua:488-799",
        "Modules/CalcSetup.lua:800-1450",
        "Modules/CalcSetup.lua:1451-1973",
        "Modules/CalcPerform.lua:1289-1942",
        "Modules/CalcPerform.lua:3497-3990",
        "Modules/Calcs.lua:384-870",
    ] {
        assert!(
            sites.contains(&expect),
            "stub site {expect} not reached; report: {sites:?}"
        );
    }
    eprintln!(
        "u0_smoke: {runs} buildOutput runs on stubs, {} distinct unported sites hit:",
        report.len()
    );
    for (s, n) in &report {
        eprintln!("  {n:>4}  {s}");
    }
}
