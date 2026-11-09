//! Gallery "u13_drop" (U13): the Rust side of `lua/gallery/u13_drop.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::drop_down::{DropDownControl, DropTipArgs, SelFunc};
use pob_ui_core::controls::LabelControl;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;
use pob_ui_core::tooltip::Tooltip;

use super::_u13::{options, tostring_item, tostring_opt};
use super::_u9::{ControlHost, Gallery};

const VP2: ViewPort = ViewPort {
    x: 900.0,
    y: 420.0,
    width: 360.0,
    height: 200.0,
};

fn tooltip_func(cx: &mut Cx, tooltip: &mut Tooltip, (mode, index, value): DropTipArgs) {
    tooltip.clear(false);
    tooltip.add_line(
        cx,
        14.0,
        &format!(
            "^7{} {} {}",
            mode.as_str(),
            tostring_opt(index),
            tostring_item(value.as_ref())
        ),
    );
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let host2: Rc<Cell<Option<CtrlId>>> = Rc::default();
    let h2 = host2.clone();
    Gallery::new(
        parts,
        move |cx, host| {
            let status: Rc<RefCell<String>> = Rc::new(RefCell::new("-".to_owned()));
            let sel_func = |name: &'static str| -> Option<SelFunc> {
                let status = status.clone();
                Some(Rc::new(move |_cx, index, value| {
                    *status.borrow_mut() = format!("{name} {index} {}", tostring_item(value));
                }))
            };
            let second = cx
                .add_free(
                    None,
                    New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), ControlHost).host(),
                )
                .id();
            h2.set(Some(second));
            // the first DropDownControl's TooltipHost loads Classes/Tooltip.lua
            module_load(cx);
            let basic = cx.add(
                host,
                "basic",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(20, 40, 200, 20),
                    options("Option", 30),
                    sel_func("basic"),
                    None,
                    false,
                ),
            );
            cx.get_mut(basic).tt.tooltip_func = Some(Rc::new(tooltip_func));
            cx.add(
                host,
                "text",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(260, 40, 200, 20),
                    options("Text", 5),
                    sel_func("text"),
                    Some("^7A plain tooltip text".into()),
                    false,
                ),
            );
            let disabled = cx.add(
                host,
                "disabled",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(500, 40, 160, 20),
                    options("Off", 3),
                    sel_func("disabled"),
                    Some("^7never shown".into()),
                    false,
                ),
            );
            cx.base_mut(disabled).enabled = Prop::Val(false);
            cx.get_mut(disabled).sel_index = 2;
            let border = cx.add(
                host,
                "border",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(700, 40, 160, 20),
                    options("Border", 4),
                    sel_func("border"),
                    None,
                    false,
                ),
            );
            cx.get_mut(border).border_func = Some(Rc::new(|_cx, _me| (0.8, 0.3, 0.1)));
            let bottom = cx.add(
                host,
                "bottom",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(20, 680, 200, 20),
                    options("Bottom", 10),
                    sel_func("bottom"),
                    None,
                    false,
                ),
            );
            cx.get_mut(bottom).tt.tooltip_func = Some(Rc::new(tooltip_func));
            let st = status.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 10, 0, 16),
                    Prop::f(move |_cx, _| format!("^7{}", st.borrow())),
                ),
            );
            cx.add(
                second,
                "clipBelow",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(910, 430, 200, 20),
                    options("Below", 30),
                    sel_func("clipBelow"),
                    None,
                    false,
                ),
            );
            let clip_above = cx.add(
                second,
                "clipAbove",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(910, 590, 200, 20),
                    options("Above", 30),
                    sel_func("clipAbove"),
                    None,
                    false,
                ),
            );
            cx.get_mut(clip_above).tt.tooltip_func = Some(Rc::new(tooltip_func));
        },
        move |cx, host, ev, vp| {
            let second = host2.get().expect("G:Init ran");
            cx.process_controls_input(host, ev, vp);
            cx.process_controls_input(second, ev, &VP2);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            cx.sg.set_draw_color(0.2, 0.2, 0.3);
            cx.sg.draw_image(None, VP2.x, VP2.y, VP2.width, VP2.height);
            cx.draw_controls(host, vp, None);
            cx.draw_controls(second, &VP2, None);
        },
    )
}
