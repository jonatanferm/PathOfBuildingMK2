//! @pob-file Modules/PantheonTools.lua @16de4b82d
//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! initEnv segment CS1 (CalcSetup.lua 488-799): env, actors, base mods, bandit/pantheon, config;
//! and Modules/PantheonTools.lua.
//!
//! Owner: CS1. Stub created by U0: signatures are final (frozen; change through a CCR),
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

// @pob Modules/PantheonTools.lua:4-4 h=9f7e4127
// (module-level statements: pantheon)

/// @pob Modules/PantheonTools.lua:6-19 h=7b14b491
/// `pantheon.applySoulMod(db, modParser, god)` (`modParser` is `modLib.parseMod`).
pub fn applySoulMod(db: &ModDB, god: &pob_gamedata::schema::misc::Pantheon) {
    unported!("Modules/PantheonTools.lua:6-19")
}

/// @pob Modules/CalcSetup.lua:488-799 h=9a76817b
/// CS1: CalcSetup.lua 488-799.
pub(crate) fn seg(ctx: &mut InitEnvCtx) -> Flow {
    let () = unported!("Modules/CalcSetup.lua:488-799");
    // U0-SKELETON so the pipeline runs end to end on stubs (CS1 replaces it with
    // the Lua statements 489-500, which create the env exactly this way).
    if ctx.env.is_none() {
        let spec = ctx
            .r#override
            .spec
            .get()
            .unwrap_or_else(|| ctx.build.spec.get());
        let env = Env::new(&ctx.build, ctx.mode, ctx.r#override.clone(), spec);
        ctx.modDB = Some(env.modDB.get());
        ctx.env = Some(env);
    }
    Flow::Normal
}
