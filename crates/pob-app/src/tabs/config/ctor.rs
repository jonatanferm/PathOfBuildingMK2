//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ConfigTab.lua:1-834 (module locals,
//! the ConfigTab constructor building every option control), :835-837 (`IsSectionCollapsed`),
//! :1302-1334 (the UI half of `UpdateCustomModsControls`, CCR-0110 §2), the tooltip helpers of
//! src/Modules/ConfigOptions.lua:11-115 and `string:matchOrPattern` (src/Modules/Common.lua:1038-1079).
//! Owner: U41.
//!
//! @pob-file Classes/ConfigTab.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//!
//! # Model and UI halves
//!
//! The model half of the constructor is calc:BM-B's and has already run when this constructor
//! runs (`build_xml::Init_model`, DIVERGENCE(init-order) of `build/init.rs`): `self.input`,
//! `placeholder`, `defaultState`, the config sets (`ctor_model`, L140-148), `enemyLevel` (L150,
//! which `UpdateLevel` has set since) and the `varData.var` block L672-692 with its control-state
//! tables (`ConfigTab::ctor_controls_model`, CCR-0037/CCR-0080 §1). This file builds the controls
//! (the UI statements of L133-834) and does not write the model's input/placeholder/defaults: the
//! build's sections are already loaded into them.
//!
//! # The var-controls bridge (CCR-0110 §3)
//!
//! Lua has one object per option control; the port has two: the real control (a node here) and
//! the model's control-state table `configTab.varControls[var]` (the fields the ConfigOptions
//! applies read and write: `selIndex`, `list`, `enabled`, `placeholder`, plus `buf`/`state` for
//! the model's `UpdateControls`). The model tables are canonical for that state:
//! * UI -> model: [`ConfigTab::push_var_control`] copies `state`/`buf`/`selIndex` of the control
//!   `self.varControls[var]` into its table at the start of the control's own callback (the
//!   applies run by the callback's BuildModList read them);
//! * model -> UI: [`ConfigTab::pull_var_controls`] copies every table's `buf` (through
//!   `SetText`, only when it differs), `placeholder`, `state`, `selIndex` and `enabled` into the
//!   controls. It runs after every BuildModList this file runs, at the end of the constructor
//!   (the state the model's Load/UpdateControls/BuildModList left, DIVERGENCE(init-order)), and in
//!   the default `UpdateControls` hook. U42's Draw should run it first (a BuildModList run
//!   elsewhere, e.g. SyncLoadouts or Init, changes the tables without the controls).
//!
//! # Seams to U42 (CCR-0110 §1)
//!
//! `self:UpdateControls()`, `self:AddUndoState()`, `self:OpenConfigSetManagePopup()`,
//! `configModBrowser.OpenAddModPopup` and `configVisibility.isShowAllExcluded` are U42's
//! (ConfigTab.lua:835-1362, ConfigModBrowser.lua, ConfigVisibility.lua): [`ConfigHooks`], one
//! entry each (U42 replaces its entries with [`set_config_hooks`]). `SetActiveConfigSet` is
//! `BuildHooks::config_set_active_config_set` (U19's seam, model half plus U42's UI tail).
//! `UpdateCustomModsControls` (1302-1334) and `IsSectionCollapsed` (835-837) are ported here
//! because the constructor and the custom mod blocks call them (CCR-0110 §2).
//!
//! # Lua semantics
//!
//! * Option keys absent from the data at 16de4b82d (the dump schema `ConfigOptionDef` is
//!   `deny_unknown_fields` and has no field for them): `ifNode` (L382-395), `ifEnemyStat`
//!   (L550-570), `implyMinionCond`/`implyEnemyCond` (L222-223), `inactiveText` (L348-350),
//!   `doNotHighlight` (L696) and `resizable` (L333/L322). Their branches are ported against the
//!   value nil (the constants in [`VarExtras`]); `ifNode` takes node ids.
//! * `type(innerShown) == "boolean" and innerShown or innerShown()` (L698, L716, L723, L746,
//!   L807): `innerShown`/`eligibleShown` are always functions here, so it is the call.
//! * `listOrSingleIfOption` calls `ifFunc(ifOption)` with the table itself after the loop
//!   (L240): every ifFunc then indexes a table key (nil) or tests `type(..) ~= "string"`, i.e.
//!   gives its `implyCond` part or false ([`IfArg::Table`]).
//! * The search filter: `buf:lower():gsub(..)` keeps the first result; `PCall(string.matchOrPattern,
//!   ..)` is a `catch_unwind` around the pure port of Common.lua's function (a Lua pattern error
//!   in the typed text is caught, as PCall catches it).
//! * `self.calcFunc`/`self.calcBase` are `BuildState::calc_func`/`calc_base` (ui doc §10.1).
//! * `configSets[active].input[varData.var] = state` with a nil `var` is the Lua "table index is
//!   nil" error (no option of the data has a control type without a `var`).
//! * The option tooltip's `tooltipFunc(tooltip, mode, index, value)` receives what the control's
//!   TooltipHost passes: nothing (EditControl), the state (CheckBoxControl, as `mode`) or
//!   `(mode, index, value)` (DropDownControl): [`TipArgs`].
//!
//! DIVERGENCE(init-order): see the bridge. PoB's constructor runs `UpdateCustomModsControls`
//! over NewConfigSet's default list, and a loaded `Config` section's Load
//! (ConfigTab.lua:937 SetActiveConfigSet -> 1352 UpdateControls) hides those blocks
//! (`shown = false`) and builds the loaded ones. The model is already loaded here, so for a
//! build with a `Config` section the constructor builds the first (hidden) batch from a default
//! list of its own and then the loaded ones: the same controls in the same `self.controls`
//! slots. The model's Load-time `BuildModList` effects on the control tables arrive through the
//! bridge pull at the end of the constructor.
//! DIVERGENCE(list-falsy-val): `type(value) == "table" and value.val or value` (L764) gives the
//! list entry itself for a falsy `val`; no list of the data has one (`val = false`/`nil`), so the
//! port uses `val`.
//!
//! ORDER: `pairs(section.varControlList)` (L272): a sum of heights, order-independent (c).
//! `pairs(mainEnv.skillsUsed)` (L630): an any-match, order-independent (c). `pairs(subGroup.patterns)`
//! and `pairs(orPatterns)` (matchOrPattern): set iteration; the result is an any-match, so the
//! order matters only when one alternative raises a pattern error and another matches, where
//! PoB's own result is per-process random (c; insertion order here). The bridge iterates
//! `varControls` in option order (each control independent, (c)).
//!
//! Props with side effects (ui doc §7.3): none (the props read; the section height closure only
//! calls `GetSize`, whose block override writes a `Cell`, custom_mod_block.rs).

#![allow(non_snake_case)]

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;

use indexmap::{IndexMap, IndexSet};
use pob_lua::math::m_max;
use pob_lua::string::StripEscapes;
use pob_lua::{n, tostring_num};
use pob_sg::Font;
use pob_ui_core::controls::drop_down::{DropTipArgs, DropTipMode};
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl, LabelControl,
    ResizableEditControl, ScrollBarControl, SectionControl,
};
use pob_ui_core::luastr::gsub;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::Tooltip;

use super::custom_mod_block::CustomModBlockControl;
use super::ConfigTab;
use crate::app::CxApp;
use crate::model::gamedata::schema::config_defs::{
    ConfigOptionDef, StrOrList, Tooltip as OptTooltip,
};
use crate::model::gamedata::LuaFnRef;
use crate::model::{self, EnvRef, Name, Output, Table, Val};

// @pob Classes/ConfigTab.lua:1-14 h=da0287eb
// (the math/table locals are Rust functions; `varList` is `data.configOptions`, `configVisibility`
// and `configModBrowser` are U42's modules, reached through [`ConfigHooks`])

// @pob Classes/ConfigTab.lua:129-130 h=337d725f
// (`newClass("ConfigTab", "UndoHandler", "ControlHost", "Control")`: the struct in `mod.rs`,
// `.host()` in the constructor; the UndoHandler state is U42's `core::State`)

/// This unit's part of the split `ConfigTab` (ui doc §12.1): the fields the constructor creates.
#[derive(Default)]
pub struct State {
    /// `self.build` (the BuildMode node)
    pub build: CtrlId,
    /// `self.toggleConfigs` (L155)
    pub toggle_configs: bool,
    /// `self.sectionList` (L152)
    pub section_list: Vec<CtrlId>,
    /// The fields Lua stores on each SectionControl (`varControlList`, `col`, `collapsed`).
    pub sections: HashMap<CtrlId, SectionData>,
    /// `self.varControls` (L153): var -> control (a later option with the same var replaces the
    /// earlier one, L675), in option order.
    pub var_controls: IndexMap<Name, VarControl>,
    /// `self.customSection` (L293)
    pub custom_section: Option<CtrlId>,
    /// `self.customModsBlockControls` (L827, UpdateCustomModsControls)
    pub custom_mods_block_controls: Option<Vec<CtrlId>>,
}

/// The fields of a section control (L262-265).
pub struct SectionData {
    /// `section.varControlList`
    pub var_control_list: Vec<CtrlId>,
    /// `section.col`
    pub col: Option<f64>,
    /// `section.collapsed`
    pub collapsed: bool,
}

/// One entry of `self.varControls`.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct VarControl {
    pub id: CtrlId,
    pub class: VarClass,
}

/// The control class an option's `type` selects (L280-347; the model's `_className`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum VarClass {
    CheckBox,
    Edit,
    ResizableEdit,
    DropDown,
    Control,
}

/// A list option's `value` (`{ val = …, label = … }`): the `DropItem` value.
pub type ListVal = Val;

/// lua: ConfigTab.lua:133 ConfigTab widget (drawn by BuildMode through ConfigTab:Draw, U42).
/// CCR-0110 §4: the impl lives here because `New<ConfigTab>` needs it; `Draw` is an inherent
/// function (U42), not a Widget method, as for every tab.
impl Widget for ConfigTab {
    const NAME: &'static str = "ConfigTab";
    const CAPS: Caps = Caps::empty();
}

// ---- seams (CCR-0110 §1) ----------------------------------------------------------------------

/// The ConfigTab methods this unit calls that U42 ports. Each default names its owner.
#[derive(Clone)]
pub struct ConfigHooks {
    /// `self:UpdateControls()` (ConfigTab.lua:1008-1022, U42). Default: the model half
    /// (calc:BM-B `ConfigTab::UpdateControls`), the bridge pull and the UI half of
    /// UpdateCustomModsControls.
    pub update_controls: Rc<dyn Fn(&mut Cx, CtrlId)>,
    /// `self:AddUndoState()` (UndoHandler.lua with ConfigTab:CreateUndoState, U42). Default:
    /// nothing.
    pub add_undo_state: Rc<dyn Fn(&mut Cx, CtrlId)>,
    /// `self:OpenConfigSetManagePopup()` (ConfigTab.lua:1270-1278, U42)
    pub open_config_set_manage_popup: Rc<dyn Fn(&mut Cx, CtrlId)>,
    /// `configModBrowser.OpenAddModPopup(configTab, blockData)` (ConfigModBrowser.lua, U42)
    pub open_add_mod_popup: Rc<dyn Fn(&mut Cx, CtrlId, &Table)>,
    /// `configVisibility.isShowAllExcluded(varData)` (ConfigVisibility.lua:144-155, U42)
    pub is_show_all_excluded: Rc<dyn Fn(&ConfigOptionDef) -> bool>,
}

