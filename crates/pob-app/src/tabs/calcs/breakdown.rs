//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CalcBreakdownControl.lua 1-789.
//! Owner: U40.
//!
//! @pob-file Classes/CalcBreakdownControl.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! The calculation breakdown pane (`newClass("CalcBreakdownControl", "Control", "ControlHost")`):
//! the Calcs tab's `controls.breakdown` (CalcsTab.lua:156, shown beside a hovered/pinned cell and
//! inside a pinned overlay section, CalcSectionControl.lua:429) and the build sidebar's
//! `controls.breakdown` (Build.lua:616-621, shown beside a hovered/pinned stat line). It builds a
//! list of sections from the cell's `displayData` (text lines, the radius visualiser, generic
//! tables, the reservation/damage-type/slot tables and modifier tables from `Tabulate`), sizes
//! them, and draws them with a scroll bar when they do not fit.
//!
//! Also here: Common.lua 836-871 (`formatRound`, `formatPercent`, `formatSec` and their
//! `getFormat*` closures), which the ui doc gives to the first consuming unit (§13 item 15,
//! CCR-0033 §6).
//!
//! Data: the section data, the breakdown tables, the rows and the columns are calc [`Table`]s,
//! annotated in place as the Lua does. `breakdown.colList` is the calc's table, so the
//! `col.width`/`col.x` this control writes land in it (and `width` keeps its maximum across
//! `SetBreakdownData` calls, as in PoB); the slot rows get `sourceLabel`/`sourceLabelTooltip`
//! written into the calc's `breakdown.slots` rows, which are sorted in place. A row's
//! `<key>Tooltip` closure is a [`CellTooltip`] and its `<key>Node` a [`model::Node`], both as
//! `Val::Obj`. `Tabulate` rows (`TabRow`) become `{ value = …, mod = … }` tables.
//!
//! Lua semantics:
//! * `self.shown` is the Control field (`IsShown`), written as `Prop::Val`.
//! * `self.pinned` is nil/false/true; only its truthiness is read (bool).
//! * `FormatModName` returns gsub's two values; every caller uses the first.
//! * Lua comparisons in the sort functions go through [`lua_lt`] (numbers, strings; anything
//!   else raises "attempt to compare").
//! * `{[row.mod.flags] = ModFlag, [row.mod.keywordFlags] = KeywordFlag}` (L460) has one entry
//!   when both flag values are equal (the second store wins): only KeywordFlag's names are added
//!   then, as in PoB.
//!
//! ORDER:
//! * L113-114 `pairs(section.colList)` / `pairs(section.rowList)`, L253 `pairs(breakdown.slots)`,
//!   L281 `pairs(section.rowList)`: (c) sequences built with `t_insert` or constructors (array
//!   part, index order); `Table::pairs` visits the array part first, in index order.
//! * L373 `pairs(sourceTotals)` (string keys): (c) each source type fills its own line list;
//!   the only state left behind is `cfg.source` (the last type), which nothing reads afterwards
//!   (insertion order here).
//! * L460-461 `pairs({...})`, `pairs(ModFlag)` / `pairs(KeywordFlag)`: (c) the names are sorted
//!   (L467); duplicates (a name in both tables) are equal strings.
//! * L499 `pairs(SkillType)`: (c) SkillType's values are distinct, so the first match is the
//!   only one (`SkillType::name`).
//! * L341/L349 `table.sort` of the modifier rows (Lua 5.1 `auxsort`, `pob_lua::sort`, not
//!   stable): rows tied under the comparator (same `mod.name` and value, e.g. two tree nodes
//!   each giving "40% increased Critical Strike Chance" with different flags) keep an order
//!   that depends on `Tabulate`'s, i.e. the order the calc added the mods. For tree mods that is
//!   CalcSetup's `pairs(env.allocNodes)` merge order, per-process random in PoB (CCR-0028 §9,
//!   ui doc §8 rule 6 (b)). The port sorts exactly what the store returns; against the oracle
//!   (UI and bridge are two PoB processes) such tables can differ in the order of tied rows
//!   (`tests/u40/e_radius.pobscript` notes the case it avoids).
//!
//! DIVERGENCE(div-var): Multiplier/PerStat tag text (L481, L485): for a tag with `divVar`, PoB
//! prints the `div` that the last `EvalMod` wrote into the shared tag (ModStore.lua:401, 499);
//! the Rust calc never writes it (calc doc §5.3). The port prints `tag.div`, or for a `divVar`
//! tag `actor.modDB:GetMultiplier(tag.divVar, cfg)` of this section's actor and cfg (ui doc §13
//! item 14, CCR-0015 §1; `known_diffs.toml` U40-1).
//!
//! DIVERGENCE(con-print): L365-367 `ConPrintTable(row.mod)` for a mod without `source` (a
//! console dump right before the Lua error L368 raises) is not ported; the error is.
//!
//! DIVERGENCE(calcs-tab-late): the sidebar control is built by `BuildHooks::breakdown`
//! (Build.lua:616), which runs before `cx.build().tabs.calcs` is assigned; its `calcsTab` is
//! resolved when used (`tabs::calcs::core::tab`). The sidebar control reads only the model
//! through it (envs, `input`, the build), so nothing changes.
//!
//! Seams to units not ported yet ([`BreakdownHooks`], CCR-0116): `ItemsTab:AddItemTooltip`
//! (U38) and the node viewer (`PassiveTreeView`, U23: `viewer:Draw`, `viewer.highlightRing`).
//! The defaults add nothing to the tooltip and draw nothing (the viewer's frame, the highlight
//! ring call and the viewport are this file's and are drawn).
//!
//! Props with side effects (ui doc §7.3): none (`scrollBar.x` only reads).

#![allow(non_snake_case, clippy::nonminimal_bool, clippy::too_many_lines)]

use std::cell::RefCell;
use std::rc::Rc;

use indexmap::IndexMap;
use pob_lua::math::{lpow, m_abs, m_floor, m_max, m_max_n, m_min};
use pob_lua::{n, tonumber, tostring_num, Name};
use pob_sg::{Align, Font, ImageFlags, ImageHandle};
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::ScrollBarControl;
use pob_ui_core::luastr::format_num_sep_main;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::Tooltip;

use super::core as calcs;
use super::CalcsTab;
use crate::app::CxApp;
use crate::model::{self, EnvRef, Key as TKey, Table, Val};
use crate::tree_view::PassiveTreeView;
use pob_calc::actor::ActorRef;
use pob_calc::cfg::{Cfg, ModCfg};
use pob_calc::classes::ModStoreRef;
use pob_gamedata::{KeywordFlag, ModFlag, ModType, SkillType};

// @pob Classes/CalcBreakdownControl.lua:1-15 h=0f8c7ca8
// (`local t_insert, m_max, …, band`: Rust collections, `pob_lua::math`, `&`)

/// A row's `<key>Tooltip` closure (`row.sourceTooltip = function(tooltip) … end`, L417, 284,
/// 430), stored in the row table as `Val::Obj`.
pub struct CellTooltip(pub Rc<dyn Fn(&mut Cx, &mut Tooltip)>);

/// A breakdown section (`{ type = "TEXT" | "RADIUS" | "TABLE", … }`).
pub enum SectionKind {
    /// `{ type = "TEXT", lines = breakdown, textSize = 16 }`
    Text { lines: Val, text_size: f64 },
    /// `{ type = "RADIUS", radius = breakdown.radius }`
    Radius { radius: Val },
    /// `{ type = "TABLE", label, footer, rowList, colList }`
    Table {
        label: Val,
        footer: Val,
        row_list: Table,
        col_list: Table,
    },
}

pub struct Section {
    pub kind: SectionKind,
    /// `section.width` (nil until sized; RADIUS sets it in its constructor)
    pub width: f64,
    /// `section.height`
    pub height: f64,
    /// `section.offset`
    pub offset: f64,
}

/// The Lua class (`newClass("CalcBreakdownControl", "Control", "ControlHost")`).
/// @pob Classes/CalcBreakdownControl.lua:16-17 h=9a855af0
pub struct CalcBreakdownControl {
    /// `self.calcsTab` (`None` for the sidebar control: DIVERGENCE(calcs-tab-late))
    pub calcs_tab: Option<CtrlId>,
    /// `self.tooltip`
    pub tooltip: Tooltip,
    /// `self.nodeViewer` (U23's object; lent to [`BreakdownHooks::node_viewer_draw`])
    pub node_viewer: PassiveTreeView,
    /// `self.rangeGuide`
    pub range_guide: ImageHandle,
    /// `self.uiOverlay`
    pub ui_overlay: ImageHandle,
    /// `self.borderThickness`
    pub border_thickness: f64,
    /// `self.pinnedColour`
    pub pinned_colour: [f64; 3],
    /// `self.borderColour`
    pub border_colour: [f64; 3],
    /// `self.pinned`
    pub pinned: bool,
    /// `self.sourceData` (a cell table, compared by identity)
    pub source_data: Option<Table>,
    /// `self.forceActor` (`"player"`, `"minion"`, …)
    pub force_actor: Option<String>,
    /// `self.envName` (nil: `"calcsEnv"`)
    pub env_name: Option<&'static str>,
    /// `self.clearDisplayFunc`
    pub clear_display_func: Option<Rc<dyn Fn(&mut Cx)>>,
    /// `self.sectionList`
    pub section_list: Vec<Section>,
    /// `self.contentWidth` (nil until the first non-empty `SetBreakdownData`)
    pub content_width: Option<f64>,
    /// `self.contentHeight`
    pub content_height: Option<f64>,
}

impl Widget for CalcBreakdownControl {
    const NAME: &'static str = "CalcBreakdownControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, _no_tooltip: bool) {
        Self::draw(cx, me, vp);
    }
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        Self::is_mouse_over(cx, me)
    }
    fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, double_click: bool) -> Focus {
        Self::on_key_down(cx, me, key, double_click)
    }
    fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        Self::on_key_up(cx, me, key)
    }
}

// ---- seams to units not ported yet (CCR-0116) ------------------------------------------------

/// What this control calls in other units' classes.
#[derive(Clone)]
pub struct BreakdownHooks {
    /// `build.itemsTab:AddItemTooltip(tooltip, item, slot)` (ItemsTab.lua, U38)
    pub add_item_tooltip: Rc<dyn Fn(&mut Cx, &mut Tooltip, &model::ItemRef, Option<&str>)>,
    /// The node viewer (PassiveTreeView, U23), L632-637: `viewer.zoom = zoom`, `viewer.zoomX =
    /// zoomX`, `viewer.zoomY = zoomY`, then `viewer:Draw(build, { x = 0, y = 0, width = 300,
    /// height = 300 }, { })` inside the viewport this control set. Arguments: the control (whose
    /// `node_viewer` field is the viewer), `zoom`, `zoomX`, `zoomY`, the viewer's viewport.
    pub node_viewer_draw: Rc<dyn Fn(&mut Cx, CtrlId, f64, f64, f64, &ViewPort)>,
    /// `viewer.highlightRing` (L640)
    pub node_viewer_highlight_ring: Rc<dyn Fn(&Cx, CtrlId) -> Option<ImageHandle>>,
}

