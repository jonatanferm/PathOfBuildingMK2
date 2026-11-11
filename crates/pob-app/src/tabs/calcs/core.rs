//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CalcsTab.lua:1-162, 226-440 (ctor, Draw, NewSection, display stat, CheckFlag, SearchMatch).
//! Owner: U39.
//!
//! @pob-file Classes/CalcsTab.lua @16de4b82d
//!
//! The Calcs tab (`newClass("CalcsTab", "UndoHandler", "ControlHost", "Control")`): a free node
//! owned by the build node (`build.calcsTab`, `BuildState::tabs.calcs`), like the Notes tab
//! (U22, the template). The model half is calc:BM-A's and calc:PWR's (`model.calcsTab`:
//! `input`, `mainEnv`, `calcsEnv`, ..., `ctor_model`/`sectionList`, Load/Save, BuildOutput,
//! the power builder, Create/RestoreUndoState; ui doc §10.2); this file is the UI half. The
//! BuildMode seams at the bottom ([`ctor_hook`], [`draw_tab`], [`mod_flag`], [`set_mod_flag`],
//! [`calcs_breakdown`]) are one match arm each in `init::default_hooks` /
//! `on_frame::default_shell_hooks` (CCR-0106 §2).
//!
//! Split with the model half (markers): calc:BM-A covers CalcsTab.lua 24-40 (the `input`
//! defaults, `sectionList`), 45 (the first line of the SkillSelect `NewSection`), 151-154 (the
//! CalcSections loop) and 370-375 (`NewSection`), and skips 41-42 for this unit. The UI
//! statements of those lines are here, commented with their line numbers and no marker; the
//! rest of 1-162 and 226-440 carries this unit's markers.
//!
//! `CalcBreakdownControl` (`self.controls.breakdown`, L156) is U40's. Until it lands,
//! [`CalcsHooks`] constructs a stand-in ([`BreakdownStandIn`]: never shown, no drawing, keeps
//! `sourceData`/`pinned` as `SetBreakdownData` does, CalcBreakdownControl.lua:60-68); U40
//! replaces the three hook entries (one line each in [`default_calcs_hooks`]).
//!
//! Lua semantics:
//! * `self.displayPinned` is nil, false or true; only its truthiness is read (bool here).
//! * `t_insert(self.controls, x)` puts the search box at index 1 (it is also
//!   `controls.search`, so `pairs` visits it twice and DrawControls draws it twice, as in PoB)
//!   and the sections at 2, 3, ...
//! * `CheckFlag` returns true or nil (`bool`).
//! * `SearchMatch` uses `find` with a pattern (not plain): a malformed search pattern raises the
//!   Lua error PoB raises.
//! * The SkillSelect section's controls are created by the table constructor before the section
//!   (`add_free`, owned by the tab), then stored in the section's `controls` by its constructor
//!   (CalcSectionControl.lua:42), so their creation order is the Lua's.
//!
//! ORDER: no `pairs` loops (`RestoreUndoState`'s is calc:PWR's).
//!
//! DIVERGENCE(reset-undo): `CalcsTab:Load` ends with `self:ResetUndo()` (L200, UI). calc:BM-A's
//! Load runs inside `Init_model`, before the UI constructors, so [`ctor_hook`] calls
//! `ResetUndo` right after constructing the tab when the build file has a `Calcs` section (the
//! state is the same `copyTable(self.input)`: nothing writes `calcsTab.input` between the load
//! and the constructor).
//!
//! DIVERGENCE(build-output-tail): CalcsTab.lua:464-467 (the UI tail of BuildOutput:
//! `SetBreakdownData()` then `SetBreakdownData(self.displayData, self.displayPinned)` when a stat
//! is displayed) is [`build_output_ui_tail`]; the calc facade's `build_output` (U15) does not
//! call it and U20's recalc sites (Build.lua:695, :1266) are not wired to it yet (CCR-0111 §1).
//! With the breakdown stand-in it is unobservable (see the CCR).
//!
//! Props with side effects (ui doc §7.3): none.

#![allow(non_snake_case, clippy::option_if_let_else, clippy::single_match_else)]

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::math::{m_floor, m_max};
use pob_lua::{n, tonumber};
use pob_ui_core::controls::drop_down::{DropTipArgs, SelFunc};
use pob_ui_core::controls::scroll_bar::ScrollDir;
use pob_ui_core::controls::{
    ButtonControl, CheckBoxControl, DropDownControl, DropItem, EditArgs, EditControl,
    ScrollBarControl,
};
use pob_ui_core::input::InputEvents;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{Tooltip, TtKey, TtObj};
use pob_ui_core::undo::{UndoOps, UndoStack};

use super::section::{CalcSectionControl, CalcSectionRef, CellControl, UpdateFunc};
use super::{sections_data, CalcsTab};
use crate::app::CxApp;
use crate::build::init::{self, MinionListValue};
use crate::build::on_frame::{self, CalcsBreakdown};
use crate::main_mode::background::draw_background;
use crate::model::{self, build_xml, EnvRef, Key as TKey, Name, Table, Val};
use pob_calc::actor::ActorRef;
// CCR-0103 §4: model.rs (U15) does not re-export the calc modules; the undo states are calc:PWR's
use pob_calc::modules::power;

// @pob Classes/CalcsTab.lua:1-11 h=ad472bf7
// (`local pairs, ipairs, t_insert, m_max, m_floor`: Rust iteration and `pob_lua::math`)

/// `buffModeDropList` (CalcsTab.lua:12-17): `{ label = .., buffMode = .. }`.
/// @pob Classes/CalcsTab.lua:12-17 h=cdf8491a
const BUFF_MODE_DROP_LIST: [(&str, &str); 4] = [
    ("Unbuffed", "UNBUFFED"),
    ("Buffed", "BUFFED"),
    ("In Combat", "COMBAT"),
    ("Effective DPS", "EFFECTIVE"),
];

