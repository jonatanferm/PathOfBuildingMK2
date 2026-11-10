//! U20 acceptance (ui doc §12 U20 row): Build shell B (Build.lua:771-1617, 2222-2335) on both
//! sides of the UI oracle. Owner: U20.
//!
//! `u20_scenarios`: every `tests/u20/*.pobscript` at 1280x800 and 1920x1080 (a script that sets
//! its own `screen` runs once). Per run: zero T3 divergences (the same draw ops per
//! `(layer, sub)` bucket and the same side effects, frame by frame), the same window titles,
//! and T4: every `snapshot` step's build XML (`buildMode:SaveDB`) and every file a script
//! names with `#! u20-saved FILE` (written by Save / Save As) identical between the sides after
//! the normalisation below.
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc, ui doc §12.1), so the calc numbers
//! are PoB's and the comparison is of U20's code. calc:PWR's `power::BuildOutput` is on
//! rust-port now, but the Rust calculator is not at L6 parity yet (CA's floors; FOLLOWUPS PWR
//! item 3), so running it here would test the calc units rather than U20.
//!
//! Real on both sides: Init (U19), OnFrame (the input loop, the overlay-pane loop, the class
//! dropdown sync, the recalc on `buildFlag` through the calc, the four RefreshStatList calls
//! and the title of every frame (see the RefreshStatList stand-in below), the top/side bar
//! backgrounds), SyncLoadouts,
//! EstimatePlayerProgress, UpdateClassDropdowns, UpdateSecondaryAscendancyDropdown,
//! CloseBuild, Shutdown, CanExit, ResetModFlags, SaveDB / SaveDBFile, the Save and Save As
//! popups. Stand-ins (both sides run the same ones) for the units not ported yet:
//! * `main.modes.LIST` draws nothing (pob-app's `PendingMode`, as in u16-u19);
//! * `RefreshStatList` (U21) only calls `EstimatePlayerProgress` (Build.lua:2118 is its only
//!   call into U20); `RefreshSkillSelectControls` (U21) does nothing (the skill dropdowns stay
//!   empty). With RefreshStatList stubbed, `FormatStat` (U21) never runs, so nothing writes
//!   `self.lastShowThousandsSeparators` &c. (Build.lua:1749-1753) and the five comparisons at
//!   L1274-1288 fire on every frame on both sides: the four extra RefreshStatList calls and the
//!   per-frame window title above are a property of the stubs, not of PoB, where they only
//!   happen after a display setting changes. The sides match because both run the same stubs;
//!   once U21 lands, its FormatStat writes `on_frame::State.last_*` (CCR-0104 §2);
//! * the tabs' `Draw` (U22-U46) draw nothing; the Lua tabs are PoB's own otherwise (Init
//!   constructs and loads them, and their modFlags count in `self.unsaved`). The Rust side reads
//!   `spec.modFlag` from the stand-in of `spec:AddUndoState` (UndoHandler.lua:37 sets it; the
//!   spec's undo is U25's);
//! * `main:OpenAboutPopup` / `OpenOptionsPopup` open a message popup naming their argument;
//! * `#! u20-panes`: two overlay panes (`build.overlayPanes`, CalcSectionControl's in PoB,
//!   U39) that record their calls as window titles and draw a rectangle, installed right after
//!   `buildMode:Init` on both sides.
//!
//! T4 normalisation (written reasons, ui doc §11.1 T4): attributes are compared as sets (Lua
//! builds `xml.attrib` with a table constructor whose `pairs` order is per-process random);
//! the top-level sections are compared as a multiset (`pairs(self.savers)`, Build.lua:2288),
//! and so are the children of `Calcs`, `ConfigSet`, `ItemSet`, `Sockets` and `Overrides`
//! (written from `pairs`, see `unordered_children`);
//! `PlayerStat` / `FullDPSSkill` / `MinionStat` are dropped from both (the display-stat writer
//! is U21's `build_xml::set_save_stats_hook`, not registered yet); the UI-saver sections
//! (`Import`, `Notes`, `TreeView`: U43, U22, U25-U28) are compared on their own and a
//! difference is reported, not failed (until their owners register savers, calc:BM-A writes back
//! what it loaded, CCR-0080 §8, and an empty element for a build without one; the Lua tabs
//! write their own state, e.g. ImportTab's `exportParty`); a Lua
//! `Gem` `enableGlobal1/2="nil"` / `Skill` `includeInFullDPS`, `mainActiveSkill(Calcs)="nil"`
//! compare equal to the Rust defaults `"false"` / `"0"` (A8 item 15: the frozen
//! `GemInstance` / `SocketGroup` fields are not `Option`); the `variantId` / `nameSpec` of a gem
//! whose variant PoB picks through `pairs` (SkillsTab.lua:472-478, several variants share the
//! granted effect; PoB's pick differs between its own runs) are not compared. Each report line counts what was set aside and says
//! whether the texts were also byte-identical.
//!
//! Compared side effects exclude `ImageLoad` (the Lua tabs' constructors load the tree's
//! images; U23 ports them).

