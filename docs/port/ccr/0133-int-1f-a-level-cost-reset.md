# CCR-0133: a per-process reset for SkillsTab.lua:1190's cleared level costs

Unit: **INT-1F-A** (docs/port/parity/INT-1-fresh-triage.md §6; owner OF-C, with BM-B for the
reset). Status: **pending**. Code on `port/INT-1F-A` follows it. Amends CCR-0019 §8. Number
reserved with `scripts/port/next_ccr.sh`.

## 1. The write is process state; the oracle boots a fresh process per build

SkillsTab.lua:1188-1191 (`if gemInstance.triggered then … levels[gemInstance.level].cost = {}`)
writes into PoB's global `data`, so the cleared cost stays for the rest of the process.
CCR-0019 §8 keeps the write in the thread-local `LEVEL_COST_CLEARED`
(`crates/pob-calc-core/src/model/skills_tab.rs:141-155`), which nothing could clear.

The oracle boots a new PoB for every build (`pob_run` → `JitPob::boot_oracle`), but a CA test
process runs many builds on one thread. After an item-granted triggered level-1 Vulnerability
(fresh_015), every later build of the test would see a normal level-1 Vulnerability gem as free,
while its oracle run would not (triage §4 A, "process scope").

**Contract change** (calc doc §4.4 table, CCR-0019 §8 row): BM-B's `model/skills_tab.rs` gains

```rust
/// Forget every `levels[level].cost = {}` write of this thread, as a freshly booted PoB process
/// would. Rust-only (tests); the desktop app never calls it.
pub fn reset_level_cost_cleared();
```

It is the counterpart of `pob_gamedata::skill_stat_map::reset_stat_map_state` (CT, calc doc
§4.4). Oracle harnesses that run several builds per thread call it once per build, before the
build is loaded. `crates/pob-oracle/tests/CA.rs` does so in `ca_l6_corpus` (next to
`global_cache::clear_all()`) and in `ca_d13_fixed_point`. The desktop app keeps PoB's process
scope.

**Proposed diff** (calc-architecture.md §4.4, the `levels[level].cost` row): Accessor column
`pob_calc::model::skills_tab::levelCostCleared(id, level)` → add "; `reset_level_cost_cleared()`
for a freshly booted process (tests)".

## 2. Which level the reader keys (OF-C, CalcOffence.lua:1720)

Lua mutates the table `grantedEffect.levels[gemInstance.level]` and CalcOffence.lua:1720 reads
`activeSkill.activeEffect.grantedEffectLevel`, which CalcActiveSkill.lua:514/579 sets to
`activeGrantedEffect.levels[activeEffect.level]` with `activeGrantedEffect =
activeEffect.grantedEffect` (CalcActiveSkill.lua:95). The two are the same table exactly when
`(activeEffect.grantedEffect.id, activeEffect.level)` is the cleared pair, so
`calc_offence/costs_passes.rs` (L1720) checks `levelCostCleared(activeEffect.grantedEffect.id,
activeEffect.level)` and then yields nil. No contract change.

## 3. Other readers (not changed here)

CalcPerform.lua:1985-1991 (`ManaCostGainAsReservation` / `LifeCostGainAsReservation`, PF:
`calc_perform/pools_charges.rs`) also reads `grantedEffectLevel.cost` and does not consult
`levelCostCleared` yet (AMENDMENTS A8 item 11). INT-1F-A owns only the OF-C reader.
