//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CheckBoxControl.lua 1-117. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct CheckBoxControl {
    pub tt: TooltipHost<bool>,
    pub label: Prop<String>,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, bool)>>,
    pub state: bool,
    pub clicked: bool,
}

impl CheckBoxControl {
    /// lua: CheckBoxControl.lua:9-19 CheckBoxControl(anchor, rect, label, changeFunc, tooltipText, initialState)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        change_func: Option<Rc<dyn Fn(&mut Cx, bool)>>,
        tooltip_text: Option<Prop<Option<String>>>,
        initial_state: bool,
    ) -> New<CheckBoxControl> {
        New::new(
            Base::new(anchor, rect),
            CheckBoxControl {
                tt: TooltipHost::new(tooltip_text),
                label: label.into(),
                change_func,
                state: initial_state,
                clicked: false,
            },
        )
    }
}

impl Widget for CheckBoxControl {
    const NAME: &'static str = "CheckBoxControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: CheckBoxControl.lua:21-38
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U9: CheckBoxControl:IsMouseOver")
    }
    /// lua: CheckBoxControl.lua:40-92
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: CheckBoxControl:Draw")
    }
    /// lua: CheckBoxControl.lua:94-102
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U9: CheckBoxControl:OnKeyDown")
    }
    /// lua: CheckBoxControl.lua:104-117
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U9: CheckBoxControl:OnKeyUp")
    }
}
