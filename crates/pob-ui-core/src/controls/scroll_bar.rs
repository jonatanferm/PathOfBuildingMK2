//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ScrollBarControl.lua 1-327. Owner: U9.
//!
//! @pob-file Classes/ScrollBarControl.lua @16de4b82d
//!
//! Surface notes (owner's amendments to the UI-U0 stub, ui doc §13 item 12):
//! - `self.enabled` IS the Control `enabled` property (Control.lua:60): the constructor and
//!   `SetContentDimension` write `base.enabled = Prop::Val(..)`, `IsEnabled()` reads it through
//!   GetProperty, and the raw reads (`if mOver and self.enabled`, the autoHide `shown` function,
//!   `scrollBar.enabled` in DropDownControl.lua:42/GemSelectControl.lua:39) use
//!   [`ScrollBarControl::enabled_raw`]. The stub's separate `enabled_flag` is gone.
//! - `step` and `dir` are optional constructor arguments (`step or self.width * 2`,
//!   `dir or "VERTICAL"`: DropDownControl.lua:37 and TextListControl.lua:12 pass no `dir`).
//! - The fields that start nil in Lua (`conDim`, `viewDim`, `offsetMax`, `knobDim`,
//!   `knobTravel`, `dragCX/CY`, `dragKnobPos`, `holdComp`, `holdTime`, `holdBase`,
//!   `holdPauseTime`) are `Option`s; arithmetic on one that is still nil raises the Lua error
//!   (e.g. `GetKnobPosForOffset` before the first `SetContentDimension`), `self.offsetMax or 0`
//!   in `SetOffset` keeps its default.
//! - `IsMouseOver` returns two values; [`ScrollBarControl::is_mouse_over_comp`] is the full call,
//!   the `Widget` method returns the first. The class's own `self:IsMouseOver()` calls use the
//!   two-value form statically: no PoB code overrides IsMouseOver on a scroll bar (grep).
//! - `GetTime()` values are kept as Lua numbers (`f64` of `Sys::time_ms`).

use pob_lua::math::{m_ceil, m_floor, m_max, m_min};
use pob_sg::Key;

use crate::draw::{draw_arrow, ArrowDir};
use crate::prelude::*;

/// The `dir` constructor argument ("HORIZONTAL" | "VERTICAL"; ScrollBarControl.lua:14).
/// Any other string behaves as vertical (every test is `self.dir == "HORIZONTAL"`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollDir {
    Horizontal,
    Vertical,
}

/// The second return value of `ScrollBarControl:IsMouseOver` (`mOverComp`), also `holdComp`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ScrollComp {
    Up,
    Down,
    SlideUp,
    SlideDown,
    Knob,
}

pub struct ScrollBarControl {
    pub step: f64,
    pub dir: ScrollDir,
    pub offset: f64,
    /// `self.conDim`
    pub con_dim: Option<f64>,
    /// `self.viewDim`
    pub view_dim: Option<f64>,
    /// `self.offsetMax`
    pub offset_max: Option<f64>,
    /// `self.knobDim`
    pub knob_dim: Option<f64>,
    /// `self.knobTravel`
    pub knob_travel: Option<f64>,
    /// GetProperty("locked"); nil => false
    pub locked: Prop<bool>,
    pub dragging: bool,
    /// `self.dragCX`, `self.dragCY`, `self.dragKnobPos`
    pub drag_cx: Option<f64>,
    pub drag_cy: Option<f64>,
    pub drag_knob_pos: Option<f64>,
    /// `self.holdComp` ("UP" | "DOWN" | nil)
    pub hold_comp: Option<ScrollComp>,
    pub hold_time: Option<f64>,
    pub hold_base: Option<f64>,
    /// `self.holdRepeating` (true | nil)
    pub hold_repeating: bool,
    pub hold_pause_time: Option<f64>,
}

#[track_caller]
fn num(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| crate::lua_error!("attempt to perform arithmetic on a nil value"))
}

