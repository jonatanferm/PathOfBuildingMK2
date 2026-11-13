//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/NotableDBControl.lua 1-293
//! (`newClass("NotableDBControl", "ListControl")`). `string:matchOrPattern` (Modules/Common.lua:
//! 1038-1079) is the shared `pob_lua::string::matchOrPattern` (FUP7; U28 ported it first here).
//! Owner: U28.
//!
//! @pob-file Classes/NotableDBControl.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The anoint popup's notable list (ItemsTab.lua:3085, U36): the anointable notables of a tree,
//! filtered by a search box, sorted by name or by the power an anoint adds. The power sort runs
//! in a coroutine (`ListBuilder`, resumed once per `Draw`) that yields every 50 ms of
//! `GetTime()`, showing "Sorting... (n%)" meanwhile.
//!
//! ListControl hooks: `GetRowValue`, `AddValueTooltip`, `GetDragValue`, `OnSelClick` (empty),
//! `OnSelCopy`. Own methods: `Draw` (the coroutine step, then the super Draw with `noTooltip`
//! nil), `DoesNotableMatchFilters`, `SetSortMode`, `BuildSortOrder`, `CalculatePowerStat`,
//! `ListBuilder`.
//!
//! # The `itemsTab` argument (CCR-0112)
//!
//! Lua passes the ItemsTab (`self.itemsTab = itemsTab`) and reads `itemsTab.build` (its
//! `outputRevision` and `calcsTab:GetMiscCalculator()`), `itemsTab.displayItem`,
//! `itemsTab:anointItem(node)`, `itemsTab:AppendAnointTooltip(tooltip, node)` and
//! `itemsTab.socketViewer:AddNodeName(tooltip, node, build)`, all of them UI units that are not
//! ported yet (U33-U36, U24). The control receives that object as an `Rc<dyn`
//! [`NotableItemsTab`]`>`; the ItemsTab port implements it (galleries pass stand-ins).
//!
//! # Notes
//!
//! - `self.db` is `build.spec.tree.nodes` (an [`LjIntMap`]); the nodes are the tree's shared
//!   node objects, and ListBuilder writes `node.measuredPower` on them (L141, L144, L149, L161)
//!   as the Lua does (`Node.extra`).
//! - ORDER: L125 `pairs(self.db)` over `tree.nodes`, whose order is per-process random in PoB
//!   (the table held the string key `root`, calc doc §3.4); Rust walks the LjIntMap order. The
//!   list is sorted afterwards (L166) by `dn`, after `measuredPower` in stat mode: a total order
//!   unless two anointable notables share a name and a power, which no tree has (resolution (c)).
//! - ORDER: `matchOrPattern` iterates string-keyed sets with `pairs` (Common.lua:1053, 1073);
//!   the shared port keeps their insertion order. The result is "does any alternative match", so
//!   the order matters only when one alternative is a malformed pattern and another matches:
//!   PoB's answer then depends on which `pairs` reaches first (resolution (c)).
//! - L36-L37 `ListControl(anchor, rect, 16, "VERTICAL", false)`: `scroll` is the truthy string
//!   "VERTICAL" (`scroll_h` false). L38's dropdown gets `self.sortDropList` as its list, which
//!   BuildSortOrder later wipes and refills in place: the Rust dropdown owns its `Vec`, so
//!   [`NotableDBControl::build_sort_order`] writes the new entries into it (U13's
//!   DIVERGENCE(alias) note: aliasing callers reproduce the wipe themselves).
//! - `data.powerStatList` is the process-global game data; galleries without game data install
//!   the dump's list with [`set_power_stat_list`] (a test seam the product never sets).
//! - DIVERGENCE(wasm-calc): where `calcFunc` answers `Pending` (wasm only, ui doc §10.4) the
//!   builder yields and asks again on the next resume; on desktop it is always `Ready`.
//! - L290-294 `OnSelCopy` reads the undeclared global `item` (LUA-GLOBAL-NIL): Ctrl+C on a row
//!   raises "attempt to index global 'item' (a nil value)", as in PoB.
//! - Props with side effects (ui doc §7.3): none.
#![allow(non_snake_case, clippy::float_cmp)]

use std::cell::Cell;
use std::rc::Rc;
use std::task::Poll;

use pob_lua::format::tostring_num;
use pob_lua::math::{m_floor, m_max};
use pob_lua::pattern;
use pob_lua::string::matchOrPattern;
use pob_ui_core::controls::drop_down::{DropDownControl, DropItem, SelFunc};
use pob_ui_core::controls::edit::{EditArgs, EditControl};
use pob_ui_core::controls::list::{ListControl, ListData, ListHooks, ListValue};
use pob_ui_core::controls::popup_dialog::PopupDialog;
use pob_ui_core::coroutine::{co, Coroutine};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::{Tooltip, TtKey, TtObj};

use crate::calc::MiscCalc;
use crate::model::{self, ItemRef, LjIntMap, Name, NodeRef, Output, Override, Table, Val};

/// `data.powerStatList` (Modules/Data.lua:129-235).
pub type PowerStatList = model::gamedata::schema::item_data::PowerStatList;

// @pob Classes/NotableDBControl.lua:1-15 h=8b28043e (header and locals)

/// lua: NotableDBControl.lua:17-19 IsAnointableNode(node)
/// @pob Classes/NotableDBControl.lua:16-19 h=5d05cc69
fn IsAnointableNode(node: &NodeRef) -> bool {
    // L18 node.recipe and #node.recipe >= 1
    let recipe = node.extra.get("recipe");
    recipe.truthy() && len_of(&recipe) >= 1
}

// @pob Classes/NotableDBControl.lua:20-22 h=39501fa0 (newClass: the struct and Widget impl below)

// ---- the `itemsTab` argument (Rust plumbing, CCR-0112) ---------------------------------------

