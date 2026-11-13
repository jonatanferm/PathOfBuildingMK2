# CCR-0117: U44 ImportTab B seams (unported tab methods, slot SetSelItemId, override nodes, parseMod, nil group fields)

**Status: applied (CCRDOC, 2026-11-12; code `f06df6f` Merge port/U44): §1 `TabSeams` (U53 filled them, CCR-0145 §5), §3, §4 and §7; §6 was fixed by U53 (CCR-0145 §3). Rejected because they were not built: §2 (`ItemSlot_SetSelItemId` stays private), §5 (pob-app depends on `pob-modparser` directly) and §8 (`sg_bridge::deliver_subscripts` still frees the slot before the callback; the u44 harness keeps its stub).**

Unit: **U44** (ImportTab B: `ImportTab.lua:789-1912`, `1925-1932`; CCR-0109 §5 fixes the range).
Filed status: **pending** (code on `port/U44` follows it). Number reserved with
`scripts/port/next_ccr.sh`.

## 1. The other tabs' UI methods the import calls: `character::TabSeams`

Evidence: `ImportTab.lua:1157`, `:1386` (`itemsTab:DeleteItem`, ItemsTab.lua:1857-1903, U34),
`:1164`, `:1481` (`itemsTab:PopulateSlots`, ItemsTab.lua:1652-1656 + the slot dropdowns, U32/U34),
`:1165`, `:1482` (`itemsTab:AddUndoState`, U38), `:1818` (`slots[slotName]:SetSelItemId`,
ItemSlotControl.lua:72-84, U32), `:1484` (`skillsTab:AddUndoState`, U30), `:1410-1411`
(`skillsTab.controls.groupList.selIndex/selValue = nil`, U31), `:1412`
(`skillsTab:SetDisplayGroup()`, SkillsTab.lua:1275-1308, U30), `:1227`, `:1232`
(`configTab.varControls[var]:SetSel(i)` with the list option's selFunc, ConfigTab.lua:316-321,
U41). Of these only U32 (the slot controls) is merged; its `ItemsTabHooks` (slot_helper.rs)
cover PopulateSlots but raise "not ported" for DeleteItem (U34) and AddUndoState (U38).

Applied (U44's file, `tabs/import/character.rs`): `TabSeams` with `default_tab_seams()`,
`tab_seams()` / `set_tab_seams(..)` (the `ShellHooks` pattern, CCR-0104 §1). The defaults act on
the model as the Lua methods do:

* `items_delete_item`: ItemsTab.lua:1857-1903 over the model's slot tables, item sets,
  `orderList` and `treeTab.specList` (including PoB's `allocNodes[depNodeId] = nil` with the
  `ipairs` index, L1888), then `PopulateSlots` / `AddUndoState` through the seams;
* `items_populate_slots`: U32's `slot_helper::items_hooks().populate_slots` (the model half, then
  the slot dropdown lists);
* `items_add_undo_state` / `skills_add_undo_state`: `modFlag = true` (UndoHandler.lua:37) through
  `on_frame::ShellHooks::set_tab_flag` (`TabFlag::Items` / `TabFlag::Skills`); the undo buffers
  are the tabs';
* `items_slot_set_sel_item_id`: U32's `tabs::items::slot::set_sel_item_id` (ItemSlotControl.lua:
  72-84 on the model's slot table, §2);
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
`pub`); ImportTab.lua:1818 calls it on a slot. Work-around: U32's copy
`tabs::items::slot::set_sel_item_id` (CCR-0107 §2), also used by the DeleteItem stand-in.
Proposed (BM-A, as CCR-0107 §2):

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

