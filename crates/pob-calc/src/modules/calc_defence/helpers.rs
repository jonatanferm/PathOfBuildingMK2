//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! CalcDefence.lua 1-650: module header and defence helpers.
//!
//! Owner: DF-A. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Modules/CalcDefence.lua:7-43 h=fbb8bcde
// (module-level statements: calcs,pairs,ipairs,t_insert,m_min,m_max,m_abs,m_floor,m_ceil,m_sqrt,m_modf,m_huge,s_format,tempTable…)

/// @pob Modules/CalcDefence.lua:45-55 h=0fa67014
pub(crate) fn buildAllyLifePools(output: &Output) -> Table {
    unported!("Modules/CalcDefence.lua:45-55")
}

/// @pob Modules/CalcDefence.lua:58-64 h=61fc1712
pub fn hitChance(evasion: f64, accuracy: f64) -> f64 {
    unported!("Modules/CalcDefence.lua:58-64")
}

/// @pob Modules/CalcDefence.lua:67-72 h=6dcb2e6c
pub fn armourReductionF(armour: f64, raw: f64) -> f64 {
    // TEMP-DFD-VERIFY (not committed)
    if armour == 0.0 && raw == 0.0 { return 0.0; }
    armour / (armour + raw * 5.0) * 100.0
}

/// @pob Modules/CalcDefence.lua:75-77 h=f9d1d27f
pub fn armourReduction(armour: f64, raw: f64) -> f64 {
    // TEMP-DFD-VERIFY (not committed)
    round(armourReductionF(armour, raw), None)
}

/// @pob Modules/CalcDefence.lua:84-143 h=e1281963
/// → (damageBreakdown, totalDamageTaken).
pub fn applyDmgTakenConversion(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
    sourceType: Name,
    baseDmg: f64,
) -> (Table, f64) {
    unported!("Modules/CalcDefence.lua:84-143")
}

/// @pob Modules/CalcDefence.lua:150-177 h=b88dfa45
/// → (receivedDamageSum, damages).
pub fn takenHitFromDamage(rawDamage: f64, damageType: Name, actor: &ActorRef) -> (f64, Table) {
    unported!("Modules/CalcDefence.lua:150-177")
}

/// @pob Modules/CalcDefence.lua:179-183 h=735ad761
pub(crate) fn calcLifeHitPoolWithLossPrevention(
    life: f64,
    maxLife: f64,
    lifeLossPrevented: f64,
    lifeLossBelowHalfPrevented: f64,
) -> f64 {
    unported!("Modules/CalcDefence.lua:179-183")
}

/// @pob Modules/CalcDefence.lua:190-424 h=8c0e4fc8
pub fn reducePoolsByDamage(
    poolTable: Option<&Table>,
    damageTable: &Table,
    actor: &ActorRef,
) -> Table {
    unported!("Modules/CalcDefence.lua:190-424")
}

/// @pob Modules/CalcDefence.lua:431-478 h=a0f88e59
pub(crate) fn incomingDamageBreakdown(
    breakdownTable: &Table,
    poolsRemaining: &Table,
    output: &Output,
) -> Table {
    unported!("Modules/CalcDefence.lua:431-478")
}

/// @pob Modules/CalcDefence.lua:481-508 h=d72afd51
pub fn defenceForConditionals(env: &EnvRef, actor: &ActorRef) {
    unported!("Modules/CalcDefence.lua:481-508")
}

/// @pob Modules/CalcDefence.lua:512-648 h=ef34d9c5
pub fn resistances(actor: &ActorRef) {
    unported!("Modules/CalcDefence.lua:512-648")
}
