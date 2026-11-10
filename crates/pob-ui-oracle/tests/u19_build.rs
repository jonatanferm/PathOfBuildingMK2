//! U19 acceptance (ui doc §12 U19 row): `buildMode:Init` (Build.lua:1-770) on both sides of the
//! UI oracle. Owner: U19.
//!
//! `u19_scenarios`: every `tests/u19/*.pobscript` (a new build, the fixture builds) with the top
//! bar and the side bar at three widths (1024x768, 1280x800 and 1920x1080 at DPI 1.0) and at
//! 1920x1080 at DPI 1.5 (a script that sets its own `screen` runs once). Zero T3 divergences:
//! the same draw ops per `(layer, sub)` bucket and the same side effects, frame by frame.
//!
//! Stand-ins (both sides run the same ones). U19 ports Init; the methods Init calls that U20/U21
//! port, the tabs, and the build list are not ported yet:
//! * `main.modes.LIST` draws nothing (as in u16_main.rs; pob-app's `PendingMode`);
//! * `buildMode:OnFrame` (U20) only runs `ProcessControlsInput`, `SetDrawLayer(5)` and
//!   `DrawControls` (Build.lua:1253, 1323, 1348), after setting `controls.pointDisplay.str/req`
//!   to fixed texts (EstimatePlayerProgress, U20); pob-app's `init::pending_on_frame`;
//! * `UpdateClassDropdowns` (U20) only records its argument as a window title (`UCD <treeVersion>`),
//!   so the order of its calls among the window titles is compared (the fixture builds use the
//!   3_13 and 3_29 trees, the new build the latest); `SyncLoadouts` (U20) and `RefreshStatList`
//!   (U21) do nothing (the class, loadout and skill dropdowns stay empty; the stat box has no
//!   lines);
//! * the tabs are PoB's own on the Lua side (Init constructs and loads them; nothing draws them
//!   while OnFrame is the stand-in) and pending placeholders on the Rust side;
//! * `main:OpenAboutPopup` / `OpenOptionsPopup` open a message popup naming their argument.
//!
//! pob-app does not register its BUILD mode yet (CCR-0098 §5): the Rust side installs
//! `pob_app::build::mode()` itself after `OnInit`.
//!
//! Compared side effects exclude `ImageLoad` (the Lua tabs' constructors load the tree's
//! images; U23 ports them).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init::{self, InfoControl};
use pob_app::main_mode::ModeArg;
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::ViewPort;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// The three widths of the acceptance row (1024, 1280, 1920), and 1920x1080 at DPI 1.5.
const SCREENS: [(i32, i32, f32); 4] = [
    (1024, 768, 1.0),
    (1280, 800, 1.0),
    (1920, 1080, 1.0),
    (1920, 1080, 1.5),
];

/// The fixed texts of the EstimatePlayerProgress stand-in.
const POINT_STR: &str = "^7 12 / 123   ^7 0 / 8";
const POINT_REQ: &str = "Required Level: 90\nEstimated Progress:\nActs: 10";

fn inputs() -> Option<PathBuf> {
    let root = pob_testpaths::require(
        pob_testpaths::pob_root(),
        "PoB checkout (.PathOfBuilding)",
    )?;
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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u19")
}

/// The Lua stand-ins (see the module notes).
fn lua_stubs() -> String {
    format!(
        r#"
local function stubMode()
	local m = {{ }}
	function m:Init(a1, a2) self.a1, self.a2 = a1, a2 end
	function m:GetArgs() return self.a1, self.a2 end
	function m:OnFrame(inputEvents, viewPort) end
	return m
end
main.modes.LIST = stubMode()
local b = main.modes.BUILD
function b:UpdateClassDropdowns(treeVersion) SetWindowTitle("UCD " .. tostring(treeVersion)) end
function b:SyncLoadouts() end
function b:RefreshStatList() end
function b:OnFrame(inputEvents)
	self.controls.pointDisplay.str = {str:?}
	self.controls.pointDisplay.req = {req:?}
	self:ProcessControlsInput(inputEvents, main.viewPort)
	SetDrawLayer(5)
	self:DrawControls(main.viewPort)
end
function main:OpenAboutPopup(helpSectionName) self:OpenMessagePopup("About", tostring(helpSectionName)) end
function main:OpenOptionsPopup() self:OpenMessagePopup("Options", "stub") end
"#,
        str = POINT_STR,
        req = POINT_REQ,
    )
}

fn rust_on_frame(cx: &mut Cx, ev: &mut InputEvents, vp: &ViewPort) {
    if let Some(node) = cx.app().build.as_ref().map(|b| b.node.id()) {
        if let Some(pd) = cx.child(node, "pointDisplay") {
            let w = cx.w_mut::<InfoControl>(pd);
            w.str_ = Some(POINT_STR.to_owned());
            w.req = Some(POINT_REQ.to_owned());
        }
    }
    init::pending_on_frame(cx, ev, vp);
}

