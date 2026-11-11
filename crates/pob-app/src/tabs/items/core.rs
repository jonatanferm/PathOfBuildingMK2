//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua:1216-1947 (the model halves
//! of 1216-1727 are calc:BM-A). Owner: U34.
//!
//! @pob-file Classes/ItemsTab.lua @16de4b82d
//!
//! What this file ports (ui doc §10.2, §12.6 U34 row):
//! * [`ItemsTab::draw`]: `ItemsTabClass:Draw` (L1419-1570), the whole function: the scroll bars,
//!   the keyboard shortcuts, the input pass, the set list refresh, the display item tooltip and
//!   its mod toggles, `UpdateSockets`, the portrait re-anchoring, the controls and the spec list
//!   refresh.
//! * The UI halves of the model functions of calc:BM-A (each commented `// lua: … (model part:
//!   calc:BM-A <fn>)`): [`SetActiveItemSet`] (the slot lists and `build:SyncLoadouts()`),
//!   [`PopulateSlots`] (the slot lists), and `Load`'s `self:ResetUndo()` (L1326, in
//!   [`ctor_hook`]; DIVERGENCE(load-tail)). `NewItemSet`, `UpdateSockets`,
//!   `GetSocketAndJewelForNodeID`, `AddItem` and `Save` have no UI statements.
//! * [`EquipItemInSet`] (L1619-1649), re-ported whole: its UI statements (the SHIFT redirect,
//!   L1632-1637, and `AddUndoState`, L1647) sit between the model statements, so calc:BM-A's
//!   `EquipItemInSet` (which leaves them out) cannot be called for a half.
//! * The functions of L1726-1945 that no calc unit ported: [`AddForbiddenJewelCounterpart`],
//!   [`UpdateForbiddenJewelCounterpart`], [`AddDisplayItem`], [`SortItemList`], [`DeleteItem`],
//!   [`CopyAnointsAndEldritchImplicits`].
//!
//! The tab node (`self`) is U33's free node ([`super::ctor::tab`]); the shared fields live in
//! U33's [`super::ctor::State`] (CCR-0120 §2). The functions here are reached by the other units
//! through U32's [`ItemsTabHooks`](super::slot_helper::ItemsTabHooks) and U33's
//! [`ItemsUiHooks`](super::ctor::ItemsUiHooks) (this unit's entries in their defaults point here),
//! BuildMode's hooks (`BuildHooks::items_tab` / `items_set_active_item_set` /
//! `items_equip_item_in_set`, `ShellHooks::draw_tab`) and U44's `TabSeams::items_delete_item`.
//! Calls from here into units that are not ported go through [`CoreHooks`] (CCR-0126 §1):
//! the UndoHandler state of U38 (`Undo`, `Redo`, `ResetUndo`) and U35's
//! `ToggleDisplayItemModLine`. `Draw`'s last line (L1569) calls U19's
//! `BuildHooks::tree_get_spec_list` (`self.build.treeTab:GetSpecList()`, U25), whose default
//! raises until U25 lands (CCR-0126 §3).
//!
//! Lua semantics:
//! * `self.items[id]` is calc:BM-A's `itemsTab.items` (`u32` keys; a nil or non-integral id is
//!   nil); `self.itemOrderList` is its `orderList`; `self.slots[name]` is the model's slot table
//!   (ItemSlotControl's model half, CCR-0107 §6), `slot:SetSelItemId(..)` U32's
//!   [`set_sel_item_id`] on it.
//! * `self.slotOrder` and `self.showStatDifferences` are calc:BM-A's side-table fields
//!   (`build_extra(build).itemsTab`).
//! * `for id, event in ipairs(inputEvents)` stops at the first hole (`next_ipairs`);
//!   `inputEvents[id] = nil` is `consume`.
//! * `table.sort` is `pob_lua::sort` (LuaJIT's `auxsort`, the same comparisons and swaps).
//!
//! ORDER:
//! * L1858 `pairs(self.slots)` (DeleteItem; string keys): each slot and each item set entry is
//!   written independently (`SetSelItemId(0)` / `selItemId = 0`, `buildFlag = true`); a socket
//!   slot's `SetSelItemId` rebuilds the cluster graphs of the spec whatever the order. (c).
//! * L1864 `pairs(self.itemSets)`: independent writes. (c).
//! * L1872 `pairs(self.itemOrderList)` and L1878 `pairs(self.build.treeTab.specList)`:
//!   sequences: LuaJIT keeps them in the array part, so `pairs` is index order, as the port
//!   iterates.
//! * L1880 `pairs(spec.jewels)`: the model's `LjIntMap` (LuaJIT's order for the model's insertion
//!   sequence); every matching node is cleared, so the result does not depend on the order.
//! * L1738 / L1789 `pairs(self.items)` (the Forbidden jewel pair): an any-match (L1738) and
//!   independent per-item writes (L1789). (c).
//! * L1745 `pairs(main.uniqueDB.list)` (string keys): the first unique titled `otherTitle`; the
//!   unique DB has one entry per title for the two Forbidden jewels. (c).
//!
//! DIVERGENCE(load-tail): `ItemsTab:Load` ends with `self:ResetUndo()` (L1326, UI). calc:BM-A's
//! Load runs inside `Init_model`, before the UI constructors, so [`ctor_hook`] calls `ResetUndo`
//! right after constructing the tab when the build file has an `Items` section (as U39's CalcsTab
//! does). Between PoB's Load and that point only the deferred passive tree sections load
//! (Build.lua:655-677: "passive trees ... are loaded last"): their `SetActiveSpec` repopulates the
//! jewel socket slots from the loaded spec's `jewels`. During PoB's Load the spec was still the
//! TreeTab constructor's fresh one (no jewels), so every socket slot read `jewels[nodeId] or 0`
//! (ItemSlotControl.lua:87-88) = 0 when `CreateUndoState` copied `slotSelItemId`: undoing back to
//! the load state unequips every tree jewel in PoB. [`ctor_hook`] reproduces that state: it sets
//! the socket slots' `selItemId` to 0 around the `ResetUndo` call and puts the loaded values back
//! (CCR-0126 §4 proposes a load hook in calc:BM-A instead). The `SyncLoadouts` of Load's
//! `SetActiveItemSet` (L1616) is not replayed: Build.lua:726 calls it again before any frame
//! draws its lists (as CCR-0120 §6's ctor-sync-loadouts).
//! DIVERGENCE(sort-error): `SortItemList` sorts a copy of `itemOrderList` and writes it back; a
//! comparator that raises (a nil `slotOrder` of an equipped slot, an item id with no item) leaves
//! PoB's list partially sorted and the port's unchanged. Both then show the error prompt.
//!
//! Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::{n, pattern, Name};
use pob_sg::Key;
use pob_ui_core::controls::list::ListControl;
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{DropDownControl, DropItem};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{ModLineRef, Tooltip, TtModLine};

