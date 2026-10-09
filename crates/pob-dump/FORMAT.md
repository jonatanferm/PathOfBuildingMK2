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
cargo run -p pob-dump --release -- --pob .PathOfBuilding/src --out data/dump --parse-oracle-only
cargo run -p pob-dump -- --pob .PathOfBuilding/src --codegen            # writes crates/pob-dump/generated/*
cargo run -p pob-dump -- --pob .PathOfBuilding/src --codegen --check    # fails if a committed file differs
```

## Files

| file | Lua root path | source |
|---|---|---|
| `<key>.json` | `data.<key>` | every table-valued key of PoB's `data` (skills, gems, itemBases, uniques, itemMods, masterMods, mapMods (= `Data/ModMap.lua`), bosses, bossSkills, pantheons, skillStatMap, minions, ...) |
| `misc.json` | `data.<key>` for each member | keys that come from `Data/Misc.lua`, every non-table value of `data` (functions, strings, numbers), and `data.misc` (its own file name would be `misc.json`); an object `{ "<key>": <value> }` |
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
| `parse_oracle.jsonl` | | PoB's `modLib.parseMod` over every mod line it parses, see below |
| `manifest.json` | | format version, PoB version and commit, every file with its roots, size and sha256, counts, the parse oracle summary (no timings: two dumps are byte-identical) |

## Value encoding

* `nil` never appears (Lua tables cannot hold it), except `null` in `modCache.json`.
* booleans and valid-UTF-8 strings are JSON booleans and strings.
* A string that is not valid UTF-8 is `{"$bytes": "<lowercase hex>"}`.
* Numbers: LuaJIT has only doubles. An integral value in (-2^53, 2^53) is
  written as a JSON integer (`10`). Anything else uses the shortest of
  `%.15g`, `%.16g`, `%.17g` that parses back to the identical double
  (`0.80000001192093`, `1e+300`), so every number round-trips exactly.
  Non-finite values and negative zero are `{"$float": "inf" | "-inf" | "nan" | "-0"}`.
  Readers must parse decimals with correct rounding: with `serde_json`, enable
  its `float_roundtrip` feature (the default parser is up to 1 ulp off, e.g.
  `data.misc.stdBossDPSMult` = 0.9090909090909091 reads as ...092).

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

## parse_oracle.jsonl

One JSON object per line, one record per distinct mod line, sorted by the
line (bytewise). Each is the result of PoB's own `modLib.parseMod` —
`Modules/ModParser.lua` loaded through `Modules/ModTools` exactly as
`Modules/Main.lua` loads it — with the memo cache **not** seeded from
`Data/ModCache.lua` (every line is parsed natively):

```json
{"line": "+10 to Strength",
 "mods": [{"flags":0,"keywordFlags":0,"name":"Str","type":"BASE","value":10}],
 "extra": null,
 "passes": [{"order": 1, "steps": [
   {"list":"specialModList","pattern":null},
   {"list":"preFlagList","pattern":null},
   {"list":"preSkillNameList","pattern":null},
   {"list":"formList","pattern":"^([%+%-][%d%.]+)%%? to","line":80,"caps":["+10"]},
   {"list":"modTagList","pattern":null},
   {"list":"modNameList","pattern":"strength","line":163},
   {"list":"skillNameList","pattern":null},
   {"list":"modFlagList","pattern":null},
   {"list":"suffixTypes","pattern":null}]}],
 "sources": ["affix:Explicit", "affix:Graft", ..., "modCache", "tree:2_6", ...]}
