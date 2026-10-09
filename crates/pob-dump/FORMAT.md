# pob-dump output format (format 1)

`pob-dump` runs Path of Building's own data load under LuaJIT
(`GameVersions.lua`, `Modules/Common`, `Modules/CalcFormat`, `Modules/Data`,
the order `Modules/Main.lua` uses) and writes the resulting Lua tables as
JSON. Nothing is hand-mapped: every field PoB has is in the dump, including
everything `Data.lua` derives at load time (Vaal AltX/AltY gem synthesis,
`processMod` sources, `gemForBaseName`, `itemBaseLists`, `bossStats`,
`Uniques/Special/Generated.lua` output, ...).

```
cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump
cargo run -p pob-dump -- --pob .PathOfBuilding/src --codegen-enums crates/pob-dump/generated/enums.rs
```

## Files

| file | Lua root path | source |
|---|---|---|
| `<key>.json` | `data.<key>` | every table-valued key of PoB's `data` (skills, gems, itemBases, uniques, itemMods, masterMods, mapMods (= `Data/ModMap.lua`), bosses, bossSkills, pantheons, skillStatMap, minions, ...) |
| `misc.json` | `data.<key>` for each member | keys that come from `Data/Misc.lua` plus every non-table value of `data` (functions, strings, numbers); an object `{ "<key>": <value> }` |
| `global.json` | `global` | globals defined by `Data/Global.lua` (`SkillType`, `ModFlag`, `KeywordFlag`, `colorCodes`, helper functions, ...) |
| `gameVersions.json` | `GameVersions` | globals defined by `GameVersions.lua` (`treeVersionList`, `treeVersions`, ...) |
| `calcSections.json` | `CalcSections` | `Modules/CalcSections.lua` (raw) |
| `configOptions.json` | `ConfigOptions` | `Modules/ConfigOptions.lua` (`apply`/`tooltipFunc` closures as `$fn`) |
| `modCache.json` | `ModCache` | `Data/ModCache.lua`, see below |
| `statDescriptions/<name>.json` | `StatDescriptions.<name>` | each `Data/StatDescriptions/<name>.lua`, as the file returns it |
| `files/<path>.json` | `files["Data/<path>.lua"]` | `Data/**/*.lua` files PoB's data load does not reach (TradeSiteStats, QueryMods, CurrencyNames, TattooPassives, TimelessJewelData/LegionPassives, ...), plus `PearlSupports` (loaded only into locals) |
| `trees/<ver>.json` | `trees["<ver>"]` | `TreeData/<ver>/tree.lua`, every version incl. 2_6, 3_6-3_9, ruthless, alternate |
| `trees/<ver>.sprites.json` | `treeSprites["<ver>"]` | `TreeData/<ver>/sprites.lua` where present |
| `trees/3_19.assets.json` | `treeAssets["3_19"]` | `TreeData/3_19/Assets.lua` |
| `trees/legion.json` | `trees["legion"]` | `TreeData/legion/tree-legion.lua` |
| `closures.json` | | manifest of every function (see below) |
| `manifest.json` | | format version, PoB version, every file with its roots, size and sha256, counts, timings |

## Value encoding

* `nil` never appears (Lua tables cannot hold it), except `null` in `modCache.json`.
* booleans and valid-UTF-8 strings are JSON booleans and strings.
* A string that is not valid UTF-8 is `{"$bytes": "<lowercase hex>"}`.
* Numbers: LuaJIT has only doubles. An integral value in (-2^53, 2^53) is
  written as a JSON integer (`10`). Anything else uses the shortest of
  `%.15g`, `%.16g`, `%.17g` that parses back to the identical double
  (`0.80000001192093`, `1e+300`), so every number round-trips exactly.
  Non-finite values and negative zero are `{"$float": "inf" | "-inf" | "nan" | "-0"}`.

### Tables

A Lua table is encoded as a **JSON array** exactly when its keys are
`1..n` with `n >= 1` and it has no metatable. Every other table — including
the empty table — is a **JSON object** with these members:

* each key that is a valid-UTF-8 string: `"key": value`. A key that begins
  with `$` is written with one extra `$` (`"$x"` is written `"$$x"`), so
  members starting with a single `$` are always reserved;
* `"$seq": [ ... ]` — the values at keys `1..n`, where `n` is the largest
  such that keys 1..n are all present (e.g. mod tags inside a mod, the
  positional stat values of a skill level);
* `"$num": { "<number>": value, ... }` — all other numeric keys, keyed by
  their canonical number text (as above, `inf`/`-inf`/`-0` spelled out),
  ascending numerically (e.g. `skillTypes = { [SkillType.Spell] = true }`
  becomes `{"$num": {"2": true}}`; a holey array keeps 1..n in `$seq` and the
  rest here);
