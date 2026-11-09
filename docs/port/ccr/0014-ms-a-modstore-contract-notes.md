# CCR-0014: MS-A ModStore contract notes

Status: proposed by MS-A (2026-10-09). Numbering: first free number at the time of writing;
the integrator may renumber (the workaround markers in MS-A's files say `CCR-0014`).

Found while porting Classes/ModStore.lua 1-362, ModDB.lua, ModList.lua and running the MS-A
acceptance test (`crates/pob-oracle/tests/MS-A.rs`, L3 query matrix on all57).

## 1. `StoreMods::DB` is an unordered map (classes/mod.rs, U0)

`StoreMods::DB(ahash::AHashMap<Name, Vec<ModRef>>)` has no stable name order, but calc doc §5.3
documents `all_mods()` as "ModDB: name order is insertion order of names" and §3.4 asks string
keyed tables to follow insertion order. No Lua query observes the order (every query loops the
names it is given; `AddDB`'s `pairs(modDB.mods)` only decides which bucket is created first), so
this is about the Rust-only accessors (`all_mods`, `mod_names`) that the oracle, injection and the
UI read.

Proposed diff (crates/pob-calc/src/classes/mod.rs):

```diff
-    DB(ahash::AHashMap<Name, Vec<ModRef>>),
+    DB(indexmap::IndexMap<Name, Vec<ModRef>, ahash::RandomState>),
```

and the constructors' `ahash::AHashMap::new()` → `indexmap::IndexMap::default()` (two sites).
`indexmap` is already a pob-calc dependency. Workaround until then: `ModStore::all_mods` /
`mod_names` return ModDB names sorted bytewise (`// CCR-0014` in `classes/mod_store.rs`).

## 2. `Actor.activeSkillList` cannot be nil (actor.rs, U0)

The enemy actor is `{ modDB = env.enemyDB, level = env.enemyLevel }` (Modules/CalcSetup.lua:519-522):
it has no `activeSkillList`. So `GetStat("ManaReservedPercent" | "LifeReservedPercent", …)` on the
enemy modDB raises in PoB (`ipairs(nil)` at Classes/ModStore.lua:322 / 336, once `output.Mana` /
`output.Life` is non-zero), while the Rust `Actor` always holds a (possibly empty) list and returns
0. Measured: 964 such queries over all57 (all on the enemy store; every one is a PoB error).
These stats are read by `PerStat`/`StatThreshold` tags; a tag that reads them on the enemy is not
known in the data, so the impact is theoretical.

Proposed diff (crates/pob-calc/src/actor.rs): `pub activeSkillList: Field<Option<Shared<Vec<ActiveSkillRef>>>>`
with `None` for the enemy (CS1 creates the player's as `Some`), or accept the divergence and list
it in `known_diffs.toml`. No workaround in GetStat (it cannot tell the enemy apart); the MS-A test
counts these queries as known differences.

## 3. Tag key `warcryPowerBonus` (pob-gamedata Tag census, U0) and a tag mutation site

Modules/CalcPerform.lua:2380-2384 writes into the first tag of every warcry buff mod:

```lua
for _, warcryBuff in ipairs(buff.modList) do
    if warcryBuff[1] and warcryBuff[1].effectType == "Warcry" and warcryBuff[1].div then
        warcryBuff[1].warcryPowerBonus = m_floor((warcryBuff[1].limit and m_min(warcryPower, warcryBuff[1].limit) or warcryPower) / warcryBuff[1].div)
```

(read back at 2398, 2411, 3090, 3099). Two consequences:

- `pob_gamedata::Tag` has no `warcryPowerBonus` field and its deserialiser rejects unknown keys
  ("unknown tag key \"warcryPowerBonus\""), so such a mod cannot be represented: 4 mods over
  all57 (warcry builds) in the MS-A dump. Proposed: add `pub warcryPowerBonus: Option<f64>` to
  `Tag` (field, `gen_to_tag`, `for_each_tag_field`), or route unknown numeric keys to `Tag.extra`.
- The write mutates a tag of a mod that `buff.modList` received **without a copy**
  (Modules/CalcActiveSkill.lua:862 `t_insert(modList, skillModList[i])`), i.e. possibly a mod
  shared with the skill's stat-map mods. This site is missing from contracts.md#mod-mutation
  (§3.6 requires a CCR before porting such a site). The owner of CalcPerform 2366-2420 (PF unit)
  must check whether the mod is fresh at that point; if it is shared, the Rust port has to rebuild
  the mod (or keep the bonus outside the tag) and accept the identity difference.

## 4. MS-A acceptance items that depend on other units

- `Max`/`Min` call `self:EvalMod(value.mod, cfg)` on **every** row, tagged or not
  (Classes/ModStore.lua:251, 262). With EvalMod a stub (MS-B), a non-empty Max/Min cannot be
  computed, so "Max returns nil for all values ≤ 0" is only demonstrable after MS-B. The MS-A test
  detects a ported EvalMod (it returns `mod.value` for an untagged mod) and then gates Max/Min.
  Until then it reports them: 279,758 / 280,026 equal on all57; the other 268 are calls with at
  least one MAX/MIN row, where the stub EvalMod returns nil and Rust raises the comparison error. Proposed: the calc doc §11.1 MS-A row reads "Max returns nil for all
  values ≤ 0 (gated once MS-B's EvalMod lands)".
- `NewMod`, `ReplaceMod`, `ConvertMod` build their mod with `modLib.createMod`, and `MergeMod` uses
  `modLib.compareModParams` (unit CT; stubs at the time of writing). The MS-A test runs its
  NewMod/ReplaceMod/ConvertMod/MergeNewMod scenarios against PoB once both are ported (detected at
  run time). Verified locally with a temporary createMod/compareModParams (not committed):
  6,840 / 6,840 scenario stores equal on all57. `ReplaceModInternal`/`ConvertModInternal` parent
  recursion is covered without CT by ScaleAddList(replace) against PoB and by unit tests.

## 5. Doc text

calc doc §5.3 lists `pub fn MergeMod(&self, m: ModRef)`; the frozen code (and ModList.lua:73) has
`MergeMod(&self, m: ModRef, skipNonAdditive: bool)`. Proposed: fix the doc line.
