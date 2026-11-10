# CCR-0087 (TR-A): `TriggerConfig` cannot hold what the configTable arms return

**Status: escalated by CCR2 2026-11-10 (needs code; frozen `TriggerConfig`, conflicts with in-flight TR-B; AMENDMENTS A8 E2). See AMENDMENTS A8.**

**Filed status: proposed (TR-A, 2026-11-10).** No frozen file was changed. TR-A works around every item
inside its own file (`crates/pob-calc/src/modules/calc_triggers/handlers.rs`), marked `CCR-0087`.

- **Raised by:** TR-A
- **Affects:** calc doc §8.1 (`TriggerConfig`, `calc_triggers/mod.rs`, U0), TR-B (`config_table.rs`,
  which builds the configs), the TR-A stub signatures `defaultComparer` / `findTriggerSkill`
- **Pin:** 16de4b82d

## 1. `triggeredSkillCond` has no field

`defaultTriggerHandler` reads `config.triggeredSkillCond` (a `function(env, skill)`,
CalcTriggers.lua:408, 413, 418, 419, 797, 811, 814); 20 configTable arms set it (e.g. 1118, 1134,
1158, 1166, 1344). `TriggerConfig.extra: Table` cannot hold a Rust closure.

Workaround: `handlers.rs` adds `impl TriggerConfig { set_triggeredSkillCond, triggeredSkillCond }`,
storing the closure (`SkillCondFn = Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>`) boxed in
`extra["triggeredSkillCond"]` as a `Val::Obj`. **TR-B must set it through the setter.**

## 2. `comparer` has the wrong type

The frozen field is `Option<fn(&EnvRef, Name, &ActiveSkillRef, f64) -> bool>`. In Lua:

- `findTriggerSkill` calls `comparer(env, uuid, source, triggerRate)` with `source` and
  `triggerRate` nil for the first candidate (CalcTriggers.lua:83; `defaultComparer` 71 reads
  `not source` and `triggerRate or 0`);
- the arms' comparers close over locals (`requiredManaCost` 1090-1094, `requiredLifeCost`
  1104-1108) and call `defaultComparer` (1318-1322, 1354-1358).

A plain `fn` with non-optional arguments can express neither. Workaround: `set_comparer` /
`comparer_fn` store a `ComparerFn = Rc<dyn Fn(&EnvRef, Name, Option<&ActiveSkillRef>, Option<f64>)
-> bool>` in `extra["comparer"]`; the typed field must stay `None` (debug-asserted). The TR-A stub
signatures follow the Lua: `defaultComparer(env, uuid, source: Option<&ActiveSkillRef>,
triggerRate: Option<f64>)` and `findTriggerSkill(…, comparer: Option<ComparerFn>)`.

## 3. `sourceWeapon` is a flag, not a name

The arms write `sourceWeapon = true` (CalcTriggers.lua:1537, 1580); the handler only tests it
(403-405, 447). `TriggerConfig.sourceWeapon: Option<Name>` has no `true`. Workaround:
`TriggerConfig::sourceWeapon()` = typed field `is_some()` **or** `extra.sourceWeapon` truthy.

## 4. The other keys the handler reads (census, CalcTriggers.lua 394-900)

Untyped, read from `extra` under their Lua names: `triggeredSkills` (a list of
`packageSkillDataForSimulation` tables, 400), `trigRate` (401), `allowTriggered` (410),
`sourceName` (442), `ignoreSourceRate` (719), `stagesAreOverlaps` (798, 821), `overlaps` (798),
`customTriggerName` (888). Typed fields used as is: `source`, `actor`, `triggerName`,
`triggerOnUse`, `useCastRate`, `triggerChance` (written back at 762), `assumingEveryHitKills`,
`triggerSkillCond`, `customHandler`.

## 5. Handler visibility

The stubs declared the handlers and helpers `pub(crate)`; TR-A made them `pub` so the acceptance
test (`crates/pob-oracle/tests/TR-A.rs`, an external crate) can drive them before TR-B lands.

## Proposed diff (`calc_triggers/mod.rs`)

```diff
 pub struct TriggerConfig {
     pub source: Option<ActiveSkillRef>,
     pub actor: Option<ActorRef>,
     pub triggerName: Option<Name>,
     pub triggerOnUse: bool,
     pub useCastRate: bool,
     pub triggerChance: Option<f64>,
     pub assumingEveryHitKills: bool,
-    pub sourceWeapon: Option<Name>,
+    pub sourceWeapon: bool,
     pub triggerSkillCond: Option<Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>>,
+    pub triggeredSkillCond: Option<Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>>,
     pub customHandler: Option<fn(&EnvRef, &mut TriggerConfig) -> Option<Table>>,
-    pub comparer: Option<fn(&EnvRef, Name, &ActiveSkillRef, f64) -> bool>,
+    pub comparer: Option<Rc<dyn Fn(&EnvRef, Name, Option<&ActiveSkillRef>, Option<f64>) -> bool>>,
     pub extra: Table,
 }
```

With the diff applied, the accessors in `handlers.rs` become plain field reads (one-line change
each); `extra` keeps the §4 keys.
