# CCR-0025 (IT-A): Item:ParseRaw contract notes

**Status: applied 2026-11-10 by CCR2 (§2, §4, §5, §6); §1 and §3 need code. See AMENDMENTS A8.**

**Filed status: proposed (IT-A, 2026-10-10).** Every item is already worked around inside the IT-A file
`crates/pob-calc/src/classes/item_parse.rs` (marked `CCR-0025`); no frozen file was edited.

- **Raised by:** IT-A
- **Affects:** calc doc §11.1 (IT-A row: deps), §5.9 (`Item.name`), §9.2 ("Replay"),
  docs/port/contracts.md `#class-census` (Item), CCR-0032 §2 (shapes IT-A chooses), IT-C
  (`item_mods.rs`, `item_tools.rs` read what ParseRaw stores), the UI port (`main` options)
- **Pin:** 16de4b82d

## 1. `main.defaultItemAffixQuality` has no home

ParseRaw reads the global `main.defaultItemAffixQuality` (Item.lua:814, 1275, 1327, 1345);
`itemLib.formatModLine` (IT-C) reads it too (ItemTools.lua:363), and ItemsTab (BM-A/UI) at 923, 2280, 2325,
2358, 3348. It is a UI option: `Main.lua:113` sets `0.5`, the settings file (`Main.lua:636`) and the
options slider (`Main.lua:1137`) change it. No frozen file models `main`.

Workaround: a thread-local in item_parse.rs, `item_parse::main_defaultItemAffixQuality()` /
`set_main_defaultItemAffixQuality(Option<f64>)`, default `Some(0.5)` (Lua `nil` is representable,
which the `or 1` at Item.lua:1275 allows for).

Proposed: move it to `crates/pob-calc/src/module_state.rs` as a `MainOptions` thread-local
(`defaultItemAffixQuality: Cell<Option<f64>>`), owned by U0, set by the UI's options dialog;
item_parse.rs and item_tools.rs read it from there.

Until U0 applies this, the thread-local in item_parse.rs is the only home. IT-C's `formatModLine`
(ItemTools.lua:363) and the ItemsTab readers must read `item_parse::main_defaultItemAffixQuality()`
and must not keep their own copy. A second copy would let ParseRaw and formatModLine use different
values once the options slider changes one of them.

```diff
 // module_state.rs
+/// The `main.*` options the calc reads (Modules/Main.lua): defaultItemAffixQuality (Main.lua:113).
+pub struct MainOptions { pub defaultItemAffixQuality: Cell<Option<f64>> }
+thread_local! { pub static MAIN: MainOptions = MainOptions { defaultItemAffixQuality: Cell::new(Some(0.5)) }; }
```

## 2. Shapes of the values ParseRaw stores (CCR-0032 §2 leaves these to IT-A)

| Lua value | Rust | Reader |
|---|---|---|
| a mod line's `modList` (the fresh mod tables `modLib.parseMod` returned, or `{ }`) | list `Table` of `Val::Mod(Arc<Mod>)` | IT-C BuildModList (Item.lua:2556-2560 sets `mod.source` on them: a mutation of a fresh copy, §3.6 — IT-C replaces the entry with a new `Mod`) |
| `self.affixes` | `Val::Obj(Rc<item::ItemAffixes>)` (CCR-0032 §2) | `Item::affixes_table()` |
| `self.rareLikeUnique` (`data.rareLikeUniques[title:lower()]`) | `Val::Obj(Rc<RareLikeUniqueRef>)` | `Item::rareLikeUnique_ref()` |
| `self.enchantments` (`data.enchantments[...]`) | `Val::Obj(Rc<EnchantmentSlotRef>)` | `Item::enchantments_ref()` |
| `self.mutatedLines` (`data.foulbornMap[title]`) | `Val::Obj(Rc<Arc<FoulbornLines>>)` | `Item::mutatedLines_ref()` |
| `self.clusterJewel` (`data.clusterJewels.jewels[baseName]`; U0 typed it `Field<Option<Table>>`) | a `Table` copy with the Lua keys (`size`, `sizeIndex`, `minNodes`, `maxNodes`, `smallIndicies`, `notableIndicies`, `socketIndicies`, `totalIndicies`, `skills[id] = { name, icon, masteryIcon, tag, stats, enchant }`), `item_parse::clusterJewel_table` | ParseRaw (833-839), IT-C, PS-C. Nothing writes it, so a copy equals the alias |
| `self.pendingAffixList[i]` = `{ modId, table = self.prefixes|self.suffixes }` | `Table` whose `table` member aliases the item's `prefixes`/`suffixes` `Table` | ParseRaw only |
| `self.requirements = { }` (Item.lua:514) | the typed `requirements` table is wiped, not replaced (`LUA-ALIAS`): nothing keeps a reference to an item's requirements across a parse | — |

