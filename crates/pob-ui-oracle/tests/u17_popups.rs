//! U17 acceptance (ui doc §12 U17 row): the Options, About and Update popups and
//! main:DrawBackground on both sides of the UI oracle. Owner: U17.
//!
//! * `u17_scenarios`: every `tests/u17/*.pobscript` (a script that sets its own `screen` runs
//!   once, the others at both acceptance screens). Zero T3 divergences: the same draw ops per
//!   `(layer, sub)` bucket and the same side effects, frame by frame, and no error prompt on
//!   either side. A script with `#! u17-settings` also compares the Settings.xml its Save button
//!   wrote (T4, attribute order normalised: PoB writes attributes in `pairs` order).
//!
//! Harness: `main.modes.LIST` is the real build list on both sides (BuildList.lua against U18's
//! port, which draws `main:DrawBackground` and the list of the fixture's builds): Options' Save
//! calls `main.modes.LIST:BuildList()` in LIST mode (Main.lua:1219, CCR-0099 B), so
//! `e_options_save`, which changes the build path, sees the list rebuilt from the new folder
//! (FOLLOWUPS "CCRDOC" 1; a no-op stand-in hid it). `main.modes.BUILD` (Build, U19-U21) is a
//! stand-in on both sides: a mode whose `OnFrame` draws `main:DrawBackground(main.viewPort)` and
//! nothing else (no scenario opens a build). Everything else is real: the popups are PoB's Lua
//! against U17's port.
//!
//! `#! u17-setup NAME` steps run on both sides right after the first frame (the popups read
//! `main.screenW/H`, which main:OnFrame sets), in file order. A scenario that clicks Options' Save
//! must turn dev mode on first (`dev`): outside dev mode Save calls main:SetManifestBranch, which
//! rewrites the PoB checkout's manifest.xml.
//!
//! Compared side effects exclude `ImageLoad` (as U16: the Lua side's frame 0 loads the images of
//! modules other units port; the tree background, U23, is loaded by U17's stand-in on first use,
//! CCR-0099 A). `FileWrite` is recorded by neither side (CCR-0028 item 3).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::main_mode::{self, Mode, ModeArg};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_core::ids::ViewPort;
use pob_ui_core::input::InputEvents;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

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

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u17")
}

/// The stand-in BUILD mode of the Lua side (see the module notes).
const LUA_STUBS: &str = r"
local function stubMode()
	local m = { }
	function m:Init(a1, a2) self.a1, self.a2 = a1, a2 end
	function m:GetArgs() return self.a1, self.a2 end
	function m:OnFrame(inputEvents, viewPort) main:DrawBackground(main.viewPort) end
	return m
end
main.modes.BUILD = stubMode()
";

/// The Rust stand-in BUILD mode: PendingMode's argument echo plus `main:DrawBackground`.
#[derive(Default)]
struct StubMode {
    args: RefCell<Vec<ModeArg>>,
    view_mode: RefCell<Option<String>>,
}

impl Mode for StubMode {
    fn init(&self, _cx: &mut Cx, args: &[ModeArg]) {
        *self.args.borrow_mut() = vec![main_mode::mode_arg(args, 1), main_mode::mode_arg(args, 2)];
    }
    fn on_frame(&self, cx: &mut Cx, _input_events: &mut InputEvents, _vp: &ViewPort) {
        let vp = cx.main.view_port;
        main_mode::background::draw_background(cx, &vp);
    }
    fn get_args(&self, _cx: &Cx) -> Option<Vec<ModeArg>> {
        Some(self.args.borrow().clone())
    }
    fn view_mode(&self, _cx: &Cx) -> Option<String> {
        self.view_mode.borrow().clone()
    }
    fn set_view_mode(&self, _cx: &mut Cx, v: &str) {
        *self.view_mode.borrow_mut() = Some(v.to_owned());
    }
}

fn rust_stubs(cx: &mut Cx) {
    let modes = &mut cx.app_mut().main.modes;
    modes.insert("BUILD".to_owned(), Rc::new(StubMode::default()));
}

