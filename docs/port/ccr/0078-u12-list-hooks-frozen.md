# CCR-0078 (U12): ListControl hook set frozen; drag-target fields; U12 stub amendments

**Status: applied 2026-11-10 by CCR2 (A, C: ui doc §6.8, §13 item 12); B needs code. See AMENDMENTS A8.**

Unit: U12 (lists). Filed status: proposed. Number: next free at the time of writing (0076 and 0077 are
taken on other branches; renumber on collision).

Applies to: ui doc §6.8 (the ListControl surface), §6.3 (`Base`, owner U4), §13 item 12.

## Grep that froze the set (ui doc §6.8 "Rule: each base-class unit must grep every subclass")

`grep -rn '"ListControl"' src/` (Export/ excluded: the dev tool is out of scope) gives 18
subclasses: BuildList, ComparePowerReportList, ConfigSetList, FolderList, ItemDB, ItemList,
ItemSetList, MinionList (and MinionSearchList, a MinionList subclass), NotableDB, PassiveMastery,
PassiveSpecList, PowerReportList, SharedItemList, SharedItemSetList, SkillList, SkillSetList,
TimelessJewelList, TradeStatWeightMultiplierList; plus one plain `new("ListControl")`
(Modules/ConfigModBrowser.lua:232). Instance assignments of list methods:
ConfigModBrowser.lua:235 (GetRowValue), :243 (AddValueTooltip), :251 (OnSelClick);
CompareTab.lua:1614 (OnSelKeyDown), :1624 (CanReceiveDrag), :1625-1627 (OnSelCopy, OnSelCut,
OnSelDelete), :1628 (CanReceiveDrag on the PathControl); BuildListControl.lua:33/:36
(CanReceiveDrag/ReceiveDrag on its PathControl).

Methods the subclasses define, per name (class:line of the first definition):

| Method | Called by ListControl.lua | Defined by |
|---|---|---|
| GetRowValue | 227, 327 | every subclass; ConfigModBrowser instance |
| GetRowIcon | 229-230 | SkillList:241 |
| AddValueTooltip | 332-334 | SkillList, TradeStat, ItemSet, SharedItem, TimelessJewel, PowerReport, SharedItemSet, ItemList, PassiveMastery, ComparePowerReport, ItemDB, MinionList, NotableDB; ConfigModBrowser instance |
| GetDragValue | 162 | ItemSet:89, SharedItem:48, BuildList:226, SharedItemSet:70, ItemList:279, ItemDB:346, MinionList:83, NotableDB:279 |
| OnDragSend | 453-454 | none |
| OnOrderChange | 447-448 **(selIndex, selDragIndex)** | SkillList:130 (reads both), ItemSet, PassiveSpec, ItemList, ConfigSet, SkillSet |
| OnSelect | 104-105 | SkillList |
| OnSelClick | 381-382 | 15 subclasses; ConfigModBrowser instance |
| OnSelCopy | 398-399 | SkillList, SharedItem, BuildList, ItemList, ItemDB, NotableDB; CompareTab instance |
| OnSelCut | 402-403 (not in the header comment) | BuildList; CompareTab instance |
| OnSelDelete | 406-407 | 11 subclasses; CompareTab instance |
| OnSelKeyDown | 409-410 | ItemSet, BuildList, PassiveSpec, SharedItemSet, ConfigSet, SkillSet; CompareTab instance |
| OnAnyKeyDown | 413-414 | none |
| OverrideSelectIndex | 95 | TimelessJewel:55 |
| SetHighlightColor | 282 | TimelessJewel:30 |
| CanDragToValue | 195, 203, 238, 258, 275 **(index, value, self.otherDragSource)** | BuildList:255 |
| ReSort | 368-369 | PowerReport:62, ComparePowerReport:74 |
| GetColumnProperty | 145, 218, 295, 306, 368 | none (class method only) |

Not list hooks (they are framework `Widget` methods of the subclass or of the target):
`Draw` (9 subclasses; `self.ListControl.Draw(self, viewPort)` = `ListControl::draw`),
`OnHoverKeyUp` (SkillList, ItemList, ItemDB), `CanReceiveDrag`/`ReceiveDrag` (called on the
drag targets, which may be PathControl, DropDownControl, ItemSlotControl or a list:
`Widget::can_receive_drag`/`receive_drag`, presence = `Caps::CAN_RECEIVE_DRAG`, L164).

## A. Signature changes to the frozen §6.8 surface

```diff
-    pub on_order_change: Option<fn(&mut Cx, CtrlId)>,
+    pub on_order_change: Option<fn(&mut Cx, CtrlId, usize /*selIndex*/, usize /*selDragIndex*/)>,
-    pub can_drag_to_value: Option<fn(&Cx, CtrlId, usize, &ListValue, CtrlId) -> bool>,
+    pub can_drag_to_value: Option<fn(&Cx, CtrlId, usize, &ListValue, Option<CtrlId>) -> bool>,
```

