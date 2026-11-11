//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence pass loop body A, OF-E (CalcOffence.lua 2607-3345): exerted attacks (warcries),
//! pacts, Ruthless / Fist of War, Explosive Arrow, crit chance / multiplier / effect,
//! double / triple damage, hit rate, culling, reservation DPS, `postCritFunc` and the
//! Cryogenesis added damage redirection.
//!
//! Owner: OF-E.
#![allow(non_snake_case)]
// literal Lua shapes (`local x = 0` then assignment, `not a and not b`, `x * 2 / 3`)
#![allow(
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::float_cmp,
    clippy::assign_op_pattern
)]

use super::helpers_setup::{
    calcAreaOfEffect, calcSkillCooldown, calcSkillDuration, calcWarcryCastTime, runSkillFunc,
};
use super::{HitPassLocals, OffenceCtx};
use crate::calcs_facade::calcs;
use crate::data_fns::{skillFunc, SkillFuncImpl, SkillHook};
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;

/// `dmgTypeList` (CalcOffence.lua:34, a module-level constant of OF-A's range).
const dmgTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// `activeSkill.skillTypes[t]` (truthiness).
fn has_type(skill: &ActiveSkillRef, t: SkillType::T) -> bool {
    skill.skillTypes.get().with(|s| s.has(t))
}

