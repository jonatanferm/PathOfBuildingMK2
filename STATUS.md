# Project status

## Headline numbers

- 5 crates, 100+ commits, ~13 000 lines of Rust.
- 259 tests pass workspace-wide.
- Release `pob-desktop` binary: ~9.6 MB on macOS arm64.
- Engine `compute()` averages 2.2 ms per call against the full 3.25 tree
  in release.
- **ModParser parses 100% of every PoB tree version (3.10–3.28 + every
  alternate / ruthless variant — ~156 000 stat lines)**. Lines that
  aren't recognised by a structured parser fall through to a Misc:
  Flag/Base mod that preserves the data, so nothing is silently dropped.

## What works today

A buildable, testable, runnable Path of Building MK2 desktop app with seven tabs and
real calc output.

- **`cargo build --workspace`** — all five crates compile clean.
- **`cargo run -p pob-extract --release`** — one-time extraction of 28 modern PoE
  passive trees, 1062 item bases, 810 skill gems, and 1488 skill effects from the
  upstream PoB checkout (`.PathOfBuilding/` in-repo, or `../PathOfBuilding/` legacy)
  into `data/`.
- **`cargo test --workspace`** — 259 tests pass across the workspace.
- **`cargo run -p pob-desktop --release`** — opens the app.

## End-to-end demo

1. Pick Witch in the side panel, then Occultist for the ascendancy.
2. Tree tab: drag, scroll-zoom. Type `Frenzy` in the search box, hover a matched
   notable, watch the orange shortest-path overlay light up; click each node along it
   and see Strength / Life / resists / DPS climb in the side panel.
3. Items tab: select Amulet, paste a copy of any rare amulet from the wiki, click
   "Equip from paste", flip back to Tree, see Life / mana / resists update.
4. Skills tab: filter "Arc", select it, slide level to 20. Side panel grows a Main skill
   block with Avg hit / Avg w/ crit / Speed / DPS.
5. Config tab: enable "Bleeding" condition. Allocate a passive that has "10% increased
   Damage while Bleeding" — DPS now reflects the bonus.
6. Calcs tab: filter "Resist" or "MainSkill" to see the raw output dictionary.
7. Notes tab: free-form notes, persisted with the build.
8. Import / Export tab: paste an upstream PoB share code or PoB XML; auto-detects
   format and imports class / ascendancy / level / allocated nodes / notes.
9. File menu: New (cmd+N), Open... (cmd+O), Save (cmd+S), Save As... (cmd+shift+S)
   — `.mk2` build file format, native file dialogs via `rfd`.
10. Status bar: tree version dropdown (3_25..3_28 + alternate / ruthless variants), node
    counts, save status, and contextual help.

## Module map

| Crate | What it owns |
|---|---|
| `pob-data` | Types + JSON loaders for tree, bases, gems, skills, items. Wasm-clean. |
| `pob-engine` | Mod / ModDB / ModParser / Env / perform; Character + ConfigState; share code; PoB-format import; SkillRegistry. Wasm-clean. |
| `pob-extract` | mlua-driven build-time tool. Sandboxes Lua with stub `mod` / `flag` / `skill` helpers + `SkillType`/`KeywordFlag`/`ModFlag` constants so PoB's data files run without their full calc engine. |
| `pob-ui` | egui app: tree (paint+pan+zoom+search+pathfind), items, skills, config, calcs, notes, import-export. |
| `pob-desktop` | Thin eframe wrapper. ~9.5 MB release binary. |

## Engine scope

Computed today (with mod sources from class base attributes, level, allocated tree
nodes, equipped items, and config-driven conditions/multipliers):

- Attributes: Strength / Dex / Int (with `+ to all Attributes` summed in).
- Pools: Life (50 + 12×(L-1) + Str/2 + mods), Mana (40 + 6×(L-1) + Int/2 + mods),
  Energy Shield, Ward.
- Resists: per-element raw + cap (75 + bonus) + capped Total. Chaos same.
- Defences: Armour, Evasion, Block (capped 75 + max bonus), Spell Block,
  Spell Suppression, Physical Damage Reduction vs 1000-pt baseline phys hit.
- Recovery: Life regen (flat + percent), Mana regen (1.75% baseline + flat × inc),
  Energy Shield Recharge baseline.
- Charges: cast / attack speed multipliers, crit chance + multiplier.
- Main skill (when set): base hit min/max from the skill's level data ×
  damageEffectiveness using PoB's *character-level-driven* effectiveness formula
  (mirrors `Modules/CalcTools.lua:198`); skill-flag-aware mod filtering (Spell /
  Attack / Melee / Projectile / Area); crit chance from skill data + INC mods; enemy
  resist with per-element penetration; hit chance vs evasion; mana cost from skill
  data; final DPS.
