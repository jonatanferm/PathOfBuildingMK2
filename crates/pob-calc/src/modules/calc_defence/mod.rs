//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! `calcs.defence` and `calcs.buildDefenceEstimations`: contexts and
//! orchestrators (calc doc §7.1-§7.2). Owner: U0 (frozen). Segments: DF-B
//! `primary`, DF-C `recovery`, DF-D `est_inputs`, DF-E `est_pools`, DF-F
//! `est_ehp`, DF-G `est_maxhit`; helpers: DF-A `helpers`.
#![allow(non_snake_case)]

use crate::prelude::*;

pub mod est_ehp;
pub mod est_inputs;
pub mod est_maxhit;
pub mod est_pools;
pub mod helpers;
pub mod primary;
pub mod recovery;

pub use helpers::{
    applyDmgTakenConversion, armourReduction, armourReductionF, defenceForConditionals, hitChance,
    reducePoolsByDamage, resistances, takenHitFromDamage,
};

crate::ctx_struct! {
    /// The locals of `calcs.defence` that cross a segment boundary (§7.1; `env` per CCR-0005).
    pub struct DefenceCtx {
        env: EnvRef,
        actor: ActorRef,
        modDB: ModDB,
        enemyDB: ModDB,
        output: Output,
        breakdown: Option<Breakdown>,
        spellSuppressionChance: Option<f64>,
    }
}

crate::ctx_struct! {
    /// The locals of `calcs.buildDefenceEstimations` that cross a segment boundary (§7.1).
    pub struct DefEstCtx {
        env: EnvRef,
        actor: ActorRef,
        modDB: ModDB,
        enemyDB: ModDB,
        output: Output,
        breakdown: Option<Breakdown>,
        damageCategoryConfig: Name,
    }
}

/// `actor.enemy.modDB` (indexing a nil `enemy` raises, as in Lua).
fn enemy_modDB(actor: &ActorRef) -> ModDB {
    match actor.enemy() {
        Some(e) => e.modDB.get(),
        None => lua_error!("attempt to index field 'enemy' (a nil value)"),
    }
}

impl DefenceCtx {
    /// CalcDefence.lua 652-658.
    pub fn init(env: &EnvRef, actor: &ActorRef) -> DefenceCtx {
        // L653
        let modDB = actor.modDB.get();
        // L654
        let enemyDB = enemy_modDB(actor);
        // L656
        let output = actor.output.get();
        // L658
        let breakdown = actor.breakdown.get();
        DefenceCtx {
            env: env.clone(),
            actor: actor.clone(),
            modDB,
            enemyDB,
            output,
            breakdown,
            spellSuppressionChance: None,
        }
    }
}

impl DefEstCtx {
    /// CalcDefence.lua 1675-1680.
    pub fn init(env: &EnvRef, actor: &ActorRef) -> DefEstCtx {
        // L1675
        let modDB = actor.modDB.get();
        // L1676
        let enemyDB = enemy_modDB(actor);
        // L1678
        let output = actor.output.get();
        // L1680
        let breakdown = actor.breakdown.get();
        DefEstCtx {
            env: env.clone(),
            actor: actor.clone(),
            modDB,
            enemyDB,
            output,
            breakdown,
            damageCategoryConfig: Name::default(),
        }
    }
}

/// `calcs.defence(env, actor)`.
/// @pob Modules/CalcDefence.lua:651-658 h=a2495a8e
pub fn defence(env: &EnvRef, actor: &ActorRef) {
    let mut c = DefenceCtx::init(env, actor);
    seg!(primary::seg(&mut c)); // DF-B 659-1210
    seg!(recovery::seg(&mut c)); // DF-C 1211-1673
}

/// `calcs.buildDefenceEstimations(env, actor)`.
/// @pob Modules/CalcDefence.lua:1674-1680 h=4a9d7dc4
pub fn buildDefenceEstimations(env: &EnvRef, actor: &ActorRef) {
    let mut c = DefEstCtx::init(env, actor);
    seg!(est_inputs::seg(&mut c)); // DF-D 1681-2152
    seg!(est_pools::seg(&mut c)); // DF-E 2153-2611
    seg!(est_ehp::seg(&mut c)); // DF-F 2612-3164
    seg!(est_maxhit::seg(&mut c)); // DF-G 3165-3873
}