/// `#! u17-setup NAME`: Lua code.
fn lua_setup(name: &str) -> &'static str {
    match name {
        "dev" => "launch.devMode = true",
        "version" => r#"launch.versionNumber = "2.60.0""#,
        "custom" => {
            r#"launch.proxyURL = "socks5h://proxy.example:1080" launch.connectionProtocol = 2
main.buildPath = "/custom/builds/" main.betaTest = true main.defaultGemQuality = 20
main.nodePowerTheme = "GREEN/BLUE" main.dpiScaleOverridePercent = 150
main.invertSliderScrollDirection = true main.disableDevAutoSave = true"#
        }
        "options" => "main:OpenOptionsPopup()",
        "about" => "main:OpenAboutPopup()",
        "about7" => "main:OpenAboutPopup(7)",
        "about99" => "main:OpenAboutPopup(99)",
        "about_party" => r#"main:OpenAboutPopup("party tab")"#,
        "about_hotkeys" => r#"main:OpenAboutPopup("hotkeys")"#,
        "update" => "main:OpenUpdatePopup()",
        other => panic!("unknown u17-setup {other}"),
    }
}

/// `#! u17-setup NAME`: the same on the Rust side.
fn rust_setup(cx: &mut Cx, name: &str) {
    use main_mode::{about_popup, options_popup, update_popup};
    match name {
        "dev" => cx.app_mut().launch.dev_mode = true,
        "version" => cx.app_mut().launch.version_number = Some("2.60.0".to_owned()),
        "custom" => {
            let a = cx.app_mut();
            a.launch.proxy_url = Some("socks5h://proxy.example:1080".to_owned());
            a.launch.connection_protocol = Some(2.0);
            a.main.build_path = Some("/custom/builds/".to_owned());
            a.main.beta_test = Some(true);
            a.main.default_gem_quality = Some(20.0);
            a.main.node_power_theme = "GREEN/BLUE".to_owned();
            a.main.dpi_scale_override_percent = 150.0;
            a.main.invert_slider_scroll_direction = Some(true);
            a.main.disable_dev_auto_save = Some(true);
        }
        "options" => options_popup::open_options_popup(cx, None),
        "about" => about_popup::open_about_popup(cx, ModeArg::Nil),
        "about7" => about_popup::open_about_popup(cx, ModeArg::Num(7.0)),
        "about99" => about_popup::open_about_popup(cx, ModeArg::Num(99.0)),
        "about_party" => about_popup::open_about_popup(cx, ModeArg::from("party tab")),
        "about_hotkeys" => about_popup::open_about_popup(cx, ModeArg::from("hotkeys")),
        "update" => update_popup::open_update_popup(cx),
        other => panic!("unknown u17-setup {other}"),
    }
}

fn setups(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#! u17-setup "))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    setups: Vec<String>,
    frames: usize,
}

impl App for LuaSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        self.app
            .with_vm(|pob| pob.eval::<()>(LUA_STUBS))
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.frames += 1;
        if self.frames == 1 {
            let mut code = String::new();
            for s in &self.setups {
                code.push_str(lua_setup(s));
                code.push('\n');
            }
            self.app
                .with_vm(|pob| pob.eval::<()>(&code))
                .unwrap_or_else(|e| panic!("Lua setups: {e}"));
        }
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for LuaSide {
    fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
        anyhow::bail!("u17 scenarios do not open builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (the oracle's Rust platform
/// does not, CCR-0028 item 2).
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::OpenUrl(url.to_owned()));
        self.inner.open_url(url);
    }
    fn set_window_title(&mut self, title: &str) {
        self.effects
            .borrow_mut()
            .push(SideEffect::WindowTitle(title.to_owned()));
        self.inner.set_window_title(title);
    }
    fn take_screenshot(&mut self) {
        self.effects.borrow_mut().push(SideEffect::Screenshot);
        self.inner.take_screenshot();
    }
    fn restart(&mut self) {
        self.inner.restart();
    }
    fn exit(&mut self, msg: Option<&str>) {
        self.effects.borrow_mut().push(SideEffect::Exit);
        self.inner.exit(msg);
    }
    fn script_path(&self) -> String {
        self.inner.script_path()
    }
    fn runtime_path(&self) -> String {
        self.inner.runtime_path()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        self.inner.user_path()
    }
    fn con_print(&mut self, s: &str) {
        self.inner.con_print(s);
    }
}

