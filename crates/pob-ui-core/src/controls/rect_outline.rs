//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/RectangleOutlineControl.lua 1-23. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct RectangleOutlineControl {
    pub colors: [f64; 3],
    pub stroke: f64,
}

impl RectangleOutlineControl {
    /// lua: RectangleOutlineControl.lua:9-14 RectangleOutlineControl(anchor, rect, colors, stroke)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        colors: [f64; 3],
        stroke: f64,
    ) -> New<RectangleOutlineControl> {
        New::new(
            Base::new(anchor, rect),
            RectangleOutlineControl { colors, stroke },
        )
    }
}

impl Widget for RectangleOutlineControl {
    const NAME: &'static str = "RectangleOutlineControl";
    const CAPS: Caps = Caps::DRAW;
    /// lua: RectangleOutlineControl.lua:16-23
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: RectangleOutlineControl:Draw")
    }
}
