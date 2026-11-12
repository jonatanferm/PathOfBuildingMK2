//! U29 acceptance (ui doc §12 U29 row: filter and sort modes; DPS sort order identical through
//! the bridge, the model mutated temporarily; gem tooltips). Owner: U29.
//!
//! * `u29_data_invariants`: the data facts the port's ORDER notes rely on (gem names unique, at
//!   most one reservation key per skill level, no empty `flavourText` list).
//! * `u29_dps_sort`: for fixture builds, a GemSelectControl on the main socket group's empty
//!   slot (`index = #gemList + 1`) is focused (OnFocusGained → UpdateSortCache → BuildList) and
//!   drawn once, which runs the DPS builder to its end; the Rust side computes every gem's DPS
//!   through `LuaCalcBridge` (each `calcFunc` call serialises the temporarily mutated model). The
//!   sorted list, `canSupport`, `dps` and `dpsColor` of every listed gem equal PoB's.
//!   `U29_DPS_TYPES` (default `EXCEPTIONAL`, which keeps the run to a few dozen bridge calls)
//!   sets `showSupportGemTypes`; `U29_DPS_FIXTURES` selects fixtures by substring.
//! * `u29_gem_tooltips`: `GemTooltip.AddGemTooltip` for every gem of `data.gems` at level 1 /
//!   quality 0 and level 20 / quality 20, and for every gem instance of the build's socket groups
//!   (their `displayEffect`s): every tooltip line (size, text, font) equals PoB's.
//! * `u29_scenarios`: every `tests/u29/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5, zero
//!   T3 divergences. Three GemSelectControls (the main group's gems 1 and 2, and its empty slot)
//!   sit in a stand-in SkillsTab host; the stand-in `calcFunc` (both sides) returns the base
//!   output with `CombinedDPS` moved by the hovered gem's name length, so the scenarios exercise
//!   the model mutation, the sort, the colours and the tooltips without the cost of a calc per
//!   gem (the DPS numbers themselves are `u29_dps_sort`'s).
//!
//! Stand-ins (both sides): `main.modes.LIST` draws nothing; `UpdateClassDropdowns` records a
//! window title, `SyncLoadouts` does nothing, `EstimatePlayerProgress` sets fixed texts (as
//! u21); `buildMode:OnFrame` runs only the stand-in host's `ProcessControlsInput` and
//! `DrawControls`; the gem change function records its arguments as a window title.
//! The Lua stand-in SkillsTab host is `setmetatable({ controls = { } }, { __index =
//! build.skillsTab })` (its own controls and selControl, the tab's model fields and methods); the
//! Rust one is a host node of class "SkillsTab" (the trace markers name the host class).

#![allow(
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::case_sensitive_file_extension_comparisons
)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::Poll;

use pob_app::app::CxApp;
use pob_app::build::init::{self, InfoControl};
use pob_app::build::stats;
use pob_app::calc::{CalcFacade, MiscCalc};
use pob_app::controls::gem_select::GemSelectControl;
use pob_app::controls::gem_tooltip::{self, TooltipGemData, TooltipGemInstance};
use pob_app::main_mode::ModeArg;
use pob_calc::lua_table::Output;
use pob_calc::model::BuildRef;
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

// TP2: the fixtures and scripts of a test run as independent cases on
// `oracle_cache::par_map` (at most `POB_TEST_PAR` LuaJIT PoBs / Rust apps at once in the whole
// process, which replaces the old one-PoB-at-a-time test lock), each on a fresh thread (the Rust
// side's thread-local state stays per case), and the Lua
// side of each case goes through the Lua-side cache (`lua_run`). Results are collected in case
// order, so output and failures do not depend on the schedule.

/// ui doc §11.3: 1280x800 @1.0 and 1920x1080 @1.5.
const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

const POINT_STR: &str = "^7 12 / 123   ^7 0 / 8";
const POINT_REQ: &str = "Required Level: 90\nEstimated Progress:\nActs: 10";

/// The stand-in controls: (key, y, which gem slot: Some(i) = `gemList[i]`, None = the empty slot).
const CONTROLS: [(&str, f64, Option<f64>); 3] = [
    ("gem1", 60.0, Some(1.0)),
    ("gem2", 90.0, Some(2.0)),
    ("gem3", 120.0, None),
];

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

