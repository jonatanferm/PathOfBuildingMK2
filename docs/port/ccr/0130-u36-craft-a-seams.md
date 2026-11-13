# CCR-0130: U36 crafting A seams

**Status: applied (CCRDOC, 2026-11-12; code `faf8539` Merge port/U36): §1, §2 and §4; §3 was applied by U53 (CCR-0145 §5).**

Unit: **U36** (ItemsTab crafting A, `Classes/ItemsTab.lua:2609-3120` and
`Modules/AddImplicitPopup.lua` 1-414; `pob-app/src/tabs/items/{craft_a.rs,add_implicit.rs}`).
Filed status: **pending**. Code on `port/U36` follows it. Number reserved with
`scripts/port/next_ccr.sh`.

## 1. U36's hook entries (applied, one line each)

U33's `ItemsUiHooks` (CCR-0120 §1) has seven U36 entries. `default_items_ui_hooks` (ctor.rs) now
points them at the ports: `craft_a::{OpenItemSetManagePopup, CraftItem, EditDisplayItemText,
AppendAddedNotableTooltip}`, `EnchantDisplayItem`/`AnointDisplayItem` (the hooks pass the slot
as `f64`; the ports take Lua's `enchantSlot or 1` as `Option<f64>`), and
`add_implicit::AddImplicitToDisplayItem`. The signatures are unchanged.

## 2. Ported methods of other units are called directly

As CCR-0124 §2 proposes: `display_item::{SetDisplayItem, CreateDisplayItemFromRaw,
AddModComparisonTooltip}` (U35; AddImplicitPopup.lua:365 passes `replaceImplicits =
value.type == "vestigial"`) and the model's `ItemsTab::getAnoint` (calc:BM-A, ItemsTab.lua:
2978-2993, whose marker stays in pob-calc) are called as `pub fn`s. `AddItemTooltip` (U38) still
goes through U32's `ItemsTabHooks::add_item_tooltip` and `AddDisplayItem` (U34) through
`ItemsUiHooks::add_display_item`, the entries the oracle tests replace.

## 3. Finding for U33/U24 (not changed here): `socket_viewer_add_node_name` still raises

Evidence: `ctor.rs` `default_items_ui_hooks` keeps `socket_viewer_add_node_name` as
`not_ported("PassiveTreeView:AddNodeName", "U24")`, although U24 is merged
(`tree_view/names.rs` `PassiveTreeView::add_node_name`). The constructor's cluster jewel affix
tooltip (ItemsTab.lua:893-896) reaches it. U36's anoint popup needs the same call
(NotableDBControl.lua:247 via `itemsTab.socketViewer:AddNodeName`): U36's `NotableItemsTab`
adapter lends `ctor::State::socketViewer` and calls `add_node_name` directly.

Proposed (U33's `ctor.rs`, one line): `socket_viewer_add_node_name: Rc::new(|cx, me, tt, node|
cx.lend::<ItemsTab, _, _>(me, |w| &mut w.ctor.socketViewer, |cx, v| v.add_node_name(cx, tt,
node)))`.

## 4. `data.itemBaseLists` entries

`data.itemBaseLists` is an untyped dump root (`Data::other`); each entry's `base` is a `$ref` to
`data.itemBases[name]`, which the loader resolves to an untyped table. CraftItem needs the typed
`ItemBaseRef` (it becomes `item.base`), so the port reads the entry's `name` and takes
`data().itemBases[name]`, which is the same table in Lua (Data.lua:1259 stores `base =
data.itemBases[name]`). No change proposed; a typed accessor for the two roots would remove the
lookup.