* `"$keys": [[key, value], ...]` — every other key (booleans, non-UTF-8
  strings as `{"$bytes"}`, tables and functions as their `$ref`/`$fn`),
  sorted by the key's path text;
* `"$meta": value` — the table's metatable (raw, past any `__metatable`),
  encoded like any other value. Semantic examples: each skill's `statMap`
  has `"$meta": {"$ref": "data.skillStatMapMeta"}`, whose `__index` (a
  `$fn`) falls back lazily to `data.skillStatMap` and runs `processMod`;
  `data.costs` has an `__index` alias closure; dkjson objects carry
  `{"__jsontype": "object"}`.

Members are ordered: string keys (bytewise), then `$seq`, `$num`, `$keys`,
`$meta`. Decoding is unambiguous: a JSON array is a sequence, an object is a
table rebuilt from its members, and the only other objects are the
single-purpose `$ref`, `$fn`, `$float`, `$bytes` values (which can never be
confused with a table because a table's `$`-keys are escaped).

### Shared tables, cycles and paths

Every table and function gets one **canonical path**: the path at which a
breadth-first walk from the root first reaches it, visiting keys in the
order above (so the shortest path wins, ties broken by key order). Entries
under `$keys` keys are walked only after the ordinary walk, so a table used
as a key gets its path from where it is stored as a value.

The table is written in full only at its canonical path; everywhere else it
is `{"$ref": "<canonical path>"}`. This preserves sharing and breaks cycles:
`data.gems[...].grantedEffect` is `{"$ref": "data.skills.Fireball"}`,
`statMap._grantedEffect` points back at its skill, `data.gemForSkill` is a
`$keys` list of `[{"$ref": skill}, gemId]`. Refs may cross files (the root
path names the file, see the table above) and roots (ConfigOptions refers to
`data.mapMods.Prefix`).

Path syntax is Lua-like: `root`, then `.name` for identifier string keys,
`["..."]` (JSON string escaping) for other string keys, `[n]` for numeric
keys (canonical number text), `[true]`/`[false]`, `[@<path>]` for table or
function keys, `[$bytes:<hex>]` for non-UTF-8 string keys, and `<meta>` for a
metatable. Examples: `data.skills.Fireball.levels[3]`,
`data.gems["Metadata/Items/Gems/SkillGemFireball"]`, `data.costs<meta>.__index`.

### Functions

Every function value is replaced by

```json
{"$fn": "<canonical path>", "src": "<file>:<linedefined>-<lastlinedefined>", "hash": "<sha256>"}
```

`file` is relative to PoB's `src/`; `hash` is the sha256 of the source lines
`linedefined..=lastlinedefined` exactly as on disk (CR stripped, joined with
`\n`; if several functions share a line, they share that text). A function
reachable from several places has one id (its canonical path) and the same
object everywhere. C functions have `"src": "[C]"`.

`closures.json` lists each function once: `id`, `family`, `src`, `hash`,
`what` (`Lua`/`C`/`main`) and `aliases` (every other path it was found at),
plus counts per family. Families: `skill-hook:<hook>` (`data.skills.*`
preDamageFunc, initialFunc, postCritFunc, preSkillTypeFunc,
explosiveArrowFunc), `minion-hook:<field>`, `modmap:apply`,
`config:<field>` (apply, tooltipFunc, tooltip), `calc-section`,
`power-stat`, `data-metatable`, `data-helper`, `global-helper`,
`stat-descriptions`, `tree`, `extra-file`.

## ModCache

`modCache.json` is `{"<line>": {"mods": <mod list> | null, "extra": <string> | null}}`
for each `c["<line>"] = { modList, extraText }` in `Data/ModCache.lua`
(PoB's memoized ModParser output; `nil` becomes `null`). Mod lists use the
table encoding above (a mod's tags are its `$seq`).

## Determinism

The dump is byte-for-byte reproducible. LuaJIT randomizes its string hash
seed per state, so `pairs()` order varies between runs, and a few results of
PoB's own load depend on it: `ExplosiveTrap` and `ExplosiveTrapAltX` share one
`statMap` table, and Data.lua's per-skill pass (last writer wins) sets its
`_grantedEffect` and the mods' `source`; `clusterJewelInfoForNotable[*].jewelTypes`
is filled in `pairs()` order. The dumper replaces the global `pairs` before
PoB loads with one that iterates in sorted key order (booleans, numbers,
strings), which is one of the orders PoB itself could produce.