/// The installed defaults: U42's ports (core.rs, mod_browser.rs, visibility.rs).
pub fn default_config_hooks() -> ConfigHooks {
    ConfigHooks {
        // U42's ports (CCR-0110 §1, CCR-0121 §3)
        update_controls: Rc::new(ConfigTab::update_controls),
        add_undo_state: Rc::new(ConfigTab::add_undo_state),
        open_config_set_manage_popup: Rc::new(ConfigTab::open_config_set_manage_popup),
        open_add_mod_popup: Rc::new(super::mod_browser::open_add_mod_popup_hook),
        is_show_all_excluded: Rc::new(super::visibility::isShowAllExcluded),
    }
}

thread_local! {
    static CONFIG_HOOKS: RefCell<Option<ConfigHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn config_hooks() -> ConfigHooks {
    CONFIG_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_config_hooks)
            .clone()
    })
}

/// Replace hook entries (U42; tests).
pub fn set_config_hooks(f: impl FnOnce(&mut ConfigHooks)) {
    CONFIG_HOOKS.with(|h| {
        let mut b = h.borrow_mut();
        f(b.get_or_insert_with(default_config_hooks));
    });
}

// ---- small helpers -----------------------------------------------------------------------------

/// Lua `tostring(v)` for the values config inputs hold.
pub fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(x) => tostring_num(*x),
        Val::Str(s) => s.to_string(),
        other => format!("{other:?}"),
    }
}

fn model_ref(cx: &Cx) -> model::BuildRef {
    cx.build().model.clone()
}

/// `self:BuildModList()` (calc:CO-C).
fn build_mod_list(cx: &Cx) {
    let m = model_ref(cx);
    m.configTab.BuildModList(&m);
}

/// `self.configSets[self.activeConfigSetId]`
fn active_set(cx: &Cx) -> Table {
    cx.build().model.configTab.activeConfigSet()
}

/// `self.configSets[self.activeConfigSetId].input`
fn active_input(cx: &Cx) -> Table {
    active_set(cx)
        .table(n!("input"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'input' (a nil value)"))
}

/// `self.configSets[self.activeConfigSetId].placeholder`
fn active_placeholder(cx: &Cx) -> Table {
    active_set(cx)
        .table(n!("placeholder"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'placeholder' (a nil value)"))
}

/// `input[varData.var]` (a nil key reads nil).
fn input_of(cx: &Cx, var: Option<Name>) -> Val {
    var.map_or(Val::Nil, |v| active_input(cx).get(v))
}

/// `self:GetDefaultState(varData.var, type(cur))` (calc:BM-B). A nil `var` reads nil from
/// `placeholder` and `defaultState`, and `type(cur)` is then "nil" (cur = input[nil]): nil.
fn default_state(cx: &Cx, var: Option<Name>, cur: &Val) -> Val {
    match var {
        Some(v) => cx
            .build()
            .model
            .configTab
            .GetDefaultState(v, Name::from(cur.lua_type())),
        None => Val::Nil,
    }
}

/// `cur ~= nil and cur ~= def` (the "changed from default" test of L699-701, L717-719, L724-726, L748-750).
fn is_changed(cx: &Cx, var: Option<Name>) -> bool {
    let cur = input_of(cx, var);
    let def = default_state(cx, var, &cur);
    !cur.is_nil() && !cur.raw_eq(&def)
}

/// `self.build.calcsTab.mainEnv`
fn main_env(cx: &Cx) -> EnvRef {
    cx.build()
        .model
        .calcsTab
        .mainEnv
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"))
}

/// A `mainEnv.<x>Used` table (nil: the Lua index error).
#[track_caller]
fn used(t: Option<Table>, name: &str) -> Table {
    t.unwrap_or_else(|| lua_error!("attempt to index field '{name}' (a nil value)"))
}

/// The `ifFunc` argument: one option name, or the option table itself (L237).
#[derive(Clone, Copy, Debug)]
enum IfArg {
    One(Name),
    Table,
}

impl IfArg {
    /// `t[ifOption]` truthiness (a table key reads nil).
    fn index(self, t: &Table) -> bool {
        match self {
            IfArg::One(n) => t.truthy(n),
            IfArg::Table => false,
        }
    }
}

type ShownFn = Rc<dyn Fn(&Cx) -> bool>;
type IfFunc = Rc<dyn Fn(&Cx, IfArg) -> bool>;
type TipFn = Rc<dyn Fn(&Cx) -> Option<String>>;
type IfTipFn = Rc<dyn Fn(&Cx, Name) -> Option<String>>;

/// One entry of `tooltipFuncs` (L366): a string or a function.
#[derive(Clone)]
enum TipPart {
    Str(String),
    Fn(TipFn),
}

/// lua: ConfigTab.lua:231-242 listOrSingleIfOption
fn list_or_single_if_option(if_option: StrOrList, if_func: IfFunc) -> ShownFn {
    // L232
    Rc::new(move |cx| {
        // L233
        if let StrOrList::List(list) = &if_option {
            // L234
            for if_opt in list {
                // L235
                if if_func(cx, IfArg::One(*if_opt)) {
                    // L236
                    return true;
                }
            }
        }
        // L240
        match &if_option {
            StrOrList::Str(s) => if_func(cx, IfArg::One(*s)),
            StrOrList::List(_) => if_func(cx, IfArg::Table),
        }
    })
}

/// lua: ConfigTab.lua:244-258 listOrSingleIfTooltip
fn list_or_single_if_tooltip(if_option: StrOrList, if_func: IfTipFn) -> TipFn {
    // L246
    Rc::new(move |cx| match &if_option {
        // L247
        StrOrList::List(list) => {
            // L248
            let mut out: Option<String> = None;
            // L249
            for if_opt in list {
                // L250
                if let Some(cur) = if_func(cx, *if_opt) {
                    // L251
                    out = Some(match out {
                        Some(o) => o + "\n" + &cur,
                        None => cur,
                    });
                }
            }
            // L254
            out
        }
        // L256
        StrOrList::Str(s) => if_func(cx, *s),
    })
}

/// The dev-mode tooltip of the `*Used` options (L408-421 and its seven copies): every mod of
/// `t[ifOption]` as `modLib.formatMod(mod) .. "|" .. mod.source`, when `launch.devModeAlt`.
fn used_mods_tooltip(cx: &Cx, t: Option<&Table>, if_option: Name) -> Option<String> {
    // L409
    if !cx.app().launch.dev_mode_alt {
        // L410
        return None;
    }
    // L412
    let mut out: Option<String> = None;
    // L413
    let mods = t?.get(if_option);
    // L414
    if !mods.truthy() {
        // L415
        return out;
    }
    // L417-419
    append_mods(&mut out, &mods);
    // L420
    out
}

/// `for _, mod in ipairs(mods) do out = (out and out.."\n" or "") .. modLib.formatMod(mod) .. "|" .. mod.source end`
fn append_mods(out: &mut Option<String>, mods: &Val) {
    let Some(list) = mods.table() else {
        lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            mods.lua_type()
        )
    };
    for (_, m) in list.ipairs() {
        let Val::Mod(m) = m else {
            lua_error!("attempt to index local 'mod' (a {} value)", m.lua_type())
        };
        let source = m
            .source
            .as_deref()
            .unwrap_or_else(|| lua_error!("attempt to concatenate field 'source' (a nil value)"));
        let line = format!("{}|{}", pob_gamedata::mod_tools::formatMod(&m), source);
        *out = Some(match out.take() {
            Some(o) => o + "\n" + &line,
            None => line,
        });
    }
}

/// The option keys the data never sets (see the module notes); nil for every option.
struct VarExtras {
    /// `varData.ifNode` (node ids)
    if_node: Option<NodeIfOpt>,
    /// `varData.ifEnemyStat`
    if_enemy_stat: Option<StrOrList>,
    /// `varData.implyMinionCond`
    imply_minion_cond: Option<Name>,
    /// `varData.implyEnemyCond`
    imply_enemy_cond: Option<Name>,
    /// `varData.doNotHighlight`
    do_not_highlight: bool,
    /// `varData.resizable`
    resizable: bool,
    /// `varData.inactiveText`
    inactive_text: Option<String>,
}

/// `varData.ifNode`: a node id or a list of them (never set by the data).
#[derive(Clone)]
#[allow(dead_code)]
enum NodeIfOpt {
    One(f64),
    List(Vec<f64>),
}

const VAR_EXTRAS: VarExtras = VarExtras {
    if_node: None,
    if_enemy_stat: None,
    imply_minion_cond: None,
    imply_enemy_cond: None,
    do_not_highlight: false,
    resizable: false,
    inactive_text: None,
};

/// A single-name option key as `listOrSingle*`'s argument.
fn single(n: Option<Name>) -> Option<StrOrList> {
    n.map(StrOrList::Str)
}

// ---- string:matchOrPattern (Common.lua:1038-1079) ----------------------------------------------

/// lua: Common.lua:1038-1079 `string:matchOrPattern(pattern)`; `Err` when a generated pattern
/// raises (what `PCall` reports as `err`).
/// @pob Modules/Common.lua:1038-1079 h=f7b8bae7
pub fn match_or_pattern(s: &str, pattern: &str) -> Result<bool, String> {
    std::panic::catch_unwind(|| match_or_pattern_raw(s, pattern))
        .map_err(|p| pob_host::panic_message(&*p))
}

struct SubGroup {
    open: usize,
    close: usize,
    patterns: IndexSet<String>,
}

/// L1039-1070 generateOrPatterns (the `orPatterns` set; `pairs` order is insertion order here,
/// see ORDER in the module notes).
fn generate_or_patterns(pattern: &str) -> IndexSet<String> {
    // L1040
    let mut sub_groups: Vec<SubGroup> = Vec::new();
    // L1041
    let mut index = 1usize;
    // find and call generate patterns on all subGroups
    // L1043
    let groups: Vec<String> = pob_lua::pattern::gmatch(pattern, "%b()")
        .map(|c| c[0].to_str().into_owned())
        .collect();
    for sub_group in groups {
        // L1044 (`subGroups[index]` is always nil here: index == #subGroups + 1)
        let init = sub_groups
            .get(index - 1)
            .map_or(1, |g: &SubGroup| g.close as i64);
        let (open, close, _) = pob_lua::pattern::find(pattern, &sub_group, Some(init), true)
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
        // L1045
        let inner = pob_lua::string::sub(&sub_group, 2, Some(-2)).into_owned();
        sub_groups.push(SubGroup {
            open,
            close,
            patterns: generate_or_patterns(&inner),
        });
        // L1046
        index += 1;
    }

    // generate complete patterns from the subGroup patterns
    // L1050
    let first_open = sub_groups.first().map_or(0, |g| g.open as i64);
    let mut generated_patterns: Vec<String> =
        vec![pob_lua::string::sub(pattern, 1, Some(first_open - 1)).into_owned()];
    // L1051
    for (i, sub_group) in sub_groups.iter().enumerate() {
        // L1052
        let next_open = sub_groups.get(i + 1).map_or(0, |g| g.open as i64);
        let regular_next_string =
            pob_lua::string::sub(pattern, sub_group.close as i64 + 1, Some(next_open - 1))
                .into_owned();
        // L1053
        let mut temp_patterns: Vec<String> = Vec::new();
        // L1054
        for sub_pattern in &generated_patterns {
            // L1055
            for sub_group_pattern in &sub_group.patterns {
                // L1056
                temp_patterns.push(format!(
                    "{sub_pattern}{sub_group_pattern}{regular_next_string}"
                ));
            }
        }
        // L1059
        generated_patterns = temp_patterns;
    }

    // apply | operators
    // L1063
    let mut or_patterns: IndexSet<String> = IndexSet::new();
    // L1064
    for generated_pattern in &generated_patterns {
        // L1065
        for c in pob_lua::pattern::gmatch(generated_pattern, "[^|]+") {
            // L1066 store string as key to avoid duplicates.
            or_patterns.insert(c[0].to_str().into_owned());
        }
    }
    // L1069
    or_patterns
}

fn match_or_pattern_raw(s: &str, pattern: &str) -> bool {
    // L1072
    let or_patterns = generate_or_patterns(pattern);
    // L1073
    for or_pattern in &or_patterns {
        // L1074
        if pob_lua::pattern::r#match(s, or_pattern, None).is_some() {
            // L1075
            return true;
        }
    }
    // L1078
    false
}

// ---- ConfigOptions.lua tooltip helpers (11-115) ------------------------------------------------
//
// The calc's CO-A marks these lines `@pob-skip` ("UI tooltip helper"); their ports are here,
// without `@pob` markers (a second marker would cover the in-scope lines twice, CCR-0110 §5).

/// `tooltipFunc(tooltip, mode, index, value)`'s arguments as each control's TooltipHost passes
/// them (see the module notes).
#[derive(Clone)]
pub enum TipArgs {
    /// EditControl: `self:DrawTooltip(x, y, w, h, viewPort)`
    None,
    /// CheckBoxControl: `..., self.state` (arrives as `mode`)
    Check(bool),
    /// DropDownControl: `..., mode, index, value`
    Drop(DropTipArgs),
}

impl TipArgs {
    /// `mode` when it is one of DropDownControl's strings (a checkbox's boolean never equals one).
    fn mode(&self) -> Option<DropTipMode> {
        match self {
            TipArgs::Drop((m, _, _)) => Some(*m),
            _ => None,
        }
    }
    /// `value` (nil unless a dropdown passed one).
    fn value(&self) -> Option<&DropItem> {
        match self {
            TipArgs::Drop((_, _, v)) => v.as_ref(),
            _ => None,
        }
    }
}

/// `applyModes[mode]` with `applyModes = { BODY = true, HOVER = true }`.
fn apply_mode(args: &TipArgs) -> bool {
    matches!(args.mode(), Some(DropTipMode::Body | DropTipMode::Hover))
}

/// `value.val` (nil `value`: the Lua index error).
#[track_caller]
fn value_val(args: &TipArgs) -> Val {
    let v = args
        .value()
        .unwrap_or_else(|| lua_error!("attempt to index local 'value' (a nil value)"));
    v.get::<ListVal>().cloned().unwrap_or(Val::Nil)
}

/// `varData.tooltipFunc(tooltip, mode, index, value)`: the ConfigOptions.lua helper the dump
/// names by its source range.
fn call_option_tooltip_func(cx: &mut Cx, f: &LuaFnRef, tooltip: &mut Tooltip, args: &TipArgs) {
    match f.src_range() {
        Some((_, 11, 30)) => apply_pantheon_description(cx, tooltip, args),
        Some((_, 32, 44)) => bandit_tooltip(cx, tooltip, args),
        Some((_, 46, 59)) => boss_skills_tooltip(cx, tooltip, args),
        Some((_, 74, 115)) => map_affix_tooltip(cx, tooltip, args),
        _ => lua_error!(
            "ConfigOptions tooltipFunc {} ({}) is not ported",
            f.id,
            f.src
        ),
    }
}

/// `varData.tooltip(modList, build)` (the two tooltip functions).
fn call_option_tooltip(cx: &Cx, f: &LuaFnRef) -> Option<String> {
    match f.src_range() {
        Some((_, 61, 65)) => Some(low_life_tooltip(cx)),
        Some((_, 67, 72)) => Some(full_life_tooltip(cx)),
        _ => lua_error!("ConfigOptions tooltip {} ({}) is not ported", f.id, f.src),
    }
}

/// lua: Modules/ConfigOptions.lua:11-30 applyPantheonDescription(tooltip, mode, index, value)
fn apply_pantheon_description(cx: &mut Cx, tooltip: &mut Tooltip, args: &TipArgs) {
    // L12
    tooltip.clear(false);
    // L13
    let val = value_val(args);
    if val.str().as_deref() == Some("None") {
        // L14
        return;
    }
    // L16-17
    if apply_mode(args) {
        // L18
        let key = val
            .str()
            .unwrap_or_else(|| lua_error!("attempt to index local 'god' (a nil value)"));
        let god = model::data()
            .pantheons
            .get(&Name::from(&key))
            .unwrap_or_else(|| lua_error!("attempt to index local 'god' (a nil value)"));
        // L19
        for soul in &god.souls {
            // L20
            let name = &soul.name;
            // L21-24
            let lines: Vec<&str> = soul.mods.iter().map(|m| &*m.line).collect();
            // L25
            tooltip.add_line(cx, 20.0, &format!("^8{name}"));
            // L26
            tooltip.add_line(cx, 14.0, &format!("^6{}", lines.join("\n")));
            // L27
            tooltip.add_separator(cx, Some(10.0));
        }
    }
}

/// lua: Modules/ConfigOptions.lua:32-44 banditTooltip(tooltip, mode, index, value)
fn bandit_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, args: &TipArgs) {
    // L33-38
    let bandit_benefits = |v: &str| match v {
        "None" => Some("Grants 1 Passive Skill Point"),
        "Oak" => Some("+40 to Maximum ^xE05030Life"),
        "Kraityn" => Some("8% increased Movement Speed"),
        "Alira" => Some("+15% to all Elemental Resistances"),
        _ => None,
    };
    // L39
    let apply = apply_mode(args);
    // L40
    tooltip.clear(false);
    // L41
    if apply {
        // L42
        let val = value_val(args);
        let text = val
            .str()
            .and_then(|s| bandit_benefits(&s))
            .unwrap_or_else(|| lua_error!("attempt to concatenate field '?' (a nil value)"));
        tooltip.add_line(cx, 14.0, &format!("^8{text}"));
    }
}

/// lua: Modules/ConfigOptions.lua:46-59 bossSkillsTooltip(tooltip, mode, index, value)
fn boss_skills_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, args: &TipArgs) {
    // L47
    let apply = apply_mode(args);
    // L48
    tooltip.clear(false);
    // L49
    if apply {
        // L50-54
        tooltip.add_line(
            cx,
            14.0,
            "^7Used to fill in defaults for specific boss skills if the boss config is not set\n\
\n\
Bosses' damage is modified by roll range configuration, defaulted at a 70% roll, at the normal monster level for your character level (capped at 85)\n\
Fill in the exact damage numbers if more precision is needed",
        );
        // L55
        let val = value_val(args);
        if val.str().as_deref() != Some("None") {
            // L56
            let key = val
                .str()
                .unwrap_or_else(|| lua_error!("attempt to concatenate field 'val' (a nil value)"));
            let skill = model::data()
                .bossSkills
                .get(&Name::from(&key))
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            tooltip.add_line(cx, 14.0, &format!("\n^7{key}: {}", skill.tooltip));
        }
    }
}