1. **OnOrderChange** gets both indices: ListControl.lua:448 `self:OnOrderChange(self.selIndex,
   self.selDragIndex)`; SkillListControl.lua:130-145 uses them to move `mainSocketGroup` and
   `calcsTab.input.skill_number`.
2. **CanDragToValue**'s source is optional: ListControl.lua:238 passes `self.otherDragSource`
   without a nil guard (the guarded calls are :258 and :275).

`ListOverrides` follows (one `Option<Rc<dyn Fn>>` per field; the type aliases `GetRowValueFn`,
… are public). `ListHooks::BASE` is the base-class table subclasses spread
(`..ListHooks::BASE`); `LIST_CONTROL_HOOKS` is `new("ListControl")`'s.

Other additions to the §6.8 surface (not changes): `select_index` returns the Lua `bool`
(BuildListControl and the key handlers test it; 0 is Lua's nil index); `ListControl::new` for a
plain list; `list_remove`/`list_insert` (`t_remove`/`t_insert` through `ListData`);
`get_row_region`; `duck` (ControlHost.lua:20); `ListValue::get/rc`; the remaining fields
(`font`, `scroll_h`, `label_position_offset`, `default_text`, `sel_cx/sel_cy`, `sel_drag_index`,
`drag_target`, `other_drag_targeting`, `hover_index`, `hover_value`); `ListColumn { width:
Option<Prop<f64>>, label: Option<Prop<String>>, sortable: Option<Prop<bool>>, font: Option<Font>,
offset, computed_width }` (a nil width is meaningful, L145; `_offset`/`_width` are written by Draw).
`scroll` stays a `bool`; `"HORIZONTAL"` is `scroll_h = true` after `state` (only Export uses it).

## B. `otherDragSource` / `otherDragTargeting` on any control (needs U4, ui doc §6.3)

ListControl.lua:165, :184, :186, :460-461 write these two fields on every drag target, and
PathControl.lua:86, DropDownControl.lua:290 and ItemSlotControl.lua:150 read
`self.otherDragSource`. Targets are arbitrary controls (ItemsTab.lua:1188-1202 adds
`build.controls.mainSkillMinion`, a DropDownControl, and every ItemSlotControl), so the field
belongs to `Base` (Control):

```diff
 pub struct Base {
     ...
     pub mouse_clip_rect: Option<Prop<Option<[f64; 4]>>>,
+    /// `self.otherDragSource` (written by a dragging ListControl; `false` = None)
+    pub other_drag_source: Option<CtrlId>,
+    /// `self.otherDragTargeting`
+    pub other_drag_targeting: bool,
 }
```

Local workaround (marked `// CCR-0078`): `pob_ui_core::controls::list::{other_drag_source,
set_other_drag_source, other_drag_targeting, set_other_drag_targeting}`. They use the
`ListControl`/`PathControl` fields when the target is one, else a per-thread side table keyed by
the target id. **U13 (DropDownControl) and U32 (ItemSlotControl) read the field through
`other_drag_source(cx, me)`** until the `Base` fields land; then the four functions become
`cx.base(id).other_drag_source` accessors and the side table and the two struct fields go.

## C. U12 stub amendments (ui doc §13 item 12)

| Stub | Now | Evidence |
|---|---|---|
| `PathControl.sub_path: String` | `Option<String>` (nil until the constructor's SetSubPath, whose equality test must fail even for `""`); new `base_name`, `folder_list: Vec<Folder>`, `other_drag_source`, `other_drag_targeting`; `new(.., sub_path: Option<String>, ..)`; `UNDO_OPS`, `undo`, `redo` | PathControl.lua:17-20, :25, :86, :103-109; BuildListControl.lua:31 |
| `TextListRow { height, cells }` | plus `x`, `align`, `font`, `underline: Option<Vec<bool>>`, `extra: Option<Rc<dyn Any>>`; rows are `Rc<TextListRow>` (`hoveredLine.line` is the row) | TextListControl.lua:49-52, :74; Build.lua:1944-2018, :2084, :1361 |
| `TextListControl::new(.., columns: Vec<..>, list: Vec<TextListRow>, ..)` | `columns: Option<Vec<..>>`, `list: Vec<Rc<TextListRow>>`; new `on_click`, `hovered_line: Option<HoveredLine>` | TextListControl.lua:17, :67-88; Main.lua:1359; Build.lua:576 |

## Ui doc text to apply (§6.8, after "U12 owns `ListControl`. The surface is frozen here:")

Replace the two hook lines as in A, and add after the `ListControl` block:

> Hook set frozen by U12 (CCR-0078): the 18 ListHooks fields above are every method
> ListControl.lua calls on `self`; `CanReceiveDrag`/`ReceiveDrag` are `Widget` methods of the
> drag target. `otherDragSource`/`otherDragTargeting` are `Base` fields (CCR-0078 B).