use std::cell::RefCell;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, OverlayPane, TabFlag};
use pob_app::main_mode::{Mode, ModeArg};
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::cx::Cx;
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::ViewPort;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.0)];

/// The two fake overlay panes: name, x, y, width, height.
const PANES: [(&str, f64, f64, f64, f64); 2] = [
    ("A", 400.0, 200.0, 300.0, 300.0),
    ("B", 550.0, 350.0, 300.0, 300.0),
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

fn test_dir() -> PathBuf {
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u20")
}

/// The Lua stand-ins (see the module notes).
fn lua_stubs(panes: bool) -> String {
    let pane_code = PANES
        .iter()
        .map(|(name, x, y, w, h)| {
            format!("t_insert(self.overlayPanes, fakePane({name:?}, {x}, {y}, {w}, {h}))")
        })
        .collect::<Vec<_>>()
        .join("\n");
    format!(
        r#"
local t_insert = table.insert
local function stubMode()
	local m = {{ }}
	function m:Init(a1, a2) self.a1, self.a2 = a1, a2 end
	function m:GetArgs() return self.a1, self.a2 end
	function m:OnFrame(inputEvents, viewPort) end
	return m
end
local function fakePane(name, x, y, w, h)
	local p = {{ isOverlay = true }}
	function p:IsMouseInOverlay(cx, cy) return cx >= x and cx < x + w and cy >= y and cy < y + h end
	function p:HandleOverlayClick(key, cx, cy) SetWindowTitle("click " .. name .. " " .. key .. " " .. cx .. " " .. cy) end
	function p:HandleOverlayRelease(key) SetWindowTitle("release " .. name .. " " .. key) end
	function p:DrawOverlay(viewPort, inputEvents)
		SetDrawColor(1, 0, 0)
		DrawImage(nil, x, y, w, h)
	end
	return p
end
main.modes.LIST = stubMode()
local b = main.modes.BUILD
function b:RefreshStatList() self:EstimatePlayerProgress() end
function b:RefreshSkillSelectControls(controls, mainGroup, suffix) end
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({{ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "calcsTab", "compareTab" }}) do
		if self[t] then self[t].Draw = function() end end
	end
	if {panes} and self.overlayPanes then
{pane_code}	end
end
function main:OpenAboutPopup(helpSectionName) self:OpenMessagePopup("About", tostring(helpSectionName)) end
function main:OpenOptionsPopup() self:OpenMessagePopup("Options", "stub") end
"#,
        panes = if panes { "true" } else { "false" },
    )
}

/// A fake pane on the Rust side (the Lua `fakePane`).
struct FakePane {
    name: &'static str,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
}

impl OverlayPane for FakePane {
    fn is_overlay(&self, _cx: &Cx) -> bool {
        true
    }
    fn is_mouse_in_overlay(&self, _cx: &Cx, cx_: f64, cy: f64) -> bool {
        cx_ >= self.x && cx_ < self.x + self.w && cy >= self.y && cy < self.y + self.h
    }
    fn handle_overlay_click(&self, cx: &mut Cx, key: Key, x: f64, y: f64) {
        let t = format!(
            "click {} {} {} {}",
            self.name,
            key.name(),
            pob_lua::tostring_num(x),
            pob_lua::tostring_num(y)
        );
        cx.sys.platform.set_window_title(&t);
    }
    fn handle_overlay_release(&self, cx: &mut Cx, key: Key) {
        let t = format!("release {} {}", self.name, key.name());
        cx.sys.platform.set_window_title(&t);
    }
    fn draw_overlay(&self, cx: &mut Cx, _vp: &ViewPort, _ev: &mut InputEvents) {
        cx.sg.set_draw_color(1.0, 0.0, 0.0);
        cx.sg.draw_image(None, self.x, self.y, self.w, self.h);
    }
}

/// `main.modes.BUILD` with the Init wrapper of the Lua stand-ins (installs the fake panes).
struct BuildWithPanes {
    inner: Rc<dyn Mode>,
    panes: bool,
}

impl Mode for BuildWithPanes {
    fn init(&self, cx: &mut Cx, args: &[ModeArg]) {
        self.inner.init(cx, args);
        if self.panes && cx.app().build.is_some() {
            for (name, x, y, w, h) in PANES {
                of::push_overlay_pane(cx, Rc::new(FakePane { name, x, y, w, h }));
            }
        }
    }
    fn shutdown(&self, cx: &mut Cx) {
        self.inner.shutdown(cx);
    }
    fn on_frame(&self, cx: &mut Cx, ev: &mut InputEvents, vp: &ViewPort) {
        self.inner.on_frame(cx, ev, vp);
    }
    fn get_args(&self, cx: &Cx) -> Option<Vec<ModeArg>> {
        self.inner.get_args(cx)
    }
    fn can_exit(&self, cx: &mut Cx, reason: &str) -> Option<bool> {
        self.inner.can_exit(cx, reason)
    }
    fn view_mode(&self, cx: &Cx) -> Option<String> {
        self.inner.view_mode(cx)
    }
    fn set_view_mode(&self, cx: &mut Cx, v: &str) {
        self.inner.set_view_mode(cx, v);
    }
    fn output_revision(&self, cx: &Cx) -> Option<f64> {
        self.inner.output_revision(cx)
    }
    fn build_flag(&self, cx: &Cx) -> bool {
        self.inner.build_flag(cx)
    }
}

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn rust_stubs(cx: &mut Cx) {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    let h = &mut cx.app_mut().main.hooks;
    h.open_about_popup = Rc::new(|cx, s: ModeArg| {
        pob_ui_core::popups::open_message_popup(cx, "About", &s.tostring());
    });
    h.open_options_popup = Rc::new(|cx| {
        pob_ui_core::popups::open_message_popup(cx, "Options", "stub");
    });
    init::set_hooks(|h| {
        *h = init::default_hooks();
        // U21 stand-in: RefreshStatList only runs EstimatePlayerProgress
        h.refresh_stat_list = Rc::new(pob_app::build::loadouts::estimate_player_progress);
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc, reached from
        // SyncLoadouts' SetSel)
        h.tree_get_spec_list = Rc::new(|cx| {
            let m = &cx.build().model;
            let gv = &m.data.gameVersions;
            m.treeTab
                .specList
                .with(Clone::clone)
                .iter()
                .map(|spec| {
                    let tv = spec.treeVersion.get();
                    let prefix = if tv == gv.latestTreeVersion {
                        String::new()
                    } else {
                        format!("[{}] ", gv.treeVersions[&tv].display)
                    };
                    let title = spec
                        .title
                        .get()
                        .map_or_else(|| "Default".to_owned(), |s| s.to_string());
                    format!("{prefix}{title}")
                })
                .collect()
        });
    });
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        // the Lua side replaces every tab's Draw (lua_stubs), Notes included (U22's arm of the
        // default draws it; CCR-0106 §3)
        h.draw_tab = Rc::new(|_, _, _, _| {});
        // U21 stand-in, as lua_stubs (U21's is the default now, CCR-0106 §4)
        h.refresh_skill_select_controls = Rc::new(|_, _, _, _| {});
        h.tab_flag =
            Rc::new(|_, flag| flag == TabFlag::Spec && SPEC_MOD_FLAG.with(std::cell::Cell::get));
        h.set_tab_flag = Rc::new(|_, flag, v| {
            if flag == TabFlag::Spec {
                SPEC_MOD_FLAG.with(|f| f.set(v));
            }
        });
    });
}

