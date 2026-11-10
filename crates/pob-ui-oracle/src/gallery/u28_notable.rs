//! Gallery "u28_notable" (U28): the Rust side of `lua/gallery/u28_notable.lua`. The control is
//! pob-app's real `NotableDBControl`; [`FakeItemsTab`] is the Lua gallery's stand-in `itemsTab`;
//! `data.powerStatList` is the dump's (the Lua side runs Modules/Data.lua:129-235).
#![allow(clippy::type_complexity, clippy::semicolon_if_nothing_returned)]

use std::cell::Cell;
use std::rc::Rc;
use std::task::Poll;

use pob_app::calc::MiscCalc;
use pob_app::model::{
    gamedata::ItemBase, Item, ItemRef, LStr, LjIntMap, Node, NodeRef, Output, Override, Shared,
    Table, Val,
};
use pob_app::tabs::tree::notable_db::{set_power_stat_list, NotableDBControl, NotableItemsTab};
use pob_host::{App, PlatformParts};
use pob_lua::format::tostring_num;
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem};
use pob_ui_core::controls::{ButtonControl, LabelControl, ListControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::module_load;
use pob_ui_core::tooltip::Tooltip;

use super::_u12::frame;
use super::_u28::{power_stat_list, tostring_opt};
use super::_u9::Gallery;

/// `calcFunc(override)`: `{ FullDPS = power or 100, Life = (power or 100) / 2 }`.
struct FakeCalc;

impl MiscCalc for FakeCalc {
    fn call(&self, ov: Option<Rc<Override>>, _use_full_dps: Option<bool>) -> Poll<Output> {
        let ov = ov.expect("an override");
        let item = ov.repItem.get().expect("repItem");
        let power = item.extra.get("power").num_or(100.0);
        let out = Output::new();
        out.set("FullDPS", power);
        out.set("Life", power / 2.0);
        Poll::Ready(out)
    }
}

/// The Lua gallery's `itemsTab`.
struct FakeItemsTab {
    output_revision: Cell<f64>,
    display_item: ItemRef,
}

impl NotableItemsTab for FakeItemsTab {
    fn build_output_revision(&self, _cx: &Cx) -> f64 {
        self.output_revision.get()
    }
    fn get_misc_calculator(&self, _cx: &mut Cx) -> Rc<dyn MiscCalc> {
        Rc::new(FakeCalc)
    }
    fn display_item(&self, _cx: &Cx) -> Option<ItemRef> {
        Some(self.display_item.clone())
    }
    /// `{ power = node and node.fakePower }`
    fn anoint_item(&self, _cx: &mut Cx, node: Option<&NodeRef>) -> ItemRef {
        let item = Item::default();
        if let Some(n) = node {
            item.extra.set("power", n.extra.get("fakePower"));
        }
        Rc::new(item)
    }
    fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef) {
        tooltip.add_line(cx, 20.0, &format!("^7{}", node.dn.get()));
    }
    fn append_anoint_tooltip(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef) {
        tooltip.add_line(cx, 14.0, &format!("^7Anoint: {}", node.dn.get()));
    }
}

/// `{ list = { } }` / `{ list = { }, extra = "x" }` / `{ }`
fn mod_entry(list: bool, extra: bool) -> Table {
    let t = Table::new();
    if list {
        t.set("list", Table::new());
    }
    if extra {
        t.set("extra", "x");
    }
    t
}

fn strings(v: &[&str]) -> Table {
    let t = Table::new();
    for s in v {
        t.push(*s);
    }
    t
}

