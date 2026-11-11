//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Build.lua:1-770 (helpers, buildMode:Init).
//! Owner: U19.
//!
//! @pob-file Modules/Build.lua @16de4b82d
//!
//! `buildMode:Init` (Build.lua:64-727) interleaves model and UI statements. Its model half
//! (64-110, 603-698) is calc:BM-A's `build_xml::Init_model` (ui doc §10.2, CCR-0080 §1), which
//! runs in one call; this file runs the UI statements around it:
//!
//! 1. L65-76 (dbFileSubPath, `main:SetMode("LIST")`, saveAsSortMode);
//! 2. `Init_model` (L77-110, then L603-698 model part: LoadDB, tab models, section loaders,
//!    PostLoad, UpdateSocketGroups, outputRevision; the first BuildOutput is left to the facade);
//! 3. L111-602 (the controls), then the UI parts of 603-698 in PoB's order (tab UI constructors
//!    with the TreeTab constructor's `SetActiveSpec` UI lines, the breakdown control, L646
//!    UpdateClassDropdowns, the UI-only section loaders, each deferred Tree load's
//!    `SetActiveSpec` UI lines (CCR-0098 §6), BuildOutput, RefreshStatList), then 699-727.
//!
//! DIVERGENCE(init-order): the controls are created after the tab models and the section loads
//! instead of before them. Nothing is drawn during Init and the controls read only fields the
//! sections do not change (characterLevel, characterLevelAutoMode, buildName, viewMode), except
//! `self.spec` at L297, for which the previous build's spec is used as PoB does (the Lua module
//! object keeps it across builds). A section load that fails after L603 closes the build before
//! the controls exist (PoB creates them first; OnFrame then runs on them once).
//!
//! DIVERGENCE(default-char-level): `Init_model` reads `main.defaultCharLevel` as nil (CCR-0098 §2);
//! for a build without a `Build` section the level and its auto mode are set afterwards from
//! `main.defaultCharLevel` and `configTab:BuildModList()` runs once more (it only derives the
//! enemy level from them).
//!
//! Calls into units that are not ported yet (U20, U21, the tabs, U25's spec undo) go through
//! [`BuildHooks`] (CCR-0098 §3); `default_hooks` names the owner of each entry. The owners
//! replace their entry there.
//!
//! Props with side effects (ui doc §7.3): `controls.buildName.width` writes `self.strWidth` /
//! `self.strLimited` (Cells in [`State`]).
//!
//! ORDER: `pairs(self.controls.warnings.lines)` (L587, L595) iterates a sequence (InsertIfNew
//! appends), i.e. ascending; `pairs(self.savers)` (L679) is calc:BM-A's (only TreeTab has a
//! PostLoad).

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use pob_lua::math::{lpow, m_abs, m_floor, m_max, m_min};
use pob_lua::{n, tonumber, tostring_num};
use pob_sg::{Align, Font};
use pob_ui_core::controls::drop_down::{DropTipArgs, SelFunc};
use pob_ui_core::controls::text_list::{HoveredLine, TextListColumn};
use pob_ui_core::controls::{
    ButtonControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl, TextListControl,
};
use pob_ui_core::input::InputEvents;
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{Tooltip, TtKey, TtObj};

use super::{BuildMode, BuildState, TabIds};
use crate::app::CxApp;
use crate::main_mode::{self, ModeArg};
use crate::model;
use crate::model::build_xml;

// @pob-skip Modules/Build.lua:1-21 reason="module header: local aliases of globals (pob_lua::math is used directly)"

// ---- module-level helpers ----------------------------------------------------------------

/// lua: Build.lua:24-30 `local function InsertIfNew(t, val)`
/// @pob Modules/Build.lua:24-30 h=9511a2c5
pub fn insert_if_new<T: PartialEq>(t: Option<&mut Vec<T>>, val: T) {
    // L25
    let Some(t) = t else { return };
    // L26
    for v in t.iter() {
        // L27
        if *v == val {
            return;
        }
    }
    // L30
    t.push(val);
}

/// lua: Build.lua:32-62 `local function matchFlags(reqFlags, notFlags, flags)`.
/// A string flag argument is passed as a one-element slice (L39-41, L50-52); `flags(name)` is
/// `flags[name]` truthy. Lua returns `true` or nothing.
/// @pob Modules/Build.lua:32-62 h=f5b56dbe
pub fn match_flags(
    req_flags: Option<&[&str]>,
    not_flags: Option<&[&str]>,
    flags: impl Fn(&str) -> bool,
) -> bool {
    // L39-44
    if let Some(req_flags) = req_flags {
        // L43
        for flag in req_flags {
            // L44
            if !flags(flag) {
                // L45
                return false;
            }
        }
    }
    // L50-55
    if let Some(not_flags) = not_flags {
        // L54
        for flag in not_flags {
            // L55
            if flags(flag) {
                // L56
                return false;
            }
        }
    }
    // Both flag checks passed, default true
    // L61
    true
}

/// One entry of `acts` (Build.lua:729-764).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Act {
    pub level: f64,
    pub questPoints: f64,
}

/// lua: Build.lua:729-764 `local acts = { ... }` (`acts[i]` is `ACTS[i - 1]`; read by
/// EstimatePlayerProgress, U20).
/// @pob Modules/Build.lua:729-764 h=b47f4c47
pub const ACTS: [Act; 11] = [
    // https://www.poewiki.net/wiki/Passive_skill
    Act { level: 1.0, questPoints: 0.0 },
    // Act 1   : The Dweller of the Deep
    // Act 1   : The Marooned Mariner
    Act { level: 12.0, questPoints: 2.0 },
    // Act 1,2 : The Way Forward (Reward after reaching Act 2)
    // Act 2   : Through Sacred Ground (Fellshrine Reward 3.25)
    Act { level: 22.0, questPoints: 4.0 },
    // Act 3   : Victario's Secrets
    // Act 3   : Piety's Pets
    Act { level: 32.0, questPoints: 6.0 },
    // Act 4   : An Indomitable Spirit
    Act { level: 40.0, questPoints: 7.0 },
    // Act 5   : In Service to Science
    // Act 5   : Kitava's Torments
    Act { level: 44.0, questPoints: 9.0 },
    // Act 6   : The Father of War
    // Act 6   : The Puppet Mistress
    // Act 6   : The Cloven One
    Act { level: 50.0, questPoints: 12.0 },
    // Act 7   : The Master of a Million Faces
    // Act 7   : Queen of Despair
    // Act 7   : Kishara's Star
    Act { level: 54.0, questPoints: 15.0 },
    // Act 8   : Love is Dead
    // Act 8   : Reflection of Terror
    // Act 8   : The Gemling Legion
    Act { level: 60.0, questPoints: 18.0 },
    // Act 9   : Queen of the Sands
    // Act 9   : The Ruler of Highgate
    Act { level: 64.0, questPoints: 20.0 },
    // Act 10  : Vilenta's Vengeance
    // Act 10  : An End to Hunger (+2)
    Act { level: 67.0, questPoints: 23.0 },
];

/// lua: Build.lua:766-769 `local function actExtra(act, extra)`
/// @pob Modules/Build.lua:766-769 h=27810e4e
pub fn act_extra(act: f64, extra: f64) -> f64 {
    // Act 2 : Deal With The Bandits (+1 if the player kills all bandits)
    // L768
    if act > 2.0 {
        extra
    } else {
        0.0
    }
}

// ---- the build mode's own fields and control classes ----------------------------------------

/// `self.treeListSpecialLinks` etc.: `{ [linkName] = { setId = .. } }`, written by SyncLoadouts
/// (U20) and read by the buildLoadouts selFunc (L355-385).
#[derive(Clone, Debug, Default)]
pub struct SpecialLinks {
    /// LUA-ORDER: only looked up by key.
    pub links: indexmap::IndexMap<String, f64>,
}

/// U19's part of the buildMode object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.anchorTopBarLeft` (L121)
    pub anchor_top_bar_left: Option<CtrlId>,
    /// `self.anchorTopBarRight` (L189)
    pub anchor_top_bar_right: Option<CtrlId>,
    /// `self.anchorSideBar` (L436)
    pub anchor_side_bar: Option<CtrlId>,
    /// `self.strWidth`, written by `controls.buildName.width` (L154)
    pub str_width: Cell<f64>,
    /// `self.strLimited` (L155)
    pub str_limited: Cell<bool>,
    /// the `miscTooltip` local of Init (L118), shared by the instance `Draw`s
    pub misc_tooltip: Tooltip,
    /// `self.treeListSpecialLinks`, `itemListSpecialLinks`, `skillListSpecialLinks`,
    /// `configListSpecialLinks` (SyncLoadouts, U20)
    pub tree_list_special_links: SpecialLinks,
    pub item_list_special_links: SpecialLinks,
    pub skill_list_special_links: SpecialLinks,
    pub config_list_special_links: SpecialLinks,
    /// `self.targetVersion = nil` (L107): the frozen model field cannot hold nil.
    pub target_version_nil: bool,
}

/// `new("Control"):Control(..)` with the `str` / `req` fields that `controls.pointDisplay`
/// carries (L194; EstimatePlayerProgress, U20, writes them).
pub struct InfoControl {
    /// `control.str` (nil until written)
    pub str_: Option<String>,
    /// `control.req`
    pub req: Option<String>,
}
impl Widget for InfoControl {
    const NAME: &'static str = "Control";
    const CAPS: Caps = Caps::empty();
}

/// `control.str` of `controls.warnings`: nil, a string, or the `{}` the Draw stores (L600).
#[derive(Clone, Debug, Default, PartialEq)]
pub enum WarnStr {
    #[default]
    Nil,
    Str(String),
    Table,
}

/// `new("Control"):Control(..)` with the `lines` / `str` fields of `controls.warnings` (L579).
pub struct WarningsControl {
    /// `control.lines` (a sequence; InsertIfNew appends)
    pub lines: Vec<String>,
    pub str_: WarnStr,
}
impl Widget for WarningsControl {
    const NAME: &'static str = "Control";
    const CAPS: Caps = Caps::empty();
}

/// A `classDrop.list` entry `{ label = .., classId = .., ascendancies = { .. } }` (filled by
/// UpdateClassDropdowns, U20).
pub struct ClassListValue {
    pub class_id: f64,
    pub ascendancies: Vec<DropItem>,
}

/// An `ascendDrop` / `secondaryAscendDrop` entry `{ label = .., ascendClassId = .. }`; `extra`
/// holds the owner's other fields.
pub struct AscendListValue {
    pub ascend_class_id: f64,
    pub extra: Option<Rc<dyn std::any::Any>>,
}

/// A `mainSkillMinion` entry: `{ label = .., itemSetId = .. }` or `{ label = .., minionId = .. }`
/// (RefreshSkillSelectControls, U21).
pub struct MinionListValue {
    pub item_set_id: Option<f64>,
    pub minion_id: Option<model::Name>,
}

/// `controls.breakdown`'s fields that Init sets (L618-622); the constructor (U40) applies them.
pub struct BreakdownInit {
    /// `pinnedColour = hexToRGB(colorCodes.CUSTOM)`
    pub pinned_colour: [f64; 3],
    /// `envName = "mainEnv"`
    pub env_name: &'static str,
    /// `clearDisplayFunc = function() self:ClearDisplayStat() end`
    pub clear_display_func: Rc<dyn Fn(&mut Cx)>,
}

// ---- seams to the units that own the other buildMode methods and the tabs (CCR-0098 §3) ------

