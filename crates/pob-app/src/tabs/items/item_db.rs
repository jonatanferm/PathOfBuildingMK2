//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemDBControl.lua 1-400
//! (`newClass("ItemDBControl", "ListControl")`). Owner: U32.
//!
//! @pob-file Classes/ItemDBControl.lua @16de4b82d
//!
//! `self.db` is `main.uniqueDB` / `main.rareDB` (Main.lua:159-161; U16's `MainState::unique_db`
//! / `rare_db`), read at use through [`DbType`]. Rows are the DB's `ItemRef`s (Lua table
//! identity). The list builder is a coroutine (`coroutine.create(self.ListBuilder)`, L310)
//! resumed once per Draw (ui doc §6.13): its body yields when more than 50 ms of `GetTime()`
//! passed since the last yield (L268-273).
//!
//! The dropdowns are built over `self.leagueList`, `self.typeList` and `self.sortDropList`,
//! which the Lua then mutates in place (`LoadLeaguesAndTypes` appends to the first two,
//! `BuildSortOrder` refills the third): the port writes the same entries into the dropdowns'
//! lists where the Lua writes the aliased tables (DropDownControl's DIVERGENCE(alias)).
//!
//! `item.measuredPower` (L260, L265) is a field PoB writes into the shared DB item; the port
//! writes it into the item's untyped fields (`Item.extra`), as the calc doc's census does for
//! other untyped item fields.
//!
//! `string.matchOrPattern` (Common.lua:1038-1079) has no port in a shared crate; [`match_or_pattern`]
//! is a local one (CCR-0107 §3). `PCall` (L164-201) is `pob_lua::error::catch`.
//!
//! `self.itemsTab.build.calcsTab:GetMiscCalculator()` (L258) is the facade's `misc_calculator`
//! (CalcsTab.lua:760-762 returns the calculator BuildOutput made), not U20's per-recalc
//! `BuildState::calc_func` cache, which is unset until the first OnFrame recalc.
//!
//! DIVERGENCE(pending-calc): the facade and `calcFunc` return `Poll::Pending` only on wasm (ui
//! doc §10.4); there the item (or the slot) is skipped and keeps the value found so far. Never
//! on desktop.
//!
//! ORDER:
//! * L78 `pairs(self.db.list)`: flags only, then sorted by key (`pairsSortByKey`) (c).
//! * L249 `pairs(self.db.list)` (string keys): the list is sorted afterwards by name (unique
//!   keys, so a total order) (c). With the stat sort, ties in `measuredPower` fall through to
//!   the name.
//! * L261 `pairs(self.itemsTab.slots)`: `measuredPower` is the maximum over the slots (c).
//! * L170/L177/L184 `pairs(item.*ModLines)` iterate sequences (ascending); the result is an
//!   any-match (c).
//! * `matchOrPattern`'s `pairs(orPatterns)` / `pairs(subGroup.patterns)`: insertion order; the
//!   result is an any-match, except that PoB's order decides whether a malformed or-branch
//!   raises before a matching one is tried (PoB is random there; the port tries them in the
//!   order they were generated) (c).
//!
//! Props with side effects (ui doc §7.3): none (the rareDB/uniqueDB `height` closures are
//! U33's).
#![allow(non_snake_case)]

use std::rc::Rc;

use indexmap::IndexMap;
use pob_calc::modules::power::{powerStat_table, GetFromOutput};
use pob_lua::math::{m_floor, m_max};
use pob_lua::{n, pattern};
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::controls::{EditArgs, EditControl};
use pob_ui_core::coroutine::{co, Coroutine};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtKey, TtObj};
use pob_ui_core::tooltip::Tooltip;

use super::slot::{item_label, set_sel_item_id, wiki_matches_key, wiki_open_item};
use super::slot_helper::items_hooks;
use crate::app::CxApp;
use crate::calc::MiscCalc;
use crate::main_mode::ItemDb;
use crate::model::{self, Table, Val};

// @pob-skip Classes/ItemDBControl.lua:1-20 reason="header comment, local aliases, newClass and the ItemDBData annotation (the struct and Widget impl below)"

/// `dbType`: "RARE" | "UNIQUE" (which of `main.rareDB` / `main.uniqueDB` is `self.db`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum DbType {
    Rare,
    Unique,
}

/// `self.db` (read at use: the Lua holds the table `main` holds).
fn db(cx: &Cx, t: DbType) -> &ItemDb {
    let m = &cx.app().main;
    match t {
        DbType::Unique => &m.unique_db,
        DbType::Rare => &m.rare_db,
    }
}

/// One `self.sortDropList` entry (`{ label, sortMode, itemField, stat, transform }`, L225-231),
/// as the Lua table `GetFromOutput` reads.
pub struct SortEntry {
    pub t: Table,
}

impl SortEntry {
    fn sort_mode(&self) -> Option<model::LStr> {
        self.t.str(n!("sortMode"))
    }
    fn has_stat(&self) -> bool {
        self.t.truthy(n!("stat"))
    }
}

/// `self.sortControl.NAME` / `.STAT` (L32-35).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortKey {
    /// `{ key = "name", dir = "ASCEND", func = function(a,b) return a:gsub("^The ","") < b:gsub("^The ","") end }`
    Name,
    /// `{ key = "measuredPower", dir = "DESCEND" }`
    Stat,
}

/// The row value as an item.
fn item_of(v: &ListValue) -> model::ItemRef {
    v.rc::<model::Item>()
        .unwrap_or_else(|| lua_error!("ItemDBControl: a list value that is not an item"))
}

/// `item.<key>` of a sort key: the name, or `measuredPower` (nil until the builder sets it).
#[derive(Clone, Debug, PartialEq)]
enum SortVal {
    Nil,
    Num(f64),
    Str(String),
}

