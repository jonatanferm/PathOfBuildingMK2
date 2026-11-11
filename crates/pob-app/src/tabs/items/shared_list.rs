//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SharedItemListControl.lua 1-80
//! (`newClass("SharedItemListControl", "ListControl")`). Owner: U32.
//!
//! @pob-file Classes/SharedItemListControl.lua @16de4b82d
//!
//! The list is `main.sharedItemList` itself (L18; U16's `MainState::shared_item_list`), aliased
//! through [`SharedItemsSource`]: a drag reorder, a drop and a delete write through. Rows are
//! the shared `ItemRef`s (Lua table identity).
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `GetDragValue`, `OnSelClick`,
//! `OnSelCopy`, `OnSelDelete`; `ReceiveDrag` is the framework's drag-target method (no
//! `CanReceiveDrag`: every drag source arms it).
//!
//! LUA-QUIRK(insert-zero): ReceiveDrag inserts at `self.selDragIndex or #self.list` (L59): with
//! no drag index that is before the last item, and on an empty list position 0, which
//! LuaJIT's `table.insert` stores at `t[0]` (no bounds check): outside the sequence, so neither
//! drawn nor saved, and moved to `t[1]` by the next insert at 0. The port keeps that slot in
//! [`SharedItemListControl::zero_slot`]. DIVERGENCE: the slot belongs to the control here and to
//! `main.sharedItemList` in PoB (a new build's control starts without it).
//!
//! ORDER: no `pairs` loops.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::pattern;
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListSource, ListValue};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtKey, TtObj};
use pob_ui_core::tooltip::Tooltip;

use super::slot::{drag_item, item_label};
use super::slot_helper::items_hooks;
use crate::app::{app, app_mut, CxApp};
use crate::model;

// @pob-skip Classes/SharedItemListControl.lua:1-11 reason="header comment, local aliases and newClass (the struct and Widget impl below)"

/// `self.list = main.sharedItemList` (aliased).
pub struct SharedItemsSource;

impl ListSource for SharedItemsSource {
    fn len(&self, cx: &Cx) -> usize {
        app(&*cx.ext).main.shared_item_list.len()
    }
    fn get(&self, cx: &Cx, i: usize) -> Option<ListValue> {
        let l = &app(&*cx.ext).main.shared_item_list;
        (i >= 1)
            .then(|| l.get(i - 1))
            .flatten()
            .cloned()
            .map(ListValue::of)
    }
    fn insert(&self, cx: &mut Cx, i: usize, v: ListValue) {
        let item = v.rc::<model::Item>().unwrap_or_else(|| {
            lua_error!("SharedItemsSource::insert: a value that is not an item")
        });
        let l = &mut app_mut(&mut *cx.ext).main.shared_item_list;
        if i == 0 || i > l.len() + 1 {
            lua_error!("bad argument #2 to 'insert' (position out of bounds)");
        }
        l.insert(i - 1, item);
    }
    fn remove(&self, cx: &mut Cx, i: usize) -> ListValue {
        let l = &mut app_mut(&mut *cx.ext).main.shared_item_list;
        if i == 0 || i > l.len() {
            lua_error!("t_remove: position out of range");
        }
        ListValue::of(l.remove(i - 1))
    }
}

/// The row value as an item.
fn item_of(v: &ListValue) -> model::ItemRef {
    v.rc::<model::Item>()
        .unwrap_or_else(|| lua_error!("SharedItemListControl: a list value that is not an item"))
}

/// `SharedItemListControl`: the items shared between builds.
pub struct SharedItemListControl {
    pub list: ListControl,
    /// `self.itemsTab`
    pub items_tab: CtrlId,
    /// `main.sharedItemList[0]` (LUA-QUIRK(insert-zero))
    pub zero_slot: Option<model::ItemRef>,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: SharedItemListControl::GetRowValue,
    add_value_tooltip: Some(SharedItemListControl::AddValueTooltip),
    get_drag_value: Some(SharedItemListControl::GetDragValue),
    on_sel_click: Some(SharedItemListControl::OnSelClick),
    on_sel_copy: Some(SharedItemListControl::OnSelCopy),
    on_sel_delete: Some(SharedItemListControl::OnSelDelete),
    ..ListHooks::BASE
};

