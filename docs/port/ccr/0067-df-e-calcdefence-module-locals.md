# CCR-0067: DF-E — CalcDefence module locals have no Rust items; probe-cache dump mixing

**Status: applied 2026-11-10 by ORC-X2 (§1-§2: `est_pools.rs` imports DF-A's `isElemental`/`dmgTypeList`/`allyLifePoolList`/`calcLifeHitPoolWithLossPrevention`, the copies are deleted; §3: `testkit::find_pair`, used by `segment_diff` and `tests/DF-E.rs`). See AMENDMENTS A9.**

Filed status: proposed by DF-E (2026-11-09)

DF-E (`calc_defence/est_pools.rs`, CalcDefence.lua 2153-2611) reads three module-level locals of
CalcDefence.lua and calls one module-local function, all owned by DF-A (`calc_defence/helpers.rs`,
CalcDefence.lua 1-650). Two gaps; the code workaround is marked `// CCR-0067`.

## §1 `isElemental`, `dmgTypeList`, `allyLifePoolList` are not exposed

**Problem.** U0's `helpers.rs` stub covers CalcDefence.lua 7-43 with one comment marker
(`// @pob Modules/CalcDefence.lua:7-43`) and defines no Rust item for the tables. Every
CalcDefence segment that iterates them (DF-B..G; DF-E reads all three) needs the same data, and
defining them per segment would duplicate DF-A's lines (and break `provenance.py coverage`,
which allows each Lua line once).

**Evidence.** CalcDefence.lua:24 (`isElemental`), 28 (`dmgTypeList`), 33-43 (`allyLifePoolList`);
reads in the DF-E range: 2325, 2390, 2440, 2471, 2480, 2503, 2522, 2545, 2580.
`crates/pob-calc/src/modules/calc_defence/helpers.rs` lines 17-18 (comment marker only).

**Proposed diff (DF-A's file).**

```diff
 // crates/pob-calc/src/modules/calc_defence/helpers.rs
-// @pob Modules/CalcDefence.lua:7-43 h=fbb8bcde
-// (module-level statements: …)
+// @pob Modules/CalcDefence.lua:7-43 h=fbb8bcde
+pub(crate) fn isElemental(damageType: Name) -> bool { … }            // L24
+pub(crate) const dmgTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"]; // L28
+pub(crate) struct AllyLifePool { pub key: &'static str, pub life: &'static str,
+    pub mitigation: &'static str, pub redirect: Option<&'static str>,
+    pub fallback: Option<&'static str>, pub meleeOnly: bool, pub label: &'static str }
+pub(crate) const allyLifePoolList: [AllyLifePool; 9] = [ … ];          // L33-43, source order
```

(`hitSourceList`, `resistTypeList`, `tempTable1` likewise for the segments that read them.
DF-D filed the same gap concurrently as CCR-0068; one applied diff should serve both.)

**Workaround (DF-E-owned).** `est_pools.rs` keeps private copies of the three tables, with
the same names and the row shape above, and no `@pob` marker. When DF-A lands with the items
above, delete the copies and `use super::helpers::{isElemental, dmgTypeList, allyLifePoolList}`;
the call sites do not change.

## §2 `calcLifeHitPoolWithLossPrevention` is still `unported!`

**Problem.** DF-E calls the DF-A helper at CalcDefence.lua:2289. Its frozen signature exists
(`helpers::calcLifeHitPoolWithLossPrevention(life, maxLife, lifeLossPrevented,
lifeLossBelowHalfPrevented) -> f64`), but the body is a stub that returns `0.0`, so the L4 test
of this segment cannot pass until DF-A merges.

**Workaround.** A private copy of the 3-statement body (CalcDefence.lua:180-182, same operation
order) in `est_pools.rs`, marked `// CCR-0067`. Once DF-A merges, delete it and import
`super::helpers::calcLifeHitPoolWithLossPrevention` (same signature). No contract change needed.

## §3 The shared probe cache can pair dumps from different PoB runs (testkit, ORC-X)

**Problem.** `testkit::segment_diff` resolves `<seg>:in#n` and `<seg>:out#n` independently
(`find_dump` over `dump_dirs`). The probe cache key is `<patched-source sha>/<build blake3>`,
shared by every unit, and `oracle.sh run --stage A,B` rewrites only the selected hits. When
another unit re-probes the same build with a selection that includes only one of the two stages
(DF-D selects `buildDefenceEstimations:1681,…:2153`; DF-E `…:2153,…:2612`), the directory ends
up with `:in` from one process and `:out` from another. PoB's string-keyed `pairs` order differs
per process (calc doc §3.4), so the env's mod lists differ in order and content position
(`env.auxSkillList[i].buffList`, `env.modDB.mods.* LUA-ORDER`) although the segment touches
none of them. Observed: 192 of 363 all57 hits failed only on such env paths until DF-E paired
the dumps itself.

**Evidence.** `.parity/probe-cache/6d4921b78d93480f/8a773457c3e8b986/`:
`buildDefenceEstimations:2153#*.json.zst` written 16:28, `…:2612#*` 16:23 (2026-11-09).

**Proposed diff (ORC-X's testkit).** Make `segment_diff` take both dumps from the first
directory that has both (and/or have `oracle.sh run` write a run id into each dump so a mixed
pair is detected and reported instead of diffed).

**Workaround (DF-E-owned).** `crates/pob-oracle/tests/DF-E.rs` pairs the dumps itself (first
directory holding both) and looks first in a private `target/dfe-probe/<stem>/state/` filled by
`oracle.sh exec probe … --out-dir target/dfe-probe/<stem>` (one run per build).
