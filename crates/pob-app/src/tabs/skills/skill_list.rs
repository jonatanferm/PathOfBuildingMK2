//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SkillListControl.lua 1-267
//! (`newClass("SkillListControl", "ListControl")`: the Skills tab's socket group list).
//! Owner: U31.
//!
//! @pob-file Classes/SkillListControl.lua @16de4b82d
//!
//! The list is the active skill set's `socketGroupList` (calc:BM-B's
//! `model.skillsTab.socketGroupList`), aliased through [`SocketGroupListSource`]: reorder,
//! delete and the New button write through. Rows are the socket groups ([`ListValue::of`] of the
//! `SocketGroupRef`, Lua table identity). `SkillsTab:SetActiveSkillSet` (U30, L1495) replaces the
//! list with [`SkillListControl::set_list`].
//!
//! The SkillsTab methods this control calls go through `gem_slots`' [`SkillsTabUiHooks`]
//! (`SetDisplayGroup`, `AddUndoState`, `CopySocketGroup`, `gemSlots[1]`), `BuildHooks`
//! (`AddSocketGroupTooltip`) and calc:BM-B (`RebuildImbuedSupportBySlot`); CCR-0119.
//! [`skill_list_ctor`] has the shape of U30's `SkillsHooks::skill_list_ctor` seam.
//!
//! ListControl hooks: `GetRowValue`, `GetRowIcon`, `AddValueTooltip`, `OnOrderChange`,
//! `OnSelect`, `OnSelCopy`, `OnSelDelete`; `OnHoverKeyUp` and `Draw` are `Widget` methods.
//!
//! Notes:
//! - `slot_map` (L9-27) is module state: one image handle per slot name, created when the file
//!   is loaded (the first constructor) and loaded again by every constructor (L73-75). ORDER:
//!   L73 `pairs(slot_map)` is string-keyed (per-process order in PoB); the loads are independent
//!   of each other (c), so the port loads in declaration order.
//! - A New group (L60-64) has `includeInFullDPS`, `slotEnabled`, `mainActiveSkill` and
//!   `mainActiveSkillCalcs` nil: the CCR-0019 markers (`$includeInFullDPSNil`, `false`, and NaN
//!   for the two numbers, as calc:CS3's groups, CCR-0083 §3).
//! - `wipeTable(self.list)` (L47) removes every row of the aliased list in place.
//! - `socketGroup.includeInFullDPS = not …` (L211) writes a boolean, so the nil marker goes.
//! - Not in PoB (CCR-0142 §3 B, FOLLOWUPS "CCRDOC" 7): a group the delete paths (L47, L175,
//!   L186) take out of the list is torn down ([`SocketGroup::teardown`]: its
//!   `displaySkillList(Calcs)` are cleared), because each display skill's `socketGroup` points
//!   back to it, a cycle Lua's GC collects. The lists are only read for groups in the list; the
//!   recalc that `buildFlag` asks for replaces the envs that still hold the old skills.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::n;
use pob_sg::{ImageFlags, ImageHandle};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListSource, ListValue};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtKey, TtObj};
use pob_ui_core::tooltip::Tooltip;

use super::gem_slots::{add_undo_state, gem_slot, set_display_group, skills_tab_ui_hooks};
use crate::app::CxApp;
use crate::build::init;
use crate::controls::gem_select::{wiki_matches_key, wiki_open, wiki_open_gem};
use crate::model::{self, Shared, SocketGroup, SocketGroupRef, Val};

// @pob-skip Classes/SkillListControl.lua:1-8 reason="header comment and the ipairs/table locals"
// @pob-skip Classes/SkillListControl.lua:28-31 reason="class annotation and newClass (the struct and Widget impl below)"

