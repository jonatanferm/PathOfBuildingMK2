# Divergences from PoB

A running list of places the Rust port computes something differently to the upstream
Lua codebase. Each divergence is a deliberate shortcut taken to ship a phase; the goal
is parity with PoB by v1.0.

Format: short heading, what we do, what PoB does, why it differs, and a tag for the
phase that should fix it.

## Modifier system

### Range mod values collapse to min — Phase 3a (open)

`+(20-30) to Strength` parses as `+20 Base Strength`. PoB uses the average of the range
when displaying a non-itemised stat (e.g. on a passive node) and the rolled value
otherwise. Targeted fix: `ModValue::Range` is already first-class; surface min/avg/max
in `eval_mod` so the calc layer uses the average for tree mods.

### Per-X scaling on parser-produced mods drops the multiplier — Phase 3a (closed)

Closed by phase 3a-cont: `1% increased Damage per Power Charge` now parses as an Inc mod
with a `Multiplier{var=PowerCharge}` tag. Verified by `mod_db::tests::multiplier_tag_scales`.

### Conditional clauses with `unless` are dropped — Phase 3a (open)

`unless you have used X recently` clauses are not parsed. We strip "if you've X recently"
and "while X" but not the `unless` form.

### `Effect of` modifiers are stat-name-only — Phase 3 (open)

`Effect of your Curses`, `Effect of non-Curse Auras` etc. parse to a `CurseEffect` /
`AuraEffect` Base mod, but the calc engine doesn't yet apply that to outgoing curse /
aura mods (PoB applies it as a multiplier to the relevant skill mods). Targeted fix
lives in skill mod assembly when ActiveSkill grows from a stub to a real type.

## Calc engine

### Ailment damage is rough — Phase 3 (open)

BleedDPS / PoisonDPS / IgniteDPS use:
- `phys_avg × 0.70 × (1+inc/100) × more` for bleed
- `avg × 0.30 × (1+inc/100) × more` for poison (single stack)
- `avg × 0.90 × (1+inc/100) × more` for ignite

PoB correctly handles ailment chance, ailment scaling damage (poison stacks ramp linearly
with cast speed; ignite is single-instance non-stacking; bleed has a movement modifier),
duration mods, faster ailment damage, and ailment magnitude. We do not yet model any of
those.

### Hit chance uses fixed enemy evasion — Phase 3 (open)

`MainSkillHitChance` uses `enemy_evasion = 1500.0`. PoB pulls the enemy evasion from the
ConfigState. Targeted fix: add `enemy_evasion` to `ConfigState`, surface a slider in the
Config tab, and read it here. Rationale for the shortcut: 1500 is the PoB-default baseline
evasion for level 84 enemies, so the number is correct for the default config.

### No ascendancy point counter — Phase 3 (open)

We let users allocate any node, including ascendancy nodes, without checking the 8-point
ascendancy budget. PoB enforces it via `PassiveSpec:CountAllocNodes` plus a paint-step
gate. Easy fix: extend `Character::allocated` semantics to track a separate
`ascendancy_allocated` set with a 8-point cap.

### Skill DPS is single-target hit + ailment, no enemy mitigation — Phase 3 (open)

We compute `MainSkillDPS = final_avg × cps` after applying enemy element resist + hit
chance, but do not model:
- AoE / projectile / chain stat-derived mods (e.g. Arc's "more damage per remaining chain"
  is in the skill data but the calc layer doesn't apply the `PerStat ChainRemaining` tag
  yet — that requires a `ChainRemaining` value to live in `EvalState`).
- Enemy armour mitigation against physical hits.
- Block / dodge / suppression on the *defender* side from the player's perspective.

PoB walks all of these; ours doesn't yet.

### Items don't apply slot-conditional mods — Phase 3 (open)

A unique boots' `while you've taken a Critical Strike Recently, …` mod parses cleanly but
applies unconditionally because the parser doesn't emit a `SlotName` tag and the engine
doesn't filter by slot when applying item mods. The `apply_item_set` source attribution
gives us `Source::Item(slot_index)` so the engine *could* filter — Phase 3 fix is to add
a `slot_only` config knob to the apply pass.

## Data extraction

### Skill files: closures inside skill tables become sentinels — Phase 3c (closed)

A few skill definitions reference inline Lua closures (typically for stat-interpolation
or per-level overrides). We can't represent those in JSON, so the extractor emits
`{__lua_function: true}` in those slots. The calc engine never reads those fields today.

### Tree data: alternate / ruthless trees treated identically — Phase 1 (closed)

PoB tags ruthless and alternate trees with separate game flags so the UI can colour them
differently / restrict access. We extract them into separate JSON files but the UI only
loads `3_25.json` by default. Phase 4 polish should expose a tree-version dropdown.

## UI

### POB share-code import is MK2-only — Phase 5 (closed)

Closed: `pob_engine::import_pob_code` and `import_pob_xml` parse upstream PoB share
codes. Items / skills / config are still empty in the imported `Character` — the
upstream document encodes those as nested elements with attribute-encoded data, which is
non-trivial to round-trip.

### POB share-code export is MK2-only — Phase 5 (closed for class+tree+notes)

Closed: `pob_engine::export_pob_code` writes a PoB-readable XML document with class,
ascendancy, level, allocated nodes, and notes. PoB will accept it and fill items /
skills / config with defaults. Round-tripping items + skill setup back to upstream PoB
requires full document serialisation.

### Live-PoB validation harness — Phase 2g (skeleton)

`crates/pob-extract/src/bin/pob_diff.rs` boots an mlua sandbox with the SimpleGraphic
shims PoB needs at module-load time, plus the `SkillType` / `KeywordFlag` constants. It
does *not* yet load PoB's full `Modules/` + `Classes/` graph and run a build through it
— that's the next chunk: stub or vendor enough of `Common.lua` / `Main.lua` /
`HeadlessWrapper.lua` to reach `calcs.buildOutput`. Hardcoded reference values in
`crates/pob-engine/tests/validation.rs` cover the regression-detection role until then.

### Tree rendering uses egui shapes, not wgpu — Phase 4a (open)

Performance is fine at typical zooms (~3000 nodes, sub-millisecond paint), but egui's
fixed-vertex pipeline allocates more per frame than necessary. A wgpu custom paint
callback would let us upload the static tree geometry once and only update colours on
allocation change. Optimisation; not correctness.
