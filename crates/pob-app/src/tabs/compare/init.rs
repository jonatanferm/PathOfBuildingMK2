//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua:1-1242 (module locals,
//! the constructor, InitControls, GetShortBuildName, PopulateSetDropdown, the config helpers,
//! makeConfigControl, RebuildConfigControls, CopyCompareConfig). Owner: U46. The import, remove,
//! re-import, copy-to-primary and popup methods (`:1244-1645`) are in `import.rs`.
//!
//! @pob-file Classes/CompareTab.lua @16de4b82d
//!
//! # The tab node and the split struct
//!
//! `buildMode.compareTab` is a free node owned by the build node ([`ctor_hook`], the
//! `BuildHooks::compare_tab` arm), found again through `BuildState::tabs.compare`. The class is
//! split over U46-U50 (ui doc §12.1); the fields the constructor sets (L129-191) and that the
//! constructor's closures read or write are this file's [`State`] (`cx.w::<CompareTab>(me).init`),
//! including the power-report fields (L180-186) that U48's ComparePowerBuilder reads and the
//! content heights U49/U50's Draw passes write. The fields only U47's layout code writes
//! (`treeLayout` L156, `configSectionLayout` L177) are U47's `layout::State`: their constructor
//! values (nil, `{}`) are those fields' defaults.
//!
//! Ui doc §12.1 puts the `Widget` impl with the owner of `Draw` (U47). BuildMode draws every tab
//! through `ShellHooks::draw_tab` (CCR-0104 §1), so the impl has no methods (`CAPS = empty`, as
//! for NotesTab/ItemsTab/ConfigTab); the constructor needs it, so it is here (CCR-0136 §2).
//! Until U47 adds the `BuildTab::Compare` arm of `draw_tab`, the tab is constructed (every
//! control) and draws nothing, as before.
//!
//! # Seams (CCR-0136 §1)
//!
//! `self.compareEntries[i]` are `CompareEntry` objects (U47, CompareEntry.lua): a
//! [`CompareEntryRef`] (Lua table identity: `Rc::ptr_eq`) holding the entry's label, its
//! lightweight Build's model (`entry.treeTab`/`itemsTab`/`skillsTab`/`configTab`/`calcsTab`
//! model halves, `buildFlag`, `spec`, `mainSocketGroup`) and U47's own state. The entry methods
//! and UI-half fields the constructor's closures and `import.rs` use go through
//! [`CompareHooks`] (entry construction, `SetActiveSpec`/`SetActiveSkillSet`/`SetActiveItemSet`/
//! `SetMainSocketGroup`, the entry ConfigTab's `SetActiveConfigSet`/`UpdateControls`/
//! `varControls`, the entry TreeTab's `viewer.searchStr` and `OpenVersionConvertPopup`) and the
//! `ComparePowerReportListControl` constructor (U48). Each default raises "… is not ported yet
//! (Uxx)" when called, except the power report list, whose default is a placeholder control
//! that draws nothing ([`PendingPowerReportList`]).
//!
//! The primary build's tabs are reached through their owners' seams: `treeTab:SetActiveSpec`
//! (`BuildHooks::tree_set_active_spec`), `itemsTab:SetActiveItemSet` (`BuildHooks::
//! items_set_active_item_set`), `itemsTab:AddUndoState` (U32's `ItemsTabHooks`),
//! `configTab:SetActiveConfigSet` (`BuildHooks::config_set_active_config_set`),
//! `configTab:UpdateControls` (U41's `ConfigHooks`), `treeTab:OpenVersionConvertPopup` (U25),
//! the ImportTab's controls (U43) and `spec:SetWindowTitleWithBuildClass` (U19's
//! `set_window_title_with_build_class`). `configTab:BuildModList` is the model's (calc:CO-C).
//!
//! # Lua semantics
//!
//! * `self.activeCompareIndex` is a Lua number used as an index (0 = none): `usize`.
//! * `entry.treeTab.specList[index]` and the other `orderList[index]` reads are 1-based reads of
//!   the model's lists (nil outside them); `socketGroupList[entry.mainSocketGroup]` and
//!   `displaySkillList[mainActiveSkill or 1]` index with a Lua number (nil unless it is an
//!   integer in range): [`at_num`].
//! * `mainSocketGroup.mainActiveSkill or 1`: the model keeps a nil number as NaN (contracts.md
//!   "Nil in typed fields"): [`or_one`].
//! * `activeSkill.activeEffect.srcInstance.X = v` with a nil `srcInstance` is the Lua error
//!   "attempt to index field 'srcInstance' (a nil value)".
//! * `tonumber(buf)` is `pob_lua::tonumber` (nil for an empty or non-numeric buffer).
//! * `self.controls.cmpStageCountLabel.shown = function() return self.controls.cmpStageCount.shown
//!   end` reads the raw field: a function stored there is truthy ([`raw_shown`]).
//! * `self.controls.compareBuildSelect.list = list` (UpdateBuildSelector) assigns the list field
//!   without `SetList`, as the Lua does.
//! * `control.state = pVal or false`: a check box state is the truthiness of the input value.
//! * makeConfigControl's `tostring(pVal or "")` is Lua `tostring` of the stored input value
//!   (`config::ctor::tostring_val`).
//!
//! DIVERGENCE(alias): `calcsBuffModeDropList` (L470-475) is one table passed to both
//! `primCalcsMode` and `cmpCalcsMode`, and `self.treeVersionDropdownList` (L743-749) to both
//! `leftVersionSelect` and `rightVersionSelect`; DropDownControl stores the table itself, so a
//! later `SetList` on one (which wipes the old list first, DropDownControl.lua:530) would empty
//! the other's. The port gives each control its own copy (U13's DIVERGENCE(alias)); no code
//! calls `SetList` on these four controls.
//!
//! ORDER: `pairs(self.configControls)` in RebuildConfigControls (L1181) only clears
//! `self.controls` keys (order-independent, (c)); `pairs(cInput)` in CopyCompareConfig (L1236)
//! copies distinct keys (order-independent, (c)). The controls are created in the Lua's order
//! (D7: the oracle's canonical `pairs(self.controls)` order is creation order).
//!
//! Props with side effects (ui doc §7.3): none.

#![allow(non_snake_case, non_upper_case_globals)]

use std::any::Any;
use std::cell::{Cell, RefCell};
use std::rc::Rc;

use indexmap::IndexMap;
use pob_lua::{n, Name};
use pob_sg::Font;
use pob_ui_core::controls::drop_down::{DropTipArgs, DropTipMode};
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
    ScrollBarControl,
};
use pob_ui_core::coroutine::Coroutine;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use pob_calc::active_skill::SkillEffectRef;

use super::CompareTab;
use crate::app::CxApp;
use crate::build::init::{self as build_init, MinionListValue};
use crate::model::gamedata::schema::config_defs::ConfigOptionDef;
use crate::model::gamedata::schema::item_data::PowerStat;
use crate::model::{self, Table, Val};
use crate::tabs::config::ctor::{config_hooks, tostring_val, VarClass};
use crate::tabs::tree::TreeTab;

// @pob Classes/CompareTab.lua:1-20 h=f2538385
// (header comment, the `local` aliases and `require`s: the Rust functions and modules below;
// `tradeHelpers`/`buySimilar` are U51's, `calcsHelpers` U48's, `buildListHelpers` U18's
// `build_list::helpers`, `itemSlotHelper` U32's `items::slot_helper`, `configVisibility` U42's)

/// `CLUSTER_NODE_OFFSET` (L20): node IDs below this value are normal passive tree nodes; IDs at
/// or above are cluster jewel nodes.
pub const CLUSTER_NODE_OFFSET: f64 = 65536.0;

/// lua: CompareTab.lua:22-47 `wrapInfoLine(str, width)`
/// Wrap a string into lines for a given pixel width at font height 14 ("VAR").
/// Breaks BEFORE a word that would exceed the width, so rendered lines never
/// overshoot into the next column.
/// @pob Classes/CompareTab.lua:22-47 h=6d9370b0
pub fn wrapInfoLine(cx: &Cx, str: Option<&str>, width: f64) -> Vec<String> {
    use pob_ui_core::luastr::sub;
    // L26
    let mut lines: Vec<String> = Vec::new();
    // L27
    let Some(str) = str.filter(|s| !s.is_empty() && width > 0.0) else {
        // L28
        return lines;
    };
    // L30
    let mut lineStart: i64 = 1;
    // L31
    let mut pos: i64 = 1;
    let len = str.len() as i64;
    // L32
    while pos <= len {
        // L33
        let found = pob_lua::pattern::find(str, "%s+", Some(pos), false);
        let (ws, we) = match found {
            Some((a, b, _)) => (Some(a as i64), Some(b as i64)),
            None => (None, None),
        };
        // L34
        let wordEnd = ws.map_or(len, |ws| ws - 1);
        // L35
        if lineStart < pos
            && cx.sg.draw_string_width(14.0, Font::Var, &sub(str, lineStart as f64, Some(wordEnd as f64)))
                > width
        {
            // L36
            let committed = pob_ui_core::luastr::gsub(&sub(str, lineStart as f64, Some(pos as f64 - 1.0)), "%s+$", "");
            // L37
            lines.push(committed);
            // L38
            lineStart = pos;
        }
        // L40
        let Some(we) = we else {
            // L41
            lines.push(sub(str, lineStart as f64, None).into_owned());
            // L42
            break;
        };
        // L44
        pos = we + 1;
    }
    // L46
    lines
}

/// `LAYOUT` (L49-98): layout constants (shared across Draw, DrawConfig, DrawItems, DrawCalcs,
/// etc.).
/// @pob Classes/CompareTab.lua:49-98 h=b68eac33
pub mod LAYOUT {
    // Main tab control bar
    pub const controlBarHeight: f64 = 126.0;

    // Tree view header/footer
    pub const treeHeaderHeight: f64 = 74.0;
    pub const treeFooterHeight: f64 = 30.0;
    pub const treeOverlayCheckX: f64 = 155.0;

    // Summary view columns
    pub const summaryCol1: f64 = 10.0;
    pub const summaryCol2Right: f64 = 440.0;
    pub const summaryCol3Right: f64 = 580.0;
    pub const summaryCol4: f64 = 600.0;
    pub const summaryHeaderHeight: f64 = 38.0;

    // Items view
    pub const itemsCheckboxOffset: f64 = 78.0;
    pub const itemsCopyBtnW: f64 = 60.0;
    pub const itemsEquipBtnW: f64 = 60.0;
    pub const itemsCopyBtnH: f64 = 18.0;
    pub const itemsBuyBtnW: f64 = 60.0;
    pub const itemsMinColWidth: f64 = 700.0;
    pub const itemsHScrollBarHeight: f64 = 16.0;
    pub const compareColGap: f64 = 48.0;
    pub const skillsHeaderHeight: f64 = 24.0;
    pub const skillsHScrollBarHeight: f64 = 16.0;

    // Calcs view
    pub const calcsMaxCardWidth: f64 = 400.0;
    pub const calcsLabelWidth: f64 = 132.0;
    pub const calcsSepW: f64 = 2.0;
    pub const calcsHeaderBarHeight: f64 = 12.0;

    // Power report section (inside Summary view)
    pub const powerReportLeft: f64 = 10.0;

    // Config view (shared between Draw() layout and DrawConfig())
    pub const configRowHeight: f64 = 22.0;
    pub const configColumnHeaderHeight: f64 = 20.0;
    pub const configFixedHeaderHeight: f64 = 82.0;
    pub const configSectionWidth: f64 = 560.0;
    pub const configSectionGap: f64 = 24.0;
    pub const configSectionColumnGap: f64 = 10.0;
    pub const configSectionInnerPad: f64 = 20.0;
    pub const configLabelOffset: f64 = 10.0;
    pub const configCol2: f64 = 234.0;
    pub const configCol3: f64 = 400.0;
}

/// A `reqFlags` / `notFlags` argument of [`matchFlags`]: a string or a list of strings.
#[derive(Clone, Copy, Debug)]
pub enum FlagSpec<'a> {
    Str(&'a str),
    List(&'a [&'a str]),
}

impl<'a> FlagSpec<'a> {
    /// `if type(x) == "string" then x = { x } end`
    fn as_list(self) -> Vec<&'a str> {
        match self {
            FlagSpec::Str(s) => vec![s],
            FlagSpec::List(l) => l.to_vec(),
        }
    }
}

/// lua: CompareTab.lua:100-124 `matchFlags(reqFlags, notFlags, flags)`
/// Flag matching for stat filtering. `flags(flag)` is the truthiness of `flags[flag]`.
/// @pob Classes/CompareTab.lua:100-124 h=34db17ae
pub fn matchFlags(
    reqFlags: Option<FlagSpec<'_>>,
    notFlags: Option<FlagSpec<'_>>,
    flags: &dyn Fn(&str) -> bool,
) -> bool {
    // L102-104
    let reqFlags = reqFlags.map(FlagSpec::as_list);
    // L105
    if let Some(reqFlags) = reqFlags {
        // L106
        for flag in reqFlags {
            // L107
            if !flags(flag) {
                // L108
                return false;
            }
        }
    }
    // L112-114
    let notFlags = notFlags.map(FlagSpec::as_list);
    // L115
    if let Some(notFlags) = notFlags {
        // L116
        for flag in notFlags {
            // L117
            if flags(flag) {
                // L118
                return false;
            }
        }
    }
    // L122
    true
}

// ---- the compare entries and the seams (CCR-0136 §1) -----------------------------------------

/// One `self.compareEntries[i]`: a `CompareEntry` (CompareEntry.lua, U47). Lua compares entries
/// by identity (`Rc::ptr_eq`).
pub type CompareEntryRef = Rc<CompareEntryHandle>;

/// What U46's code reads of a CompareEntry; U47 fills `ext` with the rest of its state.
pub struct CompareEntryHandle {
    /// `entry.label` (CompareEntry.lua:17: `label or "Comparison Build"`)
    pub label: String,
    /// The entry's lightweight Build: `entry.treeTab`/`itemsTab`/`skillsTab`/`configTab`/
    /// `calcsTab` model halves, `entry.buildFlag`, `entry.spec`, `entry.mainSocketGroup`
    pub model: model::BuildRef,
    /// U47's state of the entry (its UI tab halves, its calc facade, `xmlText`, ...)
    pub ext: RefCell<Option<Box<dyn Any>>>,
}

