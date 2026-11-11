//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:1160-1352 (OnFrame).
//! Owner: U20. Also SetDisplayStat / ClearDisplayStat (Build.lua:1353-1391, U20's range) and
//! `tableDeepEquals` (Common.lua:625-640, the one caller in U20's range).
//!
//! @pob-file Modules/Build.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! OnFrame calls into units that are not ported yet: the tabs' `Draw` and mod flags, the
//! ImportTab code box (U43), the CalcsTab breakdown (U39/U40), the sidebar CalcBreakdownControl
//! (U40), `GetSidebarBreakdown` / `RefreshSkillSelectControls` (U21), the Spectre library lists
//! (U31) and the similar-builds list (U52). They go through [`ShellHooks`] (CCR-0104 §1), the
//! same pattern as U19's `init::BuildHooks`; each default names its owner. A tab that is not
//! ported has no node, so its default `Draw` draws nothing and its mod flags read `false`.
//!
//! The recalc (Build.lua:1258-1272, ui doc §10.1) runs through the calc facade
//! (`calc.build_output`, `calc.misc_calculator`); on desktop both are always ready. A `Pending`
//! (wasm only, calc doc D15) skips the statements that read the result in this frame.
//!
//! `self.lastShowThousandsSeparators`, `lastShowThousandsSeparator`, `lastUseCompactValues`,
//! `lastShowDecimalSeparator` and `lastShowTitlebarName` are written by `buildMode:FormatStat`
//! (Build.lua:1749-1753, U21's range) on every formatted stat, so the five comparisons at
//! L1274-1288 only fire after a display setting changes (or before the first stat is formatted).
//! They live in [`State`] (`BuildMode::on_frame`, all `pub`); U21's FormatStat writes them
//! (CCR-0104 §2). Until U21 lands nothing formats a stat, the fields stay nil and the comparisons
//! fire every frame, on both sides of the oracle test (its Lua side stubs RefreshStatList too).
//!
//! Overlay panes (`self.overlayPanes`, Build.lua:626; CalcSectionControl's ToggleOverlay /
//! RaiseOverlay, U39) are [`OverlayPane`] trait objects; the loops read `overlayPanes[i]` live,
//! as `ipairs` and the numeric `for` over `#overlayPanes` do in Lua.
//!
//! DIVERGENCE(init-closed): when U19's Init returned before creating the controls (a section
//! load closed the build, `init.rs` DIVERGENCE(init-order)), PoB still runs one OnFrame on the
//! controls it created first; here OnFrame stops after the `abortSave` close.

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::n;
use pob_ui_core::controls::text_list::{HoveredLine, TextListControl};
use pob_ui_core::controls::{DropDownControl, DropItem, ScrollBarControl};
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::*;

use super::init::{self, AscendListValue, ClassListValue};
use super::loadouts::ActValue;
use super::{class_drop, io, BuildMode};
use crate::app::CxApp;
use crate::model;

/// The build tabs (`self.importTab`, ...), for the tab hooks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BuildTab {
    Import,
    Notes,
    Party,
    Config,
    Tree,
    Skills,
    Items,
    Calcs,
    Compare,
}

/// The flags OnFrame (L1327) and ResetModFlags (L1100-1111) read and clear on the tabs and the spec.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TabFlag {
    /// `self.notesTab.modFlag`
    Notes,
    /// `self.partyTab.modFlag`
    Party,
    /// `self.configTab.modFlag`
    Config,
    /// `self.treeTab.modFlag`
    Tree,
    /// `self.treeTab.searchFlag`
    TreeSearch,
    /// `self.spec.modFlag` (UndoHandler.lua:37, the spec's undo, U25)
    Spec,
    /// `self.skillsTab.modFlag`
    Skills,
    /// `self.itemsTab.modFlag`
    Items,
    /// `self.calcsTab.modFlag`
    Calcs,
}

/// A pinned calcs pane (`self.overlayPanes[i]`, a CalcSectionControl with `isOverlay`; U39).
pub trait OverlayPane {
    /// `pane.isOverlay`
    fn is_overlay(&self, cx: &Cx) -> bool;
    /// `pane:IsMouseInOverlay(cursorX, cursorY)`
    fn is_mouse_in_overlay(&self, cx: &Cx, x: f64, y: f64) -> bool;
    /// `pane:HandleOverlayClick(key, cursorX, cursorY)`
    fn handle_overlay_click(&self, cx: &mut Cx, key: Key, x: f64, y: f64);
    /// `pane:HandleOverlayRelease(key)`
    fn handle_overlay_release(&self, cx: &mut Cx, key: Key);
    /// `pane:DrawOverlay(viewPort, inputEvents)`
    fn draw_overlay(&self, cx: &mut Cx, vp: &ViewPort, input_events: &mut InputEvents);
}

/// The fields of a stat-box line (`hovered.line`) that SetDisplayStat reads: `breakdown`,
/// `modNames`, `ignoredSections`, `actorName`, `breakdownColour`. The stat list (U21) stores
/// one in each line's `TextListRow::extra` (CCR-0104 §2).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct StatLineInfo {
    /// `line.breakdown` (a breakdown key)
    pub breakdown: Option<String>,
    /// `line.modNames`
    pub mod_names: Option<Vec<String>>,
    /// `line.ignoredSections` (`{ [label] = true }`)
    pub ignored_sections: Option<Vec<String>>,
    /// `line.actorName`
    pub actor_name: Option<String>,
    /// `line.breakdownColour`
    pub breakdown_colour: Option<[f64; 3]>,
}

