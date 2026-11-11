//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SkillsTab.lua:1-742, 1081-1502 (model halves of 441-630, 1129-1274, 1478-1502 are calc:BM-B).
//! Owner: U30.
//!
//! @pob-file Classes/SkillsTab.lua @16de4b82d
//!
//! The Skills tab (`newClass("SkillsTab", "UndoHandler", "ControlHost", "Control")`): a free node
//! owned by the build node (`build.skillsTab`, `BuildState::tabs.skills`), like the Notes and
//! Calcs tabs (U22, U39). The model half is calc:BM-B's and calc:BM-A's (`model.skillsTab`:
//! `socketGroupList`, `skillSets`, `skillSetOrderList`, `activeSkillSetId`, `displayGroup`,
//! `defaultGemLevel`, `defaultGemQuality`, `imbuedSupportBySlot`, the UI settings in
//! `ui_state()`; LoadSkill/Load/Save, FindSkillGem, ProcessGemLevel, ProcessSocketGroup,
//! UpdateSocketGroups, the model halves of SetDisplayGroup/SetActiveSkillSet, NewSkillSet,
//! RebuildImbuedSupportBySlot and the imbued-support change function; ui doc §10.2). This file is
//! the UI half: the constructor's controls, Draw, copy/paste, UpdateGemSlots, the UI tails of
//! SetDisplayGroup/SetActiveSkillSet/Load, AddSocketGroupTooltip, the undo state and the skill set
//! manager popup. The BuildMode seams at the bottom ([`ctor_hook`], [`draw_tab`], [`mod_flag`],
//! [`set_mod_flag`], [`set_active_skill_set_hook`], [`add_socket_group_tooltip_hook`]) are one
//! arm each in `init::default_hooks` / `on_frame::default_shell_hooks` (CCR-0106 §2).
//!
//! Split with the model half (markers): calc:BM-A covers SkillsTab.lua 89, 95-96 and 423-426 (the
//! constructor's model half, `build_xml::skillsTab_ctor_model`), calc:BM-B 323-349, 441-629,
//! 1103-1127, 1129-1273, 1275-1278, 1452-1475 and 1478-1502. The UI statements inside those lines
//! (`// UI:` in BM-B's file) are here, commented with their line numbers and no marker; the rest of
//! 1-742 and 1081-1502 carries this unit's markers. CreateGemSlot (743-1080) and the list
//! controls are U31's.
//!
//! # Seams (CCR-0118)
//!
//! [`SkillsHooks`] (thread-local, [`skills_hooks`] / [`set_skills_hooks`], the CCR-0098 §3
//! pattern) holds what this file calls in units not merged yet:
//! * U31: `new("SkillListControl")` (`controls.groupList`), `self:CreateGemSlot(index)` and
//!   `new("SkillSetListControl")` (the manager popup). The defaults are stand-ins with the Lua
//!   controls' geometry: the two lists are [`ListStandIn`]s (a `ListControl` part over
//!   `socketGroupList` / an empty list, shown, so the controls anchored to the group list show,
//!   but drawing nothing and taking no input); the gem slot controls of CreateGemSlot are the
//!   Lua classes with the same anchors, rects and `controls` keys, never shown and without
//!   callbacks. U31 replaces the three entries.
//!   [`GemSlot`] (`self.gemSlots[i]`, written by CreateGemSlot and read by UpdateGemSlots and
//!   SetDisplayGroup) is declared here; CreateGemSlot stores it with [`SkillsTab::set_gem_slot`].
//! * U38/U34: `itemsTab:GetSocketDescriptionLine(item)`, `itemsTab:AddItemTooltip(tooltip, item,
//!   slot)`, `itemsTab:AddUndoState()`. The defaults are stand-ins (a socket count, the item name,
//!   nothing).
//!
//! The public methods U31 (SkillListControl, SkillSetListControl, CreateGemSlot), U44 (ImportTab)
//! and U46 (CompareEntry) call are [`SkillsTab::set_display_group`], [`SkillsTab::add_undo_state`],
//! [`SkillsTab::copy_socket_group`], [`SkillsTab::paste_socket_group`],
//! [`SkillsTab::add_socket_group_tooltip`], [`SkillsTab::set_active_skill_set`],
//! [`SkillsTab::update_gem_slots`], [`SkillsTab::gem_slot`] and [`tab`].
//!
//! # Lua semantics
//!
//! * The UI settings `self.sortGemsByDPS`, `sortGemsByDPSField`, `showSupportGemTypes`,
//!   `showLegacyGems` live in BM-B's `model.skillsTab.ui_state()` (CCR-0019), together with the
//!   selected indices of the three option dropdowns Load goes through; the controls' callbacks
//!   write both, so the model's Load/Save and GemSelectControl (U29) read what the controls show.
//! * A nil `socketGroup.label`, `gemInstance.level`/`quality` are BM-B's markers (CCR-0019):
//!   `SetText` of them writes `"nil"` (`tostring(nil)`), `%d` of them raises the Lua error.
//! * `socketGroup.sourceItem`, `sourceNode`, `imbuedSupport`, `displayGemList` and
//!   `gemInstance.color`, `matchesSocket` are in the objects' `extra` tables (CCR-0040, CCR-0083).
//! * `data.skillColorMap` (Data.lua:306) is `{ STRENGTH, DEXTERITY, INTELLIGENCE, NORMAL }` of
//!   `colorCodes`; Main only ever changes POSITIVE/NEGATIVE/HIGHLIGHT, so [`skill_color_map`] reads
//!   `cx.main.cc`.
//! * `self.controls.imbuedSupport:gemChangeFunc(nil)` (L364) is a method call: the control lands
//!   in `gemData` and `gemMatch` is nil, so it is the clearing branch (`gemData = nil`).
//!
//! ORDER: the `pairs` loops are over sequences (`pairs(socketGroup.gemList)` L1294, the gem lists
//! and socket group lists of CreateUndoState L1401-1406) or over `skillSets`, an integer-keyed
//! table the model keeps as an insertion-ordered map: CreateUndoState copies it in that order and
//! RestoreUndoState inserts the copies back in the same order (c: nothing iterates `skillSets` for
//! an observable order except through `skillSetOrderList`).
//!
//! DIVERGENCE(load-ui-tail): `SkillsTab:Load` (calc:BM-B) runs inside `Init_model`, before the UI
//! constructors, so its UI statements (L528-549: the option controls' selections, text and states;
//! L569's SetActiveSkillSet UI tail; L570 `ResetUndo`) run in [`ctor_hook`] right after the
//! constructor, when the build file has a `Skills` section. LoadSkill's imbued-support call (L459)
//! returns at L325 during Init (the constructor's `displayGroup` is nil), so its UI lines never run.
//!
//! DIVERGENCE(ctor-sync-loadouts): the `self.build:SyncLoadouts()` of the constructor's and Load's
//! `SetActiveSkillSet` (L1501, reached from L426 and L569) is not repeated: Build:Init calls
//! SyncLoadouts after every tab is loaded (Build.lua:726), and the earlier calls only fill the
//! loadout dropdown that call rebuilds. Every later SetActiveSkillSet calls it.
//!
//! DIVERGENCE(default-gem-quality): L96 `self.defaultGemQuality = main.defaultGemQuality` is
//! calc:BM-A's model half, which writes nil (headless). [`ctor_hook`] writes
//! `main.defaultGemQuality` when the build file has no `Skills` section (with one, Load's L536
//! overwrote it before the UI ran).
//!
//! DIVERGENCE(paste-main-active-skill): a pasted group's `mainActiveSkill` and
//! `mainActiveSkillCalcs` are nil in Lua (L711; the readers write `or 1`, e.g. CalcSetup.lua:1886)
//! and 1 here (the frozen `Cell<f64>` has no nil, CCR-0118 §3):
//! Save writes `"1"` where PoB writes `"nil"` until the group becomes the main group.
//!
//! DIVERGENCE(node-name): `sourceNode.name` (L402, L1321) is the node's `dn` (PassiveTree.lua:534
//! `node.dn = node.name`; the model's `Node` has no `name`).
//!
//! Props with side effects (ui doc §7.3): none.

#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::option_if_let_else,
    clippy::option_option,
    clippy::len_zero,
    clippy::single_match_else
)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::m_min;
use pob_lua::{lfmt, n, tonumber, tostring_num};
use pob_ui_core::controls::drop_down::{DropTipArgs, DropTipMode, SelFunc};
use pob_ui_core::controls::list::{ListData, ListHooks, ListSource};
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl,
    LabelControl, ListControl, ListValue, ScrollBarControl, SectionControl,
};
use pob_ui_core::input::InputEvents;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;
use pob_ui_core::undo::{UndoOps, UndoStack};

use super::SkillsTab;
use crate::app::CxApp;
use crate::build::init;
use crate::controls::gem_select::{GemChangeFunc, GemIdVal, GemSelectControl};
use crate::main_mode::background::draw_background;
use crate::model::gamedata::GemDataRef;
use crate::model::skills_tab::{
    set_socketGroup_label, skillSet_socketGroupList, socketGroup_includeInFullDPS,
    socketGroup_label,
};
use crate::model::{
    self, build_xml, GemInstance, GemInstanceRef, Item, ItemRef, LStr, Name, Node, Shared,
    SocketGroup, SocketGroupRef, Table, Val,
};

// @pob Classes/SkillsTab.lua:1-75 h=5c18127a
// (L1-5 the header comment; L6-11 the `pairs`/`ipairs`/`table`/`math` locals: Rust iteration,
// `Shared` lists and `pob_lua::math`)

/// `groupSlotDropList` (L13-28): `{ label, slotName }`.
const GROUP_SLOT_DROP_LIST: [(&str, Option<&str>); 14] = [
    ("None", None),
    ("Weapon 1", Some("Weapon 1")),
    ("Weapon 2", Some("Weapon 2")),
    ("Weapon 1 (Swap)", Some("Weapon 1 Swap")),
    ("Weapon 2 (Swap)", Some("Weapon 2 Swap")),
    ("Helmet", Some("Helmet")),
    ("Body Armour", Some("Body Armour")),
    ("Gloves", Some("Gloves")),
    ("Boots", Some("Boots")),
    ("Amulet", Some("Amulet")),
    ("Ring 1", Some("Ring 1")),
    ("Ring 2", Some("Ring 2")),
    ("Ring 3", Some("Ring 3")),
    ("Belt", Some("Belt")),
];

/// `defaultGemLevelList` (L30-57): `{ label, description, gemLevel }`.
const DEFAULT_GEM_LEVEL_LIST: [(&str, &str, &str); 5] = [
    (
        "Normal Maximum",
        "All gems default to their highest valid non-corrupted gem level.",
        "normalMaximum",
    ),
    (
        "Corrupted Maximum",
        "Normal gems default to their highest valid corrupted gem level.\nAwakened gems default to their highest valid non-corrupted gem level.",
        "corruptedMaximum",
    ),
    (
        "Awakened Maximum",
        "All gems default to their highest valid corrupted gem level.",
        "awakenedMaximum",
    ),
    (
        "Match Character Level",
        "All gems default to their highest valid non-corrupted gem level that your character meets the level requirement for.\nThis hides gems with a minimum level requirement above your character level, preventing them from showing up in the dropdown list.",
        "characterLevel",
    ),
    ("Level 1", "All gems default to level 1.", "levelOne"),
];

/// `showSupportGemTypeList` (L59-63): `{ label, show }`.
const SHOW_SUPPORT_GEM_TYPE_LIST: [(&str, &str); 3] = [
    ("All", "ALL"),
    ("Non-Exceptional", "NORMAL"),
    ("Exceptional", "EXCEPTIONAL"),
];

/// `sortGemTypeList` (L65-75): `{ label, type }`.
const SORT_GEM_TYPE_LIST: [(&str, &str); 9] = [
    ("Full DPS", "FullDPS"),
    ("Combined DPS", "CombinedDPS"),
    ("Hit DPS", "TotalDPS"),
    ("Average Hit", "AverageDamage"),
    ("DoT DPS", "TotalDot"),
    ("Bleed DPS", "BleedDPS"),
    ("Ignite DPS", "IgniteDPS"),
    ("Poison DPS", "TotalPoisonDPS"),
    ("Effective Hit Pool", "TotalEHP"),
];

