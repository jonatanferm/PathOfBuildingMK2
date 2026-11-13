//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/MinionListControl.lua 1-107
//! (`newClass("MinionListControl", "ListControl")`, the spectre lists of the Spectre Library
//! popup). Owner: U31.
//!
//! @pob-file Classes/MinionListControl.lua @16de4b82d
//!
//! The only constructors are Build.lua:1536-1537 (`OpenSpectreLibrary`, U20), which reach this
//! file through `on_frame::ShellHooks::minion_list_control` / `minion_search_list_control`
//! (CCR-0104 §1; the entries are [`spectre_library_list`] and
//! [`super::minion_search_list::spectre_library_source`]).
//!
//! Rows are minion ids (`data.minions` keys, Lua strings): a row value is a [`ListValue`] keyed
//! by the interned [`Name`] ([`name_value`]), so `selValue == value` and `isValueInArray` compare
//! the strings, as in Lua. `self.data` is the build's `data`, i.e. the global game data
//! ([`model::data`]); the struct keeps no field for it.
//!
//! The destination list (`controls.list`, no `dest`) aliases the popup's `destList`
//! ([`NameListSource`] over the `Rc<RefCell<Vec<Name>>>` the Save button reads); reorder, delete
//! and the received drags write through. The source list (`dest` set) owns its list
//! ([`ListData::Local`]).
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `GetDragValue`, `OnSelClick`,
//! `OnSelDelete`; `CanReceiveDrag` / `ReceiveDrag` are the framework's drag-target methods.
//! MinionSearchListControl (the subclass) overrides no method of these.
//!
//! Props with side effects (ui doc §7.3): none. ORDER: no `pairs` loops.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::lfmt;
use pob_ui_core::controls::list::{
    ListControl, ListData, ListHooks, ListSource, ListValue, ValKey,
};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::TtKey;
use pob_ui_core::tooltip::Tooltip;

use crate::model::{self, Name};

// @pob-skip Classes/MinionListControl.lua:1-13 reason="header comment, the table/string locals and newClass (the struct and Widget impl below)"

/// A row value: a minion id (keyed by the interned string, Lua string equality).
pub fn name_value(id: Name) -> ListValue {
    ListValue {
        key: ValKey(id.as_char_ptr() as usize),
        data: Rc::new(id),
    }
}

/// The minion id of a row value.
#[track_caller]
pub fn value_name(v: &ListValue) -> Name {
    *v.get::<Name>()
        .unwrap_or_else(|| lua_error!("MinionListControl: a list value that is not a minion id"))
}

/// `self.list = destList` (Build.lua:1523/1536): the popup's list, shared with its Save button.
pub struct NameListSource(pub Rc<RefCell<Vec<Name>>>);

impl ListSource for NameListSource {
    fn len(&self, _cx: &Cx) -> usize {
        self.0.borrow().len()
    }
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        if i == 0 {
            return None;
        }
        self.0.borrow().get(i - 1).copied().map(name_value)
    }
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let id = value_name(&v);
        let mut l = self.0.borrow_mut();
        if i == 0 || i > l.len() + 1 {
            lua_error!("bad argument #2 to 'insert' (position out of bounds)");
        }
        l.insert(i - 1, id);
    }
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        let mut l = self.0.borrow_mut();
        if i == 0 || i > l.len() {
            lua_error!("t_remove: position out of range");
        }
        name_value(l.remove(i - 1))
    }
}

/// `isValueInArray(list, value)` over a list control's list (identity of the row keys).
pub fn list_contains(cx: &Cx, list: CtrlId, v: &ListValue) -> bool {
    let n = ListControl::list_len(cx, list);
    (1..=n).any(|i| ListControl::list_get(cx, list, i).is_some_and(|x| x == *v))
}

/// `self.data.minions[minionId]` (nil is the Lua index error at the use).
#[track_caller]
fn minion(id: Name) -> model::gamedata::schema::minions::MinionDataRef {
    model::data()
        .minions
        .get(&id)
        .cloned()
        .unwrap_or_else(|| lua_error!("attempt to index local 'minion' (a nil value)"))
}

