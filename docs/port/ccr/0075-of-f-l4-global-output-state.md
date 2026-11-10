# CCR-0075: OF-F — L4 runs leave `globalOutput`/`globalBreakdown` unset

**Status: applied 2026-11-10 by ORC-X2 (`probe::run_segment` sets `OFFENCE_STATE.globalOutput/globalBreakdown` from the injected actor for every offence stage from 2174 on; the seeding in `tests/OF-F.rs`/`OF-H.rs` is deleted). The related `$foreign` parent: see CCR-0049 §4 / AMENDMENTS A9. See AMENDMENTS A9.**

Filed status: proposed by OF-F (2026-11-09)

## Problem

`globalOutput` and `globalBreakdown` are module upvalues of CalcOffence.lua (65/67), assigned at
the top of each pass-loop body (`globalOutput, globalBreakdown = output, breakdown`, 2176 / 2607 /
4165) and read after it: by OF-E (e.g. 3290 `globalOutput.IntimidatingUpTimeRatio`, 3300
`globalOutput.HitSpeed`), by OF-F (3433/3436/3441/3443 warcry effects, 3672-3677 leech instance
caps, 3751-3755 `globalOutput.Life/EnergyShield/Mana`, 3797-3801 burst damage, 3835 the burst
breakdown, 3881-3904 PvP, 4143), by `combineStat` (2137) and by the explosiveArrowFunc hook
(3038). In the port they live in `OFFENCE_STATE` (calc doc §7.3, `module_state.rs`).

A pob-state/1 dump carries only the env, the listed locals and the GlobalCache, so a stage dump at
`offence:3346`, `offence:3957`, `offence:2607`+ never carries these upvalues, and neither
`pob_oracle::inject::inject` nor `pob_calc::probe::run_segment` / `testkit::segment_diff` sets
`OFFENCE_STATE`. Every L4 run of an offence segment after 2176 therefore starts with
`globalOutput = nil` (or a stale value from an earlier run on the thread), and the first read
raises `attempt to index upvalue 'globalOutput'` (OF-F's `seg_passHit` reads it unconditionally
at 3672-3677, so every `offence:3346` hit would raise).

## Proposed diff (ORC-X files)

In `pob_calc::probe::run_segment`, for the offence stages from `offence:2174` on (every stage
whose segment runs after a `globalOutput, globalBreakdown = output, breakdown` statement), set

```rust
OFFENCE_STATE.with(|s| {
    s.globalOutput.set(Some(actor.output.get()));
    s.globalBreakdown.set(actor.breakdown.get());
});
```

before running the segment, where `actor` is the injected `actor` local. This is exact for every
offence stage: the three assignments all copy the function-level `output`/`breakdown` locals,
which are `actor.output`/`actor.breakdown` (CalcOffence.lua 354/356) and are never reassigned.
(The `2174` segment itself assigns them at 2176 before any read, so setting them there is
harmless.)

## Workaround (until applied)

`crates/pob-oracle/tests/OF-F.rs` (`run_hit`, cites CCR-0075) sets `OFFENCE_STATE` from the
injected actor before `run_segment`. OF-E, OF-G, OF-H and OF-I tests need the same.

## Related inject limit (report only; no contract change proposed)

On 21 of the 1169 `offence:3346` hits over slice5 + all57 + `corpus/OF-F` (allflame_exp_013 /
036 / 051, upstream_Dual_Savior, upstream_Mirage_Archer_Toxic_Rain), the active skill was built in
another env and reused: `activeSkill.baseSkillModList.parent` is
`{"$foreign": {"env": "env7", "path": "modDB"}}` in the dump. Injection cannot rebuild that store,
so every `skillModList` query that falls through to it sees no mods (e.g. `LifeOnKill` 0 instead
of 574 on allflame_exp_036 `offence:3346#4`). `tests/OF-F.rs` classifies these hits as
`foreign-parent` (not gated; at most 2 % of hits, calc doc §9.4). Dumping the foreign env's stores
reachable from the dumped graph would close the gap (ORC-X).
