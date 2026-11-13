//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:1618-2221 (FormatStat, breakdown index, sidebar breakdown, AddDisplayStatList, item warnings, RefreshStatList, CompareStatList, AddStatComparesToTooltip).
//! Owner: U21.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! The buildMode methods of Build.lua:1718-2220 (RefreshSkillSelectControls, 1618-1716, is
//! `skill_select.rs`), plus the display-stat half of `Build:Save` (1033-1080) that calc:BM-A's
//! `build_xml` calls through `set_save_stats_hook` (CCR-0080 §8, ui doc §10.2).
//!
//! `self` is the buildMode object: `self.controls.*` are the children of the build node,
//! `self.calcsTab` / `self.characterLevel` are the model's, `self.displayStats` /
//! `self.minionDisplayStats` are `display_stats.rs`, and the fields these methods own
//! (`lastShowThousandsSeparators` …, `breakdownIndex`) are [`State`] in the build node.
//!
//! DIVERGENCE(persist): the Lua buildMode object outlives a build, so FormatStat's `last*`
//! fields keep the previous build's values until the next FormatStat; the Rust node is created
//! by each `Init`, so they start nil. They are written again by Init's RefreshStatList (L696),
//! which always formats rows (the resistances have `condFunc = true`), before OnFrame reads them.
//!
//! DIVERGENCE(section-list): BuildBreakdownIndex reads `self.calcsTab.sectionList`, the
//! CalcSectionControls (U39). Until that exists the index is built from the same data:
//! the SkillSelect section's two breakdown rows (CalcsTab.lua:142-144; its other rows hold
//! controls and no cells) followed by `Modules/CalcSections` (`data().calcSections`, which
//! CalcsTab.lua:150-154 adds in order). The cells are converted to calc tables once per index,
//! so the index shares them by reference as the Lua does; they carry no `calcSection` back
//! pointer (CalcSectionControl.lua:38), which only CalcsTab's own breakdown reads.
//!
//! ORDER: `pairs(breakdownCells)` (L1801) sets each key once per subsection, so its order does
//! not matter (c). `pairs(cell)` (L1789) copies fields (c). The two `pairs` over the pool
//! warning tables (L2025, L2036) iterate a constructor's string keys: PoB's order is
//! per-process random (calc doc D8); the port uses source order (b: the warnings' order is a
//! multiset in comparisons).
//!
//! `EstimatePlayerProgress` (L2118) is U20's: it is called through [`set_estimate_player_progress`]
//! (CCR-0103 §2); with nothing registered it is U20's `loadouts::estimate_player_progress`
//! (CCR-0106 §4).

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_lua::math::{m_abs, m_floor, m_huge};
use pob_lua::{lfmt, lua_error, tostring_num, Name};
use pob_sg::{Align, Font};
use pob_ui_core::color_codes::hex_to_rgb;
use pob_ui_core::controls::text_list::{TextListControl, TextListRow};
use pob_ui_core::luastr::format_num_sep_main;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;

use super::display_stats::{self, DisplayStat, Suffix};
use super::init::{insert_if_new, match_flags, WarningsControl};
use super::BuildMode;
use crate::app::CxApp;
use crate::model::{self, gamedata::ModType, Output, Table, Val, XmlChild, XmlNode};
// CCR-0103 §4: model.rs (U15) does not re-export the actor and active skill types
use pob_calc::active_skill::ActiveSkillRef;
use pob_calc::actor::{Actor, ActorRef};

/// U21's part of the buildMode object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.lastShowThousandsSeparators` (FormatStat L1749; OnFrame, U20, compares it)
    pub lastShowThousandsSeparators: Option<bool>,
    /// `self.lastShowThousandsSeparator`
    pub lastShowThousandsSeparator: Option<String>,
    /// `self.lastShowDecimalSeparator`
    pub lastShowDecimalSeparator: Option<String>,
    /// `self.lastShowTitlebarName`
    pub lastShowTitlebarName: Option<bool>,
    /// `self.lastUseCompactValues`
    pub lastUseCompactValues: Option<bool>,
    /// `self.breakdownIndex` (nil after Init, L623)
    pub breakdownIndex: Option<Rc<BreakdownIndex>>,
}

/// The fields a stat box row carries besides its text (Build.lua:1994-2004:
/// `breakdown`, `modNames`, `actorName`, `ignoredSections`, `breakdownColour`); the row's
/// `extra` (`hoveredLine.line`, read by SetDisplayStat, U20).
#[derive(Clone, Debug, Default)]
pub struct StatLine {
    /// `breakdown` (nil → `None`; `""` is a key)
    pub breakdown: Option<String>,
    /// `modNames`
    pub modNames: Option<&'static [&'static str]>,
    /// `actorName` ("player" / "minion")
    pub actorName: Option<&'static str>,
    /// `ignoredSections`: the set `{ [label] = true }`
    pub ignoredSections: Option<Rc<Vec<String>>>,
    /// `breakdownColour`
    pub breakdownColour: Option<[f64; 3]>,
}

/// One `self.breakdownIndex[key]`: `{ colData = colData, modSectionsByName = modSectionsByName }`.
#[derive(Clone)]
pub struct BreakdownEntry {
    /// the column whose cell names the breakdown (a table of cells)
    pub colData: Table,
    /// mod name → the cells naming it (each a copy of its row with the cell's fields)
    pub modSectionsByName: Rc<HashMap<String, Vec<Table>>>,
}

/// `self.breakdownIndex`. LUA-ORDER: only looked up by key.
pub type BreakdownIndex = HashMap<String, BreakdownEntry>;