fn sort_val(item: &model::Item, key: SortKey) -> SortVal {
    match key {
        SortKey::Name => SortVal::Str(item.name.get().to_string()),
        SortKey::Stat => match item.extra.get(n!("measuredPower")) {
            Val::Num(x) => SortVal::Num(x),
            Val::Nil => SortVal::Nil,
            _ => lua_error!("measuredPower is not a number"),
        },
    }
}

/// Lua `a < b` for the sort values (strings: byte order; mixed types raise).
fn lua_lt(a: &SortVal, b: &SortVal) -> bool {
    match (a, b) {
        (SortVal::Num(a), SortVal::Num(b)) => a < b,
        (SortVal::Str(a), SortVal::Str(b)) => a.as_bytes() < b.as_bytes(),
        _ => lua_error!("attempt to compare two incompatible values"),
    }
}

/// `string.matchOrPattern(self, pattern)` (Common.lua:1038-1079; CCR-0107 §3 local port).
pub fn match_or_pattern(s: &str, pat: &str) -> bool {
    // Common.lua:1039
    fn generate_or_patterns(pat: &str) -> IndexMap<String, ()> {
        // Common.lua:1040
        let mut sub_groups: Vec<(usize, usize, IndexMap<String, ()>)> = Vec::new();
        // Common.lua:1041 (`subGroups[index]` is always nil at the lookup: init 1)
        // Common.lua:1043
        let groups: Vec<String> = pattern::gmatch(pat, "%b()")
            .map(|c| c[0].to_str().into_owned())
            .collect();
        for sub_group in groups {
            // Common.lua:1044
            let (open, close, _) = pattern::find(pat, &sub_group, Some(1), true)
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
            // Common.lua:1045
            let inner = pob_lua::string::sub(&sub_group, 2, Some(-2)).into_owned();
            sub_groups.push((open, close, generate_or_patterns(&inner)));
        }
        // generate complete patterns from the subGroup patterns
        // Common.lua:1050
        let first_open = sub_groups.first().map_or(0, |g| g.0) as i64;
        let mut generated: Vec<String> =
            vec![pob_lua::string::sub(pat, 1, Some(first_open - 1)).into_owned()];
        // Common.lua:1051
        for (i, (_open, close, patterns)) in sub_groups.iter().enumerate() {
            // Common.lua:1052
            let next_open = sub_groups.get(i + 1).map_or(0, |g| g.0) as i64;
            let regular_next_string =
                pob_lua::string::sub(pat, *close as i64 + 1, Some(next_open - 1)).into_owned();
            // Common.lua:1053
            let mut temp: Vec<String> = Vec::new();
            // Common.lua:1054
            for sub_pattern in &generated {
                // Common.lua:1055
                for sub_group_pattern in patterns.keys() {
                    // Common.lua:1056
                    temp.push(format!(
                        "{sub_pattern}{sub_group_pattern}{regular_next_string}"
                    ));
                }
            }
            // Common.lua:1059
            generated = temp;
        }
        // apply | operators
        // Common.lua:1063
        let mut or_patterns: IndexMap<String, ()> = IndexMap::new();
        // Common.lua:1064
        for generated_pattern in &generated {
            // Common.lua:1065
            for c in pattern::gmatch(generated_pattern, "[^|]+") {
                // Common.lua:1066
                or_patterns.insert(c[0].to_str().into_owned(), ());
            }
        }
        // Common.lua:1069
        or_patterns
    }
    // Common.lua:1072
    let or_patterns = generate_or_patterns(pat);
    // Common.lua:1073
    for or_pattern in or_patterns.keys() {
        // Common.lua:1074
        if pattern::r#match(s, or_pattern, None).is_some() {
            // Common.lua:1075
            return true;
        }
    }
    // Common.lua:1078
    false
}

/// `local err, match = PCall(string.matchOrPattern, s, p)`; `not err and match`.
#[cfg(not(target_arch = "wasm32"))]
fn pcall_match(s: &str, p: &str) -> bool {
    pob_lua::error::catch(|| match_or_pattern(s, p)).unwrap_or(false)
}

/// DIVERGENCE(wasm-pcall): wasm32 is panic=abort (calc doc D15), so a malformed search pattern
/// aborts there instead of matching nothing.
#[cfg(target_arch = "wasm32")]
fn pcall_match(s: &str, p: &str) -> bool {
    match_or_pattern(s, p)
}

/// `ItemDBControl`: the unique or rare template database list.
pub struct ItemDBControl {
    pub list: ListControl,
    /// `self.itemsTab`
    pub items_tab: CtrlId,
    /// `self.dbType`
    pub db_type: DbType,
    /// `self.sortDropList` (the `controls.sort` dropdown's entries; UNIQUE only)
    pub sort_drop_list: Vec<DropItem>,
    /// `self.sortOrder`
    pub sort_order: Vec<SortKey>,
    /// `self.sortMode`
    pub sort_mode: String,
    /// `self.sortDetail`
    pub sort_detail: Option<Rc<SortEntry>>,
    /// `self.leaguesAndTypesLoaded`
    pub leagues_and_types_loaded: bool,
    /// `self.leagueList`
    pub league_list: Vec<String>,
    /// `self.typeList`
    pub type_list: Vec<String>,
    /// `self.slotList`
    pub slot_list: Vec<String>,
    /// `self.listBuildFlag`
    pub list_build_flag: bool,
    /// `self.listOutputRevision`
    pub list_output_revision: Option<f64>,
    /// `self.listBuilder` (nil when `None`)
    pub list_builder: Option<Coroutine>,
}

