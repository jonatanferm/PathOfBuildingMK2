# CCR-0072 (PS-C): cluster subgraph representation and three frozen-signature gaps

**Status: proposed (PS-C, 2026-11-09).** §3-§6 are conventions PS-C follows inside its own
file `crates/pob-calc/src/classes/passive_spec_cluster.rs` (no frozen file changed). §1-§2 ask
for signature changes of PS-C's frozen stubs; PS-C works around each locally (marked
`CCR-0072`). §6 asks for two model fields.

- **Raised by:** PS-C
- **Affects:** `crates/pob-calc/src/classes/passive_spec_cluster.rs` (PS-C); readers of
  `spec.subGraphs` (U23/U25 tree view: PassiveTreeView.lua:655-661, 749-757); `model/mod.rs`
  `Build` / `TreeTab` (U0, §6)
- **Pin:** 16de4b82d

## 1. `BeginLegacyClusterHashConversion` returns a boolean

PassiveSpec.lua:1734-1739 returns `needsLegacyClusterHashConversion`, which
`BuildClusterJewelGraphs` (:1807) reads. The frozen stub returns `()`. Workaround:
`BeginLegacyClusterHashConversion_ret(&self) -> bool` holds the body; the frozen fn calls it.

```diff
-    pub fn BeginLegacyClusterHashConversion(&self);
+    pub fn BeginLegacyClusterHashConversion(&self) -> bool;
```

## 2. The orbit-index helpers take the `indicies` table, whose `pairs` order matters

`ApplyClusterOrbitIndexAdjustment` (:1936-1941) and `BuildLegacyClusterOrbitMappings`
(:1944-1968) iterate `indicies` (`{ [nodeIndex] = node }`, keys 0..15) with `pairs`
(pairs_inventory rows 62/63). In `BuildLegacyClusterOrbitMappings` the order is observable:
`legacyNodeIdsByOidx[legacyNodeOidx] = node.id` keeps the last node per colliding index (the
16→12 and 16→6 translations map two indices to one), and the registration order of
`legacyClusterNodeMap` follows `pairs(legacyNodeIdsByOidx)`. LuaJIT's order of `indicies`
(key 0 in the hash part, 1..n in the array part after the rehashes the insertion sequence
causes) is not a `Table`'s insertion order. Both Lua functions return nothing; the frozen
stubs return a `Table`.

Workaround: `ApplyClusterOrbitIndexAdjustment_lj` / `BuildLegacyClusterOrbitMappings_lj` take
`&LjIntMap<NodeRef>` (built by `BuildSubgraph` with Lua's insertion sequence) and return `()`.
The frozen fns convert their `Table` (values `Val::Obj(NodeRef)`) by inserting its `pairs`
in order into a new `LjIntMap` and return `indicies` itself.

```diff
-    pub fn ApplyClusterOrbitIndexAdjustment(&self, indicies: &Table, startOidx: f64,
-        clusterTotalIndicies: f64, skillsPerOrbit: f64) -> Table;
+    pub fn ApplyClusterOrbitIndexAdjustment(&self, indicies: &LjIntMap<NodeRef>, startOidx: f64,
+        clusterTotalIndicies: f64, skillsPerOrbit: f64);
-    pub fn BuildLegacyClusterOrbitMappings(&self, indicies: &Table, proxyNode: &NodeRef,
-        clusterTotalIndicies: f64, skillsPerOrbit: f64) -> Table;
+    pub fn BuildLegacyClusterOrbitMappings(&self, indicies: &LjIntMap<NodeRef>, proxyNode: &NodeRef,
+        clusterTotalIndicies: f64, skillsPerOrbit: f64);
```

## 3. `subGraph.connectors` holds link pairs

`linkNodes` (:2017-2026) calls `self.tree:BuildConnector(node1, node2)` (PassiveTree.lua
909-1040, the UI's: U23 `tree_view/connectors.rs`) and appends its one or two connector
objects to `subGraph.connectors`. The calc does not build connector polygons. Convention:
each `linkNodes` call appends one pair `{ node1, node2 }` (a list `Table` of two
`Val::Obj(NodeRef)`) to `subGraph.connectors`, in call order; the tree view builds the
connectors of a subgraph from these pairs (`BuildConnector(pair[1], pair[2])`, appending
`connectors[1]` and, when present, `connectors[2]`), which gives PoB's list. Nothing in the
calc reads `connectors`.

Subgraph tables otherwise follow the Lua: `{ nodes = { node, … }, group = { oo = { … }, x, y },
connectors, parentSocket, entranceNode }`, nodes as `Val::Obj(NodeRef)`.

## 4. `pairs(self.subGraphs)` order

`spec.subGraphs` is a frozen `Table` (insertion order); PoB's removal loop (:1812) iterates it
in LuaJIT's integer-key order (pairs_inventory row 59), and that order decides the order of
`allocSubgraphNodes`, hence the re-allocation order of `allocNodes` (an `LjIntMap`: colliding
cluster ids iterate in insertion order) and of `allocExtendedNodes`. Convention: PS-C keeps a
mirror `LjIntMap<()>` of the table's keys in `spec.extra["__subGraphs_keys"]` (port plumbing),
replayed with every store PoB makes (`subGraphs[id] = subGraph`, `= nil` at :2118,
`wipeTable` at :1829), and iterates the mirror. When the table's live keys differ from the
mirror's (PS-A's `Init` wipes the `Table` where Lua assigns a fresh `{ }`), the mirror is
re-created fresh. Remaining difference: an `Init` while the table is already empty but its
mirror holds dead keys keeps those dead keys (Lua starts from a fresh table); only the order
of the next removal loop could differ.

Proposed: `PassiveSpec.subGraphs: Shared<LjIntMap<Table>>`.

## 5. Undo state

`CreateUndoState` (:2361-2383) returns a `Table` whose `hashList`, `hashOverrides`
(`Val::Obj(NodeRef)` values) and `masteryEffects` are filled in the `pairs` order of the
spec's `LjIntMap`s. `RestoreUndoState` rebuilds `hashOverrides` / `masteryEffects` as
`LjIntMap`s by inserting each table's entries in that order (the order Lua inserted them into
its copies), so `pairs` over them is LuaJIT's order of PoB's copies, and calls
`ImportFromNodeList_impl` (CCR-0052 §5). `secondaryAscendClassId = self.secondaryAscendClassId`
(:2377) reads a field PassiveSpec never sets (the id is `curSecondaryAscendClassId`): the
state's value is nil in PoB, and in the port (`LUA-QUIRK`; `spec.extra` has no such key).