thread_local! {
    static ESTIMATE_PLAYER_PROGRESS: RefCell<Option<Rc<dyn Fn(&mut Cx)>>> = const { RefCell::new(None) };
    /// `local req = { }` (Build.lua:2184): the module-level list AddRequirementsToTooltip reuses.
    static REQ: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// Registers `buildMode:EstimatePlayerProgress()` (U20, Build.lua:771-1617), which
/// RefreshStatList calls last (L2118). CCR-0103 §2.
pub fn set_estimate_player_progress(f: Option<Rc<dyn Fn(&mut Cx)>>) {
    ESTIMATE_PLAYER_PROGRESS.with(|h| *h.borrow_mut() = f);
}

// ---- accessors -------------------------------------------------------------------------------

/// The buildMode node (`self`).
#[track_caller]
fn node(cx: &Cx) -> CtrlId {
    cx.build().node.id()
}

/// `self.controls[key]` (a missing control is a Lua nil index).
#[track_caller]
fn ctl(cx: &Cx, key: &'static str) -> CtrlId {
    cx.child(node(cx), key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `self.calcsTab.mainEnv`.
#[track_caller]
fn main_env(cx: &Cx) -> model::EnvRef {
    cx.build()
        .model
        .calcsTab
        .mainEnv
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"))
}

/// `self.calcsTab.mainEnv[actorName]` ("player" / "minion"; other names are nil).
fn env_actor(env: &model::Env, actorName: &str) -> Option<ActorRef> {
    match actorName {
        "player" => Some(env.player.clone()),
        "minion" => env.minion.get(),
        _ => None,
    }
}

/// `actor.mainSkill`
#[track_caller]
fn main_skill(actor: &Actor) -> ActiveSkillRef {
    actor
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `matchFlags(statData.flag, statData.notFlag, skillFlags)` (Build.lua:32-62).
fn stat_match_flags(statData: &DisplayStat, skillFlags: &Table) -> bool {
    let flag = statData.flag.map(|f| [f]);
    let not_flag = statData.notFlag.map(|f| [f]);
    match_flags(
        flag.as_ref().map(|f| &f[..]),
        not_flag.as_ref().map(|f| &f[..]),
        |name| skillFlags.truthy(Name::from(name)),
    )
}

/// `t_insert(self.controls.statBox.list, row)` (`statBoxList` aliases the control's list).
fn push_row(cx: &mut Cx, stat_box: CtrlId, row: TextListRow) {
    cx.w_mut::<TextListControl>(stat_box)
        .list
        .push(Rc::new(row));
}

/// A row with a single centred line (`{ height = h, align = "CENTER_X", x = 140, text }`).
fn centred_row(height: f64, text: Option<String>) -> TextListRow {
    TextListRow {
        height,
        cells: vec![text],
        x: Some(140.0),
        align: Some(Align::CenterX),
        ..TextListRow::default()
    }
}

/// `InsertIfNew(self.controls.warnings.lines, v)`
fn insert_warning(cx: &mut Cx, v: String) {
    let w = ctl(cx, "warnings");
    insert_if_new(Some(&mut cx.w_mut::<WarningsControl>(w).lines), v);
}

/// The operand of `..` (string or number; anything else raises).
#[track_caller]
fn concat(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `tostring(v)` for the values Save writes.
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        Val::Table(t) => format!("table: {:p}", t.as_ptr()),
        other => format!("{}: {:p}", other.lua_type(), other as *const Val),
    }
}

/// `x * y` on Lua values.
#[track_caller]
fn mul(a: &Val, b: &Val) -> f64 {
    a.num() * b.num()
}

/// `s_format("%" .. statData.fmt, ...)`: a nil `fmt` is the concatenation error.
#[track_caller]
fn fmt_of(statData: &DisplayStat) -> &'static str {
    statData
        .fmt
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'fmt' (a nil value)"))
}

/// `valStr:match("^([%+%-]?%d+%.%d+)(%D*)$")` and `number:gsub("0+$", ""):gsub("%.$", "") .. suffix`
/// (FormatStat L1741-1744, CompareStatList L2138-2141).
fn strip_trailing_zeros(valStr: String) -> String {
    let caps = pob_lua::lmatch(&valStr, "^([%+%-]?%d+%.%d+)(%D*)$", None)
        .map(|c| (c[0].to_str().into_owned(), c[1].to_str().into_owned()));
    if let Some((number, suffix)) = caps {
        let n = pob_lua::gsub(&number, "0+$", "", None).0;
        let n = pob_lua::gsub(&n, "%.$", "", None).0;
        n + &suffix
    } else {
        valStr
    }
}

// ---- FormatStat ------------------------------------------------------------------------------

/// lua: Build.lua:1718-1755 `buildMode:FormatStat(statData, statVal, overCapStatVal, colorOverride)`.
/// The `last*` writes go to the build node's [`State`] (no build, no node: nothing to write;
/// only a build's OnFrame reads them).
/// @pob Modules/Build.lua:1718-1755 h=a7e1949e
pub fn format_stat(
    cx: &mut Cx,
    statData: &DisplayStat,
    statVal: &Val,
    overCapStatVal: &Val,
    colorOverride: Option<&str>,
) -> String {
    // L1719
    if matches!(statVal, Val::Table(_) | Val::Data(_)) {
        return String::new();
    }
    // L1720
    let val = statVal.num()
        * (if statData.pc || statData.r#mod {
            100.0
        } else {
            1.0
        })
        - (if statData.r#mod { 100.0 } else { 0.0 });
    // L1721
    let mut color = match colorOverride {
        Some(c) => c.to_owned(),
        None => {
            if display_stats::le(&Val::Num(0.0), statVal) {
                "^7".to_owned()
            } else if statData.chaosInoc {
                "^8".to_owned()
            } else {
                cx.main.cc.NEGATIVE.clone()
            }
        }
    };
    // L1722
    if statData.label == Some("Unreserved Life") && statVal.raw_eq(&Val::Num(0.0)) {
        // L1723
        color = cx.main.cc.NEGATIVE.clone();
    }

    // L1726
    let mut valStr: Option<String> = None;
    // L1727
    if statData.compactValue && cx.app().main.use_compact_values && val != m_huge && val != -m_huge
    {
        // L1728
        let absVal = m_abs(val);
        // L1729
        if absVal >= 1000000000.0 {
            // L1730
            valStr = Some(lfmt!("%.1fB", val / 1000000000.0));
        } else if absVal >= 1000000.0 {
            // L1732
            valStr = Some(lfmt!("%.1fM", val / 1000000.0));
        } else if absVal >= 10000.0 {
            // L1734
            valStr = Some(lfmt!("%.1fK", val / 1000.0));
        }
    }
    // L1737
    #[allow(clippy::single_match_else)]
    let mut valStr = match valStr {
        Some(s) => s,
        None => {
            // L1738
            let s = lfmt!(&format!("%{}", fmt_of(statData)), val);
            // L1739-1742
            strip_trailing_zeros(s)
        }
    };
    // L1745
    valStr = color + &format_num_sep_main(&cx.main, &valStr);

    // L1747
    if overCapStatVal.truthy() && display_stats::lt(&Val::Num(0.0), overCapStatVal) {
        // L1748
        valStr = valStr + "^x808080" + " (+" + &lfmt!("%d", overCapStatVal.num()) + "%)";
    }
    // L1750-1754
    let show_thousands = cx.main.show_thousands_separators;
    let thousands = cx.main.thousands_separator.clone();
    let decimal = cx.main.decimal_separator.clone();
    let titlebar = cx.app().main.show_titlebar_name;
    let compact = cx.app().main.use_compact_values;
    if let Some(n) = cx.app().build.as_ref().map(|b| b.node.id()) {
        let st = &mut cx.w_mut::<BuildMode>(n).stats;
        st.lastShowThousandsSeparators = Some(show_thousands);
        st.lastShowThousandsSeparator = Some(thousands);
        st.lastShowDecimalSeparator = Some(decimal);
        st.lastShowTitlebarName = Some(titlebar);
        st.lastUseCompactValues = Some(compact);
    }
    // L1754
    valStr
}

// ---- breakdown index -------------------------------------------------------------------------

/// A dumped `CalcSections` value as a calc value (tables become fresh calc tables).
fn data_to_val(v: &model::gamedata::DataValue) -> Val {
    use model::gamedata::DataValue;
    match v {
        DataValue::Nil => Val::Nil,
        DataValue::Bool(b) => Val::Bool(*b),
        DataValue::Num(n) => Val::Num(*n),
        DataValue::Str(s) => Val::from(&**s),
        DataValue::Table(t) => {
            let out = Table::new();
            for (k, x) in &t.fields {
                out.set(Name::from(&k[..]), data_to_val(x));
            }
            for x in &t.seq {
                out.push(data_to_val(x));
            }
            for (k, x) in &t.num {
                out.set(*k, data_to_val(x));
            }
            Val::Table(out)
        }
        DataValue::Ref(p) => data_to_val(&resolve_calc_sections_ref(p)),
        // CalcSections holds no functions (the update function, [6], is CalcsTab's own)
        DataValue::Fn(_) => Val::Nil,
    }
}

/// A `$ref` inside the CalcSections dump (`CalcSections[1][5][1].data[6][2][2].modName`).
fn resolve_calc_sections_ref(path: &str) -> model::gamedata::DataValue {
    use model::gamedata::DataValue;
    let rest = path
        .strip_prefix("CalcSections")
        .unwrap_or_else(|| lua_error!("CalcSections: unresolvable reference {path}"));
    let mut cur = model::data().calcSections.0.clone();
    let b = rest.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let next = match b[i] {
            b'[' => {
                let j = rest[i..].find(']').map(|j| i + j).unwrap_or(b.len());
                let n: f64 = rest[i + 1..j].parse().unwrap_or(f64::NAN);
                i = j + 1;
                cur.table().map(|t| t.index(n).clone())
            }
            b'.' => {
                let j = rest[i + 1..]
                    .find(['.', '['])
                    .map_or(b.len(), |j| i + 1 + j);
                let k = &rest[i + 1..j];
                i = j;
                cur.table().map(|t| t.field(k).clone())
            }
            _ => None,
        };
        cur = match next {
            Some(DataValue::Ref(p)) => resolve_calc_sections_ref(&p),
            Some(v) => v,
            None => lua_error!("CalcSections: unresolvable reference {path}"),
        };
    }
    cur
}

/// `self.calcsTab.sectionList[i].subSection` for every section, in order (see the module doc).
fn section_subsections() -> Vec<Table> {
    let mut out = Vec::new();
    // CalcsTab.lua:45-144: the SkillSelect section; only its rows 142 and 144 hold cells
    let skill_select = Table::new();
    let sub = Table::new();
    sub.set(Name::from("label"), "View Skill Details");
    let data = Table::new();
    for (label, flag, format, breakdown) in [
        (
            "Aura and Buff Skills",
            "buffs",
            "{output:BuffList}",
            Some("SkillBuffs"),
        ),
        ("Combat Buffs", "combat", "{output:CombatList}", None),
        (
            "Curses and Debuffs",
            "effective",
            "{output:CurseList}",
            Some("SkillDebuffs"),
        ),
    ] {
        let row = Table::new();
        row.set(Name::from("label"), label);
        row.set(Name::from("flag"), flag);
        row.set(Name::from("textSize"), 12.0);
        let col = Table::new();
        col.set(Name::from("format"), format);
        if let Some(b) = breakdown {
            let cell = Table::new();
            cell.set(Name::from("breakdown"), b);
            col.push(cell);
        }
        row.push(col);
        data.push(row);
    }
    sub.set(Name::from("data"), data);
    skill_select.push(sub);
    out.push(skill_select);
    // CalcsTab.lua:150-154: the CalcSections module's sections, `unpack(section)`'s 5th value
    if let Val::Table(sections) = data_to_val(&model::data().calcSections.0) {
        for (_, section) in sections.ipairs() {
            if let Some(s) = section.get(5).table() {
                out.push(s);
            }
        }
    }
    out
}

/// lua: Build.lua:1764-1810 `buildMode:BuildBreakdownIndex()`
///
/// lazily index the calcs tab sections so that we can look up the matching sections by
/// breakdown name. this function works by iterating through calc sections. it will link every
/// breakdown key to the first found calc section column (basically, a box). the result will
/// contain references to this column data and will also contain a set of mod names referred to
/// by cells of said box. if a cell shouldn't be picked, the field `skipSideBarIndex` can be set
/// on the cell.
/// @pob Modules/Build.lua:1764-1810 h=fba45532
pub fn build_breakdown_index(cx: &mut Cx) -> Rc<BreakdownIndex> {
    // L1765
    if let Some(index) = cx.w::<BuildMode>(node(cx)).stats.breakdownIndex.clone() {
        // L1766
        return index;
    }
    // L1768
    let mut index = BreakdownIndex::new();
    // L1769-1770
    for subSections in section_subsections() {
        // L1771 (`section.subSection` is always set for these sections)
        // L1772
        for (_, subSection) in subSections.ipairs() {
            // cells for each mod name
            // L1774
            let mut modSectionsByName: HashMap<String, Vec<Table>> = HashMap::new();
            // L1775
            let mut breakdownCells: HashMap<String, Table> = HashMap::new();
            // L1777
            let subSectionData = subSection.get(Name::from("data"));
            // L1778
            for (_, row) in ipairs_val(&subSectionData) {
                // L1779
                for (_, colData) in ipairs_val(&row) {
                    // L1780
                    for (_, cell) in ipairs_val(&colData) {
                        // L1781
                        let b = cell.get(Name::from("breakdown"));
                        if !cell.get(Name::from("skipSideBarIndex")).truthy() && b.truthy() {
                            let key = concat(&b);
                            if let std::collections::hash_map::Entry::Vacant(e) =
                                breakdownCells.entry(key)
                            {
                                // L1782
                                e.insert(colData.table().unwrap_or_else(|| {
                                    lua_error!("attempt to index local 'colData'")
                                }));
                            }
                        }
                        // L1784
                        let modName = cell.get(Name::from("modName"));
                        if modName.truthy() {
                            // inherit flags from parent row
                            // L1786
                            let row_t = row.table().unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'pairs' (table expected)")
                            });
                            let copy = row_t.copyTable(true);
                            // L1787-1789
                            let cell_t = cell.table().unwrap_or_else(|| {
                                lua_error!("bad argument #1 to 'pairs' (table expected)")
                            });
                            for (k, v) in cell_t.pairs() {
                                copy.set(k, v);
                            }
                            // L1790
                            let names: Vec<Val> = match &modName {
                                Val::Table(t) => t.ipairs().into_iter().map(|(_, v)| v).collect(),
                                other => vec![other.clone()],
                            };
                            // L1791
                            for name in names {
                                // L1792-1793
                                modSectionsByName
                                    .entry(concat(&name))
                                    .or_default()
                                    .push(copy.clone());
                            }
                        }
                    }
                }
            }
            // L1799
            let modSectionsByName = Rc::new(modSectionsByName);
            for (key, colData) in breakdownCells {
                // ensure that we only set each breakdown once
                // L1801
                index.entry(key).or_insert_with(|| BreakdownEntry {
                    colData,
                    modSectionsByName: modSectionsByName.clone(),
                });
            }
        }
    }
    // L1808
    let index = Rc::new(index);
    let n = node(cx);
    cx.w_mut::<BuildMode>(n).stats.breakdownIndex = Some(index.clone());
    // L1809
    index
}