```

* `mods` is the returned mod list in the table encoding above (the same
  encoding as `modCache.json`, so records compare directly with it: a mod's
  tags are its `$seq`, `-0` is `{"$float":"-0"}`; ModParser's jewel closures
  are `$fn` objects whose id is their path inside the record, e.g.
  `mods[1].value.func`, and whose `src` is the ModParser.lua function). `null`
  when parseMod returned nothing.
* `extra`: parseMod's second result (the unparsed remainder) or `null`.
* `passes`: one entry per call of ModParser's local `parseMod(line, order)`
  (6640): order 1, and order 2 when pass 1 returned mods with a remainder
  (6998-7001). `steps` lists, in call order:
  * the early exits at 6641-6656 when taken: the first `jewelFuncList`
    pattern (pairs order) whose match has a capture (`caps`; `candidates` when
    more than one pattern matched), or an exact `jewelFuncList`,
    `clusterJewelSkills` or `unsupportedModList` key (`"exact": true`);
  * every `scan()` call (6616): `list` is the ModParser.lua local the call
    scanned (`specialModList`, `preFlagList`, `preSkillNameList`, `formList`,
    `modTagList`, `skillNameList`, `modNameList`, `penTypes`, `costTypes`,
    `baseCostTypes`, `flagTypes`, `modFlagList`, `suffixTypes`, ...);
    `pattern` the winning key or `null` (no match); `line` the
    ModParser.lua line that defines that key; `gen: true` when the key was
    written by a generating statement (a loop; `line` is that statement)
    rather than a literal table entry; `caps` the captures, when any.
    Keys of `specialModList` that ModParser wraps into `"^" .. k .. "$"`
    (5949-5953) report the line of `k`; `jewelFuncList` keys report the
    `jewel*Funcs` entry they were lowercased from; `regenTypes`,
    `degenTypes`, `costTypes`, `baseCostTypes` report the `resourceTypes`
    entry they were copied from.
  * `ties`: on a full tie (same start, same end, same pattern length) PoB's
    winner depends on `pairs()` order, i.e. it is random per process. The
    oracle then takes the candidate with the lowest ModParser.lua line (then
    the bytewise-smallest pattern), which is one of PoB's possible outcomes
    and the rule of calc-architecture.md section 3.4, and lists every
    candidate as `{"pattern", "line"}`. (At 16de4b82d the corpus has none.)
* `sources`: which corpus parts produced the line (sorted labels):
  `modCache` (every key of `Data/ModCache.lua`); `tree:<version>` (PoB's
  `PassiveTree` class built for every `treeVersionList` entry, as
  `main:LoadTree` does: node, mastery, tattoo and timeless-jewel node stats
  with ProcessStats' line joining); `unique:<type>` (PoB's `Item` class over
  every `data.uniques` entry, all variants, with the default item affix
  quality at 0, 0.05, ..., 1); `catalyst:<type>` (amulet/belt/ring uniques
  with each catalyst at 20%); `rare` (`data.rares`, crafted as `Main.lua`
  does); `affix:<category>` (array-part lines of every `Data/Mod*.lua`
  table: `data.itemMods.*`, master, veiled, necropolis, crucible, beast
  craft mods, through `itemLib.applyRange` at 0, 0.05, ..., 1, retried
  joined with the next line as `Item.lua` does; plus each `tradeHashes`
  stat description's lines joined with a space, as `Main.lua`'s ModCache
  rebuild and `ConfigModBrowser.lua` parse them); `enchant`
  (`data.enchantments`, `/`-separated lines); `cluster` (cluster jewel
  enchant lines, each and joined, small passive stats, notables, keystones);
  `base` (item base implicits at every roll position, flask and tincture
  buff lines); `legion` (timeless jewel additions); `foulborn`
  (`Data/ModFoulbornMap.lua`'s per-unique mutation lines at every roll
  position); `pantheon`.
  Not in the corpus: user-typed text (ConfigTab custom mods, imported
  items), which has no finite source; `Data/ModMap.lua` (map mods carry
  their own `apply` closures, PoB never parses them) and
  `Data/ModScalability.lua` (stat-description templates, not mod lines).
* `error`: present (with `mods`/`extra` null) only if parseMod raised a Lua
  error (none at 16de4b82d).

The instrumentation is done in memory by a `loadfile` source hook
(`src/lua/prelude.lua`, `src/lua/parse_oracle.lua`); every rewrite keeps all
line numbers and no file on disk changes: `scan` and the start of `parseMod`
call recording functions, top-level writes into the pattern tables are
routed through a recorder of the writing line, and `local cache = { }`
(6994) hands the pattern tables to the oracle. `scan` is re-implemented
with identical semantics; it skips a pattern only when a piece of literal
text every match of it must contain is absent from the line (a pure
speed-up; the equivalence is tested).

Negative zero: PoB's reference LuaJIT (x86-64) has only doubles, so
`-tonumber("0")` (the `reduced`/`slower` forms, ModParser.lua:6767, and
functions such as 4343) is `-0`, and `Data/ModCache.lua` records 14 such
`value=-0`. On a dual-number LuaJIT (every ARM64 build, including the
vendored one), `tonumber` returns an integer, the interpreter negates an
integer 0 to `+0` and the JIT to `-0`, so results even varied between runs,
and the literal `-0` is read as `+0`. The dumper therefore (a) rewrites a
`=-0` table value literal to `=(0/-1)` when this LuaJIT loses the sign
(counted in the manifest's `stats` as `negZeroLiterals:<file>`; at
16de4b82d only the 14 in ModCache.lua), and (b) runs the parse oracle with
the JIT off and with ModParser's `tonumber` returning doubles (`n / 1`,
through a chunk environment that is otherwise `_G`). Both are no-ops on a
single-number build; the result is x86-64 PoB's, and the test requires every
ModCache line, sign of zero included, to match.

The manifest's `parseOracle` member
holds the summary: record count, records with mods / without / with a
remainder, errors, full ties, unresolved lines (all 0), new lines per source,
and the size of every pattern table.

## Generated code

`--codegen` writes, into `crates/pob-dump/generated/`:

* `enums.rs`: `skill_type`, `mod_flag`, `keyword_flag` constants;
* `global.rs`: `ModFlag`, `KeywordFlag` (u32, `ENTRIES`, `by_name`),
  `MatchKeywordFlags`, `ClearMatchKeywordFlagsCache`, `SkillType` (u8,
  `COUNT`, `ENTRIES`, `by_name`, `name`), `SkillTypeExpr`, `SkillTypeSet` and
  `colorCodes`, the shape of calc-architecture.md section 4.2;
* `misc.rs`: record structs `misc` (`data.misc`, Modules/Data.lua),
  `gameConstants`, `characterConstants`, `monsterConstants` (Data/Misc.lua):
  one `f64` field per Lua key in source order; keys that are not Rust
  identifiers become `%`->`pct`, `+`->`plus`, `-`->`minus` field names with
  `#[serde(rename = "<verbatim key>")]` and `deny_unknown_fields`, so the
  structs deserialise from `misc.json`; `KEYS`, `PINNED` (the pinned
  values) and `get("<verbatim key>")`.

Every generated item carries a `/// @pob <file>:<a>-<b> h=<sha1[..8]>`
provenance marker and every file a `//! @pob-file <file> @<pin>` header.
`--codegen --check` regenerates in memory and fails when a committed file
differs byte for byte.
