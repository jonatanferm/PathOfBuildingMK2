# CCR-0080 (BM-A): build XML load/save — ranges outside the row, side tables, frozen-signature gaps

**Status: proposed (BM-A, 2026-11-09).** No frozen file was changed: every item below is worked
around inside BM-A's own files (`crates/pob-calc/src/model/{build_xml,items_tab,tree_tab,calcs_tab,xml}.rs`),
marked `CCR-0080`.

- **Raised by:** BM-A
- **Affects:** calc doc §5.8 (`Build`, `ItemsTab`, `TreeTab`, `CalcsTab` in `model/mod.rs`, U0),
  §11.1 BM-A row; ui doc U20/U21 (Build shell, display stats), U33/U34 (ItemsTab), U42 (ConfigTab);
  ORC-X `pob-oracle/src/testkit.rs` (`load_build`); CCR-0019 §10, CCR-0052 §8-§9
- **Pin:** 16de4b82d

## 1. Lua ranges BM-A needs outside its unit row

The in-range functions call model code that the ui doc assigns to UI units (or nobody). BM-A ported
the model halves, with `@pob` markers, so that a headless build loads like PoB's:

| Lua | Why | Rust |
|---|---|---|
| Build.lua 64-110, 603-698 (`Build:Init`, model half) | `loadBuildFromXML` is "Build:Init + LoadDB" (contracts.md#model-census): initial state, LoadDB, tab constructors, section loaders (trees deferred), PostLoad, UpdateSocketGroups, first BuildOutput | `build_xml::Init_model` |
| ItemsTab.lua 37, 101-108, 140-235, 1196-1215 (constructor) | `self.slots` / `orderedSlots` / `sockets` and the first item set exist before Load in PoB; CalcSetup (CS2) iterates `orderedSlots` and reads `slot.slotName/nodeId/weaponSet/selItemId/active/parentSlot/slotNum` | `ItemsTab::ctor_model` |
| ItemSlotControl.lua 21-70, 72-84, 86-123 | `SetSelItemId` / `Populate` are what `PopulateSlots`, `SetActiveItemSet`, `AddItem` call; Populate clears invalid selections and deactivates abyssal sockets (model state CS2 reads) | `items_tab::{new_ItemSlotControl, ItemSlot_SetSelItemId, ItemSlot_Populate}` |
| ItemsTab.lua 2560-2605 (`IsItemValidForSlot`), 2978-2993 (`getAnoint`), 70-74 (`isAnointable`) | called by Populate / EquipItemInSet / AddItem / getMissingAnointCount (CalcSetup.lua:1011) | `ItemsTab::{IsItemValidForSlot, getAnoint}`, `isAnointable` |
| TreeTab.lua 26-38 (constructor) | the first spec and `SetActiveSpec(1)` | `TreeTab::ctor_model` |
| CalcsTab.lua 24-40 (+45, 147-150, 370-375; CalcSectionControl.lua 20-49) | `input` defaults (`skill_number`, `misc_buffMode`) and the section list Load/Save use | `CalcsTab::ctor_model`, `CalcsTab::sectionList` |
| SkillsTab.lua 89, 95-96, 423-426 | CCR-0019 §10 asked BM-A's `Build:Init` to run them | `build_xml::skillsTab_ctor_model` |
| ConfigTab.lua 672-681 (constructor, the `varControls` half) | Build.lua:645 runs `BuildModList` right after the constructors; option applies index `configTab.varControls[var]` (`enemySizePreset` → `enemyRadius:SetPlaceholder`, map affixes → `multiplierMapModTier.selIndex`, `enemyIsBoss`/`presetBossSkills`), which raise on a missing control. This is the population CCR-0037 proposes for BM-B; CCR-0038's methods stay CO-B's local helpers | `build_xml::configTab_varControls_model` (one control-state table per option with a `var`: `_className`, `enabled`, `buf`, `state`, `placeholder`, `list`, `selIndex`) |
| ConfigTab.lua 1205-1240 (`ImportCalcSettings`) | Build.lua:687; no calc or UI unit lists it | `build_xml::ImportCalcSettings` (§6) |

**Slot model.** A slot is a `Table` with the `ItemSlotControl` fields the model reads: `slotName`,
`slotNum`, `label`, `nodeId`, `weaponSet`, `selItemId`, `active`, `inactive`, `parentSlot` (the
parent slot table), `abyssalSocketList`, `items` (Populate's id list), `controls.activate.state`
(flask slots). The `shown` closures (ItemsTab.lua 158-208, ItemSlotControl.lua 33) are the Rust-only
key `$shown` (`weapon`, `weaponSwap`, `abyssalSwap`, `abyssalWeapon`, `graft`, `ring3`, `default`)
interpreted by `slot_IsShown`. `impl Drop for ItemsTab` breaks the `parentSlot` ↔
`abyssalSocketList` cycles (calc doc §3.3 rule 4). U33/U34 wrap these tables in their controls.

**Proposed diff:** calc doc §11.1 BM-A row, add the ranges above to the Lua column; ui doc U33/U34:
"the slot tables are calc:BM-A's (`ItemsTab::ctor_model`); the controls wrap them".

## 2. Fields the frozen model structs lack (per-build side table)

`Build` has no field for `characterLevelAutoMode`, `timelessData`, `importLink`, `xmlSectionList`,
`dbFileName`, `buildName`, `modFlag`; `ItemsTab` none for `showStatDifferences`,
`tradeQuery.statSortSelectionList`, `lastSlot`, `slotOrder`; `TreeTab` none for `showConvert`
(CCR-0052 §9). BM-A keeps them in `build_xml::build_extra(build) -> Table` (a thread-local side
table keyed by the build's `Weak`, released with the build; `build_extra_ref(&Build)` for `&self`
methods) with public accessors (`characterLevelAutoMode`, `timelessData`, `importLink`,
`xmlSectionList`, `dbFileName`, `ItemsTab::showStatDifferences`, `ItemsTab::statSortSelectionList`,
`TreeTab::showConvert`).

**Proposed diff (model/mod.rs):** `pub extra: Table` on `Build` (and the typed fields
`characterLevelAutoMode: Cell<bool>`, `timelessData: Table`), `pub showStatDifferences: Cell<bool>`
on `ItemsTab`, `pub showConvert: Cell<bool>` on `TreeTab`; BM-A then moves the side table onto them.

## 3. `calcsTab.sectionList`

CalcsTab Load/Save read and write `self.sectionList[i].subSection[j].collapsed` (CalcsTab.lua
186-194, 215-223), the `CalcSectionControl`s of the constructor. `CalcsTab` has no such field; BM-A
keeps the model half (`{ id, subSection = { { id, collapsed } } }`, ids from `label:gsub("%W", "")`,
`collapsed = defaultCollapsed`, possibly nil) in a per-tab side table built from
`data.calcSections`. **Proposed:** `pub sectionList: Table` on `CalcsTab`, owned by BM-A; U-calcs
wraps it.

## 4. `Load` return values

`TreeTab:Load` (unknown tree version, TreeTab.lua:515) and `CalcsTab:Load` (malformed Input/Section,
CalcsTab.lua:169/179/184) return `true`, and `Build:Init` closes the build (Build.lua:665-677). The
frozen `Load`s return `()`. Workaround: `TreeTab::Load_ret` / `CalcsTab::Load_ret -> bool` hold the
bodies; `Init_model` uses them. **Proposed:** `Load(...) -> bool` on both.

## 5. Item and item-set ids are `u32` map keys

`ItemsTab.items: Shared<IndexMap<u32, ItemRef>>`, `itemSets: Shared<IndexMap<u32, Table>>` and
`orderList`/`itemSetOrderList: Shared<Vec<u32>>` cannot hold a non-integral or negative id, which a
Lua table accepts. The port raises a `LuaError` naming CCR-0080 there (never seen in the corpus;
PoB itself only creates positive integer ids). `id = nil` raises "table index is nil" as in Lua.

Two other unreachable-by-corpus inputs are also left as a raise rather than a reproduction.
`CalcsTab:Save` (CalcsTab.lua:211) writes `tostring(v)` for a table or function `input` value, which is
an address string (`table: 0x...`) no port can reproduce, so the port raises a `LuaError` there. An
integer or boolean `input` key becomes an `Input name="..."` attribute in the port, while Lua's
`composeNode` (xml.lua:122-125) would fail the save ("value for attribute 'name' ... is not a
string"). The Calcs tab only ever writes string keys, so neither case arises from PoB itself.

## 6. `ConfigTab:ImportCalcSettings` (ConfigTab.lua 1205-1240)

Called by `Build:Init` (Build.lua:687) when the config input is empty after load (old builds). No
unit lists it; BM-A ported it next to `Init_model` (it calls BM-B's `UpdateControls` and CO-C's
`BuildModList`). **Proposed:** list it under BM-B (or BM-A) in calc doc §11.1.

## 7. `xml.ParseXML` result shape

Lua returns a pseudo-root table whose array part holds the top-level element nodes *and* text runs;
`dbXML[1]` may therefore be a string (`LoadDB` then reports "root element missing"). The frozen
`ParseXML -> (Vec<XmlNode>, Option<String>)` keeps only element nodes. Added
`xml::ParseXML_root -> Result<XmlNode, String>` (pseudo-root with `elem = ""`, both kinds of children);
`LoadDB` uses it. The `empty = true` flag Lua sets on empty elements is not stored: its one
reader, `ConfigTab:Load` (ConfigTab.lua:885 `if xml.empty`), is ported as
`xml.children.is_empty()` (BM-B, config_tab.rs), which is equivalent because xml.lua sets `empty`
exactly when the element has no children (xml.lua:64 self-closing tag, :78 `#top == 0`). `composeNode`'s type errors (non-string element names / attribute values) cannot occur with
a typed `XmlNode`; `SaveXMLFile` returns `None` on success (Lua `true`) and `Some(err)` on failure.

## 8. `Build:Save` display stats and the UI-only savers

`Build:Save` (Build.lua 1033-1080) writes `PlayerStat`/`FullDPSSkill`/`MinionStat` from
`Modules/BuildDisplayStats` (ui doc U21, in `pob-app`, invisible to pob-calc) and needs
`calcsTab.mainEnv`. BM-A calls a hook the UI registers (`build_xml::set_save_stats_hook`) at that
point; with no hook nothing is written (PoB's Load never reads those elements). The savers `Notes`,
`TreeView` and `Import` (Build.lua:629-639) are UI tabs (U20): until U20 provides them, `Init_model`
keeps the loaded section and `SaveDB` writes it back unchanged. `SaveDB` iterates the savers in the
constructor's order (PoB: `pairs`, random per process).

## 9. Entry points

`loadBuildFromXML` runs `Build:Init` completely, including the first `calcsTab:BuildOutput()`
(`modules::power::BuildOutput`, PWR; a stub until PWR lands). `loadBuildModelFromXML -> Result` stops
before it (tests that only need the model). `pob-oracle/src/testkit.rs::load_build` (ORC-X) calls
`Build::new` + `LoadDB`, which in PoB only parses the XML, loads the `<Build>` section and stores the
other sections; it no longer gives the "empty Build:Init model" its doc comment describes once
LoadDB is real. **Proposed:** testkit switches to `loadBuildModelFromXML` (or keeps `Build::new` for
pure injection).

## 10. Dependencies

PS-C (`BuildClusterJewelGraphs`) and CO-C (`ConfigTab:BuildModList`) are merged; `tests/BM-A.rs`
has no PS-C or CO-C bucket and fails on every difference on all 57 corpus builds, with one
self-emptying exception for **PS-B** (`BuildAllDependsAndPaths`, PassiveSpec.lua:1092-1695, still
an `unported!` stub on rust-port; port/PS-B 17bdabd is unmerged). PoB's pass deallocates nodes
that are disconnected, ignored or masteries without an effect; the stub removes nothing. Two
legacy upstream builds are affected (`upstream_Mirage_Archer_Toxic_Rain` spec 1 on the 3_6 tree,
`upstream_Dual_Wield_Cospris_CoC` on 3_13). The test excuses only: a spec's allocated set that is
a strict superset of PoB's, and a `<Tree>` section equal once the Spec `nodes` lists (Rust's a
superset of PoB's) and URL texts are left out, and only on a build whose Rust load hit the
PS-B stub (`stub_report`). Once PS-B merges, the stub is no longer hit and the bucket is empty.
Merging port/PS-B into this branch locally made both builds pass with zero differences.

If BM-B later owns the ConfigTab constructor's `varControls` half (CCR-0037), it replaces
`build_xml::configTab_varControls_model`, and `Init_model` calls BM-B's version instead.
