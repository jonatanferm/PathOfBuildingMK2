//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ImportTab.lua:789-1912, 1925-1932
//! (character list, downloads, import tree/items/skills, reimport state). Owner: U44.
//!
//! @pob-file Classes/ImportTab.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The ImportTab methods U43's controls reach through `core::ImportHooks` (CCR-0109 §5; U44's
//! entries are wired in `core::default_import_hooks`):
//! `ProcessSiteJSON`, `SaveAccountHistory`, `DownloadPassiveTree`, `DownloadItems`,
//! `DownloadSiteCharacterList`, `BuildCharacterList`, `ImportPassiveTreeAndJewels`, the socket
//! group reimport state, `GetOrCreateGuardianItemSet`, `AssignGuardianItemSet`,
//! `ImportItemsAndSkills`, `ImportItem`, `ImportSocketedItems`, `GuessMainSocketGroup` and
//! `SetPredefinedBuildName`. U43 ports `Draw` (776-787) and `HexToChar`/`UrlDecode`
//! (1913-1924), CCR-0109 §5/§7. Also here as first consumer in wave order (ui doc §12 note 15):
//! `jsonToLua` and `codePointToUTF8` (Common.lua:216-230, 407-410).
//!
//! The import methods write the build model (`cx.build().model`, the calc plan's halves of the
//! tabs). The other tabs' UI methods they call go through [`TabSeams`] (CCR-0117 §1): U32's
//! slot hooks where they exist (PopulateSlots, SetSelItemId), else defaults that act on the
//! model as the Lua methods do until the owners land (ItemsTab U34/U38, SkillsTab U30-U31,
//! ConfigTab U41-U42; each entry names its owner).
//!
//! Lua semantics:
//! * Decoded JSON is `poe_api::Json` (dkjson: `null` reads as nil, see `json_get`); objects
//!   are serde_json maps (sorted keys), so `pairs` over a decoded object runs in key order.
//!   PoB's order there is LuaJIT's string hash order, per process random (STYLE §3.4);
//!   `LUA-ORDER` marks each such loop.
//! * `self:ProcessSiteJSON(json)` is `loadstring("return " .. jsonToLua(json))` in an empty
//!   environment: [`json_to_lua`] runs Common.lua's gsub chain, and [`eval_lua_table`]
//!   evaluates the resulting Lua expression (table constructors, strings with Lua escapes,
//!   numbers, `true`/`false`/`nil`, and globals of the empty environment, which are nil).
//!   A table whose keys are exactly `1..n` becomes a JSON array, any other table an object
//!   (number keys as `tostring` writes them).
//! * `copyTable(charData)` copies are `Json` clones.
//! * Socket groups and gems the import creates are the model's types; their Lua-nil fields
//!   follow CCR-0019 (`$includeInFullDPSNil`, `mainActiveSkill = NaN`).
//! * Mod line tables built before `item:BuildAndParseRaw()` (ImportTab.lua:1680-1777) carry
//!   `line` and the flags; their `mods`/`extra` (from `modLib.parseMod`) are dropped by
//!   ParseRaw, which rebuilds every mod line from the raw text (Item.lua `self.*ModLines = { }`),
//!   so only the flavour-text lines, whose insertion depends on `modList`, call `parseMod`.
//! * DIVERGENCE(dead-tab) (core.rs): a download that completes after its ImportTab was replaced
//!   writes into an object nobody reaches in Lua; the port skips the tab's writes and keeps the
//!   writes to `main`.
//!
//! ORDER: see the `LUA-ORDER` notes at each `pairs` loop.

use std::rc::Rc;

use pob_lua::{lfmt, n, Name};
use pob_ui_core::controls::{CheckBoxControl, DropDownControl, DropItem, EditControl};
use pob_ui_core::luastr::lmatch;
use pob_ui_core::prelude::*;

use super::accounts::{json_list, LeagueEntry, Realm};
use super::core::{alive, main_extra, main_extra_mut, sha1_hex, st, st_mut, url_encode};
use super::poe_api::{dkjson_decode, json_get, json_truthy, Json};
use crate::app::CxApp;
use crate::build::init as build_init;
use crate::build::loadouts::{self, ActValue};
use crate::build::on_frame::{self as build_on_frame, TabFlag};
use crate::build::BuildMode;
use crate::model::build_xml;
use crate::model::{self, ItemRef, Key, LStr, LjIntMap, NodeRef, Shared, Table, Val};
use crate::platform::net::{download_page, PageResponse};
use crate::tabs::items::slot as items_slot;
use crate::tabs::items::slot_helper as items_slot_helper;

/// This unit's part of the split class struct (ui doc §12.1): U44's methods keep their state in
/// U43's `core::State` (CCR-0109 §5 lists the fields), so it has none of its own.
#[derive(Default)]
pub struct State {}

/// A `siteCharSelect` / `charSelect` entry's fields besides `label`: `{ label = charName,
/// char = char, searchFilter = .., detail = .. }` (ImportTab.lua:1080-1085).
#[derive(Clone, Debug)]
pub struct CharEntry {
    pub char: Json,
}

// ---- seams to the tabs whose UI is not ported (CCR-0117 §1) ----------------------------------

pub type Hook = Rc<dyn Fn(&mut Cx)>;

/// The other tabs' UI methods the import calls. Each default acts on the model as the Lua
/// method does; the owners replace their entries when their tabs land.
#[derive(Clone)]
pub struct TabSeams {
    /// `self.build.itemsTab:DeleteItem(item)` (ItemsTab.lua:1857-1903, U34)
    pub items_delete_item: Rc<dyn Fn(&mut Cx, &ItemRef, bool)>,
    /// `self.build.itemsTab:PopulateSlots()` (ItemsTab.lua:1652-1656 and the slots' UI lists,
    /// U32/U34)
    pub items_populate_slots: Hook,
    /// `self.build.itemsTab:AddUndoState()` (UndoHandler.lua:34-41 on ItemsTab, U38)
    pub items_add_undo_state: Hook,
    /// `self.build.itemsTab.slots[slotName]:SetSelItemId(id)` (ItemSlotControl.lua:72-84, U32)
    pub items_slot_set_sel_item_id: Rc<dyn Fn(&mut Cx, &str, f64)>,
    /// `self.build.skillsTab:AddUndoState()` (UndoHandler.lua:34-41 on SkillsTab, U30)
    pub skills_add_undo_state: Hook,
    /// `self.build.skillsTab.controls.groupList.selIndex = nil` / `.selValue = nil`
    /// (SkillListControl, U31)
    pub skills_clear_group_list_selection: Hook,
    /// `self.build.skillsTab:SetDisplayGroup(socketGroup)` (SkillsTab.lua:1275-1308, U30)
    pub skills_set_display_group: Rc<dyn Fn(&mut Cx, Option<model::SocketGroupRef>)>,
    /// `self.build.configTab.varControls[var]:SetSel(index)` (DropDownControl.lua:160-169 with
    /// the option's selFunc, ConfigTab.lua:316-321, U41)
    pub config_var_set_sel: Rc<dyn Fn(&mut Cx, &str, f64)>,
}

/// The stand-ins (CCR-0117 §1).
pub fn default_tab_seams() -> TabSeams {
    TabSeams {
        items_delete_item: Rc::new(model_delete_item),
        // U32's (the model half, then the slot controls' dropdown lists)
        items_populate_slots: Rc::new(|cx| {
            let f = items_slot_helper::items_hooks().populate_slots;
            f(cx);
        }),
        items_add_undo_state: Rc::new(|cx| set_tab_mod_flag(cx, TabFlag::Items)),
        items_slot_set_sel_item_id: Rc::new(model_slot_set_sel_item_id),
        skills_add_undo_state: Rc::new(|cx| set_tab_mod_flag(cx, TabFlag::Skills)),
        // the group list control (U31) does not exist yet: nothing holds its selection
        skills_clear_group_list_selection: Rc::new(|_| {}),
        skills_set_display_group: Rc::new(|cx, g| {
            let m = cx.build().model.clone();
            m.skillsTab.SetDisplayGroup(&m, g);
        }),
        config_var_set_sel: Rc::new(model_config_var_set_sel),
    }
}

thread_local! {
    static TAB_SEAMS: std::cell::RefCell<Option<TabSeams>> = const { std::cell::RefCell::new(None) };
}

/// The installed seams (a clone; entries are `Rc`s).
pub fn tab_seams() -> TabSeams {
    TAB_SEAMS.with(|h| h.borrow_mut().get_or_insert_with(default_tab_seams).clone())
}

/// Replace seam entries (tests, and owners that register at run time).
pub fn set_tab_seams(f: impl FnOnce(&mut TabSeams)) {
    TAB_SEAMS.with(|h| f(h.borrow_mut().get_or_insert_with(default_tab_seams)));
}

/// `tab:AddUndoState()`'s `self.modFlag = true` (UndoHandler.lua:37) through the shell's flag
/// seam (`on_frame::ShellHooks::set_tab_flag`); the undo buffer is the tab's (not ported).
fn set_tab_mod_flag(cx: &mut Cx, flag: TabFlag) {
    let f = build_on_frame::shell_hooks().set_tab_flag;
    f(cx, flag, true);
}

/// `slot:SetSelItemId(selItemId)` on the model's slot table (ItemSlotControl.lua:72-84: U32's
/// `tabs::items::slot::set_sel_item_id`, its copy of `pob_calc`'s private
/// `ItemSlot_SetSelItemId`, CCR-0107 §2 / CCR-0117 §2).
fn model_slot_set_sel_item_id(cx: &mut Cx, slot_name: &str, sel_item_id: f64) {
    let m = cx.build().model.clone();
    let slot = m
        .itemsTab
        .slots
        .with(|s| s.get(&Name::from(slot_name)).cloned())
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    items_slot::set_sel_item_id(&m, &slot, sel_item_id);
}

/// `self.build.itemsTab:DeleteItem(item, deferUndoState)` on the model (ItemsTab.lua:1857-1903,
/// U34's; CCR-0117 §1).
fn model_delete_item(cx: &mut Cx, item: &ItemRef, defer_undo_state: bool) {
    let m = cx.build().model.clone();
    let id = item.id.get();
    let id_val = id.map_or(Val::Nil, Val::Num);
    // L1858 LUA-ORDER (string keys; each slot is independent)
    let slots: Vec<(Name, Table)> = m
        .itemsTab
        .slots
        .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
    for (slot_name, slot) in slots {
        // L1859
        if slot.get(n!("selItemId")).raw_eq(&id_val) {
            // L1860
            items_slot::set_sel_item_id(&m, &slot, 0.0);
            // L1861
            m.buildFlag.set(true);
        }
        // L1863
        if slot.get(n!("nodeId")).is_nil() {
            // L1864 LUA-ORDER (item set ids; each set is independent)
            let sets: Vec<Table> = m.itemsTab.itemSets.with(|s| s.values().cloned().collect());
            for item_set in sets {
                let entry = item_set
                    .table(slot_name)
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                // L1865
                if entry.get(n!("selItemId")).raw_eq(&id_val) {
                    // L1866
                    entry.set(n!("selItemId"), 0.0);
                    // L1867
                    m.buildFlag.set(true);
                }
            }
        }
    }
    // L1872
    if let Some(id) = id.and_then(model::items_tab::id_key) {
        let pos = m
            .itemsTab
            .orderList
            .with(|l| l.iter().position(|x| *x == id));
        if let Some(pos) = pos {
            // L1874
            m.itemsTab.orderList.with_mut(|l| {
                l.remove(pos);
            });
        }
    }
    // L1878 (an array: index order)
    for spec in m.treeTab.specList.snapshot() {
        // L1879
        let mut rebuild_cluster_jewel_graphs = false;
        // L1880 LUA-ORDER (LjIntMap: PoB's order)
        for (node_id, item_id) in spec.jewels.with(LjIntMap::pairs) {
            // L1881
            if Some(item_id) == id {
                // L1882
                spec.jewels.with_mut(|j| j.set(node_id, Some(0.0)));
                // L1883
                rebuild_cluster_jewel_graphs = true;
                // Deallocate all nodes that required this jewel
                // L1885
                if let Some(node) = spec.nodes.with(|n| n.get(node_id)) {
                    // L1886 (`depNodeId` is the list index, as in PoB)
                    for (dep_node_id, dep_node) in node.depends.get().snapshot().iter().enumerate()
                    {
                        // L1887
                        dep_node.alloc.set(false);
                        // L1888
                        spec.allocNodes
                            .with_mut(|a| a.set(dep_node_id as i64 + 1, None));
                    }
                    // L1890
                    node.alloc.set(false);
                    // L1891
                    spec.allocNodes.with_mut(|a| a.set(node_id, None));
                }
            }
        }
        // L1895
        if rebuild_cluster_jewel_graphs && !defer_undo_state {
            // L1896
            spec.BuildClusterJewelGraphs();
        }
    }
    // L1899
    if let Some(k) = id.and_then(model::items_tab::id_key) {
        m.itemsTab.items.with_mut(|i| {
            i.shift_remove(&k);
        });
    }
    // L1900
    if !defer_undo_state {
        let s = tab_seams();
        // L1901
        (s.items_populate_slots)(cx);
        // L1902
        (s.items_add_undo_state)(cx);
    }
}

/// `self.build.configTab.varControls[var]:SetSel(newSel)` on the model's control table
/// (DropDownControl.lua:160-169; the option dropdowns have no search, so `GetDropCount` is
/// `#self.list` and `DropIndexToListIndex` the identity) with the list option's selFunc
/// (ConfigTab.lua:316-321).
fn model_config_var_set_sel(cx: &mut Cx, var: &str, new_sel: f64) {
    let m = cx.build().model.clone();
    let var = Name::from(var);
    let control = m.configTab.varControl(var);
    let list = control
        .table(n!("list"))
        .unwrap_or_else(|| lua_error!("attempt to get length of field 'list' (a nil value)"));
    // L161
    let new_sel = pob_lua::math::m_max(1.0, pob_lua::math::m_min(list.len() as f64, new_sel));
    // L163
    if !Val::Num(new_sel).raw_eq(&control.get(n!("selIndex"))) {
        // L164
        control.set(n!("selIndex"), new_sel);
        // L166 → ConfigTab.lua:317 (`value.val` of `self.list[newSel]`)
        let value = list.get(new_sel);
        let val = value.get(n!("val"));
        let input = m
            .configTab
            .activeConfigSet()
            .table(n!("input"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'input' (a nil value)"));
        input.set(var, val);
        // ConfigTab.lua:318
        set_tab_mod_flag(cx, TabFlag::Config);
        // ConfigTab.lua:319
        m.configTab.BuildModList(&m);
        // ConfigTab.lua:320
        m.buildFlag.set(true);
    }
}

// ---- JSON value helpers ------------------------------------------------------------------------

/// `v` used as a string (`..`, `:lower()`, `:match()`): a string, or a number Lua coerces;
/// anything else is the Lua error.
fn jstr(v: Option<&Json>, what: &str) -> String {
    match v {
        Some(Json::String(s)) => s.clone(),
        Some(Json::Number(x)) => pob_lua::tostring_num(x.as_f64().unwrap_or(f64::NAN)),
        Some(other) => lua_error!(
            "attempt to concatenate field '{what}' (a {} value)",
            lua_type(Some(other))
        ),
        None => lua_error!("attempt to concatenate field '{what}' (a nil value)"),
    }
}

/// `v` used as a number (arithmetic / comparison): a number, or a numeric string.
fn jnum(v: Option<&Json>, what: &str) -> f64 {
    match v {
        Some(Json::Number(x)) => x.as_f64().unwrap_or(f64::NAN),
        Some(Json::String(s)) => pob_lua::tonumber(s).unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on field '{what}' (a string value)")
        }),
        other => lua_error!(
            "attempt to perform arithmetic on field '{what}' (a {} value)",
            lua_type(other)
        ),
    }
}

/// `v` as an optional number (nil stays nil).
fn jnum_opt(v: Option<&Json>, what: &str) -> Option<f64> {
    v.map(|v| jnum(Some(v), what))
}

/// `type(v)`.
fn lua_type(v: Option<&Json>) -> &'static str {
    match v {
        None | Some(Json::Null) => "nil",
        Some(Json::Bool(_)) => "boolean",
        Some(Json::Number(_)) => "number",
        Some(Json::String(_)) => "string",
        Some(Json::Array(_) | Json::Object(_)) => "table",
    }
}

