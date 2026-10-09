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
use pob_oracle::oracle::{find_parity_dir, find_up};

fn skip(why: &str) {
    let required = std::env::var_os("POB_ORACLE_REQUIRE").is_some_and(|v| v == "1")
        || std::env::var_os("CI").is_some_and(|v| !v.is_empty() && v != "false" && v != "0");
    assert!(!required, "required input missing: {why}");
    eprintln!("SKIPPED: {why}");
}

fn init_data() -> bool {
    if pob_gamedata::is_initialised() {
        return true;
    }
    let dir = std::env::var_os("POB_DUMP_DIR")
        .map(PathBuf::from)
        .or_else(|| find_up("data/dump/manifest.json").map(|p| p.parent().unwrap().to_path_buf()));
    let Some(dir) = dir else { return false };
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

fn slice5() -> Vec<String> {
    let list = PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("corpus/U0/slice5.txt");
    std::fs::read_to_string(list)
        .unwrap()
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty() && !l.starts_with('#'))
        .map(str::to_owned)
        .collect()
}

#[test]
fn build_output_on_stubs() {
    if !init_data() {
        return skip("data/dump not found");
    }
    let Some(parity) = find_parity_dir() else {
        return skip(".parity/builds not found");
    };
    pob_calc::stub::stub_reset();
    let builds = slice5();
    assert_eq!(builds.len(), 5);
    let mut runs = 0;
    for stem in &builds {
        let xml =
            std::fs::read_to_string(parity.join("builds").join(format!("{stem}.xml"))).unwrap();
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
