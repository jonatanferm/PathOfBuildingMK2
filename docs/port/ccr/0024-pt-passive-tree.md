# CCR-0024 (PT): PassiveTree representation notes, the `tree.nodes` order and the tree-dependent uniques

**Status: applied 2026-11-10 by CCR2 (§1-§4, §6, §8, §9: contracts.md#class-census, calc doc §3.4/§4.4); §5 and §7 need code (pob-dump). See AMENDMENTS A8.**

**Filed status: proposed (PT, 2026-10-10).** §1-§4, §6, §8 and §9 are conventions PT already follows
inside its own file `crates/pob-calc/src/classes/passive_tree.rs` (no frozen file changed; the
consumers PS-A/PS-B/PS-C, CS0, MP7 and U23 read the tree through them). §5 and §7 ask P1
(`crates/pob-dump`) for two dump additions; PT works around both locally (marked `CCR-0024`).

- **Raised by:** PT
- **Affects:** `crates/pob-calc/src/classes/passive_tree.rs` (PT), `crates/pob-calc/src/classes/mod.rs`
  `Node` / `PassiveTree` (U0, frozen: no change requested, only the conventions below),
  `crates/pob-dump` (P1: §5, §7), `pob_gamedata::DataSel` (U0: §8, note only), consumers of
  `node.nodesInRadius` (PS-A/PS-B 1067-1078/1174-1197/1560/2488, CS0 CalcSetup.lua:888-925),
  U23 (`tree_view/assets.rs`)
- **Pin:** 16de4b82d

## 1. String node ids (legion and tattoo nodes)

`Node.id` is `f64` (frozen), but `LegionPassives.nodes[i].id` and `TattooPassives.nodes[k].id`
are strings (`"vaal_keystone_1"`, `"karui_notable_add_strength"`). PassiveTree.lua reads them at
:783 (`self.tattoo.idMap[node.id] = node.dn`) and :864/:870 (`"Tree:"..node.id`,
`"Tree"..node.id`).

Convention: such a node has `Node.id = NaN` and the string in `node.extra.id`.
`passive_tree::node_id_val(&Node) -> Val` returns the Lua value of `node.id` (number or string);
every port that reads a legion/tattoo node's id MUST use it.

## 2. `nil` in the `Cell<f64>` position fields

`x`, `y`, `o`, `oidx` and `angle` are `Cell<f64>` (frozen), but Lua leaves them `nil` on nodes
without a group (cluster notables, legion/tattoo nodes, mastery effects): PassiveTree.lua:898
derives a position only `if node.group`. Convention: `nil` is `f64::NAN`.
`passive_tree::node_xy(&Node) -> Option<(f64, f64)>` is `node.x and node.y`. Where Lua does
arithmetic on a nil position it raises; the port raises the same error (`lua_error!`, e.g. the
socket bounding box at :667-668), it never computes with NaN silently.

## 3. `node.nodesInRadius[radiusIndex]`

`nodesInRadius` is `Field<Option<Table>>` (frozen). Its elements are node-id maps whose
`pairs()` order is observable (pairs_inventory.md rows for PassiveSpec.lua:1078 and
CalcSetup.lua:898/925: **int**), so each element is a `Val::Obj(Rc<LjIntMap<NodeRef>>)` in that
table (keys 1..#data.jewelRadius), built by inserting in the Lua order (:678/:705 into a fresh
`{ }`). Read it with `passive_tree::nodesInRadiusAt(&Node, radiusIndex)`. The maps are never
written after the tree is built (every other reference in PoB only reads them).
`attributesInRadius[radiusIndex]` stays an empty `Table` (PoB never fills it).

Possible follow-up (not needed now): type the field as
`Field<Option<Rc<Vec<Rc<LjIntMap<NodeRef>>>>>>`.

## 4. Untyped keys and nodes inside dynamic tables

A node stored in a dynamic `Table` (`keystoneMap`, `notableMap`, `ascendancyMap`,
`clusterNodeMap`, `masteryEffects`, `legion.nodes`, `tattoo.nodes`) is `Val::Obj(NodeRef)`
(`passive_tree::node_val`; read back with `v.obj::<Node>()`), so identity (`Rc::ptr_eq`) is kept.
Mastery effects (:561) are `Node`s too (`id`, `sd` and the ProcessStats outputs).

Raw tree keys that the census did not type stay in `node.extra` / `tree.extra` under their Lua
names: on nodes `skill`, `name`, `stats`, `orbit`, `orbitIndex`, `g`, `group` (the raw group id
until :614 replaces it; the port then clears `extra.group` and sets `Node.group`), `out`,
`in`, `isProxy`, `isBlighted`, `isAscendancyStart`, `m`, `ks`, `not`, `spc`, `icon`, …, plus
the ones PassiveTree.lua assigns (`startArt`, `overlay`, `rsq`, `size`, `bloodlineOverlayPrefix`,
`charmSocket`, `unknown`, `extra`); on the tree `groups`, `constants`, `min_x`/`max_x`/…,
`size`, `classNotables`, `nodeOverlay`, `points`, `jewelSlots`, …. `node.extra.extra` is the
Lua field `node.extra` (ProcessStats :845). The image-keyed members `assets` and `sprites`
(fractional zoom keys) are not loaded: they are U23's (ui doc, row U23).

## 5. `pairs(tree.nodes)` order (asks P1 for the source key order)

PoB iterates `tree.nodes` with `pairs` to build every derived structure; the order is
observable through the insertion order of each `nodesInRadius` map (§3: `LjIntMap` order
depends on insertion order) and of `classNotables`, and (for duplicate names) through which
node wins in `notableMap`/`keystoneMap`/`ascendancyMap`/`clusterNodeMap`.

`tree.nodes` is built by the `tree.lua` table constructor. Its integer keys with table values
are runtime stores in source order (lj_parse.c `expr_table`), so the layout depends on the
source order of the keys:

- **2_6 … 3_9** (no string key in `nodes`; 2_6 is a positional list): deterministic. With the
  source order the port's `LjIntMap` reproduces PoB exactly (test `PT.rs`,
  `PT_VERSIONS=2_6,3_6,3_7,3_8,3_9`): `pairs(tree.nodes)` 5/5, `nodesInRadius` 1904/1904,
  `classNotables` 35/35 identical. With the ascending order (what the port can do today):
  1/5, 456/1904, 7/35.
