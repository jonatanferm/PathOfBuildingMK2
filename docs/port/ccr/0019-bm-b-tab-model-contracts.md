# CCR-0019: BM-B tab model contracts (skills, config, party)

**Status: applied 2026-11-10 by CCR2 (§1, §3, §8, §9, §11, §12; §10 superseded by CCR-0080); §2, §4-§7 need code. See AMENDMENTS A8.**

**Unit:** BM-B (calc doc §11.1). **Filed status:** proposed; worked around locally in the BM-B files
(`crates/pob-calc/src/model/{skills_tab,party_tab,config_tab}.rs`, marked `CCR-0019`).
**Number:** 0014 was taken by several in-flight units (CT, MS-A, MP0, HYG1); the integrator may
renumber.

Every item cites the Lua at the pin (16de4b82d) and gives the proposed contract change. Items 3
and 8-11 are conventions BM-B established that other units (CS2/CS3, PF-F, OF-C, CO-C, IT-C, the UI
units U30/U42/U45) must follow; they need no frozen-file change but should be recorded in
`docs/port/contracts.md#model-census`.

## 1. Lua ranges BM-B needs outside its unit row

The unit's ranges (SkillsTab 441-630, 1129-1274, 1478-1502; PartyTab 516-669, 718-1042; ConfigTab
839-1129) call model code that the ui doc assigns to the UI units. BM-B ported the model halves,
with their own `@pob` markers (none is in `scope.toml`, so coverage is unchanged):

| Lua | Why BM-B needs it | Rust |
|---|---|---|
| SkillsTab.lua:323-349 (`imbuedSupport` GemSelectControl change function) | LoadSkill:458 calls `self.controls.imbuedSupport.gemChangeFunc(…)` | `SkillsTab::imbuedSupport_gemChangeFunc` |
| SkillsTab.lua:1103-1127 `FindSkillGem` | ProcessSocketGroup:1196 | `SkillsTab::FindSkillGem` |
| SkillsTab.lua:1275-1278 (`SetDisplayGroup`, model half) | SetActiveSkillSet:1500 | `SkillsTab::SetDisplayGroup` |
| SkillsTab.lua:1452-1462 `NewSkillSet` | Load:555/561, SetActiveSkillSet:1482 | `SkillsTab::NewSkillSet` |
| SkillsTab.lua:1464-1475 `RebuildImbuedSupportBySlot` | SetActiveSkillSet:1494 | `SkillsTab::RebuildImbuedSupportBySlot` |
| ConfigTab.lua:683-692 (`self.defaultState`, constructor) | GetDefaultState:946 | `config_tab::defaultState(data)` |
| ConfigTab.lua:1280-1300 `NewConfigSet` | Load:886/891/905 | `ConfigTab::NewConfigSet` |
| ConfigTab.lua:1337-1362 `SetActiveConfigSet` (model half) | Load:937 | `ConfigTab::SetActiveConfigSet` |
| PartyTab.lua:22-36 (constructor, model half) | Load/Save/ParseBuffs read `self.actor.Aura`, `lastContent`, … | `PartyTab::ctor_model` |

**Proposed diff:** calc doc §11.1 BM-B row, add "SkillsTab.lua 323-349, 1103-1127, 1275-1278,
1452-1475; ConfigTab.lua 683-692, 1280-1300, 1337-1362; PartyTab.lua 22-36 (model halves)";
ui doc §10.2 table: U30/U42/U45 keep the UI statements of those functions and call the model halves.

## 2. `SkillsTab` has no field for the saved UI settings

SkillsTab.lua:538-549 (Load) and 574-582 (Save) read and write `self.sortGemsByDPS`,
`self.sortGemsByDPSField`, `self.showSupportGemTypes`, `self.showLegacyGems`, and Load goes through
three dropdowns whose `selIndex` survives a failed `SelByValue` (DropDownControl.lua:136-150), so a
second Load keeps the previous selection. The frozen struct (calc doc §5.8) has none of these.