/// `ipairs(v)` over a decoded value (a nil `v` is the Lua error).
fn jlist(v: Option<&Json>, what: &str) -> Vec<Json> {
    match v {
        Some(v) => json_list(v),
        None => lua_error!("bad argument #1 to 'ipairs' (table expected, got nil) ({what})"),
    }
}

/// `t[i]` of a decoded list (1-based).
fn jindex(v: Option<&Json>, i: usize) -> Option<&Json> {
    match v {
        Some(Json::Array(a)) => a.get(i.checked_sub(1)?).filter(|x| !x.is_null()),
        Some(Json::Object(o)) => o.get(&i.to_string()).filter(|x| !x.is_null()),
        Some(_) => lua_error!("attempt to index a non-table value"),
        None => lua_error!("attempt to index a nil value"),
    }
}

/// `t.k = v` on a decoded table (nil removes the key).
fn jset(t: &mut Json, k: &str, v: Option<Json>) {
    let Json::Object(o) = t else {
        lua_error!("attempt to index a {} value", lua_type(Some(t)))
    };
    match v {
        Some(v) if !v.is_null() => {
            o.insert(k.to_owned(), v);
        }
        _ => {
            o.remove(k);
        }
    }
}

/// A calc-side value as decoded JSON (`responseLua.bandit_choice = .. or configTab.input.bandit`).
fn val_to_json(v: &Val) -> Option<Json> {
    match v {
        Val::Nil => None,
        Val::Bool(b) => Some(Json::Bool(*b)),
        Val::Num(x) => serde_json::Number::from_f64(*x).map(Json::Number),
        Val::Str(s) => Some(Json::String(s.to_string())),
        _ => lua_error!("CCR-0117: a table value in a decoded JSON field"),
    }
}

/// A decoded JSON value as a calc table value (`spec.jewel_data[id] = value`).
fn json_to_val(v: &Json) -> Val {
    match v {
        Json::Null => Val::Nil,
        Json::Bool(b) => Val::Bool(*b),
        Json::Number(x) => Val::Num(x.as_f64().unwrap_or(f64::NAN)),
        Json::String(s) => Val::Str(LStr::from(s.as_str())),
        Json::Array(a) => {
            let t = Table::new();
            for (i, x) in a.iter().enumerate() {
                t.set(i as i64 + 1, json_to_val(x));
            }
            Val::Table(t)
        }
        Json::Object(o) => {
            let t = Table::new();
            // LUA-ORDER (dkjson inserts in document order; the table's pairs order is PoB's)
            for (k, x) in o {
                t.set(Name::from(k.as_str()), json_to_val(x));
            }
            Val::Table(t)
        }
    }
}

/// `tostring(v)` of a decoded value (EditControl:SetText).
fn jtostring(v: Option<&Json>) -> String {
    match v {
        None | Some(Json::Null) => "nil".to_owned(),
        Some(Json::Bool(b)) => b.to_string(),
        Some(Json::Number(x)) => pob_lua::tostring_num(x.as_f64().unwrap_or(f64::NAN)),
        Some(Json::String(s)) => s.clone(),
        Some(_) => "table".to_owned(),
    }
}

/// `errMsg:gsub("\n", " ")` (one value: inside a concatenation).
fn err_line(err: &str) -> String {
    pob_lua::gsub(err, "\n", " ", None).0
}

/// `self.controls.<key>`
fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `self.controls.<key>.buf`
fn buf(cx: &Cx, me: CtrlId, key: &'static str) -> String {
    cx.w::<EditControl>(ctl(cx, me, key)).buf.clone()
}

/// `self.controls.<key>.state`
fn check_state(cx: &Cx, me: CtrlId, key: &'static str) -> bool {
    cx.w::<CheckBoxControl>(ctl(cx, me, key)).state
}

/// `charSelect.list[charSelect.selIndex].char`
fn selected_char(cx: &Cx, dropdown: CtrlId) -> Json {
    let w = cx.w::<DropDownControl>(dropdown);
    let entry = w
        .sel_index
        .checked_sub(1)
        .and_then(|i| w.list.get(i))
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    entry
        .get::<CharEntry>()
        .map(|e| e.char.clone())
        .unwrap_or_else(|| lua_error!("attempt to index field 'char' (a nil value)"))
}

/// `dropdown:GetSelValueByKey("league")` of `siteCharSelectLeague`.
fn sel_league(cx: &Cx, dropdown: CtrlId) -> Option<String> {
    DropDownControl::get_sel_value_by_key(cx, dropdown, |v| {
        v.get::<LeagueEntry>().and_then(|e| e.league.clone())
    })
}

// ---- Common.lua helpers (first consumer, ui doc §12 note 15) --------------------------------

/// lua: Common.lua:216-230 codePointToUTF8(codePoint)
/// @pob Modules/Common.lua:216-230 h=84282797
pub fn code_point_to_utf8(code_point: f64) -> Vec<u8> {
    let c = code_point as u32;
    // L217
    if (0xD800 as f64..=0xDFFF as f64).contains(&code_point) {
        // L218
        b"?".to_vec()
    // L219
    } else if code_point <= 0x7F as f64 {
        // L220
        vec![c as u8]
    // L221
    } else if code_point <= 0x07FF as f64 {
        // L222
        vec![(0xC0 + (c >> 6)) as u8, (0x80 + (c & 0x3F)) as u8]
    // L223
    } else if code_point <= 0xFFFF as f64 {
        // L224
        vec![
            (0xE0 + (c >> 12)) as u8,
            (0x80 + ((c >> 6) & 0x3F)) as u8,
            (0x80 + (c & 0x3F)) as u8,
        ]
    // L225
    } else if code_point <= 0x10FFFF as f64 {
        // L226
        vec![
            (0xF0 + (c >> 18)) as u8,
            (0x80 + ((c >> 12) & 0x3F)) as u8,
            (0x80 + ((c >> 6) & 0x3F)) as u8,
            (0x80 + (c & 0x3F)) as u8,
        ]
    } else {
        // L228
        b"?".to_vec()
    }
}

/// lua: Common.lua:407-410 jsonToLua(json)
/// @pob Modules/Common.lua:407-410 h=21d853ee
pub fn json_to_lua(json: &str) -> Vec<u8> {
    use pob_lua::pattern::{gsub_bytes_with, Repl};
    let step = |s: &[u8], p: &str, r: &str| {
        gsub_bytes_with(s, p.as_bytes(), Repl::Str(r.as_bytes()), None).0
    };
    // L408
    let s = step(json.as_bytes(), "%[", "{");
    let s = step(&s, "%]", "}");
    let s = step(&s, "\"(%d[%d%.]*)\":", "[%1]=");
    let s = step(&s, "\"([^\"]+)\":", "[\"%1\"]=");
    let s = step(&s, "\\/", "/");
    let s = step(&s, "{(%w+)}", "{[0]=%1}");
    // L409
    let mut f = |c: &pob_lua::pattern::Captures<'_>| {
        let hex = c
            .first()
            .map(|c| c.to_str().into_owned())
            .unwrap_or_default();
        Some(code_point_to_utf8(
            pob_lua::tonumber_base(&hex, 16).unwrap_or(f64::NAN),
        ))
    };
    gsub_bytes_with(&s, b"\\u(%x%x%x%x)", Repl::Func(&mut f), None).0
}

/// A Lua value of the evaluated constructor (keys of tables in constructor order).
enum LVal {
    Nil,
    Bool(bool),
    Num(f64),
    Str(Vec<u8>),
    Table(Vec<(LKey, LVal)>),
}

#[derive(Clone, PartialEq)]
enum LKey {
    Num(f64),
    Str(Vec<u8>),
    Bool(bool),
}

/// `func()` of `loadstring("return " .. text)` with `setfenv(func, {})`: a recursive-descent
/// evaluator for the expressions `jsonToLua` produces (module docs). An error is the
/// `loadstring` message (or the runtime error) PoB would report.
struct LuaEval<'a> {
    s: &'a [u8],
    i: usize,
}

impl LuaEval<'_> {
    fn err<T>(&self, what: &str) -> Result<T, String> {
        Err(format!(
            "[string \"return ...\"]:1: {what} near byte {}",
            self.i + 1
        ))
    }
    fn skip_ws(&mut self) {
        loop {
            while self.i < self.s.len() && self.s[self.i].is_ascii_whitespace() {
                self.i += 1;
            }
            if self.s[self.i..].starts_with(b"--") {
                while self.i < self.s.len() && self.s[self.i] != b'\n' {
                    self.i += 1;
                }
            } else {
                return;
            }
        }
    }
    fn peek(&mut self) -> Option<u8> {
        self.skip_ws();
        self.s.get(self.i).copied()
    }
    fn expect(&mut self, c: u8) -> Result<(), String> {
        if self.peek() == Some(c) {
            self.i += 1;
            Ok(())
        } else {
            self.err(&format!("'{}' expected", c as char))
        }
    }
    fn name(&mut self) -> Option<Vec<u8>> {
        self.skip_ws();
        let st = self.i;
        if self
            .s
            .get(self.i)
            .is_some_and(|c| c.is_ascii_alphabetic() || *c == b'_')
        {
            while self
                .s
                .get(self.i)
                .is_some_and(|c| c.is_ascii_alphanumeric() || *c == b'_')
            {
                self.i += 1;
            }
            Some(self.s[st..self.i].to_vec())
        } else {
            None
        }
    }
    fn number(&mut self) -> Result<f64, String> {
        let st = self.i;
        if self.s[self.i..].starts_with(b"0x") || self.s[self.i..].starts_with(b"0X") {
            self.i += 2;
            while self.s.get(self.i).is_some_and(u8::is_ascii_hexdigit) {
                self.i += 1;
            }
        } else {
            while let Some(&c) = self.s.get(self.i) {
                let exp_sign = (c == b'+' || c == b'-')
                    && matches!(self.s.get(self.i.wrapping_sub(1)), Some(b'e' | b'E'));
                if c.is_ascii_alphanumeric() || c == b'.' || exp_sign {
                    self.i += 1;
                } else {
                    break;
                }
            }
        }
        let lex = String::from_utf8_lossy(&self.s[st..self.i]).into_owned();
        pob_lua::tonumber(&lex).map_or_else(|| self.err("malformed number"), Ok)
    }
    fn string(&mut self, q: u8) -> Result<Vec<u8>, String> {
        self.i += 1;
        let mut out = Vec::new();
        loop {
            let Some(&c) = self.s.get(self.i) else {
                return self.err("unfinished string");
            };
            self.i += 1;
            match c {
                b'\n' | b'\r' => return self.err("unfinished string"),
                c if c == q => return Ok(out),
                b'\\' => {
                    let Some(&e) = self.s.get(self.i) else {
                        return self.err("unfinished string");
                    };
                    self.i += 1;
                    match e {
                        b'a' => out.push(7),
                        b'b' => out.push(8),
                        b'f' => out.push(12),
                        b'n' => out.push(b'\n'),
                        b'r' => out.push(b'\r'),
                        b't' => out.push(b'\t'),
                        b'v' => out.push(11),
                        b'\\' | b'"' | b'\'' => out.push(e),
                        b'\n' | b'\r' => out.push(b'\n'),
                        b'z' => {
                            while self.s.get(self.i).is_some_and(u8::is_ascii_whitespace) {
                                self.i += 1;
                            }
                        }
                        b'x' => {
                            let h = self.s.get(self.i..self.i + 2).unwrap_or_default();
                            match std::str::from_utf8(h)
                                .ok()
                                .and_then(|h| u8::from_str_radix(h, 16).ok())
                            {
                                Some(b) if h.len() == 2 => {
                                    self.i += 2;
                                    out.push(b);
                                }
                                _ => return self.err("invalid escape sequence"),
                            }
                        }
                        b'u' => {
                            if self.s.get(self.i) != Some(&b'{') {
                                return self.err("invalid escape sequence");
                            }
                            let st = self.i + 1;
                            let mut j = st;
                            while self.s.get(j).is_some_and(u8::is_ascii_hexdigit) {
                                j += 1;
                            }
                            if j == st || self.s.get(j) != Some(&b'}') {
                                return self.err("invalid escape sequence");
                            }
                            let cp = u32::from_str_radix(
                                std::str::from_utf8(&self.s[st..j]).unwrap_or("0"),
                                16,
                            )
                            .unwrap_or(u32::MAX);
                            if cp > 0x10FFFF {
                                return self.err("invalid escape sequence");
                            }
                            self.i = j + 1;
                            out.extend(code_point_to_utf8(f64::from(cp)));
                        }
                        d if d.is_ascii_digit() => {
                            let mut v = u32::from(d - b'0');
                            for _ in 0..2 {
                                match self.s.get(self.i) {
                                    Some(&d) if d.is_ascii_digit() => {
                                        v = v * 10 + u32::from(d - b'0');
                                        self.i += 1;
                                    }
                                    _ => break,
                                }
                            }
                            if v > 255 {
                                return self.err("invalid escape sequence");
                            }
                            out.push(v as u8);
                        }
                        _ => return self.err("invalid escape sequence"),
                    }
                }
                c => out.push(c),
            }
        }
    }
    fn expr(&mut self) -> Result<LVal, String> {
        match self.peek() {
            Some(b'{') => self.table(),
            Some(q @ (b'"' | b'\'')) => self.string(q).map(LVal::Str),
            Some(b'-') => {
                self.i += 1;
                match self.expr()? {
                    LVal::Num(x) => Ok(LVal::Num(-x)),
                    LVal::Str(s) => match pob_lua::tonumber(&String::from_utf8_lossy(&s)) {
                        Some(x) => Ok(LVal::Num(-x)),
                        None => Err("attempt to perform arithmetic on a string value".to_owned()),
                    },
                    LVal::Nil => Err("attempt to perform arithmetic on a nil value".to_owned()),
                    LVal::Bool(_) => {
                        Err("attempt to perform arithmetic on a boolean value".to_owned())
                    }
                    LVal::Table(_) => {
                        Err("attempt to perform arithmetic on a table value".to_owned())
                    }
                }
            }
            Some(c) if c.is_ascii_digit() || c == b'.' => self.number().map(LVal::Num),
            Some(_) => match self.name() {
                Some(n) if n == b"true" => Ok(LVal::Bool(true)),
                Some(n) if n == b"false" => Ok(LVal::Bool(false)),
                // `nil`, or a global of the empty environment
                Some(_) => Ok(LVal::Nil),
                None => self.err("unexpected symbol"),
            },
            None => self.err("unexpected symbol"),
        }
    }
    fn table(&mut self) -> Result<LVal, String> {
        self.expect(b'{')?;
        let mut fields: Vec<(LKey, LVal)> = Vec::new();
        let mut n = 0.0;
        let store = |fields: &mut Vec<(LKey, LVal)>, k: LKey, v: LVal| {
            fields.retain(|(x, _)| *x != k);
            if !matches!(v, LVal::Nil) {
                fields.push((k, v));
            }
        };
        loop {
            match self.peek() {
                Some(b'}') => {
                    self.i += 1;
                    return Ok(LVal::Table(fields));
                }
                Some(b'[') => {
                    self.i += 1;
                    let k = self.expr()?;
                    self.expect(b']')?;
                    self.expect(b'=')?;
                    let v = self.expr()?;
                    let k = match k {
                        LVal::Num(x) if x.is_nan() => return Err("table index is NaN".to_owned()),
                        LVal::Num(x) => LKey::Num(x),
                        LVal::Str(s) => LKey::Str(s),
                        LVal::Bool(b) => LKey::Bool(b),
                        LVal::Nil => return Err("table index is nil".to_owned()),
                        LVal::Table(_) => {
                            return Err("CCR-0117: a table key in a site response".to_owned())
                        }
                    };
                    store(&mut fields, k, v);
                }
                _ => {
                    let save = self.i;
                    let named = self.name().filter(|_| self.peek() == Some(b'='));
                    if let Some(name) =
                        named.filter(|n| n != b"true" && n != b"false" && n != b"nil")
                    {
                        self.i += 1;
                        let v = self.expr()?;
                        store(&mut fields, LKey::Str(name), v);
                    } else {
                        self.i = save;
                        let v = self.expr()?;
                        n += 1.0;
                        store(&mut fields, LKey::Num(n), v);
                    }
                }
            }
            match self.peek() {
                Some(b',' | b';') => self.i += 1,
                Some(b'}') => {}
                _ => return self.err("'}' expected"),
            }
        }
    }
}