/// The ItemsTab the control receives (`self.itemsTab`): one method per Lua access.
pub trait NotableItemsTab {
    /// `itemsTab.build.outputRevision`
    fn build_output_revision(&self, cx: &Cx) -> f64;
    /// `itemsTab.build.calcsTab:GetMiscCalculator()` (its first result, `calcFunc`)
    fn get_misc_calculator(&self, cx: &mut Cx) -> Rc<dyn MiscCalc>;
    /// `itemsTab.displayItem`
    fn display_item(&self, cx: &Cx) -> Option<ItemRef>;
    /// `itemsTab:anointItem(node)` (ItemsTab.lua:3011)
    fn anoint_item(&self, cx: &mut Cx, node: Option<&NodeRef>) -> ItemRef;
    /// `itemsTab.socketViewer:AddNodeName(tooltip, node, itemsTab.build)` (PassiveTreeView.lua)
    fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
    /// `itemsTab:AppendAnointTooltip(tooltip, node)` (ItemsTab.lua:3028)
    fn append_anoint_tooltip(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef);
}

thread_local! {
    static POWER_STAT_LIST: Cell<Option<&'static PowerStatList>> = const { Cell::new(None) };
}

/// Test seam (never set by the product): `data.powerStatList` for a process without game data
/// (the galleries load the dump's `powerStatList.json`). `None` restores the game data's.
#[doc(hidden)]
pub fn set_power_stat_list(list: Option<&'static PowerStatList>) {
    POWER_STAT_LIST.with(|c| c.set(list));
}

/// `data.powerStatList`
fn power_stat_list() -> &'static PowerStatList {
    POWER_STAT_LIST
        .with(Cell::get)
        .unwrap_or_else(|| &model::data().powerStatList)
}

// ---- small Lua helpers -------------------------------------------------------------------------

/// `#v` of a table value (nil and other kinds raise).
#[track_caller]
fn len_of(v: &Val) -> usize {
    match v {
        Val::Table(t) => t.len(),
        Val::Data(model::gamedata::Value::Table(t)) => t.len(),
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// `ipairs(v)` values of a table value.
#[track_caller]
fn ipairs_of(v: &Val) -> Vec<Val> {
    if !matches!(v, Val::Table(_) | Val::Data(_)) {
        lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            v.lua_type()
        );
    }
    let mut out = Vec::new();
    let mut i = 1i64;
    loop {
        let x = v.get(i);
        if x.is_nil() {
            return out;
        }
        out.push(x);
        i += 1;
    }
}

/// Lua `a .. b` operand.
#[track_caller]
fn cat(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// Lua `a < b` (numbers, or strings by bytes as LuaJIT's `lj_str_cmp`).
#[track_caller]
fn lua_lt(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x < y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() < y.as_bytes(),
        (x, y) if x.lua_type() == y.lua_type() => {
            lua_error!("attempt to compare two {} values", x.lua_type())
        }
        (x, y) => lua_error!("attempt to compare {} with {}", x.lua_type(), y.lua_type()),
    }
}

/// `PCall(f, ...)`: `Err` when `f` raised a Lua error.
fn pcall<R>(f: impl FnOnce() -> R) -> Result<R, String> {
    #[cfg(not(target_arch = "wasm32"))]
    {
        pob_lua::error::catch(f).map_err(|e| e.0)
    }
    #[cfg(target_arch = "wasm32")]
    {
        Ok(f())
    }
}

// ---- the control -------------------------------------------------------------------------------

/// One `self.sortControl` entry (L31-L34): `{ key, dir, func }`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SortControl {
    /// `NAME = { key = "dn", dir = "ASCEND", func = function(a,b) return a < b end }`
    Name,
    /// `STAT = { key = "measuredPower", dir = "DESCEND" }`
    Stat,
}

impl SortControl {
    /// `a[data.key]`
    fn key(self, node: &NodeRef) -> Val {
        match self {
            SortControl::Name => Val::Str(node.dn.get()),
            SortControl::Stat => node.extra.get("measuredPower"),
        }
    }
}

pub struct NotableDBControl {
    pub list: ListControl,
    /// `self.itemsTab`
    pub items_tab: Rc<dyn NotableItemsTab>,
    /// `self.db`
    pub db: LjIntMap<NodeRef>,
    /// `self.dbType` ("ANOINT"; never read)
    pub db_type: Option<String>,
    /// `self.sortDropList` (also the sort dropdown's list, see the module docs)
    pub sort_drop_list: Vec<Table>,
    /// `self.sortOrder`
    pub sort_order: Vec<SortControl>,
    /// `self.sortMode`
    pub sort_mode: Val,
    /// `self.sortDetail`
    pub sort_detail: Option<Table>,
    /// `self.listBuildFlag`
    pub list_build_flag: bool,
    /// `self.listOutputRevision`
    pub list_output_revision: Option<f64>,
    /// `self.listBuilder` (lent while it runs, ui doc §6.13)
    pub list_builder: Option<Coroutine>,
    /// `self.sortMaxPower`
    pub sort_max_power: Option<f64>,
}

