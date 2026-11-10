//! Gallery "u28_timeless_list" (U28): the Rust side of `lua/gallery/u28_timeless_list.lua`. The
//! control is pob-app's real `TimelessJewelListControl`; [`FakeBuild`] is the Lua gallery's
//! stand-in `build` (same data, same recorded calls).
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_app::model::{Item, ItemRef, LStr, NodeRef, Table, Val};
use pob_app::tabs::tree::timeless_list::{
    math_randomseed, ItemBaseRef, TimelessBuild, TimelessJewelListControl,
};
use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_ui_core::controls::{ButtonControl, LabelControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;
use pob_ui_core::tooltip::Tooltip;

use super::_u13::frame_bg;
use super::_u28::{push_event, tostring_opt};
use super::_u9::Gallery;

const TYPE_LABELS: [&str; 11] = [
    "Glorious Vanity",
    "Lethal Pride",
    "Brutal Restraint",
    "Militant Faith",
    "Elegant Hubris",
    "Heroic Tragedy",
    "Festering Vengeance",
    "Extinguishing Grasp",
    "Baleful Dominion",
    "Destructive Aspiration",
    "Reclaimed Malevolence",
];

/// The Lua gallery's `self.build`.
struct FakeBuild {
    timeless_data: Table,
    output_revision: Cell<f64>,
    alloc_nodes: Vec<i64>,
    sockets: Table,
    events: Rc<RefCell<Vec<String>>>,
    next_id: Cell<f64>,
    build_flag: Cell<Option<bool>>,
}

fn key(v: &Val) -> Option<i64> {
    v.opt_num().filter(|n| n.fract() == 0.0).map(|n| n as i64)
}

impl TimelessBuild for FakeBuild {
    fn timeless_data(&self, _cx: &Cx) -> Table {
        self.timeless_data.clone()
    }
    fn output_revision(&self, _cx: &Cx) -> f64 {
        self.output_revision.get()
    }
    fn spec_alloc_node(&self, _cx: &Cx, node_id: &Val) -> bool {
        key(node_id).is_some_and(|k| self.alloc_nodes.contains(&k))
    }
    fn spec_node(&self, _cx: &Cx, _node_id: &Val) -> Option<NodeRef> {
        None
    }
    fn spec_tree_size(&self, _cx: &Cx) -> Val {
        Val::Nil
    }
    fn set_build_flag(&self, _cx: &mut Cx) {
        self.build_flag.set(Some(true));
    }
    /// The Lua gallery's `Item` class keeps `raw`.
    fn new_item(&self, _cx: &Cx, raw: Option<&str>) -> ItemRef {
        let item = Item::default();
        item.raw.set(LStr::from(raw.expect("every jewel type has a text")));
        Rc::new(item)
    }
    fn get_socket_and_jewel_for_node_id(&self, _cx: &Cx, node_id: &Val) -> Option<Table> {
        key(node_id).and_then(|k| self.sockets.get(k).table())
    }
    fn sockets(&self, _cx: &Cx, node_id: &Val) -> Option<Table> {
        key(node_id).and_then(|k| self.sockets.get(k).table())
    }
    fn add_item_stat_differences(
        &self,
        cx: &mut Cx,
        tooltip: &mut Tooltip,
        item: &ItemRef,
        _base: Option<ItemBaseRef>,
        slot: &Val,
    ) {
        // tostring(slot and slot.slotName)
        let slot_name = match slot {
            Val::Nil => "nil".to_owned(),
            Val::Bool(false) => "false".to_owned(),
            s => s.get("slotName").str().expect("slotName").to_string(),
        };
        tooltip.add_line(cx, 14.0, &format!("^8slot {slot_name}"));
        let raw = item.raw.get();
        for line in raw.split('\n').filter(|l| !l.is_empty()) {
            tooltip.add_line(cx, 12.0, &format!("^7{line}"));
        }
    }
    fn add_item(&self, _cx: &mut Cx, item: &ItemRef, no_auto_equip: bool) {
        let id = self.next_id.get() + 1.0;
        self.next_id.set(id);
        item.id.set(Some(id));
        push_event(&self.events, format!("add {} {no_auto_equip}", tostring_num(id)));
    }
    fn is_item_valid_for_slot(&self, _cx: &Cx, _item: &ItemRef, slot_name: &Val) -> bool {
        !slot_name.raw_eq(&Val::from("Jewel 7960"))
    }
    fn set_sel_item_id(&self, _cx: &mut Cx, socket: &Table, sel_item_id: Option<f64>) {
        let name = socket.str("slotName").expect("slotName");
        let id = sel_item_id.map_or_else(|| "nil".to_owned(), tostring_num);
        push_event(&self.events, format!("sel {name} {id}"));
    }
    fn populate_slots(&self, _cx: &mut Cx) {
        push_event(&self.events, "populate".to_owned());
    }
    fn add_undo_state(&self, _cx: &mut Cx) {
        push_event(&self.events, "undo".to_owned());
    }
}

fn idt(id: f64) -> Table {
    let t = Table::new();
    t.set("id", id);
    t
}

fn names(list: &[String]) -> Table {
    let t = Table::new();
    for n in list {
        t.push(n.as_str());
    }
    t
}

fn fake_build() -> (Rc<FakeBuild>, Table) {
    let events = Rc::new(RefCell::new(Vec::new()));
    let sockets = Table::new();
    for id in [26725.0, 54127.0, 7960.0] {
        let slot = Table::new();
        slot.set("slotName", format!("Jewel {}", tostring_num(id)));
        sockets.set(id, slot);
    }
    let shared = Table::new();
    let ty = idt(1.0);
    ty.set("label", TYPE_LABELS[0]);
    shared.set("type", ty);
    shared.set("conqueror", idt(2.0));
    shared.set("devotionVariant1", idt(1.0));
    shared.set("devotionVariant2", idt(1.0));
    let socket = idt(26725.0);
    socket.set("keystone", "Acrobatics");
    socket.set("label", "Acrobatics: 26725");
    shared.set("socket", socket);
    let desired = Table::new();
    for (k, idx, name) in [("vaal_small_a", 1.0, "Small A"), ("vaal_notable_b", 2.0, "Notable B")] {
        let d = Table::new();
        d.set("desiredIdx", idx);
        d.set("displayName", name);
        desired.set(k, d);
    }
    shared.set("desiredNodes", desired);
    let rows = Table::new();
    for i in 1..=24 {
        let fi = f64::from(i);
        let seed = 1000.0 + fi * 7.0;
        let row = Table::new();
        row.set(
            "label",
            format!("^7Seed {}  weight {}", tostring_num(seed), tostring_num(fi * 1.5)),
        );
        row.set("seed", seed);
        row.set("total", if i == 5 { 0.0 } else { fi * 1.5 });
        let a = Table::new();
        a.set("targetNodeNames", names(&[format!("Name {i}"), format!("Other {i}")]));
        row.set("vaal_small_a", a);
        if i % 2 == 0 {
            let b = Table::new();
            b.set("targetNodeNames", Table::new());
            row.set("vaal_notable_b", b);
        }
        match i {
            3 => {
                row.set("socketId", 54127.0);
                row.set("socketLabel", "Unallocated: 54127");
            }
            4 => row.set("socketId", -1.0),
            6 => {
                row.set("socketId", 7960.0);
                row.set("socketLabel", "Invalid slot: 7960");
            }
            _ => {}
        }
        rows.push(row);
    }
    let td = Table::new();
    td.set("socketAllocate", true);
    td.set("searchResults", rows);
    td.set("sharedResults", shared.clone());
    let build = Rc::new(FakeBuild {
        timeless_data: td,
        output_revision: Cell::new(1.0),
        alloc_nodes: vec![26725, 7960],
        sockets,
        events,
        next_id: Cell::new(0.0),
        build_flag: Cell::new(None),
    });
    (build, shared)
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            // the oracle seeds LuaJIT's PRNG with 0 (determinism patch 2)
            math_randomseed(0.0);
            let (build, shared) = fake_build();
            // the ListControl constructs the first Tooltip: Classes/Tooltip.lua loads
            module_load(cx);
            let list = cx.add(
                host,
                "list",
                TimelessJewelListControl::new(
                    cx,
                    Anchor::NONE,
                    rect(20, 40, 360, 300),
                    build.clone(),
                ),
            );
            let (b, s) = (build.clone(), shared.clone());
            cx.add(
                host,
                "type",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(420, 40, 160, 20),
                    "Next jewel type",
                    move |_cx| {
                        let ty = s.table("type").expect("type");
                        let id = ty.num("id") % 11.0 + 1.0;
                        ty.set("id", id);
                        ty.set("label", TYPE_LABELS[id as usize - 1]);
                        s.table("conqueror").expect("conqueror").set("id", id % 3.0 + 1.0);
                        b.output_revision.set(b.output_revision.get() + 1.0);
                    },
                ),
            );
            let (b, s) = (build.clone(), shared.clone());
            cx.add(
                host,
                "devotion",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(420, 70, 160, 20),
                    "Devotion 1 = 5",
                    move |_cx| {
                        s.table("devotionVariant1").expect("dv1").set("id", 5.0);
                        b.output_revision.set(b.output_revision.get() + 1.0);
                    },
                ),
            );
            cx.add(
                host,
                "scroll",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(420, 100, 160, 20),
                    "Scroll to 15",
                    move |cx| {
                        TimelessJewelListControl::scroll_to_index(cx, list.id(), 15.0);
                    },
                ),
            );
            let (b, s) = (build.clone(), shared);
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 10, 0, 16),
                    Prop::f(move |cx, _| {
                        let sel = cx.get(list).list.sel_index;
                        let hl = cx.get(list).highlight_index;
                        let flag = b
                            .build_flag
                            .get()
                            .map_or_else(|| "nil".to_owned(), |f| f.to_string());
                        format!(
                            "^7sel {} hl {} type {} flag {flag} | {}",
                            tostring_opt(sel),
                            tostring_opt(hl),
                            tostring_num(s.table("type").expect("type").num("id")),
                            b.events.borrow().join(", ")
                        )
                    }),
                ),
            );
        },
        frame_bg,
    )
}
