//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SectionControl.lua 1-33. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct SectionControl {
    pub label: Prop<String>,
}

impl SectionControl {
    /// lua: SectionControl.lua:10-14 SectionControl(anchor, rect, label)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
    ) -> New<SectionControl> {
        New::new(
            Base::new(anchor, rect),
            SectionControl {
                label: label.into(),
            },
        )
    }
}

impl Widget for SectionControl {
    const NAME: &'static str = "SectionControl";
    const CAPS: Caps = Caps::DRAW;
    /// lua: SectionControl.lua:16-33
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: SectionControl:Draw")
    }
}
