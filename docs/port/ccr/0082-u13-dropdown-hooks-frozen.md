# CCR-0082 (U13): DropDownHooks frozen (empty); DropItem/selFunc/SearchHost surface amendments

Unit: U13 (dropdown and search). Status: proposed. Number: next free at the time of writing
(0080 is taken twice on other branches, 0081 by U11; renumber on collision).

Applies to: ui doc §6.8 (the DropDownControl bullets), §13 item 12 (`DropTipMode`, `DropTipArgs`,
`DropDownHooks` fields, `SearchHost` fields), the UI-U0 stubs
`pob-ui-core/src/controls/{drop_down,search_host}.rs`.

## Grep that froze the set (ui doc §6.8 "Rule: each base-class unit must grep every subclass")

Over `src/` (Export/ excluded, the dev tool is out of scope):

- `newClass(.., "DropDownControl")`: `ItemSlotControl` (Classes/ItemSlotControl.lua:12) and
  `TimelessJewelSocketControl` (Classes/TimelessJewelSocketControl.lua:10). Nothing subclasses
  either.
- `new("DropDownControl")`: 157 sites in 21 files (CalcsTab, ExtBuildListControl, ItemDBControl,
  PartyTab, TradeQuery, CompareTab, SkillsTab, AddImplicitPopup, ConfigTab, ItemListControl,
  MinionSearchListControl, TradeQueryGenerator, Build, CompareBuySimilar, BuildList, ItemsTab,
  PowerReportListControl, ImportTab, NotableDBControl, TreeTab, Main).
- Methods/fields assigned on those instances (a script over every variable a dropdown is assigned
  to): `function self.controls.mainSkillMinion.CanReceiveDrag/ReceiveDrag/tooltipFunc`
  (Modules/Build.lua:541, :548, :551); `tooltipFunc =` (19 sites), `tooltipText =` (11),
  `borderFunc =` (ConfigTab.lua:697), `selFunc =` (TreeTab.lua:2399, and `selFunc(..)` called from
  outside at ItemsTab.lua:604/674/689/2192, TreeTab.lua:2407), `dropped = true`
  (PassiveTreeView.lua:526), `selIndex =` (79), `list =` (26), `enableDroppedWidth =` (19),
  `maxDroppedWidth =` (20), `enableChangeBoxWidth =` (TreeTab.lua:53/132/177), `width =`,
  `enabled =`, `shown =`, `x/y =`, `controls.scrollBar.enabled = true` (TreeTab.lua:54).
- Class methods the subclasses define: ItemSlotControl `CanReceiveDrag`, `ReceiveDrag`, `Draw`,
  `OnKeyDown`, `OnHoverKeyUp` (+ its own `SetSelItemId`, `Populate`); TimelessJewelSocketControl
  `Draw`. Super calls: `self.DropDownControl:Draw(viewPort)`, `:OnKeyDown(key)`,
  `:GetHoverIndex()`, and the fields `self.DropDownControl.list/dropUp/dropped`.

The methods DropDownControl.lua calls on `self` are `DropIndexToListIndex`, `ListIndexToDropIndex`,
`GetDropCount`, `DrawSearchHighlights`, `SetSel`, `ScrollSelIntoView`, `IsMouseOver`, `SetList`,
`CheckDroppedWidth` and the Control/ControlHost/TooltipHost/SearchHost methods. **No subclass and
no instance overrides any of them.** Everything that is overridden is a framework method
(`Draw`, `OnKeyDown`, `OnHoverKeyUp`, `CanReceiveDrag`, `ReceiveDrag`), i.e. the subclass's own
`Widget` impl or `Overrides` on an instance (presence = `Caps::CAN_RECEIVE_DRAG`,
ListControl.lua:164).

## A. `DropDownHooks` is frozen empty and removed

```diff
-/// The overridable methods ItemSlot and TimelessJewelSocket use (frozen by U13 at its gate).
-#[derive(Default)]
-pub struct DropDownHooks {
-    pub can_receive_drag: Option<fn(&Cx, CtrlId, &str, &DragValue) -> bool>,
-    pub receive_drag: Option<fn(&mut Cx, CtrlId, &str, &DragValue, CtrlId)>,
-}
 pub struct DropDownControl {
-    pub hooks: &'static DropDownHooks,
```

The stub's two slots were never consulted by DropDownControl.lua (it does not call
`CanReceiveDrag`/`ReceiveDrag` on itself; ListControl calls them on its drag targets through
`cx.can_receive_drag`/`cx.receive_drag`). A table nobody reads would mislead U28/U32, so the type
and the field are gone. A subclass embeds `DropDownControl` as a field, builds it with
`DropDownControl::state(list, sel_func, tooltip_text, ignore_search_order)` and chains
`.init(|cx, me| DropDownControl::init(cx, me.0))`; its `Widget` impl adds the `Caps` it defines and
calls `DropDownControl::{draw, on_key_down, on_key_up, on_char, is_mouse_over_comp,
get_hover_index}` for the super calls (gallery `u13_slot` is that pattern, verified against
the Lua at both screens).