/// A `groupSlotDropList` entry's `slotName` (the dropdown item's value).
pub struct GroupSlotValue(pub Option<&'static str>);
/// A `defaultGemLevelList` entry's `description` and `gemLevel`.
pub struct GemLevelValue {
    pub description: &'static str,
    pub gem_level: &'static str,
}
/// A `showSupportGemTypeList` entry's `show`.
pub struct ShowSupportValue(pub &'static str);
/// A `sortGemTypeList` entry's `type`.
pub struct SortTypeValue(pub &'static str);

fn group_slot_items() -> Vec<DropItem> {
    GROUP_SLOT_DROP_LIST
        .iter()
        .map(|&(label, slot)| DropItem::new(label, Rc::new(GroupSlotValue(slot))))
        .collect()
}
fn default_gem_level_items() -> Vec<DropItem> {
    DEFAULT_GEM_LEVEL_LIST
        .iter()
        .map(|&(label, description, gem_level)| {
            DropItem::new(
                label,
                Rc::new(GemLevelValue {
                    description,
                    gem_level,
                }),
            )
        })
        .collect()
}
fn show_support_items() -> Vec<DropItem> {
    SHOW_SUPPORT_GEM_TYPE_LIST
        .iter()
        .map(|&(label, show)| DropItem::new(label, Rc::new(ShowSupportValue(show))))
        .collect()
}
fn sort_type_items() -> Vec<DropItem> {
    SORT_GEM_TYPE_LIST
        .iter()
        .map(|&(label, ty)| DropItem::new(label, Rc::new(SortTypeValue(ty))))
        .collect()
}

// @pob Classes/SkillsTab.lua:76-81 h=fddf8120
// (L78-79 `newClass("SkillsTab", "UndoHandler", "ControlHost", "Control")`: the `Widget` impl
// below, `.host()` in the constructor and [`UNDO`])

/// `self.gemSlots[index]` (CreateGemSlot, L743-1080, U31): the controls of one gem row.
#[derive(Clone, Copy, Debug, Default)]
pub struct GemSlot {
    /// `slot.delete` (ButtonControl)
    pub delete: CtrlId,
    /// `slot.nameSpec` (GemSelectControl)
    pub name_spec: CtrlId,
    /// `slot.level` (EditControl)
    pub level: CtrlId,
    /// `slot.quality` (EditControl)
    pub quality: CtrlId,
    /// `slot.enabled` (CheckBoxControl)
    pub enabled: CtrlId,
    /// `slot.count` (EditControl)
    pub count: CtrlId,
    /// `slot.errMsg` (LabelControl)
    pub err_msg: CtrlId,
    /// `slot.enableGlobal1` (CheckBoxControl)
    pub enable_global1: CtrlId,
    /// `slot.enableGlobal2` (CheckBoxControl)
    pub enable_global2: CtrlId,
}

/// One undo state (CreateUndoState, L1397-1418).
pub struct UndoState {
    /// `state.activeSkillSetId`
    pub activeSkillSetId: f64,
    /// `state.skillSets` (copies, in the model map's order)
    pub skillSets: Vec<(u32, Table)>,
    /// `state.skillSetOrderList`
    pub skillSetOrderList: Vec<u32>,
    /// `state.activeSocketGroup` (`build.mainSocketGroup`)
    pub activeSocketGroup: f64,
    /// `state.activeSocketGroup2` (`build.calcsTab.input.skill_number`)
    pub activeSocketGroup2: Val,
}

/// U30's part of the `SkillsTab` object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.build` (the build node)
    pub build: CtrlId,
    /// `self.anchorGroupDetail`
    pub anchor_group_detail: CtrlId,
    /// `self.anchorGemSlots`
    pub anchor_gem_slots: CtrlId,
    /// `self.gemSlots` (1-based in Lua: `gemSlots[i]` is `gem_slots[i - 1]`)
    pub gem_slots: Vec<GemSlot>,
    /// `self.undo`, `self.redo`, `self.modFlag` (UndoHandler)
    pub undo: UndoStack<UndoState>,
}

/// drawn by BuildMode (`self.skillsTab:Draw(tabViewPort, inputEvents)`, Build.lua:1311)
impl Widget for SkillsTab {
    const NAME: &'static str = "SkillsTab";
    const CAPS: Caps = Caps::empty();
}

fn undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<UndoState> {
    &mut cx.w_mut::<SkillsTab>(me).core.undo
}

/// The UndoHandler class of the tab (UndoHandler.lua; the states are L1397-1441).
pub const UNDO: UndoOps<UndoState> = UndoOps {
    stack: undo_stack,
    create: SkillsTab::create_undo_state,
    restore: SkillsTab::restore_undo_state,
};

// ---- the seams to units not merged yet (U31, U34/U38) ----------------------------------------

/// What this unit calls in U31's controls and the items tab's UI (module notes; CCR-0118).
#[derive(Clone)]
pub struct SkillsHooks {
    /// `self.controls.groupList = new("SkillListControl"):SkillListControl(anchor, rect, self)`
    /// (L113): constructs the control, stores it as `controls.groupList` of the tab, returns it.
    /// The control is a `ListControl` subclass (the tab writes its `list`, `selIndex`,
    /// `selValue` through the `ListControl` part).
    pub skill_list_ctor: Rc<dyn Fn(&mut Cx, CtrlId, Anchor, [Prop<f64>; 4]) -> CtrlId>,
    /// `self:CreateGemSlot(index)` (L743-1080): creates the row's controls in the tab and stores
    /// the row with [`SkillsTab::set_gem_slot`].
    pub create_gem_slot: Rc<dyn Fn(&mut Cx, CtrlId, usize)>,
    /// `new("SkillSetListControl"):SkillSetListControl(nil, rect, self)` as the first control of
    /// the manager popup (L1445): constructs it in `popup` under `key`, returns it.
    pub skill_set_list_ctor: Rc<dyn Fn(&mut Cx, CtrlId, CtrlKey, [Prop<f64>; 4], CtrlId) -> CtrlId>,
    /// `self.build.itemsTab:GetSocketDescriptionLine(item)` (ItemsTab.lua:4185-4212, U38)
    pub items_get_socket_description_line: Rc<dyn Fn(&Cx, &ItemRef) -> String>,
    /// `self.build.itemsTab:AddItemTooltip(tooltip, item, slot)` (ItemsTab.lua:4214, U38)
    pub items_add_item_tooltip: Rc<dyn Fn(&mut Cx, &mut Tooltip, &ItemRef, &Table)>,
    /// `self.build.itemsTab:AddUndoState()` (UndoHandler of the items tab, U34/U38)
    pub items_add_undo_state: Rc<dyn Fn(&mut Cx)>,
}

/// The stand-in list of U31's `SkillListControl` / `SkillSetListControl` until U31 lands: a
/// `ListControl` part (the fields the tab writes: `list`, `selIndex`, `selValue`) with the Lua
/// rect, shown (the tab's controls anchored to the group list show), drawing nothing and taking
/// no input (`IsMouseOver` is false, so it never gets the focus).
pub struct ListStandIn {
    pub list: ListControl,
}

static LIST_STANDIN_HOOKS: ListHooks = ListHooks {
    get_row_value: |_, _, _, _, _| String::new(),
    ..ListHooks::BASE
};

impl ListStandIn {
    /// `ListControl(anchor, rect, 16, "VERTICAL", true, list)` (SkillListControl.lua:36,
    /// SkillSetListControl.lua:18).
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], list: ListData) -> New<ListStandIn> {
        New::new(
            Base::new(anchor, rect),
            ListStandIn {
                list: ListControl::state(&LIST_STANDIN_HOOKS, 16.0, true, true, list, false),
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.id()))
    }
}

impl Widget for ListStandIn {
    const NAME: &'static str = "ListStandIn";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    fn draw(_cx: &mut Cx, _me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {}
    fn is_mouse_over(_cx: &Cx, _me: CtrlId) -> bool {
        false
    }
    fn on_key_down(_cx: &mut Cx, _me: CtrlId, _key: Key, _double_click: bool) -> Focus {
        None
    }
    fn on_key_up(_cx: &mut Cx, _me: CtrlId, _key: Key) -> Focus {
        None
    }

    pob_ui_core::impl_parts!(ListStandIn: list => ListControl);
}

/// The stand-in for `SkillListControl` until U31 lands ([`ListStandIn`] over `socketGroupList`).
fn standin_skill_list(cx: &mut Cx, tab: CtrlId, anchor: Anchor, rect: [Prop<f64>; 4]) -> CtrlId {
    let list = cx.build().model.skillsTab.socketGroupList.get();
    cx.add(
        tab,
        "groupList",
        ListStandIn::new(anchor, rect, ListData::Source(Rc::new(GroupListSource(list)))),
    )
    .id()
}

/// The stand-in for `CreateGemSlot(index)` until U31 lands: the row's controls with the classes,
/// `controls` keys, anchors and rects of SkillsTab.lua:743-1080, never shown and without
/// callbacks (so `UpdateGemSlots`/`SetDisplayGroup` write into the same controls U31 creates).
fn standin_create_gem_slot(cx: &mut Cx, me: CtrlId, index: usize) {
    let hidden = || Prop::Val(false);
    let anchor_gem_slots = cx.w::<SkillsTab>(me).core.anchor_gem_slots;
    let prev = (index > 1).then(|| SkillsTab::gem_slot(cx, me, index - 1));
    let delete = cx
        .add(
            me,
            format!("gemSlot{index}Delete"),
            ButtonControl::new(Anchor::NONE, rect(0, 0, 20, 20), "x", |_| {}).shown(hidden()),
        )
        .id();
    if let Some(prev) = prev {
        let y = Prop::f(move |cx, _| {
            if cx.is_shown(prev.enable_global1) || cx.is_shown(prev.enable_global2) {
                24.0
            } else {
                2.0
            }
        });
        cx.set_anchor(delete, TopLeft, Some(prev.delete), BottomLeft, Some((0.0.into(), y)), false);
    } else {
        cx.set_anchor(delete, TopLeft, Some(anchor_gem_slots), TopLeft, Some((0.0.into(), 0.0.into())), false);
    }
    let name_spec = cx
        .add(
            me,
            format!("gemSlot{index}Name"),
            GemSelectControl::new(
                Anchor::new(Left, delete, Right),
                rect(2, 0, 300, 20),
                me,
                index as f64,
                None,
                false,
                false,
            )
            .shown(hidden()),
        )
        .id();
    let edit = |cx: &mut Cx, key: String, other: CtrlId, x: i32| {
        cx.add(
            me,
            key,
            EditControl::new(
                Anchor::new(Left, other, Right),
                rect(x, 0, 60, 20),
                EditArgs {
                    filter: Some("%D".to_owned()),
                    limit: Some(2),
                    ..EditArgs::default()
                },
            )
            .shown(Prop::Val(false)),
        )
        .id()
    };
    let level = edit(cx, format!("gemSlot{index}Level"), name_spec, 2);
    let quality = edit(cx, format!("gemSlot{index}Quality"), level, 2);
    let enabled = cx
        .add(
            me,
            format!("gemSlot{index}Enable"),
            CheckBoxControl::new(Anchor::new(Left, quality, Right), rect(18, 0, 20, 0), None, None, None, false)
                .shown(hidden()),
        )
        .id();
    let count = edit(cx, format!("gemSlot{index}Count"), enabled, 18);
    let err_msg = cx
        .add(
            me,
            format!("gemSlot{index}ErrMsg"),
            LabelControl::new(Anchor::new(Left, count, Right), rect(2, 2, 0, 16), "").shown(hidden()),
        )
        .id();
    let enable_global1 = cx
        .add(
            me,
            format!("gemSlot{index}EnableGlobal1"),
            CheckBoxControl::new(Anchor::new(TopLeft, delete, BottomLeft), rect(0, 2, 20, 0), Some(String::new()), None, None, false)
                .shown(hidden()),
        )
        .id();
    let enable_global2 = cx
        .add(
            me,
            format!("gemSlot{index}EnableGlobal2"),
            CheckBoxControl::new(
                Anchor::new(Left, enable_global1, Right).collapse(),
                rect(0, 0, 20, 0),
                Some(String::new()),
                None,
                None,
                false,
            )
            .shown(hidden()),
        )
        .id();
    SkillsTab::set_gem_slot(
        cx,
        me,
        index,
        GemSlot {
            delete,
            name_spec,
            level,
            quality,
            enabled,
            count,
            err_msg,
            enable_global1,
            enable_global2,
        },
    );
}

/// The stand-in for `SkillSetListControl` until U31 lands ([`ListStandIn`] over an empty list;
/// the popup's Done button is the tab's).
fn standin_skill_set_list(
    cx: &mut Cx,
    popup: CtrlId,
    key: CtrlKey,
    rect: [Prop<f64>; 4],
    _tab: CtrlId,
) -> CtrlId {
    cx.add(popup, key, ListStandIn::new(Anchor::NONE, rect, ListData::default()))
        .id()
}

/// The defaults (U31 and U38 replace the entries with their controls and methods).
pub fn default_skills_hooks() -> SkillsHooks {
    SkillsHooks {
        skill_list_ctor: Rc::new(standin_skill_list),
        create_gem_slot: Rc::new(standin_create_gem_slot),
        skill_set_list_ctor: Rc::new(standin_skill_set_list),
        // stand-in: the number of sockets (ItemsTab.lua:4185-4212 is U38's)
        items_get_socket_description_line: Rc::new(|_cx, item| {
            format!("{} sockets", item.sockets.get().len())
        }),
        // stand-in: the item's name (ItemsTab.lua:4214 is U38's)
        items_add_item_tooltip: Rc::new(|cx, tooltip, item, _slot| {
            let name = item.name.get();
            tooltip.add_line(cx, 16.0, &format!("^7{name}"));
        }),
        // the items tab's UndoHandler is U34's/U38's
        items_add_undo_state: Rc::new(|_| {}),
    }
}

thread_local! {
    static SKILLS_HOOKS: RefCell<Option<SkillsHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn skills_hooks() -> SkillsHooks {
    SKILLS_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_skills_hooks)
            .clone()
    })
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_skills_hooks(f: impl FnOnce(&mut SkillsHooks)) {
    SKILLS_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_skills_hooks)));
}

// ---- model access -----------------------------------------------------------------------------

fn model(cx: &Cx) -> model::BuildRef {
    cx.build().model.clone()
}

/// `self.displayGroup`
fn display_group(cx: &Cx) -> Option<SocketGroupRef> {
    cx.build().model.skillsTab.displayGroup.get()
}

/// `self.displayGroup` where the Lua indexes it (nil is the Lua index error).
#[track_caller]
fn dg(cx: &Cx) -> SocketGroupRef {
    display_group(cx)
        .unwrap_or_else(|| lua_error!("attempt to index field 'displayGroup' (a nil value)"))
}

/// `data.skillColorMap[color]` (Data.lua:306; module notes).
pub fn skill_color_map(cx: &Cx, color: Option<u8>) -> Option<String> {
    let cc = &cx.main.cc;
    match color {
        Some(1) => Some(cc.STRENGTH.clone()),
        Some(2) => Some(cc.DEXTERITY.clone()),
        Some(3) => Some(cc.INTELLIGENCE.clone()),
        Some(4) => Some(cc.NORMAL.clone()),
        _ => None,
    }
}

