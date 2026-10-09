//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! initEnv segment CS3 (CalcSetup.lua 1451-1975): skills, supports, minions; restarts at 1923; final merge.
//! The final `return env, cachedPlayerDB, …` (1974) is the orchestrator's `InitEnvCtx::finish`.
//!
//! Owner: CS3. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use super::InitEnvCtx;
use crate::prelude::*;

/// @pob Modules/CalcSetup.lua:1451-1973 h=952365eb
/// CS3: CalcSetup.lua 1451-1973 (`Flow::Restart` at 1923).
pub(crate) fn seg(ctx: &mut InitEnvCtx) -> Flow {
    unported!("Modules/CalcSetup.lua:1451-1973")
}
