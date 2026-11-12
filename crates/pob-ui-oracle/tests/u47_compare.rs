//! U47 acceptance (ui doc §12 U47 row: CompareTab B, CompareTab.lua:1650-2515 and
//! CompareEntry.lua 1-552; "Layout at 3 widths for all compare views"): the full app (Launch,
//! the build shell, the Compare tab) on both sides of the UI oracle, in u22/u46's form.
//! Owner: U47.
//!
//! `u47_scenarios`: every `tests/u47/*.pobscript`; each scenario exists at 1280x800, 1600x900 and
//! 1920x1080 (dpi 1.0: three content widths; `<name>_<W>x<H>.pobscript` with its own `screen`
//! step, because the Compare mode button moves up when the build name fits in the top bar,
//! Build.lua:437-440). Each script opens `allflame_exp_001` (u46's fixture user dir), presses
//! the Compare button and drives the sub-views. Per run: zero T3 divergences (draw ops per
//! `(layer, sub)` bucket and side effects, frame by frame), the same window titles (the
//! comparison build's own TreeTab titles included), and T4 at every `snapshot` step (u46's rules).
//!
//! What runs on each side:
//! * `CompareTab:Draw` and its layout passes (LayoutTreeView, LayoutConfigView,
//!   UpdateSetSelectors, RefreshCalcsSkillControls, LayoutCalcsSkillControls, HandleScrollInput,
//!   DrawControlList): PoB's (Lua) and U47's (Rust), with U46's constructor and methods;
//! * `CompareEntry` (constructor, LoadFromXML, LoadBuildSection, SyncCalcsSkillSelection,
//!   Rebuild, RefreshSkillSelectControls, ...): PoB's and U47's. The Rust entry's calc is U15's
//!   LuaCalcBridge (as the primary build's);
//! * the comparison build is imported on the tab's first `Draw` by `self:ImportBuild(xml,
//!   label)` from the file of the script's `#! u47-import FILE` directive (both sides; no
//!   popup), so the scripts are the same at every width;
//! * the sub-view passes (`DrawTree`, `DrawSummary`, `DrawItems`, `DrawSkills`, `DrawCalcs`,
//!   `DrawConfig`: U49/U50) are stand-ins on both sides that print the layout state this unit
//!   computes and the entry's state ([`LUA_STUBS`], [`stub_hooks`]): `treeLayout`,
//!   `configSectionLayout`, `calcsSkillHeaderHeight`, the selectors RefreshSkillSelectControls
//!   filled, the entry's level/main group/specs/notes and its Life/TotalDPS;
//! * `ComparePowerReportListControl` (U48) draws nothing on both sides; every other tab's `Draw`
//!   draws nothing (as u22), plus u22's `utf8.next`.

#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::tabs::compare::entry as ce;
use pob_app::tabs::compare::init as ci;
use pob_app::tabs::compare::layout::{self as cl, CompareDrawHooks};
use pob_app::tabs::compare::CompareTab;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_lua::lfmt;
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Align, Font, Key, Sg, Sys};
use pob_ui_core::controls::{CheckBoxControl, DropDownControl, EditControl};
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

const SCREENS: [(i32, i32, f32); 3] = [(1280, 800, 1.0), (1600, 900, 1.0), (1920, 1080, 1.0)];

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u47")
}

/// The Lua stand-ins (see the module notes).
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
local function N(v)
	if type(v) == "number" then return string.format("%.4f", v) end
	return tostring(v)
end
local function SH(c)
	if type(c.shown) == "function" then return "fn" end
	return tostring(c.shown)
end
local function DD(c)
	local first = c.list[1]
	if type(first) == "table" then first = first.label end
	return N(c.selIndex) .. "/" .. #c.list .. ":" .. tostring(first)
