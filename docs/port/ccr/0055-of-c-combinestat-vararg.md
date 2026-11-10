# CCR-0055: `combineStat`'s vararg is a stat name (OF-C)

**Status: applied 2026-11-10 by CCR2 (§1 calc doc §7.2); §2 needs code. See AMENDMENTS A8.**

Filed status: proposed (applied locally in `calc_offence/costs_passes.rs`, marked `CCR-0055`).
Unit: OF-C. Affects: calc-architecture.md §7.2 (frozen stub signature), callers OF-D, OF-F, OF-I.

## Problem

calc-architecture.md §7.2 (line 1346) and U0's stub freeze

```rust
pub(super) fn combineStat(ctx: &OffenceCtx, stat: Name, mode: Name, extra: Option<&Table>);
```

but the Lua vararg is the name of a chance stat, used as a key:

- `Modules/CalcOffence.lua:2066` `local function combineStat(stat, mode, ...)`
- `:2082-2083`, `:2130-2131` `output.MainHand[...] * output.MainHand.HitChance` (the first vararg
  indexes `output.MainHand` / `output.OffHand`)
- callers pass a string: `:5653` `combineStat("BleedDPS", "CHANCE_AILMENT", "BleedChance")`,
  `:5655` `combineStat("PoisonDPS", "CHANCE", "PoisonChance")`, `:5656`, `:5658`, `:5665`, `:5667`,
  `:5695` `combineStat("ImpaleModifier", "CHANCE", "ImpaleChance")`. No caller passes a table.

With `Option<&Table>` OF-I cannot pass `"BleedChance"`.

## Change

```diff
-// costs_passes.rs [OF-C]: pub(super) fn combineStat(ctx: &OffenceCtx, stat: Name, mode: Name, extra: Option<&Table>);
+// costs_passes.rs [OF-C]: pub fn combineStat(ctx: &OffenceCtx, stat: Name, mode: Name, extra: Option<Name>);
```

- `extra: Option<Name>`: `None` is a call without the third argument; `output.MainHand[nil]` reads
  nil, so the `CHANCE` / `CHANCE_AILMENT` arithmetic raises as in Lua.
- `pub` (was `pub(crate)` in the stub): the OF-C acceptance test (`crates/pob-oracle/tests/OF-C.rs`,
  `ofc_combine_stat_modes`) calls it on an injected ctx; there is no other way to exercise it
  before OF-D/OF-F/OF-I land.

Callers that pass `None` (every call at 2510-2514, 3959-3990 and most of 5652-5694) compile
unchanged; OF-I passes `Some(n!("BleedChance"))` etc.

## §2 (ORC-X, informational): an absent-marked typed field written with an empty table is not exported

`crates/pob-calc/src/probe.rs` `struct_members` (≈1451) skips a typed field when
`reg_absent(ptr, k) && val_is_default(&v)`, and `val_is_default` treats an empty table as default.
The injection marks every typed field that is nil in the `:in` dump absent. OF-C sets
`activeSkill.weapon1Cfg.skillStats = output.MainHand` (CalcOffence.lua:2008, likewise 2025) while
`output.MainHand` is still `{ }`, so the export after the segment drops the field and the L4 diff
reports `locals.activeSkill.weapon1Cfg.skillStats Missing` (pob `{"$ref":"locals.output.MainHand"}`)
on every attack build, although the Rust state is right (`skillStats == Some(output.MainHand)`).

Workaround (test side, `crates/pob-oracle/tests/OF-C.rs`): after the segment the test calls
`pob_calc::probe::unmark_absent(rc_ptr(&cfg), "skillStats")` for each weapon cfg whose `skillStats`
is set. Proposed ORC-X fix: treat `Some(x)` of an `Option<Table>`-typed field as present (or have
`Field::set` of a probed object clear its absent mark), so segment tests need no per-field
announcement. No other OF-C write hits this (the conversion tables and `infoMessage` are never
empty).
