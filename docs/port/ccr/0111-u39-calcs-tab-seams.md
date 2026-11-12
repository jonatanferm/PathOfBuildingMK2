# CCR-0111: U39 Calcs tab seams (BuildOutput UI tail, the breakdown seam, shared section tables)

**Status: applied (CCRDOC, 2026-11-12; code `e4f87ce` Merge port/U39). §1 was applied by U40 (CCR-0116 §2); §2 `CalcsHooks` (ui doc §10.5) and §3-§6 are as built. §3's optional U21 proposal was not done (U21 keeps its own section list).**

Unit: **U39** (CalcsTab, `Classes/CalcsTab.lua:1-162, 226-440`, `Classes/CalcSectionControl.lua
1-550`, `Modules/CalcSections.lua` as data). Filed status: **pending** (code on `port/U39` follows it).
Number reserved with `scripts/port/next_ccr.sh`.

## 1. The UI tail of `CalcsTab:BuildOutput` is not called by anyone

Evidence: `CalcsTab.lua:464-467`:

```lua
if self.displayData then
	self.controls.breakdown:SetBreakdownData()
	self.controls.breakdown:SetBreakdownData(self.displayData, self.displayPinned)
end
```

runs inside `BuildOutput`, i.e. at every recalc (`Build.lua:695` in Init, `:1266` in OnFrame).
calc:PWR's `power::BuildOutput` skips it (`power.rs:287`, "L464-467 UI"), U15's facade
documents that it "belongs to the CalcsTab unit (U39)" (`pob-ui-oracle/src/lua_calc.rs` module
doc), and U20's two recalc sites (`build/init.rs` L695, `build/on_frame.rs` L1266) call only
`calc.build_output(&model)`.

U39 provides the function: `pob_app::tabs::calcs::core::build_output_ui_tail(cx)` (a no-op when
the tab does not exist or nothing is displayed). Proposed (U20's files, one call each, after
`build_output` returned `Ready` and before the next statement):

```diff
 // build/on_frame.rs, L1266
         let built = cx.build_mut().calc.build_output(&model).is_ready();
         init::flush_window_titles(cx);
         if built {
+            // CalcsTab.lua:464-467 (the UI tail of BuildOutput)
+            crate::tabs::calcs::core::build_output_ui_tail(cx);
 // build/init.rs, L695
     let _ = cx.build_mut().calc.build_output(&calc_model);
+    crate::tabs::calcs::core::build_output_ui_tail(cx);
```

Not applied by U39 (U20's statements, not a hook arm). Until U40 lands the breakdown is U39's
stand-in (§2), for which the two calls are unobservable: they leave `sourceData`/`pinned` as they
were (at the end of every CalcsTab Draw `sourceData == displayData` and `pinned ==
displayPinned`) and only clear `breakdown.shown`, which nothing reads before CalcsTab:Draw sets
it again (the stand-in is never under the mouse and draws nothing). With U40's real control the
missing call leaves a pinned breakdown showing the previous calc's numbers until the user
re-hovers, so U40 should apply this diff (or ask U20 to) together with its hooks.

## 2. The `CalcBreakdownControl` seam for U40

`self.controls.breakdown = new("CalcBreakdownControl"):CalcBreakdownControl(self)` (L156) and the
calls CalcsTab/CalcSectionControl make on it go through `tabs::calcs::core::CalcsHooks`
(thread-local, `calcs_hooks()` / `set_calcs_hooks()`, the CCR-0098 §3 pattern):

| Entry | Lua | Default (until U40) |
|---|---|---|
| `breakdown_ctor(cx, calcsTab) -> CtrlId` | L156, stored as `controls.breakdown` | `BreakdownStandIn` (never shown, no drawing, not under the mouse, ignores keys) |
| `set_breakdown_data(cx, bd, Option<&Table>, pinned)` | `breakdown:SetBreakdownData(displayData, pinned)` (L366, 380, 393, 465-466) | CalcBreakdownControl.lua:61-67 (`pinned`, `sourceData`, `shown = false`) |
| `breakdown_source_data(cx, bd) -> Option<Table>` | `breakdown.sourceData` (L352) | the stand-in's `source_data` |

`breakdown:Draw(viewPort)` (CalcSectionControl.lua:429) and the framework calls (DrawControls,
OnKeyDown via `SelectControl`, `IsMouseOver`) are dynamic dispatch on the control, so U40's widget
gets them without a hook. U40 replaces the three defaults in `default_calcs_hooks` (one line
each). The data U40 reads: `displayData` is the cell table (`colData`, a calc `Table` with the
module's fields plus `x`, `y`, `xOffset`, `yOffset`, `width`, `height` and `calcSection`, a
`Val::Obj(Rc<CalcSectionRef>)` naming the section control); `tabs::calcs::core::check_flag(cx, obj,
actor, player)` and `CalcsTab::clear_display_stat(cx, tab)` are public.

## 3. One subsection table for the UI and the model

`CalcsTab:Load/Save` (calc:BM-A, CCR-0080 §3) read `self.sectionList[i].subSection[j].collapsed`
from BM-A's side table (`model::CalcsTab::sectionList()`, built by `ctor_model` from the same
module data). The UI's CalcSectionControl must toggle that same value. Applied (U39's files): the
tab hands each section the model's `subSection` list after copying the module's subsection fields
(`label`, `defaultCollapsed`, `data`) into the model's tables (`core.rs::model_sub_section`, keyed
by position and checked by section id), so `subSec.collapsed` is one value, as in Lua. The UI does
not repeat CalcSectionControl.lua:34 and :49 (the model half's): the model loads the build before
the UI constructors run, so L49 would undo the loaded collapsed states. The oracle test T4-checks
the `Calcs` section after collapsing three subsections (`tests/u39/b_collapse.pobscript`).

Proposed (U21, optional): `stats.rs`'s DIVERGENCE(section-list) builds the breakdown index from
its own copy of the module data; once the Calcs tab exists, `model.calcsTab.sectionList()`'s
subsections carry `data` (the cells U39 annotates, including `calcSection`), so the index could
read them and drop its copy.

## 4. Hook arms (CCR-0106 §2)

Added, one line each: `init::default_hooks().calcs_tab = ctor_hook`;
`on_frame::default_shell_hooks()` arms `BuildTab::Calcs => draw_tab`, `TabFlag::Calcs =>
mod_flag / set_mod_flag`, and `calcs_breakdown` (Build.lua:1213-1215: the tab's
`controls.breakdown` and the "pinned overlay cell" condition) instead of the "not ported"
default.

## 5. `ResetUndo` at the end of `CalcsTab:Load`

`CalcsTab.lua:200` (`self:ResetUndo()`, UI) runs inside Load, which calc:BM-A runs inside
`Init_model` before the UI constructors. U39's `ctor_hook` calls `ResetUndo` right after
constructing the tab when the build file has a `Calcs` section (module doc
DIVERGENCE(reset-undo)); nothing writes `calcsTab.input` in between.

## 6. Markers

CalcsTab.lua 24-40, 45, 151-154, 370-375 and CalcSectionControl.lua 20-49 keep calc:BM-A's
markers, and 41-42 BM-A's `@pob-skip` (whose reason names this unit); U39's markers cover the rest
of its ranges without overlap (`provenance.py coverage --range Classes/CalcsTab.lua:1-773` and
`--range Classes/CalcSectionControl.lua:1-551`: 100%, 0 covered twice). The UI statements inside
BM-A's ranges are in `core.rs`/`section.rs` with `// L<n>` comments and no marker.
