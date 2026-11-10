//! Support module of the U14 galleries (`u14_keys`, `u14_mouse`): the Rust side of
//! `lua/gallery/_u14.lua`. Owner: U14.
//!
//! `pin_screen_h` is the Lua `opts.pinScreenH`: `main.screenH` is overwritten after the launch
//! sets it, so the centred popups sit at the same virtual coordinates at both acceptance screens.
//! The help button of the cloud-error popup calls `OpenURL`; the U11 platform wrapper records that
//! side effect (CCR-0028 item 2, see `_u11.rs`).
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{ButtonControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::popups::{self, LaunchInfo, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u11::{flush_effects, wrap_platform};
use super::_u9::Gallery;

type Log = Rc<RefCell<String>>;

fn set_log(log: &Log, s: String) {
    *log.borrow_mut() = s;
}

/// `G:OpenCustom()`
fn open_custom(cx: &mut Cx, log: &Log) {
    let log = log.clone();
    popups::open_popup(
        cx,
        360.0,
        200.0,
        Prop::f(|cx, _| format!("Custom ^7({})", cx.main.popups.len())),
        move |cx, p| {
            cx.add(
                p,
                "label",
                LabelControl::new(Anchor::NONE, rect(0, 16, 0, 16), "^7Name:"),
            );
            cx.add(
                p,
                "edit",
                EditControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(20, 40, 220, 20),
                    EditArgs {
                        init: Some(String::new()),
                        limit: Some(20),
                        ..EditArgs::default()
                    },
                ),
            );
            cx.add(
                p,
                "stack",
                ButtonControl::new(
                    Anchor::key(Left, "edit", Right),
                    rect(8, 0, 80, 20),
                    "Stack",
                    |cx| {
                        popups::open_message_popup(cx, "Stacked", "On top of the custom popup");
                    },
                ),
            );
            let l = log.clone();
            cx.add(
                p,
                "ok",
                ButtonControl::new(Anchor::NONE, rect(-45, 160, 80, 20), "Ok", move |cx| {
                    let buf = cx.get(cx.child_as::<EditControl>(p, "edit")).buf.clone();
                    set_log(&l, format!("ok:{buf}"));
                    popups::close_popup(cx);
                }),
            );
            let l = log.clone();
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 160, 80, 20), "Cancel", move |cx| {
                    set_log(&l, "cancel".to_owned());
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts {
            enter: Some("ok".into()),
            default: Some("edit".into()),
            escape: Some("cancel".into()),
            ..PopupOpts::default()
        },
    );
}

/// `G:OpenScroll()`
fn open_scroll(cx: &mut Cx) {
    // the Lua closure reaches `controls.scrollBar` through the local table
    let scroll_bar: Rc<Cell<Option<CtrlId>>> = Rc::default();
    let sb = scroll_bar.clone();
    popups::open_popup(
        cx,
        300.0,
        200.0,
        "Scroll",
        move |cx, p| {
            let s = cx.add(
                p,
                "scrollBar",
                ScrollBarControl::new(
                    Anchor::nil(TopRight, TopRight),
                    rect(-4, 30, 18, 150),
                    Some(16.0),
                    None,
                    false,
                ),
            );
            sb.set(Some(s.id()));
            cx.add(
                p,
                "offset",
                LabelControl::new(
                    Anchor::NONE,
                    rect(0, 60, 0, 16),
                    Prop::f(move |cx, _| {
                        format!("^7offset {}", pob_lua::tostring_num(cx.get(s).offset))
                    }),
                ),
            );
        },
        PopupOpts {
            scroll_bar_func: Some(Rc::new(move |cx: &mut Cx| {
                let s = scroll_bar.get().expect("scrollBar");
                ScrollBarControl::set_content_dimension(cx, s, 600.0, 150.0);
            })),
            ..PopupOpts::default()
        },
    );
}

fn host_button(
    cx: &mut Cx,
    host: CtrlId,
    key: &'static str,
    y: i32,
    label: &str,
    f: impl Fn(&mut Cx) + 'static,
) {
    cx.add(
        host,
        key,
        ButtonControl::new(Anchor::NONE, rect(20, y, 180, 20), label, f),
    );
}

/// A U14 gallery app.
pub fn app(mut parts: PlatformParts, pin_screen_h: Option<f64>) -> Box<dyn App> {
    let effects = wrap_platform(&mut parts);
    let log: Log = Rc::new(RefCell::new("none".to_owned()));
    Gallery::new(
        parts,
        move |cx, host| {
            // the first ButtonControl's TooltipHost loads Classes/Tooltip.lua
            module_load(cx);
            host_button(cx, host, "msg", 20, "Message", |cx| {
                popups::open_message_popup(
                    cx,
                    "Info",
                    "Hello there.\nThis is a ^1coloured^7 message\nwith three lines.",
                );
            });
            let l = log.clone();
            host_button(cx, host, "confirm", 50, "Confirm", move |cx| {
                let l = l.clone();
                popups::open_confirm_popup(
                    cx,
                    "Delete Build",
                    "Are you sure you want to delete this build?",
                    "Delete",
                    move |_| set_log(&l, "deleted".to_owned()),
                    None,
                );
            });
            let l = log.clone();
            host_button(cx, host, "confirm3", 80, "Confirm 3", move |cx| {
                let (l1, l2) = (l.clone(), l.clone());
                popups::open_confirm_popup(
                    cx,
                    "Connect",
                    "Continue without connecting?",
                    "Continue",
                    move |_| set_log(&l1, "continue".to_owned()),
                    Some((
                        "Connect Path",
                        Rc::new(move |_: &mut Cx| set_log(&l2, "connect".to_owned())),
                    )),
                );
            });
            let l = log.clone();
            host_button(cx, host, "custom", 110, "Custom", move |cx| {
                open_custom(cx, &l)
            });
            let l = log.clone();
            host_button(cx, host, "folder", 140, "New Folder", move |cx| {
                let user = cx.sys.platform.user_path().expect("GetUserPath");
                let l = l.clone();
                popups::open_new_folder_popup(
                    cx,
                    &format!("{user}/"),
                    Some(Rc::new(move |_, name: Option<String>| {
                        set_log(&l, format!("folder {}", name.as_deref().unwrap_or("nil")))
                    })),
                );
            });
            host_button(cx, host, "cloud", 170, "Cloud Error", |cx| {
                // the gallery mini-launch's `launch` table and Launch.lua:9 APP_NAME
                let launch = LaunchInfo {
                    app_name: "Path of Building".to_owned(),
                    version_number: "?".to_owned(),
                    version_branch: None,
                    dev_mode: false,
                };
                popups::open_cloud_error_popup(cx, "Builds/Missing.xml", &launch);
            });
            host_button(cx, host, "scroll", 200, "Scroll", open_scroll);
            let l = log.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 240, 0, 16),
                    Prop::f(move |cx, _| {
                        format!("^7log: {}  popups: {}", l.borrow(), cx.main.popups.len())
                    }),
                ),
            );
        },
        move |cx, host, ev, vp| {
            if let Some(h) = pin_screen_h {
                cx.main.screen_h = h;
            }
            // Main.lua:402-407
            if let Some(p) = cx.main.popups.first().copied() {
                popups::popup_process_input(cx, p.id(), ev, vp);
                ev.wipe();
            } else {
                cx.process_controls_input(host, ev, vp);
            }
            flush_effects(cx, &effects);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            cx.draw_controls(host, vp, None);
            // Main.lua:437-443
            popups::draw_popup_layer(cx, vp);
            flush_effects(cx, &effects);
        },
    )
}