/// `slot_map` (L9-27): slot name → icon path (the handles are [`SLOT_ICONS`]).
const SLOT_MAP: [(&str, &str); 17] = [
    ("Weapon 1", "Assets/icon_weapon.png"),
    ("Weapon 2", "Assets/icon_weapon_2.png"),
    ("Weapon 1 Swap", "Assets/icon_weapon_swap.png"),
    ("Weapon 2 Swap", "Assets/icon_weapon_2_swap.png"),
    ("Bow", "Assets/icon_bow.png"),
    ("Quiver", "Assets/icon_quiver.png"),
    ("Shield", "Assets/icon_shield.png"),
    ("Shield Swap", "Assets/icon_shield_swap.png"),
    ("Helmet", "Assets/icon_helmet.png"),
    ("Body Armour", "Assets/icon_body_armour.png"),
    ("Gloves", "Assets/icon_gloves.png"),
    ("Boots", "Assets/icon_boots.png"),
    ("Amulet", "Assets/icon_amulet.png"),
    ("Ring 1", "Assets/icon_ring_left.png"),
    ("Ring 2", "Assets/icon_ring_right.png"),
    ("Ring 3", "Assets/icon_ring_right.png"),
    ("Belt", "Assets/icon_belt.png"),
];

thread_local! {
    /// `slot_map[k].icon` (`NewImageHandle()` at module load), indexed like [`SLOT_MAP`].
    static SLOT_ICONS: RefCell<Option<Vec<ImageHandle>>> = const { RefCell::new(None) };
}

/// lua: SkillListControl.lua:9-27 `local slot_map = { … }` (the module-level handles; module
/// notes)
/// @pob Classes/SkillListControl.lua:9-27 h=767bc3af
fn slot_icons(cx: &mut Cx) -> Vec<ImageHandle> {
    if let Some(v) = SLOT_ICONS.with(|s| s.borrow().clone()) {
        return v;
    }
    let v: Vec<ImageHandle> = SLOT_MAP.iter().map(|_| cx.sg.new_image_handle()).collect();
    SLOT_ICONS.with(|s| *s.borrow_mut() = Some(v.clone()));
    v
}

/// `slot_map[slot] and slot_map[slot].icon`
fn slot_icon(slot: &str) -> Option<ImageHandle> {
    let i = SLOT_MAP.iter().position(|(k, _)| *k == slot)?;
    SLOT_ICONS.with(|s| s.borrow().as_ref().map(|v| v[i].clone()))
}

/// `self.list = skillsTab.socketGroupList` (aliased; module notes).
pub struct SocketGroupListSource(pub Shared<Vec<SocketGroupRef>>);

impl ListSource for SocketGroupListSource {
    fn len(&self, _cx: &Cx) -> usize {
        self.0.len()
    }
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        self.0.get(i).map(ListValue::of)
    }
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let g = group_of(&v);
        self.0.with_mut(|l| {
            if i == 0 || i > l.len() + 1 {
                lua_error!("bad argument #2 to 'insert' (position out of bounds)");
            }
            l.insert(i - 1, g);
        });
    }
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        let g = self
            .0
            .remove(Some(i))
            .unwrap_or_else(|| lua_error!("t_remove: position out of range"));
        ListValue::of(g)
    }
}

/// The socket group of a row value.
#[track_caller]
pub fn group_of(v: &ListValue) -> SocketGroupRef {
    v.rc::<SocketGroup>()
        .unwrap_or_else(|| lua_error!("SkillListControl: a list value that is not a socket group"))
}

/// `self.skillsTab.build.calcsTab.input.skill_number` (a number; nil is the comparison error)
#[track_caller]
fn skill_number(cx: &Cx) -> f64 {
    let v = cx.build().model.calcsTab.input.get(n!("skill_number"));
    v.opt_num()
        .unwrap_or_else(|| lua_error!("attempt to compare {} with number", v.lua_type()))
}

fn set_skill_number(cx: &Cx, v: f64) {
    cx.build().model.calcsTab.input.set(n!("skill_number"), v);
}

/// `self.skillsTab.build.mainSocketGroup`
fn main_socket_group(cx: &Cx) -> f64 {
    cx.build().model.mainSocketGroup.get()
}

fn set_main_socket_group(cx: &Cx, v: f64) {
    cx.build().model.mainSocketGroup.set(v);
}

/// `skillsTab.build.buildFlag = true`
fn set_build_flag(cx: &Cx) {
    cx.build().model.buildFlag.set(true);
}

/// `skillsTab.displayGroup` is `group` (identity)
fn is_display_group(cx: &Cx, group: &SocketGroupRef) -> bool {
    cx.build()
        .model
        .skillsTab
        .displayGroup
        .get()
        .is_some_and(|g| Rc::ptr_eq(&g, group))
}