/// The methods and objects of other units that U46's code calls (CCR-0136 §1). Each default
/// names its owner.
#[derive(Clone)]
pub struct CompareHooks {
    // U47 (CompareEntry.lua)
    /// `new("CompareEntry"):CompareEntry(xmlText, label)`. `None` when the entry has no
    /// `calcsTab` (LoadFromXML failed before creating the tabs, or `xmlText` is nil).
    pub new_compare_entry: Rc<dyn Fn(&mut Cx, Option<&str>, Option<&str>) -> Option<CompareEntryRef>>,
    /// `entry:SetActiveSpec(index)` (CompareEntry.lua:265-270)
    pub entry_set_active_spec: Rc<dyn Fn(&mut Cx, &CompareEntryRef, f64)>,
    /// `entry:SetActiveItemSet(id)` (CompareEntry.lua:272-277)
    pub entry_set_active_item_set: Rc<dyn Fn(&mut Cx, &CompareEntryRef, f64)>,
    /// `entry:SetActiveSkillSet(id)` (CompareEntry.lua:279-284)
    pub entry_set_active_skill_set: Rc<dyn Fn(&mut Cx, &CompareEntryRef, f64)>,
    /// `entry:SetMainSocketGroup(index)` (CompareEntry.lua:291-294)
    pub entry_set_main_socket_group: Rc<dyn Fn(&mut Cx, &CompareEntryRef, f64)>,
    /// `entry.configTab:SetActiveConfigSet(setId)` (the entry's ConfigTab)
    pub entry_config_set_active_config_set: Rc<dyn Fn(&mut Cx, &CompareEntryRef, f64)>,
    /// `entry.configTab:UpdateControls()` (the entry's ConfigTab)
    pub entry_config_update_controls: Rc<dyn Fn(&mut Cx, &CompareEntryRef)>,
    /// `entry.configTab.varControls and entry.configTab.varControls[var]`: the entry ConfigTab's
    /// option control (`None`: no control)
    pub entry_config_var_control: Rc<dyn Fn(&Cx, &CompareEntryRef, Name) -> Option<(CtrlId, VarClass)>>,
    /// `if entry.treeTab and entry.treeTab.viewer then entry.treeTab.viewer.searchStr = buf end`
    pub entry_set_viewer_search_str: Rc<dyn Fn(&mut Cx, &CompareEntryRef, &str)>,
    /// `entry.treeTab:OpenVersionConvertPopup(version, true)`
    pub entry_open_version_convert_popup: Rc<dyn Fn(&mut Cx, &CompareEntryRef, Name)>,
    // U48 (ComparePowerReportListControl.lua)
    /// `self.controls.comparePowerReportList = new("ComparePowerReportListControl"):
    /// ComparePowerReportListControl(nil, {0, 0, 750, 250})` and `.compareTab = self`
    /// (L1039-1040): adds the control to the tab `me` under that key and returns it
    pub power_report_list: Rc<dyn Fn(&mut Cx, CtrlId) -> CtrlId>,
}

#[allow(dead_code)] // every entry has its owner's body now (U47)
#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// The placeholders (CCR-0136 §1).
pub fn default_compare_hooks() -> CompareHooks {
    use super::entry;
    CompareHooks {
        // U47 (CCR-0136 §1: one line each)
        new_compare_entry: Rc::new(entry::new_compare_entry),
        entry_set_active_spec: Rc::new(entry::entry_set_active_spec),
        entry_set_active_item_set: Rc::new(entry::entry_set_active_item_set),
        entry_set_active_skill_set: Rc::new(entry::entry_set_active_skill_set),
        entry_set_main_socket_group: Rc::new(entry::entry_set_main_socket_group),
        entry_config_set_active_config_set: Rc::new(entry::entry_config_set_active_config_set),
        entry_config_update_controls: Rc::new(entry::entry_config_update_controls),
        // CCR-0137 §2: the entry has no ConfigTab controls
        entry_config_var_control: Rc::new(|_, _, _| None),
        entry_set_viewer_search_str: Rc::new(entry::entry_set_viewer_search_str),
        entry_open_version_convert_popup: Rc::new(entry::entry_open_version_convert_popup),
        power_report_list: Rc::new(|cx, me| {
            cx.add(
                me,
                "comparePowerReportList",
                New::new(Base::new(Anchor::NONE, rect(0, 0, 750, 250)), PendingPowerReportList),
            )
            .id()
        }),
    }
}

thread_local! {
    static COMPARE_HOOKS: RefCell<Option<CompareHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn compare_hooks() -> CompareHooks {
    COMPARE_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_compare_hooks)
            .clone()
    })
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_compare_hooks(f: impl FnOnce(&mut CompareHooks)) {
    COMPARE_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_compare_hooks)));
}

/// The placeholder for U48's `ComparePowerReportListControl` until it lands: a control with an
/// empty `Draw` (so the tab's `DrawControls` visits it, as it visits the real one).
pub struct PendingPowerReportList;

impl Widget for PendingPowerReportList {
    const NAME: &'static str = "ComparePowerReportListControl";
    const CAPS: Caps = Caps::DRAW.union(Caps::IS_MOUSE_OVER);
    fn draw(_cx: &mut Cx, _me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {}
    /// CCR-0137 §4: ListControl.lua:119-124 `IsMouseOver` (the placeholder has no scroll bar
    /// child): U47's HandleScrollInput calls it while the list is shown.
    fn is_mouse_over(cx: &Cx, me: CtrlId) -> bool {
        cx.is_shown(me) && cx.is_mouse_in_bounds(me)
    }
}

// ---- the tab's state -------------------------------------------------------------------------

/// `self.comparePowerCategories` (L181)
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct PowerCategories {
    pub treeNodes: bool,
    pub items: bool,
    pub skillGems: bool,
    pub supportGems: bool,
    pub config: bool,
}

/// One `self.configControls[var]` / `self.configControlList[i]` entry (L1213-1218): the same
/// table is in both and in its section's `items`.
pub struct ConfigCtrlInfo {
    pub primaryControl: CtrlId,
    pub compareControl: CtrlId,
    pub varData: &'static ConfigOptionDef,
    /// `ctrlInfo.visible` (LayoutConfigView, U47, writes it)
    pub visible: Cell<bool>,
}

/// One `self.configSections[i]` (L1198): `{ name = varData.section, col = varData.col, items = {} }`.
pub struct ConfigSection {
    pub name: String,
    pub col: Option<f64>,
    pub items: RefCell<Vec<Rc<ConfigCtrlInfo>>>,
}

/// The CompareTab fields of the constructor (L129-191).
pub struct State {
    /// `self.primaryBuild` (the BuildMode node; the model is `cx.build().model`)
    pub primaryBuild: CtrlId,
    /// `self.compareEntries` (indexed 1..N for future 3+ build support)
    pub compareEntries: Vec<CompareEntryRef>,
    /// `self.activeCompareIndex` (0 = none)
    pub activeCompareIndex: usize,
    /// `self.compareViewMode` ("SUMMARY", "TREE", "SKILLS", "ITEMS", "CALCS", "CONFIG")
    pub compareViewMode: &'static str,
    /// `self.scrollY`, `self.itemsScrollX`, `self.skillsScrollX`
    pub scrollY: f64,
    pub itemsScrollX: f64,
    pub skillsScrollX: f64,
    /// Total content height for scroll clamping (populated at end of each Draw* pass)
    pub summaryTotalContentHeight: f64,
    pub itemsTotalContentHeight: f64,
    pub skillsTotalContentHeight: f64,
    /// `self.treeSearchNeedsSync`
    pub treeSearchNeedsSync: bool,
    /// `self.treeOverlayMode` (false = side-by-side, true = overlay with green/red/blue nodes)
    pub treeOverlayMode: bool,
    /// `self.itemTooltip`
    pub itemTooltip: Tooltip,
    /// `self.itemsExpandedMode` (false = compact names only, true = full item details inline)
    pub itemsExpandedMode: bool,
    /// `self.calcsTooltip`
    pub calcsTooltip: Tooltip,
    /// `self.calcsShowOnlyDifferences`
    pub calcsShowOnlyDifferences: bool,
    /// `self.configControls` (var -> { control, varData }), in insertion order
    pub configControls: IndexMap<Name, Rc<ConfigCtrlInfo>>,
    /// `self.configControlList` (ordered list for layout)
    pub configControlList: Vec<Rc<ConfigCtrlInfo>>,
    /// `self.configNeedsRebuild` (trigger initial build)
    pub configNeedsRebuild: bool,
    /// `self.configCompareId` (track which compare entry controls were built for)
    pub configCompareId: Option<usize>,
    /// `self.configToggle` (show all / hide ineligible toggle)
    pub configToggle: bool,
    /// `self.configSections` (section groups from ConfigOptions)
    pub configSections: Vec<Rc<ConfigSection>>,
    /// `self.configTotalContentHeight`
    pub configTotalContentHeight: f64,
    /// `self.comparePowerStat` (selected data.powerStatList entry)
    pub comparePowerStat: Option<&'static PowerStat>,
    /// `self.comparePowerCategories`
    pub comparePowerCategories: PowerCategories,
    /// `self.comparePowerResults` (sorted list of result entries; U48 defines the entries)
    pub comparePowerResults: Option<Rc<dyn Any>>,
    /// `self.comparePowerCoroutine` (active coroutine)
    pub comparePowerCoroutine: Option<Coroutine>,
    /// `self.comparePowerProgress` (0-100)
    pub comparePowerProgress: f64,
    /// `self.comparePowerDirty` (flag to restart calculation)
    pub comparePowerDirty: bool,
    /// `self.comparePowerCompareId` (track which compare entry was calculated)
    pub comparePowerCompareId: Option<usize>,
    /// `self.comparePowerListSynced` (nil until the metric selector clears it)
    pub comparePowerListSynced: bool,
    /// `self.configOptions` (`require("Modules.ConfigOptions")`)
    pub configOptions: &'static [ConfigOptionDef],
    /// `self.treeVersionDropdownList` (L743)
    pub treeVersionDropdownList: Vec<DropItem>,
}

/// A `calcsBuffModeDropList` entry's `buffMode` (L470-475).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct BuffModeValue {
    pub buffMode: &'static str,
}

/// A `treeVersionDropdownList` entry's `value` (L745-748): the tree version.
pub type TreeVersionValue = Name;

/// A `powerStatList` entry (L1007-1012): `{ label = "-- Select Metric --", stat = nil }` or a
/// `data.powerStatList` entry.
#[derive(Clone, Copy)]
pub struct PowerStatValue(pub Option<&'static PowerStat>);

impl Default for State {
    fn default() -> State {
        State {
            primaryBuild: CtrlId::default(),
            compareEntries: Vec::new(),
            activeCompareIndex: 0,
            compareViewMode: "SUMMARY",
            scrollY: 0.0,
            itemsScrollX: 0.0,
            skillsScrollX: 0.0,
            summaryTotalContentHeight: 0.0,
            itemsTotalContentHeight: 0.0,
            skillsTotalContentHeight: 0.0,
            treeSearchNeedsSync: true,
            treeOverlayMode: true,
            itemTooltip: Tooltip::default(),
            itemsExpandedMode: false,
            calcsTooltip: Tooltip::default(),
            calcsShowOnlyDifferences: true,
            configControls: IndexMap::new(),
            configControlList: Vec::new(),
            configNeedsRebuild: true,
            configCompareId: None,
            configToggle: false,
            configSections: Vec::new(),
            configTotalContentHeight: 0.0,
            comparePowerStat: None,
            comparePowerCategories: PowerCategories {
                treeNodes: true,
                items: true,
                skillGems: true,
                supportGems: true,
                config: true,
            },
            comparePowerResults: None,
            comparePowerCoroutine: None,
            comparePowerProgress: 0.0,
            comparePowerDirty: false,
            comparePowerCompareId: None,
            comparePowerListSynced: false,
            configOptions: &[],
            treeVersionDropdownList: Vec::new(),
        }
    }
}

/// drawn by BuildMode (`self.compareTab:Draw(tabViewPort, inputEvents)`, Build.lua:1317; U47's
/// `Draw` is an inherent function reached through `ShellHooks::draw_tab`)
impl Widget for CompareTab {
    const NAME: &'static str = "CompareTab";
    const CAPS: Caps = Caps::empty();
}

// ---- helpers (Rust-only) ----------------------------------------------------------------------

/// The tab's state.
pub fn st(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<CompareTab>(me).init
}

/// The tab's state, mutable.
pub fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
    &mut cx.w_mut::<CompareTab>(me).init
}

/// `self.controls[key]`
pub fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

/// `self.primaryBuild` (the model half).
fn primary(cx: &Cx) -> model::BuildRef {
    cx.build().model.clone()
}

/// `self.compareViewMode == mode`
fn mode_is(cx: &Cx, me: CtrlId, mode: &str) -> bool {
    st(cx, me).compareViewMode == mode
}

/// `#self.compareEntries > 0`
fn has_entries(cx: &Cx, me: CtrlId) -> bool {
    !st(cx, me).compareEntries.is_empty()
}

/// `self:GetActiveCompare() ~= nil`
fn has_active(cx: &Cx, me: CtrlId) -> bool {
    CompareTab::GetActiveCompare(cx, me).is_some()
}

/// `t[i]` of a 1-based list for a Lua number `i` (nil unless `i` is an integer in range).
pub fn at_num<T: Clone>(list: &[T], i: f64) -> Option<T> {
    if i.fract() != 0.0 || i < 1.0 || i > list.len() as f64 {
        return None;
    }
    list.get(i as usize - 1).cloned()
}

/// `x or 1` for a model number that keeps nil as NaN.
fn or_one(x: f64) -> f64 {
    if x.is_nan() {
        1.0
    } else {
        x
    }
}

/// The raw `shown` field as a Lua value's truthiness (a function is truthy).
fn raw_shown(cx: &Cx, id: CtrlId) -> bool {
    match &cx.base(id).shown {
        Prop::Fn(_) => true,
        Prop::Val(v) => *v,
    }
}

/// `build.skillsTab.socketGroupList[index]`
fn socket_group(build: &model::BuildRef, index: f64) -> Option<model::SocketGroupRef> {
    let list = build.skillsTab.socketGroupList.get();
    list.with(|l| at_num(l, index))
}

