# CCR 0010 (P3): pob-state/1 details as built

**Requested by:** P3 (oracle core). **Affects:** calc-architecture.md §9.2, §9.5
(documentation only; no Rust contract changes). The full format is in
`crates/pob-oracle/README.md` ("The probe oracle").

## 1. Data markers use pob-dump canonical paths

§9.2 says data objects are written as `{"$data":"skills/Barrage"}`. AMENDMENTS
A2 makes `crates/pob-dump/FORMAT.md` the binding dump encoding, whose
canonical paths are Lua-like (`data.skills.Barrage`,
`data.skills.Barrage.levels[20]`). The probe dumps use the same paths, so a
marker can be resolved against `data/dump` directly.

Proposed diff (§9.2, "pob-state/1 dump"):

```diff
-Data objects are written as `{"$data":"skills/Barrage"}`, nodes as `{"$node":id}`, ...
+Data objects are written as `{"$data":"data.skills.Barrage"}` (pob-dump canonical paths, FORMAT.md), nodes as `{"$node":id}` (allocated) / `{"$treeNode":id}`, ...
```

## 2. skillCond layers with a function `__index`

§9.2 asks for "skillCond flattened per layer, with its IndexMeta kind". Table
`__index` layers (CalcActiveSkill.lua:482, :487) are dumped through `$meta`
with `__index` as a `$ref` to the parent layer, which is exactly the layer
structure. The one function `__index` layer in Calc*.lua
(CalcOffence.lua:4422, `function(table, key) return skillCfg.skillCond[key] or cfg.skillCond[key] end`)
is dumped as `{"$fn": "Modules/CalcOffence.lua:4422-4422", "upnames": ["skillCfg", "cfg"]}`:
the oracle's LuaJIT state has no `debug` library (mlua safe mode, `jit.rs`),
so upvalue values are unreachable; both upvalues are `offence` locals that
the probes at that site can list in `locals`.

Proposed diff (§9.2):

```diff
-... every cfg (skillCond flattened per layer, with its IndexMeta kind), ...
+... every cfg (skillCond layers via `$meta.__index`: a `$ref` for table layers, `{"$fn", "upnames"}` for the function layer of CalcOffence.lua:4422), ...
```

## 3. Default dump selection is the final main env

Hits are numbered per process, and a build load runs `calcs.buildOutput`
several times plus `buildActiveSkill`/`calcFullDPS` envs, so `perform:in#1`
is usually not the env whose outputs are `outputs.json`. The probe runtime
adds a `<glob>@main` selector (first hit of each matching probe in
`calcsTab.mainEnv`) and uses it by default for env-level probes and line
probes. Nothing in §9.2 changes; ORC-X tests should name hits as
`hits.json` lists them (e.g. `offence:out#4`), not assume `#1`.

## 4. Nondeterminism sampling (§9.5)

Over the 57 builds PoB's outputs never differed between runs. The dumped
states differ mostly in mod order within mod lists / ModDB names
(string-keyed iteration in PoB), but also in values: **the gem variant a
skill resolves to varies between PoB processes** (allflame_exp_019:
`ExplosiveTrap` / `ExplosiveTrapAltX` for one skill uuid;
upstream_Dual_Wield_Cospris_CoC: `VaalRighteousFire` / its AltX variant in
`requirementsTableGems` and `effectList[i].gemData/grantedEffect`). The calc
port has to accept either variant where PoB does. A third run still adds
new order-only entries for many builds, so the record is a sample.
`pob_oracle nondet --max-runs N` keeps running while runs add entries.

The record (`pob-nondet/2`) uses the paths the state diff walks (the
resolved root, `env.…`), records a list as unordered only when its own
elements move, and records mod-store differences per store name (an order,
or each run's whole list as an accepted multiset). Proposed addition to
§9.5:

```diff
+Order-only differences inside mod stores are sampled, not enumerated; the state
+diff compares mod stores as multisets and reports unrecorded order differences as
+`LUA-ORDER` (not a value mismatch). PoB's run-to-run value differences inside a
+mod store are recorded per store name as alternative multisets.
```