use super::ctor::{self, forbiddenJewelCounterpart, isAnointable};
use super::item_list::id_value;
use super::slot::{set_sel_item_id, ItemSlotControl};
use super::slot_helper::{items_hooks, populate_slot_lists};
use super::ItemsTab;
use crate::app::CxApp;
use crate::model::{self, build_xml, Table, Val};
// CCR-0120 §3: model.rs (U15) does not re-export ItemTools (`itemLib.influenceInfo.default`)
use pob_calc::classes::item_tools::influenceInfo_default;

/// This unit's part of the split class struct (ui doc §12.1). Every field the functions of
/// L1216-1947 read or write is either the model's (calc:BM-A) or one of the shared fields of
/// U33's [`super::ctor::State`] (`displayItem`, `maxY`, `snapHScroll`, the tooltip, the
/// anchors; CCR-0120 §2), so it has none of its own.
#[derive(Default)]
pub struct State {}

// ---- the methods of other units this file calls (CCR-0126 §1) --------------------------------

pub type CoreHook = Rc<dyn Fn(&mut Cx, CtrlId)>;

/// The ItemsTab methods of units that are not ported which this file calls and which neither
/// U32's `ItemsTabHooks` nor U33's `ItemsUiHooks` has. `CtrlId` is the ItemsTab node. The owners
/// replace their entry in [`default_core_hooks`] (one line each).
#[derive(Clone)]
pub struct CoreHooks {
    // ---- U38 (the UndoHandler state: CreateUndoState/RestoreUndoState, L5021-5055) ----
    /// `self:Undo()` (UndoHandler.lua:44-50)
    pub undo: CoreHook,
    /// `self:Redo()` (UndoHandler.lua:53-58)
    pub redo: CoreHook,
    /// `self:ResetUndo()` (UndoHandler.lua:25-29; Load's L1326)
    pub reset_undo: CoreHook,
    // ---- U35 (ItemsTab.lua:1948-2608) ----
    /// `self:ToggleDisplayItemModLine(line.modLine)` (L2123-2136)
    pub toggle_display_item_mod_line: Rc<dyn Fn(&mut Cx, CtrlId, &ModLineRef)>,
}

#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The defaults. `ResetUndo` runs on every build load (DIVERGENCE(load-tail)) and has nothing
/// to reset until U38's undo buffers exist, so it does nothing; the user-triggered ones raise a
/// Lua error naming the owner (PoB's error prompt shows it).
pub fn default_core_hooks() -> CoreHooks {
    CoreHooks {
        undo: Rc::new(|_, _| not_ported("ItemsTab:Undo (CreateUndoState/RestoreUndoState)", "U38")),
        redo: Rc::new(|_, _| not_ported("ItemsTab:Redo (CreateUndoState/RestoreUndoState)", "U38")),
        reset_undo: Rc::new(|_, _| {}),
        // U35: `line.modLine` is the `TtModLine` of AddItemTooltip (CCR-0073 B), whose `line` is
        // the mod line table
        toggle_display_item_mod_line: Rc::new(|cx, me, modLine| {
            let line = modLine
                .downcast_ref::<TtModLine>()
                .and_then(|l| l.line.downcast_ref::<Table>());
            super::display_item::ToggleDisplayItemModLine(cx, me, line);
        }),
    }
}

thread_local! {
    static CORE_HOOKS: RefCell<Option<CoreHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn core_hooks() -> CoreHooks {
    CORE_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_core_hooks)
            .clone()
    })
}

/// Replace hook entries (the owners, and tests).
pub fn set_core_hooks(f: impl FnOnce(&mut CoreHooks)) {
    CORE_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_core_hooks)));
}

// ---- helpers (Rust-only plumbing) ------------------------------------------------------------

/// `self.controls[key]` (a missing control raises when indexed, as in Lua).
#[track_caller]
fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `self.items[id]` (nil / non-integral id → nil).
fn item_at(m: &model::BuildRef, id: &Val) -> Option<model::ItemRef> {
    let k = model::items_tab::id_key(id.opt_num()?)?;
    m.itemsTab.items.with(|i| i.get(&k).cloned())
}

/// `self.slots[slotName]` (the model's slot table).
fn slot_table(m: &model::BuildRef, slot_name: &str) -> Option<Table> {
    m.itemsTab
        .slots
        .with(|s| s.get(&Name::from(slot_name)).cloned())
}

/// `self.itemSets[itemSetId]`
fn item_set(m: &model::BuildRef, id: f64) -> Option<Table> {
    let k = model::items_tab::id_key(id)?;
    m.itemsTab.itemSets.with(|s| s.get(&k).cloned())
}

/// `self.activeItemSet` (indexing nil raises).
#[track_caller]
fn active_item_set(m: &model::BuildRef) -> Table {
    m.itemsTab
        .activeItemSet
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
}

/// The model's side table of the tab (`self.slotOrder`, `self.showStatDifferences`).
fn tab_extra(m: &model::BuildRef) -> Table {
    build_xml::build_extra(m).get_or_new_table(n!("itemsTab"))
}

/// `self.slotOrder[slotName]` (nil → `None`).
fn slot_order(m: &model::BuildRef, slot_name: &str) -> Option<f64> {
    tab_extra(m)
        .table(n!("slotOrder"))
        .and_then(|t| t.get(slot_name).opt_num())
}

/// `item.variantList[item.variant]` (nil when either is nil or the index is out of range).
fn variant_name(item: &model::ItemRef) -> Option<model::LStr> {
    let list = item.variantList.get()?;
    let v = item.variant.get()?;
    if v.fract() != 0.0 || v < 1.0 {
        return None;
    }
    list.get(v as usize)
}

fn add_undo_state(cx: &mut Cx) {
    let f = items_hooks().add_undo_state;
    f(cx);
}

// ---- Draw ------------------------------------------------------------------------------------

