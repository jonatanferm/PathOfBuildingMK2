//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/AddImplicitPopup.lua 1-414 (the
//! "Add Implicit to Item" popup, `addImplicit.AddImplicitToDisplayItem(itemsTab, displayItem)`).
//! Owner: U36.
//!
//! @pob-file Modules/AddImplicitPopup.lua @16de4b82d
//!
//! The module's one function is reached from the constructor's "Add Implicit..." button
//! (ItemsTab.lua:571-573, U33) through U36's `ItemsUiHooks::add_implicit_to_display_item` entry.
//! `itemsTab:SetDisplayItem` and `itemsTab:AddModComparisonTooltip` (U35) are called directly
//! (`display_item::AddModComparisonTooltip` with `replaceImplicits`, CCR-0124 §2);
//! `itemsTab:AddItemTooltip` (U38) goes through U32's `ItemsTabHooks::add_item_tooltip`.
//!
//! Lua semantics:
//! * `modGroups` and the inner lists of `modList` are the dropdowns' lists (`modGroupSelect.list
//!   = modGroups`; `modSelect.list = modList[i]`, a direct store). The Rust dropdowns own their
//!   `Vec`s, so the popup keeps the Lua tables in [`ImpState`] (`modSelectList` is the table
//!   `modSelect.list` names) and copies them into the dropdowns after every mutation
//!   ([`sync`]), before anything reads the dropdowns.
//! * The entries are Lua tables compared by identity and decorated by the sort
//!   (`defaultSortOrder`, `sortValue`, `sortValues`): [`ImplicitEntry`] behind an `Rc`, with
//!   `Cell` fields. A Vestigial entry is both its group and its only list mod (L156 inserts
//!   `elem[1]` into `modGroups`), so it is one `Rc` in both lists, and setDefaultSortOrder's
//!   second loop overwrites the group's `defaultSortOrder` with 1, as in Lua.
//! * `copyTable(data.itemMods.Vestigial[id])` (L132): the copy is only read, so the data mod is
//!   shared (`ItemModRef`), as is `mod.modTags` (a fresh table per mod line, as U35's
//!   `mod_tags`).
//! * DIVERGENCE(pending-calc): `GetMiscCalculator()`/`calcFunc(..)` only return Pending on wasm
//!   (ui doc §10.4); the power sort then sorts by 0. Never on desktop.
//!
//! ORDER:
//! * L50 `pairs(displayItem.affixes)` (string keys: per-process random in PoB; the port walks
//!   the dump's bytewise id order). The groups (L56) and their list mods are sorted afterwards
//!   (L73-86, L88-116), and the group labels are recomputed from each sorted list's first entry
//!   (L118-120), so the dropdown contents and order are deterministic (the L73-86 group sort
//!   keys on the group `mod`'s statOrder and level, which every tier of a group shares in the
//!   dump's Exarch/Eater groups), unless two distinct groups tie on statOrder and level: their
//!   order then follows `pairs` (and `table.sort`'s unstable tie handling). A group's `mod` (and
//!   its `defaultOrder`) is the first mod of the group that `pairs` reached. That `mod` is what
//!   the Type dropdown's tooltip lists and compares (L359, L365): in PoB those tooltip lines
//!   depend on the process's hash seed whenever a group has several mods (a group's base,
//!   Unique and Pinnacle presence tiers). Resolution (b) for that tooltip; not covered by the u36
//!   scenarios, which hover the Modifier dropdown of an Eldritch source instead. (Observed: a
//!   draft of u36's f_add_implicit left the mouse on the Type box; PoB's tooltip listed the
//!   "While a Unique Enemy is in your Presence" tier of the spell physical damage group, the
//!   port's the base tier; the dropdown entries themselves agreed.)
//! * L122 `pairs(data.vestigialModMappings)`: the list is sorted by mod id at L145-152 (a total
//!   order: the ids are distinct), so the order is unobservable (c).
//! * L87 `pairs(modList)`, L118 `pairs(modGroups)`: per-entry work, order unobservable (c).
//!
//! Props with side effects (ui doc §7.3): L347-352 (`modGroupSelectLabel.shown` writes
//! `controls.modSelectLabel.y = 45`): the label's `y` is a `Prop` reading a shared `Cell`
//! (`ImpState::modSelectLabelY`), which the shown function and the source dropdown's selFunc
//! (L313, L322) write.
#![allow(non_snake_case, clippy::too_many_lines)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_lua::math::m_max;
use pob_lua::Name;
use pob_ui_core::controls::drop_down::{DropTipMode, SelFunc};
use pob_ui_core::controls::search_host::SearchHost;
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;

use super::ctor::PowerSortValue;
use super::slot_helper::items_hooks;
use crate::app::CxApp;
use crate::calc::facade::MiscCalc;
use crate::model::{self, gamedata, Table};

// @pob Modules/AddImplicitPopup.lua:1-3 h=8c73e5bb
// (`local m_max = math.max`, `local t_insert = table.insert`: `pob_lua::math`, `Vec::push`)

type ItemModRef = gamedata::schema::affixes::ItemModRef;
type PowerStat = gamedata::schema::item_data::PowerStat;

/// `buildModSortList()` (L6-16): the "Sort by" list and `sortStats[stat] = entry` (the same
/// function as ItemsTab.lua:76-87, a module-local copy).
/// @pob Modules/AddImplicitPopup.lua:4-16 h=15759335
fn buildModSortList() -> (Vec<DropItem>, indexmap::IndexMap<Name, &'static PowerStat>) {
    // L4 `local M = {}`: the module is this file
    // L7
    let mut sortList = vec![DropItem::new("Default", Rc::new(PowerSortValue(None)))];
    // L8
    let mut sortStats = indexmap::IndexMap::new();
    // L9
    for entry in &model::data().powerStatList.stats {
        // L10
        if let Some(stat) = entry.stat.filter(|_| !entry.ignoreForNodes) {
            // L11
            sortList.push(DropItem::new(
                entry.label.to_string(),
                Rc::new(PowerSortValue(Some(entry))),
            ));
            // L12
            sortStats.insert(stat, entry);
        }
    }
    // L15
    (sortList, sortStats)
}