end
local function lines(vp, list)
	SetDrawColor(1, 1, 1)
	local y = vp.y + 240
	for _, s in ipairs(list) do
		DrawString(vp.x + 10, y, "LEFT", 14, "VAR", s)
		y = y + 16
	end
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "skillsTab", "itemsTab", "calcsTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	local ct = self.compareTab
	if not ct then return end
	local cls = common.classes.CompareTab
	ct.controls.comparePowerReportList.Draw = function() end
	function ct:Draw(viewPort, inputEvents)
		if U47_XML and not self.u47Imported then
			self.u47Imported = true
			self:ImportBuild(U47_XML, U47_LABEL)
		end
		return cls.Draw(self, viewPort, inputEvents)
	end
	function ct:DrawSummary(vp, entry)
		local c = self.controls
		local out = entry.calcsTab.mainOutput
		lines(vp, {
			"u47 SUMMARY " .. entry.label .. " lvl=" .. N(entry.characterLevel) .. " msg=" .. N(entry.mainSocketGroup)
				.. " specs=" .. #entry.treeTab.specList .. " active=" .. N(entry.treeTab.activeSpec)
				.. " notes=" .. #entry.notesText .. " life=" .. N(out.Life) .. " dps=" .. N(out.TotalDPS)
				.. " scrollY=" .. N(self.scrollY),
			"cmp sg=" .. DD(c.cmpSocketGroup) .. " ms=" .. SH(c.cmpMainSkill) .. ":" .. DD(c.cmpMainSkill)
				.. " part=" .. SH(c.cmpSkillPart) .. " stage=" .. SH(c.cmpStageCount) .. ":" .. c.cmpStageCount.buf
				.. " mine=" .. SH(c.cmpMineCount) .. ":" .. c.cmpMineCount.buf .. " minion=" .. SH(c.cmpMinion)
				.. " mskill=" .. SH(c.cmpMinionSkill),
			"sets spec=" .. DD(c.compareSpecSelect) .. " skill=" .. DD(c.compareSkillSetSelect)
				.. " item=" .. DD(c.compareItemSetSelect) .. " cfg=" .. DD(c.compareConfigSetSelect),
		})
	end
	function ct:DrawTree(vp, inputEvents, entry)
		local c = self.controls
		-- nil on the frame a sub-tab click switches to TREE (LayoutTreeView ran before the click)
		local l = self.treeLayout or {}
		lines(vp, {
			"u47 TREE ov=" .. tostring(l.overlay) .. " hh=" .. N(l.headerHeight) .. " fh=" .. N(l.footerHeight)
				.. " fy=" .. N(l.footerY) .. " hw=" .. N(l.halfWidth) .. " rx=" .. N(l.rightAbsX),
			"ls=" .. DD(c.leftSpecSelect) .. " rs=" .. DD(c.rightSpecSelect) .. " lv=" .. N(c.leftVersionSelect.selIndex)
				.. " rv=" .. N(c.rightVersionSelect.selIndex) .. " lt=" .. c.leftTreeSearch.buf .. " rt=" .. c.rightTreeSearch.buf
				.. " ot=" .. c.overlayTreeSearch.buf .. " vs=" .. entry.treeTab.viewer.searchStr,
		})
	end
	function ct:DrawItems(vp, entry, inputEvents)
		local c = self.controls
		lines(vp, {
			"u47 ITEMS col=" .. N(self.itemsColWidth) .. " p=" .. DD(c.primaryItemSetSelect) .. " c=" .. DD(c.compareItemSetSelect2)
				.. " scrollY=" .. N(self.scrollY),
		})
	end
	function ct:DrawSkills(vp, entry)
		lines(vp, { "u47 SKILLS scrollY=" .. N(self.scrollY) })
	end
	function ct:DrawCalcs(vp, entry)
		local c = self.controls
		lines(vp, {
			"u47 CALCS hdr=" .. N(self.calcsSkillHeaderHeight) .. " psg=" .. DD(c.primCalcsSocketGroup) .. " csg=" .. DD(c.cmpCalcsSocketGroup)
				.. " pms=" .. DD(c.primCalcsMainSkill) .. " cms=" .. DD(c.cmpCalcsMainSkill)
				.. " pmode=" .. N(c.primCalcsMode.selIndex) .. " cmode=" .. N(c.cmpCalcsMode.selIndex)
				.. " pmin=" .. tostring(c.primCalcsShowMinion.state or false) .. " cmin=" .. tostring(c.cmpCalcsShowMinion.state or false),
		})
	end
	function ct:DrawConfig(vp, entry, headerOnly)
		if headerOnly then return end
		local out = { "u47 CONFIG total=" .. N(self.configTotalContentHeight) .. " scrollY=" .. N(self.scrollY)
			.. " n=" .. #self.configSectionLayout .. " ctrls=" .. #self.configControlList }
		for _, sec in ipairs(self.configSectionLayout) do
			table.insert(out, sec.name .. " " .. N(sec.x) .. "," .. N(sec.y) .. " col=" .. N(sec.col) .. " rows=" .. #sec.rows
				.. " diff=" .. sec.diffCount .. " h=" .. N(sec.height))
		end
		lines(vp, out)
	end
end
"#;

// ---- the Rust stand-ins ----

thread_local! {
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25), as u46.
    static SPEC_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
    /// `#! u47-import`: (xml, label)
    static IMPORT: RefCell<Option<(String, String)>> = const { RefCell::new(None) };
    /// `self.u47Imported`
    static IMPORTED: Cell<bool> = const { Cell::new(false) };
}

