# CCR-0069 (U9): `MainCore.invert_slider_scroll_direction`, and the U9 control surface amendments

Unit: U9 (basic controls and draw helpers). Status: proposed. Number: next free at the time of
writing (0056-0060, 0062, 0066 may be taken by unmerged branches; renumber on collision).

## A. Missing `MainCore` field (needs U4 / the ui doc §6.11 owner)

`SliderControl:OnKeyUp` reads the global `main.invertSliderScrollDirection`:

- `Classes/SliderControl.lua:168` `elseif (not main.invertSliderScrollDirection and key == "WHEELDOWN") or (main.invertSliderScrollDirection and  key == "WHEELUP") or ...`
- `Classes/SliderControl.lua:176` (the mirrored branch)
- written by `Modules/Main.lua:653-654` (Settings.xml), `:1173` (options checkbox), `:1250` (options cancel)

`MainCore` (ui doc §6.11, `pob-ui-core/src/main_core.rs`, U4) has no such field, and U9 may not
edit that file.

Proposed diff (ui doc §6.11 and `main_core.rs`):

```diff
 pub struct MainCore {
     ...
     pub disable_scroll_control_interaction: bool,
+    /// main.invertSliderScrollDirection (nil until Settings.xml / the options dialog set it)
+    pub invert_slider_scroll_direction: bool,
     ...
 }
 // MainCore::new
+            invert_slider_scroll_direction: false,
```

Local workaround (marked `// CCR-0069`): `pob_ui_core::controls::slider::INVERT_SLIDER_SCROLL_DIRECTION`,
a `thread_local! Cell<bool>` (false by default, as main:Init leaves the field nil). When the field
lands, `SliderControl::on_key_up` reads `cx.main.invert_slider_scroll_direction` and the
thread-local is deleted; U16/U18 (Main.lua options) write the field.

## B. Owner amendments to the U9 stubs (ui doc §13 item 12: "non-§6 constructor signatures of the generic controls (U9/U12)", `ArrowDir`, `world_to_screen`)

Recorded here so the dependants (U10-U14, tabs) see them in one place. The doc text in §6 is
unchanged; only stub signatures that §6/§9 do not freeze changed.

| Stub | Now | Evidence |
|---|---|---|
| `CheckBoxControl.label: Prop<String>` | `Prop<Option<String>>`, plus `label_width`, `label_right`, `border_func: Option<Rc<dyn Fn(&Cx) -> (f64, f64, f64)>>` | nil labels: ItemsTab.lua:480, CalcsTab.lua:94, ItemSlotControl.lua:43; `if label then` CheckBoxControl.lua:31, :82; labelWidth read by CompareTab.lua:1793; borderFunc ConfigTab.lua:697 |
| `ScrollBarControl::new(.., step: f64, dir: ScrollDir, ..)` | `step: Option<f64>, dir: Option<ScrollDir>` | `step or self.width * 2`, `dir or "VERTICAL"` (ScrollBarControl.lua:16-17); DropDownControl.lua:37, TextListControl.lua:12 pass no dir |
| `ScrollBarControl.enabled_flag` | removed: `self.enabled` is `Base::enabled` (Control.lua:60); raw reads use `ScrollBarControl::enabled_raw` | ScrollBarControl.lua:19, :32, :37, :83; DropDownControl.lua:42, GemSelectControl.lua:39 |
| `ScrollBarControl.con_dim/view_dim: f64` | `Option<f64>`; new `offset_max`, `knob_dim`, `knob_travel`, `locked`, `dragging`, `drag_*`, `hold_*`; `ScrollComp`, `is_mouse_over_comp` (2 returns), `set_offset_from_knob_pos`, `get_knob_pos_for_offset`; `is_scroll_*_key` take `&Cx` | ScrollBarControl.lua:28-111 |
| `SliderControl::new(.., scroll_wheel_speed_tbl: Option<Vec<(String, f64)>>)` | `Option<ScrollWheelSpeedTbl { shift, ctrl, default }>`; new `knob_size`, `drag_cx`, `drag_knob_x`; `get_div_val(cx, me, Option<f64>) -> (f64, f64)`; `is_mouse_over_comp`; getters take `&Cx` | SliderControl.lua:19, :59-67; TreeTab.lua:1741, :1763-1764 |
| `DraggerControl` callbacks `Fn(&mut Cx, (f64, f64))` for down/up, `on_right_click: Fn(&mut Cx)` | all three are `DragFn = Rc<dyn Fn(&mut Cx, (f64, f64))>`; new `locked`, `over_size_text`, `cursor_x/y`, `clicked` | DraggerControl.lua:143 passes `{ X = cursorX, Y = cursorY }`; ResizableEditControl.lua:19 reads it |
| `RectangleOutlineControl::new(.., colors: [f64; 3], stroke: f64)` | `Option<[f64; 3]>`, `Option<f64>` (`colors or {1,1,1}`, `stroke or 1`) | RectangleOutlineControl.lua:11-12 |
| `draw.rs` | `ArrowDir::from_lua`, `lua_for_num` (LuaJIT numeric `for`, sign-of-step direction, accumulated variable) | Main.lua:1504-1512, :1544, :1564; SliderControl.lua:104 |
