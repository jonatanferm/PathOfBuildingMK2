//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ButtonControl.lua 1-130. Owner: U9.
//! The ui doc §9.1 reference port.
//!
//! @pob-file Classes/ButtonControl.lua @16de4b82d
//!
//! `Draw` returns `self.onHover()` (L102); DrawControls ignores a Draw's return value, so the
//! port calls `on_hover` and drops nothing observable.

use std::rc::Rc;

use pob_sg::{Align, Font, ImageFlags, ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;

pub type ClickFn = Rc<dyn Fn(&mut Cx) -> Focus>;

pub struct ButtonControl {
    pub tt: TooltipHost<()>,
    pub label: Prop<String>,
    pub on_click: ClickFn,
    pub on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
    pub force_tooltip: bool,
    /// GetProperty("locked"); nil => false
    pub locked: Prop<bool>,
    pub enter_func: Option<Rc<dyn Fn(&mut Cx)>>,
    pub image: Option<ImageHandle>,
    /// `self.overSizeText or 0`
    pub over_size_text: f64,
    pub clicked: bool,
}

impl ButtonControl {
    /// lua: ButtonControl.lua:9-17 ButtonControl(anchor, rect, label, onClick, onHover, forceTooltip)
    /// @pob Classes/ButtonControl.lua:9-17 h=a10e4f4e
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        on_click: impl Fn(&mut Cx) + 'static,
    ) -> New<ButtonControl> {
        Self::new_ret(anchor, rect, label, move |cx| {
            on_click(cx);
            None
        })
    }
    /// For onClick functions whose return value Lua feeds into SelectControl (`return self.onClick()` in OnKeyUp).
    pub fn new_ret(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        on_click: impl Fn(&mut Cx) -> Focus + 'static,
    ) -> New<ButtonControl> {
        // L10
        let base = Base::new(anchor, rect);
        New::new(
            base,
            ButtonControl {
                // L11
                tt: TooltipHost::new(None),
                // L12
                label: label.into(),
                // L13
                on_click: Rc::new(on_click),
                // L14
                on_hover: None,
                // L15
                force_tooltip: false,
                locked: Prop::Val(false),
                enter_func: None,
                image: None,
                over_size_text: 0.0,
                clicked: false,
            },
        )
    }
    /// lua: ButtonControl.lua:25-32
    /// @pob Classes/ButtonControl.lua:25-32 h=0bd08396
    pub fn set_image(cx: &mut Cx, me: CtrlId, path: Option<&str>) {
        // L26
        let img = if let Some(path) = path {
            // L27
            let h = cx.sg.new_image_handle();
            // L28
            h.load(&mut cx.sg, path, ImageFlags::empty());
            Some(h)
        } else {
            // L30
            None
        };
        cx.w_mut::<Self>(me).image = img;
    }
}

impl New<ButtonControl> {
    /// 5th and 6th constructor arguments.
    pub fn on_hover(mut self, f: impl Fn(&mut Cx) + 'static) -> Self {
        self.w.on_hover = Some(Rc::new(f));
        self
    }
    pub fn force_tooltip(mut self, v: bool) -> Self {
        self.w.force_tooltip = v;
        self
    }
}

impl Widget for ButtonControl {
    const NAME: &'static str = "ButtonControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP)
        .union(Caps::CLICK);

    /// lua: ButtonControl.lua:19-23
    /// @pob Classes/ButtonControl.lua:19-23 h=75b7302c
    fn click(cx: &mut Cx, me: CtrlId) {
        // L20
        if cx.is_shown(me) && cx.is_enabled(me) {
            // L21
            let f = cx.w::<Self>(me).on_click.clone();
            f(cx);
        }
    }