/// `colorCodes[name]` (nil for an unknown key).
fn color_code(cx: &Cx, name: &str) -> Option<String> {
    let cc = &cx.main.cc;
    Some(
        match name {
            "NORMAL" => &cc.NORMAL,
            "MAGIC" => &cc.MAGIC,
            "RARE" => &cc.RARE,
            "UNIQUE" => &cc.UNIQUE,
            "RELIC" => &cc.RELIC,
            "GEM" => &cc.GEM,
            "PROPHECY" => &cc.PROPHECY,
            "CURRENCY" => &cc.CURRENCY,
            "CRAFTED" => &cc.CRAFTED,
            _ => return None,
        }
        .clone(),
    )
}

/// `".." .. colorCodes[k] .. ".."`: a nil colour is the Lua concatenation error.
#[track_caller]
fn color_code_cat(cx: &Cx, name: &str) -> String {
    color_code(cx, name).unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"))
}

/// `tostring(v)` of a number field BM-B stores as NaN for Lua nil (CCR-0019).
fn tostring_nan(v: f64) -> String {
    if v.is_nan() {
        "nil".to_owned()
    } else {
        tostring_num(v)
    }
}

/// `tostring(v)` of a scalar value.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => if *b { "true" } else { "false" }.to_owned(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        _ => lua_error!("CCR-0118: tostring of a table value"),
    }
}

/// `self.list = skillsTab.socketGroupList` (the list aliases the model's; the `SharedVecSource`
/// adapter needs `Clone` elements, which `SocketGroup` is not).
pub struct GroupListSource(pub Shared<Vec<SocketGroupRef>>);

impl ListSource for GroupListSource {
    /// `#self.list`
    fn len(&self, _cx: &Cx) -> usize {
        self.0.len()
    }
    /// `self.list[i]`
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        self.0.get(i).map(ListValue::of)
    }
    /// `t_insert(self.list, i, v)`
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let g = v
            .rc::<SocketGroup>()
            .unwrap_or_else(|| lua_error!("GroupListSource::insert: value of another type"));
        if i == 0 || i > self.0.len() + 1 {
            lua_error!("bad argument #2 to 'insert' (position out of bounds)");
        }
        self.0.insert(i, g);
    }
    /// `t_remove(self.list, i)`
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        let g = self
            .0
            .remove(Some(i))
            .unwrap_or_else(|| lua_error!("GroupListSource::remove: index {i} out of range"));
        ListValue::of(g)
    }
}

/// A number field BM-B stores as NaN for Lua nil, as a format argument.
fn nan_nil(v: f64) -> Option<f64> {
    (!v.is_nan()).then_some(v)
}

/// `a > b` of two numbers that may be Lua nil (NaN, CCR-0019): nil raises.
#[track_caller]
fn gt(a: f64, b: f64) -> bool {
    if a.is_nan() || b.is_nan() {
        lua_error!("attempt to compare nil with number");
    }
    a > b
}

/// `self.build.itemsTab.slots[slotName]`
fn items_slot(cx: &Cx, slot_name: &str) -> Option<Table> {
    cx.build()
        .model
        .itemsTab
        .slots
        .with(|m| m.get(&Name::from(slot_name)).cloned())
}

/// `self.build.itemsTab.items[slot.selItemId]`
fn items_item(cx: &Cx, slot: &Table) -> Option<ItemRef> {
    let id = slot.get(n!("selItemId"));
    let Val::Num(id) = id else { return None };
    if id.fract() != 0.0 || id < 0.0 || id > f64::from(u32::MAX) {
        return None;
    }
    cx.build()
        .model
        .itemsTab
        .items
        .with(|m| m.get(&(id as u32)).cloned())
}

/// `x.name` of an item, a passive node or a table (`source.name`).
fn val_name(v: &Val) -> Option<String> {
    if let Some(item) = v.obj::<Item>() {
        return Some(item.name.get().to_string());
    }
    if let Some(node) = v.obj::<Node>() {
        // DIVERGENCE(node-name)
        return Some(node.dn.get().to_string());
    }
    v.table().and_then(|t| t.str(n!("name"))).map(|s| s.to_string())
}

/// `x.dn` of a passive node or a table (`source.dn`).
fn val_dn(v: &Val) -> Option<String> {
    if v.obj::<Item>().is_some() {
        return None;
    }
    if let Some(node) = v.obj::<Node>() {
        return Some(node.dn.get().to_string());
    }
    v.table().and_then(|t| t.str(n!("dn"))).map(|s| s.to_string())
}

/// `x.rarity` of an item or a table (a passive node has none).
fn val_rarity(v: &Val) -> Option<String> {
    if let Some(item) = v.obj::<Item>() {
        return Some(item.rarity.get().to_string());
    }
    if v.obj::<Node>().is_some() {
        return None;
    }
    v.table().and_then(|t| t.str(n!("rarity"))).map(|s| s.to_string())
}

/// `gem.grantedEffect or (gem.gemData and gem.gemData.grantedEffect)`
fn gem_granted_effect(gem: &GemInstance) -> Option<model::gamedata::GrantedEffectRef> {
    gem.grantedEffect
        .get()
        .or_else(|| gem.gemData.get().map(|g| g.grantedEffect.clone()))
}

/// `local function getSelectedItem()` (L218-231): `(item, groupSlot)`; `groupSlot` is the
/// selected entry's `slotName` (`Some(None)` for "None").
fn get_selected_item(cx: &Cx, group_slot: CtrlId) -> (Option<ItemRef>, Option<Option<&'static str>>) {
    // L219
    let mut item = None;
    // L220
    let groupSlot = DropDownControl::get_sel_value(cx, group_slot)
        .map(|v| v.get::<GroupSlotValue>().and_then(|s| s.0));
    // L221
    if let Some(Some(slotName)) = groupSlot {
        // L222
        let slot = items_slot(cx, slotName);
        // L223
        if let Some(slot) = slot {
            // L224
            item = items_item(cx, &slot);
            // L225
            if item.is_none() {
                // L226
                return (None, None);
            }
        }
    }
    // L230
    (item, groupSlot)
}

/// `local function getSocketCounts(item)` (L246-255): `(maxSockets, abyssalSocketCount)`.
fn get_socket_counts(item: &Item) -> (f64, f64) {
    // L247
    let mut abyssalSocketCount = 0.0;
    // L248
    for socket in item.sockets.get().snapshot() {
        // L249
        if socket.str(n!("color")).as_deref() == Some("A") {
            // L250
            abyssalSocketCount += 1.0;
        }
    }
    // L253
    let base = item
        .base
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"));
    let maxSockets = base.socketLimit.unwrap_or(0.0) - abyssalSocketCount;
    // L254
    (maxSockets, abyssalSocketCount)
}

/// `copyTable(socketGroup, true)` (a shallow copy: every field, the `extra` keys included).
fn copy_socket_group(s: &SocketGroup) -> SocketGroup {
    SocketGroup {
        label: model::Field::new(s.label.get()),
        enabled: std::cell::Cell::new(s.enabled.get()),
        slotEnabled: std::cell::Cell::new(s.slotEnabled.get()),
        slot: model::Field::new(s.slot.get()),
        source: model::Field::new(s.source.get()),
        includeInFullDPS: std::cell::Cell::new(s.includeInFullDPS.get()),
        groupCount: std::cell::Cell::new(s.groupCount.get()),
        gemList: model::Field::new(s.gemList.get()),
        mainActiveSkill: std::cell::Cell::new(s.mainActiveSkill.get()),
        mainActiveSkillCalcs: std::cell::Cell::new(s.mainActiveSkillCalcs.get()),
        noSupports: std::cell::Cell::new(s.noSupports.get()),
        displaySkillList: model::Field::new(s.displaySkillList.get()),
        displaySkillListCalcs: model::Field::new(s.displaySkillListCalcs.get()),
        displayLabel: model::Field::new(s.displayLabel.get()),
        explodeSources: model::Field::new(s.explodeSources.get()),
        extra: s.extra.copyTable(true),
    }
}

/// `copyTable(gem, true)` (a shallow copy).
fn copy_gem_instance(s: &GemInstance) -> GemInstance {
    GemInstance {
        nameSpec: model::Field::new(s.nameSpec.get()),
        skillId: model::Field::new(s.skillId.get()),
        gemId: model::Field::new(s.gemId.get()),
        level: std::cell::Cell::new(s.level.get()),
        quality: std::cell::Cell::new(s.quality.get()),
        enabled: std::cell::Cell::new(s.enabled.get()),
        enableGlobal1: std::cell::Cell::new(s.enableGlobal1.get()),
        enableGlobal2: std::cell::Cell::new(s.enableGlobal2.get()),
        count: std::cell::Cell::new(s.count.get()),
        gemData: model::Field::new(s.gemData.get()),
        grantedEffect: model::Field::new(s.grantedEffect.get()),
        skillPart: std::cell::Cell::new(s.skillPart.get()),
        skillPartCalcs: std::cell::Cell::new(s.skillPartCalcs.get()),
        skillStageCount: std::cell::Cell::new(s.skillStageCount.get()),
        skillStageCountCalcs: std::cell::Cell::new(s.skillStageCountCalcs.get()),
        skillMineCount: std::cell::Cell::new(s.skillMineCount.get()),
        skillMineCountCalcs: std::cell::Cell::new(s.skillMineCountCalcs.get()),
        skillMinion: model::Field::new(s.skillMinion.get()),
        skillMinionCalcs: model::Field::new(s.skillMinionCalcs.get()),
        skillMinionItemSet: std::cell::Cell::new(s.skillMinionItemSet.get()),
        skillMinionItemSetCalcs: std::cell::Cell::new(s.skillMinionItemSetCalcs.get()),
        skillMinionSkill: std::cell::Cell::new(s.skillMinionSkill.get()),
        skillMinionSkillCalcs: std::cell::Cell::new(s.skillMinionSkillCalcs.get()),
        fromItem: std::cell::Cell::new(s.fromItem.get()),
        noSupports: std::cell::Cell::new(s.noSupports.get()),
        triggered: std::cell::Cell::new(s.triggered.get()),
        displayEffect: model::Field::new(s.displayEffect.get()),
        supportEffect: model::Field::new(s.supportEffect.get()),
        reqLevel: std::cell::Cell::new(s.reqLevel.get()),
        extra: s.extra.copyTable(true),
    }
}

/// `gemInstance.displayEffect or gemInstance` (L1373): a skill effect or the gem itself.
enum DisplayEffect {
    Effect(pob_calc::active_skill::SkillEffectRef),
    Gem(GemInstanceRef),
}

impl DisplayEffect {
    fn level(&self) -> f64 {
        match self {
            DisplayEffect::Effect(e) => e.level.get(),
            DisplayEffect::Gem(g) => g.level.get(),
        }
    }
    fn quality(&self) -> f64 {
        match self {
            DisplayEffect::Effect(e) => e.quality.get(),
            DisplayEffect::Gem(g) => g.quality.get(),
        }
    }
    /// `.srcInstance` (a gem instance has none)
    fn src_instance(&self) -> Option<GemInstanceRef> {
        match self {
            DisplayEffect::Effect(e) => e.srcInstance.get(),
            DisplayEffect::Gem(_) => None,
        }
    }
    /// `.superseded`
    fn superseded(&self) -> bool {
        match self {
            DisplayEffect::Effect(e) => e.superseded.get(),
            DisplayEffect::Gem(g) => g.extra.truthy(n!("superseded")),
        }
    }
    /// `not .isSupporting or not next(.isSupporting)`
    fn supports_nothing(&self) -> bool {
        match self {
            DisplayEffect::Effect(e) => e.isSupporting.get().map_or(true, |s| s.with(|s| s.is_empty())),
            DisplayEffect::Gem(g) => g
                .extra
                .table(n!("isSupporting"))
                .map_or(true, |t| t.pairs().next().is_none()),
        }
    }
}

/// `string.format("%s%s ^7%d%s/%d%s", ...)` of one skill effect (L1336-1343, L1351-1358).
fn effect_line(cx: &Cx, e: &pob_calc::active_skill::SkillEffectRef) -> String {
    let ge = e.grantedEffect.get();
    let src = e.srcInstance.get();
    let magic = cx.main.cc.MAGIC.clone();
    let color = skill_color_map(cx, ge.color);
    let level = src.as_ref().map_or(e.level.get(), |s| s.level.get());
    let level_plus = match &src {
        Some(s) if gt(e.level.get(), s.level.get()) => {
            format!("{magic}+{}^7", tostring_num(e.level.get() - s.level.get()))
        }
        _ => String::new(),
    };
    let quality = src.as_ref().map_or(e.quality.get(), |s| s.quality.get());
    let quality_plus = match &src {
        Some(s) if gt(e.quality.get(), s.quality.get()) => {
            format!("{magic}+{}^7", tostring_num(e.quality.get() - s.quality.get()))
        }
        _ => String::new(),
    };
    lfmt!(
        "%s%s ^7%d%s/%d%s",
        color.as_deref(),
        ge.name.as_str(),
        nan_nil(level),
        level_plus.as_str(),
        nan_nil(quality),
        quality_plus.as_str()
    )
}

// ---- the tab ----------------------------------------------------------------------------------