/// `skillsTab:RebuildImbuedSupportBySlot()` (calc:BM-B)
fn rebuild_imbued_support_by_slot(cx: &Cx) {
    let m = cx.build().model.clone();
    m.skillsTab.RebuildImbuedSupportBySlot(&m);
}

/// `SkillListControl`: the socket groups of the active skill set.
pub struct SkillListControl {
    pub list: ListControl,
    /// `self.skillsTab` (the SkillsTab node)
    pub skills_tab: CtrlId,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: SkillListControl::GetRowValue,
    get_row_icon: Some(SkillListControl::GetRowIcon),
    add_value_tooltip: Some(SkillListControl::AddValueTooltip),
    on_order_change: Some(SkillListControl::OnOrderChange),
    on_select: Some(SkillListControl::OnSelect),
    on_sel_copy: Some(SkillListControl::OnSelCopy),
    on_sel_delete: Some(SkillListControl::OnSelDelete),
    ..ListHooks::BASE
};

impl SkillListControl {
    /// lua: SkillListControl.lua:32-77 `SkillListClass:SkillListControl(anchor, rect, skillsTab)`
    /// (`socket_group_list` is `skillsTab.socketGroupList`)
    /// @pob Classes/SkillListControl.lua:32-77 h=3ebfa029
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        skills_tab: CtrlId,
        socket_group_list: Shared<Vec<SocketGroupRef>>,
    ) -> New<SkillListControl> {
        // L36
        let l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            true,
            ListData::Source(Rc::new(SocketGroupListSource(socket_group_list))),
            false,
        );
        New::new(
            Base::new(anchor, r),
            SkillListControl {
                list: l,
                // L37
                skills_tab,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(move |cx, me| {
            let me = me.0;
            // L9-27 (module load: the first constructor creates the handles)
            let icons = slot_icons(cx);
            // L38
            cx.w_mut::<ListControl>(me).label = Some(Prop::Val("^7Socket Groups:".to_owned()));
            // L39-41
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, TopRight),
                    rect(0, -2, 60, 18),
                    "Delete",
                    move |cx| {
                        // L40
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'socketGroup' (a nil value)")
                        });
                        Self::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L42-44
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L43
                cx.w::<ListControl>(me)
                    .sel_value
                    .as_ref()
                    .is_some_and(|v| group_of(v).source.get().is_none())
            });
            // L45-55
            let delete_all = cx.add(
                me,
                "deleteAll",
                ButtonControl::new(
                    Anchor::new(Right, delete, Left),
                    rect(-4, 0, 70, 18),
                    "Delete All",
                    move |cx| {
                        // L46-54
                        popups::open_confirm_popup(
                            cx,
                            "Delete All",
                            "Are you sure you want to delete all socket groups in this build?",
                            "Delete",
                            move |cx| {
                                // L47 (module notes)
                                while ListControl::list_len(cx, me) > 0 {
                                    let n = ListControl::list_len(cx, me);
                                    if let Some(g) = ListControl::list_remove(cx, me, n) {
                                        // CCR-0142 §3 B (module notes)
                                        group_of(&g).teardown();
                                    }
                                }
                                // L48
                                rebuild_imbued_support_by_slot(cx);
                                // L49
                                set_display_group(cx, skills_tab, None);
                                // L50
                                add_undo_state(cx, skills_tab);
                                // L51
                                set_build_flag(cx);
                                let w = cx.w_mut::<ListControl>(me);
                                // L52
                                w.sel_index = None;
                                // L53
                                w.sel_value = None;
                            },
                            None,
                        );
                    },
                ),
            );
            // L56-58
            cx.base_mut(delete_all).enabled = Prop::f(move |cx, _| {
                // L57
                ListControl::list_len(cx, me) > 0
            });
            // L59-72
            cx.add(
                me,
                "new",
                ButtonControl::new_ret(
                    Anchor::new(Right, delete_all, Left),
                    rect(-4, 0, 60, 18),
                    "New",
                    move |cx| {
                        // L60-64 (the nil fields: module notes)
                        let g = SocketGroup::default();
                        model::skills_tab::set_socketGroup_label(&g, Some(""));
                        g.enabled.set(true);
                        g.extra.set(n!("$includeInFullDPSNil"), true);
                        g.mainActiveSkill.set(f64::NAN);
                        g.mainActiveSkillCalcs.set(f64::NAN);
                        let newGroup: SocketGroupRef = Rc::new(g);
                        // L65
                        let n = ListControl::list_len(cx, me);
                        ListControl::list_insert(cx, me, n + 1, ListValue::of(newGroup.clone()));
                        // L66
                        let n = ListControl::list_len(cx, me);
                        cx.w_mut::<ListControl>(me).sel_index = Some(n);
                        // L67
                        cx.w_mut::<ListControl>(me).sel_value = Some(ListValue::of(newGroup.clone()));
                        // L68
                        set_display_group(cx, skills_tab, Some(newGroup));
                        // L69
                        add_undo_state(cx, skills_tab);
                        // L70
                        set_build_flag(cx);
                        // L71
                        Some(gem_slot(cx, skills_tab, 1).name_spec)
                    },
                ),
            );
            // L73 (ORDER: module notes)
            for (h, (_, path)) in icons.iter().zip(SLOT_MAP.iter()) {
                // L74
                h.load(&mut cx.sg, path, ImageFlags::empty());
            }
            // L76 return self
        })
    }

    /// `self.controls.groupList.list = list` (SkillsTab.lua:1495, U30's SetActiveSkillSet).
    pub fn set_list(cx: &mut Cx, me: CtrlId, list: Shared<Vec<SocketGroupRef>>) {
        cx.w_mut::<ListControl>(me).list = ListData::Source(Rc::new(SocketGroupListSource(list)));
    }

    /// lua: SkillListControl.lua:79-118 `SkillListClass:GetRowValue(column, index, socketGroup)`
    /// @pob Classes/SkillListControl.lua:79-118 h=e430d11c
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, index: usize, v: &ListValue) -> String {
        let socketGroup = group_of(v);
        // L80
        if column == 1 {
            let cc = &cx.main.cc;
            // L81
            let mut label = socketGroup
                .displayLabel
                .get()
                .map_or_else(|| "?".to_owned(), |s| s.to_string());
            // L82
            let currentMainSkill = main_socket_group(cx) == index as f64;
            // L83
            let disabled = !socketGroup.enabled.get() || !socketGroup.slotEnabled.get();
            // L84
            if disabled {
                // L85
                let colour = if currentMainSkill { "" } else { "^x7F7F7F" };
                // L86
                label = format!("{colour}{label} (Disabled)");
            }
            // L88
            if currentMainSkill {
                // L89
                let activeLabel = if disabled { " (Forced Active)" } else { " (Active)" };
                // L90
                label = format!("{label}{}{activeLabel}", cc.RELIC);
            }
            // L92
            if socketGroup.includeInFullDPS.get() {
                // L93
                label = format!("{label}{} (FullDPS)", cc.CUSTOM);
            }

            // L96
            if socketGroup.source.get().is_none() {
                // L97
                let mut colorStr = String::new();
                // L98
                for gem in socketGroup.gemList.get().snapshot() {
                    // L99
                    let gemData = gem.gemData.get();
                    if (gemData.is_some() || gem.grantedEffect.get().is_some()) && gem.enabled.get() {
                        // L100
                        let grantedEffect = gem.grantedEffect.get().or_else(|| gemData.map(|d| d.grantedEffect.clone()));
                        // L101
                        if let Some(grantedEffect) = grantedEffect {
                            // L102
                            let char = match grantedEffect.color {
                                Some(1) => "R",
                                Some(2) => "G",
                                Some(3) => "B",
                                _ => "W",
                            };
                            // L103
                            let colorCode = match gem.extra.get(n!("color")) {
                                Val::Nil | Val::Bool(false) => String::new(),
                                c => c.str().map_or_else(
                                    || lua_error!("attempt to concatenate a non-string value"),
                                    |s| s.to_string(),
                                ),
                            };
                            // L104
                            if !colorStr.is_empty() {
                                // L105
                                colorStr.push_str("^7-");
                            }
                            // L107
                            colorStr = format!("{colorStr}{colorCode}{char}");
                        }
                    }
                }
                // L111
                if !colorStr.is_empty() {
                    // L112
                    label = format!("{label} ^7{colorStr}");
                }
            }

            // L116
            return label;
        }
        String::new()
    }

    /// lua: SkillListControl.lua:120-128 `SkillListClass:AddValueTooltip(tooltip, index,
    /// socketGroup)`
    /// @pob Classes/SkillListControl.lua:120-128 h=79559562
    fn AddValueTooltip(cx: &mut Cx, _me: CtrlId, tooltip: &mut Tooltip, _index: usize, v: &ListValue) {
        let socketGroup = group_of(v);
        // L121
        if socketGroup.displaySkillList.get().is_none() {
            // L122
            tooltip.clear(false);
            // L123
            return;
        }
        // L125
        let rev = cx.build().model.outputRevision.get();
        let key = TtKey::Obj(TtObj(socketGroup.clone() as Rc<dyn std::any::Any>));
        if tooltip.check_for_update(cx, &[key, TtKey::Num(rev)]) {
            // L126
            let f = init::hooks(cx).skills_add_socket_group_tooltip;
            f(cx, tooltip, &socketGroup);
        }
    }

    /// lua: SkillListControl.lua:130-149 `SkillListClass:OnOrderChange(selIndex, selDragIndex)`
    /// @pob Classes/SkillListControl.lua:130-149 h=a3ca2b06
    fn OnOrderChange(cx: &mut Cx, me: CtrlId, selIndex: usize, selDragIndex: usize) {
        let skills_tab = cx.w::<SkillListControl>(me).skills_tab;
        let (selIndex, selDragIndex) = (selIndex as f64, selDragIndex as f64);
        // L131
        let skillsTabIndex = main_socket_group(cx);
        // L132
        if skillsTabIndex == selIndex {
            // L133
            set_main_socket_group(cx, selDragIndex);
        // L134
        } else if skillsTabIndex > selIndex && skillsTabIndex <= selDragIndex {
            // L135
            set_main_socket_group(cx, skillsTabIndex - 1.0);
        // L136
        } else if skillsTabIndex < selIndex && skillsTabIndex >= selDragIndex {
            // L137
            set_main_socket_group(cx, skillsTabIndex + 1.0);
        }
        // L139
        let calcsTabIndex = skill_number(cx);
        // L140
        if calcsTabIndex == selIndex {
            // L141
            set_skill_number(cx, selDragIndex);
        // L142
        } else if calcsTabIndex > selIndex && calcsTabIndex <= selDragIndex {
            // L143
            set_skill_number(cx, calcsTabIndex - 1.0);
        // L144
        } else if calcsTabIndex < selIndex && calcsTabIndex >= selDragIndex {
            // L145
            set_skill_number(cx, calcsTabIndex + 1.0);
        }
        // L147
        add_undo_state(cx, skills_tab);
        // L148
        set_build_flag(cx);
    }

    /// lua: SkillListControl.lua:151-153 `SkillListClass:OnSelect(index, socketGroup)`
    /// @pob Classes/SkillListControl.lua:151-153 h=978ba6ac
    fn OnSelect(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
        let skills_tab = cx.w::<SkillListControl>(me).skills_tab;
        // L152
        set_display_group(cx, skills_tab, Some(group_of(v)));
    }

    /// lua: SkillListControl.lua:155-159 `SkillListClass:OnSelCopy(index, socketGroup)`
    /// @pob Classes/SkillListControl.lua:155-159 h=24fb94ad
    fn OnSelCopy(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
        let skills_tab = cx.w::<SkillListControl>(me).skills_tab;
        let socketGroup = group_of(v);
        // L156
        if socketGroup.source.get().is_none() {
            // L157
            let f = skills_tab_ui_hooks().copy_socket_group;
            f(cx, skills_tab, &socketGroup);
        }
    }

    /// lua: SkillListControl.lua:161-197 `SkillListClass:OnSelDelete(index, socketGroup)`
    /// @pob Classes/SkillListControl.lua:161-197 h=2eddff74
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let skills_tab = cx.w::<SkillListControl>(me).skills_tab;
        let socketGroup = group_of(v);
        // L162-171
        let updateActiveSocketGroupIndex = move |cx: &mut Cx| {
            // L163
            let selIndex = cx
                .w::<ListControl>(me)
                .sel_index
                .map_or_else(|| lua_error!("attempt to compare nil with number"), |i| i as f64);
            let skillsTabIndex = main_socket_group(cx);
            // L164
            if skillsTabIndex > selIndex {
                // L165
                set_main_socket_group(cx, skillsTabIndex - 1.0);
            }
            // L167
            let calcsTabIndex = skill_number(cx);
            // L168
            if calcsTabIndex > selIndex {
                // L169
                set_skill_number(cx, calcsTabIndex - 1.0);
            }
        };
        // L172
        if socketGroup.source.get().is_some() {
            // L173
            popups::open_message_popup(
                cx,
                "Delete Socket Group",
                "This socket group cannot be deleted as it is created by an equipped item.",
            );
        // L174
        } else if socketGroup.gemList.get().get(1).is_none() {
            // L175
            ListControl::list_remove(cx, me, index);
            // CCR-0142 §3 B (module notes)
            socketGroup.teardown();
            // L176
            rebuild_imbued_support_by_slot(cx);
            // L177
            if is_display_group(cx, &socketGroup) {
                // L178
                cx.build().model.skillsTab.displayGroup.set(None);
            }
            // L180
            updateActiveSocketGroupIndex(cx);
            // L181
            add_undo_state(cx, skills_tab);
            // L182
            set_build_flag(cx);
            // L183
            cx.w_mut::<ListControl>(me).sel_value = None;
        } else {
            // L185
            let label = socketGroup
                .displayLabel
                .get()
                .unwrap_or_else(|| lua_error!("attempt to concatenate field 'displayLabel' (a nil value)"));
            let msg = format!("Are you sure you want to delete '{label}'?");
            let group = socketGroup.clone();
            popups::open_confirm_popup(
                cx,
                "Delete Socket Group",
                &msg,
                "Delete",
                move |cx| {
                    // L186
                    ListControl::list_remove(cx, me, index);
                    // CCR-0142 §3 B (module notes)
                    group.teardown();
                    // L187
                    rebuild_imbued_support_by_slot(cx);
                    // L188
                    if is_display_group(cx, &group) {
                        // L189
                        set_display_group(cx, skills_tab, None);
                    }
                    // L191
                    updateActiveSocketGroupIndex(cx);
                    // L192
                    add_undo_state(cx, skills_tab);
                    // L193
                    set_build_flag(cx);
                    // L194
                    cx.w_mut::<ListControl>(me).sel_value = None;
                },
                None,
            );
        }
    }

    /// lua: SkillListControl.lua:241-267 `SkillListClass:GetRowIcon(column, index, socketGroup)`
    /// @pob Classes/SkillListControl.lua:241-267 h=0cf47b25
    fn GetRowIcon(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> Option<ImageHandle> {
        let socketGroup = group_of(v);
        // L242
        if column == 1 {
            // L243
            let mut slot = socketGroup.slot.get().map(|s| s.to_string());
            // L244
            let m = &cx.build().model;
            let set = m
                .itemsTab
                .activeItemSet
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"));
            let sel = |name: &str| -> Val {
                let v = set
                    .table(name)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                    .get(n!("selItemId"));
                // `… .selItemId or 0`
                if v.truthy() {
                    v
                } else {
                    Val::Num(0.0)
                }
            };
            let base_type = |id: &Val| -> String {
                // `itemsTab.items[sel] and itemsTab.items[sel].base.type or "None"`
                match crate::tabs::items::slot::item_by_id(cx, id) {
                    Some(item) => item
                        .base
                        .get()
                        .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
                        .r#type
                        .to_string(),
                    None => "None".to_owned(),
                }
            };
            // L245
            let weapon1Sel = sel("Weapon 1");
            // L246
            let weapon1Type = base_type(&weapon1Sel);
            // L247
            let weapon1SwapSel = sel("Weapon 1 Swap");
            // L248
            let weapon1SwapType = base_type(&weapon1SwapSel);
            // L249
            let weapon2Sel = sel("Weapon 2");
            // L250
            let weapon2Type = base_type(&weapon2Sel);
            // L251
            let weapon2SwapSel = sel("Weapon 2 Swap");
            // L252
            let weapon2SwapType = base_type(&weapon2SwapSel);
            // L253
            if slot.as_deref() == Some("Weapon 1") && weapon1Type == "Bow" {
                // L254
                slot = Some(weapon1Type);
            }
            // L256
            if slot.as_deref() == Some("Weapon 1 Swap") && weapon1SwapType == "Bow" {
                // L257
                slot = Some(format!("{weapon1SwapType} Swap"));
            }
            // L259
            if slot.as_deref() == Some("Weapon 2") && (weapon2Type == "Quiver" || weapon2Type == "Shield") {
                // L260
                slot = Some(weapon2Type);
            }
            // L262
            if slot.as_deref() == Some("Weapon 2 Swap") && (weapon2SwapType == "Quiver" || weapon2SwapType == "Shield") {
                // L263
                slot = Some(format!("{weapon2SwapType} Swap"));
            }
            // L265
            return slot.as_deref().and_then(slot_icon);
        }
        None
    }
}

