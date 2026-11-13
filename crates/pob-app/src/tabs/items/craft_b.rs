//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua:3121-3992 (crafting B:
//! the Corrupt popup, the custom modifier popup, the crucible modifier popup and
//! AddItemSetTooltip). Owner: U37.
//!
//! @pob-file Classes/ItemsTab.lua @16de4b82d
//!
//! The methods are free functions over the ItemsTab node `me` (`self`), named as the Lua methods,
//! as U35's `display_item.rs`. They read the shared ItemsTab fields of U33's
//! [`ctor::State`](super::ctor::State) (`displayItem`). U33's constructor buttons and U32's set
//! list reach them through the hook tables: U37's entries of
//! [`default_items_ui_hooks`](super::ctor::default_items_ui_hooks),
//! [`default_items_hooks`](super::slot_helper::default_items_hooks) and U19's
//! `BuildHooks::items_add_item_set_tooltip` are these functions (one line each, CCR-0107 §1 /
//! CCR-0120 §1 / CCR-0126 §2). The ported methods of other units are called directly
//! (`SetDisplayItem` and `AddModComparisonTooltip(.., replaceImplicits)` of U35, CCR-0124 §1/§2);
//! `AddItemTooltip` (U38) goes through U32's `ItemsTabHooks::add_item_tooltip`, as U35 does.
//!
//! Each popup's Lua locals are one `Rc` state struct shared by the popup's closures (ui doc §7.1
//! "shared mutable upvalue"); the Lua local functions are Rust functions over that state and the
//! popup id `p` (`controls.x` is `cx.child(p, "x")`, looked up at call time).
//!
//! Lua semantics:
//! * A dropdown's `list` that Lua mutates in place (`wipeTable(control.list)` + `t_insert`,
//!   CorruptDisplayItem) or that aliases a popup local (`modList` of
//!   AddCustomModifierToDisplayItem: the dropdown is built with `modList`, which `buildMods`
//!   wipes and refills and `applySort` sorts in place) is stored into the Rust dropdown's own
//!   `list` right after each such mutation (`w.list = ..`, no `SetList`/`CheckDroppedWidth`, as
//!   Lua calls none). The dropdown reads its list only when it draws or handles input, which
//!   never happens between a Lua mutation and the store, so the alias is unobservable. The search
//!   state is left stale exactly where Lua leaves it (`UpdateSearch` only where Lua calls it).
//! * The list entries keep Lua's identity: `modList` entries are `Rc<CustomEntry>` (the
//!   dropdown's `DropItem` value is the same `Rc`, so `listMod == selected` is `Rc::ptr_eq`), mods
//!   are the data's `ItemModRef` (`SelByValue(mod, "mod")` and `mod.group` compare by `Arc`).
//! * `data.beastCraft` and `data.crucible` are untyped dump roots ([`pob_gamedata::Data::other`]);
//!   their mods are converted once to `ItemMod` (the typed affix shape every callee takes:
//!   `CanHaveMod`, `AddModComparisonTooltip`), cached process-wide so each mod has one identity.
//!   `data.essences` and `Data/DelveDropOnly.lua` / `Data/IncursionDropOnly.lua` are read the same
//!   way.
//! * `mod.type` used as a key (`[listMod.type] = true`, `modLine[mod.type]`) and in a
//!   concatenation raises on nil, as in Lua.
//! * DIVERGENCE(pending-calc): `GetMiscCalculator()` and `calcFunc(..)` only return Pending on
//!   wasm (calc facade, ui doc §10.4); a power sort then gets the value 0 for every entry. Never
//!   on desktop.
//! * The crucible list comparator (L3907-3915) indexes the `"None"` string entry (`a.affixType`,
//!   `a.defaultOrder`: the string metatable gives nil); its `nil < string` compare raises when a
//!   crucible mod without `type` meets it (5 amulet `SpellDamage` mods of the data), as in Lua.
//!
//! ORDER (ui doc §8 rule 6): every `pairs` over a string-keyed table of this range is walked in
//! canonical order, keys ascending (bytewise), resolution (a); the u37 oracle reloads these
//! functions with the same loops patched to a sorted `pairs` (`tests/u37_craft.rs` `LUA_STUBS`,
//! as u42 does for ConfigModBrowser; CCR-0129 §3 proposes a patches.rs entry). The order is
//! observable at L3151 and L3157 (`implicitList` is sorted at L3163 with ties on equal stripped
//! lines and levels, so the input order decides the dropdown order), L3662 (the PREFIX/SUFFIX
//! comparator ties on equal stat orders and levels), L3582 (the `ConPrintf` order of unknown
//! drop-restricted ids) and L3882 (`nodeSelections`: a later mod of a multi-node location
//! overwrites an earlier one; and the `"None"` entry, which compares equal to every entry, ends
//! where the sort's pivots put it). The other loops (L3642 essences, L3686 veiled, L3711 fossil,
//! L3724 necropolis, L3737 beastcraft) are followed by sorts with a total order (the data's
//! essence `type`/`tier` pairs are distinct; the other keys are the mod ids, and the veiled,
//! necropolis and beastcraft mods are all Prefix/Suffix), so for them the order is also
//! unobservable (c); the oracle patches them too. L3213 `pairs(selectedGroups)` (each entry
//! selects in its own dropdown), L3960 `pairs(nodeSelections)` and L3961 `pairs(modList[nodeId])`
//! (integer keys; each match sets its own dropdown, one match per mod id) are (c).
//!
//! Props with side effects (ui doc §7.3): none (the `shown` functions of this range only read).
#![allow(non_snake_case, non_upper_case_globals, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;
use std::sync::{Arc, OnceLock};

use pob_lua::math::{m_max, round};
use pob_lua::{lfmt, n, tostring_num, Name};
use pob_ui_core::controls::drop_down::DropTipMode;
use pob_ui_core::controls::search_host::SearchHost;
use pob_ui_core::controls::slider::SliderControl;
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::luastr::{gsub, lmatch};
use pob_ui_core::popups::{close_popup, open_popup, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;
use pob_ui_core::wrap::wrap_string;

use super::ctor::{self, PowerSortValue};
use super::slot_helper::items_hooks;
use super::ItemsTab;
use crate::app::CxApp;
use crate::calc::facade::MiscCalc;
use crate::model::{self, gamedata, Table, Val};
// CCR-0120 §3: `itemLib` is calc:IT-B's `item_tools` (not re-exported by model.rs)
use gamedata::schema::affixes::{ItemMod, ItemModRef};
use gamedata::schema::item_data::PowerStat;
use gamedata::{DataTable, DataValue};
use pob_calc::classes::item_tools::{applyRange, isModLineScalable};

/// This unit's part of the split class struct (ui doc §12.1). Every popup keeps its state in its
/// own closures (the Lua locals), so this part is empty.
#[derive(Default)]
pub struct State {}

// ---- helpers (Rust-only plumbing) ------------------------------------------------------------

fn st(cx: &Cx, me: CtrlId) -> &ctor::State {
    &cx.w::<ItemsTab>(me).ctor
}

/// `self.displayItem` that Lua then indexes (nil raises).
#[track_caller]
fn di(cx: &Cx, me: CtrlId) -> model::ItemRef {
    st(cx, me)
        .displayItem
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index field 'displayItem' (a nil value)"))
}

/// `controls[key]` of a popup, looked up at call time (nil raises when indexed).
#[track_caller]
fn ctl(cx: &Cx, p: CtrlId, key: impl Into<CtrlKey>) -> CtrlId {
    let key = key.into();
    cx.child(p, key.clone())
        .unwrap_or_else(|| lua_error!("attempt to index field '{key:?}' (a nil value)"))
}

/// `control.shown = v` (a raw boolean store).
fn set_shown(cx: &mut Cx, id: CtrlId, v: bool) {
    cx.base_mut(id).shown = Prop::Val(v);
}

/// `main.popups[1].height = h`.
#[track_caller]
fn set_popup_height(cx: &mut Cx, h: f64) {
    let Some(popup) = cx.main.popups.first().copied() else {
        lua_error!("attempt to index field '?' (a nil value)")
    };
    cx.base_mut(popup.id()).height = Prop::Val(h);
}

/// `table.concat(mod, "/")`.
fn concat_mod(r#mod: &ItemMod) -> String {
    r#mod
        .lines
        .iter()
        .map(|l| &**l)
        .collect::<Vec<&str>>()
        .join("/")
}

/// `mod.type` in a concatenation (nil raises).
#[track_caller]
fn mod_type_str(r#mod: &ItemMod) -> Name {
    r#mod
        .r#type
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'type' (a nil value)"))
}

/// `modTags = mod.modTags` (Lua shares the data table; a copy is equivalent: nothing mutates it
/// in place, see calc:IT-B Item.lua:1051, as U35's `mod_tags`).
fn mod_tags(r#mod: &ItemMod) -> Table {
    let t = Table::new();
    for tag in &r#mod.modTags {
        t.push(*tag);
    }
    t
}

/// `t[k] = true` with a string key from a Lua value (`[listMod.type] = true`; nil raises).
#[track_caller]
fn set_flag(t: &Table, k: Option<Name>) {
    let k = k.unwrap_or_else(|| lua_error!("table index is nil"));
    t.set(k, true);
}

/// `self.build.calcsTab:GetMiscCalculator()` (`None` = Pending, wasm only).
fn misc_calc(cx: &mut Cx) -> Option<Rc<dyn MiscCalc>> {
    let m = cx.build().model.clone();
    match cx.build_mut().calc.misc_calculator(&m) {
        std::task::Poll::Ready((f, _base)) => Some(f),
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => None,
    }
}

/// `calcFunc({ repSlotName = slotName, repItem = item }, useFullDPS)` (`None` = Pending).
fn calc_with(
    calcFunc: &Rc<dyn MiscCalc>,
    slotName: Name,
    item: &model::ItemRef,
    useFullDPS: bool,
) -> Option<model::Output> {
    let ov = model::Override::default();
    ov.repSlotName.set(Some(slotName));
    ov.repItem.set(Some(item.clone()));
    match calcFunc.call(Some(Rc::new(ov)), Some(useFullDPS)) {
        std::task::Poll::Ready(o) => Some(o),
        // DIVERGENCE(pending-calc)
        std::task::Poll::Pending => None,
    }
}

/// `data.powerStatList.GetFromOutput(output, sortStats[stat])` (a nil entry raises in
/// GetFromOutput's `statTable.stat`). DIVERGENCE(pending-calc): a Pending calc gives 0.
#[track_caller]
fn get_from_output(
    output: Option<model::Output>,
    sortStats: &indexmap::IndexMap<Name, &'static PowerStat>,
    stat: Name,
) -> f64 {
    let entry = sortStats
        .get(&stat)
        .unwrap_or_else(|| lua_error!("attempt to index local 'statTable' (a nil value)"));
    output.map_or(0.0, |o| {
        pob_calc::modules::power::GetFromOutput(
            &o,
            &pob_calc::modules::power::powerStat_table(entry),
            false,
        )
    })
}

/// `self.build.spec.nodes` for `checkLineForAllocates`.
fn check_line(cx: &Cx, line: &str) -> String {
    let spec = cx.build().model.spec.get();
    spec.nodes
        .with(|nodes| checkLineForAllocates(line, Some(nodes)))
}

/// lua: ItemsTab.lua:2493-2501 `checkLineForAllocates(line, nodes)` (a module local, U35's
/// range: `display_item.rs` ports it as a private fn). // CCR-0129 §1: a statement-for-statement
/// copy until U35 makes its port `pub` (no marker here, U35's covers the lines).
fn checkLineForAllocates(line: &str, nodes: Option<&model::LjIntMap<model::NodeRef>>) -> String {
    // L2494
    if let Some(nodes) = nodes.filter(|_| lmatch(line, "Allocates").is_some()) {
        // L2495
        let nodeId = lmatch(line, "%d+").and_then(|s| pob_lua::tonumber(&s));
        // L2496
        let node = nodeId
            .filter(|x| x.fract() == 0.0)
            .and_then(|x| nodes.get(x as i64));
        if let Some(node) = node {
            // L2497
            return format!("Allocates {}", node.dn.get());
        }
    }
    // L2500
    line.to_owned()
}

/// The string keys of an affix table in canonical order (bytewise; ORDER (a), module notes).
fn sorted_affixes(t: &'static gamedata::schema::affixes::AffixTable) -> Vec<(Name, ItemModRef)> {
    let mut v: Vec<(Name, ItemModRef)> = t.0.iter().map(|(k, m)| (*k, m.clone())).collect();
    v.sort_by(|a, b| a.0.as_str().as_bytes().cmp(b.0.as_str().as_bytes()));
    v
}

