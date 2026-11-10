//! Support module of the U12 galleries (`u12_*`). Owner: U12.

use pob_sg::{Align, Font};
use pob_ui_core::prelude::*;

use super::_u9::{effects_len, record_image_load};

/// `G:OnFrame` of the U12 galleries: input, a background, the controls, then the drag text
/// exactly as `main:OnFrame` draws it (Modules/Main.lua:445-454), which a dragging
/// ListControl requests through `main.showDragText`.
pub fn frame(cx: &mut Cx, host: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
    cx.process_controls_input(host, ev, vp);
    cx.sg.set_draw_color(0.1, 0.1, 0.1);
    cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
    cx.draw_controls(host, vp, None);
    draw_drag_text(cx);
}

/// Modules/Main.lua:445-454 (the gallery copies the block: `main:OnFrame` is not run).
pub fn draw_drag_text(cx: &mut Cx) {
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
}

/// `local h = NewImageHandle(); h:Load(path)` with its `ImageLoad` side effect.
pub fn load_image(cx: &mut Cx, path: &str) -> pob_sg::ImageHandle {
    let h = cx.sg.new_image_handle();
    let before = effects_len(cx);
    h.load(&mut cx.sg, path, pob_sg::ImageFlags::empty());
    record_image_load(cx, before, path, pob_sg::ImageFlags::empty());
    h
}

/// Lua `tostring(n)` of an optional index (`"nil"` for nil).
pub fn tostring_opt(v: Option<usize>) -> String {
    v.map_or_else(|| "nil".to_owned(), |i| pob_lua::format::tostring_num(i as f64))
}
