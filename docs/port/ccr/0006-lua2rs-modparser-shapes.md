# CCR-0006: types and helper signatures that ModParser/ConfigOptions ports need (from T1 lua2rs)

- **Raised by:** T1 (`scripts/port/lua2rs/`)
- **Affects:** calc-architecture.md §4.3 (`Tag`, `TagType`, `Value` conversions), §4.4 (`AilmentData`),
  §6 (`ModSpec`, `Caps` semantics, MP0 `helpers.rs` signatures). Consumers: U0, MP0-MP6, CO-A/B.
- **Pin:** 16de4b82d

lua2rs emits Rust against the doc's code listings. Translating every list entry and every
`apply` closure surfaced the gaps below. Each is resolved in lua2rs's default output shape
(`scripts/port/lua2rs/shape.py`) and in its compile-check stubs
(`scripts/port/lua2rs/check/src/{gamedata,modparser}.rs`, marked `CCR-0006`), so U0 can adopt,
rename or reject each item and re-run `python3 scripts/port/lua2rs check` with a `--shape` file.

## 1. `ModSpec` lacks four keys the parseMod driver reads

ModParser.lua:6876-6990 copies every key of the matched spec tables into `misc` and reads:

| key | set at | read at |
|---|---|---|
| `playerTag` (a tag) | 1133, 1134, 1137, 1269 | 6959, 6976 |
| `playerTagList` (tags) | 1263, 1266 | 6961-6963, 6977-6979 |
| `actorEnemy` (bool) | 1283 | 6983 |
| `onlyAddToBanners` (bool) | 1292 | 6935 |

`playerTag`/`playerTagList` are tags, which `ModSpec.extra: Vec<(Name, Value)>` cannot hold
(`Value` has no tag variant).

```diff
 pub struct ModSpec {
     pub flags: u32, pub keywordFlags: u32, pub tag: Option<Tag>, pub tagList: Vec<Tag>,
     ...
     pub extra: Vec<(Name, Value)>,
+    pub playerTag: Option<Tag>, pub playerTagList: Vec<Tag>,
+    pub actorEnemy: bool, pub onlyAddToBanners: bool,
 }
```

## 2. `TagType` lacks `Global`; `Tag` lacks `excludeItemType`

- `{ type = "Global" }`: ModParser.lua:427 (modNameList), 1087 (modFlagList), 2410 and 5013
  (specialModList). No EvalMod branch reads it (it is inert in the calc), but the mods carry it,
  so ModCache/parse-oracle comparisons and `formatTag` see it. `TagType::Other(Name)` cannot
  be written with the §4.3 `tag!` macro (`$t:ident`).
- `excludeItemType`: set at ModParser.lua:5256 (`{ type = "DisablesItem", slotName = "Flask 5",
  excludeItemType = "Tincture" }`), read at CalcSetup.lua:952 and 970.

```diff
 pub enum TagType { ..., GlobalEffect, InSlot, SlotNumber, DisablesItem, IgnoreCond,
+    Global,
     Other(Name) }
 pub struct Tag { ...
+    pub excludeItemType: Option<Name>,
     pub extra: Option<Box<ValueTable>>, }
```

Also (no proposal, recorded for U0/MS-B): `Tag.ramp: Option<Arc<[[f64; 2]]>>` fits
`DistanceRamp` (`ramp = {{35,0},{70,1}}`) but not `MeleeProximity`, whose ramp is one flat pair
`{MaxBonusPct, MinBonusPct}` (ModParser.lua:3005; read as `tag.ramp[1]`/`tag.ramp[2]` at
ModStore.lua:615-630). lua2rs leaves that entry for hand-porting.

## 3. `AilmentData.default`

`data.nonDamagingAilment[k].default` is read at ModParser.lua:2942-2947, 3736 and
3769-3793 (17 specialModList entries). Data.lua:424-431 defines it per ailment, `nil` for
Freeze, so the schema field is `pub default: Option<f64>` (verbatim key; `default` is a
legal Rust identifier).

## 4. Conversions the generated code relies on (§4.3 lists the traits, not the impls)

- `impl From<T> for Value` for `f64`, `Option<f64>` (None -> Nil), `bool`, `&str`, `Name`,
  `Option<Name>` (None -> Nil), and `Mod` (-> `Value::Mod(Arc::new(m))`, the `{ mod = ... }` payloads).
