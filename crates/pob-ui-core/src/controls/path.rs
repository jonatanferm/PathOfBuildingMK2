//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PathControl.lua 1-110. Owner: U12.
//! UI-U0 stub: struct, constructor and Widget surface; signatures beyond ui doc §6 are the
//! owning unit's to finalise (it may amend them before its wave gate).
#![allow(unused_imports)]

use std::rc::Rc;

use pob_sg::{ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

pub struct PathControl {
    pub base_path: String,
    pub sub_path: String,
    pub on_change: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub undo: UndoStack<String>,
}

impl PathControl {
    /// lua: PathControl.lua:12-22 PathControl(anchor, rect, basePath, subPath, onChange)
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        base_path: String,
        sub_path: String,
        on_change: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    ) -> New<PathControl> {
        New::new(
            Base::new(anchor, rect),
            PathControl {
                base_path,
                sub_path,
                on_change,
                undo: UndoStack::default(),
            },
        )
        .host()
    }
    /// lua: PathControl.lua:24-64 SetSubPath
    pub fn set_sub_path(cx: &mut Cx, me: CtrlId, sub_path: &str, no_undo: bool) {
        let _ = (cx, me, sub_path, no_undo);
        todo!("U12: PathControl:SetSubPath")
    }
    /// lua: PathControl.lua:103-105 CreateUndoState
    pub fn create_undo_state(cx: &mut Cx, me: CtrlId) -> String {
        let _ = (cx, me);
        todo!("U12: PathControl:CreateUndoState")
    }
    /// lua: PathControl.lua:107-109 RestoreUndoState
    pub fn restore_undo_state(cx: &mut Cx, me: CtrlId, state: String) {
        let _ = (cx, me, state);
        todo!("U12: PathControl:RestoreUndoState")
    }
}

impl Widget for PathControl {
    const NAME: &'static str = "PathControl";
    const CAPS: Caps = Caps::DRAW.union(Caps::IS_MOUSE_OVER).union(Caps::KEY_DOWN);
    /// lua: PathControl.lua:66-71
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U12: PathControl:IsMouseOver")
    }
    /// lua: PathControl.lua:73-91
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U12: PathControl:Draw")
    }
    /// lua: PathControl.lua:93-101
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U12: PathControl:OnKeyDown")
    }
}