## 3. Replay seam for callees owned by later units

ParseRaw calls `itemLib.applyRange` (IT-C), `Item:BuildModList` / `Item:CanHaveMod` (IT-C) and
`modLib.parseMod` (MP0-MP6). IT-C depends on IT-A, so IT-A's L2 test cannot wait for them. The calc
doc's generic replay (§9.2, `pob_oracle::replay`, ORC-X) is not built. item_parse.rs therefore calls
those four through private wrappers that, **only with the `probe` feature** (which only pob-oracle
enables), consult `item_parse::replay::Hooks` (thread-local `Rc<dyn Fn>`s) and otherwise call the
owning unit's function. `tests/IT-A.rs` fills the hooks from PoB's recorded calls (keyed by their
arguments) and also checks that Rust makes the same calls in the same order. `itemLib.applyRange`
itself calls `modLib.parseMod` on its fallback path (ItemTools.lua:323); those nested calls belong
to the replayed applyRange and the oracle does not record them.

Proposed: keep the seam until ORC-X's replay can swap callees by function id; then ORC-X may replace
it. Once IT-C and MP1-MP6 land, `IT_A` should also run without hooks (the test already prints how
many recorded `parseMod` calls the native parser reproduces).

## 4. `Item.name` cannot be nil

`self.name = self.rawLines[l]` (Item.lua:470) is nil when the text ends right after the `--------`
that follows the rarity line (`"Rarity: Rare\n--------"`). U0 typed `name` as `Field<LStr>`; the port
stores `""` (marked `LUA-QUIRK`). Observable only by code that indexes or concatenates `name` of such
an item (PoB raises there, the port does not), e.g. BuildRaw. Proposed: leave as is (a degenerate
paste) or retype `name: Field<Option<LStr>>` if a caller ever needs the error.

## 5. IT-A needs IT-B (missing dependency in §11.1)

ParseRaw calls IT-B's `specToNumber`, `getCatalystScalar`, `normaliseModLine`, `sortCraftedModLines`,
`Item:CheckModLineVariant`, `Item:GetModLineVariantCount`, `Item:NormaliseVariantSelections`,
`Item:NormaliseQuality` and uses the module tables IT-B made public (`influenceItemMap`,
`lineFlags`/`is_lineFlag`, `catalystList`, `catalystDescriptorList`, `influenceInfo`,
`uniqueModStatOrder`, `ItemAffixes`; CCR-0032 §2). With U0's stubs, `specToNumber` returns `None`
(no quality, item level, requirements, …) and the module tables do not exist. `port/IT-B` (done,
CCR-0032) was therefore merged into `port/IT-A`.

```diff
-| **IT-A** Item ParseRaw | Item.lua 424-1645 | `pob-calc/src/classes/item_parse.rs` | U0, MP0 | …
+| **IT-A** Item ParseRaw | Item.lua 424-1645 | `pob-calc/src/classes/item_parse.rs` | U0, MP0, IT-B | …
```

## 6. Smaller notes

- `escapeGGGString` (Modules/Common.lua:1110-1116) is not in `scope.toml` and has no owner; ParseRaw
  is its only calc-side caller, so item_parse.rs ports it (`/// @pob Modules/Common.lua:1110-1116`).
- Dead Lua paths kept as comments: `self.base.utility_flask` (Item.lua:1042) and
  `self.base.tincture.buff` (1066-1073) never exist (`ItemBase` is `deny_unknown_fields` and has no
  such keys), so `tinctureBuffLines` stays nil.
- `LUA-ORDER` sites (string-keyed `pairs`, random in PoB; the port visits dump order):
  `pairs(self.affixes)` (591, 1505), `pairs(data.itemBases)` (971, ties of equal match length only),
  `pairs(self.mutatedLines)` (1552). The int-keyed `pairs` at 917/920/925 only write tables.