pub type Hook = Rc<dyn Fn(&mut Cx)>;
/// A tab's UI constructor (`new("NotesTab"):NotesTab(self)`): it creates the tab node (owned by
/// the build node, `cx.add_free(Some(build), ..)`) and returns its id.
pub type TabCtor = Rc<dyn Fn(&mut Cx, CtrlId) -> CtrlId>;

/// The methods U19's code calls that other units port. Each default names its owner; owners
/// replace their entry in [`default_hooks`] (one line each).
#[derive(Clone)]
pub struct BuildHooks {
    // U20 (Build.lua:771-1617, 2222-2335)
    /// `buildMode:OnFrame(inputEvents)`
    pub on_frame: Rc<dyn Fn(&mut Cx, &mut InputEvents, &ViewPort)>,
    /// `buildMode:Shutdown()`
    pub shutdown: Hook,
    /// `buildMode:CanExit(mode)`
    pub can_exit: Rc<dyn Fn(&mut Cx, &str) -> Option<bool>>,
    /// `self:CloseBuild()`
    pub close_build: Hook,
    /// `self:OpenSavePopup(mode)`
    pub open_save_popup: Rc<dyn Fn(&mut Cx, &str)>,
    /// `self:SaveDBFile()` (UI tails of the model's SaveDBFile)
    pub save_db_file: Hook,
    /// `self:OpenSaveAsPopup()`
    pub open_save_as_popup: Hook,
    /// `self:OpenConversionPopup()`
    pub open_conversion_popup: Hook,
    /// `self:SyncLoadouts()`
    pub sync_loadouts: Hook,
    /// `self:UpdateClassDropdowns(treeVersion)` (nil from Init, Build.lua:646)
    pub update_class_dropdowns: Rc<dyn Fn(&mut Cx, Option<model::Name>)>,
    /// `self:OpenSpectreLibrary()`
    pub open_spectre_library: Hook,
    // U21 (Build.lua:1618-2221)
    /// `self:RefreshStatList()`
    pub refresh_stat_list: Hook,
    /// `self:SetDisplayStat(displayData, pin)`
    pub set_display_stat: Rc<dyn Fn(&mut Cx, Option<HoveredLine>, bool)>,
    /// `self:ClearDisplayStat()`
    pub clear_display_stat: Hook,
    // U18
    /// `main.modes.LIST.subPath` (nil → `None`)
    pub list_sub_path: Rc<dyn Fn(&Cx) -> Option<String>>,
    // U25 (TreeTab UI, the PassiveSpec UndoHandler)
    /// `self.spec:AddUndoState()`
    pub spec_add_undo_state: Hook,
    /// `self.treeTab:GetSpecList()`
    pub tree_get_spec_list: Rc<dyn Fn(&Cx) -> Vec<String>>,
    /// `self.treeTab:SetActiveSpec(specId)` (model half + UI tail)
    pub tree_set_active_spec: Rc<dyn Fn(&mut Cx, f64)>,
    // U33/U34 (ItemsTab UI)
    /// `self.itemsTab:SetActiveItemSet(itemSetId)` (model half + UI tail)
    pub items_set_active_item_set: Rc<dyn Fn(&mut Cx, f64)>,
    /// `self.itemsTab:EquipItemInSet(item, itemSetId)`
    pub items_equip_item_in_set: Rc<dyn Fn(&mut Cx, &model::ItemRef, f64)>,
    /// `self.itemsTab:AddItemSetTooltip(tooltip, itemSet)`
    pub items_add_item_set_tooltip: Rc<dyn Fn(&mut Cx, &mut Tooltip, &model::Table)>,
    // U30 (SkillsTab UI)
    /// `self.skillsTab:SetActiveSkillSet(skillSetId)` (model half + UI tail)
    pub skills_set_active_skill_set: Rc<dyn Fn(&mut Cx, f64)>,
    /// `self.skillsTab:AddSocketGroupTooltip(tooltip, socketGroup)`
    pub skills_add_socket_group_tooltip:
        Rc<dyn Fn(&mut Cx, &mut Tooltip, &model::SocketGroupRef)>,
    // U42 (ConfigTab UI)
    /// `self.configTab:SetActiveConfigSet(configSetId)` (model half + UI tail)
    pub config_set_active_config_set: Rc<dyn Fn(&mut Cx, f64)>,
    // U43 (ImportTab UI)
    /// `self.importTab:TryFetchCharacterList()`
    pub import_try_fetch_character_list: Hook,
    /// `saver:Load(node, self.dbFileName)` for the UI-only savers (`Notes` U22, `TreeView`
    /// U25-U28, `Import` U43); `true` closes the build.
    pub load_ui_section: Rc<dyn Fn(&mut Cx, &str, &model::XmlNode, Option<&str>) -> bool>,
    // the tabs' UI constructors (L608-615, L625) and the breakdown control (L617-622, U40)
    pub import_tab: TabCtor,
    pub notes_tab: TabCtor,
    pub party_tab: TabCtor,
    pub config_tab: TabCtor,
    pub items_tab: TabCtor,
    pub tree_tab: TabCtor,
    pub skills_tab: TabCtor,
    pub calcs_tab: TabCtor,
    pub compare_tab: TabCtor,
    /// `self.controls.breakdown = new("CalcBreakdownControl"):CalcBreakdownControl(self.calcsTab)`
    /// plus L618-622: stores `controls.breakdown` in the build node.
    pub breakdown: Rc<dyn Fn(&mut Cx, CtrlId, BreakdownInit)>,
}

#[track_caller]
fn not_ported(what: &str, owner: &str) -> ! {
    lua_error!("{what} is not ported yet ({owner})")
}

/// A tab whose UI constructor is not ported yet: no node (`Ctrl::null()`).
fn pending_tab() -> TabCtor {
    Rc::new(|_cx, _build| CtrlId::default())
}

/// The placeholders installed until the owners land (CCR-0098 §3). Init calls
/// UpdateClassDropdowns, RefreshStatList and SyncLoadouts unconditionally, so those do nothing;
/// the user-triggered ones raise a Lua error that names the owner (PoB's error prompt shows it).
pub fn default_hooks() -> BuildHooks {
    BuildHooks {
        on_frame: Rc::new(super::on_frame::on_frame),
        shutdown: Rc::new(super::io::shutdown),
        can_exit: Rc::new(super::io::can_exit),
        close_build: Rc::new(super::io::close_build),
        open_save_popup: Rc::new(super::popups::open_save_popup),
        save_db_file: Rc::new(|cx| {
            super::io::save_db_file(cx);
        }),
        open_save_as_popup: Rc::new(super::popups::open_save_as_popup),
        open_conversion_popup: Rc::new(super::popups::open_conversion_popup),
        sync_loadouts: Rc::new(|cx| {
            super::loadouts::sync_loadouts(cx);
        }),
        update_class_dropdowns: Rc::new(super::class_drop::update_class_dropdowns),
        open_spectre_library: Rc::new(super::popups::open_spectre_library),
        // U21 (CCR-0098 §3)
        refresh_stat_list: Rc::new(super::stats::refresh_stat_list),
        // U20 (Build.lua:1353-1391 are U20's range)
        set_display_stat: Rc::new(super::on_frame::set_display_stat),
        clear_display_stat: Rc::new(super::on_frame::clear_display_stat),
        list_sub_path: Rc::new(|_| None),
        spec_add_undo_state: Rc::new(|_| {}),
        tree_get_spec_list: Rc::new(|_| not_ported("TreeTab:GetSpecList", "U25")),
        tree_set_active_spec: Rc::new(|cx, id| {
            let m = cx.build().model.clone();
            m.treeTab.SetActiveSpec(&m, id);
        }),
        items_set_active_item_set: Rc::new(|cx, id| {
            let m = cx.build().model.clone();
            m.itemsTab.SetActiveItemSet(&m, id);
        }),
        items_equip_item_in_set: Rc::new(|cx, item, id| {
            let m = cx.build().model.clone();
            m.itemsTab.EquipItemInSet(&m, item, id);
        }),
        items_add_item_set_tooltip: Rc::new(|_, _, _| {
            not_ported("ItemsTab:AddItemSetTooltip", "U34")
        }),
        skills_set_active_skill_set: Rc::new(crate::tabs::skills::core::set_active_skill_set_hook),
        skills_add_socket_group_tooltip: Rc::new(crate::tabs::skills::core::add_socket_group_tooltip_hook),
        config_set_active_config_set: Rc::new(|cx, id| {
            let m = cx.build().model.clone();
            m.configTab.SetActiveConfigSet(&m, Some(id), false);
        }),
        import_try_fetch_character_list: Rc::new(crate::tabs::import::core::try_fetch_character_list_hook),
        // one arm per UI-only saver (U22 Notes; TreeView U25-U28, Import U43); calc:BM-A keeps
        // the other sections and writes them back (CCR-0080 §8)
        load_ui_section: Rc::new(|cx, elem, node, db| match elem {
            "Notes" => crate::tabs::notes::load_ui_section(cx, node, db),
            "Import" => crate::tabs::import::core::load_ui_section(cx, node, db),
            _ => false,
        }),
        import_tab: Rc::new(crate::tabs::import::core::ctor_hook),
        notes_tab: Rc::new(crate::tabs::notes::ctor_hook),
        party_tab: Rc::new(crate::tabs::party::ctor_hook),
        config_tab: Rc::new(crate::tabs::config::ctor::ctor_hook),
        items_tab: Rc::new(crate::tabs::items::ctor::ctor_hook),
        tree_tab: pending_tab(),
        skills_tab: Rc::new(crate::tabs::skills::core::ctor_hook),
        calcs_tab: Rc::new(crate::tabs::calcs::core::ctor_hook),
        compare_tab: pending_tab(),
        // U40 (CalcBreakdownControl, Build.lua:616-621)
        breakdown: Rc::new(crate::tabs::calcs::breakdown::sidebar_breakdown_ctor),
    }
}

thread_local! {
    static HOOKS: RefCell<Option<BuildHooks>> = const { RefCell::new(None) };
    /// The class texts `PassiveSpec:SetWindowTitleWithBuildClass` sent while the model ran
    /// (see [`flush_window_titles`]).
    static TITLES: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn hooks(_cx: &Cx) -> BuildHooks {
    HOOKS.with(|h| h.borrow_mut().get_or_insert_with(default_hooks).clone())
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_hooks(f: impl FnOnce(&mut BuildHooks)) {
    HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_hooks)));
}

/// The placeholder `buildMode:OnFrame` until U20 (Build.lua:1160-1352): only the controls'
/// input and drawing (`ProcessControlsInput` L1253, `SetDrawLayer(5)` L1323, `DrawControls`
/// L1348), and the window titles the model sent. Not a port of OnFrame.
pub fn pending_on_frame(cx: &mut Cx, input_events: &mut InputEvents, _vp: &ViewPort) {
    let Some(node) = cx.app().build.as_ref().map(|b| b.node.id()) else {
        return;
    };
    flush_window_titles(cx);
    if cx.child(node, "pointDisplay").is_none() {
        // Init returned before creating the controls
        return;
    }
    // placeholder for EstimatePlayerProgress (U20), which sets the text before the first draw
    let pd = cx.child(node, "pointDisplay").unwrap();
    if cx.w::<InfoControl>(pd).str_.is_none() {
        cx.w_mut::<InfoControl>(pd).str_ = Some(String::new());
    }
    let vp = cx.main.view_port;
    cx.process_controls_input(node, input_events, &vp);
    cx.sg.set_draw_layer(Some(5), None);
    cx.draw_controls(node, &vp, None);
}

