//! Gallery "u10_hover" (U10): the Rust side of `lua/gallery/u10_hover.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::Cell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_sg::Font;
use pob_ui_core::controls::slider::SliderControl;
use pob_ui_core::controls::{ButtonControl, LabelControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{module_load, TtModLine};
use pob_ui_core::tooltip::{ModLineRef, Tooltip, TtColor, TtKey};

use super::_u9::Gallery;

#[derive(Default)]
struct State {
    memo_key: Cell<i64>,
    rebuilds: Cell<i64>,
}

fn button(
    cx: &mut Cx,
    host: CtrlId,
    key: &'static str,
    r: [Prop<f64>; 4],
    label: &'static str,
    on_click: impl Fn(&mut Cx) + 'static,
) -> Ctrl<ButtonControl> {
    cx.add(
        host,
        key,
        ButtonControl::new(Anchor::NONE, r, label, on_click),
    )
}

fn set_func(cx: &mut Cx, b: Ctrl<ButtonControl>, f: impl Fn(&mut Cx, &mut Tooltip) + 'static) {
    cx.get_mut(b).tt.tooltip_func = Some(Rc::new(move |cx, tt, ()| f(cx, tt)));
}

fn mod_line(disabled: bool) -> Option<ModLineRef> {
    Some(Rc::new(TtModLine {
        disabled: Rc::new(move || disabled),
        line: Rc::new(()),
    }))
}

fn edge_lines(cx: &mut Cx, tt: &mut Tooltip, n: i64) {
    tt.clear(false);
    for i in 1..=n {
        tt.add_line(cx, 16.0, &format!("^7Edge tooltip line {i}"));
    }
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State::default());
    s.memo_key.set(1);
    Gallery::new(
        parts,
        move |cx, host| {
            // the first ButtonControl loads Classes/Tooltip.lua
            module_load(cx);
            let plain = button(cx, host, "plain", rect(20, 20, 100, 20), "Plain", |_| {});
            cx.get_mut(plain).tt.tooltip_text =
                Some(Prop::Val(Some("^7A plain tooltip line".to_owned())));
            let multi = button(cx, host, "multi", rect(130, 20, 100, 20), "Multi", |_| {});
            cx.get_mut(multi).tt.tooltip_text = Some(Prop::f(|_, _| {
                Some(
                    "^7First line\n^1Second line, red\nthird line keeps the red\n\n^xFFFF00after an empty line"
                        .to_owned(),
                )
            }));
            let st = s.clone();
            let memo = button(cx, host, "memo", rect(240, 20, 100, 20), "Memo", move |_| {
                st.memo_key.set(st.memo_key.get() + 1)
            });
            let st = s.clone();
            set_func(cx, memo, move |cx, tt| {
                let key = st.memo_key.get();
                if tt.check_for_update(
                    cx,
                    &[
                        TtKey::Num(key as f64),
                        TtKey::Str("const".into()),
                        TtKey::Nil,
                        TtKey::Bool(true),
                    ],
                ) {
                    st.rebuilds.set(st.rebuilds.get() + 1);
                    tt.add_line(cx, 16.0, &format!("^7Rebuilt {} times", st.rebuilds.get()));
                    tt.add_separator(cx, Some(10.0));
                    tt.add_line(cx, 14.0, &format!("^7key {key}"));
                }
            });
            button(cx, host, "nsmt", rect(350, 20, 100, 20), "NSMT", |cx| {
                cx.main.not_supported_mod_tooltips = !cx.main.not_supported_mod_tooltips
            });
            let seps = button(cx, host, "seps", rect(460, 20, 100, 20), "Seps", |_| {});
            set_func(cx, seps, |cx, tt| {
                tt.clear(false);
                tt.color = Some(TtColor::Esc("^x1AA29B".to_owned()));
                tt.add_separator(cx, None);
                tt.add_line(cx, 16.0, "^7Left aligned");
                tt.add_separator(cx, Some(14.0));
                tt.add_separator(cx, Some(10.0));
                tt.center = true;
                tt.add_line(cx, 16.0, "^7Centred line");
                tt.add_line(cx, 12.0, "^8a shorter centred line");
                tt.add_separator(cx, Some(6.0));
                tt.center = false;
                tt.add_line(cx, 14.0, "^7Left again");
                tt.add_separator(cx, None);
            });
            let wrap = button(cx, host, "wrap", rect(570, 20, 100, 20), "Wrap", |_| {});
            set_func(cx, wrap, |cx, tt| {
                tt.clear(false);
                tt.max_width = Some(220.0);
                tt.add_line(
                    cx,
                    16.0,
                    "^7This rather long line is wrapped by main:WrapString at the tooltip's maximum width minus the padding.",
                );
                tt.add_line(cx, 14.0, "short");
                tt.add_line(
                    cx,
                    14.0,
                    "^xAAAAFFAnotherveryveryverylongwordwithoutanyspaces then words",
                );
            });
            let strike = button(cx, host, "strike", rect(680, 20, 100, 20), "Strike", |_| {});
            set_func(cx, strike, |cx, tt| {
                tt.clear(false);
                tt.add_line_ex(
                    cx,
                    16.0,
                    "^7Enabled mod line",
                    Some(Font::FontinSc),
                    mod_line(false),
                    None,
                );
                tt.add_line_ex(
                    cx,
                    16.0,
                    "^8Disabled mod line",
                    Some(Font::FontinSc),
                    mod_line(true),
                    None,
                );
                tt.center = true;
                tt.add_line_ex(
                    cx,
                    16.0,
                    "^8Disabled, centred",
                    Some(Font::Fontin),
                    mod_line(true),
                    None,
                );
                tt.add_line(cx, 14.0, "^7no mod line");
            });
            // placement at the four edges
            let right = button(
                cx,
                host,
                "right",
                rect(Prop::f(|cx, _| cx.main.screen_w - 70.0), 60, 60, 20),
                "Right",
                |_| {},
            );
            set_func(cx, right, |cx, tt| edge_lines(cx, tt, 3));
            let wide = button(cx, host, "wide", rect(300, 60, 60, 20), "Wide", |_| {});
            set_func(cx, wide, |cx, tt| {
                tt.clear(false);
                tt.add_line(
                    cx,
                    20.0,
                    &format!("^7{}", "A very wide tooltip line. ".repeat(6)),
                );
                tt.add_line(
                    cx,
                    16.0,
                    "^7It flips left, clamps at the viewport and moves below the button.",
                );
            });
            let bottom = button(
                cx,
                host,
                "bottom",
                rect(20, Prop::f(|cx, _| cx.main.screen_h - 30.0), 60, 20),
                "Bottom",
                |_| {},
            );
            set_func(cx, bottom, |cx, tt| edge_lines(cx, tt, 6));
            let corner = button(
                cx,
                host,
                "corner",
                rect(
                    Prop::f(|cx, _| cx.main.screen_w - 70.0),
                    Prop::f(|cx, _| cx.main.screen_h - 30.0),
                    60,
                    20,
                ),
                "Corner",
                |_| {},
            );
            set_func(cx, corner, |cx, tt| edge_lines(cx, tt, 6));
            let tall = button(
                cx,
                host,
                "tall",
                rect(500, Prop::f(|cx, _| cx.main.screen_h - 60.0), 60, 20),
                "Tall",
                |_| {},
            );
            set_func(cx, tall, |cx, tt| edge_lines(cx, tt, 60));
            // tooltip.Draw override (TreeTab.lua:1741-1755)
            let slider = cx.add(
                host,
                "slider",
                SliderControl::new(
                    Anchor::NONE,
                    rect(20, 120, 200, 18),
                    Some(Rc::new(|_, _| {})),
                    None,
                ),
            );
            let sid = slider.id();
            cx.get_mut(slider).tt.tooltip_func = Some(Rc::new(move |cx, tt, _val| {
                tt.clear(false);
                if !cx.w::<SliderControl>(sid).dragging {
                    tt.add_line(cx, 16.0, "^7This tooltip is drawn by an overridden Draw");
                }
            }));
            cx.get_mut(slider).tt.tooltip.draw_override =
                Some(Rc::new(move |cx, tt, x, y, width, height, vp| {
                    let slider_offset_x =
                        pob_lua::round(184.0 * (1.0 - cx.w::<SliderControl>(sid).val), None);
                    let (_tooltip_width, tooltip_height) = tt.get_size(cx);
                    if cx.main.screen_w >= 1338.0 - slider_offset_x {
                        return tt.real_draw(
                            cx,
                            x - 8.0 - slider_offset_x,
                            y - 4.0 - tooltip_height,
                            width,
                            height,
                            vp,
                        );
                    }
                    tt.real_draw(cx, x, y, width, height, vp)
                }));
            let st = s.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 160, 0, 16),
                    Prop::f(move |cx, _| {
                        format!(
                            "^7rebuilds {}  key {}  nsmt {}",
                            st.rebuilds.get(),
                            st.memo_key.get(),
                            cx.main.not_supported_mod_tooltips
                        )
                    }),
                ),
            );
        },
        |cx, host, ev, vp| {
            cx.process_controls_input(host, ev, vp);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            cx.draw_controls(host, vp, None);
        },
    )
}
