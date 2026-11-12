# CCR-0112: U28 timeless and notable lists (the `build`/`itemsTab` arguments, `math.random`, `matchOrPattern`)

**Status: applied (CCRDOC, 2026-11-12; code `8be58b6` Merge port/U28): the `TimelessBuild`, `SocketViewer` and `NotableItemsTab` traits (U53 filled the placeholders, CCR-0145 §5), §3 and §4. §2's move of the PRNG to `pob_lua::math` was not done: two LuaJIT `math.random` ports remain (FOLLOWUPS "CCRDOC" 8).**

Unit: **U28** (`TimelessJewelListControl` 1-352, `TimelessJewelSocketControl` 1-57,
`NotableDBControl` 1-293; files `pob-app/src/tabs/tree/{timeless_list,timeless_socket,notable_db}.rs`).
Filed status: **pending** (code on `port/U28` follows it). Number reserved with `scripts/port/next_ccr.sh`.

No frozen contract changes. This CCR records the seams U28 introduced inside its own files, which
the units that construct these controls (U27 for the timeless controls, U36 for the notable DB)
and the units whose methods they call (U23, U24, U32-U38) must know about, plus two shared helpers
it ported locally.

## 1. The constructor's `build` / `itemsTab` argument is a trait object

Evidence: `TimelessJewelListControl.lua:17` (`self.build = build`) and its reads of
`build.timelessData` (L18-19, L301, L340), `build.outputRevision` (L294), `build.spec.allocNodes`
(L302, L343), `build.buildFlag` (L345) and `build.itemsTab` (L293, L331, L338-349);
`TimelessJewelSocketControl.lua:20-21, 32, 44-49` (`build.spec.nodes`, `build.spec.tree.size`, the
`socketViewer` PassiveTreeView's `zoom`/`zoomX`/`zoomY` and `Draw`); `NotableDBControl.lua:27, 135-137,
142, 194, 201, 245, 247, 273` (`itemsTab.build.outputRevision`, `itemsTab.build.calcsTab:GetMiscCalculator()`,
`itemsTab.displayItem`, `itemsTab:anointItem`, `itemsTab.socketViewer:AddNodeName`,
`itemsTab:AppendAnointTooltip`). Of these, AddItemStatDifferences (U38), AddUndoState (U34), the
socket's SetSelItemId (U32), anointItem / AppendAnointTooltip / displayItem (U33-U36), AddNodeName
(U24) and PassiveTreeView (U23) are not ported, and the galleries need stand-ins for all of them.

What U28 did (its own files; nothing frozen changes):

```rust
// tabs/tree/timeless_list.rs
pub trait TimelessBuild {            // one method per Lua access of `build`
    fn timeless_data(&self, cx: &Cx) -> Table;
    fn output_revision(&self, cx: &Cx) -> f64;
    fn spec_alloc_node(&self, cx: &Cx, node_id: &Val) -> bool;
    fn spec_node(&self, cx: &Cx, node_id: &Val) -> Option<NodeRef>;
    fn spec_tree_size(&self, cx: &Cx) -> Val;
    fn set_build_flag(&self, cx: &mut Cx);
    fn new_item(&self, cx: &Cx, raw: Option<&str>) -> ItemRef;          // new("Item"):Item(raw)
    fn get_socket_and_jewel_for_node_id(&self, cx: &Cx, node_id: &Val) -> Option<Table>;
    fn sockets(&self, cx: &Cx, node_id: &Val) -> Option<Table>;
    fn add_item_stat_differences(&self, cx: &mut Cx, tooltip: &mut Tooltip, item: &ItemRef,
                                 base: Option<ItemBaseRef>, slot: &Val);  // slot: nil | false | socket
    fn add_item(&self, cx: &mut Cx, item: &ItemRef, no_auto_equip: bool);
    fn is_item_valid_for_slot(&self, cx: &Cx, item: &ItemRef, slot_name: &Val) -> bool;
    fn set_sel_item_id(&self, cx: &mut Cx, socket: &Table, sel_item_id: Option<f64>);
    fn populate_slots(&self, cx: &mut Cx);
    fn add_undo_state(&self, cx: &mut Cx);
}
pub struct AppTimelessBuild;         // the product's: cx.build()
// tabs/tree/timeless_socket.rs
pub trait SocketViewer { fn set_zoom(..); fn set_zoom_x(..); fn set_zoom_y(..); fn draw(&self, cx, vp); }
// tabs/tree/notable_db.rs
pub trait NotableItemsTab {
    fn build_output_revision(&self, cx: &Cx) -> f64;
    fn get_misc_calculator(&self, cx: &mut Cx) -> Rc<dyn MiscCalc>;
    fn display_item(&self, cx: &Cx) -> Option<ItemRef>;
    fn anoint_item(&self, cx: &mut Cx, node: Option<&NodeRef>) -> ItemRef;
    fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
    fn append_anoint_tooltip(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
}
```

`AppTimelessBuild` reads the model (`build_xml::timelessData`, `outputRevision`, `spec`,
`buildFlag`) and calls the model halves of the ItemsTab methods that exist (calc:BM-A:
`GetSocketAndJewelForNodeID`, `sockets`, `AddItem`, `IsItemValidForSlot`, `PopulateSlots`). Its
UI-only methods are placeholders, in U19's convention (CCR-0098 §3): `add_item_stat_differences`
does nothing (every hover of a new row reaches it, so an error would fire on hover), and
`set_sel_item_id` / `add_undo_state` raise "… is not ported yet (U32/U34)".

Requested of later units:

- **U27** constructs both timeless controls with `Rc::new(AppTimelessBuild)` (or its own
  `TimelessBuild`) and passes a `SocketViewer` over its `PassiveTreeView` (U23).
- **U32/U34/U38** replace the three placeholders of `AppTimelessBuild` (an edit to
  `timeless_list.rs`, which U28 hands over for those three method bodies) with their ports.
- **U36** implements `NotableItemsTab` for the ItemsTab (its `displayItem`, `anointItem`,
  `AppendAnointTooltip`, `socketViewer:AddNodeName` from U24, and the build's misc calculator through
  `CalcFacade::misc_calculator`) and constructs `NotableDBControl::new(.., db = spec.tree.nodes)`.

## 2. `math.random` ported locally (CCR-0033 §3)

Evidence: `TimelessJewelListControl.lua:133-134` (`m_random(2, 17)`); `pob_lua::math` still has no
PRNG (A8 needs-code item 10, A9 "still open"). `timeless_list.rs` ports LuaJIT 2.1 `lib_math.c`
(`random_seed`, `math_random` two-argument form, `lj_prng.c` TW223) as `math_random` /
`math_randomseed`, `// CCR-0033`. Its state starts as after `math.randomseed(0)` (what the UI
oracle's determinism patch 2 sets; old LuaJIT 2.1 builds seed the same way at `luaopen_math`).
Unit test: `math.randomseed(0)` then eight `math.random(2, 17)` and `randomseed(12.5)` equal
`/usr/bin/luajit` (2.1.1788856981). The gallery `u28_timeless_list` exercises it through Militant
Faith against the oracle. Proposed for ORC-X2's successor: move it to `pob_lua::math` (one PRNG
state per Lua state; U43's PoEAPI needs it too) and re-export it from U28's file.

## 3. `string:matchOrPattern` (Common.lua:1038-1079)

ui doc §13 item 15 gives it to the first consuming unit in wave order (U12 did not take it). U28
ports it as `notable_db::matchOrPattern` (pub; ItemDB U32, MinionSearchList U31, ConfigTab U41,
CompareTab U46 and PassiveTreeView U23 call it too). Faithful details kept: the sub-group search
always starts at 1 (`subGroups[index]` is always nil at L1044), so a repeated group is found at its
first occurrence (`("cold cold"):matchOrPattern("(cold) (cold)")` is false, as in LuaJIT); the two
string-keyed `pairs` (L1055, L1073) use insertion order (see the ORDER note in `notable_db.rs`).
Unit test: 16 vectors plus a malformed pattern against LuaJIT running the Lua lines verbatim.

## 4. `data.powerStatList` test seam

`NotableDBControl:BuildSortOrder` (L94) reads `data.powerStatList`. Galleries run without game data
(`pob_gamedata::data()` is not initialised there), so `notable_db::set_power_stat_list` (doc-hidden,
never called by the product) installs the dump's `powerStatList.json`; the Lua gallery runs
`Modules/Data.lua:129-235` itself. For that file read, `pob-ui-oracle` (itself dev-only) now lists
`pob-testpaths` under `[dependencies]` instead of `[dev-dependencies]`.