/// `#! NAME ARG` directives.
fn directives(text: &str, name: &str) -> Vec<String> {
    let tag = format!("#! {name}");
    text.lines()
        .filter_map(|l| l.trim().strip_prefix(tag.as_str()))
        .map(|s| s.trim().to_owned())
        .collect()
}

fn lua_setup(name: &str) -> &'static str {
    match name {
        "newbuild" => r#"main:SetMode("BUILD", false, "Unnamed build")"#,
        other => panic!("unknown u20-setup {other}"),
    }
}

fn rust_setup(cx: &mut Cx, name: &str) {
    match name {
        "newbuild" => pob_app::main_mode::on_frame::set_mode(
            cx,
            "BUILD",
            vec![
                ModeArg::Bool(false),
                ModeArg::Str("Unnamed build".to_owned()),
            ],
        ),
        other => panic!("unknown u20-setup {other}"),
    }
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    setups: Vec<String>,
    panes: bool,
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
        let mut code = lua_stubs(self.panes);
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
        self.app.snapshot()
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        self.app.after_frame()
    }
    fn take_error(&mut self) -> Option<String> {
        self.app.take_error()
    }
}

// ---- the Rust side ----

/// Records the platform side effects the Lua side's bridge records (the oracle's Rust platform
/// does not, CCR-0028 item 2); as in u16-u19.
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
    panes: bool,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, setups: Vec<String>, panes: bool, src: PathBuf) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        // ext:calc is the LuaCalcBridge until calc:PWR lands (ui doc §12.1; the Rust
        // `power::BuildOutput` is still a stub on rust-port)
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || Box::new(LuaCalcBridge::new(Some(src.clone())))),
        );
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
        app.cx.app_mut().launch.force_release_install = true;
        RustSide {
            app,
            setups,
            panes,
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
        let build: Rc<dyn Mode> = Rc::new(BuildWithPanes {
            inner: pob_app::build::mode(),
            panes: self.panes,
        });
        let modes = &mut self.app.cx.app_mut().main.modes;
        modes.insert("BUILD".to_owned(), build);
        // as in u19: the LIST stand-in on both sides
        modes.insert(
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
        Ok(self.app.snapshot())
    }
    fn after_frame(&mut self) -> anyhow::Result<()> {
        // the Lua side fails the run on PoB's error prompt; so does this one
        if let Some(p) = self
            .app
            .cx
            .app()
            .launch
            .prompt_msg
            .clone()
            .filter(|p| p.starts_with("^1Error"))
        {
            anyhow::bail!("the error prompt (launch:ShowErrMsg): {p}");
        }
        Ok(())
    }
}