/// `build.calcsTab.mainOutput.<stat>` in arithmetic (nil: the Lua arithmetic error).
fn main_output_num(cx: &Cx, stat: &str) -> f64 {
    let out = cx
        .build()
        .model
        .calcsTab
        .mainOutput
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainOutput' (a nil value)"));
    match out.get(Name::from(stat)) {
        Val::Num(x) => x,
        other => lua_error!(
            "attempt to perform arithmetic on field '{stat}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// lua: Modules/ConfigOptions.lua:61-65 LowLifeTooltip(modList, build)
fn low_life_tooltip(cx: &Cx) -> String {
    // L62
    let mut out = format!(
        "You will automatically be considered to be on Low ^xE05030Life ^7if you have at least {}% ^xE05030Life ^7reserved",
        tostring_num(100.0 - main_output_num(cx, "LowLifePercentage"))
    );
    // L63
    out += "\nbut you can use this option to force it if necessary.";
    // L64
    out
}

/// lua: Modules/ConfigOptions.lua:67-72 FullLifeTooltip(modList, build)
fn full_life_tooltip(cx: &Cx) -> String {
    // L68 (a concatenated number goes through tostring)
    let mut out = format!(
        "You can be considered to be on Full ^xE05030Life ^7if you have at least {}% ^xE05030Life ^7left.",
        tostring_num(main_output_num(cx, "FullLifePercentage"))
    );
    // L69
    out += "\nYou will automatically be considered to be on Full ^xE05030Life ^7if you have Chaos Inoculation,";
    // L70
    out += "\nbut you can use this option to force it if necessary.";
    // L71
    out
}

/// lua: Modules/ConfigOptions.lua:74-115 mapAffixTooltip(tooltip, mode, index, value)
fn map_affix_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, args: &TipArgs) {
    use pob_gamedata::Value;
    // L75
    tooltip.clear(false);
    // L76
    let val = value_val(args);
    if val.str().as_deref() == Some("NONE") {
        // L77
        return;
    }
    // L79-80
    if apply_mode(args) {
        let key = val
            .str()
            .unwrap_or_else(|| lua_error!("attempt to concatenate field 'val' (a nil value)"));
        // L81
        tooltip.add_line(cx, 14.0, &format!("^7{key}"));
        // L82 `data.mapMods.AffixData[value.val] or {}`
        let Some(affix_data) = model::data().mapMods.AffixData.get(&Name::from(&key)) else {
            // `#affixData.tooltipLines` of `{}`: the Lua length error
            lua_error!("attempt to get length of field 'tooltipLines' (a nil value)")
        };
        let tooltip_lines = &affix_data.tooltipLines;
        let n_lines = tooltip_lines.len();
        // `affixData.values[i]`, `values[i][j]` (Lua 1-based; nil past the end)
        let at = |v: &Value, i: usize| -> Value {
            match v {
                Value::Table(t) => t.array.get(i - 1).cloned().unwrap_or(Value::Nil),
                Value::Nil => lua_error!("attempt to index a nil value"),
                _ => lua_error!("attempt to index a {} value", "number"),
            }
        };
        let values = Value::Table(std::sync::Arc::new(pob_gamedata::ValueTable {
            array: affix_data.values.clone(),
            fields: Vec::new(),
        }));
        // L83
        if n_lines > 0 {
            let ty = affix_data.r#type.map(|t| t.as_str());
            // L84
            if ty == Some("check") {
                // L85
                for line in tooltip_lines {
                    // L86
                    tooltip.add_line(cx, 14.0, &format!("^7{line}"));
                }
            // L88
            } else if ty == Some("list") {
                // L89
                for (i, tier) in ["Low", "Med", "High"].iter().enumerate() {
                    let i = i + 1;
                    // L90
                    tooltip.add_line(cx, 16.0, &format!("^7{tier}: "));
                    // L91
                    for (j, line) in tooltip_lines.iter().enumerate() {
                        let j = j + 1;
                        // L92 `(n > 1) and values[i][j] or values[i]`: a nil/false
                        // `values[i][j]` falls through to the row `values[i]` itself
                        let vi = at(&values, i);
                        let mod_value = if n_lines > 1 {
                            lua_or(at(&vi, j), || vi.clone())
                        } else {
                            vi.clone()
                        };
                        // L93
                        if mod_value == Value::Nil {
                            // L94
                            tooltip.add_line(cx, 14.0, &format!("   ^7{line}"));
                        // L95
                        } else if mod_value != Value::Num(0.0) {
                            // L96
                            tooltip.add_line(
                                cx,
                                14.0,
                                &format!(
                                    "   ^7{}",
                                    pob_lua::lformat!(&line[..], value_fmt_arg(&mod_value))
                                ),
                            );
                        }
                    }
                }
            // L100
            } else if ty == Some("count") {
                // L101
                for (i, tier) in ["Low", "Med", "High"].iter().enumerate() {
                    let i = i + 1;
                    // L102
                    tooltip.add_line(cx, 16.0, &format!("^7{tier}: "));
                    // L103
                    for (j, line) in tooltip_lines.iter().enumerate() {
                        let j = j + 1;
                        // L104 `(n > 1) and (values[i][j] and values[i][j][k] or nil)
                        // or values[i][k]`: when the parenthesised part is nil/false the
                        // whole `A and B or C` falls through to `values[i][k]` (Profane's
                        // `{100}` second line: modValue = {100, {100}})
                        let vi = at(&values, i);
                        let pick = |k: usize| -> Value {
                            if n_lines > 1 {
                                let vij = at(&vi, j);
                                let inner = if lua_truthy(&vij) {
                                    at(&vij, k)
                                } else {
                                    Value::Nil
                                };
                                lua_or(inner, || at(&vi, k))
                            } else {
                                at(&vi, k)
                            }
                        };
                        let mod_value = [pick(1), pick(2)];
                        // L105
                        if mod_value[1] == Value::Nil {
                            // L106
                            tooltip.add_line(cx, 14.0, &format!("   ^7{line}"));
                        // L107
                        } else if mod_value[1] != Value::Num(0.0) {
                            // L108
                            let s = pob_lua::lformat!(
                                &line[..],
                                value_fmt_arg(&mod_value[0]),
                                value_fmt_arg(&mod_value[1])
                            );
                            tooltip.add_line(cx, 14.0, &format!("   ^7{s}"));
                        }
                    }
                }
            }
        }
    }
}

/// Lua truthiness of a data value (everything but nil and false).
fn lua_truthy(v: &pob_gamedata::Value) -> bool {
    !matches!(
        v,
        pob_gamedata::Value::Nil | pob_gamedata::Value::Bool(false)
    )
}

/// `a or b` on data values.
fn lua_or(a: pob_gamedata::Value, b: impl FnOnce() -> pob_gamedata::Value) -> pob_gamedata::Value {
    if lua_truthy(&a) {
        a
    } else {
        b()
    }
}

/// A `string.format` argument from a data value. Lua ignores surplus arguments, so a table
/// only matters when a `%d` consumes it; `FmtArg` has no table variant, so it goes in as nil
/// and a consuming spec raises the same "number expected" format error (Lua says "got table").
/// Shipped map-mod lines only ever leave a table as the unused trailing argument (Profane).
fn value_fmt_arg(v: &pob_gamedata::Value) -> pob_lua::format::FmtArg<'_> {
    use pob_gamedata::Value;
    use pob_lua::format::FmtArg;
    match v {
        Value::Num(x) => FmtArg::Num(*x),
        Value::Str(s) => FmtArg::Str(&s[..]),
        Value::Bool(b) => FmtArg::Bool(*b),
        _ => FmtArg::Nil,
    }
}

// ---- the constructor ---------------------------------------------------------------------------

/// `BuildHooks::config_tab`: `self.configTab = new("ConfigTab"):ConfigTab(self)` (Build.lua:611;
/// the model part is calc:BM-A/BM-B's, already run).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    cx.add_free(Some(build), ConfigTab::new(build)).id()
}

