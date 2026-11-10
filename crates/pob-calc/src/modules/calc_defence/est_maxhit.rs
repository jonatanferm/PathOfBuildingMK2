//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! defence segment DF-G (CalcDefence.lua 3165-3873): max hit, DoT EHP,
//! degens / net regen, and the incoming-hit breakdown.
//!
//! Owner: DF-G. The module-level locals this segment reads (`isElemental`,
//! `dmgTypeList`, `allyLifePoolList`, CalcDefence.lua 24-43) are DF-A's
//! (`helpers.rs`, comment marker 7-43).
#![allow(non_snake_case, unused_assignments)] // unused_assignments: Lua dead stores (`local hitTaken = 0`) kept verbatim
// literal Lua shapes: `a = a + b`, `local x` assigned in each branch, the NaN test `x == x`
#![allow(clippy::assign_op_pattern, clippy::needless_late_init, clippy::eq_op)]

use super::helpers::{
    allyLifePoolList, dmgTypeList, incomingDamageBreakdown, isElemental, reducePoolsByDamage,
    takenHitFromDamage,
};
use super::DefEstCtx;
use crate::prelude::*;

// ---- Rust plumbing --------------------------------------------------------------------------

/// `dmgTypeList` (CalcDefence.lua:28) as names, in list order (the `ipairs` loops).
fn dmg_type_names() -> [Name; 5] {
    dmgTypeList.map(Name::from)
}

/// `colorCodes[name]` (a nil code would raise in the `..` that uses it).
fn color_code(name: &str) -> &'static str {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| lua_error!("attempt to concatenate field '{name}' (a nil value)"))
}

/// Lua `v ~= 0` (nil, strings, tables are all `~= 0`).
fn ne0(v: &Val) -> bool {
    !matches!(v, Val::Num(x) if *x == 0.0)
}

/// Lua `tonumber(v)` for one argument (nil and non-numeric values give nil).
fn tonumber_val(v: &Val) -> Option<f64> {
    match v {
        Val::Num(x) => Some(*x),
        Val::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

/// A string key of a `pairs` loop over damage-type tables.
fn key_name(k: Key) -> Name {
    k.name()
        .unwrap_or_else(|| lua_error!("attempt to concatenate a non-string damage type key"))
}

/// `actor.damageShiftTable` / `actor.damageOverTimeShiftTable` indexed by `damageType`.
fn shift_row(t: Option<Table>, damageType: Name) -> Val {
    match t {
        Some(t) => t.get(damageType),
        None => lua_error!("attempt to index a nil value (damage shift table)"),
    }
}

/// `pairs(t)` on a value that must be a table.
fn pairs_of(v: &Val) -> Table {
    v.table().unwrap_or_else(|| {
        lua_error!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            v.lua_type()
        )
    })
}

/// `t.rowList` of a breakdown entry (indexing a non-table raises).
fn row_list(entry: &Val) -> Table {
    entry
        .get(n!("rowList"))
        .table()
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'insert' (table expected)"))
}

/// A breakdown entry that must be a table (`t_insert(breakdown.X, …)`).
fn entry_table(v: Val) -> Table {
    v.table().unwrap_or_else(|| {
        lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            v.lua_type()
        )
    })
}

/// `{ label = …, key = … }` column descriptor.
fn col(label: &'static str, key: &'static str) -> Table {
    tbl! { label = label, key = key }
}

/// The breakdown table literal at 3446-3453 / 3454-3461 / 3462-3469 (and 3687-3710).
fn degen_regen_table(label: &'static str) -> Table {
    tbl! {
        label = label,
        rowList = tbl!(),
        colList = tbl![col("Type", "type"), col("Degen", "degen")],
    }
}

/// `{ type = s_format("%s", damageType), degen = s_format("%.2f", x) }`.
fn degen_row(damageType: Name, degen: f64) -> Table {
    let t = Table::new();
    t.set(n!("type"), lfmt!("%s", damageType));
    t.set(n!("degen"), lfmt!("%.2f", degen));
    t
}

/// The TotalBuildDegen colList (3395-3405).
fn build_degen_total_table() -> Table {
    tbl! {
        rowList = tbl!(),
        colList = tbl![
            col("Base Type", "type"),
            col("Final Type", "type2"),
            col("Base", "base"),
            col("Taken As Percent", "conv"),
            col("Multiplier", "mult"),
            col("Total", "total"),
        ],
    }
}

/// The per-type BuildDegen colList (3414-3423).
fn build_degen_type_table() -> Table {
    tbl! {
        rowList = tbl!(),
        colList = tbl![
            col("Base Type", "type"),
            col("Base", "base"),
            col("Taken As Percent", "conv"),
            col("Multiplier", "mult"),
            col("Total", "total"),
        ],
    }
}

/// The TotalDegen colList (3562-3573 / 3631-3642).
fn enemy_degen_total_table() -> Table {
    tbl! {
        rowList = tbl!(),
        colList = tbl![
            col("Source", "source"),
            col("Base Type", "type"),
            col("Final Type", "type2"),
            col("Base", "base"),
            col("Taken As Percent", "conv"),
            col("Multiplier", "mult"),
            col("Total", "total"),
        ],
    }
}

/// The per-type EnemyDegen colList (3583-3593 / 3652-3662).
fn enemy_degen_type_table() -> Table {
    tbl! {
        rowList = tbl!(),
        colList = tbl![
            col("Source", "source"),
            col("Base Type", "type"),
            col("Base", "base"),
            col("Taken As Percent", "conv"),
            col("Multiplier", "mult"),
            col("Total", "total"),
        ],
    }
}

/// The two enemy-degen breakdown inserts (3561-3601 and 3630-3670 are identical
/// apart from the `type` value).
#[allow(clippy::too_many_arguments)]
fn enemy_degen_breakdown(
    breakdown: &Breakdown,
    output: &Output,
    source: &str,
    baseType: Name,
    damageConvertedType: Name,
    baseVal: f64,
    convertPercent: f64,
    total: f64,
) {
    // L3562
    let cur = breakdown.get(n!("TotalDegen"));
    let v = if cur.truthy() {
        cur
    } else {
        Val::Table(enemy_degen_total_table())
    };
    breakdown.set(n!("TotalDegen"), v);
    // L3574
    let row = Table::new();
    row.set(n!("source"), source);
    row.set(n!("type"), baseType);
    row.set(n!("type2"), damageConvertedType);
    row.set(n!("base"), lfmt!("%.1f", baseVal));
    row.set(n!("conv"), lfmt!("x %.2f%%", convertPercent));
    row.set(
        n!("mult"),
        lfmt!(
            "x %.2f",
            output.num(cat!(damageConvertedType, "TakenDotMult"))
        ),
    );
    row.set(n!("total"), lfmt!("%.1f", total));
    row_list(&breakdown.get(n!("TotalDegen"))).push(row);
    // L3583
    let k = cat!(damageConvertedType, "EnemyDegen");
    let cur = breakdown.get(k);
    let v = if cur.truthy() {
        cur
    } else {
        Val::Table(enemy_degen_type_table())
    };
    breakdown.set(k, v);
    // L3594
    let row = Table::new();
    row.set(n!("source"), source);
    row.set(n!("type"), baseType);
    row.set(n!("base"), lfmt!("%.1f", baseVal));
    row.set(n!("conv"), lfmt!("x %.2f%%", convertPercent));
    row.set(
        n!("mult"),
        lfmt!(
            "x %.2f",
            output.num(cat!(damageConvertedType, "TakenDotMult"))
        ),
    );
    row.set(n!("total"), lfmt!("%.1f", total));
    row_list(&breakdown.get(k)).push(row);
}

