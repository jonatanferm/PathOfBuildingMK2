//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! defence segment DF-C (CalcDefence.lua 1211-1673): recovery, leech, degen.
//!
//! Owner: DF-C. Recovery rate modifiers, leech caps, regeneration and degeneration, energy
//! shield recharge, recoup, ward recharge, base damage reduction, movement speed, avoidance,
//! ailment avoidance and ailment duration/effect on self.
#![allow(non_snake_case)]
// literal Lua statement shapes (`x = x + y`, `a * b / c`)
#![allow(clippy::assign_op_pattern)]
// Lua locals initialised before their first assignment (`local regen = 0`)
#![allow(unused_assignments)]

use super::DefenceCtx;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::tag;

/// `local dmgTypeList` (CalcDefence.lua:28; module-level, DF-A's `helpers.rs` owns the marker).
const dmgTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// `isElemental[damageType]` (CalcDefence.lua:24; module-level, DF-A's `helpers.rs` owns the marker).
fn isElemental(damageType: &str) -> bool {
    matches!(damageType, "Fire" | "Cold" | "Lightning")
}

/// `x or d` where `x` is a mod store query that returns a Lua value (`Override`) and `d` a
/// number: the Lua value is kept as is (it is stored in `output`).
fn or_num(v: Value, d: f64) -> Val {
    if v.truthy() {
        Val::from(v)
    } else {
        Val::Num(d)
    }
}

/// `modDB:Tabulate("FLAG", nil, addFlag)[1].mod.source` (indexing a missing row raises).
fn first_flag_source(modDB: &ModDB, addFlag: Name) -> Option<AStr> {
    let rows = modDB.Tabulate(Some(ModType::FLAG), None, &[addFlag]);
    match rows.first() {
        Some(row) => row.r#mod.source.clone(),
        None => lua_error!("attempt to index field '?' (a nil value)"),
    }
}

/// `breakdown[k]` as a table for `t_insert` (a nil value raises, as `table.insert(nil, …)`).
fn bd_table(breakdown: &Breakdown, k: Name) -> Table {
    breakdown
        .table(k)
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'insert' (table expected, got nil)"))
}

/// A `modDB:Flag(…)` result as a Lua value: `ModDB:FlagInternal` returns `true` or nothing
/// (ModDB.lua:206-227), so an unset flag stored in `output` is nil, not `false`.
fn flag_val(b: bool) -> Val {
    if b {
        Val::Bool(true)
    } else {
        Val::Nil
    }
}

/// `spellSuppressionChance` as an arithmetic operand (nil raises).
fn spell_suppression(ctx: &DefenceCtx) -> f64 {
    ctx.spellSuppressionChance.unwrap_or_else(|| {
        lua_error!(
            "attempt to perform arithmetic on upvalue 'spellSuppressionChance' (a nil value)"
        )
    })
}

