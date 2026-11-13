//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1497-1573 (DrawArrow,
//! DrawCheckMark, WorldToScreen, RenderCircle, RenderRing). Ui doc §6.11. Owner: U9.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! Numeric `for` loops with a fractional step (`for d = 0, 360, 0.15`) are LuaJIT's: the loop
//! variable is accumulated (`d = d + step`, `BC_FORL`), not recomputed from an index, so the
//! sequence of `d` (and the rounding drift it carries) is Lua's.
//!
//! `m_sin`/`m_cos` are libm `sin`/`cos`, as LuaJIT's `math.sin`/`math.cos` (`math_extern`).
//!
//! ORDER: RenderCircle's `minX`/`maxX` tables are only indexed (`for ly = minY, maxY`), never
//! iterated with `pairs`, so a map stands in for them; no order is observable.
#![allow(clippy::many_single_char_names)] // Lua names (x, y, w, h, r, g, b)

use std::collections::HashMap;

use pob_lua::math::{m_max, m_min, m_pi, round};

use crate::cx::Cx;

/// The `dir` argument of main:DrawArrow ("UP" | "RIGHT" | "DOWN" | "LEFT"; anything else draws nothing).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ArrowDir {
    Up,
    Right,
    Down,
    Left,
}

impl ArrowDir {
    /// The Lua string; `None` for a string DrawArrow ignores.
    pub fn from_lua(s: &str) -> Option<ArrowDir> {
        match s {
            "UP" => Some(ArrowDir::Up),
            "RIGHT" => Some(ArrowDir::Right),
            "DOWN" => Some(ArrowDir::Down),
            "LEFT" => Some(ArrowDir::Left),
            _ => None,
        }
    }
}

/// A Lua numeric `for v = start, stop, step do body(v) end` (LuaJIT `BC_FORI`/`BC_FORL`): the
/// direction comes from the sign of `step`, the variable is accumulated (`v = v + step`), and a
/// NaN bound or start runs no iteration. A zero step loops forever, as in Lua.
pub fn lua_for_num(start: f64, stop: f64, step: f64, mut body: impl FnMut(f64)) {
    let up = !step.is_sign_negative();
    let mut v = start;
    while if up { v <= stop } else { v >= stop } {
        body(v);
        v += step;
    }
}

/// lua: Main.lua:1497-1513
/// @pob Modules/Main.lua:1497-1513 h=8af24e36
pub fn draw_arrow(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, dir: ArrowDir) {
    let (width, height) = (w, h);
    // L1498
    let x1 = x - width / 2.0;
    // L1499
    let x2 = x + width / 2.0;
    // L1500
    let x_mid = (x1 + x2) / 2.0;
    // L1501
    let y1 = y - height / 2.0;
    // L1502
    let y2 = y + height / 2.0;
    // L1503
    let y_mid = (y1 + y2) / 2.0;
    // L1504
    match dir {
        ArrowDir::Up => cx
            .sg
            .draw_image_quad(None, [x_mid, y1, x_mid, y1, x2, y2, x1, y2]),
        ArrowDir::Right => cx
            .sg
            .draw_image_quad(None, [x1, y1, x2, y_mid, x2, y_mid, x1, y2]),
        ArrowDir::Down => cx
            .sg
            .draw_image_quad(None, [x1, y1, x2, y1, x_mid, y2, x_mid, y2]),
        ArrowDir::Left => cx
            .sg
            .draw_image_quad(None, [x1, y_mid, x2, y1, x2, y2, x1, y_mid]),
    }
}

/// lua: Main.lua:1515-1521
/// @pob Modules/Main.lua:1515-1521 h=5d2de1e9
pub fn draw_check_mark(cx: &mut Cx, x: f64, y: f64, size: f64) {
    // L1516
    let size = size / 0.8;
    // L1517
    let x = x - size / 2.0;
    // L1518
    let y = y - size / 2.0;
    // L1519
    cx.sg.draw_image_quad(
        None,
        [
            x + size * 0.15,
            y + size * 0.50,
            x + size * 0.30,
            y + size * 0.45,
            x + size * 0.50,
            y + size * 0.80,
            x + size * 0.40,
            y + size * 0.90,
        ],
    );
    // L1520
    cx.sg.draw_image_quad(
        None,
        [
            x + size * 0.40,
            y + size * 0.90,
            x + size * 0.35,
            y + size * 0.75,
            x + size * 0.80,
            y + size * 0.10,
            x + size * 0.90,
            y + size * 0.20,
        ],
    );
}

#[inline]
fn m_sin(x: f64) -> f64 {
    x.sin()
}
#[inline]
fn m_cos(x: f64) -> f64 {
    x.cos()
}

/// The `do`-block upvalues of WorldToScreen (Main.lua:1524-1526), computed once like the Lua.
struct Trig {
    cos45: f64,
    cos35: f64,
    sin35: f64,
}

thread_local! {
    static TRIG: Trig = Trig {
        // L1524
        cos45: m_cos(m_pi / 4.0),
        // L1525
        cos35: m_cos(m_pi * 0.195),
        // L1526
        sin35: m_sin(m_pi * 0.195),
    };
}

/// lua: Main.lua:1523-1537 (WorldToScreen, with its `do`-block upvalues cos45/cos35/sin35)
/// @pob Modules/Main.lua:1523-1537 h=b40ed37f
pub fn world_to_screen(x: f64, y: f64, z: f64, width: f64, height: f64) -> (f64, f64) {
    TRIG.with(|t| {
        let (cos45, cos35, sin35) = (t.cos45, t.cos35, t.sin35);
        // World -> camera
        // L1529
        let cx = (x - y) * cos45;
        // L1530
        let cy = -5.33 - (y + x) * cos45 * cos35 - z * sin35;
        // L1531
        let cz = 122.0 + (y + x) * cos45 * sin35 - z * cos35;
        // Camera -> screen
        // L1533
        let sx = width * 0.5 + cx / cz * 1.27 * height;
        // L1534
        let sy = height * 0.5 + cy / cz * 1.27 * height;
        // L1535
        (round(sx, None), round(sy, None))
    })
}

