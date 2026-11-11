# CCR-0122: U24 tree tooltips (the bridge's node-id `removeNodes`, the compare build's ItemsTab, the hook wiring)

Unit: **U24** (`PassiveTreeView.lua:1412-1856`; files `pob-app/src/tree_view/{names.rs,tooltip.rs}`).
Status: **pending** (the code on `port/U24` follows it). Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes in U24's files. Two gaps in other units' surfaces, and the one-line
edit of U23's hook table that CCR-0115 A invited.

## 1. LuaCalcBridge cannot carry `override.removeNodes[<node id>]` (U15)

Evidence: PassiveTreeView.lua:1704-1706 (`elseif isGranted then nodeOutput = calcFunc({
removeNodes = { [node.id] = true } })`, a node allocated by an item, e.g. an anoint). CS2 reads
the id half of PoB's mixed set from `override.extra["$removeNodeIds"]` (a table id → true,
CCR-0105 §1, `calc_setup/init_env_items.rs:234-246`), and U24 fills it there
(`tree_view/tooltip.rs::remove_node_id`). The bridge's `override_lua`
(`pob-ui-oracle/src/lua_calc.rs:458-478`) only transfers scalar `override.extra` entries and
raises "LuaCalcBridge: override[\"$removeNodeIds\"] (table) cannot cross to the Lua calc" for it.

Effect: through the bridge, hovering a granted node (the allflame fixtures' anointed Force of
Darkness) raises on the Rust side, so `u24_scenarios` does not hover one (its module notes). The
Rust calc (`RustCalc`) is not affected.

Proposed (U15, `lua_calc.rs::override_lua`, after the `removeNodes` loop):

```diff
+    // CCR-0105 §1: the node-id half of `override.removeNodes` (PassiveTreeView.lua:1706)
+    if let Some(ids) = ov.extra.table(Name::from("$removeNodeIds")) {
+        if ov.removeNodes.get().is_none() {
+            s.push_str("o.removeNodes = { }\n");
+        }
+        for (k, v) in ids.pairs() {
+            if v.truthy() {
+                let _ = writeln!(s, "o.removeNodes[{}] = true", key_lua(&k));
+            }
+        }
+    }
     for (k, v) in ov.extra.pairs() {
+        if matches!(&k, Key::S(n) if n.as_str() == "$removeNodeIds") { continue; }
```

(or, with CCR-0105's proposed `Override.removeNodeIds` field, the same loop over that field).

## 2. `ItemsTabHooks::add_item_tooltip` has no build argument (U32 → U38, U46)

Evidence: PassiveTreeView.lua:1492 and :1783 call `self.compareSpec.build.itemsTab:AddItemTooltip`
(the compare spec's own build); L1501 calls `build.itemsTab:AddItemTooltip`. U32's hook
(`tabs/items/slot_helper.rs:88-90`) is `Fn(&mut Cx, &mut Tooltip, &ItemRef, Option<&Table>,
bool)`: it is the open build's ItemsTab. TreeTab's compare spec (TreeTab.lua:451) is a spec of the
open build, so U24's calls are PoB's there. CompareTab sets `viewer.compareSpec =
compareEntry.spec` (CompareTab.lua:3421), a spec of another loaded build.

Proposed for U38/U46 (whoever ports the CompareTab overlay first): give the hook the model build
whose ItemsTab is meant (`&model::BuildRef`), and U24 passes `compareSpec.build` at L1492/L1783
(`tooltip.rs` marks both call sites). Until then U24 calls the hook as is (module notes of
`tooltip.rs`).

## 3. Applied: U23's `default_hooks` point at U24's methods

CCR-0115 A: "U24 may instead give `PassiveTreeView` the methods in its own files and point the
default at them". `tree_view/mod.rs::default_hooks` now has

```rust
add_node_tooltip: Rc::new(|cx, view, tt, node| view.add_node_tooltip(cx, tt, node, false)),
add_compare_node_tooltip: Rc::new(|cx, view, tt, node| view.add_compare_node_tooltip(cx, tt, node)),
```

U24's public surface, for its other callers (TreeTab U25 with `returnEarly = true`,
PassiveMasteryControl U25, NotableDBControl/ItemsTab `socketViewer:AddNodeName` U36,
ComparePowerReportListControl U46):

```rust
impl PassiveTreeView {
    pub fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
    pub fn add_node_tooltip(&mut self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef, return_early: bool);
    pub fn add_compare_node_tooltip(&mut self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
    pub fn add_compare_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
}
```

The Lua `build` argument is the open build (`cx.build().model`) in every PoB caller, so the
methods take none.
