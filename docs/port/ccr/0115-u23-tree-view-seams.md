# CCR-0115: U23 tree view seams (hooks into U24/U25/U26/U32-34, model re-exports, applied seams)

Unit: **U23** (tree assets and view). Status: **pending** (the code on `port/U23` follows it).

U23's range (`PassiveTreeView.lua:1-1411`, the image/sprite/connector parts of `PassiveTree.lua`,
`Main.lua:352-364`) calls code later units own and reads calc-plan items `pob-app/src/model.rs`
(U15) does not re-export. U23 adds the seams below in its own files (`// CCR-0115`), so the
owners plug in by one line each, and applies two seams earlier CCRs reserved for it.

## A. `TreeViewHooks` (`pob-app/src/tree_view/mod.rs`, U23)

Evidence (PassiveTreeView.lua): `:1052` `self:AddNodeTooltip(self.tooltip, node, build)` and
`:1157` `self:AddCompareNodeTooltip(...)` (U24, `:1476-1830`, `tree_view/tooltip.rs`, whose stub
has no function yet); `:512`, `:538` `build.treeTab:OpenMasteryPopup(hoverNode, viewPort)` and
`:535`, `:541` `build.treeTab:ModifyNodePopup(hoverNode, viewPort)` (U25, TreeTab.lua);
`:785` `build.calcsTab:BuildPower()` (the per-frame call site U26 owns, ui doc §12 U26 row);
`:523-528` the jewel-socket right click (`build.itemsTab.sockets[id]` is U32's
ItemSlotControl: `slot:IsEnabled()`, `slot.dropped = true`, `itemsTab:SelectControl(slot)`,
U33/U34).

```rust
#[derive(Clone)]
pub struct TreeViewHooks {
    pub add_node_tooltip: Rc<dyn Fn(&mut Cx, &mut PassiveTreeView, &mut Tooltip, &NodeRef)>,
    pub add_compare_node_tooltip: Rc<dyn Fn(&mut Cx, &mut PassiveTreeView, &mut Tooltip, &NodeRef)>,
    pub open_mastery_popup: Rc<dyn Fn(&mut Cx, &NodeRef, &ViewPort)>,
    pub modify_node_popup: Rc<dyn Fn(&mut Cx, &NodeRef, &ViewPort)>,
    pub build_power: Rc<dyn Fn(&mut Cx)>,
    pub socket_right_click: Rc<dyn Fn(&mut Cx, f64)>,   // the node id
}
pub fn default_hooks() -> TreeViewHooks;   // tooltips empty, popups raise naming the owner,
                                           // build_power = calc:PWR's CalcsTab:BuildPower
pub fn hooks() -> TreeViewHooks;
pub fn set_hooks(f: impl FnOnce(&mut TreeViewHooks));
```

The tooltip hooks receive the view and the tooltip separately: the view lends `self.tooltip`
out (`std::mem::take`) for the call, so U24's `AddNodeTooltip(tooltip, node, build)` can read the
view's other fields (`skillTooltip`, `showStatDifferences`, `tracePath`) through `&mut
PassiveTreeView`. Proposed: U24, U25, U26 and U32/U34 replace their entry in
`tree_view::default_hooks` (U24 may instead give `PassiveTreeView` the methods in its own files
and point the default at them).

`spec:AddUndoState()` uses the existing `BuildHooks::spec_add_undo_state` (U19, U25 installs it);
`spec:SetWindowTitleWithBuildClass()` uses U19's `init::set_window_title_with_build_class`.

## B. Model items not re-exported by `model.rs` (U15)

The view calls these directly from `pob_calc` (marked `// CCR-0115`):

| Item | Used for |
|---|---|
| `pob_calc::classes::passive_spec::main_LoadTree` | Main.lua:359 (the model half of `main:LoadTree`; `build/class_drop.rs` calls it too, CCR-0104 §4) |
| `pob_calc::classes::passive_tree::{setJewelRadiiGlobally, nodesInRadiusAt, node_id_val}` | Main.lua:354/357; PassiveTreeView.lua:994/1005 (`hoverNode.nodesInRadius[index][node.id]`); :1401 (`tostring(node.id)`) |
| `pob_calc::modules::power::{BuildPower, powerStat, powerMax}` | PassiveTreeView.lua:785-786, :922, :934-935 (`build.calcsTab.powerStat/powerMax` live in the build side table, CCR-0101 §1) |

Proposed diff to `pob-app/src/model.rs`:

```diff
+pub use pob_calc::classes::passive_spec::main_LoadTree;
+pub use pob_calc::classes::passive_tree::{node_id_val, nodesInRadiusAt, setJewelRadiiGlobally};
+pub mod power { pub use pob_calc::modules::power::{BuildPower, powerMax, powerStat}; }
```

## C. Seams of earlier CCRs, applied by U23

* CCR-0096 B: `main_mode/init.rs::default_hooks().load_tree` now calls
  `tree_view::assets::main_load_tree` (Main.lua:352-364: the model tree plus its UI images in
  `cx.app().trees`).
* CCR-0099 A: `main_mode/background.rs::tree_background` is the lookup
  `main.tree[latestTreeVersion].assets.Background2/Background1` in `cx.app().trees`
  (`tree_view::assets::latest_tree_assets`, `TreeAssets::asset_handle`).

## D. Notes for the owners of neighbouring units

* `TreeCache` (U16's `app.rs`) keeps `(PassiveTreeRef, Rc<TreeAssets>)` per version, as declared.
  The model's own tree cache (`passive_spec::MAIN_TREE`) is separate and per thread; a spec the
  calc creates for a tree other than the latest gets its UI images at the view's first draw
  (`tree_view::assets::tree_assets`, DIVERGENCE(load-order) in `assets.rs`).
* `require("TreeData.3_19.Assets")` is cached per thread like Lua's `require` per process; the
  3.19+ trees share its asset objects (`Rc<Asset>`, `LoadImage` replaces their handle), as PoB's
  shallow `copyTable` does.
* `subGraph.connectors` (CCR-0072 §3) node pairs are turned into connector polygons when drawn
  (`tree_view::connectors::build_subgraph_connectors`).
* The oracle's `ImageLoad` side effects are recorded only for the Lua side
  (`NewImageHandle():Load` through the bridge), not for `pob_sg::ImageHandle::load`; U23's test
  checks the images through the draw ops and `u23_assets` instead.
