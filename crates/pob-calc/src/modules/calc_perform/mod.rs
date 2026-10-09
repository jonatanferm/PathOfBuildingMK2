//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! `calcs.perform`: the context of its measured cross-segment locals and the
//! orchestrator (calc doc §7.1-§7.2). Owner: U0 (frozen). Segments: PF-C
//! `setup_flasks`, PF-D `pools_charges`, PF-E `buffs`, PF-F `party_curses`,
//! PF-G `ailments_dispatch`; helpers: PF-A `actor_a`, PF-B `actor_b`.
#![allow(non_snake_case)]

use crate::prelude::*;

pub mod actor_a;
pub mod actor_b;
pub mod ailments_dispatch;
pub mod buffs;
pub mod party_curses;
pub mod pools_charges;
pub mod setup_flasks;

pub use actor_a::{doActorLifeMana, doActorLifeManaReservation};
pub use actor_b::actionSpeedMod;

crate::ctx_struct! {
    /// The locals of `calcs.perform` that cross a segment boundary (§7.1). Fields whose local
    /// is declared in a later segment are default-initialised and first written there.
    /// `allyBuffs` is `env.partyMembers["Aura"]` (nil when absent), hence `Option`
    /// (contracts.md#ctx-deviations).
    pub struct PerformCtx {
        env: EnvRef,
        skipEHP: bool,
        modDB: ModDB,
        enemyDB: ModDB,
        output: Output,
        breakdown: Option<Breakdown>,
        partyTabEnableExportBuffs: bool,
        hasGuaranteedBonechill: bool,
        guaranteedChillSkills: Table,
        summonedMinionOverride: Option<f64>,
        nonUniqueFlasksApplyToMinion: bool,
        buffs: Table,
        guards: Table,
        minionBuffs: Table,
        debuffs: Table,
        curses: Table,
        minionCurses: Table,
        linkSkills: Table,
        allyBuffs: Option<Table>,
        buffExports: Table,
        appliedCombustion: bool,
        warcryList: Table,
    }
}

impl PerformCtx {
    /// CalcPerform.lua 1287-1288 (`local modDB = env.modDB`, `local enemyDB = env.enemyDB`).
    pub fn init(env: &EnvRef, skipEHP: bool) -> PerformCtx {
        // L1287
        let modDB = env.modDB.get();
        // L1288
        let enemyDB = env.enemyDB.get();
        PerformCtx {
            env: env.clone(),
            skipEHP,
            modDB,
            enemyDB,
            output: Table::new(),
            breakdown: None,
            partyTabEnableExportBuffs: false,
            hasGuaranteedBonechill: false,
            guaranteedChillSkills: Table::new(),
            summonedMinionOverride: None,
            nonUniqueFlasksApplyToMinion: false,
            buffs: Table::new(),
            guards: Table::new(),
            minionBuffs: Table::new(),
            debuffs: Table::new(),
            curses: Table::new(),
            minionCurses: Table::new(),
            linkSkills: Table::new(),
            allyBuffs: None,
            buffExports: Table::new(),
            appliedCombustion: false,
            warcryList: Table::new(),
        }
    }
}

/// `calcs.perform(env, skipEHP)`.
/// @pob Modules/CalcPerform.lua:1286-1288 h=5b6c1c01
pub fn perform(env: &EnvRef, skipEHP: bool) {
    let mut ctx = PerformCtx::init(env, skipEHP); // L1287-1288
    seg!(setup_flasks::seg(&mut ctx)); // PF-C 1289-1942 (output 1305-1308 is ctx-assigned here)
    seg!(pools_charges::seg(&mut ctx)); // PF-D 1943-2307
    seg!(buffs::seg(&mut ctx)); // PF-E 2308-2996
    seg!(party_curses::seg(&mut ctx)); // PF-F 2997-3496
    seg!(ailments_dispatch::seg(&mut ctx)); // PF-G 3497-3990 (defence/offence/triggers/mirages/cacheData)
}
