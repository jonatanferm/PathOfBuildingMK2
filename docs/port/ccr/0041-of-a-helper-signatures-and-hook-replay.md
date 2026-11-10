# CCR-0041: OF-A — helper signatures that cannot say nil, and skill-hook replay for L4

**Status: applied 2026-11-10 by CCR2 (§2, §3: calc doc §7.2, §9.2); §1, §4, §5 need code. See AMENDMENTS A8.**

Filed status: proposed by OF-A (2026-11-09). §1 is worked around locally (frozen signature kept),
§2 is applied in OF-A's own file (no other caller), §3 adds a small probe hook to
`crates/pob-calc/src/probe.rs` (cannot live in a test), §4 lives locally in
`crates/pob-oracle/tests/OF-A.rs` (`mod l4kit`) and is proposed for `pob_oracle::testkit`.

## 1. `calcRadiusBreakpoints` returns four nils for a zero radius

U0's stub (`calc_offence/helpers_setup.rs`) declares
`calcRadiusBreakpoints(...) -> (f64, f64, f64, f64)`.

Lua (CalcOffence.lua:198-224): the four locals are declared without a value (200) and only
assigned inside `if radius > 0 then` (201-222), so a radius of 0 returns `nil, nil, nil, nil`.
The callers pass them straight to `breakdown.area` (395, 408, 476; OF-B: 1316, 1400, 1408),
which prints the breakpoint lines only when they are non-nil (CalcBreakdown.lua:106; CT already
made `area`'s breakpoints `Option<f64>`, CCR-0091). The `radiusTertiaryBaseMargin` path
(461-465) feeds them to `m_min`, which raises on nil. A plain `f64` cannot carry that.

```diff
-pub(crate) fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64, radiusMod: Option<f64>)
-    -> (f64, f64, f64, f64)
+pub fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64, radiusMod: Option<f64>)
+    -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>)   // (inc, more, red, less); None = Lua nil
```

OF-B's three calls pass the tuple to `breakdown.area` unchanged. Same change as CCR-0043 (OF-B).

**Workaround until applied (marked `// CCR-0041`):** the frozen
`calcRadiusBreakpoints -> (f64, f64, f64, f64)` stays (OF-B's `radius_breakpoints` wrapper,
CCR-0043, calls it for every radius, so it must not raise on 0) and returns NaN for each nil;
the body is `calcRadiusBreakpointsOpt -> (Option<f64>, …)`, which OF-A's own callers
(`calcAreaOfEffect`, CalcOffence.lua 394, 406, 461, 475) use. When this CCR / CCR-0043 is
applied, `calcRadiusBreakpointsOpt` takes the name and OF-B drops its wrapper.

## 2. `setMoltenStrikeTertiaryRadiusBreakdown`'s `label` can be nil

Stub: `label: &str`. Lua inserts it with `t_insert(breakdownRadius, label)` (258); the only
caller passes `skillData.radiusTertiaryLabel` (421), which Molten Strike's data sets only for
some skill parts (`skill("radiusTertiaryLabel", …, { type = "SkillPart", skillPartList = { 2, 3 } })`,
Data/Skills/act_str.lua:7901, 8109, 8225) while `projectileSpeedAppliesToMSAreaOfEffect` is set
for all of them (7895, 8103, 8219). `t_insert(t, nil)` stores nil at `#t + 1` (no error).

```diff
-    label: &str,
+    label: Option<&str>,
```

Also: the helpers become `pub` (were `pub(crate)`) so `crates/pob-oracle/tests/OF-A.rs` can fuzz
them against PoB's local functions (§11.1 row OF-A acceptance). No other change of meaning.

## 3. Skill hooks during an L4 run: `pob_calc::probe` handler (testkit gap)

The L4 test of OF-A must run `seg_setup` with hooks replayed (§11.1: "L4 `362`→`1037` (hooks
replayed)"). ORC-X's testkit could replay a callee given its hit (`replay::Replay`), but had no
way to reach a call made *inside* a Rust segment: `runSkillFunc` dispatches to
`data_fns::skillFunc`, whose unported hooks are no-op stubs, and pob-calc cannot depend on
pob-oracle. Missing pieces, now added:

* `crates/pob-calc/src/probe.rs` (feature `probe`): `SkillHookHandler`
  (`Box<dyn FnMut(&ActiveSkillRef, SkillHook) -> bool>`), `set_skill_hook_handler(Option<…>) ->
  Option<…>` and `skill_hook_handled(skill, hook) -> bool` (the handler is taken out while it runs,
  so it may call the calc).
* `helpers_setup::runSkillFunc` offers every call to the handler first
  (`#[cfg(feature = "probe")]`), and skips its body when the handler returns true. Non-probe
  builds are the Lua function unchanged.

## 4. L4 kit for segments that call skill hooks (proposed for `pob_oracle::testkit`)

Implemented locally in `crates/pob-oracle/tests/OF-A.rs`, `mod l4kit` (the unit brief: testkit
gaps get local helpers plus a CCR); proposed to move into `crates/pob-oracle/src/testkit.rs`
unchanged, so that OF-B..OF-I can reuse it:

* `segment_run_in(dirs, build, stage_in, stage_out, hooks: Option<&Rc<HookCalls>>) -> SegmentRun`
  (`mismatches`, `hooks_replayed`, `hook_notes`): `testkit::segment_diff` plus the handler of §3.
  For each `runSkillFunc(name)` of a skill that has the hook it applies the recorded
  `runSkillFunc:in → :out` delta of the first unused recorded call with that `name` made in the
  same enclosing function-probe call as `stage_in` (the last entry of the dump's `stack`, e.g.
  `offence:in#4`). Calls that a `hits.json` of the dump directory lists before `stage_in` are
  skipped, so a segment starting mid-offence (OF-E/OF-F: `postCritFunc` once per pass) picks its
  own; without such an index the first unused call of the name is taken (right for
  `initialFunc`, `preSkillTypeFunc`, `preDamageFunc`, `preDotFunc`, which run once per offence
  call). A skill without the hook needs no replay (Lua's call does nothing).
* `HookCalls::new(dirs)`: the recorded `runSkillFunc:in/out` dumps of one build, indexed lazily by
  enclosing hit (shared across a build's L4 runs).
* `hits_of(dirs, probe)`: every dumped hit of a probe id, by `n` (the L4 test enumerates
  `offence:362#n` and pairs them with `offence:1037` by enclosing hit: a disabled skill returns at
  371, so `offence:362#n` and `offence:1037#n` are different calls after the first disabled one).

**Gap found in `testkit::segment_diff` (not changed here):** it takes `stage_in` and `stage_out`
from whichever probe-cache directory has each first; the cache holds one directory per
patched-source sha (other units' runs, provenance on/off), i.e. per PoB process, and PoB's state
differs between processes (calc doc §9.5: mod order inside stores, the AltX/AltY gem variant of
aux skills). Pairing an `:in` of one run with an `:out` of another gave 30 false failures (all57:
allflame_exp_038, _044) in OF-A's L4. Proposed: `segment_diff` picks the first directory that
holds both hits (`same_run_dirs`), as the OF-A test does (it uses the one run directory holding
the most `offence:362` hits next to `offence:1037` ones).

Dumps needed per build: `scripts/port/oracle.sh exec probe <xml> --out-dir D --cache
--max-dumps 3000 --select '<seg in>,<seg out>,runSkillFunc:in,runSkillFunc:out'`
(`oracle.sh run` has no `--max-dumps`, and the default 200 is too few for all hits of the 30
offence calls of an all57 build with 5 `runSkillFunc` calls each).

## 5. Open (not fixed here): `$foreign` mod-store parents

Offence calls in a mirage env (stack `perform:in → mirages:in → perform:in → offence:in`) run a
skill copied from the main env (`copyActiveSkill`), whose `baseSkillModList.parent` is the main
env's `modDB`, dumped as `{"$foreign": {"env": "env7", "path": "modDB"}}`. `inject` resolves one
env, so the parent stays unlinked and queries through the skill's mod list miss the main env's
mods (OF-A L4 all57: 17 of 1118 hits, in allflame_exp_013, _036, _051, upstream_Dual_Savior,
upstream_Mirage_Archer_Toxic_Rain; known_diffs.toml OF-A-1). Fix for ORC-X: let the probe dump
the foreign env's referenced stores (or have `segment_run_in` inject the main env's dump from the
same run and link `$foreign` markers to it). Every OF-* L4 test on all57 will meet the same hits.

## Not changed (recorded)

* `calcWarcryCastTime` calls `calcs.actionSpeedMod` (PF-B, still `unported!` at the pin), so it is
  ported but not fuzzed by OF-A.
