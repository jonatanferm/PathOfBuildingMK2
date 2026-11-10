//! Gallery "u28_timeless_socket" (U28): the Rust side of `lua/gallery/u28_timeless_socket.lua`.
//! The control is pob-app's real `TimelessJewelSocketControl`; [`FakeBuild`] and [`FakeViewer`]
//! are the Lua gallery's stand-ins.
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_app::model::{ItemRef, Node, NodeRef, Table, Val};
use pob_app::tabs::tree::timeless_list::{ItemBaseRef, TimelessBuild};
use pob_app::tabs::tree::timeless_socket::{SocketViewer, TimelessJewelSocketControl};
use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_sg::{Align, Font};
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem, SelFunc};
use pob_ui_core::controls::LabelControl;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;
use pob_ui_core::tooltip::Tooltip;

use super::_u13::frame_bg;
use super::_u9::Gallery;

/// The Lua gallery's `build` (`spec.nodes`, `spec.tree.size`).
struct FakeBuild {
    nodes: Vec<(i64, NodeRef)>,
}

impl TimelessBuild for FakeBuild {
    fn timeless_data(&self, _cx: &Cx) -> Table {
        unreachable!("not read by the socket control")
    }
    fn output_revision(&self, _cx: &Cx) -> f64 {
        unreachable!()
    }
    fn spec_alloc_node(&self, _cx: &Cx, _node_id: &Val) -> bool {
        unreachable!()
    }
    fn spec_node(&self, _cx: &Cx, node_id: &Val) -> Option<NodeRef> {
        let k = node_id.opt_num().filter(|n| n.fract() == 0.0)? as i64;
        self.nodes.iter().find(|(id, _)| *id == k).map(|(_, n)| n.clone())
    }
    fn spec_tree_size(&self, _cx: &Cx) -> Val {
        Val::Num(25000.0)
    }
    fn set_build_flag(&self, _cx: &mut Cx) {
        unreachable!()
    }
    fn new_item(&self, _cx: &Cx, _raw: Option<&str>) -> ItemRef {
        unreachable!()
    }
    fn get_socket_and_jewel_for_node_id(&self, _cx: &Cx, _node_id: &Val) -> Option<Table> {
        unreachable!()
    }
    fn sockets(&self, _cx: &Cx, _node_id: &Val) -> Option<Table> {
        unreachable!()
    }
    fn add_item_stat_differences(
        &self,
        _cx: &mut Cx,
        _tooltip: &mut Tooltip,
        _item: &ItemRef,
        _base: Option<ItemBaseRef>,
        _slot: &Val,
    ) {
        unreachable!()
    }
    fn add_item(&self, _cx: &mut Cx, _item: &ItemRef, _no_auto_equip: bool) {
        unreachable!()
    }
    fn is_item_valid_for_slot(&self, _cx: &Cx, _item: &ItemRef, _slot_name: &Val) -> bool {
        unreachable!()
    }
    fn set_sel_item_id(&self, _cx: &mut Cx, _socket: &Table, _sel_item_id: Option<f64>) {
        unreachable!()
    }
    fn populate_slots(&self, _cx: &mut Cx) {
        unreachable!()
    }
    fn add_undo_state(&self, _cx: &mut Cx) {
        unreachable!()
    }
}

/// The Lua gallery's `viewer` (a PassiveTreeView stand-in).
#[derive(Default)]
struct FakeViewer {
    zoom: Cell<Option<f64>>,
    zoom_x: Cell<f64>,
    zoom_y: Cell<f64>,
}

impl SocketViewer for FakeViewer {
    fn set_zoom(&self, _cx: &mut Cx, v: f64) {
        self.zoom.set(Some(v));
    }
    fn set_zoom_x(&self, _cx: &mut Cx, v: f64) {
        self.zoom_x.set(v);
    }
    fn set_zoom_y(&self, _cx: &mut Cx, v: f64) {
        self.zoom_y.set(v);
    }
    fn draw(&self, cx: &mut Cx, vp: &ViewPort) {
        cx.sg.set_draw_color(0.2, 0.3, 0.4);
        cx.sg.draw_image(None, vp.x, vp.y, vp.width, vp.height);
        let zoom = self
            .zoom
            .get()
            .map_or_else(|| "nil".to_owned(), tostring_num);
        let text = pob_lua::lformat!(
            "^7zoom %s x %.3f y %.3f inputs %d",
            zoom.as_str(),
            self.zoom_x.get(),
            self.zoom_y.get(),
            0.0
        );
        cx.sg
            .draw_string(4.0, 4.0, Align::Left, 14.0, Font::Var, &text);
    }
}

fn node(x: f64, y: f64) -> NodeRef {
    let n = Node::default();
    n.x.set(x);
    n.y.set(y);
    Rc::new(n)
}

fn sockets() -> Vec<DropItem> {
    let entry = |label: &str, keystone: Option<&str>, id: f64| {
        let t = Table::new();
        t.set("label", label);
        if let Some(k) = keystone {
            t.set("keystone", k);
        }
        t.set("id", id);
        DropItem::new(label, Rc::new(t))
    };
    vec![
        entry("All", None, -1.0),
        entry("# Acrobatics: 26725", Some("Acrobatics"), 26725.0),
        entry("Iron Reflexes: 54127", Some("Iron Reflexes"), 54127.0),
        entry("Unknown: 999", Some("Unknown"), 999.0),
        DropItem::str("A string entry"),
    ]
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            let last_sel: Rc<RefCell<String>> = Rc::new(RefCell::new("-".to_owned()));
            let build: Rc<dyn TimelessBuild> = Rc::new(FakeBuild {
                nodes: vec![(26725, node(300.0, -1200.0)), (54127, node(-4500.5, 812.25))],
            });
            let viewer: Rc<dyn SocketViewer> = Rc::new(FakeViewer::default());
            let sel_func = |name: &'static str| -> SelFunc {
                let last_sel = last_sel.clone();
                Rc::new(move |_cx, index, value| {
                    let v = value.map_or_else(
                        || "nil".to_owned(),
                        |v| match v.get::<Table>() {
                            Some(t) => t.str("label").expect("label").to_string(),
                            None => v.as_str().expect("a string entry").to_owned(),
                        },
                    );
                    *last_sel.borrow_mut() = format!("{name} {index} {v}");
                })
            };
            // the DropDownControl's TooltipHost constructs the first Tooltip
            module_load(cx);
            let top = cx.add(
                host,
                "top",
                TimelessJewelSocketControl::new(
                    Anchor::NONE,
                    rect(40, 300, 200, 20),
                    sockets(),
                    Some(sel_func("top")),
                    build.clone(),
                    viewer.clone(),
                ),
            );
            let bottom = cx.add(
                host,
                "bottom",
                TimelessJewelSocketControl::new(
                    Anchor::NONE,
                    rect(700, 660, 200, 20),
                    sockets(),
                    Some(sel_func("bottom")),
                    build,
                    viewer,
                ),
            );
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 10, 0, 16),
                    Prop::f(move |cx, _| {
                        let t = cx.w::<DropDownControl>(top.id()).sel_index;
                        let b = cx.w::<DropDownControl>(bottom.id()).sel_index;
                        format!(
                            "^7{} | top {} bottom {}",
                            last_sel.borrow(),
                            tostring_num(t as f64),
                            tostring_num(b as f64)
                        )
                    }),
                ),
            );
        },
        frame_bg,
    )
}
