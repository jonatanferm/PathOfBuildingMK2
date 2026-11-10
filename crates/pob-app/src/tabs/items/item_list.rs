//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemListControl.lua 1-388
//! (`newClass("ItemListControl", "ListControl")`). Owner: U32.
//!
//! @pob-file Classes/ItemListControl.lua @16de4b82d
//!
//! The list is `itemsTab.itemOrderList` itself (L18 passes the table; calc:BM-A's
//! `build.itemsTab.orderList`) or, while a loadout filter is selected, a filtered copy
//! (L190): [`IdListSource`] aliases the model vector, so a drag reorder writes through, and
//! `UpdateList` switches between the two as the Lua assigns `self.list`. Rows are item ids
//! (numbers), whose Lua `==` is value equality: the [`ListValue`] key is the id.
//!
//! ListControl hooks (U12's frozen table): `GetRowValue`, `AddValueTooltip`, `GetDragValue`,
//! `OnOrderChange`, `OnSelClick`, `OnSelCopy`, `OnSelDelete`. `ReceiveDrag` is the framework's
//! drag-target method; the class has no `CanReceiveDrag`, so every drag source arms it
//! (ListControl.lua:164). `Draw` and `OnHoverKeyUp` are `Widget` methods.
//!
//! Calls into the ItemsTab UI go through [`ItemsTabHooks`](super::slot_helper::ItemsTabHooks).
//!
//! LUA-QUIRK(abyss-set-title): OnSelDelete's abyssal branch (L352-354) indexes the string
//! `FindEquippedAbyssJewel` returns (`equipSet.title`), which reads `string.title`, nil: the
//! message always says "in set 'Default'". Kept.
//!
//! ORDER:
//! * L32 `pairs(itemsTab.itemOrderList)` and L42/L58 `pairs(itemsTab.build.treeTab.specList)`
//!   iterate sequences (array part: ascending), as the port does.
//! * L55 `pairs(itemsTab.slots)` (string keys): every slot gets `SetSelItemId(0)`; the writes
//!   go to distinct slots and jewel nodes, so the order does not matter (c).
//! * L59 `pairs(spec.jewels)`, L170 `pairs(filterItemSet)`, L177 `pairs(filterSpec.jewels)`,
//!   L215 `pairs(spec.jewels)`, L236 `pairs(itemSet)`: every iteration writes the same value or
//!   computes an any-match, so the order does not matter (c).
//! * L235 `pairs(self.itemsTab.itemSets)`: the last set holding the jewel names the result.
//!   Item-set ids are integers chosen as the lowest free id (ItemsTab.lua:1577, Load's ids), so
//!   PoB's table keeps them in its array part and `pairs` is ascending; the port iterates the
//!   ids ascending (a).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::rc::Rc;

use pob_lua::n;
use pob_lua::pattern;
use pob_lua::Shared;
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem};
use pob_ui_core::controls::list::{
    ListControl, ListData, ListHooks, ListSource, ListValue, ValKey,
};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::popups;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtKey, TtObj};
use pob_ui_core::tooltip::Tooltip;

use super::slot::{item_label, set_sel_item_id, wiki_matches_key, wiki_open_item};
use super::slot_helper::items_hooks;
use crate::app::CxApp;
use crate::build::BuildMode;
use crate::model::{self, Table, Val};

// @pob-skip Classes/ItemListControl.lua:1-11 reason="header comment, local aliases and newClass (the struct and Widget impl below)"

/// A row value: an item id (`ListValue` key = the id, Lua number equality).
pub fn id_value(id: u32) -> ListValue {
    ListValue {
        key: ValKey(id as usize),
        data: Rc::new(id),
    }
}

/// The item id of a row value.
pub fn value_id(v: &ListValue) -> u32 {
    *v.get::<u32>()
        .unwrap_or_else(|| lua_error!("ItemListControl: a list value that is not an id"))
}

/// `self.list = <a model id list>`: `itemsTab.itemOrderList` / `itemsTab.itemSetOrderList`
/// aliased; reorder and delete write through.
pub struct IdListSource(pub Shared<Vec<u32>>);

impl ListSource for IdListSource {
    fn len(&self, _cx: &Cx) -> usize {
        self.0.len()
    }
    fn get(&self, _cx: &Cx, i: usize) -> Option<ListValue> {
        self.0.get(i).map(id_value)
    }
    fn insert(&self, _cx: &mut Cx, i: usize, v: ListValue) {
        let id = value_id(&v);
        self.0.with_mut(|l| {
            if i == 0 || i > l.len() + 1 {
                lua_error!("bad argument #2 to 'insert' (position out of bounds)");
            }
            l.insert(i - 1, id);
        });
    }
    fn remove(&self, _cx: &mut Cx, i: usize) -> ListValue {
        let id = self.0.with_mut(|l| {
            if i == 0 || i > l.len() {
                lua_error!("t_remove: position out of range");
            }
            l.remove(i - 1)
        });
        id_value(id)
    }
}