/// The defaults until U38/U23 land: no tooltip lines, a viewer that draws nothing and has no
/// highlight ring image.
pub fn default_breakdown_hooks() -> BreakdownHooks {
    BreakdownHooks {
        add_item_tooltip: Rc::new(|_, _, _, _| {}),
        node_viewer_draw: Rc::new(|_, _, _, _, _, _| {}),
        node_viewer_highlight_ring: Rc::new(|_, _| None),
    }
}

thread_local! {
    static HOOKS: RefCell<Option<BreakdownHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn breakdown_hooks() -> BreakdownHooks {
    HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_breakdown_hooks)
            .clone()
    })
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_breakdown_hooks(f: impl FnOnce(&mut BreakdownHooks)) {
    HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_breakdown_hooks)));
}

// ---- Lua value helpers ------------------------------------------------------------------------

/// A `..` operand: a string, or a number through `tostring`.
#[track_caller]
fn lstr(v: &Val, what: &str) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!(
            "attempt to concatenate {what} (a {} value)",
            other.lua_type()
        ),
    }
}

/// `tostring(v)` of a cell value.
fn ltostring(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

/// A string argument of a `string.*` function (numbers coerce; anything else raises).
#[track_caller]
fn lstr_arg(v: &Val, f: &str) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!(
            "bad argument #1 to '{f}' (string expected, got {})",
            other.lua_type()
        ),
    }
}

/// `select(2, string.gsub(s, p, r))`: the number of matches.
#[track_caller]
fn gsub_count(v: &Val, p: &str, r: &str) -> usize {
    pob_lua::gsub(&lstr_arg(v, "gsub"), p, r, None).1
}

/// A table operand (`#t`, `t.k`, `ipairs(t)`).
#[track_caller]
fn tbl(v: &Val, what: &str) -> Table {
    v.table()
        .unwrap_or_else(|| lua_error!("attempt to index {what} (a {} value)", v.lua_type()))
}

/// `#v` of a table value (a data table counts its sequence).
#[track_caller]
fn len_of(v: &Val, what: &str) -> usize {
    match v {
        Val::Table(t) => t.len(),
        Val::Data(_) | Val::Mod(_) => {
            let mut i = 0;
            while !v.get(TKey::from(i as i64 + 1)).is_nil() {
                i += 1;
            }
            i
        }
        Val::Str(s) => s.len(),
        other => lua_error!(
            "attempt to get length of {what} (a {} value)",
            other.lua_type()
        ),
    }
}

/// Lua `a < b`.
#[track_caller]
pub fn lua_lt(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x < y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() < y.as_bytes(),
        _ if a.lua_type() == b.lua_type() => {
            lua_error!("attempt to compare two {} values", a.lua_type())
        }
        _ => lua_error!(
            "attempt to compare {} with {}",
            a.lua_type(),
            b.lua_type()
        ),
    }
}

/// Lua `a <= b`.
#[track_caller]
fn lua_le(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x <= y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() <= y.as_bytes(),
        _ => {
            lua_lt(a, b);
            false
        }
    }
}

/// `table.sort(t, lt)` on a calc table: `#t` elements, the Lua 5.1 algorithm, written back
/// into `t` (the sort functions here never read `t`).
fn sort_table(t: &Table, mut lt: impl FnMut(&Val, &Val) -> bool) {
    let n = t.len();
    let mut v: Vec<Val> = (1..=n).map(|i| t.rawget(i as i64)).collect();
    pob_lua::sort(&mut v, |a, b| lt(a, b));
    for (i, x) in v.into_iter().enumerate() {
        t.set(i as i64 + 1, x);
    }
}

/// `table.concat(list, sep)` of a list value.
#[track_caller]
fn concat_list(v: &Val, sep: &str) -> String {
    if !matches!(v, Val::Table(_) | Val::Data(_)) {
        lua_error!(
            "bad argument #1 to 'concat' (table expected, got {})",
            v.lua_type()
        );
    }
    let mut out = String::new();
    let mut i = 1i64;
    loop {
        let e = v.get(TKey::from(i));
        if e.is_nil() {
            break;
        }
        if i > 1 {
            out.push_str(sep);
        }
        match &e {
            Val::Str(s) => out.push_str(s),
            Val::Num(x) => out.push_str(&tostring_num(*x)),
            other => lua_error!(
                "invalid value (at index {i}) in table for 'concat' (a {} value)",
                other.lua_type()
            ),
        }
        i += 1;
    }
    out
}

/// A string field read as a `Name` (`cfg.source = sectionData.modSource`): strings and
/// numbers; nil/false → `None`.
fn opt_name(v: &Val) -> Option<Name> {
    match v {
        Val::Str(s) => Some(Name::from(s)),
        Val::Num(x) => Some(Name::from(tostring_num(*x).as_str())),
        _ => None,
    }
}

/// `x:match(p)` of a string value (the first capture).
#[track_caller]
fn smatch(v: &Val, p: &str, what: &str) -> Option<String> {
    let s = match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => tostring_num(*x),
        other => lua_error!("attempt to index {what} (a {} value)", other.lua_type()),
    };
    pob_ui_core::luastr::lmatch(&s, p)
}

/// `actor.mainSkill[name]` for a cfg name (`"skill" .. "Cfg"`, …): the typed cfg fields, then
/// the computed ones (`"dot" .. damageType .. "Cfg"`, CalcOffence.lua:5812, in `extra`).
fn main_skill_cfg(skill: &pob_calc::active_skill::ActiveSkill, name: &str) -> Option<Cfg> {
    match name {
        "skillCfg" => Some(skill.skillCfg.get()),
        "weapon1Cfg" | "weapon2Cfg" | "bleedCfg" | "poisonCfg" | "igniteCfg" | "OHbleedCfg"
        | "OHpoisonCfg" | "OHigniteCfg" | "dotCfg" | "decayCfg" => {
            skill.cfg_field(Name::from(name)).get()
        }
        _ => skill.extra.get(Name::from(name)).obj::<ModCfg>(),
    }
}

// ---- Common.lua 836-871 (the format* family, ui doc §13 item 15) ------------------------------

/// Formats 1234.56 -> "1234.6" [dec=1]
/// @pob Modules/Common.lua:836-840 h=cc6485a5
pub fn formatRound(val: f64, dec: Option<f64>) -> f64 {
    // L838
    let dec = dec.unwrap_or(0.0);
    // L839
    m_floor(val * lpow(10.0, dec) + 0.5) / lpow(10.0, dec)
}

/// @pob Modules/Common.lua:841-845 h=37b2e752
pub fn getFormatRound(dec: Option<f64>) -> impl Fn(f64) -> f64 {
    // L842-844
    move |val| formatRound(val, dec)
}

/// Formats 12.3456 -> "1234.6%" [dec=1]
/// @pob Modules/Common.lua:847-851 h=91b63023
pub fn formatPercent(val: Option<f64>, dec: Option<f64>) -> String {
    // L849
    let dec = dec.unwrap_or(0.0);
    // L850
    let v = m_floor(val.unwrap_or(0.0) * 100.0 * lpow(10.0, dec) + 0.5) / lpow(10.0, dec);
    format!("{}%", tostring_num(v))
}

/// @pob Modules/Common.lua:852-856 h=8fee56fd
pub fn getFormatPercent(dec: Option<f64>) -> impl Fn(Option<f64>) -> String {
    // L853-855
    move |val| formatPercent(val, dec)
}

/// Formats 1234.56 -> "1234.5s" [dec=1]
/// @pob Modules/Common.lua:858-866 h=1c58c0a5
pub fn formatSec(val: f64, dec: Option<f64>) -> String {
    // L860
    let dec = dec.unwrap_or(0.0);
    // L861
    if val == 0.0 {
        // L862
        "0s".to_owned()
    } else {
        // L864
        pob_lua::lua_format(
            &format!("%.{}fs", tostring_num(dec)),
            &[pob_lua::FmtArg::from(val)],
        )
    }
}

/// @pob Modules/Common.lua:867-871 h=853ffbfe
pub fn getFormatSec(dec: Option<f64>) -> impl Fn(f64) -> String {
    // L868-870
    move |val| formatSec(val, dec)
}

// ---- the class ---------------------------------------------------------------------------------

fn st(cx: &Cx, me: CtrlId) -> &CalcBreakdownControl {
    cx.w::<CalcBreakdownControl>(me)
}

fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut CalcBreakdownControl {
    cx.w_mut::<CalcBreakdownControl>(me)
}

