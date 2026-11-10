# CCR-0046: `calcSkillDuration` called with an empty env table and no enemyDB

**Status: applied 2026-11-10 by RETYPE (AMENDMENTS A8 E5 approved, A10): `calcSkillDuration(…, env: Option<&EnvRef>, enemyDB: Option<&ModDB>)`, every caller updated, SK-A's `calcSkillDuration_noEnv` deleted. Previously: escalated by CCR2 2026-11-10.**

Requested by: SK-A (Data/Skills/act_int.lua hooks)
Filed status: proposed

## Problem

The frozen signature of the global `calcSkillDuration` (OF-A, `crates/pob-calc/src/modules/calc_offence/helpers_setup.rs`)
is

```rust
pub fn calcSkillDuration(skillModList: &ModList, skillCfg: Option<&ModCfg>, skillData: &Table,
                         env: &EnvRef, enemyDB: &ModDB) -> f64
```

but one Lua caller passes neither an env nor an enemy DB:

- `Data/Skills/act_int.lua:21113` (WintertideBrand `preDamageFunc`):
  `local duration = calcSkillDuration(activeSkill.skillModList, activeSkill.skillCfg, activeSkill.skillData, {}) * debuffDurationMult`

The body (`Modules/CalcOffence.lua:322-335`) reads `env.mode_effective` (nil on the fresh `{}`, so the
`enemyDB` branch at 329-330 never runs and `enemyDB` is never read). The other callers
(CalcOffence.lua:1220, 2632, 2655, 2681, 2727, 2774, 2804; CalcPerform.lua:2385) pass the real env and
`enemyDB`.

A Rust caller has no `EnvRef` for the `{}` literal (building a throwaway `Env` would also need a
`ModDB` for the never-read `enemyDB`).

## Proposed diff

```diff
-pub fn calcSkillDuration(skillModList: &ModList, skillCfg: Option<&ModCfg>, skillData: &Table,
-                         env: &EnvRef, enemyDB: &ModDB) -> f64
+/// `env = None` is a Lua table without `mode_effective` (act_int.lua:21113 passes `{}`);
+/// `enemyDB` is read only when `env.mode_effective` is true.
+pub fn calcSkillDuration(skillModList: &ModList, skillCfg: Option<&ModCfg>, skillData: &Table,
+                         env: Option<&EnvRef>, enemyDB: Option<&ModDB>) -> f64
```

with L328 as `if env.is_some_and(|e| e.mode_effective.get())` and L329 raising Lua's index-of-nil
error when `enemyDB` is `None` there. Callers with an env pass `Some(&env), Some(&enemyDB)`.

## Workaround (until applied)

`crates/pob-calc/src/data_fns/skills_act_int.rs` has a private `calcSkillDuration_noEnv` (marked
`// CCR-0046`) with the body of CalcOffence.lua 322-335 for the `{}` env; WintertideBrand calls it. When
the CCR lands, replace it with `calcSkillDuration(…, None, None)`. Its lines carry no `@pob` marker
(OF-A's marker owns CalcOffence.lua 322-335).
