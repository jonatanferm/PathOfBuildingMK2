//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SliderControl.lua 1-185. Owner: U9.
//!
//! @pob-file Classes/SliderControl.lua @16de4b82d
//!
//! Surface notes (owner's amendments to the UI-U0 stub, ui doc §13 item 12):
//! - `scroll_wheel_speed_tbl` is the Lua table `{ SHIFT = .., CTRL = .., DEFAULT = .. }` as a
//!   struct (every PoB caller passes all three keys: TreeTab.lua:1741, :1763-1764).
//! - `knob_size` (`self.knobSize = self.height - 2`, raw height at construction), `drag_cx`,
//!   `drag_knob_x` are fields; `div_count` is `Option<f64>` (ItemsTab.lua:2423 sets it to nil).
//! - `IsMouseOver` returns two values; [`SliderControl::is_mouse_over_comp`] is the full call,
//!   the `Widget` method returns the first. The class's own `self:IsMouseOver()` calls use the
//!   two-value form statically: no PoB code overrides IsMouseOver on a slider instance (grep).
//! - `GetDivVal` returns `(divIndex, fraction)`.
//! - `main.invertSliderScrollDirection` (L168, L176) is not a `MainCore` field. CCR-0069 asks for
//!   it; until then it lives in [`INVERT_SLIDER_SCROLL_DIRECTION`] (`nil`/false by default, as
//!   main:Init leaves it until Settings.xml sets it). // CCR-0069
//! - `Draw`'s divider loop `for d = 0, knobTravel + 0.5, knobTravel / self.divCount` is a Lua
//!   float `for` ([`crate::draw::lua_for_num`]).

use std::cell::Cell;
use std::rc::Rc;

use pob_lua::math::{m_ceil, m_max, m_min};
use pob_sg::Key;

use crate::draw::{draw_arrow, lua_for_num, ArrowDir};
use crate::prelude::*;
use crate::tooltip::TooltipHost;

thread_local! {
    /// `main.invertSliderScrollDirection` (Main.lua:653-654 reads it from Settings.xml, :1173 sets it). // CCR-0069
    pub static INVERT_SLIDER_SCROLL_DIRECTION: Cell<bool> = const { Cell::new(false) };
}

/// `scrollWheelSpeedTbl`: `{ ["SHIFT"] = .., ["CTRL"] = .., ["DEFAULT"] = .. }`
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct ScrollWheelSpeedTbl {
    pub shift: f64,
    pub ctrl: f64,
    pub default: f64,
}

/// The second return value of `SliderControl:IsMouseOver` (`mOverComp`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SliderComp {
    Knob,
    Slide,
}

pub struct SliderControl {
    pub tt: TooltipHost<f64>,
    /// `self.knobSize`
    pub knob_size: f64,
    pub val: f64,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, f64)>>,
    pub scroll_wheel_speed_tbl: ScrollWheelSpeedTbl,
    /// `self.divCount`
    pub div_count: Option<f64>,
    pub dragging: bool,
    /// `self.dragCX`
    pub drag_cx: Option<f64>,
    /// `self.dragKnobX`
    pub drag_knob_x: Option<f64>,
}

#[track_caller]
fn num(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| crate::lua_error!("attempt to perform arithmetic on a nil value"))
}

