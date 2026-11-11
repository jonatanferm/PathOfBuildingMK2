//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! `calcs.initEnv`: the context of its measured cross-segment locals and the
//! orchestrator (calc doc §7.1-§7.2). Owner: U0 (frozen). The segments are
//! CS1 (`init_env_base`), CS2 (`init_env_items`), CS3 (`init_env_skills`);
//! CS0 owns the helpers in `init_mod_db`, `node_mods` and `support`.
#![allow(non_snake_case)]

use crate::prelude::*;

pub mod init_env_base;
pub mod init_env_items;
pub mod init_env_skills;
pub mod init_mod_db;
pub mod node_mods;
pub mod support;

pub use init_mod_db::initModDB;
pub use node_mods::{buildModListForNode, buildModListForNodeList};
pub use support::wipeEnv;

crate::ctx_struct! {
    /// The locals of `calcs.initEnv` that cross a segment boundary (§7.1; params rule CCR-0005).
    /// `env` and `modDB` are nil until CS1 creates them (CalcSetup.lua:481, 486), so they are
    /// `Option` (contracts.md#ctx-deviations).
    pub struct InitEnvCtx {
        build: BuildRef,
        mode: Mode,
        specEnv: Option<SpecEnv>,
        cachedPlayerDB: Option<ModDB>,
        cachedEnemyDB: Option<ModDB>,
        cachedMinionDB: Option<ModDB>,
        env: Option<EnvRef>,
        accelerate: Accelerate,
        r#override: Rc<Override>,
        modDB: Option<ModDB>,
    }
}

impl InitEnvCtx {
    /// CalcSetup.lua 476-487 (the header lines of the orchestrator's marker).
    pub fn init(
        build: &BuildRef,
        mode: Mode,
        r#override: Option<Rc<Override>>,
        specEnv: Option<&SpecEnv>,
    ) -> InitEnvCtx {
        // L476
        pob_gamedata::ClearMatchKeywordFlagsCache();
        // accelerator variables
        // L478
        let cachedPlayerDB = specEnv.and_then(|s| s.cachedPlayerDB.clone());
        // L479
        let cachedEnemyDB = specEnv.and_then(|s| s.cachedEnemyDB.clone());
        // L480
        let cachedMinionDB = specEnv.and_then(|s| s.cachedMinionDB.clone());
        // L481
        let env = specEnv.and_then(|s| s.env.clone());
        // L482 (`specEnv.accelerate or { }`: absent fields are nil = false)
        let accelerate = specEnv.map(|s| s.accelerate).unwrap_or_default();
        // environment variables
        // L485
        let r#override = r#override.unwrap_or_default();
        // L486 (`local enemyDB = nil` at 487 does not cross: CS1 declares its own)
        let modDB = None;
        InitEnvCtx {
            build: build.clone(),
            mode,
            specEnv: specEnv.cloned(),
            cachedPlayerDB,
            cachedEnemyDB,
            cachedMinionDB,
            env,
            accelerate,
            r#override,
            modDB,
        }
    }

    /// `return env, cachedPlayerDB, cachedEnemyDB, cachedMinionDB` (CalcSetup.lua:1974).
    /// @pob Modules/CalcSetup.lua:1974-1975 h=47199551
    pub fn finish(self) -> (EnvRef, Option<ModDB>, Option<ModDB>, Option<ModDB>) {
        let env = match self.env {
            Some(e) => e,
            None => lua_error!("calcs.initEnv: env is nil at the final return"),
        };
        (
            env,
            self.cachedPlayerDB,
            self.cachedEnemyDB,
            self.cachedMinionDB,
        )
    }
}

/// `calcs.initEnv(build, mode, override, specEnv)` → (env, cachedPlayerDB, cachedEnemyDB, cachedMinionDB).
/// The restarts at CalcSetup.lua:1433 (CS2) and 1923 (CS3) re-run initEnv with the same
/// `override` object and `specEnv`.
/// @pob Modules/CalcSetup.lua:475-487 h=9d11df63
/// The cached DBs are `ModDB?` in Lua (nil until CS1 runs `specCopy`, CalcSetup.lua:590-692).
pub fn initEnv(
    build: &BuildRef,
    mode: Mode,
    r#override: Option<Rc<Override>>,
    specEnv: Option<&SpecEnv>,
) -> (EnvRef, Option<ModDB>, Option<ModDB>, Option<ModDB>) {
    let mut ctx = InitEnvCtx::init(build, mode, r#override, specEnv);
    for f in [
        init_env_base::seg as fn(&mut InitEnvCtx) -> Flow, // CS1 488-799
        init_env_items::seg,                               // CS2 800-1450 (Restart at 1433)
        init_env_skills::seg,                              // CS3 1451-1973 (Restart at 1923)
    ] {
        match f(&mut ctx) {
            Flow::Normal => {}
            Flow::Restart => {
                return initEnv(
                    build,
                    mode,
                    Some(ctx.r#override.clone()),
                    ctx.specEnv.as_ref(),
                )
            }
            Flow::Return => unreachable!("initEnv segments only restart"),
        }
    }
    ctx.finish()
}
