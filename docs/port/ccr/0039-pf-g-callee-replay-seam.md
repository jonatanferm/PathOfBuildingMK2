# CCR-0039: PF-G — callee replay seam, `calcs.mirages` return value, ally Life pool shape, exporter absent marks

Status: proposed by PF-G (2026-11-09)

PF-G (`calc_perform/ailments_dispatch.rs`, CalcPerform.lua 3497-3990) is the first calc segment
whose L4 test replays callees (calc doc §9.2 "Replay": "PF-G calling `offence`"). Porting it
and its test hit seven gaps in frozen contracts and shared tests. Each is worked around locally (or reported) and marked
`// CCR-0039` where it touches code.

## §1 There is no interception point for replaying a callee

**Problem.** §9.2 says the L4 tests swap a not-yet-ported callee for its recorded delta, but the
Rust segment calls `calcs::defence` etc. directly (`calcs_facade.rs`, U0), and
`pob_oracle::replay::Replay::apply_report` can only run *between* calls. Nothing lets a test run
the delta *in place of* the call. OF-A (skill hooks), TR-* (`buildActiveSkill`) and PF-C
(`createMinionSkills`) need the same thing.

**Evidence.** calc doc §9.2 Replay paragraph; `crates/pob-calc/src/calcs_facade.rs` (plain
re-exports); `crates/pob-oracle/src/replay.rs` (`apply_report(hit, env)`).

**Workaround (PF-G-owned).** `ailments_dispatch::replay_hook` (feature `probe` only, so the
shipped calc has no hook): a thread-local `Box<dyn FnMut(callee, env, actor) -> Option<Val>>`;
every callee call of the segment goes through `callee(name, env, actor, || …)`, which returns
the hook's `Some(ret)` instead of running the Rust callee. The test
(`crates/pob-oracle/tests/PF-G.rs`) installs a hook that applies the delta between the line
probes around the call (`crates/pob-oracle/corpus/PF-G/stages.toml`).

**Proposed diff.** Move the seam to `pob_calc::probe` (ORC-X) and route the facade through it:

```diff
 // crates/pob-calc/src/probe.rs (feature "probe")
+pub mod replay_hook {
+    pub type Hook = Box<dyn FnMut(&str, &EnvRef, &[Val]) -> Option<Val>>;  // callee id, env, args
+    pub fn set(h: Option<Hook>) -> Option<Hook>;
+    pub fn try_replay(callee: &str, env: &EnvRef, args: &[Val]) -> Option<Val>;
+}
+/// `callee!("defence", env, [actor] => calcs::defence(env, actor))`: the call, or its replay.
+#[macro_export] macro_rules! callee { … }   // expands to the plain call without `probe`
```

and have the units call `callee!(…)` at their not-yet-ported call sites (PF-G then drops its
local copy).

## §2 `calcs.mirages` returns a value the caller tests

**Problem.** The frozen signature is `pub fn mirages(env: &EnvRef)` (`calc_mirages.rs`), but
PoB's `calcs.mirages` returns `nil` (CalcMirages.lua:64) or `calculateMirage(env, config)` =
`not config.calcMainSkillOffence` (CalcMirages.lua:57, 427), and perform skips the main skill's
offence when it is truthy (CalcPerform.lua:3798 `if not calcs.mirages(env) then`).

**Workaround.** PF-G treats the Rust call as returning nil (so `offence` always runs unless a
replay returns the recorded truthy value).

**Proposed diff.**

```diff
 // crates/pob-calc/src/modules/calc_mirages.rs (MI)
-pub fn mirages(env: &EnvRef) {
+/// The Lua result's truthiness (`nil` → false).
+pub fn mirages(env: &EnvRef) -> bool {
```

and in `ailments_dispatch.rs` `if !calcs::mirages(&env)`.

## §3 Shape of `allyLifePools()` (PF-B)

