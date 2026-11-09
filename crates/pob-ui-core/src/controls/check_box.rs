//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CheckBoxControl.lua 1-117. Owner: U9.
//!
//! @pob-file Classes/CheckBoxControl.lua @16de4b82d
//!
//! Surface notes (owner's amendments to the UI-U0 stub, ui doc §13 item 12):
//! - `label` is `Prop<Option<String>>`: PoB passes `nil` (ItemsTab.lua:480, CalcsTab.lua:94, …),
//!   and `IsMouseOver`/`Draw` test `if label then` (an empty string is truthy).
//! - `label_width` (`self.labelWidth`, read by CompareTab.lua:1793), `label_right`
//!   (`self.labelRight`) and `border_func` (`self.borderFunc`, ConfigTab.lua:697) are fields.
//! - The constructor computes `labelWidth` with `DrawStringWidth`, which needs the font set, so it
//!   runs as the constructor tail (`New::init`): after insertion, before any mixin, as in Lua
//!   (`new(..):CheckBoxControl(..)` returns before the mixin table is applied).
//! - `rect[4] = rect[3] or 0` (L10) writes the caller's rect table; the Rust rect is moved in, so
//!   only the control's own copy (`rectStart`) changes, which is all Lua observes afterwards
//!   (no PoB caller reuses the rect table).
//! - `state` is `bool`: Lua stores `initialState` (nil or a boolean) and `not self.state`; every
//!   reader tests truthiness, and the tooltipFunc argument is frozen as `bool` (ui doc §6.9). A
//!   caller assigning `nil` (ConfigTab.lua:1016) writes `false`.
#![allow(clippy::many_single_char_names)] // Lua names (x, y, w, h, r, g, b)

use std::rc::Rc;

use pob_sg::{Align, Font, Key};

use crate::draw::draw_check_mark;
use crate::prelude::*;
use crate::tooltip::TooltipHost;

pub struct CheckBoxControl {
    pub tt: TooltipHost<bool>,
    pub label: Prop<Option<String>>,
    /// `self.labelWidth` (set by the constructor tail)
    pub label_width: f64,
    /// `self.labelRight`
    pub label_right: bool,
    pub change_func: Option<Rc<dyn Fn(&mut Cx, bool)>>,
    /// `self.borderFunc` (returns r, g, b)
    pub border_func: Option<Rc<dyn Fn(&Cx) -> (f64, f64, f64)>>,
    pub state: bool,
    pub clicked: bool,
}

impl CheckBoxControl {
    /// lua: CheckBoxControl.lua:9-19 CheckBoxControl(anchor, rect, label, changeFunc, tooltipText, initialState)
    /// @pob Classes/CheckBoxControl.lua:9-19 h=7a934e19
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<Option<String>>>,
        change_func: Option<Rc<dyn Fn(&mut Cx, bool)>>,
        tooltip_text: Option<Prop<Option<String>>>,
        initial_state: bool,
    ) -> New<CheckBoxControl> {
        let mut rect = rect;
        // L10 rect[4] = rect[3] or 0
        rect[3] = rect[2].clone();
        // L11
        let base = Base::new(anchor, rect);
        let label = label.into();
        New::new(
            base,
            CheckBoxControl {
                // L12
                tt: TooltipHost::new(tooltip_text),
                // L13
                label,
                label_width: 0.0,
                // L15
                label_right: false,
                // L16
                change_func,
                border_func: None,
                // L17
                state: initial_state,
                clicked: false,
            },
        )
        .init(|cx, me| {
            // L14 self.labelWidth = DrawStringWidth(self.width - 4, "VAR", label or "") + 5
            let width = cx.base(me).width.raw();
            let label = match &cx.get(me).label {
                Prop::Val(v) => v.clone().unwrap_or_default(),
                Prop::Fn(_) => {
                    crate::lua_error!("DrawStringWidth() argument 3: expected string, got function")
                }
            };
            let lw = cx.sg.draw_string_width(width - 4.0, Font::Var, &label) + 5.0;
            cx.get_mut(me).label_width = lw;
        })
    }
}

