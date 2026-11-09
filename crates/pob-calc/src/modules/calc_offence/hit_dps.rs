//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence pass loop body B and pass combination, OF-F (CalcOffence.lua 3346-4154).
//!
//! Owner: OF-F. `seg_passHit` is the second half of the hit pass loop body (base and per-type
//! hit damage, crits and luck, enemy mitigation, leech, gain on hit/kill, average hit/damage,
//! DPS, PvP); `seg_combinePasses` combines the passes (`combineStat`), builds the DPS
//! breakdowns and the leech rates. The two local functions of the range (`useThisResist`
//! 3509-3517 and `getLeechInstances` 3740-3746) do not cross a segment boundary, so they are
//! closures inside `seg_passHit` and their provenance is part of its marker.
//!
//! Module-level upvalues: `globalOutput`/`globalBreakdown` (CalcOffence.lua:65/67) are read
//! from `OFFENCE_STATE` (§7.3); `dmgTypeList` (34) and `isElemental` (31) are immutable
//! module-level constants declared in OF-A's range (7-67), mirrored here as private items
//! (no provenance marker of their own: the declaration is covered by OF-A's).
#![allow(
    non_snake_case,
    unused_variables,
    unused_assignments,
    unused_mut,
    clippy::all,
    clippy::pedantic
)]

use pob_lua::format::FmtArg;

use super::costs_passes::combineStat;
use super::helpers_setup::{calcDamage, calcResistForType};
use super::{HitPassLocals, OffenceCtx};
use crate::calcs;
use crate::modules::calc_format::formatNumSep;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;

/// `local dmgTypeList = {"Physical", "Lightning", "Cold", "Fire", "Chaos"}` (CalcOffence.lua:34).
fn dmgTypeList() -> [Name; 5] {
    [
        n!("Physical"),
        n!("Lightning"),
        n!("Cold"),
        n!("Fire"),
        n!("Chaos"),
    ]
}

/// `isElemental[damageType]` with `local isElemental = { Fire = true, Cold = true, Lightning = true }`
/// (CalcOffence.lua:31).
fn isElemental(damageType: Name) -> bool {
    damageType == n!("Fire") || damageType == n!("Cold") || damageType == n!("Lightning")
}

// ---- Rust-only plumbing ----------------------------------------------------------------------

/// The module upvalue `globalOutput` (indexing it while nil raises).
fn global_output() -> Output {
    OFFENCE_STATE
        .with(|s| s.globalOutput.get())
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'globalOutput' (a nil value)"))
}

/// The module upvalue `globalBreakdown`.
fn global_breakdown() -> Option<Breakdown> {
    OFFENCE_STATE.with(|s| s.globalBreakdown.get())
}

/// Lua `v ~= x` for a number `x` (a non-number is never equal; no coercion, no error).
fn ne(v: &Val, x: f64) -> bool {
    match v {
        Val::Num(n) => *n != x,
        _ => true,
    }
}

/// A dynamic value as a `string.format` argument (a non-number/string raises in `lfmt!`
/// for the numeric conversions, as `string.format` does).
fn fa(v: &Val) -> FmtArg<'_> {
    match v {
        Val::Num(n) => FmtArg::Num(*n),
        Val::Str(s) => FmtArg::Str(s),
        Val::Bool(b) => FmtArg::Bool(*b),
        _ => FmtArg::Nil,
    }
}

/// `t[k]` used as a table (`t_insert(t[k], …)`, `t[k].x = …`): raises when it is not one.
fn tab(t: &Table, k: impl Into<Key>) -> Table {
    let k = k.into();
    match t.get(k) {
        Val::Table(x) => x,
        v => lua_error!("bad argument #1 to 'insert' (table expected, got {})", v.lua_type()),
    }
}

/// `t_insert(t[k], v)`.
fn ins(t: &Table, k: impl Into<Key>, v: impl Into<Val>) {
    tab(t, k).push(v);
}

/// `x or y` on two values, used as a number.
fn num_or_else(a: Val, b: impl FnOnce() -> Val) -> f64 {
    lor(a, b).num()
}