## 6. UI-only statements

- `self.build.treeTab.viewer.searchStrCached = ""` (:1878): the tree viewer's search cache;
  `TreeTab` has no viewer in the model. Not ported (comment at the site); U25 clears its
  search cache after `BuildClusterJewelGraphs`.
- `SetWindowTitleWithBuildClass` (:2390-2396): `main:SetWindowTitleSubtext` is the UI's and
  `build.buildName` is not in the model. The port computes `classText` as PoB does and hands
  it to a host receiver (`passive_spec_cluster::set_window_title_subtext_hook`); the receiver
  formats `"%s (%s)"` with its build name. Proposed: `Build.buildName: Field<LStr>`, after
  which the port passes the formatted subtext.

## 7. Smaller points

- `spec.jewel_data` (ImportTab.lua:1139-1142: a table by node id of the account JSON's jewel
  data) and `spec.extended_hashes` (ImportTab.lua:1152, a list) are read from `spec.extra`
  under those names (contracts.md class census: untyped spec fields).
- `spec.legacyClusterNodeMap` is a `Val::Obj(RefCell<LjIntMap<f64>>)` in `spec.extra` (PoB
  iterates it with `pairs`, :1781); `legacyClusterNodeMapReverse` stays a `Table` (CCR-0052 §7;
  PS-A's `GetSocketedJewel` reads it).
- `ReplaceNode` (:1697-1718) assigns fields of the replacement that may be nil. On a spec node
  (an overlay of its tree node, CCR-0052 §1) a nil assignment makes later reads fall back to
  the tree node; the port writes the tree node's value in that case (`name`, `sprites`,
  `effectSprites`, `icon`, `spriteId`, `activeEffectImage`, `overrideType`, `keystoneMod`,
  `isTattoo`). `reminderText` (`newNode.reminderText or wipeTable(old.reminderText)`) wipes,
  in place, the list the spec node reads, which may be the tree node's own (the spec node's
  `extra` aliases it), as in PoB.
- `modList:NewMod("PassiveSkillEffect", "INC", effect)` (:2313) has no source: the port adds
  `createMod(…, None, …)` (mod.source nil), as `NewMod` does with a nil source.
- Cluster nodes have no `linkedId` in Lua (nil); `Node::linkedId` is an empty list. Only
  `ReconnectNodeToClassStart` reads it (on tree nodes) and would raise on a cluster node.

## 8. Reference cycles of removed subgraph nodes

Subgraph nodes link to each other (`linked` rings, the entrance ↔ parent socket pair) and,
after PS-B, through `depends` / `path`: `Rc` cycles (calc doc §3.3 rule 4). PoB drops the old
subgraph nodes at every `BuildClusterJewelGraphs` (:1812-1829) and lets the GC collect them.
The port, after the removal loop (all `t_remove`s from parent sockets done), replaces the
`linked`, `depends` and `path` of every removed node with fresh empty values (never clearing a
list in place, so a list another node aliases keeps its contents). A holder of an old node (none
in the calc: `nodes`, `allocNodes` and `subGraphs` drop them; PS-B's `checkNodeLinks` pass
drops stale references from live nodes) sees it unlinked. The spec's live cluster nodes are
released by PS-A's `Drop for PassiveSpec` (CCR-0052 §10), as they are in `spec.nodes`.