impl ItemsTab {
    /// lua: ItemsTab.lua:1419-1570 `ItemsTabClass:Draw(viewPort, inputEvents)`
    /// @pob Classes/ItemsTab.lua:1419-1570 h=5d67df2f
    pub fn draw(cx: &mut Cx, me: CtrlId, viewPort: &ViewPort, inputEvents: &mut InputEvents) {
        let m = cx.build().model.clone();
        {
            let b = cx.base_mut(me);
            // L1420
            b.x = Prop::Val(viewPort.x);
            // L1421
            b.y = Prop::Val(viewPort.y);
            // L1422
            b.width = Prop::Val(viewPort.width);
            // L1423
            b.height = Prop::Val(viewPort.height);
        }
        let scrollBarH = ctl(cx, me, "scrollBarH");
        let scrollBarV = ctl(cx, me, "scrollBarV");
        {
            let b = cx.base_mut(scrollBarH);
            // L1424
            b.width = Prop::Val(viewPort.width);
            // L1425
            b.x = Prop::Val(viewPort.x);
            // L1426
            b.y = Prop::Val(viewPort.y + viewPort.height - 18.0);
            let b = cx.base_mut(scrollBarV);
            // L1427
            b.height = Prop::Val(viewPort.height - 18.0);
            // L1428
            b.x = Prop::Val(viewPort.x + viewPort.width - 18.0);
            // L1429
            b.y = Prop::Val(viewPort.y);
        }
        // L1430
        {
            // L1431
            let lastSlot = ctor::lastSlot(cx, me)
                .unwrap_or_else(|| lua_error!("attempt to index field 'lastSlot' (a nil value)"));
            let mut maxY = cx.pos(lastSlot).1 + 24.0;
            // L1432
            let anchorDisplayItem = cx.w::<ItemsTab>(me).ctor.anchorDisplayItem;
            let mut maxX = cx.pos(anchorDisplayItem).0 + 462.0;
            // L1433
            if cx.w::<ItemsTab>(me).ctor.displayItem.is_some() {
                // L1434
                let (x, y) = cx.pos(ctl(cx, me, "displayItemTooltipAnchor"));
                // L1435
                let (ttW, ttH) = cx
                    .w::<ItemsTab>(me)
                    .ctor
                    .displayItemTooltip
                    .get_dynamic_size(cx, viewPort);
                // L1436
                maxY = m_max(maxY, y + ttH + 4.0);
                // L1437
                maxX = m_max(maxX, x + ttW + 80.0);
            }
            // L1439
            let contentHeight = maxY - cx.base(me).y.raw();
            // L1440
            let contentWidth = maxX - cx.base(me).x.raw();
            // L1441
            let mut v = contentHeight > viewPort.height;
            // L1442
            let h = contentWidth > viewPort.width - if v { 20.0 } else { 0.0 };
            // L1443
            if h {
                // L1444
                v = contentHeight > viewPort.height - 20.0;
            }
            // L1446
            ScrollBarControl::set_content_dimension(
                cx,
                scrollBarV,
                contentHeight,
                viewPort.height - if h { 20.0 } else { 0.0 },
            );
            // L1447
            ScrollBarControl::set_content_dimension(
                cx,
                scrollBarH,
                contentWidth,
                viewPort.width - if v { 20.0 } else { 0.0 },
            );
            // L1448
            match cx.w::<ItemsTab>(me).ctor.snapHScroll {
                Some("RIGHT") => {
                    // L1449
                    let max = cx
                        .w::<ScrollBarControl>(scrollBarH)
                        .offset_max
                        .unwrap_or_else(|| lua_error!("bad argument #1 to 'SetOffset'"));
                    ScrollBarControl::set_offset(cx, scrollBarH, max);
                }
                // L1450
                Some("LEFT") => {
                    // L1451
                    ScrollBarControl::set_offset(cx, scrollBarH, 0.0);
                }
                _ => {}
            }
            // L1453
            cx.w_mut::<ItemsTab>(me).ctor.snapHScroll = None;
            // L1454
            let maxY = if h {
                cx.base(scrollBarH).y.raw()
            } else {
                viewPort.y + viewPort.height
            };
            cx.w_mut::<ItemsTab>(me).ctor.maxY = Some(maxY);
        }
        // L1456
        let x = cx.base(me).x.raw() - cx.w::<ScrollBarControl>(scrollBarH).offset;
        cx.base_mut(me).x = Prop::Val(x);
        // L1457
        let y = cx.base(me).y.raw() - cx.w::<ScrollBarControl>(scrollBarV).offset;
        cx.base_mut(me).y = Prop::Val(y);

        // L1459
        let mut i = 0;
        while let Some(event) = inputEvents.next_ipairs(&mut i) {
            // L1460
            if let EventKind::KeyDown { .. } = event.kind {
                // L1461
                if event.key == Key::Char(b'v') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1462
                    let newItem = cx.sys.platform.paste();
                    // L1463
                    if let Some(newItem) = newItem {
                        // L1464
                        let f = items_hooks().create_display_item_from_raw;
                        f(cx, &newItem, true);
                    }
                // L1466
                } else if event.key == Key::Char(b'e') {
                    // L1467
                    let mOverControl = cx.get_mouse_over_control(me);
                    // L1468
                    let sel = mOverControl.and_then(|c| {
                        cx.try_w::<ItemSlotControl>(c)
                            .map(|w| w.slot.get(n!("selItemId")))
                    });
                    if let Some(selItemId) = sel.filter(|s| !s.raw_eq(&Val::Num(0.0))) {
                        // Trigger itemList's double click procedure
                        // L1470
                        let itemList = ctl(cx, me, "itemList");
                        on_sel_click(cx, itemList, &selItemId);
                    }
                // L1472
                } else if event.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1473
                    let f = core_hooks().undo;
                    f(cx, me);
                    // L1474
                    m.buildFlag.set(true);
                // L1475
                } else if event.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1476
                    let f = core_hooks().redo;
                    f(cx, me);
                    // L1477
                    m.buildFlag.set(true);
                // L1478
                } else if event.key == Key::Char(b'f') && cx.sys.is_key_down(Key::Ctrl) {
                    let uniqueDB = ctl(cx, me, "uniqueDB");
                    let rareDB = ctl(cx, me, "rareDB");
                    let selectDB = ctl(cx, me, "selectDB");
                    let uniqueSearch = cx.child(uniqueDB, "search");
                    let rareSearch = cx.child(rareDB, "search");
                    let selControl = cx.host(me).sel_control;
                    // L1479
                    let selUnique = selControl == uniqueSearch;
                    // L1480
                    let selRare = selControl == rareSearch;
                    // L1481
                    if selUnique
                        || (cx.is_shown(selectDB)
                            && !selRare
                            && cx.w::<DropDownControl>(selectDB).sel_index == 2)
                    {
                        // L1482
                        cx.select_control(me, rareSearch);
                        // L1483
                        cx.w_mut::<DropDownControl>(selectDB).sel_index = 2;
                    } else {
                        // L1485
                        cx.select_control(me, uniqueSearch);
                        // L1486
                        cx.w_mut::<DropDownControl>(selectDB).sel_index = 1;
                    }
                // L1488
                } else if event.key == Key::Char(b'd') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1489
                    let v = m.itemsTab.showStatDifferences(&m);
                    tab_extra(&m).set(n!("showStatDifferences"), !v);
                    // L1490
                    m.buildFlag.set(true);
                }
            }
        }
        // L1494
        cx.process_controls_input(me, inputEvents, viewPort);
        // L1495
        let mut i = 0;
        while let Some(event) = inputEvents.next_ipairs(&mut i) {
            // L1496
            if event.kind == EventKind::KeyUp {
                // L1497
                if ScrollBarControl::is_scroll_down_key(cx, scrollBarV, event.key) {
                    // L1498
                    if cx.is_mouse_over(scrollBarV) || !cx.is_shown(scrollBarH) {
                        // L1499
                        ScrollBarControl::scroll(cx, scrollBarV, 1.0);
                    } else {
                        // L1501
                        ScrollBarControl::scroll(cx, scrollBarH, 1.0);
                    }
                // L1503
                } else if ScrollBarControl::is_scroll_up_key(cx, scrollBarV, event.key) {
                    // L1504
                    if cx.is_mouse_over(scrollBarV) || !cx.is_shown(scrollBarH) {
                        // L1505
                        ScrollBarControl::scroll(cx, scrollBarV, -1.0);
                    } else {
                        // L1507
                        ScrollBarControl::scroll(cx, scrollBarH, -1.0);
                    }
                }
            }
        }

        // L1513
        crate::main_mode::background::draw_background(cx, viewPort);

        // L1515
        let mut newItemList = Vec::new();
        let setSelect = ctl(cx, me, "setSelect");
        // L1516
        for (index, itemSetId) in m
            .itemsTab
            .itemSetOrderList
            .snapshot()
            .into_iter()
            .enumerate()
        {
            // L1517
            let itemSet = item_set(&m, f64::from(itemSetId))
                .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
            // L1518
            newItemList.push(DropItem::str(
                itemSet
                    .str(n!("title"))
                    .map_or_else(|| "Default".to_owned(), |t| t.to_string()),
            ));
            // L1519
            if f64::from(itemSetId) == m.itemsTab.activeItemSetId.get() {
                // L1520
                cx.w_mut::<DropDownControl>(setSelect).sel_index = index + 1;
            }
        }
        // L1523
        DropDownControl::set_list(cx, setSelect, newItemList);

        // L1525
        if cx.w::<ItemsTab>(me).ctor.displayItem.is_some() {
            // L1526
            let (x, y) = cx.pos(ctl(cx, me, "displayItemTooltipAnchor"));
            // L1527
            cx.lend::<ItemsTab, Tooltip, _>(
                me,
                |w| &mut w.ctor.displayItemTooltip,
                |cx, tt| tt.draw(cx, x, y, None, None, viewPort),
            );

            // Toggle mods
            // L1530
            let (cursorX, cursorY) = cx.sys.cursor_pos();
            // L1531 (the lines are read live: a toggle rebuilds the tooltip's `lines` table in
            // place, Tooltip:Clear's wipeTable, so the loop goes on over the new lines)
            let mut li = 0;
            loop {
                let line = cx
                    .w::<ItemsTab>(me)
                    .ctor
                    .displayItemTooltip
                    .lines
                    .get(li)
                    .map(|l| (l.mod_line.clone(), l.bounds.get()));
                let Some((modLine, bounds)) = line else { break };
                li += 1;
                // L1532
                let (Some(modLine), Some(b)) = (modLine, bounds) else {
                    continue;
                };
                // L1533 local b = line.bounds
                // L1534
                if cursorX >= b.x
                    && cursorX <= b.x + b.width
                    && cursorY >= b.y
                    && cursorY <= b.y + b.height
                {
                    // L1535
                    cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.15);
                    // L1536
                    cx.sg.draw_image(None, b.x, b.y, b.width, b.height);
                    // L1537
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);

                    // L1539
                    let mut id = 0;
                    while let Some(event) = inputEvents.next_ipairs(&mut id) {
                        // L1540
                        if matches!(event.kind, EventKind::KeyDown { .. }) && event.key.is_button()
                        {
                            // L1541
                            inputEvents.consume(id);
                            // L1542
                            let f = core_hooks().toggle_display_item_mod_line;
                            f(cx, me, &modLine);
                            // L1543
                            break;
                        }
                    }
                }
            }
        }

        // L1551 (model part: calc:BM-A UpdateSockets; the slot controls read the model's
        // `inactive`/`label`)
        m.itemsTab.UpdateSockets(&m);

        let itemList = ctl(cx, me, "itemList");
        let setManage = ctl(cx, me, "setManage");
        let portrait = cx.app().main.portrait_mode;
        // L1553
        if portrait {
            // L1554
            let lastSlot = ctor::lastSlot(cx, me);
            cx.set_anchor(
                itemList,
                TopRight,
                lastSlot,
                BottomRight,
                Some((Prop::Val(0.0), Prop::Val(40.0))),
                false,
            );
        } else {
            // L1556
            cx.set_anchor(
                itemList,
                TopLeft,
                Some(setManage),
                TopRight,
                Some((Prop::Val(20.0), Prop::Val(20.0))),
                false,
            );
        }
        // L1558
        let to = if portrait { setManage } else { itemList };
        let craftDisplayItem = ctl(cx, me, "craftDisplayItem");
        cx.set_anchor(
            craftDisplayItem,
            TopLeft,
            Some(to),
            TopRight,
            Some((
                Prop::Val(20.0),
                Prop::Val(if portrait { 0.0 } else { -20.0 }),
            )),
            false,
        );
        // L1559 `main.portraitMode and 0` is `false` (not a number) when not portrait: SetAnchor
        // keeps the offsets only when both are given, so the anchor keeps its own x/y
        let anchorDisplayItem = cx.w::<ItemsTab>(me).ctor.anchorDisplayItem;
        if portrait {
            cx.set_anchor(
                anchorDisplayItem,
                TopLeft,
                Some(to),
                TopRight,
                Some((Prop::Val(20.0), Prop::Val(0.0))),
                false,
            );
        } else {
            cx.set_anchor(anchorDisplayItem, TopLeft, Some(to), TopRight, None, false);
        }

        // L1561
        cx.draw_controls(me, viewPort, None);
        // L1562
        if cx.is_shown(scrollBarH) {
            // L1563
            cx.draw(scrollBarH, viewPort, false);
        }
        // L1565
        if cx.is_shown(scrollBarV) {
            // L1566
            cx.draw(scrollBarV, viewPort, false);
        }

        // L1569
        let f = crate::build::init::hooks(cx).tree_get_spec_list;
        let list: Vec<DropItem> = f(cx).into_iter().map(DropItem::str).collect();
        let specSelect = ctl(cx, me, "specSelect");
        DropDownControl::set_list(cx, specSelect, list);
    }
}

