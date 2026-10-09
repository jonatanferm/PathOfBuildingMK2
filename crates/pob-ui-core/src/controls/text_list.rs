//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TextListControl.lua 1-122. Owner: U12.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

/// One `columns[i]` entry (`{ x, align }`).
#[derive(Clone, Debug, PartialEq)]
pub struct TextListColumn {
    pub x: f64,
    pub align: pob_sg::Align,
}

/// One `list[i]` row (`{ height, [1] = text, ... }`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct TextListRow {
    pub height: f64,
    pub cells: Vec<Option<String>>,
}

pub struct TextListControl {
    pub columns: Vec<TextListColumn>,
    pub list: Vec<TextListRow>,
    pub section_heights: Option<Vec<f64>>,
}

impl TextListControl {
    /// lua: TextListControl.lua:9-21 TextListControl(anchor, rect, columns, list, sectionHeights)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        columns: Vec<TextListColumn>,
        list: Vec<TextListRow>,
        section_heights: Option<Vec<f64>>,
    ) -> New<TextListControl> {
        New::new(
            Base::new(anchor, rect),
            TextListControl {
                columns,
                list,
                section_heights,
            },
        )
        .host()
    }
}

impl Widget for TextListControl {
    const NAME: &'static str = "TextListControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: TextListControl.lua:23-28
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U12: TextListControl:IsMouseOver")
    }
    /// lua: TextListControl.lua:30-81
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U12: TextListControl:Draw")
    }
    /// lua: TextListControl.lua:83-94
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U12: TextListControl:OnKeyDown")
    }
    /// lua: TextListControl.lua:96-122
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U12: TextListControl:OnKeyUp")
    }
}