impl ScrollBarControl {
    /// lua: ScrollBarControl.lua:14-26 ScrollBarControl(anchor, rect, step, dir, autoHide)
    /// @pob Classes/ScrollBarControl.lua:14-26 h=f54f3c1e
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        step: Option<f64>,
        dir: Option<ScrollDir>,
        auto_hide: bool,
    ) -> New<ScrollBarControl> {
        // L15
        let mut base = Base::new(anchor, rect);
        // L16 (raw `self.width` only when step is nil)
        let step = step.unwrap_or_else(|| base.width.raw() * 2.0);
        // L17
        let dir = dir.unwrap_or(ScrollDir::Vertical);
        // L19
        base.enabled = Prop::Val(false);
        // L20
        if auto_hide {
            // L21
            base.shown = Prop::f(|cx, me| {
                // L22
                ScrollBarControl::enabled_raw(cx, me)
            });
        }
        New::new(
            base,
            ScrollBarControl {
                step,
                dir,
                // L18
                offset: 0.0,
                con_dim: None,
                view_dim: None,
                offset_max: None,
                knob_dim: None,
                knob_travel: None,
                locked: Prop::Val(false),
                dragging: false,
                drag_cx: None,
                drag_cy: None,
                drag_knob_pos: None,
                hold_comp: None,
                hold_time: None,
                hold_base: None,
                hold_repeating: false,
                hold_pause_time: None,
            },
        )
    }

    /// The raw field read `self.enabled` (a boolean after the constructor; a function stored
    /// there by a caller would be truthy).
    pub fn enabled_raw(cx: &Cx, me: CtrlId) -> bool {
        match &cx.base(me).enabled {
            Prop::Val(v) => *v,
            Prop::Fn(_) => true,
        }
    }

    /// lua: ScrollBarControl.lua:28-48 SetContentDimension
    /// @pob Classes/ScrollBarControl.lua:28-48 h=03681d39
    pub fn set_content_dimension(cx: &mut Cx, me: CtrlId, con_dim: f64, view_dim: f64) {
        // L29
        cx.w_mut::<Self>(me).con_dim = Some(con_dim);
        // L30
        cx.w_mut::<Self>(me).view_dim = Some(view_dim);
        // L31
        if con_dim <= view_dim {
            // L32
            cx.base_mut(me).enabled = Prop::Val(false);
            let w = cx.w_mut::<Self>(me);
            // L33
            w.offset_max = Some(0.0);
            // L34
            w.offset = 0.0;
        } else {
            // L36
            let (width, height) = cx.size(me);
            // L37
            cx.base_mut(me).enabled = Prop::Val(true);
            let w = cx.w_mut::<Self>(me);
            // L38
            if w.dir == ScrollDir::Horizontal {
                // L39
                let knob_dim = m_max(height, (width - height * 2.0) * view_dim / con_dim);
                w.knob_dim = Some(knob_dim);
                // L40
                w.knob_travel = Some((width - height * 2.0) - knob_dim);
            } else {
                // L42
                let knob_dim = m_max(width, (height - width * 2.0) * view_dim / con_dim);
                w.knob_dim = Some(knob_dim);
                // L43
                w.knob_travel = Some((height - width * 2.0) - knob_dim);
            }
            // L45
            w.offset_max = Some(con_dim - view_dim);
            // L46
            w.offset = m_min(w.offset, con_dim - view_dim);
        }
    }

    /// lua: ScrollBarControl.lua:50-52 SetOffset
    /// @pob Classes/ScrollBarControl.lua:50-52 h=132703ab
    pub fn set_offset(cx: &mut Cx, me: CtrlId, offset: f64) {
        let w = cx.w_mut::<Self>(me);
        // L51
        w.offset = m_floor(m_max(0.0, m_min(w.offset_max.unwrap_or(0.0), offset)));
    }

    /// lua: ScrollBarControl.lua:54-56 Scroll
    /// @pob Classes/ScrollBarControl.lua:54-56 h=9b185a77
    pub fn scroll(cx: &mut Cx, me: CtrlId, mult: f64) {
        // L55
        let (offset, step) = {
            let w = cx.w::<Self>(me);
            (w.offset, w.step)
        };
        Self::set_offset(cx, me, offset + step * mult);
    }

    /// lua: ScrollBarControl.lua:58-64 ScrollIntoView
    /// @pob Classes/ScrollBarControl.lua:58-64 h=49e368d2
    pub fn scroll_into_view(cx: &mut Cx, me: CtrlId, min_dim: f64, size: f64) {
        let offset = cx.w::<Self>(me).offset;
        // L59
        if offset > min_dim {
            // L60
            Self::set_offset(cx, me, min_dim);
        } else if offset + num(cx.w::<Self>(me).view_dim) < min_dim + size {
            // L62
            let view_dim = num(cx.w::<Self>(me).view_dim);
            Self::set_offset(cx, me, min_dim + size - view_dim);
        }
    }

    /// lua: ScrollBarControl.lua:66-68 SetOffsetFromKnobPos
    /// @pob Classes/ScrollBarControl.lua:66-68 h=6c4b3bc2
    pub fn set_offset_from_knob_pos(cx: &mut Cx, me: CtrlId, knob_pos: f64) {
        // L67
        let (offset_max, knob_travel) = {
            let w = cx.w::<Self>(me);
            (num(w.offset_max), num(w.knob_travel))
        };
        Self::set_offset(cx, me, offset_max * (knob_pos / knob_travel));
    }

    /// lua: ScrollBarControl.lua:70-72 GetKnobPosForOffset
    /// @pob Classes/ScrollBarControl.lua:70-72 h=dea3dbd8
    pub fn get_knob_pos_for_offset(cx: &Cx, me: CtrlId) -> f64 {
        let w = cx.w::<Self>(me);
        // L71
        num(w.knob_travel) * (w.offset / num(w.offset_max))
    }

    /// lua: ScrollBarControl.lua:74-111 (both return values)
    /// @pob Classes/ScrollBarControl.lua:74-111 h=0908320b
    pub fn is_mouse_over_comp(cx: &Cx, me: CtrlId) -> (bool, Option<ScrollComp>) {
        // L75
        if !cx.is_shown(me) {
            // L76
            return (false, None);
        }
        // L78
        let (x, y) = cx.pos(me);
        // L79
        let (width, height) = cx.size(me);
        // L80
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L81
        let m_over =
            cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height;
        // L82
        let mut m_over_comp = None;
        // L83
        if m_over && Self::enabled_raw(cx, me) {
            // L84-L85 (`local relDim`, `local shortDim, longDim`, assigned below)
            // L86
            let (rel_dim, short_dim, long_dim) = if cx.w::<Self>(me).dir == ScrollDir::Horizontal {
                // L87-L89
                (cursor_x - x, height, width)
            } else {
                // L91-L93
                (cursor_y - y, width, height)
            };
            // L95
            if rel_dim < short_dim {
                // L96
                m_over_comp = Some(ScrollComp::Up);
            } else if rel_dim >= long_dim - short_dim {
                // L98
                m_over_comp = Some(ScrollComp::Down);
            } else {
                // L100
                let knob_pos = Self::get_knob_pos_for_offset(cx, me);
                // L101
                if rel_dim < short_dim + knob_pos {
                    // L102
                    m_over_comp = Some(ScrollComp::SlideUp);
                } else if rel_dim >= short_dim + knob_pos + num(cx.w::<Self>(me).knob_dim) {
                    // L104
                    m_over_comp = Some(ScrollComp::SlideDown);
                } else {
                    // L106
                    m_over_comp = Some(ScrollComp::Knob);
                }
            }
        }
        // L110
        (m_over, m_over_comp)
    }

    /// lua: ScrollBarControl.lua:321-324 IsScrollDownKey
    /// @pob Classes/ScrollBarControl.lua:321-324 h=b23e5c2d
    pub fn is_scroll_down_key(_cx: &Cx, _me: CtrlId, key: Key) -> bool {
        // L323 isValueInTable({"WHEELDOWN", "PAGEDOWN"}, key)
        matches!(key, Key::WheelDown | Key::PageDown)
    }

    /// lua: ScrollBarControl.lua:325-327 IsScrollUpKey
    /// @pob Classes/ScrollBarControl.lua:325-327 h=e39b7a04
    pub fn is_scroll_up_key(_cx: &Cx, _me: CtrlId, key: Key) -> bool {
        // L326 isValueInTable({"WHEELUP", "PAGEUP"}, key)
        matches!(key, Key::WheelUp | Key::PageUp)
    }
}

