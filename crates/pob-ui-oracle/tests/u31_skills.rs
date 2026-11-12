//! U31 acceptance (ui doc §12 U31 row: gem slot rows, the rebuilt controls removed correctly;
//! list drag/reorder; minion lists): the full app (Launch, the build shell, the Skills tab's U31
//! controls) on both sides of the UI oracle, in u22's form (the wave-4 template), as u32. Owner:
//! U31.
//!
//! `u31_scenarios`: every `tests/u31/*.pobscript` at 1280x800 @1.0 and 1920x1080 @1.5 (both 1280
//! virtual px wide). Each script loads a fixture build, switches to the Skills tab (Ctrl+2) and
//! drives the U31 controls. Per run: zero T3 divergences (draw ops per `(layer, sub)` bucket and
//! side effects, frame by frame), the same window titles, and T4: every `snapshot` step's build
//! XML identical between the sides after u22's normalisation (attributes as sets, the top-level
//! sections and the `Calcs`/`ConfigSet`/`ItemSet`/`Sockets`/`Overrides` children as multisets,
//! the pending UI-saver sections reported, not failed). The Rust side's calc is U15's
//! LuaCalcBridge (as u20-u22, u32), so the gem tooltips' `calcFunc` calls give PoB's numbers.
//!
//! The SkillsTab UI (U30) is not merged, so both sides run the same **stand-in SkillsTab**:
//! * the controls are the real ones on the Lua side (built by SkillsTab.lua's constructor, Load
//!   and UpdateGemSlots) and U31's on the Rust side: `controls.groupList` (SkillListControl at its
//!   Lua rect), the gem slot rows of CreateGemSlot (every `gemSlot<i>*` control), with
//!   `anchorGemSlots` re-anchored to the tab at (20, 400) and shown while `displayGroup` is set
//!   (the Lua anchor chain through `anchorGroupDetail` is U30's), and two stand-in buttons:
//!   "Manage sets" (SkillsTab:OpenSkillSetManagePopup, L1442-1450, U30's, ported below) and
//!   "Spectres" (`build:OpenSpectreLibrary()`, U20's, real on both sides: it constructs U31's
//!   MinionListControl / MinionSearchListControl);
//! * `Draw` is the stand-in below (both sides): the tab's rect, `ProcessControlsInput` and
//!   X
//!   through to `self.controls`), `main:DrawBackground`, `UpdateGemSlots`, and one status line
//!   (`display: <displayGroup.displayLabel> main: <mainSocketGroup> undo: <count> set:
//!   <activeSkillSetId> spectres: <#spectreList>`);
//! * `#! u31-fake-calc` (scripts that focus a gem selector): `calcsTab:GetMiscCalculator()`
//!   returns a stand-in `calcFunc` on both sides (u29's form: the base output with `CombinedDPS`
//!   moved by the display group's gems, see [`FakeMisc`]); PoB's GemSelectControl computes the
//!   DPS of every supporting gem when it gains focus (GemSelectControl.lua:362-371), one bridge
//!   calc each. `build_output` (the recalcs) stays the bridge's;
//! * `sortGemsByDPS` is false on both sides (set after Load): the gem selectors sort by name, as
//!   one bridge calc per gem for the DPS sort would take hours here (the DPS sort is u29's);
//! * Lua stand-ins on both sides: `AddSocketGroupTooltip` (U30: two lines naming the group and
//!   its gem count);
//! * Lua real, Rust ported here (faithful copies of U30's Lua, test-only): `UpdateGemSlots`
//!   (L1081-1100), `SetDisplayGroup` (L1275-1308; of the UI tail only the gem rows and
//!   `groupLabel`, the other controls are not in the stand-in), the UI half of
//!   `SetActiveSkillSet` (L1495, L1500, L1501), `CopySocketGroup` (L694-706),
//!   `OpenSkillSetManagePopup`; `AddUndoState` is the real UndoHandler on the Lua side and sets
//!   `modFlag` (and counts) on the Rust side. The Rust `controls.groupLabel` (the tab-group master
//!   of the gem edits) is a hidden stand-in EditControl (no script presses TAB).
//! * every other tab's `Draw` draws nothing (as u22), and u22's `utf8.next` stand-in.
//!
//! `u31_rebuilt_slot_controls`: CreateGemSlot run again for the first and the last row replaces
//! the row's nine `controls` keys; the old controls leave the tab's controls (not drawn, not
//! hit-tested) and are freed at the end of the frame, except the three the next row still
//! references in Lua (its anchor and `y`), which stay alive detached; the frame draws the same
//! strings as before.