struct NullPlatform;
impl pob_sg::Platform for NullPlatform {
    fn time_ms(&self) -> i64 {
        0
    }
    fn copy(&mut self, _: &str) {}
    fn paste(&mut self) -> Option<String> {
        None
    }
    fn open_url(&mut self, _: &str) {}
    fn set_window_title(&mut self, _: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _: Option<&str>) {}
    fn script_path(&self) -> String {
        String::new()
    }
    fn runtime_path(&self) -> String {
        String::new()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(String::new())
    }
    fn con_print(&mut self, _: &str) {}
}

struct RustSide {
    app: pob_app::PobApp,
    setups: Vec<String>,
    frames: usize,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, setups: Vec<String>) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::new(parts);
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
        app.cx.app_mut().launch.force_release_install = true;
        RustSide {
            app,
            setups,
            frames: 0,
            effects,
        }
    }
    fn flush(&mut self) {
        let e: Vec<_> = self.effects.borrow_mut().drain(..).collect();
        if let Some(t) = self.app.cx.sg.trace.as_mut() {
            for e in e {
                t.effect(e);
            }
        }
    }
}

impl App for RustSide {
    fn sg(&mut self) -> &mut Sg {
        self.app.sg()
    }
    fn sys(&mut self) -> &mut Sys {
        self.app.sys()
    }
    fn on_init(&mut self) {
        self.app.on_init();
        rust_stubs(&mut self.app.cx);
        self.flush();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
        self.frames += 1;
        if self.frames == 1 {
            for s in self.setups.clone() {
                rust_setup(&mut self.app.cx, &s);
            }
        }
        self.flush();
    }
    fn on_key_down(&mut self, key: Key, double_click: bool) {
        self.app.on_key_down(key, double_click);
        self.flush();
    }
    fn on_key_up(&mut self, key: Key) {
        self.app.on_key_up(key);
        self.flush();
    }
    fn on_char(&mut self, ch: Key) {
        self.app.on_char(ch);
        self.flush();
    }
    fn on_lua_error(&mut self, msg: String) {
        self.app.on_lua_error(msg);
    }
}

impl ScenarioApp for RustSide {
    fn load_build(&mut self, _name: &str, _xml: &str) -> anyhow::Result<()> {
        anyhow::bail!("u17 scenarios do not open builds")
    }
    fn snapshot(&mut self) -> anyhow::Result<Option<String>> {
        Ok(None)
    }
}

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Sides {
    lua: Vec<FrameTrace>,
    rust: Vec<FrameTrace>,
    rust_error: Option<String>,
    /// `launch.promptMsg` at the end (an error prompt fails the run).
    lua_prompt: Option<String>,
    rust_prompt: Option<String>,
    /// Settings.xml at the end of each run.
    lua_settings: Option<String>,
    rust_settings: Option<String>,
}

