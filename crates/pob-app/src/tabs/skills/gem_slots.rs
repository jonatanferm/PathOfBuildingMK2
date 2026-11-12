//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SkillsTab.lua:743-1080
//! (`SkillsTabClass:CreateGemSlot(index)`: the controls of one gem row). Owner: U31.
//!
//! @pob-file Classes/SkillsTab.lua @16de4b82d
//!
//! The rest of SkillsTab.lua is U30's (UI) and calc:BM-B's (model halves). The row's model
//! state is BM-B's: `self.displayGroup`, `defaultGemLevel`, `defaultGemQuality`,
//! `ProcessSocketGroup`, `ProcessGemLevel` and `UpdateSocketGroups` are
//! `cx.build().model.skillsTab`'s; `gemInstance.color`, `matchesSocket`, `errMsg`, `new` and
//! `naturalMaxLevel` live in the instance's `extra` table (CCR-0019, CCR-0040).
//!
//! # Seams (CCR-0119)
//!
//! CreateGemSlot, SkillListControl and SkillSetListControl read and call SkillsTab UI state that
//! U30 ports in `core.rs` (`self.gemSlots`, `self.anchorGemSlots`, `AddUndoState`, the UI tail of
//! `SetDisplayGroup`, `CopySocketGroup`). They go through [`SkillsTabUiHooks`] (thread-local,
//! [`skills_tab_ui_hooks`] / [`set_skills_tab_ui_hooks`], the CCR-0098 §3 / CCR-0107 §1
//! pattern), written while U30 was unmerged; every entry takes the SkillsTab node (`self` of the
//! Lua; `skillsTab` of the list controls) and the defaults are U30's methods, so a test can put a
//! stand-in tab behind the controls (`tests/u31_skills.rs`). The other SkillsTab methods the
//! controls call have seams of their own: `SetActiveSkillSet` and `AddSocketGroupTooltip` are
//! `build::init::BuildHooks` entries, `skillsTab.modFlag` is `on_frame::ShellHooks::set_tab_flag
//! (TabFlag::Skills)` and `build:SyncLoadouts()` is `BuildHooks::sync_loadouts`.
//! `self.controls.groupLabel` is read from the tab node's controls (`cx.child(me,
//! "groupLabel")`), as the Lua reads `self.controls`. U30's `SkillsHooks` defaults construct these
//! controls ([`CreateGemSlot`], `skill_list::skill_list_ctor`,
//! `skill_set_list::skill_set_list_ctor`; CCR-0118 §1).
//!
//! [`GemSlot`] (`self.gemSlots[i]`) is U30's `core::GemSlot`.
//!
//! # Lua semantics
//!
//! - The row's closures capture the Lua `slot` table: here a shared [`Rc<Cell<GemSlot>>`] filled
//!   as each control is created (`slot.delete = …`), so a closure reads the controls of *its*
//!   row, as in Lua, even if `self.gemSlots[index]` were replaced later. `prevSlot` (L771) is
//!   the previous row's value at creation time.
//! - A rebuilt row (CreateGemSlot for an index that already has one; no PoB path does it, since
//!   UpdateGemSlots only creates missing rows): `self.controls[key] = new` drops the old
//!   controls from the tab, and `cx.add` buries them (D8). The next row's delete button is
//!   anchored to this row's old delete button and its `y` reads the old enableGlobal check boxes
//!   (L772-774), which keeps those three alive in Lua: they are detached instead (not drawn, not
//!   hit-tested, freed with the tab), so the rows below keep their positions.
//! - `self.gemSlots[index] = slot` (L745) runs first in Lua; nothing reads `self.gemSlots[index]`
//!   before the function returns (the constructors call no callback), so the port stores the
//!   finished row at the end.
//! - `self.displayGroup.gemList[index] = gemInstance` (L805, L846, L868, L959, L998) with `index >
//!   #gemList + 1` would leave a hole, which the frozen `Shared<Vec<_>>` cannot hold: the port
//!   raises (`Shared::set`'s hole-free contract). The controls of such a row are hidden (L777:
//!   `index <= #gemList + 1`), so no PoB path reaches it.
//! - LUA-QUIRK: L845, L867, L958 and L997 create the instance with `level = self.defaultGemLevel
//!   or 20`, i.e. the default-level *mode string* (`"normalMaximum"`). The branch is reached only
//!   when the control is disabled (`index <= #gemList` is false), and the string level is
//!   overwritten (L852) or the function raises at `gemInstance.gemData.vaalGem` (L964) before
//!   anything reads it, except the `slot.level:SetText(gemInstance.level)` of L869, L960 and
//!   L999, which shows that string. The typed `level` holds NaN (nil, CCR-0019) meanwhile and
//!   the SetText calls write the string ([`default_level_text`]).
//! - L997 creates the instance without `enableGlobal2` (nil): `false` here (the CCR-0019 bool
//!   convention; Save writes "false" where PoB writes "nil").
//! - `SetText(n)` of a number field that may be nil (NaN, CCR-0019) writes `tostring(nil)`.
//! - `grantedEffect.unsupported` (L1015): no granted effect in the data has the key (calc:BM-B's
//!   `grantedEffect_unsupported`), so it is `false`.
//! - `grantedEffect.hasGlobalEffect` (L1015) is a runtime write into `data`: read through
//!   `skill_stat_map::hasGlobalEffect` (STYLE.md "Runtime writes into data").
//! - `calcFunc()` / `GetMiscCalculator` return `Poll::Pending` only on the web build: the
//!   tooltip then shows no stat comparison that frame (as in U29's GemSelectControl).
//!
//! Props with side effects (ui doc §7.3): none (the `shown`/`enabled`/`x`/`label`/`borderFunc`
//! closures only read). ORDER: L902 `pairs(qualityList)` walks a sequence (array part).