impl Widget for CheckBoxControl {
    const NAME: &'static str = "CheckBoxControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: CheckBoxControl.lua:21-38
    /// @pob Classes/CheckBoxControl.lua:21-38 h=9a9534fd
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L22
        if !cx.is_shown(me) {
            // L23
            return false;
        }
        // L25
        let (mut x, y) = cx.pos(me);
        // L26
        let (mut width, height) = cx.size(me);
        // L27
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();

        // Include the label in the clickable area.
        // L30
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        // L31
        if label.is_some() {
            let w = cx.w::<Self>(me);
            // L32
            if !w.label_right {
                // L33
                x -= w.label_width;
            }
            // L35
            width += w.label_width;
        }
        // L37
        cursor_x >= x && cursor_y >= y && cursor_x < x + width && cursor_y < y + height
    }

    /// lua: CheckBoxControl.lua:40-92
    /// @pob Classes/CheckBoxControl.lua:40-92 h=5f23aa91
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L41
        let (x, y) = cx.pos(me);
        // L42 (raw `self.width`)
        let size = cx.base(me).width.raw();
        // L43
        let enabled = cx.is_enabled(me);
        // L44
        let m_over = cx.is_mouse_over(me);
        // L45
        if !enabled {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else if let Some(f) = cx.w::<Self>(me).border_func.clone() {
            // L50
            let (r, g, b) = f(cx);
            // L51
            cx.sg.set_draw_color(r, g, b);
        } else {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L55
        cx.sg.draw_image(None, x, y, size, size);
        let clicked = cx.w::<Self>(me).clicked;
        // L56
        if !enabled {
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        } else if clicked && m_over {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        } else if m_over {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L65
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, size - 2.0, size - 2.0);
        // L66
        if cx.w::<Self>(me).state {
            // L67
            if !enabled {
                cx.sg.set_draw_color(0.33, 0.33, 0.33);
            } else if m_over {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            } else {
                cx.sg.set_draw_color(0.75, 0.75, 0.75);
            }
            // L74
            draw_check_mark(cx, x + size / 2.0, y + size / 2.0, size * 0.8);
        }
        // L76
        if enabled {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        }
        // L81
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        let label_right = cx.w::<Self>(me).label_right;
        // L82
        match label {
            Some(label) if label_right => {
                // L83 (alignment nil => "LEFT"; raw `self.width`)
                let width = cx.base(me).width.raw();
                cx.sg.draw_string(
                    x + width + 5.0,
                    y + 2.0,
                    Align::Left,
                    size - 4.0,
                    Font::Var,
                    &label,
                );
            }
            Some(label) => {
                // L85
                cx.sg.draw_string(
                    x - 5.0,
                    y + 2.0,
                    Align::RightX,
                    size - 4.0,
                    Font::Var,
                    &label,
                );
            }
            None => {}
        }
        // L87
        if m_over && !no_tooltip {
            // L88
            cx.sg.set_draw_layer(None, Some(100));
            // L89
            let state = cx.w::<Self>(me).state;
            TooltipHost::draw_tooltip::<Self>(cx, me, |w| &mut w.tt, x, y, size, size, vp, state);
            // L90
            cx.sg.set_draw_layer(None, Some(0));
        }
    }

    /// lua: CheckBoxControl.lua:94-102
    /// @pob Classes/CheckBoxControl.lua:94-102 h=3d5f6949
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L95
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L96
            return None;
        }
        // L98
        if key == Key::LeftButton {
            // L99
            cx.w_mut::<Self>(me).clicked = true;
        }
        // L101
        Some(me)
    }

    /// lua: CheckBoxControl.lua:104-117
    /// @pob Classes/CheckBoxControl.lua:104-117 h=a9380fdc
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L105
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L106
            return None;
        }
        // L108
        if key == Key::LeftButton {
            // L109
            if cx.is_mouse_over(me) {
                // L110
                let state = !cx.w::<Self>(me).state;
                cx.w_mut::<Self>(me).state = state;
                // L111
                if let Some(f) = cx.w::<Self>(me).change_func.clone() {
                    // L112
                    let state = cx.w::<Self>(me).state;
                    f(cx, state);
                }
            }
        }
        // L116
        cx.w_mut::<Self>(me).clicked = false;
        None
    }
}