/// `self.controls.itemList:OnSelClick(0, selItemId, true)` (L1470): the list's instance
/// override, else its class hook (ItemListControl.lua:299-333).
fn on_sel_click(cx: &mut Cx, itemList: CtrlId, selItemId: &Val) {
    let (ov, class) = {
        let w = cx.w::<ListControl>(itemList);
        (w.ov.on_sel_click.clone(), w.hooks.on_sel_click)
    };
    // the list's row values are item ids (ItemListControl's `id_value`)
    let id = selItemId
        .opt_num()
        .and_then(model::items_tab::id_key)
        .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
    let value = id_value(id);
    if let Some(f) = ov {
        f(cx, itemList, 0, &value, true);
    } else if let Some(f) = class {
        f(cx, itemList, 0, &value, true);
    } else {
        lua_error!("attempt to call method 'OnSelClick' (a nil value)");
    }
}

// ---- item sets and slots ---------------------------------------------------------------------

/// lua: ItemsTab.lua:1591-1617 `ItemsTabClass:SetActiveItemSet(itemSetId)` (model part: calc:BM-A
/// `ItemsTab::SetActiveItemSet`, L1592-1615; its `PopulateSlots` (L1615) is the model's, whose
/// UI half, the slot controls' lists, follows here).
pub fn SetActiveItemSet(cx: &mut Cx, itemSetId: f64) {
    let m = cx.build().model.clone();
    m.itemsTab.SetActiveItemSet(&m, itemSetId);
    // L1615 (UI part of PopulateSlots)
    populate_slot_lists(cx);
    // L1616
    let f = crate::build::init::hooks(cx).sync_loadouts;
    f(cx);
}

