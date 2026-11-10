//! U21 acceptance (ui doc §12 U21 row): the build stats (Build.lua:1618-2221,
//! Modules/BuildDisplayStats.lua) on both sides of the UI oracle. Owner: U21.
//!
//! * `u21_format_stat`: `buildMode:FormatStat` against PoB's for 500 values, each under 10
//!   stat descriptions (fmt / pc / mod / compactValue / chaosInoc / the "Unreserved Life" label,
//!   with and without an over-cap value and a colour override) and 3 option sets (compact
//!   values, thousands separators, custom separators).
//! * `u21_sidebar`: every `tests/u21/*.pobscript` (the 6 fixture builds idle and hovered; two
//!   switch the main socket group, which recalculates) at 1280x800 @1.0 and 1920x1080 @1.5. The
//!   Rust side computes through `LuaCalcBridge` (ui doc §10.4). Zero T3 divergences: the stat box
//!   text, the skill selectors (RefreshSkillSelectControls) and the warnings are drawn by
//!   `DrawControls` on both sides.
//! * `u21_compare_tooltips`: `buildMode:AddStatComparesToTooltip` for the 6 fixtures: per build,
//!   unallocating one notable, allocating one (with `nodeCount = 2`) and the full-DPS calculator,
//!   each compared line by line (size and text) with PoB's tooltip; the Rust side's outputs come
//!   from the bridge's `MiscCalc`.
//! * `u21_sidebar_breakdown`: `buildMode:GetSidebarBreakdown` (through it `BuildBreakdownIndex`
//!   and `GetStatBreakdownKey`) for the 6 fixtures: every index key, and every display-stat row
//!   of both lists with its breakdown key and with its raw `breakdown or stat`; each result is
//!   serialized with sorted keys (CalcSectionControl's `calcSection` / `enabled` skipped) and
//!   compared with PoB's.
//!
//! Stand-ins (both sides run the same ones): `main.modes.LIST` draws nothing; `UpdateClassDropdowns`
//! records its argument as a window title and `SyncLoadouts` does nothing (U20);
//! `EstimatePlayerProgress` (U20) sets fixed `pointDisplay` texts; `buildMode:OnFrame` (U20) runs
//! only `ProcessControlsInput`, the recalc block (Build.lua:1259-1271 without the breakdown and
//! the misc calculator), `RefreshSkillSelectControls` (L1291), `SetDrawLayer(5)` and
//! `DrawControls`; `SkillsTab:AddSocketGroupTooltip` (U30) adds one line naming the group.
//! Compared side effects exclude `ImageLoad` (the Lua tabs load the tree's images; U23).

#![allow(clippy::format_push_string, clippy::case_sensitive_file_extension_comparisons)]

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;
use std::task::Poll;

use pob_app::app::CxApp;
use pob_app::build::display_stats::DisplayStat;
use pob_app::build::init::{self, InfoControl};
use pob_app::build::{skill_select, stats};
use pob_app::calc::CalcFacade;
use pob_app::main_mode::ModeArg;
use pob_calc::model::BuildRef;
use pob_calc::prelude::{LjIntMap, Override, Shared, Val};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::ViewPort;
use pob_ui_core::tooltip::tooltip::Tooltip;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3). The bridge boots its own, one per
/// operation, while the Lua side's VM is already dropped.
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

/// ui doc §11.3: 1280x800 @1.0 and 1920x1080 @1.5.
const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

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

// ---- stand-ins ----------------------------------------------------------------------------------

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
function b:EstimatePlayerProgress()
	self.controls.pointDisplay.str = {str:?}
	self.controls.pointDisplay.req = {req:?}
end
function b:OnFrame(inputEvents)
	self:ProcessControlsInput(inputEvents, main.viewPort)
	if self.buildFlag then
		wipeGlobalCache()
		self.outputRevision = self.outputRevision + 1
		self.buildFlag = false
		self.skillsTab:UpdateSocketGroups()
		self.calcsTab:BuildOutput()
		self:RefreshStatList()
	end
	self:RefreshSkillSelectControls(self.controls, self.mainSocketGroup, "")
	SetDrawLayer(5)
	self:DrawControls(main.viewPort)