#![allow(non_snake_case, clippy::single_match_else)]

use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::task::Poll;

use pob_lua::{n, tonumber, tostring_num};
use pob_sg::Font;
use pob_ui_core::controls::{ButtonControl, CheckBoxControl, EditArgs, EditControl, LabelControl};
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{TtKey, TtObj};
use pob_ui_core::tooltip::Tooltip;

use super::SkillsTab;
use crate::app::CxApp;
use crate::build::stats;
use crate::controls::gem_select::{GemChangeFunc, GemSelectControl};
use crate::controls::gem_tooltip::stat_describer;
use crate::model::gamedata::schema::skills::GrantedEffect;
use crate::model::{self, GemInstance, GemInstanceRef, SocketGroupRef, Val};

/// `self.gemSlots[index]`: U30's declaration (`core::GemSlot`, the nine control ids of a row).
pub use super::core::GemSlot;

/// U31's part of the split `SkillsTab` struct (ui doc §12.1): none. `self.gemSlots` and
/// `self.anchorGemSlots` are U30's fields (module notes: Seams).
#[derive(Default)]
pub struct State {}

// ---- the SkillsTab seams (CCR-0119) -------------------------------------------------------------

/// The SkillsTab (U30) state and methods U31's controls use (module notes). `CtrlId` arguments
/// are the SkillsTab node.
#[derive(Clone)]
pub struct SkillsTabUiHooks {
    /// `self.gemSlots[index]` (nil: `None`)
    pub gem_slot: Rc<dyn Fn(&Cx, CtrlId, usize) -> Option<GemSlot>>,
    /// `self.gemSlots[index] = slot`
    pub set_gem_slot: Rc<dyn Fn(&mut Cx, CtrlId, usize, GemSlot)>,
    /// `self.anchorGemSlots`
    pub anchor_gem_slots: Rc<dyn Fn(&Cx, CtrlId) -> CtrlId>,
    /// `self:AddUndoState()` (the tab's UndoHandler)
    pub add_undo_state: Rc<dyn Fn(&mut Cx, CtrlId)>,
    /// `self:SetDisplayGroup(socketGroup)` (model half and UI tail)
    pub set_display_group: Rc<dyn Fn(&mut Cx, CtrlId, Option<SocketGroupRef>)>,
    /// `self:CopySocketGroup(socketGroup)`
    pub copy_socket_group: Rc<dyn Fn(&mut Cx, CtrlId, &SocketGroupRef)>,
}

/// The SkillsTab's (U30's `core.rs`, CCR-0119 §2).
pub fn default_skills_tab_ui_hooks() -> SkillsTabUiHooks {
    SkillsTabUiHooks {
        gem_slot: Rc::new(|cx, me, i| {
            SkillsTab::has_gem_slot(cx, me, i).then(|| SkillsTab::gem_slot(cx, me, i))
        }),
        set_gem_slot: Rc::new(SkillsTab::set_gem_slot),
        anchor_gem_slots: Rc::new(|cx, me| cx.w::<SkillsTab>(me).core.anchor_gem_slots),
        add_undo_state: Rc::new(SkillsTab::add_undo_state),
        set_display_group: Rc::new(SkillsTab::set_display_group),
        copy_socket_group: Rc::new(SkillsTab::copy_socket_group),
    }
}

thread_local! {
    static SKILLS_TAB_UI_HOOKS: RefCell<Option<SkillsTabUiHooks>> = const { RefCell::new(None) };
}

/// The installed hooks (a clone; entries are `Rc`s).
pub fn skills_tab_ui_hooks() -> SkillsTabUiHooks {
    SKILLS_TAB_UI_HOOKS.with(|h| {
        h.borrow_mut()
            .get_or_insert_with(default_skills_tab_ui_hooks)
            .clone()
    })
}

/// Replace hook entries (U30 at run time, tests).
pub fn set_skills_tab_ui_hooks(f: impl FnOnce(&mut SkillsTabUiHooks)) {
    SKILLS_TAB_UI_HOOKS.with(|h| f(h.borrow_mut().get_or_insert_with(default_skills_tab_ui_hooks)));
}

/// `self:AddUndoState()`
pub(super) fn add_undo_state(cx: &mut Cx, me: CtrlId) {
    let f = skills_tab_ui_hooks().add_undo_state;
    f(cx, me);
}

/// `self:SetDisplayGroup(socketGroup)`
pub(super) fn set_display_group(cx: &mut Cx, me: CtrlId, group: Option<SocketGroupRef>) {
    let f = skills_tab_ui_hooks().set_display_group;
    f(cx, me, group);
}

/// `self.gemSlots[index]` where the Lua indexes it (nil is the index error).
#[track_caller]
pub fn gem_slot(cx: &Cx, me: CtrlId, index: usize) -> GemSlot {
    let f = skills_tab_ui_hooks().gem_slot;
    f(cx, me, index).unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
}

// ---- helpers ------------------------------------------------------------------------------------

/// `self.build.buildFlag = true`
fn set_build_flag(cx: &Cx) {
    cx.build().model.buildFlag.set(true);
}

/// `self.displayGroup` (nil: `None`)
fn display_group(cx: &Cx) -> Option<SocketGroupRef> {
    cx.build().model.skillsTab.displayGroup.get()
}

/// `self.displayGroup` where the Lua indexes it.
#[track_caller]
fn dg(cx: &Cx) -> SocketGroupRef {
    display_group(cx).unwrap_or_else(|| lua_error!("attempt to index field 'displayGroup' (a nil value)"))
}

/// `#self.displayGroup.gemList`
#[track_caller]
fn gem_count(cx: &Cx) -> usize {
    dg(cx).gemList.get().len()
}

