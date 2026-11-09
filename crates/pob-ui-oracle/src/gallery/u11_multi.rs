//! Gallery "u11_multi" (U11): the Rust side of `lua/gallery/u11_multi.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::Cell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_lua::tostring;
use pob_ui_core::controls::{
    EditArgs, EditControl, LabelControl, ResizableEditControl, ScrollBarControl,
};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u11::{flush_effects, wrap_platform};
use super::_u9::Gallery;

fn opt(v: Option<f64>) -> String {
    v.map_or_else(|| "nil".to_owned(), tostring)
}

/// The gallery app.
pub fn app(mut parts: PlatformParts) -> Box<dyn App> {
    let effects = wrap_platform(&mut parts);
    let changes = Rc::new(Cell::new(0i64));
    let ch0 = changes.clone();
    Gallery::new(
        parts,
        move |cx, host| {
            let changes = ch0;
            let changed = {
                let changes = changes.clone();
                move || -> Option<Rc<dyn Fn(&mut Cx, &str)>> {
                    let changes = changes.clone();
                    Some(Rc::new(move |_: &mut Cx, _buf: &str| {
                        changes.set(changes.get() + 1);
                    }))
                }
            };
            // the first EditControl's TooltipHost loads Classes/Tooltip.lua
            module_load(cx);
            let notes = cx.add(
                host,
                "notes",
                EditControl::new(
                    Anchor::NONE,
                    rect(20, 20, 400, 160),
                    EditArgs {
                        init: Some(
                            "Line one has words\nsecond line, with punctuation!!\n\tindented   spaces\nfour\n\nsix after a blank"
                                .into(),
                        ),
                        filter: Some("^%C\t\n".into()),
                        change_func: changed(),
                        line_height: Some(16.0),
                        allow_zoom: true,
                        ..Default::default()
                    },
                ),
            );
            let resize = cx.add(
                host,
                "resize",
                ResizableEditControl::new(
                    Anchor::NONE,
                    rect(460, 20, 300, 100),
                    EditArgs {
                        init: Some("resize me\nline 2".into()),
                        filter: Some("^%C\t\n".into()),
                        change_func: changed(),
                        line_height: Some(14.0),
                        ..Default::default()
                    },
                    Some(200.0),
                    Some(60.0),
                    Some(500.0),
                    Some(300.0),
                ),
            );
            let small = cx.add(
                host,
                "small",
                EditControl::new(
                    Anchor::NONE,
                    rect(20, 220, 240, 80),
                    EditArgs {
                        init: Some(
                            "a\nb\nc\nd\ne\nf\ng\nh a much longer line that does not fit in the box at all\ni\nj"
                                .into(),
                        ),
                        filter: Some("^%C\t\n".into()),
                        change_func: changed(),
                        line_height: Some(14.0),
                        ..Default::default()
                    },
                ),
            );
            let ch = changes.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 320, 0, 16),
                    Prop::f(move |cx, _| {
                        let n = cx.get(notes);
                        let v = cx.child_as::<ScrollBarControl>(notes, "scrollBarV");
                        let h = cx.child_as::<ScrollBarControl>(notes, "scrollBarH");
                        format!(
                            "^7caret {} sel {} lh {} offV {} offH {} changes {} undo {}",
                            n.caret,
                            n.sel.map_or_else(|| "nil".to_owned(), |v| v.to_string()),
                            opt(n.line_height),
                            tostring(cx.get(v).offset),
                            tostring(cx.get(h).offset),
                            ch.get(),
                            n.undo.undo.len()
                        )
                    }),
                ),
            );
            cx.add(
                host,
                "status2",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 340, 0, 16),
                    Prop::f(move |cx, _| {
                        let s = cx.get(small);
                        let v = cx.child_as::<ScrollBarControl>(small, "scrollBarV");
                        let h = cx.child_as::<ScrollBarControl>(small, "scrollBarH");
                        let r = cx.w::<EditControl>(resize);
                        format!(
                            "^7small caret {} offV {} offH {} resize {}x{} caret {}",
                            s.caret,
                            tostring(cx.get(v).offset),
                            tostring(cx.get(h).offset),
                            tostring(cx.base(resize).width.raw()),
                            tostring(cx.base(resize).height.raw()),
                            r.caret
                        )
                    }),
                ),
            );
        },
        move |cx, host, ev, vp| {
            cx.process_controls_input(host, ev, vp);
            flush_effects(cx, &effects);
            cx.sg.set_draw_color(0.1, 0.1, 0.1);
            cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
            cx.draw_controls(host, vp, None);
            flush_effects(cx, &effects);
        },
    )
}