impl ConfigTab {
    /// lua: ConfigTab.lua:133-832 ConfigTab(build) (UI statements; see the module notes)
    pub fn new(build: CtrlId) -> New<ConfigTab> {
        // L134 self:UndoHandler() (U42's core::State), L135 self:ControlHost() (.host()),
        // L136 self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            ConfigTab {
                ctor: State {
                    // L138
                    build,
                    ..State::default()
                },
                core: Default::default(),
            },
        )
        .host()
        .init(|cx, me| ConfigTab::ctor_init(cx, me.id()))
    }

    /// The `ConfigTab` widget state of `me`.
    fn st(cx: &Cx, me: CtrlId) -> &State {
        &cx.w::<ConfigTab>(me).ctor
    }

    fn st_mut(cx: &mut Cx, me: CtrlId) -> &mut State {
        &mut cx.w_mut::<ConfigTab>(me).ctor
    }

    /// `#self.controls + 1` (the controls table's integer keys are appended without holes).
    fn next_index(cx: &Cx, me: CtrlId) -> i64 {
        let mut n: i64 = 0;
        // binary probe for the border, then walk (keys 1..n are present)
        let mut step: i64 = 1;
        while cx.child(me, CtrlKey::Idx(n + step)).is_some() {
            n += step;
            step *= 2;
        }
        while step > 1 {
            step /= 2;
            if cx.child(me, CtrlKey::Idx(n + step)).is_some() {
                n += step;
            }
        }
        n + 1
    }

    /// `t_insert(self.controls, c)` for an existing control.
    fn t_insert_control(cx: &mut Cx, me: CtrlId, id: CtrlId) {
        let k = Self::next_index(cx, me);
        cx.put(me, CtrlKey::Idx(k), id);
    }

    /// @pob Classes/ConfigTab.lua:132-671 h=415b468b
    fn ctor_init(cx: &mut Cx, me: CtrlId) {
        let model = model_ref(cx);
        // the model's control-state tables (calc:BM-B; Init_model has run it; idempotent)
        model.configTab.ctor_controls_model(model.data);

        // L140-148 input/placeholder/defaultState and the config sets: model (calc:BM-B ctor_model)
        // L150 self.enemyLevel = 1: model (UpdateLevel has set it since, from the loaded build)
        // L152 self.sectionList = { }, L153 self.varControls = { }, L155 self.toggleConfigs = false:
        // State::default()
        // L157-163 A misc calculator function which is updated by the build when it is rebuilt;
        // a calculator base output matching the calcFunc
        cx.build_mut().calc_func = None;
        cx.build_mut().calc_base = None;
        // L163
        let section_anchor = cx.add(
            me,
            "sectionAnchor",
            LabelControl::new(Anchor::new(TopLeft, me, TopLeft), rect(0, 20, 0, 0), ""),
        );

        // Set selector
        // L166-169
        let set_select = cx.add(
            me,
            "setSelect",
            DropDownControl::new(
                Anchor::new(TopLeft, section_anchor, TopLeft),
                rect(76, -12, 210, 20),
                Vec::new(),
                Some(Rc::new(
                    move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L167 self:SetActiveConfigSet(self.configSetOrderList[index]): a nil id is
                        // the active set (SetActiveConfigSet, ConfigTab.lua:1343-1345)
                        let m = model_ref(cx);
                        let id = m
                            .configTab
                            .configSetOrderList
                            .with(|l| l.get(index.wrapping_sub(1)).copied())
                            .map_or_else(|| m.configTab.activeConfigSetId.get(), f64::from);
                        let f = crate::build::init::hooks(cx)
                            .config_set_active_config_set
                            .clone();
                        f(cx, id);
                        // L168
                        let h = config_hooks();
                        (h.add_undo_state)(cx, me);
                    },
                )),
                None,
                false,
            ),
        );
        // L170
        cx.get_mut(set_select).enable_dropped_width = true;
        // L171-173
        cx.base_mut(set_select).enabled =
            Prop::f(|cx, _| cx.build().model.configTab.configSetOrderList.len() > 1);
        // L174
        cx.add(
            me,
            "setLabel",
            LabelControl::new(
                Anchor::new(Right, set_select, Left),
                rect(-2, 0, 0, 16),
                "^7Config set:",
            ),
        );
        // L175-177
        cx.add(
            me,
            "setManage",
            ButtonControl::new(
                Anchor::new(Left, set_select, Right),
                rect(4, 0, 90, 20),
                "Manage...",
                move |cx| {
                    // L176
                    let h = config_hooks();
                    (h.open_config_set_manage_popup)(cx, me);
                },
            ),
        );

        // L179-181
        let search = cx.add(
            me,
            "search",
            EditControl::new(
                Anchor::new(TopLeft, section_anchor, TopLeft),
                rect(8, 15, 360, 20),
                EditArgs {
                    init: Some(String::new()),
                    prompt: Some("Search".into()),
                    filter: Some("%c".into()),
                    limit: Some(100),
                    change_func: Some(Rc::new(move |cx: &mut Cx, _buf: &str| {
                        // L180
                        let h = config_hooks();
                        (h.update_controls)(cx, me);
                    })),
                    clearable: true,
                    ..Default::default()
                },
            ),
        );
        // L182-187
        cx.add(
            me,
            "toggleConfigs",
            ButtonControl::new(
                Anchor::new(Left, search, Right),
                rect(10, 0, 200, 20),
                Prop::f(move |cx, _| {
                    // L183-184 dynamic text
                    if Self::st(cx, me).toggle_configs {
                        "Hide Ineligible Configurations".to_owned()
                    } else {
                        "Show All Configurations".to_owned()
                    }
                }),
                move |cx| {
                    // L186
                    let s = Self::st_mut(cx, me);
                    s.toggle_configs = !s.toggle_configs;
                },
            ),
        );

        // L189-258 the local functions: is_collapsed, search_match, is_show_all_config,
        // imply_cond, list_or_single_if_option, list_or_single_if_tooltip
        // L260
        let mut last_section: Option<CtrlId> = None;
        // L261
        for var_data in &model::data().configOptions {
            // L262
            if let Some(section_name) = &var_data.section {
                // L263
                let section = cx
                    .add_free(
                        Some(me),
                        SectionControl::new(
                            Anchor::new(TopLeft, search, BottomLeft),
                            rect(0, 0, 360, 0),
                            section_name.to_string(),
                        ),
                    )
                    .id();
                last_section = Some(section);
                // L264-266
                Self::st_mut(cx, me).sections.insert(
                    section,
                    SectionData {
                        var_control_list: Vec::new(),
                        col: var_data.col,
                        collapsed: false,
                    },
                );
                // L267-279
                cx.base_mut(section).height = Prop::f(move |cx, section| {
                    // L268
                    if Self::is_collapsed(cx, me, section) {
                        // L269
                        return 16.0;
                    }
                    // L271
                    let mut height = 20.0;
                    // L272 ORDER: a sum (module notes)
                    let list = Self::st(cx, me).sections[&section].var_control_list.clone();
                    for var_control in list {
                        // L273
                        if cx.is_shown(var_control) {
                            // L274
                            let (_, ctrl_height) = cx.size(var_control);
                            // L275
                            height = height + m_max(ctrl_height, 16.0) + 4.0;
                        }
                    }
                    // L278
                    m_max(height, 32.0)
                });
                // Collapse toggle, matching the Calcs tab: right aligned, '-' when expanded, '+' when collapsed.
                // Sits on the section's top border, as the header label does, to clear the option controls below.
                // L282
                // L283-287
                let toggle = cx
                    .add_free(
                        Some(me),
                        ButtonControl::new(
                            Anchor::new(TopRight, section, TopRight),
                            rect(-6, -7, 16, 16),
                            Prop::f(move |cx, _| {
                                // L284
                                if Self::st(cx, me).sections[&section].collapsed {
                                    "+".to_owned()
                                } else {
                                    "-".to_owned()
                                }
                            }),
                            move |cx| {
                                // L286
                                let s = Self::st_mut(cx, me).sections.get_mut(&section).unwrap();
                                s.collapsed = !s.collapsed;
                            },
                        ),
                    )
                    .id();
                // Deliberately not in varControlList: it must not count towards the section's height or visibility
                // L289
                Self::st_mut(cx, me).section_list.push(section);
                // L290
                Self::t_insert_control(cx, me, section);
                // L291
                Self::t_insert_control(cx, me, toggle);
                // L292
                if &**section_name == "Custom Modifiers" {
                    // L293
                    Self::st_mut(cx, me).custom_section = Some(section);
                }
            } else {
                let section = last_section.unwrap_or_else(|| {
                    lua_error!("attempt to index upvalue 'lastSection' (a nil value)")
                });
                Self::add_option(cx, me, var_data, section);
            }
        }
        Self::ctor_tail(cx, me);
    }

    /// The option branch of the constructor loop (L295-812).
    fn add_option(
        cx: &mut Cx,
        me: CtrlId,
        var_data: &'static ConfigOptionDef,
        last_section: CtrlId,
    ) {
        let var = var_data.var;
        let ty = var_data.r#type.map(|t| t.as_str());
        let anchor = || Anchor::new(TopLeft, last_section, TopLeft);
        // L296 (`control`; the callbacks find their own control through `self_id`, set below)
        let self_id: Rc<Cell<CtrlId>> = Rc::new(Cell::new(CtrlId::default()));
        let (sid1, sid2, sid3) = (self_id.clone(), self_id.clone(), self_id.clone());
        // L297-346
        let (control, class) = if ty == Some("check") {
            // L298-303
            let c = cx.add_free(
                Some(me),
                CheckBoxControl::new(
                    anchor(),
                    rect(234, 0, 18, 0),
                    var_data.label.as_ref().map(|l| l.to_string()),
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                        ConfigTab::push_var_control(
                            cx,
                            me,
                            sid1.get(),
                            var,
                            VarSource::Check(state),
                        );
                        // L299
                        active_input(cx).set(var, state);
                        // L300-302
                        Self::after_change(cx, me);
                    })),
                    None,
                    false,
                ),
            );
            (c.id(), VarClass::CheckBox)
        } else if matches!(ty, Some("count" | "integer" | "countAllowZero" | "float")) {
            // L304-314
            let filter = if matches!(ty, Some("integer" | "countAllowZero")) {
                "^%-%d"
            } else if ty == Some("float") {
                "^%d."
            } else {
                "%D"
            };
            let c = cx.add_free(
                Some(me),
                EditControl::new(
                    anchor(),
                    rect(234, 0, 90, 18),
                    EditArgs {
                        init: Some(String::new()),
                        filter: Some(filter.into()),
                        limit: Some(10),
                        // L305-314 (changeFunc(buf, true): L306-307, L313)
                        placeholder_change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                            // L307
                            active_placeholder(cx).set(var, Val::from(pob_lua::tonumber(buf)));
                            // L313
                            cx.build().model.buildFlag.set(true);
                        })),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                            ConfigTab::push_var_control(
                                cx,
                                me,
                                sid2.get(),
                                var,
                                VarSource::Buf(buf.to_owned()),
                            );
                            // L309
                            active_input(cx).set(var, Val::from(pob_lua::tonumber(buf)));
                            // L310-313
                            Self::after_change(cx, me);
                        })),
                        ..Default::default()
                    },
                ),
            );
            (c.id(), VarClass::Edit)
        } else if ty == Some("list") {
            // L315-321
            let list: Vec<DropItem> = var_data
                .list
                .iter()
                .flatten()
                .map(|item| {
                    let v: ListVal = Val::from_value(&item.val);
                    DropItem::new(item.label.to_string(), Rc::new(v))
                })
                .collect();
            let c = cx.add_free(
                Some(me),
                DropDownControl::new(
                    anchor(),
                    rect(234, 0, 118, 16),
                    list,
                    Some(Rc::new(
                        move |cx: &mut Cx, index: usize, value: Option<&DropItem>| {
                            let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                            ConfigTab::push_var_control(
                                cx,
                                me,
                                sid3.get(),
                                var,
                                VarSource::SelIndex(index),
                            );
                            // L317 value.val
                            let value = value.unwrap_or_else(|| {
                                lua_error!("attempt to index local 'value' (a nil value)")
                            });
                            let v = value.get::<ListVal>().cloned().unwrap_or(Val::Nil);
                            active_input(cx).set(var, v);
                            // L318-320
                            Self::after_change(cx, me);
                        },
                    )),
                    None,
                    false,
                ),
            );
            (c.id(), VarClass::DropDown)
        } else if ty == Some("text") && !VAR_EXTRAS.resizable {
            // L322-332
            let c = cx.add_free(
                Some(me),
                EditControl::new(
                    anchor(),
                    rect(8, 0, 344, 118),
                    Self::text_args(me, var, self_id.clone()),
                ),
            );
            (c.id(), VarClass::Edit)
        } else if ty == Some("text") && VAR_EXTRAS.resizable {
            // L333-343
            let c = cx.add_free(
                Some(me),
                ResizableEditControl::new(
                    anchor(),
                    rect(8, 0, 344, 118),
                    Self::text_args(me, var, self_id.clone()),
                    None,
                    None,
                    None,
                    Some(118.0 + 16.0 * 40.0),
                ),
            );
            (c.id(), VarClass::ResizableEdit)
        } else {
            // L345
            let c = cx.add_free(Some(me), PlainControl::new(anchor(), rect(234, 0, 16, 16)));
            (c.id(), VarClass::Control)
        };
        self_id.set(control);

        // L348-350 (varData.inactiveText: never set, see the module notes)
        if let Some(t) = &VAR_EXTRAS.inactive_text {
            if matches!(class, VarClass::Edit | VarClass::ResizableEdit) {
                cx.w_mut::<EditControl>(control).inactive_text =
                    Some(pob_ui_core::controls::edit::InactiveText::Str(t.clone()));
            }
        }

        // L352
        let shown_funcs: Rc<RefCell<Vec<ShownFn>>> = Rc::new(RefCell::new(Vec::new()));
        // L353-364
        let s1: ShownFn = {
            let shown_funcs = shown_funcs.clone();
            Rc::new(move |cx: &Cx| {
                // L354
                if !Self::search_match(cx, me, var_data) {
                    // L355
                    return false;
                }
                // L358
                let funcs = shown_funcs.borrow().clone();
                for shown_func in funcs {
                    // L359
                    if !shown_func(cx) && !Self::is_show_all_config(cx, me, var_data) {
                        // L360
                        return false;
                    }
                }
                // L363
                true
            })
        };
        {
            let s1 = s1.clone();
            cx.base_mut(control).shown = Prop::f(move |cx, _| s1(cx));
        }

        // L366
        let tooltip_funcs: Rc<RefCell<Vec<TipPart>>> = Rc::new(RefCell::new(Vec::new()));
        // L367-376
        let tooltip_text: Prop<Option<String>> = {
            let tooltip_funcs = tooltip_funcs.clone();
            Prop::f(move |cx, _| {
                // L368
                let mut out: Option<String> = None;
                // L369
                let funcs = tooltip_funcs.borrow().clone();
                for tooltip_func in funcs {
                    // L370
                    let cur = match tooltip_func {
                        TipPart::Str(s) => Some(s),
                        TipPart::Fn(f) => f(cx),
                    };
                    // L371
                    if let Some(cur) = cur {
                        // L372
                        out = Some(match out {
                            Some(o) => o + "\n" + &cur,
                            None => cur,
                        });
                    }
                }
                // L375
                out
            })
        };
        Self::set_tooltip_text(cx, control, class, tooltip_text);

        // L378-380
        if let Some(t) = &var_data.tooltip {
            tooltip_funcs.borrow_mut().push(match t {
                OptTooltip::Str(s) => TipPart::Str(s.to_string()),
                OptTooltip::Fn(f) => {
                    let f = f.clone();
                    TipPart::Fn(Rc::new(move |cx| call_option_tooltip(cx, &f)))
                }
            });
        }

        let push_shown = |f: ShownFn| shown_funcs.borrow_mut().push(f);
        let push_tip = |f: TipFn| tooltip_funcs.borrow_mut().push(TipPart::Fn(f));

        // L382-395 (varData.ifNode: never set)
        if let Some(if_node) = VAR_EXTRAS.if_node.clone() {
            push_shown(node_if_option(if_node.clone()));
            push_tip(node_if_tooltip(if_node));
        }
        // L396-400
        if let Some(if_option) = single(var_data.ifOption) {
            push_shown(list_or_single_if_option(
                if_option,
                Rc::new(|cx, opt| {
                    // L398
                    match opt {
                        IfArg::One(o) => active_input(cx).truthy(o),
                        IfArg::Table => false,
                    }
                }),
            ));
        }
        // the `implyCond(varData) or mainEnv.<used>[ifOption]` shown funcs (L402-407, :424-429,
        // :446-451, :480-485, :502-507, :551-556, :573-578, :603-608) and their dev tooltips
        let used_shown = |pick: fn(&model::Env) -> Option<Table>, name: &'static str| -> IfFunc {
            Rc::new(move |cx, opt| {
                if Self::imply_cond(cx, var_data) {
                    return true;
                }
                opt.index(&used(pick(&main_env(cx)), name))
            })
        };
        let used_tip = |pick: fn(&model::Env) -> Option<Table>| -> IfTipFn {
            Rc::new(move |cx, opt| {
                let env = main_env(cx);
                used_mods_tooltip(cx, pick(&env).as_ref(), opt)
            })
        };
        // L401-422
        if let Some(o) = var_data.ifCond.clone() {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.conditionsUsed.get(), "conditionsUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.conditionsUsed.get()),
            ));
        }
        // L423-444
        if let Some(o) = single(var_data.ifMinionCond) {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.minionConditionsUsed.get(), "minionConditionsUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.minionConditionsUsed.get()),
            ));
        }
        // L445-466
        if let Some(o) = single(var_data.ifEnemyCond) {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.enemyConditionsUsed.get(), "enemyConditionsUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.enemyConditionsUsed.get()),
            ));
        }
        // L467-478
        if let Some(o) = single(var_data.ifCondTrue) {
            push_shown(list_or_single_if_option(
                o.clone(),
                Rc::new(|cx, opt| {
                    // L469
                    opt.index(&main_env(cx).player.modDB.get().conditions)
                }),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                Rc::new(|cx, opt| {
                    // L472
                    if !cx.app().launch.dev_mode_alt {
                        // L473
                        return None;
                    }
                    // L475
                    let c = main_env(cx).player.modDB.get().conditions.get(opt);
                    Some(format!("Condition state: {opt}={}", tostring_val(&c)))
                }),
            ));
        }
        // L479-500
        if let Some(o) = single(var_data.ifMult) {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.multipliersUsed.get(), "multipliersUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.multipliersUsed.get()),
            ));
        }
        // L501-522
        if let Some(o) = single(var_data.ifEnemyMult) {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.enemyMultipliersUsed.get(), "enemyMultipliersUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.enemyMultipliersUsed.get()),
            ));
        }
        // L523-548
        if let Some(o) = var_data.ifStat.clone() {
            push_shown(list_or_single_if_option(
                o.clone(),
                Rc::new(move |cx, opt| {
                    // L525
                    if Self::imply_cond(cx, var_data) {
                        // L526
                        return true;
                    }
                    let env = main_env(cx);
                    // L528
                    opt.index(&used(env.perStatsUsed.get(), "perStatsUsed"))
                        || opt.index(&used(
                            env.enemyMultipliersUsed.get(),
                            "enemyMultipliersUsed",
                        ))
                }),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                Rc::new(|cx, opt| {
                    // L531
                    if !cx.app().launch.dev_mode_alt {
                        // L532
                        return None;
                    }
                    let env = main_env(cx);
                    // L534
                    let mut out: Option<String> = None;
                    // L535
                    let mods = used(env.perStatsUsed.get(), "perStatsUsed").get(opt);
                    // L536
                    if mods.truthy() {
                        append_mods(&mut out, &mods);
                    }
                    // L541
                    let mods2 =
                        used(env.enemyMultipliersUsed.get(), "enemyMultipliersUsed").get(opt);
                    // L542
                    if mods2.truthy() {
                        append_mods(&mut out, &mods2);
                    }
                    // L547
                    out
                }),
            ));
        }
        // L550-570 (varData.ifEnemyStat: never set)
        if let Some(o) = VAR_EXTRAS.if_enemy_stat.clone() {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.enemyPerStatsUsed.get(), "enemyPerStatsUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.enemyPerStatsUsed.get()),
            ));
        }
        // L572-592
        if let Some(o) = single(var_data.ifTagType) {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.tagTypesUsed.get(), "tagTypesUsed"),
            ));
            push_tip(list_or_single_if_tooltip(
                o,
                used_tip(|e| e.tagTypesUsed.get()),
            ));
        }
        // L594-600
        if let Some(o) = single(var_data.ifFlag) {
            push_shown(list_or_single_if_option(
                o,
                Rc::new(|cx, opt| {
                    let env = main_env(cx);
                    let main_skill = env.player.mainSkill.get().unwrap_or_else(|| {
                        lua_error!("attempt to index field 'mainSkill' (a nil value)")
                    });
                    // L596
                    let skill_mod_list = main_skill.skillModList.get();
                    // L597
                    let skill_flags = main_skill.skillFlags.get();
                    // Check both the skill mods for flags and flags that are set via calcPerform
                    // L599
                    match opt {
                        IfArg::One(o) => skill_flags.truthy(o) || skill_mod_list.Flag(None, &[o]),
                        IfArg::Table => false,
                    }
                }),
            ));
        }
        // L602-622
        if let Some(o) = var_data.ifMod.clone() {
            push_shown(list_or_single_if_option(
                o.clone(),
                used_shown(|e| e.modsUsed.get(), "modsUsed"),
            ));
            push_tip(list_or_single_if_tooltip(o, used_tip(|e| e.modsUsed.get())));
        }
        // L624-642
        if let Some(o) = var_data.ifSkill.clone() {
            // L625
            if var_data.includeTransfigured {
                // L626-636
                push_shown(list_or_single_if_option(
                    o,
                    Rc::new(|cx, opt| {
                        let IfArg::One(o) = opt else {
                            // L627 `type(gemName) ~= "string"`: nil
                            return false;
                        };
                        // L627
                        if pob_calc::modules::calc_tools::getGameIdFromGemName(
                            Some(o.as_str()),
                            true,
                        )
                        .is_none()
                        {
                            // L628
                            return false;
                        }
                        // L630 ORDER: an any-match (module notes)
                        let skills_used = used(main_env(cx).skillsUsed.get(), "skillsUsed");
                        for (skill, _) in skills_used.pairs() {
                            let Some(skill) = skill.name() else { continue };
                            // L631
                            if pob_calc::modules::calc_tools::isGemIdSame(
                                skill.as_str(),
                                o.as_str(),
                                true,
                            ) {
                                // L632
                                return true;
                            }
                        }
                        // L635
                        false
                    }),
                ));
            } else {
                // L637-640
                push_shown(list_or_single_if_option(
                    o,
                    Rc::new(|cx, opt| {
                        // L639
                        opt.index(&used(main_env(cx).skillsUsed.get(), "skillsUsed"))
                    }),
                ));
            }
        }
        // L643-662 ifSkillFlag / ifSkillData
        let active_skills_any =
            |field: fn(&pob_calc::active_skill::ActiveSkill) -> Table| -> IfFunc {
                Rc::new(move |cx, opt| {
                    // L645
                    let list = main_env(cx).player.activeSkillList.get().with(Clone::clone);
                    for active_skill in list {
                        // L646
                        if opt.index(&field(&active_skill)) {
                            // L647
                            return true;
                        }
                    }
                    // L650
                    false
                })
            };
        if let Some(o) = single(var_data.ifSkillFlag) {
            push_shown(list_or_single_if_option(
                o,
                active_skills_any(|a| a.skillFlags.get()),
            ));
        }
        // L653-662
        if let Some(o) = single(var_data.ifSkillData) {
            push_shown(list_or_single_if_option(
                o,
                active_skills_any(|a| a.skillData.get()),
            ));
        }

        // L664-666
        if let Some(f) = &var_data.tooltipFunc {
            let f = f.clone();
            Self::set_tooltip_func(
                cx,
                control,
                class,
                Rc::new(move |cx, tooltip, args| call_option_tooltip_func(cx, &f, tooltip, &args)),
            );
        }
        // L667
        let mut label_control = control;
        let mut label_is_label = false;
        // L668
        if let Some(label) = var_data.label.as_ref().filter(|_| ty != Some("check")) {
            // L669
            let h = if cx.sg.draw_string_width(14.0, Font::Var, label) > 228.0 {
                12.0
            } else {
                14.0
            };
            label_control = cx
                .add_free(
                    Some(me),
                    LabelControl::new(
                        Anchor::new(Right, control, Left),
                        rect(-4, 0, 0, h),
                        format!("^7{label}"),
                    ),
                )
                .id();
            label_is_label = true;
            // L670
            Self::t_insert_control(cx, me, label_control);
        }
        // L672-692: `if varData.var then` (in scope of calc:BM-B; the model statements ran in
        // Init_model: L673, L676, L679 input/placeholder, L682-691 defaultState). The control
        // statements:
        if let Some(var) = var {
            let ds = var_data
                .defaultState
                .as_ref()
                .map_or(Val::Nil, Val::from_value);
            // L674 control.state = varData.defaultState
            if class == VarClass::CheckBox {
                cx.w_mut::<CheckBoxControl>(control).state = ds.truthy();
            }
            // L675
            Self::st_mut(cx, me)
                .var_controls
                .insert(var, VarControl { id: control, class });
            // L677 control.placeholder = varData.defaultPlaceholderState
            if matches!(class, VarClass::Edit | VarClass::ResizableEdit) {
                let ph = var_data
                    .defaultPlaceholderState
                    .as_ref()
                    .map(|v| tostring_val(&Val::from_value(v)));
                cx.w_mut::<EditControl>(control).placeholder = ph;
            }
            // L678
            if let Some(default_index) = var_data.defaultIndex {
                // L680
                if class == VarClass::DropDown {
                    cx.w_mut::<DropDownControl>(control).sel_index = default_index as usize;
                }
            }
        }

        // @pob Classes/ConfigTab.lua:693-833 h=351a5ff6
        // L695
        let inner_shown = s1.clone();
        // L696
        if !VAR_EXTRAS.do_not_highlight {
            // L697-708
            let inner_shown = inner_shown.clone();
            let border = move |cx: &Cx| -> (f64, f64, f64) {
                // L698
                let shown = inner_shown(cx);
                // L699-700
                if is_changed(cx, var) {
                    // L702
                    if !shown {
                        // L703
                        return (0.753, 0.502, 0.502);
                    }
                    // L705
                    return (0.451, 0.576, 0.702);
                }
                // L707
                (0.5, 0.5, 0.5)
            };
            Self::set_border_func(cx, control, class, Rc::new(border));
        }

        // L711
        if !var_data.hideIfInvalid {
            // L712-720
            {
                let inner_shown = inner_shown.clone();
                cx.base_mut(control).shown = Prop::f(move |cx, _| {
                    // L713
                    if !Self::search_match(cx, me, var_data) {
                        // L714
                        return false;
                    }
                    // L716
                    let shown = inner_shown(cx);
                    // L717-719
                    #[allow(clippy::nonminimal_bool)] // L719, as written
                    let r = !shown && is_changed(cx, var) || shown;
                    r
                });
            }
            // L721-730
            if label_is_label {
                let inner_label = cx.w::<LabelControl>(label_control).label.clone();
                let inner_shown = inner_shown.clone();
                cx.w_mut::<LabelControl>(label_control).label = Prop::f(move |cx, me_l| {
                    // L723
                    let shown = inner_shown(cx);
                    // L724-726
                    if !shown && is_changed(cx, var) {
                        // L727
                        return format!(
                            "{}{}",
                            cx.main.cc.NEGATIVE,
                            StripEscapes(&inner_label.get(cx, me_l))
                        );
                    }
                    // L729
                    inner_label.get(cx, me_l)
                });
            } else if class == VarClass::CheckBox {
                let inner_label = cx.w::<CheckBoxControl>(label_control).label.clone();
                let inner_shown = inner_shown.clone();
                cx.w_mut::<CheckBoxControl>(label_control).label = Prop::f(move |cx, me_l| {
                    // L723
                    let shown = inner_shown(cx);
                    // L724-726
                    if !shown && is_changed(cx, var) {
                        // L727 StripEscapes(nil) raises
                        let l = inner_label.get(cx, me_l).unwrap_or_else(|| {
                            lua_error!("bad argument #1 to 'gsub' (string expected, got nil)")
                        });
                        return Some(format!("{}{}", cx.main.cc.NEGATIVE, StripEscapes(&l)));
                    }
                    // L729
                    inner_label.get(cx, me_l)
                });
            }
            // (an Edit/DropDown/Control `labelControl` without a label: the assigned `label`
            // function is a field these classes never read)
            // L731-732
            let output_cache: Rc<RefCell<HashMap<String, Output>>> =
                Rc::new(RefCell::new(HashMap::new()));
            let output_cache_revision: Rc<Cell<Option<f64>>> = Rc::new(Cell::new(None));
            // L733
            let inner_tooltip_func = var_data.tooltipFunc.clone();
            // L734-798
            let inner_shown = inner_shown.clone();
            Self::set_tooltip_func(
                cx,
                control,
                class,
                Rc::new(move |cx, tooltip, args| {
                    Self::option_tooltip(
                        cx,
                        me,
                        control,
                        class,
                        var_data,
                        &inner_shown,
                        inner_tooltip_func.as_ref(),
                        &output_cache,
                        &output_cache_revision,
                        tooltip,
                        &args,
                    );
                }),
            );
        }

        // L801
        let own_section = last_section;
        // L802
        let eligible_shown = cx.base(control).shown.clone();
        // L803-808
        cx.base_mut(control).shown = Prop::f(move |cx, id| {
            // L804
            if Self::is_collapsed(cx, me, own_section) {
                // L805
                return false;
            }
            // L807
            eligible_shown.get(cx, id)
        });

        // L810
        Self::t_insert_control(cx, me, control);
        // L811
        Self::st_mut(cx, me)
            .sections
            .get_mut(&last_section)
            .unwrap_or_else(|| lua_error!("attempt to index local 'lastSection' (a nil value)"))
            .var_control_list
            .push(control);
    }

    /// The constructor after the option loop (L814-831) and the init-order tail.
    fn ctor_tail(cx: &mut Cx, me: CtrlId) {
        // L814
        cx.add(
            me,
            "scrollBar",
            ScrollBarControl::new(
                Anchor::new(TopRight, me, TopRight),
                rect(0, 0, 18, 0),
                Some(50.0),
                Some(ScrollDir::Vertical),
                true,
            ),
        );
        // L815
        if let Some(custom_section) = Self::st(cx, me).custom_section {
            // L816-823
            let add_block = cx.add(
                me,
                "customModsAddBlock",
                ButtonControl::new(
                    Anchor::new(TopLeft, custom_section, TopLeft),
                    rect(8, 0, 120, 20),
                    "^7Add Mod Group",
                    move |cx| {
                        // L817
                        let custom_mods_list = active_set(cx)
                            .table(n!("customModsList"))
                            .unwrap_or_else(|| {
                                lua_error!(
                                    "attempt to get length of local 'customModsList' (a nil value)"
                                )
                            });
                        // L818
                        let t = Table::new();
                        t.set(
                            n!("title"),
                            format!("Group {}", custom_mods_list.len() + 1).as_str(),
                        );
                        t.set(n!("enabled"), true);
                        t.set(n!("text"), "");
                        custom_mods_list.push(t);
                        // L819
                        Self::update_custom_mods_controls(cx, me);
                        // L820-822
                        Self::after_change(cx, me);
                    },
                ),
            );
            // L824-826
            cx.base_mut(add_block).shown = Prop::f(move |cx, _| {
                // L825
                let s = Self::st(cx, me).custom_section.unwrap();
                !Self::is_collapsed(cx, me, s)
            });
            // L827
            Self::st_mut(cx, me).custom_mods_block_controls = Some(Vec::new());
            // L828
            // DIVERGENCE(init-order): in PoB this call sees NewConfigSet's default list
            // (ConfigTab.lua:1281), and a loaded `Config` section's Load (L937
            // SetActiveConfigSet -> L1352 UpdateControls -> L1021) then hides those blocks and
            // builds the loaded ones. The model is loaded already, so the first call gets a
            // default list of its own and the second one the model's: the same controls, in the
            // same `self.controls` slots.
            let model = model_ref(cx);
            let loaded = crate::model::build_xml::xmlSectionList(&model)
                .iter()
                .any(|n| &*n.elem == "Config");
            if loaded {
                let default_list = Table::new();
                let t = Table::new();
                t.set(n!("title"), "Default");
                t.set(n!("enabled"), true);
                t.set(n!("text"), "");
                default_list.push(t);
                Self::update_custom_mods_blocks(cx, me, custom_section, &default_list);
            }
            Self::update_custom_mods_controls(cx, me);
        }
        // DIVERGENCE(init-order) (module notes): the control state the loaded build left in the
        // model's control tables
        Self::pull_var_controls(cx, me);
        // L831 return self
    }

    /// The text option's EditArgs (L322-332, L333-343).
    fn text_args(me: CtrlId, var: Option<Name>, self_id: Rc<Cell<CtrlId>>) -> EditArgs {
        EditArgs {
            init: Some(String::new()),
            filter: Some("^%C\t\n".into()),
            // L324-325, L331 changeFunc(buf, true)
            placeholder_change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                // L325 tostring(buf)
                active_placeholder(cx).set(var, buf);
                // L331
                cx.build().model.buildFlag.set(true);
            })),
            change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                ConfigTab::push_var_control(
                    cx,
                    me,
                    self_id.get(),
                    var,
                    VarSource::Buf(buf.to_owned()),
                );
                // L327
                active_input(cx).set(var, buf);
                // L328-331
                Self::after_change(cx, me);
            })),
            line_height: Some(16.0),
            ..Default::default()
        }
    }

    /// `self:AddUndoState()`, `self:BuildModList()`, `self.build.buildFlag = true` (the common
    /// tail of the option callbacks), with the bridge pull after BuildModList.
    fn after_change(cx: &mut Cx, me: CtrlId) {
        let h = config_hooks();
        (h.add_undo_state)(cx, me);
        build_mod_list(cx);
        Self::pull_var_controls(cx, me);
        cx.build().model.buildFlag.set(true);
    }

    /// `control.tooltipText = function() … end` (L367) on the control's TooltipHost.
    fn set_tooltip_text(cx: &mut Cx, control: CtrlId, class: VarClass, p: Prop<Option<String>>) {
        match class {
            VarClass::CheckBox => cx.w_mut::<CheckBoxControl>(control).tt.tooltip_text = Some(p),
            VarClass::Edit | VarClass::ResizableEdit => {
                cx.w_mut::<EditControl>(control).tt.tooltip_text = Some(p);
            }
            VarClass::DropDown => cx.w_mut::<DropDownControl>(control).tt.tooltip_text = Some(p),
            // a `Control` has no TooltipHost: the field is never read
            VarClass::Control => {}
        }
    }

    /// `control:GetProperty("tooltipText")`
    fn get_tooltip_text(cx: &Cx, control: CtrlId, class: VarClass) -> Option<String> {
        let p = match class {
            VarClass::CheckBox => cx.w::<CheckBoxControl>(control).tt.tooltip_text.clone(),
            VarClass::Edit | VarClass::ResizableEdit => {
                cx.w::<EditControl>(control).tt.tooltip_text.clone()
            }
            VarClass::DropDown => cx.w::<DropDownControl>(control).tt.tooltip_text.clone(),
            VarClass::Control => None,
        };
        p.and_then(|p| p.get(cx, control))
    }

    /// `control.tooltipFunc = f` on the control's TooltipHost, with the arguments its class
    /// passes.
    fn set_tooltip_func(
        cx: &mut Cx,
        control: CtrlId,
        class: VarClass,
        f: Rc<dyn Fn(&mut Cx, &mut Tooltip, TipArgs)>,
    ) {
        match class {
            VarClass::CheckBox => {
                cx.w_mut::<CheckBoxControl>(control).tt.tooltip_func = Some(Rc::new(
                    move |cx: &mut Cx, tt: &mut Tooltip, state: bool| {
                        f(cx, tt, TipArgs::Check(state));
                    },
                ));
            }
            VarClass::Edit | VarClass::ResizableEdit => {
                cx.w_mut::<EditControl>(control).tt.tooltip_func =
                    Some(Rc::new(move |cx: &mut Cx, tt: &mut Tooltip, (): ()| {
                        f(cx, tt, TipArgs::None);
                    }));
            }
            VarClass::DropDown => {
                cx.w_mut::<DropDownControl>(control).tt.tooltip_func = Some(Rc::new(
                    move |cx: &mut Cx, tt: &mut Tooltip, a: DropTipArgs| {
                        f(cx, tt, TipArgs::Drop(a));
                    },
                ));
            }
            // a `Control` never draws a tooltip
            VarClass::Control => {}
        }
    }

    /// `control.borderFunc = f` (L697).
    fn set_border_func(
        cx: &mut Cx,
        control: CtrlId,
        class: VarClass,
        f: Rc<dyn Fn(&Cx) -> (f64, f64, f64)>,
    ) {
        match class {
            VarClass::CheckBox => cx.w_mut::<CheckBoxControl>(control).border_func = Some(f),
            VarClass::Edit | VarClass::ResizableEdit => {
                cx.w_mut::<EditControl>(control).border_func = Some(f);
            }
            VarClass::DropDown => {
                cx.w_mut::<DropDownControl>(control).border_func =
                    Some(Rc::new(move |cx: &Cx, _id: CtrlId| f(cx)));
            }
            // a `Control` never draws a border
            VarClass::Control => {}
        }
    }

    /// lua: ConfigTab.lua:734-798 (the option's `tooltipFunc(tooltip, mode, index, value)`)
    #[allow(clippy::too_many_arguments)]
    fn option_tooltip(
        cx: &mut Cx,
        me: CtrlId,
        control: CtrlId,
        class: VarClass,
        var_data: &'static ConfigOptionDef,
        inner_shown: &ShownFn,
        inner_tooltip_func: Option<&LuaFnRef>,
        output_cache: &Rc<RefCell<HashMap<String, Output>>>,
        output_cache_revision: &Rc<Cell<Option<f64>>>,
        tooltip: &mut Tooltip,
        args: &TipArgs,
    ) {
        let _ = me;
        let var = var_data.var;
        let ty = var_data.r#type.map(|t| t.as_str());
        // L735
        tooltip.clear(false);

        // L737
        if let Some(f) = inner_tooltip_func {
            // L738
            call_option_tooltip_func(cx, f, tooltip, args);
        } else {
            // L740
            let tooltip_text = Self::get_tooltip_text(cx, control, class);
            // L741
            if let Some(t) = tooltip_text.filter(|t| !t.is_empty()) {
                // L742
                tooltip.add_line(cx, 14.0, &t);
            }
        }

        // L746
        let shown = inner_shown(cx);
        // L747
        let inputs = active_input(cx);
        // L748
        let cur = var.map_or(Val::Nil, |v| inputs.get(v));
        // L749
        let def = default_state(cx, var, &cur);
        // L750
        if !shown && !cur.is_nil() && !cur.raw_eq(&def) {
            // L751
            let neg = cx.main.cc.NEGATIVE.clone();
            tooltip.add_line(
                cx,
                14.0,
                &format!(
                    "{neg}This config option is conditional with missing source and is invalid."
                ),
            );
        } else {
            // avoid adding comparisons for number inputs as the
            // input gets applied as soon as the user types, which
            // means comparisons don't make sense here
            // L756
            if cx.build().calc_func.is_none() {
                // L757
                let m = model_ref(cx);
                if let std::task::Poll::Ready((f, base)) = cx.build_mut().calc.misc_calculator(&m) {
                    cx.build_mut().calc_func = Some(f);
                    cx.build_mut().calc_base = Some(base);
                }
            }
            // L759
            if ty == Some("check") || ty == Some("list") {
                // L760-765
                let value_mapped = if ty == Some("check") {
                    // L762
                    Val::Bool(!cur.truthy())
                } else {
                    // L764 DIVERGENCE(list-falsy-val)
                    args.value().map_or(Val::Nil, |v| {
                        v.get::<ListVal>().cloned().unwrap_or(Val::Nil)
                    })
                };
                // L766
                if !value_mapped.raw_eq(&cur) {
                    let model = model_ref(cx);
                    // L767
                    let build_flag = model.buildFlag.get();
                    // L768
                    tooltip.add_separator(cx, Some(10.0));
                    // clear cache if build has been edited
                    // L770
                    let rev = model.outputRevision.get();
                    if output_cache_revision.get() != Some(rev) {
                        // L771
                        output_cache.borrow_mut().clear();
                        // L772
                        output_cache_revision.set(Some(rev));
                    }
                    // L774
                    let key = format!("{}:{}", tostring_val(&value_mapped), tostring_val(&cur));
                    // L775
                    if !output_cache.borrow().contains_key(&key) {
                        let var = var.unwrap_or_else(|| lua_error!("table index is nil"));
                        // L776
                        inputs.set(var, value_mapped.clone());
                        // L777
                        model.configTab.BuildModList(&model);

                        // L779
                        let calc_func = cx.build().calc_func.clone().unwrap_or_else(|| {
                            lua_error!("attempt to call field 'calcFunc' (a nil value)")
                        });
                        let out = calc_func.call(None, None);

                        // L781
                        inputs.set(var, cur.clone());
                        // L782
                        model.configTab.BuildModList(&model);
                        // (the applies' writes to the control tables are undone by the second
                        // BuildModList: no bridge pull)
                        if let std::task::Poll::Ready(out) = out {
                            output_cache.borrow_mut().insert(key.clone(), out);
                        }
                    }
                    // building the mod lists flags the build for a
                    // rebuild, but we don't actually want that as
                    // we restore the previous state if the user
                    // hasn't actually clicked
                    // L788
                    model.buildFlag.set(build_flag);
                    // L789
                    let prefix = if ty == Some("check") {
                        "^7Toggling this"
                    } else {
                        "^7Selecting this"
                    };
                    // L790
                    let cached = output_cache.borrow().get(&key).cloned();
                    let base = cx.build().calc_base.clone();
                    if let (Some(base), Some(out)) = (base, cached) {
                        crate::build::stats::add_stat_compares_to_tooltip(
                            cx,
                            tooltip,
                            &base,
                            &out,
                            &format!("{prefix} option will give you:"),
                            None,
                        );
                    }
                    // clear tooltip if it only has our separator
                    // L792
                    if tooltip.lines.len() == 1 {
                        // L793
                        tooltip.clear(false);
                    }
                }
            }
        }
    }

    // ---- the constructor's local functions (L190-259) -----------------------------------------

    /// lua: ConfigTab.lua:189-191 isCollapsed(section)
    fn is_collapsed(cx: &Cx, me: CtrlId, section: CtrlId) -> bool {
        // L190
        Self::is_section_collapsed(cx, me, section)
    }

    /// lua: ConfigTab.lua:193-204 searchMatch(varData)
    fn search_match(cx: &Cx, me: CtrlId, var_data: &ConfigOptionDef) -> bool {
        // L194 (`:gsub` keeps its first result)
        let search = cx.child_as::<EditControl>(me, "search");
        let search_str = gsub(
            &pob_lua::string::lower(&cx.get(search).buf),
            "[%-%.%+%[%]%$%^%%%?%*]",
            "%%%0",
        );
        // L195
        if pob_lua::pattern::r#match(&search_str, "%S", None).is_some() {
            // L196
            let label =
                pob_lua::string::lower(&StripEscapes(var_data.label.as_deref().unwrap_or("")));
            // L197
            let r = match_or_pattern(&label, &search_str);
            // L198
            if let Ok(true) = r {
                // L199
                return true;
            }
            // L201
            return false;
        }
        // L203
        true
    }

    /// lua: ConfigTab.lua:207-209 isShowAllConfig(varData). Override for Show All
    /// Configurations: when the toggle is on, show options that aren't on the shared exclusion
    /// list.
    fn is_show_all_config(cx: &Cx, me: CtrlId, var_data: &ConfigOptionDef) -> bool {
        // L208
        Self::st(cx, me).toggle_configs && !(config_hooks().is_show_all_excluded)(var_data)
    }

    /// lua: ConfigTab.lua:211-229 implyCond(varData)
    fn imply_cond(cx: &Cx, var_data: &ConfigOptionDef) -> bool {
        // L212
        let main_env = main_env(cx);
        // L213
        if input_of(cx, var_data.var).truthy() {
            let conditions_used = || used(main_env.conditionsUsed.get(), "conditionsUsed");
            // L214 (`varData.implyCondList` is `{}` when absent: an empty loop)
            for imply_cond in &var_data.implyCondList {
                // L216
                if conditions_used().truthy(*imply_cond) {
                    // L217
                    return true;
                }
            }
            // L221-223
            if var_data
                .implyCond
                .is_some_and(|c| conditions_used().truthy(c))
                || VAR_EXTRAS.imply_minion_cond.is_some_and(|c| {
                    used(main_env.minionConditionsUsed.get(), "minionConditionsUsed").truthy(c)
                })
                || VAR_EXTRAS.imply_enemy_cond.is_some_and(|c| {
                    used(main_env.enemyConditionsUsed.get(), "enemyConditionsUsed").truthy(c)
                })
            {
                // L224
                return true;
            }
        }
        // L228
        false
    }

    /// lua: ConfigTab.lua:835-837 IsSectionCollapsed(section): a collapsed section hides its
    /// contents, unless a search is active (CCR-0110 §2).
    /// @pob Classes/ConfigTab.lua:835-837 h=d804a099
    pub fn is_section_collapsed(cx: &Cx, me: CtrlId, section: CtrlId) -> bool {
        // L836
        let collapsed = Self::st(cx, me)
            .sections
            .get(&section)
            .is_some_and(|s| s.collapsed);
        let search = cx.child_as::<EditControl>(me, "search");
        collapsed && pob_lua::pattern::r#match(&cx.get(search).buf, "%S", None).is_none()
    }

    /// lua: ConfigTab.lua:1302-1334 UpdateCustomModsControls (the model half, 1306-1316, is
    /// calc:BM-B's `ConfigTab::UpdateCustomModsControls`; CCR-0110 §2)
    /// @pob Classes/ConfigTab.lua:1302-1334 h=378e2cbb
    pub fn update_custom_mods_controls(cx: &mut Cx, me: CtrlId) {
        // L1303
        let Some(custom_section) = Self::st(cx, me).custom_section else {
            // L1304
            return;
        };
        // L1306-1316 (model)
        let model = model_ref(cx);
        model.configTab.UpdateCustomModsControls();
        // L1306
        let Some(config_set) = model
            .configTab
            .configSet(model.configTab.activeConfigSetId.get())
        else {
            // L1308
            return;
        };
        // L1325 `configSet.customModsList`
        let custom_mods_list = config_set
            .table(n!("customModsList"))
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"));
        Self::update_custom_mods_blocks(cx, me, custom_section, &custom_mods_list);
    }

    /// L1317-1333 of UpdateCustomModsControls over `customModsList` (the constructor also runs it
    /// over the default list, see [`Self::ctor_tail`]).
    fn update_custom_mods_blocks(
        cx: &mut Cx,
        me: CtrlId,
        custom_section: CtrlId,
        custom_mods_list: &Table,
    ) {
        // L1317
        if let Some(list) = Self::st(cx, me).custom_mods_block_controls.clone() {
            // L1318
            for ctrl in list {
                // L1319
                cx.base_mut(ctrl).shown = Prop::Val(false);
            }
        }
        // L1322
        Self::st_mut(cx, me).custom_mods_block_controls = Some(Vec::new());
        // L1323
        let add_block = cx.child(me, "customModsAddBlock").unwrap_or_else(|| {
            lua_error!("attempt to index field 'customModsAddBlock' (a nil value)")
        });
        Self::st_mut(cx, me)
            .sections
            .get_mut(&custom_section)
            .unwrap()
            .var_control_list = vec![add_block];

        // L1325
        for (index, block) in custom_mods_list.ipairs() {
            let Val::Table(block) = block else {
                lua_error!(
                    "attempt to index local 'blockData' (a {} value)",
                    block.lua_type()
                )
            };
            // L1326
            let block_control = cx
                .add_free(
                    Some(me),
                    CustomModBlockControl::new(
                        Anchor::new(TopLeft, custom_section, TopLeft),
                        rect(8, 0, 344, 120),
                        me,
                        index,
                        block,
                    ),
                )
                .id();
            // L1327-1329
            cx.base_mut(block_control).shown = Prop::f(move |cx, _| {
                // L1328
                let s = Self::st(cx, me).custom_section.unwrap();
                !Self::is_section_collapsed(cx, me, s)
            });
            // L1330
            Self::st_mut(cx, me)
                .custom_mods_block_controls
                .get_or_insert_with(Vec::new)
                .push(block_control);
            // L1331
            Self::t_insert_control(cx, me, block_control);
            // L1332
            Self::st_mut(cx, me)
                .sections
                .get_mut(&custom_section)
                .unwrap()
                .var_control_list
                .push(block_control);
        }
    }

    /// The default `UpdateControls` hook (ConfigTab.lua:1008-1022 is U42's): the model half
    /// over the control tables (calc:BM-B), the bridge pull, and the UI half of
    /// UpdateCustomModsControls (L1021).
    pub fn update_controls_default(cx: &mut Cx, me: CtrlId) {
        let model = model_ref(cx);
        model.configTab.UpdateControls(&model);
        Self::pull_var_controls(cx, me);
        Self::update_custom_mods_controls(cx, me);
    }

    // ---- the var-controls bridge (module notes, CCR-0110 §3) -----------------------------------

    /// UI -> model: the state the control `self.varControls[var]` just took, into its control
    /// table (only when `control` is that control: a duplicate option's earlier control has no
    /// table).
    pub fn push_var_control(cx: &mut Cx, me: CtrlId, control: CtrlId, var: Name, src: VarSource) {
        if Self::st(cx, me).var_controls.get(&var).map(|v| v.id) != Some(control) {
            return;
        }
        let table = cx.build().model.configTab.varControl(var);
        match src {
            VarSource::Check(state) => table.set(n!("state"), state),
            VarSource::Buf(buf) => table.set(n!("buf"), buf.as_str()),
            VarSource::SelIndex(i) => table.set(n!("selIndex"), i as f64),
        }
    }

    /// model -> UI: every control table's state into its control (`buf` through `SetText`
    /// only when it differs, `placeholder`, `state`, `selIndex`, `enabled`).
    pub fn pull_var_controls(cx: &mut Cx, me: CtrlId) {
        let model = model_ref(cx);
        let controls: Vec<(Name, VarControl)> = Self::st(cx, me)
            .var_controls
            .iter()
            .map(|(k, v)| (*k, *v))
            .collect();
        for (var, vc) in controls {
            let t = match model.configTab.varControls.get(var) {
                Val::Table(t) => t,
                _ => continue,
            };
            let id = vc.id;
            match vc.class {
                VarClass::Edit | VarClass::ResizableEdit => {
                    let buf = t.get(n!("buf"));
                    if !buf.is_nil() {
                        let buf = tostring_val(&buf);
                        if cx.w::<EditControl>(id).buf != buf {
                            EditControl::set_text(cx, id, &buf, false);
                        }
                    }
                    let ph = t.get(n!("placeholder"));
                    cx.w_mut::<EditControl>(id).placeholder =
                        (!ph.is_nil()).then(|| tostring_val(&ph));
                }
                VarClass::CheckBox => {
                    cx.w_mut::<CheckBoxControl>(id).state = t.get(n!("state")).truthy();
                }
                VarClass::DropDown => {
                    if let Some(i) = t.get(n!("selIndex")).opt_num() {
                        cx.w_mut::<DropDownControl>(id).sel_index = i as usize;
                    }
                }
                VarClass::Control => {}
            }
            let enabled = t.get(n!("enabled"));
            if !enabled.is_nil() {
                let e = enabled.truthy();
                let b = cx.base_mut(id);
                if !matches!(b.enabled, Prop::Val(v) if v == e) {
                    b.enabled = Prop::Val(e);
                }
            }
        }
    }
}