This is not only cosmetic: the LuaCalcBridge (U15) loads `build:SaveDB()` into PoB, where
SkillsTab.lua:453 `tonumber("nan")` is NaN (LuaJIT parses it), not nil, so `or 1` does not apply
and Build.lua:1655 `displaySkillList[NaN]` raises "attempt to index local 'activeSkill'" on the
first recalc after an import ("build load failed"). Work-around in the u44 harness
(`NilGroupFieldBridge`, marked CCR-0117 §6): around each bridge call a NaN group field is set to
the 1 that `"nil"` loads as and gets its NaN back afterwards unless CalcSetup.lua:1879-1888 set
it in the bridge's VM (index == the MAIN / CALCS env's main group, non-empty skill list). The
same wrapper hands back two other SkillsTab:Load defaults the bridge adopts into the model and
real PoB never applies to an in-process group: a gem's nil `count` (SkillsTab.lua:501 `or 1`;
ImportTab.lua:1843 sets none) and a granted skill's empty `nameSpec` (SkillsTab.lua:489-493
names a skillId-only gem; CalcSetup.lua:1487-1492 + SkillsTab.lua:1166 leave it ""). Those two
are U15 bridge-adoption limits (`lua_calc.rs` `copy_gem`), not model bugs; listed for U15.

## 7. Harness

`tests/u44_character.rs` is a copy of U43's `tests/u43_import.rs` harness (test files cannot
share modules) with: the u44 directives, the items/skills/config `modFlag` stand-ins on the
Rust side (§1), the §6 normalisation and bridge wrapper, and a site-import probe. CCR-0109 §8
(Net completions held back by `Driver::frame`) still applies; the copy keeps u43's
`DeferredNet` work-around. Also:

* U25 stand-in for `BuildHooks::tree_set_active_spec`: the default hook runs only the model half
  of TreeTab:SetActiveSpec (`model/tree_tab.rs` marks L556 `SetWindowTitleWithBuildClass` and L557
  `UpdateClassDropdowns` as UI), so ImportTab.lua:1210 sent no "(Hierophant + Farrul Bloodline)"
  title. The stand-in adds `init::set_window_title_with_build_class` and
  `update_class_dropdowns`; U25's hook must do the same. `import_passive_tree_and_jewels` flushes
  the queued titles after the hook (the `init.rs` pattern).
* PoB nondeterminism (calc doc D8): ImportTab.lua:1384 `pairs(self.build.itemsTab.slots)` is
  observable (DeleteItem's PopulateSlots empties the quiver slot once the bow is deleted, so
  Maloney's Mechanism survived "Delete equipment" in some PoB processes and not in others). The
  Lua side iterates the slots in `__canonPairs` order there (creation order = the Rust insertion
  order). `Spec masteryEffects` follows `pairs` of the JSON object (ImportTab.lua:1136, string
  keys) and is compared as a set (counted).

## 8. pob-ui-oracle (U8): the sub-script slot is freed before the download callback runs

Evidence: `crates/pob-ui-oracle/src/sg_bridge.rs` `deliver_subscripts` takes the slot
(`Option::take`, freeing it) and then calls `launch:OnSubFinished(id, ...)`. A download started
inside that callback (ImportTab.lua:948, the profile page requested from the get-characters
callback of DownloadSiteCharacterList) gets the lowest free slot, i.e. the same id;
Launch.lua:206-220 then ends with `self.subScripts[id] = nil`, dropping the new entry, and the
next frame's `OnSubFinished` fails at Launch.lua:224 ("attempt to index a nil value"). Real
SimpleGraphic keeps the slot until the callback returned (PoB's own nested downloads work).
Work-around (u44 harness Lua stub, marked CCR-0117 §8): during OnSubFinished/OnSubError a
re-registration of `id` survives the trailing nil. Proposed (U8, `sg_bridge.rs`): keep the slot
occupied while the callback runs, e.g.

```diff
-        let Some(r) = b.subs.borrow_mut().get_mut(slot).and_then(Option::take) else {
+        let Some(r) = b.subs.borrow_mut().get_mut(slot).and_then(|s| s.as_mut().map(std::mem::take)) else {
             continue;
         };
         ...call OnSubFinished / OnSubError...
+        b.subs.borrow_mut()[slot] = None;
```

(the response type needs `Default`, or the slot holds a "running" marker separately).
