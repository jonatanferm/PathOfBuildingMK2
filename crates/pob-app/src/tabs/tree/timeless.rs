//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TreeTab.lua:1170-2888 (FindTimelessJewel).
//! Owner: U27.
//!
//! @pob-file Classes/TreeTab.lua @16de4b82d
//! @pob-file Classes/TradeQueryRequests.lua @16de4b82d
//!
//! `TreeTabClass:FindTimelessJewel()`: the "Find a Timeless Jewel" popup (jewel type, conqueror,
//! socket, filters, the node weight sliders with their `tooltip.Draw` overrides, the desired and
//! fallback node lists, the seed search over the legion / abyss LUTs, the result list and the
//! trade URL). The Lua is one function whose locals are shared by every control callback; here
//! they are the fields of one [`Up`] (`Rc`) that each closure captures (ui doc §7.1, shared
//! mutable upvalues). The local functions are methods of [`Up`] with their Lua names.
//! `controls` is the popup node (ui doc §6.10); `main:OpenPopup` runs last in Lua, so the whole
//! body runs inside [`popups::open_popup`]'s builder, in Lua order.
//!
//! The TreeTab fields this function reads and writes (`self.allocatedNodesInRadiusCount`,
//! `self.tradeQueryRequests`, `self.timelessJewelLeagueSelect`, `self.realmList`,
//! `self.selectedRealmIndex`, `self.tradeTypeIndex`, `self.lastSearchMore`) are [`State`];
//! `self.tradeLeaguesList` is U25's (`core::State::trade_leagues_list`, TreeTab.lua:189).
//!
//! Lua semantics:
//! * The jewel types, conquerors, devotion variants, sockets and ascendancy options are Lua
//!   tables ([`Table`]): `timelessData.jewelType = value` stores the very entry, and
//!   `timelessData.sharedResults.type = timelessData.jewelType` aliases it, as the result list
//!   (U28) reads it. Dropdown entries carry that `Table` as their value.
//! * `modData` is one table the `nodeSelect` dropdown aliases (`DropDownControl(.., modData, ..)`):
//!   `buildMods` refills it in place. DropDownControl DIVERGENCE(alias): the port assigns the
//!   dropdown's list from the table after each `buildMods`. `controls.conquerorSelect.list = ..`
//!   is the same raw assignment.
//! * `controls.searchTradeLeagueSelect:SetList(leagueList)` wipes the dropdown's previous list
//!   table first (DropDownControl.lua:530), which is a `self.tradeLeaguesList[realm]` table once
//!   a list was set: switching the realm empties the previous realm's cached leagues, so going
//!   back fetches them again. [`Up::league_set_list`] reproduces it (the dropdown's current
//!   list table is tracked in [`Up::league_list`]).
//! * The search rows (`searchListTbl`) hold the strings `gmatch` produced and, after a slider
//!   moved, numbers (`getNodeWeights`); `t_concat` writes both (`%.14g`).
//! * `desiredNode[4] < 0.001` after a `"required"` column compares a string with a number when
//!   the fourth column came from the text (`"id, required, 0, 0"`): PoB raises "attempt to
//!   compare string with number", and so does the port.
//! * `legionNode.modListGenerated` is a cache written into the shared LegionPassives tables
//!   (`require` caches the module, so it lives for the whole process, across builds). Ported as
//!   is: the key `modListGenerated` of the legion node's `extra` / the addition table. Its value
//!   is either a mod list ([`ModListVal`]) or a list of `{ modList, divisor }` tables. The
//!   addition branch's `elseif legionAddition.modListGenerated` sets no divisor, so a second
//!   "Generate" in the same process scores those additions without the `/100` of the first.
//! * The fallback-weight pseudo nodes (`{ id, modList, divisor }`) are passed to the misc
//!   calculator as `addNodes`: the port builds a [`Node`] with that `modList` and a NaN id
//!   (CalcSetup.lua:717 `nodes[node.id] = node` keys it under a string in Lua; the calc's
//!   id-keyed map puts it under one fixed key, and every call adds exactly one node). A `nil`
//!   `modList` (a line `parseMod` does not know) adds nothing (`ModList:AddList` skips nil,
//!   ModList.lua:88-94): the node gets an empty list.
//! * `self.tradeQueryRequests = new("TradeQueryRequests"):TradeQueryRequests()`: TradeQuery*
//!   is U51 (optional, deferred). This function only calls `FetchLeagues`
//!   (TradeQueryRequests.lua:468-495, one `launch:DownloadPage`), which is ported locally as
//!   [`fetch_leagues`] (CCR-0132); the object keeps its `hostName` only.
//! * `dkjson.encode(search)` (the trade URL's query): this dkjson sorts object keys
//!   (`sortedkeys`, dkjson.lua:133-151), so the text is deterministic; [`Json::encode`] is the
//!   subset the query uses (objects, arrays, strings, numbers via `tostring`).
//!
//! DIVERGENCE(closed-popup-callback): the league list arrives through `launch:DownloadPage`, a
//! frame or more later. When the popup was closed meanwhile, Lua's callback writes into the
//! closed popup's controls (the objects live on until collected; nothing draws them). The port
//! skips those control writes when the popup node is dead (its id would panic, D8); the
//! `self.tradeLeaguesList` update happens as in Lua.
//! DIVERGENCE(viewer-tooltip): `self.viewer:AddNodeTooltip(tooltip, node, self.build, true)`
//! (L1560, L1598, L1732) lends the TreeTab's viewer to U24's `add_node_tooltip` with
//! `returnEarly = true` through [`TimelessHooks::add_node_tooltip`] (the product's default calls
//! U24's method; tests may stand in for it).
//!
//! ORDER:
//! * `pairs(legionNodes)` (L1294, L1362), `pairs(legionAdditions)` (L1386): sequences, index
//!   order on both sides (c).
//! * `pairs(self.build.spec.nodes)` (L1311): the socket list is sorted by label afterwards and
//!   every label holds the socket id (c).
//! * `pairs(treeData.nodes[socketId].nodesInRadius[3])` (L1322): the nearest keystone with a
//!   strict `<`; distinct keystones are never at the same distance from a socket (c).
//! * `pairs(self.build.spec.allocNodes)` (L1613), `pairs(radiusNodes)` (L1622): the options
//!   are sorted by name afterwards; two allocated notables of one name in one radius would keep
//!   their LjIntMap order (c, by the tree data: no such pair).
//! * `pairs(legionPassive.stats)` (L2011), `pairs(legionAddition.stats)` (L2093), the abyss
//!   component's `pairs(node.stats)`: set membership or the single matching entry (c).
//! * `pairs(treeData.classes)` (L2520), the `pairs(radiusNodes)` of L2525/L2534: sets and counts
//!   (c). The `targetNodes` set they fill is an [`LjIntMap`] in that insertion order; its
//!   `pairs` (L2593) fixes the order of each result's `targetNodeNames` and the order of the
//!   float additions into `totalWeight`. `nodesInRadius` comes from `pairs(tree.nodes)`, whose
//!   order is per-process in PoB (the `root` string key, ui doc §8 rule 6): (b) for the
//!   result list's node-name tooltips; the totals are exact for weights that are binary
//!   fractions (the scenarios use such weights).
//! * `pairs(data.readAbyssJewelLUT(..))` (L2562): LjIntMap, LuaJIT's order (a).
//! * `pairs(resultNodes)` (L2750): seeds stored in ascending order with `nil` stores for the
//!   dropped seeds, an [`LjIntMap`] (a): the results' order before the unstable `t_sort`
//!   (ties on `total` are common) is LuaJIT's.
//! * `pairs(desiredNodes)` (L2785): fills `sortedNodeArray` by `desiredIdx` and sets
//!   `result[legionId]` (c).
//! * `pairs(json_data.result)` (FetchLeagues): a JSON array, index order (c).
//!
//! Props with side effects (ui doc §7.3): none (`protectAllocatedLabel`'s `y` and `shown`, the
//! buttons' `locked` only read).

#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::float_cmp,
    // Lua statement shapes kept verbatim (STYLE §3.1 rule 3)
    clippy::assign_op_pattern,
    clippy::format_push_string,
    clippy::manual_range_contains,
    clippy::needless_late_init,
    clippy::used_underscore_binding,
    clippy::explicit_auto_deref,
    clippy::option_option,
    clippy::unused_self,
    clippy::semicolon_if_nothing_returned
)]

use std::cell::{Cell, RefCell};
use std::collections::{BTreeMap, HashMap};
use std::rc::Rc;
use std::sync::Arc;

use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::{lformat, round, tostring_num, LjIntMap};
use pob_ui_core::controls::drop_down::{DropTipArgs, DropTipMode, SelFunc};
use pob_ui_core::controls::slider::{ScrollWheelSpeedTbl, SliderControl};
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl,
    LabelControl, ListControl,
};
use pob_ui_core::input::InputEvents;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::timeless_list::{AppTimelessBuild, TimelessJewelListControl};
use super::timeless_socket::{SocketViewer, TimelessJewelSocketControl};
use super::TreeTab;
use crate::app::CxApp;
use crate::controls::gem_tooltip::stat_describer;
use crate::model::{self, ModList, Node, NodeRef, Override, PassiveTreeRef, Shared, Table, Val};
use crate::platform::net::{download_page, DownloadParams, PageResponse};
use crate::tree_view::PassiveTreeView;
// `ModStore::ModList` (new("ModList")), `node_id_val` and `GetFromOutput` are not re-exported by
// model.rs (U15; the same gap as CCR-0103 §4 / CCR-0115 / CCR-0123 §9)
use pob_calc::classes::passive_tree::node_id_val;
use pob_calc::classes::ModStore;
use pob_calc::modules::power::{powerStat_table, GetFromOutput};

// ---- the TreeTab's part (ui doc §12.1) --------------------------------------------------------

/// `self.tradeQueryRequests` (TradeQueryRequests.lua:24-34; only `hostName` is read here).
#[derive(Clone, Debug)]
pub struct TradeQueryRequests {
    /// `self.hostName`
    pub host_name: String,
}

/// U27's part of the TreeTab object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.allocatedNodesInRadiusCount`
    pub allocated_nodes_in_radius_count: f64,
    /// `self.tradeQueryRequests` (nil until the first FindTimelessJewel)
    pub trade_query_requests: Option<TradeQueryRequests>,
    /// `self.timelessJewelLeagueSelect` (the league dropdown's last pick)
    pub timeless_jewel_league_select: Option<String>,
    /// `self.realmList`
    pub realm_list: Vec<String>,
    /// `self.selectedRealmIndex`
    pub selected_realm_index: Option<usize>,
    /// `self.tradeTypeIndex`
    pub trade_type_index: Option<usize>,
    /// `self.lastSearchMore`
    pub last_search_more: Option<bool>,
}

/// The calls this unit makes into another unit's UI (DIVERGENCE(viewer-tooltip)).
#[derive(Clone)]
pub struct TimelessHooks {
    /// `self.viewer:AddNodeTooltip(tooltip, node, self.build, true)` (U24)
    pub add_node_tooltip: Rc<dyn Fn(&mut Cx, &mut PassiveTreeView, &mut Tooltip, &NodeRef)>,
}

pub fn default_timeless_hooks() -> TimelessHooks {
    TimelessHooks {
        add_node_tooltip: Rc::new(|cx, view, tt, node| view.add_node_tooltip(cx, tt, node, true)),
    }
}

thread_local! {
    static HOOKS: RefCell<Option<TimelessHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn timeless_hooks() -> TimelessHooks {
    HOOKS.with(|h| h.borrow_mut().get_or_insert_with(default_timeless_hooks).clone())
}

/// Replace hook entries (tests).
pub fn set_timeless_hooks(f: impl FnOnce(&mut TimelessHooks)) {
    HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_timeless_hooks)));
}

fn tt_state(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<TreeTab>(me).timeless
}

// ---- Lua value helpers ----------------------------------------------------------------------

/// A `modList` value of the Lua (a ModList object, or the mod array `modLib.parseMod` returns).
pub struct ModListVal(pub ModList);