fn crate_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR"))
}

fn fixture_dir() -> PathBuf {
    crate_dir().join("fixtures/user1")
}

fn fixture_builds() -> Vec<String> {
    let dir = fixture_dir().join("Path of Building/Builds");
    let mut v: Vec<String> = std::fs::read_dir(&dir)
        .unwrap_or_else(|e| panic!("{}: {e}", dir.display()))
        .filter_map(Result::ok)
        .map(|e| e.file_name().to_string_lossy().into_owned())
        .filter(|n| n.ends_with(".xml"))
        .collect();
    v.sort();
    assert_eq!(v.len(), 6, "the 6 fixture builds");
    v
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| crate_dir().join("../../target"), PathBuf::from)
        .join("ui-parity")
}

// ---- configuration shared by both sides ----------------------------------------------------------

#[derive(Clone, Debug)]
struct Mode {
    /// the stand-in calcFunc (scenarios) or the bridge's (dps sort)
    fake_calc: bool,
    /// select gem3 on the first frame (dps sort)
    focus_new: bool,
    /// `skillsTab.showSupportGemTypes`, when set
    show_types: Option<String>,
}

// ---- the Lua side ------------------------------------------------------------------------------

fn lua_stubs(mode: &Mode) -> String {
    let controls: Vec<String> = CONTROLS
        .iter()
        .map(|(k, y, i)| {
            format!(
                "{{ {k:?}, {y}, {} }}",
                i.map_or("nil".to_owned(), pob_lua::tostring_num)
            )
        })
        .collect();
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
function b:EstimatePlayerProgress()
	self.controls.pointDisplay.str = {str:?}
	self.controls.pointDisplay.req = {req:?}
end
local U29 = {{ fake = {fake}, focusNew = {focus}, showTypes = {show} }}
local function fakeCalc(override, useFullDPS)
	local build = main.modes.BUILD
	local base = build.calcsTab.miscCalculator[2]
	local out = copyTable(base, true)
	local group = build.skillsTab.displayGroup
	local gem = group.gemList[#group.gemList]
	local name = gem and gem.gemData and gem.gemData.name or ""
	out.CombinedDPS = (base.CombinedDPS or 0) + (#name - 15) * 100
	return out
end
function b:OnFrame(inputEvents)
	if not __u29 then
		local st = setmetatable({{ controls = {{ }} }}, {{ __index = self.skillsTab }})
		local group = self.skillsTab.socketGroupList[self.mainSocketGroup]
		self.skillsTab.displayGroup = group
		if U29.showTypes then self.skillsTab.showSupportGemTypes = U29.showTypes end
		if U29.fake then
			local calcsTab = self.calcsTab
			function calcsTab:GetMiscCalculator() return fakeCalc, self.miscCalculator[2] end
		end
		__u29 = {{ st = st }}
		for _, c in ipairs({{ {controls} }}) do
			local index = c[3] or (#group.gemList + 1)
			local ctl = new("GemSelectControl"):GemSelectControl({{ "TOPLEFT", nil, "TOPLEFT" }}, {{ 200, c[2], 300, 20 }}, st, index, function(gemId, addUndo, focusLost, bufMatchesGem)
				SetWindowTitle("gem " .. c[1] .. " " .. tostring(gemId) .. " " .. tostring(not not addUndo) .. " " .. tostring(not not focusLost) .. " " .. tostring(not not bufMatchesGem))
			end)
			st.controls[c[1]] = ctl
			__u29[c[1]] = ctl
		end
		if U29.focusNew then st:SelectControl(__u29.gem3) end
	end
	local st = __u29.st
	st:ProcessControlsInput(inputEvents, main.viewPort)
	st:DrawControls(main.viewPort)
end
function main:OpenAboutPopup(helpSectionName) self:OpenMessagePopup("About", tostring(helpSectionName)) end
function main:OpenOptionsPopup() self:OpenMessagePopup("Options", "stub") end
"#,
        str = POINT_STR,
        req = POINT_REQ,
        fake = mode.fake_calc,
        focus = mode.focus_new,
        show = mode
            .show_types
            .as_ref()
            .map_or("nil".to_owned(), |s| format!("{s:?}")),
        controls = controls.join(", "),
    )
}

struct LuaSide {
    app: LuaApp,
    mode: Mode,
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
        let code = lua_stubs(&self.mode);
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

// ---- the Rust side -----------------------------------------------------------------------------

/// The stand-in SkillsTab host (class name "SkillsTab", as the Lua proxy's `_className`).
struct SkillsTabStandIn;
impl Widget for SkillsTabStandIn {
    const NAME: &'static str = "SkillsTab";
    const CAPS: Caps = Caps::empty();
}

#[derive(Default)]
struct RustState {
    st: Option<CtrlId>,
    gems: Vec<(String, CtrlId)>,
}

thread_local! {
    static MODE: RefCell<Option<Mode>> = const { RefCell::new(None) };
    static STATE: RefCell<RustState> = RefCell::new(RustState::default());
}

/// The stand-in `calcFunc` (the Lua `fakeCalc`).
struct FakeMisc {
    build: BuildRef,
    base: Output,
}

impl MiscCalc for FakeMisc {
    fn call(&self, _ov: Option<Rc<pob_calc::env::Override>>, _full: Option<bool>) -> Poll<Output> {
        let out = self.base.copyTable(true);
        let group = self
            .build
            .skillsTab
            .displayGroup
            .get()
            .expect("displayGroup");
        let name = group
            .gemList
            .get()
            .snapshot()
            .last()
            .and_then(|g| g.gemData.get())
            .map_or(0, |g| g.name.as_str().len());
        out.set(
            pob_lua::n!("CombinedDPS"),
            self.base.num_or(pob_lua::n!("CombinedDPS"), 0.0) + (name as f64 - 15.0) * 100.0,
        );
        Poll::Ready(out)
    }
}

/// The calc facade with the stand-in `calcFunc` (`build_output` stays the inner one's).
struct FakeCalc {
    inner: Box<dyn CalcFacade>,
}

impl CalcFacade for FakeCalc {
    fn build_output(&mut self, build: &BuildRef) -> Poll<()> {
        self.inner.build_output(build)
    }
    fn misc_calculator(&mut self, build: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        let Poll::Ready((_, base)) = self.inner.misc_calculator(build) else {
            unreachable!("desktop")
        };
        Poll::Ready((
            Rc::new(FakeMisc {
                build: build.clone(),
                base: base.clone(),
            }),
            base,
        ))
    }
}

struct NoCalc;
impl CalcFacade for NoCalc {
    fn build_output(&mut self, _: &BuildRef) -> Poll<()> {
        unreachable!()
    }
    fn misc_calculator(&mut self, _: &BuildRef) -> Poll<(Rc<dyn MiscCalc>, Output)> {
        unreachable!()
    }
}

fn rust_setup(cx: &mut Cx, node: CtrlId, mode: &Mode) {
    let model = cx.build().model.clone();
    let st = cx.add_free(
        Some(node),
        New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), SkillsTabStandIn).host(),
    );
    let group = model
        .skillsTab
        .socketGroupList
        .get()
        .get(model.mainSocketGroup.get() as usize)
        .expect("main socket group");
    model.skillsTab.displayGroup.set(Some(group.clone()));
    if let Some(t) = &mode.show_types {
        model
            .skillsTab
            .ui_state()
            .set(pob_lua::n!("showSupportGemTypes"), t.as_str());
    }
    if mode.fake_calc {
        let inner = std::mem::replace(&mut cx.build_mut().calc, Box::new(NoCalc));
        cx.build_mut().calc = Box::new(FakeCalc { inner });
    }
    let mut gems = Vec::new();
    for (key, y, index) in CONTROLS {
        let index = index.unwrap_or((group.gemList.get().len() + 1) as f64);
        let k = key.to_owned();
        let change = Rc::new(
            move |cx: &mut Cx,
                  gem_id: Option<String>,
                  add_undo: bool,
                  focus_lost: bool,
                  buf_matches: bool| {
                let t = format!(
                    "gem {k} {} {add_undo} {focus_lost} {buf_matches}",
                    gem_id.as_deref().unwrap_or("nil")
                );
                cx.sys.platform.set_window_title(&t);
            },
        );
        let c = cx.add(
            st.id(),
            key,
            GemSelectControl::new(
                Anchor::NONE,
                rect(200, y, 300, 20),
                st.id(),
                index,
                Some(change),
                false,
                false,
            ),
        );
        gems.push((key.to_owned(), c.id()));
    }
    if mode.focus_new {
        let g3 = gems[2].1;
        cx.select_control(st.id(), Some(g3));
    }
    STATE.with(|s| {
        *s.borrow_mut() = RustState {
            st: Some(st.id()),
            gems,
        }
    });
}

fn rust_on_frame(cx: &mut Cx, ev: &mut InputEvents, _vp: &ViewPort) {
    let Some(node) = cx.app().build.as_ref().map(|b| b.node.id()) else {
        return;
    };
    init::flush_window_titles(cx);
    if cx.child(node, "pointDisplay").is_none() {
        return;
    }
    if STATE.with(|s| s.borrow().st.is_none()) {
        let mode = MODE.with(|m| m.borrow().clone()).expect("mode");
        rust_setup(cx, node, &mode);
    }
    let st = STATE.with(|s| s.borrow().st).expect("set up");
    let vp = cx.main.view_port;
    cx.process_controls_input(st, ev, &vp);
    cx.draw_controls(st, &vp, None);
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
        h.sync_loadouts = Rc::new(|_| {});
        h.update_class_dropdowns = Rc::new(|cx, tv| {
            let tv = tv.as_deref().unwrap_or("nil");
            cx.sys.platform.set_window_title(&format!("UCD {tv}"));
        });
    });
    stats::set_estimate_player_progress(Some(Rc::new(|cx: &mut Cx| {
        let node = cx.build().node.id();
        if let Some(pd) = cx.child(node, "pointDisplay") {
            let w = cx.w_mut::<InfoControl>(pd);
            w.str_ = Some(POINT_STR.to_owned());
            w.req = Some(POINT_REQ.to_owned());
        }
    })));
}