/// The Lua stub's `N(v)`.
fn N(v: Option<f64>) -> String {
    v.map_or_else(|| "nil".to_owned(), |v| lfmt!("%.4f", v))
}

fn Nv(v: &pob_app::model::Val) -> String {
    match v {
        pob_app::model::Val::Num(n) => N(Some(*n)),
        pob_app::model::Val::Nil => "nil".to_owned(),
        pob_app::model::Val::Str(s) => s.to_string(),
        pob_app::model::Val::Bool(b) => b.to_string(),
        other => format!("<{}>", other.lua_type()),
    }
}

/// The Lua stub's `SH(c)`.
fn SH(cx: &Cx, id: CtrlId) -> String {
    match &cx.base(id).shown {
        Prop::Fn(_) => "fn".to_owned(),
        Prop::Val(v) => v.to_string(),
    }
}

/// The Lua stub's `DD(c)`.
fn DD(cx: &Cx, id: CtrlId) -> String {
    let w = cx.w::<DropDownControl>(id);
    let first = w.list.first().map_or_else(|| "nil".to_owned(), |i| i.label.clone());
    format!("{}/{}:{first}", N(Some(w.sel_index as f64)), w.list.len())
}

fn c(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key).unwrap_or_else(|| panic!("controls.{key}"))
}

fn buf(cx: &Cx, id: CtrlId) -> String {
    cx.w::<EditControl>(id).buf.clone()
}

/// The Lua stub's `lines(vp, list)`.
fn lines(cx: &mut Cx, vp: &ViewPort, list: &[String]) {
    cx.sg.set_draw_color(1.0, 1.0, 1.0);
    let mut y = vp.y + 240.0;
    for s in list {
        cx.sg.draw_string(vp.x + 10.0, y, Align::Left, 14.0, Font::Var, s);
        y += 16.0;
    }
}