**Proposed diff** (`model/mod.rs`):
```rust
 pub struct SkillsTab {
     …
+    pub sortGemsByDPS: Cell<bool>, pub sortGemsByDPSField: Field<Name>,
+    pub showSupportGemTypes: Field<Name>, pub showLegacyGems: Cell<bool>,
+    pub extra: Table,   // dropdown selections (defaultLevel/showSupportGemTypes/sortGemsByDPSField selIndex)
 }
```
**Workaround:** `SkillsTab::ui_state()`, a thread-local side table keyed by the tab's address
(validated by identity of the tab's `imbuedSupportBySlot` table).

## 3. `PartyTab::ParseBuffs` signature

PartyTab.lua:718 `ParseBuffs(list, buf, buffType, label)`: `list` is a mod store for
`PartyMemberStats`/`EnemyConditions`/`EnemyMods` (`self.actor.modDB`, `self.enemyModList`; 523/538/541
call `list:NewMod`/`list:AddMod`, 722/738/789) and a plain table for the buff types
(`self.actor.Aura`, …; 806/842 index it); `label` is a label control (`label.label = …`, 747/900/953)
or, for `PartyMemberStats`, `self.actor.output` (523, written at 770-786). The frozen
`(list: &Table, …, label: Option<&str>)` cannot express this.

**Applied in the owned file** (no other code calls it yet):
```rust
-pub fn ParseBuffs(&self, build: &BuildRef, list: &Table, buf: &str, buffType: Name, label: Option<&str>)
+pub enum BuffList<'a> { Store(&'a ModStoreRef), Table(&'a Table) }
+pub fn ParseBuffs(&self, build: &BuildRef, list: BuffList<'_>, buf: &str, buffType: Name, label: Option<&Table>)
```

## 4. `ConfigTab.input` / `placeholder` are reassigned

ConfigTab.lua:1352-1353 `self.input = self.configSets[configSetId].input` (and `placeholder`):
`build.configTab.input` must alias the active set's table (the UI writes
`configSets[active].input[var]`, CalcSetup.lua:496 reads `build.configTab.input`). The frozen fields
are plain `Table`s.

**Proposed diff:** `model/mod.rs` `pub input: Field<Table>, pub placeholder: Field<Table>`;
`env.rs:130-131` `build.configTab.input.get()` / `.placeholder.get()`.
**Workaround:** `ConfigTab::bind_active_set`: `self.input` *is* the active set's `input` table;
switching sets gives the previously active set a copy of its contents and moves the new set's
contents into `self.input`. Observable only through a table kept across a switch (an env built
before it).

## 5. `ConfigTab.defaultState`

ConfigTab.lua:683-692 fills `self.defaultState` in the constructor; GetDefaultState:946 reads it.
**Proposed diff:** `pub defaultState: Table` on `ConfigTab`, filled by the constructor's model half.
**Workaround:** `config_tab::defaultState(data)` recomputes it from `data.configOptions` (cached per
thread); the oracle test compares it with PoB's table on every corpus build.

## 6. Lua `nil` in non-`Option` fields

| Lua | nil when | Proposed | Workaround |
|---|---|---|---|
| `socketGroup.label` (SkillsTab.lua:450) | no `label` attribute; Save then omits it (593) | `label: Field<Option<LStr>>` | `""` + `extra["$labelNil"] = true` (`skills_tab::socketGroup_label`) |
| `socketGroup.includeInFullDPS` (448) | no attribute; Save writes `tostring(nil)` = `"nil"` (591) | `Cell<Option<bool>>` | `false` + `extra["$includeInFullDPSNil"]` (`socketGroup_includeInFullDPS`) |
| `gemInstance.level`, `.quality` (495-496) | no attribute (Save writes `"nil"`, 606-607) | `Cell<Option<f64>>` | NaN |
| `gemInstance.nameSpec` (485, `gemData.nameSpec` is not a gem key; `sanitiseText(nil)` = nil) | modern-format load | none needed | `""` (ProcessSocketGroup:1166 `or ""` runs before any read) |

## 7. `item.sockets.colourAlwaysMatches`

Item.lua:2689 sets a string key on the sockets list; SkillsTab.lua:1260 reads it. `Item.sockets` is
`Field<Shared<Vec<Table>>>`. **Proposed:** `Item.socketsColourAlwaysMatches: Cell<bool>` (IT-C writes
it). **Convention used now:** `item.extra.colourAlwaysMatches`.

## 8. A write into `data`: triggered item skills' level cost

