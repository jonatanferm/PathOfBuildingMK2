//! Gallery "u12_drag" (U12): the Rust side of `lua/gallery/u12_drag.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_ui_core::controls::list::{ListData, ListHooks, ListValue};
use pob_ui_core::controls::{LabelControl, ListControl, PathControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;

use super::_u12::{frame, tostring_opt};
use super::_u9::Gallery;

/// `{ label = .., folder = .. }`
struct Item {
    label: String,
    folder: bool,
}

fn item(v: &ListValue) -> &Item {
    v.get::<Item>().expect("an Item")
}

fn drag_item(v: &DragValue) -> &Item {
    v.0.downcast_ref::<Item>().expect("an Item payload")
}

/// `newClass("U12DragList", "ListControl")`
struct U12DragList {
    list: ListControl,
    last_event: Option<String>,
}

fn get_row_value(_cx: &Cx, _me: CtrlId, _c: usize, _i: usize, v: &ListValue) -> String {
    format!("^7{}", item(v).label)
}

fn get_drag_value(_cx: &Cx, _me: CtrlId, _i: usize, v: &ListValue) -> Option<(Rc<str>, DragValue)> {
    Some((Rc::from("Entry"), DragValue(v.data.clone())))
}

fn on_order_change(cx: &mut Cx, me: CtrlId, sel_index: usize, sel_drag_index: usize) {
    cx.w_mut::<U12DragList>(me).last_event = Some(format!("order {sel_index}>{sel_drag_index}"));
}

fn on_drag_send(cx: &mut Cx, me: CtrlId, index: usize, v: &ListValue, _target: CtrlId) {
    let label = item(v).label.clone();
    cx.w_mut::<U12DragList>(me).last_event = Some(format!("sent {index} {label}"));
}

static HOOKS: ListHooks = ListHooks {
    get_row_value,
    get_drag_value: Some(get_drag_value),
    on_order_change: Some(on_order_change),
    on_drag_send: Some(on_drag_send),
    ..ListHooks::BASE
};

impl U12DragList {
    fn new(r: [Prop<f64>; 4], rows: Vec<ListValue>) -> New<U12DragList> {
        let mut list = ListControl::state(&HOOKS, 16.0, true, true, ListData::Local(rows), false);
        list.drag_target_list = Some(Vec::new());
        New::new(
            Base::new(Anchor::NONE, r),
            U12DragList {
                list,
                last_event: None,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
    }
}

impl Widget for U12DragList {
    const NAME: &'static str = "U12DragList";
    const CAPS: Caps = <ListControl as Widget>::CAPS
        .union(Caps::CAN_RECEIVE_DRAG)
        .union(Caps::RECEIVE_DRAG);
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, no_tooltip: bool) {
        ListControl::draw(cx, me, vp, no_tooltip)
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, dbl: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, dbl)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }
    fn can_receive_drag(_cx: &Cx, _me: CtrlId, ty: &str, _v: &DragValue) -> bool {
        ty == "Entry"
    }
    fn receive_drag(cx: &mut Cx, me: CtrlId, _ty: &str, v: &DragValue, _source: CtrlId) {
        let label = drag_item(v).label.clone();
        let at = cx
            .w::<ListControl>(me)
            .sel_drag_index
            .unwrap_or_else(|| ListControl::list_len(cx, me) + 1);
        ListControl::list_insert(
            cx,
            me,
            at,
            ListValue::of(Rc::new(Item {
                label: format!("{label}+"),
                folder: false,
            })),
        );
        cx.w_mut::<U12DragList>(me).last_event = Some(format!("got {label}"));
    }
    pob_ui_core::impl_parts!(U12DragList: list => ListControl);
}

fn rows(prefix: &str, n: usize, folder_every: Option<usize>) -> Vec<ListValue> {
    (1..=n)
        .map(|i| {
            ListValue::of(Rc::new(Item {
                label: format!("{prefix} {i}"),
                folder: folder_every.is_some_and(|f| i % f == 0),
            }))
        })
        .collect()
}

fn list(cx: &mut Cx, host: CtrlId, key: &'static str, r: [Prop<f64>; 4], rows: Vec<ListValue>) -> CtrlId {
    cx.add(
        host,
        key,
        ListControl::new(Anchor::NONE, r, 16.0, true, false, ListData::Local(rows), false),
    )
    .id()
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            // the first ListControl constructs the first Tooltip: Classes/Tooltip.lua loads
            module_load(cx);
            let src = cx.add(host, "src", U12DragList::new(rect(20, 40, 240, 200), rows("Src", 8, None)));
            let dst = cx.add(host, "dst", U12DragList::new(rect(300, 40, 240, 200), rows("Dst", 3, None)));
            let reject = list(cx, host, "reject", rect(580, 40, 200, 100), rows("Reject", 3, None));
            cx.w_mut::<ListControl>(reject).ov.get_row_value =
                Some(Rc::new(|_cx, _me, _c, _i, v| item(v).label.clone()));
            cx.set_ov(reject, |ov| ov.can_receive_drag = Some(Rc::new(|_cx, _me, _t, _v| false)));
            let folders = list(cx, host, "folders", rect(580, 180, 200, 120), rows("Node", 6, Some(2)));
            let folders_event: Rc<RefCell<Option<String>>> = Rc::default();
            {
                let w = cx.w_mut::<ListControl>(folders);
                w.ov.get_row_value = Some(Rc::new(|_cx, _me, _c, _i, v| {
                    let it = item(v);
                    format!("{}{}", if it.folder { "^xFFFF77" } else { "^7" }, it.label)
                }));
                w.ov.get_drag_value = Some(Rc::new(get_drag_value));
                w.ov.can_drag_to_value = Some(Rc::new(|cx, _me, _i, v, source| {
                    item(v).folder && {
                        let source = source.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'source' (a nil value)")
                        });
                        cx.w::<ListControl>(source).sel_value.as_ref() != Some(v)
                    }
                }));
                w.drag_target_list = Some(vec![folders]);
            }
            {
                let ev = folders_event.clone();
                cx.set_ov(folders, move |ov| {
                    ov.receive_drag = Some(Rc::new(move |cx, me, _ty, v, _source| {
                        let hover = cx
                            .w::<ListControl>(me)
                            .hover_value
                            .as_ref()
                            .map_or_else(|| "-".to_owned(), |h| item(h).label.clone());
                        *ev.borrow_mut() = Some(format!("into {hover}: {}", drag_item(v).label));
                    }))
                });
            }
            let last_path: Rc<RefCell<Option<String>>> = Rc::default();
            let path_event: Rc<RefCell<Option<String>>> = Rc::default();
            let lp = last_path.clone();
            let path = cx.add(
                host,
                "path",
                PathControl::new(
                    Anchor::NONE,
                    rect(20, 270, 520, 24),
                    "Builds/".to_owned(),
                    Some("One/Two/".to_owned()),
                    Some(Rc::new(move |_cx, sub_path| *lp.borrow_mut() = Some(sub_path.to_owned()))),
                ),
            );
            {
                let ev = path_event.clone();
                cx.set_ov(path, move |ov| {
                    ov.receive_drag = Some(Rc::new(move |_cx, _me, _ty, v, _source| {
                        *ev.borrow_mut() = Some(format!("path got {}", drag_item(v).label));
                    }))
                });
            }
            cx.get_mut(src)
                .list
                .drag_target_list
                .as_mut()
                .expect("set")
                .extend([dst.id(), reject, folders, path.id()]);
            cx.get_mut(dst)
                .list
                .drag_target_list
                .as_mut()
                .expect("set")
                .push(src.id());
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 310, 0, 16),
                    Prop::f(move |cx, _| {
                        format!(
                            "^7src: {} ({}) | dst: {} ({})",
                            cx.get(src).last_event.as_deref().unwrap_or("-"),
                            ListControl::list_len(cx, src.id()),
                            cx.get(dst).last_event.as_deref().unwrap_or("-"),
                            ListControl::list_len(cx, dst.id()),
                        )
                    }),
                ),
            );
            cx.add(
                host,
                "status2",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 330, 0, 16),
                    Prop::f(move |cx, _| {
                        format!(
                            "^7folders: {} | path: {} {} | sel {}",
                            folders_event.borrow().as_deref().unwrap_or("-"),
                            path_event.borrow().as_deref().unwrap_or("-"),
                            last_path.borrow().as_deref().unwrap_or("-"),
                            tostring_opt(cx.get(src).list.sel_index),
                        )
                    }),
                ),
            );
        },
        frame,
    )
}