// ---- T4 ----

/// Elements whose children PoB writes from a `pairs` loop over a string-keyed table (order
/// per-process random): `Calcs` (`pairs(self.input)`, CalcsTab.lua:204), `ConfigSet`
/// (`pairs(configSet.input)` / `placeholder`, ConfigTab.lua:970/983) and `ItemSet`
/// (`pairs(self.slots)`, ItemsTab.lua:1387); and the tree's `Sockets` / `Overrides`
/// (`pairs(self.jewels)` / `pairs(self.hashOverrides)`, PassiveSpec.lua:226/239: integer keys,
/// but filled in an order that comes from string-keyed tables, calc doc §3.4; the two Lua runs
/// of `a_fixtures_t4` write them in different orders).
fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

/// The UI-only savers (Build.lua:629-639): `Import` (ImportTab, U43), `Notes` (NotesTab, U22),
/// `TreeView` (PassiveTreeView, U25-U28). Until their owners register them, calc:BM-A writes
/// the section it loaded back unchanged (CCR-0080 §8), and an empty element when the build had
/// none (a new build), where the Lua tabs write their own state. Compared separately.
const UI_SAVERS: [&str; 3] = ["Import", "Notes", "TreeView"];

/// What a T4 comparison set aside (reported with each run).
#[derive(Default)]
struct T4Notes {
    /// UI-saver sections that differ (set aside, see `UI_SAVERS`)
    pending_sections: Vec<String>,
    /// Gem / Skill fields written as "nil" by Lua, the frozen type's default by Rust
    gem_nil_bools: usize,
    /// Gems whose variant PoB picks through a `pairs` loop (not compared)
    ambiguous_variants: usize,
}

