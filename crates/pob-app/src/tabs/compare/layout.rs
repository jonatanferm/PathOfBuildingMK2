//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua:1650-2515 (Draw,
//! DrawControlList, LayoutTreeView, syncControlValue, LayoutConfigView, UpdateSetSelectors,
//! RefreshCalcsSkillControls, LayoutCalcsSkillControls, HandleScrollInput, GetGemGrantedEffect,
//! GetSocketGroupSignature, GetSocketGroupLabel). Owner: U47.
//!
//! @pob-file Classes/CompareTab.lua @16de4b82d
//!
//! # Where the tab is drawn
//!
//! BuildMode draws the tab through `ShellHooks::draw_tab` (`self.compareTab:Draw(tabViewPort,
//! inputEvents)`, Build.lua:1317): [`draw_tab`] is the `BuildTab::Compare` arm (CCR-0136 §2,
//! installed in `build/on_frame.rs`'s `default_shell_hooks`).
//!
//! # The sub-view passes (U49, U50)
//!
//! `Draw` dispatches to `DrawTree`, `DrawSummary`, `DrawItems` (U49, `draw_a.rs`), `DrawSkills`,
//! `DrawCalcs`, `DrawConfig` (U50, `draw_b.rs`). They are reached through [`CompareDrawHooks`]
//! (CCR-0137 §3): until their owners install theirs, each one draws nothing (the placeholder
//! [`default_draw_hooks`]), so the tab and every layout pass of this file run.
//!
//! # The fields this file writes
//!
//! [`State`]: `self.treeLayout` (LayoutTreeView, read by DrawTree), `self.configSectionLayout`
//! (LayoutConfigView, read by DrawConfig), `self.calcsSkillHeaderHeight` (Draw, read by
//! DrawCalcs) and `self.itemsColWidth`, which DrawItems (U49, CompareTab.lua:3778) writes and
//! Draw reads (nil until the first Items pass). The constructor values (`treeLayout = nil`,
//! `configSectionLayout = {}`) are the defaults (CCR-0136 §2).
//!
//! # RefreshSkillSelectControls
//!
//! `self.primaryBuild:RefreshSkillSelectControls(primControls, ...)` (L2274) takes a table of
//! the tab's own controls under the keys Build.lua reads (`mainSocketGroup`, `mainSkill`, ...,
//! `mainSkillMinionLibrary = { shown = false }`). U21's port reads those keys from a host's
//! `controls`, so the map is a host node of its own ([`ControlMap`], a free node of the tab
//! that is never drawn) whose `controls` hold the tab's controls (`Cx::put`, as the Lua table
//! references them) and a placeholder control for the `{ shown = false }` table. The
//! comparison build's half is CompareEntry's own method ([`SkillControls`]).
//!
//! # Lua semantics
//!
//! * `ctrl.shown = function() ... (type(baseShown) == "function" and baseShown() or baseShown)
//!   end` (L2312-2315): `baseShown` is truthy there (the wrap is under `if baseShown`), so the
//!   `and/or` yields `baseShown()` when that is truthy and `baseShown` (truthy) otherwise: the
//!   third operand is always truthy. The port's wrapper is the view-mode gate alone (the inner
//!   functions are pure); it also does not chain one closure per frame.
//! * `control.shown == false` (L2336) is the raw field; `control.width` is a number (truthy).
//! * `#wrapInfoLine(...)` is the line count; `label .. ": " .. value` concatenates the output
//!   string (a number concatenates as `tostring`).
//! * `colY[c]` (L2167-2191) is a table indexed by column number: [`ColY`].
//! * `PCall(string.matchOrPattern, s, pattern)`: a raising pattern gives `false`.
//! * `searchStr:match("%S")`: `pob_lua::pattern::find`.
//! * `inputEvents[id] = nil` is `InputEvents::consume`.
//!
//! ORDER: `pairs(self.configControls)` (L2060) syncs each control from its own input value
//! (order-independent, (c)). The controls are positioned in `ipairs` order.
//!
//! Props with side effects (ui doc §7.3): none (the `shown` closures read only).

#![allow(non_snake_case)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::rc::Rc;

use pob_lua::math::{m_floor, m_max, m_min};
use pob_lua::Name;
use pob_sg::{Align, Font, Key};
use pob_ui_core::controls::scroll_bar::ScrollBarControl;
use pob_ui_core::controls::{CheckBoxControl, DropDownControl, DropItem, EditControl};
use pob_ui_core::input::EventKind;
use pob_ui_core::prelude::*;
use pob_ui_core::widget::PlainControl;

use super::entry::{self, CompareEntry, SkillControls};
use super::init::{self, ctl, st, st_mut, wrapInfoLine, BuffModeValue, CompareEntryRef, ConfigCtrlInfo, SetTab, LAYOUT};
use super::CompareTab;
use crate::app::CxApp;
use crate::model::gamedata::schema::config_defs::ConfigOptionDef;
use crate::model::{self, Val};
use crate::tabs::config::ctor::tostring_val;
use crate::tabs::config::visibility as configVisibility;
use crate::tabs::tree::TreeTab;

// ---- the fields this file writes ---------------------------------------------------------------

/// `self.treeLayout` (L1897-1902 overlay, L1940-1947 side by side).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct TreeLayout {
    pub overlay: bool,
    /// nil in overlay mode
    pub halfWidth: Option<f64>,
    pub headerHeight: f64,
    pub footerHeight: f64,
    pub footerY: f64,
    /// nil in overlay mode
    pub rightAbsX: Option<f64>,
}

/// One `rows[i]` of a visible config section (L2149-2150).
pub struct ConfigRow {
    pub ctrlInfo: Rc<ConfigCtrlInfo>,
    pub isDiff: bool,
}

/// One `self.configSectionLayout[i]` (L2154-2160, with `x`/`y` from L2189-2190).
pub struct ConfigSectionLayout {
    pub name: String,
    pub col: Option<f64>,
    pub rows: Vec<ConfigRow>,
    pub height: f64,
    pub diffCount: usize,
    pub x: f64,
    pub y: f64,
}

/// This unit's part of the CompareTab struct (ui doc §12.1; module notes).
#[derive(Default)]
pub struct State {
    /// `self.treeLayout` (nil outside the Tree view with a comparison)
    pub treeLayout: Option<TreeLayout>,
    /// `self.configSectionLayout`
    pub configSectionLayout: Vec<Rc<ConfigSectionLayout>>,
    /// `self.calcsSkillHeaderHeight` (nil before the first Draw reads as 0 nowhere: Draw sets it
    /// first)
    pub calcsSkillHeaderHeight: f64,
    /// `self.itemsColWidth` (DrawItems, U49, writes it)
    pub itemsColWidth: Option<f64>,
    /// The `primControls` map host (module notes), created on first use.
    pub primControlsMap: Option<CtrlId>,
}

fn lst(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<CompareTab>(me).layout
}

fn lst_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<CompareTab>(me).layout
}

// ---- the sub-view passes (U49, U50) -----------------------------------------------------------

/// The Draw* passes of U49/U50 (CCR-0137 §3). `me` is the tab.
#[derive(Clone)]
pub struct CompareDrawHooks {
    /// `self:DrawTree(contentVP, inputEvents, compareEntry)` (U49, CompareTab.lua:3403)
    pub draw_tree: Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, &mut InputEvents, &CompareEntryRef)>,
    /// `self:DrawSummary(contentVP, compareEntry)` (U49, CompareTab.lua:3162)
    pub draw_summary: Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, &CompareEntryRef)>,
    /// `self:DrawItems(contentVP, compareEntry, inputEvents)` (U49, CompareTab.lua:3695)
    pub draw_items: Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, &CompareEntryRef, &mut InputEvents)>,
    /// `self:DrawSkills(contentVP, compareEntry)` (U50, CompareTab.lua:4049)
    pub draw_skills: Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, &CompareEntryRef)>,
    /// `self:DrawCalcs(contentVP, compareEntry)` (U50, CompareTab.lua:4684)
    pub draw_calcs: Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, &CompareEntryRef)>,
    /// `self:DrawConfig(contentVP, compareEntry, headerOnly)` (U50, CompareTab.lua:4936)
    pub draw_config: Rc<dyn Fn(&mut Cx, CtrlId, &ViewPort, &CompareEntryRef, bool)>,
}

/// The placeholders: each pass draws nothing until its owner installs it (module notes).
pub fn default_draw_hooks() -> CompareDrawHooks {
    CompareDrawHooks {
        draw_tree: Rc::new(super::draw_a::draw_tree_hook), // U49 (CCR-0143 §6)
        draw_summary: Rc::new(super::draw_a::draw_summary_hook), // U49 (CCR-0143 §6)
        draw_items: Rc::new(super::draw_a::draw_items_hook), // U49 (CCR-0143 §6)
        draw_skills: Rc::new(CompareTab::DrawSkills),
        draw_calcs: Rc::new(CompareTab::DrawCalcs),
        draw_config: Rc::new(CompareTab::DrawConfig),
    }
}

thread_local! {
    static DRAW_HOOKS: RefCell<Option<CompareDrawHooks>> = const { RefCell::new(None) };
}

/// The installed passes (a clone).
pub fn draw_hooks() -> CompareDrawHooks {
    DRAW_HOOKS.with(|h| h.borrow_mut().get_or_insert_with(default_draw_hooks).clone())
}

/// Replace pass entries (tests, and owners that register at run time).
pub fn set_draw_hooks(f: impl FnOnce(&mut CompareDrawHooks)) {
    DRAW_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_draw_hooks)));
}