impl SliderControl {
    /// lua: SliderControl.lua:13-21 SliderControl(anchor, rect, changeFunc, scrollWheelSpeedTbl)
    /// @pob Classes/SliderControl.lua:13-21 h=5c76abea
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        change_func: Option<Rc<dyn Fn(&mut Cx, f64)>>,
        scroll_wheel_speed_tbl: Option<ScrollWheelSpeedTbl>,
    ) -> New<SliderControl> {
        // L14
        let base = Base::new(anchor, rect);
        // L16 (raw `self.height`)
        let knob_size = base.height.raw() - 2.0;
        New::new(
            base,
            SliderControl {
                // L15
                tt: TooltipHost::new(None),
                knob_size,
                // L17
                val: 0.0,
                // L18
                change_func,
                // L19
                scroll_wheel_speed_tbl: scroll_wheel_speed_tbl.unwrap_or(ScrollWheelSpeedTbl {
                    shift: 0.25,
                    ctrl: 0.01,
                    default: 0.05,
                }),
                div_count: None,
                dragging: false,
                drag_cx: None,
                drag_knob_x: None,
            },
        )
    }

    /// lua: SliderControl.lua:23-42 (both return values)
    /// @pob Classes/SliderControl.lua:23-42 h=49d481e0
    pub fn is_mouse_over_comp(cx: &Cx, me: CtrlId) -> (bool, Option<SliderComp>) {
        // L24
        if !cx.is_shown(me) {
            // L25
            return (false, None);
        }
        // L27
        let (x, y) = cx.pos(me);
        // L28
        let (width, height) = cx.size(me);
        // L29
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L30
        let m_over =
            cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height;
        // L31
        let mut m_over_comp = None;
        // L32
        if m_over {
            // L33
            let rel_x = cursor_x - x - 2.0;
            // L34
            let knob_x = Self::get_knob_x_for_val(cx, me);
            let knob_size = cx.w::<Self>(me).knob_size;
            // L35
            if rel_x >= knob_x && rel_x < knob_x + knob_size {
                // L36
                m_over_comp = Some(SliderComp::Knob);
            } else {
                // L38
                m_over_comp = Some(SliderComp::Slide);
            }
        }
        // L41
        (m_over, m_over_comp)
    }

    /// lua: SliderControl.lua:44-47 GetKnobTravel
    /// @pob Classes/SliderControl.lua:44-47 h=09d19128
    pub fn get_knob_travel(cx: &Cx, me: CtrlId) -> f64 {
        // L45
        let (width, _height) = cx.size(me);
        // L46
        width - cx.w::<Self>(me).knob_size - 2.0
    }

    /// lua: SliderControl.lua:49-57 SetVal
    /// @pob Classes/SliderControl.lua:49-57 h=686c1f94
    pub fn set_val(cx: &mut Cx, me: CtrlId, new_val: f64) {
        // L50
        let new_val = m_max(0.0, m_min(1.0, new_val));
        // L51
        if new_val != cx.w::<Self>(me).val {
            // L52
            cx.w_mut::<Self>(me).val = new_val;
            // L53
            if let Some(f) = cx.w::<Self>(me).change_func.clone() {
                // L54
                let val = cx.w::<Self>(me).val;
                f(cx, val);
            }
        }
    }

    /// lua: SliderControl.lua:59-67 GetDivVal (`val` nil => self.val); returns divIndex, fraction
    /// @pob Classes/SliderControl.lua:59-67 h=26726f07
    pub fn get_div_val(cx: &Cx, me: CtrlId, val: Option<f64>) -> (f64, f64) {
        let w = cx.w::<Self>(me);
        // L60
        let val = val.unwrap_or(w.val);
        // L61
        match w.div_count {
            Some(div_count) if div_count > 1.0 => {
                // L62
                let div_index = m_max(m_ceil(val * div_count), 1.0);
                // L63
                (div_index, val * div_count - div_index + 1.0)
            }
            // L65
            _ => (1.0, val),
        }
    }

    /// lua: SliderControl.lua:69-71 SetValFromKnobX
    /// @pob Classes/SliderControl.lua:69-71 h=9f6db18e
    pub fn set_val_from_knob_x(cx: &mut Cx, me: CtrlId, knob_x: f64) {
        // L70
        let v = knob_x / Self::get_knob_travel(cx, me);
        Self::set_val(cx, me, v);
    }

    /// lua: SliderControl.lua:73-76 GetKnobXForVal
    /// @pob Classes/SliderControl.lua:73-76 h=3e7d52ba
    pub fn get_knob_x_for_val(cx: &Cx, me: CtrlId) -> f64 {
        // L74
        let knob_travel = Self::get_knob_travel(cx, me);
        // L75
        knob_travel * cx.w::<Self>(me).val
    }
}

