//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua:3162-4048 (DrawSummary, DrawStatList, DrawTree, DrawItemExpanded, abyss sockets, DrawItems).
//! Owner: U49.
//!
//! @pob-file Classes/CompareTab.lua @16de4b82d
//!
//! The Summary, Tree and Items sub-views of the Compare tab. U47's `Draw` reaches them through
//! `layout::CompareDrawHooks` (CCR-0137 §3); this unit installs its passes in
//! `layout::default_draw_hooks` (one line each, CCR-0143 §6): [`draw_summary_hook`],
//! [`draw_tree_hook`], [`draw_items_hook`].
//!
//! # Seams (CCR-0143)
//!
//! * `tradeHelpers` (`Classes/TradeHelpers.lua`, U51, deferred): DrawItemExpanded and DrawItems
//!   call `getRarityColor`, `modLineTemplate`, `modLineValue`, `buildModMap`,
//!   `getSlotDiffLabel`, `drawCopyButtons`, `drawCompactSlotRow` (with its local
//!   `fitItemName`) and read `ITEM_BOX_W`. U51 is not scheduled, so this file has private
//!   copies of them (TradeHelpers.lua:71-100, 401-610, statement for statement, `lua:`
//!   comments, no `@pob` markers: the range stays U51's), marked `// CCR-0143 §1`.
//! * `viewer:Draw(compareEntry, ...)` (L3480): U23's `PassiveTreeView::draw` draws the open
//!   build (`cx.build()`), it has no `build` parameter. The comparison build's tree is drawn
//!   with the open build's `model`, `calc` and `build_name` swapped for the entry's for the
//!   duration of the call ([`with_entry_as_build`], `// CCR-0143 §2`), so every `build.X` the
//!   view reads is the entry's, as in Lua.
//! * `GetCursorPos = function() ... end` (L3425, L3460, L3475): `pob_sg::Sys` has no cursor
//!   override. [`with_cursor_origin`] re-snapshots the cursor (`Sys::begin_frame` with the same
//!   DPI scale) so `cursor_pos()` returns the patched position for the call and the original
//!   one after it (`// CCR-0143 §3`). Exact for DPI scales >= 1 (every logical position has a
//!   physical one); the tree viewports' offsets are integers (`m_floor`), so the patched
//!   positions are integers as in Lua.
//! * `hoverItemsTab:AddItemTooltip(...)` (L3988) with `compareEntry.itemsTab`: U38's
//!   `AddItemTooltip` with the entry's `ItemsTabSelf` (FUP6: its stat differences are the
//!   entry's, against the entry's slots; CCR-0143 §4 closed).
//! * `slots[name].shown()` (AddAbyssSockets, L3685-3686): the model slot's `$shown` kinds, as
//!   power.rs (`// CCR-0138 §2`: calc:BM-A's `slot_IsShown` is private).
//! * `buySimilar.openPopup(item, slot, build)` (L3948, U51): U33's
//!   `ItemsUiHooks::buy_similar_open_popup` (its default raises naming U51; CCR-0143 §7).
//! * `itemSlotHelper.DrawViewer` (the jewel-socket label hover, L3896) draws through U32's
//!   `ItemsTabHooks::socket_viewer_draw` (U23's PassiveTreeView:Draw since CCR-0145 §5).
//! * `itemSlotHelper.DrawViewer` is U32's `slot_helper::DrawViewer`; `self.calcs.
//!   getMiscCalculator` the build's [`crate::calc::CalcFacade::misc_calculator`];
//!   `self.primaryBuild:AddStatComparesToTooltip` U21's `stats::add_stat_compares_to_tooltip`;
//!   `self:GetJewelComparisonSlots` / `CopyCompareItemToPrimary` U46's;
//!   `self:RunComparePowerReport` and the list's `SetProgress` / `SetReport` U48's.
//!
//! # Lua semantics
//!
//! * `primaryOutput[statData.stat] or 0` is a [`Val`]: `type(v) == "table"` is
//!   `Val::lua_type`; `v ~= 0` is the raw comparison (a string is never `0`); arithmetic coerces
//!   numeric strings and raises on anything else ([`Val::num`]).
//! * `s_format("%"..fmt, v)` is `lua_format` with the format built at run time.
//! * `type(primaryVal) == "table" and primaryVal[statData.childStat] or 0`: a nil or `false`
//!   child is 0.
//! * `tradeHelpers.modLineTemplate` returns `gsub`'s two values; its callers keep the first.
//!   `modLineValue` is `tonumber(line:match(..))`, nil when the line has no number.
//! * `otherModMap` is always a table (buildModMap returns `{}` for a nil item), so the diff
//!   markers run for an empty side too.
//! * `slotMissing` is nil for jewel rows (falsy).
//! * `inputEvents[id] = nil` is `InputEvents::consume`; `{}` is a fresh `InputEvents`.
//! * The hover locals (`hoverItem`, `hoverItemsTab`, ...) and the click locals are upvalues
//!   of the two local helper functions: [`ItemsFrame`] holds them.
//!
//! ORDER: `pairs(item.buffModLines)` (L3584) visits an array (the parser appends, no holes): its
//! `pairs` order is index order (c). `buildModMap` fills a map that is only looked up (c).
//!
//! DIVERGENCE(web-pending): `self.calcs.getMiscCalculator` / `calcFunc` answering `Pending`
//! (wasm only): the Equip tooltip is drawn without stat lines.
//!
//! Props with side effects (ui doc §7.3): none.

#![allow(non_snake_case)]

use std::collections::HashMap;
use std::rc::Rc;
use std::task::Poll;

use pob_lua::format::FmtArg;
use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::{lfmt, n, Name};
use pob_sg::{Align, Font, Key};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{CheckBoxControl, DropDownControl, DropItem};
use pob_ui_core::input::EventKind;
use pob_ui_core::luastr::{format_num_sep_main, sub};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::entry::{self, CompareEntry};
use super::init::{ctl, matchFlags, st, st_mut, CompareEntryRef, FlagSpec, LAYOUT};
use super::layout::CompareDrawHooks;
use super::power_list::ComparePowerReportListControl;
use super::{power, CompareTab};
use crate::app::CxApp;
use crate::build::display_stats::{self, DisplayStat};
use crate::model::{self, Field, ItemRef, NodeRef, Output, Override, Table, Val};
use crate::tabs::tree::TreeTab;
// not re-exported by model.rs (U15; CCR-0115, CCR-0123 §9)
use pob_calc::actor::ActorRef;
use pob_calc::classes::item::new_Item;
use pob_calc::classes::item_tools::formatModLine;

/// This unit's part of the split class struct (ui doc §12.1). DrawSummary/DrawTree/DrawItems
/// keep no state of their own: the fields they write (`summaryTotalContentHeight`,
/// `itemsColWidth`, `itemsScrollX`, `itemsTotalContentHeight`) are U46's / U47's.
#[derive(Default)]
pub struct State {}

// ---- the hook entries (CCR-0137 §3, CCR-0143 §6) ----------------------------------------------

/// `self:DrawSummary(contentVP, compareEntry)` as `CompareDrawHooks::draw_summary`.
pub fn draw_summary_hook(cx: &mut Cx, me: CtrlId, vp: &ViewPort, compareEntry: &CompareEntryRef) {
    CompareTab::DrawSummary(cx, me, vp, compareEntry);
}

/// `self:DrawTree(contentVP, inputEvents, compareEntry)` as `CompareDrawHooks::draw_tree`.
pub fn draw_tree_hook(
    cx: &mut Cx,
    me: CtrlId,
    vp: &ViewPort,
    inputEvents: &mut InputEvents,
    compareEntry: &CompareEntryRef,
) {
    CompareTab::DrawTree(cx, me, vp, inputEvents, compareEntry);
}

/// `self:DrawItems(contentVP, compareEntry, inputEvents)` as `CompareDrawHooks::draw_items`.
pub fn draw_items_hook(
    cx: &mut Cx,
    me: CtrlId,
    vp: &ViewPort,
    compareEntry: &CompareEntryRef,
    inputEvents: &mut InputEvents,
) {
    CompareTab::DrawItems(cx, me, vp, compareEntry, inputEvents);
}

/// Install this unit's three passes into a hook table (tests that start from a stand-in table).
pub fn install(h: &mut CompareDrawHooks) {
    h.draw_summary = Rc::new(draw_summary_hook);
    h.draw_tree = Rc::new(draw_tree_hook);
    h.draw_items = Rc::new(draw_items_hook);
}

// ---- small helpers (Rust-only) ----------------------------------------------------------------

/// `control.x = x; control.y = y` (raw number fields).
fn set_xy(cx: &mut Cx, id: CtrlId, x: f64, y: f64) {
    let b = cx.base_mut(id);
    b.x = Prop::Val(x);
    b.y = Prop::Val(y);
}

fn label_width(cx: &Cx, id: CtrlId) -> f64 {
    cx.w::<CheckBoxControl>(id).label_width
}

/// `control.selIndex = n` for a Lua number (an index outside the list selects nothing).
fn sel(n: f64) -> usize {
    if n.is_finite() && n >= 0.0 && n.fract() == 0.0 {
        n as usize
    } else {
        0
    }
}

/// `items[id]` of a build's ItemsTab (`id` a Lua value: nil/false and non-numbers give nil).
fn item_at(build: &model::BuildRef, id: &Val) -> Option<ItemRef> {
    let id = id.opt_num()?;
    if id.fract() != 0.0 || id < 0.0 || id > f64::from(u32::MAX) {
        return None;
    }
    build.itemsTab.items.with(|m| m.get(&(id as u32)).cloned())
}

/// `itemsTab.slots[slotName]`
fn slot_at(build: &model::BuildRef, slotName: &str) -> Option<Table> {
    build
        .itemsTab
        .slots
        .with(|s| s.get(&Name::from(slotName)).cloned())
}

/// `local pSlot = itemsTab.slots[slotName]; local pItem = pSlot and itemsTab.items[pSlot.selItemId]`
fn slot_item(build: &model::BuildRef, slotName: &str) -> Option<ItemRef> {
    slot_at(build, slotName).and_then(|s| item_at(build, &s.get(n!("selItemId"))))
}

/// `t[k]` of a Lua value (`t` a table; anything else reads nil here: the output values that
/// DrawStatList indexes are tables or numbers).
fn index(v: &Val, k: &str) -> Val {
    match v {
        Val::Table(t) => t.get(Name::from(k)),
        _ => Val::Nil,
    }
}

/// `x or 0`
fn or0(v: Val) -> Val {
    if v.truthy() {
        v
    } else {
        Val::Num(0.0)
    }
}