/// What [`ConfigTab::push_var_control`] copies.
#[derive(Clone, Debug)]
pub enum VarSource {
    /// a CheckBoxControl's `state`
    Check(bool),
    /// an EditControl's `buf`
    Buf(String),
    /// a DropDownControl's `selIndex`
    SelIndex(usize),
}

/// lua: ConfigTab.lua:383-391 (the `ifNode` shown function; `ifNode` is never set)
fn node_if_option(if_node: NodeIfOpt) -> ShownFn {
    let f = |cx: &Cx, id: Option<f64>| -> bool {
        let Some(id) = id else { return false };
        let spec = cx.build().model.spec.get();
        // L384 (node ids are integers)
        if spec.allocNodes.with(|m| m.contains_key(id as i64)) {
            // L385
            return true;
        }
        // L387-390
        let node = spec.nodes.with(|m| m.get(id as i64));
        if let Some(node) = node.filter(|n| n.r#type.get().as_str() == "Keystone") {
            return main_env(cx)
                .keystonesAdded
                .get()
                .truthy(Name::from(&node.dn.get()));
        }
        false
    };
    Rc::new(move |cx| {
        if let NodeIfOpt::List(list) = &if_node {
            for o in list {
                if f(cx, Some(*o)) {
                    return true;
                }
            }
            return f(cx, None);
        }
        let NodeIfOpt::One(o) = &if_node else {
            unreachable!()
        };
        f(cx, Some(*o))
    })
}

/// lua: ConfigTab.lua:392-394 (the `ifNode` tooltip; `ifNode` is never set)
fn node_if_tooltip(if_node: NodeIfOpt) -> TipFn {
    let f = |cx: &Cx, id: f64| -> Option<String> {
        let spec = cx.build().model.spec.get();
        let node = spec
            .nodes
            .with(|m| m.get(id as i64))
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
        // L393
        Some(format!("This option is specific to '{}'.", node.dn.get()))
    };
    Rc::new(move |cx| match &if_node {
        NodeIfOpt::List(list) => {
            let mut out: Option<String> = None;
            for o in list {
                if let Some(cur) = f(cx, *o) {
                    out = Some(match out {
                        Some(p) => p + "\n" + &cur,
                        None => cur,
                    });
                }
            }
            out
        }
        NodeIfOpt::One(o) => f(cx, *o),
    })
}
