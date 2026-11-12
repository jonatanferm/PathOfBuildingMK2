# CCR-0129: U37 ItemsTab crafting B seams

**Status: applied (CCRDOC, 2026-11-12; code `c55e330` Merge port/U37): §2, §3 (resolution (a), test-local Lua reload) and §4. Not built: §1 (`checkLineForAllocates` is still copied in `craft_b.rs`) and §3's patch registration in `patches.rs`.**

Unit: **U37** (ItemsTab crafting B, `Classes/ItemsTab.lua:3121-3992`,
`pob-app/src/tabs/items/craft_b.rs`).
Filed status: **pending** (code on `port/U37` follows it, with the local workarounds marked
`// CCR-0129`). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `checkLineForAllocates` is private in U35's file

Evidence: `checkLineForAllocates(line, nodes)` is a module local of ItemsTab.lua (L2493-2501,
U35's range); U37's popups call it at L3536 (custom getSortValue), L3877/L3885 (crucible labels
and the item-mod match), L3932 (crucible addModifier) and L3952 (crucible tooltip). U35 ported it
as a private `fn checkLineForAllocates` in `tabs/items/display_item.rs` (it carries the `@pob`
marker for L2493-2501).

Applied locally: `craft_b.rs` has a statement-for-statement copy without a `@pob` marker (U35's
marker covers the lines once), marked `// CCR-0129 §1`.
Proposed (U35, `display_item.rs`): make it `pub fn`; U37 then deletes its copy and calls
`super::display_item::checkLineForAllocates`.

```diff
-fn checkLineForAllocates(line: &str, nodes: Option<&model::LjIntMap<model::NodeRef>>) -> String {
+pub fn checkLineForAllocates(line: &str, nodes: Option<&model::LjIntMap<model::NodeRef>>) -> String {
```

## 2. Hook entries U37 filled in other units' default tables (one line each)

As CCR-0107 §1 / CCR-0120 §1 / CCR-0126 §2 propose for every owner:

| Table (owner, file) | Entry | Now |
|---|---|---|
| `ItemsUiHooks` (U33, `tabs/items/ctor.rs`) | `corrupt_display_item`, `add_custom_modifier_to_display_item`, `add_crucible_modifier_to_display_item` | `craft_b::{CorruptDisplayItem, AddCustomModifierToDisplayItem, AddCrucibleModifierToDisplayItem}` |
| `ItemsTabHooks` (U32, `tabs/items/slot_helper.rs`) | `add_item_set_tooltip` | `craft_b::AddItemSetTooltip` |
| `BuildHooks` (U19, `build/init.rs`) | `items_add_item_set_tooltip` (named U34 as owner; the Lua is U37's range, CCR-0126 §2) | `craft_b::AddItemSetTooltip` |

## 3. ORDER: the `pairs` loops of the popups need an oracle patch

Evidence (ui doc §8 rule 6): ItemsTab.lua iterates string-keyed tables with `pairs` where the
result is observable:

* L3151 `pairs(data.itemMods.ItemExclusive)` and L3157 `pairs(self.displayItem.affixes)`
  (CorruptDisplayItem): the list is sorted at L3163 by the stripped first line, then level;
  entries tie on both (e.g. several tiers of one implicit family at one level), so the
  `pairs` order decides the dropdown order and `defaultSortOrder`.
* L3662 `pairs(self.displayItem.affixes)` (custom popup, PREFIX/SUFFIX): the L3671 comparator
  ties on equal stat orders and levels.
* L3582 `pairs(baseCategories)`: the order of the `ConPrintf` lines for unknown ids.
* L3882 `pairs(self.build.data.crucible)`: `nodeSelections` (a later matching mod of a
  multi-node location overwrites an earlier one) and the final position of the `"None"` string
  entry, which the L3907 comparator treats as equal to every entry.

The other loops (L3642 essences, L3686 veiled, L3711 fossil, L3724 necropolis, L3737 beastcraft)
are followed by total-order sorts and are unobservable (c).

Applied: resolution (a). `craft_b.rs` walks every one of these tables keys ascending (bytewise);
the u37 oracle test (`crates/pob-ui-oracle/tests/u37_craft.rs`, `LUA_STUBS`) reloads ItemsTab.lua
L3121-3979 from the file's own text at the same line numbers, with the 10 `pairs(` sites
replaced by a sorted `pairs` (each site asserted by its text and count), and installs the three
reloaded methods on `common.classes.ItemsTab`, as u42 does for ConfigModBrowser (CCR-0121 §6).
Proposed (U8, `pob-ui-oracle/src/patches.rs`): a registered patch with the same substitutions,
so every scenario that opens these popups gets it; the u37 test then drops its reload.

## 4. Notes (no change proposed)

* `data.beastCraft`, `data.crucible`, `data.essences` and `files["Data/DelveDropOnly.lua"]` /
  `["Data/IncursionDropOnly.lua"]` are untyped roots (`Data::other`, CCR-0011 §7). `craft_b.rs`
  converts the beastcraft and crucible mods once to `ItemMod` (process-wide `OnceLock`, so each
  mod keeps one identity), because `CanHaveMod`, `GetModSpawnWeight` and
  `AddModComparisonTooltip` take the typed affix shape. The crucible-only fields (`tier`,
  `nodeLocation`) stay beside it. A typed schema for these roots would remove the conversion.
* The crucible comparator (L3907-3915) compares `nil < string` when the `"None"` entry meets a
  crucible mod without `type` (the five amulet `SpellDamage1-5` mods of the data); PoB raises
  there, and so does the port (§8 rule 8: no fixes). The popup is offered only for weapons,
  shields and `canHaveShieldCrucibleTree` items (ItemsTab.lua:1008-1010), where `CanHaveMod`
  rejects those mods.
