//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1-864 (OpenPathPopup).
//! Owner: U16.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main:OpenPathPopup` (:819-852) and `main:ChangeUserPath` (:854-862).
//!
//! The popup's closures share the local `invalidPath` (the label reads it, the edit's changeFunc
//! writes it): an `Rc<RefCell<String>>` (ui doc §7.1). `controls.save` is a forward reference
//! from the changeFunc, looked up at call time.

use std::cell::RefCell;
use std::rc::Rc;

use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;

use crate::app::CxApp;

/// lua: Main.lua:819-852 main:OpenPathPopup(invalidPath, errMsg, ignoreBuild)
/// @pob Modules/Main.lua:819-852 h=e11b5ae8
pub fn open_path_popup(cx: &mut Cx, invalid_path: &str, err_msg: &str, ignore_build: bool) {
    // L820 local controls = { } (the popup's host)
    // L821
    let default_label_placement_x = 8.0;
    let invalid_path = Rc::new(RefCell::new(invalid_path.to_owned()));
    let err_msg = err_msg.to_owned();
    // L851
    popups::open_popup(
        cx,
        600.0,
        150.0,
        "Change Settings Path",
        move |cx, p| {
            // L823-829
            let ip = invalid_path.clone();
            let em = err_msg.clone();
            let label = cx.add(
                p,
                "label",
                LabelControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(default_label_placement_x, 20, 206, 16),
                    Prop::f(move |_, _| {
                        // L824-828
                        let path = pob_lua::gsub(&ip.borrow(), "?", "^1?^7", None).0;
                        format!(
                            "^7User settings path cannot be loaded: {em}\nCurrent Path: {path}/Path of Building/\nIf this location is managed by OneDrive, navigate to that folder and manually try\nto open Settings.xml in a text editor before re-opening Path of Building\nOtherwise, specify a new location for your Settings.xml:"
                        )
                    }),
                ),
            );
            // L830-837
            let ip = invalid_path.clone();
            let init = invalid_path.borrow().clone();
            let user_path = cx.add(
                p,
                "userPath",
                EditControl::new(
                    Anchor::new(TopLeft, label, TopLeft),
                    rect(0, 60, 206, 20),
                    EditArgs {
                        init: Some(init),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L831
                            *ip.borrow_mut() = pob_lua::string::sanitiseText(buf);
                            // L832
                            let ok = pob_lua::lmatch(&ip.borrow(), "?", None).is_none();
                            let save = cx.child_as::<ButtonControl>(p, "save");
                            // L833 / L835
                            cx.base_mut(save).enabled = Prop::Val(ok);
                        })),
                        ..EditArgs::default()
                    },
                ),
            );
            // L838-846
            let save = cx.add(
                p,
                "save",
                ButtonControl::new(
                    Anchor::new(TopLeft, user_path, TopLeft),
                    rect(0, 26, 206, 20),
                    "Save",
                    move |cx| {
                        // L839
                        let buf = cx.get(user_path).buf.clone();
                        let (res, msg) = super::folders::make_dir(cx, &buf);
                        // L840
                        if !res && msg.as_deref() != Some("No error") {
                            // L841
                            popups::open_message_popup(
                                cx,
                                "Error",
                                &format!(
                                    "Couldn't create '{buf}' : {}",
                                    msg.unwrap_or_else(|| lua_error!(
                                        "attempt to concatenate local 'msg' (a nil value)"
                                    ))
                                ),
                            );
                        } else {
                            // L843
                            change_user_path(cx, &buf, ignore_build);
                            // L844
                            popups::close_popup(cx);
                        }
                    },
                ),
            );
            // L847
            cx.base_mut(save).enabled = Prop::Val(false);
            // L848-850
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(0, 0, 0, 0), "Cancel", |_| {
                    // L849 Do nothing, require user to enter a location
                }),
            );
        },
        PopupOpts {
            enter: Some("save".into()),
            default: None,
            escape: Some("cancel".into()),
            ..PopupOpts::default()
        },
    );
}

/// lua: Main.lua:854-862 main:ChangeUserPath(newUserPath, ignoreBuild)
/// @pob Modules/Main.lua:854-862 h=937b200f
pub fn change_user_path(cx: &mut Cx, new_user_path: &str, ignore_build: bool) {
    // L855
    cx.app_mut().main.user_path = Some(new_user_path.to_owned());
    // L856
    let _ = super::folders::make_dir(cx, new_user_path);
    // L857
    let default_build_path = format!("{new_user_path}Builds/");
    cx.app_mut().main.default_build_path = Some(default_build_path.clone());
    // L858
    cx.app_mut().main.build_path = Some(default_build_path.clone());
    // L859
    let _ = super::folders::make_dir(cx, &default_build_path);
    // L860
    super::settings::load_settings(cx, ignore_build);
    // L861
    super::settings::load_shared_items(cx);
}
