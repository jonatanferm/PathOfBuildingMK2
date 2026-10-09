//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! CalcOffence.lua 1-347 (module header, helpers) and segment OF-A 362-1036 (setup, incl. the
//! `return` at 371). The local functions `calcAreaOfEffect` (374), `calcResistForType` (494) and
//! `runSkillFunc` (509) cross segments, so they take the ctx (§7.2).
//!
//! Owner: OF-A. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use super::OffenceCtx;
use crate::data_fns::SkillHook;
use crate::prelude::*;

// @pob Modules/CalcOffence.lua:7-67 h=3d397894
// (module-level statements: calcs,pairs,ipairs,unpack,t_insert,t_remove,m_abs,m_floor,m_ceil,m_min,m_max,m_sqrt,m_pow,m_huge,bor…)

/// @pob Modules/CalcOffence.lua:70-141 h=7343ee16
/// → (min, max).
pub(crate) fn calcDamage(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    cfg: &Cfg,
    breakdown: Option<&Table>,
    damageType: Name,
    typeFlags: u32,
    convDst: Option<Name>,
) -> (f64, f64) {
    unported!("Modules/CalcOffence.lua:70-141")
}

/// @pob Modules/CalcOffence.lua:143-154 h=4efa567c
/// → (min, max).
pub(crate) fn calcAilmentSourceDamage(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    cfg: &Cfg,
    breakdown: Option<&Table>,
    damageType: Name,
    typeFlags: u32,
) -> (f64, f64) {
    unported!("Modules/CalcOffence.lua:143-154")
}

/// @pob Modules/CalcOffence.lua:160-166 h=9488e182
pub(crate) fn calcAreaMod(incArea: f64, moreArea: f64) -> f64 {
    unported!("Modules/CalcOffence.lua:160-166")
}

/// @pob Modules/CalcOffence.lua:173-178 h=c17d4edb
pub(crate) fn calcRadius(baseRadius: f64, areaMod: f64, radiusMod: Option<f64>) -> f64 {
    unported!("Modules/CalcOffence.lua:173-178")
}

/// @pob Modules/CalcOffence.lua:185-190 h=266f4623
pub(crate) fn calcMoltenStrikeTertiaryRadius(
    baseRadius: f64,
    deadzoneRadius: f64,
    areaMod: f64,
    speedMod: f64,
) -> f64 {
    unported!("Modules/CalcOffence.lua:185-190")
}

/// @pob Modules/CalcOffence.lua:198-224 h=748821af
/// → (incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint).
pub(crate) fn calcRadiusBreakpoints(
    baseRadius: f64,
    incArea: f64,
    moreArea: f64,
    radiusMod: Option<f64>,
) -> (f64, f64, f64, f64) {
    unported!("Modules/CalcOffence.lua:198-224")
}

/// @pob Modules/CalcOffence.lua:235-294 h=3529b74f
pub(crate) fn setMoltenStrikeTertiaryRadiusBreakdown(
    breakdown: &Table,
    deadzoneRadius: f64,
    baseRadius: f64,
    label: &str,
    incArea: f64,
    moreArea: f64,
    incSpd: f64,
    moreSpd: f64,
) {
    unported!("Modules/CalcOffence.lua:235-294")
}

/// @pob Modules/CalcOffence.lua:296-309 h=d3323536
/// Global `calcSkillCooldown` → (cooldown, rounded, addedCooldown).
pub fn calcSkillCooldown(
    skillModList: &ModList,
    skillCfg: Option<&ModCfg>,
    skillData: &Table,
) -> (f64, bool, Option<f64>) {
    unported!("Modules/CalcOffence.lua:296-309")
}

/// @pob Modules/CalcOffence.lua:311-320 h=685a9ce0
pub(crate) fn calcWarcryCastTime(
    skillModList: &ModList,
    skillCfg: Option<&ModCfg>,
    skillData: &Table,
    actor: &ActorRef,
) -> f64 {
    unported!("Modules/CalcOffence.lua:311-320")
}

/// @pob Modules/CalcOffence.lua:322-335 h=84318871
/// Global `calcSkillDuration`.
pub fn calcSkillDuration(
    skillModList: &ModList,
    skillCfg: Option<&ModCfg>,
    skillData: &Table,
    env: &EnvRef,
    enemyDB: &ModDB,
) -> f64 {
    unported!("Modules/CalcOffence.lua:322-335")
}

/// @pob Modules/CalcOffence.lua:338-342 h=76364366
/// → (life, lifeMod).
pub fn calcTotemLife(env: &EnvRef, activeSkill: &ActiveSkillRef) -> (f64, f64) {
    unported!("Modules/CalcOffence.lua:338-342")
}

/// @pob Modules/CalcOffence.lua:362-1036 h=14e4320f
/// OF-A: CalcOffence.lua 362-1036 (`Flow::Return` at 371).
pub(crate) fn seg_setup(ctx: &mut OffenceCtx) -> Flow {
    unported!("Modules/CalcOffence.lua:362-1036")
}

// Local functions declared in OF-A that cross segments (§7.2). Their provenance is part of the
// OF-A segment marker above (CalcOffence.lua 374-493, 494-508, 509-514).
pub(crate) fn calcAreaOfEffect(
    ctx: &OffenceCtx,
    skillModList: &ModList,
    skillCfg: &Cfg,
    skillData: &Table,
    skillFlags: &Table,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    unported!("Modules/CalcOffence.lua:374-493")
}

pub(crate) fn calcResistForType(ctx: &OffenceCtx, damageType: Name, cfg: Option<&ModCfg>) -> f64 {
    unported!("Modules/CalcOffence.lua:494-508")
}

/// `runSkillFunc(name)`: calls `activeSkill.activeEffect.grantedEffect[name]` and every
/// support's hook with the offence top-level `output`/`breakdown` (CalcOffence.lua:509-514).
pub(crate) fn runSkillFunc(ctx: &OffenceCtx, name: SkillHook) {
    unported!("Modules/CalcOffence.lua:509-514")
}
