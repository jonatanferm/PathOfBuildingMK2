//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/ToastNotification.lua 1-254.
//! Owner: U16.
//!
//! @pob-file Modules/ToastNotification.lua @16de4b82d
//!
//! The module's locals (`toasts`, `dismissedIds`, `nextId`, `anchorMain`, `inputEvents`,
//! `viewPort`, `screenH`) are [`ToastState`] (`cx.app().main.toasts`); its methods are the free
//! functions here (`ToastNotification:Add(msg)` is `toast::add(cx, msg)`).
//!
//! A toast's `dismissButton` is a free ButtonControl node (Lua: an object outside any controls
//! table); it is buried when its toast leaves the list (Lua: garbage).
//! DIVERGENCE(gc): Lua frees a removed toast's button when it is collected; the port buries it at
//! the removal (the button is unreachable from then on in both).

use std::cell::RefCell;
use std::collections::HashSet;
use std::rc::Rc;

use pob_sg::{Align, Font};
use pob_ui_core::controls::ButtonControl;
use pob_ui_core::luastr;
use pob_ui_core::prelude::*;

use crate::app::CxApp;

// @pob Modules/ToastNotification.lua:1-22 h=0d0e90ca
// L21-22 Animation durations (ms)
const SHOW_DURATION: f64 = 250.0;
const HIDE_DURATION: f64 = 75.0;

/// `toast.mode`: nil until the first render, then "SHOWING", "SHOWN", "HIDING".
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ToastMode {
    Showing,
    Shown,
    Hiding,
}

/// One toast table (L58-66).
#[derive(Clone)]
pub struct Toast {
    pub id: String,
    pub message: String,
    pub mode: Option<ToastMode>,
    pub start: Option<f64>,
    pub height: f64,
    pub current_height: f64,
    pub dismiss_button: Option<Ctrl<ButtonControl>>,
}

/// The module's locals (L12-18).
#[derive(Default)]
pub struct ToastState {
    pub toasts: Vec<Toast>,
    pub dismissed_ids: HashSet<String>,
    /// `nextId` (starts at 1; [`generate_id`] treats 0 as 1)
    pub next_id: u64,
    pub anchor_main: Option<CtrlId>,
    pub input_events: Option<Rc<RefCell<InputEvents>>>,
    pub view_port: Option<ViewPort>,
    /// `screenH` (0 at load; `Init` without a height makes it nil)
    pub screen_h: Option<f64>,
}

/// lua: ToastNotification.lua:25-30 generateId()
/// @pob Modules/ToastNotification.lua:24-30 h=98c7bd55
fn generate_id(cx: &mut Cx) -> String {
    let t = &mut cx.app_mut().main.toasts;
    // L14 local nextId = 1
    if t.next_id == 0 {
        t.next_id = 1;
    }
    // L26
    let id = format!("toast_{}", t.next_id);
    // L27
    t.next_id += 1;
    // L29
    id
}

/// lua: ToastNotification.lua:33-37 calculateHeight(message)
/// @pob Modules/ToastNotification.lua:33-37 h=6da2d9f1
fn calculate_height(message: &str) -> f64 {
    // L34 #message:gsub("[^\n]", "")
    let line_count = message.bytes().filter(|&b| b == b'\n').count() as f64;
    // L36
    line_count * 16.0 + 20.0 + 40.0
}

/// lua: ToastNotification.lua:40-45 ToastNotification:Init(anchor, events, vp, height)
/// (main:Init passes only the anchor, so the rest become nil)
/// @pob Modules/ToastNotification.lua:40-45 h=1a7e7fa5
pub fn init(cx: &mut Cx, anchor: CtrlId) {
    let t = &mut cx.app_mut().main.toasts;
    // L41-44
    t.anchor_main = Some(anchor);
    t.input_events = None;
    t.view_port = None;
    t.screen_h = None;
}

/// lua: ToastNotification.lua:48-52 ToastNotification:UpdateFrame(events, vp, height)
/// @pob Modules/ToastNotification.lua:48-52 h=cf1bf32c
pub fn update_frame(
    cx: &mut Cx,
    events: Option<Rc<RefCell<InputEvents>>>,
    vp: ViewPort,
    height: f64,
) {
    let t = &mut cx.app_mut().main.toasts;
    // L49-51
    t.input_events = events;
    t.view_port = Some(vp);
    t.screen_h = Some(height);
}

/// lua: ToastNotification.lua:56-70 ToastNotification:Add(message): the new toast's id.
/// @pob Modules/ToastNotification.lua:56-70 h=3958bc95
pub fn add(cx: &mut Cx, message: &str) -> String {
    // L57
    let id = generate_id(cx);
    // L58-66
    let toast = Toast {
        id: id.clone(),
        message: message.to_owned(),
        mode: None,
        start: None,
        height: calculate_height(message),
        current_height: 0.0,
        dismiss_button: None,
    };
    // L67
    cx.app_mut().main.toasts.toasts.push(toast);
    // L69
    id
}