/// An evaluated value as decoded JSON (module docs: `1..n` tables are arrays).
fn lval_to_json(v: LVal) -> Json {
    match v {
        LVal::Nil => Json::Null,
        LVal::Bool(b) => Json::Bool(b),
        LVal::Num(x) => serde_json::Number::from_f64(x).map_or(Json::Null, Json::Number),
        LVal::Str(s) => Json::String(String::from_utf8_lossy(&s).into_owned()),
        LVal::Table(fields) => {
            let mut seq: Vec<Option<Json>> = vec![None; fields.len()];
            let is_seq = fields.iter().all(|(k, _)| {
                matches!(k, LKey::Num(x) if x.fract() == 0.0 && *x >= 1.0 && *x <= fields.len() as f64)
            });
            if is_seq && !fields.is_empty() {
                for (k, v) in fields {
                    if let LKey::Num(x) = k {
                        seq[x as usize - 1] = Some(lval_to_json(v));
                    }
                }
                Json::Array(seq.into_iter().map(|x| x.unwrap_or(Json::Null)).collect())
            } else if fields.is_empty() {
                Json::Array(Vec::new())
            } else {
                let mut o = serde_json::Map::new();
                for (k, v) in fields {
                    let k = match k {
                        LKey::Num(x) => pob_lua::tostring_num(x),
                        LKey::Str(s) => String::from_utf8_lossy(&s).into_owned(),
                        LKey::Bool(b) => b.to_string(),
                    };
                    o.insert(k, lval_to_json(v));
                }
                Json::Object(o)
            }
        }
    }
}

/// `loadstring("return " .. text)` + `setfenv(func, {})` + `func()`.
pub fn eval_lua_table(text: &[u8]) -> Result<Json, String> {
    let mut e = LuaEval { s: text, i: 0 };
    let v = e.expr()?;
    if e.peek().is_some() {
        return e.err("'<eof>' expected");
    }
    Ok(match v {
        LVal::Table(_) => lval_to_json(v),
        LVal::Nil => Json::Null,
        other => lval_to_json(other),
    })
}

// ---- ImportTab.lua:789-1932 --------------------------------------------------------------------

/// lua: ImportTab.lua:789-800 `ImportTabClass:ProcessSiteJSON(json)`
/// @pob Classes/ImportTab.lua:789-800 h=9f0b63b3
pub fn process_site_json(json: &str) -> Result<Json, String> {
    // L790-793
    let data = eval_lua_table(&json_to_lua(json))?;
    // L794 Sandbox the function just in case
    // L795
    // L796
    if !matches!(data, Json::Array(_) | Json::Object(_)) {
        // L797
        return Err("Return type is not a table".to_owned());
    }
    // L799
    Ok(data)
}

/// lua: ImportTab.lua:802-812 `ImportTabClass:SaveAccountHistory()`
/// @pob Classes/ImportTab.lua:802-812 h=efa9898a
pub fn save_account_history(cx: &mut Cx, me: CtrlId) {
    let name = buf(cx, me, "siteAccountName");
    // L803
    if !main_extra(cx, |m| {
        m.history_list
            .as_ref()
            .is_some_and(|h| h.set.contains(&name))
    }) {
        let list = main_extra_mut(cx, |m| {
            let h = m.history_list.get_or_insert_with(Default::default);
            // L804
            h.list.push(name.clone());
            // L805
            h.set.insert(name.clone());
            // L806-808
            pob_lua::sort(&mut h.list, |a, b| {
                pob_lua::string::lower(a) < pob_lua::string::lower(b)
            });
            h.list.clone()
        });
        // the dropdown's list is the global historyList (DIVERGENCE(alias), accounts.rs)
        let hist = ctl(cx, me, "siteAccountHistory");
        cx.w_mut::<DropDownControl>(hist).list = list.into_iter().map(DropItem::str).collect();
        // L809
        DropDownControl::check_dropped_width(cx, hist, true);
        // L810
        DropDownControl::sel_by_value(cx, hist, &|it| it.as_str() == Some(name.as_str()));
    }
}

/// lua: ImportTab.lua:814-850 `ImportTabClass:DownloadPassiveTree(realm)`
/// @pob Classes/ImportTab.lua:814-850 h=46b5e8a0
pub fn download_passive_tree(cx: &mut Cx, me: CtrlId, realm: &'static Realm) {
    {
        let s = st_mut(cx, me);
        // L815
        s.char_import_mode = "IMPORTING".to_owned();
        // L816
        s.char_import_status = "Retrieving character passive tree...".to_owned();
    }
    // L817
    let account_name = buf(cx, me, "siteAccountName");
    // L818
    let char_select = ctl(cx, me, "siteCharSelect");
    // L819
    let char_list_data = selected_char(cx, char_select);
    let char_name = jstr(json_get(&char_list_data, "name"), "name");
    // L820-823
    let url = format!(
        "{}character-window/get-passive-skills?accountName={}&character={}&realm={}",
        realm.host_name,
        pob_lua::gsub(&account_name, "#", "%%23", None).0,
        url_encode(&char_name),
        realm.realm_code
    );
    launch_download(
        cx,
        &url,
        Rc::new(
            move |cx: &mut Cx, response: PageResponse, err_msg: Option<String>| {
                if !alive(cx, me) {
                    // DIVERGENCE(dead-tab)
                    return;
                }
                let neg = cx.main.cc.NEGATIVE.clone();
                // L825
                st_mut(cx, me).char_import_mode = "SELECTCHAR".to_owned();
                // L826
                if let Some(err) = err_msg {
                    // L827-828
                    st_mut(cx, me).char_import_status = format!(
                        "{neg}Error importing character data, try again ({})",
                        err_line(&err)
                    );
                    // L829
                    return;
                // L830
                } else if response.body.as_deref() == Some("false") {
                    // L831
                    st_mut(cx, me).char_import_status =
                        format!("{neg}Failed to retrieve character data, try again.");
                    // L832
                    return;
                }
                // L834
                st_mut(cx, me).last_character_hash = Some(sha1_hex(&char_name));
                // L835
                if st(cx, me).last_league.is_none() {
                    // L836
                    let league = sel_league(cx, ctl(cx, me, "siteCharSelectLeague"));
                    st_mut(cx, me).last_league = league;
                }
                // L838
                let mut response_lua = decode_body(&response);
                // Account-name imports omit quest choices, so keep the build's current values.
                let input = cx.build().model.configTab.input.clone();
                for (k, var) in [
                    // L840
                    ("bandit_choice", "bandit"),
                    // L841
                    ("pantheon_major", "pantheonMajorGod"),
                    // L842
                    ("pantheon_minor", "pantheonMinorGod"),
                ] {
                    let a = json_get(&response_lua, k);
                    let v = if json_truthy(a) {
                        a.cloned()
                    } else {
                        val_to_json(&input.get(Name::from(var)))
                    };
                    jset(&mut response_lua, k, v);
                }
                // modify response to be like the oauth API response
                // L844
                let mut char_data = char_list_data.clone();
                // L845
                let jewels = json_get(&response_lua, "items").cloned();
                jset(&mut char_data, "passives", Some(response_lua));
                // L846
                jset(&mut char_data, "jewels", jewels);
                // L847
                let delete_jewels = check_state(cx, me, "siteCharImportTreeClearJewels");
                // L848
                import_passive_tree_and_jewels(cx, me, &char_data, delete_jewels);
            },
        ),
    );
}

/// `dkjson.decode(response.body)`, indexed right after (nil is the Lua index error).
fn decode_body(response: &PageResponse) -> Json {
    response
        .body
        .as_deref()
        .and_then(dkjson_decode)
        .filter(|v| matches!(v, Json::Object(_) | Json::Array(_)))
        .unwrap_or_else(|| lua_error!("attempt to index local 'responseLua' (a nil value)"))
}

/// `launch:DownloadPage(url, callback)`.
fn launch_download(
    cx: &mut Cx,
    url: &str,
    callback: Rc<dyn Fn(&mut Cx, PageResponse, Option<String>)>,
) {
    download_page(cx, url, callback, None);
}

/// lua: ImportTab.lua:852-886 `ImportTabClass:DownloadItems(realm)`
/// @pob Classes/ImportTab.lua:852-886 h=ab5b2a7b
pub fn download_items(cx: &mut Cx, me: CtrlId, realm: &'static Realm) {
    {
        let s = st_mut(cx, me);
        // L853
        s.char_import_mode = "IMPORTING".to_owned();
        // L854
        s.char_import_status = "Retrieving character items...".to_owned();
    }
    // L855
    let account_name = buf(cx, me, "siteAccountName");
    // L856
    let char_select = ctl(cx, me, "siteCharSelect");
    // L857
    let char_list_data = selected_char(cx, char_select);
    let char_name = jstr(json_get(&char_list_data, "name"), "name");
    // L858-861
    let url = format!(
        "{}character-window/get-items?accountName={}&character={}&realm={}",
        realm.host_name,
        pob_lua::gsub(&account_name, "#", "%%23", None).0,
        url_encode(&char_name),
        realm.realm_code
    );
    launch_download(
        cx,
        &url,
        Rc::new(
            move |cx: &mut Cx, response: PageResponse, err_msg: Option<String>| {
                if !alive(cx, me) {
                    // DIVERGENCE(dead-tab)
                    return;
                }
                let neg = cx.main.cc.NEGATIVE.clone();
                // L863
                st_mut(cx, me).char_import_mode = "SELECTCHAR".to_owned();
                // L864
                if let Some(err) = err_msg {
                    // L865-866
                    st_mut(cx, me).char_import_status = format!(
                        "{neg}Error importing character data, try again ({})",
                        err_line(&err)
                    );
                    // L867
                    return;
                // L868
                } else if response.body.as_deref() == Some("false") {
                    // L869
                    st_mut(cx, me).char_import_status =
                        format!("{neg}Failed to retrieve character data, try again.");
                    // L870
                    return;
                }
                // L872
                st_mut(cx, me).last_character_hash = Some(sha1_hex(&char_name));
                // L873
                if st(cx, me).last_league.is_none() {
                    // L874
                    let league = sel_league(cx, ctl(cx, me, "siteCharSelectLeague"));
                    st_mut(cx, me).last_league = league;
                }
                // L876
                let response_lua = decode_body(&response);
                // modify response to be like the oauth API response
                // L878
                let mut char_data = char_list_data.clone();
                // L879
                jset(
                    &mut char_data,
                    "equipment",
                    json_get(&response_lua, "items").cloned(),
                );
                // L880
                jset(
                    &mut char_data,
                    "guardian",
                    json_get(&response_lua, "guardian").cloned(),
                );
                // L881
                let clear_items = check_state(cx, me, "siteCharImportItemsClearItems");
                // L882
                let clear_skills = check_state(cx, me, "siteCharImportItemsClearSkills");
                // L883
                let ignore_weapon_swap = check_state(cx, me, "siteCharImportItemsIgnoreWeaponSwap");
                // L884
                import_items_and_skills(
                    cx,
                    me,
                    &char_data,
                    clear_items,
                    clear_skills,
                    ignore_weapon_swap,
                );
            },
        ),
    );
}

/// `FindMatchingStandardLeague(league)` (ImportTab.lua:888-903, a global the method defines).
fn find_matching_standard_league(league: &str) -> &'static str {
    // Find a Standard league name for a given league name
    // Reference https://api.pathofexile.com/league?realm=pc
    // L891
    if pob_lua::find(league, "Hardcore", None, false).is_some() {
        // L892
        "Hardcore"
    // L893
    } else if pob_lua::find(league, "HC SSF", None, false).is_some() {
        // includes Ruthless "HC SSF R "
        // L895
        "SSF Hardcore"
    // L896
    } else if pob_lua::find(league, "SSF", None, false).is_some() {
        // Any non HardCore SSF's - includes Ruthless "SSF R "
        // L898
        "SSF Standard"
    } else {
        // normal league and ruthless league (Sanctum, Ruthless Sanctum)
        // L901
        "Standard"
    }
}

/// lua: ImportTab.lua:887-1021 `ImportTabClass:DownloadSiteCharacterList(realm)`
/// @pob Classes/ImportTab.lua:887-1021 h=405aa290
pub fn download_site_character_list(cx: &mut Cx, me: CtrlId, realm: &'static Realm) {
    // L888-903 FindMatchingStandardLeague: find_matching_standard_league
    {
        let s = st_mut(cx, me);
        // L905
        s.char_import_mode = "DOWNLOADCHARLIST".to_owned();
        // L906
        s.char_import_status = "Retrieving character list...".to_owned();
    }
    // L907-913 Handle spaces in the account name
    let b = buf(cx, me, "siteAccountName");
    let account_name = if realm.realm_code == "pc" {
        // L910
        pob_lua::gsub(&b, "%s+", "", None).0
    } else {
        // L912
        let a = pob_lua::gsub(&b, "^[%s?]+", "", None).0;
        let a = pob_lua::gsub(&a, "[%s?]+$", "", None).0;
        pob_lua::gsub(&a, "%s", "+", None).0
    };
    // L914
    let account_name = pob_lua::gsub(&account_name, "(.*)[#%-]", "%1#", None).0;
    // L915-917
    let url = format!(
        "{}character-window/get-characters?accountName={}&realm={}",
        realm.host_name,
        pob_lua::gsub(&account_name, "#", "%%23", None).0,
        realm.realm_code
    );
    launch_download(
        cx,
        &url,
        Rc::new(
            move |cx: &mut Cx, response: PageResponse, err_msg: Option<String>| {
                if !alive(cx, me) {
                    // DIVERGENCE(dead-tab)
                    return;
                }
                let neg = cx.main.cc.NEGATIVE.clone();
                let fail = |cx: &mut Cx, status: String| {
                    let s = st_mut(cx, me);
                    s.char_import_status = status;
                    s.char_import_mode = "GETACCOUNTNAME".to_owned();
                };
                match err_msg.as_deref() {
                    // L919
                    Some("Response code: 401" | "Response code: 403") => {
                        // L920-922
                        fail(
                            cx,
                            format!("{neg}Account profile is private or does not exist."),
                        );
                        return;
                    }
                    // L923
                    Some("Response code: 404") => {
                        // L924-926
                        fail(cx, format!("{neg}Account name is incorrect."));
                        return;
                    }
                    // L927
                    Some(err) => {
                        // L928-931
                        fail(
                            cx,
                            format!(
                                "{neg}Error retrieving character list, try again ({})",
                                err_line(err)
                            ),
                        );
                        return;
                    }
                    None => {}
                }
                // L933
                let Ok(char_list) =
                    process_site_json(response.body.as_deref().unwrap_or_else(|| {
                        lua_error!("attempt to concatenate local 'json' (a nil value)")
                    }))
                else {
                    // L934-937
                    fail(
                        cx,
                        format!("{neg}Error processing character list, try again later"),
                    );
                    return;
                };
                //ConPrintTable(charList)
                // L940
                let char_list_len = match &char_list {
                    Json::Array(a) => a.len(),
                    _ => 0,
                };
                if char_list_len == 0 {
                    // L941-943
                    fail(cx, format!("{neg}The account has no characters to import."));
                    return;
                }
                // GGG's character API has an issue where for /get-characters the account name is not
                // case-sensitive, but for /get-passive-skills and /get-items it is.
                // This workaround grabs the profile page and extracts the correct account name from
                // one of the URLs.
                // L947
                let url = format!(
                    "{}{}{}",
                    realm.host_name,
                    realm.profile_url,
                    pob_lua::gsub(&account_name, "#", "%%23", None).0
                );
                let char_list = Rc::new(char_list);
                launch_download(
                    cx,
                    &url,
                    Rc::new(
                        move |cx: &mut Cx, response: PageResponse, err_msg: Option<String>| {
                            profile_page_done(cx, me, realm, &char_list, &response, err_msg);
                        },
                    ),
                );
            },
        ),
    );
}