/// `self.displayItem.affixes` (pairs/index of a nil table raise).
#[track_caller]
fn affixes(item: &model::ItemRef) -> &'static gamedata::schema::affixes::AffixTable {
    item.affixes_table()
        .unwrap_or_else(|| lua_error!("attempt to index field 'affixes' (a nil value)"))
}

/// `data.itemMods[category]`.
#[track_caller]
fn item_mods(category: &str) -> &'static gamedata::schema::affixes::AffixTable {
    model::data()
        .itemMods
        .get(&Name::from(category))
        .unwrap_or_else(|| lua_error!("attempt to index field '{category}' (a nil value)"))
}

/// An untyped dump root (`data.crucible`, `files["Data/DelveDropOnly.lua"]`, ...).
fn dyn_root(root: &str) -> Option<&'static Arc<DataTable>> {
    let d = model::data();
    d.other
        .get(root)
        .map(|v| d.resolve(v))
        .and_then(DataValue::table)
}

/// The string-keyed members of a dump table, keys ascending (bytewise; the dump's own order).
fn sorted_fields(t: &'static DataTable) -> Vec<(&'static str, &'static DataTable)> {
    let d = model::data();
    let mut v: Vec<(&'static str, &'static DataTable)> = t
        .fields
        .iter()
        .filter_map(|(k, v)| d.resolve(v).table().map(|t| (&**k, &**t)))
        .collect();
    v.sort_by(|a, b| a.0.as_bytes().cmp(b.0.as_bytes()));
    v
}

fn dv_str(v: &'static DataValue) -> Option<&'static str> {
    match model::data().resolve(v) {
        DataValue::Str(s) => Some(&**s),
        _ => None,
    }
}

fn dv_names(v: &'static DataValue) -> Vec<Name> {
    match model::data().resolve(v) {
        DataValue::Table(t) => t.seq.iter().filter_map(dv_str).map(Name::from).collect(),
        _ => Vec::new(),
    }
}

fn dv_nums(v: &'static DataValue) -> Vec<f64> {
    match model::data().resolve(v) {
        DataValue::Table(t) => t
            .seq
            .iter()
            .filter_map(|x| model::data().resolve(x).num())
            .collect(),
        _ => Vec::new(),
    }
}

/// A dumped affix-shaped mod table (`data.beastCraft[id]`, `data.crucible[id]`) as the typed
/// `ItemMod` (module notes).
fn item_mod_of(t: &'static DataTable) -> ItemMod {
    let d = model::data();
    let s = |k: &'static str| dv_str(t.field(k));
    ItemMod {
        lines: t
            .seq
            .iter()
            .filter_map(dv_str)
            .map(gamedata::AStr::from)
            .collect(),
        r#type: s("type").map(Name::from),
        affix: s("affix").map(gamedata::AStr::from),
        group: s("group").map(Name::from),
        level: d.resolve(t.field("level")).num(),
        modTags: dv_names(t.field("modTags")),
        statOrder: dv_nums(t.field("statOrder")),
        tags: dv_names(t.field("tags")),
        tradeHashes: BTreeMap::new(),
        weightKey: dv_names(t.field("weightKey")),
        weightVal: dv_nums(t.field("weightVal")),
        weightMultiplierKey: dv_names(t.field("weightMultiplierKey")),
        weightMultiplierVal: dv_nums(t.field("weightMultiplierVal")),
        types: None,
    }
}

/// `data.beastCraft` (mod id → mod), keys ascending.
fn beast_craft() -> &'static [(Name, ItemModRef)] {
    static C: OnceLock<Vec<(Name, ItemModRef)>> = OnceLock::new();
    C.get_or_init(|| {
        dyn_root("data.beastCraft").map_or_else(Vec::new, |t| {
            sorted_fields(t)
                .into_iter()
                .map(|(k, m)| (Name::from(k), Arc::new(item_mod_of(m))))
                .collect()
        })
    })
}

/// One `data.crucible[id]` (`nodeLocation` and `tier` are crucible-only fields).
pub struct CrucibleMod {
    pub r#mod: ItemModRef,
    pub tier: Option<f64>,
    pub nodeLocation: Vec<f64>,
}

/// `data.crucible` (mod id → mod), keys ascending.
fn crucible() -> &'static [(Name, CrucibleMod)] {
    static C: OnceLock<Vec<(Name, CrucibleMod)>> = OnceLock::new();
    C.get_or_init(|| {
        dyn_root("data.crucible").map_or_else(Vec::new, |t| {
            sorted_fields(t)
                .into_iter()
                .map(|(k, m)| {
                    (
                        Name::from(k),
                        CrucibleMod {
                            r#mod: Arc::new(item_mod_of(m)),
                            tier: model::data().resolve(m.field("tier")).num(),
                            nodeLocation: dv_nums(m.field("nodeLocation")),
                        },
                    )
                })
                .collect()
        })
    })
}

/// One `data.essences[id]`.
struct Essence {
    name: Option<&'static str>,
    tier: Option<f64>,
    r#type: Option<f64>,
    /// `essence.mods[itemType]`
    mods: Vec<(&'static str, &'static str)>,
}

/// `data.essences`, keys ascending.
fn essences() -> &'static [Essence] {
    static C: OnceLock<Vec<Essence>> = OnceLock::new();
    C.get_or_init(|| {
        let d = model::data();
        dyn_root("data.essences").map_or_else(Vec::new, |t| {
            sorted_fields(t)
                .into_iter()
                .map(|(_, e)| Essence {
                    name: dv_str(e.field("name")),
                    tier: d.resolve(e.field("tier")).num(),
                    r#type: d.resolve(e.field("type")).num(),
                    mods: match d.resolve(e.field("mods")) {
                        DataValue::Table(m) => m
                            .fields
                            .iter()
                            .filter_map(|(k, v)| dv_str(v).map(|v| (&**k, v)))
                            .collect(),
                        _ => Vec::new(),
                    },
                })
                .collect()
        })
    })
}

/// `require("Data.DelveDropOnly")` / `require("Data.IncursionDropOnly")`: mod id → categories,
/// keys ascending.
fn drop_only(file: &str) -> Vec<(&'static str, Vec<&'static str>)> {
    dyn_root(&format!("files[\"Data/{file}.lua\"]")).map_or_else(Vec::new, |t| {
        sorted_fields(t)
            .into_iter()
            .map(|(k, e)| {
                let cats = match model::data().resolve(e.field("categories")) {
                    DataValue::Table(c) => c.seq.iter().filter_map(dv_str).collect(),
                    _ => Vec::new(),
                };
                (k, cats)
            })
            .collect()
    })
}

// @pob Classes/ItemsTab.lua:3513-3514 h=b4df3906
// (`local delveDropOnlyCategories = require("Data.DelveDropOnly")`, `local
// incursionDropOnlyCategories = require("Data.IncursionDropOnly")`: the dump's file roots, read
// by `drop_only` at the use sites)

/// The `sortList` entry's `stat` (`value.stat`).
fn sort_stat(v: Option<&DropItem>) -> Option<Name> {
    v.and_then(|v| v.get::<PowerSortValue>())
        .and_then(PowerSortValue::stat)
}

/// `controls.sort.list[controls.sort.selIndex].stat` (a nil entry raises).
#[track_caller]
fn sel_sort_stat(cx: &Cx, sort: CtrlId) -> Option<Name> {
    DropDownControl::get_sel_value_by_key(cx, sort, |v| sort_stat(Some(v)))
}

fn update_search(cx: &mut Cx, drop: CtrlId) {
    SearchHost::update_search(cx, drop, |cx, me| {
        &mut cx.w_mut::<DropDownControl>(me).search
    });
}

// ---- CorruptDisplayItem -----------------------------------------------------------------------

/// One `implicitList[modType]` entry (`{ mod = mod }`, plus the sort fields).
struct ImplicitEntry {
    r#mod: ItemModRef,
    defaultSortOrder: Cell<Option<f64>>,
    sortValue: Cell<Option<f64>>,
    sortValues: RefCell<Option<Vec<(Name, f64)>>>,
}

/// The value of an `implicit<i>` dropdown entry (`{ label = .., mod = mod }`; the `{ label =
/// "None" }` entry has no `mod`).
struct ImplicitValue {
    r#mod: Option<ItemModRef>,
}

/// The locals of CorruptDisplayItem shared by its local functions and closures.
struct Corrupt {
    me: CtrlId,
    implicitList: RefCell<indexmap::IndexMap<String, Vec<Rc<ImplicitEntry>>>>,
    shownExplicits: RefCell<Vec<usize>>,
    explicitOffset: Cell<f64>,
    corruptedRanges: RefCell<BTreeMap<usize, f64>>,
    sortStats: indexmap::IndexMap<Name, &'static PowerStat>,
    itemMaxCorruptImplicits: f64,
    currentModType: RefCell<String>,
}

/// the amount of controls created (`maxImplicitNum`, L3144)
const maxImplicitNum: usize = 5;

fn implicit_key(i: usize) -> String {
    format!("implicit{i}")
}