impl NotableDBControl {
    /// lua: NotableDBControl.lua:25-50 NotableDBControl(anchor, rect, itemsTab, db, dbType)
    /// @pob Classes/NotableDBControl.lua:24-50 h=d286c88e
    pub fn new(
        anchor: Anchor,
        rect: [Prop<f64>; 4],
        items_tab: Rc<dyn NotableItemsTab>,
        db: LjIntMap<NodeRef>,
        db_type: Option<&str>,
    ) -> New<NotableDBControl> {
        // L26
        let l = ListControl::state(
            &HOOKS,
            16.0,
            true,
            false,
            ListData::Local(Vec::new()),
            false,
        );
        New::new(
            Base::new(anchor, rect),
            NotableDBControl {
                list: l,
                // L27
                items_tab,
                // L28
                db,
                // L29
                db_type: db_type.map(str::to_owned),
                // L35
                sort_drop_list: Vec::new(),
                // L36
                sort_order: Vec::new(),
                // L37
                sort_mode: Val::from("NAME"),
                sort_detail: None,
                list_build_flag: false,
                list_output_revision: None,
                list_builder: None,
                sort_max_power: None,
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.id();
            // L26 (the scroll bars)
            ListControl::init(cx, me);
            // L30
            cx.w_mut::<ListControl>(me).drag_target_list = Some(Vec::new());
            // L31-L34 self.sortControl: [`SortControl`]
            // L38
            let sel: SelFunc = Rc::new(move |cx, _index, value| {
                // L39
                let Some(entry) = value.and_then(|v| v.get::<Table>().cloned()) else {
                    lua_error!("attempt to index local 'value' (a nil value)")
                };
                NotableDBControl::set_sort_mode(cx, me, entry.get("sortMode"));
            });
            cx.add(
                me,
                "sort",
                DropDownControl::new(
                    Anchor::new(BottomLeft, me, TopLeft),
                    pob_ui_core::layout::rect(0, -22, 360, 18),
                    Vec::new(),
                    Some(sel),
                    None,
                    false,
                ),
            );
            // L41
            let search = cx.add(
                me,
                "search",
                EditControl::new(
                    Anchor::new(BottomLeft, me, TopLeft),
                    pob_ui_core::layout::rect(0, -2, 258, 18),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Search".to_owned()),
                        filter: Some("%c".to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx, _buf| {
                            // L42
                            cx.w_mut::<NotableDBControl>(me).list_build_flag = true;
                        })),
                        clearable: true,
                        ..EditArgs::default()
                    },
                ),
            );
            // L44
            cx.add(
                me,
                "searchMode",
                DropDownControl::new(
                    Anchor::new(Left, search, Right),
                    pob_ui_core::layout::rect(2, 0, 100, 18),
                    vec![
                        DropItem::str("Anywhere"),
                        DropItem::str("Names"),
                        DropItem::str("Modifiers"),
                    ],
                    Some(Rc::new(move |cx, _index, _value| {
                        // L45
                        cx.w_mut::<NotableDBControl>(me).list_build_flag = true;
                    })),
                    None,
                    false,
                ),
            );
            // L47
            NotableDBControl::build_sort_order(cx, me);
            // L48
            cx.w_mut::<NotableDBControl>(me).list_build_flag = true;
        })
    }

    /// lua: NotableDBControl.lua:54-83 DoesNotableMatchFilters(node)
    /// @pob Classes/NotableDBControl.lua:52-83 h=4f933909
    pub fn does_notable_match_filters(cx: &Cx, me: CtrlId, node: &NodeRef) -> bool {
        // L55
        if !IsAnointableNode(node) {
            // L56
            return false;
        }

        // L59
        let buf = cx.get(cx.child_as::<EditControl>(me, "search")).buf.clone();
        let searchStr = pattern::gsub(
            &pob_lua::string::lower(&buf),
            "[%-%.%+%[%]%$%^%%%?%*]",
            "%%%0",
            None,
        )
        .0;
        // L60
        if pattern::r#match(&searchStr, "%S", None).is_some() {
            // L61
            let mut found = false;
            // L62
            let mode = cx
                .get(cx.child_as::<DropDownControl>(me, "searchMode"))
                .sel_index;
            // L63
            if mode == 1 || mode == 2 {
                // L64
                let dn = pob_lua::string::lower(&node.dn.get());
                // L65
                if let Ok(true) = pcall(|| matchOrPattern(&dn, &searchStr)) {
                    // L66
                    found = true;
                }
            }
            // L69
            if mode == 1 || mode == 3 {
                // L70
                for line in node.sd.get().with(Clone::clone) {
                    // L71
                    let line = pob_lua::string::lower(&line);
                    // L72
                    if let Ok(true) = pcall(|| matchOrPattern(&line, &searchStr)) {
                        // L73
                        found = true;
                        // L74
                        break;
                    }
                }
            }
            // L78
            if !found {
                // L79
                return false;
            }
        }
        // L82
        true
    }

    /// lua: NotableDBControl.lua:86-90 SetSortMode(sortMode)
    /// @pob Classes/NotableDBControl.lua:85-90 h=3a250b71
    pub fn set_sort_mode(cx: &mut Cx, me: CtrlId, sort_mode: Val) {
        // L87
        cx.w_mut::<Self>(me).sort_mode = sort_mode;
        // L88
        Self::build_sort_order(cx, me);
        // L89
        cx.w_mut::<Self>(me).list_build_flag = true;
    }

    /// lua: NotableDBControl.lua:92-115 BuildSortOrder()
    /// @pob Classes/NotableDBControl.lua:92-115 h=796c08ba
    pub fn build_sort_order(cx: &mut Cx, me: CtrlId) {
        // L93
        let mut sort_drop_list: Vec<Table> = Vec::new();
        // L94
        for stat in &power_stat_list().stats {
            // L95
            if !stat.ignoreForItems {
                // L96-L102
                let t = Table::new();
                t.set("label", format!("Sort by {}", &*stat.label));
                let sort_mode = stat.itemField.or(stat.stat);
                t.set("sortMode", sort_mode.map_or(Val::Nil, Val::from));
                t.set("itemField", stat.itemField.map_or(Val::Nil, Val::from));
                t.set("stat", stat.stat.map_or(Val::Nil, Val::from));
                t.set(
                    "transform",
                    stat.transform
                        .clone()
                        .map_or(Val::Nil, |f| Val::Obj(Rc::new(f))),
                );
                sort_drop_list.push(t);
            }
        }
        // the sort dropdown's list is this table (L38)
        let sort = cx.child(me, "sort");
        if let Some(sort) = sort {
            cx.w_mut::<DropDownControl>(sort).list = sort_drop_list
                .iter()
                .map(|t| {
                    let label = t.str("label").map_or_else(String::new, |s| s.to_string());
                    DropItem::new(label, Rc::new(t.clone()))
                })
                .collect();
        }
        cx.w_mut::<Self>(me).sort_drop_list = sort_drop_list;
        // L105
        cx.w_mut::<Self>(me).sort_order.clear();
        // L106
        if let Some(sort) = sort {
            // L107
            cx.w_mut::<DropDownControl>(sort).sel_index = 1;
            // L108
            let sort_mode = cx.w::<Self>(me).sort_mode.clone();
            DropDownControl::sel_by_value(cx, sort, &|item: &DropItem| {
                item.get::<Table>()
                    .is_some_and(|t| t.get("sortMode").raw_eq(&sort_mode))
            });
            // L109
            let detail = {
                let dd = cx.w::<DropDownControl>(sort);
                dd.sel_index
                    .checked_sub(1)
                    .and_then(|i| dd.list.get(i))
                    .and_then(|v| v.get::<Table>().cloned())
            };
            cx.w_mut::<Self>(me).sort_detail = detail;
        }
        // L111
        if Self::sort_detail_has_stat(cx, me) {
            // L112
            cx.w_mut::<Self>(me).sort_order.push(SortControl::Stat);
        }
        // L114
        cx.w_mut::<Self>(me).sort_order.push(SortControl::Name);
    }

    /// `self.sortDetail and self.sortDetail.stat`
    fn sort_detail_has_stat(cx: &Cx, me: CtrlId) -> bool {
        cx.w::<Self>(me)
            .sort_detail
            .as_ref()
            .is_some_and(|d| d.truthy("stat"))
    }

    /// lua: NotableDBControl.lua:117-121 CalculatePowerStat(selection, original, modified)
    /// @pob Classes/NotableDBControl.lua:117-121 h=a9ad01fc
    pub fn calculate_power_stat(selection: &Table, original: &Output, modified: &Output) -> f64 {
        // L118
        let originalValue = pob_calc::modules::power::GetFromOutput(original, selection, false);
        // L119
        let modifiedValue = pob_calc::modules::power::GetFromOutput(modified, selection, false);
        // L120
        originalValue - modifiedValue
    }

    /// `GetTime()`
    fn get_time(cx: &Cx) -> f64 {
        cx.sys.time_ms() as f64
    }

    /// `calcFunc(override)`: Ready on desktop; on wasm the builder yields until the worker
    /// answers (DIVERGENCE(wasm-calc), module docs).
    async fn call_calc(calc_func: &Rc<dyn MiscCalc>, ov: Rc<Override>) -> Output {
        loop {
            if let Poll::Ready(out) = calc_func.call(Some(ov.clone()), None) {
                return out;
            }
            co::yield_now().await;
        }
    }

    /// `{ repSlotName = itemType, repItem = item }`
    fn rep_override(item_type: Name, item: ItemRef) -> Rc<Override> {
        let ov = Override::default();
        ov.repSlotName.set(Some(item_type));
        ov.repItem.set(Some(item));
        Rc::new(ov)
    }

    /// lua: NotableDBControl.lua:123-190 ListBuilder() (the coroutine body)
    /// @pob Classes/NotableDBControl.lua:123-190 h=659c3f1b
    pub async fn list_builder(me: CtrlId) {
        // L124
        let mut list: Vec<NodeRef> = Vec::new();
        // L125 (ORDER: module docs)
        let db = co::with_cx(|cx| cx.w::<Self>(me).db.pairs());
        for (_id, node) in db {
            // L126
            if co::with_cx(|cx| Self::does_notable_match_filters(cx, me, &node)) {
                // L127
                list.push(node);
            }
        }

        // L131 stat-based
        if co::with_cx(|cx| Self::sort_detail_has_stat(cx, me)) {
            // L132 local cache = { } (unused)
            // L133
            let mut infinites: Vec<NodeRef> = Vec::new();
            // L134
            let mut start = co::with_cx(|cx| Self::get_time(cx));
            // L135
            let calc_func = co::with_cx(|cx| {
                let it = cx.w::<Self>(me).items_tab.clone();
                it.get_misc_calculator(cx)
            });
            // L136
            let item_type = co::with_cx(|cx| {
                let it = cx.w::<Self>(me).items_tab.clone();
                let Some(display_item) = it.display_item(cx) else {
                    lua_error!("attempt to index field 'displayItem' (a nil value)")
                };
                let Some(base) = display_item.base.get() else {
                    lua_error!("attempt to index field 'base' (a nil value)")
                };
                base.r#type
            });
            // L137
            let base_item = co::with_cx(|cx| {
                let it = cx.w::<Self>(me).items_tab.clone();
                it.anoint_item(cx, None)
            });
            let calcBase =
                Self::call_calc(&calc_func, Self::rep_override(item_type, base_item)).await;
            // L138
            co::with_cx(|cx| cx.w_mut::<Self>(me).sort_max_power = Some(0.0));
            // L139
            let n = list.len();
            for (i, node) in list.iter().enumerate() {
                let nodeIndex = (i + 1) as f64;
                // L140
                node.extra.set("measuredPower", 0.0);
                // L141
                if !node.modKey.get().is_empty() {
                    // L142
                    let item = co::with_cx(|cx| {
                        let it = cx.w::<Self>(me).items_tab.clone();
                        it.anoint_item(cx, Some(node))
                    });
                    let output =
                        Self::call_calc(&calc_func, Self::rep_override(item_type, item)).await;
                    // L143
                    let power = co::with_cx(|cx| {
                        let Some(detail) = cx.w::<Self>(me).sort_detail.clone() else {
                            lua_error!("attempt to index local 'statTable' (a nil value)")
                        };
                        Self::calculate_power_stat(&detail, &output, &calcBase)
                    });
                    node.extra.set("measuredPower", power);
                    // L144
                    if power == f64::INFINITY {
                        // L145
                        infinites.push(node.clone());
                    } else {
                        // L147
                        co::with_cx(|cx| {
                            let w = cx.w_mut::<Self>(me);
                            let max = w.sort_max_power.unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'max' (number expected, got nil)")
                            });
                            w.sort_max_power = Some(m_max(max, power));
                        });
                    }
                }
                // L150
                let now = co::with_cx(|cx| Self::get_time(cx));
                // L151
                if now - start > 50.0 {
                    // L152
                    let text = format!(
                        "^7Sorting... ({}%)",
                        tostring_num(m_floor(nodeIndex / n as f64 * 100.0))
                    );
                    co::with_cx(|cx| cx.w_mut::<ListControl>(me).default_text = Some(text));
                    // L153
                    co::yield_now().await;
                    // L154
                    start = now;
                }
            }

            // L158
            if !infinites.is_empty() {
                // L159
                let max = co::with_cx(|cx| {
                    let w = cx.w_mut::<Self>(me);
                    let m = w.sort_max_power.unwrap_or(0.0) * 2.0;
                    w.sort_max_power = Some(m);
                    m
                });
                // L160
                for node in &infinites {
                    // L161
                    node.extra.set("measuredPower", max);
                }
            }
        }

        // L166
        let sort_order = co::with_cx(|cx| cx.w::<Self>(me).sort_order.clone());
        pob_lua::sort(&mut list, |a, b| {
            // L167
            for data in &sort_order {
                // L168
                let aVal = data.key(a);
                // L169
                let bVal = data.key(b);
                // L170
                if !aVal.raw_eq(&bVal) {
                    // L171-L183 (NAME: ASCEND with func a < b; STAT: DESCEND without func)
                    return match data {
                        SortControl::Stat => lua_lt(&bVal, &aVal),
                        SortControl::Name => lua_lt(&aVal, &bVal),
                    };
                }
            }
            // L186 (nil)
            false
        });

        co::with_cx(|cx| {
            // L188
            cx.w_mut::<ListControl>(me).list =
                ListData::Local(list.into_iter().map(ListValue::of).collect());
            // L189
            cx.w_mut::<ListControl>(me).default_text =
                Some("^7No notables found that match those filters.".to_owned());
        });
    }

    /// Draw L194-L210: start a new ListBuilder when the list must be rebuilt, then resume it
    /// once (the coroutine step of `NotableDBControl:Draw`).
    pub fn step_list_builder(cx: &mut Cx, me: CtrlId) {
        let items_tab = cx.w::<Self>(me).items_tab.clone();
        // L194
        if Some(items_tab.build_output_revision(cx)) != cx.w::<Self>(me).list_output_revision {
            // L195
            cx.w_mut::<Self>(me).list_build_flag = true;
        }
        // L197
        if cx.w::<Self>(me).list_build_flag {
            // L198
            cx.w_mut::<Self>(me).list_build_flag = false;
            // L199
            if let ListData::Local(l) = &mut cx.w_mut::<ListControl>(me).list {
                l.clear();
            }
            // L200
            cx.w_mut::<Self>(me).list_builder = Some(Coroutine::create(Self::list_builder(me)));
            // L201
            let rev = items_tab.build_output_revision(cx);
            cx.w_mut::<Self>(me).list_output_revision = Some(rev);
        }
        // L203
        if cx.w::<Self>(me).list_builder.is_some() {
            // L204
            let (res, err_msg) = cx.lend::<Self, Option<Coroutine>, _>(
                me,
                |w| &mut w.list_builder,
                |cx, slot| resume(slot.as_mut().expect("checked above"), cx),
            );
            // L205
            if Self::dev_mode(cx) && !res {
                // L206
                lua_error!("{}", err_msg.unwrap_or_default());
            }
            // L208
            if cx
                .w::<Self>(me)
                .list_builder
                .as_ref()
                .is_some_and(Coroutine::is_dead)
            {
                // L209
                cx.w_mut::<Self>(me).list_builder = None;
            }
        }
    }
    /// `launch.devMode` (false outside the app, as in the gallery launch).
    fn dev_mode(cx: &Cx) -> bool {
        cx.ext
            .downcast_ref::<crate::app::AppState>()
            .is_some_and(|a| a.launch.dev_mode)
    }
}