/// lua: ToastNotification.lua:74-85 ToastNotification:Update(id, message)
/// @pob Modules/ToastNotification.lua:74-85 h=abc5c3ae
pub fn update(cx: &mut Cx, id: &str, message: &str) -> bool {
    // L75
    for toast in &mut cx.app_mut().main.toasts.toasts {
        // L76
        if toast.id == id {
            // L77-78
            toast.message = message.to_owned();
            toast.height = calculate_height(message);
            // L80
            return true;
        }
    }
    // L84
    false
}

/// lua: ToastNotification.lua:89-104 ToastNotification:Remove(id, immediate)
/// @pob Modules/ToastNotification.lua:89-104 h=339dee79
pub fn remove(cx: &mut Cx, id: &str, immediate: bool) -> bool {
    let now = cx.sys.time_ms() as f64;
    // L90
    let pos = cx
        .app()
        .main
        .toasts
        .toasts
        .iter()
        .position(|t| t.id == id);
    // L91
    if let Some(i) = pos {
        // L92
        if immediate {
            // L93
            let t = cx.app_mut().main.toasts.toasts.remove(i);
            if let Some(b) = t.dismiss_button {
                cx.bury(b);
            }
        } else {
            // L95-96
            let t = &mut cx.app_mut().main.toasts.toasts[i];
            t.mode = Some(ToastMode::Hiding);
            t.start = Some(now);
        }
        // L99
        return true;
    }
    // L103
    false
}

/// lua: ToastNotification.lua:107-115 ToastNotification:Exists(id)
/// @pob Modules/ToastNotification.lua:107-115 h=287c3aed
pub fn exists(cx: &Cx, id: &str) -> bool {
    // L108-114
    cx.app().main.toasts.toasts.iter().any(|t| t.id == id)
}

/// lua: ToastNotification.lua:118-126 ToastNotification:Get(id) (a copy of the toast table)
/// @pob Modules/ToastNotification.lua:118-126 h=64849d05
pub fn get(cx: &Cx, id: &str) -> Option<Toast> {
    // L119-125
    cx.app()
        .main
        .toasts
        .toasts
        .iter()
        .find(|t| t.id == id)
        .cloned()
}

/// lua: ToastNotification.lua:129-138 ToastNotification:Clear(immediate)
/// @pob Modules/ToastNotification.lua:129-138 h=ef17edfc
pub fn clear(cx: &mut Cx, immediate: bool) {
    let now = cx.sys.time_ms() as f64;
    // L130
    if immediate {
        // L131
        let old = std::mem::take(&mut cx.app_mut().main.toasts.toasts);
        for t in old {
            if let Some(b) = t.dismiss_button {
                cx.bury(b);
            }
        }
    } else {
        // L133-136
        for toast in &mut cx.app_mut().main.toasts.toasts {
            toast.mode = Some(ToastMode::Hiding);
            toast.start = Some(now);
        }
    }
}

/// lua: ToastNotification.lua:141-143 ToastNotification:WasDismissed(id)
/// @pob Modules/ToastNotification.lua:141-143 h=f4d12b74
pub fn was_dismissed(cx: &Cx, id: &str) -> bool {
    // L142
    cx.app().main.toasts.dismissed_ids.contains(id)
}

/// lua: ToastNotification.lua:146-152 ToastNotification:ClearDismissed(id)
/// @pob Modules/ToastNotification.lua:146-152 h=0cc68cdb
pub fn clear_dismissed(cx: &mut Cx, id: Option<&str>) {
    let t = &mut cx.app_mut().main.toasts;
    // L147
    if let Some(id) = id {
        // L148
        t.dismissed_ids.remove(id);
    } else {
        // L150
        t.dismissed_ids = HashSet::new();
    }
}

/// lua: ToastNotification.lua:155-162 ToastNotification:GetTotalHeight()
/// @pob Modules/ToastNotification.lua:155-162 h=7a5bba83
pub fn get_total_height(cx: &Cx) -> f64 {
    // L156-161
    let mut total = 0.0;
    for toast in &cx.app().main.toasts.toasts {
        total += toast.current_height;
    }
    total
}

/// The dismiss button's onClick (L177-181): the closure holds the toast table; the port finds
/// it by id (a toast already removed from the list is not drawn again, so writing it is moot).
fn on_dismiss(cx: &mut Cx, id: &str) {
    let now = cx.sys.time_ms() as f64;
    let t = &mut cx.app_mut().main.toasts;
    // L178
    t.dismissed_ids.insert(id.to_owned());
    if let Some(toast) = t.toasts.iter_mut().find(|t| t.id == id) {
        // L179-180
        toast.mode = Some(ToastMode::Hiding);
        toast.start = Some(now);
    }
}

/// `screenH` in arithmetic (nil is a Lua error).
fn screen_h(cx: &Cx) -> f64 {
    cx.app()
        .main
        .toasts
        .screen_h
        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on upvalue 'screenH' (a nil value)"))
}

