//! U53 perf pass (ui doc §12.7 U53 row "perf pass (tree < 4 ms at 1080p)", §13 item 7): the
//! Rust app alone (`pob_app::PobApp` with the Rust calc, no Lua side) on a 1920x1080 @1.0
//! framebuffer, the passive tree of each fixture build drawn for 60+ frames: idle, hovering
//! nodes (tooltips with the calc's stat differences, path highlighting), zoomed out (every node on
//! screen) and zoomed in, dragged. Owner: U53.
//!
//! Measured: the Tree tab's `Draw` (TreeTab:Draw with its PassiveTreeView:Draw, through the
//! shell's `draw_tab`, ShellHooks) per frame, without the oracle's trace recorder. Frames that
//! recalculate (`buildFlag`, the first frames after the load) are reported apart: their cost is the
//! calc's, not the tree's. The gate is the median and the 95th percentile of the tree draw of the
//! non-recalc frames below 4 ms; it is only meaningful in an optimised build, so the assertion
//! runs only when `U53_PERF_GATE=1` (run it with `cargo test --release`); a debug build prints the
//! numbers.

use std::cell::RefCell;
use std::fmt::Write as _;
use std::path::PathBuf;
use std::rc::Rc;
use std::time::Instant;

use pob_app::app::CxApp;
use pob_app::build::on_frame::{self as of, BuildTab};
use pob_host::{App, PlatformParts};
use pob_sg::{Key, Sg, Sys};
use pob_ui_oracle::boot::{self, OracleHandles};
use pob_ui_oracle::runner::{self, Driver, ScenarioApp, UserDir};

thread_local! {
    /// Tree tab draw times of the current run (ms).
    static TREE_MS: RefCell<Vec<f64>> = const { RefCell::new(Vec::new()) };
}

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(pob_testpaths::pob_root(), "PoB checkout (.PathOfBuilding)")?;
    let dump = pob_testpaths::require(pob_testpaths::dump_dir(), "data/dump")?;
    assert!(pob_oracle::testkit::init_data(&dump), "data dump");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    pob_gamedata::timeless_lut::set_jewel_fs(std::sync::Arc::new(
        pob_gamedata::timeless_lut::StdJewelFs {
            script_path: src.to_string_lossy().into_owned(),
            writable: false,
        },
    ));
    Some(root)
}

/// Time the Tree arm of the shell's `draw_tab` (the default hooks otherwise).
fn install_timer() {
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        let inner = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab == BuildTab::Tree {
                // the shipping app records no trace: stop the oracle's recorder (the frames
                // before the first tree draw keep theirs, nothing reads them)
                cx.sg.trace = None;
                let t0 = Instant::now();
                inner(cx, tab, vp, ev);
                let ms = t0.elapsed().as_secs_f64() * 1000.0;
                TREE_MS.with(|t| t.borrow_mut().push(ms));
            } else {
                inner(cx, tab, vp, ev);
            }
        });
    });
}

/// `PobApp` as a scenario app (its own `load_build`, as u53_e2e's Rust side).
struct Side(pob_app::PobApp);

impl App for Side {
    fn sg(&mut self) -> &mut Sg {
        self.0.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.0.sys()
    }
    fn on_init(&mut self) {
        self.0.on_init();
    }
    fn on_frame(&mut self) {
        self.0.on_frame();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.0.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.0.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.0.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.0.on_lua_error(msg);
    }
}

impl ScenarioApp for Side {
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.0.load_build(name, xml);
        Ok(())
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(self.0.snapshot())
    }
}

fn script(build: &str) -> String {
    let mut s = format!(
        "fixture userdir=../fixtures/user1\nframes 2\nload-build \"Builds/{build}\"\nframes 6\n"
    );
    // idle, then hover a grid of points over the tree (tooltips, paths)
    s.push_str("move 1100 540\nframes 5\n");
    for i in 0..20 {
        let x = 450 + (i % 5) * 300;
        let y = 150 + (i / 5) * 220;
        let _ = write!(s, "move {x} {y}\nframes 2\n");
    }
    // zoomed out (the whole tree), then in
    s.push_str("move 1100 540\nwheel down 3\nframes 6\nwheel up 6\nframes 6\n");
    // drag
    s.push_str("key-down LEFTBUTTON\n");
    for i in 0..8 {
        let _ = write!(s, "move {} {}\nframes 1\n", 1100 - i * 25, 540 - i * 10);
    }
    s.push_str("key-up LEFTBUTTON\nframes 4\n");
    s
}

fn pct(v: &[f64], p: f64) -> f64 {
    let mut v = v.to_vec();
    v.sort_by(f64::total_cmp);
    if v.is_empty() {
        return f64::NAN;
    }
    let i = ((v.len() - 1) as f64 * p).round() as usize;
    v[i]
}

#[test]
fn u53_tree_perf_1080p() {
    let Some(root) = inputs() else { return };
    let gate = std::env::var("U53_PERF_GATE").is_ok_and(|v| v == "1");
    let base = runner::crate_dir().join("scenarios");
    let mut failures = Vec::new();
    for build in [
        "allflame_exp_001.xml",
        "allflame_exp_048.xml",
        "upstream_Dual_Wield_Cospris_CoC.xml",
        "upstream_Generals_Perforate_Zerker.xml",
    ] {
        let text = script(build);
        let parsed = runner::parse_script(&text).expect("script");
        let fx = runner::fixtures(&parsed, &base).expect("fixtures");
        let user = UserDir::new(fx.user_dir.as_deref()).expect("user dir");
        let handles = OracleHandles::default();
        let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).expect("parts");
        TREE_MS.with(|t| t.borrow_mut().clear());
        install_timer();
        let mut app = Side(pob_app::PobApp::new(PlatformParts { sg, sys }));
        app.0.cx.app_mut().launch.force_release_install = true;
        let mut d = Driver::new(runner::screen(1920, 1080, 1.0), handles, user.path.clone());
        let t0 = Instant::now();
        let (frames, r) = runner::drive(&mut app, &parsed, &mut d);
        let wall = t0.elapsed().as_secs_f64();
        r.expect("drive");
        drop(app);
        let ms = TREE_MS.with(|t| t.borrow().clone());
        // the first 3 tree draws follow the load (Build:Init's recalc lands in their frames)
        let steady: Vec<f64> = ms.iter().skip(3).copied().collect();
        let (p50, p95, max) = (pct(&steady, 0.5), pct(&steady, 0.95), pct(&steady, 1.0));
        let _ = frames;
        println!(
            "{build}: {} tree draws in {wall:.2}s (the whole run: boot, load, recalcs); steady tree draw p50 {p50:.3} ms, p95 {p95:.3} ms, max {max:.3} ms; first draws {:?}",
            ms.len(),
            ms.iter().take(3).map(|x| (x * 1000.0).round() / 1000.0).collect::<Vec<_>>()
        );
        assert!(steady.len() >= 40, "{build}: only {} tree draws", steady.len());
        if gate && !(p50 < 4.0 && p95 < 4.0) {
            failures.push(format!("{build}: tree draw p50 {p50:.3} ms, p95 {p95:.3} ms (gate < 4 ms)"));
        }
    }
    of::set_shell_hooks(|h| *h = of::default_shell_hooks());
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
