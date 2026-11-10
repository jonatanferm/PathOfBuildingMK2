//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemSetListControl.lua 1-145
//! (`newClass("ItemSetListControl", "ListControl")`). Owner: U32.
//!
//! @pob-file Classes/ItemSetListControl.lua @16de4b82d
//!
//! The list is `itemsTab.itemSetOrderList` itself (L15; calc:BM-A's
//! `build.itemsTab.itemSetOrderList`, aliased through [`IdListSource`]); rows are item-set ids.
//! Item sets are calc:BM-A's tables (`build.itemsTab.itemSets[id]`). The drag payload of a row
//! (`"ItemList", itemSet`) is the set's [`Table`] (Lua identity: `Table::ptr_eq`).
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `GetDragValue`, `OnOrderChange`,
//! `OnSelClick`, `OnSelDelete`, `OnSelKeyDown`; `CanReceiveDrag`/`ReceiveDrag` are the
//! framework's drag-target methods.
//!
//! ORDER: L101 `pairs(value.slots)` (a shared item set's string-keyed slots): the items get
//! their ids in this order, which PoB randomises per process; the port uses the set's
//! insertion order (U16's `SharedItemSet::slots`, LUA-ORDER there) (c: the items' slots do not
//! depend on it; only their ids and their order in `itemOrderList` do).
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
use pob_ui_core::tooltip::Tooltip;

use super::item_list::{id_value, set_title, value_id, IdListSource};
use super::shared_set_list::shared_set_of;
use super::slot_helper::items_hooks;
use crate::app::CxApp;
use crate::model::{self, Table, Val};

// @pob-skip Classes/ItemSetListControl.lua:1-12 reason="header comment, local aliases and newClass (the struct and Widget impl below)"

/// `ItemSetListControl`: the build's item sets (the item set manage popup).
pub struct ItemSetListControl {
    pub list: ListControl,
    /// `self.itemsTab`
    pub items_tab: CtrlId,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: ItemSetListControl::GetRowValue,
    add_value_tooltip: Some(ItemSetListControl::AddValueTooltip),
    get_drag_value: Some(ItemSetListControl::GetDragValue),
    on_order_change: Some(ItemSetListControl::OnOrderChange),
    on_sel_click: Some(ItemSetListControl::OnSelClick),
    on_sel_delete: Some(ItemSetListControl::OnSelDelete),
    on_sel_key_down: Some(ItemSetListControl::OnSelKeyDown),
    ..ListHooks::BASE
};

/// `itemsTab.itemSets[id]`
fn item_set(cx: &Cx, id: f64) -> Option<Table> {
    let k = model::items_tab::id_key(id)?;
    cx.build()
        .model
        .itemsTab
        .itemSets
        .with(|s| s.get(&k).cloned())
}

/// `itemsTab.itemSets[id]` that Lua then indexes.
fn item_set_or_raise(cx: &Cx, id: f64) -> Table {
    item_set(cx, id).unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"))
}

/// The drag payload of a row: the item set table.
pub fn drag_set(v: &DragValue) -> Table {
    v.0.downcast_ref::<Table>()
        .cloned()
        .unwrap_or_else(|| lua_error!("ItemSetListControl: a drag value that is not an item set"))
}