/// A `Gem` element whose `gemId` has more than one variant with its `skillId` as granted
/// effect (`data.gemsByGameId[gemId]`).
fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId"))
    else {
        return false;
    };
    let n = pob_gamedata::data()
        .gems
        .values()
        .filter(|g| *g.gameId == **game_id && *g.grantedEffectId == **skill_id)
        .count();
    n > 1
}

/// The normalised XML (module notes): `(elem, sorted attributes, children)`; the root's
/// sections and the `pairs`-ordered child lists sorted.
fn normalise(node: &XmlNode, top: bool, lua: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        // A8 item 15 (CCR-0019 §4-7): the frozen `GemInstance.enableGlobal1/2` (`Cell<bool>`)
        // and `SocketGroup.includeInFullDPS` (`Cell<bool>`), `mainActiveSkill(Calcs)`
        // (`Cell<f64>`) cannot hold nil, so a gem or group whose field is nil in Lua (the
        // `EnemyExplode` gem the calc adds for explode sources; an item-granted group) saves
        // the Rust default where PoB writes `tostring(nil)`
        let rust_default = match (&*node.elem, &**k) {
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => {
                Some("false")
            }
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| lua && &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem"
            && matches!(&**k, "variantId" | "nameSpec")
            && ambiguous_variant(node)
        {
            // SkillsTab.lua:472-478: an old-format gem (no variantId) picks the first variant
            // of `pairs(possibleVariants)` (string keys, per-process order) whose
            // grantedEffectId matches, and several variants share one (Vaal Righteous Fire
            // and its AltX): PoB's own pick differs between runs, so the pick is not compared
            if k.as_ref() == "variantId" {
                notes.ambiguous_variants += 1;
            }
            attrs.push((k.to_string(), "<pairs-chosen variant>".to_owned()));
        } else {
            attrs.push((k.to_string(), v.to_string()));
        }
    }
    attrs.sort();
    let mut kids: Vec<String> = Vec::new();
    for c in &node.children {
        match c {
            XmlChild::Node(n) => {
                if matches!(&*n.elem, "PlayerStat" | "FullDPSSkill" | "MinionStat") {
                    continue;
                }
                if top && notes.pending_sections.iter().any(|p| **p == *n.elem) {
                    continue;
                }
                kids.push(normalise(n, false, lua, notes));
            }
            XmlChild::Text(t) => kids.push(format!("{t:?}")),
        }
    }
    if top || unordered_children(&node.elem) {
        kids.sort();
    }
    format!("<{} {:?}>[{}]", node.elem, attrs, kids.join(","))
}

/// The `PathOfBuilding` element of a build XML text.
fn root_element(text: &str) -> Result<XmlNode, String> {
    let root = pob_calc::model::xml::ParseXML_root(text)?;
    root.children
        .into_iter()
        .find_map(|c| match c {
            XmlChild::Node(n) => Some(n),
            XmlChild::Text(_) => None,
        })
        .ok_or_else(|| "no root element".to_owned())
}

/// First differing element of two normalised texts (for the report).
fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    let mut local = T4Notes::default();
    // the UI-saver sections: compared on their own; a difference is set aside (reported)
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|c| match c {
            XmlChild::Node(n) if *n.elem == *elem => {
                Some(normalise(n, false, lua, &mut T4Notes::default()))
            }
            _ => None,
        })
    };
    for elem in UI_SAVERS {
        if section(&lr, elem, true) != section(&rr, elem, false) {
            local.pending_sections.push(elem.to_owned());
        }
    }
    let ln = normalise(&lr, true, true, &mut local);
    let rn = normalise(&rr, true, false, &mut local);
    notes.gem_nil_bools += local.gem_nil_bools;
    notes.ambiguous_variants += local.ambiguous_variants;
    notes.pending_sections.extend(local.pending_sections);
    if ln != rn {
        return Err(format!("{what}: T4 differs: {}", first_diff(&ln, &rn)));
    }
    Ok(lua == rust)
}

// ---- runs ----

fn drop_image_loads(frames: &mut [FrameTrace]) {
    for f in frames {
        f.effects.retain(|e| !matches!(e, SideEffect::ImageLoad(_)));
    }
}