/// ImportTab.lua:948-1019: the profile page callback of DownloadSiteCharacterList.
fn profile_page_done(
    cx: &mut Cx,
    me: CtrlId,
    realm: &'static Realm,
    char_list: &Json,
    response: &PageResponse,
    err_msg: Option<String>,
) {
    let tab = alive(cx, me);
    let neg = cx.main.cc.NEGATIVE.clone();
    // L949
    if let Some(err) = err_msg {
        if tab {
            // L950-952
            let s = st_mut(cx, me);
            s.char_import_status = format!(
                "{neg}Error retrieving character list, try again ({})",
                err_line(&err)
            );
            s.char_import_mode = "GETACCOUNTNAME".to_owned();
        }
        // L953
        return;
    }
    // L955-957
    let real_account_name = response.body.as_deref().map(|body| {
        let m = lmatch(body, "/view%-profile/([^/]+)/characters")
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
        pob_lua::gsub_fn(
            &m,
            ".",
            |c| {
                let b = c.first().map_or(0, |c| c.as_bytes()[0]);
                (b > 127).then(|| lfmt!("%%%2X", f64::from(b)))
            },
            None,
        )
        .0
    });
    // L958
    let Some(real_account_name) = real_account_name else {
        if tab {
            // L959-960
            let s = st_mut(cx, me);
            s.char_import_status = format!("{neg}Failed to retrieve character list.");
            s.char_import_mode = "GETACCOUNTNAME".to_owned();
        }
        // L961
        return;
    };
    // L963
    let real_account_name = pob_lua::gsub(&real_account_name, "(.*)[#%-]", "%1#", None).0;
    // L964
    let account_name = real_account_name.clone();
    if tab {
        // L965
        let name_ctl = ctl(cx, me, "siteAccountName");
        EditControl::set_text(cx, name_ctl, &real_account_name, false);
        let s = st_mut(cx, me);
        // L966
        s.char_import_status = "Character list successfully retrieved.".to_owned();
        // L967
        s.char_import_mode = "SELECTCHAR".to_owned();
        // L968
        s.last_realm = Some(realm.id.to_owned());
    }
    // L969
    cx.app_mut().main.last_realm = Some(realm.id.to_owned());
    if tab {
        // L970
        st_mut(cx, me).last_account_hash = Some(sha1_hex(&account_name));
    }
    // L971
    cx.app_mut().main.last_account_name = Some(account_name.clone());
    // L972-973 (`sessionID` is a nil global)
    let accounts = &mut cx.app_mut().main.game_accounts;
    accounts.entry(account_name.clone()).or_default().session_id = None;
    if !tab {
        // DIVERGENCE(dead-tab)
        return;
    }
    // L974
    let mut league_list: Vec<String> = Vec::new();
    let chars = json_list(char_list);
    // L975
    for ch in &chars {
        // L976 (`t_insert(leagueList, nil)` appends nothing)
        if let Some(league) = json_get(ch, "league") {
            let league = jstr(Some(league), "league");
            if !league_list.contains(&league) {
                // L977
                league_list.push(league);
            }
        }
    }
    // L980
    pob_lua::sort::sort_default(&mut league_list);
    // L981
    let char_select_league = ctl(cx, me, "siteCharSelectLeague");
    // L982-988 (the list is wiped and filled in place: no SetList)
    let mut list: Vec<DropItem> = league_list
        .iter()
        .map(|l| {
            DropItem::new(
                l.clone(),
                Rc::new(LeagueEntry {
                    league: Some(l.clone()),
                }),
            )
        })
        .collect();
    // L989-991
    list.push(DropItem::new("All", Rc::new(LeagueEntry { league: None })));
    cx.w_mut::<DropDownControl>(char_select_league).list = list;
    // set the league combo to the last used if possible, used for previously imported characters
    // L993
    let by_league = |l: String| {
        move |it: &DropItem| -> bool {
            it.get::<LeagueEntry>().and_then(|e| e.league.as_deref()) == Some(l.as_str())
        }
    };
    if let Some(last_league) = st(cx, me).last_league.clone() {
        // L994
        DropDownControl::sel_by_value(cx, char_select_league, &by_league(last_league.clone()));
        // check that it worked
        // L996
        if sel_league(cx, char_select_league).as_deref() != Some(last_league.as_str()) {
            // League maybe over, Character will be in standard
            // L998
            let standard_league_name = find_matching_standard_league(&last_league);
            // L999
            DropDownControl::sel_by_value(
                cx,
                char_select_league,
                &by_league(standard_league_name.to_owned()),
            );
            // L1000
            if sel_league(cx, char_select_league).as_deref() != Some(standard_league_name) {
                // give up and select the first entry. Ruthless mode may not have Standard equivalents
                // L1002
                cx.w_mut::<DropDownControl>(char_select_league).sel_index = 1;
            } else {
                // L1004
                st_mut(cx, me).last_league = Some(standard_league_name.to_owned());
            }
        }
    } else {
        // L1008
        let w = cx.w::<DropDownControl>(char_select_league);
        if w.sel_index > w.list.len() {
            // L1009
            cx.w_mut::<DropDownControl>(char_select_league).sel_index = 1;
        }
    }
    // L1012
    st_mut(cx, me).last_char_list = Some(chars.clone());
    // L1013-1015
    let league = sel_league(cx, char_select_league);
    let site_char_select = ctl(cx, me, "siteCharSelect");
    build_character_list(
        cx,
        me,
        realm.realm_code,
        league.as_deref(),
        Some(&chars),
        site_char_select,
    );

    // We only get here if the accountname was correct, found, and not private, so add it to the
    // account history.
    // L1018
    save_account_history(cx, me);
}

/// `colorCodes[name]` (nil for an unknown key).
fn color_code(cx: &Cx, name: &str) -> Option<String> {
    cx.main.cc.get(name).cloned()
}

/// lua: ImportTab.lua:1027-1095 `ImportTabClass:BuildCharacterList(realm, league, characters,
/// control)`
/// @pob Classes/ImportTab.lua:1027-1095 h=1b5ca8ea
pub fn build_character_list(
    cx: &mut Cx,
    me: CtrlId,
    realm: &str,
    league: Option<&str>,
    characters: Option<&[Json]>,
    control: CtrlId,
) {
    // L1028 (wiped in place: no SetList)
    cx.w_mut::<DropDownControl>(control).list.clear();
    // L1029
    let Some(characters) = characters else {
        // L1030
        return;
    };
    let mut list: Vec<DropItem> = Vec::new();
    // L1032 (`ipairs`: the list ends at the first nil)
    for ch in characters.iter().take_while(|c| !c.is_null()) {
        // L1033
        let char_realm = json_get(ch, "realm");
        let char_league = json_get(ch, "league");
        let realm_match = matches!(char_realm, Some(Json::String(r)) if r == realm);
        let league_match = league.map_or(
            true,
            |l| matches!(char_league, Some(Json::String(c)) if c == l),
        );
        if !(realm_match && league_match) {
            continue;
        }
        // L1034
        let char_lvl = json_get(ch, "level").map_or(0.0, |v| jnum(Some(v), "level"));
        // L1035
        let char_league = char_league.map_or_else(|| "?".to_owned(), |v| jstr(Some(v), "league"));
        // L1036
        let char_name =
            json_get(ch, "name").map_or_else(|| "?".to_owned(), |v| jstr(Some(v), "name"));
        // L1037
        let char_class =
            json_get(ch, "class").map_or_else(|| "?".to_owned(), |v| jstr(Some(v), "class"));

        // L1039 (`colorCodes.DEFAULT` is nil: Global.lua has no such key)
        let mut class_color = color_code(cx, "DEFAULT");
        // L1040
        if char_class != "?" {
            // L1041
            class_color = color_code(cx, &pob_lua::string::upper(&char_class));

            // L1043
            if class_color.is_none() {
                let c = char_class.as_str();
                // L1044-1045
                let group = if matches!(
                    c,
                    "Elementalist"
                        | "Necromancer"
                        | "Occultist"
                        | "Harbinger"
                        | "Herald"
                        | "Bog Shaman"
                ) {
                    // L1046
                    Some("WITCH")
                // L1047-1048
                } else if matches!(
                    c,
                    "Guardian"
                        | "Inquisitor"
                        | "Hierophant"
                        | "Architect of Chaos"
                        | "Polytheist"
                        | "Puppeteer"
                ) {
                    // L1049
                    Some("TEMPLAR")
                // L1050-1051
                } else if matches!(
                    c,
                    "Assassin"
                        | "Trickster"
                        | "Saboteur"
                        | "Surfcaster"
                        | "Servant of Arakaali"
                        | "Blind Prophet"
                ) {
                    // L1052
                    Some("SHADOW")
                // L1053-1054
                } else if matches!(
                    c,
                    "Gladiator" | "Slayer" | "Champion" | "Gambler" | "Paladin" | "Aristocrat"
                ) {
                    // L1055
                    Some("DUELIST")
                // L1056-1057
                } else if matches!(
                    c,
                    "Raider"
                        | "Pathfinder"
                        | "Deadeye"
                        | "Warden"
                        | "Daughter of Oshabi"
                        | "Whisperer"
                        | "Wildspeaker"
                ) {
                    // L1058
                    Some("RANGER")
                // L1059-1060
                } else if matches!(
                    c,
                    "Juggernaut"
                        | "Berserker"
                        | "Chieftain"
                        | "Antiquarian"
                        | "Behemoth"
                        | "Ancestral Commander"
                ) {
                    // L1061
                    Some("MARAUDER")
                // L1062-1063
                } else if matches!(c, "Ascendant" | "Reliquarian" | "Luminary" | "Scavenger") {
                    // L1064
                    Some("SCION")
                } else {
                    None
                };
                if let Some(g) = group {
                    class_color = color_code(cx, g);
                }
            }
        }

        // L1069-1074 (`%s` of a nil colour is "nil" in LuaJIT 2.1)
        let detail = if league.is_none() {
            // L1071
            lfmt!(
                "%s%s ^x808080lvl %d in %s",
                class_color.as_deref(),
                char_class.as_str(),
                char_lvl,
                char_league.as_str()
            )
        } else {
            // L1073
            lfmt!(
                "%s%s ^x808080lvl %d",
                class_color.as_deref(),
                char_class.as_str(),
                char_lvl
            )
        };
        // L1075-1080
        list.push(
            DropItem::new(char_name.clone(), Rc::new(CharEntry { char: ch.clone() }))
                .search_filter(format!("{char_name} {char_class}"))
                .detail(detail),
        );
    }
    // L1083-1085 (`a.char.name:lower()`: a nil name is the Lua error)
    let name_of = |it: &DropItem| -> String {
        let ch = &it.get::<CharEntry>().expect("CharEntry").char;
        pob_lua::string::lower(&jstr(json_get(ch, "name"), "name"))
    };
    pob_lua::sort(&mut list, |a, b| name_of(a) < name_of(b));
    // L1086
    let mut sel_index = 1;
    // L1087
    if let Some(hash) = st(cx, me).last_character_hash.clone() {
        // L1088
        for (i, it) in list.iter().enumerate() {
            let ch = &it.get::<CharEntry>().expect("CharEntry").char;
            // L1089
            if sha1_hex(&jstr(json_get(ch, "name"), "name")) == hash {
                // L1090
                sel_index = i + 1;
                // L1091
                break;
            }
        }
    }
    let w = cx.w_mut::<DropDownControl>(control);
    w.list = list;
    w.sel_index = sel_index;
}