/// `self.displayGroup.gemList[index]`
#[track_caller]
fn gem_at(cx: &Cx, index: usize) -> Option<GemInstanceRef> {
    dg(cx).gemList.get().get(index)
}

/// `self.displayGroup and self.displayGroup.gemList[index]`
fn gem_at_opt(cx: &Cx, index: usize) -> Option<GemInstanceRef> {
    display_group(cx).and_then(|g| g.gemList.get().get(index))
}

/// `tostring(n)` of a number field that may be nil (NaN, CCR-0019).
fn tostring_nan(v: f64) -> String {
    if v.is_nan() {
        "nil".to_owned()
    } else {
        tostring_num(v)
    }
}

/// `control:SetText(text)` (no notify)
fn set_text(cx: &mut Cx, id: CtrlId, text: &str) {
    EditControl::set_text(cx, id, text, false);
}

/// `tostring(self.defaultGemLevel or 20)`: the text of a `level` the LUA-QUIRK constructors
/// write (module notes).
fn default_level_text(cx: &Cx) -> String {
    cx.build()
        .model
        .skillsTab
        .defaultGemLevel
        .get()
        .map_or_else(|| "20".to_owned(), |s| s.to_string())
}

/// `self.defaultGemQuality or 0`
fn default_gem_quality(cx: &Cx) -> f64 {
    cx.build().model.skillsTab.defaultGemQuality.get().unwrap_or(0.0)
}

/// `{ nameSpec = "", level = level, quality = quality, enabled = true, enableGlobal1 = true,
/// enableGlobal2 = enableGlobal2, count = 1, new = true }`
fn new_gem_instance(level: f64, quality: f64, enable_global2: bool) -> GemInstanceRef {
    let g = GemInstance::default();
    g.nameSpec.set(model::LStr::from(""));
    g.level.set(level);
    g.quality.set(quality);
    g.enabled.set(true);
    g.enableGlobal1.set(true);
    g.enableGlobal2.set(enable_global2);
    g.count.set(Some(1.0));
    g.extra.set(n!("new"), true);
    Rc::new(g)
}

/// `self.displayGroup.gemList[index] = gemInstance` (holes raise: module notes)
fn store_gem(cx: &Cx, index: usize, gem: &GemInstanceRef) {
    dg(cx).gemList.get().set(index, gem.clone());
}

/// `self:ProcessSocketGroup(self.displayGroup)`
fn process_display_group(cx: &Cx) {
    let m = cx.build().model.clone();
    let group = dg(cx);
    m.skillsTab.ProcessSocketGroup(&m, &group);
}

/// `self:ProcessGemLevel(gemData)` (`imbued` nil; a nil `gemData` is Lua's index error)
#[track_caller]
fn process_gem_level(cx: &Cx, gem: &GemInstanceRef) -> f64 {
    let m = cx.build().model.clone();
    let gem_data = gem
        .gemData
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'gemData' (a nil value)"));
    m.skillsTab.ProcessGemLevel(&m, &gem_data, false)
}

/// `slot.<check>.state = v`
fn set_state(cx: &mut Cx, id: CtrlId, v: bool) {
    cx.w_mut::<CheckBoxControl>(id).state = v;
}

/// `tooltip:CheckForUpdate(self.build.outputRevision, self.displayGroup)`
fn check_for_update(cx: &Cx, tooltip: &mut Tooltip) -> bool {
    let rev = cx.build().model.outputRevision.get();
    let group = display_group(cx).map_or(TtKey::Nil, |g| TtKey::Obj(TtObj(g as Rc<dyn std::any::Any>)));
    tooltip.check_for_update(cx, &[TtKey::Num(rev), group])
}

/// `self.build.calcsTab:GetMiscCalculator(self.build)` (`None`: Pending, web only)
fn misc_calculator(cx: &mut Cx) -> Option<(Rc<dyn crate::calc::MiscCalc>, model::Output)> {
    let m = cx.build().model.clone();
    match cx.build_mut().calc.misc_calculator(&m) {
        Poll::Ready(r) => Some(r),
        Poll::Pending => None,
    }
}

/// `grantedEffect.unsupported` (module notes)
const fn granted_effect_unsupported(_ge: &GrantedEffect) -> bool {
    false
}

/// `gemInstance["enableGlobal"..index]`
fn enable_global(gem: &GemInstance, index: usize) -> bool {
    match index {
        1 => gem.enableGlobal1.get(),
        2 => gem.enableGlobal2.get(),
        i => gem.extra.truthy(format!("enableGlobal{i}").as_str()),
    }
}

// ---- CreateGemSlot ------------------------------------------------------------------------------

/// The `deleteGem` local function (L747-763): removes the row's gem and refreshes the rows
/// below it.
fn delete_gem(cx: &mut Cx, me: CtrlId, index: usize) {
    // L748
    dg(cx).gemList.get().remove(Some(index));
    // L749
    let n = gem_count(cx);
    for index2 in index..=n {
        // L750 Update the other gem slot controls
        // L751
        let gemInstance = gem_at(cx, index2).unwrap_or_else(|| lua_error!("attempt to index local 'gemInstance' (a nil value)"));
        let s = gem_slot(cx, me, index2);
        // L752
        set_text(cx, s.name_spec, &gemInstance.nameSpec.get());
        // L753
        set_text(cx, s.level, &tostring_nan(gemInstance.level.get()));
        // L754
        set_text(cx, s.quality, &tostring_nan(gemInstance.quality.get()));
        // L755
        set_state(cx, s.enabled, gemInstance.enabled.get());
        // L756
        set_state(cx, s.enable_global1, gemInstance.enableGlobal1.get());
        // L757
        set_state(cx, s.enable_global2, gemInstance.enableGlobal2.get());
        // L758
        set_text(cx, s.count, &tostring_num(gemInstance.count.get().unwrap_or(1.0)));
    }
    // L760
    let m = cx.build().model.clone();
    m.skillsTab.UpdateSocketGroups(&m);
    // L761
    add_undo_state(cx, me);
    // L762
    set_build_flag(cx);
}

