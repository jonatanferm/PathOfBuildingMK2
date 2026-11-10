//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/DraggerControl.lua 1-147. Owner: U9.
//!
//! @pob-file Classes/DraggerControl.lua @16de4b82d
//!
//! Surface notes (owner's amendments to the UI-U0 stub, ui doc §13 item 12):
//! - The three callbacks receive Lua's `{ X = .., Y = .. }` table as `(X, Y)`; `onRightClick`
//!   gets the cursor position too (L273: `self.onRightClick({ X = cursorX, Y = cursorY })`;
//!   ResizableEditControl.lua:19 reads it).
//! - `locked` (`GetProperty("locked")`), `clicked`, `cursor_x`/`cursor_y` (`self.cursorX/Y`) and
//!   `over_size_text` (`self.overSizeText or 0`) are fields.
//! - `Draw` returns `self.onHover()` (L236); DrawControls ignores it.

use std::rc::Rc;

use pob_sg::{Align, Font, ImageFlags, ImageHandle, Key};

use crate::prelude::*;
use crate::tooltip::TooltipHost;

/// A DraggerControl callback: Lua passes `{ X = .., Y = .. }`.
pub type DragFn = Rc<dyn Fn(&mut Cx, (f64, f64))>;

pub struct DraggerControl {
    pub tt: TooltipHost<()>,
    pub label: Prop<String>,
    pub on_key_down: Option<DragFn>,
    pub on_key_up: Option<DragFn>,
    pub on_right_click: Option<DragFn>,
    pub on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
    pub force_tooltip: bool,
    /// GetProperty("locked"); nil => false
    pub locked: Prop<bool>,
    pub image: Option<ImageHandle>,
    /// `self.overSizeText or 0`
    pub over_size_text: f64,
    pub cursor_x: f64,
    pub cursor_y: f64,
    pub clicked: bool,
    pub dragging: bool,
}

impl DraggerControl {
    /// lua: DraggerControl.lua:9-21 DraggerControl(anchor, rect, label, onKeyDown, onKeyUp, onRightClick, onHover, forceTooltip)
    /// @pob Classes/DraggerControl.lua:9-21 h=3e395a0b
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        label: impl Into<Prop<String>>,
        on_key_down: Option<DragFn>,
        on_key_up: Option<DragFn>,
        on_right_click: Option<DragFn>,
        on_hover: Option<Rc<dyn Fn(&mut Cx)>>,
        force_tooltip: bool,
    ) -> New<DraggerControl> {
        // L10
        let base = Base::new(anchor, rect);
        New::new(
            base,
            DraggerControl {
                // L11
                tt: TooltipHost::new(None),
                // L12
                label: label.into(),
                // L13
                on_key_down,
                // L14
                on_key_up,
                // L15
                on_right_click,
                // L16
                on_hover,
                // L17
                force_tooltip,
                locked: Prop::Val(false),
                image: None,
                over_size_text: 0.0,
                // L18
                cursor_x: 0.0,
                // L19
                cursor_y: 0.0,
                clicked: false,
                dragging: false,
            },
        )
    }
    /// lua: DraggerControl.lua:23-30 SetImage
    /// @pob Classes/DraggerControl.lua:23-30 h=ba3837d0
    pub fn set_image(cx: &mut Cx, me: CtrlId, path: Option<&str>) {
        // L24
        let img = if let Some(path) = path {
            // L25
            let h = cx.sg.new_image_handle();
            // L26
            h.load(&mut cx.sg, path, ImageFlags::empty());
            Some(h)
        } else {
            // L28
            None
        };
        cx.w_mut::<Self>(me).image = img;
    }
}

