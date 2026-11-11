//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ConfigTab.lua:835-1362 (UpdateControls,
//! Draw, sets, undo; the model halves of 839-1204 are calc:BM-B/CO-C). Owner: U42.
//!
//! @pob-file Classes/ConfigTab.lua @16de4b82d
//!
//! # What is where
//!
//! ConfigTab.lua 835-1362, line by line:
//! * 835-837 `IsSectionCollapsed`, 1302-1334 `UpdateCustomModsControls`: U41's ctor.rs
//!   (CCR-0110 §2; called from here).
//! * 839-1006 `Load`, `GetDefaultState`, `Save`; 1118-1204 `UpdateLevel`, `BuildModList`;
//!   1205-1240 `ImportCalcSettings`; 1280-1300 `NewConfigSet`: model (calc:BM-B/BM-A/CO-C,
//!   `pob-calc/src/model/config_tab.rs`). Their UI statements: `Load`'s `self:ResetUndo()`
//!   (L938) is [`ctor_hook`] (DIVERGENCE(reset-undo)); `ImportCalcSettings`' `self:UpdateControls()`
//!   (L1239) runs inside `Init_model` before the controls exist, and U41's constructor pulls the
//!   control tables into the controls at its end (CCR-0110 §3).
//! * 1008-1022 `UpdateControls`: the model half is calc:BM-B's (over the control-state tables);
//!   [`ConfigTab::update_controls`] adds the bridge pull and the UI half of L1021.
//! * 1024-1116 `Draw`: [`ConfigTab::draw`].
//! * 1242-1268 `CreateUndoState`/`RestoreUndoState`: [`UNDO`] (ui doc §10.3).
//! * 1270-1278 `OpenConfigSetManagePopup`: [`ConfigTab::open_config_set_manage_popup`].
//! * 1337-1362 `SetActiveConfigSet`: the model half is calc:BM-B's;
//!   [`ConfigTab::set_active_config_set`] is the whole function on top of it (BuildHooks'
//!   `config_set_active_config_set`).
//!
//! The calc scope (docs/port/scope.toml) holds ConfigTab.lua 839-1240, 1280-1300 and 1337-1362
//! with the model's `@pob`/`@pob-skip` markers (1024-1116 is its `@pob-skip` "Draw: UI"), so the
//! UI ports of lines in those ranges carry `/// lua:` notes but no second `@pob` marker
//! (`provenance.py coverage` would count them twice; CCR-0110 §5 does the same for
//! ConfigOptions.lua; CCR-0121 §1). 1242-1278 are outside the calc scope and carry `@pob` markers here.
//!
//! # Seams
//!
//! U41's `ConfigHooks` entries (CCR-0110 §1) are these ports, one line each in U41's
//! `default_config_hooks`: `update_controls`, `add_undo_state`, `open_config_set_manage_popup`
//! (here), `open_add_mod_popup` (mod_browser.rs), `is_show_all_excluded` (visibility.rs).
//! BuildMode reaches the tab through [`ctor_hook`], [`set_active_config_set_hook`] (U19's
//! `BuildHooks`) and [`draw_tab`], [`mod_flag`], [`set_mod_flag`] (U20's `ShellHooks`).
//!
//! # Lua semantics
//!
//! * `self.modFlag` is the UndoHandler `mod_flag` of [`State::undo`] (ui doc §6.12):
//!   `AddUndoState` sets it, ConfigSetListControl writes it directly, ResetModFlags clears it.
//! * `self.build.buildFlag = true` is `model.buildFlag`.
//! * `copyTable(configSet.customModsList)` with a nil list is the Lua `pairs(nil)` error (Load's
//!   migration always leaves a list).
//! * `colY` (Draw) is a Lua table indexed by column numbers: an `i64` map (`colY[c] or 0`).
//!
//! DIVERGENCE(reset-undo) (CCR-0121 §2): `ConfigTab:Load` ends with `self:ResetUndo()` (L938, UI). calc:BM-B's
//! Load runs inside `Init_model`, before the UI constructors, so [`ctor_hook`] runs
//! `ResetUndo` right after the constructor when the build file has a `Config` section (as U39
//! does for CalcsTab). Between PoB's Load and that point only `Build:Init`'s later statements
//! run: none of them writes the active set's `input`/`customModsList` except
//! `ImportCalcSettings`, which runs only when `input` is empty (Build.lua:685), i.e. never after a
//! loaded `Config` section with defaults.
//! DIVERGENCE(bridge): `Draw` first pulls the model's control tables into the controls
//! (CCR-0110 §3: a BuildModList elsewhere, e.g. SyncLoadouts, changes the tables without the
//! controls); `RestoreUndoState` and `SetActiveConfigSet` pull again after their BuildModList
//! (in Lua the applies write the real controls directly).
//!
//! ORDER: `pairs(state.input)` (RestoreUndoState L1254/L1262): the copy's insertion order only
//! decides `pairs(configSet.input)` in Save, whose `Input` children T4 compares as a multiset
//! (c). `pairs(self.varControls)` (UpdateControls, model): each control independent (c).
//! `pairs(section.varControlList)` (Draw L1066): PoB's lists are arrays built by `t_insert`, so
//! `pairs` walks them in index order there too (c).
//!
//! Props with side effects (ui doc §7.3): none (Draw assigns `varControl.y`/`section.x`/`y`
//! as plain values, as the Lua does).
#![allow(non_snake_case)]

