# CCR-0120: U33 ItemsTab constructor seams

Unit: **U33** (ItemsTab A, `Classes/ItemsTab.lua:1-1215`, `pob-app/src/tabs/items/ctor.rs`).
Status: **pending** (code on `port/U33` follows it, with the local workarounds marked
`// CCR-0120`). Number reserved with `scripts/port/next_ccr.sh`.

## 1. `ItemsUiHooks`: the methods the constructor's closures call

Evidence: the constructor's closures call ItemsTab methods of U34-U38 (`AddDisplayItem` L342,
`SelectDisplayItemVariant` L403-441, `UpdateDisplayItemVariantControls` L393,
`UpdateDisplayItemTooltip` (17 sites), `UpdateDisplayItemRangeLines` L395/L1047,
`UpdateSocketControls` L509, `CraftClusterJewel` L705/L717, `UpdateAffixControls` L727/L813,
`UpdateCustomControls` L1046/L1082/L1107/L1148, `AddModComparisonTooltip` L927,
`GetComparisonSlotNameForItem` L356, `OpenItemSetManagePopup` L127, `CraftItem` L311,
`EditDisplayItemText` L317/L348, `EnchantDisplayItem` L521/L527, `AnointDisplayItem`
L533-557, `AppendAddedNotableTooltip` L896, `CorruptDisplayItem` L566,
`AddCustomModifierToDisplayItem` L995, `AddCrucibleModifierToDisplayItem` L1006,
`addImplicit.AddImplicitToDisplayItem` L572) and of other objects (`self.socketViewer:AddNodeName`
L870, U24; `self.build.treeTab:OpenSpecManagePopup` L216, U25; `self.tradeQuery:PriceItem` L132
and `buySimilar.openPopup` L357, U51). U32's `ItemsTabHooks` (CCR-0107 §1) covers only what
U32's controls call (`SetActiveItemSet`, `AddUndoState`, `SetDisplayItem`, `AddItemSetTooltip`,
...), which the constructor uses for those.

Applied locally (U33's own file): `ctor::ItemsUiHooks` + `items_ui_hooks()` /
`set_items_ui_hooks(..)` / `default_items_ui_hooks()`, every entry `(cx, me: the ItemsTab node,
..)` raising "… is not ported yet (Uxx)" by default. Proposed: each owner replaces its entry in
`default_items_ui_hooks` (one line each), as with CCR-0107 §1.

## 2. Where the shared ItemsTab fields and the `Widget` impl live

Ui doc §12.1 puts the class's `Widget` impl with the owner of its `Draw` (U34). BuildMode draws
every tab through `ShellHooks::draw_tab` (CCR-0104 §1), so the impl has no methods
(`CAPS = empty`, as `NotesTab`/`CalcsTab`); the constructor (`cx.add_free(.., ItemsTab::new(..))`)
needs it first, so it is in `ctor.rs`. The fields that U34-U38 write and the constructor's
closures read (`displayItem`, `maxY`, `snapHScroll`, `displayItemTooltip`, `anchorDisplayItem`,
the slot/socket control maps, the dropdowns' extra Lua fields `outputTable`/`outputIndex`/
`newVariantVisible`/`newVariantEnabled`/`variantGroupId`, the stacked labels' `lineCount`) are
`ctor::State` fields, reached as `cx.w::<ItemsTab>(me).ctor.<field>`. Proposed: U34 keeps the
impl where it is (adding nothing), and the later units use these fields instead of declaring
their own copies.

`self.lastSlot` is calc:BM-A's (`build_extra(build).itemsTab.lastSlot`, a model slot table set by
`ctor_model` L1212 and by the model's `UpdateSockets` L1673-1676); `ctor::lastSlot(cx, me)` maps
it to the slot control by `slotName`.

## 3. `itemLib` (calc:IT-B's `item_tools`) is not re-exported by `pob-app/src/model.rs`

Evidence: the constructor reads `itemLib.influenceInfo.all/default` (L45, L589) and calls
`itemLib.applyRange` (L956); `model.rs` (U15) re-exports `pob_calc::classes::{Item, ...}` but not
`item_tools`. Workaround: `use pob_calc::classes::item_tools::{applyRange, influenceInfo_all,
influenceInfo_default}` in `ctor.rs`, marked `// CCR-0120 §3` (as CCR-0103 §4 did for `power`).
Proposed diff (U15's file):

```diff
 pub use pob_calc::classes::{
     Item, ItemRef, ModDB, ModList, Node, NodeRef, PassiveSpec, PassiveSpecRef, PassiveTree,
     PassiveTreeRef,
 };
+/// `itemLib` (Modules/ItemTools.lua, calc:IT-B)
+pub use pob_calc::classes::item_tools as itemLib;
```

## 4. `self.socketViewer = new("PassiveTreeView"):PassiveTreeView()` (L100)

`crate::tree_view::PassiveTreeView` is U23's (not merged when U33 was written; its branch has
`PassiveTreeView::new(cx)`). `ctor::State::socketViewer` holds `PassiveTreeView::default()`;
U23 replaces it with its constructor at the L100 comment of `ItemsTab::new` (one line). The
viewer is only used through U32's `ItemsTabHooks::socket_viewer_draw` and this CCR's
`socket_viewer_add_node_name` until then.

## 5. The `items_tab` arm of `init::default_hooks`

Per CCR-0106 §2, `BuildHooks::items_tab` now defaults to `crate::tabs::items::ctor::ctor_hook`
(one line in U19's `init.rs`). `ShellHooks::draw_tab` gets no `Items` arm: `Draw` is U34's. Until
U34 lands, the Items tab is constructed (every control, the slot lists) but draws nothing, as
before.

## 6. Notes for U34-U38

* `SetActiveItemSet(1)` inside the constructor (L1209) would call `build:SyncLoadouts()`
  (L1616) while PoB's `itemsTab`/`treeTab`/`skillsTab` are still nil; the port skips that call
  (DIVERGENCE(ctor-sync-loadouts) in `ctor.rs`): Build.lua:726 overwrites its result.
* The foulborn checkboxes' and range sliders' `shown` functions write `box.state`/`slider.val`;
  the port applies those writes in the controls' `Draw`/`OnKeyDown`/`OnKeyUp` overrides
  (DIVERGENCE(mutated-state), DIVERGENCE(range-slider-val)). U35's `UpdateDisplayItemRangeLines`
  writes `displayItemRangeSlider.val` directly, as the Lua does.
* `tests/u33_items_ctor.rs` has test-only ports of `Draw` (L1419-1572 without the shortcuts),
  `UpdateDisplayItemTooltip`, `UpdateSocketControls` and U32's `GetEquippedSlotForItem`; U34/U35
  can check their real ports against them.