/// lua: ItemsTab.lua:1652-1656 `ItemsTabClass:PopulateSlots()` (model part: calc:BM-A
/// `ItemsTab::PopulateSlots`; the UI half is every slot control's list, U32's
/// `populate_slot_lists`; DIVERGENCE(populate-split) in `slot.rs`).
pub fn PopulateSlots(cx: &mut Cx) {
    let m = cx.build().model.clone();
    m.itemsTab.PopulateSlots(&m);
    populate_slot_lists(cx);
}

/// lua: ItemsTab.lua:1619-1649 `ItemsTabClass:EquipItemInSet(item, itemSetId)` (calc:BM-A's
/// `ItemsTab::EquipItemInSet` leaves out L1632-1637 and L1647; see the module docs)
/// @pob Classes/ItemsTab.lua:1619-1649 h=06719695
pub fn EquipItemInSet(cx: &mut Cx, item: &model::ItemRef, itemSetId: f64) {
    let m = cx.build().model.clone();
    let mut item = item.clone();
    // L1621
    let itemSet = item_set(&m, itemSetId);
    // L1622
    let mut slotName = item.GetPrimarySlot().to_string();
    // L1623
    let slot = slot_table(&m, &slotName)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let itemSet =
        itemSet.unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
    if slot.get(n!("weaponSet")).raw_eq(&Val::Num(1.0)) && itemSet.truthy(n!("useSecondWeaponSet"))
    {
        // Redirect to second weapon set
        // L1625
        slotName = format!("{slotName} Swap");
    }
    // L1627
    let known = item_at(&m, &item.id.get().map_or(Val::Nil, Val::Num)).is_some();
    if item.id.get().is_none() || !known {
        // L1628
        item = crate::main_mode::new_item(Some(&item.raw.get()), None, false);
        // L1629
        let f = items_hooks().add_item;
        f(cx, &item, true, None);
    }
    // L1631
    let altSlot = pattern::gsub(&slotName, "1", "2", None).0;
    // L1632
    if cx.sys.is_key_down(Key::Shift) {
        // Redirect to second slot if possible
        // L1634
        if m.itemsTab
            .IsItemValidForSlot(&m, &item, &altSlot, Some(&itemSet))
        {
            // L1635
            slotName = altSlot.clone();
        }
    }
    let itemId = item.id.get().map_or(Val::Nil, Val::Num);
    // L1638
    if m.itemsTab
        .activeItemSet
        .get()
        .is_some_and(|s| s.ptr_eq(&itemSet))
    {
        // L1639
        let slot = slot_table(&m, &slotName)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        set_sel_item_id(&m, &slot, itemId.num());
    } else {
        // L1641
        itemSet
            .table(&*slotName)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .set(n!("selItemId"), itemId);
        // L1642
        let alt = itemSet
            .table(&*altSlot)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let altSel = alt.get(n!("selItemId"));
        if !altSel.raw_eq(&Val::Num(0.0)) {
            let altItem = item_at(&m, &altSel)
                .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
            if !m
                .itemsTab
                .IsItemValidForSlot(&m, &altItem, &altSlot, Some(&itemSet))
            {
                // L1643
                alt.set(n!("selItemId"), 0.0);
            }
        }
    }
    // L1646
    PopulateSlots(cx);
    // L1647
    add_undo_state(cx);
    // L1648
    m.buildFlag.set(true);
}

// ---- the Forbidden jewel pair ----------------------------------------------------------------