// ---- helpers (Rust-only) ----------------------------------------------------------------------

/// The `primControls` map host (module notes): a ControlHost that is never drawn.
pub struct ControlMap;
impl Widget for ControlMap {
    const NAME: &'static str = "ControlMap";
    const CAPS: Caps = Caps::empty();
}

fn set_xy(cx: &mut Cx, id: CtrlId, x: f64, y: f64) {
    let b = cx.base_mut(id);
    b.x = Prop::Val(x);
    b.y = Prop::Val(y);
}

fn set_x(cx: &mut Cx, id: CtrlId, x: f64) {
    cx.base_mut(id).x = Prop::Val(x);
}

fn set_y(cx: &mut Cx, id: CtrlId, y: f64) {
    cx.base_mut(id).y = Prop::Val(y);
}

fn set_width(cx: &mut Cx, id: CtrlId, w: f64) {
    cx.base_mut(id).width = Prop::Val(w);
}

/// The raw number field `control.x`.
fn raw_x(cx: &Cx, id: CtrlId) -> f64 {
    cx.base(id).x.raw()
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

/// `self.primaryBuild.treeTab.viewer.searchStr or ""` (None: no tree tab).
fn primary_viewer_search(cx: &Cx) -> Option<String> {
    let id = cx.build().tabs.tree.id();
    cx.try_w::<TreeTab>(id).map(|t| t.core.viewer.search_str.clone())
}

/// `colY` (L2167): a table indexed by column number.
#[derive(Default)]
struct ColY(HashMap<i64, f64>);

impl ColY {
    fn key(c: f64) -> i64 {
        // section columns are integers in ConfigOptions
        c as i64
    }
    fn get(&self, c: f64) -> Option<f64> {
        self.0.get(&Self::key(c)).copied()
    }
    fn set(&mut self, c: f64, v: f64) {
        self.0.insert(Self::key(c), v);
    }
}

/// `PCall(string.matchOrPattern, s, pattern)`: `(err, match)` → `not err and match`.
fn p_call_match(s: &str, pattern: &str) -> bool {
    std::panic::catch_unwind(std::panic::AssertUnwindSafe(|| {
        pob_lua::string::matchOrPattern(s, pattern)
    }))
    .unwrap_or(false)
}

// ---- Draw ---------------------------------------------------------------------------------------

/// `ShellHooks::draw_tab` for `BuildTab::Compare`: `self.compareTab:Draw(tabViewPort,
/// inputEvents)` (Build.lua:1317).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = init::tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'compareTab' (a nil value)"));
    CompareTab::Draw(cx, me, vp, ev);
}