/// A `buffModeDropList` entry's `buffMode` (the dropdown item's value).
pub struct BuffModeValue(pub &'static str);

/// The Calculation Mode tooltip (CalcsTab.lua:132-139, a long string).
const MODE_TOOLTIP: &str = "This controls the calculation of the stats shown in this tab.
The stats in the sidebar are always shown in Effective DPS mode, regardless of this setting.

Unbuffed: No auras, buffs, or other support skills or effects will apply. This is equivalent to standing in town.
Buffed: Aura and buff skills apply. This is equivalent to standing in your hideout with auras and buffs turned on.
In Combat: Charges and combat buffs such as Onslaught will also apply. This will show your character sheet stats in combat.
Effective DPS: Curses and enemy properties (such as resistances and status conditions) will also apply. This estimates your true DPS.";

// @pob Classes/CalcsTab.lua:19-23 h=67b77af4
// (`newClass("CalcsTab", "UndoHandler", "ControlHost", "Control")`: the `Widget` impl below,
// `.host()` in the constructor and [`UNDO`])

/// U39's part of the `CalcsTab` object (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.build` (the build node)
    pub build: CtrlId,
    /// `self.colWidth`
    pub col_width: f64,
    /// `self.sectionList` (the section controls; the model half is `model.calcsTab.sectionList()`)
    pub section_list: Vec<CtrlId>,
    /// `self.displayData` (a cell table, compared by identity)
    pub display_data: Option<Table>,
    /// `self.displayPinned`
    pub display_pinned: bool,
    /// `self.undo`, `self.redo`, `self.modFlag` (UndoHandler)
    pub undo: UndoStack<Table>,
}

/// drawn by BuildMode (`self.calcsTab:Draw(tabViewPort, inputEvents)`, Build.lua:1315)
impl Widget for CalcsTab {
    const NAME: &'static str = "CalcsTab";
    const CAPS: Caps = Caps::empty();
}

fn undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<Table> {
    &mut cx.w_mut::<CalcsTab>(me).core.undo
}

fn create_undo_state(cx: &mut Cx, _me: CtrlId) -> Table {
    let m = cx.build().model.clone();
    power::CreateUndoState(&m)
}

fn restore_undo_state(cx: &mut Cx, _me: CtrlId, state: Table) {
    let m = cx.build().model.clone();
    power::RestoreUndoState(&m, &state);
}

/// The UndoHandler class of the tab (CalcsTab.lua:764-773 are calc:PWR's; ui doc §10.3).
pub const UNDO: UndoOps<Table> = UndoOps {
    stack: undo_stack,
    create: create_undo_state,
    restore: restore_undo_state,
};

// ---- the CalcBreakdownControl seam (U40) ------------------------------------------------------

/// What this unit calls on `self.controls.breakdown` (CalcBreakdownControl, U40).
#[derive(Clone)]
pub struct CalcsHooks {
    /// `new("CalcBreakdownControl"):CalcBreakdownControl(self)` stored as `self.controls.breakdown`
    /// (L156); returns the control
    pub breakdown_ctor: Rc<dyn Fn(&mut Cx, CtrlId) -> CtrlId>,
    /// `breakdown:SetBreakdownData(displayData, pinned)` (`None`: no arguments)
    pub set_breakdown_data: Rc<dyn Fn(&mut Cx, CtrlId, Option<&Table>, bool)>,
    /// `breakdown.sourceData`
    pub breakdown_source_data: Rc<dyn Fn(&Cx, CtrlId) -> Option<Table>>,
}

/// The stand-in for `CalcBreakdownControl` until U40 lands: a control that is never shown,
/// draws nothing, is never under the mouse and ignores keys; `SetBreakdownData` keeps
/// `pinned` and `sourceData` (CalcBreakdownControl.lua:61-68 without the section list).
#[derive(Default)]
pub struct BreakdownStandIn {
    /// `self.sourceData`
    pub source_data: Option<Table>,
    /// `self.pinned`
    pub pinned: bool,
}

impl Widget for BreakdownStandIn {
    const NAME: &'static str = "CalcBreakdownControl";
    const CAPS: Caps = Caps::DRAW
        .union(Caps::IS_MOUSE_OVER)
        .union(Caps::KEY_DOWN)
        .union(Caps::KEY_UP);
    fn draw(_cx: &mut Cx, _me: CtrlId, _vp: &ViewPort, _no_tooltip: bool) {}
    fn is_mouse_over(_cx: &Cx, _me: CtrlId) -> bool {
        false
    }
    fn on_key_down(_cx: &mut Cx, _me: CtrlId, _key: Key, _double_click: bool) -> Focus {
        None
    }
    fn on_key_up(_cx: &mut Cx, _me: CtrlId, _key: Key) -> Focus {
        None
    }
}

/// The defaults (U40 replaces each entry with its CalcBreakdownControl).
pub fn default_calcs_hooks() -> CalcsHooks {
    CalcsHooks {
        // U40 (CCR-0111 §2)
        breakdown_ctor: Rc::new(super::breakdown::calcs_breakdown_ctor),
        set_breakdown_data: Rc::new(super::breakdown::calcs_set_breakdown_data),
        breakdown_source_data: Rc::new(super::breakdown::calcs_breakdown_source_data),
    }
}

thread_local! {
    static CALCS_HOOKS: RefCell<Option<CalcsHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn calcs_hooks() -> CalcsHooks {
    CALCS_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_calcs_hooks)
            .clone()
    })
}

/// Replace hook entries (tests, and owners that register at run time).
pub fn set_calcs_hooks(f: impl FnOnce(&mut CalcsHooks)) {
    CALCS_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_calcs_hooks)));
}

// ---- model access -----------------------------------------------------------------------------

/// `self.calcsEnv` (the model's; nil before the first BuildOutput).
pub fn calcs_env(cx: &Cx) -> Option<EnvRef> {
    cx.build().model.calcsTab.calcsEnv.get()
}

/// `self.input`
fn input(cx: &Cx) -> Table {
    cx.build().model.calcsTab.input.clone()
}

/// `self.input.showMinion and env.minion or env.player` (L397; CalcSectionControl.lua:190, 388).
pub fn input_actor(cx: &Cx, env: &EnvRef) -> ActorRef {
    if input(cx).truthy(n!("showMinion")) {
        if let Some(m) = env.minion.get() {
            return m;
        }
    }
    env.player.clone()
}

/// A table key from a flag value (`skillFlags[obj.flag]`).
#[track_caller]
fn key_of(v: &Val) -> TKey {
    match v {
        Val::Str(s) => TKey::from(Name::from(s)),
        Val::Num(x) => TKey::from(*x),
        Val::Bool(b) => TKey::from(*b),
        other => lua_error!("CheckFlag: a {} flag", other.lua_type()),
    }
}