end
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	if self.skillsTab then
		function self.skillsTab:AddSocketGroupTooltip(tooltip, socketGroup)
			tooltip:AddLine(16, "^7group " .. tostring(socketGroup.label))
		end
	end
end
function main:OpenAboutPopup(helpSectionName) self:OpenMessagePopup("About", tostring(helpSectionName)) end
function main:OpenOptionsPopup() self:OpenMessagePopup("Options", "stub") end
"#,
        str = POINT_STR,
        req = POINT_REQ,
    )
}

fn rust_on_frame(cx: &mut Cx, ev: &mut InputEvents, _vp: &ViewPort) {
    let Some(node) = cx.app().build.as_ref().map(|b| b.node.id()) else {
        return;
    };
    init::flush_window_titles(cx);
    if cx.child(node, "pointDisplay").is_none() {
        return;
    }
    let vp = cx.main.view_port;
    // L1253
    cx.process_controls_input(node, ev, &vp);
    let m = cx.build().model.clone();
    // L1261-1271 (without the pinned breakdown and the misc calculator)
    if m.buildFlag.get() {
        pob_calc::modules::common::wipeGlobalCache();
        m.outputRevision.set(m.outputRevision.get() + 1.0);
        m.buildFlag.set(false);
        m.skillsTab.UpdateSocketGroups(&m);
        let _ = cx.build_mut().calc.build_output(&m);
        stats::refresh_stat_list(cx);
    }
    // L1291
    let msg = m.mainSocketGroup.get();
    skill_select::refresh_skill_select_controls(cx, node, msg, "");
    cx.sg.set_draw_layer(Some(5), None);
    cx.draw_controls(node, &vp, None);
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
        h.skills_add_socket_group_tooltip = Rc::new(|cx, tooltip, group| {
            tooltip.add_line(cx, 16.0, &format!("^7group {}", group.label.get()));
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

// ---- the Lua side ---------------------------------------------------------------------------

struct LuaSide {
    app: LuaApp,
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
        let code = lua_stubs();
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

// ---- the Rust side --------------------------------------------------------------------------

/// Records the platform side effects the Lua side's bridge records (as in u19_build.rs).
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
    /// `bridge`: the PoB `src/` the LuaCalcBridge loads (None: the Rust calc).
    fn new(mut parts: PlatformParts, bridge: Option<PathBuf>) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = match bridge {
            Some(src) => pob_app::PobApp::with_calc(
                parts,
                Rc::new(move || -> Box<dyn CalcFacade> {
                    Box::new(LuaCalcBridge::new(Some(src.clone())))
                }),
            ),
            None => pob_app::PobApp::new(parts),
        };
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
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
        // CCR-0098 §5: main.modes.BUILD is Build.lua's object (main:Init installed the
        // placeholder); CCR-0100 C: LIST is stubbed on both sides
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
                ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }
            )
        })
        .count()
}

/// The strings drawn (the stat box, selectors and warnings are text).
fn drawn_texts(frames: &[FrameTrace]) -> usize {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| matches!(c.call, ApiCall::DrawString { .. }))
        .count()
}

// ---- u21_sidebar ----------------------------------------------------------------------------

struct Sides {
    lua: Vec<FrameTrace>,
    rust: Vec<FrameTrace>,
    lua_error: Option<String>,
    rust_error: Option<String>,
}

fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
) -> anyhow::Result<Sides> {
    let user = UserDir::new(fixture)?;
    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide { app: lua_app };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut lua, r) = runner::drive(&mut lua_side, script, &mut d);
    let lua_error = r.err().map(|e| e.to_string());
    drop(lua_side);
    // Rust (the bridge boots its own PoB per calc)
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, Some(root.join("src")));
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

