//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/LabelControl.lua 1-24. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct LabelControl {
    pub label: Prop<String>,
}

impl LabelControl {
    /// lua: LabelControl.lua:12-19 LabelControl(anchor, rect, label)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
    ) -> New<LabelControl> {
        New::new(
            Base::new(anchor, rect),
            LabelControl {
                label: label.into(),
            },
        )
    }
}

impl Widget for LabelControl {
    const NAME: &'static str = "LabelControl";
    const CAPS: Caps = Caps::DRAW;
    /// lua: LabelControl.lua:21-24
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: LabelControl:Draw")
    }
}
