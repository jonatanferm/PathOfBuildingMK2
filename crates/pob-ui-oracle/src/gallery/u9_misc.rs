//! Gallery "u9_misc" (U9): the Rust side of `lua/gallery/u9_misc.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_sg::{Align, Font};
use pob_ui_core::controls::{
    DraggerControl, LabelControl, RectangleOutlineControl, SectionControl,
};
use pob_ui_core::draw::{
    draw_arrow, draw_check_mark, render_circle, render_ring, world_to_screen, ArrowDir,
};
use pob_ui_core::prelude::*;

use super::_u9::{set_image_traced, tooltip_module_load, Gallery};

struct State {
    downs: Cell<i64>,
    last_down: RefCell<String>,
    last_up: RefCell<String>,
    rights: Cell<i64>,
    hovers: Cell<i64>,
}

fn dragger(label: &'static str, x: i32, y: i32, w: i32, h: i32) -> New<DraggerControl> {
    DraggerControl::new(
        Anchor::NONE,
        rect(x, y, w, h),
        label,
        None,
        None,
        None,
        None,
        false,
    )
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State {
        downs: Cell::new(0),
        last_down: RefCell::new("-".to_owned()),
        last_up: RefCell::new("-".to_owned()),
        rights: Cell::new(0),
        hovers: Cell::new(0),
    });
    Gallery::new(
        parts,
        move |cx, host| {
            let section = cx.add(
                host,
                "section",
                SectionControl::new(Anchor::NONE, rect(20, 30, 300, 120), "Section label"),
            );
            let st = s.clone();
            cx.add(
                host,
                "sectionFn",
                SectionControl::new(
                    Anchor::new(TopLeft, section, BottomLeft),
                    rect(0, 20, 300, 60),
                    Prop::f(move |_, _| format!("Downs {}", st.downs.get())),
                ),
            );
            let label = cx.add(
                host,
                "label",
                LabelControl::new(
                    Anchor::new(TopLeft, section, TopLeft),
                    rect(10, 14, 0, 16),
                    "^7Label in section",
                ),
            );
            let label_big = cx.add(
                host,
                "labelBig",
                LabelControl::new(
                    Anchor::new(TopLeft, label, BottomLeft),
                    rect(0, 4, 0, 20),
                    "^xFFD700Bigger",
                ),
            );
            cx.add(
                host,
                "labelNext",
                LabelControl::new(
                    Anchor::new(Left, label_big, Right),
                    rect(6, 0, 0, 12),
                    "^8next to it",
                ),
            );
            cx.add(
                host,
                "outline",
                RectangleOutlineControl::new(Anchor::NONE, rect(360, 30, 100, 60), None, None),
            );
            cx.add(
                host,
                "outline2",
                RectangleOutlineControl::new(
                    Anchor::NONE,
                    rect(480, 30, 100, 60),
                    Some([1.0, 0.5, 0.0]),
                    Some(3.0),
                ),
            );
            // the first DraggerControl loads Classes/Tooltip.lua
            tooltip_module_load(cx);
            let (s1, s2, s3, s4, s5) = (s.clone(), s.clone(), s.clone(), s.clone(), s.clone());
            cx.add(
                host,
                "dragger",
                DraggerControl::new(
                    Anchor::NONE,
                    rect(360, 120, 14, 14),
                    "//",
                    Some(Rc::new(move |_, (x, y)| {
                        s1.downs.set(s1.downs.get() + 1);
                        *s1.last_down.borrow_mut() =
                            format!("{},{}", tostring_num(x), tostring_num(y));
                    })),
                    Some(Rc::new(move |_, (x, y)| {
                        *s2.last_up.borrow_mut() =
                            format!("{},{}", tostring_num(x), tostring_num(y));
                    })),
                    Some(Rc::new(move |_, _| s3.rights.set(s3.rights.get() + 1))),
                    Some(Rc::new(move |_| s4.hovers.set(s4.hovers.get() + 1))),
                    false,
                )
                .shown(Prop::f(move |_, _| s5.rights.get() == 0)),
            );
            cx.add(host, "draggerPlus", dragger("+", 400, 120, 20, 20));
            cx.add(host, "draggerMinus", dragger("-", 430, 120, 20, 20));
            cx.add(host, "draggerX", dragger("x", 460, 120, 20, 20));
            let text = cx.add(host, "draggerText", dragger("Drag", 490, 120, 60, 20));
            cx.get_mut(text).over_size_text = 2.0;
            let locked = cx.add(host, "draggerLocked", dragger("Locked", 560, 120, 60, 20));
            cx.get_mut(locked).locked = Prop::Val(true);
            cx.add(
                host,
                "draggerOff",
                dragger("Off", 630, 120, 60, 20).enabled(Prop::Val(false)),
            );
            let img = cx.add(host, "draggerImg", dragger("", 700, 110, 40, 40));
            set_image_traced(cx, "Assets/icon_helmet.png", |cx| {
                DraggerControl::set_image(cx, img.id(), Some("Assets/icon_helmet.png"))
            });
            let st = s.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 260, 0, 16),
                    Prop::f(move |_, _| {
                        format!(
                            "downs {}  down {}  up {}  rights {}  hovers {}",
                            st.downs.get(),
                            st.last_down.borrow(),
                            st.last_up.borrow(),
                            st.rights.get(),
                            st.hovers.get()
                        )
                    }),
                ),
            );
        },
        |cx, host, ev, vp| {
            cx.process_controls_input(host, ev, vp);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            draw_arrow(cx, 40.0, 320.0, 20.0, 20.0, ArrowDir::Up);
            draw_arrow(cx, 70.0, 320.0, 20.0, 20.0, ArrowDir::Right);
            draw_arrow(cx, 100.0, 320.0, 20.0, 14.0, ArrowDir::Down);
            draw_arrow(cx, 130.0, 320.0, 14.0, 20.0, ArrowDir::Left);
            if let Some(d) = ArrowDir::from_lua("NONE") {
                draw_arrow(cx, 160.0, 320.0, 20.0, 20.0, d);
            }
            cx.sg.set_draw_color(0.0, 1.0, 0.0);
            draw_check_mark(cx, 200.0, 320.0, 16.0);
            draw_check_mark(cx, 230.0, 320.0, 24.0 * 0.8);
            cx.sg.set_draw_color(0.8, 0.2, 0.2);
            render_circle(cx, 300.0, 280.0, 200.0, 120.0, 0.0, 0.0, 50.0);
            cx.sg.set_draw_color(0.2, 0.6, 1.0);
            render_ring(cx, 520.0, 280.0, 200.0, 120.0, 10.0, -20.0, 40.0, 2.0);
            let (sx, sy) = world_to_screen(30.0, 40.0, 5.0, 200.0, 120.0);
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            let text = format!("w2s {} {}", tostring_num(sx), tostring_num(sy));
            cx.sg
                .draw_string(20.0, 420.0, Align::Left, 14.0, Font::Var, &text);
            cx.draw_controls(host, vp, None);
        },
    )
}
