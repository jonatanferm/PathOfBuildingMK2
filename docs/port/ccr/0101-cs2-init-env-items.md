# CCR-0101 (CS2): CalcSetup.lua 800-1450 — removeNodes id keys, flaskSlotMap, explodeSources, restart actor

**Filed status: proposed (CS2, 2026-11-10).** No frozen file was changed. Every item is worked
around inside CS2's own file (`crates/pob-calc/src/modules/calc_setup/init_env_items.rs`) and
marked there; the test notes are in `crates/pob-oracle/tests/CS2.rs`.

`scripts/port/next_ccr.sh` (named by the CS2 task) does not exist on rust-port c4628cb or in the
main checkout; 0101 is the next number after the highest CCR file on rust-port and in every
worktree (0100), so it was taken by hand. Renumber on merge if another unit took it first.

- **Raised by:** CS2
- **Affects:** `pob_calc::env::Override` (U0, frozen), `Env.flaskSlotMap` / `Env.explodeSources`
  (U0, frozen; CCR-0030 #3/#5, A8 E1), CalcPerform.lua:1710 reader (PF-C), PassiveTreeView /
  power callers that build `override.removeNodes` (UI, PWR)
- **Pin:** 16de4b82d

## 1. `override.removeNodes` mixes node keys and node-id keys

CalcSetup.lua:1393 and 1408 test `override.removeNodes[node.id]`; CalcSetup.lua:748/750 (CS1)
test `override.removeNodes[node]`. PoB's callers build both shapes: `{ [node] = true }`
(CalcsTab.lua:626, 638; PassiveTreeView.lua:1700, 1702) and, for anointed nodes, `{ [node.id] =
true }` (PassiveTreeView.lua:1706; Calcs.lua:77 documents it: "Only keyed by node id for anointed
nodes"). In PoB a node-keyed set never matches the number key at 1393/1408, and an id-keyed set
never matches the node key at 748.

The frozen `Override.removeNodes: Field<Option<Shared<LjIntMap<NodeRef>>>>` is CS1's node-keyed
set (CCR-0082 §2: "the map holds this very node"); it cannot say whether an entry was keyed by the
node or by its id. Reading it by id at 1393/1408 would wrongly drop an anointed copy of a node the
power calc removes.

**Workaround (CS2):** 1393/1408 read the id keys from `override.extra["$removeNodeIds"]` (a table
id → true); the typed map is only the node-keyed half. A caller that ports PassiveTreeView.lua:1706
must fill that table.

**Proposed diff** (calc doc §5.4, `env.rs`):

```diff
 pub struct Override {
     pub removeNodes: Field<Option<Shared<LjIntMap<NodeRef>>>>,
+    /// `override.removeNodes[<node id>] = true` entries (PassiveTreeView.lua:1706).
+    pub removeNodeIds: Field<Option<Shared<LjIntMap<bool>>>>,
```

and CS2 reads `removeNodeIds` (presence of the whole `removeNodes` table in Lua is "either is
Some").

## 2. `env.flaskSlotMap[item]` (CCR-0030 #5, still escalated as A8 E1)

CS2 writes the item-keyed map at CalcSetup.lua:1023/1035; PF-C's reader (CalcPerform.lua:1710)
raises today. **Workaround:** the item → slot entries live in a side map,
`env.extra["$flaskSlotMap"]` (`Val::Obj(Rc<Shared<PtrMap<Item, f64>>>)`), with the public accessor
`init_env_items::flaskSlotMap_get(env, item)`; probe builds also record them as the typed table's
object keys (`Registry::push_obj_key`), so the export writes `env.flaskSlotMap` as PoB dumps it.
**Proposed:** PF-C reads `flaskSlotMap_get` until CCR-0030 #5 retypes the field; then the side map
goes.

## 3. `env.explodeSources` (CCR-0030 #3)

As CCR-0083 §2 asked, CS2 keeps the Lua list (items from 838, then `tableConcat(nodes,
env.explodeSources)` at 1423) in `env.extra["explodeSources"]`, which CS3 reads. The typed field
stays the (Lua-empty) list CS1 created, so the exporter writes `env.explodeSources` empty (typed
fields win over `extra` in the export). No change beyond CCR-0030 #3.

## 4. The restart keeps the first pass's player alive (no change requested)

CalcSetup.lua:1428 sets `override.extraJewelFuncs.actor = env.player` and restarts (1433). Lua's
reference keeps that player (and its filled `itemList`) alive through the second pass, where
EvalMod's `ItemCondition` (ModStore.lua:697, the `ExtraJewelFunc` mods' `{SlotName}` tags) needs a
live `actor.itemList`. `ModStore.actor` is `Weak` (calc doc §3.3 rule 4), so the first env's
player would be freed with its env and every ExtraJewelFunc mod would fail its tags. CS2 stores the
strong reference in `override.extra["$extraJewelFuncsActor"]` (Rust-only plumbing; exported as an
extra override key, which the L4 test drops). A typed `Override` field would be cleaner but is not
needed.

## 5. Exporter / L4 notes (for ORC-X2)

* A second pass's injected `override.extraJewelFuncs` store has a `$foreign` placeholder actor
  (another env's player), which upgrades to nothing; the CS2 test points it at the injected env's
  player (any live actor satisfies `{SlotName}` tags).
* Hits PoB made during `Build:Init` before its item set loaded (empty `env.player.itemList` at 1451)
  cannot be injected onto the loaded model; the test skips them as `pre-load`.
* The exporter cannot write several values CS2 stores as the contracts prescribe; the CS2 L4 test
  compares them semantically instead (each class counted in its output): `env.radiusJewelList[i].nodes`
  (`Val::Obj(Rc<LjIntMap<NodeRef>>)`, exported as `{"$userdata": true}`; compared as node-id sets),
  jewel funcs (`Value::JewelFunc` vs PoB's `{"$fn": src}`), `item.slotModList[3]` of items the
  model loaded (IT-C's `extra["$slotModList3"]`, not written; compared by length), and every
  object PoB first emits under `env.explodeSources` (the typed field exports empty, so the same
  item / node is emitted under another canonical path; compared by identity).
* `env.allocNodes` is rebuilt by injection without PoB's insertion history, so tree-node mods in
  the stores (and the node half of `explodeSources`) come out in another LuaJIT `next` order; the
  test checks the non-tree mods' order exactly and the tree-node mods as a multiset.

## 6. Found downstream once items reach the env (for the owners; not CS2 code)

* **MI / AS:** upstream_Dual_Savior raises `attempt to index field 'actor' (a nil value)`
  (calc_active_skill.rs:723) in MAIN and in both misc calculators: The Saviour's Reflection mirage
  sets `env.player.mainSkill = newSkill` (CalcMirages.lua:155), and the mirage env that owns the
  skill's actor is dropped (`ActiveSkill.actor` is `Weak`, calc doc §3.3 rule 4); Lua keeps it
  alive through the skill. CALCS then has no `mainEnv`. CA's L6 floors bound these four errors.
* **CA / PF (D13):** allflame_exp_050's second `buildOutput` on a warm GlobalCache differs from the
  first (BleedStacks 0.5875 → 1.25, CombinedDPS 7.41M → 15.76M); with `wipeGlobalCache()` between
  the runs both agree, and the first run equals PoB's L6 output. `ca_d13_fixed_point` reports it
  (left failing: whether PoB differs the same way on a warm cache is not established).
* **CS3 / ORC-X testkit:** `cs3_l5_init_env` still raises on all 171 runs (`attempt to index a nil
  value`), before and after CS2: `testkit::load_build` runs only `Build:LoadDB`, without BM-A's
  `Init_model`; the same builds through `build_xml::loadBuildModelFromXML` run `initEnv` without
  error (checked on allflame_exp_000/009: 18 and 22 equipped items).