impl CompareTab {
    /// lua: CompareTab.lua:1650-1866 `CompareTabClass:Draw(viewPort, inputEvents)`
    /// DRAW - Main render method
    /// @pob Classes/CompareTab.lua:1650-1866 h=42a34998
    #[allow(clippy::too_many_lines)]
    pub fn Draw(cx: &mut Cx, me: CtrlId, viewPort: &ViewPort, inputEvents: &mut InputEvents) {
        // L1651
        crate::main_mode::background::draw_background(cx, viewPort);
        // Position top-bar controls
        // L1653-1654
        let anchor = ctl(cx, me, "subTabAnchor");
        set_xy(cx, anchor, viewPort.x + 4.0, viewPort.y + 96.0);

        // Draw dividers between top-bar sections when a comparison is loaded
        // L1657
        if !st(cx, me).compareEntries.is_empty() {
            // L1658
            cx.sg.set_draw_color(0.25, 0.25, 0.25);
            // L1659-1661
            cx.sg.draw_image(None, viewPort.x + 4.0, viewPort.y + 32.0, viewPort.width - 8.0, 2.0);
            cx.sg.draw_image(None, viewPort.x + 4.0, viewPort.y + 88.0, viewPort.width - 8.0, 2.0);
            cx.sg.draw_image(None, viewPort.x + 4.0, viewPort.y + 122.0, viewPort.width - 8.0, 2.0);
        }

        // L1664-1666
        let label = ctl(cx, me, "compareBuildLabel");
        cx.base_mut(label).x = Prop::f(|_, _| 0.0);

        // L1668-1673
        let contentVP = ViewPort {
            x: viewPort.x,
            y: viewPort.y + LAYOUT::controlBarHeight,
            width: viewPort.width,
            height: viewPort.height - LAYOUT::controlBarHeight,
        };

        // Get active comparison early (needed for footer positioning before ProcessControlsInput)
        // L1676
        let compareEntry = Self::GetActiveCompare(cx, me);

        // Rebuild compare entry if its buildFlag is set (e.g. after version convert or spec change)
        // L1679
        if let Some(e) = compareEntry.as_ref().filter(|e| e.model.buildFlag.get()) {
            // L1680
            CompareEntry::Rebuild(e);
        }

        // Layout: position controls and draw backgrounds for current view mode
        // (must happen before ProcessControlsInput so controls render on top of backgrounds)
        // L1685
        Self::LayoutTreeView(cx, me, &contentVP, compareEntry.as_ref());
        // L1686
        Self::LayoutConfigView(cx, me, &contentVP, compareEntry.as_ref());
        // L1687
        if let Some(e) = &compareEntry {
            // L1688
            Self::UpdateSetSelectors(cx, me, e);
        }
        // L1690
        let mut i = 0;
        while let Some(event) = inputEvents.next_ipairs(&mut i) {
            // L1691
            if matches!(event.kind, EventKind::KeyDown { .. })
                && event.key == Key::Char(b'f')
                && cx.sys.is_key_down(Key::Ctrl)
            {
                // L1692
                if st(cx, me).compareViewMode == "CONFIG" && compareEntry.is_some() {
                    // L1693
                    let edit = ctl(cx, me, "configSearchEdit");
                    cx.select_control(me, Some(edit));
                }
            }
        }
        // Layout and refresh calcs skill detail controls
        // L1698
        lst_mut(cx, me).calcsSkillHeaderHeight = 0.0;
        // L1699
        if st(cx, me).compareViewMode == "CALCS" {
            if let Some(e) = &compareEntry {
                // L1700
                let h = Self::LayoutCalcsSkillControls(cx, me, &contentVP, Some(e));
                lst_mut(cx, me).calcsSkillHeaderHeight = h;
            }
        }

        // Draw calcs skill header background
        // L1704
        if st(cx, me).compareViewMode == "CALCS" && lst(cx, me).calcsSkillHeaderHeight > 0.0 {
            // L1705
            cx.sg.set_draw_color(0.05, 0.05, 0.05);
            // L1706
            let h = lst(cx, me).calcsSkillHeaderHeight;
            cx.sg.draw_image(None, contentVP.x, contentVP.y, contentVP.width, h);
        }

        // Layout shared vertical scrollbar for Summary/Items/Skills/Config.
        // L1710
        let mode = st(cx, me).compareViewMode;
        // L1711
        let viewScrollBar = ctl(cx, me, "viewScrollBar");
        // L1712
        if compareEntry.is_some() && mode != "TREE" && mode != "CALCS" {
            // L1713-1715
            let mut topReserve = 0.0;
            let mut bottomReserve = 0.0;
            let mut contentHeight = 0.0;
            let s = st(cx, me);
            // L1716
            if mode == "SUMMARY" {
                // L1717-1718
                topReserve = LAYOUT::summaryHeaderHeight;
                contentHeight = s.summaryTotalContentHeight;
            } else if mode == "CONFIG" {
                // L1720-1721
                topReserve = LAYOUT::configFixedHeaderHeight;
                contentHeight = s.configTotalContentHeight;
            } else if mode == "ITEMS" {
                // L1723
                topReserve = LAYOUT::itemsCheckboxOffset;
                // L1724
                let hbar = ctl(cx, me, "itemsHScrollBar");
                bottomReserve = if ScrollBarControl::enabled_raw(cx, hbar) {
                    LAYOUT::itemsHScrollBarHeight
                } else {
                    0.0
                };
                // L1725
                contentHeight = st(cx, me).itemsTotalContentHeight;
            } else if mode == "SKILLS" {
                // L1727
                topReserve = LAYOUT::skillsHeaderHeight;
                // L1728
                let hbar = ctl(cx, me, "skillsHScrollBar");
                bottomReserve = if ScrollBarControl::enabled_raw(cx, hbar) {
                    LAYOUT::skillsHScrollBarHeight
                } else {
                    0.0
                };
                // L1729
                contentHeight = st(cx, me).skillsTotalContentHeight;
            }
            // L1731
            let viewHeight = m_max(contentVP.height - topReserve - bottomReserve, 0.0);
            // L1732-1734
            set_xy(cx, viewScrollBar, contentVP.x + contentVP.width - 18.0, contentVP.y + topReserve);
            cx.base_mut(viewScrollBar).height = Prop::Val(viewHeight);
            // L1735
            ScrollBarControl::set_content_dimension(cx, viewScrollBar, contentHeight, viewHeight);
            // L1736
            let scrollY = st(cx, me).scrollY;
            ScrollBarControl::set_offset(cx, viewScrollBar, scrollY);
            // L1737
            st_mut(cx, me).scrollY = cx.w::<ScrollBarControl>(viewScrollBar).offset;
        } else {
            // L1739
            ScrollBarControl::set_content_dimension(cx, viewScrollBar, 0.0, contentVP.height);
            // L1740
            ScrollBarControl::set_offset(cx, viewScrollBar, 0.0);
        }

        // Process input events for our controls (including footer controls)
        // L1744
        cx.process_controls_input(me, inputEvents, viewPort);
        // L1745
        Self::HandleScrollInput(cx, me, &contentVP, inputEvents);
        // L1746
        if cx.is_shown(viewScrollBar) {
            // L1747
            st_mut(cx, me).scrollY = cx.w::<ScrollBarControl>(viewScrollBar).offset;
        }

        // L1750
        let drawingTree = st(cx, me).compareViewMode == "TREE" && compareEntry.is_some();

        // Draw TREE view BEFORE controls so header dropdowns render on top of the tree
        // L1753
        if drawingTree {
            let e = compareEntry.clone().expect("drawingTree");
            // L1754
            let f = draw_hooks().draw_tree;
            f(cx, me, &contentVP, inputEvents, &e);

            // Elevate to main draw layer 1 (matching TreeTab pattern) so controls
            // render above all tree sublayers (tree uses sublayers up to 100)
            // L1758
            cx.sg.set_draw_layer(Some(1), None);

            // Redraw header + footer backgrounds at this higher layer to cover any
            // tree artifacts that bled into those regions via high sublayers
            // L1762
            let layout = lst(cx, me).treeLayout;
            // L1763
            if let Some(layout) = layout {
                // L1764-1771
                cx.sg.set_draw_color(0.05, 0.05, 0.05);
                cx.sg.draw_image(None, contentVP.x, contentVP.y, contentVP.width, layout.headerHeight);
                cx.sg.set_draw_color(0.85, 0.85, 0.85);
                cx.sg.draw_image(None, contentVP.x, contentVP.y + layout.headerHeight - 2.0, contentVP.width, 2.0);
                cx.sg.set_draw_color(0.05, 0.05, 0.05);
                cx.sg.draw_image(None, contentVP.x, layout.footerY, contentVP.width, layout.footerHeight);
                cx.sg.set_draw_color(0.85, 0.85, 0.85);
                cx.sg.draw_image(None, contentVP.x, layout.footerY, contentVP.width, 2.0);
            }
            // L1773
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L1774-1775
            let lx = raw_x(cx, ctl(cx, me, "leftSpecSelect"));
            let short = Self::GetShortBuildName(cx.build().build_name.as_deref());
            let text = format!("{}{short}", cx.main.cc.POSITIVE);
            cx.sg.draw_string(lx, contentVP.y + 4.0, Align::Left, 18.0, Font::Var, &text);
            // L1776-1777
            let rx = raw_x(cx, ctl(cx, me, "rightSpecSelect"));
            let text = format!("{}{}", cx.main.cc.WARNING, e.label);
            cx.sg.draw_string(rx, contentVP.y + 4.0, Align::Left, 18.0, Font::Var, &text);
        }

        // L1780
        match &compareEntry {
            None => {
                // No comparison build loaded - show instructions
                // L1782
                cx.sg.set_viewport(contentVP.x, contentVP.y, contentVP.width, contentVP.height);
                // L1783
                cx.sg.set_draw_color(1.0, 1.0, 1.0);
                // L1784-1785
                cx.sg
                    .draw_string(0.0, 40.0, Align::Center, 20.0, Font::Var, "^7No comparison build loaded.");
                // L1786-1787
                let pos = cx.main.cc.POSITIVE.clone();
                cx.sg.draw_string(
                    0.0,
                    70.0,
                    Align::Center,
                    16.0,
                    Font::Var,
                    &format!("^7Click {pos}Import...^7 above to import a build to compare against."),
                );
                // L1788
                cx.sg.reset_viewport();
            }
            Some(compareEntry) => {
                // Position items expanded mode checkbox and item set dropdowns (inside content area, top-left)
                // Label draws to the left of the checkbox, so offset x by labelWidth to keep it visible
                // L1792
                if st(cx, me).compareViewMode == "ITEMS" {
                    // L1793-1794
                    let check = ctl(cx, me, "itemsExpandedCheck");
                    let lw = label_width(cx, check);
                    set_xy(cx, check, contentVP.x + 10.0 + lw, contentVP.y + 8.0);

                    // L1796
                    let colWidth = lst(cx, me).itemsColWidth.unwrap_or_else(|| {
                        m_max(m_floor(contentVP.width / 2.0), LAYOUT::itemsMinColWidth)
                    });
                    // L1797
                    let itemSetLabelW = cx.sg.draw_string_width(16.0, Font::Var, "^7Item set:") + 4.0;
                    // L1798
                    let hbar = ctl(cx, me, "itemsHScrollBar");
                    let scrollOffsetX = -cx.w::<ScrollBarControl>(hbar).offset;

                    // Item set dropdowns
                    // L1801-1802
                    let buildLabelY = contentVP.y + 30.0;
                    let row1Y = contentVP.y + 52.0;

                    // Primary build item set dropdown
                    // L1805-1808
                    let pLabel = ctl(cx, me, "primaryItemSetLabel");
                    let pSelect = ctl(cx, me, "primaryItemSetSelect");
                    set_xy(cx, pLabel, contentVP.x + scrollOffsetX + 10.0, row1Y + 2.0);
                    set_xy(cx, pSelect, contentVP.x + scrollOffsetX + 10.0 + itemSetLabelW, row1Y);

                    // Compare build item set dropdown
                    // L1811-1814
                    let cLabel = ctl(cx, me, "compareItemSetLabel2");
                    let cSelect = ctl(cx, me, "compareItemSetSelect2");
                    set_xy(cx, cLabel, contentVP.x + scrollOffsetX + colWidth + 10.0, row1Y + 2.0);
                    set_xy(cx, cSelect, contentVP.x + scrollOffsetX + colWidth + 10.0 + itemSetLabelW, row1Y);

                    // L1816
                    cx.sg.set_draw_color(1.0, 1.0, 1.0);
                    // L1817-1818
                    let short = Self::GetShortBuildName(cx.build().build_name.as_deref());
                    let text = format!("{}{short}", cx.main.cc.POSITIVE);
                    cx.sg.draw_string(raw_x(cx, pLabel), buildLabelY, Align::Left, 18.0, Font::Var, &text);
                    // L1819-1820
                    let text = format!("{}{}", cx.main.cc.WARNING, compareEntry.label);
                    cx.sg.draw_string(raw_x(cx, cLabel), buildLabelY, Align::Left, 18.0, Font::Var, &text);
                    // L1821
                    cx.sg.set_draw_color(0.5, 0.5, 0.5);
                    // L1822
                    cx.sg.draw_image(
                        None,
                        contentVP.x + 4.0,
                        contentVP.y + LAYOUT::itemsCheckboxOffset - 2.0,
                        contentVP.width - 8.0,
                        2.0,
                    );

                    // Populate primary build item set list
                    // L1825-1826
                    let p = cx.build().model.clone();
                    Self::PopulateSetDropdown(cx, SetTab::Items(&p.itemsTab), pSelect);

                    // Populate compare build item set list
                    // L1830-1831
                    Self::PopulateSetDropdown(cx, SetTab::Items(&compareEntry.model.itemsTab), cSelect);
                }

                // Dispatch to sub-view (TREE already drawn above)
                // L1836-1846
                let h = draw_hooks();
                match st(cx, me).compareViewMode {
                    "SUMMARY" => (h.draw_summary)(cx, me, &contentVP, compareEntry),
                    "ITEMS" => (h.draw_items)(cx, me, &contentVP, compareEntry, inputEvents),
                    "SKILLS" => (h.draw_skills)(cx, me, &contentVP, compareEntry),
                    "CALCS" => (h.draw_calcs)(cx, me, &contentVP, compareEntry),
                    "CONFIG" => (h.draw_config)(cx, me, &contentVP, compareEntry, false),
                    _ => {}
                }
            }
        }

        // L1849
        cx.draw_controls(me, viewPort, None);
        // L1850
        if st(cx, me).compareViewMode == "CONFIG" {
            if let Some(e) = &compareEntry {
                // L1851
                let f = draw_hooks().draw_config;
                f(cx, me, &contentVP, e, true);
                // L1852-1858
                let list = [
                    ctl(cx, me, "copyConfigBtn"),
                    ctl(cx, me, "configToggleBtn"),
                    ctl(cx, me, "configSearchEdit"),
                    ctl(cx, me, "configPrimarySetLabel"),
                    ctl(cx, me, "configPrimarySetSelect"),
                ];
                Self::DrawControlList(cx, me, viewPort, &list);
            }
        }
        // L1860
        if drawingTree {
            // L1861
            cx.sg.set_draw_layer(Some(0), None);
        }
        // L1863
        if cx.is_shown(viewScrollBar) {
            // L1864
            st_mut(cx, me).scrollY = cx.w::<ScrollBarControl>(viewScrollBar).offset;
        }
    }

    /// lua: CompareTab.lua:1872-1881 `CompareTabClass:DrawControlList(viewPort, controls)`
    /// DRAW HELPERS
    /// @pob Classes/CompareTab.lua:1872-1881 h=ccd1dac9
    pub fn DrawControlList(cx: &mut Cx, me: CtrlId, viewPort: &ViewPort, controls: &[CtrlId]) {
        // L1873-1875
        let noTooltip = |cx: &Cx, control: CtrlId| {
            cx.host(me)
                .sel_control
                .is_some_and(|s| cx.base(s).has_focus == Some(true) && s != control)
        };
        // L1876
        for &control in controls {
            // L1877
            if cx.is_shown(control) && cx.has(control, Caps::DRAW) {
                // L1878
                let nt = noTooltip(cx, control);
                cx.draw(control, viewPort, nt);
            }
        }
    }