/// Records the platform side effects the Lua side's bridge records (as in u21_stats.rs).
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
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf, mode: &Mode) -> RustSide {
        MODE.with(|m| *m.borrow_mut() = Some(mode.clone()));
        STATE.with(|s| *s.borrow_mut() = RustState::default());
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || -> Box<dyn CalcFacade> {
                Box::new(LuaCalcBridge::new(Some(src.clone())))
            }),
        );
        effects.borrow_mut().clear();
        app.cx.app_mut().launch.force_release_install = true;
        RustSide { app, effects }
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
        let modes = &mut self.app.cx.app_mut().main.modes;
        modes.insert("BUILD".to_owned(), pob_app::build::mode());
        modes.insert(
            "LIST".to_owned(),
            Rc::new(pob_app::main_mode::PendingMode::new("LIST")),
        );
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

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
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

/// Both sides driven by one script; the Lua side's VM is dropped before the Rust side runs
/// (`keep` keeps both alive for a post-run comparison, Lua first).
fn run_lua(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    screen: pob_sg::ScreenInfo,
    mode: &Mode,
) -> (LuaSide, Vec<FrameTrace>, Option<String>) {
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false).expect("Lua side");
    let mut lua_side = LuaSide {
        app: lua_app,
        mode: mode.clone(),
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (frames, r) = runner::drive(&mut lua_side, script, &mut d);
    (lua_side, frames, r.err().map(|e| e.to_string()))
}