impl CalcBreakdownControl {
    /// lua: CalcBreakdownControl.lua:20-40 `CalcBreakdownControl(calcsTab)`
    /// @pob Classes/CalcBreakdownControl.lua:19-40 h=e4f69df5
    pub fn new(calcsTab: Option<CtrlId>) -> New<CalcBreakdownControl> {
        // L21 self:Control(); L22 self:ControlHost()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            CalcBreakdownControl {
                // L23
                calcs_tab: calcsTab,
                tooltip: Tooltip::default(),
                node_viewer: PassiveTreeView::default(),
                range_guide: ImageHandle::default(),
                ui_overlay: ImageHandle::default(),
                border_thickness: 0.0,
                pinned_colour: [0.0; 3],
                border_colour: [0.0; 3],
                pinned: false,
                source_data: None,
                force_actor: None,
                env_name: None,
                clear_display_func: None,
                section_list: Vec::new(),
                content_width: None,
                content_height: None,
            },
        )
        .host()
        .init(|cx, me| {
            let me = me.id();
            // L24
            cx.base_mut(me).shown = Prop::Val(false);
            // L25 self.tooltip = new("Tooltip"):Tooltip() (the field above)
            // L26 self.nodeViewer = new("PassiveTreeView"):PassiveTreeView() (U23's object)
            // L27
            let rangeGuide = cx.sg.new_image_handle();
            // L28
            rangeGuide.load(&mut cx.sg, "Assets/range_guide.png", ImageFlags::empty());
            // L29
            let uiOverlay = cx.sg.new_image_handle();
            // L30
            uiOverlay.load(&mut cx.sg, "Assets/game_ui_small.png", ImageFlags::empty());
            {
                let w = st_mut(cx, me);
                w.range_guide = rangeGuide;
                w.ui_overlay = uiOverlay;
                // L31
                w.border_thickness = 2.0;
            }
            // L32-33
            let sb = cx
                .add(
                    me,
                    "scrollBar",
                    ScrollBarControl::new(
                        Anchor::new(Right, me, Right),
                        rect(-2, 0, 18, 0),
                        Some(80.0),
                        Some(ScrollDir::Vertical),
                        true,
                    ),
                )
                .id();
            // L34-36
            cx.base_mut(sb).x = Prop::f(move |cx, _| -st(cx, me).border_thickness);
            let w = st_mut(cx, me);
            // L37
            w.pinned_colour = [0.25, 1.0, 0.25];
            // L38
            w.border_colour = [0.33, 0.66, 0.33];
        })
    }

    /// `self.controls.scrollBar`
    fn scroll_bar(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child(me, "scrollBar")
            .unwrap_or_else(|| lua_error!("attempt to index field 'scrollBar' (a nil value)"))
    }

    /// `self.calcsTab` (DIVERGENCE(calcs-tab-late))
    fn calcs_tab(cx: &Cx, me: CtrlId) -> CtrlId {
        st(cx, me)
            .calcs_tab
            .or_else(|| calcs::tab(cx))
            .unwrap_or_else(|| lua_error!("attempt to index field 'calcsTab' (a nil value)"))
    }

    /// lua: CalcBreakdownControl.lua:42-47 `IsMouseOver()`
    /// @pob Classes/CalcBreakdownControl.lua:42-47 h=f1e8064e
    pub fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        // L43
        if !cx.is_shown(me) {
            // L44
            return false;
        }
        // L46
        cx.is_mouse_in_bounds(me) || cx.get_mouse_over_control(me).is_some()
    }

    /// lua: CalcBreakdownControl.lua:49-56 `GetActor()`: `(actor, env)`
    /// @pob Classes/CalcBreakdownControl.lua:49-56 h=ac4e21d6
    pub fn get_actor(cx: &Cx, me: CtrlId) -> (Option<ActorRef>, EnvRef) {
        let w = st(cx, me);
        let model = &cx.build().model;
        // L50
        let env = match w.env_name.unwrap_or("calcsEnv") {
            "mainEnv" => model.calcsTab.mainEnv.get(),
            _ => model.calcsTab.calcsEnv.get(),
        }
        .unwrap_or_else(|| lua_error!("attempt to index local 'env' (a nil value)"));
        // L51
        let mut actor = Some(calcs::input_actor(cx, &env));
        // L52
        if let Some(fa) = &w.force_actor {
            // L53
            actor = match fa.as_str() {
                "player" => Some(env.player.clone()),
                "enemy" => Some(env.enemy.clone()),
                "minion" => env.minion.get(),
                _ => None,
            };
        }
        // L55
        (actor, env)
    }

    /// lua: CalcBreakdownControl.lua:60-144 `SetBreakdownData(displayData, pinned, forceActor)`
    /// @pob Classes/CalcBreakdownControl.lua:57-144 h=4480d014
    pub fn set_breakdown_data(
        cx: &mut Cx,
        me: CtrlId,
        displayData: Option<&Table>,
        pinned: bool,
        forceActor: Option<&str>,
    ) {
        // L61
        st_mut(cx, me).pinned = pinned;
        // L62
        let same = match (&st(cx, me).source_data, displayData) {
            (Some(a), Some(b)) => a.ptr_eq(b),
            (None, None) => true,
            _ => false,
        };
        if same {
            // L63
            return;
        }
        {
            let w = st_mut(cx, me);
            // L65
            w.source_data = displayData.cloned();
            // L66
            w.force_actor = forceActor.map(str::to_owned);
        }
        // L67
        cx.base_mut(me).shown = Prop::Val(false);
        // L68
        let Some(displayData) = displayData else {
            // L69
            return;
        };

        // Build list of sections
        // L73
        let (actor, env) = Self::get_actor(cx, me);
        // L74
        st_mut(cx, me).section_list.clear();
        // L75
        for (_, sectionData) in displayData.ipairs() {
            let sectionData = tbl(&sectionData, "local 'sectionData'");
            // L76
            if calcs::check_flag(cx, &sectionData, actor.as_ref(), Some(&env.player)) {
                // L77
                if sectionData.truthy(n!("breakdown")) {
                    // L78
                    Self::add_breakdown_section(cx, me, &sectionData);
                } else if sectionData.truthy(n!("modName")) {
                    // L79-80
                    Self::add_mod_section(cx, me, &sectionData, None);
                }
            }
        }
        // L84
        if st(cx, me).section_list.is_empty() {
            // L85
            if let Some(f) = st(cx, me).clear_display_func.clone() {
                // L86
                f(cx);
            } else {
                // L88
                let tab = Self::calcs_tab(cx, me);
                CalcsTab::clear_display_stat(cx, tab);
            }
            // L90
            return;
        }

        // L93
        cx.base_mut(me).shown = Prop::Val(true);

        // Determine the size of each section, and the combined content size of the breakdown
        // L96
        let mut contentWidth = 0.0;
        // L97
        let mut offset = 2.0;
        let showSep = cx.main.show_thousands_separators;
        let mut sections = std::mem::take(&mut st_mut(cx, me).section_list);
        // L98
        for section in &mut sections {
            match &section.kind {
                // L99
                SectionKind::Text { lines, text_size } => {
                    // L100
                    section.width = 0.0;
                    // L101
                    let mut i = 1i64;
                    loop {
                        let line = lines.get(TKey::from(i));
                        if line.is_nil() {
                            break;
                        }
                        // count how many commas will be added
                        // L102
                        let num = gsub_count(&line, "%d%d%d%d", "");
                        let w = cx.sg.draw_string_width(*text_size, Font::Var, &lstr_arg(&line, "DrawStringWidth"));
                        // L103
                        if showSep && num > 0 {
                            // L104
                            section.width = m_max(section.width, w + 8.0 + (4.0 * num as f64));
                        } else {
                            // L106
                            section.width = m_max(section.width, w + 8.0);
                        }
                        i += 1;
                    }
                    // L109
                    section.height = len_of(lines, "field 'lines'") as f64 * text_size + 4.0;
                }
                // L110
                SectionKind::Table {
                    label,
                    footer,
                    row_list,
                    col_list,
                } => {
                    // This also calculates the width of each column in the table
                    // L112
                    section.width = 4.0;
                    // L113
                    for (_, col) in col_list.pairs() {
                        let col = tbl(&col, "local 'col'");
                        let key = col.get(n!("key"));
                        let colLabel = lstr_arg(&col.get(n!("label")), "DrawStringWidth");
                        // L114
                        for (_, row) in row_list.pairs() {
                            let row = tbl(&row, "local 'row'");
                            let v = match opt_name(&key) {
                                Some(k) => row.get(k),
                                None => row.get(key_of(&key)),
                            };
                            // L115
                            if v.truthy() {
                                // count how many commas will be added
                                // L116
                                let num = gsub_count(&v, "%d%d%d%d", "");
                                let cw = col.get(n!("width"));
                                let cw = if cw.truthy() { cw.num() } else { 0.0 };
                                let lw = cx.sg.draw_string_width(16.0, Font::Var, &colLabel) + 6.0;
                                let vw = cx.sg.draw_string_width(12.0, Font::Var, &lstr_arg(&v, "DrawStringWidth"));
                                // L117
                                if showSep && num > 0 {
                                    // L118
                                    col.set(n!("width"), m_max_n(&[cw, lw, vw + 6.0 + (4.0 * num as f64)]));
                                } else {
                                    // L120
                                    col.set(n!("width"), m_max_n(&[cw, lw, vw + 6.0]));
                                }
                            }
                        }
                        // L124
                        let cw = col.get(n!("width"));
                        if cw.truthy() {
                            // L125
                            section.width += cw.num();
                        }
                    }
                    // L128
                    section.height = row_list.len() as f64 * 14.0 + 20.0;
                    // L129
                    if label.truthy() {
                        // L130
                        let s = format!("{}:", lstr(label, "field 'label'"));
                        contentWidth = m_max(contentWidth, 6.0 + cx.sg.draw_string_width(16.0, Font::Var, &s));
                        // L131
                        section.height += 16.0;
                    }
                    // L133
                    if footer.truthy() {
                        // L134
                        let f = lstr_arg(footer, "DrawStringWidth");
                        contentWidth = m_max(contentWidth, 6.0 + cx.sg.draw_string_width(12.0, Font::Var, &f));
                        // counts newlines in the string
                        // L135
                        let lines = gsub_count(footer, "\n", "\n");
                        // L136
                        section.height += 12.0 * (lines as f64 + 1.0);
                    }
                }
                SectionKind::Radius { .. } => {}
            }
            // L139
            contentWidth = m_max(contentWidth, section.width);
            // L140
            section.offset = offset;
            // L141
            offset = offset + section.height + 8.0;
        }
        let w = st_mut(cx, me);
        w.section_list = sections;
        w.content_width = Some(contentWidth);
        // L143
        w.content_height = Some(offset - 6.0);
    }

    // Add sections based on the breakdown data generated by the Calcs module
    /// lua: CalcBreakdownControl.lua:147-297 `AddBreakdownSection(sectionData)`
    /// @pob Classes/CalcBreakdownControl.lua:146-297 h=9d3972f1
    pub fn add_breakdown_section(cx: &mut Cx, me: CtrlId, sectionData: &Table) {
        // L148
        let (actor, _) = Self::get_actor(cx, me);
        let actor =
            actor.unwrap_or_else(|| lua_error!("attempt to index local 'actor' (a nil value)"));
        let actorBreakdown = actor
            .breakdown
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'breakdown' (a nil value)"));
        // L149
        let breakdown: Val;
        // L150
        let key = lstr(&sectionData.get(n!("breakdown")), "field 'breakdown'");
        let m = pob_lua::lmatch(&key, "^(%a+)%.(%a+)$", None);
        // L151
        if let Some(m) = m {
            let (ns, name) = (m[0].to_str(), m[1].to_str());
            // L152
            let t = actorBreakdown.get(Name::from(&ns));
            breakdown = if t.truthy() {
                t.get(Name::from(&name))
            } else {
                t
            };
        } else {
            // L154
            breakdown = actorBreakdown.get(Name::from(key.as_str()));
        }
        // L156
        if !breakdown.truthy() {
            // L157
            return;
        }

        // L160
        if len_of(&breakdown, "local 'breakdown'") > 0 {
            // Text lines
            // L162-166
            st_mut(cx, me).section_list.push(Section {
                kind: SectionKind::Text {
                    lines: breakdown.clone(),
                    text_size: 16.0,
                },
                width: 0.0,
                height: 0.0,
                offset: 0.0,
            });
        }

        // L169
        let radius = breakdown.get(n!("radius"));
        if radius.truthy() {
            // Radius visualiser
            // L171-176
            st_mut(cx, me).section_list.push(Section {
                kind: SectionKind::Radius { radius },
                width: 8.0 + 1920.0 / 4.0,
                height: 4.0 + 1080.0 / 4.0,
                offset: 0.0,
            });
        }

        // L179
        let bRowList = breakdown.get(n!("rowList"));
        if bRowList.truthy() && len_of(&bRowList, "field 'rowList'") > 0 {
            // sort by the first column (the value)
            // L181
            let rowList = tbl(&bRowList, "field 'rowList'").copyTable(true);
            // L182
            let colList = breakdown.get(n!("colList"));
            let colKey = colList.get(TKey::from(1i64)).get(n!("key"));
            // avoid sorting ailment breakdowns
            // L184
            if breakdown.get(n!("source")).truthy() {
                // L185
                sort_table(&rowList, |a, b| {
                    // L186
                    if a.get(n!("reqNum")).truthy() {
                        // L187
                        return lua_lt(&b.get(n!("reqNum")), &a.get(n!("reqNum")));
                    }
                    // L189
                    lua_lt(&b.get(key_of(&colKey)), &a.get(key_of(&colKey)))
                });
            }
            // Generic table
            // L193-199
            let section = Section {
                kind: SectionKind::Table {
                    label: breakdown.get(n!("label")),
                    footer: breakdown.get(n!("footer")),
                    row_list: rowList,
                    col_list: tbl(&colList, "field 'colList'"),
                },
                width: 0.0,
                height: 0.0,
                offset: 0.0,
            };
            // L200
            st_mut(cx, me).section_list.push(section);
        }

        // L203
        let reservations = breakdown.get(n!("reservations"));
        if reservations.truthy() && len_of(&reservations, "field 'reservations'") > 0 {
            // Reservations table, used for life/mana reservation breakdowns
            // L205-218
            let section = Section {
                kind: SectionKind::Table {
                    label: Val::Nil,
                    footer: Val::Nil,
                    row_list: tbl(&reservations, "field 'reservations'"),
                    col_list: col_list(&[
                        ("Skill", "skillName", false),
                        ("Base", "base", false),
                        ("MCM", "mult", false),
                        ("More/less", "more", false),
                        ("Inc/red", "inc", false),
                        ("Efficiency", "efficiency", false),
                        ("Efficiency More/less", "efficiencyMore", false),
                        ("Reservation", "total", false),
                    ]),
                },
                width: 0.0,
                height: 0.0,
                offset: 0.0,
            };
            // L219
            st_mut(cx, me).section_list.push(section);
        }

        // L222
        let damageTypes = breakdown.get(n!("damageTypes"));
        if damageTypes.truthy() && len_of(&damageTypes, "field 'damageTypes'") > 0 {
            // L223-236
            let section = Section {
                kind: SectionKind::Table {
                    label: Val::Nil,
                    footer: Val::Nil,
                    row_list: tbl(&damageTypes, "field 'damageTypes'"),
                    col_list: col_list(&[
                        ("From", "source", true),
                        ("Base", "base", false),
                        ("Inc/red", "inc", false),
                        ("More/less", "more", false),
                        ("Converted Damage", "convSrc", false),
                        ("Total", "total", false),
                        ("Conversion", "convDst", false),
                        ("Gain", "gainDst", false),
                    ]),
                },
                width: 0.0,
                height: 0.0,
                offset: 0.0,
            };
            // L237
            st_mut(cx, me).section_list.push(section);
        }

        // L240
        let slots = breakdown.get(n!("slots"));
        if slots.truthy() && len_of(&slots, "field 'slots'") > 0 {
            let slots = tbl(&slots, "field 'slots'");
            // Slots table, used for armour/evasion/ES total breakdowns
            // L242-243
            let colList: Table;
            let rowList: Table;
            // L244
            if sectionData.truthy(n!("gearOnly")) {
                // Only show basic table for gear and base ES/Armour/Evasion value
                // L246-250
                colList = col_list(&[
                    ("Value", "base", true),
                    ("Source", "source", false),
                    ("Name", "sourceLabel", false),
                ]);

                // L252
                rowList = Table::new();
                // L253
                for (_, row) in slots.pairs() {
                    // L254
                    let item = row.get(n!("item"));
                    if item.truthy()
                        && item
                            .obj::<model::Item>()
                            .is_some_and(|i| i.armourData.get().is_some())
                    {
                        // L255
                        rowList.push(row);
                    }
                }
            } else {
                // L259-266
                colList = col_list(&[
                    ("Base", "base", true),
                    ("Inc/red", "inc", false),
                    ("More/less", "more", false),
                    ("Total", "total", true),
                    ("Source", "source", false),
                    ("Name", "sourceLabel", false),
                ]);

                // L268
                rowList = slots;
            }

            // L271-273
            sort_table(&rowList, |a, b| lua_lt(&b.get(n!("base")), &a.get(n!("base"))));

            // L275-279
            let section_rows = rowList.clone();
            // L280
            st_mut(cx, me).section_list.push(Section {
                kind: SectionKind::Table {
                    label: Val::Nil,
                    footer: Val::Nil,
                    row_list: rowList,
                    col_list: colList,
                },
                width: 0.0,
                height: 0.0,
                offset: 0.0,
            });
            // L281
            for (_, row) in section_rows.pairs() {
                let row = tbl(&row, "local 'row'");
                // L282
                let item = row.get(n!("item"));
                if item.truthy() {
                    let it = item.obj::<model::Item>().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'item' (a {} value)", item.lua_type())
                    });
                    // L283
                    let rarity = it.rarity.get();
                    let cc = cx
                        .main
                        .cc
                        .get(rarity.as_str())
                        .cloned()
                        .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                    row.set(n!("sourceLabel"), format!("{cc}{}", it.name.get()));
                    // L284-286
                    let r = row.clone();
                    let f: Rc<dyn Fn(&mut Cx, &mut Tooltip)> = Rc::new(move |cx, tooltip| {
                        let item = r.get(n!("item")).obj::<model::Item>().unwrap_or_else(|| {
                            lua_error!("bad argument #2 to 'AddItemTooltip'")
                        });
                        let slot = opt_name(&r.get(n!("source")));
                        let h = breakdown_hooks().add_item_tooltip;
                        h(cx, tooltip, &item, slot.as_ref().map(Name::as_str));
                    });
                    row.set(n!("sourceLabelTooltip"), Val::Obj(Rc::new(CellTooltip(f))));
                } else {
                    // L288
                    row.set(n!("sourceLabel"), row.get(n!("sourceName")));
                }
            }
        }

        // L293
        let modList = breakdown.get(n!("modList"));
        if modList.truthy() && len_of(&modList, "field 'modList'") > 0 {
            // Provided mod list
            // L295
            Self::add_mod_section(cx, me, sectionData, Some(&tbl(&modList, "field 'modList'")));
        }
    }

    // Add a table section showing a list of modifiers
    /// lua: CalcBreakdownControl.lua:300-527 `AddModSection(sectionData, modList)`
    /// @pob Classes/CalcBreakdownControl.lua:299-527 h=dfbdc05b
    pub fn add_mod_section(cx: &mut Cx, me: CtrlId, sectionData: &Table, modList: Option<&Table>) {
        // L301
        let (actor, _) = Self::get_actor(cx, me);
        let actor =
            actor.unwrap_or_else(|| lua_error!("attempt to index local 'actor' (a nil value)"));
        // L302
        let build = cx.build().model.clone();

        // Build list of modifiers to display
        // L305
        let sCfg = sectionData.get(n!("cfg"));
        let cfg: Cfg = if sCfg.truthy() {
            let name = format!("{}Cfg", lstr(&sCfg, "field 'cfg'"));
            let skill = actor.mainSkill.get().unwrap_or_else(|| {
                lua_error!("attempt to index field 'mainSkill' (a nil value)")
            });
            match main_skill_cfg(&skill, &name) {
                Some(c) => c.copyTable(true),
                None => ModCfg::new(),
            }
        } else {
            ModCfg::new()
        };
        // L306
        cfg.source.set(opt_name(&sectionData.get(n!("modSource"))));
        // L307
        cfg.actor.set(opt_name(&sectionData.get(n!("actor"))));
        // L308
        let rowList: Table;
        // L309
        let modStore: ModStoreRef = if sectionData.truthy(n!("enemy")) {
            actor
                .enemy()
                .unwrap_or_else(|| lua_error!("attempt to index field 'enemy' (a nil value)"))
                .modDB
                .get()
        } else if sCfg.truthy() {
            actor
                .mainSkill
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
                .skillModList
                .get()
        } else {
            actor.modDB.get()
        };
        let modName = sectionData.get(n!("modName"));
        let sModType = sectionData.get(n!("modType"));
        // L310
        if let Some(modList) = modList {
            // L311
            rowList = modList.copyTable(false);
        } else {
            let modType = match &sModType {
                Val::Nil | Val::Bool(false) => None,
                v => Some(ModType::parse(&lstr(v, "field 'modType'")).unwrap_or(ModType::Nil)),
            };
            // L313
            let names: Vec<Name> = if let Val::Table(t) = &modName {
                // L314 (`unpack(sectionData.modName)`)
                t.ipairs()
                    .iter()
                    .map(|(_, v)| Name::from(lstr(v, "a name").as_str()))
                    .collect()
            } else {
                // L316
                vec![Name::from(lstr(&modName, "field 'modName'").as_str())]
            };
            let rows = modStore.Tabulate(modType, Some(&*cfg), &names);
            rowList = Table::new();
            for r in rows {
                let t = Table::new();
                t.set(n!("value"), Val::from_value(&r.value));
                t.set(n!("mod"), Val::Mod(r.r#mod));
                rowList.push(t);
            }
        }
        // L319
        if rowList.is_empty() {
            // L320
            return;
        }

        // Create section data
        // L324-336
        let colList = col_list(&[
            ("Value", "displayValue", false),
            ("Stat", "name", false),
            ("Skill types", "flags", false),
            ("Notes", "tags", false),
            ("Source", "source", false),
            ("Source Name", "sourceName", false),
        ]);
        // L337
        st_mut(cx, me).section_list.push(Section {
            kind: SectionKind::Table {
                label: sectionData.get(n!("label")),
                footer: Val::Nil,
                row_list: rowList.clone(),
                col_list: colList.clone(),
            },
            width: 0.0,
            height: 0.0,
            offset: 0.0,
        });

        let by_name_value = |a: &Val, b: &Val| -> bool {
            let (am, bm) = (a.get(n!("mod")), b.get(n!("mod")));
            let (an, bn) = (am.get(n!("name")), bm.get(n!("name")));
            let (av, bv) = (a.get(n!("value")), b.get(n!("value")));
            lua_lt(&bn, &an)
                || (an.raw_eq(&bn)
                    && matches!(av, Val::Num(_))
                    && matches!(bv, Val::Num(_))
                    && lua_lt(&bv, &av))
        };
        // L339
        if modList.is_none() && !sModType.truthy() {
            // Sort modifiers by type
            // L341
            sort_table(&rowList, |a, b| {
                let (at, bt) = (a.get(n!("mod")).get(n!("type")), b.get(n!("mod")).get(n!("type")));
                // L342
                if at.raw_eq(&bt) {
                    // L343
                    by_name_value(a, b)
                } else {
                    // L345
                    lua_lt(&at, &bt)
                }
            });
        } else {
            // Sort modifiers by value
            // L349-351
            sort_table(&rowList, by_name_value);
        }

        // L354
        let mut sourceTotals: IndexMap<String, Rc<RefCell<Vec<String>>>> = IndexMap::new();
        // L355
        if modList.is_none() && !sectionData.truthy(n!("modSource")) {
            // Build list of totals from each modifier source
            // L357
            let mut types: Vec<Val> = Vec::new();
            // L358
            let mut typeList: Vec<Val> = Vec::new();
            // L359
            for (_, row) in rowList.ipairs() {
                let rmod = row.get(n!("mod"));
                let rtype = rmod.get(n!("type"));
                // Find all the modifier types and source types that are present in the modifier list
                // L361
                if !types.iter().any(|t| t.raw_eq(&rtype)) {
                    if rtype.is_nil() {
                        lua_error!("table index is nil");
                    }
                    // L362
                    types.push(rtype.clone());
                    // L363
                    typeList.push(rtype);
                }
                // L365-367 DIVERGENCE(con-print)
                // L368
                let Some(sourceType) = smatch(&rmod.get(n!("source")), "[^:]+", "field 'source'")
                else {
                    // L369-370 `sourceTotals[nil] = { }`
                    lua_error!("table index is nil");
                };
                // L369
                if !sourceTotals.contains_key(&sourceType) {
                    // L370
                    sourceTotals.insert(sourceType, Rc::new(RefCell::new(Vec::new())));
                }
            }
            // L373
            for (sourceType, lines) in &sourceTotals {
                // L374
                cfg.source.set(Some(Name::from(sourceType.as_str())));
                // L375
                for modType in &typeList {
                    let mt_str = lstr(modType, "local 'modType'");
                    let mt = ModType::parse(&mt_str).unwrap_or(ModType::Nil);
                    // L376
                    if let Val::Table(names) = &modName {
                        // Multiple stats, show each separately
                        // L378
                        for (_, mn) in names.ipairs() {
                            let mn = lstr(&mn, "local 'modName'");
                            // L379
                            let mut total = modStore.Combine(mt, Some(&*cfg), &[Name::from(mn.as_str())]);
                            // L380
                            if mt_str == "MORE" {
                                // L381
                                total = Val::Num(pob_lua::round((total.num() - 1.0) * 100.0, None));
                            }
                            // L383
                            if total.truthy() && !total.raw_eq(&Val::Num(0.0)) {
                                // L384
                                let v = lstr(&Self::format_mod_value(&total, modType), "a value");
                                let pretty = pob_ui_core::luastr::gsub(&mn, "(%l)(%u)", "%1 %2");
                                lines.borrow_mut().push(format!("{v} {pretty}"));
                            }
                        }
                    } else {
                        // L388
                        let mn = lstr(&modName, "field 'modName'");
                        let mut total = modStore.Combine(mt, Some(&*cfg), &[Name::from(mn.as_str())]);
                        // L389
                        if mt_str == "MORE" {
                            // L390
                            total = Val::Num(pob_lua::round((total.num() - 1.0) * 100.0, None));
                        }
                        // L392
                        if total.truthy() && !total.raw_eq(&Val::Num(0.0)) {
                            // L393
                            let v = lstr(&Self::format_mod_value(&total, modType), "a value");
                            lines.borrow_mut().push(v);
                        }
                    }
                }
            }
        }

        // Process modifier data
        // L401
        for (_, row) in rowList.ipairs() {
            let row = tbl(&row, "local 'row'");
            let rmod = row.get(n!("mod"));
            // L402
            if !sModType.truthy() {
                // No modifier type specified, so format the value to convey type
                // L404
                let v = Self::format_mod_value(&row.get(n!("value")), &rmod.get(n!("type")));
                row.set(n!("displayValue"), v);
            } else {
                // L406
                colList.get(1i64).table().expect("colList[1]").set(n!("right"), true);
                // L407
                row.set(n!("displayValue"), formatRound(row.get(n!("value")).num(), Some(2.0)));
            }
            // L409
            if modList.is_some() || matches!(modName, Val::Table(_)) {
                // Multiple stat names specified, add this modifier's stat to the table
                // L411
                row.set(n!("name"), Self::format_mod_name(&lstr(&rmod.get(n!("name")), "field 'name'")));
            }
            // L413
            let rsource = rmod.get(n!("source"));
            let sourceType = smatch(&rsource, "[^:]+", "field 'source'").unwrap_or_default();
            // L414
            row.set(n!("source"), sourceType.as_str());
            // L415
            if modList.is_none() && !sectionData.truthy(n!("modSource")) {
                // No modifier source specified, add the source type to the table
                // L417-422
                let st_ = sourceType.clone();
                let lines = sourceTotals.get(&sourceType).cloned();
                let f: Rc<dyn Fn(&mut Cx, &mut Tooltip)> = Rc::new(move |cx, tooltip| {
                    // L418
                    tooltip.add_line(cx, 16.0, &format!("Total from {st_}:"));
                    // L419
                    let lines = lines.clone().unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
                    });
                    let lines = lines.borrow().clone();
                    for line in &lines {
                        // L420
                        tooltip.add_line(cx, 14.0, line);
                    }
                });
                row.set(n!("sourceTooltip"), Val::Obj(Rc::new(CellTooltip(f))));
            }
            // L424
            if sourceType == "Item" {
                // Modifier is from an item, add item name and tooltip
                // L426
                let itemId = smatch(&rsource, "Item:(%d+):.+", "field 'source'");
                // L427
                let item = itemId
                    .and_then(|i| tonumber(&i))
                    .and_then(|id| build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned()));
                // L428
                if let Some(item) = item {
                    // L429
                    let rarity = item.rarity.get();
                    let cc = cx
                        .main
                        .cc
                        .get(rarity.as_str())
                        .cloned()
                        .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                    row.set(n!("sourceName"), format!("{cc}{}", item.name.get()));
                    // L430-432
                    let r = row.clone();
                    let f: Rc<dyn Fn(&mut Cx, &mut Tooltip)> = Rc::new(move |cx, tooltip| {
                        let slot = opt_name(&r.get(n!("mod")).get(n!("sourceSlot")));
                        let h = breakdown_hooks().add_item_tooltip;
                        h(cx, tooltip, &item, slot.as_ref().map(Name::as_str));
                    });
                    row.set(n!("sourceNameTooltip"), Val::Obj(Rc::new(CellTooltip(f))));
                }
            } else if sourceType == "Tree" {
                // Modifier is from a passive node, add node name, and add node ID (used to show node location)
                // L436
                let nodeId = smatch(&rsource, "Tree:(%d+)", "field 'source'");
                // L437
                let tattooNodeId = smatch(&rsource, "Tree:(%w+)", "field 'source'");
                // L438
                if let Some(nodeId) = nodeId {
                    // L439
                    let nodeIdNumber = tonumber(&nodeId).unwrap_or(f64::NAN) as i64;
                    // L440
                    let spec = build.spec.get();
                    let node = spec
                        .nodes
                        .with(|m| m.get(nodeIdNumber))
                        .or_else(|| spec.tree.get().nodes.get(nodeIdNumber))
                        .or_else(|| {
                            build
                                .latestTree
                                .get()
                                .unwrap_or_else(|| {
                                    lua_error!("attempt to index field 'latestTree' (a nil value)")
                                })
                                .nodes
                                .get(nodeIdNumber)
                        })
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index local 'node' (a nil value)")
                        });
                    // L441
                    row.set(n!("sourceName"), Val::Str(node.dn.get()));
                    // L442
                    row.set(n!("sourceNameNode"), Val::Obj(node as Rc<dyn std::any::Any>));
                } else if let Some(tattooNodeId) = tattooNodeId {
                    // L444
                    let tattoo = build.spec.get().tree.get().tattoo.get().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'tattoo' (a nil value)")
                    });
                    let idMap = tattoo.get(n!("idMap"));
                    row.set(n!("sourceName"), tbl(&idMap, "field 'idMap'").get(Name::from(tattooNodeId.as_str())));
                }
            } else if sourceType == "Skill" {
                // Extract skill name
                // L448
                let id = smatch(&rsource, "Skill:(.+)", "field 'source'");
                let ge = id
                    .and_then(|id| build.data.skills.get(&Name::from(id.as_str())).cloned())
                    .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
                row.set(n!("sourceName"), ge.name);
            } else if sourceType == "Pantheon" {
                // L450
                row.set(n!("sourceName"), match_val(&rsource, "Pantheon:(.+)"));
            } else if sourceType == "Spectre" {
                // L452
                row.set(n!("sourceName"), match_val(&rsource, "Spectre:(.+)"));
            } else if sourceType == "Custom" {
                // L454
                row.set(n!("sourceName"), match_val(&rsource, "Custom:(.+)"));
            }

            // L457
            let flags = rmod.get(n!("flags"));
            let keywordFlags = rmod.get(n!("keywordFlags"));
            if !flags.raw_eq(&Val::Num(0.0)) || !keywordFlags.raw_eq(&Val::Num(0.0)) {
                // Combine, sort and format modifier flags
                // L459
                let mut flagNames: Vec<String> = Vec::new();
                let fl = flags.num() as i64 as u32;
                let kw = keywordFlags.num() as i64 as u32;
                // L460 (one entry when the keys are equal: the second store wins)
                let srcs: Vec<(u32, &[(&str, u32)])> = if flags.raw_eq(&keywordFlags) {
                    vec![(kw, KeywordFlag::ENTRIES)]
                } else {
                    vec![(fl, ModFlag::ENTRIES), (kw, KeywordFlag::ENTRIES)]
                };
                for (flags, src) in srcs {
                    // L461
                    for (name, val) in src {
                        // L462
                        if flags & val == *val {
                            // L463
                            flagNames.push((*name).to_owned());
                        }
                    }
                }
                // L467
                pob_lua::sort(&mut flagNames, |a, b| a.as_bytes() < b.as_bytes());
                // L468
                row.set(n!("flags"), flagNames.join(", "));
            }
            // L470
            row.set(n!("tags"), Val::Nil);
            // L471
            if rmod.get(1i64).truthy() {
                // Format modifier tags
                // L473
                let mval = rmod.get(n!("value"));
                let mut baseVal: Val = if matches!(mval, Val::Num(_)) {
                    Val::from(format!("{} ", Self::format_mod_base(&rmod, mval.num())))
                } else {
                    Val::Bool(false)
                };
                // L474
                let mut ti = 1i64;
                loop {
                    let tag = rmod.get(ti);
                    if tag.is_nil() {
                        break;
                    }
                    ti += 1;
                    let g = |k: &'static str| tag.get(Name::from(k));
                    // L475
                    let mut desc: Option<String> = None;
                    let ttype = g("type");
                    let ttype_s = match &ttype {
                        Val::Str(s) => s.to_string(),
                        _ => String::new(),
                    };
                    let neg = || if g("neg").truthy() { "Not " } else { "" };
                    let actor_prefix = || {
                        let a = g("actor");
                        if a.truthy() {
                            format!("{} ", lstr(&a, "field 'actor'"))
                        } else {
                            String::new()
                        }
                    };
                    // L476
                    if ttype_s == "Condition" || ttype_s == "ActorCondition" {
                        // L477
                        let cond = if g("var").truthy() || g("varList").truthy() {
                            format!(": {}{}", neg(), Self::format_var_name_or_list(&g("var"), &g("varList")))
                        } else {
                            String::new()
                        };
                        // L478
                        let a = g("actor");
                        let ap = if a.truthy() {
                            let s = lstr(&a, "field 'actor'");
                            format!(
                                "{}{} ",
                                pob_lua::string::upper(&pob_lua::string::sub(&s, 1, Some(1))),
                                pob_lua::string::sub(&s, 2, None)
                            )
                        } else {
                            String::new()
                        };
                        desc = Some(format!("{ap}Condition{cond}"));
                    } else if ttype_s == "Multiplier" {
                        // L480
                        let base = if g("base").truthy() {
                            Val::from(format!(
                                "{} + {} ",
                                Self::format_mod_base(&rmod, g("base").num()),
                                tostring_num(m_abs(mval.num()))
                            ))
                        } else {
                            baseVal.clone()
                        };
                        // L481 DIVERGENCE(div-var)
                        let div = Self::tag_div(&tag, &actor, &cfg);
                        let divs = if div.truthy() {
                            format!("{} ", lstr(&div, "field 'div'"))
                        } else {
                            String::new()
                        };
                        desc = Some(format!(
                            "{}per {divs}{}",
                            lstr(&base, "local 'base'"),
                            Self::format_var_name_or_list(&g("var"), &g("varList"))
                        ));
                        // L482
                        baseVal = Val::from("");
                    } else if ttype_s == "PerStat" {
                        // L484
                        let base = if g("base").truthy() {
                            Val::from(format!(
                                "{} + {} ",
                                Self::format_mod_base(&rmod, g("base").num()),
                                tostring_num(m_abs(mval.num()))
                            ))
                        } else {
                            baseVal.clone()
                        };
                        // L485 DIVERGENCE(div-var)
                        let div = Self::tag_div(&tag, &actor, &cfg);
                        let divs = if div.truthy() {
                            lstr(&div, "field 'div'")
                        } else {
                            "1".to_owned()
                        };
                        desc = Some(format!(
                            "{}per {divs} {}{}",
                            lstr(&base, "local 'base'"),
                            actor_prefix(),
                            Self::format_var_name_or_list(&g("stat"), &g("statList"))
                        ));
                        // L486
                        baseVal = Val::from("");
                    } else if ttype_s == "PercentStat" {
                        // L488
                        let percent = g("percent");
                        let percent = if percent.truthy() { percent.num() } else { 1.0 };
                        let finalPercent = (mval.num() * (percent / 100.0)) * 100.0;
                        // L489
                        let base = if g("base").truthy() {
                            format!(
                                "{} + {} ",
                                Self::format_mod_base(&rmod, g("base").num()),
                                tostring_num(m_abs(finalPercent))
                            )
                        } else {
                            Self::format_mod_base(&rmod, finalPercent)
                        };
                        // L490
                        let pv = g("percentVar");
                        let var = if pv.truthy() { pv } else { g("stat") };
                        desc = Some(format!(
                            "{base}% of {}{}",
                            actor_prefix(),
                            Self::format_var_name_or_list(&var, &g("statList"))
                        ));
                        // L491
                        baseVal = Val::from("");
                    } else if ttype_s == "MultiplierThreshold" || ttype_s == "StatThreshold" {
                        // L493
                        let var = if g("var").truthy() { g("var") } else { g("stat") };
                        let list = if g("varList").truthy() { g("varList") } else { g("statList") };
                        let tp = g("thresholdPercent");
                        let tps = if tp.truthy() {
                            format!("{}% ", lstr(&tp, "field 'thresholdPercent'"))
                        } else {
                            String::new()
                        };
                        let th = g("threshold");
                        let ths = if th.truthy() {
                            lstr(&th, "field 'threshold'")
                        } else {
                            let tv = g("thresholdVar");
                            let tv = if tv.truthy() { tv } else { g("thresholdStat") };
                            Self::format_mod_name(&lstr(&tv, "local 'modName'"))
                        };
                        desc = Some(format!(
                            "If {}{}{tps}{ths}",
                            Self::format_var_name_or_list(&var, &list),
                            if g("upper").truthy() { " <= " } else { " >= " }
                        ));
                    } else if ttype_s == "SkillName" {
                        // L495
                        let snl = g("skillNameList");
                        let s = if snl.truthy() {
                            concat_list(&snl, " / ")
                        } else {
                            lstr(&g("skillName"), "field 'skillName'")
                        };
                        desc = Some(format!("Skill: {s}"));
                    } else if ttype_s == "SkillId" {
                        // L497
                        let id = lstr(&g("skillId"), "field 'skillId'");
                        let ge = build
                            .data
                            .skills
                            .get(&Name::from(id.as_str()))
                            .cloned()
                            .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
                        desc = Some(format!("Skill: {}", ge.name));
                    } else if ttype_s == "SkillType" {
                        // L499 ORDER: (c) see the module doc
                        let st_ = g("skillType");
                        if let Some(name) = st_
                            .opt_num()
                            .filter(|x| x.fract() == 0.0 && *x >= 0.0 && *x <= 255.0)
                            .and_then(|x| SkillType::name(x as SkillType::T))
                        {
                            // L500-502
                            desc = Some(format!("Skill type: {}{}", neg(), Self::format_mod_name(name)));
                        }
                        // L505
                        if desc.is_none() {
                            // L506
                            desc = Some(format!("Skill type: {}?", neg()));
                        }
                    } else if ttype_s == "BaseFlag" {
                        // L509
                        desc = Some(format!(
                            "Base flag: {}{}",
                            neg(),
                            Self::format_mod_name(&ltostring(&g("baseFlag")))
                        ));
                    } else if ttype_s == "SlotNumber" {
                        // L511
                        desc = Some(format!("When in slot #{}", lstr(&g("num"), "field 'num'")));
                    } else if ttype_s == "GlobalEffect" {
                        // L513
                        desc = Some(Self::format_mod_name(&lstr(&g("effectType"), "local 'modName'")));
                    } else if ttype_s == "Limit" {
                        // L515
                        let lv = g("limitVar");
                        let s = if lv.truthy() {
                            Self::format_mod_name(&lstr(&lv, "local 'modName'"))
                        } else {
                            Self::format_mod_base(&rmod, g("limit").num())
                        };
                        desc = Some(format!("Limited to {s}"));
                    } else if ttype_s == "MonsterTag" {
                        // L517
                        let mtl = g("monsterTagList");
                        let s = if mtl.truthy() {
                            concat_list(&mtl, "/")
                        } else {
                            lstr(&g("monsterTag"), "field 'monsterTag'")
                        };
                        desc = Some(format!("Monster Tag: {s}"));
                    } else {
                        // L519
                        desc = Some(Self::format_mod_name(&lstr(&ttype, "local 'modName'")));
                    }
                    // L521
                    if let Some(desc) = desc {
                        // L522
                        let cur = row.get(n!("tags"));
                        let prefix = if cur.truthy() {
                            format!("{}, ", lstr(&cur, "field 'tags'"))
                        } else {
                            String::new()
                        };
                        row.set(n!("tags"), format!("{prefix}{desc}"));
                    }
                }
            }
        }
    }

    /// `tag.div` (L481, L485), DIVERGENCE(div-var): a `divVar` tag's `div` is
    /// `actor.modDB:GetMultiplier(tag.divVar, cfg)`.
    fn tag_div(tag: &Val, actor: &ActorRef, cfg: &Cfg) -> Val {
        let divVar = tag.get(n!("divVar"));
        if divVar.truthy() {
            let v = Name::from(lstr(&divVar, "field 'divVar'").as_str());
            Val::Num(actor.modDB.get().GetMultiplier(v, Some(&**cfg), false))
        } else {
            tag.get(n!("div"))
        }
    }

    /// lua: CalcBreakdownControl.lua:529-531 `FormatModName(modName)` (the first value)
    /// @pob Classes/CalcBreakdownControl.lua:529-531 h=154d7b4b
    pub fn format_mod_name(modName: &str) -> String {
        // L530
        let s = pob_ui_core::luastr::gsub(modName, "([%l%d]:?)(%u)", "%1 %2");
        pob_ui_core::luastr::gsub(&s, "(%l)(%d)", "%1 %2")
    }

    /// lua: CalcBreakdownControl.lua:533-535 `FormatVarNameOrList(var, varList)`
    /// @pob Classes/CalcBreakdownControl.lua:533-535 h=2aca6bbc
    pub fn format_var_name_or_list(var: &Val, varList: &Val) -> String {
        // L534
        if var.truthy() {
            Self::format_mod_name(&lstr(var, "local 'modName'"))
        } else {
            Self::format_mod_name(&concat_list(varList, " / "))
        }
    }

    /// lua: CalcBreakdownControl.lua:537-539 `FormatModBase(mod, base)`
    /// @pob Classes/CalcBreakdownControl.lua:537-539 h=04fdf10c
    pub fn format_mod_base(r#mod: &Val, base: f64) -> String {
        // L538
        if matches!(r#mod.get(n!("type")), Val::Str(ref s) if &**s == "BASE") {
            pob_lua::lua_format("%+g", &[pob_lua::FmtArg::from(m_abs(base))])
        } else {
            format!("{}%", tostring_num(m_abs(base)))
        }
    }

    /// lua: CalcBreakdownControl.lua:541-569 `FormatModValue(value, modType)`
    /// @pob Classes/CalcBreakdownControl.lua:541-569 h=66689c32
    pub fn format_mod_value(value: &Val, modType: &Val) -> Val {
        let mt = match modType {
            Val::Str(s) => s.to_string(),
            _ => String::new(),
        };
        // L542
        if mt == "BASE" {
            // L543
            Val::from(pob_lua::lua_format("%+g base", &[pob_lua::FmtArg::from(value.num())]))
        } else if mt == "INC" {
            // L545
            if lua_le(&Val::Num(0.0), value) {
                // L546
                Val::from(format!("{}% increased", lstr(value, "local 'value'")))
            } else {
                // L548
                Val::from(format!("{}% reduced", tostring_num(-value.num())))
            }
        } else if mt == "MORE" {
            // L551
            if lua_le(&Val::Num(0.0), value) {
                // L552
                Val::from(format!("{}% more", lstr(value, "local 'value'")))
            } else {
                // L554
                Val::from(format!("{}% less", tostring_num(-value.num())))
            }
        } else if mt == "OVERRIDE" {
            // L557
            Val::from(format!("Override: {}", lstr(value, "local 'value'")))
        } else if mt == "FLAG" {
            // L559
            Val::from(if value.truthy() { "True" } else { "False" })
        } else if mt == "LIST" {
            // L561
            let m = value.get(n!("mod"));
            if m.truthy() {
                // L562
                Val::from(format!(
                    "Modifier: {}",
                    Self::format_mod_name(&lstr(&m.get(n!("name")), "local 'modName'"))
                ))
            } else {
                // L564
                Val::from("?")
            }
        } else {
            // L567
            value.clone()
        }
    }

    /// lua: CalcBreakdownControl.lua:571-653 `DrawBreakdownTable(viewPort, x, y, section)`
    /// @pob Classes/CalcBreakdownControl.lua:571-653 h=5692fe9e
    pub fn draw_breakdown_table(cx: &mut Cx, me: CtrlId, vp: &ViewPort, x: f64, y: f64, si: usize) {
        let (label, footer, row_list, col_list, s_width, s_height) = {
            let s = &st(cx, me).section_list[si];
            let SectionKind::Table {
                label,
                footer,
                row_list,
                col_list,
            } = &s.kind
            else {
                unreachable!("DrawBreakdownTable on a non-TABLE section")
            };
            (label.clone(), footer.clone(), row_list.clone(), col_list.clone(), s.width, s.height)
        };
        let mut y = y;
        // L572
        let (cursorX, cursorY) = cx.sys.cursor_pos();
        // L573
        if label.truthy() {
            // Draw table label if able
            // L575
            let s = format!("^7{}:", lstr(&label, "field 'label'"));
            cx.sg.draw_string(x + 2.0, y, Align::Left, 16.0, Font::Var, &s);
            // L576
            y += 16.0;
        }
        // L578
        let mut colX = x + 4.0;
        // L579
        for (index, col) in col_list.ipairs() {
            let col = tbl(&col, "local 'col'");
            // L580
            let cw = col.get(n!("width"));
            if cw.truthy() {
                // Column is present, draw the separator and label
                // L582
                col.set(n!("x"), colX);
                // L583
                if index > 1 {
                    // Skip the separator for the first column
                    // L585
                    cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    // L586
                    let h = s_height
                        - if label.truthy() { 16.0 } else { 0.0 }
                        - if footer.truthy() { 12.0 } else { 0.0 };
                    cx.sg.draw_image(None, colX - 2.0, y, 1.0, h);
                }
                // L588
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L589
                let l = lstr_arg(&col.get(n!("label")), "DrawString");
                cx.sg.draw_string(colX, y + 2.0, Align::Left, 16.0, Font::Var, &l);
                // L590
                colX += cw.num();
            }
        }
        // L593
        let mut rowY = y + 20.0;
        // L594
        for (_, row) in row_list.ipairs() {
            let row = tbl(&row, "local 'row'");
            // Draw row separator
            // L596
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
            // L597
            cx.sg.draw_image(None, x + 2.0, rowY - 1.0, s_width - 4.0, 1.0);
            // L598
            for (_, col) in col_list.ipairs() {
                let col = tbl(&col, "local 'col'");
                let key = col.get(n!("key"));
                let cw = col.get(n!("width"));
                let v = row.get(key_of(&key));
                // L599
                if cw.truthy() && v.truthy() {
                    let colW = cw.num();
                    let colXv = col.get(n!("x")).num();
                    // This row has an entry for this column, draw it
                    // counts letters in the string
                    // L601
                    let alpha = gsub_count(&v, "%a", " ");
                    // counts " to " in the string
                    // L602
                    let notes = gsub_count(&v, " to ", " ");
                    // counts parenthesis in the string
                    // L603
                    let paren = gsub_count(&v, "%b()", " ");
                    let right = col.truthy(n!("right"));
                    // L604
                    if (alpha == 0 || notes > 0 || paren > 0) && right {
                        // L605
                        let s = format!("^7{}", format_num_sep_main(&cx.main, &ltostring(&v)));
                        cx.sg.draw_string(colXv + colW - 4.0, rowY + 1.0, Align::RightX, 12.0, Font::Var, &s);
                    } else if alpha == 0 || notes > 0 || paren > 0 {
                        // L607
                        let s = format!("^7{}", format_num_sep_main(&cx.main, &ltostring(&v)));
                        cx.sg.draw_string(colXv, rowY + 1.0, Align::Left, 12.0, Font::Var, &s);
                    } else {
                        // L609
                        let s = format!("^7{}", ltostring(&v));
                        cx.sg.draw_string(colXv, rowY + 1.0, Align::Left, 12.0, Font::Var, &s);
                    }
                    // L611
                    let keyS = lstr(&key, "field 'key'");
                    let ttFunc = row.get(Name::from(format!("{keyS}Tooltip").as_str())).obj::<CellTooltip>();
                    // L612
                    let ttNodeV = row.get(Name::from(format!("{keyS}Node").as_str()));
                    let ttNode = ttNodeV.obj::<model::Node>();
                    // L613
                    if (ttFunc.is_some() || ttNodeV.truthy())
                        && cursorY >= vp.y + 2.0
                        && cursorY < vp.y + vp.height - 2.0
                        && cursorX >= colXv
                        && cursorY >= rowY
                        && cursorX < colXv + colW
                        && cursorY < rowY + 14.0
                    {
                        // Mouse is over the cell, draw highlighting lines and show the tooltip/node location
                        // L615
                        cx.sg.set_draw_layer(None, Some(15));
                        // L616
                        cx.sg.set_draw_color(0.0, 1.0, 0.0);
                        // L617
                        cx.sg.draw_image(None, colXv - 2.0, rowY - 1.0, colW, 1.0);
                        // L618
                        cx.sg.draw_image(None, colXv - 2.0, rowY + 13.0, colW, 1.0);
                        // L619
                        if let Some(ttFunc) = ttFunc {
                            let f = ttFunc.0.clone();
                            let vp = *vp;
                            cx.lend::<CalcBreakdownControl, Tooltip, _>(me, |w| &mut w.tooltip, |cx, tt| {
                                // L620
                                tt.clear(false);
                                // L621
                                f(cx, tt);
                                // L622
                                tt.draw(cx, colXv, rowY, Some(colW), Some(12.0), &vp);
                            });
                        } else if let Some(ttNode) = ttNode.filter(|n| !n.x.get().is_nan() && !n.y.get().is_nan()) {
                            // The source "node" from cluster jewels don't know their location because it's the abstract node in tree.lua rather than the generated node from the cluster jewel.
                            // L624
                            let mut viewerX = colXv + colW + 5.0;
                            // L625
                            if vp.x + vp.width < viewerX + 304.0 {
                                // L626
                                viewerX = colXv - 309.0;
                            }
                            // L628
                            let viewerY = m_min(rowY, vp.y + vp.height - 304.0);
                            // L629
                            cx.sg.set_draw_color(1.0, 1.0, 1.0);
                            // L630
                            cx.sg.draw_image(None, viewerX, viewerY, 304.0, 304.0);
                            // L631 local viewer = self.nodeViewer
                            // L632 viewer.zoom = 5
                            // L633
                            let size = cx.build().model.spec.get().tree.get().extra.get(n!("size"));
                            let scale = size.num() / 1500.0;
                            // L634
                            let zoomX = -ttNode.x.get() / scale;
                            // L635
                            let zoomY = -ttNode.y.get() / scale;
                            // L636
                            cx.sg.set_viewport(viewerX + 2.0, viewerY + 2.0, 300.0, 300.0);
                            // L637
                            let h = breakdown_hooks();
                            (h.node_viewer_draw)(
                                cx,
                                me,
                                5.0,
                                zoomX,
                                zoomY,
                                &ViewPort { x: 0.0, y: 0.0, width: 300.0, height: 300.0 },
                            );
                            // L638
                            cx.sg.set_draw_layer(None, Some(30));
                            // L639
                            cx.sg.set_draw_color(1.0, 0.0, 0.0);
                            // L640
                            let ring = (h.node_viewer_highlight_ring)(cx, me);
                            cx.sg.draw_image(ring.as_ref(), 135.0, 135.0, 30.0, 30.0);
                            // L641
                            cx.sg.reset_viewport();
                        }
                        // L643
                        cx.sg.set_draw_layer(None, Some(10));
                    }
                }
            }
            // L647
            rowY += 14.0;
        }
        // L649
        if footer.truthy() {
            // Draw table footer if able
            // L651
            let s = format!("^7{}", lstr(&footer, "field 'footer'"));
            cx.sg.draw_string(x + 2.0, rowY, Align::Left, 12.0, Font::Var, &s);
        }
    }

    /// lua: CalcBreakdownControl.lua:655-688 `DrawRadiusVisual(x, y, width, height, radius)`
    /// @pob Classes/CalcBreakdownControl.lua:655-688 h=5a2c9d4c
    pub fn draw_radius_visual(cx: &mut Cx, me: CtrlId, x: f64, y: f64, width: f64, height: f64, radius: f64) {
        // L656
        cx.sg.set_draw_color(0.75, 0.75, 0.75);
        // L657
        let rg = st(cx, me).range_guide.clone();
        cx.sg.draw_image(Some(&rg), x, y, width, height);
        // L658-668 (commented-out code in the Lua)
        // L669
        cx.sg.set_draw_color_a(0.5, 1.0, 0.5, 0.33);
        // L670
        pob_ui_core::draw::render_circle(cx, x, y, width, height, 0.0, 0.0, radius);
        // L671-685 (commented-out code in the Lua)
        // L686
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L687
        let ov = st(cx, me).ui_overlay.clone();
        cx.sg.draw_image(Some(&ov), x, y, width, height);
    }

    /// lua: CalcBreakdownControl.lua:690-753 `Draw(viewPort)`
    /// @pob Classes/CalcBreakdownControl.lua:690-753 h=77a52329
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort) {
        // L691
        let sourceData = st(cx, me).source_data.clone();
        // L692
        let scrollBar = Self::scroll_bar(cx, me);
        let w = st(cx, me);
        let (contentWidth, contentHeight) = (
            w.content_width
                .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field 'contentWidth' (a nil value)")),
            w.content_height
                .unwrap_or_else(|| lua_error!("attempt to compare nil with number")),
        );
        // L693
        let mut width = contentWidth;
        // L694
        let mut height = contentHeight;
        // L695
        let borderThickness = w.border_thickness;
        // L696
        if contentHeight > vp.height {
            // Content won't fit the screen height, so set the scrollbar
            // L698
            width = contentWidth + cx.base(scrollBar).width.raw();
            // L699
            height = vp.height;
            // L700
            cx.base_mut(scrollBar).height = Prop::Val(height - borderThickness * 2.0);
            // L701
            ScrollBarControl::set_content_dimension(
                cx,
                scrollBar,
                contentHeight - borderThickness * 2.0,
                vp.height - borderThickness * 2.0,
            );
        } else {
            // L703
            ScrollBarControl::set_content_dimension(cx, scrollBar, 0.0, 0.0);
        }
        {
            let b = cx.base_mut(me);
            // L705
            b.width = Prop::Val(width);
            // L706
            b.height = Prop::Val(height);
        }
        // Calculate position based on the source cell
        let sourceData = sourceData
            .unwrap_or_else(|| lua_error!("attempt to index local 'sourceData' (a nil value)"));
        // L708
        let mut x = sourceData.num(n!("x")) + sourceData.num(n!("width")) + 5.0;
        // L709
        let mut y = m_min(sourceData.num(n!("y")), vp.y + vp.height - height);
        // L710
        if x + width > vp.x + vp.width {
            // L711
            x = m_max(vp.x, sourceData.num(n!("x")) - 5.0 - width);
        }
        {
            let b = cx.base_mut(me);
            // L713
            b.x = Prop::Val(x);
            // L714
            b.y = Prop::Val(y);
        }
        // Draw background
        // L716
        cx.sg.set_draw_layer(None, Some(10));
        // L717
        cx.sg.set_draw_color_a(0.0, 0.0, 0.0, 0.9);
        // L718
        cx.sg.draw_image(
            None,
            x + borderThickness,
            y + borderThickness,
            width - borderThickness * 2.0,
            height - borderThickness * 2.0,
        );
        // Draw border (this is put in sub layer 11 so it draws over the contents, in case they don't fit the screen)
        // L720
        cx.sg.set_draw_layer(None, Some(11));
        // L721
        let [r, g, b] = if st(cx, me).pinned {
            // L722
            st(cx, me).pinned_colour
        } else {
            // L724
            st(cx, me).border_colour
        };
        cx.sg.set_draw_color(r, g, b);
        // L726
        cx.sg.draw_image(None, x, y, width, borderThickness);
        // L727
        cx.sg.draw_image(None, x, y + height - borderThickness, width, borderThickness);
        // L728
        cx.sg.draw_image(None, x, y, borderThickness, height);
        // L729
        cx.sg.draw_image(None, x + width - borderThickness, y, borderThickness, height);
        // L730
        cx.sg.set_draw_layer(None, Some(10));
        // L731
        cx.draw_controls(me, vp, None);
        // Draw the sections
        // L733
        y -= cx.w::<ScrollBarControl>(scrollBar).offset;
        // L734
        let n_sections = st(cx, me).section_list.len();
        for i in 0..n_sections {
            // L735
            let sectionY = y + st(cx, me).section_list[i].offset;
            enum K {
                Text(Val, f64),
                Table,
                Radius(f64, f64, Val),
            }
            let k = {
                let s = &st(cx, me).section_list[i];
                match &s.kind {
                    SectionKind::Text { lines, text_size } => K::Text(lines.clone(), *text_size),
                    SectionKind::Table { .. } => K::Table,
                    SectionKind::Radius { radius } => K::Radius(s.width, s.height, radius.clone()),
                }
            };
            match k {
                // L736
                K::Text(lines, textSize) => {
                    // L737
                    let mut lineY = sectionY + 2.0;
                    // L738
                    let mut li = 1i64;
                    loop {
                        let line = lines.get(TKey::from(li));
                        if line.is_nil() {
                            break;
                        }
                        li += 1;
                        // L739
                        cx.sg.set_draw_color(1.0, 1.0, 1.0);
                        // counts decimals with 2 or more digits
                        // L740
                        let _dec = gsub_count(&line, "%.%d%d.", " ");
                        // L741
                        let s = format_num_sep_main(&cx.main, &lstr_arg(&line, "gsub"));
                        cx.sg.draw_string(x + 4.0, lineY, Align::Left, textSize, Font::Var, &s);
                        // L742
                        lineY += textSize;
                    }
                }
                // L744
                K::Table => {
                    // L745
                    Self::draw_breakdown_table(cx, me, vp, x, sectionY, i);
                }
                // L746
                K::Radius(sw, sh, radius) => {
                    // L747
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    // L748
                    cx.sg.draw_image(None, x + 2.0, sectionY, sw - 4.0, sh);
                    // L749
                    Self::draw_radius_visual(cx, me, x + 4.0, sectionY + 2.0, sw - 8.0, sh - 4.0, radius.num());
                }
            }
        }
        // L752
        cx.sg.set_draw_layer(None, Some(0));
    }

    /// lua: CalcBreakdownControl.lua:755-777 `OnKeyDown(key, doubleClick)`
    /// @pob Classes/CalcBreakdownControl.lua:755-777 h=d57716a5
    pub fn on_key_down(cx: &mut Cx, me: CtrlId, key: Key, _double_click: bool) -> Focus {
        // L756
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L757
            return None;
        }
        // L759
        let mOverControl = cx.get_mouse_over_control(me);
        // L760
        if let Some(m) = mOverControl.filter(|&m| cx.has(m, Caps::KEY_DOWN)) {
            // L761 (`OnKeyDown(key)`: no doubleClick)
            return cx.on_key_down(m, key, false);
        }
        // L763
        let mOver = Self::is_mouse_over(cx, me);
        // L764
        if key.name().contains("BUTTON") {
            // L765
            if !mOver {
                // Mouse click outside the control, hide the breakdown
                // L767
                if let Some(f) = st(cx, me).clear_display_func.clone() {
                    // L768
                    f(cx);
                } else {
                    // L770
                    let tab = Self::calcs_tab(cx, me);
                    CalcsTab::clear_display_stat(cx, tab);
                }
                // L772
                cx.base_mut(me).shown = Prop::Val(false);
                // L773
                return None;
            }
        }
        // L776
        Some(me)
    }

    /// lua: CalcBreakdownControl.lua:779-789 `OnKeyUp(key)`
    /// @pob Classes/CalcBreakdownControl.lua:779-789 h=45ce78d3
    pub fn on_key_up(cx: &mut Cx, me: CtrlId, key: Key) -> Focus {
        // L780
        if !cx.is_shown(me) || !cx.is_enabled(me) {
            // L781
            return None;
        }
        let sb = Self::scroll_bar(cx, me);
        // L783
        if ScrollBarControl::is_scroll_down_key(cx, sb, key) {
            // L784
            ScrollBarControl::scroll(cx, sb, 1.0);
        } else if ScrollBarControl::is_scroll_up_key(cx, sb, key) {
            // L785-786
            ScrollBarControl::scroll(cx, sb, -1.0);
        }
        // L788
        Some(me)
    }
}