/// @pob Modules/CalcOffence.lua:3346-3955 h=0090c651
/// OF-F: CalcOffence.lua 3346-3955 (pass body B, incl. the loop `end`).
pub(crate) fn seg_passHit(ctx: &mut OffenceCtx, hp: &mut HitPassLocals) -> Flow {
    let env = ctx.env.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let canDeal = ctx.canDeal.clone();
    let isAttack = ctx.isAttack;
    let monsterLife = ctx.monsterLife;
    let quantityMultiplier = ctx.quantityMultiplier;
    let pass = hp.pass.clone();
    let source = hp.source.clone();
    let output = hp.output.clone();
    let cfg = hp.cfg.clone();
    let breakdown = hp.breakdown.clone();
    let hitRate = hp.hitRate;
    let hitRate = || {
        hitRate.unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on upvalue 'hitRate' (a nil value)")
        })
    };
    let globalOutput = global_output();
    let globalBreakdown = global_breakdown();
    let data = data();
    let scfg: Option<&ModCfg> = Some(&*skillCfg);
    let pcfg: Option<&ModCfg> = Some(&*cfg);

    // Calculate base hit damage
    // L3347
    for damageType in dmgTypeList() {
        // L3348
        let mut damageTypeMin = cat!(damageType, "Min");
        // L3349
        let mut damageTypeMax = cat!(damageType, "Max");
        // L3350
        let grantedEffectLevel = activeSkill
            .activeEffect
            .get()
            .grantedEffectLevel
            .get()
            .unwrap_or_else(|| {
                lua_error!("attempt to index field 'grantedEffectLevel' (a nil value)")
            });
        let mut baseMultiplier = num_or_else(
            lor(
                Val::from_value(grantedEffectLevel.get(n!("baseMultiplier"))),
                || skillData.get(n!("baseMultiplier")),
            ),
            || Val::Num(1.0),
        );
        // L3351
        let mut damageEffectiveness = num_or_else(
            lor(
                Val::from_value(grantedEffectLevel.get(n!("damageEffectiveness"))),
                || skillData.get(n!("damageEffectiveness")),
            ),
            || Val::Num(1.0),
        );
        // L3352
        let mut addedMin = skillModList.Sum(ModType::BASE, pcfg, &[damageTypeMin])
            + enemyDB.Sum(ModType::BASE, pcfg, &[cat!("Self", damageTypeMin)]);
        // L3353
        let mut addedMax = skillModList.Sum(ModType::BASE, pcfg, &[damageTypeMax])
            + enemyDB.Sum(ModType::BASE, pcfg, &[cat!("Self", damageTypeMax)]);
        // L3354
        let mut addedMult = calcLib::r#mod(
            &skillModList,
            pcfg,
            &[cat!("Added", damageType, "Damage"), n!("AddedDamage")],
        );
        // L3355
        let mut baseMin = (source.num0(damageTypeMin) + source.num0(cat!(damageType, "BonusMin")))
            * baseMultiplier
            + addedMin * damageEffectiveness * addedMult;
        // L3356
        let mut baseMax = (source.num0(damageTypeMax) + source.num0(cat!(damageType, "BonusMax")))
            * baseMultiplier
            + addedMax * damageEffectiveness * addedMult;
        // L3357
        output.set(cat!(damageTypeMin, "Base"), baseMin);
        // L3358
        output.set(cat!(damageTypeMax, "Base"), baseMax);
        // L3359
        if let Some(breakdown) = &breakdown {
            // L3360
            breakdown.set(damageType, tbl! { damageTypes = Table::new() });
            // L3361
            if baseMin != 0.0 && baseMax != 0.0 {
                // L3362
                ins(breakdown, damageType, "Base damage:");
                // L3363
                let mut plus = "";
                // L3364
                if source.num0(damageTypeMin) != 0.0 || source.num0(damageTypeMax) != 0.0 {
                    // L3365
                    let line = lfmt!(
                        "%d to %d ^8(base damage from %s)",
                        fa(&source.get(damageTypeMin)),
                        fa(&source.get(damageTypeMax)),
                        if source.truthy(n!("type")) { "weapon" } else { "skill" }
                    );
                    ins(breakdown, damageType, line);
                    // L3366
                    if baseMultiplier != 1.0 {
                        // L3367
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("x %.2f ^8(base damage multiplier)", baseMultiplier),
                        );
                    }
                    // L3369
                    plus = "+ ";
                }
                // L3371
                if addedMin != 0.0 || addedMax != 0.0 {
                    // L3372
                    ins(
                        breakdown,
                        damageType,
                        lfmt!("%s%d to %d ^8(added damage)", plus, addedMin, addedMax),
                    );
                    // L3373
                    if damageEffectiveness != 1.0 {
                        // L3374
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("x %.2f ^8(damage effectiveness)", damageEffectiveness),
                        );
                    }
                    // L3376
                    if addedMult != 1.0 {
                        // L3377
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("x %.2f ^8(added damage multiplier)", addedMult),
                        );
                    }
                }
                // L3380
                ins(
                    breakdown,
                    damageType,
                    lfmt!("= %.1f to %.1f", baseMin, baseMax),
                );
            }
        }
    }

    // Calculate hit damage for each damage type
    // L3386
    let (mut totalHitMin, mut totalHitMax, mut totalHitAvg) = (0.0, 0.0, 0.0);
    // L3387
    let (mut totalCritMin, mut totalCritMax, mut totalCritAvg) = (0.0, 0.0, 0.0);
    // L3388
    let mut ghostReaver = skillModList.Flag(None, &[n!("GhostReaver")]);
    // L3389
    let mut ghostReaverLifeLeech = 0.0;
    // L3390
    output.set(n!("LifeLeech"), 0.0);
    // L3391
    output.set(n!("LifeLeechInstant"), 0.0);
    // L3392
    output.set(n!("EnergyShieldLeech"), 0.0);
    // L3393
    output.set(n!("EnergyShieldLeechInstant"), 0.0);
    // L3394
    output.set(n!("ManaLeech"), 0.0);
    // L3395
    output.set(n!("ManaLeechInstant"), 0.0);
    // L3396
    output.set(n!("impaleStoredHitAvg"), 0.0);
    // L3397
    for pass in 1..=2_i32 {
        // Pass 1 is critical strike damage, pass 2 is non-critical strike
        // L3399
        cfg.skillCond
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'skillCond' (a nil value)"))
            .set(n!("CriticalStrike"), pass == 1);
        // L3400
        let mut lifeLeechTotal = 0.0;
        // L3401
        let mut energyShieldLeechTotal = 0.0;
        // L3402
        let mut manaLeechTotal = 0.0;
        // L3403
        let mut noLifeLeech = skillModList.Flag(pcfg, &[n!("CannotLeechLife")])
            || enemyDB.Flag(None, &[n!("CannotLeechLifeFromSelf")])
            || skillModList.Flag(pcfg, &[n!("CannotGainLife")]);
        // L3404
        let mut noEnergyShieldLeech = skillModList.Flag(pcfg, &[n!("CannotLeechEnergyShield")])
            || enemyDB.Flag(None, &[n!("CannotLeechEnergyShieldFromSelf")])
            || skillModList.Flag(pcfg, &[n!("CannotGainEnergyShield")]);
        // L3405
        let mut noManaLeech = skillModList.Flag(pcfg, &[n!("CannotLeechMana")])
            || enemyDB.Flag(None, &[n!("CannotLeechManaFromSelf")])
            || skillModList.Flag(pcfg, &[n!("CannotGainMana")]);
        // L3406
        for damageType in dmgTypeList() {
            // L3407 (eight names, seven values: damageTypeHitAvgNotLucky is nil until 3478)
            let (
                mut damageTypeHitMin,
                mut damageTypeHitMax,
                mut damageTypeHitAvg,
                mut damageTypeLuckyChance,
                mut damageTypeHitAvgLucky,
                mut damageTypeUnluckyChance,
                mut damageTypeHitAvgUnlucky,
            ) = (0.0, 0.0, 0.0, 0.0, 0.0, 0.0, 0.0);
            let mut damageTypeHitAvgNotLucky: f64;
            // L3408
            if skillFlags.truthy(n!("hit")) && canDeal.truthy(damageType) {
                // L3409
                let bd_arg = if pass == 2 {
                    breakdown.as_ref().and_then(|b| b.get(damageType).table())
                } else {
                    None
                };
                (damageTypeHitMin, damageTypeHitMax) = calcDamage(
                    &activeSkill,
                    &output,
                    &cfg,
                    bd_arg.as_ref(),
                    damageType,
                    0,
                    None,
                );
                // L3410
                let mut conversionTable = cfg
                    .conversionTable
                    .get()
                    .or_else(|| activeSkill.conversionTable.get())
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index local 'conversionTable' (a nil value)")
                    });
                // L3411
                let mut convMult = conversionTable
                    .get(damageType)
                    .table()
                    .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                    .num(n!("mult"));
                // L3412
                if pass == 2 {
                    if let Some(breakdown) = &breakdown {
                        // L3413
                        ins(breakdown, damageType, "Hit damage:");
                        // L3414
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("%d to %d ^8(total damage)", damageTypeHitMin, damageTypeHitMax),
                        );
                        // L3415
                        if convMult != 1.0 {
                            // L3416
                            ins(
                                breakdown,
                                damageType,
                                lfmt!(
                                    "x %g ^8(%g%% converted to other damage types)",
                                    convMult,
                                    (1.0 - convMult) * 100.0
                                ),
                            );
                        }
                        // L3418
                        if ne(&output.get(n!("DoubleDamageEffect")), 0.0) {
                            // L3419
                            if ne(&output.get(n!("TripleDamageEffect")), 0.0) {
                                // L3420
                                ins(breakdown, damageType, lfmt!("x %.2f ^8(1 + %.2f + %.2f multiplier from %.1f%% chance to deal double damage and %d%% chance to deal triple damage)", 1.0 + output.num(n!("DoubleDamageEffect")) + output.num(n!("TripleDamageEffect")), fa(&output.get(n!("DoubleDamageEffect"))), fa(&output.get(n!("TripleDamageEffect"))), fa(&output.get(n!("DoubleDamageChance"))), fa(&output.get(n!("TripleDamageChance")))));
                            } else {
                                // L3422
                                ins(
                                    breakdown,
                                    damageType,
                                    lfmt!(
                                        "x %.2f ^8(multiplier from %d%% chance to deal double damage)",
                                        1.0 + output.num(n!("DoubleDamageEffect")),
                                        fa(&output.get(n!("DoubleDamageChance")))
                                    ),
                                );
                            }
                        } else if ne(&output.get(n!("TripleDamageEffect")), 0.0) {
                            // L3425
                            ins(
                                breakdown,
                                damageType,
                                lfmt!(
                                    "x %.2f ^8(multiplier from %d%% chance to deal triple damage)",
                                    1.0 + output.num(n!("TripleDamageEffect")),
                                    fa(&output.get(n!("TripleDamageChance")))
                                ),
                            );
                        }
                        // L3427
                        if ne(&output.get(n!("RuthlessBlowHitEffect")), 1.0) {
                            // L3428
                            ins(
                                breakdown,
                                damageType,
                                lfmt!(
                                    "x %.2f ^8(ruthless blow effect modifier)",
                                    fa(&output.get(n!("RuthlessBlowHitEffect")))
                                ),
                            );
                        }
                        // L3430
                        if ne(&output.get(n!("FistOfWarDamageEffect")), 1.0) {
                            // L3431
                            ins(
                                breakdown,
                                damageType,
                                lfmt!(
                                    "x %.2f ^8(fist of war effect modifier)",
                                    fa(&output.get(n!("FistOfWarDamageEffect")))
                                ),
                            );
                        }
                        // L3433
                        if ne(&globalOutput.get(n!("OffensiveWarcryEffect")), 1.0)
                            && !activeSkill
                                .skillModList
                                .get()
                                .Flag(None, &[n!("Condition:WarcryMaxHit")])
                        {
                            // L3434
                            ins(
                                breakdown,
                                damageType,
                                lfmt!(
                                    "x %.2f ^8(aggregated warcry exerted effect modifier)",
                                    fa(&globalOutput.get(n!("OffensiveWarcryEffect")))
                                ),
                            );
                        }
                        // L3436
                        if ne(&globalOutput.get(n!("MaxOffensiveWarcryEffect")), 1.0)
                            && activeSkill
                                .skillModList
                                .get()
                                .Flag(None, &[n!("Condition:WarcryMaxHit")])
                        {
                            // L3437
                            ins(
                                breakdown,
                                damageType,
                                lfmt!(
                                    "x %.2f ^8(aggregated max warcry exerted effect modifier)",
                                    fa(&globalOutput.get(n!("MaxOffensiveWarcryEffect")))
                                ),
                            );
                        }
                    }
                }
                // L3440
                if activeSkill
                    .skillModList
                    .get()
                    .Flag(None, &[n!("Condition:WarcryMaxHit")])
                {
                    // L3441
                    let v = convMult
                        * output.num(n!("ScaledDamageEffect"))
                        * output.num(n!("RuthlessBlowHitEffect"))
                        * output.num(n!("FistOfWarDamageEffect"))
                        * globalOutput.num(n!("MaxOffensiveWarcryEffect"));
                    output.set(n!("allMult"), v);
                } else {
                    // L3443
                    let v = convMult
                        * output.num(n!("ScaledDamageEffect"))
                        * output.num(n!("RuthlessBlowHitEffect"))
                        * output.num(n!("FistOfWarDamageEffect"))
                        * globalOutput.num(n!("OffensiveWarcryEffect"));
                    output.set(n!("allMult"), v);
                }
                // L3445
                let mut allMult = output.get(n!("allMult"));
                // L3446
                if pass == 1 {
                    // Apply crit multiplier
                    // L3448
                    allMult = Val::Num(allMult.num() * output.num(n!("CritMultiplier")));
                }
                let allMult = allMult.num();
                // L3450
                damageTypeHitMin = damageTypeHitMin * allMult;
                // L3451
                damageTypeHitMax = damageTypeHitMax * allMult;
                // L3452
                if skillModList.Flag(scfg, &[n!("LuckyHits")])
                    || (pass == 2
                        && damageType == n!("Lightning")
                        && skillModList.Flag(scfg, &[n!("LightningNoCritLucky")]))
                    || (pass == 1 && skillModList.Flag(scfg, &[n!("CritLucky")]))
                    || (damageType == n!("Lightning")
                        && modDB.Flag(None, &[n!("LightningLuckHits")]))
                    || (damageType == n!("Chaos") && modDB.Flag(None, &[n!("ChaosLuckyHits")]))
                    || (damageType == n!("Fire") && modDB.Flag(None, &[n!("FireLuckyHits")]))
                    || (damageType == n!("Cold") && modDB.Flag(None, &[n!("ColdLuckyHits")]))
                    || ((damageType == n!("Lightning")
                        || damageType == n!("Cold")
                        || damageType == n!("Fire"))
                        && skillModList.Flag(scfg, &[n!("ElementalLuckHits")]))
                {
                    // L3460
                    damageTypeLuckyChance = 1.0;
                } else {
                    // L3462
                    damageTypeLuckyChance =
                        m_min(skillModList.Sum(ModType::BASE, scfg, &[n!("LuckyHitsChance")]), 100.0)
                            / 100.0;
                }
                // L3464
                if skillModList.Flag(scfg, &[n!("UnluckyHits")]) {
                    // L3465
                    damageTypeLuckyChance = damageTypeLuckyChance - 1.0;
                }
                // L3467
                let mut rolls = damageTypeLuckyChance;
                // L3468
                if skillModList.Flag(scfg, &[n!("ExtremeLuck")]) {
                    // L3469
                    rolls = rolls * 2.0;
                }
                // L3471
                if skillModList.Flag(scfg, &[n!("Unexciting")]) {
                    // L3472
                    rolls = 0.0;
                }
                // L3474
                output.set(n!("DamageRolls"), rolls);
                // L3475
                if pass == 1 {
                    // L3476
                    output.set(n!("CritDamageRolls"), rolls);
                }
                // L3478
                rolls = m_abs(rolls) + 2.0;
                // L3479
                damageTypeHitAvgNotLucky = damageTypeHitMin / rolls + damageTypeHitMax / rolls;
                // L3480
                damageTypeHitAvgLucky =
                    damageTypeHitMin / rolls + (rolls - 1.0) * damageTypeHitMax / rolls;
                // L3481
                damageTypeHitAvgUnlucky =
                    (rolls - 1.0) * damageTypeHitMin / rolls + damageTypeHitMax / rolls;
                // L3482
                if damageTypeLuckyChance >= 0.0 {
                    // L3483
                    damageTypeHitAvg = damageTypeHitAvgNotLucky * (1.0 - damageTypeLuckyChance)
                        + damageTypeHitAvgLucky * damageTypeLuckyChance;
                } else {
                    // L3485
                    damageTypeHitAvg = damageTypeHitAvgNotLucky
                        * (1.0 - m_abs(damageTypeLuckyChance))
                        + damageTypeHitAvgUnlucky * m_abs(damageTypeLuckyChance);
                }
                // L3487
                if (damageTypeHitMin != 0.0 || damageTypeHitMax != 0.0) && env.mode_effective.get()
                {
                    // Apply enemy resistances and damage taken modifiers
                    // L3489
                    let mut resist = 0.0;
                    // L3490
                    let mut pen = 0.0;
                    // L3491
                    let mut sourceRes = damageType;
                    // L3492
                    let mut takenInc = enemyDB.Sum(
                        ModType::INC,
                        pcfg,
                        &[n!("DamageTaken"), cat!(damageType, "DamageTaken")],
                    );
                    // L3493
                    let mut takenMore =
                        enemyDB.More(pcfg, &[n!("DamageTaken"), cat!(damageType, "DamageTaken")]);
                    // Check if player is supposed to ignore a damage type, or if it's ignored on enemy side
                    // L3495
                    let useThisResist = |damageType: Name| -> f64 {
                        // L3496
                        if skillModList.Flag(
                            pcfg,
                            &names([
                                Some(cat!("Ignore", damageType, "Resistance")),
                                isElemental(damageType).then(|| n!("IgnoreElementalResistances")),
                            ]),
                        ) || enemyDB.Flag(None, &[cat!("SelfIgnore", damageType, "Resistance")])
                        {
                            // L3497
                            return 0.0;
                        }
                        // L3499
                        let mut chanceToIgnore = skillModList.Sum(
                            ModType::BASE,
                            pcfg,
                            &names([
                                Some(cat!("ChanceToIgnore", damageType, "Resistance")),
                                isElemental(damageType)
                                    .then(|| n!("ChanceToIgnoreElementalResistances")),
                            ]),
                        );
                        // L3500
                        chanceToIgnore = m_min(m_max(chanceToIgnore, 0.0), 100.0);
                        // L3501
                        1.0 - chanceToIgnore / 100.0
                    };
                    // L3503
                    if damageType == n!("Physical") {
                        // store pre-armour physical damage from attacks for impale calculations
                        // L3505
                        if pass == 1 {
                            // L3506
                            let v = output.num(n!("impaleStoredHitAvg"))
                                + damageTypeHitAvg * (output.num(n!("CritChance")) / 100.0);
                            output.set(n!("impaleStoredHitAvg"), v);
                        } else {
                            // L3508
                            let v = output.num(n!("impaleStoredHitAvg"))
                                + damageTypeHitAvg * (1.0 - output.num(n!("CritChance")) / 100.0);
                            output.set(n!("impaleStoredHitAvg"), v);
                        }
                        // L3510
                        let mut enemyArmour =
                            m_max(calcLib::val(&enemyDB, n!("Armour"), None), 0.0);
                        // L3511
                        let mut armourReduction = calcs::armourReductionF(
                            enemyArmour,
                            damageTypeHitAvg
                                * skillModList.More(pcfg, &[n!("CalcArmourAsThoughDealing")]),
                        );
                        // L3512
                        let mut ChanceToIgnoreEnemyPhysicalDamageReduction = m_min(
                            skillModList.Sum(
                                ModType::BASE,
                                pcfg,
                                &[n!("ChanceToIgnoreEnemyPhysicalDamageReduction")],
                            ),
                            100.0,
                        );
                        // L3513
                        if ChanceToIgnoreEnemyPhysicalDamageReduction > 0.0
                            && ChanceToIgnoreEnemyPhysicalDamageReduction < 100.0
                        {
                            let mode = env
                                .configInput
                                .get(n!("ChanceToIgnoreEnemyPhysicalDamageReductionMode"));
                            // L3514
                            if matches!(&mode, Val::Str(s) if &**s == "MAX") {
                                // L3515
                                ChanceToIgnoreEnemyPhysicalDamageReduction = 100.0;
                            } else if matches!(&mode, Val::Str(s) if &**s == "MIN") {
                                // L3517
                                ChanceToIgnoreEnemyPhysicalDamageReduction = 0.0;
                            }
                        }
                        // L3520
                        if skillModList.Flag(pcfg, &[n!("IgnoreEnemyPhysicalDamageReduction")])
                            || ChanceToIgnoreEnemyPhysicalDamageReduction >= 100.0
                        {
                            // L3521
                            resist = 0.0;
                        } else {
                            // L3523
                            resist = m_min(
                                m_max(
                                    0.0,
                                    enemyDB.Sum(
                                        ModType::BASE,
                                        None,
                                        &[n!("PhysicalDamageReduction")],
                                    ) + skillModList.Sum(
                                        ModType::BASE,
                                        pcfg,
                                        &[n!("EnemyPhysicalDamageReduction")],
                                    ) + armourReduction,
                                ),
                                data.misc.EnemyPhysicalDamageReductionCap,
                            );
                            // L3524
                            resist = if resist > 0.0 {
                                resist
                                    * (1.0
                                        - (skillModList.Sum(
                                            ModType::BASE,
                                            None,
                                            &[n!("PartialIgnoreEnemyPhysicalDamageReduction")],
                                        ) / 100.0
                                            + ChanceToIgnoreEnemyPhysicalDamageReduction / 100.0))
                            } else {
                                resist
                            };
                        }
                    } else {
                        // L3527
                        resist = calcResistForType(ctx, damageType, None); // LUA-GLOBAL-NIL dotCfg
                        // L3528
                        if ((skillModList.Flag(pcfg, &[n!("ChaosDamageUsesLowestResistance")])
                            || skillModList
                                .Flag(pcfg, &[n!("ChaosDamageUsesHighestResistance")]))
                            && damageType == n!("Chaos"))
                            || (skillModList
                                .Flag(pcfg, &[n!("ElementalDamageUsesLowestResistance")])
                                && isElemental(damageType))
                        {
                            // Default to using the current damage type
                            // L3531
                            let mut elementUsed = damageType;
                            // L3532
                            if isElemental(damageType) {
                                // L3533
                                takenInc = takenInc
                                    + enemyDB.Sum(
                                        ModType::INC,
                                        pcfg,
                                        &[n!("ElementalDamageTaken")],
                                    );
                            }
                            // Find the lowest resist of all the elements and use that if it's lower
                            // L3536
                            for eleDamageType in dmgTypeList() {
                                // L3537
                                if isElemental(eleDamageType)
                                    && useThisResist(eleDamageType) > 0.0
                                    && damageType != eleDamageType
                                {
                                    // L3538
                                    let mut currentElementResist =
                                        calcResistForType(ctx, eleDamageType, None); // LUA-GLOBAL-NIL dotCfg
                                    // If it's explicitly lower, then use the resist and update which element we're using to account for penetration
                                    // L3540
                                    if skillModList
                                        .Flag(pcfg, &[n!("ChaosDamageUsesHighestResistance")])
                                    {
                                        // L3541
                                        if resist < currentElementResist {
                                            // L3542
                                            resist = currentElementResist;
                                            // L3543
                                            elementUsed = eleDamageType;
                                        }
                                    } else {
                                        // L3546
                                        if resist > currentElementResist {
                                            // L3547
                                            resist = currentElementResist;
                                            // L3548
                                            elementUsed = eleDamageType;
                                        }
                                    }
                                }
                            }
                            // Update the penetration based on the element used
                            // L3554
                            if isElemental(elementUsed) {
                                // L3555
                                pen = skillModList.Sum(
                                    ModType::BASE,
                                    pcfg,
                                    &[cat!(elementUsed, "Penetration"), n!("ElementalPenetration")],
                                );
                            } else if elementUsed == n!("Chaos") {
                                // L3557
                                pen = skillModList.Sum(
                                    ModType::BASE,
                                    pcfg,
                                    &[n!("ChaosPenetration")],
                                );
                            }
                            // L3559
                            sourceRes = elementUsed;
                        } else if isElemental(damageType) {
                            // L3561
                            pen = skillModList.Sum(
                                ModType::BASE,
                                pcfg,
                                &[cat!(damageType, "Penetration"), n!("ElementalPenetration")],
                            );
                            // L3562
                            takenInc = takenInc
                                + enemyDB.Sum(ModType::INC, pcfg, &[n!("ElementalDamageTaken")]);
                        } else if damageType == n!("Chaos") {
                            // L3564
                            pen = skillModList.Sum(ModType::BASE, pcfg, &[n!("ChaosPenetration")]);
                        }
                    }
                    // L3567
                    let mut invertChanceEle = m_max(
                        m_min(
                            skillModList.Sum(ModType::CHANCE, pcfg, &[n!("HitsInvertEleResChance")]),
                            1.0,
                        ),
                        0.0,
                    );
                    // L3568
                    let mut invertChanceChaos = m_max(
                        m_min(
                            skillModList.Sum(
                                ModType::CHANCE,
                                pcfg,
                                &[n!("HitsInvertChaosResChance")],
                            ),
                            1.0,
                        ),
                        0.0,
                    );
                    // L3569
                    let mut invertChance = if isElemental(damageType) {
                        invertChanceEle
                    } else if damageType == n!("Chaos") {
                        invertChanceChaos
                    } else {
                        0.0
                    };
                    // L3570
                    sourceRes = if env
                        .modDB
                        .get()
                        .Flag(None, &[cat!("Enemy", sourceRes, "ResistEqualToYours")])
                    {
                        cat!("Your ", sourceRes, " Resistance")
                    } else if env
                        .partyMembers
                        .get()
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index field 'partyMembers' (a nil value)")
                        })
                        .modDB
                        .get()
                        .Flag(None, &[cat!("Enemy", sourceRes, "ResistEqualToYours")])
                    {
                        cat!("Party Member ", sourceRes, " Resistance")
                    } else {
                        sourceRes
                    };
                    // L3571
                    if skillFlags.truthy(n!("projectile")) {
                        // L3572
                        takenInc = takenInc
                            + enemyDB.Sum(ModType::INC, None, &[n!("ProjectileDamageTaken")]);
                    }
                    // L3574
                    if skillFlags.truthy(n!("projectile")) && skillFlags.truthy(n!("attack")) {
                        // L3575
                        takenInc = takenInc
                            + enemyDB.Sum(ModType::INC, None, &[n!("ProjectileAttackDamageTaken")]);
                    }
                    // L3577
                    if skillFlags.truthy(n!("trap")) || skillFlags.truthy(n!("mine")) {
                        // L3578
                        takenInc = takenInc
                            + enemyDB.Sum(ModType::INC, None, &[n!("TrapMineDamageTaken")]);
                    }
                    // L3580
                    let mut effMult = (1.0 + takenInc / 100.0) * takenMore;
                    // L3581
                    let mut useResChance = useThisResist(damageType);
                    // L3582
                    let mut useRes = useResChance > 0.0;
                    // L3583
                    let mut effectiveResist = resist;
                    // L3584
                    if skillModList.Flag(
                        pcfg,
                        &names([isElemental(damageType).then(|| n!("CannotElePenIgnore"))]),
                    ) {
                        // L3585
                        effectiveResist = if invertChance > 0.0 {
                            resist - 2.0 * invertChance * resist
                        } else {
                            resist
                        };
                        // L3586
                        effMult = effMult * (1.0 - effectiveResist / 100.0);
                    } else if useRes {
                        // L3588
                        if invertChance > 0.0 {
                            // L3589
                            effectiveResist = ((resist - pen) * (1.0 - invertChance)
                                + (-resist - pen) * invertChance)
                                * useResChance;
                        } else {
                            // L3591
                            effectiveResist = (resist - pen) * useResChance;
                        }
                        // L3593
                        effMult = effMult * (1.0 - effectiveResist / 100.0);
                    }
                    // L3595
                    damageTypeHitMin = damageTypeHitMin * effMult;
                    // L3596
                    damageTypeHitMax = damageTypeHitMax * effMult;
                    // L3597
                    damageTypeHitAvg = damageTypeHitAvg * effMult;
                    // L3598
                    if env.mode == Mode::CALCS {
                        // L3599
                        output.set(cat!(damageType, "EffMult"), effMult);
                    }
                    // L3601
                    let showEffMult = pass == 2
                        && breakdown.is_some()
                        && (effMult != 1.0 || sourceRes != damageType || invertChance > 0.0);
                    if showEffMult
                        && skillModList.Flag(
                            pcfg,
                            &names([isElemental(damageType).then(|| n!("CannotElePenIgnore"))]),
                        )
                    {
                        let breakdown = breakdown.as_ref().unwrap();
                        // L3602
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("x %.3f ^8(effective DPS modifier)", effMult),
                        );
                        // L3603
                        let t = breakdown.effMult(
                            damageType,
                            resist,
                            0.0,
                            takenInc,
                            effMult,
                            takenMore,
                            Some(sourceRes),
                            useRes,
                            Some(invertChance),
                            Some(effectiveResist),
                        );
                        breakdown.set(cat!(damageType, "EffMult"), t);
                    } else if showEffMult {
                        let breakdown = breakdown.as_ref().unwrap();
                        // L3605
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("x %.3f ^8(effective DPS modifier)", effMult),
                        );
                        // L3606
                        let t = breakdown.effMult(
                            damageType,
                            resist,
                            pen,
                            takenInc,
                            effMult,
                            takenMore,
                            Some(sourceRes),
                            useRes,
                            Some(invertChance),
                            Some(effectiveResist),
                        );
                        breakdown.set(cat!(damageType, "EffMult"), t);
                    }
                }
                // L3609
                if pass == 2 {
                    if let Some(breakdown) = &breakdown {
                        // L3610
                        ins(
                            breakdown,
                            damageType,
                            lfmt!("= %d to %d", damageTypeHitMin, damageTypeHitMax),
                        );
                    }
                }

                // Beginning of Leech Calculation for this DamageType
                // L3614
                let mut lifeLeech = 0.0;
                // L3615
                let mut energyShieldLeech = 0.0;
                // L3616
                let mut manaLeech = 0.0;
                // L3617
                if skillFlags.truthy(n!("mine"))
                    || skillFlags.truthy(n!("trap"))
                    || skillFlags.truthy(n!("totem"))
                {
                    // L3618
                    lifeLeech =
                        skillModList.Sum(ModType::BASE, pcfg, &[n!("DamageLifeLeechToPlayer")]);
                } else {
                    // L3620
                    if skillModList.Flag(None, &[n!("LifeLeechBasedOnChaosDamage")]) {
                        // L3621
                        if damageType == n!("Chaos") {
                            // L3622
                            lifeLeech = skillModList.Sum(
                                ModType::BASE,
                                pcfg,
                                nm![
                                    "DamageLeech",
                                    "DamageLifeLeech",
                                    "PhysicalDamageLifeLeech",
                                    "LightningDamageLifeLeech",
                                    "ColdDamageLifeLeech",
                                    "FireDamageLifeLeech",
                                    "ChaosDamageLifeLeech",
                                    "ElementalDamageLifeLeech"
                                ],
                            ) + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfDamageLifeLeech")])
                                / 100.0;
                        }
                    } else {
                        // L3625
                        lifeLeech = skillModList.Sum(
                            ModType::BASE,
                            pcfg,
                            &names([
                                Some(n!("DamageLeech")),
                                Some(n!("DamageLifeLeech")),
                                Some(cat!(damageType, "DamageLifeLeech")),
                                isElemental(damageType).then(|| n!("ElementalDamageLifeLeech")),
                            ]),
                        ) + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfDamageLifeLeech")])
                            / 100.0;
                    }
                    // L3627
                    energyShieldLeech = skillModList.Sum(
                        ModType::BASE,
                        pcfg,
                        &names([
                            Some(n!("DamageEnergyShieldLeech")),
                            Some(cat!(damageType, "DamageEnergyShieldLeech")),
                            isElemental(damageType).then(|| n!("ElementalDamageEnergyShieldLeech")),
                        ]),
                    ) + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfDamageEnergyShieldLeech")])
                        / 100.0;
                    // L3628
                    manaLeech = skillModList.Sum(
                        ModType::BASE,
                        pcfg,
                        &names([
                            Some(n!("DamageLeech")),
                            Some(n!("DamageManaLeech")),
                            Some(cat!(damageType, "DamageManaLeech")),
                            isElemental(damageType).then(|| n!("ElementalDamageManaLeech")),
                        ]),
                    ) + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfDamageManaLeech")])
                        / 100.0;
                }

                // L3631
                if lifeLeech > 0.0 && !noLifeLeech {
                    // L3632
                    lifeLeechTotal = lifeLeechTotal + damageTypeHitAvg * lifeLeech / 100.0;
                }
                // L3634
                if manaLeech > 0.0 && !noManaLeech {
                    // L3635
                    manaLeechTotal = manaLeechTotal + damageTypeHitAvg * manaLeech / 100.0;
                }
                // L3637
                if energyShieldLeech > 0.0 && !noEnergyShieldLeech {
                    // L3638
                    energyShieldLeechTotal =
                        energyShieldLeechTotal + damageTypeHitAvg * energyShieldLeech / 100.0;
                }
            } else {
                // L3641
                if let Some(breakdown) = &breakdown {
                    // L3642
                    let line: Val = cat!("You can't deal ", damageType, " damage").into();
                    breakdown.set(damageType, tbl![line]);
                }
            }
            // L3647
            if pass == 1 {
                // L3648
                output.set(cat!(damageType, "CritAverage"), damageTypeHitAvg);
                // L3649
                totalCritAvg = totalCritAvg + damageTypeHitAvg;
                // L3650
                totalCritMin = totalCritMin + damageTypeHitMin;
                // L3651
                totalCritMax = totalCritMax + damageTypeHitMax;
            } else {
                // L3653
                if env.mode == Mode::CALCS {
                    // L3654
                    output.set(cat!(damageType, "Min"), damageTypeHitMin);
                    // L3655
                    output.set(cat!(damageType, "Max"), damageTypeHitMax);
                }
                // L3657
                output.set(cat!(damageType, "HitAverage"), damageTypeHitAvg);
                // L3658
                totalHitAvg = totalHitAvg + damageTypeHitAvg;
                // L3659
                totalHitMin = totalHitMin + damageTypeHitMin;
                // L3660
                totalHitMax = totalHitMax + damageTypeHitMax;
            }
        }
        // L3663
        if skillData.truthy(n!("lifeLeechPerUse")) && !noLifeLeech {
            // L3664
            lifeLeechTotal = lifeLeechTotal + skillData.num(n!("lifeLeechPerUse"));
        }
        // L3666
        if skillData.truthy(n!("manaLeechPerUse")) {
            // L3667
            manaLeechTotal = manaLeechTotal + skillData.num(n!("manaLeechPerUse"));
        }

        // leech caps per instance
        // L3671
        if ghostReaver {
            // L3672
            lifeLeechTotal = m_min(
                lifeLeechTotal,
                globalOutput.num(n!("MaxEnergyShieldLeechInstance")),
            );
        } else {
            // L3674
            lifeLeechTotal = m_min(lifeLeechTotal, globalOutput.num(n!("MaxLifeLeechInstance")));
        }
        // L3676
        energyShieldLeechTotal = m_min(
            energyShieldLeechTotal,
            globalOutput.num(n!("MaxEnergyShieldLeechInstance")),
        );
        // L3677
        manaLeechTotal = m_min(manaLeechTotal, globalOutput.num(n!("MaxManaLeechInstance")));
        // L3678
        if ghostReaver && noEnergyShieldLeech {
            // L3679
            lifeLeechTotal = 0.0;
        }

        // L3682
        let mut portion = if pass == 1 {
            output.num(n!("CritChance")) / 100.0
        } else {
            1.0 - output.num(n!("CritChance")) / 100.0
        };
        // L3683
        if ghostReaver {
            // L3684
            ghostReaverLifeLeech = ghostReaverLifeLeech + lifeLeechTotal * portion;
        } else {
            // L3686
            let v = output.num(n!("LifeLeech")) + lifeLeechTotal * portion;
            output.set(n!("LifeLeech"), v);
        }
        // L3688
        let v = output.num(n!("EnergyShieldLeech")) + energyShieldLeechTotal * portion;
        output.set(n!("EnergyShieldLeech"), v);
        // L3689
        let v = output.num(n!("ManaLeech")) + manaLeechTotal * portion;
        output.set(n!("ManaLeech"), v);
    }
    // L3691
    output.set(n!("TotalMin"), totalHitMin);
    // L3692
    output.set(n!("TotalMax"), totalHitMax);

    // L3694
    if skillModList.Flag(scfg, &[n!("ElementalEquilibrium")])
        && !env.configInput.truthy(n!("EEIgnoreHitDamage"))
        && (output.num(n!("FireHitAverage"))
            + output.num(n!("ColdHitAverage"))
            + output.num(n!("LightningHitAverage"))
            > 0.0)
    {
        // Update enemy hit-by-damage-type conditions
        // L3696
        enemyDB.conditions.set(
            n!("HitByFireDamage"),
            output.num(n!("FireHitAverage")) > 0.0,
        );
        // L3697
        enemyDB.conditions.set(
            n!("HitByColdDamage"),
            output.num(n!("ColdHitAverage")) > 0.0,
        );
        // L3698
        enemyDB.conditions.set(
            n!("HitByLightningDamage"),
            output.num(n!("LightningHitAverage")) > 0.0,
        );
    }

    // L3701
    let mut highestType = n!("Physical");

    // For each damage type, calculate percentage of total damage. Also tracks the highest damage type and outputs a Condition:TypeIsHighestDamageType flag for whichever the highest type is
    // L3704
    for damageType in dmgTypeList() {
        // L3705
        if output.num(cat!(damageType, "HitAverage")) > 0.0 {
            // L3706
            let mut portion = output.num(cat!(damageType, "HitAverage")) / totalHitAvg * 100.0;
            // L3707
            skillModList.NewMod(
                cat!("Condition:", damageType, "HasDamage"),
                ModType::FLAG,
                true,
                "Config",
                0,
                0,
                &[],
            );
            // L3708
            if output.num(cat!(damageType, "HitAverage"))
                > output.num(cat!(highestType, "HitAverage"))
            {
                // L3709
                highestType = damageType;
            }
            // L3711
            if let Some(breakdown) = &breakdown {
                // L3712
                ins(
                    breakdown,
                    damageType,
                    lfmt!("Portion of total damage: %d%%", portion),
                );
            }
        }
    }
    // L3716
    if !skillModList.Flag(None, &[n!("IsHighestDamageTypeOVERRIDE")]) {
        // L3717
        skillModList.NewMod(
            cat!("Condition:", highestType, "IsHighestDamageType"),
            ModType::FLAG,
            true,
            "Config",
            0,
            0,
            &[],
        );
    }

    // Calculate leech
    // L3721
    let getLeechInstances = |amount: f64, total: f64| -> (f64, f64) {
        // L3722
        if total == 0.0 {
            // L3723
            return (0.0, 0.0);
        }
        // L3725
        let mut duration = amount / total / data.misc.LeechRateBase;
        // L3726
        (duration, duration * hitRate())
    };

    //Instant Leech
    // L3730
    let v = m_max(
        m_min(skillModList.Sum(ModType::BASE, pcfg, &[n!("InstantLifeLeech")]), 100.0),
        0.0,
    ) / 100.0;
    output.set(n!("LifeLeechInstantProportion"), v);
    // L3731
    if output.num(n!("LifeLeechInstantProportion")) > 0.0 {
        // L3732
        let v = output.num(n!("LifeLeech")) * output.num(n!("LifeLeechInstantProportion"));
        output.set(n!("LifeLeechInstant"), v);
        // L3733
        let v =
            output.num(n!("LifeLeech")) * (1.0 - output.num(n!("LifeLeechInstantProportion")));
        output.set(n!("LifeLeech"), v);
    }
    // L3735
    if ghostReaver && ghostReaverLifeLeech > 0.0 {
        // L3736
        let v = output.num(n!("EnergyShieldLeech")) + ghostReaverLifeLeech;
        output.set(n!("EnergyShieldLeech"), v);
        // L3737
        output.set(n!("LifeLeech"), 0.0);
        // L3738
        output.set(n!("LifeLeechInstant"), 0.0);
    }
    // L3740
    let v = m_max(
        m_min(
            skillModList.Sum(ModType::BASE, pcfg, &[n!("InstantEnergyShieldLeech")]),
            100.0,
        ),
        0.0,
    ) / 100.0;
    output.set(n!("EnergyShieldLeechInstantProportion"), v);
    // L3741
    if output.num(n!("EnergyShieldLeechInstantProportion")) > 0.0 {
        // L3742
        let v = output.num(n!("EnergyShieldLeech"))
            * output.num(n!("EnergyShieldLeechInstantProportion"));
        output.set(n!("EnergyShieldLeechInstant"), v);
        // L3743
        let v = output.num(n!("EnergyShieldLeech"))
            * (1.0 - output.num(n!("EnergyShieldLeechInstantProportion")));
        output.set(n!("EnergyShieldLeech"), v);
    }
    // L3745
    let v = m_max(
        m_min(skillModList.Sum(ModType::BASE, pcfg, &[n!("InstantManaLeech")]), 100.0),
        0.0,
    ) / 100.0;
    output.set(n!("ManaLeechInstantProportion"), v);
    // L3746
    if output.num(n!("ManaLeechInstantProportion")) > 0.0 {
        // L3747
        let v = output.num(n!("ManaLeech")) * output.num(n!("ManaLeechInstantProportion"));
        output.set(n!("ManaLeechInstant"), v);
        // L3748
        let v =
            output.num(n!("ManaLeech")) * (1.0 - output.num(n!("ManaLeechInstantProportion")));
        output.set(n!("ManaLeech"), v);
    }

    // `a, b = f()` stores right to left in LuaJIT (lj_parse.c parse_assignment): b first.
    // L3751
    let (d, i) = getLeechInstances(output.num(n!("LifeLeech")), globalOutput.num(n!("Life")));
    output.set(n!("LifeLeechInstances"), i);
    output.set(n!("LifeLeechDuration"), d);
    // L3752
    let v = output.num(n!("LifeLeechInstant")) * hitRate();
    output.set(n!("LifeLeechInstantRate"), v);
    // L3753
    let (d, i) = getLeechInstances(
        output.num(n!("EnergyShieldLeech")),
        globalOutput.num(n!("EnergyShield")),
    );
    output.set(n!("EnergyShieldLeechInstances"), i);
    output.set(n!("EnergyShieldLeechDuration"), d);
    // L3754
    let v = output.num(n!("EnergyShieldLeechInstant")) * hitRate();
    output.set(n!("EnergyShieldLeechInstantRate"), v);
    // L3755
    let (d, i) = getLeechInstances(output.num(n!("ManaLeech")), globalOutput.num(n!("Mana")));
    output.set(n!("ManaLeechInstances"), i);
    output.set(n!("ManaLeechDuration"), d);
    // L3756
    let v = output.num(n!("ManaLeechInstant")) * hitRate();
    output.set(n!("ManaLeechInstantRate"), v);

    // Calculate gain on hit
    // L3759
    if skillFlags.truthy(n!("mine"))
        || skillFlags.truthy(n!("trap"))
        || skillFlags.truthy(n!("totem"))
    {
        // L3760
        output.set(n!("LifeOnHit"), 0.0);
        // L3761
        output.set(n!("EnergyShieldOnHit"), 0.0);
        // L3762
        output.set(n!("ManaOnHit"), 0.0);
    } else {
        // L3764
        let v = if !skillModList.Flag(pcfg, &[n!("CannotGainLife")])
            && !skillModList.Flag(pcfg, &[n!("CannotRecoverLifeOutsideLeech")])
        {
            skillModList.Sum(ModType::BASE, pcfg, &[n!("LifeOnHit")])
                + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfLifeOnHit")])
        } else {
            0.0
        };
        output.set(n!("LifeOnHit"), v);
        // L3765
        let v = if !skillModList.Flag(pcfg, &[n!("CannotGainEnergyShield")]) {
            skillModList.Sum(ModType::BASE, pcfg, &[n!("EnergyShieldOnHit")])
                + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfEnergyShieldOnHit")])
        } else {
            0.0
        };
        output.set(n!("EnergyShieldOnHit"), v);
        // L3766
        let v = if !skillModList.Flag(pcfg, &[n!("CannotGainMana")]) {
            skillModList.Sum(ModType::BASE, pcfg, &[n!("ManaOnHit")])
                + enemyDB.Sum(ModType::BASE, pcfg, &[n!("SelfManaOnHit")])
        } else {
            0.0
        };
        output.set(n!("ManaOnHit"), v);
    }
    // L3768
    let v = output.num(n!("LifeOnHit")) * hitRate();
    output.set(n!("LifeOnHitRate"), v);
    // L3769
    let v = output.num(n!("EnergyShieldOnHit")) * hitRate();
    output.set(n!("EnergyShieldOnHitRate"), v);
    // L3770
    let v = output.num(n!("ManaOnHit")) * hitRate();
    output.set(n!("ManaOnHitRate"), v);

    // Calculate gain on kill
    // L3773
    if skillFlags.truthy(n!("mine"))
        || skillFlags.truthy(n!("trap"))
        || skillFlags.truthy(n!("totem"))
    {
        // L3774
        output.set(n!("LifeOnKill"), 0.0);
        // L3775
        output.set(n!("EnergyShieldOnKill"), 0.0);
        // L3776
        output.set(n!("ManaOnKill"), 0.0);
    } else {
        // L3778
        let v = if !skillModList.Flag(pcfg, &[n!("CannotGainLife")])
            && !skillModList.Flag(pcfg, &[n!("CannotRecoverLifeOutsideLeech")])
        {
            m_floor(skillModList.Sum(ModType::BASE, pcfg, &[n!("LifeOnKill")]))
        } else {
            0.0
        };
        output.set(n!("LifeOnKill"), v);
        // L3779
        let v = if !skillModList.Flag(pcfg, &[n!("CannotGainEnergyShield")]) {
            m_floor(skillModList.Sum(ModType::BASE, pcfg, &[n!("EnergyShieldOnKill")]))
        } else {
            0.0
        };
        output.set(n!("EnergyShieldOnKill"), v);
        // L3780
        let v = if !skillModList.Flag(pcfg, &[n!("CannotGainMana")]) {
            m_floor(skillModList.Sum(ModType::BASE, pcfg, &[n!("ManaOnKill")]))
        } else {
            0.0
        };
        output.set(n!("ManaOnKill"), v);
    }

    // Enemy Regeneration Rate
    // L3784
    let v = enemyDB.Sum(ModType::INC, pcfg, &[n!("LifeRegen")]);
    output.set(n!("EnemyLifeRegen"), v);
    // L3785
    let v = enemyDB.Sum(ModType::INC, pcfg, &[n!("ManaRegen")]);
    output.set(n!("EnemyManaRegen"), v);
    // L3786
    let v = enemyDB.Sum(ModType::INC, pcfg, &[n!("EnergyShieldRegen")]);
    output.set(n!("EnemyEnergyShieldRegen"), v);

    // Calculate average damage and final DPS
    // L3789
    let v = totalHitAvg * (1.0 - output.num(n!("CritChance")) / 100.0)
        + totalCritAvg * output.num(n!("CritChance")) / 100.0;
    output.set(n!("AverageHit"), v);
    // L3790
    if skillFlags.truthy(n!("monsterExplode")) {
        // L3791
        let v = output.num(n!("AverageHit")) / monsterLife * 100.0;
        output.set(n!("AverageHitToMonsterLifePercentage"), v);
        // L3792
        if skillData.truthy(n!("hitChanceIsExplodeChance")) {
            // L3793
            let v = output.get(n!("ExplodeChance"));
            output.set(n!("HitChance"), v);
        }
    }
    // L3796
    let v = output.num(n!("AverageHit")) * output.num(n!("HitChance")) / 100.0;
    output.set(n!("AverageDamage"), v);
    // L3797
    let v = output.num_or(n!("AverageBurstHits"), 1.0);
    globalOutput.set(n!("AverageBurstHits"), v);
    // L3798
    let mut repeatPenalty = if skillModList.Flag(None, &[n!("HasSeals")])
        && activeSkill
            .skillTypes
            .get()
            .with(|s| s.has(SkillType::CanRapidFire))
        && !skillModList.Flag(None, &[n!("NoRepeatBonuses")])
    {
        calcLib::r#mod(&skillModList, scfg, &[n!("SealRepeatPenalty")])
    } else {
        1.0
    };
    // L3799
    let v = output.num(n!("AverageDamage"))
        + output.num(n!("AverageDamage"))
            * (globalOutput.num(n!("AverageBurstHits")) - 1.0)
            * repeatPenalty;
    globalOutput.set(n!("AverageBurstDamage"), v);
    // L3800
    let v = globalOutput.num(n!("AverageBurstHits")) > 1.0;
    globalOutput.set(n!("ShowBurst"), v);
    // L3801
    let v = output.num(n!("AverageDamage"))
        * num_or_else(globalOutput.get(n!("HitSpeed")), || {
            globalOutput.get(n!("Speed"))
        })
        * skillData.num(n!("dpsMultiplier"))
        * quantityMultiplier;
    output.set(n!("TotalDPS"), v);
    // L3802
    if let Some(breakdown) = &breakdown {
        // L3803
        if ne(&output.get(n!("CritEffect")), 1.0) {
            // L3804
            breakdown.set(n!("AverageHit"), Table::new());
            // L3805
            let mut damageRolls = output.num_or(n!("DamageRolls"), 0.0);
            // L3806
            let mut critRolls = output.num_or(n!("CritDamageRolls"), 0.0);
            // L3807
            let mut rolls = m_abs(damageRolls) + 2.0;
            // L3808
            let mut rollsCrit = m_abs(critRolls) + 2.0;
            // L3809
            if damageRolls > 0.0 {
                // L3810
                ins(
                    breakdown,
                    n!("AverageHit"),
                    lfmt!(
                        "(1/%d) x %d + (%d/%d) x %d = %.1f ^8(average from non-crits)",
                        rolls,
                        totalHitMin,
                        rolls - 1.0,
                        rolls,
                        totalHitMax,
                        totalHitAvg
                    ),
                );
            }
            // L3812
            if damageRolls < 0.0 {
                // L3813
                ins(
                    breakdown,
                    n!("AverageHit"),
                    lfmt!(
                        "(%d/%d) x %d + (1/%d) x %d = %.1f ^8(average from non-crits)",
                        rolls - 1.0,
                        rolls,
                        totalHitMin,
                        rolls,
                        totalHitMax,
                        totalHitAvg
                    ),
                );
            }
            // L3815
            if damageRolls > 0.0
                || critRolls > 0.0
                || skillModList.Flag(scfg, &[n!("CritLucky")])
            {
                // L3816
                ins(
                    breakdown,
                    n!("AverageHit"),
                    lfmt!(
                        "(1/%d) x %d + (%d/%d) x %d = %.1f ^8(average from crits)",
                        rollsCrit,
                        totalCritMin,
                        rollsCrit - 1.0,
                        rollsCrit,
                        totalCritMax,
                        totalCritAvg
                    ),
                );
                // L3817
                ins(breakdown, n!("AverageHit"), "");
            }
            // L3819
            if damageRolls < 0.0 && !skillModList.Flag(scfg, &[n!("CritLucky")]) {
                // L3820
                ins(
                    breakdown,
                    n!("AverageHit"),
                    lfmt!(
                        "(%d/%d) x %d + (1/%d) x %d = %.1f ^8(average from crits)",
                        rolls - 1.0,
                        rolls,
                        totalCritMin,
                        rolls,
                        totalCritMax,
                        totalCritAvg
                    ),
                );
                // L3821
                ins(breakdown, n!("AverageHit"), "");
            }
            // L3823
            ins(
                breakdown,
                n!("AverageHit"),
                lfmt!(
                    "%.1f x (1 - %.4f) ^8(damage from non-crits)",
                    totalHitAvg,
                    output.num(n!("CritChance")) / 100.0
                ),
            );
            // L3824
            ins(
                breakdown,
                n!("AverageHit"),
                lfmt!(
                    "+ %.1f x %.4f ^8(damage from crits)",
                    totalCritAvg,
                    output.num(n!("CritChance")) / 100.0
                ),
            );
            // L3825
            ins(
                breakdown,
                n!("AverageHit"),
                lfmt!("= %.1f", fa(&output.get(n!("AverageHit")))),
            );
        }
        // L3827
        if output.num(n!("HitChance")) < 100.0 {
            // L3828
            breakdown.set(n!("AverageDamage"), Table::new());
            // L3829
            ins(
                breakdown,
                n!("AverageDamage"),
                lfmt!("%s:", fa(&pass.get(n!("label")))),
            );
            // L3830
            ins(
                breakdown,
                n!("AverageDamage"),
                lfmt!("%.1f ^8(average hit)", fa(&output.get(n!("AverageHit")))),
            );
            // L3831
            ins(
                breakdown,
                n!("AverageDamage"),
                lfmt!(
                    "x %.2f ^8(chance to hit)",
                    output.num(n!("HitChance")) / 100.0
                ),
            );
            // L3832
            ins(
                breakdown,
                n!("AverageDamage"),
                lfmt!("= %.1f", fa(&output.get(n!("AverageDamage")))),
            );
        }
    }
    // L3835
    if let Some(globalBreakdown) = globalBreakdown
        .as_ref()
        .filter(|_| globalOutput.num(n!("AverageBurstDamage")) > 0.0)
    {
        // L3836
        globalBreakdown.set(n!("AverageBurstDamage"), Table::new());
        // L3837
        ins(
            globalBreakdown,
            n!("AverageBurstDamage"),
            lfmt!("%.1f ^8(average hit)", fa(&output.get(n!("AverageHit")))),
        );
        // L3838
        if output.num(n!("HitChance")) < 100.0 {
            // L3839
            ins(
                globalBreakdown,
                n!("AverageBurstDamage"),
                lfmt!(
                    "x %.2f ^8(chance to hit)",
                    output.num(n!("HitChance")) / 100.0
                ),
            );
        }
        // L3841
        if repeatPenalty < 1.0 {
            // L3842
            ins(
                globalBreakdown,
                n!("AverageBurstDamage"),
                lfmt!(
                    "x %.2f ^8(number of repeats)",
                    globalOutput.num(n!("AverageBurstHits")) - 1.0
                ),
            );
            // L3843
            ins(
                globalBreakdown,
                n!("AverageBurstDamage"),
                lfmt!("x %.2f ^8(repeat penalty)", repeatPenalty),
            );
            // L3844
            ins(
                globalBreakdown,
                n!("AverageBurstDamage"),
                lfmt!(
                    "= %.1f ^8(repeat damage total)",
                    globalOutput.num(n!("AverageBurstDamage")) - output.num(n!("AverageDamage"))
                ),
            );
            // L3845
            ins(globalBreakdown, n!("AverageBurstDamage"), "");
            // L3846
            ins(
                globalBreakdown,
                n!("AverageBurstDamage"),
                lfmt!("+ %.1f ^8(first hit)", fa(&output.get(n!("AverageHit")))),
            );
            // L3847
            if output.num(n!("HitChance")) < 100.0 {
                // L3848
                ins(
                    globalBreakdown,
                    n!("AverageBurstDamage"),
                    lfmt!(
                        "x %.2f ^8(chance to hit)",
                        output.num(n!("HitChance")) / 100.0
                    ),
                );
            }
        } else {
            // L3851
            ins(
                globalBreakdown,
                n!("AverageBurstDamage"),
                lfmt!(
                    "x %.2f ^8(number of hits)",
                    fa(&globalOutput.get(n!("AverageBurstHits")))
                ),
            );
        }
        // L3853
        ins(
            globalBreakdown,
            n!("AverageBurstDamage"),
            lfmt!(
                "= %.1f ^8(total burst damage)",
                fa(&globalOutput.get(n!("AverageBurstDamage")))
            ),
        );
    }

    // Calculate PvP values

    //setup flags
    // L3860
    skillFlags.set(n!("isPvP"), false);
    // L3861
    skillFlags.set(n!("notAttackPvP"), false);
    // L3862
    skillFlags.set(n!("attackPvP"), false);
    // L3863
    skillFlags.set(n!("weapon1AttackPvP"), false);
    // L3864
    skillFlags.set(n!("weapon2AttackPvP"), false);
    // L3865
    skillFlags.set(n!("notAveragePvP"), false);

    // L3867
    if env.configInput.truthy(n!("PvpScaling")) {
        // L3868
        skillFlags.set(n!("isPvP"), true);
        // L3869
        let v = skillFlags.get(n!("attack"));
        skillFlags.set(n!("attackPvP"), v);
        // L3870
        let v = !skillFlags.truthy(n!("attack"));
        skillFlags.set(n!("notAttackPvP"), v);
        // L3871
        let v = skillFlags.get(n!("weapon1Attack"));
        skillFlags.set(n!("weapon1AttackPvP"), v);
        // L3872
        let v = skillFlags.get(n!("weapon2Attack"));
        skillFlags.set(n!("weapon2AttackPvP"), v);
        // L3873
        let v = skillFlags.get(n!("notAverage"));
        skillFlags.set(n!("notAveragePvP"), v);
        // L3874
        let mut PvpTvalue = lor(env.configInput.get(n!("multiplierPvpTvalueOverride")), || {
            Val::Nil
        });
        // L3875
        if PvpTvalue.truthy() {
            // L3876
            PvpTvalue = Val::Num(PvpTvalue.num() / 1000.0);
        } else {
            // L3878
            if skillData.truthy(n!("cooldown")) {
                // L3879
                PvpTvalue = skillData.get(n!("cooldown"));
            } else if skillFlags.truthy(n!("mine")) {
                // L3881
                PvpTvalue = Val::Num(
                    output.num_or(n!("MineLayingTime"), 1.0)
                        / globalOutput.num(n!("ActionSpeedMod")),
                );
            } else if skillFlags.truthy(n!("trap")) {
                // L3883
                PvpTvalue = Val::Num(
                    output.num_or(n!("TrapThrowingTime"), 1.0)
                        / globalOutput.num(n!("ActionSpeedMod")),
                );
            } else {
                // L3885
                PvpTvalue = Val::Num(
                    1.0 / (num_or_else(globalOutput.get(n!("HitSpeed")), || {
                        globalOutput.get(n!("Speed"))
                    }) / globalOutput.num(n!("ActionSpeedMod")))
                        * skillModList.More(pcfg, &[n!("PvpTvalueMultiplier")]),
                );
            }
            // L3887
            if PvpTvalue.num() > 2147483647.0 {
                // L3888
                PvpTvalue = Val::Num(1.0);
            }
        }
        let PvpTvalue = PvpTvalue.num();
        // L3891
        let mut PvpMultiplier = skillModList.More(pcfg, &[n!("PvpDamageMultiplier")]);

        // L3893
        let mut PvpNonElemental1 = data.misc.PvpNonElemental1;
        // L3894
        let mut PvpNonElemental2 = data.misc.PvpNonElemental2;
        // L3895
        let mut PvpElemental1 = data.misc.PvpElemental1;
        // L3896
        let mut PvpElemental2 = data.misc.PvpElemental2;

        // L3898
        let mut percentageNonElemental = (output.num(n!("PhysicalHitAverage"))
            + output.num(n!("ChaosHitAverage")))
            / (totalHitMin + totalHitMax)
            * 2.0;
        // L3899
        let mut percentageElemental = 1.0 - percentageNonElemental;
        // L3900
        let mut portionNonElemental = lpow(
            output.num(n!("AverageHit")) / PvpTvalue / PvpNonElemental2,
            PvpNonElemental1,
        ) * PvpTvalue
            * PvpNonElemental2
            * percentageNonElemental;
        // L3901
        let mut portionElemental = lpow(
            output.num(n!("AverageHit")) / PvpTvalue / PvpElemental2,
            PvpElemental1,
        ) * PvpTvalue
            * PvpElemental2
            * percentageElemental;
        // L3902
        let v = (portionNonElemental + portionElemental) * PvpMultiplier;
        output.set(n!("PvpAverageHit"), v);
        // L3903
        let v = output.num(n!("PvpAverageHit")) * output.num(n!("HitChance")) / 100.0;
        output.set(n!("PvpAverageDamage"), v);
        // L3904
        let v = output.num(n!("PvpAverageDamage"))
            * num_or_else(globalOutput.get(n!("HitSpeed")), || {
                globalOutput.get(n!("Speed"))
            })
            * skillData.num(n!("dpsMultiplier"));
        output.set(n!("PvpTotalDPS"), v);

        // fix for these being nan
        // L3907
        if output.num(n!("PvpAverageHit")).is_nan() {
            // L3908
            output.set(n!("PvpAverageHit"), 0.0);
        }
        // L3910
        if output.num(n!("PvpAverageDamage")).is_nan() {
            // L3911
            output.set(n!("PvpAverageDamage"), 0.0);
        }
        // L3913
        if output.num(n!("PvpTotalDPS")).is_nan() {
            // L3914
            output.set(n!("PvpTotalDPS"), 0.0);
        }

        // L3917
        if let Some(breakdown) = &breakdown {
            // L3918
            breakdown.set(n!("PvpAverageHit"), Table::new());
            // L3919
            let mut percentBoth = percentageNonElemental > 0.0 && percentageElemental > 0.0;
            // L3920
            ins(
                breakdown,
                n!("PvpAverageHit"),
                lfmt!(
                    "Pvp Formula is (D/(T*M))^E*T*%s, where D is the damage, T is the time taken,",
                    if percentBoth { "M*P" } else { "M" }
                ),
            );
            // L3921
            ins(
                breakdown,
                n!("PvpAverageHit"),
                lfmt!(
                    " M is the multiplier%s",
                    if percentBoth {
                        ", E is the exponent and P is the percentage of that type (ele or non ele)"
                    } else {
                        " and E is the exponent"
                    }
                ),
            );
            // L3922
            if percentBoth {
                // L3923
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(M= %.1f for ele and %.1f for non-ele)(E= %.2f for ele and %.2f for non-ele)",
                        PvpElemental2,
                        PvpNonElemental2,
                        PvpElemental1,
                        PvpNonElemental1
                    ),
                );
                // L3924
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f * %.2f = %.1f",
                        fa(&output.get(n!("AverageHit"))),
                        PvpTvalue,
                        PvpNonElemental2,
                        PvpNonElemental1,
                        PvpTvalue,
                        PvpNonElemental2,
                        percentageNonElemental,
                        portionNonElemental
                    ),
                );
                // L3925
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f * %.2f = %.1f",
                        fa(&output.get(n!("AverageHit"))),
                        PvpTvalue,
                        PvpElemental2,
                        PvpElemental1,
                        PvpTvalue,
                        PvpElemental2,
                        percentageElemental,
                        portionElemental
                    ),
                );
                // L3926
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(portionNonElemental + portionElemental)%s",
                        if PvpMultiplier != 1.0 { " * PvP multiplier" } else { " " }
                    ),
                );
                // L3927
                if PvpMultiplier != 1.0 {
                    // L3928
                    ins(
                        breakdown,
                        n!("PvpAverageHit"),
                        lfmt!(
                            "(%.1f + %.1f) * %g",
                            portionNonElemental,
                            portionElemental,
                            PvpMultiplier
                        ),
                    );
                } else {
                    // L3930
                    ins(
                        breakdown,
                        n!("PvpAverageHit"),
                        lfmt!("%.1f + %.1f", portionNonElemental, portionElemental),
                    );
                }
            } else if percentageElemental <= 0.0 {
                // L3933
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(M= %.1f for non-ele)(E= %.2f for non-ele)",
                        PvpNonElemental2,
                        PvpNonElemental1
                    ),
                );
                // L3934
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f = %.1f",
                        fa(&output.get(n!("AverageHit"))),
                        PvpTvalue,
                        PvpNonElemental2,
                        PvpNonElemental1,
                        PvpTvalue,
                        PvpNonElemental2,
                        portionNonElemental
                    ),
                );
                // L3935
                if PvpMultiplier != 1.0 {
                    // L3936
                    ins(
                        breakdown,
                        n!("PvpAverageHit"),
                        lfmt!(
                            "%.1f * %g ^8(portionNonElemental * PvP multiplier)",
                            portionNonElemental,
                            PvpMultiplier
                        ),
                    );
                }
            } else if percentageNonElemental <= 0.0 {
                // L3939
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(M= %.1f for ele)(E= %.2f for ele)",
                        PvpElemental2,
                        PvpElemental1
                    ),
                );
                // L3940
                ins(
                    breakdown,
                    n!("PvpAverageHit"),
                    lfmt!(
                        "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f = %.1f",
                        fa(&output.get(n!("AverageHit"))),
                        PvpTvalue,
                        PvpElemental2,
                        PvpElemental1,
                        PvpTvalue,
                        PvpElemental2,
                        portionElemental
                    ),
                );
                // L3941
                if PvpMultiplier != 1.0 {
                    // L3942
                    ins(
                        breakdown,
                        n!("PvpAverageHit"),
                        lfmt!(
                            "%.1f * %g ^8(portionElemental * PvP multiplier)",
                            portionElemental,
                            PvpMultiplier
                        ),
                    );
                }
            }
            // L3945
            ins(
                breakdown,
                n!("PvpAverageHit"),
                lfmt!("= %.1f", fa(&output.get(n!("PvpAverageHit")))),
            );
            // L3946
            if isAttack {
                // L3947
                breakdown.set(n!("PvpAverageDamage"), Table::new());
                // L3948
                ins(
                    breakdown,
                    n!("PvpAverageDamage"),
                    lfmt!("%s:", fa(&pass.get(n!("label")))),
                );
                // L3949
                ins(
                    breakdown,
                    n!("PvpAverageDamage"),
                    lfmt!(
                        "%.1f ^8(average pvp hit)",
                        fa(&output.get(n!("PvpAverageHit")))
                    ),
                );
                // L3950
                ins(
                    breakdown,
                    n!("PvpAverageDamage"),
                    lfmt!(
                        "x %.2f ^8(chance to hit)",
                        output.num(n!("HitChance")) / 100.0
                    ),
                );
                // L3951
                ins(
                    breakdown,
                    n!("PvpAverageDamage"),
                    lfmt!("= %.1f", fa(&output.get(n!("PvpAverageDamage")))),
                );
            }
        }
    }
    // L3955 (end of the pass loop body)
    Flow::Normal
}

