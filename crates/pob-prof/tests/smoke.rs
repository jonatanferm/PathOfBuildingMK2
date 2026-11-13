//! End-to-end smoke test of the driver (PROF1): boot the real app headless, open a fixture build,
//! hover and click, and check the measurements and guards. Needs the game data (data.bin) and the
//! app's assets (crate::paths); without them it is skipped with a message, unless
//! `POB_PROF_REQUIRE_INPUTS=1`. Runs in a debug build too (slower; the numbers are not checked).

use std::path::Path;
use std::time::Instant;

use pob_prof::driver::{self, RunOptions};
use pob_prof::paths;
use pob_prof::results;
use pob_prof::scenario::Scenario;

fn inputs() -> Option<paths::Inputs> {
    match paths::locate(None, None) {
        Ok(i) => Some(i),
        Err(e) => {
            assert!(
                std::env::var("POB_PROF_REQUIRE_INPUTS").as_deref() != Ok("1"),
                "inputs required: {e}"
            );
            eprintln!("SKIP: {e}");
            None
        }
    }
}

#[test]
fn boot_open_click_and_guards() {
    let t_main = Instant::now();
    let Some(inputs) = inputs() else { return };
    let data_ms = driver::init_process(&inputs).expect("init");
    let user = paths::crate_dir().join("fixtures/user");
    let text = format!(
        "# smoke\nscreen 1920 1080\nfixture userdir={}\nstep boot\nframes 2\nexpect-mode LIST\n\
         step open\nload-build \"Builds/allflame_exp_048.xml\"\nframes 3\nexpect-mode BUILD\nexpect-recalc\n\
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
    assert!(open.frames == 3 && open.frame_max_ms >= open.frame_p50_ms);
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
}
