# Project status

## Headline numbers

- 5 crates, 100+ commits, ~13 000 lines of Rust.
- 444 tests pass workspace-wide.
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
- **`cargo test --workspace`** — 444 tests pass across the workspace.
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
  Battlemage's / Rallying / General's / Infernal), Intimidate enemy debuff,
  Enduring Cry life regen, Ancestral Cry elemental resists, Seismic
  Cry armour + stun threshold, Battlemage's Cry crit chance, Rallying
  Cry per-ally exert damage. Buff injection re-ordered so basic-stat
  outputs reflect the cry buffs end-to-end (LifeRegen, FireResist,
  Armour, MainSkillCritChance).
- Issue #145 ([#145](https://github.com/jonatanferm/PathOfBuildingMK2/issues/145)):
  remaining warcries — Rallying Cry per-ally weapon-damage projection
  (lands as `Damage` MORE on each enabled `Party:<name>` source),
  Infernal Cry phys-as-fire (injects `PhysicalDamageGainAsFire` BASE
  scaled by WarcryPower / 5, capped at 25), and General's Cry parallel-
  actor envelope (mirage count + cooldown + DPS contribution to
  FullDPS for melee-tagged main skills).
- `PhysicalDamageGainAs<Element>` consumer in the calc pipeline
  (`perform_skill_dps`): for any physical-hit skill, the four
  `PhysicalDamageGainAs{Fire,Cold,Lightning,Chaos}` BASE percentages
  add an extra hit of the gained element scaled by phys avg ×
  pct/100, then attenuated by that element's resist + penetration.
  Also reads the `NonChaosDamageGainAs<X>` aggregator. Mirrors PoB's
  `Modules/CalcOffence.lua:1869` damage-conversion block.
- Pantheon: soul levels 1-4 + NearbyEnemies / OnlyOneNearbyEnemy condition.
- Flask recovery: instant/gradual split, low-life multiplier, LifeAdditional.
- Party tab auto-extraction with auto AuraEffect detection and
  manual % override; user edits preserved across re-paste.
- External-site URL recogniser (pobb.in / pastebin / poeplanner).
- Issue #33: external-site **fetch** + import. Pasting a `https://pobb.in/<id>`
  or `https://pastebin.com/<id>` URL into the Import-Export tab spawns a
  background `ureq` GET, decodes the body via `import_pob_code`, and swaps
  the active character. Errors (404 / 429 / network / decode) surface as
  readable banner messages. Wasm shows a "desktop only" stub for the URL
  branch (CORS). Poeplanner is recognised but flagged as unsupported for
  full-build import — upstream PoB only imports its passive-tree URLs.
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
  stop seeing weapon-attack rows. Click-through breakdown re-derivation
  (`pob_engine::breakdown::derive_for`) ports the Damage / Speed / Crit
  slice of `Modules/CalcBreakdown.lua`: clicking a row in the Calcs tab
  opens a step-by-step panel that walks Base → INC → MORE → quality →
  crit factor → speed → DPS for the headline keys
  (`MainSkillAverageHit`, `MainSkillDPS`, `FullDPS`,
  `(Cast|Attack|Movement)SpeedMult`, `MainSkillSpeed`, `CritChance`,
  `CritMultiplier`, `CritEffect`, etc.). Rows without a custom
  breakdown still render the legacy contributing-mods view.
