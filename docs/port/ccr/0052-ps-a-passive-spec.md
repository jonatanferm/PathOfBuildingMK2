# CCR-0052 (PS-A): PassiveSpec representation notes and four frozen-signature gaps

**Status: proposed (PS-A, 2026-11-09).** §1-§2, §7-§11 are conventions PS-A follows inside its own
file `crates/pob-calc/src/classes/passive_spec.rs` (no frozen file changed). §3-§6 ask for
signature changes of PS-A's frozen stubs; PS-A works around each locally (marked `CCR-0052`).

- **Raised by:** PS-A
- **Affects:** `crates/pob-calc/src/classes/passive_spec.rs` (PS-A), its consumers PS-B
  (`passive_spec_paths.rs`), PS-C (`passive_spec_cluster.rs`), BM-A (`tree_tab.rs`,
  `build_xml.rs`), CS0, U25/U27 (tree view / import UI); `crates/pob-calc/src/model/mod.rs`
  `TreeTab` (U0, §9); CCR-0020 §7 (same request as §4)
- **Pin:** 16de4b82d

## 1. Spec nodes overlay tree nodes by a shallow copy

PassiveSpec.lua:56 makes each spec node `setmetatable({ linked = { }, power = { } }, treeNode)`:
reads fall through to the tree node, writes stay on the spec node. `Node`'s fields are plain
`Field`/`Cell`s that every consumer reads directly, so the port cannot fall back lazily.
Convention (`passive_spec::new_spec_node`): the spec node is a **shallow copy** of the tree node
(`copy_node`: every field cloned; `Field` handles such as `sd`, `mods`, `modList`, `linkedId`,
`nodesInRadius`, `group` alias the tree node's tables, exactly what an `__index` read returns),
with a fresh `linked`, its own `extra` (a shallow copy of the tree node's `extra` plus
`power = {}`) and `Node::tree = Some(treeNode)`.

The difference from Lua: a write to the *tree* node after the spec was built is not seen through
the spec node. In scope the only such write is `treeNode.masteryCache` (PassiveSpec.lua:1046),
which is read only through the tree node (:1031). Assigning `nil` to a spec-node field restores
the tree node's value in Lua; every such site in PS-A (`node.reminderText = nil`, :1050) clears a
field the tree node does not have, so removing it from the copy is the same. PS-B/PS-C must keep
this in mind for any `node.x = nil` on a field that tree nodes carry.

`copy_node` is also `copyTable(node, true)` for nodes (the tattoo override copies, :181) and is
public for PS-B/PS-C.

## 2. `nil` class ids are `NaN`

`curClassId`, `curAscendClassId`, `curSecondaryAscendClassId` are `Cell<f64>` (frozen) but `nil`
in Lua until the first `SelectClass` (`if self.curClassId then`, :574/:599/:635) and, for the
secondary id, forever on trees without `alternate_ascendancies` (`SelectSecondaryAscendClass`
returns at :633). `new_PassiveSpec` starts them at `NaN`; `Save` writes `tostring(nil)` =
`"nil"` for them (as PoB does for `secondaryAscendClassId` on old trees), `EncodeURL` reads
`curSecondaryAscendClassId or 0`. A `tonumber` that fails in `Load` passes `NaN` (Lua `nil`).

## 3. `ConnectToClass` returns a boolean

PassiveSpec.lua:694-765 returns `true`/`false`; Build.lua:273 and PassiveTreeView.lua:492 read it.
The frozen stub returns `()`. Workaround: `ConnectToClass_ret(&self, classId) -> bool` holds the
body; the frozen `ConnectToClass` calls it.

```diff
-    pub fn ConnectToClass(&self, classId: f64) {
+    pub fn ConnectToClass(&self, classId: f64) -> bool {
```

## 4. `GetShortestPathToClassStart` returns the Lua set (CCR-0020 §7)

The Lua result is the set `{ [nodeId] = true }` (:989-995) whose `pairs()` order the abyss LUT
reader iterates (PassiveSpec.lua:1118, via PS-B). Workaround:
`GetShortestPathToClassStart_set(&self, rootId) -> Option<LjIntMap<bool>>` builds it in LuaJIT's
layout (`TNEW` with two hash slots for the constructor, then `path[cur.id] = true` per step); the
frozen fn returns the set's nodes in that order. `BuildSplitPersonalityPath` uses the set.

```diff
-    pub fn GetShortestPathToClassStart(&self, rootId: f64) -> Option<Shared<Vec<NodeRef>>>;
+    pub fn GetShortestPathToClassStart(&self, rootId: f64) -> Option<LjIntMap<bool>>;
```

## 5. `ImportFromNodeList` table parameters

The frozen signature takes `hashOverrides: &Table` and `masteryEffects: &Table`, but
`spec.hashOverrides` is a `Shared<LjIntMap<NodeRef>>` (Lua assigns the parameter to it, :273, and
`Load` passes `self.hashOverrides` itself, :190), and both loops (:276, :282) iterate with
`pairs` in LuaJIT's integer-key order (pairs_inventory rows 33/34), which `Table` (insertion
order) does not reproduce. `masterySelections` is filled in that order, and `Save` writes it in
its `pairs` order (:203), so the saved `masteryEffects` attribute depends on it.

