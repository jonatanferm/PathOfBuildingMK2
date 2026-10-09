# CCR-0011 (U0): corrections to the calc doc's contract text, as frozen by U0

**Raised by:** U0 (skeleton). **Affects:** calc-architecture.md §3.2, §4.3, §4.4, §5.1-§5.4, §5.7,
§5.10, §6, §7.2. **Status:** implemented in U0's frozen files (they are the contract); this CCR asks
the integrator to update the doc text to match. `docs/port/contracts.md` is the reference for the
final shapes. Pin: 16de4b82d.

Each item: the doc text, the evidence, what U0 froze.

## 1. `copyTable` does not keep the metatable (§3.2)

§3.2: "`wipeTable(t)` / `copyTable(t)` / `copyTable(t, true)` → `t.wipe()` / `t.copyTable(false)` /
`t.copyTable(true)`. Both keep the metatable (Common.lua:496)."

Evidence: `Modules/Common.lua:477-487` (`copyTable`) never calls `setmetatable`; line 496 is inside
`copyTableSafe`, which keeps it only with `preserveMeta`. `wipeTable` (549-557) only nils keys, so it
keeps the metatable.

Froze: `Table::copyTable` returns a table **without** metatable; `Table::wipe` keeps it.

```diff
-| `wipeTable(t)` / `copyTable(t)` / `copyTable(t, true)` | `t.wipe()` / `t.copyTable(false)` / `t.copyTable(true)`. Both keep the metatable (Common.lua:496). |
+| `wipeTable(t)` / `copyTable(t)` / `copyTable(t, true)` | `t.wipe()` / `t.copyTable(false)` / `t.copyTable(true)`. `wipe` keeps the metatable; `copyTable` does not (Common.lua:477-487; only `copyTableSafe(…, preserveMeta)` copies it, 494-496). |
```

## 2. `ModStore.parent` is reassigned (§5.3)

`CalcSetup.lua:689-692`: `env.modDB.parent = cachedPlayerDB`, `env.enemyDB.parent = cachedEnemyDB`,
`env.minion.modDB.parent = cachedMinionDB`. Froze `pub parent: Field<Option<ModStoreRef>>`.

## 3. `initEnv`'s cached DBs are optional (§5.10, §7.2)

`CalcSetup.lua:472-474` annotates them `ModDB?`; they are nil until CS1's `specCopy` (590-692).
`InitEnvCtx.env` and `.modDB` are nil until CS1 creates the env (481, 486, 489-506). Froze
`initEnv(...) -> (EnvRef, Option<ModDB>, Option<ModDB>, Option<ModDB>)` and
`InitEnvCtx { env: Option<EnvRef>, modDB: Option<ModDB>, … }`.

## 4. Other ctx field types (§7.2)

- `PerformCtx.allyBuffs: Option<Table>`: `local allyBuffs = env.partyMembers["Aura"]`
  (CalcPerform.lua:2206) is nil when the party actor has no `Aura` entry.
- `OffenceCtx.ailmentData: Option<&'static IndexMap<Name, AilmentData>>`: it is
  `data.nonDamagingAilment` (CalcOffence.lua:4156), immutable data, not a calc table; `None` until
  OF-G declares it.

## 5. `ModCfg::STRING_CFG` is a function (§5.2)

A `ModCfg` holds `Cell`/`RefCell`/`Rc` and is not `Sync`, so it cannot be a `static`. Froze
`ModCfg::STRING_CFG() -> &'static ModCfg` (one leaked all-nil cfg per thread).
`ModCfg` also gets `conversionTable` (CalcOffence.lua:77/145 read `cfg.conversionTable`) and
`baseFlags` (ModStore.lua:882 reads `cfg.baseFlags`).

## 6. Mod / Tag / Value (§4.3), from the census of the dump, ModCache and the parse oracle