/// `main.tree[treeVersion]` (the cached tree, loaded on demand as `main:LoadTree` does).
fn main_tree(cx: &mut Cx, version: Name) -> model::PassiveTreeRef {
    if let Some(t) = crate::build::class_drop::main_tree(cx, version) {
        return t;
    }
    let load_tree = cx.app().main.hooks.load_tree.clone();
    load_tree(cx, version.as_str());
    crate::build::class_drop::main_tree(cx, version)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// `isAscendancyInTree(className, treeVersion)` (ImportTab.lua:1169-1182).
fn is_ascendancy_in_tree(tree: &model::PassiveTreeRef, class_name: &str) -> bool {
    // L1170
    let classes = &tree.classes;
    // L1171 LUA-ORDER (the result is a boolean)
    for (_, class) in classes.pairs() {
        let Some(class) = class.table() else {
            lua_error!("attempt to index local 'class' (a non-table value)")
        };
        // L1172
        if class.str(n!("name")).as_deref() == Some(class_name) {
            // L1173
            return true;
        }
        let subs = class.table(n!("classes")).unwrap_or_else(|| {
            lua_error!("attempt to get length of field 'classes' (a nil value)")
        });
        // L1175
        for i in 0..=subs.len() as i64 {
            // L1176
            let ascend_class = subs.table(i).unwrap_or_else(|| {
                lua_error!("attempt to index local 'ascendClass' (a nil value)")
            });
            // L1177
            if ascend_class.str(n!("name")).as_deref() == Some(class_name) {
                // L1178
                return true;
            }
        }
    }
    false
}

/// `self.build.Act` (written by EstimatePlayerProgress).
fn build_act(cx: &Cx) -> Option<ActValue> {
    let me = cx.build().node.id();
    cx.w::<BuildMode>(me).on_frame.act.clone()
}

/// lua: ImportTab.lua:1131-1253 `ImportTabClass:ImportPassiveTreeAndJewels(charData,
/// deleteJewels)`
/// @pob Classes/ImportTab.lua:1131-1253 h=25686615
pub fn import_passive_tree_and_jewels(
    cx: &mut Cx,
    me: CtrlId,
    char_data: &Json,
    delete_jewels: bool,
) -> String {
    let m = cx.build().model.clone();
    let seams = tab_seams();
    // L1132
    let char_passives = json_get(char_data, "passives")
        .cloned()
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));

    // fix table keys being strings
    // L1135
    let mut masteries: LjIntMap<f64> = LjIntMap::new();
    // L1136 LUA-ORDER (string keys of a decoded object)
    if let Some(Json::Object(o)) = json_get(&char_passives, "mastery_effects") {
        for (key, value) in o {
            // L1137
            let k = pob_lua::tonumber(key).unwrap_or_else(|| lua_error!("table index is nil"));
            masteries.set(k as i64, Some(jnum(Some(value), "mastery_effects")));
        }
    }
    // L1139
    let jewel_data = Table::new();
    m.spec.get().extra.set(n!("jewel_data"), jewel_data.clone());
    // L1140 LUA-ORDER (string keys of a decoded object)
    if let Some(Json::Object(o)) = json_get(&char_passives, "jewel_data") {
        for (key, value) in o {
            // L1141
            let k = pob_lua::tonumber(key).unwrap_or_else(|| lua_error!("table index is nil"));
            jewel_data.set(Key::from(k), json_to_val(value));
        }
    }
    // L1143
    let mut skill_overrides: LjIntMap<NodeRef> = LjIntMap::new();
    // L1144 LUA-ORDER (string keys of a decoded object)
    if let Some(Json::Object(o)) = json_get(&char_passives, "skill_overrides") {
        for (node_id, over) in o {
            // json keys are strings, not numbers
            // L1146
            let node_id_num =
                pob_lua::tonumber(node_id).unwrap_or_else(|| lua_error!("table index is nil"));
            // L1147: the override table becomes a node (CCR-0117 §3)
            let spec = m.spec.get();
            let tattoo_nodes = spec
                .tree
                .get()
                .tattoo
                .get()
                .and_then(|t| t.table(n!("nodes")))
                .unwrap_or_else(|| lua_error!("attempt to index field 'tattoo' (a nil value)"));
            let name = jstr(json_get(over, "name"), "name");
            let new_node = tattoo_nodes
                .get(name.as_str())
                .obj::<model::Node>()
                .unwrap_or_else(|| lua_error!("attempt to index local 'newNode' (a nil value)"));
            // L1148 `override.id = nodeIdNum`
            let node: NodeRef = Rc::new(model::Node {
                id: node_id_num,
                ..model::Node::default()
            });
            for (k, v) in json_object(over) {
                if !matches!(
                    k.as_str(),
                    "name" | "icon" | "activeEffectImage" | "isTattoo" | "reminderText"
                ) {
                    node.extra.set(Name::from(k.as_str()), json_to_val(v));
                }
            }
            spec.ReplaceNode(&node, &new_node);
            // L1149
            skill_overrides.set(node_id_num as i64, Some(node));
        }
    }

    // L1152
    let hashes_ex = json_get(&char_passives, "hashes_ex")
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
    m.spec
        .get()
        .extra
        .set(n!("extended_hashes"), json_to_val(hashes_ex));
    // L1153
    if delete_jewels {
        // L1154 LUA-ORDER (string keys; the deleted set is the same in any order)
        let slots: Vec<Table> = m.itemsTab.slots.with(|s| s.values().cloned().collect());
        for slot in slots {
            // L1155
            let sel = slot.get(n!("selItemId"));
            if !sel.raw_eq(&Val::Num(0.0)) && slot.get(n!("nodeId")).truthy() {
                // L1156 ignore allocated cluster nodes on Import when Delete Jewel is true,
                // clean slate
                m.spec.get().extra.set(n!("ignoreAllocatingSubgraph"), true);
                // L1157
                let item = sel
                    .opt_num()
                    .and_then(model::items_tab::id_key)
                    .and_then(|k| m.itemsTab.items.with(|i| i.get(&k).cloned()))
                    .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
                (seams.items_delete_item)(cx, &item, false);
            }
        }
    }
    // L1161
    if let Some(jewels) = json_get(char_data, "jewels") {
        for item_data in json_list(jewels) {
            // L1162
            import_item(cx, &item_data, None, false, None);
        }
    }
    // L1164
    (seams.items_populate_slots)(cx);
    // L1165
    (seams.items_add_undo_state)(cx);

    // Alternate trees don't have an identifier, so we're forced to look up something that is
    // unique to that tree. Hopefully this changes, because it's totally unmaintainable
    // L1169-1182 isAscendancyInTree: is_ascendancy_in_tree

    // L1184
    let alternate_ascendancy_id = match json_get(&char_passives, "alternate_ascendancy") {
        // L1185-1186 oauth responses have bloodline names
        Some(Json::String(name)) => {
            // L1188
            let latest = m
                .latestTree
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'latestTree' (a nil value)"));
            let bloodline = latest.secondaryAscendNameMap.get(name.as_str());
            // L1189
            bloodline.get(n!("ascendClassId")).opt_num()
        }
        // L1190-1192 site responses have integer ids
        Some(v) if json_truthy(Some(v)) => Some(jnum(Some(v), "alternate_ascendancy")),
        // L1194-1195
        _ => Some(0.0),
    };
    // Character import uses current GGG cluster hashes.
    // L1198
    m.spec.get().extra.set(n!("clusterHashFormatVersion"), 2.0);
    // L1199
    let league = jstr(json_get(char_data, "league"), "league");
    let ruthless_suffix = if lmatch(&league, "Ruthless").is_some() {
        "_ruthless"
    } else {
        ""
    };
    // L1200
    let class = jstr(json_get(char_data, "class"), "class");
    let latest_tree_version = cx.data().gameVersions.latestTreeVersion;
    let latest_main_tree = main_tree(cx, latest_tree_version);
    let phrecia_suffix = if is_ascendancy_in_tree(&latest_main_tree, &class) {
        ""
    } else {
        "_alternate"
    };
    // L1201-1209
    let hashes: Vec<f64> = jlist(json_get(&char_passives, "hashes"), "hashes")
        .iter()
        .map(|h| jnum(Some(h), "hashes"))
        .collect();
    let effects: Vec<(f64, f64)> = masteries
        .pairs()
        .into_iter()
        .map(|(k, v)| (k as f64, v))
        .collect();
    m.spec.get().ImportFromNodeList_impl(
        Some(Name::from(class.as_str())),
        f64::NAN,
        f64::NAN,
        // a nil id (no such bloodline) is NaN (CCR-0117 §3)
        alternate_ascendancy_id.unwrap_or(f64::NAN),
        &hashes,
        Shared::new(skill_overrides),
        &effects,
        Some(Name::from(
            format!("{latest_tree_version}{ruthless_suffix}{phrecia_suffix}").as_str(),
        )),
    );
    // L1210
    let hooks = build_init::hooks(cx);
    (hooks.tree_set_active_spec)(cx, m.treeTab.activeSpec.get());
    // TreeTab.lua:556 `self.build.spec:SetWindowTitleWithBuildClass()` (the model queues the
    // class text; the UI sends it, init.rs `flush_window_titles`)
    build_init::flush_window_titles(cx);
    // L1211
    m.spec.get().BuildClusterJewelGraphs();
    // L1212
    (hooks.spec_add_undo_state)(cx);
    // L1213
    let level = json_get(char_data, "level");
    m.characterLevel
        .set(level.map_or(100.0, |l| jnum(Some(l), "level")));
    // L1214
    build_xml::build_extra(&m).set(n!("characterLevelAutoMode"), false);
    // L1215
    m.configTab.UpdateLevel(&m);
    // L1216
    let level_ctl = build_ctl(cx, "characterLevel");
    EditControl::set_text(cx, level_ctl, &jtostring(level), false);
    // L1217
    loadouts::estimate_player_progress(cx);
    // L1218
    let mut resistance_penalty_index = 3.0;
    // L1219 Estimate resistance penalty setting based on act progression estimate
    match build_act(cx) {
        // L1220
        Some(ActValue::Endgame) => resistance_penalty_index = 3.0,
        // L1221
        Some(ActValue::Act(act)) => {
            // L1222
            if act < 5.0 {
                resistance_penalty_index = 1.0;
            // L1223
            } else if act > 5.0 && act < 11.0 {
                resistance_penalty_index = 2.0;
            // L1224
            } else if act > 10.0 {
                resistance_penalty_index = 3.0;
            }
        }
        None => {}
    }
    // L1227
    (seams.config_var_set_sel)(cx, "resistancePenalty", resistance_penalty_index);

    // L1229-1235 setSelByVal(dropdown, val)
    let set_sel_by_val = |cx: &mut Cx, var: &str, val: &str| {
        let control = cx.build().model.configTab.varControl(Name::from(var));
        let list = control
            .table(n!("list"))
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
        // L1230
        for (i, v) in list.ipairs() {
            // L1231
            if v.get(n!("val")).raw_eq(&Val::from(val)) {
                // L1232
                (seams.config_var_set_sel)(cx, var, i as f64);
            }
        }
    };

    // L1237-1238
    let bandit = match json_get(&char_passives, "bandit_choice") {
        Some(Json::String(s)) if s == "Eramir" => "None".to_owned(),
        Some(v) if json_truthy(Some(v)) => jstr(Some(v), "bandit_choice"),
        _ => "None".to_owned(),
    };
    // L1239-1240
    set_sel_by_val(cx, "bandit", &bandit);

    // L1242
    let major_god = json_get(&char_passives, "pantheon_major")
        .filter(|v| json_truthy(Some(v)))
        .map_or_else(|| "None".to_owned(), |v| jstr(Some(v), "pantheon_major"));
    // L1243-1244
    set_sel_by_val(cx, "pantheonMajorGod", &major_god);

    // L1246
    let minor_god = json_get(&char_passives, "pantheon_minor")
        .filter(|v| json_truthy(Some(v)))
        .map_or_else(|| "None".to_owned(), |v| jstr(Some(v), "pantheon_minor"));
    // L1247-1248
    set_sel_by_val(cx, "pantheonMinorGod", &minor_god);

    // L1250-1251
    let build_name = cx.build().build_name.clone();
    let char_name = jstr(json_get(char_data, "name"), "name");
    let subtext = lfmt!(
        "%s (%s, %s, %s)",
        build_name.as_deref(),
        char_name.as_str(),
        class.as_str(),
        league.as_str()
    );
    crate::main_mode::folders::set_window_title_subtext(cx, Some(&subtext));
    let _ = me;
    // L1252
    format!(
        "{}Passive tree and jewels successfully imported.",
        cx.main.cc.POSITIVE
    )
}

/// The entries of a decoded object (none for any other value).
fn json_object(v: &Json) -> Vec<(String, &Json)> {
    match v {
        Json::Object(o) => o.iter().map(|(k, v)| (k.clone(), v)).collect(),
        _ => Vec::new(),
    }
}