#[test]
fn u21_sidebar() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let dir = crate_dir().join("tests/u21");
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(&dir)
        .expect("tests/u21")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u21/*.pobscript");
    let filter = std::env::var("U21_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let mut total_texts = 0;
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        for (w, h, s) in SCREENS {
            let tag = format!("u21_{stem}@{w}x{h}@{s}");
            let sides = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(w, h, s))
            {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
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
            let texts = drawn_texts(&run.lua);
            total_texts += texts;
            println!(
                "{tag}: {} frames, {ops} Lua draw ops ({texts} strings), {n} divergences{}{}",
                run.lua.len(),
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
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
    }
    println!("u21_sidebar: {ran} runs, {total_texts} Lua strings drawn");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u21_compare_tooltips -------------------------------------------------------------------

/// One comparison: an override (built on each side), `useFullDPS`, the header and `nodeCount`.
struct Case {
    name: String,
    add: Option<f64>,
    remove: Option<f64>,
    full_dps: Option<bool>,
    header: &'static str,
    node_count: Option<f64>,
}

/// The first allocated and the first unallocated (next to the tree) plain notable, by id.
fn pick_nodes(build: &BuildRef) -> (f64, f64) {
    let spec = build.spec.get();
    let mut nodes: Vec<_> = spec.nodes.with(LjIntMap::pairs);
    nodes.sort_by_key(|(id, _)| *id);
    let plain = |n: &pob_calc::classes::Node| {
        n.ascendancyName.get().is_none()
            && !n.isMastery.get()
            && !n.isJewelSocket.get()
            && !n.isKeystone.get()
            && n.classStartIndex.get().is_none()
            && !n.modList.get().is_empty()
    };
    let add = nodes
        .iter()
        .find(|(_, n)| {
            !n.alloc.get()
                && plain(n)
                && n.isNotable.get()
                && n.linked.get().snapshot().iter().any(|l| l.alloc.get())
        })
        .map(|(_, n)| n.id)
        .expect("an unallocated notable");
    let rem = nodes
        .iter()
        .find(|(_, n)| n.alloc.get() && plain(n) && n.isNotable.get())
        .map(|(_, n)| n.id)
        .expect("an allocated notable");
    (add, rem)
}

fn cases(xml: &str) -> Vec<Case> {
    let model = pob_calc::model::build_xml::loadBuildModelFromXML(xml, pob_gamedata::data())
        .expect("BM-A loads the fixture");
    let (add, rem) = pick_nodes(&model);
    vec![
        Case {
            name: format!("removeNodes {rem}"),
            add: None,
            remove: Some(rem),
            full_dps: None,
            header: "^7Unallocating this node will give you:",
            node_count: None,
        },
        Case {
            name: format!("addNodes {add}"),
            add: Some(add),
            remove: None,
            full_dps: None,
            header: "^7Allocating this node and all nodes leading to it will give you:",
            node_count: Some(2.0),
        },
        Case {
            name: "useFullDPS".to_owned(),
            add: None,
            remove: None,
            full_dps: Some(true),
            header: "^7Full DPS:",
            node_count: None,
        },
    ]
}

/// `size|text` per tooltip line.
fn tooltip_lines(tt: &Tooltip) -> Vec<String> {
    tt.lines
        .iter()
        .map(|l| format!("{}|{}", pob_lua::tostring_num(l.size), l.text.as_deref().unwrap_or("<nil>")))
        .collect()
}

fn lua_compare(lua: &mut LuaSide, cases: &[Case]) -> Vec<Vec<String>> {
    let mut code = String::from(
        "local b = main.modes.BUILD\nlocal calcFunc, calcBase = b.calcsTab:GetMiscCalculator()\nlocal res = { }\n",
    );
    for c in cases {
        let ov = match (c.add, c.remove) {
            (Some(a), _) => format!("{{ addNodes = {{ [b.spec.nodes[{a}]] = true }} }}"),
            (_, Some(r)) => format!("{{ removeNodes = {{ [b.spec.nodes[{r}]] = true }} }}"),
            _ => "nil".to_owned(),
        };
        let fd = match c.full_dps {
            Some(true) => "true",
            Some(false) => "false",
            None => "nil",
        };
        let nc = c.node_count.map_or("nil".to_owned(), pob_lua::tostring_num);
        code.push_str(&format!(
            r#"do
	local out = calcFunc({ov}, {fd})
	local tt = new("Tooltip"):Tooltip()
	local n = b:AddStatComparesToTooltip(tt, calcBase, out, {h:?}, {nc})
	local lines = {{ "count=" .. tostring(n) }}
	for _, l in ipairs(tt.lines) do table.insert(lines, tostring(l.size) .. "|" .. tostring(l.text)) end
	table.insert(res, table.concat(lines, "\n"))
end
"#,
            h = c.header
        ));
    }
    code.push_str("return res\n");
    let out: Vec<String> = lua
        .app
        .with_vm(|pob| pob.eval::<Vec<String>>(&code))
        .unwrap_or_else(|e| panic!("Lua compare: {e}"));
    out.into_iter()
        .map(|s| s.split('\n').map(str::to_owned).collect())
        .collect()
}

fn node_map(build: &BuildRef, id: f64) -> Shared<LjIntMap<pob_calc::classes::NodeRef>> {
    let node = build
        .spec
        .get()
        .nodes
        .with(|m| m.get(id as i64))
        .expect("node");
    let mut m = LjIntMap::new();
    m.set(id as i64, Some(node));
    Shared::new(m)
}

fn rust_compare(rust: &mut RustSide, cases: &[Case]) -> Vec<Vec<String>> {
    let cx = &mut rust.app.cx;
    let model = cx.build().model.clone();
    let (calc, base) = match cx.build_mut().calc.misc_calculator(&model) {
        Poll::Ready(x) => x,
        Poll::Pending => unreachable!("desktop"),
    };
    let mut out = Vec::new();
    for c in cases {
        let ov = match (c.add, c.remove) {
            (Some(a), _) => {
                let o = Override::default();
                o.addNodes.set(Some(node_map(&model, a)));
                Some(Rc::new(o))
            }
            (_, Some(r)) => {
                let o = Override::default();
                o.removeNodes.set(Some(node_map(&model, r)));
                Some(Rc::new(o))
            }
            _ => None,
        };
        let output = match calc.call(ov, c.full_dps) {
            Poll::Ready(o) => o,
            Poll::Pending => unreachable!("desktop"),
        };
        let mut tt = Tooltip::default();
        let n = stats::add_stat_compares_to_tooltip(cx, &mut tt, &base, &output, c.header, c.node_count);
        let mut lines = vec![format!("count={}", pob_lua::tostring_num(n))];
        lines.extend(tooltip_lines(&tt));
        out.push(lines);
    }
    out
}

#[test]
fn u21_compare_tooltips() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut failures = Vec::new();
    let (mut compared, mut lines_total) = (0, 0);
    for name in fixture_builds() {
        let xml = std::fs::read_to_string(fixture_dir().join("Path of Building/Builds").join(&name))
            .unwrap();
        let cases = cases(&xml);
        let script = runner::parse_script(&format!(
            "frames 1\nload-build \"Builds/{name}\"\nframes 2\n"
        ))
        .unwrap();
        let user = UserDir::new(Some(&fixture_dir())).unwrap();
        let screen = runner::screen(1280, 800, 1.0);
        // Lua
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).unwrap();
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (_, r) = runner::drive(&mut lua_side, &script, &mut d);
        r.unwrap_or_else(|e| panic!("{name}: Lua: {e}"));
        let lua = lua_compare(&mut lua_side, &cases);
        drop(lua_side);
        // Rust
        user.reset().unwrap();
        let handles = OracleHandles::default();
        let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
        let mut rust_side = RustSide::new(PlatformParts { sg, sys }, Some(root.join("src")));
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
        r.unwrap_or_else(|e| panic!("{name}: Rust: {e}"));
        let rust = rust_compare(&mut rust_side, &cases);
        drop(rust_side);
        for ((c, l), r) in cases.iter().zip(&lua).zip(&rust) {
            compared += 1;
            lines_total += l.len() - 1;
            println!("{name} {}: {} | {} lines", c.name, l[0], l.len() - 1);
            if l != r {
                failures.push(format!(
                    "{name} {}:\n  lua:  {l:?}\n  rust: {r:?}",
                    c.name
                ));
            }
        }
    }
    println!("u21_compare_tooltips: {compared} comparisons, {lines_total} tooltip lines");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

// ---- u21_sidebar_breakdown ------------------------------------------------------------------

/// CalcSectionControl's own fields on the section data (CalcSectionControl.lua:38 `calcSection`,
/// the per-frame `data.enabled`): not part of the index (stats.rs DIVERGENCE(section-list)).
const SECTION_CONTROL_FIELDS: &str = "{ calcSection = true, enabled = true }";

/// The Lua serializer: keys sorted (by type name, then value), functions as `function`,
/// CalcSectionControl fields skipped.
const LUA_SER: &str = r#"
local skip = __SKIP__
local function ser(v)
	local t = type(v)
	if t == "table" then
		local keys = { }
		for k in pairs(v) do
			if not skip[k] then table.insert(keys, k) end
		end
		table.sort(keys, function(a, b)
			local ta, tb = type(a), type(b)
			if ta ~= tb then return ta < tb end
			if ta == "boolean" then return not a and b end
			return a < b
		end)
		local parts = { }
		for _, k in ipairs(keys) do
			table.insert(parts, tostring(k) .. "=" .. ser(v[k]))
		end
		return "{" .. table.concat(parts, ",") .. "}"
	elseif t == "string" then
		return '"' .. v .. '"'
	elseif t == "function" then
		return "function"
	else
		return tostring(v)
	end
end
"#;

/// [`LUA_SER`] on the Rust side.
fn rust_ser(v: &Val) -> String {
    use pob_calc::prelude::Key;
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => pob_lua::tostring_num(*n),
        Val::Str(s) => format!("\"{s}\""),
        Val::Table(t) => {
            let mut kv: Vec<(Key, Val)> = t
                .pairs()
                .filter(|(k, _)| {
                    !matches!(k, Key::S(n) if &**n == "calcSection" || &**n == "enabled")
                })
                .collect();
            // Lua type names order: boolean < number < string
            let rank = |k: &Key| match k {
                Key::B(_) => 0,
                Key::I(_) | Key::F(_) => 1,
                Key::S(_) => 2,
            };
            kv.sort_by(|(a, _), (b, _)| {
                rank(a).cmp(&rank(b)).then_with(|| match (a, b) {
                    (Key::B(x), Key::B(y)) => x.cmp(y),
                    (Key::I(x), Key::I(y)) => x.cmp(y),
                    (Key::I(_) | Key::F(_), Key::I(_) | Key::F(_)) => a
                        .as_num()
                        .partial_cmp(&b.as_num())
                        .unwrap_or(std::cmp::Ordering::Equal),
                    (Key::S(x), Key::S(y)) => x.as_bytes().cmp(y.as_bytes()),
                    _ => std::cmp::Ordering::Equal,
                })
            });
            let parts: Vec<String> = kv
                .iter()
                .map(|(k, v)| {
                    let ks = match k {
                        Key::B(b) => b.to_string(),
                        Key::I(i) => i.to_string(),
                        Key::F(f) => pob_lua::tostring_num(f.get()),
                        Key::S(s) => s.to_string(),
                    };
                    format!("{ks}={}", rust_ser(v))
                })
                .collect();
            format!("{{{}}}", parts.join(","))
        }
        other => format!("{other:?}"),
    }
}

fn lua_sidebar(lua: &mut LuaSide) -> Vec<String> {
    let mut code = LUA_SER.replace("__SKIP__", SECTION_CONTROL_FIELDS);
    code.push_str(
        r#"
local b = main.modes.BUILD
local res = { }
local keys = { }
for k in pairs(b:BuildBreakdownIndex()) do table.insert(keys, k) end
table.sort(keys)
for _, k in ipairs(keys) do
	table.insert(res, "index " .. k .. " => " .. ser(b:GetSidebarBreakdown(k, nil, nil, "player")))
end
for _, list in ipairs({ { b.displayStats, "player" }, { b.minionDisplayStats, "minion" } }) do
	for i, statData in ipairs(list[1]) do
		local ignored
		if statData.ignoredSections then
			ignored = { }
			for _, label in ipairs(statData.ignoredSections) do ignored[label] = true end
		end
		local key = b:GetStatBreakdownKey(statData, list[2])
		table.insert(res, list[2] .. " " .. i .. " " .. tostring(key) .. " => " .. ser(b:GetSidebarBreakdown(key, statData.modNames, ignored, list[2])))
		local raw = statData.breakdown or statData.stat
		if raw then
			table.insert(res, list[2] .. " " .. i .. " raw " .. raw .. " => " .. ser(b:GetSidebarBreakdown(raw, statData.modNames, ignored, "player")))
		end
	end
end
return res
"#,
    );
    lua.app
        .with_vm(|pob| pob.eval::<Vec<String>>(&code))
        .unwrap_or_else(|e| panic!("Lua sidebar breakdown: {e}"))
}

fn rust_sidebar(rust: &mut RustSide) -> Vec<String> {
    use pob_app::build::display_stats;
    let cx = &mut rust.app.cx;
    let mut res = Vec::new();
    let mut keys: Vec<String> = stats::build_breakdown_index(cx).keys().cloned().collect();
    keys.sort();
    for k in keys {
        let t = stats::get_sidebar_breakdown(cx, Some(&k), None, None, Some("player"));
        res.push(format!("index {k} => {}", rust_ser(&Val::Table(t))));
    }
    for (list, actor) in [
        (display_stats::displayStats(), "player"),
        (display_stats::minionDisplayStats(), "minion"),
    ] {
        for (i, stat_data) in list.iter().enumerate() {
            let i = i + 1;
            let ignored: Option<Vec<String>> = stat_data
                .ignoredSections
                .map(|s| s.iter().map(|x| (*x).to_owned()).collect());
            let key = stats::get_stat_breakdown_key(cx, stat_data, actor);
            let t = stats::get_sidebar_breakdown(
                cx,
                key.as_deref(),
                stat_data.modNames,
                ignored.as_deref(),
                Some(actor),
            );
            res.push(format!(
                "{actor} {i} {} => {}",
                key.as_deref().unwrap_or("nil"),
                rust_ser(&Val::Table(t))
            ));
            if let Some(raw) = stat_data.breakdown.or(stat_data.stat) {
                let t = stats::get_sidebar_breakdown(
                    cx,
                    Some(raw),
                    stat_data.modNames,
                    ignored.as_deref(),
                    Some("player"),
                );
                res.push(format!("{actor} {i} raw {raw} => {}", rust_ser(&Val::Table(t))));
            }
        }
    }
    res
}

/// `buildMode:GetSidebarBreakdown` (and through it `BuildBreakdownIndex`) for the 6 fixtures:
/// every index key, then every display-stat row (player and minion lists) with its
/// `GetStatBreakdownKey` key and with its raw `breakdown or stat`, each result serialized
/// field by field (sorted keys) and compared with PoB's.
#[test]
fn u21_sidebar_breakdown() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut failures = Vec::new();
    let (mut compared, mut cells) = (0, 0);
    for name in fixture_builds() {
        let script = runner::parse_script(&format!(
            "frames 1\nload-build \"Builds/{name}\"\nframes 2\n"
        ))
        .unwrap();
        let user = UserDir::new(Some(&fixture_dir())).unwrap();
        let screen = runner::screen(1280, 800, 1.0);
        // Lua
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).unwrap();
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (_, r) = runner::drive(&mut lua_side, &script, &mut d);
        r.unwrap_or_else(|e| panic!("{name}: Lua: {e}"));
        let lua = lua_sidebar(&mut lua_side);
        drop(lua_side);
        // Rust
        user.reset().unwrap();
        let handles = OracleHandles::default();
        let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
        let mut rust_side = RustSide::new(PlatformParts { sg, sys }, Some(root.join("src")));
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (_, r) = runner::drive(&mut rust_side, &script, &mut d);
        r.unwrap_or_else(|e| panic!("{name}: Rust: {e}"));
        let rust = rust_sidebar(&mut rust_side);
        drop(rust_side);
        let index_keys = lua.iter().filter(|l| l.starts_with("index ")).count();
        let n_cells: usize = lua.iter().map(|l| l.matches("breakdown=").count()).sum();
        let bytes: usize = lua.iter().map(String::len).sum();
        println!(
            "{name}: {} calls ({index_keys} index keys), {n_cells} breakdown fields, {bytes} bytes",
            lua.len()
        );
        compared += lua.len();
        cells += n_cells;
        if lua.len() != rust.len() {
            failures.push(format!("{name}: {} Lua calls, {} Rust", lua.len(), rust.len()));
        }
        for (l, r) in lua.iter().zip(&rust) {
            if l != r {
                failures.push(format!("{name}:\n  lua:  {l}\n  rust: {r}"));
            }
        }
    }
    println!("u21_sidebar_breakdown: {compared} calls, {cells} breakdown fields");
    let shown: Vec<_> = failures.iter().take(20).cloned().collect();
    assert!(failures.is_empty(), "{} failures:\n{}", failures.len(), shown.join("\n"));
}

// ---- u21_format_stat ------------------------------------------------------------------------

/// 500 deterministic values: integers, fractions, negatives, magnitudes around the compact
/// thresholds (1e4, 1e6, 1e9), zero and infinities.
fn values() -> Vec<f64> {
    let mut v = vec![0.0, 1.0, -1.0, 0.5, -0.5, 0.05, 0.004, 9999.0, 9999.95, 10000.0, 10049.0,
        999999.0, 999950.0, 1000000.0, 1234567.0, 999999999.0, 1e9, 1.5e9, 123456789012.0,
        f64::INFINITY, -f64::INFINITY, 100.0, 75.0, 0.75, 1.25, 2.5, -12.345, 33.333333,
        66.666666, 0.1, 0.2, 0.3];
    let mut x: u64 = 0x2545_F491_4F6C_DD1D;
    while v.len() < 500 {
        // xorshift64*
        x ^= x >> 12;
        x ^= x << 25;
        x ^= x >> 27;
        let r = x.wrapping_mul(0x2545_F491_4F6C_DD1D);
        let mant = (r >> 11) as f64 / (1u64 << 53) as f64; // [0, 1)
        let exp = ((r >> 3) % 13) as i32 - 3; // 1e-3 .. 1e9
        let sign = if r & 1 == 1 { -1.0 } else { 1.0 };
        let mut val = sign * mant * 10f64.powi(exp);
        if r & 6 == 0 {
            val = val.round();
        }
        // no -0: LuaJIT on this aarch64 host is dual-number (CCR-0002: a ±0 sign is a host
        // artefact, never a port difference)
        if val == 0.0 {
            val = 0.0;
        }
        v.push(val);
    }
    v
}

/// The stat descriptions: (Lua table, Rust row).
fn stat_datas() -> Vec<(&'static str, DisplayStat)> {
    let e = DisplayStat::EMPTY;
    vec![
        ("{ fmt = \"d\" }", DisplayStat { fmt: Some("d"), ..e }),
        ("{ fmt = \".1f\", compactValue = true }", DisplayStat { fmt: Some(".1f"), compactValue: true, ..e }),
        ("{ fmt = \".2f%%\" }", DisplayStat { fmt: Some(".2f%%"), ..e }),
        ("{ fmt = \"d%%\", pc = true }", DisplayStat { fmt: Some("d%%"), pc: true, ..e }),
        ("{ fmt = \"+d%%\", mod = true }", DisplayStat { fmt: Some("+d%%"), r#mod: true, ..e }),
        ("{ fmt = \".3fs\" }", DisplayStat { fmt: Some(".3fs"), ..e }),
        ("{ fmt = \".0f\", compactValue = true, chaosInoc = true }", DisplayStat { fmt: Some(".0f"), compactValue: true, chaosInoc: true, ..e }),
        ("{ fmt = \"+.1f\", label = \"Unreserved Life\" }", DisplayStat { fmt: Some("+.1f"), label: Some("Unreserved Life"), ..e }),
        ("{ fmt = \".1fm\" }", DisplayStat { fmt: Some(".1fm"), ..e }),
        ("{ fmt = \".2f\", compactValue = true }", DisplayStat { fmt: Some(".2f"), compactValue: true, ..e }),
    ]
}

/// The option sets: (compact, show thousands separators, thousands separator, decimal separator).
const OPTIONS: [(bool, bool, &str, &str); 3] = [
    (false, true, ",", "."),
    (true, true, ",", "."),
    (true, false, ".", ","),
];

fn lua_num(x: f64) -> String {
    if x == f64::INFINITY {
        "math.huge".to_owned()
    } else if x == -f64::INFINITY {
        "-math.huge".to_owned()
    } else {
        format!("{x:e}")
    }
}

/// The over-cap value and colour override of value `i` (some rows only).
fn extras(i: usize, x: f64) -> (Option<f64>, Option<&'static str>) {
    let over = (i % 7 == 3 && x.is_finite()).then(|| (x.abs() * 0.37).floor());
    let color = (i % 11 == 5).then_some("^xFF7700");
    (over, color)
}

#[test]
fn u21_format_stat() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let vals = values();
    assert_eq!(vals.len(), 500);
    let datas = stat_datas();
    // Lua: one table of results per option set
    let mut code = String::from("local b = main.modes.BUILD\nlocal res = { }\nlocal vals = {\n");
    for (i, x) in vals.iter().enumerate() {
        let (over, color) = extras(i, *x);
        code.push_str(&format!(
            "\t{{ {}, {}, {} }},\n",
            lua_num(*x),
            over.map_or("nil".to_owned(), lua_num),
            color.map_or("nil".to_owned(), |c| format!("{c:?}"))
        ));
    }
    code.push_str("}\nlocal datas = {\n");
    for (l, _) in &datas {
        code.push_str(&format!("\t{l},\n"));
    }
    code.push_str("}\n");
    for (compact, show, th, dec) in OPTIONS {
        code.push_str(&format!(
            "main.useCompactValues = {compact}\nmain.showThousandsSeparators = {show}\nmain.thousandsSeparator = {th:?}\nmain.decimalSeparator = {dec:?}\n\
             for _, v in ipairs(vals) do for _, d in ipairs(datas) do table.insert(res, b:FormatStat(d, v[1], v[2], v[3])) end end\n"
        ));
    }
    code.push_str("return res\n");
    let user = UserDir::new(Some(&fixture_dir())).unwrap();
    let lua: Vec<String> = {
        let handles = OracleHandles::default();
        let mut app = runner::lua_side(&root, &user, &handles, Entry::Launch, false).unwrap();
        app.on_init();
        app.with_vm(|pob| pob.eval::<Vec<String>>(&code))
            .unwrap_or_else(|e| panic!("Lua FormatStat: {e}"))
    };
    // Rust
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).unwrap();
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, None);
    rust_side.app.on_init();
    let cx = &mut rust_side.app.cx;
    let mut rust = Vec::new();
    for (compact, show, th, dec) in OPTIONS {
        cx.app_mut().main.use_compact_values = compact;
        cx.main.show_thousands_separators = show;
        cx.main.thousands_separator = th.to_owned();
        cx.main.decimal_separator = dec.to_owned();
        for (i, x) in vals.iter().enumerate() {
            let (over, color) = extras(i, *x);
            let over = over.map_or(Val::Nil, Val::Num);
            for (_, d) in &datas {
                rust.push(stats::format_stat(cx, d, &Val::Num(*x), &over, color));
            }
        }
    }
    assert_eq!(lua.len(), rust.len());
    let mut diffs = Vec::new();
    let mut host = 0;
    let per_opt = vals.len() * datas.len();
    for (k, (l, r)) in lua.iter().zip(&rust).enumerate() {
        if l != r {
            let o = k / per_opt;
            let i = (k % per_opt) / datas.len();
            let d = k % datas.len();
            // CCR-0001: `%d` of +inf is x64's cvttsd2si MIN in PoB (what the port prints);
            // this host's arm64 LuaJIT saturates to MAX instead
            let x64_min = r.contains("9,223,372,036,854,775,808")
                || r.contains("9223372036854775808");
            if vals[i] == f64::INFINITY && datas[d].1.fmt.is_some_and(|f| f.contains('d')) && x64_min
            {
                host += 1;
                continue;
            }
            diffs.push(format!(
                "options {o} value {} ({}) data {}: lua {l:?} rust {r:?}",
                i, vals[i], datas[d].0
            ));
        }
    }
    println!(
        "u21_format_stat: {} values x {} stat descriptions x {} option sets = {} strings, {} differ, {host} host-profile (+inf %d, CCR-0001)",
        vals.len(),
        datas.len(),
        OPTIONS.len(),
        lua.len(),
        diffs.len()
    );
    assert!(diffs.is_empty(), "{}", diffs[..diffs.len().min(40)].join("\n"));
}