/// A `sourceList` entry (`{ label, sourceId }`).
#[derive(Clone, Debug)]
pub struct ImplicitSource {
    pub label: String,
    pub sourceId: &'static str,
}

/// A `modGroups` / `modList[i]` entry (L56-61, L64-70, L134-141): the union of the three Lua
/// table shapes; absent fields are `None`.
#[derive(Debug)]
pub struct ImplicitEntry {
    /// `label` (a group's is recomputed at L119)
    pub label: RefCell<String>,
    /// `mod`
    pub r#mod: ItemModRef,
    /// `modListIndex` (groups and Vestigial entries)
    pub modListIndex: Cell<Option<usize>>,
    /// `defaultOrder` (the `pairs` key: the affix id; Eldritch entries)
    pub defaultOrder: Option<Name>,
    /// `affixType` (`mod.type`; Eldritch list mods)
    pub affixType: Option<Name>,
    /// `type` (`"exarch"`, `"eater"`, `"vestigial"`; nil for an Eldritch group)
    pub r#type: Option<&'static str>,
    /// `uniqueTitle` (Vestigial)
    pub uniqueTitle: Option<String>,
    /// `id` (Vestigial: the `vestigialModMappings` key)
    pub id: Option<Name>,
    /// `defaultSortOrder`
    pub defaultSortOrder: Cell<Option<f64>>,
    /// `sortValue`
    pub sortValue: Cell<Option<f64>>,
    /// `sortValues[stat]` (`None`: nil)
    pub sortValues: RefCell<Option<Vec<(Name, f64)>>>,
}

type Entry = Rc<ImplicitEntry>;
/// A Lua list of entries shared by reference (`modList[i]`, `modSelect.list`).
type EntryList = Rc<RefCell<Vec<Entry>>>;

/// The locals of one popup that its closures share.
struct ImpState {
    /// `displayItem` (the argument)
    displayItem: model::ItemRef,
    /// `sourceList`
    sourceList: Vec<ImplicitSource>,
    /// `modList`
    modList: Vec<EntryList>,
    /// `modGroups`
    modGroups: Vec<Entry>,
    /// `sortStats`
    sortStats: indexmap::IndexMap<Name, &'static PowerStat>,
    /// the table `controls.modSelect.list` names
    modSelectList: EntryList,
    /// `controls.modSelectLabel.y` (written by a `shown` function, ui doc §7.3)
    modSelectLabelY: Rc<Cell<f64>>,
}

type Imp = Rc<RefCell<ImpState>>;

