//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! defence segment DF-E (CalcDefence.lua 2153-2611): stun, Life Recoverable, prevented life
//! loss, Energy Shield bypass, Mind over Matter, Guard, Aegis, ally pools (Frost Shield, ally
//! redirects, Soul Link), Vaal Arctic Armour and the total pools.
//!
//! Owner: DF-E.
#![allow(non_snake_case, non_upper_case_globals, unused_assignments)]
// Lua statement shapes kept verbatim (`x = x * y`, `s = s .. t`; STYLE §3.1 rule 3).
#![allow(clippy::assign_op_pattern, clippy::string_add_assign)]

use super::helpers::{
    allyLifePoolList, calcLifeHitPoolWithLossPrevention, dmgTypeList, isElemental,
};
use super::DefEstCtx;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;

// ---- the segment --------------------------------------------------------------------------------

/// DF-E: CalcDefence.lua 2153-2611 (the local function `calcMoMEBPool`, 2351-2363, is
/// [`calcMoMEBPool`]).
/// @pob Modules/CalcDefence.lua:2153-2350 h=d058a741
/// @pob Modules/CalcDefence.lua:2364-2611 h=66913b3a
pub fn seg(ctx: &mut DefEstCtx) -> Flow {
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let modDB = ctx.modDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let damageCategoryConfig = ctx.damageCategoryConfig;
    let misc = &data().misc;

    // -- stun
    // L2154
    {
        // L2155
        let mut stunThresholdBase = 0.0;
        // L2156
        let mut stunThresholdSource: Val = Val::Nil;
        // L2157
        if modDB.Flag(None, nm!["StunThresholdBasedOnEnergyShieldInsteadOfLife"]) {
            // L2158
            let stunThresholdMult =
                modDB.Sum(ModType::BASE, None, nm!["StunThresholdEnergyShieldPercent"]);
            // L2159
            stunThresholdBase = output.num(n!("EnergyShield")) * stunThresholdMult / 100.0;
            // L2160
            stunThresholdSource =
                concat(&[Val::Num(stunThresholdMult), "% of Energy Shield".into()]);
        } else if modDB.Flag(None, nm!["StunThresholdBasedOnManaInsteadOfLife"]) {
            // L2161
            // L2162
            let stunThresholdMult = modDB.Sum(ModType::BASE, None, nm!["StunThresholdManaPercent"]);
            // L2163
            stunThresholdBase = output.num(n!("Mana")) * stunThresholdMult / 100.0;
            // L2164
            stunThresholdSource = concat(&[Val::Num(stunThresholdMult), "% of Mana".into()]);
        } else if modDB.Flag(None, nm!["ChaosInoculation"]) {
            // L2165
            // L2166
            stunThresholdBase = modDB.Sum(ModType::BASE, None, nm!["Life"]);
            // L2167
            stunThresholdSource = "Life before Chaos Inoculation".into();
        } else {
            // L2169
            stunThresholdBase = output.num(n!("Life"));
            // L2170
            stunThresholdSource = "Life".into();
        }
        // L2172
        if modDB.Flag(None, nm!["AddESToStunThreshold"]) {
            // L2173
            let ESMult = modDB.Sum(ModType::BASE, None, nm!["ESToStunThresholdPercent"]);
            // L2174
            stunThresholdBase = stunThresholdBase + output.num(n!("EnergyShield")) * ESMult / 100.0;
            // L2175
            stunThresholdSource = concat(&[
                stunThresholdSource,
                " and ".into(),
                Val::Num(ESMult),
                "% of Energy Shield".into(),
            ]);
        }
        // L2177
        let StunThresholdMod = 1.0 + modDB.Sum(ModType::INC, None, nm!["StunThreshold"]) / 100.0;
        // L2178
        output.set(n!("StunThreshold"), stunThresholdBase * StunThresholdMod);

        // L2180
        let mut notAvoidChance = if modDB.Flag(None, nm!["StunImmune"]) {
            0.0
        } else {
            100.0 - m_min(modDB.Sum(ModType::BASE, None, nm!["AvoidStun"]), 100.0)
        };
        // L2181
        if lt(
            &output.get(n!("totalTakenHit")),
            &output.get(n!("EnergyShield")),
        ) && !env.modDB.get().Flag(None, nm!["EnergyShieldProtectsMana"])
        {
            // L2182
            notAvoidChance = notAvoidChance * 0.5;
        }
        // L2184
        output.set(n!("StunAvoidChance"), 100.0 - notAvoidChance);

        // L2186
        if let Some(breakdown) = &breakdown {
            // L2187
            breakdown.set(
                n!("StunThreshold"),
                tbl![lfmt!(
                    "%d ^8(base from %s)",
                    stunThresholdBase,
                    fa(&stunThresholdSource)
                )],
            );
            // L2188
            if StunThresholdMod != 1.0 {
                // L2189
                t_insert(
                    breakdown,
                    n!("StunThreshold"),
                    lfmt!("* %.2f ^8(increased threshold)", StunThresholdMod),
                );
                // L2190
                t_insert(
                    breakdown,
                    n!("StunThreshold"),
                    lfmt!("= %d", fa(&output.get(n!("StunThreshold")))),
                );
            }
            // L2192
            breakdown.set(
                n!("StunAvoidChance"),
                tbl![
                    concat(&[colorCode("CUSTOM"), "NOTE: Having any energy shield when the hit occurs grants 50% chance to avoid stun.".into()]),
                    concat(&[colorCode("CUSTOM"), "POB only applies this modifier when ES > Total incoming damage.".into()]),
                ],
            );
        }
        // L2197
        if ge(&output.get(n!("StunAvoidChance")), &Val::Num(100.0)) {
            // L2198
            output.set(n!("StunDuration"), 0.0);
            // L2199
            output.set(n!("BlockDuration"), 0.0);
            // L2200
            if let Some(breakdown) = &breakdown {
                // L2201
                breakdown.set(n!("StunDuration"), tbl!["Cannot be Stunned"]);
                // L2202
                breakdown.set(n!("BlockDuration"), tbl!["Cannot be Stunned"]);
            }
        } else {
            // L2205
            let stunDuration = 1.0 + modDB.Sum(ModType::INC, None, nm!["StunDuration"]) / 100.0;
            // L2206
            let baseStunDuration = misc.StunBaseDuration;
            // L2207
            let stunRecovery = 1.0 + modDB.Sum(ModType::INC, None, nm!["StunRecovery"]) / 100.0;
            // L2208
            let stunAndBlockRecovery =
                1.0 + modDB.Sum(ModType::INC, None, nm!["StunRecovery", "BlockRecovery"]) / 100.0;
            // L2209
            output.set(
                n!("StunDuration"),
                m_ceil(baseStunDuration * stunDuration / stunRecovery * misc.ServerTickRate)
                    / misc.ServerTickRate,
            );
            // L2210
            output.set(
                n!("BlockDuration"),
                m_ceil(
                    baseStunDuration * stunDuration / stunAndBlockRecovery * misc.ServerTickRate,
                ) / misc.ServerTickRate,
            );
            // L2211
            if let Some(breakdown) = &breakdown {
                // L2212
                breakdown.set(
                    n!("StunDuration"),
                    tbl![lfmt!("%.2fs ^8(base)", baseStunDuration)],
                );
                // L2213
                breakdown.set(
                    n!("BlockDuration"),
                    tbl![lfmt!("%.2fs ^8(base)", baseStunDuration)],
                );
                // L2214
                if stunDuration != 1.0 {
                    // L2215
                    t_insert(
                        breakdown,
                        n!("StunDuration"),
                        lfmt!("* %.2f ^8(increased duration)", stunDuration),
                    );
                    // L2216
                    t_insert(
                        breakdown,
                        n!("BlockDuration"),
                        lfmt!("* %.2f ^8(increased duration)", stunDuration),
                    );
                }
                // L2218
                if stunRecovery != 1.0 {
                    // L2219
                    t_insert(
                        breakdown,
                        n!("StunDuration"),
                        lfmt!("/ %.2f ^8(increased/reduced recovery)", stunRecovery),
                    );
                }
                // L2221
                if stunAndBlockRecovery != 1.0 {
                    // L2222
                    t_insert(
                        breakdown,
                        n!("BlockDuration"),
                        lfmt!(
                            "/ %.2f ^8(increased/reduced block recovery)",
                            stunAndBlockRecovery
                        ),
                    );
                }
                // L2224
                t_insert(
                    breakdown,
                    n!("StunDuration"),
                    lfmt!("rounded up to nearest server tick"),
                );
                // L2225
                t_insert(
                    breakdown,
                    n!("StunDuration"),
                    lfmt!("= %.2fs", fa(&output.get(n!("StunDuration")))),
                );

                // L2227
                t_insert(
                    breakdown,
                    n!("BlockDuration"),
                    lfmt!("rounded up to nearest server tick"),
                );
                // L2228
                t_insert(
                    breakdown,
                    n!("BlockDuration"),
                    lfmt!("= %.2fs", fa(&output.get(n!("BlockDuration")))),
                );
            }
        }
        // L2231
        output.set(
            n!("InterruptStunAvoidChance"),
            m_min(
                modDB.Sum(ModType::BASE, None, nm!["AvoidInterruptStun"]),
                100.0,
            ),
        );
        // L2232
        let mut effectiveEnemyDamage =
            output.num(n!("totalTakenHit")) + output.num(n!("PhysicalTakenHit")) * 0.25;
        // L2233
        if damageCategoryConfig != n!("Average") {
            // L2234
            effectiveEnemyDamage =
                effectiveEnemyDamage * (1.0 + misc.StunNotMeleeDamageMult * 3.0) / 4.0;
        } else if damageCategoryConfig != n!("Melee") {
            // L2235
            // L2236
            effectiveEnemyDamage = effectiveEnemyDamage * misc.StunNotMeleeDamageMult;
        }
        // L2238
        let baseStunChance = m_min(
            misc.StunBaseMult * effectiveEnemyDamage / output.num(n!("StunThreshold")),
            100.0,
        );
        // L2239
        output.set(
            n!("SelfStunChance"),
            (if baseStunChance > misc.MinStunChanceNeeded {
                baseStunChance
            } else {
                0.0
            }) * notAvoidChance
                / 100.0,
        );
        // L2240
        if let Some(breakdown) = &breakdown {
            // L2241
            breakdown.set(
                n!("SelfStunChance"),
                tbl![
                    lfmt!("%d%% ^8(stun multiplier)", misc.StunBaseMult),
                    lfmt!(
                        "* %.1f ^8(effective enemy stun damage)",
                        effectiveEnemyDamage
                    ),
                    lfmt!(
                        "/ %d ^8(stun threshold)",
                        fa(&output.get(n!("StunThreshold")))
                    ),
                ],
            );
            // L2246
            if baseStunChance < misc.MinStunChanceNeeded {
                // L2247
                t_insert(
                    breakdown,
                    n!("SelfStunChance"),
                    lfmt!("= %.2f%%", baseStunChance),
                );
                // L2248
                t_insert(
                    breakdown,
                    n!("SelfStunChance"),
                    lfmt!(
                        "Stun Chance has to be more than %d%% to stun.",
                        misc.MinStunChanceNeeded
                    ),
                );
            } else {
                // L2250
                if notAvoidChance != 100.0 {
                    // L2251
                    t_insert(
                        breakdown,
                        n!("SelfStunChance"),
                        lfmt!(
                            "* %.2f ^8(1 - chance to avoid stun)",
                            notAvoidChance / 100.0
                        ),
                    );
                }
                // L2253
                t_insert(
                    breakdown,
                    n!("SelfStunChance"),
                    lfmt!("= %.2f%%", fa(&output.get(n!("SelfStunChance")))),
                );
            }
        }
    }

    // -- Life Recoverable
    // L2259
    output.set(n!("LifeRecoverable"), output.get(n!("LifeUnreserved")));
    // L2260
    if env.configInput.truthy(n!("conditionLowLife")) {
        // L2261
        output.set(
            n!("LifeRecoverable"),
            m_min(
                output.num(n!("Life"))
                    * output.num_or(n!("LowLifePercentage"), misc.LowPoolThreshold)
                    / 100.0,
                output.num(n!("LifeUnreserved")),
            ),
        );
        // L2262
        if lt(
            &output.get(n!("LifeRecoverable")),
            &output.get(n!("LifeUnreserved")),
        ) {
            // L2263
            output.set(n!("CappingLife"), true);
        }
    }

    // -- Dissolution of the flesh life pool change
    // L2268
    if modDB.Flag(None, nm!["DamageInsteadReservesLife"]) {
        // L2269
        output.set(
            n!("LifeRecoverable"),
            (output.num(n!("LifeCancellableReservation")) / 100.0) * output.num(n!("Life")),
        );
    }

    // L2272
    output.set(
        n!("LifeRecoverable"),
        m_max(output.num(n!("LifeRecoverable")), 1.0),
    );

    // -- Prevented life loss taken over 4 seconds (and Petrified Blood)
    // L2275
    {
        // L2276
        let recoverable = output.num(n!("LifeRecoverable"));
        // L2277
        let preventedLifeLoss = m_min(
            modDB.Sum(ModType::BASE, None, nm!["LifeLossPrevented"]),
            100.0,
        );
        // L2278
        output.set(n!("preventedLifeLoss"), preventedLifeLoss);
        // L2279
        let initialLifeLossBelowHalfPrevented =
            modDB.Sum(ModType::BASE, None, nm!["LifeLossBelowHalfPrevented"]);
        // L2280
        output.set(
            n!("preventedLifeLossBelowHalf"),
            (1.0 - output.num(n!("preventedLifeLoss")) / 100.0) * initialLifeLossBelowHalfPrevented,
        );
        // L2281
        let mut portionLife = 1.0;
        // L2282
        if !env.configInput.truthy(n!("conditionLowLife")) {
            // --portion of life that is lowlife
            // L2284
            portionLife = m_min(output.num(n!("Life")) * 0.5 / recoverable, 1.0);
            // L2285
            output.set(
                n!("preventedLifeLossTotal"),
                output.num(n!("preventedLifeLoss"))
                    + output.num(n!("preventedLifeLossBelowHalf")) * portionLife,
            );
        } else {
            // L2287
            output.set(
                n!("preventedLifeLossTotal"),
                output.num(n!("preventedLifeLoss")) + output.num(n!("preventedLifeLossBelowHalf")),
            );
        }
        // L2289
        output.set(
            n!("LifeHitPool"),
            calcLifeHitPoolWithLossPrevention(
                recoverable,
                output.num(n!("Life")),
                preventedLifeLoss,
                initialLifeLossBelowHalfPrevented,
            ),
        );

        // L2291
        if let Some(breakdown) = &breakdown {
            let k = n!("preventedLifeLossTotal");
            // L2292
            breakdown.set(k, tbl![lfmt!("Total life protected:")]);
            // L2295
            if output.num(n!("preventedLifeLoss")) != 0.0 {
                // L2296
                t_insert(
                    breakdown,
                    k,
                    lfmt!(
                        "%.2f ^8(portion taken over 4 seconds instead)",
                        output.num(n!("preventedLifeLoss")) / 100.0
                    ),
                );
            }
            // L2298
            if output.num(n!("preventedLifeLossBelowHalf")) != 0.0 {
                // L2299
                if portionLife != 1.0 {
                    // L2300
                    if output.num(n!("preventedLifeLoss")) != 0.0 {
                        // L2301
                        t_insert(breakdown, k, lfmt!(""));
                    }
                    // L2303
                    t_insert(
                        breakdown,
                        k,
                        lfmt!(
                            "%s%.2f ^8(initial portion taken by petrified blood)",
                            if output.num(n!("preventedLifeLoss")) != 0.0 {
                                "+ "
                            } else {
                                ""
                            },
                            initialLifeLossBelowHalfPrevented / 100.0
                        ),
                    );
                    // L2304
                    if output.num(n!("preventedLifeLoss")) != 0.0 {
                        // L2305
                        t_insert(
                            breakdown,
                            k,
                            lfmt!(
                                "* %.2f ^8(portion not already taken over time)",
                                1.0 - output.num(n!("preventedLifeLoss")) / 100.0
                            ),
                        );
                    }
                    // L2307
                    t_insert(
                        breakdown,
                        k,
                        lfmt!("* %.2f ^8(portion of life on low life)", portionLife),
                    );
                    // L2308
                    t_insert(
                        breakdown,
                        k,
                        lfmt!(
                            "= %.2f ^8(final portion taken by petrified blood)",
                            output.num(n!("preventedLifeLossBelowHalf")) * portionLife / 100.0
                        ),
                    );
                    // L2309
                    t_insert(breakdown, k, lfmt!(""));
                } else {
                    // L2311
                    t_insert(
                        breakdown,
                        k,
                        lfmt!(
                            "%s%.2f ^8(%s taken by petrified blood)",
                            if output.num(n!("preventedLifeLoss")) != 0.0 {
                                "+ "
                            } else {
                                ""
                            },
                            initialLifeLossBelowHalfPrevented / 100.0,
                            if output.num(n!("preventedLifeLoss")) != 0.0 {
                                "initial portion"
                            } else {
                                "portion"
                            }
                        ),
                    );
                    // L2312
                    if output.num(n!("preventedLifeLoss")) != 0.0 {
                        // L2313
                        t_insert(
                            breakdown,
                            k,
                            lfmt!(
                                "* %.2f ^8(portion not already taken over time)",
                                1.0 - output.num(n!("preventedLifeLoss")) / 100.0
                            ),
                        );
                        // L2314
                        t_insert(
                            breakdown,
                            k,
                            lfmt!(
                                "= %.2f ^8(final portion taken by petrified blood)",
                                output.num(n!("preventedLifeLossBelowHalf")) / 100.0
                            ),
                        );
                    }
                }
            }
            // L2318
            t_insert(
                breakdown,
                k,
                lfmt!(
                    "%.2f ^8(portion taken from life)",
                    1.0 - output.num(n!("preventedLifeLossTotal")) / 100.0
                ),
            );
        }
    }

    // -- Energy Shield bypass
    // L2323
    output.set(n!("AnyBypass"), false);
    // L2324
    output.set(n!("MinimumBypass"), 100.0);
    // L2325
    for damageType in dmgTypeList {
        let damageType = Name::from(damageType);
        let kBypass = cat!(damageType, "EnergyShieldBypass");
        // L2326
        if modDB.Flag(None, nm!["UnblockedDamageDoesBypassES"]) {
            // L2327
            output.set(kBypass, 100.0);
            // L2328
            output.set(n!("AnyBypass"), true);
        } else {
            // L2330 (`or 0` is dead: Sum never returns nil)
            output.set(
                kBypass,
                lor(Val::from_value(&modDB.Override(None, &[kBypass])), || {
                    Val::Num(modDB.Sum(ModType::BASE, None, &[kBypass]))
                }),
            );
            // L2331
            if ne0(&output.get(kBypass)) {
                // L2332
                output.set(n!("AnyBypass"), true);
            }
            // L2334
            if damageType == n!("Chaos") {
                // L2335
                if !modDB.Flag(None, nm!["ChaosNotBypassEnergyShield"]) {
                    // L2336
                    output.set(kBypass, output.num(kBypass) + 100.0);
                } else {
                    // L2338
                    output.set(n!("AnyBypass"), true);
                }
            }
        }
        // L2342
        output.set(kBypass, m_max(m_min(output.num(kBypass), 100.0), 0.0));
        // L2343
        output.set(
            n!("MinimumBypass"),
            m_min(output.num(n!("MinimumBypass")), output.num(kBypass)),
        );
    }

    // L2346
    output.set(n!("ehpSectionAnySpecificTypes"), false);
    // -- Mind over Matter
    // L2348
    output.set(n!("OnlySharedMindOverMatter"), false);
    // L2349
    output.set(n!("AnySpecificMindOverMatter"), false);
    // L2350
    output.set(
        n!("sharedMindOverMatter"),
        m_min(
            modDB.Sum(ModType::BASE, None, nm!["DamageTakenFromManaBeforeLife"]),
            100.0,
        ),
    );
    // L2351-2363: `local function calcMoMEBPool` is [`calcMoMEBPool`].
    // L2364
    if output.num(n!("sharedMindOverMatter")) > 0.0 {
        // L2365
        let MindOverMatter = output.num(n!("sharedMindOverMatter")) / 100.0;
        // L2366
        let esBypass = output.num(n!("MinimumBypass")) / 100.0;

        // L2368
        let (sharedMoMPool, breakdownMaxMana, breakdownMana, breakdownES) = calcMoMEBPool(
            &modDB,
            &output,
            output.num(n!("LifeRecoverable")),
            MindOverMatter,
            esBypass,
        );
        // L2369
        output.set(n!("sharedManaEffectiveLife"), sharedMoMPool);
        // L2370
        output.set(
            n!("sharedMoMHitPool"),
            calcMoMEBPool(
                &modDB,
                &output,
                output.num(n!("LifeHitPool")),
                MindOverMatter,
                esBypass,
            )
            .0,
        );

        // L2372
        if let Some(breakdown) = &breakdown {
            // L2373
            if output.truthy(n!("sharedMindOverMatter")) {
                let k = n!("sharedMindOverMatter");
                // L2374
                let lifeProtected =
                    (breakdownMana + breakdownES) / MindOverMatter - (breakdownMana + breakdownES);
                // L2375
                breakdown.set(
                    k,
                    tbl![
                        lfmt!("Effective life from Mind over Matter:"),
                        lfmt!(
                            "%d life (%d life is protected)",
                            fa(&output.get(n!("LifeRecoverable"))),
                            lifeProtected
                        ),
                        lfmt!(
                            "%d mana%s",
                            breakdownMana,
                            if breakdownMana == breakdownMaxMana {
                                String::new()
                            } else {
                                lfmt!(
                                    " (%d mana shadowed by energy shield)",
                                    breakdownMaxMana - breakdownMana
                                )
                            }
                            .as_str()
                        ),
                    ],
                );
                // L2380
                if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
                    t_insert(breakdown, k, lfmt!("%d energy shield", breakdownES));
                }
                // L2381
                t_insert(
                    breakdown,
                    k,
                    lfmt!(
                        "%d + %d %s= %d",
                        fa(&output.get(n!("LifeRecoverable"))),
                        breakdownMana,
                        if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
                            lfmt!("+ %d ", breakdownES)
                        } else {
                            String::new()
                        }
                        .as_str(),
                        sharedMoMPool
                    ),
                );
            }
        }
    } else {
        // L2387
        output.set(
            n!("sharedManaEffectiveLife"),
            output.get(n!("LifeRecoverable")),
        );
        // L2388
        output.set(n!("sharedMoMHitPool"), output.get(n!("LifeHitPool")));
    }
    // L2390
    for damageType in dmgTypeList {
        let damageType = Name::from(damageType);
        let kMoM = cat!(damageType, "MindOverMatter");
        let kBypass = cat!(damageType, "EnergyShieldBypass");
        // L2391
        output.set(
            kMoM,
            m_min(
                modDB.Sum(
                    ModType::BASE,
                    None,
                    &[cat!(damageType, "DamageTakenFromManaBeforeLife")],
                ),
                100.0 - output.num(n!("sharedMindOverMatter")),
            ),
        );
        // L2392
        if output.num(kMoM) > 0.0
            || (output.num(kBypass) > output.num(n!("MinimumBypass"))
                && output.num(n!("sharedMindOverMatter")) > 0.0)
        {
            // L2393
            output.set(n!("ehpSectionAnySpecificTypes"), true);
            // L2394
            output.set(n!("AnySpecificMindOverMatter"), true);
            // L2395
            output.set(n!("OnlySharedMindOverMatter"), false);
            // L2396
            let MindOverMatter =
                (output.num(kMoM) + output.num(n!("sharedMindOverMatter"))) / 100.0;
            // L2397
            let esBypass = output.num(kBypass) / 100.0;

            // L2399
            let (typedMoMPool, breakdownMaxMana, breakdownMana, breakdownES) = calcMoMEBPool(
                &modDB,
                &output,
                output.num(n!("LifeRecoverable")),
                MindOverMatter,
                esBypass,
            );
            // L2400
            output.set(cat!(damageType, "ManaEffectiveLife"), typedMoMPool);
            // L2401
            output.set(
                cat!(damageType, "MoMHitPool"),
                calcMoMEBPool(
                    &modDB,
                    &output,
                    output.num(n!("LifeHitPool")),
                    MindOverMatter,
                    esBypass,
                )
                .0,
            );

            // L2403
            if let Some(breakdown) = &breakdown {
                // L2404
                if output.truthy(kMoM) {
                    // L2405
                    let lifeProtected = (breakdownMana + breakdownES) / MindOverMatter
                        - (breakdownMana + breakdownES);
                    // L2406
                    breakdown.set(
                        kMoM,
                        tbl![
                            lfmt!("Effective life from Mind over Matter:"),
                            lfmt!(
                                "%d life (%d life is protected)",
                                fa(&output.get(n!("LifeRecoverable"))),
                                lifeProtected
                            ),
                            lfmt!(
                                "%d mana%s",
                                breakdownMana,
                                if breakdownMana == breakdownMaxMana {
                                    String::new()
                                } else {
                                    lfmt!(
                                        " (%d mana shadowed by energy shield)",
                                        breakdownMaxMana - breakdownMana
                                    )
                                }
                                .as_str()
                            ),
                        ],
                    );
                    // L2411
                    if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
                        t_insert(breakdown, kMoM, lfmt!("%d energy shield", breakdownES));
                    }
                    // L2412
                    t_insert(
                        breakdown,
                        kMoM,
                        lfmt!(
                            "%d + %d %s= %d",
                            fa(&output.get(n!("LifeRecoverable"))),
                            breakdownMana,
                            if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
                                lfmt!("+ %d ", breakdownES)
                            } else {
                                String::new()
                            }
                            .as_str(),
                            typedMoMPool
                        ),
                    );
                }
            }
        } else {
            // L2418
            output.set(
                cat!(damageType, "ManaEffectiveLife"),
                output.get(n!("sharedManaEffectiveLife")),
            );
            // L2419
            output.set(
                cat!(damageType, "MoMHitPool"),
                output.get(n!("sharedMoMHitPool")),
            );
        }
    }

    // -- Guard
    // L2424
    output.set(n!("AnyGuard"), false);
    // L2425
    output.set(
        n!("sharedGuardAbsorbRate"),
        m_min(
            modDB.Sum(ModType::BASE, None, nm!["GuardAbsorbRate"]),
            100.0,
        ),
    );
    // L2426
    if output.num(n!("sharedGuardAbsorbRate")) > 0.0 {
        // L2427
        output.set(n!("OnlySharedGuard"), true);
        // L2428
        output.set(
            n!("sharedGuardAbsorb"),
            calcLib::val(&modDB, n!("GuardAbsorbLimit"), None),
        );
        // L2429
        let lifeProtected = output.num(n!("sharedGuardAbsorb"))
            / (output.num(n!("sharedGuardAbsorbRate")) / 100.0)
            * (1.0 - output.num(n!("sharedGuardAbsorbRate")) / 100.0);
        // L2430
        if let Some(breakdown) = &breakdown {
            // L2431
            breakdown.set(
                n!("sharedGuardAbsorb"),
                tbl![
                    lfmt!("Total life protected:"),
                    lfmt!(
                        "%d ^8(guard limit)",
                        fa(&output.get(n!("sharedGuardAbsorb")))
                    ),
                    lfmt!(
                        "/ %.2f ^8(portion taken from guard)",
                        output.num(n!("sharedGuardAbsorbRate")) / 100.0
                    ),
                    lfmt!(
                        "x %.2f ^8(portion taken from life and energy shield)",
                        1.0 - output.num(n!("sharedGuardAbsorbRate")) / 100.0
                    ),
                    lfmt!("= %d", lifeProtected),
                ],
            );
        }
    }
    // L2440
    for damageType in dmgTypeList {
        let damageType = Name::from(damageType);
        let kRate = cat!(damageType, "GuardAbsorbRate");
        let kAbsorb = cat!(damageType, "GuardAbsorb");
        // L2441
        output.set(
            kRate,
            m_min(modDB.Sum(ModType::BASE, None, &[kRate]), 100.0),
        );
        // L2442
        if output.num(kRate) > 0.0 {
            // L2443
            output.set(n!("ehpSectionAnySpecificTypes"), true);
            // L2444
            output.set(n!("AnyGuard"), true);
            // L2445
            output.set(n!("OnlySharedGuard"), false);
            // L2446
            output.set(
                kAbsorb,
                calcLib::val(&modDB, cat!(damageType, "GuardAbsorbLimit"), None),
            );
            // L2447
            let lifeProtected = output.num(kAbsorb) / (output.num(kRate) / 100.0)
                * (1.0 - output.num(kRate) / 100.0);
            // L2448
            if let Some(breakdown) = &breakdown {
                // L2449
                breakdown.set(
                    kAbsorb,
                    tbl![
                        lfmt!("Total life protected:"),
                        lfmt!("%d ^8(guard limit)", fa(&output.get(kAbsorb))),
                        lfmt!(
                            "/ %.2f ^8(portion taken from guard)",
                            output.num(kRate) / 100.0
                        ),
                        lfmt!(
                            "x %.2f ^8(portion taken from life and energy shield)",
                            1.0 - output.num(kRate) / 100.0
                        ),
                        lfmt!("= %d", lifeProtected),
                    ],
                );
            }
        }
    }

    // --aegis
    // L2461
    output.set(n!("AnyAegis"), false);
    // L2462
    output.set(
        n!("sharedAegis"),
        modDB.Max(None, nm!["AegisValue"]).unwrap_or(0.0),
    );
    // L2463
    output.set(
        n!("sharedElementalAegis"),
        modDB.Max(None, nm!["ElementalAegisValue"]).unwrap_or(0.0),
    );
    // L2464
    if output.num(n!("sharedAegis")) > 0.0 {
        // L2465
        output.set(n!("AnyAegis"), true);
    }
    // L2467
    if output.num(n!("sharedElementalAegis")) > 0.0 {
        // L2468
        output.set(n!("ehpSectionAnySpecificTypes"), true);
        // L2469
        output.set(n!("AnyAegis"), true);
    }
    // L2471
    for damageType in dmgTypeList {
        let damageType = Name::from(damageType);
        let kAegis = cat!(damageType, "Aegis");
        // L2472
        let aegisValue = modDB
            .Max(None, &[cat!(damageType, "AegisValue")])
            .unwrap_or(0.0);
        // L2473
        if aegisValue > 0.0 {
            // L2474
            output.set(n!("ehpSectionAnySpecificTypes"), true);
            // L2475
            output.set(n!("AnyAegis"), true);
            // L2476
            output.set(kAegis, aegisValue);
        } else {
            // L2478
            output.set(kAegis, 0.0);
        }
        // L2480
        if isElemental(damageType.as_str()) {
            // L2481
            output.set(
                cat!(damageType, "AegisDisplay"),
                output.num(kAegis) + output.num(n!("sharedElementalAegis")),
            );
        }
    }

    // -- taken from allies before you, eg. frost shield
    // L2486
    {
        // -- frost shield
        // L2488
        output.set(
            n!("FrostShieldLife"),
            modDB.Sum(ModType::BASE, None, nm!["FrostGlobeHealth"]),
        );
        // L2489
        output.set(
            n!("FrostShieldDamageMitigation"),
            modDB.Sum(ModType::BASE, None, nm!["FrostGlobeDamageMitigation"]),
        );

        // L2491
        let lifeProtected = output.num(n!("FrostShieldLife"))
            / (output.num(n!("FrostShieldDamageMitigation")) / 100.0)
            * (1.0 - output.num(n!("FrostShieldDamageMitigation")) / 100.0);
        // L2492
        if let Some(breakdown) = &breakdown {
            // L2493
            breakdown.set(
                n!("FrostShieldLife"),
                tbl![
                    lfmt!("Total life protected:"),
                    lfmt!(
                        "%d ^8(frost shield limit)",
                        fa(&output.get(n!("FrostShieldLife")))
                    ),
                    lfmt!(
                        "/ %.2f ^8(portion taken from frost shield)",
                        output.num(n!("FrostShieldDamageMitigation")) / 100.0
                    ),
                    lfmt!(
                        "x %.2f ^8(portion taken from life and energy shield)",
                        1.0 - output.num(n!("FrostShieldDamageMitigation")) / 100.0
                    ),
                    lfmt!("= %d", lifeProtected),
                ],
            );
        }

        // -- Every ally redirect uses the same pool rules; the Safeguarding Golem is the only melee-only case.
        // L2503
        for ally in &allyLifePoolList {
            let allyLife = Name::from(ally.life);
            let allyMitigation = Name::from(ally.mitigation);
            // L2504
            if let Some(redirect) = ally.redirect {
                // L2505
                let mut mitigation = modDB.Sum(ModType::BASE, None, &[Name::from(redirect)]);
                // L2506
                if ally.meleeOnly {
                    // L2507
                    mitigation = if damageCategoryConfig == n!("Melee") {
                        mitigation
                    } else if damageCategoryConfig == n!("Average") {
                        mitigation / 4.0
                    } else {
                        0.0
                    };
                }
                // L2509
                output.set(allyMitigation, mitigation);
                // L2510
                if mitigation != 0.0 {
                    // L2511
                    let mut life = modDB.Sum(ModType::BASE, None, &[allyLife]);
                    // L2512
                    if life == 0.0 {
                        if let Some(fallback) = ally.fallback {
                            // L2513
                            life = modDB.Sum(ModType::BASE, None, &[Name::from(fallback)]);
                        }
                    }
                    // L2515
                    output.set(
                        allyLife,
                        lor(Val::from_value(&modDB.Override(None, &[allyLife])), || {
                            Val::Num(life)
                        }),
                    );
                }
            }
        }

        // -- Companionship and an ally-specific redirect can both spend the same minion's Life.
        // L2521
        let mut sharedMinionLifeOverride: Val = Val::Nil;
        // L2522
        for ally in &allyLifePoolList {
            let allyLife = Name::from(ally.life);
            let allyMitigation = Name::from(ally.mitigation);
            // L2523
            if modDB.Flag(None, &[cat!("MinionLifeShares", allyLife)]) {
                // L2524
                let specificOverride = Val::from_value(&modDB.Override(None, &[allyLife]));
                // L2525
                if !Val::from_value(&modDB.Override(None, nm!["TotalMinionLife"])).truthy()
                    && !specificOverride.is_nil()
                {
                    // L2526
                    output.set(n!("TotalMinionLife"), specificOverride);
                    // L2527
                    sharedMinionLifeOverride = Val::Bool(true);
                } else if !output.truthy(n!("TotalMinionLife"))
                    || output.get(n!("TotalMinionLife")).raw_eq(&Val::Num(0.0))
                {
                    // L2528
                    // L2529
                    output.set(n!("TotalMinionLife"), output.get(allyLife));
                }
                // L2531
                output.set(
                    n!("MinionAllyDamageMitigation"),
                    output.num(n!("MinionAllyDamageMitigation"))
                        + output.num_or(allyMitigation, 0.0),
                );
                // L2532
                output.set(allyMitigation, 0.0);
                // L2533
                output.set(allyLife, Val::Nil);
            }
        }

        // -- When Vaal Rejuvenation is treated as the nearest Totem, both redirects use its Life.
        // L2538
        if output.num_or(n!("TotemAllyDamageMitigation"), 0.0) > 0.0
            && output.num_or(n!("TotalTotemLife"), 0.0) == 0.0
            && output.num_or(n!("TotalVaalRejuvenationTotemLife"), 0.0) > 0.0
        {
            // L2539
            output.set(
                n!("VaalRejuvenationTotemAllyDamageMitigation"),
                output.num(n!("VaalRejuvenationTotemAllyDamageMitigation"))
                    + output.num(n!("TotemAllyDamageMitigation")),
            );
            // L2540
            output.set(n!("TotemAllyDamageMitigation"), 0.0);
            // L2541
            output.set(n!("TotalTotemLife"), Val::Nil);
        }

        // L2544
        if let Some(breakdown) = &breakdown {
            // L2545
            for ally in &allyLifePoolList {
                let allyLife = Name::from(ally.life);
                // L2546
                let lifeList: Val = match actor.allyLifeList.get() {
                    Some(t) => t.get(allyLife),
                    None => Val::Nil,
                };
                // L2547
                let r#override = lor(Val::from_value(&modDB.Override(None, &[allyLife])), || {
                    land(Val::Bool(ally.life == "TotalMinionLife"), || {
                        land(sharedMinionLifeOverride.clone(), || {
                            output.get(n!("TotalMinionLife"))
                        })
                    })
                });
                // L2548
                if output.truthy(allyLife) && (lifeList.truthy() || r#override.truthy()) {
                    // L2549
                    breakdown.set(allyLife, Table::new());
                    // L2550
                    if r#override.truthy() {
                        // L2551
                        t_insert(
                            breakdown,
                            allyLife,
                            lfmt!("%d ^8(from config)", fa(&output.get(allyLife))),
                        );
                    } else {
                        // L2553
                        let lifeList = match lifeList.table() {
                            Some(t) => t,
                            None => lua_error!(
                                "bad argument #1 to 'ipairs' (table expected, got {})",
                                lifeList.lua_type()
                            ),
                        };
                        for_ipairs!((_, entry) in lifeList => {
                            // L2554
                            let count = entry.get(n!("count"));
                            let countText = if count.truthy() && lt(&Val::Num(1.0), &count) {
                                concat(&[count.clone(), "x ".into()])
                            } else {
                                Val::from("")
                            };
                            t_insert(
                                breakdown,
                                allyLife,
                                lfmt!(
                                    "%d ^8(%s%s)",
                                    fa(&entry.get(n!("life"))),
                                    fa(&countText),
                                    fa(&entry.get(n!("name")))
                                ),
                            );
                        });
                    }
                }
            }
        }

        // -- from Allied Energy Shield
        // L2562
        output.set(
            n!("SoulLinkMitigation"),
            modDB.Sum(ModType::BASE, None, nm!["TakenFromParentESBeforeYou"]),
        );
        // L2563
        if output.num(n!("SoulLinkMitigation")) != 0.0 {
            // L2564
            let parent = match actor.parent() {
                Some(p) => p,
                None => lua_error!("attempt to index field 'parent' (a nil value)"),
            };
            output.set(
                n!("AlliedEnergyShield"),
                parent
                    .output
                    .get()
                    .num_or(n!("EnergyShieldRecoveryCap"), 0.0),
            );
        } else {
            // L2566
            output.set(
                n!("SoulLinkMitigation"),
                modDB.Sum(ModType::BASE, None, nm!["TakenFromPartyMemberESBeforeYou"]),
            );
            // L2567
            if output.num(n!("SoulLinkMitigation")) != 0.0 {
                // L2568
                let partyMembers = match actor.partyMembers.get().upgrade() {
                    Some(p) => p,
                    None => lua_error!("attempt to index field 'partyMembers' (a nil value)"),
                };
                output.set(
                    n!("AlliedEnergyShield"),
                    partyMembers
                        .output
                        .get()
                        .num_or(n!("EnergyShieldRecoveryCap"), 0.0),
                );
            }
        }
    }

    // -- Vaal Arctic Armour
    // L2574
    {
        // L2575
        output.set(
            n!("VaalArcticArmourLife"),
            modDB.Sum(ModType::BASE, None, nm!["VaalArcticArmourMaxHits"]),
        );
        // L2576
        output.set(
            n!("VaalArcticArmourMitigation"),
            m_min(
                -modDB.Sum(ModType::MORE, None, nm!["VaalArcticArmourMitigation"]) / 100.0,
                1.0,
            ),
        );
    }

    // --total pool
    // L2580
    for damageType in dmgTypeList {
        let damageType = Name::from(damageType);
        let kPool = cat!(damageType, "TotalPool");
        let kHitPool = cat!(damageType, "TotalHitPool");
        let kBypass = cat!(damageType, "EnergyShieldBypass");
        let kMEL = cat!(damageType, "ManaEffectiveLife");
        // L2581
        output.set(kPool, output.get(kMEL));
        // L2582
        output.set(kHitPool, output.get(cat!(damageType, "MoMHitPool")));
        // L2583
        let mut manatext = String::from("Mana");
        // L2584
        if output.num(kBypass) < 100.0 {
            // L2585
            if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
                // L2586
                manatext = manatext + " and non-bypassed Energy Shield";
            } else {
                // L2588
                if output.num(kBypass) > 0.0 {
                    // L2589
                    let poolProtected = output.num(n!("EnergyShieldRecoveryCap"))
                        / (1.0 - output.num(kBypass) / 100.0)
                        * (output.num(kBypass) / 100.0);
                    // L2590
                    output.set(
                        kPool,
                        m_max(output.num(kPool) - poolProtected, 0.0)
                            + m_min(output.num(kPool), poolProtected)
                                / (output.num(kBypass) / 100.0),
                    );
                    // L2591
                    output.set(
                        kHitPool,
                        m_max(output.num(kHitPool) - poolProtected, 0.0)
                            + m_min(output.num(kHitPool), poolProtected)
                                / (output.num(kBypass) / 100.0),
                    );
                } else {
                    // L2593
                    output.set(
                        kPool,
                        output.num(kPool) + output.num(n!("EnergyShieldRecoveryCap")),
                    );
                    // L2594
                    output.set(
                        kHitPool,
                        output.num(kHitPool) + output.num(n!("EnergyShieldRecoveryCap")),
                    );
                }
            }
        }
        // L2598
        if let Some(breakdown) = &breakdown {
            // L2599
            breakdown.set(
                kPool,
                tbl![lfmt!("Life: %d", fa(&output.get(n!("LifeRecoverable"))))],
            );
            // L2602
            if !output.get(kMEL).raw_eq(&output.get(n!("LifeRecoverable"))) {
                // L2603
                t_insert(
                    breakdown,
                    kPool,
                    lfmt!(
                        "%s through MoM: %d",
                        manatext.as_str(),
                        output.num(kMEL) - output.num(n!("LifeRecoverable"))
                    ),
                );
            }
            // L2605
            if (!modDB.Flag(None, nm!["EnergyShieldProtectsMana"])) && output.num(kBypass) < 100.0 {
                // L2606
                t_insert(
                    breakdown,
                    kPool,
                    lfmt!(
                        "Non-bypassed Energy Shield: %d",
                        output.num(kPool) - output.num(kMEL)
                    ),
                );
            }
            // L2608
            t_insert(
                breakdown,
                kPool,
                lfmt!("TotalPool: %d", fa(&output.get(kPool))),
            );
        }
    }

    Flow::Normal
}