/// `self.spec:SetWindowTitleWithBuildClass()` (PassiveSpec.lua:2390-2396, calc:PS-C) from the
/// UI: the model sends the class text to its hook, the UI adds `self.build.buildName` and calls
/// `main:SetWindowTitleSubtext` (CCR-0072 §6).
pub fn set_window_title_with_build_class(cx: &mut Cx) {
    let spec = cx.build().model.spec.get();
    spec.SetWindowTitleWithBuildClass();
    flush_window_titles(cx);
}

/// Sends the titles the model queued (`main:SetWindowTitleSubtext(s_format("%s (%s)",
/// self.build.buildName, classText))`, PassiveSpec.lua:2395), in order. Callers that run model
/// code which may set the title (TreeTab:SetActiveSpec, PassiveSpec conversions) flush after it.
pub fn flush_window_titles(cx: &mut Cx) {
    let titles: Vec<String> = TITLES.with(|t| std::mem::take(&mut *t.borrow_mut()));
    for class_text in titles {
        let name = cx
            .app()
            .build
            .as_ref()
            .and_then(|b| b.build_name.clone())
            // LuaJIT's `%s` formats nil as "nil"
            .unwrap_or_else(|| "nil".to_owned());
        main_mode::folders::set_window_title_subtext(
            cx,
            Some(&format!("{name} ({class_text})")),
        );
    }
}

fn install_title_hook() {
    // CCR-0098 §4: model.rs (U15) does not re-export the hook
    pob_calc::classes::passive_spec_cluster::set_window_title_subtext_hook(Some(Box::new(|s: &str| {
        TITLES.with(|t| t.borrow_mut().push(s.to_owned()));
    })));
}

// ---- small accessors -------------------------------------------------------------------------

/// `self.buildName` as an operand of `..` (nil raises).
#[track_caller]
fn build_name_concat(cx: &Cx) -> String {
    cx.build()
        .build_name
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'buildName' (a nil value)"))
}

/// `self.controls[key]` (a missing control is a Lua nil index).
#[track_caller]
fn ctl(cx: &Cx, me: CtrlId, key: &'static str) -> CtrlId {
    cx.child(me, key)
        .unwrap_or_else(|| lua_error!("attempt to index field '{key}' (a nil value)"))
}

fn st(cx: &Cx, me: CtrlId) -> &State {
    &cx.w::<BuildMode>(me).init
}

#[track_caller]
fn anchor_top_bar_right(cx: &Cx, me: CtrlId) -> CtrlId {
    st(cx, me)
        .anchor_top_bar_right
        .unwrap_or_else(|| lua_error!("attempt to index field 'anchorTopBarRight' (a nil value)"))
}

/// L143-145 `local function buildNameConditional()`: conditional for smaller screens to move
/// "Current build" to the side bar.
fn build_name_conditional(cx: &Cx, me: CtrlId) -> bool {
    // L144
    cx.pos(anchor_top_bar_right(cx, me)).0 < 800.0
}

/// L191-193 `local function getPointDisplayX()` (I had it hardcoded to -323 before switching to
/// the control sizing)
fn get_point_display_x(cx: &Cx, me: CtrlId) -> f64 {
    // L192
    -(23.0
        + cx.size(ctl(cx, me, "pointDisplay")).0
        + cx.size(ctl(cx, me, "levelScalingButton")).0
        + cx.size(ctl(cx, me, "characterLevel")).0)
}

/// `self.modFlag = true`, `self.buildFlag = true` (the callbacks' common tail).
fn set_mod_and_build_flag(cx: &mut Cx) {
    cx.build_mut().mod_flag = true;
    cx.build().model.buildFlag.set(true);
}

/// `self.skillsTab.socketGroupList[i]` (nil for a non-integer or out-of-range index).
fn socket_group(cx: &Cx, i: f64) -> Option<model::SocketGroupRef> {
    if i.fract() != 0.0 || i < 1.0 {
        return None;
    }
    cx.build()
        .model
        .skillsTab
        .socketGroupList
        .get()
        .get(i as usize)
}

/// `mainSocketGroup.displaySkillList[mainSocketGroup.mainActiveSkill].activeEffect` with
/// `mainSocketGroup = self.skillsTab.socketGroupList[self.mainSocketGroup]`; a nil on the way is
/// the Lua index error.
#[track_caller]
fn main_active_effect(cx: &Cx) -> std::rc::Rc<pob_calc::active_skill::SkillEffect> {
    let msg = cx.build().model.mainSocketGroup.get();
    let group = socket_group(cx, msg)
        .unwrap_or_else(|| lua_error!("attempt to index local 'mainSocketGroup' (a nil value)"));
    let list = group
        .displaySkillList
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'displaySkillList' (a nil value)"));
    let mas = group.mainActiveSkill.get();
    let skill = if mas.fract() == 0.0 && mas >= 1.0 {
        list.get(mas as usize)
    } else {
        None
    }
    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    skill.activeEffect.get()
}

/// `…activeEffect.srcInstance` (L499, L511, L522, L534, L561).
#[track_caller]
fn main_src_instance(cx: &Cx) -> model::GemInstanceRef {
    main_active_effect(cx)
        .srcInstance
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'srcInstance' (a nil value)"))
}

/// The string of a dropdown string entry (`value` in the buildLoadouts selFunc).
fn drop_str(v: Option<&DropItem>) -> Option<String> {
    v.map(|d| d.as_str().map_or_else(|| d.label.clone(), str::to_owned))
}

/// L361-373 `findSetId(setOrderList, value, sets, setSpecialLinks)`: item, skill, and config sets
/// have identical structure; return id as soon as it's found.
#[allow(clippy::redundant_else)] // the Lua if/else
fn find_set_id(
    set_order_list: &[u32],
    value: Option<&str>,
    sets: &indexmap::IndexMap<u32, model::Table>,
    set_special_links: &SpecialLinks,
) -> Option<f64> {
    // L362
    for set_order in set_order_list {
        // L366
        let set = sets
            .get(set_order)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        let title = set
            .str(n!("title"))
            .map_or_else(|| "Default".to_owned(), |s| s.to_string());
        if value == Some(title.as_str()) {
            // L367
            return Some(f64::from(*set_order));
        } else {
            // L363
            let link_match = link_match_value(value);
            // L364
            if let Some(link_match) = link_match {
                // L368
                return Some(special_link(set_special_links, &link_match));
            }
        }
    }
    // L372
    None
}

/// L377-389 `findNamedSetId(treeList, value, setSpecialLinks)`: trees have a different structure
/// with id/name pairs; return id as soon as it's found.
#[allow(clippy::redundant_else)] // the Lua if/else
fn find_named_set_id(
    tree_list: &[String],
    value: Option<&str>,
    set_special_links: &SpecialLinks,
) -> Option<f64> {
    // L380
    for (id, spec) in tree_list.iter().enumerate() {
        // L378
        if value == Some(spec.as_str()) {
            // L379
            return Some((id + 1) as f64);
        } else {
            // L384
            let link_match = link_match_value(value);
            // L382
            if let Some(link_match) = link_match {
                // L383
                return Some(special_link(set_special_links, &link_match));
            }
        }
    }
    // L388
    None
}

/// `string.match(value, "%{(%w+)%}")` with the selFunc's `value` (nil raises, as
/// `string.match` does; it is only reached inside the find loops, L363/L384).
#[track_caller]
fn link_match_value(value: Option<&str>) -> Option<String> {
    link_match(
        value.unwrap_or_else(|| lua_error!("bad argument #1 to 'match' (string expected, got nil)")),
    )
}

/// `string.match(value, "%{(%w+)%}")`
fn link_match(value: &str) -> Option<String> {
    let caps = pob_lua::lmatch(value, "%{(%w+)%}", None)?;
    match caps.first() {
        Some(pob_lua::pattern::Capture::Str(s)) => Some(String::from_utf8_lossy(s).into_owned()),
        _ => None,
    }
}

