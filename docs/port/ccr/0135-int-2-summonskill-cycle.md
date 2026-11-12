# CCR-0135: INT-2: `ActiveSkill.summonSkill` closes an `Rc` cycle (every minion skill leaks its env's skill graph)

Unit: **INT-2** (performance; found while profiling, not fixed here: the files belong to the
calc core contract and to CS1/AS). Status: **applied** 2026-11-12 by LEAK1 (AMENDMENTS A12; the §3 open point resolved with a `summonSkillKeep` keep-alive). Number reserved with
`scripts/port/next_ccr.sh`. Measurements: `docs/port/parity/INT-2-perf.md` §5.

## 1. Problem

The frozen contract types the minion skill's back-pointer to its summoning skill as a strong
`Rc`. Together with the forward edge `ActiveSkill.minion` it is an `Rc` cycle, which calc doc
§3.3 rule 4 forbids:

- calc doc §3.3 rule 4 (`calc-architecture.md:212`): "`Rc` cycles are forbidden. Back-pointers
  (…) are `Weak`. `Env` owns its actors. `GlobalCache` owns cached envs, and wiping it frees
  them."
- §5.4 (`calc-architecture.md:971`), `crates/pob-calc-core/src/active_skill.rs:80`:
  `pub summonSkill: Field<Option<ActiveSkillRef>>` (strong).
- The cycle. `Modules/CalcActiveSkill.lua:914`
  (`calcs.createActiveSkill(activeEffect, activeSkill.supportList, minion, nil, activeSkill)`,
  `calc_active_skill.rs:2444`) creates each minion skill with `summonSkill = activeSkill`
  (`createActiveSkill`, CalcActiveSkill.lua:89, `calc_active_skill.rs:383`) and puts it in
  `minion.activeSkillList` / `minion.mainSkill`. The summoning skill owns the minion
  (`activeSkill.minion = minion`, `calc_active_skill.rs:1916`). So:
  summoning skill → `minion` (strong) → `activeSkillList` / `mainSkill` (strong) → minion skill
  → `summonSkill` (strong) → summoning skill. Lua's GC collects this cycle; `Rc` does not.

Effect: whenever an env with a minion-summoning skill is dropped (a misc-calculator env of the
power report, a `GlobalCache` entry that `cacheData` overwrites, a `buildActiveSkill` "CACHE" env of
the cost-warning pass, a replaced `mainEnv`/`calcsEnv`), the summoning skills, their minion
actors (with mod DBs, outputs) and the minion skills stay allocated for the rest of the process.

## 2. Evidence (measured, release bench profile, `pob-calc/benches/INT-2.rs`)