/// `build.skillsTab.socketGroupList[build.calcsTab.input.skill_number]`
fn calcs_socket_group(build: &model::BuildRef) -> Option<model::SocketGroupRef> {
    match build.calcsTab.input.get(n!("skill_number")) {
        Val::Num(i) => socket_group(build, i),
        _ => None,
    }
}

/// Which socket group and skill list a skill-detail control edits: the main skill's
/// (`socketGroupList[mainSocketGroup]`, `displaySkillList`, `mainActiveSkill`) or the Calcs
/// view's (`socketGroupList[calcsTab.input.skill_number]`, `displaySkillListCalcs`,
/// `mainActiveSkillCalcs`).
#[derive(Clone, Copy, PartialEq, Eq)]
enum SkillList {
    Main,
    Calcs,
}

/// The body shared by the skill-detail callbacks: `local mainSocketGroup = <build>.skillsTab.
/// socketGroupList[<index>]; if mainSocketGroup then local displaySkillList =
/// mainSocketGroup.displaySkillList[Calcs]; local activeSkill = displaySkillList and
/// displaySkillList[mainSocketGroup.mainActiveSkill[Calcs] or 1]; if activeSkill and
/// activeSkill.activeEffect then <set>; <build>.buildFlag = true`. `set` gets
/// `activeSkill.activeEffect` (never nil in the model) and returns whether the buildFlag line
/// runs. `build` is None when `if entry then` fails.
fn with_active_effect(
    build: Option<&model::BuildRef>,
    which: SkillList,
    set: &dyn Fn(&SkillEffectRef) -> bool,
) {
    let Some(m) = build else { return };
    let group = match which {
        SkillList::Main => socket_group(m, m.mainSocketGroup.get()),
        SkillList::Calcs => calcs_socket_group(m),
    };
    let Some(group) = group else { return };
    let (list, main) = match which {
        SkillList::Main => (group.displaySkillList.get(), group.mainActiveSkill.get()),
        SkillList::Calcs => (group.displaySkillListCalcs.get(), group.mainActiveSkillCalcs.get()),
    };
    let Some(list) = list else { return };
    let Some(active_skill) = list.with(|l| at_num(l, or_one(main))) else { return };
    let effect = active_skill.activeEffect.get();
    if set(&effect) {
        m.buildFlag.set(true);
    }
}

/// `activeSkill.activeEffect.srcInstance` as an indexed value (nil raises).
fn src_of(effect: &SkillEffectRef) -> model::GemInstanceRef {
    effect
        .srcInstance
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"))
}

/// The minion selectors' body (L452-459, L564-571, L686-693): `if selected then if
/// selected.itemSetId then src.skillMinionItemSet[Calcs] = selected.itemSetId elseif
/// selected.minionId then src.skillMinion[Calcs] = selected.minionId end; buildFlag = true end`.
fn set_minion(effect: &SkillEffectRef, selected: Option<(Option<f64>, Option<Name>)>, which: SkillList) -> bool {
    let Some((item_set_id, minion_id)) = selected else { return false };
    if let Some(id) = item_set_id {
        let src = src_of(effect);
        match which {
            SkillList::Main => src.skillMinionItemSet.set(Some(id)),
            SkillList::Calcs => src.skillMinionItemSetCalcs.set(Some(id)),
        }
    } else if let Some(id) = minion_id {
        let src = src_of(effect);
        match which {
            SkillList::Main => src.skillMinion.set(Some(id)),
            SkillList::Calcs => src.skillMinionCalcs.set(Some(id)),
        }
    }
    true
}

/// `self:GetActiveCompare()`'s model (None: no active entry).
fn entry_build(cx: &Cx, me: CtrlId) -> Option<model::BuildRef> {
    CompareTab::GetActiveCompare(cx, me).map(|e| e.model.clone())
}

/// `self.controls.X.list[index]` of a minion dropdown, as its `MinionListValue`.
fn minion_sel(cx: &Cx, dropdown: CtrlId, index: usize) -> Option<(Option<f64>, Option<Name>)> {
    let w = cx.w::<DropDownControl>(dropdown);
    let item = index.checked_sub(1).and_then(|i| w.list.get(i))?;
    Some(
        item.get::<MinionListValue>()
            .map_or((None, None), |v| (v.item_set_id, v.minion_id)),
    )
}

/// The model of the active compare entry.
fn active_model(cx: &Cx, me: CtrlId) -> Option<(CompareEntryRef, model::BuildRef)> {
    CompareTab::GetActiveCompare(cx, me).map(|e| {
        let m = e.model.clone();
        (e, m)
    })
}

/// `n.0` as an `f64` (the model's set ids are `u32`).
fn id_num(id: u32) -> f64 {
    f64::from(id)
}

/// Which build a skill/config control edits (the closures' `self.primaryBuild` / `entry`).
#[derive(Clone)]
enum Target {
    Primary,
    Entry(CompareEntryRef),
}

impl Target {
    fn model(&self, cx: &Cx) -> model::BuildRef {
        match self {
            Target::Primary => primary(cx),
            Target::Entry(e) => e.model.clone(),
        }
    }
}

// ---- the constructor ---------------------------------------------------------------------------