- `IntoTagField`: `&str`/`Name`/`Option<Name>` -> `Option<Name>`; `&str` -> `Option<AStr>`;
  `f64`/`Option<f64>` -> `Option<f64>`; `bool` -> `bool` and `Option<bool>`; `u32` -> `u32`;
  `SkillType::T` -> `Option<SkillType::T>`; `&[Name]`, `&[f64]`, `&[SkillType::T]`, `&[[f64; 2]]`
  -> the matching `Option<Arc<[_]>>`; `Sockets` -> `Option<Sockets>`.

## 5. `Caps`: the three call shapes (clarification of §6, no type change)

§6 says "#1 = num (tonumber(cap1)), #k>=2 = caps[k-2]". That is the specialModList shape only:

| list | Lua call | param 1 | param k>=2 |
|---|---|---|---|
| specialModList | `f(tonumber(cap[1]), unpack(cap))` (6662-6663) | `caps.num` | `caps.cap[k-2]` |
| preFlagList | `f(unpack(preFlagCap))` (6680-6682) | `caps.cap[0]` (a string) | `caps.cap[k-1]` |
| modTagList | `f(tonumber(c1), ...)` if `c1:match("%d+")` else `f(c1, ...)` (6698-6702, 6706-6712) | `caps.num` or `caps.cap[0]` | `caps.cap[k-2]` |

MP0's `scan` must fill `caps.num = tonumber(cap1)` for specialModList and, for modTagList,
whenever cap1 contains a digit. lua2rs types param 1 of a modTagList closure from the
pattern (`lpat.first_capture_digits`); the 3 closures whose first capture may or may not
contain digits (1401, 2001, 2007) are left for hand-porting.

## 6. MP0 `helpers.rs` signatures used by the generated lists

The Lua helpers take dynamically typed arguments; the generated calls use these signatures
(argument conversion happens at the call site, e.g. `tonumber(level)` inside the helper is
reproduced by passing `Option<f64>`):

```rust
pub use pob_gamedata::mod_tools::createMod as r#mod;             // ModParser.lua:2011
pub fn flag(name: Name, source: Option<&str>, flags: u32, keywordFlags: u32, tags: &[Tag]) -> Mod;   // 2012-2014
#[derive(Default)] pub struct TriggerExtraSkillOptions {          // 2055-2072 `options`
    pub noSupports: Option<bool>, pub sourceSkill: Option<String>, pub triggerChance: Option<f64>,
    pub ignoreHexproof: bool, pub onCrit: bool }
pub fn triggerExtraSkill(name: &str, level: Option<f64>, options: Option<TriggerExtraSkillOptions>) -> Vec<Mod>;
pub fn grantedExtraSkill(name: &str, level: Option<f64>, noSupports: Option<bool>) -> Option<Vec<Mod>>;   // 2047-2054
pub fn extraSupport(name: &str, level: Option<f64>, slot: Option<&str>, appliesToGrantedSkills: Option<bool>) -> Option<Vec<Mod>>; // 2073-2107
pub enum LuaArg<'a> { Nil, Num(f64), Str(&'a str) }             // explodeFunc's `amount` is used as number and string
pub fn explodeFunc(chance: Option<f64>, amount: LuaArg, r#type: &str, tags: &[Tag]) -> Option<Vec<Mod>>; // 2108-2120
pub fn dealNoNonDamageType(dmgType: &str, forMinion: bool) -> Option<Vec<Mod>>;   // 2018-2032
pub fn gemIdLookup(name: &str) -> Option<Name>;                  // the gemIdLookup table, 2033-2046
pub fn dmgTypes(name: &str) -> Option<Name>;                     // the dmgTypes table, 2121-2127
```

`mod(...)`/`flag(...)` calls are emitted with createMod's argument dispatch (ModTools.lua:36-51)
already resolved, so `r#mod` is exactly §4.3 `createMod`.

## 7. Notes (no change requested)

- Registry ids: the dump spells config ids `ConfigOptions[<i>].apply` (AMENDMENTS A2,
  `closures.json`), not `configOptions.<var>.apply` (§8.1). lua2rs emits the dump ids and the
  dump's sha256 hashes (verified equal for all 531 inline closures).
- `ModStore::NewMod` takes `source: &str`, but 26 ConfigOptions applies call `NewMod` without a
  source. ModStore.lua:109-111 defines `NewMod(...)` as `AddMod(createMod(...))`, so lua2rs emits
  `modList.AddMod(Arc::new(createMod(..., None, ...)))` for those; no contract change needed.