    /// lua: CompareTab.lua:1885-2028 `CompareTabClass:LayoutTreeView(contentVP, compareEntry)`
    /// Pre-draw tree header/footer backgrounds and position tree controls.
    /// Must run before ProcessControlsInput so controls render on top of backgrounds.
    /// @pob Classes/CompareTab.lua:1885-2028 h=e7e6777f
    #[allow(clippy::too_many_lines)]
    pub fn LayoutTreeView(cx: &mut Cx, me: CtrlId, contentVP: &ViewPort, compareEntry: Option<&CompareEntryRef>) {
        // L1886
        lst_mut(cx, me).treeLayout = None;
        // L1887
        let Some(compareEntry) = compareEntry.filter(|_| st(cx, me).compareViewMode == "TREE") else {
            return;
        };

        // L1889-1891
        let headerHeight = LAYOUT::treeHeaderHeight;
        let footerHeight = LAYOUT::treeFooterHeight;
        let footerY = contentVP.y + contentVP.height - footerHeight;

        let leftSpecSelect = ctl(cx, me, "leftSpecSelect");
        let rightSpecSelect = ctl(cx, me, "rightSpecSelect");
        let treeOverlayCheck = ctl(cx, me, "treeOverlayCheck");

        // L1893
        if st(cx, me).treeOverlayMode {
            // ========== OVERLAY MODE LAYOUT ==========
            // L1895
            let specWidth = m_min(m_floor(contentVP.width * 0.25), 200.0);

            // L1897-1902
            lst_mut(cx, me).treeLayout = Some(TreeLayout {
                overlay: true,
                halfWidth: None,
                headerHeight,
                footerHeight,
                footerY,
                rightAbsX: None,
            });

            // Header background + separator
            // L1905-1908
            cx.sg.set_draw_color(0.05, 0.05, 0.05);
            cx.sg.draw_image(None, contentVP.x, contentVP.y, contentVP.width, headerHeight);
            cx.sg.set_draw_color(0.85, 0.85, 0.85);
            cx.sg.draw_image(None, contentVP.x, contentVP.y + headerHeight - 2.0, contentVP.width, 2.0);

            // Footer background
            // L1911-1914
            cx.sg.set_draw_color(0.05, 0.05, 0.05);
            cx.sg.draw_image(None, contentVP.x, footerY, contentVP.width, footerHeight);
            cx.sg.set_draw_color(0.85, 0.85, 0.85);
            cx.sg.draw_image(None, contentVP.x, footerY, contentVP.width, 2.0);

            // Position spec/version in header row 1
            // L1917-1919
            set_xy(cx, leftSpecSelect, contentVP.x + 4.0, contentVP.y + 8.0);
            set_width(cx, leftSpecSelect, specWidth);

            // L1921
            let rightSpecX = contentVP.x + m_floor(contentVP.width / 2.0) + 4.0;
            // L1922-1924
            set_xy(cx, rightSpecSelect, rightSpecX, contentVP.y + 8.0);
            set_width(cx, rightSpecSelect, specWidth);

            // Overlay checkbox in header row 2
            // L1927-1928
            set_xy(cx, treeOverlayCheck, contentVP.x + LAYOUT::treeOverlayCheckX, contentVP.y + 34.0);

            // Overlay search in footer (full width)
            // L1931-1933
            let search = ctl(cx, me, "overlayTreeSearch");
            set_xy(cx, search, contentVP.x + 4.0, footerY + 4.0);
            set_width(cx, search, contentVP.width - 8.0);
        } else {
            // ========== SIDE-BY-SIDE MODE LAYOUT ==========
            // L1936
            let halfWidth = m_floor(contentVP.width / 2.0) - 2.0;
            // L1937
            let rightAbsX = contentVP.x + halfWidth + 4.0;
            // L1938
            let specWidth = m_min(m_floor(halfWidth * 0.55), 200.0);

            // L1940-1947
            lst_mut(cx, me).treeLayout = Some(TreeLayout {
                overlay: false,
                halfWidth: Some(halfWidth),
                headerHeight,
                footerHeight,
                footerY,
                rightAbsX: Some(rightAbsX),
            });

            // Header background + separator
            // L1950-1953
            cx.sg.set_draw_color(0.05, 0.05, 0.05);
            cx.sg.draw_image(None, contentVP.x, contentVP.y, contentVP.width, headerHeight);
            cx.sg.set_draw_color(0.85, 0.85, 0.85);
            cx.sg.draw_image(None, contentVP.x, contentVP.y + headerHeight - 2.0, contentVP.width, 2.0);

            // Footer backgrounds (two halves)
            // L1956-1961
            cx.sg.set_draw_color(0.05, 0.05, 0.05);
            cx.sg.draw_image(None, contentVP.x, footerY, halfWidth, footerHeight);
            cx.sg.draw_image(None, rightAbsX, footerY, halfWidth, footerHeight);
            cx.sg.set_draw_color(0.85, 0.85, 0.85);
            cx.sg.draw_image(None, contentVP.x, footerY, halfWidth, 2.0);
            cx.sg.draw_image(None, rightAbsX, footerY, halfWidth, 2.0);

            // Position spec/version in header row 1
            // L1964-1966
            set_xy(cx, leftSpecSelect, contentVP.x + 4.0, contentVP.y + 8.0);
            set_width(cx, leftSpecSelect, specWidth);

            // L1968-1970
            set_xy(
                cx,
                rightSpecSelect,
                contentVP.x + m_floor(contentVP.width / 2.0) + 4.0,
                contentVP.y + 8.0,
            );
            set_width(cx, rightSpecSelect, specWidth);

            // Overlay checkbox in header row 2
            // L1973-1974
            set_xy(cx, treeOverlayCheck, contentVP.x + LAYOUT::treeOverlayCheckX, contentVP.y + 34.0);

            // Position footer search fields
            // L1977-1979
            let la = ctl(cx, me, "leftFooterAnchor");
            set_xy(cx, la, contentVP.x + 4.0, footerY + 4.0);
            let ls = ctl(cx, me, "leftTreeSearch");
            set_width(cx, ls, halfWidth - 8.0);

            // L1981-1983
            let ra = ctl(cx, me, "rightFooterAnchor");
            set_xy(cx, ra, rightAbsX + 4.0, footerY + 4.0);
            let rs = ctl(cx, me, "rightTreeSearch");
            set_width(cx, rs, halfWidth - 8.0);
        }
        // L1985-1987
        set_y(cx, leftSpecSelect, contentVP.y + 22.0);
        set_y(cx, rightSpecSelect, contentVP.y + 22.0);
        set_y(cx, treeOverlayCheck, contentVP.y + 48.0);

        // (Common) Update spec dropdown lists
        let p = cx.build().model.clone();
        // L1990 (the primary build always has a treeTab)
        {
            // L1991
            let list: Vec<DropItem> = crate::tabs::tree::TreeTab::GetSpecList(cx)
                .into_iter()
                .map(DropItem::str)
                .collect();
            cx.w_mut::<DropDownControl>(leftSpecSelect).list = list;
            // L1992
            cx.w_mut::<DropDownControl>(leftSpecSelect).sel_index = sel(p.treeTab.activeSpec.get());
        }
        // L1994 (an entry always has a treeTab)
        {
            // L1995
            let list: Vec<DropItem> = entry::GetSpecList(compareEntry).into_iter().map(DropItem::str).collect();
            cx.w_mut::<DropDownControl>(rightSpecSelect).list = list;
            // L1996
            cx.w_mut::<DropDownControl>(rightSpecSelect).sel_index =
                sel(compareEntry.model.treeTab.activeSpec.get());
        }

        // (Common) Update version dropdown selection to match current spec
        let versions: Vec<Name> = st(cx, me)
            .treeVersionDropdownList
            .iter()
            .map(|v| v.get::<init::TreeVersionValue>().copied().unwrap_or_else(|| Name::from("")))
            .collect();
        // L2000 (`self.primaryBuild.spec` is always set)
        {
            let tv = p.spec.get().treeVersion.get();
            // L2001
            if let Some(i) = versions.iter().position(|v| *v == tv) {
                // L2003
                let id = ctl(cx, me, "leftVersionSelect");
                cx.w_mut::<DropDownControl>(id).sel_index = i + 1;
            }
        }
        // L2008
        {
            let tv = compareEntry.model.spec.get().treeVersion.get();
            // L2009
            if let Some(i) = versions.iter().position(|v| *v == tv) {
                // L2011
                let id = ctl(cx, me, "rightVersionSelect");
                cx.w_mut::<DropDownControl>(id).sel_index = i + 1;
            }
        }

        // (Common) Sync search fields when entering tree mode or changing compare entry
        // L2018
        if st(cx, me).treeSearchNeedsSync {
            // L2019
            st_mut(cx, me).treeSearchNeedsSync = false;
            // L2020
            if let Some(s) = primary_viewer_search(cx) {
                // L2021
                let id = ctl(cx, me, "leftTreeSearch");
                EditControl::set_text(cx, id, &s, false);
                // L2022
                let id = ctl(cx, me, "overlayTreeSearch");
                EditControl::set_text(cx, id, &s, false);
            }
            // L2024
            let s = entry::ext(compareEntry).viewer.borrow().search_str.clone();
            // L2025
            let id = ctl(cx, me, "rightTreeSearch");
            EditControl::set_text(cx, id, &s, false);
        }
    }