/// A Lua table indexed by number: integer-valued keys only (`round` results), so a float key
/// with a fraction (never stored by RenderCircle) reads nil.
fn num_key(k: f64) -> Option<i64> {
    // Lua normalises an integral float key; +0 and -0 are the same key.
    (k.fract() == 0.0 && k.abs() < 9.0e15).then_some(k as i64)
}

/// lua: Main.lua:1539-1560
/// @pob Modules/Main.lua:1539-1560 h=f0a2b75e
pub fn render_circle(cx: &mut Cx, x: f64, y: f64, w: f64, h: f64, ox: f64, oy: f64, radius: f64) {
    let (width, height) = (w, h);
    // L1540 (`tempTable1`/`tempTable2` are undeclared globals in Main.lua, so `wipeTable(nil)`
    // returns a fresh table on every call) // LUA-GLOBAL-NIL
    let mut min_x: HashMap<i64, f64> = HashMap::new();
    // L1541
    let mut max_x: HashMap<i64, f64> = HashMap::new();
    // L1542
    let mut min_y = height;
    // L1543
    let mut max_y = 0.0;
    // L1544 for d = 0, 360, 0.15 do
    let mut d = 0.0_f64;
    while d <= 360.0 {
        // L1545
        let r = d / 180.0 * m_pi;
        // L1546
        let (px, py) = world_to_screen(
            ox + m_sin(r) * radius,
            oy + m_cos(r) * radius,
            0.0,
            width,
            height,
        );
        // L1547
        if py >= 0.0 && py < height {
            // L1548
            let px = m_min(width, m_max(0.0, px));
            // L1549
            min_y = m_min(min_y, py);
            // L1550
            max_y = m_max(max_y, py);
            let k = num_key(py).unwrap_or_else(|| crate::lua_error!("table index is NaN"));
            // L1551
            let cur = min_x.get(&k).copied().unwrap_or(px);
            min_x.insert(k, m_min(cur, px));
            // L1552
            let cur = max_x.get(&k).copied().unwrap_or(px);
            max_x.insert(k, m_max(cur, px));
        }
        d += 0.15;
    }
    // L1555 for ly = minY, maxY do
    let mut ly = min_y;
    while ly <= max_y {
        // L1556
        if let Some(mn) = num_key(ly).and_then(|k| min_x.get(&k).copied()) {
            // L1557
            let mx = num_key(ly)
                .and_then(|k| max_x.get(&k).copied())
                .unwrap_or_else(|| {
                    crate::lua_error!("attempt to perform arithmetic on a nil value")
                });
            cx.sg.draw_image(None, x + mn, y + ly, mx - mn + 1.0, 1.0);
        }
        ly += 1.0;
    }
}

/// lua: Main.lua:1562-1572
/// @pob Modules/Main.lua:1562-1572 h=ed8a1690
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
    let (width, height) = (w, h);
    // L1563
    let mut last_x: Option<f64> = None;
    let mut last_y: Option<f64> = None;
    // L1564 for d = 0, 360, 0.2 do
    let mut d = 0.0_f64;
    while d <= 360.0 {
        // L1565
        let r = d / 180.0 * m_pi;
        // L1566
        let (px, py) = world_to_screen(
            ox + m_sin(r) * radius,
            oy + m_cos(r) * radius,
            0.0,
            width,
            height,
        );
        // L1567 (`px ~= lastX` is true while lastX is nil)
        if px >= -size / 2.0
            && px < width + size / 2.0
            && py >= -size / 2.0
            && py < height + size / 2.0
            && (last_x != Some(px) || last_y != Some(py))
        {
            // L1568
            cx.sg
                .draw_image(None, x + px - size / 2.0, y + py, size, size);
            // L1569
            last_x = Some(px);
            last_y = Some(py);
        }
        d += 0.2;
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn world_to_screen_origin_is_screen_centre_offset() {
        // x = y = z = 0: cx = 0, cy = -5.33, cz = 122.
        let (sx, sy) = world_to_screen(0.0, 0.0, 0.0, 200.0, 100.0);
        assert_eq!(sx, 100.0);
        assert_eq!(sy, round(50.0 + -5.33 / 122.0 * 1.27 * 100.0, None));
    }

    #[test]
    fn lua_for_num_follows_luajit() {
        let collect = |a, b, s| {
            let mut v = Vec::new();
            lua_for_num(a, b, s, |d| v.push(d));
            v
        };
        assert_eq!(collect(0.0, 1.0, 0.25), vec![0.0, 0.25, 0.5, 0.75, 1.0]);
        assert_eq!(collect(3.0, 1.0, -1.0), vec![3.0, 2.0, 1.0]);
        // `-0` step counts as negative (sign bit, vm_*.dasc `tbnz CARG3, #63`)
        assert_eq!(collect(0.0, 1.0, -0.0), Vec::<f64>::new());
        assert_eq!(collect(0.0, f64::NAN, 1.0).len(), 0);
        // an infinite step runs once
        assert_eq!(collect(0.0, 10.0, f64::INFINITY), vec![0.0]);
        // accumulation: 0.1 added ten times is below 1.0 by an ulp, so 1.0 is not reached exactly
        let v = collect(0.0, 1.0, 0.1);
        assert_eq!(v.len(), 11);
        assert_ne!(v[10], 1.0);
    }
}