- Skill `statMap` integration: each skill's intrinsic mods (e.g. Arc's `+15% MORE
  Damage per remaining chain` from `constantStats × statMap`) are converted into real
  Mod objects via `pob_engine::skill::skill_mods()` and applied to the env before the
  damage query. Quality scales `qualityStats` linearly.
- Ailments: BleedDPS / PoisonDPS (with steady-state stacking) / IgniteDPS, factoring
  ailment chance, ailment-damage / damage-over-time / per-ailment damage multipliers.
- FullDPS aggregates the four damage sources.
- Effective HP per damage type: PhysicalEHP / FireEHP / ColdEHP / LightningEHP /
  ChaosEHP, with spell-suppression factored in for elemental.
- Items: paste-text parser handles implicit/explicit/crafted/enchant/fractured/
  corrupted/veiled sections; canonical bases (when `bases.json` is loaded) provide
  intrinsic armour / evasion / energy_shield / ward / block_chance values.
- Mastery node selections: Character.mastery_selections picks one of a mastery node's
  effects; perform pulls only the selected effect's stats.

Modifier system handles `Sum` / `More` / `Flag` / `Override` / `List` queries with
`Condition` / `ActorCondition` / `Multiplier` / `PerStat` / `PercentStat` /
`StatThreshold` / `MultiplierThreshold` / `SkillType` / `SkillName` / `SkillId` /
`SlotName` tag types. Tags from PoB skill-data closures (`mod()` / `flag()` /
`skill()`) are recovered from the inert recordings the extractor emits.

ModParser parses 100% of every modern tree version (~156 000 stat lines). Structured
parsers cover the bulk; an explicit fallback path emits a `Misc:<canonicalised>` Flag
or Base mod for the long tail so nothing is silently dropped. As specific calc
consumers come online they replace Misc: keys with real calc paths.

## What's documented

- [`docs/architecture-current.md`](docs/architecture-current.md) — map of the upstream
  Lua codebase being ported.
- [`docs/divergences.md`](docs/divergences.md) — running list of deliberate shortcuts.
- [`docs/packaging.md`](docs/packaging.md) — macOS / Windows / Linux build + bundle
  notes.
- [`docs/decisions/`](docs/decisions/) — ADRs for workspace layout (0001) and skill
  data extraction (0002, since closed).

## What's next

Closed since the previous status snapshot:

- Slot-conditional item mods, ascendancy 8-point cap, PoB XML export
  round-trip, ailment DPS overhaul (faster-* + EnemyMoving + AdditionalPoisonChance
  + PoisonStackLimit), paren-range averaging, `unless` clause parsing,
  CurseEffect / AuraEffect outgoing scaling, wgpu tree renderer.