    /// lua: CompareTab.lua:2047-2225 `CompareTabClass:LayoutConfigView(contentVP, compareEntry)`
    /// Position config controls and build section-grouped display when in CONFIG view.
    /// @pob Classes/CompareTab.lua:2047-2225 h=4be4182a
    #[allow(clippy::too_many_lines)]
    pub fn LayoutConfigView(cx: &mut Cx, me: CtrlId, contentVP: &ViewPort, compareEntry: Option<&CompareEntryRef>) {
        // L2048
        let Some(compareEntry) = compareEntry.filter(|_| st(cx, me).compareViewMode == "CONFIG") else {
            return;
        };

        // Rebuild controls if compare entry changed or config was modified
        // L2051
        let s = st(cx, me);
        if s.configCompareId != Some(s.activeCompareIndex) || s.configNeedsRebuild {
            // L2052
            Self::RebuildConfigControls(cx, me, Some(compareEntry));
            let s = st_mut(cx, me);
            // L2053
            s.configCompareId = Some(s.activeCompareIndex);
            // L2054
            s.configNeedsRebuild = false;
        }

        // Sync control values with current input (in case changed from normal Config tab or externally)
        let p = cx.build().model.clone();
        // L2058-2059
        let pInput = p.configTab.input.clone();
        let cInput = compareEntry.model.configTab.input.clone();
        // L2060 (ORDER: module notes)
        let infos: Vec<(Name, Rc<ConfigCtrlInfo>)> =
            st(cx, me).configControls.iter().map(|(k, v)| (*k, v.clone())).collect();
        for (var, ctrlInfo) in infos {
            // L2061
            let varData = ctrlInfo.varData;
            // L2062
            syncControlValue(cx, ctrlInfo.primaryControl, varData, &pInput.get(var));
            // L2063
            syncControlValue(cx, ctrlInfo.compareControl, varData, &cInput.get(var));
        }

        // Position header controls
        // L2067-2073
        let row1Y = contentVP.y + 4.0;
        let row2Y = contentVP.y + 28.0;
        let configHeaderLeftX = contentVP.x + 10.0;
        let configSetSelectX = contentVP.x + 80.0;
        let configSetSelectW = 225.0;
        let inputEndX = configSetSelectX + configSetSelectW;
        let actionX = inputEndX + 10.0;

        // L2075-2078
        let copyConfigBtn = ctl(cx, me, "copyConfigBtn");
        set_xy(cx, copyConfigBtn, actionX, row1Y);
        let configToggleBtn = ctl(cx, me, "configToggleBtn");
        set_xy(cx, configToggleBtn, actionX, row2Y);

        // L2080-2082
        let configSearchEdit = ctl(cx, me, "configSearchEdit");
        set_x(cx, configSearchEdit, configHeaderLeftX);
        set_width(cx, configSearchEdit, inputEndX - configHeaderLeftX);
        set_y(cx, configSearchEdit, row2Y);

        // Update primary config set dropdown list
        // L2085
        let pConfigTab = &p.configTab;
        let configPrimarySetSelect = ctl(cx, me, "configPrimarySetSelect");
        // L2086
        let mut pSetList: Vec<DropItem> = Vec::new();
        // L2087
        let order: Vec<u32> = pConfigTab.configSetOrderList.snapshot();
        for (index, setId) in order.iter().enumerate() {
            // L2088
            let configSet = pConfigTab.configSets.with(|s| s.get(setId).cloned());
            // L2089
            let title = configSet
                .and_then(|c| c.str(pob_lua::n!("title")))
                .map_or_else(|| "Default".to_owned(), |t| t.to_string());
            pSetList.push(DropItem::str(title));
            // L2090
            if f64::from(*setId) == pConfigTab.activeConfigSetId.get() {
                // L2091
                cx.w_mut::<DropDownControl>(configPrimarySetSelect).sel_index = index + 1;
            }
        }
        // L2094
        DropDownControl::set_list(cx, configPrimarySetSelect, pSetList);
        // L2095-2096
        let configPrimarySetLabel = ctl(cx, me, "configPrimarySetLabel");
        set_xy(cx, configPrimarySetLabel, configHeaderLeftX, row1Y + 2.0);
        // L2097-2099
        set_x(cx, configPrimarySetSelect, configSetSelectX);
        set_width(cx, configPrimarySetSelect, configSetSelectW);
        set_y(cx, configPrimarySetSelect, row1Y);

        // Build section layout: multi-column grid, mirroring regular ConfigTab
        // L2102-2108
        let rowHeight = LAYOUT::configRowHeight;
        let sectionInnerPad = LAYOUT::configSectionInnerPad;
        let sectionGap = LAYOUT::configSectionGap;
        let fixedHeaderHeight = LAYOUT::configFixedHeaderHeight;
        let sectionWidth = LAYOUT::configSectionWidth;
        let columnStride = sectionWidth + LAYOUT::configSectionColumnGap;
        let scrollableH = contentVP.height - fixedHeaderHeight;

        // Hide ALL config controls first (selectively shown below)
        // L2111
        let list: Vec<Rc<ConfigCtrlInfo>> = st(cx, me).configControlList.clone();
        for ctrlInfo in &list {
            // L2112-2113
            cx.base_mut(ctrlInfo.primaryControl).shown = Prop::f(|_, _| false);
            cx.base_mut(ctrlInfo.compareControl).shown = Prop::f(|_, _| false);
        }

        // Search filter: match config labels against search text
        // L2117
        let buf = cx.w::<EditControl>(configSearchEdit).buf.to_ascii_lowercase();
        let searchStr = pob_lua::gsub(&buf, "[%-%.%+%[%]%$%^%%%?%*]", "%%%0", None).0;
        // L2118
        let hasSearch = pob_lua::pattern::find(&searchStr, "%S", None, false).is_some();
        // L2119-2123
        let searchMatch = |varData: &ConfigOptionDef| -> bool {
            // L2120
            if !hasSearch {
                return true;
            }
            // L2121
            let label = varData.label.as_deref().unwrap_or("").to_ascii_lowercase();
            // L2122
            p_call_match(&label, &searchStr)
        };

        // First pass: compute rows and height for each section
        // L2126
        let mut visibleSections: Vec<ConfigSectionLayout> = Vec::new();
        // L2127
        let sections = st(cx, me).configSections.clone();
        for section in &sections {
            // L2128-2129
            let mut diffs: Vec<Rc<ConfigCtrlInfo>> = Vec::new();
            let mut commons: Vec<Rc<ConfigCtrlInfo>> = Vec::new();
            // L2130
            let items: Vec<Rc<ConfigCtrlInfo>> = section.items.borrow().clone();
            for ctrlInfo in items {
                // L2131
                if searchMatch(ctrlInfo.varData) {
                    let var = ctrlInfo.varData.var.unwrap_or_else(|| lua_error!("table index is nil"));
                    // L2132-2133
                    let (pVal, cVal) =
                        Self::NormalizeConfigVals(ctrlInfo.varData, pInput.get(var), cInput.get(var));
                    // L2134
                    let isDiff = tostring_val(&pVal) != tostring_val(&cVal);
                    // L2135
                    if isDiff {
                        // L2136
                        diffs.push(ctrlInfo);
                    } else {
                        // L2138
                        let varData = ctrlInfo.varData;
                        // L2139-2140
                        let relevant = configVisibility::isRelevantForBuild(varData, Some(&p))
                            || configVisibility::isRelevantForBuild(varData, Some(&compareEntry.model));
                        // L2141
                        if relevant
                            || (st(cx, me).configToggle && !configVisibility::isShowAllExcluded(varData))
                        {
                            // L2142
                            commons.push(ctrlInfo);
                        }
                    }
                }
            }

            // L2148
            let mut rows: Vec<ConfigRow> = Vec::new();
            // L2149
            for ci in &diffs {
                rows.push(ConfigRow {
                    ctrlInfo: ci.clone(),
                    isDiff: true,
                });
            }
            // L2150
            for ci in commons {
                rows.push(ConfigRow {
                    ctrlInfo: ci,
                    isDiff: false,
                });
            }

            // L2152
            if !rows.is_empty() {
                // L2153
                let sectionHeight = sectionInnerPad + rows.len() as f64 * rowHeight + 8.0;
                // L2154-2160
                visibleSections.push(ConfigSectionLayout {
                    name: section.name.clone(),
                    col: section.col,
                    rows,
                    height: sectionHeight,
                    diffCount: diffs.len(),
                    x: 0.0,
                    y: 0.0,
                });
            }
        }

        // Second pass: multi-column placement (same algorithm as ConfigTab)
        // L2165
        let mut maxCol = m_floor((contentVP.width - 10.0) / columnStride);
        // L2166
        if maxCol < 1.0 {
            maxCol = 1.0;
        }
        // L2167
        let mut colY = ColY::default();
        colY.set(1.0, 0.0);
        // L2168
        let mut maxColY = 0.0;
        // L2169
        let mut sectionLayout: Vec<Rc<ConfigSectionLayout>> = Vec::new();

        // L2171
        for mut sec in visibleSections {
            // L2172
            let h = sec.height;
            // L2173
            let col: f64;
            // Try preferred column if it fits
            // L2175-2176
            let preferred = sec.col.filter(|&c| {
                colY.get(c).unwrap_or(0.0) + h + 28.0 <= scrollableH && 10.0 + c * columnStride <= contentVP.width
            });
            if let Some(c) = preferred {
                // L2177
                col = c;
            } else {
                // Find shortest column
                // L2180
                let mut best = 1.0;
                // L2181
                let mut c = 2.0;
                while c <= maxCol {
                    // L2182
                    let v = colY.get(c).unwrap_or(0.0);
                    colY.set(c, v);
                    // L2183
                    if v < colY.get(best).unwrap_or_else(|| lua_error!("attempt to compare nil with number")) {
                        // L2184
                        best = c;
                    }
                    c += 1.0;
                }
                col = best;
            }
            // L2188
            let cy = colY.get(col).unwrap_or(0.0);
            colY.set(col, cy);
            // L2189
            sec.x = 10.0 + (col - 1.0) * columnStride;
            // L2190
            sec.y = cy + sectionGap;
            // L2191
            colY.set(col, cy + h + sectionGap);
            // L2192
            maxColY = m_max(maxColY, cy + h + sectionGap);
            // L2193
            sectionLayout.push(Rc::new(sec));
        }

        // Third pass: position controls at absolute coords
        // L2197-2200
        let scrollTopAbs = contentVP.y + fixedHeaderHeight;
        let scrollBottomAbs = contentVP.y + contentVP.height;
        let ctrlH = rowHeight;
        let mouseClipRect = [contentVP.x, scrollTopAbs, contentVP.width, scrollBottomAbs - scrollTopAbs];
        let scrollY = st(cx, me).scrollY;
        // L2201
        for sec in &sectionLayout {
            // L2202
            let sectionAbsX = contentVP.x + sec.x;
            // L2203
            let mut rowY = sec.y + sectionInnerPad;
            // L2204
            for row in &sec.rows {
                // L2205
                let ci = &row.ctrlInfo;
                // L2206-2209
                set_xy(
                    cx,
                    ci.primaryControl,
                    sectionAbsX + LAYOUT::configCol2,
                    contentVP.y + fixedHeaderHeight + rowY - scrollY,
                );
                set_xy(
                    cx,
                    ci.compareControl,
                    sectionAbsX + LAYOUT::configCol3,
                    contentVP.y + fixedHeaderHeight + rowY - scrollY,
                );
                // L2210-2214
                let pc = ci.primaryControl;
                let shownFn = move |cx: &Cx, _: CtrlId| {
                    // L2211
                    let ay = cx.base(pc).y.raw();
                    // L2212-2213
                    ay + ctrlH > scrollTopAbs
                        && ay < scrollBottomAbs
                        && st(cx, me).compareViewMode == "CONFIG"
                        && Self::GetActiveCompare(cx, me).is_some()
                };
                // L2215-2216
                cx.base_mut(ci.primaryControl).shown = Prop::f(shownFn);
                cx.base_mut(ci.compareControl).shown = Prop::f(shownFn);
                // L2217-2218
                cx.base_mut(ci.primaryControl).mouse_clip_rect = Some(Prop::Val(Some(mouseClipRect)));
                cx.base_mut(ci.compareControl).mouse_clip_rect = Some(Prop::Val(Some(mouseClipRect)));
                // L2219
                rowY += rowHeight;
            }
        }

        // L2223
        lst_mut(cx, me).configSectionLayout = sectionLayout;
        // L2224
        st_mut(cx, me).configTotalContentHeight = maxColY + sectionGap;
    }

