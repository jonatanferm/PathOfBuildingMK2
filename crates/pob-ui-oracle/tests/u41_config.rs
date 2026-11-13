//! U41 acceptance (ui doc §12 U41 row: every option type; ifFlag/ifSkill visibility; custom mod
//! blocks; tooltips with enable compares through the bridge): the full app (Launch, the build
//! list, the build shell, the Config tab's constructor) on both sides of the UI oracle. Owner: U41.
//!
//! `u41_scenarios`: every `tests/u41/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (both
//! 1280 virtual px wide, so the scripts' coordinates hold at both; a script that sets its own
//! `screen` runs once). Each script starts at the real build list (U18) of the fixture user
//! dir, opens a build from it, switches to the Config tab (Ctrl+5) and drives it. Per run: zero
//! T3 divergences (the same draw ops per `(layer, sub)` bucket and the same side effects, frame
//! by frame), the same window titles, and T4: every `snapshot` step's build XML
//! (`buildMode:SaveDB`) and every file a script names with `#! u41-saved FILE` identical
//! between the sides after the normalisation below.
//!
//! The Rust side's calc is U15's LuaCalcBridge (ext:calc, ui doc §12.1), as in u20-u22: the
//! option tooltips' compares (`calcFunc()` with the option toggled) run PoB's own calc on the
//! Rust model.
//!
//! Real on both sides: Launch, Main (U16), the build list (U18), Init (U19), OnFrame,
//! SyncLoadouts, SaveDBFile and ResetModFlags (U20), RefreshStatList and the stat box (U21),
//! the ConfigTab constructor, its option controls, CustomModBlockControl and
//! UpdateCustomModsControls (U41). Stand-ins for the units not ported yet:
//! * `ConfigTab:Draw` (ConfigTab.lua:1024-1116, U42): the Lua side runs PoB's own; the Rust side
//!   runs [`config_draw_stand_in`], a test-local port of the same lines (without Undo/Redo,
//!   which no script uses), preceded by U41's bridge pull (`ConfigTab::pull_var_controls`), as
//!   ctor.rs asks of U42's Draw;
//! * `self.configTab.modFlag` (UndoHandler.lua:37 via `configTab:AddUndoState`, U42) reads from
//!   the stand-in of the `add_undo_state` hook; the default `UpdateControls` hook (U41's
//!   composition of the model half) stands in for ConfigTab.lua:1008-1022;
//! * the other tabs' `Draw` (U25-U46, Notes aside) draw nothing (the Lua tabs' `Draw` are
//!   replaced after `buildMode:Init`);
//! * `utf8.next` on the Lua side (the oracle's `lua-utf8` stand-in lacks it; see `LUA_STUBS`);
//! * `spec.modFlag` (U25's spec undo) reads from the stand-in of `spec:AddUndoState`, and
//!   `TreeTab:GetSpecList` (U25) is TreeTab.lua:492-498 (as in u20).
//!
//! T4 normalisation (as u20, which documents each rule): attributes as sets; the top-level
//! sections as a multiset (`pairs(self.savers)`); the children of `Calcs`, `ConfigSet`,
//! `ItemSet`, `Sockets` and `Overrides` as multisets; the Lua `nil` gem/group fields equal to
//! the frozen Rust defaults; the `pairs`-chosen gem variants not compared. The UI-saver
//! sections `TreeView` and `Import` (U25-U28, U43) are compared on their own and a difference is
//! reported, not failed.
//!
//! Compared side effects exclude `ImageLoad` (the Lua tabs' constructors load the tree's
//! images; U23 ports them).

use std::cell::RefCell;
use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::tabs::config::{ctor as cfg, ConfigTab};
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Key, Sg, Sys};
use pob_ui_core::controls::{DropDownControl, DropItem, ScrollBarControl};
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::oracle_cache::{self, LuaRun};
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