static HOOKS: ListHooks = ListHooks {
    get_row_value: ItemDBControl::GetRowValue,
    add_value_tooltip: Some(ItemDBControl::AddValueTooltip),
    get_drag_value: Some(ItemDBControl::GetDragValue),
    on_sel_click: Some(ItemDBControl::OnSelClick),
    on_sel_copy: Some(ItemDBControl::OnSelCopy),
    ..ListHooks::BASE
};

fn str_items(l: &[&str]) -> Vec<DropItem> {
    l.iter().map(|s| DropItem::str(*s)).collect()
}

impl ItemDBControl {
    /// lua: ItemDBControl.lua:26-73 `ItemDBClass:ItemDBControl(anchor, rect, itemsTab, db, dbType)`
    /// @pob Classes/ItemDBControl.lua:21-73 h=02078f28
    pub fn new(
        anchor: Anchor,
        r: [Prop<f64>; 4],
        items_tab: CtrlId,
        db_type: DbType,
    ) -> New<ItemDBControl> {
        // L27
        let mut l = ListControl::state(&HOOKS, 16.0, true, false, ListData::default(), false);
        // L31
        l.drag_target_list = Some(Vec::new());
        New::new(
            Base::new(anchor, r),
            ItemDBControl {
                list: l,
                // L28-30
                items_tab,
                db_type,
                // L32-35 (self.sortControl: SortKey)
                // L36
                sort_drop_list: Vec::new(),
                // L37
                sort_order: Vec::new(),
                // L38
                sort_mode: "NAME".to_owned(),
                sort_detail: None,
                // L39
                leagues_and_types_loaded: false,
                // L40
                league_list: vec!["Any league".to_owned(), "No league".to_owned()],
                // L41
                type_list: [
                    "Any type",
                    "Armour",
                    "Jewellery",
                    "One Handed Melee",
                    "Two Handed Melee",
                ]
                .map(str::to_owned)
                .to_vec(),
                // L42
                slot_list: [
                    "Any slot",
                    "Weapon 1",
                    "Weapon 2",
                    "Helmet",
                    "Body Armour",
                    "Gloves",
                    "Boots",
                    "Amulet",
                    "Ring",
                    "Belt",
                    "Jewel",
                    "Flask",
                    "Graft 1",
                    "Graft 2",
                ]
                .map(str::to_owned)
                .to_vec(),
                list_build_flag: false,
                list_output_revision: None,
                list_builder: None,
            },
        )
        .host()
        .init(|cx, me| ListControl::init(cx, me.0))
        .init(move |cx, me| {
            let me = me.0;
            let flag = move |cx: &mut Cx| cx.w_mut::<ItemDBControl>(me).list_build_flag = true;
            // L43
            let base_y = if db_type == DbType::Rare {
                -22.0
            } else {
                -62.0
            };
            let (slot_list, type_list, league_list) = {
                let w = cx.w::<ItemDBControl>(me);
                (
                    w.slot_list
                        .iter()
                        .map(|s| DropItem::str(s.clone()))
                        .collect::<Vec<_>>(),
                    w.type_list
                        .iter()
                        .map(|s| DropItem::str(s.clone()))
                        .collect::<Vec<_>>(),
                    w.league_list
                        .iter()
                        .map(|s| DropItem::str(s.clone()))
                        .collect::<Vec<_>>(),
                )
            };
            // L44-46
            let slot = cx.add(
                me,
                "slot",
                DropDownControl::new(
                    Anchor::new(BottomLeft, me, TopLeft),
                    rect(0, base_y, 179, 18),
                    slot_list,
                    Some(Rc::new(move |cx, _i, _v| flag(cx))),
                    None,
                    false,
                ),
            );
            // L47-49
            cx.add(
                me,
                "type",
                DropDownControl::new(
                    Anchor::new(Left, slot, Right),
                    rect(2, 0, 179, 18),
                    type_list,
                    Some(Rc::new(move |cx, _i, _v| flag(cx))),
                    None,
                    false,
                ),
            );
            // L50
            if db_type == DbType::Unique {
                // L51-53
                let sort = cx.add(
                    me,
                    "sort",
                    DropDownControl::new(
                        Anchor::new(BottomLeft, me, TopLeft),
                        rect(0, base_y + 20.0, 179, 18),
                        Vec::new(),
                        Some(Rc::new(move |cx, _index, value| {
                            // L52
                            let mode = value
                                .and_then(|v| v.get::<SortEntry>().and_then(SortEntry::sort_mode))
                                .unwrap_or_else(|| {
                                    lua_error!("attempt to index local 'value' (a nil value)")
                                });
                            ItemDBControl::SetSortMode(cx, me, &mode);
                        })),
                        None,
                        false,
                    ),
                );
                // L54-56
                cx.add(
                    me,
                    "league",
                    DropDownControl::new(
                        Anchor::new(Left, sort, Right),
                        rect(2, 0, 179, 18),
                        league_list,
                        Some(Rc::new(move |cx, _i, _v| flag(cx))),
                        None,
                        false,
                    ),
                );
                // L57-59
                let requirement = cx.add(
                    me,
                    "requirement",
                    DropDownControl::new(
                        Anchor::new(Left, sort, BottomLeft),
                        rect(0, 11, 179, 18),
                        str_items(&[
                            "Any requirements",
                            "Current level",
                            "Current attributes",
                            "Current useable",
                        ]),
                        Some(Rc::new(move |cx, _i, _v| flag(cx))),
                        None,
                        false,
                    ),
                );
                // L60-62
                cx.add(
                    me,
                    "obtainable",
                    DropDownControl::new(
                        Anchor::new(Left, requirement, Right),
                        rect(2, 0, 179, 18),
                        str_items(&[
                            "Obtainable",
                            "Any source",
                            "Unobtainable",
                            "Vendor Recipe",
                            "Upgraded",
                            "Boss Item",
                            "Corruption",
                            "Core Drop Pool",
                        ]),
                        Some(Rc::new(move |cx, _i, _v| flag(cx))),
                        None,
                        false,
                    ),
                );
            }
            // L64-66
            let search = cx.add(
                me,
                "search",
                EditControl::new(
                    Anchor::new(BottomLeft, me, TopLeft),
                    rect(0, -2, 258, 18),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Search".to_owned()),
                        filter: Some("%c".to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx, _buf| flag(cx))),
                        clearable: true,
                        ..EditArgs::default()
                    },
                ),
            );
            // L67-69
            cx.add(
                me,
                "searchMode",
                DropDownControl::new(
                    Anchor::new(Left, search, Right),
                    rect(2, 0, 100, 18),
                    str_items(&["Anywhere", "Names", "Modifiers"]),
                    Some(Rc::new(move |cx, _i, _v| flag(cx))),
                    None,
                    false,
                ),
            );
            // L70
            ItemDBControl::BuildSortOrder(cx, me);
            // L71
            flag(cx);
            // L72 return self
        })
    }

    fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
        cx.child(me, key)
            .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
    }

    fn sel(cx: &Cx, me: CtrlId, key: &'static str) -> usize {
        cx.w::<DropDownControl>(Self::ctl(cx, me, key)).sel_index
    }

    /// lua: ItemDBControl.lua:75-93 `ItemDBClass:LoadLeaguesAndTypes()`
    /// @pob Classes/ItemDBControl.lua:75-93 h=f895b3b6
    pub fn LoadLeaguesAndTypes(cx: &mut Cx, me: CtrlId) {
        let t = cx.w::<ItemDBControl>(me).db_type;
        // L76-77
        let mut league_flag: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        let mut type_flag: std::collections::BTreeSet<String> = std::collections::BTreeSet::new();
        // L78 (ORDER (c))
        for item in db(cx, t).list.values() {
            // L79
            if let Some(league) = item.extra.str(n!("league")) {
                // L80
                for c in pattern::gmatch(&league, " ?([%w ]+),?") {
                    // L81
                    league_flag.insert(c[0].to_str().into_owned());
                }
            }
            // L84
            type_flag.insert(item.r#type.get().to_string());
        }
        // L86-91 (pairsSortByKey: string keys in `<` order = byte order)
        let has_league = cx.child(me, "league");
        let type_ctl = Self::ctl(cx, me, "type");
        for league_name in league_flag {
            // L87 (self.leagueList is controls.league's list)
            cx.w_mut::<ItemDBControl>(me)
                .league_list
                .push(league_name.clone());
            if let Some(l) = has_league {
                cx.w_mut::<DropDownControl>(l)
                    .list
                    .push(DropItem::str(league_name));
            }
        }
        for ty in type_flag {
            // L90 (self.typeList is controls.type's list)
            cx.w_mut::<ItemDBControl>(me).type_list.push(ty.clone());
            cx.w_mut::<DropDownControl>(type_ctl)
                .list
                .push(DropItem::str(ty));
        }
        // L92
        cx.w_mut::<ItemDBControl>(me).leagues_and_types_loaded = true;
    }

    /// lua: ItemDBControl.lua:95-213 `ItemDBClass:DoesItemMatchFilters(item)`
    /// @pob Classes/ItemDBControl.lua:95-213 h=11ff55cd
    pub fn DoesItemMatchFilters(cx: &Cx, me: CtrlId, item: &model::Item) -> bool {
        let w = cx.w::<ItemDBControl>(me);
        let unique = w.db_type == DbType::Unique;
        // L96
        let slot_sel = Self::sel(cx, me, "slot");
        if slot_sel > 1 {
            // L97
            let primary_slot = item.GetPrimarySlot();
            let want = &w.slot_list[slot_sel - 1];
            // L98
            if &*primary_slot != want.as_str()
                && pattern::gsub(&primary_slot, " %d", "", None).0 != *want
            {
                // L99
                return false;
            }
        }
        // L102
        let type_sel = Self::sel(cx, me, "type");
        // L103
        if type_sel > 1 {
            let ty = item.r#type.get();
            let base = || {
                item.base
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"))
            };
            // L104
            if type_sel == 2 {
                // L105
                if base().armour.is_none() {
                    // L106
                    return false;
                }
            } else if type_sel == 3 {
                // L109
                if !(&*ty == "Amulet" || &*ty == "Ring" || &*ty == "Belt") {
                    // L110
                    return false;
                }
            } else if type_sel == 4 || type_sel == 5 {
                // L113
                let weapon_info = cx.build().model.data.weaponTypeInfo.get(&ty);
                // L114
                if !weapon_info.is_some_and(|wi| {
                    wi.melee && ((type_sel == 4 && wi.oneHand) || (type_sel == 5 && !wi.oneHand))
                }) {
                    // L115
                    return false;
                }
            } else if w.type_list.get(type_sel - 1).map(String::as_str) != Some(&*ty) {
                // L118
                return false;
            }
        }
        // L121
        if unique {
            let league_sel = Self::sel(cx, me, "league");
            if league_sel > 1 {
                let league = item.extra.str(n!("league"));
                // L122
                let rejected = (league_sel == 2 && league.is_some())
                    || (league_sel > 2
                        && match &league {
                            None => true,
                            Some(l) => {
                                let p = w.league_list.get(league_sel - 1).cloned().unwrap_or_else(
                                    || {
                                        lua_error!(
                                            "bad argument #1 to 'match' (string expected, got nil)"
                                        )
                                    },
                                );
                                pattern::r#match(l, &p, None).is_none()
                            }
                        });
                if rejected {
                    // L123
                    return false;
                }
            }
        }
        // L126
        if unique {
            let ob = Self::sel(cx, me, "obtainable");
            if ob != 2 {
                // L127
                let source = item.extra.str(n!("source"));
                let source_s = source.as_deref().unwrap_or("").to_owned();
                // L128
                let obtainable = !(source_s == "No longer obtainable"
                    || item.extra.str(n!("league")).as_deref() == Some("Race Events"));
                // L129
                if (ob == 1 && !obtainable) || (ob == 3 && obtainable) {
                    // L130
                    return false;
                } else if ob == 4 && source_s != "Vendor Recipe" {
                    // L131-132
                    return false;
                } else if ob == 5 && pattern::r#match(&source_s, "Upgraded from", None).is_none() {
                    // L133-134
                    return false;
                } else if ob == 6
                    && pattern::r#match(&source_s, "Drops from unique", None).is_none()
                {
                    // L135-136
                    return false;
                } else if ob == 7 && pattern::r#match(&source_s, "Vaal Orb", None).is_none() {
                    // L137-138
                    return false;
                } else if ob == 8 && source.is_some() {
                    // L139-140
                    return false;
                }
            }
        }
        // L143
        if unique {
            let req = Self::sel(cx, me, "requirement");
            if req > 1 {
                let m = &cx.build().model;
                // L144
                if req == 2 || req == 4 {
                    if let Some(level) = item.requirements.opt_num(n!("level")) {
                        if level > m.characterLevel.get() {
                            // L145
                            return false;
                        }
                    }
                }
                // L147
                if req > 2 {
                    let out = m.calcsTab.mainOutput.get().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'mainOutput' (a nil value)")
                    });
                    let r = &item.requirements;
                    if r.num(n!("strMod")) > out.num(n!("Str"))
                        || r.num(n!("dexMod")) > out.num(n!("Dex"))
                        || r.num(n!("intMod")) > out.num(n!("Int"))
                    {
                        // L148
                        return false;
                    }
                }
            }
        }
        // L151
        let buf = cx.w::<EditControl>(Self::ctl(cx, me, "search")).buf.clone();
        let mut search_str = pattern::gsub(
            &pob_lua::string::lower(&buf),
            "[%-%.%+%[%]%$%^%%%?%*]",
            "%%%0",
            None,
        )
        .0;
        // L152
        if pattern::find(&search_str, "%S", None, false).is_some() {
            // L153
            let mut found = false;
            // L154
            let mode = Self::sel(cx, me, "searchMode");
            // L155
            if mode == 1 || mode == 2 {
                // check if any explicit mods have a mutated line
                // L157
                let mut search_name = pob_lua::string::lower(&item.name.get());
                // L158
                for mod_line in item.explicitModLines.get().snapshot() {
                    // L159
                    if mod_line.truthy(n!("newModId")) {
                        // L160
                        search_name = format!("foulborn {search_name}");
                        // L161
                        break;
                    }
                }
                // L164-165
                if pcall_match(&search_name, &search_str) {
                    // L166
                    found = true;
                }
            }
            // L169
            if mode == 1 || mode == 3 {
                let line_of = |t: &Table| -> String {
                    let line = t.get(n!("line"));
                    let Some(line) = line.str() else {
                        lua_error!(
                            "attempt to index field 'line' (a {} value)",
                            line.lua_type()
                        )
                    };
                    pob_lua::string::lower(&line)
                };
                // L170
                for line in item.enchantModLines.get().snapshot() {
                    // L171-172
                    if pcall_match(&line_of(&line), &search_str) {
                        // L173
                        found = true;
                        // L174
                        break;
                    }
                }
                // L177
                for line in item.implicitModLines.get().snapshot() {
                    // L178-179
                    if pcall_match(&line_of(&line), &search_str) {
                        // L180
                        found = true;
                        // L181
                        break;
                    }
                }
                // L184
                let data = cx.build().model.data;
                for line in item.explicitModLines.get().snapshot() {
                    // Include the other side of Foulborn transformations in modifier searches.
                    // L186
                    let new_mod = line.str(n!("newModId")).and_then(|id| {
                        let id = model::Name::from(&id);
                        let tab = |cat: &str| {
                            data.itemMods
                                .get(&model::Name::from(cat))
                                .and_then(|t| t.0.get(&id).cloned())
                        };
                        tab("ItemExclusive").or_else(|| tab("Foulborn"))
                    });
                    // L187
                    let mut search_line = line_of(&line);
                    // L188
                    if let Some(new_mod) = new_mod {
                        // L189
                        let joined: Vec<&str> = new_mod.lines.iter().map(|s| &**s).collect();
                        search_line.push_str(&pob_lua::string::lower(&joined.join(" ")));
                    }
                    // L191-192
                    if pcall_match(&search_line, &search_str) {
                        // L193
                        found = true;
                        // L194
                        break;
                    }
                }
                // L197
                if !found {
                    // L198
                    search_str = pattern::gsub(&search_str, " ", "", None).0;
                    // L199
                    let mods = item
                        .baseModList
                        .get()
                        .map(|l| l.ipairs())
                        .unwrap_or_else(|| {
                            lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
                        });
                    for m in mods {
                        // L200-201
                        if pcall_match(&pob_lua::string::lower(&m.name), &search_str) {
                            // L202
                            found = true;
                            // L203
                            break;
                        }
                    }
                }
            }
            // L208
            if !found {
                // L209
                return false;
            }
        }
        // L212
        true
    }

    /// lua: ItemDBControl.lua:215-219 `ItemDBClass:SetSortMode(sortMode)`
    /// @pob Classes/ItemDBControl.lua:215-219 h=a1b422b4
    pub fn SetSortMode(cx: &mut Cx, me: CtrlId, sort_mode: &str) {
        // L216
        cx.w_mut::<ItemDBControl>(me).sort_mode = sort_mode.to_owned();
        // L217
        Self::BuildSortOrder(cx, me);
        // L218
        cx.w_mut::<ItemDBControl>(me).list_build_flag = true;
    }

    /// lua: ItemDBControl.lua:221-245 `ItemDBClass:BuildSortOrder()`
    /// @pob Classes/ItemDBControl.lua:221-245 h=a48e7aca
    pub fn BuildSortOrder(cx: &mut Cx, me: CtrlId) {
        // L222
        let mut drop_list: Vec<DropItem> = Vec::new();
        // L223
        for stat in &cx.data().powerStatList.stats {
            // L224
            if !stat.ignoreForItems {
                // L225-231
                let t = powerStat_table(stat);
                let entry = Table::new();
                let label = format!("Sort by {}", stat.label);
                entry.set(n!("label"), label.as_str());
                entry.set(
                    n!("sortMode"),
                    stat.itemField.or(stat.stat).map_or(Val::Nil, Val::from),
                );
                entry.set(n!("itemField"), t.get(n!("itemField")));
                entry.set(n!("stat"), t.get(n!("stat")));
                entry.set(n!("transform"), t.get(n!("transform")));
                drop_list.push(DropItem::new(label, Rc::new(SortEntry { t: entry })));
            }
        }
        cx.w_mut::<ItemDBControl>(me).sort_drop_list = drop_list.clone();
        // L234
        cx.w_mut::<ItemDBControl>(me).sort_order.clear();
        // L235
        if let Some(sort) = cx.child(me, "sort") {
            // (self.sortDropList is controls.sort's list)
            cx.w_mut::<DropDownControl>(sort).list = drop_list;
            // L236
            DropDownControl::check_dropped_width(cx, sort, true);
            // L237
            cx.w_mut::<DropDownControl>(sort).sel_index = 1;
            // L238
            let mode = cx.w::<ItemDBControl>(me).sort_mode.clone();
            DropDownControl::sel_by_value(cx, sort, &|v: &DropItem| {
                v.get::<SortEntry>()
                    .and_then(SortEntry::sort_mode)
                    .is_some_and(|m| *m == *mode)
            });
            // L239
            let detail = DropDownControl::get_sel_value(cx, sort)
                .and_then(|v| v.value.downcast::<SortEntry>().ok());
            cx.w_mut::<ItemDBControl>(me).sort_detail = detail;
        }
        let w = cx.w_mut::<ItemDBControl>(me);
        // L241
        if w.sort_detail.as_ref().is_some_and(|d| d.has_stat()) {
            // L242
            w.sort_order.push(SortKey::Stat);
        }
        // L244
        w.sort_order.push(SortKey::Name);
    }

    /// lua: ItemDBControl.lua:247-301 `ItemDBClass:ListBuilder()`, the coroutine body.
    /// @pob Classes/ItemDBControl.lua:247-301 h=35f297cc
    async fn ListBuilder(me: CtrlId) {
        // L248-253 (ORDER (c))
        let (list, detail) = co::with_cx(|cx| {
            let t = cx.w::<ItemDBControl>(me).db_type;
            let all: Vec<model::ItemRef> = db(cx, t).list.values().cloned().collect();
            let list: Vec<model::ItemRef> = all
                .into_iter()
                .filter(|item| Self::DoesItemMatchFilters(cx, me, item))
                .collect();
            (list, cx.w::<ItemDBControl>(me).sort_detail.clone())
        });

        // L255 -- stat-based
        if let Some(detail) = detail.filter(|d| d.has_stat()) {
            // L256
            let use_full_dps = detail.t.str(n!("stat")).as_deref() == Some("FullDPS");
            // L257
            let mut start = co::with_cx(|cx| cx.sys.time_ms());
            // L258
            // (`self.build` is nil: GetMiscCalculator takes no argument)
            let calc_func: Option<Rc<dyn MiscCalc>> = co::with_cx(|cx| {
                let m = cx.build().model.clone();
                match cx.build_mut().calc.misc_calculator(&m) {
                    std::task::Poll::Ready((f, _base)) => Some(f),
                    // DIVERGENCE(pending-calc): no calculator yet (wasm only)
                    std::task::Poll::Pending => None,
                }
            });
            // L259
            for (item_index0, item) in list.iter().enumerate() {
                if let Some(calc_func) = &calc_func {
                    co::with_cx(|cx| {
                        Self::measure_item(cx, item, &detail, calc_func, use_full_dps);
                    });
                }
                // L268
                let now = co::with_cx(|cx| cx.sys.time_ms());
                // L269
                if now - start > 50 {
                    // L270
                    co::with_cx(|cx| {
                        let pct = m_floor((item_index0 + 1) as f64 / list.len() as f64 * 100.0);
                        cx.w_mut::<ListControl>(me).default_text =
                            Some(format!("^7Sorting... ({}%)", pob_lua::tostring_num(pct)));
                    });
                    // L271
                    co::yield_now().await;
                    // L272
                    start = now;
                }
            }
        }

        co::with_cx(|cx| {
            let order = cx.w::<ItemDBControl>(me).sort_order.clone();
            let mut list = list;
            // L277-297
            pob_lua::sort(&mut list, |a, b| {
                // L278
                for key in &order {
                    // L279-280
                    let a_val = sort_val(a, *key);
                    let b_val = sort_val(b, *key);
                    // L281
                    if a_val != b_val {
                        // L282
                        return match key {
                            // L292 (ASCEND with func)
                            SortKey::Name => {
                                let strip = |v: &SortVal| match v {
                                    SortVal::Str(s) => {
                                        SortVal::Str(pattern::gsub(s, "^The ", "", None).0)
                                    }
                                    _ => lua_error!(
                                        "attempt to index local 'a' (a non-string value)"
                                    ),
                                };
                                lua_lt(&strip(&a_val), &strip(&b_val))
                            }
                            // L286 (DESCEND without func)
                            SortKey::Stat => lua_lt(&b_val, &a_val),
                        };
                    }
                }
                false
            });
            // L299
            let values: Vec<ListValue> = list.into_iter().map(ListValue::of).collect();
            let w = cx.w_mut::<ListControl>(me);
            w.list = ListData::Local(values);
            // L300
            w.default_text = Some("^7No items found that match those filters.".to_owned());
        });
    }

    /// L260-267: `item.measuredPower` over the valid, active slots.
    fn measure_item(
        cx: &mut Cx,
        item: &model::ItemRef,
        detail: &SortEntry,
        calc_func: &Rc<dyn MiscCalc>,
        use_full_dps: bool,
    ) {
        let m = cx.build().model.clone();
        // L260
        item.extra.set(n!("measuredPower"), -f64::INFINITY);
        let use_second = m
            .itemsTab
            .activeItemSet
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
            .truthy(n!("useSecondWeaponSet"));
        // L261 (ORDER (c))
        let slots: Vec<(model::Name, Table)> = m
            .itemsTab
            .slots
            .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
        for (slot_name, slot) in slots {
            // L262
            let weapon_set = slot.get(n!("weaponSet"));
            if m.itemsTab.IsItemValidForSlot(&m, item, &slot_name, None)
                && !slot.truthy(n!("inactive"))
                && (!weapon_set.truthy()
                    || weapon_set.raw_eq(&Val::Num(if use_second { 2.0 } else { 1.0 })))
            {
                // L263
                let base = item
                    .base
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"));
                let ov = model::Override::default();
                if base.flask.is_some() {
                    ov.toggleFlask.set(Some(item.clone()));
                } else if base.tincture.is_some() {
                    ov.toggleTincture.set(Some(item.clone()));
                } else {
                    ov.repSlotName.set(Some(slot_name));
                    ov.repItem.set(Some(item.clone()));
                }
                let std::task::Poll::Ready(output) =
                    calc_func.call(Some(Rc::new(ov)), Some(use_full_dps))
                else {
                    // DIVERGENCE(pending-calc)
                    continue;
                };
                // L264
                let measured_power = GetFromOutput(&output, &detail.t, false);
                // L265
                let cur = item.extra.num(n!("measuredPower"));
                item.extra
                    .set(n!("measuredPower"), m_max(cur, measured_power));
            }
        }
    }

    /// lua: ItemDBControl.lua:330-334 `ItemDBClass:GetRowValue(column, index, item)`
    /// @pob Classes/ItemDBControl.lua:330-334 h=cebf0d0c
    fn GetRowValue(cx: &Cx, _me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
        // L331
        if column == 1 {
            // L332
            return item_label(cx, &item_of(v));
        }
        String::new()
    }

    /// lua: ItemDBControl.lua:336-344 `ItemDBClass:AddValueTooltip(tooltip, index, item)`
    /// @pob Classes/ItemDBControl.lua:336-344 h=722a8e00
    fn AddValueTooltip(
        cx: &mut Cx,
        _me: CtrlId,
        tooltip: &mut Tooltip,
        _index: usize,
        v: &ListValue,
    ) {
        // L337
        if !cx.main.popups.is_empty() {
            // L338
            tooltip.clear(false);
            // L339
            return;
        }
        let item = item_of(v);
        // L341
        let keys = [
            TtKey::Obj(TtObj(item.clone())),
            TtKey::Bool(cx.sys.is_key_down(Key::Shift)),
            TtKey::Bool(cx.app().launch.dev_mode_alt),
            TtKey::Num(cx.build().model.outputRevision.get()),
        ];
        if tooltip.check_for_update(cx, &keys) {
            // L342
            let f = items_hooks().add_item_tooltip;
            let b = cx.build().model.clone();
            f(cx, &b, tooltip, &item, None, true);
        }
    }

    /// lua: ItemDBControl.lua:346-348 `ItemDBClass:GetDragValue(index, item)`
    /// @pob Classes/ItemDBControl.lua:346-348 h=145dcc62
    fn GetDragValue(
        _cx: &Cx,
        _me: CtrlId,
        _index: usize,
        v: &ListValue,
    ) -> Option<(Rc<str>, DragValue)> {
        // L347
        Some((Rc::from("Item"), DragValue(item_of(v))))
    }

    /// lua: ItemDBControl.lua:350-387 `ItemDBClass:OnSelClick(index, item, doubleClick)`
    /// @pob Classes/ItemDBControl.lua:350-387 h=c3b34c1a
    fn OnSelClick(
        cx: &mut Cx,
        me: CtrlId,
        _index: usize,
        v: &ListValue,
        double_click: bool,
    ) -> Option<bool> {
        let item = item_of(v);
        let h = items_hooks();
        let m = cx.build().model.clone();
        // L351
        if cx.sys.is_key_down(Key::Ctrl) {
            // Add item
            // L353
            let new_item = crate::main_mode::new_item(Some(&item.raw.get()), None, false);
            // L354
            new_item.NormaliseQuality();
            // L355
            (h.add_item)(cx, &new_item, true, None);

            // Equip item if able
            // L358
            let mut slot_name = new_item.GetPrimarySlot().to_string();
            let slot_of = |name: &str| {
                m.itemsTab
                    .slots
                    .with(|s| s.get(&model::Name::from(name)).cloned())
            };
            // L359
            if let Some(slot) = slot_of(&slot_name) {
                // L360
                let use_second = m
                    .itemsTab
                    .activeItemSet
                    .get()
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index field 'activeItemSet' (a nil value)")
                    })
                    .truthy(n!("useSecondWeaponSet"));
                if slot.get(n!("weaponSet")).raw_eq(&Val::Num(1.0)) && use_second {
                    // Redirect to second weapon set
                    // L362
                    slot_name = format!("{slot_name} Swap");
                }
                // L364
                if cx.sys.is_key_down(Key::Shift) {
                    // Redirect to second slot if possible
                    // L366
                    let alt_slot = pattern::gsub(&slot_name, "1", "2", None).0;
                    // L367
                    if m.itemsTab
                        .IsItemValidForSlot(&m, &new_item, &alt_slot, None)
                    {
                        // L368
                        slot_name = alt_slot;
                    }
                }
                // L371
                let slot = slot_of(&slot_name)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                let id = new_item.id.get().unwrap_or_else(|| {
                    lua_error!("ItemDBControl: AddItem left the item without an id")
                });
                set_sel_item_id(&m, &slot, id);
            }

            // L374
            (h.add_forbidden_jewel_counterpart)(cx, &new_item);

            // L376
            (h.populate_slots)(cx);
            // L377
            (h.add_undo_state)(cx);
            // L378
            m.buildFlag.set(true);
        } else if double_click {
            // disallow dragging after double click since the window can jump when
            // the display item tooltip is created, which might cause the drag item
            // to get stuck to the cursor
            // L383
            cx.w_mut::<ListControl>(me).sel_dragging = false;
            // L384
            (h.create_display_item_from_raw)(cx, &item.raw.get(), true);
            // L385
            return Some(false);
        }
        None
    }

    /// lua: ItemDBControl.lua:389-391 `ItemDBClass:OnSelCopy(index, item)`
    /// @pob Classes/ItemDBControl.lua:389-391 h=4939d715
    fn OnSelCopy(cx: &mut Cx, _me: CtrlId, _index: usize, v: &ListValue) {
        // L390
        let text = pattern::gsub(&item_of(v).raw.get(), "\n", "\r\n", None).0;
        cx.sys.platform.copy(&text);
    }
}