/// The Lua side of one case through the Lua-side cache (TP2): `run_lua`, then `query` on the VM
/// (its result is `extra`); `frames` are kept only when `keep_frames` (without image loads, as
/// the comparisons use them). The key covers this file (stubs and Lua-side wrapper), the
/// Lua-side code, PoB, the fixture user dir, the test's scripts, the script, the screen, the mode
/// and the query.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    screen: pob_sg::ScreenInfo,
    mode: &Mode,
    query: Option<&str>,
    keep_frames: bool,
) -> LuaRun {
    let fixture = fixture_dir();
    let scripts = crate_dir().join("tests/u29");
    let key = oracle_cache::ui_key(
        "ui-u29",
        file!(),
        root,
        &[&fixture, &scripts],
        &[
            &format!("{script:?}"),
            &format!("{screen:?}"),
            &format!("{mode:?}"),
            query.unwrap_or(""),
            if keep_frames { "frames" } else { "" },
        ],
    );
    oracle_cache::cached_lua_run(key, || {
        let (mut lua_side, mut frames, error) = run_lua(root, script, user, screen, mode);
        let mut extra = Vec::new();
        if let (Some(q), None) = (query, &error) {
            extra = lua_side
                .app
                .with_vm(|pob| pob.eval::<Vec<String>>(q))
                .map_err(|e| anyhow::anyhow!("Lua query: {e}"))?;
        }
        drop(lua_side);
        if keep_frames {
            drop_image_loads(&mut frames);
        } else {
            frames.clear();
        }
        Ok(LuaRun {
            frames,
            error,
            snapshots: Vec::new(),
            extra,
        })
    })
    .unwrap_or_else(|e| panic!("Lua side: {e:#}"))
}