/// lua: ItemsTab.lua:1728-1771 `ItemsTabClass:AddForbiddenJewelCounterpart(item)`
/// Given one half of a Forbidden Flame/Flesh pair, adds the other half with the same notable
/// Both jewels are generated from the same sorted class/notable list
/// @pob Classes/ItemsTab.lua:1726-1771 h=a426d630
pub fn AddForbiddenJewelCounterpart(
    cx: &mut Cx,
    item: Option<&model::ItemRef>,
) -> Option<model::ItemRef> {
    // L1729
    let otherTitle = item
        .and_then(|i| i.title.get())
        .and_then(|t| forbiddenJewelCounterpart(&t));
    // L1730
    let item = item?;
    let otherTitle = otherTitle?;
    if cx.app().main.unique_db.loading
        || item.variantList.get().is_none()
        || item.variant.get().is_none()
    {
        // L1731
        return None;
    }
    // L1733
    let variantName = variant_name(item);
    // L1734
    let Some(variantName) = variantName else {
        // L1735
        return None;
    };

    let m = cx.build().model.clone();
    // L1738 ORDER: (c), see the module docs
    for other in m
        .itemsTab
        .items
        .with(|i| i.values().cloned().collect::<Vec<_>>())
    {
        // L1739
        if other.title.get().as_deref() == Some(otherTitle)
            && other.variantList.get().is_some()
            && variant_name(&other).as_ref() == Some(&variantName)
        {
            // L1740
            return None;
        }
    }

    // L1744
    let mut dbItem = None;
    // L1745 ORDER: (c), see the module docs
    for unique in cx.app().main.unique_db.list.values() {
        // L1746
        if unique.title.get().as_deref() == Some(otherTitle) {
            // L1747
            dbItem = Some(unique.clone());
            // L1748
            break;
        }
    }
    // L1751
    let dbItem = dbItem?;

    // L1755
    let newItem = crate::main_mode::new_item(Some(&dbItem.raw.get()), None, false);
    // L1756
    let mut variantIndex = None;
    // L1757
    for (index, name) in newItem
        .variantList
        .get()
        .map(|l| l.snapshot())
        .unwrap_or_default()
        .into_iter()
        .enumerate()
    {
        // L1758
        if name == variantName {
            // L1759
            variantIndex = Some(index + 1);
            // L1760
            break;
        }
    }
    // L1763
    let variantIndex = variantIndex?;
    // L1766
    newItem.variant.set(Some(variantIndex as f64));
    // L1767
    newItem.BuildAndParseRaw();
    // L1768
    newItem.NormaliseQuality();
    // L1769
    let f = items_hooks().add_item;
    f(cx, &newItem, true, None);
    // L1770
    Some(newItem)
}

/// lua: ItemsTab.lua:1774-1801 `ItemsTabClass:UpdateForbiddenJewelCounterpart(oldItem, item)`
/// Keeps the Forbidden jewels in sync: manually editing one jewel moves the other counterpart to
/// the same notable
/// @pob Classes/ItemsTab.lua:1773-1801 h=7fee005c
pub fn UpdateForbiddenJewelCounterpart(
    cx: &mut Cx,
    oldItem: Option<&model::ItemRef>,
    item: Option<&model::ItemRef>,
) {
    // L1775
    let otherTitle = item
        .and_then(|i| i.title.get())
        .and_then(|t| forbiddenJewelCounterpart(&t));
    // L1776
    let (Some(otherTitle), Some(oldItem), Some(item)) = (otherTitle, oldItem, item) else {
        // L1777
        return;
    };
    if oldItem.title.get() != item.title.get() {
        return;
    }
    // L1779
    if !(item.variantList.get().is_some()
        && item.variant.get().is_some()
        && oldItem.variantList.get().is_some()
        && oldItem.variant.get().is_some())
    {
        // L1780
        return;
    }
    // L1782
    let oldName = variant_name(oldItem);
    // L1783
    let newName = variant_name(item);
    // L1784
    let (Some(oldName), Some(newName)) = (oldName, newName) else {
        // L1785
        return;
    };
    if oldName == newName {
        return;
    }

    let m = cx.build().model.clone();
    // L1788 ORDER: (c), see the module docs
    for other in m
        .itemsTab
        .items
        .with(|i| i.values().cloned().collect::<Vec<_>>())
    {
        // L1789
        if other.id.get() != item.id.get()
            && other.title.get().as_deref() == Some(otherTitle)
            && other.variantList.get().is_some()
            && variant_name(&other).as_ref() == Some(&oldName)
        {
            // L1790
            let list = other
                .variantList
                .get()
                .map(|l| l.snapshot())
                .unwrap_or_default();
            for (index, name) in list.into_iter().enumerate() {
                // L1791
                if name == newName {
                    // L1792
                    other.variant.set(Some((index + 1) as f64));
                    // L1793
                    other.BuildAndParseRaw();
                    // L1794
                    other.BuildModList();
                    // L1795
                    m.buildFlag.set(true);
                    // L1796
                    break;
                }
            }
        }
    }
}

/// lua: ItemsTab.lua:1804-1817 `ItemsTabClass:AddDisplayItem(noAutoEquip)`
/// Adds the current display item to the build's item list
/// @pob Classes/ItemsTab.lua:1803-1817 h=2b194087
pub fn AddDisplayItem(cx: &mut Cx, me: CtrlId, noAutoEquip: bool) {
    let m = cx.build().model.clone();
    let h = items_hooks();
    // L1805
    let item = cx.w::<ItemsTab>(me).ctor.displayItem.clone();
    // L1806
    let oldItem = item
        .as_ref()
        .and_then(|i| i.id.get())
        .and_then(|id| item_at(&m, &Val::Num(id)));
    // Add it to the list and clear the current display item
    // L1808
    let added = item
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
    (h.add_item)(cx, &added, noAutoEquip, None);
    // L1809
    (h.set_display_item)(cx, None);

    // L1811
    UpdateForbiddenJewelCounterpart(cx, oldItem.as_ref(), item.as_ref());
    // L1812
    AddForbiddenJewelCounterpart(cx, item.as_ref());

    // L1814
    (h.populate_slots)(cx);
    // L1815
    (h.add_undo_state)(cx);
    // L1816
    m.buildFlag.set(true);
}

// ---- the item list ---------------------------------------------------------------------------

