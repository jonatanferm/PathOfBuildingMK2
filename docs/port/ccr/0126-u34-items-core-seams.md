# CCR-0126: U34 ItemsTab B seams

**Status: applied (CCRDOC, 2026-11-12; code `2167f22` Merge port/U34): §1 `CoreHooks` (filled by U35, U38 and U53) and §2; §3 was resolved by U25 (GetSpecList). §4 DIVERGENCE(load-tail) stays: the model hook at ItemsTab.lua:1326 was not built.**

Unit: **U34** (ItemsTab B, `Classes/ItemsTab.lua:1216-1947`, `pob-app/src/tabs/items/core.rs`).
Filed status: **pending** (code on `port/U34` follows it). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. `CoreHooks`: the methods of U35/U38 that `Draw` and `Load` call

Evidence: `ItemsTab:Draw` calls `self:Undo()` (L1473), `self:Redo()` (L1476) and
`self:ToggleDisplayItemModLine(line.modLine)` (L1542); `ItemsTab:Load` ends with
`self:ResetUndo()` (L1326). The ItemsTab UndoHandler state (`CreateUndoState`/`RestoreUndoState`,
L5021-5055, and the `undo`/`redo` buffers with `modFlag`) is U38's (ui doc §10.3, U38 row), and
`ToggleDisplayItemModLine` (L2123-2136) is U35's. Neither U32's `ItemsTabHooks` (CCR-0107 §1:
`add_undo_state`, `set_mod_flag`) nor U33's `ItemsUiHooks` (CCR-0120 §1) has entries for them.

Applied locally (U34's own file): `core::CoreHooks` + `core_hooks()` / `set_core_hooks(..)` /
`default_core_hooks()`: `undo`, `redo` and `toggle_display_item_mod_line` raise "… is not ported
yet (U35/U38)"; `reset_undo` does nothing (it runs on every build load and there are no buffers to
reset until U38 lands). Proposed: U38 replaces `undo`/`redo`/`reset_undo` (and, in U32's table,
`add_undo_state`/`set_mod_flag`, and the `TabFlag::Items` arms of `ShellHooks::tab_flag` /
`set_tab_flag`), U35 replaces `toggle_display_item_mod_line` (one line each).

`RestoreUndoState` ends with `self:PopulateSlots()` (L5054): U38 calls `core::PopulateSlots`
(the model half plus the slot lists), which is the "restore tail" ui doc §10.3 gives U34.

## 2. The entries U34 filled in other units' default tables (one line each)

As CCR-0107 §1 / CCR-0120 §1 / CCR-0104 §1 propose for every owner:

| Table (owner, file) | Entry | Now |
|---|---|---|
| `ItemsTabHooks` (U32, `tabs/items/slot_helper.rs`) | `add_forbidden_jewel_counterpart`, `sort_item_list`, `delete_item` | `core::{AddForbiddenJewelCounterpart, SortItemList, DeleteItem}` |
| `ItemsUiHooks` (U33, `tabs/items/ctor.rs`) | `add_display_item` | `core::add_display_item_hook` |
| `BuildHooks` (U19, `build/init.rs`) | `items_tab` | `core::ctor_hook` (U33's constructor, then Load's `ResetUndo`, DIVERGENCE(load-tail)) |
| `BuildHooks` | `items_set_active_item_set` | `core::SetActiveItemSet` (was the model half only: Build.lua:549's loadout switch now also refreshes the slot lists and calls `SyncLoadouts`, L1615-1616) |
| `BuildHooks` | `items_equip_item_in_set` | `core::EquipItemInSet` (was the model half only, without SHIFT and `AddUndoState`) |
| `ShellHooks` (U20, `build/on_frame.rs`) | `draw_tab` | the `BuildTab::Items` arm: `core::draw_tab` |

`ItemsTabHooks::populate_slots` and `set_active_item_set` already had U32's complete defaults (the
same statements as `core::PopulateSlots` / `core::SetActiveItemSet`) and are left as they are.