- Cluster jewel data foundation (#21): `data/cluster_jewels.json` and
  `data/cluster_jewel_mods.json` capture the three jewel categories
  (Small / Medium / Large with their ring slots + small-passive
  options) and 557 prefix / suffix / corruption mods.
- Cluster jewel sub-graph synthesis (#21):
  `pob_engine::cluster_synth` parses a cluster jewel item's
  `Adds N Passive Skills` / `1 Added Passive Skill is X` /
  `Added Small Passive Skills grant: …` mod lines, materialises a
  synth sub-graph of notable / small / inner-socket nodes around the
  parent Large jewel socket, and emits collision-free synthetic
  `NodeId`s following PoB's `BuildSubgraph` id scheme. The new
  `Character.jewels` map keys those parsed jewels by host socket id;
  the perform entry point `compute_full_with_clusters` injects mods
  from any synthesised node that's both allocated and connected to
  an allocated parent socket. UI plumbing (Tree-tab right-click to
  paste a cluster jewel + sub-graph rendering near the host socket)
  and PoB-XML `<Slot name="Jewel N">` round-trip are follow-up slices.
- Radius-jewel framework (#31): `pob_data::jewel_radius` ships the
  canonical 3.16+ radii table (Small=960, Medium=1440, Large=1800,
  Very Large=2400, Massive=2880) plus the 3.15-and-older fallback,
  and `pob_engine::jewel_radius` exposes node-position math, in-radius
  scans, vanilla "X% increased Y to Passives in Radius" identification,
  and `apply_radius_jewels` — for each jewel socketed into a tree
  socket, the framework parses every mod line (stripping the "to
  nearby allocated passives" / "from Passives in Radius" trailers),
  identifies the radius bucket (defaults to Medium; explicit "Only
  affects Passives in <Size> Ring" overrides), and emits one mod copy
  per in-radius allocated node sourced as `Source::Passive(node_id)`.
  `Character::socketed_jewels` (a `SocketedJewels` Vec keyed by tree
  socket node id) round-trips through `CharacterSnapshot`, so saves
  preserve socketed jewels. Cluster / Abyss / Timeless / Charm jewels
  are stored in the same map but routed through their own dispatch
  paths and skipped by `identify_radius_jewel`'s subtype check.
  Timeless jewels (#30) and the named-unique handlers
  (Watcher's Eye / Healthy Mind / Karui Heart / Pure Talent /
  Intuitive Leap / Conqueror's Efficiency) build on the
  `HandlerKind` enum exposed by this slice.
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
- Jewel sockets PoB-XML round-trip (#195): `<Slot name="Jewel <NodeId>"
  itemId="…"/>` now imports + exports for both cluster jewels (routed
  through `character.jewels`) and radius / timeless / abyss jewels
  (routed through `character.socketed_jewels`). Builds with jewels
  round-trip cleanly through the wire format.
- Tree-tab search QoL (#205): Cmd/Ctrl+F focuses, Enter cycles through
  matches in deterministic node-id order, Esc clears. The matching
  walks node `name` + every `stats` line, case-insensitive.
- Cluster jewel paste UI (#197): right-click a Large jewel socket,
  paste a Cluster Jewel item, and the sub-graph renders near the
  host socket. Corruption-roll handling reads `cluster_jewel_mods.json`.
- Timeless jewel keystone replacement slice 1 (#30 → #219): the 23
  conqueror keystones across the six Timeless jewels swap into
  `output.Keystone:<replacement>` when a Timeless jewel is socketed
  in a radius that contains the original. Notable / small-node
  replacement (the per-seed LUT half) is tracked separately under
  #227.
- Cluster jewel sub-graph synthesis (#21 → #190): full sub-graph
  materialisation for socketed Cluster Jewels — every notable / small
  / inner-socket node spawns with collision-free synthetic node ids,
  and `compute_full_with_clusters` injects mods from any allocated
  synthesised node connected to an allocated parent socket.
- Generic radius-jewel framework (#31 → #191): the framework slice
  ships, with `HandlerKind` covering vanilla `SelfAllocated`, the
  named-unique handlers, and the Pathfinder dispatch arm.
- Named-unique radius-jewel handlers (#196 → #226 / #228 / #233 /
  #234): Watcher's Eye (aura-conditional global buff), Healthy Mind
  (`Inc Life` → `Inc Mana × 2` transform), Fertile Mind (Dex → Int
  attribute swap), Conqueror's Efficiency-style non-radius jewels
  (item mods apply globally instead of being silently dropped),
  Pure Talent (per-class bonuses gated on connected starts), and
  Intuitive Leap (path-finder bypass — in-radius nodes allocate
  without a connecting chain, with iterative orphan-protection
  on un-allocate).
- Live character API import (#32 → #188): paste a POESESSID +
  account name, fetch the character list, pick a character; class +
  ascendancy + level + allocated tree + equipped items land
  end-to-end. Char API follow-ups (skills + masteries + cluster
  jewel nodes + POESESSID persistence + wasm fetch) are tracked
  under #194.
- Alt-quality variants (#36 → #192): Anomalous / Divergent /
  Phantasmal variants pick the right `qualityStats` at compute time
  and round-trip the `<Gem qualityId>` attribute through PoB XML.
- Calcs-tab `CalcBreakdown.lua` port (#34 → #189): Damage / Speed /
  Crit headline keys grow click-through breakdowns that walk
  Base → INC → MORE → quality → crit factor → speed → DPS in a
  step-by-step panel. Rows without a custom breakdown still render
  the legacy contributing-mods view.
- Minion build pass slices 3-16 (#20 → #172 / #175 / #176 / #177 /
  #179 / #180 / #181 / #182 / #193 / #201 / #218 / #232): real
  minion-side perform pass with `MinionState`, intrinsic mod
  parsing, life / ES / armour / evasion / resists / damage / DPS,
  hit-chance vs enemy evasion, life regen, alt-life-table support,
  spectre lifeScaling. `MainSkillDPS` mirrors `MinionDPS ×
  NumberOfMinions` for summoner builds. Slices 15 + 16 (movement
  speed, total HP pool, crit factor) are in flight.
- External-site URL fetch + import (#33 → #202): pasting a
  `https://pobb.in/<id>` or `https://pastebin.com/<id>` URL into the
  Import-Export tab spawns a background `ureq` GET, decodes the
  body, and swaps the active character. Errors surface as readable
  banner messages.

Still open (in rough priority):

1. **AoE radius rolloff and projectile pierce/chain variance**: we now model
   shotgun overlap and the per-target multiplier, but not AoE damage falloff
   or pierce/chain damage variance per hop.
2. **Live `pob_diff` ailment baselines in CI**: reference builds exist
   (`marauder_l90_bleeding_cleave.xml`, `witch_l90_arc_with_items.xml`,
   etc.), but locking PoB-vs-engine deltas behind a regression test still
   requires running pob_diff in the test environment.
3. **Timeless jewel notable / small-node replacement (#227)**: keystone
   swaps shipped in slice 1, but the bulk of what these jewels do —
   per-seed notable rolls — needs the compressed-binary LUT extraction
   from `Data/TimelessJewelData/*.zip` + a compute-time lookup keyed by
   `(jewel_id, seed, original_node_id)`. Multiple-choice notables (Vaal
   "Might / Legacy of the Vaal") are a sub-deferral.
4. **Per-mod power scoring (#207)**: engine-side single-node + batch
   ranking primitives are wired (`pob_engine::power::score_node_addition`,
   `score_node_removal`, `rank_node_additions`). Tree-overlay heatmap
   rendering, items-tab top-modlines list, and compare-tab per-source
   delta are the UI consumers.
5. **Char API import follow-ups (#194)**: live import lands class +
   ascendancy + level + tree + items, but skills, masteries,
   cluster-jewel sub-graph nodes, POESESSID persistence (OS keyring),
   and the wasm fetch path remain.
6. **Build power overlay UI on the tree tab (#220)**: per-node DPS / EHP
   shading driven by #207's scoring primitives, plus the version
   converter / spec compare overlay / spec dropdown rich tooltips.

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
