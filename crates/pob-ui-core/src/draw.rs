//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1497-1573 (DrawArrow,
//! DrawCheckMark, WorldToScreen, RenderCircle, RenderRing). Ui doc §6.11. Owner: U9. UI-U0 stub.

use crate::cx::Cx;

/// The `dir` argument of main:DrawArrow ("UP" | "RIGHT" | "DOWN" | "LEFT"; anything else draws nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowDir {
    Up,
    Right,
    Down,
    Left,
}

/// lua: Main.lua:1497-1513
pub fn draw_arrow(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, dir: ArrowDir) {
    let _ = (cx, x, y, w, h, dir);
    todo!("U9: Main.lua:1497-1513 DrawArrow")
}

/// lua: Main.lua:1515-1521
pub fn draw_check_mark(cx: &mut Cx, x: f64, y: f64, size: f64) {
    let _ = (cx, x, y, size);
    todo!("U9: Main.lua:1515-1521 DrawCheckMark")
}

/// lua: Main.lua:1523-1537 (WorldToScreen, with its `do`-block upvalues cos45/cos35/sin35)
pub fn world_to_screen(x: f64, y: f64, z: f64, width: f64, height: f64) -> (f64, f64) {
    let _ = (x, y, z, width, height);
    todo!("U9: Main.lua:1523-1537 WorldToScreen")
}

/// lua: Main.lua:1539-1560
pub fn render_circle(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64, radius: f64) {
    let _ = (cx, x, y, w, h, ox, oy, radius);
    todo!("U9: Main.lua:1539-1560 RenderCircle")
}

/// lua: Main.lua:1562-1572
pub fn render_ring(
    cx: &mut Cx,
    x: f64,
    y: f64,
    w: f64,
    h: f64,
    ox: f64,
    oy: f64,
    radius: f64,
    size: f64,
) {
    let _ = (cx, x, y, w, h, ox, oy, radius, size);
    todo!("U9: Main.lua:1562-1572 RenderRing")
}
