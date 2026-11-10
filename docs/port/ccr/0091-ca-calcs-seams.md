# CCR-0091: CA (Calcs.lua) test seams and contract notes

Status: proposed by CA (port/CA, 2026-11-10)

No frozen signature of calc doc §5.10 changes. This CCR records what CA added next to the frozen
API and one change it made in three other units' test files, so the integrator can review them
together.

## 1. `buildOutput`'s blocks are public functions (addition)

`crates/pob-calc/src/modules/calcs.rs`: `calcs.buildOutput` (Calcs.lua:384-870) keeps one
`@pob` marker; its three post-processing blocks are separate `pub fn`s so the L5 test can run
each one on PoB's injected state (`crates/pob-oracle/tests/CA.rs`, `ca_l5_post`):

```rust
pub fn buildOutput_costWarnings(env: &EnvRef, mode: Mode, output: &Output); // Calcs.lua:400-435
pub fn buildOutput_main(env: &EnvRef, output: &Output);                     // 437-600
pub fn buildOutput_calcs(build: &BuildRef, env: &EnvRef, output: &Output);  // 602-866
```

`buildOutput` calls them in Lua order; nothing else does.

## 2. `calcs::replay_hook` (feature `probe` only; addition)

Evidence: `findTriggerSkill` (CalcTriggers.lua:79-81) calls `calcs.buildActiveSkill` whenever the
uuid is uncached **or `env.mode == "CALCULATOR"`**, and the Manaforged branch does the same. TR-A /
TR-B's L4 tests inject `triggers:in` on a placeholder build (`testkit::empty_build`) and stand the
nested calc in with the GlobalCache entries of `triggers:out` (their module docs). While
`buildActiveSkill` was CA's `unported!` stub the call was a no-op; ported, it runs `initEnv` on the
placeholder build, which raises (`attempt to index a nil value`, init_env_base.rs:298): TR-A went
from passing to 40 failing hits.

Change: `calcs.rs` gains, under `#[cfg(feature = "probe")]`, `pub mod replay_hook { Call, Hook,
set }` (the pattern of `calc_mirages::replay_hook` / CCR-0077), checked at `buildActiveSkill`'s
entry. The shipped calc has no hook.

Diff in other units' tests (minimal, marked in place):

```rust
// crates/pob-oracle/tests/TR-A.rs run_hit, crates/pob-oracle/tests/TR-B.rs run_hit
pob_calc::modules::calcs::replay_hook::set(Some(Box::new(|_| true)));
let run = std::panic::catch_unwind(...);
pob_calc::modules::calcs::replay_hook::set(None);
```

The same applies to PF-A's `getCachedOutputValue` probe: CalcPerform.lua:33-35 calls
`calcs.buildActiveSkill` whenever the uuid is uncached **or `env.mode == "CALCULATOR"`**, and PF-A
stands the callee in with `replay_build_active_skill` (the `:in → :out` delta). Ported, the call
ran `initEnv` on the placeholder build (4 of 12 `pfa_cached_outputs` hits, `#9..#12`, failed with
`attempt to index a nil value`). Same diff, guarded to that one function:

```rust
// crates/pob-oracle/tests/PF-A.rs run_hit (f == "getCachedOutputValue" only)
pob_calc::modules::calcs::replay_hook::set(Some(Box::new(|_| true)));
let run = std::panic::catch_unwind(...);
pob_calc::modules::calcs::replay_hook::set(None);
```

This restores exactly the assumption those tests were written under (the injected cache entries
replace the nested calc). Measured after the change: TR-A L4 563/563 hits pass (as before); PF-A
`pfa_function_probes_hit_sample` getCachedOutputValue 12/12 (as on rust-port f275edb).

## 3. Notes for the docs

- `ModStore:Flag` returns `true` or nothing in Lua (ModDB.lua 206-227, ModList.lua 156-175), so
  `output.EnergyShieldProtectsMana = env.modDB:Flag(nil, "EnergyShieldProtectsMana")`
  (Calcs.lua:406) leaves the key **nil** when the flag is off. The Rust `Flag` returns `bool`;
  any port that stores a Flag result in a table must store `true`/nil, not `false` (found by
  `ca_l5_post`; the same trap as FOLLOWUPS "CS0" item 1).
- `FullDPS.cullingDPS` (Calcs.lua:348, set only when `cullingMulti > 0`) is not a frozen field; CA
  keeps it in `FullDPS.extra`.
- calc doc §5.10 writes `MiscCalculator { usedFullDPS: bool }`; U0's frozen stub has
  `usedFullDPS: Cell<bool>` (kept).
