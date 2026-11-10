# CCR-0083 (CS3): CalcSetup.lua 1451-1975 — data write, model nils, oracle model gaps

**Status: applied 2026-11-10 by CCR2 (§4 extra keys, §6, §7; calc doc §4.4, contracts.md); §1, §3, §4 typed fields, §5 need code; §2 = CCR-0030 #3. See AMENDMENTS A8.**

**Filed status: proposed (CS3, 2026-11-09).** No frozen file was changed. Every item below is worked
around inside CS3's own files (`crates/pob-calc/src/modules/calc_setup/init_env_skills.rs`,
`crates/pob-oracle/tests/CS3.rs`, `crates/pob-oracle/corpus/CS3/`), marked `CCR-0083`.

- **Raised by:** CS3
- **Affects:** calc doc §5.4 (`SkillEffect`), §5.8 (`SocketGroup`, `GemInstance`, `Build`), §9.2
  (state dump, injection); CT (`calc_tools::canGrantedEffectSupportActiveSkill`), CalcTriggers
  readers (TR-A/B), CS2 (`env.explodeSources`), ORC-X (`state_dump.lua`, `inject.rs`); CCR-0019,
  CCR-0030 #3, CCR-0074 §6
- **Pin:** 16de4b82d

## 1. `grantedEffect.fromItem = true` writes into PoB's global `data`

CalcSetup.lua:1636-1638 (`addExtraSupports`): `if value and grantedEffect then grantedEffect.fromItem
= true end` mutates `data.skills[<support>]`; the flag stays set for the rest of the process. It is
read at CalcTools.lua:95 (`canGrantedEffectSupportActiveSkill`: an item ExtraSupport does not support
an item-granted skill), CalcTriggers.lua:34, 37, 448, 1068, 1590, ModParser.lua:2043. The Rust data
is immutable (`GrantedEffect.fromItem: bool` from the dump).

Workaround: `init_env_skills` records the write in a thread-local set and exposes
`pob_calc::modules::calc_setup::init_env_skills::grantedEffect_fromItem(&GrantedEffect) -> bool`
(data flag **or** runtime flag) and `reset_grantedEffect_fromItem()`. **CT and the CalcTriggers
units must read `grantedEffect.fromItem` through it**; until they do, an ExtraSupport gem
(`Socketed gems are supported by level N X`, e.g. on uniques) wrongly supports an item-granted skill
on the same item in the Rust calc (CalcTools.lua:95 sees `false`).