impl Widget for ItemDBControl {
    const NAME: &'static str = "ItemDBControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS.union(Caps::HOVER_KEY_UP);

    /// lua: ItemDBControl.lua:303-328 `ItemDBClass:Draw(viewPort)`
    /// @pob Classes/ItemDBControl.lua:303-328 h=2b5f39ad
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        let rev = cx.build().model.outputRevision.get();
        // L304
        if Some(rev) != cx.w::<ItemDBControl>(me).list_output_revision {
            // L305
            cx.w_mut::<ItemDBControl>(me).list_build_flag = true;
        }
        // L307
        if cx.w::<ItemDBControl>(me).list_build_flag {
            // L308
            cx.w_mut::<ItemDBControl>(me).list_build_flag = false;
            // L309
            cx.w_mut::<ListControl>(me).list = ListData::Local(Vec::new());
            // L310
            cx.w_mut::<ItemDBControl>(me).list_builder =
                Some(Coroutine::create(ItemDBControl::ListBuilder(me)));
            // L311
            cx.w_mut::<ItemDBControl>(me).list_output_revision = Some(rev);
        }
        let t = cx.w::<ItemDBControl>(me).db_type;
        // L313
        if cx.w::<ItemDBControl>(me).list_builder.is_some() && !db(cx, t).loading {
            // L314
            let (res, err_msg) = cx.lend(
                me,
                |w: &mut ItemDBControl| &mut w.list_builder,
                |cx, co| {
                    let co = co.as_mut().expect("checked");
                    #[cfg(not(target_arch = "wasm32"))]
                    let r = co.try_resume(cx);
                    // DIVERGENCE(wasm-pcall): no unwinding on wasm32; an error aborts
                    #[cfg(target_arch = "wasm32")]
                    let r = (co.resume(cx), None::<String>);
                    r
                },
            );
            // L315
            if cx.app().launch.dev_mode && !res {
                // L316
                lua_error!("{}", err_msg.unwrap_or_default());
            }
            // L318
            if cx
                .w::<ItemDBControl>(me)
                .list_builder
                .as_ref()
                .is_some_and(Coroutine::is_dead)
            {
                // L319
                cx.w_mut::<ItemDBControl>(me).list_builder = None;
            }
        }
        // L322
        if db(cx, t).loading {
            // L323
            cx.w_mut::<ListControl>(me).default_text = Some("^7Loading...".to_owned());
        } else if !cx.w::<ItemDBControl>(me).leagues_and_types_loaded {
            // L325
            Self::LoadLeaguesAndTypes(cx, me);
        }
        // L327 self.ListControl.Draw(self, viewPort): noTooltip is nil
        ListControl::draw(cx, me, vp, false);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        ListControl::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        ListControl::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        ListControl::on_key_up(cx, me, key)
    }
    fn duck(cx: &Cx, me: CtrlId, q: Duck) -> bool {
        ListControl::duck(cx, me, q)
    }

    /// lua: ItemDBControl.lua:393-400 `ItemDBClass:OnHoverKeyUp(key)`
    /// @pob Classes/ItemDBControl.lua:393-400 h=c57fae99
    fn on_hover_key_up(cx: &mut Cx, me: CtrlId, key: Key) {
        // L394
        if wiki_matches_key(key) {
            // L395
            let item = ListControl::get_hover_value(cx, me);
            // L396
            if let Some(item) = item {
                // L397
                wiki_open_item(cx, &item_of(&item));
            }
        }
    }

    pob_ui_core::impl_parts!(ItemDBControl: list => ListControl);
}
