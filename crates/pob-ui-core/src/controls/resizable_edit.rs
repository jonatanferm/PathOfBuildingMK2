//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ResizableEditControl.lua 1-51. Owner: U11.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::controls::edit::{EditArgs, EditControl};
use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct ResizableEditControl {
    pub edit: EditControl,
    pub min_width: f64,
    pub max_width: f64,
    pub min_height: f64,
    pub max_height: f64,
}

impl ResizableEditControl {
    /// lua: ResizableEditControl.lua:12-31 ResizableEditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        args: EditArgs,
        min_width: f64,
        max_width: f64,
        min_height: f64,
        max_height: f64,
    ) -> New<ResizableEditControl> {
        New::new(
            Base::new(anchor, rect),
            ResizableEditControl {
                edit: EditControl::state(args),
                min_width,
                max_width,
                min_height,
                max_height,
            },
        )
        .host()
        .init(|cx, me| EditControl::init(cx, me.id()))
    }
    /// lua: ResizableEditControl.lua:37-44 SetBoundedDrag
    pub fn set_bounded_drag(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U11: ResizableEditControl:SetBoundedDrag")
    }
    /// lua: ResizableEditControl.lua:46-48 SetWidth
    pub fn set_width(cx: &mut Cx, me: CtrlId, width: f64) {
        let _ = (cx, me, width);
        todo!("U11: ResizableEditControl:SetWidth")
    }
    /// lua: ResizableEditControl.lua:49-51 SetHeight
    pub fn set_height(cx: &mut Cx, me: CtrlId, height: f64) {
        let _ = (cx, me, height);
        todo!("U11: ResizableEditControl:SetHeight")
    }
}

impl Widget for ResizableEditControl {
    const NAME: &'static str = "ResizableEditControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CHAR)
        .union(Caps::FOCUS_GAINED);
    crate::impl_parts!(ResizableEditControl: edit => EditControl);
    /// lua: ResizableEditControl.lua:33-36
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U11: ResizableEditControl:Draw")
    }
}