/// lua: ItemsTab.lua:1820-1854 `ItemsTabClass:SortItemList()`
/// Sorts the build's item list
/// @pob Classes/ItemsTab.lua:1819-1854 h=849ac24c
pub fn SortItemList(cx: &mut Cx) {
    let m = cx.build().model.clone();
    let get_equipped = items_hooks().get_equipped_slot_for_item;
    let order: Vec<f64> = m
        .itemsTab
        .itemSetOrderList
        .snapshot()
        .into_iter()
        .map(f64::from)
        .collect();
    // L1821 (DIVERGENCE(sort-error): a copy, written back)
    let mut list = m.itemsTab.orderList.snapshot();
    {
        let cx: &Cx = cx;
        let item = |id: u32| {
            m.itemsTab
                .items
                .with(|i| i.get(&id).cloned())
                .unwrap_or_else(|| lua_error!("attempt to index local 'itemA' (a nil value)"))
        };
        let slot_order_of = |name: &str| -> f64 {
            slot_order(&m, name).unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
        };
        pob_lua::sort(&mut list, |a, b| {
            // L1822
            let itemA = item(*a);
            // L1823
            let itemB = item(*b);
            // L1824
            let primSlotA = itemA.GetPrimarySlot();
            // L1825
            let primSlotB = itemB.GetPrimarySlot();
            // L1826
            if primSlotA != primSlotB {
                // L1827
                if slot_order(&m, &primSlotA).is_none() {
                    // L1828
                    return false;
                // L1829
                } else if slot_order(&m, &primSlotB).is_none() {
                    // L1830
                    return true;
                }
                // L1832
                return slot_order_of(&primSlotA) < slot_order_of(&primSlotB);
            }
            // L1834
            let (equipSlotA, equipSetA) = get_equipped(cx, &itemA);
            // L1835
            let (equipSlotB, equipSetB) = get_equipped(cx, &itemB);
            // L1836
            match (&equipSlotA, &equipSlotB) {
                (Some(sa), Some(sb)) => {
                    // L1837
                    if !sa.ptr_eq(sb) {
                        // L1838
                        let na = sa.str(n!("slotName")).unwrap_or_default();
                        let nb = sb.str(n!("slotName")).unwrap_or_default();
                        return slot_order_of(&na) < slot_order_of(&nb);
                    }
                    match (&equipSetA, &equipSetB) {
                        // L1839-1840
                        (Some(_), None) => return false,
                        // L1841-1842
                        (None, Some(_)) => return true,
                        // L1843
                        (Some(ea), Some(eb)) => {
                            // L1844
                            let ia = pob_calc::modules::common::isValueInArray(
                                &order,
                                &ea.get(n!("id")).opt_num().unwrap_or(f64::NAN),
                            );
                            let ib = pob_calc::modules::common::isValueInArray(
                                &order,
                                &eb.get(n!("id")).opt_num().unwrap_or(f64::NAN),
                            );
                            let (Some(ia), Some(ib)) = (ia, ib) else {
                                lua_error!("attempt to compare nil with number")
                            };
                            return ia < ib;
                        }
                        (None, None) => {}
                    }
                }
                // L1846-1847
                (Some(_), None) => return true,
                // L1848-1849
                (None, Some(_)) => return false,
                (None, None) => {}
            }
            // L1851
            itemA.name.get() < itemB.name.get()
        });
    }
    m.itemsTab.orderList.with_mut(|l| *l = list);
    // L1853
    add_undo_state(cx);
}

/// lua: ItemsTab.lua:1857-1904 `ItemsTabClass:DeleteItem(item, deferUndoState)`
/// Deletes an item
/// @pob Classes/ItemsTab.lua:1856-1904 h=a95fdf85
pub fn DeleteItem(cx: &mut Cx, item: &model::ItemRef, deferUndoState: bool) {
    let m = cx.build().model.clone();
    let id = item.id.get();
    let id_val = id.map_or(Val::Nil, Val::Num);
    // L1858 ORDER: (c), see the module docs
    let slots: Vec<(Name, Table)> = m
        .itemsTab
        .slots
        .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
    for (slotName, slot) in slots {
        // L1859
        if slot.get(n!("selItemId")).raw_eq(&id_val) {
            // L1860
            set_sel_item_id(&m, &slot, 0.0);
            // L1861
            m.buildFlag.set(true);
        }
        // L1863
        if slot.get(n!("nodeId")).is_nil() {
            // L1864 ORDER: (c), see the module docs
            let sets: Vec<Table> = m.itemsTab.itemSets.with(|s| s.values().cloned().collect());
            for itemSet in sets {
                let entry = itemSet
                    .table(slotName)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                // L1865
                if entry.get(n!("selItemId")).raw_eq(&id_val) {
                    // L1866
                    entry.set(n!("selItemId"), 0.0);
                    // L1867
                    m.buildFlag.set(true);
                }
            }
        }
    }
    // L1872 (a sequence: index order)
    let pos = id.and_then(model::items_tab::id_key).and_then(|k| {
        m.itemsTab
            .orderList
            .with(|l| l.iter().position(|x| *x == k))
    });
    if let Some(pos) = pos {
        // L1874
        m.itemsTab.orderList.with_mut(|l| {
            l.remove(pos);
        });
        // L1875 break
    }
    // L1878 (a sequence: index order)
    for spec in m.treeTab.specList.snapshot() {
        // L1879
        let mut rebuildClusterJewelGraphs = false;
        // L1880 ORDER: see the module docs
        for (nodeId, itemId) in spec.jewels.with(model::LjIntMap::pairs) {
            // L1881
            if Some(itemId) == id {
                // L1882
                spec.jewels.with_mut(|j| j.set(nodeId, Some(0.0)));
                // L1883
                rebuildClusterJewelGraphs = true;
                // Deallocate all nodes that required this jewel
                // L1885
                if let Some(node) = spec.nodes.with(|n| n.get(nodeId)) {
                    // L1886 (`depNodeId` is the list index, as in PoB)
                    for (depNodeId, depNode) in node.depends.get().snapshot().iter().enumerate() {
                        // L1887
                        depNode.alloc.set(false);
                        // L1888
                        spec.allocNodes
                            .with_mut(|a| a.set(depNodeId as i64 + 1, None));
                    }
                    // L1890
                    node.alloc.set(false);
                    // L1891
                    spec.allocNodes.with_mut(|a| a.set(nodeId, None));
                }
            }
        }
        // L1895
        if rebuildClusterJewelGraphs && !deferUndoState {
            // L1896
            spec.BuildClusterJewelGraphs();
        }
    }
    // L1899
    if let Some(k) = id.and_then(model::items_tab::id_key) {
        m.itemsTab.items.with_mut(|i| {
            i.shift_remove(&k);
        });
    }
    // L1900
    if !deferUndoState {
        // L1901
        let h = items_hooks();
        (h.populate_slots)(cx);
        // L1902
        (h.add_undo_state)(cx);
    }
}

