//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! CalcPerform.lua 636-1285: actor helpers B and the ally life pool tables.
//!
//! Owner: PF-B. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/CalcPerform.lua:636-977 h=f6f72d62
/// `skillCfg` at 915 is an undeclared global (CCR-0004): LUA-GLOBAL-NIL.
pub(crate) fn doActorMisc(env: &EnvRef, actor: &ActorRef) {
    unported!("Modules/CalcPerform.lua:636-977")
}

/// @pob Modules/CalcPerform.lua:980-1139 h=d188062b
pub(crate) fn doActorCharges(env: &EnvRef, actor: &ActorRef) {
    unported!("Modules/CalcPerform.lua:980-1139")
}

/// @pob Modules/CalcPerform.lua:1141-1151 h=c94cebd4
pub fn actionSpeedMod(actor: &ActorRef) -> f64 {
    unported!("Modules/CalcPerform.lua:1141-1151")
}

/// @pob Modules/CalcPerform.lua:1154-1244 h=fd93e0f2
pub(crate) fn initMinionModDB(env: &EnvRef, activeSkill: &ActiveSkillRef, output: Option<&Output>) {
    unported!("Modules/CalcPerform.lua:1154-1244")
}

/// @pob Modules/CalcPerform.lua:1247-1253 h=ad6137e8
pub(crate) fn addMinionModifiers(modList: &ModList, skillCfg: Option<&ModCfg>, minion: &ActorRef) {
    unported!("Modules/CalcPerform.lua:1247-1253")
}

/// @pob Modules/CalcPerform.lua:1256-1272 h=093bd951
/// The ally life pool tables (CalcPerform.lua 1256-1272): `minionLifePoolBySkill`,
/// `spectreLifePool`, `companionshipLifePool`, `totemLifePoolByStat`, `allyLifeRedirects`.
pub(crate) fn allyLifePools() -> Table {
    unported!("Modules/CalcPerform.lua:1256-1272")
}