fn run_rust(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    screen: pob_sg::ScreenInfo,
    mode: &Mode,
) -> (RustSide, Vec<FrameTrace>, Option<String>) {
    user.reset().expect("user dir");
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles).expect("oracle parts");
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, root.join("src"), mode);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (frames, err) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    (rust_side, frames, err)
}

// Each case runs on a fresh `par_map` thread (256 MB stack): the Rust side's process-global PoB
// state lives in thread-locals (`skills_tab::levelCostCleared`, the stat-map memo, the stat
// describer's scopes), and the Lua side is a fresh VM per run (or a cache read), so each run
// gets fresh state. (Before TP2 a separate `in_thread` per Rust run did this.)

// ---- u29_data_invariants -----------------------------------------------------------------------

#[test]
fn u29_data_invariants() {
    let Some(_root) = inputs() else { return };
    let data = pob_gamedata::data();
    // gem names are unique (SortGemList's comparator is a total order)
    let mut names = std::collections::HashSet::new();
    for g in data.gems.values() {
        assert!(names.insert(g.name), "duplicate gem name {}", g.name);
    }
    // at most one of GemTooltip's reservationMap keys per skill level (pairs order is moot)
    let keys = [
        "manaReservationFlat",
        "manaReservationPercent",
        "lifeReservationFlat",
        "lifeReservationPercent",
    ];
    let mut levels = 0;
    for ge in data.skills.values() {
        for l in ge.levels.0.iter().flatten() {
            levels += 1;
            let n = keys
                .iter()
                .filter(|k| l.num(pob_lua::Name::from(k)).is_some())
                .count();
            assert!(n <= 1, "{}: {n} reservation keys on one level", ge.id);
        }
    }
    println!(
        "u29_data_invariants: {} gems, {} granted effects, {levels} levels",
        data.gems.len(),
        data.skills.len()
    );
}

// ---- u29_dps_sort ------------------------------------------------------------------------------

/// The sorted list and sort cache of gem3, read from the Lua VM.
const LUA_DPS: &str = r#"
local ctl = __u29.gem3
local sc = ctl.sortCache
local res = { "dpsBuilder=" .. tostring(ctl.dpsBuilder ~= nil) .. " pending=" .. tostring(sc.pendingGems ~= nil) .. " base=" .. tostring(sc.baseDPS) }
for i, id in ipairs(ctl.list) do
	table.insert(res, i .. " " .. id .. " " .. tostring(sc.canSupport[id] or false) .. " " .. tostring(sc.dps[id]) .. " " .. tostring(sc.dpsColor[id]))
end
return res
"#;

fn rust_dps(rust: &mut RustSide) -> Vec<String> {
    let cx = &mut rust.app.cx;
    let g3 = STATE.with(|s| s.borrow().gems[2].1);
    let w = cx.w::<GemSelectControl>(g3);
    let sc = w.sort_cache.clone().expect("sortCache");
    let sc = sc.borrow();
    let mut res = vec![format!(
        "dpsBuilder={} pending={} base={}",
        w.dps_builder.is_some(),
        sc.pending_gems.is_some(),
        pob_lua::tostring_num(sc.base_dps)
    )];
    for (i, id) in w.list.iter().enumerate() {
        res.push(format!(
            "{} {id} {} {} {}",
            i + 1,
            sc.can_support.contains(id),
            sc.dps
                .get(id)
                .map_or("nil".to_owned(), |d| pob_lua::tostring_num(*d)),
            sc.dps_color.get(id).map_or("nil", String::as_str)
        ));
    }
    res
}