/// The Lua stub's Draw* passes (statement for statement).
fn stub_hooks() -> CompareDrawHooks {
    CompareDrawHooks {
        draw_summary: Rc::new(|cx, me, vp, entry| {
            let m = &entry.model;
            let out = m.calcsTab.mainOutput.get().expect("mainOutput");
            let e = ce::ext(entry);
            let l1 = format!(
                "u47 SUMMARY {} lvl={} msg={} specs={} active={} notes={} life={} dps={} scrollY={}",
                entry.label,
                N(Some(m.characterLevel.get())),
                N(Some(m.mainSocketGroup.get())),
                m.treeTab.specList.len(),
                N(Some(m.treeTab.activeSpec.get())),
                e.notesText.len(),
                Nv(&out.get(pob_lua::Name::from("Life"))),
                Nv(&out.get(pob_lua::Name::from("TotalDPS"))),
                N(Some(ci::st(cx, me).scrollY)),
            );
            drop(e);
            let k = |key| c(cx, me, key);
            let l2 = format!(
                "cmp sg={} ms={}:{} part={} stage={}:{} mine={}:{} minion={} mskill={}",
                DD(cx, k("cmpSocketGroup")),
                SH(cx, k("cmpMainSkill")),
                DD(cx, k("cmpMainSkill")),
                SH(cx, k("cmpSkillPart")),
                SH(cx, k("cmpStageCount")),
                buf(cx, k("cmpStageCount")),
                SH(cx, k("cmpMineCount")),
                buf(cx, k("cmpMineCount")),
                SH(cx, k("cmpMinion")),
                SH(cx, k("cmpMinionSkill")),
            );
            let l3 = format!(
                "sets spec={} skill={} item={} cfg={}",
                DD(cx, k("compareSpecSelect")),
                DD(cx, k("compareSkillSetSelect")),
                DD(cx, k("compareItemSetSelect")),
                DD(cx, k("compareConfigSetSelect")),
            );
            lines(cx, vp, &[l1, l2, l3]);
        }),
        draw_tree: Rc::new(|cx, me, vp, _ev, entry| {
            let l = cx.w::<CompareTab>(me).layout.treeLayout;
            let k = |key| c(cx, me, key);
            let l1 = format!(
                "u47 TREE ov={} hh={} fh={} fy={} hw={} rx={}",
                l.map_or_else(|| "nil".to_owned(), |l| l.overlay.to_string()),
                N(l.map(|l| l.headerHeight)),
                N(l.map(|l| l.footerHeight)),
                N(l.map(|l| l.footerY)),
                N(l.and_then(|l| l.halfWidth)),
                N(l.and_then(|l| l.rightAbsX))
            );
            let vs = ce::ext(entry).viewer.borrow().search_str.clone();
            let l2 = format!(
                "ls={} rs={} lv={} rv={} lt={} rt={} ot={} vs={vs}",
                DD(cx, k("leftSpecSelect")),
                DD(cx, k("rightSpecSelect")),
                N(Some(cx.w::<DropDownControl>(k("leftVersionSelect")).sel_index as f64)),
                N(Some(cx.w::<DropDownControl>(k("rightVersionSelect")).sel_index as f64)),
                buf(cx, k("leftTreeSearch")),
                buf(cx, k("rightTreeSearch")),
                buf(cx, k("overlayTreeSearch")),
            );
            lines(cx, vp, &[l1, l2]);
        }),
        draw_items: Rc::new(|cx, me, vp, _entry, _ev| {
            let k = |key| c(cx, me, key);
            let l1 = format!(
                "u47 ITEMS col={} p={} c={} scrollY={}",
                N(cx.w::<CompareTab>(me).layout.itemsColWidth),
                DD(cx, k("primaryItemSetSelect")),
                DD(cx, k("compareItemSetSelect2")),
                N(Some(ci::st(cx, me).scrollY)),
            );
            lines(cx, vp, &[l1]);
        }),
        draw_skills: Rc::new(|cx, me, vp, _entry| {
            let l1 = format!("u47 SKILLS scrollY={}", N(Some(ci::st(cx, me).scrollY)));
            lines(cx, vp, &[l1]);
        }),
        draw_calcs: Rc::new(|cx, me, vp, _entry| {
            let k = |key| c(cx, me, key);
            let l1 = format!(
                "u47 CALCS hdr={} psg={} csg={} pms={} cms={} pmode={} cmode={} pmin={} cmin={}",
                N(Some(cx.w::<CompareTab>(me).layout.calcsSkillHeaderHeight)),
                DD(cx, k("primCalcsSocketGroup")),
                DD(cx, k("cmpCalcsSocketGroup")),
                DD(cx, k("primCalcsMainSkill")),
                DD(cx, k("cmpCalcsMainSkill")),
                N(Some(cx.w::<DropDownControl>(k("primCalcsMode")).sel_index as f64)),
                N(Some(cx.w::<DropDownControl>(k("cmpCalcsMode")).sel_index as f64)),
                cx.w::<CheckBoxControl>(k("primCalcsShowMinion")).state,
                cx.w::<CheckBoxControl>(k("cmpCalcsShowMinion")).state,
            );
            lines(cx, vp, &[l1]);
        }),
        draw_config: Rc::new(|cx, me, vp, _entry, headerOnly| {
            if headerOnly {
                return;
            }
            let s = ci::st(cx, me);
            let layout = &cx.w::<CompareTab>(me).layout;
            let mut out = vec![format!(
                "u47 CONFIG total={} scrollY={} n={} ctrls={}",
                N(Some(s.configTotalContentHeight)),
                N(Some(s.scrollY)),
                layout.configSectionLayout.len(),
                s.configControlList.len()
            )];
            for sec in &layout.configSectionLayout {
                out.push(format!(
                    "{} {},{} col={} rows={} diff={} h={}",
                    sec.name,
                    N(Some(sec.x)),
                    N(Some(sec.y)),
                    N(sec.col),
                    sec.rows.len(),
                    sec.diffCount,
                    N(Some(sec.height))
                ));
            }
            lines(cx, vp, &out);
        }),
    }
}