/// `self.build.controls.<key>`
fn build_ctl(cx: &Cx, key: &'static str) -> CtrlId {
    let me = cx.build().node.id();
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// ImportTab.lua:1255 `SOCKET_GROUP_REIMPORT_KEY_SEPARATOR`
/// @pob Classes/ImportTab.lua:1255-1255 h=b44f026e
const SOCKET_GROUP_REIMPORT_KEY_SEPARATOR: &str = "\x1f";

/// lua: ImportTab.lua:1257-1269 `local function getSocketGroupReimportKey(socketGroup)`
/// @pob Classes/ImportTab.lua:1257-1269 h=54242033
fn get_socket_group_reimport_key(socket_group: &model::SocketGroup) -> String {
    // Use a rarely-used separator to avoid accidental collisions when concatenating fields.
    // L1259
    let mut gem_name_parts: Vec<String> = Vec::new();
    let gems = socket_group.gemList.get().snapshot();
    // L1260
    for gem in &gems {
        // L1261
        gem_name_parts.push(pob_lua::string::lower(&gem.nameSpec.get()));
    }
    // L1263-1268
    [
        socket_group
            .slot
            .get()
            .map_or_else(String::new, |s| s.to_string()),
        socket_group
            .source
            .get()
            .map_or_else(String::new, |s| s.to_string()),
        pob_lua::tostring_num(gems.len() as f64),
        gem_name_parts.join(SOCKET_GROUP_REIMPORT_KEY_SEPARATOR),
    ]
    .join(SOCKET_GROUP_REIMPORT_KEY_SEPARATOR)
}

/// One gem's entry of a reimport snapshot (ImportTab.lua:1274-1291).
#[derive(Clone)]
struct GemReimportState {
    enabled: bool,
    count: Option<f64>,
    skill_part: Option<f64>,
    skill_part_calcs: Option<f64>,
    skill_stage_count: Option<f64>,
    skill_stage_count_calcs: Option<f64>,
    skill_mine_count: Option<f64>,
    skill_mine_count_calcs: Option<f64>,
    skill_minion: Option<Name>,
    skill_minion_calcs: Option<Name>,
    skill_minion_item_set: Option<f64>,
    skill_minion_item_set_calcs: Option<f64>,
    skill_minion_skill: Option<f64>,
    skill_minion_skill_calcs: Option<f64>,
    enable_global1: bool,
    enable_global2: bool,
}

/// A socket group's reimport snapshot (ImportTab.lua:1293-1302).
#[derive(Clone)]
struct SocketGroupReimportState {
    enabled: bool,
    include_in_full_dps: Val,
    group_count: Option<f64>,
    label: Option<LStr>,
    main_active_skill: f64,
    main_active_skill_calcs: f64,
    gem_states: Vec<GemReimportState>,
    is_main_group: bool,
}

/// lua: ImportTab.lua:1271-1303 `local function snapshotSocketGroupReimportState(socketGroup,
/// isMainGroup)`
/// @pob Classes/ImportTab.lua:1271-1303 h=556b39ff
fn snapshot_socket_group_reimport_state(
    socket_group: &model::SocketGroup,
    is_main_group: bool,
) -> SocketGroupReimportState {
    // L1272
    let mut gem_states = Vec::new();
    // L1273
    for gem in socket_group.gemList.get().snapshot() {
        // L1274-1291
        gem_states.push(GemReimportState {
            enabled: gem.enabled.get(),
            count: gem.count.get(),
            skill_part: gem.skillPart.get(),
            skill_part_calcs: gem.skillPartCalcs.get(),
            skill_stage_count: gem.skillStageCount.get(),
            skill_stage_count_calcs: gem.skillStageCountCalcs.get(),
            skill_mine_count: gem.skillMineCount.get(),
            skill_mine_count_calcs: gem.skillMineCountCalcs.get(),
            skill_minion: gem.skillMinion.get(),
            skill_minion_calcs: gem.skillMinionCalcs.get(),
            skill_minion_item_set: gem.skillMinionItemSet.get(),
            skill_minion_item_set_calcs: gem.skillMinionItemSetCalcs.get(),
            skill_minion_skill: gem.skillMinionSkill.get(),
            skill_minion_skill_calcs: gem.skillMinionSkillCalcs.get(),
            enable_global1: gem.enableGlobal1.get(),
            enable_global2: gem.enableGlobal2.get(),
        });
    }
    // L1293-1302
    SocketGroupReimportState {
        enabled: socket_group.enabled.get(),
        include_in_full_dps: model::skills_tab::socketGroup_includeInFullDPS(socket_group),
        group_count: socket_group.groupCount.get(),
        label: model::skills_tab::socketGroup_label(socket_group),
        main_active_skill: socket_group.mainActiveSkill.get(),
        main_active_skill_calcs: socket_group.mainActiveSkillCalcs.get(),
        gem_states,
        is_main_group,
    }
}

/// lua: ImportTab.lua:1305-1322 `local function applyGemReimportState(gem, state)`
/// @pob Classes/ImportTab.lua:1305-1322 h=7f0474c7
fn apply_gem_reimport_state(gem: &model::GemInstance, state: &GemReimportState) {
    // L1306
    gem.enabled.set(state.enabled);
    // L1307
    gem.count.set(state.count);
    // L1308
    gem.skillPart.set(state.skill_part);
    // L1309
    gem.skillPartCalcs.set(state.skill_part_calcs);
    // L1310
    gem.skillStageCount.set(state.skill_stage_count);
    // L1311
    gem.skillStageCountCalcs.set(state.skill_stage_count_calcs);
    // L1312
    gem.skillMineCount.set(state.skill_mine_count);
    // L1313
    gem.skillMineCountCalcs.set(state.skill_mine_count_calcs);
    // L1314
    gem.skillMinion.set(state.skill_minion);
    // L1315
    gem.skillMinionCalcs.set(state.skill_minion_calcs);
    // L1316
    gem.skillMinionItemSet.set(state.skill_minion_item_set);
    // L1317
    gem.skillMinionItemSetCalcs
        .set(state.skill_minion_item_set_calcs);
    // L1318
    gem.skillMinionSkill.set(state.skill_minion_skill);
    // L1319
    gem.skillMinionSkillCalcs
        .set(state.skill_minion_skill_calcs);
    // L1320
    gem.enableGlobal1.set(state.enable_global1);
    // L1321
    gem.enableGlobal2.set(state.enable_global2);
}

/// `socketGroup.includeInFullDPS = v` with Lua nil (CCR-0019).
fn set_include_in_full_dps(socket_group: &model::SocketGroup, v: &Val) {
    socket_group.includeInFullDPS.set(v.truthy());
    socket_group.extra.set(
        n!("$includeInFullDPSNil"),
        if v.is_nil() {
            Val::Bool(true)
        } else {
            Val::Nil
        },
    );
}

/// lua: ImportTab.lua:1324-1338 `local function applySocketGroupReimportState(socketGroup, state)`
/// @pob Classes/ImportTab.lua:1324-1338 h=1d231780
fn apply_socket_group_reimport_state(
    socket_group: &model::SocketGroup,
    state: &SocketGroupReimportState,
) {
    // L1325
    socket_group.enabled.set(state.enabled);
    // L1326
    set_include_in_full_dps(socket_group, &state.include_in_full_dps);
    // L1327
    socket_group.groupCount.set(state.group_count);
    // L1328
    model::skills_tab::set_socketGroup_label(socket_group, state.label.as_deref());
    // L1329
    socket_group.mainActiveSkill.set(state.main_active_skill);
    // L1330
    socket_group
        .mainActiveSkillCalcs
        .set(state.main_active_skill_calcs);
    // L1331 (`state.gemStates` is always a table)
    let gems = socket_group.gemList.get().snapshot();
    // L1332
    for (gem_index, gem_state) in state.gem_states.iter().enumerate() {
        // L1333
        if let Some(gem) = gems.get(gem_index) {
            // L1334
            apply_gem_reimport_state(gem, gem_state);
        }
    }
}

/// ImportTab.lua:1340 `GUARD_ITEM_SET`
/// @pob Classes/ImportTab.lua:1340-1340 h=5228c860
const GUARD_ITEM_SET: &str = "Animate Guardian";

/// `itemsTab.itemSets[id]` (nil for an unknown or non-integral id).
fn item_set(m: &model::BuildRef, id: f64) -> Option<Table> {
    let k = model::items_tab::id_key(id)?;
    m.itemsTab.itemSets.with(|s| s.get(&k).cloned())
}

/// Locates AG's item set from the import
/// lua: ImportTab.lua:1342-1354 `ImportTabClass:GetOrCreateGuardianItemSet()`
/// @pob Classes/ImportTab.lua:1342-1354 h=944f8463
fn get_or_create_guardian_item_set(cx: &mut Cx) -> Table {
    // L1343
    let m = cx.build().model.clone();
    // L1344
    for item_set_id in m.itemsTab.itemSetOrderList.snapshot() {
        // L1345
        let item_set = item_set(&m, f64::from(item_set_id))
            .unwrap_or_else(|| lua_error!("attempt to index local 'itemSet' (a nil value)"));
        // L1346
        if item_set.str(n!("title")).as_deref() == Some(GUARD_ITEM_SET) {
            // L1347
            return item_set;
        }
    }
    // L1350
    let item_set = m.itemsTab.NewItemSet(None);
    // L1351
    item_set.set(n!("title"), GUARD_ITEM_SET);
    // L1352
    let id = item_set.num(n!("id"));
    m.itemsTab.itemSetOrderList.push(
        model::items_tab::id_key(id)
            .unwrap_or_else(|| lua_error!("CCR-0080: non-integer item set id")),
    );
    // L1353
    item_set
}

/// Allocates AG's item set for the AG skill gem.
/// lua: ImportTab.lua:1357-1372 `ImportTabClass:AssignGuardianItemSet(itemSetId)`
/// @pob Classes/ImportTab.lua:1357-1372 h=c56607ab
fn assign_guardian_item_set(cx: &mut Cx, item_set_id: f64) {
    // L1358
    let m = cx.build().model.clone();
    // L1359
    for socket_group in m.skillsTab.socketGroupList.get().snapshot() {
        // L1360
        for gem in socket_group.gemList.get().snapshot() {
            // L1361
            if gem
                .grantedEffect
                .get()
                .is_some_and(|g| &*g.name == "Animate Guardian")
            {
                // L1362
                for cell in [&gem.skillMinionItemSet, &gem.skillMinionItemSetCalcs] {
                    // L1363
                    let current = cell.get();
                    // L1364
                    let current_set = current.and_then(|c| item_set(&m, c));
                    // L1365
                    if current.is_none()
                        || current_set
                            .is_some_and(|s| s.str(n!("title")).as_deref() == Some(GUARD_ITEM_SET))
                    {
                        // L1366
                        cell.set(Some(item_set_id));
                    }
                }
            }
        }
    }
}

/// The skill-order rank of a group (ImportTab.lua:1430-1447): the smallest index in
/// `skillOrder` of its active gems.
fn skill_order_rank(group: &model::SocketGroup, skill_order: &[Name]) -> Option<usize> {
    let mut order: Option<usize> = None;
    for gem in group.gemList.get().snapshot() {
        if let Some(ge) = gem.grantedEffect.get() {
            if !ge.support {
                let i = skill_order
                    .iter()
                    .position(|n| *n == ge.name)
                    .map(|i| i + 1);
                if let Some(i) = i {
                    if order.map_or(true, |o| i < o) {
                        order = Some(i);
                    }
                }
            }
        }
    }
    order
}

/// lua: ImportTab.lua:1381-1491 `ImportTabClass:ImportItemsAndSkills(charData, clearItems,
/// clearSkills, ignoreWeaponSwap)`
/// @pob Classes/ImportTab.lua:1381-1491 h=db9867b8
pub fn import_items_and_skills(
    cx: &mut Cx,
    _me: CtrlId,
    char_data: &Json,
    clear_items: bool,
    clear_skills: bool,
    ignore_weapon_swap: bool,
) -> (Json, String) {
    let m = cx.build().model.clone();
    let seams = tab_seams();
    // L1382
    let char_data = char_data.clone();
    // L1383
    if clear_items {
        // L1384 LUA-ORDER: string keys, insertion order (calc doc D8). PoB's order is its
        // per-process string-hash order and is observable here: DeleteItem's PopulateSlots can
        // empty a slot visited later (a quiver once its bow is deleted), whose item then stays.
        let slots: Vec<Table> = m.itemsTab.slots.with(|s| s.values().cloned().collect());
        for slot in slots {
            // L1385
            let sel = slot.get(n!("selItemId"));
            if !sel.raw_eq(&Val::Num(0.0)) && !slot.get(n!("nodeId")).truthy() {
                // L1386
                let item = sel
                    .opt_num()
                    .and_then(model::items_tab::id_key)
                    .and_then(|k| m.itemsTab.items.with(|i| i.get(&k).cloned()))
                    .unwrap_or_else(|| lua_error!("attempt to index local 'item' (a nil value)"));
                (seams.items_delete_item)(cx, &item, false);
            }
        }
    }

    // L1391
    let main_skill_empty = m.skillsTab.socketGroupList.get().is_empty();
    // L1392-1393
    let mut skill_order: Option<Vec<Name>> = None;
    let mut preserved: Option<indexmap::IndexMap<String, Vec<SocketGroupReimportState>>> = None;
    // L1394
    if clear_skills {
        // L1395
        let mut order = Vec::new();
        // L1396
        let mut pres: indexmap::IndexMap<String, Vec<SocketGroupReimportState>> =
            indexmap::IndexMap::new();
        let groups = m.skillsTab.socketGroupList.get().snapshot();
        // L1397
        for socket_group in &groups {
            // L1398
            for gem in socket_group.gemList.get().snapshot() {
                // L1399
                if let Some(ge) = gem.grantedEffect.get().filter(|g| !g.support) {
                    // L1400
                    order.push(ge.name);
                }
            }
        }
        // L1404
        for (index, socket_group) in groups.iter().enumerate() {
            // L1405
            let key = get_socket_group_reimport_key(socket_group);
            // L1406-1407
            let is_main = (index + 1) as f64 == m.mainSocketGroup.get();
            pres.entry(key)
                .or_default()
                .push(snapshot_socket_group_reimport_state(socket_group, is_main));
        }
        // L1409 (wiped in place: skill sets alias the list)
        m.skillsTab.socketGroupList.get().with_mut(Vec::clear);
        // L1410-1411
        (seams.skills_clear_group_list_selection)(cx);
        // L1412
        (seams.skills_set_display_group)(cx, None);
        // L1413
        m.skillsTab.RebuildImbuedSupportBySlot(&m);
        skill_order = Some(order);
        preserved = Some(pres);
    }
    // L1415
    for item_data in jlist(json_get(&char_data, "equipment"), "equipment") {
        // L1416
        import_item(cx, &item_data, None, ignore_weapon_swap, None);
    }
    // L1418
    if let Some(guardian) =
        json_get(&char_data, "guardian").filter(|g| jindex(Some(g), 1).is_some())
    {
        // L1419
        let guardian_set = get_or_create_guardian_item_set(cx);
        let guardian_set_id = guardian_set.num(n!("id"));
        // L1420
        for item_data in json_list(guardian) {
            // L1421
            import_item(
                cx,
                &item_data,
                None,
                ignore_weapon_swap,
                Some(guardian_set_id),
            );
        }
        // L1423
        assign_guardian_item_set(cx, guardian_set_id);
    }
    // L1425
    if let Some(skill_order) = &skill_order {
        let list = m.skillsTab.socketGroupList.get();
        // L1426-1429 (groups by identity)
        let group_order: Vec<model::SocketGroupRef> = list.snapshot();
        let index_of = |g: &model::SocketGroupRef| {
            group_order
                .iter()
                .position(|x| Rc::ptr_eq(x, g))
                .expect("group in the list")
        };
        // L1430-1460
        list.with_mut(|v| {
            pob_lua::sort(v, |a, b| {
                // L1431-1439
                let order_a = skill_order_rank(a, skill_order);
                // L1440-1448
                let order_b = skill_order_rank(b, skill_order);
                match (order_a, order_b) {
                    // L1449-1454
                    (Some(oa), Some(ob)) => {
                        if oa != ob {
                            oa < ob
                        } else {
                            index_of(a) < index_of(b)
                        }
                    }
                    // L1455-1456
                    (None, None) => index_of(a) < index_of(b),
                    // L1457-1458 `return orderA`
                    (oa, _) => oa.is_some(),
                }
            });
        });
    }
    // L1462
    if let Some(mut preserved) = preserved {
        // L1463
        let mut restored_main_socket_group: Option<f64> = None;
        // L1464
        for (index, socket_group) in m
            .skillsTab
            .socketGroupList
            .get()
            .snapshot()
            .iter()
            .enumerate()
        {
            // L1465
            let key = get_socket_group_reimport_key(socket_group);
            // L1466
            if let Some(state_list) = preserved.get_mut(&key).filter(|l| !l.is_empty()) {
                // L1467
                let state = state_list.remove(0);
                // L1468
                apply_socket_group_reimport_state(socket_group, &state);
                // L1469
                if state.is_main_group {
                    // L1470
                    restored_main_socket_group = Some((index + 1) as f64);
                }
            }
        }
        // L1474
        if let Some(r) = restored_main_socket_group {
            // L1475
            m.mainSocketGroup.set(r);
        }
    }
    // L1478
    if main_skill_empty {
        // L1479
        m.mainSocketGroup.set(guess_main_socket_group(cx));
    }
    // L1481
    (seams.items_populate_slots)(cx);
    // L1482
    (seams.items_add_undo_state)(cx);
    // L1483
    m.skillsTab.UpdateSocketGroups(&m);
    // L1484
    (seams.skills_add_undo_state)(cx);
    // L1485
    let level = json_get(&char_data, "level");
    m.characterLevel.set(jnum(level, "level"));
    // L1486
    m.configTab.UpdateLevel(&m);
    // L1487
    let level_ctl = build_ctl(cx, "characterLevel");
    EditControl::set_text(cx, level_ctl, &jtostring(level), false);
    // L1488
    m.buildFlag.set(true);
    // charData for the wrapper
    // L1490
    let msg = format!(
        "{}Items and skills successfully imported.",
        cx.main.cc.POSITIVE
    );
    (char_data, msg)
}

/// ImportTab.lua:1493 `rarityMap`
/// @pob Classes/ImportTab.lua:1493-1493 h=b26f5dc9
fn rarity_map(frame_type: f64) -> Option<&'static str> {
    match frame_type {
        0.0 => Some("NORMAL"),
        1.0 => Some("MAGIC"),
        2.0 => Some("RARE"),
        3.0 => Some("UNIQUE"),
        9.0 | 10.0 => Some("RELIC"),
        _ => None,
    }
}

/// ImportTab.lua:1494-1495 `slotMap`
/// @pob Classes/ImportTab.lua:1494-1495 h=01a00d76
fn slot_map(inventory_id: &str) -> Option<&'static str> {
    Some(match inventory_id {
        "Weapon" => "Weapon 1",
        "Offhand" => "Weapon 2",
        "Weapon2" => "Weapon 1 Swap",
        "Offhand2" => "Weapon 2 Swap",
        "Helm" => "Helmet",
        "BodyArmour" => "Body Armour",
        "Gloves" => "Gloves",
        "Boots" => "Boots",
        "Amulet" => "Amulet",
        "Ring" => "Ring 1",
        "Ring2" => "Ring 2",
        "Ring3" => "Ring 3",
        "Belt" => "Belt",
        "BrequelGrafts" => "Graft 1",
        "BrequelGrafts2" => "Graft 2",
        _ => return None,
    })
}

/// A mod line table `{ line = line, extra = extra, mods = modList or { }, <flags> }`; `mods`
/// and `extra` are dropped by BuildAndParseRaw (module docs).
fn mod_line(line: &str, flags: &[(&str, Val)]) -> Table {
    let t = Table::new();
    t.set(n!("line"), line);
    t.set(n!("mods"), Table::new());
    for (k, v) in flags {
        t.set(Name::from(k), v.clone());
    }
    t
}

/// `for line in text:gmatch("[^\n]+")`
fn lines_of(text: &str) -> Vec<String> {
    pob_lua::gmatch(text, "[^\n]+")
        .map(|c| {
            c.first()
                .map(|c| c.to_str().into_owned())
                .unwrap_or_default()
        })
        .collect()
}

/// `itemMod.description or itemMod` / `itemMod.flags or itemMod` (OAuth mod objects or site
/// strings).
fn mod_desc_and_flags(item_mod: &Json) -> (String, Json) {
    let desc = json_get(item_mod, "description").unwrap_or(item_mod);
    let flags = json_get(item_mod, "flags").unwrap_or(item_mod).clone();
    (jstr(Some(desc), "description"), flags)
}

/// `flags.<k>` of a mod's flags (a string mod has no fields: nil).
fn flag(flags: &Json, k: &str) -> Val {
    match flags {
        Json::Object(_) => json_get(flags, k).map_or(Val::Nil, json_to_val),
        Json::String(_) => Val::Nil,
        _ => lua_error!(
            "attempt to index local 'flags' (a {} value)",
            lua_type(Some(flags))
        ),
    }
}

/// `tonumber(property.values[1][1]:match("%d+"))`
fn first_value_number(property: &Json) -> Option<f64> {
    let v = jstr(jindex(jindex(json_get(property, "values"), 1), 1), "values");
    lmatch(&v, "%d+").and_then(|d| pob_lua::tonumber(&d))
}

/// `property.values[1][1]`
fn first_value(property: &Json) -> String {
    jstr(jindex(jindex(json_get(property, "values"), 1), 1), "values")
}

/// `data.gemGrantedEffectIdForVaalGemId[id]` (a dynamic root, contracts.md; CCR-0117 §4).
fn gem_granted_effect_id_for_vaal_gem_id(id: &str) -> Option<Name> {
    let data = crate::model::data();
    let root = data.other.get("data.gemGrantedEffectIdForVaalGemId")?;
    let pob_gamedata::DataValue::Table(t) = data.resolve(root) else {
        return None;
    };
    data.resolve(t.field(id)).as_str().map(Name::from)
}

