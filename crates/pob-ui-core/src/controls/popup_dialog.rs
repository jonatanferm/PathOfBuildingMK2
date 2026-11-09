//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/PopupDialog.lua 1-97
//! (`newClass("PopupDialog", "ControlHost", "Control")`). Owner: U14. The constructor is
//! `popups::open_popup` (it needs `main` and runs the `controls` builder between the node's
//! creation and the anchor rules); ProcessInput is `popups::popup_process_input` (the framework
//! has no ProcessInput method: only `main:OnFrame` calls it, Main.lua:403).
//!
//! @pob-file Classes/PopupDialog.lua @16de4b82d
//!
//! Props with side effects (ui doc §7.3): none (`title` may be a function; it only reads).

use std::rc::Rc;

use pob_lua::math::m_floor;
use pob_sg::{Align, Font};

use crate::prelude::*;

pub struct PopupDialog {
    /// self.title (GetProperty("title") in Draw)
    pub title: Prop<String>,
    /// self.enterControl: a key of `self.controls`
    pub enter_control: Option<CtrlKey>,
    /// self.escapeControl: a key of `self.controls`
    pub escape_control: Option<CtrlKey>,
    /// self.scrollBarFunc ("allow scrollbar functionality inside of popups")
    pub scroll_bar_func: Option<Rc<dyn Fn(&mut Cx)>>,
    /// self.resizeFunc ("allow resizing of popup")
    pub resize_func: Option<Rc<dyn Fn(&mut Cx)>>,
}

impl Widget for PopupDialog {
    const NAME: &'static str = "PopupDialog";
    const CAPS: Caps = Caps::DRAW;

    /// lua: PopupDialog.lua:44-70
    /// @pob Classes/PopupDialog.lua:44-70 h=544a06e3
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L45
        let (x, y) = cx.pos(me);
        // L46
        let (width, height) = cx.size(me);
        // Draw dialog background
        // L48
        cx.sg.set_draw_color(0.8, 0.8, 0.8);
        // L49
        cx.sg.draw_image(None, x, y, width, height);
        // L50
        cx.sg.set_draw_color(0.1, 0.1, 0.1);
        // L51
        cx.sg
            .draw_image(None, x + 2.0, y + 2.0, width - 4.0, height - 4.0);
        // Draw dialog title box
        // L53
        let title = cx.prop::<Self, _>(me, |w| &w.title);
        // L54
        let title_width = cx.sg.draw_string_width(16.0, Font::Var, &title);
        // L55
        let title_x = x + m_floor((width - title_width - 8.0) / 2.0);
        // L56
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L57
        cx.sg
            .draw_image(None, title_x, y - 10.0, title_width + 8.0, 24.0);
        // L58
        cx.sg.set_draw_color(0.0, 0.0, 0.0);
        // L59
        cx.sg
            .draw_image(None, title_x + 2.0, y - 8.0, title_width + 4.0, 20.0);
        // L60
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L61
        cx.sg
            .draw_string(title_x + 4.0, y - 7.0, Align::Left, 16.0, Font::Var, &title);
        // L62
        if let Some(f) = cx.w::<Self>(me).scroll_bar_func.clone() {
            // L63
            f(cx);
        }
        // L65
        if let Some(f) = cx.w::<Self>(me).resize_func.clone() {
            // L66
            f(cx);
        }
        // Draw controls
        // L69
        cx.draw_controls(me, vp, None);
    }
}