/// The Lua side of one run through the Lua-side cache (TP4, as u41): PoB boot, `script` and what
/// the test keeps of the VM and the user dir afterwards (`extra`, JSON). The key covers this file
/// (stubs and Lua-side wrapper), the Lua-side code, PoB, this test's scripts and fixtures, the
/// fixture user dir, the script, the screen and the other arguments.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    setups: &[String],
) -> anyhow::Result<LuaRun> {
    let scripts = test_dir();
    let mut inputs: Vec<&Path> = vec![&scripts];
    inputs.extend(fixture);
    let key = oracle_cache::ui_key(
        "ui-u17",
        pob_ui_oracle::test_source!(),
        root,
        &inputs,
        &[
            &format!("{script:?}"),
            &format!("{screen:?}"),
            &format!("{setups:?}"),
            &user.path.to_string_lossy(),
        ],
    );
    oracle_cache::cached_lua_run(key, || {
        let settings_file = user.path.join("Path of Building/Settings.xml");
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false)?;
        let mut lua_side = LuaSide {
            app: lua_app,
            setups: setups.to_vec(),
            frames: 0,
        };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (lua, r) = runner::drive(&mut lua_side, script, &mut d);
        r.map_err(|e| anyhow::anyhow!("Lua side: {e}"))?;
        let lua_prompt = lua_side
            .app
            .with_vm(|pob| pob.eval::<Option<String>>("return launch.promptMsg"))
            .map_err(|e| anyhow::anyhow!("Lua promptMsg: {e}"))?;
        let lua_settings = std::fs::read_to_string(&settings_file).ok();
        drop(lua_side);
        Ok(LuaRun {
            frames: lua,
            error: None,
            snapshots: Vec::new(),
            extra: vec![
                serde_json::to_string(&lua_prompt)?,
                serde_json::to_string(&lua_settings)?,
            ],
        })
    })
}

/// TP4 fix round 1: the user dir of one case at a fixed path, `<tmp>/pob-ui-oracle/named/<tag>`,
/// and an exclusive lock on `<tag>.lock` that keeps another process running the same case out of
/// it while it lives (bind it first, so the dir goes before the lock). This test's Lua side draws
/// the user dir's path (the Options dialog's build path tooltip), and the text's width moves the layout, so a
/// cached Lua side matches only a Rust side run in the same directory (a per-process
/// `UserDir::new` path failed every warm run); the path goes into the key.
fn case_user_dir(fixture: Option<&Path>, tag: &str) -> anyhow::Result<(std::fs::File, UserDir)> {
    let base = std::env::temp_dir().join("pob-ui-oracle").join("named");
    std::fs::create_dir_all(&base)?;
    let lock = std::fs::OpenOptions::new()
        .create(true)
        .truncate(false)
        .write(true)
        .open(base.join(format!("{tag}.lock")))?;
    lock.lock()?;
    let mut user = UserDir::new(fixture)?;
    let _ = std::fs::remove_dir_all(&user.path);
    user.path = base.join(tag);
    user.reset()?;
    Ok((lock, user))
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    setups: &[String],
    tag: &str,
) -> anyhow::Result<Sides> {
    let (_lock, user) = case_user_dir(fixture, tag)?;
    let settings_file = user.path.join("Path of Building/Settings.xml");

    // Lua (cached, TP4)
    let run = lua_run(root, script, &user, fixture, screen, setups)?;
    let mut lua = run.frames;
    let lua_prompt = serde_json::from_str(run.extra.first().map_or("null", String::as_str))?;
    let lua_settings = serde_json::from_str(run.extra.get(1).map_or("null", String::as_str))?;

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, setups.to_vec());
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut rust, rust_error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    let rust_prompt = rust_side.app.cx.app().launch.prompt_msg.clone();
    let rust_settings = std::fs::read_to_string(&settings_file).ok();
    drop(rust_side);
    drop_image_loads(&mut lua);
    drop_image_loads(&mut rust);
    Ok(Sides {
        lua,
        rust,
        rust_error,
        lua_prompt,
        rust_prompt,
        lua_settings,
        rust_settings,
    })
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(
            || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"),
            PathBuf::from,
        )
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| {
            matches!(
                c.call,
                ApiCall::DrawImage { .. }
                    | ApiCall::DrawImageQuad { .. }
                    | ApiCall::DrawString { .. }
            )
        })
        .count()
}