/// The stub's `ct:Draw`: the import on the first Draw, then the real Draw.
fn draw_compare(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = ci::tab(cx).expect("compareTab");
    if !IMPORTED.with(Cell::get) {
        if let Some((xml, label)) = IMPORT.with(|i| i.borrow().clone()) {
            IMPORTED.with(|i| i.set(true));
            CompareTab::ImportBuild(cx, me, Some(&xml), Some(&label));
        }
    }
    cl::draw_tab(cx, vp, ev);
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    IMPORTED.with(|i| i.set(false));
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
    });
    ci::set_compare_hooks(|h| {
        *h = ci::default_compare_hooks();
        // the power report list draws nothing (as the Lua stub's); U48's own scenarios are u48
        h.power_report_list = Rc::new(|cx, me| {
            cx.add(
                me,
                "comparePowerReportList",
                New::new(Base::new(Anchor::NONE, rect(0, 0, 750, 250)), ci::PendingPowerReportList),
            )
            .id()
        });
    });
    cl::set_draw_hooks(|h| *h = stub_hooks());
    of::set_shell_hooks(|h| {
        *h = of::default_shell_hooks();
        h.draw_tab = Rc::new(move |cx, tab, vp, ev| match tab {
            BuildTab::Compare => draw_compare(cx, vp, ev),
            // LUA_STUBS replaces the other tabs' Draw
            BuildTab::Notes
            | BuildTab::Party
            | BuildTab::Tree
            | BuildTab::Skills
            | BuildTab::Items
            | BuildTab::Calcs
            | BuildTab::Config
            | BuildTab::Import => {}
        });
        let (flag, set_flag) = (h.tab_flag.clone(), h.set_tab_flag.clone());
        h.tab_flag = Rc::new(move |cx, f| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(Cell::get)
            } else {
                flag(cx, f)
            }
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| {
            if f == TabFlag::Spec {
                SPEC_MOD_FLAG.with(|c| c.set(v));
            } else {
                set_flag(cx, f, v);
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

/// `#! u47-import FILE` (relative to the script's user dir `Path of Building/Builds`): the XML
/// and the label (the file stem).
fn import_of(text: &str, user_dir: Option<&Path>) -> Option<(String, String)> {
    let f = directives(text, "u47-import").into_iter().next()?;
    let p = user_dir?.join("Path of Building/Builds").join(&f);
    let xml = std::fs::read_to_string(&p).unwrap_or_else(|e| panic!("{}: {e}", p.display()));
    let label = Path::new(&f).file_stem().unwrap().to_string_lossy().into_owned();
    Some((xml, label))
}

// ---- the Lua side ----

struct LuaSide {
    app: LuaApp,
    import: Option<(String, String)>,
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
            .with_vm(|pob| {
                if let Some((xml, label)) = &self.import {
                    pob.lua.globals().set("U47_XML", xml.as_str())?;
                    pob.lua.globals().set("U47_LABEL", label.as_str())?;
                }
                pob.eval::<()>(LUA_STUBS)
            })
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

/// Records the platform side effects the Lua side's bridge records (as in u16-u45).
struct RecordingPlatform {
    inner: Box<dyn pob_sg::Platform>,
    effects: Rc<RefCell<Vec<SideEffect>>>,
}

impl pob_sg::Platform for RecordingPlatform {
    fn time_ms(&self) -> i64 {
        self.inner.time_ms()
    }
    fn copy(&mut self, text: &str) {
        self.effects.borrow_mut().push(SideEffect::Copy(text.to_owned()));
        self.inner.copy(text);
    }
    fn paste(&mut self) -> Option<String> {
        self.inner.paste()
    }
    fn open_url(&mut self, url: &str) {
        self.effects.borrow_mut().push(SideEffect::OpenUrl(url.to_owned()));
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
    import: Option<(String, String)>,
}

impl RustSide {
    fn new(mut parts: PlatformParts, src: PathBuf, import: Option<(String, String)>) -> RustSide {
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
        RustSide { app, effects, import }
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
        IMPORT.with(|i| *i.borrow_mut() = self.import.clone());
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
    matches!(elem, "Calcs" | "ConfigSet" | "ItemSet" | "Sockets" | "Overrides")
}

/// The UI-only savers compared separately (a difference is reported, not failed).
const PENDING_SAVERS: [&str; 2] = ["Import", "TreeView"];

#[derive(Default)]
struct T4Notes {
    pending_sections: Vec<String>,
    gem_nil_bools: usize,
    ambiguous_variants: usize,
}

fn ambiguous_variant(node: &XmlNode) -> bool {
    let (Some(game_id), Some(skill_id)) = (node.attrib.get("gemId"), node.attrib.get("skillId")) else {
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
            ("Gem", "enableGlobal1" | "enableGlobal2") | ("Skill", "includeInFullDPS") => Some("false"),
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs") => Some("0"),
            _ => None,
        };
        if let Some(d) = rust_default.filter(|_| &**v == "nil") {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), d.to_owned()));
        } else if &*node.elem == "Gem" && matches!(&**k, "variantId" | "nameSpec") && ambiguous_variant(node) {
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
    for ch in &node.children {
        match ch {
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
        .find_map(|ch| match ch {
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

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
    let mut local = T4Notes::default();
    let section = |root: &XmlNode, elem: &str, lua: bool| {
        root.children.iter().find_map(|ch| match ch {
            XmlChild::Node(n) if *n.elem == *elem => Some(normalise(n, false, lua, &mut T4Notes::default())),
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
}

/// Run `script` on both sides with `fixture` as the user dir (fresh copies per side). The Lua
/// VM is dropped before the Rust side starts (memory).
fn run_sides(
    root: &Path,
    script: &pob_host::script::Script,
    fixture: Option<&Path>,
    screen: pob_sg::ScreenInfo,
    import: Option<(String, String)>,
) -> anyhow::Result<(Side, Side)> {
    let user = UserDir::new(fixture)?;

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let mut lua_side = LuaSide {
        app: lua_app,
        import: import.clone(),
    };
    let mut d = Driver::new(screen, handles, user.path.clone());
    let (mut frames, r) = runner::drive(&mut lua_side, script, &mut d);
    drop(lua_side);
    drop_image_loads(&mut frames);
    let lua = Side {
        frames,
        error: r.err().map(|e| e.to_string()),
        snapshots: std::mem::take(&mut d.snapshots),
    };

    // Rust
    user.reset()?;
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(root, &user.path, &handles)?;
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust_side = RustSide::new(PlatformParts { sg, sys }, src, import);
    let mut d = Driver::new(screen, handles, user.path.clone());
    let res = std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| runner::drive(&mut rust_side, script, &mut d)));
    let (mut frames, error) = match res {
        Ok((frames, r)) => (frames, r.err().map(|e| e.to_string())),
        Err(p) => (Vec::new(), Some(format!("panicked: {}", pob_host::panic_message(&*p)))),
    };
    drop(rust_side);
    drop_image_loads(&mut frames);
    let rust = Side {
        frames,
        error,
        snapshots: std::mem::take(&mut d.snapshots),
    };
    Ok((lua, rust))
}

fn out_dir() -> PathBuf {
    std::env::var_os("CARGO_TARGET_DIR")
        .map_or_else(|| PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../../target"), PathBuf::from)
        .join("ui-parity")
}

fn draw_ops(frames: &[FrameTrace]) -> usize {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter(|c| matches!(c.call, ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } | ApiCall::DrawString { .. }))
        .count()
}

/// The texts of a run's DrawString calls (for `#! u47-expect-text`).
fn draw_texts(frames: &[FrameTrace]) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter_map(|c| match &c.call {
            ApiCall::DrawString { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect()
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
        errs.push(format!("snapshots: lua {} rust {}", lua.snapshots.len(), rust.snapshots.len()));
    }
    for ((ln, lx), (_, rx)) in lua.snapshots.iter().zip(&rust.snapshots) {
        n += 1;
        match compare_t4(&format!("snapshot {ln}"), lx, rx, notes) {
            Ok(b) => same += usize::from(b),
            Err(e) => errs.push(e),
        }
    }
    if errs.is_empty() {
        Ok((n, same))
    } else {
        Err(errs)
    }
}

/// `#! u47-expect-text TEXT`: some Lua DrawString contains TEXT (guards a script whose clicks
/// silently stopped reaching the tab, which both sides would agree on); `#! u47-expect-snapshot
/// NAME TEXT`: the Lua snapshot NAME contains TEXT.
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    let mut errs = Vec::new();
    let texts = draw_texts(&lua.frames);
    for want in directives(text, "u47-expect-text") {
        if !texts.iter().any(|t| t.contains(want.as_str())) {
            errs.push(format!("no Lua DrawString contains {want:?}"));
        }
    }
    for d in directives(text, "u47-expect-snapshot") {
        let (name, want) = d.split_once(' ').unwrap_or((d.as_str(), ""));
        let found = lua.snapshots.iter().any(|(n, x)| n == name && x.contains(want));
        if !found {
            errs.push(format!("Lua snapshot {name} does not contain {want:?}"));
        }
    }
    errs
}

#[test]
fn u47_scenarios() {
    let _g = LOCK.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u47")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u47/*.pobscript");
    let filter = std::env::var("U47_FILTER").ok();
    let only_screen = std::env::var("U47_SCREEN").ok();
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
        let own_screen = matches!(script.steps.first(), Some(pob_host::script::Step::Screen { .. }));
        let screens: Vec<(i32, i32, f32)> = if own_screen {
            vec![(1280, 800, 1.0)]
        } else {
            SCREENS.to_vec()
        };
        for (w, h, s) in screens {
            if only_screen.as_ref().is_some_and(|o| *o != w.to_string()) {
                continue;
            }
            let tag = if own_screen {
                format!("u47_{stem}")
            } else {
                format!("u47_{stem}@{w}x{h}@{s}")
            };
            let import = import_of(&text, fx.user_dir.as_deref());
            let (lua, rust) = match run_sides(&root, &script, fx.user_dir.as_deref(), runner::screen(w, h, s), import) {
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
                    t4_pairs += p;
                    t4_bytes += b;
                    let mut pending = notes.pending_sections.clone();
                    pending.sort();
                    pending.dedup();
                    format!(
                        "T4 {p} identical ({b} byte-identical; set aside: {} Lua nil gem/group fields, {} pairs-chosen gem variants, pending UI-saver sections differing {:?})",
                        notes.gem_nil_bools, notes.ambiguous_variants, pending
                    )
                }
                Err(e) => format!("T4 FAILED: {}", e.join("; ")),
            };
            println!(
                "{tag}: {} frames, {ops} Lua draw ops, {n} divergences, {t4_line}, titles {:?}{}{}{}",
                run.lua.len(),
                distinct_titles(&titles.0),
                lua.error.as_ref().map_or_else(String::new, |e| format!(", Lua error: {e}")),
                rust.error.as_ref().map_or_else(String::new, |e| format!(", Rust error: {e}")),
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
                failures.push(format!(
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
    }
    println!("u47: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}