/// `MinionListControl`: a list of minion ids, with an "Add" button (`dest` set) or a "Remove"
/// button (the destination list itself).
pub struct MinionListControl {
    pub list: ListControl,
    /// `self.dest` (the destination list control)
    pub dest: Option<CtrlId>,
}

pub(super) static HOOKS: ListHooks = ListHooks {
    get_row_value: MinionListControl::GetRowValue,
    add_value_tooltip: Some(MinionListControl::AddValueTooltip),
    get_drag_value: Some(MinionListControl::GetDragValue),
    on_sel_click: Some(MinionListControl::OnSelClick),
    on_sel_delete: Some(MinionListControl::OnSelDelete),
    ..ListHooks::BASE
};

impl MinionListControl {
    /// The fields of `self:ListControl(anchor, rect, 16, "VERTICAL", not dest, list)` plus
    /// `self.dest` (L15, L17); the subclass embeds this state.
    pub(super) fn state(
        hooks: &'static ListHooks,
        list: ListData,
        dest: Option<CtrlId>,
    ) -> MinionListControl {
        // L15
        let l = ListControl::state(hooks, 16.0, true, dest.is_none(), list, false);
        MinionListControl {
            list: l,
            // L17
            dest,
        }
    }

    /// lua: MinionListControl.lua:14-37 `MinionListClass:MinionListControl(anchor, rect, data,
    /// list, dest)` (`data` is the global data: module notes)
    /// @pob Classes/MinionListControl.lua:14-37 h=007ecaa0
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        list: ListData,
        dest: Option<CtrlId>,
    ) -> New<MinionListControl> {
        New::new(Base::new(anchor, r), Self::state(&HOOKS, list, dest))
            .host()
            .init(|cx, me| ListControl::init(cx, me.0))
            .init(|cx, me| Self::ctor_tail(cx, me.0))
    }

    /// L16-L36 (after `self:ListControl(..)` and `self.data = data`; also the subclass's).
    pub(super) fn ctor_tail(cx: &mut Cx, me: CtrlId) {
        let dest = cx.w::<MinionListControl>(me).dest;
        // L18
        if let Some(dest) = dest {
            // L19
            cx.w_mut::<ListControl>(me).drag_target_list = Some(vec![dest]);
            // L20
            cx.w_mut::<ListControl>(me).label = Some(Prop::Val("^7Available Spectres:".to_owned()));
            // L21-23
            let add = cx.add(
                me,
                "add",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, TopRight),
                    rect(0, -2, 60, 18),
                    "Add",
                    move |cx| {
                        // L22
                        Self::AddSel(cx, me);
                    },
                ),
            );
            // L24-26
            cx.base_mut(add).enabled = Prop::f(move |cx, _| {
                // L25
                let sel = cx.w::<ListControl>(me).sel_value.clone();
                sel.is_some_and(|v| !list_contains(cx, dest, &v))
            });
        } else {
            // L28
            cx.w_mut::<ListControl>(me).label = Some(Prop::Val("^7Spectres in Build:".to_owned()));
            // L29-31
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, TopRight),
                    rect(0, -2, 60, 18),
                    "Remove",
                    move |cx| {
                        // L30
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        // `OnSelDelete(nil, nil)` cannot happen: the button is disabled without a
                        // selection (L33), and a selection always has both fields
                        let value = value.unwrap_or_else(|| {
                            lua_error!("bad argument #2 to 'remove' (number expected, got nil)")
                        });
                        Self::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L32-34
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L33
                cx.w::<ListControl>(me).sel_value.is_some()
            });
        }
        // L36 return self
    }

    /// lua: MinionListControl.lua:39-43 `MinionListClass:AddSel()`
    /// @pob Classes/MinionListControl.lua:39-43 h=6c423c54
    pub fn AddSel(cx: &mut Cx, me: CtrlId) {
        let dest = cx.w::<MinionListControl>(me).dest;
        let sel = cx.w::<ListControl>(me).sel_value.clone();
        // L40
        if let Some(dest) = dest {
            // `isValueInArray(dest.list, nil)` is nil and `t_insert(dest.list, nil)` stores
            // nothing, so a nil selection changes nothing
            let Some(sel) = sel else { return };
            if !list_contains(cx, dest, &sel) {
                // L41
                let n = ListControl::list_len(cx, dest);
                ListControl::list_insert(cx, dest, n + 1, sel);
            }
        }
    }

    /// lua: MinionListControl.lua:45-50 `MinionListClass:GetRowValue(column, index, minionId)`
    /// @pob Classes/MinionListControl.lua:45-50 h=1af2207a
    fn GetRowValue(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        // L46
        let id = value_name(v);
        let minion = model::data().minions.get(&id).cloned();
        // L47
        if column == 1 {
            // L48
            let minion = minion
                .unwrap_or_else(|| lua_error!("attempt to index local 'minion' (a nil value)"));
            return minion.name.to_string();
        }
        String::new()
    }

    /// lua: MinionListControl.lua:52-81 `MinionListClass:AddValueTooltip(tooltip, index,
    /// minionId)`
    /// @pob Classes/MinionListControl.lua:52-81 h=3ccb0ff6
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        let id = value_name(v);
        // L53
        if tooltip.check_for_update(cx, &[TtKey::Str(Rc::from(id.as_str()))]) {
            // L54
            let minion = minion(id);
            // L55
            tooltip.add_line(cx, 18.0, &format!("^7{}", minion.name));
            // L56
            tooltip.add_line(cx, 14.0, &lfmt!("^7Life Multiplier: x%.2f", minion.life));
            // L57
            if let Some(es) = minion.energyShield {
                // L58
                tooltip.add_line(
                    cx,
                    14.0,
                    &lfmt!("^7Energy Shield: %d%% of base Life", es * 100.0),
                );
            }
            // L60
            if let Some(armour) = minion.armour {
                // L61
                tooltip.add_line(cx, 14.0, &lfmt!("^7Armour Multiplier: x%.2f", 1.0 + armour));
            }
            // L63
            if let Some(evasion) = minion.evasion {
                // L64
                tooltip.add_line(
                    cx,
                    14.0,
                    &lfmt!("^7Evasion Multiplier: x%.2f", 1.0 + evasion),
                );
            }
            // L66-71
            let cc = cx.main.cc.clone();
            let line = lfmt!(
                "^7Resistances: %s%d^7/%s%d^7/%s%d^7/%s%d",
                cc.FIRE.as_str(),
                minion.fireResist,
                cc.COLD.as_str(),
                minion.coldResist,
                cc.LIGHTNING.as_str(),
                minion.lightningResist,
                cc.CHAOS.as_str(),
                minion.chaosResist
            );
            tooltip.add_line(cx, 14.0, &line);
            // L72
            tooltip.add_line(cx, 14.0, &lfmt!("^7Base Damage: x%.2f", minion.damage));
            // L73
            tooltip.add_line(
                cx,
                14.0,
                &lfmt!("^7Base Attack Speed: %.2f", 1.0 / minion.attackTime),
            );
            // L75
            for skillId in &minion.skillList {
                // L76
                if let Some(skill) = model::data().skills.get(skillId) {
                    // L77
                    tooltip.add_line(cx, 14.0, &format!("^7Skill: {}", skill.name));
                }
            }
        }
    }

    /// lua: MinionListControl.lua:83-85 `MinionListClass:GetDragValue(index, value)`
    /// @pob Classes/MinionListControl.lua:83-85 h=9a6adf8d
    fn GetDragValue(
        _cx: &Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
    ) -> Option<(Rc<str>, DragValue)> {
        // L84
        Some((Rc::from("MinionId"), DragValue(Rc::new(value_name(v)))))
    }

    /// lua: MinionListControl.lua:95-99 `MinionListClass:OnSelClick(index, minionId,
    /// doubleClick)`
    /// @pob Classes/MinionListControl.lua:95-99 h=33c680d2
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        _index: usize,
        _v: &ListValue,
        double_click: bool,
    ) -> Option<bool> {
        // L96
        if double_click && cx.w::<MinionListControl>(me).dest.is_some() {
            // L97
            Self::AddSel(cx, me);
        }
        None
    }

    /// lua: MinionListControl.lua:101-107 `MinionListClass:OnSelDelete(index, minionId)`
    /// @pob Classes/MinionListControl.lua:101-107 h=e4ef9779
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, _v: &ListValue) {
        // L102
        if cx.w::<MinionListControl>(me).dest.is_none() {
            // L103 (`t_remove` of a position outside 1..#list does nothing)
            ListControl::list_remove(cx, me, index);
            let w = cx.w_mut::<ListControl>(me);
            // L104
            w.sel_index = None;
            // L105
            w.sel_value = None;
        }
    }

    /// The drag payload as a minion id (another list's payload type is not a minion id).
    fn drag_name(v: &DragValue) -> Option<Name> {
        v.0.downcast_ref::<Name>().copied()
    }

    /// lua: MinionListControl.lua:87-89 `MinionListClass:CanReceiveDrag(type, value)`
    /// @pob Classes/MinionListControl.lua:87-89 h=b77f0df8
    pub(super) fn CanReceiveDrag(cx: &Cx, me: CtrlId, ty: &str, v: &DragValue) -> bool {
        // L88
        ty == "MinionId"
            && !Self::drag_name(v).is_some_and(|id| list_contains(cx, me, &name_value(id)))
    }

    /// lua: MinionListControl.lua:91-93 `MinionListClass:ReceiveDrag(type, value, source)`
    /// @pob Classes/MinionListControl.lua:91-93 h=9d4ffdb3
    pub(super) fn ReceiveDrag(cx: &mut Cx, me: CtrlId, _ty: &str, v: &DragValue, _source: CtrlId) {
        // L92 (a "MinionId" payload is always a minion id: CanReceiveDrag accepted the type)
        let id = Self::drag_name(v).unwrap_or_else(|| {
            lua_error!("MinionListControl: a drag value that is not a minion id")
        });
        let pos = cx
            .w::<ListControl>(me)
            .sel_drag_index
            .unwrap_or_else(|| ListControl::list_len(cx, me) + 1);
        ListControl::list_insert(cx, me, pos, name_value(id));
    }
}

