//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TreeTab.lua:1-1046 (the data halves of 500-560 are calc:BM-A).
//! Owner: U25.
//!
//! @pob-file Classes/TreeTab.lua @16de4b82d
//!
//!
//! The Tree tab (`newClass("TreeTab", "ControlHost")`): a free node owned by the build node
//! (`build.treeTab`, `BuildState::tabs.tree`), like the Calcs and Notes tabs. The model half is
//! calc:BM-A's (`model.treeTab`: `specList`, `activeSpec`, `ctor_model`, Load/PostLoad/Save,
//! the model part of `SetActiveSpec`; ui doc §10.2); this file is the UI half: the constructor
//! (its controls, the viewer, `SetCompareSpec`), Draw, GetSpecList, the UI tail of
//! SetActiveSpec, SetCompareSpec, ConvertToVersion, ConvertAllToVersion, RemoveTattooFromNode,
//! SaveMasteryPopup and the spec's UndoHandler. The popups are `popups.rs`.
//!
//! Split with the model half (markers): calc:BM-A covers TreeTab.lua 26-38 (`ctor_model`) and
//! 500-585. The UI statements of those lines are here, commented with their line numbers and no
//! marker (L27, L29-33, L38; SetActiveSpec L556-557, L578-584).
//!
//! The spec's UndoHandler (PassiveSpec.lua:22, 28, 194; UndoHandler.lua; ui doc §10.3): every
//! `PassiveSpec` is an UndoHandler with its own `undo`/`redo` buffers and `modFlag`. The calc
//! port leaves the buffers to the UI (passive_spec.rs L28/L194 comments), so they live here,
//! one [`UndoStack`] per spec, keyed by the spec's `Rc` identity ([`State::spec_undo`]). Every
//! Lua call is on `self.build.spec` (the active spec), so [`SPEC_UNDO`] resolves the stack of
//! the active spec; a spec that never had `ResetUndo`/`AddUndoState` has empty buffers, as
//! `UndoHandler()` leaves them (PassiveSpec.lua:28).
//!
//! Seams to units not ported yet ([`TreeTabHooks`], one entry each, defaults name the owner):
//! U26's `SetPowerCalc`, `PowerReportListControl` (constructor, `label`, `SetReport` with
//! `BuildPowerReportList`), U27's `FindTimelessJewel`. The power-builder callbacks
//! (L292-324) are [`power_builder_progress_callback`] / [`power_builder_callback`]: they need
//! `&mut Cx`, which calc:PWR's `build.powerBuilderCallback` (`Rc<dyn Fn()>`) does not pass, so
//! U26 calls them where it resumes the PowerBuilder instead of registering them in the model.
//!
//! Lua semantics:
//! * `self.showConvert` is calc:BM-A's (`model.treeTab.showConvert`, CCR-0052 §9).
//! * `self.controls.compareSelect:SetList(newSpecList)` then `t_insert(newSpecList, "Manage
//!   trees...")` (L456-457): the compare dropdown aliases the list, so it shows the "Manage"
//!   entry too, while its dropped width was measured without it (DropDownControl
//!   DIVERGENCE(alias): reproduced here by assigning the longer list after `SetList`).
//! * `nodePowerMaxDepthSelect`'s list `{ "All", 5, 10, 15, "Custom" }` holds numbers: their
//!   [`DropItem`]s carry an `f64` value and the label `tostring(n)`.
//! * `self.defaultTattoo[nodeName]`, `self.showLegacyTattoo` (nil until the checkbox is used):
//!   `Option`s.
//!
//! DIVERGENCE(set-active-spec-order): Lua runs SetActiveSpec's UI lines 556-557 (window title,
//! `UpdateClassDropdowns`) between the model lines 555 and 558; the port runs calc:BM-A's model
//! half (550-555, 558-577) in one call and then the UI lines (CCR-0098 §6: the model has no
//! hook there). 558-577 change only item slots, which 556-557 do not read.
//! DIVERGENCE(reset-undo): `PassiveSpec:Load` ends with `self:ResetUndo()`
//! (PassiveSpec.lua:194). calc:BM-A's TreeTab:Load runs inside `Init_model` (and PostLoad after
//! it), before the UI constructor, so [`ctor_hook`] calls `ResetUndo` for every spec right after
//! constructing the tab when the build file has a legacy `Spec` section or a `Tree` section with
//! `Spec` children (a `Tree` without them leaves TreeTab.lua:522-524's unloaded fallback spec,
//! whose buffers stay empty, as in PoB). The state differs from PoB's only where PostLoad's
//! `BuildClusterJewelGraphs` changed the allocation (cluster jewel subgraph nodes): see CCR-0123.
//! DIVERGENCE(load-version-select): the `SetActiveSpec` at the end of a deferred `Tree` load
//! (L526) runs in calc:BM-A's `Init_model`, before the UI tabs exist. Init replays its UI lines
//! where Build.lua:671-676 runs the load: L556-557 (`tree_tab_load_set_active_spec_ui_tails`,
//! CCR-0098 §6), then L579 and L581-583 ([`load_set_active_spec_ui_tail`]: the ItemsTab spec
//! dropdown gets that load's raw `activeSpec`, `versionSelect` the active spec's version).
//! With more than one `Tree` section, L582 uses the final spec for every replay (that section's
//! own spec in PoB; CCR-0098 §6's approximation): the last replay leaves the same state. L584
//! (`SyncLoadouts`) is left to Init, which calls it again at Build.lua:726.
//! DIVERGENCE(items-spec-select): `self.build.itemsTab.controls.specSelect` (L579,
//! PassiveSpecListControl.lua:125-131) is ItemsTab's control (U33, ported): it is updated when
//! the build's ItemsTab node has a `specSelect` DropDownControl (always, with the real ItemsTab
//! constructor) and skipped otherwise (a test stand-in for the Items tab).
//!
//! ORDER: `pairs(self.build.spec.allocNodes)` (L76) only counts (c); `pairs(self.build.itemsTab
//! .slots)` (L558) is calc:BM-A's; `pairs(self.build.spec.hashOverrides)` (L145) removes every
//! tattoo, whose result does not depend on the order (c). The popups' loops: `popups.rs`.
//!
//! Props with side effects (ui doc §7.3): none (the label/width functions of specConvert,
//! specConvertAll and powerReport only read).

#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::m_min;
use pob_lua::{n, pattern, tostring_num};
use pob_sg::{Font, Key};
use pob_ui_core::controls::drop_down::{DropTipArgs, DropTipMode};
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl,
    LabelControl, ScrollBarControl,
};
use pob_ui_core::input::{EventKind, InputEvents};
use pob_ui_core::luastr::format_num_sep_main;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;
use pob_ui_core::undo::{UndoOps, UndoStack};

use super::TreeTab;
use crate::app::CxApp;
use crate::build::init::{self, flush_window_titles, set_window_title_with_build_class};
use crate::main_mode::toast;
use crate::model::xml::XmlChild;
use crate::model::{self, build_xml, Name, NodeRef, PassiveSpecRef, Table, Val};
use crate::tree_view::PassiveTreeView;
// calc:PWR's accessors and the PassiveSpec constructor are not re-exported by model.rs (U15;
// the same gap as CCR-0103 §4 / CCR-0115)
use pob_calc::classes::passive_spec::new_PassiveSpec; // CCR-0123 §9
use pob_calc::modules::power; // CCR-0123 §9

// @pob Classes/TreeTab.lua:1-25 h=b32db93e
// (header comment, the module locals (pob_lua is used directly), `dkjson` (U27's), newClass;
// `---@param build Build`: the struct and Widget impl below)

/// U25's part of the TreeTab object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.build` (the BuildMode node)
    pub build: CtrlId,
    /// `self.isComparing`
    pub is_comparing: bool,
    /// `self.isCustomMaxDepth`
    pub is_custom_max_depth: bool,
    /// `self.viewer` (lent while it draws, ui doc §7.2)
    pub viewer: PassiveTreeView,
    /// `self.activeCompareSpec`
    pub active_compare_spec: f64,
    /// `self.compareSpec`
    pub compare_spec: Option<PassiveSpecRef>,
    /// `self.anchorControls` (a `Control` outside `self.controls`)
    pub anchor_controls: CtrlId,
    /// `self.treeVersions` (`{ label = display, value = num }` per `treeVersionList` entry)
    pub tree_versions: Vec<DropItem>,
    /// `self.tradeLeaguesList` (U27 fills it)
    pub trade_leagues_list: Table,
    /// `self.defaultTattoo[nodeName]`
    pub default_tattoo: Vec<(String, f64)>,
    /// `self.powerStatList` (the `data.powerStatList` entries without `ignoreForNodes`)
    pub power_stat_list: Vec<DropItem>,
    /// `self.powerBuilderToastId`
    pub power_builder_toast_id: Option<String>,
    /// `self.lastProgressToastUpdate`
    pub last_progress_toast_update: f64,
    /// `self.jumpToNode`, `self.jumpToX`, `self.jumpToY`
    pub jump_to_node: bool,
    pub jump_to_x: f64,
    pub jump_to_y: f64,
    /// `self.modFlag`
    pub mod_flag: bool,
    /// `self.searchFlag` (nil and false are both false)
    pub search_flag: bool,
    /// `self.showLegacyTattoo` (nil until the ModifyNodePopup checkbox sets it)
    pub show_legacy_tattoo: Option<bool>,
    /// The UndoHandler buffers of each spec (module docs), keyed by `Rc` identity; the entries
    /// of specs that left the list are dropped when a new entry is made.
    pub spec_undo: Vec<(PassiveSpecRef, UndoStack<Table>)>, // CCR-0123 §1
}

