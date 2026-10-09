//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! `calcs.offence`: the context of its measured cross-segment locals, the
//! pass/ailment loop locals and the orchestrator (calc doc §7.1-§7.2).
//! Owner: U0 (frozen). Segments: OF-A `helpers_setup`, OF-B `skill_types`,
//! OF-C `costs_passes`, OF-D `speed`, OF-E `crit`, OF-F `hit_dps`, OF-G
//! `ailments_a`, OF-H `ailments_b`, OF-I `dot_combined`.
#![allow(non_snake_case)]

use crate::prelude::*;

pub mod ailments_a;
pub mod ailments_b;
pub mod costs_passes;
pub mod crit;
pub mod dot_combined;
pub mod helpers_setup;
pub mod hit_dps;
pub mod skill_types;
pub mod speed;

pub use helpers_setup::{calcSkillCooldown, calcSkillDuration, calcTotemLife};

crate::ctx_struct! {
    /// The locals of `calcs.offence` that cross a segment boundary (§7.1). Local functions that
    /// cross (`calcAreaOfEffect`, `calcResistForType`, `runSkillFunc`, `combineStat`) are
    /// ctx-taking fns in their defining segment's file. `ailmentData` is
    /// `data.nonDamagingAilment` (immutable data, so a reference; contracts.md#ctx-deviations).
    pub struct OffenceCtx {
        env: EnvRef,
        actor: ActorRef,
        activeSkill: ActiveSkillRef,
        modDB: ModDB,
        enemyDB: ModDB,
        output: Output,
        breakdown: Option<Breakdown>,
        skillModList: ModList,
        skillData: Table,
        skillFlags: Table,
        skillCfg: Cfg,
        isAttack: bool,
        debuffDurationMult: f64,
        costs: Table,
        monsterLife: f64,
        canDeal: Table,
        passList: Shared<Vec<Table>>,
        storedMainHandAccuracy: Option<f64>,
        storedMainHandAccuracyVsEnemy: Option<f64>,
        storedSustainedTraumaBreakdown: Table,
        quantityMultiplier: f64,
        ailmentData: Option<&'static indexmap::IndexMap<Name, pob_gamedata::AilmentData>>,
    }
}

crate::ctx_struct! {
    /// The locals of the hit pass loop body (2606-3955) that cross its split at 3346.
    pub struct HitPassLocals {
        pass: Table,
        source: Table,
        output: Output,
        cfg: Cfg,
        breakdown: Option<Breakdown>,
        hitRate: Option<f64>,
    }
}

crate::ctx_struct! {
    /// The locals of the ailment pass loop body (4164-5648) that cross its split at 4990.
    /// The loop-local functions `checkWeapon1HFlags`, `calcAverageSourceDamage` and
    /// `calcAilmentDamage` (OF-G) take these locals explicitly.
    pub struct AilmentPassLocals {
        pass: Table,
        output: Output,
        cfg: Cfg,
        breakdown: Option<Breakdown>,
        ailmentMode: Option<Name>,
    }
}

impl OffenceCtx {
    /// CalcOffence.lua 349-361 (exact alias reads).
    pub fn init(env: &EnvRef, actor: &ActorRef, activeSkill: &ActiveSkillRef) -> OffenceCtx {
        // L350
        let modDB = actor.modDB.get();
        // L352
        let enemyDB = match actor.enemy() {
            Some(e) => e.modDB.get(),
            None => lua_error!("attempt to index field 'enemy' (a nil value)"),
        };
        // L354
        let output = actor.output.get();
        // L356
        let breakdown = actor.breakdown.get();
        // L358-361
        let skillModList = activeSkill.skillModList.get();
        let skillData = activeSkill.skillData.get();
        let skillFlags = activeSkill.skillFlags.get();
        let skillCfg = activeSkill.skillCfg.get();
        OffenceCtx {
            env: env.clone(),
            actor: actor.clone(),
            activeSkill: activeSkill.clone(),
            modDB,
            enemyDB,
            output,
            breakdown,
            skillModList,
            skillData,
            skillFlags,
            skillCfg,
            isAttack: false,
            debuffDurationMult: 0.0,
            costs: Table::new(),
            monsterLife: 0.0,
            canDeal: Table::new(),
            passList: Shared::new(Vec::new()),
            storedMainHandAccuracy: None,
            storedMainHandAccuracyVsEnemy: None,
            storedSustainedTraumaBreakdown: Table::new(),
            quantityMultiplier: 0.0,
            ailmentData: None,
        }
    }
}

impl HitPassLocals {
    /// The loop variable; the body (OF-E, 2607-2613) assigns the rest.
    pub fn new(pass: Table) -> Self {
        HitPassLocals {
            pass,
            source: Table::new(),
            output: Table::new(),
            cfg: ModCfg::new(),
            breakdown: None,
            hitRate: None,
        }
    }
}

impl AilmentPassLocals {
    /// The loop variable; the body (OF-G, 4165-4171) assigns the rest.
    pub fn new(pass: Table) -> Self {
        AilmentPassLocals {
            pass,
            output: Table::new(),
            cfg: ModCfg::new(),
            breakdown: None,
            ailmentMode: None,
        }
    }
}

/// `calcs.offence(env, actor, activeSkill)`.
/// @pob Modules/CalcOffence.lua:348-361 h=9365b42f
pub fn offence(env: &EnvRef, actor: &ActorRef, activeSkill: &ActiveSkillRef) {
    let mut ctx = OffenceCtx::init(env, actor, activeSkill); // L349-361
    seg!(helpers_setup::seg_setup(&mut ctx)); // OF-A 362-1036 (incl. return at 371)
    seg!(skill_types::seg_skillTypeStats(&mut ctx)); // OF-B 1037-1691
    seg!(costs_passes::seg_costsAndPasses(&mut ctx)); // OF-C 1692-2173
    seg!(speed::seg_speed(&mut ctx)); // OF-D 2174-2605 (sets globalOutput at 2176)

    // `for _, pass in ipairs(passList) do` (the hit pass loop header; `passList` is read live)
    // @pob Modules/CalcOffence.lua:2606-2606 h=a7253828
    let passList = ctx.passList.clone();
    for_ipairs!((_, pass) in passList => {
        let mut hp = HitPassLocals::new(table_or_raise(pass));
        seg!(crit::seg_passCrit(&mut ctx, &mut hp)); // OF-E 2607-3345 (sets globalOutput at 2607)
        seg!(hit_dps::seg_passHit(&mut ctx, &mut hp)); // OF-F 3346-3955
    });
    seg!(hit_dps::seg_combinePasses(&mut ctx)); // OF-F 3956-4154
    seg!(ailments_a::seg_ailmentPrelude(&mut ctx)); // OF-G 4155-4163

    // `for _, pass in ipairs(passList) do` (the ailment loop header)
    // @pob Modules/CalcOffence.lua:4164-4164 h=a7253828
    let passList = ctx.passList.clone();
    for_ipairs!((_, pass) in passList => {
        let mut al = AilmentPassLocals::new(table_or_raise(pass));
        seg!(ailments_a::seg_bleedPoison(&mut ctx, &mut al)); // OF-G 4165-4989 (sets globalOutput at 4165)
        seg!(ailments_b::seg_igniteNonDmgImpale(&mut ctx, &mut al)); // OF-H 4990-5648
    });
    seg!(dot_combined::seg_dotCombined(&mut ctx)); // OF-I 5649-6263
}

/// A pass entry is always a table in PoB.
fn table_or_raise(pass: Table) -> Table {
    pass
}