/// `itemsTab.items[id]` for a u32 id.
fn item(cx: &Cx, id: u32) -> Option<model::ItemRef> {
    cx.build()
        .model
        .itemsTab
        .items
        .with(|m| m.get(&id).cloned())
}

/// `itemsTab.items[id]` that Lua then indexes (nil raises).
fn item_or_raise(cx: &Cx, id: u32) -> model::ItemRef {
    item(cx, id).unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"))
}

/// `itemSet.title or "Default"`
pub fn set_title(set: &Table) -> String {
    set.str(n!("title"))
        .map_or_else(|| "Default".to_owned(), |s| s.to_string())
}

/// `spec.title or "Default"`
fn spec_title(spec: &model::PassiveSpec) -> String {
    spec.title
        .get()
        .map_or_else(|| "Default".to_owned(), |s| s.to_string())
}

/// `spec.nodes[nodeId] and spec.nodes[nodeId].alloc`
fn node_alloc(spec: &model::PassiveSpec, node_id: i64) -> bool {
    spec.nodes
        .with(|m| m.get(node_id))
        .is_some_and(|n| n.alloc.get())
}

/// `isValueInArray(list, value)` over strings (Common.lua:569-575: the first index).
fn index_of(list: &[String], v: &str) -> Option<usize> {
    list.iter().position(|s| s == v).map(|i| i + 1)
}

/// `ItemListControl`: the build's item list.
pub struct ItemListControl {
    pub list: ListControl,
    /// `self.itemsTab`
    pub items_tab: CtrlId,
    /// `self.loadoutListKey`
    pub loadout_list_key: Option<String>,
    /// `self.lastOutputRevision`
    pub last_output_revision: Option<f64>,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: ItemListControl::GetRowValue,
    add_value_tooltip: Some(ItemListControl::AddValueTooltip),
    get_drag_value: Some(ItemListControl::GetDragValue),
    on_order_change: Some(ItemListControl::OnOrderChange),
    on_sel_click: Some(ItemListControl::OnSelClick),
    on_sel_copy: Some(ItemListControl::OnSelCopy),
    on_sel_delete: Some(ItemListControl::OnSelDelete),
    ..ListHooks::BASE
};

