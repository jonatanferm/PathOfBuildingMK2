# CCR-0070 (PS-B): BuildAllDependsAndPaths representation notes and a PS-C call seam

**Status: proposed (PS-B, 2026-11-09).** No frozen signature changes. §1 is a type mismatch
between two frozen contracts, worked around inside `passive_spec_paths.rs` (marked `CCR-0070`);
§2 adds port plumbing to PS-B's own file; §3-§4 are notes for PS-C and the integrator.

- **Raised by:** PS-B
- **Affects:** `crates/pob-calc/src/classes/passive_spec_paths.rs` (PS-B), `Node::conqueredBy`
  (`classes/mod.rs`, U0), readers of `node.conqueredBy` (CS0/CalcSetup, U25 tree view),
  PS-C (`passive_spec_cluster.rs`)
- **Pin:** 16de4b82d

## 1. `node.conqueredBy` is a `Table`, `jewelData.conqueredBy` is immutable data

`Node::conqueredBy` is `Field<Option<Table>>` (frozen). PassiveSpec.lua:1187 assigns the jewel's
`item.jewelData.conqueredBy` itself; IT-C builds that value from the parsed `JewelData` mod
(`jewelData[value.key] = value.value`, Item.lua:2478), which in the port is an immutable
`Val::Data(Value::Table)` (`item_mods.rs` `apply_data_list`). Workaround: PS-B stores a shallow
`Table` copy (`Table::from_value_table`; `conqueror` stays `Val::Data`), made once per radius jewel
and call, so all nodes of one jewel share one table as in Lua. If `jewelData.conqueredBy` already
is a `Table` it is stored as is. Every reader reads the value by field (PassiveSpec.lua:1224-1401,
PassiveTreeView.lua:24; no identity comparison in scope), so the copy is unobservable.
Abyss conquests (L1119-1123) are fresh tables in Lua too; their `modification` is the
`Val::Data` that `readAbyssJewelLUT` returns.

Proposed (optional): document on `Node::conqueredBy` that readers must use `Val`-generic access
(`.get(...)`) for `conqueror`, which may be a data table.

## 2. `PscCalls`: PS-B's calls into PS-C go through a seam

`passive_spec_paths::PscCalls` (+ `set_psc_calls`) is a thread-local trait object. Unset (the
product never sets it) every call goes to the `PassiveSpec` method (`ReplaceNode`,
`NodeAdditionOrReplacementFromString`, `ReconnectNodeToClassStart`, `NodeInKeystoneRadius`) or, for
`ConPrintf`, to stderr. The PS-B oracle test (`crates/pob-oracle/tests/PS-B.rs`) installs a
recorder so PS-B can be verified while PS-C's bodies are `unported!`: both sides record the calls in
order and run identical stand-ins. Once PS-C lands the seam can stay (it then also serves as a call
trace) or be removed together with the test's stand-ins.

## 3. Note for PS-C: `ReplaceNode` nil fall-through matters to PS-B

`old.name = newNode.name` (:1703) with a nil `newNode.name` (legion / tattoo / runegraft nodes)
makes `old.name` read the tree node's value again. PS-B reads `self.allocNodes[id].name` right after
`ReplaceNode(node, self.hashOverrides[id])` (:1429/:1437) as a table key; a port that stores nil
there would raise "table index is nil" where PoB counts the mastery type. CCR-0072 already lists
this fall-through.

## 4. Smaller points

- `self.allocatedMasteryTypes = { }` / `self.allocatedTattooTypes = { }` (:1414/:1416): the frozen
  fields are `Table`s, so their contents are wiped (CalcSetup.lua:706-707 copies both; no other
  holder).
- `data.resolveAbyssJewelComponent(component, self.tree.legion)` (:1255) is evaluated against the
  tree's mutable legion `Table` by a local adapter (`resolve_abyss_component`), as CCR-0020 §8
  allows; the `@pob` marker of DataAbyssJewelLookUpTableHelper.lua:48-57 stays with LUT.
- The 0-1 BFS queue (:1643-1686) writes `queue[queueStart - 1]` (front) and
  `queue[queueLength + 1]` (back) and reads only `queue[queueStart]`, so a `VecDeque` of the live
  range is the same queue (indices may go below 1 in Lua).
- `pairs(changedNode.stats)` (:1261) and `pairs(addition.stats)` (:1327) iterate string keys (random
  in PoB, dump order in the port): `LUA-ORDER`. Each replacement only rewrites its own stat's
  `(min-max)` range, so the order is unobservable unless two stats share a range text.