- **3.10+**: `nodes` also holds the string key `root` (assigned nil at :510). The template
  table keeps `root` in the hash part, where its slot depends on LuaJIT's per-state seeded
  string hash (lj_str.c `g->str.seed = lj_prng_u64`), so PoB's own order differs between
  processes (test `tree_nodes_order_varies_between_lua_states`, `--ignored`: 4 Lua states gave
  4 distinct `pairs(tree.nodes)` orders of the latest tree). The port puts a
  phantom key (`-1`, never an array candidate) where `root` would be and removes it at :510.

The dump does not record source order (FORMAT.md: members are written in sorted order), so
`new_PassiveTree` currently inserts in ascending id order. Over all 41 versions one run gave
7 846/39 720 identical `nodesInRadius` orders with the ascending order and 13 150/39 720 with
the source order (the 3.10+ part varies from run to run with PoB's seed; every source-order
mismatch is in a 3.10+ tree). The sets themselves (`nir` records) are identical everywhere.

**Proposed (P1):** `pob-dump` records, for every tree version, the source order of the keys of
`trees["<ver>"].nodes` (e.g. `trees/<ver>.json` gets `"$nodeOrder": [ids…]`, or a sibling
`trees/<ver>.order.json`), and pob-gamedata exposes it:

```rust
// pob-gamedata/src/data.rs
pub fn tree_node_order(version: &str) -> Option<std::borrow::Cow<'static, [i64]>>;
```

PT then calls `new_passive_tree_with_node_order(ver, tree_node_order(ver).as_deref())` from
`new_PassiveTree` (the function exists; `PT.rs` already feeds it the order parsed from
`TreeData/<ver>/tree.lua`). Until then, the old trees' `nodesInRadius` orders are not PoB's.

