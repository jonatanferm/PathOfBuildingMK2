//! The part of the global `main` (Modules/Main.lua) that generic controls read or write (ui doc
//! §6.11). Owner: U4.
//!
//! main.inputEvents is owned by pob-app's MainState and passed down as `&mut InputEvents` (only
//! Main.lua touches it globally; verified by grep).

use std::rc::Rc;

use crate::color_codes::ColorCodes;
use crate::controls::popup_dialog::PopupDialog;
use crate::cx::Cx;
use crate::ids::{Ctrl, CtrlId, ViewPort};

pub struct MainCore {
    /// the `main` ControlHost node (bottom bar controls)
    pub node: CtrlId,
    /// main.screenW/H (set at the top of main:OnFrame)
    pub screen_w: f64,
    pub screen_h: f64,
    pub view_port: ViewPort,
    /// [0] == Lua popups[1] (topmost)
    pub popups: Vec<Ctrl<PopupDialog>>,
    pub show_drag_text: Option<String>,
    /// Data/Global.lua colorCodes (mutable; updateColorCode)
    pub cc: ColorCodes,
    pub show_flavour_text: bool,
    pub not_supported_mod_tooltips: bool,
    pub unicode: bool,
    pub disable_scroll_control_interaction: bool,
    pub thousands_separator: String,
    pub decimal_separator: String,
    pub show_thousands_separators: bool,
    /// main.onFrameFuncs (insertion order; ORDER noted)
    pub on_frame_funcs: Vec<(String, Rc<dyn Fn(&mut Cx)>)>,
}

impl MainCore {
    /// The values `main:Init` gives these fields before Settings.xml is loaded (Main.lua:65
    /// `popups`, :108-125 the display options, :282 `onFrameFuncs`, :309 `unicode`), with `node`
    /// the `main` ControlHost node. pob-app (U16) runs the rest of `main:Init`, including
    /// `unicode = type(_G.utf8) == "table"` and the onFrameFuncs it registers.
    /// CCR-0034 (U4 addition).
    pub fn new(node: CtrlId) -> MainCore {
        MainCore {
            node,
            screen_w: 0.0,
            screen_h: 0.0,
            view_port: ViewPort::default(),
            popups: Vec::new(),
            show_drag_text: None,
            cc: ColorCodes::default(),
            show_flavour_text: true,
            not_supported_mod_tooltips: true,
            unicode: false,
            disable_scroll_control_interaction: false,
            thousands_separator: ",".to_owned(),
            decimal_separator: ".".to_owned(),
            show_thousands_separators: true,
            on_frame_funcs: Vec::new(),
        }
    }
}