Workaround: `ImportFromNodeList_impl(…, hashOverrides: Shared<LjIntMap<NodeRef>>,
masteryEffects: &[(f64, f64)] /* pairs order */, …)` holds the body; `Load` calls it with
`self.hashOverrides` (alias, as in Lua) and the `masteryEffects` `LjIntMap` it built (:153-157).
The frozen fn converts its `Table`s (keys = node ids, override values `Val::Obj(NodeRef)`,
effect values numbers or numeric strings, `tonumber` at :278) in their insertion order.

```diff
     pub fn ImportFromNodeList(&self, className: Option<Name>, classId: f64, ascendClassId: f64,
-        secondaryAscendClassId: f64, hashList: &[f64], hashOverrides: &Table, masteryEffects: &Table,
+        secondaryAscendClassId: f64, hashList: &[f64], hashOverrides: Option<Shared<LjIntMap<NodeRef>>>,
+        masteryEffects: &LjIntMap<f64>,
         treeVersion: Option<Name>);
```

## 6. `CountAllocNodes` and `GetSocketedJewel` arities

At the pin `CountAllocNodes` returns four values (:865); the frozen tuple has six
(`weaponSet1Used`, `weaponSet2Used` do not exist in v2.67.2). The port returns `0` for the last
two (Lua: `nil`). `GetSocketedJewel` returns one value (:920); the frozen second element is always
`None`. Proposed: drop the extra elements.

## 7. Untyped spec and node fields (`extra`)

Spec (`spec.extra`): `curClass`, `curAscendClass`, `curSecondaryAscendClass` (the class tables),
`clusterHashFormatVersion` (number), `ignoreAllocatingSubgraph`, `checkNodeLinks` (booleans),
`legacyClusterNodeMapReverse` (PS-C: a `Table` node id → legacy node id, read at :914-916).
Nodes (`node.extra`): `visited` (boolean), `power` (table), `reminderText` (a list of strings),
`intuitiveLeapLikesAffecting` (PS-B: a list `Table` of `Val::Obj(NodeRef)`; `#` of a missing list
raises, as in Lua), and on tree nodes `masteryCache` (`Val::Obj(NodeRef)`: the cache node of
:1033, a `Node` so that `ProcessStats` can fill it).

## 8. `main:LoadTree`

PassiveSpec.lua:41 calls `main:LoadTree(treeVersion)` (Modules/Main.lua 352-363), which no calc
unit owns. `passive_spec::main_LoadTree` ports it as plumbing: a thread-local `main.tree` cache,
`data.setJewelRadiiGlobally(treeVersion)` on every call, `nil` for a version outside
`treeVersionList`. `main_tree_insert` preloads a tree (tests: CCR-0024 §5 source order).
Proposed: the calc doc names an owner (U0 `calcs_facade.rs` or BM-A) and BM-A / the UI use this
one cache.

## 9. `treeTab.showConvert`

`ImportFromNodeList` writes `self.build.treeTab.showConvert` (:260) when it switches tree
versions; `TreeTab` (model/mod.rs, U0) has no such field (it is a UI flag, TreeTab.lua:566).
The port does not store it. Proposed: `pub showConvert: Cell<bool>` on `TreeTab`.

## 10. Reference cycles of spec nodes

`Node.linked` (frozen, `Shared<Vec<NodeRef>>`) links spec nodes to each other, and PS-B's
`depends`/`path` do too: `Rc` cycles (calc doc §3.3 rule 4). `impl Drop for PassiveSpec` clears
`linked`, `depends` and `path` of the spec's nodes when the spec is dropped. A spec node that
outlives its spec (an old env still holding it) therefore loses its links; nothing in scope
reads them after the spec is gone. `Init` called again on a live spec (ImportFromNodeList with a
new tree version) replaces the node set without clearing the old one (Lua's GC; the old nodes
leak until the spec drops).

## 11. Smaller points

- `self:UndoHandler()` (:28) and `self:ResetUndo()` (:194) are no-ops: the undo stack is the
  UI's (ui doc §10.3; PS-C ports CreateUndoState/RestoreUndoState, U25 wires `UndoOps`).
- `Init` re-creates the spec's tables (`self.nodes = { }` etc.) by replacing the contents of the
  `Shared` handles (the frozen fields are not `Field`s); `subGraphs` and `ignoredNodes` (`Table`)
  are wiped. Only a holder of the old handle could tell the difference.
- `launch:ShowErrMsg` and `ConPrintf` print to stderr.
- `jewelIdNum` (:130) and `idx` (:449) are globals in Lua; nothing reads them.
- `tree.spriteMap` is not loaded (UI art, CCR-0024 §4), so `override.sprites` /
  `override.effectSprites` (:285-286) are nil.
- `common.base64` (runtime/lua/base64.lua) is ported as `passive_spec::base64_decode` /
  `base64_encode` (lenient decode: characters outside the alphabet are dropped).