/// `local res, errMsg = coroutine.resume(co)`
fn resume(co: &mut Coroutine, cx: &mut Cx) -> (bool, Option<String>) {
    #[cfg(not(target_arch = "wasm32"))]
    {
        co.try_resume(cx)
    }
    #[cfg(target_arch = "wasm32")]
    {
        (co.resume(cx), None)
    }
}

/// The node of a list value.
fn node_of(v: &ListValue) -> NodeRef {
    v.rc::<model::Node>()
        .unwrap_or_else(|| lua_error!("NotableDBControl: a list value that is not a node"))
}

/// lua: NotableDBControl.lua:219-234 GetRowValue(column, index, node)
/// @pob Classes/NotableDBControl.lua:215-234 h=9d556ffe
fn get_row_value(cx: &Cx, me: CtrlId, column: usize, _index: usize, v: &ListValue) -> String {
    let node = node_of(v);
    // L220
    if column == 1 {
        let w = cx.w::<NotableDBControl>(me);
        // L221
        if w.sort_detail.as_ref().is_some_and(|d| d.truthy("stat")) {
            let measured = node.extra.get("measuredPower");
            let max = w.sort_max_power;
            // L222
            let gt0 = |v: &Val| lua_lt(&Val::Num(0.0), v);
            if max.is_some_and(|m| m > 0.0) && measured.truthy() && gt0(&measured) {
                let max = max.expect("checked");
                // L223
                let scaledPower = measured.num() / max;
                // L224
                let powerRed = scaledPower * (255.0 - 128.0) + 128.0;
                // L225
                let powerColor = pob_lua::lformat!("^x%X8080", powerRed);
                // L226
                return format!("{powerColor}{}", node.dn.get());
            }
            // L228
            return format!("^x808080{}", node.dn.get());
        }
        // L231
        return format!("{}{}", cx.main.cc.CRAFTED, node.dn.get());
    }
    // (nil: the list has one column)
    String::new()
}