impl Widget for ScrollBarControl {
    const NAME: &'static str = "ScrollBarControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);

    /// lua: ScrollBarControl.lua:74-111 (first return value)
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        Self::is_mouse_over_comp(cx, me).0
    }

    /// lua: ScrollBarControl.lua:113-257
    /// @pob Classes/ScrollBarControl.lua:113-257 h=a8e5559e
    fn draw(cx: &mut Cx, me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {
        use ScrollComp::{Down, Knob, SlideDown, SlideUp, Up};
        // L114
        let (x, y) = cx.pos(me);
        // L115
        let (width, height) = cx.size(me);
        // L116
        let enabled = cx.is_enabled(me);
        // L117
        let (_m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
        // L118
        let dir = cx.w::<Self>(me).dir;
        // L119
        if !cx.sys.is_key_down(Key::LeftButton) {
            // L120
            cx.w_mut::<Self>(me).dragging = false;
        }
        // L122
        if cx.w::<Self>(me).dragging {
            // L123
            let (cursor_x, cursor_y) = cx.sys.cursor_pos();
            let (drag_cx, drag_cy, drag_knob_pos) = {
                let w = cx.w::<Self>(me);
                (w.drag_cx, w.drag_cy, w.drag_knob_pos)
            };
            // L124
            if dir == ScrollDir::Horizontal {
                // L125
                Self::set_offset_from_knob_pos(
                    cx,
                    me,
                    (cursor_x - num(drag_cx)) + num(drag_knob_pos),
                );
            } else {
                // L127
                Self::set_offset_from_knob_pos(
                    cx,
                    me,
                    (cursor_y - num(drag_cy)) + num(drag_knob_pos),
                );
            }
        } else if let Some(hold_comp) = cx.w::<Self>(me).hold_comp {
            // L130
            if m_over_comp == Some(hold_comp) {
                // L131
                let now = cx.sys.time_ms() as f64;
                // L132
                if !cx.w::<Self>(me).hold_repeating {
                    // L133
                    if now - num(cx.w::<Self>(me).hold_time) > 500.0 {
                        let w = cx.w_mut::<Self>(me);
                        // L134
                        w.hold_repeating = true;
                        // L135
                        w.hold_time = Some(now - 1.0);
                    }
                }
                // L138
                if cx.w::<Self>(me).hold_repeating {
                    // L139
                    if let Some(pause) = cx.w::<Self>(me).hold_pause_time {
                        let w = cx.w_mut::<Self>(me);
                        // L140
                        w.hold_time = Some(num(w.hold_time) + (now - pause));
                        // L141
                        w.hold_pause_time = None;
                    }
                    // L143
                    let time = now - num(cx.w::<Self>(me).hold_time);
                    let (hold_base, step) = {
                        let w = cx.w::<Self>(me);
                        (w.hold_base, w.step)
                    };
                    // L144
                    if hold_comp == Up {
                        // L145
                        Self::set_offset(cx, me, num(hold_base) - m_ceil(time / 50.0) * step);
                    } else if hold_comp == Down {
                        // L147
                        Self::set_offset(cx, me, num(hold_base) + m_ceil(time / 50.0) * step);
                    }
                }
            } else if cx.w::<Self>(me).hold_repeating && cx.w::<Self>(me).hold_pause_time.is_none()
            {
                // L151
                let now = cx.sys.time_ms() as f64;
                cx.w_mut::<Self>(me).hold_pause_time = Some(now);
            }
        }
        let sg = &mut cx.sg;
        // Draw up/left button background
        // L155
        if !enabled {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over_comp == Some(Up) {
            sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L162
        if dir == ScrollDir::Horizontal {
            sg.draw_image(None, x, y, height, height);
        } else {
            sg.draw_image(None, x, y, width, width);
        }
        // L167
        if enabled && m_over_comp == Some(Up) {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L172
        if dir == ScrollDir::Horizontal {
            sg.draw_image(None, x + 1.0, y + 1.0, height - 2.0, height - 2.0);
        } else {
            sg.draw_image(None, x + 1.0, y + 1.0, width - 2.0, width - 2.0);
        }
        // Draw up/left arrow
        // L178
        if !enabled {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over_comp == Some(Up) {
            sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L185
        if dir == ScrollDir::Horizontal {
            // L186
            draw_arrow(
                cx,
                x + height / 2.0,
                y + height / 2.0,
                height / 2.0,
                height / 2.0,
                ArrowDir::Left,
            );
        } else {
            // L188
            draw_arrow(
                cx,
                x + width / 2.0,
                y + width / 2.0,
                width / 2.0,
                width / 2.0,
                ArrowDir::Up,
            );
        }
        let sg = &mut cx.sg;
        // Draw down/right button background
        // L191
        if !enabled {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over_comp == Some(Down) {
            sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L198
        if dir == ScrollDir::Horizontal {
            sg.draw_image(None, x + width - height, y, height, height);
        } else {
            sg.draw_image(None, x, y + height - width, width, width);
        }
        // L203
        if enabled && m_over_comp == Some(Down) {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L208
        if dir == ScrollDir::Horizontal {
            sg.draw_image(
                None,
                x + width - height + 1.0,
                y + 1.0,
                height - 2.0,
                height - 2.0,
            );
        } else {
            sg.draw_image(
                None,
                x + 1.0,
                y + height - width + 1.0,
                width - 2.0,
                width - 2.0,
            );
        }
        // Draw down/right arrow
        // L214
        if !enabled {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over_comp == Some(Down) {
            sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L221
        if dir == ScrollDir::Horizontal {
            // L222
            draw_arrow(
                cx,
                x + width - height / 2.0,
                y + height / 2.0,
                height / 2.0,
                height / 2.0,
                ArrowDir::Right,
            );
        } else {
            // L224
            draw_arrow(
                cx,
                x + width / 2.0,
                y + height - width / 2.0,
                width / 2.0,
                width / 2.0,
                ArrowDir::Down,
            );
        }
        let dragging = cx.w::<Self>(me).dragging;
        let sg = &mut cx.sg;
        // Draw slide background
        // L227
        if !enabled {
            sg.set_draw_color(0.33, 0.33, 0.33);
        } else if dragging
            || m_over_comp == Some(Knob)
            || m_over_comp == Some(SlideUp)
            || m_over_comp == Some(SlideDown)
        {
            sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L234
        if dir == ScrollDir::Horizontal {
            // L235
            sg.draw_image(None, x + height, y, width - height * 2.0, height);
            // L236
            sg.set_draw_color(0.0, 0.0, 0.0);
            // L237
            sg.draw_image(
                None,
                x + height,
                y + 1.0,
                width - height * 2.0,
                height - 2.0,
            );
        } else {
            // L239
            sg.draw_image(None, x, y + width, width, height - width * 2.0);
            // L240
            sg.set_draw_color(0.0, 0.0, 0.0);
            // L241
            sg.draw_image(None, x + 1.0, y + width, width - 2.0, height - width * 2.0);
        }
        // Draw knob
        // L244
        if enabled {
            // L245
            if dragging || m_over_comp == Some(Knob) {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            } else {
                cx.sg.set_draw_color(0.5, 0.5, 0.5);
            }
            // L250
            let knob_pos = Self::get_knob_pos_for_offset(cx, me);
            let knob_dim = num(cx.w::<Self>(me).knob_dim);
            // L251
            if dir == ScrollDir::Horizontal {
                // L252
                cx.sg.draw_image(
                    None,
                    x + height + knob_pos + 1.0,
                    y + 2.0,
                    knob_dim - 2.0,
                    height - 4.0,
                );
            } else {
                // L254
                cx.sg.draw_image(
                    None,
                    x + 2.0,
                    y + width + knob_pos + 1.0,
                    width - 4.0,
                    knob_dim - 2.0,
                );
            }
        }
    }

    /// lua: ScrollBarControl.lua:259-293
    /// @pob Classes/ScrollBarControl.lua:259-293 h=c61bfbfb
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L260
        if !cx.is_shown(me) || !cx.is_enabled(me) || cx.prop::<Self, _>(me, |w| &w.locked) {
            // L261
            return None;
        }
        // L263
        if key == Key::LeftButton {
            // L264
            let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
            // L265
            if !m_over {
                // L266
                return None;
            }
            match m_over_comp {
                // L268
                Some(ScrollComp::Knob) => {
                    // L269
                    if !cx.w::<Self>(me).dragging {
                        // L270
                        cx.w_mut::<Self>(me).dragging = true;
                        // L271
                        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
                        // L272
                        cx.w_mut::<Self>(me).drag_cx = Some(cursor_x);
                        // L273
                        cx.w_mut::<Self>(me).drag_cy = Some(cursor_y);
                        // L274
                        let kp = Self::get_knob_pos_for_offset(cx, me);
                        cx.w_mut::<Self>(me).drag_knob_pos = Some(kp);
                    }
                }
                // L276
                Some(ScrollComp::Up) => {
                    // L277
                    Self::scroll(cx, me, -1.0);
                    // L278
                    cx.w_mut::<Self>(me).hold_comp = Some(ScrollComp::Up);
                    // L279
                    let now = cx.sys.time_ms() as f64;
                    cx.w_mut::<Self>(me).hold_time = Some(now);
                    // L280
                    let w = cx.w_mut::<Self>(me);
                    w.hold_base = Some(w.offset);
                }
                // L281
                Some(ScrollComp::Down) => {
                    // L282
                    Self::scroll(cx, me, 1.0);
                    // L283
                    cx.w_mut::<Self>(me).hold_comp = Some(ScrollComp::Down);
                    // L284
                    let now = cx.sys.time_ms() as f64;
                    cx.w_mut::<Self>(me).hold_time = Some(now);
                    // L285
                    let w = cx.w_mut::<Self>(me);
                    w.hold_base = Some(w.offset);
                }
                // L286
                Some(ScrollComp::SlideUp) => {
                    // L287
                    let kp = Self::get_knob_pos_for_offset(cx, me);
                    let knob_dim = num(cx.w::<Self>(me).knob_dim);
                    Self::set_offset_from_knob_pos(cx, me, kp - knob_dim);
                }
                // L288
                Some(ScrollComp::SlideDown) => {
                    // L289
                    let kp = Self::get_knob_pos_for_offset(cx, me);
                    let knob_dim = num(cx.w::<Self>(me).knob_dim);
                    Self::set_offset_from_knob_pos(cx, me, kp + knob_dim);
                }
                None => {}
            }
        }
        // L292
        Some(me)
    }

    /// lua: ScrollBarControl.lua:295-319
    /// @pob Classes/ScrollBarControl.lua:295-319 h=9e8aa505
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L296
        if !cx.is_shown(me) || !cx.is_enabled(me) || cx.prop::<Self, _>(me, |w| &w.locked) {
            // L297
            return None;
        }
        // L299
        if key == Key::LeftButton {
            // L300
            if cx.w::<Self>(me).dragging {
                // L301
                cx.w_mut::<Self>(me).dragging = false;
                // L302
                let (cursor_x, cursor_y) = cx.sys.cursor_pos();
                let (dir, drag_cx, drag_cy, drag_knob_pos) = {
                    let w = cx.w::<Self>(me);
                    (w.dir, w.drag_cx, w.drag_cy, w.drag_knob_pos)
                };
                // L303
                if dir == ScrollDir::Horizontal {
                    // L304
                    Self::set_offset_from_knob_pos(
                        cx,
                        me,
                        (cursor_x - num(drag_cx)) + num(drag_knob_pos),
                    );
                } else {
                    // L306
                    Self::set_offset_from_knob_pos(
                        cx,
                        me,
                        (cursor_y - num(drag_cy)) + num(drag_knob_pos),
                    );
                }
            } else if cx.w::<Self>(me).hold_comp.is_some() {
                let w = cx.w_mut::<Self>(me);
                // L309
                w.hold_comp = None;
                // L310
                w.hold_repeating = false;
                // L311
                w.hold_pause_time = None;
            }
        } else if Self::is_scroll_down_key(cx, me, key) {
            // L315
            Self::scroll(cx, me, 1.0);
        } else if Self::is_scroll_up_key(cx, me, key) {
            // L317
            Self::scroll(cx, me, -1.0);
        }
        None
    }
}