/// `{ self.sidebarBreakdownData, pin, hovered.line.actorName }` (L1378).
#[derive(Clone, Debug)]
pub struct BreakdownInputs {
    pub data: model::Table,
    pub pin: bool,
    pub actor_name: Option<String>,
}

/// `self.calcsTab.controls.breakdown` and whether it is an overlay breakdown (L1213-1215).
#[derive(Clone, Copy, Debug, Default)]
pub struct CalcsBreakdown {
    /// `self.calcsTab.controls.breakdown` (`None`: CalcsTab not ported, U39/U40)
    pub breakdown: Option<CtrlId>,
    /// `self.calcsTab.displayPinned and self.calcsTab.displayData and
    /// self.calcsTab.displayData.calcSection and self.calcsTab.displayData.calcSection.isOverlay`
    pub overlay_breakdown: bool,
}

/// U20's part of the buildMode object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.overlayPanes` (Build.lua:626; Init's fresh node starts empty)
    pub overlay_panes: Vec<Rc<dyn OverlayPane>>,
    /// `self.overlayBreakdownControl`
    pub overlay_breakdown_control: Option<CtrlId>,
    /// `self.breakdownInputs`
    pub breakdown_inputs: Option<BreakdownInputs>,
    /// `self.sidebarBreakdownData`
    pub sidebar_breakdown_data: Option<model::Table>,
    /// `self.actionOnSave` (`"LIST"`, `"EXIT"`, `"UPDATE"`)
    pub action_on_save: Option<String>,
    /// `self.Act` (EstimatePlayerProgress)
    pub act: Option<ActValue>,
    /// `self.secondaryAscendDropEntryCount` (UpdateSecondaryAscendancyDropdown)
    pub secondary_ascend_drop_entry_count: Option<f64>,
    /// `self.secondaryAscendDropAltAscendancies` (compared by identity)
    pub secondary_ascend_drop_alt_ascendancies: Option<model::Table>,
    /// `self.secondaryAscendDropLegacySelection`
    pub secondary_ascend_drop_legacy_selection: Option<String>,
    /// `self.lastShowThousandsSeparators` &c.: written by U21's `FormatStat` (Build.lua:1749-1753;
    /// CCR-0104 §2), compared by OnFrame (L1274-1288); see the module doc
    pub last_show_thousands_separators: Option<bool>,
    pub last_show_thousands_separator: Option<String>,
    pub last_use_compact_values: Option<bool>,
    pub last_show_decimal_separator: Option<String>,
    pub last_show_titlebar_name: Option<bool>,
}

// ---- seams to the units U20's code calls (CCR-0104 §1) ---------------------------------------

/// The methods U20's code calls that other units port. Each default names its owner; owners
/// replace their entry in [`default_shell_hooks`] (one line each).
#[derive(Clone)]
pub struct ShellHooks {
    // the tabs (U22 Notes, U25 Tree, U30 Skills, U34 Items, U39 Calcs, U41 Config, U43 Import,
    // U45 Party, U46 Compare)
    /// `self.<tab>:Draw(tabViewPort, inputEvents)` (L1300-1318)
    pub draw_tab: Rc<dyn Fn(&mut Cx, BuildTab, &ViewPort, &mut InputEvents)>,
    /// `self.<tab>.modFlag` / `self.treeTab.searchFlag` / `self.spec.modFlag`
    pub tab_flag: Rc<dyn Fn(&Cx, TabFlag) -> bool>,
    /// `self.<tab>.modFlag = v` (ResetModFlags)
    pub set_tab_flag: Rc<dyn Fn(&mut Cx, TabFlag, bool)>,
    /// `self.importTab:SelectControl(self.importTab.controls.importCodeIn)` (L1182, U43)
    pub import_select_code_in: Rc<dyn Fn(&mut Cx)>,
    /// `self.calcsTab.controls.breakdown` and the overlay condition (L1213-1215, U39/U40)
    pub calcs_breakdown: Rc<dyn Fn(&Cx) -> CalcsBreakdown>,
    // the sidebar CalcBreakdownControl `self.controls.breakdown` (U40)
    /// `self.controls.breakdown.pinned`
    pub breakdown_pinned: Rc<dyn Fn(&Cx, CtrlId) -> bool>,
    /// `self.controls.breakdown:SetBreakdownData(displayData, pinned, forceActor)` (`None`: no
    /// arguments)
    pub breakdown_set_data: Rc<dyn Fn(&mut Cx, CtrlId, Option<&BreakdownInputs>)>,
    /// `self.controls.breakdown.borderColour = colour`
    pub breakdown_set_border_colour: Rc<dyn Fn(&mut Cx, CtrlId, [f64; 3])>,
    // U21 (Build.lua:1618-2221)
    /// `self:GetSidebarBreakdown(key, modNames, ignoredSections, actorName)`
    pub get_sidebar_breakdown: Rc<dyn Fn(&mut Cx, &StatLineInfo) -> model::Table>,
    /// `self:RefreshSkillSelectControls(self.controls, self.mainSocketGroup, "")`
    pub refresh_skill_select_controls: Rc<dyn Fn(&mut Cx, CtrlId, f64, &str)>,
    // U31 (MinionListControl, MinionSearchListControl)
    /// `controls.list = new("MinionListControl"):MinionListControl(nil, {-139, 40, 265, 250},
    /// self.data, destList)` in the Spectre library popup `p`; returns the control
    pub minion_list_control: Rc<dyn Fn(&mut Cx, CtrlId, Rc<RefCell<Vec<model::Name>>>) -> CtrlId>,
    /// `controls.source = new("MinionSearchListControl"):MinionSearchListControl(nil, {139, 60,
    /// 265, 230}, self.data, sourceList, controls.list)`; returns the control
    pub minion_search_list_control: Rc<dyn Fn(&mut Cx, CtrlId, Vec<model::Name>, CtrlId) -> CtrlId>,
    // U52 (ExtBuildListControl, PoBArchivesProvider) and U43 (build codes)
    /// `controls.similarBuildList = new("ExtBuildListControl"):ExtBuildListControl(nil, {0,
    /// padding, width, height}, buildProviders)` with its `shown`/`height`/`width` overrides,
    /// `SetImportCode(code)` and `Init("PoB Archives")` (L1586-1602) in the popup `p`. The
    /// third argument is `self:SaveDB("code")` (L1598): the hook calls it after creating the
    /// control, where the Lua does, and encodes the XML it returns
    pub similar_build_list: Rc<dyn Fn(&mut Cx, CtrlId, &dyn Fn(&mut Cx) -> String)>,
    // the UI-only savers (Build.lua:629-639: `Notes` U22, `TreeView` U25-U28, `Import` U43)
    /// `saver:Save(node)` of a UI-only saver (`elem` is the section name): `Some(node)` replaces
    /// the section calc:BM-A's SaveDB wrote (the section it loaded, CCR-0080 §8); `None` keeps it
    /// (`&mut Cx`: a saver's `Save` may change its tab, e.g. NotesTab:Save's
    /// `SetShowColorCodes(false)`, NotesTab.lua:85; CCR-0106 §1)
    pub save_ui_section: Rc<dyn Fn(&mut Cx, &str) -> Option<model::XmlNode>>,
}

fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The placeholders installed until the owners land (CCR-0104 §1). OnFrame calls the tab
/// draws, the flags, the CalcsTab breakdown, the sidebar breakdown and
/// RefreshSkillSelectControls every frame, so those read as "no tab / nothing pinned" and draw
/// nothing; the user-triggered ones raise a Lua error naming the owner.
#[allow(clippy::single_match)] // the tab hooks get one arm per ported tab (CCR-0106 §2)
pub fn default_shell_hooks() -> ShellHooks {
    ShellHooks {
        // one arm per ported tab (U22 Notes); the others draw nothing / read false
        draw_tab: Rc::new(|cx, tab, vp, ev| match tab {
            BuildTab::Notes => crate::tabs::notes::draw_tab(cx, vp, ev),
            BuildTab::Party => crate::tabs::party::draw_tab(cx, vp, ev),
            BuildTab::Calcs => crate::tabs::calcs::core::draw_tab(cx, vp, ev),
            BuildTab::Skills => crate::tabs::skills::core::draw_tab(cx, vp, ev),
            BuildTab::Import => crate::tabs::import::core::draw_tab(cx, vp, ev),
            BuildTab::Config => crate::tabs::config::core::draw_tab(cx, vp, ev),
            BuildTab::Items => crate::tabs::items::core::draw_tab(cx, vp, ev),
            _ => {}
        }),
        tab_flag: Rc::new(|cx, flag| match flag {
            TabFlag::Notes => crate::tabs::notes::mod_flag(cx),
            TabFlag::Party => crate::tabs::party::mod_flag(cx),
            TabFlag::Calcs => crate::tabs::calcs::core::mod_flag(cx),
            TabFlag::Skills => crate::tabs::skills::core::mod_flag(cx),
            TabFlag::Config => crate::tabs::config::core::mod_flag(cx),
            TabFlag::Items => crate::tabs::items::tooltip::mod_flag(cx),
            _ => false,
        }),
        set_tab_flag: Rc::new(|cx, flag, v| match flag {
            TabFlag::Notes => crate::tabs::notes::set_mod_flag(cx, v),
            TabFlag::Party => crate::tabs::party::set_mod_flag(cx, v),
            TabFlag::Calcs => crate::tabs::calcs::core::set_mod_flag(cx, v),
            TabFlag::Skills => crate::tabs::skills::core::set_mod_flag(cx, v),
            TabFlag::Config => crate::tabs::config::core::set_mod_flag(cx, v),
            TabFlag::Items => crate::tabs::items::tooltip::set_mod_flag(cx, v),
            _ => {}
        }),
        import_select_code_in: Rc::new(crate::tabs::import::core::select_code_in),
        calcs_breakdown: Rc::new(crate::tabs::calcs::core::calcs_breakdown),
        // U40 (the sidebar CalcBreakdownControl)
        breakdown_pinned: Rc::new(crate::tabs::calcs::breakdown::sidebar_breakdown_pinned),
        breakdown_set_data: Rc::new(crate::tabs::calcs::breakdown::sidebar_breakdown_set_data),
        breakdown_set_border_colour: Rc::new(
            crate::tabs::calcs::breakdown::sidebar_breakdown_set_border_colour,
        ),
        // U21's (CCR-0106 §4: U20 and U21 merged without wiring these two)
        get_sidebar_breakdown: Rc::new(|cx, line| {
            let mod_names: Option<Vec<&str>> = line
                .mod_names
                .as_ref()
                .map(|v| v.iter().map(String::as_str).collect());
            super::stats::get_sidebar_breakdown(
                cx,
                line.breakdown.as_deref(),
                mod_names.as_deref(),
                line.ignored_sections.as_deref(),
                line.actor_name.as_deref(),
            )
        }),
        refresh_skill_select_controls: Rc::new(super::skill_select::refresh_skill_select_controls),
        minion_list_control: Rc::new(crate::tabs::skills::minion_list::spectre_library_list),
        minion_search_list_control: Rc::new(
            crate::tabs::skills::minion_search_list::spectre_library_source,
        ),
        similar_build_list: Rc::new(|_, _, _| not_ported("ExtBuildListControl", "U52")),
        // one arm per UI-only saver (U22 Notes); calc:BM-A writes the other loaded sections
        // back (CCR-0080 §8)
        save_ui_section: Rc::new(|cx, elem| match elem {
            "Notes" => crate::tabs::notes::save_ui_section(cx),
            "Import" => crate::tabs::import::core::save_ui_section(cx),
            _ => None,
        }),
    }
}

