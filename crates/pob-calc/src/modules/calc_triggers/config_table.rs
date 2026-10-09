//! @pob-file Modules/CalcTriggers.lua @16de4b82d
//! CalcTriggers.lua 902-1627: `configTable` (82 arms, as one match in Lua order) and `calcs.triggers`.
//!
//! Owner: TR-B. Stub created by U0: signatures are final (frozen; change through a CCR),
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

/// @pob Modules/CalcTriggers.lua:902-1582 h=75156de6
/// `configTable[key](env)`: one arm per Lua key, in Lua order (provenance per arm: TR-B).
pub(crate) fn configTable(key: Name, env: &EnvRef) -> Option<TriggerConfig> {
    unported!("Modules/CalcTriggers.lua:902-1582")
}

/// @pob Modules/CalcTriggers.lua:1585-1600 h=d4ca6f88
pub(crate) fn getUniqueItemTriggerName(skill: &ActiveSkillRef) -> Option<Name> {
    unported!("Modules/CalcTriggers.lua:1585-1600")
}

/// @pob Modules/CalcTriggers.lua:1602-1627 h=4f18f39d
pub fn triggers(env: &EnvRef, actor: Option<&ActorRef>) {
    unported!("Modules/CalcTriggers.lua:1602-1627")
}
