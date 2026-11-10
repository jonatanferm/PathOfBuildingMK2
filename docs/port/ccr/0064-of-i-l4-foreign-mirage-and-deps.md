# CCR-0064: OF-I L4 — foreign mirage output in dumps, and OF-I's real callee dependencies

**Status: applied 2026-11-10 by CCR2 (§2 calc doc §11.1 OF-I row); §1 needs code. See AMENDMENTS A8.**

Filed status: proposed by OF-I (2026-11-09). Informational for ORC-X and the unit table; no frozen
signature changes. OF-I's own files need no workaround code beyond the test classification below.

## 1. `activeSkill.mirage.output` is dumped as `{"$foreign": …}` (ORC-X, pob-state/1)

CalcOffence.lua:6213-6247 reads `activeSkill.mirage.output.TotalDPS`, `.IgniteDPS`, `.BleedDPS`,
`.PoisonDPS`, `.ImpaleDPS`, `.DecayDPS`, `.TotalDot`, `.BurningGroundDPS`, `.CausticGroundDPS` and
`.CullMultiplier`. `calcs.mirages` (CalcMirages.lua) builds the mirage in its own env and stores that
env's `player.output` in `activeSkill.mirage.output`. The state dump writes a table of another env as
`{"$foreign": {"env": "env8", "path": "player.output"}}` without contents, so an injected
`offence:5650` state has no mirage output and the L4 pair cannot be compared.

Seen on all57 `allflame_exp_013`, `allflame_exp_036`, `allflame_exp_051`,
`upstream_Mirage_Archer_Toxic_Rain` (main env, `offence:5650@main`): PoB `MirageDPS` 814579.66 /
… vs none in the Rust run, `CombinedDPS` short by exactly the mirage contribution.

Proposed ORC-X change: export a foreign table that a dumped local reaches through a calc field
(`activeSkill.mirage.output`, also `minion.output` of a foreign actor) by value (scalars and nested
plain tables, `$foreign` kept as an annotation), so injection can rebuild it as a detached table.
Until then `crates/pob-oracle/tests/OF-I.rs` reports such pairs as FOREIGN (not compared).

## 2. OF-I's L4 depends on OF-A and DF callees (unit table §11.1)

The row lists `OF-C (sigs)` only. The segment also calls:

- `calcResistForType` (OF-A, CalcOffence.lua:494-508) at 5713, 5755, 5830 (Decay, burning ground,
  every non-physical skill DoT in effective mode);
- `calcs.applyDmgTakenConversion` (DF, CalcDefence.lua:84-143) at 5971/5987/6003/6022/6037/6050/6064
  (self-hit sources; reached only with the matching mods or `HasTrauma`);
- `runSkillFunc("preDotFunc")` (OF-A, 509-514) at 5806 — no granted effect defines `preDotFunc`
  at the pin, so the stub is equivalent (the OF-I test asserts this against `data.skills`).

While the first two are `unported!`, a run that reaches them is reported BLOCKED. At OF-I's commit
9 of the 58 main-env hits are blocked (all on `calcResistForType`; every mismatch of those runs is
the resistance factor of `effMult` and what follows from it). The "CombinedDPS/TotalDot strict on
dps30" gate therefore covers 37 of the dps30 hits now; re-run `--test OF-I` after OF-A merges.

```diff
-| **OF-I** | CalcOffence.lua 5649-6263 | `calc_offence/dot_combined.rs` | OF-C (sigs) | …
+| **OF-I** | CalcOffence.lua 5649-6263 | `calc_offence/dot_combined.rs` | OF-C (sigs); OF-A, DF-A (callees, for the full L4 gate) | …
```