thread_local! {
    static SHELL_HOOKS: RefCell<Option<ShellHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn shell_hooks() -> ShellHooks {
    SHELL_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_shell_hooks)
            .clone()
    })
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_shell_hooks(f: impl FnOnce(&mut ShellHooks)) {
    SHELL_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_shell_hooks)));
}

/// `table.insert(self.calcsTab.build.overlayPanes, pane)` (CalcSectionControl.lua:240).
pub fn push_overlay_pane(cx: &mut Cx, pane: Rc<dyn OverlayPane>) {
    let me = cx.build().node.id();
    cx.w_mut::<BuildMode>(me).on_frame.overlay_panes.push(pane);
}

/// `self.overlayPanes` (CalcSectionControl's ToggleOverlay / RaiseOverlay edit it in place).
pub fn overlay_panes_mut(cx: &mut Cx) -> &mut Vec<Rc<dyn OverlayPane>> {
    let me = cx.build().node.id();
    &mut cx.w_mut::<BuildMode>(me).on_frame.overlay_panes
}

/// `self.overlayPanes[i]` (1-based; nil past the end), read live.
fn overlay_pane(cx: &Cx, me: CtrlId, i: usize) -> Option<Rc<dyn OverlayPane>> {
    i.checked_sub(1)
        .and_then(|i| cx.w::<BuildMode>(me).on_frame.overlay_panes.get(i).cloned())
}

/// `self.unsaved` as L1327 computes it.
fn compute_unsaved(cx: &Cx, h: &ShellHooks) -> bool {
    // L1327
    cx.build().mod_flag
        || (h.tab_flag)(cx, TabFlag::Notes)
        || (h.tab_flag)(cx, TabFlag::Party)
        || (h.tab_flag)(cx, TabFlag::Config)
        || (h.tab_flag)(cx, TabFlag::Tree)
        || (h.tab_flag)(cx, TabFlag::TreeSearch)
        || (h.tab_flag)(cx, TabFlag::Spec)
        || (h.tab_flag)(cx, TabFlag::Skills)
        || (h.tab_flag)(cx, TabFlag::Items)
        || (h.tab_flag)(cx, TabFlag::Calcs)
}

fn set_view_mode(cx: &Cx, mode: &'static str) {
    cx.build().model.viewMode.set(model::Name::from(mode));
}

fn refresh_stat_list(cx: &mut Cx) {
    let f = init::hooks(cx).refresh_stat_list;
    f(cx);
}