impl ItemListControl {
    /// lua: ItemListControl.lua:17-83 `ItemListClass:ItemListControl(anchor, rect, itemsTab,
    /// forceTooltip)`
    /// @pob Classes/ItemListControl.lua:13-83 h=df9a0ce3
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        items_tab: CtrlId,
        order_list: Shared<Vec<u32>>,
        force_tooltip: bool,
    ) -> New<ItemListControl> {
        // L18
        let mut l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            true,
            ListData::Source(Rc::new(IdListSource(order_list))),
            force_tooltip,
        );
        // L20
        l.default_text = Some("^x7F7F7FThis is the list of items that have been added to this build.\nYou can add items to this list by dragging them from\none of the other lists, or by clicking 'Add to build' when\nviewing an item.".to_owned());
        // L21
        l.drag_target_list = Some(Vec::new());
        New::new(
            Base::new(anchor, r),
            ItemListControl {
                list: l,
                // L19
                items_tab,
                loadout_list_key: None,
                last_output_revision: None,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(|cx, me| {
            let me = me.0;
            // L22-24
            let loadout_filter = cx.add(
                me,
                "loadoutFilter",
                DropDownControl::new(
                    Anchor::new(BottomLeft, me, TopLeft),
                    rect(0, -2, 110, 18),
                    Vec::new(),
                    Some(Rc::new(move |cx, _index, _value| {
                        // L23
                        ItemListControl::UpdateList(cx, me);
                    })),
                    None,
                    false,
                ),
            );
            // L25
            cx.get_mut(loadout_filter).enable_dropped_width = true;
            // L26-29
            let sort = cx.add(
                me,
                "sort",
                ButtonControl::new(
                    Anchor::new(Left, loadout_filter, Right),
                    rect(4, 0, 42, 18),
                    "Sort",
                    move |cx| {
                        // L27
                        let f = items_hooks().sort_item_list;
                        f(cx);
                        // L28
                        ItemListControl::UpdateList(cx, me);
                    },
                ),
            );
            // L30-49
            let delete_unused = cx.add(
                me,
                "deleteUnused",
                ButtonControl::new(
                    Anchor::new(Left, sort, Right),
                    rect(4, 0, 84, 18),
                    "Del Unused",
                    move |cx| ItemListControl::delete_unused(cx, me),
                ),
            );
            // L50-52
            cx.base_mut(delete_unused).enabled = Prop::f(move |cx, _| {
                // L51
                ListControl::list_len(cx, me) > 0
            });
            // L53-72
            let delete_all = cx.add(
                me,
                "deleteAll",
                ButtonControl::new(
                    Anchor::new(Left, delete_unused, Right),
                    rect(4, 0, 58, 18),
                    "Del All",
                    move |cx| {
                        // L54
                        popups::open_confirm_popup(
                            cx,
                            "Delete All",
                            "Are you sure you want to delete all items in this build?",
                            "Delete",
                            move |cx| ItemListControl::delete_all(cx, me),
                            None,
                        );
                    },
                ),
            );
            // L73-75
            cx.base_mut(delete_all).enabled = Prop::f(move |cx, _| {
                // L74
                ListControl::list_len(cx, me) > 0
            });
            // L76-78
            let delete = cx.add(
                me,
                "delete",
                ButtonControl::new(
                    Anchor::new(Left, delete_all, Right),
                    rect(4, 0, 50, 18),
                    "Delete",
                    move |cx| {
                        // L77
                        let (index, value) = {
                            let w = cx.w::<ListControl>(me);
                            (w.sel_index, w.sel_value.clone())
                        };
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'item' (a nil value)")
                        });
                        ItemListControl::OnSelDelete(cx, me, index.unwrap_or(0), &value);
                    },
                ),
            );
            // L79-81
            cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
                // L80
                cx.w::<ListControl>(me).sel_value.is_some()
            });
            // L82 return self
        })
    }

    /// The "Del Unused" onClick (L31-48).
    fn delete_unused(cx: &mut Cx, me: CtrlId) {
        let h = items_hooks();
        let m = cx.build().model.clone();
        // L31
        let mut del_list: Vec<u32> = Vec::new();
        // L32 (a sequence, ORDER)
        for item_id in m.itemsTab.orderList.snapshot() {
            // L33
            let it = item(cx, item_id);
            let equipped = match &it {
                Some(it) => (h.get_equipped_slot_for_item)(cx, it).0.is_some(),
                None => lua_error!("attempt to index local 'item' (a nil value)"),
            };
            if !equipped
                && Self::FindEquippedAbyssJewel(cx, me, item_id, false).is_none()
                && Self::FindSocketedJewel(cx, me, item_id, false).is_none()
            {
                // L34
                del_list.push(item_id);
            }
        }
        // Delete in reverse order so as to not delete the wrong item whilst deleting
        // L38
        for id in del_list.iter().rev() {
            // L39
            let it = item_or_raise(cx, *id);
            (h.delete_item)(cx, &it, true);
        }
        // Rebuild cluster jewel graphs, populate slots, and create an undo state, as we deferred doing this during itemsTab:DeleteItem(...)
        // L42 (a sequence, ORDER)
        for spec in m.treeTab.specList.snapshot() {
            // L43
            spec.BuildClusterJewelGraphs();
        }
        // L45
        (h.populate_slots)(cx);
        // L46
        (h.add_undo_state)(cx);
        // L47
        m.buildFlag.set(true);
        // L48
        Self::UpdateList(cx, me);
    }

    /// The "Del All" confirm callback (L55-70).
    fn delete_all(cx: &mut Cx, me: CtrlId) {
        let h = items_hooks();
        let m = cx.build().model.clone();
        // L55 (ORDER (c))
        let slots: Vec<Table> = m.itemsTab.slots.with(|s| s.values().cloned().collect());
        for slot in slots {
            // L56
            set_sel_item_id(&m, &slot, 0.0);
        }
        // L58 (a sequence)
        for spec in m.treeTab.specList.snapshot() {
            // L59 (ORDER (c))
            for (node_id, _item_id) in spec.jewels.with(|j| j.pairs()) {
                // L60
                spec.jewels.with_mut(|j| j.set(node_id, Some(0.0)));
            }
        }
        // L63
        m.itemsTab.orderList.wipe();
        // L64
        m.itemsTab.items.with_mut(indexmap::IndexMap::clear);
        // L65
        (h.populate_slots)(cx);
        // L66
        (h.add_undo_state)(cx);
        // L67
        m.buildFlag.set(true);
        // L68-69
        let w = cx.w_mut::<ListControl>(me);
        w.sel_index = None;
        w.sel_value = None;
        // L70
        Self::UpdateList(cx, me);
    }

    /// lua: ItemListControl.lua:85-119 `ItemListClass:UpdateLoadoutList()`
    /// @pob Classes/ItemListControl.lua:85-119 h=a84085f8
    pub fn UpdateLoadoutList(cx: &mut Cx, me: CtrlId) -> bool {
        // L86
        let mut list: Vec<String> = vec![
            "Any Loadout".to_owned(),
            "Current Loadout".to_owned(),
            "Unused Items".to_owned(),
        ];
        // L87 (listValues: membership in `list`)
        // L88 local build = self.itemsTab.build
        // L89
        let build_node = cx.app().build.as_ref().map(|b| b.node.id());
        if let Some(loadouts) = build_node.and_then(|b| cx.child(b, "buildLoadouts")) {
            // L90
            let vals: Vec<String> = cx
                .w::<DropDownControl>(loadouts)
                .list
                .iter()
                .map(|v| v.label.clone())
                .collect();
            for val in vals {
                // L91
                if val != "^7^7Loadouts:"
                    && val != "^7^7-----"
                    && val != "^7^7New Loadout"
                    && val != "^7^7Sync"
                    && val != "^7^7Help >>"
                {
                    // L92
                    if !list.contains(&val) {
                        // L93-94
                        list.push(val);
                    }
                }
            }
        }
        // L99 (itemSetOrderList always exists)
        let m = cx.build().model.clone();
        // L100
        for item_set_id in m.itemsTab.itemSetOrderList.snapshot() {
            // L101
            let item_set = m.itemsTab.itemSets.with(|s| s.get(&item_set_id).cloned());
            // L102
            let title = item_set.as_ref().map(set_title);
            // L103
            if let Some(title) = title {
                if !list.contains(&title) {
                    // L104-105
                    list.push(title);
                }
            }
        }
        // L109
        let list_key = list.join("\0");
        // L110
        if cx.w::<ItemListControl>(me).loadout_list_key.as_deref() == Some(list_key.as_str()) {
            // L111
            return false;
        }
        // L113
        cx.w_mut::<ItemListControl>(me).loadout_list_key = Some(list_key);
        let filter = cx.child(me, "loadoutFilter").expect("loadoutFilter");
        // L114
        let sel_index = cx.w::<DropDownControl>(filter).sel_index;
        // L115
        let sel_value = cx
            .w::<DropDownControl>(filter)
            .list
            .get(sel_index.wrapping_sub(1))
            .map_or_else(|| "Any Loadout".to_owned(), |v| v.label.clone());
        // L116
        let items: Vec<DropItem> = list.iter().map(|s| DropItem::str(s.clone())).collect();
        DropDownControl::set_list(cx, filter, items);
        // L117
        cx.w_mut::<DropDownControl>(filter).sel_index = index_of(&list, &sel_value).unwrap_or(1);
        // L118
        true
    }

    /// lua: ItemListControl.lua:121-194 `ItemListClass:UpdateList()`
    /// @pob Classes/ItemListControl.lua:121-194 h=aec78da2
    pub fn UpdateList(cx: &mut Cx, me: CtrlId) {
        // L122
        Self::UpdateLoadoutList(cx, me);
        let filter = cx.child(me, "loadoutFilter").expect("loadoutFilter");
        // L123
        let sel_filter = cx.w::<DropDownControl>(filter).sel_index;
        // L124
        let filter_val = cx
            .w::<DropDownControl>(filter)
            .list
            .get(sel_filter.wrapping_sub(1))
            .map_or_else(|| "Any Loadout".to_owned(), |v| v.label.clone());
        // L125
        let selected_item_id = cx.w::<ListControl>(me).sel_value.as_ref().map(value_id);
        let m = cx.build().model.clone();

        // L127
        if sel_filter == 1 || filter_val == "Any Loadout" {
            // L128-129
            let w = cx.w_mut::<ListControl>(me);
            w.list = ListData::Source(Rc::new(IdListSource(m.itemsTab.orderList.clone())));
            w.is_mutable = true;
        } else {
            // L131
            cx.w_mut::<ListControl>(me).is_mutable = false;
            // L132-133
            let mut filter_item_set: Option<Table> = None;
            let mut filter_spec: Option<model::PassiveSpecRef> = None;
            let tree_tab = &m.treeTab;
            // L134
            if sel_filter == 2 || filter_val == "Current Loadout" {
                // L135
                filter_item_set = m.itemsTab.activeItemSet.get();
                // L136
                let active = tree_tab.activeSpec.get();
                filter_spec = if active >= 1.0 && active.fract() == 0.0 {
                    tree_tab.specList.get(active as usize)
                } else {
                    None
                };
            } else if sel_filter != 3 && filter_val != "Unused Items" {
                // L138
                let filter_title = pattern::gsub(&filter_val, "^%[[^%]]+%]%s*", "", None).0;
                // L139
                for item_set_id in m.itemsTab.itemSetOrderList.snapshot() {
                    // L140
                    let item_set = m
                        .itemsTab
                        .itemSets
                        .with(|s| s.get(&item_set_id).cloned())
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index local 'itemSet' (a nil value)")
                        });
                    // L141
                    if set_title(&item_set) == filter_title {
                        // L142
                        filter_item_set = Some(item_set);
                        // L143
                        break;
                    }
                }
                // L146 local treeTab = self.itemsTab.build.treeTab
                // L147
                for spec in tree_tab.specList.snapshot() {
                    // L148
                    if spec_title(&spec) == filter_title {
                        // L149
                        filter_spec = Some(spec);
                        // L150
                        break;
                    }
                }
                // L153
                let link_id = pob_ui_core::luastr::lmatch(&filter_val, "%{(%w+)%}");
                // L154-155
                let (item_link, tree_link) = match &link_id {
                    Some(id) => {
                        let b = cx.build().node.id();
                        let st = &cx.w::<BuildMode>(b).init;
                        (
                            st.item_list_special_links.links.get(&**id).copied(),
                            st.tree_list_special_links.links.get(&**id).copied(),
                        )
                    }
                    None => (None, None),
                };
                // L156
                if filter_item_set.is_none() {
                    let order = m.itemsTab.itemSetOrderList.snapshot();
                    let first = (order.len() == 1)
                        .then(|| m.itemsTab.itemSets.with(|s| s.get(&order[0]).cloned()))
                        .flatten();
                    filter_item_set = first.or_else(|| {
                        item_link.and_then(|set_id| {
                            model::items_tab::id_key(set_id)
                                .and_then(|k| m.itemsTab.itemSets.with(|s| s.get(&k).cloned()))
                        })
                    });
                }
                // L157
                if filter_spec.is_none() {
                    let first = (tree_tab.specList.len() == 1)
                        .then(|| tree_tab.specList.get(1))
                        .flatten();
                    filter_spec = first.or_else(|| {
                        tree_link.and_then(|set_id| {
                            (set_id >= 1.0 && set_id.fract() == 0.0)
                                .then(|| tree_tab.specList.get(set_id as usize))
                                .flatten()
                        })
                    });
                }
            }
            // L159
            let filter_item_set = filter_item_set.unwrap_or_default();
            // L160
            let mut new_list: Vec<ListValue> = Vec::new();
            let h = items_hooks();
            // L161
            for item_id in m.itemsTab.orderList.snapshot() {
                // L162
                let Some(it) = item(cx, item_id) else {
                    continue;
                };
                // L163
                // L164
                if sel_filter == 3 || filter_val == "Unused Items" {
                    // L165
                    if (h.get_equipped_slot_for_item)(cx, &it).0.is_none()
                        && Self::FindEquippedAbyssJewel(cx, me, item_id, false).is_none()
                        && Self::FindSocketedJewel(cx, me, item_id, false).is_none()
                    {
                        // L166
                        new_list.push(id_value(item_id));
                    }
                } else {
                    // L169
                    let mut in_loadout = false;
                    // L170 (ORDER (c))
                    for (_, slot) in filter_item_set.pairs() {
                        // L171
                        if let Val::Table(slot) = slot {
                            if slot
                                .get(n!("selItemId"))
                                .raw_eq(&Val::Num(f64::from(item_id)))
                            {
                                // L172
                                in_loadout = true;
                                // L173
                                break;
                            }
                        }
                    }
                    // L176
                    if !in_loadout {
                        if let Some(spec) = &filter_spec {
                            // L177 (ORDER (c))
                            for (node_id, jewel_id) in spec.jewels.with(|j| j.pairs()) {
                                // L178
                                if jewel_id == f64::from(item_id) && node_alloc(spec, node_id) {
                                    // L179
                                    in_loadout = true;
                                    // L180
                                    break;
                                }
                            }
                        }
                    }
                    // L184
                    if in_loadout {
                        // L185
                        new_list.push(id_value(item_id));
                    }
                }
            }
            // L190
            cx.w_mut::<ListControl>(me).list = ListData::Local(new_list);
        }
        // L192
        let sel_index = selected_item_id.and_then(|id| {
            (1..=ListControl::list_len(cx, me))
                .find(|i| ListControl::list_get(cx, me, *i).is_some_and(|v| value_id(&v) == id))
        });
        // L193
        let sel_value = sel_index.and_then(|i| ListControl::list_get(cx, me, i));
        let w = cx.w_mut::<ListControl>(me);
        w.sel_index = sel_index;
        w.sel_value = sel_value;
    }

    /// lua: ItemListControl.lua:206-227 `ItemListClass:FindSocketedJewel(jewelId,
    /// excludeActiveSpec)`
    /// @pob Classes/ItemListControl.lua:206-227 h=6139ad28
    pub fn FindSocketedJewel(
        cx: &Cx,
        _me: CtrlId,
        jewel_id: u32,
        exclude_active_spec: bool,
    ) -> Option<String> {
        // L207
        let jewel = item(cx, jewel_id)?;
        if &*jewel.r#type.get() != "Jewel" {
            // L208
            return None;
        }
        let m = &cx.build().model;
        // L210
        let tree_tab = &m.treeTab;
        // L211
        let mut equip_tree: Option<String> = None;
        // L212
        let mut match_active = false;
        // L213
        let specs = tree_tab.specList.snapshot();
        for spec_id in (1..=specs.len()).rev() {
            // L214
            let spec = &specs[spec_id - 1];
            // L215 (ORDER (c))
            for (node_id, item_id) in spec.jewels.with(|j| j.pairs()) {
                // L216
                if item_id == f64::from(jewel_id) && node_alloc(spec, node_id) {
                    // L217
                    if exclude_active_spec
                        && (spec_id as f64 == tree_tab.activeSpec.get() || match_active)
                    {
                        // L218
                        equip_tree = None;
                        // L219
                        match_active = true;
                    } else {
                        // L221
                        equip_tree = Some(spec_title(spec));
                    }
                }
            }
        }
        // L226
        equip_tree
    }

    /// lua: ItemListControl.lua:229-248 `ItemListClass:FindEquippedAbyssJewel(jewelId,
    /// excludeActiveSet)`
    /// @pob Classes/ItemListControl.lua:229-248 h=7cb2a2f9
    pub fn FindEquippedAbyssJewel(
        cx: &Cx,
        _me: CtrlId,
        jewel_id: u32,
        exclude_active_set: bool,
    ) -> Option<String> {
        // L230
        let jewel = item(cx, jewel_id)?;
        let base = jewel
            .base
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"));
        if base.subType.as_deref() != Some("Abyss") {
            // L231
            return None;
        }
        let m = &cx.build().model;
        // L233
        let mut equip_set: Option<String> = None;
        // L234
        let mut match_active = false;
        let active = m.itemsTab.activeItemSet.get();
        // L235 (ascending ids, ORDER (a))
        let mut sets: Vec<(u32, Table)> = m
            .itemsTab
            .itemSets
            .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
        sets.sort_by_key(|(k, _)| *k);
        for (_, item_set) in sets {
            // L236 (ORDER (c))
            for (_slot_name, slot) in item_set.pairs() {
                // L237
                if let Val::Table(slot) = slot {
                    if slot
                        .get(n!("selItemId"))
                        .raw_eq(&Val::Num(f64::from(jewel_id)))
                    {
                        // L238
                        if exclude_active_set
                            && (active.as_ref().is_some_and(|a| a.ptr_eq(&item_set))
                                || match_active)
                        {
                            // L239
                            equip_set = None;
                            // L240
                            match_active = true;
                        } else {
                            // L242
                            equip_set = Some(set_title(&item_set));
                        }
                    }
                }
            }
        }
        // L247
        equip_set
    }

    /// lua: ItemListControl.lua:250-266 `ItemListClass:GetRowValue(column, index, itemId)`
    /// @pob Classes/ItemListControl.lua:250-266 h=c20f217f
    fn GetRowValue(cx: &Cx, me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        let item_id = value_id(v);
        // L251
        let it = item(cx, item_id);
        // L252
        if column == 1 {
            // L253
            let mut used = Self::FindEquippedAbyssJewel(cx, me, item_id, true)
                .or_else(|| Self::FindSocketedJewel(cx, me, item_id, true))
                .unwrap_or_default();
            let it =
                it.unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
            // L254
            if used.is_empty() {
                // L255
                let (slot, item_set) = (items_hooks().get_equipped_slot_for_item)(cx, &it);
                // L256
                if slot.is_none() {
                    // L257
                    used = "  ^9(Unused)".to_owned();
                } else if let Some(item_set) = item_set {
                    // L259
                    used = format!("  ^9(Used in '{}')", set_title(&item_set));
                }
            } else {
                // L262
                used = format!("  ^9(Used in '{used}')");
            }
            // L264
            return format!("{}{used}", item_label(cx, &it));
        }
        String::new()
    }

    /// lua: ItemListControl.lua:268-277 `ItemListClass:AddValueTooltip(tooltip, index, itemId)`
    /// @pob Classes/ItemListControl.lua:268-277 h=888662b9
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        // L269
        if !cx.main.popups.is_empty() {
            // L270
            tooltip.clear(false);
            // L271
            return;
        }
        // L273
        let it = item(cx, value_id(v));
        // L274
        let keys = [
            it.clone().map_or(TtKey::Nil, |i| TtKey::Obj(TtObj(i))),
            TtKey::Bool(cx.sys.is_key_down(Key::Shift)),
            TtKey::Bool(cx.app().launch.dev_mode_alt),
            TtKey::Num(cx.build().model.outputRevision.get()),
        ];
        if tooltip.check_for_update(cx, &keys) {
            let it =
                it.unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
            // L275
            let f = items_hooks().add_item_tooltip;
            f(cx, tooltip, &it, None, false);
        }
    }

    /// lua: ItemListControl.lua:279-281 `ItemListClass:GetDragValue(index, itemId)`
    /// @pob Classes/ItemListControl.lua:279-281 h=f60eb3d1
    fn GetDragValue(
        cx: &Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
    ) -> Option<(Rc<str>, DragValue)> {
        // L280
        let it = item_or_raise(cx, value_id(v));
        Some((Rc::from("Item"), DragValue(it)))
    }

    /// lua: ItemListControl.lua:295-297 `ItemListClass:OnOrderChange()`
    /// @pob Classes/ItemListControl.lua:295-297 h=dc8da666
    fn OnOrderChange(cx: &mut Cx, _me: CtrlId, _sel_index: usize, _sel_drag_index: usize) {
        // L296
        let f = items_hooks().add_undo_state;
        f(cx);
    }

    /// lua: ItemListControl.lua:299-333 `ItemListClass:OnSelClick(index, itemId, doubleClick)`
    /// @pob Classes/ItemListControl.lua:299-333 h=9e26ecee
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        _index: usize,
        v: &ListValue,
        double_click: bool,
    ) -> Option<bool> {
        let h = items_hooks();
        let m = cx.build().model.clone();
        // L300
        let it = item_or_raise(cx, value_id(v));
        // L301
        if cx.sys.is_key_down(Key::Ctrl) {
            // L302
            let mut slot_name = it.GetPrimarySlot().to_string();
            // L303
            let slot_of = |name: &str| {
                m.itemsTab
                    .slots
                    .with(|s| s.get(&model::Name::from(name)).cloned())
            };
            if let Some(slot) = slot_of(&slot_name) {
                // L304
                let use_second = m
                    .itemsTab
                    .activeItemSet
                    .get()
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index field 'activeItemSet' (a nil value)")
                    })
                    .truthy(n!("useSecondWeaponSet"));
                if slot.get(n!("weaponSet")).raw_eq(&Val::Num(1.0)) && use_second {
                    // Redirect to second weapon set
                    // L306
                    slot_name = format!("{slot_name} Swap");
                }
                // L308
                if cx.sys.is_key_down(Key::Shift) {
                    // Redirect to second slot if possible
                    // L310
                    let alt_slot = pattern::gsub(&slot_name, "1", "2", None).0;
                    // L311
                    if m.itemsTab.IsItemValidForSlot(&m, &it, &alt_slot, None) {
                        // L312
                        slot_name = alt_slot;
                    }
                }
                // L315
                let slot = slot_of(&slot_name)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                let id = it.id.get().map_or(Val::Nil, Val::Num);
                if slot.get(n!("selItemId")).raw_eq(&id) {
                    // L316
                    set_sel_item_id(&m, &slot, 0.0);
                } else {
                    // L318
                    set_sel_item_id(&m, &slot, id.num());
                }
                // L320
                (h.populate_slots)(cx);
                // L321
                (h.add_undo_state)(cx);
                // L322
                m.buildFlag.set(true);
            }
        } else if double_click {
            // disallow dragging since if the cursor is outside the selection after
            // the second click, the item will be stuck onto the cursor
            // L327
            cx.w_mut::<ListControl>(me).sel_dragging = false;
            // L328
            let new_item = crate::main_mode::new_item(Some(&it.BuildRaw()), None, false);
            // L329
            new_item.id.set(it.id.get());
            // L330
            (h.set_display_item)(cx, Some(new_item));
            // L331
            return Some(false);
        }
        None
    }

    /// lua: ItemListControl.lua:335-338 `ItemListClass:OnSelCopy(index, itemId)`
    /// @pob Classes/ItemListControl.lua:335-338 h=93ff0a3f
    fn OnSelCopy(cx: &mut Cx, _me: CtrlId, _index: usize, v: &ListValue) {
        // L336
        let it = item_or_raise(cx, value_id(v));
        // L337 (Copy takes gsub's first result)
        let text = pattern::gsub(&it.BuildRaw(), "\n", "\r\n", None).0;
        cx.sys.platform.copy(&text);
    }

    /// The confirm/plain delete tail shared by the four branches (L346-349, L356-359, L365-368,
    /// L371-374).
    fn delete_and_refresh(cx: &mut Cx, me: CtrlId, it: &model::ItemRef) {
        let f = items_hooks().delete_item;
        // L346
        f(cx, it, false);
        // L347-348
        let w = cx.w_mut::<ListControl>(me);
        w.sel_index = None;
        w.sel_value = None;
        // L349
        Self::UpdateList(cx, me);
    }

    /// lua: ItemListControl.lua:340-378 `ItemListClass:OnSelDelete(index, itemId)`
    /// @pob Classes/ItemListControl.lua:340-378 h=252bfb71
    fn OnSelDelete(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
        let item_id = value_id(v);
        // L341
        let it = item_or_raise(cx, item_id);
        let name = it.name.get().to_string();
        // L342
        let (equip_slot, equip_set) = (items_hooks().get_equipped_slot_for_item)(cx, &it);
        // L343
        if let Some(equip_slot) = equip_slot {
            // L344
            let in_set =
                equip_set.map_or_else(String::new, |s| format!(" in set '{}'", set_title(&s)));
            let label = equip_slot.get(n!("label"));
            let Some(label) = label.str() else {
                lua_error!(
                    "attempt to concatenate field 'label' (a {} value)",
                    label.lua_type()
                )
            };
            // L345-350
            popups::open_confirm_popup(
                cx,
                "Delete Item",
                &format!("{name} is currently equipped in {label}{in_set}.\nAre you sure you want to delete it?"),
                "Delete",
                move |cx| Self::delete_and_refresh(cx, me, &it),
                None,
            );
        } else {
            // L352
            let equip_set = Self::FindEquippedAbyssJewel(cx, me, item_id, true);
            // L353
            if equip_set.is_some() {
                // L354 LUA-QUIRK(abyss-set-title): `("<title>").title` is nil
                let in_set = " in set 'Default'";
                // L355-360
                popups::open_confirm_popup(
                    cx,
                    "Delete Item",
                    &format!("{name} is currently equipped in an Abyssal Socket{in_set}.\nAre you sure you want to delete it?"),
                    "Delete",
                    move |cx| Self::delete_and_refresh(cx, me, &it),
                    None,
                );
            } else {
                // L362
                let equip_tree = Self::FindSocketedJewel(cx, me, item_id, true);
                // L363
                if let Some(equip_tree) = equip_tree {
                    // L364-369
                    popups::open_confirm_popup(
                        cx,
                        "Delete Item",
                        &format!("{name} is currently equipped in passive tree '{equip_tree}'.\nAre you sure you want to delete it?"),
                        "Delete",
                        move |cx| Self::delete_and_refresh(cx, me, &it),
                        None,
                    );
                } else {
                    // L371-374
                    Self::delete_and_refresh(cx, me, &it);
                }
            }
        }
    }
}