- Per-skill chain damage scaling (issue #11): `output.ChainRemaining = ChainMax
  - skillChainCount` (default 0) restored, so Arc-style PerStat:ChainRemaining
  MORE bonuses now apply at full strength on the per-cast average.
- Enemy physical mitigation: enemy armour reduces physical hit damage via PoB's
  `armour / (armour + 5 × raw)` formula, with a config slider + boss-preset
  defaults. Hit-side block / dodge / suppression are read from
  `enemy_block_chance` / `enemy_dodge_chance` / `enemy_suppression_chance` and
  fold into `MainSkillDPS`.
- Projectile shotgun multiplier (`projectiles_hitting_target` config, capped at
  `ProjectileCount`); AoE shotgun overlap (`enemies_hit_by_aoe`).
- Trap / mine timing model: per-throw counts, multi-throw penalty, per-skill
  cooldown gating, DoT-only throw timing, cast-speed isolation.
- Warcry layer: WarcryPower config, loadout aggregates, auto-uptime,
  per-cry active markers (Intimidating / Enduring / Ancestral / Seismic /
  Battlemage's / Rallying / General's), Intimidate enemy debuff,
  Enduring Cry life regen, Ancestral Cry elemental resists, Seismic
  Cry armour + stun threshold, Battlemage's Cry crit chance, Rallying
  Cry per-ally exert damage. Buff injection re-ordered so basic-stat
  outputs reflect the cry buffs end-to-end (LifeRegen, FireResist,
  Armour, MainSkillCritChance). Remaining infra-blocked warcries
  (Rallying ally projection / Infernal phys-to-fire / General's
  parallel actor) tracked in [#145](https://github.com/jonatanferm/PathOfBuildingMK2/issues/145).
- Pantheon: soul levels 1-4 + NearbyEnemies / OnlyOneNearbyEnemy condition.
- Flask recovery: instant/gradual split, low-life multiplier, LifeAdditional.
- Party tab auto-extraction with auto AuraEffect detection and
  manual % override; user edits preserved across re-paste.
- External-site URL recogniser (pobb.in / pastebin / poeplanner).
- NearbyAllies config + Multiplier:NearbyAlly for Rallying Cry's
  per-ally exert damage and ally-scaling PerStat mods.
- CritChance BASE addition path in both `perform_basic_stats` and
  `perform_skill_dps`, enabling Battlemage's Cry / Diamond Flask /
  Watcher's Eye / Power Charge On Critical Strike to lift headline
  crit instead of being silently dropped.
- AscendancyStart medallions render the real `AscendancyMiddle`
  sprite from a bundled `ascendancy.png` atlas; class-start
  portraits are gated on the allocated class so only the active
  class shows its dedicated portrait, the other six fall back to
  the inactive background. (Issue #110.)
- CI: `cargo fmt --check` and `cargo clippy -D warnings` are gated, not
  advisory.
- Calcs-tab section layout port (#34): `data/calc_sections.json` ships
  the full PoB section tree (29 sections, 600+ rows). The Calcs tab has
  an opt-in **PoB layout** view that renders the imported sections in
  three columns (Offence / Core / Defence) with per-row output values.
  Skill-flag visibility (`flag = "spell"` / `notFlag = "attack"`) is
  honoured against the active main skill's `baseFlags` so spell builds
  stop seeing weapon-attack rows.
- Cluster jewel data foundation (#21): `data/cluster_jewels.json` and
  `data/cluster_jewel_mods.json` capture the three jewel categories
  (Small / Medium / Large with their ring slots + small-passive
  options) and 557 prefix / suffix / corruption mods. Sub-graph
  synthesis (placing synthesised notable nodes into the live tree) is
  the next slice.
- Tattoo full pipeline (#98): catalogue (167 tattoos in
  `data/tattoos.json`) + right-click picker on the Tree tab + gold
  badge overlay on tattooed nodes. PoB-XML round-trip already worked
  via PR #93's engine-side override mechanism.
- Minion build foundation (#20): `data/minions.json` (62 minions with
  base life / damage / resists / cap counters / mod recordings); the
  four `monster_*_life_table` arrays from `Data/Misc.lua`; and a
  `MinionState` skeleton that surfaces `MinionLife` / `MinionFireResist`
  etc. on the player's output for the active main skill's primary
  minion. A real minion perform pass (with `MinionLife` INC / MORE
  scaling, support-gem mods, and minion DPS) is the next slice.
- mod_parser canonical key naming for `Gain N% of <Source> as Extra
  <Target>` mods. Item-text mods now mint `<Source>DamageGainAs<Target>`
  (matching PoB) instead of MK2-internal `<Target>DamageGain`, so they
  combine with the same key `aura_buff_mods` already produces from
  Hatred-shape skill statMaps.
- pob-ui scaffold: `LoadedApp` carries `cluster_jewels`,
  `cluster_jewel_mods`, `tattoos`, `minions`, and `calc_sections` so
  feature slices don't have to re-do the load plumbing.
- Wasm Builds tab via IndexedDB (#101): `app/pob-web` users can save and
  load `.mk2` builds across page reloads using IndexedDB, with manual
  download as a fallback.

Still open (in rough priority):

1. **AoE radius rolloff and projectile pierce/chain variance**: we now model
   shotgun overlap and the per-target multiplier, but not AoE damage falloff
   or pierce/chain damage variance per hop.
2. **Damage conversion calc-side read (`PhysicalDamageGainAs<Element>`)**:
   the mod_parser now mints PoB-canonical keys, but the calc pipeline still
   doesn't *read* them to add converted element damage to the hit total.
   Same gap blocks Infernal Cry's phys-as-fire piece.
3. **Live `pob_diff` ailment baselines in CI**: reference builds exist
   (`marauder_l90_bleeding_cleave.xml`, `witch_l90_arc_with_items.xml`,
   etc.), but locking PoB-vs-engine deltas behind a regression test still
   requires running pob_diff in the test environment.
4. **Cluster jewel sub-graph synthesis (#21)**: data is extracted; the
   tree-side synthesis pass that places synthesised notables into the
   allocated tree when a Cluster Jewel is socketed is the next slice.
   Timeless jewels (#30) and the generic radius-jewel framework (#31) are
   separate follow-ups.
5. **Real minion perform pass (#20)**: `MinionState` is wired and Life /
   resists land on the player's output, but the next slice needs a real
   minion-side `ModDB` so player-side `MinionLife` INC / MORE scaling and
   support-gem mods route through, and `MinionDPS` becomes a real number.
6. **Vaal / alternate skill variants per gem (#36)**: variant-picker UI
   ships, but alt-quality (Anomalous / Divergent / Phantasmal) needs the
   per-variant `qualityStats` selected at compute time + PoB XML
   `qualityId` round-trip.

## Build commands cheat sheet

```bash
# Build everything
cargo build --workspace

# Run desktop app (after extracting data)
cargo run -p pob-extract --release           # one-time
cargo run -p pob-desktop --release           # day-to-day

# Run tests
cargo test --workspace                       # most things
cargo test --release -p pob-engine           # includes the perf smoke test

# Lints
cargo clippy --workspace --all-targets
cargo fmt --all
```