/// lua: ImportTab.lua:1497-1821 `ImportTabClass:ImportItem(itemData, slotName,
/// ignoreWeaponSwap, itemSetId)`
/// @pob Classes/ImportTab.lua:1497-1821 h=20683a41
pub fn import_item(
    cx: &mut Cx,
    item_data: &Json,
    slot_name: Option<String>,
    ignore_weapon_swap: bool,
    item_set_id: Option<f64>,
) {
    let m = cx.build().model.clone();
    let data = m.data;
    let mut item_data = item_data.clone();
    let mut slot_name = slot_name;
    // L1498
    if slot_name.is_none() {
        let inventory_id = json_get(&item_data, "inventoryId");
        let inv = match inventory_id {
            Some(Json::String(s)) => Some(s.as_str()),
            _ => None,
        };
        // L1499
        if inv == Some("PassiveJewels") {
            // L1500
            let latest = m
                .latestTree
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'latestTree' (a nil value)"));
            let x = jnum(json_get(&item_data, "x"), "x");
            let slot = latest.extra.get(n!("jewelSlots")).get(Key::from(x + 1.0));
            slot_name = Some(format!(
                "Jewel {}",
                match slot {
                    Val::Num(n) => pob_lua::tostring_num(n),
                    Val::Str(s) => s.to_string(),
                    _ => lua_error!("attempt to concatenate field '?' (a nil value)"),
                }
            ));
        // L1501
        } else if inv == Some("Flask") {
            // L1502
            let x = jnum(json_get(&item_data, "x"), "x");
            slot_name = Some(format!("Flask {}", pob_lua::tostring_num(x + 1.0)));
        // L1503
        } else if !(ignore_weapon_swap && matches!(inv, Some("Weapon2" | "Offhand2"))) {
            // L1504
            slot_name = inv.and_then(slot_map).map(str::to_owned);
        }
    }
    // L1507
    let Some(slot_name) = slot_name else {
        // Ignore any items that won't go into known slots
        // L1509
        return;
    };

    // L1512
    let item: ItemRef = pob_calc::classes::item::new_Item(None, None, false);

    // Determine rarity, display name and base type of the item
    // L1515
    let frame_type = jnum_opt(json_get(&item_data, "frameType"), "frameType");
    let mut rarity = frame_type.and_then(rarity_map);
    let name = jstr(json_get(&item_data, "name"), "name");
    let type_line = jstr(json_get(&item_data, "typeLine"), "typeLine");
    // L1516
    if !name.is_empty() {
        // L1517
        let title = pob_lua::string::sanitiseText(&name);
        item.title.set(Some(LStr::from(title.as_str())));
        // L1518
        let base_name = pob_lua::gsub(
            &pob_lua::string::sanitiseText(&type_line),
            "Synthesised ",
            "",
            None,
        )
        .0;
        let mut base_name = pob_lua::gsub(&base_name, "^Vestigial ", "", None).0;
        // L1519
        item.name
            .set(LStr::from(format!("{title}, {base_name}").as_str()));
        // L1520
        if base_name == "Two-Toned Boots" {
            // Hack for Two-Toned Boots
            // L1522
            base_name = "Two-Toned Boots (Armour/Energy Shield)".to_owned();
        }
        item.baseName.set(Some(Name::from(base_name.as_str())));
        // L1524
        let base = data.itemBases.get(base_name.as_str()).cloned();
        item.base.set(base.clone());
        // L1525
        if let Some(base) = base {
            // L1526
            item.r#type.set(base.r#type);
        } else {
            // L1528
            cx.sys
                .platform
                .con_print(&format!("Unrecognised base in imported item: {base_name}"));
        }
    } else {
        // L1531
        let mut item_name = pob_lua::string::sanitiseText(&type_line);
        // L1532
        if lmatch(&item_name, "Energy Blade").is_some() {
            // L1533
            let mut one_handed = false;
            // L1534
            for p in jlist(json_get(&item_data, "properties"), "properties") {
                let pname = jstr(json_get(&p, "name"), "name");
                // L1535
                if data
                    .weaponTypeInfo
                    .get(&Name::from(pname.as_str()))
                    .is_some_and(|w| w.oneHand)
                {
                    // L1536
                    one_handed = true;
                    // L1537
                    break;
                }
            }
            // L1540
            item_name = if one_handed {
                "Energy Blade One Handed"
            } else {
                "Energy Blade Two Handed"
            }
            .to_owned();
            // L1541
            rarity = Some("NORMAL");
            // L1542
            jset(
                &mut item_data,
                "implicitMods",
                Some(Json::Array(Vec::new())),
            );
            // L1543
            jset(
                &mut item_data,
                "explicitMods",
                Some(Json::Array(Vec::new())),
            );
        }
        item.name.set(LStr::from(item_name.as_str()));
        // L1545 LUA-ORDER (string keys: the first base whose name the item name contains)
        for (base_name, base_data) in &data.itemBases {
            // L1546
            if let Some((s, e, _)) = pob_lua::find(&item_name, base_name, Some(1), true) {
                // L1547
                item.baseName.set(Some(Name::from(base_name)));
                // L1548
                let prefix = pob_lua::string::sub(&item_name, 1, Some(s as i64 - 1)).into_owned();
                item.extra.set(n!("namePrefix"), prefix.as_str());
                // L1549
                let suffix = pob_lua::string::sub(&item_name, e as i64 + 1, None).into_owned();
                item.extra.set(n!("nameSuffix"), suffix.as_str());
                // L1550
                item.r#type.set(base_data.r#type);
                // L1551
                break;
            }
        }
        // L1555
        if item.baseName.get().is_none() {
            // L1556
            if let Some((s, e, _)) = pob_lua::find(&item_name, "Two-Toned Boots", Some(1), true) {
                // Hack for Two-Toned Boots
                // L1559
                item.baseName
                    .set(Some(n!("Two-Toned Boots (Armour/Energy Shield)")));
                // L1560
                let prefix = pob_lua::string::sub(&item_name, 1, Some(s as i64 - 1)).into_owned();
                item.extra.set(n!("namePrefix"), prefix.as_str());
                // L1561
                let suffix = pob_lua::string::sub(&item_name, e as i64 + 1, None).into_owned();
                item.extra.set(n!("nameSuffix"), suffix.as_str());
                // L1562
                item.r#type.set(n!("Boots"));
            }
        }
        // L1565
        item.base.set(
            item.baseName
                .get()
                .and_then(|b| data.itemBases.get(b.as_str()).cloned()),
        );
    }
    // L1567
    let Some(base) = item.base.get() else {
        // L1568
        return;
    };
    let Some(rarity) = rarity else {
        return;
    };
    item.rarity.set(Name::from(rarity));

    // Import item data
    // L1572
    item.uniqueID
        .set(json_get(&item_data, "id").map(|v| LStr::from(jstr(Some(v), "id").as_str())));
    // L1573
    if let Some(influences) = json_get(&item_data, "influences").filter(|v| json_truthy(Some(v))) {
        // L1574
        for cur_influence_info in &pob_calc::classes::item::influenceInfo {
            // L1575
            let v = json_get(
                influences,
                &pob_lua::string::lower(cur_influence_info.display),
            );
            item.influences.set(
                Name::from(cur_influence_info.key),
                v.map_or(Val::Nil, json_to_val),
            );
        }
    }
    // L1578
    if json_truthy(json_get(&item_data, "searing")) {
        // L1579
        item.influences.set(n!("cleansing"), true);
    }
    // L1581
    if json_truthy(json_get(&item_data, "tangled")) {
        // L1582
        item.influences.set(n!("tangle"), true);
    }
    // L1584
    let ilvl = jnum(json_get(&item_data, "ilvl"), "ilvl");
    if ilvl > 0.0 {
        // L1585
        item.itemLevel.set(Some(ilvl));
    }
    // L1587
    if base.weapon.is_some()
        || base.armour.is_some()
        || base.flask.is_some()
        || base.tincture.is_some()
    {
        // L1588
        item.quality.set(Some(0.0));
    }
    // L1590
    if let Some(properties) = json_get(&item_data, "properties").filter(|v| json_truthy(Some(v))) {
        // L1591 (an array: index order)
        for property in json_list(properties) {
            let pname = jstr(json_get(&property, "name"), "name");
            // L1592
            if pname == "Quality" {
                // L1593
                item.quality.set(first_value_number(&property));
            // L1594
            } else if lmatch(&pname, "Quality %(").is_some() {
                // L1595-1608
                let catalyst = lmatch(&pname, "Quality %((.*) Modifiers%)").and_then(|c| {
                    Some(match c.as_str() {
                        "Attack" => 1.0,
                        "Speed" => 2.0,
                        "Suffix" => 3.0,
                        "Life and Mana" => 4.0,
                        "Caster" => 5.0,
                        "Attribute" => 6.0,
                        "Physical and Chaos Damage" => 7.0,
                        "Resistance" => 8.0,
                        "Prefix" => 9.0,
                        "Defense" => 10.0,
                        "Elemental Damage" => 11.0,
                        "Critical" => 12.0,
                        _ => return None,
                    })
                });
                // L1609
                item.catalyst.set(catalyst);
                // L1610
                item.catalystQuality.set(first_value_number(&property));
            // L1611
            } else if pname == "Radius" {
                // L1612
                item.jewelRadiusLabel
                    .set(Some(LStr::from(first_value(&property).as_str())));
            // L1613
            } else if pname == "Limited to" {
                // L1614
                item.limit.set(pob_lua::tonumber(&first_value(&property)));
            // L1615
            } else if pname == "Evasion Rating" {
                // L1616
                if item.baseName.get().as_deref() == Some("Two-Toned Boots (Armour/Energy Shield)")
                {
                    // Another hack for Two-Toned Boots
                    // L1618
                    item.baseName
                        .set(Some(n!("Two-Toned Boots (Armour/Evasion)")));
                    // L1619
                    item.base.set(
                        data.itemBases
                            .get("Two-Toned Boots (Armour/Evasion)")
                            .cloned(),
                    );
                }
            // L1621
            } else if pname == "Energy Shield" {
                // L1622
                if item.baseName.get().as_deref() == Some("Two-Toned Boots (Armour/Evasion)") {
                    // Yet another hack for Two-Toned Boots
                    // L1624
                    item.baseName
                        .set(Some(n!("Two-Toned Boots (Evasion/Energy Shield)")));
                    // L1625
                    item.base.set(
                        data.itemBases
                            .get("Two-Toned Boots (Evasion/Energy Shield)")
                            .cloned(),
                    );
                }
            // L1627
            } else if pob_lua::find(&pname, "Intangibility", None, false).is_some() {
                // L1628
                item.extra.set(
                    n!("intangibility"),
                    Val::from(first_value_number(&property)),
                );
            // L1629
            } else if pname == "Memory Strands" {
                // L1630
                item.extra.set(
                    n!("memoryStrands"),
                    Val::from(pob_lua::tonumber(&first_value(&property))),
                );
            }
            // L1632
            if matches!(
                pname.as_str(),
                "Energy Shield" | "Ward" | "Armour" | "Evasion Rating"
            ) {
                // L1633
                let armour_data = item.armourData.get().unwrap_or_else(|| {
                    let t = Table::new();
                    item.armourData.set(Some(t.clone()));
                    t
                });
                let key =
                    pob_lua::gsub(&pob_lua::gsub(&pname, " Rating", "", None).0, " ", "", None).0;
                // L1634
                for value in jlist(json_get(&property, "values"), "values") {
                    // L1635
                    let v = jstr(jindex(Some(&value), 1), "values");
                    let v = pob_lua::tonumber(&v).unwrap_or_else(|| {
                        lua_error!("attempt to perform arithmetic on a nil value")
                    });
                    let cur = armour_data.get(key.as_str());
                    let cur = if cur.truthy() { cur.num() } else { 0.0 };
                    armour_data.set(key.as_str(), cur + v);
                }
            }
        }
    }
    // L1640
    item.split.set(json_truthy(json_get(&item_data, "split")));
    // L1641
    item.mirrored.set(
        json_truthy(json_get(&item_data, "duplicated"))
            || json_truthy(json_get(&item_data, "mirrored")),
    );
    // L1642
    item.corrupted
        .set(json_truthy(json_get(&item_data, "corrupted")));
    // L1643
    item.fractured
        .set(json_truthy(json_get(&item_data, "fractured")));
    // L1644
    item.synthesised
        .set(json_truthy(json_get(&item_data, "synthesised")));
    // L1645
    item.extra.set(
        n!("vestigial"),
        json_get(&item_data, "vestigial").map_or(Val::Nil, json_to_val),
    );
    // L1646
    let sockets_json = json_get(&item_data, "sockets").cloned();
    if json_truthy(sockets_json.as_ref()) && jindex(sockets_json.as_ref(), 1).is_some() {
        // L1647
        let mut sockets: Vec<Table> = Vec::new();
        // L1648 (an array: index order)
        for socket in json_list(sockets_json.as_ref().expect("checked")) {
            let colour = json_get(&socket, "sColour");
            // L1649
            if matches!(colour, Some(Json::String(c)) if c == "A") {
                // L1650 `item.abyssalSocketCount or 0 + 1`: 1 once set (precedence, as in PoB)
                if item.abyssalSocketCount.get() == 0.0 {
                    item.abyssalSocketCount.set(1.0);
                }
            }
            // L1652
            let t = Table::new();
            t.set(
                n!("group"),
                json_get(&socket, "group").map_or(Val::Nil, json_to_val),
            );
            t.set(n!("color"), colour.map_or(Val::Nil, json_to_val));
            sockets.push(t);
        }
        item.sockets.set(Shared::new(sockets));
        // L1654
        let count = item.abyssalSocketCount.get();
        if count > 0.0 && lmatch(&item.name.get(), "Energy Blade").is_some() {
            // L1655
            let mut mods = jlist(json_get(&item_data, "explicitMods"), "explicitMods");
            mods.push(Json::String(format!(
                "Has {} Abyssal Sockets",
                pob_lua::tostring_num(count)
            )));
            jset(&mut item_data, "explicitMods", Some(Json::Array(mods)));
        }
    }
    // L1658
    if let Some(socketed_items) =
        json_get(&item_data, "socketedItems").filter(|v| json_truthy(Some(v)))
    {
        // L1659
        let socketed_items = socketed_items.clone();
        import_socketed_items(cx, &item, &socketed_items, &slot_name);
    }
    // L1661
    let socketed = json_get(&item_data, "socketedItems");
    if json_truthy(json_get(&item_data, "requirements"))
        && (!json_truthy(socketed) || jindex(socketed, 1).is_none())
    {
        // Requirements cannot be trusted if there are socketed gems, as they may override the
        // item's natural requirements
        // L1663
        item.requirements.wipe();
        // L1664
        for req in jlist(json_get(&item_data, "requirements"), "requirements") {
            let rname = jstr(json_get(&req, "name"), "name");
            // L1665
            if rname == "Level" {
                // L1666
                let v = jindex(jindex(json_get(&req, "values"), 1), 1);
                item.requirements
                    .set(n!("level"), v.map_or(Val::Nil, json_to_val));
            // L1667
            } else if rname == "Class:" {
                // L1668
                let v = jstr(jindex(jindex(json_get(&req, "values"), 1), 1), "values");
                item.classRestriction.set(Some(Name::from(v.as_str())));
            }
        }
    }
    // L1672-1677
    let enchant: Shared<Vec<Table>> = Shared::new(Vec::new());
    let scourge: Shared<Vec<Table>> = Shared::new(Vec::new());
    let class_req: Shared<Vec<Table>> = Shared::new(Vec::new());
    let implicit: Shared<Vec<Table>> = Shared::new(Vec::new());
    let explicit: Shared<Vec<Table>> = Shared::new(Vec::new());
    let crucible: Shared<Vec<Table>> = Shared::new(Vec::new());
    item.enchantModLines.set(enchant.clone());
    item.scourgeModLines.set(scourge.clone());
    item.classRequirementModLines.set(class_req);
    item.implicitModLines.set(implicit.clone());
    item.explicitModLines.set(explicit.clone());
    item.crucibleModLines.set(crucible.clone());
    let plain_lines = |key: &str| -> Vec<String> {
        json_get(&item_data, key)
            .filter(|v| json_truthy(Some(v)))
            .map(|v| {
                jlist(Some(v), key)
                    .iter()
                    .flat_map(|l| lines_of(&jstr(Some(l), key)))
                    .collect()
            })
            .unwrap_or_default()
    };
    // L1678-1685
    for line in plain_lines("enchantMods") {
        // L1682-1683
        enchant.push(mod_line(&line, &[("crafted", Val::Bool(true))]));
    }
    // L1686-1693
    for line in plain_lines("scourgeMods") {
        // L1690-1691
        scourge.push(mod_line(&line, &[("scourge", Val::Bool(true))]));
    }
    // L1694
    if let Some(mods) = json_get(&item_data, "implicitMods").filter(|v| json_truthy(Some(v))) {
        // L1695
        for item_mod in jlist(Some(mods), "implicitMods") {
            // L1696-1697
            let (mod_line_text, flags) = mod_desc_and_flags(&item_mod);
            // L1698
            for line in lines_of(&mod_line_text) {
                // L1699-1700
                implicit.push(mod_line(&line, &[("vestigial", flag(&flags, "vestigial"))]));
            }
        }
    }
    // TODO: Remove once 3.29 releases https://www.pathofexile.com/developer/docs/changelog#3-29-0
    // L1705-1712
    for line in plain_lines("fracturedMods") {
        // L1709-1710
        explicit.push(mod_line(&line, &[("fractured", Val::Bool(true))]));
    }
    // L1713
    if let Some(mods) = json_get(&item_data, "explicitMods").filter(|v| json_truthy(Some(v))) {
        // L1714
        for item_mod in jlist(Some(mods), "explicitMods") {
            // L1715-1716
            let (mod_line_text, flags) = mod_desc_and_flags(&item_mod);
            // L1717
            for line in lines_of(&mod_line_text) {
                // L1718-1723
                explicit.push(mod_line(
                    &line,
                    &[
                        ("fractured", flag(&flags, "fractured")),
                        ("crafted", flag(&flags, "crafted")),
                        ("mutated", flag(&flags, "mutated")),
                        ("vestigial", flag(&flags, "vestigial")),
                    ],
                ));
            }
        }
    }
    // L1727-1734
    for line in plain_lines("crucibleMods") {
        // L1731-1732
        crucible.push(mod_line(&line, &[("crucible", Val::Bool(true))]));
    }
    // TODO: Remove once 3.29 releases https://www.pathofexile.com/developer/docs/changelog#3-29-0
    // L1736-1743
    for line in plain_lines("craftedMods") {
        // L1740-1741
        explicit.push(mod_line(&line, &[("crafted", Val::Bool(true))]));
    }
    // TODO: Remove once 3.29 releases https://www.pathofexile.com/developer/docs/changelog#3-29-0
    // L1745-1752
    for line in plain_lines("mutatedMods") {
        // L1749-1750
        explicit.push(mod_line(&line, &[("mutated", Val::Bool(true))]));
    }
    // Sometimes flavour text has actual mods that PoB cares about
    // Right now, the only known one is "This item can be anointed by Cassia"
    // L1755-1776
    for line in plain_lines("flavourText") {
        // Remove any text outside of curly braces, if they exist.
        // This fixes lines such as:
        //   "<default>{This item can be anointed by Cassia}"
        // To now be:
        //   "This item can be anointed by Cassia"
        // L1763
        let start_bracket = pob_lua::find(&line, "{", None, false).map(|f| f.0);
        // L1764
        let end_bracket = pob_lua::find(&line, "}", None, false).map(|f| f.0);
        let mut line = line;
        // L1765
        if let (Some(s), Some(e)) = (start_bracket, end_bracket) {
            if e > s {
                // L1766
                line = pob_lua::string::sub(&line, s as i64 + 1, Some(e as i64 - 1)).into_owned();
            }
        }

        // If the line parses, then it should be included as an explicit mod
        // L1770 (CCR-0117 §5: `modLib.parseMod` through pob-modparser)
        let (mod_list, _extra) = pob_modparser::parseMod(&line, false);
        // L1771
        if mod_list.is_some() {
            // L1772
            explicit.push(mod_line(&line, &[]));
        }
    }
    // L1777
    if json_truthy(json_get(&item_data, "foilVariation"))
        || json_truthy(json_get(&item_data, "isRelic"))
    {
        // L1778-1791
        const FOIL_VARIANTS: [&str; 12] = [
            "Amethyst",
            "Verdant",
            "Ruby",
            "Cobalt",
            "Sunset",
            "Aureate",
            "Celestial Quartz",
            "Celestial Ruby",
            "Celestial Emerald",
            "Celestial Aureate",
            "Celestial Pearl",
            "Celestial Amethyst",
        ];
        // L1792
        let variant = json_get(&item_data, "foilVariation")
            .and_then(|v| v.as_f64())
            .filter(|v| v.fract() == 0.0 && *v >= 1.0 && *v <= 12.0)
            .map(|v| FOIL_VARIANTS[v as usize - 1]);
        item.extra.set(n!("foilType"), variant.unwrap_or("Rainbow"));
    }

    // Add and equip the new item
    // L1796
    item.BuildAndParseRaw();
    //ConPrintf("%s", item.raw)
    // L1798
    if item.base.get().is_some() {
        // L1799
        let unique_id = json_get(&item_data, "id").map(|v| jstr(Some(v), "id"));
        let mut rep_item: Option<ItemRef> = None;
        // L1800 LUA-ORDER (unique ids are unique: the first match is the only one)
        for it in m
            .itemsTab
            .items
            .with(|i| i.values().cloned().collect::<Vec<_>>())
        {
            // L1801
            if it.uniqueID.get().as_deref() == unique_id.as_deref() {
                // L1802-1803
                rep_item = Some(it);
                // L1804
                break;
            }
        }
        // L1807
        if let Some(rep_item) = rep_item {
            // Item already exists in the build, overwrite it
            // L1809
            item.id.set(rep_item.id.get());
            // L1810
            let k = item
                .id
                .get()
                .and_then(model::items_tab::id_key)
                .unwrap_or_else(|| lua_error!("table index is nil"));
            m.itemsTab.items.with_mut(|i| {
                i.insert(k, item.clone());
            });
            // L1811
            item.BuildModList();
        } else {
            // L1813
            m.itemsTab.AddItem(&m, &item, true, None);
        }
        let id = item.id.get().unwrap_or(f64::NAN);
        // L1815
        if let Some(set_id) = item_set_id.filter(|s| *s != m.itemsTab.activeItemSetId.get()) {
            // L1816
            let set = item_set(&m, set_id)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            set.table(slot_name.as_str())
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                .set(n!("selItemId"), id);
        } else {
            // L1818
            let f = tab_seams().items_slot_set_sel_item_id;
            f(cx, &slot_name, id);
        }
    }
}