/// `v[k]` where `v` may be a breakdown object (`breakdown.MainHand`, `pass.breakdown`).
fn bd_index(v: &Val, k: impl Into<Key>) -> Val {
    if let Some(b) = v.obj::<Breakdown>() {
        return b.t.get(k);
    }
    v.get(k)
}

/// A value used as a table (a plain table or a breakdown object's table).
fn as_table(v: &Val) -> Table {
    match v {
        Val::Table(t) => t.clone(),
        other => match other.obj::<Breakdown>() {
            Some(b) => b.t.clone(),
            None => lua_error!("attempt to index a {} value", other.lua_type()),
        },
    }
}

/// @pob Modules/CalcOffence.lua:3956-4154 h=9757aaae
/// OF-F: CalcOffence.lua 3956-4154 (combine passes).
pub(crate) fn seg_combinePasses(ctx: &mut OffenceCtx) -> Flow {
    let ctx: &OffenceCtx = ctx;
    let env = ctx.env.clone();
    let activeSkill = ctx.activeSkill.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let isAttack = ctx.isAttack;
    let quantityMultiplier = ctx.quantityMultiplier;
    let passList = ctx.passList.clone();
    let data = data();
    let scfg: Option<&ModCfg> = Some(&*skillCfg);
    let AVERAGE = n!("AVERAGE");
    let DPS = n!("DPS");

    // L3957
    if isAttack {
        // Combine crit stats, average damage and DPS
        // L3959
        combineStat(ctx, n!("PreEffectiveCritChance"), AVERAGE, None);
        // L3960
        combineStat(ctx, n!("CritChance"), AVERAGE, None);
        // L3961
        combineStat(ctx, n!("CritMultiplier"), AVERAGE, None);
        // L3962
        combineStat(ctx, n!("AverageDamage"), DPS, None);
        // L3963
        combineStat(ctx, n!("PvpAverageDamage"), DPS, None);
        // L3964
        combineStat(ctx, n!("TotalDPS"), DPS, None);
        // L3965
        combineStat(ctx, n!("PvpTotalDPS"), DPS, None);
        // L3966
        combineStat(ctx, n!("LifeLeechDuration"), DPS, None);
        // L3967
        combineStat(ctx, n!("LifeLeechInstances"), DPS, None);
        // L3968
        combineStat(ctx, n!("LifeLeechInstant"), DPS, None);
        // L3969
        combineStat(ctx, n!("LifeLeechInstantRate"), DPS, None);
        // L3970
        combineStat(ctx, n!("LifeLeechInstantProportion"), DPS, None);
        // L3971
        combineStat(ctx, n!("EnergyShieldLeechDuration"), DPS, None);
        // L3972
        combineStat(ctx, n!("EnergyShieldLeechInstances"), DPS, None);
        // L3973
        combineStat(ctx, n!("EnergyShieldLeechInstant"), DPS, None);
        // L3974
        combineStat(ctx, n!("EnergyShieldLeechInstantRate"), DPS, None);
        // L3975
        combineStat(ctx, n!("EnergyShieldLeechInstantProportion"), DPS, None);
        // L3976
        combineStat(ctx, n!("ManaLeechDuration"), DPS, None);
        // L3977
        combineStat(ctx, n!("ManaLeechInstances"), DPS, None);
        // L3978
        combineStat(ctx, n!("ManaLeechInstant"), DPS, None);
        // L3979
        combineStat(ctx, n!("ManaLeechInstantRate"), DPS, None);
        // L3980
        combineStat(ctx, n!("ManaLeechInstantProportion"), DPS, None);
        // L3981
        combineStat(ctx, n!("LifeOnHit"), DPS, None);
        // L3982
        combineStat(ctx, n!("LifeOnHitRate"), DPS, None);
        // L3983
        combineStat(ctx, n!("LifeOnKill"), DPS, None);
        // L3984
        combineStat(ctx, n!("EnergyShieldOnHit"), DPS, None);
        // L3985
        combineStat(ctx, n!("EnergyShieldOnHitRate"), DPS, None);
        // L3986
        combineStat(ctx, n!("EnergyShieldOnKill"), DPS, None);
        // L3987
        combineStat(ctx, n!("ManaOnHit"), DPS, None);
        // L3988
        combineStat(ctx, n!("ManaOnHitRate"), DPS, None);
        // L3989
        combineStat(ctx, n!("ManaOnKill"), DPS, None);
        // L3990
        combineStat(ctx, n!("impaleStoredHitAvg"), DPS, None);
        // L3991
        if let Some(breakdown) = &breakdown {
            // L3992
            if skillFlags.truthy(n!("bothWeaponAttack")) {
                // L3993
                for critStat in [n!("PreEffectiveCritChance"), n!("CritChance")] {
                    // L3994
                    let combinedBreakdown = tbl!["Both weapons:"];
                    // L3995
                    for_ipairs!((_, pass) in passList => {
                        // L3996
                        let label = pass.get(n!("label"));
                        let line: Val = match &label {
                            Val::Str(s) => Val::from(format!("{s}:")),
                            Val::Num(x) => Val::from(format!("{}:", tostring_num(*x))),
                            other => lua_error!(
                                "attempt to concatenate field 'label' (a {} value)",
                                other.lua_type()
                            ),
                        };
                        combinedBreakdown.push(line);
                        // L3997
                        let passBreakdown = pass.get(n!("breakdown"));
                        if bd_index(&passBreakdown, critStat).truthy() {
                            // L3998
                            let lines = as_table(&bd_index(&passBreakdown, critStat));
                            for_ipairs!((_, line) in lines => {
                                // L3999
                                combinedBreakdown.push(line);
                            });
                        } else {
                            // L4002
                            let v = pass.get(n!("output")).get(critStat);
                            combinedBreakdown.push(lfmt!("%.2f%%", fa(&v)));
                        }
                    });
                    // L4005
                    combinedBreakdown.push(lfmt!(
                        "= %.2f%% ^8(average)",
                        fa(&output.get(critStat))
                    ));
                    // L4006
                    breakdown.set(critStat, combinedBreakdown);
                }
            } else {
                // L4009
                let handBreakdown = lor(
                    land(skillFlags.get(n!("weapon1Attack")), || {
                        breakdown.get(n!("MainHand"))
                    }),
                    || breakdown.get(n!("OffHand")),
                );
                // L4010
                let v = bd_index(&handBreakdown, n!("PreEffectiveCritChance"));
                breakdown.set(n!("PreEffectiveCritChance"), v);
                // L4011
                let v = bd_index(&handBreakdown, n!("CritChance"));
                breakdown.set(n!("CritChance"), v);
            }
            // L4013
            let breakdownSource = if skillFlags.truthy(n!("weapon1Attack")) {
                "MainHand.CritChance"
            } else {
                "OffHand.CritChance"
            };
            // L4014
            if breakdown.truthy(n!("PreEffectiveCritChance")) {
                // L4015
                as_table(&breakdown.get(n!("PreEffectiveCritChance")))
                    .set(n!("breakdownSource"), breakdownSource);
            }
            // L4017
            if breakdown.truthy(n!("CritChance")) {
                // L4018
                as_table(&breakdown.get(n!("CritChance")))
                    .set(n!("breakdownSource"), breakdownSource);
            }
        }
        // L4021
        if skillFlags.truthy(n!("bothWeaponAttack")) {
            // L4022
            if let Some(breakdown) = &breakdown {
                let mainHand = || as_table(&output.get(n!("MainHand")));
                let offHand = || as_table(&output.get(n!("OffHand")));
                // L4023
                breakdown.set(n!("AverageDamage"), Table::new());
                // L4024
                ins(breakdown, n!("AverageDamage"), "Both weapons:");
                // L4025
                if skillData.truthy(n!("doubleHitsWhenDualWielding")) {
                    // L4026
                    ins(
                        breakdown,
                        n!("AverageDamage"),
                        lfmt!(
                            "%.1f + %.1f ^8(skill hits with both weapons at once)",
                            fa(&mainHand().get(n!("AverageDamage"))),
                            fa(&offHand().get(n!("AverageDamage")))
                        ),
                    );
                } else {
                    // L4028
                    ins(
                        breakdown,
                        n!("AverageDamage"),
                        lfmt!(
                            "(%.1f + %.1f) / 2 ^8(skill alternates weapons)",
                            fa(&mainHand().get(n!("AverageDamage"))),
                            fa(&offHand().get(n!("AverageDamage")))
                        ),
                    );
                }
                // L4030
                ins(
                    breakdown,
                    n!("AverageDamage"),
                    lfmt!("= %.1f", fa(&output.get(n!("AverageDamage")))),
                );
                // L4031
                if skillFlags.truthy(n!("isPvP")) {
                    // L4032
                    breakdown.set(n!("PvpAverageDamage"), Table::new());
                    // L4033
                    ins(breakdown, n!("PvpAverageDamage"), "Both weapons:");
                    // L4034
                    if skillData.truthy(n!("doubleHitsWhenDualWielding")) {
                        // L4035
                        ins(
                            breakdown,
                            n!("PvpAverageDamage"),
                            lfmt!(
                                "%.1f + %.1f ^8(skill hits with both weapons at once)",
                                fa(&mainHand().get(n!("PvpAverageDamage"))),
                                fa(&offHand().get(n!("PvpAverageDamage")))
                            ),
                        );
                    } else {
                        // L4037
                        ins(
                            breakdown,
                            n!("PvpAverageDamage"),
                            lfmt!(
                                "(%.1f + %.1f) / 2 ^8(skill alternates weapons)",
                                fa(&mainHand().get(n!("PvpAverageDamage"))),
                                fa(&offHand().get(n!("PvpAverageDamage")))
                            ),
                        );
                    }
                    // L4039
                    ins(
                        breakdown,
                        n!("PvpAverageDamage"),
                        lfmt!("= %.1f", fa(&output.get(n!("PvpAverageDamage")))),
                    );
                }
            }
        }
    }
    // L4044
    if env.mode == Mode::CALCS {
        // L4045
        if skillData.truthy(n!("showAverage")) {
            // L4046
            let s = formatNumSep(&lfmt!("%.1f", fa(&output.get(n!("AverageDamage")))));
            output.set(n!("DisplayDamage"), format!("{s} average damage"));
        } else {
            // L4048
            let s = formatNumSep(&lfmt!("%.1f", fa(&output.get(n!("TotalDPS")))));
            output.set(n!("DisplayDamage"), format!("{s} DPS"));
        }
    }
    // L4051
    if let Some(breakdown) = &breakdown {
        // `output.HitSpeed and s_format("x %.2f ^8(hit rate)", output.HitSpeed) or
        // s_format("x %.2f ^8(<kind> rate)", output.Speed)` (4055, 4060, 4065)
        let rate_line = |fmt_speed: &str| -> String {
            let hs = output.get(n!("HitSpeed"));
            if hs.truthy() {
                lfmt!("x %.2f ^8(hit rate)", fa(&hs))
            } else {
                lfmt!(fmt_speed, fa(&output.get(n!("Speed"))))
            }
        };
        // L4052
        if isAttack {
            // L4053
            let a = lfmt!("%.1f ^8(average damage)", fa(&output.get(n!("AverageDamage"))));
            let b = rate_line("x %.2f ^8(attack rate)");
            breakdown.set(n!("TotalDPS"), tbl![a, b]);
        } else if skillData.truthy(n!("triggered")) {
            // L4058
            let a = lfmt!("%.1f ^8(average damage)", fa(&output.get(n!("AverageDamage"))));
            let b = rate_line("x %.2f ^8(trigger rate)");
            breakdown.set(n!("TotalDPS"), tbl![a, b]);
        } else {
            // L4063
            let a = lfmt!("%.1f ^8(average hit)", fa(&output.get(n!("AverageDamage"))));
            let b = rate_line("x %.2f ^8(cast rate)");
            breakdown.set(n!("TotalDPS"), tbl![a, b]);
        }
        // L4068
        if ne(&skillData.get(n!("dpsMultiplier")), 1.0) {
            // L4069
            ins(
                breakdown,
                n!("TotalDPS"),
                lfmt!(
                    "x %g ^8(DPS multiplier for this skill)",
                    fa(&skillData.get(n!("dpsMultiplier")))
                ),
            );
        }
        // L4071
        if quantityMultiplier > 1.0 {
            // L4072
            ins(
                breakdown,
                n!("TotalDPS"),
                lfmt!(
                    "x %g ^8(quantity multiplier for this skill)",
                    quantityMultiplier
                ),
            );
        }
        // L4074
        ins(
            breakdown,
            n!("TotalDPS"),
            lfmt!("= %.1f", fa(&output.get(n!("TotalDPS")))),
        );
        // L4075
        if skillFlags.truthy(n!("isPvP")) {
            // L4076
            let mut rateType = "cast";
            // L4077
            if isAttack {
                // L4078
                rateType = "attack";
            } else if skillData.truthy(n!("triggered")) {
                // L4080
                rateType = "trigger";
            }
            // L4082
            let a = lfmt!(
                "%.1f ^8(average pvp hit)",
                fa(&output.get(n!("PvpAverageDamage")))
            );
            let hs = output.get(n!("HitSpeed"));
            let b = if hs.truthy() {
                lfmt!("x %.2f ^8(hit rate)", fa(&hs))
            } else {
                lfmt!(
                    "x %.2f ^8(%s rate)",
                    fa(&output.get(n!("Speed"))),
                    rateType
                )
            };
            breakdown.set(n!("PvpTotalDPS"), tbl![a, b]);
            // L4086
            if ne(&skillData.get(n!("dpsMultiplier")), 1.0) {
                // L4087
                ins(
                    breakdown,
                    n!("PvpTotalDPS"),
                    lfmt!(
                        "x %g ^8(DPS multiplier for this skill)",
                        fa(&skillData.get(n!("dpsMultiplier")))
                    ),
                );
            }
            // L4089
            if quantityMultiplier > 1.0 {
                // L4090
                ins(
                    breakdown,
                    n!("PvpTotalDPS"),
                    lfmt!(
                        "x %g ^8(quantity multiplier for this skill)",
                        quantityMultiplier
                    ),
                );
            }
            // L4092
            ins(
                breakdown,
                n!("PvpTotalDPS"),
                lfmt!("= %.1f", fa(&output.get(n!("PvpTotalDPS")))),
            );
        }
    }

    // L4096
    if activeSkill.minion.get().is_some() {
        // L4097
        let v = output.num(n!("SummonedMinionsPerCast"))
            * num_or_else(output.get(n!("HitSpeed")), || output.get(n!("Speed")))
            * skillData.num(n!("dpsMultiplier"));
        skillData.set(n!("summonSpeed"), v);
    }

    // Calculate leech rates
    // L4101
    let v = output.num(n!("Life"))
        * data.misc.LeechRateBase
        * calcLib::r#mod(&skillModList, scfg, &[n!("LifeLeechRate")]);
    output.set(n!("LifeLeechInstanceRate"), v);
    // L4102
    let v = output.num(n!("LifeLeechInstances")) * output.num(n!("LifeLeechInstanceRate"));
    output.set(n!("LifeLeechRate"), v);
    // L4103
    let v = output.get(n!("LifeLeechInstanceRate"));
    output.set(n!("LifeLeechPerHit"), v);
    // L4104
    let v = output.num(n!("EnergyShield"))
        * data.misc.LeechRateBase
        * calcLib::r#mod(&skillModList, scfg, &[n!("EnergyShieldLeechRate")]);
    output.set(n!("EnergyShieldLeechInstanceRate"), v);
    // L4105
    let v = output.num(n!("EnergyShieldLeechInstances"))
        * output.num(n!("EnergyShieldLeechInstanceRate"));
    output.set(n!("EnergyShieldLeechRate"), v);
    // L4106
    let v = output.get(n!("EnergyShieldLeechInstanceRate"));
    output.set(n!("EnergyShieldLeechPerHit"), v);
    // L4107
    let v = output.num(n!("Mana"))
        * data.misc.LeechRateBase
        * calcLib::r#mod(&skillModList, scfg, &[n!("ManaLeechRate")]);
    output.set(n!("ManaLeechInstanceRate"), v);
    // L4108
    let v = output.num(n!("ManaLeechInstances")) * output.num(n!("ManaLeechInstanceRate"));
    output.set(n!("ManaLeechRate"), v);
    // L4109
    let v = output.get(n!("ManaLeechInstanceRate"));
    output.set(n!("ManaLeechPerHit"), v);
    // On full life, Immortal Ambition treats life leech as energy shield leech
    // L4111
    if skillModList.Flag(None, &[n!("ImmortalAmbition")]) {
        // L4112
        let v = output.num(n!("EnergyShieldLeechRate")) + output.num(n!("LifeLeechRate"));
        output.set(n!("EnergyShieldLeechRate"), v);
        // L4113
        let v = output.num(n!("EnergyShieldLeechPerHit")) + output.num(n!("LifeLeechPerHit"));
        output.set(n!("EnergyShieldLeechPerHit"), v);
        // Clears output.LifeLeechRate to disable leechLife flag
        // L4115
        output.set(n!("LifeLeechRate"), 0.0);
        // L4116
        output.set(n!("LifeLeechPerHit"), 0.0);
    }
    // Disable non-instant life leech
    // L4119
    if skillModList.Flag(None, &[n!("UnaffectedByNonInstantLifeLeech")]) {
        // L4120
        output.set(n!("LifeLeechRate"), 0.0);
        // L4121
        output.set(n!("LifeLeechPerHit"), 0.0);
        // L4122
        output.set(n!("LifeLeechInstances"), 0.0);
    }
    // L4124
    let v = output.num(n!("LifeLeechInstantRate"))
        + m_min(output.num(n!("LifeLeechRate")), output.num(n!("MaxLifeLeechRate")))
            * output.num(n!("LifeRecoveryRateMod"));
    output.set(n!("LifeLeechRate"), v);
    // L4125
    let v = output.num(n!("LifeLeechInstant"))
        + m_min(output.num(n!("LifeLeechPerHit")), output.num(n!("MaxLifeLeechRate")))
            * output.num(n!("LifeLeechDuration"))
            * output.num(n!("LifeRecoveryRateMod"));
    output.set(n!("LifeLeechPerHit"), v);
    // L4126
    let v = output.num(n!("EnergyShieldLeechInstantRate"))
        + m_min(
            output.num(n!("EnergyShieldLeechRate")),
            output.num(n!("MaxEnergyShieldLeechRate")),
        ) * output.num(n!("EnergyShieldRecoveryRateMod"));
    output.set(n!("EnergyShieldLeechRate"), v);
    // L4127
    let v = output.num(n!("EnergyShieldLeechInstant"))
        + m_min(
            output.num(n!("EnergyShieldLeechPerHit")),
            output.num(n!("MaxEnergyShieldLeechRate")),
        ) * output.num(n!("EnergyShieldLeechDuration"))
            * output.num(n!("EnergyShieldRecoveryRateMod"));
    output.set(n!("EnergyShieldLeechPerHit"), v);
    // L4128
    let v = output.num(n!("ManaLeechInstantRate"))
        + m_min(output.num(n!("ManaLeechRate")), output.num(n!("MaxManaLeechRate")))
            * output.num(n!("ManaRecoveryRateMod"));
    output.set(n!("ManaLeechRate"), v);
    // L4129
    let v = output.num(n!("ManaLeechInstant"))
        + m_min(output.num(n!("ManaLeechPerHit")), output.num(n!("MaxManaLeechRate")))
            * output.num(n!("ManaLeechDuration"))
            * output.num(n!("ManaRecoveryRateMod"));
    output.set(n!("ManaLeechPerHit"), v);
    // L4130
    let v = output.num(n!("LifeLeechRate")) > 0.0;
    skillFlags.set(n!("leechLife"), v);
    // L4131
    let v = output.num(n!("EnergyShieldLeechRate")) > 0.0;
    skillFlags.set(n!("leechES"), v);
    // L4132
    let v = output.num(n!("ManaLeechRate")) > 0.0;
    skillFlags.set(n!("leechMana"), v);
    // L4133
    if skillData.truthy(n!("showAverage")) {
        // L4134
        let v = output.num(n!("LifeLeechPerHit")) + output.num(n!("LifeOnHit"));
        output.set(n!("LifeLeechGainPerHit"), v);
        // L4135
        let v = output.num(n!("EnergyShieldLeechPerHit")) + output.num(n!("EnergyShieldOnHit"));
        output.set(n!("EnergyShieldLeechGainPerHit"), v);
        // L4136
        let v = output.num(n!("ManaLeechPerHit")) + output.num(n!("ManaOnHit"));
        output.set(n!("ManaLeechGainPerHit"), v);
    } else {
        // L4138
        let v = output.num(n!("LifeLeechRate")) + output.num(n!("LifeOnHitRate"));
        output.set(n!("LifeLeechGainRate"), v);
        // L4139
        let v = output.num(n!("EnergyShieldLeechRate")) + output.num(n!("EnergyShieldOnHitRate"));
        output.set(n!("EnergyShieldLeechGainRate"), v);
        // L4140
        let v = output.num(n!("ManaLeechRate")) + output.num(n!("ManaOnHitRate"));
        output.set(n!("ManaLeechGainRate"), v);
    }
    // L4142
    if let Some(breakdown) = &breakdown {
        let globalOutput = global_output();
        // L4143
        let mut hitRate = output.num(n!("HitChance")) / 100.0
            * num_or_else(globalOutput.get(n!("HitSpeed")), || {
                globalOutput.get(n!("Speed"))
            })
            * skillData.num(n!("dpsMultiplier"));
        // L4144
        if skillFlags.truthy(n!("leechLife")) {
            // L4145
            let t = breakdown.leech(
                output.num(n!("LifeLeechInstant")),
                output.num(n!("LifeLeechInstantRate")),
                output.num(n!("LifeLeechInstances")),
                output.num(n!("Life")),
                n!("LifeLeechRate"),
                output.num(n!("MaxLifeLeechRate")),
                output.num(n!("LifeLeechDuration")),
                output.num(n!("LifeLeechInstantProportion")),
                hitRate,
            );
            breakdown.set(n!("LifeLeech"), t);
        }
        // L4147
        if skillFlags.truthy(n!("leechES")) {
            // L4148
            let t = breakdown.leech(
                output.num(n!("EnergyShieldLeechInstant")),
                output.num(n!("EnergyShieldLeechInstantRate")),
                output.num(n!("EnergyShieldLeechInstances")),
                output.num(n!("EnergyShield")),
                n!("EnergyShieldLeechRate"),
                output.num(n!("MaxEnergyShieldLeechRate")),
                output.num(n!("EnergyShieldLeechDuration")),
                output.num(n!("EnergyShieldLeechInstantProportion")),
                hitRate,
            );
            breakdown.set(n!("EnergyShieldLeech"), t);
        }
        // L4150
        if skillFlags.truthy(n!("leechMana")) {
            // L4151
            let t = breakdown.leech(
                output.num(n!("ManaLeechInstant")),
                output.num(n!("ManaLeechInstantRate")),
                output.num(n!("ManaLeechInstances")),
                output.num(n!("Mana")),
                n!("ManaLeechRate"),
                output.num(n!("MaxManaLeechRate")),
                output.num(n!("ManaLeechDuration")),
                output.num(n!("ManaLeechInstantProportion")),
                hitRate,
            );
            breakdown.set(n!("ManaLeech"), t);
        }
    }
    Flow::Normal
}