#[test]
fn u29_dps_sort() {
    let Some(root) = inputs() else { return };
    let types = std::env::var("U29_DPS_TYPES").unwrap_or_else(|_| "EXCEPTIONAL".to_owned());
    let filter = std::env::var("U29_DPS_FIXTURES").ok();
    let mode = Mode {
        fake_calc: false,
        focus_new: true,
        show_types: Some(types.clone()),
    };
    let names: Vec<String> = fixture_builds()
        .into_iter()
        .filter(|name| !filter.as_ref().is_some_and(|f| !name.contains(f.as_str())))
        .collect();
    let ran = names.len();
    let results = oracle_cache::par_map(names, |name| {
        let script = runner::parse_script(&format!(
            "frames 1\nload-build \"Builds/{name}\"\nframes 2\n"
        ))
        .unwrap();
        let user = UserDir::new(Some(&fixture_dir())).unwrap();
        let screen = runner::screen(1280, 800, 1.0);
        let t0 = std::time::Instant::now();
        let run = lua_run(&root, &script, &user, screen, &mode, Some(LUA_DPS), false);
        assert!(run.error.is_none(), "{name}: Lua: {:?}", run.error);
        let lua = run.extra;
        let t1 = std::time::Instant::now();
        // the Rust side on this case thread (fresh per case: its thread-local state is new)
        let rust = {
            let (mut rust_side, _, e) = run_rust(&root, &script, &user, screen, &mode);
            assert!(e.is_none(), "{name}: Rust: {e:?}");
            rust_dps(&mut rust_side)
        };
        let supported = lua.iter().filter(|l| l.contains(" true ")).count();
        let colors = ["^x228866", "^xFF4422", "^xFFFF66"]
            .map(|c| lua.iter().filter(|l| l.ends_with(c)).count());
        let line = format!(
            "{name} ({types}): {} gems listed, {supported} canSupport, dps up/down/same {colors:?}; Lua {:.1}s, Rust (bridge) {:.1}s",
            lua.len() - 1,
            (t1 - t0).as_secs_f64(),
            t1.elapsed().as_secs_f64()
        );
        let mut failure = None;
        if lua != rust {
            let mut diff = Vec::new();
            for i in 0..lua.len().max(rust.len()) {
                let (l, r) = (lua.get(i), rust.get(i));
                if l != r {
                    diff.push(format!("  {i}: lua {l:?}\n     rust {r:?}"));
                }
            }
            failure = Some(format!(
                "{name}: {} differing rows\n{}",
                diff.len(),
                diff.join("\n")
            ));
        }
        (line, failure)
    });
    let mut failures = Vec::new();
    for (line, failure) in results {
        println!("{line}");
        failures.extend(failure);
    }
    println!("{}", oracle_cache::summary());
    assert!(ran > 0, "no fixture selected");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u29_gem_tooltips --------------------------------------------------------------------------

const LUA_TOOLTIPS: &str = r#"
local gemTooltip = require("Classes.GemTooltip")
local b = main.modes.BUILD
local function ser(tt, head)
	local lines = { head }
	for _, l in ipairs(tt.lines) do
		table.insert(lines, tostring(l.size) .. "|" .. tostring(l.text) .. "|" .. tostring(l.font))
	end
	return table.concat(lines, "\n")
end
local res = { }
local ids = { }
for id in pairs(data.gems) do table.insert(ids, id) end
table.sort(ids)
for _, id in ipairs(ids) do
	for _, lq in ipairs({ { 1, 0 }, { 20, 20 } }) do
		local tt = new("Tooltip"):Tooltip()
		local ok, err = pcall(gemTooltip.AddGemTooltip, tt, b, { gemData = data.gems[id], level = lq[1], quality = lq[2] })
		table.insert(res, ok and ser(tt, id .. " " .. lq[1] .. "/" .. lq[2]) or (id .. " " .. lq[1] .. "/" .. lq[2] .. " ERROR"))
	end
end
for gi, group in ipairs(b.skillsTab.socketGroupList) do
	for ji, gem in ipairs(group.gemList) do
		if gem.gemData then
			local tt = new("Tooltip"):Tooltip()
			local ok, err = pcall(gemTooltip.AddGemTooltip, tt, b, gem)
			local head = "group " .. gi .. " gem " .. ji .. " " .. gem.gemData.id
			table.insert(res, ok and ser(tt, head) or (head .. " ERROR"))
		end
	end
end
return res
"#;

fn rust_tooltips(rust: &mut RustSide) -> Vec<String> {
    let cx = &mut rust.app.cx;
    let model = cx.build().model.clone();
    // The calc's `ProcessSocketGroup` of item-granted groups (CalcSetup) writes
    // `levels[l].cost = {}` into PoB's data (SkillsTab.lua:1188, `levelCostCleared`); through
    // the bridge that write happened in the bridge's VM, so replay it in this process.
    for group in model.skillsTab.socketGroupList.get().snapshot() {
        model.skillsTab.ProcessSocketGroup(&model, &group);
    }
    let ser = |tt: &Tooltip, head: String| {
        let mut lines = vec![head];
        for l in &tt.lines {
            lines.push(format!(
                "{}|{}|{}",
                pob_lua::tostring_num(l.size),
                l.text.as_deref().unwrap_or("nil"),
                l.font.map_or("nil", pob_sg::Font::lua_name)
            ));
        }
        lines.join("\n")
    };
    let mut res = Vec::new();
    let mut ids: Vec<_> = pob_gamedata::data().gems.iter().collect();
    ids.sort_by(|a, b| a.0.as_str().as_bytes().cmp(b.0.as_str().as_bytes()));
    for (id, gem) in ids {
        for (l, q) in [(1.0, 0.0), (20.0, 20.0)] {
            let head = format!("{id} {l}/{q}", l = l as i64, q = q as i64);
            let inst = TooltipGemInstance {
                gem_data: TooltipGemData::Gem(gem.clone()),
                level: l,
                quality: q,
                display_effect: None,
            };
            let mut tt = Tooltip::default();
            let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                gem_tooltip::add_gem_tooltip(cx, &mut tt, &model, &inst, None);
            }))
            .is_ok();
            res.push(if ok {
                ser(&tt, head)
            } else {
                format!("{head} ERROR")
            });
        }
    }
    for (gi, group) in model
        .skillsTab
        .socketGroupList
        .get()
        .snapshot()
        .iter()
        .enumerate()
    {
        for (ji, gem) in group.gemList.get().snapshot().iter().enumerate() {
            if let Some(gd) = gem.gemData.get() {
                let head = format!("group {} gem {} {}", gi + 1, ji + 1, gd.id);
                let inst = TooltipGemInstance::of_gem(gem, gd);
                let mut tt = Tooltip::default();
                let ok = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
                    gem_tooltip::add_gem_tooltip(cx, &mut tt, &model, &inst, None);
                }))
                .is_ok();
                res.push(if ok {
                    ser(&tt, head)
                } else {
                    format!("{head} ERROR")
                });
            }
        }
    }
    res
}