/// `actor.mainSkill.skillFlags`
#[track_caller]
fn skill_flags(actor: &ActorRef) -> Table {
    actor
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
        .skillFlags
        .get()
}

/// `x .. y` / `x:method()` operand (a string, or a number via `tostring`).
#[track_caller]
fn string_of(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(x) => pob_lua::tostring_num(*x),
        other => lua_error!("attempt to index a {} value", other.lua_type()),
    }
}

/// lua: CalcsTab.lua:396-433 `CalcsTab:CheckFlag(obj, actor, player)`
/// @pob Classes/CalcsTab.lua:396-433 h=2f93f617
pub fn check_flag(
    cx: &Cx,
    obj: &Table,
    actor: Option<&ActorRef>,
    player: Option<&ActorRef>,
) -> bool {
    // L397
    let actor = match actor {
        Some(a) => a.clone(),
        None => {
            let env = calcs_env(cx)
                .unwrap_or_else(|| lua_error!("attempt to index field 'calcsEnv' (a nil value)"));
            input_actor(cx, &env)
        }
    };
    // L398
    let skillFlags = skill_flags(&actor);
    // L399
    let flag = obj.get(n!("flag"));
    if flag.truthy() && !skillFlags.truthy(key_of(&flag)) {
        // L400
        return false;
    }
    // L402
    let flagList = obj.get(n!("flagList"));
    if flagList.truthy() {
        // L403
        for (_, flag) in flagList
            .table()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected)"))
            .ipairs()
        {
            // L404
            if !skillFlags.truthy(key_of(&flag)) {
                // L405
                return false;
            }
        }
    }
    // L409
    let playerFlag = obj.get(n!("playerFlag"));
    if playerFlag.truthy() {
        let player = match player {
            Some(p) => p.clone(),
            None => {
                calcs_env(cx)
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index field 'calcsEnv' (a nil value)")
                    })
                    .player
                    .clone()
            }
        };
        if !skill_flags(&player).truthy(key_of(&playerFlag)) {
            // L410
            return false;
        }
    }
    // L412
    let notFlag = obj.get(n!("notFlag"));
    if notFlag.truthy() && skillFlags.truthy(key_of(&notFlag)) {
        // L413
        return false;
    }
    // L415
    let notFlagList = obj.get(n!("notFlagList"));
    if notFlagList.truthy() {
        // L416
        for (_, flag) in notFlagList
            .table()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'ipairs' (table expected)"))
            .ipairs()
        {
            // L417
            if skillFlags.truthy(key_of(&flag)) {
                // L418
                return false;
            }
        }
    }
    // L422
    let haveOutput = obj.get(n!("haveOutput"));
    if haveOutput.truthy() {
        let haveOutput = string_of(&haveOutput);
        let output = actor.output.get();
        // L423
        let m = pob_lua::lmatch(&haveOutput, "^(%a+)%.(%a+)$", None);
        // L424
        if let Some(m) = m {
            let (ns, var) = (m[0].to_str(), m[1].to_str());
            // L425
            let o = output.get(Name::from(&ns));
            let v = if o.truthy() {
                o.get(Name::from(&var))
            } else {
                Val::Nil
            };
            if !o.truthy() || !v.truthy() || v.raw_eq(&Val::Num(0.0)) {
                // L426
                return false;
            }
        } else {
            // L428
            let v = output.get(Name::from(haveOutput.as_str()));
            if !v.truthy() || v.raw_eq(&Val::Num(0.0)) {
                // L429
                return false;
            }
        }
    }
    // L432
    true
}

impl CalcsTab {
    /// lua: CalcsTab.lua:24-161 `CalcsTab(build)`
    /// @pob Classes/CalcsTab.lua:46-150 h=f254fcf8
    /// @pob Classes/CalcsTab.lua:155-161 h=b4f3bf0a
    pub fn new(build: CtrlId) -> New<CalcsTab> {
        // L26 self:UndoHandler() (`State::undo`); L27 self:ControlHost(); L28 self:Control()
        New::new(
            Base::new(Anchor::NONE, rect(0, 0, 0, 0)),
            CalcsTab {
                core: State {
                    // L30
                    build,
                    // L38
                    col_width: 230.0,
                    // L39
                    section_list: Vec::new(),
                    display_data: None,
                    display_pinned: false,
                    undo: UndoStack::default(),
                },
            },
        )
        .host()
        .init(|cx, me| Self::init(cx, me.id()))
    }

