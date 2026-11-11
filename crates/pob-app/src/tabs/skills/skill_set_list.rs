//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SkillSetListControl.lua 1-127
//! (`newClass("SkillSetListControl", "ListControl")`: the skill sets of the "Manage Skill Sets"
//! popup). Owner: U31.
//!
//! @pob-file Classes/SkillSetListControl.lua @16de4b82d
//!
//! The list is `skillsTab.skillSetOrderList` itself (L18; calc:BM-B's
//! `model.skillsTab.skillSetOrderList`, which is never reassigned, only changed in place),
//! aliased through U32's [`IdListSource`]; rows are skill set ids. Skill sets are calc:BM-B's
//! tables (`model.skillsTab.skillSets[id]`: `id`, `title`, `socketGroupList`).
//!
//! The SkillsTab calls go through the existing seams (CCR-0119): `SetActiveSkillSet` is
//! `BuildHooks::skills_set_active_skill_set`, `AddUndoState` is `gem_slots`'
//! [`SkillsTabUiHooks`], `skillsTab.modFlag = true` is `ShellHooks::set_tab_flag(TabFlag::Skills,
//! true)`, `build:SyncLoadouts()` is `BuildHooks::sync_loadouts`; `NewSkillSet` is calc:BM-B's.
//! [`skill_set_list_ctor`] has the shape of U30's `SkillsHooks::skill_set_list_ctor` seam
//! (SkillsTab.lua:1444).
//!
//! ListControl hooks: `GetRowValue`, `OnOrderChange`, `OnSelClick`, `OnSelDelete`,
//! `OnSelKeyDown`.
//!
//! Notes:
//! - Copy (L21-37): `copyTable(skillSet, true)` and the per-group / per-gem `copyTable(x, true)`
//!   are shallow copies of the model structs ([`copy_socket_group`], [`copy_gem_instance`]: every
//!   field, the `extra` keys included). ORDER: L24/L27 `pairs` walk sequences (array part).
//! - `controls.save.enabled = buf:match("%S")` (L64) stores the match string or nil: only its
//!   truthiness is read (`IsEnabled`).
//! - L109 "Delete Item Set" (sic) is PoB's title.
//! - `self.list[m_max(1, index - 1)]` (L115) is nil only for an empty list, which `#self.list >
//!   1` (L108) rules out; a nil id would make SetActiveSkillSet fall back to the first set.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::{n, pattern};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use super::gem_slots::add_undo_state;
use crate::app::CxApp;
use crate::build::init;
use crate::build::on_frame::{shell_hooks, TabFlag};
use crate::model::{self, GemInstance, GemInstanceRef, Shared, SocketGroup, SocketGroupRef, Table, Val};
use crate::tabs::items::item_list::{id_value, value_id, IdListSource};

// @pob-skip Classes/SkillSetListControl.lua:1-16 reason="header comment, the table/math/string locals, newClass and the constructor's annotations (the struct and Widget impl below)"

/// `skillsTab.skillSets[id]`
fn skill_set(cx: &Cx, id: f64) -> Option<Table> {
    let k = model::items_tab::id_key(id)?;
    cx.build().model.skillsTab.skillSets.with(|s| s.get(&k).cloned())
}

/// `skillsTab.skillSets[id]` that the Lua then indexes.
#[track_caller]
fn skill_set_or_raise(cx: &Cx, id: f64) -> Table {
    skill_set(cx, id).unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"))
}

/// `skillSet.title or "Default"`
fn set_title(set: &Table) -> String {
    set.str(n!("title")).map_or_else(|| "Default".to_owned(), |s| s.to_string())
}

/// `self.skillsTab.modFlag = true`
fn set_mod_flag(cx: &mut Cx) {
    let f = shell_hooks().set_tab_flag;
    f(cx, TabFlag::Skills, true);
}

/// `self.skillsTab.build:SyncLoadouts()`
fn sync_loadouts(cx: &mut Cx) {
    let f = init::hooks(cx).sync_loadouts;
    f(cx);
}

/// `self.skillsTab:SetActiveSkillSet(skillSetId)`
fn set_active_skill_set(cx: &mut Cx, id: f64) {
    let f = init::hooks(cx).skills_set_active_skill_set;
    f(cx, id);
}