## B. DropDownControl surface amendments

| Stub | Now | Evidence |
|---|---|---|
| `DropItem { label, search_filter, value }` | plus `detail: Option<String>`; builders `DropItem::str(s)`, `DropItem::new(label, value)`, `.search_filter(..)`, `.detail(..)`, `get::<T>()`, `as_str()` | DropDownControl.lua:319, :393, :399 read `listVal.detail`; ImportTab.lua:1071-1079 sets it |
| `sel_func: Option<Rc<dyn Fn(&mut Cx, usize, &DropItem)>>` | `Option<SelFunc>`, `SelFunc = Rc<dyn Fn(&mut Cx, usize, Option<&DropItem>)>` | L166 `self.selFunc(newSel, self.list[newSel])`: after `SetList({})` (ImportTab.lua:102-103) a wheel over the dropdown clamps to index 1 of an empty list and passes nil |
| `set_sel(cx, me, usize, bool)` | kept, plus `set_sel_num(cx, me, f64, bool)` (the Lua number) | L499/L513: `ListIndexToDropIndex(selIndex, 0) - 1` can be 0 |
| (none) | `state`, `init`; `is_mouse_over_comp -> (bool, Option<DropComp>)`; `drop_index_to_list_index`, `list_index_to_drop_index` (`Option<f64>`), `get_drop_count`, `draw_search_highlights`, `get_sel_value_by_key`, `scroll_sel_into_view`, `get_hover_index`; inherent `draw`/`on_key_down`/`on_key_up`/`on_char` for super calls | L60-L133, L152-L154, L171-L210, L524-L526; ItemSlotControl.lua:148/171/176, TimelessJewelSocketControl.lua:28/31 |
| fields `list, sel_index, sel_func, dropped, drop_up, ignore_search_order` | `ignore_search_order` removed (it is `search.ignore_order`); new `drop_height`, `dropped_width`, `max_dropped_width`, `enable_dropped_width`, `enable_change_box_width`, `hover_sel_drop: Option<f64>`, `hover_sel: Option<usize>`, `border_func: Option<BorderFunc>` (`Rc<dyn Fn(&Cx, CtrlId) -> (f64, f64, f64)>`) | L44-L55, L218-L246, L256-L258, L353-L357 |
| `DropTipMode` | plus `as_str()` (the Lua string) | tooltipFuncs print/compare the mode |

`self.otherDragSource` is read through `controls::list::other_drag_source(cx, me)` (CCR-0078 B
workaround; `// CCR-0078` at DropDownControl.lua:290). `self.selControl` is `HostData::sel_control`
written raw.

## C. SearchHost surface amendments

| Stub | Now | Evidence |
|---|---|---|
| `list_accessor: Option<ListAccessor>`, `ListAccessor = Rc<dyn Fn(&Cx, CtrlId) -> Vec<String>>` | unchanged type; documented as `listAccessor` composed with `valueAccessor` (both pure) | SearchHost.lua:10-12, :43, :139 |
| `match_count: usize`, `last_search_time: Option<i64>` | `last_search_time` removed (no Lua counterpart); new `search_infos: Vec<SearchInfo>`, `SearchInfo { ranges: Vec<SearchRange>, matches }`, `SearchRange { from, to }` (1-based inclusive bytes) | SearchHost.lua:14, :44, :49; DropDownControl.lua:71, :93, :372 read `searchInfos` |
| (none) | `update_search(cx, me, get)` (public: called on dropdowns from outside), `update_match_count()` | ItemsTab.lua:2906, :3287, :3567; AddImplicitPopup.lua:272, :284 |

## Ui doc text to apply (§6.8, the DropDownControl bullets)

> `DropDownControl` (U13) follows the same pattern, with an empty class-hook table (CCR-0082):
>
> - `DropItem { label: String, search_filter: Option<String>, detail: Option<String>, value: Rc<dyn Any> }`; a Lua string entry is a `DropItem` whose `value` is the `String` (`DropItem::str`).
> - No `DropDownHooks`: no subclass or instance overrides a method DropDownControl.lua calls on `self`; ItemSlot and TimelessJewelSocket override framework methods (`Widget`), Build.lua's `mainSkillMinion` uses `Overrides`.
> - `sel_func: Option<Rc<dyn Fn(&mut Cx, usize, Option<&DropItem>)>>`.
> - The tooltip argument is `(DropTipMode /* BODY|OUT|HOVER */, Option<usize>, Option<DropItem>)`.
