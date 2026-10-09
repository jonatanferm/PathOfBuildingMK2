//! @pob-file Modules/CalcTriggers.lua @16de4b82d
//! CalcTriggers.lua 1-901: trigger helpers and handlers.
//!
//! Owner: TR-A. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use super::TriggerConfig;
use crate::prelude::*;

// @pob Modules/CalcTriggers.lua:8-21 h=5c4fa880
// (module-level statements: calcs,ipairs,t_insert,t_remove,m_min,m_max,m_ceil,m_floor,m_modf,s_format,m_huge,bor,band)

/// @pob Modules/CalcTriggers.lua:24-31 h=8cdaa24e
pub(crate) fn addTriggerIncMoreMods(activeSkill: &ActiveSkillRef, sourceSkill: &ActiveSkillRef) {
    unported!("Modules/CalcTriggers.lua:24-31")
}

/// @pob Modules/CalcTriggers.lua:33-39 h=8db65e26
pub(crate) fn slotMatch(env: &EnvRef, skill: &ActiveSkillRef) -> bool {
    unported!("Modules/CalcTriggers.lua:33-39")
}

/// @pob Modules/CalcTriggers.lua:41-43 h=f5df59e5
/// Global `isTriggered(skill)` (Lua returns the first truthy operand; callers test it).
pub fn isTriggered(skill: &ActiveSkillRef) -> bool {
    unported!("Modules/CalcTriggers.lua:41-43")
}

/// @pob Modules/CalcTriggers.lua:45-63 h=c820cea2
/// → (addsCastTime, csi).
pub(crate) fn processAddedCastTime(
    skill: &ActiveSkillRef,
    breakdown: Option<&Table>,
) -> (Option<f64>, Option<f64>) {
    unported!("Modules/CalcTriggers.lua:45-63")
}

/// @pob Modules/CalcTriggers.lua:65-67 h=c64d7c16
pub(crate) fn packageSkillDataForSimulation(skill: &ActiveSkillRef, env: &EnvRef) -> Table {
    unported!("Modules/CalcTriggers.lua:65-67")
}

/// @pob Modules/CalcTriggers.lua:69-72 h=615d5921
pub(crate) fn defaultComparer(
    env: &EnvRef,
    uuid: Name,
    source: Option<&ActiveSkillRef>,
    triggerRate: f64,
) -> bool {
    unported!("Modules/CalcTriggers.lua:69-72")
}

/// @pob Modules/CalcTriggers.lua:75-87 h=0a8c3102
/// → (source, triggerRate, uuid).
pub(crate) fn findTriggerSkill(
    env: &EnvRef,
    skill: &ActiveSkillRef,
    source: Option<ActiveSkillRef>,
    triggerRate: Option<f64>,
    comparer: Option<fn(&EnvRef, Name, &ActiveSkillRef, f64) -> bool>,
) -> (Option<ActiveSkillRef>, Option<f64>, Option<Name>) {
    unported!("Modules/CalcTriggers.lua:75-87")
}

/// @pob Modules/CalcTriggers.lua:91-134 h=6bdf7133
/// Global `calcMultiSpellRotationImpact` → (mainRate, trigRateTable).
pub fn calcMultiSpellRotationImpact(
    env: &EnvRef,
    skillRotation: &Table,
    sourceRate: f64,
    triggerCD: f64,
    chance: Option<f64>,
    actor: &ActorRef,
) -> (f64, Table) {
    unported!("Modules/CalcTriggers.lua:91-134")
}

/// @pob Modules/CalcTriggers.lua:136-220 h=f6f674ce
pub(crate) fn helmetFocusHandler(env: &EnvRef, config: &mut TriggerConfig) -> Option<Table> {
    unported!("Modules/CalcTriggers.lua:136-220")
}

/// @pob Modules/CalcTriggers.lua:222-392 h=804aedd9
pub(crate) fn CWCHandler(env: &EnvRef, config: &mut TriggerConfig) -> Option<Table> {
    unported!("Modules/CalcTriggers.lua:222-392")
}

/// @pob Modules/CalcTriggers.lua:394-900 h=8a0c6dbc
pub(crate) fn defaultTriggerHandler(env: &EnvRef, config: &mut TriggerConfig) -> Option<Table> {
    unported!("Modules/CalcTriggers.lua:394-900")
}