    /// lua: CompareTab.lua:2228-2259 `CompareTabClass:UpdateSetSelectors(compareEntry)`
    /// Update comparison build set selectors (spec, skill set, item set, skill controls).
    /// @pob Classes/CompareTab.lua:2228-2259 h=597621c1
    pub fn UpdateSetSelectors(cx: &mut Cx, me: CtrlId, compareEntry: &CompareEntryRef) {
        let m = &compareEntry.model;
        // Tree spec list (reuse GetSpecList from TreeTab)
        // L2230
        {
            let id = ctl(cx, me, "compareSpecSelect");
            // L2231
            let list: Vec<DropItem> = entry::GetSpecList(compareEntry).into_iter().map(DropItem::str).collect();
            cx.w_mut::<DropDownControl>(id).list = list;
            // L2232
            cx.w_mut::<DropDownControl>(id).sel_index = sel(m.treeTab.activeSpec.get());
        }
        // Skill set list
        // L2235-2236
        let id = ctl(cx, me, "compareSkillSetSelect");
        Self::PopulateSetDropdown(cx, SetTab::Skills(&m.skillsTab), id);
        // Item set list
        // L2239-2240
        let id = ctl(cx, me, "compareItemSetSelect");
        Self::PopulateSetDropdown(cx, SetTab::Items(&m.itemsTab), id);
        // Config set list
        // L2243-2244
        let id = ctl(cx, me, "compareConfigSetSelect");
        Self::PopulateSetDropdown(cx, SetTab::Config(&m.configTab), id);

        // Refresh comparison build skill selector controls
        // L2248-2257
        let cmpControls = SkillControls {
            mainSocketGroup: ctl(cx, me, "cmpSocketGroup"),
            mainSkill: ctl(cx, me, "cmpMainSkill"),
            mainSkillPart: ctl(cx, me, "cmpSkillPart"),
            mainSkillStageCount: ctl(cx, me, "cmpStageCount"),
            mainSkillMineCount: ctl(cx, me, "cmpMineCount"),
            mainSkillMinion: ctl(cx, me, "cmpMinion"),
            mainSkillMinionSkill: ctl(cx, me, "cmpMinionSkill"),
        };
        // L2258
        CompareEntry::RefreshSkillSelectControls(
            cx,
            compareEntry,
            &cmpControls,
            Val::Num(m.mainSocketGroup.get()),
            "",
        );
    }

