//! Gallery "u9_button" (U9): the Rust side of `lua/gallery/u9_button.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::Cell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::{ButtonControl, LabelControl};
use pob_ui_core::prelude::*;

use super::_u9::{set_image_traced, tooltip_module_load, Gallery};

#[derive(Default)]
struct State {
    clicks: Cell<i64>,
    enters: Cell<i64>,
    hovers: Cell<i64>,
}

fn add(c: &Cell<i64>, n: i64) {
    c.set(c.get() + n);
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State::default());
    let s0 = s.clone();
    Gallery::new(
        parts,
        move |cx, host| {
            let s = s0;
            // the first ButtonControl loads Classes/Tooltip.lua
            tooltip_module_load(cx);
            let st = s.clone();
            let plain = cx.add(
                host,
                "plain",
                ButtonControl::new(Anchor::NONE, rect(20, 20, 120, 24), "Plain", move |_| {
                    add(&st.clicks, 1)
                }),
            );
            let st = s.clone();
            let plus = cx.add(
                host,
                "plus",
                ButtonControl::new(
                    Anchor::new(Left, plain, Right),
                    rect(8, 0, 24, 24),
                    "+",
                    move |_| add(&st.clicks, 10),
                ),
            );
            let st = s.clone();
            let minus = cx.add(
                host,
                "minus",
                ButtonControl::new(
                    Anchor::new(Left, plus, Right),
                    rect(8, 0, 24, 24),
                    "-",
                    move |_| add(&st.clicks, -1),
                ),
            );
            let st = s.clone();
            cx.add(
                host,
                "close",
                ButtonControl::new(
                    Anchor::new(Left, minus, Right),
                    rect(8, 0, 20, 20),
                    "x",
                    move |_| add(&st.clicks, 100),
                ),
            );
            let st = s.clone();
            cx.add(
                host,
                "disabled",
                ButtonControl::new(Anchor::NONE, rect(20, 60, 120, 24), "Disabled", move |_| {
                    add(&st.clicks, 1000)
                })
                .enabled(Prop::Val(false)),
            );
            let st = s.clone();
            let locked = cx.add(
                host,
                "locked",
                ButtonControl::new(Anchor::NONE, rect(160, 60, 120, 24), "Locked", move |_| {
                    add(&st.clicks, 1)
                }),
            );
            cx.get_mut(locked).locked = Prop::Val(true);
            let st = s.clone();
            let st2 = s.clone();
            cx.add(
                host,
                "lockedFn",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(300, 60, 120, 24),
                    "Locked odd",
                    move |_| add(&st.clicks, 1),
                )
                .mixin(move |cx, c| {
                    let st = st2.clone();
                    cx.get_mut(c).locked = Prop::f(move |_, _| st.clicks.get() % 2 == 1);
                }),
            );
            let st = s.clone();
            let image = cx.add(
                host,
                "image",
                ButtonControl::new(Anchor::NONE, rect(20, 100, 40, 40), "", move |_| {
                    add(&st.clicks, 1)
                }),
            );
            set_image_traced(cx, "Assets/icon_helmet.png", |cx| {
                ButtonControl::set_image(cx, image.id(), Some("Assets/icon_helmet.png"))
            });
            let image_off = cx.add(
                host,
                "imageOff",
                ButtonControl::new(Anchor::NONE, rect(70, 100, 40, 40), "", |_| {})
                    .enabled(Prop::Val(false)),
            );
            set_image_traced(cx, "Assets/icon_helmet.png", |cx| {
                ButtonControl::set_image(cx, image_off.id(), Some("Assets/icon_helmet.png"))
            });
            let st = s.clone();
            let over = cx.add(
                host,
                "over",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(130, 100, 120, 24),
                    "Oversize",
                    move |_| add(&st.clicks, 1),
                ),
            );
            cx.get_mut(over).over_size_text = 3.0;
            let st = s.clone();
            let st2 = s.clone();
            cx.add(
                host,
                "hover",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(270, 100, 120, 24),
                    "Hover me",
                    move |_| add(&st.clicks, 1),
                )
                .on_hover(move |_| add(&st2.hovers, 1)),
            );
            let st = s.clone();
            let st2 = s.clone();
            let enter = cx.add(
                host,
                "enter",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(410, 100, 120, 24),
                    Prop::f(move |_, _| format!("Enter {}", st2.enters.get())),
                    move |_| add(&st.clicks, 1),
                ),
            );
            let st = s.clone();
            cx.get_mut(enter).enter_func = Some(Rc::new(move |_| add(&st.enters, 1)));
            let st = s.clone();
            let st2 = s.clone();
            cx.add(
                host,
                "hidden",
                ButtonControl::new(Anchor::NONE, rect(550, 100, 120, 24), "Hidden", move |_| {
                    add(&st.clicks, 1)
                })
                .shown(Prop::f(move |_, _| st2.clicks.get() < 20)),
            );
            let st = s.clone();
            let status = cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 160, 0, 16),
                    Prop::f(move |_, _| {
                        format!(
                            "^7clicks {}  enters {}  hovers {}",
                            st.clicks.get(),
                            st.enters.get(),
                            st.hovers.get()
                        )
                    }),
                ),
            );
            cx.add(
                host,
                "statusRight",
                LabelControl::new(
                    Anchor::new(Left, status, Right),
                    rect(10, 0, 0, 14),
                    "^xFF8000after",
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
