# CCR-0043: `calcRadiusBreakpoints` returns four nils for a zero radius (OF-B)

Status: proposed (worked around locally in `calc_offence/skill_types.rs`, marked `CCR-0043`).
Unit: OF-B. Affects: the OF-A stub signature in `calc_offence/helpers_setup.rs` (frozen by U0),
callers OF-A (`calcAreaOfEffect`) and OF-B.

## Problem

U0 froze

```rust
/// @pob Modules/CalcOffence.lua:198-224
pub(crate) fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64,
                                    radiusMod: Option<f64>) -> (f64, f64, f64, f64);
```

but the Lua only assigns the four results inside `if radius > 0 then` and otherwise returns four
nils:

- `Modules/CalcOffence.lua:199-201` `local radius = calcRadius(…)` / `local incAreaBreakpoint, …`
  (declared, unassigned) / `if radius > 0 then`
- `:223` `return incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint`

The nils are observable: `breakdown.area` prints the two breakpoint lines only
`if incBreakpoint and moreBreakpoint and redBreakpoint and lessBreakpoint`
(`Modules/CalcBreakdown.lua:106`). CT already took `Option<f64>` breakpoints in
`Breakdown::area` for exactly this reason (CCR-0014). With the frozen `f64` return, a caller
cannot tell "0% needed" from nil. Callers: CalcOffence.lua 394, 406, 461, 475 (OF-A,
`calcAreaOfEffect`), 1316, 1400, 1408 (OF-B: trap trigger, mine detonation, mine aura radius
breakdowns).

## Change

```diff
-pub(crate) fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64,
-                                    radiusMod: Option<f64>) -> (f64, f64, f64, f64);
+pub(crate) fn calcRadiusBreakpoints(baseRadius: f64, incArea: f64, moreArea: f64,
+                                    radiusMod: Option<f64>)
+    -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>);
```

## Workaround until applied

`skill_types.rs::radius_breakpoints` calls the frozen function and maps its result to four
`None`s when `calcRadius(baseRadius, calcAreaMod(incArea, moreArea), None) > 0` is false — the
test the Lua makes at line 201, with the same pure helpers (no side effects, so calling them
again is unobservable). Once applied, the wrapper is replaced by a direct call.
