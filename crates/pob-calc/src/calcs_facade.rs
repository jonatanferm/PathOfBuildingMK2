//! The `calcs.*` namespace and the UI entry points (calc doc §5.10).
//!
//! Lua globals defined by calc modules keep their names as free fns:
//! `wipeEnv` (CalcSetup:306), `doActorLifeMana` (CalcPerform:69),
//! `doActorLifeManaReservation` (551), `isTriggered` /
//! `calcMultiSpellRotationImpact` (CalcTriggers:41/91), `cacheSkillUUID` /
//! `cacheData` / `wipeGlobalCache` (Common), `calcSkillCooldown` /
//! `calcSkillDuration` (CalcOffence:296/322).
#![allow(non_snake_case)]

/// `calcs.*`.
pub mod calcs {
    pub use crate::modules::calc_active_skill::{
        buildActiveSkillModList, copyActiveSkill, createActiveSkill, createMinionSkills,
        mergeSkillInstanceMods,
    };
    pub use crate::modules::calc_defence::{
        applyDmgTakenConversion, armourReduction, armourReductionF, buildDefenceEstimations,
        defence, defenceForConditionals, hitChance, reducePoolsByDamage, resistances,
        takenHitFromDamage,
    };
    pub use crate::modules::calc_mirages::mirages;
    pub use crate::modules::calc_offence::{calcTotemLife, offence};
    pub use crate::modules::calc_perform::{actionSpeedMod, perform};
    pub use crate::modules::calc_setup::{
        buildModListForNode, buildModListForNodeList, initEnv, initModDB,
    };
    pub use crate::modules::calc_triggers::triggers;
    pub use crate::modules::calcs::{
        buildActiveSkill, buildOutput, calcFullDPS, getMiscCalculator, FullDPS, MiscCalculator,
    };
}

pub use crate::modules::calc_offence::{calcSkillCooldown, calcSkillDuration};
pub use crate::modules::calc_perform::{doActorLifeMana, doActorLifeManaReservation};
pub use crate::modules::calc_setup::wipeEnv;
pub use crate::modules::calc_triggers::{calcMultiSpellRotationImpact, isTriggered};
pub use crate::modules::common::{cacheData, cacheSkillUUID, wipeGlobalCache};

/// Desktop wrapper used by the UI (D15): a Lua error in the calc becomes `Err`,
/// as PoB shows a calc error. (wasm32 is `panic=abort`: the calc runs in a
/// worker the UI restarts.)
#[cfg(not(target_arch = "wasm32"))]
pub fn try_buildOutput(
    build: &crate::model::BuildRef,
    mode: crate::env::Mode,
) -> Result<crate::env::EnvRef, pob_lua::LuaError> {
    pob_lua::error::catch(|| calcs::buildOutput(build, mode))
}