SkillsTab.lua:1188-1190: for a triggered gem instance specified by skill id,
`gemInstance.grantedEffect.levels[gemInstance.level].cost = {}`: PoB empties the cost of that
granted effect level in its global `data`, for every later use in the process. `pob_gamedata::Data`
is immutable. **Contract:** BM-B records the write; `pob_calc::model::skills_tab::levelCostCleared(id,
level)` is true afterwards, and every reader of `grantedEffectLevel.cost` (CalcActiveSkill/CalcOffence
costs, OF-C; CS3) must treat the cost as `{}` then.

## 9. Representations other units read

- `skillsTab.skillSets[id]` is a table `{ id, title, socketGroupList }` whose `socketGroupList` is
  `Val::Obj(Rc<Shared<Vec<SocketGroupRef>>>)` (`skills_tab::skillSet_socketGroupList`).
- `skillsTab.imbuedSupportBySlot[slot]` is `Val::Obj(Rc<GrantedEffectRef>)` (read by CalcSetup.lua:1660, CS3).
- `socketGroup.extra.imbuedSupport`; `gemInstance.extra.{color, errMsg, new, naturalMaxLevel,
  reqStr, reqDex, reqInt, matchesSocket}`.
- Party: `partyTab.actor.extra.{Aura, Curse, Warcry, Link}` (each `{ [type] = { [name] = entry,
  Vaal = { … } }, limit = n }`), an entry is `{ modList = Val::Obj(Rc<ModStore>) (ModList),
  effectMult, isMark }` (`party_tab::buff_modList` also accepts an array table of `Val::Mod`, the
  shape CalcPerform builds for some `buffExports`); `partyTab.extra.{controls, lastContent}`.
- Config: `configSets[id] = { id, title, input, placeholder, customModsList }`.

## 10. Constructors' model halves are not run by `Build::new`

`Build::new` (U0, `model/mod.rs`) leaves `configTab.configSets` empty (Lua: ConfigTab.lua:145-148
creates set 1 and activates it), `skillsTab.defaultGemLevel = None` (SkillsTab.lua:95:
`"normalMaximum"`; `defaultGemQuality = main.defaultGemQuality`, 96) and the party actor's buff
tables absent (PartyTab.lua:22). Every BM-B entry point that needs them creates them (Load always
does; `PartyTab::ctor_model` is idempotent), but a build that skips a section's Load differs.
**Proposed:** `Build::new` calls `configTab.NewConfigSet(Some(1.0), None)` +
`SetActiveConfigSet(…, Some(1.0), true)`, `partyTab.ctor_model()`, and sets
`skillsTab.defaultGemLevel = "normalMaximum"` (BM-A owns `Build:Init`).

## 11. Lua behaviour the port reproduces (not contract changes)

- SkillsTab.lua:1182 indexes `data.gemForSkill` (keyed by granted-effect *tables*, Data.lua:1094)
  with the skill id *string*: always nil, so a gem specified by skill id alone always gets
  `grantedEffect = data.skills[id]`, never `gemData`.
- SkillsTab.lua:474-479 (old format, no `variantId`): Vaal gems and their synthesised `AltX`
  variants share `gameId` and `grantedEffectId`, so the record picked depends on LuaJIT's per-process
  `pairs` order (corpus: `upstream_Dual_Wield_Cospris_CoC`, Vaal Righteous Fire). The port takes the
  first in data order (§3.4); the BM-B test tolerates either. Candidate for the nondet allow-list
  (§9.5).
- ConfigTab.lua:876: a `<Placeholder string=…>` is stored in `input`, not `placeholder`.
- ConfigTab.lua:906: `configSetOrderList[index]` uses the child's position among *all* children, so a
  config with non-ConfigSet children before a ConfigSet gets holes; the port keeps what `ipairs` sees.
- PartyTab.lua:919: `aura` is the undeclared global (CCR-0004), the test is always true.
- `exportBuffs`/`ParseBuffs` labels and `buffExports` iterate with `pairs`: PoB's text order is random
  per process (the test compares them as multisets).

## 12. Seen while testing (other units)

- `pob_gamedata::Tag` has no `warcryPowerBonus` (CalcPerform.lua Warcry `GlobalEffect` tags); MS-A's
  CCR-0014 reports it. The BM-B export test strips it from injected mods and counts those exports as
  pending.
- CT changed `validateGemLevel` to a generic `<G: GemLevelTarget>`; BM-B calls it with `&GemInstance`,
  which compiles against both the frozen and the CT signature.