/// `setSpecialLinks[linkMatch]["setId"]` (a missing link is the Lua index error).
#[track_caller]
fn special_link(links: &SpecialLinks, key: &str) -> f64 {
    *links
        .links
        .get(key)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

/// `#t` of an id-keyed set table (`#self.itemsTab.itemSets`, L326): the border of the ids.
/// LuaJIT's border on a table with holes is layout dependent; the sets PoB creates are dense
/// (ids 1..n) unless one was deleted.
fn id_table_len(ids: impl Iterator<Item = u32>) -> f64 {
    let ids: std::collections::BTreeSet<u32> = ids.collect();
    let mut n = 0u32;
    while ids.contains(&(n + 1)) {
        n += 1;
    }
    f64::from(n)
}

// ---- buildMode:Init ------------------------------------------------------------------------

/// The class callbacks' common tail (L262-265 and the two popup branches).
fn select_class(cx: &mut Cx, class_id: f64) {
    let spec = cx.build().model.spec.get();
    // L262 self.spec:SelectClass(value.classId)
    spec.SelectClass(class_id);
    // L263 self.spec:AddUndoState()
    let f = hooks(cx).spec_add_undo_state;
    f(cx);
    // L264 self.spec:SetWindowTitleWithBuildClass()
    set_window_title_with_build_class(cx);
    // L265 self.buildFlag = true
    cx.build().model.buildFlag.set(true);
}

/// lua: Build.lua:64-727 `buildMode:Init(dbFileName, buildName, buildXML, convertBuild,
/// importLink)` (model part: calc:BM-A `build_xml::Init_model`, 64-110 and 603-698).
pub fn init(cx: &mut Cx, args: &[ModeArg]) {
    let a_db_file_name = main_mode::mode_arg(args, 1);
    let a_build_name = main_mode::mode_arg(args, 2);
    let a_build_xml = main_mode::mode_arg(args, 3);
    let a_convert_build = main_mode::mode_arg(args, 4);
    let a_import_link = main_mode::mode_arg(args, 5);
    let db_file_name = a_db_file_name.as_str().map(str::to_owned);
    let build_name = a_build_name.as_str().map(str::to_owned);
    let import_link = a_import_link.as_str().map(str::to_owned);
    let h = hooks(cx);

    // `self.spec` of the previous build (the Lua module object keeps it; L297 reads it)
    let prev_secondary = cx.app().build.as_ref().map(|b| {
        let v = b.model.spec.get().curSecondaryAscendClassId.get();
        if v.is_nan() {
            0.0
        } else {
            v
        }
    });
    if let Some(old) = cx.app_mut().build.take() {
        cx.bury(old.node);
    }
    let node: Ctrl<BuildMode> = cx.add_free(
        None,
        New::new(Base::new(Anchor::NONE, rect(0, 0, 0, 0)), BuildMode::default()).host(),
    );
    let me = node.id();
    let model_ref = model::Build::new(model::data());
    let calc = (cx.app().calc_factory)();
    cx.app_mut().build = Some(BuildState {
        node,
        tabs: TabIds::default(),
        model: model_ref.clone(),
        calc,
        calc_func: None,
        calc_base: None,
        mod_flag: false,
        db_file_name: None,
        db_file_name_is_false: false,
        db_file_sub_path: None,
        build_name: None,
        import_link: None,
        save_as_sort_mode: String::new(),
        abort_save: None,
        unsaved: false,
    });
    install_title_hook();
    // U21: Build:Save's display stats (CCR-0080 §8)
    super::stats::install_save_stats_hook();

    // L65
    cx.build_mut().db_file_name = db_file_name.clone();
    cx.build_mut().db_file_name_is_false = matches!(a_db_file_name, ModeArg::Bool(false));
    // L66
    cx.build_mut().build_name = build_name.clone();
    // L67
    cx.build_mut().import_link = import_link.clone();
    // L68
    if let Some(db) = &db_file_name {
        // L69
        let build_path_len = cx.app().main.build_path.as_deref().map_or_else(
            || lua_error!("attempt to get length of field 'buildPath' (a nil value)"),
            str::len,
        );
        let name_len = build_name.as_deref().map_or_else(
            || lua_error!("attempt to get length of field 'buildName' (a nil value)"),
            str::len,
        );
        let sub = pob_lua::string::sub(db, build_path_len as i64 + 1, Some(-(name_len as i64) - 5))
            .into_owned();
        cx.build_mut().db_file_sub_path = Some(sub);
    } else {
        // L71
        let sub = (h.list_sub_path)(cx).unwrap_or_default();
        cx.build_mut().db_file_sub_path = Some(sub);
    }
    // L73
    if build_name.is_none() {
        // L74
        main_mode::on_frame::set_mode(cx, "LIST", Vec::new());
    }
    // L76
    cx.build_mut().save_as_sort_mode = "NAME".to_owned();

    // L78-110 (model part: calc:BM-A Init_model; continues with L603-698's model part)
    let closes = build_xml::Init_model(
        &model_ref,
        db_file_name.as_deref(),
        build_name.as_deref(),
        a_build_xml.as_str(),
        a_convert_build.truthy(),
        import_link.as_deref(),
        false,
    );
    // L94 / L100 self.modFlag (BM-A's load-time value)
    cx.build_mut().mod_flag = build_xml::build_extra(&model_ref).truthy(n!("modFlag"));
    // LoadDBFile clears self.dbFileName when the file cannot be opened (L2269); LoadDB may set
    // self.importLink (L2245)
    if build_xml::dbFileName(&model_ref).is_none() && db_file_name.is_some() {
        cx.build_mut().db_file_name = None;
    }
    cx.build_mut().import_link = build_xml::importLink(&model_ref).map(|s| s.to_string());
    if closes {
        // L106
        if model_ref.targetVersion.get() != model::data().gameVersions.liveTargetVersion {
            // L107
            cx.w_mut::<BuildMode>(me).init.target_version_nil = true;
            // L108
            let f = h.open_conversion_popup.clone();
            f(cx);
        } else {
            // L91 / L97 / L666 / L675 self:CloseBuild()
            let f = h.close_build.clone();
            f(cx);
        }
        // L92 / L98 / L109 / L667 / L676
        return;
    }
    // DIVERGENCE(default-char-level), CCR-0098 §2: L83 / L88 with main.defaultCharLevel
    if let Some(dcl) = cx.app().main.default_char_level {
        let has_build_section = build_xml::xmlSectionList(&model_ref)
            .iter()
            .any(|n| &*n.elem == "Build");
        if !has_build_section {
            model_ref.characterLevel.set(m_min(m_max(dcl, 1.0), 100.0));
            build_xml::build_extra(&model_ref)
                .set(n!("characterLevelAutoMode"), dcl == 1.0);
            model_ref.configTab.BuildModList(&model_ref);
        }
    }

    init_controls(cx, me, &h, build_name.as_deref(), prev_secondary);

    // Initialise build components (UI parts of L603-698)
    // L605-607 (model part: calc:BM-A)
    // L608 self.importTab = new("ImportTab"):ImportTab(self)
    let import = (h.import_tab)(cx, me);
    // L609
    let notes = (h.notes_tab)(cx, me);
    // L610 (model part: calc:BM-A)
    let party = (h.party_tab)(cx, me);
    // L611 (model part: calc:BM-A)
    let config = (h.config_tab)(cx, me);
    // L612 (model part: calc:BM-A)
    let items = (h.items_tab)(cx, me);
    // L613 (model part: calc:BM-A). CCR-0098 §6: the UI lines of the constructor's
    // `SetActiveSpec(1)` (TreeTab.lua:37 -> :556-557), which Init_model ran; they come after
    // the ~~temp~~ rename (L421-428), as in PoB.
    tree_tab_ctor_set_active_spec_ui_tail(cx, &h, &model_ref);
    let tree = (h.tree_tab)(cx, me);
    // L614 (model part: calc:BM-A)
    let skills = (h.skills_tab)(cx, me);
    // L615 (model part: calc:BM-A)
    let calcs = (h.calcs_tab)(cx, me);
    // L616-621
    let pinned_colour = pob_ui_core::color_codes::hex_to_rgb(&cx.main.cc.CUSTOM)
        .unwrap_or_else(|| lua_error!("failed to set breakdown pin colour"));
    let clear_display = h.clear_display_stat.clone();
    (h.breakdown)(
        cx,
        me,
        BreakdownInit {
            pinned_colour,
            env_name: "mainEnv",
            // L619-621
            clear_display_func: Rc::new(move |cx| clear_display(cx)),
        },
    );
    // L622-623 ensure that we don't refer to outdated calc tab sections: self.breakdownIndex =
    // nil (U21's field, fresh with the node)
    // L624
    let compare = (h.compare_tab)(cx, me);
    // L625-626 Used for pined calcs panes: self.overlayPanes = { } (U20's field, fresh)
    cx.build_mut().tabs = TabIds {
        tree: Ctrl::from_raw(tree),
        skills: Ctrl::from_raw(skills),
        items: Ctrl::from_raw(items),
        calcs: Ctrl::from_raw(calcs),
        config: Ctrl::from_raw(config),
        import: Ctrl::from_raw(import),
        notes: Ctrl::from_raw(notes),
        party: Ctrl::from_raw(party),
        compare: Ctrl::from_raw(compare),
    };

    // Load sections from the build file
    // L629-642 self.savers / self.legacyLoaders (calc:BM-A's SAVERS)

    // special rebuild to properly initialise boss placeholders
    // L645 (model part: calc:BM-A)
    // L646
    let f = h.update_class_dropdowns.clone();
    f(cx, None);
    // L648-656 (model part: calc:BM-A)
    // L657-670 the UI-only savers' Load, in document order
    let db = cx.build().db_file_name.clone();
    for node in build_xml::xmlSectionList(&model_ref) {
        let elem = node.elem.to_string();
        if matches!(elem.as_str(), "Notes" | "TreeView" | "Import") {
            // L665
            if (h.load_ui_section)(cx, &elem, &node, db.as_deref()) {
                // L666
                let f = h.close_build.clone();
                f(cx);
                // L667
                return;
            }
        }
    }

    // L671-676 (model part: calc:BM-A). CCR-0098 §6: the UI lines of the `SetActiveSpec` at
    // the end of each deferred `Tree` section's `TreeTab:Load` (TreeTab.lua:525 -> :556-557)
    tree_tab_load_set_active_spec_ui_tails(cx, &h, &model_ref);
    // L677-694 (model part: calc:BM-A) up to the first BuildOutput
    // L695
    let calc_model = model_ref.clone();
    let _ = cx.build_mut().calc.build_output(&calc_model);
    // CalcsTab.lua:464-467, the UI tail of BuildOutput (CCR-0111 §1, applied by U40)
    crate::tabs::calcs::core::build_output_ui_tail(cx);
    // L696
    let f = h.refresh_stat_list.clone();
    f(cx);
    // L697
    model_ref.buildFlag.set(false);

    // Init's UI tail (L699, L725-727; L603-698 is `Init_model`'s segment)
    // @pob Modules/Build.lua:699-699 h=ee9e4c06
    // L699
    set_window_title_with_build_class(cx);

    // @pob-skip Modules/Build.lua:701-723 reason="commented-out test and profiling code"
    // @pob Modules/Build.lua:724-727 h=1fab6893
    // L725
    cx.build_mut().abort_save = Some(false);
    // L726
    let f = h.sync_loadouts.clone();
    f(cx);
}

/// CCR-0098 §6 workaround (delete once calc:BM-A's `TreeTab:SetActiveSpec` calls a UI hook for
/// TreeTab.lua:556-557): replays those two lines for the constructor's `SetActiveSpec(1)`
/// (TreeTab.lua:37, at L613), whose spec is a fresh `PassiveSpec` of class 0 with no
/// ascendancy (PassiveSpec.lua:2391: the class name). Called before the TreeTab UI ctor hook:
/// in PoB it runs inside the ctor, after the viewer and before `SetCompareSpec` (:35-38).
fn tree_tab_ctor_set_active_spec_ui_tail(
    cx: &mut Cx,
    h: &BuildHooks,
    model_ref: &model::BuildRef,
) {
    let class0 = model_ref.latestTree.get().and_then(|tree| {
        tree.classes
            .get(0.0)
            .table()
            .and_then(|c| c.str(n!("name")))
            .map(|s| s.to_string())
    });
    TITLES.with(|t| {
        t.borrow_mut()
            .push(class0.unwrap_or_else(|| "nil".to_owned()));
    });
    flush_window_titles(cx);
    (h.update_class_dropdowns)(cx, Some(model_ref.data.gameVersions.latestTreeVersion));
}

/// CCR-0098 §6 workaround (delete with the hook): replays TreeTab.lua:556-557 for the
/// `SetActiveSpec` at the end of each deferred `Tree` section's `TreeTab:Load` (L671-676 ->
/// TreeTab.lua:525; a legacy `Spec` section sets the spec without it, :502-508). Approximation
/// (CCR-0098 §6): every replay uses the build's final spec, where PoB uses that load's own
/// active spec; the two differ only for a build file with more than one `Tree` section.
/// PostLoad and UpdateSocketGroups (L678-693) do not change the final spec's class.
fn tree_tab_load_set_active_spec_ui_tails(
    cx: &mut Cx,
    h: &BuildHooks,
    model_ref: &model::BuildRef,
) {
    let tree_loads = build_xml::xmlSectionList(model_ref)
        .iter()
        .filter(|n| &*n.elem == "Tree")
        .count();
    for _ in 0..tree_loads {
        set_window_title_with_build_class(cx);
        let tv = model_ref.spec.get().treeVersion.get();
        (h.update_class_dropdowns)(cx, Some(tv));
    }
}

/// lua: Build.lua:111-602 (Init's controls: top bar, side bar, skill selectors, stat box,
/// warnings) and the `~~temp~~` rename (L420-432)
/// @pob Modules/Build.lua:111-602 h=ab379c60
fn init_controls(
    cx: &mut Cx,
    me: CtrlId,
    h: &BuildHooks,
    build_name_arg: Option<&str>,
    prev_secondary: Option<f64>,
) {
    // L112
    cx.build_mut().abort_save = Some(true);

    // L114
    cx.clear_children(me);
    // L115-117 self.secondaryAscendDrop* = nil (U20's fields; fresh with the node)

    // L119 local miscTooltip = new("Tooltip"):Tooltip()
    cx.w_mut::<BuildMode>(me).init.misc_tooltip = Tooltip::default();

    // Controls: top bar, left side
    // L122
    let anchor_top_bar_left = cx
        .add_free(Some(me), PlainControl::new(Anchor::NONE, rect(4, 4, 0, 20)))
        .id();
    cx.w_mut::<BuildMode>(me).init.anchor_top_bar_left = Some(anchor_top_bar_left);
    // L123-129
    let hk = h.clone();
    let back = cx.add(
        me,
        "back",
        ButtonControl::new(
            Anchor::new(Left, anchor_top_bar_left, Right),
            rect(0, 0, 60, 20),
            "<< Back",
            move |cx| {
                // L124
                if cx.build().unsaved {
                    // L125
                    (hk.open_save_popup)(cx, "LIST");
                } else {
                    // L127
                    (hk.close_build)(cx);
                }
            },
        ),
    );
    // L130-132
    let hk = h.clone();
    let save = cx.add(
        me,
        "save",
        ButtonControl::new(
            Anchor::new(Left, back, Right),
            rect(8, 0, 50, 20),
            "Save",
            move |cx| {
                // L131
                (hk.save_db_file)(cx);
            },
        ),
    );
    // L133-135
    cx.base_mut(save).enabled = Prop::f(|cx, _| {
        // L134
        let b = cx.build();
        b.db_file_name.is_none() || b.unsaved
    });
    // L136-138
    let hk = h.clone();
    let save_as = cx.add(
        me,
        "saveAs",
        ButtonControl::new(
            Anchor::new(Left, save, Right),
            rect(8, 0, 70, 20),
            "Save As",
            move |cx| {
                // L137
                (hk.open_save_as_popup)(cx);
            },
        ),
    );
    // L139-141
    cx.base_mut(save_as).enabled = Prop::f(|cx, _| {
        // L140
        cx.build().db_file_name.is_some()
    });

    // conditional for smaller screens to move "Current build" to the side bar
    // L144-146 buildNameConditional: see build_name_conditional
    // L147
    let build_name = cx
        .add(
            me,
            "buildName",
            PlainControl::new(Anchor::new(Left, save_as, Right), rect(4, 36, 0, 20)),
        )
        .id();
    // L148-157
    cx.base_mut(build_name).width = Prop::f(move |cx, _control| {
        // L149-152
        let limit = if build_name_conditional(cx, me) {
            203.0
        } else {
            cx.pos(anchor_top_bar_right(cx, me)).0
                - 98.0
                - 62.0
                - cx.size(ctl(cx, me, "pointDisplay")).0
                - cx.size(ctl(cx, me, "levelScalingButton")).0
                - cx.size(ctl(cx, me, "characterLevel")).0
                - cx.size(ctl(cx, me, "back")).0
                - cx.size(ctl(cx, me, "save")).0
                - cx.size(ctl(cx, me, "saveAs")).0
        };
        // L153
        let bnw = cx
            .sg
            .draw_string_width(
                16.0,
                Font::Var,
                cx.build().build_name.as_deref().unwrap_or_else(|| {
                    lua_error!("DrawStringWidth() argument 3: expected string, got nil")
                }),
            );
        let s = st(cx, me);
        // L154
        s.str_width.set(m_min(bnw, limit));
        // L155
        s.str_limited.set(bnw > limit);
        // L156
        s.str_width.get() + 98.0
    });
    // L158-180
    cx.set_ov(build_name, |ov| {
        ov.draw = Some(Rc::new(move |cx, control, _vp, _no_tooltip| {
            // L159
            let (x, y) = cx.pos(control);
            // L160
            let (width, height) = cx.size(control);
            let str_width = st(cx, me).str_width.get();
            // L161
            cx.sg.set_draw_color(0.5, 0.5, 0.5);
            // L162
            cx.sg.draw_image(None, x + 91.0, y, str_width + 6.0, 20.0);
            // L163
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
            // L164
            cx.sg.draw_image(None, x + 92.0, y + 1.0, str_width + 4.0, 18.0);
            // L165
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L166
            cx.sg.set_viewport(x, y + 2.0, str_width + 94.0, 16.0);
            // L167
            let text = format!("Current build:  {}", build_name_concat(cx));
            cx.sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, &text);
            // L168
            cx.sg.reset_viewport();
            // L169
            if cx.is_mouse_in_bounds(control) {
                // L170
                cx.sg.set_draw_layer(None, Some(10));
                let mut tt = std::mem::take(&mut cx.w_mut::<BuildMode>(me).init.misc_tooltip);
                // L171
                tt.clear(false);
                let b = cx.build();
                let sub = b.db_file_sub_path.clone();
                let name = b.build_name.clone();
                // L172
                if let Some(sub) = sub.filter(|s| !s.is_empty()) {
                    // L173
                    let name = build_name_concat(cx);
                    tt.add_line(cx, 16.0, &format!("{sub}{name}"));
                } else if st(cx, me).str_limited.get() {
                    // L174-175 (unreachable with a nil name: L153 raised first)
                    tt.add_line(cx, 16.0, name.as_deref().unwrap_or_default());
                }
                // L177
                let vp = cx.main.view_port;
                tt.draw(cx, x, y, Some(width), Some(height), &vp);
                cx.w_mut::<BuildMode>(me).init.misc_tooltip = tt;
                // L178
                cx.sg.set_draw_layer(None, Some(0));
            }
        }));
    });
    // L181-183
    cx.base_mut(build_name).x = Prop::f(move |cx, _| {
        // L182
        if build_name_conditional(cx, me) {
            -196.0
        } else {
            8.0
        }
    });
    // L184-186
    cx.base_mut(build_name).y = Prop::f(move |cx, _| {
        // L185
        if build_name_conditional(cx, me) {
            32.0
        } else {
            0.0
        }
    });

    // Controls: top bar, right side
    // L189
    let anchor_top_bar_right = cx
        .add_free(
            Some(me),
            PlainControl::new(
                Anchor::NONE,
                rect(Prop::f(|cx, _| cx.main.screen_w / 2.0 + 6.0), 4, 0, 20),
            ),
        )
        .id();
    cx.w_mut::<BuildMode>(me).init.anchor_top_bar_right = Some(anchor_top_bar_right);

    // L191-193 getPointDisplayX: see get_point_display_x
    // L194
    let point_display = cx
        .add(
            me,
            "pointDisplay",
            New::new(
                Base::new(
                    Anchor::new(Left, anchor_top_bar_right, Right),
                    rect(Prop::f(move |cx, _| get_point_display_x(cx, me)), 0, 0, 20),
                ),
                InfoControl {
                    str_: None,
                    req: None,
                },
            ),
        )
        .id();
    // L195-197
    cx.base_mut(point_display).width = Prop::f(|cx, control| {
        // L196
        let s = cx.w::<InfoControl>(control).str_.clone().unwrap_or_else(|| {
            lua_error!("bad argument #3 to 'DrawStringWidth' (string expected, got nil)")
        });
        cx.sg.draw_string_width(16.0, Font::Fixed, &s) + 8.0
    });
    // L198-214
    cx.set_ov(point_display, |ov| {
        ov.draw = Some(Rc::new(move |cx, control, _vp, _no_tooltip| {
            // L199
            let (x, y) = cx.pos(control);
            // L200
            let (width, height) = cx.size(control);
            // L201
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L202
            cx.sg.draw_image(None, x, y, width + 2.0, height);
            // L203
            cx.sg.set_draw_color(0.0, 0.0, 0.0);
            // L204
            cx.sg.draw_image(None, x + 1.0, y + 1.0, width, height - 2.0);
            // L205
            cx.sg.set_draw_color(1.0, 1.0, 1.0);
            // L206
            let s = cx.w::<InfoControl>(control).str_.clone().unwrap_or_default();
            cx.sg
                .draw_string(x + 4.0, y + 2.0, Align::Left, 16.0, Font::Fixed, &s);
            // L207
            if cx.is_mouse_in_bounds(control) {
                // L208
                cx.sg.set_draw_layer(None, Some(10));
                let mut tt = std::mem::take(&mut cx.w_mut::<BuildMode>(me).init.misc_tooltip);
                // L209
                tt.clear(false);
                // L210 (AddLine of a nil text adds nothing, Tooltip.lua:92)
                if let Some(req) = cx.w::<InfoControl>(control).req.clone() {
                    tt.add_line(cx, 16.0, &req);
                }
                // L211
                let vp = cx.main.view_port;
                tt.draw(cx, x, y, Some(width), Some(height), &vp);
                cx.w_mut::<BuildMode>(me).init.misc_tooltip = tt;
                // L212
                cx.sg.set_draw_layer(None, Some(0));
            }
        }));
    });
    // L215-221
    let auto_mode = build_xml::characterLevelAutoMode(&cx.build().model);
    let level_scaling_button = cx
        .add(
            me,
            "levelScalingButton",
            ButtonControl::new(
                Anchor::new(Left, point_display, Right),
                rect(7, 0, 50, 20),
                if auto_mode { "Auto" } else { "Manual" },
                move |cx| {
                    let model = cx.build().model.clone();
                    // L216
                    let auto = !build_xml::characterLevelAutoMode(&model);
                    build_xml::build_extra(&model).set(n!("characterLevelAutoMode"), auto);
                    // L217
                    let b = ctl(cx, me, "levelScalingButton");
                    cx.w_mut::<ButtonControl>(b).label =
                        Prop::Val(if auto { "Auto" } else { "Manual" }.to_owned());
                    // L218
                    model.configTab.BuildModList(&model);
                    // L219-220
                    set_mod_and_build_flag(cx);
                },
            ),
        )
        .id();
    // L222-229
    let character_level = cx
        .add(
            me,
            "characterLevel",
            EditControl::new(
                Anchor::new(Left, level_scaling_button, Right),
                rect(5, 0, 106, 20),
                EditArgs {
                    init: Some(String::new()),
                    prompt: Some("Level".to_owned()),
                    filter: Some("%D".to_owned()),
                    limit: Some(3),
                    change_func: Some(Rc::new(move |cx, buf| {
                        let model = cx.build().model.clone();
                        // L223
                        model
                            .characterLevel
                            .set(m_min(m_max(tonumber(buf).unwrap_or(1.0), 1.0), 100.0));
                        // L224
                        model.configTab.BuildModList(&model);
                        // L225-226
                        set_mod_and_build_flag(cx);
                        // L227
                        build_xml::build_extra(&model).set(n!("characterLevelAutoMode"), false);
                        // L228
                        let b = ctl(cx, me, "levelScalingButton");
                        cx.w_mut::<ButtonControl>(b).label = Prop::Val("Manual".to_owned());
                    })),
                    ..EditArgs::default()
                },
            ),
        )
        .id();
    // L230
    let level = cx.build().model.characterLevel.get();
    EditControl::set_text(cx, character_level, &tostring_num(level), false);
    // L231-258
    cx.w_mut::<EditControl>(character_level).tt.tooltip_func =
        Some(Rc::new(move |cx, tooltip, ()| {
            let model = cx.build().model.clone();
            // L232
            if tooltip.check_for_update(cx, &[TtKey::Num(model.characterLevel.get())]) {
                // L233
                tooltip.add_line(cx, 16.0, "Experience multiplier:");
                // L234
                let player_level = model.characterLevel.get();
                // L235
                let safe_zone = 3.0 + m_floor(player_level / 16.0);
                // L236
                for (i, exp_level) in model.data.monsterExperienceLevelMap.iter().enumerate() {
                    let level = (i + 1) as f64;
                    // L237
                    let diff = m_abs(player_level - exp_level) - safe_zone;
                    // L238
                    let mut mult;
                    // L239
                    if diff <= 0.0 {
                        // L240
                        mult = 1.0;
                    } else {
                        // L242
                        mult = lpow(
                            (player_level + 5.0) / (player_level + 5.0 + lpow(diff, 2.5)),
                            1.5,
                        );
                    }
                    // L244
                    if player_level >= 95.0 {
                        // L245
                        let pen = [0.935, 0.885, 0.813, 0.7175, 0.6];
                        let k = player_level - 94.0;
                        let xp_penalty = if k.fract() == 0.0 && (1.0..=5.0).contains(&k) {
                            pen[k as usize - 1]
                        } else {
                            0.0
                        };
                        // L246
                        mult = mult * (1.0 / (1.0 + 0.1 * (player_level - 94.0))) * xp_penalty;
                    }
                    // L248
                    if mult > 0.01 {
                        // L249
                        let mut line = tostring_num(level);
                        // L250
                        if level >= 68.0 {
                            // L251
                            line.push_str(&pob_lua::lfmt!(" (Tier %d)", level - 67.0));
                        }
                        // L253
                        line.push_str(&pob_lua::lfmt!(": %.1f%%", mult * 100.0));
                        // L254
                        tooltip.add_line(cx, 14.0, &line);
                    }
                }
            }
        }));
    // L259-282
    let class_sel: SelFunc = Rc::new(move |cx, _index, value| {
        let value = value
            .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
        let class_id = value
            .get::<ClassListValue>()
            .map_or(f64::NAN, |v| v.class_id);
        let label = value.label.clone();
        let spec = cx.build().model.spec.get();
        // L260 (a nil classId is ~= every class id)
        if class_id.is_nan() || class_id != spec.curClassId.get() {
            // L261
            if spec.CountAllocNodes().0 == 0.0 || spec.IsClassConnected(class_id) {
                // L262-265
                select_class(cx, class_id);
            } else {
                // L267-279
                popups::open_confirm_popup(
                    cx,
                    "Class Change",
                    &format!("Changing class to {label} will reset your passive tree.\nThis can be avoided by connecting one of the {label} starting nodes to your tree."),
                    "Continue",
                    move |cx| {
                        // L268-271
                        select_class(cx, class_id);
                    },
                    Some((
                        "Connect Path",
                        Rc::new(move |cx: &mut Cx| {
                            // L273
                            let spec = cx.build().model.spec.get();
                            if spec.ConnectToClass_ret(class_id) {
                                // L274-277
                                select_class(cx, class_id);
                            }
                        }),
                    )),
                );
            }
        }
    });
    let class_drop = cx
        .add(
            me,
            "classDrop",
            DropDownControl::new(
                Anchor::new(Left, character_level, Right),
                rect(10, 0, 85, 20),
                Vec::new(),
                Some(class_sel),
                None,
                false,
            ),
        )
        .id();
    // L283-288
    let ascend_sel: SelFunc = Rc::new(move |cx, _index, value| {
        let value = value
            .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
        let id = value
            .get::<AscendListValue>()
            .map_or(f64::NAN, |v| v.ascend_class_id);
        let spec = cx.build().model.spec.get();
        // L284
        spec.SelectAscendClass(id);
        // L285
        let f = hooks(cx).spec_add_undo_state;
        f(cx);
        // L286
        set_window_title_with_build_class(cx);
        // L287
        cx.build().model.buildFlag.set(true);
    });
    let ascend_drop = cx
        .add(
            me,
            "ascendDrop",
            DropDownControl::new(
                Anchor::new(Left, class_drop, Right),
                rect(4, 0, 120, 20),
                Vec::new(),
                Some(ascend_sel),
                None,
                false,
            ),
        )
        .id();
    // L289-303
    let secondary_sel: SelFunc = Rc::new(move |cx, _index, value| {
        // L292 (the build always has a spec here)
        let Some(value) = value else { return };
        let id = value
            .get::<AscendListValue>()
            .map_or(f64::NAN, |v| v.ascend_class_id);
        let spec = cx.build().model.spec.get();
        // L295
        spec.SelectSecondaryAscendClass(id);
        // L296
        let f = hooks(cx).spec_add_undo_state;
        f(cx);
        // L297
        set_window_title_with_build_class(cx);
        // L298
        cx.build().model.buildFlag.set(true);
    });
    let secondary_ascend_drop = cx
        .add(
            me,
            "secondaryAscendDrop",
            DropDownControl::new(
                Anchor::new(Left, ascend_drop, Right),
                rect(4, 0, 160, 20),
                vec![DropItem::new(
                    "None",
                    Rc::new(AscendListValue {
                        ascend_class_id: 0.0,
                        extra: None,
                    }),
                )],
                Some(secondary_sel),
                None,
                false,
            ),
        )
        .id();
    // L300
    cx.w_mut::<DropDownControl>(secondary_ascend_drop)
        .enable_dropped_width = true;
    // L301
    cx.w_mut::<DropDownControl>(secondary_ascend_drop)
        .max_dropped_width = 360.0;
    // L302
    let initial_secondary_selection = prev_secondary.unwrap_or(0.0);
    // L303
    DropDownControl::sel_by_value(cx, secondary_ascend_drop, &|it: &DropItem| {
        it.get::<AscendListValue>()
            .is_some_and(|v| v.ascend_class_id == initial_secondary_selection)
    });

    // L305-419
    let hk = h.clone();
    let loadouts_sel: SelFunc = Rc::new(move |cx, _index, value| {
        let value_s = drop_str(value);
        let is = |s: &str| value_s.as_deref() == Some(s);
        let loadouts = ctl(cx, me, "buildLoadouts");
        // L306
        if is("^7^7Loadouts:") || is("^7^7-----") {
            // L307
            DropDownControl::set_sel(cx, loadouts, 1, false);
            // L308
            return;
        }
        // L310
        if is("^7^7Sync") {
            // L311
            (hk.sync_loadouts)(cx);
            // L312
            DropDownControl::set_sel(cx, loadouts, 1, false);
            // L313
            return;
        }
        // L315
        if is("^7^7Help >>") {
            // L316
            let f = cx.app().main.hooks.open_about_popup.clone();
            f(cx, ModeArg::Num(7.0));
            // L317
            DropDownControl::set_sel(cx, loadouts, 1, false);
            // L318
            return;
        }
        // L320
        if is("^7^7New Loadout") {
            open_new_loadout_popup(cx, &hk);
            // L355
            DropDownControl::set_sel(cx, loadouts, 1, false);
            // L356
            return;
        }
        // a nil `value` raises only inside the find loops (L363/L384), not here
        let value = value_s;

        // L361-373 findSetId, L377-389 findNamedSetId: see find_set_id / find_named_set_id
        let model = cx.build().model.clone();
        // L391
        let one_skill = model.skillsTab.skillSetOrderList.len() == 1;
        // L392
        let one_item = model.itemsTab.itemSetOrderList.len() == 1;
        // L393
        let one_config = model.configTab.configSetOrderList.len() == 1;

        // L395
        let spec_list = (hk.tree_get_spec_list)(cx);
        let s = st(cx, me);
        let new_spec_id =
            find_named_set_id(&spec_list, value.as_deref(), &s.tree_list_special_links);
        // L396
        let new_item_id = if one_item {
            Some(1.0)
        } else {
            let order = model.itemsTab.itemSetOrderList.with(Clone::clone);
            model.itemsTab.itemSets.with(|sets| {
                find_set_id(&order, value.as_deref(), sets, &s.item_list_special_links)
            })
        };
        // L397
        let new_skill_id = if one_skill {
            Some(1.0)
        } else {
            let order = model.skillsTab.skillSetOrderList.with(Clone::clone);
            model.skillsTab.skillSets.with(|sets| {
                find_set_id(&order, value.as_deref(), sets, &s.skill_list_special_links)
            })
        };
        // L398
        let new_config_id = if one_config {
            Some(1.0)
        } else {
            let order = model.configTab.configSetOrderList.with(Clone::clone);
            model.configTab.configSets.with(|sets| {
                find_set_id(&order, value.as_deref(), sets, &s.config_list_special_links)
            })
        };

        // if exact match nor special grouping cannot find setIds, bail
        // L401
        let (Some(new_spec_id), Some(new_item_id), Some(new_skill_id), Some(new_config_id)) =
            (new_spec_id, new_item_id, new_skill_id, new_config_id)
        else {
            // L402
            return;
        };

        // L405
        if new_spec_id != model.treeTab.activeSpec.get() {
            // L406
            (hk.tree_set_active_spec)(cx, new_spec_id);
            flush_window_titles(cx);
        }
        // L408
        if new_item_id != model.itemsTab.activeItemSetId.get() {
            // L409
            (hk.items_set_active_item_set)(cx, new_item_id);
        }
        // L411
        if new_skill_id != model.skillsTab.activeSkillSetId.get() {
            // L412
            (hk.skills_set_active_skill_set)(cx, new_skill_id);
        }
        // L414
        if new_config_id != model.configTab.activeConfigSetId.get() {
            // L415
            (hk.config_set_active_config_set)(cx, new_config_id);
        }

        // L418
        let loadouts = ctl(cx, me, "buildLoadouts");
        DropDownControl::sel_by_value(cx, loadouts, &|it: &DropItem| {
            it.as_str() == value.as_deref()
        });
    });
    cx.add(
        me,
        "buildLoadouts",
        DropDownControl::new(
            Anchor::new(Left, secondary_ascend_drop, Right),
            rect(4, 0, 190, 20),
            Vec::new(),
            Some(loadouts_sel),
            None,
            false,
        ),
    );

    // L421
    if build_name_arg == Some("~~temp~~") {
        // Remove temporary build file
        // L423 os.remove(self.dbFileName) (a failure is ignored, as os.remove returns it)
        if let Some(db) = cx.build().db_file_name.clone() {
            let _ = cx.sys.fs.remove(&db);
        }
        // L424
        cx.build_mut().build_name = Some("Unnamed build".to_owned());
        // L425
        cx.build_mut().db_file_name = None;
        cx.build_mut().db_file_name_is_false = true;
        let model = cx.build().model.clone();
        build_xml::set_dbFileName(&model, None);
        build_xml::build_extra(&model).set(n!("buildName"), "Unnamed build");
        // L426
        cx.build_mut().db_file_sub_path = None;
        // L427
        cx.build_mut().mod_flag = true;
    }

    // List of display stats
    // L431-434 self.displayStats / minionDisplayStats / extraSaveStats: U21's
    // Modules/BuildDisplayStats.lua (display_stats.rs) is read directly by its users

    // Controls: Side bar
    // L437
    let anchor_side_bar = cx
        .add_free(Some(me), PlainControl::new(Anchor::NONE, rect(4, 60, 0, 0)))
        .id();
    cx.w_mut::<BuildMode>(me).init.anchor_side_bar = Some(anchor_side_bar);
    // L438-440
    cx.base_mut(anchor_side_bar).y = Prop::f(move |cx, _| {
        // L439
        if build_name_conditional(cx, me) {
            60.0
        } else {
            36.0
        }
    });

    /// `self.viewMode == mode` (the `locked` functions)
    fn view_mode_is(cx: &Cx, mode: &str) -> bool {
        &*cx.build().model.viewMode.get() == mode
    }
    fn set_view_mode(cx: &mut Cx, mode: &str) {
        cx.build().model.viewMode.set(model::Name::from(mode));
    }
    // L442-445
    let hk = h.clone();
    let mode_import = cx.add(
        me,
        "modeImport",
        ButtonControl::new(
            Anchor::new(TopLeft, anchor_side_bar, TopLeft),
            rect(0, 0, 134, 20),
            "Import/Export Build",
            move |cx| {
                // L443
                set_view_mode(cx, "IMPORT");
                // L444
                (hk.import_try_fetch_character_list)(cx);
            },
        ),
    );
    // L446
    cx.get_mut(mode_import).locked = Prop::f(|cx, _| view_mode_is(cx, "IMPORT"));
    // L447-449
    let mode_notes = cx.add(
        me,
        "modeNotes",
        ButtonControl::new(
            Anchor::new(Left, mode_import, Right),
            rect(4, 0, 58, 20),
            "Notes",
            |cx| {
                // L448
                set_view_mode(cx, "NOTES");
            },
        ),
    );
    // L450
    cx.get_mut(mode_notes).locked = Prop::f(|cx, _| view_mode_is(cx, "NOTES"));
    // L451-453
    let mode_config = cx.add(
        me,
        "modeConfig",
        ButtonControl::new(
            Anchor::new(TopRight, anchor_side_bar, TopLeft),
            rect(300, 0, 100, 20),
            "Configuration",
            |cx| {
                // L452
                set_view_mode(cx, "CONFIG");
            },
        ),
    );
    // L454
    cx.get_mut(mode_config).locked = Prop::f(|cx, _| view_mode_is(cx, "CONFIG"));
    // L455-457
    let mode_tree = cx.add(
        me,
        "modeTree",
        ButtonControl::new(
            Anchor::new(TopLeft, anchor_side_bar, TopLeft),
            rect(0, 26, 72, 20),
            "Tree",
            |cx| {
                // L456
                set_view_mode(cx, "TREE");
            },
        ),
    );
    // L458
    cx.get_mut(mode_tree).locked = Prop::f(|cx, _| view_mode_is(cx, "TREE"));
    // L459-461
    let mode_skills = cx.add(
        me,
        "modeSkills",
        ButtonControl::new(
            Anchor::new(Left, mode_tree, Right),
            rect(4, 0, 72, 20),
            "Skills",
            |cx| {
                // L460
                set_view_mode(cx, "SKILLS");
            },
        ),
    );
    // L462
    cx.get_mut(mode_skills).locked = Prop::f(|cx, _| view_mode_is(cx, "SKILLS"));
    // L463-465
    let mode_items = cx.add(
        me,
        "modeItems",
        ButtonControl::new(
            Anchor::new(Left, mode_skills, Right),
            rect(4, 0, 72, 20),
            "Items",
            |cx| {
                // L464
                set_view_mode(cx, "ITEMS");
            },
        ),
    );
    // L466
    cx.get_mut(mode_items).locked = Prop::f(|cx, _| view_mode_is(cx, "ITEMS"));
    // L467-469
    let mode_calcs = cx.add(
        me,
        "modeCalcs",
        ButtonControl::new(
            Anchor::new(Left, mode_items, Right),
            rect(4, 0, 72, 20),
            "Calcs",
            |cx| {
                // L468
                set_view_mode(cx, "CALCS");
            },
        ),
    );
    // L470
    cx.get_mut(mode_calcs).locked = Prop::f(|cx, _| view_mode_is(cx, "CALCS"));
    // L471-473
    let mode_party = cx.add(
        me,
        "modeParty",
        ButtonControl::new(
            Anchor::new(TopLeft, anchor_side_bar, TopLeft),
            rect(0, 52, 72, 20),
            "Party",
            |cx| {
                // L472
                set_view_mode(cx, "PARTY");
            },
        ),
    );
    // L474
    cx.get_mut(mode_party).locked = Prop::f(|cx, _| view_mode_is(cx, "PARTY"));
    // L475-477
    let mode_compare = cx.add(
        me,
        "modeCompare",
        ButtonControl::new(
            Anchor::new(Left, mode_party, Right),
            rect(4, 0, 72, 20),
            "Compare",
            |cx| {
                // L476
                set_view_mode(cx, "COMPARE");
            },
        ),
    );
    // L478
    cx.get_mut(mode_compare).locked = Prop::f(|cx, _| view_mode_is(cx, "COMPARE"));

    // Skills
    // L480
    let main_skill_label = cx.add(
        me,
        "mainSkillLabel",
        LabelControl::new(
            Anchor::new(TopLeft, anchor_side_bar, TopLeft),
            rect(0, 80, 300, 16),
            "^7Main Skill:",
        ),
    );
    // L481-485
    let main_socket_group = cx
        .add(
            me,
            "mainSocketGroup",
            DropDownControl::new(
                Anchor::new(TopLeft, main_skill_label, BottomLeft),
                rect(0, 2, 300, 18),
                Vec::new(),
                Some(Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L482
                    cx.build().model.mainSocketGroup.set(index as f64);
                    // L483-484
                    set_mod_and_build_flag(cx);
                })),
                None,
                false,
            ),
        )
        .id();
    // L486
    cx.w_mut::<DropDownControl>(main_socket_group).max_dropped_width = 500.0;
    // L487-492
    let hk = h.clone();
    cx.w_mut::<DropDownControl>(main_socket_group).tt.tooltip_func = Some(Rc::new(
        move |cx: &mut Cx, tooltip: &mut Tooltip, (_mode, index, _value): DropTipArgs| {
            // L488
            let socket_group = index.and_then(|i| socket_group(cx, i as f64));
            // L489
            if let Some(socket_group) = socket_group {
                let rev = cx.build().model.outputRevision.get();
                let obj: Rc<dyn std::any::Any> = socket_group.clone();
                if tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(obj)), TtKey::Num(rev)]) {
                    // L490
                    (hk.skills_add_socket_group_tooltip)(cx, tooltip, &socket_group);
                }
            }
        },
    ));
    // L493-498
    let main_skill = cx
        .add(
            me,
            "mainSkill",
            DropDownControl::new(
                Anchor::new(TopLeft, main_socket_group, BottomLeft),
                rect(0, 2, 300, 18),
                Vec::new(),
                Some(Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L494
                    let msg = cx.build().model.mainSocketGroup.get();
                    let group = socket_group(cx, msg).unwrap_or_else(|| {
                        lua_error!("attempt to index local 'mainSocketGroup' (a nil value)")
                    });
                    // L495
                    group.mainActiveSkill.set(index as f64);
                    // L496-497
                    set_mod_and_build_flag(cx);
                })),
                None,
                false,
            ),
        )
        .id();
    // L499-505
    let main_skill_part = cx
        .add(
            me,
            "mainSkillPart",
            DropDownControl::new(
                Anchor::new(TopLeft, main_skill, BottomLeft).collapse(),
                rect(0, 2, 300, 18),
                Vec::new(),
                Some(Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L500-501
                    let src_instance = main_src_instance(cx);
                    // L502
                    src_instance.skillPart.set(Some(index as f64));
                    // L503-504
                    set_mod_and_build_flag(cx);
                })),
                None,
                false,
            ),
        )
        .id();
    // L506-510
    let main_skill_stage_count_label = cx
        .add(
            me,
            "mainSkillStageCountLabel",
            LabelControl::new(
                Anchor::new(TopLeft, main_skill_part, BottomLeft).collapse(),
                rect(0, 3, 0, 16),
                "^7Stages:",
            )
            .shown(Prop::f(move |cx, _| {
                // L508
                cx.is_shown(ctl(cx, me, "mainSkillStageCount"))
            })),
        )
        .id();
    // L511-517
    cx.add(
        me,
        "mainSkillStageCount",
        EditControl::new(
            Anchor::new(Left, main_skill_stage_count_label, Right).collapse(),
            rect(2, 0, 60, 18),
            EditArgs {
                filter: Some("%D".to_owned()),
                change_func: Some(Rc::new(|cx: &mut Cx, buf: &str| {
                    // L512-513
                    let src_instance = main_src_instance(cx);
                    // L514
                    src_instance.skillStageCount.set(tonumber(buf));
                    // L515-516
                    set_mod_and_build_flag(cx);
                })),
                ..EditArgs::default()
            },
        ),
    );
    // L518-522
    let main_skill_mine_count_label = cx
        .add(
            me,
            "mainSkillMineCountLabel",
            LabelControl::new(
                Anchor::new(TopLeft, main_skill_stage_count_label, BottomLeft).collapse(),
                rect(0, 3, 0, 16),
                "^7Active Mines:",
            )
            .shown(Prop::f(move |cx, _| {
                // L520
                cx.is_shown(ctl(cx, me, "mainSkillMineCount"))
            })),
        )
        .id();
    // L523-529
    cx.add(
        me,
        "mainSkillMineCount",
        EditControl::new(
            Anchor::new(Left, main_skill_mine_count_label, Right).collapse(),
            rect(2, 0, 60, 18),
            EditArgs {
                filter: Some("%D".to_owned()),
                change_func: Some(Rc::new(|cx: &mut Cx, buf: &str| {
                    // L524-525
                    let src_instance = main_src_instance(cx);
                    // L526
                    src_instance.skillMineCount.set(tonumber(buf));
                    // L527-528
                    set_mod_and_build_flag(cx);
                })),
                ..EditArgs::default()
            },
        ),
    );
    // L530-540
    let main_skill_minion = cx
        .add(
            me,
            "mainSkillMinion",
            DropDownControl::new(
                Anchor::new(TopLeft, main_skill_mine_count_label, BottomLeft).collapse(),
                rect(0, 3, 178, 18),
                Vec::new(),
                Some(Rc::new(|cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                    // L531-532
                    let src_instance = main_src_instance(cx);
                    let value = value.unwrap_or_else(|| {
                        lua_error!("attempt to index local 'value' (a nil value)")
                    });
                    let v = value.get::<MinionListValue>();
                    // L533
                    if let Some(item_set_id) = v.and_then(|v| v.item_set_id) {
                        // L534
                        src_instance.skillMinionItemSet.set(Some(item_set_id));
                    } else {
                        // L536
                        src_instance
                            .skillMinion
                            .set(v.and_then(|v| v.minion_id));
                    }
                    // L538-539
                    set_mod_and_build_flag(cx);
                })),
                None,
                false,
            ),
        )
        .id();
    // L541-547
    cx.set_ov(main_skill_minion, |ov| {
        ov.can_receive_drag = Some(Rc::new(|cx, control, ty, value| {
            // L542
            let d = cx.w::<DropDownControl>(control);
            let sel = d
                .list
                .get(d.sel_index.wrapping_sub(1))
                .and_then(|it| it.get::<MinionListValue>())
                .and_then(|v| v.item_set_id);
            if ty == "Item" && sel.is_some() {
                // L543-544
                let effect = main_active_effect(cx);
                let minion_uses = effect.grantedEffect.get().minionUses.clone();
                // L545 return minionUses and minionUses[value:GetPrimarySlot()] -- O_O
                let Some(item) = value.0.downcast_ref::<model::ItemRef>() else {
                    lua_error!("attempt to call method 'GetPrimarySlot' (a nil value)")
                };
                return minion_uses.is_some_and(|u| u.contains(&item.GetPrimarySlot()));
            }
            false
        }));
        // L548-550
        ov.receive_drag = Some(Rc::new(|cx, control, _ty, value, _source| {
            // L549
            let d = cx.w::<DropDownControl>(control);
            let item_set_id = d
                .list
                .get(d.sel_index.wrapping_sub(1))
                .and_then(|it| it.get::<MinionListValue>())
                .and_then(|v| v.item_set_id)
                .unwrap_or(f64::NAN);
            let Some(item) = value.0.downcast_ref::<model::ItemRef>().cloned() else {
                lua_error!("ReceiveDrag: the dragged value is not an item")
            };
            let f = hooks(cx).items_equip_item_in_set;
            f(cx, &item, item_set_id);
        }));
    });
    // L551-558
    let hk = h.clone();
    cx.w_mut::<DropDownControl>(main_skill_minion).tt.tooltip_func = Some(Rc::new(
        move |cx: &mut Cx, tooltip: &mut Tooltip, (_mode, _index, value): DropTipArgs| {
            // L552
            tooltip.clear(false);
            let value = value
                .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
            // L553
            if let Some(item_set_id) = value
                .get::<MinionListValue>()
                .and_then(|v| v.item_set_id)
            {
                // L554
                let model = cx.build().model.clone();
                let set = model
                    .itemsTab
                    .itemSets
                    .with(|s| s.get(&(item_set_id as u32)).cloned())
                    .unwrap_or_default();
                (hk.items_add_item_set_tooltip)(cx, tooltip, &set);
                // L555
                tooltip.add_separator(cx, Some(14.0));
                // L556
                let tip = format!(
                    "{}Tip: You can drag items from the Items tab onto this dropdown to equip them onto the minion.",
                    cx.main.cc.TIP
                );
                tooltip.add_line(cx, 14.0, &tip);
            }
        },
    ));
    // L559-561
    let hk = h.clone();
    cx.add(
        me,
        "mainSkillMinionLibrary",
        ButtonControl::new(
            Anchor::new(Left, main_skill_minion, Right),
            rect(2, 0, 120, 18),
            "Manage Spectres...",
            move |cx| {
                // L560
                (hk.open_spectre_library)(cx);
            },
        ),
    );
    // L562-568
    let main_skill_minion_skill = cx
        .add(
            me,
            "mainSkillMinionSkill",
            DropDownControl::new(
                Anchor::new(TopLeft, main_skill_minion, BottomLeft).collapse(),
                rect(0, 2, 200, 16),
                Vec::new(),
                Some(Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                    // L563-564
                    let src_instance = main_src_instance(cx);
                    // L565
                    src_instance.skillMinionSkill.set(Some(index as f64));
                    // L566-567
                    set_mod_and_build_flag(cx);
                })),
                None,
                false,
            ),
        )
        .id();
    // L569
    let stat_box_anchor = cx
        .add(
            me,
            "statBoxAnchor",
            PlainControl::new(
                Anchor::new(TopLeft, main_skill_minion_skill, BottomLeft).collapse(),
                rect(0, 2, 0, 0),
            ),
        )
        .id();
    // L570
    let stat_box = cx
        .add(
            me,
            "statBox",
            TextListControl::new(
                Anchor::new(TopLeft, stat_box_anchor, BottomLeft),
                rect(0, 2, 300, 0),
                Some(vec![
                    TextListColumn {
                        x: 170.0,
                        align: Align::RightX,
                    },
                    TextListColumn {
                        x: 174.0,
                        align: Align::Left,
                    },
                ]),
                Vec::new(),
                None,
            ),
        )
        .id();
    // L571-575
    cx.base_mut(stat_box).height = Prop::f(move |cx, control| {
        // L572
        let (_x, y) = cx.pos(control);
        // L573
        let warn_height = if cx.app().main.show_warnings
            && !cx.w::<WarningsControl>(ctl(cx, me, "warnings")).lines.is_empty()
        {
            18.0
        } else {
            0.0
        };
        // L574
        cx.main.screen_h - cx.app().main.main_bar_height - 4.0 - y - warn_height
    });
    // L576-578
    let hk = h.clone();
    cx.w_mut::<TextListControl>(stat_box).on_click =
        Some(Rc::new(move |cx: &mut Cx, hovered_line: Option<HoveredLine>| {
            // L577
            (hk.set_display_stat)(cx, hovered_line, true);
        }));
    // L579
    let warnings = cx
        .add(
            me,
            "warnings",
            New::new(
                Base::new(
                    Anchor::new(TopLeft, stat_box, BottomLeft).collapse(),
                    rect(0, 0, 0, 18),
                ),
                WarningsControl {
                    // L580
                    lines: Vec::new(),
                    str_: WarnStr::Nil,
                },
            ),
        )
        .id();
    // L581-583
    cx.base_mut(warnings).width = Prop::f(|cx, control| {
        // L582
        match &cx.w::<WarningsControl>(control).str_ {
            WarnStr::Nil => 0.0,
            WarnStr::Str(s) => cx.sg.draw_string_width(16.0, Font::Fixed, s) + 8.0,
            WarnStr::Table => {
                lua_error!("bad argument #3 to 'DrawStringWidth' (string expected, got table)")
            }
        }
    });
    // L584-602
    cx.set_ov(warnings, |ov| {
        ov.draw = Some(Rc::new(move |cx, control, _vp, _no_tooltip| {
            // L585
            if !cx.w::<WarningsControl>(control).lines.is_empty() {
                // L586-587 (pairs over a sequence: ascending)
                let count = cx.w::<WarningsControl>(control).lines.len();
                // L588
                let s = pob_lua::lfmt!(
                    format!("{}%d Warnings", cx.main.cc.NEGATIVE).as_str(),
                    count as f64
                );
                cx.w_mut::<WarningsControl>(control).str_ = WarnStr::Str(s.clone());
                // L589
                let (x, y) = cx.pos(control);
                // L590
                let (width, height) = cx.size(control);
                // L591
                cx.sg
                    .draw_string(x, y + 2.0, Align::Left, 16.0, Font::Fixed, &s);
                // L592
                if cx.is_mouse_in_bounds(control) {
                    // L593
                    cx.sg.set_draw_layer(None, Some(10));
                    let mut tt =
                        std::mem::take(&mut cx.w_mut::<BuildMode>(me).init.misc_tooltip);
                    // L594
                    tt.clear(false);
                    // L595 ORDER: a sequence, ascending
                    let lines = cx.w::<WarningsControl>(control).lines.clone();
                    for v in &lines {
                        tt.add_line(cx, 16.0, v);
                    }
                    // L596
                    let vp = cx.main.view_port;
                    tt.draw(cx, x, y, Some(width), Some(height), &vp);
                    cx.w_mut::<BuildMode>(me).init.misc_tooltip = tt;
                    // L597
                    cx.sg.set_draw_layer(None, Some(0));
                }
            } else {
                // L600
                cx.w_mut::<WarningsControl>(control).str_ = WarnStr::Table;
            }
        }));
    });
}

