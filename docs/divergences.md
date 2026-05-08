# Divergences from PoB

A running list of places the Rust port computes something differently to the upstream
Lua codebase. Each divergence is a deliberate shortcut taken to ship a phase; the goal
is parity with PoB by v1.0.

Format: short heading, what we do, what PoB does, why it differs, and a tag for the
phase that should fix it.

## Modifier system

### Range mod values collapse to min — Phase 3a (closed in 7a)

Closed: `consume_number` now returns the average of the bounds for `(a-b)` paren-range
forms, so `+(20-30) to Strength` evaluates to +25 — matching PoB's display semantic for
non-itemised stats. The `Adds N to M Damage` form still emits `ModValue::Range { min, max }`
so range-aware consumers (the hit-damage calc) keep both bounds.

### Per-X scaling on parser-produced mods drops the multiplier — Phase 3a (closed)

Closed by phase 3a-cont: `1% increased Damage per Power Charge` now parses as an Inc mod
with a `Multiplier{var=PowerCharge}` tag. Verified by `mod_db::tests::multiplier_tag_scales`.

### Conditional clauses with `unless` are dropped — Phase 3a (closed in 7c)

Closed: `strip_unless_clause` recognises both `unless you've X recently` and
`unless you have X recently` and emits a negated Condition tag with the same
canonical var names as the `if you've X recently` path (`KilledRecently`,
`CritRecently`, `BeenHitRecently`, etc.).

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

### Hit chance uses fixed enemy evasion — Phase 3 (closed in 7b)

Closed: `ConfigState.enemy_evasion` exists, defaults to 1500 (the PoB level-84 baseline),
is surfaced as a slider in the Config tab, and read by the accuracy block in
`perform.rs::compute_with_skills` when computing `MainSkillHitChance`.

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

### Live-PoB validation harness — Phase 2g (operational; near parity)

`crates/pob-extract/src/bin/pob_diff.rs` boots PoB's full Lua codebase under mlua
(HeadlessWrapper + Launch + Modules/Main + tree data + uniques + rares + Build mode),
loads any `--build` XML through `main:SetMode("BUILD", …)`, runs the official calc
engine, pulls `env.player.output` (~617 scalar keys) back into Rust, and prints a
side-by-side diff against `pob_engine::compute_with_skills` for the comparable subset.

Status on Witch L90 baseline:
- 26 curated probe keys: 0 divergent
- 263 auto-discovered shared scalar keys: 1 divergent (TotalEHP off by ~0.2%
  due to PoB's iterative damage-shaving solver vs our analytic approximation)
- 0 PoB-only keys remaining — pob-engine emits every non-trivial PoB output

Marauder L90 baseline shows 2 divergent keys (TotalEHP +1.6, plus
`impaleStoredHitAvg` which PoB derives from a class-specific calc not yet
modelled in pob-engine).

LuaJIT → Lua 5.4 compatibility shims live in `build_lua_sandbox`:
- `jit.opt.start`, `unpack`, `loadstring`, LuaJIT-style `bit.*`
- `package.preload` stubs for native libs (lcurl/sha1/sha2/lfs/socket/cjson/dkjson/base64)
- functional `lua-utf8` mapping to builtin `string.*` and `utf8`
- `xml` loaded from PoB's own `runtime/lua/xml.lua`
- lenient `string.gsub` replacement-pattern shim (handles bare `%` escapes)
- `string.format` int-coercion for `%d`/`%i` with float arguments

Diff fixes the harness has surfaced and resolved:
- Post-Act-10 -60 resist penalty for level >= 68
- Path-validation: pob-engine only credits passive nodes reachable from class
  start (matches PoB; previously it summed every entry in `character.allocated`)
- CritMultiplier scale (decimal 1.5 ≡ 150%, not raw 150)
- CritChance defaults to 0 with no main skill selected
- Pool / hit-pool decomposition per damage type
- `*TakenHitMult` / `BaseTakenHitMult` / `TakenDotMult` per element
- ~50 game-constant outputs (max ailment magnitudes, charge defaults, totem
  resists, missing-resist deltas, attribute aliases, leech caps)

Open work:
- PoB's iterative damage-shaving solver (used for `MaximumHitTaken` and
  `TotalEHP`): pob-engine's analytic ratio approximation matches to ~0.2%.
  Implementing the solver would close the last divergence but is unlikely to
  matter for users.
- Per-skill chain damage scaling: Arc-style "+15% MORE damage per chain
  remaining" mods are loaded as PerStat:ChainRemaining MORE multipliers but
  applying them in the per-cast average overshoots, because PoB averages
  across the chain count (hit 0 has full bonus, hit ChainMax has none). For
  now we omit the scaling from the displayed AverageHit; full chain
  iteration is a Phase 3e follow-up.

Closed in this phase:
- Items, skill gem selection (with multi-group socketing + supports), and
  Config inputs all flow through `import_pob_xml` into CharacterState.
- Support gems linked into the active socket group buff the main skill via
  `skill_mods` + `addSkillTypes` / `excludeSkillTypes` filtering.
- Per-gem enabled toggle persisted through both PoB XML import and
  CharacterSnapshot share codes.
- Calcs tab gained a stat-breakdown side panel: click any stat to see every
  contributing mod (BASE / INC / MORE / FLAG) with source + tag annotations.
- Side panel grew a per-element defence section showing per-damage-type
  EHP and MaxHitTaken numbers.

### Tree rendering uses egui shapes, not wgpu — Phase 4a (open)

Performance is fine at typical zooms (~3000 nodes, sub-millisecond paint), but egui's
fixed-vertex pipeline allocates more per frame than necessary. A wgpu custom paint
callback would let us upload the static tree geometry once and only update colours on
allocation change. Optimisation; not correctness.
