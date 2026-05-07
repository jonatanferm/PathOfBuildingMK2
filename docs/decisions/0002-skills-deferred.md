# ADR 0002 — Skill data deferred to Phase 2

**Status:** accepted
**Date:** 2026-05-07

## Context

`src/Data/Skills/*.lua` defines per-skill metadata + level scaling, but it does so by
calling closures that *create* mod tables at load time:

```lua
statMap = {
    ["arc_damage_+%_final_for_each_remaining_chain"] = {
        mod("Damage", "MORE", nil, 0, bit.bor(KeywordFlag.Hit, KeywordFlag.Ailment),
            { type = "PerStat", stat = "ChainRemaining" }),
    },
},
```

The `mod()`, `flag()`, `skill()` functions are passed into the file as varargs and live
in `Modules/CalcActiveSkill.lua` and `Modules/Common.lua`. Faithfully extracting every
skill means modelling the mod construction protocol, not just walking tables.

## Decision

In Phase 1 we extract only the *static* skill data — name, baseTypeName, color, base
flags, level requirements, cost, cast time. We do **not** extract `statMap`, `qualityStats`,
or per-level `stats`/`statInterpolation` tables that involve the mod-construction closures.
We extract `Gems.lua` fully (it's pure data).

In Phase 2 (engine MVP) we'll teach the extractor to inert-stub the `mod`/`flag`/`skill`
helpers so they record their arguments instead of building Lua tables, then write those
recordings as JSON. By that point we'll have a `Mod` type to deserialise into.

## Consequences

- Phase 1 produces: `passive_tree.json`, `bases.json`, `gems.json`. No skills.
- The validation harness in Phase 2 starts with a tiny set of skills whose mod handling
  we've built out. We'll grow coverage incrementally.
- This means the engine cannot compute DPS for arbitrary skills until late Phase 2 / early
  Phase 3. Acceptable for the validation flywheel — we pick a few canonical skills first.