#[test]
fn u29_gem_tooltips() {
    let Some(root) = inputs() else { return };
    let filter = std::env::var("U29_TT_FIXTURES").ok();
    let mode = Mode {
        fake_calc: true,
        focus_new: false,
        show_types: None,
    };
    let names: Vec<String> = fixture_builds()
        .into_iter()
        .filter(|name| !filter.as_ref().is_some_and(|f| !name.contains(f.as_str())))
        .collect();
    let results = oracle_cache::par_map(names, |name| {
        let mut out = TooltipCase::default();
        let script = runner::parse_script(&format!(
            "frames 1\nload-build \"Builds/{name}\"\nframes 2\n"
        ))
        .unwrap();
        let user = UserDir::new(Some(&fixture_dir())).unwrap();
        let screen = runner::screen(1280, 800, 1.0);
        let run = lua_run(&root, &script, &user, screen, &mode, Some(LUA_TOOLTIPS), false);
        assert!(run.error.is_none(), "{name}: Lua: {:?}", run.error);
        let lua: Vec<String> = run.extra;
        // the Rust side on this case thread (fresh per case: its thread-local state is new)
        let rust = {
            let (mut rust_side, _, e) = run_rust(&root, &script, &user, screen, &mode);
            assert!(e.is_none(), "{name}: Rust: {e:?}");
            rust_tooltips(&mut rust_side)
        };
        let mut bad = 0;
        for i in 0..lua.len().max(rust.len()) {
            let (l, r) = (lua.get(i), rust.get(i));
            let head =
                |t: Option<&String>| t.and_then(|t| t.lines().next()).unwrap_or("").to_owned();
            if l != r && head(l).starts_with("group ") && head(l) != head(r) {
                // PoB picks an old-format gem's variant in `pairs` order (u15_lua_calc's
                // `reference_xml` note): the two VMs loaded different gems, nothing to compare
                out.variants += 1;
                out.lines.push(format!(
                    "{name} #{i}: load variant differs: lua {:?} rust {:?}",
                    head(l),
                    head(r)
                ));
                continue;
            }
            if l != r {
                bad += 1;
                if bad <= 8 {
                    out.failures
                        .push(format!("{name} #{i}:\n  lua:  {l:?}\n  rust: {r:?}"));
                }
            }
        }
        if bad > 8 {
            out.failures
                .push(format!("{name}: {bad} differing tooltips in all"));
        }
        out.tooltips += lua.len();
        out.tt_lines += lua.iter().map(|t| t.lines().count() - 1).sum::<usize>();
        out.errors += lua.iter().filter(|t| t.ends_with(" ERROR")).count();
        out.lines
            .push(format!("{name}: {} tooltips compared, {bad} differ", lua.len()));
        out
    });
    let mut failures = Vec::new();
    let (mut tooltips, mut lines, mut errors, mut variants) = (0, 0, 0, 0);
    for case in results {
        for l in &case.lines {
            println!("{l}");
        }
        failures.extend(case.failures);
        tooltips += case.tooltips;
        lines += case.tt_lines;
        errors += case.errors;
        variants += case.variants;
    }
    println!("u29_gem_tooltips: {tooltips} tooltips, {lines} lines ({errors} raise a Lua error on both sides, {variants} skipped: PoB loaded another gem variant)");
    println!("{}", oracle_cache::summary());
    assert!(tooltips > 0, "no fixture selected");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// One fixture's tooltip comparison (collected in fixture order).
#[derive(Default)]
struct TooltipCase {
    lines: Vec<String>,
    failures: Vec<String>,
    tooltips: usize,
    tt_lines: usize,
    errors: usize,
    variants: usize,
}

// ---- u29_scenarios -----------------------------------------------------------------------------

#[test]
fn u29_scenarios() {
    let Some(root) = inputs() else { return };
    let dir = crate_dir().join("tests/u29");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/u29")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u29/*.pobscript");
    let filter = std::env::var("U29_FILTER").ok();
    let mode = Mode {
        fake_calc: true,
        focus_new: false,
        show_types: None,
    };
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        for (w, h, s) in SCREENS {
            cases.push((path.clone(), stem.clone(), (w, h, s)));
        }
    }
    let ran = cases.len();
    let results = oracle_cache::par_map(cases, |(path, stem, (w, h, s))| {
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        {
            let tag = format!("u29_{stem}@{w}x{h}@{s}");
            let user = UserDir::new(fx.user_dir.as_deref()).unwrap();
            let screen = runner::screen(w, h, s);
            let run = lua_run(&root, &script, &user, screen, &mode, None, true);
            let (lua, lua_error) = (run.frames, run.error);
            // the Rust side on this case thread (fresh per case: its thread-local state is new)
            let (mut rust, rust_error) = {
                let (rust_side, rust, rust_error) = run_rust(&root, &script, &user, screen, &mode);
                drop(rust_side);
                (rust, rust_error)
            };
            drop_image_loads(&mut rust);
            let run = RunResult {
                lua,
                rust,
                rust_error: rust_error.clone(),
                lua_snapshots: Vec::new(),
                rust_snapshots: Vec::new(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let titles: usize = run
                .lua
                .iter()
                .flat_map(|f| &f.effects)
                .filter(|e| matches!(e, SideEffect::WindowTitle(t) if t.starts_with("gem ")))
                .count();
            let line = format!(
                "{tag}: {} frames, {ops} Lua draw ops, {titles} gem change calls, {n} divergences{}{}",
                run.lua.len(),
                lua_error.as_ref().map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust_error.as_ref().map_or_else(String::new, |e| format!(", Rust error: {e}"))
            );
            let mut failure = None;
            if n != 0
                || rust_error.is_some()
                || lua_error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
            {
                failure = Some(format!(
                    "{tag}: {n} divergences, frames lua {} rust {} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
            (line, failure)
        }
    });
    let mut failures = Vec::new();
    for (line, failure) in results {
        println!("{line}");
        failures.extend(failure);
    }
    println!("u29_scenarios: {ran} runs");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
