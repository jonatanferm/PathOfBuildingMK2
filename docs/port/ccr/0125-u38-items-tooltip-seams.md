# CCR-0125: U38 ItemsTab tooltips seams

**Status: applied (CCRDOC, 2026-11-12; code `8ff2b69` Merge port/U38): §4, plus the local workarounds as the as-built design: §1 `tooltip::flavour_lookup`, §2 DIVERGENCE(temp-item-id) counting down from `u32::MAX`, §3 `tooltip::copy_item`. Not built: §1's fix of U33's `ctor::flavourLookup` (the copy that drops every entry is unused; FOLLOWUPS "CCRDOC" 9), §3 `Item::shallow_copy`, §5 (the re-exports) and §6 (the bridge spec).**

Unit: **U38** (ItemsTab tooltips, `Classes/ItemsTab.lua:3993-5055`,
`pob-app/src/tabs/items/tooltip.rs`). Filed status: **pending** (code on `port/U38` follows it, with
the local workarounds marked `// CCR-0125`). Number reserved with `scripts/port/next_ccr.sh`.

## 1. U33's `ctor::flavourLookup` drops every entry

Evidence: `Data/FlavourText.lua:5-10` (every entry's `text` is a list of lines:
`text = { "\"More blood... hack open flesh... must drink...\"", }`); ItemsTab.lua:62-69 stores
that list (`flavourLookup[entry.name][entry.id] = entry.text`) and AddItemTooltip iterates it
(`for _, line in ipairs(flavour)`, L4590). `crates/pob-app/src/tabs/items/ctor.rs`
`flavourLookup()` reads `entry.field("text")` through `text_of`, which accepts only a string or
a number, so `continue` skips every entry and the map is empty: no unique would show its flavour
text (`main.showFlavourText` is on by default).

