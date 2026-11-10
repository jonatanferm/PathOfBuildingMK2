//! Gallery "u9_slider" (U9): the Rust side of `lua/gallery/u9_slider.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::Cell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_lua::format::{lua_format, FmtArg};
use pob_ui_core::controls::slider::{ScrollWheelSpeedTbl, SliderControl};
use pob_ui_core::controls::LabelControl;
use pob_ui_core::prelude::*;

use super::_u9::{tooltip_module_load, Gallery};

#[derive(Default)]
struct State {
    changes: Cell<i64>,
    last_val: Cell<f64>,
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State::default());
    Gallery::new(
        parts,
        move |cx, host| {
            let st = s.clone();
            let changed: Rc<dyn Fn(&mut Cx, f64)> = Rc::new(move |_, val| {
                st.changes.set(st.changes.get() + 1);
                st.last_val.set(val);
            });
            // the first SliderControl loads Classes/Tooltip.lua
            tooltip_module_load(cx);
            cx.add(
                host,
                "plain",
                SliderControl::new(
                    Anchor::NONE,
                    rect(20, 20, 200, 18),
                    Some(changed.clone()),
                    None,
                ),
            );
            let div = cx.add(
                host,
                "div",
                SliderControl::new(
                    Anchor::NONE,
                    rect(20, 60, 300, 16),
                    Some(changed.clone()),
                    None,
                ),
            );
            cx.get_mut(div).div_count = Some(4.0);
            let custom = cx.add(
                host,
                "custom",
                SliderControl::new(
                    Anchor::NONE,
                    rect(20, 100, 150, 20),
                    None,
                    Some(ScrollWheelSpeedTbl {
                        shift: 0.2,
                        ctrl: 0.002,
                        default: 0.02,
                    }),
                ),
            );
            let disabled = cx.add(
                host,
                "disabled",
                SliderControl::new(
                    Anchor::NONE,
                    rect(20, 140, 100, 18),
                    Some(changed.clone()),
                    None,
                )
                .enabled(Prop::Val(false)),
            );
            cx.get_mut(disabled).val = 0.5;
            let st = s.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 200, 0, 16),
                    Prop::f(move |cx, _| {
                        let (idx, frac) = SliderControl::get_div_val(cx, div.id(), None);
                        lua_format(
                            "changes %d  val %.4f  div %d %.4f  custom %.4f",
                            &[
                                FmtArg::Num(st.changes.get() as f64),
                                FmtArg::Num(st.last_val.get()),
                                FmtArg::Num(idx),
                                FmtArg::Num(frac),
                                FmtArg::Num(cx.get(custom).val),
                            ],
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