/// `controls.<key>` of the popup (nil raises when indexed).
#[track_caller]
fn pc(cx: &Cx, p: CtrlId, key: &'static str) -> CtrlId {
    cx.child(p, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

fn sel_index(cx: &Cx, drop: CtrlId) -> usize {
    cx.w::<DropDownControl>(drop).sel_index
}

fn dd_search(cx: &mut Cx, me: CtrlId) -> &mut SearchHost {
    &mut cx.w_mut::<DropDownControl>(me).search
}

/// `drop:UpdateSearch()`
fn update_search(cx: &mut Cx, drop: CtrlId) {
    SearchHost::update_search(cx, drop, dd_search);
}

fn entry_items(list: &[Entry]) -> Vec<DropItem> {
    list.iter()
        .map(|e| DropItem::new(e.label.borrow().clone(), e.clone()))
        .collect()
}

/// Copies `modGroups` and `modSelect.list` into their dropdowns (see the module docs).
fn sync(cx: &mut Cx, p: CtrlId, s: &Imp) {
    let (groups, sel) = {
        let st = s.borrow();
        let sel = st.modSelectList.borrow().clone();
        (entry_items(&st.modGroups), entry_items(&sel))
    };
    if let Some(c) = cx.child(p, "modGroupSelect") {
        cx.w_mut::<DropDownControl>(c).list = groups;
    }
    if let Some(c) = cx.child(p, "modSelect") {
        cx.w_mut::<DropDownControl>(c).list = sel;
    }
}

/// `sourceList[controls.source.selIndex].sourceId` (nil entry raises when indexed).
#[track_caller]
fn cur_source(cx: &Cx, p: CtrlId, s: &Imp) -> &'static str {
    let i = sel_index(cx, pc(cx, p, "source"));
    s.borrow()
        .sourceList
        .get(i.wrapping_sub(1))
        .map(|x| x.sourceId)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// `modList[i]` (nil → `None`).
fn mod_list(st: &ImpState, i: Option<usize>) -> Option<EntryList> {
    i.and_then(|i| st.modList.get(i.wrapping_sub(1)).cloned())
}

/// `modList[modGroups[k].modListIndex]` (`modGroups[k]` nil raises when indexed).
#[track_caller]
fn group_list(st: &ImpState, k: usize) -> Option<EntryList> {
    let g = st
        .modGroups
        .get(k.wrapping_sub(1))
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    mod_list(st, g.modListIndex.get())
}

/// `control.list = t` for `modSelect` with a nil `t`: an empty list (no entry to read).
fn set_mod_select(s: &Imp, l: Option<EntryList>) {
    s.borrow_mut().modSelectList = l.unwrap_or_default();
}

/// `mod.statOrder[i] ~= .. / <` operands (nil raises in the comparison).
#[track_caller]
fn stat_order(m: &gamedata::schema::affixes::ItemMod, i: usize) -> Option<f64> {
    m.statOrder.get(i - 1).copied()
}

#[track_caller]
fn lt_num(a: Option<f64>, b: Option<f64>) -> bool {
    match (a, b) {
        (Some(a), Some(b)) => a < b,
        _ => lua_error!("attempt to compare nil with number"),
    }
}

/// `mod.level` in a comparison (nil raises).
#[track_caller]
fn level(m: &gamedata::schema::affixes::ItemMod) -> f64 {
    m.level
        .unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
}

/// `mod.group` that Lua indexes (`:gsub`, `:match`; nil raises).
#[track_caller]
fn group_of(m: &gamedata::schema::affixes::ItemMod) -> Name {
    m.group
        .unwrap_or_else(|| lua_error!("attempt to index field 'group' (a nil value)"))
}

/// `tonumber(a.defaultOrder:match("%d+$"))` (a nil operand raises in the comparison).
fn default_order_num(e: &ImplicitEntry) -> Option<f64> {
    let d = e
        .defaultOrder
        .unwrap_or_else(|| lua_error!("attempt to index field 'defaultOrder' (a nil value)"));
    pob_ui_core::luastr::lmatch(&d, "%d+$").and_then(|s| pob_lua::tonumber(&s))
}

/// A fresh `modTags` table of a mod line (`modTags = listMod.mod.modTags`).
fn mod_tags(m: &gamedata::schema::affixes::ItemMod) -> Table {
    let t = Table::new();
    for tag in &m.modTags {
        t.push(*tag);
    }
    t
}

/// `setDefaultSortOrder()` (L30-42).
fn setDefaultSortOrder(st: &ImpState) {
    // L31
    for (groupIndex, group) in st.modGroups.iter().enumerate() {
        // L32
        group.defaultSortOrder.set(Some((groupIndex + 1) as f64));
        // L33
        group.sortValue.set(None);
    }
    // L35
    for listMods in &st.modList {
        // L36
        for (index, listMod) in listMods.borrow().iter().enumerate() {
            // L37
            listMod.defaultSortOrder.set(Some((index + 1) as f64));
            // L38
            listMod.sortValue.set(None);
            // L39
            *listMod.sortValues.borrow_mut() = None;
        }
    }
}

/// `buildMods(sourceId)` (L45-160): mutates modList to contain mods from the specified source.
fn buildMods(cx: &Cx, s: &Imp, sourceId: &str) {
    let mut st = s.borrow_mut();
    let displayItem = st.displayItem.clone();
    // L46
    st.modList.clear();
    // L47
    st.modGroups.clear();
    // L48
    let mut groupIndexes: Vec<(Name, usize)> = Vec::new();
    // L49
    if sourceId == "EXARCH" || sourceId == "EATER" {
        let affixes = displayItem
            .affixes_table()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
        let sourceLower = sourceId.to_ascii_lowercase();
        // L50 (ORDER: see the module docs)
        for (i, r#mod) in &affixes.0 {
            // L51
            let ty = || {
                r#mod
                    .r#type
                    .unwrap_or_else(|| lua_error!("attempt to index field 'type' (a nil value)"))
            };
            if displayItem.GetModSpawnWeight(r#mod, None, None, None) > 0.0
                && sourceLower == ty().to_ascii_lowercase()
            {
                let ty = ty();
                // L52
                let modLabel = r#mod
                    .lines
                    .iter()
                    .map(|l| &**l)
                    .collect::<Vec<_>>()
                    .join("/");
                // L53
                let group = pob_ui_core::luastr::gsub(
                    &pob_ui_core::luastr::gsub(&group_of(r#mod), "PinnaclePresence", ""),
                    "UniquePresence",
                    "",
                );
                let group = Name::from(group.as_str());
                // L54
                if !groupIndexes.iter().any(|(g, _)| *g == group) {
                    // L55
                    st.modList.push(Rc::new(RefCell::new(Vec::new())));
                    // L56
                    let n = st.modList.len();
                    st.modGroups.push(Rc::new(ImplicitEntry {
                        // L57
                        label: RefCell::new(modLabel.clone()),
                        // L58
                        r#mod: r#mod.clone(),
                        // L59
                        modListIndex: Cell::new(Some(n)),
                        // L60
                        defaultOrder: Some(*i),
                        affixType: None,
                        r#type: None,
                        uniqueTitle: None,
                        id: None,
                        defaultSortOrder: Cell::new(None),
                        sortValue: Cell::new(None),
                        sortValues: RefCell::new(None),
                    }));
                    // L62
                    let g = st.modGroups.len();
                    groupIndexes.push((group, g));
                }
                // L64
                let gi = groupIndexes
                    .iter()
                    .find(|(g, _)| *g == group)
                    .map(|(_, k)| *k)
                    .expect("set above");
                let typ: &'static str = if sourceId == "EXARCH" {
                    "exarch"
                } else {
                    "eater"
                };
                st.modList[gi - 1].borrow_mut().push(Rc::new(ImplicitEntry {
                    // L65
                    label: RefCell::new(modLabel),
                    // L66
                    r#mod: r#mod.clone(),
                    modListIndex: Cell::new(None),
                    // L69
                    defaultOrder: Some(*i),
                    // L67
                    affixType: Some(ty),
                    // L68
                    r#type: Some(typ),
                    uniqueTitle: None,
                    id: None,
                    defaultSortOrder: Cell::new(None),
                    sortValue: Cell::new(None),
                    sortValues: RefCell::new(None),
                }));
            }
        }
        // L73
        pob_lua::sort(&mut st.modGroups, |a, b| {
            // L74
            let modA = &a.r#mod;
            // L75
            let modB = &b.r#mod;
            // L76
            let n = m_max(modA.lines.len() as f64, modB.lines.len() as f64) as usize;
            for i in 1..=n {
                // L77
                if modA.lines.get(i - 1).is_none() {
                    // L78
                    return true;
                } else if modB.lines.get(i - 1).is_none() {
                    // L80
                    return false;
                } else if stat_order(modA, i) != stat_order(modB, i) {
                    // L82
                    return lt_num(stat_order(modA, i), stat_order(modB, i));
                }
            }
            // L85
            level(modA) > level(modB)
        });
        // L87 (ORDER: unobservable)
        let lists = st.modList.clone();
        for list in &lists {
            // L88
            pob_lua::sort(&mut list.borrow_mut(), |a, b| {
                // L89
                let modA = &a.r#mod;
                // L90
                let modB = &b.r#mod;
                // L91
                if modA.group != modB.group {
                    let (ga, gb) = (group_of(modA), group_of(modB));
                    // L92
                    if pob_ui_core::luastr::lmatch(&ga, "PinnaclePresence").is_some() {
                        // L93
                        return false;
                    } else if pob_ui_core::luastr::lmatch(&gb, "PinnaclePresence").is_some() {
                        // L95
                        return true;
                    } else if pob_ui_core::luastr::lmatch(&ga, "UniquePresence").is_some() {
                        // L97
                        return false;
                    }
                    // L99
                    return true;
                }
                // L102
                let n = m_max(modA.lines.len() as f64, modB.lines.len() as f64) as usize;
                if n >= 1 {
                    let j = 1;
                    // L103
                    if modA.lines.get(j - 1).is_none() {
                        // L104
                        return true;
                    } else if modB.lines.get(j - 1).is_none() {
                        // L106
                        return false;
                    } else if stat_order(modA, j) != stat_order(modB, j) {
                        // L108
                        return lt_num(stat_order(modA, j), stat_order(modB, j));
                    }
                    // L110
                    let modAVal = default_order_num(a);
                    // L111
                    let modBVal = default_order_num(b);
                    // L112 (every path of the loop body returns at j = 1)
                    return lt_num(modAVal, modBVal);
                }
                // L115
                level(modA) > level(modB)
            });
        }
        // L118 (ORDER: unobservable)
        for group in &st.modGroups {
            // L119
            let first = mod_list(&st, group.modListIndex.get())
                .and_then(|l| l.borrow().first().cloned())
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            let label = first.label.borrow().clone();
            let label = pob_ui_core::luastr::gsub(&label, "%([%d%.]+%-[%d%.]+%)", "#");
            let label = pob_ui_core::luastr::gsub(&label, "[%d%.]+", "#");
            *group.label.borrow_mut() = label;
        }
    } else if sourceId == "VESTIGIAL" {
        let vestigial = model::data().itemMods.get(&Name::from("Vestigial"));
        let gamedata::DataValue::Table(mappings) = model::data().resolve(
            model::data()
                .other
                .get("data.vestigialModMappings")
                .unwrap_or(&gamedata::DataValue::Nil),
        ) else {
            lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
        };
        // L122 (ORDER: sorted below)
        for (id, uniqueTitle) in &mappings.fields {
            let gamedata::DataValue::Str(uniqueTitle) = model::data().resolve(uniqueTitle) else {
                lua_error!("attempt to index local 'uniqueTitle' (a non-string value)")
            };
            // L123
            let titleLower = uniqueTitle.to_ascii_lowercase();
            // L124
            let unique = cx.app().main.unique_db.by_title.get(&titleLower).cloned();
            // L125-129
            let skip = match &unique {
                None => true,
                Some(unique) => {
                    let ut = unique
                        .base
                        .get()
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index field 'base' (a nil value)")
                        })
                        .r#type;
                    ut != displayItem.r#type.get()
                        || displayItem
                            .title
                            .get()
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index field 'title' (a nil value)")
                            })
                            .to_ascii_lowercase()
                            == titleLower
                }
            };
            if skip {
                // L130
                continue;
            }
            // L132
            let id = Name::from(id);
            let r#mod = vestigial
                .and_then(|t| t.0.get(&id))
                .cloned()
                .unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
                });
            // L133
            let modLabel = format!(
                "{}{}",
                cx.main.cc.VESTIGIAL,
                r#mod
                    .lines
                    .iter()
                    .map(|l| &**l)
                    .collect::<Vec<_>>()
                    .join("/")
            );
            // L134
            let n = st.modList.len();
            st.modList
                .push(Rc::new(RefCell::new(vec![Rc::new(ImplicitEntry {
                    // L135
                    label: RefCell::new(modLabel),
                    // L136
                    r#mod: r#mod.clone(),
                    // L139
                    modListIndex: Cell::new(Some(n + 1)),
                    defaultOrder: None,
                    affixType: None,
                    // L137
                    r#type: Some("vestigial"),
                    // L138
                    uniqueTitle: Some(uniqueTitle.to_string()),
                    // L140
                    id: Some(id),
                    defaultSortOrder: Cell::new(None),
                    sortValue: Cell::new(None),
                    sortValues: RefCell::new(None),
                })])));
            // L142 groupIndexes[mod.group] = #modGroups (never read)
        }
        // L145
        pob_lua::sort(&mut st.modList, |a, b| {
            // L146
            let a = a.borrow()[0].clone();
            // L147
            let b = b.borrow()[0].clone();
            // L148 (`group` is no field of these tables: nil == nil)
            // L149
            a.id.unwrap_or_default().as_bytes() < b.id.unwrap_or_default().as_bytes()
        });
        // L153
        let lists = st.modList.clone();
        for (i, elem) in lists.iter().enumerate() {
            // L154
            let group = elem.borrow()[0].clone();
            // L155
            group.modListIndex.set(Some(i + 1));
            // L156
            st.modGroups.push(group);
        }
    }
    // L159
    setDefaultSortOrder(&st);
}