impl CompareTab {
    /// lua: CompareTab.lua:126-198 `CompareTabClass:CompareTab(primaryBuild)`
    /// @pob Classes/CompareTab.lua:126-198 h=c8a2b6a5
    pub fn new(primaryBuild: CtrlId) -> New<CompareTab> {
        // L127 newClass("CompareTab", "ControlHost", "Control"): the Widget impl above
        // L130-131 self:ControlHost(); self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            CompareTab {
                init: State {
                    // L133
                    primaryBuild,
                    // L136 Comparison entries (indexed 1..N for future 3+ build support)
                    compareEntries: Vec::new(),
                    // L137
                    activeCompareIndex: 0,
                    // L140 Sub-view mode
                    compareViewMode: "SUMMARY",
                    // L143-145 Scroll offset for scrollable views
                    scrollY: 0.0,
                    itemsScrollX: 0.0,
                    skillsScrollX: 0.0,
                    // L148-150 Total content height for scroll clamping (populated at end of
                    // each Draw* pass)
                    summaryTotalContentHeight: 0.0,
                    itemsTotalContentHeight: 0.0,
                    skillsTotalContentHeight: 0.0,
                    // L153 Tree layout cache (set in Draw, used by DrawTree): U47's
                    // `layout::State` (nil = its default)
                    // L156 Track when tree search fields need syncing with viewer state
                    treeSearchNeedsSync: true,
                    // L159 Tree overlay mode (false = side-by-side, true = overlay with
                    // green/red/blue nodes)
                    treeOverlayMode: true,
                    // L162 Tooltip for item hover in Items view
                    itemTooltip: Tooltip::default(),
                    // L165 Items expanded mode (false = compact names only, true = full item
                    // details inline)
                    itemsExpandedMode: false,
                    // L168 Tooltip for calcs hover breakdown
                    calcsTooltip: Tooltip::default(),
                    // L169
                    calcsShowOnlyDifferences: true,
                    // L172 Interactive config controls state
                    configControls: IndexMap::new(),
                    // L173 ordered list for layout
                    configControlList: Vec::new(),
                    // L174 trigger initial build
                    configNeedsRebuild: true,
                    // L175 track which compare entry controls were built for
                    configCompareId: None,
                    // L176 show all / hide ineligible toggle
                    configToggle: false,
                    // L177 section groups from ConfigOptions
                    configSections: Vec::new(),
                    // L178 computed section layout for drawing: U47's `layout::State`
                    // L179
                    configTotalContentHeight: 0.0,
                    // L182 Compare power report state: selected data.powerStatList entry
                    comparePowerStat: None,
                    // L183
                    comparePowerCategories: PowerCategories {
                        treeNodes: true,
                        items: true,
                        skillGems: true,
                        supportGems: true,
                        config: true,
                    },
                    // L184 sorted list of result entries
                    comparePowerResults: None,
                    // L185 active coroutine
                    comparePowerCoroutine: None,
                    // L186 0-100
                    comparePowerProgress: 0.0,
                    // L187 flag to restart calculation
                    comparePowerDirty: false,
                    // L188 track which compare entry was calculated
                    comparePowerCompareId: None,
                    comparePowerListSynced: false,
                    // L191 Pre-load static module data
                    configOptions: &model::data().configOptions,
                    // L743 (InitControls)
                    treeVersionDropdownList: Vec::new(),
                },
                ..CompareTab::default()
            },
        )
        .host()
        .init(|cx, me| {
            // L192 self.calcSections = LoadModule("Modules/CalcSections"): U39's
            // `tabs::calcs::sections_data` (U50 reads it)
            // L193 self.calcs = require("Modules.Calcs"): `model::calcs` (U48 calls it)
            // L196 Controls for the comparison screen
            Self::InitControls(cx, me.id());
            // L197 return self
        })
    }

    /// lua: CompareTab.lua:200-1071 `CompareTabClass:InitControls()`
    /// @pob Classes/CompareTab.lua:200-1071 h=1dd35712
    pub fn InitControls(cx: &mut Cx, me: CtrlId) {
        // Sub-tab buttons
        // L202
        let subTabs = ["Summary", "Tree", "Skills", "Items", "Calcs", "Config"];
        // L203
        let subTabModes = ["SUMMARY", "TREE", "SKILLS", "ITEMS", "CALCS", "CONFIG"];

        // L205
        let subTabAnchor = cx
            .add(me, "subTabAnchor", PlainControl::new(Anchor::NONE, rect(0, 0, 0, 20)))
            .id();
        let mut prev: Option<CtrlId> = None;
        // L206
        for (i0, tabName) in subTabs.iter().enumerate() {
            let i = i0 + 1;
            // L207
            let mode = subTabModes[i0];
            // L208-211 (`self.controls[prevName]`: the previous button)
            let anchor = if i == 1 {
                Anchor::new(TopLeft, subTabAnchor, TopLeft)
            } else {
                Anchor::new(Left, prev.unwrap_or(subTabAnchor), Right)
            };
            // L212-223
            let id = cx
                .add(
                    me,
                    format!("subTab{tabName}"),
                    ButtonControl::new(
                        anchor,
                        rect(if i == 1 { 0 } else { 4 }, 0, 72, 20),
                        *tabName,
                        move |cx| {
                            // Clear tree overlay compareSpec when leaving TREE mode
                            // L214-215
                            if mode_is(cx, me, "TREE") && st(cx, me).treeOverlayMode {
                                if let Some(tt) = crate::tabs::tree::core::tab(cx) {
                                    // L216
                                    cx.w_mut::<TreeTab>(tt).core.viewer.compare_spec = None;
                                }
                            }
                            let s = st_mut(cx, me);
                            // L218
                            s.compareViewMode = mode;
                            // L219
                            s.scrollY = 0.0;
                            // L220
                            if mode == "TREE" {
                                // L221
                                s.treeSearchNeedsSync = true;
                            }
                        },
                    ),
                )
                .id();
            // L224-226
            cx.base_mut(id).shown = Prop::f(move |cx, _| has_entries(cx, me));
            // L227-229
            cx.w_mut::<ButtonControl>(id).locked = Prop::f(move |cx, _| mode_is(cx, me, mode));
            prev = Some(id);
        }

        // Build B selector dropdown
        // L233
        let compareBuildLabel = cx
            .add(
                me,
                "compareBuildLabel",
                LabelControl::new(
                    Anchor::new(TopLeft, subTabAnchor, TopLeft),
                    rect(0, -88, 0, 16),
                    "^7Compare with:",
                ),
            )
            .id();
        // L234-239
        let compareBuildSelect = cx
            .add(
                me,
                "compareBuildSelect",
                DropDownControl::new(
                    Anchor::new(Left, compareBuildLabel, Right),
                    rect(4, 0, 250, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L235
                        if index > 0 && index <= st(cx, me).compareEntries.len() {
                            let s = st_mut(cx, me);
                            // L236
                            s.activeCompareIndex = index;
                            // L237
                            s.treeSearchNeedsSync = true;
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L240-242
        cx.base_mut(compareBuildSelect).enabled = Prop::f(move |cx, _| has_entries(cx, me));

        // Import button (opens import popup)
        // L245-247
        let importBtn = cx
            .add(
                me,
                "importBtn",
                ButtonControl::new(
                    Anchor::new(Left, compareBuildSelect, Right),
                    rect(8, 0, 100, 20),
                    "Import...",
                    move |cx| {
                        // L246
                        Self::OpenImportPopup(cx, me);
                    },
                ),
            )
            .id();

        // Re-import current build button
        // L250-252
        let reimportBtn = cx
            .add(
                me,
                "reimportBtn",
                ButtonControl::new(
                    Anchor::new(Left, importBtn, Right),
                    rect(4, 0, 140, 20),
                    "Re-import Current",
                    move |cx| {
                        // L251
                        Self::ReimportPrimary(cx, me);
                    },
                ),
            )
            .id();
        // L253-270
        cx.w_mut::<ButtonControl>(reimportBtn).tt.tooltip_func =
            Some(Rc::new(|cx: &mut Cx, tooltip: &mut Tooltip, ()| {
                Self::reimport_tooltip(cx, tooltip);
            }));
        // L271-276
        cx.base_mut(reimportBtn).enabled = Prop::f(|cx, _| {
            use crate::tabs::import::core as import_core;
            // L272
            let importTab = import_core::tab(cx)
                .unwrap_or_else(|| lua_error!("attempt to index local 'importTab' (a nil value)"));
            // check if either oauth or account name import have a character selected
            // L274
            let siteActive = import_core::st(cx, importTab).char_import_mode == "SELECTCHAR"
                && DropDownControl::get_sel_value(cx, ctl(cx, importTab, "siteCharSelect")).is_some();
            // L275
            DropDownControl::get_sel_value(cx, ctl(cx, importTab, "charSelect")).is_some() || siteActive
        });

        // Remove comparison build button
        // L279-283
        let removeBtn = cx
            .add(
                me,
                "removeBtn",
                ButtonControl::new(Anchor::new(Left, reimportBtn, Right), rect(4, 0, 70, 20), "Remove", move |cx| {
                    let s = st(cx, me);
                    // L280
                    if s.activeCompareIndex > 0 && s.activeCompareIndex <= s.compareEntries.len() {
                        let index = s.activeCompareIndex;
                        // L281
                        Self::RemoveBuild(cx, me, index);
                    }
                }),
            )
            .id();
        // L284-286
        cx.base_mut(removeBtn).enabled = Prop::f(move |cx, _| has_entries(cx, me));

        // ============================================================
        // Comparison build set selectors (row between build selector and sub-tabs)
        // ============================================================
        // L291-293
        let setsEnabled = move || Prop::f(move |cx, _| has_entries(cx, me));

        // Tree spec selector for comparison build
        // L296
        let compareSpecLabel = cx
            .add(
                me,
                "compareSpecLabel",
                LabelControl::new(Anchor::new(TopLeft, subTabAnchor, TopLeft), rect(0, -54, 0, 16), "^7Tree set:"),
            )
            .id();
        // L297
        cx.base_mut(compareSpecLabel).shown = setsEnabled();
        // L298-307
        let compareSpecSelect = cx
            .add(
                me,
                "compareSpecSelect",
                DropDownControl::new(
                    Anchor::new(Left, compareSpecLabel, Right),
                    rect(2, 0, 150, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        Self::sel_compare_spec(cx, me, index);
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L308
        cx.base_mut(compareSpecSelect).enabled = setsEnabled();
        {
            let w = cx.w_mut::<DropDownControl>(compareSpecSelect);
            // L309
            w.max_dropped_width = 500.0;
            // L310
            w.enable_dropped_width = true;
        }

        // Skill set selector for comparison build
        // L313
        let compareSkillSetLabel = cx
            .add(
                me,
                "compareSkillSetLabel",
                LabelControl::new(Anchor::new(Left, compareSpecSelect, Right), rect(8, 0, 0, 16), "^7Skill set:"),
            )
            .id();
        // L314
        cx.base_mut(compareSkillSetLabel).shown = setsEnabled();
        // L315-320
        let compareSkillSetSelect = cx
            .add(
                me,
                "compareSkillSetSelect",
                DropDownControl::new(
                    Anchor::new(Left, compareSkillSetLabel, Right),
                    rect(2, 0, 150, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L316
                        let Some((entry, m)) = active_model(cx, me) else { return };
                        // L317 (`entry.skillsTab` always exists)
                        let Some(id) = at_num(&m.skillsTab.skillSetOrderList.with(Clone::clone), index as f64)
                        else {
                            return;
                        };
                        // L318
                        let f = compare_hooks().entry_set_active_skill_set;
                        f(cx, &entry, id_num(id));
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L321
        cx.base_mut(compareSkillSetSelect).enabled = setsEnabled();
        // Item set selector for comparison build
        // L323
        let compareItemSetLabel = cx
            .add(
                me,
                "compareItemSetLabel",
                LabelControl::new(Anchor::new(Left, compareSkillSetSelect, Right), rect(8, 0, 0, 16), "^7Item set:"),
            )
            .id();
        // L324
        cx.base_mut(compareItemSetLabel).shown = setsEnabled();
        // L325-330
        let compareItemSetSelect = cx
            .add(
                me,
                "compareItemSetSelect",
                DropDownControl::new(
                    Anchor::new(Left, compareItemSetLabel, Right),
                    rect(2, 0, 150, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        Self::sel_compare_item_set(cx, me, index);
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L331
        cx.base_mut(compareItemSetSelect).enabled = setsEnabled();
        // Config set selector for comparison build
        // L333
        let compareConfigSetLabel = cx
            .add(
                me,
                "compareConfigSetLabel",
                LabelControl::new(Anchor::new(Left, compareItemSetSelect, Right), rect(8, 0, 0, 16), "^7Config set:"),
            )
            .id();
        // L334
        cx.base_mut(compareConfigSetLabel).shown = setsEnabled();
        // L335-346
        let compareConfigSetSelect = cx
            .add(
                me,
                "compareConfigSetSelect",
                DropDownControl::new(
                    Anchor::new(Left, compareConfigSetLabel, Right),
                    rect(2, 0, 150, 20),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L336
                        // L337 (`entry.configTab` always exists)
                        let Some((entry, m)) = active_model(cx, me) else { return };
                        // L338
                        let setId = at_num(&m.configTab.configSetOrderList.with(Clone::clone), index as f64);
                        // L339
                        if let Some(setId) = setId {
                            // L340
                            let f = compare_hooks().entry_config_set_active_config_set;
                            f(cx, &entry, id_num(setId));
                            // L341
                            m.buildFlag.set(true);
                            // L342
                            st_mut(cx, me).configNeedsRebuild = true;
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L347
        cx.base_mut(compareConfigSetSelect).enabled = setsEnabled();
        // L348
        cx.w_mut::<DropDownControl>(compareConfigSetSelect).enable_dropped_width = true;

        // ============================================================
        // Comparison build main skill selector (row between sets and sub-tabs)
        // ============================================================
        // L352
        let cmpSkillLabel = cx
            .add(
                me,
                "cmpSkillLabel",
                LabelControl::new(Anchor::new(TopLeft, subTabAnchor, TopLeft), rect(0, -32, 0, 16), "^7Skill:"),
            )
            .id();
        // L353
        cx.base_mut(cmpSkillLabel).shown = setsEnabled();
        // L354
        cx.base_mut(cmpSkillLabel).anchor.collapse = true;

        // Socket group dropdown
        // L357-362
        let cmpSocketGroup = Self::add_dropdown(
            cx,
            me,
            "cmpSocketGroup".into(),
            Anchor::new(Left, cmpSkillLabel, Right),
            rect(4, 0, 200, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L358
                // L359
                if let Some(entry) = Self::GetActiveCompare(cx, me) {
                    // L360
                    let f = compare_hooks().entry_set_main_socket_group;
                    f(cx, &entry, index as f64);
                }
            }),
        );
        // L363
        cx.base_mut(cmpSocketGroup).shown = setsEnabled();
        {
            let w = cx.w_mut::<DropDownControl>(cmpSocketGroup);
            // L364
            w.max_dropped_width = 500.0;
            // L365
            w.enable_dropped_width = true;
        }
        // L366
        cx.base_mut(cmpSocketGroup).anchor.collapse = true;

        // Active skill within group
        // L369-378
        let cmpMainSkill = Self::add_dropdown(
            cx,
            me,
            "cmpMainSkill".into(),
            Anchor::new(Left, cmpSocketGroup, Right),
            rect(4, 0, 225, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L370
                // L371
                if let Some((_entry, m)) = active_model(cx, me) {
                    // L372
                    // L373
                    if let Some(mainSocketGroup) = socket_group(&m, m.mainSocketGroup.get()) {
                        // L374
                        mainSocketGroup.mainActiveSkill.set(index as f64);
                        // L375
                        m.buildFlag.set(true);
                    }
                }
            }),
        );
        // L379
        cx.base_mut(cmpMainSkill).shown = Prop::Val(false);
        // L380
        cx.base_mut(cmpMainSkill).anchor.collapse = true;

        // Skill part (multi-part skills)
        // L383-396
        let cmpSkillPart = Self::add_dropdown(
            cx,
            me,
            "cmpSkillPart".into(),
            Anchor::new(Left, cmpMainSkill, Right),
            rect(4, 0, 200, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L384-390
                with_active_effect(entry_build(cx, me).as_ref(), SkillList::Main, &|effect| {
                    // L391
                    src_of(effect).skillPart.set(Some(index as f64));
                    // L392
                    true
                });
            }),
        );
        // L397
        cx.base_mut(cmpSkillPart).shown = Prop::Val(false);
        // L398
        cx.base_mut(cmpSkillPart).anchor.collapse = true;

        // Stage count
        // L401
        let cmpStageCountLabel = cx
            .add(
                me,
                "cmpStageCountLabel",
                LabelControl::new(Anchor::new(Left, cmpSkillPart, Right), rect(6, 0, 0, 16), "^7Stages:"),
            )
            .id();
        // L402
        cx.base_mut(cmpStageCountLabel).shown =
            Prop::f(move |cx, _| raw_shown(cx, ctl(cx, me, "cmpStageCount")));
        // L403
        cx.base_mut(cmpStageCountLabel).anchor.collapse = true;
        // L404-417
        let cmpStageCount = Self::add_count_edit(
            cx,
            me,
            "cmpStageCount".into(),
            Anchor::new(Left, cmpStageCountLabel, Right),
            rect(4, 0, 52, 20),
            Rc::new(move |cx: &mut Cx, buf: &str| {
                // L405-411
                with_active_effect(entry_build(cx, me).as_ref(), SkillList::Main, &|effect| {
                    // L412
                    src_of(effect).skillStageCount.set(pob_lua::tonumber(buf));
                    // L413
                    true
                });
            }),
        );
        // L418
        cx.base_mut(cmpStageCount).shown = Prop::Val(false);
        // L419
        cx.base_mut(cmpStageCount).anchor.collapse = true;

        // Mine count
        // L422
        let cmpMineCountLabel = cx
            .add(
                me,
                "cmpMineCountLabel",
                LabelControl::new(Anchor::new(Left, cmpStageCount, Right), rect(6, 0, 0, 16), "^7Mines:"),
            )
            .id();
        // L423
        cx.base_mut(cmpMineCountLabel).shown =
            Prop::f(move |cx, _| raw_shown(cx, ctl(cx, me, "cmpMineCount")));
        // L424
        cx.base_mut(cmpMineCountLabel).anchor.collapse = true;
        // L425-438
        let cmpMineCount = Self::add_count_edit(
            cx,
            me,
            "cmpMineCount".into(),
            Anchor::new(Left, cmpMineCountLabel, Right),
            rect(4, 0, 52, 20),
            Rc::new(move |cx: &mut Cx, buf: &str| {
                // L426-432
                with_active_effect(entry_build(cx, me).as_ref(), SkillList::Main, &|effect| {
                    // L433
                    src_of(effect).skillMineCount.set(pob_lua::tonumber(buf));
                    // L434
                    true
                });
            }),
        );
        // L439
        cx.base_mut(cmpMineCount).shown = Prop::Val(false);
        // L440
        cx.base_mut(cmpMineCount).anchor.collapse = true;

        // Minion selector
        // L443-463
        let cmpMinion = Self::add_dropdown(
            cx,
            me,
            "cmpMinion".into(),
            Anchor::new(Left, cmpMineCount, Right),
            rect(6, 0, 140, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L444-450
                let selected = minion_sel(cx, ctl(cx, me, "cmpMinion"), index);
                with_active_effect(entry_build(cx, me).as_ref(), SkillList::Main, &|effect| {
                    set_minion(effect, selected, SkillList::Main)
                });
            }),
        );
        // L464
        cx.base_mut(cmpMinion).shown = Prop::Val(false);
        // L465
        cx.base_mut(cmpMinion).anchor.collapse = true;

        // Minion skill selector
        // L468-481
        let cmpMinionSkill = Self::add_dropdown(
            cx,
            me,
            "cmpMinionSkill".into(),
            Anchor::new(Left, cmpMinion, Right),
            rect(4, 0, 140, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L469-475
                with_active_effect(entry_build(cx, me).as_ref(), SkillList::Main, &|effect| {
                    // L476
                    src_of(effect).skillMinionSkill.set(Some(index as f64));
                    // L477
                    true
                });
            }),
        );
        // L482
        cx.base_mut(cmpMinionSkill).shown = Prop::Val(false);
        // L483
        cx.base_mut(cmpMinionSkill).anchor.collapse = true;

        // ============================================================
        // Calcs view skill detail controls (per-build, independent of sidebar & regular Calcs tab)
        // ============================================================
        // L488-493 (one copy per control: DIVERGENCE(alias) in the module notes)
        let calcsBuffModeDropList = || -> Vec<DropItem> {
            [
                ("Unbuffed", "UNBUFFED"),
                ("Buffed", "BUFFED"),
                ("In Combat", "COMBAT"),
                ("Effective DPS", "EFFECTIVE"),
            ]
            .iter()
            .map(|(label, buffMode)| DropItem::new(*label, Rc::new(BuffModeValue { buffMode })))
            .collect()
        };
        // Primary build calcs skill controls (L495-594)
        Self::add_calcs_skill_controls(cx, me, true, calcsBuffModeDropList());
        // Compare build calcs skill controls (L597-723)
        Self::add_calcs_skill_controls(cx, me, false, calcsBuffModeDropList());

        // L725-727
        let calcsShowOnlyDifferencesCheck = cx
            .add(
                me,
                "calcsShowOnlyDifferencesCheck",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(0, 0, 18, 0),
                    Some("Show only differences".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L726
                        st_mut(cx, me).calcsShowOnlyDifferences = state;
                    })),
                    Some(Prop::Val(Some(
                        "Show only rows that differ between both builds. Disable to include unchanged rows.".to_owned(),
                    ))),
                    false,
                ),
            )
            .id();
        // L728-730
        cx.base_mut(calcsShowOnlyDifferencesCheck).shown =
            Prop::f(move |cx, _| mode_is(cx, me, "CALCS") && has_active(cx, me));

        Self::init_tree_and_items_controls(cx, me);
        Self::init_config_and_power_controls(cx, me);
    }

    /// The tree footer, item set and tree set controls of InitControls (L732-918).
    fn init_tree_and_items_controls(cx: &mut Cx, me: CtrlId) {
        // ============================================================
        // Tree footer controls (visible only in TREE view mode with a comparison loaded)
        // ============================================================
        // L735-737
        let treeFooterShown = move || Prop::f(move |cx, _| mode_is(cx, me, "TREE") && has_active(cx, me));
        // L738-740
        let treeSideBySideShown = move || {
            Prop::f(move |cx, _| mode_is(cx, me, "TREE") && has_active(cx, me) && !st(cx, me).treeOverlayMode)
        };

        // Build version dropdown list (shared between left and right)
        // L743
        let mut treeVersionDropdownList = Vec::new();
        // L744
        for num in &model::data().gameVersions.treeVersionList {
            // L745-748
            let display = model::data()
                .gameVersions
                .treeVersions
                .get(num)
                .map(|t| t.display.to_string())
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            treeVersionDropdownList.push(DropItem::new(display, Rc::new(*num as TreeVersionValue)));
        }
        st_mut(cx, me).treeVersionDropdownList = treeVersionDropdownList.clone();

        // Overlay toggle checkbox
        // L752-758
        let treeOverlayCheck = cx
            .add(
                me,
                "treeOverlayCheck",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(0, 0, 20, 0),
                    Some("Overlay comparison".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        let s = st_mut(cx, me);
                        // L753
                        s.treeOverlayMode = state;
                        // L754
                        s.treeSearchNeedsSync = true;
                        // L755
                        if !state {
                            if let Some(tt) = crate::tabs::tree::core::tab(cx) {
                                // L756
                                cx.w_mut::<TreeTab>(tt).core.viewer.compare_spec = None;
                            }
                        }
                    })),
                    None,
                    true,
                ),
            )
            .id();
        // L759
        cx.base_mut(treeOverlayCheck).shown = treeFooterShown();

        // Overlay-mode search (single search for primary viewer)
        // L762-766
        let overlayTreeSearch = cx
            .add(
                me,
                "overlayTreeSearch",
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 0, 300, 20),
                    search_args(Rc::new(|cx: &mut Cx, buf: &str| {
                        // L763-765
                        set_primary_viewer_search(cx, buf);
                    })),
                ),
            )
            .id();
        // L767-769
        cx.base_mut(overlayTreeSearch).shown =
            Prop::f(move |cx, _| mode_is(cx, me, "TREE") && has_active(cx, me) && st(cx, me).treeOverlayMode);

        // Items expanded mode toggle
        // L772-775
        let itemsExpandedCheck = cx
            .add(
                me,
                "itemsExpandedCheck",
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(0, 0, 20, 0),
                    Some("Expanded mode".to_owned()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        let s = st_mut(cx, me);
                        // L773
                        s.itemsExpandedMode = state;
                        // L774
                        s.scrollY = 0.0;
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L776-778
        cx.base_mut(itemsExpandedCheck).shown =
            Prop::f(move |cx, _| mode_is(cx, me, "ITEMS") && has_active(cx, me));

        // Item set dropdown for primary build
        // L781-783
        let itemsShown = move || Prop::f(move |cx, _| mode_is(cx, me, "ITEMS") && has_active(cx, me));
        // L784
        let primaryItemSetLabel = cx
            .add(me, "primaryItemSetLabel", LabelControl::new(Anchor::NONE, rect(0, 0, 0, 16), "^7Item set:"))
            .id();
        // L785
        cx.base_mut(primaryItemSetLabel).shown = itemsShown();
        // L786-791
        let primaryItemSetSelect = Self::add_dropdown(
            cx,
            me,
            "primaryItemSetSelect".into(),
            Anchor::NONE,
            rect(0, 0, 216, 20),
            Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                let m = primary(cx);
                // L787
                let id = at_num(&m.itemsTab.itemSetOrderList.with(Clone::clone), index as f64);
                if let Some(id) = id {
                    // L788
                    let f = build_init::hooks(cx).items_set_active_item_set;
                    f(cx, id_num(id));
                    // L789
                    let f = crate::tabs::items::slot_helper::items_hooks().add_undo_state;
                    f(cx);
                }
            }),
        );
        // L792
        cx.base_mut(primaryItemSetSelect).enabled = itemsShown();
        // L793
        cx.base_mut(primaryItemSetSelect).shown = itemsShown();

        // Item set dropdown for compare build
        // L796
        let compareItemSetLabel2 = cx
            .add(me, "compareItemSetLabel2", LabelControl::new(Anchor::NONE, rect(0, 0, 0, 16), "^7Item set:"))
            .id();
        // L797
        cx.base_mut(compareItemSetLabel2).shown = itemsShown();
        // L798-803
        let compareItemSetSelect2 = Self::add_dropdown(
            cx,
            me,
            "compareItemSetSelect2".into(),
            Anchor::NONE,
            rect(0, 0, 216, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                Self::sel_compare_item_set(cx, me, index);
            }),
        );
        // L804
        cx.base_mut(compareItemSetSelect2).enabled = itemsShown();
        // L805
        cx.base_mut(compareItemSetSelect2).shown = itemsShown();

        // Tree set dropdown for primary build
        // L808
        let primaryTreeSetLabel = cx
            .add(me, "primaryTreeSetLabel", LabelControl::new(Anchor::NONE, rect(0, 0, 0, 16), "^7Tree set:"))
            .id();
        // L809
        cx.base_mut(primaryTreeSetLabel).shown = itemsShown();
        // L810-815
        let primaryTreeSetSelect = Self::add_dropdown(
            cx,
            me,
            "primaryTreeSetSelect".into(),
            Anchor::NONE,
            rect(0, 0, 216, 20),
            Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                sel_primary_spec(cx, index);
            }),
        );
        // L816
        cx.base_mut(primaryTreeSetSelect).enabled = itemsShown();
        // L817
        cx.base_mut(primaryTreeSetSelect).shown = itemsShown();
        {
            let w = cx.w_mut::<DropDownControl>(primaryTreeSetSelect);
            // L818
            w.max_dropped_width = 500.0;
            // L819
            w.enable_dropped_width = true;
        }

        // Tree set dropdown for compare build
        // L822
        let compareTreeSetLabel = cx
            .add(me, "compareTreeSetLabel", LabelControl::new(Anchor::NONE, rect(0, 0, 0, 16), "^7Tree set:"))
            .id();
        // L823
        cx.base_mut(compareTreeSetLabel).shown = itemsShown();
        // L824-832
        let compareTreeSetSelect = Self::add_dropdown(
            cx,
            me,
            "compareTreeSetSelect".into(),
            Anchor::NONE,
            rect(0, 0, 216, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                Self::sel_compare_spec(cx, me, index);
            }),
        );
        // L833
        cx.base_mut(compareTreeSetSelect).enabled = itemsShown();
        // L834
        cx.base_mut(compareTreeSetSelect).shown = itemsShown();
        {
            let w = cx.w_mut::<DropDownControl>(compareTreeSetSelect);
            // L835
            w.max_dropped_width = 500.0;
            // L836
            w.enable_dropped_width = true;
        }

        // Footer anchor controls (side-by-side only)
        // L839
        let leftFooterAnchor = cx
            .add(me, "leftFooterAnchor", PlainControl::new(Anchor::NONE, rect(0, 0, 0, 20)))
            .id();
        // L840
        cx.base_mut(leftFooterAnchor).shown = treeSideBySideShown();
        // L841
        let rightFooterAnchor = cx
            .add(me, "rightFooterAnchor", PlainControl::new(Anchor::NONE, rect(0, 0, 0, 20)))
            .id();
        // L842
        cx.base_mut(rightFooterAnchor).shown = treeSideBySideShown();

        // Left side (primary build) spec/version controls (header, both modes)
        // L845-850
        let leftSpecSelect = Self::add_dropdown(
            cx,
            me,
            "leftSpecSelect".into(),
            Anchor::NONE,
            rect(0, 0, 180, 20),
            Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                sel_primary_spec(cx, index);
            }),
        );
        // L851
        cx.base_mut(leftSpecSelect).shown = treeFooterShown();
        {
            let w = cx.w_mut::<DropDownControl>(leftSpecSelect);
            // L852
            w.max_dropped_width = 500.0;
            // L853
            w.enable_dropped_width = true;
        }

        // L855-859
        let leftVersionSelect = cx
            .add(
                me,
                "leftVersionSelect",
                DropDownControl::new(
                    Anchor::new(Left, leftSpecSelect, Right),
                    rect(4, 0, 100, 20),
                    treeVersionDropdownList.clone(),
                    Some(Rc::new(|cx: &mut Cx, _index: usize, selected: Option<&DropItem>| {
                        // L856 (`self.primaryBuild.spec` is always set)
                        let Some(v) = selected.and_then(|s| s.get::<TreeVersionValue>()).copied() else {
                            return;
                        };
                        if v != primary(cx).spec.get().treeVersion.get() {
                            // L857
                            let tt = crate::tabs::tree::core::tab(cx).unwrap_or_else(|| {
                                lua_error!("attempt to index field 'treeTab' (a nil value)")
                            });
                            TreeTab::OpenVersionConvertPopup(cx, tt, v, true);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L860
        cx.base_mut(leftVersionSelect).shown = treeFooterShown();

        // Left search (footer, side-by-side only)
        // L863-867
        let leftTreeSearch = cx
            .add(
                me,
                "leftTreeSearch",
                EditControl::new(
                    Anchor::new(TopLeft, leftFooterAnchor, TopLeft),
                    rect(0, 0, 200, 20),
                    search_args(Rc::new(|cx: &mut Cx, buf: &str| {
                        // L864-866
                        set_primary_viewer_search(cx, buf);
                    })),
                ),
            )
            .id();
        // L868
        cx.base_mut(leftTreeSearch).shown = treeSideBySideShown();

        // Right side (compare build) spec/version controls (header, both modes)
        // L871-880
        let rightSpecSelect = Self::add_dropdown(
            cx,
            me,
            "rightSpecSelect".into(),
            Anchor::NONE,
            rect(0, 0, 180, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                Self::sel_compare_spec(cx, me, index);
            }),
        );
        // L881
        cx.base_mut(rightSpecSelect).shown = treeFooterShown();
        {
            let w = cx.w_mut::<DropDownControl>(rightSpecSelect);
            // L882
            w.max_dropped_width = 500.0;
            // L883
            w.enable_dropped_width = true;
        }

        // L885-892
        let rightVersionSelect = cx
            .add(
                me,
                "rightVersionSelect",
                DropDownControl::new(
                    Anchor::new(Left, rightSpecSelect, Right),
                    rect(4, 0, 100, 20),
                    treeVersionDropdownList,
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, selected: Option<&DropItem>| {
                        // L886
                        let entry = Self::GetActiveCompare(cx, me);
                        // L887 (`entry.spec` is always set)
                        let (Some(entry), Some(v)) =
                            (entry, selected.and_then(|s| s.get::<TreeVersionValue>()).copied())
                        else {
                            return;
                        };
                        // L888
                        if v != entry.model.spec.get().treeVersion.get() {
                            // L889
                            let f = compare_hooks().entry_open_version_convert_popup;
                            f(cx, &entry, v);
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L893
        cx.base_mut(rightVersionSelect).shown = treeFooterShown();

        // Copy compared tree to primary build
        // L896-898
        let copySpecBtn = cx
            .add(
                me,
                "copySpecBtn",
                ButtonControl::new(Anchor::new(Left, rightVersionSelect, Right), rect(4, 0, 76, 20), "Copy tree", move |cx| {
                    // L897
                    Self::CopyCompareSpecToPrimary(cx, me, false);
                }),
            )
            .id();
        // L899
        cx.base_mut(copySpecBtn).shown = treeFooterShown();
        // L900-903
        let copySpecEnabled: PropFn<bool> = Rc::new(move |cx, _| {
            // L901
            let entry = Self::GetActiveCompare(cx, me);
            // L902
            entry.is_some_and(|e| {
                let tt = &e.model.treeTab;
                tt.specList.with(|l| at_num(l, tt.activeSpec.get())).is_some()
            })
        });
        cx.base_mut(copySpecBtn).enabled = Prop::Fn(copySpecEnabled.clone());

        // L905-907
        let copySpecUseBtn = cx
            .add(
                me,
                "copySpecUseBtn",
                ButtonControl::new(Anchor::new(Left, copySpecBtn, Right), rect(2, 0, 100, 20), "Copy and use", move |cx| {
                    // L906
                    Self::CopyCompareSpecToPrimary(cx, me, true);
                }),
            )
            .id();
        // L908
        cx.base_mut(copySpecUseBtn).shown = treeFooterShown();
        // L909
        cx.base_mut(copySpecUseBtn).enabled = Prop::Fn(copySpecEnabled);

        // Right search (footer, side-by-side only)
        // L912-917
        let rightTreeSearch = cx
            .add(
                me,
                "rightTreeSearch",
                EditControl::new(
                    Anchor::new(TopLeft, rightFooterAnchor, TopLeft),
                    rect(0, 0, 200, 20),
                    search_args(Rc::new(move |cx: &mut Cx, buf: &str| {
                        // L913
                        // L914 (the hook stands for `entry.treeTab and entry.treeTab.viewer`)
                        if let Some(entry) = Self::GetActiveCompare(cx, me) {
                            // L915
                            let f = compare_hooks().entry_set_viewer_search_str;
                            f(cx, &entry, buf);
                        }
                    })),
                ),
            )
            .id();
        // L918
        cx.base_mut(rightTreeSearch).shown = treeSideBySideShown();
    }

    /// The config view and Compare Power Report controls and the scroll bars of InitControls
    /// (L920-1071).
    fn init_config_and_power_controls(cx: &mut Cx, me: CtrlId) {
        let configShown = move || Prop::f(move |cx, _| mode_is(cx, me, "CONFIG") && has_active(cx, me));
        // Config view: "Copy Config from Compare Build" button
        // L921-923
        let copyConfigBtn = cx
            .add(
                me,
                "copyConfigBtn",
                ButtonControl::new(Anchor::NONE, rect(0, 0, 240, 20), "Copy Config from Compare Build", move |cx| {
                    Self::CopyCompareConfig(cx, me);
                }),
            )
            .id();
        // L924-926
        cx.base_mut(copyConfigBtn).shown = configShown();

        // Config view: "Show All / Hide Ineligible" toggle button
        // L929-935
        let configToggleBtn = cx
            .add(
                me,
                "configToggleBtn",
                ButtonControl::new(
                    Anchor::NONE,
                    rect(0, 0, 240, 20),
                    // L930-932
                    Prop::f(move |cx, _| {
                        if st(cx, me).configToggle {
                            "Hide Ineligible Configurations".to_owned()
                        } else {
                            "Show All Configurations".to_owned()
                        }
                    }),
                    move |cx| {
                        // L934
                        let s = st_mut(cx, me);
                        s.configToggle = !s.configToggle;
                    },
                ),
            )
            .id();
        // L936-938
        cx.base_mut(configToggleBtn).shown = configShown();

        // Config view: search bar
        // L941
        let configSearchEdit = cx
            .add(
                me,
                "configSearchEdit",
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 0, 240, 20),
                    EditArgs {
                        init: Some(String::new()),
                        prompt: Some("Search".to_owned()),
                        filter: Some("%c".to_owned()),
                        limit: Some(100),
                        clearable: true,
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L942-944
        cx.base_mut(configSearchEdit).shown = configShown();

        // Config view: primary build config set dropdown
        // L947-949 (`configShown` above)
        // L950
        let configPrimarySetLabel = cx
            .add(me, "configPrimarySetLabel", LabelControl::new(Anchor::NONE, rect(0, 0, 0, 16), "^7Config set:"))
            .id();
        // L951
        cx.base_mut(configPrimarySetLabel).shown = configShown();
        // L952-959 (a nil list is `SetList(list or { })`)
        let configPrimarySetSelect = Self::add_dropdown(
            cx,
            me,
            "configPrimarySetSelect".into(),
            Anchor::NONE,
            rect(0, 0, 150, 20),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L953
                let m = primary(cx);
                // L954
                let setId = at_num(&m.configTab.configSetOrderList.with(Clone::clone), index as f64);
                // L955
                if let Some(setId) = setId {
                    // L956
                    let f = build_init::hooks(cx).config_set_active_config_set;
                    f(cx, id_num(setId));
                    // L957
                    st_mut(cx, me).configNeedsRebuild = true;
                }
            }),
        );
        // L960
        cx.base_mut(configPrimarySetSelect).shown = configShown();
        // L961
        cx.w_mut::<DropDownControl>(configPrimarySetSelect).enable_dropped_width = true;
        // L962-964
        cx.base_mut(configPrimarySetSelect).enabled =
            Prop::f(|cx, _| primary(cx).configTab.configSetOrderList.len() > 1);

        // ============================================================
        // Compare Power Report controls (Summary view)
        // ============================================================
        // L969-971
        let powerReportShown = move || Prop::f(move |cx, _| mode_is(cx, me, "SUMMARY") && has_entries(cx, me));

        // Metric dropdown
        // L974
        let mut powerStatList = vec![DropItem::new("-- Select Metric --", Rc::new(PowerStatValue(None)))];
        // L975
        for entry in &model::data().powerStatList.stats {
            // L976
            if entry.stat.is_some() && !entry.ignoreForNodes {
                // L977
                powerStatList.push(DropItem::new(entry.label.to_string(), Rc::new(PowerStatValue(Some(entry)))));
            }
        }
        // L980-990
        let comparePowerStatSelect = cx
            .add(
                me,
                "comparePowerStatSelect",
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 200, 20),
                    powerStatList,
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        let value = value.and_then(|v| v.get::<PowerStatValue>()).copied();
                        let s = st_mut(cx, me);
                        match value {
                            // L981
                            Some(PowerStatValue(Some(stat)))
                                if !s.comparePowerStat.is_some_and(|c| std::ptr::eq(c, stat)) =>
                            {
                                // L982
                                s.comparePowerStat = Some(stat);
                                // L983
                                s.comparePowerDirty = true;
                            }
                            // L984
                            Some(PowerStatValue(None)) => {
                                // L985
                                s.comparePowerStat = None;
                                // L986
                                s.comparePowerResults = None;
                                // L987
                                s.comparePowerCoroutine = None;
                                // L988
                                s.comparePowerListSynced = false;
                            }
                            _ => {}
                        }
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L991
        cx.base_mut(comparePowerStatSelect).shown = powerReportShown();
        // L992-998
        cx.w_mut::<DropDownControl>(comparePowerStatSelect).tt.tooltip_func =
            Some(Rc::new(move |cx: &mut Cx, tooltip: &mut Tooltip, (mode, _index, _value): DropTipArgs| {
                // L993
                tooltip.clear(false);
                // L994
                if mode == DropTipMode::Out || cx.w::<DropDownControl>(ctl(cx, me, "comparePowerStatSelect")).dropped {
                    // L995
                    return;
                }
                // L997
                tooltip.add_line(cx, 14.0, "Select a metric to calculate power report");
            }));

        // Category checkboxes: L1001-1006, L1008-1013, L1015-1020, L1022-1027, L1029-1034
        let power_check = |cx: &mut Cx,
                           key: &'static str,
                           label: &str,
                           tip: &str,
                           set: fn(&mut PowerCategories, bool)|
         -> CtrlId {
            // L1001 / L1008 / L1015 / L1022 / L1029
            let id = cx
                .add(
                    me,
                    key,
                    CheckBoxControl::new(
                        Anchor::NONE,
                        rect(0, 0, 18, 0),
                        Some(label.to_owned()),
                        Some(Rc::new(move |cx: &mut Cx, state: bool| {
                            let s = st_mut(cx, me);
                            // L1002 / L1009 / L1016 / L1023 / L1030
                            set(&mut s.comparePowerCategories, state);
                            // L1003 / L1010 / L1017 / L1024 / L1031
                            s.comparePowerDirty = true;
                        })),
                        Some(Prop::Val(Some(tip.to_owned()))),
                        false,
                    ),
                )
                .id();
            // L1005 / L1012 / L1019 / L1026 / L1033
            cx.base_mut(id).shown = powerReportShown();
            // L1006 / L1013 / L1020 / L1027 / L1034
            cx.w_mut::<CheckBoxControl>(id).state = true;
            id
        };
        power_check(
            cx,
            "comparePowerTreeCheck",
            "Tree:",
            "Include passive tree nodes from compared build",
            |c, v| c.treeNodes = v,
        );
        power_check(cx, "comparePowerItemsCheck", "Items:", "Include items from compared build", |c, v| {
            c.items = v;
        });
        power_check(
            cx,
            "comparePowerGemsCheck",
            "Skill gems:",
            "Include skill gem groups unique to compared build",
            |c, v| c.skillGems = v,
        );
        power_check(
            cx,
            "comparePowerSupportGemsCheck",
            "Support gems:",
            "Include support gems from compared build's active skill",
            |c, v| c.supportGems = v,
        );
        power_check(
            cx,
            "comparePowerConfigCheck",
            "Config:",
            "Include config option differences from compared build",
            |c, v| c.config = v,
        );

        // Power report list control (static height, own scrollbar)
        // L1037-1038
        let f = compare_hooks().power_report_list;
        let comparePowerReportList = f(cx, me);
        // L1039
        cx.base_mut(comparePowerReportList).shown = powerReportShown();

        // Scrollbar for Calcs sub-tab
        // L1042
        let calcsScrollBar = cx
            .add(
                me,
                "calcsScrollBar",
                ScrollBarControl::new(Anchor::NONE, rect(0, 0, 18, 0), Some(50.0), Some(ScrollDir::Vertical), true),
            )
            .id();
        // L1043 local calcsScrollBar
        // L1044-1046
        cx.base_mut(calcsScrollBar).shown = Prop::f(move |cx, sb| {
            mode_is(cx, me, "CALCS") && has_active(cx, me) && ScrollBarControl::enabled_raw(cx, sb)
        });

        // Shared vertical scrollbar for Summary/Items/Skills/Config sub-tabs
        // L1049
        let viewScrollBar = cx
            .add(
                me,
                "viewScrollBar",
                ScrollBarControl::new(Anchor::NONE, rect(0, 0, 18, 0), Some(50.0), Some(ScrollDir::Vertical), true),
            )
            .id();
        // L1050 local viewScrollBar
        // L1051-1056
        cx.base_mut(viewScrollBar).shown = Prop::f(move |cx, sb| {
            has_active(cx, me)
                && !mode_is(cx, me, "TREE")
                && !mode_is(cx, me, "CALCS")
                && ScrollBarControl::enabled_raw(cx, sb)
        });

        // Horizontal scrollbar for Items sub-tab
        // L1059
        let itemsHScrollBar = cx
            .add(
                me,
                "itemsHScrollBar",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(0, 0, 0, LAYOUT::itemsHScrollBarHeight),
                    Some(60.0),
                    Some(ScrollDir::Horizontal),
                    true,
                ),
            )
            .id();
        // L1060 local itemsHScrollBar
        // L1061-1063
        cx.base_mut(itemsHScrollBar).shown = Prop::f(move |cx, sb| {
            mode_is(cx, me, "ITEMS") && has_active(cx, me) && ScrollBarControl::enabled_raw(cx, sb)
        });

        // Horizontal scrollbar for Skills sub-tab
        // L1066
        let skillsHScrollBar = cx
            .add(
                me,
                "skillsHScrollBar",
                ScrollBarControl::new(
                    Anchor::NONE,
                    rect(0, 0, 0, LAYOUT::skillsHScrollBarHeight),
                    Some(60.0),
                    Some(ScrollDir::Horizontal),
                    true,
                ),
            )
            .id();
        // L1067 local skillsHScrollBar
        // L1068-1070
        cx.base_mut(skillsHScrollBar).shown = Prop::f(move |cx, sb| {
            mode_is(cx, me, "SKILLS") && has_active(cx, me) && ScrollBarControl::enabled_raw(cx, sb)
        });
    }

    /// The Re-import button's `tooltipFunc` (L253-270).
    fn reimport_tooltip(cx: &mut Cx, tooltip: &mut Tooltip) {
        use crate::tabs::import::character::CharEntry;
        use crate::tabs::import::core as import_core;
        use crate::tabs::import::poe_api::json_get;
        // L254
        tooltip.clear(false);
        // L255
        let importTab = import_core::tab(cx)
            .unwrap_or_else(|| lua_error!("attempt to index local 'importTab' (a nil value)"));
        // L256
        if import_core::st(cx, importTab).char_import_mode == "SELECTCHAR" {
            // L257
            let charSelect = ctl(cx, importTab, "charSelect");
            // L258
            let charData = {
                let w = cx.w::<DropDownControl>(charSelect);
                w.sel_index.checked_sub(1).and_then(|i| w.list.get(i)).cloned()
            };
            // L259
            let ch = charData.as_ref().and_then(|d| d.get::<CharEntry>()).map(|e| e.char.clone());
            if let Some(ch) = ch.filter(|c| !c.is_null()) {
                let field = |k: &str| -> String {
                    match json_get(&ch, k) {
                        Some(serde_json::Value::String(s)) => s.clone(),
                        Some(serde_json::Value::Number(n)) => pob_lua::tostring_num(n.as_f64().unwrap_or(0.0)),
                        _ => lua_error!("attempt to concatenate field '{k}' (a nil value)"),
                    }
                };
                // L260
                tooltip.add_line(cx, 16.0, "Re-import character from the game server:");
                // L261
                let line = format!("^7{} ({}, {})", field("name"), field("class"), field("league"));
                tooltip.add_line(cx, 14.0, &line);
            } else {
                // L263
                tooltip.add_line(cx, 16.0, "Re-import the currently selected character.");
            }
            // L265
            tooltip.add_line(cx, 14.0, "^7Refreshes passive tree, jewels, items, and skills.");
        } else {
            // L267
            tooltip.add_line(cx, 16.0, "^7No character selected.");
            // L268
            tooltip.add_line(cx, 14.0, "^7Go to Import/Export Build tab and select a character first.");
        }
    }

    /// The tree spec selectors of the compared build share the body (`compareSpecSelect`
    /// L298-307, `compareTreeSetSelect` L824-832, `rightSpecSelect` L871-880): `local entry =
    /// self:GetActiveCompare(); if entry and entry.treeTab and entry.treeTab.specList[index] then
    /// entry:SetActiveSpec(index); if self.primaryBuild.spec then
    /// self.primaryBuild.spec:SetWindowTitleWithBuildClass() end end`.
    fn sel_compare_spec(cx: &mut Cx, me: CtrlId, index: usize) {
        // L299 / L825 / L872
        let Some((entry, m)) = active_model(cx, me) else { return };
        // L300 / L826 / L873
        if m.treeTab.specList.with(|l| at_num(l, index as f64)).is_none() {
            return;
        }
        // L301 / L827 / L874
        let f = compare_hooks().entry_set_active_spec;
        f(cx, &entry, index as f64);
        // Restore primary build's window title (SetActiveSpec changes it)
        // L303 / L828 / L876 (`self.primaryBuild.spec` is always set)
        // L304 / L829 / L877
        build_init::set_window_title_with_build_class(cx);
    }

    /// The item set selectors of the compared build share the body (`compareItemSetSelect`
    /// L325-330, `compareItemSetSelect2` L798-803).
    fn sel_compare_item_set(cx: &mut Cx, me: CtrlId, index: usize) {
        // L326 / L799
        let Some((entry, m)) = active_model(cx, me) else { return };
        // L327 / L800 (`entry.itemsTab` always exists)
        let Some(id) = at_num(&m.itemsTab.itemSetOrderList.with(Clone::clone), index as f64) else {
            return;
        };
        // L328 / L801
        let f = compare_hooks().entry_set_active_item_set;
        f(cx, &entry, id_num(id));
    }

    /// `self.controls[key] = new("DropDownControl"):DropDownControl(anchor, rect, {}, selFunc)`
    fn add_dropdown(
        cx: &mut Cx,
        me: CtrlId,
        key: CtrlKey,
        anchor: Anchor,
        r: [Prop<f64>; 4],
        sel: pob_ui_core::controls::drop_down::SelFunc,
    ) -> CtrlId {
        cx.add(me, key, DropDownControl::new(anchor, r, Vec::new(), Some(sel), None, false))
            .id()
    }

    /// `self.controls[key] = new("EditControl"):EditControl(anchor, rect, "", nil, "%D", 5,
    /// changeFunc)`
    fn add_count_edit(
        cx: &mut Cx,
        me: CtrlId,
        key: CtrlKey,
        anchor: Anchor,
        r: [Prop<f64>; 4],
        change: Rc<dyn Fn(&mut Cx, &str)>,
    ) -> CtrlId {
        cx.add(
            me,
            key,
            EditControl::new(
                anchor,
                r,
                EditArgs {
                    init: Some(String::new()),
                    filter: Some("%D".to_owned()),
                    limit: Some(5),
                    change_func: Some(change),
                    ..EditArgs::default()
                },
            ),
        )
        .id()
    }

    /// The calcs skill detail controls of one build: the primary build's (`prim*`, L495-594) or
    /// the compared build's (`cmp*`, L597-723, whose callbacks start with `local entry =
    /// self:GetActiveCompare(); if entry then`). Anchors in the comments: primary / compared.
    fn add_calcs_skill_controls(cx: &mut Cx, me: CtrlId, is_prim: bool, buffModes: Vec<DropItem>) {
        let prefix = if is_prim { "prim" } else { "cmp" };
        let key = move |s: &str| -> String { format!("{prefix}{s}") };
        // The build a callback edits: `self.primaryBuild`, or `self:GetActiveCompare()` (None:
        // `if entry then` fails and the callback does nothing).
        let build_of = move |cx: &Cx| -> Option<model::BuildRef> {
            if is_prim {
                Some(primary(cx))
            } else {
                Self::GetActiveCompare(cx, me).map(|e| e.model.clone())
            }
        };

        // L495-498 / L597-603
        let socketGroup = Self::add_dropdown(
            cx,
            me,
            key("CalcsSocketGroup").into(),
            Anchor::NONE,
            rect(0, 0, 200, 18),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L598-599
                let Some(m) = build_of(cx) else { return };
                // L496 / L600
                m.calcsTab.input.set(n!("skill_number"), index as f64);
                // L497 / L601
                m.buildFlag.set(true);
            }),
        );
        // L499 / L604
        cx.base_mut(socketGroup).shown = Prop::Val(false);
        {
            let w = cx.w_mut::<DropDownControl>(socketGroup);
            // L500 / L605
            w.max_dropped_width = 400.0;
            // L501 / L606
            w.enable_dropped_width = true;
        }

        // L503-509 / L608-617
        let mainSkill = Self::add_dropdown(
            cx,
            me,
            key("CalcsMainSkill").into(),
            Anchor::NONE,
            rect(0, 0, 200, 18),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                let Some(m) = build_of(cx) else { return };
                // L504 / L611
                // L505 / L612
                if let Some(mainSocketGroup) = calcs_socket_group(&m) {
                    // L506 / L613
                    mainSocketGroup.mainActiveSkillCalcs.set(index as f64);
                    // L507 / L614
                    m.buildFlag.set(true);
                }
            }),
        );
        // L510 / L618
        cx.base_mut(mainSkill).shown = Prop::Val(false);

        // L512-522 / L620-633
        let skillPart = Self::add_dropdown(
            cx,
            me,
            key("CalcsSkillPart").into(),
            Anchor::NONE,
            rect(0, 0, 150, 18),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L513-517 / L621-627
                with_active_effect(build_of(cx).as_ref(), SkillList::Calcs, &|effect| {
                    // L518 / L628
                    src_of(effect).skillPartCalcs.set(Some(index as f64));
                    // L519 / L629
                    true
                });
            }),
        );
        // L523 / L634
        cx.base_mut(skillPart).shown = Prop::Val(false);

        // L525-535 / L636-649
        let stageCount = Self::add_count_edit(
            cx,
            me,
            key("CalcsStageCount").into(),
            Anchor::NONE,
            rect(0, 0, 52, 18),
            Rc::new(move |cx: &mut Cx, buf: &str| {
                // L526-530 / L637-643
                with_active_effect(build_of(cx).as_ref(), SkillList::Calcs, &|effect| {
                    // L531 / L644
                    src_of(effect).skillStageCountCalcs.set(pob_lua::tonumber(buf));
                    // L532 / L645
                    true
                });
            }),
        );
        // L536 / L650
        cx.base_mut(stageCount).shown = Prop::Val(false);

        // L538-548 / L652-665
        let mineCount = Self::add_count_edit(
            cx,
            me,
            key("CalcsMineCount").into(),
            Anchor::NONE,
            rect(0, 0, 52, 18),
            Rc::new(move |cx: &mut Cx, buf: &str| {
                // L539-543 / L653-659
                with_active_effect(build_of(cx).as_ref(), SkillList::Calcs, &|effect| {
                    // L544 / L660
                    src_of(effect).skillMineCountCalcs.set(pob_lua::tonumber(buf));
                    // L545 / L661
                    true
                });
            }),
        );
        // L549 / L666
        cx.base_mut(mineCount).shown = Prop::Val(false);

        // L551-554 / L668-674
        let showMinion = cx
            .add(
                me,
                key("CalcsShowMinion"),
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(0, 0, 18, 0),
                    None,
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L669-670
                        let Some(m) = build_of(cx) else { return };
                        // L552 / L671
                        m.calcsTab.input.set(n!("showMinion"), state);
                        // L553 / L672
                        m.buildFlag.set(true);
                    })),
                    Some(Prop::Val(Some("Show stats for the minion instead of the player.".to_owned()))),
                    false,
                ),
            )
            .id();
        // L555 / L675
        cx.base_mut(showMinion).shown = Prop::Val(false);

        // L557-574 / L677-697
        let minion_key = key("CalcsMinion");
        let minion = Self::add_dropdown(
            cx,
            me,
            minion_key.clone().into(),
            Anchor::NONE,
            rect(0, 0, 140, 18),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L558-562 / L678-684
                let dd = cx
                    .child(me, minion_key.clone())
                    .unwrap_or_else(|| lua_error!("attempt to index field '{minion_key}' (a nil value)"));
                let selected = minion_sel(cx, dd, index);
                with_active_effect(build_of(cx).as_ref(), SkillList::Calcs, &|effect| {
                    set_minion(effect, selected, SkillList::Calcs)
                });
            }),
        );
        // L575 / L698
        cx.base_mut(minion).shown = Prop::Val(false);

        // L577-587 / L700-713
        let minionSkill = Self::add_dropdown(
            cx,
            me,
            key("CalcsMinionSkill").into(),
            Anchor::NONE,
            rect(0, 0, 140, 18),
            Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                // L578-582 / L701-707
                with_active_effect(build_of(cx).as_ref(), SkillList::Calcs, &|effect| {
                    // L583 / L708
                    src_of(effect).skillMinionSkillCalcs.set(Some(index as f64));
                    // L584 / L709
                    true
                });
            }),
        );
        // L588 / L714
        cx.base_mut(minionSkill).shown = Prop::Val(false);

        // L590-593 / L716-722
        let mode = cx
            .add(
                me,
                key("CalcsMode"),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 120, 18),
                    buffModes,
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        // L717-718
                        let Some(m) = build_of(cx) else { return };
                        // L591 / L719 value.buffMode
                        let value = value
                            .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                        let bm = value.get::<BuffModeValue>().map_or(Val::Nil, |v| Val::from(v.buffMode));
                        m.calcsTab.input.set(n!("misc_buffMode"), bm);
                        // L592 / L720
                        m.buildFlag.set(true);
                    })),
                    None,
                    false,
                ),
            )
            .id();
        // L594 / L723
        cx.base_mut(mode).shown = Prop::Val(false);
    }
}