## 6. `node.sd` is a copy of `node.stats`

3.10+ (:535) `node.sd = node.stats` aliases one table; ProcessStats (:810-817) splits
multi-line entries in place, so after the load `node.stats` is split too. The port's `sd` is a
`Shared<Vec<LStr>>` copy; `node.extra.stats` keeps the raw (unsplit) list. No reader of a tree
node's `stats` exists in scope (TreeTab.lua:862 and PassiveSpec.lua:1261/1290/1327/1380 read
legion/tattoo `stats`, which are keyed by stat id and are never split). Mastery effects: :561
`sd = effect.stats` (alias) and :565 `effect.stats = …sd`: the port writes the split list back
into `effect.stats` (same content).

## 7. Tree-dependent generated uniques (asks P1)

:789-791 calls `buildTreeDependentUniques(self)` for the latest tree
(Data/Uniques/Special/Generated.lua:548-666). It (a) sorts every class's `classNotables` list in
place (`buildForbidden`, :568-570, :584-586) and (b) appends four items to
`data.uniques.generated`: Forbidden Flame, Forbidden Flesh, Skin of the Lords, Impossible
Escape. PoB always has them: Main.lua:152 loads the latest tree before `loadItemDBs`
(Main.lua:163-170).

`pob_gamedata::Data` is immutable, so PT ports only (a). The dump lacks (b): `data/dump/uniques.json`
`generated` has 11 entries (Megalomaniac … Pearl of Tsoatha), none of the four.

**Proposed (P1):** `pob-dump` calls `buildTreeDependentUniques(<latest PassiveTree>)` (or
`main:LoadTree(latestTreeVersion)`) before serialising `data.uniques`, so `uniques.generated`
ends with the four items in PoB's order.

## 8. `setJewelRadiiGlobally` and `DataSel`

Data.lua:593-594 writes `outerSquared`/`innerSquared` into the shared rows of
`data.jewelRadii["3_15"|"3_16"]`; the port writes them into the copy held by the thread-local
`DataSel` (`data.jewelRadius`), leaving `Data.jewelRadii` untouched (no in-scope reader of
`data.jewelRadii` uses the squares). Note for U0: after Data.lua:630
(`data.jewelRadius = data.setJewelRadiiGlobally(latestTreeVersion)`, which returns nothing)
PoB has `data.jewelRadius == nil` and `data.maxJewelRadius` = the 3_16 value until the next
`main:LoadTree`; `DataSel::default()` has an empty list and `0`. Main.lua:152 always loads a tree
first, so this is not observable; PT raises the Lua error (`ipairs` on nil) when a tree is built
without a selection.

## 9. Smaller points

- `require("Data.TimelessJewelData.LegionPassives")` / `require("Data.TattooPassives")` (:79-80)
  are cached per Lua state, so every tree shares and re-processes one legion and one tattoo
  table (`:733-786` replace each node's `modList`, `:727-729` rename abyss additions). The port
  keeps one thread-local copy (`require_legion_tattoo`), built from the dump.
- ProcessStats `if not node.sd then return end` (:803): the port's `sd` is never nil (a node or
  mastery effect without stats gets an empty list, which leaves the same `modKey`/`mods`/`modList`).
- `node.mods` (frozen `Shared<Vec<Table>>`) is hole-free after ProcessStats, as in Lua; the
  transient holes while lines are combined (:833 before :851) live in a local.
- `getFile` (39-50, a disabled curl download) is `@pob-skip`ped and returns `None`; the tree
  comes from `pob_gamedata::tree_bytes` (A2), so 82-111 (tree.lua / data.json file I/O) is
  skipped too. The image, sprite and connector blocks inside 75-794 (234-435, 626-627,
  635-645, 748-753, 771-777, 876-885) are `@pob-skip`ped for U23.
- `new_passive_tree_with_node_order` is an extra public constructor (§5; tests).