/// Attribute-order normal form of an XML text (xml.lua's own reader and writer), as U16's T4.
fn normalise(xml: &str) -> String {
    use pob_app::model::{XmlChild, XmlNode};
    fn sort(n: &mut XmlNode) {
        n.attrib.sort_keys();
        for c in &mut n.children {
            if let XmlChild::Node(c) = c {
                sort(c);
            }
        }
        // `pairs(self.gameAccounts)` / `pairs(sharedItemSet.slots)` (Main.lua:763, :773): PoB's
        // string-key order, random per process.
        if &*n.elem == "Accounts" || &*n.elem == "ItemSet" {
            n.children.sort_by_key(|c| match c {
                XmlChild::Node(c) => pob_app::model::xml::ComposeXML(c),
                XmlChild::Text(t) => t.to_string(),
            });
        }
    }
    let mut root = pob_app::model::xml::ParseXML_root(xml).expect("parse");
    sort(&mut root);
    let XmlChild::Node(top) = &root.children[0] else {
        panic!("no root element")
    };
    pob_app::model::xml::ComposeXML(top)
}

/// One case's outcome (TP4: cases run on `oracle_cache::par_map`; collected in case order).
#[derive(Default)]
struct CaseOut {
    lines: Vec<String>,
    failures: Vec<String>,
    ran: usize,
    total_ops: usize,
}

#[test]
fn u17_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u17")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u17/*.pobscript");
    let filter = std::env::var("U17_FILTER").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
        let own_screen = script.steps.first().and_then(|s| match s {
            pob_host::script::Step::Screen { w, h, dpi } => Some((*w, *h, *dpi)),
            _ => None,
        });
        let screens: Vec<(i32, i32, f32)> = match own_screen {
            Some(s) => vec![s],
            None => runner::ACCEPTANCE_SCREENS.to_vec(),
        };
        for (w, h, s) in screens {
            cases.push((path.clone(), stem.clone(), (w, h, s)));
        }
    }
    let results = oracle_cache::par_map(cases, |(path, stem, (w, h, s))| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let setups = setups(&text);
        let compare_settings = text.lines().any(|l| l.trim() == "#! u17-settings");
        let clicks_save = text.contains("Options Save");
        assert!(
            !clicks_save || setups.iter().any(|s| s == "dev"),
            "{stem}: Save outside dev mode would rewrite the checkout's manifest.xml"
        );
        {
            let tag = format!("u17_{stem}@{w}x{h}@{s}");
            let sides = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &setups,
                &tag,
            ) {
                Ok(s) => s,
                Err(e) => {
                    out.failures.push(format!("{tag}: {e:#}"));
                    return out;
                }
            };
            out.ran += 1;
            let run = RunResult {
                lua: sides.lua,
                rust: sides.rust,
                rust_error: sides.rust_error.clone(),
                lua_snapshots: Vec::new(),
                rust_snapshots: Vec::new(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            out.total_ops += ops;
            let settings = if compare_settings {
                match (&sides.lua_settings, &sides.rust_settings) {
                    (Some(l), Some(r)) => {
                        let same = normalise(l) == normalise(r);
                        out.lines.push(format!(
                            "{tag}: Settings.xml written by the Rust side:\n{r}"
                        ));
                        if !same {
                            out.failures
                                .push(format!("{tag}: Settings.xml\n--- lua\n{l}\n--- rust\n{r}"));
                        }
                        format!(", Settings.xml Rust == PoB (normalised): {same}")
                    }
                    other => {
                        out.failures
                            .push(format!("{tag}: Settings.xml missing: {other:?}"));
                        ", Settings.xml missing".to_owned()
                    }
                }
            } else {
                String::new()
            };
            out.lines.push(format!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences{settings}{}",
                run.lua.len(),
                sides
                    .rust_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}"))
            ));
            if sides.lua_prompt.is_some() || sides.rust_prompt.is_some() {
                out.failures.push(format!(
                    "{tag}: error prompt: lua {:?} rust {:?}",
                    sides.lua_prompt, sides.rust_prompt
                ));
            }
            if n != 0 || sides.rust_error.is_some() || run.lua.len() != run.rust.len() || ops == 0 {
                out.failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
        out
    });
    let mut failures = Vec::new();
    let mut ran = 0;
    let mut total_ops = 0;
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        ran += case.ran;
        total_ops += case.total_ops;
    }
    println!("u17: {ran} runs, {total_ops} Lua draw ops compared");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
