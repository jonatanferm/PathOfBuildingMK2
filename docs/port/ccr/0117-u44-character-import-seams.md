# CCR-0117: U44 ImportTab B seams (unported tab methods, slot SetSelItemId, override nodes, parseMod, nil group fields)

Unit: **U44** (ImportTab B: `ImportTab.lua:789-1912`, `1925-1932`; CCR-0109 §5 fixes the range).
Status: **pending** (code on `port/U44` follows it). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. The other tabs' UI methods the import calls: `character::TabSeams`

Evidence: `ImportTab.lua:1157`, `:1386` (`itemsTab:DeleteItem`, ItemsTab.lua:1857-1903, U34),
`:1164`, `:1481` (`itemsTab:PopulateSlots`, ItemsTab.lua:1652-1656 + the slot dropdowns, U32/U34),
`:1165`, `:1482` (`itemsTab:AddUndoState`, U38), `:1818` (`slots[slotName]:SetSelItemId`,
ItemSlotControl.lua:72-84, U32), `:1484` (`skillsTab:AddUndoState`, U30), `:1410-1411`
(`skillsTab.controls.groupList.selIndex/selValue = nil`, U31), `:1412`
(`skillsTab:SetDisplayGroup()`, SkillsTab.lua:1275-1308, U30), `:1227`, `:1232`
(`configTab.varControls[var]:SetSel(i)` with the list option's selFunc, ConfigTab.lua:316-321,
U41). None of these UI tabs is ported yet.

Applied (U44's file, `tabs/import/character.rs`): `TabSeams` with `default_tab_seams()`,
`tab_seams()` / `set_tab_seams(..)` (the `ShellHooks` pattern, CCR-0104 §1). The defaults act on
the model as the Lua methods do:

* `items_delete_item`: ItemsTab.lua:1857-1903 over the model's slot tables, item sets,
  `orderList` and `treeTab.specList` (including PoB's `allocNodes[depNodeId] = nil` with the
  `ipairs` index, L1888), then `PopulateSlots` / `AddUndoState` through the seams;
* `items_populate_slots`: `model.itemsTab.PopulateSlots`;
* `items_add_undo_state` / `skills_add_undo_state`: `modFlag = true` (UndoHandler.lua:37) through
  `on_frame::ShellHooks::set_tab_flag` (`TabFlag::Items` / `TabFlag::Skills`); the undo buffers
  are the tabs';
* `items_slot_set_sel_item_id`: ItemSlotControl.lua:72-84 on the model's slot table (§2);
* `skills_clear_group_list_selection`: nothing (no group list control exists);
* `skills_set_display_group`: `model.skillsTab.SetDisplayGroup` (the model half, 1275-1278);
* `config_var_set_sel`: DropDownControl.lua:160-169 on the model's control-state table
  (`varControls[var].selIndex`, CCR-0037/0080; no search, so `GetDropCount` is `#list`) and
  ConfigTab.lua:317-320 (`input[var] = value.val`, `modFlag` through `TabFlag::Config`,
  `BuildModList`, `buildFlag`).

Proposed: U30/U31/U32/U34/U38/U41 replace their entries in `default_tab_seams` (one line each)
when their tabs land; the model-side defaults then become the model halves those units call.

## 2. `ItemSlotControl:SetSelItemId` is private in pob-calc

Evidence: `pob-calc/src/model/items_tab.rs:209` `fn ItemSlot_SetSelItemId` (BM-A ported it, not
`pub`); ImportTab.lua:1818 calls it on a slot. Work-around: `character::slot_set_sel_item_id`, the
same 10 lines over the slot table (marked CCR-0117 §2). Proposed (BM-A):

```diff
-fn ItemSlot_SetSelItemId(build: &BuildRef, slot: &Table, selItemId: Val) {
+pub fn ItemSlot_SetSelItemId(build: &BuildRef, slot: &Table, selItemId: Val) {
```

## 3. Account JSON as spec data

Evidence: ImportTab.lua:1144-1150 passes the decoded `skill_overrides[nodeId]` table itself to
`spec:ReplaceNode(override, tattooNode)` and stores it in `hashOverrides`; `ReplaceNode`
(PassiveSpec.lua:1697-1718, `pob_calc` `PassiveSpec::ReplaceNode(&NodeRef, &NodeRef)`) takes
nodes. Applied: the override is a fresh `Node { id: nodeIdNum }` whose `extra` holds the JSON's
fields other than those `ReplaceNode` overwrites (`stats`, …), then `ReplaceNode`. The overrides
and `mastery_effects` go to `ImportFromNodeList_impl` as `LjIntMap`s filled in the JSON's key
order (CCR-0052 §5; the JSON objects' order is PoB's per-process string hash order).
`alternate_ascendancy` naming no bloodline (`bloodline and bloodline.ascendClassId` = nil) is passed
as NaN, the spec's nil (`SelectSecondaryAscendClass` reads NaN as nil). `spec.jewel_data` and
`spec.extended_hashes` are `spec.extra` tables (CCR-0072 §7). No contract change needed.

## 4. `data.gemGrantedEffectIdForVaalGemId` (ImportTab.lua:1839)

A dynamic root (contracts.md lists ImportTab.lua:1839 as its reader). Read with
`data.other["data.gemGrantedEffectIdForVaalGemId"]` + `DataTable::field` + `Data::resolve`
(`character::gem_granted_effect_id_for_vaal_gem_id`). No change needed unless a second reader
appears (then type it, contracts.md).

## 5. `modLib.parseMod` is not reachable through `crate::model`

Evidence: ImportTab.lua:1770 (flavour text lines are kept only when `modLib.parseMod(line)`
returns a list). `pob_calc::classes::item_tools::parseMod` is `pub(crate)`; `model.rs` (U15, the
only door to the calc plan, D15) re-exports no parser. Work-around: `pob-modparser` added to
`pob-app`'s dependencies and `pob_modparser::parseMod` called directly (marked CCR-0117 §5).
The other mod lines of ImportItem need no parse: their `mods`/`extra` are dropped by
`item:BuildAndParseRaw()`, whose ParseRaw rebuilds every mod line from the raw text.
Proposed (U15, `pob-app/src/model.rs`):

```diff
+/// `modLib.parseMod(line, isComb)` (Modules/ModParser.lua via pob-modparser).
+pub use pob_modparser::parseMod;
```

## 6. BM-B: `SkillsTab:Save` writes `tostring(NaN)` for a nil `mainActiveSkill`

Evidence: ImportTab.lua:1855 creates socket groups without `mainActiveSkill` /
`mainActiveSkillCalcs` (nil; CalcSetup.lua:1883/1888 sets them only on the main group);
SkillsTab.lua:596-597 saves `tostring(nil)` = `"nil"`. The model keeps a nil there as NaN
(`calc_setup/init_env_skills.rs` module docs, CCR-0019 convention), and
`skills_tab.rs` `Save` writes `tostring_num(mainActiveSkill)`, which is not `"nil"` for NaN.
The u44 test compares both spellings as `nil` (counted in its output). Proposed (BM-B,
`model/skills_tab.rs` Save, L596-597): `tostring_opt_nan(socketGroup.mainActiveSkill.get())`
(the helper the file already has for gem levels).

## 7. Harness

`tests/u44_character.rs` is a copy of U43's `tests/u43_import.rs` harness (test files cannot
share modules) with: the u44 directives, the items/skills/config `modFlag` stand-ins on the
Rust side (§1), the §6 normalisation, and a site-import probe. CCR-0109 §8 (Net completions held
back by `Driver::frame`) still applies; the copy keeps u43's `DeferredNet` work-around.
