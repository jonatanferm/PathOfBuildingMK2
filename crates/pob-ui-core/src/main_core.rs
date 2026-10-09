//! The part of the global `main` (Modules/Main.lua) that generic controls read or write (ui doc
//! §6.11). Owner: U4. UI-U0: the struct as frozen.
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