/// lua: ImportTab.lua:1823-1898 `ImportTabClass:ImportSocketedItems(item, socketedItems,
/// slotName)`
/// @pob Classes/ImportTab.lua:1823-1898 h=b17e8d06
pub fn import_socketed_items(cx: &mut Cx, item: &ItemRef, socketed_items: &Json, slot_name: &str) {
    let m = cx.build().model.clone();
    let data = m.data;
    // Build socket group list
    // L1825 (integer keys from the item's socket groups: LjIntMap for PoB's order)
    let mut item_socket_group_list: LjIntMap<model::SocketGroupRef> = LjIntMap::new();
    // L1826
    let mut abyssal_socket_id = 1;
    // L1827
    for socketed_item in jlist(Some(socketed_items), "socketedItems") {
        // L1828
        if json_truthy(json_get(&socketed_item, "abyssJewel")) {
            // L1829
            import_item(
                cx,
                &socketed_item,
                Some(format!("{slot_name} Abyssal Socket {abyssal_socket_id}")),
                false,
                None,
            );
            // L1830
            abyssal_socket_id += 1;
        } else {
            // L1832
            let mut normalized_basename = pob_lua::string::sanitiseText(&jstr(
                json_get(&socketed_item, "typeLine"),
                "typeLine",
            ));
            // L1833
            let mut gem_id = data
                .gemForBaseName
                .get(pob_lua::string::lower(&normalized_basename).as_str())
                .copied();
            // L1834
            if let Some(hybrid) =
                json_get(&socketed_item, "hybrid").filter(|v| json_truthy(Some(v)))
            {
                // Used by transfigured gems and dual-skill gems (currently just Stormbind)
                // L1836
                normalized_basename = pob_lua::string::sanitiseText(&jstr(
                    json_get(hybrid, "baseTypeName"),
                    "baseTypeName",
                ));
                // L1837
                gem_id = data
                    .gemForBaseName
                    .get(pob_lua::string::lower(&normalized_basename).as_str())
                    .copied();
                // L1838
                if let Some(g) = gem_id.filter(|_| json_truthy(json_get(hybrid, "isVaalGem"))) {
                    // L1839
                    let ge = data
                        .gems
                        .get(&g)
                        .map(|g| g.grantedEffectId)
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                    gem_id = gem_granted_effect_id_for_vaal_gem_id(&ge);
                }
            }
            // L1842
            if let Some(gem_id) = gem_id {
                // L1843
                let gem_instance = model::GemInstance::default();
                gem_instance.level.set(20.0);
                gem_instance.quality.set(0.0);
                gem_instance.enabled.set(true);
                gem_instance.enableGlobal1.set(true);
                gem_instance.gemId.set(Some(gem_id));
                // L1844
                let gem_data = data
                    .gems
                    .get(&gem_id)
                    .cloned()
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                gem_instance.nameSpec.set(LStr::from(&*gem_data.name));
                // L1845
                gem_instance.extra.set(
                    n!("support"),
                    json_get(&socketed_item, "support").map_or(Val::Nil, json_to_val),
                );
                // L1846 (an array: index order)
                for property in jlist(json_get(&socketed_item, "properties"), "properties") {
                    let pname = jstr(json_get(&property, "name"), "name");
                    // L1847
                    if pname == "Level" {
                        // L1848
                        gem_instance
                            .level
                            .set(first_value_number(&property).unwrap_or(f64::NAN));
                    // L1849
                    } else if pname == "Quality" {
                        // L1850
                        gem_instance
                            .quality
                            .set(first_value_number(&property).unwrap_or(f64::NAN));
                    }
                }
                let gem_instance = Rc::new(gem_instance);
                // L1853
                let socket = jnum(json_get(&socketed_item, "socket"), "socket");
                let group_id = item
                    .sockets
                    .get()
                    .with(|s| s.get(socket as usize).cloned())
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                    .get(n!("group"));
                let group_key = group_id
                    .opt_num()
                    .unwrap_or_else(|| lua_error!("table index is nil"))
                    as i64;
                // L1854
                let socket_group = if let Some(g) = item_socket_group_list.get(group_key) {
                    g
                } else {
                    // L1855
                    let g = model::SocketGroup::default();
                    model::skills_tab::set_socketGroup_label(&g, Some(""));
                    g.enabled.set(true);
                    g.slot.set(Some(Name::from(slot_name)));
                    set_include_in_full_dps(&g, &Val::Nil);
                    g.mainActiveSkill.set(f64::NAN);
                    g.mainActiveSkillCalcs.set(f64::NAN);
                    let g = Rc::new(g);
                    item_socket_group_list.set(group_key, Some(g.clone()));
                    g
                };
                // L1857-1858
                socket_group.gemList.get().push(gem_instance);
                // L1859
                if let Some(built_in) =
                    json_get(&socketed_item, "builtInSupport").filter(|v| json_truthy(Some(v)))
                {
                    // L1860
                    let imbued = pob_lua::gsub(
                        &jstr(Some(built_in), "builtInSupport"),
                        "Supported by Level 1 ",
                        "",
                        None,
                    )
                    .0;
                    socket_group.extra.set(n!("imbuedSupport"), imbued.as_str());
                    // L1861 (the global `data`)
                    let key = format!("{} support", pob_lua::string::lower(&imbued));
                    let gem = data
                        .gemForBaseName
                        .get(key.as_str())
                        .and_then(|g| data.gems.get(g))
                        .cloned();
                    m.skillsTab.imbuedSupport_gemChangeFunc(
                        &m,
                        gem,
                        true,
                        Some(Name::from(slot_name)),
                    );
                }
            }
        }
    }

    // Import the socket groups
    // L1868 LUA-ORDER (LjIntMap: PoB's order of the group ids)
    for (_, item_socket_group) in item_socket_group_list.pairs() {
        let item_gems = item_socket_group.gemList.get().snapshot();
        // Check if this socket group matches an existing one
        // L1870
        let mut rep_group: Option<model::SocketGroupRef> = None;
        // L1871 (an array: index order)
        for socket_group in m.skillsTab.socketGroupList.get().snapshot() {
            let gems = socket_group.gemList.get().snapshot();
            // L1872
            if gems.len() == item_gems.len()
                && socket_group
                    .slot
                    .get()
                    .map_or(true, |s| s.as_str() == slot_name)
            {
                // L1873
                let mut is_match = true;
                // L1874
                for (gem_index, gem) in gems.iter().enumerate() {
                    // L1875
                    if pob_lua::string::lower(&gem.nameSpec.get())
                        != pob_lua::string::lower(&item_gems[gem_index].nameSpec.get())
                    {
                        // L1876
                        is_match = false;
                        // L1877
                        break;
                    }
                }
                // L1880
                if is_match {
                    // L1881
                    rep_group = Some(socket_group);
                    // L1882
                    break;
                }
            }
        }
        // L1886
        if let Some(rep_group) = rep_group {
            // Update the existing one
            // L1888
            for (gem_index, gem) in rep_group.gemList.get().snapshot().iter().enumerate() {
                // L1889
                let item_gem = &item_gems[gem_index];
                // L1890
                gem.level.set(item_gem.level.get());
                // L1891
                gem.quality.set(item_gem.quality.get());
            }
        } else {
            // L1894
            m.skillsTab
                .socketGroupList
                .get()
                .push(item_socket_group.clone());
        }
        // L1896
        m.skillsTab.ProcessSocketGroup(&m, &item_socket_group);
    }
}

/// Return the index of the group with the most gems
/// lua: ImportTab.lua:1901-1911 `ImportTabClass:GuessMainSocketGroup()`
/// @pob Classes/ImportTab.lua:1901-1911 h=00cedc2f
pub fn guess_main_socket_group(cx: &Cx) -> f64 {
    // L1902
    let mut largest_group_size = 0;
    // L1903
    let mut largest_group_index = 1;
    // L1904
    for (i, socket_group) in cx
        .build()
        .model
        .skillsTab
        .socketGroupList
        .get()
        .snapshot()
        .iter()
        .enumerate()
    {
        let n = socket_group.gemList.get().len();
        // L1905
        if n > largest_group_size {
            // L1906
            largest_group_size = n;
            // L1907
            largest_group_index = i + 1;
        }
    }
    // L1910
    largest_group_index as f64
}

/// lua: ImportTab.lua:1926-1932 `ImportTabClass:SetPredefinedBuildName()`
/// @pob Classes/ImportTab.lua:1926-1932 h=f654ac63
pub fn set_predefined_build_name(cx: &mut Cx, me: CtrlId) {
    // L1927
    let b = buf(cx, me, "siteAccountName");
    let account_name = pob_lua::gsub(&pob_lua::gsub(&b, "%s+", "", None).0, "#%d+", "", None).0;
    // L1928
    let char_select = ctl(cx, me, "siteCharSelect");
    // L1929
    let char_data = selected_char(cx, char_select);
    // L1930
    let char_name = jstr(json_get(&char_data, "name"), "name");
    // L1931
    cx.app_mut().main.predefined_build_name = Some(format!("{account_name} - {char_name}"));
}

// ---- the hooks U43's controls call (CCR-0109 §5) ----------------------------------------------

/// `core::ImportHooks::import_passive_tree_and_jewels` (the result is discarded by every caller,
/// ImportTab.lua:260, :848).
pub fn import_passive_tree_and_jewels_hook(
    cx: &mut Cx,
    me: CtrlId,
    char_data: &Json,
    delete_jewels: bool,
) {
    import_passive_tree_and_jewels(cx, me, char_data, delete_jewels);
}

/// `core::ImportHooks::import_items_and_skills` (the results are discarded by every caller,
/// ImportTab.lua:301, :883).
pub fn import_items_and_skills_hook(
    cx: &mut Cx,
    me: CtrlId,
    char_data: &Json,
    clear_items: bool,
    clear_skills: bool,
    ignore_weapon_swap: bool,
) {
    import_items_and_skills(
        cx,
        me,
        char_data,
        clear_items,
        clear_skills,
        ignore_weapon_swap,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn json_to_lua_quirks() {
        // the gsub chain of Common.lua:408-409, as LuaJIT runs it
        assert_eq!(
            String::from_utf8(json_to_lua(r#"{"a":[1,2],"5":"x\/y","n":"é"}"#)).unwrap(),
            "{[\"a\"]={1,2},[5]=\"x/y\",[\"n\"]=\"é\"}"
        );
        // `{(%w+)}` makes single-word arrays start at index 0
        assert_eq!(String::from_utf8(json_to_lua("[5]")).unwrap(), "{[0]=5}");
        assert_eq!(code_point_to_utf8(f64::from(0xD800)), b"?");
    }

    #[test]
    fn site_json() {
        let v = process_site_json(
            r#"[{"name":"A b","league":"Standard","level":90,"lastActive":true,"x":null},{"name":"C","realm":"pc","level":-1.5e2}]"#,
        )
        .unwrap();
        let a = json_list(&v);
        assert_eq!(a.len(), 2);
        assert_eq!(json_get(&a[0], "name").and_then(Json::as_str), Some("A b"));
        assert_eq!(json_get(&a[0], "level").and_then(Json::as_f64), Some(90.0));
        assert!(json_get(&a[0], "x").is_none());
        assert_eq!(
            json_get(&a[1], "level").and_then(Json::as_f64),
            Some(-150.0)
        );
        // `{(%w+)}` turns `[5]` into `{[0]=5}`: not a sequence
        let v = process_site_json("[5]").unwrap();
        assert!(matches!(v, Json::Object(_)));
        assert!(json_list(&v).is_empty());
        assert_eq!(process_site_json("{}").unwrap(), Json::Array(Vec::new()));
        assert!(process_site_json("\"str\"").is_err());
        assert!(process_site_json("{\"a\":").is_err());
        assert!(process_site_json("<html>").is_err());
    }

    #[test]
    fn standard_leagues() {
        assert_eq!(find_matching_standard_league("Hardcore Mirage"), "Hardcore");
        assert_eq!(
            find_matching_standard_league("HC SSF R Mirage"),
            "SSF Hardcore"
        );
        assert_eq!(find_matching_standard_league("SSF Mirage"), "SSF Standard");
        assert_eq!(find_matching_standard_league("Ruthless Mirage"), "Standard");
    }
}