    /// The constructor body after the node exists (L32-L160).
    #[allow(clippy::too_many_lines)]
    fn init(cx: &mut Cx, me: CtrlId) {
        let model = cx.build().model.clone();
        // L32 self.calcs = require("Modules.Calcs") (the calc facade, U15)
        // L34-36 the `input` defaults (model half: calc:BM-A `CalcsTab::ctor_model`, idempotent)
        model.calcsTab.ctor_model();

        // L41 (covered by calc:BM-A's @pob-skip of 41-42, which names this unit)
        let search = cx
            .add(
                me,
                "search",
                EditControl::new(
                    Anchor::new(TopLeft, me, TopLeft),
                    rect(4, 5, 260, 20),
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
        // L42 t_insert(self.controls, self.controls.search)
        cx.put(me, CtrlKey::Idx(1), search);

        // Special section for skill/mode selection
        // L45 self:NewSection(3, "SkillSelect", 1, colorCodes.NORMAL, {{ ... }}, function(section) ... end)
        // L46-59
        let main_socket_group = cx
            .add_free(
                Some(me),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 300, 16),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L48
                        input(cx).set(n!("skill_number"), index as f64);
                        // L49
                        CalcsTab::add_undo_state(cx, me);
                        // L50
                        cx.build().model.buildFlag.set(true);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L51-58 the mixin `{ tooltipFunc = ... }`
        cx.w_mut::<DropDownControl>(main_socket_group).tt.tooltip_func = Some(Rc::new(
            move |cx: &mut Cx, tooltip: &mut Tooltip, (_mode, index, _value): DropTipArgs| {
                // L53
                let socket_group = index.and_then(|i| {
                    cx.build().model.skillsTab.socketGroupList.get().get(i)
                });
                // L54
                if let Some(socket_group) = socket_group {
                    let rev = cx.build().model.outputRevision.get();
                    let obj: Rc<dyn std::any::Any> = socket_group.clone();
                    if tooltip.check_for_update(cx, &[TtKey::Obj(TtObj(obj)), TtKey::Num(rev)]) {
                        // L55
                        let f = init::hooks(cx).skills_add_socket_group_tooltip;
                        f(cx, tooltip, &socket_group);
                    }
                }
            },
        ));
        // L60-66
        let main_skill = cx
            .add_free(
                Some(me),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 300, 16),
                    Vec::new(),
                    Some(Rc::new(|cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L62
                        let mainSocketGroup = calcs_socket_group(cx);
                        // L63
                        mainSocketGroup.mainActiveSkillCalcs.set(index as f64);
                        // L64
                        cx.build().model.buildFlag.set(true);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L67-74
        let main_skill_part = cx
            .add_free(
                Some(me),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 250, 16),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L69-70
                        let srcInstance = calcs_src_instance(cx);
                        // L71
                        srcInstance.skillPartCalcs.set(Some(index as f64));
                        // L72
                        CalcsTab::add_undo_state(cx, me);
                        // L73
                        cx.build().model.buildFlag.set(true);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L75-83
        let main_skill_stage_count = cx
            .add_free(
                Some(me),
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 0, 52, 16),
                    EditArgs {
                        filter: Some("%D".to_owned()),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L77-78
                            let srcInstance = calcs_src_instance(cx);
                            // L79
                            srcInstance.skillStageCountCalcs.set(tonumber(buf));
                            // L80
                            CalcsTab::add_undo_state(cx, me);
                            // L81
                            cx.build().model.buildFlag.set(true);
                        })),
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L84-92
        let main_skill_mine_count = cx
            .add_free(
                Some(me),
                EditControl::new(
                    Anchor::NONE,
                    rect(0, 0, 52, 16),
                    EditArgs {
                        filter: Some("%D".to_owned()),
                        change_func: Some(Rc::new(move |cx: &mut Cx, buf: &str| {
                            // L86-87
                            let srcInstance = calcs_src_instance(cx);
                            // L88
                            srcInstance.skillMineCountCalcs.set(tonumber(buf));
                            // L89
                            CalcsTab::add_undo_state(cx, me);
                            // L90
                            cx.build().model.buildFlag.set(true);
                        })),
                        ..EditArgs::default()
                    },
                ),
            )
            .id();
        // L93-98
        let show_minion = cx
            .add_free(
                Some(me),
                CheckBoxControl::new(
                    Anchor::NONE,
                    rect(0, 0, 18, 0),
                    None,
                    Some(Rc::new(move |cx: &mut Cx, state: bool| {
                        // L95
                        input(cx).set(n!("showMinion"), state);
                        // L96
                        CalcsTab::add_undo_state(cx, me);
                    })),
                    Some(Prop::Val(Some(
                        "Show stats for the minion instead of the player.".to_owned(),
                    ))),
                    false,
                ),
            )
            .id();
        // L99-111
        let main_skill_minion = cx
            .add_free(
                Some(me),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 160, 16),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        // L101-102
                        let srcInstance = calcs_src_instance(cx);
                        let value = value.unwrap_or_else(|| {
                            lua_error!("attempt to index local 'value' (a nil value)")
                        });
                        let v = value.get::<MinionListValue>();
                        // L103
                        if let Some(item_set_id) = v.and_then(|v| v.item_set_id) {
                            // L104
                            srcInstance.skillMinionItemSetCalcs.set(Some(item_set_id));
                        } else {
                            // L106
                            srcInstance
                                .skillMinionCalcs
                                .set(v.and_then(|v| v.minion_id));
                        }
                        // L108
                        CalcsTab::add_undo_state(cx, me);
                        // L109
                        cx.build().model.buildFlag.set(true);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L112-116
        let main_skill_minion_library = cx
            .add_free(
                Some(me),
                ButtonControl::new(
                    Anchor::NONE,
                    rect(0, 0, 100, 16),
                    "Manage Spectres...",
                    |cx| {
                        // L114
                        let f = init::hooks(cx).open_spectre_library;
                        f(cx);
                    },
                ),
            )
            .id();
        // L117-125
        let main_skill_minion_skill = cx
            .add_free(
                Some(me),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 200, 16),
                    Vec::new(),
                    Some(Rc::new(move |cx: &mut Cx, index: usize, _value: Option<&DropItem>| {
                        // L119-120
                        let srcInstance = calcs_src_instance(cx);
                        // L121
                        srcInstance.skillMinionSkillCalcs.set(Some(index as f64));
                        // L122
                        CalcsTab::add_undo_state(cx, me);
                        // L123
                        cx.build().model.buildFlag.set(true);
                    }) as SelFunc),
                    None,
                    false,
                ),
            )
            .id();
        // L126-140
        let mode = cx
            .add_free(
                Some(me),
                DropDownControl::new(
                    Anchor::NONE,
                    rect(0, 0, 100, 16),
                    BUFF_MODE_DROP_LIST
                        .iter()
                        .map(|&(label, buff_mode)| {
                            DropItem::new(label, Rc::new(BuffModeValue(buff_mode)))
                        })
                        .collect(),
                    Some(Rc::new(move |cx: &mut Cx, _index: usize, value: Option<&DropItem>| {
                        // L129
                        let v = value
                            .and_then(|v| v.get::<BuffModeValue>())
                            .map(|v| v.0)
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index local 'value' (a nil value)")
                            });
                        input(cx).set(n!("misc_buffMode"), v);
                        // L130
                        CalcsTab::add_undo_state(cx, me);
                        // L131
                        cx.build().model.buildFlag.set(true);
                    }) as SelFunc),
                    Some(Prop::Val(Some(MODE_TOOLTIP.to_owned()))),
                    false,
                ),
            )
            .id();
        // L45-144 the subsection table
        let rows = Table::new();
        let control_row = |label: &str, flag: Option<(&str, &str)>, name: &str, id: CtrlId| {
            let row = Table::new();
            row.set(n!("label"), label);
            if let Some((k, v)) = flag {
                row.set(Name::from(k), v);
            }
            let col = Table::new();
            col.set(n!("controlName"), name);
            col.set(n!("control"), Val::Obj(Rc::new(CellControl(id))));
            row.push(col);
            row
        };
        // L46
        rows.push(control_row("Socket Group", None, "mainSocketGroup", main_socket_group));
        // L60
        rows.push(control_row("Active Skill", None, "mainSkill", main_skill));
        // L67
        rows.push(control_row(
            "Skill Part",
            Some(("playerFlag", "multiPart")),
            "mainSkillPart",
            main_skill_part,
        ));
        // L75
        rows.push(control_row(
            "Skill Stages",
            Some(("playerFlag", "multiStage")),
            "mainSkillStageCount",
            main_skill_stage_count,
        ));
        // L84
        rows.push(control_row(
            "Active Mines",
            Some(("playerFlag", "mine")),
            "mainSkillMineCount",
            main_skill_mine_count,
        ));
        // L93
        rows.push(control_row(
            "Show Minion Stats",
            Some(("flag", "haveMinion")),
            "showMinion",
            show_minion,
        ));
        // L99
        rows.push(control_row(
            "Minion",
            Some(("flag", "minion")),
            "mainSkillMinion",
            main_skill_minion,
        ));
        // L112
        rows.push(control_row(
            "Spectre Library",
            Some(("flag", "spectre")),
            "mainSkillMinionLibrary",
            main_skill_minion_library,
        ));
        // L117
        rows.push(control_row(
            "Minion Skill",
            Some(("flag", "haveMinion")),
            "mainSkillMinionSkill",
            main_skill_minion_skill,
        ));
        // L126
        rows.push(control_row("Calculation Mode", None, "mode", mode));
        // L141-143
        for (label, flag, format, breakdown) in [
            ("Aura and Buff Skills", "buffs", "{output:BuffList}", Some("SkillBuffs")),
            ("Combat Buffs", "combat", "{output:CombatList}", None),
            ("Curses and Debuffs", "effective", "{output:CurseList}", Some("SkillDebuffs")),
        ] {
            let row = Table::new();
            row.set(n!("label"), label);
            row.set(n!("flag"), flag);
            row.set(n!("textSize"), 12.0);
            let col = Table::new();
            col.set(n!("format"), format);
            if let Some(b) = breakdown {
                let cell = Table::new();
                cell.set(n!("breakdown"), b);
                col.push(cell);
            }
            row.push(col);
            rows.push(row);
        }
        // L45 `{{ defaultCollapsed = false, label = "View Skill Details", data = {...} }}`
        let sub = Table::new();
        sub.set(n!("defaultCollapsed"), false);
        sub.set(n!("label"), "View Skill Details");
        sub.set(n!("data"), rows);
        let subSection = Table::new();
        subSection.push(sub);
        // L144-148
        let update: UpdateFunc = Rc::new(move |cx: &mut Cx, section: CtrlId| {
            let input = input(cx);
            let skill_number = input.get(n!("skill_number")).opt_num().unwrap_or(f64::NAN);
            // L145
            let f = on_frame::shell_hooks().refresh_skill_select_controls;
            f(cx, section, skill_number, "Calcs");
            // L146
            let show = input.truthy(n!("showMinion"));
            let c = cx.child(section, "showMinion").unwrap_or_else(|| {
                lua_error!("attempt to index field 'showMinion' (a nil value)")
            });
            cx.w_mut::<CheckBoxControl>(c).state = show;
            // L147
            let buff_mode = input.get(n!("misc_buffMode"));
            let m = cx
                .child(section, "mode")
                .unwrap_or_else(|| lua_error!("attempt to index field 'mode' (a nil value)"));
            DropDownControl::sel_by_value(cx, m, &|it: &DropItem| {
                it.get::<BuffModeValue>()
                    .is_some_and(|v| buff_mode.raw_eq(&Val::from(v.0)))
            });
        });
        let colour = cx.main.cc.NORMAL.clone();
        Self::new_section(cx, me, 3.0, "SkillSelect", 1.0, &colour, &subSection, Some(update));

        // Add sections from the CalcSections module
        // L151 local sectionData = LoadModule("Modules/CalcSections")
        let sectionData = sections_data::load();
        // L152
        for section in sectionData {
            // L153 self:NewSection(unpack(section))
            Self::new_section(
                cx,
                me,
                section.width,
                &section.id,
                section.group,
                &section.colour,
                &section.sub_section,
                None,
            );
        }

        // L156
        let f = calcs_hooks().breakdown_ctor;
        f(cx, me);

        // L158
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
        // L159 self.powerBuilderInitialized = nil (calc:PWR / U26)
    }

    /// The tab's `self.controls.breakdown`.
    #[track_caller]
    pub fn breakdown(cx: &Cx, me: CtrlId) -> CtrlId {
        cx.child(me, "breakdown")
            .unwrap_or_else(|| lua_error!("attempt to index field 'breakdown' (a nil value)"))
    }

    /// `(self.displayData, self.displayPinned)`
    pub fn display(cx: &Cx, me: CtrlId) -> (Option<Table>, bool) {
        let s = &cx.w::<CalcsTab>(me).core;
        (s.display_data.clone(), s.display_pinned)
    }

    /// `self.modFlag = v`
    pub fn set_mod_flag_of(cx: &mut Cx, me: CtrlId, v: bool) {
        cx.w_mut::<CalcsTab>(me).core.undo.mod_flag = v;
    }

    /// `self:AddUndoState()` (UndoHandler.lua:34-41)
    pub fn add_undo_state(cx: &mut Cx, me: CtrlId) {
        UNDO.add_undo_state(cx, me, false);
    }

    /// lua: CalcsTab.lua:226-368 `CalcsTab:Draw(viewPort, inputEvents)`
    /// @pob Classes/CalcsTab.lua:226-368 h=6006b3c1
    #[allow(clippy::too_many_lines)]
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        {
            let b = cx.base_mut(me);
            // L227-230
            b.x = vp.x.into();
            b.y = vp.y.into();
            b.width = vp.width.into();
            b.height = vp.height.into();
        }
        let colWidth = cx.w::<CalcsTab>(me).core.col_width;
        let sectionList = cx.w::<CalcsTab>(me).core.section_list.clone();
        let sec = |cx: &Cx, s: CtrlId| -> (bool, bool, f64, f64, f64, f64) {
            let w = cx.w::<CalcSectionControl>(s);
            (
                cx.base(s).enabled.raw(),
                w.is_overlay,
                w.group,
                cx.base(s).width.raw(),
                cx.base(s).height.raw(),
                w.width_cols,
            )
        };

        // Arrange the sections
        // L233
        let baseX = vp.x + 4.0;
        // L234
        let baseY = vp.y + 30.0;
        // L235
        let mut maxCol = m_floor(vp.width / (colWidth + 8.0));
        // L236
        if cx.app().main.portrait_mode {
            maxCol = 3.0;
        }
        // L237
        let mut colY = ColY::default();
        // L238
        let mut maxY = 0.0;
        // L239
        for &section in &sectionList {
            // L240
            CalcSectionControl::update_size(cx, section);
            let (enabled, isOverlay, group, width, height, widthCols) = sec(cx, section);
            // L241
            if enabled && !isOverlay {
                // L242
                let mut col: Option<f64> = None;
                // L243
                if group == 1.0 {
                    // Group 1: Offense or 3 wide sections
                    // This group is put into the first 3 columns, with each section placed into the highest available location
                    // L246
                    col = Some(1.0);
                    // L247 if 1 col wide
                    if width == colWidth {
                        // L248
                        let mut minY = colY.or(1.0, baseY);
                        // L249
                        for c in [2.0, 3.0] {
                            // L250
                            if colY.or(c, baseY) < minY {
                                // L251
                                col = Some(c);
                                // L252
                                minY = colY.or(c, baseY);
                            }
                        }
                    } else {
                        // L256
                        for c in [2.0, 3.0] {
                            // L257
                            let cc = col.unwrap_or(1.0);
                            let v = m_max(colY.or(cc, baseY), colY.or(c, baseY));
                            colY.set(cc, v);
                        }
                    }
                // L260
                } else if group == 2.0 {
                    // Group 2: Defense (the first 4 sections)
                    // This group is put entirely into the 4th column
                    // L263
                    if maxCol >= 4.0 {
                        // L264
                        col = Some(4.0);
                    }
                // L266
                } else if group == 3.0 {
                    // Group 3: Defense (the remaining sections)
                    // This group is put into a 5th column if there's room for one, otherwise they are handled separately
                    // L269
                    if maxCol >= 5.0 {
                        // L270
                        col = Some(5.0);
                    }
                }
                // L273
                if let Some(col) = col {
                    // L274
                    let x = baseX + (colWidth + 8.0) * (col - 1.0);
                    cx.base_mut(section).x = Prop::Val(x);
                    // L275
                    let y = colY.or(col, baseY);
                    cx.base_mut(section).y = Prop::Val(y);
                    // L276
                    let mut c = col;
                    while c <= col + widthCols - 1.0 {
                        // L277
                        colY.set(c, y + height + 8.0);
                        c += 1.0;
                    }
                    // L279
                    maxY = m_max(maxY, colY.req(col));
                }
            }
        }
        // L283
        if maxCol < 5.0 {
            // There's no room for a 5th column
            // Each section from group 3 will instead be placed into column 4 if there's room, otherwise they'll be put in columns 1-3
            // L286
            for c in [1.0, 2.0, 3.0] {
                // L287
                let v = m_max(m_max(colY.req(1.0), colY.req(2.0)), colY.req(3.0));
                colY.set(c, v);
            }
            let portrait = cx.app().main.portrait_mode;
            // L289
            for &section in &sectionList {
                let (enabled, isOverlay, group, _, height, _) = sec(cx, section);
                // L290
                if enabled && !isOverlay && ((portrait && group == 2.0) || group == 3.0) {
                    // L291
                    let mut col = 3.0;
                    // L292
                    if colY.req(col) + height + 4.0 >= m_max(vp.y + vp.height, maxY) {
                        // No room in the 4th column, find the highest available location in columns 1-4
                        // L294
                        let mut minY = colY.req(col);
                        // L295
                        for c in [3.0, 2.0, 1.0] {
                            // L296
                            if colY.req(c) < minY {
                                // L297
                                col = c;
                                // L298
                                minY = colY.req(c);
                            }
                        }
                    }
                    // L302
                    cx.base_mut(section).x = Prop::Val(baseX + (colWidth + 8.0) * (col - 1.0));
                    // L303
                    let y = colY.req(col);
                    cx.base_mut(section).y = Prop::Val(y);
                    // L304
                    colY.set(col, y + height + 8.0);
                    // L305
                    maxY = m_max(maxY, colY.req(col));
                }
            }
        }
        let scrollBar = cx
            .child(me, "scrollBar")
            .unwrap_or_else(|| lua_error!("attempt to index field 'scrollBar' (a nil value)"));
        // L309
        cx.base_mut(scrollBar).height = Prop::Val(vp.height);
        // L310
        ScrollBarControl::set_content_dimension(cx, scrollBar, maxY - (baseY - 26.0), vp.height);
        // L311
        for &section in &sectionList {
            // L312
            if !cx.w::<CalcSectionControl>(section).is_overlay {
                // Give sections their actual Y position and let them update
                // L314
                let y = cx.base(section).y.raw() - cx.w::<ScrollBarControl>(scrollBar).offset;
                cx.base_mut(section).y = Prop::Val(y);
                // L315
                CalcSectionControl::update_pos(cx, section);
            }
        }

        let search = cx
            .child(me, "search")
            .unwrap_or_else(|| lua_error!("attempt to index field 'search' (a nil value)"));
        // L319
        let offset = cx.w::<ScrollBarControl>(scrollBar).offset;
        cx.base_mut(search).y = Prop::Val(4.0 - offset);

        // L321
        let mut i = 0;
        while let Some(event) = ev.next_ipairs(&mut i) {
            // L322
            if let EventKind::KeyDown { .. } = event.kind {
                // L323
                if event.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L324
                    UNDO.undo(cx, me);
                    // L325
                    cx.build().model.buildFlag.set(true);
                // L326
                } else if event.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L327
                    UNDO.redo(cx, me);
                    // L328
                    cx.build().model.buildFlag.set(true);
                // L329
                } else if event.key == Key::Char(b'f') && cx.sys.is_key_down(Key::Ctrl) {
                    // L330
                    cx.select_control(me, Some(search));
                }
            }
        }
        // L334
        cx.process_controls_input(me, ev, vp);
        // L335
        let mut i = 0;
        while let Some(event) = ev.next_ipairs(&mut i) {
            // L336
            if let EventKind::KeyUp = event.kind {
                // L337
                if ScrollBarControl::is_scroll_down_key(cx, scrollBar, event.key) {
                    // L338
                    ScrollBarControl::scroll(cx, scrollBar, 1.0);
                // L339
                } else if ScrollBarControl::is_scroll_up_key(cx, scrollBar, event.key) {
                    // L340
                    ScrollBarControl::scroll(cx, scrollBar, -1.0);
                }
            }
        }

        // L345
        draw_background(cx, vp);

        // L347
        if !cx.w::<CalcsTab>(me).core.display_pinned {
            // L348
            cx.w_mut::<CalcsTab>(me).core.display_data = None;
        }

        let hooks = calcs_hooks();
        // L351
        let breakdown = Self::breakdown(cx, me);
        // L352
        let overlayBreakdown = (hooks.breakdown_source_data)(cx, breakdown)
            .and_then(|s| s.obj::<CalcSectionRef>(n!("calcSection")))
            .is_some_and(|s| cx.w::<CalcSectionControl>(s.0).is_overlay);
        // L353
        if overlayBreakdown {
            // L354
            cx.base_mut(breakdown).shown = Prop::Val(false);
        }
        // L356
        let sel = cx.host(me).sel_control;
        cx.draw_controls(me, vp, sel);
        // L357
        if overlayBreakdown {
            // L358
            cx.base_mut(breakdown).shown = Prop::Val(true);
        }

        // L361
        if cx.w::<CalcsTab>(me).core.display_data.is_some() {
            // L362
            if cx.w::<CalcsTab>(me).core.display_pinned && cx.host(me).sel_control.is_none() {
                // L363
                cx.select_control(me, Some(breakdown));
            }
        } else {
            // L366
            (hooks.set_breakdown_data)(cx, breakdown, None, false);
        }
    }

    /// lua: CalcsTab.lua:370-375 `CalcsTab:NewSection(width, ...)` (the range is calc:BM-A's
    /// model half: `sectionList` with the section ids; this is the UI's: the control).
    /// `subSection` is the module's subsection list; the control gets the model's subsection
    /// tables with the module's fields copied in (see `section.rs`).
    pub fn new_section(
        cx: &mut Cx,
        me: CtrlId,
        width: f64,
        id: &str,
        group: f64,
        colour: &str,
        subSection: &Table,
        update_func: Option<UpdateFunc>,
    ) {
        let k = cx.w::<CalcsTab>(me).core.section_list.len() + 1;
        let shared = model_sub_section(cx, k, id, subSection);
        let colWidth = cx.w::<CalcsTab>(me).core.col_width;
        // L371-373 (`#self.controls` is the search box plus the sections so far)
        let section = cx
            .add(
                me,
                CtrlKey::Idx(k as i64 + 1),
                CalcSectionControl::new(
                    me,
                    width * colWidth + 8.0 * (width - 1.0),
                    id,
                    group,
                    colour,
                    shared,
                    update_func,
                    // L372
                    width,
                ),
            )
            .id();
        // L374
        cx.w_mut::<CalcsTab>(me).core.section_list.push(section);
    }

    /// lua: CalcsTab.lua:377-381 `CalcsTab:ClearDisplayStat()`
    /// @pob Classes/CalcsTab.lua:377-381 h=b8919a9f
    pub fn clear_display_stat(cx: &mut Cx, me: CtrlId) {
        {
            let s = &mut cx.w_mut::<CalcsTab>(me).core;
            // L378
            s.display_data = None;
            // L379
            s.display_pinned = false;
        }
        // L380
        let bd = Self::breakdown(cx, me);
        let f = calcs_hooks().set_breakdown_data;
        f(cx, bd, None, false);
    }

    /// lua: CalcsTab.lua:383-394 `CalcsTab:SetDisplayStat(displayData, pin)`
    /// @pob Classes/CalcsTab.lua:383-394 h=e62252b5
    pub fn set_display_stat(cx: &mut Cx, me: CtrlId, displayData: Option<Table>, pin: bool) {
        let (cur, pinned) = Self::display(cx, me);
        // L384
        let Some(displayData) = displayData.filter(|_| pin || !pinned) else {
            // L385
            return;
        };
        // L387
        if pin && pinned && cur.is_some_and(|c| c.ptr_eq(&displayData)) {
            // L388
            Self::clear_display_stat(cx, me);
            // L389
            return;
        }
        {
            let s = &mut cx.w_mut::<CalcsTab>(me).core;
            // L391
            s.display_data = Some(displayData.clone());
            // L392
            s.display_pinned = pin;
        }
        // L393
        let bd = Self::breakdown(cx, me);
        let f = calcs_hooks().set_breakdown_data;
        f(cx, bd, Some(&displayData), pin);
    }

    /// lua: CalcsTab.lua:435-438 `CalcsTab:SearchMatch(txt)`
    /// @pob Classes/CalcsTab.lua:435-438 h=156b886b
    pub fn search_match(cx: &Cx, me: CtrlId, txt: &str) -> bool {
        let search = cx
            .child(me, "search")
            .unwrap_or_else(|| lua_error!("attempt to index field 'search' (a nil value)"));
        // L436
        let searchStr = pob_lua::string::lower(&cx.w::<EditControl>(search).buf);
        // L437
        !searchStr.is_empty()
            && pob_lua::find(&pob_lua::string::lower(txt), &searchStr, None, false).is_some()
    }
}

