//! @pob-file Modules/CalcActiveSkill.lua @16de4b82d
//! Modules/CalcActiveSkill.lua 1-934: active skills, their mod lists and minion skills.
//! `mergeLevelCache` (CalcActiveSkill.lua:21) becomes a thread-local memo keyed by (Arc pointer of the data mod, value bits) (§5.6).
//!
//! Owner: AS. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Modules/CalcActiveSkill.lua:7-21 h=0d514f10
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_floor,m_min,m_max,bor,band,bnot,mergeLevelCache)

/// @pob Modules/CalcActiveSkill.lua:22-50 h=b49f410e
pub(crate) fn mergeLevelMod(modList: &ModList, r#mod: &ModRef, value: Option<&Value>) {
    unported!("Modules/CalcActiveSkill.lua:22-50")
}

/// @pob Modules/CalcActiveSkill.lua:53-79 h=28ec5dde
pub fn mergeSkillInstanceMods(
    env: &EnvRef,
    modList: &ModList,
    skillEffect: &SkillEffectRef,
    extraStats: Option<&Table>,
) {
    unported!("Modules/CalcActiveSkill.lua:53-79")
}

/// @pob Modules/CalcActiveSkill.lua:83-164 h=6a08350d
/// `calcs.createActiveSkill(activeEffect, supportList, actor, socketGroup, summonSkill)`.
pub fn createActiveSkill(
    activeEffect: SkillEffectRef,
    supportList: &Shared<Vec<SkillEffectRef>>,
    actor: ActorWeak,
    socketGroup: Option<SocketGroupRef>,
    summonSkill: Option<ActiveSkillRef>,
) -> ActiveSkillRef {
    let () = unported!("Modules/CalcActiveSkill.lua:83-164");
    // U0-SKELETON: the table createActiveSkill starts from (AS replaces the body).
    ActiveSkill::new(activeEffect, supportList.clone(), actor)
}

/// @pob Modules/CalcActiveSkill.lua:167-192 h=0b95f5c3
/// → (newSkill, newEnv).
pub fn copyActiveSkill(
    env: &EnvRef,
    mode: Mode,
    skill: &ActiveSkillRef,
) -> (ActiveSkillRef, EnvRef) {
    let () = unported!("Modules/CalcActiveSkill.lua:167-192");
    // U0-SKELETON: stub result (AS replaces the body).
    (skill.clone(), env.clone())
}

/// @pob Modules/CalcActiveSkill.lua:195-226 h=4e145083
/// → the weapon flags, or nil when the weapon type is unknown (second result: `info`).
pub(crate) fn getWeaponFlags(
    env: &EnvRef,
    weaponData: &Table,
    weaponTypes: Option<&pob_gamedata::NameSet>,
) -> (Option<u32>, Option<pob_gamedata::WeaponTypeInfo>) {
    unported!("Modules/CalcActiveSkill.lua:195-226")
}

/// @pob Modules/CalcActiveSkill.lua:229-875 h=35d34a41
pub fn buildActiveSkillModList(env: &EnvRef, activeSkill: &ActiveSkillRef) {
    unported!("Modules/CalcActiveSkill.lua:229-875")
}

/// @pob Modules/CalcActiveSkill.lua:878-934 h=b0ce6385
pub fn createMinionSkills(env: &EnvRef, activeSkill: &ActiveSkillRef) {
    unported!("Modules/CalcActiveSkill.lua:878-934")
}