/// `local function calcMoMEBPool(lifePool, MoMEffect, esBypass)` (a closure over `output` and
/// `modDB`) → `(lifePool + manaUsed + maxESUsable, maxManaUsable, manaUsed, maxESUsable)`.
/// @pob Modules/CalcDefence.lua:2351-2363 h=0f5b099c
fn calcMoMEBPool(
    modDB: &ModDB,
    output: &Output,
    lifePool: f64,
    MoMEffect: f64,
    esBypass: f64,
) -> (f64, f64, f64, f64) {
    // L2352
    let mana = m_max(output.num_or(n!("ManaUnreserved"), 0.0), 0.0);
    // L2353
    let maxMoMPool = if MoMEffect < 1.0 {
        lifePool / (1.0 - MoMEffect) - lifePool
    } else {
        m_huge
    };
    // L2354
    let maxManaUsable = m_floor(m_min(mana, maxMoMPool));
    // L2355
    let maxESUsable = if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) && esBypass < 1.0 {
        m_floor(m_min_n(&[
            output.num(n!("EnergyShieldRecoveryCap")),
            maxMoMPool * (1.0 - esBypass),
            (lifePool + maxManaUsable) / (1.0 - (1.0 - esBypass) * MoMEffect)
                - (lifePool + maxManaUsable),
        ]))
    } else {
        0.0
    };
    // L2361
    let manaUsed = m_floor(m_min(maxMoMPool - maxESUsable, maxManaUsable));
    // L2362
    (
        lifePool + manaUsed + maxESUsable,
        maxManaUsable,
        manaUsed,
        maxESUsable,
    )
}

