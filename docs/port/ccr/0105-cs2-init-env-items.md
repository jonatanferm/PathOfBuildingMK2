# CCR-0105 (CS2): CalcSetup.lua 800-1450 — removeNodes id keys, flaskSlotMap, explodeSources, restart actor

**Status: applied (CCRDOC, 2026-11-12; code `3d4735d` Merge port/CS2) with the extra-key representation. Calc doc §5.4 `Override` lists `extra["$removeNodeIds"]` (§1, also filled by U24's tooltips) and `extra["$extraJewelFuncsActor"]` (§4). §1's typed `removeNodeIds` field is rejected because it was not built. §2-§3 are superseded by RETYPE E1 (typed `Env.flaskSlotMap` and `explodeSources`; AMENDMENTS A10).**

**Filed status: proposed (CS2, 2026-11-10).** No frozen file was changed. Every item is worked
around inside CS2's own file (`crates/pob-calc/src/modules/calc_setup/init_env_items.rs`) and
marked there; the test notes are in `crates/pob-oracle/tests/CS2.rs`.

Number reserved with `scripts/port/next_ccr.sh` (rust-port 85604a9) in CS2 fix round 1. The
first filing took 0101 by hand, before that script was on rust-port; 0101 is PWR's
(`0101-pwr-power-seams.md`), so this CCR was renumbered and every `CCR-0101` reference in
`init_env_items.rs` now reads `CCR-0105`.

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
must fill that table. Since the node-keyed half never matches a number key, the Lua test
`not override.removeNodes or not override.removeNodes[node.id]` is ported as
`!removeNodes_byId(..)` alone (a caller that fills only the id table leaves the typed map None, so
the typed map's presence must not short-circuit the test).

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
  (another env's player), which upgrades to nothing. When the hit is the recursive call of a
  restart the test ran itself (the previous hit), the test swaps in the store and actor of that
  Rust pass 1, as PoB's own pass 1 hands them on. Otherwise (PoB reused a caller's override that
  already holds a store) it points the actor at the injected env's player. Either way the
  `ItemCondition` tags see a live `actor.itemList`.
* The restart path is compared against PoB on the synthetic builds `corpus/CS2/*.xml` (fix
  round 1; all57 has no `ExtraJewelFunc` mod). `cs2_ejf_rare_jewel` restarts 6 times. All 6
  second passes are identical to PoB's 1451 dump, with 9 extra-func radius entries each (905-931).
  `cs2_ejf_unique_amulet` does not restart, in PoB or in Rust.
* A dumped closure (`{"$fn": src}`) has no upvalues, so an injected `ExtraJewelFunc` mod from
  2630-2634 raises in `firstToUpper(attr)`. The test puts back the upvalues from a second,
  never-injected load of the build (docs/port/FOLLOWUPS.md "CS2" 2).
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
  (calc_active_skill.rs:713 after the rust-port merge) in MAIN and in both misc calculators:
  The Saviour's Reflection mirage sets `env.player.mainSkill = newSkill` (CalcMirages.lua:155), and the mirage env that owns the
  skill's actor is dropped (`ActiveSkill.actor` is `Weak`, calc doc §3.3 rule 4); Lua keeps it
  alive through the skill. CALCS then has no `mainEnv`. CA's L6 floors bound these four errors.
  Tracked as docs/port/FOLLOWUPS.md "CA" 6.
* **CA (D13):** allflame_exp_050's second `buildOutput` on a warm GlobalCache differs from the
  first (BleedStacks 0.5875 → 1.25, CombinedDPS 7.41M → 15.76M); with `wipeGlobalCache()` between
  the runs both agree, and the first run equals PoB's L6 output. Fix round 1 measured PoB: it
  differs the same way (26 keys, the same values; `pob_headless` under LuaJIT), so this is PoB
  behavior that the port reproduces. `ca_d13_fixed_point` records it as `D13_POB_WARM_CACHE`
  (warm run against PoB's values, fixed point after a wipe); docs/port/FOLLOWUPS.md "CA" 5.
* **CS3 / ORC-X testkit:** `cs3_l5_init_env` still raises on all 171 runs (`attempt to index a nil
  value`), before and after CS2: `testkit::load_build` runs only `Build:LoadDB`, without BM-A's
  `Init_model`; the same builds through `build_xml::loadBuildModelFromXML` run `initEnv` without
  error (checked on allflame_exp_000/009: 18 and 22 equipped items).