impl SharedItemListControl {
    /// lua: SharedItemListControl.lua:17-30 `SharedItemListClass:SharedItemListControl(anchor,
    /// rect, itemsTab, forceTooltip)`
    /// @pob Classes/SharedItemListControl.lua:13-30 h=469fdb5e
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        items_tab: CtrlId,
        force_tooltip: bool,
    ) -> New<SharedItemListControl> {
        // L18
        let mut l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            true,
            ListData::Source(Rc::new(SharedItemsSource)),
            force_tooltip,
        );
        // L20
        l.label = Some(Prop::Val("^7Shared items:".to_owned()));
        // L21
        l.default_text = Some("^x7F7F7FThis is a list of items that will be shared between all of\nyour builds.\nYou can add items to this list by dragging them from\none of the other lists.".to_owned());
        // L22
        l.drag_target_list = Some(Vec::new());
        New::new(
            Base::new(anchor, r),
            SharedItemListControl {
                list: l,
                // L19
                items_tab,
                zero_slot: None,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(|cx, me| {
            let me = me.0;
            // L23-25
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, TopRight),
                    rect(0, -2, 60, 18),
                    "Delete",
                    move |cx| {
                        // L24
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'item' (a nil value)")
                        });
                        SharedItemListControl::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L26-28
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L27
                cx.w::<ListControl>(me).sel_value.is_some()
            });
            // L29 return self
        })
    }

    /// lua: SharedItemListControl.lua:32-36 `SharedItemListClass:GetRowValue(column, index, item)`
    /// @pob Classes/SharedItemListControl.lua:32-36 h=134928e8
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        // L33
        if column == 1 {
            // L34
            return item_label(cx, &item_of(v));
        }
        String::new()
    }

    /// lua: SharedItemListControl.lua:38-46 `SharedItemListClass:AddValueTooltip(tooltip, index,
    /// item)`
    /// @pob Classes/SharedItemListControl.lua:38-46 h=a77293a1
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        // L39
        if !cx.main.popups.is_empty() {
            // L40
            tooltip.clear(false);
            // L41
            return;
        }
        let item = item_of(v);
        // L43
        let keys = [
            TtKey::Obj(TtObj(item.clone())),
            TtKey::Bool(cx.sys.is_key_down(Key::Shift)),
            TtKey::Bool(cx.app().launch.dev_mode_alt),
            TtKey::Num(cx.build().model.outputRevision.get()),
        ];
        if tooltip.check_for_update(cx, &keys) {
            // L44
            let f = items_hooks().add_item_tooltip;
            f(cx, tooltip, &item, None, false);
        }
    }

    /// lua: SharedItemListControl.lua:48-50 `SharedItemListClass:GetDragValue(index, item)`
    /// @pob Classes/SharedItemListControl.lua:48-50 h=89234986
    fn GetDragValue(
        _cx: &Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
    ) -> Option<(Rc<str>, DragValue)> {
        // L49
        Some((Rc::from("Item"), DragValue(item_of(v))))
    }

    /// lua: SharedItemListControl.lua:63-68 `SharedItemListClass:OnSelClick(index, item,
    /// doubleClick)`
    /// @pob Classes/SharedItemListControl.lua:63-68 h=1e2f6cad
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        _index: usize,
        v: &ListValue,
        double_click: bool,
    ) -> Option<bool> {
        // L64
        if double_click {
            // L65
            let f = items_hooks().create_display_item_from_raw;
            f(cx, &item_of(v).raw.get(), true);
            // L66
            cx.w_mut::<ListControl>(me).sel_dragging = false;
        }
        None
    }

    /// lua: SharedItemListControl.lua:70-72 `SharedItemListClass:OnSelCopy(index, item)`
    /// @pob Classes/SharedItemListControl.lua:70-72 h=d436f033
    fn OnSelCopy(cx: &mut Cx, _me: CtrlId, _index: usize, v: &ListValue) {
        // L71
        let text = pattern::gsub(&item_of(v).BuildRaw(), "\n", "\r\n", None).0;
        cx.sys.platform.copy(&text);
    }

    /// lua: SharedItemListControl.lua:74-80 `SharedItemListClass:OnSelDelete(index, item)`
    /// @pob Classes/SharedItemListControl.lua:74-80 h=96a68148
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let name = item_of(v).name.get().to_string();
        // L75-79
        popups::open_confirm_popup(
            cx,
            "Delete Item",
            &format!("Are you sure you want to remove '{name}' from the shared item list?"),
            "Delete",
            move |cx| {
                // L76
                ListControl::list_remove(cx, me, index);
                // L77-78
                let w = cx.w_mut::<ListControl>(me);
                w.sel_index = None;
                w.sel_value = None;
            },
            None,
        );
    }
}

impl Widget for SharedItemListControl {
    const NAME: &'static str = "SharedItemListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS.union(Caps::RECEIVE_DRAG);

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

    /// lua: SharedItemListControl.lua:52-61 `SharedItemListClass:ReceiveDrag(type, value, source)`
    /// @pob Classes/SharedItemListControl.lua:52-61 h=73949be6
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, _source: CtrlId) {
        // L53
        if ty == "Item" {
            let value = drag_item(v);
            // L54
            let raw = value.BuildRaw();
            // L55
            let new_item = crate::main_mode::new_item(Some(&raw), None, false);
            // L56
            if value.id.get().is_none() {
                // L57
                new_item.NormaliseQuality();
            }
            // L59
            let pos = cx
                .w::<ListControl>(me)
                .sel_drag_index
                .unwrap_or_else(|| ListControl::list_len(cx, me));
            if pos == 0 {
                // LUA-QUIRK(insert-zero): t[1] = t[0]; t[0] = newItem (#list is 0 here)
                let prev = cx
                    .w_mut::<SharedItemListControl>(me)
                    .zero_slot
                    .replace(new_item);
                if let Some(prev) = prev {
                    ListControl::list_insert(cx, me, 1, ListValue::of(prev));
                }
            } else {
                ListControl::list_insert(cx, me, pos, ListValue::of(new_item));
            }
        }
    }

    pob_ui_core::impl_parts!(SharedItemListControl: list => ListControl);
}