    /// lua: ButtonControl.lua:34-39
    /// @pob Classes/ButtonControl.lua:34-39 h=da2eab36
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L35
        if !cx.is_shown(me) {
            // L36
            return false;
        }
        // L38
        cx.is_mouse_in_bounds(me)
    }

    /// lua: ButtonControl.lua:41-105
    /// @pob Classes/ButtonControl.lua:41-105 h=b1686f8d
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L42
        let (x, y) = cx.pos(me);
        // L43
        let (width, height) = cx.size(me);
        // L44
        let enabled = cx.is_enabled(me);
        // L45
        let m_over = cx.is_mouse_over(me);
        // L46
        let locked = cx.prop::<Self, _>(me, |w| &w.locked);
        // L47
        if !enabled {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over || locked {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L54
        cx.sg.draw_image(None, x, y, width, height);
        let (clicked, image) = {
            let w = cx.w::<Self>(me);
            (w.clicked, w.image.clone())
        };
        // L55
        if !enabled {
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        } else if clicked && m_over {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        } else if m_over || locked {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L64
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L65
        if let Some(image) = image {
            // L66
            if enabled {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            } else {
                cx.sg.set_draw_color(0.33, 0.33, 0.33);
            }
            // L71
            cx.sg
                .draw_image(Some(&image), x + 2.0, y + 2.0, width - 4.0, height - 4.0);
            // L72
            if clicked && m_over {
                // L73
                cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.5);
                // L74
                cx.sg
                    .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
            }
        }
        // L77
        if enabled {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        }
        // L82
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        // L83
        if label == "+" {
            // L84
            cx.sg.draw_image(
                None,
                x + width * 0.2,
                y + height * 0.45,
                width * 0.6,
                height * 0.1,
            );
            // L85
            cx.sg.draw_image(
                None,
                x + width * 0.45,
                y + height * 0.2,
                width * 0.1,
                height * 0.6,
            );
        } else if label == "-" {
            // L87
            cx.sg.draw_image(
                None,
                x + width * 0.2,
                y + height * 0.45,
                width * 0.6,
                height * 0.1,
            );
        } else if label == "x" {
            // L89
            cx.sg.draw_image_quad(
                None,
                [
                    x + width * 0.2,
                    y + height * 0.3,
                    x + width * 0.3,
                    y + height * 0.2,
                    x + width * 0.8,
                    y + height * 0.7,
                    x + width * 0.7,
                    y + height * 0.8,
                ],
            );
            // L90
            cx.sg.draw_image_quad(
                None,
                [
                    x + width * 0.7,
                    y + height * 0.2,
                    x + width * 0.8,
                    y + height * 0.3,
                    x + width * 0.3,
                    y + height * 0.8,
                    x + width * 0.2,
                    y + height * 0.7,
                ],
            );
        } else {
            // L92
            let over_size = cx.w::<Self>(me).over_size_text;
            // L93
            cx.sg.draw_string(
                x + width / 2.0,
                y + 2.0 - over_size,
                Align::CenterX,
                height - 4.0 + over_size * 2.0,
                Font::Var,
                &label,
            );
        }
        // L95
        if m_over {
            // L96
            if !no_tooltip || cx.w::<Self>(me).force_tooltip {
                // L97
                cx.sg.set_draw_layer(None, Some(100));
                // L98
                TooltipHost::draw_tooltip::<Self>(
                    cx,
                    me,
                    |w| &mut w.tt,
                    x,
                    y,
                    width,
                    height,
                    vp,
                    (),
                );
                // L99
                cx.sg.set_draw_layer(None, Some(0));
            }
            // L101
            if let Some(h) = cx.w::<Self>(me).on_hover.clone() {
                // L102 (the return value is ignored by DrawControls)
                h(cx);
            }
        }
    }

    /// lua: ButtonControl.lua:107-117
    /// @pob Classes/ButtonControl.lua:107-117 h=0d514af9
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L108
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L109
            return None;
        }
        // L111
        if key == Key::LeftButton {
            // L112
            cx.w_mut::<Self>(me).clicked = true;
        } else if let Some(f) = cx.w::<Self>(me).enter_func.clone() {
            // L114
            f(cx);
        }
        // L116
        Some(me)
    }

    /// lua: ButtonControl.lua:119-130
    /// @pob Classes/ButtonControl.lua:119-130 h=3cfae84d
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L120
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L121
            return None;
        }
        // L123
        if key == Key::LeftButton && cx.w::<Self>(me).clicked {
            // L124
            cx.w_mut::<Self>(me).clicked = false;
            // L125
            if cx.is_mouse_over(me) {
                // L126
                let f = cx.w::<Self>(me).on_click.clone();
                return f(cx);
            }
        }
        // L129
        cx.w_mut::<Self>(me).clicked = false;
        None
    }
}