/// A table key from a column's `key` value.
#[track_caller]
fn key_of(v: &Val) -> TKey {
    match v {
        Val::Str(s) => TKey::from(Name::from(s)),
        Val::Num(x) => TKey::from(*x),
        Val::Bool(b) => TKey::from(*b),
        _ => lua_error!("table index is nil"),
    }
}

/// `s:match(p)` as a value (nil when it does not match).
fn match_val(v: &Val, p: &str) -> Val {
    smatch(v, p, "field 'source'").map_or(Val::Nil, Val::from)
}

/// A column-list constructor `{ { label = .., key = .., right = true }, … }`.
fn col_list(cols: &[(&str, &str, bool)]) -> Table {
    let t = Table::new();
    for (label, key, right) in cols {
        let c = Table::new();
        c.set(n!("label"), *label);
        c.set(n!("key"), *key);
        if *right {
            c.set(n!("right"), true);
        }
        t.push(c);
    }
    t
}

// ---- the hook entries other units install (CCR-0111 §2, CCR-0104 §1, CCR-0098 §3) ----------

/// `CalcsHooks::breakdown_ctor`: `self.controls.breakdown =
/// new("CalcBreakdownControl"):CalcBreakdownControl(self)` (CalcsTab.lua:156).
pub fn calcs_breakdown_ctor(cx: &mut Cx, tab: CtrlId) -> CtrlId {
    cx.add(tab, "breakdown", CalcBreakdownControl::new(Some(tab))).id()
}