impl SkillsTab {
    /// lua: SkillsTab.lua:82-438 `SkillsTab(build)`
    /// @pob Classes/SkillsTab.lua:82-88 h=caf1f96d
    /// @pob Classes/SkillsTab.lua:90-94 h=3cf3db11
    /// @pob Classes/SkillsTab.lua:97-322 h=e2cbc7d2
    /// @pob Classes/SkillsTab.lua:350-422 h=c11d9856
    /// @pob Classes/SkillsTab.lua:427-440 h=973e5f61
    pub fn new(build: CtrlId) -> New<SkillsTab> {
        // L83 self:UndoHandler() (`State::undo`); L84 self:ControlHost(); L85 self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            SkillsTab {
                core: State {
                    // L87
                    build,
                    ..State::default()
                },
                gem_slots: super::gem_slots::State::default(),
            },
        )
        .host()
        .init(|cx, me| Self::init(cx, me.id()))
    }

    /// The constructor body after the node exists (L89-L437).
    fn init(cx: &mut Cx, me: CtrlId) {
        let hooks = skills_hooks();
        // L89 self.socketGroupList = { } (model half: calc:BM-A)
        // L91-94 the UI settings: the defaults of `model.skillsTab.ui_state()` (calc:BM-B), which
        // the model's Load already read the build file into
        // L95-96 (model half: calc:BM-A; DIVERGENCE(default-gem-quality))

        // Set selector
        // L99-102
        let set_select = cx
            .add(
                me,
                "setSelect",
                DropDownControl::new(
                    Anchor::new(TopLeft, me, TopLeft),
                    rect(76, 8, 210, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L100 self:SetActiveSkillSet(self.skillSetOrderList[index])
                        let id = cx
                            .build()
                            .model
                            .skillsTab
                            .skillSetOrderList
                            .get(index)
                            .map(f64::from);
                        SkillsTab::set_active_skill_set(cx, me, id);
                        // L101
                        SkillsTab::add_undo_state(cx, me);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L103
        cx.w_mut::<DropDownControl>(set_select).enable_dropped_width = true;
        // L104-106
        cx.base_mut(set_select).enabled =
            Prop::f(|cx, _| cx.build().model.skillsTab.skillSetOrderList.len() > 1);
        // L107
        cx.add(
            me,
            "setLabel",
            LabelControl::new(Anchor::new(Right, set_select, Left), rect(-2, 0, 0, 16), "^7Skill set:"),
        );
        // L108-110
        cx.add(
            me,
            "setManage",
            ButtonControl::new(Anchor::new(Left, set_select, Right), rect(4, 0, 90, 20), "Manage...", move |cx| {
                // L109
                SkillsTab::open_skill_set_manage_popup(cx, me);
            }),
        );

        // Socket group list
        // L113
        let group_list = (hooks.skill_list_ctor)(cx, me, Anchor::new(TopLeft, me, TopLeft), rect(20, 54, 360, 300));
        // L114-122
        cx.add(
            me,
            "groupTip",
            LabelControl::new(
                Anchor::new(TopLeft, group_list, BottomLeft),
                rect(0, 8, 0, 14),
                "^7Usage Tips:\n- You can copy/paste socket groups using Ctrl+C and Ctrl+V.\n- Ctrl + Click to enable/disable socket groups.\n- Ctrl + Right click to include/exclude in FullDPS calculations.\n- Right click to set as the Main skill group.\n",
            ),
        );

        // Gem options
        // L125
        let optionInputsX = 170.0;
        // L126
        let optionInputsY = 45.0;
        // L127
        let option_section = cx
            .add(
                me,
                "optionSection",
                SectionControl::new(
                    Anchor::new(TopLeft, group_list, BottomLeft),
                    rect(0.0, optionInputsY + 50.0, 360.0, 156.0),
                    "Gem Options",
                ),
            )
            .id();
        // L128-130
        let sort_gems_by_dps = cx
            .add(
                me,
                "sortGemsByDPS",
                CheckBoxControl::new(
                    Anchor::new(TopLeft, group_list, BottomLeft),
                    rect(optionInputsX, optionInputsY + 70.0, 20.0, 0.0),
                    Some("Sort gems by DPS:".to_owned()),
                    Some(Rc::new(|cx: &mut Cx, state: bool| {
                        // L129
                        cx.build().model.skillsTab.ui_state().set(n!("sortGemsByDPS"), state);
                    })),
                    None,
                    true,
                ),
            )
            .id();
        // L131-133
        cx.add(
            me,
            "sortGemsByDPSFieldControl",
            DropDownControl::new(
                Anchor::new(Left, sort_gems_by_dps, Right),
                rect(10, 0, 140, 20),
                sort_type_items(),
                Some(Rc::new(|cx: &mut Cx, index: usize, value: Option<&DropItem>| {
                    let v = value
                        .and_then(|v| v.get::<SortTypeValue>())
                        .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                    let ui = cx.build().model.skillsTab.ui_state();
                    // L132
                    ui.set(n!("sortGemsByDPSField"), v.0);
                    ui.set(n!("sortGemsByDPSFieldSelIndex"), index as f64);
                }) as SelFunc),
                None,
                false,
            ),
        );
        // L134-136
        let default_level = cx
            .add(
                me,
                "defaultLevel",
                DropDownControl::new(
                    Anchor::new(TopLeft, group_list, BottomLeft),
                    rect(optionInputsX, optionInputsY + 94.0, 170.0, 20.0),
                    default_gem_level_items(),
                    Some(Rc::new(|cx: &mut Cx, index: usize, value: Option<&DropItem>| {
                        let v = value
                            .and_then(|v| v.get::<GemLevelValue>())
                            .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                        let st = &cx.build().model.skillsTab;
                        // L135
                        st.defaultGemLevel.set(Some(Name::from(v.gem_level)));
                        st.ui_state().set(n!("defaultLevelSelIndex"), index as f64);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L137-142
        cx.w_mut::<DropDownControl>(default_level).tt.tooltip_func = Some(Rc::new(
            |cx: &mut Cx, tooltip: &mut Tooltip, (mode, _index, value): DropTipArgs| {
                // L138
                tooltip.clear(false);
                // L139
                if mode != DropTipMode::Out {
                    let v = value
                        .as_ref()
                        .map(|v| v.get::<GemLevelValue>().map(|v| v.description))
                        .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                    if let Some(description) = v {
                        // L140
                        tooltip.add_line(cx, 16.0, &format!("^7{description}"));
                    }
                }
            },
        ));
        // L143
        cx.add(
            me,
            "defaultLevelLabel",
            LabelControl::new(Anchor::new(Right, default_level, Left), rect(-4, 0, 0, 16), "^7Default gem level:"),
        );
        // L144-146
        let default_quality = cx
            .add(
                me,
                "defaultQuality",
                EditControl::new(
                    Anchor::new(TopLeft, group_list, BottomLeft),
                    rect(optionInputsX, optionInputsY + 118.0, 60.0, 20.0),
                    EditArgs {
                        filter: Some("%D".to_owned()),
                        limit: Some(2),
                        change_func: Some(Rc::new(|cx: &mut Cx, buf: &str| {
                            // L145
                            let q = m_min(tonumber(buf).unwrap_or(0.0), 23.0);
                            cx.build().model.skillsTab.defaultGemQuality.set(Some(q));
                        })),
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L147
        cx.add(
            me,
            "defaultQualityLabel",
            LabelControl::new(Anchor::new(Right, default_quality, Left), rect(-4, 0, 0, 16), "^7Default gem quality:"),
        );
        // L148-150
        let show_support = cx
            .add(
                me,
                "showSupportGemTypes",
                DropDownControl::new(
                    Anchor::new(TopLeft, group_list, BottomLeft),
                    rect(optionInputsX, optionInputsY + 142.0, 170.0, 20.0),
                    show_support_items(),
                    Some(Rc::new(|cx: &mut Cx, index: usize, value: Option<&DropItem>| {
                        let v = value
                            .and_then(|v| v.get::<ShowSupportValue>())
                            .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                        let ui = cx.build().model.skillsTab.ui_state();
                        // L149
                        ui.set(n!("showSupportGemTypes"), v.0);
                        ui.set(n!("showSupportGemTypesSelIndex"), index as f64);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L151
        cx.add(
            me,
            "showSupportGemTypesLabel",
            LabelControl::new(Anchor::new(Right, show_support, Left), rect(-4, 0, 0, 16), "^7Show support gems:"),
        );
        // L152-154
        cx.add(
            me,
            "showLegacyGems",
            CheckBoxControl::new(
                Anchor::new(TopLeft, group_list, BottomLeft),
                rect(optionInputsX, optionInputsY + 166.0, 20.0, 0.0),
                Some("^7Show legacy gems:".to_owned()),
                Some(Rc::new(|cx: &mut Cx, state: bool| {
                    // L153
                    cx.build().model.skillsTab.ui_state().set(n!("showLegacyGems"), state);
                })),
                None,
                false,
            ),
        );

        // Socket group details
        // L157
        let anchor_group_detail = if cx.app().main.portrait_mode {
            // L158
            cx.add_free(
                Some(me),
                PlainControl::new(Anchor::new(TopLeft, option_section, BottomLeft), rect(0, 20, 0, 0)),
            )
        } else {
            // L160
            cx.add_free(
                Some(me),
                PlainControl::new(Anchor::new(TopLeft, group_list, TopRight), rect(20, 0, 0, 0)),
            )
        }
        .id();
        cx.w_mut::<SkillsTab>(me).core.anchor_group_detail = anchor_group_detail;
        // L162-164
        cx.base_mut(anchor_group_detail).shown = Prop::f(|cx, _| display_group(cx).is_some());
        // L165-170
        cx.add(
            me,
            "groupLabel",
            EditControl::new(
                Anchor::new(TopLeft, anchor_group_detail, TopLeft),
                rect(0, 0, 380, 20),
                EditArgs {
                    prompt: Some("Label".to_owned()),
                    filter: Some("%c".to_owned()),
                    limit: Some(50),
                    change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                        let g = dg(cx);
                        // L166
                        set_socketGroup_label(&g, Some(buf));
                        // L167
                        let m = model(cx);
                        m.skillsTab.ProcessSocketGroup(&m, &g);
                        // L168
                        SkillsTab::add_undo_state(cx, me);
                        // L169
                        m.buildFlag.set(true);
                    })),
                    ..EditArgs::default()
                },
            ),
        );
        // L171
        let group_slot_label = cx
            .add(
                me,
                "groupSlotLabel",
                LabelControl::new(Anchor::new(TopLeft, anchor_group_detail, TopLeft), rect(0, 30, 0, 16), "^7Socketed in:"),
            )
            .id();
        // L172-188
        let group_slot = cx
            .add(
                me,
                "groupSlot",
                DropDownControl::new(
                    Anchor::new(TopLeft, anchor_group_detail, TopLeft),
                    rect(85, 28, 130, 20),
                    group_slot_items(),
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        let value = value
                            .and_then(|v| v.get::<GroupSlotValue>())
                            .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                        let m = model(cx);
                        let st = &m.skillsTab;
                        let g = dg(cx);
                        let by_slot = |slot: Option<Name>| slot.map_or(Val::Nil, |s| st.imbuedSupportBySlot.get(s));
                        // maintain imbued support to new slot
                        // L174
                        if by_slot(g.slot.get()).truthy() && g.extra.truthy(n!("imbuedSupport")) {
                            // L175
                            if let Some(new_slot) = value.0.filter(|s| !by_slot(Some(Name::from(s))).truthy()) {
                                // L176 copyTable(grantedEffect, true): a new table with the same
                                // contents (here a new box around the same record)
                                let ge = by_slot(g.slot.get())
                                    .obj::<model::gamedata::GrantedEffectRef>()
                                    .map(|ge| (*ge).clone())
                                    .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected)"));
                                st.imbuedSupportBySlot.set(
                                    Name::from(new_slot),
                                    Val::Obj(Rc::new(ge) as Rc<dyn std::any::Any>),
                                );
                            } else {
                                let imb = cx.child(me, "imbuedSupport").unwrap_or_else(|| {
                                    lua_error!("attempt to index field 'imbuedSupport' (a nil value)")
                                });
                                // L178
                                cx.w_mut::<GemSelectControl>(imb).gem_id = None;
                                // L179
                                EditControl::set_text(cx, imb, "", false);
                                // L180 reset saved support to None
                                g.extra.set(n!("imbuedSupport"), Val::Nil);
                            }
                            // L182 (the slot is set: L174's lookup found an entry)
                            if let Some(s) = g.slot.get() {
                                st.imbuedSupportBySlot.set(s, Val::Nil);
                            }
                        }

                        // L185
                        g.slot.set(value.0.map(Name::from));
                        // L186
                        SkillsTab::add_undo_state(cx, me);
                        // L187
                        m.buildFlag.set(true);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L189-203
        cx.w_mut::<DropDownControl>(group_slot).tt.tooltip_func = Some(Rc::new(
            |cx: &mut Cx, tooltip: &mut Tooltip, (mode, index, value): DropTipArgs| {
                // L190
                tooltip.clear(false);
                // L191
                if mode == DropTipMode::Out || index == Some(1) {
                    // L192
                    tooltip.add_line(cx, 16.0, "Select the item in which this skill is socketed.");
                    // L193
                    tooltip.add_line(
                        cx,
                        16.0,
                        "This will allow the skill to benefit from modifiers on the item that affect socketed gems.",
                    );
                } else {
                    let slot_name = value
                        .as_ref()
                        .and_then(|v| v.get::<GroupSlotValue>().map(|v| v.0))
                        .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                    // L195
                    let slot = slot_name
                        .and_then(|s| items_slot(cx, s))
                        .unwrap_or_else(|| lua_error!("attempt to index local 'slot' (a nil value)"));
                    // L196
                    let ttItem = items_item(cx, &slot);
                    // L197
                    if let Some(ttItem) = ttItem {
                        // L198
                        let f = skills_hooks().items_add_item_tooltip;
                        f(cx, tooltip, &ttItem, &slot);
                    } else {
                        // L200
                        tooltip.add_line(cx, 16.0, "No item is equipped in this slot.");
                    }
                }
            },
        ));
        // L204-206
        cx.base_mut(group_slot).enabled = Prop::f(|cx, _| dg(cx).source.get().is_none());
        // L207-211
        let group_enabled = cx
            .add(
                me,
                "groupEnabled",
                CheckBoxControl::new(
                    Anchor::new(Left, group_slot, Right),
                    rect(70, 0, 20, 0),
                    Some("Enabled:".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L208
                        dg(cx).enabled.set(state);
                        // L209
                        SkillsTab::add_undo_state(cx, me);
                        // L210
                        cx.build().model.buildFlag.set(true);
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L212-216
        let include_in_full_dps = cx
            .add(
                me,
                "includeInFullDPS",
                CheckBoxControl::new(
                    Anchor::new(Left, group_enabled, Right),
                    rect(145, 0, 20, 0),
                    Some("Include in Full DPS:".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        let g = dg(cx);
                        // L213
                        g.includeInFullDPS.set(state);
                        g.extra.set(n!("$includeInFullDPSNil"), Val::Nil);
                        // L214
                        SkillsTab::add_undo_state(cx, me);
                        // L215
                        cx.build().model.buildFlag.set(true);
                    })),
                    None,
                    false,
                ),
            )
            .id();

        // L218-231 getSelectedItem: [`get_selected_item`]
        // L232-241
        let sockets_label = cx
            .add(
                me,
                "socketsLabel",
                LabelControl::new(
                    Anchor::new(TopLeft, group_slot_label, BottomLeft),
                    rect(0, 8, 0, 16),
                    Prop::f(move |cx, _| {
                        // L233
                        let (item, _) = get_selected_item(cx, group_slot);
                        // L234
                        let mut socketLine = String::new();
                        // L235
                        if let Some(item) = item.as_ref().filter(|i| i.base.get().is_some_and(|b| b.socketLimit.is_none())) {
                            let _ = item;
                            // L236
                            socketLine = "This item cannot have sockets".to_owned();
                        // L237
                        } else if let Some(item) = &item {
                            // L238
                            let f = skills_hooks().items_get_socket_description_line;
                            socketLine = f(cx, item);
                        }
                        // L240
                        format!("^7Item sockets: {socketLine}")
                    }),
                ),
            )
            .id();
        // L242-245
        cx.base_mut(sockets_label).shown = Prop::f(move |cx, _| {
            // L243
            let (item, _) = get_selected_item(cx, group_slot);
            // L244
            item.is_some()
        });
        // L246-255 getSocketCounts: [`get_socket_counts`]
        // L256-289
        let optimise_sockets = cx
            .add(
                me,
                "optimiseSockets",
                ButtonControl::new(Anchor::new(Left, sockets_label, Right), rect(4, 0, 120, 18), "Optimise Sockets", move |cx| {
                    // L257
                    let (item, groupSlot) = get_selected_item(cx, group_slot);
                    // L258
                    let (Some(item), Some(groupSlot)) = (item, groupSlot) else {
                        // L259
                        return;
                    };
                    if item.base.get().is_none() {
                        return;
                    }

                    // L262
                    let f = skills_hooks().items_add_undo_state;
                    f(cx);

                    // L264
                    let (mut maxSockets, abyssalSocketCount) = get_socket_counts(&item);

                    // L266
                    let mut groupCount = 0.0;
                    // L267
                    item.sockets.set(Shared::new(Vec::new()));
                    let m = model(cx);
                    // L268
                    for group in m.skillsTab.socketGroupList.get().snapshot() {
                        // L269
                        let colours = ["R", "G", "B"];
                        // L270
                        if group.slot.get().as_deref() == groupSlot {
                            // L271
                            for gem in group.gemList.get().snapshot() {
                                // L272
                                let grantedEffect = gem_granted_effect(&gem);
                                // L273
                                if let Some(grantedEffect) = grantedEffect.filter(|_| maxSockets > 0.0) {
                                    // L274
                                    let gemColour = grantedEffect
                                        .color
                                        .and_then(|c| colours.get(usize::from(c).wrapping_sub(1)).copied())
                                        .unwrap_or("W");
                                    // L275
                                    let t = Table::new();
                                    t.set(n!("color"), gemColour);
                                    t.set(n!("group"), groupCount);
                                    item.sockets.get().push(t);
                                    // L276
                                    maxSockets -= 1.0;
                                }
                            }
                            // L279
                            groupCount += 1.0;
                        }
                    }
                    // L282
                    let mut i = 0.0;
                    while i <= abyssalSocketCount - 1.0 {
                        // L283
                        groupCount += 1.0;
                        // L284
                        let t = Table::new();
                        t.set(n!("color"), "A");
                        t.set(n!("group"), groupCount);
                        item.sockets.get().push(t);
                        i += 1.0;
                    }
                    // L286
                    item.BuildAndParseRaw();
                    // L287
                    m.skillsTab.UpdateSocketGroups(&m);
                    // L288
                    m.buildFlag.set(true);
                }),
            )
            .id();
        // L290-293
        cx.base_mut(optimise_sockets).shown = Prop::f(move |cx, _| {
            // L291
            let (item, _) = get_selected_item(cx, group_slot);
            // L292
            item.is_some_and(|item| {
                item.base
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
                    .socketLimit
                    .is_some()
            })
        });
        // L294-313
        cx.base_mut(optimise_sockets).enabled = Prop::f(move |cx, _| {
            // L295
            let (item, groupSlot) = get_selected_item(cx, group_slot);
            // L296
            let (Some(item), Some(groupSlot)) = (item, groupSlot) else {
                // L297
                return false;
            };
            if item.base.get().is_none() {
                return false;
            }
            // L299
            let (mut maxSockets, _) = get_socket_counts(&item);
            // L300
            for group in cx.build().model.skillsTab.socketGroupList.get().snapshot() {
                // L301
                if group.slot.get().as_deref() == groupSlot {
                    // L302
                    for gem in group.gemList.get().snapshot() {
                        // L303
                        if gem_granted_effect(&gem).is_some() && maxSockets > 0.0 {
                            // L304
                            if !gem.extra.truthy(n!("matchesSocket")) {
                                // L305
                                return true;
                            }
                            // L307
                            maxSockets -= 1.0;
                        }
                    }
                }
            }
            // L312
            false
        });
        // L314
        cx.w_mut::<ButtonControl>(optimise_sockets).tt.tooltip_text = Some(Prop::Val(Some(
            "Rebuild the item's sockets to match the groups assigned to it.".to_owned(),
        )));
        // self.imbuedSupportBySlot is used by CalcSetup to add an ExtraSupport mod of the selected gem
        // Each displayGroup has its own "imbuedSupport" and is saved to the xml to load when changing sockets or loading a build
        // "slotName" is used on import, which uses builtInSupport to get the gemData and pass in here
        // buildFlag to true triggers the reload/run the CalcSetup to add on the support
        // the last var in the GemSelectControl init, the true, sets imbuedSelect to true which sets the level to 1 and support filtering
        // L320 self.imbuedSupportBySlot = { } (the model's table; the model's Load already filled it)
        // L321
        let crafted = cx.main.cc.CRAFTED.clone();
        let imbued_support_label = cx
            .add(
                me,
                "imbuedSupportLabel",
                LabelControl::new(
                    Anchor::new(TopLeft, sockets_label, BottomLeft).collapse(),
                    rect(0, 8, 0, 16),
                    format!("{crafted}Imbued Support:"),
                ),
            )
            .id();
        // L322-350
        let imbued_change: GemChangeFunc = Rc::new(move |cx: &mut Cx, gem_id: Option<String>, _add_undo: bool, _focus_lost: bool, buf_matches_gem: bool| {
            SkillsTab::imbued_support_gem_change_func(cx, me, gem_id, buf_matches_gem, None);
        });
        let imbued_support = cx
            .add(
                me,
                "imbuedSupport",
                GemSelectControl::new(
                    Anchor::new(Left, imbued_support_label, Right),
                    rect(8, 0, 250, 20),
                    me,
                    1.0,
                    Some(imbued_change),
                    true,
                    true,
                ),
            )
            .id();
        // socketedIn must be set and the displayGroup must have an imbued, otherwise disable the imbued dropdown
        // L351-353
        let is_imbued_enabled = |cx: &Cx| -> bool {
            // L352
            let Some(g) = display_group(cx) else { return false };
            let Some(slot) = g.slot.get() else { return false };
            let by_slot = cx.build().model.skillsTab.imbuedSupportBySlot.get(slot);
            (by_slot.truthy() && g.extra.truthy(n!("imbuedSupport"))) || !by_slot.truthy()
        };
        // L354-356
        cx.base_mut(imbued_support).enabled = Prop::f(move |cx, _| is_imbued_enabled(cx));
        // L357-360 don't show imbued for skills from items
        cx.base_mut(imbued_support_label).shown =
            Prop::f(|cx, _| display_group(cx).is_some_and(|g| g.source.get().is_none()));
        // L361-365
        let imbued_support_clear = cx
            .add(
                me,
                "imbuedSupportClear",
                ButtonControl::new(Anchor::new(Left, imbued_support_label, Right), rect(260, 0, 20, 20), "x", move |cx| {
                    // L362
                    cx.w_mut::<GemSelectControl>(imbued_support).gem_id = None;
                    // L363
                    EditControl::set_text(cx, imbued_support, "", false);
                    // L364 `self.controls.imbuedSupport:gemChangeFunc(nil)` (module notes)
                    SkillsTab::imbued_support_gem_change_func(cx, me, None, false, None);
                }),
            )
            .id();
        // L366-368
        cx.base_mut(imbued_support_clear).enabled = Prop::f(move |cx, _| is_imbued_enabled(cx));
        // L369
        cx.w_mut::<ButtonControl>(imbued_support_clear).tt.tooltip_text =
            Some(Prop::Val(Some("Remove this imbued support.".to_owned())));

        // L371
        let group_count_label = cx
            .add(
                me,
                "groupCountLabel",
                LabelControl::new(Anchor::new(Left, include_in_full_dps, Right), rect(16, 0, 0, 16), "^7Count:"),
            )
            .id();
        // L372-374
        cx.base_mut(group_count_label).shown = Prop::f(|cx, _| dg(cx).source.get().is_some());
        // L375-379
        let group_count = cx
            .add(
                me,
                "groupCount",
                EditControl::new(
                    Anchor::new(Left, group_count_label, Right),
                    rect(4, 0, 60, 20),
                    EditArgs {
                        filter: Some("%D".to_owned()),
                        limit: Some(2),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L376
                            dg(cx).groupCount.set(Some(tonumber(buf).unwrap_or(1.0)));
                            // L377
                            SkillsTab::add_undo_state(cx, me);
                            // L378
                            cx.build().model.buildFlag.set(true);
                        })),
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L380-382
        cx.base_mut(group_count).shown = Prop::f(|cx, _| dg(cx).source.get().is_some());
        // L383
        let source_note = cx
            .add(
                me,
                "sourceNote",
                LabelControl::new(Anchor::new(TopLeft, sockets_label, TopLeft), rect(0, 30, 0, 16), ""),
            )
            .id();
        // L384-386
        cx.base_mut(source_note).shown = Prop::f(|cx, _| dg(cx).source.get().is_some());
        // L387-417
        cx.w_mut::<LabelControl>(source_note).label = Prop::f(|cx, _| Self::source_note_label(cx));

        // Scroll bar
        // L420
        cx.add(
            me,
            "scrollBarH",
            ScrollBarControl::new(Anchor::NONE, rect(0, 0, 0, 18), Some(100.0), Some(ScrollDir::Horizontal), true),
        );

        // Initialise skill sets
        // L423-425 (model half: calc:BM-A)
        // L426 self:SetActiveSkillSet(1): the model half ran in calc:BM-A; its UI tail (L1495)
        // points the list at the model's current list (DIVERGENCE(ctor-sync-loadouts) for L1501)
        Self::set_group_list_list(cx, me);

        // Skill gem slots
        // L429
        let anchor_gem_slots = cx
            .add_free(
                Some(me),
                PlainControl::new(Anchor::new(TopLeft, imbued_support_label, BottomLeft), rect(0, 30, 0, 0)),
            )
            .id();
        cx.w_mut::<SkillsTab>(me).core.anchor_gem_slots = anchor_gem_slots;
        // L430
        cx.w_mut::<SkillsTab>(me).core.gem_slots = Vec::new();
        // L431
        (hooks.create_gem_slot)(cx, me, 1);
        let slot1 = Self::gem_slot(cx, me, 1);
        // L432
        cx.add(
            me,
            "gemNameHeader",
            LabelControl::new(Anchor::new(BottomLeft, slot1.name_spec, TopLeft), rect(0, -2, 0, 16), "^7Gem name:"),
        );
        // L433
        cx.add(
            me,
            "gemLevelHeader",
            LabelControl::new(Anchor::new(BottomLeft, slot1.level, TopLeft), rect(0, -2, 0, 16), "^7Level:"),
        );
        // L434
        cx.add(
            me,
            "gemQualityHeader",
            LabelControl::new(Anchor::new(BottomLeft, slot1.quality, TopLeft), rect(0, -2, 0, 16), "^7Quality:"),
        );
        // L435
        cx.add(
            me,
            "gemEnableHeader",
            LabelControl::new(Anchor::new(BottomLeft, slot1.enabled, TopLeft), rect(-16, -2, 0, 16), "^7Enabled:"),
        );
        // L436
        cx.add(
            me,
            "gemCountHeader",
            LabelControl::new(Anchor::new(BottomLeft, slot1.count, TopLeft), rect(8, -2, 0, 16), "^7Count:"),
        );
        // L437 return self
    }

    /// `self.controls.sourceNote.label` (L387-417).
    fn source_note_label(cx: &Cx) -> String {
        let g = dg(cx);
        // L388
        let mut label;
        // L389
        if let Some(explodeSources) = g.explodeSources.get() {
            // L390-391
            label = "^7This is a special group created for the enemy explosion effect,\nwhich comes from the following sources:".to_owned();
            // L392
            for (_, source) in explodeSources.ipairs() {
                // L393
                let rarity = val_rarity(&source).unwrap_or_else(|| "NORMAL".to_owned());
                let name = val_name(&source)
                    .or_else(|| val_dn(&source))
                    .unwrap_or_else(|| "???".to_owned());
                label = format!("{label}\n\t{}{name}", color_code_cat(cx, &rarity));
            }
            // L395
            label.push_str("^7\nYou cannot delete this group, but it will disappear if you lose the above sources.");
        } else {
            // L397
            let activeGem = g
                .gemList
                .get()
                .get(1)
                .unwrap_or_else(|| lua_error!("attempt to index local 'activeGem' (a nil value)"));
            // L398
            let mut sourceName;
            let source_item = g.extra.get(n!("sourceItem"));
            let source_node = g.extra.get(n!("sourceNode"));
            // L399
            if source_item.truthy() {
                // L400
                let rarity = val_rarity(&source_item)
                    .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                let name = val_name(&source_item)
                    .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                sourceName = format!("'{}{name}", color_code_cat(cx, &rarity));
            // L401
            } else if source_node.truthy() {
                // L402 (DIVERGENCE(node-name))
                let name = val_name(&source_node)
                    .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                sourceName = format!("'{}{name}", cx.main.cc.NORMAL);
            } else {
                // L404
                sourceName = format!("'{}?", cx.main.cc.NORMAL);
            }
            // L406
            sourceName.push_str("^7'");
            // L407-409
            let color = activeGem
                .extra
                .str(n!("color"))
                .unwrap_or_else(|| lua_error!("attempt to concatenate field 'color' (a nil value)"));
            let gem_name = activeGem
                .grantedEffect
                .get()
                .map_or_else(|| activeGem.nameSpec.get().to_string(), |ge| ge.name.to_string());
            let tail = if source_node.truthy() {
                "un-allocate the node."
            } else {
                "un-equip the item."
            };
            label = format!(
                "^7This is a special group created for the '{color}{gem_name}^7' skill,\nwhich is being provided by {sourceName}.\nYou cannot delete this group, but it will disappear if you {tail}"
            );
            // L410
            if !g.noSupports.get() && !source_node.truthy() {
                // L411-413
                label = format!(
                    "{label}\n\nYou cannot add support gems to this group, but support gems in\nany other group socketed into {sourceName}\nwill automatically apply to the skill."
                );
            }
        }
        // L416
        label
    }

    /// The imbued-support selector's change function (L322-350): `gemData` is the gem id the
    /// selector passes (or nil), `gemMatch` its 4th argument, `slotName` the 5th. The model half
    /// is calc:BM-B's `imbuedSupport_gemChangeFunc`; L328 and L338 are this half.
    pub fn imbued_support_gem_change_func(
        cx: &mut Cx,
        me: CtrlId,
        gem_id: Option<String>,
        gem_match: bool,
        slot_name: Option<Name>,
    ) {
        let m = model(cx);
        // L323
        let targetSlot = slot_name.or_else(|| display_group(cx).and_then(|g| g.slot.get()));
        // L324
        if targetSlot.is_none() {
            // L325
            return;
        }
        // L328
        Self::add_undo_state(cx, me);
        // L329-330 (`gemData = nil` unless gemMatch)
        let gem = if gem_match {
            gem_id.map(|id| {
                // L333 local gem = data.gems[gemData.id or gemData]
                m.data
                    .gems
                    .get(&Name::from(id.as_str()))
                    .cloned()
                    .unwrap_or_else(|| lua_error!("attempt to index local 'gem' (a nil value)"))
            })
        } else {
            None
        };
        // L323-L349 (model half: calc:BM-B)
        m.skillsTab
            .imbuedSupport_gemChangeFunc(&m, gem.clone(), gem_match, slot_name);
        // L338
        if let Some(gem) = gem {
            let col = skill_color_map(cx, gem.grantedEffect.color)
                .unwrap_or_else(|| lua_error!("CCR-0118: skillColorMap[nil]"));
            let imb = Self::control(cx, me, "imbuedSupport");
            cx.w_mut::<EditControl>(imb).inactive_col = col;
        }
    }

    /// `self.controls[key]` (nil is the Lua index error).
    #[track_caller]
    pub fn control(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
        cx.child(me, key)
            .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
    }

    /// `self.gemSlots[index]` (nil is the Lua index error).
    #[track_caller]
    pub fn gem_slot(cx: &Cx, me: CtrlId, index: usize) -> GemSlot {
        cx.w::<SkillsTab>(me)
            .core
            .gem_slots
            .get(index.wrapping_sub(1))
            .copied()
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
    }

    /// `self.gemSlots[index] ~= nil`
    pub fn has_gem_slot(cx: &Cx, me: CtrlId, index: usize) -> bool {
        index >= 1 && index <= cx.w::<SkillsTab>(me).core.gem_slots.len()
    }

    /// `self.gemSlots[index] = slot` (CreateGemSlot, L745): `index` is at most `#gemSlots + 1`.
    pub fn set_gem_slot(cx: &mut Cx, me: CtrlId, index: usize, slot: GemSlot) {
        let slots = &mut cx.w_mut::<SkillsTab>(me).core.gem_slots;
        if index == slots.len() + 1 {
            slots.push(slot);
        } else if index >= 1 && index <= slots.len() {
            slots[index - 1] = slot;
        } else {
            lua_error!("CCR-0118: gemSlots[{index}] with {} slots (a hole)", slots.len());
        }
    }

    /// `self.modFlag`
    pub fn mod_flag_of(cx: &Cx, me: CtrlId) -> bool {
        cx.w::<SkillsTab>(me).core.undo.mod_flag
    }

    /// `self.modFlag = v`
    pub fn set_mod_flag_of(cx: &mut Cx, me: CtrlId, v: bool) {
        cx.w_mut::<SkillsTab>(me).core.undo.mod_flag = v;
    }

    /// `self:AddUndoState()` (UndoHandler.lua:34-41)
    pub fn add_undo_state(cx: &mut Cx, me: CtrlId) {
        UNDO.add_undo_state(cx, me, false);
    }

    /// `self.controls.groupList.list = self.socketGroupList` (L1495).
    fn set_group_list_list(cx: &mut Cx, me: CtrlId) {
        let gl = Self::control(cx, me, "groupList");
        let list = cx.build().model.skillsTab.socketGroupList.get();
        cx.w_mut::<ListControl>(gl).list = ListData::Source(Rc::new(GroupListSource(list)));
    }

    /// The UI statements of `SkillsTab:Load` (L528-549, L569-570; DIVERGENCE(load-ui-tail)): the
    /// model's Load already set the values these controls show.
    /// (lines of calc:BM-B's `@pob Classes/SkillsTab.lua:523-571` segment)
    fn load_ui_tail(cx: &mut Cx, me: CtrlId) {
        let m = model(cx);
        let ui = m.skillsTab.ui_state();
        // L528-534 self.controls.defaultLevel:SelByValue(..., "gemLevel")
        let dl = Self::control(cx, me, "defaultLevel");
        cx.w_mut::<DropDownControl>(dl).sel_index = ui.num(n!("defaultLevelSelIndex")) as usize;
        // L535 (model half)
        // L536 (model half)
        // L537 self.controls.defaultQuality:SetText(self.defaultGemQuality or "")
        let dq = Self::control(cx, me, "defaultQuality");
        let q = m.skillsTab.defaultGemQuality.get().map_or_else(String::new, tostring_num);
        EditControl::set_text(cx, dq, &q, false);
        // L538-540 (model half)
        // L541 self.controls.sortGemsByDPS.state = self.sortGemsByDPS
        let sg = Self::control(cx, me, "sortGemsByDPS");
        cx.w_mut::<CheckBoxControl>(sg).state = ui.truthy(n!("sortGemsByDPS"));
        // L542-544 (model half)
        // L545 self.controls.showLegacyGems.state = self.showLegacyGems
        let sl = Self::control(cx, me, "showLegacyGems");
        cx.w_mut::<CheckBoxControl>(sl).state = ui.truthy(n!("showLegacyGems"));
        // L546 self.controls.showSupportGemTypes:SelByValue(...)
        let ss = Self::control(cx, me, "showSupportGemTypes");
        cx.w_mut::<DropDownControl>(ss).sel_index = ui.num(n!("showSupportGemTypesSelIndex")) as usize;
        // L547 self.controls.sortGemsByDPSFieldControl:SelByValue(...)
        let sf = Self::control(cx, me, "sortGemsByDPSFieldControl");
        cx.w_mut::<DropDownControl>(sf).sel_index = ui.num(n!("sortGemsByDPSFieldSelIndex")) as usize;
        // L548-549 (model half)
        // L550-567 LoadSkill: the imbued-support call returns at L325 (DIVERGENCE(load-ui-tail))
        // L569 self:SetActiveSkillSet(...): the model half ran; its UI tail
        Self::set_active_skill_set_ui_tail(cx, me, false);
        // L570
        UNDO.reset_undo(cx, me);
    }

    /// lua: SkillsTab.lua:631-697 `SkillsTab:Draw(viewPort, inputEvents)`
    /// @pob Classes/SkillsTab.lua:630-692 h=d2a94c18
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        let scroll_bar_h = Self::control(cx, me, "scrollBarH");
        {
            let b = cx.base_mut(me);
            // L632-635
            b.x = vp.x.into();
            b.y = vp.y.into();
            b.width = vp.width.into();
            b.height = vp.height.into();
        }
        {
            let b = cx.base_mut(scroll_bar_h);
            // L636
            b.width = vp.width.into();
            // L637
            b.x = vp.x.into();
            // L638
            b.y = (vp.y + vp.height - 18.0).into();
        }

        // L640-644
        {
            let header = Self::control(cx, me, "gemCountHeader");
            // L641
            let maxX = cx.pos(header).0 + cx.size(header).0 + 350.0;
            // L642
            let contentWidth = maxX - cx.base(me).x.raw();
            // L643
            ScrollBarControl::set_content_dimension(cx, scroll_bar_h, contentWidth, vp.width);
        }
        // L645
        let x = cx.base(me).x.raw() - cx.w::<ScrollBarControl>(scroll_bar_h).offset;
        cx.base_mut(me).x = Prop::Val(x);

        // L647
        let mut i = 0;
        while let Some(event) = ev.next_ipairs(&mut i) {
            // L648
            if let EventKind::KeyDown { .. } = event.kind {
                // L649
                if event.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L650
                    UNDO.undo(cx, me);
                    // L651
                    cx.build().model.buildFlag.set(true);
                // L652
                } else if event.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L653
                    UNDO.redo(cx, me);
                    // L654
                    cx.build().model.buildFlag.set(true);
                // L655
                } else if event.key == Key::Char(b'v') && cx.sys.is_key_down(Key::Ctrl) {
                    // L656
                    Self::paste_socket_group(cx, me, None);
                }
            }
        }
        // L660
        cx.process_controls_input(me, ev, vp);
        // L661
        let mut i = 0;
        while let Some(event) = ev.next_ipairs(&mut i) {
            // L662
            if let EventKind::KeyUp = event.kind {
                // L663
                if ScrollBarControl::is_scroll_down_key(cx, scroll_bar_h, event.key) {
                    // L664
                    ScrollBarControl::scroll(cx, scroll_bar_h, 1.0);
                // L665
                } else if ScrollBarControl::is_scroll_up_key(cx, scroll_bar_h, event.key) {
                    // L666
                    ScrollBarControl::scroll(cx, scroll_bar_h, -1.0);
                }
            }
        }

        // L671
        draw_background(cx, vp);

        // L673
        let mut newSetList = Vec::new();
        let set_select = Self::control(cx, me, "setSelect");
        let m = model(cx);
        let order = m.skillsTab.skillSetOrderList.with(Clone::clone);
        // L674
        for (index0, skillSetId) in order.iter().enumerate() {
            // L675
            let skillSet = m
                .skillsTab
                .skillSets
                .with(|s| s.get(skillSetId).cloned())
                .unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"));
            // L676
            let title = skillSet
                .str(n!("title"))
                .map_or_else(|| "Default".to_owned(), |s| s.to_string());
            newSetList.push(DropItem::str(title));
            // L677
            if f64::from(*skillSetId) == m.skillsTab.activeSkillSetId.get() {
                // L678
                cx.w_mut::<DropDownControl>(set_select).sel_index = index0 + 1;
            }
        }
        // L681
        DropDownControl::set_list(cx, set_select, newSetList);

        let core = &cx.w::<SkillsTab>(me).core;
        let anchor_group_detail = core.anchor_group_detail;
        // L683
        if cx.app().main.portrait_mode {
            // L684
            let option_section = Self::control(cx, me, "optionSection");
            cx.set_anchor(anchor_group_detail, TopLeft, Some(option_section), BottomLeft, Some((0.0.into(), 20.0.into())), false);
        } else {
            // L686
            let group_list = Self::control(cx, me, "groupList");
            cx.set_anchor(anchor_group_detail, TopLeft, Some(group_list), TopRight, Some((20.0.into(), 0.0.into())), false);
        }

        // L689
        Self::update_gem_slots(cx, me);

        // L691
        cx.draw_controls(me, vp, None);
    }

    /// lua: SkillsTab.lua:694-706 `SkillsTab:CopySocketGroup(socketGroup)`
    /// @pob Classes/SkillsTab.lua:693-707 h=5a236dac
    pub fn copy_socket_group(cx: &mut Cx, _me: CtrlId, socketGroup: &SocketGroupRef) {
        // L695
        let mut skillText = String::new();
        // L696
        if let Some(label) = socketGroup_label(socketGroup).filter(|l| pob_lua::find(l, "%S", None, false).is_some()) {
            // L697
            skillText = format!("{skillText}Label: {label}\r\n");
        }
        // L699
        if let Some(slot) = socketGroup.slot.get() {
            // L700
            skillText = format!("{skillText}Slot: {slot}\r\n");
        }
        // L702
        for gemInstance in socketGroup.gemList.get().snapshot() {
            // L703
            let line = lfmt!(
                "%s %d/%d %s %d\r\n",
                &gemInstance.nameSpec.get(),
                nan_nil(gemInstance.level.get()),
                nan_nil(gemInstance.quality.get()),
                if gemInstance.enabled.get() { "" } else { "DISABLED" },
                gemInstance.count.get().unwrap_or(1.0)
            );
            skillText.push_str(&line);
        }
        // L705
        cx.sys.platform.copy(&skillText);
    }

    /// lua: SkillsTab.lua:708-740 `SkillsTab:PasteSocketGroup(testInput)`
    /// @pob Classes/SkillsTab.lua:708-741 h=3bd33b26
    pub fn paste_socket_group(cx: &mut Cx, me: CtrlId, testInput: Option<&str>) {
        // L709
        let skillText = cx
            .sys
            .platform
            .paste()
            .or_else(|| testInput.map(str::to_owned))
            .map(|t| pob_lua::string::sanitiseText(&t));
        // L710
        if let Some(skillText) = skillText {
            // L711
            let newGroup: SocketGroupRef = Rc::new(SocketGroup::default());
            set_socketGroup_label(&newGroup, Some(""));
            newGroup.enabled.set(true);
            newGroup.gemList.set(Shared::new(Vec::new()));
            // `includeInFullDPS` is nil in the table constructor (CCR-0019's marker)
            newGroup.extra.set(n!("$includeInFullDPSNil"), true);
            // DIVERGENCE(paste-main-active-skill): `mainActiveSkill`/`mainActiveSkillCalcs` are
            // nil here; every Lua reader writes `or 1`, the frozen fields are numbers (CCR-0118 §3)
            newGroup.mainActiveSkill.set(1.0);
            newGroup.mainActiveSkillCalcs.set(1.0);
            // L712
            let label = pob_lua::lmatch(&skillText, "Label: (%C+)", None);
            // L713
            if let Some(label) = label {
                // L714
                set_socketGroup_label(&newGroup, Some(&label[0].to_str()));
            }
            // L716
            let slot = pob_lua::lmatch(&skillText, "Slot: (%C+)", None);
            // L717
            if let Some(slot) = slot {
                // L718
                newGroup.slot.set(Some(Name::from(&slot[0].to_str())));
            }
            // L720
            for caps in pob_lua::gmatch(&skillText, "([ %a']+) (%d+)/(%d+) ?(%a*) (%d+)") {
                let (nameSpec, level, quality, state, count) = (
                    caps[0].to_str().into_owned(),
                    caps[1].to_str().into_owned(),
                    caps[2].to_str().into_owned(),
                    caps[3].to_str().into_owned(),
                    caps[4].to_str().into_owned(),
                );
                // L721-729
                let gem = GemInstance::default();
                gem.nameSpec.set(LStr::from(nameSpec.as_str()));
                gem.level.set(tonumber(&level).unwrap_or(20.0));
                gem.quality.set(tonumber(&quality).unwrap_or(0.0));
                gem.enabled.set(state != "DISABLED");
                gem.count.set(Some(tonumber(&count).unwrap_or(1.0)));
                gem.enableGlobal1.set(true);
                gem.enableGlobal2.set(true);
                newGroup.gemList.get().push(Rc::new(gem));
            }
            // L731
            if newGroup.gemList.get().len() > 0 {
                let m = model(cx);
                let list = m.skillsTab.socketGroupList.get();
                // L732
                list.push(newGroup.clone());
                let gl = Self::control(cx, me, "groupList");
                let l = cx.w_mut::<ListControl>(gl);
                // L733
                l.sel_index = Some(list.len());
                // L734
                l.sel_value = Some(ListValue::of(newGroup.clone()));
                // L735
                Self::set_display_group(cx, me, Some(newGroup));
                // L736
                Self::add_undo_state(cx, me);
                // L737
                m.buildFlag.set(true);
            }
        }
    }

    /// lua: SkillsTab.lua:1081-1100 `SkillsTab:UpdateGemSlots()`
    /// @pob Classes/SkillsTab.lua:1081-1102 h=986ebd5d
    pub fn update_gem_slots(cx: &mut Cx, me: CtrlId) {
        // L1082
        let Some(displayGroup) = display_group(cx) else {
            // L1083
            return;
        };
        let gemList = displayGroup.gemList.get();
        // L1085
        let n = gemList.len() + 1;
        for slotIndex in 1..=n {
            // L1086
            if !Self::has_gem_slot(cx, me, slotIndex) {
                // L1087
                let f = skills_hooks().create_gem_slot;
                f(cx, me, slotIndex);
            }
            // L1089
            let slot = Self::gem_slot(cx, me, slotIndex);
            // L1090 (`#self.displayGroup.gemList` is read again: CreateGemSlot does not change it)
            if slotIndex == gemList.len() + 1 {
                // L1091
                EditControl::set_text(cx, slot.name_spec, "", false);
                // L1092
                EditControl::set_text(cx, slot.level, "", false);
                // L1093
                EditControl::set_text(cx, slot.quality, "", false);
                // L1094
                cx.w_mut::<CheckBoxControl>(slot.enabled).state = false;
                // L1095
                EditControl::set_text(cx, slot.count, "1", false);
            } else {
                // L1097 (BM-B's ProcessSocketGroup always sets `color`)
                let color = gemList
                    .get(slotIndex)
                    .and_then(|g| g.extra.str(n!("color")))
                    .map_or_else(String::new, |s| s.to_string());
                cx.w_mut::<EditControl>(slot.name_spec).inactive_col = color;
            }
        }
    }

    /// lua: SkillsTab.lua:1275-1307 `SkillsTab:SetDisplayGroup(socketGroup)`: the model half
    /// (L1276-1278, calc:BM-B) and the controls (L1280-1306).
    /// @pob Classes/SkillsTab.lua:1274-1274 h=f449aee3
    /// @pob Classes/SkillsTab.lua:1279-1308 h=7d430c87
    pub fn set_display_group(cx: &mut Cx, me: CtrlId, socketGroup: Option<SocketGroupRef>) {
        let m = model(cx);
        // L1276-1278 (model half: calc:BM-B)
        m.skillsTab.SetDisplayGroup(&m, socketGroup.clone());
        if let Some(socketGroup) = socketGroup {
            Self::set_display_group_ui_tail(cx, me, &socketGroup);
        }
    }

    /// L1280-1306: the controls of `SetDisplayGroup` (inside `if socketGroup then`).
    fn set_display_group_ui_tail(cx: &mut Cx, me: CtrlId, socketGroup: &SocketGroupRef) {
        // Update the main controls
        // L1281
        let gl = Self::control(cx, me, "groupLabel");
        let label = socketGroup_label(socketGroup).map_or_else(|| "nil".to_owned(), |s| s.to_string());
        EditControl::set_text(cx, gl, &label, false);
        // L1282
        let gs = Self::control(cx, me, "groupSlot");
        let slot = socketGroup.slot.get();
        DropDownControl::sel_by_value(cx, gs, &|v: &DropItem| {
            v.get::<GroupSlotValue>().map(|s| s.0) == Some(slot.as_deref())
        });
        // L1283
        let ge = Self::control(cx, me, "groupEnabled");
        cx.w_mut::<CheckBoxControl>(ge).state = socketGroup.enabled.get();
        // L1284
        let fd = Self::control(cx, me, "includeInFullDPS");
        cx.w_mut::<CheckBoxControl>(fd).state =
            socketGroup_includeInFullDPS(socketGroup).truthy() && socketGroup.enabled.get();
        // L1285
        let gc = Self::control(cx, me, "groupCount");
        let count = tostring_num(socketGroup.groupCount.get().unwrap_or(1.0));
        EditControl::set_text(cx, gc, &count, false);
        let imb = Self::control(cx, me, "imbuedSupport");
        let imbuedSupport = socketGroup.extra.get(n!("imbuedSupport"));
        // L1286
        if imbuedSupport.truthy() {
            let name = tostring_val(&imbuedSupport);
            // L1287
            let d = model::data();
            let key = format!("{} support", pob_lua::string::lower(&name));
            let gemId: Option<GemDataRef> = d
                .gemForBaseName
                .get(key.as_str())
                .and_then(|id| d.gems.get(id))
                .cloned();
            // L1288
            cx.w_mut::<GemSelectControl>(imb).gem_id = gemId.clone().map(GemIdVal::Data);
            // L1289
            EditControl::set_text(cx, imb, &name, false);
            // L1290
            let gemId = gemId.unwrap_or_else(|| lua_error!("attempt to index local 'gemId' (a nil value)"));
            let col = skill_color_map(cx, gemId.grantedEffect.color)
                .unwrap_or_else(|| lua_error!("CCR-0118: skillColorMap[nil]"));
            cx.w_mut::<EditControl>(imb).inactive_col = col;
        } else {
            // L1292
            cx.w_mut::<GemSelectControl>(imb).gem_id = None;
            // L1293
            EditControl::set_text(cx, imb, "", false);
        }

        // Update the gem slot controls
        // L1297
        Self::update_gem_slots(cx, me);
        // L1298 ORDER: (c) `pairs` over a sequence visits 1..n
        for (index0, gemInstance) in socketGroup.gemList.get().snapshot().iter().enumerate() {
            let slot = Self::gem_slot(cx, me, index0 + 1);
            // L1299
            EditControl::set_text(cx, slot.name_spec, &gemInstance.nameSpec.get(), false);
            // L1300
            EditControl::set_text(cx, slot.level, &tostring_nan(gemInstance.level.get()), false);
            // L1301
            EditControl::set_text(cx, slot.quality, &tostring_nan(gemInstance.quality.get()), false);
            // L1302
            cx.w_mut::<CheckBoxControl>(slot.enabled).state = gemInstance.enabled.get();
            // L1303
            cx.w_mut::<CheckBoxControl>(slot.enable_global1).state = gemInstance.enableGlobal1.get();
            // L1304
            cx.w_mut::<CheckBoxControl>(slot.enable_global2).state = gemInstance.enableGlobal2.get();
            // L1305
            let count = tostring_num(gemInstance.count.get().unwrap_or(1.0));
            EditControl::set_text(cx, slot.count, &count, false);
        }
    }

    /// lua: SkillsTab.lua:1309-1395 `SkillsTab:AddSocketGroupTooltip(tooltip, socketGroup)`
    /// @pob Classes/SkillsTab.lua:1309-1396 h=ac37696d
    pub fn add_socket_group_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, socketGroup: &SocketGroupRef) {
        // L1310
        if let Some(explodeSources) = socketGroup.explodeSources.get() {
            // L1311
            for (_, source) in explodeSources.ipairs() {
                // L1312
                let rarity = val_rarity(&source).unwrap_or_else(|| "NORMAL".to_owned());
                let name = val_name(&source)
                    .or_else(|| val_dn(&source))
                    .unwrap_or_else(|| "???".to_owned());
                let line = format!("^7Source: {}{name}", color_code_cat(cx, &rarity));
                tooltip.add_line(cx, 18.0, &line);
            }
            // L1314
            return;
        }
        // L1316
        if socketGroup.enabled.get() && !socketGroup.slotEnabled.get() {
            // L1317
            tooltip.add_line(cx, 16.0, "^7Note: this group is disabled because it is socketed in the inactive weapon set.");
        }
        // L1319
        let source_item = socketGroup.extra.get(n!("sourceItem"));
        let sourceSingle = if source_item.truthy() {
            source_item
        } else {
            socketGroup.extra.get(n!("sourceNode"))
        };
        // L1320
        if sourceSingle.truthy() {
            // L1321 (DIVERGENCE(node-name))
            let rarity = val_rarity(&sourceSingle).unwrap_or_else(|| "NORMAL".to_owned());
            let name = val_name(&sourceSingle).unwrap_or_else(|| lua_error!("attempt to concatenate field 'name' (a nil value)"));
            let line = format!("^7Source: {}{name}", color_code_cat(cx, &rarity));
            tooltip.add_line(cx, 18.0, &line);
            // L1322
            tooltip.add_separator(cx, Some(10.0));
        }
        // L1324
        let mut gemShown: Vec<GemInstanceRef> = Vec::new();
        let displaySkillList = socketGroup
            .displaySkillList
            .get()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
        let skills = displaySkillList.snapshot();
        // L1325
        for (index0, activeSkill) in skills.iter().enumerate() {
            let index = index0 + 1;
            // L1326
            if index > 1 {
                // L1327
                tooltip.add_separator(cx, Some(10.0));
            }
            // L1329
            tooltip.add_line(cx, 16.0, &format!("^7Active Skill #{index}:"));
            // L1330
            for skillEffect in activeSkill.effectList.get().snapshot() {
                // L1331-1338
                let line = effect_line(cx, &skillEffect);
                tooltip.add_line(cx, 20.0, &line);
                // L1339
                if let Some(src) = skillEffect.srcInstance.get() {
                    // L1340
                    gemShown.push(src);
                }
            }
            // L1343
            if let Some(minion) = activeSkill.minion.get() {
                // L1344
                tooltip.add_separator(cx, Some(10.0));
                // L1345
                tooltip.add_line(cx, 16.0, &format!("^7Active Skill #{index}'s Main Minion Skill:"));
                // L1346
                let activeEffect = minion
                    .mainSkill
                    .get()
                    .and_then(|s| s.effectList.get().get(1))
                    .unwrap_or_else(|| lua_error!("attempt to index local 'activeEffect' (a nil value)"));
                // L1347-1354
                let line = effect_line(cx, &activeEffect);
                tooltip.add_line(cx, 20.0, &line);
                // L1355
                if let Some(src) = activeEffect.srcInstance.get() {
                    // L1356
                    gemShown.push(src);
                }
            }
        }
        // L1360
        let mut showOtherHeader = true;
        // L1361 `socketGroup.displayGemList or socketGroup.gemList`
        let gems: Vec<GemInstanceRef> = match socketGroup.extra.table(n!("displayGemList")) {
            Some(t) => t
                .ipairs()
                .into_iter()
                .map(|(_, v)| {
                    v.obj::<GemInstance>()
                        .unwrap_or_else(|| lua_error!("CCR-0118: displayGemList entry is not a gem"))
                })
                .collect(),
            None => socketGroup.gemList.get().snapshot(),
        };
        for gemInstance in gems {
            // L1362
            if !gemShown.iter().any(|g| Rc::ptr_eq(g, &gemInstance)) {
                // L1363
                if showOtherHeader {
                    // L1364
                    showOtherHeader = false;
                    // L1365
                    tooltip.add_separator(cx, Some(10.0));
                    // L1366
                    tooltip.add_line(cx, 16.0, "^7Inactive Gems:");
                }
                // L1368
                let mut reason = "";
                // L1369
                let displayEffect = match gemInstance.displayEffect.get() {
                    Some(e) => DisplayEffect::Effect(e),
                    None => DisplayEffect::Gem(gemInstance.clone()),
                };
                // L1370
                let grantedEffect = gemInstance
                    .gemData
                    .get()
                    .map(|g| g.grantedEffect.clone())
                    .or_else(|| gemInstance.grantedEffect.get());
                // L1371
                if let Some(grantedEffect) = &grantedEffect {
                    // L1373
                    if !gemInstance.enabled.get() {
                        // L1374
                        reason = "(Disabled)";
                    // L1375
                    } else if !socketGroup.enabled.get() || !socketGroup.slotEnabled.get() {
                    // L1376
                    } else if grantedEffect.support {
                        // L1377
                        if displayEffect.superseded() {
                            // L1378
                            reason = "(Superseded)";
                        // L1379
                        } else if displayEffect.supports_nothing() && skills.len() > 0 {
                            // L1380
                            reason = "(Cannot apply to any of the active skills)";
                        }
                    }
                } else {
                    // L1372
                    reason = "(Unsupported)";
                }
                // L1383-1391
                let magic = cx.main.cc.MAGIC.clone();
                let color = gemInstance
                    .extra
                    .str(n!("color"))
                    .map(|s| s.to_string());
                let name = gemInstance
                    .grantedEffect
                    .get()
                    .map(|ge| ge.name.to_string())
                    .or_else(|| gemInstance.gemData.get().map(|g| g.name.to_string()))
                    .unwrap_or_else(|| gemInstance.nameSpec.get().to_string());
                let src = displayEffect.src_instance();
                let level = src.as_ref().map_or(displayEffect.level(), |s| s.level.get());
                let level_plus = if gt(displayEffect.level(), gemInstance.level.get()) {
                    format!("{magic}+{}^7", tostring_num(displayEffect.level() - gemInstance.level.get()))
                } else {
                    String::new()
                };
                let quality = src.as_ref().map_or(displayEffect.quality(), |s| s.quality.get());
                let quality_plus = if gt(displayEffect.quality(), gemInstance.quality.get()) {
                    format!("{magic}+{}^7", tostring_num(displayEffect.quality() - gemInstance.quality.get()))
                } else {
                    String::new()
                };
                let line = lfmt!(
                    "%s%s ^7%d%s/%d%s %s",
                    color.as_deref(),
                    name.as_str(),
                    nan_nil(level),
                    level_plus.as_str(),
                    nan_nil(quality),
                    quality_plus.as_str(),
                    reason
                );
                tooltip.add_line(cx, 20.0, &line);
            }
        }
    }

    /// lua: SkillsTab.lua:1397-1418 `SkillsTab:CreateUndoState()`
    /// @pob Classes/SkillsTab.lua:1397-1419 h=87364554
    fn create_undo_state(cx: &mut Cx, _me: CtrlId) -> UndoState {
        let m = model(cx);
        let st = &m.skillsTab;
        // L1398 local state = { }; L1399
        let activeSkillSetId = st.activeSkillSetId.get();
        // L1400
        let mut skillSets = Vec::new();
        // L1401 ORDER: (c) the model map's order (module notes)
        let sets: Vec<(u32, Table)> = st.skillSets.with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
        for (skillSetIndex, skillSet) in sets {
            // L1402
            let newSkillSet = skillSet.copyTable(true);
            // L1403
            let mut newList: Vec<SocketGroupRef> = Vec::new();
            // L1404
            for socketGroup in skillSet_socketGroupList(&skillSet).snapshot() {
                // L1405
                let newGroup = copy_socket_group(&socketGroup);
                // L1406-1409
                let gems: Vec<GemInstanceRef> = socketGroup
                    .gemList
                    .get()
                    .snapshot()
                    .iter()
                    .map(|gem| Rc::new(copy_gem_instance(gem)))
                    .collect();
                newGroup.gemList.set(Shared::new(gems));
                // L1410
                newList.push(Rc::new(newGroup));
            }
            newSkillSet.set(
                n!("socketGroupList"),
                Val::Obj(Rc::new(Shared::new(newList)) as Rc<dyn std::any::Any>),
            );
            // L1412
            skillSets.push((skillSetIndex, newSkillSet));
        }
        // L1414
        let skillSetOrderList = st.skillSetOrderList.with(Clone::clone);
        // Save active socket group for both skillsTab and calcsTab to UndoState
        // L1416
        let activeSocketGroup = m.mainSocketGroup.get();
        // L1417
        let activeSocketGroup2 = m.calcsTab.input.get(n!("skill_number"));
        // L1418
        UndoState {
            activeSkillSetId,
            skillSets,
            skillSetOrderList,
            activeSocketGroup,
            activeSocketGroup2,
        }
    }

    /// lua: SkillsTab.lua:1421-1441 `SkillsTab:RestoreUndoState(state)`
    /// @pob Classes/SkillsTab.lua:1420-1442 h=bf9adf26
    fn restore_undo_state(cx: &mut Cx, me: CtrlId, state: UndoState) {
        let m = model(cx);
        let st = &m.skillsTab;
        // L1422
        let displayId = display_group(cx).and_then(|dgr| {
            st.socketGroupList
                .get()
                .snapshot()
                .iter()
                .position(|g| Rc::ptr_eq(g, &dgr))
                .map(|i| i + 1)
        });
        // L1423
        st.skillSets.with_mut(|s| s.clear());
        // L1424-1426 ORDER: (c) the state's order (module notes)
        st.skillSets.with_mut(|s| {
            for (k, v) in state.skillSets {
                s.insert(k, v);
            }
        });
        // L1427
        st.skillSetOrderList.wipe();
        // L1428-1430
        for v in state.skillSetOrderList {
            st.skillSetOrderList.push(v);
        }
        // L1431
        Self::set_active_skill_set(cx, me, Some(state.activeSkillSetId));
        // L1432
        let g = displayId.and_then(|i| st.socketGroupList.get().get(i));
        Self::set_display_group(cx, me, g);
        // L1433
        let gl = Self::control(cx, me, "groupList");
        if cx.w::<ListControl>(gl).sel_value.is_some() {
            // L1434
            let idx = cx.w::<ListControl>(gl).sel_index;
            let v = idx
                .and_then(|i| st.socketGroupList.get().get(i))
                .map(ListValue::of);
            cx.w_mut::<ListControl>(gl).sel_value = v;
        }
        // Load active socket group for both skillsTab and calcsTab from UndoState
        // L1437
        m.mainSocketGroup.set(state.activeSocketGroup);
        // L1438
        m.calcsTab.input.set(n!("skill_number"), state.activeSocketGroup2);
    }

    /// lua: SkillsTab.lua:1444-1451 `SkillsTab:OpenSkillSetManagePopup()`
    /// @pob Classes/SkillsTab.lua:1443-1451 h=edd78c31
    pub fn open_skill_set_manage_popup(cx: &mut Cx, me: CtrlId) {
        // L1445
        popups::open_popup(
            cx,
            370.0,
            290.0,
            "Manage Skill Sets",
            move |cx, p| {
                let p = p.id();
                // L1446
                let f = skills_hooks().skill_set_list_ctor;
                f(cx, p, CtrlKey::Idx(1), rect(0, 50, 350, 200), me);
                // L1447-1449
                cx.add(
                    p,
                    CtrlKey::Idx(2),
                    ButtonControl::new(Anchor::NONE, rect(0, 260, 90, 20), "Done", |cx| {
                        // L1448
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts::default(),
        );
    }

    /// lua: SkillsTab.lua:1478-1502 `SkillsTab:SetActiveSkillSet(skillSetId)`: the model half
    /// (calc:BM-B) and the UI lines (L1495, the UI part of L1500's SetDisplayGroup, L1501).
    /// `None` is Lua's nil (`self.skillSetOrderList[index]` past the end).
    /// @pob Classes/SkillsTab.lua:1463-1463 h=da39a3ee
    /// @pob Classes/SkillsTab.lua:1476-1477 h=3759b500
    pub fn set_active_skill_set(cx: &mut Cx, me: CtrlId, skillSetId: Option<f64>) {
        let m = model(cx);
        // L1485-1487 `if not skillSetId then skillSetId = self.activeSkillSetId end`
        let id = skillSetId.unwrap_or_else(|| m.skillsTab.activeSkillSetId.get());
        // L1479-1500 (model half: calc:BM-B)
        m.skillsTab.SetActiveSkillSet(&m, id);
        Self::set_active_skill_set_ui_tail(cx, me, true);
    }

    /// The UI lines of SetActiveSkillSet after the model half: L1495, the controls of L1500's
    /// SetDisplayGroup, and L1501 when `sync` (DIVERGENCE(ctor-sync-loadouts)).
    fn set_active_skill_set_ui_tail(cx: &mut Cx, me: CtrlId, sync: bool) {
        // L1495
        Self::set_group_list_list(cx, me);
        // L1500 (the model half set displayGroup = socketGroupList[1] and processed it)
        if let Some(g) = display_group(cx) {
            Self::set_display_group_ui_tail(cx, me, &g);
        }
        // L1501
        if sync {
            let f = init::hooks(cx).sync_loadouts;
            f(cx);
        }
    }
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1, CCR-0106 §2) -----------------------------

/// `self.skillsTab` of the open build, when the tab exists.
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.skills.id();
    cx.ui.try_w::<SkillsTab>(id).map(|_| id)
}

#[track_caller]
fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'skillsTab' (a nil value)"))
}

/// `BuildHooks::skills_tab`: `self.skillsTab = new("SkillsTab"):SkillsTab(self)` (Build.lua:614),
/// then the UI statements of the Load that already ran (DIVERGENCE(load-ui-tail),
/// DIVERGENCE(default-gem-quality)).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = cx.add_free(Some(build), SkillsTab::new(build)).id();
    let model = cx.build().model.clone();
    if build_xml::xmlSectionList(&model)
        .iter()
        .any(|n| &*n.elem == "Skills")
    {
        SkillsTab::load_ui_tail(cx, me);
    } else {
        // L96 self.defaultGemQuality = main.defaultGemQuality
        let q = cx.app().main.default_gem_quality;
        model.skillsTab.defaultGemQuality.set(q);
    }
    me
}

/// `ShellHooks::draw_tab` for `BuildTab::Skills`: `self.skillsTab:Draw(tabViewPort, inputEvents)`
/// (Build.lua:1311).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    SkillsTab::draw(cx, me, vp, ev);
}

/// `ShellHooks::tab_flag` for `TabFlag::Skills`: `self.skillsTab.modFlag`.
pub fn mod_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| SkillsTab::mod_flag_of(cx, me))
}

/// `ShellHooks::set_tab_flag` for `TabFlag::Skills`: `self.skillsTab.modFlag = v`
/// (ResetModFlags, Build.lua:1108).
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        SkillsTab::set_mod_flag_of(cx, me, v);
    }
}

/// `BuildHooks::skills_set_active_skill_set`: `self.skillsTab:SetActiveSkillSet(skillSetId)`
/// (Build.lua:412). Before the tab exists (Init), the model half only.
pub fn set_active_skill_set_hook(cx: &mut Cx, id: f64) {
    match tab(cx) {
        Some(me) => SkillsTab::set_active_skill_set(cx, me, Some(id)),
        None => {
            let m = cx.build().model.clone();
            m.skillsTab.SetActiveSkillSet(&m, id);
        }
    }
}

/// `BuildHooks::skills_add_socket_group_tooltip`: `self.skillsTab:AddSocketGroupTooltip(tooltip,
/// socketGroup)` (Build.lua:540, CalcsTab.lua:55).
pub fn add_socket_group_tooltip_hook(cx: &mut Cx, tooltip: &mut Tooltip, socketGroup: &SocketGroupRef) {
    SkillsTab::add_socket_group_tooltip(cx, tooltip, socketGroup);
}