#![allow(
    clippy::format_push_string,
    clippy::too_many_lines,
    clippy::case_sensitive_file_extension_comparisons
)]

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use pob_app::app::CxApp;
use pob_app::build::init;
use pob_app::build::on_frame::{self as of, BuildTab, TabFlag};
use pob_app::model::{self, SocketGroupRef};
use pob_app::tabs::skills::gem_slots::{self as gs, CreateGemSlot, GemSlot};
use pob_app::tabs::skills::skill_list::{skill_list_ctor, SkillListControl};
use pob_app::tabs::skills::skill_set_list::skill_set_list_ctor;
use pob_calc::model::{XmlChild, XmlNode};
use pob_host::{App, PlatformParts};
use pob_lua::n;
use pob_sg::trace::{FrameTrace, SideEffect};
use pob_sg::{Align, Font, Key, Sg, Sys};
use pob_ui_core::controls::{ButtonControl, CheckBoxControl, EditArgs, EditControl};
use pob_ui_core::popups;
use pob_ui_core::prelude::*;
use pob_ui_oracle::boot::{self, Entry, OracleHandles};
use pob_ui_oracle::diff_report;
use pob_ui_oracle::lua_calc::LuaCalcBridge;
use pob_ui_oracle::runner::{self, Driver, LuaApp, RunResult, ScenarioApp, UserDir};

/// One LuaJIT PoB at a time (memory; ui doc §11.3).
static LOCK: std::sync::Mutex<()> = std::sync::Mutex::new(());

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
    Path::new(env!("CARGO_MANIFEST_DIR")).join("tests/u31")
}

// ---- the stand-in layout (tab-relative; see the module notes) ----

/// `anchorGemSlots` (re-anchored)
const GEM_SLOTS: [f64; 2] = [20.0, 400.0];
const MANAGE: [f64; 4] = [400.0, 8.0, 110.0, 20.0];
const SPECTRES: [f64; 4] = [520.0, 8.0, 110.0, 20.0];

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
local function u31Keep(k)
	return k == "groupList" or k == "u31Sets" or k == "u31Spectres" or (type(k) == "string" and k:match("^gemSlot%d") ~= nil)
end
local function u31Filter(saved)
	local t = { }
	for k, ctl in pairs(saved) do
		if u31Keep(k) then t[k] = ctl end
	end
	return setmetatable(t, { __index = saved, __newindex = function(tt, k, v)
		saved[k] = v
		if u31Keep(k) then rawset(tt, k, v) end
	end })