// ---- Rust-only plumbing ----------------------------------------------------------------------------

/// `t_insert(breakdown[k], s)`: `table.insert` on a nil field raises, as in Lua.
fn t_insert(breakdown: &Breakdown, k: Name, s: String) {
    match breakdown.get(k) {
        Val::Table(t) => t.push(s),
        other => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// A Lua value as a `string.format` argument.
fn fa(v: &Val) -> pob_lua::format::FmtArg<'_> {
    use pob_lua::format::FmtArg;
    match v {
        Val::Nil => FmtArg::Nil,
        Val::Bool(b) => FmtArg::Bool(*b),
        Val::Num(x) => FmtArg::Num(*x),
        Val::Str(s) => FmtArg::Str(s),
        other => lua_error!(
            "bad argument to 'format' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// Lua `a .. b .. …` on strings and numbers (numbers via `tostring`); anything else raises.
fn concat(parts: &[Val]) -> Val {
    let mut s = String::new();
    for p in parts {
        match p {
            Val::Str(x) => s.push_str(x),
            Val::Num(x) => s.push_str(&tostring_num(*x)),
            other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
        }
    }
    Val::from(s)
}

/// `colorCodes[name]` (a nil code raises when concatenated, as in Lua).
fn colorCode(name: &str) -> Val {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| Val::from(*v))
        .unwrap_or(Val::Nil)
}

/// Lua `a < b` on numbers (other operand kinds raise "attempt to compare").
fn lt(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x < y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() < y.as_bytes(),
        _ => lua_error!("attempt to compare {} with {}", a.lua_type(), b.lua_type()),
    }
}

/// Lua `a >= b` (`b <= a`).
fn ge(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x >= y,
        _ => lua_error!("attempt to compare {} with {}", a.lua_type(), b.lua_type()),
    }
}

/// Lua `v ~= 0`.
fn ne0(v: &Val) -> bool {
    !v.raw_eq(&Val::Num(0.0))
}