/// The quality edit's `tooltipFunc` (L888-946).
fn quality_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, index: usize) {
    // L889
    if check_for_update(cx, tooltip) {
        // L890 Get the gem instance from the skills
        // L891
        let Some(gemInstance) = gem_at(cx, index) else {
            // L893
            return;
        };
        // L895
        let gemData = gemInstance.gemData.get();

        // L897-921 Function for both granted effect and secondary such as vaal
        let addQualityLines = |cx: &mut Cx, tooltip: &mut Tooltip, qualityList: &[(model::Name, f64)], grantedEffect: &GrantedEffect| {
            let cc = cx.main.cc.clone();
            // L899
            tooltip.add_line(cx, 18.0, &format!("{}{}", cc.GEM, grantedEffect.name));
            // L900 Hardcoded to use 20% quality instead of grabbing from gem, this is for consistency and so we always show something
            // L901
            tooltip.add_line(cx, 16.0, &format!("{}At +20% Quality:", cc.NORMAL));
            // L902 (ORDER: module notes)
            for qual in qualityList {
                // L903 Do the stats one at a time because we're not guaranteed to get the descriptions in the same order we look at them here
                // L904
                let stats = model::Table::new();
                // L905
                stats.set(qual.0, qual.1 * 20.0);
                // L906
                let scope = grantedEffect
                    .statDescriptionScope
                    .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                let (descriptions, _) = stat_describer::describe_stats(cx, &stats, scope.as_str(), false);
                // L907 line may be nil if the value results in no line due to not being enough quality
                // L908
                for line in descriptions {
                    // L909 (`if line then`: every description is a string)
                    // L910 Check if we have a handler for the mod in the gem's statMap or in the shared stat map for skills
                    // L911
                    let handled = pob_gamedata::skill_stat_map::statMapLookup(grantedEffect, qual.0).is_some()
                        || model::data().skillStatMap.get(&qual.0).is_some();
                    if handled {
                        // L912
                        tooltip.add_line(cx, 16.0, &format!("{}{line}", cc.MAGIC));
                    } else {
                        // L914
                        let line = format!("{}{line}", cc.UNSUPPORTED);
                        // L915
                        let line = if cx.main.not_supported_mod_tooltips {
                            format!("{line}{}", cx.app().main.not_supported_tooltip_text)
                        } else {
                            line
                        };
                        // L916
                        tooltip.add_line(cx, 16.0, &line);
                    }
                }
            }
        };
        // L922 Check if there is quality for the effect
        // L923
        if let Some(gd) = &gemData {
            let ge = &gd.grantedEffect;
            // L924
            let qualityTable = &ge.qualityStats;
            // L925
            if !qualityTable.is_empty() {
                // L926
                addQualityLines(cx, tooltip, qualityTable, ge);
                // L927
                tooltip.add_separator(cx, Some(10.0));
            }
        }
        // L930
        if let Some(ge) = gemData.as_ref().and_then(|gd| gd.secondaryGrantedEffect.clone()) {
            // L931
            let qualityTable = &ge.qualityStats;
            // L932
            if !qualityTable.is_empty() {
                // L933
                addQualityLines(cx, tooltip, qualityTable, &ge);
                // L934
                tooltip.add_separator(cx, Some(10.0));
            }
        }

        // L938
        if let Some((calcFunc, calcBase)) = misc_calculator(cx) {
            // L939 (`if calcFunc then`: always set)
            // L940
            let storedQuality = gemInstance.quality.get();
            // L941
            gemInstance.quality.set(20.0);
            // L942
            let output = calcFunc.call(None, None);
            // L943
            gemInstance.quality.set(storedQuality);
            // L944
            if let Poll::Ready(output) = output {
                stats::add_stat_compares_to_tooltip(
                    cx,
                    tooltip,
                    &calcBase,
                    &output,
                    "^7Setting to 20 quality will give you:",
                    None,
                );
            }
        }
    }
}

/// The enabled check box's `tooltipFunc` (L975-987).
fn enabled_tooltip(cx: &mut Cx, tooltip: &mut Tooltip, index: usize) {
    // L976
    if check_for_update(cx, tooltip) {
        // L977
        if let Some(gem) = gem_at(cx, index) {
            // L978
            if let Some((calcFunc, calcBase)) = misc_calculator(cx) {
                // L979 (`if calcFunc then`: always set)
                // L980
                gem.enabled.set(!gem.enabled.get());
                // L981
                let output = calcFunc.call(None, None);
                // L982
                gem.enabled.set(!gem.enabled.get());
                // L983
                if let Poll::Ready(output) = output {
                    let header = if gem.enabled.get() {
                        "^7Disabling this gem will give you:"
                    } else {
                        "^7Enabling this gem will give you:"
                    };
                    stats::add_stat_compares_to_tooltip(cx, tooltip, &calcBase, &output, header, None);
                }
            }
        }
    }
}

/// `slot.enableGlobal<i>.label` (L1055-1057 / L1074-1076): "Enable <granted effect i's name>:".
#[track_caller]
fn enable_global_label(cx: &Cx, index: usize, effect: usize) -> String {
    let gem = gem_at(cx, index).unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    let gem_data = gem
        .gemData
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'gemData' (a nil value)"));
    let ge = gem_data
        .grantedEffectList
        .get(effect - 1)
        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
    format!("Enable {}:", ge.name)
}