/// The life/mana/ES split of one degen (3473-3489 and 3715-3731, same statements).
/// Returns `(energyShieldDegen, lifeDegen, manaDegen)`.
fn split_degen(modDB: &ModDB, output: &Output, damageType: Name, degen: f64) -> (f64, f64, f64) {
    // L3473
    let mut energyShieldDegen = 0.0;
    // L3474
    let mut lifeDegen = 0.0;
    // L3475
    let mut manaDegen = 0.0;
    // L3476
    let takenFromMana =
        output.num(cat!(damageType, "MindOverMatter")) + output.num(n!("sharedMindOverMatter"));
    // L3477
    if output.num(n!("EnergyShieldRegenRecovery")) > 0.0 {
        // L3478
        if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
            // L3479
            lifeDegen = degen * (1.0 - takenFromMana / 100.0);
            // L3480
            energyShieldDegen = degen
                * (1.0 - output.num(cat!(damageType, "EnergyShieldBypass")) / 100.0)
                * (takenFromMana / 100.0);
        } else {
            // L3482
            lifeDegen = degen
                * (output.num(cat!(damageType, "EnergyShieldBypass")) / 100.0)
                * (1.0 - takenFromMana / 100.0);
            // L3483
            energyShieldDegen =
                degen * (1.0 - output.num(cat!(damageType, "EnergyShieldBypass")) / 100.0);
        }
        // L3485
        manaDegen = degen
            * (output.num(cat!(damageType, "EnergyShieldBypass")) / 100.0)
            * (takenFromMana / 100.0);
    } else {
        // L3487
        lifeDegen = degen * (1.0 - takenFromMana / 100.0);
        // L3488
        manaDegen = degen * (takenFromMana / 100.0);
    }
    (energyShieldDegen, lifeDegen, manaDegen)
}