struct Side {
    frames: Vec<FrameTrace>,
    error: Option<String>,
    snapshots: Vec<(String, String)>,
    saved: Vec<(String, Option<String>)>,
}

fn read_saved(user: &UserDir, names: &[String]) -> Vec<(String, Option<String>)> {
    names
        .iter()
        .map(|n| {
            let p = user.path.join("Path of Building").join(n);
            (n.clone(), std::fs::read_to_string(p).ok())
        })
        .collect()
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    setups: &[String],
    panes: bool,
    saved: &[String],
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide {
        app: lua_app,
        setups: setups.to_vec(),
        panes,
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    drop(lua_side);
    drop_image_loads(&mut frames);
    let lua = Side {
        frames,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, setups.to_vec(), panes, src);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        runner::drive(&mut rust_side, script, &mut d)
    }));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (
            Vec::new(),
            Some(format!("panicked: {}", pob_host::panic_message(&*p))),
        ),
    };
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
        saved: read_saved(&user, saved),
    };
    Ok((lua, rust))
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

/// The titles without the per-frame repeats (for the report line).
fn distinct_titles(t: &[String]) -> Vec<String> {
    let mut out: Vec<String> = Vec::new();
    for s in t {
        if out.last() != Some(s) {
            out.push(s.clone());
        }
    }
    out
}

/// Every T4 pair of a run; `Ok(number of pairs, byte-identical pairs)`.
fn t4(lua: &Side, rust: &Side, notes: &mut T4Notes) -> Result<(usize, usize), Vec<String>> {
    let mut errs = Vec::new();
    let (mut n, mut same) = (0, 0);
    if lua.snapshots.len() != rust.snapshots.len() {
        errs.push(format!(
            "snapshots: lua {} rust {}",
            lua.snapshots.len(),
            rust.snapshots.len()
        ));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    for ((name, l), (_, r)) in lua.saved.iter().zip(&rust.saved) {
        match (l, r) {
            (Some(l), Some(r)) => {
                n += 1;
                match compare_t4(&format!("saved {name}"), l, r, notes) {
                    Ok(b) => same += usize::from(b),
                    Err(e) => errs.push(e),
                }
            }
            (None, None) => errs.push(format!("saved {name}: written by neither side")),
            (l, r) => errs.push(format!(
                "saved {name}: lua {} rust {}",
                if l.is_some() { "wrote it" } else { "did not" },
                if r.is_some() { "wrote it" } else { "did not" }
            )),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

#[test]
fn u20_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u20")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u20/*.pobscript");
    let filter = std::env::var("U20_FILTER").ok();
    let mut failures = Vec::new();
    let mut ran = 0;
    let (mut t4_pairs, mut t4_bytes) = (0, 0);
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
        let setups = directives(&text, "u20-setup");
        let panes = !directives(&text, "u20-panes").is_empty();
        let saved = directives(&text, "u20-saved");
        for (w, h, s) in screens {
            let tag = if own_screen {
                format!("u20_{stem}")
            } else {
                format!("u20_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &setups,
                panes,
                &saved,
            ) {
                Ok(s) => s,
                Err(e) => {
                    failures.push(format!("{tag}: {e:#}"));
                    continue;
                }
            };
            ran += 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let t4r = t4(&lua, &rust, &mut notes);
            let run = RunResult {
                lua: lua.frames,
                rust: rust.frames,
                rust_error: rust.error.clone(),
                lua_snapshots: lua.snapshots.clone(),
                rust_snapshots: rust.snapshots.clone(),
                audit: Vec::new(),
                compared: true,
            };
            let n = diff_report::write_report(&out_dir(), &tag, &run, false).unwrap_or(usize::MAX);
            let ops = draw_ops(&run.lua);
            let t4_line = match &t4r {
                Ok((p, b)) => {
                    t4_pairs += p;
                    t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}"))
            );
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
            {
                failures.push(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?} (see {})",
                    run.lua.len(),
                    run.rust.len(),
                    distinct_titles(&titles.0),
                    distinct_titles(&titles.1),
                    lua.error,
                    rust.error,
                    out_dir().join(&tag).join("report.txt").display()
                ));
            }
        }
    }
    println!("u20: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