impl Widget for TreeTab {
    const NAME: &'static str = "TreeTab";
    // drawn by BuildMode (Build.lua:1309, `ShellHooks::draw_tab`); not a Control
    const CAPS: Caps = Caps::empty();
}

/// One `self.powerStatList` entry: its index in `data.powerStatList` (1-based).
#[derive(Clone, Copy, Debug)]
pub struct PowerStatRef(pub usize);

/// `data.powerStatList[i]` as calc:PWR's table (`stat`, `label`, ...).
pub fn power_stat_table(r: PowerStatRef) -> Table {
    let stats = &model::data().powerStatList.stats;
    power::powerStat_table(&stats[r.0 - 1])
}

// ---- seams to U26/U27 ------------------------------------------------------------------------

/// `selectedNode.x`, `selectedNode.y` of the PowerReportListControl's callback (L279-287).
pub type JumpFn = Rc<dyn Fn(&mut Cx, Option<f64>, Option<f64>)>;

/// The TreeTab methods and controls of U26/U27 that U25's code calls (module docs).
#[derive(Clone)]
pub struct TreeTabHooks { // CCR-0123 §6
    /// `self:SetPowerCalc(powerStat)` (TreeTab.lua:1047-1061, U26)
    pub set_power_calc: Rc<dyn Fn(&mut Cx, CtrlId, Option<Table>)>,
    /// `self.controls.powerReportList = new("PowerReportListControl"):PowerReportListControl(
    /// anchor, rect, callback)` (L279, U26): adds the control as `powerReportList` of the tab
    pub power_report_list: Rc<dyn Fn(&mut Cx, CtrlId, Anchor, [Prop<f64>; 4], JumpFn) -> CtrlId>,
    /// `self.controls.powerReportList.label = message` (L300, U26)
    pub power_report_list_set_label: Rc<dyn Fn(&mut Cx, CtrlId, &str)>,
    /// `local report = self:BuildPowerReportList(powerStat)` +
    /// `self.controls.powerReportList:SetReport(powerStat, report)` (L316-317, U26)
    pub power_report_set: Rc<dyn Fn(&mut Cx, CtrlId, Table)>,
    /// `self:FindTimelessJewel()` (TreeTab.lua:1170-2888, U27)
    pub find_timeless_jewel: Rc<dyn Fn(&mut Cx, CtrlId)>,
}

fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The placeholders until U26/U27 land: the power report list is a plain control that starts
/// hidden (L288) and draws nothing; the user-triggered methods raise a Lua error naming the
/// owner.
pub fn default_tree_tab_hooks() -> TreeTabHooks {
    TreeTabHooks {
        set_power_calc: Rc::new(|_, _, _| not_ported("TreeTab:SetPowerCalc", "U26")),
        power_report_list: Rc::new(|cx, me, anchor, r, _jump| {
            cx.add(me, "powerReportList", PlainControl::new(anchor, r)).id()
        }),
        power_report_list_set_label: Rc::new(|_, _, _| {}),
        power_report_set: Rc::new(|_, _, _| not_ported("TreeTab:BuildPowerReportList", "U26")),
        find_timeless_jewel: Rc::new(TreeTab::FindTimelessJewel), // U27 (CCR-0123 §6)
    }
}

thread_local! {
    static HOOKS: RefCell<Option<TreeTabHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn tree_tab_hooks() -> TreeTabHooks {
    HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_tree_tab_hooks)
            .clone()
    })
}

/// Replace hook entries (owners at run time, tests).
pub fn set_tree_tab_hooks(f: impl FnOnce(&mut TreeTabHooks)) {
    HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_tree_tab_hooks)));
}

// ---- the spec's UndoHandler (module docs) ----------------------------------------------------

fn spec_stack_index(cx: &mut Cx, me: CtrlId, spec: &PassiveSpecRef) -> usize {
    let st = &cx.w::<TreeTab>(me).core;
    if let Some(i) = st.spec_undo.iter().position(|(s, _)| Rc::ptr_eq(s, spec)) {
        return i;
    }
    // the buffers of specs deleted or converted away go with them, as Lua's collector frees
    // them: only the list's specs and the active one keep an entry
    let m = cx.build().model.clone();
    let (list, active) = (m.treeTab.specList.snapshot(), m.spec.get());
    let st = &mut cx.w_mut::<TreeTab>(me).core;
    st.spec_undo
        .retain(|(s, _)| Rc::ptr_eq(s, &active) || list.iter().any(|l| Rc::ptr_eq(s, l)));
    // UndoHandler.lua:17-21 (PassiveSpec.lua:28): empty buffers
    st.spec_undo.push((spec.clone(), UndoStack::default()));
    st.spec_undo.len() - 1
}

fn spec_undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<Table> {
    let spec = cx.build().model.spec.get();
    let i = spec_stack_index(cx, me, &spec);
    &mut cx.w_mut::<TreeTab>(me).core.spec_undo[i].1
}

/// `self:CreateUndoState()` of the active spec (PassiveSpec.lua:2361-2383, calc:PS-C).
fn spec_create_undo_state(cx: &mut Cx, _me: CtrlId) -> Table {
    cx.build().model.spec.get().CreateUndoState()
}

/// `self:RestoreUndoState(state)` of the active spec (PassiveSpec.lua:2385-2388, calc:PS-C; its
/// `SetWindowTitleWithBuildClass` is sent here).
fn spec_restore_undo_state(cx: &mut Cx, _me: CtrlId, state: Table) {
    let spec = cx.build().model.spec.get();
    spec.RestoreUndoState(&state, None);
    flush_window_titles(cx);
}

/// The active spec's UndoHandler (`self.build.spec:AddUndoState()` etc.; `me` is the TreeTab).
pub const SPEC_UNDO: UndoOps<Table> = UndoOps {
    stack: spec_undo_stack,
    create: spec_create_undo_state,
    restore: spec_restore_undo_state,
};

/// `spec:ResetUndo()` (UndoHandler.lua:25-29) for any spec of the list.
/// (the same statements as `UndoOps::reset_undo`, whose marker covers UndoHandler.lua:25-29)
pub fn spec_reset_undo(cx: &mut Cx, me: CtrlId, spec: &PassiveSpecRef) {
    let i = spec_stack_index(cx, me, spec);
    {
        let st = &mut cx.w_mut::<TreeTab>(me).core.spec_undo[i].1;
        // L26-27
        st.undo.clear();
        st.redo.clear();
    }
    // L28
    let s = spec.CreateUndoState();
    cx.w_mut::<TreeTab>(me).core.spec_undo[i].1.undo.push_front(s);
}

/// `self.spec.modFlag` (UndoHandler.lua:37).
pub fn spec_mod_flag(cx: &Cx, me: CtrlId) -> bool {
    let spec = cx.build().model.spec.get();
    cx.w::<TreeTab>(me)
        .core
        .spec_undo
        .iter()
        .find(|(s, _)| Rc::ptr_eq(s, &spec))
        .is_some_and(|(_, st)| st.mod_flag)
}

/// `self.spec.modFlag = v` (ResetModFlags, Build.lua:1107).
pub fn set_spec_mod_flag(cx: &mut Cx, me: CtrlId, v: bool) {
    spec_undo_stack(cx, me).mod_flag = v;
}

// ---- helpers -----------------------------------------------------------------------------------

fn st(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<TreeTab>(me).core
}

fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<TreeTab>(me).core
}

/// `self.controls.<key>`
pub fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `#self.specList`
fn spec_count(cx: &Cx) -> f64 {
    cx.build().model.treeTab.specList.len() as f64
}

/// `self.specList[index]` (nil for a non-integer or out-of-range index)
pub fn spec_at(cx: &Cx, index: f64) -> Option<PassiveSpecRef> {
    (index >= 1.0 && index.fract() == 0.0)
        .then(|| cx.build().model.treeTab.specList.get(index as usize))
        .flatten()
}

