# CCR-0052 (PS-A): PassiveSpec representation notes and four frozen-signature gaps

**Status: proposed (PS-A, 2026-11-09).** §1-§2, §7-§12 are conventions PS-A follows inside its own
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
`nodesInRadius`, `group` alias the tree node's tables, which is what an `__index` read returns
when the tree node has that field), with a fresh `linked`, a fresh `depends`, its own `extra`
(a shallow copy of the tree node's `extra` plus `power = {}`) and `Node::tree = Some(treeNode)`.

Aliasing is only faithful for fields the Lua tree node actually carries. A `Node` cannot hold
`nil` in its non-`Option` handle fields: `Node::default` gives each one a live, empty handle,
so for a field that is `nil` on the Lua tree node the copy would alias that placeholder, and
node N of every spec on the same tree version (TreeTab's specs, the convert spec) would share
it. Audit of the non-`Option` handle fields of `Node` against PassiveTree.lua:
- `depends`: never set on tree nodes, so `node.depends` reads `nil` and
  `node.depends = wipeTable(node.depends)` (:1142, Common.lua:549-552) gives each spec node a
  fresh `{ }`. `new_spec_node` therefore gives `depends` its own empty list (as for `linked`).
  Without this, an in-place `wipeTable` of one spec's `depends` (PS-B) would clear and refill
  another spec's, and `DeallocNode` (:832) would deallocate dependants another spec computed.
- `linked`: fresh (the `{ linked = { } }` constructor).
- `linkedId`: set on every tree node (PassiveTree.lua:543); aliasing is faithful.
- `mods`, `modList`, `modKey`: set on every tree node by `ProcessStats` (:799-800, through
  `ProcessNode` :623/:905); aliasing is faithful. That includes PoB's in-place
  `node.modList:NewMod(...)` on a spec node (:1726), which mutates the tree node's list.
- `sd`: `node.sd = node.stats` (:535); the only tree node without `stats` is `root` (checked on
  TreeData 3_26 and 3_6: 1 of 3160 and 0 of 1862 nodes), which has no `group` and is not
  copied into a spec (:54). So every spec node's tree node has a real `sd`.
- `extra`: shallow-copied, not aliased.
Every other handle field is an `Option` and copies `nil` as `None`.
For the tattoo override copies (`copyTable(tattooNode, true)`, :181) `linked` and `depends`
are fresh as well: tattoo data nodes have neither.

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
effect values numbers or numeric strings) in their insertion order.

Effect values: Lua stores the raw `effect` in `masterySelections` (:279) and calls `tonumber`
only for the `< 65536` test (:278); the frozen `masterySelections` is a `LjIntMap<f64>`, so the
port stores `tonumber(effect)`. A numeric-string effect would therefore be a string in PoB
(`tree.masteryEffects["123"]` misses where `[123]` hits) and a number in the port. The only
caller that passes untyped values is ImportTab (:1137 copies `charPassives.mastery_effects`
values as they are); the GGG account JSON has numbers there (upstream fixture
`spec/System/SampleCharacter.json:3450-3458`, e.g. `"63559": 64875`), and `Load` builds numbers
itself (:153-157), so the two agree on every input PoB receives.

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
new tree version) clears the same links of the node set it replaces (Lua's GC collects that
set once unreachable); the fields are replaced, never cleared in place. A holder of an old node
(none in scope: `allocNodes` and the other node tables are re-created by the same `Init`) sees
it unlinked.

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

## 12. Oracle-test seam: `SpecStubs` (integration fix, 2026-11-10)

The PS-A, PS-B and PS-C oracle tests each run one unit's code against PoB with the other two
units' methods replaced in PoB (`PassiveSpecClass.X = function() end`). While those were
`unported!` in the port both sides agreed; once all three landed the port ran the real bodies and
the tests compared different code: PS-A's `load/msd` carried a runegraft's text that only the
real `ReplaceNode` at PassiveSpec.lua:287 copies (PoB's stub does not), then the real
`BuildAllDependsAndPaths` raised "attempt to index local 'item'" (PS-A's builds have no items;
PassiveSpec.lua:1534 -> :1065); PS-B's real `BuildClusterJewelGraphs` met stand-in jewels and
PS-C's real `BuildAllDependsAndPaths` loaded the timeless LUTs.

`passive_spec::SpecStubs` (+ `set_spec_stubs`, `spec_stubs`) is a thread-local set of flags. Each
flag makes the method return at once, as the PoB stub does, for every caller:
`BuildAllDependsAndPaths` (PS-B; the counters its PoB stub initialises are the port's fields,
which start at 0), `BuildClusterJewelGraphs` and `ReplaceNode` (PS-C; returns `nil` = `false`).
The product never sets it (`SpecStubs::NONE`). PS-A sets all three, PS-B the two PS-C ones
(inside `BuildAllDependsAndPaths` its `PscCalls` recorder, CCR-0070 §2, already takes PS-C's
calls), PS-C the PS-B one. The whole-spec comparison against an unstubbed PoB is BM-A's.