/// `EditControl(nil, rect, "", "Search", "%c", 100, changeFunc, nil, nil, true)` (the tree
/// search edits L773, L874, L923).
fn search_args(change: Rc<dyn Fn(&mut Cx, &str)>) -> EditArgs {
    EditArgs {
        init: Some(String::new()),
        prompt: Some("Search".to_owned()),
        filter: Some("%c".to_owned()),
        limit: Some(100),
        change_func: Some(change),
        clearable: true,
        ..EditArgs::default()
    }
}

/// `if self.primaryBuild.treeTab and self.primaryBuild.treeTab.viewer then
/// self.primaryBuild.treeTab.viewer.searchStr = buf end` (L774-776, L875-877).
fn set_primary_viewer_search(cx: &mut Cx, buf: &str) {
    if let Some(tt) = crate::tabs::tree::core::tab(cx) {
        cx.w_mut::<crate::tabs::tree::TreeTab>(tt).core.viewer.search_str = buf.to_owned();
    }
}

/// The primary build's tree set selectors (`primaryTreeSetSelect` L821-826, `leftSpecSelect`
/// L856-861): `if self.primaryBuild.treeTab and self.primaryBuild.treeTab.specList[index] then
/// self.primaryBuild.modFlag = true; self.primaryBuild.treeTab:SetActiveSpec(index) end`.
fn sel_primary_spec(cx: &mut Cx, index: usize) {
    let m = primary(cx);
    // L822 / L857
    if m.treeTab.specList.with(|l| at_num(l, index as f64)).is_none() {
        return;
    }
    // L823 / L858
    cx.build_mut().mod_flag = true;
    // L824 / L859
    let f = build_init::hooks(cx).tree_set_active_spec;
    f(cx, index as f64);
}

