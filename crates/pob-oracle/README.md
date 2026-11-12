# pob-oracle

Path of Building's own Lua, run headless. Everything Lua-related lives here so
that `pob-calc` (the Rust port of PoB's calculations) has no Lua runtime:

| Module | What |
|---|---|
| `jit` | PoB booted under vendored LuaJIT (desktop; PoB's own runtime). Oracle parity runs boot through `JitPob::boot_oracle`, which switches the JIT off first (`jit.off()`: the interpreter is the reference, CCR-0001/0002; `POB_ORACLE_JIT=on` for diagnostics only). |
| `lua`, `headless` | A small pure-Rust Lua 5.1 interpreter and PoB booted in it (wasm / fallback). |
| `worker` | A background thread owning a booted PoB; the current app's calc backend. |
| `mod_parser` | PoB's `Modules/ModParser.lua`, run unchanged. |
| `oracle` | The parity harness: PoB's numbers per build as JSON, and typed loaders for tests. |
| `patcher` | The probe oracle's patched copy of PoB's sources (line / function probes, write provenance). |
| `runner` | Runs the patched PoB (one build per process, machine-wide lock, probe cache). |
| `schema` | Readers for `pob-state/1`, `pob-hits/1`, `pob-provenance/1`; `$id`/`$ref` resolution. |
| `diff` | PoB-vs-Rust comparison (§9.2 tolerance, mod-store multisets, `LUA-ORDER`, unit filters). |
| `nondet` | PoB's run-to-run differences (`.parity/nondet/<build>.json`). |
| `inject`, `replay`, `testkit` | ORC-X: a `pob-state/1` dump injected into the Rust calc graph (and `pob_calc::probe::export_root` back), recorded callee deltas applied in place, helpers for the unit L4/L5 tests (`segment_diff`, `round_trip`). |
| `mod_json` | Mods, tags and mod values of the oracle's JSON as `pob_gamedata::{Mod, Tag, Value}` (the encoders are `pob_calc::probe::{mod_json, tag_json, value_json}`). |

Binaries: `pob_oracle` (the harness), `pob_calc_diff` (PoB vs Rust),
`pob_timing` (LuaJIT ModStore query timings, gate G1a), `pob_headless`
(ad-hoc runs with `HEADLESS_EXEC`), `lua_bench`.

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
let mods: &[pob_gamedata::Mod] = &st.init_env.player.mod_db.mods;
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

The same shape is what `pob_oracle::mod_json::decode_mod` reads into a
`pob_gamedata::Mod` and `pob_calc::probe::mod_json` writes, i.e. the Lua table
`modLib.createMod` builds with its array part (the tags) under `"tags"`:

```json
{"name": "Life", "type": "BASE", "value": 12, "flags": 0, "keywordFlags": 0,
 "source": "Base", "tags": [{"type": "Multiplier", "var": "Level", "base": 38}]}
```

`source` / `sourceSlot` appear when set; `replaced` / `converted` when true;
`value` is any value above (a nested `{ mod = … }` value holds a mod object).
Tags use PoB's field names (`limitVar`, `skillNameList`, `modFlags`, …). PoB
writes every field it set; the Rust serialiser omits fields that are unset,
`false` or zero, which the typed model treats alike (a tag key the typed
`Tag` cannot hold exactly is kept verbatim in `tag.extra`). Every mod of the
OccVortex stages dump round-trips through `Mod` unchanged up to that
(`tests/oracle_harness.rs`), and every mod of the all57 probe dumps exactly
(`tests/ORC-X.rs`).

### `outputs.json`

```text
{ "format": "pob-oracle/outputs/1", "build": <stem>, "pobVersion": "2.67.2",
  "luajit": { "arch": "arm64", "jit": false, "numberMode": "dual" },   # the LuaJIT that ran (CCR-0001/0002)
  "mainSkill": { "index", "id", "name", "skillPartName" },     # index into activeSkillList, 1-based
  "player": <env.player.output>,                             # calcsTab.mainEnv, after the build loaded
  "minion": <env.minion.output> | null }
```

`player` is the complete table: numbers, booleans, strings and nested tables
(`MainHand`/`OffHand`, `SkillDPS` list, `*WarningList`, `Req*Item`, …).

### `stages.json`

```text
{ "format": "pob-oracle/stages/1", "build", "pobVersion", "luajit",
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

## The probe oracle (calc-architecture.md §9.2, §9.5)

```text
pob_oracle patch   [--stages F] [--pob SRC] [--root DIR]        # prints <root>/<sha>/src
pob_oracle probe   <build.xml> --out-dir DIR [--stages F | --patched SRC]
                   [--select G,..] [--max-dumps N] [--no-provenance] [--cache]
pob_oracle nondet  <build.xml> [--out FILE] [--keep DIR] [--verify] [--max-runs N] [--cache]
                   [--stages F | --patched SRC] [--select G,..]
pob_calc_diff outputs POB OURS [--json-out DIR] [--nondet DIR] [--unit ID] [--provenance DIR]
pob_calc_diff state   POB.json[.zst] OURS.json[.zst] [--nondet FILE]
pob_timing <build.xml> [--iters N] [--json OUT]
```

Every command that boots PoB holds an exclusive `flock` on
`$POB_ORACLE_LOCK` (default `/tmp/pob-port-oracle.lock`), the lock
`scripts/port/oracle.sh` takes, so one LuaJIT PoB (≈400 MB) runs at a time
machine-wide. Run corpus work through the script:

```text
scripts/port/oracle.sh run --build X.xml [--stage HIT[,HIT..]] [--stages F]  # -> probe cache
scripts/port/oracle.sh corpus [--stages F] [build.xml ..]   # -> .parity/probe/<stem>/ + cache
scripts/port/oracle.sh nondet [--verify] [--max-runs N] [--stages F] [build.xml ..] # -> .parity/nondet/<stem>.json
```

`--stages` defaults to `docs/port/stages.toml` (U0); the P3 tests use the
dummy `corpus/P3/stages.toml`.

### Patched sources

`patcher::patch` copies PoB's `src/` to `.parity/patched-src/<sha>/src/`
(`patcher::shared_root()`: one copy per `<sha>` for every worktree and caller,
`$POB_ORACLE_SRC_ROOT` overrides; FUP1, CCR-0102 §2. Copies unused for 24 h are
pruned, `$POB_PATCHED_SRC_MAX_AGE_HOURS`)
(patched files written, the rest hard-linked; every other entry of the PoB
checkout symlinked next to it, so `runtime/` and `manifest.xml` resolve) and
writes `manifest.json` (every site and edit). `<sha>` hashes the patcher
version, the stages file, the patched texts and a stat fingerprint of the
source tree. No line number changes. Every patched file is compiled with
`loadstring` (after the loader's `a += b` rewrite) before it is published.

* **Line probes** (`[[stage]]`: `id`, `file`, `line`, `locals`, optional
  `expect` text the line must contain): the line is prefixed with
  `if __PROBE then __PROBE("<id>", { a = a, ... }) end `.
* **Function probes**: `calcs.initEnv`, `perform`, `defence`,
  `buildDefenceEstimations`, `offence`, `triggers`, `mirages`,
  `buildActiveSkillModList`, `createActiveSkill`, `runSkillFunc`,
  `getCachedOutputValue`, `ConfigTab:BuildModList`, every skill hook of
  `Data/Skills/*.lua` (`skill:<id>:<field>`, 133), every ConfigOptions
  `apply` (`config:<var>:apply`, 531 + `config:mapAffixDropDownFunction`),
  plus `[[function]]` entries of the stages file (`mergeBuff` in
  `corpus/P3/stages.toml`). `function a.b(p) ... end`
  becomes `a.b = __PROBE_FN("<id>", {"p"}, function(p) ... end)` (methods
  and `local function` likewise); the wrapper fires `<id>:in` and `<id>:out`.
  With the P3 stages that is 678 function probes: 13 named + 133 skill
  hooks + 532 config applies.
* **Call probes**: the three jewel-func call sites of CalcSetup
  (`jewelFunc`), counted separately from the function probes.
* **Write provenance**: the creation sites of `env.player.output`,
  `env.enemy.output`, `output.MainHand/OffHand/Minion`, `minion.output` and
  the breakdown table become `__PROV(kind, {})` proxies whose `__newindex`
  records the last writer `file:line` per key (`pairs`/`next`/`ipairs` are
  replaced before PoB boots so they iterate the backing table in the same
  order); every statement of `ModDB.lua`/`ModList.lua` that stores a mod
  records the first caller outside the mod store classes.

Over the 57 corpus builds the probed PoB's `outputs.json` is identical to
the unpatched oracle's.

### Hits and dumps

Hits are numbered `<id>#<n>` per process (a function probe's `:in` and
`:out` share n), which separates passes, recursive envs and repeated
performs; each dump records its env (`env<k>`, numbered as `initEnv`
returns them) and the stack of enclosing function-probe hits. `--select`
globs (`perform:in#1`, `offence:*`, `config:*:apply:in#1`) choose which
hits are serialised; `<glob>@main` takes, per matching probe id, the first
hit in the build's final main env (`calcsTab.mainEnv`, whose outputs are
`outputs.json`; candidates are the envs of `calcs.buildOutput(build,
"MAIN")`, buffered until the run ends). The default is
`runner::DEFAULT_SELECT` (the env-level probes and skill hooks `@main`, the
env-less config applies / jewel funcs / `ConfigTab:BuildModList` `#1`) plus
`<id>@main` for every line probe. A probe run directory holds:

* `outputs.json`: as `pob_oracle outputs`;
* `hits.json` (`pob-hits/1`): `counts` per probe id, `dumps`
  (`{hit, file, stack, env}`), `envs`, `mainEnv`, `skipped`;
* `provenance.json` (`pob-provenance/1`): flattened output key -> last
  writer for `output`, `breakdown`, `enemy`, `minion`, `minionBreakdown` of
  the main env;
* `state/<hit>.json` (`pob-state/2`): `{"$env", build, format, hit, kind
  (line|in|out), n, probe, root: {env, locals, globalCache}, stack}`.

`pob-state/2` values follow `crates/pob-dump/FORMAT.md` (`$seq`, `$num`,
`$keys`, `$meta`, `$float`, `$bytes`, `$$`-escaped keys), keys sorted.
Identity: every table is written in full once, at its canonical path (first
reached breadth-first, so the shortest path wins), with `"$id": n` when it is
referenced again, and as `{"$ref": n}` elsewhere (`schema::resolve_ids`
turns these into path references). Markers: `{"$data": "data.skills.X"}`
for PoB's data tables, `{"$node": id}` / `{"$treeNode": id}`,
`{"$object": class}`, `{"$foreign": {env, path}}` for other envs' envs, actors,
active skills and breakdowns (their mod stores and actor outputs are written in full),
`{"$fn": "file:a-b"}`. Mod stores are `{"$store": "ModDB"|"ModList",
"mods": ...}` (ModDB grouped by name, names sorted) with each mod in
the mod shape (`pob_oracle::mod_json`) plus `"$at": "<file>:<line>"`; items are
`{"$item": name, ...}` with modList, slotModList and the weapon / armour /
flask / tincture / jewel data; active skills carry `"$uuid"`. Two members are
injection inputs that no comparison reads (`schema::INPUT_KEYS`, with `$at`),
added by FUP3 (the `/2`; `/1` dumps lack them and are still read): an item's
`"$modLines"` (the mod lines `Item:FindModifierSubstring` reads and the item's
variant fields; `inject` fills the item's `*ModLines`), and a ModParser.lua
closure's `"$up": [[name, value], …]` (its upvalues, read through the
`getupvalue` that `JitPob::install_probe_upvalue_reader` gives the encoder;
`mod_json::closure_args` fills `JewelFuncSpec.args`).

Dumps are cached zstd-compressed at
`.parity/probe-cache/<sha>/<build-blake3>/<hit>.json.zst`
(`runner::cached_state` prints the `oracle.sh` command on a miss).
`<sha>` (`patcher::plan_sha`) and the in-process `runner::source_key` take the
stat fingerprint (path, size, mtime) of every file of PoB's `src/` except
`Data/TimelessJewelData/*.bin` (`patcher::is_derived_cache`: PoB inflates the
zips into these and rewrites them on any run that finds them stale), plus the
`mlua` / `mlua-sys` / `luajit-src` versions of `Cargo.lock` and
`runner::ORACLE_CACHE_EPOCH`.

The same tree holds the tests' shared entries (HYG3): `query-<key>/` (the
results of in-process queries, `runner::QueryCache`) and `sel-<key>/` (one
`bin probe` run of a selection, `runner::SelectionCache`; the key also hashes
`runner.rs` and `bin/pob_oracle.rs`, so a change to how a dump is made
re-probes). `POB_QUERY_CACHE=refresh` reruns PoB and overwrites an entry;
`POB_QUERY_CACHE=off` neither reads nor writes one (a probe run goes to a
private `target/tmp/probe-off/` directory that is deleted after use).

### Nondeterminism

LuaJIT seeds its string hash per process, so PoB's `pairs` order over
string keys (and everything derived from it) changes between processes.
`pob_oracle nondet` probes a build in two processes, and in a third when
they differ (always with `--verify`), and writes every differing output
key / state path with the values PoB produced, plus lists whose order alone
differs, to `.parity/nondet/<stem>.json` (`pob-nondet/2`). With
`--max-runs N` it keeps running (up to N processes) while a run still adds
something the record does not cover structurally (list indices
generalised); the result's `lastNew` lists what the last run added.
`pob_calc_diff` accepts any recorded value and compares recorded lists as
multisets.

State paths in the record are the paths `pob_calc_diff state` walks (the
resolved dump root: `env.player.activeSkillList[6].baseSkillModList.mods`;
a differing hit stack / env id is kept under `$stack` / `$env`). A list is
recorded as unordered only when its own elements move (not when only lists
nested in its elements do). Mod stores are recorded where the diff
compares them, per store name (`<store>.mods.<Name>`, a ModList's
`<store>.mods`): as an order when the runs' multisets agree, otherwise as
each run's whole list (an absent name as `[]`), which the diff accepts as a
multiset. Records of another format fail to load (regenerate them).

Over the 57 corpus builds (`--verify --max-runs 6`: 3-6 processes per
build, 52 builds converged) PoB's outputs never differed between runs; the
differences are inside the dumped states (22,730 order entries, 222 value
entries, 93 of them per-name mod-store alternatives). Most are the order of mods within
a mod list or a ModDB name (`baseSkillModList`, buff mod lists, `enemyDB`
lists), which PoB builds by iterating string-keyed tables. But PoB also
differs in values: **which gem variant a skill resolves to changes between
processes** (allflame_exp_019: `ExplosiveTrap` or `ExplosiveTrapAltX` for
the same skill uuid, visible in `baseSkillModList` mod sources;
upstream_Dual_Wield_Cospris_CoC: `VaalRighteousFire` or its AltX variant in
`requirementsTableGems` and `effectList[i].gemData/grantedEffect`). A calc
port that picks either variant matches PoB; a test that pins one does not.
(The Lua cause is not traced yet; Modules/Data.lua builds `gemForSkill` /
`gemForBaseName` / the AltX/AltY Vaal variants inside `pairs(data.gems)`.)
Order differences are only sampled by a few runs, so a further run can
still show an order the record lacks; the state diff compares mod stores as
multisets and reports order-only differences as `LUA-ORDER`, not as value
mismatches.

### `pob_calc_diff`

`outputs` pairs builds by stem (files or directories, as
`scripts/parity/diff.py`) and compares flattened outputs:
`|a-b| <= 1e-9·max(1,|a|)`, booleans and strings exact, nondeterministic
keys per `.parity/nondet`. `--json-out DIR` writes
`DIR/<stem>.json = {build, pob, ours, mismatches, summary}`, which
`scripts/parity/report.py DIR` scores. `--unit ID` keeps only mismatches
whose PoB writer (provenance) lies in the unit's Lua ranges from the unit
table of `docs/port/calc-architecture.md`. `state` diffs two `pob-state/1`
dumps: mod stores as multisets per name, order-only differences reported as
`LUA-ORDER`.

## Tests

`cargo test -p pob-oracle` runs the harness end to end on OccVortex
(`tests/oracle_harness.rs`), the probe oracle's acceptance tests
(`tests/P3.rs`; `-- --ignored all57` checks the `oracle.sh corpus` /
`nondet` results over the whole corpus), the ModCache parser oracle and the
worker / ModParser smoke tests. They need the PoB checkout (and `data/lua`,
`.parity/builds`); they are found upward from the crate, and the tests skip
with a message when absent.
