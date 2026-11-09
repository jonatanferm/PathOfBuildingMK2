# CCR-0074 (CS0): CalcSetup.lua 1-474 helper signatures and representations

**Status: proposed (CS0, 2026-11-09).** CS0 changed the signatures of its own frozen stubs
(nothing calls them yet: CS1-CS3 and PF-B are their callers) to carry what the Lua passes and
returns; the changed code is marked `CCR-0074`.

- **Raised by:** CS0
- **Affects:** calc-architecture.md §5.10 (`calcs.buildModListForNode(List)` re-exports),
  `crates/pob-calc/src/modules/calc_setup/{node_mods,support}.rs` (CS0), their callers CS1
  (CalcSetup.lua:774), CS2 (1421-1423, radius jewel list 886-925), CS3 (1641-1829),
  `active_skill.rs` `SkillEffect.req` (U0)
- **Pin:** 16de4b82d

## 1. `buildModListForNode` takes the scratch list and returns the explode source

Lua: `calcs.buildModListForNode(env, node, reuse)` — `reuse` is a ModList or nil (the scratch list
of CalcSetup.lua:272), not a flag; it returns `modList` and, when the list has a `CanExplode` mod,
`modList:Flag(nil, "CanExplode") and node` (CalcSetup.lua:252-256; read at 274).

```diff
-pub fn buildModListForNode(env: &EnvRef, node: &NodeRef, reuse: bool) -> ModList
+pub fn buildModListForNode(env: &EnvRef, node: &NodeRef, reuse: Option<&ModList>) -> (ModList, Val)
```

The second value is `Val::Nil` (no second return), `Val::Bool(false)` or `Val::Obj(NodeRef)`.

## 2. `buildModListForNodeList` returns `explodeSources`

Lua returns `modList, explodeSources` (CalcSetup.lua:303); CS2 concatenates the list into
`env.explodeSources` (1421-1423).

```diff
-pub fn buildModListForNodeList(env, nodeList, finishJewels) -> ModList
+pub fn buildModListForNodeList(env, nodeList, finishJewels) -> (ModList, Vec<Val>)
```

Entries are `Val::Obj(NodeRef)` or `Val::Bool(false)` (`t_insert(explodeSources, false)` happens
when a node has a `CanExplode` mod whose flag is off; a `nil` explode appends nothing).
`env.explodeSources` is `Shared<Vec<Table>>` and cannot hold nodes: that is CCR-0030 row 3 (its
`ExplodeSource` enum would be the element type here too).

## 3. `applyGemMods` takes Tabulate rows

`applyGemMods(effect, modList)` iterates `{ value = …, mod = … }` rows
(`env.modDB:Tabulate("LIST", cfg, "GemProperty")`, CalcSetup.lua:1614/1704/1741/1775/1872) and reads
`mod.value` and `mod.mod` (384-414).

```diff
-pub(crate) fn applyGemMods(effect: &SkillEffectRef, modList: &[Value])
+pub fn applyGemMods(effect: &SkillEffectRef, modList: &[TabRow])
```

The rows appended to `effect.gemPropertyInfo` are `{ value = Val::from_value(row.value), mod =
Val::Mod(row.mod) }` tables (CCR-0040 §6).

## 4. `applySocketMods` takes the gem's data

CalcSetup.lua:1708/1779 pass `gemInstance.gemData` (it becomes `socketCfg.skillGem`, a
`GemDataRef` in `ModCfg`), not a skill effect.

```diff
-pub(crate) fn applySocketMods(env, gem: &SkillEffectRef, groupCfg: &Cfg, socketNum: f64, modSource: Option<&str>)
+pub fn applySocketMods(env, gem: &GemDataRef, groupCfg: &Cfg, socketNum: f64, modSource: Option<&str>)
```

LUA-ALIAS (§3.6 audit addition): `modLib.setSource(value.value, …)` at CalcSetup.lua:424 rewrites
the mod held by the `SocketProperty` mod value that `env.modDB` stores (ModStore:List returns the
stored value table; ModTools.lua:239-246 mutate it). The port adds a re-sourced copy to
`env.player.modDB`, so the stored SocketProperty value keeps its original inner source. The
player's modDB is identical; only a dump of the SocketProperty value itself differs.

## 5. Visibility

`applyGemMods`, `applySocketMods` and `addBestSupport` are `local function`s in Lua. They are `pub`
(was `pub(crate)`) so the CS0 acceptance test (`crates/pob-oracle/tests/CS0.rs`) can run them on
oracle-seeded inputs; CS3 is the only calc caller.

## 6. `SkillEffect.req` is a number

GemProperty rows with `key = "req"` (ModParser.lua:2374-2375) make `effect.req = (effect.req or 0)
+ value.value` a number, read as `gemInstance.reqOverride = supportEffect.req`
(CalcSetup.lua:1705/1776, used at 1848-1850). The frozen `SkillEffect.req: Field<Option<Table>>`
cannot hold it. Workaround: `applyGemMods` keeps `req` (and every key other than `level`,
`quality`, `globalQuality`, `itemQuality`, `supportQuality`, `actorLevel`) in `effect.extra`;
CS3 must read `effect.extra.opt_num("req")` until the field changes.

```diff
-    pub req: Field<Option<Table>>,
+    pub req: Cell<Option<f64>>,
```

## 7. Radius jewel entries (for CS2)

`env.radiusJewelList` is `Shared<Vec<Table>>`; CS0 reads each entry as documented at the top of
`node_mods.rs`: `type` (string), `nodes` (`Val::Obj(Rc<LjIntMap<NodeRef>>)` =
`node.nodesInRadius[radiusIndex]`, CCR-0024 §3, or a `Table`, e.g. the empty `{ }` fallback),
`func` (`Val::Data(Value::JewelFunc(spec))`, i.e. the `func` field of a `JewelFunc` /
`ExtraJewelFunc` LIST value; the CalcSetup.lua:877-884 default is the `"$default"` spec of
CCR-0039 §4), `item` (`Val::Obj(ItemRef)`), `nodeId` (number), `data` (table). CS2 must build them
in that shape.

## 8. Node fields are read directly

Spec nodes are shallow copies of tree nodes (CCR-0052 §1), so `buildModListForNode` reads
`node.r#type`, `node.modList`, `node.keystoneMod` and writes `node.finalModList` /
`node.grantedSkills` on the node it is given, never through `Node::tree`.
