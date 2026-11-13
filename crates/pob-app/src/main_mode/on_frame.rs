//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1-864 (main:OnFrame, input queue, SetMode).
//! Owner: U16.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main:OnFrame` (:379-493), the input queue (`OnKeyDown`/`OnKeyUp`/`OnChar`, :495-505),
//! `main:SetMode` (:507-511) and `main:CallMode` (:513-518, as [`current_mode`] plus a direct
//! [`Mode`] call at each site). The popup layer (:437-443) is U14's
//! `pob_ui_core::popups::draw_popup_layer`.
//!
//! ORDER: `pairs(self.onFrameFuncs)` (L490) runs in insertion order; see init.rs (§8 rule 6 (c)).

use std::rc::Rc;

use pob_sg::{Align, Font};
use pob_ui_core::popups;
use pob_ui_core::prelude::*;

use super::{Mode, ModeArg};
use crate::app::CxApp;

/// `self.modes[self.mode]` (nil when no mode is set or the name is unknown).
pub fn current_mode(cx: &Cx) -> Option<Rc<dyn Mode>> {
    let m = &cx.app().main;
    m.mode.as_ref().and_then(|name| m.modes.get(name).cloned())
}

/// `self.modes[self.mode]` where Lua indexes it unconditionally (a nil mode is a Lua error).
fn current_mode_or_err(cx: &Cx) -> Rc<dyn Mode> {
    current_mode(cx).unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// lua: Main.lua:379-493 main:OnFrame()
/// @pob Modules/Main.lua:379-493 h=32977665
pub fn main_on_frame(cx: &mut Cx) {
    // L380
    let (w, h) = cx.sg.get_virtual_screen_size();
    cx.main.screen_w = w;
    cx.main.screen_h = h;

    // L382-386
    cx.app_mut().main.portrait_mode = cx.main.screen_h > cx.main.screen_w;
    // L387
    while let Some(new_mode) = cx.app().main.new_mode.clone() {
        // L388
        if cx.app().main.mode.is_some() {
            // L389
            if let Some(m) = current_mode(cx) {
                m.shutdown(cx);
            }
        }
        // L391-392
        cx.app_mut().main.mode = Some(new_mode);
        cx.app_mut().main.new_mode = None;
        // L393
        let args = cx.app().main.new_mode_args.clone();
        if let Some(m) = current_mode(cx) {
            m.init(cx, &args);
        }
        // L394
        if cx.app().main.new_mode_change_to_tree {
            // L395
            current_mode_or_err(cx).set_view_mode(cx, "TREE");
        }
        // L397
        cx.app_mut().main.new_mode_change_to_tree = false;
    }

    // L400
    let vp = ViewPort {
        x: 0.0,
        y: 0.0,
        width: cx.main.screen_w,
        height: cx.main.screen_h,
    };
    cx.main.view_port = vp;

    let events = cx.app().main.input_events.clone();
    // L402
    if let Some(p) = cx.main.popups.first().copied() {
        // L403
        popups::popup_process_input(cx, p.id(), &mut events.borrow_mut(), &vp);
        // L404
        events.borrow_mut().wipe();
    } else {
        // L406
        let node = cx.main.node;
        cx.process_controls_input(node, &mut events.borrow_mut(), &vp);
    }

    // L409
    if let Some(m) = current_mode(cx) {
        m.on_frame(cx, &mut events.borrow_mut(), &vp);
    }

    // L411
    if let Some(msg) = cx.app().launch.update_err_msg.clone() {
        // L412
        super::toast::add(cx, &format!("Update check failed!\n{msg}"));
        // L413
        cx.app_mut().launch.update_err_msg = None;
    }
    // L415
    if let Some(avail) = cx.app().launch.update_available.clone() {
        // L416
        if avail == "none" {
            // L417
            super::toast::add(
                cx,
                "No update available\nYou are running the latest version.",
            );
            // L418
            cx.app_mut().launch.update_available = None;
        } else if !cx.app().main.update_available_shown {
            // L420
            super::toast::add(
                cx,
                "Update Available\nAn update has been downloaded and is ready\nto be applied.",
            );
            // L421
            cx.app_mut().main.update_available_shown = true;
        }
    }

    // L426 Update and render toasts
    let screen_h = cx.main.screen_h;
    super::toast::update_frame(cx, Some(events.clone()), vp, screen_h);
    // L427
    let total_toast_height = super::toast::render(cx);
    // L428
    cx.app_mut().main.main_bar_height = 58.0 + total_toast_height;

    // L431 Draw main controls
    cx.sg.set_draw_color(0.85, 0.85, 0.85);
    // L432
    cx.sg
        .draw_image(None, 0.0, cx.main.screen_h - 58.0, 312.0, 58.0);
    // L433
    cx.sg.set_draw_color(0.1, 0.1, 0.1);
    // L434
    cx.sg
        .draw_image(None, 0.0, cx.main.screen_h - 54.0, 308.0, 54.0);
    // L435
    let node = cx.main.node;
    cx.draw_controls(node, &vp, None);

    // L437-443: U14's port (the dim and the topmost popup at layer 10)
    popups::draw_popup_layer(cx, &vp);

    // L445
    if let Some(text) = cx.main.show_drag_text.clone() {
        // L446
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L447
        let str_width = cx.sg.draw_string_width(16.0, Font::Var, &text);
        // L448
        cx.sg.set_draw_layer(Some(20), Some(0));
        // L449
        cx.sg.set_draw_color_a(0.15, 0.15, 0.15, 0.75);
        // L450
        cx.sg
            .draw_image(None, cursor_x, cursor_y - 8.0, str_width + 2.0, 18.0);
        // L451
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L452
        cx.sg.draw_string(
            cursor_x + 1.0,
            cursor_y - 7.0,
            Align::Left,
            16.0,
            Font::Var,
            &text,
        );
        // L453
        cx.main.show_drag_text = None;
    }

    // L456-474: a commented-out colour test

    // L476
    if !cx.app().main.item_lib_wiki_triggered {
        // L477
        let mut i = 0;
        loop {
            let ev = events.borrow().next_ipairs(&mut i);
            let Some(event) = ev else { break };
            // L478
            if event.kind == EventKind::KeyUp && event.key == Key::F(1) {
                // L479
                let view_mode = current_mode_or_err(cx).view_mode(cx);
                let tab_name = format!(
                    "{} tab",
                    view_mode.map_or_else(|| "Build List".to_owned(), |v| v.to_ascii_lowercase())
                );
                // L480 self:OpenAboutPopup(tabName or 1)
                let f = cx.app().main.hooks.open_about_popup.clone();
                f(cx, ModeArg::Str(tab_name));
                // L481
                break;
            }
        }
    }
    // L485
    cx.app_mut().main.item_lib_wiki_triggered = false;

    // L487
    events.borrow_mut().wipe();

    // L489 TODO: this pattern may pose memory management issues for classes that don't exist for the lifetime of the program
    // L490 for _, onFrameFunc in pairs(self.onFrameFuncs) do (a function removed by an earlier one
    // is not called; `pairs` skips keys that became nil)
    let names: Vec<String> = cx
        .main
        .on_frame_funcs
        .iter()
        .map(|(k, _)| k.clone())
        .collect();
    for name in names {
        let f = cx
            .main
            .on_frame_funcs
            .iter()
            .find(|(k, _)| *k == name)
            .map(|(_, f)| f.clone());
        if let Some(f) = f {
            // L491
            f(cx);
        }
    }
}

/// lua: Main.lua:495-497 main:OnKeyDown(key, doubleClick)
/// @pob Modules/Main.lua:495-497 h=91d44839
pub fn main_on_key_down(cx: &mut Cx, key: Key, double_click: bool) {
    // L496
    cx.app().main.input_events.borrow_mut().push(InputEvent {
        kind: EventKind::KeyDown { double_click },
        key,
    });
}

/// lua: Main.lua:499-501 main:OnKeyUp(key)
/// @pob Modules/Main.lua:499-501 h=80f76995
pub fn main_on_key_up(cx: &mut Cx, key: Key) {
    // L500
    cx.app().main.input_events.borrow_mut().push(InputEvent {
        kind: EventKind::KeyUp,
        key,
    });
}

/// lua: Main.lua:503-505 main:OnChar(key)
/// @pob Modules/Main.lua:503-505 h=3beeb978
pub fn main_on_char(cx: &mut Cx, key: Key) {
    // L504
    cx.app().main.input_events.borrow_mut().push(InputEvent {
        kind: EventKind::Char,
        key,
    });
}

/// lua: Main.lua:507-511 main:SetMode(newMode, ...)
/// @pob Modules/Main.lua:507-511 h=95ff8326
pub fn set_mode(cx: &mut Cx, new_mode: &str, args: Vec<ModeArg>) {
    let m = &mut cx.app_mut().main;
    // L508
    m.new_mode = Some(new_mode.to_owned());
    // L509
    m.new_mode_args = args;
    // L510
    m.predefined_build_name = None;
}

/// `main:CallMode(func, ...)`: the mode lookup. Each call site calls the [`Mode`] method itself
/// (a Rust trait has no `modeTbl[func]` lookup by name).
/// lua: Main.lua:513-518 main:CallMode(func, ...)
/// @pob Modules/Main.lua:513-518 h=2a11efc6
pub fn call_mode_target(cx: &Cx) -> Option<Rc<dyn Mode>> {
    // L514-516
    current_mode(cx)
}
