//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! CalcPerform.lua 1-635: module header and actor helpers A.
//!
//! Owner: PF-A. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

// @pob Modules/CalcPerform.lua:7-22 h=d1d8c60a
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_min,m_max,m_ceil,m_floor,m_modf,s_format,m_huge,bor,band,bnot)

/// @pob Modules/CalcPerform.lua:31-42 h=1053e69a
/// `getCachedOutputValue(env, activeSkill, ...)`: the cached outputs named, as values.
pub(crate) fn getCachedOutputValue(
    env: &EnvRef,
    activeSkill: &ActiveSkillRef,
    names: &[Name],
) -> Vec<Val> {
    unported!("Modules/CalcPerform.lua:31-42")
}

/// @pob Modules/CalcPerform.lua:45-67 h=ff615531
/// `mergeBuff(src, destTable, destKey)`: `src` and the merged buff are buff mod tables.
pub(crate) fn mergeBuff(src: &Table, destTable: &Table, destKey: Name) {
    unported!("Modules/CalcPerform.lua:45-67")
}

/// @pob Modules/CalcPerform.lua:69-133 h=b6d0f139
/// Global `doActorLifeMana(actor)`.
pub fn doActorLifeMana(actor: &ActorRef) {
    unported!("Modules/CalcPerform.lua:69-133")
}

/// @pob Modules/CalcPerform.lua:138-547 h=a56d79fa
pub(crate) fn doActorAttribsConditions(env: &EnvRef, actor: &ActorRef) {
    unported!("Modules/CalcPerform.lua:138-547")
}

/// @pob Modules/CalcPerform.lua:551-584 h=88ae145e
/// Global `doActorLifeManaReservation(actor, addAura)`.
pub fn doActorLifeManaReservation(actor: &ActorRef, addAura: bool) {
    unported!("Modules/CalcPerform.lua:551-584")
}

/// @pob Modules/CalcPerform.lua:587-617 h=cc53e1d0
pub(crate) fn determineCursePriority(curseName: Name, activeSkill: Option<&ActiveSkillRef>) -> f64 {
    unported!("Modules/CalcPerform.lua:587-617")
}

/// @pob Modules/CalcPerform.lua:619-633 h=7a9a1545
pub(crate) fn applyEnemyModifiers(actor: &ActorRef, clearCache: bool) {
    unported!("Modules/CalcPerform.lua:619-633")
}
