//! @pob-file Modules/CalcMirages.lua @16de4b82d
//! CalcMirages.lua 1-427: mirage skills (the `MirageConfig` type is U0's, §8.1).
//!
//! Owner: MI. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// The config each mirage family builds inline (CalcMirages.lua 60-427), with the closures as boxed
/// Rust closures (calc doc §8.1).
pub struct MirageConfig {
    pub calcMainSkillOffence: bool,
    pub mirageSkill: Option<ActiveSkillRef>,
    pub compareFunc: Option<
        Box<
            dyn Fn(
                &ActiveSkillRef,
                &EnvRef,
                &MirageConfig,
                Option<ActiveSkillRef>,
            ) -> Option<ActiveSkillRef>,
        >,
    >,
    pub preCalcFunc: Box<dyn Fn(&EnvRef, &ActiveSkillRef, &EnvRef)>,
    pub postCalcFunc: Box<dyn Fn(&EnvRef, &ActiveSkillRef, &EnvRef)>,
    pub mirageSkillNotFoundFunc: Box<dyn Fn(&EnvRef, &MirageConfig)>,
}

// @pob Modules/CalcMirages.lua:8-22 h=31864e9a
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_min,m_max,m_ceil,m_floor,m_modf,s_format,m_huge,bor,band)

/// @pob Modules/CalcMirages.lua:24-58 h=83929a90
/// → `not config.calcMainSkillOffence` (nil when no mirage skill).
pub(crate) fn calculateMirage(env: &EnvRef, config: &MirageConfig) -> Option<bool> {
    unported!("Modules/CalcMirages.lua:24-58")
}

/// @pob Modules/CalcMirages.lua:60-428 h=e1da88f5
pub fn mirages(env: &EnvRef) {
    unported!("Modules/CalcMirages.lua:60-428")
}
