# CCR-0044: L4 cannot replay a callee in the middle of a segment (OF-B)

**Status: partly applied 2026-11-10 by ORC-X2: the `unported!` replay hook (`pob_gamedata::stub::set_replay_hook`) and item 2 (another env's actors and mod stores are dumped in full, so a copied skill's store chain injects with its mods); `testkit::segment_diff_with_replay` and the `[[function]]` probes for OF-A's helpers still need code. See AMENDMENTS A8, A9.**

Filed status: proposed. Unit: OF-B (test side only; nothing in the frozen calc API changes for
shipping code). Affects: ORC-X (`pob_oracle::replay`, `testkit`), U0 (`pob_calc::stub`), every
segment unit whose callees are not yet ported (calc doc §9.2 "Replay", §11 OF-A..I rows).

## Problem

Calc doc §9.2 says `pob_oracle::replay` "swaps a not-yet-ported callee for its recorded
`in→out` state delta, keyed by hit id", naming "OF-A calling skill hooks". As built
(`crates/pob-oracle/src/replay.rs`), `Replay::apply` applies a recorded delta to a live state
**between** Rust calls. A segment is one Rust function, so a callee it calls in the middle
(OF-B: `runSkillFunc("preSkillTypeFunc")` at CalcOffence.lua:1054, `calcAreaOfEffect` 1185,
`calcSkillDuration` 1220, `calcSkillCooldown` 1299/1320, `calcAreaMod`/`calcRadius`/
`calcRadiusBreakpoints` 1311-1317 and 1395-1410, `calcTotemLife` 1440, `calcWarcryCastTime`
1469) runs as its `unported!` stub (`Default` return, no writes). There is no point where the
test can apply the callee's delta, nor can it supply a return value: `unported!`
(`crates/pob-gamedata/src/stub.rs`) only counts the hit.

Two further gaps showed up in OF-B's L4 corpus run:

1. Only `runSkillFunc` and the skill hooks are function probes; OF-A's local helpers
   (`calcAreaOfEffect` 374, `calcRadius` 173, …) are not, so even a hook would have no recorded
   `:in`/`:out` pair to replay (a `[[function]]` stages entry can add them).
2. A mirage/trigger env's skill made by `copyActiveSkill` (CalcActiveSkill.lua:184) chains
   `skillModList → baseSkillModList → <source env>.modDB`; the nested env's dump writes that
   parent as `{"$foreign": {"env": "env7", "path": "modDB"}}`, which injects as an empty mod
   store, so every query through it is wrong (allflame_exp_013 `offence:1037#4`: Kinetic Blast
   in a mirage env; `FarShot`, `ProjectileCount` read 0).

## Proposed change

```diff
 // crates/pob-gamedata/src/stub.rs (U0)
+/// Test hook: when set (thread-local), `unported!` sites with a registered replay call it
+/// instead of returning `Default`. Never set outside tests.
+pub fn set_replay_hook(f: Option<Box<dyn Fn(&'static str) -> Option<Box<dyn std::any::Any>>>>);
 macro_rules! unported {
-    ($site:literal) => {{ $crate::stub::hit($site); Default::default() }};
+    ($site:literal) => {{ $crate::stub::hit($site);
+        $crate::stub::replay($site).unwrap_or_default() }};
 }
```

with `pob_oracle::testkit::segment_diff_with_replay(build, stage_in, stage_out, callees)`
installing a hook that, on the n-th hit of a site, applies the n-th recorded `<callee>:in/out`
delta of the same enclosing call (matched by the dumps' `stack`) and decodes the recorded
`ret` (`Applied::ret`) into the stub's return type; plus `[[function]]` probes for OF-A's
local helpers in `docs/port/stages.toml`, and foreign-env injection (dump the referenced
env's object, or inject the source env alongside).

## What OF-B does meanwhile (`crates/pob-oracle/tests/OF-B.rs`)

Each L4 run records the unported sites it hit (`stub_report` before/after) and attributes a
mismatch to a hit stub only when its path is one the callee writes, or one an OF-B statement
writes from the callee's return value (a table in the test citing the Lua lines). Every other
path must match. Hits with a `preSkillTypeFunc` hook or a `$foreign` mod-store parent that
still differ are listed separately and excluded from the gate. `OFB_STRICT=1` fails on any
attribution (the gate once OF-A is merged: the stubs are then no longer hit).
