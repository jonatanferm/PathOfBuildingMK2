# Path of Building Community — Architecture Map

This document maps the current Lua/SimpleGraphic codebase at `../PathOfBuilding/` and is the
reference for porting it to Rust in this repo. It describes the **what** and **where**, not the
**how to port** — porting decisions go in `decisions/` ADRs.

Throughout, file paths are relative to `../PathOfBuilding/src/` unless noted.

## 1. Top-level layout

```
src/
  Launch.lua            # SimpleGraphic-driven entry point + frame loop
  Modules/
    Main.lua            # Mode dispatch, popups, toast notifications
    Build.lua           # The user's character — orchestrates tabs + calc engine
    Calcs.lua           # Calc engine entry: buildOutput, calcFullDPS
    CalcSetup.lua       # Phase 1: build env from Build → modDB/items/passives
    CalcPerform.lua     # Phase 2: apply modifiers, compute stats
    CalcOffence.lua     # Damage/DPS sub-pass (~6k lines)
    CalcDefence.lua     # Survivability sub-pass (~3.8k lines)
    CalcActiveSkill.lua # Active skill construction + skill mod merging
    CalcBreakdown.lua   # Re-derives human-readable explanations of stats
    CalcSections.lua    # Declarative layout for the Calcs tab
    CalcTools.lua       # Small math helpers (calcLib.mod, calcLib.val, ...)
    ModParser.lua       # English mod text → structured mods (~6.8k lines)
    Data.lua            # Loads everything from src/Data/ at startup
  Classes/
    PassiveTree.lua     # Static tree data (nodes, groups, sprites)
    PassiveSpec.lua     # User's tree allocation (mutable)
    PassiveTreeView.lua # Render + interaction (couples to SimpleGraphic)
    Item.lua            # Item parser + data model
    ModDB.lua           # Hash-indexed modifier storage
    ModList.lua         # Linear modifier list
    ModStore.lua        # Abstract base + EvalMod (tag/condition resolution)
    *Tab.lua            # One per UI tab (Tree, Items, Skills, Config, Calcs, ...)
    *Control.lua        # Custom widgets (Edit, Button, List, ...)
  Data/                 # Game data as Lua tables (gems, mods, bases, ...)
  TreeData/3_xx/        # Per-version passive tree data
  Export/               # Tool: GGPK extraction → src/Data/ generators
  Assets/               # Images
  Modules/Build*Tools.lua, ItemTools.lua, ModTools.lua, ...
```

## 2. Calc engine — public surface

**Entry point:** `calcs.buildOutput(build, mode)` at `Modules/Calcs.lua:417`.
- Inputs: `Build` object, `mode ∈ {"MAIN", "CALCS", "CALCULATOR"}`.
- Output: `env` whose `env.player.output` is a flat dict of ~200 stats.
- Caller: `Classes/CalcsTab.lua:420,427,438,440` invokes when `build.buildFlag = true`.

**Aggregate DPS:** `calcs.calcFullDPS(build, mode, override, specEnv)` at `Modules/Calcs.lua:176`
loops every active skill, runs perform per-skill, sums into `output.FullDPS`, `FullDotDPS`,
`SkillDPS[i]`. There is a `GlobalCache` between them keyed on (skill, override) — this is a
mutable Lua singleton; in Rust we'll probably want a hashmap on `Build`.

## 3. Pipeline

```
                     Build
                       │
                       ▼
          calcs.buildOutput(build, mode)        Calcs.lua:417
                       │
              ┌────────┴────────┐
              ▼                 ▼
   calcs.initEnv(...)   ─►  env (see §4)        CalcSetup.lua:358
              │
              ▼
   calcs.perform(env, skipEHP)                  CalcPerform.lua:1098
        ├── doActorLifeMana, doActorAttribsConditions, …
        ├── for each active skill: createActiveSkill  CalcActiveSkill.lua:82
        ├── calcs.calcOffence(env)              CalcOffence.lua
        └── calcs.calcDefence(env)              CalcDefence.lua
              │
              ▼
   env.player.output  (flat dict)
```

`initEnv` builds the env from scratch on the first call, then can be invoked with
`specEnv` for cheap incremental updates (calculator pane).

`perform` is one giant procedural function that does ~60 successive passes — each pass
reads from the modDB and writes one or more output keys. Order matters: e.g. attribute
conditions before stat-derived multipliers before damage application.

## 4. The `env` object

Mutable state passed top-to-bottom through the pipeline. Shape (paraphrased):

