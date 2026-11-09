# CCR-0040: AS — contract gaps met porting CalcActiveSkill.lua

Status: proposed by AS (2026-11-09)

AS ported `Modules/CalcActiveSkill.lua` 1-934 into
`crates/pob-calc/src/modules/calc_active_skill.rs`. The frozen types below cannot hold what the
Lua stores, or the ModStore API lacks an operation the Lua performs. Each site is worked around
locally and marked `// CCR-0040` (or `// CCR-0030` where ORC-X's CCR-0030 already covers it).

## 1. Minion actor fields with no typed slot (`actor.rs`)

`buildActiveSkillModList` builds the minion as a fresh table (CalcActiveSkill.lua:703-792) and
sets keys `Actor` does not have:

| Lua key | Value | Evidence (write / read) | Workaround |
|---|---|---|---|
| `minion.type` | the minion id (string) | 707 / 719, 753, 891; CalcPerform.lua:1205 | `minion.extra["type"]` (string) |
| `minion.lifeTable` | one of `data.monsterLifeTable{,2,3}` / `monsterAllyLifeTable` (shared data table) | 724-727 / CalcPerform.lua:1160, 1166 | `minion.extra["lifeTable"] = Val::Obj(LifeTable(&'static [f64]))` (`pub struct LifeTable` in calc_active_skill.rs) |
| `minion.itemSet` | an item set table of `build.itemsTab.itemSets` | 740, 745 / 768-779; CalcPerform.lua:1222 | `minion.extra["itemSet"]` (the `Table`) |
| `minion.uses` | `activeGrantedEffect.minionUses` (CCR-0030 #2) | 723 / 766-777; CalcPerform.lua:1222-1224 | `minion.extra["uses"]` = table of slot name → `true` |

Proposed diff:

```diff
 pub struct Actor {
+    pub r#type: Field<Option<Name>>,                    // minion.type
+    pub lifeTable: Cell<Option<&'static [f64]>>,        // minion.lifeTable (data identity)
+    pub itemSet: Field<Option<Table>>,                  // minion.itemSet
-    pub uses: Cell<Option<f64>>,
+    pub uses: Field<Option<pob_gamedata::NameSet>>,     // minion.uses = grantedEffect.minionUses
```

(CCR-0030 #2 proposes `Field<Option<Value>>` for `uses`; `minionUses` is loaded as a `NameSet`,
so `Option<NameSet>` keeps the data identity without a conversion.) Readers until then: PF
(CalcPerform.lua:1160-1224) reads these keys from `minion.extra`.

## 2. `SocketGroup.sourceItem` (`model/mod.rs`)

CalcSetup.lua:1486 sets `group.sourceItem = grantedSkill.sourceItem` (an item);
CalcActiveSkill.lua:290 and 504 read it. `SocketGroup` has no field; AS reads
`socketGroup.extra["sourceItem"]` (truthiness / non-nil only).

```diff
 pub struct SocketGroup {
+    pub sourceItem: Field<Option<ItemRef>>,
```

## 3. `GrantedEffect.minionList`: nil vs `{ }` (`pob-gamedata/src/schema/skills.rs`)

CalcActiveSkill.lua:671-679 distinguishes `minionList == nil` (no minions) from `minionList = { }`
(Raise Spectre: use `build.spectreList`, `isSpectre = true`). The schema's
`#[serde(default)] minionList: Vec<Name>` loads both as an empty `Vec`. The dump keeps the
difference (`skills.json`: `"minionList": {}` on exactly `RaiseSpectre`, `RaiseSpectreAltX`).
Workaround: `calc_active_skill::minionList_is_set` (non-empty, or one of those two ids);
`tests/AS.rs::as_census` checks it against the dump for every skill.

```diff
-    #[serde(default)]
-    pub minionList: Vec<Name>,
+    #[serde(default)]
+    pub minionList: Option<Vec<Name>>,
```

## 4. Positional ModList access (`classes/mod_list.rs`, MS-A)

The global-effect split (CalcActiveSkill.lua:805-869) walks the skill mod list by index
(`while skillModList[i] do … t_remove(skillModList, i)`). The ModStore API has `len`/`ipairs`
but no `self[i]` read and no `table.remove`. AS implements both as private helpers over
`ModStore::mods` (`pub(crate)`), keeping MS-A's name → positions index consistent
(`list_at`, `list_remove`). Proposed (MS-A's file):

```diff
 impl ModStore {
+    /// `self[i]` of a ModList (1-based; nil → None).
+    pub fn list_at(&self, i1: usize) -> Option<ModRef>;
+    /// `t_remove(modList, i)` (LuaJIT table.remove: nothing outside 1..=#t).
+    pub fn list_remove(&self, i1: usize) -> Option<ModRef>;
```

## 5. `getWeaponFlags` signature (own file, already changed)

U0's stub took `weaponTypes: Option<&pob_gamedata::NameSet>`; the Lua passes a **list** of
weapon-type sets (`{ activeGrantedEffect.weaponTypes, <support weaponTypes>… }`,
CalcActiveSkill.lua:296-300, iterated at 201). The function is file-local (`local function`),
so AS changed it to `weaponTypes: Option<&[NameSet]>`. Doc §5 has no entry for it; nothing else
calls it.

## 6. `activeEffect.gemPropertyInfo` row shape (for CS0)

CalcSetup.lua:413-414 appends Tabulate rows (`{ value = …, mod = … }`) to
`effect.gemPropertyInfo`; CalcActiveSkill.lua:571-573 reads `.value.key`, `.value.value`,
`.mod.source`, `.mod[1]`. `SkillEffect.gemPropertyInfo` is `Field<Option<Table>>`; AS reads each
row as a `Table` with `value` = `Val::from_value(row.value)` and `mod` = `Val::Mod(row.mod)`.
CS0 must write the rows in that shape (or a typed `Shared<Vec<TabRow>>` replaces the table).

## 7. `Rc` cycle through `summonSkill` (note)

`activeSkill.minion` (strong) → `minion.activeSkillList` (strong) → each minion skill's
`summonSkill` (strong, `Field<Option<ActiveSkillRef>>`) → `activeSkill`: a cycle, which calc
doc §3.3 rule 4 forbids; every env with a minion skill leaks. Proposal: make
`ActiveSkill.summonSkill` a `Weak` (readers upgrade; the summoner always outlives its minion
skills), or have the env break the cycle when it is dropped. Not changed by AS (frozen field).

## 8. Probe harness: absent marks are not cleared by calc writes (note for ORC-X)

For function-level L4 (`buildActiveSkillModList:in#n` → run → `:out#n`), the injection marks
the skill's nil fields absent in the probe registry, and the exporter keeps honouring the mark
after the Rust code assigns the field (`extraSkillModList`, `minionList`, `weapon1Cfg`, …). A
fresh Rust object (the minion actor) exports every frozen default (`modDB`, `output`,
`activeSkillList`, `strDmgBonus`) that PoB's `minion = { }` does not have yet. `tests/AS.rs`
fixes the marks by hand (`fix_absent_marks`). A generic fix would be for `export_root` to treat
an absent mark as stale when the typed field is no longer at its injected (nil) value.