    /// The `primControls` map host (module notes): `{ mainSocketGroup = ..., ...,
    /// mainSkillMinionLibrary = { shown = false } }`.
    fn prim_controls_map(cx: &mut Cx, me: CtrlId) -> CtrlId {
        let host = if let Some(h) = lst(cx, me).primControlsMap {
            h
        } else {
            let h = cx
                .add_free(Some(me), New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), ControlMap).host())
                .id();
            lst_mut(cx, me).primControlsMap = Some(h);
            h
        };
        // L2264-2273
        for (key, name) in [
            ("mainSocketGroup", "primCalcsSocketGroup"),
            ("mainSkill", "primCalcsMainSkill"),
            ("mainSkillPart", "primCalcsSkillPart"),
            ("mainSkillStageCount", "primCalcsStageCount"),
            ("mainSkillMineCount", "primCalcsMineCount"),
            ("mainSkillMinion", "primCalcsMinion"),
            ("mainSkillMinionSkill", "primCalcsMinionSkill"),
        ] {
            let id = ctl(cx, me, name);
            cx.put(host, key, id);
        }
        // mainSkillMinionLibrary = { shown = false } (Lua builds a fresh table each call; only
        // RefreshSkillSelectControls writes it and nothing reads it, so one placeholder serves)
        let lib = match cx.child(host, "mainSkillMinionLibrary") {
            Some(lib) => lib,
            None => cx
                .add(host, "mainSkillMinionLibrary", PlainControl::new(Anchor::NONE, rect(0, 0, 0, 0)))
                .id(),
        };
        cx.base_mut(lib).shown = Prop::Val(false);
        host
    }

    /// lua: CompareTab.lua:2262-2318 `CompareTabClass:RefreshCalcsSkillControls(compareEntry)`
    /// Refresh calcs skill detail controls for both builds.
    /// @pob Classes/CompareTab.lua:2262-2318 h=c4d4b48c
    pub fn RefreshCalcsSkillControls(cx: &mut Cx, me: CtrlId, compareEntry: &CompareEntryRef) {
        let p = cx.build().model.clone();
        let raw_shown_true = |cx: &Cx, id: CtrlId| matches!(cx.base(id).shown, Prop::Val(true));
        let buff_mode_sel = |cx: &mut Cx, id: CtrlId, v: &Val| {
            DropDownControl::sel_by_value(cx, id, &|item: &DropItem| {
                item.get::<BuffModeValue>()
                    .is_some_and(|b| Val::from(b.buffMode).raw_eq(v))
            });
        };
        // Build control maps for RefreshSkillSelectControls
        // L2264-2273
        let primControls = Self::prim_controls_map(cx, me);
        // L2274
        let skill_number = p.calcsTab.input.get(pob_lua::n!("skill_number")).opt_num().unwrap_or(f64::NAN);
        crate::build::skill_select::refresh_skill_select_controls(cx, primControls, skill_number, "Calcs");
        // L2275-2276
        let id = ctl(cx, me, "primCalcsSocketGroup");
        cx.base_mut(id).shown = Prop::Val(true);
        let mode = ctl(cx, me, "primCalcsMode");
        cx.base_mut(mode).shown = Prop::Val(true);
        // L2277
        buff_mode_sel(cx, mode, &p.calcsTab.input.get(pob_lua::n!("misc_buffMode")));
        // L2278
        let minion_shown = raw_shown_true(cx, ctl(cx, me, "primCalcsMinion"));
        let show_minion = ctl(cx, me, "primCalcsShowMinion");
        cx.base_mut(show_minion).shown = Prop::Val(minion_shown);
        // L2279
        cx.w_mut::<CheckBoxControl>(show_minion).state = p.calcsTab.input.truthy(pob_lua::n!("showMinion"));

        // L2281-2290
        let cmpControls = SkillControls {
            mainSocketGroup: ctl(cx, me, "cmpCalcsSocketGroup"),
            mainSkill: ctl(cx, me, "cmpCalcsMainSkill"),
            mainSkillPart: ctl(cx, me, "cmpCalcsSkillPart"),
            mainSkillStageCount: ctl(cx, me, "cmpCalcsStageCount"),
            mainSkillMineCount: ctl(cx, me, "cmpCalcsMineCount"),
            mainSkillMinion: ctl(cx, me, "cmpCalcsMinion"),
            mainSkillMinionSkill: ctl(cx, me, "cmpCalcsMinionSkill"),
        };
        let cm = &compareEntry.model;
        // L2291
        CompareEntry::RefreshSkillSelectControls(
            cx,
            compareEntry,
            &cmpControls,
            cm.calcsTab.input.get(pob_lua::n!("skill_number")),
            "Calcs",
        );
        // L2292-2293
        cx.base_mut(cmpControls.mainSocketGroup).shown = Prop::Val(true);
        let mode = ctl(cx, me, "cmpCalcsMode");
        cx.base_mut(mode).shown = Prop::Val(true);
        // L2294
        buff_mode_sel(cx, mode, &cm.calcsTab.input.get(pob_lua::n!("misc_buffMode")));
        // L2295
        let minion_shown = raw_shown_true(cx, cmpControls.mainSkillMinion);
        let show_minion = ctl(cx, me, "cmpCalcsShowMinion");
        cx.base_mut(show_minion).shown = Prop::Val(minion_shown);
        // L2296
        cx.w_mut::<CheckBoxControl>(show_minion).state = cm.calcsTab.input.truthy(pob_lua::n!("showMinion"));

        // Wrap .shown booleans set by RefreshSkillSelectControls with a view-mode gate,
        // so controls auto-hide when not in CALCS mode (matching configShown pattern)
        // L2300-2307
        let calcsControlNames = [
            "primCalcsSocketGroup", "primCalcsMainSkill", "primCalcsSkillPart",
            "primCalcsStageCount", "primCalcsMineCount", "primCalcsShowMinion", "primCalcsMinion",
            "primCalcsMinionSkill", "primCalcsMode",
            "cmpCalcsSocketGroup", "cmpCalcsMainSkill", "cmpCalcsSkillPart",
            "cmpCalcsStageCount", "cmpCalcsMineCount", "cmpCalcsShowMinion", "cmpCalcsMinion",
            "cmpCalcsMinionSkill", "cmpCalcsMode",
        ];
        // L2308
        for name in calcsControlNames {
            // L2309
            let ctrl = ctl(cx, me, name);
            // L2310-2311 (`if baseShown then`: a function is truthy)
            let baseShown = match &cx.base(ctrl).shown {
                Prop::Fn(_) => true,
                Prop::Val(v) => *v,
            };
            if baseShown {
                // L2312-2315 (module notes: the third operand is always truthy)
                cx.base_mut(ctrl).shown = Prop::f(move |cx, _| {
                    st(cx, me).compareViewMode == "CALCS" && Self::GetActiveCompare(cx, me).is_some()
                });
            }
        }
    }

    /// lua: CompareTab.lua:2321-2409 `CompareTabClass:LayoutCalcsSkillControls(vp, compareEntry)`
    /// Layout calcs skill detail controls into a two-column header area
    /// @pob Classes/CompareTab.lua:2321-2409 h=2d7c0a9d
    #[allow(clippy::too_many_lines)]
    pub fn LayoutCalcsSkillControls(cx: &mut Cx, me: CtrlId, vp: &ViewPort, compareEntry: Option<&CompareEntryRef>) -> f64 {
        // L2322
        let Some(compareEntry) = compareEntry.filter(|_| st(cx, me).compareViewMode == "CALCS") else {
            return 0.0;
        };

        // L2324
        Self::RefreshCalcsSkillControls(cx, me, compareEntry);

        // L2326-2332
        let colWidth = m_floor((vp.width - 20.0) / 2.0);
        let leftX = vp.x + 4.0;
        let rightX = leftX + colWidth + 12.0;
        let labelW = 120.0;
        let controlW = colWidth - labelW - 8.0;
        let rowH = 22.0;
        let y = vp.y + 4.0;

        // Helper to position a row of label + control
        // L2335-2345
        let layoutRow = |cx: &mut Cx, control: CtrlId, x: f64, currentY: f64, width: Option<f64>| -> bool {
            // L2336
            let hidden = match &cx.base(control).shown {
                Prop::Val(v) => !*v,
                Prop::Fn(_) => !cx.is_shown(control),
            };
            if hidden {
                // L2337
                return false;
            }
            // L2339-2340
            set_xy(cx, control, x + labelW, currentY);
            // L2341 (`control.width` is a number)
            // L2342
            let w = cx.base(control).width.raw();
            set_width(cx, control, m_min(width.unwrap_or(controlW), w));
            // L2344
            true
        };

        // Track max rows for both columns
        // L2348-2349
        let mut leftY = y;
        let mut rightY = y;

        // Title row
        // L2352-2353
        leftY += rowH;
        rightY += rowH;

        // { suffix, useControlW, alwaysAdvance }
        // L2356-2366
        // (`"primCalcs" .. suffix` / `"cmpCalcs" .. suffix` spelled out)
        let calcsRows: [(&'static str, &'static str, bool, bool); 9] = [
            ("primCalcsSocketGroup", "cmpCalcsSocketGroup", true, true),
            ("primCalcsMainSkill", "cmpCalcsMainSkill", true, false),
            ("primCalcsSkillPart", "cmpCalcsSkillPart", true, false),
            ("primCalcsStageCount", "cmpCalcsStageCount", false, false),
            ("primCalcsMineCount", "cmpCalcsMineCount", false, false),
            ("primCalcsShowMinion", "cmpCalcsShowMinion", false, false),
            ("primCalcsMinion", "cmpCalcsMinion", true, false),
            ("primCalcsMinionSkill", "cmpCalcsMinionSkill", true, false),
            ("primCalcsMode", "cmpCalcsMode", false, true),
        ];
        // L2367
        for (primName, cmpName, useControlW, alwaysAdvance) in calcsRows {
            // L2369
            let width = if useControlW { Some(controlW) } else { None };
            // L2370-2371
            let prim = ctl(cx, me, primName);
            let primShown = layoutRow(cx, prim, leftX, leftY, width);
            let cmp = ctl(cx, me, cmpName);
            let cmpShown = layoutRow(cx, cmp, rightX, rightY, width);
            // L2372
            if primShown || alwaysAdvance {
                leftY += rowH;
            }
            // L2373
            if cmpShown || alwaysAdvance {
                rightY += rowH;
            }
        }

        // Account for text info lines (Aura/Buffs, Combat Buffs, Curses) + separator
        // L2377 padding before text
        let mut textLinesHeight = 2.0;
        // L2378-2381
        let primaryOutput = cx.build().model.calcsTab.calcsEnv.get().map(|e| e.player.output.get());
        let compareOutput = compareEntry.model.calcsTab.calcsEnv.get().map(|e| e.player.output.get());
        // L2382
        if primaryOutput.is_some() || compareOutput.is_some() {
            // L2383
            let wrapWidth = colWidth - 8.0;
            // L2384-2389
            let infoLabel = |key: &str| match key {
                "BuffList" => "Aura/Buff Skills",
                "CombatList" => "Combat Buffs",
                _ => "Curses/Debuffs",
            };
            let infoKeys = ["BuffList", "CombatList", "CurseList"];
            // L2390
            for key in infoKeys {
                // L2391-2392
                let primaryValue = primaryOutput.as_ref().map_or(Val::Nil, |o| o.get(Name::from(key)));
                let compareValue = compareOutput.as_ref().map_or(Val::Nil, |o| o.get(Name::from(key)));
                let nonEmpty = |v: &Val| v.truthy() && !matches!(v, Val::Str(s) if s.is_empty());
                // L2393
                if nonEmpty(&primaryValue) || nonEmpty(&compareValue) {
                    // L2394
                    let label = infoLabel(key);
                    let concat = |v: &Val| match v {
                        Val::Str(s) => s.to_string(),
                        Val::Num(n) => pob_lua::tostring_num(*n),
                        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
                    };
                    // L2395
                    let primaryLineCount = if nonEmpty(&primaryValue) {
                        let s = format!("{label}: {}", concat(&primaryValue));
                        wrapInfoLine(cx, Some(&s), wrapWidth).len() as f64
                    } else {
                        0.0
                    };
                    // L2396
                    let compareLineCount = if nonEmpty(&compareValue) {
                        let s = format!("{label}: {}", concat(&compareValue));
                        wrapInfoLine(cx, Some(&s), wrapWidth).len() as f64
                    } else {
                        0.0
                    };
                    // L2397
                    textLinesHeight += m_max(m_max(primaryLineCount, compareLineCount), 1.0) * 18.0;
                }
            }
        }

        // L2402
        let textBaseY = m_max(leftY, rightY);
        // L2403 + separator padding
        let headerHeight = textBaseY - vp.y + textLinesHeight + 4.0;
        // L2404
        let showOnlyDiffCheck = ctl(cx, me, "calcsShowOnlyDifferencesCheck");
        // L2405
        let state = st(cx, me).calcsShowOnlyDifferences;
        cx.w_mut::<CheckBoxControl>(showOnlyDiffCheck).state = state;
        // L2406-2407
        let lw = label_width(cx, showOnlyDiffCheck);
        set_xy(cx, showOnlyDiffCheck, leftX + lw + 2.0, vp.y + headerHeight + 4.0);
        // L2408
        headerHeight
    }

    /// lua: CompareTab.lua:2412-2461 `CompareTabClass:HandleScrollInput(contentVP, inputEvents)`
    /// Handle scroll events for scrollable views.
    /// @pob Classes/CompareTab.lua:2412-2461 h=3c35da1d
    pub fn HandleScrollInput(cx: &mut Cx, me: CtrlId, contentVP: &ViewPort, inputEvents: &mut InputEvents) {
        // L2413
        let (cursorX, cursorY) = cx.sys.cursor_pos();
        // L2414-2415
        let mouseInContent = cursorX >= contentVP.x
            && cursorX < contentVP.x + contentVP.width
            && cursorY >= contentVP.y
            && cursorY < contentVP.y + contentVP.height;

        // L2417
        let listControl = ctl(cx, me, "comparePowerReportList");
        // L2418
        let mouseOverList = cx.is_shown(listControl) && cx.is_mouse_over(listControl);
        // L2419
        let viewScrollBar = ctl(cx, me, "viewScrollBar");
        let mouseOverViewScrollBar = cx.is_shown(viewScrollBar) && cx.is_mouse_over(viewScrollBar);

        // L2421
        let mut id = 0;
        while let Some(event) = inputEvents.next_ipairs(&mut id) {
            // L2422
            if event.kind == EventKind::KeyUp && mouseInContent && !mouseOverList {
                let mode = st(cx, me).compareViewMode;
                // L2423
                if mode == "CALCS" {
                    let calcsScrollBar = ctl(cx, me, "calcsScrollBar");
                    // L2424
                    if event.key == Key::WheelUp {
                        // L2425
                        ScrollBarControl::scroll(cx, calcsScrollBar, -1.0);
                        // L2426
                        inputEvents.consume(id);
                    } else if event.key == Key::WheelDown {
                        // L2428
                        ScrollBarControl::scroll(cx, calcsScrollBar, 1.0);
                        // L2429
                        inputEvents.consume(id);
                    }
                // L2431
                } else if event.key == Key::WheelUp && mode != "TREE" && !mouseOverViewScrollBar {
                    // L2432
                    let s = st_mut(cx, me);
                    s.scrollY = m_max(s.scrollY - 40.0, 0.0);
                    // L2433
                    let y = s.scrollY;
                    ScrollBarControl::set_offset(cx, viewScrollBar, y);
                    // L2434
                    st_mut(cx, me).scrollY = cx.w::<ScrollBarControl>(viewScrollBar).offset;
                    // L2435
                    inputEvents.consume(id);
                // L2436
                } else if event.key == Key::WheelDown && mode != "TREE" && !mouseOverViewScrollBar {
                    // L2437
                    let mut maxScroll = 0.0;
                    // L2438
                    let viewportH = contentVP.height;
                    let s = st(cx, me);
                    // L2439 (`self.configTotalContentHeight` is always a number)
                    if mode == "CONFIG" {
                        // L2440
                        let scrollViewH = viewportH - LAYOUT::configFixedHeaderHeight;
                        // L2441
                        maxScroll = m_max(s.configTotalContentHeight - scrollViewH, 0.0);
                    // L2442
                    } else if mode == "SUMMARY" && s.summaryTotalContentHeight > 0.0 {
                        // L2443
                        let scrollViewH = viewportH - LAYOUT::summaryHeaderHeight;
                        // L2444
                        maxScroll = m_max(s.summaryTotalContentHeight - scrollViewH, 0.0);
                    // L2445
                    } else if mode == "ITEMS" && s.itemsTotalContentHeight > 0.0 {
                        let total = s.itemsTotalContentHeight;
                        // L2446
                        let hbar = ctl(cx, me, "itemsHScrollBar");
                        let hBarReserve = if ScrollBarControl::enabled_raw(cx, hbar) {
                            LAYOUT::itemsHScrollBarHeight
                        } else {
                            0.0
                        };
                        // L2447
                        let scrollViewH = viewportH - LAYOUT::itemsCheckboxOffset - hBarReserve;
                        // L2448
                        maxScroll = m_max(total - scrollViewH, 0.0);
                    // L2449
                    } else if mode == "SKILLS" && s.skillsTotalContentHeight > 0.0 {
                        let total = s.skillsTotalContentHeight;
                        // L2450
                        let hbar = ctl(cx, me, "skillsHScrollBar");
                        let hBarReserve = if ScrollBarControl::enabled_raw(cx, hbar) {
                            LAYOUT::skillsHScrollBarHeight
                        } else {
                            0.0
                        };
                        // L2451
                        let scrollViewH = viewportH - LAYOUT::skillsHeaderHeight - hBarReserve;
                        // L2452
                        maxScroll = m_max(total - scrollViewH, 0.0);
                    }
                    // L2454
                    let s = st_mut(cx, me);
                    s.scrollY = m_min(s.scrollY + 40.0, maxScroll);
                    // L2455
                    let y = s.scrollY;
                    ScrollBarControl::set_offset(cx, viewScrollBar, y);
                    // L2456
                    st_mut(cx, me).scrollY = cx.w::<ScrollBarControl>(viewScrollBar).offset;
                    // L2457
                    inputEvents.consume(id);
                }
            }
        }
    }

    // ---- COMPARE POWER REPORT helpers (U48 calls them) ----

    /// lua: CompareTab.lua:2468-2473 `CompareTabClass:GetGemGrantedEffect(gem)`
    /// Resolve the granted effect for a gem instance
    /// @pob Classes/CompareTab.lua:2468-2473 h=d13b9a2c
    pub fn GetGemGrantedEffect(gem: &model::GemInstance) -> Option<model::gamedata::GrantedEffectRef> {
        // L2469
        if let Some(gemData) = gem.gemData.get() {
            // L2470
            return Some(gemData.grantedEffect.clone());
        }
        // L2472
        gem.grantedEffect.get()
    }

    /// lua: CompareTab.lua:2476-2486 `CompareTabClass:GetSocketGroupSignature(group)`
    /// Build a signature string for a socket group (sorted gem names)
    /// @pob Classes/CompareTab.lua:2476-2486 h=229568be
    pub fn GetSocketGroupSignature(group: &model::SocketGroup) -> String {
        // L2477
        let mut names: Vec<String> = Vec::new();
        // L2478
        for gem in group.gemList.get().snapshot() {
            // L2479-2482 (`gem.nameSpec` is always a string)
            let name = gem
                .grantedEffect
                .get()
                .map_or_else(|| gem.nameSpec.get().to_string(), |g| g.name.to_string());
            names.push(name);
        }
        // L2484 (byte order, as Lua string comparison)
        names.sort();
        // L2485
        names.join("+")
    }

    /// lua: CompareTab.lua:2489-2513 `CompareTabClass:GetSocketGroupLabel(group)`
    /// Get a display label for a socket group (active skills only)
    /// @pob Classes/CompareTab.lua:2489-2513 h=51aa1edb
    pub fn GetSocketGroupLabel(group: &model::SocketGroup) -> String {
        let gems = group.gemList.get().snapshot();
        let name_of = |gem: &model::GemInstanceRef| {
            gem.grantedEffect
                .get()
                .map_or_else(|| gem.nameSpec.get().to_string(), |g| g.name.to_string())
        };
        // L2490
        let mut names: Vec<String> = Vec::new();
        // L2491
        for gem in &gems {
            // L2492
            let isSupport = gem.grantedEffect.get().is_some_and(|g| g.support);
            // L2493
            if !isSupport {
                // L2494-2497
                names.push(name_of(gem));
            }
        }
        // L2500
        if names.is_empty() {
            // Fallback: show all gem names if no active skills found
            // L2502
            for gem in &gems {
                // L2503-2506
                names.push(name_of(gem));
            }
        }
        // L2509
        if names.is_empty() {
            // L2510
            return "(empty group)".to_owned();
        }
        // L2512
        names.join(" + ")
    }
}

