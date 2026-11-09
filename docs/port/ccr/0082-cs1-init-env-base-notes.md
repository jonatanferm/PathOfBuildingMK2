# CCR-0082 (CS1): initEnv segment CS1 — u0_smoke expectation, Override node sets, export notes

**Status: proposed (CS1, 2026-11-09).** No frozen signature changes. §1 asks for an edit of a
U0-owned test that CS1's port breaks; §2-§4 record how CS1 reads the frozen types and what the
CS1 L4 test (`crates/pob-oracle/tests/CS1.rs`) had to work around in ORC-X's export.

- **Raised by:** CS1
- **Affects:** `crates/pob-oracle/tests/u0_smoke.rs` (U0), `pob_calc::env::Override` (U0, docs
  only), `crates/pob-calc/src/probe.rs` export (ORC-X, follow-up only)
- **Pin:** 16de4b82d

## 1. `u0_smoke::build_output_on_stubs` assumes CS1 is a stub

`crates/pob-oracle/tests/u0_smoke.rs:86-95` asserts that the stub site
`Modules/CalcSetup.lua:488-799` is hit, and `:76-78` panics on any Lua error of
`try_buildOutput` on `model::build_xml::loadBuildFromXML` (still a BM-A stub: an empty `Build`).
With CS1 ported the segment runs for real on that empty model and raises, exactly as PoB would:
`env.spec.tree.classes[env.classId]` is nil for an empty tree, and CalcSetup.lua:593 indexes it
(`attempt to index a nil value`). Result after CS1: `build_output_on_stubs ... FAILED`
(`slice5_allflame_exp_024 MAIN: Lua error attempt to index a nil value`).

Proposed diff (until BM-A's loader fills the model; then the original `unwrap_or_else` can
return):

```diff
-            let env = pob_calc::try_buildOutput(&build, mode)
-                .unwrap_or_else(|e| panic!("{stem} {mode:?}: Lua error {e}"));
-            // the pipeline produced an env with a player output table
-            assert_eq!(env.mode, mode);
-            let _ = env.player.output.get();
+            // CS1 (CalcSetup.lua 488-799) is ported and raises on the empty model of the
+            // stubbed loader (no tree classes), as PoB would; accept that until BM-A lands
+            match pob_calc::try_buildOutput(&build, mode) {
+                Ok(env) => {
+                    assert_eq!(env.mode, mode);
+                    let _ = env.player.output.get();
+                }
+                Err(e) => eprintln!("{stem} {mode:?}: Lua error {e} (empty model)"),
+            }
             runs += 1;
@@
     for expect in [
-        "Modules/CalcSetup.lua:488-799",
-        "Modules/CalcSetup.lua:800-1450",
-        "Modules/CalcSetup.lua:1451-1973",
-        "Modules/CalcPerform.lua:1289-1942",
-        "Modules/CalcPerform.lua:3497-3990",
-        "Modules/Calcs.lua:384-870",
     ] {
```

(Every later stub expectation becomes unreachable once CS1 raises first; the list can only be
non-empty again with a real model.)

## 2. `Override.addNodes` / `removeNodes` are node-id maps of the node

PoB passes node-keyed sets: `override.addNodes = { [node] = true }`, iterated with
`for node in pairs(override.addNodes)` (CalcSetup.lua:717) and probed with
`override.removeNodes[node]` (748/750). The frozen type is
`Field<Option<Shared<LjIntMap<NodeRef>>>>` (id → node). CS1 iterates the map's values (LuaJIT
order of the ids; PoB's order is the nodes' address order, random per process, marked
`LUA-ORDER`) and reads `override.removeNodes[node]` as "the map holds this very node"
(`Rc::ptr_eq`, Lua table identity). Callers (PWR, CalcsTab power, CompareTab) must therefore put
the spec's own node objects (the ones in `spec.allocNodes` / `spec.nodes`) into these maps, as
PoB does. No change requested; a doc line on `Override` would help.

## 3. ORC-X export: notes for a follow-up (no CS1 change)

Found while writing the CS1 L4 test; each is worked around in the test and documented there.

1. **Eager struct fields.** `Env::new`, `Actor::default` and `SpecEnv` hold every field from
   construction, so a fresh env exports `env.buffs`, `env.curseSlots`, `env.requirementsTable`,
   `env.player.output`, `env.player.weaponData1`, `env.enemy.itemList`, `specEnv.accelerate`, …
   with default values that PoB's tables do not have yet at CalcSetup.lua:800 (49 674 such
   `Extra` mismatches over the CS1 corpus). Injection marks absent fields of injected objects
   (`Registry::set_absent`), but objects created by the Rust code are not marked. A rule such as
   "export a defaulted field only after the Rust code assigned it" would remove this noise for
   every segment that creates objects (CS1, CS3, PF-B's minion actors).
2. **Address reuse in the probe registry.** The registry keys injected tables by address. A test
   that replaces an injected local (CS1 replaces the plain-table `override` local by an
   `Override`) frees that table; a table the segment creates afterwards may get the same address
   and is then exported with the freed table's contents (observed: a node-keyed entry inside
   `env.enemyDB.conditions`). The CS1 test keeps replaced values alive until the export.
   Suggested fix: `Registry::keep` every injected object (the registry already keeps some).
3. **`specEnv.accelerate` identity.** `SpecEnv.accelerate` is a `Copy` struct, so PoB's
   `specEnv.accelerate` = `accelerate` local identity (CalcSetup.lua:482) exports as a separate
   table (`Value` mismatch against `{"$ref": "locals.accelerate"}`).

## 4. `env.player.itemList`

`Actor.itemList` is a plain `Shared<IndexMap>` (not a `Field`), so `env.player.itemList = { }`
(CalcSetup.lua:536) clears the fresh actor's own map instead of assigning a new table. On a fresh
env (the only place 536 runs) nothing else holds that map, so this is equivalent.
