//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/EditControl.lua 1-761
//! (`newClass("EditControl", "ControlHost", "Control", "UndoHandler", "TooltipHost")`). Owner: U11.
//! UI-U0 stub: the ui doc §6.8 surface (`new`, `state`, `init`, the listed methods, `buf`/`caret`/
//! `sel` as 1-based byte positions, `change_func`).

use std::rc::Rc;

use pob_sg::Key;

use crate::prelude::*;
use crate::tooltip::TooltipHost;
use crate::undo::UndoStack;

/// EditControl(anchor, rect, init, prompt, filter, limit, changeFunc, lineHeight, allowZoom, clearable) (EditControl.lua:43)
#[derive(Default)]
pub struct EditArgs {
    pub init: Option<String>,
    pub prompt: Option<String>,
    /// nil => main.unicode and "%c" or "^%w%p "
    pub filter: Option<String>,
    pub limit: Option<usize>,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub line_height: Option<f64>,
    pub allow_zoom: bool,
    pub clearable: bool,
}

/// CreateUndoState (EditControl.lua:743-750): `{ buf, caret }`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct EditUndoState {
    pub buf: String,
    pub caret: usize,
}

pub struct EditControl {
    pub tt: TooltipHost<()>,
    pub undo: UndoStack<EditUndoState>,
    pub buf: String,
    /// 1-based byte position, as in Lua
    pub caret: usize,
    /// 1-based byte position, as in Lua
    pub sel: Option<usize>,
    pub prompt: Option<String>,
    pub filter: Option<String>,
    pub limit: Option<usize>,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, &str)>>,
    pub line_height: Option<f64>,
    pub allow_zoom: bool,
    pub clearable: bool,
}

impl EditControl {
    /// lua: EditControl.lua:43-99
    pub fn new(anchor: Anchor, rect: [Prop<f64>; 4], args: EditArgs) -> New<EditControl> {
        New::new(Base::new(anchor, rect), EditControl::state(args))
            .host()
            .init(|cx, me| EditControl::init(cx, me.id()))
    }
    /// The constructor's field part (subclasses embed it: GemSelectControl, ResizableEditControl).
    pub fn state(args: EditArgs) -> EditControl {
        let _ = args;
        todo!("U11: EditControl.lua:43-99 (fields)")
    }
    /// The constructor's tail that needs the node (scrollbars and buttons anchored to `me`).
    pub fn init(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U11: EditControl.lua:43-99 (children)")
    }
    /// lua: EditControl.lua:101-109 SetText(text, notify)
    pub fn set_text(cx: &mut Cx, me: CtrlId, text: &str, notify: bool) {
        let _ = (cx, me, text, notify);
        todo!("U11: EditControl:SetText")
    }
    /// lua: EditControl.lua:111-116 SetPlaceholder(text, notify)
    pub fn set_placeholder(cx: &mut Cx, me: CtrlId, text: &str, notify: bool) {
        let _ = (cx, me, text, notify);
        todo!("U11: EditControl:SetPlaceholder")
    }
    /// lua: EditControl.lua:118-123 SetProtected(bool)
    pub fn set_protected(cx: &mut Cx, me: CtrlId, protected: bool) {
        let _ = (cx, me, protected);
        todo!("U11: EditControl:SetProtected")
    }
    /// lua: EditControl.lua:132-136
    pub fn select_all(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U11: EditControl:SelectAll")
    }
    /// lua: EditControl.lua:138-143
    pub fn get_sel_text(cx: &Cx, me: CtrlId) -> String {
        let _ = (cx, me);
        todo!("U11: EditControl:GetSelText")
    }
    /// lua: EditControl.lua:145-165
    pub fn replace_sel(cx: &mut Cx, me: CtrlId, text: &str) {
        let _ = (cx, me, text);
        todo!("U11: EditControl:ReplaceSel")
    }
    /// lua: EditControl.lua:167-187
    pub fn insert(cx: &mut Cx, me: CtrlId, text: &str) {
        let _ = (cx, me, text);
        todo!("U11: EditControl:Insert")
    }
    /// lua: UndoHandler.lua:44-50 (EditControl's undo state: EditControl.lua:743-761)
    pub fn undo(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U11: EditControl undo")
    }
    /// lua: UndoHandler.lua:53-58 (EditControl's undo state: EditControl.lua:743-761)
    pub fn redo(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U11: EditControl redo")
    }
}

impl Widget for EditControl {
    const NAME: &'static str = "EditControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CHAR)
        .union(Caps::FOCUS_GAINED);

    /// lua: EditControl.lua:125-130
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        let _ = (cx, me);
        todo!("U11: EditControl:IsMouseOver")
    }
    /// lua: EditControl.lua:246-434
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U11: EditControl:Draw")
    }
    /// lua: EditControl.lua:436-441
    fn on_focus_gained(cx: &mut Cx, me: CtrlId) {
        let _ = (cx, me);
        todo!("U11: EditControl:OnFocusGained")
    }
    /// lua: EditControl.lua:443-666
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        let _ = (cx, me, key, double_click);
        todo!("U11: EditControl:OnKeyDown")
    }
    /// lua: EditControl.lua:668-727
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        let _ = (cx, me, key);
        todo!("U11: EditControl:OnKeyUp")
    }
    /// lua: EditControl.lua:729-741
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        let _ = (cx, me, ch);
        todo!("U11: EditControl:OnChar")
    }
}