impl ItemSetListControl {
    /// lua: ItemSetListControl.lua:14-46 `ItemSetListClass:ItemSetListControl(anchor, rect,
    /// itemsTab)`
    /// @pob Classes/ItemSetListControl.lua:14-46 h=1e0111dd
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        items_tab: CtrlId,
        order_list: pob_lua::Shared<Vec<u32>>,
    ) -> New<ItemSetListControl> {
        // L15
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
            ItemSetListControl {
                list: l,
                // L16
                items_tab,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(|cx, me| {
            let me = me.0;
            let sel_value = move |cx: &Cx| cx.w::<ListControl>(me).sel_value.as_ref().map(value_id);
            // L17-25
            let copy = cx.add(
                me,
                "copy",
                ButtonControl::new(
                    Anchor::new(BottomLeft, me, Top),
                    rect(2, -4, 60, 18),
                    "Copy",
                    move |cx| {
                        let m = cx.build().model.clone();
                        // L18
                        let src = sel_value(cx)
                            .and_then(|id| item_set(cx, f64::from(id)))
                            .unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
                            });
                        let new_set = src.copyTable(false);
                        // L19
                        let mut id = 1.0;
                        // L20
                        while item_set(cx, id).is_some() {
                            // L21
                            id += 1.0;
                        }
                        new_set.set(n!("id"), id);
                        // L23
                        let k = model::items_tab::id_key(id).expect("an integer id");
                        m.itemsTab
                            .itemSets
                            .with_mut(|s| s.insert(k, new_set.clone()));
                        // L24
                        ItemSetListControl::RenameSet(cx, me, &new_set, true);
                    },
                ),
            );
            // L26-28
            cx.base_mut(copy).enabled = Prop::f(move |cx, _| {
                // L27
                sel_value(cx).is_some()
            });
            // L29-31
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(Left, copy, Right),
                    rect(4, 0, 60, 18),
                    "Delete",
                    move |cx| {
                        // L30
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'itemSet' (a nil value)")
                        });
                        ItemSetListControl::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L32-34
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L33
                sel_value(cx).is_some() && ListControl::list_len(cx, me) > 1
            });
            // L35-37
            let rename = cx.add(
                me,
                "rename",
                ButtonControl::new(
                    Anchor::new(BottomRight, me, Top),
                    rect(-2, -4, 60, 18),
                    "Rename",
                    move |cx| {
                        // L36
                        let set = sel_value(cx)
                            .and_then(|id| item_set(cx, f64::from(id)))
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index local 'itemSet' (a nil value)")
                            });
                        ItemSetListControl::RenameSet(cx, me, &set, false);
                    },
                ),
            );
            // L38-40
            cx.base_mut(rename).enabled = Prop::f(move |cx, _| {
                // L39
                sel_value(cx).is_some()
            });
            // L41-44
            cx.add(
                me,
                "new",
                ButtonControl::new(
                    Anchor::new(Right, rename, Left),
                    rect(-4, 0, 60, 18),
                    "New",
                    move |cx| {
                        // L42
                        let f = items_hooks().new_item_set;
                        let new_set = f(cx);
                        // L43
                        ItemSetListControl::RenameSet(cx, me, &new_set, true);
                    },
                ),
            );
            // L45 return self
        })
    }

    /// lua: ItemSetListControl.lua:48-74 `ItemSetListClass:RenameSet(itemSet, addOnName)`
    /// @pob Classes/ItemSetListControl.lua:48-74 h=95174414
    pub fn RenameSet(cx: &mut Cx, me: CtrlId, item_set: &Table, add_on_name: bool) {
        let title = item_set.str(n!("title")).map(|s| s.to_string());
        // L73 (the arguments are evaluated after the constructors; these draw nothing)
        let popup_title = if title.is_some() {
            "Rename"
        } else {
            "Set Name"
        };
        let set = item_set.clone();
        popups::open_popup(
            cx,
            370.0,
            100.0,
            popup_title,
            move |cx, p| {
                let p = p.id();
                // L49 local controls = { }
                // L50
                cx.add(
                    p,
                    "label",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(0, 20, 0, 16),
                        "^7Enter name for this item set:",
                    ),
                );
                // L51-53
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
                                // L52
                                let save = cx.child_as::<ButtonControl>(p, "save");
                                let ok = pattern::find(buf, "%S", None, false).is_some();
                                cx.base_mut(save).enabled = Prop::Val(ok);
                            })),
                            ..EditArgs::default()
                        },
                    ),
                );
                // L54-65
                let set_s = set.clone();
                let save = cx.add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                        let edit = cx.child_as::<EditControl>(p, "edit");
                        // L55
                        let buf = cx.get(edit).buf.clone();
                        set_s.set(n!("title"), buf.as_str());
                        let h = items_hooks();
                        // L56
                        (h.set_mod_flag)(cx, true);
                        // L57
                        if add_on_name {
                            // L58
                            let id = set_s.num(n!("id"));
                            let n = ListControl::list_len(cx, me);
                            ListControl::list_insert(cx, me, n + 1, id_value(id as u32));
                            // L59-60
                            let n = ListControl::list_len(cx, me);
                            let w = cx.w_mut::<ListControl>(me);
                            w.sel_index = Some(n);
                            w.sel_value = Some(id_value(id as u32));
                        }
                        // L62
                        (h.add_undo_state)(cx);
                        // L63
                        let f = crate::build::init::hooks(cx).sync_loadouts;
                        f(cx);
                        // L64
                        popups::close_popup(cx);
                    }),
                );
                // L66
                cx.base_mut(save).enabled = Prop::Val(false);
                // L67-72
                let set_c = set.clone();
                cx.add(
                    p,
                    "cancel",
                    ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", move |cx| {
                        // L68
                        if add_on_name {
                            // L69
                            let id = set_c.num(n!("id"));
                            if let Some(k) = model::items_tab::id_key(id) {
                                cx.build()
                                    .model
                                    .itemsTab
                                    .itemSets
                                    .with_mut(|s| s.shift_remove(&k));
                            }
                        }
                        // L71
                        popups::close_popup(cx);
                    }),
                );
            },
            // L73
            PopupOpts {
                enter: Some("save".into()),
                default: Some("edit".into()),
                escape: Some("cancel".into()),
                ..PopupOpts::default()
            },
        );
    }

    /// lua: ItemSetListControl.lua:76-81 `ItemSetListClass:GetRowValue(column, index, itemSetId)`
    /// @pob Classes/ItemSetListControl.lua:76-81 h=b7c06438
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        let id = f64::from(value_id(v));
        // L77
        let set = item_set(cx, id);
        // L78
        if column == 1 {
            let set =
                set.unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
            // L79
            let current = if id == cx.build().model.itemsTab.activeItemSetId.get() {
                "  ^9(Current)"
            } else {
                ""
            };
            return format!("{}{current}", set_title(&set));
        }
        String::new()
    }

    /// lua: ItemSetListControl.lua:83-87 `ItemSetListClass:AddValueTooltip(tooltip, index,
    /// itemSetId)`
    /// @pob Classes/ItemSetListControl.lua:83-87 h=c8d1d6bf
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        // L84
        let set = item_set_or_raise(cx, f64::from(value_id(v)));
        // L85
        tooltip.clear(false);
        // L86
        let f = items_hooks().add_item_set_tooltip;
        f(cx, tooltip, &set);
    }

    /// lua: ItemSetListControl.lua:89-91 `ItemSetListClass:GetDragValue(index, itemSetId)`
    /// @pob Classes/ItemSetListControl.lua:89-91 h=62270e0c
    fn GetDragValue(
        cx: &Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
    ) -> Option<(Rc<str>, DragValue)> {
        // L90 (a nil set would be a nil payload; every row id names a set)
        let set = item_set_or_raise(cx, f64::from(value_id(v)));
        Some((Rc::from("ItemList"), DragValue(Rc::new(set))))
    }

    /// lua: ItemSetListControl.lua:112-114 `ItemSetListClass:OnOrderChange()`
    /// @pob Classes/ItemSetListControl.lua:112-114 h=c3576887
    fn OnOrderChange(cx: &mut Cx, _me: CtrlId, _sel_index: usize, _sel_drag_index: usize) {
        // L113
        let f = items_hooks().set_mod_flag;
        f(cx, true);
    }

    /// lua: ItemSetListControl.lua:116-121 `ItemSetListClass:OnSelClick(index, itemSetId,
    /// doubleClick)`
    /// @pob Classes/ItemSetListControl.lua:116-121 h=c3469fc2
    fn OnSelClick(
        cx: &mut Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
        double_click: bool,
    ) -> Option<bool> {
        let id = f64::from(value_id(v));
        // L117
        if double_click && id != cx.build().model.itemsTab.activeItemSetId.get() {
            let h = items_hooks();
            // L118
            (h.set_active_item_set)(cx, id);
            // L119
            (h.add_undo_state)(cx);
        }
        None
    }

    /// lua: ItemSetListControl.lua:123-138 `ItemSetListClass:OnSelDelete(index, itemSetId)`
    /// @pob Classes/ItemSetListControl.lua:123-138 h=1265fb62
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue) {
        let id = value_id(v);
        // L124
        let set = item_set(cx, f64::from(id));
        // L125
        if ListControl::list_len(cx, me) > 1 {
            let set =
                set.unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
            // L126-136
            popups::open_confirm_popup(
                cx,
                "Delete Item Set",
                &format!(
                    "Are you sure you want to delete '{}'?\nThis will not delete any items used by the set.",
                    set_title(&set)
                ),
                "Delete",
                move |cx| {
                    // L127
                    ListControl::list_remove(cx, me, index);
                    // L128
                    cx.build()
                        .model
                        .itemsTab
                        .itemSets
                        .with_mut(|s| s.shift_remove(&id));
                    // L129-130
                    {
                        let w = cx.w_mut::<ListControl>(me);
                        w.sel_index = None;
                        w.sel_value = None;
                    }
                    let h = items_hooks();
                    // L131
                    if f64::from(id) == cx.build().model.itemsTab.activeItemSetId.get() {
                        // L132
                        let i = m_max(1.0, index as f64 - 1.0) as usize;
                        let next = ListControl::list_get(cx, me, i).map_or(Val::Nil, |v| {
                            Val::Num(f64::from(value_id(&v)))
                        });
                        // (a nil id: SetActiveItemSet falls back to itemSetOrderList[1])
                        (h.set_active_item_set)(cx, next.opt_num().unwrap_or(f64::NAN));
                    }
                    // L134
                    (h.add_undo_state)(cx);
                    // L135
                    let f = crate::build::init::hooks(cx).sync_loadouts;
                    f(cx);
                },
                None,
            );
        }
    }

    /// lua: ItemSetListControl.lua:140-145 `ItemSetListClass:OnSelKeyDown(index, itemSetId, key)`
    /// @pob Classes/ItemSetListControl.lua:140-145 h=72ebd2aa
    fn OnSelKeyDown(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue, key: Key) {
        // L141
        let set = item_set(cx, f64::from(value_id(v)));
        // L142
        if key == Key::F(2) {
            let set =
                set.unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
            // L143
            Self::RenameSet(cx, me, &set, false);
        }
    }
}

