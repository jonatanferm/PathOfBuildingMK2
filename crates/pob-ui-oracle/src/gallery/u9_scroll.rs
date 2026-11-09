//! Gallery "u9_scroll" (U9): the Rust side of `lua/gallery/u9_scroll.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::scroll_bar::{ScrollBarControl, ScrollDir};
use pob_ui_core::controls::{ButtonControl, LabelControl};
use pob_ui_core::prelude::*;

use super::_u9::{tooltip_module_load, Gallery};

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            let v = cx.add(
                host,
                "v",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(300, 20, 18, 300),
                    Some(40.0),
                    None,
                    false,
                ),
            );
            let h = cx.add(
                host,
                "h",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(20, 340, 400, 16),
                    Some(30.0),
                    Some(ScrollDir::Horizontal),
                    false,
                ),
            );
            let auto = cx.add(
                host,
                "auto",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(340, 20, 14, 200),
                    None,
                    Some(ScrollDir::Vertical),
                    true,
                ),
            );
            cx.add(
                host,
                "off",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(380, 20, 18, 200),
                    Some(20.0),
                    None,
                    false,
                ),
            );
            let locked = cx.add(
                host,
                "locked",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(420, 20, 18, 200),
                    Some(20.0),
                    None,
                    false,
                ),
            );
            cx.get_mut(locked).locked = Prop::Val(true);
            // the first ButtonControl loads Classes/Tooltip.lua
            tooltip_module_load(cx);
            cx.add(
                host,
                "intoView",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(20, 400, 100, 20),
                    "Into view",
                    move |cx| ScrollBarControl::scroll_into_view(cx, v.id(), 600.0, 50.0),
                ),
            );
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 370, 0, 16),
                    Prop::f(move |cx, _| {
                        format!(
                            "v {}  h {}  auto {} ({})  locked {}",
                            cx.get(v).offset as i64,
                            cx.get(h).offset as i64,
                            cx.get(auto).offset as i64,
                            ScrollBarControl::enabled_raw(cx, auto.id()),
                            cx.get(locked).offset as i64
                        )
                    }),
                ),
            );
        },
        |cx, host, ev, vp| {
            let c = |cx: &Cx, k: &'static str| cx.child(host, k).unwrap();
            let (v, h, auto, off, locked) = (
                c(cx, "v"),
                c(cx, "h"),
                c(cx, "auto"),
                c(cx, "off"),
                c(cx, "locked"),
            );
            ScrollBarControl::set_content_dimension(cx, v, 1000.0, 300.0);
            ScrollBarControl::set_content_dimension(cx, h, 1200.0, 400.0);
            let con = if cx.w::<ScrollBarControl>(v).offset > 400.0 {
                100.0
            } else {
                500.0
            };
            ScrollBarControl::set_content_dimension(cx, auto, con, 200.0);
            ScrollBarControl::set_content_dimension(cx, off, 100.0, 200.0);
            ScrollBarControl::set_content_dimension(cx, locked, 800.0, 200.0);
            cx.process_controls_input(host, ev, vp);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            cx.draw_controls(host, vp, None);
        },
    )
}