// ---- helpers of the class (L1073-1242) -------------------------------------------------------

/// The set lists PopulateSetDropdown reads (`tab`, `orderListField`, `setsField`,
/// `activeIdField`): the call sites pass `itemsTab, "itemSetOrderList", "itemSets",
/// "activeItemSetId"`, the skill and the config equivalents.
#[derive(Clone, Copy)]
pub enum SetTab<'a> {
    Items(&'a model::ItemsTab),
    Skills(&'a model::SkillsTab),
    Config(&'a model::ConfigTab),
}

impl SetTab<'_> {
    /// `tab[orderListField]`, `tab[setsField][setId].title` per entry, `tab[activeIdField]`
    #[allow(clippy::option_option)] // nil set / set without a title
    fn read(&self) -> (Vec<u32>, Vec<Option<Option<String>>>, f64) {
        let title = |t: &Table| t.str(n!("title")).map(|s| s.to_string());
        match self {
            SetTab::Items(t) => {
                let order = t.itemSetOrderList.with(Clone::clone);
                let sets = order.iter().map(|id| t.itemSets.with(|s| s.get(id).map(title))).collect();
                (order, sets, t.activeItemSetId.get())
            }
            SetTab::Skills(t) => {
                let order = t.skillSetOrderList.with(Clone::clone);
                let sets = order.iter().map(|id| t.skillSets.with(|s| s.get(id).map(title))).collect();
                (order, sets, t.activeSkillSetId.get())
            }
            SetTab::Config(t) => {
                let order = t.configSetOrderList.with(Clone::clone);
                let sets = order.iter().map(|id| t.configSets.with(|s| s.get(id).map(title))).collect();
                (order, sets, t.activeConfigSetId.get())
            }
        }
    }
}

impl CompareTab {
    /// lua: CompareTab.lua:1073-1081 `CompareTabClass:GetShortBuildName(fullName)`
    /// Get a short display name from a build name (strips "AccountName - " prefix)
    /// @pob Classes/CompareTab.lua:1073-1081 h=2f8a215f
    pub fn GetShortBuildName(fullName: Option<&str>) -> String {
        // L1075
        let Some(fullName) = fullName else { return "Your Build".to_owned() };
        // L1076
        let dashPos = pob_lua::pattern::find(fullName, " %- ", None, false).map(|(a, _, _)| a);
        // L1077
        if let Some(dashPos) = dashPos {
            // L1078
            return pob_ui_core::luastr::sub(fullName, dashPos as f64 + 3.0, None).into_owned();
        }
        // L1080
        fullName.to_owned()
    }

    /// lua: CompareTab.lua:1083-1102 `CompareTabClass:PopulateSetDropdown(tab, orderListField,
    /// setsField, activeIdField, control)`
    /// Populate a set-selector dropdown from a tab's ordered set list.
    /// tab: the tab object (e.g. itemsTab, skillsTab, configTab)
    /// orderListField/setsField/activeIdField: string keys on tab
    /// control: the DropDownControl to populate
    /// @pob Classes/CompareTab.lua:1083-1102 h=7ff2d987
    pub fn PopulateSetDropdown(cx: &mut Cx, tab: SetTab<'_>, control: CtrlId) {
        // L1088
        let mut list: Vec<DropItem> = Vec::new();
        // L1089-1091
        let (orderList, sets, activeId) = tab.read();
        // L1092 (the model's order lists always exist)
        // L1093
        for (i0, setId) in orderList.iter().enumerate() {
            let index = i0 + 1;
            // L1094 local set = sets[setId]
            // L1095 (set and set.title) or "Default"
            let label = sets[i0].clone().flatten().unwrap_or_else(|| "Default".to_owned());
            list.push(DropItem::str(label));
            // L1096
            if f64::from(*setId) == activeId {
                // L1097
                cx.w_mut::<DropDownControl>(control).sel_index = index;
            }
        }
        // L1101
        DropDownControl::set_list(cx, control, list);
    }

    /// lua: CompareTab.lua:1104-1120 `CompareTabClass:FormatConfigValue(varData, val)`
    /// Format a config value for read-only display
    /// @pob Classes/CompareTab.lua:1104-1120 h=86f8b2bc
    pub fn FormatConfigValue(cx: &Cx, varData: &ConfigOptionDef, val: &Val) -> String {
        // L1106
        if val.is_nil() {
            return "^8(not set)".to_owned();
        }
        let ty = varData.r#type.as_deref();
        // L1107
        if ty == Some("check") {
            // L1108
            if val.truthy() {
                format!("{}Yes", cx.main.cc.POSITIVE)
            } else {
                format!("{}No", cx.main.cc.NEGATIVE)
            }
        // L1109
        } else if let (Some("list"), Some(list)) = (ty, varData.list.as_ref()) {
            // L1110
            for item in list {
                // L1111
                if Val::from_value(&item.val).raw_eq(val) {
                    // L1112 (`item.label` is always set)
                    return item.label.to_string();
                }
            }
            // L1115
            tostring_val(val)
        } else {
            // L1117
            tostring_val(val)
        }
    }

    /// lua: CompareTab.lua:1122-1130 `CompareTabClass:NormalizeConfigVals(varData, pVal, cVal)`
    /// Normalize config values so that functionally equivalent states compare equal
    /// (nil/false for checks, nil/0 for counts/integers/floats)
    /// @pob Classes/CompareTab.lua:1122-1130 h=9efc6d0b
    pub fn NormalizeConfigVals(varData: &ConfigOptionDef, pVal: Val, cVal: Val) -> (Val, Val) {
        let or = |v: Val, d: Val| if v.truthy() { v } else { d };
        let ty = varData.r#type.as_deref();
        // L1125
        if ty == Some("check") {
            // L1126
            return (or(pVal, Val::Bool(false)), or(cVal, Val::Bool(false)));
        // L1127
        } else if matches!(ty, Some("count" | "integer" | "float")) {
            // L1128
            return (or(pVal, Val::Num(0.0)), or(cVal, Val::Num(0.0)));
        }
        // L1130
        (pVal, cVal)
    }

    /// lua: CompareTab.lua:1132-1176 `makeConfigControl(varData, inputTable, configTab,
    /// buildObj, sourceControl)` (a module local)
    /// Create a single config control for a given varData, writing to the specified
    /// input/configTab/build. The control is created as a free node of the tab (`me`); the
    /// caller stores it.
    /// @pob Classes/CompareTab.lua:1132-1176 h=e40f98e9
    fn makeConfigControl(
        cx: &mut Cx,
        me: CtrlId,
        varData: &'static ConfigOptionDef,
        target: Target,
        sourceControl: Option<(CtrlId, VarClass)>,
    ) -> Option<(CtrlId, VarClass)> {
        let build = target.model(cx);
        let inputTable = build.configTab.input.clone();
        let var = varData.var.unwrap_or_else(|| lua_error!("table index is nil"));
        // the callbacks' `inputTable[varData.var] = v; configTab:UpdateControls();
        // configTab:BuildModList(); buildObj.buildFlag = true`
        let after = {
            let target = target.clone();
            move |cx: &mut Cx, v: Val| {
                let build = target.model(cx);
                build.configTab.input.set(var, v);
                match &target {
                    Target::Primary => {
                        if let Some(ct) = crate::tabs::config::core::tab(cx) {
                            let f = config_hooks().update_controls;
                            f(cx, ct);
                        }
                    }
                    Target::Entry(e) => {
                        let f = compare_hooks().entry_config_update_controls;
                        f(cx, e);
                    }
                }
                build.configTab.BuildModList(&build);
                build.buildFlag.set(true);
            }
        };
        // L1133 local control
        let control: Option<(CtrlId, VarClass)>;
        // L1134
        let pVal = inputTable.get(var);
        let ty = varData.r#type.as_deref();
        // L1135
        if ty == Some("check") {
            // L1136-1141
            let c = cx
                .add_free(
                    Some(me),
                    CheckBoxControl::new(
                        Anchor::NONE,
                        rect(0, 0, 18, 0),
                        None,
                        Some(Rc::new(move |cx: &mut Cx, state: bool| {
                            // L1137-1140
                            after(cx, Val::Bool(state));
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L1142
            cx.w_mut::<CheckBoxControl>(c).state = pVal.truthy();
            control = Some((c, VarClass::CheckBox));
        // L1143-1144
        } else if matches!(ty, Some("count" | "integer" | "countAllowZero" | "float")) {
            // L1145-1146
            let filter = if ty == Some("integer") {
                "^%-%d"
            } else if ty == Some("float") {
                "^%d."
            } else {
                "%D"
            };
            // L1147-1155
            let init = tostring_val(&if pVal.truthy() { pVal.clone() } else { Val::from("") });
            let c = cx
                .add_free(
                    Some(me),
                    EditControl::new(
                        Anchor::NONE,
                        rect(0, 0, 90, 18),
                        EditArgs {
                            init: Some(init),
                            filter: Some(filter.to_owned()),
                            limit: Some(7),
                            change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                                // L1151-1154
                                after(cx, Val::from(pob_lua::tonumber(buf)));
                            })),
                            ..EditArgs::default()
                        },
                    ),
                )
                .id();
            control = Some((c, VarClass::Edit));
        // L1156
        } else if let (Some("list"), Some(list)) = (ty, varData.list.as_ref()) {
            // L1157-1163 (the list's entries are `varData.list`'s)
            let items: Vec<DropItem> = list
                .iter()
                .map(|item| DropItem::new(item.label.to_string(), Rc::new(Val::from_value(&item.val))))
                .collect();
            let c = cx
                .add_free(
                    Some(me),
                    DropDownControl::new(
                        Anchor::NONE,
                        rect(0, 0, 150, 18),
                        items,
                        Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                            // L1159 value.val
                            let value = value
                                .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
                            let v = value.get::<Val>().cloned().unwrap_or(Val::Nil);
                            // L1159-1162
                            after(cx, v);
                        })),
                        None,
                        false,
                    ),
                )
                .id();
            // L1164
            let want = if pVal.truthy() {
                pVal.clone()
            } else {
                list.first().map_or(Val::Nil, |i| Val::from_value(&i.val))
            };
            DropDownControl::sel_by_value(cx, c, &|item: &DropItem| {
                item.get::<Val>().is_some_and(|v| v.raw_eq(&want))
            });
            control = Some((c, VarClass::DropDown));
        } else {
            control = None;
        }
        // L1166
        if let Some((c, class)) = control {
            // L1167
            cx.base_mut(c).shown = Prop::f(|_, _| false);
            // L1168-1170 Reuse tooltip behavior from the source ConfigTab control so compare
            // view matches normal Config tab hover help (including dynamic tooltip funcs).
            // L1171
            if let Some((src, src_class)) = sourceControl {
                // L1172-1173
                copy_tooltip(cx, src, src_class, c, class);
            }
        }
        // L1176
        control
    }

    /// lua: CompareTab.lua:1178-1228 `CompareTabClass:RebuildConfigControls(compareEntry)`
    /// Rebuild interactive config controls for all config options (both primary and compare
    /// builds)
    /// @pob Classes/CompareTab.lua:1178-1228 h=f932d4c5
    pub fn RebuildConfigControls(cx: &mut Cx, me: CtrlId, compareEntry: Option<&CompareEntryRef>) {
        // L1180 Remove old config controls
        // L1181 ORDER: see the module notes
        let vars: Vec<Name> = st(cx, me).configControls.keys().copied().collect();
        for var in vars {
            // L1182
            cx.remove_child(me, &CtrlKey::from(format!("cfg_p_{var}")));
            // L1183
            cx.remove_child(me, &CtrlKey::from(format!("cfg_c_{var}")));
        }
        let s = st_mut(cx, me);
        // L1185
        s.configControls = IndexMap::new();
        // L1186
        s.configControlList = Vec::new();
        // L1187
        s.configSections = Vec::new();

        // L1189
        let Some(compareEntry) = compareEntry else { return };

        // L1191
        let configOptions = st(cx, me).configOptions;
        // L1192-1194 (`configTab.input` always exists)
        // L1196
        let mut currentSection: Option<Rc<ConfigSection>> = None;
        // L1197
        for varData in configOptions {
            // L1198
            if let Some(section) = varData.section.as_deref() {
                // L1199 Skip sections not relevant in Compare view
                // L1200
                if section != "Custom Modifiers" && section != "Map Modifiers and Player Debuffs" {
                    // L1201
                    let sec = Rc::new(ConfigSection {
                        name: section.to_owned(),
                        col: varData.col,
                        items: RefCell::new(Vec::new()),
                    });
                    // L1202
                    st_mut(cx, me).configSections.push(sec.clone());
                    currentSection = Some(sec);
                } else {
                    // L1204
                    currentSection = None;
                }
            // L1206
            } else if let (Some(sec), Some(var)) = (currentSection.clone(), varData.var) {
                if varData.r#type.as_deref() == Some("text") {
                    continue;
                }
                // L1207
                let pSource = crate::tabs::config::core::tab(cx).and_then(|ct| {
                    cx.w::<crate::tabs::config::ConfigTab>(ct)
                        .ctor
                        .var_controls
                        .get(&var)
                        .map(|v| (v.id, v.class))
                });
                // L1208
                let cSource = {
                    let f = compare_hooks().entry_config_var_control;
                    f(cx, compareEntry, var)
                };
                // L1209
                let pCtrl = Self::makeConfigControl(cx, me, varData, Target::Primary, pSource);
                // L1210
                let cCtrl = Self::makeConfigControl(cx, me, varData, Target::Entry(compareEntry.clone()), cSource);

                // L1212
                if let (Some((pCtrl, _)), Some((cCtrl, _))) = (pCtrl, cCtrl) {
                    // L1213 (a control a later option with the same var replaces stays in the
                    // lists: detach it instead of burying it)
                    let pk = CtrlKey::from(format!("cfg_p_{var}"));
                    let ck = CtrlKey::from(format!("cfg_c_{var}"));
                    cx.detach_child(me, &pk);
                    cx.put(me, pk, pCtrl);
                    // L1214
                    cx.detach_child(me, &ck);
                    cx.put(me, ck, cCtrl);

                    // L1216-1221
                    let ctrlInfo = Rc::new(ConfigCtrlInfo {
                        primaryControl: pCtrl,
                        compareControl: cCtrl,
                        varData,
                        visible: Cell::new(false),
                    });
                    let s = st_mut(cx, me);
                    // L1222
                    s.configControls.insert(var, ctrlInfo.clone());
                    // L1223
                    s.configControlList.push(ctrlInfo.clone());
                    // L1224
                    sec.items.borrow_mut().push(ctrlInfo);
                }
            }
        }
    }

    /// lua: CompareTab.lua:1230-1242 `CompareTabClass:CopyCompareConfig()`
    /// Copy all config settings from compare build to primary build
    /// @pob Classes/CompareTab.lua:1230-1242 h=8024a1ac
    pub fn CopyCompareConfig(cx: &mut Cx, me: CtrlId) {
        // L1232
        let Some(compareEntry) = Self::GetActiveCompare(cx, me) else { return };
        // L1234
        let cInput = compareEntry.model.configTab.input.clone();
        let p = primary(cx);
        // L1235 ORDER: see the module notes
        for (k, v) in cInput.pairs() {
            // L1236
            p.configTab.input.set(k, v);
        }
        // L1238
        if let Some(ct) = crate::tabs::config::core::tab(cx) {
            let f = config_hooks().update_controls;
            f(cx, ct);
        }
        // L1239
        p.configTab.BuildModList(&p);
        // L1240
        p.buildFlag.set(true);
        // L1241
        st_mut(cx, me).configNeedsRebuild = true;
    }
}

