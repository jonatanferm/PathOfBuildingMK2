# CCR-0107: U32 item lists and slots seams

**Status: applied (CCRDOC, 2026-11-12; code `12e0e27` Merge port/U32): §1 `ItemsTabHooks` (ui doc §10.5) and §6. Rejected because they were not built: §2 (`ItemSlot_SetSelItemId` stays private; U32's copy `tabs::items::slot::set_sel_item_id` is the one the UI uses), §3 (no single shared `matchOrPattern`; ui doc §13 item 15 lists the copies), §4 (the wiki helpers stay in `tabs/items/slot.rs` and `controls/gem_select.rs`) and §5 (the identity tokens of `shared_set_list.rs` stay).**

Unit: **U32** (ItemSlotControl, ItemListControl, ItemDBControl, ItemSetListControl,
SharedItemListControl, SharedItemSetListControl, ItemSlotHelper). Filed status: **pending** (code on
`port/U32` follows it, with the local workarounds marked `// CCR-0107`). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. `ItemsTabHooks`: the ItemsTab methods U32's controls call

Evidence: the controls call `self.itemsTab:PopulateSlots()` (ItemSlotControl.lua:25, :139;
ItemListControl.lua:45, :65, :289, :320; ItemDBControl.lua:376), `:AddUndoState()` (17 sites),
`:AddItem(..)`, `:AddForbiddenJewelCounterpart(..)`, `:SortItemList()`, `:DeleteItem(..)`,
`:SetActiveItemSet(..)`, `:NewItemSet()`, `:SetDisplayItem(..)`, `:CreateDisplayItemFromRaw(..)`,
`:GetEquippedSlotForItem(..)`, `:AddItemTooltip(..)`, `:AddItemSetTooltip(..)`, `.modFlag`, and
`itemsTab.socketViewer` (ItemSlotHelper.lua:19-30). Those are ItemsTab.lua lines of U33-U38 and
the PassiveTreeView of U23, which are not ported; the ui doc has no seam for a control unit that
precedes its tab.

Applied locally (U32's own file, `pob-app/src/tabs/items/slot_helper.rs`), in the form of U19's
`BuildHooks` (CCR-0098 §3): `ItemsTabHooks` + `items_hooks()` / `set_items_hooks(..)` /
`default_items_hooks()`. Defaults: `populate_slots` (calc:BM-A's model `PopulateSlots`, then
U32's `ItemSlotControl::populate_list` for every slot control of the tab), `add_item`
(calc:BM-A), `set_active_item_set` (calc:BM-A + the slot lists + `build:SyncLoadouts()`),
`new_item_set` (calc:BM-A); every other entry raises "… is not ported yet (Uxx)". Proposed: the
owners (U33-U38, U23) replace their entries in `default_items_hooks` (one line each), as the
wave-3 owners do in `default_hooks`.

## 2. calc:BM-A's `ItemSlot_SetSelItemId` is private

Evidence: `crates/pob-calc/src/model/items_tab.rs:207-232` ports ItemSlotControl.lua:72-84
(`SetSelItemId`) as a private `fn ItemSlot_SetSelItemId`; the UI calls it on
`itemsTab.slots[name]` from ItemListControl.lua:56/316/318, ItemDBControl.lua:371 and the
slot's own selFunc/ReceiveDrag (and later ImportTab.lua:1818, TimelessJewelListControl.lua:344,
CompareTab.lua:1457/2747/2760/2774, TradeQuery.lua:1257).

Proposed diff (BM-A's file):

```diff
-fn ItemSlot_SetSelItemId(build: &BuildRef, slot: &Table, selItemId: Val) {
+pub fn ItemSlot_SetSelItemId(build: &BuildRef, slot: &Table, selItemId: Val) {
```

Workaround: `pob_app::tabs::items::slot::set_sel_item_id` is a local copy of the body (the
spec read is `build.spec` directly: the UI only exists once the TreeTab constructed the spec).
Delete it once the function is public. `slot_IsShown` (the `$shown` kinds) could become public
for U33 the same way.

## 3. `string.matchOrPattern` (Common.lua:1038-1079) has no shared port

Evidence: ItemDBControl.lua:164-201 `PCall(string.matchOrPattern, ..)`; NotableDBControl (U28),
SkillsTab and ConfigModBrowser call it too. No crate ports it (`grep -r matchOrPattern crates/`).

Proposed: one `pob_ui_core::luastr::match_or_pattern(s, pattern) -> bool` (raising the pattern
errors as `lua_error!`, so `pob_lua::error::catch` is `PCall`). Workaround:
`pob_app::tabs::items::item_db::match_or_pattern` (pub, with the Common.lua line anchors), which
the owner of the shared helper can move.

## 4. `itemLib.wiki.openItem` / `matchesKey` (ItemTools.lua:384-413) have no UI owner

Evidence: calc:IT-B skips the block as UI (`item_tools.rs:979`); ItemSlotControl.lua:175-183,
ItemListControl.lua:380-388, ItemDBControl.lua:393-400 (and GemSelectControl, SkillListControl)
call it; no ui-doc row lists ItemTools.lua. Workaround: `slot::wiki_open_item` /
`slot::wiki_matches_key` (no `@pob` marker, the range stays IT-B's skip). Proposed: a shared
`pob_app::controls::wiki` module (the U29 gem tooltip unit is a natural owner), or this one made
the shared copy.

## 5. `main.sharedItemSetList` entries need identity

Evidence: `SharedItemSetListControl` keeps `main.sharedItemSetList` as its list (L15) and its
rows are Lua tables compared by identity (`selValue`, ListControl's reorder, the
`"SharedItemList"` drag payload that ItemSetListControl.lua:97-110 reads). U16 holds the
entries by value: `MainState::shared_item_set_list: Vec<SharedItemSet>`
(`pob-app/src/main_mode/mod.rs`), which a `ListValue` (an `Rc` identity) cannot alias.

Proposed diff (U16's files):

```diff
-    pub shared_item_set_list: Vec<SharedItemSet>,
+    pub shared_item_set_list: Vec<Rc<RefCell<SharedItemSet>>>,
```

(settings.rs' loader/saver wrap/borrow each entry). Workaround: `shared_set_list.rs` gives every
entry an identity token in a per-thread vector parallel to the main list (rebuilt when the
lengths differ; this file's inserts and removes keep both in step), marked `// CCR-0107`.

## 6. Notes for U33/U34

* `ItemSlotControl::new(anchor, x, y, itemsTab, slot)` takes calc:BM-A's slot table (the one
  `ctor_model` built for that slot name), not `slotName/slotLabel/nodeId`; U33 wraps
  `build.itemsTab.orderedSlots` in order and sets the `shown` mixins of ItemsTab.lua:158-208
  from the table's `$shown` kind (the test `crates/pob-ui-oracle/tests/u32_items.rs` `ctor` does
  exactly that).
* The constructor runs before `build.tabs.items` is set (Build.lua:612); after creating the
  slot controls, call `slot_helper::populate_slot_lists_of(cx, tab)` (L1211's UI half).
* The flask checkbox's state is the model's `slot.controls.activate.state`
  (ItemsTab.lua:1277/1610); `ItemSlotControl::sync_activate` copies it in before drawing and
  before a key reaches the checkbox (DIVERGENCE(activate-state) in `slot.rs`).

## 7. Found by the U32 scenarios (other owners)

* calc:BM-B: after an equip change re-creates an item-granted skill group (The Gull's
  `TriggeredSummonLesserShrine`, `SkillsTab:UpdateSocketGroups`), PoB saves the gem with
  `count="nil"`, the port with `count="1"` (the frozen gem `count` cannot hold nil). The calc
  reads `count or 1`, so the numbers agree; `tests/u32_items.rs`' T4 normalisation reads a
  `Gem` `count="nil"` as "1" on both sides and counts it among the set-aside fields.
* `BuildState::calc_func` (U20) is set only after an OnFrame recalc; code that ports
  `calcsTab:GetMiscCalculator()` should call the facade's `misc_calculator` (ItemDBControl does).