/// `colY` of Draw: a Lua table indexed by column number (1..5), nil until set.
#[derive(Default)]
struct ColY(Vec<Option<f64>>);

impl ColY {
    fn get(&self, c: f64) -> Option<f64> {
        if c.fract() != 0.0 || c < 1.0 {
            return None;
        }
        self.0.get(c as usize - 1).copied().flatten()
    }
    /// `colY[c] or d`
    fn or(&self, c: f64, d: f64) -> f64 {
        self.get(c).unwrap_or(d)
    }
    /// `colY[c]` in arithmetic or a comparison (nil raises)
    #[track_caller]
    fn req(&self, c: f64) -> f64 {
        self.get(c)
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on field '?' (a nil value)"))
    }
    fn set(&mut self, c: f64, v: f64) {
        if c.fract() != 0.0 || c < 1.0 {
            lua_error!("colY: a non-integer column");
        }
        let i = c as usize - 1;
        if self.0.len() <= i {
            self.0.resize(i + 1, None);
        }
        self.0[i] = Some(v);
    }
}

/// The model's `sectionList[k].subSection` with the module's subsection fields (`label`,
/// `defaultCollapsed`, `data`) copied into its tables (see `section.rs`).
fn model_sub_section(cx: &Cx, k: usize, id: &str, subSection: &Table) -> Table {
    let list = cx.build().model.calcsTab.sectionList();
    let section = list.get(k).table().unwrap_or_else(|| {
        lua_error!("CalcsTab: the model's section list has no section {k} ({id})")
    });
    if section.str(n!("id")).as_deref() != Some(id) {
        lua_error!(
            "CalcsTab: section {k} is {:?} in the model and {id:?} in the UI",
            section.get(n!("id"))
        );
    }
    let subs = section.table(n!("subSection")).unwrap_or_default();
    for (i, src) in subSection.ipairs() {
        let Some(src) = src.table() else {
            lua_error!("attempt to index local 'subSec' (a {} value)", src.lua_type())
        };
        let dst = subs.get(i).table().unwrap_or_else(|| {
            lua_error!("CalcsTab: the model's section {id} has no subsection {i}")
        });
        // ORDER (c): distinct keys copied into another table
        for (key, v) in src.pairs() {
            let is_model = matches!(&key, TKey::S(s) if &**s == "id" || &**s == "collapsed");
            if !is_model {
                dst.set(key, v);
            }
        }
    }
    subs
}

