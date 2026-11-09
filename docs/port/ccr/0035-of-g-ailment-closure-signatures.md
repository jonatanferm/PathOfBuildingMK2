# CCR-0035: OF-G — the ailment loop closures' frozen signatures differ from the Lua

Status: proposed by OF-G (2026-11-09); applied locally in `calc_offence/ailments_a.rs`
(OF-G owns the file and the three functions). OF-H is the only other caller and has not
been ported yet, so nothing else changes.

## Problem

U0's stub (`crates/pob-calc/src/modules/calc_offence/ailments_a.rs`, calc doc §7.2 comment
"ailments_a.rs [OF-G]: checkWeapon1HFlags(ctx, al, cfg) / calcAverageSourceDamage(ctx, al, …) /
calcAilmentDamage(ctx, al, …)") froze parameter lists that the Lua functions do not have:

| Function | Frozen stub | Lua (CalcOffence.lua) |
|---|---|---|
| `checkWeapon1HFlags` | `(ctx, al, cfg: &Cfg) -> bool` | `local function checkWeapon1HFlags(targetCfg)` sets `targetCfg.flags` and returns nothing (4308-4310); callers 4426, 4699 (OF-G), 5004 (OF-H) |
| `calcAverageSourceDamage` | `(ctx, al, ailment: Name, sourceType: Name) -> (f64, f64)` "→ (min, max)" | `local function calcAverageSourceDamage(ailment)` returns `sourceHitDmg, sourceCritDmg` (4315-4330); no second parameter; callers 5405, 5430 (OF-H), both as the last argument of `calcAilmentDamage`, so its two results fill `sourceHitDmg, sourceCritDmg` |
| `calcAilmentDamage` | `(ctx, al, ailment: Name, sourceHitDmg: f64, sourceCritDmg: f64, extra: Option<&Table>) -> Table` | `local function calcAilmentDamage(type, sourceCritChance, sourceHitDmg, sourceCritDmg, hideFromBreakdown)` returns the number `baseVal` (4333-4410); callers 4572-4574, 4843-4845 (OF-G), 5184-5186, 5405, 5430 (OF-H) |

A port with the frozen shapes would have to drop `sourceCritChance` and `hideFromBreakdown` and
return a table where every caller does arithmetic on a number (e.g. 4572
`calcAilmentDamage("Bleed", …, true) * basePercent / 100`).

## Change (applied)

```diff
-pub(crate) fn checkWeapon1HFlags(ctx: &OffenceCtx, al: &AilmentPassLocals, cfg: &Cfg) -> bool
+pub(crate) fn checkWeapon1HFlags(ctx: &OffenceCtx, al: &AilmentPassLocals, targetCfg: &ModCfg)
-pub(crate) fn calcAverageSourceDamage(ctx: &OffenceCtx, al: &AilmentPassLocals, ailment: Name,
-    sourceType: Name) -> (f64, f64)
+pub(crate) fn calcAverageSourceDamage(ctx: &OffenceCtx, al: &AilmentPassLocals, ailment: Name)
+    -> (f64, f64)                                    // (sourceHitDmg, sourceCritDmg)
-pub(crate) fn calcAilmentDamage(ctx: &OffenceCtx, al: &AilmentPassLocals, ailment: Name,
-    sourceHitDmg: f64, sourceCritDmg: f64, extra: Option<&Table>) -> Table
+pub(crate) fn calcAilmentDamage(ctx: &OffenceCtx, al: &AilmentPassLocals, r#type: Name,
+    sourceCritChance: f64, sourceHitDmg: f64, sourceCritDmg: f64, hideFromBreakdown: bool) -> f64
```

`hideFromBreakdown` is `bool`: the calls that omit it (4845, 5186, 5405, 5430) pass nil, which the body
reads only as `not hideFromBreakdown`, so `false` is equivalent.

Doc text to update (calc doc §7.2, the `ailments_a.rs [OF-G]` comment line): list the three
signatures above.