Proposed diff (U33's file):

```diff
-pub fn flavourLookup() -> &'static indexmap::IndexMap<String, indexmap::IndexMap<String, String>> {
+pub fn flavourLookup() -> &'static indexmap::IndexMap<String, indexmap::IndexMap<String, Vec<String>>> {
 ...
-            let (Some(name), Some(id), Some(text)) = (
-                text_of(entry.field("name")),
-                text_of(entry.field("id")),
-                text_of(entry.field("text")),
-            ) else {
+            let (Some(name), Some(id), gamedata::DataValue::Table(text)) = (
+                text_of(entry.field("name")),
+                text_of(entry.field("id")),
+                entry.field("text"),
+            ) else {
                 continue;
             };
+            let text: Vec<String> = text.seq.iter().map_while(text_of).collect();
```

Workaround: `tooltip::flavour_lookup()` builds the same map with the lines (no `@pob` marker;
the range stays U33's), marked DIVERGENCE(flavour-lookup). Delete it once U33's is fixed.

## 2. Negative item ids (`buildSpecForJewelComparison`)

Evidence: ItemsTab.lua:4163-4170 stores a replacement jewel that is not in `self.items` under
the first free id counting down from `-1`, so `spec:BuildAllDependsAndPaths()` (via
`build.itemsTab.items[spec.jewels[nodeId]]`, PassiveSpec.lua:1112-1113) finds it.
`model::ItemsTab::items` is `Shared<IndexMap<u32, ItemRef>>` (`crates/pob-calc/src/model/mod.rs`)
and `PassiveSpec::psb_item` (`passive_spec_paths.rs:148-160`) rejects negative ids.

Proposed (calc:BM-A / PS-B): item ids as `i64` keys (or `Key::try_num` keys) in `items` and
`psb_item`. Workaround: the port counts down from `u32::MAX`
(DIVERGENCE(temp-item-id) in `tooltip.rs`); the id is visible only to the comparison spec and
is removed right after (a drop guard; DIVERGENCE(xpcall-traceback): PoB's re-raised message
carries `debug.traceback`'s text, the port's the message alone).

## 3. No shallow copy of an `Item` (`copyTableSafe(item, true, true)`, L5026)

Evidence: `CreateUndoState` copies every item with `copyTableSafe(self.items[k], true, true)`
(a new Item object, same field values, sub-tables shared). `pob_calc::classes::Item` has no
`Clone` and no copy helper. Workaround: `tooltip::copy_item` (every field; `extra` and
`influences`, which hold the item's own raw keys, are `copyTable(true)`'d; sub-table fields are
shared). DIVERGENCE(item-copy): `slotModList[1]`/`[2]` are one Lua table but two Rust fields.
Proposed: `impl Item { pub fn shallow_copy(&self) -> Item }` in calc:IT-A (`classes/item.rs`), so
a field added to `Item` cannot be forgotten by the UI copy.

## 4. Hook entries installed in other units' defaults (one line each)

As CCR-0107 §1, CCR-0116 §4 and CCR-0106 §2 propose, U38 replaced the
entries that name its methods:

| File (owner) | Entry | Now |
|---|---|---|
| `tabs/items/slot_helper.rs` (U32) | `add_item_tooltip`, `add_undo_state`, `set_mod_flag` | `tooltip::{add_item_tooltip_hook, add_undo_state_hook, set_mod_flag_hook}` |
| `tabs/calcs/breakdown.rs` (U40) | `BreakdownHooks::add_item_tooltip` | `tooltip::add_item_tooltip_named` |
| `tabs/tree/timeless_list.rs` (U28) | `AppTimelessBuild::add_item_stat_differences` | `tooltip::add_item_stat_differences_hook` |
| `build/on_frame.rs` (U20) | `ShellHooks::tab_flag` / `set_tab_flag` `TabFlag::Items` arms | `tooltip::{mod_flag, set_mod_flag}` |

Not changed: U44's `TabSeams::items_add_undo_state` (`tabs/import/character.rs`) still sets
only the flag (`set_tab_flag(TabFlag::Items, true)`, which now reaches U38's `modFlag`):
`tests/u44_character.rs` stands in that flag, so switching the entry to
`tooltip::add_undo_state_hook` belongs with an update of that test (proposed for U44).

Every oracle test that exercised those entries installs its own stand-ins on both sides
(u32, u33, u39, u40, u44), so their results do not change. For U34: `tooltip::{reset_undo,
undo, redo}` are ItemsTab's UndoHandler methods (Load's `ResetUndo`, Draw's Ctrl+Z/Y).

## 5. `model.rs` re-exports neither `ModCfg` nor ItemTools nor `copyTableSafe`

Evidence: AddItemStatDifferences builds `{ actor = "player" }` cfgs (L4673, L4678, L4880-4894),
AddItemTooltip calls `itemLib.formatModLine` (L4480) and reads `itemLib.influenceInfo` (L4239),
CreateUndoState calls `copyTableSafe` (L5026, L5033). Workaround: `use pob_calc::cfg::ModCfg`,
`pob_calc::classes::item_tools::{formatModLine, influenceInfo_all}`,
`pob_calc::modules::common::copyTableSafe` in `tooltip.rs` (as CCR-0120 §3). Proposed diff
(U15's `pob-app/src/model.rs`):

```diff
+pub use pob_calc::cfg::ModCfg;
+/// `itemLib` (Modules/ItemTools.lua, calc:IT-B)
+pub use pob_calc::classes::item_tools as itemLib;
+pub use pob_calc::modules::common::copyTableSafe;
```

## 6. The LuaCalcBridge cannot carry the jewel comparison spec (U15)

Evidence: `getReplacedItemAndOutput` (L4920-4928) passes `override.spec =
buildSpecForJewelComparison(..)` for a socket compare whose old or new jewel changes the tree
(conquered, Intuitive Leap–like, Impossible Escape). `pob-ui-oracle/src/lua_calc.rs`
`override_lua` only carries a spec that is an entry of `build.treeTab.specList` and raises
otherwise. So such tooltips cannot be checked through the bridge; `tests/u38_tooltips.rs`
skips jewels in builds with such a jewel. Proposed (U15): serialise an arbitrary spec (its
`jewels`, allocated node ids, class and tree version) and rebuild it in the bridge VM with
PoB's own `cloneSpecForJewelComparison`/`BuildAllDependsAndPaths`.