/// `ipairs(v)` over a value (nil and non-tables raise like `ipairs`).
fn ipairs_val(v: &Val) -> Vec<(usize, Val)> {
    match v {
        Val::Table(t) => t.ipairs(),
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `{ a, b, ... }` of strings.
fn str_list(items: &[&str]) -> Table {
    let t = Table::new();
    for s in items {
        t.push(*s);
    }
    t
}

/// lua: Build.lua:1816-1869 `buildMode:GetSidebarBreakdown(key, modNames, ignoredSections, actorName)`.
/// `key`: a breakdown key; `modNames`: required if the mod name is different from the
/// breakdown key (e.g. breakdown has Time while the mod has Speed); `ignoredSections` is the set
/// of labels to drop. Returns the display data list (`self.sidebarBreakdownData`).
/// @pob Modules/Build.lua:1816-1869 h=60c3d9f5
pub fn get_sidebar_breakdown(
    cx: &mut Cx,
    key: Option<&str>,
    modNames: Option<&[&str]>,
    ignoredSections: Option<&[String]>,
    actorName: Option<&str>,
) -> Table {
    let key_val = key.map_or(Val::Nil, Val::from);
    // L1817
    let env = actorName.and_then(|a| env_actor(&main_env(cx), a));
    // L1818
    let breakdownData = env
        .and_then(|e| e.breakdown.get())
        .map_or(Val::Nil, |b| match key {
            Some(k) => bd_val(b.get(Name::from(k))),
            // reading `t[nil]` is nil
            None => Val::Nil,
        });
    // L1819
    let sourceKey: Val = if breakdownData.truthy() {
        let s = breakdownData.get(Name::from("breakdownSource"));
        if s.truthy() {
            s
        } else {
            key_val.clone()
        }
    } else {
        key_val.clone()
    };
    // L1820
    let index = build_breakdown_index(cx);
    let entry = match &sourceKey {
        Val::Str(s) => index.get(&**s).cloned(),
        _ => None,
    };
    // L1821
    let Some(entry) = entry else {
        // L1822
        let output = Table::new();
        let first = Table::new();
        first.set(Name::from("breakdown"), key_val.clone());
        output.push(first);
        // it's possible for calc sections to not have a breakdown, while still
        // having a mod list. this handles e.g. spell suppression chance
        // L1825
        if let Some(modNames) = modNames {
            // L1826
            let t = Table::new();
            t.set(Name::from("modName"), str_list(modNames));
            output.push(t);
        }
        // L1828
        return output;
    };
    // L1830
    let displayData = Table::new();
    // this will include all of the hover information for the selected breakdown
    // L1832
    for (_, cell) in entry.colData.ipairs() {
        // L1833
        let displayCell = match &cell {
            Val::Table(t) => t.copyTable(false),
            other => lua_error!("attempt to index a {} value (copyTable)", other.lua_type()),
        };
        // L1834
        if displayCell.get(Name::from("breakdown")).raw_eq(&sourceKey) {
            // L1835
            displayCell.set(Name::from("breakdown"), key_val.clone());
        }
        // L1837
        displayData.push(displayCell);
    }
    // sometimes the breakdown doesn't include the mod names. for
    // example life has its mod tables separated to the inc and more breakdowns.
    // L1841
    if let Some(modNames) = modNames {
        // L1842
        for modKey in modNames {
            // L1843
            if let Some(list) = entry.modSectionsByName.get(*modKey) {
                // L1844
                for statDisplay in list {
                    // L1845
                    displayData.push(statDisplay.clone());
                }
            }
        }
    }

    // L1851
    let mut visitedSections: Vec<String> = Vec::new();
    // remove sections that have been manually excluded, or ones that have
    // already been added. it's probably not guaranteed that the sections with
    // the same label always contain the exact same contents, but this gets rid
    // of pointless repeated sections
    // L1856
    let mut i = displayData.len() as i64;
    while i >= 1 {
        // L1857
        let label = displayData.get(i).get(Name::from("label"));
        // L1858
        if label.truthy() {
            let l = concat(&label);
            // L1859
            if visitedSections.contains(&l) || ignoredSections.is_some_and(|s| s.contains(&l)) {
                // L1860
                displayData.remove(Some(i));
            }
            // L1862
            if !visitedSections.contains(&l) {
                visitedSections.push(l);
            }
        }
        i -= 1;
    }
    // L1865
    if displayData.is_empty() {
        // L1866
        let output = Table::new();
        let first = Table::new();
        first.set(Name::from("breakdown"), key_val);
        output.push(first);
        return output;
    }
    // L1868
    displayData
}

/// A value read from a breakdown table, with a nested breakdown instance as its data table.
/// `breakdown.MainHand` / `.OffHand` (CalcOffence.lua:2006/2023) are module instances, plain
/// tables in Lua; the calc stores them as `Val::Obj(Breakdown)` (costs_passes.rs
/// `newSubBreakdown`), which `Val::get` / `#` would raise on.
fn bd_val(v: Val) -> Val {
    match v.obj::<pob_calc::breakdown::Breakdown>() {
        Some(b) => Val::Table(b.t.clone()),
        None => v,
    }
}

/// `#t` of a breakdown value (only tables have a length here).
#[track_caller]
fn len_of(v: &Val) -> usize {
    match v {
        Val::Table(t) => t.len(),
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// lua: Build.lua:1872-1908 `buildMode:GetStatBreakdownKey(statData, actorName)`
/// @pob Modules/Build.lua:1872-1908 h=6fd998d0
pub fn get_stat_breakdown_key(cx: &Cx, statData: &DisplayStat, actorName: &str) -> Option<String> {
    // L1873
    let env = env_actor(&main_env(cx), actorName);
    // L1874
    let breakdown = env.and_then(|e| e.breakdown.get())?;
    // L1875-1877
    let get = |k: &str| bd_val(breakdown.get(Name::from(k)));
    // L1878
    let key: Option<String>;
    // L1879
    if let Some(b) = statData.breakdown {
        // L1880
        key = Some(b.to_owned());
    } else if let Some(childStat) = statData.childStat {
        // (`breakdown[nil]` reads nil; every row with a childStat has a stat)
        let stat = statData.stat.unwrap_or_default();
        // L1882
        let parent = if statData.stat.is_some() {
            get(stat)
        } else {
            Val::Nil
        };
        // L1883
        if parent.truthy() && parent.get(Name::from(childStat)).truthy() {
            // L1884
            key = Some(format!("{stat}.{childStat}"));
        } else {
            key = None;
        }
    } else if statData.stat.is_some_and(|s| get(s).truthy()) {
        // L1887
        key = statData.stat.map(str::to_owned);
    } else {
        key = None;
    }
    // L1889
    let key = key?;
    // Modifier-only popups do not require a calculated breakdown.
    // L1893
    if statData.modNames.is_some() {
        // L1894
        return Some(key);
    }
    // L1896
    let caps = pob_lua::lmatch(&key, "^(%a+)%.(%a+)$", None)
        .map(|c| (c[0].to_str().into_owned(), c[1].to_str().into_owned()));
    // L1897
    let breakdownData = {
        let a = match &caps {
            Some((namespace, name)) => {
                let ns = get(namespace);
                if ns.truthy() {
                    bd_val(ns.get(Name::from(name.as_str())))
                } else {
                    ns
                }
            }
            None => Val::Nil,
        };
        if a.truthy() {
            a
        } else {
            get(&key)
        }
    };
    // L1898-1904
    let nonempty = |k: &str| {
        let v = breakdownData.get(Name::from(k));
        v.truthy() && len_of(&v) > 0
    };
    if breakdownData.truthy()
        && (len_of(&breakdownData) > 0
            || breakdownData.get(Name::from("radius")).truthy()
            || nonempty("rowList")
            || nonempty("reservations")
            || nonempty("damageTypes")
            || nonempty("slots")
            || nonempty("modList"))
    {
        // L1905
        return Some(key);
    }
    // L1907
    None
}

// ---- the stat box ----------------------------------------------------------------------------

/// SimpleGraphic also has these short codes which cannot be easily translated to hex
/// (Build.lua:1989-1992 `colourCodeMap`).
fn colour_code_map(code: &str) -> Option<[f64; 3]> {
    match code {
        "^1" => Some([0.917, 0.0, 0.0]),
        "^7" => Some([1.0, 1.0, 1.0]),
        _ => None,
    }
}

/// lua: Build.lua:1913-2059 `buildMode:AddDisplayStatList(statList, actor, actorName)`: add
/// stat list for given actor.
/// @pob Modules/Build.lua:1913-2059 h=327174fc
pub fn add_display_stat_list(
    cx: &mut Cx,
    statList: &[DisplayStat],
    actor: &ActorRef,
    actorName: &'static str,
) {
    // L1914
    let stat_box = ctl(cx, "statBox");
    // L1915
    for statData in statList {
        // L1916
        let skillFlags = main_skill(actor).skillFlags.get();
        if stat_match_flags(statData, &skillFlags) {
            // L1917-1920
            let mut labelColor = "^7".to_owned();
            if let Some(c) = statData.color(&cx.main.cc) {
                labelColor = c;
            }
            // L1921
            if let Some(stat) = statData.stat {
                let output = actor.output.get();
                let o = Val::Table(output.clone());
                // L1922
                let mut statVal = output.get(Name::from(stat));
                // access output values that are one node deeper (statData.stat is a table e.g. output.MainHand.Accuracy vs output.Life)
                // L1924
                if statVal.truthy() {
                    if let Some(childStat) = statData.childStat {
                        // L1925
                        statVal = statVal.get(Name::from(childStat));
                    }
                }
                // L1927
                let shown = statVal.truthy()
                    && match statData.condFunc {
                        Some(f) => f(&statVal, &o).truthy(),
                        None => !statVal.raw_eq(&Val::Num(0.0)),
                    };
                if shown {
                    // L1928
                    let mut overCapStatVal = match statData.overCapStat {
                        Some(k) => output.get(Name::from(k)),
                        None => Val::Nil,
                    };
                    // L1929
                    if overCapStatVal.truthy() {
                        if let Some(f) = statData.overCapStatCondFunc {
                            if !f(&statVal, &o).truthy() {
                                // L1930
                                overCapStatVal = Val::Nil;
                            }
                        }
                    }
                    // L1932
                    if stat == "SkillDPS" {
                        // L1933
                        labelColor = cx.main.cc.CUSTOM.clone();
                        // L1934
                        sort_skill_dps(&output);
                        // L1935
                        let list = output.get(Name::from("SkillDPS"));
                        for (_, skillData) in ipairs_val(&list) {
                            let field = |k: &str| skillData.get(Name::from(k));
                            // L1936
                            let mut triggerStr = String::new();
                            // L1937
                            let trigger = field("trigger");
                            if trigger.truthy() && !trigger.raw_eq(&Val::from("")) {
                                // L1938
                                triggerStr = format!(
                                    "{} ({}){}",
                                    cx.main.cc.WARNING,
                                    concat(&trigger),
                                    labelColor
                                );
                            }
                            // L1940
                            let mut lhsString =
                                format!("{labelColor}{}{triggerStr}:", concat(&field("name")));
                            // L1941
                            if display_stats::le(&Val::Num(2.0), &field("count")) {
                                // L1942
                                lhsString = format!(
                                    "{labelColor}{}x {}{triggerStr}:",
                                    concat(&field("count")),
                                    concat(&field("name"))
                                );
                            }
                            // L1947
                            let fmtData = DisplayStat {
                                fmt: Some(".1f"),
                                compactValue: statData.compactValue,
                                ..DisplayStat::EMPTY
                            };
                            let v = Val::Num(mul(&field("dps"), &field("count")));
                            let formatted = format_stat(cx, &fmtData, &v, &overCapStatVal, None);
                            // L1944-1948
                            push_row(
                                cx,
                                stat_box,
                                TextListRow {
                                    height: 16.0,
                                    cells: vec![Some(lhsString), Some(formatted)],
                                    ..TextListRow::default()
                                },
                            );
                            // L1949
                            let skillPart = field("skillPart");
                            if skillPart.truthy() {
                                // L1950-1954
                                push_row(
                                    cx,
                                    stat_box,
                                    centred_row(14.0, Some(format!("^8{}", concat(&skillPart)))),
                                );
                            }
                            // L1956
                            let source = field("source");
                            if source.truthy() {
                                // L1957-1961
                                let text =
                                    format!("{}from {}", cx.main.cc.WARNING, concat(&source));
                                push_row(cx, stat_box, centred_row(14.0, Some(text)));
                            }
                        }
                    } else if !statData.hideStat {
                        // Change the color of the stat label to red if cost exceeds pool
                        // L1966
                        let mut colorOverride: Option<String> = None;
                        // L1967
                        let warn = output.get(Name::from(format!("{stat}Warning").as_str()));
                        if warn.truthy()
                            || (statData.warnFunc.is_some_and(|f| f(&statVal, &o).truthy())
                                && statData.warnColor)
                        {
                            // L1968
                            colorOverride = Some(cx.main.cc.NEGATIVE.clone());
                        }
                        // L1970
                        let mut formattedStat = format_stat(
                            cx,
                            statData,
                            &statVal,
                            &overCapStatVal,
                            colorOverride.as_deref(),
                        );
                        // L1971
                        if let Some(suffix) = statData.suffix {
                            if statData
                                .suffixCondFunc
                                .map_or(true, |f| f(&statVal, &o).truthy())
                            {
                                // L1972-1975
                                let suffix = match suffix {
                                    Suffix::Str(s) => Val::from(s),
                                    Suffix::Fn(f) => f(&statVal, &o),
                                };
                                // L1976
                                if suffix.truthy() {
                                    // L1977
                                    formattedStat =
                                        formattedStat + "^x808080 (" + &concat(&suffix) + ")";
                                }
                            }
                        }
                        // L1980
                        let breakdown = get_stat_breakdown_key(cx, statData, actorName);
                        // L1981-1987
                        let ignoredSections = statData
                            .ignoredSections
                            .map(|l| Rc::new(l.iter().map(|s| (*s).to_owned()).collect()));
                        // L1993
                        let breakdownColour = if labelColor != "^7" {
                            hex_to_rgb(&labelColor)
                        } else {
                            None
                        }
                        .or_else(|| colour_code_map(&labelColor));
                        // L1994-2004
                        let label = statData.label.unwrap_or_else(|| {
                            lua_error!("attempt to concatenate field 'label' (a nil value)")
                        });
                        let underline = breakdown.is_some();
                        push_row(
                            cx,
                            stat_box,
                            TextListRow {
                                height: 16.0,
                                cells: vec![
                                    Some(format!("{labelColor}{label}:")),
                                    Some(formattedStat),
                                ],
                                underline: Some(vec![false, underline]),
                                extra: Some(Rc::new(StatLine {
                                    breakdown,
                                    modNames: statData.modNames,
                                    actorName: Some(actorName),
                                    ignoredSections,
                                    breakdownColour,
                                })),
                                ..TextListRow::default()
                            },
                        );
                    }
                }
                // L2007
                if let Some(warnFunc) = statData.warnFunc {
                    if statVal.truthy()
                        && match statData.condFunc {
                            Some(f) => f(&statVal, &o).truthy(),
                            None => true,
                        }
                    {
                        // L2008
                        let v = warnFunc(&statVal, &o);
                        // L2009
                        if v.truthy() {
                            // L2010
                            insert_warning(cx, concat(&v));
                        }
                    }
                }
            } else if statData.label.is_some()
                && statData
                    .condFunc
                    .is_some_and(|f| f(&Val::Table(actor.output.get()), &Val::Nil).truthy())
            {
                // L2014-2016 (the label-only variant: condFunc receives the output first)
                let output = actor.output.get();
                let labelStat = statData
                    .labelStat
                    .map_or(Val::Nil, |k| output.get(Name::from(k)));
                let val = statData.val.map_or(Val::Nil, Val::from);
                let text = format!("^7{}%^x808080 ({})", concat(&labelStat), concat(&val));
                push_row(
                    cx,
                    stat_box,
                    TextListRow {
                        height: 16.0,
                        cells: vec![
                            Some(format!(
                                "{labelColor}{}:",
                                statData.label.unwrap_or_default()
                            )),
                            Some(text),
                        ],
                        ..TextListRow::default()
                    },
                );
            } else {
                // L2017
                let last_has_text = cx
                    .w::<TextListControl>(stat_box)
                    .list
                    .last()
                    .map(|r| r.cell(1).is_some());
                if last_has_text != Some(false) {
                    // L2018
                    push_row(
                        cx,
                        stat_box,
                        TextListRow {
                            height: 6.0,
                            ..TextListRow::default()
                        },
                    );
                }
            }
        }
    }
    let output = actor.output.get();
    // L2022-2032 (ORDER: see the module doc)
    for (pool, warningFlag) in [
        ("Life", "LifeCostWarningList"),
        ("Mana", "ManaCostWarningList"),
        ("Rage", "RageCostWarningList"),
        ("Energy Shield", "ESCostWarningList"),
    ] {
        // L2023
        let list = output.get(Name::from(warningFlag));
        if list.truthy() {
            // L2024
            let what = if output.truthy(Name::from("EnergyShieldProtectsMana")) && pool == "Mana" {
                "Energy Shield and Mana"
            } else {
                pool
            };
            let mut line = format!("You do not have enough {what} to use: ");
            // L2025-2027
            for (_, skill) in ipairs_val(&list) {
                line = line + &concat(&skill) + ", ";
            }
            // L2028
            line = pob_lua::string::sub(&line, 1, Some(-3)).into_owned();
            // L2029
            insert_warning(cx, line);
        }
    }
    // L2033-2043 (ORDER: see the module doc)
    for (pool, warningFlag) in [
        ("Unreserved life", "LifePercentCostPercentCostWarningList"),
        ("Unreserved Mana", "ManaPercentCostPercentCostWarningList"),
    ] {
        // L2034
        let list = output.get(Name::from(warningFlag));
        if list.truthy() {
            // L2035
            let mut line = format!("You do not have enough {pool}% to use: ");
            // L2036-2038
            for (_, skill) in ipairs_val(&list) {
                line = line + &concat(&skill) + ", ";
            }
            // L2039
            line = pob_lua::string::sub(&line, 1, Some(-3)).into_owned();
            // L2040
            insert_warning(cx, line);
        }
    }
    // L2044
    if output.truthy(Name::from("VixensTooMuchCastSpeedWarn")) {
        // L2045
        insert_warning(cx, "You may have too much cast speed or too little cooldown reduction to effectively use Vixen's Curse replacement".to_owned());
    }
    // L2047
    if output.truthy(Name::from("VixenModeNoVixenGlovesWarn")) {
        // L2048
        insert_warning(cx, "Vixen's calculation mode for Doom Blast is selected but you do not have Vixen's Entrapment Embroidered Gloves equipped".to_owned());
    }

    // L2051
    {
        let modDB = actor.modDB.get();
        let flag = |n: &str| modDB.Flag(None, &[Name::from(n)]);
        // L2052
        let mut aspectCount = 0.0;
        // L2053
        aspectCount +=
            if display_stats::lt(&Val::Num(0.0), &output.get(Name::from("CrabBarriersMax")))
                && display_stats::lt(&Val::Num(0.0), &output.get(Name::from("CrabBarriers")))
            {
                1.0
            } else {
                0.0
            };
        // L2054
        aspectCount += if aspectCount < 2.0 && flag("Condition:AspectOfTheSpiderActive") {
            1.0
        } else {
            0.0
        };
        // L2055
        aspectCount += if aspectCount < 2.0
            && (flag("Condition:CatsAgilityActive") || flag("Condition:CatsStealthActive"))
        {
            1.0
        } else {
            0.0
        };
        // L2056
        aspectCount += if aspectCount < 2.0
            && (flag("Condition:AviansFlightActive") || flag("Condition:AviansMightActive"))
        {
            1.0
        } else {
            0.0
        };
        // L2057
        if aspectCount > 1.0 {
            // L2058
            insert_warning(cx, "You have more than one Aspect skill active".to_owned());
        }
    }
}

/// `table.sort(actor.output.SkillDPS, function(a,b) return (a.dps * a.count) > (b.dps * b.count) end)`
/// (L1934): sorts the output's list in place.
fn sort_skill_dps(output: &Output) {
    let list = output.get(Name::from("SkillDPS"));
    let Val::Table(t) = list else {
        lua_error!(
            "bad argument #1 to 'sort' (table expected, got {})",
            list.lua_type()
        )
    };
    let n = t.len();
    let mut v: Vec<Val> = (1..=n).map(|i| t.get(i)).collect();
    let key = |x: &Val| mul(&x.get(Name::from("dps")), &x.get(Name::from("count")));
    pob_lua::sort(&mut v, |a, b| key(a) > key(b));
    for (i, x) in v.into_iter().enumerate() {
        t.set(i + 1, x);
    }
}

/// lua: Build.lua:2061-2075 `buildMode:InsertItemWarnings()`
/// @pob Modules/Build.lua:2061-2075 h=84833678
pub fn insert_item_warnings(cx: &mut Cx) {
    let itemWarnings = main_env(cx).itemWarnings.get();
    // L2062
    let jewel = itemWarnings.get(Name::from("jewelLimitWarning"));
    if jewel.truthy() {
        // L2063
        for (_, warning) in ipairs_val(&jewel) {
            // L2064
            insert_warning(
                cx,
                format!(
                    "You are exceeding jewel limit with the jewel {}",
                    concat(&warning)
                ),
            );
        }
    }
    // L2067
    let socket = itemWarnings.get(Name::from("socketLimitWarning"));
    if socket.truthy() {
        // L2068
        for (_, warning) in ipairs_val(&socket) {
            // L2069
            insert_warning(
                cx,
                format!("You have too many gems in your {} slot", concat(&warning)),
            );
        }
    }
    // L2072
    let anoint = itemWarnings.get(Name::from("missingAnointWarning"));
    if anoint.truthy() {
        // L2073 table.concat(list, ", ")
        let parts: Vec<String> = ipairs_val(&anoint).iter().map(|(_, v)| concat(v)).collect();
        insert_warning(
            cx,
            format!(
                "You have eligible items missing an anoint: {}",
                parts.join(", ")
            ),
        );
    }
}

/// `t_insert(statBoxList, { height = 14, align = "CENTER_X", x = 140, colorCodes.CUSTOM .. line})`
/// for a main skill's `infoMessage`, split on `:` when longer than 40 bytes (L2082-2088,
/// L2097-2104).
fn push_info_message(cx: &mut Cx, stat_box: CtrlId, msg: &str) {
    let custom = cx.main.cc.CUSTOM.clone();
    // L2082 / L2098
    if msg.len() > 40 {
        // L2083 / L2099
        let lines: Vec<String> = pob_lua::gmatch(msg, "([^:]+)")
            .map(|c| c[0].to_str().into_owned())
            .collect();
        for line in lines {
            // L2084 / L2100
            push_row(
                cx,
                stat_box,
                centred_row(14.0, Some(format!("{custom}{line}"))),
            );
        }
    } else {
        // L2087 / L2103
        push_row(
            cx,
            stat_box,
            centred_row(14.0, Some(format!("{custom}{msg}"))),
        );
    }
}

/// lua: Build.lua:2078-2119 `buildMode:RefreshStatList()`: build list of side bar stats.
/// @pob Modules/Build.lua:2078-2119 h=8b5c7e02
pub fn refresh_stat_list(cx: &mut Cx) {
    // L2079
    let w = ctl(cx, "warnings");
    cx.w_mut::<WarningsControl>(w).lines = Vec::new();
    // L2080
    let stat_box = ctl(cx, "statBox");
    cx.w_mut::<TextListControl>(stat_box).list.clear();
    let env = main_env(cx);
    let player_skill = main_skill(&env.player);
    // L2081
    if let Some(msg) = player_skill.infoMessage.get() {
        push_info_message(cx, stat_box, &msg);
        // L2090
        if let Some(msg2) = player_skill.infoMessage2.get() {
            // L2091
            push_row(cx, stat_box, centred_row(14.0, Some(format!("^8{msg2}"))));
        }
    }
    // L2094
    if let Some(minion) = env.minion.get() {
        // L2095
        push_row(cx, stat_box, TextListRow::new(18.0, &["^7Minion:"]));
        let minion_skill = main_skill(&minion);
        // L2096
        if let Some(msg) = minion_skill.infoMessage.get() {
            // Split the line if too long
            push_info_message(cx, stat_box, &msg);
            // L2102
            if let Some(msg2) = minion_skill.infoMessage2.get() {
                // L2103
                push_row(cx, stat_box, centred_row(14.0, Some(format!("^8{msg2}"))));
            }
        }
        // L2107
        let list = display_stats::minionDisplayStats();
        add_display_stat_list(cx, &list, &minion, "minion");
        // L2108
        push_row(
            cx,
            stat_box,
            TextListRow {
                height: 10.0,
                ..TextListRow::default()
            },
        );
        // L2109
        push_row(cx, stat_box, TextListRow::new(18.0, &["^7Player:"]));
    }
    // L2111
    if player_skill.skillFlags.get().truthy(Name::from("disable")) {
        // L2112
        push_row(cx, stat_box, TextListRow::new(16.0, &["^7Skill disabled:"]));
        // L2113
        let reason = player_skill.disableReason.get().map(|s| s.to_string());
        push_row(cx, stat_box, centred_row(14.0, reason));
    }
    // L2115
    let list = display_stats::displayStats();
    add_display_stat_list(cx, &list, &env.player, "player");
    // L2116
    insert_item_warnings(cx);
    // L2117
    // CCR-0106 §4: nothing registered means U20's own (it landed; the seam stays for tests)
    let f = ESTIMATE_PLAYER_PROGRESS.with(|h| h.borrow().clone());
    if let Some(f) = f {
        f(cx);
    } else {
        super::loadouts::estimate_player_progress(cx);
    }
}

// ---- comparisons -----------------------------------------------------------------------------

/// lua: Build.lua:2121-2163 `buildMode:CompareStatList(tooltip, statList, actor, baseOutput,
/// compareOutput, header, nodeCount)`
/// @pob Modules/Build.lua:2121-2163 h=bb18a4c1
#[allow(clippy::too_many_arguments)]
pub fn compare_stat_list(
    cx: &Cx,
    tooltip: &mut Tooltip,
    statList: &[DisplayStat],
    actor: &ActorRef,
    baseOutput: &Output,
    compareOutput: &Output,
    header: &str,
    nodeCount: Option<f64>,
) -> f64 {
    // L2122
    let mut count = 0.0;
    let skillFlags = main_skill(actor).skillFlags.get();
    // L2123
    for statData in statList {
        // L2124
        let Some(stat) = statData.stat else { continue };
        if !(stat_match_flags(statData, &skillFlags)
            && statData.childStat.is_none()
            && stat != "SkillDPS")
        {
            continue;
        }
        // L2125
        let statVal1 = compareOutput.get(Name::from(stat)).num_or(0.0);
        // L2126
        let statVal2 = baseOutput.get(Name::from(stat)).num_or(0.0);
        // L2127
        let mut diff = statVal1 - statVal2;
        // L2128
        if stat == "FullDPS" && !compareOutput.truthy(Name::from(stat)) {
            // L2129
            diff = 0.0;
        }
        // L2131
        let co = Val::Table(compareOutput.clone());
        let bo = Val::Table(baseOutput.clone());
        if (diff > 0.001 || diff < -0.001)
            && statData.condFunc.map_or(true, |f| {
                f(&Val::Num(statVal1), &co).truthy() || f(&Val::Num(statVal2), &bo).truthy()
            })
        {
            // L2132
            if count == 0.0 {
                // L2133
                tooltip.add_line(cx, 14.0, header);
            }
            // L2135
            let color = if (statData.lowerIsBetter && diff < 0.0)
                || (!statData.lowerIsBetter && diff > 0.0)
            {
                cx.main.cc.POSITIVE.clone()
            } else {
                cx.main.cc.NEGATIVE.clone()
            };
            // L2136
            let mult = if statData.pc || statData.r#mod {
                100.0
            } else {
                1.0
            };
            let val = diff * mult;
            // L2137 Can't use self:FormatStat, because it doesn't have %+. Adding that would have complicated a simple function
            let fmt = fmt_of(statData);
            let valStr = lfmt!(&format!("%+{fmt}"), val);
            // L2138-2141
            let valStr = strip_trailing_zeros(valStr);

            // L2143
            let valStr = format_num_sep_main(&cx.main, &valStr);

            // L2145
            let mut line = lfmt!(
                "%s%s %s",
                color.as_str(),
                valStr.as_str(),
                statData.label.unwrap_or_else(|| lua_error!(
                    "bad argument #4 to 'format' (string expected, got nil)"
                ))
            );
            // L2146
            let mut pcPerPt = String::new();
            // L2147
            if statData.compPercent && statVal1 != 0.0 && statVal2 != 0.0 {
                // L2148
                let pc = statVal1 / statVal2 * 100.0 - 100.0;
                // L2149
                line += &lfmt!(" (%+.1f%%)", pc);
                // L2150
                if let Some(nodeCount) = nodeCount {
                    // L2151
                    pcPerPt = lfmt!(" (%+.1f%%)", pc / nodeCount);
                }
            }
            // L2154
            if let Some(nodeCount) = nodeCount {
                // L2155
                line += &lfmt!(
                    &format!(" ^8[%+{fmt}%s per point]"),
                    diff * mult / nodeCount,
                    pcPerPt.as_str()
                );
            }
            // L2157
            tooltip.add_line(cx, 14.0, &line);
            // L2158
            count += 1.0;
        }
    }
    // L2162
    count
}

/// lua: Build.lua:2168-2180 `buildMode:AddStatComparesToTooltip(tooltip, baseOutput,
/// compareOutput, header, nodeCount)`: compare values of all display stats between the two
/// output tables, and add any changed stats to the tooltip. Adds the provided header line
/// before the first stat line, if any are added. Returns the number of stat lines added.
/// @pob Modules/Build.lua:2168-2180 h=f0b2a902
pub fn add_stat_compares_to_tooltip(
    cx: &Cx,
    tooltip: &mut Tooltip,
    baseOutput: &Output,
    compareOutput: &Output,
    header: &str,
    nodeCount: Option<f64>,
) -> f64 {
    // L2169
    let mut count = 0.0;
    let mut header = header.to_owned();
    let env = main_env(cx);
    // L2170
    let baseMinion = baseOutput.get(Name::from("Minion"));
    let compareMinion = compareOutput.get(Name::from("Minion"));
    if main_skill(&env.player).minion.get().is_some()
        && baseMinion.truthy()
        && compareMinion.truthy()
    {
        // L2171
        let minion = env
            .minion
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index local 'actor' (a nil value)"));
        let as_table = |v: &Val| {
            v.table().unwrap_or_else(|| {
                lua_error!(
                    "attempt to index local 'baseOutput' (a {} value)",
                    v.lua_type()
                )
            })
        };
        count += compare_stat_list(
            cx,
            tooltip,
            &display_stats::minionDisplayStats(),
            &minion,
            &as_table(&baseMinion),
            &as_table(&compareMinion),
            &format!("{header}\n^7Minion:"),
            nodeCount,
        );
        // L2172
        if count > 0.0 {
            // L2173
            header = "^7Player:".to_owned();
        } else {
            // L2175
            header += "\n^7Player:";
        }
    }
    // L2178
    count += compare_stat_list(
        cx,
        tooltip,
        &display_stats::displayStats(),
        &env.player,
        baseOutput,
        compareOutput,
        &header,
        nodeCount,
    );
    // L2179
    count
}

/// lua: Build.lua:2183-2220 `buildMode:AddRequirementsToTooltip(tooltip, level, str, dex, int,
/// strBase, dexBase, intBase)`: add requirements to tooltip (`req` is the block's shared list).
/// @pob Modules/Build.lua:2183-2220 h=a6078ab4
#[allow(clippy::too_many_arguments)]
pub fn add_requirements_to_tooltip(
    cx: &Cx,
    tooltip: &mut Tooltip,
    level: Option<f64>,
    str: Option<f64>,
    dex: Option<f64>,
    int: Option<f64>,
    strBase: Option<f64>,
    dexBase: Option<f64>,
    intBase: Option<f64>,
) {
    use crate::main_mode::folders::stat_color;
    let push = |s: String| REQ.with(|r| r.borrow_mut().push(s));
    let model = cx.build().model.clone();
    let mainOutput = || {
        model
            .calcsTab
            .mainOutput
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainOutput' (a nil value)"))
    };
    let out_num = |k: &str| mainOutput().get(Name::from(k));
    // L2186
    if let Some(level) = level.filter(|l| *l > 0.0) {
        // L2187
        push(lfmt!(
            "^x7F7F7FLevel %s%d",
            stat_color(cx, level, None, Some(model.characterLevel.get())).as_str(),
            level
        ));
    }
    // Convert normal attributes to Omni attributes
    // L2190
    let modDB = main_env(cx).modDB.get();
    if modDB.Flag(None, &[Name::from("OmniscienceRequirements")]) {
        // L2191
        let omniSatisfy = modDB.Sum(
            ModType::INC,
            None,
            &[Name::from("OmniAttributeRequirements")],
        );
        // L2192
        let mut highestAttribute = 0.0;
        // L2193
        for stat in [str, dex, int] {
            // L2194
            if stat.unwrap_or(0.0) > highestAttribute {
                // L2195
                highestAttribute = stat.unwrap_or(f64::NAN);
            }
        }
        // L2198
        let omni = m_floor(highestAttribute * (100.0 / omniSatisfy));
        // L2199
        if omni > 0.0 || display_stats::lt(&out_num("Omni"), &Val::Num(omni)) {
            // L2200
            let o = out_num("Omni").opt_num();
            push(lfmt!(
                "%s%d ^x7F7F7FOmni",
                stat_color(cx, omni, Some(0.0), o).as_str(),
                omni
            ));
        }
    } else {
        // L2203-2211
        for (stat, base, key, name) in [
            (str, strBase, "Str", "Str"),
            (dex, dexBase, "Dex", "Dex"),
            (int, intBase, "Int", "Int"),
        ] {
            if let Some(stat) = stat {
                if stat > 14.0 || display_stats::lt(&out_num(key), &Val::Num(stat)) {
                    let limit = out_num(key).opt_num();
                    push(lfmt!(
                        &format!("%s%d ^x7F7F7F{name}"),
                        stat_color(cx, stat, base, limit).as_str(),
                        stat
                    ));
                }
            }
        }
    }
    // L2213
    let req: Vec<String> = REQ.with(|r| r.borrow().clone());
    if !req.is_empty() {
        // L2214
        let fontSizeBig = if cx.main.show_flavour_text {
            18.0
        } else {
            16.0
        };
        // L2215
        tooltip.add_line_ex(
            cx,
            fontSizeBig,
            &format!("^x7F7F7FRequires {}", req.join("^x7F7F7F, ")),
            Some(Font::FontinSc),
            None,
            None,
        );
        // L2216
        tooltip.add_separator(cx, Some(10.0));
    }
    // L2218
    REQ.with(|r| r.borrow_mut().clear());
}

// ---- Build:Save's display stats --------------------------------------------------------------

/// `{ elem = elem, attrib = { ... } }` (attributes in source order; PoB's are in `pairs` order,
/// which xml.lua writes as it iterates: T4 normalises attribute order).
fn xml_elem(elem: &str, attrib: &[(&str, String)]) -> XmlChild {
    XmlChild::Node(XmlNode {
        elem: model::LStr::from(elem),
        attrib: attrib
            .iter()
            .map(|(k, v)| (model::LStr::from(*k), model::LStr::from(v.as_str())))
            .collect(),
        children: Vec::new(),
    })
}

/// lua: Build.lua:1033-1080 (inside `buildMode:Save(xml)`, whose `@pob` segment is calc:BM-A's
/// `build_xml`): the `PlayerStat` / `FullDPSSkill` / `MinionStat` elements, written through
/// `build_xml::set_save_stats_hook` (CCR-0080 §8).
///
/// DIVERGENCE(no-env): without a `mainEnv` nothing is written. PoB never saves before Init's
/// first BuildOutput (L695); the LuaCalcBridge serialises the model before it (ui doc §10.4).
/// PoB's Load never reads these elements back.
pub fn save_stats(build: &model::Build, xml: &mut XmlNode) {
    let Some(mainEnv) = build.calcsTab.mainEnv.get() else {
        return;
    };
    let mainOutput = || {
        build
            .calcsTab
            .mainOutput
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainOutput' (a nil value)"))
    };
    // L1033
    let mut addedStatNames: Vec<String> = Vec::new();
    let skillFlags = main_skill(&mainEnv.player).skillFlags.get();
    // L1034
    for statData in display_stats::displayStats().iter() {
        // L1035
        if !stat_match_flags(statData, &skillFlags) {
            continue;
        }
        // L1036
        let Some(stat) = statData.stat else { continue };
        let statName = format!("{stat}{}", statData.childStat.unwrap_or(""));
        // L1037
        if addedStatNames.contains(&statName) {
            continue;
        }
        // L1038
        if stat == "SkillDPS" {
            // L1039
            let statVal = mainOutput().get(Name::from(stat));
            // L1040
            for (_, skillData) in ipairs_val(&statVal) {
                let field = |k: &str| skillData.get(Name::from(k));
                // L1041-1044
                let mut triggerStr = Val::from("");
                let trigger = field("trigger");
                if trigger.truthy() && !trigger.raw_eq(&Val::from("")) {
                    triggerStr = trigger;
                }
                let _ = triggerStr;
                // L1045
                let mut lhsString = concat(&field("name"));
                // L1046
                if display_stats::le(&Val::Num(2.0), &field("count")) {
                    // L1047
                    lhsString = format!("{}x {}", concat(&field("count")), concat(&field("name")));
                }
                // L1049
                let value = tostring_num(mul(&field("dps"), &field("count")));
                let skillPart = field("skillPart");
                let skillPart = if skillPart.truthy() {
                    concat(&skillPart)
                } else {
                    String::new()
                };
                let source = field("source");
                let source = if source.truthy() {
                    concat(&source)
                } else if field("trigger").truthy() {
                    concat(&field("trigger"))
                } else {
                    String::new()
                };
                xml.children.push(xml_elem(
                    "FullDPSSkill",
                    &[
                        ("stat", lhsString),
                        ("value", value),
                        ("skillPart", skillPart),
                        ("source", source),
                    ],
                ));
            }
            // L1051
            addedStatNames.push(statName);
        } else {
            // L1053
            let mut statVal = mainOutput().get(Name::from(stat));
            // L1054
            if statVal.truthy() {
                if let Some(childStat) = statData.childStat {
                    // L1055
                    statVal = statVal.get(Name::from(childStat));
                }
            }
            // L1057 (`condFunc(..) or true` is always true; the call still runs)
            if statVal.truthy() {
                if let Some(f) = statData.condFunc {
                    let _ = f(&statVal, &Val::Table(mainOutput()));
                }
                // L1058
                xml.children.push(xml_elem(
                    "PlayerStat",
                    &[
                        ("stat", statName.clone()),
                        ("value", tostring_val(&statVal)),
                    ],
                ));
                // L1059
                addedStatNames.push(statName);
            }
        }
    }
    // L1064
    for stat in display_stats::extraSaveStats {
        // L1065
        let statVal = mainOutput().get(Name::from(stat as &str));
        // L1066
        if statVal.truthy() {
            // L1067
            xml.children.push(xml_elem(
                "PlayerStat",
                &[
                    ("stat", (*stat).to_owned()),
                    ("value", tostring_val(&statVal)),
                ],
            ));
        }
    }
    // L1070
    if mainEnv.minion.get().is_some() {
        let minion_out = mainOutput().get(Name::from("Minion"));
        // L1071
        for statData in display_stats::minionDisplayStats().iter() {
            // L1072
            if let Some(stat) = statData.stat {
                // L1073
                let statVal = minion_out.get(Name::from(stat));
                // L1074
                if statVal.truthy() {
                    // L1075
                    xml.children.push(xml_elem(
                        "MinionStat",
                        &[("stat", stat.to_owned()), ("value", tostring_val(&statVal))],
                    ));
                }
            }
        }
    }
}

/// Registers [`save_stats`] with calc:BM-A's `Build:Save` (idempotent).
pub fn install_save_stats_hook() {
    model::build_xml::set_save_stats_hook(Some(Rc::new(save_stats)));
}
