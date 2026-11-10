//! Gallery "u9_check" (U9): the Rust side of `lua/gallery/u9_check.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::{CheckBoxControl, LabelControl};
use pob_ui_core::prelude::*;

use super::_u9::{tooltip_module_load, Gallery};

struct State {
    changes: Cell<i64>,
    last: RefCell<String>,
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State {
        changes: Cell::new(0),
        last: RefCell::new("none".to_owned()),
    });
    Gallery::new(
        parts,
        move |cx, host| {
            // the first CheckBoxControl loads Classes/Tooltip.lua
            tooltip_module_load(cx);
            let st = s.clone();
            let changed: Rc<dyn Fn(&mut Cx, bool)> = Rc::new(move |_, state| {
                st.changes.set(st.changes.get() + 1);
                *st.last.borrow_mut() = state.to_string();
            });
            let none = || None::<String>;
            let left = cx.add(
                host,
                "left",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(200, 20, 18, 0),
                    "Label left:",
                    Some(changed.clone()),
                    None,
                    false,
                ),
            );
            let right = cx.add(
                host,
                "right",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(200, 50, 20, 0),
                    "^7Label right",
                    Some(changed.clone()),
                    None,
                    true,
                ),
            );
            cx.get_mut(right).label_right = true;
            cx.add(
                host,
                "nolabel",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(200, 80, 18, 0),
                    none(),
                    Some(changed.clone()),
                    None,
                    false,
                ),
            );
            cx.add(
                host,
                "empty",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(260, 80, 18, 0),
                    "",
                    Some(changed.clone()),
                    None,
                    true,
                ),
            );
            cx.add(
                host,
                "disabled",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(200, 110, 18, 0),
                    "Disabled:",
                    Some(changed.clone()),
                    None,
                    true,
                )
                .enabled(Prop::Val(false)),
            );
            cx.add(
                host,
                "disabledOff",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(260, 110, 18, 0),
                    none(),
                    Some(changed.clone()),
                    None,
                    false,
                )
                .enabled(Prop::Val(false)),
            );
            let border = cx.add(
                host,
                "border",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(200, 140, 18, 0),
                    "Border:",
                    Some(changed.clone()),
                    None,
                    false,
                ),
            );
            let st = s.clone();
            cx.get_mut(border).border_func = Some(Rc::new(move |_| {
                if st.changes.get() % 2 == 1 {
                    return (0.753, 0.502, 0.502);
                }
                (0.451, 0.576, 0.702)
            }));
            let fn_label = cx.add(
                host,
                "fnLabel",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(200, 170, 18, 0),
                    "Fn label:",
                    Some(changed.clone()),
                    None,
                    false,
                ),
            );
            let st = s.clone();
            cx.get_mut(fn_label).label =
                Prop::f(move |_, _| Some(format!("Changes {}:", st.changes.get())));
            cx.add(
                host,
                "anchored",
                CheckBoxControl::new(
                    Anchor::new(Left, left, Right),
                    rect(150, 0, 18, 0),
                    "Anchored:",
                    None,
                    None,
                    false,
                ),
            );
            let st = s.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 220, 0, 16),
                    Prop::f(move |_, _| {
                        format!("changes {} last {}", st.changes.get(), st.last.borrow())
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
