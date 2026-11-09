//! Gallery "u11_single" (U11): the Rust side of `lua/gallery/u11_single.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::edit::InactiveText;
use pob_ui_core::controls::{EditArgs, EditControl, LabelControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u11::{flush_effects, wrap_platform};
use super::_u9::Gallery;

#[derive(Default)]
struct State {
    changes: Cell<i64>,
    enters: Cell<i64>,
    last: RefCell<String>,
}

fn edit(
    cx: &mut Cx,
    host: CtrlId,
    key: &'static str,
    r: [Prop<f64>; 4],
    args: EditArgs,
) -> Ctrl<EditControl> {
    cx.add(host, key, EditControl::new(Anchor::NONE, r, args))
}

/// The gallery app.
pub fn app(mut parts: PlatformParts) -> Box<dyn App> {
    let effects = wrap_platform(&mut parts);
    let s = Rc::new(State::default());
    let s0 = s.clone();
    Gallery::new(
        parts,
        move |cx, host| {
            let s = s0;
            let changed = {
                let s = s.clone();
                move || -> Option<Rc<dyn Fn(&mut Cx, &str)>> {
                    let s = s.clone();
                    Some(Rc::new(move |_: &mut Cx, buf: &str| {
                        s.changes.set(s.changes.get() + 1);
                        *s.last.borrow_mut() = buf.to_owned();
                    }))
                }
            };
            // the first EditControl's TooltipHost loads Classes/Tooltip.lua
            module_load(cx);
            let plain = edit(
                cx,
                host,
                "plain",
                rect(20, 20, 260, 20),
                EditArgs {
                    init: Some("Hello there".into()),
                    prompt: Some("Name".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            cx.get_mut(plain).tt.tooltip_text = Some(Prop::Val(Some(
                "^7Type a name.\nOnly letters, digits and punctuation.".to_owned(),
            )));
            let num = edit(
                cx,
                host,
                "num",
                rect(20, 50, 120, 20),
                EditArgs {
                    init: Some("5".into()),
                    filter: Some("%D".into()),
                    limit: Some(4),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            cx.add_to_tab_group(num, plain);
            let signed = edit(
                cx,
                host,
                "signed",
                rect(160, 50, 120, 20),
                EditArgs {
                    init: Some(String::new()),
                    filter: Some("^%-%d".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            EditControl::set_placeholder(cx, signed.id(), "3", false);
            cx.get_mut(signed).number_inc = Some(2.0);
            cx.add_to_tab_group(signed, plain);
            let clear = edit(
                cx,
                host,
                "clear",
                rect(20, 80, 200, 20),
                EditArgs {
                    init: Some("clear me".into()),
                    filter: Some("%c".into()),
                    limit: Some(12),
                    change_func: changed(),
                    clearable: true,
                    ..Default::default()
                },
            );
            let secret = edit(
                cx,
                host,
                "secret",
                rect(20, 110, 200, 20),
                EditArgs {
                    init: Some("hunter2".into()),
                    prompt: Some("Pass".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            EditControl::set_protected(cx, secret.id(), true);
            let inactive = edit(
                cx,
                host,
                "inactive",
                rect(20, 140, 200, 20),
                EditArgs {
                    init: Some("raw".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            cx.get_mut(inactive).inactive_text = Some(InactiveText::Fn(Rc::new(|_, buf| {
                format!("^xFF8000[{buf}] {}", buf.len())
            })));
            let inactive_str = edit(
                cx,
                host,
                "inactiveStr",
                rect(240, 140, 200, 20),
                EditArgs {
                    init: Some("hidden".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            cx.get_mut(inactive_str).inactive_text =
                Some(InactiveText::Str("^1fixed text".to_owned()));
            cx.get_mut(inactive_str).inactive_col = "^3".to_owned();
            let ph = edit(
                cx,
                host,
                "ph",
                rect(20, 170, 200, 20),
                EditArgs {
                    init: Some(String::new()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            EditControl::set_placeholder(cx, ph.id(), "Search...", true);
            let st = s.clone();
            cx.get_mut(ph).enter_func = Some(Rc::new(move |_, buf| {
                st.enters.set(st.enters.get() + 1);
                *st.last.borrow_mut() = format!("enter:{buf}");
            }));
            cx.get_mut(ph).border_func = Some(Rc::new(|_| (0.8, 0.2, 0.2)));
            let paste = edit(
                cx,
                host,
                "paste",
                rect(20, 200, 300, 20),
                EditArgs {
                    init: Some(String::new()),
                    prompt: Some("Paste".into()),
                    filter: Some("%c".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            cx.get_mut(paste).paste_filter = Some(Rc::new(|_, text| pob_lua::string::upper(text)));
            cx.add(
                host,
                "disabled",
                EditControl::new(
                    Anchor::NONE,
                    rect(320, 20, 150, 20),
                    EditArgs {
                        init: Some("nope".into()),
                        prompt: Some("Off".into()),
                        change_func: changed(),
                        ..Default::default()
                    },
                )
                .enabled(Prop::Val(false)),
            );
            edit(
                cx,
                host,
                "url",
                rect(340, 200, 300, 20),
                EditArgs {
                    init: Some("see https://example.com/x".into()),
                    change_func: changed(),
                    ..Default::default()
                },
            );
            let st = s.clone();
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 240, 0, 16),
                    Prop::f(move |cx, _| {
                        let p = cx.get(plain);
                        format!(
                            "^7caret {} sel {} changes {} enters {} last '{}' num '{}' signed '{}'",
                            p.caret,
                            p.sel.map_or_else(|| "nil".to_owned(), |v| v.to_string()),
                            st.changes.get(),
                            st.enters.get(),
                            st.last.borrow(),
                            cx.get(num).buf,
                            cx.get(signed).buf
                        )
                    }),
                ),
            );
            cx.add(
                host,
                "status2",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 260, 0, 16),
                    Prop::f(move |cx, _| {
                        let p = cx.get(plain);
                        format!(
                            "^7paste '{}' clear '{}' secret '{}' undo {} redo {}",
                            cx.get(paste).buf,
                            cx.get(clear).buf,
                            cx.get(secret).buf,
                            p.undo.undo.len(),
                            p.undo.redo.len()
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
