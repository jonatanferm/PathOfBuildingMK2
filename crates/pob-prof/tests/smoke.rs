//! End-to-end smoke test of the driver (PROF1): boot the real app headless, open a fixture build,
//! hover and click, and check the measurements and guards. Needs the game data (`data/dump`, with
//! `data.bin`) and the app's assets (the PoB checkout), found through `pob-testpaths` and passed to
//! `paths::locate` explicitly: a missing input fails the test unless `POB_ALLOW_SKIP=1` (CCR-0031
//! B, STYLE.md). Runs in a debug build too (slower; the numbers are not checked).
//!
//! `committed_scenarios_cheap` replays committed scenarios whose guards cover clicks (so a layout
//! change that moves one of their controls fails here, not only in a release `prof.sh run`);
//! `committed_scenarios_all` (ignored: minutes in a debug build; `-- --ignored`) replays them all.
//! The tests share one lock: each holds an app (~0.6 GB), and they run one at a time.

use std::path::Path;
use std::sync::Mutex;
use std::time::Instant;

use pob_prof::driver::{self, RunOptions};
use pob_prof::paths;
use pob_prof::results;
use pob_prof::scenario::Scenario;

/// One app at a time (memory; the process-wide data and panic hook are set once anyway).
static ONE_APP: Mutex<()> = Mutex::new(());

fn inputs() -> Option<paths::Inputs> {
    let root = pob_testpaths::require(
        pob_testpaths::pob_root(),
        "PoB checkout (.PathOfBuilding: src/ and runtime/SimpleGraphic/)",
    )?;
    let bin = pob_testpaths::require(
        pob_testpaths::data_bin(),
        "data/dump/data.bin (pob-dump --compile)",
    )?;
    let dump = bin.parent().expect("data.bin has a directory");
    Some(paths::locate(Some(&root), Some(dump)).expect("paths::locate with explicit inputs"))
}

#[test]
fn boot_open_click_and_guards() {
    let _one = ONE_APP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let t_main = Instant::now();
    let Some(inputs) = inputs() else { return };
    let data_ms = driver::init_process(&inputs).expect("init");
    let user = paths::crate_dir().join("fixtures/user");
    let text = format!(
        "# smoke\nscreen 1920 1080\nfixture userdir={}\nstep boot\nframes 2\nexpect-mode LIST\n\
         expect-no-text \"Reset Tree/Tattoos\"\n\
         step open\nload-build \"Builds/allflame_exp_048.xml\"\nframes 3\nexpect-mode BUILD\nexpect-recalc\n\
         expect-text \"Reset Tree/Tattoos\"\n\
         snapshot a\nstep level\nmove 945 14\nclick LEFTBUTTON\nframes 2\nexpect-recalc\nsnapshot b\n\
         expect-differs a b\nstep idle\nframes 3\nexpect-no-recalc\n",
        user.display()
    );
    let sc = Scenario::parse(&text, Path::new("smoke.pobscript")).expect("parse");
    let r = driver::run(&sc, &inputs, &RunOptions::default(), t_main, data_ms);
    assert!(r.ok, "{:?}", r.error);
    let names: Vec<&str> = r.steps.iter().map(|s| s.name.as_str()).collect();
    assert_eq!(names, ["boot", "open", "level", "idle"]);
    let open = &r.steps[1];
    assert_eq!(open.recalcs, 1);
    assert!(open.build_load_ms.is_some() && open.recalc_ms > 0.0);
    // the traced frame of `expect-text` is not one of the step's frames
    assert!(open.frames == 3 && open.frame_max_ms >= open.frame_p50_ms);
    assert_eq!(r.steps[0].frames, 2);
    assert!(r.steps[2].recalcs == 1 && r.steps[3].recalcs == 0);
    assert!(r.startup.to_first_screen_ms > 0.0 && r.startup.data_ms > 0.0);
    // the summary of one repetition, and the JSON round trip the parent relies on
    let json = serde_json::to_string(&r).expect("json");
    let back: driver::RepResult = serde_json::from_str(&json).expect("parse json");
    let m = results::summarize(&[back], 0);
    assert!(m.contains_key("open.wall_ms") && m.contains_key("startup.to_first_screen_ms"));

    // a failing guard fails the run and names the step
    let bad = Scenario::parse(
        &format!(
            "# bad\nfixture userdir={}\nstep boot\nframes 1\nexpect-recalc\n",
            user.display()
        ),
        Path::new("bad.pobscript"),
    )
    .expect("parse");
    let r = driver::run(&bad, &inputs, &RunOptions::default(), Instant::now(), 0.0);
    assert!(!r.ok);
    let e = r.error.unwrap_or_default();
    assert!(
        e.contains("step \"boot\"") && e.contains("expect-recalc"),
        "{e}"
    );

    // a text guard that misses fails the run too, and lists what was drawn
    let bad = Scenario::parse(
        &format!(
            "# bad\nfixture userdir={}\nstep boot\nframes 1\nexpect-text \"no such string\"\n",
            user.display()
        ),
        Path::new("bad.pobscript"),
    )
    .expect("parse");
    let r = driver::run(&bad, &inputs, &RunOptions::default(), Instant::now(), 0.0);
    assert!(!r.ok);
    let e = r.error.unwrap_or_default();
    assert!(
        e.contains("step \"boot\"") && e.contains("expect-text \"no such string\": not found"),
        "{e}"
    );
}

/// Replay committed scenarios once each (their guards must pass and every step must run).
fn run_committed(names: &[&str]) {
    let _one = ONE_APP
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(inputs) = inputs() else { return };
    let data_ms = driver::init_process(&inputs).expect("init");
    let dir = paths::scenario_dir();
    for name in names {
        let t0 = Instant::now();
        let p = pob_prof::scenario::find(&dir, name).expect("scenario");
        let sc = Scenario::load(&p).expect("load");
        let r = driver::run(
            &sc,
            &inputs,
            &RunOptions::default(),
            Instant::now(),
            data_ms,
        );
        assert!(r.ok, "{name}: {:?}", r.error);
        let missing = driver::missing_steps(&sc, &r);
        assert!(missing.is_empty(), "{name}: steps not run: {missing:?}");
        eprintln!(
            "{name}: ok, {} steps, {:.1} s",
            r.steps.len(),
            t0.elapsed().as_secs_f64()
        );
    }
}

/// The committed scenarios with click-driven text guards that are cheap in a debug build.
#[test]
fn committed_scenarios_cheap() {
    run_committed(&["k_import", "f_skills", "j_compare"]);
}

/// Every committed scenario (minutes in a debug build): `-- --ignored`.
#[test]
#[ignore = "every committed scenario: minutes in a debug build"]
fn committed_scenarios_all() {
    let all = pob_prof::scenario::list(&paths::scenario_dir()).expect("list");
    let names: Vec<String> = all
        .iter()
        .filter_map(|p| p.file_stem().map(|s| s.to_string_lossy().into_owned()))
        .collect();
    let names: Vec<&str> = names.iter().map(String::as_str).collect();
    run_committed(&names);
}