/// @pob Modules/CalcDefence.lua:1211-1673 h=d5d3073d
/// DF-C: CalcDefence.lua 1211-1673.
pub(crate) fn seg(ctx: &mut DefenceCtx) -> Flow {
    let actor = ctx.actor.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let data = data();

    // -- Recovery modifiers
    // L1212
    output.set(n!("LifeRecoveryRateMod"), 1.0);
    // L1213
    if !modDB.Flag(None, nm!["CannotRecoverLifeOutsideLeech"]) {
        // L1214
        let v = calcLib::r#mod(&modDB, None, nm!["LifeRecoveryRate"]);
        output.set(n!("LifeRecoveryRateMod"), v);
    }
    // L1216
    let v = calcLib::r#mod(&modDB, None, nm!["ManaRecoveryRate"]);
    output.set(n!("ManaRecoveryRateMod"), v);
    // L1217
    let v = calcLib::r#mod(&modDB, None, nm!["EnergyShieldRecoveryRate"]);
    output.set(n!("EnergyShieldRecoveryRateMod"), v);

    // -- Leech caps
    // L1220
    let v = output.num(n!("Life")) * calcLib::val(&modDB, n!("MaxLifeLeechInstance"), None) / 100.0;
    output.set(n!("MaxLifeLeechInstance"), v);
    // L1221
    let v = calcLib::val(&modDB, n!("MaxLifeLeechRate"), None);
    output.set(n!("MaxLifeLeechRatePercent"), v);
    // L1222
    if modDB.Flag(None, nm!["MaximumLifeLeechIsEqualToParent"]) {
        // L1223
        let parent = actor
            .parent()
            .unwrap_or_else(|| lua_error!("attempt to index field 'parent' (a nil value)"));
        let v = parent.output.get().get(n!("MaxLifeLeechRatePercent"));
        output.set(n!("MaxLifeLeechRatePercent"), v);
    } else if modDB.Flag(None, nm!["MaximumLifeLeechIsEqualToPartyMember"]) {
        // L1224
        // L1225
        let partyMembers =
            actor.partyMembers.get().upgrade().unwrap_or_else(|| {
                lua_error!("attempt to index field 'partyMembers' (a nil value)")
            });
        let v = partyMembers.output.get().get(n!("MaxLifeLeechRatePercent"));
        output.set(n!("MaxLifeLeechRatePercent"), v);
    }
    // L1227
    let v = output.num(n!("Life")) * output.num(n!("MaxLifeLeechRatePercent")) / 100.0;
    output.set(n!("MaxLifeLeechRate"), v);
    // L1228
    if let Some(breakdown) = &breakdown {
        // L1229
        breakdown.set(
            n!("MaxLifeLeechRate"),
            tbl![
                lfmt!("%d ^8(maximum life)", output.num(n!("Life"))),
                lfmt!(
                    "x %d%% ^8(percentage of life to maximum leech rate)",
                    output.num(n!("MaxLifeLeechRatePercent"))
                ),
                lfmt!("= %.1f", output.num(n!("MaxLifeLeechRate"))),
            ],
        );
    }
    // L1235
    let v = output.num(n!("EnergyShield"))
        * calcLib::val(&modDB, n!("MaxEnergyShieldLeechInstance"), None)
        / 100.0;
    output.set(n!("MaxEnergyShieldLeechInstance"), v);
    // L1236
    let v = output.num(n!("EnergyShield"))
        * calcLib::val(&modDB, n!("MaxEnergyShieldLeechRate"), None)
        / 100.0;
    output.set(n!("MaxEnergyShieldLeechRate"), v);
    // L1237
    if let Some(breakdown) = &breakdown {
        // L1238
        breakdown.set(
            n!("MaxEnergyShieldLeechRate"),
            tbl![
                lfmt!(
                    "%d ^8(maximum energy shield)",
                    output.num(n!("EnergyShield"))
                ),
                lfmt!(
                    "x %d%% ^8(percentage of energy shield to maximum leech rate)",
                    calcLib::val(&modDB, n!("MaxEnergyShieldLeechRate"), None)
                ),
                lfmt!("= %.1f", output.num(n!("MaxEnergyShieldLeechRate"))),
            ],
        );
    }
    // L1244
    let v = output.num(n!("Mana")) * calcLib::val(&modDB, n!("MaxManaLeechInstance"), None) / 100.0;
    output.set(n!("MaxManaLeechInstance"), v);
    // L1245
    let v = output.num(n!("Mana")) * calcLib::val(&modDB, n!("MaxManaLeechRate"), None) / 100.0;
    output.set(n!("MaxManaLeechRate"), v);
    // L1246
    if let Some(breakdown) = &breakdown {
        // L1247
        breakdown.set(
            n!("MaxManaLeechRate"),
            tbl![
                lfmt!("%d ^8(maximum mana)", output.num(n!("Mana"))),
                lfmt!(
                    "x %d%% ^8(percentage of mana to maximum leech rate)",
                    calcLib::val(&modDB, n!("MaxManaLeechRate"), None)
                ),
                lfmt!("= %.1f", output.num(n!("MaxManaLeechRate"))),
            ],
        );
    }

    // -- Regeneration
    // L1255
    let resources: [&str; 4] = ["Mana", "Life", "Energy Shield", "Rage"];
    // `resources[j]:gsub(" ", "")` for each entry (the first return value)
    let resources_nospace: [&str; 4] = ["Mana", "Life", "EnergyShield", "Rage"];
    // L1256
    for (i0, resourceName) in resources.iter().copied().enumerate() {
        let i = i0 + 1;
        // L1257
        let resource: Name = Name::from(resources_nospace[i0]);
        // L1258
        let pool = output.num0(resource);
        // L1259
        let mut baseRegen = 0.0;
        // L1260
        let mut inc = modDB.Sum(ModType::INC, None, &[cat!(resource, "Regen")]);
        // L1261
        let more = modDB.More(None, &[cat!(resource, "Regen")]);
        // L1262
        let mut regen = 0.0;
        // L1263
        let mut regenRate = 0.0;
        // L1264
        let recoveryRateMod = output.num_or(cat!(resource, "RecoveryRateMod"), 1.0);
        // L1265
        if modDB.Flag(None, &[cat!("No", resource, "Regen")])
            || modDB.Flag(None, &[cat!("CannotGain", resource)])
        {
            // L1266
            output.set(cat!(resource, "Regen"), 0.0);
        } else if resource == n!("Life") && modDB.Flag(None, nm!["ZealotsOath"]) {
            // L1267
            // L1268
            output.set(n!("LifeRegen"), 0.0);
            // L1269
            let lifeBase = modDB.Sum(ModType::BASE, None, nm!["LifeRegen"]);
            // L1270
            if lifeBase > 0.0 {
                // L1271
                modDB.NewMod(
                    n!("EnergyShieldRegen"),
                    ModType::BASE,
                    lifeBase,
                    "Zealot's Oath",
                    0,
                    0,
                    &[],
                );
            }
            // L1273
            let lifePercent = modDB.Sum(ModType::BASE, None, nm!["LifeRegenPercent"]);
            // L1274
            if lifePercent > 0.0 {
                // L1275
                modDB.NewMod(
                    n!("EnergyShieldRegenPercent"),
                    ModType::BASE,
                    lifePercent,
                    "Zealot's Oath",
                    0,
                    0,
                    &[],
                );
            }
        } else {
            // L1278
            if inc != 0.0 {
                // legacy chain breaker increase/decrease regen rate to different resource.
                // L1279
                for j in (i + 1)..=resources.len() {
                    // L1280
                    if modDB.Flag(
                        None,
                        &[cat!(resource, "RegenTo", resources_nospace[j - 1], "Regen")],
                    ) {
                        // L1281
                        modDB.NewMod(
                            cat!(resources_nospace[j - 1], "Regen"),
                            ModType::INC,
                            inc,
                            &format!("{resourceName} instead applies to {}", resources[j - 1]),
                            0,
                            0,
                            &[],
                        );
                        // L1282
                        inc = 0.0;
                    }
                }
            }
            // L1286
            if resource == n!("Life")
                && modDB.Sum(ModType::BASE, None, nm!["LifeRegenAppliesToEnergyShield"]) > 0.0
            {
                // L1287
                let conversion = m_min(
                    modDB.Sum(ModType::BASE, None, nm!["LifeRegenAppliesToEnergyShield"]),
                    100.0,
                ) / 100.0;
                // L1288
                let lifeBase = modDB.Sum(ModType::BASE, None, nm!["LifeRegen"]);
                // L1289
                let lifePercent = modDB.Sum(ModType::BASE, None, nm!["LifeRegenPercent"]);
                // L1290
                modDB.NewMod(
                    n!("EnergyShieldRegen"),
                    ModType::BASE,
                    floor(lifeBase * conversion, Some(2)),
                    "Life Regen to ES Regen",
                    0,
                    0,
                    &[],
                );
                // L1291
                modDB.NewMod(
                    n!("EnergyShieldRegenPercent"),
                    ModType::BASE,
                    floor(lifePercent * conversion, Some(2)),
                    "Life Regen to ES Regen",
                    0,
                    0,
                    &[],
                );
            }
            // L1293
            baseRegen = modDB.Sum(ModType::BASE, None, &[cat!(resource, "Regen")])
                + pool * modDB.Sum(ModType::BASE, None, &[cat!(resource, "RegenPercent")]) / 100.0;
            // L1294
            regen = baseRegen * (1.0 + inc / 100.0) * more;
            // L1295
            if regen != 0.0 {
                // Pious Path
                // L1296
                for j in (i + 1)..=resources.len() {
                    // L1297
                    if modDB.Flag(
                        None,
                        &[cat!(
                            resource,
                            "RegenerationRecovers",
                            resources_nospace[j - 1]
                        )],
                    ) {
                        // L1298
                        modDB.NewMod(
                            cat!(resources_nospace[j - 1], "Recovery"),
                            ModType::BASE,
                            regen,
                            &format!("{resourceName} Regeneration Recovers {}", resources[j - 1]),
                            0,
                            0,
                            &[],
                        );
                    }
                }
            }
            // L1302
            regenRate = round(regen * recoveryRateMod, Some(1));
            // L1303
            output.set(cat!(resource, "Regen"), regenRate);
        }
        // L1305
        output.set(cat!(resource, "RegenInc"), inc);
        // L1306
        let mut baseDegen = modDB.Sum(ModType::BASE, None, &[cat!(resource, "Degen")])
            + (pool * modDB.Sum(ModType::BASE, None, &[cat!(resource, "DegenPercent")]) / 100.0);
        // L1307
        let tinctureDegenPercent = modDB.Sum(
            ModType::BASE,
            None,
            &[cat!(resource, "DegenPercentTincture")],
        );
        // L1308
        baseDegen = baseDegen + m_max(pool * tinctureDegenPercent / 100.0, tinctureDegenPercent); // tincture minimum 1 degen per stack
                                                                                                  // L1309
        let degenRate = if baseDegen > 0.0 {
            baseDegen * calcLib::r#mod(&modDB, None, &[cat!(resource, "Degen")])
        } else {
            0.0
        };
        // L1310
        output.set(cat!(resource, "Degen"), degenRate);
        // L1311
        let recoveryRate =
            modDB.Sum(ModType::BASE, None, &[cat!(resource, "Recovery")]) * recoveryRateMod;
        // L1312
        output.set(cat!(resource, "Recovery"), recoveryRate);
        // L1313
        let v = (if modDB.Flag(None, &[cat!("UnaffectedBy", resource, "Regen")]) {
            0.0
        } else {
            regenRate
        }) - degenRate
            + recoveryRate;
        output.set(cat!(resource, "RegenRecovery"), v);
        // L1314
        if output.num(cat!(resource, "RegenRecovery")) > 0.0 {
            // L1315
            modDB.NewMod(
                cat!("Condition:CanGain", resource),
                ModType::FLAG,
                true,
                &format!("{resourceName}Regen"),
                0,
                0,
                &[],
            );
        }
        // L1317
        let v = if pool > 0.0 {
            round(
                output.num(cat!(resource, "RegenRecovery")) / pool * 100.0,
                Some(1),
            )
        } else {
            0.0
        };
        output.set(cat!(resource, "RegenPercent"), v);
        // L1318
        if let Some(breakdown) = &breakdown {
            // L1319
            breakdown.set(cat!(resource, "RegenRecovery"), Table::new());
            // L1320
            let chain = tbl![
                tbl!["%.2f ^8(increased/reduced)", 1.0 + inc / 100.0],
                tbl!["%.2f ^8(more/less)", more],
                tbl!["%.2f ^8(recovery rate modifier)", recoveryRateMod];
                label = format!("{resourceName} Regeneration:"),
                base = tbl!["%.1f ^8(base)", baseRegen],
                total = lfmt!("= %.1f ^8per second", regenRate),
            ];
            breakdown.multiChain(
                &bd_table(breakdown, cat!(resource, "RegenRecovery")),
                &chain,
            );
            // L1328
            if modDB.Flag(None, &[cat!("UnaffectedBy", resource, "Regen")]) {
                // L1329
                bd_table(breakdown, cat!(resource, "RegenRecovery"))
                    .push(format!("Unaffected by {resourceName} Regen"));
            }
            // L1331
            if degenRate != 0.0 {
                // L1332
                bd_table(breakdown, cat!(resource, "RegenRecovery"))
                    .push(lfmt!("- %.1f ^8(degen)", degenRate));
                // L1333
                let r = (if modDB.Flag(None, &[cat!("UnaffectedBy", resource, "Regen")]) {
                    0.0
                } else {
                    regenRate
                }) - degenRate;
                bd_table(breakdown, cat!(resource, "RegenRecovery"))
                    .push(lfmt!("= %.1f ^8per second", r));
            }
            // L1335
            if recoveryRate != 0.0 {
                // L1336
                bd_table(breakdown, cat!(resource, "RegenRecovery"))
                    .push(lfmt!("+ %.1f ^8(recovery)", recoveryRate));
                // L1337
                bd_table(breakdown, cat!(resource, "RegenRecovery")).push(lfmt!(
                    "= %.1f ^8per second",
                    output.num(cat!(resource, "RegenRecovery"))
                ));
            }
        }
    }

    // -- Energy Shield Recharge
    // L1343
    // `Flag(…) and not Flag(…)`: nil when the first flag is unset
    let v = if modDB.Flag(None, nm!["EnergyShieldRechargeAppliesToLife"]) {
        Val::Bool(!modDB.Flag(None, nm!["CannotRecoverLifeOutsideLeech"]))
    } else {
        flag_val(false)
    };
    output.set(n!("EnergyShieldRechargeAppliesToLife"), v);
    // L1344
    let v = !(modDB.Flag(None, nm!["NoEnergyShieldRecharge"])
        || modDB.Flag(None, nm!["CannotGainEnergyShield"])
        || output.truthy(n!("EnergyShieldRechargeAppliesToLife")));
    output.set(n!("EnergyShieldRechargeAppliesToEnergyShield"), v);

    // L1346
    if output.truthy(n!("EnergyShieldRechargeAppliesToLife"))
        || output.truthy(n!("EnergyShieldRechargeAppliesToEnergyShield"))
    {
        // L1347
        let inc = modDB.Sum(ModType::INC, None, nm!["EnergyShieldRecharge"]);
        // L1348
        let more = modDB.More(None, nm!["EnergyShieldRecharge"]);
        // L1349
        let base = or_num(
            modDB.Override(None, nm!["EnergyShieldRecharge"]),
            data.misc.EnergyShieldRechargeBase,
        );
        // L1350
        if output.truthy(n!("EnergyShieldRechargeAppliesToLife")) {
            // L1351
            let recharge = output.num(n!("Life")) * base.num() * (1.0 + inc / 100.0) * more;
            // L1352
            let v = round(recharge * output.num(n!("LifeRecoveryRateMod")), None);
            output.set(n!("LifeRecharge"), v);
            // L1353
            if let Some(breakdown) = &breakdown {
                // L1354
                breakdown.set(n!("LifeRecharge"), Table::new());
                // L1355
                let chain = tbl![
                    tbl!["%.2f ^8(increased/reduced)", 1.0 + inc / 100.0],
                    tbl!["%.2f ^8(more/less)", more];
                    label = "Recharge rate:",
                    base = tbl![
                        "%.1f ^8(%d%% per second)",
                        output.num(n!("Life")) * base.num(),
                        base.num() * 100.0
                    ],
                    total = lfmt!("= %.1f ^8per second", recharge),
                ];
                breakdown.multiChain(&bd_table(breakdown, n!("LifeRecharge")), &chain);
                // L1362
                let chain = tbl![
                    tbl!["%.2f ^8(recovery rate modifier)", output.get(n!("LifeRecoveryRateMod"))];
                    label = "Effective Recharge rate:",
                    base = tbl!["%.1f", recharge],
                    total = lfmt!("= %.1f ^8per second", output.num(n!("LifeRecharge"))),
                ];
                breakdown.multiChain(&bd_table(breakdown, n!("LifeRecharge")), &chain);
            }
        } else {
            // L1370
            let recharge = output.num(n!("EnergyShield")) * base.num() * (1.0 + inc / 100.0) * more;
            // L1371
            let v = round(
                recharge * output.num(n!("EnergyShieldRecoveryRateMod")),
                None,
            );
            output.set(n!("EnergyShieldRecharge"), v);
            // L1372
            if let Some(breakdown) = &breakdown {
                // L1373
                breakdown.set(n!("EnergyShieldRecharge"), Table::new());
                // L1374
                let chain = tbl![
                    tbl!["%.2f ^8(increased/reduced)", 1.0 + inc / 100.0],
                    tbl!["%.2f ^8(more/less)", more];
                    label = "Recharge rate:",
                    base = tbl![
                        "%.1f ^8(%d%% per second)",
                        output.num(n!("EnergyShield")) * base.num(),
                        base.num() * 100.0
                    ],
                    total = lfmt!("= %.1f ^8per second", recharge),
                ];
                breakdown.multiChain(&bd_table(breakdown, n!("EnergyShieldRecharge")), &chain);
                // L1381
                let chain = tbl![
                    tbl![
                        "%.2f ^8(recovery rate modifier)",
                        output.get(n!("EnergyShieldRecoveryRateMod"))
                    ];
                    label = "Effective Recharge rate:",
                    base = tbl!["%.1f", recharge],
                    total = lfmt!("= %.1f ^8per second", output.num(n!("EnergyShieldRecharge"))),
                ];
                breakdown.multiChain(&bd_table(breakdown, n!("EnergyShieldRecharge")), &chain);
            }
        }
        // L1389
        let v = data.misc.EnergyShieldRechargeDelay
            / (1.0 + modDB.Sum(ModType::INC, None, nm!["EnergyShieldRechargeFaster"]) / 100.0);
        output.set(n!("EnergyShieldRechargeDelay"), v);
        // L1390
        if let Some(breakdown) = &breakdown {
            // L1391
            if output.num(n!("EnergyShieldRechargeDelay")) != data.misc.EnergyShieldRechargeDelay {
                // L1392
                breakdown.set(
                    n!("EnergyShieldRechargeDelay"),
                    tbl![
                        lfmt!("%.2fs ^8(base)", data.misc.EnergyShieldRechargeDelay),
                        lfmt!(
                            "/ %.2f ^8(faster start)",
                            1.0 + modDB.Sum(ModType::INC, None, nm!["EnergyShieldRechargeFaster"])
                                / 100.0
                        ),
                        lfmt!("= %.2fs", output.num(n!("EnergyShieldRechargeDelay"))),
                    ],
                );
            }
        }
    } else {
        // L1400
        output.set(n!("EnergyShieldRecharge"), 0.0);
    }

    // -- recoup
    // L1404
    {
        // L1405
        output.set(n!("anyRecoup"), 0.0);
        // L1406
        let recoupTypeList: [&str; 3] = ["Life", "Mana", "EnergyShield"];
        // L1407
        let recoupInc = 1.0 + modDB.Sum(ModType::INC, None, nm!["RecoupRecoveryAmount"]) / 100.0;
        // L1408
        let recoupMore = modDB.More(None, nm!["RecoupRecoveryAmount"]);
        // `(Flag("3Second"..recoupType.."Recoup") or Flag("3SecondRecoup")) and 3 or 4`
        let recoupSeconds = |recoupType: &str| -> f64 {
            if modDB.Flag(None, &[cat!("3Second", recoupType, "Recoup")])
                || modDB.Flag(None, nm!["3SecondRecoup"])
            {
                3.0
            } else {
                4.0
            }
        };
        // L1409
        for recoupType in recoupTypeList {
            // L1410
            for recoupType2 in recoupTypeList {
                // L1411
                if recoupType != recoupType2 {
                    // L1412 `s_format("Add%sRecoupTo%sRecoup", …)` of two plain strings
                    let addFlag = cat!("Add", recoupType, "RecoupTo", recoupType2, "Recoup");
                    // L1413
                    if modDB.Flag(None, &[addFlag]) {
                        // L1414
                        let baseRecoup =
                            modDB.Sum(ModType::BASE, None, &[cat!(recoupType, "Recoup")]);
                        // -- inherit source from original flag
                        // L1416
                        let flagMod_source = first_flag_source(&modDB, addFlag);
                        // L1417
                        modDB.NewMod(
                            cat!(recoupType2, "Recoup"),
                            ModType::BASE,
                            baseRecoup,
                            flagMod_source.as_deref(),
                            0,
                            0,
                            &[],
                        );
                    }
                }
            }
        }
        // L1422
        for recoupType in recoupTypeList {
            // L1423
            if recoupType == "Life" && modDB.Flag(None, nm!["EnergyShieldRecoupInsteadOfLife"]) {
                // L1424
                output.set(n!("LifeRecoup"), 0.0);
                // L1425
                let lifeRecoup = modDB.Sum(ModType::BASE, None, nm!["LifeRecoup"]);
                // L1426
                modDB.NewMod(
                    n!("EnergyShieldRecoup"),
                    ModType::BASE,
                    lifeRecoup,
                    "Life Recoup Conversion",
                    0,
                    0,
                    &[],
                );
            } else {
                // L1428
                let baseRecoup = modDB.Sum(ModType::BASE, None, &[cat!(recoupType, "Recoup")]);
                // L1429
                let v = baseRecoup
                    * output.num(cat!(recoupType, "RecoveryRateMod"))
                    * recoupInc
                    * recoupMore;
                output.set(cat!(recoupType, "Recoup"), v);
                // L1430
                let v = output.num(n!("anyRecoup")) + output.num(cat!(recoupType, "Recoup"));
                output.set(n!("anyRecoup"), v);
                // L1431
                if let Some(breakdown) = &breakdown {
                    // L1432
                    breakdown.set(
                        cat!(recoupType, "Recoup"),
                        tbl![lfmt!("%d%% ^8(base)", baseRecoup)],
                    );
                    // L1433
                    if recoupInc != 1.0 {
                        // L1434
                        bd_table(breakdown, cat!(recoupType, "Recoup"))
                            .push(lfmt!("x %.2f (increased/reduced)", recoupInc));
                    }
                    // L1436
                    if recoupMore != 1.0 {
                        // L1437
                        bd_table(breakdown, cat!(recoupType, "Recoup"))
                            .push(lfmt!("x %.2f (more/less)", recoupMore));
                    }
                    // L1439
                    if !output
                        .get(cat!(recoupType, "RecoveryRateMod"))
                        .raw_eq(&Val::Num(1.0))
                    {
                        // L1440
                        bd_table(breakdown, cat!(recoupType, "Recoup")).push(lfmt!(
                            "* %.2f ^8(recovery rate modifier)",
                            output.num(cat!(recoupType, "RecoveryRateMod"))
                        ));
                        // L1441
                        bd_table(breakdown, cat!(recoupType, "Recoup")).push(lfmt!(
                            "= %.1f%% over %d seconds",
                            output.num(cat!(recoupType, "Recoup")),
                            recoupSeconds(recoupType)
                        ));
                    } else {
                        // L1443
                        bd_table(breakdown, cat!(recoupType, "Recoup")).push(lfmt!(
                            "%.1f%% over %d seconds",
                            output.num(cat!(recoupType, "Recoup")),
                            recoupSeconds(recoupType)
                        ));
                    }
                }
            }
        }

        // L1449
        if modDB.Flag(None, nm!["UsePowerCharges"])
            && modDB.Flag(None, nm!["PowerChargesConvertToAbsorptionCharges"])
        {
            // L1450
            let ElementalEnergyShieldRecoupPerAbsorptionCharges = modDB.Sum(
                ModType::BASE,
                None,
                nm!["PerAbsorptionElementalEnergyShieldRecoup"],
            );
            // L1451
            modDB.NewMod(
                n!("ColdEnergyShieldRecoup"),
                ModType::BASE,
                ElementalEnergyShieldRecoupPerAbsorptionCharges,
                "Absorption Charges",
                0,
                0,
                &[tag!({ type = Multiplier, var = "AbsorptionCharge" })],
            );
            // L1452
            modDB.NewMod(
                n!("FireEnergyShieldRecoup"),
                ModType::BASE,
                ElementalEnergyShieldRecoupPerAbsorptionCharges,
                "Absorption Charges",
                0,
                0,
                &[tag!({ type = Multiplier, var = "AbsorptionCharge" })],
            );
            // L1453
            modDB.NewMod(
                n!("LightningEnergyShieldRecoup"),
                ModType::BASE,
                ElementalEnergyShieldRecoupPerAbsorptionCharges,
                "Absorption Charges",
                0,
                0,
                &[tag!({ type = Multiplier, var = "AbsorptionCharge" })],
            );
        }

        // L1456
        for recoupType in recoupTypeList {
            // L1457
            for damageType in dmgTypeList {
                // L1458
                let recoup = modDB.Sum(
                    ModType::BASE,
                    None,
                    &[cat!(damageType, recoupType, "Recoup")],
                );
                // L1459
                let recoupName = cat!(damageType, recoupType, "Recoup");
                // L1460
                if recoupType == "Life" && modDB.Flag(None, nm!["EnergyShieldRecoupInsteadOfLife"])
                {
                    // L1461
                    output.set(recoupName, 0.0);
                    // L1462
                    modDB.NewMod(
                        cat!(damageType, "EnergyShieldRecoup"),
                        ModType::BASE,
                        recoup,
                        "Life Recoup Conversion",
                        0,
                        0,
                        &[],
                    );
                } else {
                    // L1464
                    let v = recoup
                        * output.num(cat!(recoupType, "RecoveryRateMod"))
                        * recoupInc
                        * recoupMore;
                    output.set(recoupName, v);
                    // L1465
                    let v = output.num(n!("anyRecoup")) + output.num(recoupName);
                    output.set(n!("anyRecoup"), v);
                    // L1466
                    if let Some(breakdown) = &breakdown {
                        // L1467
                        breakdown.set(recoupName, tbl![lfmt!("%d%% ^8(base)", recoup)]);
                        // L1468
                        if recoupInc != 1.0 {
                            // L1469
                            bd_table(breakdown, recoupName)
                                .push(lfmt!("x %.2f (increased/reduced)", recoupInc));
                        }
                        // L1471
                        if recoupMore != 1.0 {
                            // L1472
                            bd_table(breakdown, recoupName)
                                .push(lfmt!("x %.2f (more/less)", recoupMore));
                        }
                        // L1474
                        if !output
                            .get(cat!(recoupType, "RecoveryRateMod"))
                            .raw_eq(&Val::Num(1.0))
                        {
                            // L1475
                            bd_table(breakdown, recoupName).push(lfmt!(
                                "* %.2f ^8(recovery rate modifier)",
                                output.num(cat!(recoupType, "RecoveryRateMod"))
                            ));
                        }
                        // L1477
                        bd_table(breakdown, recoupName).push(lfmt!(
                            "= %.1f%% over %d seconds",
                            output.num(recoupName),
                            recoupSeconds(recoupType)
                        ));
                    }
                }
                // L1480
                for recoupType2 in recoupTypeList {
                    // L1481
                    if recoupType != recoupType2 {
                        // L1482 `s_format("Add%sRecoupTo%sRecoup", …)` of two plain strings
                        let addFlag = cat!("Add", recoupType, "RecoupTo", recoupType2, "Recoup");
                        // L1483
                        if modDB.Flag(None, &[addFlag]) {
                            // L1484
                            let flagMod_source = first_flag_source(&modDB, addFlag);
                            // L1485
                            modDB.ReplaceMod(
                                cat!(damageType, recoupType2, "Recoup"),
                                ModType::BASE,
                                recoup,
                                flagMod_source.as_deref(),
                                0,
                                0,
                                &[],
                            );
                        }
                    }
                }
            }
        }
        // -- pseudo recoup (eg %physical damage prevented from hits regenerated)
        // L1492
        for resource in recoupTypeList {
            // L1493
            if !modDB.Flag(None, &[cat!("No", resource, "Regen")])
                && !modDB.Flag(None, &[cat!("CannotGain", resource)])
            {
                // L1494
                let PhysicalDamageMitigatedPseudoRecoup = modDB.Sum(
                    ModType::BASE,
                    None,
                    &[cat!("PhysicalDamageMitigated", resource, "PseudoRecoup")],
                );
                // L1495
                if PhysicalDamageMitigatedPseudoRecoup > 0.0 {
                    let durKey = cat!("PhysicalDamageMitigated", resource, "PseudoRecoupDuration");
                    let key = cat!("PhysicalDamageMitigated", resource, "PseudoRecoup");
                    // L1496
                    let v = modDB.Sum(ModType::BASE, None, &[durKey]);
                    output.set(durKey, v);
                    // L1497
                    if output.get(durKey).raw_eq(&Val::Num(0.0)) {
                        // L1498
                        output.set(durKey, 4.0);
                    }
                    // L1500
                    let inc = modDB.Sum(ModType::INC, None, &[cat!(resource, "Regen")]);
                    // L1501
                    let more = modDB.More(None, &[cat!(resource, "Regen")]);
                    // L1502
                    let v = PhysicalDamageMitigatedPseudoRecoup
                        * (1.0 + inc / 100.0)
                        * more
                        * output.num(cat!(resource, "RecoveryRateMod"));
                    output.set(key, v);
                    // L1503
                    let v = output.num(n!("anyRecoup")) + output.num(key);
                    output.set(n!("anyRecoup"), v);
                    // L1504
                    if let Some(breakdown) = &breakdown {
                        // L1505
                        breakdown.set(key, Table::new());
                        // L1506
                        if !output
                            .get(cat!(resource, "RecoveryRateMod"))
                            .raw_eq(&Val::Num(1.0))
                            || inc != 0.0
                            || more != 1.0
                        {
                            // L1507
                            bd_table(breakdown, key)
                                .push(lfmt!("%d%% ^8(base)", PhysicalDamageMitigatedPseudoRecoup));
                            // L1508
                            if inc != 0.0 || more != 1.0 {
                                // L1509
                                bd_table(breakdown, key).push(lfmt!(
                                    "* %.2f ^8(regeneration rate modifier)",
                                    (1.0 + inc / 100.0) * more
                                ));
                            }
                            // L1511
                            if !output
                                .get(cat!(resource, "RecoveryRateMod"))
                                .raw_eq(&Val::Num(1.0))
                            {
                                // L1512
                                bd_table(breakdown, key).push(lfmt!(
                                    "* %.2f ^8(recovery rate modifier)",
                                    output.num(cat!(resource, "RecoveryRateMod"))
                                ));
                            }
                        }
                        // L1515
                        bd_table(breakdown, key).push(lfmt!(
                            "= %.1f%% over %d seconds",
                            output.num(key),
                            output.num(durKey)
                        ));
                    }
                }
            }
        }
    }

    // -- Ward recharge
    // L1523
    let v = data.misc.WardRechargeDelay
        / (1.0 + modDB.Sum(ModType::INC, None, nm!["WardRechargeFaster"]) / 100.0);
    output.set(n!("WardRechargeDelay"), v);
    // L1524
    if let Some(breakdown) = &breakdown {
        // L1525
        if output.num(n!("WardRechargeDelay")) != data.misc.WardRechargeDelay {
            // L1526
            breakdown.set(
                n!("WardRechargeDelay"),
                tbl![
                    lfmt!("%.2fs ^8(base)", data.misc.WardRechargeDelay),
                    lfmt!(
                        "/ %.2f ^8(faster start)",
                        1.0 + modDB.Sum(ModType::INC, None, nm!["WardRechargeFaster"]) / 100.0
                    ),
                    lfmt!("= %.2fs", output.num(n!("WardRechargeDelay"))),
                ],
            );
        }
    }

    // -- Damage Reduction
    // L1535
    let v = or_num(
        modDB.Override(None, nm!["DamageReductionMax"]),
        data.misc.DamageReductionCap,
    );
    output.set(n!("DamageReductionMax"), v);
    // L1536
    modDB.NewMod(
        n!("ArmourAppliesToPhysicalDamageTaken"),
        ModType::BASE,
        100.0,
        None,
        0,
        0,
        &[],
    );
    // L1537
    for damageType in dmgTypeList {
        // L1538
        let o = modDB.Override(None, &[cat!(damageType, "DamageReduction")]);
        let v = if o.truthy() {
            Val::from(o)
        } else {
            let names = names([
                Some(cat!(damageType, "DamageReduction")),
                isElemental(damageType).then(|| n!("ElementalDamageReduction")),
            ]);
            Val::Num(m_min(
                m_max(0.0, modDB.Sum(ModType::BASE, None, &names)),
                output.num(n!("DamageReductionMax")),
            ))
        };
        output.set(cat!("Base", damageType, "DamageReduction"), v);
        // L1539
        let o = modDB.Override(None, &[cat!(damageType, "DamageReduction")]);
        let v = if o.truthy() {
            Val::from(o)
        } else {
            Val::Num(m_min(
                m_max(
                    0.0,
                    output.num(cat!("Base", damageType, "DamageReduction"))
                        + modDB.Sum(
                            ModType::BASE,
                            None,
                            &[cat!(damageType, "DamageReductionWhenHit")],
                        ),
                ),
                output.num(n!("DamageReductionMax")),
            ))
        };
        output.set(cat!("Base", damageType, "DamageReductionWhenHit"), v);
    }

    // -- Miscellaneous: move speed, avoidance
    // L1543
    let o = modDB.Override(None, nm!["MovementSpeed"]);
    let v = if o.truthy() {
        Val::from(o)
    } else if modDB.Flag(None, nm!["MovementSpeedEqualHighestLinkedPlayers"]) {
        // `flag and actor.partyMembers.output.MovementSpeedMod or calcLib.mod(…)`
        let partyMembers =
            actor.partyMembers.get().upgrade().unwrap_or_else(|| {
                lua_error!("attempt to index field 'partyMembers' (a nil value)")
            });
        let b = partyMembers.output.get().get(n!("MovementSpeedMod"));
        if b.truthy() {
            b
        } else {
            Val::Num(calcLib::r#mod(&modDB, None, nm!["MovementSpeed"]))
        }
    } else {
        Val::Num(calcLib::r#mod(&modDB, None, nm!["MovementSpeed"]))
    };
    output.set(n!("MovementSpeedMod"), v);
    // L1544
    if modDB.Flag(None, nm!["MovementSpeedCannotBeBelowBase"]) {
        // L1545
        let v = m_max(output.num(n!("MovementSpeedMod")), 1.0);
        output.set(n!("MovementSpeedMod"), v);
    }
    // L1547
    let v = output.num(n!("MovementSpeedMod")) * output.num(n!("ActionSpeedMod"));
    output.set(n!("EffectiveMovementSpeedMod"), v);
    // L1548
    if let Some(breakdown) = &breakdown {
        // L1549
        breakdown.set(n!("EffectiveMovementSpeedMod"), Table::new());
        // L1550
        let chain = tbl![
            tbl!["%.2f ^8(movement speed modifier)", output.get(n!("MovementSpeedMod"))],
            tbl!["%.2f ^8(action speed modifier)", output.get(n!("ActionSpeedMod"))];
            total = lfmt!(
                "= %.2f ^8(effective movement speed modifier)",
                output.num(n!("EffectiveMovementSpeedMod"))
            ),
        ];
        breakdown.multiChain(
            &bd_table(breakdown, n!("EffectiveMovementSpeedMod")),
            &chain,
        );
    }

    // L1557
    if enemyDB.Flag(None, nm!["Blind"]) {
        // L1558
        let v = calcLib::r#mod(&enemyDB, None, nm!["BlindEffect", "BuffEffectOnSelf"]) * 100.0;
        output.set(n!("BlindEffectMod"), v);
    }

    // -- recovery on block, needs to be after primary defences
    // L1562
    output.set(n!("LifeOnBlock"), 0.0);
    // L1563
    output.set(n!("LifeOnSuppress"), 0.0);
    // L1564
    if !modDB.Flag(None, nm!["CannotRecoverLifeOutsideLeech"]) {
        // L1565
        let v = modDB.Sum(ModType::BASE, None, nm!["LifeOnBlock"]);
        output.set(n!("LifeOnBlock"), v);
        // L1566
        let v = modDB.Sum(ModType::BASE, None, nm!["LifeOnSuppress"]);
        output.set(n!("LifeOnSuppress"), v);
    }

    // L1569
    let v = modDB.Sum(ModType::BASE, None, nm!["ManaOnBlock"]);
    output.set(n!("ManaOnBlock"), v);

    // L1571
    let v = modDB.Sum(ModType::BASE, None, nm!["EnergyShieldOnBlock"]);
    output.set(n!("EnergyShieldOnBlock"), v);
    // L1572
    let v = modDB.Sum(ModType::BASE, None, nm!["EnergyShieldOnSpellBlock"]);
    output.set(n!("EnergyShieldOnSpellBlock"), v);
    // L1573
    let v = modDB.Sum(ModType::BASE, None, nm!["EnergyShieldOnSuppress"]);
    output.set(n!("EnergyShieldOnSuppress"), v);

    // -- damage avoidances
    // L1576
    output.set(n!("specificTypeAvoidance"), false);
    // L1577
    for damageType in dmgTypeList {
        // L1578
        let v = m_min(
            modDB.Sum(
                ModType::BASE,
                None,
                &[cat!("Avoid", damageType, "DamageChance")],
            ),
            data.misc.AvoidChanceCap,
        );
        output.set(cat!("Avoid", damageType, "DamageChance"), v);
        // L1579
        if output.num(cat!("Avoid", damageType, "DamageChance")) > 0.0 {
            // L1580
            output.set(n!("specificTypeAvoidance"), true);
        }
    }
    // L1583
    let v = m_min(
        modDB.Sum(ModType::BASE, None, nm!["AvoidProjectilesChance"]),
        data.misc.AvoidChanceCap,
    );
    output.set(n!("AvoidProjectilesChance"), v);
    // -- hit avoidance
    // L1585
    let v = m_min(
        modDB.Sum(ModType::BASE, None, nm!["AvoidAllDamageFromHitsChance"]),
        data.misc.AvoidChanceCap,
    );
    output.set(n!("AvoidAllDamageFromHitsChance"), v);
    // -- other avoidances etc
    // L1587
    let v = if modDB.Flag(None, nm!["BlindImmune"]) {
        100.0
    } else {
        m_min(modDB.Sum(ModType::BASE, None, nm!["AvoidBlind"]), 100.0)
    };
    output.set(n!("BlindAvoidChance"), v);
    // L1588
    let v = if modDB.Flag(None, nm!["ImpaleImmune"]) {
        100.0
    } else {
        m_min(modDB.Sum(ModType::BASE, None, nm!["AvoidImpale"]), 100.0)
    };
    output.set(n!("ImpaleAvoidChance"), v);
    // -- Status effect / ailment immunities
    // L1590
    let v = flag_val(modDB.Flag(None, nm!["CorruptedBloodImmune"]));
    output.set(n!("CorruptedBloodImmunity"), v);
    // L1591
    let v = flag_val(modDB.Flag(None, nm!["MaimImmune"]));
    output.set(n!("MaimImmunity"), v);
    // L1592
    let v = flag_val(modDB.Flag(None, nm!["HinderImmune"]));
    output.set(n!("HinderImmunity"), v);
    // L1593
    let v = flag_val(modDB.Flag(None, nm!["KnockbackImmune"]));
    output.set(n!("KnockbackImmunity"), v);

    // L1595
    if modDB.Flag(None, nm!["SpellSuppressionAppliesToAilmentAvoidance"]) {
        // L1596 (`Sum(…) or 0`: Sum never returns nil)
        let spellSuppressionToAilmentPercent = modDB.Sum(
            ModType::BASE,
            None,
            nm!["SpellSuppressionAppliesToAilmentAvoidancePercent"],
        ) / 100.0;
        // -- Ancestral Vision
        // L1598
        modDB.NewMod(
            n!("AvoidElementalAilments"),
            ModType::BASE,
            m_floor(spellSuppressionToAilmentPercent * spell_suppression(ctx)),
            "Ancestral Vision",
            0,
            0,
            &[],
        );
    }
    // L1600
    if modDB.Flag(
        None,
        nm!["SpellSuppressionAppliesToChanceToDefendWithArmour"],
    ) {
        // L1601
        let spellSuppressionAppliesToChanceToDefendWithArmourPercent = modDB
            .Max(
                None,
                nm!["SpellSuppressionAppliesToChanceToDefendWithArmourPercent"],
            )
            .unwrap_or(0.0);
        // L1602
        let spellSuppressionAppliesToChanceToDefendWithArmourPercentArmour = modDB
            .Max(
                None,
                nm!["SpellSuppressionAppliesToChanceToDefendWithArmourPercentArmour"],
            )
            .unwrap_or(0.0);
        // -- Foulborn Ancestral Vision
        // L1604 (`tonumber(n)` of a number is `n`)
        modDB.NewMod(
            n!("ArmourDefense"),
            ModType::MAX,
            spellSuppressionAppliesToChanceToDefendWithArmourPercentArmour - 100.0,
            "Chance to Defend from Spell Suppression: Max Calc",
            0,
            0,
            &[tag!({ type = Condition, var = "ArmourMax" })],
        );
        // L1605
        modDB.NewMod(
            n!("ArmourDefense"),
            ModType::MAX,
            m_min(
                (spellSuppressionAppliesToChanceToDefendWithArmourPercent * spell_suppression(ctx))
                    / 100.0,
                1.0,
            ) * (spellSuppressionAppliesToChanceToDefendWithArmourPercentArmour - 100.0),
            "Chance to Defend from Spell Suppression: Average Calc",
            0,
            0,
            &[tag!({ type = Condition, var = "ArmourAvg" })],
        );
        // L1606 `modSource or "…"`: `modSource` is an undeclared global
        // LUA-GLOBAL-NIL
        modDB.NewMod(
            n!("ArmourDefense"),
            ModType::MAX,
            m_min(
                m_floor(
                    (spellSuppressionAppliesToChanceToDefendWithArmourPercent
                        * spell_suppression(ctx))
                        / 100.0,
                ),
                1.0,
            ) * (spellSuppressionAppliesToChanceToDefendWithArmourPercentArmour - 100.0),
            "Chance to Defend from Spell Suppression: Min Calc",
            0,
            0,
            &[
                tag!({ type = Condition, var = "ArmourMax", neg = true }),
                tag!({ type = Condition, var = "ArmourAvg", neg = true }),
            ],
        );
    }
    // L1608
    let v = modDB.Max(None, nm!["ArmourDefense"]).unwrap_or(0.0) / 100.0;
    output.set(n!("ArmourDefense"), v);
    // L1609
    let v = if output.num(n!("ArmourDefense")) > 0.0 {
        Val::Num((1.0 + output.num(n!("ArmourDefense"))) * 100.0)
    } else {
        Val::Nil
    };
    output.set(n!("RawArmourDefense"), v);

    // -- This is only used for breakdown purposes
    // L1612
    if modDB.Flag(None, nm!["ShockAvoidAppliesToElementalAilments"]) {
        // L1613
        let base = modDB.Sum(ModType::BASE, None, nm!["AvoidShock"]);
        // L1614
        if base != 0.0 {
            // L1615
            modDB.NewMod(
                n!("AvoidShockAppliesToElementalAilments"),
                ModType::BASE,
                base,
                "Stormshroud",
                0,
                0,
                &[],
            );
        }
    }

    // L1619
    for &ailment in &data.nonElementalAilmentTypeList {
        // L1620
        let v = if modDB.Flag(None, &[cat!(ailment, "Immune")]) {
            100.0
        } else {
            m_floor(m_min(
                modDB.Sum(
                    ModType::BASE,
                    None,
                    &[cat!("Avoid", ailment), n!("AvoidAilments")],
                ),
                100.0,
            ))
        };
        output.set(cat!(ailment, "AvoidChance"), v);
    }
    // L1622
    for &ailment in &data.elementalAilmentTypeList {
        // L1623
        let shockAvoidAppliesToAll =
            modDB.Flag(None, nm!["ShockAvoidAppliesToElementalAilments"]) && ailment != n!("Shock");
        // L1624
        let v = if modDB.Flag(
            None,
            &[cat!(ailment, "Immune"), n!("ElementalAilmentImmune")],
        ) {
            100.0
        } else {
            m_floor(m_min(
                modDB.Sum(
                    ModType::BASE,
                    None,
                    &[
                        cat!("Avoid", ailment),
                        n!("AvoidAilments"),
                        n!("AvoidElementalAilments"),
                    ],
                ) + (if shockAvoidAppliesToAll {
                    modDB.Sum(ModType::BASE, None, nm!["AvoidShock"])
                } else {
                    0.0
                }),
                100.0,
            ))
        };
        output.set(cat!(ailment, "AvoidChance"), v);
    }

    // L1627
    let v = if modDB.Flag(None, nm!["CurseImmune"]) {
        100.0
    } else {
        m_min(modDB.Sum(ModType::BASE, None, nm!["AvoidCurse"]), 100.0)
    };
    output.set(n!("CurseAvoidChance"), v);
    // L1628
    let v = if modDB.Flag(None, nm!["SilenceImmune"]) {
        Val::Num(100.0)
    } else {
        output.get(n!("CurseAvoidChance"))
    };
    output.set(n!("SilenceAvoidChance"), v);
    // L1629
    let v = m_min(
        modDB.Sum(ModType::BASE, None, nm!["ReduceCritExtraDamage"]),
        100.0,
    );
    output.set(n!("CritExtraDamageReduction"), v);
    // L1630
    let v = calcLib::r#mod(&modDB, None, nm!["LightRadius"]);
    output.set(n!("LightRadiusMod"), v);
    // L1631
    let v = m_max(modDB.Sum(ModType::INC, None, nm!["LightRadius"]), 0.0);
    output.set(n!("LightRadiusInc"), v);
    // L1632
    if let Some(breakdown) = &breakdown {
        // L1633
        let v = breakdown.r#mod(&modDB, None, nm!["LightRadius"]);
        breakdown.set(n!("LightRadiusMod"), v);
    }
    // L1635
    let v = m_max(
        modDB.More(None, nm!["CurseEffectOnSelf"])
            * (100.0 + modDB.Sum(ModType::INC, None, nm!["CurseEffectOnSelf"])),
        0.0,
    );
    output.set(n!("CurseEffectOnSelf"), v);
    // L1636
    let v = modDB.More(None, nm!["ExposureEffectOnSelf"])
        * (100.0 + modDB.Sum(ModType::INC, None, nm!["ExposureEffectOnSelf"]));
    output.set(n!("ExposureEffectOnSelf"), v);
    // L1637
    let v = modDB.More(None, nm!["WitherEffectOnSelf"])
        * (100.0 + modDB.Sum(ModType::INC, None, nm!["WitherEffectOnSelf"]));
    output.set(n!("WitherEffectOnSelf"), v);

    // -- Ailment duration on self
    // L1640
    let v = modDB.Sum(ModType::BASE, None, nm!["SelfDebuffExpirationRate"]);
    output.set(n!("DebuffExpirationRate"), v);
    // L1641
    let v = 10000.0 / (100.0 + output.num(n!("DebuffExpirationRate")));
    output.set(n!("DebuffExpirationModifier"), v);
    // L1642
    let v = !output
        .get(n!("DebuffExpirationModifier"))
        .raw_eq(&Val::Num(100.0));
    output.set(n!("showDebuffExpirationModifier"), v);
    // L1643
    let v = modDB.More(None, nm!["SelfBlindDuration"])
        * (100.0 + modDB.Sum(ModType::INC, None, nm!["SelfBlindDuration"]))
        * output.num(n!("DebuffExpirationModifier"))
        / 100.0;
    output.set(n!("SelfBlindDuration"), v);

    // -- This is only used for breakdown purposes
    // L1646
    if modDB.Flag(None, nm!["IgniteDurationAppliesToElementalAilments"]) {
        // L1647
        let inc = modDB.Sum(ModType::INC, None, nm!["SelfIgniteDuration"]);
        // L1648
        let more = modDB.More(None, nm!["SelfIgniteDuration"]);
        // L1649
        if inc != 0.0 {
            // L1650
            modDB.NewMod(
                n!("SelfIgniteDurationToElementalAilments"),
                ModType::INC,
                inc,
                "Firesong",
                0,
                0,
                &[],
            );
        }
        // L1652
        if more != 1.0 {
            // L1653
            modDB.NewMod(
                n!("SelfIgniteDurationToElementalAilments"),
                ModType::MORE,
                more,
                "Firesong",
                0,
                0,
                &[],
            );
        }
    }

    // L1657
    for &ailment in &data.nonElementalAilmentTypeList {
        // L1658
        let more = modDB.More(
            None,
            &[cat!("Self", ailment, "Duration"), n!("SelfAilmentDuration")],
        );
        // L1659
        let inc = (100.0
            + modDB.Sum(
                ModType::INC,
                None,
                &[cat!("Self", ailment, "Duration"), n!("SelfAilmentDuration")],
            ))
            * 100.0;
        // L1660
        let v = inc * more
            / (100.0
                + output.num(n!("DebuffExpirationRate"))
                + modDB.Sum(
                    ModType::BASE,
                    None,
                    &[cat!("Self", ailment, "DebuffExpirationRate")],
                ));
        output.set(cat!("Self", ailment, "Duration"), v);
    }
    // L1662
    for &ailment in &data.elementalAilmentTypeList {
        // L1663
        let igniteAppliesToAll = modDB.Flag(None, nm!["IgniteDurationAppliesToElementalAilments"])
            && ailment != n!("Ignite");
        // L1664
        let more = modDB.More(
            None,
            &[
                cat!("Self", ailment, "Duration"),
                n!("SelfAilmentDuration"),
                n!("SelfElementalAilmentDuration"),
            ],
        ) * (if igniteAppliesToAll {
            modDB.More(None, nm!["SelfIgniteDuration"])
        } else {
            1.0
        });
        // L1665
        let inc = (100.0
            + modDB.Sum(
                ModType::INC,
                None,
                &[
                    cat!("Self", ailment, "Duration"),
                    n!("SelfAilmentDuration"),
                    n!("SelfElementalAilmentDuration"),
                ],
            )
            + (if igniteAppliesToAll {
                modDB.Sum(ModType::INC, None, nm!["SelfIgniteDuration"])
            } else {
                0.0
            }))
            * 100.0;
        // L1666
        let v = more * inc
            / (100.0
                + output.num(n!("DebuffExpirationRate"))
                + modDB.Sum(
                    ModType::BASE,
                    None,
                    &[cat!("Self", ailment, "DebuffExpirationRate")],
                ));
        output.set(cat!("Self", ailment, "Duration"), v);
    }
    // L1668
    for &ailment in &data.ailmentTypeList {
        // L1669
        let a = calcLib::r#mod(&modDB, None, &[cat!("Self", ailment, "Effect")]);
        let b = if modDB.Flag(None, &[cat!("Condition:", ailment, "edSelf")]) {
            calcLib::r#mod(&modDB, None, &[cat!("Enemy", ailment, "Effect")])
        } else {
            calcLib::r#mod(&enemyDB, None, &[cat!("Enemy", ailment, "Effect")])
        };
        output.set(cat!("Self", ailment, "Effect"), a * b * 100.0);
    }
    // L1671
    Flow::Normal
}
