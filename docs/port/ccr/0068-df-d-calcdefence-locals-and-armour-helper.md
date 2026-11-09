# CCR-0068: DF-D — CalcDefence module locals; `calcs.armourReduction` still a stub

Status: proposed by DF-D (2026-11-09)

DF-D (`calc_defence/est_inputs.rs`, CalcDefence.lua 1681-2152) reads module-level locals of
CalcDefence.lua and calls one module function, all owned by DF-A (`calc_defence/helpers.rs`,
CalcDefence.lua 1-650). The code workaround is marked `// CCR-0068`.

## §1 `isElemental`, `hitSourceList`, `dmgTypeList` are not exposed

Same gap as CCR-0067 §1 (DF-E), which proposes `pub(crate)` items for these tables in
`helpers.rs`; this CCR only adds DF-D's reads to the evidence and asks for `hitSourceList`
explicitly (CCR-0067 mentions it as "likewise").

**Evidence.** CalcDefence.lua:24 `isElemental`, 27 `hitSourceList`, 28 `dmgTypeList`; reads in
the DF-D range: `dmgTypeList` 1713, 1741, 1765, 1776, 1783, 1789, 1850, 1856, 1887, 1888, 1913,
1926, 2001; `isElemental` 1715, 1742, 1817, 1858, 1860, 1929, 1936, 1952, 1965; `hitSourceList`
1942. `crates/pob-calc/src/modules/calc_defence/helpers.rs:17-18` (comment marker only).

**Proposed diff (DF-A's file), in addition to CCR-0067 §1.**

```diff
 // crates/pob-calc/src/modules/calc_defence/helpers.rs (CalcDefence.lua:7-43 marker)
+pub(crate) const hitSourceList: [&str; 2] = ["Attack", "Spell"]; // L27
```

and `isElemental` taking `&str` (or `Name`; DF-D calls it with `&str` from `dmgTypeList` and
with a `Name` key read back from a table at 1817, so either works with `.as_str()`).

**Workaround (DF-D-owned).** `est_inputs.rs` keeps private copies `isElemental(&str)`,
`hitSourceList`, `dmgTypeList` with no `@pob` marker. When DF-A lands with the items, delete the
copies and `use super::helpers::{isElemental, hitSourceList, dmgTypeList}`.

## §2 `calcs.armourReduction` is still `unported!` (no contract change)

DF-D calls `armourReduction` (CalcDefence.lua:75-77, frozen signature `(armour, raw) -> f64`) at
2030 and 2033. Until DF-A merges, the stub returns `0.0`, so every hit with armour (or ward-based
physical reduction) differs from PoB in `<type>DamageReduction`, `<type>TakenHit`,
`<type>TakenHitMult`, `<type>BaseTakenHitMult`, `totalTakenHit` and the breakdowns.

DF-D does **not** copy the body (it is DF-A's). `crates/pob-oracle/tests/DF-D.rs` counts a hit
whose segment run reached an `unported!` site as *blocked* (listed with the sites), not as a
failure, and gates the judged hits only.

Results (2026-11-09, branch merged with rust-port at 94e3c58):

| corpus | hits | committed tree (stub): judged / clean / blocked | Lua body of 67-77 patched in locally (not committed): clean |
|---|---|---|---|
| all57 | 363 | 208 / 208 / 155 | 363 / 363 |
| unit corpus `corpus/DF-D` (33 builds) | 198 | 99 / 99 / 99 | 198 / 198 |
| slice5 | 30 | 15 / 15 / 15 | 30 / 30 |

Every blocked hit reports the single stub site `Modules/CalcDefence.lua:75-77`. Once DF-A
merges, the blocked count must drop to 0.
