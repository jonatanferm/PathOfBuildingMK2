# CCR-0030: ORC-X — frozen calc types that cannot hold what PoB stores

**Status: applied 2026-11-10 by RETYPE (AMENDMENTS A8 E1 approved, A10): all seven rows; `Actor.uses` is `Field<Option<NameSet>>` (CCR-0040 §1's type: every PoB value is the data's `minionUses` set). Previously: escalated by CCR2 2026-11-10.**

Filed status: proposed by ORC-X (2026-10-10)

## Problem

ORC-X injects every all57 dump at `initEnv:out`, `perform:in` and `offence:in`
into the frozen Rust types (calc doc §5.4, §5.9) and exports them back
(`crates/pob-oracle/tests/ORC-X.rs`). The round trip is exact, but only because
the probe registry (`pob_calc::probe::Registry`) and the structs' `extra` tables
carry values that a frozen typed field cannot hold. Calc code reading the typed
field sees the default instead of PoB's value, so the units that own these reads
will diverge. Counts are injection fallbacks summed over the 57 `perform:in`
dumps (`orcx_identity_round_trip_all57` prints them per stage).

| # | Field (frozen type) | What PoB stores | Evidence | Fallbacks |
|---|---|---|---|---|
| 1 | `ActiveSkill.triggeredBy: Field<Option<Table>>` | a **skill effect** (`activeSkill.triggeredBy = skillEffect`); the same table is in another skill's `supportList`/`effectList`, so a `Table` cannot alias it | CalcActiveSkill.lua:532-536; read CalcTriggers.lua:137, 144, 233, 430-434, 448, 567-572, 583, 666, 730, 890-893; written 1225 (`triggeredBy.grantedEffect = …`) | 93 |
| 2 | `Actor.uses: Cell<Option<f64>>` | a **data table**: `minion.uses = activeGrantedEffect.minionUses` (`{ ["Weapon 1"] = …, … }`) | CalcActiveSkill.lua:723, 766-777; CalcPerform.lua:1222 | 5 |
| 3 | `Env.explodeSources: Field<Shared<Vec<Table>>>` (and `SocketGroup.explodeSources: Option<Table>`, which aliases it) | **passive tree nodes and items**: `buildModListForNodeList` returns the nodes with explode mods, CalcSetup adds items | CalcSetup.lua:268-303, 540, 838, 1421-1423, 1507-1530 | 30 |
| 4 | `Item.slotModList: [Field<Option<ModList>>; 2]` | three slot lists for rings: `self.slotModList[3]` ("Ring 3") | Item.lua:2693-2700 | 113 |
| 5 | `Env.flaskSlotMap: Field<Table>` | keys are **items**: `env.flaskSlotMap[item] = flaskNum` | CalcSetup.lua:1023, 1035; read CalcPerform.lua:1710 | 258 entries |
| 6 | `Actor.appliedEnemyModifiers: Field<Option<Table>>` | keys are **mod tables**: `cache[mod] = true` | CalcPerform.lua:620-630 | 185 entries (offence:in; `table: object key` counts rows 5 + 6) |
| 7 | `ModStore` (no `extra`) | `buffs[buff.name].notBuff = true` on a ModList | CalcPerform.lua:2331, 2810, 2834; read 3346, 3425 | 7 (offence:in) |

Not a gap, recorded for the record: `item.modList` / `item.slotModList[i]`
are plain Lua lists (`return { unpack(modList) }`, Item.lua:2518), typed as
`ModList` in Rust; the exporter writes them as plain lists.

## Proposed diff (calc doc §5.4 / §5.9 and the frozen structs)

```diff
 // active_skill.rs
-    pub triggeredBy: Field<Option<Table>>,
+    pub triggeredBy: Field<Option<SkillEffectRef>>,   // CalcActiveSkill.lua:536
 // actor.rs
-    pub uses: Cell<Option<f64>>,
+    pub uses: Field<Option<pob_gamedata::Value>>,     // minionUses data table (CalcActiveSkill.lua:723)
 // env.rs
-    pub explodeSources: Field<Shared<Vec<Table>>>,
+    pub explodeSources: Field<Shared<Vec<ExplodeSource>>>,  // enum ExplodeSource { Node(NodeRef), Item(ItemRef) }
-    pub flaskSlotMap: Field<Table>,
+    pub flaskSlotMap: Field<Shared<PtrMap<Item, f64>>>,
 // model/mod.rs
-    pub explodeSources: Field<Option<Table>>,          // SocketGroup
+    pub explodeSources: Field<Option<Shared<Vec<ExplodeSource>>>>,
 // actor.rs
-    pub appliedEnemyModifiers: Field<Option<Table>>,
+    pub appliedEnemyModifiers: Field<Option<Shared<ModPtrSet>>>,   // a set of mods by Arc identity
 // classes/mod.rs (Item)
-    pub slotModList: [Field<Option<ModList>>; 2],
+    pub slotModList: [Field<Option<ModList>>; 3],
 // classes/mod.rs (ModStore)
+    pub notBuff: Cell<bool>,                           // CalcPerform.lua:2331
```

(`ModPtrSet` would be a `PtrSet`-like set over `ModRef` (`Arc`), which
`pob_lua::PtrSet` (over `Rc`) cannot hold.)

## Workaround until applied (ORC-X, marked `// CCR-0030`)

* `crates/pob-oracle/src/inject.rs`: a value a typed field rejects goes to the
  struct's `extra` table under the same key (the typed field is marked absent);
  a typed list whose elements do not fit (#3) is injected as a dynamic table;
  `slotModList[3]`, object-keyed entries (#5, #6) and mod-store fields (#7) go
  to the probe registry.
* `crates/pob-calc/src/probe.rs`: the exporter merges `extra` and the registry
  back, so `export(inject(dump)) == dump` holds exactly (170/170 dumps).

When the CCR is applied, the reflection table in `probe.rs` (`reflect!` lists)
picks the new types up through `ProbeField` impls (`SkillEffectRef`, `Value`,
`PtrMap<Item, f64>`, …), and the fallback counters of
`orcx_identity_round_trip_all57` drop to zero for the fixed rows.
