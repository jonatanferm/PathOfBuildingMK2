# CCR-0011 (U0): corrections to the calc doc's contract text, as frozen by U0

**Raised by:** U0 (skeleton). **Affects:** calc-architecture.md §3.2, §4.3, §4.4, §5.1-§5.4, §5.7,
§5.10, §6, §7.2, AMENDMENTS A1. **Status:** implemented in U0's frozen files (they are the contract); this CCR asks
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
- **Calc-read roots are typed** (`schema/item_data.rs`, `Data` fields of the same name). The first
  U0 draft kept 35 roots untyped in `Data.other` and claimed they were "not read by the calc"; that
  was false for these 15, each read by in-scope Lua (scope.toml):

  | root | type | read at | unit |
  |---|---|---|---|
  | `enchantments` (§4.4 `pub enchantments: Enchantments`) | `Enchantments` (slot → key → `EnchantmentList::{Lines, BySource}`; `UtilityFlask` and `Flask` share the `Arc`) | Item.lua:1043-1048 | IT-A |
  | `foulbornMap` | `FoulbornMap` (title → `FoulbornLines`; the dkjson `$meta` is checked) | Item.lua:1028, 1553 | IT-A |
  | `rareLikeUniques` | `RareLikeUniques` (`RareLikeUnique { affixes: AffixTable, prefixLimit, suffixLimit, validBases, ignoreModType, allowDuplicateGroups, supportsCustomModifiers }`) | Item.lua:1036-1038, 1493-1495, 2059, 2719-2721 | IT-A, IT-C |
  | `itemTagSpecial`, `itemTagSpecialExclusionPattern` | `ItemTagSpecial` (tag → slot → patterns) | Item.lua:164-324 | IT-A |
  | `casterTagCrucibleUniques`, `minionTagCrucibleUniques` | `NameSet` | Item.lua:2713, 2716 | IT-C |
  | `abyssNotableNames` | `AHashMap<Name, AStr>` | PassiveTree.lua:728 | PT |
  | `modScalability` | `ModScalabilityMap` (line → `[ModScalability { isScalable, formats }]`; `""` → `[]` is kept: truthy in Lua) | ItemTools.lua:146-191 | IT-C |
  | `bossStats` | `BossStats` | ConfigOptions.lua:2185-2220 | CO |
  | `gemVaalGemIdForBaseGemId` | `AHashMap<Name, Name>` | CalcTools.lua:287 | CT |
  | `gemsByGameId` | `GemsByGameId` (the `data.gems` `Arc`s) | SkillsTab.lua:466 | BM-B |
  | `powerStatList` | `PowerStatList { stats: Vec<PowerStat>, GetFromOutput: LuaFnRef }` | CalcsTab.lua:743-744 | PWR |
  | `nodeIDList` | `NodeIdList { size, sizeNotable, localIdToGlobalId, nodes }` | DataLegionLookUpTableHelper.lua:13-252 | LUT |
  | `monsterExperienceLevelMap` | `Vec<f64>` | Build.lua:236 (outside the calc scope; typed anyway) | — |

  `powerStatList.GetFromOutput` (Data.lua:186-212) and the entries' `transform` closures (Data.lua:131,
  164-168) were outside scope.toml although `CalculatePowerStat` calls them; U0 added those ranges
  (owner PWR) and stubs `power::GetFromOutput` / `power::powerStatTransform`.
- The 20 roots left in `Data.other` (`data::DYNAMIC_ROOTS`) are read only by UI/trade code outside
  scope.toml; contracts.md#schema names the reader of each. `global` is the exception: its calc
  members are P1's generated code (crates/pob-dump/generated/global.rs).
- `TimelessMeta` keeps `timelessJewelTypes`, `timelessJewelSeedMin/Max`, `timelessJewelTradeIDs`,
  `timelessJewelLUTs` and `legionPassives` as `DataValue` (typed roots, untyped values). LUT owns
  those reads: it either reads them raw or files a CCR that types them.

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

## 10. `Table::insert` / `Table::remove` follow LuaJIT 2.1 (§5.1)

§5.1 froze `insert(&self, pos: usize, v)` / `remove(&self, pos: Option<usize>)`, and the first U0
draft raised "bad argument #2 … (position out of bounds)" (Lua 5.3). LuaJIT 2.1 `lib_table.c`
(vendored luajit2/src/lib_table.c:79-104, 108-129) never raises: `table.insert(t, pos, v)` moves
`t[i] = t[i-1]` for `i = #t+1` down to `pos+1` and sets `t[pos] = v`, so a `pos > #t+1` only sets
`t[pos]` (`#t` unchanged) and `pos <= 0` shifts through the hash part; `table.remove(t, pos)` acts
only for `1 <= pos <= #t` and otherwise returns nothing. Froze `insert(&self, pos: i64, v)` and
`remove(&self, pos: Option<i64>) -> Val` with exactly that behaviour (test
`lua_table::tests::insert_remove_luajit`, vectors from `luajit -e`). `Shared<Vec<T>>` (pob-lua,
hole-free list contract) keeps its own documented error for positions that would make a hole.

```diff
-    pub fn insert(&self, pos: usize, v: impl Into<Val>);
-    pub fn remove(&self, pos: Option<usize>) -> Val;
+    pub fn insert(&self, pos: i64, v: impl Into<Val>);      // LuaJIT lib_table.c: no bounds error
+    pub fn remove(&self, pos: Option<i64>) -> Val;          // nil unless 1 <= pos <= #t
```

## 11. Legacy Mod / pattern copies stay in pob-oracle (AMENDMENTS A1)

A1 moves `modifier.rs`/`modifier_serde.rs` to pob-gamedata and `lua_pattern.rs` to pob-lua. U0 did
that (pob-gamedata `Mod`, pob-lua patterns), and also kept the old files in pob-oracle
(`legacy_modifier.rs`, `legacy_modifier_serde.rs`, `lua/lua_pattern.rs`) because the oracle still
reads and writes mods in the legacy JSON shape. Until ORC-X there are two `Mod` types and two
pattern engines; ORC-X owns the legacy → `pob_gamedata::Mod` conversion and then deletes the
legacy copies. The calc crates never use the legacy types.

## Proposed doc diff

Apply items 1-11 to the named sections (the text above is the replacement wording).