impl Widget for ItemListControl {
    const NAME: &'static str = "ItemListControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS
        .union(Caps::RECEIVE_DRAG)
        .union(Caps::HOVER_KEY_UP);

    /// lua: ItemListControl.lua:196-204 `ItemListClass:Draw(viewPort)`
    /// @pob Classes/ItemListControl.lua:196-204 h=c5ab8da7
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L197
        let loadout_list_changed = Self::UpdateLoadoutList(cx, me);
        // L198
        let output_revision = Some(cx.build().model.outputRevision.get());
        // L199
        if loadout_list_changed
            || output_revision != cx.w::<ItemListControl>(me).last_output_revision
        {
            // L200
            cx.w_mut::<ItemListControl>(me).last_output_revision = output_revision;
            // L201
            Self::UpdateList(cx, me);
        }
        // L203 self.ListControl.Draw(self, viewPort): noTooltip is nil
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

    /// lua: ItemListControl.lua:283-293 `ItemListClass:ReceiveDrag(type, value, source)`
    /// @pob Classes/ItemListControl.lua:283-293 h=c4b08de1
    fn receive_drag(cx: &mut Cx, me: CtrlId, ty: &str, v: &DragValue, _source: CtrlId) {
        // L284
        if ty == "Item" {
            let value = super::slot::drag_item(v);
            let h = items_hooks();
            // L285
            let new_item = crate::main_mode::new_item(Some(&value.raw.get()), None, false);
            // L286
            new_item.NormaliseQuality();
            // L287
            let index = cx.w::<ListControl>(me).sel_drag_index;
            (h.add_item)(cx, &new_item, true, index);
            // L288
            (h.add_forbidden_jewel_counterpart)(cx, &new_item);
            // L289
            (h.populate_slots)(cx);
            // L290
            (h.add_undo_state)(cx);
            // L291
            Self::UpdateList(cx, me);
        }
    }

    /// lua: ItemListControl.lua:380-388 `ItemListClass:OnHoverKeyUp(key)`
    /// @pob Classes/ItemListControl.lua:380-388 h=baca7a7a
    fn on_hover_key_up(cx: &mut Cx, me: CtrlId, key: Key) {
        // L381
        if wiki_matches_key(key) {
            // L382
            let item_id = ListControl::get_hover_value(cx, me);
            // L383
            if let Some(item_id) = item_id {
                // L384
                let it = item_or_raise(cx, value_id(&item_id));
                // L385
                wiki_open_item(cx, &it);
            }
        }
    }

    pob_ui_core::impl_parts!(ItemListControl: list => ListControl);
}