```lua
env = {
  build = Build,            -- reference back
  data = build.data,         -- game constants from src/Data/
  mode = "MAIN" | "CALCS" | "CALCULATOR",

  modDB    = ModDB,          -- player mods
  enemyDB  = ModDB,          -- enemy mods
  itemModDB = ModDB,         -- transient item mods (flask states etc.)

  player = {
    modDB,
    level = build.characterLevel,
    itemList    = { slot -> Item },     -- equipped items
    activeSkillList = { ActiveSkill },  -- everything socketed
    mainSkill   = ActiveSkill,           -- chosen skill (set during perform)
    output      = { stat -> number },    -- the final dict
    weaponData1, weaponData2,            -- cached
  },

  minion = {                 -- only when mainSkill grants a minion
    modDB, level, activeSkillList, mainSkill, output
  },

  enemy = { modDB, level, output },

  -- caches
  allocNodes      = { id -> Node },     -- allocated tree nodes
  radiusJewelList,
  flasks          = { Item -> true },
  auxSkillList,                          -- skills providing global buffs

  -- inputs
  configInput = build.configTab.input,
  calcsInput  = build.calcsTab.input,
  override,

  mode_buffs, mode_combat, mode_effective,  -- mode flags
}
```

## 5. Modifier system

The modifier system is the whole game. Everything else is glue.

### 5.1 Mod shape

A single mod (see `Modules/ModTools.lua:20-46` for the canonical helpers):

```lua
{
  name = "FireDamage",     -- stat the mod targets (string key)
  type = "INC"|"MORE"|"BASE"|"OVERRIDE"|"FLAG"|"LIST"|"MAX"|"MIN",
  value = 20,              -- number, or table for complex/list mods
  flags = ModFlag.Attack | ModFlag.Melee,    -- bitset (skill applicability)
  keywordFlags = KeywordFlag.Fire,           -- bitset (damage/effect type)
  source = "Item 1" | "Tree" | "Skill Fireball" | "Buff X" | "Config Y",
  -- variadic positional tags follow:
  [1] = { type = "Condition", var = "FullLife" },
  [2] = { type = "Multiplier", var = "PowerCharge", limit = 5 },
  ...
}
```

Tag types (in `Classes/ModStore.lua:312-903` — `EvalMod`): `Condition`, `Multiplier`, `PerStat`,
`PercentStat`, `StatThreshold`, `MultiplierThreshold`, `ActorCondition`, `SkillType`, `SkillName`,
`SkillId`, `SkillPart`, `SocketedIn`, `ItemCondition`, `SlotName`, `DistanceRamp`, `Limit`,
`GlobalLimit`, plus a long tail.

### 5.2 ModParser