/// lua: NotableDBControl.lua:239-275 AddValueTooltip(tooltip, index, node)
/// @pob Classes/NotableDBControl.lua:236-275 h=33014dfa
fn add_value_tooltip(cx: &mut Cx, me: CtrlId, tooltip: &mut Tooltip, _index: usize, v: &ListValue) {
    let node = node_of(v);
    // L240 (`self.controls.type` never exists: nil)
    let dropped = |key: &'static str| {
        cx.w::<DropDownControl>(cx.child_as::<DropDownControl>(me, key))
            .dropped
    };
    let type_dropped = cx
        .child(me, "type")
        .is_some_and(|t| cx.w::<DropDownControl>(t).dropped);
    let dropdownDropped = type_dropped || dropped("sort") || dropped("searchMode");
    // L241
    let other_popup = cx
        .main
        .popups
        .first()
        .is_some_and(|p| cx.prop::<PopupDialog, _>(p.id(), |w| &w.title) != "Anoint Item");
    if dropdownDropped || other_popup {
        // L242
        tooltip.clear(false);
        // L243
        return;
    }
    let items_tab = cx.w::<NotableDBControl>(me).items_tab.clone();
    // L245
    let display_item = items_tab.display_item(cx);
    let display_key = display_item.map_or(TtKey::Nil, |i| {
        let o: Rc<dyn std::any::Any> = i;
        TtKey::Obj(TtObj(o))
    });
    if tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(v.data.clone())), display_key]) {
        // Node name
        // L247
        items_tab.add_node_name(cx, tooltip, &node);

        // Node description
        let sd = node.sd.get().with(Clone::clone);
        // L250
        if !sd.is_empty() {
            // L251
            tooltip.add_line(cx, 16.0, "");
            // L252
            let mods = node.mods.get();
            for (i, line) in sd.iter().enumerate() {
                // L253
                let unsupported = &**line != " " && {
                    let Some(m) = mods.get(i + 1) else {
                        lua_error!("attempt to index field '?' (a nil value)")
                    };
                    m.truthy("extra") || !m.truthy("list")
                };
                if unsupported {
                    // L254
                    let line = format!("{}{line}", cx.main.cc.UNSUPPORTED);
                    // L255
                    let line = if cx.main.not_supported_mod_tooltips {
                        format!(
                            "{line}{}",
                            pob_calc::classes::item_tools::main_notSupportedTooltipText
                        )
                    } else {
                        line
                    };
                    // L256
                    tooltip.add_line(cx, 16.0, &line);
                } else {
                    // L258
                    let line = format!("{}{line}", cx.main.cc.MAGIC);
                    tooltip.add_line(cx, 16.0, &line);
                }
            }
        }

        // Reminder text
        let reminder = node.extra.get("reminderText");
        // L264
        if reminder.truthy() {
            // L265
            tooltip.add_separator(cx, Some(14.0));
            // L266
            for line in ipairs_of(&reminder) {
                // L267
                tooltip.add_line(cx, 14.0, &format!("^xA0A080{}", cat(&line)));
            }
        }

        // Comparison
        // L272
        tooltip.add_separator(cx, Some(14.0));
        // L273
        items_tab.append_anoint_tooltip(cx, tooltip, &node);
    }
}

