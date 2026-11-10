//! Gallery "u12_list" (U12): the Rust side of `lua/gallery/u12_list.lua`.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_lua::math::{m_max, m_min};
use pob_ui_core::controls::list::{ListColumn, ListData, ListHooks, ListValue};
use pob_ui_core::controls::{LabelControl, ListControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;
use pob_ui_core::tooltip::Tooltip;

use super::_u12::{frame, load_image, tostring_opt};
use super::_u9::Gallery;

/// A `rows[i]` table of the subclass list.
struct Entry {
    name: String,
    desc: String,
    num: f64,
}

/// `{ label = .. }` rows of the plain lists.
struct Plain {
    label: String,
}

/// `newClass("U12TestList", "ListControl")`
struct U12TestList {
    list: ListControl,
    /// `self.lastEvent`
    last_event: Option<String>,
}

fn entry(v: &ListValue) -> &Entry {
    v.get::<Entry>().expect("an Entry")
}

fn set_event(cx: &mut Cx, me: CtrlId, s: String) {
    cx.w_mut::<U12TestList>(me).last_event = Some(s);
}

fn get_row_value(_cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
    let e = entry(v);
    if column == 1 {
        return e.name.clone();
    } else if column == 2 {
        return e.desc.clone();
    }
    format!("^7{}", tostring_num(e.num))
}

fn add_value_tooltip(cx: &mut Cx, _me: CtrlId, tt: &mut Tooltip, index: usize, v: &ListValue) {
    let e = entry(v);
    tt.clear(false);
    tt.add_line(cx, 16.0, &format!("^7{}", e.name));
    tt.add_line(cx, 14.0, &format!("^8Row {}: {}", tostring_num(index as f64), e.desc));
}

fn re_sort(cx: &mut Cx, me: CtrlId, col_index: usize) {
    // names and nums are distinct: any sort orders them as table.sort does
    if let ListData::Local(l) = &mut cx.w_mut::<ListControl>(me).list {
        if col_index == 1 {
            l.sort_by(|a, b| entry(a).name.cmp(&entry(b).name));
        } else {
            l.sort_by(|a, b| entry(b).num.partial_cmp(&entry(a).num).expect("finite"));
        }
    }
    set_event(cx, me, format!("sort {col_index}"));
}

fn on_select(cx: &mut Cx, me: CtrlId, index: usize, _v: &ListValue) {
    set_event(cx, me, format!("select {index}"));
}

fn on_sel_click(cx: &mut Cx, me: CtrlId, index: usize, _v: &ListValue, dbl: bool) -> Option<bool> {
    set_event(cx, me, format!("click {index}{}", if dbl { " double" } else { "" }));
    None
}

fn on_sel_copy(cx: &mut Cx, me: CtrlId, _index: usize, v: &ListValue) {
    let name = entry(v).name.clone();
    set_event(cx, me, format!("copy {name}"));
}

fn on_sel_delete(cx: &mut Cx, me: CtrlId, index: usize, _v: &ListValue) {
    ListControl::list_remove(cx, me, index);
    {
        let w = cx.w_mut::<ListControl>(me);
        w.sel_index = None;
        w.sel_value = None;
    }
    set_event(cx, me, format!("delete {index}"));
}

fn on_sel_key_down(cx: &mut Cx, me: CtrlId, _index: usize, _v: &ListValue, key: Key) {
    set_event(cx, me, format!("key {}", key.name()));
}

static HOOKS: ListHooks = ListHooks {
    get_row_value,
    add_value_tooltip: Some(add_value_tooltip),
    re_sort: Some(re_sort),
    on_select: Some(on_select),
    on_sel_click: Some(on_sel_click),
    on_sel_copy: Some(on_sel_copy),
    on_sel_delete: Some(on_sel_delete),
    on_sel_key_down: Some(on_sel_key_down),
    ..ListHooks::BASE
};

impl U12TestList {
    fn new(anchor: Anchor, r: [Prop<f64>; 4], rows: Vec<ListValue>) -> New<U12TestList> {
        let mut list = ListControl::state(&HOOKS, 16.0, true, false, ListData::Local(rows), false);
        list.col_list = vec![
            ListColumn::new(Some(Prop::Val(110.0)), Some("Name"), true),
            ListColumn::new(
                Some(Prop::f(|cx, me| cx.width(me) - 200.0)),
                Some("Description"),
                false,
            ),
            ListColumn::new(Some(Prop::Val(70.0)), Some("Num"), true),
        ];
        list.col_labels = true;
        list.label = Some(Prop::Val("^7Columns:".to_owned()));
        New::new(
            Base::new(anchor, r),
            U12TestList {
                list,
                last_event: None,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
    }
}

impl Widget for U12TestList {
    const NAME: &'static str = "U12TestList";
    const CAPS: Caps = <ListControl as Widget>::CAPS;
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
    pob_ui_core::impl_parts!(U12TestList: list => ListControl);
}

fn plain_rows(labels: impl IntoIterator<Item = String>) -> Vec<ListValue> {
    labels
        .into_iter()
        .map(|label| ListValue::of(Rc::new(Plain { label })))
        .collect()
}

fn plain(v: &ListValue) -> &Plain {
    v.get::<Plain>().expect("a Plain row")
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            let names = [
                "Fireball",
                "Arc",
                "Cyclone",
                "Blade Vortex",
                "Zombie",
                "Frostbolt",
                "Ice Nova",
                "Spark",
                "Vaal Haste",
                "Herald of Ice",
                "Molten Strike",
                "Earthquake",
                "Tornado Shot",
                "Lightning Arrow",
            ];
            let rows: Vec<ListValue> = names
                .iter()
                .enumerate()
                .map(|(i, name)| {
                    let i = (i + 1) as f64;
                    let long = if i % 3.0 == 0.0 {
                        ", a much longer text that will not fit into its column at all"
                    } else {
                        ""
                    };
                    ListValue::of(Rc::new(Entry {
                        name: (*name).to_owned(),
                        desc: format!("Description of {name}{long}"),
                        num: (i * 37.0) % 23.0,
                    }))
                })
                .collect();
            // the first ListControl constructs the first Tooltip: Classes/Tooltip.lua loads
            module_load(cx);
            let cols = cx.add(host, "cols", U12TestList::new(Anchor::NONE, rect(20, 40, 520, 200), rows));
            let icon = load_image(cx, "Assets/icon_ring_left.png");
            let plain_list = cx.add(
                host,
                "plain",
                ListControl::new(
                    Anchor::NONE,
                    rect(580, 40, 260, 170),
                    20.0,
                    true,
                    false,
                    ListData::Local(plain_rows((1..=24).map(|i| format!("Plain row {i}")))),
                    false,
                ),
            );
            let highlight: Rc<Cell<Option<usize>>> = Rc::default();
            let plain_event: Rc<RefCell<Option<String>>> = Rc::default();
            {
                let w = cx.get_mut(plain_list);
                w.show_row_separators = true;
                w.ov.get_row_value = Some(Rc::new(|_cx, _me, _c, _i, v| format!("^7{}", plain(v).label)));
                w.ov.get_row_icon =
                    Some(Rc::new(move |_cx, _me, _c, index, _v| (index % 2 == 0).then(|| icon.clone())));
                let hl = highlight.clone();
                w.ov.override_select_index = Some(Rc::new(move |cx, me, index| {
                    if cx.sys.is_key_down(Key::Shift) && cx.w::<ListControl>(me).sel_index.is_some() {
                        hl.set(Some(index));
                        return true;
                    }
                    hl.set(None);
                    false
                }));
                let hl = highlight.clone();
                w.ov.set_highlight_color = Some(Rc::new(move |cx, me, index, _v| {
                    let (Some(h), Some(sel)) = (hl.get(), cx.w::<ListControl>(me).sel_index) else {
                        return false;
                    };
                    let (h, sel, i) = (h as f64, sel as f64, index as f64);
                    if m_min(sel, h) <= i && m_max(sel, h) >= i {
                        if sel == i || h == i {
                            cx.sg.set_draw_color(1.0, 0.5, 0.0);
                        } else {
                            cx.sg.set_draw_color(1.0, 1.0, 0.0);
                        }
                        return true;
                    }
                    false
                }));
                let ev = plain_event.clone();
                w.ov.on_sel_click = Some(Rc::new(move |_cx, _me, index, _v, dbl| {
                    *ev.borrow_mut() = Some(format!("plain click {index}"));
                    dbl.then_some(false)
                }));
            }
            let no_scroll = cx.add(
                host,
                "noScroll",
                ListControl::new(
                    Anchor::NONE,
                    rect(580, 240, 260, 70),
                    16.0,
                    false,
                    false,
                    ListData::Local(plain_rows(["Fixed one".to_owned(), "Fixed two".to_owned()])),
                    false,
                ),
            );
            cx.get_mut(no_scroll).ov.get_row_value =
                Some(Rc::new(|_cx, _me, _c, _i, v| plain(v).label.clone()));
            let empty = cx.add(
                host,
                "empty",
                ListControl::new(
                    Anchor::NONE,
                    rect(20, 280, 520, 60),
                    16.0,
                    true,
                    false,
                    ListData::default(),
                    false,
                ),
            );
            cx.get_mut(empty).default_text =
                Some("^x7F7F7FThis list is empty.\nIts default text has two lines.".to_owned());
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 360, 0, 16),
                    Prop::f(move |cx, _| {
                        let c = cx.get(cols);
                        let p = cx.get(plain_list);
                        format!(
                            "^7cols: {} sel {} | plain: {} sel {} hl {}",
                            c.last_event.as_deref().unwrap_or("-"),
                            tostring_opt(c.list.sel_index),
                            plain_event.borrow().as_deref().unwrap_or("-"),
                            tostring_opt(p.sel_index),
                            tostring_opt(highlight.get()),
                        )
                    }),
                ),
            );
        },
        frame,
    )
}
