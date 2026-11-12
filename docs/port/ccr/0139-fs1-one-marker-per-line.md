# CCR-0139: FS1 one provenance marker per Lua line (calc/UI shared ranges), CS3 L5 and PF-C harness fixes

Unit: **FS1** (full-suite hygiene; the failures of `.parity/fullsuite/20261111-2122`).
Status: **applied on `port/FS1`** (comment-only edits in other units' files, listed below). Number
reserved with `scripts/port/next_ccr.sh`.

No frozen contract (types, signatures) changes. This CCR records a convention the docs leave open,
and the other units' files FS1 touched to follow it.

## 1. A Lua line carries exactly one marker; the owner of a shared range is the unit table's

Evidence: `provenance.py coverage` (calc doc §9.6: "MUST report 0 … lines … that two markers
cover") failed in the full suite with 421 lines covered twice (`U0.rs:370`). Every case was a
calc file and a UI file both marking the same lines, in one of two shapes:

* **(a) a calc `@pob-skip` naming the UI owner, and the UI owner's `@pob` marker.** The skip was
  the calc unit's placeholder from before the UI unit landed:
  * `Classes/PassiveTree.lua` 234-435, 626-627, 635-645, 748-753, 771-777, 876-885
    (`pob-calc-core/src/classes/passive_tree.rs`, PT) vs U23 (`pob-app/src/tree_view/assets.rs`,
    `connectors.rs`); ui doc §10.2 table row "U23 ports only the image/sprite/connector parts of
    PassiveTree.lua", unit table row U23;
  * `Classes/ItemsTab.lua` 1419-1570 `Draw` (`pob-calc-core/src/model/items_tab.rs`, BM-A) vs U34
    (`pob-app/src/tabs/items/core.rs`); ui doc §10.2 row "U34 ports `Draw` (1419-1572)";
  * `Modules/ItemSlotHelper.lua` 1, 10-38, 40 (`pob-calc-core/src/classes/item.rs`, IT-B: two
    `@pob` markers for `local M = {}` / `return M` and a skip of `M.DrawViewer`) vs U32
    (`pob-app/src/tabs/items/slot_helper.rs`, which ports the whole module). Both unit tables list
    the file (calc doc row IT-B, ui doc row U32); the module has no model content (CCR-0032 §1).

  Resolution: the UI owner's `@pob` markers stay (they carry hashes, so `bump` tracks them); the
  calc skips become plain comments naming the UI file. A `@pob-skip` names lines **no** unit ports.
* **(b) a split function: the calc's `@pob` marker over the whole function, and a UI marker over
  the UI half.**
  * `Classes/PassiveSpec.lua` 2021-2025 (`linkNodes`' `BuildConnector` half): PS-C's
    `BuildSubgraph` marker 1970-2359 (`passive_spec_cluster.rs`) vs U23 `build_subgraph_connectors`
    (`connectors.rs`, CCR-0072 §3);
  * `Classes/ItemsTab.lua` 1620-1649 `EquipItemInSet`: BM-A (`items_tab.rs`) vs U34's whole re-port
    (`core.rs`: the SHIFT redirect sits between the model statements, so it cannot call BM-A's).

  Resolution: the calc marker stays (calc doc row PS-C / BM-A own the file ranges, ui doc §10.2
  "Each split function is commented `// lua: … (model part: calc:… <fn>)`"); the UI copy keeps a
  `lua:` comment that names the owner's marker. An upstream change to the lines is the calc
  owner's `bump` work item; the comment tells the reader to re-port the UI half with it.
* **(c)** `Modules/ItemTools.lua` 404-412 (`itemLib.wiki.open`): IT-C's `@pob-skip` of 384-413 and a
  U23 marker in `tree_view/input.rs`. No UI unit owns ItemTools.lua (CCR-0107 §4); the other two UI
  copies (`controls/gem_select.rs`, `tabs/items/slot.rs`) are unmarked. The U23 marker became a
  `lua:` comment like them (its line comment `L401` was also wrong: `L405`).

Also fixed while there: U23's five constructor markers in `assets.rs` were off by 2-5 lines
against the pinned PassiveTree.lua (234-261 / 263-267 / 268-327 / 329-382 / 384-435; the blocks are
234-258, 260-264, 266-329, 331-387, 389-435), as were the `// Lnnn` comments inside them and the
module table; the calc skip had hidden lines 262, 328, 383, which only the corrected ranges cover.

Result: `provenance.py coverage`: 41266 in-scope lines, 0 uncovered, 0 covered twice; `check`: 0
errors.

Proposed doc diff (calc doc §9.6, after the `coverage` bullet):

```diff
 - `provenance.py coverage` reports in-scope lines … It MUST report 0 from U0 onward.
+  A line has one marker. Where a calc unit and a UI unit both port lines (a split function, or a
+  block the unit tables give to both), the unit table that names the lines' owner keeps the
+  `@pob` marker; the other side names it in a `lua:` / `L<n>` comment. A `@pob-skip` is for lines
+  no unit ports (CCR-0139).
```

## 2. CS3 L5 (`cs3_l5_init_env`) ran against a model without a tree

Evidence: 171/171 runs raised "attempt to index a nil value" at CalcSetup.lua:593
(`init_env_base.rs:288`, `classStats` nil): the test still loaded the model with
`testkit::load_build` (`Build::new` + `LoadDB`), the pre-BM-A stand-in, whose spec has no tree.
With BM-A's loader, the GloriousVanity LUT failed to load (the default `StdJewelFs::new("")` probes
`./src` of the crate directory), and CALCS / CALCULATOR compared against a model without PoB's
preceding MAIN pass (the socket groups' `displayLabel`/`displaySkillList`, the gem instances'
`displayEffect`/`supportEffect` that MAIN's initEnv writes).

Applied (test only): `loadBuildModelFromXML`; the LUTs from the PoB checkout (as CS2/BM-A/CA);
`calcs_tab_prefix` runs the part of CalcsTab.lua 441-472 that precedes the target env (PoB's targets
are the first post-load BuildOutput's envs: `mainEnv` is its MAIN env, e.g. env7 of 27 in
allflame_exp_000; CALCS follows its MAIN `buildOutput`, CALCULATOR its CALCS one); targets are
top-level initEnv calls only (an initEnv under `perform:in, mirages:in` is CalcMirages', e.g.
allflame_exp_013's env8; initEnv's own restart, nested `initEnv:in` only, stays a target);
`classify_l5` for the L5-only export/scope classes (documented on the function). No calc change.

L4 (`cs3_l4_skills_segment`) also failed 8 hits with freshly probed dumps (the full suite reused
dumps from 2026-11-10): `warcryPowerBonus` was stripped from PoB's tree only, but the injected
copy of PoB's mod keeps it (`env.override.extraJewelFuncs.actor.modDB`). It is now stripped from
both trees.

## 3. PF-C needed a manual probe run

Evidence: `PF-C.rs:67` "required input missing: no PF-C dumps under target/tmp/pfc-probe (fill
with: crates/pob-oracle/corpus/PF-C/probe.sh)" in the full suite (`POB_ORACLE_REQUIRE=1`, fresh
target dir); every other run skipped silently.

Applied (test only): PF-C probes each build itself through the shared probe cache
(`testkit::probe_selection`, as PF-G does), with probe.sh's selection, and reads only the main env's
dumps (from the run's `hits.json`). Missing inputs follow `pob_testpaths::require`. `PFC_PROBE_DIR`
still reads a probe.sh output directory.