/// `CalcsHooks::set_breakdown_data`: `breakdown:SetBreakdownData(displayData, pinned)`.
pub fn calcs_set_breakdown_data(cx: &mut Cx, bd: CtrlId, data: Option<&Table>, pinned: bool) {
    CalcBreakdownControl::set_breakdown_data(cx, bd, data, pinned, None);
}

/// `CalcsHooks::breakdown_source_data`: `breakdown.sourceData`.
pub fn calcs_breakdown_source_data(cx: &Cx, bd: CtrlId) -> Option<Table> {
    st(cx, bd).source_data.clone()
}

/// `BuildHooks::breakdown`: Build.lua:616-621.
pub fn sidebar_breakdown_ctor(cx: &mut Cx, build: CtrlId, init: crate::build::init::BreakdownInit) {
    // L616 (`self.calcsTab`: DIVERGENCE(calcs-tab-late))
    let bd = cx.add(build, "breakdown", CalcBreakdownControl::new(None)).id();
    let w = st_mut(cx, bd);
    // L617
    w.pinned_colour = init.pinned_colour;
    // L618
    w.env_name = Some(init.env_name);
    // L619-621
    w.clear_display_func = Some(init.clear_display_func);
}

/// `ShellHooks::breakdown_pinned`: `self.controls.breakdown.pinned`.
pub fn sidebar_breakdown_pinned(cx: &Cx, bd: CtrlId) -> bool {
    st(cx, bd).pinned
}

/// `ShellHooks::breakdown_set_data`: `self.controls.breakdown:SetBreakdownData(unpack(breakdownInputs))`.
pub fn sidebar_breakdown_set_data(
    cx: &mut Cx,
    bd: CtrlId,
    inputs: Option<&crate::build::on_frame::BreakdownInputs>,
) {
    match inputs {
        Some(i) => CalcBreakdownControl::set_breakdown_data(
            cx,
            bd,
            Some(&i.data),
            i.pin,
            i.actor_name.as_deref(),
        ),
        None => CalcBreakdownControl::set_breakdown_data(cx, bd, None, false, None),
    }
}

/// `ShellHooks::breakdown_set_border_colour`: `self.controls.breakdown.borderColour = colour`.
pub fn sidebar_breakdown_set_border_colour(cx: &mut Cx, bd: CtrlId, colour: [f64; 3]) {
    st_mut(cx, bd).border_colour = colour;
}