/// `copyTable(socketGroup, true)`: a shallow copy (every field, the `extra` keys included).
pub fn copy_socket_group(s: &SocketGroup) -> SocketGroup {
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

/// `copyTable(gem, true)`: a shallow copy.
pub fn copy_gem_instance(s: &GemInstance) -> GemInstance {
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

/// `SkillSetListControl`: the build's skill sets.
pub struct SkillSetListControl {
    pub list: ListControl,
    /// `self.skillsTab` (the SkillsTab node)
    pub skills_tab: CtrlId,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: SkillSetListControl::GetRowValue,
    on_order_change: Some(SkillSetListControl::OnOrderChange),
    on_sel_click: Some(SkillSetListControl::OnSelClick),
    on_sel_delete: Some(SkillSetListControl::OnSelDelete),
    on_sel_key_down: Some(SkillSetListControl::OnSelKeyDown),
    ..ListHooks::BASE
};

impl SkillSetListControl {
    /// lua: SkillSetListControl.lua:17-58 `SkillSetListClass:SkillSetListControl(anchor, rect,
    /// skillsTab)` (`order_list` is `skillsTab.skillSetOrderList`)
    /// @pob Classes/SkillSetListControl.lua:17-58 h=287f33dd
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        skills_tab: CtrlId,
        order_list: Shared<Vec<u32>>,
    ) -> New<SkillSetListControl> {
        // L18
        let l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            true,
            ListData::Source(Rc::new(IdListSource(order_list))),
            false,
        );
        New::new(
            Base::new(anchor, r),
            SkillSetListControl {
                list: l,
                // L19
                skills_tab,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(|cx, me| {
            let me = me.0;
            let sel_value = move |cx: &Cx| cx.w::<ListControl>(me).sel_value.as_ref().map(value_id);
            // L20-38
            let copy = cx.add(
                me,
                "copy",
                ButtonControl::new(Anchor::new(BottomLeft, me, Top), rect(2, -4, 60, 18), "Copy", move |cx| {
                    let m = cx.build().model.clone();
                    // L21
                    let skillSet = sel_value(cx)
                        .and_then(|id| skill_set(cx, f64::from(id)))
                        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
                    // L22
                    let newSkillSet = skillSet.copyTable(true);
                    // L23
                    let newList: Shared<Vec<SocketGroupRef>> = Shared::new(Vec::new());
                    newSkillSet.set(n!("socketGroupList"), Val::Obj(Rc::new(newList.clone())));
                    // L24 (ORDER: module notes)
                    for socketGroup in model::skills_tab::skillSet_socketGroupList(&skillSet).snapshot() {
                        // L25
                        let newGroup = copy_socket_group(&socketGroup);
                        // L26
                        let gemList: Shared<Vec<GemInstanceRef>> = Shared::new(Vec::new());
                        newGroup.gemList.set(gemList.clone());
                        // L27
                        for gem in socketGroup.gemList.get().snapshot() {
                            // L28
                            gemList.push(Rc::new(copy_gem_instance(&gem)));
                        }
                        // L30
                        newList.push(Rc::new(newGroup));
                    }
                    // L32
                    let mut id = 1.0;
                    // L33
                    while skill_set(cx, id).is_some() {
                        // L34
                        id += 1.0;
                    }
                    newSkillSet.set(n!("id"), id);
                    // L36
                    let k = model::items_tab::id_key(id).expect("an integer id");
                    m.skillsTab.skillSets.with_mut(|s| s.insert(k, newSkillSet.clone()));
                    // L37
                    Self::RenameSet(cx, me, &newSkillSet, true);
                }),
            );
            // L39-41
            cx.base_mut(copy).enabled = Prop::f(move |cx, _| {
                // L40
                sel_value(cx).is_some()
            });
            // L42-44
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(Anchor::new(Left, copy, Right), rect(4, 0, 60, 18), "Delete", move |cx| {
                    // L43
                    let (index, value) = {
                        let w = cx.w::<ListControl>(me);
                        (w.sel_index, w.sel_value.clone())
                    };
                    let value = value.unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"));
                    Self::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                }),
            );
            // L45-47
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L46
                sel_value(cx).is_some() && ListControl::list_len(cx, me) > 1
            });
            // L48-50
            let rename = cx.add(
                me,
                "rename",
                ButtonControl::new(Anchor::new(BottomRight, me, Top), rect(-2, -4, 60, 18), "Rename", move |cx| {
                    // L49
                    let set = sel_value(cx)
                        .and_then(|id| skill_set(cx, f64::from(id)))
                        .unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"));
                    Self::RenameSet(cx, me, &set, false);
                }),
            );
            // L51-53
            cx.base_mut(rename).enabled = Prop::f(move |cx, _| {
                // L52
                sel_value(cx).is_some()
            });
            // L54-56
            cx.add(
                me,
                "new",
                ButtonControl::new(Anchor::new(Right, rename, Left), rect(-4, 0, 60, 18), "New", move |cx| {
                    // L55
                    let set = cx.build().model.skillsTab.NewSkillSet(None);
                    Self::RenameSet(cx, me, &set, true);
                }),
            );
            // L57 return self
        })
    }

    /// lua: SkillSetListControl.lua:60-86 `SkillSetListClass:RenameSet(skillSet, addOnName)`
    /// @pob Classes/SkillSetListControl.lua:60-86 h=548ef5d9
    pub fn RenameSet(cx: &mut Cx, me: CtrlId, skillSet: &Table, addOnName: bool) {
        let title = skillSet.str(n!("title")).map(|s| s.to_string());
        // L85 (the title argument; evaluated after the constructors, which draw nothing)
        let popup_title = if title.is_some() { "Rename" } else { "Set Name" };
        let set = skillSet.clone();
        popups::open_popup(
            cx,
            370.0,
            100.0,
            popup_title,
            move |cx, p| {
                let p = p.id();
                // L61 local controls = { }
                // L62
                cx.add(
                    p,
                    "label",
                    LabelControl::new(Anchor::NONE, rect(0, 20, 0, 16), "^7Enter name for this skill set:"),
                );
                // L63-65
                cx.add(
                    p,
                    "edit",
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 40, 350, 20),
                        EditArgs {
                            init: title,
                            limit: Some(100),
                            change_func: Some(Rc::new(move |cx, buf| {
                                // L64 (module notes)
                                let save = cx.child_as::<ButtonControl>(p, "save");
                                let ok = pattern::find(buf, "%S", None, false).is_some();
                                cx.base_mut(save).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L66-77
                let set_s = set.clone();
                let save = cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                        let skills_tab = cx.w::<SkillSetListControl>(me).skills_tab;
                        let edit = cx.child_as::<EditControl>(p, "edit");
                        // L67
                        let buf = cx.get(edit).buf.clone();
                        set_s.set(n!("title"), buf.as_str());
                        // L68
                        set_mod_flag(cx);
                        // L69
                        if addOnName {
                            // L70
                            let id = set_s.num(n!("id"));
                            let n = ListControl::list_len(cx, me);
                            ListControl::list_insert(cx, me, n + 1, id_value(id as u32));
                            // L71
                            let n = ListControl::list_len(cx, me);
                            let w = cx.w_mut::<ListControl>(me);
                            w.sel_index = Some(n);
                            // L72
                            w.sel_value = Some(id_value(id as u32));
                        }
                        // L74
                        add_undo_state(cx, skills_tab);
                        // L75
                        sync_loadouts(cx);
                        // L76
                        popups::close_popup(cx);
                    }),
                );
                // L78
                cx.base_mut(save).enabled = Prop::Val(false);
                // L79-84
                let set_c = set.clone();
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", move |cx| {
                        // L80
                        if addOnName {
                            // L81
                            let id = set_c.num(n!("id"));
                            if let Some(k) = model::items_tab::id_key(id) {
                                cx.build().model.skillsTab.skillSets.with_mut(|s| s.shift_remove(&k));
                            }
                        }
                        // L83
                        popups::close_popup(cx);
                    }),
                );
            },
            // L85
            PopupOpts {
                enter: Some("save".into()),
                default: Some("edit".into()),
                escape: Some("cancel".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: SkillSetListControl.lua:88-93 `SkillSetListClass:GetRowValue(column, index,
    /// skillSetId)`
    /// @pob Classes/SkillSetListControl.lua:88-93 h=bfd181b3
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        let id = f64::from(value_id(v));
        // L89
        let skillSet = skill_set(cx, id);
        // L90
        if column == 1 {
            let skillSet = skillSet.unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"));
            // L91
            let current = if id == cx.build().model.skillsTab.activeSkillSetId.get() {
                "  ^9(Current)"
            } else {
                ""
            };
            return format!("{}{current}", set_title(&skillSet));
        }
        String::new()
    }

    /// lua: SkillSetListControl.lua:95-97 `SkillSetListClass:OnOrderChange()`
    /// @pob Classes/SkillSetListControl.lua:95-97 h=4a38c867
    fn OnOrderChange(cx: &mut Cx, _me: CtrlId, _sel_index: usize, _sel_drag_index: usize) {
        // L96
        set_mod_flag(cx);
    }

    /// lua: SkillSetListControl.lua:99-104 `SkillSetListClass:OnSelClick(index, skillSetId,
    /// doubleClick)`
    /// @pob Classes/SkillSetListControl.lua:99-104 h=3b616389
    fn OnSelClick(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, doubleClick: bool) -> Option<bool> {
        let skills_tab = cx.w::<SkillSetListControl>(me).skills_tab;
        let id = f64::from(value_id(v));
        // L100
        if doubleClick && id != cx.build().model.skillsTab.activeSkillSetId.get() {
            // L101
            set_active_skill_set(cx, id);
            // L102
            add_undo_state(cx, skills_tab);
        }
        None
    }

    /// lua: SkillSetListControl.lua:106-121 `SkillSetListClass:OnSelDelete(index, skillSetId)`
    /// @pob Classes/SkillSetListControl.lua:106-121 h=8a43d39c
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let skills_tab = cx.w::<SkillSetListControl>(me).skills_tab;
        let id = value_id(v);
        // L107
        let skillSet = skill_set(cx, f64::from(id));
        // L108
        if ListControl::list_len(cx, me) > 1 {
            let skillSet = skillSet.unwrap_or_else(|| lua_error!("attempt to index local 'skillSet' (a nil value)"));
            // L109-119
            popups::open_confirm_popup(
                cx,
                "Delete Item Set",
                &format!("Are you sure you want to delete '{}'?", set_title(&skillSet)),
                "Delete",
                move |cx| {
                    // L110
                    ListControl::list_remove(cx, me, index);
                    // L111
                    cx.build().model.skillsTab.skillSets.with_mut(|s| s.shift_remove(&id));
                    {
                        let w = cx.w_mut::<ListControl>(me);
                        // L112
                        w.sel_index = None;
                        // L113
                        w.sel_value = None;
                    }
                    // L114
                    if f64::from(id) == cx.build().model.skillsTab.activeSkillSetId.get() {
                        // L115 (module notes)
                        let i = m_max(1.0, index as f64 - 1.0) as usize;
                        let next = ListControl::list_get(cx, me, i).map_or(f64::NAN, |v| f64::from(value_id(&v)));
                        set_active_skill_set(cx, next);
                    }
                    // L117
                    add_undo_state(cx, skills_tab);
                    // L118
                    sync_loadouts(cx);
                },
                None,
            );
        }
    }

    /// lua: SkillSetListControl.lua:123-127 `SkillSetListClass:OnSelKeyDown(index, skillSetId,
    /// key)`
    /// @pob Classes/SkillSetListControl.lua:123-127 h=5cc6268b
    fn OnSelKeyDown(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, key: Key) {
        // L124
        if key == Key::F(2) {
            // L125
            let set = skill_set_or_raise(cx, f64::from(value_id(v)));
            Self::RenameSet(cx, me, &set, false);
        }
    }
}

impl Widget for SkillSetListControl {
    const NAME: &'static str = "SkillSetListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip);
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

    pob_ui_core::impl_parts!(SkillSetListControl: list => ListControl);
}

/// `new("SkillSetListControl"):SkillSetListControl(nil, rect, self)` stored in `popup` under
/// `key` (SkillsTab.lua:1444, the skill set manager's first control): the shape of U30's
/// `SkillsHooks::skill_set_list_ctor` seam. Returns the control.
pub fn skill_set_list_ctor(cx: &mut Cx, popup: CtrlId, key: CtrlKey, r: [Prop<f64>; 4], tab: CtrlId) -> CtrlId {
    let order = cx.build().model.skillsTab.skillSetOrderList.clone();
    cx.add(popup, key, SkillSetListControl::new(Anchor::NONE, r, tab, order)).id()
}