fn rust_stubs(cx: &mut Cx) {
    let h = &mut cx.app_mut().main.hooks;
    h.open_about_popup = Rc::new(|cx, s: ModeArg| {
        pob_ui_core::popups::open_message_popup(cx, "About", &s.tostring());
    });
    h.open_options_popup = Rc::new(|cx| {
        pob_ui_core::popups::open_message_popup(cx, "Options", "stub");
    });
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.on_frame = Rc::new(rust_on_frame);
        // U20's SyncLoadouts is the default now; the Lua side stubs it (lua_stubs)
        h.sync_loadouts = Rc::new(|_| {});
        h.update_class_dropdowns = Rc::new(|cx, tv| {
            let tv = tv.as_deref().unwrap_or("nil");
            cx.sys.platform.set_window_title(&format!("UCD {tv}"));
        });
    });
}

/// `#! u19-setup NAME` steps run right after `OnInit` on both sides.
fn lua_setup(name: &str) -> &'static str {
    match name {
        "newbuild" => r#"main:SetMode("BUILD", false, "Unnamed build")"#,
        other => panic!("unknown u19-setup {other}"),
    }
}

fn rust_setup(cx: &mut Cx, name: &str) {
    match name {
        "newbuild" => pob_app::main_mode::on_frame::set_mode(
            cx,
            "BUILD",
            vec![ModeArg::Bool(false), ModeArg::Str("Unnamed build".to_owned())],
        ),
        other => panic!("unknown u19-setup {other}"),
    }
}

fn setups(text: &str) -> Vec<String> {
    text.lines()
        .filter_map(|l| l.trim().strip_prefix("#! u19-setup "))
        .map(|s| s.trim().to_owned())
        .collect()
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    setups: Vec<String>,
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
        let mut code = lua_stubs();
        for s in &self.setups {
            code.push_str(lua_setup(s));
            code.push('\n');
        }
        self.app
            .with_vm(|pob| pob.eval::<()>(&code))
            .unwrap_or_else(|e| panic!("Lua stubs: {e}"));
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
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
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml)
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
/// does not, CCR-0028 item 2); as in u16_main.rs.
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
        rust_stubs(&mut self.app.cx);
        self.app.on_init();
        // CCR-0098 §5: main.modes.BUILD is Build.lua's object (main:Init installed the
        // placeholder)
        self.app
            .cx
            .app_mut()
            .main
            .modes
            .insert("BUILD".to_owned(), pob_app::build::mode());
        // CCR-0100 C: main.modes.LIST is U18's real BuildList now; these scenarios stub it on
        // both sides (lua_stubs), so the Rust side gets the PendingMode back before any frame.
        self.app.cx.app_mut().main.modes.insert(
            "LIST".to_owned(),
            Rc::new(pob_app::main_mode::PendingMode::new("LIST")),
        );
        for s in self.setups.clone() {
            rust_setup(&mut self.app.cx, &s);
        }
        self.flush();
    }
    fn on_frame(&mut self) {
        self.app.on_frame();
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
    fn load_build(&mut self, name: &str, xml: &str) -> anyhow::Result<()> {
        self.app.load_build(name, xml);
        Ok(())
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
    lua_error: Option<String>,
    rust_error: Option<String>,
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    setups: &[String],
) -> anyhow::Result<Sides> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide {
        app: lua_app,
        setups: setups.to_vec(),
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut lua, r) = runner::drive(&mut lua_side, script, &mut d);
    let lua_error = r.err().map(|e| e.to_string());
    drop(lua_side);

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
    drop(rust_side);
    drop_image_loads(&mut lua);
    drop_image_loads(&mut rust);
    Ok(Sides {
        lua,
        rust,
        lua_error,
        rust_error,
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
                ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }
            )
        })
        .count()
}

fn window_titles(frames: &[FrameTrace]) -> Vec<String> {
    frames
        .iter()
        .flat_map(|f| &f.effects)
        .filter_map(|e| match e {
            SideEffect::WindowTitle(t) => Some(t.clone()),
            _ => None,
        })
        .collect()
}

#[test]
fn u19_scenarios() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u19")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u19/*.pobscript");
    let filter = std::env::var("U19_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let own_screen = matches!(
            script.steps.first(),
            Some(pob_host::script::Step::Screen { .. })
        );
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            let tag = if own_screen {
                format!("u19_{stem}")
            } else {
                format!("u19_{stem}@{w}x{h}@{s}")
            };
            let sides = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &setups(&text),
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
            let titles = (window_titles(&sides.lua), window_titles(&sides.rust));
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
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, titles {:?}{}{}",
                run.lua.len(),
                titles.0,
                sides
                    .lua_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                sides
                    .rust_error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}"))
            );
            if n != 0
                || sides.rust_error.is_some()
                || sides.lua_error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, titles lua {:?} rust {:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    titles.0,
                    titles.1,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
    }
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