// TP2: every (script, screen) runs as an independent case on `oracle_cache::par_map` (at most
// `POB_TEST_PAR` LuaJIT PoBs / Rust apps at once in the whole process, which replaces the old
// one-PoB-at-a-time test lock; the bridge still boots its own PoB per operation under the
// machine-wide oracle lock, or reads it from the Lua-side cache), each case on a fresh thread
// (the Rust side's thread-local state and hooks stay per case), and the Lua side of each case
// goes through the Lua-side cache (`lua_run`). Results are collected in case order, so output
// and failures do not depend on the schedule.

const SCREENS: [(i32, i32, f32); 2] = [(1280, 800, 1.0), (1920, 1080, 1.5)];

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u41")
}

/// lua: ConfigTab.lua:1024-1116 ConfigTab:Draw(viewPort, inputEvents), the test stand-in for
/// U42's port (see the module notes; Undo/Redo, L1032-1037, are not ported here: no script
/// presses Ctrl+Z/Ctrl+Y).
fn config_draw_stand_in(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = cx.build().tabs.config.id();
    // U41's bridge (ctor.rs): the controls take the model's control-table state first
    ConfigTab::pull_var_controls(cx, me);
    {
        // L1025-1028
        let b = cx.base_mut(me);
        b.x = vp.x.into();
        b.y = vp.y.into();
        b.width = vp.width.into();
        b.height = vp.height.into();
    }
    // L1030
    let mut i = 0;
    while let Some(e) = ev.next_ipairs(&mut i) {
        // L1031
        if let EventKind::KeyDown { .. } = e.kind {
            // L1038
            if e.key == Key::Char(b'f') && cx.sys.is_key_down(Key::Ctrl) {
                // L1039
                let search = cx.child(me, "search").unwrap();
                cx.select_control(me, Some(search));
            }
        }
    }
    // L1044
    cx.process_controls_input(me, ev, vp);
    let scroll_bar = cx.child_as::<ScrollBarControl>(me, "scrollBar").id();
    // L1045
    let mut i = 0;
    while let Some(e) = ev.next_ipairs(&mut i) {
        // L1046
        if e.kind == EventKind::KeyUp {
            // L1047
            if ScrollBarControl::is_scroll_down_key(cx, scroll_bar, e.key) {
                // L1048
                ScrollBarControl::scroll(cx, scroll_bar, 1.0);
            // L1049
            } else if ScrollBarControl::is_scroll_up_key(cx, scroll_bar, e.key) {
                // L1050
                ScrollBarControl::scroll(cx, scroll_bar, -1.0);
            }
        }
    }
    // L1055
    let max_col = ((vp.width - 10.0) / 370.0).floor();
    // L1056
    let mut max_col_y: f64 = 0.0;
    // L1057
    let mut col_y: HashMap<i64, f64> = HashMap::from([(1, 0.0)]);
    // L1058
    let sections = cx.w::<ConfigTab>(me).ctor.section_list.clone();
    for section in sections {
        // L1059
        let mut y = 14.0;
        // L1060
        cx.base_mut(section).shown = Prop::Val(true);
        // L1063-1064
        let collapsed = cx.w::<ConfigTab>(me).ctor.sections[&section].collapsed;
        set_collapsed(cx, me, section, false);
        // L1065
        let mut do_show = false;
        // L1066
        let list = cx.w::<ConfigTab>(me).ctor.sections[&section]
            .var_control_list
            .clone();
        for var_control in list {
            // L1067
            if cx.is_shown(var_control) {
                // L1068
                do_show = true;
                // L1069
                let (_w, height) = cx.size(var_control);
                // L1070
                let height = pob_lua::math::m_max(height, 16.0);
                // L1071
                cx.base_mut(var_control).y = (y + 2.0).into();
                // L1072
                y = y + height + 4.0;
            }
        }
        // L1075
        set_collapsed(cx, me, section, collapsed);
        // L1076
        cx.base_mut(section).shown = Prop::Val(do_show);
        // L1077
        if do_show {
            // L1078
            let (_w, height) = cx.size(section);
            // L1079
            let col;
            let sc = cx.w::<ConfigTab>(me).ctor.sections[&section].col;
            // L1080
            if let Some(c) = sc.filter(|&c| {
                col_y.get(&(c as i64)).copied().unwrap_or(0.0) + height + 28.0 <= vp.height
                    && 10.0 + c * 370.0 <= vp.width
            }) {
                // L1081
                col = c as i64;
            } else {
                // L1083
                let mut c0 = 1i64;
                // L1084
                let mut c = 2i64;
                while (c as f64) <= max_col {
                    // L1085
                    let v = *col_y.entry(c).or_insert(0.0);
                    // L1086
                    if v < col_y[&c0] {
                        // L1087
                        c0 = c;
                    }
                    c += 1;
                }
                col = c0;
            }
            // L1091
            col_y.entry(col).or_insert(0.0);
            // L1092
            cx.base_mut(section).x = (10.0 + (col as f64 - 1.0) * 370.0).into();
            // L1093
            cx.base_mut(section).y = (col_y[&col] + 18.0).into();
            // L1094
            let v = col_y[&col] + height + 18.0;
            col_y.insert(col, v);
            // L1095
            max_col_y = pob_lua::math::m_max(max_col_y, v);
        }
    }

    // L1099
    let mut new_set_list: Vec<DropItem> = Vec::new();
    let set_select = cx.child_as::<DropDownControl>(me, "setSelect");
    let model = cx.build().model.clone();
    let order = model.configTab.configSetOrderList.with(Clone::clone);
    // L1100
    for (index, config_set_id) in order.iter().enumerate() {
        // L1101
        let config_set = model
            .configTab
            .configSet(f64::from(*config_set_id))
            .unwrap();
        // L1102
        let title = config_set
            .str(pob_lua::n!("title"))
            .map_or_else(|| "Default".to_owned(), |s| s.to_string());
        new_set_list.push(DropItem::str(title));
        // L1103
        if f64::from(*config_set_id) == model.configTab.activeConfigSetId.get() {
            // L1104
            cx.get_mut(set_select).sel_index = index + 1;
        }
    }
    // L1107
    DropDownControl::set_list(cx, set_select.id(), new_set_list);

    // L1109
    cx.base_mut(scroll_bar).height = vp.height.into();
    // L1110
    ScrollBarControl::set_content_dimension(cx, scroll_bar, max_col_y + 58.0, vp.height);
    // L1111
    let offset = cx.w::<ScrollBarControl>(scroll_bar).offset;
    let anchor = cx.child(me, "sectionAnchor").unwrap();
    cx.base_mut(anchor).y = (20.0 - offset).into();

    // L1113
    pob_app::main_mode::background::draw_background(cx, vp);

    // L1115
    cx.draw_controls(me, vp, None);
}