/// lua: Build.lua:1160-1351 `buildMode:OnFrame(inputEvents)`
/// @pob Modules/Build.lua:1160-1351 h=b1cd7c9e
pub fn on_frame(cx: &mut Cx, input_events: &mut InputEvents, _vp: &ViewPort) {
    let Some(me) = cx.app().build.as_ref().map(|b| b.node.id()) else {
        return;
    };
    let h = shell_hooks();
    init::flush_window_titles(cx);
    // Stop at drawing the background if the loaded build needs to be converted
    // L1162
    if cx.w::<BuildMode>(me).init.target_version_nil {
        // L1163
        let vp = cx.main.view_port;
        crate::main_mode::background::draw_background(cx, &vp);
        // L1164
        return;
    }

    // L1167
    if cx.build().abort_save == Some(true) && !cx.app().launch.dev_mode {
        // L1168
        io::close_build(cx);
    }
    // DIVERGENCE(init-closed) (module doc)
    if cx.child(me, "pointDisplay").is_none() {
        return;
    }

    // L1171
    let mut id = 0;
    while let Some(event) = input_events.next_ipairs(&mut id) {
        // L1172
        if let EventKind::KeyDown { .. } = event.kind {
            // L1173
            if event.key == Key::Mouse4 {
                // L1174
                if cx.build().unsaved {
                    // L1175
                    super::popups::open_save_popup(cx, "LIST");
                } else {
                    // L1177
                    io::close_build(cx);
                }
            } else if cx.sys.is_key_down(Key::Ctrl) {
                // L1179 (the elseif above)
                // L1180
                if event.key == Key::Char(b'i') {
                    // L1181
                    set_view_mode(cx, "IMPORT");
                    // L1182
                    (h.import_select_code_in)(cx);
                } else if event.key == Key::Char(b's') {
                    // L1183-1184
                    io::save_db_file(cx);
                    // L1185
                    input_events.consume(id);
                } else if event.key == Key::Char(b'w') {
                    // L1186-1187
                    if cx.build().unsaved {
                        // L1188
                        super::popups::open_save_popup(cx, "LIST");
                    } else {
                        // L1190
                        io::close_build(cx);
                    }
                } else if event.key == Key::Char(b'1') {
                    // L1192-1193
                    set_view_mode(cx, "TREE");
                } else if event.key == Key::Char(b'2') {
                    // L1194-1195
                    set_view_mode(cx, "SKILLS");
                } else if event.key == Key::Char(b'3') {
                    // L1196-1197
                    set_view_mode(cx, "ITEMS");
                } else if event.key == Key::Char(b'4') {
                    // L1198-1199
                    set_view_mode(cx, "CALCS");
                } else if event.key == Key::Char(b'5') {
                    // L1200-1201
                    set_view_mode(cx, "CONFIG");
                } else if event.key == Key::Char(b'6') {
                    // L1202-1203
                    set_view_mode(cx, "NOTES");
                } else if event.key == Key::Char(b'7') {
                    // L1204-1205
                    set_view_mode(cx, "PARTY");
                }
            }
        }
    }

    // Consume mouse events for overlay panes before normal input processing
    // L1212
    let (cursor_x, cursor_y) = cx.sys.cursor_pos();
    // L1213-1215
    let CalcsBreakdown {
        breakdown,
        overlay_breakdown,
    } = (h.calcs_breakdown)(cx);
    // `breakdown:IsMouseOver()` on a nil breakdown raises, as in Lua (only reached when
    // `overlay_breakdown` is true without a control, which CalcsTab never reports)
    let mouse_over_breakdown = |cx: &Cx| match breakdown {
        Some(b) => cx.is_mouse_over(b),
        None => lua_error!("attempt to index local 'breakdown' (a nil value)"),
    };
    // L1216
    let mut i = input_events.lua_len();
    while i >= 1 {
        // L1217
        let Some(event) = input_events.get(i) else {
            // L1218
            break;
        };
        let is_button = event.key.name().contains("BUTTON");
        // L1219
        if matches!(event.kind, EventKind::KeyDown { .. }) && is_button {
            let double_click = matches!(event.kind, EventKind::KeyDown { double_click: true });
            // L1220
            let mut pane_index = cx.w::<BuildMode>(me).on_frame.overlay_panes.len();
            while pane_index >= 1 {
                // L1221
                let Some(pane) = overlay_pane(cx, me, pane_index) else {
                    lua_error!("attempt to index local 'pane' (a nil value)");
                };
                // L1222
                if pane.is_overlay(cx) && pane.is_mouse_in_overlay(cx, cursor_x, cursor_y) {
                    // L1223
                    pane.handle_overlay_click(cx, event.key, cursor_x, cursor_y);
                    // L1224
                    input_events.consume(i);
                    // L1225
                    break;
                }
                pane_index -= 1;
            }
            // L1228
            if input_events.get(i).is_some() && overlay_breakdown && mouse_over_breakdown(cx) {
                let b = breakdown.unwrap_or_else(|| {
                    lua_error!("attempt to index local 'breakdown' (a nil value)")
                });
                // L1229
                let r = cx.on_key_down(b, event.key, double_click);
                cx.w_mut::<BuildMode>(me).on_frame.overlay_breakdown_control = r;
                // L1230
                input_events.consume(i);
            }
        } else if matches!(event.kind, EventKind::KeyUp) && is_button {
            // L1233
            let mut k = 0;
            loop {
                k += 1;
                let Some(pane) = overlay_pane(cx, me, k) else {
                    break;
                };
                // L1234
                if pane.is_overlay(cx) {
                    // L1235
                    pane.handle_overlay_release(cx, event.key);
                }
            }
            // L1238
            if let Some(c) = cx.w::<BuildMode>(me).on_frame.overlay_breakdown_control {
                // L1239
                cx.on_key_up(c, event.key);
                // L1240
                cx.w_mut::<BuildMode>(me).on_frame.overlay_breakdown_control = None;
                // L1241
                input_events.consume(i);
            }
        } else if matches!(event.kind, EventKind::KeyUp)
            && overlay_breakdown
            && mouse_over_breakdown(cx)
            && {
                // L1244
                let b = breakdown.unwrap_or_else(|| {
                    lua_error!("attempt to index local 'breakdown' (a nil value)")
                });
                let sb = cx.child(b, "scrollBar").unwrap_or_else(|| {
                    lua_error!("attempt to index field 'scrollBar' (a nil value)")
                });
                ScrollBarControl::is_scroll_down_key(cx, sb, event.key)
                    || ScrollBarControl::is_scroll_up_key(cx, sb, event.key)
            }
        {
            let b = breakdown
                .unwrap_or_else(|| lua_error!("attempt to index local 'breakdown' (a nil value)"));
            // L1245
            cx.on_key_up(b, event.key);
            // L1246
            input_events.consume(i);
        }
        i -= 1;
    }

    // L1250
    let vp = cx.main.view_port;
    cx.process_controls_input(me, input_events, &vp);

    let spec = cx.build().model.spec.get();
    let class_drop = init_ctl(cx, me, "classDrop");
    let ascend_drop = init_ctl(cx, me, "ascendDrop");
    // L1252
    let cur_class_id = spec.curClassId.get();
    DropDownControl::sel_by_value(cx, class_drop, &|it: &DropItem| {
        it.get::<ClassListValue>()
            .is_some_and(|v| v.class_id == cur_class_id)
    });
    // L1253
    let asc = DropDownControl::get_sel_value_by_key(cx, class_drop, |it| {
        it.get::<ClassListValue>()
            .map(|v| v.ascendancies.clone())
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
    });
    cx.w_mut::<DropDownControl>(ascend_drop).list = asc;
    // L1254
    let cur_ascend_class_id = spec.curAscendClassId.get();
    DropDownControl::sel_by_value(cx, ascend_drop, &|it: &DropItem| {
        it.get::<AscendListValue>()
            .is_some_and(|v| v.ascend_class_id == cur_ascend_class_id)
    });
    // L1255
    DropDownControl::check_dropped_width(cx, ascend_drop, true);
    // L1256
    class_drop::update_secondary_ascendancy_dropdown(cx, false);

    // L1258
    if cx.build().model.buildFlag.get() {
        // Wipe Global Cache
        // L1260 (CCR-0104 §4: model.rs does not re-export modules::common)
        pob_calc::modules::common::wipeGlobalCache();

        // Rebuild calculation output tables
        let model = cx.build().model.clone();
        // L1263
        model.outputRevision.set(model.outputRevision.get() + 1.0);
        // L1264
        model.buildFlag.set(false);
        // L1265
        model.skillsTab.UpdateSocketGroups(&model);
        // L1266
        let built = cx.build_mut().calc.build_output(&model).is_ready();
        init::flush_window_titles(cx);
        if built {
            // CalcsTab.lua:464-467, the UI tail of BuildOutput (CCR-0111 §1, applied by U40)
            crate::tabs::calcs::core::build_output_ui_tail(cx);
            // L1267
            let bd = init_ctl(cx, me, "breakdown");
            if (h.breakdown_pinned)(cx, bd) {
                if let Some(inputs) = cx.w::<BuildMode>(me).on_frame.breakdown_inputs.clone() {
                    // L1268
                    (h.breakdown_set_data)(cx, bd, None);
                    // L1269
                    (h.breakdown_set_data)(cx, bd, Some(&inputs));
                }
            }
            // L1271
            refresh_stat_list(cx);
            // L1272
            if let std::task::Poll::Ready((calc_func, calc_base)) =
                cx.build_mut().calc.misc_calculator(&model)
            {
                let b = cx.build_mut();
                b.calc_func = Some(calc_func);
                b.calc_base = Some(calc_base);
            }
        }
    }
    // CCR-0106 §4: U21's FormatStat writes its own copy of the five fields (`stats::State`);
    // a value there wins over this unit's (which only tests set)
    let (st, st21) = {
        let b = cx.w::<BuildMode>(me);
        (&b.on_frame, &b.stats)
    };
    let (l_sep, l_tsep, l_compact, l_dsep, l_title) = (
        st21.lastShowThousandsSeparators
            .or(st.last_show_thousands_separators),
        st21.lastShowThousandsSeparator
            .clone()
            .or_else(|| st.last_show_thousands_separator.clone()),
        st21.lastUseCompactValues.or(st.last_use_compact_values),
        st21.lastShowDecimalSeparator
            .clone()
            .or_else(|| st.last_show_decimal_separator.clone()),
        st21.lastShowTitlebarName.or(st.last_show_titlebar_name),
    );
    // L1274
    if Some(cx.main.show_thousands_separators) != l_sep {
        // L1275
        refresh_stat_list(cx);
    }
    // L1277
    if Some(&cx.main.thousands_separator) != l_tsep.as_ref() {
        // L1278
        refresh_stat_list(cx);
    }
    // L1280
    if Some(cx.app().main.use_compact_values) != l_compact {
        // L1281
        refresh_stat_list(cx);
    }
    // L1283
    if Some(&cx.main.decimal_separator) != l_dsep.as_ref() {
        // L1284
        refresh_stat_list(cx);
    }
    // L1286
    if Some(cx.app().main.show_titlebar_name) != l_title {
        // L1287
        init::set_window_title_with_build_class(cx);
    }

    // Update contents of main skill dropdowns
    // L1291
    let main_socket_group = cx.build().model.mainSocketGroup.get();
    (h.refresh_skill_select_controls)(cx, me, main_socket_group, "");
    // Draw contents of current tab
    // L1293
    let side_bar_width = 312.0;
    // L1294-1299
    let tab_view_port = ViewPort {
        x: side_bar_width,
        y: 32.0,
        width: cx.main.screen_w - side_bar_width,
        height: cx.main.screen_h - 32.0,
    };
    let view_mode = cx.build().model.viewMode.get();
    // L1300-1318
    let tab = match &*view_mode {
        "IMPORT" => Some(BuildTab::Import),
        "NOTES" => Some(BuildTab::Notes),
        "PARTY" => Some(BuildTab::Party),
        "CONFIG" => Some(BuildTab::Config),
        "TREE" => Some(BuildTab::Tree),
        "SKILLS" => Some(BuildTab::Skills),
        "ITEMS" => Some(BuildTab::Items),
        "CALCS" => Some(BuildTab::Calcs),
        "COMPARE" => Some(BuildTab::Compare),
        _ => None,
    };
    if let Some(tab) = tab {
        (h.draw_tab)(cx, tab, &tab_view_port, input_events);
    }

    // Draw overlay panes on top of all tab content (last = topmost)
    // L1321
    let mut k = 0;
    loop {
        k += 1;
        let Some(pane) = overlay_pane(cx, me, k) else {
            break;
        };
        // L1322
        if pane.is_overlay(cx) {
            // L1323
            let vp = cx.main.view_port;
            pane.draw_overlay(cx, &vp, input_events);
        }
    }

    // L1327
    let unsaved = compute_unsaved(cx, &h);
    cx.build_mut().unsaved = unsaved;

    // L1329
    cx.sg.set_draw_layer(Some(5), None);

    let (screen_w, screen_h) = (cx.main.screen_w, cx.main.screen_h);
    // Draw top bar background
    // L1332
    cx.sg.set_draw_color(0.2, 0.2, 0.2);
    // L1333
    cx.sg.draw_image(None, 0.0, 0.0, screen_w, 28.0);
    // L1334
    cx.sg.set_draw_color(0.85, 0.85, 0.85);
    // L1335
    cx.sg.draw_image(None, 0.0, 28.0, screen_w, 4.0);
    // L1336
    cx.sg.draw_image(None, screen_w / 2.0 - 2.0, 0.0, 4.0, 28.0);

    // Draw side bar background
    // L1339
    cx.sg.set_draw_color(0.1, 0.1, 0.1);
    // L1340
    cx.sg
        .draw_image(None, 0.0, 32.0, side_bar_width - 4.0, screen_h - 32.0);
    // L1341
    cx.sg.set_draw_color(0.85, 0.85, 0.85);
    // L1342
    cx.sg
        .draw_image(None, side_bar_width - 4.0, 32.0, 4.0, screen_h - 32.0);

    // L1345
    let hovered = cx
        .child(me, "statBox")
        .and_then(|sb| cx.w::<TextListControl>(sb).hovered_line.clone());
    // L1346
    set_display_stat(cx, hovered, false);
    // L1347
    let vp = cx.main.view_port;
    cx.draw_controls(me, &vp, None);
    // L1348
    let bd = init_ctl(cx, me, "breakdown");
    if (h.breakdown_pinned)(cx, bd)
        && cx.w::<BuildMode>(me).on_frame.breakdown_inputs.is_some()
        && cx.host(me).sel_control.is_none()
    {
        // L1349
        cx.select_control(me, Some(bd));
    }
}