impl Widget for SliderControl {
    const NAME: &'static str = "SliderControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);

    /// lua: SliderControl.lua:23-42 (first return value)
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        Self::is_mouse_over_comp(cx, me).0
    }

    /// lua: SliderControl.lua:78-133
    /// @pob Classes/SliderControl.lua:78-133 h=9593c211
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L79
        let (x, y) = cx.pos(me);
        // L80
        let (width, height) = cx.size(me);
        // L81
        let enabled = cx.is_enabled(me);
        // L82
        let knob_travel = Self::get_knob_travel(cx, me);
        // L83
        if cx.w::<Self>(me).dragging && !cx.sys.is_key_down(Key::LeftButton) {
            // L84
            cx.w_mut::<Self>(me).dragging = false;
        }
        // L86
        if cx.w::<Self>(me).dragging {
            // L87
            let (cursor_x, _cursor_y) = cx.sys.cursor_pos();
            // L88
            let (drag_cx, drag_knob_x) = {
                let w = cx.w::<Self>(me);
                (num(w.drag_cx), num(w.drag_knob_x))
            };
            Self::set_val_from_knob_x(cx, me, (cursor_x - drag_cx) + drag_knob_x);
        }
        // L90
        let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
        let dragging = cx.w::<Self>(me).dragging;
        // L91
        if !enabled {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if dragging || m_over {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L98
        cx.sg.draw_image(None, x, y, width, height);
        // L99
        cx.sg.set_draw_color(0.0, 0.0, 0.0);
        // L100
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L101
        if enabled {
            let (div_count, knob_size) = {
                let w = cx.w::<Self>(me);
                (w.div_count, w.knob_size)
            };
            // L102
            if let Some(div_count) = div_count {
                // L103
                cx.sg.set_draw_color(0.33, 0.33, 0.33);
                // L104
                lua_for_num(0.0, knob_travel + 0.5, knob_travel / div_count, |d| {
                    // L105
                    cx.sg
                        .draw_image(None, x + knob_size / 2.0 + d, y + 1.0, 2.0, height - 2.0);
                });
            }
            // L108
            if dragging || m_over_comp == Some(SliderComp::Knob) {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            } else {
                cx.sg.set_draw_color(0.5, 0.5, 0.5);
            }
            // L113
            let knob_x = Self::get_knob_x_for_val(cx, me);
            // L114
            if div_count.is_some() {
                // L115
                let arrow_height = knob_size / 2.0;
                // L116
                draw_arrow(
                    cx,
                    x + 1.0 + knob_x + knob_size / 2.0,
                    y + height / 2.0 - arrow_height / 2.0,
                    knob_size,
                    arrow_height,
                    ArrowDir::Up,
                );
                // L117
                draw_arrow(
                    cx,
                    x + 1.0 + knob_x + knob_size / 2.0,
                    y + height / 2.0 + arrow_height / 2.0,
                    knob_size,
                    arrow_height,
                    ArrowDir::Down,
                );
            } else {
                // L119
                cx.sg.draw_image(
                    None,
                    x + 2.0 + knob_x,
                    y + 2.0,
                    knob_size - 2.0,
                    knob_size - 2.0,
                );
            }
        }
        // L122
        if enabled && (m_over || dragging) {
            // L123
            cx.sg.set_draw_layer(None, Some(100));
            // L124
            if dragging {
                // L125
                let val = cx.w::<Self>(me).val;
                TooltipHost::draw_tooltip::<Self>(
                    cx,
                    me,
                    |w| &mut w.tt,
                    x,
                    y,
                    width,
                    height,
                    vp,
                    val,
                );
            } else {
                // L127
                let (cursor_x, _cursor_y) = cx.sys.cursor_pos();
                // L128
                let knob_size = cx.w::<Self>(me).knob_size;
                let val = (cursor_x - x - 1.0 - knob_size / 2.0) / knob_travel;
                // L129
                TooltipHost::draw_tooltip::<Self>(
                    cx,
                    me,
                    |w| &mut w.tt,
                    x,
                    y,
                    width,
                    height,
                    vp,
                    m_max(0.0, m_min(1.0, val)),
                );
            }
            // L131
            cx.sg.set_draw_layer(None, Some(0));
        }
    }

    /// lua: SliderControl.lua:135-156
    /// @pob Classes/SliderControl.lua:135-156 h=a9b4a587
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L136
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L137
            return None;
        }
        // L139
        if key == Key::LeftButton {
            // L140
            let (m_over, m_over_comp) = Self::is_mouse_over_comp(cx, me);
            // L141
            if !m_over {
                // L142
                return None;
            }
            // L144
            if !cx.w::<Self>(me).dragging {
                // L145
                cx.w_mut::<Self>(me).dragging = true;
                // L146
                let (cursor_x, _cursor_y) = cx.sys.cursor_pos();
                // L147
                cx.w_mut::<Self>(me).drag_cx = Some(cursor_x);
                // L148
                if m_over_comp == Some(SliderComp::Slide) {
                    // L149
                    let (x, _y) = cx.pos(me);
                    // L150
                    let knob_size = cx.w::<Self>(me).knob_size;
                    Self::set_val_from_knob_x(cx, me, cursor_x - x - 1.0 - knob_size / 2.0);
                }
                // L152
                let kx = Self::get_knob_x_for_val(cx, me);
                cx.w_mut::<Self>(me).drag_knob_x = Some(kx);
            }
        }
        // L155
        Some(me)
    }

    /// lua: SliderControl.lua:158-185
    /// @pob Classes/SliderControl.lua:158-185 h=dec34417
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L159
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L160
            return None;
        }
        let invert = INVERT_SLIDER_SCROLL_DIRECTION.with(Cell::get); // CCR-0069
        let tbl = cx.w::<Self>(me).scroll_wheel_speed_tbl;
        // L162
        if key == Key::LeftButton {
            // L163
            if cx.w::<Self>(me).dragging {
                // L164
                cx.w_mut::<Self>(me).dragging = false;
                // L165
                let (cursor_x, _cursor_y) = cx.sys.cursor_pos();
                // L166
                let (drag_cx, drag_knob_x) = {
                    let w = cx.w::<Self>(me);
                    (num(w.drag_cx), num(w.drag_knob_x))
                };
                Self::set_val_from_knob_x(cx, me, (cursor_x - drag_cx) + drag_knob_x);
            }
        } else if (!invert && key == Key::WheelDown)
            || (invert && key == Key::WheelUp)
            || key == Key::Down
            || key == Key::Left
        {
            // L169
            let val = cx.w::<Self>(me).val;
            if cx.sys.is_key_down(Key::Shift) {
                // L170
                Self::set_val(cx, me, val - tbl.shift);
            } else if cx.sys.is_key_down(Key::Ctrl) {
                // L172
                Self::set_val(cx, me, val - tbl.ctrl);
            } else {
                // L174
                Self::set_val(cx, me, val - tbl.default);
            }
        } else if (!invert && key == Key::WheelUp)
            || (invert && key == Key::WheelDown)
            || key == Key::Up
            || key == Key::Right
        {
            // L177
            let val = cx.w::<Self>(me).val;
            if cx.sys.is_key_down(Key::Shift) {
                // L178
                Self::set_val(cx, me, val + tbl.shift);
            } else if cx.sys.is_key_down(Key::Ctrl) {
                // L180
                Self::set_val(cx, me, val + tbl.ctrl);
            } else {
                // L182
                Self::set_val(cx, me, val + tbl.default);
            }
        }
        None
    }
}
