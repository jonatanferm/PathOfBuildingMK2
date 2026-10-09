//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1660-1786 (main:OpenPopup,
//! ClosePopup, OpenMessagePopup, OpenConfirmPopup, OpenNewFolderPopup, OpenCloudErrorPopup).
//! Ui doc §6.10. Owner: U14. UI-U0 stub: final signatures.
//!
//! Lua builds popup `controls` tables before the dialog exists, and their closures reach siblings
//! through the local `controls` table when they are called. In Rust, closures capture the popup
//! id `p`; a sibling created later is looked up with `cx.child_as::<W>(p, "key")` at call time.

use std::rc::Rc;

use crate::controls::popup_dialog::PopupDialog;
use crate::cx::Cx;
use crate::ids::{Ctrl, CtrlId, CtrlKey, ViewPort};
use crate::input::InputEvents;
use crate::prop::Prop;

/// The trailing arguments of main:OpenPopup.
#[derive(Default)]
pub struct PopupOpts {
    pub enter: Option<CtrlKey>,
    pub default: Option<CtrlKey>,
    pub escape: Option<CtrlKey>,
    pub scroll_bar_func: Option<Rc<dyn Fn(&mut Cx)>>,
    pub resize_func: Option<Rc<dyn Fn(&mut Cx)>>,
}

/// main:OpenPopup. Creates the dialog node first (centred; x/y props as in PopupDialog.lua). Then `build` adds the
/// children into the popup's host (they are owned by the popup). Then the PopupDialog.lua:25-33 anchor rules apply:
/// no point => TOP/TOP of the popup; `other` nil => the popup; `other_key` => the sibling with that key.
/// Then `default` is selected. Finally the popup is inserted at main.popups[0] (Lua popups[1]).
/// lua: Main.lua:1660-1664
pub fn open_popup(
    cx: &mut Cx,
    width: f64,
    height: f64,
    title: impl Into<Prop<String>>,
    build: impl FnOnce(&mut Cx, Ctrl<PopupDialog>),
    opts: PopupOpts,
) -> Ctrl<PopupDialog> {
    let _ = (cx, width, height, title.into(), build, opts);
    todo!("U14: main:OpenPopup")
}

/// main:ClosePopup: removes popups[0] and buries it (freed at end of frame, D8).
/// lua: Main.lua:1666-1668
pub fn close_popup(cx: &mut Cx) {
    let _ = cx;
    todo!("U14: main:ClosePopup")
}

/// lua: Main.lua:1670-1681
pub fn open_message_popup(cx: &mut Cx, title: &str, msg: &str) -> Ctrl<PopupDialog> {
    let _ = (cx, title, msg);
    todo!("U14: main:OpenMessagePopup")
}

/// lua: Main.lua:1683-1728
pub fn open_confirm_popup(
    cx: &mut Cx,
    title: &str,
    msg: &str,
    confirm_label: &str,
    on_confirm: impl Fn(&mut Cx) + 'static,
    extra: Option<(&str, Rc<dyn Fn(&mut Cx)>)>,
) -> Ctrl<PopupDialog> {
    let _ = (cx, title, msg, confirm_label, extra);
    let _ = on_confirm;
    todo!("U14: main:OpenConfirmPopup")
}

/// lua: Main.lua:1730-1756
pub fn open_new_folder_popup(
    cx: &mut Cx,
    path: &str,
    on_close: Option<Rc<dyn Fn(&mut Cx, Option<String>)>>,
) {
    let _ = (cx, path, on_close);
    todo!("U14: main:OpenNewFolderPopup")
}

/// lua: Main.lua:1760-1785
pub fn open_cloud_error_popup(cx: &mut Cx, file_name: &str) {
    let _ = (cx, file_name);
    todo!("U14: main:OpenCloudErrorPopup")
}

/// PopupDialog:ProcessInput (ESCAPE => escape:Click() or ClosePopup; RETURN => enter:Click()).
/// lua: PopupDialog.lua:72-97
pub fn popup_process_input(cx: &mut Cx, popup: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
    let _ = (cx, popup, ev, vp);
    todo!("U14: PopupDialog:ProcessInput")
}