/// lua: NotableDBControl.lua:279-281 GetDragValue(index, node)
/// @pob Classes/NotableDBControl.lua:277-281 h=9b78c0c7
fn get_drag_value(
    _cx: &Cx,
    _me: CtrlId,
    _index: usize,
    v: &ListValue,
) -> Option<(Rc<str>, DragValue)> {
    // L280
    Some((Rc::from("Node"), DragValue(v.data.clone())))
}

/// lua: NotableDBControl.lua:286-288 OnSelClick(index, node, doubleClick)
/// @pob Classes/NotableDBControl.lua:283-288 h=bb48da61
fn on_sel_click(
    _cx: &mut Cx,
    _me: CtrlId,
    _index: usize,
    _v: &ListValue,
    _dbl: bool,
) -> Option<bool> {
    // L287 Do nothing
    None
}

/// lua: NotableDBControl.lua:292-294 OnSelCopy(index, node)
/// @pob Classes/NotableDBControl.lua:290-294 h=bb45a08e
fn on_sel_copy(_cx: &mut Cx, _me: CtrlId, _index: usize, _v: &ListValue) {
    // L293 Copy(item.dn): `item` is an undeclared global // LUA-GLOBAL-NIL
    lua_error!("attempt to index global 'item' (a nil value)")
}

static HOOKS: ListHooks = ListHooks {
    get_row_value,
    add_value_tooltip: Some(add_value_tooltip),
    get_drag_value: Some(get_drag_value),
    on_sel_click: Some(on_sel_click),
    on_sel_copy: Some(on_sel_copy),
    ..ListHooks::BASE
};

impl Widget for NotableDBControl {
    const NAME: &'static str = "NotableDBControl";
    const CAPS: Caps = <ListControl as Widget>::CAPS;

    /// lua: NotableDBControl.lua:193-213 Draw(viewPort)
    /// @pob Classes/NotableDBControl.lua:192-213 h=16b02261
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        // L194-L210
        Self::step_list_builder(cx, me);
        // L212 (the super Draw gets noTooltip nil)
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

    pob_ui_core::impl_parts!(NotableDBControl: list => ListControl);
}

#[cfg(test)]
mod tests {
    //! The sort coroutine (yield points, progress text, final order, infinite powers), the row
    //! colours and the filters (`matchOrPattern` has its L0 test in `pob-lua`, `l0_string.rs`). The
    //! draw path is the gallery's (`pob-ui-oracle`, `u28_notable`).
    use std::cell::RefCell;

    use pob_host::headless::{services, Headless, HeadlessState};

    use super::*;
    use crate::model::LStr;

    struct Host;
    impl Widget for Host {
        const NAME: &'static str = "ControlHost";
        const CAPS: Caps = Caps::empty();
    }

    #[test]
    fn row_colour_format_truncates_like_luajit() {
        // luajit: string.format("^x%X8080", 191.5) = ^xBF8080, 128.25 = ^x808080, 254.999 = ^xFE8080
        assert_eq!(pob_lua::lformat!("^x%X8080", 191.5), "^xBF8080");
        assert_eq!(pob_lua::lformat!("^x%X8080", 128.25), "^x808080");
        assert_eq!(pob_lua::lformat!("^x%X8080", 254.999), "^xFE8080");
    }