/// `control.tooltipText = sourceControl.tooltipText; control.tooltipFunc =
/// sourceControl.tooltipFunc` (L1172-1173), for controls of the same class (an option's
/// ConfigTab control and its compare controls are built from the same `varData.type`, so the
/// classes match; a different class would carry fields of another shape: nil is copied).
fn copy_tooltip(cx: &mut Cx, src: CtrlId, src_class: VarClass, dst: CtrlId, dst_class: VarClass) {
    match (src_class, dst_class) {
        (VarClass::CheckBox, VarClass::CheckBox) => {
            let (t, f) = {
                let w = cx.w::<CheckBoxControl>(src);
                (w.tt.tooltip_text.clone(), w.tt.tooltip_func.clone())
            };
            let w = cx.w_mut::<CheckBoxControl>(dst);
            w.tt.tooltip_text = t;
            w.tt.tooltip_func = f;
        }
        (VarClass::Edit, VarClass::Edit) => {
            let (t, f) = {
                let w = cx.w::<EditControl>(src);
                (w.tt.tooltip_text.clone(), w.tt.tooltip_func.clone())
            };
            let w = cx.w_mut::<EditControl>(dst);
            w.tt.tooltip_text = t;
            w.tt.tooltip_func = f;
        }
        (VarClass::DropDown, VarClass::DropDown) => {
            let (t, f) = {
                let w = cx.w::<DropDownControl>(src);
                (w.tt.tooltip_text.clone(), w.tt.tooltip_func.clone())
            };
            let w = cx.w_mut::<DropDownControl>(dst);
            w.tt.tooltip_text = t;
            w.tt.tooltip_func = f;
        }
        (_, VarClass::CheckBox) => {
            let w = cx.w_mut::<CheckBoxControl>(dst);
            w.tt.tooltip_text = None;
            w.tt.tooltip_func = None;
        }
        (_, VarClass::Edit) => {
            let w = cx.w_mut::<EditControl>(dst);
            w.tt.tooltip_text = None;
            w.tt.tooltip_func = None;
        }
        (_, VarClass::DropDown) => {
            let w = cx.w_mut::<DropDownControl>(dst);
            w.tt.tooltip_text = None;
            w.tt.tooltip_func = None;
        }
        _ => {}
    }
}