/// `self.build.skillsTab.socketGroupList[self.input.skill_number]` (L62, L69, ...).
#[track_caller]
fn calcs_socket_group(cx: &Cx) -> model::SocketGroupRef {
    let n = input(cx).get(n!("skill_number")).opt_num();
    n.filter(|n| n.fract() == 0.0 && *n >= 1.0)
        .and_then(|n| cx.build().model.skillsTab.socketGroupList.get().get(n as usize))
        .unwrap_or_else(|| lua_error!("attempt to index local 'mainSocketGroup' (a nil value)"))
}

/// `mainSocketGroup.displaySkillListCalcs[mainSocketGroup.mainActiveSkillCalcs].activeEffect.srcInstance`
/// (L69-70, L77-78, L86-87, L101-102, L119-120).
#[track_caller]
fn calcs_src_instance(cx: &Cx) -> model::GemInstanceRef {
    let group = calcs_socket_group(cx);
    let list = group.displaySkillListCalcs.get().unwrap_or_else(|| {
        lua_error!("attempt to index field 'displaySkillListCalcs' (a nil value)")
    });
    let mas = group.mainActiveSkillCalcs.get();
    let skill = if mas.fract() == 0.0 && mas >= 1.0 {
        list.get(mas as usize)
    } else {
        None
    }
    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    skill
        .activeEffect
        .get()
        .srcInstance
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'srcInstance' (a nil value)"))
}

