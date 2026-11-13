# CCR-0140: the exporter's two gaps behind CS2's explained buckets

**Status: rejected (CCRDOC, 2026-11-12): not built. `probe.rs` still exports a bare `LjIntMap<NodeRef>` as `{"$userdata": true}`, and CS2's L4 test excuses both buckets within its measured `EXPLAINED_MAX`. This is test-only; FOLLOWUPS "CCRDOC" 11 names the owner.**

Unit: **FUP3** (oracle hygiene, FOLLOWUPS "CS2" 3; owner of the change: ORC-X / ORC-X2, the
owners of `crates/pob-calc-core/src/probe.rs`). Filed status: **pending**. FUP3 is limited to
`crates/pob-oracle` (no product-code changes), so it could not close these gaps itself; CS2's
bounds stay where they are (see §3). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `radiusJewelList[i].nodes` exports as `{"$userdata": true}`

Evidence. `env.radiusJewelList[i].nodes` is `node.nodesInRadius[radiusIndex]`
(CalcSetup.lua 888/915, `crates/pob-calc-setup/src/modules/calc_setup/init_env_items.rs:238-249`
`in_radius`, `:289-291` `radius_entry`), which CCR-0024 §3 types as a bare
`Val::Obj(Rc<LjIntMap<NodeRef>>)` (`crates/pob-calc-core/src/classes/passive_tree.rs:16-17,
130-132`). The exporter only knows the boxed form: `PObj::from_val`
(`crates/pob-calc-core/src/probe.rs:1415-1428`, `try_box!(… LjIntMap<NodeRef> => NodeMap …)`
downcasts `SBox<LjIntMap<NodeRef>>`), so the bare map falls through to
`{"$userdata": true}` (`probe.rs:1973-1975`). PoB's dump writes the node-id table
(`{"$num": {"<id>": {"$node": id}, …}}` or a `$ref` to it). `tests/CS2.rs` compares the id sets
itself (`radius_nodes_equal`) and excuses the paths: 10,675 mismatches on the two corpus/CS2
builds alone (FUP3 run, 2026-11-12), 129,887 on all57 + corpus/CS2 (CS2's measured bound).

**Proposed diff** (`probe.rs`, value export, before the `$userdata` fallback):

```rust
if let Some(m) = o.downcast_ref::<LjIntMap<NodeRef>>() {
    // a bare node-id map (CCR-0024 §3: nodesInRadius[i]): an inline table of node markers,
    // keys in LuaJIT `next` order, as state_dump.lua writes the node table it stands for
    return Some(node_map_json(m));
}
```

with `node_map_json` writing `{"$num": {"<id>": {"$node": id}}}` (the `NodeMap` member shape,
`probe.rs:1774-1778`). Identity: PoB's table is shared with `node.nodesInRadius[i]`, which the
exporter does not reach, so an inline value is what the dump's canonical-path rule gives for a
first occurrence. With it, CS2's `radiusJewelList.nodes` bucket goes to 0 and the
`radius_nodes_equal` excuse can go.

## 2. A second pass's `override.extraJewelFuncs` actor

Evidence (FOLLOWUPS "CS2" 3, `tests/CS2.rs` `run_hit`, the "$foreign actor" arms of the
bucket classification). On a restart's recursive call PoB's `override.extraJewelFuncs` store
(CalcSetup.lua:1425-1431) has pass 1's player as its actor; the injected store's actor is a
placeholder of that player (`$foreign` at injection), and on a `Carry` it is Rust's live pass-1
player. Either way the exported actor and PoB's dumped one do not line up path by path (PoB's
canonical paths for that player's tables run through `override.extraJewelFuncs.actor`), so
CS2 excuses every path under it: 899 mismatches on corpus/CS2 (FUP3 run), 17,414 on all57 +
corpus/CS2. Closing it needs the exporter to write a store's actor that belongs to no exported
env in full at the store's `actor` path (as `state_dump.lua` `inlineForeign` does for actors),
with the canonical-path rule of the dump, and the injector to build that actor instead of a
placeholder. This is a design change to the identity rules of `export_root`; it is proposed
here for ORC-X2 to scope, not specified.

## 3. What FUP3 did instead

CS2's `EXPLAINED_MAX` keeps its measured values (they are upper bounds and the FUP3 full-suite
run stayed at or below them; numbers in the FUP3 commit message / FOLLOWUPS "CS2" 3). FUP3 did
close FOLLOWUPS "CS2" 2 (dumped closures now carry their upvalues: pob-state/2 `$up`), which
removed CS2's `restore_jewel_func_args` workaround.