/// `treeVersions[v]`
fn tree_version_display(v: &str) -> String {
    model::data()
        .gameVersions
        .treeVersions
        .get(&Name::from(v))
        .map(|t| t.display.to_string())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// The raw `x`/`width`/... of a control (a number field).
fn raw(p: &Prop<f64>) -> f64 {
    p.raw()
}

/// `self.build.itemsTab.controls.specSelect` (U33's control; module docs).
pub fn items_spec_select(cx: &Cx) -> Option<CtrlId> { // CCR-0123 §7
    let items = cx.app().build.as_ref()?.tabs.items.id();
    if items == CtrlId::default() {
        return None;
    }
    let id = cx.child(items, "specSelect")?;
    cx.is::<DropDownControl>(id).then_some(id)
}

impl TreeTab {
    /// lua: TreeTab.lua:26-349 `TreeTabClass:TreeTab(build)` (L26-38: model half calc:BM-A
    /// `ctor_model`, which Init_model ran)
    pub fn new(build: CtrlId) -> New<TreeTab> {
        // L27 self:ControlHost()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            TreeTab {
                core: State {
                    // L29
                    build,
                    // L30-31
                    is_comparing: false,
                    is_custom_max_depth: false,
                    ..State::default()
                },
                ..TreeTab::default()
            },
        )
        .host()
        .init(|cx, me| Self::init(cx, me.id()))
    }

    /// The constructor body after the node exists.
    /// @pob Classes/TreeTab.lua:39-349 h=a4799b0d
    fn init(cx: &mut Cx, me: CtrlId) {
        let m = cx.build().model.clone();
        // L33
        let viewer = PassiveTreeView::new(cx);
        st_mut(cx, me).viewer = viewer;
        // L35-37 (model half: calc:BM-A `ctor_model`; L37's UI lines 556-557 are U19's
        // CCR-0098 §6 replay, L578-584 see the module docs)
        m.treeTab.ctor_model(&m);
        // L38
        Self::SetCompareSpec(cx, me, 1.0);

        // L40
        let anchor = cx
            .add_free(Some(me), PlainControl::new(Anchor::NONE, rect(0, 0, 0, 20)))
            .id();
        st_mut(cx, me).anchor_controls = anchor;

        // Tree list dropdown
        // L43-50
        let spec_select = cx
            .add(
                me,
                "specSelect",
                DropDownControl::new(
                    Anchor::new(Left, anchor, Right),
                    rect(0, 0, 190, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _v: Option<&DropItem>| {
                        // L44
                        if spec_at(cx, index as f64).is_some() {
                            // L45
                            cx.build_mut().mod_flag = true;
                            // L46
                            Self::SetActiveSpec(cx, me, index as f64);
                        } else {
                            // L48
                            Self::OpenSpecManagePopup(cx, me);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        {
            // L51-53
            let w = cx.w_mut::<DropDownControl>(spec_select);
            w.max_dropped_width = 1000.0;
            w.enable_dropped_width = true;
            w.enable_change_box_width = true;
        }
        // L54
        let sb = cx.child_as::<ScrollBarControl>(spec_select, "scrollBar");
        cx.base_mut(sb).enabled = Prop::Val(true);
        // L55-107
        cx.w_mut::<DropDownControl>(spec_select).tt.tooltip_func = Some(Rc::new(
            move |cx: &mut Cx, tooltip: &mut Tooltip, (mode, sel_index, _v): DropTipArgs| {
                Self::spec_select_tooltip(cx, me, tooltip, mode, sel_index);
            },
        ));

        // Compare checkbox
        // L110-119
        let compare_check = cx
            .add(
                me,
                "compareCheck",
                CheckBoxControl::new(
                    Anchor::new(Left, spec_select, Right),
                    rect(74, 0, 20, 0),
                    Some("Compare:".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L111
                        st_mut(cx, me).is_comparing = state;
                        // L112
                        let a = st(cx, me).active_compare_spec;
                        Self::SetCompareSpec(cx, me, a);
                        // L113
                        let compare_select = ctl(cx, me, "compareSelect");
                        cx.base_mut(compare_select).shown = Prop::Val(state);
                        let reset = ctl(cx, me, "reset");
                        // L114
                        if state {
                            // L115
                            cx.set_anchor(reset, Left, Some(compare_select), Right, None, false);
                        } else {
                            // L117
                            let compare_check = ctl(cx, me, "compareCheck");
                            cx.set_anchor(reset, Left, Some(compare_check), Right, None, false);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();

        // Compare tree dropdown
        // L122-128
        let compare_select = cx
            .add(
                me,
                "compareSelect",
                DropDownControl::new(
                    Anchor::new(Left, compare_check, Right),
                    rect(8, 0, 190, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _v: Option<&DropItem>| {
                        // L123
                        if spec_at(cx, index as f64).is_some() {
                            // L124
                            Self::SetCompareSpec(cx, me, index as f64);
                        } else {
                            // L126
                            Self::OpenSpecManagePopup(cx, me);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L129
        cx.base_mut(compare_select).shown = Prop::Val(false);
        {
            // L130-132
            let w = cx.w_mut::<DropDownControl>(compare_select);
            w.max_dropped_width = 1000.0;
            w.enable_dropped_width = true;
            w.enable_change_box_width = true;
        }
        // L133-158
        let reset = cx
            .add(
                me,
                "reset",
                ButtonControl::new(
                    Anchor::new(Left, compare_check, Right),
                    rect(8, 0, 145, 20),
                    "Reset Tree/Tattoos",
                    move |cx| Self::open_reset_popup(cx, me),
                ),
            )
            .id();

        // Tree Version Dropdown
        // L161-168
        let tree_versions: Vec<DropItem> = model::data()
            .gameVersions
            .treeVersionList
            .iter()
            .map(|num| DropItem::new(tree_version_display(num), Rc::new(*num)))
            .collect();
        st_mut(cx, me).tree_versions = tree_versions.clone();
        // L169
        let version_text = cx
            .add(
                me,
                "versionText",
                LabelControl::new(Anchor::new(Left, reset, Right), rect(8, 0, 0, 16), "^7Version:"),
            )
            .id();
        // L170-174
        let version_select = cx
            .add(
                me,
                "versionSelect",
                DropDownControl::new(
                    Anchor::new(Left, version_text, Right),
                    rect(8, 0, 100, 20),
                    tree_versions,
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, selected: Option<&DropItem>| {
                        let selected = selected
                            .and_then(|s| s.get::<Name>().copied())
                            .unwrap_or_else(|| lua_error!("attempt to index local 'selected' (a nil value)"));
                        // L171
                        if selected != cx.build().model.spec.get().treeVersion.get() {
                            // L172
                            Self::OpenVersionConvertPopup(cx, me, selected, true);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        {
            // L175-177
            let w = cx.w_mut::<DropDownControl>(version_select);
            w.max_dropped_width = 1000.0;
            w.enable_dropped_width = true;
            w.enable_change_box_width = true;
        }
        // L178
        DropDownControl::check_dropped_width(cx, version_select, true);
        // L179
        let n_versions = st(cx, me).tree_versions.len();
        cx.w_mut::<DropDownControl>(version_select).sel_index = n_versions;

        // Tree Search Textbox
        // L182-185
        let search_width = if cx.app().main.portrait_mode { 200 } else { 300 };
        let tree_search = cx
            .add(
                me,
                "treeSearch",
                EditControl::new(
                    Anchor::new(Left, version_select, Right),
                    rect(8, 0, search_width, 20),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Search".to_owned()),
                        filter: Some("%c".to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            let s = st_mut(cx, me);
                            // L183
                            s.viewer.search_str = buf.to_owned();
                            // L184
                            s.search_flag = buf != s.viewer.search_str_saved;
                        })),
                        clearable: true,
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L186
        cx.w_mut::<EditControl>(tree_search).tt.tooltip_text = Some(Prop::Val(Some(
            "Uses Lua pattern matching for complex searches.\nPrefix your search with \"oil:\" to search by anoint recipe.\nTo search for multiple terms: (increased.fire.damage|increased.area.of.effect|etc)".to_owned(),
        )));

        // table holding all realm/league pairs. (allLeagues[realm] = [league.id,...])
        // L189
        st_mut(cx, me).trade_leagues_list = Table::new();
        // Find Timeless Jewel Button
        // L191-193
        let find_timeless = cx
            .add(
                me,
                "findTimelessJewel",
                ButtonControl::new(
                    Anchor::new(Left, tree_search, Right),
                    rect(8, 0, 150, 20),
                    "Find Timeless Jewel",
                    move |cx| {
                        // L192
                        let f = tree_tab_hooks().find_timeless_jewel;
                        f(cx, me);
                    },
                ),
            )
            .id();

        Self::init_b(cx, me, find_timeless, spec_select);
    }
}

impl TreeTab {
    /// The constructor from L195 on (split from [`TreeTab::init`] for size; same marker).
    fn init_b(cx: &mut Cx, me: CtrlId, find_timeless: CtrlId, spec_select: CtrlId) {
        // --Default index for Tattoos
        // L196
        st_mut(cx, me).default_tattoo = Vec::new();

        // Show Node Power Checkbox
        // L199-209
        let tree_heat_map = cx
            .add(
                me,
                "treeHeatMap",
                CheckBoxControl::new(
                    Anchor::new(Left, find_timeless, Right),
                    rect(130, 0, 20, 0),
                    Some("Show Node Power:".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L200
                        st_mut(cx, me).viewer.show_heat_map = state;
                        // L201
                        let stat_select = ctl(cx, me, "treeHeatMapStatSelect");
                        cx.base_mut(stat_select).shown = Prop::Val(state);
                        let toast_id = st(cx, me).power_builder_toast_id.clone();
                        let list = ctl(cx, me, "powerReportList");
                        // L203
                        if !state && toast_id.as_deref().is_some_and(|id| toast::exists(cx, id)) {
                            // L204
                            cx.base_mut(list).shown = Prop::Val(false);
                            // L205
                            toast::remove(cx, toast_id.as_deref().unwrap_or_default(), false);
                        } else if !state {
                            // L207
                            cx.base_mut(list).shown = Prop::Val(false);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();

        // Control for setting max node depth to limit calculation time of the heat map
        // L212-241
        let depth_list = vec![
            DropItem::str("All"),
            DropItem::new("5", Rc::new(5.0_f64)),
            DropItem::new("10", Rc::new(10.0_f64)),
            DropItem::new("15", Rc::new(15.0_f64)),
            DropItem::str("Custom"),
        ];
        let depth_select = cx
            .add(
                me,
                "nodePowerMaxDepthSelect",
                DropDownControl::new(
                    Anchor::new(Left, tree_heat_map, Right),
                    rect(8, 0, 55, 20),
                    depth_list,
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        Self::node_power_max_depth_sel(cx, me, value);
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L242
        cx.w_mut::<DropDownControl>(depth_select).tt.tooltip_text = Some(Prop::Val(Some(
            "Limit of Node distance to search (lower = faster)".to_owned(),
        )));

        // Control for setting max node depth by custom value
        // L245-252
        let depth_custom = cx
            .add(
                me,
                "nodePowerMaxDepthCustom",
                EditControl::new(
                    Anchor::new(Left, depth_select, Right),
                    rect(8, 0, 70, 20),
                    EditArgs {
                        init: Some("0".to_owned()),
                        filter: Some("%D".to_owned()),
                        change_func: Some(Rc::new(move |cx: &mut Cx, value: &str| {
                            let m = cx.build().model.clone();
                            // L246
                            power::set_nodePowerMaxDepth(&m, pob_lua::tonumber(value));
                            // If the heat map is shown, recalculate it with new value
                            // L249
                            if st(cx, me).viewer.show_heat_map {
                                // L250
                                let f = tree_tab_hooks().set_power_calc;
                                f(cx, me, power::powerStat(&m));
                            }
                        })),
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L253
        cx.base_mut(depth_custom).shown = Prop::Val(false);

        // Control for selecting the power stat to sort by (Defense, DPS, etc)
        // L256-258
        let stat_select = cx
            .add(
                me,
                "treeHeatMapStatSelect",
                DropDownControl::new(
                    Anchor::new(Left, depth_select, Right),
                    rect(8, 0, 150, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        // L257
                        let stat = value
                            .and_then(|v| v.get::<PowerStatRef>().copied())
                            .map(power_stat_table);
                        let f = tree_tab_hooks().set_power_calc;
                        f(cx, me, stat);
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L259-262
        cx.w_mut::<CheckBoxControl>(tree_heat_map).tt.tooltip_text = Some(Prop::f(|cx, _| {
            // L260
            let theme = cx.app().main.node_power_theme.clone();
            let caps = pob_lua::lmatch(&theme, "(%a+)/(%a+)", None);
            let (off_col, def_col) = match &caps {
                Some(c) if c.len() >= 2 => (c[0].to_str().into_owned(), c[1].to_str().into_owned()),
                _ => lua_error!("attempt to index local 'offCol' (a nil value)"),
            };
            // L261
            Some(format!(
                "When enabled, an estimate of the offensive and defensive strength of\neach unallocated passive is calculated and displayed visually.\nOffensive power shows as {}, defensive power as {}.",
                pob_lua::string::lower(&off_col),
                pob_lua::string::lower(&def_col)
            ))
        }));

        // L264-269
        let power_stat_list: Vec<DropItem> = model::data()
            .powerStatList
            .stats
            .iter()
            .enumerate()
            .filter(|(_, stat)| !stat.ignoreForNodes)
            .map(|(i, stat)| DropItem::new(stat.label.to_string(), Rc::new(PowerStatRef(i + 1))))
            .collect();
        st_mut(cx, me).power_stat_list = power_stat_list;

        // Show/Hide Power Report Button
        // L272-275
        cx.add(
            me,
            "powerReport",
            ButtonControl::new(
                Anchor::new(Left, stat_select, Right),
                rect(8, 0, 150, 20),
                Prop::f(move |cx, _| {
                    // L273
                    let list = ctl(cx, me, "powerReportList");
                    if cx.base(list).shown.get(cx, list) {
                        "Hide Power Report".to_owned()
                    } else {
                        "Show Power Report".to_owned()
                    }
                }),
                move |cx| {
                    // L274
                    let list = ctl(cx, me, "powerReportList");
                    let shown = cx.base(list).shown.get(cx, list);
                    cx.base_mut(list).shown = Prop::Val(!shown);
                },
            ),
        );

        // Power Report List
        // L278
        let y_pos = if raw(&cx.base(tree_heat_map).y) == 0.0 {
            raw(&cx.base(spec_select).height) + 4.0
        } else {
            raw(&cx.base(spec_select).height) * 2.0 + 8.0
        };
        // L279-287
        let jump: JumpFn = Rc::new(move |cx: &mut Cx, x: Option<f64>, y: Option<f64>| {
            // this code is called by the list control when the user "selects" one of the passives in the list.
            // we use this to set a flag which causes the next Draw() to recenter the passive tree on the desired node.
            // L282
            if let Some(x) = x {
                let s = st_mut(cx, me);
                // L283-285
                s.jump_to_node = true;
                s.jump_to_x = x;
                s.jump_to_y = y.unwrap_or(f64::NAN);
            }
        });
        let f = tree_tab_hooks().power_report_list;
        let list = f(
            cx,
            me,
            Anchor::new(TopLeft, spec_select, BottomLeft),
            [0.0.into(), y_pos.into(), 700.0.into(), 170.0.into()],
            jump,
        );
        // L288
        cx.base_mut(list).shown = Prop::Val(false);
        // Progress callback from the CalcsTab power builder coroutine
        // L290-291
        {
            let s = st_mut(cx, me);
            s.power_builder_toast_id = None;
            s.last_progress_toast_update = 0.0;
        }
        // L292-324: power_builder_progress_callback / power_builder_callback (module docs) // CCR-0123 §5

        // L326
        let convert_text = cx
            .add(
                me,
                "specConvertText",
                LabelControl::new(
                    Anchor::new(BottomLeft, spec_select, TopLeft),
                    rect(0, -14, 0, 16),
                    "^7This is an older tree version, which may not be fully compatible with the current game version.",
                ),
            )
            .id();
        // L327-329
        cx.base_mut(convert_text).shown = Prop::f(|cx, _| {
            // L328
            let m = cx.build().model.clone();
            m.treeTab.showConvert(&m)
        });
        // L330-338: get_latest_tree_version, build_convert_button_label,
        // build_convert_all_button_label
        // L339-341
        let convert = cx
            .add(
                me,
                "specConvert",
                ButtonControl::new(
                    Anchor::new(Left, convert_text, Right),
                    [
                        8.0.into(),
                        0.0.into(),
                        Prop::f(|cx, _| {
                            cx.sg.draw_string_width(16.0, Font::Var, &build_convert_button_label(cx))
                                + 20.0
                        }),
                        20.0.into(),
                    ],
                    Prop::f(|cx, _| build_convert_button_label(cx)),
                    move |cx| {
                        // L340
                        let v = get_latest_tree_version(cx);
                        Self::ConvertToVersion(cx, me, v, false, true, false);
                    },
                ),
            )
            .id();
        // L342-344
        cx.add(
            me,
            "specConvertAll",
            ButtonControl::new(
                Anchor::new(Left, convert, Right),
                [
                    8.0.into(),
                    0.0.into(),
                    Prop::f(|cx, _| {
                        cx.sg.draw_string_width(16.0, Font::Var, &build_convert_all_button_label(cx))
                            + 20.0
                    }),
                    20.0.into(),
                ],
                Prop::f(|cx, _| build_convert_all_button_label(cx)),
                move |cx| {
                    // L343
                    let v = get_latest_tree_version(cx);
                    Self::OpenVersionConvertAllPopup(cx, me, v);
                },
            ),
        );
        // L345-347
        let s = st_mut(cx, me);
        s.jump_to_node = false;
        s.jump_to_x = 0.0;
        s.jump_to_y = 0.0;
        // L348 return self
    }

    /// The specSelect tooltipFunc (L55-107).
    fn spec_select_tooltip(
        cx: &mut Cx,
        me: CtrlId,
        tooltip: &mut Tooltip,
        mode: DropTipMode,
        sel_index: Option<usize>,
    ) {
        let _ = me;
        // L56
        tooltip.clear(false);
        // L57
        if mode == DropTipMode::Out {
            return;
        }
        // L58
        let Some(spec) = sel_index.and_then(|i| spec_at(cx, i as f64)) else {
            return;
        };
        let m = cx.build().model.clone();
        // L60
        let (used, _asc_used, _secondary_asc_used, sockets, _, _) = spec.CountAllocNodes();
        let name = |n: Option<Name>| {
            n.map(|n| n.to_string()).unwrap_or_else(|| {
                lua_error!("attempt to concatenate field 'curClassName' (a nil value)")
            })
        };
        // L61-63
        tooltip.add_line(cx, 16.0, &format!("Class: {}", name(spec.curClassName.get())));
        tooltip.add_line(
            cx,
            16.0,
            &format!("Ascendancy: {}", name(spec.curAscendClassName.get())),
        );
        tooltip.add_line(cx, 16.0, &format!("Points used: {}", tostring_num(used)));
        // L64
        if sockets > 0.0 {
            // L65
            tooltip.add_line(cx, 16.0, &format!("Jewel sockets: {}", tostring_num(sockets)));
        }
        // L67
        if sel_index.map(|i| i as f64) != Some(m.treeTab.activeSpec.get()) {
            // L68
            let calc = match cx.build_mut().calc.misc_calculator(&m) {
                std::task::Poll::Ready(c) => Some(c),
                // DIVERGENCE(pending-calc): no calculator yet (wasm only)
                std::task::Poll::Pending => None,
            };
            // L69
            if let Some((calc_func, calc_base)) = calc {
                // L70
                let ov = model::Override::default();
                ov.spec.set(Some(spec.clone()));
                if let std::task::Poll::Ready(output) = calc_func.call(Some(Rc::new(ov)), None) {
                    // L71
                    crate::build::stats::add_stat_compares_to_tooltip(
                        cx,
                        tooltip,
                        &calc_base,
                        &output,
                        "^7Switching to this tree will give you:",
                        None,
                    );
                }
            }
            let build_spec = m.spec.get();
            // L73
            if spec.curClassId.get() == build_spec.curClassId.get() {
                // L74-75
                let mut respec = 0.0;
                let mut respec_ascendancy = 0.0;
                let tree = build_spec.tree.get();
                // L76 (ORDER: counting only, module docs)
                for (node_id, node) in build_spec.allocNodes.with(pob_lua::LjIntMap::pairs) {
                    // Assumption: Nodes >= 65536 are small cluster passives.
                    let ty = node.r#type.get();
                    // L78-80
                    if &*ty != "ClassStart"
                        && &*ty != "AscendClassStart"
                        && (tree.clusterNodeMap.get(&*node.dn.get()).is_nil()
                            || node.isKeystone.get()
                            || node.isJewelSocket.get())
                        && node_id < 65536
                        && spec.allocNodes.with(|a| a.get(node_id).is_none())
                    {
                        // L81
                        if node.ascendancyName.get().is_some() {
                            // L82
                            respec_ascendancy += 1.0;
                        } else {
                            // L84
                            respec += 1.0;
                        }
                    }
                }
                // L88
                if respec > 0.0 || respec_ascendancy > 0.0 {
                    // L89 (`build.characterLevel`: the constructor's `build` argument)
                    let level = m.characterLevel.get();
                    let gold_cost = (level >= 1.0 && level.fract() == 0.0)
                        .then(|| model::data().goldRespecPrices.get(level as usize - 1).copied())
                        .flatten()
                        .unwrap_or_else(|| {
                            lua_error!(
                                "attempt to perform arithmetic on local 'goldCost' (a nil value)"
                            )
                        });
                    // L90
                    let total_gold = (respec * gold_cost) + (respec_ascendancy * gold_cost * 5.0);
                    // L91
                    let gold_str = format_num_sep_main(&cx.main, &tostring_num(total_gold));
                    // L92
                    tooltip.add_line(
                        cx,
                        16.0,
                        &format!("^xFFD700{gold_str} Gold ^7required to switch to this tree."),
                    );
                    // L93
                    if respec > 0.0 {
                        // L94
                        let node_word = if respec == 1.0 {
                            "Passive node to be refunded"
                        } else {
                            "Passive nodes to be refunded"
                        };
                        // L95
                        tooltip.add_line(cx, 16.0, &pob_lua::lfmt!("^7\t%d %s.", respec, node_word));
                    }
                    // L97
                    if respec_ascendancy > 0.0 {
                        // L98
                        let ascend_word = if respec_ascendancy == 1.0 {
                            "Ascendancy node to be refunded"
                        } else {
                            "Ascendancy nodes to be refunded"
                        };
                        // L99
                        tooltip.add_line(
                            cx,
                            16.0,
                            &pob_lua::lfmt!("^7\t%d %s.", respec_ascendancy, ascend_word),
                        );
                    }
                }
            }
        }
        // L104
        let tv = spec.treeVersion.get();
        tooltip.add_line(cx, 16.0, &format!("^7Game Version: {}", tree_version_display(&tv)));
    }

    /// The nodePowerMaxDepthSelect selFunc (L212-241).
    fn node_power_max_depth_sel(cx: &mut Cx, me: CtrlId, value: Option<&DropItem>) {
        let m = cx.build().model.clone();
        let depth_select = ctl(cx, me, "nodePowerMaxDepthSelect");
        let depth_custom = ctl(cx, me, "nodePowerMaxDepthCustom");
        let stat_select = ctl(cx, me, "treeHeatMapStatSelect");
        let value_num = value.and_then(|v| v.get::<f64>().copied());
        // Show custom value control and resize/move elements
        // L214
        let custom = value.and_then(DropItem::as_str) == Some("Custom");
        st_mut(cx, me).is_custom_max_depth = custom;
        // L215
        if custom {
            // L216
            cx.base_mut(depth_select).width = Prop::Val(70.0);
            // L217
            cx.base_mut(depth_custom).shown = Prop::Val(true);
            // L218
            cx.set_anchor(stat_select, Left, Some(depth_custom), Right, None, false);
            // L219
            return;
        }
        // L222
        cx.base_mut(depth_select).width = Prop::Val(55.0);
        // L223
        cx.base_mut(depth_custom).shown = Prop::Val(false);
        // L224
        cx.set_anchor(stat_select, Left, Some(depth_select), Right, None, false);

        // L226
        let old_max = power::nodePowerMaxDepth(&m);

        // L228
        if let Some(v) = value_num {
            // L229
            power::set_nodePowerMaxDepth(&m, Some(v));
        } else {
            // L231
            power::set_nodePowerMaxDepth(&m, None);
        }

        // If the heat map is shown, tell it to recalculate
        // if the new value is larger than the old
        // L236 (`oldMax ~= value`: the string "All" never equals a number or nil)
        let differs = match (old_max, value_num) {
            (Some(o), Some(v)) => o != v,
            _ => true,
        };
        if differs && st(cx, me).viewer.show_heat_map {
            let cur = power::nodePowerMaxDepth(&m);
            // L237
            if let Some(old_max) = old_max {
                if cur.map_or(true, |c| c > old_max) {
                    // L238
                    let f = tree_tab_hooks().set_power_calc;
                    f(cx, me, power::powerStat(&m));
                }
            }
        }
    }

    /// The reset button's popup (L134-157).
    fn open_reset_popup(cx: &mut Cx, me: CtrlId) {
        // L134 local controls = { }
        // L135
        let button_y = 65;
        // L157
        popups::open_popup(
            cx,
            570.0,
            100.0,
            "Reset Tree/Tattoos",
            move |cx, p| {
                let p = p.id();
                // L136
                cx.add(
                    p,
                    "warningLabel",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 30, 0, 16),
                        "^7Warning: resetting your passive tree or removing all tattoos cannot be undone.\n",
                    ),
                );
                // L137-143
                cx.add(
                    p,
                    "reset",
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(-130, button_y, 100, 20),
                        "Reset Tree",
                        move |cx| {
                            let spec = cx.build().model.spec.get();
                            // L138
                            spec.ResetNodes();
                            // L139
                            spec.BuildAllDependsAndPaths();
                            // L140
                            SPEC_UNDO.add_undo_state(cx, me, false);
                            // L141
                            cx.build().model.buildFlag.set(true);
                            // L142
                            popups::close_popup(cx);
                        },
                    ),
                );
                // L144-153
                cx.add(
                    p,
                    "removeTattoo",
                    ButtonControl::new(
                        Anchor::NONE,
                        rect(0, button_y, 144, 20),
                        "Remove All Tattoos",
                        move |cx| {
                            let spec = cx.build().model.spec.get();
                            // L145 hashOverrides will contain only the nodes that have been tattoo-ed
                            for (id, node) in spec.hashOverrides.with(pob_lua::LjIntMap::pairs) {
                                // (`pairs` skips a key whose value went nil during the loop)
                                if spec.hashOverrides.with(|h| h.get(id).is_none()) {
                                    continue;
                                }
                                // L146
                                if node.isTattoo.get() {
                                    // L147
                                    let target =
                                        spec.nodes.with(|n| n.get(id)).unwrap_or_else(|| {
                                            lua_error!(
                                                "attempt to index local 'node' (a nil value)"
                                            )
                                        });
                                    Self::RemoveTattooFromNode(cx, me, &target);
                                }
                            }
                            // L150
                            st_mut(cx, me).mod_flag = true;
                            // L151
                            cx.build().model.buildFlag.set(true);
                            // L152
                            popups::close_popup(cx);
                        },
                    ),
                );
                // L154-156
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(130, button_y, 100, 20), "Cancel", |cx| {
                        // L155
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts {
                default: Some("edit".into()),
                escape: Some("cancel".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: TreeTab.lua:351-357 `TreeTabClass:RemoveTattooFromNode(node)`
    /// @pob Classes/TreeTab.lua:351-357 h=a1005184
    pub fn RemoveTattooFromNode(cx: &mut Cx, _me: CtrlId, node: &NodeRef) {
        let spec = cx.build().model.spec.get();
        let tree = spec.tree.get();
        let id = node.id as i64;
        let tree_node = tree
            .nodes
            .get(id)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        // L352
        tree_node.isTattoo.set(false);
        // L353
        spec.hashOverrides.with_mut(|h| h.set(id, None));
        // L354
        spec.ReplaceNode(node, &tree_node);
        // L355
        node.allMasteryOptions.set(false);
        // L356
        spec.BuildAllDependsAndPaths();
    }

    /// lua: TreeTab.lua:359-490 `TreeTabClass:Draw(viewPort, inputEvents)`
    /// @pob Classes/TreeTab.lua:359-490 h=204d61f5
    pub fn draw(cx: &mut Cx, me: CtrlId, view_port: &ViewPort, input_events: &mut InputEvents) {
        let anchor = st(cx, me).anchor_controls;
        {
            let b = cx.base_mut(anchor);
            // L360
            b.x = Prop::Val(view_port.x + 4.0);
            // L361
            b.y = Prop::Val(view_port.y + view_port.height - 24.0);
        }
        let spec_select = ctl(cx, me, "specSelect");

        // L363
        let mut i = 0;
        while let Some(event) = input_events.next_ipairs(&mut i) {
            // L364
            if let EventKind::KeyDown { .. } = event.kind {
                // L365
                if event.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L366
                    SPEC_UNDO.undo(cx, me);
                    // L367
                    cx.build().model.buildFlag.set(true);
                    // L368
                    input_events.consume(i);
                } else if event.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L370
                    SPEC_UNDO.redo(cx, me);
                    // L371
                    cx.build().model.buildFlag.set(true);
                    // L372
                    input_events.consume(i);
                } else if event.key == Key::Up {
                    // L374
                    let index = cx.build().model.treeTab.activeSpec.get() - 1.0;
                    // L375
                    if spec_at(cx, index).is_some()
                        && !cx.is_mouse_over(spec_select)
                        && !cx.w::<DropDownControl>(spec_select).dropped
                    {
                        // L376
                        cx.build_mut().mod_flag = true;
                        // L377
                        Self::SetActiveSpec(cx, me, index);
                    }
                } else if event.key == Key::Down
                    && !cx.is_mouse_over(spec_select)
                    && !cx.w::<DropDownControl>(spec_select).dropped
                {
                    // L380
                    let index = cx.build().model.treeTab.activeSpec.get() + 1.0;
                    // L381
                    if spec_at(cx, index).is_some() {
                        // L382
                        cx.build_mut().mod_flag = true;
                        // L383
                        Self::SetActiveSpec(cx, me, index);
                    }
                } else if event.key == Key::Char(b'f') && cx.sys.is_key_down(Key::Ctrl) {
                    // L386
                    let s = ctl(cx, me, "treeSearch");
                    cx.select_control(me, Some(s));
                } else if event.key == Key::Char(b'm') && cx.sys.is_key_down(Key::Ctrl) {
                    // L388
                    Self::OpenSpecManagePopup(cx, me);
                }
            }
        }
        // L392
        cx.process_controls_input(me, input_events, view_port);

        let c = |cx: &Cx, k: &'static str| ctl(cx, me, k);
        let compare_check = c(cx, "compareCheck");
        let reset = c(cx, "reset");
        let version_text = c(cx, "versionText");
        let version_select = c(cx, "versionSelect");
        let compare_select = c(cx, "compareSelect");
        let tree_search = c(cx, "treeSearch");
        let find_timeless = c(cx, "findTimelessJewel");
        let tree_heat_map = c(cx, "treeHeatMap");
        let depth_select = c(cx, "nodePowerMaxDepthSelect");
        let depth_custom = c(cx, "nodePowerMaxDepthCustom");
        let stat_select = c(cx, "treeHeatMapStatSelect");
        let power_report = c(cx, "powerReport");
        let power_report_list = c(cx, "powerReportList");
        let spec_convert = c(cx, "specConvert");
        let spec_convert_text = c(cx, "specConvertText");
        let bw = |cx: &Cx, id: CtrlId| raw(&cx.base(id).width);
        let bx = |cx: &Cx, id: CtrlId| raw(&cx.base(id).x);
        let bh = |cx: &Cx, id: CtrlId| raw(&cx.base(id).height);

        // Determine positions if one line of controls doesn't fit in the screen width
        // L395-396
        let mut lines_height = 24.0;
        let right_margin = 10.0;
        let (is_comparing, is_custom_max_depth, show_heat_map) = {
            let s = st(cx, me);
            (s.is_comparing, s.is_custom_max_depth, s.viewer.show_heat_map)
        };
        // L397-402
        let width_first_line_controls = bw(cx, spec_select)
            + 8.0
            + bw(cx, compare_check)
            + bx(cx, compare_check)
            + bw(cx, reset)
            + bx(cx, reset)
            + cx.width(version_text)
            + bx(cx, version_text)
            + bw(cx, version_select)
            + bx(cx, version_select)
            + if is_comparing {
                bw(cx, compare_select) + bx(cx, compare_select)
            } else {
                0.0
            };

        // L404-410
        let width_second_line_controls = bw(cx, tree_search)
            + 8.0
            + bw(cx, find_timeless)
            + bx(cx, find_timeless)
            + bw(cx, tree_heat_map)
            + 130.0
            + bw(cx, depth_select)
            + bx(cx, depth_select)
            + if is_custom_max_depth {
                bw(cx, depth_custom) + bx(cx, depth_custom)
            } else {
                0.0
            }
            + if show_heat_map {
                bw(cx, stat_select)
                    + bx(cx, stat_select)
                    + bw(cx, power_report)
                    + bx(cx, power_report)
            } else {
                0.0
            };

        let xy = |x: f64, y: f64| Some((Prop::Val(x), Prop::Val(y)));
        // Check first line
        // L413
        if view_port.width >= width_first_line_controls + width_second_line_controls + right_margin {
            // L414
            lines_height = 0.0;
            // L415
            cx.set_anchor(tree_search, Left, Some(version_select), Right, xy(8.0, 0.0), false);
            // L416
            let y = bh(cx, spec_select) + 6.0;
            cx.set_anchor(power_report_list, TopLeft, Some(spec_select), BottomLeft, xy(0.0, y), false);
        } else {
            // L418
            cx.set_anchor(tree_search, TopLeft, Some(spec_select), BottomLeft, xy(0.0, 4.0), false);
            // L419
            let y = bh(cx, tree_search) + 6.0;
            cx.set_anchor(power_report_list, TopLeft, Some(tree_search), BottomLeft, xy(0.0, y), false);
        }

        // Check second line
        // L423
        if view_port.width >= width_second_line_controls + right_margin {
            // L424
            cx.set_anchor(tree_heat_map, Left, Some(find_timeless), Right, xy(130.0, 0.0), false);
        } else {
            // L426
            lines_height *= 2.0;
            // L427
            cx.set_anchor(tree_heat_map, TopLeft, Some(tree_search), BottomLeft, xy(124.0, 4.0), false);
            // L428
            let y = bh(cx, tree_heat_map) + 6.0;
            cx.set_anchor(power_report_list, TopLeft, Some(tree_heat_map), BottomLeft, xy(-124.0, y), false);
        }

        // determine positions for convert line of controls
        // L432-433
        let mut convert_two_line_height = 24.0;
        let convert_max_width = 900.0;
        // L434
        if view_port.width >= convert_max_width {
            // L435
            convert_two_line_height = 0.0;
            // L436
            cx.set_anchor(spec_convert, Left, Some(spec_convert_text), Right, xy(8.0, 0.0), false);
            // L437
            cx.set_anchor(spec_convert_text, BottomLeft, Some(spec_select), TopLeft, xy(0.0, -14.0), false);
        } else {
            // L439
            cx.set_anchor(spec_convert, TopLeft, Some(spec_convert_text), BottomLeft, xy(0.0, 4.0), false);
            // L440
            cx.set_anchor(spec_convert_text, BottomLeft, Some(spec_select), TopLeft, xy(0.0, -38.0), false);
        }

        // L443 (the raw `shown` field: a boolean)
        let bottom_drawer_height = if cx.base(power_report_list).shown.get(cx, power_report_list) {
            194.0
        } else {
            0.0
        };
        // L444
        cx.base_mut(spec_select).y = Prop::Val(-bottom_drawer_height - lines_height);

        let m = cx.build().model.clone();
        let show_convert = m.treeTab.showConvert(&m);
        // L446
        let tree_view_port = ViewPort {
            x: view_port.x,
            y: view_port.y,
            width: view_port.width,
            height: view_port.height
                - if show_convert {
                    64.0 + bottom_drawer_height + lines_height
                } else {
                    32.0 + bottom_drawer_height + lines_height
                },
        };
        // L447
        if st(cx, me).jump_to_node {
            let (x, y) = (st(cx, me).jump_to_x, st(cx, me).jump_to_y);
            // L448
            cx.lend::<TreeTab, _, _>(me, |w| &mut w.core.viewer, |cx, v| {
                v.focus(cx, x, y, &tree_view_port);
            });
            // L449
            st_mut(cx, me).jump_to_node = false;
        }
        // L451
        let compare_spec = if st(cx, me).is_comparing {
            spec_at(cx, st(cx, me).active_compare_spec)
        } else {
            None
        };
        st_mut(cx, me).viewer.compare_spec = compare_spec;
        // L452
        cx.lend::<TreeTab, _, _>(me, |w| &mut w.core.viewer, |cx, v| {
            v.draw(cx, &tree_view_port, input_events);
        });

        // L454
        let new_spec_list = Self::GetSpecList(cx);
        // L455
        let active_compare = st(cx, me).active_compare_spec;
        cx.w_mut::<DropDownControl>(compare_select).sel_index = active_compare as usize;
        // L456
        let items: Vec<DropItem> = new_spec_list.iter().map(DropItem::str).collect();
        DropDownControl::set_list(cx, compare_select, items);
        // L457
        let mut with_manage: Vec<DropItem> = new_spec_list.iter().map(DropItem::str).collect();
        with_manage.push(DropItem::str("Manage trees... (ctrl-m)"));
        // the compare dropdown aliases `newSpecList` (module docs)
        cx.w_mut::<DropDownControl>(compare_select).list = with_manage.clone();
        // L458
        let active = m.treeTab.activeSpec.get();
        cx.w_mut::<DropDownControl>(spec_select).sel_index = active as usize;
        // L459
        DropDownControl::set_list(cx, spec_select, with_manage);

        // L461
        if cx.base(tree_search).has_focus != Some(true) {
            // L462
            let s = st(cx, me).viewer.search_str.clone();
            EditControl::set_text(cx, tree_search, &s, false);
        }

        let show_heat_map = st(cx, me).viewer.show_heat_map;
        // L465
        cx.w_mut::<CheckBoxControl>(tree_heat_map).state = show_heat_map;
        // L466
        cx.base_mut(stat_select).shown = Prop::Val(show_heat_map);
        // L467
        let list = st(cx, me).power_stat_list.clone();
        cx.w_mut::<DropDownControl>(stat_select).list = list;
        // L468
        cx.w_mut::<DropDownControl>(stat_select).sel_index = 1;
        // L469
        DropDownControl::check_dropped_width(cx, stat_select, true);
        // L470
        if let Some(power_stat) = power::powerStat(&m) {
            // L471
            let stat = power_stat.get(n!("stat"));
            DropDownControl::sel_by_value(cx, stat_select, &|item: &DropItem| {
                let item_stat = item
                    .get::<PowerStatRef>()
                    .and_then(|r| model::data().powerStatList.stats[r.0 - 1].stat);
                match (&stat, item_stat) {
                    (Val::Nil, None) => true,
                    (Val::Str(s), Some(n)) => **s == *n,
                    _ => false,
                }
            });
        }

        // L474
        cx.sg.set_draw_layer(Some(1), None);

        // L476
        cx.sg.set_draw_color(0.05, 0.05, 0.05);
        // L477
        cx.sg.draw_image(
            None,
            view_port.x,
            view_port.y + view_port.height - (30.0 + bottom_drawer_height + lines_height),
            view_port.width,
            30.0 + bottom_drawer_height + lines_height,
        );
        // L478
        if show_convert {
            // L479
            let height = if view_port.width < convert_max_width {
                bottom_drawer_height + lines_height
            } else {
                0.0
            };
            // L480
            cx.sg.set_draw_color(0.05, 0.05, 0.05);
            // L481
            cx.sg.draw_image(
                None,
                view_port.x,
                view_port.y + view_port.height
                    - (60.0 + bottom_drawer_height + lines_height + convert_two_line_height),
                view_port.width,
                28.0 + height,
            );
            // L482
            cx.sg.set_draw_color(0.85, 0.85, 0.85);
            // L483
            cx.sg.draw_image(
                None,
                view_port.x,
                view_port.y + view_port.height
                    - (64.0 + bottom_drawer_height + lines_height + convert_two_line_height),
                view_port.width,
                4.0,
            );
        }
        // let white lines overwrite the black sections, regardless of showConvert
        // L486
        cx.sg.set_draw_color(0.85, 0.85, 0.85);
        // L487
        cx.sg.draw_image(
            None,
            view_port.x,
            view_port.y + view_port.height - (34.0 + bottom_drawer_height + lines_height),
            view_port.width,
            4.0,
        );

        // L489
        cx.draw_controls(me, view_port, None);
    }

    /// lua: TreeTab.lua:492-498 `TreeTabClass:GetSpecList()`
    /// @pob Classes/TreeTab.lua:492-498 h=0067215a
    pub fn GetSpecList(cx: &Cx) -> Vec<String> {
        let gv = &model::data().gameVersions;
        // L493
        let mut new_spec_list = Vec::new();
        // L494
        for spec in cx.build().model.treeTab.specList.snapshot() {
            let tv = spec.treeVersion.get();
            // L495
            let prefix = if tv != gv.latestTreeVersion {
                format!("[{}] ", tree_version_display(&tv))
            } else {
                String::new()
            };
            let title = spec
                .title
                .get()
                .map_or_else(|| "Default".to_owned(), |s| s.to_string());
            new_spec_list.push(format!("{prefix}{title}"));
        }
        // L497
        new_spec_list
    }

    /// `TreeTab:SetActiveSpec(specId)` (TreeTab.lua:549-585): calc:BM-A's model half, then the
    /// UI lines 556-557 and 578-584 (DIVERGENCE(set-active-spec-order), module docs).
    pub fn SetActiveSpec(cx: &mut Cx, me: CtrlId, spec_id: f64) {
        let m = cx.build().model.clone();
        // L550-555, L558-577 (model part: calc:BM-A)
        m.treeTab.SetActiveSpec(&m, spec_id);
        // L556 self.build.spec:SetWindowTitleWithBuildClass()
        set_window_title_with_build_class(cx);
        // L557 self.build:UpdateClassDropdowns(curSpec.treeVersion)
        let h = init::hooks(cx);
        let tv = m.spec.get().treeVersion.get();
        (h.update_class_dropdowns)(cx, Some(tv));
        // L574-577 (the UI part of itemsTab:PopulateSlots(), U32)
        if m.itemsTab.orderList.get(1).is_some() {
            crate::tabs::items::slot_helper::populate_slot_lists(cx);
        }
        // Update the passive tree dropdown control in itemsTab
        // L579 (DIVERGENCE(items-spec-select))
        if let Some(sel) = items_spec_select(cx) {
            cx.w_mut::<DropDownControl>(sel).sel_index = spec_id as usize;
        }
        // Update Version dropdown to active spec's
        // L581
        if let Some(version_select) = cx.child(me, "versionSelect") {
            // L582
            Self::sel_version(cx, version_select, &tv);
        }
        // L584
        let f = h.sync_loadouts.clone();
        f(cx);
    }

    /// `self.controls.versionSelect:SelByValue(version, 'value')`
    pub fn sel_version(cx: &mut Cx, version_select: CtrlId, version: &Name) {
        let v = *version;
        DropDownControl::sel_by_value(cx, version_select, &move |item: &DropItem| {
            item.get::<Name>() == Some(&v)
        });
    }

    /// lua: TreeTab.lua:587-592 `TreeTabClass:SetCompareSpec(specId)`
    /// @pob Classes/TreeTab.lua:587-592 h=5e87fd90
    pub fn SetCompareSpec(cx: &mut Cx, me: CtrlId, spec_id: f64) {
        // L588
        let a = m_min(spec_id, spec_count(cx));
        st_mut(cx, me).active_compare_spec = a;
        // L589
        let cur_spec = spec_at(cx, a);
        // L591
        st_mut(cx, me).compare_spec = cur_spec;
    }

    /// lua: TreeTab.lua:594-618 `TreeTabClass:ConvertToVersion(version, remove, success,
    /// ignoreTreeSubType)`
    /// @pob Classes/TreeTab.lua:594-618 h=7cde3339
    pub fn ConvertToVersion(
        cx: &mut Cx,
        me: CtrlId,
        version: Name,
        remove: bool,
        success: bool,
        ignore_tree_sub_type: bool,
    ) {
        let m = cx.build().model.clone();
        let mut version = version;
        let build_spec = m.spec.get();
        // L595
        let tree_sub_type_capture =
            pob_ui_core::luastr::lmatch(&build_spec.treeVersion.get(), "(_%l+_?%l*)");
        // L596
        if let Some(cap) = tree_sub_type_capture.as_deref() {
            if !ignore_tree_sub_type && pattern::find(&version, cap, None, false).is_none() {
                // L597
                let cand = format!("{version}{cap}");
                if model::data()
                    .gameVersions
                    .treeVersionList
                    .iter()
                    .any(|v| **v == *cand)
                {
                    // L598
                    version = Name::from(cand.as_str());
                }
            }
        }
        // L601
        let new_spec = new_PassiveSpec(&m, version, false);
        // L602
        new_spec.title.set(build_spec.title.get());
        // L603
        let jewels = build_spec.jewels.with(Clone::clone);
        new_spec.jewels.with_mut(|j| *j = jewels);
        // L604
        let state = build_spec.CreateUndoState();
        new_spec.RestoreUndoState(&state, Some(version));
        flush_window_titles(cx);
        // L605
        new_spec.BuildClusterJewelGraphs();
        // L606
        let active = m.treeTab.activeSpec.get();
        m.treeTab.specList.with_mut(|l| {
            let i = active as usize;
            if i > l.len() {
                lua_error!("bad argument #2 to 'insert' (position out of bounds)");
            }
            l.insert(i, new_spec);
        });
        // L607
        if remove {
            // L608
            m.treeTab.specList.with_mut(|l| {
                let i = active as usize;
                if i >= 1 && i <= l.len() {
                    l.remove(i - 1);
                }
            });
            // activeSpec + 1 is shifted down one on remove, otherwise we would set the spec below it if it exists
            // L610
            Self::SetActiveSpec(cx, me, active);
        } else {
            // L612
            Self::SetActiveSpec(cx, me, active + 1.0);
        }
        // L614
        st_mut(cx, me).mod_flag = true;
        // L615
        if success {
            // L616
            popups::open_message_popup(
                cx,
                "Tree Converted",
                &format!(
                    "The tree has been converted to {}.\nNote that some or all of the passives may have been de-allocated due to changes in the tree.\n\nYou can switch back to the old tree using the tree selector at the bottom left.",
                    tree_version_display(&version)
                ),
            );
        }
    }

    /// lua: TreeTab.lua:620-633 `TreeTabClass:ConvertAllToVersion(version)`
    /// @pob Classes/TreeTab.lua:620-633 h=a373b649
    pub fn ConvertAllToVersion(cx: &mut Cx, me: CtrlId, version: Name) {
        let m = cx.build().model.clone();
        // L621
        let curr_active_spec = m.treeTab.activeSpec.get();
        // L622-625
        let spec_version_list: Vec<Name> = m
            .treeTab
            .specList
            .snapshot()
            .iter()
            .map(|s| s.treeVersion.get())
            .collect();
        // L626
        for (index, spec_version) in spec_version_list.iter().enumerate() {
            // L627
            if *spec_version != version {
                // L628
                Self::SetActiveSpec(cx, me, (index + 1) as f64);
                // L629
                Self::ConvertToVersion(cx, me, version, true, false, false);
            }
        }
        // L632
        Self::SetActiveSpec(cx, me, curr_active_spec);
    }

    /// lua: TreeTab.lua:1001-1018 `TreeTabClass:SaveMasteryPopup(node, listControl)`
    /// (`listControl.selValue.id`: `sel_value_id`)
    /// @pob Classes/TreeTab.lua:1001-1018 h=3f4b6bd5
    pub fn SaveMasteryPopup(cx: &mut Cx, me: CtrlId, node: &NodeRef, sel_value_id: Option<f64>) {
        // L1002
        let Some(effect_id) = sel_value_id else {
            // L1003
            return;
        };
        let spec = cx.build().model.spec.get();
        let tree = spec.tree.get();
        // L1005
        let effect = tree
            .masteryEffects
            .get(effect_id)
            .obj::<model::Node>()
            .unwrap_or_else(|| lua_error!("attempt to index local 'effect' (a nil value)"));
        // L1006
        node.sd.set(effect.sd.get());
        // L1007
        node.allMasteryOptions.set(false);
        // L1008
        let reminder = Table::new();
        reminder.push("Tip: Right click to select a different effect");
        node.extra.set(n!("reminderText"), reminder);
        // L1009
        tree.ProcessStats(node, None);
        // L1010
        spec.masterySelections
            .with_mut(|s| s.set(node.id as i64, Some(effect.id)));
        // L1011
        if !node.alloc.get() {
            // L1012 (`self.viewer.tracePath and node == tracePath[#tracePath] and tracePath`)
            let trace_path = st(cx, me).viewer.trace_path.clone();
            let alt = trace_path
                .filter(|tp| tp.with(|v| v.last().is_some_and(|last| Rc::ptr_eq(last, node))));
            spec.AllocNode(node, alt.as_deref());
        }
        // L1014
        SPEC_UNDO.add_undo_state(cx, me, false);
        // L1015
        st_mut(cx, me).mod_flag = true;
        // L1016
        cx.build().model.buildFlag.set(true);
        // L1017
        popups::close_popup(cx);
    }
}

/// `getLatestTreeVersion()` (L330-332)
fn get_latest_tree_version(cx: &Cx) -> Name {
    let latest = model::data().gameVersions.latestTreeVersion;
    let m = &cx.build().model;
    // L331
    let spec = spec_at(cx, m.treeTab.activeSpec.get())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let suffix = pob_ui_core::luastr::lmatch(&spec.treeVersion.get(), &format!("^{latest}(.*)"))
        .unwrap_or_default();
    Name::from(format!("{latest}{suffix}").as_str())
}

/// `buildConvertButtonLabel()` (L333-335)
fn build_convert_button_label(cx: &Cx) -> String {
    // L334
    format!(
        "{}Convert to {}",
        cx.main.cc.POSITIVE,
        tree_version_display(&get_latest_tree_version(cx))
    )
}

/// `buildConvertAllButtonLabel()` (L336-338)
fn build_convert_all_button_label(cx: &Cx) -> String {
    // L337
    format!(
        "{}Convert all trees to {}",
        cx.main.cc.POSITIVE,
        tree_version_display(&get_latest_tree_version(cx))
    )
}

/// `self.build.powerBuilderProgressCallback(percent)` (L292-312; module docs: U26 calls it).
pub fn power_builder_progress_callback(cx: &mut Cx, me: CtrlId, percent: Option<f64>) {
    // L293
    let now = cx.sys.time_ms() as f64;
    // L294
    if now - st(cx, me).last_progress_toast_update < 100.0 {
        // L295
        return;
    }
    // L298
    let message = match percent {
        Some(p) => pob_lua::lfmt!("Building Power Report... (%d%%)", p),
        None => "Building Power Report...".to_owned(),
    };
    // L300
    let list = ctl(cx, me, "powerReportList");
    let f = tree_tab_hooks().power_report_list_set_label;
    f(cx, list, &message);
    let toast_id = st(cx, me).power_builder_toast_id.clone();
    // L302
    if toast_id
        .as_deref()
        .is_some_and(|id| toast::was_dismissed(cx, id))
    {
        // L303
        return;
    }
    // L305
    st_mut(cx, me).last_progress_toast_update = now;
    // L307
    if let Some(id) = toast_id.as_deref().filter(|id| toast::exists(cx, id)) {
        // L308
        toast::update(cx, id, &message);
    } else {
        // L310
        let id = toast::add(cx, &message);
        st_mut(cx, me).power_builder_toast_id = Some(id);
    }
}

/// `self.build.powerBuilderCallback()` (L314-324; module docs: U26 calls it).
pub fn power_builder_callback(cx: &mut Cx, me: CtrlId) {
    let m = cx.build().model.clone();
    // L315
    let power_stat = power::powerStat(&m)
        .unwrap_or_else(|| power::powerStat_table(&model::data().powerStatList.stats[0]));
    // L316-317
    let f = tree_tab_hooks().power_report_set;
    f(cx, me, power_stat);
    // L319
    if let Some(id) = st(cx, me).power_builder_toast_id.clone() {
        // L320
        toast::clear_dismissed(cx, Some(&id));
        // L321
        toast::remove(cx, &id, false);
        // L322
        st_mut(cx, me).power_builder_toast_id = None;
    }
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1, CCR-0106 §2) -----------------------------

/// `self.treeTab` of the open build, when the tab exists.
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.tree.id();
    cx.ui.try_w::<TreeTab>(id).map(|_| id)
}

fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'treeTab' (a nil value)"))
}

/// `BuildHooks::tree_tab`: `self.treeTab = new("TreeTab"):TreeTab(self)` (Build.lua:613), then
/// the `ResetUndo` tail of the spec loads that already ran (DIVERGENCE(reset-undo)).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = cx.add_free(Some(build), TreeTab::new(build)).id();
    let m = cx.build().model.clone();
    let sections = build_xml::xmlSectionList(&m);
    let is_spec = |c: &XmlChild| matches!(c, XmlChild::Node(n) if &*n.elem == "Spec");
    // PassiveSpec:Load ran for the list's specs: the legacy `Spec` section (L502-505), or a
    // `Tree` section with `Spec` children (L510-521). A `Tree` without them leaves the L522-524
    // fallback spec, which never loads, with its empty buffers.
    let specs_loaded = sections.iter().any(|n| match &*n.elem {
        "Spec" => true,
        "Tree" => n.children.iter().any(is_spec),
        _ => false,
    });
    if specs_loaded {
        // PassiveSpec.lua:194 self:ResetUndo() of each loaded spec // CCR-0123 §2
        for spec in m.treeTab.specList.snapshot() {
            spec_reset_undo(cx, me, &spec);
        }
    }
    me
}

/// The UI lines 578-583 of the `SetActiveSpec` at the end of a deferred `Tree` section's
/// `TreeTab:Load` (L526, from Build.lua:671-676; DIVERGENCE(load-version-select)). `spec_id` is
/// L526's raw `tonumber(xml.attrib.activeSpec) or 1` (not clamped by L551's `m_min`). The
/// legacy `Spec` branch returns at L508, before L526: Init calls this for `Tree` sections only
/// (otherwise versionSelect keeps L179's selIndex and the ItemsTab dropdown its L46 one).
pub fn load_set_active_spec_ui_tail(cx: &mut Cx, spec_id: f64) {
    let Some(me) = tab(cx) else { return };
    // Update the passive tree dropdown control in itemsTab
    // L579 (DIVERGENCE(items-spec-select))
    if let Some(sel) = items_spec_select(cx) {
        cx.w_mut::<DropDownControl>(sel).sel_index = spec_id as usize;
    }
    // Update Version dropdown to active spec's
    // L581
    if let Some(version_select) = cx.child(me, "versionSelect") {
        // L582
        let tv = cx.build().model.spec.get().treeVersion.get();
        TreeTab::sel_version(cx, version_select, &tv);
    }
}

/// `ShellHooks::draw_tab` for `BuildTab::Tree`: `self.treeTab:Draw(tabViewPort, inputEvents)`
/// (Build.lua:1309).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    TreeTab::draw(cx, me, vp, ev);
}

/// `self.treeTab.modFlag`
pub fn mod_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| st(cx, me).mod_flag)
}

/// `self.treeTab.modFlag = v` (ResetModFlags, Build.lua:1105)
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        st_mut(cx, me).mod_flag = v;
    }
}

/// `self.treeTab.searchFlag`
pub fn search_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| st(cx, me).search_flag)
}

/// `self.treeTab.searchFlag = v` (ResetModFlags, Build.lua:1106)
pub fn set_search_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        st_mut(cx, me).search_flag = v;
    }
}

/// `self.spec.modFlag` (TabFlag::Spec)
pub fn spec_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| spec_mod_flag(cx, me))
}

/// `self.spec.modFlag = v` (TabFlag::Spec)
pub fn set_spec_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        set_spec_mod_flag(cx, me, v);
    }
}

/// `BuildHooks::spec_add_undo_state`: `self.spec:AddUndoState()`.
pub fn spec_add_undo_state_hook(cx: &mut Cx) {
    let me = tab_or_err(cx);
    SPEC_UNDO.add_undo_state(cx, me, false);
}

/// `BuildHooks::tree_get_spec_list`: `self.treeTab:GetSpecList()`.
pub fn get_spec_list_hook(cx: &Cx) -> Vec<String> {
    TreeTab::GetSpecList(cx)
}

/// `BuildHooks::tree_set_active_spec`: `self.treeTab:SetActiveSpec(specId)`.
pub fn set_active_spec_hook(cx: &mut Cx, spec_id: f64) {
    let me = tab_or_err(cx);
    TreeTab::SetActiveSpec(cx, me, spec_id);
}

/// `BuildHooks::load_ui_section` for `TreeView` (Build.lua:634 `self.treeTab.viewer`).
pub fn load_ui_section(cx: &mut Cx, node: &model::XmlNode, db: Option<&str>) -> bool {
    let me = tab_or_err(cx);
    cx.lend::<TreeTab, _, _>(me, |w| &mut w.core.viewer, |_, v| v.load(node, db));
    false
}

/// `ShellHooks::save_ui_section` for `TreeView`.
pub fn save_ui_section(cx: &mut Cx) -> Option<model::XmlNode> {
    let me = tab(cx)?;
    let mut node = model::XmlNode {
        elem: "TreeView".into(),
        attrib: Default::default(),
        children: Vec::new(),
    };
    cx.lend::<TreeTab, _, _>(me, |w| &mut w.core.viewer, |_, v| v.save(&mut node));
    Some(node)
}

/// `TreeViewHooks::open_mastery_popup`: `build.treeTab:OpenMasteryPopup(node, viewPort)`.
pub fn open_mastery_popup_hook(cx: &mut Cx, node: &NodeRef, vp: &ViewPort) {
    let me = tab_or_err(cx);
    TreeTab::OpenMasteryPopup(cx, me, node, vp);
}

/// `TreeViewHooks::modify_node_popup`: `build.treeTab:ModifyNodePopup(node, viewPort)`.
pub fn modify_node_popup_hook(cx: &mut Cx, node: &NodeRef, _vp: &ViewPort) {
    let me = tab_or_err(cx);
    TreeTab::ModifyNodePopup(cx, me, node);
}