/// L320-357: the "New Loadout" popup of the buildLoadouts selFunc.
/// (part of the `@pob Modules/Build.lua:111-602` segment of `init_controls`)
fn open_new_loadout_popup(cx: &mut Cx, h: &BuildHooks) {
    let hk = h.clone();
    // L321 local controls = { } (the popup's host)
    // L353
    popups::open_popup(
        cx,
        370.0,
        100.0,
        "Set Name",
        move |cx, p| {
            let p = p.id();
            // L322
            cx.add(
                p,
                "label",
                LabelControl::new(Anchor::NONE, rect(0, 20, 0, 16), "^7Enter name for this loadout:"),
            );
            // L323-325
            cx.add(
                p,
                "edit",
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 40, 350, 20),
                    EditArgs {
                        init: Some("New Loadout".to_owned()),
                        limit: Some(100),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L324 controls.save.enabled = buf:match("%S")
                            let any = buf
                                .bytes()
                                .any(|c| !matches!(c, b' ' | b'\t' | b'\n' | 0x0b | 0x0c | b'\r'));
                            let save = ctl(cx, p, "save");
                            cx.base_mut(save).enabled = Prop::Val(any);
                        })),
                        ..EditArgs::default()
                    },
                ),
            );
            // L326-348
            let hk = hk.clone();
            let save = cx.add(
                p,
                "save",
                ButtonControl::new(Anchor::NONE, rect(-45, 70, 80, 20), "Save", move |cx| {
                    // L327
                    let loadout = cx.w::<EditControl>(ctl(cx, p, "edit")).buf.clone();
                    let model = cx.build().model.clone();
                    let data = model::data();

                    // L329
                    // CCR-0098 §4: model.rs (U15) does not re-export the constructor
                    let new_spec = pob_calc::classes::passive_spec::new_PassiveSpec(
                        &model,
                        data.gameVersions.latestTreeVersion,
                        false,
                    );
                    // L330
                    new_spec.title.set(Some(model::LStr::from(loadout.as_str())));
                    // L331
                    model.treeTab.specList.push(new_spec);

                    // L333
                    let n_items = model.itemsTab.itemSets.with(|s| id_table_len(s.keys().copied()));
                    let item_set = model.itemsTab.NewItemSet(Some(n_items + 1.0));
                    // L334
                    model.itemsTab.itemSetOrderList.push(item_set.num(n!("id")) as u32);
                    // L335
                    item_set.set(n!("title"), loadout.as_str());

                    // L337
                    let n_skills = model.skillsTab.skillSets.with(|s| id_table_len(s.keys().copied()));
                    let skill_set = model.skillsTab.NewSkillSet(Some(n_skills + 1.0));
                    // L338
                    model.skillsTab.skillSetOrderList.push(skill_set.num(n!("id")) as u32);
                    // L339
                    skill_set.set(n!("title"), loadout.as_str());

                    // L341
                    let n_configs = model.configTab.configSets.with(|s| id_table_len(s.keys().copied()));
                    let config_set = model.configTab.NewConfigSet(Some(n_configs + 1.0), None);
                    // L342
                    model.configTab.configSetOrderList.push(config_set.num(n!("id")) as u32);
                    // L343
                    config_set.set(n!("title"), loadout.as_str());

                    // L345
                    (hk.sync_loadouts)(cx);
                    // L346
                    cx.build_mut().mod_flag = true;
                    // L347
                    popups::close_popup(cx);
                }),
            );
            // L349
            cx.base_mut(save).enabled = Prop::Val(false);
            // L350-352
            cx.add(
                p,
                "cancel",
                ButtonControl::new(Anchor::NONE, rect(45, 70, 80, 20), "Cancel", |cx| {
                    // L351
                    popups::close_popup(cx);
                }),
            );
        },
        PopupOpts {
            enter: Some("save".into()),
            default: Some("edit".into()),
            escape: Some("cancel".into()),
            ..PopupOpts::default()
        },
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn insert_if_new_appends_once() {
        // Build.lua:24-30
        let mut v = vec!["a".to_owned()];
        insert_if_new(Some(&mut v), "b".to_owned());
        insert_if_new(Some(&mut v), "a".to_owned());
        insert_if_new(Some(&mut v), "b".to_owned());
        assert_eq!(v, ["a", "b"]);
        insert_if_new::<String>(None, "c".to_owned());
    }

    #[test]
    fn match_flags_rules() {
        // Build.lua:32-62: required flags first, then disabling flags; nil lists pass
        let flags = |f: &str| matches!(f, "attack" | "melee");
        assert!(match_flags(None, None, flags));
        assert!(match_flags(Some(&["attack"]), None, flags));
        assert!(!match_flags(Some(&["attack", "spell"]), None, flags));
        assert!(!match_flags(None, Some(&["melee"]), flags));
        assert!(match_flags(Some(&["melee"]), Some(&["spell"]), flags));
        assert!(!match_flags(Some(&["spell"]), Some(&["melee"]), flags));
    }

    #[test]
    fn acts_and_act_extra() {
        // Build.lua:729-769
        assert_eq!(ACTS.len(), 11);
        assert_eq!(ACTS[0], Act { level: 1.0, questPoints: 0.0 });
        assert_eq!(ACTS[10], Act { level: 67.0, questPoints: 23.0 });
        assert_eq!(act_extra(2.0, 1.0), 0.0);
        assert_eq!(act_extra(3.0, 1.0), 1.0);
    }

    #[test]
    fn set_lookup_helpers() {
        let links = SpecialLinks {
            links: [("abc".to_owned(), 4.0)].into_iter().collect(),
        };
        assert_eq!(link_match("x {abc} y").as_deref(), Some("abc"));
        assert_eq!(link_match("plain"), None);
        let list = vec!["Default".to_owned(), "Second".to_owned()];
        assert_eq!(find_named_set_id(&list, Some("Second"), &links), Some(2.0));
        assert_eq!(find_named_set_id(&list, Some("Nope"), &links), None);
        assert_eq!(find_named_set_id(&list, Some("{abc}"), &links), Some(4.0));
        // a nil value only raises inside the loop (Build.lua:384)
        assert_eq!(find_named_set_id(&[], None, &links), None);
        assert_eq!(id_table_len([1u32, 2, 3].into_iter()), 3.0);
        assert_eq!(id_table_len([1u32, 3].into_iter()), 1.0);
        assert_eq!(id_table_len(std::iter::empty()), 0.0);
    }
}
