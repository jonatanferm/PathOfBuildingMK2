//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SliderControl.lua 1-185. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct SliderControl {
    pub tt: TooltipHost<f64>,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, f64)>>,
    pub scroll_wheel_speed_tbl: Option<Vec<(String, f64)>>,
    pub val: f64,
    pub div_count: Option<f64>,
    pub dragging: bool,
}

impl SliderControl {
    /// lua: SliderControl.lua:13-21 SliderControl(anchor, rect, changeFunc, scrollWheelSpeedTbl)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        change_func: Option<Rc<dyn Fn(&mut Cx, f64)>>,
        scroll_wheel_speed_tbl: Option<Vec<(String, f64)>>,
    ) -> New<SliderControl> {
        New::new(
            Base::new(anchor, rect),
            SliderControl {
                tt: TooltipHost::new(None),
                change_func,
                scroll_wheel_speed_tbl,
                val: 0.0,
                div_count: None,
                dragging: false,
            },
        )
    }
    /// lua: SliderControl.lua:44-47 GetKnobTravel
    pub fn get_knob_travel(cx: &mut Cx, me: CtrlId) -> f64 {
        let _ = (cx, me);
        todo!("U9: SliderControl:GetKnobTravel")
    }
    /// lua: SliderControl.lua:49-57 SetVal
    pub fn set_val(cx: &mut Cx, me: CtrlId, new_val: f64) {
        let _ = (cx, me, new_val);
        todo!("U9: SliderControl:SetVal")
    }
    /// lua: SliderControl.lua:59-67 GetDivVal
    pub fn get_div_val(cx: &mut Cx, me: CtrlId, val: f64) -> f64 {
        let _ = (cx, me, val);
        todo!("U9: SliderControl:GetDivVal")
    }
    /// lua: SliderControl.lua:69-71 SetValFromKnobX
    pub fn set_val_from_knob_x(cx: &mut Cx, me: CtrlId, knob_x: f64) {
        let _ = (cx, me, knob_x);
        todo!("U9: SliderControl:SetValFromKnobX")
    }
    /// lua: SliderControl.lua:73-76 GetKnobXForVal
    pub fn get_knob_x_for_val(cx: &mut Cx, me: CtrlId) -> f64 {
        let _ = (cx, me);
        todo!("U9: SliderControl:GetKnobXForVal")
    }
}

impl Widget for SliderControl {
    const NAME: &'static str = "SliderControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: SliderControl.lua:23-42
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U9: SliderControl:IsMouseOver")
    }
    /// lua: SliderControl.lua:78-133
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: SliderControl:Draw")
    }
    /// lua: SliderControl.lua:135-156
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U9: SliderControl:OnKeyDown")
    }
    /// lua: SliderControl.lua:158-185
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U9: SliderControl:OnKeyUp")
    }
}