`Modules/ModParser.lua` (6784 lines) parses mod *text* (e.g. "10% increased Fire Damage to
Spells while wielding a Staff") into a list of mod tables. Entry: `parseMod(line, order)` near
line 6407. Pipeline:

1. Special-case scans (jewel functions, cluster jewel skills, unsupported mods).
2. Pre-flag scan (line-leading qualifiers: weapon types, "minions", "enemies", …).
3. Skill name prefix.
4. Form scan via `formList` (`^(%d+)%% increased` → INC, etc.).
5. Tag scan (per-charge, on-crit, while-X, …).
6. Name scan (lookup in `modNameList`).
7. Trailing flags ("to attacks", "to spells", …).
8. Generate mod with assembled `flags`/`keywordFlags`/tags.

Output cached in `Data/ModCache.lua` to avoid re-parsing. The cache is just a frozen
`{ english_text → mod_list }` map.

### 5.3 ModList vs ModDB vs ModStore

- `ModStore` (`Classes/ModStore.lua`) — abstract base. Owns `multipliers`, `conditions`,
  `actor`, `parent`. Provides the query API (`Sum`, `More`, `Flag`, `Override`, `List`,
  `Tabulate`, `HasMod`).
- `ModList` — linear `[1..N]` array. Cheap to build, full scan to query. Used per-item.
- `ModDB` — hash `mods[name] -> [mods]`. Slower to mutate, fast to query. Used as the env-
  level `modDB`/`enemyDB`/`itemModDB`.

A query walks the parent chain, so a player ModDB can transparently see ascendancy / item /
buff mods layered on top of base.

### 5.4 Query API

All in `Classes/ModStore.lua:142-241`:

- `Sum(modType, cfg, ...)` — adds matching `BASE` or `INC` values.
- `More(cfg, ...)` — multiplicative product of matching `MORE` mods.
- `Flag(cfg, ...)` — true iff any matching `FLAG` mod resolves truthy.
- `Override(cfg, ...)` — first matching `OVERRIDE` value.
- `List(cfg, ...)` — list of values from `LIST` mods.
- `Tabulate(...)` — `{ value, mod }` pairs (for breakdown).

The match condition (`ModDB.lua:132-155`):

```
mod.type == modType
  and band(flags, mod.flags) == mod.flags
  and matchKeywordFlags(keywordFlags, mod.keywordFlags)
  and (not source or mod.source:match("[^:]+") == source)
  and EvalMod(mod, cfg) ~= nil
```

`EvalMod` resolves tags against `cfg` (skill, conditions, multipliers, actor state) and either
returns the (possibly scaled) value or `nil`.

### 5.5 Mod sources

The `source` field carries provenance, used both for filtering and breakdown attribution.
Conventions: `"Item 1"`, `"Tree"`, `"Passive <id>"`, `"Ascendancy <name>"`,
`"Skill <gemName>"`, `"Buff <buffName>"`, `"Config <name>"`, `"<MinionType> Passive"`.

## 6. Output stats

`env.player.output` is one flat string-keyed dict, populated incrementally by `perform`.
Categories (not exhaustive):

- Pools: `Life`, `Mana`, `EnergyShield`, `Ward`, `Rage`.
- Defences: `Armour`, `Evasion`, `BlockChance`, `SpellBlockChance`, `DodgeChance`,
  `SpellDodgeChance`, `<Element>Resist`, `<Element>ResistMax`, `ChaosResist`,
  `DamageReductionMax`.
- Offence: per-skill — `PhysicalDPS`, `FireDPS`, `ColdDPS`, `LightningDPS`, `ChaosDPS`,
  `TotalDPS`, `HitChance`, `CritChance`, `CritMultiplier`, `Accuracy`.
- Ailments: `IgniteDPS`, `BleedDPS`, `PoisonDPS`, ailment chances, ailment magnitudes.
- Speeds: `ActionSpeed`, `AttackSpeed`, `CastSpeed`, `MovementSpeed`.
- Charges: `PowerCharges`, `FrenzyCharges`, `EnduranceCharges`, plus `*Max` variants.
- Aggregates (set by `calcFullDPS`): `FullDPS`, `FullDotDPS`, `SkillDPS[]`.

## 7. Active skills

`ActiveSkill` (per `Modules/CalcActiveSkill.lua`):

```lua
{
  activeEffect    = GemEffect,
  supportList     = [GemEffect],
  skillModList    = ModList,        -- merged: gem + supports
  baseSkillModList = ModList,
  skillData       = { stat -> value }, -- numeric (aoe, cd, …)
  skillFlags      = { attack=true, … },
  actor           = env.player or env.minion,
  minion          = nil | { type, level, mainSkill, … },
}
```

The "main skill" is set on `env.player.mainSkill` first thing in `perform`. Minion-summoning
skills create a parallel minion env that gets its own perform pass.

## 8. Passive tree

### 8.1 Static data

Per-version directory: `TreeData/3_23/`, `TreeData/3_22/`, …

```lua
-- tree.lua (sketch)
{
  classes = { … },
  groups = {
    [groupId] = { x, y, oo, nodes = [nodeId, …] },
  },
  nodes = {
    [nodeId] = {
      dn = "Display Name",
      sd = ["+10 to Strength", …],
      g  = groupId,
      o  = orbitIndex,
      oidx = orbitPosition,
      isNotable, isKeystone, isMastery, isJewelSocket,
      ascendancyName, classStartIndex,
      out = [linkedNodeIds], in = [linkedNodeIds],
      masteryEffects = [{ effect = id, stats = […] }],
    },
  },
  skillsPerOrbit = [1, 6, 16, 16, 40, 72, 72],
  orbitRadii     = [0, 82, 162, 335, 493, 662, 845],
}
```

`Classes/PassiveTree.lua:462-548` is where node types are computed at load. Edges are
materialized into `node.linkedId[]` at `:567-592`. Cluster jewels generate sub-graphs at
runtime — `PassiveSpec.lua` keeps `subGraphs[clusterGraphId]` of synthesized nodes.

### 8.2 User allocation

`Classes/PassiveSpec.lua:30-90` — `self.allocNodes[nodeId] = node` for each picked node, plus
`self.masterySelections[nodeId] = effectId`, `self.jewels[socketNodeId] = itemId`,
`self.subGraphs`, `self.tattooOverrides`.

XML format (current): `<Spec ...><nodes>id1 id2 …</nodes><Sockets/><Overrides/>…</Spec>`.

## 9. Items

Parsed from raw PoE copy-paste text (or imported XML) — see `Classes/Item.lua:298-450`.
Shape:

```lua
{
  rarity = "NORMAL"|"MAGIC"|"RARE"|"UNIQUE"|"RELIC",
  name, baseName,
  baseType,                     -- looked up in Data/Bases/<slot>.lua
  implicitModLines  = [ModLine],
  explicitModLines  = [ModLine],
  enchantModLines   = [ModLine],
  craftedModLines   = [ModLine],
  scourgeModLines, synthesisModLines, crucibleModLines = …,
  sockets   = [{ color, group }],
  qualityPercent,
  shaper, elder, fractured, … (influence flags),
  modList   = ModList,          -- materialized from all *ModLines
}
```

Items flow into the calc env at `CalcSetup.lua:948-975`: for each equipped slot,
`modDB:AddList(item.modList)` with `source = "Item <slotIndex>"`.

## 10. Skill gems

`Data/Gems.lua` — auto-generated from PoE files:

```lua
["Metadata/Items/Gems/SkillGemFireball"] = {
  name = "Fireball",
  gameId = "Metadata/Items/Gems/SkillGemFireball",
  grantedEffectId = "Fireball",
  tags = { intelligence = true, projectile = true, grants_active_skill = true, … },
  reqStr, reqDex, reqInt,
  naturalMaxLevel = 20,
  -- vaal variants set secondaryGrantedEffectId
}
```

Granted effect data (level scaling, base damage, modifier sets) lives in `Data/Skills/`.
Support gems share the same shape but lack `grants_active_skill`.

## 11. Data extraction (`src/Export/`)

The PoB project ships its own extractor that reads PoE's GGPK / OOZ bundles directly. The
key files:

- `Export/Main.lua:67-82` — driver that runs the per-resource scripts.
- `Export/Classes/GGPKData.lua:34-110` — wraps the `bun_extract_file.exe` helper to pull
  `.datc64` files from the live game install.
- `Export/Scripts/skills.lua`, `bases.lua`, `mods.lua`, `statdesc.lua`, … — per-resource
  scripts that read DAT rows and emit Lua tables into `src/Data/`.

For our port: we **do not** re-implement GGPK extraction. We piggyback on PoB's already-
extracted `src/Data/*.lua` and `src/TreeData/*/tree.lua`, converting them once at build
time into Rust-friendly formats.

## 12. Data loader

`Modules/Data.lua` is the runtime entry. It calls `LoadModule()` (a SimpleGraphic-provided
`dofile` clone) on each `Data/*.lua`, populating a global `data` table:
`data.itemMods`, `data.skills`, `data.bases`, `data.gems`, `data.uniques`, ….

`Data.lua:68-92` (`processMod`) post-processes parsed mods: attaches source, lifts
conditional logic, normalises ranges. We need to replicate this normalisation in Rust.

## 13. UI structure

### 13.1 Frame loop

`Launch.lua:108-140` — `OnFrame`. SimpleGraphic calls into Lua every frame. Routes input
events into `self.inputEvents`, then dispatches to `main:OnFrame` (`Main.lua:341-455`).

The calc engine is **demand-driven, not per-frame.** `Build.lua:1182-1191` checks
`self.buildFlag`, and only when set: clears the global cache, bumps `outputRevision`, calls
`calcsTab:BuildOutput()`. Every input change setter sets `buildFlag = true`.

### 13.2 Build object

A `Build` is a `ControlHost` (`Modules/Build.lua:597-605`) owning:

```
self.treeTab     Classes/TreeTab.lua
self.skillsTab   Classes/SkillsTab.lua:78
self.itemsTab    Classes/ItemsTab.lua:65
self.configTab   Classes/ConfigTab.lua:15
self.calcsTab    Classes/CalcsTab.lua:19
self.importTab   Classes/ImportTab.lua:22
self.notesTab    Classes/NotesTab.lua:8
self.partyTab, self.compareTab
```

`viewMode` (`Build.lua:1216-1239`) selects which tab is visible; Ctrl+1..7 shortcuts.

Each tab implements `Save(xml)` / `Load(xml)` registered in `self.savers`. Trees load
deferred to resolve jewel sockets after items.

### 13.3 Tabs

| Tab | Purpose |
|---|---|
| Tree | Allocate passive nodes, choose class/ascendancy. |
| Skills | Configure gem sockets, links, support gems, socket groups. |
| Items | Equipped items, item sets, flask configuration. |
| Config | Encounter conditions, enemy state, manual buffs. |
| Calcs | Stat dashboard with click-to-breakdown. |
| Import | POB code / character API import; export. |
| Notes | Free-form text with PoB color codes. |
| Party / Compare | Group play / build comparison. |

### 13.4 Calcs tab breakdown

Layout is **declarative** in `Modules/CalcSections.lua` — an array of sections, each
`{ width, id, group, color, [{ defaultCollapsed, label, data = [stat_specs] }] }`. Stat
specs reference output keys via `{0:output:StatName}` format strings. `breakdown` field on
each spec names a function in `CalcBreakdown.lua` that re-derives the calculation from the
current modDB+output for the popup.

Breakdowns are **forensic**: they don't drive the calculation. Output is computed first; a
breakdown is then built from the existing output and modDB by re-running a smaller, more
verbose version of the relevant calc.

### 13.5 Persistence + import

- Builds saved as XML in `~/Path of Building/Builds/<category>/<name>.xml`.
- Auto-save on every change (`Build:SaveDBFile`, `Build.lua:1965+`).
- POB sharing code = `base64(deflate(xml))`. Decode in `Modules/Main.lua:74`.
- External-site import (pathofexile.com, pobb.in, etc.) handled by `BuildSiteTools.lua`.

## 14. Things that don't port cleanly

### Engine layer

- `LoadModule(path, calcs)` (`Calcs.lua:15-21`) hot-patches `calcs.*` functions onto a
  shared table. Replace with normal Rust modules.
- Global `GlobalCache` singleton for skill DPS — make it a field on `Build` or `Env`.
- Stats keyed by **strings** (`"Life"`, `"FireDamage"`, …). For Rust, intern these or use
  enums (with a small "extension" string variant for rare cases).
- Mods carry a **variadic positional tag list**. We need a `Tag` enum.
- ModDB parent chains via Lua table identity → use `Arc<ModDB>` (or arena IDs) in Rust.
- `ModStore.EvalMod` is a giant `if/elseif` over tag types. Port as a single match.
- `ipairs/pairs`, table copying, `wipeTable`, `copyTable` — ownership decisions per call
  site.
- `mod.value` is sometimes a number, sometimes a table (for damage ranges, etc.). We need
  a `ModValue` enum.

### UI layer

- SimpleGraphic event loop, `DrawString`, `DrawImage`, custom color codes (`^7`, `^xRRGGBB`)
  — all replaced by egui. Text rendering needs a small adapter that turns PoB color codes
  into egui `LayoutJob` / `RichText`.
- Custom `Control`/`ControlHost` widget tree — replaced wholesale by egui idioms.
- Tooltip / popup / drag-drop / screenshot — different APIs; rebuild.
- Property-function getters (`control.width = function() return ... end`) — egui is
  immediate-mode, so layout is recomputed every frame anyway.

### What's actually pure (and ports cleanly)

- The mod parser (text → structured mods) — pure pure, just unicode + regex.
- `ModStore.EvalMod` — pure given its inputs.
- `CalcOffence`, `CalcDefence`, `CalcPerform` — pure (read modDB / write output).
- Tree algorithms (allocation, pathfinding) — pure.
- Item parsing — pure (input text → Item struct).
- Build XML / POB code import — pure.

## 15. Reading order for porters

When you need to dive deeper into a subsystem, read in this order:

- **Mod system:** `ModTools.lua` → `ModStore.lua` → `ModList.lua` → `ModDB.lua` → `ModParser.lua`.
- **Calc pipeline:** `Calcs.lua` → `CalcSetup.lua` → `CalcPerform.lua` → `CalcActiveSkill.lua` → `CalcOffence.lua` → `CalcDefence.lua` → `CalcBreakdown.lua`.
- **Tree:** `PassiveTree.lua` (data) → `PassiveSpec.lua` (allocation) → `PassiveTreeView.lua` (render — for reference only; we replace this).
- **Items:** `Item.lua` → `ItemTools.lua` → `ItemsTab.lua`.
- **UI shell:** `Launch.lua` → `Main.lua` → `Build.lua` → `*Tab.lua`.