/// lua: CalcsTab.lua:464-467, the UI tail of `CalcsTab:BuildOutput()` (see the module doc,
/// DIVERGENCE(build-output-tail)): refresh the displayed breakdown after a recalc.
pub fn build_output_ui_tail(cx: &mut Cx) {
    let Some(me) = tab(cx) else { return };
    let (data, pinned) = CalcsTab::display(cx, me);
    // L464
    if let Some(data) = data {
        let bd = CalcsTab::breakdown(cx, me);
        let f = calcs_hooks().set_breakdown_data;
        // L465
        f(cx, bd, None, false);
        // L466
        f(cx, bd, Some(&data), pinned);
    }
}

// ---- the BuildMode seams (CCR-0098 §3, CCR-0104 §1, CCR-0106 §2) -----------------------------

/// `self.calcsTab` of the open build, when the tab exists.
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.calcs.id();
    cx.ui.try_w::<CalcsTab>(id).map(|_| id)
}

fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'calcsTab' (a nil value)"))
}

/// `BuildHooks::calcs_tab`: `self.calcsTab = new("CalcsTab"):CalcsTab(self)` (Build.lua:615),
/// then the `ResetUndo` of the Load that already ran (DIVERGENCE(reset-undo)).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = cx.add_free(Some(build), CalcsTab::new(build)).id();
    let model = cx.build().model.clone();
    if build_xml::xmlSectionList(&model)
        .iter()
        .any(|n| &*n.elem == "Calcs")
    {
        // L200 self:ResetUndo()
        UNDO.reset_undo(cx, me);
    }
    me
}

