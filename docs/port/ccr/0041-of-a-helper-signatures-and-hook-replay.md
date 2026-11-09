# CCR-0041: OF-A — helper signatures that cannot say nil, and skill-hook replay for L4

Status: proposed by OF-A (2026-11-09); implemented in OF-A's files and, for §3-§4, in the
ORC-X testkit files named below (the unit brief asks the first L4 pilot to fix the testkit
minimally through a CCR).

## 1. `calcRadiusBreakpoints` returns four nils for a zero radius

U0's stub (`calc_offence/helpers_setup.rs`) declares
`calcRadiusBreakpoints(...) -> (f64, f64, f64, f64)`.

Lua (CalcOffence.lua:198-224): the four locals are declared without a value (200) and only
assigned inside `if radius > 0 then` (201-222), so a radius of 0 returns `nil, nil, nil, nil`.
The callers pass them straight to `breakdown.area` (395, 408, 476; OF-B: 1316, 1400, 1408),
which prints the breakpoint lines only when they are non-nil (CalcBreakdown.lua:106; CT already
made `area`'s breakpoints `Option<f64>`, CCR-0014). The `radiusTertiaryBaseMargin` path
(461-465) feeds them to `m_min`, which raises on nil. A plain `f64` cannot carry that.

```diff
-pub(crate) fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64, radiusMod: Option<f64>)
-    -> (f64, f64, f64, f64)
+pub fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64, radiusMod: Option<f64>)
+    -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>)   // (inc, more, red, less); None = Lua nil
```

OF-B's three calls pass the tuple to `breakdown.area` unchanged.

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

## 4. testkit: `segment_run`, `HookCalls`, `hits_of`

`crates/pob-oracle/src/testkit.rs`:

* `segment_run(build, stage_in, stage_out, hooks: Option<&Rc<HookCalls>>) -> SegmentRun`
  (`mismatches`, `hooks_replayed`, `hook_notes`): `segment_diff` plus the handler of §3. For each
  `runSkillFunc(name)` of a skill that has the hook it applies the recorded `runSkillFunc:in →
  :out` delta of the first unused recorded call with that `name` made in the same enclosing
  function-probe call as `stage_in` (the last entry of the dump's `stack`, e.g. `offence:in#4`).
  Calls that a `hits.json` of the dump directory lists before `stage_in` are skipped, so a segment
  starting mid-offence (OF-E/OF-F: `postCritFunc` once per pass) picks its own; without such an
  index the first unused call of the name is taken (right for `initialFunc`, `preSkillTypeFunc`,
  `preDamageFunc`, `preDotFunc`, which run once per offence call). A skill without the hook
  needs no replay (Lua's call does nothing). `segment_diff` now calls `segment_run`.
* `segment_run_in(dirs, …)` (the same on given dump directories) and `same_run_dirs(dirs, hits)`
  (the first directory holding all `hits`). **Gap found:** `segment_diff` took `stage_in` and
  `stage_out` from whichever probe-cache directory had each first; the cache holds one directory
  per patched-source sha (other units' runs, provenance on/off), i.e. per PoB process, and PoB's
  state differs between processes (calc doc §9.5: mod order inside stores, the AltX/AltY gem
  variant of aux skills). Pairing an `:in` of one run with an `:out` of another gave 30 false
  failures (all57: allflame_exp_038, _044) in OF-A's L4. `segment_run` now takes both dumps from
  one run.
* `HookCalls::new(dirs)`: the recorded `runSkillFunc:in/out` dumps of one build, indexed lazily by
  enclosing hit (shared across a build's L4 runs).
* `hits_of(dirs, probe)`: every dumped hit of a probe id, by `n` (the L4 tests enumerate
  `offence:362#n` and pair them with `offence:1037` by enclosing hit: a disabled skill returns at
  371, so `offence:362#n` and `offence:1037#n` are different calls after the first disabled one).

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
the foreign env's referenced stores (or have `segment_run` inject the main env's dump from the
same run and link `$foreign` markers to it). Every OF-* L4 test on all57 will meet the same hits.

## Not changed (recorded)

* `calcWarcryCastTime` calls `calcs.actionSpeedMod` (PF-B, still a stub returning 0), so it is
  ported but not fuzzed by OF-A.
