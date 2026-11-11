//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/ItemSlotHelper.lua 1-40. Owner: U32.
//!
//! @pob-file Modules/ItemSlotHelper.lua @16de4b82d
//!
//! Two things live here:
//!
//! * [`DrawViewer`] (`itemSlotHelper.DrawViewer`, the jewel-socket position viewer that
//!   ItemSlotControl:Draw shows for a socket slot).
//! * [`ItemsTabHooks`]: the ItemsTab methods U32's controls call (`self.itemsTab:PopulateSlots()`,
//!   `:AddUndoState()`, `:AddItemTooltip(..)`, ...). The ItemsTab UI is U33-U38's and is not
//!   ported yet, so U32 reaches it through this table, as U19 reaches the tabs through
//!   `build::init::BuildHooks` (CCR-0098 §3, CCR-0107 §1). Each entry names its owner; an entry
//!   whose Lua is model code plus U32's own controls (PopulateSlots, AddItem, SetActiveItemSet,
//!   NewItemSet) has a complete default, the others raise a Lua error naming the owner (PoB's
//!   error prompt shows it). The owners replace their entry in [`default_items_hooks`].
//!
//! The ItemsTab object itself (`self.itemsTab`) is its node id (`build.tabs.items`); the model
//! half (`build.itemsTab`, calc:BM-A) is `cx.build().model.itemsTab`.
//!
//! DIVERGENCE: none. ORDER: no `pairs` loops (the PopulateSlots default iterates the model's
//! slots through calc:BM-A's `PopulateSlots`, then the slot controls in creation order).
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::m_min;
use pob_lua::n;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::slot::ItemSlotControl;
use crate::app::CxApp;
use crate::model;

// @pob Modules/ItemSlotHelper.lua:1-1 h=5ce3829c
// (`local M = {}`: the module is this file)

/// The PassiveTreeView fields DrawViewer sets before `viewer:Draw` (ItemSlotHelper.lua:18-23).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct SocketViewerArgs {
    /// `viewer.zoom`
    pub zoom: f64,
    /// `viewer.zoomX`
    pub zoom_x: f64,
    /// `viewer.zoomY`
    pub zoom_y: f64,
}

pub type ItemsHook = Rc<dyn Fn(&mut Cx)>;