**Problem.** `actor_b::allyLifePools() -> Table` (U0 stub) stands for five module locals
(CalcPerform.lua 1256-1272) but the table's keys and the pools' identity are not specified.
PF-G compares pools by identity (`pool == spectreLifePool`, 3705, 3733, 3742) and reads
`life`, `redirect`, `source`, `limit`.

**What PF-G assumes (please confirm in PF-B):** the returned table has the keys
`minionLifePoolBySkill` (skill name → pool), `spectreLifePool`, `companionshipLifePool`,
`totemLifePoolByStat` (`TotalTotemLife` / `TotalVaalRejuvenationTotemLife` → pool) and
`allyLifeRedirects` (array of names); a pool is `{ life, redirect, source, limit? }`; every call
returns the **same** pool tables (module locals, e.g. a thread-local built once). While the stub
returns `{}`, PF-G reads no redirects (`needsAllyLife` is false); in Lua `unpack(nil)` would
raise, which cannot happen once PF-B is ported.

**Consequence for the L4 test.** On builds where PoB takes the ally-Life path (all57:
allflame_exp_014, Stone Golem/minion pool, 172 mismatches; allflame_exp_017, Scourge Arrow totem
pool, `TotalTotemLife` = 2212) the Rust segment cannot match until PF-B (`allyLifePools`,
`initMinionModDB`, `addMinionModifiers`), PF-A (`doActorAttribsConditions`, `doActorLifeMana`)
and OF-A (`calcTotemLife`) are ported; `PF-G.rs` reports them as *stub-blocked* (detected by
PoB's non-empty `env.player.allyLifeList` at the `defence` call) and excludes them from the gate.
This block (CalcPerform.lua 3667-3789) is therefore **not yet verified** against the oracle.

## §4 Exporter: an `Option` field set to `Some(empty)` stays "absent"

**Problem.** Injection marks a typed field that was nil in the dump as absent; the exporter
then drops it while its value is a default (`reg_absent(ptr, k) && val_is_default(&v)`,
`crates/pob-calc/src/probe.rs:1451`). `env.player.allyLifeList = { }` (CalcPerform.lua:3669;
`Actor.allyLifeList: Field<Option<Table>>`) sets `Some(empty table)` on an actor whose
`allyLifeList` was nil at `perform:3497`, so the export omits it (`Missing env.player.allyLifeList`,
PoB `{}`), and a replay's export → merge → refill turns it back into `None`.

**Workaround (test only).** `PF-G.rs::unmark_present_options` unmarks `allyLifeList` of each
actor when it is `Some`, before each replay and before the final diff.

**Proposed diff (ORC-X).** For `Field<Option<_>>` / `Cell<Option<_>>` fields, `None` already
encodes nil: do not `mark_absent` them on injection (or treat `Some(_)` as present in
`struct_members`).

## §5 Build-model state an L4 dump does not carry

**Problem.** The dump writes the build as `{"$object": "Build"}` and the spec as
`{"$object": "PassiveSpec"}`; injection binds them to the (empty) build model, since BM-A's
`Build:LoadDB` is still `unported!`. PF-G reads `env.spec.treeVersion` (3627; an empty name makes
`tonumber(major)` raise) and `cacheSkillUUID` reads `env.build.skillsTab.socketGroupList` by
identity (Common.lua:913-918) for the GlobalCache key.

**Workaround (test only).** `PF-G.rs::fill_build_model` sets the active spec's `treeVersion`
from the build XML and builds `skillsTab.socketGroupList` with the main skill's injected socket
group at the index PoB's `$uuid` names (other slots empty groups).

**Proposed.** Once BM-A lands, `testkit::load_build` provides the spec; the socket-group
identity still needs the injector to bind the dump's socket-group tables to
`build.skillsTab.socketGroupList` (e.g. the dumper writing `{"$socketGroup": index}` markers,
or `env.build.skillsTab.socketGroupList` in the dump).

## §6 Replays that cannot reproduce `mirages` / `triggers` (ORC-X)

**Problem.** On the corpus the delta replay of some callees does not reproduce their recorded
`out`: `mirages` on mirage builds (upstream_Dual_Savior: Saviour's Reflection, +48 mismatches;
upstream_Generals_Perforate_Zerker: General's Cry, +35) and `triggers` on
upstream_Dual_Wield_Cospris_CoC (+35). The mismatches are identities (`activeSkillList[2]` is
`env.player.mainSkill` in ours, a different skill table in PoB; `requirementsTable[i].sourceGem.
displayEffect` / `supportList[i]` / `effectList[i]` point at other tables) and fields of the
skill those callees swap in: the callee replaces tables by fresh ones at the same path, which the
replay treats as modified in place (`crates/pob-oracle/src/replay.rs` module doc, "Limits").
The test measures the live state against each call's `in` probe before the replay and its `out`
probe after it: on these builds the Rust segment is at 0 mismatches before every call, all of
them appear at the replay.

**Test policy.** `PF-G.rs` classifies such a build as *replay-limited* (reported, excluded from
the ≥ 98 % clean gate, which counts the builds whose replays all landed); any mismatch the Rust
segment itself introduces (between two calls or after the last one) fails the test.

**Proposed.** ORC-X: give the replay table identity across a callee's `in`/`out` (e.g. the dumper
writing a per-process table id next to `$id`), so a callee's fresh table is copied in instead of
merged into the old one.

## §7 `u0_smoke` (U0) expects PF-G's stub site and an error-free run on stubs

**Problem.** `crates/pob-oracle/tests/u0_smoke.rs::build_output_on_stubs` runs `buildOutput` on
the U0 skeleton and asserts (a) no Lua error and (b) that the stub site
`Modules/CalcPerform.lua:3497-3990` is hit. With PF-G ported, (b) cannot hold (the unit's
definition of done removes that `unported!`), and (a) fails: on a stub-built env the spec has no
`treeVersion` (PS-A/BM-A stubs), so `env.spec.treeVersion:match("(%d+)_(%d+)")` gives nil and
`tonumber(nil)` raises, exactly as PoB would on that state (CalcPerform.lua:3627-3631; later the
nil `env.player.mainSkill` would raise in `cacheSkillUUID`, 3989). Observed:
`slice5_allflame_exp_024 MAIN: Lua error bad argument #1 to 'tonumber' (value expected)`.
PF-G does not paper over it (that would be unfaithful).

**Proposed diff (U0).**

```diff
 // crates/pob-oracle/tests/u0_smoke.rs
-            let env = pob_calc::try_buildOutput(&build, mode)
-                .unwrap_or_else(|e| panic!("{stem} {mode:?}: Lua error {e}"));
+            // a ported segment that runs on a stub-built env raises where PoB would on the same
+            // state (PF-G: tonumber(nil) on an empty spec.treeVersion); count it, not fail
+            let env = match pob_calc::try_buildOutput(&build, mode) {
+                Ok(env) => env,
+                Err(e) => { eprintln!("{stem} {mode:?}: Lua error {e}"); lua_errors += 1; continue; }
+            };
 …
-        "Modules/CalcPerform.lua:3497-3990",
-        "Modules/Calcs.lua:384-870",   // after perform: only reached once the perform segments are ported
```

## Notes (no change requested)

* `SkillEffect.level` / `.quality` are `Cell<f64>`; PoB's `if mainSkill.activeEffect.level then`
  (3940, 3962) is therefore always taken. Every skill effect PoB builds sets both.
* Mine aura pre-stacking (3868-3905, §3.6 sites 3881/3897): Lua deep-copies the aura entry, which
  turns its `modList` (a ModList object) into a plain table of copied mods, then mutates them.
  PF-G rebuilds the changed mods and gives the copy a fresh ModList (or rewrites the copied list
  table in place when `modList` is a plain table); the shared original is untouched. The
  party-tab consumer reads the list with `ipairs` either way.