/// `self.controls[key]` (a missing control is the Lua index error).
#[track_caller]
fn init_ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// lua: Build.lua:1353-1385 `buildMode:SetDisplayStat(hovered, pin)`
/// @pob Modules/Build.lua:1353-1385 h=1138c6d4
pub fn set_display_stat(cx: &mut Cx, hovered: Option<HoveredLine>, pin: bool) {
    let me = cx.build().node.id();
    let h = shell_hooks();
    let bd = init_ctl(cx, me, "breakdown");
    // L1354
    let hovered = match hovered {
        // (`not hovered or (not pin and pinned)`, negated)
        Some(hv) if pin || !(h.breakdown_pinned)(cx, bd) => hv,
        _ => {
            // if this is a hover popup, clear when the user moved mouse out of frame
            // L1356
            if !(h.breakdown_pinned)(cx, bd) {
                // L1357
                clear_display_stat(cx);
            }
            // L1359
            return;
        }
    };
    let line = hovered
        .line
        .extra
        .as_ref()
        .and_then(|e| {
            e.downcast_ref::<StatLineInfo>().cloned().or_else(|| {
                // CCR-0106 §4: U21's rows carry `stats::StatLine` (its own copy of these fields)
                e.downcast_ref::<super::stats::StatLine>()
                    .map(|l| StatLineInfo {
                        breakdown: l.breakdown.clone(),
                        mod_names: l
                            .modNames
                            .map(|v| v.iter().map(|s| (*s).to_owned()).collect()),
                        ignored_sections: l.ignoredSections.as_ref().map(|v| (**v).clone()),
                        actor_name: l.actorName.map(str::to_owned),
                        breakdown_colour: l.breakdownColour,
                    })
            })
        })
        .unwrap_or_default();
    // L1361
    let key = line.breakdown.clone();
    // user clicked a second time: clear
    // L1363
    if pin && (h.breakdown_pinned)(cx, bd) {
        // L1364
        clear_display_stat(cx);
        // L1365
        return;
        // avoid being able to pin empty breakdowns
    } else if pin && !(h.breakdown_pinned)(cx, bd) && key.is_none() {
        // L1368
        return;
    }

    // L1371
    let data = (h.get_sidebar_breakdown)(cx, &line);
    // L1372
    data.set(n!("x"), hovered.x - 1.0);
    // L1373
    data.set(n!("y"), hovered.y);
    // L1374
    data.set(n!("width"), hovered.width);
    cx.w_mut::<BuildMode>(me).on_frame.sidebar_breakdown_data = Some(data.clone());
    // L1375
    if let Some(colour) = line.breakdown_colour {
        // L1376
        (h.breakdown_set_border_colour)(cx, bd, colour);
    }
    // L1378
    let breakdown_inputs = BreakdownInputs {
        data,
        pin,
        actor_name: line.actor_name,
    };
    // avoid rebuilding the sidebar every frame
    // L1380
    if let Some(prev) = &cx.w::<BuildMode>(me).on_frame.breakdown_inputs {
        if breakdown_inputs_equal(&breakdown_inputs, prev) {
            // L1381
            return;
        }
    }
    // L1383
    cx.w_mut::<BuildMode>(me).on_frame.breakdown_inputs = Some(breakdown_inputs.clone());
    // L1384
    (h.breakdown_set_data)(cx, bd, Some(&breakdown_inputs));
}

