//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ScrollBarControl.lua 1-327. Owner: U9.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

/// The `dir` constructor argument ("HORIZONTAL" | "VERTICAL"; ScrollBarControl.lua:14).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDir {
    Horizontal,
    Vertical,
}

pub struct ScrollBarControl {
    pub step: f64,
    pub dir: ScrollDir,
    pub auto_hide: bool,
    pub offset: f64,
    pub con_dim: f64,
    pub view_dim: f64,
    pub enabled_flag: bool,
}

impl ScrollBarControl {
    /// lua: ScrollBarControl.lua:14-26 ScrollBarControl(anchor, rect, step, dir, autoHide)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        step: f64,
        dir: ScrollDir,
        auto_hide: bool,
    ) -> New<ScrollBarControl> {
        New::new(
            Base::new(anchor, rect),
            ScrollBarControl {
                step,
                dir,
                auto_hide,
                offset: 0.0,
                con_dim: 0.0,
                view_dim: 0.0,
                enabled_flag: false,
            },
        )
    }
    /// lua: ScrollBarControl.lua:28-48 SetContentDimension
    pub fn set_content_dimension(cx: &mut Cx, me: CtrlId, con_dim: f64, view_dim: f64) {
        let _ = (cx, me, con_dim, view_dim);
        todo!("U9: ScrollBarControl:SetContentDimension")
    }
    /// lua: ScrollBarControl.lua:50-52 SetOffset
    pub fn set_offset(cx: &mut Cx, me: CtrlId, offset: f64) {
        let _ = (cx, me, offset);
        todo!("U9: ScrollBarControl:SetOffset")
    }
    /// lua: ScrollBarControl.lua:54-56 Scroll
    pub fn scroll(cx: &mut Cx, me: CtrlId, mult: f64) {
        let _ = (cx, me, mult);
        todo!("U9: ScrollBarControl:Scroll")
    }
    /// lua: ScrollBarControl.lua:58-64 ScrollIntoView
    pub fn scroll_into_view(cx: &mut Cx, me: CtrlId, min_dim: f64, size: f64) {
        let _ = (cx, me, min_dim, size);
        todo!("U9: ScrollBarControl:ScrollIntoView")
    }
    /// lua: ScrollBarControl.lua:322-324 IsScrollDownKey
    pub fn is_scroll_down_key(cx: &mut Cx, me: CtrlId, key: Key) -> bool {
        let _ = (cx, me, key);
        todo!("U9: ScrollBarControl:IsScrollDownKey")
    }
    /// lua: ScrollBarControl.lua:325-327 IsScrollUpKey
    pub fn is_scroll_up_key(cx: &mut Cx, me: CtrlId, key: Key) -> bool {
        let _ = (cx, me, key);
        todo!("U9: ScrollBarControl:IsScrollUpKey")
    }
}

impl Widget for ScrollBarControl {
    const NAME: &'static str = "ScrollBarControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: ScrollBarControl.lua:74-111
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U9: ScrollBarControl:IsMouseOver")
    }
    /// lua: ScrollBarControl.lua:113-257
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U9: ScrollBarControl:Draw")
    }
    /// lua: ScrollBarControl.lua:259-293
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U9: ScrollBarControl:OnKeyDown")
    }
    /// lua: ScrollBarControl.lua:295-319
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U9: ScrollBarControl:OnKeyUp")
    }
}
