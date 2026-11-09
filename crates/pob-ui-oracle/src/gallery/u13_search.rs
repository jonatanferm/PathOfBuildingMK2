//! Gallery "u13_search" (U13): the Rust side of `lua/gallery/u13_search.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem, SelFunc};
use pob_ui_core::controls::LabelControl;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u13::{frame_bg, tostring_opt};
use super::_u9::Gallery;

/// A `chars[i]` table: `{ label, detail, searchFilter, id }` (label/detail/searchFilter live in
/// the DropItem).
struct Char {
    id: usize,
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            let status: Rc<RefCell<String>> = Rc::new(RefCell::new("-".to_owned()));
            let names = [
                "Aria", "Bran", "Cael", "Dara", "Eryn", "Fen", "Gale", "Hale", "Iris", "Joss",
                "Kael", "Lune",
            ];
            let classes = [
                "Witch", "Ranger", "Marauder", "Duelist", "Templar", "Shadow",
            ];
            let chars: Vec<DropItem> = names
                .iter()
                .enumerate()
                .map(|(i0, name)| {
                    let i = i0 + 1;
                    let class = classes[i0 % 6];
                    DropItem::new(format!("^xAF6025{name}"), Rc::new(Char { id: i }))
                        .detail(format!(
                            "^x808080{class} lvl {}",
                            tostring_num((i * 7) as f64)
                        ))
                        .search_filter(format!("{name} {class}"))
                })
                .collect();
            // the first DropDownControl's TooltipHost loads Classes/Tooltip.lua
            module_load(cx);
            let st = status.clone();
            let sel_chars: SelFunc = Rc::new(move |_cx, index, value| {
                let id = value
                    .and_then(|v| v.get::<Char>())
                    .expect("a chars entry")
                    .id;
                *st.borrow_mut() = format!("chars {index} {id}");
            });
            let c_chars = cx.add(
                host,
                "chars",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(20, 40, 260, 20),
                    chars,
                    Some(sel_chars),
                    None,
                    false,
                ),
            );
            cx.get_mut(c_chars).tt.tooltip_func =
                Some(Rc::new(|cx, tooltip, (mode, index, value)| {
                    tooltip.clear(false);
                    let filter = value
                        .and_then(|v| v.search_filter)
                        .unwrap_or_else(|| "nil".to_owned());
                    tooltip.add_line(
                        cx,
                        14.0,
                        &format!("^7{} {} {filter}", mode.as_str(), tostring_opt(index)),
                    );
                }));
            let words = ["Fire", "Ice", "Lightning", "Chaos", "Physical"];
            let loose: Vec<DropItem> = (1..=40usize)
                .map(|i| {
                    DropItem::str(format!(
                        "{} and {} damage, a rather long entry number {}",
                        words[(i - 1) % 5],
                        words[(i * 3) % 5],
                        tostring_num(i as f64)
                    ))
                })
                .collect();
            // `c.loose` / `c.box` inside the selFuncs: the ids exist once the controls are added
            let loose_id: Rc<Cell<Option<CtrlId>>> = Rc::default();
            let (st, lid) = (status.clone(), loose_id.clone());
            let sel_loose: SelFunc = Rc::new(move |cx, index, _value| {
                *st.borrow_mut() = format!("loose {index}");
                DropDownControl::check_dropped_width(cx, lid.get().expect("c.loose"), true);
            });
            let c_loose = cx.add(
                host,
                "loose",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(320, 40, 200, 20),
                    loose,
                    Some(sel_loose),
                    None,
                    true,
                ),
            );
            loose_id.set(Some(c_loose.id()));
            cx.get_mut(c_loose).max_dropped_width = 420.0;
            DropDownControl::check_dropped_width(cx, c_loose.id(), true);
            let boxes = [
                "Short",
                "A somewhat longer entry",
                "An entry so long that the box would need far more than the cap of three hundred and ninety pixels",
                "Mid length entry",
            ]
            .map(DropItem::str)
            .to_vec();
            let box_id: Rc<Cell<Option<CtrlId>>> = Rc::default();
            let (st, bid) = (status.clone(), box_id.clone());
            let sel_box: SelFunc = Rc::new(move |cx, index, _value| {
                *st.borrow_mut() = format!("box {index}");
                DropDownControl::check_dropped_width(cx, bid.get().expect("c.box"), true);
            });
            let c_box = cx.add(
                host,
                "box",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(560, 40, 190, 20),
                    boxes,
                    Some(sel_box),
                    None,
                    false,
                ),
            );
            box_id.set(Some(c_box.id()));
            cx.get_mut(c_box).enable_change_box_width = true;
            DropDownControl::check_dropped_width(cx, c_box.id(), true);
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 10, 0, 16),
                    Prop::f(move |_cx, _| format!("^7{}", status.borrow())),
                ),
            );
        },
        frame_bg,
    )
}