end
local b = main.modes.BUILD
local origInit = b.Init
function b:Init(...)
	origInit(self, ...)
	for _, t in ipairs({ "importTab", "notesTab", "partyTab", "configTab", "treeTab", "itemsTab", "calcsTab", "compareTab" }) do
		if self[t] then self[t].Draw = function() end end
	end
	local st = self.skillsTab
	if not st then return end
	local build = self
	st.sortGemsByDPS = false
	st.anchorGemSlots:SetAnchor("TOPLEFT", st, "TOPLEFT", U31_GEM_SLOTS[1], U31_GEM_SLOTS[2])
	st.anchorGemSlots.shown = function() return st.displayGroup ~= nil end
	st.controls.u31Sets = new("ButtonControl"):ButtonControl({"TOPLEFT", st, "TOPLEFT"}, U31_MANAGE, "Manage sets", function()
		st:OpenSkillSetManagePopup()
	end)
	st.controls.u31Spectres = new("ButtonControl"):ButtonControl({"TOPLEFT", st, "TOPLEFT"}, U31_SPECTRES, "Spectres", function()
		build:OpenSpectreLibrary()
	end)
	st.u31Undo = 0
	local origUndo = st.AddUndoState
	st.AddUndoState = function(s, ...)
		s.u31Undo = s.u31Undo + 1
		return origUndo(s, ...)
	end
	if U31_FAKE then
		local calcsTab = self.calcsTab
		local function fakeCalc(override, useFullDPS)
			local base = calcsTab.miscCalculator[2]
			local out = copyTable(base, true)
			local add = 0
			local group = st.displayGroup
			if group then
				for _, g in ipairs(group.gemList) do
					add = add + (g.quality or 0) * 7 + (g.enabled and 1000 or 0) + #(g.gemData and g.gemData.name or "")
				end
			end
			out.CombinedDPS = (base.CombinedDPS or 0) + add
			return out
		end
		function calcsTab:GetMiscCalculator() return fakeCalc, self.miscCalculator[2] end
	end
	st.AddSocketGroupTooltip = function(s, tooltip, group)
		tooltip:AddLine(16, "^7group " .. tostring(group.displayLabel))
		tooltip:AddLine(14, "^7gems " .. #group.gemList)
	end
	st.Draw = function(s, viewPort, inputEvents)
		s.x = viewPort.x
		s.y = viewPort.y
		s.width = viewPort.width
		s.height = viewPort.height
		local saved = s.controls
		s.controls = u31Filter(saved)
		s:ProcessControlsInput(inputEvents, viewPort)
		s.controls = saved
		main:DrawBackground(viewPort)
		s:UpdateGemSlots()
		s.controls = u31Filter(saved)
		s:DrawControls(viewPort)
		s.controls = saved
		DrawString(viewPort.x + 4, viewPort.y + viewPort.height - 20, "LEFT", 16, "VAR", "^7display: " .. tostring(s.displayGroup and s.displayGroup.displayLabel) .. " main: " .. tostring(build.mainSocketGroup) .. " undo: " .. s.u31Undo .. " set: " .. tostring(s.activeSkillSetId) .. " spectres: " .. #build.spectreList)
	end
end
"#;

fn lua_layout() -> String {
    let f = |a: &[f64]| {
        a.iter()
            .map(|x| pob_lua::tostring_num(*x))
            .collect::<Vec<_>>()
            .join(", ")
    };
    format!(
        "U31_GEM_SLOTS = {{ {} }}\nU31_MANAGE = {{ {} }}\nU31_SPECTRES = {{ {} }}\n",
        f(&GEM_SLOTS),
        f(&MANAGE),
        f(&SPECTRES)
    )
}

/// The stand-in `calcFunc` (`#! u31-fake-calc`; the Lua `fakeCalc`): the misc calculator's base
/// output with `CombinedDPS` moved by every gem of the display group (`quality * 7`, 1000 when
/// enabled, the gem name's length), so the tooltips' temporary model changes and the gem
/// selectors' DPS builder show in the numbers without one bridge calc per gem.
struct FakeMisc {
    build: model::BuildRef,
    base: pob_calc::lua_table::Output,
}

impl pob_app::calc::MiscCalc for FakeMisc {
    fn call(
        &self,
        _ov: Option<Rc<pob_calc::env::Override>>,
        _full: Option<bool>,
    ) -> std::task::Poll<pob_calc::lua_table::Output> {
        let out = self.base.copyTable(true);
        let mut add = 0.0;
        if let Some(group) = self.build.skillsTab.displayGroup.get() {
            for g in group.gemList.get().snapshot() {
                let q = g.quality.get();
                let q = if q.is_nan() { 0.0 } else { q };
                let name = g.gemData.get().map_or(0, |d| d.name.as_str().len());
                add += q * 7.0 + if g.enabled.get() { 1000.0 } else { 0.0 } + name as f64;
            }
        }
        out.set(n!("CombinedDPS"), self.base.num_or(n!("CombinedDPS"), 0.0) + add);
        std::task::Poll::Ready(out)
    }
}

/// The calc facade with the stand-in `calcFunc` (`build_output` stays the bridge's).
struct FakeCalc {
    inner: Box<dyn pob_app::calc::CalcFacade>,
}

impl pob_app::calc::CalcFacade for FakeCalc {
    fn build_output(&mut self, build: &model::BuildRef) -> std::task::Poll<()> {
        self.inner.build_output(build)
    }
    fn misc_calculator(
        &mut self,
        build: &model::BuildRef,
    ) -> std::task::Poll<(Rc<dyn pob_app::calc::MiscCalc>, pob_calc::lua_table::Output)> {
        let std::task::Poll::Ready((_, base)) = self.inner.misc_calculator(build) else {
            unreachable!("desktop")
        };
        std::task::Poll::Ready((
            Rc::new(FakeMisc {
                build: build.clone(),
                base: base.clone(),
            }),
            base,
        ))
    }
}

thread_local! {
    /// `#! u31-fake-calc`
    static FAKE_CALC: Cell<bool> = const { Cell::new(false) };
    /// The stand-in `spec.modFlag` (UndoHandler.lua:37 via `spec:AddUndoState`, U25).
    static SPEC_MOD_FLAG: Cell<bool> = const { Cell::new(false) };
}

/// The stand-in SkillsTab node (`build.skillsTab`): `modFlag`, the undo count and U30's
/// `anchorGemSlots` / `gemSlots` fields.
#[derive(Default)]
struct U31SkillsTab {
    mod_flag: bool,
    undo: u32,
    anchor_gem_slots: CtrlId,
    gem_slots: Vec<GemSlot>,
}

impl Widget for U31SkillsTab {
    const NAME: &'static str = "SkillsTab";
    const CAPS: Caps = Caps::empty();
}

fn tab(cx: &Cx) -> CtrlId {
    skills_tab(cx).expect("the stand-in SkillsTab")
}

fn skills_tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.skills.id();
    cx.ui.is_alive(id).then_some(id)
}

/// `controls.groupList`
fn group_list(cx: &Cx, me: CtrlId) -> CtrlId {
    cx.child(me, "groupList").expect("groupList")
}

/// `tostring(n)` of a number field that may be nil (NaN, CCR-0019).
fn tostring_nan(v: f64) -> String {
    if v.is_nan() {
        "nil".to_owned()
    } else {
        pob_lua::tostring_num(v)
    }
}

/// SkillsTab.lua:1081-1100 `UpdateGemSlots()` (U30's; test port).
fn update_gem_slots(cx: &mut Cx, me: CtrlId) {
    let Some(group) = cx.build().model.skillsTab.displayGroup.get() else {
        return;
    };
    let n = group.gemList.get().len();
    for slot_index in 1..=n + 1 {
        if cx.w::<U31SkillsTab>(me).gem_slots.len() < slot_index {
            CreateGemSlot(cx, me, slot_index);
        }
        let slot = cx.w::<U31SkillsTab>(me).gem_slots[slot_index - 1];
        if slot_index == group.gemList.get().len() + 1 {
            EditControl::set_text(cx, slot.name_spec, "", false);
            EditControl::set_text(cx, slot.level, "", false);
            EditControl::set_text(cx, slot.quality, "", false);
            cx.w_mut::<CheckBoxControl>(slot.enabled).state = false;
            EditControl::set_text(cx, slot.count, "1", false);
        } else {
            let gem = group.gemList.get().get(slot_index).expect("gemList[slotIndex]");
            let col = gem.extra.str(n!("color")).expect("gemInstance.color").to_string();
            cx.w_mut::<EditControl>(slot.name_spec).inactive_col = col;
        }
    }
}

/// The UI tail of SkillsTab.lua:1275-1308 `SetDisplayGroup(socketGroup)` (U30's; test port):
/// `groupLabel` and the gem rows (L1281, L1296-1306).
fn set_display_group_ui_tail(cx: &mut Cx, me: CtrlId, group: Option<SocketGroupRef>) {
    let Some(group) = group else { return };
    let label = model::skills_tab::socketGroup_label(&group).map_or_else(|| "nil".to_owned(), |s| s.to_string());
    let gl = cx.child(me, "groupLabel").expect("groupLabel");
    EditControl::set_text(cx, gl, &label, false);
    update_gem_slots(cx, me);
    for (i, gem) in group.gemList.get().snapshot().iter().enumerate() {
        let slot = cx.w::<U31SkillsTab>(me).gem_slots[i];
        EditControl::set_text(cx, slot.name_spec, &gem.nameSpec.get(), false);
        EditControl::set_text(cx, slot.level, &tostring_nan(gem.level.get()), false);
        EditControl::set_text(cx, slot.quality, &tostring_nan(gem.quality.get()), false);
        cx.w_mut::<CheckBoxControl>(slot.enabled).state = gem.enabled.get();
        cx.w_mut::<CheckBoxControl>(slot.enable_global1).state = gem.enableGlobal1.get();
        cx.w_mut::<CheckBoxControl>(slot.enable_global2).state = gem.enableGlobal2.get();
        let count = pob_lua::tostring_num(gem.count.get().unwrap_or(1.0));
        EditControl::set_text(cx, slot.count, &count, false);
    }
}

/// SkillsTab.lua:1275-1308 `SetDisplayGroup(socketGroup)`: calc:BM-B's model half, then the
/// UI tail.
fn set_display_group(cx: &mut Cx, me: CtrlId, group: Option<SocketGroupRef>) {
    let m = cx.build().model.clone();
    m.skillsTab.SetDisplayGroup(&m, group.clone());
    set_display_group_ui_tail(cx, me, group);
}

/// SkillsTab.lua:1478-1502 `SetActiveSkillSet(skillSetId)`: calc:BM-B's model half, then L1495,
/// the UI tail of L1500 and L1501 (U30's; test port).
fn set_active_skill_set(cx: &mut Cx, id: f64) {
    let me = tab(cx);
    let m = cx.build().model.clone();
    m.skillsTab.SetActiveSkillSet(&m, id);
    let gl = group_list(cx, me);
    SkillListControl::set_list(cx, gl, m.skillsTab.socketGroupList.get());
    set_display_group_ui_tail(cx, me, m.skillsTab.displayGroup.get());
    let f = init::hooks(cx).sync_loadouts;
    f(cx);
}

/// SkillsTab.lua:694-706 `CopySocketGroup(socketGroup)` (U30's; test port).
fn copy_socket_group(cx: &mut Cx, group: &SocketGroupRef) {
    let mut text = String::new();
    if let Some(label) = model::skills_tab::socketGroup_label(group) {
        if pob_lua::pattern::find(&label, "%S", None, false).is_some() {
            text += &format!("Label: {label}\r\n");
        }
    }
    if let Some(slot) = group.slot.get() {
        text += &format!("Slot: {slot}\r\n");
    }
    for gem in group.gemList.get().snapshot() {
        text += &pob_lua::lfmt!(
            "%s %d/%d %s %d\r\n",
            gem.nameSpec.get().to_string().as_str(),
            gem.level.get(),
            gem.quality.get(),
            if gem.enabled.get() { "" } else { "DISABLED" },
            gem.count.get().unwrap_or(1.0)
        );
    }
    cx.sys.platform.copy(&text);
}

/// SkillsTab.lua:1442-1450 `OpenSkillSetManagePopup()` (U30's; test port).
fn open_skill_set_manage_popup(cx: &mut Cx, me: CtrlId) {
    popups::open_popup(
        cx,
        370.0,
        290.0,
        "Manage Skill Sets",
        move |cx, p| {
            let p = p.id();
            skill_set_list_ctor(cx, p, CtrlKey::Idx(1), rect(0, 50, 350, 200), me);
            cx.add(
                p,
                CtrlKey::Idx(2),
                ButtonControl::new(Anchor::NONE, rect(0, 260, 90, 20), "Done", |cx| {
                    popups::close_popup(cx);
                }),
            );
        },
        popups::PopupOpts::default(),
    );
}

/// The stand-in constructor (SkillsTab.lua:113, :165, :429-431 as U31 needs them, then Load's
/// SetActiveSkillSet UI tail and the stand-in buttons; see the module notes).
fn ctor(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = cx
        .add_free(
            Some(build),
            New::new(
                Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
                U31SkillsTab::default(),
            )
            .host(),
        )
        .id();
    // stand-in: the gem lists sort by name (the DPS sort is u29's; one bridge calc per gem)
    cx.build()
        .model
        .skillsTab
        .ui_state()
        .set(n!("sortGemsByDPS"), false);
    // L113
    skill_list_ctor(cx, me, Anchor::new(TopLeft, me, TopLeft), rect(20, 54, 360, 300));
    // L165 (hidden stand-in: the gem edits' tab group master)
    cx.add(
        me,
        "groupLabel",
        EditControl::new(
            Anchor::NONE,
            rect(0, 0, 380, 20),
            EditArgs {
                prompt: Some("Label".to_owned()),
                filter: Some("%c".to_owned()),
                limit: Some(50),
                ..EditArgs::default()
            },
        )
        .shown(Prop::Val(false)),
    );
    // L429 (re-anchored)
    let anchor = cx
        .add_free(
            Some(me),
            PlainControl::new(
                Anchor::new(TopLeft, me, TopLeft),
                rect(GEM_SLOTS[0], GEM_SLOTS[1], 0, 0),
            )
            .shown(Prop::f(|cx, _| cx.build().model.skillsTab.displayGroup.get().is_some())),
        )
        .id();
    cx.w_mut::<U31SkillsTab>(me).anchor_gem_slots = anchor;
    // L430-431
    CreateGemSlot(cx, me, 1);
    // Load's SetActiveSkillSet (L569 -> L1500): the display group's UI tail
    let dg = cx.build().model.skillsTab.displayGroup.get();
    set_display_group_ui_tail(cx, me, dg);
    let place = |r: [f64; 4]| rect(r[0], r[1], r[2], r[3]);
    cx.add(
        me,
        "u31Sets",
        ButtonControl::new(
            Anchor::new(TopLeft, me, TopLeft),
            place(MANAGE),
            "Manage sets",
            move |cx| open_skill_set_manage_popup(cx, me),
        ),
    );
    cx.add(
        me,
        "u31Spectres",
        ButtonControl::new(
            Anchor::new(TopLeft, me, TopLeft),
            place(SPECTRES),
            "Spectres",
            |cx| {
                let f = init::hooks(cx).open_spectre_library;
                f(cx);
            },
        ),
    );
    me
}

/// The stand-in Draw (see the module notes).
fn draw_skills(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab(cx);
    {
        let b = cx.base_mut(me);
        b.x = Prop::Val(vp.x);
        b.y = Prop::Val(vp.y);
        b.width = Prop::Val(vp.width);
        b.height = Prop::Val(vp.height);
    }
    cx.process_controls_input(me, ev, vp);
    pob_app::main_mode::background::draw_background(cx, vp);
    update_gem_slots(cx, me);
    cx.draw_controls(me, vp, None);
    let m = cx.build().model.clone();
    let display = m
        .skillsTab
        .displayGroup
        .get()
        .and_then(|g| g.displayLabel.get())
        .map_or_else(|| "nil".to_owned(), |s| s.to_string());
    let line = format!(
        "^7display: {display} main: {} undo: {} set: {} spectres: {}",
        pob_lua::tostring_num(m.mainSocketGroup.get()),
        cx.w::<U31SkillsTab>(me).undo,
        pob_lua::tostring_num(m.skillsTab.activeSkillSetId.get()),
        m.spectreList.get().len()
    );
    cx.sg.draw_string(
        vp.x + 4.0,
        vp.y + vp.height - 20.0,
        Align::Left,
        16.0,
        Font::Var,
        &line,
    );
}

fn install_skills_tab_ui_hooks() {
    gs::set_skills_tab_ui_hooks(|h| {
        *h = gs::default_skills_tab_ui_hooks();
        h.gem_slot = Rc::new(|cx, me, i| {
            cx.w::<U31SkillsTab>(me).gem_slots.get(i.wrapping_sub(1)).copied()
        });
        h.set_gem_slot = Rc::new(|cx, me, i, slot| {
            let slots = &mut cx.w_mut::<U31SkillsTab>(me).gem_slots;
            if i >= 1 && i <= slots.len() {
                slots[i - 1] = slot;
            } else if i == slots.len() + 1 {
                slots.push(slot);
            } else {
                panic!("gemSlots[{i}] with {} slots", slots.len());
            }
        });
        h.anchor_gem_slots = Rc::new(|cx, me| cx.w::<U31SkillsTab>(me).anchor_gem_slots);
        h.add_undo_state = Rc::new(|cx, me| {
            let w = cx.w_mut::<U31SkillsTab>(me);
            w.undo += 1;
            w.mod_flag = true;
        });
        h.set_display_group = Rc::new(set_display_group);
        h.copy_socket_group = Rc::new(|cx, _me, group| copy_socket_group(cx, group));
    });
}

fn rust_stubs() {
    SPEC_MOD_FLAG.with(|f| f.set(false));
    install_skills_tab_ui_hooks();
    init::set_hooks(|h| {
        *h = init::default_hooks();
        h.spec_add_undo_state = Rc::new(|_| SPEC_MOD_FLAG.with(|f| f.set(true)));
        // U25 stand-in: TreeTab.lua:492-498 (the buildLoadouts selFunc, reached from
        // SyncLoadouts' SetSel), as in u20/u22/u32
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
        h.skills_tab = Rc::new(ctor);
        h.skills_set_active_skill_set = Rc::new(set_active_skill_set);
        h.skills_add_socket_group_tooltip = Rc::new(|cx, tooltip, group| {
            let label = group
                .displayLabel
                .get()
                .map_or_else(|| "nil".to_owned(), |s| s.to_string());
            tooltip.add_line(cx, 16.0, &format!("^7group {label}"));
            let n = group.gemList.get().len();
            tooltip.add_line(cx, 14.0, &format!("^7gems {n}"));
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
        let (draw, flag, set_flag) = (
            h.draw_tab.clone(),
            h.tab_flag.clone(),
            h.set_tab_flag.clone(),
        );
        h.draw_tab = Rc::new(move |cx, t, vp, ev| {
            if t == BuildTab::Skills {
                draw_skills(cx, vp, ev);
            } else {
                draw(cx, t, vp, ev);
            }
        });
        h.tab_flag = Rc::new(move |cx, f| match f {
            TabFlag::Spec => SPEC_MOD_FLAG.with(Cell::get),
            TabFlag::Skills => skills_tab(cx).is_some_and(|me| cx.w::<U31SkillsTab>(me).mod_flag),
            _ => flag(cx, f),
        });
        h.set_tab_flag = Rc::new(move |cx, f, v| match f {
            TabFlag::Spec => SPEC_MOD_FLAG.with(|c| c.set(v)),
            TabFlag::Skills => {
                if let Some(me) = skills_tab(cx) {
                    cx.w_mut::<U31SkillsTab>(me).mod_flag = v;
                }
            }
            _ => set_flag(cx, f, v),
        });
    });
}

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
    stubs: String,
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
            .with_vm(|pob| pob.eval::<()>(self.stubs.as_str()))
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
            Rc::new(move || -> Box<dyn pob_app::calc::CalcFacade> {
                let bridge = Box::new(LuaCalcBridge::new(Some(src.clone())));
                if FAKE_CALC.with(Cell::get) {
                    Box::new(FakeCalc { inner: bridge })
                } else {
                    bridge
                }
            }),
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
            // a group the New button created (SkillListControl.lua:60-64) has both nil in PoB
            // and NaN (CCR-0019) in the port, which calc:BM-B's Save writes as "nan" (CCR-0119 §4)
            // `count`: an item-granted gem of a group UpdateSocketGroups re-creates after an
            // equip change has `count = nil` in PoB and 1 in calc:BM-B's port (the calc reads
            // `count or 1`, so both mean 1); both sides' "nil" read as "1"
            ("Gem", "count") => Some("1"),
            _ => None,
        };
        let new_group_nil = matches!(
            (&*node.elem, &**k),
            ("Skill", "mainActiveSkill" | "mainActiveSkillCalcs")
        ) && ((&**v == "nil") || (!lua && &**v == "nan"));
        if new_group_nil {
            notes.gem_nil_bools += 1;
            attrs.push((k.to_string(), "nil".to_owned()));
        } else if let Some(d) = rust_default.filter(|_| &**v == "nil") {
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

/// Compare one T4 pair; `Ok(byte_identical)`.
fn compare_t4(what: &str, lua: &str, rust: &str, notes: &mut T4Notes) -> Result<bool, String> {
    let lr = root_element(lua).map_err(|e| format!("{what}: Lua XML: {e}"))?;
    let rr = root_element(rust).map_err(|e| format!("{what}: Rust XML: {e}"))?;
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

    // Lua
    let handles = OracleHandles::default();
    let lua_app = runner::lua_side(root, &user, &handles, Entry::Launch, false)?;
    let fake = if FAKE_CALC.with(Cell::get) { "true" } else { "false" };
    let stubs = format!("U31_FAKE = {fake}\n{}{LUA_STUBS}", lua_layout());
    let mut lua_side = LuaSide {
        app: lua_app,
        stubs,
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

/// `#! u31-expect TEXT`: some Lua frame draws a string containing TEXT (guards a script whose
/// clicks silently stopped reaching their target, which both sides would agree on).
fn check_expectations(text: &str, lua: &Side) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    let mut errs = Vec::new();
    for want in directives(text, "u31-expect") {
        let found = lua
            .frames
            .iter()
            .flat_map(|f| &f.calls)
            .any(|c| match &c.call {
                ApiCall::DrawString { text, .. } => text.contains(want.as_str()),
                _ => false,
            });
        if !found {
            errs.push(format!("no Lua DrawString contains {want:?}"));
        }
    }
    errs
}

#[test]
fn u31_scenarios() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let mut scripts: Vec<PathBuf> = std::fs::read_dir(test_dir())
        .expect("tests/u31")
        .filter_map(Result::ok)
        .map(|e| e.path())
        .filter(|p| p.extension().is_some_and(|x| x == "pobscript"))
        .collect();
    scripts.sort();
    assert!(!scripts.is_empty(), "no tests/u31/*.pobscript");
    let filter = std::env::var("U31_FILTER").ok();
    let only_screen = std::env::var("U31_SCREEN").ok();
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
        let fake = !directives(&text, "u31-fake-calc").is_empty();
        FAKE_CALC.with(|f| f.set(fake));
        // a script that sets its own `screen` (the popups are centred, so their coordinates
        // depend on the virtual height) runs once, at that screen
        let own_screen = match script.steps.first() {
            Some(pob_host::script::Step::Screen { w, h, dpi }) => Some((*w, *h, *dpi)),
            _ => None,
        };
        let screens: Vec<(i32, i32, f32)> =
            own_screen.map_or_else(|| SCREENS.to_vec(), |s| vec![s]);
        for (w, h, s) in screens {
            if only_screen.as_ref().is_some_and(|o| *o != w.to_string()) {
                continue;
            }
            let tag = format!("u31_{stem}@{w}x{h}@{s}");
            let (lua, rust) = match run_sides(
                &root,
                &script,
                fx.user_dir.as_deref(),
                runner::screen(w, h, s),
                &[],
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
    println!("u31: {ran} runs, T4 {t4_pairs} pairs ({t4_bytes} byte-identical)");
    assert!(ran > 0 || filter.is_some(), "no run");
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

/// The DrawString texts of a frame trace, sorted (a multiset: a rebuilt row has a later creation
/// seq, so it is drawn after the other rows).
fn drawn_texts(frames: &[FrameTrace]) -> Vec<String> {
    use pob_sg::trace::ApiCall;
    let mut v: Vec<String> = frames
        .iter()
        .flat_map(|f| &f.calls)
        .filter_map(|c| match &c.call {
            ApiCall::DrawString { text, .. } => Some(text.clone()),
            _ => None,
        })
        .collect();
    v.sort();
    v
}

/// One traced frame of the Rust side.
fn traced_frame(rust: &mut RustSide, d: &mut Driver) -> Vec<FrameTrace> {
    rust.app.sg().trace = Some(pob_sg::trace::TraceRecorder::new());
    d.frame(rust).expect("frame");
    rust.app.sg().trace.take().expect("trace").frames
}

/// The keys of a gem row (`self.controls["gemSlot"..index..X]`) with the row's controls.
fn row_keys(index: usize, s: &GemSlot) -> [(String, CtrlId); 9] {
    [
        (format!("gemSlot{index}Delete"), s.delete),
        (format!("gemSlot{index}Name"), s.name_spec),
        (format!("gemSlot{index}Level"), s.level),
        (format!("gemSlot{index}Quality"), s.quality),
        (format!("gemSlot{index}Enable"), s.enabled),
        (format!("gemSlot{index}Count"), s.count),
        (format!("gemSlot{index}ErrMsg"), s.err_msg),
        (format!("gemSlot{index}EnableGlobal1"), s.enable_global1),
        (format!("gemSlot{index}EnableGlobal2"), s.enable_global2),
    ]
}

#[test]
fn u31_rebuilt_slot_controls() {
    let _g = LOCK
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    let Some(root) = inputs() else { return };
    let text = "fixture userdir=../../fixtures/user1\nframes 2\nload-build \"Builds/allflame_exp_048.xml\"\nframes 2\nkey-down CTRL\npress 2\nkey-up CTRL\nframes 3\n";
    let script = runner::parse_script(text).expect("script");
    let fx = runner::fixtures(&script, &test_dir()).expect("fixtures");
    let user = UserDir::new(fx.user_dir.as_deref()).expect("user dir");
    let handles = OracleHandles::default();
    let (sg, sys) = boot::oracle_parts(&root, &user.path, &handles).expect("oracle parts");
    let src = std::fs::canonicalize(root.join("src")).unwrap_or_else(|_| root.join("src"));
    let mut rust = RustSide::new(PlatformParts { sg, sys }, src);
    let mut d = Driver::new(runner::screen(1280, 800, 1.0), handles, user.path.clone());
    let (_frames, r) = runner::drive(&mut rust, &script, &mut d);
    r.expect("drive");
    let before = drawn_texts(&traced_frame(&mut rust, &mut d));
    let me = tab(&rust.app.cx);
    let n_rows = rust.app.cx.w::<U31SkillsTab>(me).gem_slots.len();
    assert!(n_rows >= 2, "the display group's rows");
    for index in [1, n_rows] {
        let cx = &mut rust.app.cx;
        let old = cx.w::<U31SkillsTab>(me).gem_slots[index - 1];
        // CreateGemSlot again, then what SetDisplayGroup writes into the row
        CreateGemSlot(cx, me, index);
        let new = cx.w::<U31SkillsTab>(me).gem_slots[index - 1];
        let dg = cx.build().model.skillsTab.displayGroup.get();
        set_display_group_ui_tail(cx, me, dg);
        for ((key, o), (_, n)) in row_keys(index, &old).into_iter().zip(row_keys(index, &new)) {
            assert_ne!(o, n, "{key}: a new control");
            assert_eq!(cx.child(me, key.clone()), Some(n), "{key}: the key names the new control");
            assert!(!cx.is_child_of(me, o), "{key}: the old control left the controls table");
        }
        cx.end_frame_gc();
        // the next row still references the old delete button and enableGlobal check boxes
        // (gem_slots.rs module notes: a rebuilt row); everything else is freed
        let kept = [old.delete, old.enable_global1, old.enable_global2];
        for (key, o) in row_keys(index, &old) {
            let referenced = index < n_rows && kept.contains(&o);
            assert_eq!(cx.ui.is_alive(o), referenced, "{key}: freed unless the next row references it");
        }
        let after = drawn_texts(&traced_frame(&mut rust, &mut d));
        assert_eq!(before, after, "row {index}: the rebuilt row draws once, as before");
    }
    println!(
        "u31_rebuilt_slot_controls: {n_rows} rows, rows 1 and {n_rows} rebuilt, {} strings per frame",
        before.len()
    );
}