/// lua: ItemsTab.lua:1906-1945 `ItemsTabClass:CopyAnointsAndEldritchImplicits(newItem,
/// copyEldritchImplicits, overwrite, sourceSlotName)` (called by U35's CreateDisplayItemFromRaw
/// and U51's TradeQuery)
/// @pob Classes/ItemsTab.lua:1906-1945 h=b41647d1
pub fn CopyAnointsAndEldritchImplicits(
    cx: &mut Cx,
    newItem: &model::ItemRef,
    copyEldritchImplicits: bool,
    overwrite: bool,
    sourceSlotName: Option<&str>,
) {
    let m = cx.build().model.clone();
    let base = || {
        newItem
            .base
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
    };
    // L1907
    let newItemType: String = match sourceSlotName {
        Some(s) => s.to_owned(),
        None => {
            if base().weapon.is_some() {
                "Weapon 1".to_owned()
            } else {
                base().r#type.to_string()
            }
        }
    };
    let activeItemSet = active_item_set(&m);
    // L1908
    let Some(entry) = activeItemSet.table(&*newItemType) else {
        return;
    };
    // L1909
    let selItemId = entry.get(n!("selItemId"));
    let currentItem = if selItemId.truthy() {
        item_at(&m, &selItemId)
    } else {
        None
    };
    // if you don't have an equipped item that matches the type of the newItem, no need to do anything
    // L1911
    let Some(currentItem) = currentItem else {
        return;
    };
    let sel_positive = || {
        let v = entry.get(n!("selItemId"));
        match v {
            Val::Num(x) => x > 0.0,
            other => lua_error!("attempt to compare number with {}", other.lua_type()),
        }
    };
    // if the new item is anointable and does not have an anoint and your current respective item does, apply that anoint to the new item
    // L1913
    if isAnointable(Some(newItem))
        && (newItem.enchantModLines.get().is_empty() || overwrite)
        && sel_positive()
    {
        // L1914
        let currentAnoint = currentItem.enchantModLines.get();
        // L1915 skip if amulet has more than one anoint e.g. Stranglegasp
        if currentAnoint.len() == 1 {
            // L1916
            newItem.enchantModLines.set(currentAnoint);
        }
    }
    // if the new item is a non-corrupted Normal, Magic, or Rare Helmet, Body Armour, Gloves, or Boots and does not have any influence
    // and your current respective item is Eater and/or Exarch, apply those implicits and influence to the new item
    // L1921
    let eldritchBaseTypes = ["Helmet", "Body Armour", "Gloves", "Boots"];
    // L1922
    let eldritchRarities = ["NORMAL", "MAGIC", "RARE"];
    // L1923
    for influence in &influenceInfo_default {
        // L1924
        if newItem.influences.truthy(Name::from(influence.key)) {
            // L1925
            return;
        }
    }

    // L1929
    let modifiableItem = !(newItem.corrupted.get() || newItem.mirrored.get());
    // L1930
    if copyEldritchImplicits
        && eldritchBaseTypes.contains(&&*base().r#type)
        && eldritchRarities.contains(&&*newItem.rarity.get())
        && (newItem.implicitModLines.get().is_empty() || overwrite)
        && modifiableItem
        && (currentItem.influences.truthy(n!("cleansing"))
            || currentItem.influences.truthy(n!("tangle")))
    // L1931 `currentItem.implicitModLines` is always a table
    {
        // L1932
        newItem
            .implicitModLines
            .set(currentItem.implicitModLines.get());
        // L1933
        newItem
            .influences
            .set(n!("tangle"), currentItem.influences.get(n!("tangle")));
        // L1934
        newItem
            .influences
            .set(n!("cleansing"), currentItem.influences.get(n!("cleansing")));
    }

    // harvest and heist enchantments on modifiable body armour or weapons
    // L1938 (`currentItem.enchantModLines` is always a table)
    if (base().weapon.is_some() || &*base().r#type == "Body Armour")
        && (newItem.enchantModLines.get().is_empty() || overwrite)
        && sel_positive()
        && modifiableItem
    {
        // L1939
        newItem
            .enchantModLines
            .set(currentItem.enchantModLines.get());
    }

    // L1942
    newItem.BuildAndParseRaw();
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1, CCR-0106 §2) -----------------------------

/// `BuildHooks::items_tab`: U33's constructor (Build.lua:612), then the `ResetUndo` of the
/// `ItemsTab:Load` that already ran when the build file has an `Items` section (L1326;
/// DIVERGENCE(load-tail)).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = ctor::ctor_hook(cx, build);
    let model = cx.build().model.clone();
    if build_xml::xmlSectionList(&model)
        .iter()
        .any(|n| &*n.elem == "Items")
    {
        // DIVERGENCE(load-tail): the jewel socket slots as PoB's Load left them (0: the TreeTab
        // constructor's spec has no jewels)
        let sockets: Vec<(Table, Val)> = model.itemsTab.slots.with(|s| {
            s.values()
                .filter(|t| !t.get(n!("nodeId")).is_nil())
                .map(|t| (t.clone(), t.get(n!("selItemId"))))
                .collect()
        });
        for (slot, _) in &sockets {
            slot.set(n!("selItemId"), 0.0);
        }
        // L1326 self:ResetUndo()
        let f = core_hooks().reset_undo;
        f(cx, me);
        for (slot, selItemId) in sockets {
            slot.set(n!("selItemId"), selItemId);
        }
    }
    me
}

/// `ShellHooks::draw_tab` for `BuildTab::Items`: `self.itemsTab:Draw(tabViewPort, inputEvents)`
/// (Build.lua:1309).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = ctor::tab(cx)
        .unwrap_or_else(|| lua_error!("attempt to index field 'itemsTab' (a nil value)"));
    ItemsTab::draw(cx, me, vp, ev);
}

/// `ItemsUiHooks::add_display_item` (U33's constructor, L342) and the other callers of
/// `itemsTab:AddDisplayItem(noAutoEquip)`.
pub fn add_display_item_hook(cx: &mut Cx, me: CtrlId, noAutoEquip: bool) {
    AddDisplayItem(cx, me, noAutoEquip);
}
