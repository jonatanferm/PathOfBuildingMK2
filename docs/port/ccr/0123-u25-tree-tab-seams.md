# CCR-0123: U25 Tree tab seams (spec undo, the load-time ResetUndo, tattoo ids, PoEURL, U26/U27 hooks)

Unit: **U25** (TreeTab core). Status: **pending** (the code on `port/U25` follows it).

U25 ports the UI half of `TreeTab.lua:1-1046`, `PassiveSpecListControl.lua` and
`PassiveMasteryControl.lua` (`pob-app/src/tabs/tree/{core.rs,popups.rs,spec_list.rs,mastery.rs}`).
Its code reaches model items that are missing or frozen in a shape the Lua does not have, and code
of units not ported yet (U26, U27, U33). Each item below says what U25 does now (marked
`// CCR-0123` or a `DIVERGENCE(...)` note in the file header) and what it proposes.

## 1. The spec's UndoHandler lives in the UI, per spec (doc amendment)

Evidence: PassiveSpec.lua:22 (`newClass("PassiveSpec", "UndoHandler")`), :28 (`self:UndoHandler()`),
:194 (`self:ResetUndo()` at the end of `Load`); UndoHandler.lua:17-58; every caller uses
`self.build.spec:AddUndoState()/Undo()/Redo()` (Build.lua:263-296, TreeTab.lua:140, 366, 370,
709, 728, 937, 945, 1014, PassiveTreeView.lua, ImportTab.lua:1212). calc:PS-A leaves the buffers to
the UI (`crates/pob-calc/src/classes/passive_spec.rs:419`, `:805`).

ui doc §10.3 places the stack in "`BuildMode` state `undo_spec`": one stack. PoB has one stack
**per spec** (switching trees keeps each tree's history, and `spec.modFlag` is the active spec's).
U25 keeps one `UndoStack<Table>` per `PassiveSpecRef` (pointer identity) in its TreeTab state
(`tabs/tree/core.rs`, `State::spec_undo`, `SPEC_UNDO: UndoOps` resolving the active spec), and
installs `BuildHooks::spec_add_undo_state` and the `TabFlag::Spec` flag hooks.

Proposed diff (ui doc §10.3 table row):

```diff
-| `PassiveSpec` (`build.spec:AddUndoState`; PassiveSpec.lua is an UndoHandler) | calc:PS-A (state fns); U25 wires `UndoOps` | `BuildMode` state `undo_spec` |
+| `PassiveSpec` (`build.spec:AddUndoState`; PassiveSpec.lua is an UndoHandler) | calc:PS-A (state fns); U25 wires `UndoOps` | TreeTab node state, one stack per spec (`tabs/tree/core.rs` `State::spec_undo`; CCR-0123 §1) |
```

## 2. `ResetUndo` at the end of `PassiveSpec:Load` (model hook)

Evidence: PassiveSpec.lua:194; `passive_spec.rs:805` (`// L194: self:ResetUndo() — the undo stack
is the UI's`). calc:BM-A's `TreeTab:Load` runs inside `Init_model`, followed by PostLoad
(`BuildClusterJewelGraphs`, TreeTab.lua:529-534), before the UI constructor exists.

Workaround (`DIVERGENCE(reset-undo)` in core.rs): `tabs::tree::core::ctor_hook` runs `ResetUndo`
for every spec of the list when the build file has a `Tree`/`Spec` section. The first undo state
then holds the allocation after PostLoad; PoB's holds it before. They differ only for specs with
cluster-jewel subgraph nodes (`BuildClusterJewelGraphs` allocates them in PostLoad), where an
Undo right after loading restores the post-PostLoad allocation instead of the pre-PostLoad one.

Proposed: a UI hook in the model, called where Lua calls `ResetUndo`:

```rust
// crates/pob-calc/src/classes/passive_spec.rs (PS-A)
thread_local! { static RESET_UNDO_HOOK: RefCell<Option<Rc<dyn Fn(&PassiveSpecRef)>>> = .. }
pub fn set_reset_undo_hook(f: Option<Rc<dyn Fn(&PassiveSpecRef)>>);
// PassiveSpec:Load, L194: if let Some(f) = hook { f(&self_rc) }
```

U25 would queue the specs (the hook has no `Cx`) and create their states with the state Load left.
Not blocking: no U25 scenario undoes a cluster-jewel build right after loading.

## 3. `Node::id` is immutable: ModifyNodePopup's tattoo aliasing

Evidence: TreeTab.lua:886-889 (`newTattooNode.id = selectedNode.id`;
`hashOverrides[selectedNode.id] = newTattooNode`: the shared `tree.tattoo.nodes[...]` table
itself). `crates/pob-calc/src/classes/mod.rs` `Node { pub id: f64, .. }`.

Workaround (`DIVERGENCE(tattoo-alias)` in popups.rs): a `copy_node` of the tattoo node with the
selected node's id, as `PassiveSpec:Load` builds overrides (PassiveSpec.lua:181). PoB-visible
difference: when two nodes carry the same tattoo, PoB's two overrides are one table whose `id` is
the last node's (so its `Save` writes the same `nodeId` twice); the port keeps both ids. Proposed:
keep the divergence (a PoB aliasing bug) and record it in `known_diffs.toml` when a T4 scenario
reaches it; alternatively `id: Cell<f64>` (calc:PT/PS owners).