/// The ItemsTab methods U32's controls call (see the module docs). `CtrlId` arguments that are
/// slots are the ItemSlotControl nodes; slots passed as Lua tables are the model's slot tables
/// (`build.itemsTab.slots[slotName]`, calc:BM-A).
#[derive(Clone)]
pub struct ItemsTabHooks {
    // ---- U34 (ItemsTab.lua:1216-1947) ----
    /// `itemsTab:PopulateSlots()` (L1652-1656): `slot:Populate()` for every slot
    pub populate_slots: ItemsHook,
    /// `itemsTab:AddItem(item, noAutoEquip, index)` (L1686-1724; all model, calc:BM-A)
    pub add_item: Rc<dyn Fn(&mut Cx, &model::ItemRef, bool, Option<usize>)>,
    /// `itemsTab:AddForbiddenJewelCounterpart(item)` (L1728-1771); returns the new item
    pub add_forbidden_jewel_counterpart:
        Rc<dyn Fn(&mut Cx, &model::ItemRef) -> Option<model::ItemRef>>,
    /// `itemsTab:SortItemList()` (L1820-1855)
    pub sort_item_list: ItemsHook,
    /// `itemsTab:DeleteItem(item, deferUndoState)` (L1857-1904)
    pub delete_item: Rc<dyn Fn(&mut Cx, &model::ItemRef, bool)>,
    /// `itemsTab:SetActiveItemSet(itemSetId)` (L1591-1617: model half + PopulateSlots +
    /// `build:SyncLoadouts()`)
    pub set_active_item_set: Rc<dyn Fn(&mut Cx, f64)>,
    /// `itemsTab:NewItemSet()` (L1573-1588, model; `itemSetId` nil)
    pub new_item_set: Rc<dyn Fn(&mut Cx) -> model::Table>,
    // ---- U35 (ItemsTab.lua:1948-2608) ----
    /// `itemsTab:CreateDisplayItemFromRaw(itemRaw, normalise)` (L1948-1958)
    pub create_display_item_from_raw: Rc<dyn Fn(&mut Cx, &str, bool)>,
    /// `itemsTab:SetDisplayItem(item)` (L2034-2115)
    pub set_display_item: Rc<dyn Fn(&mut Cx, Option<model::ItemRef>)>,
    /// `itemsTab:GetEquippedSlotForItem(item)` (L2528-2542): `slot, itemSet` (model tables)
    pub get_equipped_slot_for_item:
        Rc<dyn Fn(&Cx, &model::ItemRef) -> (Option<model::Table>, Option<model::Table>)>,
    // ---- U37 (ItemsTab.lua:3121-3992) ----
    /// `itemsTab:AddItemSetTooltip(tooltip, itemSet)` (L3982-3991)
    pub add_item_set_tooltip: Rc<dyn Fn(&mut Cx, &mut Tooltip, &model::Table)>,
    // ---- U38 (ItemsTab.lua:3993-5055, the UndoHandler state) ----
    /// `itemsTab:AddItemTooltip(tooltip, item, slot, dbMode)` (L4214-4662); `slot` is the
    /// ItemSlotControl's model table
    pub add_item_tooltip:
        Rc<dyn Fn(&mut Cx, &mut Tooltip, &model::ItemRef, Option<&model::Table>, bool)>,
    /// `itemsTab:AddUndoState()` (UndoHandler.lua:31-40 with ItemsTab:CreateUndoState,
    /// L5021-5036)
    pub add_undo_state: ItemsHook,
    /// `itemsTab.modFlag = true` (ItemSetListControl.lua:56/113, SharedItemSetListControl.lua:266)
    pub set_mod_flag: Rc<dyn Fn(&mut Cx, bool)>,
    // ---- U23 / U33 (`itemsTab.socketViewer`, a PassiveTreeView) ----
    /// `viewer.zoom = ..; viewer.zoomX = ..; viewer.zoomY = ..; viewer:Draw(itemsTab.build,
    /// viewPort, {})` (ItemSlotHelper.lua:18-29)
    pub socket_viewer_draw: Rc<dyn Fn(&mut Cx, SocketViewerArgs, &ViewPort)>,
}

#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The ItemsTab node (`self.itemsTab`), `None` while the ItemsTab UI is not constructed.
pub fn items_tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.items.id();
    cx.ui.is_alive(id).then_some(id)
}

/// The UI half of `itemsTab:PopulateSlots()` for every ItemSlotControl of the ItemsTab
/// (`t_insert(self.controls, slot)`, ItemsTab.lua:151): the dropdown lists from the model's
/// `slot.items` (see [`ItemSlotControl::populate_list`]).
pub fn populate_slot_lists(cx: &mut Cx) {
    let Some(tab) = items_tab(cx) else { return };
    populate_slot_lists_of(cx, tab);
}

/// [`populate_slot_lists`] for the ItemsTab node `tab` (its constructor runs before
/// `build.tabs.items` is set, Build.lua:612).
pub fn populate_slot_lists_of(cx: &mut Cx, tab: CtrlId) {
    for id in cx.children(tab) {
        if cx.is::<ItemSlotControl>(id) {
            ItemSlotControl::populate_list(cx, id);
        }
    }
}

