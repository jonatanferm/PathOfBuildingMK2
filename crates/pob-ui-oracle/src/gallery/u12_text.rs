//! Gallery "u12_text" (U12): the Rust side of `lua/gallery/u12_text.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_sg::{Align, Font};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::text_list::{TextListColumn, TextListRow};
use pob_ui_core::controls::{ButtonControl, LabelControl, PathControl, TextListControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u12::frame;
use super::_u9::Gallery;

#[derive(Default)]
struct State {
    clicked: RefCell<Option<String>>,
    last_path: RefCell<Option<String>>,
}

fn button(
    cx: &mut Cx,
    host: CtrlId,
    key: &'static str,
    r: [Prop<f64>; 4],
    label: &'static str,
    on_click: impl Fn(&mut Cx) + 'static,
) {
    cx.add(host, key, ButtonControl::new(Anchor::NONE, r, label, on_click));
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    let s = Rc::new(State::default());
    Gallery::new(
        parts,
        move |cx, host| {
            let mut list = Vec::new();
            for sec in 1..=3 {
                list.push(Rc::new(TextListRow::new(18.0, &[&format!("^7Section {sec}:")])));
                for i in 1..=5 {
                    let mut row = TextListRow::new(
                        16.0,
                        &[
                            &format!("^7Stat {sec}.{i}:"),
                            &format!("^x33FF77{}", sec * 100 + i * 7),
                        ],
                    );
                    row.underline = Some(vec![false, i % 2 == 1]);
                    list.push(Rc::new(row));
                }
                let mut note =
                    TextListRow::new(14.0, &[&format!("^8a centred note for section {sec}")]);
                note.align = Some(Align::CenterX);
                note.x = Some(140.0);
                list.push(Rc::new(note));
                let mut fixed = TextListRow::new(16.0, &["^7Fixed:", &format!("^7{sec}")]);
                fixed.font = Some(Font::Fixed);
                list.push(Rc::new(fixed));
                list.push(Rc::new(TextListRow::new(6.0, &[])));
            }
            let text = cx.add(
                host,
                "text",
                TextListControl::new(
                    Anchor::NONE,
                    rect(20, 40, 320, 200),
                    Some(vec![
                        TextListColumn {
                            x: 170.0,
                            align: Align::RightX,
                        },
                        TextListColumn {
                            x: 174.0,
                            align: Align::Left,
                        },
                    ]),
                    list,
                    Some(vec![0.0, 136.0, 272.0]),
                ),
            );
            {
                let s = s.clone();
                cx.get_mut(text).on_click = Some(Rc::new(move |_cx, hovered| {
                    *s.clicked.borrow_mut() = Some(
                        hovered
                            .and_then(|h| h.line.cell(1).map(str::to_owned))
                            .unwrap_or_else(|| "nothing".to_owned()),
                    );
                }));
            }
            cx.add(
                host,
                "plainText",
                TextListControl::new(
                    Anchor::NONE,
                    rect(380, 40, 300, 120),
                    None,
                    vec![
                        Rc::new(TextListRow::new(16.0, &["^7Default column"])),
                        Rc::new(TextListRow::new(20.0, &["^xFFAA00Bigger line"])),
                        Rc::new(TextListRow::new(12.0, &["^8small"])),
                    ],
                    None,
                ),
            );
            {
                let s = s.clone();
                cx.add(
                    host,
                    "status",
                    LabelControl::new(
                        Anchor::NONE,
                        rect(20, 250, 0, 16),
                        Prop::f(move |cx, _| {
                            let h = cx.get(text).hovered_line.clone();
                            let sb = cx.child(text, "scrollBar").expect("scrollBar");
                            format!(
                                "^7clicked {} | hover {} {} | offset {}",
                                s.clicked.borrow().as_deref().unwrap_or("-"),
                                h.as_ref()
                                    .and_then(|h| h.line.cell(1).map(str::to_owned))
                                    .unwrap_or_else(|| "-".to_owned()),
                                h.as_ref().map_or_else(|| "-".to_owned(), |h| tostring_num(h.y)),
                                cx.w::<ScrollBarControl>(sb).offset as i64,
                            )
                        }),
                    ),
                );
            }
            // the first TooltipHost (the breadcrumb buttons) loads Classes/Tooltip.lua
            module_load(cx);
            let path = {
                let s = s.clone();
                cx.add(
                    host,
                    "path",
                    PathControl::new(
                        Anchor::NONE,
                        rect(20, 290, 600, 24),
                        "C:/Users/me/Builds/".to_owned(),
                        Some("Alpha/Beta/Gamma/".to_owned()),
                        Some(Rc::new(move |_cx, sub_path| {
                            *s.last_path.borrow_mut() = Some(sub_path.to_owned());
                        })),
                    ),
                )
            };
            cx.add(
                host,
                "noBase",
                PathControl::new(
                    Anchor::NONE,
                    rect(20, 330, 600, 24),
                    "nobase".to_owned(),
                    None,
                    None,
                ),
            );
            button(cx, host, "undo", rect(20, 370, 80, 20), "Undo", move |cx| {
                PathControl::undo(cx, path.id())
            });
            button(cx, host, "redo", rect(110, 370, 80, 20), "Redo", move |cx| {
                PathControl::redo(cx, path.id())
            });
            button(cx, host, "deeper", rect(200, 370, 80, 20), "Deeper", move |cx| {
                let sub = cx.get(path).sub_path.clone().unwrap_or_default();
                PathControl::set_sub_path(cx, path.id(), &format!("{sub}Deep/"), false)
            });
            button(cx, host, "short", rect(290, 370, 80, 20), "Short", move |cx| {
                PathControl::set_sub_path(cx, path.id(), "Alpha/", false)
            });
            let s = s.clone();
            cx.add(
                host,
                "status2",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 400, 0, 16),
                    Prop::f(move |cx, _| {
                        let p = cx.get(path);
                        format!(
                            "^7path '{}' onChange '{}' undo {} redo {}",
                            p.sub_path.as_deref().unwrap_or(""),
                            s.last_path.borrow().as_deref().unwrap_or("-"),
                            p.undo.undo.len(),
                            p.undo.redo.len(),
                        )
                    }),
                ),
            );
        },
        frame,
    )
}