fn set_collapsed(cx: &mut Cx, me: CtrlId, section: CtrlId, v: bool) {
    cx.w_mut::<ConfigTab>(me)
        .ctor
        .sections
        .get_mut(&section)
        .unwrap()
        .collapsed = v;
}

/// The Lua stand-ins (see the module notes): every tab but Notes and Config draws nothing. Also the
/// oracle's `lua-utf8` stand-in has no `utf8.next` (EditControl.lua:558/:572, LEFT/RIGHT;
/// CCR-0033 item 4): luautf8's `Lutf8_next` for EditControl's call shape, as the u11
/// galleries install it (`lua/gallery/u11_single.lua`).
const LUA_STUBS: &str = r#"
local lutf8 = require("lua-utf8")
if not lutf8.next then
	local function iscont(s, p)
		local b = s:byte(p + 1) or 0
		return b >= 0x80 and b < 0xC0
	end
	lutf8.next = function(s, i, off)
		local e = #s
		local p = i - 1
		local idx = off
		if idx >= 0 then
			while p < e and idx > 0 do
				while p < e and iscont(s, p + 1) do
					p = p + 1
				end
				p = (p < e) and p + 1 or e
				idx = idx - 1
			end
		else
			while 0 < p and idx < 0 do
				local q = p
				while 0 < q and iscont(s, q - 1) do
					q = q - 1
				end
				p = (0 < q) and q - 1 or 0
				idx = idx + 1
			end
		end
		if idx ~= 0 or p == e then
			return
		end
		return p + 1, s:byte(p + 1)
	end
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "partyTab", "treeTab", "skillsTab", "itemsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
end
"#;

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
    /// The stand-in `configTab.modFlag` (UndoHandler.lua:37 via `configTab:AddUndoState`, U42).
    static CONFIG_MOD_FLAG: std::cell::Cell<bool> = const { std::cell::Cell::new(false) };
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    CONFIG_MOD_FLAG.with(|f| f.set(false));
    cfg::set_config_hooks(|h| {
        *h = cfg::default_config_hooks();
        h.add_undo_state = Rc::new(|_, _| CONFIG_MOD_FLAG.with(|f| f.set(true)));
    });
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc, reached from
        // SyncLoadouts' SetSel), as in u20
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
        // LUA_STUBS replaces `treeTab.Draw`: the Tree tab (U25) draws nothing here as well
        let all_tabs = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab != of::BuildTab::Tree {
                all_tabs(cx, tab, vp, ev);
            }
        });
        // keep the default arms (Notes) and add the spec and config stand-ins
        let (flag, set_flag) = (h.tab_flag.clone(), h.set_tab_flag.clone());
        h.tab_flag = Rc::new(move |cx, f| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(std::cell::Cell::get)
            } else if f == TabFlag::Config {
                CONFIG_MOD_FLAG.with(std::cell::Cell::get)
            } else {
                flag(cx, f)
            }
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(|c| c.set(v));
            } else if f == TabFlag::Config {
                CONFIG_MOD_FLAG.with(|c| c.set(v));
            } else {
                set_flag(cx, f, v);
            }
        });
        let draw = h.draw_tab.clone();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| {
            if tab == BuildTab::Config {
                config_draw_stand_in(cx, vp, ev);
            } else {
                draw(cx, tab, vp, ev);
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

// ---- the Lua side ----

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
        self.app
            .with_vm(|pob| pob.eval::<()>(LUA_STUBS))
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
/// does not, CCR-0028 item 2); as in u16-u21.
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
    fn new(mut parts: PlatformParts, src: PathBuf) -> RustSide {
        let effects = Rc::new(RefCell::new(Vec::new()));
        let inner = std::mem::replace(&mut parts.sys.platform, Box::new(NullPlatform));
        parts.sys.platform = Box::new(RecordingPlatform {
            inner,
            effects: effects.clone(),
        });
        let mut app = pob_app::PobApp::with_calc(
            parts,
            Rc::new(move || Box::new(LuaCalcBridge::new(Some(src.clone())))),
        );
        effects.borrow_mut().clear(); // Launch.lua:11 runs at script load, before tracing
                                      // pob-ui-oracle patch 4's equivalent (CCR-0091)
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
        rust_stubs();
        self.app.on_init();
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

// ---- T4 (the rules are u20's; see its module notes) ----

fn unordered_children(elem: &str) -> bool {
    matches!(
        elem,
        "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides"
    )
}

/// The UI-only savers not ported yet: compared separately, a difference is reported.
const PENDING_SAVERS: [&str; 2] = ["Import", "TreeView"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
}

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

fn normalise(node: &XmlNode, top: bool, lua: bool, notes: &mut T4Notes) -> String {
    let mut attrs: Vec<(String, String)> = Vec::new();
    for (k, v) in &node.attrib {
        let rust_default = match (&*node.elem, &**k) {
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => {
                Some("false")
            }
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem"
            && matches!(&**k, "variantId" | "nameSpec")
            && ambiguous_variant(node)
        {
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

fn first_diff(a: &str, b: &str) -> String {
    let (a, b): (Vec<&str>, Vec<&str>) = (a.split('<').collect(), b.split('<').collect());
    for (i, (x, y)) in a.iter().zip(b.iter()).enumerate() {
        if x != y {
            return format!("element {i}: lua <{x} | rust <{y}");
        }
    }
    format!("lengths {} vs {}", a.len(), b.len())
}

/// The raw text of the `<Notes>` element (from `<Notes` to `</Notes>`), for the strict check.
fn notes_text(xml: &str) -> Option<&str> {
    let a = xml.find("<Notes")?;
    let b = xml[a..]
        .find("</Notes>")
        .map(|b| a + b + "</Notes>".len())
        .or_else(|| {
            // an empty element written as `<Notes/>`
            xml[a..].find("/>").map(|b| a + b + 2)
        })?;
    Some(&xml[a..b])
}

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    // Notes: byte-identical
    let (ln, rn) = (notes_text(lua), notes_text(rust));
    if ln.is_none() || ln != rn {
        return Err(format!("{what}: Notes differ: lua {ln:?} rust {rn:?}"));
    }
    let mut local = T4Notes::default();
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|c| match c {
            XmlChild::Node(n) if *n.elem == *elem => {
                Some(normalise(n, false, lua, &mut T4Notes::default()))
            }
            _ => None,
        })
    };
    for elem in PENDING_SAVERS {
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

/// The Lua side of one run through the Lua-side cache (TP2): boot, `script`, the snapshots and
/// the `saved` files of the user dir afterwards (`extra[0]`, JSON), frames without image loads.
/// The key covers this file (stubs and Lua-side wrapper), the Lua-side code, PoB, the fixture user
/// dir, this test's scripts and fixtures, the script, the screen and `saved`.
fn lua_run(
    root: &Path,
    script: &pob_host::script::Script,
    user: &UserDir,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
) -> anyhow::Result<LuaRun> {
    let scripts = test_dir();
    let mut inputs: Vec<&Path> = vec![&scripts];
    inputs.extend(fixture);
    let key = oracle_cache::ui_key(
        "ui-u41",
        pob_ui_oracle::test_source!(),
        root,
        &inputs,
        &[
            &format!("{script:?}"),
            &format!("{screen:?}"),
            &saved.join("\n"),
        ],
    );
    oracle_cache::cached_lua_run(key, || {
        let handles = OracleHandles::default();
        let lua_app = runner::lua_side(root, user, &handles, Entry::Launch, false)?;
        let mut lua_side = LuaSide { app: lua_app };
        let mut d = Driver::new(screen, handles, user.path.clone());
        let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
        drop(lua_side);
        drop_image_loads(&mut frames);
        Ok(LuaRun {
            frames,
            error: r.err().map(|e| e.to_string()),
            snapshots: std::mem::take(&mut d.snapshots),
            extra: vec![serde_json::to_string(&read_saved(user, saved))?],
        })
    })
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    saved: &[String],
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua (cached)
    let run = lua_run(root, script, &user, fixture, screen, saved)?;
    let lua = Side {
        frames: run.frames,
        error: run.error,
        snapshots: run.snapshots,
        saved: serde_json::from_str(run.extra.first().map_or("[]", String::as_str))?,
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src);
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

/// `#! u41-expect-config ATTR...`: some element of a Lua side saved/snapshot XML must hold every
/// whitespace-separated `ATTR` (`name="x"`, `number="1"`; PoB writes attributes in `pairs`
/// order, so they are matched as a set). Guards a script whose input silently stopped reaching
/// a control, which both sides would agree on.
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    for want in directives(text, "u41-expect-config") {
        let attrs: Vec<String> = want
            .split_whitespace()
            .map(|a| a.replace("\\t", "\t").replace("\\n", "\n"))
            .collect();
        let element_has_all = |xml: &str| {
            xml.split('<')
                .any(|el| attrs.iter().all(|a| el.contains(&format!(" {a}"))))
        };
        let found = lua
            .snapshots
            .iter()
            .map(|(_, x)| x.as_str())
            .chain(lua.saved.iter().filter_map(|(_, x)| x.as_deref()))
            .any(element_has_all);
        if !found {
            errs.push(format!("no Lua build XML contains {want:?}"));
        }
    }
    // `#! u41-expect-absent ATTR...`: no element of the Lua side's last snapshot holds them all
    for want in directives(text, "u41-expect-absent") {
        let attrs: Vec<&str> = want.split_whitespace().collect();
        let Some((_, last)) = lua.snapshots.last() else {
            errs.push("u41-expect-absent: no snapshot".to_owned());
            continue;
        };
        if last
            .split('<')
            .any(|el| attrs.iter().all(|a| el.contains(&format!(" {a}"))))
        {
            errs.push(format!("the last Lua snapshot still contains {want:?}"));
        }
    }
    errs
}

/// One case's outcome (collected in case order).
#[derive(Default)]
struct CaseOut {
    line: String,
    failure: Option<String>,
    ran: usize,
    t4_pairs: usize,
    t4_bytes: usize,
}

#[test]
fn u41_scenarios() {
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u41")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u41/*.pobscript");
    let filter = std::env::var("U41_FILTER").ok();
    let mut cases = Vec::new();
    for path in scripts {
        let stem = path.file_stem().unwrap().to_string_lossy().into_owned();
        if filter.as_ref().is_some_and(|f| !stem.contains(f.as_str())) {
            continue;
        }
        let (script, _) = runner::load_script(&path).unwrap();
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
            cases.push((path.clone(), stem.clone(), own_screen, (w, h, s)));
        }
    }
    let results = oracle_cache::par_map(cases, |(path, stem, own_screen, (w, h, s))| {
        let mut out = CaseOut::default();
        let text = std::fs::read_to_string(&path).unwrap();
        let (script, base_dir) = runner::load_script(&path).unwrap();
        let fx = runner::fixtures(&script, &base_dir).unwrap();
        let saved = directives(&text, "u41-saved");
        {
            let tag = if own_screen {
                format!("u41_{stem}")
            } else {
                format!("u41_{stem}@{w}x{h}@{s}")
            };
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &saved,
            ) {
                Ok(s) => s,
                Err(e) => {
                    out.failure = Some(format!("{tag}: {e:#}"));
                    return out;
                }
            };
            out.ran = 1;
            let titles = (window_titles(&lua.frames), window_titles(&rust.frames));
            let mut notes = T4Notes::default();
            let t4r = t4(&lua, &rust, &mut notes);
            let expect = check_expectations(&text, &lua);
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
                    out.t4_pairs += p;
                    out.t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical incl. Notes ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            out.line = format!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error
                    .as_ref()
                    .map_or_else(String::new, |e| format!(", Rust error: {e}")),
                if expect.is_empty() {
                    String::new()
                } else {
                    format!(", expectations: {}", expect.join("; "))
                }
            );
            if n != 0
                || rust.error.is_some()
                || lua.error.is_some()
                || run.lua.len() != run.rust.len()
                || ops == 0
                || titles.0 != titles.1
                || t4r.is_err()
                || !expect.is_empty()
            {
                out.failure = Some(format!(
                    "{tag}: {n} divergences, frames lua {} rust {}, {ops} ops, {t4_line}, titles lua {:?} rust {:?}, errors lua {:?} rust {:?}, expectations {expect:?} (see {})",
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
        out
    });
    let mut failures = Vec::new();
    let (mut ran, mut t4_pairs, mut t4_bytes) = (0, 0, 0);
    for case in results {
        if !case.line.is_empty() {
            println!("{}", case.line);
        }
        failures.extend(case.failure);
        ran += case.ran;
        t4_pairs += case.t4_pairs;
        t4_bytes += case.t4_bytes;
    }
    println!("u41: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    println!("{}", oracle_cache::summary());
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