- A diagnosis bench (initEnv + perform in `Mode::CALCULATOR`, then drop the env and clear the
  `GlobalCache`, then try to upgrade `Weak`s taken before the drop): the env, the player and the
  enemy are freed, but every skill with a `minion` survives (strong count 2-5 = the number of its
  minion's skills) together with its minion actor. allflame_exp_039: 5 of 17 skills survive
  (Summon Sentinel of Radiance, Summon Spectral Tiger, Summon Elemental Relic, Dominating Blow,
  Animate Guardian); allflame_exp_007: 7 of 17 (the golems, Herald of Agony); allflame_exp_001: 2
  of 15 (Herald of Agony, Herald of Purity); builds without minion skills: 0. Setting
  `summonSkill` to `None` on the minions' skills before the drop frees all of them (0 survive).
- One power report (Offence/Defence, full tree) grows RSS by more than 300 MB on 32 of the 57
  corpus builds, by up to 2.25 GB (allflame_exp_039: 348 MB → 2,599 MB). The memory is never
  returned: a second report on allflame_exp_013 or allflame_exp_039 exceeds a 3 GB cap (a 5 GB
  `cargo.sh` scope was OOM-killed running 13 power reports in one process). Builds without a
  minion skill retain 70-150 MB (the first report's caches).
- The full recalc leaks too: 100 × (`wipeGlobalCache` + `CalcsTab:BuildOutput`) on
  allflame_exp_039 ends at 1,598 MB RSS (about 12.5 MB per recalc); with the fix, 349 MB. In the
  app every build edit is a recalc.

## 3. Proposed change

`summonSkill` becomes a `Weak`, like the other back-pointers (diff below; it compiles in the
bench profile and was used for the §4 numbers):

```diff
--- crates/pob-calc-core/src/active_skill.rs
-    pub summonSkill: Field<Option<ActiveSkillRef>>,
+    pub summonSkill: Field<Option<ActiveSkillWeak>>,
--- crates/pob-calc-setup/src/modules/calc_active_skill.rs (createActiveSkill, L89)
-    activeSkill.summonSkill.set(summonSkill.clone());
+    activeSkill.summonSkill.set(summonSkill.as_ref().map(Rc::downgrade));
--- (copyActiveSkill, L181)
-        skill.summonSkill.get(),
+        skill.summonSkill.get().and_then(|w| w.upgrade()),
--- (buildActiveSkillModList, L236ff)
-    let summon = activeSkill.summonSkill.get();
+    let summon = activeSkill.summonSkill.get().and_then(|w| w.upgrade());
--- crates/pob-calc-core/src/modules/calc_tools.rs:313 (canGrantedEffectSupportActiveSkill)
-    let summonSkill = activeSkill.summonSkill.get();
+    let summonSkill = activeSkill.summonSkill.get().and_then(|w| w.upgrade());
--- crates/pob-calc-perform/src/modules/calc_perform/buffs.rs:1224 (CalcPerform.lua:2710)
-                        let summonSkill = mainSkill.summonSkill.get().unwrap_or_else(|| {
+                        let summonSkill = mainSkill.summonSkill.get().and_then(|w| w.upgrade()).unwrap_or_else(|| {
--- crates/pob-calc-offence/src/data_fns/skills_misc.rs:314, :330 (minion.lua preDamageFuncs)
         .summonSkill
         .get()
+        .and_then(|w| w.upgrade())
         .unwrap_or_else(|| lua_error!("attempt to index field 'summonSkill' (a nil value)"));
```

Also to update: the `probe` reflection of `ActiveSkill` (`pob-calc-core/src/probe.rs:1119`,
`"summonSkill" => summonSkill`; needs the `Weak<ActiveSkill>` case that `actor` already has) and
`pob-ui-oracle/src/lua_calc.rs:1099` (`s.summonSkill.get()` → upgrade). The calc doc §5.4 line
971 changes to `Field<Option<ActiveSkillWeak>>` and §3.3 rule 4 lists `ActiveSkill.summonSkill`
among the back-pointers.

Open point for the implementing unit (AS, as CCR-0113): `copyActiveSkill` copies a skill that
may belong to another env (a `GlobalCache` entry's skill). If that source is a minion skill, its
summoning skill lives in the foreign env, and a `Weak` dies when `cacheData` overwrites the entry
that owned it. CCR-0113 solved the same problem for `actor` with a keep-alive in the copy's
`extra`; the same keep-alive for a foreign `summonSkill` closes no cycle (the foreign summoning
skill does not reach the copy). The bit-identical results of §4 are evidence that no copy lost
its `summonSkill` there, not a proof.

## 4. Validation done and still needed

Done here (bench profile, the diff above applied temporarily, then reverted; numbers in
`docs/port/parity/INT-2-perf.md` §5). Re-measured in INT-2's fix round 1, pinned to the
performance cores, at load average 1.5-3.3, the two sides alternating per round, with LuaJIT
measured in the same run. On the 10 INT-2 gate builds, 3 rounds each, the output digest (every
main- and CALCS-output leaf, exact f64 bits) and the power digest (every node power leaf and
`powerMax`) are identical to the unpatched calc. RSS after one power report drops to 435-691 MB
(from 1,140-2,770 MB on the nine minion builds; the one build without a minion skill,
allflame_exp_000, stays at 490 MB). Time: freeing the graphs costs what the leak saved. The
power report takes +4 %…+14 % more CPU (allflame_exp_014 4.78 → 5.47 s, LuaJIT 6.90 s), the
recalc +3 %…+6 %, and allflame_exp_000 is unchanged. The patched calc is still at or below
LuaJIT on every gate build: power 0.55-0.89 of LuaJIT, recalc 0.72-0.86. The minion builds keep
a clear margin (power ≤ 0.79). allflame_exp_000 (0.89 by the minima, one round pair at 1.01) is
level with LuaJIT within noise, with or without this CCR; the CCR does not change it. So the
INT-2 gate holds after this CCR, with the same caveat as without it: the light builds have no
margin to spare (INT-2-perf.md §1, §3).

The first version of this section gave "power 0.54-0.85, recalc 0.69-0.88" and "+0 %…+19 %".
Those ratios used LuaJIT samples taken at load average 7-10, which slows LuaJIT more than Rust
(INT-2-perf.md §2 "Noise"). The numbers above replace them.

The build with the `probe` feature (the oracle's) also needs `ProbeField for Weak<ActiveSkill>`
(`probe.rs`, next to the `Weak<Actor>` impl; `kind()` = `Kind::Skill`, `to_val` upgrades or
gives nil). Without it `pob-oracle` does not compile. That digests match says that no
`summonSkill` lookup changed a result on these builds; it does not prove the copy case of §3
cannot occur.

Needed before merge (the implementing unit): CA `ca_l6_corpus` / `ca_l5_post` /
`ca_d13_fixed_point`, PWR (108,820 exact), AS with `POB_FULL_SUITE=1`, and the INT-2 bench
(`scripts/port/perf_bench.sh`) re-run, since the timings shift.