#[allow(clippy::option_option)] // nil entry vs the "None" entry (no `mod`)
/// `control.list[index].mod` of an implicit dropdown (`None`: no entry; `Some(None)`: the
/// "None" entry).
fn implicit_list_mod(cx: &Cx, drop: CtrlId, index: usize) -> Option<Option<ItemModRef>> {
    let w = cx.w::<DropDownControl>(drop);
    let e = index.checked_sub(1).and_then(|k| w.list.get(k))?;
    Some(e.get::<ImplicitValue>().and_then(|v| v.r#mod.clone()))
}

/// `control:SelByValue(mod, "mod")` (a false value matches nothing; identity compare).
fn sel_by_mod(cx: &mut Cx, drop: CtrlId, value: Option<&ItemModRef>) {
    let Some(value) = value else { return };
    DropDownControl::sel_by_value(cx, drop, &|it| {
        it.get::<ImplicitValue>()
            .and_then(|v| v.r#mod.as_ref())
            .is_some_and(|m| Arc::ptr_eq(m, value))
    });
}

fn implicit_item(r#mod: &ItemModRef) -> DropItem {
    DropItem::new(
        concat_mod(r#mod),
        Rc::new(ImplicitValue {
            r#mod: Some(r#mod.clone()),
        }),
    )
}

fn none_item() -> DropItem {
    DropItem::new("None", Rc::new(ImplicitValue { r#mod: None }))
}

/// `a.mod[1]:lower():gsub("%(.-%)","$"):gsub("[%+%-%%]",""):gsub("%d+","$")` (L3164/L3165).
#[track_caller]
fn stripped_first_line(r#mod: &ItemMod) -> String {
    let first = r#mod
        .lines
        .first()
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let s = first.to_ascii_lowercase();
    let s = gsub(&s, "%(.-%)", "$");
    let s = gsub(&s, "[%+%-%%]", "");
    gsub(&s, "%d+", "$")
}

#[track_caller]
fn level_of(r#mod: &ItemMod) -> f64 {
    r#mod
        .level
        .unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
}

/// `local function buildImplicitList(modType)` (L3145-3175).
fn buildImplicitList(cx: &Cx, l: &Corrupt, modType: &str) {
    // L3146
    if l.implicitList.borrow().contains_key(modType) {
        // L3147
        return;
    }
    // L3149
    let mut list: Vec<Rc<ImplicitEntry>> = Vec::new();
    let new_entry = |r#mod: ItemModRef| {
        Rc::new(ImplicitEntry {
            r#mod,
            defaultSortOrder: Cell::new(None),
            sortValue: Cell::new(None),
            sortValues: RefCell::new(None),
        })
    };
    // L3150
    if modType == "Glimpse of Chaos" {
        // L3151 ORDER: (a), module notes
        for (modId, r#mod) in sorted_affixes(item_mods("ItemExclusive")) {
            // L3152
            if modId.as_str().starts_with("UniqueSpecialCorruption") {
                // L3153
                list.push(new_entry(r#mod));
            }
        }
    } else {
        let item = di(cx, l.me);
        // L3157 ORDER: (a), module notes
        for (_modId, r#mod) in sorted_affixes(affixes(&item)) {
            // L3158
            if r#mod.r#type.is_some_and(|t| t.as_str() == modType)
                && item.GetModSpawnWeight(&r#mod, None, None, None) > 0.0
            {
                // L3159
                list.push(new_entry(r#mod));
            }
        }
    }
    // L3163
    pob_lua::sort(&mut list, |a, b| {
        // L3164
        let an = stripped_first_line(&a.r#mod);
        // L3165
        let bn = stripped_first_line(&b.r#mod);
        // L3166
        if an != bn {
            // L3167
            an < bn
        } else {
            // L3169
            level_of(&a.r#mod) < level_of(&b.r#mod)
        }
    });
    // L3172
    for (index, entry) in list.iter().enumerate() {
        // L3173
        entry.defaultSortOrder.set(Some((index + 1) as f64));
    }
    l.implicitList.borrow_mut().insert(modType.to_owned(), list);
}

/// `implicitList[modType]` for an `ipairs` (nil raises).
#[track_caller]
fn implicit_list(l: &Corrupt, modType: &str) -> Vec<Rc<ImplicitEntry>> {
    l.implicitList
        .borrow()
        .get(modType)
        .cloned()
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"))
}

/// `local function buildScourgeList(control, other, modType)` (L3177-3189).
fn buildScourgeList(cx: &mut Cx, l: &Corrupt, control: CtrlId, other: CtrlId, modType: &str) {
    // L3178
    let selfMod = {
        let sel = cx.w::<DropDownControl>(control).sel_index;
        if sel > 1 {
            implicit_list_mod(cx, control, sel)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        } else {
            None
        }
    };
    // L3179
    let otherMod = {
        let sel = cx.w::<DropDownControl>(other).sel_index;
        if sel > 1 {
            implicit_list_mod(cx, other, sel)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        } else {
            None
        }
    };
    // L3180 wipeTable(control.list) / L3181
    let mut list = vec![none_item()];
    // L3182
    for entry in implicit_list(l, modType) {
        // L3183
        let r#mod = &entry.r#mod;
        // L3184
        if otherMod.as_ref().map_or(true, |o| r#mod.group != o.group) {
            // L3185
            list.push(implicit_item(r#mod));
        }
    }
    cx.w_mut::<DropDownControl>(control).list = list;
    // L3188
    sel_by_mod(cx, control, selfMod.as_ref());
}

/// `local function buildCorruptLists(modType)` (L3190-3218): avoid letting the user select the
/// same implicit twice.
fn buildCorruptLists(cx: &mut Cx, l: &Corrupt, p: CtrlId, modType: &str) {
    // L3192
    let mut selectedGroups: indexmap::IndexMap<Name, (usize, ItemModRef)> =
        indexmap::IndexMap::new();
    // L3193
    for i in 1..=maxImplicitNum {
        // L3195
        let control = ctl(cx, p, implicit_key(i));
        // L3196
        let sel = cx.w::<DropDownControl>(control).sel_index;
        let selected = implicit_list_mod(cx, control, sel);
        // L3197
        if let Some(Some(r#mod)) = selected {
            // L3198
            let group = r#mod
                .group
                .unwrap_or_else(|| lua_error!("table index is nil"));
            selectedGroups.insert(group, (i, r#mod));
        }
    }
    // L3201
    for i in 1..=maxImplicitNum {
        // L3203
        let control = ctl(cx, p, implicit_key(i));
        // L3204 wipeTable(control.list) / L3205
        let mut list = vec![none_item()];
        // L3206
        for entry in implicit_list(l, modType) {
            // L3207
            let r#mod = &entry.r#mod;
            // L3208 (`selectedGroups[nil]` reads nil)
            let sg = r#mod.group.and_then(|g| selectedGroups.get(&g));
            if sg.map_or(true, |(idx, _)| *idx == i) {
                // L3209
                list.push(implicit_item(r#mod));
            }
        }
        cx.w_mut::<DropDownControl>(control).list = list;
    }
    // L3213 ORDER: (c), module notes
    for (_, (idx, val)) in selectedGroups {
        // L3215
        let control = ctl(cx, p, implicit_key(idx));
        // L3216
        sel_by_mod(cx, control, Some(&val));
    }
}

/// `local function getSortValue(entry, modType, stat, calcFunc, slotName, useFullDPS)`
/// (L3219-3243).
fn corrupt_get_sort_value(
    cx: &mut Cx,
    l: &Corrupt,
    entry: &ImplicitEntry,
    modType: &str,
    stat: Name,
    calcFunc: Option<&Rc<dyn MiscCalc>>,
    slotName: Name,
    useFullDPS: bool,
) -> f64 {
    // L3220
    if entry.sortValues.borrow().is_none() {
        *entry.sortValues.borrow_mut() = Some(Vec::new());
    }
    // L3221
    let cached = entry
        .sortValues
        .borrow()
        .as_ref()
        .and_then(|v| v.iter().find(|(k, _)| *k == stat).map(|(_, v)| *v));
    if let Some(v) = cached {
        // L3222
        return v;
    }
    let displayItem = di(cx, l.me);
    // L3224
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L3225
    item.id.set(displayItem.id.get());
    // L3226
    item.corrupted.set(true);
    // L3227
    let r#mod = &entry.r#mod;
    // L3228
    let targetLines = if modType == "ScourgeUpside" || modType == "ScourgeDownside" {
        item.scourgeModLines.get()
    } else {
        item.implicitModLines.get()
    };
    // L3229
    targetLines.wipe();
    let scourge = l.currentModType.borrow().as_str() == "ScourgeUpside";
    // L3230
    for modLine in &r#mod.lines {
        // L3231
        let modLine = format!("{}{modLine}", if scourge { "{scourge}" } else { "" });
        let t = Table::new();
        // L3232
        if r#mod.modTags.is_empty() {
            // L3235
            t.set(n!("line"), modLine);
        } else {
            // L3233
            let tags = r#mod
                .modTags
                .iter()
                .map(Name::as_str)
                .collect::<Vec<_>>()
                .join(",");
            t.set(n!("line"), format!("{{tags:{tags}}}{modLine}"));
        }
        targetLines.push(t);
    }
    // L3238
    item.BuildAndParseRaw();
    // L3239
    let calcFunc =
        calcFunc.unwrap_or_else(|| lua_error!("attempt to call local 'calcFunc' (a nil value)"));
    let output = calc_with(calcFunc, slotName, &item, useFullDPS);
    // L3240
    let value = get_from_output(output, &l.sortStats, stat);
    // L3241
    if let Some(v) = entry.sortValues.borrow_mut().as_mut() {
        v.push((stat, value));
    }
    // L3242
    value
}

/// `local function sortModType(modType, stat, calcFunc, slotName, useFullDPS)` (L3244-3263).
fn sortModType(
    cx: &mut Cx,
    l: &Corrupt,
    modType: &str,
    stat: Option<Name>,
    calcFunc: Option<&Rc<dyn MiscCalc>>,
    slotName: Name,
    useFullDPS: bool,
) {
    // L3245
    let Some(mut list) = l.implicitList.borrow().get(modType).cloned() else {
        // L3246
        return;
    };
    // L3248
    if let Some(stat) = stat {
        // L3249
        for entry in &list {
            // L3250
            let v =
                corrupt_get_sort_value(cx, l, entry, modType, stat, calcFunc, slotName, useFullDPS);
            entry.sortValue.set(Some(v));
        }
        // L3252
        pob_lua::sort(&mut list, |a, b| {
            let (av, bv) = (sort_value(a.sortValue.get()), sort_value(b.sortValue.get()));
            // L3253
            if av != bv {
                // L3254
                return av > bv;
            }
            // L3256
            a.defaultSortOrder.get().unwrap_or(0.0) < b.defaultSortOrder.get().unwrap_or(0.0)
        });
    } else {
        // L3259
        pob_lua::sort(&mut list, |a, b| {
            // L3260
            a.defaultSortOrder.get().unwrap_or(0.0) < b.defaultSortOrder.get().unwrap_or(0.0)
        });
    }
    l.implicitList.borrow_mut().insert(modType.to_owned(), list);
}

/// `a.sortValue` in a comparison (set for every entry before the sort).
#[track_caller]
fn sort_value(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
}

/// `local function applySort(stat)` (L3264-3289).
fn corrupt_apply_sort(cx: &mut Cx, l: &Corrupt, p: CtrlId, stat: Option<Name>) {
    // L3265
    if cx.child(p, "implicit1").is_none() {
        // L3266
        return;
    }
    // L3268
    let slotName = di(cx, l.me).GetPrimarySlot();
    // L3269
    let calcFunc = if stat.is_some() { misc_calc(cx) } else { None };
    // L3270
    let useFullDPS = stat.is_some_and(|s| s.as_str() == "FullDPS");
    let current = l.currentModType.borrow().clone();
    // L3271
    if current != "ScourgeUpside" {
        // L3272
        sortModType(
            cx,
            l,
            &current,
            stat,
            calcFunc.as_ref(),
            slotName,
            useFullDPS,
        );
    } else {
        // L3274
        sortModType(
            cx,
            l,
            "ScourgeUpside",
            stat,
            calcFunc.as_ref(),
            slotName,
            useFullDPS,
        );
        // L3275
        sortModType(
            cx,
            l,
            "ScourgeDownside",
            stat,
            calcFunc.as_ref(),
            slotName,
            useFullDPS,
        );
    }
    // L3277
    if current != "ScourgeUpside" {
        // L3278
        buildCorruptLists(cx, l, p, &current);
    } else {
        let c = |cx: &Cx, i: usize| ctl(cx, p, implicit_key(i));
        // L3280
        let (a, b) = (c(cx, 1), c(cx, 2));
        buildScourgeList(cx, l, a, b, "ScourgeUpside");
        // L3281
        buildScourgeList(cx, l, b, a, "ScourgeUpside");
        // L3282
        let (a, b) = (c(cx, 3), c(cx, 4));
        buildScourgeList(cx, l, a, b, "ScourgeDownside");
        // L3283
        buildScourgeList(cx, l, b, a, "ScourgeDownside");
    }
    // L3285
    for i in 1..=maxImplicitNum {
        // L3286
        let control = ctl(cx, p, implicit_key(i));
        // L3287
        update_search(cx, control);
    }
}

/// `local function corruptItem(addingImplicits)` (L3290-3343).
fn corruptItem(cx: &mut Cx, l: &Corrupt, p: CtrlId, addingImplicits: bool) -> model::ItemRef {
    let displayItem = di(cx, l.me);
    // L3291
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L3292
    item.id.set(displayItem.id.get());
    // L3293
    item.corrupted.set(true);
    // L3294
    let mut specialCorruptLine: Option<Table> = None;
    let current = l.currentModType.borrow().clone();
    // L3295
    if current != "ScourgeUpside" {
        // L3296
        for modLine in displayItem.implicitModLines.get().snapshot() {
            // L3297
            for r#mod in mod_list_of(&modLine) {
                // L3298
                if r#mod == n!("CorruptImplicitCount") {
                    // L3299
                    specialCorruptLine = Some(modLine.clone());
                }
            }
        }
    }
    // either add implicits or roll ranges. if in the future it is possible
    // to double corrupt an item, this needs to be changed to not remove
    // implicits, like in pob2
    // L3307
    if addingImplicits {
        // L3308
        let newImplicit: Vec<Table> = {
            let mut v = Vec::new();
            // L3309
            for i in 1..=maxImplicitNum {
                // L3310
                let control = ctl(cx, p, implicit_key(i));
                let sel = cx.w::<DropDownControl>(control).sel_index;
                // L3311
                if sel > 1 {
                    // L3312
                    let r#mod = implicit_list_mod(cx, control, sel)
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                        .unwrap_or_else(|| {
                            lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
                        });
                    // L3313
                    for modLine in &r#mod.lines {
                        // L3314
                        let modLine = format!(
                            "{}{modLine}",
                            if current == "ScourgeUpside" {
                                "{scourge}"
                            } else {
                                ""
                            }
                        );
                        let t = Table::new();
                        // L3315
                        if r#mod.modTags.is_empty() {
                            // L3318
                            t.set(n!("line"), modLine);
                        } else {
                            // L3316
                            let tags = r#mod
                                .modTags
                                .iter()
                                .map(Name::as_str)
                                .collect::<Vec<_>>()
                                .join(",");
                            t.set(n!("line"), format!("{{tags:{tags}}}{modLine}"));
                        }
                        v.push(t);
                    }
                }
            }
            v
        };
        // L3323
        if !newImplicit.is_empty() {
            let target = |item: &model::ItemRef| {
                if current != "ScourgeUpside" {
                    item.implicitModLines.get()
                } else {
                    item.scourgeModLines.get()
                }
            };
            // L3324
            target(&item).wipe();
            // L3325
            if let Some(line) = &specialCorruptLine {
                // L3326
                item.implicitModLines.get().push(line.clone());
            }
            // L3328
            for implicit in newImplicit {
                // L3329
                target(&item).push(implicit);
            }
        }
    }
    // L3333
    if !addingImplicits {
        // L3334
        for (i, modLine) in item
            .explicitModLines
            .get()
            .snapshot()
            .into_iter()
            .enumerate()
        {
            // L3335
            let corruptedRange = l.corruptedRanges.borrow().get(&(i + 1)).copied();
            // L3336
            if let Some(corruptedRange) = corruptedRange {
                // L3337
                modLine.set(
                    n!("corruptedRange"),
                    if corruptedRange != 1.0 {
                        Val::Num(corruptedRange)
                    } else {
                        Val::Nil
                    },
                );
            }
        }
    }
    // L3341
    item.BuildAndParseRaw();
    // L3342
    item
}

/// `modLine.modList or {}`: the names of its mods (`mod.name`; the parsed lines' `modList` is a
/// list of mods).
fn mod_list_of(modLine: &Table) -> Vec<Name> {
    match modLine.get(n!("modList")) {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, v)| match v {
                Val::Mod(m) => m.name,
                other => lua_error!(
                    "attempt to index local 'mod' (a {} value)",
                    other.lua_type()
                ),
            })
            .collect(),
        _ => Vec::new(),
    }
}

/// `local function setImplicitControlsShown(implicitNum, canChangeImplicits)` (L3379-3386).
fn setImplicitControlsShown(cx: &mut Cx, p: CtrlId, implicitNum: f64, canChangeImplicits: bool) {
    // L3380
    for i in 1..=maxImplicitNum {
        // L3381
        let shown = canChangeImplicits && (i as f64) <= implicitNum;
        // L3382
        let c = ctl(cx, p, implicit_key(i));
        set_shown(cx, c, shown);
        // L3383
        let c = ctl(cx, p, format!("implicit{i}Label"));
        set_shown(cx, c, shown);
    }
    // L3385
    let c = ctl(cx, p, "implicitCannotBeChangedLabel");
    set_shown(cx, c, implicitNum > 0.0 && !canChangeImplicits);
}

/// `self.displayItem.rarity == "UNIQUE" or self.displayItem.rarity == "RELIC"`.
fn is_unique_rarity(item: &model::ItemRef) -> bool {
    let r = item.rarity.get();
    &*r == "UNIQUE" || &*r == "RELIC"
}

/// Opens the item corrupting popup
/// lua: ItemsTab.lua:3121-3511 `ItemsTabClass:CorruptDisplayItem()`.
/// @pob Classes/ItemsTab.lua:3121-3511 h=c95b5b45
pub fn CorruptDisplayItem(cx: &mut Cx, me: CtrlId) {
    let displayItem = di(cx, me);
    // L3122 local controls = { } (the popup's children)
    // L3123-3126 (the state below)
    // L3127
    let mut sourceList: Vec<&'static str> = vec!["Corrupted", "Scourge"];
    // L3128
    let (sortList, sortStats) = ctor::buildModSortList();
    // L3129
    let mut itemMaxCorruptImplicits = match displayItem.extra.get(n!("corruptImplicitCount")) {
        Val::Nil | Val::Bool(false) => 2.0,
        Val::Num(x) => x,
        other => lua_error!(
            "attempt to perform arithmetic on local 'itemMaxCorruptImplicits' (a {} value)",
            other.lua_type()
        ),
    };
    // L3130
    for modLine in displayItem.implicitModLines.get().snapshot() {
        // L3131
        for r#mod in mod_list_of(&modLine) {
            // the vestigial implicit modifier doesn't go away when corrupting items
            // L3133
            if r#mod == n!("CorruptImplicitCount") {
                // L3134
                itemMaxCorruptImplicits -= 1.0;
            }
        }
    }
    // L3138 (`find("glimpse of chaos")`: no magic characters, a plain substring search)
    let isGlimpse = displayItem.title.get().is_some_and(|t| {
        t.to_string()
            .to_ascii_lowercase()
            .contains("glimpse of chaos")
    }) && displayItem.extra.truthy(n!("isUnique"));
    // L3139
    if isGlimpse {
        // L3140
        sourceList.insert(0, "Glimpse of Chaos");
    }
    // L3142
    let currentModType = sourceList[0].to_owned();
    // the amount of controls created
    // L3144 (`maxImplicitNum`, a const)
    let l = Rc::new(Corrupt {
        me,
        implicitList: RefCell::new(indexmap::IndexMap::new()),
        shownExplicits: RefCell::new(Vec::new()),
        explicitOffset: Cell::new(0.0),
        corruptedRanges: RefCell::new(BTreeMap::new()),
        sortStats,
        itemMaxCorruptImplicits,
        currentModType: RefCell::new(currentModType.clone()),
    });
    // L3145-3175 (buildImplicitList)
    // L3176
    buildImplicitList(cx, &l, &currentModType);
    // L3177-3343 (buildScourgeList, buildCorruptLists, getSortValue, sortModType, applySort,
    // corruptItem)

    // L3510
    open_popup(
        cx,
        605.0,
        103.0 + 20.0 * itemMaxCorruptImplicits,
        "Corrupt Item",
        move |cx, p| {
            let p = p.id();
            // L3344
            if is_unique_rarity(&di(cx, me)) {
                // L3345
                let item = crate::main_mode::new_item(Some(&di(cx, me).BuildRaw()), None, false);
                // L3346
                let mut offset = 20.0;
                // L3347
                for (i0, r#mod) in item
                    .explicitModLines
                    .get()
                    .snapshot()
                    .into_iter()
                    .enumerate()
                {
                    let i = i0 + 1;
                    // L3348
                    let modRange = match r#mod.get(n!("range")) {
                        v if v.truthy() => v,
                        _ => cx
                            .app()
                            .main
                            .default_item_affix_quality
                            .map_or(Val::Nil, Val::Num),
                    };
                    let line = r#mod
                        .str(n!("line"))
                        .map(|s| s.to_string())
                        .unwrap_or_else(|| {
                            lua_error!("bad argument #1 to 'applyRange' (string expected, got nil)")
                        });
                    let valueScalar = r#mod.opt_num(n!("valueScalar"));
                    // L3349
                    if isModLineScalable(&line, &modRange, valueScalar)
                        && item.CheckModLineVariant(&r#mod)
                    {
                        // L3350-3354
                        let formatLabel = {
                            let (line, modRange) = (line.clone(), modRange.clone());
                            Rc::new(move |cx: &Cx, corruptedRange: f64| -> (String, usize) {
                                // L3351
                                let line = applyRange(
                                    &line,
                                    &modRange,
                                    Some(valueScalar.unwrap_or(1.0)),
                                    Some(corruptedRange),
                                );
                                // L3352
                                let lines = wrap_string(cx, &format!("^7{line}"), 16.0, 430.0);
                                // L3353
                                (lines.join("\n"), lines.len())
                            })
                        };
                        // L3355
                        let value = cx
                            .add(
                                p,
                                format!("rollRangeValue{i}"),
                                LabelControl::new(
                                    Anchor::nil(TopLeft, TopLeft),
                                    rect(10, 10.0 + offset, 200, 16),
                                    "^71.00",
                                ),
                            )
                            .id();
                        // L3357
                        let (l2, fl) = (l.clone(), formatLabel.clone());
                        let slider = cx
                            .add(
                                p,
                                format!("rollRangeSlider{i}"),
                                SliderControl::new(
                                    Anchor::new(Left, value, Right),
                                    rect(5, 0, 80, 18),
                                    Some(Rc::new(move |cx: &mut Cx, val: f64| {
                                        // L3358 0.78-1.22
                                        let r = 0.78 + round(0.44 * val, Some(2));
                                        l2.corruptedRanges.borrow_mut().insert(i, r);
                                        // L3359
                                        let v = ctl(cx, p, format!("rollRangeValue{i}"));
                                        cx.w_mut::<LabelControl>(v).label =
                                            Prop::Val(format!("^7{}", lfmt!("%.2f", r)));
                                        // L3360
                                        let (label, _) = fl(cx, r);
                                        let lab = ctl(cx, p, format!("rollRangeLabel{i}"));
                                        cx.w_mut::<LabelControl>(lab).label = Prop::Val(label);
                                    })),
                                    None,
                                ),
                            )
                            .id();
                        // L3362
                        let r = match r#mod.get(n!("corruptedRange")) {
                            Val::Nil | Val::Bool(false) => 1.0,
                            Val::Num(x) => x,
                            other => lua_error!(
                                "attempt to perform arithmetic on a {} value",
                                other.lua_type()
                            ),
                        };
                        l.corruptedRanges.borrow_mut().insert(i, r);
                        // L3363
                        cx.w_mut::<SliderControl>(slider).val = (r - 0.78) / 0.44;
                        // L3364
                        cx.w_mut::<LabelControl>(value).label =
                            Prop::Val(format!("^7{}", lfmt!("%.2f", r)));
                        // L3365
                        let (label, lineCount) = formatLabel(cx, r);
                        // L3366
                        offset += 16.0 * (lineCount as f64 - 1.0);
                        // L3367
                        let lab = cx
                            .add(
                                p,
                                format!("rollRangeLabel{i}"),
                                LabelControl::new(
                                    Anchor::new(Left, slider, Right),
                                    rect(5, 0, 200, 16),
                                    label,
                                ),
                            )
                            .id();
                        // hide them by default as they are a secondary window
                        // L3370
                        set_shown(cx, lab, false);
                        // L3371
                        set_shown(cx, slider, false);
                        // L3372
                        set_shown(cx, value, false);
                        // L3373
                        offset += 20.0;
                        // L3374
                        l.shownExplicits.borrow_mut().push(i);
                    }
                }
                // L3377
                l.explicitOffset.set(offset);
            }
            // L3379-3386 (setImplicitControlsShown)
            // L3387
            let l2 = l.clone();
            let implicits = cx
                .add(
                    p,
                    "implicits",
                    ButtonControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(5, 5, 80, 20),
                        "Implicits",
                        move |cx| {
                            let l = &l2;
                            let current = l.currentModType.borrow().clone();
                            // L3389
                            let implicitNum = if current != "ScourgeUpside" {
                                l.itemMaxCorruptImplicits
                            } else {
                                4.0
                            };
                            // L3390
                            let canChangeImplicits = current != "Corrupted"
                                || !di(cx, me).extra.truthy(n!("implicitsCannotBeChanged"));
                            // L3391
                            setImplicitControlsShown(cx, p, implicitNum, canChangeImplicits);
                            // L3392
                            let shownExplicits = l.shownExplicits.borrow().clone();
                            for i in shownExplicits {
                                // L3393-3395
                                for k in ["rollRangeLabel", "rollRangeSlider", "rollRangeValue"] {
                                    let c = ctl(cx, p, format!("{k}{i}"));
                                    set_shown(cx, c, false);
                                }
                            }
                            // L3397-3400
                            for k in ["source", "sourceLabel", "sort", "sortLabel"] {
                                let c = ctl(cx, p, k);
                                set_shown(cx, c, true);
                            }
                            // L3401
                            set_popup_height(cx, 103.0 + 20.0 * implicitNum);
                        },
                    ),
                )
                .id();
            // L3403
            cx.base_mut(implicits).shown = Prop::f(move |cx, _| {
                // L3404
                is_unique_rarity(&di(cx, me))
            });
            // L3406
            let l2 = l.clone();
            let rolls = cx
                .add(
                    p,
                    "rolls",
                    ButtonControl::new(
                        Anchor::new(Left, implicits, Right),
                        rect(5, 0, 80, 20),
                        "Roll Ranges",
                        move |cx| {
                            let l = &l2;
                            // L3409
                            setImplicitControlsShown(cx, p, 0.0, false);
                            // L3410
                            let shownExplicits = l.shownExplicits.borrow().clone();
                            for i in shownExplicits {
                                // L3411-3413
                                for k in ["rollRangeLabel", "rollRangeSlider", "rollRangeValue"] {
                                    let c = ctl(cx, p, format!("{k}{i}"));
                                    set_shown(cx, c, true);
                                }
                            }
                            // L3415-3418
                            for k in ["source", "sourceLabel", "sort", "sortLabel"] {
                                let c = ctl(cx, p, k);
                                set_shown(cx, c, false);
                            }
                            // L3419
                            set_popup_height(cx, 55.0 + l.explicitOffset.get());
                        },
                    ),
                )
                .id();
            // L3421
            cx.base_mut(rolls).shown = Prop::f(move |cx, _| {
                // L3422
                is_unique_rarity(&di(cx, me))
            });
            // L3424
            cx.add(
                p,
                "sourceLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 30, 0, 16),
                    "^7Source:",
                ),
            );
            // L3426
            let l2 = l.clone();
            let nSources = sourceList.len();
            let source = cx
                .add(
                    p,
                    "source",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 30, 150, 18),
                        sourceList.iter().map(|s| DropItem::str(*s)).collect(),
                        Some(Rc::new(move |cx: &mut Cx, _index, value| {
                            let l = &l2;
                            let value = value
                                .and_then(DropItem::as_str)
                                .unwrap_or_default()
                                .to_owned();
                            // L3428
                            if value == "Scourge" {
                                // L3429
                                *l.currentModType.borrow_mut() = "ScourgeUpside".to_owned();
                                // L3430
                                buildImplicitList(cx, l, "ScourgeUpside");
                                // L3431
                                buildImplicitList(cx, l, "ScourgeDownside");
                                // L3432
                                let implicitNum = if is_unique_rarity(&di(cx, me)) {
                                    4.0
                                } else {
                                    3.0
                                };
                                // L3433
                                setImplicitControlsShown(cx, p, implicitNum, true);
                                // L3434
                                set_popup_height(cx, 103.0 + 20.0 * implicitNum);
                                // L3435
                                let (a, b) =
                                    (ctl(cx, p, implicit_key(3)), ctl(cx, p, implicit_key(4)));
                                buildScourgeList(cx, l, a, b, "ScourgeDownside");
                                // L3436
                                buildScourgeList(cx, l, b, a, "ScourgeDownside");
                            } else {
                                // L3438
                                buildImplicitList(cx, l, &value);
                                // L3439
                                *l.currentModType.borrow_mut() = value;
                                // L3440
                                let can = !di(cx, me).extra.truthy(n!("implicitsCannotBeChanged"));
                                setImplicitControlsShown(cx, p, l.itemMaxCorruptImplicits, can);
                                // L3441
                                set_popup_height(cx, 103.0 + 20.0 * l.itemMaxCorruptImplicits);
                            }
                            // L3443
                            if let Some(sort) = cx.child(p, "sort") {
                                // L3444
                                let stat = sel_sort_stat(cx, sort);
                                corrupt_apply_sort(cx, l, p, stat);
                            } else {
                                // L3446
                                let current = l.currentModType.borrow().clone();
                                buildCorruptLists(cx, l, p, &current);
                            }
                            // L3448
                            for i in 1..=maxImplicitNum {
                                // L3449
                                let c = ctl(cx, p, implicit_key(i));
                                DropDownControl::set_sel(cx, c, 1, false);
                            }
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L3452
            cx.base_mut(source).enabled = Prop::Val(nSources > 1);
            // L3453
            cx.add(
                p,
                "sortLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(350, 20, 0, 16),
                    "^7Sort by:",
                ),
            );
            // L3454
            let l2 = l.clone();
            cx.add(
                p,
                "sort",
                DropDownControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(355, 20, 240, 18),
                    sortList,
                    Some(Rc::new(move |cx: &mut Cx, _index, value| {
                        // L3455
                        corrupt_apply_sort(cx, &l2, p, sort_stat(value));
                    })),
                    None,
                    false,
                ),
            );
            // L3457
            let implicitRowSize = 20.0;
            // L3458
            let implicitYPos = 35.0;
            // L3459
            let cannot = cx
                .add(
                    p,
                    "implicitCannotBeChangedLabel",
                    LabelControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(20, implicitYPos + implicitRowSize, 0, 20),
                        "^7This Items Implicits Cannot Be Changed",
                    ),
                )
                .id();
            // L3460
            let cannotBeChanged = di(cx, me).extra.truthy(n!("implicitsCannotBeChanged"));
            set_shown(cx, cannot, cannotBeChanged);
            // L3461
            for i in 1..=maxImplicitNum {
                // L3462
                let controlName = implicit_key(i);
                let y = implicitYPos + i as f64 * implicitRowSize;
                // L3463
                let label = cx
                    .add(
                        p,
                        format!("{controlName}Label"),
                        LabelControl::new(
                            Anchor::nil(TopRight, TopLeft),
                            rect(75, y, 0, 16),
                            format!("^7Implicit #{i}:"),
                        ),
                    )
                    .id();
                // L3465
                let drop = cx
                    .add(
                        p,
                        controlName,
                        DropDownControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(80, y, 440, 18),
                            Vec::new(),
                            None,
                            None,
                            false,
                        ),
                    )
                    .id();
                // L3466
                cx.w_mut::<DropDownControl>(drop).tt.tooltip_func =
                    Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
                        // L3467
                        tooltip.clear(false);
                        // L3468
                        let r#mod = value
                            .as_ref()
                            .and_then(|v| v.get::<ImplicitValue>())
                            .and_then(|v| v.r#mod.clone());
                        if mode != DropTipMode::Out {
                            if let Some(r#mod) = r#mod {
                                // L3469
                                for line in &r#mod.lines {
                                    // L3470
                                    tooltip.add_line(cx, 16.0, &format!("^7{line}"));
                                }
                                // L3472
                                super::display_item::AddModComparisonTooltip(
                                    cx,
                                    me,
                                    tooltip,
                                    Some(&r#mod),
                                    true,
                                );
                            }
                        }
                    }));
                // L3475
                // L3476
                let shownVal = if (i as f64) <= itemMaxCorruptImplicits {
                    // L3477
                    !cannotBeChanged
                } else {
                    // L3479
                    false
                };
                // L3481
                set_shown(cx, drop, shownVal);
                // L3482
                set_shown(cx, label, shownVal);
            }
            // L3484
            for i in 1..=maxImplicitNum {
                // L3485
                let l2 = l.clone();
                let selFunc: Rc<dyn Fn(&mut Cx, usize, Option<&DropItem>)> =
                    Rc::new(move |cx: &mut Cx, _, _| {
                        let l = &l2;
                        let current = l.currentModType.borrow().clone();
                        // L3486
                        if current == "ScourgeUpside" {
                            // L3487
                            let otherIdx = if i % 2 == 0 { i - 1 } else { i + 1 };
                            // L3488
                            let modType = if i <= 2 {
                                "ScourgeUpside"
                            } else {
                                "ScourgeDownside"
                            };
                            // remove selected entry from other dropdown of the same type
                            // L3490
                            let (o, s) = (
                                ctl(cx, p, implicit_key(otherIdx)),
                                ctl(cx, p, implicit_key(i)),
                            );
                            buildScourgeList(cx, l, o, s, modType);
                        } else {
                            // L3492
                            buildCorruptLists(cx, l, p, &current);
                        }
                    });
                let c = ctl(cx, p, implicit_key(i));
                cx.w_mut::<DropDownControl>(c).sel_func = Some(selFunc.clone());
                // L3495
                if (i as f64) <= itemMaxCorruptImplicits {
                    // L3496
                    selFunc(cx, 0, None);
                }
            }
            // L3499
            let l2 = l.clone();
            let save = cx
                .add(
                    p,
                    "save",
                    ButtonControl::new(
                        Anchor::nil(Bottom, Bottom),
                        rect(-45, -4, 80, 20),
                        "Corrupt",
                        move |cx| {
                            // L3500
                            let shown = cx.base(ctl(cx, p, "implicit1")).shown.raw();
                            let item = corruptItem(cx, &l2, p, shown);
                            super::display_item::SetDisplayItem(cx, me, Some(item));
                            // L3501
                            close_popup(cx);
                        },
                    ),
                )
                .id();
            // L3503
            let l2 = l.clone();
            cx.w_mut::<ButtonControl>(save).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, ()| {
                    // L3504
                    tooltip.clear(false);
                    // L3505
                    let shown = cx.base(ctl(cx, p, "implicit1")).shown.raw();
                    let item = corruptItem(cx, &l2, p, shown);
                    let f = items_hooks().add_item_tooltip;
                    let b = cx.build().model.clone();
                    f(cx, &b, tooltip, &item, None, false);
                }));
            // L3507
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::nil(Bottom, Bottom),
                    rect(45, -4, 80, 20),
                    "Cancel",
                    |cx| {
                        // L3508
                        close_popup(cx);
                    },
                ),
            );
        },
        PopupOpts::default(),
    );
}

// ---- AddCustomModifierToDisplayItem ----------------------------------------------------------

/// A `sourceList` entry (`{ label = .., sourceId = .. }`).
#[derive(Clone)]
struct SourceEntry {
    label: &'static str,
    sourceId: &'static str,
}

/// The value of a `source` dropdown entry.
struct SourceValue {
    sourceId: &'static str,
}

/// `defaultOrder` of a `modList` entry: a list index or a mod id (nil for the essence and
/// prefix/suffix entries, whose comparators never read it).
#[derive(Clone, Copy)]
enum Order {
    Nil,
    Num(f64),
    Str(Name),
}

/// `a.defaultOrder < b.defaultOrder` (mixed or nil operands raise).
#[track_caller]
fn order_lt(a: Order, b: Order) -> bool {
    match (a, b) {
        (Order::Num(a), Order::Num(b)) => a < b,
        (Order::Str(a), Order::Str(b)) => a.as_str().as_bytes() < b.as_str().as_bytes(),
        (Order::Nil, _) | (_, Order::Nil) => lua_error!("attempt to compare nil with a value"),
        _ => lua_error!("attempt to compare number with string"),
    }
}

/// One `modList` entry of AddCustomModifierToDisplayItem (`{ label, mod, affixType, type,
/// defaultOrder, essence }`, plus the sort fields).
struct CustomEntry {
    label: String,
    r#mod: ItemModRef,
    affixType: Option<Name>,
    /// `"crafted"` or `"custom"`
    r#type: &'static str,
    defaultOrder: Order,
    essence: Option<&'static Essence>,
    defaultSortOrder: Cell<Option<f64>>,
    sortValue: Cell<Option<f64>>,
    sortValues: RefCell<Option<Vec<(Name, f64)>>>,
}

impl CustomEntry {
    fn new(
        label: String,
        r#mod: ItemModRef,
        affixType: Option<Name>,
        r#type: &'static str,
        defaultOrder: Order,
    ) -> Rc<CustomEntry> {
        Rc::new(CustomEntry {
            label,
            r#mod,
            affixType,
            r#type,
            defaultOrder,
            essence: None,
            defaultSortOrder: Cell::new(None),
            sortValue: Cell::new(None),
            sortValues: RefCell::new(None),
        })
    }
}

/// The locals of AddCustomModifierToDisplayItem.
struct Custom {
    me: CtrlId,
    sourceList: RefCell<Vec<SourceEntry>>,
    modList: RefCell<Vec<Rc<CustomEntry>>>,
    sortStats: indexmap::IndexMap<Name, &'static PowerStat>,
}

/// The dropdown entries of `modList` (`controls.modSelect.list` aliases it).
fn mod_items(l: &Custom) -> Vec<DropItem> {
    l.modList
        .borrow()
        .iter()
        .map(|e| DropItem::new(e.label.clone(), e.clone()))
        .collect()
}

/// The alias of `controls.modSelect.list` and `modList` (module notes): store the mutated list.
fn sync_mod_select(cx: &mut Cx, l: &Custom, p: CtrlId) {
    if let Some(drop) = cx.child(p, "modSelect") {
        cx.w_mut::<DropDownControl>(drop).list = mod_items(l);
    }
}

/// `sourceList[controls.source.selIndex].sourceId` (a nil entry raises).
#[track_caller]
fn sel_source_id(cx: &Cx, l: &Custom, p: CtrlId) -> &'static str {
    let sel = cx.w::<DropDownControl>(ctl(cx, p, "source")).sel_index;
    l.sourceList
        .borrow()
        .get(sel.wrapping_sub(1))
        .map(|s| s.sourceId)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// `local function setDefaultSortOrder()` (L3521-3527).
fn setDefaultSortOrder(l: &Custom) {
    // L3522
    for (index, listMod) in l.modList.borrow().iter().enumerate() {
        // L3523
        listMod.defaultSortOrder.set(Some((index + 1) as f64));
        // L3524
        listMod.sortValue.set(None);
        // L3525
        *listMod.sortValues.borrow_mut() = None;
    }
}

/// `local function getSortValue(listMod, stat, calcFunc, slotName, useFullDPS)` (L3528-3543).
fn custom_get_sort_value(
    cx: &mut Cx,
    l: &Custom,
    listMod: &CustomEntry,
    stat: Name,
    calcFunc: &Rc<dyn MiscCalc>,
    slotName: Name,
    useFullDPS: bool,
) -> f64 {
    // L3529
    if listMod.sortValues.borrow().is_none() {
        *listMod.sortValues.borrow_mut() = Some(Vec::new());
    }
    // L3530
    let cached = listMod
        .sortValues
        .borrow()
        .as_ref()
        .and_then(|v| v.iter().find(|(k, _)| *k == stat).map(|(_, v)| *v));
    if let Some(v) = cached {
        // L3531
        return v;
    }
    let displayItem = di(cx, l.me);
    // L3533
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L3534
    item.id.set(displayItem.id.get());
    // L3535
    for line in &listMod.r#mod.lines {
        // L3536
        let t = Table::new();
        t.set(n!("line"), check_line(cx, line));
        t.set(n!("modTags"), mod_tags(&listMod.r#mod));
        t.set(Name::from(listMod.r#type), true);
        item.explicitModLines.get().push(t);
    }
    // L3538
    item.BuildAndParseRaw();
    // L3539
    let output = calc_with(calcFunc, slotName, &item, useFullDPS);
    // L3540
    let value = get_from_output(output, &l.sortStats, stat);
    // L3541
    if let Some(v) = listMod.sortValues.borrow_mut().as_mut() {
        v.push((stat, value));
    }
    // L3542
    value
}

/// `local function applySort(stat, selectFirst)` (L3544-3578).
fn custom_apply_sort(cx: &mut Cx, l: &Custom, p: CtrlId, stat: Option<Name>, selectFirst: bool) {
    // L3545
    let Some(modSelect) = cx.child(p, "modSelect").filter(|&c| cx.is_shown(c)) else {
        // L3546
        return;
    };
    // L3548
    let selected = if selectFirst {
        None
    } else {
        let sel = cx.w::<DropDownControl>(modSelect).sel_index;
        l.modList.borrow().get(sel.wrapping_sub(1)).cloned()
    };
    // L3549
    if let Some(stat) = stat {
        // L3550
        let slotName = di(cx, l.me).GetPrimarySlot();
        // L3551
        let calcFunc = misc_calc(cx);
        // L3552
        let useFullDPS = stat.as_str() == "FullDPS";
        // L3553
        let list = l.modList.borrow().clone();
        for listMod in &list {
            // L3554 DIVERGENCE(pending-calc): no calculator (wasm only) gives 0
            let v = match &calcFunc {
                Some(f) => custom_get_sort_value(cx, l, listMod, stat, f, slotName, useFullDPS),
                None => 0.0,
            };
            listMod.sortValue.set(Some(v));
        }
        // L3556
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, |a, b| {
            let (av, bv) = (sort_value(a.sortValue.get()), sort_value(b.sortValue.get()));
            // L3557
            if av != bv {
                // L3558
                return av > bv;
            }
            // L3560
            a.defaultSortOrder.get().unwrap_or(0.0) < b.defaultSortOrder.get().unwrap_or(0.0)
        });
        *l.modList.borrow_mut() = list;
    } else {
        // L3563
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, |a, b| {
            // L3564
            a.defaultSortOrder.get().unwrap_or(0.0) < b.defaultSortOrder.get().unwrap_or(0.0)
        });
        *l.modList.borrow_mut() = list;
    }
    sync_mod_select(cx, l, p);
    // L3567
    update_search(cx, modSelect);
    // L3568
    if let Some(selected) = selected {
        // L3569
        let index = l
            .modList
            .borrow()
            .iter()
            .position(|listMod| Rc::ptr_eq(listMod, &selected));
        // L3570
        if let Some(index) = index {
            // L3571
            cx.w_mut::<DropDownControl>(modSelect).sel_index = index + 1;
            // L3572 break
        }
    } else {
        // L3576
        DropDownControl::set_sel(cx, modSelect, 1, true);
    }
}

/// `local function buildDropRestricted(baseCategories, modDb)` (L3579-3602).
fn buildDropRestricted(
    cx: &mut Cx,
    l: &Custom,
    baseCategories: &[(&'static str, Vec<&'static str>)],
    modDb: &'static gamedata::schema::affixes::AffixTable,
) {
    let displayItem = di(cx, l.me);
    // L3580
    let base = displayItem
        .base
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'base' (a nil value)"));
    // L3581
    let subTypeName = base
        .subType
        .as_ref()
        .map(|subType| format!("{}: {subType}", base.r#type));
    // L3582 ORDER: (a), module notes
    for (modId, categories) in baseCategories {
        // L3583
        let Some(r#mod) = modDb.0.get(&Name::from(modId)).cloned() else {
            // L3585
            cx.sys
                .platform
                .con_print(&lfmt!("Unknown drop-restricted mod id: %s", *modId));
            // L3586 goto nextDrop
            continue;
        };
        // L3588
        for cat in categories {
            // L3589
            if &*base.r#type == *cat || subTypeName.as_deref() == Some(*cat) {
                // L3590-3596
                let label = format!("{} ({})", concat_mod(&r#mod), mod_type_str(&r#mod));
                let affixType = r#mod.r#type;
                l.modList.borrow_mut().push(CustomEntry::new(
                    label,
                    r#mod,
                    affixType,
                    "custom",
                    Order::Str(Name::from(modId)),
                ));
                // L3597 goto nextDrop
                break;
            }
        }
        // L3600 ::nextDrop::
    }
}

/// `local function sortByPrefixSuffix(a, b)` (L3603-3609).
fn sortByPrefixSuffix(a: &Rc<CustomEntry>, b: &Rc<CustomEntry>) -> bool {
    // L3604
    if a.affixType != b.affixType {
        // L3605
        a.affixType.is_some_and(|t| t.as_str() == "Prefix")
            && b.affixType.is_some_and(|t| t.as_str() == "Suffix")
    } else {
        // L3607
        order_lt(a.defaultOrder, b.defaultOrder)
    }
}

/// `mod.statOrder[i]` in a comparison (nil raises).
#[track_caller]
fn stat_order(r#mod: &ItemMod, i: usize) -> Option<f64> {
    r#mod.statOrder.get(i - 1).copied()
}

///Mutates modList to contain mods from the specified source
///@param sourceId string @The crafting source id to build the list of mods for
/// `local function buildMods(sourceId)` (L3612-3749).
fn buildMods(cx: &mut Cx, l: &Custom, sourceId: &str) {
    let displayItem = di(cx, l.me);
    let d = model::data();
    // L3613
    l.modList.borrow_mut().clear();
    // L3614
    if sourceId == "MASTER" {
        // L3615
        let mut excludeGroups: Vec<Name> = Vec::new();
        // L3616
        for k in [n!("prefixes"), n!("suffixes")] {
            let modLine =
                displayItem.extra.get(k).table().unwrap_or_else(|| {
                    lua_error!("attempt to index local 'modLine' (a nil value)")
                });
            // L3617
            let limit = match modLine.get(n!("limit")) {
                v if v.truthy() => v.num(),
                _ => match displayItem.extra.get(n!("affixLimit")) {
                    Val::Num(x) => x / 2.0,
                    other => lua_error!(
                        "attempt to perform arithmetic on field 'affixLimit' (a {} value)",
                        other.lua_type()
                    ),
                },
            };
            let mut i = 1.0;
            while i <= limit {
                // L3618
                let entry = modLine.table(i as i64);
                if let Some(entry) = entry {
                    let modId = entry.get(n!("modId"));
                    if modId.str().as_deref() != Some("None") {
                        // L3619
                        let id = modId.str().map(|s| Name::from(&s));
                        let r#mod = id
                            .and_then(|id| affixes(&displayItem).0.get(&id).cloned())
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index field '?' (a nil value)")
                            });
                        let group = r#mod
                            .group
                            .unwrap_or_else(|| lua_error!("table index is nil"));
                        if !excludeGroups.contains(&group) {
                            excludeGroups.push(group);
                        }
                    }
                }
                i += 1.0;
            }
        }
        // L3623
        for (i0, craft) in d.masterMods.0.iter().enumerate() {
            // L3624
            let types = craft
                .types
                .as_ref()
                .unwrap_or_else(|| lua_error!("attempt to index field 'types' (a nil value)"));
            let excluded = craft.group.is_some_and(|g| excludeGroups.contains(&g));
            if types.contains(&displayItem.r#type.get()) && !excluded {
                // L3625-3631
                let label = format!("{} ^8({})", concat_mod(craft), mod_type_str(craft));
                l.modList.borrow_mut().push(CustomEntry::new(
                    label,
                    craft.clone(),
                    craft.r#type,
                    "crafted",
                    Order::Num((i0 + 1) as f64),
                ));
            }
        }
        // L3634
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, |a, b| {
            // L3635
            if a.affixType != b.affixType {
                // L3636
                a.affixType.is_some_and(|t| t.as_str() == "Prefix")
                    && b.affixType.is_some_and(|t| t.as_str() == "Suffix")
            } else {
                // L3638
                order_lt(a.defaultOrder, b.defaultOrder)
            }
        });
        *l.modList.borrow_mut() = list;
    } else if sourceId == "ESSENCE" {
        // L3642 ORDER: (c), module notes
        for essence in essences() {
            // L3643
            let itemType = displayItem.r#type.get();
            let modId = essence
                .mods
                .iter()
                .find(|(k, _)| *k == itemType.as_str())
                .map(|(_, v)| *v);
            // L3644 (`affixes[nil]` reads nil)
            let r#mod = modId.and_then(|id| affixes(&displayItem).0.get(&Name::from(id)).cloned());
            // L3645
            if let Some(r#mod) = r#mod {
                // L3646-3651
                let name = essence.name.unwrap_or_else(|| {
                    lua_error!("attempt to concatenate field 'name' (a nil value)")
                });
                let label = format!(
                    "{name}   ^8[{}] ({})",
                    concat_mod(&r#mod),
                    mod_type_str(&r#mod)
                );
                let mut e = CustomEntry {
                    label,
                    r#mod,
                    affixType: None,
                    r#type: "custom",
                    defaultOrder: Order::Nil,
                    essence: None,
                    defaultSortOrder: Cell::new(None),
                    sortValue: Cell::new(None),
                    sortValues: RefCell::new(None),
                };
                e.essence = Some(essence);
                l.modList.borrow_mut().push(Rc::new(e));
            }
        }
        // L3654
        let mut list = l.modList.borrow().clone();
        let num =
            |v: Option<f64>| v.unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
        pob_lua::sort(&mut list, |a, b| {
            let (ea, eb) = (
                a.essence.expect("essence entry"),
                b.essence.expect("essence entry"),
            );
            // L3655
            if num(ea.r#type) != num(eb.r#type) {
                // L3656
                num(ea.r#type) > num(eb.r#type)
            } else {
                // L3658
                num(ea.tier) > num(eb.tier)
            }
        });
        *l.modList.borrow_mut() = list;
    } else if sourceId == "PREFIX" || sourceId == "SUFFIX" {
        // L3662 ORDER: (a), module notes
        for (_, r#mod) in sorted_affixes(affixes(&displayItem)) {
            // L3663
            let matches = r#mod
                .r#type
                .is_some_and(|t| t.as_str().eq_ignore_ascii_case(sourceId));
            if matches && displayItem.GetModSpawnWeight(&r#mod, None, None, None) > 0.0 {
                // L3664-3668
                let affix = r#mod.affix.clone().unwrap_or_else(|| {
                    lua_error!("attempt to concatenate field 'affix' (a nil value)")
                });
                let label = format!("{affix}   ^8[{}]", concat_mod(&r#mod));
                l.modList.borrow_mut().push(CustomEntry::new(
                    label,
                    r#mod,
                    None,
                    "custom",
                    Order::Nil,
                ));
            }
        }
        // L3671
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, |a, b| {
            // L3672
            let modA = &a.r#mod;
            // L3673
            let modB = &b.r#mod;
            // L3674
            let n = m_max(modA.lines.len() as f64, modB.lines.len() as f64) as usize;
            for i in 1..=n {
                // L3675
                if modA.lines.len() < i {
                    // L3676
                    return true;
                } else if modB.lines.len() < i {
                    // L3678
                    return false;
                } else if stat_order(modA, i) != stat_order(modB, i) {
                    // L3680
                    return match (stat_order(modA, i), stat_order(modB, i)) {
                        (Some(x), Some(y)) => x < y,
                        _ => lua_error!("attempt to compare nil with number"),
                    };
                }
            }
            // L3683
            level_of(modA) > level_of(modB)
        });
        *l.modList.borrow_mut() = list;
    } else if sourceId == "VEILED" {
        // L3686 ORDER: (c), module notes
        for (i, r#mod) in sorted_affixes(&d.veiledMods) {
            // L3687
            if displayItem.GetModSpawnWeight(&r#mod, None, None, None) > 0.0 {
                // L3688-3694
                let label = format!("{} ({})", concat_mod(&r#mod), mod_type_str(&r#mod));
                let affixType = r#mod.r#type;
                l.modList.borrow_mut().push(CustomEntry::new(
                    label,
                    r#mod,
                    affixType,
                    "custom",
                    Order::Str(i),
                ));
            }
        }
        // L3697
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, |a, b| {
            // L3698
            if a.affixType != b.affixType {
                // L3699
                a.affixType.is_some_and(|t| t.as_str() == "Prefix")
                    && b.affixType.is_some_and(|t| t.as_str() == "Suffix")
            } else {
                // L3701
                order_lt(a.defaultOrder, b.defaultOrder)
            }
        });
        *l.modList.borrow_mut() = list;
    } else if sourceId == "INCURSION" {
        // L3705
        buildDropRestricted(
            cx,
            l,
            &drop_only("IncursionDropOnly"),
            item_mods("Explicit"),
        );
        // L3706
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, sortByPrefixSuffix);
        *l.modList.borrow_mut() = list;
    } else if sourceId == "DELVE" {
        // L3708
        buildDropRestricted(cx, l, &drop_only("DelveDropOnly"), item_mods("Delve"));
        // L3709
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, sortByPrefixSuffix);
        *l.modList.borrow_mut() = list;
    } else if sourceId == "FOSSIL" {
        // L3711 ORDER: (c), module notes
        for (i, r#mod) in sorted_affixes(affixes(&displayItem)) {
            // L3712
            if displayItem.CheckIfModIsDelve(&r#mod)
                && displayItem.GetModSpawnWeight(&r#mod, None, None, None) > 0.0
            {
                // L3713-3719
                let label = format!("{} ({})", concat_mod(&r#mod), mod_type_str(&r#mod));
                let affixType = r#mod.r#type;
                l.modList.borrow_mut().push(CustomEntry::new(
                    label,
                    r#mod,
                    affixType,
                    "custom",
                    Order::Str(i),
                ));
            }
        }
        // L3722
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, sortByPrefixSuffix);
        *l.modList.borrow_mut() = list;
    } else if sourceId == "NECROPOLIS" {
        // L3724 ORDER: (c), module notes
        for (i, r#mod) in sorted_affixes(&d.necropolisMods) {
            // L3725
            if displayItem.GetNecropolisModSpawnWeight(&r#mod) > 0.0 {
                // L3726-3732
                let label = format!("{} ({})", concat_mod(&r#mod), mod_type_str(&r#mod));
                let affixType = r#mod.r#type;
                l.modList.borrow_mut().push(CustomEntry::new(
                    label,
                    r#mod,
                    affixType,
                    "custom",
                    Order::Str(i),
                ));
            }
        }
        // L3735
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, sortByPrefixSuffix);
        *l.modList.borrow_mut() = list;
    } else if sourceId == "BEASTCRAFT" {
        // L3737 ORDER: (c), module notes
        for (i, r#mod) in beast_craft() {
            // L3738-3744
            let label = format!("{} ({})", concat_mod(r#mod), mod_type_str(r#mod));
            l.modList.borrow_mut().push(CustomEntry::new(
                label,
                r#mod.clone(),
                r#mod.r#type,
                "custom",
                Order::Str(*i),
            ));
        }
        // L3746
        let mut list = l.modList.borrow().clone();
        pob_lua::sort(&mut list, sortByPrefixSuffix);
        *l.modList.borrow_mut() = list;
    }
    // L3748
    setDefaultSortOrder(l);
}

/// `local function addModifier()` (L3799-3817).
fn custom_add_modifier(cx: &mut Cx, l: &Custom, p: CtrlId) -> model::ItemRef {
    let displayItem = di(cx, l.me);
    // L3800
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L3801
    item.id.set(displayItem.id.get());
    // L3802
    let sourceId = sel_source_id(cx, l, p);
    // L3803
    if sourceId == "CUSTOM" {
        let buf = cx.w::<EditControl>(ctl(cx, p, "custom")).buf.clone();
        // L3804
        if lmatch(&buf, "%S").is_some() {
            // L3805
            let t = Table::new();
            t.set(n!("line"), buf);
            t.set(n!("custom"), true);
            item.explicitModLines.get().push(t);
        }
    } else {
        // L3808
        let sel = cx.w::<DropDownControl>(ctl(cx, p, "modSelect")).sel_index;
        let listMod = l.modList.borrow().get(sel.wrapping_sub(1)).cloned();
        // L3809
        if let Some(listMod) = listMod {
            // L3810
            for line in &listMod.r#mod.lines {
                // L3811
                let t = Table::new();
                t.set(n!("line"), &**line);
                t.set(n!("modTags"), mod_tags(&listMod.r#mod));
                t.set(Name::from(listMod.r#type), true);
                item.explicitModLines.get().push(t);
            }
        }
    }
    // L3815
    item.BuildAndParseRaw();
    // L3816
    item
}

/// Opens the custom modifier popup
/// lua: ItemsTab.lua:3516-3867 `ItemsTabClass:AddCustomModifierToDisplayItem()`.
/// @pob Classes/ItemsTab.lua:3516-3867 h=88bc24cf
pub fn AddCustomModifierToDisplayItem(cx: &mut Cx, me: CtrlId) {
    // L3517 local controls = { } (the popup's children)
    // L3518-3519 (the state below)
    // L3520
    let (sortList, sortStats) = ctor::buildModSortList();
    let l = Rc::new(Custom {
        me,
        sourceList: RefCell::new(Vec::new()),
        modList: RefCell::new(Vec::new()),
        sortStats,
    });
    // L3521-3749 (setDefaultSortOrder, getSortValue, applySort, buildDropRestricted,
    // sortByPrefixSuffix, buildMods)
    let displayItem = di(cx, me);
    let itemType = displayItem.r#type.get();
    let t = itemType.as_str();
    {
        let mut sourceList = l.sourceList.borrow_mut();
        let mut add = |label: &'static str, sourceId: &'static str| {
            sourceList.push(SourceEntry { label, sourceId });
        };
        // L3750
        if t != "Tincture" && t != "Graft" {
            // L3751
            if t != "Jewel" {
                // L3752
                add("Crafting Bench", "MASTER");
            }
            // L3754
            if t != "Jewel" && t != "Flask" && t != "Graft" {
                // L3755
                add("Essence", "ESSENCE");
                // L3756
                add("Veiled", "VEILED");
                // L3757
                add("Beastcraft", "BEASTCRAFT");
            }
            // L3759
            if t == "Helmet" || t == "Body Armour" || t == "Gloves" || t == "Boots" {
                // L3760
                add("Necropolis", "NECROPOLIS");
            }
            // L3762
            if displayItem.clusterJewel.get().is_none() && t != "Flask" && t != "Graft" {
                // L3763
                add("Fossil", "FOSSIL");
                // L3764
                add("Delve Drop-restricted", "DELVE");
                // L3765
                add("Incursion Drop-restricted", "INCURSION");
            }
            // L3767
            if !displayItem.extra.truthy(n!("crafted")) {
                // L3768
                add("Prefix", "PREFIX");
                // L3769
                add("Suffix", "SUFFIX");
            }
        }
    }
    // L3772
    if let Some(rareLikeUnique) = displayItem.rareLikeUnique_ref() {
        // L3773
        if let Some(applicableSourceIds) = &rareLikeUnique.supportsCustomModifiers {
            // L3775
            let newSourceList: Vec<SourceEntry> = l
                .sourceList
                .borrow()
                .iter()
                // L3776-3778
                .filter(|list| applicableSourceIds.get(&Name::from(list.sourceId)) == Some(&true))
                .cloned()
                .collect();
            // L3781
            *l.sourceList.borrow_mut() = newSourceList;
        }
    }
    // test each category to see if they actually match any mods
    // TODO: given that we need to do this, it would be better to just keep a
    // list of matching mod ids, rather than building a list to check, and then
    // building it again when selecting it
    // L3788
    let mut i = 0;
    // L3789
    loop {
        let Some(src) = l.sourceList.borrow().get(i).cloned() else {
            break;
        };
        // L3790
        buildMods(cx, &l, src.sourceId);
        // L3791
        if l.modList.borrow().is_empty() {
            // L3792
            l.sourceList.borrow_mut().remove(i);
        } else {
            // L3794
            i += 1;
        }
    }
    // L3797
    l.sourceList.borrow_mut().push(SourceEntry {
        label: "Custom",
        sourceId: "CUSTOM",
    });
    // L3798
    let first = l.sourceList.borrow()[0].sourceId;
    buildMods(cx, &l, first);
    // L3799-3817 (addModifier)
    let defaultIsCustom = first == "CUSTOM";
    let sourceItems: Vec<DropItem> = l
        .sourceList
        .borrow()
        .iter()
        .map(|s| {
            DropItem::new(
                s.label,
                Rc::new(SourceValue {
                    sourceId: s.sourceId,
                }),
            )
        })
        .collect();

    // L3866
    open_popup(
        cx,
        710.0,
        105.0,
        "Add Modifier to Item",
        move |cx, p| {
            let p = p.id();
            // L3818
            cx.add(
                p,
                "sourceLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 20, 0, 16),
                    "^7Source:",
                ),
            );
            // L3819
            let l2 = l.clone();
            let nSources = sourceItems.len();
            let source = cx
                .add(
                    p,
                    "source",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 20, 150, 18),
                        sourceItems,
                        Some(Rc::new(move |cx: &mut Cx, _index, value| {
                            let l = &l2;
                            // L3820 (`value.sourceId`)
                            let sourceId = value
                                .and_then(|v| v.get::<SourceValue>())
                                .map(|v| v.sourceId)
                                .unwrap_or_else(|| {
                                    lua_error!("attempt to index local 'value' (a nil value)")
                                });
                            buildMods(cx, l, sourceId);
                            sync_mod_select(cx, l, p);
                            // L3821
                            let modSelect = ctl(cx, p, "modSelect");
                            DropDownControl::set_sel(cx, modSelect, 1, false);
                            // L3822
                            if let Some(sort) = cx.child(p, "sort") {
                                // L3823
                                let stat = sel_sort_stat(cx, sort);
                                custom_apply_sort(cx, l, p, stat, true);
                            }
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L3826
            cx.base_mut(source).enabled = Prop::Val(nSources > 1);
            // L3827
            let notCustom = {
                let l = l.clone();
                move |cx: &Cx| sel_source_id(cx, &l, p) != "CUSTOM"
            };
            let sortLabel = cx
                .add(
                    p,
                    "sortLabel",
                    LabelControl::new(
                        Anchor::nil(TopRight, TopLeft),
                        rect(350, 20, 0, 16),
                        "^7Sort by:",
                    ),
                )
                .id();
            // L3828
            let nc = notCustom.clone();
            cx.base_mut(sortLabel).shown = Prop::f(move |cx, _| {
                // L3829
                nc(cx)
            });
            // L3831
            let l2 = l.clone();
            let sort = cx
                .add(
                    p,
                    "sort",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(355, 20, 240, 18),
                        sortList,
                        Some(Rc::new(move |cx: &mut Cx, _index, value| {
                            // L3832
                            custom_apply_sort(cx, &l2, p, sort_stat(value), true);
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L3834
            let nc = notCustom.clone();
            cx.base_mut(sort).shown = Prop::f(move |cx, _| {
                // L3835
                nc(cx)
            });
            // L3837
            cx.add(
                p,
                "modSelectLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 45, 0, 16),
                    "^7Modifier:",
                ),
            );
            // L3838
            let modSelect = cx
                .add(
                    p,
                    "modSelect",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 45, 600, 18),
                        mod_items(&l),
                        None,
                        None,
                        false,
                    ),
                )
                .id();
            // L3839
            let nc = notCustom.clone();
            cx.base_mut(modSelect).shown = Prop::f(move |cx, _| {
                // L3840
                nc(cx)
            });
            // L3842
            cx.w_mut::<DropDownControl>(modSelect).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
                    // L3843
                    tooltip.clear(false);
                    // L3844
                    let entry = value.as_ref().and_then(|v| v.get::<CustomEntry>());
                    if mode != DropTipMode::Out {
                        if let Some(entry) = entry {
                            // L3845
                            for line in &entry.r#mod.lines {
                                // L3846
                                tooltip.add_line(cx, 16.0, &format!("^7{line}"));
                            }
                            // L3848
                            let r#mod = entry.r#mod.clone();
                            super::display_item::AddModComparisonTooltip(
                                cx,
                                me,
                                tooltip,
                                Some(&r#mod),
                                false,
                            );
                        }
                    }
                }));
            // L3851
            let custom = cx
                .add(
                    p,
                    "custom",
                    EditControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 45, 440, 18),
                        EditArgs::default(),
                    ),
                )
                .id();
            // L3852
            let l2 = l.clone();
            cx.base_mut(custom).shown = Prop::f(move |cx, _| {
                // L3853
                sel_source_id(cx, &l2, p) == "CUSTOM"
            });
            // L3855
            let l2 = l.clone();
            let save = cx
                .add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 75, 80, 20), "Add", move |cx| {
                        // L3856
                        let item = custom_add_modifier(cx, &l2, p);
                        super::display_item::SetDisplayItem(cx, me, Some(item));
                        // L3857
                        close_popup(cx);
                    }),
                )
                .id();
            // L3859
            let l2 = l.clone();
            cx.w_mut::<ButtonControl>(save).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, ()| {
                    // L3860
                    tooltip.clear(false);
                    // L3861
                    let item = custom_add_modifier(cx, &l2, p);
                    let f = items_hooks().add_item_tooltip;
                    let b = cx.build().model.clone();
                    f(cx, &b, tooltip, &item, None, false);
                }));
            // L3863
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(45, 75, 80, 20), "Cancel", |cx| {
                    // L3864
                    close_popup(cx);
                }),
            );
        },
        PopupOpts {
            enter: Some("save".into()),
            default: defaultIsCustom.then(|| "custom".into()),
            ..PopupOpts::default()
        },
    );
}

// ---- AddCrucibleModifierToDisplayItem --------------------------------------------------------

/// A `modList[i]` entry of the crucible popup: the `"None"` string or `{ label, mod, affixType,
/// type = "crucible", defaultOrder }`.
#[derive(Clone)]
enum CrucEntry {
    None,
    Mod(Rc<CrucItem>),
}

struct CrucItem {
    label: String,
    r#mod: &'static CrucibleMod,
    affixType: Option<Name>,
    defaultOrder: Name,
}

impl CrucEntry {
    fn item(&self) -> DropItem {
        match self {
            CrucEntry::None => DropItem::str("None"),
            CrucEntry::Mod(m) => DropItem::new(m.label.clone(), m.clone()),
        }
    }
    /// `entry.affixType` (the string entry indexes the string library: nil)
    fn affixType(&self) -> Option<Name> {
        match self {
            CrucEntry::None => None,
            CrucEntry::Mod(m) => m.affixType,
        }
    }
    /// `entry.defaultOrder` (nil for the string entry)
    fn defaultOrder(&self) -> Option<Name> {
        match self {
            CrucEntry::None => None,
            CrucEntry::Mod(m) => Some(m.defaultOrder),
        }
    }
}

/// The 1-based `modList` index of a node location (`modList[location]`, nil outside 1..5).
#[track_caller]
fn location_index(location: f64) -> usize {
    if location.fract() == 0.0 && (1.0..=5.0).contains(&location) {
        location as usize
    } else {
        lua_error!("bad argument #1 to 'insert' (table expected, got nil)")
    }
}

/// Opens the crucible modifier popup
/// lua: ItemsTab.lua:3870-3979 `ItemsTabClass:AddCrucibleModifierToDisplayItem()`.
/// @pob Classes/ItemsTab.lua:3870-3979 h=ed534949
pub fn AddCrucibleModifierToDisplayItem(cx: &mut Cx, me: CtrlId) {
    // L3871 local controls = { } (the popup's children)
    // L3872
    let mut modList: [Vec<CrucEntry>; 5] = std::array::from_fn(|_| vec![CrucEntry::None]);
    // L3873
    let mut itemModMap: Vec<String> = Vec::new();
    let mut nodeSelections: Vec<(f64, Name)> = Vec::new();
    // L3874-3880 local function getLabelFromMod(mod)
    let getLabelFromMod = |cx: &Cx, r#mod: &ItemMod| -> String {
        // L3875 (copyTable(mod): only the lines are concatenated)
        // L3876-3878
        let label: Vec<String> = r#mod
            .lines
            .iter()
            .map(|line| check_line(cx, line))
            .collect();
        // L3879
        label.join("/")
    };
    let displayItem = di(cx, me);
    // set up name map to know what modLines the item has as we build the mods out
    // L3940
    for r#mod in displayItem.crucibleModLines.get().snapshot() {
        // L3941
        if let Some(line) = r#mod.str(n!("line")) {
            itemModMap.push(line.to_string());
        } else {
            lua_error!("table index is nil");
        }
    }
    let has = |line: &str| itemModMap.iter().any(|l| l == line);
    // L3943 buildCrucibleMods() (L3881-3917)
    {
        // L3882 ORDER: (a), module notes
        for (i, cm) in crucible() {
            let r#mod = &cm.r#mod;
            // L3883
            if displayItem.CanHaveMod(r#mod, None) {
                // item mod must match the whole mod, whether that's one line or two
                // L3885
                let line = |k: usize| r#mod.lines.get(k).map(|l| check_line(cx, l));
                let first = line(0)
                    .unwrap_or_else(|| lua_error!("bad argument #1 to 'checkLineForAllocates'"));
                if has(&first) && line(1).map_or(true, |second| has(&second)) {
                    // for multi nodes, if the first location is taken, use second
                    // works for multi vs single node, ambiguous for multi vs multi (3,4 vs 3,4) but both mods load
                    let loc1 = cm
                        .nodeLocation
                        .first()
                        .copied()
                        .unwrap_or_else(|| lua_error!("table index is nil"));
                    let taken = nodeSelections.iter().any(|(k, _)| *k == loc1);
                    // L3888
                    let target = match cm.nodeLocation.get(1) {
                        Some(&loc2) if taken => loc2,
                        // nodeSelections[nodeId] = defaultOrder, used later to match with sorted modList to get selIndex
                        _ => loc1,
                    };
                    // L3889 / L3892
                    if let Some(e) = nodeSelections.iter_mut().find(|(k, _)| *k == target) {
                        e.1 = *i;
                    } else {
                        nodeSelections.push((target, *i));
                    }
                }
                // L3895
                for &location in &cm.nodeLocation {
                    // L3896-3902
                    let label = format!(
                        "{} - Tier: {}",
                        getLabelFromMod(cx, r#mod),
                        cm.tier.map_or_else(
                            || lua_error!("attempt to concatenate field 'tier' (a nil value)"),
                            tostring_num
                        )
                    );
                    modList[location_index(location) - 1].push(CrucEntry::Mod(Rc::new(CrucItem {
                        label,
                        r#mod: cm,
                        affixType: r#mod.r#type,
                        defaultOrder: *i,
                    })));
                }
            }
        }
        // L3906
        for tierList in &mut modList {
            // L3907
            pob_lua::sort(tierList, |a, b| {
                // L3908
                if !matches!(b, CrucEntry::None) {
                    // L3909
                    if a.affixType() != b.affixType() {
                        // L3910
                        a.affixType().is_some_and(|t| t.as_str() == "Spawn")
                            && b.affixType().is_some_and(|t| t.as_str() == "MergeOnly")
                    } else {
                        // L3912
                        match (a.defaultOrder(), b.defaultOrder()) {
                            (Some(x), Some(y)) => x.as_str().as_bytes() < y.as_str().as_bytes(),
                            _ => lua_error!("attempt to compare nil with string"),
                        }
                    }
                } else {
                    false
                }
            });
        }
    }
    let modList = Rc::new(modList);
    // L3918-3938 (addModifier)

    // L3978
    open_popup(
        cx,
        710.0,
        185.0,
        "Add Crucible Modifier to Item",
        move |cx, p| {
            let p = p.id();
            // L3944
            let mut y = 45.0;
            // L3945
            for i in 1..=5 {
                // L3946
                cx.add(
                    p,
                    format!("modSelectNode{i}Label"),
                    LabelControl::new(
                        Anchor::nil(TopRight, TopLeft),
                        rect(95, y, 0, 16),
                        format!("^7Node {i}:"),
                    ),
                );
                // L3947
                let drop = cx
                    .add(
                        p,
                        format!("modSelectNode{i}"),
                        DropDownControl::new(
                            Anchor::nil(TopLeft, TopLeft),
                            rect(100, y, 555, 18),
                            modList[i - 1].iter().map(CrucEntry::item).collect(),
                            None,
                            None,
                            false,
                        ),
                    )
                    .id();
                // L3948
                cx.w_mut::<DropDownControl>(drop).tt.tooltip_func =
                    Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
                        // L3949
                        tooltip.clear(false);
                        // L3950 (`value ~= "None"`)
                        let item = value.as_ref().and_then(|v| v.get::<CrucItem>());
                        if mode != DropTipMode::Out {
                            if let Some(item) = item {
                                let r#mod = item.r#mod.r#mod.clone();
                                // L3951
                                for line in &r#mod.lines {
                                    // L3952
                                    let line = check_line(cx, line);
                                    tooltip.add_line(cx, 16.0, &format!("^7{line}"));
                                }
                                // L3954
                                super::display_item::AddModComparisonTooltip(
                                    cx,
                                    me,
                                    tooltip,
                                    Some(&r#mod),
                                    false,
                                );
                            }
                        }
                    }));
                // L3957
                y += 22.0;
            }
            // populate dropdowns with item mods
            // L3960 ORDER: (c), module notes
            for (nodeId, defaultOrder) in &nodeSelections {
                let k = location_index(*nodeId);
                // L3961 ORDER: (c)
                for (index, r#mod) in modList[k - 1].iter().enumerate() {
                    // L3962
                    if r#mod.defaultOrder() == Some(*defaultOrder) {
                        // L3963
                        let c = ctl(cx, p, format!("modSelectNode{}", tostring_num(*nodeId)));
                        cx.w_mut::<DropDownControl>(c).sel_index = index + 1;
                    }
                }
            }
            // L3967
            let ml = modList.clone();
            let save = cx
                .add(
                    p,
                    "save",
                    ButtonControl::new(Anchor::NONE, rect(-45, 157, 80, 20), "Add", move |cx| {
                        // L3968
                        let item = crucible_add_modifier(cx, me, p, &ml);
                        super::display_item::SetDisplayItem(cx, me, Some(item));
                        // L3969
                        close_popup(cx);
                    }),
                )
                .id();
            // L3971
            let ml = modList.clone();
            cx.w_mut::<ButtonControl>(save).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, ()| {
                    // L3972
                    tooltip.clear(false);
                    // L3973
                    let item = crucible_add_modifier(cx, me, p, &ml);
                    let f = items_hooks().add_item_tooltip;
                    let b = cx.build().model.clone();
                    f(cx, &b, tooltip, &item, None, false);
                }));
            // L3975
            cx.add(
                p,
                "close",
                ButtonControl::new(Anchor::NONE, rect(45, 157, 80, 20), "Cancel", |cx| {
                    // L3976
                    close_popup(cx);
                }),
            );
        },
        PopupOpts {
            enter: Some("save".into()),
            ..PopupOpts::default()
        },
    );
}