/// lua: CompareTab.lua:2031-2044 `local function syncControlValue(ctrl, varData, val)`
/// Sync a single control's displayed value with the actual input value
/// @pob Classes/CompareTab.lua:2031-2044 h=68cd961b
fn syncControlValue(cx: &mut Cx, ctrl: CtrlId, varData: &ConfigOptionDef, val: &Val) {
    let ty = varData.r#type.as_deref();
    // L2032
    if ty == Some("check") {
        // L2033
        cx.w_mut::<CheckBoxControl>(ctrl).state = val.truthy();
    // L2034-2035
    } else if matches!(ty, Some("count" | "integer" | "countAllowZero" | "float")) {
        // L2036
        let text = tostring_val(&if val.truthy() { val.clone() } else { Val::from("") });
        // avoid setting text every time as otherwise this clears user selections on every frame
        // L2038
        if cx.base(ctrl).has_focus != Some(true) && text != cx.w::<EditControl>(ctrl).buf {
            // L2039
            EditControl::set_text(cx, ctrl, &text, false);
        }
    // L2041
    } else if ty == Some("list") {
        // L2042
        let want = if val.truthy() {
            val.clone()
        } else {
            varData
                .list
                .as_ref()
                .and_then(|l| l.first())
                .map_or(Val::Nil, |i| Val::from_value(&i.val))
        };
        DropDownControl::sel_by_value(cx, ctrl, &|item: &DropItem| {
            item.get::<Val>().is_some_and(|v| v.raw_eq(&want))
        });
    }
}