/// `slot.enableGlobal<i>.shown` (L1048-1051 / L1067-1070).
fn enable_global_shown(cx: &Cx, index: usize, effect: usize) -> bool {
    // L1049 / L1068
    let Some(gem) = gem_at_opt(cx, index) else {
        return false;
    };
    // L1050 / L1069
    gem.gemData.get().is_some_and(|gd| {
        gd.vaalGem
            && gd
                .grantedEffectList
                .get(effect - 1)
                .is_some_and(|ge| !ge.support)
    })
}

/// lua: SkillsTab.lua:743-1080 `SkillsTabClass:CreateGemSlot(index)`: creates the controls for
/// editing the gem at `index` in the SkillsTab node `me` (`self.controls["gemSlot"..index..*]`)
/// and stores the row as `self.gemSlots[index]` (module notes).
/// @pob Classes/SkillsTab.lua:743-1080 h=2c49b418
#[allow(clippy::too_many_lines)]
pub fn CreateGemSlot(cx: &mut Cx, me: CtrlId, index: usize) {
    let h = skills_tab_ui_hooks();
    // D8 (module notes: a rebuilt row): the next row's anchor and `y` closure (L772-774) keep
    // this row's old delete button and enableGlobal check boxes alive in Lua, so the stores
    // below must not free them
    if (h.gem_slot)(cx, me, index + 1).is_some() {
        for key in ["Delete", "EnableGlobal1", "EnableGlobal2"] {
            cx.detach_child(me, &CtrlKey::from(format!("gemSlot{index}{key}")));
        }
    }
    // L744 local slot = { }
    let slot: Rc<Cell<GemSlot>> = Rc::new(Cell::new(GemSlot::default()));
    // L745 self.gemSlots[index] = slot (stored at the end: module notes)

    // L747-763 local function deleteGem() (`delete_gem`)
    // L764 Delete gem
    // L765-767
    let delete = cx
        .add(
            me,
            format!("gemSlot{index}Delete"),
            ButtonControl::new(Anchor::NONE, rect(0, 0, 20, 20), "x", move |cx| {
                // L766
                delete_gem(cx, me, index);
            }),
        )
        .id();
    slot.set(GemSlot { delete, ..slot.get() });
    // L768
    if index == 1 {
        // L769
        let anchor = (h.anchor_gem_slots)(cx, me);
        cx.set_anchor(delete, TopLeft, Some(anchor), TopLeft, Some((0.0.into(), 0.0.into())), false);
    } else {
        // L771
        let prevSlot = (h.gem_slot)(cx, me, index - 1)
            .unwrap_or_else(|| lua_error!("attempt to index local 'prevSlot' (a nil value)"));
        // L772-774
        let y = Prop::f(move |cx, _| {
            // L773
            if cx.is_shown(prevSlot.enable_global1) || cx.is_shown(prevSlot.enable_global2) {
                24.0
            } else {
                2.0
            }
        });
        cx.set_anchor(delete, TopLeft, Some(prevSlot.delete), BottomLeft, Some((0.0.into(), y)), false);
    }
    // L776-778
    cx.base_mut(delete).shown = Prop::f(move |cx, _| {
        // L777
        let group = dg(cx);
        index <= group.gemList.get().len() + 1 && group.source.get().is_none()
    });
    // L779-781
    cx.base_mut(delete).enabled = Prop::f(move |cx, _| {
        // L780
        index <= gem_count(cx)
    });
    // L782
    cx.w_mut::<ButtonControl>(delete).tt.tooltip_text = Some(Prop::Val(Some("Remove this gem.".to_owned())));
    // L783 self.controls["gemSlot"..index.."Delete"] = slot.delete (stored by `cx.add`)

    // L785 Gem name specification
    // L786-837
    let s = slot.clone();
    let change: GemChangeFunc = Rc::new(move |cx, gemId, addUndo, focusLost, bufMatchesGem| {
        // L787
        let Some(group) = display_group(cx) else {
            // L788
            return;
        };
        // L790
        let gemInstance = group.gemList.get().get(index);
        let slot = s.get();
        let gemInstance = match gemInstance {
            // L791
            None => {
                // L792
                if gemId.is_none() {
                    // L793
                    return;
                }
                // L795-804
                let g = new_gem_instance(1.0, default_gem_quality(cx), true);
                // L805
                store_gem(cx, index, &g);
                // L806
                set_text(cx, slot.level, &tostring_nan(g.level.get()));
                // L807
                set_text(cx, slot.quality, &tostring_nan(g.quality.get()));
                // L808
                set_state(cx, slot.enabled, true);
                // L809
                set_state(cx, slot.enable_global1, true);
                // L810
                set_state(cx, slot.enable_global2, true);
                // L811
                set_text(cx, slot.count, &tostring_num(g.count.get().unwrap_or(f64::NAN)));
                g
            }
            Some(g) => {
                // L812
                if focusLost && !bufMatchesGem {
                    // L813
                    delete_gem(cx, me, index);
                    return;
                // L814
                } else if gemId.as_deref() == g.gemId.get().map(|s| s.as_str()) {
                    // L815
                    if addUndo {
                        // L816
                        add_undo_state(cx, me);
                    }
                    // L818
                    if bufMatchesGem {
                        // L819
                        set_build_flag(cx);
                    }
                    // L821
                    return;
                }
                g
            }
        };
        // L823
        gemInstance.gemId.set(gemId.as_deref().map(model::Name::from));
        // L824
        gemInstance.skillId.set(None);
        // L825
        process_display_group(cx);
        // L826 New gems need to be constrained by ProcessGemLevel
        // L827
        let level = process_gem_level(cx, &gemInstance);
        gemInstance.level.set(level);
        // L828
        gemInstance.extra.set(n!("naturalMaxLevel"), gemInstance.level.get());
        // L829
        set_text(cx, slot.level, &tostring_nan(gemInstance.level.get()));
        // L830
        set_text(cx, slot.count, &tostring_num(gemInstance.count.get().unwrap_or(1.0)));
        // L831
        if addUndo {
            // L832
            add_undo_state(cx, me);
        }
        // L834
        if bufMatchesGem {
            // L835
            set_build_flag(cx);
        }
    });
    let name_spec = cx
        .add(
            me,
            format!("gemSlot{index}Name"),
            GemSelectControl::new(
                Anchor::new(Left, delete, Right),
                rect(2, 0, 300, 20),
                me,
                index as f64,
                Some(change),
                true,
                false,
            ),
        )
        .id();
    slot.set(GemSlot { name_spec, ..slot.get() });
    // L838
    let group_label = cx
        .child(me, "groupLabel")
        .unwrap_or_else(|| lua_error!("attempt to index local 'master' (a nil value)"));
    cx.add_to_tab_group(name_spec, group_label);
    // L839 self.controls["gemSlot"..index.."Name"] = slot.nameSpec (stored by `cx.add`)

    // L841 Gem level
    // L842-856
    let s = slot.clone();
    let level = cx
        .add(
            me,
            format!("gemSlot{index}Level"),
            EditControl::new(
                Anchor::new(Left, name_spec, Right),
                rect(2, 0, 60, 20),
                EditArgs {
                    filter: Some("%D".to_owned()),
                    limit: Some(2),
                    change_func: Some(Rc::new(move |cx, buf| {
                        let slot = s.get();
                        // L843
                        let gemInstance = match gem_at(cx, index) {
                            Some(g) => g,
                            // L844
                            None => {
                                // L845 (LUA-QUIRK level: module notes)
                                let g = new_gem_instance(f64::NAN, default_gem_quality(cx), true);
                                // L846
                                store_gem(cx, index, &g);
                                // L847
                                set_text(cx, slot.quality, &tostring_nan(g.quality.get()));
                                // L848
                                set_state(cx, slot.enabled, true);
                                // L849
                                set_state(cx, slot.enable_global1, true);
                                // L850
                                set_text(cx, slot.count, &tostring_num(g.count.get().unwrap_or(f64::NAN)));
                                g
                            }
                        };
                        // L852
                        let level = match tonumber(buf) {
                            Some(l) => l,
                            None => match gem_at(cx, index)
                                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                                .extra
                                .get(n!("naturalMaxLevel"))
                            {
                                Val::Nil | Val::Bool(false) => process_gem_level(cx, &gemInstance),
                                v => v.opt_num().unwrap_or_else(|| lua_error!("naturalMaxLevel is not a number")),
                            },
                        };
                        gemInstance.level.set(level);
                        // L853
                        process_display_group(cx);
                        // L854
                        add_undo_state(cx, me);
                        // L855
                        set_build_flag(cx);
                    })),
                    ..EditArgs::default()
                },
            ),
        )
        .id();
    slot.set(GemSlot { level, ..slot.get() });
    // L857
    cx.add_to_tab_group(level, group_label);
    // L858-860
    cx.base_mut(level).enabled = Prop::f(move |cx, _| {
        // L859
        index <= gem_count(cx)
    });
    // L861 self.controls["gemSlot"..index.."Level"] = slot.level (stored by `cx.add`)

    // L863 Gem quality
    // L864-878
    let s = slot.clone();
    let quality = cx
        .add(
            me,
            format!("gemSlot{index}Quality"),
            EditControl::new(
                Anchor::new(Left, level, Right),
                rect(2, 0, 60, 20),
                EditArgs {
                    filter: Some("%D".to_owned()),
                    limit: Some(2),
                    change_func: Some(Rc::new(move |cx, buf| {
                        let slot = s.get();
                        // L865
                        let gemInstance = match gem_at(cx, index) {
                            Some(g) => g,
                            // L866
                            None => {
                                // L867 (LUA-QUIRK level: module notes)
                                let g = new_gem_instance(f64::NAN, default_gem_quality(cx), true);
                                // L868
                                store_gem(cx, index, &g);
                                // L869
                                let text = default_level_text(cx);
                                set_text(cx, slot.level, &text);
                                // L870
                                set_state(cx, slot.enabled, true);
                                // L871
                                set_state(cx, slot.enable_global1, true);
                                // L872
                                set_text(cx, slot.count, &tostring_num(g.count.get().unwrap_or(f64::NAN)));
                                g
                            }
                        };
                        // L874
                        let q = tonumber(buf).unwrap_or_else(|| default_gem_quality(cx));
                        gemInstance.quality.set(q);
                        // L875
                        process_display_group(cx);
                        // L876
                        add_undo_state(cx, me);
                        // L877
                        set_build_flag(cx);
                    })),
                    ..EditArgs::default()
                },
            ),
        )
        .id();
    slot.set(GemSlot { quality, ..slot.get() });
    // L879-887
    cx.w_mut::<EditControl>(quality).border_func = Some(Rc::new(move |cx| {
        // L880
        let gemInstance = gem_at(cx, index);
        // L881 draw colourful colours for sockets with matching sockets
        // L882
        if let Some(g) = gemInstance {
            if g.extra.truthy(n!("matchesSocket")) {
                if let Some(color) = g.extra.str(n!("color")) {
                    // L883
                    let hex = pob_lua::gsub(&color, "%^", "0", None).0;
                    let [r, gr, b] = pob_ui_core::color_codes::hex_to_rgb(&hex).unwrap_or([0.5, 0.5, 0.5]);
                    return (r, gr, b);
                }
            }
        }
        // L885
        (0.5, 0.5, 0.5)
    }));
    // L888-947
    cx.w_mut::<EditControl>(quality).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, ()| {
        quality_tooltip(cx, tooltip, index);
    }));
    // L948
    cx.add_to_tab_group(quality, group_label);
    // L949-951
    cx.base_mut(quality).enabled = Prop::f(move |cx, _| {
        // L950
        index <= gem_count(cx)
    });
    // L952 self.controls["gemSlot"..index.."Quality"] = slot.quality (stored by `cx.add`)

    // L954 Enable gem
    // L955-974
    let s = slot.clone();
    let enabled = cx
        .add(
            me,
            format!("gemSlot{index}Enable"),
            CheckBoxControl::new(
                Anchor::new(Left, quality, Right),
                rect(18, 0, 20, 0),
                None::<String>,
                Some(Rc::new(move |cx: &mut Cx, state: bool| {
                    let slot = s.get();
                    // L956
                    let gemInstance = match gem_at(cx, index) {
                        Some(g) => g,
                        // L957
                        None => {
                            // L958 (LUA-QUIRK level: module notes)
                            let g = new_gem_instance(f64::NAN, default_gem_quality(cx), true);
                            // L959
                            store_gem(cx, index, &g);
                            // L960
                            let text = default_level_text(cx);
                            set_text(cx, slot.level, &text);
                            // L961
                            set_text(cx, slot.quality, &tostring_nan(g.quality.get()));
                            // L962
                            set_text(cx, slot.count, &tostring_num(g.count.get().unwrap_or(f64::NAN)));
                            g
                        }
                    };
                    // L964
                    let gem_data = gemInstance
                        .gemData
                        .get()
                        .unwrap_or_else(|| lua_error!("attempt to index field 'gemData' (a nil value)"));
                    if !gem_data.vaalGem {
                        // L965
                        set_state(cx, slot.enable_global1, true);
                        // L966
                        gemInstance.enableGlobal1.set(true);
                        // L967
                        set_state(cx, slot.enable_global2, true);
                        // L968
                        gemInstance.enableGlobal2.set(true);
                    }
                    // L970
                    gemInstance.enabled.set(state);
                    // L971
                    process_display_group(cx);
                    // L972
                    add_undo_state(cx, me);
                    // L973
                    set_build_flag(cx);
                }) as Rc<dyn Fn(&mut Cx, bool)>),
                None,
                false,
            ),
        )
        .id();
    slot.set(GemSlot { enabled, ..slot.get() });
    // L975-987
    cx.w_mut::<CheckBoxControl>(enabled).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, _state| {
        enabled_tooltip(cx, tooltip, index);
    }));
    // L988-990
    cx.base_mut(enabled).enabled = Prop::f(move |cx, _| {
        // L989
        index <= gem_count(cx)
    });
    // L991 self.controls["gemSlot"..index.."Enable"] = slot.enabled (stored by `cx.add`)

    // L993 Count gem
    // L994-1009
    let s = slot.clone();
    let count = cx
        .add(
            me,
            format!("gemSlot{index}Count"),
            EditControl::new(
                Anchor::new(Left, enabled, Right),
                rect(18, 0, 60, 20),
                EditArgs {
                    filter: Some("%D".to_owned()),
                    limit: Some(2),
                    change_func: Some(Rc::new(move |cx, buf| {
                        let slot = s.get();
                        // L995
                        let gemInstance = match gem_at(cx, index) {
                            Some(g) => g,
                            // L996
                            None => {
                                // L997 (no `enableGlobal2`; LUA-QUIRK level: module notes)
                                let g = new_gem_instance(f64::NAN, default_gem_quality(cx), false);
                                // L998
                                store_gem(cx, index, &g);
                                // L999
                                let text = default_level_text(cx);
                                set_text(cx, slot.level, &text);
                                // L1000
                                set_text(cx, slot.quality, &tostring_nan(g.quality.get()));
                                // L1001
                                set_state(cx, slot.enabled, true);
                                // L1002
                                set_state(cx, slot.enable_global1, true);
                                g
                            }
                        };
                        // L1004
                        gemInstance.count.set(Some(tonumber(buf).unwrap_or(1.0)));
                        // L1005 (a direct `buf` write: no SetText)
                        let text = tostring_num(gemInstance.count.get().unwrap_or(f64::NAN));
                        cx.w_mut::<EditControl>(slot.count).buf = text;
                        // L1006
                        process_display_group(cx);
                        // L1007
                        add_undo_state(cx, me);
                        // L1008
                        set_build_flag(cx);
                    })),
                    ..EditArgs::default()
                },
            ),
        )
        .id();
    slot.set(GemSlot { count, ..slot.get() });
    // L1010-1021
    cx.base_mut(count).shown = Prop::f(move |cx, _| {
        // L1011
        let gemInstance = gem_at_opt(cx, index);
        // L1012
        if let Some(gemInstance) = gemInstance {
            // L1013
            let grantedEffectList: Vec<pob_gamedata::GrantedEffectRef> = match gemInstance.gemData.get() {
                Some(gd) => gd.grantedEffectList.clone(),
                None => gemInstance.grantedEffect.get().into_iter().collect(),
            };
            // L1014
            for (i, grantedEffect) in grantedEffectList.iter().enumerate() {
                // L1015
                if !grantedEffect.support
                    && !granted_effect_unsupported(grantedEffect)
                    && (!pob_gamedata::skill_stat_map::hasGlobalEffect(grantedEffect)
                        || enable_global(&gemInstance, i + 1))
                {
                    // L1016
                    return true;
                }
            }
        }
        // L1020
        false
    });
    // L1022-1028
    cx.w_mut::<EditControl>(count).tt.tooltip_func = Some(Rc::new(move |cx, tooltip, ()| {
        // L1023
        if check_for_update(cx, tooltip) {
            // L1024
            tooltip.add_line(cx, 16.0, "^8Note: `count` integer value scales the DPS of associated skill by a scalar.");
            // L1025
            tooltip.add_line(cx, 16.0, "^8To be used with totems, minions, shot-gunning of projectiles (e.g., VD, magma-orbs),");
            // L1026
            tooltip.add_line(cx, 16.0, "^8multi-hit projectiles (e.g. ball-lightning), traps, mines.");
        }
    }));
    // L1029-1031
    cx.base_mut(count).enabled = Prop::f(move |cx, _| {
        // L1030
        index <= gem_count(cx)
    });
    // L1032 self.controls["gemSlot"..index.."Count"] = slot.count (stored by `cx.add`)

    // L1034 Parser/calculator error message
    // L1035-1038
    let err_msg = cx
        .add(
            me,
            format!("gemSlot{index}ErrMsg"),
            LabelControl::new(
                Anchor::new(Left, count, Right),
                rect(2, 2, 0, 16),
                Prop::f(move |cx, _| {
                    // L1036
                    let gemInstance = gem_at_opt(cx, index);
                    // L1037
                    let msg = gemInstance
                        .and_then(|g| match g.extra.get(n!("errMsg")) {
                            Val::Nil | Val::Bool(false) => None,
                            v => Some(v),
                        })
                        .map_or_else(String::new, |v| {
                            v.str().map_or_else(|| lua_error!("attempt to concatenate a non-string value"), |s| s.to_string())
                        });
                    format!("^1{msg}")
                }),
            ),
        )
        .id();
    slot.set(GemSlot { err_msg, ..slot.get() });
    // L1039 self.controls["gemSlot"..index.."ErrMsg"] = slot.errMsg (stored by `cx.add`)

    // L1041 Enable global-effect skill 1
    // L1042-1047
    let enable_global1 = cx
        .add(
            me,
            format!("gemSlot{index}EnableGlobal1"),
            CheckBoxControl::new(
                Anchor::new(TopLeft, delete, BottomLeft),
                rect(0, 2, 20, 0),
                Some(String::new()),
                Some(Rc::new(move |cx: &mut Cx, state: bool| {
                    // L1043
                    let gemInstance = gem_at(cx, index)
                        .unwrap_or_else(|| lua_error!("attempt to index local 'gemInstance' (a nil value)"));
                    // L1044
                    gemInstance.enableGlobal1.set(state);
                    // an explicit value is no longer CalcSetup's nil (CCR-0145 §4)
                    gemInstance.extra.set(n!("$enableGlobal1Nil"), Val::Nil);
                    // L1045
                    add_undo_state(cx, me);
                    // L1046
                    set_build_flag(cx);
                }) as Rc<dyn Fn(&mut Cx, bool)>),
                None,
                false,
            ),
        )
        .id();
    slot.set(GemSlot { enable_global1, ..slot.get() });
    // L1048-1051
    cx.base_mut(enable_global1).shown = Prop::f(move |cx, _| enable_global_shown(cx, index, 1));
    // L1052-1054
    cx.base_mut(enable_global1).x = Prop::f(move |cx, _| {
        // L1053
        if cx.is_shown(me) {
            let label = cx
                .w::<CheckBoxControl>(enable_global1)
                .label
                .get(cx, enable_global1)
                .unwrap_or_else(|| lua_error!("bad argument #3 to 'DrawStringWidth' (string expected, got nil)"));
            cx.sg.draw_string_width(16.0, Font::Var, &label) + 5.0
        } else {
            0.0
        }
    });
    // L1055-1057
    cx.w_mut::<CheckBoxControl>(enable_global1).label = Prop::f(move |cx, _| {
        // L1056
        Some(enable_global_label(cx, index, 1))
    });
    // L1058 self.controls["gemSlot"..index.."EnableGlobal1"] = slot.enableGlobal1 (stored by `cx.add`)

    // L1060 Enable global-effect skill 2
    // L1061-1066
    let enable_global2 = cx
        .add(
            me,
            format!("gemSlot{index}EnableGlobal2"),
            CheckBoxControl::new(
                Anchor::new(Left, enable_global1, Right).collapse(),
                rect(0, 0, 20, 0),
                Some(String::new()),
                Some(Rc::new(move |cx: &mut Cx, state: bool| {
                    // L1062
                    let gemInstance = gem_at(cx, index)
                        .unwrap_or_else(|| lua_error!("attempt to index local 'gemInstance' (a nil value)"));
                    // L1063
                    gemInstance.enableGlobal2.set(state);
                    // an explicit value is no longer CalcSetup's nil (CCR-0145 §4)
                    gemInstance.extra.set(n!("$enableGlobal2Nil"), Val::Nil);
                    // L1064
                    add_undo_state(cx, me);
                    // L1065
                    set_build_flag(cx);
                }) as Rc<dyn Fn(&mut Cx, bool)>),
                None,
                false,
            ),
        )
        .id();
    slot.set(GemSlot { enable_global2, ..slot.get() });
    // L1067-1070
    cx.base_mut(enable_global2).shown = Prop::f(move |cx, _| enable_global_shown(cx, index, 2));
    // L1071-1073
    cx.base_mut(enable_global2).x = Prop::f(move |cx, _| {
        // L1072
        if cx.is_shown(me) {
            let label = cx
                .w::<CheckBoxControl>(enable_global2)
                .label
                .get(cx, enable_global2)
                .unwrap_or_else(|| lua_error!("bad argument #3 to 'DrawStringWidth' (string expected, got nil)"));
            cx.sg.draw_string_width(16.0, Font::Var, &label) + 12.0
        } else {
            0.0
        }
    });
    // L1074-1076
    cx.w_mut::<CheckBoxControl>(enable_global2).label = Prop::f(move |cx, _| {
        // L1075
        Some(enable_global_label(cx, index, 2))
    });
    // L1077 self.controls["gemSlot"..index.."EnableGlobal2"] = slot.enableGlobal2 (stored by `cx.add`)

    // L745 self.gemSlots[index] = slot (module notes)
    (h.set_gem_slot)(cx, me, index, slot.get());
}