## 4. PoEURL redirect resolution (`LaunchSubScript`)

Evidence: TreeTab.lua:769-795 (a curl sub-script that returns `INFO_REDIRECT_URL`).
`pob_sg::Sys::net` only downloads pages (`platform/net.rs`, U16).

Workaround (`DIVERGENCE(poeurl-resolve)` in popups.rs): `LaunchSubScript` yields no id, which is
what PoB does when sub-scripts are unavailable: the popup shows "Resolving PoEURL link..." and
the Import button stays disabled. Proposed: `Sys::net.resolve_redirect(url) -> u64` with its
completion delivered like `download_page` (CCR-0026 owner, U7/U16). poeurl.com is defunct, so
this is low priority.

## 5. Power builder callbacks need `&mut Cx`

Evidence: TreeTab.lua:292-324 (`self.build.powerBuilderProgressCallback`, `powerBuilderCallback`:
toasts, `powerReportList.label`, `BuildPowerReportList`, `SetReport`);
`crates/pob-calc/src/modules/power.rs:127-141` (`set_powerBuilderCallback(build,
Option<Rc<dyn Fn()>>)`).

The model's callbacks take no `Cx`, so U25 cannot register its closures there. U25 exports
`tabs::tree::core::{power_builder_progress_callback, power_builder_callback}(cx, me, ..)`; U26,
which steps the PowerBuilder where PoB resumes the coroutine, calls them at the points calc:PWR
calls the model callbacks (or registers thin model callbacks that queue the events for its own
`Cx`-side drain). No model change requested.

## 6. Seams for U26/U27 (`TreeTabHooks`, core.rs) and the installed hooks

```rust
#[derive(Clone)]
pub struct TreeTabHooks {
    pub set_power_calc: Rc<dyn Fn(&mut Cx, CtrlId, Option<Table>)>,              // U26 SetPowerCalc
    pub power_report_list: Rc<dyn Fn(&mut Cx, CtrlId, Anchor, [Prop<f64>; 4], JumpFn) -> CtrlId>, // U26
    pub power_report_list_set_label: Rc<dyn Fn(&mut Cx, CtrlId, &str)>,            // U26
    pub power_report_set: Rc<dyn Fn(&mut Cx, CtrlId, Table)>,                     // U26 BuildPowerReportList + SetReport
    pub find_timeless_jewel: Rc<dyn Fn(&mut Cx, CtrlId)>,                         // U27
}
pub fn default_tree_tab_hooks() -> TreeTabHooks;
pub fn tree_tab_hooks() -> TreeTabHooks;
pub fn set_tree_tab_hooks(f: impl FnOnce(&mut TreeTabHooks));
```

Defaults: the power report list is a hidden `PlainControl` (`powerReportList`, L288), the label
setter is a no-op, the user-triggered methods raise a Lua error naming the owner. U26/U27 replace
their entries (one line each).

U25 replaced its placeholder entries (one line each, the CCR-0098 §3 / CCR-0104 §1 / CCR-0115
pattern): `init::default_hooks` (`spec_add_undo_state`, `tree_get_spec_list`,
`tree_set_active_spec`, `tree_tab`, the `TreeView` arm of `load_ui_section`),
`on_frame::default_shell_hooks` (the `Tree` arm of `draw_tab`, the `Tree`/`TreeSearch`/`Spec` arms
of `tab_flag`/`set_tab_flag`, the `TreeView` arm of `save_ui_section`), `tree_view::default_hooks`
(`open_mastery_popup`, `modify_node_popup`).

## 7. ItemsTab's `specSelect` (U33)

Evidence: TreeTab.lua:579, PassiveSpecListControl.lua:124-132
(`self.build.itemsTab.controls.specSelect`). The ItemsTab UI constructor is U33's and not ported.
Workaround (`DIVERGENCE(items-spec-select)`): `tabs::tree::core::items_spec_select` looks the
dropdown up as the ItemsTab node's `specSelect` child and skips the update when there is none, so
U33 needs no change here.

## 8. `SetActiveSpec`'s UI lines 556-557 (restates CCR-0098 §6)

`tabs::tree::core::TreeTab::SetActiveSpec` runs calc:BM-A's model half (550-555, 558-577) and
then 556-557 (window title, `UpdateClassDropdowns`) and 578-584 (`DIVERGENCE(set-active-spec-order)`).
CCR-0098 §6's model hook would let the UI lines run in Lua's place; the reorder is unobservable
today (558-577 only move jewels between item slots).

## 9. `pob-app/src/model.rs` re-exports (U15)

U25 names `pob_calc::classes::passive_spec::{new_PassiveSpec, copy_node}` and
`pob_calc::modules::power` (`powerStat`, `powerStat_table`, `nodePowerMaxDepth`,
`set_nodePowerMaxDepth`) directly, as U23/U39 do (CCR-0103 §4, CCR-0115). Proposed: re-export
them from `model.rs`.
