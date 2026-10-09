//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! CalcSetup.lua 305-474: wipeEnv and the support helpers.
//!
//! Owner: CS0. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/CalcSetup.lua:306-380 h=67ad3f26
/// Global `wipeEnv(env, accelerate)`.
pub fn wipeEnv(env: &EnvRef, accelerate: &Accelerate) {
    unported!("Modules/CalcSetup.lua:306-380")
}

/// @pob Modules/CalcSetup.lua:382-417 h=6cf90c88
/// `applyGemMods(effect, modList)`: `modList` holds the GemProperty mods.
pub(crate) fn applyGemMods(effect: &SkillEffectRef, modList: &[Value]) {
    unported!("Modules/CalcSetup.lua:382-417")
}

/// @pob Modules/CalcSetup.lua:419-426 h=091f9eb8
pub(crate) fn applySocketMods(
    env: &EnvRef,
    gem: &SkillEffectRef,
    groupCfg: &Cfg,
    socketNum: f64,
    modSource: Option<&str>,
) {
    unported!("Modules/CalcSetup.lua:419-426")
}

/// @pob Modules/CalcSetup.lua:428-457 h=40b82627
pub(crate) fn addBestSupport(
    supportEffect: &SkillEffectRef,
    appliedSupportList: &Shared<Vec<SkillEffectRef>>,
    mode: Mode,
) {
    unported!("Modules/CalcSetup.lua:428-457")
}