impl Widget for MinionListControl {
    const NAME: &'static str = "MinionListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS
        .union(Caps::CAN_RECEIVE_DRAG)
        .union(Caps::RECEIVE_DRAG);

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
    fn can_receive_drag(cx: &Cx, me: CtrlId, ty: &str, v: &DragValue) -> bool {
        Self::CanReceiveDrag(cx, me, ty, v)
    }
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, source: CtrlId) {
        Self::ReceiveDrag(cx, me, ty, v, source);
    }

    pob_ui_core::impl_parts!(MinionListControl: list => ListControl);
}

/// `controls.list = new("MinionListControl"):MinionListControl(nil, {-139, 40, 265, 250},
/// self.data, destList)` (Build.lua:1536) in the Spectre Library popup `popup`: the
/// `ShellHooks::minion_list_control` entry (CCR-0104 §1).
pub fn spectre_library_list(
    cx: &mut Cx,
    popup: CtrlId,
    dest_list: Rc<RefCell<Vec<Name>>>,
) -> CtrlId {
    cx.add(
        popup,
        "list",
        MinionListControl::new(
            Anchor::NONE,
            rect(-139, 40, 265, 250),
            ListData::Source(Rc::new(NameListSource(dest_list))),
            None,
        ),
    )
    .id()
}