impl Widget for DraggerControl {
    const NAME: &'static str = "DraggerControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    /// lua: DraggerControl.lua:32-37
    /// @pob Classes/DraggerControl.lua:32-37 h=a0380b32
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L33
        if !cx.is_shown(me) {
            // L34
            return false;
        }
        // L36
        cx.is_mouse_in_bounds(me)
    }

    /// lua: DraggerControl.lua:39-109
    /// @pob Classes/DraggerControl.lua:39-109 h=35558d0a
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        // L40
        let (x, y) = cx.pos(me);
        // L41
        let (width, height) = cx.size(me);
        // L42
        let enabled = cx.is_enabled(me);
        // L43
        let m_over = cx.is_mouse_over(me);
        // L44
        let locked = cx.prop::<Self, _>(me, |w| &w.locked);

        // L46
        if !enabled {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over || locked {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L53
        cx.sg.draw_image(None, x, y, width, height);
        let (clicked, image) = {
            let w = cx.w::<Self>(me);
            (w.clicked, w.image.clone())
        };
        // L54
        if !enabled {
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        } else if clicked && m_over {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        } else if m_over || locked {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L63
        cx.sg
            .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
        // L64
        if let Some(image) = image {
            // L65
            if enabled {
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
            } else {
                cx.sg.set_draw_color(0.33, 0.33, 0.33);
            }
            // L70
            cx.sg
                .draw_image(Some(&image), x + 2.0, y + 2.0, width - 4.0, height - 4.0);
            // L71
            if clicked && m_over {
                // L72
                cx.sg.set_draw_color_a(1.0, 1.0, 1.0, 0.5);
                // L73
                cx.sg
                    .draw_image(None, x + 1.0, y + 1.0, width - 2.0, height - 2.0);
            }
        }
        // L76
        let dragging = cx.w::<Self>(me).dragging;
        if !enabled {
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else if m_over || locked || dragging {
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L83
        let label = cx.prop::<Self, _>(me, |w| &w.label);
        // L84
        if label == "+" {
            // L85
            cx.sg.draw_image(
                None,
                x + width * 0.2,
                y + height * 0.45,
                width * 0.6,
                height * 0.1,
            );
            // L86
            cx.sg.draw_image(
                None,
                x + width * 0.45,
                y + height * 0.2,
                width * 0.1,
                height * 0.6,
            );
        } else if label == "-" {
            // L88
            cx.sg.draw_image(
                None,
                x + width * 0.2,
                y + height * 0.45,
                width * 0.6,
                height * 0.1,
            );
        } else if label == "x" {
            // L90
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
            // L91
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
        } else if label == "//" {
            // L93
            cx.sg.draw_image_quad(
                None,
                [
                    x + width * 0.75,
                    y + height * 0.15,
                    x + width * 0.85,
                    y + height * 0.25,
                    x + width * 0.25,
                    y + height * 0.85,
                    x + width * 0.15,
                    y + height * 0.75,
                ],
            );
            // L94
            cx.sg.draw_image_quad(
                None,
                [
                    x + width * 0.75,
                    y + height * 0.5,
                    x + width * 0.85,
                    y + height * 0.6,
                    x + width * 0.6,
                    y + height * 0.85,
                    x + width * 0.5,
                    y + height * 0.75,
                ],
            );
        } else {
            // L96
            let over_size = cx.w::<Self>(me).over_size_text;
            // L97
            cx.sg.draw_string(
                x + width / 2.0,
                y + 2.0 - over_size,
                Align::CenterX,
                height - 4.0 + over_size * 2.0,
                Font::Var,
                &label,
            );
        }
        // L99
        if m_over {
            // L100
            if !no_tooltip || cx.w::<Self>(me).force_tooltip {
                // L101
                cx.sg.set_draw_layer(None, Some(100));
                // L102
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
                // L103
                cx.sg.set_draw_layer(None, Some(0));
            }
            // L105
            if let Some(h) = cx.w::<Self>(me).on_hover.clone() {
                // L106 (the return value is ignored by DrawControls)
                h(cx);
            }
        }
    }

    /// lua: DraggerControl.lua:111-128
    /// @pob Classes/DraggerControl.lua:111-128 h=4e1aef3b
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L112
        if !cx.is_shown(me) || !cx.is_enabled(me) || cx.prop::<Self, _>(me, |w| &w.locked) {
            // L113
            return None;
        }
        // L115
        if key == Key::LeftButton {
            // L116
            let (cursor_x, cursor_y) = cx.sys.cursor_pos();

            // L118
            cx.w_mut::<Self>(me).clicked = true;
            // L119
            cx.w_mut::<Self>(me).dragging = true;
            // L120
            if let Some(f) = cx.w::<Self>(me).on_key_down.clone() {
                // L121
                f(cx, (cursor_x, cursor_y));
            }

            // L124
            cx.w_mut::<Self>(me).cursor_x = cursor_x;
            // L125
            cx.w_mut::<Self>(me).cursor_y = cursor_y;
        }
        // L127
        Some(me)
    }

    /// lua: DraggerControl.lua:129-147
    /// @pob Classes/DraggerControl.lua:129-147 h=2906e903
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L130
        if !cx.is_shown(me) || !cx.is_enabled(me) || cx.prop::<Self, _>(me, |w| &w.locked) {
            // L131
            return None;
        }
        // L133
        let (cursor_x, cursor_y) = cx.sys.cursor_pos();
        // L134
        if key == Key::LeftButton {
            // L135
            cx.w_mut::<Self>(me).clicked = false;
            // L136
            cx.w_mut::<Self>(me).dragging = false;
            // L137
            if let Some(f) = cx.w::<Self>(me).on_key_up.clone() {
                // L138
                let (sx, sy) = {
                    let w = cx.w::<Self>(me);
                    (w.cursor_x, w.cursor_y)
                };
                f(cx, (sx - cursor_x, sy - cursor_y));
            }
        }
        // L141
        if key == Key::RightButton && cx.is_mouse_in_bounds(me) {
            // L142
            if let Some(f) = cx.w::<Self>(me).on_right_click.clone() {
                // L143
                f(cx, (cursor_x, cursor_y));
            }
        }
        // L146
        Some(me)
    }
}