/// `tableDeepEquals(breakdownInputs, self.breakdownInputs)` for the `{ data, pin, actorName }`
/// sequences: `#` differs when only one has an `actorName`; each slot compares as
/// tableDeepEquals does.
fn breakdown_inputs_equal(a: &BreakdownInputs, b: &BreakdownInputs) -> bool {
    a.actor_name.is_some() == b.actor_name.is_some()
        && table_deep_equals(
            &model::Val::Table(a.data.clone()),
            &model::Val::Table(b.data.clone()),
        )
        && a.pin == b.pin
        && a.actor_name == b.actor_name
}

/// lua: Common.lua:625-640 `tableDeepEquals(t1, t2)`
/// @pob Modules/Common.lua:625-640 h=33ef5684
pub fn table_deep_equals(t1: &model::Val, t2: &model::Val) -> bool {
    use model::Val;
    // L626
    if t1.raw_eq(t2) {
        // L627
        return true;
    }
    // L629
    let (Val::Table(a), Val::Table(b)) = (t1, t2) else {
        // `#t1` of a non-table raises; every caller passes tables or nil
        if !t1.truthy() || !t2.truthy() {
            // L630
            return false;
        }
        lua_error!("attempt to get length of a {} value", t1.lua_type());
    };
    if a.len() != b.len() {
        // L630
        return false;
    }
    // L632 ORDER: the result does not depend on the order (it is false iff some key differs)
    for (k, v1) in a.pairs() {
        // L633
        let v2 = b.get(k);
        // L634
        let type_v1 = v1.lua_type();
        // L635
        if !(type_v1 == v2.lua_type()
            && ((type_v1 == "table" && table_deep_equals(&v1, &v2)) || v1.raw_eq(&v2)))
        {
            // L636
            return false;
        }
    }
    // L639
    true
}