/// DF-G: CalcDefence.lua 3165-3873.
/// @pob Modules/CalcDefence.lua:3165-3873 h=be305576
pub(crate) fn seg(ctx: &mut DefEstCtx) -> Flow {
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let damageCategoryConfig = ctx.damageCategoryConfig;

    // L3165
    {
        // fix total pools, as they aren't used anymore
        // L3167
        for damageType in dmg_type_names() {
            let kPool = cat!(damageType, "TotalHitPool");
            // ward
            // L3169
            let wardBypass = modDB.Sum(ModType::BASE, None, nm!["WardBypass"]);
            // L3170
            if wardBypass > 0.0 {
                // L3171
                let poolProtected =
                    output.num(n!("Ward")) / (1.0 - wardBypass / 100.0) * (wardBypass / 100.0);
                // L3172
                let mut sourcePool = output.num(kPool);
                // L3173
                sourcePool = m_max(sourcePool - poolProtected, 0.0)
                    + m_min(sourcePool, poolProtected) / (wardBypass / 100.0);
                // L3174
                output.set(kPool, sourcePool);
            } else {
                // L3176 (`a + output.Ward or 0` is `(a + output.Ward) or 0`: the sum is never nil) LUA-QUIRK
                let v = output.num(kPool) + output.num(n!("Ward"));
                output.set(kPool, v);
            }
            // aegis
            // L3179
            let elemAegis = if isElemental(damageType.as_str()) {
                output.num0(cat!(damageType, "AegisDisplay"))
            } else {
                0.0
            };
            let v = output.num(kPool)
                + m_max(
                    m_max(
                        output.num(cat!(damageType, "Aegis")),
                        output.num(n!("sharedAegis")),
                    ),
                    elemAegis,
                );
            output.set(kPool, v);
            // guard skill
            // L3181 (`a or 0 + b or 0` is `a or (0 + b) or 0`) LUA-QUIRK
            let GuardAbsorbRate = {
                let a = output.get(n!("sharedGuardAbsorbRate"));
                if a.truthy() {
                    a.num()
                } else {
                    0.0 + output.num(cat!(damageType, "GuardAbsorbRate"))
                }
            };
            // L3182
            if GuardAbsorbRate > 0.0 {
                // L3183 LUA-QUIRK (as 3181)
                let GuardAbsorb = {
                    let a = output.get(n!("sharedGuardAbsorb"));
                    if a.truthy() {
                        a.num()
                    } else {
                        0.0 + output.num(cat!(damageType, "GuardAbsorb"))
                    }
                };
                // L3184
                if GuardAbsorbRate >= 100.0 {
                    // L3185
                    let v = output.num(kPool) + GuardAbsorb;
                    output.set(kPool, v);
                } else {
                    // L3187
                    let poolProtected =
                        GuardAbsorb / (GuardAbsorbRate / 100.0) * (1.0 - GuardAbsorbRate / 100.0);
                    // L3188
                    let v = m_max(output.num(kPool) - poolProtected, 0.0)
                        + m_min(output.num(kPool), poolProtected) / (1.0 - GuardAbsorbRate / 100.0);
                    output.set(kPool, v);
                }
            }
            // Undo the ally pool drains in reverse order to recover the incoming hit.
            // L3192
            for index in (1..=allyLifePoolList.len()).rev() {
                // L3193
                let ally = &allyLifePoolList[index - 1];
                // L3194
                let life = output.get(Name::from(ally.life));
                // L3195
                let mitigation = output.get(Name::from(ally.mitigation));
                // L3196
                if life.truthy()
                    && life.num() > 0.0
                    && mitigation.truthy()
                    && mitigation.num() > 0.0
                {
                    let life = life.num();
                    // L3197
                    let mitigation = m_min(mitigation.num(), 100.0);
                    // L3198
                    if mitigation == 100.0 {
                        // L3199
                        let v = output.num(kPool) + life;
                        output.set(kPool, v);
                    } else {
                        // L3201
                        let poolProtected =
                            life / (mitigation / 100.0) * (1.0 - mitigation / 100.0);
                        // L3202
                        let v = m_max(output.num(kPool) - poolProtected, 0.0)
                            + m_min(output.num(kPool), poolProtected) / (1.0 - mitigation / 100.0);
                        output.set(kPool, v);
                    }
                }
            }
        }

        // L3208
        for damageType in dmg_type_names() {
            // L3209
            let mut partMin = m_huge;
            // L3210
            let mut useConversionSmoothing = false;
            // L3211
            for damageConvertedType in dmg_type_names() {
                // L3212
                let convertPercent = shift_row(actor.damageShiftTable.get(), damageType)
                    .get(damageConvertedType)
                    .num();
                // L3213
                let takenFlat = output.get(cat!(damageConvertedType, "takenFlat"));
                // L3214
                if convertPercent > 0.0 || ne0(&takenFlat) {
                    // L3215
                    let mut hitTaken = 0.0;
                    // L3216
                    let effectiveAppliedArmour =
                        output.get(cat!(damageConvertedType, "EffectiveAppliedArmour"));
                    // L3217
                    let damageConvertedMulti = convertPercent / 100.0;
                    // L3218
                    let totalHitPool = output.num(cat!(damageConvertedType, "TotalHitPool"));
                    // L3219
                    let totalTakenMulti = output
                        .num(cat!(damageConvertedType, "AfterReductionTakenHitMulti"))
                        * (1.0 - output.num(n!("VaalArcticArmourMitigation")));
                    // L3220
                    if damageConvertedMulti <= 0.0 {
                        // L3221
                        let takenWithoutIncoming = m_max(takenFlat.num(), 0.0) * totalTakenMulti;
                        // L3222
                        hitTaken = if takenWithoutIncoming >= totalHitPool {
                            0.0
                        } else {
                            m_huge
                        };
                    } else if matches!(effectiveAppliedArmour, Val::Num(x) if x == 0.0)
                        && convertPercent == 100.0
                    {
                        // use a simpler calculation for no armour DR
                        // L3224
                        let totalResistMult =
                            output.num(cat!(damageConvertedType, "ResistTakenHitMulti"));
                        // L3225
                        let drMulti = totalResistMult
                            * (1.0
                                - output.num(cat!(damageConvertedType, "DamageReduction")) / 100.0);
                        // L3226
                        hitTaken = m_max(
                            totalHitPool / damageConvertedMulti / drMulti - takenFlat.num(),
                            0.0,
                        ) / totalTakenMulti;
                    } else {
                        // get relevant raw reductions and reduction modifiers
                        // L3229
                        let totalResistMult =
                            output.num(cat!(damageConvertedType, "ResistTakenHitMulti"));

                        // L3231
                        let reductionPercent = if modDB.Flag(
                            None,
                            &[cat!(
                                "SelfIgnore",
                                "Base",
                                damageConvertedType,
                                "DamageReduction"
                            )],
                        ) {
                            0.0
                        } else {
                            let w = output.get(cat!(
                                "Base",
                                damageConvertedType,
                                "DamageReductionWhenHit"
                            ));
                            if w.truthy() {
                                w.num()
                            } else {
                                output.num(cat!("Base", damageConvertedType, "DamageReduction"))
                            }
                        };
                        // L3232
                        let flatDR = reductionPercent / 100.0;
                        // L3233
                        let enemyOverwhelmPercent = if modDB.Flag(
                            None,
                            &[cat!("SelfIgnore", damageConvertedType, "DamageReduction")],
                        ) {
                            0.0
                        } else {
                            output.num(cat!(damageConvertedType, "EnemyOverwhelm"))
                        };

                        // We know the damage and armour calculation chain. The important part for max hit calculations is:
                        // 		dmgAfterRes = RAW * DamageConvertedMulti * ResistanceMulti
                        // 		armourDR = AppliedArmour / (AppliedArmour + 5 * dmgAfterRes)
                        // 		totalDR = max(min(armourDR + FlatDR, MaxReduction) - Overwhelm, 0)	-- min and max is complicated to actually math out so skip caps first and tack it on later. Result should be close enough
                        // 		dmgReceived = dmgAfterRes * (1 - totalDR)
                        // 		damageTaken = (dmgReceived + takenFlat) * TakenMulti
                        // If we consider damageTaken to be the total hit pool of the actor, we can go backwards in the chain until we find the max hit - the RAW damage.
                        // Unfortunately the above is slightly simplified and is missing a line that *really* complicates stuff for exact calculations:
                        //		damageTaken = damageTakenAsPhys + damageTakenAsFire + damageTakenAsCold + damageTakenAsLightning + damageTakenAsChaos
                        // Trying to solve that for RAW might require solving a polynomial equation of 6th degree, so this solution settles for solving the parts independently and then approximating the final result
                        //
                        // To solve only one part the above can be expressed as this:
                        //		5 * (1 - FlatDR + Overwhelm) * TakenMulti * ResistanceMulti * ResistanceMulti * DamageConvertedMulti * DamageConvertedMulti * RAW * RAW + ((Overwhelm - FlatDR) * AppliedArmour * TakenMulti - 5 * (damageTaken - takenFlat * TakenMulti)) * ResistanceMulti * DamageConvertedMulti * RAW - (damageTaken - takenFlat * TakenMulti) * AppliedArmour = 0
                        // Which means that
                        // 		RAW = [quadratic]

                        let effectiveAppliedArmour = effectiveAppliedArmour.num();
                        let takenFlat = takenFlat.num();
                        // L3251
                        let resistXConvert = totalResistMult * damageConvertedMulti;
                        // L3252
                        let a = 5.0
                            * (1.0 - flatDR + enemyOverwhelmPercent / 100.0)
                            * totalTakenMulti
                            * resistXConvert
                            * resistXConvert;
                        // L3253
                        let b = ((enemyOverwhelmPercent / 100.0 - flatDR)
                            * effectiveAppliedArmour
                            * totalTakenMulti
                            - 5.0 * (totalHitPool - takenFlat * totalTakenMulti))
                            * resistXConvert;
                        // L3254
                        let c =
                            -effectiveAppliedArmour * (totalHitPool - takenFlat * totalTakenMulti);

                        // L3256 (the quotient is a number, so never falsy)
                        let RAW = if a != 0.0 {
                            (m_sqrt(m_max(b * b - 4.0 * a * c, 0.0)) - b) / (2.0 * a)
                        } else {
                            m_huge
                        };

                        // tack on some caps
                        // L3259
                        let noDRMaxHit =
                            totalHitPool / damageConvertedMulti / totalResistMult / totalTakenMulti
                                * (1.0 - takenFlat * totalTakenMulti / totalHitPool);
                        // L3260
                        let maxDRMaxHit = noDRMaxHit
                            / (1.0
                                - (output.num(n!("DamageReductionMax")) - enemyOverwhelmPercent)
                                    / 100.0);
                        // L3261
                        hitTaken = m_floor(m_max(m_min(RAW, maxDRMaxHit), noDRMaxHit));
                        // L3262
                        useConversionSmoothing = useConversionSmoothing || convertPercent != 100.0;
                    }
                    // L3264
                    partMin = m_min(partMin, hitTaken);
                }
            }

            // L3268
            let enemyDamageMult = output.num(cat!(damageType, "EnemyDamageMult"));

            // L3270
            let finalMaxHit;
            // L3271
            if partMin == m_huge {
                // L3272
                finalMaxHit = m_huge;
            } else if useConversionSmoothing {
                // L3274
                let mut passIncomingDamage = partMin;
                // L3275
                let mut previousOverkill: Option<f64> = None;
                // L3276
                let limit = data().misc.maxHitSmoothingPasses;
                let mut n = 1.0;
                while n <= limit {
                    // L3277
                    let (_, passDamages) =
                        takenHitFromDamage(passIncomingDamage, damageType, &actor);
                    // L3278
                    let passPools = reducePoolsByDamage(None, &passDamages, &actor);
                    // L3279
                    let passOverkill =
                        passPools.num(n!("OverkillDamage")) - passPools.num(n!("hitPoolRemaining"));
                    // L3280
                    let mut passRatio = 0.0;
                    // L3281 LUA-ORDER (string keys; only NaN handling of m_max can see the order)
                    for (partType, _) in passDamages.pairs() {
                        // L3282
                        let partPool = output.num0(cat!(key_name(partType), "TotalHitPool"));
                        // L3283
                        if partPool > 0.0 {
                            // L3284
                            passRatio = m_max(passRatio, (passOverkill + partPool) / partPool);
                        }
                    }
                    // L3287
                    if passRatio <= 0.0 {
                        // L3288
                        passRatio = 1.0;
                    }
                    // L3290
                    let mut stepSize = 1.0;
                    // L3291
                    if let Some(previousOverkill) =
                        previousOverkill.filter(|p| n > 1.0 && *p != 0.0 && *p == *p)
                    {
                        // L3292
                        stepSize = m_min(
                            m_abs((passOverkill - previousOverkill) / previousOverkill),
                            2.0,
                        );
                    }
                    // L3294 (`a and x or b and y or 0`; x and y are numbers, never falsy)
                    let stepAdjust = if stepSize > 1.0 {
                        -passOverkill / stepSize
                    } else if n > 1.0 {
                        -passOverkill * stepSize
                    } else {
                        0.0
                    };
                    // L3295
                    previousOverkill = Some(passOverkill);
                    // L3296
                    passIncomingDamage = (passIncomingDamage + stepAdjust) / m_sqrt(passRatio);
                    // L3297
                    if passOverkill < 1.0 && passOverkill > -1.0 {
                        // L3298
                        break;
                    }
                    n += 1.0;
                }

                // L3302
                finalMaxHit = round(passIncomingDamage / enemyDamageMult, None);
            } else {
                // L3304
                finalMaxHit = round(partMin / enemyDamageMult, None);
            }

            // L3307
            let maxHitCurType = cat!(damageType, "MaximumHitTaken");
            // L3308
            output.set(maxHitCurType, finalMaxHit);

            // L3310
            if let Some(breakdown) = &breakdown {
                // L3311
                breakdown.set(
                    maxHitCurType,
                    tbl! {
                        label = "Maximum hit damage breakdown",
                        rowList = tbl!(),
                        colList = tbl![
                            col("Type", "type"),
                            col("Pool", "pool"),
                            col("Incoming", "incoming"),
                            col("Multi", "multi"),
                            col("Taken", "taken"),
                        ],
                    },
                );

                // L3323
                let (mut fullTaken, takenDamages) =
                    takenHitFromDamage(finalMaxHit * enemyDamageMult, damageType, &actor);
                // L3324
                fullTaken = if fullTaken == fullTaken {
                    fullTaken
                } else {
                    0.0
                };

                // L3326 LUA-ORDER (string keys: rowList order follows pairs order)
                for (takenType, takenAmt) in takenDamages.pairs() {
                    let takenTypeName = key_name(takenType);
                    // L3327
                    let conversion = shift_row(actor.damageShiftTable.get(), damageType)
                        .get(takenTypeName)
                        .num();
                    // L3328
                    let incoming = finalMaxHit * enemyDamageMult * conversion / 100.0;
                    // L3329
                    let takenAmt = takenAmt.num();
                    let nanToZero = if takenAmt == takenAmt { takenAmt } else { 0.0 };
                    // L3330
                    takenDamages.set(takenType, nanToZero);
                    // L3331
                    let row = Table::new();
                    row.set(n!("type"), lfmt!("%d%% as %s", conversion, takenTypeName));
                    row.set(
                        n!("pool"),
                        lfmt!("%d", output.num(cat!(takenTypeName, "TotalHitPool"))),
                    );
                    row.set(n!("incoming"), lfmt!("%.0f", incoming));
                    row.set(n!("multi"), lfmt!("x%.3f", nanToZero / incoming));
                    row.set(n!("taken"), lfmt!("%.0f", nanToZero));
                    row_list(&breakdown.get(maxHitCurType)).push(row);
                }

                // L3340
                let fullMulti = fullTaken / finalMaxHit / enemyDamageMult;
                // L3341
                entry_table(breakdown.get(maxHitCurType))
                    .push("^8Maximum hit is calculated in reverse -");
                // L3342
                entry_table(breakdown.get(maxHitCurType))
                    .push("^8from health pools, via damage reductions, to the max hit:");
                // L3343
                entry_table(breakdown.get(maxHitCurType))
                    .push(lfmt!("%d ^8(used pool)", fullTaken));
                // L3344
                if round(fullMulti, Some(2)) != 1.0 {
                    // L3345
                    entry_table(breakdown.get(maxHitCurType))
                        .push(lfmt!("/ %.2f ^8(modifiers to damage taken)", fullMulti));
                }
                // L3347
                if enemyDamageMult != 1.0 {
                    // L3348
                    entry_table(breakdown.get(maxHitCurType)).push(lfmt!(
                        "/ %.2f ^8(modifiers to enemy damage)",
                        enemyDamageMult
                    ));
                }
                // L3350
                entry_table(breakdown.get(maxHitCurType)).push(lfmt!(
                    "= %.0f ^8maximum survivable enemy damage%s",
                    finalMaxHit,
                    if useConversionSmoothing {
                        " (approximate)"
                    } else {
                        ""
                    }
                ));

                // L3352
                entry_table(breakdown.get(maxHitCurType))
                    .push("^8Such a hit would drain the following resources:");
                // L3353
                let cur = entry_table(breakdown.get(maxHitCurType));
                let pools = reducePoolsByDamage(None, &takenDamages, &actor);
                let v = incomingDamageBreakdown(&cur, &pools, &output);
                breakdown.set(maxHitCurType, v);
            }
        }

        // second minimum used for power calcs, as there are issues using average or minimum
        // L3358
        let mut minimum = m_huge;
        // L3359
        let mut SecondMinimum = m_huge;
        // L3360
        for damageType in dmg_type_names() {
            let k = cat!(damageType, "MaximumHitTaken");
            // L3361
            if output.num(k) < minimum {
                // L3362
                SecondMinimum = minimum;
                // L3363
                minimum = output.num(k);
            } else if output.num(k) < SecondMinimum {
                // L3365
                SecondMinimum = output.num(k);
            }
        }
        // L3368
        output.set(n!("SecondMinimalMaximumHitTaken"), SecondMinimum);
    }

    // effective health pool vs dots
    // L3372
    for damageType in dmg_type_names() {
        // L3373
        let v = output.num(cat!(damageType, "TotalPool"))
            / output.num(cat!(damageType, "TakenDotMult"));
        output.set(cat!(damageType, "DotEHP"), v);
        // L3374
        if let Some(breakdown) = &breakdown {
            // L3375
            breakdown.set(
                cat!(damageType, "DotEHP"),
                tbl![
                    lfmt!("Total Pool: %d", output.num(cat!(damageType, "TotalPool"))),
                    lfmt!(
                        "Dot Damage Taken modifier: %.2f",
                        output.num(cat!(damageType, "TakenDotMult"))
                    ),
                    lfmt!(
                        "Total Effective Dot Pool: %d",
                        output.num(cat!(damageType, "DotEHP"))
                    ),
                ],
            );
        }
    }

    // degens
    // L3384
    {
        // L3385
        output.set(n!("TotalBuildDegen"), 0.0);
        // L3386
        for damageType in dmg_type_names() {
            // L3387
            let baseVal = modDB.Sum(ModType::BASE, None, &[cat!(damageType, "Degen")]);
            // L3388
            if baseVal > 0.0 {
                // L3389 LUA-ORDER (string keys: float sum order and breakdown row order)
                let shift = pairs_of(&shift_row(actor.damageOverTimeShiftTable.get(), damageType));
                for (damageConvertedType, convertPercent) in shift.pairs() {
                    let damageConvertedType = key_name(damageConvertedType);
                    let convertPercent = convertPercent.num();
                    // L3390
                    if convertPercent > 0.0 {
                        // L3391
                        let total = baseVal
                            * (convertPercent / 100.0)
                            * output.num(cat!(damageConvertedType, "TakenDotMult"));
                        // L3392
                        let k = cat!(damageConvertedType, "BuildDegen");
                        let v = output.num0(k) + total;
                        output.set(k, v);
                        // L3393
                        let v = output.num(n!("TotalBuildDegen")) + total;
                        output.set(n!("TotalBuildDegen"), v);
                        // L3394
                        if let Some(breakdown) = &breakdown {
                            // L3395
                            let cur = breakdown.get(n!("TotalBuildDegen"));
                            let v = if cur.truthy() {
                                cur
                            } else {
                                Val::Table(build_degen_total_table())
                            };
                            breakdown.set(n!("TotalBuildDegen"), v);
                            // L3406
                            let row = Table::new();
                            row.set(n!("type"), damageType);
                            row.set(n!("type2"), damageConvertedType);
                            row.set(n!("base"), lfmt!("%.1f", baseVal));
                            row.set(n!("conv"), lfmt!("x %.2f%%", convertPercent));
                            row.set(
                                n!("mult"),
                                lfmt!(
                                    "x %.2f",
                                    output.num(cat!(damageConvertedType, "TakenDotMult"))
                                ),
                            );
                            row.set(n!("total"), lfmt!("%.1f", total));
                            row_list(&breakdown.get(n!("TotalBuildDegen"))).push(row);
                            // L3414
                            let cur = breakdown.get(k);
                            let v = if cur.truthy() {
                                cur
                            } else {
                                Val::Table(build_degen_type_table())
                            };
                            breakdown.set(k, v);
                            // L3424
                            let row = Table::new();
                            row.set(n!("type"), damageType);
                            row.set(n!("base"), lfmt!("%.1f", baseVal));
                            row.set(n!("conv"), lfmt!("x %.2f%%", convertPercent));
                            row.set(
                                n!("mult"),
                                lfmt!(
                                    "x %.2f",
                                    output.num(cat!(damageConvertedType, "TakenDotMult"))
                                ),
                            );
                            row.set(n!("total"), lfmt!("%.1f", total));
                            row_list(&breakdown.get(k)).push(row);
                        }
                    }
                }
            }
        }
        // L3436
        if matches!(output.get(n!("TotalBuildDegen")), Val::Num(x) if x == 0.0) {
            // L3437
            output.set(n!("TotalBuildDegen"), Val::Nil);
        } else {
            // L3439
            output.set(n!("NetLifeRegen"), output.get(n!("LifeRegenRecovery")));
            // L3440
            output.set(n!("NetManaRegen"), output.get(n!("ManaRegenRecovery")));
            // L3441
            output.set(
                n!("NetEnergyShieldRegen"),
                output.get(n!("EnergyShieldRegenRecovery")),
            );
            // L3442
            let mut totalLifeDegen = 0.0;
            // L3443
            let mut totalManaDegen = 0.0;
            // L3444
            let mut totalEnergyShieldDegen = 0.0;
            // L3445
            if let Some(breakdown) = &breakdown {
                // L3446
                breakdown.set(n!("NetLifeRegen"), degen_regen_table("Total Life Degen"));
                // L3454
                breakdown.set(n!("NetManaRegen"), degen_regen_table("Total Mana Degen"));
                // L3462
                breakdown.set(
                    n!("NetEnergyShieldRegen"),
                    degen_regen_table("Total Energy Shield Degen"),
                );
            }
            // L3471
            for damageType in dmg_type_names() {
                // L3472
                if output.truthy(cat!(damageType, "BuildDegen")) {
                    // L3473-3489
                    let degen = output.num(cat!(damageType, "BuildDegen"));
                    let (energyShieldDegen, lifeDegen, manaDegen) =
                        split_degen(&modDB, &output, damageType, degen);
                    // L3490
                    totalLifeDegen = totalLifeDegen + lifeDegen;
                    // L3491
                    totalManaDegen = totalManaDegen + manaDegen;
                    // L3492
                    totalEnergyShieldDegen = totalEnergyShieldDegen + energyShieldDegen;
                    // L3493
                    if let Some(breakdown) = &breakdown {
                        // L3494
                        row_list(&breakdown.get(n!("NetLifeRegen")))
                            .push(degen_row(damageType, lifeDegen));
                        // L3498
                        row_list(&breakdown.get(n!("NetManaRegen")))
                            .push(degen_row(damageType, manaDegen));
                        // L3502
                        row_list(&breakdown.get(n!("NetEnergyShieldRegen")))
                            .push(degen_row(damageType, energyShieldDegen));
                    }
                }
            }
            // L3509
            let v = output.num(n!("NetLifeRegen")) - totalLifeDegen;
            output.set(n!("NetLifeRegen"), v);
            // L3510
            let v = output.num(n!("NetManaRegen")) - totalManaDegen;
            output.set(n!("NetManaRegen"), v);
            // L3511
            let v = output.num(n!("NetEnergyShieldRegen")) - totalEnergyShieldDegen;
            output.set(n!("NetEnergyShieldRegen"), v);
            // L3512
            let v = output.num(n!("NetLifeRegen"))
                + output.num(n!("NetManaRegen"))
                + output.num(n!("NetEnergyShieldRegen"));
            output.set(n!("TotalNetRegen"), v);
            // L3513
            if let Some(breakdown) = &breakdown {
                let life = entry_table(breakdown.get(n!("NetLifeRegen")));
                // L3514
                life.push(lfmt!(
                    "%.1f ^8(total life regen)",
                    output.num(n!("LifeRegenRecovery"))
                ));
                // L3515
                entry_table(breakdown.get(n!("NetLifeRegen")))
                    .push(lfmt!("- %.1f ^8(total life degen)", totalLifeDegen));
                // L3516
                entry_table(breakdown.get(n!("NetLifeRegen")))
                    .push(lfmt!("= %.1f", output.num(n!("NetLifeRegen"))));
                // L3517
                entry_table(breakdown.get(n!("NetManaRegen"))).push(lfmt!(
                    "%.1f ^8(total mana regen)",
                    output.num(n!("ManaRegenRecovery"))
                ));
                // L3518
                entry_table(breakdown.get(n!("NetManaRegen")))
                    .push(lfmt!("- %.1f ^8(total mana degen)", totalManaDegen));
                // L3519
                entry_table(breakdown.get(n!("NetManaRegen")))
                    .push(lfmt!("= %.1f", output.num(n!("NetManaRegen"))));
                // L3520
                entry_table(breakdown.get(n!("NetEnergyShieldRegen"))).push(lfmt!(
                    "%.1f ^8(total energy shield regen)",
                    output.num(n!("EnergyShieldRegenRecovery"))
                ));
                // L3521
                entry_table(breakdown.get(n!("NetEnergyShieldRegen"))).push(lfmt!(
                    "- %.1f ^8(total energy shield degen)",
                    totalEnergyShieldDegen
                ));
                // L3522
                entry_table(breakdown.get(n!("NetEnergyShieldRegen")))
                    .push(lfmt!("= %.1f", output.num(n!("NetEnergyShieldRegen"))));
                // L3523
                breakdown.set(
                    n!("TotalNetRegen"),
                    tbl![
                        lfmt!("Net Life Regen: %.1f", output.num(n!("NetLifeRegen"))),
                        lfmt!("+ Net Mana Regen: %.1f", output.num(n!("NetManaRegen"))),
                        lfmt!(
                            "+ Net Energy Shield Regen: %.1f",
                            output.num(n!("NetEnergyShieldRegen"))
                        ),
                        lfmt!("= Total Net Regen: %.1f", output.num(n!("TotalNetRegen"))),
                    ],
                );
            }
        }
        // L3531
        let enemyCritAilmentEffect = 1.0
            + output.num0(n!("EnemyCritChance")) / 100.0
                * 0.5
                * (1.0 - output.num(n!("CritExtraDamageReduction")) / 100.0);
        // this is just used so that ailments don't always showup if the enemy has no other way of applying the ailment and they have a low crit chance
        // L3533
        let enemyCritThreshold = 10.1;
        // L3534
        let enemyBleedChance = 0.0;
        // L3535
        let mut enemyIgniteChance = 0.0;
        // L3536
        if ne0(&output.get(n!("SelfIgniteEffect")))
            && output.num(n!("IgniteAvoidChance")) < 100.0
            && ne0(&output.get(n!("SelfIgniteDuration")))
            && damageCategoryConfig != n!("DamageOverTime")
        {
            // L3537
            enemyIgniteChance = enemyDB.Sum(
                ModType::BASE,
                None,
                nm!["IgniteChance", "ElementalAilmentChance"],
            );
            // L3538 (`(cond and EnemyCritChance or 0)`: a number, never falsy)
            let enemyCritAilmentChance = if !modDB
                .Flag(None, nm!["CritsOnYouDontAlwaysApplyElementalAilments"])
            {
                if output.num(n!("EnemyCritChance")) > enemyCritThreshold || enemyIgniteChance > 0.0
                {
                    output.num(n!("EnemyCritChance"))
                } else {
                    0.0
                }
            } else {
                0.0
            };
            // L3539
            enemyIgniteChance = (enemyCritAilmentChance
                + (1.0 - enemyCritAilmentChance / 100.0) * enemyIgniteChance)
                * (1.0 - output.num(n!("IgniteAvoidChance")) / 100.0);
        }
        // L3541
        let mut enemyPoisonChance = 0.0;
        // L3542
        if ne0(&output.get(n!("SelfPoisonEffect")))
            && output.num(n!("PoisonAvoidChance")) < 100.0
            && ne0(&output.get(n!("SelfPoisonDuration")))
            && damageCategoryConfig != n!("DamageOverTime")
        {
            // L3543
            enemyPoisonChance = enemyDB.Sum(ModType::BASE, None, nm!["PoisonChance"])
                * (1.0 - output.num(n!("PoisonAvoidChance")) / 100.0);
        }
        // L3545
        if damageCategoryConfig == n!("DamageOverTime")
            || (enemyIgniteChance + enemyPoisonChance + enemyBleedChance) > 0.0
        {
            // L3546
            let v = output.num0(n!("TotalBuildDegen"));
            output.set(n!("TotalDegen"), v);
            // L3547
            if damageCategoryConfig == n!("DamageOverTime") {
                // L3548
                for damageType in dmg_type_names() {
                    // L3549
                    let mut source = "Config";
                    // L3550
                    let mut baseVal =
                        tonumber_val(&env.configInput.get(cat!("enemy", damageType, "Damage")));
                    // L3551
                    if baseVal.is_none() {
                        // L3552
                        source = "Default";
                        // L3553
                        baseVal = Some(
                            tonumber_val(
                                &env.configPlaceholder
                                    .get(cat!("enemy", damageType, "Damage")),
                            )
                            .unwrap_or(0.0),
                        );
                    }
                    let baseVal = baseVal.unwrap_or_default();
                    // L3555
                    if baseVal > 0.0 {
                        // L3556 LUA-ORDER (string keys: float sum order and breakdown row order)
                        let shift =
                            pairs_of(&shift_row(actor.damageOverTimeShiftTable.get(), damageType));
                        for (damageConvertedType, convertPercent) in shift.pairs() {
                            let damageConvertedType = key_name(damageConvertedType);
                            let convertPercent = convertPercent.num();
                            // L3557
                            if convertPercent > 0.0 {
                                // L3558
                                let total = baseVal
                                    * (convertPercent / 100.0)
                                    * output.num(cat!(damageConvertedType, "TakenDotMult"));
                                // L3559
                                let k = cat!(damageConvertedType, "EnemyDegen");
                                let v = output.num0(k) + total;
                                output.set(k, v);
                                // L3560
                                let v = output.num(n!("TotalDegen")) + total;
                                output.set(n!("TotalDegen"), v);
                                // L3561
                                if let Some(breakdown) = &breakdown {
                                    // L3562-3601
                                    enemy_degen_breakdown(
                                        breakdown,
                                        &output,
                                        source,
                                        damageType,
                                        damageConvertedType,
                                        baseVal,
                                        convertPercent,
                                        total,
                                    );
                                }
                            }
                        }
                    }
                }
            } else if (enemyIgniteChance + enemyPoisonChance + enemyBleedChance) > 0.0 {
                // L3608 (string-keyed `ailmentList`: insertion order) LUA-ORDER
                let mut ailmentList: Vec<(&'static str, Name, Vec<Name>)> = Vec::new();
                // L3609
                if enemyBleedChance > 0.0 {
                    // L3610
                    ailmentList.push(("Bleed", n!("Physical"), vec![n!("Physical")]));
                }
                // L3612
                if enemyIgniteChance > 0.0 {
                    // L3613
                    let sourceTypes = if enemyDB.Flag(None, nm!["AllDamageIgnites"]) {
                        dmg_type_names().to_vec()
                    } else {
                        vec![n!("Fire")]
                    };
                    ailmentList.push(("Ignite", n!("Fire"), sourceTypes));
                }
                // L3615
                if enemyPoisonChance > 0.0 {
                    // L3616
                    ailmentList.push(("Poison", n!("Chaos"), vec![n!("Physical"), n!("Chaos")]));
                }
                // L3618 LUA-ORDER (string keys: TotalDegen sum order and breakdown row order)
                for (source, ailmentDamageType, sourceTypes) in &ailmentList {
                    let source = *source;
                    let ailmentDamageType = *ailmentDamageType;
                    // L3619
                    let mut baseVal = 0.0;
                    // L3620
                    for damageType in sourceTypes {
                        // L3621
                        baseVal = baseVal + output.num(cat!(*damageType, "TakenDamage"));
                    }
                    // L3623
                    let percentBase = data()
                        .misc
                        .get(cat!(source, "PercentBase").as_str())
                        .unwrap_or_else(|| {
                            lua_error!("attempt to perform arithmetic on a nil value")
                        });
                    baseVal = baseVal
                        * percentBase
                        * (enemyCritAilmentEffect / output.num(n!("EnemyCritEffect")))
                        * output.num(cat!("Self", source, "Effect"))
                        / 100.0;
                    // L3624
                    if baseVal > 0.0 {
                        // L3625 LUA-ORDER (string keys: float sum order and breakdown row order)
                        let shift = pairs_of(&shift_row(
                            actor.damageOverTimeShiftTable.get(),
                            ailmentDamageType,
                        ));
                        for (damageConvertedType, convertPercent) in shift.pairs() {
                            let damageConvertedType = key_name(damageConvertedType);
                            let convertPercent = convertPercent.num();
                            // L3626
                            if convertPercent > 0.0 {
                                // L3627
                                let total = baseVal
                                    * (convertPercent / 100.0)
                                    * output.num(cat!(damageConvertedType, "TakenDotMult"));
                                // L3628
                                let k = cat!(damageConvertedType, "EnemyDegen");
                                let v = output.num0(k) + total;
                                output.set(k, v);
                                // L3629
                                let v = output.num(n!("TotalDegen")) + total;
                                output.set(n!("TotalDegen"), v);
                                // L3630
                                if let Some(breakdown) = &breakdown {
                                    // L3631-3670
                                    enemy_degen_breakdown(
                                        breakdown,
                                        &output,
                                        source,
                                        ailmentDamageType,
                                        damageConvertedType,
                                        baseVal,
                                        convertPercent,
                                        total,
                                    );
                                }
                            }
                        }
                    }
                }
            }
            // L3677
            if output.num(n!("TotalDegen")) == output.num0(n!("TotalBuildDegen")) {
                // L3678
                output.set(n!("TotalDegen"), Val::Nil);
            } else {
                // L3680
                output.set(
                    n!("ComprehensiveNetLifeRegen"),
                    output.get(n!("LifeRegenRecovery")),
                );
                // L3681
                output.set(
                    n!("ComprehensiveNetManaRegen"),
                    output.get(n!("ManaRegenRecovery")),
                );
                // L3682
                output.set(
                    n!("ComprehensiveNetEnergyShieldRegen"),
                    output.get(n!("EnergyShieldRegenRecovery")),
                );
                // L3683
                let mut totalLifeDegen = 0.0;
                // L3684
                let mut totalManaDegen = 0.0;
                // L3685
                let mut totalEnergyShieldDegen = 0.0;
                // L3686
                if let Some(breakdown) = &breakdown {
                    // L3687
                    breakdown.set(
                        n!("ComprehensiveNetLifeRegen"),
                        degen_regen_table("Total Life Degen"),
                    );
                    // L3695
                    breakdown.set(
                        n!("ComprehensiveNetManaRegen"),
                        degen_regen_table("Total Mana Degen"),
                    );
                    // L3703
                    breakdown.set(
                        n!("ComprehensiveNetEnergyShieldRegen"),
                        degen_regen_table("Total Energy Shield Degen"),
                    );
                }
                // L3712
                for damageType in dmg_type_names() {
                    // L3713
                    let typeDegen = output.num0(cat!(damageType, "BuildDegen"))
                        + output.num0(cat!(damageType, "EnemyDegen"));
                    // L3714
                    if typeDegen != 0.0 {
                        // L3715-3731
                        let (energyShieldDegen, lifeDegen, manaDegen) =
                            split_degen(&modDB, &output, damageType, typeDegen);
                        // L3732
                        totalLifeDegen = totalLifeDegen + lifeDegen;
                        // L3733
                        totalManaDegen = totalManaDegen + manaDegen;
                        // L3734
                        totalEnergyShieldDegen = totalEnergyShieldDegen + energyShieldDegen;
                        // L3735
                        if let Some(breakdown) = &breakdown {
                            // L3736
                            row_list(&breakdown.get(n!("ComprehensiveNetLifeRegen")))
                                .push(degen_row(damageType, lifeDegen));
                            // L3740
                            row_list(&breakdown.get(n!("ComprehensiveNetManaRegen")))
                                .push(degen_row(damageType, manaDegen));
                            // L3744
                            row_list(&breakdown.get(n!("ComprehensiveNetEnergyShieldRegen")))
                                .push(degen_row(damageType, energyShieldDegen));
                        }
                    }
                }
                // L3751
                let v = output.num(n!("ComprehensiveNetLifeRegen"))
                    + output.num0(n!("LifeRecoupRecoveryAvg"))
                    - totalLifeDegen
                    - output.num0(n!("LifeLossLostAvg"));
                output.set(n!("ComprehensiveNetLifeRegen"), v);
                // L3752
                let v = output.num(n!("ComprehensiveNetManaRegen"))
                    + output.num0(n!("ManaRecoupRecoveryAvg"))
                    - totalManaDegen;
                output.set(n!("ComprehensiveNetManaRegen"), v);
                // L3753
                let v = output.num(n!("ComprehensiveNetEnergyShieldRegen"))
                    + output.num0(n!("EnergyShieldRecoupRecoveryAvg"))
                    - totalEnergyShieldDegen;
                output.set(n!("ComprehensiveNetEnergyShieldRegen"), v);
                // L3754
                let v = output.num(n!("ComprehensiveNetLifeRegen"))
                    + output.num(n!("ComprehensiveNetManaRegen"))
                    + output.num(n!("ComprehensiveNetEnergyShieldRegen"));
                output.set(n!("ComprehensiveTotalNetRegen"), v);
                // L3755
                if let Some(breakdown) = &breakdown {
                    let life = || entry_table(breakdown.get(n!("ComprehensiveNetLifeRegen")));
                    let mana = || entry_table(breakdown.get(n!("ComprehensiveNetManaRegen")));
                    let es = || entry_table(breakdown.get(n!("ComprehensiveNetEnergyShieldRegen")));
                    // L3756
                    life().push(lfmt!(
                        "%.1f ^8(total life regen)",
                        output.num(n!("LifeRegenRecovery"))
                    ));
                    // L3757
                    if output.num0(n!("LifeRecoupRecoveryAvg")) != 0.0 {
                        // L3758
                        life().push(lfmt!(
                            "+ %.1f ^8(average life recoup)",
                            output.num0(n!("LifeRecoupRecoveryAvg"))
                        ));
                    }
                    // L3760
                    life().push(lfmt!("- %.1f ^8(total life degen)", totalLifeDegen));
                    // L3761
                    if output.num0(n!("LifeLossLostAvg")) != 0.0 {
                        // L3762
                        life().push(lfmt!(
                            "- %.1f ^8(average life lost over time)",
                            output.num0(n!("LifeLossLostAvg"))
                        ));
                    }
                    // L3764
                    life().push(lfmt!("= %.1f", output.num(n!("ComprehensiveNetLifeRegen"))));
                    // L3765
                    mana().push(lfmt!(
                        "%.1f ^8(total mana regen)",
                        output.num(n!("ManaRegenRecovery"))
                    ));
                    // L3766
                    if output.num0(n!("ManaRecoupRecoveryAvg")) != 0.0 {
                        // L3767
                        mana().push(lfmt!(
                            "+ %.1f ^8(average mana recoup)",
                            output.num0(n!("ManaRecoupRecoveryAvg"))
                        ));
                    }
                    // L3769
                    mana().push(lfmt!("- %.1f ^8(total mana degen)", totalManaDegen));
                    // L3770
                    mana().push(lfmt!("= %.1f", output.num(n!("ComprehensiveNetManaRegen"))));
                    // L3771
                    es().push(lfmt!(
                        "%.1f ^8(total energy shield regen)",
                        output.num(n!("EnergyShieldRegenRecovery"))
                    ));
                    // L3772
                    if output.num0(n!("EnergyShieldRecoupRecoveryAvg")) != 0.0 {
                        // L3773
                        es().push(lfmt!(
                            "+ %.1f ^8(average energy shield recoup)",
                            output.num0(n!("EnergyShieldRecoupRecoveryAvg"))
                        ));
                    }
                    // L3775
                    es().push(lfmt!(
                        "- %.1f ^8(total energy shield degen)",
                        totalEnergyShieldDegen
                    ));
                    // L3776
                    es().push(lfmt!(
                        "= %.1f",
                        output.num(n!("ComprehensiveNetEnergyShieldRegen"))
                    ));
                    // L3777
                    breakdown.set(
                        n!("ComprehensiveTotalNetRegen"),
                        tbl![
                            lfmt!(
                                "Net Life Regen: %.1f",
                                output.num(n!("ComprehensiveNetLifeRegen"))
                            ),
                            lfmt!(
                                "+ Net Mana Regen: %.1f",
                                output.num(n!("ComprehensiveNetManaRegen"))
                            ),
                            lfmt!(
                                "+ Net Energy Shield Regen: %.1f",
                                output.num(n!("ComprehensiveNetEnergyShieldRegen"))
                            ),
                            lfmt!(
                                "= Total Net Regen: %.1f",
                                output.num(n!("ComprehensiveTotalNetRegen"))
                            ),
                        ],
                    );
                }
            }
        }
    }

    //region Add breakdown to enemy incoming damage
    // L3789
    if let Some(breakdown) = &breakdown {
        // L3790
        let takenHit = Table::new();
        // L3791
        for damageType in dmg_type_names() {
            let kHit = cat!(damageType, "TakenHit");
            // L3792 (`x > 0 and x or nil`: x is a number, never falsy)
            let v = if output.num(kHit) > 0.0 {
                Val::Num(output.num(kHit))
            } else {
                Val::Nil
            };
            takenHit.set(damageType, v);
            // L3793
            let row = Table::new();
            row.set(n!("type"), lfmt!("%s", damageType));
            row.set(
                n!("incoming"),
                lfmt!(
                    "%.1f incoming damage",
                    output.num(cat!(damageType, "TakenDamage"))
                ),
            );
            row.set(
                n!("mult"),
                lfmt!(
                    "x %.3f damage mult",
                    output.num(cat!(damageType, "TakenHitMult"))
                ),
            );
            row.set(n!("value"), lfmt!("%d", m_ceil(output.num(kHit))));
            row_list(&breakdown.get(n!("totalTakenHit"))).push(row);
        }
        // L3800
        let poolsRemaining = reducePoolsByDamage(None, &takenHit, &actor);

        // L3802
        entry_table(breakdown.get(n!("totalTakenHit")))
            .push("Such a hit would drain the following resources:");
        // L3803
        let cur = entry_table(breakdown.get(n!("totalTakenHit")));
        let v = incomingDamageBreakdown(&cur, &poolsRemaining, &output);
        breakdown.set(n!("totalTakenHit"), v);

        // L3805
        for damageType in dmg_type_names() {
            // L3806
            let breakdownTable = Table::new();
            // L3807
            let resourcesLost = poolsRemaining
                .get(n!("resourcesLostToTypeDamage"))
                .get(damageType);
            // L3808
            let mut resourcesLostSum = 0.0;

            // L3810
            breakdownTable.push(lfmt!("Final %s Damage taken:", damageType));
            // L3811
            breakdownTable.push(lfmt!(
                "%.1f incoming damage",
                output.num(cat!(damageType, "TakenDamage"))
            ));
            // L3812
            breakdownTable.push(lfmt!(
                "x %.3f damage mult",
                output.num(cat!(damageType, "TakenHitMult"))
            ));
            // L3813
            breakdownTable.push(lfmt!("= %.1f", output.num(cat!(damageType, "TakenHit"))));

            // L3815
            breakdownTable.push("This part of the hit drains the following resources:");
            // L3816
            for ally in &allyLifePoolList {
                // L3817
                let lost = resourcesLost.get(Name::from(ally.key));
                // L3818
                if lost.truthy() {
                    // L3819
                    resourcesLostSum = resourcesLostSum + lost.num();
                    // L3820
                    let f = format!("\t%d {}{}", color_code("GEM"), ally.label);
                    breakdownTable.push(lfmt!(f.as_str(), lost.num()));
                }
            }
            // L3823
            if resourcesLost.get(n!("aegis")).truthy() {
                let lost = resourcesLost.get(n!("aegis")).num();
                // L3824
                resourcesLostSum = resourcesLostSum + lost;
                // L3825
                let f = format!("\t%d {}%s Aegis charge", color_code("GEM"));
                breakdownTable.push(lfmt!(f.as_str(), lost, damageType));
            }
            // L3827
            if resourcesLost.get(n!("sharedElementalAegis")).truthy() {
                let lost = resourcesLost.get(n!("sharedElementalAegis")).num();
                // L3828
                resourcesLostSum = resourcesLostSum + lost;
                // L3829
                let f = format!("\t%d {}Elemental Aegis charge", color_code("GEM"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3831
            if resourcesLost.get(n!("sharedAegis")).truthy() {
                let lost = resourcesLost.get(n!("sharedAegis")).num();
                // L3832
                resourcesLostSum = resourcesLostSum + lost;
                // L3833
                let f = format!("\t%d {}Shared Aegis charge", color_code("GEM"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3835
            if resourcesLost.get(n!("guard")).truthy() {
                let lost = resourcesLost.get(n!("guard")).num();
                // L3836
                resourcesLostSum = resourcesLostSum + lost;
                // L3837
                let f = format!("\n\t%d {}%s Guard charge", color_code("SCOURGE"));
                breakdownTable.push(lfmt!(f.as_str(), lost, damageType));
            }
            // L3839
            if resourcesLost.get(n!("sharedGuard")).truthy() {
                let lost = resourcesLost.get(n!("sharedGuard")).num();
                // L3840
                resourcesLostSum = resourcesLostSum + lost;
                // L3841
                let f = format!("\t%d {}Shared Guard charge", color_code("SCOURGE"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3843
            if resourcesLost.get(n!("ward")).truthy() {
                let lost = resourcesLost.get(n!("ward")).num();
                // L3844
                resourcesLostSum = resourcesLostSum + lost;
                // L3845
                let f = format!("\t%d {}Ward", color_code("WARD"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3847
            if resourcesLost.get(n!("energyShield")).truthy() {
                let lost = resourcesLost.get(n!("energyShield")).num();
                // L3848
                resourcesLostSum = resourcesLostSum + lost;
                // L3849
                let f = format!("\t%d {}Energy Shield", color_code("ES"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3851
            if resourcesLost.get(n!("mana")).truthy() {
                let lost = resourcesLost.get(n!("mana")).num();
                // L3852
                resourcesLostSum = resourcesLostSum + lost;
                // L3853
                let f = format!("\t%d {}Mana", color_code("MANA"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3855
            let llp = resourcesLost.get(n!("lifeLossPrevented"));
            if llp.truthy() && llp.num() > 0.0 {
                // L3856
                resourcesLostSum = resourcesLostSum + llp.num();
                // L3857
                let f = format!("\t%d {}Life ^7Loss Prevented", color_code("LIFE"));
                breakdownTable.push(lfmt!(f.as_str(), llp.num()));
            }
            // L3859
            let life = resourcesLost.get(n!("life"));
            if life.truthy() && life.num() > 0.0 {
                // L3860
                resourcesLostSum = resourcesLostSum + life.num();
                // L3861
                let f = format!("\t%d {}Life", color_code("LIFE"));
                breakdownTable.push(lfmt!(f.as_str(), life.num()));
            }
            // L3863
            if resourcesLost.get(n!("overkill")).truthy() {
                let lost = resourcesLost.get(n!("overkill")).num();
                // L3864
                resourcesLostSum = resourcesLostSum + lost;
                // L3865
                let f = format!("\t%d {}Overkill damage", color_code("NEGATIVE"));
                breakdownTable.push(lfmt!(f.as_str(), lost));
            }
            // L3867
            breakdownTable.push(lfmt!("\t^8(%d total)", resourcesLostSum));
            // L3868
            breakdown.set(cat!(damageType, "TakenHit"), breakdownTable);
        }
    }
    //endregion

    ctx.output = output;
    ctx.breakdown = breakdown;
    Flow::Normal
}