/// lua: ToastNotification.lua:166-252 ToastNotification:Render(): the total toast height.
/// @pob Modules/ToastNotification.lua:166-252 h=07726a8f
pub fn render(cx: &mut Cx) -> f64 {
    // L167
    let mut total_toast_height = 0.0;
    // L168
    let mut toasts_to_remove: Vec<usize> = Vec::new();

    // L170
    let n = cx.app().main.toasts.toasts.len();
    for i in 0..n {
        // L171
        if cx.app().main.toasts.toasts[i].mode.is_none() {
            let now = cx.sys.time_ms() as f64;
            let id = cx.app().main.toasts.toasts[i].id.clone();
            let anchor = cx
                .app()
                .main
                .toasts
                .anchor_main
                .unwrap_or_else(|| lua_error!("ToastNotification:Render before Init"));
            {
                // L172-173
                let t = &mut cx.app_mut().main.toasts.toasts[i];
                t.mode = Some(ToastMode::Showing);
                t.start = Some(now);
            }
            // L174-181
            let owner = cx.main.node;
            let button = cx.add_free(
                Some(owner),
                ButtonControl::new(
                    Anchor::new(BottomLeft, anchor, BottomLeft),
                    rect(4, 0, 80, 20),
                    "Dismiss",
                    move |cx| on_dismiss(cx, &id),
                ),
            );
            cx.app_mut().main.toasts.toasts[i].dismiss_button = Some(button);
        }

        // L184
        let now = cx.sys.time_ms() as f64;
        let t = &mut cx.app_mut().main.toasts.toasts[i];
        let start = t.start.unwrap_or_default();
        match t.mode {
            // L185
            Some(ToastMode::Showing) => {
                // L186
                if now >= start + SHOW_DURATION {
                    // L187-188
                    t.mode = Some(ToastMode::Shown);
                    t.current_height = t.height;
                } else {
                    // L190
                    t.current_height = t.height * (now - start) / SHOW_DURATION;
                }
            }
            // L192-193
            Some(ToastMode::Shown) => t.current_height = t.height,
            // L194
            Some(ToastMode::Hiding) => {
                // L195
                if now >= start + HIDE_DURATION {
                    // L196-197
                    toasts_to_remove.push(i);
                    t.current_height = 0.0;
                } else {
                    // L199
                    t.current_height = t.height * (1.0 - (now - start) / HIDE_DURATION);
                }
            }
            None => {}
        }

        // L203
        total_toast_height += t.current_height;
    }

    // L206-209 Remove finished toasts (in reverse order to preserve indices)
    for &i in toasts_to_remove.iter().rev() {
        let t = cx.app_mut().main.toasts.toasts.remove(i);
        if let Some(b) = t.dismiss_button {
            cx.bury(b);
        }
    }

    // L211
    let mut y_offset = 58.0;
    // L212
    let n = cx.app().main.toasts.toasts.len();
    for i in 0..n {
        let t = cx.app().main.toasts.toasts[i].clone();
        // L213
        if t.current_height > 0.0 {
            // L214
            let toast_y = screen_h(cx) - y_offset - t.current_height;

            // L216-218 Toast background
            cx.sg.set_draw_color(0.85, 0.85, 0.85);
            cx.sg.draw_image(None, 0.0, toast_y, 312.0, t.current_height);
            // L219-220
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg
                .draw_image(None, 0.0, toast_y + 4.0, 308.0, t.current_height - 4.0);

            // L223 Toast text
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L224
            let first = luastr::gsub(&t.message, "\n.*", "");
            cx.sg
                .draw_string(4.0, toast_y + 8.0, Align::Left, 20.0, Font::Var, &first);
            // L225
            let rest = luastr::gsub(&t.message, "^[^\n]*\n?", "");
            cx.sg
                .draw_string(4.0, toast_y + 28.0, Align::Left, 16.0, Font::Var, &rest);

            // L228 Position and draw dismiss button for fully shown toasts
            if let (Some(ToastMode::Shown), Some(button)) = (t.mode, t.dismiss_button) {
                // L229-230 y is relative to anchor (screenH - 4), negative goes up
                cx.base_mut(button).y = Prop::Val(-(y_offset + 4.0));

                // L233 Handle input for the button
                let events = cx.app().main.toasts.input_events.clone();
                if let Some(events) = events {
                    // L234
                    let mut idx = 0;
                    loop {
                        let ev = events.borrow().next_ipairs(&mut idx);
                        let Some(event) = ev else { break };
                        // L235
                        if cx.is_mouse_over(button.id()) {
                            match event.kind {
                                // L236-237
                                EventKind::KeyDown { double_click } => {
                                    cx.on_key_down(button.id(), event.key, double_click);
                                }
                                // L238-239
                                EventKind::KeyUp => {
                                    cx.on_key_up(button.id(), event.key);
                                }
                                EventKind::Char => {}
                            }
                        }
                    }
                }
                // L244
                let vp = cx
                    .app()
                    .main
                    .toasts
                    .view_port
                    .unwrap_or_else(|| lua_error!("ToastNotification:Render: viewPort is nil"));
                cx.draw(button.id(), &vp, false);
            }

            // L247
            y_offset += t.current_height;
        }
    }

    // L251
    total_toast_height
}

// @pob Modules/ToastNotification.lua:254-254 h=8ee79c41
// (return ToastNotification: the module is this file)
