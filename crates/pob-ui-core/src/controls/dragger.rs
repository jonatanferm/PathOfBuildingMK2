//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/DraggerControl.lua 1-147. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct DraggerControl {
    pub tt: TooltipHost<()>,
    pub label: Prop<String>,
    pub on_key_down: Option<Rc<dyn Fn(&mut Cx, (f64, f64))>>,
    pub on_key_up: Option<Rc<dyn Fn(&mut Cx, (f64, f64))>>,
    pub on_right_click: Option<Rc<dyn Fn(&mut Cx)>>,
    pub on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
    pub force_tooltip: bool,
    pub image: Option<ImageHandle>,
    pub dragging: bool,
}

impl DraggerControl {
    /// lua: DraggerControl.lua:9-21 DraggerControl(anchor, rect, label, onKeyDown, onKeyUp, onRightClick, onHover, forceTooltip)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        on_key_down: Option<Rc<dyn Fn(&mut Cx, (f64, f64))>>,
        on_key_up: Option<Rc<dyn Fn(&mut Cx, (f64, f64))>>,
        on_right_click: Option<Rc<dyn Fn(&mut Cx)>>,
        on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
        force_tooltip: bool,
    ) -> New<DraggerControl> {
        New::new(
            Base::new(anchor, rect),
            DraggerControl {
                tt: TooltipHost::new(None),
                label: label.into(),
                on_key_down,
                on_key_up,
                on_right_click,
                on_hover,
                force_tooltip,
                image: None,
                dragging: false,
            },
        )
    }
    /// lua: DraggerControl.lua:23-30 SetImage
    pub fn set_image(cx: &mut Cx, me: CtrlId, path: Option<&str>) {
        let _ = (cx, me, path);
        todo!("U9: DraggerControl:SetImage")
    }
}

impl Widget for DraggerControl {
    const NAME: &'static str = "DraggerControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: DraggerControl.lua:32-37
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U9: DraggerControl:IsMouseOver")
    }
    /// lua: DraggerControl.lua:39-109
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: DraggerControl:Draw")
    }
    /// lua: DraggerControl.lua:111-128
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U9: DraggerControl:OnKeyDown")
    }
    /// lua: DraggerControl.lua:129-147
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U9: DraggerControl:OnKeyUp")
    }
}