/// A Lua value as a `string.format` argument (a table raises, as `%d`/`%f` of a table does).
fn fa(v: &Val) -> FmtArg<'_> {
    match v {
        Val::Nil => FmtArg::Nil,
        Val::Bool(b) => FmtArg::Bool(*b),
        Val::Num(n) => FmtArg::Num(*n),
        Val::Str(s) => FmtArg::Str(s),
        other => lua_error!(
            "bad argument #2 to 'format' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// `x and x > 0` (a nil or false `x` is falsy; a non-number raises the comparison error).
fn gt0(v: &Val) -> bool {
    match v {
        Val::Nil | Val::Bool(false) => false,
        Val::Num(n) => *n > 0.0,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}

/// `t.k` of a `weaponData`/`armourData`/`flaskData` table.
fn tk(t: &Table, k: &str) -> Val {
    t.get(Name::from(k))
}

/// `modLine.line` (a string; nil raises where the Lua passes it to a string function).
fn mod_line_text(modLine: &Table) -> String {
    modLine
        .str(n!("line"))
        .map(|s| s.to_string())
        .unwrap_or_else(|| lua_error!("attempt to index local 'line' (a nil value)"))
}

/// The open build's ItemsTab node (`self.primaryBuild.itemsTab`).
fn items_tab_node(cx: &Cx) -> Option<CtrlId> {
    crate::tabs::items::slot_helper::items_tab(cx)
}

/// `{ [node] = true }` as an `addNodes` set.
fn node_set(node: &NodeRef) -> pob_lua::Shared<model::LjIntMap<NodeRef>> {
    let mut m = model::LjIntMap::new();
    m.set(node.id as i64, Some(node.clone()));
    pob_lua::Shared::new(m)
}

/// `GetCursorPos = function() local x, y = origGetCursorPos() return x - ox, y - oy end`
/// around `f`, then `GetCursorPos = origGetCursorPos` (module notes, CCR-0143 §3).
fn with_cursor_origin<R>(cx: &mut Cx, ox: f64, oy: f64, f: impl FnOnce(&mut Cx) -> R) -> R {
    // CCR-0143 §3
    let (x, y) = cx.sys.cursor_pos();
    let scale = cx.sg.scale();
    set_cursor_logical(cx, x - ox, y - oy, scale);
    let r = f(cx);
    set_cursor_logical(cx, x, y, scale);
    r
}

/// Re-snapshot the cursor so that `cursor_pos()` (`round(phys / scale)`) reads `(x, y)`.
fn set_cursor_logical(cx: &mut Cx, x: f64, y: f64, scale: f32) {
    let phys = |v: f64| -> i32 {
        let base = (v * f64::from(scale)).round() as i32;
        // the physical position whose rounding gives v (it exists for scale >= 1)
        [base, base - 1, base + 1, base - 2, base + 2]
            .into_iter()
            .find(|p| f64::from(((*p as f32) / scale).round()) == v)
            .unwrap_or(base)
    };
    let p = (phys(x), phys(y));
    cx.sys.begin_frame(p, scale);
}

/// `viewer:Draw(compareEntry, ...)`: the open build's model, calc facade and name swapped for
/// the entry's while `f` runs (module notes, CCR-0143 §2; U47's `entry::with_entry_as_build`
/// since FUP6, which the entry's ItemsTab and ConfigTab methods use too).
fn with_entry_as_build<R>(cx: &mut Cx, entry: &CompareEntryRef, f: impl FnOnce(&mut Cx) -> R) -> R {
    // CCR-0143 §2
    entry::with_entry_as_build(cx, entry, f)
}

// ---- tradeHelpers copies (U51's TradeHelpers.lua; CCR-0143 §1) ---------------------------------

/// `local ITEM_BOX_W = 310` / `M.ITEM_BOX_W` (TradeHelpers.lua:519-520)
const ITEM_BOX_W: f64 = 310.0;
/// `local ITEM_BOX_H = 20` (TradeHelpers.lua:521)
const ITEM_BOX_H: f64 = 20.0;

/// lua: TradeHelpers.lua:71-79 `M.getRarityColor(item)` (CCR-0143 §1)
fn getRarityColor(cx: &Cx, item: Option<&ItemRef>) -> String {
    // L72
    let Some(item) = item else {
        return "^7".to_owned();
    };
    // L73 (`item.rarity` is always a string)
    let rarity = item.rarity.get();
    match cx.main.cc.get(rarity.as_str()) {
        // L74
        Some(c) => c.clone(),
        // L76
        None => "^7".to_owned(),
    }
}

/// lua: TradeHelpers.lua:82-85 `M.modLineTemplate(line)` (first value; CCR-0143 §1)
fn modLineTemplate(line: &str) -> String {
    // Replace decimal numbers first (e.g. "1.5"), then integers
    // L84
    pob_lua::gsub(line, "%-?[%d]+%.?[%d]*", "#", None).0
}

/// lua: TradeHelpers.lua:91-99 `M.modLineValue(line, onlyFromTo)` (CCR-0143 §1)
fn modLineValue(line: &str, onlyFromTo: bool) -> Option<f64> {
    // L92
    let caps = pob_lua::pattern::r#match(line, "(%-?%d+%.?%d*) to (%-?%d+%.?%d*)", None);
    // L93
    if let Some(c) = caps {
        let low = pob_lua::tonumber(c[0].as_str());
        let high = pob_lua::tonumber(c[1].as_str());
        // L94 (the captures are numerals)
        let (Some(low), Some(high)) = (low, high) else {
            lua_error!("attempt to perform arithmetic on a nil value")
        };
        return Some((low + high) / 2.0);
    } else if onlyFromTo {
        // L96
        return None;
    }
    // L98
    pob_lua::pattern::r#match(line, "%-?[%d]+%.?[%d]*", None)
        .and_then(|c| pob_lua::tonumber(c[0].as_str()))
}

/// One `modMap[template]` entry of `buildModMap` (`{ line = ..., value = ... }`).
struct ModEntry {
    line: String,
    value: Option<f64>,
}

type ModMap = HashMap<String, ModEntry>;

/// `{item.enchantModLines or {}, item.scourgeModLines or {}, item.implicitModLines or {},
/// item.explicitModLines or {}, item.crucibleModLines or {}}`
fn mod_line_lists(item: &ItemRef) -> [Vec<Table>; 5] {
    [
        item.enchantModLines.get().snapshot(),
        item.scourgeModLines.get().snapshot(),
        item.implicitModLines.get().snapshot(),
        item.explicitModLines.get().snapshot(),
        item.crucibleModLines.get().snapshot(),
    ]
}

/// lua: TradeHelpers.lua:401-416 `M.buildModMap(item)` (CCR-0143 §1)
fn buildModMap(item: Option<&ItemRef>) -> ModMap {
    // L402
    let mut modMap = ModMap::new();
    // L403
    let Some(item) = item else { return modMap };
    // L404
    for modList in mod_line_lists(item) {
        // L405
        for modLine in modList {
            // L406
            if item.CheckModLineVariant(&modLine) {
                // L407
                let formatted = formatModLine(&modLine, false);
                // L408
                if formatted.is_some() {
                    let line = mod_line_text(&modLine);
                    // L409
                    let template = modLineTemplate(&line);
                    // L410
                    let value = modLineValue(&line, false);
                    modMap.insert(template, ModEntry { line, value });
                }
            }
        }
    }
    // L415
    modMap
}

/// lua: TradeHelpers.lua:421-435 `M.getSlotDiffLabel(pItem, cItem)` (CCR-0143 §1)
fn getSlotDiffLabel(cx: &Cx, pItem: Option<&ItemRef>, cItem: Option<&ItemRef>) -> String {
    let cc = &cx.main.cc;
    // L422
    if pItem.is_none() && cItem.is_none() {
        // L423
        return "^8(both empty)".to_owned();
    }
    // L425
    match (pItem, cItem) {
        (Some(p), Some(c)) if *p.name.get() == *c.name.get() => {
            // L426
            format!("{}(match)", cc.POSITIVE)
        }
        // L427-428
        (None, _) => format!("{}(missing)", cc.NEGATIVE),
        // L429-430
        (_, None) => format!("{}(extra)", cc.TIP),
        // L432
        _ => format!("{}(different)", cc.WARNING),
    }
}

/// `drawCopyButtons`' results `b1Hover, b2Hover, b3Hover, btn2X, btnY, equipW, btnH` (nil for
/// a row without them: every hover false).
#[derive(Clone, Copy, Default)]
struct CopyButtons {
    b1Hover: bool,
    b2Hover: bool,
    b3Hover: bool,
    b2X: f64,
    b2Y: f64,
    b2W: f64,
    b2H: f64,
}

/// lua: TradeHelpers.lua:440-496 `M.drawCopyButtons(cursorX, cursorY, btnStartX, btnY,
/// slotMissing, copyBtnW, copyBtnH, buyBtnW, equipBtnW)` (CCR-0143 §1)
#[allow(clippy::too_many_arguments)]
fn drawCopyButtons(
    cx: &mut Cx,
    cursorX: f64,
    cursorY: f64,
    btnStartX: f64,
    btnY: f64,
    slotMissing: bool,
    copyBtnW: f64,
    copyBtnH: f64,
    buyBtnW: f64,
    equipBtnW: f64,
) -> CopyButtons {
    // L441-444
    let btnW = copyBtnW;
    let btnH = copyBtnH;
    let buyW = buyBtnW;
    let equipW = equipBtnW;
    // L445-447
    let btn3X = btnStartX;
    let btn1X = btn3X + buyW + 4.0;
    let btn2X = btn1X + btnW + 4.0;

    // L449
    let drawBtn = |cx: &mut Cx, x: f64, w: f64, hover: bool, label: &str| {
        // L450
        let pressed = hover && cx.sys.is_key_down(Key::LeftButton);
        // Outer border
        // L452
        if hover {
            // L453
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
        } else {
            // L455
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        }
        // L457
        cx.sg.draw_image(None, x, btnY, w, btnH);
        // Inner fill
        // L459
        if pressed {
            // L460
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
        } else if hover {
            // L462
            cx.sg.set_draw_color(0.33, 0.33, 0.33);
        } else {
            // L464
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
        }
        // L466
        cx.sg
            .draw_image(None, x + 1.0, btnY + 1.0, w - 2.0, btnH - 2.0);
        // Label
        // L468
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L469
        cx.sg.draw_string(
            x + w / 2.0,
            btnY + 1.0,
            Align::CenterX,
            14.0,
            Font::Var,
            label,
        );
    };

    // "Buy" button
    // L473-474
    let b3Hover =
        cursorX >= btn3X && cursorX < btn3X + buyW && cursorY >= btnY && cursorY < btnY + btnH;
    // L475
    drawBtn(cx, btn3X, buyW, b3Hover, "^7Buy");

    // "Copy" button
    // L478-479
    let b1Hover =
        cursorX >= btn1X && cursorX < btn1X + btnW && cursorY >= btnY && cursorY < btnY + btnH;
    // L480
    drawBtn(cx, btn1X, btnW, b1Hover, "^7Copy");

    // L482
    let b2Hover;
    // L483
    if slotMissing {
        // Show "Missing slot" label instead of Equip button
        // L485
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L486
        cx.sg.draw_string(
            btn2X + equipW / 2.0,
            btnY + 1.0,
            Align::CenterX,
            14.0,
            Font::Var,
            "^xBBBBBBMissing slot",
        );
        // L487
        b2Hover = false;
    } else {
        // "Equip" button
        // L490-491
        b2Hover = cursorX >= btn2X
            && cursorX < btn2X + equipW
            && cursorY >= btnY
            && cursorY < btnY + btnH;
        // L492
        drawBtn(cx, btn2X, equipW, b2Hover, "^7Equip");
    }

    // L495
    CopyButtons {
        b1Hover,
        b2Hover,
        b3Hover,
        b2X: btn2X,
        b2Y: btnY,
        b2W: equipW,
        b2H: btnH,
    }
}

/// lua: TradeHelpers.lua:499-514 `local function fitItemName(colorCode, name, maxW)`
/// (CCR-0143 §1)
fn fitItemName(cx: &Cx, colorCode: &str, name: &str, maxW: f64) -> String {
    // L500
    let display = format!("{colorCode}{name}");
    // L501
    if cx.sg.draw_string_width(16.0, Font::Var, &display) <= maxW {
        // L502
        return display;
    }
    // L504
    let (mut lo, mut hi) = (0.0, name.len() as f64);
    // L505
    while lo < hi {
        // L506
        let mid = m_floor((lo + hi + 1.0) / 2.0);
        // L507
        let s = format!("{colorCode}{}...", sub(name, 1.0, Some(mid)));
        if cx.sg.draw_string_width(16.0, Font::Var, &s) <= maxW {
            // L508
            lo = mid;
        } else {
            // L510
            hi = mid - 1.0;
        }
    }
    // L513
    format!("{colorCode}{}...", sub(name, 1.0, Some(lo)))
}

/// Which build's ItemsTab `hoverItemsTab` is (`primaryItemsTab` / `compareItemsTab`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum ItemsTabSide {
    Primary,
    Compare,
}

/// `drawCompactSlotRow`'s results.
struct CompactRow {
    buttons: CopyButtons,
    hoverItem: Option<ItemRef>,
    hoverItemsTab: Option<ItemsTabSide>,
    hoverBoxX: f64,
    hoverBoxY: f64,
    hoverBoxW: f64,
    hoverBoxH: f64,
}

/// lua: TradeHelpers.lua:523-610 `M.drawCompactSlotRow(drawY, slotLabel, pItem, cItem,
/// colWidth, cursorX, cursorY, maxLabelW, primaryItemsTab, compareItemsTab, pWarn, cWarn,
/// slotMissing, copyBtnW, copyBtnH, buyBtnW, equipBtnW, xOffset, shouldUnderlineLabel)`
/// (CCR-0143 §1; the two ItemsTabs are [`ItemsTabSide`]s; `pHover`/`cHover` are not read by
/// the caller)
#[allow(clippy::too_many_arguments)]
fn drawCompactSlotRow(
    cx: &mut Cx,
    drawY: f64,
    slotLabel: &str,
    pItem: Option<&ItemRef>,
    cItem: Option<&ItemRef>,
    colWidth: f64,
    cursorX: f64,
    cursorY: f64,
    maxLabelW: f64,
    pWarn: Option<&str>,
    cWarn: Option<&str>,
    slotMissing: bool,
    copyBtnW: f64,
    copyBtnH: f64,
    buyBtnW: f64,
    equipBtnW: f64,
    xOffset: f64,
    shouldUnderlineLabel: bool,
) -> CompactRow {
    // L527 (`xOffset or 0`: the caller always passes a number)
    // L528-529
    let mut pName = pItem.map_or_else(|| "(empty)".to_owned(), |i| i.name.get().to_string());
    let mut cName = cItem.map_or_else(|| "(empty)".to_owned(), |i| i.name.get().to_string());
    // L530
    if let Some(w) = pWarn.filter(|w| !w.is_empty()) {
        pName.push_str(w);
    }
    // L531
    if let Some(w) = cWarn.filter(|w| !w.is_empty()) {
        cName.push_str(w);
    }
    // L532-534
    let pColor = getRarityColor(cx, pItem);
    let cColor = getRarityColor(cx, cItem);
    let diffLabel = getSlotDiffLabel(cx, pItem, cItem);

    // Layout positions (fixed 310px box width matching regular Items tab)
    // L537-539
    let labelX = 10.0 + xOffset;
    let pBoxX = labelX + maxLabelW + 4.0;
    let pBoxW = ITEM_BOX_W;

    // L541-542
    let cBoxX = colWidth + 10.0 + xOffset;
    let cBoxW = ITEM_BOX_W;

    // Diff indicator position
    // L545
    let diffX = pBoxX + pBoxW + 6.0;

    // Hover detection
    // L548-551
    let pHover = pItem.is_some()
        && cursorX >= pBoxX
        && cursorX < pBoxX + pBoxW
        && cursorY >= drawY
        && cursorY < drawY + ITEM_BOX_H;
    let cHover = cItem.is_some()
        && cursorX >= cBoxX
        && cursorX < cBoxX + cBoxW
        && cursorY >= drawY
        && cursorY < drawY + ITEM_BOX_H;

    // Draw slot label
    // L554
    cx.sg.set_draw_color(1.0, 1.0, 1.0);
    // L555
    let labelText = format!("^7{slotLabel}:");
    // L556
    cx.sg.draw_string(
        labelX,
        drawY + 2.0,
        Align::Left,
        16.0,
        Font::Var,
        &labelText,
    );

    // L558
    if shouldUnderlineLabel {
        // L559
        let labelW = cx.sg.draw_string_width(16.0, Font::Var, &labelText);
        // L560
        cx.sg
            .draw_image(None, labelX, drawY + 2.0 + 16.0, labelW, 1.0);
    }

    // Draw primary item box
    // L564
    let pBorderGray = if pHover { 0.5 } else { 0.33 };
    // L565-570
    cx.sg.set_draw_color(pBorderGray, pBorderGray, pBorderGray);
    cx.sg.draw_image(None, pBoxX, drawY, pBoxW, ITEM_BOX_H);
    cx.sg.set_draw_color(0.05, 0.05, 0.05);
    cx.sg.draw_image(
        None,
        pBoxX + 1.0,
        drawY + 1.0,
        pBoxW - 2.0,
        ITEM_BOX_H - 2.0,
    );
    cx.sg.set_draw_color(1.0, 1.0, 1.0);
    let t = fitItemName(cx, &pColor, &pName, pBoxW - 8.0);
    cx.sg
        .draw_string(pBoxX + 4.0, drawY + 2.0, Align::Left, 16.0, Font::Var, &t);

    // Draw diff indicator (between the two item boxes)
    // L573
    cx.sg
        .draw_string(diffX, drawY + 3.0, Align::Left, 14.0, Font::Var, &diffLabel);

    // Draw compare item box
    // L576
    let cBorderGray = if cHover { 0.5 } else { 0.33 };
    // L577-582
    cx.sg.set_draw_color(cBorderGray, cBorderGray, cBorderGray);
    cx.sg.draw_image(None, cBoxX, drawY, cBoxW, ITEM_BOX_H);
    cx.sg.set_draw_color(0.05, 0.05, 0.05);
    cx.sg.draw_image(
        None,
        cBoxX + 1.0,
        drawY + 1.0,
        cBoxW - 2.0,
        ITEM_BOX_H - 2.0,
    );
    cx.sg.set_draw_color(1.0, 1.0, 1.0);
    let t = fitItemName(cx, &cColor, &cName, cBoxW - 8.0);
    cx.sg
        .draw_string(cBoxX + 4.0, drawY + 2.0, Align::Left, 16.0, Font::Var, &t);

    // Draw buttons
    // L585
    let mut buttons = CopyButtons::default();
    // L586
    if cItem.is_some() {
        // L587
        let btnStartX = cBoxX + cBoxW + 6.0;
        // L588-589
        buttons = drawCopyButtons(
            cx,
            cursorX,
            cursorY,
            btnStartX,
            drawY + 1.0,
            slotMissing,
            copyBtnW,
            copyBtnH,
            buyBtnW,
            equipBtnW,
        );
    }

    // Determine hovered item and tooltip anchor position
    // L593-595
    let mut hoverItem = None;
    let mut hoverItemsTab = None;
    let (mut hoverBoxX, mut hoverBoxY, mut hoverBoxW, mut hoverBoxH) = (0.0, 0.0, 0.0, 0.0);
    // L596
    if pHover {
        // L597-599
        hoverItem = pItem.cloned();
        hoverItemsTab = Some(ItemsTabSide::Primary);
        (hoverBoxX, hoverBoxY, hoverBoxW, hoverBoxH) = (pBoxX, drawY, pBoxW, ITEM_BOX_H);
    } else if cHover {
        // L601-603
        hoverItem = cItem.cloned();
        hoverItemsTab = Some(ItemsTabSide::Compare);
        (hoverBoxX, hoverBoxY, hoverBoxW, hoverBoxH) = (cBoxX, drawY, cBoxW, ITEM_BOX_H);
    }

    // L606-607
    CompactRow {
        buttons,
        hoverItem,
        hoverItemsTab,
        hoverBoxX,
        hoverBoxY,
        hoverBoxW,
        hoverBoxH,
    }
}

// ---- DrawItemExpanded's `emit` ------------------------------------------------------------------

/// `local function emit(lx, ly, align, fs, fStyle, str)` (L3500-3507) with its upvalues
/// `measureMode` and `maxLineW`.
struct Emit {
    measureMode: bool,
    maxLineW: f64,
}

impl Emit {
    fn emit(&mut self, cx: &mut Cx, lx: f64, ly: f64, align: Align, fs: f64, str: &str) {
        // L3501
        if self.measureMode {
            // L3502
            let w = cx.sg.draw_string_width(fs, Font::Var, str);
            // L3503
            if w > self.maxLineW {
                self.maxLineW = w;
            }
        } else {
            // L3505
            cx.sg.draw_string(lx, ly, align, fs, Font::Var, str);
        }
    }
}

// ---- DrawItems' local helpers -----------------------------------------------------------------

/// The locals of DrawItems that its two local functions read and write (L3797-3815).
struct ItemsFrame {
    drawY: f64,
    colWidth: f64,
    contentWidth: f64,
    scrollOffsetX: f64,
    checkboxOffset: f64,
    scrollViewH: f64,
    cursorX: f64,
    cursorY: f64,
    vp: ViewPort,
    // L3800-3803
    hoverItem: Option<ItemRef>,
    hoverX: f64,
    hoverY: f64,
    hoverW: f64,
    hoverH: f64,
    hoverItemsTab: Option<ItemsTabSide>,
    // L3806-3809
    clickedCopySlot: Option<String>,
    clickedEquipSlot: Option<String>,
    clickedBuySlot: Option<String>,
    clickedBuyItem: Option<ItemRef>,
    // L3812-3815
    hoverEquipItem: Option<ItemRef>,
    hoverEquipSlotName: Option<String>,
    hoverEquipBtnX: f64,
    hoverEquipBtnY: f64,
    hoverEquipBtnW: f64,
    hoverEquipBtnH: f64,
}

impl ItemsFrame {
    /// lua: CompareTab.lua:3819-3843 `local function processSlotButtons(b1Hover, b2Hover,
    /// b3Hover, b2X, b2Y, b2W, b2H, cItem, copySlotName, equipSlotName)`
    fn processSlotButtons(
        &mut self,
        inputEvents: &mut InputEvents,
        b: CopyButtons,
        cItem: Option<&ItemRef>,
        copySlotName: &str,
        equipSlotName: &str,
    ) {
        // L3820
        if b.b2Hover && cItem.is_some() {
            // L3821-3824
            self.hoverEquipItem = cItem.cloned();
            self.hoverEquipSlotName = Some(equipSlotName.to_owned());
            (self.hoverEquipBtnX, self.hoverEquipBtnY) = (b.b2X, b.b2Y);
            (self.hoverEquipBtnW, self.hoverEquipBtnH) = (b.b2W, b.b2H);
        }
        // L3826 (`inputEvents` is always a table)
        if let Some(cItem) = cItem {
            // L3827
            let mut id = 0;
            while let Some(event) = inputEvents.next_ipairs(&mut id) {
                // L3828
                if event.kind == EventKind::KeyUp && event.key == Key::LeftButton {
                    // L3829
                    if b.b1Hover {
                        // L3830-3831
                        self.clickedCopySlot = Some(copySlotName.to_owned());
                        inputEvents.consume(id);
                    } else if b.b2Hover {
                        // L3833-3834
                        self.clickedEquipSlot = Some(equipSlotName.to_owned());
                        inputEvents.consume(id);
                    } else if b.b3Hover {
                        // L3836-3838
                        self.clickedBuySlot = Some(equipSlotName.to_owned());
                        self.clickedBuyItem = Some(cItem.clone());
                        inputEvents.consume(id);
                    }
                }
            }
        }
    }
}

/// lua: CompareTab.lua:3847-3913 `local function drawSlotEntry(label, pItem, cItem,
/// copySlotName, equipSlotName, labelW, pWarn, cWarn, slotMissing)`
#[allow(clippy::too_many_arguments)]
fn drawSlotEntry(
    cx: &mut Cx,
    me: CtrlId,
    f: &mut ItemsFrame,
    inputEvents: &mut InputEvents,
    label: &str,
    pItem: Option<&ItemRef>,
    cItem: Option<&ItemRef>,
    copySlotName: &str,
    equipSlotName: &str,
    labelW: f64,
    pWarn: Option<&str>,
    cWarn: Option<&str>,
    slotMissing: bool,
) {
    // L3848
    if st(cx, me).itemsExpandedMode {
        // === EXPANDED MODE ===
        // L3850
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L3851
        let text = format!("^7{label}:{}", pWarn.unwrap_or(""));
        cx.sg.draw_string(
            f.scrollOffsetX + 10.0,
            f.drawY,
            Align::Left,
            16.0,
            Font::Var,
            &text,
        );
        // L3852
        let labelEndW = cx.sg.draw_string_width(16.0, Font::Var, &text);
        // L3853
        let diff = getSlotDiffLabel(cx, pItem, cItem);
        cx.sg.draw_string(
            f.scrollOffsetX + 10.0 + labelEndW + 8.0,
            f.drawY + 2.0,
            Align::Left,
            14.0,
            Font::Var,
            &diff,
        );

        // L3855
        if cItem.is_some() {
            // L3856
            let b = drawCopyButtons(
                cx,
                f.cursorX,
                f.cursorY,
                f.scrollOffsetX + f.contentWidth - 214.0,
                f.drawY + 21.0,
                slotMissing,
                LAYOUT::itemsCopyBtnW,
                LAYOUT::itemsCopyBtnH,
                LAYOUT::itemsBuyBtnW,
                LAYOUT::itemsEquipBtnW,
            );
            // L3857
            f.processSlotButtons(inputEvents, b, cItem, copySlotName, equipSlotName);
        }

        // L3860
        f.drawY += 20.0;

        // L3862-3863
        let pModMap = buildModMap(pItem);
        let cModMap = buildModMap(cItem);
        // L3864
        let itemStartY = f.drawY;
        // L3865
        let leftHeight = CompareTab::DrawItemExpanded(
            cx,
            pItem,
            f.scrollOffsetX + 20.0,
            f.drawY,
            f.colWidth - 30.0,
            Some(&cModMap),
            false,
        );
        // L3866
        let rightHeight = CompareTab::DrawItemExpanded(
            cx,
            cItem,
            f.scrollOffsetX + f.colWidth + 20.0,
            f.drawY,
            f.colWidth - 30.0,
            Some(&pModMap),
            false,
        );

        // L3868
        cx.sg.set_draw_color(0.25, 0.25, 0.25);
        // L3869
        let maxH = m_max(leftHeight, rightHeight);
        // L3870
        cx.sg
            .draw_image(None, f.scrollOffsetX + f.colWidth, itemStartY, 1.0, maxH);

        // L3872
        f.drawY = f.drawY + maxH + 6.0;
    } else {
        // === COMPACT MODE ===
        // L3875
        let p = cx.build().model.clone();
        let slot = slot_at(&p, equipSlotName);
        // L3876
        let nodeId = slot.as_ref().map_or(Val::Nil, |s| s.get(n!("nodeId")));
        // L3877
        let shouldUnderline = nodeId.truthy();
        // L3878-3884
        let row = drawCompactSlotRow(
            cx,
            f.drawY,
            label,
            pItem,
            cItem,
            f.colWidth,
            f.cursorX,
            f.cursorY,
            labelW,
            pWarn,
            cWarn,
            slotMissing,
            LAYOUT::itemsCopyBtnW,
            LAYOUT::itemsCopyBtnH,
            LAYOUT::itemsBuyBtnW,
            LAYOUT::itemsEquipBtnW,
            f.scrollOffsetX,
            shouldUnderline,
        );

        // L3886 (`scrollOffsetX or 0`: always a number)
        let labelX = f.scrollOffsetX + 10.0;
        // draw passive tree view when hovering over the label, if the slot is a jewel socket
        // L3888-3889
        if labelX <= f.cursorX
            && f.cursorX <= (labelX + labelW)
            && f.drawY < f.cursorY
            && f.cursorY <= (f.drawY + 20.0)
        {
            // L3890
            if nodeId.truthy() {
                // L3891
                let boxSize = 250.0;
                // anchor bottom left to label top left, keeping in mind what our viewport was
                // L3893
                cx.sg.reset_viewport();
                // L3894-3895
                let boxX = f.vp.x + labelX;
                let boxY = (f.vp.y + f.checkboxOffset) + f.drawY - boxSize;
                // L3896-3897
                let id = nodeId.num();
                crate::tabs::items::slot_helper::DrawViewer(cx, id, boxX, boxY, boxSize, boxSize);
                // restore viewport
                // L3899
                cx.sg
                    .set_viewport(f.vp.x, f.vp.y + f.checkboxOffset, f.vp.width, f.scrollViewH);
            }
        }
        // L3902
        if let Some(item) = row.hoverItem {
            // L3903-3906
            f.hoverItem = Some(item);
            f.hoverItemsTab = row.hoverItemsTab;
            (f.hoverX, f.hoverY) = (row.hoverBoxX, row.hoverBoxY);
            (f.hoverW, f.hoverH) = (row.hoverBoxW, row.hoverBoxH);
        }

        // L3909
        f.processSlotButtons(inputEvents, row.buttons, cItem, copySlotName, equipSlotName);

        // L3911
        f.drawY += 20.0;
    }
}

// ---- the passes -------------------------------------------------------------------------------

impl CompareTab {
    /// lua: CompareTab.lua:3162-3306 `CompareTabClass:DrawSummary(vp, compareEntry)`
    /// @pob Classes/CompareTab.lua:3162-3306 h=adc6805d
    #[allow(clippy::too_many_lines)]
    pub fn DrawSummary(cx: &mut Cx, me: CtrlId, vp: &ViewPort, compareEntry: &CompareEntryRef) {
        let p = cx.build().model.clone();
        // L3163-3166 (`self.primaryBuild.calcsTab` / `compareEntry.calcsTab` always exist)
        let primaryEnvMain = p.calcsTab.mainEnv.get();
        let compareEnvMain = compareEntry.model.calcsTab.mainEnv.get();

        // If each selected builds skill is a minion skill, use it
        // L3169-3172
        let minion_of = |env: &Option<model::EnvRef>| -> Option<ActorRef> {
            let env = env.as_ref()?;
            env.player.mainSkill.get().and_then(|s| s.minion.get())?;
            env.minion.get()
        };
        let primaryMinionSkill = minion_of(&primaryEnvMain);
        let compareMinionSkill = minion_of(&compareEnvMain);
        // L3173
        let summaryUseMinion = primaryMinionSkill.is_some() || compareMinionSkill.is_some();

        // L3175
        let primaryOutput: Option<Output> = match &primaryMinionSkill {
            Some(m) => Some(m.output.get()),
            None => p.calcsTab.mainOutput.get(),
        };
        // L3176
        let compareOutput: Option<Output> = match &compareMinionSkill {
            Some(m) => Some(m.output.get()),
            None => CompareEntry::GetOutput(compareEntry),
        };
        // L3177
        let (Some(primaryOutput), Some(compareOutput)) = (primaryOutput, compareOutput) else {
            // L3178
            return;
        };

        // L3181-3184
        let lineHeight = 18.0;
        let _ = lineHeight;
        let headerHeight = 22.0;
        let headerReserve = LAYOUT::summaryHeaderHeight;
        let scrollViewH = vp.height - headerReserve;

        // Column positions (col3R and col4 shift right dynamically to avoid name overlap)
        // L3187-3188
        let col1 = LAYOUT::summaryCol1;
        let col2R = LAYOUT::summaryCol2Right;

        // L3190-3193
        let primaryName = Self::GetShortBuildName(cx.build().build_name.as_deref());
        let compareName = compareEntry.label.clone();
        let primaryNameW = cx
            .sg
            .draw_string_width(headerHeight, Font::Var, &primaryName);
        let _ = primaryNameW;
        let compareNameW = cx
            .sg
            .draw_string_width(headerHeight, Font::Var, &compareName);

        // L3195-3198
        let minCol3R = col2R + compareNameW + 16.0;
        let maxCol3R = vp.width - 200.0;
        let col3R = m_min(m_max(LAYOUT::summaryCol3Right, minCol3R), maxCol3R);
        let col4 = col3R + 20.0;

        // L3200
        cx.sg.set_viewport(vp.x, vp.y, vp.width, headerReserve);
        // L3201
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L3202
        cx.sg
            .draw_string(col1, 4.0, Align::Left, headerHeight, Font::Var, "^7Stat");
        // L3203
        let t = format!("{}{primaryName}", cx.main.cc.POSITIVE);
        cx.sg
            .draw_string(col2R, 4.0, Align::RightX, headerHeight, Font::Var, &t);
        // L3204-3205
        let t = format!("{}{compareName}", cx.main.cc.WARNING);
        cx.sg
            .draw_string(col3R, 4.0, Align::RightX, headerHeight, Font::Var, &t);
        // L3206
        cx.sg.draw_string(
            col4,
            4.0,
            Align::Left,
            headerHeight,
            Font::Var,
            "^7Difference",
        );
        // L3207
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        // L3208
        cx.sg
            .draw_image(None, 4.0, headerHeight + 8.0, vp.width - 8.0, 2.0);

        // L3210
        cx.sg
            .set_viewport(vp.x, vp.y + headerReserve, vp.width, scrollViewH);
        // L3211
        let mut drawY = -st(cx, me).scrollY;

        // Stat comparison
        // L3214
        let displayStats = if summaryUseMinion {
            display_stats::minionDisplayStats()
        } else {
            display_stats::displayStats()
        };
        // L3215-3216 (`primaryEnvMain.player` indexes a nil env when the build has no mainEnv)
        let player = |env: &Option<model::EnvRef>, what: &str| -> ActorRef {
            env.as_ref()
                .map(|e| e.player.clone())
                .unwrap_or_else(|| lua_error!("attempt to index local '{what}' (a nil value)"))
        };
        let primaryActor = primaryMinionSkill
            .clone()
            .unwrap_or_else(|| player(&primaryEnvMain, "primaryEnvMain"));
        let compareActor = compareMinionSkill
            .clone()
            .unwrap_or_else(|| player(&compareEnvMain, "compareEnvMain"));

        // L3218
        drawY = Self::DrawStatList(
            cx,
            drawY,
            &displayStats,
            &primaryOutput,
            &compareOutput,
            Some(&primaryActor),
            Some(&compareActor),
            col1,
            col4,
            col2R,
            col3R,
        );

        // ========================================
        // Compare Power Report section
        // ========================================
        // L3223
        drawY += 16.0;

        // Separator
        // L3226
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        // L3227
        cx.sg.draw_image(None, 4.0, drawY, vp.width - 8.0, 2.0);
        // L3228
        drawY += 8.0;

        // Header
        // L3231
        cx.sg.set_draw_color(1.0, 1.0, 1.0);
        // L3232
        cx.sg.draw_string(
            LAYOUT::powerReportLeft,
            drawY,
            Align::Left,
            20.0,
            Font::Var,
            "^7Compare Power Report",
        );
        // L3233
        drawY += 24.0;

        // Run the coroutine driver (advances calculation each frame)
        // L3236
        Self::RunComparePowerReport(cx, me, compareEntry);

        // Position controls dynamically based on drawY
        // The controls need absolute screen positions (vp.x/vp.y offset + viewport-local drawY)
        // L3240-3241
        let controlY = vp.y + headerReserve + drawY;
        let ctrlBaseX = vp.x + LAYOUT::powerReportLeft;

        // Metric dropdown
        // L3244-3245
        let statSelect = ctl(cx, me, "comparePowerStatSelect");
        set_xy(cx, statSelect, ctrlBaseX + 60.0, controlY);

        // Label for dropdown
        // L3248
        cx.sg.draw_string(
            LAYOUT::powerReportLeft,
            drawY,
            Align::Left,
            16.0,
            Font::Var,
            "^7Metric:",
        );

        // Category checkboxes (positioned to the right of dropdown)
        // L3251
        let mut checkX = ctrlBaseX + 280.0;
        // L3252-3254
        let c = ctl(cx, me, "comparePowerTreeCheck");
        let lw = label_width(cx, c);
        set_xy(cx, c, checkX + lw, controlY);
        checkX = checkX + label_width(cx, c) + 26.0;

        // L3256-3258
        let c = ctl(cx, me, "comparePowerItemsCheck");
        let lw = label_width(cx, c);
        set_xy(cx, c, checkX + lw, controlY);
        checkX = checkX + label_width(cx, c) + 26.0;

        // L3260-3262
        let c = ctl(cx, me, "comparePowerGemsCheck");
        let lw = label_width(cx, c);
        set_xy(cx, c, checkX + lw, controlY);
        checkX = checkX + label_width(cx, c) + 26.0;

        // L3264-3266
        let c = ctl(cx, me, "comparePowerSupportGemsCheck");
        let lw = label_width(cx, c);
        set_xy(cx, c, checkX + lw, controlY);
        checkX = checkX + label_width(cx, c) + 26.0;

        // L3268-3269
        let c = ctl(cx, me, "comparePowerConfigCheck");
        let lw = label_width(cx, c);
        set_xy(cx, c, checkX + lw, controlY);

        // L3271
        drawY += 28.0;

        // Update the list control with current data (only when changed)
        // L3274
        let listControl = ctl(cx, me, "comparePowerReportList");
        // L3275
        if st(cx, me).comparePowerCoroutine.is_some() {
            // L3276
            let progress = st(cx, me).comparePowerProgress;
            ComparePowerReportListControl::SetProgress(cx, listControl, progress);
            // L3277
            st_mut(cx, me).comparePowerListSynced = false;
        } else if st(cx, me).comparePowerResults.is_some() && !st(cx, me).comparePowerListSynced {
            // L3279
            let (stat, results) = (st(cx, me).comparePowerStat, power::results(cx, me));
            ComparePowerReportListControl::SetReport(cx, listControl, stat, results);
            // L3280
            st_mut(cx, me).comparePowerListSynced = true;
        } else if st(cx, me).comparePowerStat.is_none() && !st(cx, me).comparePowerListSynced {
            // L3282
            ComparePowerReportListControl::SetReport(cx, listControl, None, None);
            // L3283
            st_mut(cx, me).comparePowerListSynced = true;
        }

        // Update the impact column label to match the selected stat
        // L3287
        if let Some(stat) = st(cx, me).comparePowerStat {
            // L3288 (`label or ""`: every powerStatList entry has a label)
            ComparePowerReportListControl::set_impact_label(cx, listControl, &stat.label);
        }

        // Position the list control (absolute screen coordinates).
        // The list has a fixed height and its own internal scrollbar for rows.
        // Width matches the table columns (720) plus scrollbar (20px border/scroll area).
        // L3294-3295
        let listHeight = 250.0;
        let listWidth = 740.0;
        // L3296-3299
        set_xy(
            cx,
            listControl,
            vp.x + LAYOUT::powerReportLeft,
            vp.y + headerReserve + drawY,
        );
        let b = cx.base_mut(listControl);
        b.width = Prop::Val(listWidth);
        b.height = Prop::Val(listHeight);

        // bottom padding
        // L3301
        drawY = drawY + listHeight + 20.0;

        // L3303
        let s = st_mut(cx, me);
        s.summaryTotalContentHeight = drawY + s.scrollY + 36.0;

        // L3305
        cx.sg.reset_viewport();
    }

    /// lua: CompareTab.lua:3309-3398 `CompareTabClass:DrawStatList(drawY, displayStats,
    /// primaryOutput, compareOutput, primaryActor, compareActor, col1, col4, col2R, col3R)`
    /// @pob Classes/CompareTab.lua:3309-3398 h=cadc2873
    #[allow(clippy::too_many_arguments, clippy::too_many_lines)]
    pub fn DrawStatList(
        cx: &mut Cx,
        mut drawY: f64,
        displayStats: &[DisplayStat],
        primaryOutput: &Output,
        compareOutput: &Output,
        primaryActor: Option<&ActorRef>,
        compareActor: Option<&ActorRef>,
        col1: f64,
        col4: f64,
        col2R: f64,
        col3R: f64,
    ) -> f64 {
        // L3310
        let lineHeight = 16.0;

        // Get skill flags from each build's selected actor (player, or minion when the
        // top-section "Skill:" is a minion skill) for stat filtering
        // L3314-3315
        let flags_of = |a: Option<&ActorRef>| {
            a.and_then(|a| a.mainSkill.get())
                .map(|s| s.skillFlags.get())
        };
        let primaryFlags = flags_of(primaryActor);
        let compareFlags = flags_of(compareActor);
        let has = |flags: &Option<Table>, f: &str| {
            flags.as_ref().is_some_and(|t| t.truthy(Name::from(f)))
        };
        let pVal = Val::Table(primaryOutput.clone());
        let cVal = Val::Table(compareOutput.clone());

        // L3317
        for statData in displayStats {
            let flag = statData.flag.map(FlagSpec::Str);
            let notFlag = statData.notFlag.map(FlagSpec::Str);
            // L3318
            if statData.stat.is_none() && statData.label.is_none() {
                // Empty entry = section spacer (matches sidebar behavior)
                // L3320
                drawY += 6.0;
            } else if statData.stat == Some("SkillDPS") {
                // Skip: multi-row SkillDPS doesn't fit compare layout
            } else if statData.hideStat {
                // Skip: hidden stats
            } else if !matchFlags(flag, notFlag, &|f| has(&primaryFlags, f))
                && !matchFlags(flag, notFlag, &|f| has(&compareFlags, f))
            {
                // Skip: stat not relevant to either build's active skill
            } else if let Some(stat) = statData.stat {
                // Normal stat with value
                // L3330-3331
                let mut primaryVal = or0(primaryOutput.get(Name::from(stat)));
                let mut compareVal = or0(compareOutput.get(Name::from(stat)));

                // Handle childStat (e.g. MainHand.Accuracy)
                // L3334
                if let Some(child) = statData.childStat {
                    // L3335-3336
                    primaryVal = if primaryVal.lua_type() == "table" {
                        or0(index(&primaryVal, child))
                    } else {
                        Val::Num(0.0)
                    };
                    compareVal = if compareVal.lua_type() == "table" {
                        or0(index(&compareVal, child))
                    } else {
                        Val::Num(0.0)
                    };
                }

                // Skip table-type stat values
                // L3340
                if primaryVal.lua_type() == "table" || compareVal.lua_type() == "table" {
                    // L3341-3342
                    primaryVal = Val::Num(0.0);
                    compareVal = Val::Num(0.0);
                }

                // Skip zero-value stats, check condFunc
                // L3346-3347
                let zero = Val::Num(0.0);
                if (!primaryVal.raw_eq(&zero) || !compareVal.raw_eq(&zero))
                    && statData.condFunc.map_or(true, |f| {
                        f(&primaryVal, &pVal).truthy() || f(&compareVal, &cVal).truthy()
                    })
                {
                    // Format values
                    // L3349
                    let fmt = statData.fmt.unwrap_or("d");
                    // L3350
                    let multiplier = if statData.pc || statData.r#mod {
                        100.0
                    } else {
                        1.0
                    };
                    // L3351-3352
                    let f = format!("%{fmt}");
                    let mut primaryStr = lfmt!(&f, primaryVal.num() * multiplier);
                    let mut compareStr = lfmt!(&f, compareVal.num() * multiplier);
                    // L3353-3354
                    primaryStr = format_num_sep_main(&cx.main, &primaryStr);
                    compareStr = format_num_sep_main(&cx.main, &compareStr);

                    // Determine diff color and string
                    // L3357-3359
                    let diff = compareVal.num() - primaryVal.num();
                    let mut diffStr = String::new();
                    let mut diffColor = "^7".to_owned();
                    // L3360
                    if diff > 0.001 || diff < -0.001 {
                        // L3361
                        let isBetter = (statData.lowerIsBetter && diff < 0.0)
                            || (!statData.lowerIsBetter && diff > 0.0);
                        // L3362
                        diffColor = if isBetter {
                            cx.main.cc.POSITIVE.clone()
                        } else {
                            cx.main.cc.NEGATIVE.clone()
                        };
                        // L3363
                        let diffVal = diff * multiplier;
                        // L3364
                        diffStr = lfmt!(&format!("%+{fmt}"), diffVal);
                        // L3365
                        diffStr = format_num_sep_main(&cx.main, &diffStr);
                        // Add percentage if primary value is non-zero
                        // L3367
                        if !primaryVal.raw_eq(&zero) {
                            // L3368
                            let pc = compareVal.num() / primaryVal.num() * 100.0 - 100.0;
                            // L3369
                            diffStr.push_str(&lfmt!(" (%+.1f%%)", pc));
                        }
                    }

                    // Draw stat row
                    // L3374
                    let labelColor = statData
                        .color(&cx.main.cc)
                        .unwrap_or_else(|| "^7".to_owned());
                    // L3375
                    let t = format!("{labelColor}{}", statData.label.unwrap_or(stat));
                    cx.sg
                        .draw_string(col1, drawY, Align::Left, lineHeight, Font::Var, &t);
                    // L3376
                    let t = format!("^7{primaryStr}");
                    cx.sg
                        .draw_string(col2R, drawY, Align::RightX, lineHeight, Font::Var, &t);
                    // L3377
                    let t = format!("{}{compareStr}", cx.main.cc.SPLITPERSONALITY);
                    cx.sg
                        .draw_string(col3R, drawY, Align::RightX, lineHeight, Font::Var, &t);
                    // L3378
                    if !diffStr.is_empty() {
                        // L3379
                        let t = format!("{diffColor}{diffStr}");
                        cx.sg
                            .draw_string(col4, drawY, Align::Left, lineHeight, Font::Var, &t);
                    }
                    // L3381
                    drawY = drawY + lineHeight + 1.0;
                }
            } else if let (Some(label), Some(condFunc)) = (statData.label, statData.condFunc) {
                // Label-only stat (e.g. "Chaos Resistance: Immune")
                // L3385
                let labelColor = statData
                    .color(&cx.main.cc)
                    .unwrap_or_else(|| "^7".to_owned());
                // L3386
                if condFunc(&pVal, &Val::Nil).truthy() || condFunc(&cVal, &Val::Nil).truthy() {
                    // L3387
                    let valStr = statData.val.unwrap_or("");
                    // L3388-3389
                    let primaryShown = condFunc(&pVal, &Val::Nil).truthy();
                    let compareShown = condFunc(&cVal, &Val::Nil).truthy();
                    // L3390
                    let t = format!("{labelColor}{label}");
                    cx.sg
                        .draw_string(col1, drawY, Align::Left, lineHeight, Font::Var, &t);
                    // L3391
                    let t = format!("^7{}", if primaryShown { valStr } else { "-" });
                    cx.sg
                        .draw_string(col2R, drawY, Align::RightX, lineHeight, Font::Var, &t);
                    // L3392
                    let t = format!(
                        "{}{}",
                        cx.main.cc.WARNING,
                        if compareShown { valStr } else { "-" }
                    );
                    cx.sg
                        .draw_string(col3R, drawY, Align::RightX, lineHeight, Font::Var, &t);
                    // L3393
                    drawY = drawY + lineHeight + 1.0;
                }
            }
        }
        // L3397
        drawY
    }

    /// lua: CompareTab.lua:3403-3486 `CompareTabClass:DrawTree(vp, inputEvents, compareEntry)`
    /// @pob Classes/CompareTab.lua:3403-3486 h=a713689b
    #[allow(clippy::too_many_lines)]
    pub fn DrawTree(
        cx: &mut Cx,
        me: CtrlId,
        vp: &ViewPort,
        inputEvents: &mut InputEvents,
        compareEntry: &CompareEntryRef,
    ) {
        // L3404
        let Some(layout) = cx.w::<CompareTab>(me).layout.treeLayout else {
            return;
        };

        // L3407-3409 (`origGetCursorPos` is `cx.sys.cursor_pos`; module notes)
        let headerHeight = layout.headerHeight;
        let footerHeight = layout.footerHeight;
        // the primary build's TreeTab and its viewer (`treeTab and treeTab.viewer`)
        let primaryTreeTab =
            crate::tabs::tree::core::tab(cx).filter(|id| cx.try_w::<TreeTab>(*id).is_some());

        // L3411
        if layout.overlay {
            // ========== OVERLAY MODE ==========
            // Uses the primary build's viewer with compareSpec set to the compare entry's spec.
            // PassiveTreeView automatically renders green (added), red (removed), blue (mastery differs).
            // L3415-3417
            let treeAbsX = vp.x;
            let treeAbsY = vp.y + headerHeight;
            let treeHeight = vp.height - headerHeight - footerHeight;

            // L3419
            if let Some(tt) = primaryTreeTab {
                // Set compareSpec to enable overlay coloring
                // L3421
                let spec = compareEntry.model.spec.get();
                cx.w_mut::<TreeTab>(tt).core.viewer.compare_spec = Some(spec);

                // L3423
                cx.sg.set_viewport(treeAbsX, treeAbsY, vp.width, treeHeight);
                // L3424
                cx.sg.set_draw_layer(None, Some(0));
                // L3425-3428 (GetCursorPos patched until L3437)
                with_cursor_origin(cx, treeAbsX, treeAbsY, |cx| {
                    // L3429
                    let treeVP = ViewPort {
                        x: 0.0,
                        y: 0.0,
                        width: vp.width,
                        height: treeHeight,
                    };
                    // L3430
                    cx.lend::<TreeTab, _, _>(
                        tt,
                        |w| &mut w.core.viewer,
                        |cx, v| {
                            v.draw(cx, &treeVP, inputEvents);
                        },
                    );
                    // L3431
                    cx.sg.reset_viewport();

                    // Clear compareSpec so it doesn't affect the normal Tree tab
                    // L3434
                    cx.w_mut::<TreeTab>(tt).core.viewer.compare_spec = None;
                });
            }

            // L3437
            // (GetCursorPos = origGetCursorPos: with_cursor_origin restored it)
            // L3438
            return;
        }

        // ========== SIDE-BY-SIDE MODE ==========
        // L3442-3443
        let halfWidth = layout.halfWidth.unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on local 'halfWidth' (a nil value)")
        });
        let treeHeight = vp.height - headerHeight - footerHeight;

        // Divider (from header bottom to viewport bottom)
        // L3446
        cx.sg.set_draw_color(0.5, 0.5, 0.5);
        // L3447
        cx.sg.draw_image(
            None,
            vp.x + halfWidth,
            vp.y + headerHeight,
            4.0,
            vp.height - headerHeight,
        );

        // Route input events to the panel containing the mouse
        // L3450
        let (mouseX, _mouseY) = cx.sys.cursor_pos();
        // L3451
        let leftHasInput = mouseX < (vp.x + halfWidth + 2.0);

        // Left tree: SetViewport clips drawing; patch GetCursorPos so mouse coords
        // are viewport-relative (matching the {x=0,y=0} viewport passed to the tree)
        // L3455-3456
        let leftAbsX = vp.x;
        let leftAbsY = vp.y + headerHeight;
        // L3457
        if let Some(tt) = primaryTreeTab {
            // L3458
            cx.sg
                .set_viewport(leftAbsX, leftAbsY, halfWidth, treeHeight);
            // L3459
            cx.sg.set_draw_layer(None, Some(0));
            // L3460-3463
            with_cursor_origin(cx, leftAbsX, leftAbsY, |cx| {
                // L3464
                let leftTreeVP = ViewPort {
                    x: 0.0,
                    y: 0.0,
                    width: halfWidth,
                    height: treeHeight,
                };
                // L3465
                let mut none = InputEvents::default();
                let ev = if leftHasInput {
                    &mut *inputEvents
                } else {
                    &mut none
                };
                cx.lend::<TreeTab, _, _>(
                    tt,
                    |w| &mut w.core.viewer,
                    |cx, v| {
                        v.draw(cx, &leftTreeVP, ev);
                    },
                );
                // L3466
                cx.sg.reset_viewport();
            });
        }

        // Right tree: same approach - SetViewport for clipping, patched cursor
        // L3470-3471
        let rightAbsX = vp.x + halfWidth + 4.0;
        let rightAbsY = vp.y + headerHeight;
        // L3472 (`compareEntry.treeTab and compareEntry.treeTab.viewer`: the entry always has
        // its viewer, entry.rs)
        {
            // L3473
            cx.sg
                .set_viewport(rightAbsX, rightAbsY, halfWidth, treeHeight);
            // L3474
            cx.sg.set_draw_layer(None, Some(0));
            // L3475-3478
            with_cursor_origin(cx, rightAbsX, rightAbsY, |cx| {
                // L3479
                let rightTreeVP = ViewPort {
                    x: 0.0,
                    y: 0.0,
                    width: halfWidth,
                    height: treeHeight,
                };
                // L3480 (lent out of the entry: nothing the draw calls reads it)
                let mut none = InputEvents::default();
                let ev = if leftHasInput {
                    &mut none
                } else {
                    &mut *inputEvents
                };
                let mut viewer = std::mem::take(&mut *entry::ext(compareEntry).viewer.borrow_mut());
                with_entry_as_build(cx, compareEntry, |cx| {
                    viewer.draw(cx, &rightTreeVP, ev);
                });
                *entry::ext(compareEntry).viewer.borrow_mut() = viewer;
                // L3481
                cx.sg.reset_viewport();
            });
        }

        // Restore original GetCursorPos
        // L3485 (with_cursor_origin restored it)
    }

    /// lua: CompareTab.lua:3495-3666 `CompareTabClass:DrawItemExpanded(item, x, startY,
    /// colWidth, otherModMap, measureMode)`
    /// Draw a single item's full details at (x, startY) within colWidth.
    /// otherModMap: optional table from buildModMap() of the other item for diff highlighting.
    /// Returns the total height consumed (the widest line in measure mode).
    /// @pob Classes/CompareTab.lua:3495-3666 h=ca4f7448
    #[allow(clippy::too_many_lines)]
    fn DrawItemExpanded(
        cx: &mut Cx,
        item: Option<&ItemRef>,
        x: f64,
        startY: f64,
        _colWidth: f64,
        otherModMap: Option<&ModMap>,
        measureMode: bool,
    ) -> f64 {
        // L3496-3499
        let lineHeight = 16.0;
        let fontSize = 14.0;
        let mut drawY = startY;
        let mut e = Emit {
            measureMode,
            maxLineW: 0.0,
        };
        // L3500-3507: `emit` is [`Emit::emit`]
        let magic = cx.main.cc.MAGIC.clone();

        // L3509
        let Some(item) = item else {
            // L3510
            e.emit(cx, x, drawY, Align::Left, fontSize, "^8(empty)");
            // L3511
            if measureMode {
                return e.maxLineW;
            }
            // L3512
            return lineHeight;
        };

        // Item name
        // L3516
        let rarityColor = getRarityColor(cx, Some(item));
        // L3517
        let t = format!("{rarityColor}{}", item.name.get());
        e.emit(cx, x, drawY, Align::Left, 16.0, &t);
        // L3518
        drawY += 18.0;

        // Base type label
        // L3521
        let base = item.base.get();
        // L3522
        if let Some(base) = &base {
            // L3523
            if base.weapon.is_some() {
                // L3524
                let weaponData = item.weaponData.get().and_then(|t| t.get(1).table());
                // L3525
                if let Some(weaponData) = weaponData {
                    // L3526
                    if tk(&weaponData, "PhysicalDPS").truthy() {
                        // L3527
                        let t = lua_fmt(
                            &format!("^x7F7F7FPhys DPS: {magic}%.1f"),
                            &[&tk(&weaponData, "PhysicalDPS")],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3528
                        drawY += lineHeight;
                    }
                    // L3530
                    if tk(&weaponData, "ElementalDPS").truthy() {
                        // L3531
                        let t = lua_fmt(
                            &format!("^x7F7F7FEle DPS: {magic}%.1f"),
                            &[&tk(&weaponData, "ElementalDPS")],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3532
                        drawY += lineHeight;
                    }
                    // L3534
                    if tk(&weaponData, "ChaosDPS").truthy() {
                        // L3535
                        let t = lua_fmt(
                            &format!("^x7F7F7FChaos DPS: {magic}%.1f"),
                            &[&tk(&weaponData, "ChaosDPS")],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3536
                        drawY += lineHeight;
                    }
                    // L3538
                    if tk(&weaponData, "TotalDPS").truthy() {
                        // L3539
                        let t = lua_fmt(
                            &format!("^x7F7F7FTotal DPS: {magic}%.1f"),
                            &[&tk(&weaponData, "TotalDPS")],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3540
                        drawY += lineHeight;
                    }
                    // L3542
                    let t = lua_fmt(
                        &format!("^x7F7F7FCrit: {magic}%.2f%%"),
                        &[&tk(&weaponData, "CritChance")],
                    );
                    e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                    // L3543
                    drawY += lineHeight;
                    // L3544
                    let t = lua_fmt(
                        &format!("^x7F7F7FAPS: {magic}%.2f"),
                        &[&tk(&weaponData, "AttackRate")],
                    );
                    e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                    // L3545
                    drawY += lineHeight;
                }
            } else if base.armour.is_some() {
                // L3548
                let armourData = item.armourData.get();
                // L3549
                if let Some(armourData) = armourData {
                    // L3550
                    let v = tk(&armourData, "Armour");
                    if gt0(&v) {
                        // L3551
                        let t = lua_fmt(&format!("^x7F7F7FArmour: {magic}%d"), &[&v]);
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3552
                        drawY += lineHeight;
                    }
                    // L3554
                    let v = tk(&armourData, "Evasion");
                    if gt0(&v) {
                        // L3555
                        let t = lua_fmt(&format!("^x7F7F7FEvasion: {magic}%d"), &[&v]);
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3556
                        drawY += lineHeight;
                    }
                    // L3558
                    let v = tk(&armourData, "EnergyShield");
                    if gt0(&v) {
                        // L3559
                        let t = lua_fmt(&format!("^x7F7F7FES: {magic}%d"), &[&v]);
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3560
                        drawY += lineHeight;
                    }
                    // L3562
                    let v = tk(&armourData, "Ward");
                    if gt0(&v) {
                        // L3563
                        let t = lua_fmt(&format!("^x7F7F7FWard: {magic}%d"), &[&v]);
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3564
                        drawY += lineHeight;
                    }
                    // L3566
                    let v = tk(&armourData, "BlockChance");
                    if gt0(&v) {
                        // L3567
                        let t = lua_fmt(&format!("^x7F7F7FBlock: {magic}%d%%"), &[&v]);
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3568
                        drawY += lineHeight;
                    }
                }
            } else if base.flask.is_some() {
                // L3572
                let flaskData = item.flaskData.get();
                // L3573
                if let Some(flaskData) = flaskData {
                    let duration_or0 = || or0(tk(&flaskData, "duration"));
                    // L3574
                    if tk(&flaskData, "lifeTotal").truthy() {
                        // L3575
                        let t = lua_fmt(
                            &format!("^x7F7F7FLife: {magic}%d ^x7F7F7F(%.1fs)"),
                            &[&tk(&flaskData, "lifeTotal"), &duration_or0()],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3576
                        drawY += lineHeight;
                    }
                    // L3578
                    if tk(&flaskData, "manaTotal").truthy() {
                        // L3579
                        let t = lua_fmt(
                            &format!("^x7F7F7FMana: {magic}%d ^x7F7F7F(%.1fs)"),
                            &[&tk(&flaskData, "manaTotal"), &duration_or0()],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3580
                        drawY += lineHeight;
                    }
                    // L3582
                    if !tk(&flaskData, "lifeTotal").truthy()
                        && !tk(&flaskData, "manaTotal").truthy()
                        && tk(&flaskData, "duration").truthy()
                    {
                        // L3583
                        let t = lua_fmt(
                            &format!("^x7F7F7FDuration: {magic}%.2fs"),
                            &[&tk(&flaskData, "duration")],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3584
                        drawY += lineHeight;
                    }
                    // L3586
                    if tk(&flaskData, "chargesUsed").truthy()
                        && tk(&flaskData, "chargesMax").truthy()
                    {
                        // L3587
                        let t = lua_fmt(
                            &format!("^x7F7F7FCharges: {magic}%d/%d"),
                            &[
                                &tk(&flaskData, "chargesUsed"),
                                &tk(&flaskData, "chargesMax"),
                            ],
                        );
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3588
                        drawY += lineHeight;
                    }
                    // Flask buff mods
                    // L3591 (`item.buffModLines` is always a list)
                    // L3592 (ORDER: module notes)
                    for modLine in item.buffModLines.get().snapshot() {
                        // L3593
                        let color = if modLine.truthy(n!("extra")) {
                            cx.main.cc.UNSUPPORTED.clone()
                        } else {
                            cx.main.cc.MAGIC.clone()
                        };
                        // L3594
                        let t = format!("{color}{}", mod_line_text(&modLine));
                        e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                        // L3595
                        drawY += lineHeight;
                    }
                }
            }

            // Quality (if not shown in type-specific section)
            // L3602
            if let Some(q) = item.quality.get() {
                if q > 0.0 && base.weapon.is_none() && base.armour.is_none() && base.flask.is_none()
                {
                    // L3603
                    let t = lfmt!(&format!("^x7F7F7FQuality: {magic}+%d%%"), q);
                    e.emit(cx, x, drawY, Align::Left, fontSize, &t);
                    // L3604
                    drawY += lineHeight;
                }
            }
        }

        // Separator before mods
        // L3609
        if drawY > startY + 18.0 {
            // L3610
            drawY += 2.0;
        }

        // Mod lines with diff highlighting
        // L3614
        for modListData in mod_line_lists(item) {
            // L3615
            let mut drewAny = false;
            // L3616
            for modLine in modListData {
                // L3617
                if item.CheckModLineVariant(&modLine) {
                    // L3618
                    let formatted = formatModLine(&modLine, false);
                    // L3619
                    if let Some(mut formatted) = formatted {
                        // L3620
                        if let Some(otherModMap) = otherModMap {
                            let line = mod_line_text(&modLine);
                            // L3621
                            let template = modLineTemplate(&line);
                            // L3622
                            let otherEntry = otherModMap.get(&template);
                            match otherEntry {
                                // L3623
                                None => {
                                    // Mod exists only on this side
                                    // L3625
                                    formatted = format!("{}+ {formatted}", cx.main.cc.POSITIVE);
                                }
                                // L3626
                                Some(otherEntry) if otherEntry.line != line => {
                                    // Same mod template but different values
                                    // L3628-3629
                                    let myVal = modLineValue(&line, false);
                                    let otherVal = otherEntry.value;
                                    let (Some(myVal), Some(otherVal)) = (myVal, otherVal) else {
                                        lua_error!("attempt to compare nil with number")
                                    };
                                    // L3630
                                    if myVal > otherVal {
                                        // L3631
                                        formatted = format!("{}> {formatted}", cx.main.cc.POSITIVE);
                                    } else if myVal < otherVal {
                                        // L3633
                                        formatted = format!("{}< {formatted}", cx.main.cc.NEGATIVE);
                                    }
                                    // If equal after rounding, no indicator needed
                                }
                                // If exact match (same line text), no indicator — it's identical
                                Some(_) => {}
                            }
                        }
                        // L3639
                        e.emit(cx, x, drawY, Align::Left, fontSize, &formatted);
                        // L3640
                        drawY += lineHeight;
                        // L3641
                        drewAny = true;
                    }
                }
            }
            // L3645
            if drewAny {
                // small gap between mod sections
                // L3646
                drawY += 2.0;
            }
        }

        // Corrupted/Split/Mirrored
        // L3651
        if item.corrupted.get() {
            // L3652
            let t = format!("{}Corrupted", cx.main.cc.NEGATIVE);
            e.emit(cx, x, drawY, Align::Left, fontSize, &t);
            // L3653
            drawY += lineHeight;
        }
        // L3655
        if item.split.get() {
            // L3656
            let t = format!("{}Split", cx.main.cc.NEGATIVE);
            e.emit(cx, x, drawY, Align::Left, fontSize, &t);
            // L3657
            drawY += lineHeight;
        }
        // L3659
        if item.mirrored.get() {
            // L3660
            let t = format!("{}Mirrored", cx.main.cc.NEGATIVE);
            e.emit(cx, x, drawY, Align::Left, fontSize, &t);
            // L3661
            drawY += lineHeight;
        }

        // L3664
        if measureMode {
            return e.maxLineW;
        }
        // L3665
        drawY - startY
    }

    /// lua: CompareTab.lua:3668-3674 `CompareTabClass:ShouldShowRing3(compareEntry)`
    /// @pob Classes/CompareTab.lua:3668-3674 h=18d75a33
    pub fn ShouldShowRing3(cx: &Cx, compareEntry: &CompareEntryRef) -> bool {
        let flag = |b: &model::BuildRef| {
            b.calcsTab.mainEnv.get().is_some_and(|env| {
                env.modDB
                    .get()
                    .Flag(None, pob_lua::nm!["AdditionalRingSlot"])
            })
        };
        // L3669-3672
        let primaryHas = flag(&cx.build().model);
        let compareHas = flag(&compareEntry.model);
        // L3673
        primaryHas || compareHas
    }

    /// lua: CompareTab.lua:3676-3693 `CompareTabClass:AddAbyssSockets(comparison, destTable,
    /// requireBothSides)`
    /// @pob Classes/CompareTab.lua:3676-3693 h=28e2fa53
    pub fn AddAbyssSockets(
        cx: &Cx,
        comparison: &CompareEntryRef,
        destTable: &mut Vec<String>,
        requireBothSides: bool,
    ) {
        let p = cx.build().model.clone();
        let c = &comparison.model;
        // `slots[name].shown()` (CCR-0143 §5, as CCR-0138 §2)
        let shown = |b: &model::BuildRef, name: &str| {
            let slot = slot_at(b, name)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            abyssal_slot_shown(b, &slot)
        };
        // L3680-3681
        let equipmentSlots = [
            "Weapon 1",
            "Weapon 2",
            "Weapon 1 Swap",
            "Weapon 2 Swap",
            "Helmet",
            "Body Armour",
            "Gloves",
            "Boots",
            "Belt",
        ];
        // L3682
        for slot in equipmentSlots {
            // L3683
            for number in 1..=6 {
                // L3684
                let abyssalSocketName = format!("{slot} Abyssal Socket {number}");
                // L3685
                let mainHas = shown(&p, &abyssalSocketName);
                // L3686
                let comparisonHas = shown(c, &abyssalSocketName);
                // L3687-3688
                if (requireBothSides && mainHas && comparisonHas)
                    || (!requireBothSides && (mainHas || comparisonHas))
                {
                    // L3689
                    destTable.push(abyssalSocketName);
                }
            }
        }
    }

    /// lua: CompareTab.lua:3695-4044 `CompareTabClass:DrawItems(vp, compareEntry, inputEvents)`
    /// @pob Classes/CompareTab.lua:3695-4044 h=c82a4fcf
    #[allow(clippy::too_many_lines, clippy::needless_late_init)] // `local primaryContentW` (L3724)
    pub fn DrawItems(
        cx: &mut Cx,
        me: CtrlId,
        vp: &ViewPort,
        compareEntry: &CompareEntryRef,
        inputEvents: &mut InputEvents,
    ) {
        let p = cx.build().model.clone();
        let c = compareEntry.model.clone();
        // L3696-3697
        let mut baseSlots: Vec<String> = [
            "Weapon 1",
            "Weapon 2",
            "Weapon 1 Swap",
            "Weapon 2 Swap",
            "Helmet",
            "Body Armour",
            "Gloves",
            "Boots",
            "Amulet",
            "Ring 1",
            "Ring 2",
            "Belt",
            "Flask 1",
            "Flask 2",
            "Flask 3",
            "Flask 4",
            "Flask 5",
        ]
        .iter()
        .map(|s| (*s).to_owned())
        .collect();
        // L3698
        if Self::ShouldShowRing3(cx, compareEntry) {
            // L3699 t_insert(baseSlots, 10, "Ring 3")
            baseSlots.insert(9, "Ring 3".to_owned());
        }

        // L3702
        Self::AddAbyssSockets(cx, compareEntry, &mut baseSlots, false);

        // L3704-3705
        let primaryHasRing3 = p.calcsTab.mainEnv.get().is_some_and(|env| {
            env.modDB
                .get()
                .Flag(None, pob_lua::nm!["AdditionalRingSlot"])
        });
        // L3706
        let lineHeight = 20.0;
        let _ = lineHeight;

        // Fetch jewel slots once so we can factor their labels and items into width measurement.
        // L3709
        let jewelSlots = Self::GetJewelComparisonSlots(cx, compareEntry);

        // Unified max slot label width across base + jewel slots
        // L3712
        let mut maxLabelW: f64 = 0.0;
        // L3713
        for sn in &baseSlots {
            // L3714
            let w = cx
                .sg
                .draw_string_width(16.0, Font::Var, &format!("^7{sn}:"));
            // L3715
            if w > maxLabelW {
                maxLabelW = w;
            }
        }
        // L3717
        for jE in &jewelSlots {
            // L3718
            let w = cx
                .sg
                .draw_string_width(16.0, Font::Var, &format!("^7{}:", jE.label));
            // L3719
            if w > maxLabelW {
                maxLabelW = w;
            }
        }
        // L3721
        maxLabelW += 2.0;

        // Measure widest primary-side content to decide compare column x-offset
        // L3724
        let primaryContentW;
        // L3725
        if st(cx, me).itemsExpandedMode {
            // Expanded mode: measure the widest rendered line of every primary item card
            // L3727
            let mut widest: f64 = 0.0;
            // L3728-3729 (`self.primaryBuild.itemsTab` always exists)
            // L3730
            for slotName in &baseSlots {
                // L3731-3732
                let pItem = slot_item(&p, slotName);
                // L3733
                if let Some(pItem) = &pItem {
                    // L3734
                    let w = Self::DrawItemExpanded(cx, Some(pItem), 0.0, 0.0, 0.0, None, true);
                    // L3735
                    if w > widest {
                        widest = w;
                    }
                }
            }
            // L3738
            for jE in &jewelSlots {
                // L3739
                if let Some(pItem) = &jE.pItem {
                    // L3740
                    let w = Self::DrawItemExpanded(cx, Some(pItem), 0.0, 0.0, 0.0, None, true);
                    // L3741
                    if w > widest {
                        widest = w;
                    }
                }
            }
            // Label row in expanded mode: drawn at x=10, diff label drawn right-aligned at colWidth-10
            // L3745
            let labelRowW = 10.0
                + cx.sg
                    .draw_string_width(16.0, Font::Var, &format!("^7{}:", baseSlots[0]));
            // Item card drawn at x + 20, so total primary-column content width is 20 + widestLine + 10 padding
            // L3747
            primaryContentW = m_max(20.0 + widest + 10.0, labelRowW);
        } else {
            // Compact mode
            // L3750
            let mut maxDiffW: f64 = 0.0;
            // L3751-3754 (both ItemsTabs always exist)
            // L3755-3761
            let mut measureDiff = |cx: &Cx, pItem: Option<&ItemRef>, cItem: Option<&ItemRef>| {
                // L3756
                let lbl = getSlotDiffLabel(cx, pItem, cItem);
                // L3757
                if !lbl.is_empty() {
                    // L3758
                    let w = cx.sg.draw_string_width(14.0, Font::Var, &lbl);
                    // L3759
                    if w > maxDiffW {
                        maxDiffW = w;
                    }
                }
            };
            // L3762
            for slotName in &baseSlots {
                // L3763-3766
                let pItem = slot_item(&p, slotName);
                let cItem = slot_item(&c, slotName);
                // L3767
                measureDiff(cx, pItem.as_ref(), cItem.as_ref());
            }
            // L3769
            for jE in &jewelSlots {
                // L3770
                measureDiff(cx, jE.pItem.as_ref(), jE.cItem.as_ref());
            }
            // L3772
            primaryContentW = 10.0 + maxLabelW + 4.0 + ITEM_BOX_W + 6.0 + maxDiffW;
        }

        // L3775-3778
        let colWidth = primaryContentW + LAYOUT::compareColGap;
        let contentWidth = colWidth * 2.0;
        let needsHScroll = contentWidth > vp.width;
        cx.w_mut::<CompareTab>(me).layout.itemsColWidth = Some(colWidth);

        // L3780
        let checkboxOffset = LAYOUT::itemsCheckboxOffset;

        // Position + configure the horizontal scrollbar
        // L3783
        let hBar = ctl(cx, me, "itemsHScrollBar");
        // L3784-3786
        set_xy(
            cx,
            hBar,
            vp.x,
            vp.y + vp.height - LAYOUT::itemsHScrollBarHeight,
        );
        cx.base_mut(hBar).width = Prop::Val(vp.width);
        // L3787
        ScrollBarControl::set_content_dimension(cx, hBar, contentWidth, vp.width);
        // L3788
        st_mut(cx, me).itemsScrollX = cx.w::<ScrollBarControl>(hBar).offset;

        // L3790
        let bottomReserve = if needsHScroll {
            LAYOUT::itemsHScrollBarHeight
        } else {
            0.0
        };
        // L3791
        let scrollViewH = vp.height - checkboxOffset - bottomReserve;
        // L3792
        cx.sg
            .set_viewport(vp.x, vp.y + checkboxOffset, vp.width, scrollViewH);
        // L3793-3794
        let drawY = 4.0 - st(cx, me).scrollY;
        let scrollOffsetX = -st(cx, me).itemsScrollX;

        // Get cursor position relative to viewport for hover detection
        // L3797-3799
        let (mut cursorX, mut cursorY) = cx.sys.cursor_pos();
        cursorX -= vp.x;
        cursorY -= vp.y + checkboxOffset;
        // L3800-3913: the hover, click and Equip-hover locals and the two local functions
        // (ItemsFrame, ItemsFrame::processSlotButtons, drawSlotEntry)
        let mut f = ItemsFrame {
            drawY,
            colWidth,
            contentWidth,
            scrollOffsetX,
            checkboxOffset,
            scrollViewH,
            cursorX,
            cursorY,
            vp: *vp,
            hoverItem: None,
            hoverX: 0.0,
            hoverY: 0.0,
            hoverW: 0.0,
            hoverH: 0.0,
            hoverItemsTab: None,
            clickedCopySlot: None,
            clickedEquipSlot: None,
            clickedBuySlot: None,
            clickedBuyItem: None,
            hoverEquipItem: None,
            hoverEquipSlotName: None,
            hoverEquipBtnX: 0.0,
            hoverEquipBtnY: 0.0,
            hoverEquipBtnW: 0.0,
            hoverEquipBtnH: 0.0,
        };

        // L3915
        for slotName in &baseSlots {
            // Get items from both builds
            // L3917-3920
            let pItem = slot_item(&p, slotName);
            let cItem = slot_item(&c, slotName);

            // L3922
            let slotMissing = slotName == "Ring 3" && !primaryHasRing3;
            // L3923
            drawSlotEntry(
                cx,
                me,
                &mut f,
                inputEvents,
                slotName,
                pItem.as_ref(),
                cItem.as_ref(),
                slotName,
                slotName,
                maxLabelW,
                None,
                None,
                slotMissing,
            );
        }

        // === TREE SET DROPDOWNS ===
        // L3927
        f.drawY += 22.0;

        // Convert drawY to absolute screen coords for control positioning
        // L3930
        let absY = vp.y + checkboxOffset + f.drawY;
        // L3931
        let treeSetLabelW = cx.sg.draw_string_width(16.0, Font::Var, "^7Tree set:") + 4.0;

        // L3933-3936
        let pLabel = ctl(cx, me, "primaryTreeSetLabel");
        set_xy(cx, pLabel, vp.x + scrollOffsetX + 10.0, absY + 2.0);
        let pSelect = ctl(cx, me, "primaryTreeSetSelect");
        set_xy(
            cx,
            pSelect,
            vp.x + scrollOffsetX + 10.0 + treeSetLabelW,
            absY,
        );

        // L3938-3941
        let cLabel = ctl(cx, me, "compareTreeSetLabel");
        set_xy(
            cx,
            cLabel,
            vp.x + scrollOffsetX + colWidth + 10.0,
            absY + 2.0,
        );
        let cSelect = ctl(cx, me, "compareTreeSetSelect");
        set_xy(
            cx,
            cSelect,
            vp.x + scrollOffsetX + colWidth + 10.0 + treeSetLabelW,
            absY,
        );

        // Populate tree set lists
        // L3944 (`self.primaryBuild.treeTab` always exists)
        {
            // L3945
            let list: Vec<DropItem> = TreeTab::GetSpecList(cx)
                .into_iter()
                .map(DropItem::str)
                .collect();
            cx.w_mut::<DropDownControl>(pSelect).list = list;
            // L3946
            cx.w_mut::<DropDownControl>(pSelect).sel_index = sel(p.treeTab.activeSpec.get());
        }
        // L3948 (`compareEntry.treeTab` always exists)
        {
            // L3949
            let list: Vec<DropItem> = entry::GetSpecList(compareEntry)
                .into_iter()
                .map(DropItem::str)
                .collect();
            cx.w_mut::<DropDownControl>(cSelect).list = list;
            // L3950
            cx.w_mut::<DropDownControl>(cSelect).sel_index = sel(c.treeTab.activeSpec.get());
        }

        // L3953
        f.drawY += 24.0;

        // === JEWELS SECTION ===
        // L3956
        if !jewelSlots.is_empty() {
            // Section header
            // L3958
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L3959
            cx.sg.draw_string(
                scrollOffsetX + 10.0,
                f.drawY,
                Align::Left,
                16.0,
                Font::Var,
                "^7-- Jewels --",
            );
            // L3960
            f.drawY += 20.0;

            // L3962
            for jEntry in &jewelSlots {
                // Tree allocation warning text
                // L3964-3965
                let warn = format!("{}  (tree missing allocated node)", cx.main.cc.WARNING);
                let pWarn = if jEntry.pItem.is_some() && !jEntry.pNodeAllocated {
                    warn.clone()
                } else {
                    String::new()
                };
                let cWarn = if jEntry.cItem.is_some() && !jEntry.cNodeAllocated {
                    warn
                } else {
                    String::new()
                };

                // L3967
                drawSlotEntry(
                    cx,
                    me,
                    &mut f,
                    inputEvents,
                    &jEntry.label,
                    jEntry.pItem.as_ref(),
                    jEntry.cItem.as_ref(),
                    &jEntry.cSlotName,
                    &jEntry.pSlotName,
                    maxLabelW,
                    Some(&pWarn),
                    Some(&cWarn),
                    false,
                );
            }
        }

        // Process item copy button clicks
        // L3972
        if let Some(slot) = f.clickedCopySlot.clone() {
            // L3973
            Self::CopyCompareItemToPrimary(cx, &slot, compareEntry, false);
        } else if let Some(slot) = f.clickedEquipSlot.clone() {
            // L3975
            Self::CopyCompareItemToPrimary(cx, &slot, compareEntry, true);
        }

        // Process buy button click
        // L3979
        if let (Some(slot), Some(item)) = (f.clickedBuySlot.clone(), f.clickedBuyItem.clone()) {
            // L3980 (U33's seam for `buySimilar.openPopup(item, slot, build)`, CCR-0143 §7)
            let hook = crate::tabs::items::ctor::items_ui_hooks().buy_similar_open_popup;
            let tab = items_tab_node(cx).unwrap_or_default();
            hook(cx, tab, Some(item), Some(slot));
        }

        // Draw item tooltip on hover (compact mode only, on top of everything)
        // L3984
        cx.sg.reset_viewport();
        // L3985
        let maxTooltipWidth = m_min(600.0, m_max(260.0, vp.width - 24.0));
        // L3986
        if cx.main.popups.is_empty() && f.hoverItem.is_some() && f.hoverItemsTab.is_some() {
            let hoverItem = f.hoverItem.clone().expect("hoverItem");
            let (hx, hy, hw, hh) = (f.hoverX, f.hoverY, f.hoverW, f.hoverH);
            cx.lend::<CompareTab, _, _>(
                me,
                |w| &mut w.init.itemTooltip,
                |cx, tt| {
                    // L3987
                    tt.clear(false);
                    // L3988 (`hoverItemsTab`: the primary build's ItemsTab or `compareEntry.itemsTab`)
                    let hoverItemsTab = match f.hoverItemsTab {
                        Some(ItemsTabSide::Compare) => Some(
                            crate::tabs::items::tooltip::ItemsTabSelf::Entry(compareEntry.clone()),
                        ),
                        _ => {
                            items_tab_node(cx).map(crate::tabs::items::tooltip::ItemsTabSelf::Open)
                        }
                    };
                    if let Some(hoverItemsTab) = hoverItemsTab {
                        crate::tabs::items::tooltip::AddItemTooltip(
                            cx,
                            hoverItemsTab,
                            tt,
                            &hoverItem,
                            &Val::Nil,
                            false,
                            Some(maxTooltipWidth),
                        );
                    }
                    // L3989
                    cx.sg.set_draw_layer(None, Some(100));
                    // L3990
                    tt.draw(
                        cx,
                        vp.x + hx,
                        vp.y + checkboxOffset + hy,
                        Some(hw),
                        Some(hh),
                        vp,
                    );
                    // L3991
                    cx.sg.set_draw_layer(None, Some(0));
                },
            );
        }

        // Draw stat comparison tooltip when hovering Equip button
        // L3995
        if cx.main.popups.is_empty() && f.hoverItem.is_none() {
            if let (Some(hoverEquipItem), Some(hoverEquipSlotName)) =
                (f.hoverEquipItem.clone(), f.hoverEquipSlotName.clone())
            {
                let (bx, by, bh) = (f.hoverEquipBtnX, f.hoverEquipBtnY, f.hoverEquipBtnH);
                cx.lend::<CompareTab, _, _>(
                    me,
                    |w| &mut w.init.itemTooltip,
                    |cx, tt| {
                        Self::equip_tooltip(
                            cx,
                            tt,
                            &hoverEquipItem,
                            &hoverEquipSlotName,
                            maxTooltipWidth,
                        );
                        // L4034
                        cx.sg.set_draw_layer(None, Some(100));
                        // Force tooltip to the left of the button by passing a large width
                        // so the right-side placement overflows and the Draw logic flips to left
                        // L4037
                        tt.draw(
                            cx,
                            vp.x + bx,
                            vp.y + checkboxOffset + by,
                            Some(vp.width),
                            Some(bh),
                            vp,
                        );
                        // L4038
                        cx.sg.set_draw_layer(None, Some(0));
                    },
                );
            }
        }

        // L4041
        let s = st_mut(cx, me);
        s.itemsTotalContentHeight = f.drawY + s.scrollY + 36.0;

        // L4043
        cx.sg.reset_viewport();
    }

    /// L3996-4033 of DrawItems: the Equip button's stat comparison lines.
    fn equip_tooltip(
        cx: &mut Cx,
        tt: &mut Tooltip,
        hoverEquipItem: &ItemRef,
        hoverEquipSlotName: &str,
        maxTooltipWidth: f64,
    ) {
        // L3996
        tt.clear(false);
        // L3997
        tt.max_width = Some(maxTooltipWidth);
        // L3998 (DIVERGENCE(web-pending): Pending reads as a nil calcFunc)
        let m = cx.build().model.clone();
        let Poll::Ready((calcFunc, calcBase)) = cx.build_mut().calc.misc_calculator(&m) else {
            return;
        };
        // L3999
        {
            // Create a fresh item to evaluate
            // L4001 (`hoverEquipItem.raw` is always a string)
            let raw = hoverEquipItem.raw.get().to_string();
            let newItem = new_Item(Some(&raw), None, false);
            // L4002
            newItem.NormaliseQuality();

            // Determine what's currently in the target slot
            // L4005-4006
            let pSlot = slot_at(&m, hoverEquipSlotName);
            let selItem = pSlot
                .as_ref()
                .and_then(|s| item_at(&m, &s.get(n!("selItemId"))));

            // For jewel sockets that aren't allocated, temporarily allocate the node
            // L4009
            let mut ov = Override {
                repSlotName: Field::new(Some(Name::from(hoverEquipSlotName))),
                repItem: Field::new(Some(newItem.clone())),
                ..Override::default()
            };
            // L4010
            if let Some(nodeId) = pSlot
                .as_ref()
                .map(|s| s.get(n!("nodeId")))
                .filter(Val::truthy)
            {
                // L4011
                let pSpec = m.spec.get();
                // L4012 (`pSpec.allocNodes` always exists; a non-integer id is in neither map)
                let id = nodeId.num();
                let key = (id.fract() == 0.0).then_some(id as i64);
                let allocated = key.is_some_and(|k| pSpec.allocNodes.with(|a| a.get(k)).is_some());
                if !allocated {
                    // L4013
                    let node = key.and_then(|k| pSpec.nodes.with(|n| n.get(k)));
                    // L4014
                    if let Some(node) = node {
                        // L4015
                        ov.addNodes = Field::new(Some(node_set(&node)));
                    }
                }
            }

            // L4020
            let Poll::Ready(output) = calcFunc.call(Some(Rc::new(ov)), None) else {
                return;
            };
            // L4021
            let slotLabel = pSlot
                .as_ref()
                .and_then(|s| s.str(n!("label")))
                .map_or_else(|| hoverEquipSlotName.to_owned(), |s| s.to_string());
            // L4022-4023
            let header = if let Some(selItem) = &selItem {
                // L4024
                let rarity = selItem.rarity.get();
                let rc = cx
                    .main
                    .cc
                    .get(rarity.as_str())
                    .cloned()
                    .unwrap_or_else(|| "^7".to_owned());
                format!(
                    "^7Equipping this item in {slotLabel} will give you:\n(replacing {rc}{}^7)",
                    selItem.name.get()
                )
            } else {
                // L4026
                format!("^7Equipping this item in {slotLabel} will give you:")
            };
            // L4028
            let count = crate::build::stats::add_stat_compares_to_tooltip(
                cx, tt, &calcBase, &output, &header, None,
            );
            // L4029
            if count == 0.0 {
                // L4030-4031
                tt.add_line(cx, 14.0, &header);
                tt.add_line(cx, 14.0, "^7No changes.");
            }
        }
    }
}

/// `string.format(fmt, ...)` with Lua values as arguments.
fn lua_fmt(fmt: &str, args: &[&Val]) -> String {
    let a: Vec<FmtArg<'_>> = args.iter().map(|v| fa(v)).collect();
    pob_lua::lua_format(fmt, &a)
}

/// `slot.shown()` of a model abyssal-socket slot (ItemsTab.lua:173-175 / 195-197,
/// ItemSlotControl.lua:34-36; CCR-0143 §5, as CCR-0138 §2).
fn abyssal_slot_shown(build: &model::BuildRef, slot: &Table) -> bool {
    let useSecond = || {
        build
            .itemsTab
            .activeItemSet
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
            .truthy(n!("useSecondWeaponSet"))
    };
    match slot.str(n!("$shown")).as_deref() {
        Some("abyssalSwap") => !slot.truthy(n!("inactive")) && useSecond(),
        Some("abyssalWeapon") => !slot.truthy(n!("inactive")) && !useSecond(),
        _ => !slot.truthy(n!("inactive")),
    }
}
