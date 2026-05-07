# Project status

## Headline numbers

- 5 crates, 40+ commits, ~13 000 lines of Rust.
- 79 tests pass workspace-wide.
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
  sibling `../PathOfBuilding/` checkout into `data/`.
- **`cargo test --workspace`** — 61 tests pass (engine: 45 unit + 1 parser-coverage
  + 4 validation + 2 pathfind + 1 layout, data: 4 unit + 5 round-trip).
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
  damageEffectiveness, hit damage × (1 + total inc/100) × total more, crit factor,
  enemy resist × penetration, hit chance vs evasion (attacks only), final DPS.
- Ailments: rough seeded-from-hit BleedDPS / PoisonDPS / IgniteDPS.

Modifier system handles `Sum` / `More` / `Flag` / `Override` / `List` queries with
`Condition` / `ActorCondition` / `Multiplier` / `PerStat` / `PercentStat` /
`StatThreshold` / `MultiplierThreshold` tag resolution.

ModParser covers **~65% of the 3.25 passive tree's stat strings** (up from 25% in the
phase-2 minimum). The remaining 35% are mostly conditional / suffix-clause forms and
niche multi-stat lines (`+1 to maximum number of Summoned Golems`, `Hits have N% chance
to ignore Enemy Physical Damage Reduction`, etc.) documented in `docs/divergences.md`.

## What's documented

- [`docs/architecture-current.md`](docs/architecture-current.md) — map of the upstream
  Lua codebase being ported.
- [`docs/divergences.md`](docs/divergences.md) — running list of deliberate shortcuts.
- [`docs/packaging.md`](docs/packaging.md) — macOS / Windows / Linux build + bundle
  notes.
- [`docs/decisions/`](docs/decisions/) — ADRs for workspace layout (0001) and skill
  data extraction (0002, since closed).

## What's next

In rough priority order:

1. **Slot-conditional item mods**: items currently apply unconditionally. PoB filters
   "while using a shield" mods to slots that match the body. Fix: emit `SlotName` /
   `SocketedIn` tags from the item-paste parser; the engine already supports them in
   `eval_mod`.
2. **POB-format export**: we read PoB XML but write only MK2. Adding XML write means
   round-tripping back to PoB, which a lot of users will want.
3. **More accurate ailment DPS**: poison stacking, ignite chance, ailment scaling
   damage, faster ailment damage, ailment magnitude. PoB's CalcOffence has thousands of
   lines for this; we have a rough single-stack model today.
4. **More parser coverage** to ~70%: the remaining unparsed tree lines are mostly
   conditional / weapon-class / chance-to forms. Each expansion is small and well-
   isolated.
5. **Validation harness driven by live PoB**: hardcoded reference values are useful as
   regression detectors, but the right shape is to run PoB headless under Lua, drive a
   build through both engines, and diff `env.player.output`. Most of the substrate is
   already there (mlua + sandbox in `pob-extract`).
6. **Wgpu custom paint for the tree** if profiling reveals it. Currently 2.2ms per
   compute pass on the full tree (in release), well under a 60Hz frame budget.

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