    const POWER_STATS: &str = r#"{
        "GetFromOutput": {"$fn": "data.powerStatList.GetFromOutput", "src": "Modules/Data.lua:186-212", "hash": "x"},
        "$seq": [
            {"combinedOffDef": true, "ignoreForItems": true, "label": "Offence/Defence"},
            {"ignoreForNodes": true, "itemField": "Name", "label": "Name", "reverseSort": true,
             "transform": {"$fn": "data.powerStatList[2].transform", "src": "Modules/Data.lua:131-131", "hash": "x"}},
            {"label": "Full DPS", "stat": "FullDPS"},
            {"label": "Life", "stat": "Life"}
        ]
    }"#;

    /// `anointItem(node)` carries the node's power; calcFunc answers it and advances the clock.
    struct FakeItemsTab {
        clock: Headless,
        display: ItemRef,
        calls: Rc<RefCell<Vec<String>>>,
    }

    struct FakeCalc {
        clock: Headless,
        calls: Rc<RefCell<Vec<String>>>,
    }

    impl MiscCalc for FakeCalc {
        fn call(&self, ov: Option<Rc<Override>>, _full: Option<bool>) -> Poll<Output> {
            let ov = ov.expect("an override");
            let item = ov.repItem.get().expect("repItem");
            let power = item.extra.get("power");
            self.calls.borrow_mut().push(format!(
                "calc {} {}",
                ov.repSlotName.get().expect("repSlotName"),
                tostring_num(power.num_or(-1.0))
            ));
            self.clock.clock_ms.set(self.clock.clock_ms.get() + 30);
            let out = Output::new();
            out.set("FullDPS", power.num_or(100.0));
            Poll::Ready(out)
        }
    }

    impl NotableItemsTab for FakeItemsTab {
        fn build_output_revision(&self, _cx: &Cx) -> f64 {
            1.0
        }
        fn get_misc_calculator(&self, _cx: &mut Cx) -> Rc<dyn MiscCalc> {
            Rc::new(FakeCalc {
                clock: self.clock.clone(),
                calls: self.calls.clone(),
            })
        }
        fn display_item(&self, _cx: &Cx) -> Option<ItemRef> {
            Some(self.display.clone())
        }
        fn anoint_item(&self, _cx: &mut Cx, node: Option<&NodeRef>) -> ItemRef {
            let item = Rc::new(model::Item::default());
            if let Some(n) = node {
                item.extra.set("power", n.extra.get("fakePower"));
            }
            item
        }
        fn add_node_name(&self, cx: &mut Cx, tooltip: &mut Tooltip, node: &NodeRef) {
            tooltip.add_line(cx, 20.0, &format!("^7{}", node.dn.get()));
        }
        fn append_anoint_tooltip(&self, cx: &mut Cx, tooltip: &mut Tooltip, _node: &NodeRef) {
            tooltip.add_line(cx, 14.0, "^7anoint");
        }
    }

    fn node(id: f64, dn: &str, recipe: bool, mod_key: &str, power: f64) -> NodeRef {
        let n = model::Node {
            id,
            ..model::Node::default()
        };
        n.dn.set(LStr::from(dn));
        n.modKey.set(LStr::from(mod_key));
        n.sd.set(pob_lua::Shared::new(vec![LStr::from(format!("{dn} line"))]));
        if recipe {
            let r = Table::new();
            r.push("AmberOil");
            n.extra.set("recipe", r);
        }
        n.extra.set("fakePower", power);
        Rc::new(n)
    }

    fn setup() -> (Cx, CtrlId, Headless, Rc<RefCell<Vec<String>>>, Vec<NodeRef>) {
        let list: &'static PowerStatList = Box::leak(Box::new(
            serde_json::from_str(POWER_STATS).expect("power stats"),
        ));
        set_power_stat_list(Some(list));
        let mut cx = Cx::headless();
        let h = HeadlessState::new("", "");
        let (p, f, n) = services(&h);
        cx.sys = pob_sg::Sys::new(p, f, n);
        let host = cx
            .add_free(
                None,
                New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), Host).host(),
            )
            .id();
        let base: model::gamedata::ItemBase =
            serde_json::from_str(r#"{"type": "Amulet", "tags": {}, "req": {}}"#).expect("base");
        let display = Rc::new(model::Item::default());
        display.base.set(Some(std::sync::Arc::new(base)));
        let calls = Rc::new(RefCell::new(Vec::new()));
        let items_tab = Rc::new(FakeItemsTab {
            clock: h.clone(),
            display,
            calls: calls.clone(),
        });
        let nodes = vec![
            node(1.0, "Bravery", true, "k1", 50.0),
            node(2.0, "Arsonist", true, "k2", 200.0),
            node(3.0, "Not Anointable", false, "k3", 999.0),
            node(4.0, "Divine Fury", true, "k4", f64::INFINITY),
            node(5.0, "Cold Heart", true, "", 0.0),
            node(6.0, "Explosive", true, "k6", 50.0),
            node(7.0, "Arcane Focus", true, "k7", 10.0),
        ];
        let mut db: LjIntMap<NodeRef> = LjIntMap::new();
        for n in &nodes {
            db.set(n.id as i64, Some(n.clone()));
        }
        let me = cx
            .add(
                host,
                "db",
                NotableDBControl::new(
                    Anchor::NONE,
                    rect(10, 60, 360, 360),
                    items_tab,
                    db,
                    Some("ANOINT"),
                ),
            )
            .id();
        (cx, me, h, calls, nodes)
    }

    fn names(cx: &Cx, me: CtrlId) -> Vec<String> {
        (1..=ListControl::list_len(cx, me))
            .map(|i| {
                node_of(&ListControl::list_get(cx, me, i).unwrap())
                    .dn
                    .get()
                    .to_string()
            })
            .collect()
    }

    /// Draw's coroutine part (L194-L210), without the ListControl draw (no fonts here).
    fn step(cx: &mut Cx, me: CtrlId) {
        NotableDBControl::step_list_builder(cx, me);
    }

    #[test]
    fn name_sort_is_the_default_and_finishes_in_one_resume() {
        let (mut cx, me, _h, calls, _nodes) = setup();
        // L94-L114: entry 1 is ignoreForItems; "Sort by Name" (sortMode "Name") is selected
        // because SelByValue("NAME", "sortMode") finds nothing and selIndex stays 1.
        let sort = cx.child(me, "sort").unwrap();
        let labels: Vec<String> = cx
            .w::<DropDownControl>(sort)
            .list
            .iter()
            .map(|i| i.label.clone())
            .collect();
        assert_eq!(labels, ["Sort by Name", "Sort by Full DPS", "Sort by Life"]);
        assert_eq!(
            cx.w::<NotableDBControl>(me).sort_order,
            vec![SortControl::Name]
        );
        step(&mut cx, me);
        assert!(cx.w::<NotableDBControl>(me).list_builder.is_none());
        assert!(calls.borrow().is_empty());
        assert_eq!(
            names(&cx, me),
            [
                "Arcane Focus",
                "Arsonist",
                "Bravery",
                "Cold Heart",
                "Divine Fury",
                "Explosive"
            ]
        );
        assert_eq!(
            cx.w::<ListControl>(me).default_text.as_deref(),
            Some("^7No notables found that match those filters.")
        );
        let row = get_row_value(&cx, me, 1, 1, &ListControl::list_get(&cx, me, 1).unwrap());
        assert_eq!(row, "^xB8DAF1Arcane Focus");
        // a second Draw with the same outputRevision does not rebuild
        step(&mut cx, me);
        assert!(cx.w::<NotableDBControl>(me).list_builder.is_none());
    }

    #[test]
    fn stat_sort_yields_every_50ms_and_orders_by_power() {
        let (mut cx, me, h, calls, nodes) = setup();
        // the sort dropdown's selFunc: SetSortMode("FullDPS")
        let sort = cx.child(me, "sort").unwrap();
        DropDownControl::set_sel(&mut cx, sort, 2, false);
        assert_eq!(
            cx.w::<NotableDBControl>(me).sort_order,
            vec![SortControl::Stat, SortControl::Name]
        );
        assert_eq!(cx.w::<DropDownControl>(sort).sel_index, 2);
        h.clock_ms.set(1000);
        // start = 1000; calcBase -> 1030; the filtered nodes in LjIntMap order (ids 1..7 are the
        // array part) 1, 2, 4, 5, 6, 7: 1 -> 1060 (yield at 1/6), 2 -> 1090, 4 -> 1120 (yield at
        // 3/6), 5 has no modKey (no calc), 6 -> 1150, 7 -> 1180 (yield at 6/6).
        let mut texts = Vec::new();
        for _ in 0..3 {
            step(&mut cx, me);
            assert!(cx.w::<NotableDBControl>(me).list_builder.is_some());
            assert_eq!(
                ListControl::list_len(&cx, me),
                0,
                "the list stays wiped while sorting"
            );
            texts.push(cx.w::<ListControl>(me).default_text.clone().unwrap());
        }
        assert_eq!(
            texts,
            [
                "^7Sorting... (16%)",
                "^7Sorting... (50%)",
                "^7Sorting... (100%)"
            ]
        );
        step(&mut cx, me);
        assert!(cx.w::<NotableDBControl>(me).list_builder.is_none());
        assert_eq!(
            *calls.borrow(),
            [
                "calc Amulet -1",
                "calc Amulet 50",
                "calc Amulet 200",
                "calc Amulet inf",
                "calc Amulet 50",
                "calc Amulet 10"
            ]
        );
        // measuredPower = output - base (the base item has no power: 100); the infinite one gets
        // 2 * sortMaxPower (L158-L161)
        let power = |i: usize| nodes[i].extra.get("measuredPower").num();
        assert_eq!(power(0), -50.0);
        assert_eq!(power(1), 100.0);
        assert_eq!(power(3), 200.0);
        assert_eq!(power(4), 0.0);
        assert_eq!(power(6), -90.0);
        assert_eq!(cx.w::<NotableDBControl>(me).sort_max_power, Some(200.0));
        assert_eq!(
            names(&cx, me),
            [
                "Divine Fury",
                "Arsonist",
                "Cold Heart",
                "Bravery",
                "Explosive",
                "Arcane Focus"
            ]
        );
        // L222-L228 colours: 200/200 -> FF, 100/200 -> 0xBF (191.5), <= 0 -> 808080
        let row =
            |i: usize| get_row_value(&cx, me, 1, i, &ListControl::list_get(&cx, me, i).unwrap());
        assert_eq!(row(1), "^xFF8080Divine Fury");
        assert_eq!(row(2), "^xBF8080Arsonist");
        assert_eq!(row(3), "^x808080Cold Heart");
        assert_eq!(row(4), "^x808080Bravery");
    }

    #[test]
    fn search_filters_names_and_modifiers() {
        let (mut cx, me, _h, _calls, _nodes) = setup();
        step(&mut cx, me);
        let search = cx.child(me, "search").unwrap();
        EditControl::set_text(&mut cx, search, "ar(s|c)", true);
        assert!(
            cx.w::<NotableDBControl>(me).list_build_flag,
            "changeFunc sets listBuildFlag"
        );
        step(&mut cx, me);
        assert_eq!(names(&cx, me), ["Arcane Focus", "Arsonist"]);
        // "Modifiers" only: every node's sd line is "<dn> line"
        let mode = cx.child(me, "searchMode").unwrap();
        DropDownControl::set_sel(&mut cx, mode, 3, false);
        EditControl::set_text(&mut cx, search, "heart line", true);
        step(&mut cx, me);
        assert_eq!(names(&cx, me), ["Cold Heart"]);
        // "Names" only
        DropDownControl::set_sel(&mut cx, mode, 2, false);
        step(&mut cx, me);
        assert!(names(&cx, me).is_empty());
        // a malformed pattern matches nothing (PCall); the escape list leaves `(` alone
        DropDownControl::set_sel(&mut cx, mode, 1, false);
        EditControl::set_text(&mut cx, search, "(", true);
        step(&mut cx, me);
        assert!(names(&cx, me).is_empty());
        // `%`, `.`, `-` are escaped (L59): "cold heart." matches nothing, "cold heart" does
        EditControl::set_text(&mut cx, search, "cold heart.", true);
        step(&mut cx, me);
        assert!(names(&cx, me).is_empty());
        EditControl::set_text(&mut cx, search, "   ", true);
        step(&mut cx, me);
        assert_eq!(names(&cx, me).len(), 6, "blank search: no filter (L60)");
    }

    #[test]
    fn on_sel_copy_raises_like_pob() {
        let (mut cx, me, _h, _calls, nodes) = setup();
        let v = ListValue::of(nodes[0].clone());
        let r = pcall(|| on_sel_copy(&mut cx, me, 1, &v));
        assert_eq!(
            r.unwrap_err(),
            "attempt to index global 'item' (a nil value)"
        );
    }
}
