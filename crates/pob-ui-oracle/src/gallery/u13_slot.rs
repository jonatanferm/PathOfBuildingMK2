//! Gallery "u13_slot" (U13): the Rust side of `lua/gallery/u13_slot.lua`. `U13Slot` is the
//! ItemSlotControl embedding pattern: `DropDownControl` as a field, `state` + `init`, and the
//! super calls as static calls.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_sg::{Align, Font};
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem};
use pob_ui_core::controls::list::{other_drag_source, ListData, ListValue};
use pob_ui_core::controls::{LabelControl, ListControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u13::{frame_bg, tostring_item, tostring_opt};
use super::_u9::Gallery;

/// `{ label = .., ok = .. }`
struct Entry {
    label: String,
    ok: bool,
}

fn entry(v: &DragValue) -> &Entry {
    v.0.downcast_ref::<Entry>().expect("an Entry payload")
}

/// `newClass("U13Slot", "DropDownControl")`
struct U13Slot {
    dd: DropDownControl,
    label: String,
    /// `self.lastEvent`
    last_event: Option<String>,
}

impl U13Slot {
    fn new(r: [Prop<f64>; 4], list: Vec<DropItem>, label: &str) -> New<U13Slot> {
        New::new(
            Base::new(Anchor::NONE, r),
            U13Slot {
                dd: DropDownControl::state(list, None, None, false),
                label: label.to_owned(),
                last_event: None,
            },
        )
        .host()
        .init(|cx, me| DropDownControl::init(cx, me.0))
        .init(|cx, me| {
            let id = me.0;
            // the selFunc closure over `self`
            cx.w_mut::<DropDownControl>(id).sel_func = Some(Rc::new(move |cx, index, value| {
                cx.w_mut::<U13Slot>(id).last_event =
                    Some(format!("sel {index} {}", tostring_item(value)));
            }));
            cx.w_mut::<DropDownControl>(id).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (mode, index, _value)| {
                    tooltip.clear(false);
                    let dropped = cx.w::<DropDownControl>(id).dropped;
                    tooltip.add_line(
                        cx,
                        14.0,
                        &format!(
                            "^7slot {} {}{}",
                            mode.as_str(),
                            tostring_opt(index),
                            if dropped { " dropped" } else { "" }
                        ),
                    );
                }));
        })
    }
}

impl Widget for U13Slot {
    const NAME: &'static str = "U13Slot";
    const CAPS: Caps = <DropDownControl as Widget>::CAPS
        .union(Caps::CAN_RECEIVE_DRAG)
        .union(Caps::RECEIVE_DRAG);
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        DropDownControl::is_mouse_over_comp(cx, me).0
    }
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        let (x, y) = cx.pos(me);
        let (width, height) = cx.size(me);
        let label = format!("^7{}:", cx.w::<U13Slot>(me).label);
        cx.sg.draw_string(
            x - 2.0,
            y + 2.0,
            Align::RightX,
            height - 4.0,
            Font::Var,
            &label,
        );
        // self.DropDownControl:Draw(viewPort): noTooltip is nil
        DropDownControl::draw(cx, me, vp, false);
        cx.draw_controls(me, vp, None);
        if cx.is_mouse_over(me) && other_drag_source(cx, me).is_some() {
            cx.sg.set_draw_color_a(0.2, 0.2, 1.0, 0.5);
            cx.sg.draw_image(None, x, y + height, width, 4.0);
        }
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            return None;
        }
        DropDownControl::on_key_down(cx, me, key)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        DropDownControl::on_key_up(cx, me, key)
    }
    fn on_char(cx: &mut Cx, me: CtrlId, ch: Key) -> Focus {
        DropDownControl::on_char(cx, me, ch)
    }
    fn can_receive_drag(_cx: &Cx, _me: CtrlId, ty: &str, v: &DragValue) -> bool {
        ty == "Entry" && entry(v).ok
    }
    fn receive_drag(cx: &mut Cx, me: CtrlId, _ty: &str, v: &DragValue, _source: CtrlId) {
        let label = entry(v).label.clone();
        let w = cx.w_mut::<U13Slot>(me);
        w.dd.list.push(DropItem::str(label.clone()));
        w.dd.sel_index = w.dd.list.len();
        w.last_event = Some(format!("got {label}"));
    }
    pob_ui_core::impl_parts!(U13Slot: dd => DropDownControl);
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            let minion_event: Rc<RefCell<String>> = Rc::new(RefCell::new("-".to_owned()));
            let rows: Vec<ListValue> = (1..=6usize)
                .map(|i| {
                    ListValue::of(Rc::new(Entry {
                        label: format!("Gem {i}"),
                        ok: i % 2 == 1,
                    }))
                })
                .collect();
            // the ListControl constructs the first Tooltip: Classes/Tooltip.lua loads
            module_load(cx);
            let src = cx.add(
                host,
                "src",
                ListControl::new(
                    Anchor::NONE,
                    rect(20, 40, 200, 200),
                    16.0,
                    true,
                    false,
                    ListData::Local(rows),
                    false,
                ),
            );
            {
                let w = cx.get_mut(src);
                w.ov.get_row_value = Some(Rc::new(|_cx, _me, _c, _i, v| {
                    let e = v.get::<Entry>().expect("an Entry");
                    format!("^7{}{}", e.label, if e.ok { "" } else { " (no)" })
                }));
                w.ov.get_drag_value = Some(Rc::new(|_cx, _me, _i, v| {
                    Some((Rc::from("Entry"), DragValue(v.data.clone())))
                }));
            }
            let slot = cx.add(
                host,
                "slot",
                U13Slot::new(rect(360, 40, 200, 20), vec![DropItem::str("None")], "Slot"),
            );
            let ev = minion_event.clone();
            let minion = cx.add(
                host,
                "minion",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(360, 120, 200, 20),
                    vec![DropItem::str("Minion A"), DropItem::str("Minion B")],
                    Some(Rc::new(move |_cx, index, _value| {
                        *ev.borrow_mut() = format!("minion sel {index}");
                    })),
                    None,
                    false,
                ),
            );
            let ev = minion_event.clone();
            cx.set_ov(minion, |ov| {
                ov.can_receive_drag = Some(Rc::new(|_cx, _me, _ty, v| !entry(v).ok));
                ov.receive_drag = Some(Rc::new(move |cx, me, _ty, v, _source| {
                    let sel = cx.w::<DropDownControl>(me).sel_index;
                    *ev.borrow_mut() = format!("minion got {} at {sel}", entry(v).label);
                }));
            });
            let plain = cx.add(
                host,
                "plain",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(360, 200, 200, 20),
                    vec![DropItem::str("Plain")],
                    None,
                    None,
                    false,
                ),
            );
            cx.get_mut(src).drag_target_list = Some(vec![slot.id(), minion.id(), plain.id()]);
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 10, 0, 16),
                    Prop::f(move |cx, _| {
                        let last = cx
                            .get(slot)
                            .last_event
                            .as_deref()
                            .unwrap_or("nil")
                            .to_owned();
                        format!("^7{last} | {}", minion_event.borrow())
                    }),
                ),
            );
        },
        frame_bg,
    )
}
