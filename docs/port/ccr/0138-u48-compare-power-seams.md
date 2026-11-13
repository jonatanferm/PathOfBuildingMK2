# CCR-0138: U48 CompareTab C seams (copies of U47/U49 helpers, the abyssal slot `shown`, the compare id)

**Status: applied (CCRDOC, 2026-11-12; code `b9888df` Merge port/U48): §2 (`abyssal_slot_shown`), §3, §4 and §5. Not built: §1's switch to U49's now-public methods (`power.rs` keeps its five private copies) and §3's deletion of U46's unused `init::State::comparePowerCompareId`.**

**FUP7 (2026-11-13) built §1 (power.rs calls U49's `ShouldShowRing3`/`AddAbyssSockets` and U47's group helpers) and §3's deletion of `init::State::comparePowerCompareId`.**

Unit: **U48** (`Classes/CompareTab.lua:2516-3161`, `Classes/ComparePowerReportListControl.lua`,
`Classes/CompareCalcsHelpers.lua`; files `pob-app/src/tabs/compare/{power.rs,power_list.rs,calcs_helpers.rs}`).
Filed status: **pending** (code on `port/U48` follows it, with the local workarounds marked `// CCR-0138`).
Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. Three local workarounds for methods and fields that other units own,
plus the one-line hook install CCR-0136 §1 asks for.

## 1. Copies of other units' CompareTab methods (U47, U49)

Evidence: `ComparePowerBuilder` calls `self:ShouldShowRing3(compareEntry)` and
`self:AddAbyssSockets(compareEntry, baseSlots, true)` (CompareTab.lua:2554, 2559, 2719, 2724;
defined at :3668-3674 and :3676-3693, U49's range), and `self:GetGemGrantedEffect(gem)`,
`self:GetSocketGroupSignature(group)`, `self:GetSocketGroupLabel(group)` (:2587, 2594, 2909,
2913, 2935, 2973, 2981; defined at :2466-2513, U47's range). Neither U47 nor U49 has landed.

Workaround (power.rs, marked CCR-0138 §1): private free functions `should_show_ring3`,
`add_abyss_sockets`, `get_gem_granted_effect`, `get_socket_group_signature`,
`get_socket_group_label`, statement for statement as the Lua, without `@pob` markers (the ranges
stay the owners'; free functions so the owners' inherent `CompareTab::ShouldShowRing3` etc.
cannot clash at merge).

Proposed: when U47/U49 land, power.rs calls `CompareTab::ShouldShowRing3`/`AddAbyssSockets`/
`GetGemGrantedEffect`/`GetSocketGroupSignature`/`GetSocketGroupLabel` and the copies are
deleted (five call sites, one line each).

## 2. `slot.shown()` of a model item slot (calc:BM-A)

Evidence: AddAbyssSockets reads `self.primaryBuild.itemsTab.slots[name].shown()` and
`comparison.itemsTab.slots[name].shown()` (CompareTab.lua:3685-3686) for both builds; the
comparison build has no UI ItemSlotControls in the port (its ItemsTab is a model). The closures
are ItemsTab.lua:173-175 / 195-197 and ItemSlotControl.lua:34-36; calc:BM-A interprets them from
the slot's `$shown` key in `pob-calc-core/src/model/items_tab.rs:177` `slot_IsShown`, which is
private.

Workaround (power.rs `abyssal_slot_shown`, marked CCR-0138 §2): the same `$shown` match for the
three kinds an abyssal socket can have (`abyssalSwap`, `abyssalWeapon`, default).

Proposed (calc:BM-A's file): `pub fn slot_IsShown`, and U48/U49 call it.

## 3. `self.comparePowerCompareId` holds the entry, not a number

Evidence: CompareTab.lua:3131-3132 `if self.comparePowerCompareId ~= compareEntry then
self.comparePowerCompareId = compareEntry`: the field holds the CompareEntry table (identity
compare; the reference keeps the entry alive). U46 declared it `Option<usize>` in `init::State`
(`init.rs:484`), which can only hold an index or an address (an address can be reused after a
RemoveBuild, so a new entry could compare equal to a removed one).

Workaround: U48's `power::State::comparePowerCompareId: Option<CompareEntryRef>` (compared with
`Rc::ptr_eq`); U46's field stays unused (its constructor still writes `None`, as L188 writes nil).

Proposed (U46's file): delete `init::State::comparePowerCompareId`.

## 4. The hook install (CCR-0136 §1)

`init.rs` `default_compare_hooks`: `power_report_list: Rc::new(super::power_list::ctor_hook)`
(one line). `PendingPowerReportList` stays: `tests/u46_compare.rs` now installs it as its
`power_report_list` (its Lua stub replaces the list's `Draw` with an empty one, so the Rust side
must draw nothing too; three lines in U46's test, `rust_stubs`).

## 5. Notes for U49 (DrawSummary) and U50 (DrawCalcs)

* DrawSummary's list lines (CompareTab.lua:3274-3289) are
  `ComparePowerReportListControl::SetProgress(cx, list, progress)`,
  `ComparePowerReportListControl::SetReport(cx, list, st.comparePowerStat, power::results(cx, me))`
  and `ComparePowerReportListControl::set_impact_label(cx, list, &stat.label)`
  (`listControl.impactColumn.label = …`); `self:RunComparePowerReport(compareEntry)` is
  `CompareTab::RunComparePowerReport(cx, me, &entry)`. `tests/u48_compare_power.rs`'s stand-in
  Draw is a template.
* `calcsHelpers.X` is `tabs::compare::calcs_helpers::X`; `DrawCalcsTooltip` takes the tooltip
  (`self.calcsTooltip`, lend it out of `init::State`), the primary model, the label, `colData`
  (a CalcSections table of `tabs::calcs::sections_data`), the row label and rect, the viewport
  and the entry; the row-matching helper (CompareTab.lua:4639-4675) uses `TabulateMods`,
  `ModRowKey` (rows are `{ value, mod }` tables) and `GetBreakdownLines`.