Not changed (proposed for their owners):

* `TabSeams::items_delete_item` (U44, `tabs/import/character.rs`, CCR-0117 §1) defaults to U44's
  model copy `model_delete_item`; it can become `crate::tabs::items::core::DeleteItem` (the same
  statements; its `PopulateSlots`/`AddUndoState` go through U32's hooks instead of `TabSeams`),
  and `model_delete_item` deleted.
* `BuildHooks::items_add_item_set_tooltip` (`build/init.rs`) names U34 as its owner; the Lua
  (`AddItemSetTooltip`, L3982-3991) is U37's range.
* `TimelessJewelListControl`'s `add_undo_state` seam (U28, `tabs/tree/timeless_list.rs:192`) names
  U34; `itemsTab:AddUndoState()` is U38's (it can call `items_hooks().add_undo_state`).
* `TreeViewHooks::socket_right_click` (U23, `tree_view/mod.rs`) names "U32/U34"; the Lua
  (PassiveTreeView.lua:503-510) is U23's range. With U33's `ctor::State::sockets` it is
  `slot.dropped = true; cx.select_control(items_tab, Some(slot)); build.viewMode = "ITEMS"`.

## 3. `BuildHooks::tree_get_spec_list` raises every frame on the Items tab

Evidence: `ItemsTab:Draw` ends with `self.controls.specSelect:SetList(self.build.treeTab:GetSpecList())`
(L1569), every frame. U19's default for `tree_get_spec_list` raises "TreeTab:GetSpecList is not
ported yet (U25)", so until U25 lands the desktop app shows the error prompt when the Items tab is
opened (the oracle tests install the u20/u22/u32/u33 stand-in, a port of TreeTab.lua:492-498).
Proposed (U25 or U19, `build/init.rs`): make that stand-in the default until U25 replaces it; it
reads only the model (`treeTab.specList`, `gameVersions`).

## 4. Load's `ResetUndo` (L1326) runs before the deferred passive trees in PoB

Evidence: Build.lua:655-677 loads every section except the passive trees in document order, then
the deferred `Tree` sections. `ItemsTab:Load` ends with `SetActiveItemSet` (whose `PopulateSlots`
sets every jewel socket slot to `build.spec.jewels[nodeId] or 0`, ItemSlotControl.lua:87-88, with
the TreeTab constructor's fresh spec: 0) and `ResetUndo()`, so PoB's first undo state has every
socket slot at 0 (undoing back to it unequips the tree jewels: the u34 oracle runs show it).
calc:BM-A's `Init_model` runs the whole load, deferred trees included, before the UI tabs exist,
and `ItemsTab::Load` marks L1326 `// UI:` with no hook.

Workaround (U34's `core::ctor_hook`, DIVERGENCE(load-tail)): set the socket slots' `selItemId` to 0
around the `ResetUndo` call, then restore the loaded values. Proposed (calc:BM-A,
`model/items_tab.rs`): a UI hook at L1326 (as `build_xml::set_save_stats_hook`), registered by
the UI before `Init_model`, which captures the undo state at that point; U34's `ctor_hook` then
drops the workaround.

## 5. Notes for U35/U38

* `core::CopyAnointsAndEldritchImplicits(cx, newItem, copyEldritchImplicits, overwrite,
  sourceSlotName)` is public for U35's `CreateDisplayItemFromRaw` (L1952) and U51's TradeQuery.
* `core::AddDisplayItem` calls `items_hooks().set_display_item(cx, None)` (U35) and the U32 hooks
  for `AddItem`/`PopulateSlots`/`AddUndoState`.
* `SortItemList` calls `items_hooks().get_equipped_slot_for_item` (U35, L2528-2542) from the
  comparator: until U35 lands, Sort raises.
* `tests/u34_items_core.rs` has a test-only port of the UndoHandler with
  `CreateUndoState`/`RestoreUndoState` (keeping the item objects themselves, where PoB keeps
  shallow copies); U38 can check its real port against it.
