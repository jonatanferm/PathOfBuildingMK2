# pob-oracle

Path of Building's own Lua, run headless. Everything Lua-related lives here so
that `pob-calc` (the Rust port of PoB's calculations) has no Lua runtime:

| Module | What |
|---|---|
| `jit` | PoB booted under vendored LuaJIT (desktop; PoB's own runtime). |
| `lua`, `headless` | A small pure-Rust Lua 5.1 interpreter and PoB booted in it (wasm / fallback). |
| `worker` | A background thread owning a booted PoB; the current app's calc backend. |
| `mod_parser` | PoB's `Modules/ModParser.lua`, run unchanged. |
| `oracle` | The parity harness: PoB's numbers per build as JSON, and typed loaders for tests. |

Binaries: `pob_oracle` (the harness), `pob_headless` (ad-hoc runs with
`HEADLESS_EXEC`), `lua_bench`.

## The oracle

PoB keeps global state between builds (caches, `GlobalCache`, data tables a
calc mutates), so the oracle boots a **fresh process per build**. This is not
theoretical: loading `allflame_exp_000..011` and then `allflame_exp_012` in one
process gives 012 a CombinedDPS of 381,673, while 012 on its own (and in PoB
when opened fresh) is 537,431.

```text
pob_oracle outputs <build.xml> [--json OUT] [--pob SRC]   # stdout without --json
pob_oracle stages  <build.xml>  --json OUT  [--pob SRC]
pob_oracle all     <build.xml>  --out-dir DIR [--pob SRC] # DIR/outputs.json + DIR/stages.json
```

PoB is found with `--pob`, `$POB_SRC`, or `.PathOfBuilding/src` in the nearest
ancestor of the working directory / crate / executable (so it also works from a
git worktree nested in the main checkout). PoB's console output goes to stderr.

Nothing in PoB's sources is changed. `src/oracle.lua` runs after boot and
wraps `calcs.initEnv` from Lua; `stages` arms the wrapper and calls
`calcsTab:BuildOutput()`, so the first `MAIN`-mode `initEnv` without a spec env
(`calcs.buildOutput`'s own) is serialised the moment it returns, before
`calcs.perform` adds to it.

### Corpus runs

`scripts/parity/oracle_dump.sh [build.xml ...]` runs `pob_oracle all` over
`.parity/builds/*.xml` (`JOBS`, default 3, processes at a time) into
`.parity/oracle/<stem>/{outputs,stages,log}.json|txt` plus `timings.tsv`.
Release build, 8-core machine, 3 jobs: all 57 corpus builds in 53.5 s wall
(2.7 s per build on average, 5.9 s worst); 82 MB of JSON.

`scripts/parity/diff.py REF CAND` compares two sets of output trees (files or
directories of `<stem>/outputs.json` / `<stem>.json`) key by key and prints a
scoreboard: numbers match when `|d| < 0.5` or `|d| <= 1%` of the larger
magnitude, booleans and strings exactly. It reads oracle dumps and any plain
JSON object of outputs, so the Rust calc only has to write its output tree as
JSON to be scored against PoB. Options: `--key K` (one key across builds),
`--only REGEX`, `--builds`, `--top N`, `--abs`, `--rel`, `--fail`.

### From Rust

```rust
use pob_oracle::{Oracle, OracleOutput, OracleStages};

let out: OracleOutput = Oracle::outputs("build.xml")?;   // fresh pob_oracle process
let life = out.num("Life");                               // top-level number
let speed = out.get("MainHand.Speed");                    // dotted path, arrays 1-based
let flat = out.flat();                                    // path -> Leaf (Num / Bool / Str)

let st: OracleStages = Oracle::stages("build.xml")?;
let mods: &[pob_calc::Mod] = &st.init_env.player.mod_db.mods;
let main = st.main_skill().unwrap();                      // SkillStage: types, flags, data, cfg, mods
```

`Oracle::locate()` finds the binary via `$POB_ORACLE_BIN` or next to the
running executable (`cargo build -p pob-oracle --bin pob_oracle` first; inside
this crate's tests use `Oracle::with_bin(env!("CARGO_BIN_EXE_pob_oracle"))`).
With `$POB_ORACLE_CACHE` (e.g. `.parity/oracle`) set, dumps newer than the
build file are read from there instead of re-running PoB. `oracle::load_outputs`
/ `load_stages` read a dump file directly.

## JSON conventions

Plain JSON, one convention for every Lua value:

| Lua | JSON |
|---|---|
| `nil` | `null` (or the key is absent) |
| boolean | `true` / `false` |
| finite number | number; integers without a fraction, others as the shortest decimal that reads back to the same double (`%.15g`/`%.16g`/`%.17g`) |
| `inf`, `-inf`, NaN | `{"$num": "inf"}`, `{"$num": "-inf"}`, `{"$num": "nan"}` |
| string | string (control characters escaped; other bytes verbatim, read lossily as UTF-8) |
| table that is a non-empty sequence `1..n` | array |
| any other table | object, keys sorted; numeric keys written as strings (`"1"`, `"2.5"`); boolean keys as `"true"`/`"false"`; functions, userdata and table keys dropped |
| empty table | `{}` |
| PoB mod (string `name` and a mod `type`) | the mod shape below |
| PoB class instance | `{"$item": name}` for items, `{"$object": className}` otherwise |
| active skill / gem record / gem instance / granted effect | `{"$skill": id}`, `{"$gem": name}`, `{"$gemInstance": nameSpec}`, `{"$grantedEffect": id}` |
| a table already being encoded / nesting too deep | `{"$cycle": true}` / `{"$depth": true}` |

Single-key objects whose key starts with `$` are therefore markers, never data.
In the corpus only `Req{Str,Dex,Int}Item.sourceItem/sourceGem` (which item or gem
sets an attribute requirement) produce reference markers in outputs.

### Mods

The same shape is `pob_calc::modifier::Mod`'s serde format
(`pob_calc::modifier_serde`), i.e. the Lua table `modLib.createMod` builds with
its array part (the tags) under `"tags"`:

```json
{"name": "Life", "type": "BASE", "value": 12, "flags": 0, "keywordFlags": 0,
 "source": "Base", "tags": [{"type": "Multiplier", "var": "Level", "base": 38}]}
```

`source` / `sourceSlot` appear when set; `replaced` / `converted` when true;
`value` is any value above (a nested `{ mod = … }` value holds a mod object).
Tags use PoB's field names (`limitVar`, `skillNameList`, `modFlags`, …). PoB
writes every field it set; the Rust serialiser omits fields that are unset,
`false` or zero, which the typed model treats alike. Every mod of the
OccVortex stages dump round-trips through `Mod` unchanged up to that
(`tests/oracle_harness.rs`).

### `outputs.json`

```text
{ "format": "pob-oracle/outputs/1", "build": <stem>, "pobVersion": "2.67.2",
  "mainSkill": { "index", "id", "name", "skillPartName" },     # index into activeSkillList, 1-based
  "player": <env.player.output>,                             # calcsTab.mainEnv, after the build loaded
  "minion": <env.minion.output> | null }
```

`player` is the complete table: numbers, booleans, strings and nested tables
(`MainHand`/`OffHand`, `SkillDPS` list, `*WarningList`, `Req*Item`, …).

### `stages.json`

```text
{ "format": "pob-oracle/stages/1", "build", "pobVersion",
  "mainEnvIsCaptured": bool,            # the captured env is calcsTab.mainEnv
  "initEnv": {                          # right after calcs.initEnv(build, "MAIN")
    "mode": "MAIN", "mainSkillIndex",
    "player" / "enemy" / "minion"|null: { "level", "modDB": <store> },
    "itemModDB": <store>,
    "activeSkills": [ {                 # env.player.activeSkillList order
      "index", "id", "name", "isMain", "socketGroup", "slot", "includeInFullDPS",
      "skillPart", "skillPartName", "minion",
      "effects": [ { "id", "name", "support", "level", "quality", "qualityId" } ],
      "skillTypes": [SkillType ids, sorted], "skillTypeNames": [...],
      "skillFlags": {...}, "skillData": {...},
      "skillCfg" / "weapon1Cfg" / "weapon2Cfg": {   # skillGem -> name, skillGrantedEffect -> id,
          "flags", "keywordFlags", "skillName", "skillTypes": [...], "skillCond", ... },
      "skillModList": <store> } ] },
  "skillOutputs": [ {                   # every activeSkillList entry with a socket group
    "index", "id", "name", "socketGroup", "includeInFullDPS", "count", "enabled",
    "skillPartName", "error", "output", "minionOutput", "mirage": { "name", "count", "output" } } ] }

<store> = { "class": "ModDB"|"ModList", "mods": [Mod...], "conditions": {...},
            "multipliers": {...}, "parent": <store>|null }
```

* `initEnv.minion` is whatever `env.minion` holds when `initEnv` returns;
  PoB creates the minion actor in `calcs.perform`, so it is `null` for all 57
  corpus builds (minion outputs are in `skillOutputs[].minionOutput`).
* ModDB mods are grouped by name (names sorted), PoB's order within a name;
  ModList mods are in PoB's order.
* `skillOutputs` follow `calcs.calcFullDPS`: a fresh `CALCULATOR` env per
  skill, `env.player.mainSkill` set to that skill, `calcs.perform(env, true)`
  (EHP skipped). For the main skill this equals `outputs.json`'s `player`
  except for keys only the MAIN env has: the EHP / FullDPS / SkillDPS keys,
  and display values PoB fills only when building breakdowns
  (`Bleed/Ignite/PoisonDotMulti`, the coloured `ReqStrString`). Over the 57
  corpus builds no other key present in both differs (TotalDPS is checked in
  `tests/oracle_harness.rs`). `count` / `enabled` are calcFullDPS's
  `getActiveSkillCount`.

## Tests

`cargo test -p pob-oracle` runs the harness end to end on OccVortex
(`tests/oracle_harness.rs`), the ModCache parser oracle and the worker /
ModParser smoke tests. They need the PoB checkout (and `data/lua`,
`.parity/builds`); they are found upward from the crate, and the tests skip
with a message when absent.