- `ModType::Nil`: `sup_str.lua:1448` creates `mod("CorruptingCryStagesFromExerted", nil, 0, …)`.
- `TagType` gains `Global` (1252 hits), `Skill` (2) and `LuaNonTable` (a non-table tag entry:
  SkillStatMap.lua:1735 `skill("Damage", nil, ModFlag.Spell, 0, {tag})` passes the flags to
  `makeSkillDataMod`'s varargs, so `mod[1] = 2`, `mod[2] = 0`, `mod[3] = tag`).
- `Tag.ramp: Option<Ramp>` where `Ramp = Points([[f64;2]]) | Flat([f64])`: DistanceRamp uses point
  lists, MeleeProximity a flat `{ max, min }` pair (ShieldCrush statMap).
- `Tag.excludeItemType` (DisablesItem tags).
- Numeric tag keys can hold Lua strings (`div = "10"`, 17×; `limit`, `percent`; `modLib.parseTags`
  makes every value but `threshold` a string): the field holds `tonumber(s)` and `extra` keeps the
  string under the same key (equality tests in Lua compare the string).
- `flags` given as a table: `makeSkillMod(name, type, value, flags, …)` called as
  `mod("AvoidStun", "BASE", 100, {tag})` (SkillStatMap.lua:509, sup_str.lua:584) stores the tag as
  `flags`; it is kept in `Mod.extra["flags"]` (`flags = 0`), and MS-A must raise where Lua's
  `band` raises.
- Holey tags (`mod[2] = tag`, `mod[1] = nil`; ModCache, parse oracle) are `Mod.extra["#2"]`.
- `JewelFuncSpec` gains `radius` (ModParser.lua:2622/2630 `{ radius = …, type = "Other", func = … }`).

## 7. Data schema (§4.4)

- `SkillPart { name, flags: Vec<(Name, bool)> }`: a part's other keys are boolean skill flags
  (CalcActiveSkill.lua:257-263 copies `true`/`false` into `skillFlags`).
- `SkillLevel.extra` holds exactly the census keys (`SkillLevel::EXTRA_KEYS`); others fail the load.
- `StatMapEntry.skillFlag` (data.skillStatMap, 2 entries).
- `Data.gemForSkill: AHashMap<Name, Name>` (skill id → **gem id**) and
  `gemForBaseName: AHashMap<AStr, Name>`: the Lua values are gem ids, not gem tables
  (`data.gems[data.gemForSkill[ge]]`); `Data::gem_for_skill` does the double lookup.
- `Data.skills` is an `IndexMap` (dump order) rather than an `AHashMap`.
- `data.bin` is postcard of the dump tree (roots + ref targets), and `Data` is deserialised from it
  at load (≈0.27 s release); typed `Data` is not serialised.
- Roots not read by the calc are kept untyped in `Data.other` (35 roots; listed in contracts.md).

## 8. ModParser (§6)

`ModSpec` gains the keys the driver reads that §6 does not list: `onlyAddToBanners`, `actorEnemy`,
`playerTag`, `playerTagList` (ModParser.lua:6934-6983), and `entries` for the array part of a
`modExtraTags` table (6888-6907). `scan` returns `(&'static T, String, Caps)`.

## 8b. CCR-0006 adopted

U0 adopted every type-level item of CCR-0006 (T1): `ModSpec.{playerTag, playerTagList, actorEnemy,
onlyAddToBanners, modNameIsTable}`, `TagType::Global`, `Tag.excludeItemType`,
`AilmentData.default`, the `From`/`IntoTagField` impls, and the MP0 helper signatures of its §6
(`TriggerExtraSkillOptions`, `LuaArg`, `gemIdLookup`, `dmgTypes`, `r#mod`, `level: Option<f64>`).
The lua2rs output compiles against the real crates.

## 9. Stage ids (§9.2)

Line probes before a split function's final `end` are `<fn>:end`; `<fn>:in`/`<fn>:out` are the
patcher's function probes (a line probe named `<fn>:out` collides with them).

## Proposed doc diff

Apply items 1-9 to the named sections (the text above is the replacement wording).