impl Widget for ItemSetListControl {
    const NAME: &'static str = "ItemSetListControl";
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

    /// lua: ItemSetListControl.lua:93-95 `ItemSetListClass:CanReceiveDrag(type, value)`
    /// @pob Classes/ItemSetListControl.lua:93-95 h=8c697762
    fn can_receive_drag(_cx: &Cx, _me: CtrlId, ty: &str, _v: &DragValue) -> bool {
        // L94
        ty == "SharedItemList"
    }

    /// lua: ItemSetListControl.lua:97-110 `ItemSetListClass:ReceiveDrag(type, value, source)`
    /// @pob Classes/ItemSetListControl.lua:97-110 h=77bb167a
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, _source: CtrlId) {
        // L98
        if ty == "SharedItemList" {
            let value = shared_set_of(cx, v)
                .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
            let h = items_hooks();
            // L99
            let item_set = (h.new_item_set)(cx);
            // L100
            item_set.set(
                n!("title"),
                value.title.as_deref().map_or(Val::Nil, Val::from),
            );
            // L101 (ORDER, module docs)
            for (slot_name, item) in &value.slots {
                // L102
                let new_item = crate::main_mode::new_item(Some(&item.raw.get()), None, false);
                // L103
                new_item.NormaliseQuality();
                // L104
                (h.add_item)(cx, &new_item, true, None);
                // L105
                item_set
                    .table(slot_name.as_str())
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                    .set(
                        n!("selItemId"),
                        new_item.id.get().map_or(Val::Nil, Val::Num),
                    );
            }
            // L107
            let pos = cx
                .w::<ListControl>(me)
                .sel_drag_index
                .unwrap_or_else(|| ListControl::list_len(cx, me) + 1);
            let id = item_set.num(n!("id"));
            ListControl::list_insert(cx, me, pos, id_value(id as u32));
            // L108
            (h.add_undo_state)(cx);
        }
    }

    pob_ui_core::impl_parts!(ItemSetListControl: list => ListControl);
}
