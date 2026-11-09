//! Support module of the U13 galleries (`u13_*`). Owner: U13.

use pob_lua::format::tostring_num;
use pob_ui_core::controls::drop_down::DropItem;
use pob_ui_core::prelude::*;

/// The galleries' `options(prefix, n)`: string entries, every 7th one colour-coded.
pub fn options(prefix: &str, n: usize) -> Vec<DropItem> {
    (1..=n)
        .map(|i| {
            let colour = if i % 7 == 0 { "^xE05030" } else { "" };
            DropItem::str(format!("{colour}{prefix} {i}"))
        })
        .collect()
}

/// Lua `tostring(index)` of an optional index (`"nil"` for nil).
pub fn tostring_opt(v: Option<usize>) -> String {
    v.map_or_else(|| "nil".to_owned(), |i| tostring_num(i as f64))
}

/// Lua `tostring(value)` of a string entry (`"nil"` for nil; a table entry is never printed by
/// the galleries).
pub fn tostring_item(v: Option<&DropItem>) -> String {
    v.map_or_else(
        || "nil".to_owned(),
        |v| v.as_str().expect("a string entry").to_owned(),
    )
}

/// `G:OnFrame` of a single-host U13 gallery: input, a background, the controls.
pub fn frame_bg(cx: &mut Cx, host: CtrlId, ev: &mut InputEvents, vp: &ViewPort) {
    cx.process_controls_input(host, ev, vp);
    cx.sg.set_draw_color(0.1, 0.1, 0.1);
    cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
    cx.draw_controls(host, vp, None);
}