/// Lua `a .. b` operand: a string, or a number (`%.14g`).
#[track_caller]
fn cat(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `tonumber(v)`
fn tonumber(v: &Val) -> Option<f64> {
    match v {
        Val::Num(x) => Some(*x),
        Val::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

/// `v < x` / `v > x` operand: a number (anything else raises, as Lua's `<` does).
#[track_caller]
fn cmp_num(v: &Val) -> f64 {
    match v {
        Val::Num(x) => *x,
        other => lua_error!("attempt to compare {} with number", other.lua_type()),
    }
}

/// `t_concat(t, sep)` (lib_table.c `table.concat`: strings and numbers up to `#t`).
#[track_caller]
fn t_concat(t: &Table, sep: &str) -> String {
    let n = t.len();
    let mut out = String::new();
    for i in 1..=n {
        if i > 1 {
            out.push_str(sep);
        }
        match t.get(i as i64) {
            Val::Str(s) => out.push_str(&s),
            Val::Num(x) => out.push_str(&tostring_num(x)),
            other => lua_error!(
                "invalid value (at index {i}) in table for 'concat' ({})",
                other.lua_type()
            ),
        }
    }
    out
}

/// `t_concat` of a string list.
fn concat_strs(v: &[model::LStr], sep: &str) -> String {
    v.iter().map(|s| &**s).collect::<Vec<_>>().join(sep)
}

/// `s:match(p)` truthiness.
fn lmatch(s: &str, p: &str) -> bool {
    pob_lua::lmatch(s, p, None).is_some()
}

/// `isValueInArray(tbl, val)` over a string array.
fn in_array(tbl: &[&str], v: &str) -> bool {
    tbl.contains(&v)
}

/// The integer key of a node id value (`t[id]`; `None` reads nil).
fn int_key(v: &Val) -> Option<i64> {
    v.opt_num()
        .filter(|n| n.fract() == 0.0 && n.abs() < 9.0e15)
        .map(|n| n as i64)
}

/// `{ ... }` of strings (`copyTable(node.sd)`).
fn str_table(v: &[model::LStr]) -> Table {
    let t = Table::new();
    for s in v {
        t.push(Val::Str(s.clone()));
    }
    t
}

/// Dropdown entries of a list of tables: label = `entry.label`, value = the table.
fn table_items(t: &Table) -> Vec<DropItem> {
    t.ipairs()
        .into_iter()
        .map(|(_, v)| match v {
            Val::Table(e) => {
                let label = e
                    .str("label")
                    .unwrap_or_else(|| lua_error!("attempt to concatenate field 'label' (a nil value)"));
                DropItem::new(label.to_string(), Rc::new(e))
            }
            other => DropItem::str(cat(&other)),
        })
        .collect()
}

/// The table of a dropdown entry (nil for a string entry).
fn item_table(item: Option<&DropItem>) -> Option<Table> {
    item.and_then(|i| i.get::<Table>().cloned())
}

/// `t_sort(t, function(a, b) return a.label < b.label end)` on a list of tables.
fn sort_by_label(t: &Table) {
    let mut v: Vec<Table> = t
        .ipairs()
        .into_iter()
        .map(|(_, e)| e.table().unwrap_or_else(|| lua_error!("attempt to index local 'a' (a nil value)")))
        .collect();
    let label = |e: &Table| e.str("label").unwrap_or_else(|| lua_error!("attempt to compare two nil values"));
    pob_lua::sort(&mut v, |a, b| label(a).as_bytes() < label(b).as_bytes());
    for (i, e) in v.into_iter().enumerate() {
        t.set(i as i64 + 1, e);
    }
}

/// `pairs(v)` of a table value (a mutable table or a data table): key/value snapshot.
fn val_pairs(v: &Val) -> Vec<(Val, Val)> {
    match v {
        Val::Table(t) => t.pairs().map(|(k, v)| (k.to_val(), v)).collect(),
        Val::Data(model::gamedata::value::Value::Table(t)) => t
            .array
            .iter()
            .enumerate()
            .map(|(i, v)| (Val::Num(i as f64 + 1.0), Val::from_value(v)))
            .chain(
                t.fields
                    .iter()
                    .map(|(k, v)| (Val::Str(model::LStr::from(k.as_str())), Val::from_value(v))),
            )
            .collect(),
        other => lua_error!("bad argument #1 to 'pairs' (table expected, got {})", other.lua_type()),
    }
}

/// `ipairs(v)` of a table value.
fn val_ipairs(v: &Val) -> Vec<Val> {
    let mut out = Vec::new();
    let mut i = 1i64;
    loop {
        let e = v.get(i);
        if e.is_nil() {
            break;
        }
        out.push(e);
        i += 1;
    }
    out
}

/// `#v` of a list value.
fn val_len(v: &Val) -> usize {
    match v {
        Val::Table(t) => t.len(),
        Val::Data(_) => val_ipairs(v).len(),
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// A legion passive: a legion node (a [`Node`], its untyped fields in `extra`) or a legion
/// addition (a table).
#[derive(Clone)]
enum Lp {
    Node(NodeRef),
    Add(Table),
}

impl Lp {
    fn of(v: &Val) -> Lp {
        if let Some(n) = v.obj::<Node>() {
            Lp::Node(n)
        } else if let Some(t) = v.table() {
            Lp::Add(t)
        } else {
            lua_error!("attempt to index local 'node' (a {} value)", v.lua_type())
        }
    }
    /// `x.id`
    fn id(&self) -> Val {
        match self {
            Lp::Node(n) => node_id_val(n),
            Lp::Add(t) => t.get("id"),
        }
    }
    /// `x.id` as a string operand of `:match`
    fn id_str(&self) -> String {
        match self.id() {
            Val::Str(s) => s.to_string(),
            other => lua_error!("attempt to index field 'id' (a {} value)", other.lua_type()),
        }
    }
    /// `x.dn`
    fn dn(&self) -> Val {
        match self {
            Lp::Node(n) => Val::Str(n.dn.get()),
            Lp::Add(t) => t.get("dn"),
        }
    }
    /// `x.sd` (a list of strings)
    fn sd(&self) -> Vec<model::LStr> {
        match self {
            Lp::Node(n) => n.sd.get().snapshot(),
            Lp::Add(t) => val_ipairs(&t.get("sd")).iter().map(|s| model::LStr::from(cat(s).as_str())).collect(),
        }
    }
    /// `x[k]` of an untyped field
    fn get(&self, k: &'static str) -> Val {
        match self {
            Lp::Node(n) => n.extra.get(k),
            Lp::Add(t) => t.get(k),
        }
    }
    /// `x[k] = v`
    fn set(&self, k: &'static str, v: Val) {
        match self {
            Lp::Node(n) => n.extra.set(k, v),
            Lp::Add(t) => t.set(k, v),
        }
    }
    /// `#x.sortedStats`
    fn sorted_stats_len(&self) -> usize {
        val_len(&self.get("sortedStats"))
    }
    /// `x.modList` (a legion node's ModList; an addition's `modList` field, nil in the data)
    fn mod_list(&self) -> Option<ModList> {
        match self {
            Lp::Node(n) => Some(n.modList.get()),
            Lp::Add(t) => t.obj::<ModListVal>("modList").map(|m| m.0.clone()),
        }
    }
}

/// `legionNodes[i]` / `legionAdditions[i]` for a Lua number index (nil outside the list).
fn lp_at(t: &Table, i: f64) -> Option<Lp> {
    let v = if i.fract() == 0.0 { t.get(i) } else { Val::Nil };
    (!v.is_nil()).then(|| Lp::of(&v))
}

/// `modLib.parseMod(line)` as a mod list (nil when the line is not recognised).
fn parse_mod(line: &str) -> Option<ModList> {
    let (mods, _extra) = pob_modparser::parseMod(line, false);
    let mods = mods?;
    let ml = ModStore::ModList(None);
    for m in mods {
        ml.AddMod(Arc::new(m));
    }
    Some(ml)
}

/// `modList[1].value` in arithmetic.
#[track_caller]
fn first_mod_value(ml: Option<&ModList>) -> f64 {
    let Some(ml) = ml else { lua_error!("attempt to index field 'modList' (a nil value)") };
    match ml.all_mods().first() {
        Some(m) => Val::from_value(&m.value).num(),
        None => lua_error!("attempt to index field '?' (a nil value)"),
    }
}

/// `newNode` of setupFallbackWeights: `{ id, calcMultiple, node, modList, divisor }`.
struct NewNode {
    id: Val,
    /// `newNode.calcMultiple` (set, never read)
    _calc_multiple: bool,
    /// `newNode.node`: the `{ modList, divisor, id }` per stat
    node: Option<Vec<SubNode>>,
    mod_list: Option<ModList>,
    divisor: Option<f64>,
}

/// One `{ modList = .., divisor = 100, id = .. }` of `newNode.node`.
struct SubNode {
    mod_list: Option<ModList>,
    divisor: Option<f64>,
    _id: Val,
}

/// `powerEntry` of generateFallbackWeights.
struct PowerEntry {
    id: Val,
    weight1: Option<f64>,
    weight2: Option<f64>,
}

/// The table `searchSingleSocket` returns.
struct SearchResult {
    result_nodes: LjIntMap<Table>,
    seed_weights: HashMap<i64, f64>,
    desired_nodes: Table,
    _socket_info: Option<Table>,
}

// ---- dkjson.encode (the subset the trade query uses) ------------------------------------------

/// A value `dkjson.encode` writes.
enum Json {
    Num(f64),
    Str(String),
    Arr(Vec<Json>),
    /// keys sorted bytewise (dkjson.lua:133-151 `sortedkeys`, all keys strings)
    Obj(BTreeMap<String, Json>),
}

impl Json {
    fn obj(kv: Vec<(&str, Json)>) -> Json {
        Json::Obj(kv.into_iter().map(|(k, v)| (k.to_owned(), v)).collect())
    }
    /// dkjson.lua:164-178 `quotestring` (the ASCII escapes; the query holds ASCII only)
    fn quote(s: &str, out: &mut String) {
        out.push('"');
        for ch in s.chars() {
            match ch {
                '"' => out.push_str("\\\""),
                '\\' => out.push_str("\\\\"),
                '\u{8}' => out.push_str("\\b"),
                '\u{c}' => out.push_str("\\f"),
                '\n' => out.push_str("\\n"),
                '\r' => out.push_str("\\r"),
                '\t' => out.push_str("\\t"),
                c if (c as u32) < 0x20 || c as u32 == 0x7f => {
                    out.push_str(&format!("\\u{:04x}", c as u32));
                }
                c => out.push(c),
            }
        }
        out.push('"');
    }
    /// dkjson.lua:278-376 `encode2` (no indent)
    fn encode(&self, out: &mut String) {
        match self {
            Json::Num(x) => {
                // L292-299: NaN / ±inf are "null"; else num2str (tostring)
                if x.is_nan() || x.is_infinite() {
                    out.push_str("null");
                } else {
                    out.push_str(&tostring_num(*x));
                }
            }
            Json::Str(s) => Self::quote(s, out),
            Json::Arr(v) => {
                out.push('[');
                for (i, e) in v.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    e.encode(out);
                }
                out.push(']');
            }
            Json::Obj(m) => {
                out.push('{');
                for (i, (k, v)) in m.iter().enumerate() {
                    if i > 0 {
                        out.push(',');
                    }
                    Self::quote(k, out);
                    out.push(':');
                    v.encode(out);
                }
                out.push('}');
            }
        }
    }
}

/// `dkjson.encode(v)`
fn dkjson_encode(v: &Json) -> String {
    let mut s = String::new();
    v.encode(&mut s);
    s
}

// ---- TradeQueryRequests:FetchLeagues (CCR-0132) ----------------------------------------------

/// `callback(leagues, errMsg)`
type LeaguesCallback = Rc<dyn Fn(&mut Cx, Vec<String>, Option<String>)>;

/// lua: TradeQueryRequests.lua:468-495 `TradeQueryRequestsClass:FetchLeagues(realm, callback)`
/// (U51's file, ported here for its one caller; CCR-0132).
/// @pob Classes/TradeQueryRequests.lua:468-495 h=6d820187
pub fn fetch_leagues(cx: &mut Cx, tqr: &TradeQueryRequests, realm: String, callback: LeaguesCallback) {
    use crate::tabs::import::poe_api::{dkjson_decode, json_get, Json as J};
    // L469
    let header = format!(
        "Authorization: Bearer {}",
        crate::tabs::import::poe_api::auth_token(cx).unwrap_or_default()
    );
    // L470-494
    download_page(
        cx,
        &format!("{}api/trade/data/leagues", tqr.host_name),
        Rc::new(move |cx: &mut Cx, response: PageResponse, err_msg: Option<String>| {
            let fallback = || vec!["Standard".to_owned(), "Hardcore".to_owned()];
            // L473
            if let Some(e) = err_msg {
                // L474
                return callback(cx, fallback(), Some(e));
            }
            // L476
            let Some(body) = response.body.as_deref() else {
                lua_error!("bad argument #1 to 'decode' (string expected, got nil)")
            };
            let json_data = dkjson_decode(body);
            // L477
            let api_error = json_data.as_ref().and_then(|j| json_get(j, "error")).cloned();
            if json_data.is_none() || api_error.as_ref().is_some_and(|e| !matches!(e, J::Bool(false))) {
                // L478-483
                let mut err = match &api_error {
                    Some(J::String(s)) => s.clone(),
                    Some(J::Number(n)) => tostring_num(n.as_f64().unwrap_or(f64::NAN)),
                    _ => "Failed to parse trade leagues JSON".to_owned(),
                };
                // L480
                if let Some(J::Object(_)) = &api_error {
                    let e = api_error.as_ref().unwrap();
                    // L481
                    err = match (json_get(e, "message"), json_get(e, "code")) {
                        (Some(J::String(m)), _) => m.clone(),
                        (Some(J::Bool(true)), _) => lua_error!("attempt to concatenate a boolean value"),
                        (_, Some(J::String(c))) => c.clone(),
                        (_, Some(J::Number(c))) => tostring_num(c.as_f64().unwrap_or(f64::NAN)),
                        (_, Some(J::Bool(true))) => "true".to_owned(),
                        _ => "Failed to parse trade leagues JSON".to_owned(),
                    };
                }
                // L483
                return callback(cx, fallback(), Some(err));
            }
            let json_data = json_data.unwrap();
            // L485
            let mut leagues = Vec::new();
            // L486 ORDER: a JSON array, index order (c)
            let result = json_get(&json_data, "result");
            let entries: Vec<J> = match result {
                Some(J::Array(a)) => a.clone(),
                Some(J::Object(o)) => o.values().cloned().collect(),
                _ => lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"),
            };
            for value in &entries {
                // L487
                if json_get(value, "realm").and_then(J::as_str) == Some(realm.as_str()) {
                    // L488
                    match json_get(value, "id") {
                        Some(J::String(s)) => leagues.push(s.clone()),
                        Some(J::Number(n)) => leagues.push(tostring_num(n.as_f64().unwrap_or(f64::NAN))),
                        _ => {}
                    }
                }
            }
            // L491
            callback(cx, leagues, None);
        }),
        Some(DownloadParams {
            header: Some(header),
            body: None,
        }),
    );
}

// ---- the popup's locals ------------------------------------------------------------------------

/// `local totalMods = { [2] = "Strength", [3] = "Dexterity", [4] = "Devotion" }`
fn total_mods(id: &Val) -> Option<&'static str> {
    match id.opt_num() {
        Some(2.0) => Some("Strength"),
        Some(3.0) => Some("Dexterity"),
        Some(4.0) => Some("Devotion"),
        _ => None,
    }
}

/// `totalModIDs[a] and totalModIDs[a][b]`
fn total_mod_ids(a: &Val, b: &Val) -> bool {
    let (Some(a), Some(b)) = (a.str(), b.str()) else { return false };
    match &*a {
        "total_strength" => matches!(&*b, "karui_notable_add_strength" | "karui_attribute_strength" | "karui_small_strength"),
        "total_dexterity" => matches!(&*b, "maraketh_notable_add_dexterity" | "maraketh_attribute_dex" | "maraketh_small_dex"),
        "total_devotion" => matches!(&*b, "templar_notable_devotion" | "templar_devotion_node" | "templar_small_devotion"),
        _ => false,
    }
}

/// `reverseTotalModIDs[id]`
fn reverse_total_mod_ids(id: &Val) -> bool {
    id.str().is_some_and(|s| {
        matches!(
            &*s,
            "karui_notable_add_strength"
                | "karui_attribute_strength"
                | "karui_small_strength"
                | "maraketh_notable_add_dexterity"
                | "maraketh_attribute_dex"
                | "maraketh_small_dex"
                | "templar_notable_devotion"
                | "templar_devotion_node"
                | "templar_small_devotion"
        )
    })
}

/// `local ignoredMods = { .. }`
const IGNORED_MODS: [&str; 9] = [
    "Might of the Vaal",
    "Legacy of the Vaal",
    "Strength",
    "Add Strength",
    "Dex",
    "Add Dexterity",
    "Devotion",
    "Price of Glory",
    "Ward",
];

/// `{ label = .., id = .. }`
fn entry(label: &str, id: f64) -> Table {
    let t = Table::new();
    t.set("label", label);
    t.set("id", id);
    t
}

/// The socket viewer (`local socketViewer = new("PassiveTreeView"):PassiveTreeView()`), lent to
/// its own `Draw` (ui doc §7.2).
struct Viewer(RefCell<PassiveTreeView>);

impl SocketViewer for Viewer {
    fn set_zoom(&self, _cx: &mut Cx, v: f64) {
        self.0.borrow_mut().zoom = v;
    }
    fn set_zoom_x(&self, _cx: &mut Cx, v: f64) {
        self.0.borrow_mut().zoom_x = Some(v);
    }
    fn set_zoom_y(&self, _cx: &mut Cx, v: f64) {
        self.0.borrow_mut().zoom_y = Some(v);
    }
    fn draw(&self, cx: &mut Cx, vp: &ViewPort) {
        let mut v = std::mem::take(&mut *self.0.borrow_mut());
        v.draw(cx, vp, &mut InputEvents::default());
        *self.0.borrow_mut() = v;
    }
}

/// FindTimelessJewel's locals shared by its local functions and the control callbacks.
struct Up {
    /// `self` (the TreeTab)
    me: CtrlId,
    /// `controls` (the popup)
    p: Cell<CtrlId>,
    /// `treeData`
    tree: PassiveTreeRef,
    /// `treeData.legion`
    legion: Table,
    /// `legionNodes`, `legionAdditions`
    legion_nodes: Table,
    legion_additions: Table,
    /// `timelessData`
    timeless_data: Table,
    /// `modData` (aliased by `nodeSelect`)
    mod_data: Table,
    /// `jewelTypes`
    jewel_types: Table,
    /// `conquerorTypes`
    conqueror_types: Table,
    /// `devotionVariants`
    devotion_variants: Table,
    /// `abyssAscendancyOptions`
    abyss_ascendancy_options: Table,
    /// `jewelSockets`
    jewel_sockets: Table,
    /// `searchListTbl`, `searchListFallbackTbl` (reassigned by parseSearchList)
    search_list_tbl: RefCell<Table>,
    search_list_fallback_tbl: RefCell<Table>,
    /// `allocatedNodes` (written, never read)
    allocated_nodes: RefCell<LjIntMap<bool>>,
    /// `protectedNodes` (reassigned by buildNodeOptionCheckboxes), `protectedNodesCount`
    protected_nodes: RefCell<Table>,
    protected_nodes_count: Cell<f64>,
    /// `nodeSliderStatLabel`, `nodeSlider2StatLabel`
    node_slider_stat_label: RefCell<String>,
    node_slider2_stat_label: RefCell<String>,
    /// `controls.searchTradeButton.lastSearch`
    last_search: RefCell<Option<(f64, f64)>>,
    /// The table `controls.searchTradeLeagueSelect.list` aliases (module notes)
    league_list: RefCell<Option<Table>>,
}

impl Up {
    /// `controls.<key>`
    #[track_caller]
    fn c(&self, cx: &Cx, key: impl Into<CtrlKey> + std::fmt::Debug + Clone) -> CtrlId {
        let k = key.clone();
        cx.child(self.p.get(), key)
            .unwrap_or_else(|| lua_error!("attempt to index field '{k:?}' (a nil value)"))
    }
    /// `controls.<key>` that may be nil
    fn c_opt(&self, cx: &Cx, key: impl Into<CtrlKey>) -> Option<CtrlId> {
        cx.child(self.p.get(), key)
    }
    fn td(&self) -> &Table {
        &self.timeless_data
    }
    /// `timelessData.jewelType`
    fn jt(&self) -> Val {
        self.timeless_data.get("jewelType")
    }
    /// `timelessData.jewelType.id`
    fn jt_id(&self) -> Val {
        self.jt().get("id")
    }
    /// `timelessData.jewelType.id == n`
    fn jt_is(&self, n: f64) -> bool {
        self.jt_id().opt_num() == Some(n)
    }
    /// `timelessData.jewelType.id >= n`
    #[track_caller]
    fn jt_ge(&self, n: f64) -> bool {
        match self.jt_id() {
            Val::Num(x) => x >= n,
            other => lua_error!("attempt to compare {} with number", other.lua_type()),
        }
    }
    /// `timelessData.jewelType.id` as a number operand
    #[track_caller]
    fn jt_num(&self) -> f64 {
        self.jt_id().num()
    }
    /// `timelessData.jewelType.name`
    fn jt_name(&self) -> String {
        cat(&self.jt().get("name"))
    }
    /// `timelessData.jewelSocket.id`
    fn socket_id(&self) -> Val {
        self.timeless_data.get("jewelSocket").get("id")
    }
    /// `self.build.modFlag = true`
    fn mod_flag(cx: &mut Cx) {
        cx.build_mut().mod_flag = true;
    }
    /// `controls.socketFilter.state`
    fn socket_filter_state(&self, cx: &Cx) -> bool {
        cx.w::<CheckBoxControl>(self.c(cx, "socketFilter")).state
    }

    /// The raw `label` of a LabelControl.
    fn label(cx: &Cx, id: CtrlId) -> String {
        cx.w::<LabelControl>(id).label.raw()
    }
    fn set_label(cx: &mut Cx, id: CtrlId, s: impl Into<String>) {
        cx.w_mut::<LabelControl>(id).label = Prop::Val(s.into());
    }
    fn set_shown(cx: &mut Cx, id: CtrlId, v: bool) {
        cx.base_mut(id).shown = Prop::Val(v);
    }

    /// `controls.nodeSelect.list[controls.nodeSelect.selIndex].id`
    fn node_select_id(&self, cx: &Cx) -> Val {
        let ns = self.c(cx, "nodeSelect");
        let w = cx.w::<DropDownControl>(ns);
        match w.sel_index.checked_sub(1).and_then(|i| w.list.get(i)) {
            Some(item) => item_table(Some(item)).map_or(Val::Nil, |t| t.get("id")),
            None => lua_error!("attempt to index field '?' (a nil value)"),
        }
    }

    /// The aliased `nodeSelect.list = modData` after a refill (module notes).
    fn sync_node_select(&self, cx: &mut Cx) {
        if let Some(ns) = self.c_opt(cx, "nodeSelect") {
            cx.w_mut::<DropDownControl>(ns).list = table_items(&self.mod_data);
        }
    }

    /// lua: TreeTab.lua:1359-1412 `local function buildMods()`
    /// @pob Classes/TreeTab.lua:1359-1412 h=61d38e3b
    fn buildMods(&self, cx: &mut Cx) {
        let name = self.jt_name();
        // L1360
        self.mod_data.wipe();
        // L1361
        let smallModData = Table::new();
        // L1362 ORDER: a sequence (c)
        for (_, v) in self.legion_nodes.pairs() {
            let node = Lp::of(&v);
            let id = node.id_str();
            let dn = cat(&node.dn());
            // L1363-1365
            if lmatch(&id, &format!("^{name}_.+"))
                && !lmatch(&id, "^abyss_special_ascendancy_notable_")
                && !in_array(&IGNORED_MODS, &dn)
                && !node.get("ks").truthy()
            {
                let sd = node.sd();
                // L1366
                if node.get("not").truthy() {
                    // L1367-1372
                    let sd1 = sd.first().map_or_else(
                        || lua_error!("attempt to concatenate field '?' (a nil value)"),
                        |s| s.to_string(),
                    );
                    let t = Table::new();
                    t.set("label", format!("{dn}{}{sd1}", " ".repeat(48)));
                    t.set("descriptions", str_table(&sd));
                    t.set("type", name.as_str());
                    t.set("id", id.as_str());
                    self.mod_data.push(t.clone());
                    // L1373
                    if let Some(sd2) = sd.get(1) {
                        // L1374
                        let l = cat(&t.get("label"));
                        t.set("label", format!("{l} {sd2}"));
                    }
                } else {
                    // L1377-1382
                    let t = Table::new();
                    t.set("label", dn.as_str());
                    t.set("descriptions", str_table(&sd));
                    t.set("type", name.as_str());
                    t.set("id", id.as_str());
                    smallModData.push(t);
                }
            }
        }
        // L1386 ORDER: a sequence (c)
        for (_, v) in self.legion_additions.pairs() {
            let addition = Lp::of(&v);
            let id = addition.id_str();
            let dn = cat(&addition.dn());
            // exclude passives that are already added (vaal, attributes, devotion)
            // L1388
            if lmatch(&id, &format!("^{name}_.+")) && !in_array(&IGNORED_MODS, &dn) && name != "vaal" {
                // L1389-1394
                let t = Table::new();
                t.set("label", dn.as_str());
                t.set("descriptions", str_table(&addition.sd()));
                t.set("type", name.as_str());
                t.set("id", id.as_str());
                self.mod_data.push(t);
            }
        }
        // L1397
        sort_by_label(&self.mod_data);
        // L1398
        sort_by_label(&smallModData);
        // L1399
        if let Some(total) = total_mods(&self.jt_id()) {
            // L1400-1406
            let t = Table::new();
            t.set("label", format!("Total {total}"));
            t.set(
                "descriptions",
                str_table(&[model::LStr::from(
                    format!("This is a hybrid node containing all additions to {total}").as_str(),
                )]),
            );
            t.set("type", name.as_str());
            t.set("id", format!("total_{}", pob_lua::string::lower(total)));
            t.set("totalMod", true);
            self.mod_data.insert(1, t);
        }
        // L1408
        let dots = Table::new();
        dots.set("label", "...");
        self.mod_data.insert(1, dots);
        // L1409
        for i in 1..=smallModData.len() {
            // L1410
            let n = self.mod_data.len() as i64;
            self.mod_data.set(n + 1, smallModData.get(i as i64));
        }
        self.sync_node_select(cx);
    }

    /// lua: TreeTab.lua:1414-1426 `local function getNodeWeights()`
    /// @pob Classes/TreeTab.lua:1414-1426 h=b5bc1a36
    fn getNodeWeights(&self, cx: &Cx) -> [Val; 3] {
        let w = |key: &'static str| -> Val {
            let l = Self::label(cx, self.c(cx, key));
            Val::from(pob_lua::string::lower(&pob_lua::string::sub(&l, 3, None)))
        };
        // L1415-1419
        let mut nodeWeights = [w("nodeSliderValue"), w("nodeSlider2Value"), w("nodeSlider3Value")];
        // L1420
        for nodeWeight in &mut nodeWeights {
            // L1421
            if let Some(n) = tonumber(nodeWeight) {
                // L1422
                *nodeWeight = Val::Num(round(n, Some(3)));
            }
        }
        // L1425
        nodeWeights
    }

    /// The `gmatch` split of L1436-1441 / L1447-1452.
    fn split_rows(text: &str) -> Table {
        let tbl = Table::new();
        for inputLine in pob_lua::gmatch(text, "[^\r\n]+") {
            let row = Table::new();
            for splitLine in pob_lua::gmatch(&inputLine[0].to_str(), "([^,%s]+)") {
                row.push(&*splitLine[0].to_str());
            }
            tbl.push(row);
        }
        tbl
    }

    /// lua: TreeTab.lua:1430-1502 `local function parseSearchList(mode, fallback)`
    /// @pob Classes/TreeTab.lua:1428-1515 h=d0265f19
    /// (with the two locals before it, the initial loads L1503-1504 (in `find_timeless_jewel`)
    /// and `updateSearchList` (L1505-1515, [`Up::updateSearchList`]))
    fn parseSearchList(&self, cx: &mut Cx, mode: u8, fallback: bool) {
        // L1431
        if mode == 0 {
            // L1432
            if fallback {
                // timelessData.searchListFallback => searchListFallbackTbl
                // L1434
                if let Some(text) = truthy_str(&self.td().get("searchListFallback")) {
                    // L1435-1441
                    *self.search_list_fallback_tbl.borrow_mut() = Self::split_rows(&text);
                }
            } else {
                // timelessData.searchList => searchListTbl
                // L1445
                if let Some(text) = truthy_str(&self.td().get("searchList")) {
                    // L1446-1452
                    *self.search_list_tbl.borrow_mut() = Self::split_rows(&text);
                }
            }
        } else {
            let (edit_key, tbl, td_key) = if fallback {
                // searchListFallbackTbl => controls.searchListFallback
                ("searchListFallback", self.search_list_fallback_tbl.borrow().clone(), "searchListFallback")
            } else {
                // searchListTbl => controls.searchList
                ("searchList", self.search_list_tbl.borrow().clone(), "searchList")
            };
            // L1458 / L1480
            let (Some(edit), Some(_)) = (self.c_opt(cx, edit_key), self.c_opt(cx, "nodeSelect")) else {
                return;
            };
            // L1459 / L1481
            let mut searchText = String::new();
            // L1460 / L1482
            for (_, curRow) in tbl.ipairs() {
                let curRow = curRow.table().unwrap_or_else(|| lua_error!("attempt to index local 'curRow' (a nil value)"));
                // L1461 / L1483
                if curRow.get(1i64).raw_eq(&self.node_select_id(cx)) {
                    // L1462 / L1484
                    let nodeWeights = self.getNodeWeights(cx);
                    let [a, b, c] = nodeWeights;
                    // L1463-1465 / L1485-1487
                    curRow.set(2i64, a);
                    curRow.set(3i64, b);
                    curRow.set(4i64, c);
                }
                // L1467 / L1489
                if !searchText.is_empty() {
                    // L1468 / L1490
                    searchText.push('\n');
                }
                // L1470 / L1492
                searchText.push_str(&t_concat(&curRow, ", "));
            }
            // L1472 / L1494
            if !self.td().get(td_key).raw_eq(&Val::from(searchText.as_str())) {
                // L1473 / L1495
                self.td().set(td_key, searchText.as_str());
                // L1474 / L1496
                EditControl::set_text(cx, edit, &searchText, false);
                // L1475 / L1497
                Self::mod_flag(cx);
            }
        }
    }

    /// lua: TreeTab.lua:1505-1515 `local function updateSearchList(text, fallback)`
    fn updateSearchList(&self, cx: &mut Cx, text: &str, fallback: bool) {
        // L1506
        if fallback {
            // L1507
            self.td().set("searchListFallback", text);
            // L1508
            let e = self.c(cx, "searchListFallback");
            EditControl::set_text(cx, e, text, false);
        } else {
            // L1510
            self.td().set("searchList", text);
            // L1511
            let e = self.c(cx, "searchList");
            EditControl::set_text(cx, e, text, false);
        }
        // L1513
        self.parseSearchList(cx, 0, fallback);
        // L1514
        Self::mod_flag(cx);
    }

    /// lua: TreeTab.lua:1538-1564 `local function buildNodeOptionCheckboxes(nodes)`
    /// @pob Classes/TreeTab.lua:1538-1564 h=d2607328
    fn buildNodeOptionCheckboxes(self: &Rc<Self>, cx: &mut Cx, nodes: Option<Vec<(String, NodeRef)>>) {
        // L1539
        let mut i = 1;
        // L1540
        *self.protected_nodes.borrow_mut() = Table::new();
        // L1541
        self.protected_nodes_count.set(0.0);
        // clear old boxes
        // L1543
        while self.c_opt(cx, format!("protectAllocatedBox{i}")).is_some() {
            // L1544
            cx.remove_child(self.p.get(), &CtrlKey::from(format!("protectAllocatedBox{i}")));
            // L1545
            i += 1;
        }
        // L1547
        let Some(nodes) = nodes else {
            // L1548
            return;
        };
        // L1550
        for (i, (label, node)) in nodes.into_iter().enumerate() {
            let i = i + 1;
            // L1551
            let prevBox = self.c_opt(cx, format!("protectAllocatedBox{}", i - 1));
            // L1552
            let anchor = prevBox.unwrap_or_else(|| self.c(cx, "protectAllocatedLabel"));
            // L1553-1556
            let up = self.clone();
            let l2 = label.clone();
            let box_ = cx
                .add(
                    self.p.get(),
                    format!("protectAllocatedBox{i}"),
                    CheckBoxControl::new(
                        Anchor::new(TopLeft, anchor, BottomLeft),
                        rect(0, 4, 18, 18),
                        Some(label.clone()),
                        Some(Rc::new(move |_cx: &mut Cx, state: bool| {
                            // L1554
                            up.protected_nodes.borrow().set(l2.as_str(), state);
                            // L1555
                            up.protected_nodes_count
                                .set(up.protected_nodes_count.get() + if state { 1.0 } else { -1.0 });
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L1557
            cx.w_mut::<CheckBoxControl>(box_).label_right = true;
            // L1558-1561
            let me = self.me;
            cx.w_mut::<CheckBoxControl>(box_).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, _| {
                // L1559
                tooltip.clear(false);
                // L1560
                viewer_add_node_tooltip(cx, me, tooltip, &node);
            }));
            // L1562 (the `cx.add` above)
        }
    }

    /// lua: TreeTab.lua:1603-1635 `setAllocatedNodes = function()`
    /// @pob Classes/TreeTab.lua:1603-1635 h=9c1432ac
    fn setAllocatedNodes(self: &Rc<Self>, cx: &mut Cx) {
        // L1604
        self.allocated_nodes.borrow_mut().wipe();
        // L1605
        tt_state(cx, self.me).allocated_nodes_in_radius_count = 0.0;
        // L1606
        let sid = self.socket_id();
        let socket_node = int_key(&sid).and_then(|k| self.tree.nodes.get(k));
        if sid.raw_eq(&Val::Num(-1.0)) || socket_node.is_none() {
            // L1607
            self.buildNodeOptionCheckboxes(cx, None);
            // L1608
            return;
        }
        let m = cx.build().model.clone();
        let spec = m.spec.get();
        // L1610
        let mut nodeOptions: Vec<(String, NodeRef)> = Vec::new();
        // L1611
        if self.jt_is(11.0) {
            // Reclaimed Malevolence can replace an allocated notable in the selected ascendancy.
            // L1613 ORDER: module notes (c)
            for nodeId in spec.allocNodes.with(LjIntMap::keys) {
                // L1614
                let baseNode = self.tree.nodes.get(nodeId);
                // L1615
                if let Some(baseNode) = baseNode {
                    if baseNode.ascendancyName.get() == spec.curAscendClassName.get()
                        && baseNode.r#type.get() == model::Name::from("Notable")
                    {
                        // L1616
                        self.allocated_nodes.borrow_mut().set(nodeId, Some(true));
                        // L1617
                        nodeOptions.push((baseNode.dn.get().to_string(), baseNode));
                    }
                }
            }
        } else {
            // L1621
            let radiusNodes = radius_nodes(&socket_node.unwrap());
            // L1622 ORDER: module notes (c)
            for nodeId in radiusNodes.keys() {
                // L1623
                if granted(&m, nodeId) || spec.allocNodes.with(|a| a.contains_key(nodeId)) {
                    // L1624
                    self.allocated_nodes.borrow_mut().set(nodeId, Some(true));
                    // L1625
                    if let Some(baseNode) = self.tree.nodes.get(nodeId) {
                        if baseNode.isNotable.get() {
                            // L1626-1627
                            nodeOptions.push((baseNode.dn.get().to_string(), baseNode));
                        }
                    }
                }
            }
        }
        // L1632
        pob_lua::sort(&mut nodeOptions, |a, b| a.0.as_bytes() < b.0.as_bytes());
        let n = nodeOptions.len();
        // L1633
        self.buildNodeOptionCheckboxes(cx, Some(nodeOptions));
        // L1634
        tt_state(cx, self.me).allocated_nodes_in_radius_count = n as f64;
    }

    /// lua: TreeTab.lua:1851-1875 `local function updateSliders(sliderData)`
    /// @pob Classes/TreeTab.lua:1851-1875 h=3f45e4ac
    fn updateSliders(&self, cx: &mut Cx, sliderData: &Table) {
        let s1 = self.c(cx, "nodeSlider");
        let s2 = self.c(cx, "nodeSlider2");
        let s3 = self.c(cx, "nodeSlider3");
        let required = |v: Val| v.raw_eq(&Val::from("required"));
        // L1852
        if required(sliderData.get(2i64)) {
            // L1853
            cx.w_mut::<SliderControl>(s1).val = 1.0;
            // L1854
            Self::set_label(cx, self.c(cx, "nodeSliderValue"), lformat!("^7%.3f", 10.0));
        } else {
            // L1856
            cx.w_mut::<SliderControl>(s1).val =
                m_min(m_max(tonumber(&sliderData.get(2i64)).unwrap_or(0.0) / 10.0, 0.0), 1.0);
            // L1857
            let v = cx.w::<SliderControl>(s1).val;
            Self::set_label(cx, self.c(cx, "nodeSliderValue"), lformat!("^7%.3f", v * 10.0));
        }
        // L1859
        if cx.base(s2).enabled.raw() {
            // L1860
            if required(sliderData.get(3i64)) {
                // L1861
                cx.w_mut::<SliderControl>(s2).val = 1.0;
                // L1862
                Self::set_label(cx, self.c(cx, "nodeSlider2Value"), lformat!("^7%.3f", 10.0));
            } else {
                // L1864
                cx.w_mut::<SliderControl>(s2).val =
                    m_min(m_max(tonumber(&sliderData.get(3i64)).unwrap_or(0.0) / 10.0, 0.0), 1.0);
                // L1865
                let v = cx.w::<SliderControl>(s2).val;
                Self::set_label(cx, self.c(cx, "nodeSlider2Value"), lformat!("^7%.3f", v * 10.0));
            }
        }
        // L1868
        if required(sliderData.get(4i64)) {
            // L1869
            cx.w_mut::<SliderControl>(s3).val = 1.0;
            // L1870
            Self::set_label(cx, self.c(cx, "nodeSlider3Value"), "^7Required");
        } else {
            // L1872
            cx.w_mut::<SliderControl>(s3).val =
                m_min(m_max(tonumber(&sliderData.get(4i64)).unwrap_or(0.0) / 500.0, 0.0), 1.0);
            // L1873
            let v = cx.w::<SliderControl>(s3).val;
            Self::set_label(cx, self.c(cx, "nodeSlider3Value"), lformat!("^7%.f", v * 500.0));
        }
    }

    /// lua: TreeTab.lua:1878-1886 `local function getLegionStatLabels(legionPassive)`
    fn getLegionStatLabels(&self, legionPassive: &Lp) -> (f64, String, String) {
        let sd = legionPassive.sd();
        // L1879
        let statCount = if self.jt_ge(7.0) { legionPassive.sorted_stats_len() } else { sd.len() } as f64;
        // L1880
        if statCount > sd.len() as f64 {
            let sd1 = sd
                .first()
                .map_or_else(|| lua_error!("attempt to concatenate field '?' (a nil value)"), |s| s.to_string());
            // L1881
            return (statCount, format!("Minimum value: {sd1}"), format!("Maximum value: {sd1}"));
        }
        // L1883-1885
        (
            statCount,
            if statCount == 1.0 {
                concat_strs(&sd, " + ")
            } else {
                sd.first().map_or_else(|| "None".to_owned(), |s| s.to_string())
            },
            sd.get(1).map_or_else(|| "None".to_owned(), |s| s.to_string()),
        )
    }

    /// lua: TreeTab.lua:1887-1941, the `nodeSelect` dropdown's selFunc
    fn nodeSelectChanged(&self, cx: &mut Cx, value: Option<Table>) {
        let Some(value) = value else { lua_error!("attempt to index local 'value' (a nil value)") };
        // L1888
        *self.node_slider_stat_label.borrow_mut() = "None".to_owned();
        // L1889
        *self.node_slider2_stat_label.borrow_mut() = "None".to_owned();
        let id = value.get("id");
        // L1890
        if !id.truthy() {
            return;
        }
        // L1891
        let mut statCount = 0.0;
        // L1892
        for (_, v) in self.legion_nodes.ipairs() {
            let legionNode = Lp::of(&v);
            // L1893
            if legionNode.id().raw_eq(&id) {
                // L1894
                let (c, a, b) = self.getLegionStatLabels(&legionNode);
                statCount = c;
                *self.node_slider_stat_label.borrow_mut() = a;
                *self.node_slider2_stat_label.borrow_mut() = b;
                // L1895
                break;
            }
        }
        // L1898
        if statCount == 0.0 {
            // L1899
            for (_, v) in self.legion_additions.ipairs() {
                let legionAddition = Lp::of(&v);
                // L1900
                if legionAddition.id().raw_eq(&id) {
                    // L1901
                    let (c, a, b) = self.getLegionStatLabels(&legionAddition);
                    statCount = c;
                    *self.node_slider_stat_label.borrow_mut() = a;
                    *self.node_slider2_stat_label.borrow_mut() = b;
                    // L1902
                    break;
                }
            }
        }
        let s2 = self.c(cx, "nodeSlider2");
        // L1906
        if statCount <= 1.0 {
            // L1907
            Self::set_label(cx, self.c(cx, "nodeSlider2Label"), "^9Secondary Node Weight:");
            // L1908
            cx.w_mut::<SliderControl>(s2).val = 0.0;
            // L1909
            Self::set_label(cx, self.c(cx, "nodeSlider2Value"), lformat!("^9%.3f", 0.0));
        } else {
            // L1911
            Self::set_label(cx, self.c(cx, "nodeSlider2Label"), "^7Secondary Node Weight:");
            // L1912
            let v = cx.w::<SliderControl>(s2).val;
            Self::set_label(cx, self.c(cx, "nodeSlider2Value"), lformat!("^7%.3f", v * 10.0));
        }
        // L1914
        cx.base_mut(s2).enabled = Prop::Val(statCount > 1.0);

        // L1916
        let nodeWeights = self.getNodeWeights(cx);
        // L1917
        let newNode = format!(
            "{}, {}, {}, {}",
            cat(&id),
            cat(&nodeWeights[0]),
            cat(&nodeWeights[1]),
            cat(&nodeWeights[2])
        );
        // L1918
        let fallback = self
            .c_opt(cx, "searchListFallback")
            .filter(|&f| cx.base(f).shown.raw());
        let (edit, tbl) = if let Some(f) = fallback {
            (f, self.search_list_fallback_tbl.borrow().clone())
        } else {
            (self.c(cx, "searchList"), self.search_list_tbl.borrow().clone())
        };
        // L1919 / L1929
        for (_, searchRow) in tbl.ipairs() {
            let searchRow = searchRow.table().unwrap_or_else(|| lua_error!("attempt to index local 'searchRow' (a nil value)"));
            // update nodeSlider values and prevent duplicate searchList entries
            // L1921 / L1931
            if searchRow.get(1i64).raw_eq(&id) {
                // L1922 / L1932
                self.updateSliders(cx, &searchRow);
                // L1923 / L1933
                return;
            }
        }
        // L1926 / L1936
        let len = cx.w::<EditControl>(edit).buf.len();
        cx.w_mut::<EditControl>(edit).caret = len + 1;
        // L1927 / L1937
        let text = format!("{}{newNode}", if len > 0 { "\n" } else { "" });
        EditControl::insert(cx, edit, &text);
        // L1939
        Self::mod_flag(cx);
    }
}

/// `x` when it is a string (`if timelessData.searchList then .. :gmatch`); a non-string truthy
/// value raises at the method call.
fn truthy_str(v: &Val) -> Option<String> {
    match v {
        Val::Nil | Val::Bool(false) => None,
        Val::Str(s) => Some(s.to_string()),
        Val::Num(x) => Some(tostring_num(*x)),
        other => lua_error!("attempt to index field 'searchList' (a {} value)", other.lua_type()),
    }
}

/// `treeData.nodes[socketId].nodesInRadius[3]` (nil indexes raise).
fn radius_nodes(node: &Node) -> Rc<LjIntMap<NodeRef>> {
    pob_calc::classes::passive_tree::nodesInRadiusAt(node, 3.0)
        .unwrap_or_else(|| lua_error!("attempt to index field 'nodesInRadius' (a nil value)"))
}

/// `self.build.calcsTab.mainEnv.grantedPassives[nodeId] ~= nil`
fn granted(m: &model::BuildRef, nodeId: i64) -> bool {
    let Some(env) = m.calcsTab.mainEnv.get() else {
        lua_error!("attempt to index field 'mainEnv' (a nil value)")
    };
    env.grantedPassives.get().with(|g| g.contains_key(nodeId))
}

/// `self.viewer:AddNodeTooltip(tooltip, node, self.build, true)` (DIVERGENCE(viewer-tooltip))
fn viewer_add_node_tooltip(cx: &mut Cx, me: CtrlId, tooltip: &mut Tooltip, node: &NodeRef) {
    let f = timeless_hooks().add_node_tooltip;
    cx.lend::<TreeTab, _, _>(me, |w| &mut w.core.viewer, |cx, v| f(cx, v, tooltip, node));
}

/// The `tooltip.Draw` override of the four sliders (L1748-1756, L1784-1792, L1813-1821,
/// L1840-1848): above the knob when the screen is wide enough.
fn slider_tooltip_draw(cx: &mut Cx, slider: CtrlId, min_screen_w: f64) {
    let f: pob_ui_core::tooltip::TooltipDrawFn = Rc::new(move |cx, tt, x, y, width, height, vp| {
        // L1750 / L1786 / L1815 / L1842
        let sliderOffsetX = round(184.0 * (1.0 - cx.w::<SliderControl>(slider).val), None);
        // L1751 / L1787 / L1816 / L1843
        let (_tooltipWidth, tooltipHeight) = tt.get_size(cx);
        // L1752 / L1788 / L1817 / L1844
        if cx.main.screen_w >= min_screen_w - sliderOffsetX {
            // L1753 / L1789 / L1818 / L1845
            return tt.real_draw(cx, x - 8.0 - sliderOffsetX, y - 4.0 - tooltipHeight, width, height, vp);
        }
        // L1755 / L1791 / L1820 / L1847
        tt.real_draw(cx, x, y, width, height, vp)
    });
    // L1748 / L1784 / L1813 / L1840 (`tooltip.realDraw = tooltip.Draw`: Tooltip::real_draw)
    cx.w_mut::<SliderControl>(slider).tt.tooltip.draw_override = Some(f);
}

impl Up {
    /// lua: TreeTab.lua:1952-1977 `local function generateFallbackWeights(nodes, powerStat)`
    /// @pob Classes/TreeTab.lua:1952-1977 h=2c7e312d
    fn generateFallbackWeights(&self, cx: &mut Cx, nodes: &[NewNode], powerStat: &Table) -> Option<Vec<PowerEntry>> {
        let m = cx.build().model.clone();
        // L1953 (Poll::Pending: web only, the calc is in a worker; the click does nothing)
        let std::task::Poll::Ready((calcFunc, calcBase)) = cx.build_mut().calc.misc_calculator(&m) else {
            return None;
        };
        // L1954
        let mut newList = Vec::new();
        // L1955
        let basePower = GetFromOutput(&calcBase, powerStat, false);
        // L1956
        for newNode in nodes {
            // L1957
            let mut powerEntry = PowerEntry {
                id: newNode.id.clone(),
                weight1: None,
                weight2: None,
            };
            // nodes that have multiple lines are represented as a list in newNode.node
            // L1959
            let nodeLines: Vec<(Option<ModList>, Option<f64>)> = match &newNode.node {
                Some(v) => v.iter().map(|s| (s.mod_list.clone(), s.divisor)).collect(),
                None => vec![(newNode.mod_list.clone(), newNode.divisor)],
            };
            // L1960
            for (i0, (mod_list, divisor)) in nodeLines.iter().enumerate() {
                // L1961 local node = nodeLines[i]
                // L1962 calcFunc({ addNodes = { [node] = true } })
                // a nil `modList` adds nothing (CalcSetup.lua:141 `modList:AddList(node.modList)`,
                // ModList.lua:88-94 `if modList then`): an empty list
                let mod_list = mod_list.clone().unwrap_or_else(|| ModStore::ModList(None));
                let node: NodeRef = Rc::new(Node {
                    id: f64::NAN,
                    modList: model::Field::new(mod_list),
                    ..Node::default()
                });
                let mut set = LjIntMap::new();
                set.set(node.id as i64, Some(node));
                let ov = Override {
                    addNodes: model::Field::new(Some(Shared::new(set))),
                    ..Override::default()
                };
                let std::task::Poll::Ready(nodeOutput) = calcFunc.call(Some(Rc::new(ov)), None) else {
                    return None;
                };
                // L1963
                let nodePower = GetFromOutput(&nodeOutput, powerStat, false);
                // avoid infinity
                // L1965
                let w = if basePower == 0.0 {
                    // L1966
                    0.0
                } else {
                    // L1968-1970
                    let powerGain = (nodePower - basePower) /
                        // normalize with absolute base power so that the result isn't negative
                        basePower.abs();
                    // L1971
                    powerGain / divisor.or(newNode.divisor).unwrap_or(1.0)
                };
                match i0 {
                    0 => powerEntry.weight1 = Some(w),
                    1 => powerEntry.weight2 = Some(w),
                    // `powerEntry["weight" .. i]` beyond 2 is never read (L2108-2113)
                    _ => {}
                }
            }
            // L1974
            newList.push(powerEntry);
        }
        // L1976
        Some(newList)
    }
}

/// `replaceHelperFunc(statToFix, statKey, statMod, value)` (L1981-1997): the first value of
/// `gsub` (every caller keeps only that).
fn replaceHelperFunc(statToFix: &str, statKey: &Val, statMod: &Val, value: f64) -> String {
    let mut value = value;
    // L1982 note the only one we actually care about is "Ritual of Flesh" life regen
    if statMod.get("fmt").raw_eq(&Val::from("g")) {
        let key = match statKey {
            Val::Str(s) => s.to_string(),
            other => lua_error!("attempt to index local 'statKey' (a {} value)", other.lua_type()),
        };
        // L1983
        if pob_lua::find(&key, "per_minute", None, false).is_some() {
            // L1984
            value = round(value / 60.0, Some(1));
        } else if pob_lua::find(&key, "permyriad", None, false).is_some() {
            // L1986
            value = value / 100.0;
        } else if pob_lua::find(&key, "_ms", None, false).is_some() {
            // L1988
            value = value / 1000.0;
        }
    }
    //if statMod.fmt == "d" then -- only ever d or g, and we want both past here
    let min = statMod.get("min");
    let max = statMod.get("max");
    let repl = tostring_num(value);
    // L1992
    if !min.raw_eq(&max) {
        // L1993
        let pat = format!("%({}%-{}%)", cat(&min), cat(&max));
        return pob_lua::gsub(statToFix, &pat, &repl, None).0;
    } else if !min.raw_eq(&Val::Num(value)) {
        // only true for might/legacy of the vaal which can combine stats
        // L1995
        return pob_lua::gsub(statToFix, &cat(&min), &repl, None).0;
    }
    // if it doesn't need to be changed
    // L1997
    statToFix.to_owned()
}

impl Up {
    /// `local function buildStatModLists(legionPassive)` (L1999-2015)
    fn buildStatModLists(cx: &mut Cx, legionPassive: &Lp) -> Table {
        // Give each stat its own mod list even when several stats share one display line.
        // L2001
        let modLists = Table::new();
        let stats = legionPassive.get("stats");
        // L2002
        for (statIndex, statKey) in val_ipairs(&legionPassive.get("sortedStats")).into_iter().enumerate() {
            // L2003
            let statValues = Table::new();
            // L2004 ORDER: set (c)
            for (key, _) in val_pairs(&stats) {
                // L2005
                let v = if key.raw_eq(&statKey) { 100.0 } else { 0.0 };
                match &key {
                    Val::Str(s) => statValues.set(model::Name::from(&**s), v),
                    other => statValues.set(other.opt_num().unwrap_or(f64::NAN), v),
                }
            }
            // L2007
            let (lines, _) = stat_describer::describe_stats(cx, &statValues, "stat_descriptions", false);
            let Some(line) = lines.first().cloned() else {
                lua_error!("bad argument #1 to 'parseMod' (string expected, got nil)")
            };
            // L2008
            let e = Table::new();
            if let Some(ml) = parse_mod(&line) {
                e.set("modList", Val::Obj(Rc::new(ModListVal(ml))));
            }
            e.set("divisor", 100.0);
            modLists.set(statIndex as i64 + 1, e);
        }
        // L2010
        modLists
    }

    /// The `newNode.node` list from a `modListGenerated` / `modLists` table (`copyTable`).
    fn sub_nodes(t: &Table, id: &Val) -> Vec<SubNode> {
        t.ipairs()
            .into_iter()
            .map(|(_, e)| {
                let e = e.table().unwrap_or_else(|| lua_error!("attempt to index local 'node' (a nil value)"));
                SubNode {
                    mod_list: e.obj::<ModListVal>("modList").map(|m| m.0.clone()),
                    divisor: e.get("divisor").opt_num(),
                    // L2032 / L2073 node.id = legionNode.id
                    _id: id.clone(),
                }
            })
            .collect()
    }

    /// `x.modListGenerated` as a mod list.
    fn generated_mod_list(lp: &Lp) -> Option<Option<ModList>> {
        let v = lp.get("modListGenerated");
        if !v.truthy() {
            return None;
        }
        Some(v.obj::<ModListVal>().map(|m| m.0.clone()))
    }

    /// lua: TreeTab.lua:1979-2118 `local function setupFallbackWeights()`
    /// @pob Classes/TreeTab.lua:1979-2118 h=a241d573
    fn setupFallbackWeights(&self, cx: &mut Cx) {
        // L1981-1998 replaceHelperFunc (is duplicated from PassiveSpec.lua): [`replaceHelperFunc`]
        // L1999-2015 buildStatModLists: [`Up::buildStatModLists`]

        // L2017
        let mut nodes: Vec<NewNode> = Vec::new();
        // L2018
        let usesVariableRolls = self.jt_is(1.0) || self.jt_ge(7.0);
        // L2019
        for (_, modNode) in self.mod_data.ipairs() {
            let modNode = modNode.table().unwrap_or_else(|| lua_error!("attempt to index local 'modNode' (a nil value)"));
            let mod_id = modNode.get("id");
            // L2020
            if !mod_id.truthy() {
                continue;
            }
            // L2021
            let mut newNode: Option<NewNode> = None;
            // L2022
            for (_, v) in self.legion_nodes.ipairs() {
                let legionNode = Lp::of(&v);
                // L2023
                if legionNode.id().raw_eq(&mod_id) || total_mod_ids(&mod_id, &legionNode.id()) {
                    // L2024-2025
                    let mut nn = NewNode {
                        id: mod_id.clone(),
                        _calc_multiple: false,
                        node: None,
                        mod_list: None,
                        divisor: None,
                    };
                    // L2026
                    if usesVariableRolls {
                        // L2027
                        if legionNode.sorted_stats_len() > 1 {
                            // L2028
                            nn._calc_multiple = true;
                            // L2029
                            let gen = legionNode.get("modListGenerated");
                            if let Val::Table(g) = &gen {
                                // L2030
                                nn.node = Some(Self::sub_nodes(g, &legionNode.id()));
                            } else {
                                // L2032
                                let modLists = Self::buildStatModLists(cx, &legionNode);
                                // L2033
                                legionNode.set("modListGenerated", Val::Table(modLists.copyTable(false)));
                                // L2034
                                nn.node = Some(Self::sub_nodes(&modLists, &legionNode.id()));
                            }
                            // L2036-2038 (`node.id = legionNode.id` in sub_nodes)
                        } else {
                            // L2040
                            let sd = legionNode.sd();
                            let originalLine = sd
                                .first()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| lua_error!("bad argument #1 to 'gsub' (string expected, got nil)"));
                            // L2041
                            let sortedStats = legionNode.get("sortedStats");
                            let key1 = sortedStats.get(1i64);
                            let statMod = legionNode.get("stats").get(lua_key(&key1));
                            let line = replaceHelperFunc(&originalLine, &key1, &statMod, 100.0);
                            // L2042
                            if line == originalLine && sd.len() > 1 {
                                // Some fixed game stats represent several display lines; score the complete effect together.
                                // L2044
                                nn.mod_list = legionNode.mod_list();
                            } else if let Some(g) = Self::generated_mod_list(&legionNode) {
                                // L2046
                                nn.mod_list = g;
                            } else {
                                // L2048
                                let modList = parse_mod(&line);
                                // L2049
                                legionNode.set(
                                    "modListGenerated",
                                    modList.clone().map_or(Val::Nil, |m| Val::Obj(Rc::new(ModListVal(m)))),
                                );
                                // L2050
                                nn.mod_list = modList;
                            }
                            // L2052
                            nn.divisor = Some(if line != originalLine { 100.0 } else { 1.0 });
                        }
                    } else {
                        // L2055
                        nn.mod_list = legionNode.mod_list();
                        // L2056
                        if modNode.truthy("totalMod") {
                            // L2057
                            nn.divisor = Some(first_mod_value(nn.mod_list.as_ref()));
                        }
                    }
                    newNode = Some(nn);
                    // L2060
                    break;
                }
            }
            // L2063
            if newNode.is_none() {
                // L2064
                for (_, v) in self.legion_additions.ipairs() {
                    let legionAddition = Lp::of(&v);
                    // L2065
                    if legionAddition.id().raw_eq(&mod_id) || total_mod_ids(&mod_id, &legionAddition.id()) {
                        // L2066-2067
                        let mut nn = NewNode {
                            id: mod_id.clone(),
                            _calc_multiple: false,
                            node: None,
                            mod_list: None,
                            divisor: None,
                        };
                        // L2068
                        if usesVariableRolls && legionAddition.sorted_stats_len() > 1 {
                            // L2069
                            nn._calc_multiple = true;
                            // L2070
                            let gen = legionAddition.get("modListGenerated");
                            if let Val::Table(g) = &gen {
                                // L2071
                                nn.node = Some(Self::sub_nodes(g, &legionAddition.id()));
                            } else {
                                // L2073
                                let modLists = Self::buildStatModLists(cx, &legionAddition);
                                // L2074
                                legionAddition.set("modListGenerated", Val::Table(modLists.copyTable(false)));
                                // L2075
                                nn.node = Some(Self::sub_nodes(&modLists, &legionAddition.id()));
                            }
                            // L2077-2079 (`node.id = legionAddition.id` in sub_nodes)
                        } else if let Some(ml) = legionAddition.mod_list() {
                            // L2081
                            nn.mod_list = Some(ml);
                        } else if let Some(g) = Self::generated_mod_list(&legionAddition) {
                            // L2083
                            nn.mod_list = g;
                        } else {
                            // generate modList
                            // L2086
                            let sd = legionAddition.sd();
                            let originalLine = sd
                                .first()
                                .map(|s| s.to_string())
                                .unwrap_or_else(|| lua_error!("attempt to index local 'line' (a nil value)"));
                            // L2087
                            let mut line = originalLine.clone();
                            // L2088
                            if usesVariableRolls {
                                // L2089 ORDER: should only be length 1 (c)
                                for (key, stat) in val_pairs(&legionAddition.get("stats")) {
                                    // L2090
                                    line = replaceHelperFunc(&line, &key, &stat, 100.0);
                                }
                            }
                            // L2093
                            let modList = parse_mod(&line);
                            // L2094
                            legionAddition.set(
                                "modListGenerated",
                                modList.clone().map_or(Val::Nil, |m| Val::Obj(Rc::new(ModListVal(m)))),
                            );
                            // L2095
                            nn.mod_list = modList;
                            // L2096
                            nn.divisor = Some(if line != originalLine { 100.0 } else { 1.0 });
                        }
                        // L2098
                        if !usesVariableRolls && modNode.truthy("totalMod") {
                            // L2099
                            nn.divisor = Some(first_mod_value(nn.mod_list.as_ref()));
                        }
                        newNode = Some(nn);
                        // L2101
                        break;
                    }
                }
            }
            // L2105
            if let Some(nn) = newNode {
                // L2106
                nodes.push(nn);
            }
        }
        // L2110
        let fwl = self.c(cx, "fallbackWeightsList");
        let powerStat = {
            let w = cx.w::<DropDownControl>(fwl);
            item_table(w.sel_index.checked_sub(1).and_then(|i| w.list.get(i)))
                .unwrap_or_else(|| lua_error!("attempt to index local 'statTable' (a nil value)"))
        };
        let Some(output) = self.generateFallbackWeights(cx, &nodes, &powerStat) else {
            return;
        };
        // L2111
        let mut newList = String::new();
        // L2112
        let weightScalar = 100.0;
        // L2113
        for legionNode in &output {
            let Some(w1) = legionNode.weight1 else {
                // `nil ~= 0` is true; `round(nil * ..)` raises
                lua_error!("attempt to perform arithmetic on field 'weight1' (a nil value)")
            };
            // L2114
            if w1 != 0.0 || legionNode.weight2.is_some_and(|w2| w2 != 0.0) {
                // L2115
                if !newList.is_empty() {
                    // L2116
                    newList.push('\n');
                }
                // L2118
                newList.push_str(&format!(
                    "{}, {}, {}, 0",
                    cat(&legionNode.id),
                    tostring_num(round(w1 * weightScalar, Some(3))),
                    tostring_num(round(legionNode.weight2.unwrap_or(0.0) * weightScalar, Some(3)))
                ));
            }
        }
        // L2121
        self.updateSearchList(cx, &newList, true);
    }
}

/// A table key from a Lua value (`t[k]`; nil and NaN read nil: a non-key).
fn lua_key(v: &Val) -> model::Key {
    match v {
        Val::Str(s) => model::Key::from(model::Name::from(&**s)),
        Val::Num(x) => model::Key::try_num(*x).unwrap_or_else(|| model::Key::from("\u{0}nil")),
        Val::Bool(b) => model::Key::from(*b),
        _ => model::Key::from("\u{0}nil"),
    }
}

// ---- searchSingleSocket / formatResults --------------------------------------------------------

/// `resultNodes[curSeed][id] = resultNodes[curSeed][id] or { targetNodeNames = { }, totalWeight = 0 }`
fn result_entry(seed: &Table, id: &Val) -> Table {
    let k = lua_key(id);
    if let Some(t) = seed.get(k).table() {
        return t;
    }
    let t = Table::new();
    t.set("targetNodeNames", Table::new());
    t.set("totalWeight", 0.0);
    match id {
        Val::Nil => lua_error!("table index is nil"),
        _ => seed.set(k, t.clone()),
    }
    t
}

/// `e.totalWeight = e.totalWeight + w`, `t_insert(e, targetNode)`, `t_insert(e.targetNodeNames,
/// name)` in the order of each site.
fn add_weight(e: &Table, w: f64) {
    let tw = e.get("totalWeight").num();
    e.set("totalWeight", tw + w);
}

/// `treeData.nodes[id].name`
fn tree_node_name(n: &Node) -> Val {
    n.extra.get("name")
}

impl Up {
    /// lua: TreeTab.lua:2442-2729 `local function searchSingleSocket(socketId, socketInfo)`
    /// @pob Classes/TreeTab.lua:2442-2729 h=8f8fb731
    fn searchSingleSocket(&self, cx: &mut Cx, socketId: f64, socketInfo: Option<Table>) -> Option<SearchResult> {
        let m = cx.build().model.clone();
        let spec = m.spec.get();
        let socketKey = int_key(&Val::Num(socketId));
        let socketNode = socketKey.and_then(|k| self.tree.nodes.get(k));
        // L2443
        let Some(socketNode) = socketNode.filter(|n| n.isJewelSocket.get()) else {
            // L2444
            return None;
        };

        // L2447
        let radiusNodes = radius_nodes(&socketNode);
        // L2448
        let isAbyssJewel = self.jt_ge(7.0);
        // L2449
        let zorathPath = if self.jt_is(11.0) { spec.GetShortestPathToClassStart_set(socketId) } else { None };
        // L2450
        if self.jt_is(11.0) && zorathPath.is_none() {
            // L2451
            return None;
        }
        // L2453
        let mut allocatedNodes: HashMap<i64, bool> = HashMap::new();
        // L2454
        let mut unAllocatedNodesDistance: HashMap<i64, f64> = HashMap::new();
        // L2455
        let mut targetNodes: LjIntMap<bool> = LjIntMap::new();
        // L2456
        let (mut attributeSmalls, mut otherSmalls) = (0.0, 0.0);
        // L2457
        let desiredNodes = Table::new();
        // L2458
        let mut minimumWeights: Vec<(Val, f64)> = Vec::new();
        // L2459
        let mut resultNodes: LjIntMap<Table> = LjIntMap::new();
        // L2460
        let mut rootNodes: std::collections::HashSet<i64> = std::collections::HashSet::new();
        // L2461
        let mut desiredIdx = 0.0;
        // L2462
        let searchListCombinedTbl = Table::new();
        // L2463
        let searchListNodeFound = Table::new();

        // L2465
        for (_, curRow) in self.search_list_tbl.borrow().clone().ipairs() {
            let curRow = curRow.table().unwrap_or_else(|| lua_error!("attempt to index local 'curRow' (a nil value)"));
            // L2466
            let k = curRow.get(1i64);
            if k.is_nil() {
                lua_error!("table index is nil");
            }
            searchListNodeFound.set(lua_key(&k), true);
            // L2467
            searchListCombinedTbl.push(curRow.copyTable(false));
        }
        // L2469
        for (_, curRow) in self.search_list_fallback_tbl.borrow().clone().ipairs() {
            let curRow = curRow.table().unwrap_or_else(|| lua_error!("attempt to index local 'curRow' (a nil value)"));
            // L2470
            if !searchListNodeFound.get(lua_key(&curRow.get(1i64))).truthy() {
                // L2471
                searchListCombinedTbl.push(curRow.copyTable(false));
            }
        }

        let total = total_mods(&self.jt_id());
        // L2475
        for (_, desiredNode) in searchListCombinedTbl.ipairs() {
            let desiredNode = desiredNode.table().unwrap();
            // L2476
            if desiredNode.len() <= 1 {
                continue;
            }
            // L2477
            let mut displayName: Option<String> = None;
            // L2478
            let mut singleStat = false;
            // L2479
            if let Some(total) = total {
                if desiredNode.get(1i64).raw_eq(&Val::from(format!("total_{}", pob_lua::string::lower(total)))) {
                    // L2480
                    desiredNode.set(1i64, "totalStat");
                    // L2481
                    displayName = Some(total.to_owned());
                }
            }
            // L2483
            if displayName.is_none() {
                // L2484
                for (_, v) in self.legion_nodes.ipairs() {
                    let legionNode = Lp::of(&v);
                    // L2485
                    if legionNode.id().raw_eq(&desiredNode.get(1i64)) {
                        // non-vaal replacements only support one nodeWeight
                        // L2487
                        if self.jt_num() > 1.0 && self.jt_num() < 7.0 {
                            // L2488
                            singleStat = true;
                        }
                        // L2490
                        displayName = Some(concat_strs(&legionNode.sd(), " + "));
                        // L2491
                        break;
                    }
                }
            }
            // L2495
            if displayName.is_none() {
                // L2496
                for (_, v) in self.legion_additions.ipairs() {
                    let legionAddition = Lp::of(&v);
                    // L2497
                    if legionAddition.id().raw_eq(&desiredNode.get(1i64)) {
                        // Original timeless additions only store an ID; Abyss records also store their rolls.
                        // L2499
                        singleStat = self.jt_num() < 7.0;
                        // L2500
                        displayName = Some(concat_strs(&legionAddition.sd(), " + "));
                        // L2501
                        break;
                    }
                }
            }
            // L2505
            let Some(displayName) = displayName else { continue };
            // L2506 (ipairs reads the row live)
            let mut i = 1i64;
            loop {
                let val = desiredNode.get(i);
                if val.is_nil() {
                    break;
                }
                // L2507
                if singleStat && i == 2 {
                    // L2508
                    let v = tonumber(&desiredNode.get(2i64))
                        .or_else(|| tonumber(&desiredNode.get(3i64)))
                        .unwrap_or(1.0);
                    desiredNode.set(2i64, v);
                }
                // L2510
                if val.raw_eq(&Val::from("required")) {
                    // L2511
                    let v = if singleStat && i == 2 { desiredNode.get(2i64) } else { Val::Num(0.0) };
                    desiredNode.set(i, v);
                    // L2512
                    let d4 = desiredNode.get(4i64);
                    if d4.is_nil() || cmp_num(&d4) < 0.001 {
                        // L2513
                        desiredNode.set(4i64, 0.001);
                    }
                }
                i += 1;
            }
            // L2517
            let d4 = desiredNode.get(4i64);
            if !d4.is_nil() && cmp_num(&tonumber(&d4).map_or(Val::Nil, Val::Num)) > 0.0 {
                // L2518
                minimumWeights.push((desiredNode.get(1i64), tonumber(&d4).unwrap()));
            }
            // if we're protecting a node and the number of protected nodes is less than the total allocated in radius and the total desired nodes is less than the total allocated in radius
            // these constraints avoid a blank result in the case where you set a min weight of 1 onto a non devotion stat with zero unprotected nodes
            // L2522
            let count = self.protected_nodes_count.get();
            let in_radius = cx.w::<TreeTab>(self.me).timeless.allocated_nodes_in_radius_count;
            if self.jt_is(4.0)
                && count > 0.0
                && count < in_radius
                && (searchListCombinedTbl.len() as f64) < in_radius
            {
                // L2523
                minimumWeights.push((desiredNode.get(1i64), 1.0));
            }
            let key1 = desiredNode.get(1i64);
            let entry = Table::new();
            // L2526-2527 / L2533-2534
            entry.set("nodeWeight", tonumber(&desiredNode.get(2i64)).unwrap_or(0.001));
            entry.set("nodeWeight2", tonumber(&desiredNode.get(3i64)).unwrap_or(0.001));
            // L2528 / L2535
            entry.set("displayName", displayName.as_str());
            // L2525
            if let Some(old) = desiredNodes.get(lua_key(&key1)).table() {
                // L2529
                entry.set("desiredIdx", old.get("desiredIdx"));
            } else {
                // L2532
                desiredIdx += 1.0;
                // L2536
                entry.set("desiredIdx", desiredIdx);
            }
            if key1.is_nil() {
                lua_error!("table index is nil");
            }
            desiredNodes.set(lua_key(&key1), entry);
        }
        // L2542
        searchListCombinedTbl.wipe();

        // L2544 ORDER: set (c)
        for (_, class) in self.tree.classes.pairs() {
            // L2545
            if let Some(k) = int_key(&class.get("startNodeId")) {
                rootNodes.insert(k);
            }
        }

        let socketFilter = self.socket_filter_state(cx);
        let alloc = |id: i64| spec.allocNodes.with(|a| a.contains_key(id));
        // L2548
        if socketFilter && !isAbyssJewel {
            // L2549
            if !self.td().get("socketFilterDistance").truthy() {
                self.td().set("socketFilterDistance", 0.0);
            }
            let dist = self.td().get("socketFilterDistance");
            // L2550 ORDER: set (c)
            for nodeId in radiusNodes.keys() {
                // L2551
                allocatedNodes.insert(nodeId, granted(&m, nodeId) || alloc(nodeId));
                // L2552
                if cmp_num(&dist) > 0.0 {
                    // L2553
                    let n = spec.nodes.with(|s| s.get(nodeId)).unwrap_or_else(|| {
                        lua_error!("attempt to index field '?' (a nil value)")
                    });
                    unAllocatedNodesDistance.insert(nodeId, n.pathDist.get().unwrap_or(1000.0));
                }
            }
        }

        // L2558
        if !isAbyssJewel {
            // L2559 ORDER: module notes (b)
            for nodeId in radiusNodes.keys() {
                let tn = self.tree.nodes.get(nodeId).unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                // L2560-2563
                let pass = !rootNodes.contains(&nodeId)
                    && !tn.isJewelSocket.get()
                    && !tn.isKeystone.get()
                    && (!socketFilter
                        || allocatedNodes.get(&nodeId).copied().unwrap_or(false)
                        || (cmp_num(&self.td().get("socketFilterDistance")) > 0.0
                            && *unAllocatedNodesDistance.get(&nodeId).unwrap_or_else(|| {
                                lua_error!("attempt to compare nil with number")
                            }) <= self.td().get("socketFilterDistance").num()));
                if pass {
                    // L2564
                    if tn.isNotable.get() || self.jt_is(1.0) {
                        // L2565
                        targetNodes.set(nodeId, Some(true));
                    } else if desiredNodes.truthy("totalStat") && !tn.isNotable.get() {
                        // L2567
                        if in_array(&["Strength", "Intelligence", "Dexterity"], &tn.dn.get()) {
                            // L2568
                            attributeSmalls += 1.0;
                        } else {
                            // L2570
                            otherSmalls += 1.0;
                        }
                    }
                }
            }
        }

        // L2577
        let mut seedWeights: HashMap<i64, f64> = HashMap::new();
        // L2578 Elegant Hubris
        let seedMultiplier = if self.jt_is(5.0) { 20.0 } else { 1.0 };
        let d = &model::data().timeless;
        let jtn = self.jt_num();
        let bound = |t: &model::gamedata::value::DataValue| -> f64 {
            t.table()
                .map(|t| t.index(jtn))
                .and_then(|v| v.num())
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field '?' (a nil value)"))
        };
        let additions = d.timelessJewelAdditions;
        let path_keys: Option<Vec<f64>> = zorathPath.as_ref().map(|p| p.keys().into_iter().map(|k| k as f64).collect());
        let ascend = spec.curAscendClassName.get();
        let label = cat(&self.jt().get("label"));
        // L2579 (a Lua numeric `for` with a float step)
        let start = bound(&d.timelessJewelSeedMin) * seedMultiplier;
        let stop = bound(&d.timelessJewelSeedMax) * seedMultiplier;
        let mut curSeed = start;
        while curSeed <= stop {
            let seedKey = curSeed as i64;
            // L2580
            seedWeights.insert(seedKey, 0.0);
            // L2581
            resultNodes.set(seedKey, Some(Table::new()));
            let weight_add = |seedWeights: &mut HashMap<i64, f64>, w: f64| {
                let e = seedWeights.entry(seedKey).or_insert(0.0);
                *e += w;
            };
            // L2582
            if isAbyssJewel {
                // Abyss files name the affected passives directly, so score those entries
                // instead of checking every passive in a jewel radius.
                // L2585 ORDER: LjIntMap (a)
                let lut = pob_gamedata::timeless_lut::readAbyssJewelLUT(
                    curSeed,
                    socketId,
                    jtn,
                    path_keys.as_deref(),
                    ascend.as_ref().map(model::Name::as_str),
                );
                for (targetNode, modification) in lut.pairs() {
                    // L2586
                    let treeNode = self.tree.nodes.get(targetNode);
                    // L2587
                    let mut scoreNode = treeNode.as_ref().is_some_and(|t| {
                        !rootNodes.contains(&targetNode) && !t.isJewelSocket.get() && !t.isKeystone.get()
                    });
                    // L2588
                    if scoreNode {
                        let t = treeNode.as_ref().unwrap();
                        if t.ascendancyName.get().is_some()
                            && self.protected_nodes.borrow().truthy(model::Name::from(&*t.dn.get()))
                        {
                            // L2589
                            resultNodes.set(seedKey, None);
                            // L2590
                            break;
                        }
                    }
                    // L2592
                    if scoreNode && socketFilter && zorathPath.is_none() {
                        // L2593
                        if !self.td().get("socketFilterDistance").truthy() {
                            self.td().set("socketFilterDistance", 0.0);
                        }
                        // L2594
                        let allocated = granted(&m, targetNode) || alloc(targetNode);
                        // L2595
                        let n = spec.nodes.with(|s| s.get(targetNode)).unwrap_or_else(|| {
                            lua_error!("attempt to index field '?' (a nil value)")
                        });
                        let distance = n.pathDist.get().unwrap_or(1000.0);
                        // L2596
                        let dist = self.td().get("socketFilterDistance");
                        scoreNode = allocated || (cmp_num(&dist) > 0.0 && distance <= dist.num());
                    }
                    // L2598
                    if scoreNode {
                        let treeNode = treeNode.as_ref().unwrap();
                        let seedT = resultNodes.get(seedKey).unwrap();
                        // L2599
                        for component in val_ipairs(&Val::from_value(&modification)) {
                            // L2600
                            let changedNode = resolve_component(&component, &self.legion);
                            // L2601
                            let changedNodeId = changedNode.as_ref().map_or(Val::Nil, Lp::id);
                            // L2602
                            let desiredNode = if changedNodeId.is_nil() {
                                None
                            } else {
                                desiredNodes.get(lua_key(&changedNodeId)).table()
                            };
                            // L2603
                            if let Some(desiredNode) = desiredNode {
                                let changedNode = changedNode.as_ref().unwrap();
                                // L2604
                                let (_, _statMod1, roll1) = component_roll(&component, changedNode, 1.0);
                                // L2605
                                let (_, statMod2, roll2) = component_roll(&component, changedNode, 2.0);
                                // L2606
                                let mut weight = desiredNode.get("nodeWeight").num() * roll1.unwrap_or(1.0);
                                // L2607
                                if statMod2.truthy() {
                                    // L2608
                                    weight += desiredNode.get("nodeWeight2").num() * roll2.unwrap_or(1.0);
                                }
                                // L2610
                                let e = result_entry(&seedT, &changedNodeId);
                                // L2611
                                add_weight(&e, weight);
                                // L2612
                                e.push(targetNode as f64);
                                // L2613
                                e.table("targetNodeNames").unwrap().push(tree_node_name(treeNode));
                                // L2614
                                weight_add(&mut seedWeights, weight);
                            }
                        }
                    }
                }
            }
            // This list is empty for Abyss jewels because they were scored above.
            // L2621 ORDER: module notes (b)
            for targetNode in targetNodes.keys() {
                let tn = self.tree.nodes.get(targetNode).unwrap();
                let tdn = model::Name::from(&*tn.dn.get());
                // L2622
                let jewelDataTbl = pob_gamedata::timeless_lut::readLUT(curSeed, targetNode as f64, jtn);
                // L2623
                if jewelDataTbl.is_empty() {
                    // L2624
                    cx.sys.platform.con_print(&format!("Missing LUT: {label}"));
                    continue;
                }
                let j = |i: f64| -> f64 {
                    (i >= 1.0 && i.fract() == 0.0)
                        .then(|| jewelDataTbl.get(i as usize - 1).copied())
                        .flatten()
                        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field '?' (a nil value)"))
                };
                let seedT = resultNodes.get(seedKey).unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                // L2626
                let mut curNode: Option<Lp>;
                // L2627
                let mut curNodeId = Val::Nil;
                let protected = self.protected_nodes.borrow().truthy(tdn);
                // L2628 protected
                if self.jt_is(4.0) && protected {
                    // L2629 protected node is a replacement, invalidate seed
                    if j(1.0) >= additions {
                        // L2630
                        resultNodes.set(seedKey, None);
                        // L2631
                        break;
                    }
                    // L2633 only add if user has not entered their own Devotion to the table
                    if !desiredNodes.truthy("totalStat") {
                        // L2634-2639
                        let e = Table::new();
                        e.set("nodeWeight", 0.1); // keeps total score low to let desired stats decide sort
                        e.set("nodeWeight2", 0.0);
                        e.set("displayName", "Devotion");
                        e.set("desiredIdx", desiredIdx + 1.0);
                        desiredNodes.set("totalStat", e);
                    }
                    // L2641
                    curNodeId = Val::from("totalStat");
                }
                let _ = &curNodeId;
                // L2643 replace
                if j(1.0) >= additions && !protected {
                    // L2644
                    curNode = lp_at(&self.legion_nodes, j(1.0) + 1.0 - additions);
                    // L2645
                    curNodeId = curNode.as_ref().map_or(Val::Nil, Lp::id);
                } else {
                    // add
                    // L2647
                    curNode = lp_at(&self.legion_additions, j(1.0) + 1.0);
                    // L2648
                    curNodeId = curNode.as_ref().map_or(Val::Nil, Lp::id);
                }
                // L2650
                if desiredNodes.truthy("totalStat") && reverse_total_mod_ids(&curNodeId) {
                    // L2651
                    curNodeId = Val::from("totalStat");
                }
                let desired = |id: &Val| -> Option<Table> {
                    if id.is_nil() {
                        None
                    } else {
                        desiredNodes.get(lua_key(id)).table()
                    }
                };
                // L2653
                if self.jt_is(1.0) {
                    // L2654
                    let headerSize = jewelDataTbl.len() as f64;
                    // L2655
                    if headerSize == 2.0 || headerSize == 3.0 {
                        // L2656
                        if let Some(dn) = desired(&curNodeId) {
                            // L2657
                            let e = result_entry(&seedT, &curNodeId);
                            let cn = curNode.as_ref().unwrap_or_else(|| lua_error!("attempt to index local 'curNode' (a nil value)"));
                            let stats = cn.get("stats");
                            let sorted = cn.get("sortedStats");
                            // L2658
                            let statMod1 = stats.get(lua_key(&sorted.get(1i64)));
                            // L2659
                            let mut weight = dn.get("nodeWeight").num() * j(statMod1.get("index").num() + 1.0);
                            // L2660
                            let statMod2 = stats.get(lua_key(&sorted.get(2i64)));
                            // L2661
                            if statMod2.truthy() {
                                // L2662
                                weight += dn.get("nodeWeight2").num() * j(statMod2.get("index").num() + 1.0);
                            }
                            // L2664
                            e.push(targetNode as f64);
                            // L2665
                            e.table("targetNodeNames").unwrap().push(tree_node_name(&tn));
                            // L2666
                            add_weight(&e, weight);
                            // L2667
                            weight_add(&mut seedWeights, weight);
                        }
                    } else if headerSize == 6.0 || headerSize == 8.0 {
                        // L2670
                        for (i0, _jewelData) in jewelDataTbl.iter().enumerate() {
                            let i = i0 as f64 + 1.0;
                            // L2671
                            curNode = lp_at(&self.legion_additions, j(i) + 1.0);
                            // L2672
                            curNodeId = curNode.as_ref().map_or(Val::Nil, Lp::id);
                            // L2673
                            if i <= headerSize / 2.0 {
                                // L2674
                                if let Some(dn) = desired(&curNodeId) {
                                    // L2675
                                    let e = result_entry(&seedT, &curNodeId);
                                    // L2676
                                    let weight = dn.get("nodeWeight").num() * j(i + headerSize / 2.0);
                                    // L2677
                                    add_weight(&e, weight);
                                    // L2678
                                    e.push(targetNode as f64);
                                    // L2679
                                    e.table("targetNodeNames").unwrap().push(tree_node_name(&tn));
                                    // L2680
                                    weight_add(&mut seedWeights, weight);
                                }
                            } else {
                                // L2683
                                break;
                            }
                        }
                    }
                } else if let Some(dn) = desired(&curNodeId) {
                    // L2688
                    let e = result_entry(&seedT, &curNodeId);
                    let w = dn.get("nodeWeight").num();
                    // L2689
                    add_weight(&e, w);
                    // L2690
                    e.push(targetNode as f64);
                    // L2691
                    e.table("targetNodeNames").unwrap().push(tree_node_name(&tn));
                    // L2692
                    weight_add(&mut seedWeights, w);
                }
            }
            // L2696
            if let (Some(seedT), Some(ts)) = (resultNodes.get(seedKey), desiredNodes.table("totalStat")) {
                // L2697
                let e = result_entry(&seedT, &Val::from("totalStat"));
                // L2698 Militant Faith
                if self.jt_is(4.0) {
                    // L2699
                    let mut addedWeight = ts.get("nodeWeight").num() * (5.0 * otherSmalls + 10.0 * attributeSmalls);
                    // L2700
                    addedWeight += e.get("totalWeight").num() * 4.0;
                    // L2701
                    add_weight(&e, addedWeight);
                    // L2702
                    weight_add(&mut seedWeights, addedWeight);
                } else {
                    // L2704
                    let mut addedWeight = ts.get("nodeWeight").num() * (4.0 * otherSmalls + 2.0 * attributeSmalls);
                    // L2705
                    addedWeight += e.get("totalWeight").num() * 19.0;
                    // L2706
                    add_weight(&e, addedWeight);
                    // L2707
                    weight_add(&mut seedWeights, addedWeight);
                }
            }
            // L2710
            if let Some(seedT) = resultNodes.get(seedKey) {
                // check minimum weights
                // L2712
                for (reqNode, weight) in &minimumWeights {
                    // L2713
                    let got = if reqNode.is_nil() {
                        0.0
                    } else {
                        seedT.get(lua_key(reqNode)).table().map_or(0.0, |t| t.get("totalWeight").num())
                    };
                    if got < *weight {
                        // L2714
                        resultNodes.set(seedKey, None);
                        // L2715
                        break;
                    }
                }
            }
            curSeed += seedMultiplier;
        }

        // L2722-2727
        Some(SearchResult {
            result_nodes: resultNodes,
            seed_weights: seedWeights,
            desired_nodes: desiredNodes,
            _socket_info: socketInfo,
        })
    }
}

/// `data.resolveAbyssJewelComponent(component, treeData.legion)` (DataAbyssJewelLookUpTableHelper.lua:48-57)
/// on the tree's legion `Table` (as PS-B's `resolve_abyss_component`, CCR-0020 §8).
fn resolve_component(component: &Val, legion: &Table) -> Option<Lp> {
    if !matches!(component, Val::Table(_) | Val::Data(_)) {
        lua_error!("attempt to index local 'component' (a nil value)");
    }
    let ty = component.get("type");
    let id = || component.get("id").num();
    // L52
    if ty.opt_num() == Some(1.0) {
        // L53
        let nodes = legion.table("nodes").unwrap_or_else(|| lua_error!("attempt to index field 'nodes' (a nil value)"));
        lp_at(&nodes, id() + 1.0 - model::data().timeless.timelessJewelAdditions)
    } else if ty.opt_num() == Some(2.0) {
        // L55
        let additions = legion.table("additions").unwrap_or_else(|| lua_error!("attempt to index field 'additions' (a nil value)"));
        lp_at(&additions, id() + 1.0)
    } else {
        None
    }
}

/// `data.getAbyssJewelComponentRoll(component, node, statIndex)`
/// (DataAbyssJewelLookUpTableHelper.lua:58-73) on a legion passive → (statKey, statMod, roll).
fn component_roll(component: &Val, node: &Lp, statIndex: f64) -> (Val, Val, Option<f64>) {
    // L60
    let mut statKey = Val::Nil;
    let mut statMod = Val::Nil;
    // L61 ORDER: the stat indices of a legion passive are unique (c)
    for (key, m) in val_pairs(&node.get("stats")) {
        // L62
        if m.get("index").opt_num() == Some(statIndex) {
            // L63
            statKey = key;
            statMod = m;
            // L64
            break;
        }
    }
    // L67
    let mut roll = if statMod.truthy() {
        let idx = statMod.get("index").num();
        component.get("rolls").get(idx).opt_num()
    } else {
        None
    };
    // A negative stored value is shown as a positive number in "reduced" descriptions.
    // L69
    if let Some(r) = roll {
        if r < 0.0 && cmp_num(&statMod.get("min")) >= 0.0 {
            // L70
            roll = Some(-r);
        }
    }
    // L72
    (statKey, statMod, roll)
}

/// lua: TreeTab.lua:2731-2745 `local function formatSearchValue(input)`
/// @pob Classes/TreeTab.lua:2731-2745 h=2f13a29a
fn formatSearchValue(input: f64) -> String {
    // L2732-2739
    let matchPattern1 = " 0";
    let replacePattern1 = "   ";
    let matchPattern2 = ".0 ";
    let replacePattern2 = "    ";
    let matchPattern3 = "  %.";
    let replacePattern3 = "0.";
    let matchPattern4 = "%.([0-9])0";
    let replacePattern4 = ".%1  ";
    let g = |s: String, p: &str, r: &str| pob_lua::gsub(&s, p, r, None).0;
    // L2740-2744
    let s = format!(" {}", lformat!("%006.2f", input));
    let s = g(s, matchPattern1, replacePattern1);
    let s = g(s, matchPattern1, replacePattern1);
    let s = g(s, matchPattern2, replacePattern2);
    let s = g(s, matchPattern2, replacePattern2);
    let s = g(s, matchPattern3, replacePattern3);
    g(s, matchPattern4, replacePattern4)
}

impl Up {
    /// lua: TreeTab.lua:2747-2800 `local function formatResults(resultNodes, seedWeights, desiredNodes, socketInfo)`
    /// @pob Classes/TreeTab.lua:2747-2800 h=4e3853be
    fn formatResults(&self, r: &SearchResult, socketInfo: Option<&Table>) -> Vec<Table> {
        // L2748
        let mut results = Vec::new();
        let tmw = self.td().get("totalMinimumWeight");
        // L2749 ORDER: LjIntMap (a)
        for (seedMatch, seedData) in r.result_nodes.pairs() {
            let sw = *r
                .seed_weights
                .get(&seedMatch)
                .unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
            // filter out the results so that only the ones that beat the total minimum weight parameter remain in search results
            // L2751
            let passesMin = !tmw.truthy() || sw >= cmp_num(&tmw);
            // L2752
            if !(sw > 0.0 && passesMin) {
                continue;
            }
            // L2753
            let labelPrefix = socketInfo.map_or_else(String::new, |s| format!("{} | ", cat(&s.get("label"))));
            // L2754-2758
            let result = Table::new();
            let seedNum = seedMatch as f64;
            let mut label = format!("{labelPrefix}{}:", tostring_num(seedNum));
            result.set("label", label.as_str());
            result.set("seed", seedNum);
            result.set("total", sw);
            // L2759
            if let Some(s) = socketInfo {
                // L2760
                result.set("socketId", s.get("id"));
                // L2761
                result.set("socketLabel", s.get("label"));
            }
            // L2763
            if self.jt_is(1.0) || self.jt_is(3.0) || self.jt_ge(7.0) {
                // These jewel types all use seeds below 10000.
                // L2765
                if seedNum < 1000.0 {
                    // L2766
                    label = format!("  {label}");
                }
            } else if self.jt_is(4.0) {
                // Militant Faith [2000-10000]
                // L2770
                if seedNum < 10000.0 {
                    // L2771
                    label = format!("  {label}");
                }
            } else {
                // Elegant Hubris [2000-160000]
                // L2775
                if seedNum < 10000.0 {
                    // L2776
                    label = format!("    {label}");
                } else if seedNum < 100000.0 {
                    // L2778
                    label = format!("  {label}");
                }
            }
            // L2781
            let mut sortedNodeArray: [Option<String>; 8] = Default::default();
            // L2782 ORDER: (c), module notes
            for (legionId, desiredNode) in r.desired_nodes.pairs() {
                let desiredIdx = desiredNode.get("desiredIdx").num();
                let slot = |i: f64| (i >= 1.0 && i <= 8.0 && i.fract() == 0.0).then(|| i as usize - 1);
                // L2783
                if let Some(sd) = seedData.get(legionId).table() {
                    // L2784
                    if desiredIdx == 8.0 {
                        // L2785
                        sortedNodeArray[7] = Some(" ...".to_owned());
                    } else if desiredIdx < 8.0 {
                        // L2787
                        if let Some(s) = slot(desiredIdx) {
                            sortedNodeArray[s] = Some(formatSearchValue(sd.get("totalWeight").num()));
                        }
                    }
                    // L2789
                    let e = result.get(legionId).table().unwrap_or_else(|| {
                        let t = Table::new();
                        result.set(legionId, t.clone());
                        t
                    });
                    // L2790
                    e.set("targetNodeNames", sd.get("targetNodeNames"));
                } else if desiredIdx < 8.0 {
                    // L2792
                    if let Some(s) = slot(desiredIdx) {
                        sortedNodeArray[s] = Some("     0     ".to_owned());
                    }
                }
            }
            // L2795 (`t_concat` up to the border: the indices are 1..n without holes)
            for s in sortedNodeArray.iter() {
                let Some(s) = s else { break };
                label.push_str(s);
            }
            result.set("label", label.as_str());
            // L2796
            results.push(result);
        }
        // L2799
        results
    }

    /// `t_sort(t, cmp)` of a list table in place.
    fn sort_results(t: &Table, lt: impl FnMut(&Table, &Table) -> bool) {
        let mut v: Vec<Table> = t.ipairs().into_iter().filter_map(|(_, e)| e.table()).collect();
        pob_lua::sort(&mut v, lt);
        for (i, e) in v.into_iter().enumerate() {
            t.set(i as i64 + 1, e);
        }
    }

    /// The common tail of the Search button's two branches (L2855-2859 / L2876-2880).
    fn search_done(&self, cx: &mut Cx) {
        let results = self.td().table("searchResults").unwrap_or_default();
        let st = self.c(cx, "searchTradeButton");
        // L2855 / L2876
        cx.base_mut(st).enabled = Prop::Val(!results.is_empty());
        // L2856 / L2877
        *self.last_search.borrow_mut() = None;
        // L2857 / L2878
        cx.w_mut::<ButtonControl>(st).label = Prop::Val("Open Trade URL".to_owned());
        let sr = self.c(cx, "searchResults");
        // L2858 / L2879
        cx.w_mut::<TimelessJewelListControl>(sr).highlight_index = None;
        // L2859 / L2880
        cx.w_mut::<ListControl>(sr).sel_index = Some(1);
    }

    /// The Search button's onClick (L2817-2882).
    fn search(&self, cx: &mut Cx) {
        let td = self.td().clone();
        let results = td.table("searchResults").unwrap_or_else(|| lua_error!("bad argument #1 to 'wipeTable'"));
        let shared = td.table("sharedResults").unwrap_or_else(|| lua_error!("bad argument #1 to 'wipeTable'"));
        let devotion = |k: &'static str| -> Val {
            let i = td.get(k);
            int_key(&i).map_or(Val::Nil, |i| self.devotion_variants.get(i))
        };
        // L2818
        if self.socket_id().raw_eq(&Val::Num(-1.0)) {
            // L2819
            results.wipe();
            // L2820
            shared.wipe();
            // L2821
            shared.set("type", td.get("jewelType"));
            // L2822
            shared.set("conqueror", td.get("conquerorType"));
            // L2823
            shared.set("devotionVariant1", devotion("devotionVariant1"));
            // L2824
            shared.set("devotionVariant2", devotion("devotionVariant2"));
            // L2825
            shared.set("multiSocket", true);

            // L2827
            for socketIdx in 2..=self.jewel_sockets.len() {
                // L2828
                let currentSocket = self.jewel_sockets.get(socketIdx as i64).table().unwrap();
                // L2829
                let id = currentSocket.get("id").num();
                let searchResult = self.searchSingleSocket(cx, id, Some(currentSocket.clone()));
                // L2830
                if let Some(searchResult) = searchResult {
                    // L2835
                    shared.set("desiredNodes", searchResult.desired_nodes.clone());
                    // L2837
                    for result in self.formatResults(&searchResult, Some(&currentSocket)) {
                        // L2838
                        results.set(results.len() as i64 + 1, result);
                    }
                }
            }

            // L2843-2848
            Self::sort_results(&results, |a, b| {
                let (ta, tb) = (a.get("total").num(), b.get("total").num());
                if ta == tb {
                    let l = |t: &Table| t.str("socketLabel").map_or_else(String::new, |s| s.to_string());
                    return l(a).as_bytes() < l(b).as_bytes();
                }
                ta > tb
            });
            self.search_done(cx);
        } else {
            let sock = td.table("jewelSocket").unwrap_or_default();
            // L2862
            let searchResult = self.searchSingleSocket(cx, self.socket_id().num(), Some(sock.clone()));
            // L2863
            if let Some(searchResult) = searchResult {
                // L2868
                results.wipe();
                // L2869
                shared.wipe();
                // L2870
                shared.set("type", td.get("jewelType"));
                // L2871
                shared.set("conqueror", td.get("conquerorType"));
                // L2872
                shared.set("devotionVariant1", devotion("devotionVariant1"));
                // L2873
                shared.set("devotionVariant2", devotion("devotionVariant2"));
                // L2874
                shared.set("socket", td.get("jewelSocket"));
                // L2875
                shared.set("desiredNodes", searchResult.desired_nodes.clone());

                // L2877
                for result in self.formatResults(&searchResult, None) {
                    // L2878
                    results.set(results.len() as i64 + 1, result);
                }
                // L2880
                Self::sort_results(&results, |a, b| a.get("total").num() > b.get("total").num());
                self.search_done(cx);
            }
        }
    }

    /// The Reset button's onClick (L2806-2813).
    fn reset(self: &Rc<Self>, cx: &mut Cx) {
        // L2807
        self.updateSearchList(cx, "", true);
        // L2808
        self.updateSearchList(cx, "", false);
        // L2809
        let a = self.c(cx, "abyssAscendancySelect");
        cx.w_mut::<DropDownControl>(a).sel_index = 1;
        // L2810
        if let Some(r) = self.td().table("searchResults") {
            r.wipe();
        }
        // L2811
        let st = self.c(cx, "searchTradeButton");
        cx.base_mut(st).enabled = Prop::Val(false);
        // L2812
        self.setAllocatedNodes(cx);
    }
}

// ---- the trade controls --------------------------------------------------------------------------

impl Up {
    /// `controls.searchTradeLeagueSelect:SetList(list)` with the aliasing of DropDownControl.lua:528-535
    /// (module notes).
    fn league_set_list(&self, cx: &mut Cx, list: &Table) {
        let Some(ls) = self.c_opt(cx, "searchTradeLeagueSelect") else { return };
        // L530 wipeTable(self.list)
        if let Some(old) = self.league_list.borrow_mut().replace(list.clone()) {
            old.wipe();
        }
        // L531-533
        let items: Vec<DropItem> = list.ipairs().into_iter().map(|(_, v)| DropItem::str(cat(&v))).collect();
        DropDownControl::set_list(cx, ls, items);
    }

    /// lua: TreeTab.lua:2356-2398 `local function updateLeagues()`
    fn updateLeagues(self: &Rc<Self>, cx: &mut Cx) {
        let realm = self.c(cx, "realmSelection");
        // L2357
        let currentRealmId = match DropDownControl::get_sel_value(cx, realm) {
            Some(v) => pob_lua::string::lower(&v.label),
            None => lua_error!("attempt to index a nil value"),
        };
        let all = cx.w::<TreeTab>(self.me).core.trade_leagues_list.clone();
        // L2358
        if all.get(currentRealmId.as_str()).is_nil() {
            all.set(currentRealmId.as_str(), Table::new());
        }
        // L2359
        let leagueList = all.table(currentRealmId.as_str()).unwrap();
        // L2360
        if !leagueList.is_empty() {
            // L2361
            self.league_set_list(cx, &leagueList);
            // restore the last league selected
            let last = cx.w::<TreeTab>(self.me).timeless.timeless_jewel_league_select.clone();
            // L2363
            for (i, league) in leagueList.ipairs() {
                // L2364
                if last.as_deref().is_some_and(|l| league.raw_eq(&Val::from(l))) {
                    // L2365
                    let ls = self.c(cx, "searchTradeLeagueSelect");
                    DropDownControl::set_sel(cx, ls, i, false);
                    // L2366
                    break;
                }
            }
        } else {
            let tqr = cx
                .w::<TreeTab>(self.me)
                .timeless
                .trade_query_requests
                .clone()
                .unwrap_or_else(|| lua_error!("attempt to index field 'tradeQueryRequests' (a nil value)"));
            let up = self.clone();
            let leagueList = Rc::new(RefCell::new(leagueList));
            let realm_id = currentRealmId.clone();
            // L2370
            fetch_leagues(
                cx,
                &tqr,
                currentRealmId,
                Rc::new(move |cx: &mut Cx, leagues: Vec<String>, errMsg: Option<String>| {
                    let popup_alive = cx.ui.is_alive(up.p.get());
                    // L2371
                    if let Some(errMsg) = errMsg {
                        // L2372 (DIVERGENCE(closed-popup-callback))
                        if popup_alive {
                            let msg = up.c(cx, "msg");
                            Up::set_label(
                                cx,
                                msg,
                                format!("^1Error fetching league list, default league will be used\n{errMsg}^7"),
                            );
                        }
                        // L2373
                        return;
                    }
                    let all = cx.w::<TreeTab>(up.me).core.trade_leagues_list.clone();
                    // L2375
                    let tempLeagueTable = Table::new();
                    // L2376
                    for league in &leagues {
                        // L2377
                        if !matches!(league.as_str(), "Standard" | "Ruthless" | "Hardcore" | "Hardcore Ruthless") {
                            // L2378
                            if !(pob_lua::find(league, "Hardcore", None, false).is_some()
                                || pob_lua::find(league, "Ruthless", None, false).is_some())
                            {
                                // set the dynamic, base league name to index 1 to sync league shown in dropdown on load with default/old behavior of open trade url
                                // L2380
                                tempLeagueTable.push(league.as_str());
                                // L2381
                                let cur = leagueList.borrow().clone();
                                for (_, val) in cur.ipairs() {
                                    // L2382
                                    tempLeagueTable.push(val);
                                }
                                // L2384
                                *leagueList.borrow_mut() = tempLeagueTable.copyTable(false);
                                // L2385
                                all.set(realm_id.as_str(), leagueList.borrow().clone());
                            } else {
                                // L2387
                                leagueList.borrow().push(league.as_str());
                            }
                        }
                    }
                    let l = leagueList.borrow().clone();
                    // L2391-2394
                    l.push("Standard");
                    l.push("Hardcore");
                    l.push("Ruthless");
                    l.push("Hardcore Ruthless");
                    // L2395 (DIVERGENCE(closed-popup-callback))
                    if popup_alive {
                        up.league_set_list(cx, &l);
                    }
                }),
            );
        }
    }

    /// The "Open Trade URL" button's onClick (L2221-2333).
    fn searchTrade(&self, cx: &mut Cx) {
        let td = self.td().clone();
        let results = td.table("searchResults").unwrap_or_default();
        let shared = td.table("sharedResults").unwrap_or_default();
        let sr = self.c(cx, "searchResults");
        let more = cx.w::<CheckBoxControl>(self.c(cx, "searchMore")).state;
        // L2222
        let mut seedTrades: Vec<Json> = Vec::new();
        // L2223
        let (mut startRow, mut endRow): (f64, f64);
        let hi = cx.w::<TimelessJewelListControl>(sr).highlight_index;
        let sel = cx.w::<ListControl>(sr).sel_index;
        // L2224
        if let (Some(hi), false) = (hi, more) {
            let sel = sel.unwrap_or_else(|| lua_error!("bad argument #1 to 'min' (number expected, got nil)")) as f64;
            // L2225
            startRow = m_min(sel, hi as f64);
            // L2226
            endRow = m_max(sel, hi as f64);
        } else {
            // L2228
            startRow = sel.map_or(1.0, |s| s as f64);
            // L2229
            let maxFilters = if more { 180.0 } else { 10.0 };
            // L2230
            let conq1 = shared.get("conqueror").get("id").opt_num() == Some(1.0);
            endRow = startRow + m_floor(maxFilters / if conq1 { 3.0 } else { 1.0 });
        }

        let n = results.len() as f64;
        // L2234
        let mut seedCount = m_min(n - startRow, endRow - startRow) + 1.0;
        // update if not highlighted already

        // L2237
        let prevSearch = *self.last_search.borrow();
        // L2238
        if let Some((p1, p2)) = prevSearch {
            if p1 == startRow && p2 == seedCount {
                // L2239
                startRow = endRow + 1.0;
                // L2240
                if startRow > n {
                    // L2241
                    return;
                }
                // L2243
                seedCount = m_min(n - startRow + 1.0, seedCount);
                // L2244
                endRow = startRow + seedCount - 1.0;
            }
        }
        // L2246
        cx.w_mut::<ListControl>(sr).sel_index = Some(startRow as usize);
        // scroll so that startRow is the first visible row
        // L2248
        TimelessJewelListControl::scroll_to_index(cx, sr, startRow);
        // L2249
        cx.w_mut::<TimelessJewelListControl>(sr).highlight_index = Some(endRow as usize);
        // L2250
        *self.last_search.borrow_mut() = Some((startRow, seedCount));

        let jtn = self.jt_num();
        let trade_ids = model::data()
            .timeless
            .timelessJewelTradeIDs
            .table()
            .map(|t| t.index(jtn).clone())
            .unwrap_or_default();
        let tid = |dv: &model::gamedata::value::DataValue| -> Option<String> {
            dv.as_str().map(str::to_owned).or_else(|| dv.num().map(tostring_num))
        };
        // L2252
        let mut i = startRow;
        while i <= startRow + seedCount - 1.0 {
            // L2253
            let result = results
                .get(i)
                .table()
                .unwrap_or_else(|| lua_error!("attempt to index local 'result' (a nil value)"));
            // L2255
            let conquerorKeystoneTradeIds = trade_ids.field("keystone");
            if conquerorKeystoneTradeIds.table().is_none() {
                lua_error!("attempt to index field 'keystone' (a nil value)");
            }
            let ck = |k: f64| tid(conquerorKeystoneTradeIds.table().unwrap().index(k));
            // L2256
            let conquerorTradeIds: Vec<Option<String>>;
            // L2257
            if self.jt_ge(7.0) {
                // L2258
                conquerorTradeIds = vec![ck(1.0)];
            } else if shared.get("conqueror").get("id").num() > 1.0 {
                // L2260
                conquerorTradeIds = vec![ck(shared.get("conqueror").get("id").num() - 1.0)];
            } else {
                // L2262
                conquerorTradeIds = vec![ck(1.0), ck(2.0), ck(3.0)];
            }

            // L2265 (ipairs: stops at the first nil)
            for tradeId in conquerorTradeIds.into_iter().map_while(|x| x) {
                // L2266-2272
                let seed = result.get("seed").num();
                seedTrades.push(Json::obj(vec![
                    ("id", Json::Str(tradeId)),
                    ("value", Json::obj(vec![("min", Json::Num(seed)), ("max", Json::Num(seed))])),
                ]));
            }
            i += 1.0;
        }

        // L2276-2282
        let tradeTypes = ["securable", "available", "onlineleague", "online", "any"];
        let tti = cx.w::<TreeTab>(self.me).timeless.trade_type_index;
        let option = tti
            .and_then(|i| i.checked_sub(1))
            .and_then(|i| tradeTypes.get(i))
            .map_or_else(|| lua_error!("attempt to index a nil value (trade type)"), |s| Json::Str((*s).to_owned()));
        // L2283-2302
        let mut stats = vec![Json::obj(vec![
            ("filters", Json::Arr(seedTrades)),
            ("type", Json::Str("count".to_owned())),
            ("value", Json::obj(vec![("min", Json::Num(1.0))])),
        ])];

        // L2304
        let devotion_ids = trade_ids.field("devotion");
        if devotion_ids.truthy() {
            // L2305
            let mut devotionFilters = Vec::new();
            let dv_id = |k: &'static str| shared.get(k).get("id").num();
            let dev = |k: f64| -> Json {
                match devotion_ids.table().map(|t| t.index(k)).and_then(tid) {
                    Some(s) => Json::obj(vec![("id", Json::Str(s))]),
                    // `{ id = nil }`: an empty table encodes as `[]` (dkjson isarray)
                    None => Json::Arr(Vec::new()),
                }
            };
            // L2306
            if dv_id("devotionVariant1") > 1.0 {
                // L2307
                devotionFilters.push(dev(dv_id("devotionVariant1") - 1.0));
            }
            // L2309
            if dv_id("devotionVariant2") > 1.0 {
                // L2310
                devotionFilters.push(dev(dv_id("devotionVariant2") - 1.0));
            }
            // L2312
            if !devotionFilters.is_empty() {
                // L2313-2316
                stats.push(Json::obj(vec![
                    ("filters", Json::Arr(devotionFilters)),
                    ("type", Json::Str("and".to_owned())),
                ]));
            }
        }
        let search = Json::obj(vec![
            (
                "query",
                Json::obj(vec![
                    ("status", Json::obj(vec![("option", option)])),
                    ("stats", Json::Arr(stats)),
                ]),
            ),
            ("sort", Json::obj(vec![("price", Json::Str("asc".to_owned()))])),
        ]);

        // if the league was not selected via dropdown, then default to the first league in the dropdown or "" if the leagues could not be read
        // L2321
        let rs = self.c(cx, "realmSelection");
        let selectedRealm = match DropDownControl::get_sel_value(cx, rs) {
            Some(v) => pob_lua::string::lower(&v.label),
            None => lua_error!("attempt to index a nil value"),
        };

        // L2323
        let realmPath = if selectedRealm == "pc" { String::new() } else { format!("{selectedRealm}/") };
        // L2324-2328
        let ls = self.c(cx, "searchTradeLeagueSelect");
        let league = match DropDownControl::get_sel_value(cx, ls) {
            Some(v) => v.label,
            None => lua_error!("attempt to concatenate a nil value"),
        };
        let q: String = dkjson_encode(&search)
            .bytes()
            .map(|b| {
                if b.is_ascii_alphanumeric() {
                    (b as char).to_string()
                } else {
                    format!("%{b:02X}")
                }
            })
            .collect();
        let url = format!("https://www.pathofexile.com/trade/search/{realmPath}{league}/?q={q}");
        // L2329
        cx.sys.platform.open_url(&url);
        // L2330
        cx.sys.platform.copy(&url);

        // L2332
        let st = self.c(cx, "searchTradeButton");
        cx.w_mut::<ButtonControl>(st).label = Prop::Val("Open Next Trade URL".to_owned());
    }
}

// ---- the constructor ---------------------------------------------------------------------------

/// `rect` of numbers.
fn r4(x: f64, y: f64, w: f64, h: f64) -> [Prop<f64>; 4] {
    rect(x, y, w, h)
}

fn tip<A: 'static>(lines: &'static [&'static str]) -> pob_ui_core::tooltip::host::TooltipFn<A> {
    Rc::new(move |cx, tooltip, _| {
        tooltip.clear(false);
        for l in lines {
            tooltip.add_line(cx, 16.0, l);
        }
    })
}

impl TreeTab {
    /// lua: TreeTab.lua:1170-2888 `TreeTabClass:FindTimelessJewel()`
    /// @pob Classes/TreeTab.lua:1170-1357 h=1a1985eb
    pub fn FindTimelessJewel(cx: &mut Cx, me: CtrlId) {
        // L1171
        let socketViewer = Rc::new(Viewer(RefCell::new(PassiveTreeView::new(cx))));
        let m = cx.build().model.clone();
        // L1172
        let treeData = m.spec.get().tree.get();
        let legion = treeData.legion.get().unwrap_or_else(|| lua_error!("attempt to index field 'legion' (a nil value)"));
        // L1173
        let legionNodes = legion.table("nodes").unwrap_or_default();
        // L1174
        let legionAdditions = legion.table("additions").unwrap_or_default();
        // L1175
        let timelessData = model::build_xml::timelessData(&m);
        // L1176 local controls = { } (the popup); L1177
        let modData = Table::new();
        // L1178-1195 ignoredMods, totalMods, totalModIDs, reverseTotalModIDs: [`IGNORED_MODS`],
        // [`total_mods`], [`total_mod_ids`], [`reverse_total_mod_ids`]
        // L1196-1208
        let jewelTypes = Table::new();
        for (label, name, id) in [
            ("Glorious Vanity", "vaal", 1.0),
            ("Lethal Pride", "karui", 2.0),
            ("Brutal Restraint", "maraketh", 3.0),
            ("Militant Faith", "templar", 4.0),
            ("Elegant Hubris", "eternal", 5.0),
            ("Heroic Tragedy", "kalguur", 6.0),
            ("Festering Vengeance", "abyss_murderous", 7.0),
            ("Extinguishing Grasp", "abyss_searching", 8.0),
            ("Baleful Dominion", "abyss_hypnotic", 9.0),
            ("Destructive Aspiration", "abyss_ghastly", 10.0),
            ("Reclaimed Malevolence", "abyss_special", 11.0),
        ] {
            let t = entry(label, id);
            t.set("name", name);
            jewelTypes.push(t);
        }
        // rebuild `timelessData.jewelType` as we only store the minimum amount of `jewelType` data in build XML
        let nonempty = |v: &Val| match v {
            Val::Table(t) => t.pairs().next().is_some(),
            other => lua_error!("bad argument #1 to 'next' (table expected, got {})", other.lua_type()),
        };
        // L1210
        if nonempty(&timelessData.get("jewelType")) {
            let want = timelessData.get("jewelType").get("id");
            // L1211
            for (_, jewelType) in jewelTypes.ipairs() {
                // L1212
                if jewelType.get("id").raw_eq(&want) {
                    // L1213
                    timelessData.set("jewelType", jewelType);
                    // L1214
                    break;
                }
            }
        } else {
            // L1218
            timelessData.set("jewelType", jewelTypes.get(1i64));
        }
        // L1220-1262
        let conquerorTypes = Table::new();
        for (i, labels) in [
            &["Any", "Doryani (Corrupted Soul)", "Xibaqua (Divine Flesh)", "Ahuana (Immortal Ambition)"][..],
            &["Any", "Kaom (Strength of Blood)", "Rakiata (Tempered by War)", "Akoya (Chainbreaker)"],
            &["Any", "Asenath (Dance with Death)", "Nasima (Second Sight)", "Balbala (The Traitor)"],
            &["Any", "Avarius (Power of Purpose)", "Dominus (Inner Conviction)", "Maxarius (Transcendence)"],
            &["Any", "Cadiro (Supreme Decadence)", "Victario (Supreme Grandstanding)", "Caspiro (Supreme Ostentation)"],
            &["Any", "Vorana (Black Scythe Training)", "Uhtred (Celestial Mathematics)", "Medved (The Unbreaking Circle)"],
            &["Tecrod (Overwhelming Hate)"],
            &["Ulaman (Weighted Exchange)"],
            &["Kurgal (Reconstructed Essence)"],
            &["Amanamu (The Loyal Few)"],
            &["Zorath"],
        ]
        .into_iter()
        .enumerate()
        {
            let list = Table::new();
            for (j, l) in labels.iter().enumerate() {
                list.push(entry(l, j as f64 + 1.0));
            }
            conquerorTypes.set(i as i64 + 1, list);
        }
        let conquerors_of = |id: &Val| -> Table {
            int_key(id)
                .and_then(|k| conquerorTypes.get(k).table())
                .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"))
        };
        // rebuild `timelessData.conquerorType` as we only store the minimum amount of `conquerorType` data in build XML
        // L1264
        if nonempty(&timelessData.get("conquerorType")) {
            let want = timelessData.get("conquerorType").get("id");
            // L1265
            for (_, conquerorType) in conquerors_of(&timelessData.get("jewelType").get("id")).ipairs() {
                // L1266
                if conquerorType.get("id").raw_eq(&want) {
                    // L1267
                    timelessData.set("conquerorType", conquerorType);
                    // L1268
                    break;
                }
            }
        } else {
            // L1272
            timelessData.set("conquerorType", conquerors_of(&timelessData.get("jewelType").get("id")).get(1i64));
        }
        // L1274-1292
        let devotionVariants = Table::new();
        for (id, label) in [
            (1.0, "Any"),
            (2.0, "Totem Damage"),
            (3.0, "Brand Damage"),
            (4.0, "Channelling Damage"),
            (5.0, "Area Damage"),
            (6.0, "Elemental Damage"),
            (7.0, "Elemental Resistances"),
            (8.0, "Effect of non-Damaging Ailments"),
            (9.0, "Elemental Ailment Duration"),
            (10.0, "Duration of Curses"),
            (11.0, "Minion Attack and Cast Speed"),
            (12.0, "Minions Accuracy Rating"),
            (13.0, "Mana Regen"),
            (14.0, "Mana Cost (legacy)"),
            (17.0, "Mana Cost Efficiency"),
            (15.0, "Non-Curse Aura Effect"),
            (16.0, "Defences from Shield"),
        ] {
            devotionVariants.push(entry(label, id));
        }
        // L1293
        let abyssAscendancyOptions = Table::new();
        // L1294 ORDER: a sequence (c)
        for (_, v) in legionNodes.pairs() {
            let node = Lp::of(&v);
            // L1295
            if lmatch(&node.id_str(), "^abyss_special_ascendancy_notable_%d+$") {
                // L1296-1300
                let t = Table::new();
                t.set("label", node.dn());
                t.set("node", v.clone());
                t.set("id", node.id());
                abyssAscendancyOptions.push(t);
            }
        }
        // L1303
        sort_by_label(&abyssAscendancyOptions);
        // L1304
        let any = Table::new();
        any.set("label", "Any");
        abyssAscendancyOptions.insert(1, any);
        // L1305
        let jewelSockets = Table::new();
        // L1306-1310
        let all = entry("All Sockets", -1.0);
        all.set("keystone", "Multi-Socket Search");
        jewelSockets.push(all);
        let spec = m.spec.get();
        // L1311 ORDER: module notes (c)
        for (socketId, socketData) in spec.nodes.with(LjIntMap::pairs) {
            // L1312
            if socketData.isJewelSocket.get() && socketData.extra.str("name").as_deref() != Some("Charm Socket") {
                // L1313
                let mut keystone = Val::from("Unknown");
                // L1314
                if socketId == 26725 {
                    // L1315
                    keystone = Val::from("Marauder");
                } else if socketId == 54127 {
                    // L1317
                    keystone = Val::from("Duelist");
                } else if socketId == 7960 {
                    // L1319
                    keystone = Val::from("Templar/Witch");
                } else {
                    // L1321
                    let mut minDistance = f64::INFINITY;
                    let tn = treeData
                        .nodes
                        .get(socketId)
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                    // L1322 ORDER: module notes (c)
                    for (_, nodeInRadius) in radius_nodes(&tn).pairs() {
                        // L1323
                        if nodeInRadius.isKeystone.get() {
                            // L1324
                            let distance = pob_lua::math::m_sqrt(
                                pob_lua::math::lpow(nodeInRadius.x.get() - socketData.x.get(), 2.0)
                                    + pob_lua::math::lpow(nodeInRadius.y.get() - socketData.y.get(), 2.0),
                            );
                            // L1325
                            if distance < minDistance {
                                // L1326
                                keystone = tree_node_name(&nodeInRadius);
                                // L1327
                                minDistance = distance;
                            }
                        }
                    }
                }
                // L1332
                let mut label = format!("{}: {}", cat(&keystone), socketId);
                // L1333
                if spec.allocNodes.with(|a| a.contains_key(socketId)) {
                    // L1334
                    label = format!("# {label}");
                }
                // L1336-1340
                let t = Table::new();
                t.set("label", label.as_str());
                t.set("keystone", keystone);
                t.set("id", socketId as f64);
                jewelSockets.push(t);
            }
        }
        // Sort all sockets except all sockets option
        // L1344
        let allSocketsEntry = jewelSockets.remove(Some(1));
        // L1345
        sort_by_label(&jewelSockets);
        // L1346
        jewelSockets.insert(1, allSocketsEntry);
        // rebuild `timelessData.jewelSocket` as we only store the minimum amount of `jewelSocket` data in build XML
        // L1348
        if nonempty(&timelessData.get("jewelSocket")) {
            let want = timelessData.get("jewelSocket").get("id");
            // L1349
            for (_, jewelSocket) in jewelSockets.ipairs() {
                // L1350
                if jewelSocket.get("id").raw_eq(&want) {
                    // L1351
                    timelessData.set("jewelSocket", jewelSocket);
                    // L1352
                    break;
                }
            }
        } else {
            // L1356
            timelessData.set("jewelSocket", jewelSockets.get(1i64));
        }

        let up = Rc::new(Up {
            me,
            p: Cell::new(CtrlId::default()),
            tree: treeData,
            legion,
            legion_nodes: legionNodes,
            legion_additions: legionAdditions,
            timeless_data: timelessData,
            mod_data: modData,
            jewel_types: jewelTypes,
            conqueror_types: conquerorTypes,
            devotion_variants: devotionVariants,
            abyss_ascendancy_options: abyssAscendancyOptions,
            jewel_sockets: jewelSockets,
            search_list_tbl: RefCell::new(Table::new()),
            search_list_fallback_tbl: RefCell::new(Table::new()),
            allocated_nodes: RefCell::new(LjIntMap::new()),
            protected_nodes: RefCell::new(Table::new()),
            protected_nodes_count: Cell::new(0.0),
            node_slider_stat_label: RefCell::new("None".to_owned()),
            node_slider2_stat_label: RefCell::new("None".to_owned()),
            last_search: RefCell::new(None),
            league_list: RefCell::new(None),
        });

        // the settings column is top anchored and the results/trade block bottom anchored,
        // so the panel grows by a row for every row the settings column gains
        // L2885-2887 (panelWidth: L2802)
        let edgePadding = 12.0;
        let listWidth = 500.0;
        let panelWidth = edgePadding * 3.0 + listWidth * 2.0;
        let rowSpacing = 6.0;
        let rowHeight = 17.0;
        let panelHeight = 565.0 + rowSpacing + rowHeight;
        popups::open_popup(
            cx,
            panelWidth,
            panelHeight,
            "Find a Timeless Jewel",
            move |cx, p| {
                up.p.set(p.id());
                build_controls(cx, &up, socketViewer);
            },
            PopupOpts::default(),
        );
    }
}

/// The statements of FindTimelessJewel from L1428 on, before `main:OpenPopup` (L2887): they run
/// inside the popup's builder (module notes).
fn build_controls(cx: &mut Cx, up: &Rc<Up>, socketViewer: Rc<Viewer>) {
    let p = up.p.get();
    let me = up.me;
    let td = up.timeless_data.clone();

    // L1428-1429 (Up::search_list_tbl / search_list_fallback_tbl)
    // L1503 initial load: [timelessData.searchList => searchListTbl]
    up.parseSearchList(cx, 0, false);
    // L1504 initial load: [timelessData.searchListFallback => searchListFallbackTbl]
    up.parseSearchList(cx, 0, true);

    // @pob Classes/TreeTab.lua:1517-1536 h=91f92730
    // L1517
    let devotionSelectLabel = cx
        .add(p, "devotionSelectLabel", LabelControl::new(Anchor::nil(TopRight, TopLeft), r4(920.0, 25.0, 0.0, 16.0), "^7Devotion modifiers:"))
        .id();
    // L1518
    Up::set_shown(cx, devotionSelectLabel, up.jt_is(4.0));
    // L1519-1521
    let t2 = td.clone();
    let devotionSelect1 = cx
        .add(
            p,
            "devotionSelect1",
            DropDownControl::new(
                Anchor::new(Top, devotionSelectLabel, Bottom),
                r4(0.0, 8.0, 200.0, 18.0),
                table_items(&up.devotion_variants),
                Some(Rc::new(move |_cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L1520
                    t2.set("devotionVariant1", index as f64);
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L1522
    cx.w_mut::<DropDownControl>(devotionSelect1).sel_index = sel_of(&td.get("devotionVariant1"));
    // L1523-1525
    let t2 = td.clone();
    let devotionSelect2 = cx
        .add(
            p,
            "devotionSelect2",
            DropDownControl::new(
                Anchor::new(Top, devotionSelect1, Bottom),
                r4(0.0, 7.0, 200.0, 18.0),
                table_items(&up.devotion_variants),
                Some(Rc::new(move |_cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L1524
                    t2.set("devotionVariant2", index as f64);
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L1526
    cx.w_mut::<DropDownControl>(devotionSelect2).sel_index = sel_of(&td.get("devotionVariant2"));

    // L1528-1531
    let rowSpacing = 6.0;
    let rowHeight = 17.0;
    let labelHeight = 16.0;
    let labelSpacing = 4.0;
    // L1532-1535 (Up::allocated_nodes, protected_nodes, protected_nodes_count; setAllocatedNodes)
    // L1536
    tt_state(cx, me).allocated_nodes_in_radius_count = 0.0;
    // L1538-1564 buildNodeOptionCheckboxes: [`Up::buildNodeOptionCheckboxes`]

    // @pob Classes/TreeTab.lua:1566-1601 h=b0cecb61
    // L1566-1581
    let u = up.clone();
    let jewelSelect = cx
        .add(
            p,
            "jewelSelect",
            DropDownControl::new(
                Anchor::nil(TopLeft, TopLeft),
                r4(520.0, 25.0, 200.0, rowHeight),
                table_items(&up.jewel_types),
                Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    let Some(value) = item_table(value) else {
                        lua_error!("attempt to index local 'value' (a nil value)")
                    };
                    // L1567
                    u.td().set("jewelType", value.clone());
                    // L1568 Militant Faith
                    let id = value.get("id");
                    Up::set_shown(cx, u.c(cx, "devotionSelectLabel"), id.opt_num() == Some(4.0));
                    // L1569
                    Up::set_shown(cx, u.c(cx, "abyssAscendancyLabel"), id.opt_num() == Some(11.0));
                    // L1570
                    let a = u.c(cx, "abyssAscendancySelect");
                    cx.w_mut::<DropDownControl>(a).sel_index = 1;
                    // L1571
                    let cl = int_key(&u.jt_id()).and_then(|k| u.conqueror_types.get(k).table()).unwrap_or_default();
                    let cs = u.c(cx, "conquerorSelect");
                    cx.w_mut::<DropDownControl>(cs).list = table_items(&cl);
                    // L1572
                    cx.w_mut::<DropDownControl>(cs).sel_index = 1;
                    // L1573
                    u.td().set("conquerorType", cl.get(1i64));
                    // L1574
                    let ns = u.c(cx, "nodeSelect");
                    cx.w_mut::<DropDownControl>(ns).sel_index = 1;
                    // L1575
                    u.buildMods(cx);
                    // L1576
                    u.updateSearchList(cx, "", false);
                    // L1577
                    u.updateSearchList(cx, "", true);
                    // L1578
                    if u.socket_filter_state(cx) {
                        // L1579
                        u.setAllocatedNodes(cx);
                    }
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L1582
    cx.w_mut::<DropDownControl>(jewelSelect).sel_index = sel_of(&up.jt_id());
    // L1583
    cx.add(p, "jewelSelectLabel", LabelControl::new(Anchor::new(Right, jewelSelect, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Jewel Type:"));

    // L1586-1589
    let t2 = td.clone();
    let conquerorSelect = cx
        .add(
            p,
            "conquerorSelect",
            DropDownControl::new(
                Anchor::new(TopLeft, jewelSelect, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                table_items(&int_key(&up.jt_id()).and_then(|k| up.conqueror_types.get(k).table()).unwrap_or_default()),
                Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L1587
                    t2.set("conquerorType", item_table(value).map_or(Val::Nil, Val::Table));
                    // L1588
                    Up::mod_flag(cx);
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L1590
    cx.w_mut::<DropDownControl>(conquerorSelect).sel_index = sel_of(&td.get("conquerorType").get("id"));
    // L1591
    cx.add(p, "conquerorSelectLabel", LabelControl::new(Anchor::new(Right, conquerorSelect, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Conqueror:"));
    // L1592-1601
    cx.w_mut::<DropDownControl>(conquerorSelect).tt.tooltip_func = Some(Rc::new(move |cx, tt, (_, _idx, val): DropTipArgs| {
        // L1593
        tt.clear(false);
        let Some(val) = val else { lua_error!("attempt to index local 'val' (a nil value)") };
        // L1594
        let keystoneName = pob_lua::lmatch(&val.label, "%((.+)%)$", None).map(|c| c[0].to_str().into_owned());
        // L1595
        if let Some(keystoneName) = keystoneName {
            // L1596
            let tree = cx.build().model.spec.get().tree.get();
            let keyStoneNode = tree.keystoneMap.obj::<Node>(model::Name::from(keystoneName.as_str()));
            // L1597
            if let Some(keyStoneNode) = keyStoneNode {
                // L1598
                viewer_add_node_tooltip(cx, me, tt, &keyStoneNode);
            }
        }
    }));

    // L1603-1635 setAllocatedNodes: [`Up::setAllocatedNodes`]

    // @pob Classes/TreeTab.lua:1638-1734 h=1819dc6a
    // L1638-1642
    let u = up.clone();
    let socketSelect = cx
        .add(
            p,
            "socketSelect",
            TimelessJewelSocketControl::new(
                Anchor::new(TopLeft, conquerorSelect, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                table_items(&up.jewel_sockets),
                Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L1639
                    u.td().set("jewelSocket", item_table(value).map_or(Val::Nil, Val::Table));
                    // L1640 reset list when changing sockets
                    u.setAllocatedNodes(cx);
                    // L1641
                    Up::mod_flag(cx);
                }) as SelFunc),
                Rc::new(AppTimelessBuild),
                socketViewer,
            ),
        )
        .id();
    // we need to search through `jewelSockets` for the correct `id` as the `idx` can become stale due to dynamic sorting
    // L1644
    let want = up.socket_id();
    for (idx, jewelSocket) in up.jewel_sockets.ipairs() {
        // L1645
        if jewelSocket.get("id").raw_eq(&want) {
            // L1646
            cx.w_mut::<DropDownControl>(socketSelect).sel_index = idx;
            // L1647
            break;
        }
    }
    // L1650
    cx.add(p, "socketSelectLabel", LabelControl::new(Anchor::new(Right, socketSelect, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Jewel Socket:"));

    // L1652-1664
    let u = up.clone();
    let socketFilter = cx
        .add(
            p,
            "socketFilter",
            CheckBoxControl::new(
                Anchor::new(TopLeft, socketSelect, BottomLeft),
                r4(0.0, rowSpacing, rowHeight, 0.0),
                None::<String>,
                Some(Rc::new(move |cx: &mut Cx, value: bool| {
                    // L1653
                    u.td().set("socketFilter", value);
                    // L1654
                    Up::mod_flag(cx);
                    // L1655-1657
                    Up::set_shown(cx, u.c(cx, "socketFilterAdditionalDistanceLabel"), value);
                    Up::set_shown(cx, u.c(cx, "socketFilterAdditionalDistance"), value);
                    Up::set_shown(cx, u.c(cx, "socketFilterAdditionalDistanceValue"), value);

                    // L1659
                    if value {
                        // L1660
                        u.setAllocatedNodes(cx);
                    } else {
                        // L1662
                        u.buildNodeOptionCheckboxes(cx, None);
                    }
                })),
                None,
                false,
            ),
        )
        .id();
    // L1665
    cx.add(p, "socketFilterLabel", LabelControl::new(Anchor::new(Right, socketFilter, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Filter Nodes:"));
    // L1666-1670
    cx.w_mut::<CheckBoxControl>(socketFilter).tt.tooltip_func = Some(tip(&[
        "^7Enable this option to exclude nodes that you do not have allocated on your active passive skill tree.",
        "^7This can be useful if you're never going to path towards those excluded nodes and don't care what happens to them.",
    ]));
    // L1671
    cx.w_mut::<CheckBoxControl>(socketFilter).state = td.truthy("socketFilter");

    // own row under the socket filter, so it never collides with the node distance slider
    // L1674-1677
    let t2 = td.clone();
    let socketAllocate = cx
        .add(
            p,
            "socketAllocate",
            CheckBoxControl::new(
                Anchor::new(TopLeft, socketFilter, BottomLeft),
                r4(0.0, rowSpacing, rowHeight, 0.0),
                None::<String>,
                Some(Rc::new(move |cx: &mut Cx, value: bool| {
                    // L1675
                    t2.set("socketAllocate", value);
                    // L1676
                    Up::mod_flag(cx);
                })),
                None,
                false,
            ),
        )
        .id();
    // L1678
    cx.add(p, "socketAllocateLabel", LabelControl::new(Anchor::new(Right, socketAllocate, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Socket Jewel:"));
    // L1679-1684
    cx.w_mut::<CheckBoxControl>(socketAllocate).tt.tooltip_func = Some(tip(&[
        "^7Double clicking a result also equips the jewel in its jewel socket.",
        "^7The socket must be allocated on your current tree; if it isn't, the jewel is only added to your item list.",
        "^7A jewel already in that socket is replaced.",
    ]));
    // L1685
    cx.w_mut::<CheckBoxControl>(socketAllocate).state = td.truthy("socketAllocate");

    // Protect notables that must not be replaced by Militant Faith or Reclaimed Malevolence.
    // L1688-1695
    let t2 = td.clone();
    let protectAllocatedLabel = cx
        .add(
            p,
            "protectAllocatedLabel",
            LabelControl::new(
                Anchor::nil(TopLeft, TopLeft),
                [
                    Prop::Val(15.0),
                    Prop::f(move |_cx, _| {
                        // L1691
                        if t2.get("jewelType").get("id").opt_num() == Some(11.0) { 85.0 } else { 25.0 }
                    }),
                    Prop::Val(0.0),
                    Prop::Val(16.0),
                ],
                "^7Protect allocated nodes from changing:",
            ),
        )
        .id();
    // set shown and list on load
    // L1697
    if cx.w::<CheckBoxControl>(socketFilter).state {
        // L1698
        up.setAllocatedNodes(cx);
    }
    // L1700-1702
    let t2 = td.clone();
    cx.base_mut(protectAllocatedLabel).shown = Prop::f(move |cx, _| {
        // L1701
        let id = t2.get("jewelType").get("id").opt_num();
        let sock = t2.get("jewelSocket");
        (id == Some(4.0) || id == Some(11.0))
            && cx.w::<CheckBoxControl>(socketFilter).state
            && !(sock.truthy() && sock.get("id").raw_eq(&Val::Num(-1.0)))
    });

    // This requirement is separate from ordinary node weights and remains above
    // the protection list so adding protected nodes does not move the selector.
    // L1706
    let abyssAscendancyLabel = cx
        .add(p, "abyssAscendancyLabel", LabelControl::new(Anchor::nil(TopLeft, TopLeft), r4(15.0, 25.0, 0.0, 16.0), "^7Required ascendancy outcome:"))
        .id();
    // L1707
    Up::set_shown(cx, abyssAscendancyLabel, up.jt_is(11.0));
    // L1708-1719
    let u = up.clone();
    let abyssAscendancySelect = cx
        .add(
            p,
            "abyssAscendancySelect",
            DropDownControl::new(
                Anchor::new(TopLeft, abyssAscendancyLabel, BottomLeft),
                r4(0.0, 8.0, 200.0, 18.0),
                table_items(&up.abyss_ascendancy_options),
                Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L1709
                    let mut searchLines: Vec<String> = Vec::new();
                    // L1710
                    for (_, searchRow) in u.search_list_tbl.borrow().clone().ipairs() {
                        let searchRow = searchRow.table().unwrap();
                        let first = match searchRow.get(1i64) {
                            Val::Str(s) => s.to_string(),
                            Val::Num(x) => tostring_num(x),
                            other => lua_error!("attempt to index field '?' (a {} value)", other.lua_type()),
                        };
                        // L1711
                        if !lmatch(&first, "^abyss_special_ascendancy_notable_") {
                            // L1712
                            searchLines.push(t_concat(&searchRow, ", "));
                        }
                    }
                    // L1715
                    let id = item_table(value).map_or(Val::Nil, |t| t.get("id"));
                    if id.truthy() {
                        // L1716
                        searchLines.push(format!("{}, 1, 0, 1", cat(&id)));
                    }
                    // L1718
                    u.updateSearchList(cx, &searchLines.join("\n"), false);
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L1720
    cx.w_mut::<DropDownControl>(abyssAscendancySelect).sel_index = 1;
    // L1721
    for (_, searchRow) in up.search_list_tbl.borrow().clone().ipairs() {
        let searchRow = searchRow.table().unwrap();
        // L1722
        for (optionIndex, option) in up.abyss_ascendancy_options.ipairs() {
            // L1723
            if option.get("id").raw_eq(&searchRow.get(1i64)) {
                // L1724
                cx.w_mut::<DropDownControl>(abyssAscendancySelect).sel_index = optionIndex;
                // L1725
                break;
            }
        }
    }
    // L1729-1734
    cx.w_mut::<DropDownControl>(abyssAscendancySelect).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, (_, _, value): DropTipArgs| {
        // L1730
        tooltip.clear(false);
        // L1731
        if let Some(node) = item_table(value.as_ref()).and_then(|t| t.obj::<Node>("node")) {
            // L1732
            viewer_add_node_tooltip(cx, me, tooltip, &node);
        }
    }));

    build_sliders(cx, up, socketFilter, socketAllocate);
}

/// A Lua number stored into `selIndex` (`controls.x.selIndex = timelessData.y`).
fn sel_of(v: &Val) -> usize {
    match v.opt_num() {
        Some(x) if x >= 0.0 => x as usize,
        _ => lua_error!("selIndex: a {} value", v.lua_type()),
    }
}

/// L1736-1950: the distance slider, the three node weight sliders and the node dropdown.
/// @pob Classes/TreeTab.lua:1736-1849 h=0b1bf516
fn build_sliders(cx: &mut Cx, up: &Rc<Up>, socketFilter: CtrlId, socketAllocate: CtrlId) {
    let p = up.p.get();
    let td = up.timeless_data.clone();
    let rowSpacing = 6.0;
    let rowHeight = 17.0;
    let labelHeight = 16.0;
    let labelSpacing = 4.0;

    // L1736
    let socketFilterAdditionalDistanceMAX = 10.0;
    // L1737
    let socketFilterAdditionalDistanceLabel = cx
        .add(p, "socketFilterAdditionalDistanceLabel", LabelControl::new(Anchor::new(Left, socketFilter, Right), r4(10.0, 0.0, 0.0, 16.0), "^7Node Distance:"))
        .id();
    // L1738-1741
    let u = up.clone();
    let dist = cx
        .add(
            p,
            "socketFilterAdditionalDistance",
            SliderControl::new(
                Anchor::new(Left, socketFilterAdditionalDistanceLabel, Right),
                r4(10.0, 0.0, 66.0, 18.0),
                Some(Rc::new(move |cx: &mut Cx, value: f64| {
                    // L1739
                    let d = m_floor(value * socketFilterAdditionalDistanceMAX + 0.01);
                    u.td().set("socketFilterDistance", d);
                    // L1740
                    let l = u.c(cx, "socketFilterAdditionalDistanceValue");
                    Up::set_label(cx, l, lformat!("^7%d", d));
                })),
                Some(ScrollWheelSpeedTbl {
                    shift: 1.0,
                    ctrl: 1.0 / (socketFilterAdditionalDistanceMAX * 2.0),
                    default: 1.0 / socketFilterAdditionalDistanceMAX,
                }),
            ),
        )
        .id();
    // L1742-1747
    cx.w_mut::<SliderControl>(dist).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, _| {
        // L1743
        tooltip.clear(false);
        // L1744
        if !cx.w::<SliderControl>(dist).dragging {
            // L1745
            tooltip.add_line(cx, 16.0, "^7This controls the maximum amount of points that need to be spent to grab a node before its filtered out");
        }
    }));
    // L1748-1756
    slider_tooltip_draw(cx, dist, 1384.0);
    // L1757
    let distValue = cx
        .add(p, "socketFilterAdditionalDistanceValue", LabelControl::new(Anchor::new(Left, dist, Right), r4(5.0, 0.0, 0.0, 16.0), "^70"))
        .id();
    // L1758
    let d0 = match td.get("socketFilterDistance") {
        Val::Nil | Val::Bool(false) => 0.0,
        v => v.num(),
    };
    SliderControl::set_val(cx, dist, d0 / socketFilterAdditionalDistanceMAX);
    // L1759-1761
    let sf = td.truthy("socketFilter");
    Up::set_shown(cx, socketFilterAdditionalDistanceLabel, sf);
    Up::set_shown(cx, dist, sf);
    Up::set_shown(cx, distValue, sf);

    // L1763
    let scrollWheelSpeedTbl = ScrollWheelSpeedTbl {
        shift: 0.01,
        ctrl: 0.0001,
        default: 0.001,
    };
    // L1764
    let scrollWheelSpeedTbl2 = ScrollWheelSpeedTbl {
        shift: 0.2,
        ctrl: 0.002,
        default: 0.02,
    };

    // `controls.searchListFallback and controls.searchListFallback.shown or false`
    let fallback_shown = |cx: &Cx, u: &Up| u.c_opt(cx, "searchListFallback").is_some_and(|f| cx.base(f).shown.raw());

    // L1766 (Up::node_slider_stat_label)
    // L1767-1770
    let u = up.clone();
    let nodeSlider = cx
        .add(
            p,
            "nodeSlider",
            SliderControl::new(
                Anchor::new(TopLeft, socketAllocate, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                Some(Rc::new(move |cx: &mut Cx, value: f64| {
                    // L1768
                    let l = u.c(cx, "nodeSliderValue");
                    Up::set_label(cx, l, lformat!("^7%.3f", value * 10.0));
                    // L1769
                    let f = fallback_shown(cx, &u);
                    u.parseSearchList(cx, 1, f);
                })),
                Some(scrollWheelSpeedTbl),
            ),
        )
        .id();
    // L1771
    cx.add(p, "nodeSliderLabel", LabelControl::new(Anchor::new(Right, nodeSlider, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Primary Node Weight:"));
    // L1772-1782
    let u = up.clone();
    cx.w_mut::<SliderControl>(nodeSlider).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, _| {
        // L1773
        tooltip.clear(false);
        // L1774
        if !cx.w::<SliderControl>(nodeSlider).dragging {
            let label = u.node_slider_stat_label.borrow().clone();
            // L1775
            if label == "None" {
                // L1776
                tooltip.add_line(cx, 16.0, "^7For nodes with multiple stats this slider controls the weight of the first stat listed.");
            } else {
                // L1778
                tooltip.add_line(cx, 16.0, "^7This slider controls the weight of the following stat:");
                // L1779
                tooltip.add_line(cx, 16.0, &format!("^7        {label}"));
            }
        }
    }));
    // L1783
    cx.add(p, "nodeSliderValue", LabelControl::new(Anchor::new(Left, nodeSlider, Right), r4(5.0, 0.0, 0.0, 16.0), "^71.000"));
    // L1784-1792
    slider_tooltip_draw(cx, nodeSlider, 1338.0);
    // L1793
    SliderControl::set_val(cx, nodeSlider, 0.1);

    // L1795 (Up::node_slider2_stat_label)
    // L1796-1799
    let u = up.clone();
    let nodeSlider2 = cx
        .add(
            p,
            "nodeSlider2",
            SliderControl::new(
                Anchor::new(TopLeft, nodeSlider, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                Some(Rc::new(move |cx: &mut Cx, value: f64| {
                    // L1797
                    let l = u.c(cx, "nodeSlider2Value");
                    Up::set_label(cx, l, lformat!("^7%.3f", value * 10.0));
                    // L1798
                    let f = fallback_shown(cx, &u);
                    u.parseSearchList(cx, 1, f);
                })),
                Some(scrollWheelSpeedTbl),
            ),
        )
        .id();
    // L1800
    cx.add(p, "nodeSlider2Label", LabelControl::new(Anchor::new(Right, nodeSlider2, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Secondary Node Weight:"));
    // L1801-1811
    let u = up.clone();
    cx.w_mut::<SliderControl>(nodeSlider2).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, _| {
        // L1802
        tooltip.clear(false);
        // L1803
        if !cx.w::<SliderControl>(nodeSlider2).dragging {
            let label = u.node_slider2_stat_label.borrow().clone();
            // L1804
            if label == "None" {
                // L1805
                tooltip.add_line(cx, 16.0, "^7For nodes with multiple stats this slider controls the weight of the second stat listed.");
            } else {
                // L1807
                tooltip.add_line(cx, 16.0, "^7This slider controls the weight of the following stat:");
                // L1808
                tooltip.add_line(cx, 16.0, &format!("^7        {label}"));
            }
        }
    }));
    // L1812
    cx.add(p, "nodeSlider2Value", LabelControl::new(Anchor::new(Left, nodeSlider2, Right), r4(5.0, 0.0, 0.0, 16.0), "^71.000"));
    // L1813-1821
    slider_tooltip_draw(cx, nodeSlider2, 1384.0);
    // L1822
    SliderControl::set_val(cx, nodeSlider2, 0.1);

    // L1824-1831
    let u = up.clone();
    let nodeSlider3 = cx
        .add(
            p,
            "nodeSlider3",
            SliderControl::new(
                Anchor::new(TopLeft, nodeSlider2, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                Some(Rc::new(move |cx: &mut Cx, value: f64| {
                    let l = u.c(cx, "nodeSlider3Value");
                    // L1825
                    if value == 1.0 {
                        // L1826
                        Up::set_label(cx, l, "^7Required");
                    } else {
                        // L1828
                        Up::set_label(cx, l, lformat!("^7%.f", value * 500.0));
                    }
                    // L1830
                    let f = fallback_shown(cx, &u);
                    u.parseSearchList(cx, 1, f);
                })),
                Some(scrollWheelSpeedTbl2),
            ),
        )
        .id();
    // L1832
    cx.add(p, "nodeSlider3Label", LabelControl::new(Anchor::new(Right, nodeSlider3, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Minimum Node Weight:"));
    // L1833-1838
    cx.w_mut::<SliderControl>(nodeSlider3).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, _| {
        // L1834
        tooltip.clear(false);
        // L1835
        if !cx.w::<SliderControl>(nodeSlider3).dragging {
            // L1836
            tooltip.add_line(cx, 16.0, "^7Seeds that do not meet the minimum weight threshold for a desired node are excluded from the search results.");
        }
    }));
    // L1839
    cx.add(p, "nodeSlider3Value", LabelControl::new(Anchor::new(Left, nodeSlider3, Right), r4(5.0, 0.0, 0.0, 16.0), "^70"));
    // L1840-1848
    slider_tooltip_draw(cx, nodeSlider3, 1728.0);
    // L1849
    SliderControl::set_val(cx, nodeSlider3, 0.0);

    // L1851-1875 updateSliders: [`Up::updateSliders`]

    build_node_select(cx, up, nodeSlider3);
}

/// L1877-1950: `buildMods()`, getLegionStatLabels and the node dropdown.
/// @pob Classes/TreeTab.lua:1877-1950 h=05631d16
fn build_node_select(cx: &mut Cx, up: &Rc<Up>, nodeSlider3: CtrlId) {
    let p = up.p.get();
    let rowSpacing = 6.0;
    let rowHeight = 17.0;
    let labelHeight = 16.0;
    let labelSpacing = 4.0;
    // L1877
    up.buildMods(cx);
    // L1878-1886 getLegionStatLabels: [`Up::getLegionStatLabels`]
    // L1887-1941
    let u = up.clone();
    let nodeSelect = cx
        .add(
            p,
            "nodeSelect",
            DropDownControl::new(
                Anchor::new(TopLeft, nodeSlider3, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                table_items(&up.mod_data),
                Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    u.nodeSelectChanged(cx, item_table(value));
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L1942
    cx.add(p, "nodeSelectLabel", LabelControl::new(Anchor::new(Right, nodeSelect, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Search for Node:"));
    // L1943-1950
    cx.w_mut::<DropDownControl>(nodeSelect).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, (mode, _, value): DropTipArgs| {
        // L1944
        tooltip.clear(false);
        let Some(value) = item_table(value.as_ref()) else {
            lua_error!("attempt to index local 'value' (a nil value)")
        };
        // L1945
        if mode != DropTipMode::Out {
            if let Some(d) = value.table("descriptions") {
                // L1946
                for (_, line) in d.ipairs() {
                    // L1947
                    tooltip.add_line(cx, 16.0, &format!("^7{}", cat(&line)));
                }
            }
        }
    }));

    build_lists(cx, up, nodeSelect);
}

/// L2120-2220: the fallback weight controls, the two search lists, their buttons, the results.
/// @pob Classes/TreeTab.lua:2120-2152 h=36035c9f
fn build_lists(cx: &mut Cx, up: &Rc<Up>, nodeSelect: CtrlId) {
    let p = up.p.get();
    let me = up.me;
    let td = up.timeless_data.clone();
    let rowSpacing = 6.0;
    let rowHeight = 17.0;
    let labelHeight = 16.0;
    let labelSpacing = 4.0;

    // L2120
    let fallbackWeightsList = Table::new();
    // L2121
    for ps in &model::data().powerStatList.stats {
        // L2122
        if !ps.ignoreForItems && &*ps.label != "Name" {
            // L2123-2127
            let full = powerStat_table(ps);
            let t = Table::new();
            t.set("label", format!("Sort by {}", &*ps.label));
            t.set("stat", full.get("stat"));
            t.set("transform", full.get("transform"));
            fallbackWeightsList.push(t);
        }
    }
    // L2130-2132
    let t2 = td.clone();
    let fwl = cx
        .add(
            p,
            "fallbackWeightsList",
            DropDownControl::new(
                Anchor::new(TopLeft, nodeSelect, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                table_items(&fallbackWeightsList),
                Some(Rc::new(move |_cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L2131
                    t2.get_or_new_table("fallbackWeightMode").set("idx", index as f64);
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L2133
    cx.add(p, "fallbackWeightsLabel", LabelControl::new(Anchor::new(Right, fwl, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Fallback Weight Mode:"));
    // L2134
    let idx = td.get("fallbackWeightMode").get("idx");
    cx.w_mut::<DropDownControl>(fwl).sel_index = if idx.truthy() { sel_of(&idx) } else { 1 };
    // L2135-2138
    let u = up.clone();
    let fwb = cx
        .add(
            p,
            "fallbackWeightsButton",
            ButtonControl::new(Anchor::new(Left, fwl, Right), r4(5.0, 0.0, 66.0, 18.0), "Generate", move |cx| {
                // L2136
                u.setupFallbackWeights(cx);
                // L2137
                let b = u.c(cx, "searchListFallbackButton");
                cx.w_mut::<ButtonControl>(b).label = Prop::Val("^4Fallback Nodes".to_owned());
            }),
        )
        .id();
    // L2139-2142
    cx.w_mut::<ButtonControl>(fwb).tt.tooltip_func = Some(tip(&[
        "^7Click this button to generate new fallback node weights, replacing your old ones.",
    ]));
    // L2143-2147
    let t2 = td.clone();
    let tmw = cx
        .add(
            p,
            "totalMinimumWeight",
            EditControl::new(
                Anchor::new(TopLeft, fwl, BottomLeft),
                r4(0.0, rowSpacing, 200.0, rowHeight),
                EditArgs {
                    init: Some(String::new()),
                    filter: Some("%D".to_owned()),
                    change_func: Some(Rc::new(move |cx: &mut Cx, val: &str| {
                        // L2144
                        let num = pob_lua::tonumber(val);
                        // L2145
                        t2.set("totalMinimumWeight", num.map_or(Val::Nil, Val::Num));
                        // L2146
                        Up::mod_flag(cx);
                    })),
                    ..EditArgs::default()
                },
            ),
        )
        .id();
    // L2148
    cx.add(p, "totalMinimumWeightLabel", LabelControl::new(Anchor::new(Right, tmw, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Total Minimum Weight:"));
    // L2149-2152
    cx.w_mut::<EditControl>(tmw).tt.tooltip_func = Some(tip(&[
        "^7Optional: Only show results where total weight meets or exceeds this value.",
    ]));

    // @pob Classes/TreeTab.lua:2154-2220 h=b45ac537
    // L2154-2158
    let listWidth = 500.0;
    let listHeight = 200.0;
    let buttonHeight = 20.0;
    let edgePadding = 12.0;
    let listYOffset = -(buttonHeight + edgePadding * 2.0);
    let search_list_args = |key: &'static str, fallback: bool| -> EditArgs {
        let u = up.clone();
        EditArgs {
            init: td.str(key).map(|s| s.to_string()),
            filter: Some("^%C\t\n".to_owned()),
            change_func: Some(Rc::new(move |cx: &mut Cx, value: &str| {
                // L2162 / L2173
                u.td().set(key, value);
                // L2163 / L2174
                u.parseSearchList(cx, 0, fallback);
                // L2164 / L2175
                Up::mod_flag(cx);
            })),
            line_height: Some(16.0),
            allow_zoom: true,
            ..EditArgs::default()
        }
    };
    // L2159-2165
    let searchList = cx
        .add(
            p,
            "searchList",
            EditControl::new(
                Anchor::nil(BottomLeft, BottomLeft),
                r4(edgePadding, listYOffset, listWidth, listHeight),
                search_list_args("searchList", false),
            ),
        )
        .id();
    // L2166
    Up::set_shown(cx, searchList, true);
    // L2167
    cx.base_mut(searchList).enabled = Prop::Val(true);
    // L2168
    let text = td.str("searchList").map_or_else(String::new, |s| s.to_string());
    EditControl::set_text(cx, searchList, &text, false);

    // L2170-2176
    let searchListFallback = cx
        .add(
            p,
            "searchListFallback",
            EditControl::new(
                Anchor::nil(BottomLeft, BottomLeft),
                r4(edgePadding, listYOffset, listWidth, listHeight),
                search_list_args("searchListFallback", true),
            ),
        )
        .id();
    // L2177
    Up::set_shown(cx, searchListFallback, false);
    // L2178
    cx.base_mut(searchListFallback).enabled = Prop::Val(false);
    // L2179
    let text = td.str("searchListFallback").map_or_else(String::new, |s| s.to_string());
    EditControl::set_text(cx, searchListFallback, &text, false);

    // L2181-2190
    let searchListButton = cx
        .add(
            p,
            "searchListButton",
            ButtonControl::new(
                Anchor::nil(BottomLeft, BottomLeft),
                r4(edgePadding, listYOffset - listHeight - rowSpacing, 106.0, buttonHeight),
                "^7Desired Nodes",
                move |cx| {
                    // L2184
                    if cx.base(searchListFallback).shown.raw() {
                        // L2185
                        Up::set_shown(cx, searchListFallback, false);
                        // L2186
                        cx.base_mut(searchListFallback).enabled = Prop::Val(false);
                        // L2187
                        Up::set_shown(cx, searchList, true);
                        // L2188
                        cx.base_mut(searchList).enabled = Prop::Val(true);
                    }
                },
            ),
        )
        .id();
    // L2191-2197
    cx.w_mut::<ButtonControl>(searchListButton).tt.tooltip_func = Some(tip(&[
        "^7This contains a list of your desired nodes along with their primary, secondary, and minimum weights.",
        "^7This list can be updated manually or by selecting the node you want to update via the search dropdown list and then moving the node weight sliders.",
    ]));
    // L2198
    cx.w_mut::<ButtonControl>(searchListButton).locked = Prop::f(move |cx, _| cx.base(searchList).shown.raw());

    // L2200-2206
    let searchListFallbackButton = cx
        .add(
            p,
            "searchListFallbackButton",
            ButtonControl::new(Anchor::new(Left, searchListButton, Right), r4(5.0, 0.0, 110.0, buttonHeight), "^7Fallback Nodes", move |cx| {
                // L2201
                Up::set_shown(cx, searchList, false);
                // L2202
                cx.base_mut(searchList).enabled = Prop::Val(false);
                // L2203
                Up::set_shown(cx, searchListFallback, true);
                // L2204
                cx.base_mut(searchListFallback).enabled = Prop::Val(true);
                // L2205
                if let Some(b) = cx.child(p, "searchListFallbackButton") {
                    cx.w_mut::<ButtonControl>(b).label = Prop::Val("^7Fallback Nodes".to_owned());
                }
            }),
        )
        .id();
    // L2207-2214
    cx.w_mut::<ButtonControl>(searchListFallbackButton).tt.tooltip_func = Some(tip(&[
        "^7This contains a list of your fallback nodes along with their primary, secondary, and minimum weights.",
        "^7This list can be updated manually or by selecting the node you want to update via the search dropdown list and then moving the node weight sliders.",
        "^7Fallback node weights are only used when no matching entry exists in the desired nodes list, allowing you to override or disable specific automatic weights.",
        "^7Fallback node weights typically contain automatically generated stat weights based on your current build.",
        "^7Any manual changes made to your fallback nodes are lost when you click the generate button, as it completely replaces them.",
    ]));
    // L2215
    cx.w_mut::<ButtonControl>(searchListFallbackButton).locked = Prop::f(move |cx, _| cx.base(searchListFallback).shown.raw());

    // L2217-2218
    let resultsList = TimelessJewelListControl::new(
        cx,
        Anchor::nil(BottomLeft, BottomLeft),
        r4(edgePadding * 2.0 + listWidth, -(buttonHeight + edgePadding * 2.0), listWidth, listHeight),
        Rc::new(AppTimelessBuild),
    );
    let searchResults = cx.add(p, "searchResults", resultsList).id();
    // L2219
    tt_state(cx, me).trade_query_requests = Some(TradeQueryRequests {
        host_name: "https://www.pathofexile.com/".to_owned(),
    });
    // L2220
    cx.add(p, "msg", LabelControl::new(Anchor::NONE, r4(-280.0, 5.0, 0.0, 20.0), ""));

    build_trade(cx, up, searchResults);
}

/// L2221-2440: the trade URL button, league / realm / trade type dropdowns, "search more".
/// @pob Classes/TreeTab.lua:2221-2342 h=824ed60c
fn build_trade(cx: &mut Cx, up: &Rc<Up>, searchResults: CtrlId) {
    let p = up.p.get();
    let me = up.me;
    let td = up.timeless_data.clone();
    let rowSpacing = 6.0;
    let labelHeight = 16.0;
    let labelSpacing = 4.0;
    let buttonHeight = 20.0;

    // L2221-2333
    let u = up.clone();
    let searchTradeButton = cx
        .add(
            p,
            "searchTradeButton",
            ButtonControl::new(
                Anchor::new(BottomRight, searchResults, TopRight),
                r4(0.0, -rowSpacing, 170.0, buttonHeight),
                "Open Trade URL",
                move |cx| u.searchTrade(cx),
            ),
        )
        .id();
    // L2334
    let n = td.table("searchResults").map_or(0, |t| t.len());
    cx.base_mut(searchTradeButton).enabled = Prop::Val(td.truthy("searchResults") && n > 0);
    // L2335-2342
    cx.w_mut::<ButtonControl>(searchTradeButton).tt.tooltip_func = Some(tip(&[
        "^7Click to generate and open a trade URL for searching for jewels in this list.",
        "^7Paste the URL in a web browser to search.",
        "",
        "^7You can click to select a row so that search begins from there.",
        "^7After selecting a row You can also shift+click on another row to select a range of rows to search.",
    ]));

    // @pob Classes/TreeTab.lua:2344-2440 h=1ee4c0bf
    // L2344-2347
    let searchTradeLeagueSelect = cx
        .add(
            p,
            "searchTradeLeagueSelect",
            DropDownControl::new(
                Anchor::new(Right, searchTradeButton, Left),
                r4(-labelSpacing, 0.0, 140.0, buttonHeight),
                Vec::new(),
                Some(Rc::new(move |cx: &mut Cx, _idx: usize, val: Option<&DropItem>| {
                    // L2346
                    tt_state(cx, me).timeless_jewel_league_select = val.map(|v| v.label.clone());
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // L2348-2349
    cx.add(p, "searchTradeLeagueLabel", LabelControl::new(Anchor::new(TopRight, searchTradeLeagueSelect, TopLeft), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7League:"));
    // Realm selection
    // L2351-2353
    tt_state(cx, me).realm_list = vec!["PC".to_owned(), "Sony".to_owned(), "Xbox".to_owned()];
    // L2354-2355
    let realms: Vec<DropItem> = cx.w::<TreeTab>(me).timeless.realm_list.iter().map(|s| DropItem::str(s.as_str())).collect();
    let realmSelection = cx
        .add(
            p,
            "realmSelection",
            DropDownControl::new(Anchor::new(BottomLeft, searchTradeLeagueSelect, TopLeft), r4(0.0, -rowSpacing, 50.0, buttonHeight), realms, None, None, false),
        )
        .id();
    // L2356-2398 updateLeagues: [`Up::updateLeagues`]
    // L2399-2402
    let u = up.clone();
    cx.w_mut::<DropDownControl>(realmSelection).sel_func = Some(Rc::new(move |cx: &mut Cx, idx: usize, _: Option<&DropItem>| {
        // L2400
        tt_state(cx, me).selected_realm_index = Some(idx);
        // L2401
        u.updateLeagues(cx);
    }));
    // remember previous choice
    // L2404
    let sel = cx.w::<TreeTab>(me).timeless.selected_realm_index.unwrap_or(1);
    DropDownControl::set_sel(cx, realmSelection, sel, false);
    // manually call the function because when initialising, because the
    // function does not get called when the selection index stays the same
    // L2407
    let f = cx.w::<DropDownControl>(realmSelection).sel_func.clone().unwrap();
    let si = cx.w::<DropDownControl>(realmSelection).sel_index;
    f(cx, si, None);
    // L2408-2409
    cx.add(p, "realmLabel", LabelControl::new(Anchor::new(TopRight, realmSelection, TopLeft), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Realm:"));

    // Buyout selection
    // L2412-2418
    let tradeTypes: Vec<DropItem> = [
        "Instant buyout",
        "Instant buyout and in person",
        "In person (online in league)",
        "In person (online)",
        "Any (includes offline)",
    ]
    .into_iter()
    .map(DropItem::str)
    .collect();
    // L2419-2422
    let tradeTypeSelection = cx
        .add(
            p,
            "tradeTypeSelection",
            DropDownControl::new(
                Anchor::new(BottomRight, searchTradeButton, TopRight),
                r4(0.0, -rowSpacing, 205.0, buttonHeight),
                tradeTypes,
                Some(Rc::new(move |cx: &mut Cx, index: usize, _: Option<&DropItem>| {
                    // L2421
                    tt_state(cx, me).trade_type_index = Some(index);
                }) as SelFunc),
                None,
                false,
            ),
        )
        .id();
    // remember previous choice
    // L2424
    let tti = cx.w::<TreeTab>(me).timeless.trade_type_index.unwrap_or(1);
    tt_state(cx, me).trade_type_index = Some(tti);
    // L2425
    DropDownControl::set_sel(cx, tradeTypeSelection, tti, false);

    // Checkbox to search a lot at once, or just a few
    // L2428-2433
    let searchMore = cx
        .add(
            p,
            "searchMore",
            CheckBoxControl::new(
                Anchor::new(BottomRight, tradeTypeSelection, TopRight),
                r4(0.0, -rowSpacing, 19.0, 0.0),
                None::<String>,
                Some(Rc::new(move |cx: &mut Cx, state: bool| {
                    // L2430
                    tt_state(cx, me).last_search_more = Some(state);
                })),
                Some(Prop::Val(Some(
                    "By default, this tool only searches for 4 or 10 (depending on conqueror) seeds at once to preserve sorting.\nThis option searches up to 180 or 60 seeds at once if you don't care about the sorting.".to_owned(),
                ))),
                false,
            ),
        )
        .id();
    // remember previous choice
    // L2435
    if let Some(true) = cx.w::<TreeTab>(me).timeless.last_search_more {
        // L2436
        cx.w_mut::<CheckBoxControl>(searchMore).state = true;
    }
    // L2438-2439
    cx.add(p, "searchMoreLabel", LabelControl::new(Anchor::new(Right, searchMore, Left), r4(-labelSpacing, 0.0, 0.0, labelHeight), "^7Search Maximum Amount:"));

    // L2442-2729 searchSingleSocket: [`Up::searchSingleSocket`]
    // L2731-2745 formatSearchValue: [`formatSearchValue`]
    // L2747-2800 formatResults: [`Up::formatResults`]

    build_buttons(cx, up);
}

/// L2802-2887: the Reset / Cancel / Search buttons (`main:OpenPopup` is the caller's).
/// @pob Classes/TreeTab.lua:2802-2888 h=499529fd
fn build_buttons(cx: &mut Cx, up: &Rc<Up>) {
    let p = up.p.get();
    // L2802-2804
    let edgePadding = 12.0;
    let listWidth = 500.0;
    let buttonHeight = 20.0;
    let panelWidth = edgePadding * 3.0 + listWidth * 2.0;
    let buttonDivider = 10.0;
    let buttonWidth = 80.0;
    // reset button anchored to middle of panel and other buttons anchored to it
    // L2806-2813
    let u = up.clone();
    let resetButton = cx
        .add(
            p,
            "resetButton",
            ButtonControl::new(
                Anchor::nil(BottomLeft, BottomLeft),
                r4(panelWidth / 2.0 - buttonWidth / 2.0, -edgePadding, buttonWidth, buttonHeight),
                "Reset",
                move |cx| u.reset(cx),
            ),
        )
        .id();
    // L2814-2816
    cx.add(
        p,
        "closeButton",
        ButtonControl::new(Anchor::new(Left, resetButton, Right), r4(buttonDivider, 0.0, buttonWidth, buttonHeight), "Cancel", |cx| {
            // L2815
            popups::close_popup(cx);
        }),
    );
    // L2817-2882
    let u = up.clone();
    cx.add(
        p,
        "searchButton",
        ButtonControl::new(Anchor::new(Right, resetButton, Left), r4(-buttonDivider, 0.0, buttonWidth, buttonHeight), "Search", move |cx| {
            u.search(cx);
        }),
    );
    // L2886-2887: the caller's `open_popup`
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `formatSearchValue` against LuaJIT 2.1 (`/usr/bin/luajit`) running TreeTab.lua:2731-2745
    /// verbatim.
    #[test]
    fn format_search_value_matches_luajit() {
        for (v, want) in [
            (0.0, "     0.0  "),
            (1.0, "     1.0  "),
            (2.5, "     2.5  "),
            (0.5, "     0.5  "),
            (10.0, "   10.0  "),
            (12.25, "   12.25"),
            (100.0, " 100.0  "),
            (0.001, "     0.0  "),
            (3.33333, "     3.33"),
            (99.995, " 100.0  "),
            (1000.0, " 1000.0  "),
            (7.05, "     7.05"),
            (0.1, "     0.1  "),
            (40.5, "   40.5  "),
            (123.456, " 123.46"),
        ] {
            assert_eq!(formatSearchValue(v), want, "formatSearchValue({v})");
        }
    }

    /// dkjson's sorted object keys and number spelling (dkjson.lua:133-151, 201-203).
    #[test]
    fn dkjson_encode_sorted() {
        let v = Json::obj(vec![
            ("sort", Json::obj(vec![("price", Json::Str("asc".into()))])),
            (
                "query",
                Json::obj(vec![
                    ("status", Json::obj(vec![("option", Json::Str("securable".into()))])),
                    (
                        "stats",
                        Json::Arr(vec![Json::obj(vec![
                            ("type", Json::Str("count".into())),
                            ("value", Json::obj(vec![("min", Json::Num(1.0))])),
                            ("filters", Json::Arr(vec![])),
                        ])]),
                    ),
                ]),
            ),
        ]);
        assert_eq!(
            dkjson_encode(&v),
            r#"{"query":{"stats":[{"filters":[],"type":"count","value":{"min":1}}],"status":{"option":"securable"}},"sort":{"price":"asc"}}"#
        );
        assert_eq!(dkjson_encode(&Json::Str("a\"b\\\n".into())), r#""a\"b\\\n""#);
    }

    /// `replaceHelperFunc` (TreeTab.lua:1981-1998): a range, and the `g` per-minute unit.
    #[test]
    fn replace_helper() {
        let m = Table::new();
        m.set("min", 10.0);
        m.set("max", 20.0);
        assert_eq!(
            replaceHelperFunc("(10-20)% increased Damage", &Val::from("x"), &Val::Table(m), 100.0),
            "100% increased Damage"
        );
        let g = Table::new();
        g.set("min", 5.0);
        g.set("max", 5.0);
        g.set("fmt", "g");
        assert_eq!(
            replaceHelperFunc(
                "Regenerate 5 Life per second",
                &Val::from("life_regen_per_minute"),
                &Val::Table(g),
                120.0
            ),
            "Regenerate 2 Life per second"
        );
    }
}
