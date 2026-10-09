//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PopupDialog.lua 1-97
//! (`newClass("PopupDialog", "ControlHost", "Control")`). Owner: U14. UI-U0 stub; the
//! constructor is `popups::open_popup`, ProcessInput is `popups::popup_process_input`.

use std::rc::Rc;

use crate::prelude::*;

pub struct PopupDialog {
    pub title: Prop<String>,
    pub enter_control: Option<CtrlKey>,
    pub escape_control: Option<CtrlKey>,
    pub scroll_bar_func: Option<Rc<dyn Fn(&mut Cx)>>,
    pub resize_func: Option<Rc<dyn Fn(&mut Cx)>>,
}

impl Widget for PopupDialog {
    const NAME: &'static str = "PopupDialog";
    const CAPS: Caps = Caps::DRAW;

    /// lua: PopupDialog.lua:44-70
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        let _ = (cx, me, vp, no_tooltip);
        todo!("U14: PopupDialog:Draw")
    }
}
