# CCR-0116: U40 CalcBreakdownControl seams (hook arms, the BuildOutput UI tail, U23/U38 hooks)

**Status: applied (CCRDOC, 2026-11-12; code `572bc74` Merge port/U40): §1, §2, §4 `BreakdownHooks` (U53 filled the node-viewer entries, CCR-0145 §5), §5 (`known_diffs.toml` U40-1) and §6. §3's optional `BuildHooks::breakdown` argument was not added (DIVERGENCE(calcs-tab-late)).**

Unit: **U40** (CalcBreakdownControl, `Classes/CalcBreakdownControl.lua:1-789`; also
`Modules/Common.lua:836-871`, the `format*` family, ui doc §13 item 15). Filed status: **pending**
(code on `port/U40` follows it). Number reserved with `scripts/port/next_ccr.sh`.

## 1. Hook arms installed (one line each, as CCR-0098 §3 / CCR-0104 §1 / CCR-0111 §2 propose)

Applied in other units' files:

| File (owner) | Entry | Now |
|---|---|---|
| `tabs/calcs/core.rs` (U39) `default_calcs_hooks` | `breakdown_ctor`, `set_breakdown_data`, `breakdown_source_data` | `breakdown::{calcs_breakdown_ctor, calcs_set_breakdown_data, calcs_breakdown_source_data}` (the real control replaces `BreakdownStandIn`, which stays in U39's file; the u39 harness uses it, §6) |
| `build/on_frame.rs` (U20) `default_shell_hooks` | `breakdown_pinned`, `breakdown_set_data`, `breakdown_set_border_colour` | `breakdown::sidebar_breakdown_*` |
| `build/init.rs` (U19) `default_hooks` | `breakdown` | `breakdown::sidebar_breakdown_ctor` (applies `BreakdownInit`: `pinnedColour`, `envName`, `clearDisplayFunc`, Build.lua:617-621) |

## 2. CCR-0111 §1 applied: the UI tail of `CalcsTab:BuildOutput`

CCR-0111 §1 asked U40 to apply its diff together with the hooks (with the real control, a pinned
Calcs breakdown would otherwise keep the previous calc's sections after a recalc). Applied:
`crate::tabs::calcs::core::build_output_ui_tail(cx)` right after `calc.build_output(..)` in
`build/init.rs` (Build.lua:695) and in `build/on_frame.rs` (Build.lua:1266, inside `if built`,
before L1267).

## 3. `self.calcsTab` of the sidebar control

Evidence: Build.lua:616 `new("CalcBreakdownControl"):CalcBreakdownControl(self.calcsTab)`; U19's
`init.rs` calls `BuildHooks::breakdown(cx, build, init)` before it assigns
`cx.build_mut().tabs` (the `TabIds` store after L624), so the hook cannot read the tab id.
Work-around (U40's file, `DIVERGENCE(calcs-tab-late)`): the sidebar control stores
`calcs_tab: None` and resolves `tabs::calcs::core::tab(cx)` when it needs the UI tab (only for
`self.calcsTab:ClearDisplayStat()`, which the sidebar control never reaches: it has a
`clearDisplayFunc`). Everything else it reads through `calcsTab` is model state. Proposed
(optional, U19): pass the calcs tab id to `BuildHooks::breakdown`
(`Rc<dyn Fn(&mut Cx, CtrlId, CtrlId, BreakdownInit)>`).

## 4. Hooks for U38 and U23 (`breakdown::BreakdownHooks`)

The control calls into classes not ported yet. `breakdown::{BreakdownHooks,
default_breakdown_hooks, breakdown_hooks, set_breakdown_hooks}` (thread-local, the CCR-0098 §3
pattern):

| Entry | Lua | Default |
|---|---|---|
| `add_item_tooltip(cx, tooltip, item, slot)` | `build.itemsTab:AddItemTooltip(tooltip, item, slot)` (L285, L431; slot = `row.source` / `row.mod.sourceSlot`) | adds nothing |
| `node_viewer_draw(cx, bd, zoom, zoomX, zoomY, vp)` | L632-637: `viewer.zoom/zoomX/zoomY = …; viewer:Draw(build, {0, 0, 300, 300}, {})` | draws nothing |
| `node_viewer_highlight_ring(cx, bd) -> Option<ImageHandle>` | `viewer.highlightRing` (L640) | `None` (the L640 DrawImage is then untextured) |

The viewer object is the control's `node_viewer: tree_view::PassiveTreeView` field (U23's type,
lent with `cx.lend` by U23's hook). Proposed: U38 and U23 install their entries (one line each in
`default_breakdown_hooks`); U23 also runs the `PassiveTreeView` constructor's side effects (its
image loads) for this field, which the placeholder `Default` does not.

## 5. `known_diffs.toml`

Entry `U40-1` added (CCR-0015 §1 / ui doc §13 item 14: the `divVar` tag text, L481/L485).

## 6. Tests

`crates/pob-ui-oracle/tests/u39_calcs.rs` (U39's) stubs the Calcs tab breakdown's methods on the
Lua side to match `BreakdownStandIn`. With the hook arms of §1 the Rust default is the real
control, so the u39 harness now overrides U40's three `CalcsHooks` arms with U39's stand-in
(the closures `default_calcs_hooks` had before §1, moved into the test) and keeps its Lua stub:
u39 compares the Calcs tab with the stand-in on both sides, as accepted for U39. The sidebar
control is real on both sides there (with u40's `AddItemTooltip`/`nodeViewer` stand-ins).

Why not the real control in u39: tried; 12 of 13 u39 runs had zero divergences with it, but
`u39/d_skill_mode_undo` hovers (through the open Calculation Mode list, at 500,145) a cell whose
breakdown is `SkillBuffs` (Calcs.lua:762-772). Its rows tie under the L349 comparator (Herald
of Ice/Thunder's Attack and Spell `ColdMin`/`ColdMax`/`LightningMin` mods, equal name and value),
and the order of `env.buffs[name]` is PoB's per-process random order (calc doc D8;
`.parity/nondet/allflame_exp_001.json` records `env.buffs["Herald of Ice"].mods`). Verified: two
fresh LuaJIT PoB processes loading the same build gave different `SkillBuffs.modList` orders, and
the failing pairs changed between runs (Cold Max at 1280x800, Lightning Min/Cold Min at
1920x1080). The UI VM and the bridge's VM are two processes, so the tied rows' order cannot be
matched. U40's own scenarios (`tests/u40/`) cover the real control in the Calcs tab, pinned, in
the overlay pane and in the sidebar, and avoid tied mod lists (see `e_radius.pobscript`).