/// `ShellHooks::draw_tab` for `BuildTab::Calcs`: `self.calcsTab:Draw(tabViewPort, inputEvents)`
/// (Build.lua:1315).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    CalcsTab::draw(cx, me, vp, ev);
}

/// `ShellHooks::tab_flag` for `TabFlag::Calcs`: `self.calcsTab.modFlag`.
pub fn mod_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| cx.w::<CalcsTab>(me).core.undo.mod_flag)
}

/// `ShellHooks::set_tab_flag` for `TabFlag::Calcs`: `self.calcsTab.modFlag = v`
/// (ResetModFlags, Build.lua:1110).
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        CalcsTab::set_mod_flag_of(cx, me, v);
    }
}

/// `ShellHooks::calcs_breakdown`: `self.calcsTab.controls.breakdown` and `self.calcsTab.displayPinned
/// and self.calcsTab.displayData and self.calcsTab.displayData.calcSection and
/// self.calcsTab.displayData.calcSection.isOverlay` (Build.lua:1213-1215).
pub fn calcs_breakdown(cx: &Cx) -> CalcsBreakdown {
    let Some(me) = tab(cx) else {
        return CalcsBreakdown::default();
    };
    let (data, pinned) = CalcsTab::display(cx, me);
    let overlay_breakdown = pinned
        && data
            .and_then(|d| d.obj::<CalcSectionRef>(n!("calcSection")))
            .is_some_and(|s| cx.w::<CalcSectionControl>(s.0).is_overlay);
    CalcsBreakdown {
        breakdown: cx.child(me, "breakdown"),
        overlay_breakdown,
    }
}