impl Widget for SkillListControl {
    const NAME: &'static str = "SkillListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS.union(Caps::HOVER_KEY_UP);

    /// lua: SkillListControl.lua:237-239 `SkillListClass:Draw(viewPort)`
    /// @pob Classes/SkillListControl.lua:237-239 h=4864b1e9
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L238 (`self.ListControl.Draw(self, viewPort)`: the noTooltip argument is dropped)
        let _ = no_tooltip;
        ListControl::draw(cx, me, vp, false);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }

    /// lua: SkillListControl.lua:199-234 `SkillListClass:OnHoverKeyUp(key)`
    /// @pob Classes/SkillListControl.lua:199-234 h=80424ded
    fn on_hover_key_up(cx: &mut Cx, me: CtrlId, key: Key) {
        let skills_tab = cx.w::<SkillListControl>(me).skills_tab;
        // L200
        let item = ListControl::get_hover_value(cx, me);
        // L201
        if let Some(item) = item {
            let item = group_of(&item);
            // L202
            if wiki_matches_key(key) {
                // L203 Get the first gem in the group
                // L204
                let gem = item.gemList.get().get(1);
                // L205
                if let Some(gem) = gem {
                    // L206 either the skill is from a gem or granted from an item/passive
                    // L207
                    if let Some(gem_data) = gem.gemData.get() {
                        wiki_open_gem(cx, &gem_data);
                    } else {
                        let ge = gem
                            .grantedEffect
                            .get()
                            .unwrap_or_else(|| lua_error!("attempt to index field 'grantedEffect' (a nil value)"));
                        // `openGem(name)` of a string: `itemLib.wiki.open(name)` (ItemTools.lua:395)
                        wiki_open(cx, ge.name.as_str());
                    }
                }
            // L209
            } else if key == Key::RightButton {
                // L210
                if cx.sys.is_key_down(Key::Ctrl) {
                    // L211 (module notes)
                    item.includeInFullDPS.set(!item.includeInFullDPS.get());
                    item.extra.set(n!("$includeInFullDPSNil"), Val::Nil);
                    // L212
                    if is_display_group(cx, &item) {
                        // L213
                        set_display_group(cx, skills_tab, Some(item.clone()));
                    }
                    // L215
                    add_undo_state(cx, skills_tab);
                    // L216
                    set_build_flag(cx);
                } else {
                    // L218
                    let index = ListControl::get_hover_index(cx, me);
                    // L219
                    if let Some(index) = index {
                        // L220
                        set_main_socket_group(cx, index as f64);
                        // L221
                        add_undo_state(cx, skills_tab);
                        // L222
                        set_build_flag(cx);
                    }
                }
            // L225
            } else if key == Key::LeftButton && cx.sys.is_key_down(Key::Ctrl) {
                // L226
                item.enabled.set(!item.enabled.get());
                // L227
                if is_display_group(cx, &item) {
                    // L228
                    set_display_group(cx, skills_tab, Some(item.clone()));
                }
                // L230
                add_undo_state(cx, skills_tab);
                // L231
                set_build_flag(cx);
            }
        }
    }

    pob_ui_core::impl_parts!(SkillListControl: list => ListControl);
}

/// `self.controls.groupList = new("SkillListControl"):SkillListControl(anchor, rect, self)`
/// (SkillsTab.lua:113) in the SkillsTab node `tab`: the shape of U30's
/// `SkillsHooks::skill_list_ctor` seam. Returns the control.
pub fn skill_list_ctor(cx: &mut Cx, tab: CtrlId, anchor: Anchor, r: [Prop<f64>; 4]) -> CtrlId {
    let list = cx.build().model.skillsTab.socketGroupList.get();
    cx.add(tab, "groupList", SkillListControl::new(anchor, r, tab, list)).id()
}