use std::collections::HashMap;

use pob_lua::math::{m_floor, m_max};
use pob_lua::n;
use pob_ui_core::controls::{ButtonControl, DropDownControl, DropItem, ScrollBarControl};
use pob_ui_core::popups::{self, PopupOpts};
use pob_ui_core::prelude::*;
use pob_ui_core::undo::{UndoOps, UndoStack};

use super::ctor;
use super::set_list::ConfigSetListControl;
use super::ConfigTab;
use crate::app::CxApp;
use crate::model::{self, Table};

/// U42's part of the split `ConfigTab` (ui doc §12.1).
#[derive(Default)]
pub struct State {
    /// `self.undo`, `self.redo`, `self.modFlag` (UndoHandler, L134)
    pub undo: UndoStack<Table>,
}

fn undo_stack(cx: &mut Cx, me: CtrlId) -> &mut UndoStack<Table> {
    &mut cx.w_mut::<ConfigTab>(me).core.undo
}

fn model_ref(cx: &Cx) -> model::BuildRef {
    cx.build().model.clone()
}

/// `self.configSets[self.activeConfigSetId]`
fn active_config_set(cx: &Cx) -> Table {
    cx.build().model.configTab.activeConfigSet()
}

/// lua: ConfigTab.lua:1242-1248 `ConfigTab:CreateUndoState()`
/// @pob Classes/ConfigTab.lua:1242-1248 h=2bd11e6f
fn create_undo_state(cx: &mut Cx, _me: CtrlId) -> Table {
    // L1243
    let configSet = active_config_set(cx);
    // L1244-1247
    let state = Table::new();
    let input = configSet
        .table(n!("input"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
    state.set(n!("input"), input.copyTable(false));
    let customModsList = configSet
        .table(n!("customModsList"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
    state.set(n!("customModsList"), customModsList.copyTable(false));
    state
}

/// lua: ConfigTab.lua:1250-1268 `ConfigTab:RestoreUndoState(state)`
/// @pob Classes/ConfigTab.lua:1250-1268 h=e87f6ea7
fn restore_undo_state(cx: &mut Cx, me: CtrlId, state: Table) {
    // L1251
    let configSet = active_config_set(cx);
    let input = configSet
        .table(n!("input"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'wipeTable' (table expected, got nil)"));
    // L1252
    if let Some(stateInput) = state.table(n!("input")) {
        // L1253
        input.wipe();
        // L1254 ORDER: (c), module notes
        for (k, v) in stateInput.pairs() {
            // L1255
            input.set(k, v);
        }
        // L1257
        if let Some(list) = state.table(n!("customModsList")) {
            // L1258
            configSet.set(n!("customModsList"), list.copyTable(false));
        }
    } else {
        // L1261
        input.wipe();
        // L1262 ORDER: (c), module notes
        for (k, v) in state.pairs() {
            // L1263
            input.set(k, v);
        }
    }
    // L1266
    ConfigTab::update_controls(cx, me);
    // L1267
    let m = model_ref(cx);
    m.configTab.BuildModList(&m);
    // DIVERGENCE(bridge) (module notes)
    ConfigTab::pull_var_controls(cx, me);
}

/// The UndoHandler class of the tab (ui doc §10.3).
pub const UNDO: UndoOps<Table> = UndoOps {
    stack: undo_stack,
    create: create_undo_state,
    restore: restore_undo_state,
};

impl ConfigTab {
    /// `self:AddUndoState()` (UndoHandler.lua:34-41 with CreateUndoState)
    pub fn add_undo_state(cx: &mut Cx, me: CtrlId) {
        UNDO.add_undo_state(cx, me, false);
    }

    /// `self.modFlag`
    pub fn mod_flag_of(cx: &Cx, me: CtrlId) -> bool {
        cx.w::<ConfigTab>(me).core.undo.mod_flag
    }

    /// `self.modFlag = v`
    pub fn set_mod_flag_of(cx: &mut Cx, me: CtrlId, v: bool) {
        cx.w_mut::<ConfigTab>(me).core.undo.mod_flag = v;
    }

    /// lua: ConfigTab.lua:1008-1022 `ConfigTab:UpdateControls()`: the model half over the
    /// control-state tables (calc:BM-B, L1009-1020 and the model half of L1021), the bridge
    /// pull into the controls (CCR-0110 §3), and the UI half of `UpdateCustomModsControls`
    /// (L1021, U41's port).
    pub fn update_controls(cx: &mut Cx, me: CtrlId) {
        let m = model_ref(cx);
        // L1009-1020 (model, then the controls)
        m.configTab.UpdateControls(&m);
        Self::pull_var_controls(cx, me);
        // L1021
        Self::update_custom_mods_controls(cx, me);
    }

    /// lua: ConfigTab.lua:1024-1116 `ConfigTab:Draw(viewPort, inputEvents)` (the calc scope's
    /// `@pob-skip` covers these lines; module notes)
    #[allow(clippy::too_many_lines)]
    pub fn draw(cx: &mut Cx, me: CtrlId, vp: &ViewPort, ev: &mut InputEvents) {
        // DIVERGENCE(bridge): the controls take the model's control-table state first
        Self::pull_var_controls(cx, me);
        {
            let b = cx.base_mut(me);
            // L1025
            b.x = Prop::Val(vp.x);
            // L1026
            b.y = Prop::Val(vp.y);
            // L1027
            b.width = Prop::Val(vp.width);
            // L1028
            b.height = Prop::Val(vp.height);
        }

        // L1030
        let mut i = 0;
        while let Some(event) = ev.next_ipairs(&mut i) {
            // L1031
            if let EventKind::KeyDown { .. } = event.kind {
                // L1032
                if event.key == Key::Char(b'z') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1033
                    UNDO.undo(cx, me);
                    // L1034
                    cx.build().model.buildFlag.set(true);
                // L1035
                } else if event.key == Key::Char(b'y') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1036
                    UNDO.redo(cx, me);
                    // L1037
                    cx.build().model.buildFlag.set(true);
                // L1038
                } else if event.key == Key::Char(b'f') && cx.sys.is_key_down(Key::Ctrl) {
                    // L1039
                    let search = cx.child(me, "search").unwrap_or_else(|| {
                        lua_error!("attempt to index field 'search' (a nil value)")
                    });
                    cx.select_control(me, Some(search));
                }
            }
        }

        // L1044
        cx.process_controls_input(me, ev, vp);
        let scroll_bar = cx.child_as::<ScrollBarControl>(me, "scrollBar").id();
        // L1045
        let mut i = 0;
        while let Some(event) = ev.next_ipairs(&mut i) {
            // L1046
            if event.kind == EventKind::KeyUp {
                // L1047
                if ScrollBarControl::is_scroll_down_key(cx, scroll_bar, event.key) {
                    // L1048
                    ScrollBarControl::scroll(cx, scroll_bar, 1.0);
                // L1049
                } else if ScrollBarControl::is_scroll_up_key(cx, scroll_bar, event.key) {
                    // L1050
                    ScrollBarControl::scroll(cx, scroll_bar, -1.0);
                }
            }
        }

        // L1055
        let maxCol = m_floor((vp.width - 10.0) / 370.0);
        // L1056
        let mut maxColY: f64 = 0.0;
        // L1057
        let mut colY: HashMap<i64, f64> = HashMap::from([(1, 0.0)]);
        // L1058
        let sectionList = cx.w::<ConfigTab>(me).ctor.section_list.clone();
        for section in sectionList {
            // L1059
            let mut y = 14.0;
            // L1060
            cx.base_mut(section).shown = Prop::Val(true);
            // Probe with the section expanded, so a collapsed section that still has
            // eligible options keeps its (clickable) header on screen
            // L1063
            let collapsed = Self::section_data(cx, me, section).collapsed;
            // L1064
            Self::section_data_mut(cx, me, section).collapsed = false;
            // L1065
            let mut doShow = false;
            // L1066 ORDER: (c), module notes
            let list = Self::section_data(cx, me, section).var_control_list.clone();
            for varControl in list {
                // L1067
                if cx.is_shown(varControl) {
                    // L1068
                    doShow = true;
                    // L1069
                    let (_width, height) = cx.size(varControl);
                    // L1070
                    let height = m_max(height, 16.0);
                    // L1071
                    cx.base_mut(varControl).y = Prop::Val(y + 2.0);
                    // L1072
                    y = y + height + 4.0;
                }
            }
            // L1075
            Self::section_data_mut(cx, me, section).collapsed = collapsed;
            // L1076
            cx.base_mut(section).shown = Prop::Val(doShow);
            // L1077
            if doShow {
                // L1078
                let (_width, height) = cx.size(section);
                // L1079
                let col: i64;
                let sectionCol = Self::section_data(cx, me, section).col;
                // L1080
                if let Some(c) = sectionCol.filter(|&c| {
                    colY.get(&(c as i64)).copied().unwrap_or(0.0) + height + 28.0 <= vp.height
                        && 10.0 + c * 370.0 <= vp.width
                }) {
                    // L1081
                    col = c as i64;
                } else {
                    // L1083
                    let mut c0: i64 = 1;
                    // L1084
                    let mut c: i64 = 2;
                    while (c as f64) <= maxCol {
                        // L1085
                        let v = *colY.entry(c).or_insert(0.0);
                        // L1086
                        if v < colY[&c0] {
                            // L1087
                            c0 = c;
                        }
                        c += 1;
                    }
                    col = c0;
                }
                // L1091
                colY.entry(col).or_insert(0.0);
                // L1092
                cx.base_mut(section).x = Prop::Val(10.0 + (col as f64 - 1.0) * 370.0);
                // L1093
                cx.base_mut(section).y = Prop::Val(colY[&col] + 18.0);
                // L1094
                let v = colY[&col] + height + 18.0;
                colY.insert(col, v);
                // L1095
                maxColY = m_max(maxColY, v);
            }
        }

        // L1099
        let mut newSetList: Vec<DropItem> = Vec::new();
        let setSelect = cx.child_as::<DropDownControl>(me, "setSelect");
        let m = model_ref(cx);
        let order = m.configTab.configSetOrderList.with(Clone::clone);
        // L1100
        for (index, configSetId) in order.iter().enumerate() {
            // L1101
            let configSet = m
                .configTab
                .configSet(f64::from(*configSetId))
                .unwrap_or_else(|| lua_error!("attempt to index local 'configSet' (a nil value)"));
            // L1102
            let title = configSet
                .str(n!("title"))
                .map_or_else(|| "Default".to_owned(), |s| s.to_string());
            newSetList.push(DropItem::str(title));
            // L1103
            if f64::from(*configSetId) == m.configTab.activeConfigSetId.get() {
                // L1104
                cx.get_mut(setSelect).sel_index = index + 1;
            }
        }
        // L1107
        DropDownControl::set_list(cx, setSelect.id(), newSetList);

        // L1109
        cx.base_mut(scroll_bar).height = Prop::Val(vp.height);
        // L1110
        ScrollBarControl::set_content_dimension(cx, scroll_bar, maxColY + 58.0, vp.height);
        // L1111
        let offset = cx.w::<ScrollBarControl>(scroll_bar).offset;
        let anchor = cx
            .child(me, "sectionAnchor")
            .unwrap_or_else(|| lua_error!("attempt to index field 'sectionAnchor' (a nil value)"));
        cx.base_mut(anchor).y = Prop::Val(20.0 - offset);

        // L1113
        crate::main_mode::background::draw_background(cx, vp);

        // L1115
        cx.draw_controls(me, vp, None);
    }

    fn section_data(cx: &Cx, me: CtrlId, section: CtrlId) -> &ctor::SectionData {
        cx.w::<ConfigTab>(me)
            .ctor
            .sections
            .get(&section)
            .unwrap_or_else(|| lua_error!("ConfigTab: a section without its fields"))
    }

    fn section_data_mut(cx: &mut Cx, me: CtrlId, section: CtrlId) -> &mut ctor::SectionData {
        cx.w_mut::<ConfigTab>(me)
            .ctor
            .sections
            .get_mut(&section)
            .unwrap_or_else(|| lua_error!("ConfigTab: a section without its fields"))
    }

    /// lua: ConfigTab.lua:1270-1277 `ConfigTab:OpenConfigSetManagePopup()`
    /// @pob Classes/ConfigTab.lua:1270-1277 h=469b741b
    pub fn open_config_set_manage_popup(cx: &mut Cx, me: CtrlId) {
        // L1271
        popups::open_popup(
            cx,
            370.0,
            290.0,
            "Manage Config Sets",
            move |cx, p| {
                // L1272
                let order = cx.build().model.configTab.configSetOrderList.clone();
                cx.add(
                    p,
                    CtrlKey::Idx(1),
                    ConfigSetListControl::new(Anchor::NONE, rect(0, 50, 350, 200), me, order),
                );
                // L1273-1275
                cx.add(
                    p,
                    CtrlKey::Idx(2),
                    ButtonControl::new(Anchor::NONE, rect(0, 260, 90, 20), "Done", |cx| {
                        // L1274
                        popups::close_popup(cx);
                    }),
                );
            },
            PopupOpts::default(),
        );
    }

    /// lua: ConfigTab.lua:1337-1362 `ConfigTab:SetActiveConfigSet(configSetId)` (not `init`:
    /// the constructor's call is the model's). The model half (calc:BM-B, L1339-1354 and
    /// L1360) runs with `init = true`, then L1357-1358 and L1361 run here: UpdateControls
    /// with its UI half, BuildModList (and the bridge pull, DIVERGENCE(bridge)), SyncLoadouts.
    /// `me` is `None` before the tab's constructor has run (the model halves only).
    pub fn set_active_config_set(cx: &mut Cx, me: Option<CtrlId>, configSetId: Option<f64>) {
        let m = model_ref(cx);
        // L1339-1354, L1360 (model; `init`: L1356-1359 below)
        m.configTab.SetActiveConfigSet(&m, configSetId, true);
        // L1356 (not init)
        if let Some(me) = me {
            // L1357
            Self::update_controls(cx, me);
            // L1358
            m.configTab.BuildModList(&m);
            Self::pull_var_controls(cx, me);
        } else {
            // L1357-1358 (no controls yet)
            m.configTab.UpdateControls(&m);
            m.configTab.BuildModList(&m);
        }
        // L1360
        m.buildFlag.set(true);
        // L1361
        let f = crate::build::init::hooks(cx).sync_loadouts;
        f(cx);
    }
}

// ---- seams (CCR-0110 §1; CCR-0098 §3, CCR-0104 §1) -------------------------------------------

/// `self.configTab` of the open build, when the tab exists.
pub fn tab(cx: &Cx) -> Option<CtrlId> {
    let id = cx.app().build.as_ref()?.tabs.config.id();
    cx.ui.try_w::<ConfigTab>(id).map(|_| id)
}

fn tab_or_err(cx: &Cx) -> CtrlId {
    tab(cx).unwrap_or_else(|| lua_error!("attempt to index field 'configTab' (a nil value)"))
}

/// `BuildHooks::config_tab`: `self.configTab = new("ConfigTab"):ConfigTab(self)` (Build.lua:611,
/// U41's constructor), then the `ResetUndo` of the Load that already ran
/// (DIVERGENCE(reset-undo)).
pub fn ctor_hook(cx: &mut Cx, build: CtrlId) -> CtrlId {
    let me = ctor::ctor_hook(cx, build);
    let m = model_ref(cx);
    if model::build_xml::xmlSectionList(&m)
        .iter()
        .any(|n| &*n.elem == "Config")
    {
        // L938 self:ResetUndo()
        UNDO.reset_undo(cx, me);
    }
    me
}

/// `BuildHooks::config_set_active_config_set`: `self.configTab:SetActiveConfigSet(id)`.
pub fn set_active_config_set_hook(cx: &mut Cx, id: f64) {
    let me = tab(cx);
    ConfigTab::set_active_config_set(cx, me, Some(id));
}

/// `ShellHooks::draw_tab` for `BuildTab::Config`: `self.configTab:Draw(tabViewPort,
/// inputEvents)` (Build.lua:1307).
pub fn draw_tab(cx: &mut Cx, vp: &ViewPort, ev: &mut InputEvents) {
    let me = tab_or_err(cx);
    ConfigTab::draw(cx, me, vp, ev);
}

/// `ShellHooks::tab_flag` for `TabFlag::Config`: `self.configTab.modFlag`.
pub fn mod_flag(cx: &Cx) -> bool {
    tab(cx).is_some_and(|me| ConfigTab::mod_flag_of(cx, me))
}

/// `ShellHooks::set_tab_flag` for `TabFlag::Config`: `self.configTab.modFlag = v`
/// (ResetModFlags, Build.lua:1104).
pub fn set_mod_flag(cx: &mut Cx, v: bool) {
    if let Some(me) = tab(cx) {
        ConfigTab::set_mod_flag_of(cx, me, v);
    }
}
