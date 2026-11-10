# CCR-0049: OF-H — ailment loop body B: closure calls, shadowed loop locals, `isValueInArray`

**Status: needs code (CCR2, 2026-11-10; ORC-X2: §1-§4). See AMENDMENTS A8.**

Filed status: proposed by OF-H (2026-11-09). Nothing frozen is edited; the workarounds live in OF-H's
own files (`crates/pob-calc/src/modules/calc_offence/ailments_b.rs`,
`crates/pob-oracle/tests/OF-H.rs`).

## 1. OF-G's loop-local functions (depends on CCR-0035)

OF-H calls `checkWeapon1HFlags` (CalcOffence.lua:5004), `calcAilmentDamage` (5184-5186, 5405,
5430) and `calcAverageSourceDamage` (5405, 5430). The U0 stubs in `ailments_a.rs` cannot express
these calls (CCR-0035, filed by OF-G: `calcAilmentDamage(type, sourceCritChance, sourceHitDmg,
sourceCritDmg, hideFromBreakdown)` returns a number; `calcAverageSourceDamage(ailment)` returns
two numbers that fill the last two arguments of `calcAilmentDamage`).

Workaround: `ailments_b.rs` calls them through `use self::ccr0035 as closures;`, a private module
holding copies of the three functions with the CCR-0035 signatures (ported from 4308-4410, no
`@pob` marker so provenance coverage of OF-G's lines is unchanged).

**Integration diff** (when OF-G with CCR-0035 merges):

```diff
-use self::ccr0035 as closures; // CCR-0035
+use super::ailments_a as closures;
```

and delete `mod ccr0035 { … }` at the end of `ailments_b.rs`. The call sites need no change.

## 2. In-loop stages inject the shadowing `output`/`breakdown` into `OffenceCtx`

`offence:4990` (stages.toml, line 4991) is inside the ailment loop, so its dumped `output` /
`breakdown` locals are the loop locals of 4168/4171 (`pass.output`, `pass.breakdown`).
`probe::run_segment` fills both `OffenceCtx` and `AilmentPassLocals` from the same local list, so
`OffenceCtx.output`/`.breakdown` receive the pass tables instead of `actor.output` /
`actor.breakdown` (CalcOffence.lua 354/356). OF-H's segment never reads the function-level
`output`/`breakdown` (they are shadowed for all of 4990-5648), so its L4 test is unaffected; the
same holds for `offence:2607`/`3346`/`4165`. Proposed (ORC-X, `crates/pob-calc/src/probe.rs`
`run_segment`): for the pass-body stages, fill `OffenceCtx.output`/`.breakdown` from the injected
`actor` instead of the locals, so a ctx exported after a pass-body run is the function's state.

## 3. `isValueInArray` (Common.lua:569-575) has no Rust port

OF-H's non-damaging ailment breakdown (5449, 5452, 5455) calls it. No crate fn exists
(`grep -rn isValueInArray crates/` is empty). OF-H uses a private `isValueInArray(&[f64], f64)
-> bool` (the Lua result is used only as a truth value). Proposed: the owner of
`modules/common.rs` (CT) adds the general helper; OF-H then switches to it.

## 4. `$foreign` references in nested-env dumps (ORC-X, informational)

`allflame_exp_013` `offence:4990#4` (stack `perform:in#4 > mirages:in#4 > perform:in#5 >
offence:in#4`, Sacred Wisps mirage) has `activeSkill.baseSkillModList.parent =
{"$foreign": {"env": "env7", "path": "modDB"}}`: the mirage skill's mod list chains to the main
env's modDB, which the dump of env8 does not carry, so the injected `skillModList` misses every
parent mod (e.g. `EnemyChillEffect`, `EnemyAilmentDuration`) and the segment (and OF-A's
`calcAilmentSourceDamage`) computes different values. OF-H's test reports such hits as
`foreign-ref` instead of failing them. Proposed: `inject` resolves `$foreign` markers against a
dump of the referenced env (or the probe dumps it alongside).