/// The defaults (see the module docs).
pub fn default_items_hooks() -> ItemsTabHooks {
    ItemsTabHooks {
        // the model half (calc:BM-A `ItemsTab::PopulateSlots`: ItemSlotControl.lua:86-123 model
        // part for every slot), then the dropdown lists (ItemSlotControl.lua:92-104 UI part)
        populate_slots: Rc::new(|cx| {
            let m = cx.build().model.clone();
            m.itemsTab.PopulateSlots(&m);
            populate_slot_lists(cx);
        }),
        add_item: Rc::new(|cx, item, no_auto_equip, index| {
            let m = cx.build().model.clone();
            m.itemsTab.AddItem(&m, item, no_auto_equip, index);
        }),
        add_forbidden_jewel_counterpart: Rc::new(|cx, item| {
            super::core::AddForbiddenJewelCounterpart(cx, Some(item))
        }),
        sort_item_list: Rc::new(super::core::SortItemList),
        delete_item: Rc::new(super::core::DeleteItem),
        // L1592-1615 model (calc:BM-A), the UI half of its PopulateSlots, L1616 SyncLoadouts
        set_active_item_set: Rc::new(|cx, id| {
            let m = cx.build().model.clone();
            m.itemsTab.SetActiveItemSet(&m, id);
            populate_slot_lists(cx);
            let f = crate::build::init::hooks(cx).sync_loadouts;
            f(cx);
        }),
        new_item_set: Rc::new(|cx| cx.build().model.itemsTab.NewItemSet(None)),
        create_display_item_from_raw: Rc::new(|_, _, _| {
            not_ported("ItemsTab:CreateDisplayItemFromRaw", "U35")
        }),
        set_display_item: Rc::new(|_, _| not_ported("ItemsTab:SetDisplayItem", "U35")),
        get_equipped_slot_for_item: Rc::new(|_, _| {
            not_ported("ItemsTab:GetEquippedSlotForItem", "U35")
        }),
        add_item_set_tooltip: Rc::new(|_, _, _| not_ported("ItemsTab:AddItemSetTooltip", "U37")),
        add_item_tooltip: Rc::new(super::tooltip::add_item_tooltip_hook),
        add_undo_state: Rc::new(super::tooltip::add_undo_state_hook),
        set_mod_flag: Rc::new(super::tooltip::set_mod_flag_hook),
        socket_viewer_draw: Rc::new(|_, _, _| not_ported("PassiveTreeView:Draw", "U23")),
    }
}

thread_local! {
    static HOOKS: RefCell<Option<ItemsTabHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn items_hooks() -> ItemsTabHooks {
    HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_items_hooks)
            .clone()
    })
}

/// Replace hook entries (the owners, and tests).
pub fn set_items_hooks(f: impl FnOnce(&mut ItemsTabHooks)) {
    HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_items_hooks)));
}

/// lua: ItemSlotHelper.lua:3-38 `M.DrawViewer(itemsTab, nodeId, x, y, w, h)`
/// Draws a small display window of a jewel socket's position. Note that this will reset the
/// viewport and the draw layer.
/// @pob Modules/ItemSlotHelper.lua:3-38 h=d4bc904d
pub fn DrawViewer(cx: &mut Cx, node_id: f64, x: f64, y: f64, w: f64, h: f64) {
    let spec = cx.build().model.spec.get();
    // L11
    let node = if node_id.fract() == 0.0 {
        spec.nodes.with(|m| m.get(node_id as i64))
    } else {
        None
    };
    // L12
    let Some(node) = node else { return };
    // L13
    cx.sg.set_draw_layer(None, Some(15));
    // L14
    cx.sg.set_draw_color(1.0, 1.0, 1.0);

    // L16
    let border_width = 1.0;
    // L17
    cx.sg
        .draw_image(None, x, y, w + 2.0 * border_width, h + 2.0 * border_width);

    // L19 local viewer = itemsTab.socketViewer
    // L21 viewer.zoom = 17
    let zoom = 17.0;

    // L23
    let view_port_size = m_min(w, h);
    // L24
    let scale = spec.tree.get().extra.num(n!("size")) / (view_port_size * zoom);
    // L25
    let zoom_x = -node.x.get() / scale;
    // L26
    let zoom_y = -node.y.get() / scale;
    // offset viewport to be inside borders
    // L28
    cx.sg.set_viewport(x + border_width, y + border_width, w, h);
    // draw the actual image
    // L30
    let draw = items_hooks().socket_viewer_draw;
    draw(
        cx,
        SocketViewerArgs {
            zoom,
            zoom_x,
            zoom_y,
        },
        &ViewPort {
            x: 0.0,
            y: 0.0,
            width: w,
            height: h,
        },
    );
    // L31
    cx.sg.set_draw_layer(None, Some(30));
    // L32
    cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.2);
    // draw crosshair
    // L34
    cx.sg.draw_image(None, (w / 2.0).floor() - 1.0, 0.0, 2.0, h);
    // L35
    cx.sg.draw_image(None, 0.0, (h / 2.0).floor() - 1.0, w, 2.0);
    // L36
    cx.sg.reset_viewport();
    // L37
    cx.sg.set_draw_layer(None, Some(0));
}

// @pob Modules/ItemSlotHelper.lua:40-40 h=85c34a63
// (`return M`)
