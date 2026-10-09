# CCR-0015 (IT-B): Item core stub signatures and the shape of `Item.extra`

**Status: proposed (IT-B, 2026-10-09).** The changes in §1 are already made in the IT-B-owned
file `crates/pob-calc/src/classes/item.rs` (marked `CCR-0015`); nothing outside it calls the
changed functions (`cargo check -p pob-calc` and the wasm32 check are green). §2 records a
convention that IT-A and IT-C must follow; §3 is one documented deviation.

- **Raised by:** IT-B
- **Affects:** `crates/pob-calc/src/classes/item.rs` (U0 stub signatures), docs/port/contracts.md
  `#class-census` (Item), IT-A (`item_parse.rs`), IT-C (`item_mods.rs`, `item_tools.rs`)
- **Pin:** 16de4b82d

## 1. Stub signatures that did not match the Lua

| Function | U0 stub | Lua evidence | Now |
|---|---|---|---|
| `Item::GetVariantGroupOptions` | `-> Vec<Table>` | Item.lua:362-367 inserts `variantId` (a number) into `options`; ItemsTab.lua:372/1998/2005 use `#` and the ids | `-> Vec<f64>` |
| `Item::MutateMod` | `mutatedValue: Option<f64>` | ItemsTab.lua:1044/1105 pass `not line.mutated` (a boolean); Item.lua:1647 `mutatedValue and …`, 1678 `mutated = mutatedValue` | `mutatedValue: bool` |
| `getTagBasedModifiers` | `-> Vec<Table>` | Item.lua:142-286 returns nothing (it only `ConPrintf`s; its one caller is commented out, :296) | `-> ()` |
| `DrawViewer` (ItemSlotHelper.lua) | `fn DrawViewer(nodeId, x, y, w, h)` with an `unported!` body | ItemSlotHelper.lua:10-38 is `M.DrawViewer(itemsTab, …)`: `SetDrawLayer`, `DrawImage`, `SetViewport`, `socketViewer:Draw` — UI only, no calc state | removed; `// @pob-skip Modules/ItemSlotHelper.lua:10-38` (UI port owns drawing) |

Proposed diff for the record (contracts.md has no per-function list, so only the stub file changes):

```diff
-    pub fn GetVariantGroupOptions(&self, groupId: f64, excludeSelected: bool) -> Vec<Table> {
+    pub fn GetVariantGroupOptions(&self, groupId: f64, excludeSelected: bool) -> Vec<f64> {
-    pub fn MutateMod(&self, modId: Name, newModId: Name, mutatedValue: Option<f64>) {
+    pub fn MutateMod(&self, modId: Name, newModId: Name, mutatedValue: bool) {
-pub fn getTagBasedModifiers(tagName: &str, itemSlotName: &str) -> Vec<Table> {
+pub fn getTagBasedModifiers(tagName: &str, itemSlotName: &str) {
-/// @pob Modules/ItemSlotHelper.lua:10-38 h=112b6e05
-pub fn DrawViewer(nodeId: f64, x: f64, y: f64, w: f64, h: f64) { unported!(…) }
+// @pob-skip Modules/ItemSlotHelper.lua:10-38 reason="M.DrawViewer: UI drawing …"
```

## 2. Shape of the untyped Item fields (`Item.extra`) — binding for IT-A / IT-C

contracts.md `#class-census` puts 71 Item fields in `extra: Table` without saying how they are
stored. IT-B reads them (BuildRaw, the variant functions, Craft, GetModSpawnWeight) and IT-A
writes them (ParseRaw), so the shape must be shared. Rule: **each field is stored under its Lua
name with its Lua shape**, using `Val` as in calc doc §5.1:

- strings `Val::Str`, numbers `Val::Num`, booleans `Val::Bool`; nil = absent;
- Lua tables are calc `Table`s with the Lua keys: `versionList` (list of version names),
  `variantGroups[groupId][variantId][versionId|0] = true`, `variantGroupSelections[groupId] =
  variantId`, `prefixes`/`suffixes` (lists of `{ modId, range, fractured }`, `range` a number or a
  list of numbers, plus the optional `limit`), `baseLines[baseName] = { line, variantList,
  versionList, variantGroupList }` (string keys: insertion order, `LUA-ORDER` in BuildRaw),
  `upgradePaths`, `rawLines`, `modMagnitudeMods`, …;
- `affixes` (Item.lua:1032-1039: `data.itemMods[...]` or `rareLikeUnique.affixes`) is
  `Val::Obj(Rc<item::ItemAffixes>)`, `ItemAffixes(pub &'static AffixTable)`; `Item::affixes_table()`
  reads it;
- `rareLikeUnique`, `enchantments`, `mutatedLines` (data references) may be any truthy `Val`
  (IT-B only tests their truthiness); IT-A chooses, e.g. `Val::Obj` of the data `Arc`;
- the influence flags `self.shaper` … `self.tangle` (Item.lua:560, ItemTools.lua:16-25) live in
  the typed `Item.influences` table (as the census says), not in `extra`.

Mod lines (`*ModLines` elements, `baseLines` values) are `Table`s with their Lua keys (`line`,
`modTags` as a list of strings, `variantList`/`versionList`/`variantGroupList` as
`{[id] = true}`, the line flags, `range`, `corruptedRange`, `modGroup`, `order`, `modId`,
`newModId`, `modList`, …).

The module-level locals of Item.lua that ParseRaw uses are public in `item.rs`: `dmgTypeList`,
`catalystList` (+ `catalystList_get`), `catalystDescriptorList`, `catalystTags`,
`getCatalystScalar`, `normaliseModLine`, `uniqueModStatOrder` (thread-local, D17),
`sortCraftedModLines`, `influenceInfo`, `influenceItemMap`, `lineFlags` (+ `is_lineFlag`),
`specToNumber`.

## 3. Deviation: `Utils.stringify` of a mod

`writeStringify` (Utils.lua:19-62) walks any Lua table. A `Val::Mod` (an immutable `Arc<Mod>`,
calc doc §3.6) is not walked; the port raises `Disallowed stringify type table`. No in-scope
caller passes a mod (the callers are TradeQuery*/Export scripts, outside the calc scope).