/// `local function addModifier()` of the crucible popup (L3918-3938).
fn crucible_add_modifier(
    cx: &mut Cx,
    me: CtrlId,
    p: CtrlId,
    modList: &[Vec<CrucEntry>; 5],
) -> model::ItemRef {
    let displayItem = di(cx, me);
    // L3919
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L3920
    item.id.set(displayItem.id.get());
    // L3921
    item.crucibleModLines.set(model::Shared::new(Vec::new()));
    // L3922-3928
    let listMod: Vec<Option<CrucEntry>> = (1..=5)
        .map(|k| {
            let sel = cx
                .w::<DropDownControl>(ctl(cx, p, format!("modSelectNode{k}")))
                .sel_index;
            modList[k - 1].get(sel.wrapping_sub(1)).cloned()
        })
        .collect();
    // L3929 (ipairs stops at the first nil)
    for nodeMod in listMod.into_iter().map_while(|e| e) {
        // L3930
        if let CrucEntry::Mod(nodeMod) = nodeMod {
            // L3931
            for line in &nodeMod.r#mod.r#mod.lines {
                // L3932
                let t = Table::new();
                t.set(n!("line"), check_line(cx, line));
                t.set(n!("modTags"), mod_tags(&nodeMod.r#mod.r#mod));
                set_flag(&t, Some(n!("crucible")));
                item.crucibleModLines.get().push(t);
            }
        }
    }
    // L3936
    item.BuildAndParseRaw();
    // L3937
    item
}

// ---- AddItemSetTooltip ----------------------------------------------------------------------

/// lua: ItemsTab.lua:3982-3991 `ItemsTabClass:AddItemSetTooltip(tooltip, itemSet)`.
/// @pob Classes/ItemsTab.lua:3982-3991 h=f664e16b
pub fn AddItemSetTooltip(cx: &mut Cx, tooltip: &mut Tooltip, itemSet: &Table) {
    let m = cx.build().model.clone();
    // L3983
    for slot in m.itemsTab.orderedSlots.snapshot() {
        // L3984
        if !slot.truthy(n!("nodeId")) {
            // L3985 (`itemSet[slot.slotName]`: a nil slot name or entry raises)
            let slotName = slot
                .str(n!("slotName"))
                .unwrap_or_else(|| lua_error!("table index is nil"));
            let entry = itemSet
                .table(Name::from(&slotName))
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            let item = super::slot::item_by_id(cx, &entry.get(n!("selItemId")));
            // L3986
            if let Some(item) = item {
                // L3987
                let label = slot.str(n!("label")).unwrap_or_else(|| {
                    lua_error!("bad argument #2 to 'format' (string expected, got nil)")
                });
                let rarity = item.rarity.get();
                let cc = cx.main.cc.get(&rarity).cloned().unwrap_or_else(|| {
                    lua_error!("bad argument #3 to 'format' (string expected, got nil)")
                });
                let line = lfmt!("^7%s: %s%s", &*label, &cc, &*item.name.get());
                tooltip.add_line(cx, 16.0, &line);
            }
        }
    }
}