/// `applyCandidateMod(item, listMod)` (L186-211): applies a candidate listMod to the item,
/// mirroring the mutation addModifier() performs at click-time. Eldritch (exarch/eater) sources
/// replace an existing implicit of the same type when present; other sources append.
fn applyCandidateMod(item: &model::ItemRef, listMod: &ImplicitEntry) {
    let ty = listMod.r#type;
    // L187
    if ty == Some("exarch") || ty == Some("eater") {
        let ty = Name::from(ty.unwrap_or_default());
        // L188-194
        let lines = item.implicitModLines.get();
        let index = lines
            .snapshot()
            .iter()
            .position(|implicitMod| implicitMod.truthy(ty))
            .map(|i| i + 1);
        // L195
        if let Some(index) = index {
            // L196
            for (i, line) in listMod.r#mod.lines.iter().enumerate() {
                // L197
                let t = Table::new();
                t.set(Name::from("line"), &**line);
                t.set(Name::from("modTags"), mod_tags(&listMod.r#mod));
                t.set(ty, true);
                item.implicitModLines.get().set(index + i, t);
            }
            // L199
            return;
        }
    } else if ty == Some("vestigial") {
        // L202
        item.implicitModLines.set(model::Shared::new(Vec::new()));
    }
    // L204
    for line in &listMod.r#mod.lines {
        // L205
        let modLine = Table::new();
        modLine.set(Name::from("line"), &**line);
        modLine.set(Name::from("modTags"), mod_tags(&listMod.r#mod));
        // L206
        if let Some(ty) = ty {
            // L207
            modLine.set(Name::from(ty), true);
        }
        // L209
        item.implicitModLines.get().push(modLine);
    }
}

/// `self.build.calcsTab:GetMiscCalculator()` (`None` = Pending, wasm only).
fn misc_calc(cx: &mut Cx) -> Option<Rc<dyn MiscCalc>> {
    super::craft_a::misc_calc(cx).map(|(f, _)| f)
}

/// `getSortValue(listMod, stat, calcFunc, slotName, useFullDPS)` (L212-225).
fn getSortValue(
    s: &Imp,
    listMod: &ImplicitEntry,
    stat: Name,
    calcFunc: Option<&Rc<dyn MiscCalc>>,
    slotName: Name,
    useFullDPS: bool,
) -> f64 {
    // L213
    if listMod.sortValues.borrow().is_none() {
        *listMod.sortValues.borrow_mut() = Some(Vec::new());
    }
    // L214
    let cached = listMod
        .sortValues
        .borrow()
        .as_ref()
        .and_then(|v| v.iter().find(|(k, _)| *k == stat).map(|(_, x)| *x));
    if let Some(v) = cached {
        // L215
        return v;
    }
    let displayItem = s.borrow().displayItem.clone();
    // L217
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L218
    item.id.set(displayItem.id.get());
    // L219
    applyCandidateMod(&item, listMod);
    // L220
    item.BuildAndParseRaw();
    // L221
    let output =
        calcFunc.and_then(|f| super::craft_a::calc_rep(f, slotName, &item, Some(useFullDPS)));
    // L222
    let statEntry = s.borrow().sortStats.get(&stat).copied();
    let value = match (output, statEntry) {
        (Some(o), Some(e)) => pob_calc::modules::power::GetFromOutput(
            &o,
            &pob_calc::modules::power::powerStat_table(e),
            false,
        ),
        (Some(_), None) => lua_error!("attempt to index local 'statTable' (a nil value)"),
        // DIVERGENCE(pending-calc)
        (None, _) => 0.0,
    };
    // L223
    listMod
        .sortValues
        .borrow_mut()
        .get_or_insert_with(Vec::new)
        .push((stat, value));
    // L224
    value
}

/// `(a.defaultSortOrder or 0) < (b.defaultSortOrder or 0)`
fn by_default(a: &ImplicitEntry, b: &ImplicitEntry) -> bool {
    a.defaultSortOrder.get().unwrap_or(0.0) < b.defaultSortOrder.get().unwrap_or(0.0)
}

/// `a.sortValue` in a comparison (nil raises).
#[track_caller]
fn sv(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to compare nil with number"))
}

/// The power comparator of L240-245 / L256-261.
fn by_power(a: &ImplicitEntry, b: &ImplicitEntry) -> bool {
    if a.sortValue.get() != b.sortValue.get() {
        return sv(a.sortValue.get()) > sv(b.sortValue.get());
    }
    by_default(a, b)
}

/// `applySort(stat, selectFirst)` (L226-295).
fn applySort(cx: &mut Cx, p: CtrlId, s: &Imp, stat: Option<Name>, selectFirst: bool) {
    // L227
    let (Some(modSelect), Some(modGroupSelect)) =
        (cx.child(p, "modSelect"), cx.child(p, "modGroupSelect"))
    else {
        // L228
        return;
    };
    if !cx.is_shown(modGroupSelect) {
        return;
    }
    // L230
    let selectedGroup = if selectFirst {
        None
    } else {
        s.borrow()
            .modGroups
            .get(sel_index(cx, modGroupSelect).wrapping_sub(1))
            .cloned()
    };
    // L231
    let selectedMod = if selectFirst {
        None
    } else {
        s.borrow()
            .modSelectList
            .borrow()
            .get(sel_index(cx, modSelect).wrapping_sub(1))
            .cloned()
    };
    // L232
    if let Some(stat) = stat {
        // L233
        let slotName = s.borrow().displayItem.GetPrimarySlot();
        // L234
        let calcFunc = misc_calc(cx);
        // L235
        let useFullDPS = &*stat == "FullDPS";
        // L236
        let lists = s.borrow().modList.clone();
        for listMods in &lists {
            // L237
            let mods = listMods.borrow().clone();
            for listMod in &mods {
                // L238
                let v = getSortValue(s, listMod, stat, calcFunc.as_ref(), slotName, useFullDPS);
                listMod.sortValue.set(Some(v));
            }
            // L240
            pob_lua::sort(&mut listMods.borrow_mut(), |a, b| by_power(a, b));
        }
        // L247
        let groups = s.borrow().modGroups.clone();
        for group in &groups {
            // L248
            let mut best: Option<f64> = None;
            // L249
            let list = mod_list(&s.borrow(), group.modListIndex.get());
            for listMod in list.map(|l| l.borrow().clone()).unwrap_or_default() {
                // L250
                if best.is_none() || sv(listMod.sortValue.get()) > best.unwrap_or_default() {
                    // L251
                    best = listMod.sortValue.get();
                }
            }
            // L254
            group.sortValue.set(Some(best.unwrap_or(0.0)));
        }
        // L256
        pob_lua::sort(&mut s.borrow_mut().modGroups, |a, b| by_power(a, b));
    } else {
        // L263
        let lists = s.borrow().modList.clone();
        for listMods in &lists {
            // L264
            pob_lua::sort(&mut listMods.borrow_mut(), |a, b| by_default(a, b));
        }
        // L268
        pob_lua::sort(&mut s.borrow_mut().modGroups, |a, b| by_default(a, b));
    }
    sync(cx, p, s);
    // L272
    update_search(cx, modGroupSelect);
    // L273
    if let Some(selectedGroup) = selectedGroup {
        // L274
        let found = s
            .borrow()
            .modGroups
            .iter()
            .position(|g| Rc::ptr_eq(g, &selectedGroup));
        if let Some(index) = found {
            // L276
            cx.w_mut::<DropDownControl>(modGroupSelect).sel_index = index + 1;
        }
    } else {
        // L281
        DropDownControl::set_sel(cx, modGroupSelect, 1, true);
    }
    // L283
    let l = group_list(&s.borrow(), sel_index(cx, modGroupSelect));
    set_mod_select(s, l);
    sync(cx, p, s);
    // L284
    update_search(cx, modSelect);
    // L285
    if let Some(selectedMod) = selectedMod {
        // L286
        let found = s
            .borrow()
            .modSelectList
            .borrow()
            .iter()
            .position(|m| Rc::ptr_eq(m, &selectedMod));
        if let Some(index) = found {
            // L288
            cx.w_mut::<DropDownControl>(modSelect).sel_index = index + 1;
        }
    } else {
        // L293
        DropDownControl::set_sel(cx, modSelect, 1, true);
    }
}

/// `addModifier()` (L296-309).
fn addModifier(cx: &Cx, p: CtrlId, s: &Imp) -> model::ItemRef {
    let displayItem = s.borrow().displayItem.clone();
    // L297
    let item = crate::main_mode::new_item(Some(&displayItem.BuildRaw()), None, false);
    // L298
    item.id.set(displayItem.id.get());
    // L299
    let sourceId = cur_source(cx, p, s);
    // L300
    if sourceId == "CUSTOM" {
        // L301
        let buf = cx.w::<EditControl>(pc(cx, p, "custom")).buf.clone();
        if pob_ui_core::luastr::lmatch(&buf, "%S").is_some() {
            // L302
            let t = Table::new();
            t.set(Name::from("line"), buf.as_str());
            t.set(Name::from("custom"), true);
            item.implicitModLines.get().push(t);
        }
    } else {
        // L305
        let list = group_list(&s.borrow(), sel_index(cx, pc(cx, p, "modGroupSelect")))
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
        let listMod = list
            .borrow()
            .get(sel_index(cx, pc(cx, p, "modSelect")).wrapping_sub(1))
            .cloned()
            .unwrap_or_else(|| lua_error!("attempt to index local 'listMod' (a nil value)"));
        applyCandidateMod(&item, &listMod);
    }
    // L307
    item.BuildAndParseRaw();
    // L308
    item
}

/// `controls.sort.list[controls.sort.selIndex].stat`
fn sort_stat(cx: &Cx, sort: CtrlId) -> Option<Name> {
    DropDownControl::get_sel_value(cx, sort)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
        .get::<PowerSortValue>()
        .and_then(PowerSortValue::stat)
}

/// `value.mod`, `value.uniqueTitle` lines of the two tooltips (L359-364, L383-388).
fn add_mod_lines(cx: &Cx, tooltip: &mut Tooltip, value: &ImplicitEntry) {
    // L359 / L383
    for line in &value.r#mod.lines {
        // L360 / L384
        tooltip.add_line(cx, 16.0, &format!("^7{line}"));
    }
    // L362 / L386
    if let Some(t) = &value.uniqueTitle {
        // L363 / L387
        tooltip.add_line(cx, 16.0, &format!("^7Source: {}{t}^7", cx.main.cc.UNIQUE));
    }
}

/// lua: AddImplicitPopup.lua:21-412 `M.AddImplicitToDisplayItem(itemsTab, displayItem)`: opens
/// the custom Implicit popup. `me` is `itemsTab`.
/// @pob Modules/AddImplicitPopup.lua:18-412 h=fb99662d
pub fn AddImplicitToDisplayItem(cx: &mut Cx, me: CtrlId, displayItem: Option<model::ItemRef>) {
    // L22-25 controls, sourceList, modList, modGroups
    // L26
    let (sortList, sortStats) = buildModSortList();
    // L27
    let Some(displayItem) = displayItem else {
        return;
    };
    // L28 these closures should probably be refactored to be outside this function
    // L29 at some point
    let s: Imp = Rc::new(RefCell::new(ImpState {
        displayItem: displayItem.clone(),
        sourceList: Vec::new(),
        modList: Vec::new(),
        modGroups: Vec::new(),
        sortStats,
        modSelectList: Rc::default(),
        modSelectLabelY: Rc::new(Cell::new(70.0)),
    }));
    // L161
    let titleLower = displayItem.title.get().map(|t| t.to_ascii_lowercase());
    // L162
    let displayDBUnique = titleLower
        .as_ref()
        .and_then(|t| cx.app().main.unique_db.by_title.get(t).cloned());
    // L163
    if &*displayItem.rarity.get() == "UNIQUE" {
        if let Some(displayDBUnique) = &displayDBUnique {
            // L164
            let baseType = displayItem
                .base
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
                .r#type;
            let vestigialBase = match model::data().resolve(
                model::data()
                    .other
                    .get("data.vestigialUniqueBaseTypes")
                    .unwrap_or(&gamedata::DataValue::Nil),
            ) {
                gamedata::DataValue::Table(t) => !matches!(
                    model::data().resolve(t.field(&baseType)),
                    gamedata::DataValue::Nil | gamedata::DataValue::Bool(false)
                ),
                _ => lua_error!("attempt to index field 'vestigialUniqueBaseTypes' (a nil value)"),
            };
            // L165 the source field should mean that the item comes from a specific
            // L166 mechanic, which usually means it is not in the core drop pool
            // L167
            if vestigialBase
                && (!displayDBUnique.extra.truthy(Name::from("source"))
                    // L168 this is the only exception to the above, as it occasionally drops
                    // L169 instead of other body armours in legion
                    // L170
                    || titleLower.as_deref() == Some("stasis prison"))
            {
                // L171
                s.borrow_mut().sourceList.push(ImplicitSource {
                    label: "Vestigial".to_owned(),
                    sourceId: "VESTIGIAL",
                });
            }
        }
    }
    // L173
    let rarity = displayItem.rarity.get();
    let ty = displayItem.r#type.get();
    if (&*rarity != "UNIQUE" && &*rarity != "RELIC")
        && matches!(&*ty, "Helmet" | "Body Armour" | "Gloves" | "Boots")
    {
        // L174
        if displayItem.influences.truthy(Name::from("cleansing")) {
            // L175
            s.borrow_mut().sourceList.push(ImplicitSource {
                label: "Searing Exarch".to_owned(),
                sourceId: "EXARCH",
            });
        }
        // L177
        if displayItem.influences.truthy(Name::from("tangle")) {
            // L178
            s.borrow_mut().sourceList.push(ImplicitSource {
                label: "Eater of Worlds".to_owned(),
                sourceId: "EATER",
            });
        }
    }
    // L181
    s.borrow_mut().sourceList.push(ImplicitSource {
        label: "Custom".to_owned(),
        sourceId: "CUSTOM",
    });
    // L182
    let first = s.borrow().sourceList[0].sourceId;
    buildMods(cx, &s, first);
    // L183-211 applyCandidateMod: [`applyCandidateMod`]; L212-225 getSortValue; L226-295
    // applySort; L296-309 addModifier

    // L407-410 (`controls.modSelect.shown()` with `controls.source.selIndex` = 1: computed
    // before the dialog exists, as the popup's size is its first argument; the function is pure)
    let modSelectShown = !matches!(first, "CUSTOM" | "VESTIGIAL");
    let popupHeight = if modSelectShown { 130.0 } else { 110.0 };
    // L411
    let default = (first == "CUSTOM").then(|| CtrlKey::from("custom"));
    popups::open_popup(
        cx,
        710.0,
        popupHeight,
        "Add Implicit to Item",
        move |cx, p| {
            let p = p.id();
            // L310
            cx.add(
                p,
                "sourceLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 20, 0, 16),
                    "^7Source:",
                ),
            );
            // L311
            let s2 = s.clone();
            let sourceSel: SelFunc = Rc::new(move |cx, _index, value| {
                let value = value
                    .and_then(|v| v.get::<ImplicitSource>().cloned())
                    .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                // L312
                if value.sourceId != "CUSTOM" {
                    // L313
                    s2.borrow().modSelectLabelY.set(70.0);
                    // L314
                    buildMods(cx, &s2, value.sourceId);
                    sync(cx, p, &s2);
                    // L315
                    DropDownControl::set_sel(cx, pc(cx, p, "modGroupSelect"), 1, false);
                    // L316
                    let l = group_list(&s2.borrow(), 1);
                    set_mod_select(&s2, l);
                    sync(cx, p, &s2);
                    // L317
                    DropDownControl::set_sel(cx, pc(cx, p, "modSelect"), 1, false);
                    // L318
                    if let Some(sort) = cx.child(p, "sort") {
                        // L319
                        let stat = sort_stat(cx, sort);
                        applySort(cx, p, &s2, stat, true);
                    }
                } else {
                    // L322
                    s2.borrow().modSelectLabelY.set(45.0);
                }
            });
            let list = s
                .borrow()
                .sourceList
                .iter()
                .map(|x| DropItem::new(x.label.clone(), Rc::new(x.clone())))
                .collect();
            let nSources = s.borrow().sourceList.len();
            cx.add(
                p,
                "source",
                DropDownControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(100, 20, 150, 18),
                    list,
                    Some(sourceSel),
                    None,
                    false,
                )
                // L325
                .enabled(Prop::Val(nSources > 1)),
            );
            // L326
            let s2 = s.clone();
            cx.add(
                p,
                "sortLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(350, 20, 0, 16),
                    "^7Sort by:",
                )
                // L327
                .shown(Prop::f(move |cx, _| {
                    // L328
                    cur_source(cx, p, &s2) != "CUSTOM"
                })),
            );
            // L330
            let s2 = s.clone();
            let sortSel: SelFunc = Rc::new(move |cx, _index, value| {
                // L331
                let stat = value
                    .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"))
                    .get::<PowerSortValue>()
                    .and_then(PowerSortValue::stat);
                applySort(cx, p, &s2, stat, true);
            });
            let s2 = s.clone();
            cx.add(
                p,
                "sort",
                DropDownControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(355, 20, 240, 18),
                    sortList,
                    Some(sortSel),
                    None,
                    false,
                )
                // L333
                .shown(Prop::f(move |cx, _| {
                    // L334
                    cur_source(cx, p, &s2) != "CUSTOM"
                })),
            );
            // L336
            let s2 = s.clone();
            let y_cell = s.borrow().modSelectLabelY.clone();
            cx.add(
                p,
                "modGroupSelectLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    rect(95, 45, 0, 16),
                    Prop::f(move |cx, _| {
                        // L337
                        if cx.is_shown(pc(cx, p, "modSelect")) {
                            // L338
                            "^7Type:".to_owned()
                        } else {
                            // L340
                            "^7Modifier:".to_owned()
                        }
                    }),
                )
                // L347
                .shown(Prop::f(move |cx, _| {
                    // L348
                    if cur_source(cx, p, &s2) == "CUSTOM" {
                        // L349 (ui doc §7.3: a Cell)
                        y_cell.set(45.0);
                    }
                    // L351
                    cur_source(cx, p, &s2) != "CUSTOM"
                })),
            );
            // L343
            let s2 = s.clone();
            let groupSel: SelFunc = Rc::new(move |cx, _index, value| {
                let value = value
                    .and_then(|v| v.value.clone().downcast::<ImplicitEntry>().ok())
                    .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                // L344
                let l = mod_list(&s2.borrow(), value.modListIndex.get());
                set_mod_select(&s2, l);
                sync(cx, p, &s2);
                // L345
                DropDownControl::set_sel(cx, pc(cx, p, "modSelect"), 1, false);
            });
            let list = entry_items(&s.borrow().modGroups);
            let s2 = s.clone();
            let modGroupSelect = cx
                .add(
                    p,
                    "modGroupSelect",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 45, 600, 18),
                        list,
                        Some(groupSel),
                        None,
                        false,
                    )
                    // L353
                    .shown(Prop::f(move |cx, _| {
                        // L354
                        cur_source(cx, p, &s2) != "CUSTOM"
                    })),
                )
                .id();
            // L356
            cx.w_mut::<DropDownControl>(modGroupSelect).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
                    // L357
                    tooltip.clear(false);
                    // L358
                    let value =
                        value.and_then(|v| v.value.clone().downcast::<ImplicitEntry>().ok());
                    if let Some(value) = value.filter(|_| mode != DropTipMode::Out) {
                        // L359-364
                        add_mod_lines(cx, tooltip, &value);
                        // L365
                        super::display_item::AddModComparisonTooltip(
                            cx,
                            me,
                            tooltip,
                            Some(&value.r#mod),
                            value.r#type == Some("vestigial"),
                        );
                    }
                }));
            // L368
            let y_cell = s.borrow().modSelectLabelY.clone();
            let modSelectShown = move |cx: &Cx, s: &Imp| {
                // L377 (`modSelectHidden`, L370-375: CUSTOM; vestigial implicits aren't grouped
                // together, and the type selector will act as the mod selector)
                !matches!(cur_source(cx, p, s), "CUSTOM" | "VESTIGIAL")
            };
            let s2 = s.clone();
            cx.add(
                p,
                "modSelectLabel",
                LabelControl::new(
                    Anchor::nil(TopRight, TopLeft),
                    [
                        Prop::Val(95.0),
                        Prop::f(move |_, _| y_cell.get()),
                        Prop::Val(0.0),
                        Prop::Val(16.0),
                    ],
                    "^7Modifier:",
                )
                // L379
                .shown(Prop::f(move |cx, _| modSelectShown(cx, &s2))),
            );
            // L369
            let first = s.borrow().sourceList[0].sourceId;
            let initial = if first != "CUSTOM" {
                group_list(&s.borrow(), 1)
            } else {
                None
            };
            set_mod_select(&s, initial);
            let list = entry_items(&s.borrow().modSelectList.borrow());
            let s2 = s.clone();
            let modSelect = cx
                .add(
                    p,
                    "modSelect",
                    DropDownControl::new(
                        Anchor::nil(TopLeft, TopLeft),
                        rect(100, 70, 600, 18),
                        list,
                        None,
                        None,
                        false,
                    )
                    // L376
                    .shown(Prop::f(move |cx, _| modSelectShown(cx, &s2))),
                )
                .id();
            // L380
            cx.w_mut::<DropDownControl>(modSelect).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, (mode, _index, value)| {
                    // L381
                    tooltip.clear(false);
                    // L382
                    let value =
                        value.and_then(|v| v.value.clone().downcast::<ImplicitEntry>().ok());
                    if let Some(value) = value.filter(|_| mode != DropTipMode::Out) {
                        // L383-388
                        add_mod_lines(cx, tooltip, &value);
                        // L389
                        super::display_item::AddModComparisonTooltip(
                            cx,
                            me,
                            tooltip,
                            Some(&value.r#mod),
                            false,
                        );
                    }
                }));
            // L392
            let s2 = s.clone();
            cx.add(
                p,
                "custom",
                EditControl::new(
                    Anchor::nil(TopLeft, TopLeft),
                    rect(100, 45, 440, 18),
                    EditArgs::default(),
                )
                // L393
                .shown(Prop::f(move |cx, _| {
                    // L394
                    cur_source(cx, p, &s2) == "CUSTOM"
                })),
            );
            // L396
            let s2 = s.clone();
            let save = cx
                .add(
                    p,
                    "save",
                    ButtonControl::new(
                        Anchor::nil(BottomRight, Bottom),
                        rect(-4, -8, 80, 20),
                        "Add",
                        move |cx| {
                            // L397
                            let item = addModifier(cx, p, &s2);
                            super::display_item::SetDisplayItem(cx, me, Some(item));
                            // L398
                            popups::close_popup(cx);
                        },
                    ),
                )
                .id();
            // L400
            let s2 = s.clone();
            cx.w_mut::<ButtonControl>(save).tt.tooltip_func =
                Some(Rc::new(move |cx, tooltip, ()| {
                    // L401
                    tooltip.clear(false);
                    // L402
                    let item = addModifier(cx, p, &s2);
                    let f = items_hooks().add_item_tooltip;
                    let b = cx.build().model.clone();
                    f(cx, &b, tooltip, &item, None, false);
                }));
            // L404
            cx.add(
                p,
                "close",
                ButtonControl::new(
                    Anchor::nil(BottomLeft, Bottom),
                    rect(4, -8, 80, 20),
                    "Cancel",
                    |cx| {
                        // L405
                        popups::close_popup(cx);
                    },
                ),
            );
        },
        PopupOpts {
            enter: Some("save".into()),
            default,
            ..PopupOpts::default()
        },
    );
}

// @pob Modules/AddImplicitPopup.lua:414-414 h=85c34a63
// (`return M`: the module is this file)