**Proposed diff:** move the set and the accessor to `pob_calc::modules::calc_tools` (CT) as
`grantedEffect_fromItem`, and replace every `grantedEffect.fromItem` read in calc code with it
(the same pattern as CCR-0019's `levelCostCleared`).

## 2. `env.explodeSources` holds items and nodes (CCR-0030 #3)

CS3 reads `env.explodeSources` (1507, 1522, 1530) and stores it in `group.explodeSources` (by
identity). The typed `Field<Shared<Vec<Table>>>` cannot hold the items/nodes/`false` it contains.
Workaround: CS3 reads the Lua list from `env.extra["explodeSources"]` (a `Table` of
`Val::Obj(ItemRef)` / `Val::Obj(NodeRef)` / `false`) when present, else the typed list; the explode
gem key is `explodeSource.modSource or explodeSource.id` read from the item / node / table.
**CS2 should write the Lua list into `env.extra["explodeSources"]`** until CCR-0030 #3 types it.

## 3. Nil model numbers CS3 creates

The groups CS3 creates (1480, 1518) have no `mainActiveSkill` / `mainActiveSkillCalcs` (Lua nil,
read as `or 1` at 1883/1886), and `build.mainSocketGroup` is nil before a build loads (1589). The
typed `Cell<f64>` fields cannot be nil; CS3 uses NaN for nil (the CCR-0019 convention for
`gemInstance.level`) and `or 1` treats NaN as nil. The literal's nil `includeInFullDPS` gets
CCR-0019's `$includeInFullDPSNil` marker. **Proposed:** `SocketGroup.mainActiveSkill`,
`mainActiveSkillCalcs: Cell<Option<f64>>`, `Build.mainSocketGroup: Cell<Option<f64>>`.

## 4. Fields CS3 writes that have no typed slot

Kept in the structs' `extra` tables (key = Lua name): `socketGroup.sourceItem`, `sourceNode`,
`displayGemList` (a `Table` of `Val::Obj(GemInstanceRef)`), `imbuedSupport` (BM-B);
`gemInstance.reqOverride`, `triggerChance`, `explodeSource`, `reqStr/Dex/Int` (BM-B),
`matchesSocket` (ItemsTab); skill effects' `socketQuality`, `enabled`; the cfg
`groupCfg.propertyModList` (the Tabulate rows as a Lua list, so `copyTable(groupCfg, true)` carries
it into every `gemCfg`). `effect.req` is read from `effect.extra["req"]` (CCR-0074 §6).
Typed booleans copied from `grantedSkill` (`noSupports`, `triggered`) turn a nil into `false`.
**Proposed:** typed fields for the ones calc code reads (`sourceItem`, `displayGemList`,
`reqOverride`, `explodeSource`); the rest may stay in `extra`.

## 5. The oracle has no build model and no item sockets (ORC-X)

* A pob-state dump writes the build as `{"$object":"Build"}` (state_dump.lua 388-401), so
  `build.skillsTab.socketGroupList`, `build.mainSocketGroup`, `itemsTab.slots[*].weaponSet`,
  `itemsTab.activeItemSet`, `skillsTab.imbuedSupportBySlot`/`displayGroup` — everything CS3 reads
  and rewrites in the model — are missing from L4 inputs and outputs, and BM-A's loader is not
  ported. `item.sockets` is not in `itemFields` (state_dump.lua:673), so `socketedIn`
  (1700/1771) is always nil after injection.
* A table reached only through dynamic contexts (a local) is injected as a `Table` even when the
  same table is a socket group / gem instance elsewhere in the model, and a table first reached
  dynamically and then through a typed member (`effect.srcInstance`) can become two objects.

CS3's test works around all three without touching ORC-X files: it patches PoB with its own
`corpus/CS3/stages.toml` and adds one entry, `cs3Model` (`corpus/CS3/model.lua`: socket group
list, main socket group, weapon sets, `useSecondWeaponSet`, imbued supports, display group, the
env items' `sockets`), to the two probes' local tables; before injecting it adds a local
`cs3GroupKinds` of stand-in active skills (`$uuid`) whose `socketGroup` refers to each model group,
so ORC-X's kind pass types the groups (and through them the gems and their effects).

**Proposed (ORC-X):** a `model` member in pob-state/1 (the build's skills tab, item slots, active item
set, main socket group), `sockets` in `itemFields`, and kind hints for locals.

## 6. Export representation (for the record)

The Rust exporter writes an immutable data value (`Val::Data`, e.g. a GemProperty row's `value`,
which in Lua *is* the mod's own value table and therefore shared by every row and env) in full at
every occurrence; PoB writes it once and `$ref`s it. CS3's L4 test accepts such a difference
when the referenced tables are equal ("value-identity").

## 7. Found by the L4 test outside CS3's range (for the owners)

* **CalcPerform.lua:2382 mutates a shared mod (§3.6 audit addition, PF-E).**
  `warcryBuff[1].warcryPowerBonus = …` writes into the first tag of a warcry buff mod. The mod comes
  from the skill's stat map, which `mergeLevelCache` (CalcActiveSkill.lua:21) shares across envs,
  so every later env's buff list in PoB carries the earlier perform's value. CS3's test drops the
  key from PoB's dump before diffing (CS3 never writes it). PF-E must port the write as
  "copy, then insert" only if the copy is what later readers see; otherwise the cross-env leak is
  PoB behaviour that a fresh Rust env will not reproduce.
* **Minion actor representation (AS, CCR-0030 #2 / CCR-0040)** and the minion's item set
  (`build.itemsTab.itemSets`, not in any dump; BM-A) show up in every build with minion skills; the
  test accepts them as AS's own test does.
* **Buff list order (AS).** `activeSkill.buffList` (and the mods inside a buff) follow the skill
  mod list order, which follows PoB's per-process stat-map `pairs` order; accepted as a permutation.
