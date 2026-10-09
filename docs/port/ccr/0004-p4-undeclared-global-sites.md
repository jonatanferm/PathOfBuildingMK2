# CCR-0004: §7.3 misses nine undeclared-global read sites

**Status: applied 2026-10-09 by CCR1.** Applied to calc doc §7.3 (and §12). `globals_lint.py --check` now parses the applied doc text; `CCR_0004_SITES` was dropped.

- **Raised by:** P4 (`scripts/port/globals_lint.py`)
- **Affects:** calc-architecture.md §7.3 (and D16), units CT, PF-B, OF-D, BM-B, SK-B, LUT
- **Pin:** 16de4b82d

## Problem

§7.3 states that the undeclared-global read sites "listed by `globals_lint.py` at the pin"
are `dotCfg` (CalcOffence 3527, 3538), `affectedByAura` (CalcPerform 3464) and `modSource`
(CalcDefence 1606), plus `nullValue` (ModDB.lua:262, ModList.lua:207). The scope-aware
lint (every identifier resolved with Lua's scoping rules; a name counts as declared when any
PoB runtime file assigns it as a global, when SimpleGraphic/HeadlessWrapper provide it, or
when it is a Lua/LuaJIT standard global) reports all six of those **and nine more** in the
calc scope. Each was checked by hand; each read always yields `nil` in PoB:

| Site | Name | Why it is nil | Owner |
|---|---|---|---|
| Classes/PartyTab.lua:919 | `aura` | the `for aura, ...` variable of 911-917 is out of scope; `list["Aura"]["Vaal"][aura]` indexes with nil | BM-B |
| Data/Skills/act_dex.lua:12991 | `cfg` | Scourge Arrow `preDamageFunc(activeSkill, output)` has no `cfg`; `Sum("BASE", nil, "Multiplier:ScourgeArrowStage")` | SK-B |
| Modules/CalcBreakdown.lua:212 | `skillCfg` | `breakdown.leech(...)` has no `skillCfg`; `calcLib.mod(modDB, nil, rate)` | CT |
| Modules/CalcBreakdown.lua:232 | `skillCfg` | same | CT |
| Modules/CalcOffence.lua:2506 | `cfg` | no `cfg` is in scope at the top level of `calcs.offence` there (the `local cfg` at 2180/2611/4169 are loop locals); `skillModList:Sum("BASE", nil, "Multiplier:TraumaStacks")` | OF-D |
| Modules/CalcOffence.lua:2507 | `cfg` | same (`Multiplier:VoltaxicWaitingStages`) | OF-D |
| Modules/CalcPerform.lua:915 | `skillCfg` | `doActorMisc`: the `local skillCfg` at 661 belongs to a block closed before 915; `Sum("BASE", nil, "MaximumRage")`, `More(nil, "MaximumRage")` | PF-B |
| Modules/DataLegionLookUpTableHelper.lua:136 | `scriptPath` | `repairLUTs`; `scriptPath` is only a local of UpdateCheck.lua:92 (a separate Lua state). `nil .. "/Data/..."` raises a Lua error if this path runs | LUT |
| Modules/DataLegionLookUpTableHelper.lua:144 | `scriptPath` | same | LUT |

Reproduce: `python3 scripts/port/globals_lint.py` (report) or `--check` (exit 0 only when
the reported set equals §7.3 ∪ this CCR).

Without this CCR the owning units would port e.g. CalcOffence.lua:2506 with a cfg in scope
and get different `BuffOnSelf` multipliers whenever a `Multiplier:TraumaStacks` mod carries
a cfg-dependent tag.

## Proposed diff (calc-architecture.md §7.3, second bullet)

```diff
-- The undeclared-global sites listed by `globals_lint.py` at the pin are: `dotCfg` (CalcOffence 3527, 3538), `affectedByAura` (CalcPerform 3464; `pairs(nil)` raises a LuaError when an `AffectedByAuraMod` LIST mod exists), and `modSource` (CalcDefence 1606, so `nil or "…"` takes the literal). Each is ported literally with `// LUA-GLOBAL-NIL`. `nullValue` (ModDB.lua:262, ModList.lua:207) is also an undeclared global (nil), so `EvalMod(…) or nullValue` leaves nil results out of `List`.
+- The undeclared-global read sites listed by `globals_lint.py` at the pin are: `dotCfg` (CalcOffence 3527, 3538), `cfg` (CalcOffence 2506, 2507), `affectedByAura` (CalcPerform 3464; `pairs(nil)` raises a LuaError when an `AffectedByAuraMod` LIST mod exists), `skillCfg` (CalcPerform 915; CalcBreakdown 212, 232), `modSource` (CalcDefence 1606, so `nil or "…"` takes the literal), `aura` (PartyTab 919), `cfg` (Data/Skills/act_dex.lua 12991, Scourge Arrow preDamageFunc) and `scriptPath` (DataLegionLookUpTableHelper 136, 144; concatenating nil raises a LuaError). Each is ported literally with `// LUA-GLOBAL-NIL`. `nullValue` (ModDB.lua:262, ModList.lua:207) is also an undeclared global (nil), so `EvalMod(…) or nullValue` leaves nil results out of `List`. `globals_lint.py` also lists implicit globals (assignments to undeclared names inside functions, e.g. `mult` CalcPerform 2575/2580, `newMod` 1925, `srcList` 1738, `uncancellableReservation` 563, `simBreakdown` CalcTriggers 814) and their reads; ports keep them as function locals unless a read before the write is reachable.
```

## Local workaround

`scripts/port/globals_lint.py` carries the nine sites as `CCR_0004_SITES` (marked
`CCR-0004`); `--check` accepts exactly §7.3 ∪ CCR-0004.