/// `t_insert(tbl[k], v)`: the field must hold a table (Lua raises on nil).
fn tins(tbl: &Table, k: Name, v: impl Into<Val>) {
    match tbl.get(k) {
        Val::Table(t) => t.push(v),
        other => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// Lua `a == b` for a number `a` and any value `b` (a number only equals a number).
fn num_eq(a: f64, b: &Val) -> bool {
    match b {
        Val::Num(x) => a == *x,
        _ => false,
    }
}

/// A pass entry's `breakdown` field: `breakdown and breakdown.MainHand` (a breakdown
/// instance, or nil / false).
fn breakdown_of(v: Val) -> Option<Breakdown> {
    match v {
        Val::Nil | Val::Bool(false) => None,
        Val::Table(t) => Some(Breakdown {
            t,
            ..Breakdown::default()
        }),
        other => match other.obj::<Breakdown>() {
            Some(b) => Some((*b).clone()),
            None => lua_error!("pass.breakdown is a {}", other.lua_type()),
        },
    }
}

/// A pass entry's `cfg` field (`activeSkill.weapon1Cfg`, `weapon2Cfg` or `skillCfg`).
fn cfg_of(v: Val) -> Cfg {
    match v.obj::<ModCfg>() {
        Some(c) => c,
        None => lua_error!("pass.cfg is a {}", v.lua_type()),
    }
}

/// `actor.parent` (nil raises when indexed).
fn parent_of(actor: &ActorRef) -> ActorRef {
    match actor.parent() {
        Some(p) => p,
        None => lua_error!("attempt to index field 'parent' (a nil value)"),
    }
}

/// `actor.partyMembers` (nil raises when indexed).
fn party_of(actor: &ActorRef) -> ActorRef {
    match actor.partyMembers.get().upgrade() {
        Some(p) => p,
        None => lua_error!("attempt to index field 'partyMembers' (a nil value)"),
    }
}

/// A mod's `source` for `mod.source:match(...)` (nil raises).
fn mod_source(m: &Mod) -> &str {
    match m.source.as_deref() {
        Some(s) => s,
        None => lua_error!("attempt to index field 'source' (a nil value)"),
    }
}

/// A mod's numeric `value` in arithmetic (non-numbers raise).
fn mod_value_num(m: &Mod) -> f64 {
    match m.value.num() {
        Some(v) => v,
        None => lua_error!(
            "attempt to perform arithmetic on field 'value' (a {} value)",
            m.value.lua_type()
        ),
    }
}

/// @pob Modules/CalcOffence.lua:2607-3345 h=6f0af00d
/// OF-E: CalcOffence.lua 2607-3345 (sets `globalOutput` at 2607).
pub(crate) fn seg_passCrit(ctx: &mut OffenceCtx, hp: &mut HitPassLocals) -> Flow {
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let pass = hp.pass.clone();

    // L2607
    let globalOutput: Output = ctx.output.clone();
    let globalBreakdown: Option<Breakdown> = ctx.breakdown.clone();
    OFFENCE_STATE.with(|s| {
        s.globalOutput.set(Some(globalOutput.clone()));
        s.globalBreakdown.set(globalBreakdown.clone());
    });
    // L2608
    hp.source = match pass.get(n!("source")) {
        Val::Table(t) => t,
        other => lua_error!("pass.source is a {}", other.lua_type()),
    };
    // ---@class Output
    // L2610
    hp.output = match pass.get(n!("output")) {
        Val::Table(t) => t,
        other => lua_error!("pass.output is a {}", other.lua_type()),
    };
    // L2611
    hp.cfg = cfg_of(pass.get(n!("cfg")));
    // ---@class Breakdown
    // L2613
    hp.breakdown = breakdown_of(pass.get(n!("breakdown")));
    let source = hp.source.clone();
    let output = hp.output.clone();
    let cfg = hp.cfg.clone();
    let breakdown = hp.breakdown.clone();

    // Exerted Attack members
    // L2616
    let exertedDoubleDamage =
        env.modDB
            .get()
            .Sum(ModType::BASE, Some(&*cfg), nm!["ExertDoubleDamageChance"]);
    // L2617
    let exertingWarcryCount =
        env.modDB
            .get()
            .Sum(ModType::BASE, None, nm!["Multiplier:ExertingWarcryCount"]);
    // L2618
    globalOutput.set(n!("OffensiveWarcryEffect"), 1.0);
    // L2619
    globalOutput.set(n!("MaxOffensiveWarcryEffect"), 1.0);
    // L2620
    globalOutput.set(n!("TheoreticalOffensiveWarcryEffect"), 1.0);
    // L2621
    globalOutput.set(n!("TheoreticalMaxOffensiveWarcryEffect"), 1.0);
    // L2622
    globalOutput.set(n!("RallyingHitEffect"), 1.0);
    // L2623
    globalOutput.set(n!("AilmentWarcryEffect"), 1.0);
    // L2624
    globalOutput.set(n!("GlobalWarcryUptimeRatio"), 0.0);

    // L2626
    if env.mode_buffs.get() {
        // Iterative over all the active skills to account for exerted attacks provided by warcries
        // L2628
        if !has_type(&activeSkill, SkillType::NeverExertable)
            && !has_type(&activeSkill, SkillType::Triggered)
            && !has_type(&activeSkill, SkillType::Channel)
            && !has_type(&activeSkill, SkillType::OtherThingUsesSkill)
            && !has_type(&activeSkill, SkillType::Retaliation)
            && !has_type(&activeSkill, SkillType::SummonsTotem)
        {
            // L2630
            let activeSkillList = actor.activeSkillList.get();
            for_ipairs!((index, value) in activeSkillList => {
                let valueName = value.activeEffect.get().grantedEffect.get().name;
                let valueDisabled = || value.skillFlags.get().truthy(n!("disable"));
                // L2631
                if valueName.as_str() == "Ancestral Cry"
                    && has_type(&activeSkill, SkillType::MeleeSingleTarget)
                    && !globalOutput.truthy(n!("AncestralCryCalculated"))
                    && !valueDisabled()
                {
                    // L2632
                    let v = calcSkillDuration(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), Some(&env), Some(&enemyDB));
                    globalOutput.set(n!("AncestralCryDuration"), v);
                    // L2633
                    let v = calcSkillCooldown(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get()).0;
                    globalOutput.set(n!("AncestralCryCooldown"), v);
                    // L2634
                    let v = calcWarcryCastTime(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), &actor);
                    globalOutput.set(n!("AncestralCryCastTime"), v);
                    // L2635 (`Sum(...) or 0`: Sum always returns a number)
                    let v = env.modDB.get().Sum(ModType::BASE, None, nm!["NumAncestralExerts"]);
                    globalOutput.set(n!("AncestralExertsCount"), v);
                    // L2636
                    let baseUptimeRatio = m_min((globalOutput.num(n!("AncestralExertsCount")) / globalOutput.num(n!("Speed"))) / (globalOutput.num(n!("AncestralCryCooldown")) + globalOutput.num(n!("AncestralCryCastTime"))), 1.0) * 100.0;
                    // L2637
                    let storedUses = value.skillData.get().num(n!("storedUses")) + value.skillModList.get().Sum(ModType::BASE, Some(&*value.skillCfg.get()), nm!["AdditionalCooldownUses"]);
                    // L2638
                    globalOutput.set(n!("AncestralUpTimeRatio"), m_min(100.0, baseUptimeRatio * storedUses));
                    // L2639
                    let v = globalOutput.num(n!("GlobalWarcryUptimeRatio")) + globalOutput.num(n!("AncestralUpTimeRatio"));
                    globalOutput.set(n!("GlobalWarcryUptimeRatio"), v);
                    // L2640
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2641
                        globalBreakdown.set(n!("AncestralUpTimeRatio"), Table::new());
                        // L2642
                        tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("(%d ^8(number of exerts)", globalOutput.num(n!("AncestralExertsCount"))));
                        // L2643
                        tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("/ %.2f) ^8(attacks per second)", globalOutput.num(n!("Speed"))));
                        // L2644
                        if globalOutput.num(n!("AncestralCryCastTime")) > 0.0 {
                            // L2645
                            tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("/ (%.2f ^8(warcry cooldown)", globalOutput.num(n!("AncestralCryCooldown"))));
                            // L2646
                            tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("+ %.2f) ^8(warcry casttime)", globalOutput.num(n!("AncestralCryCastTime"))));
                        } else {
                            // L2648
                            tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("/ %.2f ^8(average warcry cooldown)", globalOutput.num(n!("AncestralCryCooldown"))));
                        }
                        // L2650
                        tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("* %d ^8(stored uses)", storedUses));
                        // L2651
                        tins(globalBreakdown, n!("AncestralUpTimeRatio"), lfmt!("= %d%%", globalOutput.num(n!("AncestralUpTimeRatio"))));
                    }
                    // L2653
                    globalOutput.set(n!("AncestralCryCalculated"), true);
                } else if valueName.as_str() == "Infernal Cry"
                    && !globalOutput.truthy(n!("InfernalCryCalculated"))
                    && !valueDisabled()
                {
                    // L2654 (elseif)
                    // L2655
                    let v = calcSkillDuration(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), Some(&env), Some(&enemyDB));
                    globalOutput.set(n!("InfernalCryDuration"), v);
                    // L2656
                    let v = calcSkillCooldown(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get()).0;
                    globalOutput.set(n!("InfernalCryCooldown"), v);
                    // L2657
                    let v = calcWarcryCastTime(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), &actor);
                    globalOutput.set(n!("InfernalCryCastTime"), v);
                    // L2658
                    if has_type(&activeSkill, SkillType::Melee) {
                        // L2659 (`Sum(...) or 0`: Sum always returns a number)
                        let v = env.modDB.get().Sum(ModType::BASE, None, nm!["NumInfernalExerts"]);
                        globalOutput.set(n!("InfernalExertsCount"), v);
                        // L2660
                        let baseUptimeRatio = m_min((globalOutput.num(n!("InfernalExertsCount")) / globalOutput.num(n!("Speed"))) / (globalOutput.num(n!("InfernalCryCooldown")) + globalOutput.num(n!("InfernalCryCastTime"))), 1.0) * 100.0;
                        // L2661
                        let storedUses = value.skillData.get().num(n!("storedUses")) + value.skillModList.get().Sum(ModType::BASE, Some(&*value.skillCfg.get()), nm!["AdditionalCooldownUses"]);
                        // L2662
                        globalOutput.set(n!("InfernalUpTimeRatio"), m_min(100.0, baseUptimeRatio * storedUses));
                        // L2663
                        let v = globalOutput.num(n!("GlobalWarcryUptimeRatio")) + globalOutput.num(n!("InfernalUpTimeRatio"));
                        globalOutput.set(n!("GlobalWarcryUptimeRatio"), v);
                        // L2664
                        if let Some(globalBreakdown) = &globalBreakdown {
                            // L2665
                            globalBreakdown.set(n!("InfernalUpTimeRatio"), Table::new());
                            // L2666
                            tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("(%d ^8(number of exerts)", globalOutput.num(n!("InfernalExertsCount"))));
                            // L2667
                            tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("/ %.2f) ^8(attacks per second)", globalOutput.num(n!("Speed"))));
                            // L2668
                            if globalOutput.num(n!("InfernalCryCastTime")) > 0.0 {
                                // L2669
                                tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("/ (%.2f ^8(warcry cooldown)", globalOutput.num(n!("InfernalCryCooldown"))));
                                // L2670
                                tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("+ %.2f) ^8(warcry casttime)", globalOutput.num(n!("InfernalCryCastTime"))));
                            } else {
                                // L2672
                                tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("/ %.2f ^8(average warcry cooldown)", globalOutput.num(n!("InfernalCryCooldown"))));
                            }
                            // L2674
                            tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("* %d ^8(stored uses)", storedUses));
                            // L2675
                            tins(globalBreakdown, n!("InfernalUpTimeRatio"), lfmt!("= %d%%", globalOutput.num(n!("InfernalUpTimeRatio"))));
                        }
                    }
                    // L2678
                    globalOutput.set(n!("InfernalCryCalculated"), true);
                } else if valueName.as_str() == "Intimidating Cry"
                    && has_type(&activeSkill, SkillType::Melee)
                    && !globalOutput.truthy(n!("IntimidatingCryCalculated"))
                    && !valueDisabled()
                {
                    // L2679 (elseif)
                    // L2680
                    globalOutput.set(n!("CreateWarcryOffensiveCalcSection"), true);
                    // L2681
                    let v = calcSkillDuration(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), Some(&env), Some(&enemyDB));
                    globalOutput.set(n!("IntimidatingCryDuration"), v);
                    // L2682
                    let v = calcSkillCooldown(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get()).0;
                    globalOutput.set(n!("IntimidatingCryCooldown"), v);
                    // L2683
                    let v = calcWarcryCastTime(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), &actor);
                    globalOutput.set(n!("IntimidatingCryCastTime"), v);
                    // L2684 (`Sum(...) or 0`: Sum always returns a number)
                    let v = env.modDB.get().Sum(ModType::BASE, None, nm!["NumIntimidatingExerts"]);
                    globalOutput.set(n!("IntimidatingExertsCount"), v);
                    // L2685
                    let baseUptimeRatio = m_min((globalOutput.num(n!("IntimidatingExertsCount")) / globalOutput.num(n!("Speed"))) / (globalOutput.num(n!("IntimidatingCryCooldown")) + globalOutput.num(n!("IntimidatingCryCastTime"))), 1.0) * 100.0;
                    // L2686
                    let storedUses = value.skillData.get().num(n!("storedUses")) + value.skillModList.get().Sum(ModType::BASE, Some(&*value.skillCfg.get()), nm!["AdditionalCooldownUses"]);
                    // L2687
                    globalOutput.set(n!("IntimidatingUpTimeRatio"), m_min(100.0, baseUptimeRatio * storedUses));
                    // L2688
                    let v = globalOutput.num(n!("GlobalWarcryUptimeRatio")) + globalOutput.num(n!("IntimidatingUpTimeRatio"));
                    globalOutput.set(n!("GlobalWarcryUptimeRatio"), v);
                    // L2689
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2690
                        globalBreakdown.set(n!("IntimidatingUpTimeRatio"), Table::new());
                        // L2691
                        tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("(%d ^8(number of exerts)", globalOutput.num(n!("IntimidatingExertsCount"))));
                        // L2692
                        tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("/ %.2f) ^8(attacks per second)", globalOutput.num(n!("Speed"))));
                        // L2693
                        if globalOutput.num(n!("IntimidatingCryCastTime")) > 0.0 {
                            // L2694
                            tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("/ (%.2f ^8(warcry cooldown)", globalOutput.num(n!("IntimidatingCryCooldown"))));
                            // L2695
                            tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("+ %.2f) ^8(warcry casttime)", globalOutput.num(n!("IntimidatingCryCastTime"))));
                        } else {
                            // L2697
                            tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("/ %.2f ^8(average warcry cooldown)", globalOutput.num(n!("IntimidatingCryCooldown"))));
                        }
                        // L2699
                        tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("* %d ^8(stored uses)", storedUses));
                        // L2700
                        tins(globalBreakdown, n!("IntimidatingUpTimeRatio"), lfmt!("= %d%%", globalOutput.num(n!("IntimidatingUpTimeRatio"))));
                    }
                    // L2702
                    let ddChance = m_min(
                        skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["DoubleDamageChance"])
                            + (if env.mode_effective.get() { enemyDB.Sum(ModType::BASE, Some(&*cfg), nm!["SelfDoubleDamageChance"]) } else { 0.0 })
                            + exertedDoubleDamage,
                        100.0,
                    );
                    // L2703
                    globalOutput.set(n!("IntimidatingAvgDmg"), 2.0 * (1.0 - ddChance / 100.0)); // 1
                    // L2704
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2705
                        globalBreakdown.set(n!("IntimidatingAvgDmg"), tbl![
                            "Average Intimidating Cry Damage:",
                            lfmt!("%.2f%% ^8(base double damage increase to hit 100%%)", (1.0 - ddChance / 100.0) * 100.0),
                            lfmt!("x %d ^8(double damage multiplier)", 2.0),
                            lfmt!("= %.2f", globalOutput.num(n!("IntimidatingAvgDmg"))),
                        ]);
                    }
                    // L2712
                    let v = 1.0 + globalOutput.num(n!("IntimidatingAvgDmg")) * globalOutput.num(n!("IntimidatingUpTimeRatio")) / 100.0;
                    globalOutput.set(n!("IntimidatingHitEffect"), v);
                    // L2713
                    let v = 1.0 + globalOutput.num(n!("IntimidatingAvgDmg"));
                    globalOutput.set(n!("IntimidatingMaxHitEffect"), v);
                    // L2714
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2715
                        globalBreakdown.set(n!("IntimidatingHitEffect"), tbl![
                            lfmt!("1 + (%.2f ^8(average exerted damage)", globalOutput.num(n!("IntimidatingAvgDmg"))),
                            lfmt!("x %.2f) ^8(uptime %%)", globalOutput.num(n!("IntimidatingUpTimeRatio")) / 100.0),
                            lfmt!("= %.2f", globalOutput.num(n!("IntimidatingHitEffect"))),
                        ]);
                    }

                    // L2722
                    let v = globalOutput.num(n!("TheoreticalOffensiveWarcryEffect")) * globalOutput.num(n!("IntimidatingHitEffect"));
                    globalOutput.set(n!("TheoreticalOffensiveWarcryEffect"), v);
                    // L2723
                    let v = globalOutput.num(n!("TheoreticalMaxOffensiveWarcryEffect")) * globalOutput.num(n!("IntimidatingMaxHitEffect"));
                    globalOutput.set(n!("TheoreticalMaxOffensiveWarcryEffect"), v);
                    // L2724
                    globalOutput.set(n!("IntimidatingCryCalculated"), true);
                } else if valueName.as_str() == "Rallying Cry"
                    && has_type(&activeSkill, SkillType::Melee)
                    && !globalOutput.truthy(n!("RallyingCryCalculated"))
                    && !valueDisabled()
                {
                    // L2725 (elseif)
                    // L2726
                    globalOutput.set(n!("CreateWarcryOffensiveCalcSection"), true);
                    // L2727
                    let v = calcSkillDuration(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), Some(&env), Some(&enemyDB));
                    globalOutput.set(n!("RallyingCryDuration"), v);
                    // L2728
                    let v = calcSkillCooldown(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get()).0;
                    globalOutput.set(n!("RallyingCryCooldown"), v);
                    // L2729
                    let v = calcWarcryCastTime(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), &actor);
                    globalOutput.set(n!("RallyingCryCastTime"), v);
                    // L2730 (`Sum(...) or 0`: Sum always returns a number)
                    let v = env.modDB.get().Sum(ModType::BASE, None, nm!["NumRallyingExerts"]);
                    globalOutput.set(n!("RallyingExertsCount"), v);
                    // L2731
                    let baseUptimeRatio = m_min((globalOutput.num(n!("RallyingExertsCount")) / globalOutput.num(n!("Speed"))) / (globalOutput.num(n!("RallyingCryCooldown")) + globalOutput.num(n!("RallyingCryCastTime"))), 1.0) * 100.0;
                    // L2732
                    let storedUses = value.skillData.get().num(n!("storedUses")) + value.skillModList.get().Sum(ModType::BASE, Some(&*value.skillCfg.get()), nm!["AdditionalCooldownUses"]);
                    // L2733
                    globalOutput.set(n!("RallyingUpTimeRatio"), m_min(100.0, baseUptimeRatio * storedUses));
                    // L2734
                    let v = globalOutput.num(n!("GlobalWarcryUptimeRatio")) + globalOutput.num(n!("RallyingUpTimeRatio"));
                    globalOutput.set(n!("GlobalWarcryUptimeRatio"), v);
                    // L2735
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2736
                        globalBreakdown.set(n!("RallyingUpTimeRatio"), Table::new());
                        // L2737
                        tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("(%d ^8(number of exerts)", globalOutput.num(n!("RallyingExertsCount"))));
                        // L2738
                        tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("/ %.2f) ^8(attacks per second)", globalOutput.num(n!("Speed"))));
                        // L2739
                        if globalOutput.num(n!("RallyingCryCastTime")) > 0.0 {
                            // L2740
                            tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("/ (%.2f ^8(warcry cooldown)", globalOutput.num(n!("RallyingCryCooldown"))));
                            // L2741
                            tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("+ %.2f) ^8(warcry casttime)", globalOutput.num(n!("RallyingCryCastTime"))));
                        } else {
                            // L2743
                            tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("/ %.2f ^8(average warcry cooldown)", globalOutput.num(n!("RallyingCryCooldown"))));
                        }
                        // L2745
                        tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("* %d ^8(stored uses)", storedUses));
                        // L2746
                        tins(globalBreakdown, n!("RallyingUpTimeRatio"), lfmt!("= %d%%", globalOutput.num(n!("RallyingUpTimeRatio"))));
                    }
                    // L2748
                    let v = m_min(env.modDB.get().Sum(ModType::BASE, Some(&*cfg), nm!["Multiplier:NearbyAlly"]), 5.0)
                        * (env.modDB.get().Sum(ModType::BASE, None, nm!["RallyingExertMoreDamagePerAlly"]) / 100.0);
                    globalOutput.set(n!("RallyingAvgDmg"), v);
                    // L2749
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2750
                        globalBreakdown.set(n!("RallyingAvgDmg"), tbl![
                            "Average Rallying Cry Damage:",
                            lfmt!("%.2f ^8(average damage multiplier per ally)", env.modDB.get().Sum(ModType::BASE, None, nm!["RallyingExertMoreDamagePerAlly"]) / 100.0),
                            lfmt!("x %d ^8(number of nearby allies (max=5))", m_min(env.modDB.get().Sum(ModType::BASE, Some(&*cfg), nm!["Multiplier:NearbyAlly"]), 5.0)),
                            lfmt!("= %.2f", globalOutput.num(n!("RallyingAvgDmg"))),
                        ]);
                    }
                    // L2757
                    let v = 1.0 + globalOutput.num(n!("RallyingAvgDmg")) * globalOutput.num(n!("RallyingUpTimeRatio")) / 100.0;
                    globalOutput.set(n!("RallyingHitEffect"), v);
                    // L2758
                    let v = 1.0 + globalOutput.num(n!("RallyingAvgDmg"));
                    globalOutput.set(n!("RallyingMaxHitEffect"), v);
                    // L2759
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2760
                        globalBreakdown.set(n!("RallyingHitEffect"), tbl![
                            lfmt!("1 + (%.2f ^8(average exerted damage)", globalOutput.num(n!("RallyingAvgDmg"))),
                            lfmt!("x %.2f) ^8(uptime %%)", globalOutput.num(n!("RallyingUpTimeRatio")) / 100.0),
                            lfmt!("= %.2f", globalOutput.num(n!("RallyingHitEffect"))),
                        ]);
                    }
                    // L2766
                    let v = globalOutput.num(n!("OffensiveWarcryEffect")) * globalOutput.num(n!("RallyingHitEffect"));
                    globalOutput.set(n!("OffensiveWarcryEffect"), v);
                    // L2767
                    let v = globalOutput.num(n!("MaxOffensiveWarcryEffect")) * globalOutput.num(n!("RallyingMaxHitEffect"));
                    globalOutput.set(n!("MaxOffensiveWarcryEffect"), v);
                    // L2768
                    let v = globalOutput.num(n!("TheoreticalOffensiveWarcryEffect")) * globalOutput.num(n!("RallyingHitEffect"));
                    globalOutput.set(n!("TheoreticalOffensiveWarcryEffect"), v);
                    // L2769
                    let v = globalOutput.num(n!("TheoreticalMaxOffensiveWarcryEffect")) * globalOutput.num(n!("RallyingMaxHitEffect"));
                    globalOutput.set(n!("TheoreticalMaxOffensiveWarcryEffect"), v);
                    // L2770
                    globalOutput.set(n!("RallyingCryCalculated"), true);
                } else if valueName.as_str() == "Seismic Cry"
                    && has_type(&activeSkill, SkillType::Slam)
                    && !globalOutput.truthy(n!("SeismicCryCalculated"))
                    && !valueDisabled()
                {
                    // L2772 (elseif)
                    // L2773
                    globalOutput.set(n!("CreateWarcryOffensiveCalcSection"), true);
                    // L2774
                    let v = calcSkillDuration(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), Some(&env), Some(&enemyDB));
                    globalOutput.set(n!("SeismicCryDuration"), v);
                    // L2775
                    let v = calcSkillCooldown(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get()).0;
                    globalOutput.set(n!("SeismicCryCooldown"), v);
                    // L2776
                    let v = calcWarcryCastTime(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), &actor);
                    globalOutput.set(n!("SeismicCryCastTime"), v);
                    // L2777 (`Sum(...) or 0`: Sum always returns a number)
                    let v = env.modDB.get().Sum(ModType::BASE, None, nm!["NumSeismicExerts"]);
                    globalOutput.set(n!("SeismicExertsCount"), v);
                    // L2778
                    let baseUptimeRatio = m_min((globalOutput.num(n!("SeismicExertsCount")) / globalOutput.num(n!("Speed"))) / (globalOutput.num(n!("SeismicCryCooldown")) + globalOutput.num(n!("SeismicCryCastTime"))), 1.0) * 100.0;
                    // L2779
                    let storedUses = value.skillData.get().num(n!("storedUses")) + value.skillModList.get().Sum(ModType::BASE, Some(&*value.skillCfg.get()), nm!["AdditionalCooldownUses"]);
                    // L2780
                    globalOutput.set(n!("SeismicUpTimeRatio"), m_min(100.0, baseUptimeRatio * storedUses));
                    // L2781
                    let v = globalOutput.num(n!("GlobalWarcryUptimeRatio")) + globalOutput.num(n!("SeismicUpTimeRatio"));
                    globalOutput.set(n!("GlobalWarcryUptimeRatio"), v);
                    // account for AoE increase
                    // L2783
                    if activeSkill.skillModList.get().Flag(None, nm!["Condition:WarcryMaxHit"]) {
                        // L2784
                        let v = env.modDB.get().Sum(ModType::BASE, None, nm!["SeismicMoreAoE"]);
                        skillModList.NewMod(n!("AreaOfEffect"), ModType::MORE, v, "Max Seismic Exert AoE", 0, 0, &[]);
                    } else {
                        // L2786
                        let v = m_floor(env.modDB.get().Sum(ModType::BASE, None, nm!["SeismicMoreAoE"]) / 100.0 * globalOutput.num(n!("SeismicUpTimeRatio")));
                        skillModList.NewMod(n!("AreaOfEffect"), ModType::MORE, v, "Avg Seismic Exert AoE", 0, 0, &[]);
                    }
                    // L2788
                    calcAreaOfEffect(ctx, &skillModList, &skillCfg, &skillData, &skillFlags, &globalOutput, globalBreakdown.as_ref());
                    // L2789
                    if let Some(globalBreakdown) = &globalBreakdown {
                        // L2790
                        globalBreakdown.set(n!("SeismicUpTimeRatio"), Table::new());
                        // L2791
                        tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("(%d ^8(number of exerts)", globalOutput.num(n!("SeismicExertsCount"))));
                        // L2792
                        tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("/ %.2f) ^8(attacks per second)", globalOutput.num(n!("Speed"))));
                        // L2793
                        if globalOutput.num(n!("SeismicCryCastTime")) > 0.0 {
                            // L2794
                            tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("/ (%.2f ^8(warcry cooldown)", globalOutput.num(n!("SeismicCryCooldown"))));
                            // L2795
                            tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("+ %.2f) ^8(warcry casttime)", globalOutput.num(n!("SeismicCryCastTime"))));
                        } else {
                            // L2797
                            tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("/ %.2f ^8(average warcry cooldown)", globalOutput.num(n!("SeismicCryCooldown"))));
                        }
                        // L2799
                        tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("* %d ^8(stored uses)", storedUses));
                        // L2800
                        tins(globalBreakdown, n!("SeismicUpTimeRatio"), lfmt!("= %d%%", globalOutput.num(n!("SeismicUpTimeRatio"))));
                    }
                    // L2802
                    globalOutput.set(n!("SeismicCryCalculated"), true);
                } else if valueName.as_str() == "Battlemage's Cry"
                    && !globalOutput.truthy(n!("BattleMageCryCalculated"))
                    && !valueDisabled()
                {
                    // L2803 (elseif)
                    // L2804
                    let v = calcSkillDuration(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), Some(&env), Some(&enemyDB));
                    globalOutput.set(n!("BattleMageCryDuration"), v);
                    // L2805
                    let v = calcSkillCooldown(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get()).0;
                    globalOutput.set(n!("BattleMageCryCooldown"), v);
                    // L2806
                    let v = calcWarcryCastTime(&value.skillModList.get(), Some(&*value.skillCfg.get()), &value.skillData.get(), &actor);
                    globalOutput.set(n!("BattleMageCryCastTime"), v);
                    // L2807
                    if has_type(&activeSkill, SkillType::Melee) {
                        // L2808 (`Sum(...) or 0`: Sum always returns a number)
                        let v = env.modDB.get().Sum(ModType::BASE, None, nm!["NumBattlemageExerts"]);
                        globalOutput.set(n!("BattleCryExertsCount"), v);
                        // L2809
                        let baseUptimeRatio = m_min((globalOutput.num(n!("BattleCryExertsCount")) / globalOutput.num(n!("Speed"))) / (globalOutput.num(n!("BattleMageCryCooldown")) + globalOutput.num(n!("BattleMageCryCastTime"))), 1.0) * 100.0;
                        // L2810
                        let storedUses = value.skillData.get().num(n!("storedUses")) + value.skillModList.get().Sum(ModType::BASE, Some(&*value.skillCfg.get()), nm!["AdditionalCooldownUses"]);
                        // L2811
                        globalOutput.set(n!("BattlemageUpTimeRatio"), m_min(100.0, baseUptimeRatio * storedUses));
                        // L2812
                        let v = globalOutput.num(n!("GlobalWarcryUptimeRatio")) + globalOutput.num(n!("BattlemageUpTimeRatio"));
                        globalOutput.set(n!("GlobalWarcryUptimeRatio"), v);
                        // L2813
                        if let Some(globalBreakdown) = &globalBreakdown {
                            // L2814
                            globalBreakdown.set(n!("BattlemageUpTimeRatio"), Table::new());
                            // L2815
                            tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("(%d ^8(number of exerts)", globalOutput.num(n!("BattleCryExertsCount"))));
                            // L2816
                            tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("/ %.2f) ^8(attacks per second)", globalOutput.num(n!("Speed"))));
                            // L2817
                            if globalOutput.num(n!("BattleMageCryCastTime")) > 0.0 {
                                // L2818
                                tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("/ (%.2f ^8(warcry cooldown)", globalOutput.num(n!("BattleMageCryCooldown"))));
                                // L2819
                                tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("+ %.2f) ^8(warcry casttime)", globalOutput.num(n!("BattleMageCryCastTime"))));
                            } else {
                                // L2821
                                tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("/ %.2f ^8(average warcry cooldown)", globalOutput.num(n!("BattleMageCryCooldown"))));
                            }
                            // L2823
                            tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("* %d ^8(stored uses)", storedUses));
                            // L2824
                            tins(globalBreakdown, n!("BattlemageUpTimeRatio"), lfmt!("= %d%%", globalOutput.num(n!("BattlemageUpTimeRatio"))));
                        }
                    }
                    // L2827
                    globalOutput.set(n!("BattleMageCryCalculated"), true);
                }
            });

            // L2831
            if activeSkill
                .skillModList
                .get()
                .Flag(None, nm!["Condition:WarcryMaxHit"])
            {
                // L2832
                globalOutput.set(
                    n!("AilmentWarcryEffect"),
                    globalOutput.get(n!("MaxOffensiveWarcryEffect")),
                );
                // L2833
                skillData.set(n!("showAverage"), true);
                // L2834
                skillFlags.set(n!("showAverage"), true);
                // L2835
                skillFlags.set(n!("notAverage"), false);
            } else {
                // L2837
                globalOutput.set(
                    n!("AilmentWarcryEffect"),
                    globalOutput.get(n!("OffensiveWarcryEffect")),
                );
            }

            // Calculate Exerted Attack Uptime
            // There are various strategies a player could use to maximize either warcry effect stacking or staggering
            // 1) they don't pay attention and therefore we calculated exerted attack uptime as just the maximum uptime of any enabled warcries that exert attacks
            // L2843
            let warcryList: &[Name] = &[
                n!("AncestralUpTimeRatio"),
                n!("InfernalUpTimeRatio"),
                n!("IntimidatingUpTimeRatio"),
                n!("RallyingUpTimeRatio"),
                n!("SeismicUpTimeRatio"),
                n!("BattlemageUpTimeRatio"),
            ];
            // L2844
            for_ipairs!((_, cryTimeRatio) in warcryList => {
                // L2845
                let v = m_max(globalOutput.num0(n!("ExertedAttackUptimeRatio")), globalOutput.num0(cryTimeRatio));
                globalOutput.set(n!("ExertedAttackUptimeRatio"), v);
            });
            // L2847
            if let Some(globalBreakdown) = &globalBreakdown {
                // L2848
                globalBreakdown.set(n!("ExertedAttackUptimeRatio"), Table::new());
                // L2849
                tins(
                    globalBreakdown,
                    n!("ExertedAttackUptimeRatio"),
                    lfmt!("Maximum of:"),
                );
                // L2850
                for_ipairs!((_, cryTimeRatio) in warcryList => {
                    // L2851
                    if globalOutput.truthy(cryTimeRatio) {
                        // L2852
                        let cry = pob_lua::pattern::r#match(cryTimeRatio.as_str(), "(.+)Up.*", None)
                            .map(|c| c[0].to_str().into_owned());
                        tins(globalBreakdown, n!("ExertedAttackUptimeRatio"), lfmt!("%d%% ^8(%s Cry Uptime)", globalOutput.num0(cryTimeRatio), cry.as_deref()));
                    }
                });
                // L2855
                tins(
                    globalBreakdown,
                    n!("ExertedAttackUptimeRatio"),
                    lfmt!("= %d%%", globalOutput.num(n!("ExertedAttackUptimeRatio"))),
                );
            }
            // L2857
            if globalOutput.num(n!("ExertedAttackUptimeRatio")) > 0.0
                && !globalOutput.truthy(n!("ExertedAttackUptimeRatioCalculated"))
            {
                // L2858
                let incExertedAttacks =
                    skillModList.Sum(ModType::INC, Some(&*cfg), nm!["ExertIncrease"]);
                // L2859
                let moreExertedAttacks =
                    skillModList.Sum(ModType::MORE, Some(&*cfg), nm!["ExertIncrease"]);
                // L2860
                let moreExertedAttackDamage =
                    skillModList.Sum(ModType::MORE, Some(&*cfg), nm!["ExertAttackIncrease"]);
                // L2861
                let overexertionExertedDamage = skillModList.Sum(
                    ModType::MORE,
                    Some(&*cfg),
                    nm!["OverexertionExertAverageIncrease"],
                );
                // L2862
                let echoesOfCreationExertedDamage = skillModList.Sum(
                    ModType::MORE,
                    Some(&*cfg),
                    nm!["EchoesExertAverageIncrease"],
                );
                // L2863
                if activeSkill
                    .skillModList
                    .get()
                    .Flag(None, nm!["Condition:WarcryMaxHit"])
                {
                    // L2864
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::INC,
                        incExertedAttacks,
                        "Exerted Attacks",
                        0,
                        0,
                        &[],
                    );
                    // L2865
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        moreExertedAttacks,
                        "Exerted Attacks",
                        0,
                        0,
                        &[],
                    );
                    // L2866
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        moreExertedAttackDamage,
                        "Exerted Attack Damage",
                        ModFlag::Attack,
                        0,
                        &[],
                    );
                    // L2867
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        overexertionExertedDamage * exertingWarcryCount,
                        "Max Autoexertion Support",
                        0,
                        0,
                        &[],
                    );
                    // L2868
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        echoesOfCreationExertedDamage * exertingWarcryCount,
                        "Max Echoes of Creation",
                        0,
                        0,
                        &[],
                    );
                } else {
                    // L2870
                    let v = incExertedAttacks * globalOutput.num(n!("ExertedAttackUptimeRatio"))
                        / 100.0;
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::INC,
                        v,
                        "Uptime Scaled Exerted Attacks",
                        0,
                        0,
                        &[],
                    );
                    // L2871
                    let v = moreExertedAttacks * globalOutput.num(n!("ExertedAttackUptimeRatio"))
                        / 100.0;
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        v,
                        "Uptime Scaled Exerted Attacks",
                        0,
                        0,
                        &[],
                    );
                    // L2872
                    let v = moreExertedAttackDamage
                        * globalOutput.num(n!("ExertedAttackUptimeRatio"))
                        / 100.0;
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        v,
                        "Uptime Scaled Exerted Attack Damage",
                        ModFlag::Attack,
                        0,
                        &[],
                    );
                    // L2873
                    let v = overexertionExertedDamage
                        * globalOutput.num(n!("GlobalWarcryUptimeRatio"))
                        / 100.0;
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        v,
                        "Uptime Scaled Autoexertion Support",
                        0,
                        0,
                        &[],
                    );
                    // L2874
                    let v = echoesOfCreationExertedDamage
                        * globalOutput.num(n!("GlobalWarcryUptimeRatio"))
                        / 100.0;
                    skillModList.NewMod(
                        n!("Damage"),
                        ModType::MORE,
                        v,
                        "Uptime Scaled Echoes of Creation",
                        0,
                        0,
                        &[],
                    );
                }
                // L2876
                let v = calcLib::r#mod(&skillModList, Some(&*skillCfg), nm!["ExertIncrease"]);
                globalOutput.set(n!("ExertedAttackAvgDmg"), v);
                // L2877
                let v = globalOutput.num(n!("ExertedAttackAvgDmg"))
                    * calcLib::r#mod(
                        &skillModList,
                        Some(&*skillCfg),
                        nm![
                            "ExertAttackIncrease",
                            "OverexertionExertAverageIncrease",
                            "EchoesExertAverageIncrease"
                        ],
                    );
                globalOutput.set(n!("ExertedAttackAvgDmg"), v);
                // L2878
                let v = globalOutput.num(n!("ExertedAttackAvgDmg"))
                    * globalOutput.num(n!("ExertedAttackUptimeRatio"))
                    / 100.0;
                globalOutput.set(n!("ExertedAttackHitEffect"), v);
                // L2879
                globalOutput.set(
                    n!("ExertedAttackMaxHitEffect"),
                    globalOutput.get(n!("ExertedAttackAvgDmg")),
                );
                // L2880
                if let Some(globalBreakdown) = &globalBreakdown {
                    // L2881
                    globalBreakdown.set(
                        n!("ExertedAttackHitEffect"),
                        tbl![
                            lfmt!(
                                "(%.2f ^8(average exerted damage)",
                                globalOutput.num(n!("ExertedAttackAvgDmg"))
                            ),
                            lfmt!(
                                "x %.2f) ^8(uptime %%)",
                                globalOutput.num(n!("ExertedAttackUptimeRatio")) / 100.0
                            ),
                            lfmt!("= %.2f", globalOutput.num(n!("ExertedAttackHitEffect"))),
                        ],
                    );
                }
                // L2887
                globalOutput.set(n!("ExertedAttackUptimeRatioCalculated"), true);
            }
        }

        // Each Pact has different eligible spell types, but shares the same uptime calculation.
        // L2892
        if has_type(&activeSkill, SkillType::Spell) && !has_type(&activeSkill, SkillType::Brand) {
            // L2893
            let activeSkillList = actor.activeSkillList.get();
            for_ipairs!((_, value) in activeSkillList => {
                // L2894
                let pactName: Name = value.activeEffect.get().grantedEffect.get().name;
                // L2895
                let mut pactKey: Option<String> = pob_lua::pattern::r#match(pactName.as_str(), "^Pact of (.+)$", None)
                    .map(|c| c[0].to_str().into_owned());
                // L2896
                if let Some(pk) = pactKey.take() {
                    // L2897
                    let pactKey: String = if pk == "K'Tash" { "Ktash".to_owned() } else { pk };
                    // L2898
                    let isVaal = has_type(&activeSkill, SkillType::Vaal);
                    // L2899-2903 (`and`/`or` chain; only its truthiness is used)
                    let pactApplies = (pactKey == "Beidat" && !isVaal && !has_type(&activeSkill, SkillType::Channel)
                        && (skillFlags.truthy(n!("projectile")) || has_type(&activeSkill, SkillType::Cascadable) || skillFlags.truthy(n!("chaining")) && !skillFlags.truthy(n!("projectile"))))
                        || (pactKey == "Ghorr" && !isVaal && has_type(&activeSkill, SkillType::DamageOverTime))
                        || (pactKey == "Ktash" && isVaal && (has_type(&activeSkill, SkillType::Damage) || has_type(&activeSkill, SkillType::DamageOverTime)))
                        || (pactKey == "Lycia" && !isVaal && has_type(&activeSkill, SkillType::Channel) && skillFlags.truthy(n!("selfCast")));
                    // L2904
                    let calculated: Name = cat!("PactOf", pactKey, "Calculated");
                    // L2905
                    if pactApplies && !globalOutput.truthy(calculated) && !value.skillFlags.get().truthy(n!("disable")) {
                        let vModList = value.skillModList.get();
                        let vCfg = value.skillCfg.get();
                        let vData = value.skillData.get();
                        // L2906
                        let cooldown = calcSkillCooldown(&vModList, Some(&*vCfg), &vData).0;
                        // L2907
                        let castTime0 = match value.activeEffect.get().grantedEffect.get().castTime {
                            Some(t) => t,
                            None => lua_error!("attempt to perform arithmetic on field 'castTime' (a nil value)"),
                        };
                        let castRate = 1.0 / castTime0 * calcLib::r#mod(&vModList, Some(&*vCfg), nm!["Speed"]) * calcs::actionSpeedMod(&actor);
                        // L2908
                        let castTime = 1.0 / m_min(castRate, data().misc.ServerTickRate);
                        // L2909
                        let count = vModList.Sum(ModType::BASE, Some(&*vCfg), &[cat!(pactKey, "EmpoweredSpells")]);
                        // L2910
                        let storedUses = vData.num(n!("storedUses")) + vModList.Sum(ModType::BASE, Some(&*vCfg), nm!["AdditionalCooldownUses"]);
                        // L2911
                        let mut uptime = if globalOutput.num0(n!("Speed")) == 0.0 {
                            100.0
                        } else {
                            m_min((count / globalOutput.num(n!("Speed"))) / (cooldown + castTime), 1.0) * 100.0
                        };
                        // L2912
                        uptime = m_min(100.0, uptime * storedUses);
                        // L2913
                        let effect = if skillModList.Flag(None, nm!["Condition:PactMaxHit"]) { 100.0 } else { uptime };
                        // L2914
                        let effectMult = effect / 100.0;

                        // L2916
                        globalOutput.set(n!("CreatePactOffensiveCalcSection"), true);
                        // L2917
                        globalOutput.set(cat!("PactOf", pactKey, "Cooldown"), cooldown);
                        // L2918
                        globalOutput.set(cat!("PactOf", pactKey, "CastTime"), castTime);
                        // L2919
                        globalOutput.set(cat!(pactKey, "EmpoweredCount"), count);
                        // L2920
                        globalOutput.set(cat!(pactKey, "UpTimeRatio"), uptime);
                        // L2921
                        if let Some(globalBreakdown) = &globalBreakdown {
                            // L2922
                            globalBreakdown.set(cat!(pactKey, "UpTimeRatio"), tbl![
                                lfmt!("(%.2f ^8(number of Empowered)", count),
                                lfmt!("/ %.2f) ^8(casts per second)", globalOutput.num0(n!("Speed"))),
                                lfmt!("/ (%.2f ^8(pact cooldown)", cooldown),
                                lfmt!("+ %.2f) ^8(pact cast time)", castTime),
                                lfmt!("* %.2f ^8(stored uses)", storedUses),
                                lfmt!("= %.2f%%", uptime),
                            ]);
                        }

                        // L2932
                        let damage: Option<Value> = vModList.List(Some(&*vCfg), &[cat!(pactKey, "PactDamage")]).into_iter().next();
                        // L2933
                        if let Some(damage) = damage.filter(Value::truthy) {
                            // L2934
                            let r#mod: ModRef = match damage.inner_mod() {
                                Some(m) => m.clone(),
                                None => lua_error!("attempt to index local 'mod' (a nil value)"),
                            };
                            // L2935
                            skillModList.NewMod(
                                r#mod.name,
                                r#mod.r#type,
                                mod_value_num(&r#mod) * effectMult,
                                &format!("Uptime Scaled {pactName}"),
                                r#mod.flags,
                                r#mod.keywordFlags,
                                &r#mod.tags,
                            );
                        }

                        // L2938
                        if pactKey == "Beidat" {
                            // Coverage bonuses are averaged with the same uptime as the damage bonus.
                            // L2940
                            if skillFlags.truthy(n!("projectile")) && !skillModList.Flag(Some(&*skillCfg), nm!["NoAdditionalProjectiles"]) && !skillModList.Flag(Some(&*skillCfg), nm!["SingleProjectile"]) {
                                // L2941
                                let v = vModList.Sum(ModType::BASE, Some(&*vCfg), nm!["BeidatAdditionalProjectiles"]) * effectMult;
                                globalOutput.set(n!("BeidatAdditionalProjectiles"), v);
                                // L2942
                                let v = globalOutput.num(n!("ProjectileCount")) + globalOutput.num(n!("BeidatAdditionalProjectiles"));
                                globalOutput.set(n!("ProjectileCount"), v);
                            }
                            // L2944
                            if skillFlags.truthy(n!("chaining")) && !skillFlags.truthy(n!("projectile")) && !skillModList.Flag(Some(&*skillCfg), nm!["CannotChain"]) && !skillModList.Flag(Some(&*skillCfg), nm!["NoAdditionalChains"]) {
                                // L2945
                                let v = vModList.Sum(ModType::BASE, Some(&*vCfg), nm!["BeidatAdditionalBeamChains"]) * effectMult;
                                globalOutput.set(n!("BeidatAdditionalBeamChains"), v);
                                // L2946
                                let v = globalOutput.num(n!("ChainMax")) + globalOutput.num(n!("BeidatAdditionalBeamChains"));
                                globalOutput.set(n!("ChainMax"), v);
                                // L2947
                                globalOutput.set(n!("ChainMaxString"), globalOutput.get(n!("ChainMax")));
                                // L2948
                                let v = m_max(0.0, globalOutput.num(n!("ChainMax")) - globalOutput.num(n!("Chain")));
                                globalOutput.set(n!("ChainRemaining"), v);
                            }
                            // L2950
                            if has_type(&activeSkill, SkillType::Cascadable) {
                                // L2951
                                let v = vModList.Sum(ModType::BASE, Some(&*vCfg), nm!["BeidatAdditionalCascades"]) * effectMult;
                                globalOutput.set(n!("BeidatAdditionalCascades"), v);
                            }
                        } else if pactKey == "Ktash" {
                            // L2953 (elseif)
                            // L2954
                            let v = vModList.Sum(ModType::BASE, Some(&*vCfg), nm!["KtashPactSoulRefundChance"]) * effectMult;
                            globalOutput.set(n!("KtashSoulRefundChance"), v);
                            // L2955
                            if globalOutput.truthy(n!("SoulGainPreventionDuration")) {
                                // L2956
                                let duration = globalOutput.num(n!("SoulGainPreventionDuration"));
                                // L2957
                                let durationMod = 1.0 + vModList.Sum(ModType::BASE, Some(&*vCfg), nm!["KtashPactSoulGainPrevention"]) * effectMult / 100.0;
                                // L2958
                                let v = m_max(m_ceil(duration * durationMod * data().misc.ServerTickRate), 1.0) / data().misc.ServerTickRate;
                                globalOutput.set(n!("SoulGainPreventionDuration"), v);
                                // L2959
                                if let Some(globalBreakdown) = &globalBreakdown {
                                    // L2960
                                    globalBreakdown.set(n!("SoulGainPreventionDuration"), tbl![
                                        lfmt!("%.3fs ^8(before Pact of K'Tash)", duration),
                                        lfmt!("x %.4f ^8(average Pact modifier)", durationMod),
                                        "rounded up to nearest server tick",
                                        lfmt!("= %.3fs", globalOutput.num(n!("SoulGainPreventionDuration"))),
                                    ]);
                                }
                            }
                        }
                        // L2969
                        globalOutput.set(calculated, true);
                    }
                }
            });
        }
    }

    // L2976
    output.set(n!("RuthlessBlowHitEffect"), 1.0);
    // L2977
    output.set(n!("RuthlessBlowAilmentEffect"), 1.0);
    // L2978
    output.set(n!("FistOfWarDamageEffect"), 1.0);
    // L2979
    if env.mode_combat.get() {
        // L2980
        let ruthlessEffect = lor(env.configInput.get(n!("ruthlessSupportMode")), || {
            Val::from("AVERAGE")
        });
        let ruthlessEffectIs = |s: &str| ruthlessEffect.str().is_some_and(|r| &*r == s);
        // Calculate Ruthless Blow chance/multipliers + Fist of War multipliers
        // L2982
        output.set(
            n!("RuthlessBlowMaxCount"),
            skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["RuthlessBlowMaxCount"]),
        );
        // L2983
        let usedByMirage = || match skillCfg.skillCond.get() {
            Some(t) => t.truthy(n!("usedByMirage")),
            None => lua_error!("attempt to index field 'skillCond' (a nil value)"),
        };
        if output.num(n!("RuthlessBlowMaxCount")) > 0.0
            && (!usedByMirage()
                || skillData.num0(n!("mirageUses")) > output.num(n!("RuthlessBlowMaxCount")))
        {
            // L2984
            if ruthlessEffectIs("AVERAGE") {
                // L2985
                output.set(
                    n!("RuthlessBlowChance"),
                    round(100.0 / output.num(n!("RuthlessBlowMaxCount")), None),
                );
            } else if ruthlessEffectIs("MAX") {
                // L2986 (elseif)
                // L2987
                output.set(n!("RuthlessBlowChance"), 100.0);
                // L2988
                let v = skillData.num(n!("dpsMultiplier"))
                    / output.num_or(n!("RuthlessBlowMaxCount"), 1.0);
                skillData.set(n!("dpsMultiplier"), v);
            }
        } else {
            // L2991
            output.set(n!("RuthlessBlowChance"), 0.0);
        }
        // L2993
        output.set(
            n!("RuthlessBlowHitMultiplier"),
            1.0 + skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["RuthlessBlowHitMultiplier"])
                / 100.0,
        );
        // L2994
        output.set(
            n!("RuthlessBlowAilmentMultiplier"),
            1.0 + skillModList.Sum(
                ModType::BASE,
                Some(&*cfg),
                nm!["RuthlessBlowAilmentMultiplier"],
            ) / 100.0,
        );
        // L2995
        let v = 1.0 - output.num(n!("RuthlessBlowChance")) / 100.0
            + output.num(n!("RuthlessBlowChance")) / 100.0
                * output.num(n!("RuthlessBlowHitMultiplier"));
        output.set(n!("RuthlessBlowHitEffect"), v);
        // L2996
        let v = 1.0 - output.num(n!("RuthlessBlowChance")) / 100.0
            + output.num(n!("RuthlessBlowChance")) / 100.0
                * output.num(n!("RuthlessBlowAilmentMultiplier"));
        output.set(n!("RuthlessBlowAilmentEffect"), v);

        // L2998 (`Sum(...) or 0`: Sum always returns a number)
        globalOutput.set(
            n!("FistOfWarCooldown"),
            skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["FistOfWarCooldown"]),
        );
        // If Fist of War & Active Skill is a Slam Skill & NOT a Vaal Skill & NOT used by mirage or other
        // L3000
        if !num_eq(0.0, &globalOutput.get(n!("FistOfWarCooldown")))
            && has_type(&activeSkill, SkillType::Slam)
            && !has_type(&activeSkill, SkillType::Vaal)
            && !has_type(&activeSkill, SkillType::OtherThingUsesSkill)
        {
            // L3001
            globalOutput.set(
                n!("FistOfWarDamageMultiplier"),
                skillModList.Sum(ModType::BASE, None, nm!["FistOfWarDamageMultiplier"]) / 100.0,
            );
            // L3002
            let v = m_min(
                (1.0 / globalOutput.num(n!("Speed"))) / globalOutput.num(n!("FistOfWarCooldown")),
                1.0,
            ) * 100.0;
            globalOutput.set(n!("FistOfWarUptimeRatio"), v);
            // L3003
            if let Some(globalBreakdown) = &globalBreakdown {
                // L3004
                globalBreakdown.set(
                    n!("FistOfWarUptimeRatio"),
                    tbl![
                        lfmt!(
                            "min( (1 / %.2f) ^8(second per attack)",
                            globalOutput.num(n!("Speed"))
                        ),
                        lfmt!(
                            "/ %.2f, 1) ^8(fist of war cooldown)",
                            globalOutput.num(n!("FistOfWarCooldown"))
                        ),
                        lfmt!("= %d%%", globalOutput.num(n!("FistOfWarUptimeRatio"))),
                    ],
                );
            }
            // L3010
            globalOutput.set(
                n!("AvgFistOfWarDamage"),
                globalOutput.get(n!("FistOfWarDamageMultiplier")),
            );
            // L3011
            let v = 1.0
                + globalOutput.num(n!("FistOfWarDamageMultiplier"))
                    * (globalOutput.num(n!("FistOfWarUptimeRatio")) / 100.0);
            globalOutput.set(n!("AvgFistOfWarDamageEffect"), v);
            // L3012
            if let Some(globalBreakdown) = &globalBreakdown {
                // L3013
                globalBreakdown.set(
                    n!("AvgFistOfWarDamageEffect"),
                    tbl![
                        lfmt!(
                            "1 + (%.2f ^8(fist of war damage multiplier)",
                            globalOutput.num(n!("FistOfWarDamageMultiplier"))
                        ),
                        lfmt!(
                            "x %.2f) ^8(fist of war uptime ratio)",
                            globalOutput.num(n!("FistOfWarUptimeRatio")) / 100.0
                        ),
                        lfmt!("= %.2f", globalOutput.num(n!("AvgFistOfWarDamageEffect"))),
                    ],
                );
            }
            // L3019
            let v = 1.0 + globalOutput.num(n!("FistOfWarDamageMultiplier"));
            globalOutput.set(n!("MaxFistOfWarDamageEffect"), v);
            // L3020
            if activeSkill
                .skillModList
                .get()
                .Flag(None, nm!["Condition:WarcryMaxHit"])
            {
                // L3021
                output.set(
                    n!("FistOfWarDamageEffect"),
                    globalOutput.get(n!("MaxFistOfWarDamageEffect")),
                );
                // L3022
                let v = skillModList.Sum(ModType::BASE, None, nm!["FistOfWarIncAoE"]);
                skillModList.NewMod(
                    n!("AreaOfEffect"),
                    ModType::INC,
                    v,
                    "Max Fist of War Boosted AoE",
                    0,
                    0,
                    &[],
                );
            } else {
                // L3024
                output.set(
                    n!("FistOfWarDamageEffect"),
                    globalOutput.get(n!("AvgFistOfWarDamageEffect")),
                );
                // L3025
                let v = m_floor(
                    skillModList.Sum(ModType::BASE, None, nm!["FistOfWarIncAoE"]) / 100.0
                        * globalOutput.num(n!("FistOfWarUptimeRatio")),
                );
                skillModList.NewMod(
                    n!("AreaOfEffect"),
                    ModType::INC,
                    v,
                    "Avg Fist Of War Boosted AoE",
                    0,
                    0,
                    &[],
                );
            }
            // L3027
            calcAreaOfEffect(
                ctx,
                &skillModList,
                &skillCfg,
                &skillData,
                &skillFlags,
                &globalOutput,
                globalBreakdown.as_ref(),
            );
            // L3028
            let v = globalOutput.num(n!("TheoreticalOffensiveWarcryEffect"))
                * globalOutput.num(n!("AvgFistOfWarDamageEffect"));
            globalOutput.set(n!("TheoreticalOffensiveWarcryEffect"), v);
            // L3029
            let v = globalOutput.num(n!("TheoreticalMaxOffensiveWarcryEffect"))
                * globalOutput.num(n!("MaxFistOfWarDamageEffect"));
            globalOutput.set(n!("TheoreticalMaxOffensiveWarcryEffect"), v);
        } else {
            // L3031
            output.set(n!("FistOfWarDamageEffect"), 1.0);
        }
    }

    // Calculate maximum sustainable fuses and explosion rate for Explosive Arrow
    // Does not take into account mines or traps
    // L3037
    let grantedEffect = activeSkill.activeEffect.get().grantedEffect.get();
    if grantedEffect.name.as_str() == "Explosive Arrow" {
        // L3038
        match SkillHook::explosiveArrowFunc
            .of(&grantedEffect)
            .map(skillFunc)
        {
            Some(SkillFuncImpl::ExplosiveArrow(f)) => {
                f(
                    &activeSkill,
                    &output,
                    &globalOutput,
                    globalBreakdown.as_ref(),
                    &env,
                );
            }
            Some(SkillFuncImpl::Std(_)) => {
                lua_error!(
                    "explosiveArrowFunc of {} is not an explosiveArrowFunc",
                    grantedEffect.id
                )
            }
            None => lua_error!("attempt to call field 'explosiveArrowFunc' (a nil value)"),
        }
    }

    // Calculate crit chance, crit multiplier, and their combined effect
    // L3042
    if skillModList.Flag(Some(&*cfg), nm!["NeverCrit"]) {
        // L3043
        output.set(n!("PreEffectiveCritChance"), 0.0);
        // L3044
        output.set(n!("CritChance"), 0.0);
        // L3045
        output.set(n!("CritMultiplier"), 0.0);
        // L3046
        output.set(n!("BonusCritDotMultiplier"), 0.0);
        // L3047
        output.set(n!("CritEffect"), 1.0);
    } else if skillModList.Flag(
        Some(&*cfg),
        nm!["SpellSkillsCannotDealCriticalStrikesExceptOnFinalRepeat"],
    ) {
        // L3048 (elseif)
        // L3049
        if output.num_or(n!("Repeats"), 1.0) == 1.0 {
            // L3050
            output.set(n!("PreEffectiveCritChance"), 0.0);
            // L3051
            output.set(n!("CritChance"), 0.0);
            // L3052
            output.set(n!("CritMultiplier"), 0.0);
            // L3053
            output.set(n!("BonusCritDotMultiplier"), 0.0);
            // L3054
            output.set(n!("CritEffect"), 1.0);
        } else if skillModList.Flag(
            Some(&*cfg),
            nm!["SpellSkillsAlwaysDealCriticalStrikesOnFinalRepeat"],
        ) {
            // L3055 (elseif)
            let repeatMode = env.configInput.get(n!("repeatMode"));
            let repeatModeIs = |s: &str| repeatMode.str().is_some_and(|r| &*r == s);
            // L3056
            if repeatModeIs("None") {
                // L3057
                output.set(n!("PreEffectiveCritChance"), 0.0);
                // L3058
                output.set(n!("CritChance"), 0.0);
            } else if repeatModeIs("AVERAGE") {
                // L3059 (elseif)
                // L3060
                output.set(
                    n!("PreEffectiveCritChance"),
                    100.0 / output.num(n!("Repeats")),
                );
                // L3061
                output.set(n!("CritChance"), 100.0 / output.num(n!("Repeats")));
                // L3062
                if let Some(breakdown) = &breakdown {
                    // L3063
                    breakdown.set(
                        n!("CritChance"),
                        tbl![
                            lfmt!("100%%"),
                            lfmt!("/ %d ^8(number of repeats)", output.num(n!("Repeats"))),
                            lfmt!(
                                "= %.2f%% average critical strike chance",
                                output.num(n!("CritChance"))
                            ),
                        ],
                    );
                }
            } else {
                // L3070
                output.set(n!("PreEffectiveCritChance"), 100.0);
                // L3071
                output.set(n!("CritChance"), 100.0);
            }
            //else -- this shouldn't ever be a case but leaving this here if someone wants to implement it
        }
    } else {
        // L3076
        let mut critOverride: Val =
            Val::from_value(&skillModList.Override(Some(&*cfg), nm!["CritChance"]));
        // destructive link
        // L3078
        if skillModList.Flag(Some(&*cfg), nm!["MainHandCritIsEqualToParent"]) {
            // L3079
            let parent = parent_of(&actor);
            let parentOutput = parent.output.get();
            critOverride = lor(
                land(parentOutput.get(n!("MainHand")), || {
                    parentOutput.get(n!("MainHand")).get(n!("CritChance"))
                }),
                || parent.weaponData1.get().get(n!("CritChance")),
            );
        } else if skillModList.Flag(Some(&*cfg), nm!["MainHandCritIsEqualToPartyMember"]) {
            // L3080 (elseif)
            // L3081
            let party = party_of(&actor);
            let partyOutput = party.output.get();
            critOverride = lor(
                land(partyOutput.get(n!("MainHand")), || {
                    partyOutput.get(n!("MainHand")).get(n!("CritChance"))
                }),
                || {
                    lor(
                        land(Val::Table(party.weaponData1.get()), || {
                            party.weaponData1.get().get(n!("CritChance"))
                        }),
                        || Val::Num(0.0),
                    )
                },
            );
        }
        // L3083
        let mut baseCrit: Val = lor(
            lor(critOverride.clone(), || source.get(n!("CritChance"))),
            || Val::Num(0.0),
        );

        // L3085
        let baseCritFromMainHand = skillModList.Flag(Some(&*cfg), nm!["BaseCritFromMainHand"]);
        // L3086
        let baseCritFromParentMainHand =
            skillModList.Flag(Some(&*cfg), nm!["AttackCritIsEqualToParentMainHand"]);
        // L3087
        if baseCritFromMainHand {
            // L3088
            baseCrit = actor.weaponData1.get().get(n!("CritChance"));
        } else if baseCritFromParentMainHand {
            // L3089 (elseif)
            // L3090
            let parent = parent_of(&actor);
            let prev = baseCrit.clone();
            baseCrit = lor(
                land(Val::Table(parent.weaponData1.get()), || {
                    parent.weaponData1.get().get(n!("CritChance"))
                }),
                || prev,
            );
        }

        // L3093
        if num_eq(100.0, &critOverride) {
            // L3094
            output.set(n!("PreEffectiveCritChance"), 100.0);
            // L3095
            output.set(n!("PreBifurcateCritChance"), 100.0);
            // L3096
            output.set(n!("CritChance"), 100.0);
        } else {
            // L3098
            let mut base = 0.0;
            // L3099
            let mut inc = 0.0;
            // L3100
            let mut more = 1.0;
            // L3101
            if !critOverride.truthy() {
                // L3102
                base = skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["CritChance"])
                    + (if env.mode_effective.get() {
                        enemyDB.Sum(ModType::BASE, None, nm!["SelfCritChance"])
                    } else {
                        0.0
                    });
                // L3103
                inc = skillModList.Sum(ModType::INC, Some(&*cfg), nm!["CritChance"])
                    + (if env.mode_effective.get() {
                        enemyDB.Sum(ModType::INC, None, nm!["SelfCritChance"])
                    } else {
                        0.0
                    });
                // L3104
                more = skillModList.More(Some(&*cfg), nm!["CritChance"])
                    * (if env.mode_effective.get() {
                        enemyDB.More(None, nm!["SelfCritChance"])
                    } else {
                        1.0
                    });
            }
            // The game uses an integer permyriad chance for each critical strike roll.
            // L3107
            output.set(
                n!("CritChance"),
                round(
                    (baseCrit.num() + base) * (1.0 + inc / 100.0) * more * 100.0,
                    None,
                ) / 100.0,
            );
            // L3108
            let preCapCritChance = output.num(n!("CritChance"));
            // L3109
            let cap = lor(
                Val::from_value(&skillModList.Override(None, nm!["CritChanceCap"])),
                || Val::Num(skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["CritChanceCap"])),
            );
            output.set(
                n!("CritChance"),
                m_min(output.num(n!("CritChance")), cap.num()),
            );
            // L3110
            if (baseCrit.num() + base) > 0.0 {
                // L3111
                output.set(n!("CritChance"), m_max(output.num(n!("CritChance")), 0.0));
            }
            // L3113
            output.set(n!("PreEffectiveCritChance"), output.get(n!("CritChance")));
            // L3114
            let preLuckyCritChance = output.num(n!("CritChance"));
            // L3115
            let mut critRolls = 0.0;

            // L3117
            if env.mode_effective.get() && skillModList.Flag(Some(&*cfg), nm!["CritChanceLucky"]) {
                // L3118
                critRolls = critRolls + 1.0;
            }
            // L3120
            if skillModList.Flag(Some(&*skillCfg), nm!["ExtremeLuck"]) {
                // L3121
                critRolls = critRolls * 2.0;
            }
            // L3123
            if critRolls != 0.0 {
                // L3124
                if modDB.Flag(None, nm!["Unexciting"]) {
                    // Unexciting rolls three times and keeps the median result -> 3p^2 - 2p^3
                    // L3126
                    let v = (3.0 * lpow(output.num(n!("CritChance")) / 100.0, 2.0)
                        - 2.0 * lpow(output.num(n!("CritChance")) / 100.0, 3.0))
                        * 100.0;
                    output.set(n!("CritChance"), v);
                } else {
                    // L3128
                    let v = (1.0
                        - lpow(1.0 - output.num(n!("CritChance")) / 100.0, critRolls + 1.0))
                        * 100.0;
                    output.set(n!("CritChance"), v);
                }
            }
            // L3131
            output.set(n!("PreBifurcateCritChance"), output.get(n!("CritChance")));
            // L3132
            let preBifurcateCritChance = output.num(n!("CritChance"));
            // L3133
            if env.mode_effective.get() && skillModList.Flag(Some(&*cfg), nm!["BifurcateCrit"]) {
                // L3134
                let v = (1.0 - lpow(1.0 - output.num(n!("CritChance")) / 100.0, 2.0)) * 100.0;
                output.set(n!("CritChance"), v);
            }
            // L3136
            let mut preHitCheckCritChance = output.num(n!("CritChance"));
            // L3137
            let preSkillUseCritChance = output.num(n!("CritChance"));
            // L3138
            if env.mode_effective.get() {
                // L3139
                if skillModList.Flag(Some(&*skillCfg), nm!["Every3UseCrit"]) {
                    // L3140
                    output.set(
                        n!("CritChance"),
                        (2.0 * output.num(n!("CritChance")) + 100.0) / 3.0,
                    );
                }
                // L3142
                if skillModList.Flag(Some(&*skillCfg), nm!["Every5UseCrit"]) {
                    // L3143
                    output.set(
                        n!("CritChance"),
                        (4.0 * output.num(n!("CritChance")) + 100.0) / 5.0,
                    );
                }
                // L3145
                preHitCheckCritChance = output.num(n!("CritChance"));
                // L3146
                output.set(
                    n!("CritChance"),
                    output.num(n!("CritChance")) * output.num(n!("AccuracyHitChance")) / 100.0,
                );
            }
            // L3148
            if let Some(breakdown) = breakdown
                .as_ref()
                .filter(|_| !num_eq(output.num(n!("CritChance")), &baseCrit))
            {
                // L3149
                breakdown.set(n!("CritChance"), Table::new());
                // L3150
                let baseCritFromMainHandStr = if baseCritFromMainHand {
                    " from main weapon"
                } else if baseCritFromParentMainHand {
                    " from parent main weapon"
                } else {
                    ""
                };
                // L3151
                if base != 0.0 {
                    // L3152
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!(
                            "(%g + %g) ^8(base%s)",
                            baseCrit.num(),
                            base,
                            baseCritFromMainHandStr
                        ),
                    );
                } else {
                    // L3154
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!(
                            "%g ^8(base%s)",
                            baseCrit.num() + base,
                            baseCritFromMainHandStr
                        ),
                    );
                }
                // L3156
                if inc != 0.0 {
                    // L3157
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!("x %.2f", 1.0 + inc / 100.0) + " ^8(increased/reduced)",
                    );
                }
                // L3159
                if more != 1.0 {
                    // L3160
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!("x %.2f", more) + " ^8(more/less)",
                    );
                }
                // L3162
                tins(
                    breakdown,
                    n!("CritChance"),
                    lfmt!(
                        "= %.2f%% ^8(crit chance)",
                        output.num(n!("PreEffectiveCritChance"))
                    ),
                );
                // L3163
                if preCapCritChance > 100.0 {
                    // L3164
                    let overCap = preCapCritChance - 100.0;
                    // L3165
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!(
                            "Crit is overcapped by %.2f%% (%d%% increased Critical Strike Chance)",
                            overCap,
                            overCap / more / (baseCrit.num() + base) * 100.0
                        ),
                    );
                }
                // L3167
                let copy = match breakdown.get(n!("CritChance")) {
                    Val::Table(t) => t.copyTable(false),
                    other => lua_error!(
                        "bad argument #1 to 'pairs' (table expected, got {})",
                        other.lua_type()
                    ),
                };
                breakdown.set(n!("PreEffectiveCritChance"), copy);
                // L3168
                if env.mode_effective.get() {
                    // L3169
                    if critRolls != 0.0 {
                        // L3170
                        if skillModList.Flag(Some(&*skillCfg), nm!["Unexciting"]) {
                            // L3171
                            tins(breakdown, n!("CritChance"), "Crit Chance is Unexciting:");
                            // L3172
                            tins(
                                breakdown,
                                n!("CritChance"),
                                lfmt!(
                                    "(3 x %.4f^ 2) - (2 x %.4f^ 3)",
                                    preLuckyCritChance / 100.0,
                                    preLuckyCritChance / 100.0
                                ),
                            );
                        } else {
                            // L3174
                            tins(breakdown, n!("CritChance"), "Crit Chance is Lucky:");
                            // L3175
                            tins(
                                breakdown,
                                n!("CritChance"),
                                lfmt!(
                                    "1 - (1 - %.4f)^ %d",
                                    preLuckyCritChance / 100.0,
                                    critRolls + 1.0
                                ),
                            );
                        }
                        // L3177
                        tins(
                            breakdown,
                            n!("CritChance"),
                            lfmt!("= %.2f%%", preBifurcateCritChance),
                        );
                    }
                    // L3179
                    if skillModList.Flag(Some(&*cfg), nm!["BifurcateCrit"]) {
                        // L3180
                        tins(breakdown, n!("CritChance"), "Critical Strike Bifurcates:");
                        // L3181
                        tins(
                            breakdown,
                            n!("CritChance"),
                            lfmt!(
                                "1 - (1 - %.4f) x (1 - %.4f)",
                                preBifurcateCritChance / 100.0,
                                preBifurcateCritChance / 100.0
                            ),
                        );
                        // L3182
                        tins(
                            breakdown,
                            n!("CritChance"),
                            lfmt!("= %.2f%%", preSkillUseCritChance),
                        );
                    }
                    // L3184
                    if skillModList.Flag(Some(&*skillCfg), nm!["Every3UseCrit"]) {
                        // L3185
                        tins(
                            breakdown,
                            n!("CritChance"),
                            lfmt!(
                                "+ %.2f%% ^8(crit every 3rd use)",
                                (2.0 * preSkillUseCritChance + 100.0) / 3.0 - preSkillUseCritChance
                            ),
                        );
                    }
                    // L3187
                    if skillModList.Flag(Some(&*skillCfg), nm!["Every5UseCrit"]) {
                        // L3188
                        tins(
                            breakdown,
                            n!("CritChance"),
                            lfmt!(
                                "+ %.2f%% ^8(crit every 5th use)",
                                (4.0 * preSkillUseCritChance + 100.0) / 5.0 - preSkillUseCritChance
                            ),
                        );
                    }
                    // L3190
                    if skillModList.Flag(Some(&*skillCfg), nm!["Every3UseCrit"])
                        || skillModList.Flag(Some(&*skillCfg), nm!["Every5UseCrit"])
                    {
                        // L3191
                        tins(
                            breakdown,
                            n!("CritChance"),
                            lfmt!("= %.2f%%", preHitCheckCritChance),
                        );
                    }
                }
                // L3194
                if env.mode_effective.get() && output.num(n!("AccuracyHitChance")) < 100.0 {
                    // L3195
                    tins(breakdown, n!("CritChance"), "Crit confirmation roll:");
                    // L3196
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!("%.2f%%", preHitCheckCritChance),
                    );
                    // L3197
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!(
                            "x %.2f ^8(chance to hit)",
                            output.num(n!("AccuracyHitChance")) / 100.0
                        ),
                    );
                    // L3198
                    tins(
                        breakdown,
                        n!("CritChance"),
                        lfmt!("= %.2f%%", output.num(n!("CritChance"))),
                    );
                }
            }
        }
    }
    // L3203
    if !output.truthy(n!("CritEffect")) {
        // L3204
        if skillModList.Flag(Some(&*cfg), nm!["NoCritMultiplier"]) {
            // L3205
            output.set(n!("CritMultiplier"), 1.0);
        } else {
            // L3207
            let mut extraDamage =
                skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["CritMultiplier"]) / 100.0;
            // L3208
            let multiOverride =
                Val::from_value(&skillModList.Override(Some(&*skillCfg), nm!["CritMultiplier"]));
            // L3209
            if multiOverride.truthy() {
                // L3210
                extraDamage = (multiOverride.num() - 100.0) / 100.0;
            }
            // L3212
            if env.mode_effective.get() {
                // L3213
                let enemyInc =
                    1.0 + enemyDB.Sum(ModType::INC, None, nm!["SelfCritMultiplier"]) / 100.0;
                // L3214
                extraDamage = extraDamage
                    + enemyDB.Sum(ModType::BASE, None, nm!["SelfCritMultiplier"]) / 100.0;
                // L3215
                extraDamage = round(extraDamage * enemyInc, Some(2));
                // L3216
                if let Some(breakdown) = breakdown.as_ref().filter(|_| enemyInc != 1.0) {
                    // L3217
                    breakdown.set(
                        n!("CritMultiplier"),
                        tbl![
                            lfmt!(
                                "%d%% ^8(additional extra damage)",
                                (enemyDB.Sum(ModType::BASE, None, nm!["SelfCritMultiplier"])
                                    + skillModList.Sum(
                                        ModType::BASE,
                                        Some(&*cfg),
                                        nm!["CritMultiplier"]
                                    ))
                                    / 100.0
                            ),
                            lfmt!(
                                "x %.2f ^8(increased/reduced extra crit damage taken by enemy)",
                                enemyInc
                            ),
                            lfmt!("= %d%% ^8(extra crit damage)", extraDamage * 100.0),
                        ],
                    );
                }
            }
            // if crit bifurcates are enabled, roll for crit twice and add multiplier for each
            // L3225
            let critOverride =
                Val::from_value(&skillModList.Override(Some(&*cfg), nm!["CritChance"]));
            // L3226
            if env.mode_effective.get()
                && skillModList.Flag(Some(&*cfg), nm!["BifurcateCrit"])
                && output.truthy(n!("PreBifurcateCritChance"))
                && !num_eq(100.0, &critOverride)
            {
                // get crit chance and calculate odds of critting twice
                // L3228
                let critChancePercentage = output.num(n!("PreBifurcateCritChance"));
                // L3229
                let mut bifurcateMultiChance = (lpow(critChancePercentage, 2.0)) / 100.0;
                // L3230
                let effectiveCritChance = output.num(n!("CritChance"));
                // L3231
                let mut bifurcateUseChance = 1.0;
                // Guaranteed crit uses do not roll crit chance and therefore cannot bifurcate
                // L3233
                if skillModList.Flag(Some(&*skillCfg), nm!["Every3UseCrit"]) {
                    // L3234
                    bifurcateUseChance = bifurcateUseChance * 2.0 / 3.0;
                }
                // L3236
                if skillModList.Flag(Some(&*skillCfg), nm!["Every5UseCrit"]) {
                    // L3237
                    bifurcateUseChance = bifurcateUseChance * 4.0 / 5.0;
                }
                // L3239
                bifurcateMultiChance = bifurcateMultiChance * bifurcateUseChance;
                // L3240
                let conditionalBifurcateChance = if effectiveCritChance > 0.0 {
                    bifurcateMultiChance / effectiveCritChance
                } else {
                    0.0
                };
                // L3241
                output.set(n!("CritBifurcates"), 1.0 + conditionalBifurcateChance);
                // L3242
                let damageBonus = extraDamage;
                // L3243
                let bifurcatedBonus = conditionalBifurcateChance * extraDamage;
                // L3244
                if let Some(breakdown) = &breakdown {
                    // L3245
                    breakdown.set(
                        n!("CritBifurcates"),
                        tbl![
                            lfmt!("%.2f%% ^8(pre-bifurcate crit chance)", critChancePercentage),
                            lfmt!("x %.2f%%", critChancePercentage),
                        ],
                    );
                    // L3249
                    if bifurcateUseChance < 1.0 {
                        // L3250
                        tins(
                            breakdown,
                            n!("CritBifurcates"),
                            lfmt!(
                                "x %.2f%% ^8(uses that can bifurcate)",
                                bifurcateUseChance * 100.0
                            ),
                        );
                    }
                    // L3252
                    tins(
                        breakdown,
                        n!("CritBifurcates"),
                        lfmt!(
                            "= %.2f%% ^8(chance both crit rolls succeed)",
                            bifurcateMultiChance
                        ),
                    );
                    // L3253
                    tins(
                        breakdown,
                        n!("CritBifurcates"),
                        lfmt!("/ %.2f%% ^8(effective crit chance)", effectiveCritChance),
                    );
                    // L3254
                    tins(
                        breakdown,
                        n!("CritBifurcates"),
                        lfmt!(
                            "= %.2f ^8(crit Bifurcates effect)",
                            1.0 + conditionalBifurcateChance
                        ),
                    );
                }
                // L3256
                extraDamage = damageBonus + bifurcatedBonus;
                // mod doesn't affect output and is purely descriptive
                // L3258
                skillModList.NewMod(
                    n!("CritMultiplier"),
                    ModType::MORE,
                    floor(conditionalBifurcateChance * 100.0, Some(2)),
                    "Bifurcated Crit Damage Bonus",
                    ModFlag::Hit,
                    0,
                    &[],
                );
            }
            // L3260
            output.set(n!("CritMultiplier"), 1.0 + m_max(0.0, extraDamage));
        }
        // L3262
        let critChancePercentage = output.num(n!("CritChance")) / 100.0;
        // L3263
        output.set(
            n!("CritEffect"),
            1.0 - critChancePercentage + critChancePercentage * output.num(n!("CritMultiplier")),
        );
        // L3264
        let v = (skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["CritMultiplier"]) - 50.0)
            * skillModList.Sum(
                ModType::BASE,
                Some(&*cfg),
                nm!["CritMultiplierAppliesToDegen"],
            )
            / 10000.0;
        output.set(n!("BonusCritDotMultiplier"), v);
        // L3265
        if let Some(breakdown) = breakdown
            .as_ref()
            .filter(|_| output.num(n!("CritEffect")) != 1.0)
        {
            // L3266
            breakdown.set(
                n!("CritEffect"),
                tbl![
                    lfmt!(
                        "(1 - %.4f) ^8(portion of damage from non-crits)",
                        critChancePercentage
                    ),
                    lfmt!(
                        "+ [ (%.4f x %g) ^8(portion of damage from crits)",
                        critChancePercentage,
                        output.num(n!("CritMultiplier"))
                    ),
                    lfmt!("= %.3f", output.num(n!("CritEffect"))),
                ],
            );
        }
    }
    // L3273
    if !num_eq(0.0, &output.get(n!("CritChance"))) {
        // L3274
        skillModList.conditions.set(n!("CritInPast8Sec"), true);
    }

    // L3277
    output.set(n!("ScaledDamageEffect"), 1.0);

    // Calculate chance and multiplier for dealing triple damage on Normal and Crit
    // L3280
    output.set(
        n!("TripleDamageChanceOnCrit"),
        m_min(
            skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["TripleDamageChanceOnCrit"]),
            100.0,
        ),
    );
    // L3281 LUA-QUIRK: `Sum(...) or 0 + (...) + (...)` parses as `Sum(...) or (0 + ... + ...)`, and
    // Sum always returns a number, so the enemy and on-crit terms are never evaluated.
    output.set(
        n!("TripleDamageChance"),
        m_min(
            skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["TripleDamageChance"]),
            100.0,
        ),
    );
    // L3282
    output.set(
        n!("TripleDamageEffect"),
        2.0 * output.num(n!("TripleDamageChance")) / 100.0,
    );

    // Calculate chance and multiplier for dealing double damage on Normal and Crit
    // L3285
    output.set(
        n!("DoubleDamageChanceOnCrit"),
        m_min(
            skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["DoubleDamageChanceOnCrit"]),
            100.0,
        ),
    );
    // L3286
    let v = m_min(
        skillModList.Sum(ModType::BASE, Some(&*cfg), nm!["DoubleDamageChance"])
            + (if env.mode_effective.get() {
                enemyDB.Sum(ModType::BASE, Some(&*cfg), nm!["SelfDoubleDamageChance"])
            } else {
                0.0
            })
            + (output.num(n!("DoubleDamageChanceOnCrit")) * output.num(n!("CritChance")) / 100.0),
        100.0,
    );
    output.set(n!("DoubleDamageChance"), v);
    // L3287
    if globalOutput.truthy(n!("IntimidatingUpTimeRatio"))
        && activeSkill
            .skillModList
            .get()
            .Flag(None, nm!["Condition:WarcryMaxHit"])
    {
        // L3288
        output.set(n!("DoubleDamageChance"), 100.0);
    } else if globalOutput.truthy(n!("IntimidatingUpTimeRatio")) {
        // L3289 (elseif)
        // L3290
        let v = m_min(
            output.num(n!("DoubleDamageChance")) + globalOutput.num(n!("IntimidatingUpTimeRatio")),
            100.0,
        );
        output.set(n!("DoubleDamageChance"), v);
    }
    // Triple Damage overrides Double Damage. If you have both, it's the same as just having Triple
    // We need to subtract the probability of both happening in favor of Triple Damage
    // L3294
    if output.num(n!("TripleDamageChance")) > 0.0 {
        // L3295
        let v = m_max(
            output.num(n!("DoubleDamageChance"))
                - output.num(n!("TripleDamageChance")) * output.num(n!("DoubleDamageChance"))
                    / 100.0,
            0.0,
        );
        output.set(n!("DoubleDamageChance"), v);
    }
    // L3297
    output.set(
        n!("DoubleDamageEffect"),
        output.num(n!("DoubleDamageChance")) / 100.0,
    );
    // L3298
    let v = output.num(n!("ScaledDamageEffect"))
        * (1.0 + output.num(n!("DoubleDamageEffect")) + output.num(n!("TripleDamageEffect")));
    output.set(n!("ScaledDamageEffect"), v);

    // L3300
    let hitRate = output.num(n!("HitChance")) / 100.0
        * lor(globalOutput.get(n!("HitSpeed")), || {
            globalOutput.get(n!("Speed"))
        })
        .num()
        * skillData.num(n!("dpsMultiplier"));
    hp.hitRate = Some(hitRate);

    // Calculate culling DPS
    // L3303
    let mut criticalCull = skillModList
        .Max(Some(&*cfg), nm!["CriticalCullPercent"])
        .unwrap_or(0.0);
    // L3304
    if criticalCull > 0.0 {
        // L3305
        criticalCull = m_min(
            criticalCull,
            criticalCull * (1.0 - lpow(1.0 - output.num(n!("CritChance")) / 100.0, hitRate)),
        );
    }
    // L3307
    let regularCull = skillModList
        .Max(Some(&*cfg), nm!["CullPercent"])
        .unwrap_or(0.0);
    // L3308
    let maxCullPercent = m_max(criticalCull, regularCull);
    // L3309
    globalOutput.set(n!("CullPercent"), maxCullPercent);
    // L3310
    globalOutput.set(
        n!("CullMultiplier"),
        100.0 / (100.0 - globalOutput.num(n!("CullPercent"))),
    );

    //Calculate reservation DPS
    // L3313
    globalOutput.set(
        n!("ReservationDpsMultiplier"),
        100.0 / (100.0 - enemyDB.Sum(ModType::BASE, None, nm!["LifeReservationPercent"])),
    );

    // L3315
    runSkillFunc(ctx, SkillHook::postCritFunc);

    // Added damage redirection (Cryogenesis)
    // Convert all added damage mods to the target type before the damage loop
    // so breakdowns show the redirected source correctly.
    // Base Elemental Hit is excluded per the node text.
    // L3321
    let mut addedDamageRedirectType: Option<&str> = None;
    // L3322
    if skillModList.Flag(Some(&*cfg), nm!["AllAddedDamageAsLightning"]) {
        // L3323
        addedDamageRedirectType = Some("Lightning");
    } else if skillModList.Flag(Some(&*cfg), nm!["AllAddedDamageAsCold"]) {
        // L3324 (elseif)
        // L3325
        addedDamageRedirectType = Some("Cold");
    }
    // L3327
    if let Some(addedDamageRedirectType) = addedDamageRedirectType {
        // L3328
        for_ipairs!((_, damageType) in dmgTypeList[..] => {
            // L3329
            if damageType != addedDamageRedirectType {
                // L3330
                let rows = skillModList.Tabulate(Some(ModType::BASE), Some(&*cfg), &[cat!(damageType, "Min")]);
                for_ipairs!((_, value) in rows => {
                    // L3331
                    if pob_lua::pattern::r#match(mod_source(&value.r#mod), "ElementalHit", None).is_none() {
                        // L3332
                        let r#mod = value.r#mod.clone();
                        // L3333
                        let mut tags: Vec<Tag> = Vec::with_capacity(r#mod.tags.len() + 1);
                        tags.push(Tag { r#type: TagType::Other(n!("Cryogenesis Added Damage")), ..Default::default() });
                        tags.extend(r#mod.tags.iter().cloned());
                        skillModList.ConvertMod(cat!(damageType, "Min"), cat!(addedDamageRedirectType, "Min"), ModType::BASE, r#mod.value.clone(), mod_source(&r#mod), r#mod.flags, r#mod.keywordFlags, &tags);
                    }
                });
                // L3336
                let rows = skillModList.Tabulate(Some(ModType::BASE), Some(&*cfg), &[cat!(damageType, "Max")]);
                for_ipairs!((_, value) in rows => {
                    // L3337
                    if pob_lua::pattern::r#match(mod_source(&value.r#mod), "ElementalHit", None).is_none() {
                        // L3338
                        let r#mod = value.r#mod.clone();
                        // L3339
                        let mut tags: Vec<Tag> = Vec::with_capacity(r#mod.tags.len() + 1);
                        tags.push(Tag { r#type: TagType::Other(n!("Cryogenesis Added Damage")), ..Default::default() });
                        tags.extend(r#mod.tags.iter().cloned());
                        skillModList.ConvertMod(cat!(damageType, "Max"), cat!(addedDamageRedirectType, "Max"), ModType::BASE, r#mod.value.clone(), mod_source(&r#mod), r#mod.flags, r#mod.keywordFlags, &tags);
                    }
                });
            }
        });
    }

    Flow::Normal
}