// ---- the build's hooks -----------------------------------------------------------------------

/// `BuildHooks::compare_tab`: `self.compareTab = new("CompareTab"):CompareTab(self)`
/// (Build.lua:624).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    cx.add_free(Some(build), CompareTab::new(build)).id()
}

/// The CompareTab node of the open build (`None` before its constructor ran).
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.compare.id();
    cx.ui.try_w::<CompareTab>(id).map(|_| id)
}

/// `ImportHooks::compare_import_build` (U43, CCR-0109 §5): `self.build.compareTab:ImportBuild(
/// xml, label)` (ImportTab.lua:658).
pub fn import_build_hook(cx: &mut Cx, xml: Option<&str>, label: &str) -> bool {
    let me = tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'compareTab' (a nil value)"));
    CompareTab::ImportBuild(cx, me, xml, Some(label))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn short_build_name_strips_the_account() {
        assert_eq!(CompareTab::GetShortBuildName(None), "Your Build");
        assert_eq!(CompareTab::GetShortBuildName(Some("Acc - My Build")), "My Build");
        assert_eq!(CompareTab::GetShortBuildName(Some("A - B - C")), "B - C");
        assert_eq!(CompareTab::GetShortBuildName(Some("NoDash")), "NoDash");
        assert_eq!(CompareTab::GetShortBuildName(Some("a-b")), "a-b");
    }

    #[test]
    fn match_flags_follows_the_lua() {
        let flags = |f: &str| f == "attack" || f == "projectile";
        assert!(matchFlags(None, None, &flags));
        assert!(matchFlags(Some(FlagSpec::Str("attack")), None, &flags));
        assert!(!matchFlags(Some(FlagSpec::Str("spell")), None, &flags));
        assert!(matchFlags(Some(FlagSpec::List(&["attack", "projectile"])), None, &flags));
        assert!(!matchFlags(Some(FlagSpec::List(&["attack", "spell"])), None, &flags));
        assert!(!matchFlags(None, Some(FlagSpec::Str("attack")), &flags));
        assert!(matchFlags(None, Some(FlagSpec::List(&["spell", "minion"])), &flags));
    }

    #[test]
    fn at_num_is_a_lua_index() {
        let l = [10, 20, 30];
        assert_eq!(at_num(&l, 1.0), Some(10));
        assert_eq!(at_num(&l, 3.0), Some(30));
        assert_eq!(at_num(&l, 0.0), None);
        assert_eq!(at_num(&l, 4.0), None);
        assert_eq!(at_num(&l, 1.5), None);
        assert_eq!(at_num(&l, f64::NAN), None);
        assert_eq!(or_one(f64::NAN), 1.0);
        assert_eq!(or_one(2.0), 2.0);
    }
}