/// lua: Build.lua:1387-1391 `buildMode:ClearDisplayStat()`
/// @pob Modules/Build.lua:1387-1391 h=d7fcef7e
pub fn clear_display_stat(cx: &mut Cx) {
    let me = cx.build().node.id();
    let h = shell_hooks();
    let bd = init_ctl(cx, me, "breakdown");
    // L1388
    (h.breakdown_set_data)(cx, bd, None);
    let st = &mut cx.w_mut::<BuildMode>(me).on_frame;
    // L1389
    st.breakdown_inputs = None;
    // L1390
    st.sidebar_breakdown_data = None;
}

#[cfg(test)]
mod tests {
    use super::*;
    use model::{Table, Val};

    #[test]
    fn table_deep_equals_rules() {
        // Common.lua:625-640
        let a = Table::new();
        a.set(1.0, "x");
        a.set(n!("k"), 2.0);
        let b = Table::new();
        b.set(1.0, "x");
        b.set(n!("k"), 2.0);
        assert!(table_deep_equals(
            &Val::Table(a.clone()),
            &Val::Table(b.clone())
        ));
        // nested tables compare deeply
        let ia = Table::new();
        ia.set(n!("breakdown"), "Life");
        let ib = Table::new();
        ib.set(n!("breakdown"), "Life");
        a.set(2.0, Val::Table(ia));
        b.set(2.0, Val::Table(ib.clone()));
        assert!(table_deep_equals(
            &Val::Table(a.clone()),
            &Val::Table(b.clone())
        ));
        ib.set(n!("breakdown"), "Mana");
        assert!(!table_deep_equals(
            &Val::Table(a.clone()),
            &Val::Table(b.clone())
        ));
        // `#t1 ~= #t2`
        let c = Table::new();
        c.set(1.0, "x");
        assert!(!table_deep_equals(&Val::Table(a.clone()), &Val::Table(c)));
        // nil on either side
        assert!(!table_deep_equals(&Val::Table(a), &Val::Nil));
        assert!(table_deep_equals(&Val::Nil, &Val::Nil));
        // only t1's keys are visited: an extra hash key of t2 does not count
        let d = Table::new();
        d.set(1.0, "x");
        let e = Table::new();
        e.set(1.0, "x");
        e.set(n!("extra"), true);
        assert!(table_deep_equals(&Val::Table(d), &Val::Table(e)));
    }

    #[test]
    fn breakdown_inputs_compare_like_the_lua_sequence() {
        let t = Table::new();
        t.set(n!("x"), 10.0);
        let a = BreakdownInputs {
            data: t,
            pin: false,
            actor_name: None,
        };
        let t2 = Table::new();
        t2.set(n!("x"), 10.0);
        let b = BreakdownInputs {
            data: t2,
            pin: false,
            actor_name: None,
        };
        assert!(breakdown_inputs_equal(&a, &b));
        let c = BreakdownInputs {
            pin: true,
            ..b.clone()
        };
        assert!(!breakdown_inputs_equal(&a, &c));
        let d = BreakdownInputs {
            actor_name: Some("player".to_owned()),
            ..b
        };
        assert!(!breakdown_inputs_equal(&a, &d));
    }
}