/// The gallery's node db (ids 1..8, the array part on both sides).
fn db() -> LjIntMap<NodeRef> {
    type Spec = (&'static str, &'static [&'static str], &'static [(bool, bool)], bool, &'static str, f64, Option<&'static [&'static str]>);
    let specs: [Spec; 8] = [
        ("Bravery", &["10% increased Damage", "Unsupported effect"], &[(true, false), (true, true)], true, "k1", 50.0, Some(&["Reminder one", "Reminder two"])),
        ("Arsonist", &["Fire Damage"], &[(true, false)], true, "k2", 200.0, None),
        ("Not Anointable", &["Nothing"], &[(true, false)], false, "k3", 999.0, None),
        ("Cold Heart", &[" ", "Cold Damage"], &[(false, false), (true, false)], true, "", 0.0, None),
        ("Divine Fury", &["Holy Fire"], &[(false, false)], true, "k5", f64::INFINITY, None),
        ("The Explosive", &["Area Damage"], &[(true, false)], true, "k6", 50.0, None),
        ("Arcane Focus", &["Spell Damage", "Cast Speed"], &[(true, false), (true, false)], true, "k7", 10.0, None),
        ("Heart of Ice", &["Cold Penetration"], &[(true, false)], true, "k8", 120.0, None),
    ];
    let mut db = LjIntMap::new();
    for (i, (dn, sd, mods, recipe, mod_key, power, reminder)) in specs.into_iter().enumerate() {
        let id = (i + 1) as f64;
        let n = Node {
            id,
            ..Node::default()
        };
        n.dn.set(LStr::from(dn));
        n.sd.set(Shared::new(sd.iter().map(|s| LStr::from(*s)).collect()));
        n.mods.set(Shared::new(mods.iter().map(|&(l, e)| mod_entry(l, e)).collect()));
        if recipe {
            n.extra.set("recipe", strings(&["AmberOil", "AzureOil"]));
        }
        n.modKey.set(LStr::from(mod_key));
        n.extra.set("fakePower", power);
        if let Some(r) = reminder {
            n.extra.set("reminderText", strings(r));
        }
        db.set(id as i64, Some(Rc::new(n)));
    }
    db
}

/// Lua `tostring(v)` of a number or nil.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

/// The gallery app.
pub fn app(parts: PlatformParts) -> Box<dyn App> {
    Gallery::new(
        parts,
        |cx, host| {
            set_power_stat_list(Some(power_stat_list()));
            // Main.lua:120 is the constant pob-calc carries (main:Init does not run here either)
            let base: ItemBase = serde_json::from_str(r#"{"type": "Amulet", "tags": {}, "req": {}}"#)
                .expect("an item base");
            let display_item = Item::default();
            display_item.base.set(Some(std::sync::Arc::new(base)));
            let items_tab = Rc::new(FakeItemsTab {
                output_revision: Cell::new(1.0),
                display_item: Rc::new(display_item),
            });
            // the ListControl constructs the first Tooltip: Classes/Tooltip.lua loads
            module_load(cx);
            let db_ctrl = cx.add(
                host,
                "db",
                NotableDBControl::new(
                    Anchor::NONE,
                    rect(10, 100, 360, 300),
                    items_tab.clone(),
                    db(),
                    Some("ANOINT"),
                ),
            );
            let it = items_tab.clone();
            cx.add(
                host,
                "rev",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(420, 100, 160, 20),
                    "Output revision",
                    move |_cx| it.output_revision.set(it.output_revision.get() + 1.0),
                ),
            );
            let target = cx.add(
                host,
                "target",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(420, 140, 160, 20),
                    vec![DropItem::str("Drop target")],
                    None,
                    None,
                    false,
                ),
            );
            cx.w_mut::<ListControl>(db_ctrl.id()).drag_target_list = Some(vec![target.id()]);
            cx.add(
                host,
                "status",
                LabelControl::new(
                    Anchor::NONE,
                    rect(20, 10, 0, 16),
                    Prop::f(move |cx, _| {
                        let w = cx.get(db_ctrl);
                        format!(
                            "^7sort {} max {} rev {} sel {}",
                            tostring_val(&w.sort_mode),
                            w.sort_max_power.map_or_else(|| "nil".to_owned(), tostring_num),
                            tostring_num(items_tab.output_revision.get()),
                            tostring_opt(w.list.sel_index)
                        )
                    }),
                ),
            );
        },
        frame,
    )
}
