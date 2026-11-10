//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! defence segment DF-D (CalcDefence.lua 1681-2152): not-hit chances, enemy
//! damage inputs, damage shift ("taken as") tables, damage taken multipliers
//! and the incoming hit damage multipliers of `calcs.buildDefenceEstimations`.
//!
//! Owner: DF-D.
#![allow(non_snake_case)]
// `local reductMult = 1` is overwritten before any read (kept verbatim)
#![allow(unused_assignments)]
// literal Lua statement shapes (`x = x * y`, `not a and b or 0`, locals assigned later)
#![allow(
    clippy::assign_op_pattern,
    clippy::nonminimal_bool,
    clippy::needless_late_init,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::bool_to_int_with_if
)]

use super::helpers::{dmgTypeList, hitSourceList, isElemental};
use super::{armourReduction, DefEstCtx};
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::ModType::{BASE, INC};

/// Lua `tonumber(v)` (nil and non-numeric values give nil).
fn tonumber(v: &Val) -> Option<f64> {
    match v {
        Val::Num(n) => Some(*n),
        Val::Str(s) => pob_lua::tonumber(s),
        _ => None,
    }
}

/// `t[k]` used as a table (`t[k].rowList`, `t_insert(t[k], …)`): anything but a
/// table raises, as indexing it would in Lua.
fn tab(t: &Table, k: impl Into<Key>) -> Table {
    let k = k.into();
    match t.get(k) {
        Val::Table(x) => x,
        other => lua_error!(
            "attempt to index field '{:?}' (a {} value)",
            k,
            other.lua_type()
        ),
    }
}

/// `data.monsterPhysConversionMultiTable[env.enemyLevel]` in arithmetic.
fn monsterPhysConversionMulti(enemyLevel: f64) -> f64 {
    let t = &data().monsterPhysConversionMultiTable;
    if enemyLevel >= 1.0 && enemyLevel.fract() == 0.0 && enemyLevel <= t.len() as f64 {
        t[enemyLevel as usize - 1]
    } else {
        lua_error!("attempt to perform arithmetic on field '?' (a nil value)")
    }
}

/// `{ label = l, key = k }` (the colList rows).
fn col(label: &'static str, key: &'static str) -> Table {
    Table::from_ctor(vec![
        crate::lua_table::TableField::named(n!("label"), true, label),
        crate::lua_table::TableField::named(n!("key"), true, key),
    ])
}

/// `{ type = t, value = v }` (the "taken" breakdown rows).
fn typeValueRow(r#type: String, value: String) -> Table {
    tbl! { [n!("type")] => r#type, [n!("value")] => value }
}

/// @pob Modules/CalcDefence.lua:1681-2152 h=3809a1c4
/// DF-D: CalcDefence.lua 1681-2152.
pub(crate) fn seg(ctx: &mut DefEstCtx) -> Flow {
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();

    // L1682 `local condList = modDB.conditions`: never read in this segment

    // L1684
    let damageCategoryConfig: Name = match env.configInput.get(n!("enemyDamageType")) {
        Val::Nil | Val::Bool(false) => n!("Average"),
        Val::Str(s) => Name::from(&s),
        Val::Num(x) => Name::from(tostring_num(x).as_str()),
        other => lua_error!(
            "attempt to concatenate a {} value (enemyDamageType)",
            other.lua_type()
        ),
    };
    ctx.damageCategoryConfig = damageCategoryConfig;
    let dcc = damageCategoryConfig.as_str();

    // chance to not be hit calculations
    // L1687
    if dcc != "DamageOverTime" {
        // L1688
        let worstOf = env.configInput.num_or(n!("EHPUnluckyWorstOf"), 1.0);
        // L1689
        output.set(
            n!("MeleeNotHitChance"),
            100.0
                - (1.0 - output.num(n!("MeleeEvadeChance")) / 100.0)
                    * (1.0 - output.num(n!("EffectiveAttackDodgeChance")) / 100.0)
                    * (1.0 - output.num(n!("AvoidAllDamageFromHitsChance")) / 100.0)
                    * 100.0,
        );
        // L1690
        output.set(
            n!("ProjectileNotHitChance"),
            100.0
                - (1.0 - output.num(n!("ProjectileEvadeChance")) / 100.0)
                    * (1.0 - output.num(n!("EffectiveAttackDodgeChance")) / 100.0)
                    * (1.0 - output.num(n!("AvoidAllDamageFromHitsChance")) / 100.0)
                    * (1.0
                        - (if output.truthy(n!("specificTypeAvoidance")) {
                            0.0
                        } else {
                            output.num(n!("AvoidProjectilesChance"))
                        }) / 100.0)
                    * 100.0,
        );
        // L1691
        output.set(
            n!("SpellNotHitChance"),
            100.0
                - (1.0 - output.num(n!("EffectiveSpellDodgeChance")) / 100.0)
                    * (1.0 - output.num(n!("AvoidAllDamageFromHitsChance")) / 100.0)
                    * 100.0,
        );
        // L1692
        output.set(
            n!("SpellProjectileNotHitChance"),
            100.0
                - (1.0 - output.num(n!("EffectiveSpellDodgeChance")) / 100.0)
                    * (1.0 - output.num(n!("AvoidAllDamageFromHitsChance")) / 100.0)
                    * (1.0
                        - (if output.truthy(n!("specificTypeAvoidance")) {
                            0.0
                        } else {
                            output.num(n!("AvoidProjectilesChance"))
                        }) / 100.0)
                    * 100.0,
        );
        // L1693
        output.set(
            n!("UntypedNotHitChance"),
            100.0 - (1.0 - output.num(n!("AvoidAllDamageFromHitsChance")) / 100.0) * 100.0,
        );
        // L1694
        output.set(
            n!("AverageNotHitChance"),
            (output.num(n!("MeleeNotHitChance"))
                + output.num(n!("ProjectileNotHitChance"))
                + output.num(n!("SpellNotHitChance"))
                + output.num(n!("SpellProjectileNotHitChance")))
                / 4.0,
        );
        // L1695
        output.set(
            n!("AverageEvadeChance"),
            (output.num(n!("MeleeEvadeChance")) + output.num(n!("ProjectileEvadeChance"))) / 4.0,
        );
        // L1696
        let v = output.get(cat!(damageCategoryConfig, "NotHitChance"));
        output.set(n!("ConfiguredNotHitChance"), v);
        // L1697
        let v = output.num_or(cat!(damageCategoryConfig, "EvadeChance"), 0.0);
        output.set(n!("ConfiguredEvadeChance"), v);
        // unlucky config to lower the value of block, dodge, evade etc for ehp
        // L1699
        if worstOf > 1.0 {
            // L1700
            let v = output.num(n!("ConfiguredNotHitChance")) / 100.0
                * output.num(n!("ConfiguredNotHitChance"));
            output.set(n!("ConfiguredNotHitChance"), v);
            // L1701
            let v = output.num(n!("ConfiguredEvadeChance")) / 100.0
                * output.num(n!("ConfiguredEvadeChance"));
            output.set(n!("ConfiguredEvadeChance"), v);
            // L1702
            if worstOf == 4.0 {
                // L1703
                let v = output.num(n!("ConfiguredNotHitChance")) / 100.0
                    * output.num(n!("ConfiguredNotHitChance"));
                output.set(n!("ConfiguredNotHitChance"), v);
                // L1704
                let v = output.num(n!("ConfiguredEvadeChance")) / 100.0
                    * output.num(n!("ConfiguredEvadeChance"));
                output.set(n!("ConfiguredEvadeChance"), v);
            }
        }
    }

    //Enemy damage input and modifications
    // L1710
    output.set(n!("totalEnemyDamage"), 0.0);
    // L1711
    output.set(n!("totalEnemyDamageIn"), 0.0);
    // L1712
    if dcc == "DamageOverTime" {
        // L1713
        for damageType in dmgTypeList {
            // L1714
            output.set(cat!(damageType, "EnemyPen"), 0.0);
            // L1715 missing taunt from allies
            let v = calcLib::r#mod(
                &enemyDB,
                None,
                &names([
                    Some(n!("Damage")),
                    Some(cat!(damageType, "Damage")),
                    isElemental(damageType).then(|| n!("ElementalDamage")),
                ]),
            );
            output.set(cat!(damageType, "EnemyDamageMult"), v);
            // L1716
            output.set(cat!(damageType, "EnemyOverwhelm"), 0.0);
            // L1717
            output.set(cat!(damageType, "EnemyDamage"), 0.0);
        }
    } else {
        // L1720
        if let Some(breakdown) = &breakdown {
            // L1721
            breakdown.set(
                n!("totalEnemyDamage"),
                tbl! {
                    label = "Total damage from the enemy",
                    rowList = tbl!(),
                    colList = tbl![
                        col("Type", "type"),
                        col("Value", "value"),
                        col("Mult", "mult"),
                        col("Crit", "crit"),
                        col("Conversion", "convMult"),
                        col("Final", "final"),
                        col("From", "from"),
                    ],
                },
            );
        }
        // L1735
        let enemyCritChance = if enemyDB.Flag(None, nm!["NeverCrit"]) {
            0.0
        } else if enemyDB.Flag(None, nm!["AlwaysCrit"]) {
            100.0
        } else {
            let base = lor(
                Val::from_value(&modDB.Override(None, nm!["enemyCritChance"])),
                || env.configInput.get(n!("enemyCritChance")),
            );
            let base = lor(base, || env.configPlaceholder.get(n!("enemyCritChance")));
            let base = lor(base, || Val::Num(0.0)).num();
            m_max(
                m_min(
                    base * (1.0
                        + modDB.Sum(INC, None, nm!["EnemyCritChance"]) / 100.0
                        + enemyDB.Sum(INC, None, nm!["CritChance"]) / 100.0)
                        * (1.0 - output.num(n!("ConfiguredEvadeChance")) / 100.0),
                    100.0,
                ),
                0.0,
            )
        };
        // L1736
        output.set(n!("EnemyCritChance"), enemyCritChance);
        // L1737
        let enemyCritDamage = m_max(
            lor(
                lor(env.configInput.get(n!("enemyCritDamage")), || {
                    env.configPlaceholder.get(n!("enemyCritDamage"))
                }),
                || Val::Num(0.0),
            )
            .num()
                + enemyDB.Sum(BASE, None, nm!["CritMultiplier"]),
            0.0,
        );
        // L1738
        output.set(
            n!("EnemyCritEffect"),
            1.0 + enemyCritChance / 100.0
                * (enemyCritDamage / 100.0)
                * (1.0 - output.num(n!("CritExtraDamageReduction")) / 100.0),
        );
        // L1739 Match all keywordFlags parameter for enemy min-max damage mods
        let enemyCfg = ModCfg::new();
        enemyCfg
            .keywordFlags
            .set(pob_lua::bit::bnot(KeywordFlag::MatchAll));
        let enemyCfg = Some(&*enemyCfg);
        // L1740
        let enemyDamageConversion = Table::new();
        // L1741
        for damageType in dmgTypeList {
            // L1742 missing taunt from allies
            let enemyDamageMult = calcLib::r#mod(
                &enemyDB,
                None,
                &names([
                    Some(n!("Damage")),
                    Some(cat!(damageType, "Damage")),
                    isElemental(damageType).then(|| n!("ElementalDamage")),
                ]),
            );
            // L1743
            let mut enemyDamage =
                tonumber(&env.configInput.get(cat!("enemy", damageType, "Damage")));
            // L1744
            let mut enemyPen = tonumber(&env.configInput.get(cat!("enemy", damageType, "Pen")));
            // L1745
            let mut enemyOverwhelm =
                tonumber(&env.configInput.get(cat!("enemy", damageType, "Overwhelm")));
            // L1746
            let sourceStr = if enemyDamage.is_none() {
                "Default"
            } else {
                "Config"
            };

            // L1748
            if enemyDamage.is_none() {
                // L1749
                enemyDamage = Some(
                    tonumber(
                        &env.configPlaceholder
                            .get(cat!("enemy", damageType, "Damage")),
                    )
                    .unwrap_or(0.0),
                );
            }
            // L1751
            if enemyPen.is_none() {
                // L1752
                enemyPen = Some(
                    tonumber(&env.configPlaceholder.get(cat!("enemy", damageType, "Pen")))
                        .unwrap_or(0.0),
                );
            }
            // L1754
            if enemyOverwhelm.is_none() {
                // L1755 (sic: the placeholder key is "enemy<Type>enemyOverwhelm")
                enemyOverwhelm = Some(
                    tonumber(&env.configPlaceholder.get(cat!(
                        "enemy",
                        damageType,
                        "enemyOverwhelm"
                    )))
                    .unwrap_or(0.0),
                );
            }
            let mut enemyDamage = enemyDamage.unwrap_or(0.0);
            let enemyPen = enemyPen.unwrap_or(0.0);
            let mut enemyOverwhelm = enemyOverwhelm.unwrap_or(0.0);

            // Add min-max enemy damage from mods
            // L1759
            enemyDamage = enemyDamage
                + (enemyDB.Sum(BASE, enemyCfg, &[cat!(damageType, "Min")])
                    + enemyDB.Sum(BASE, enemyCfg, &[cat!(damageType, "Max")]))
                    / 2.0;

            // Conversion and Gain As Mods
            // L1762
            let mut conversionTotal = 0.0;
            // L1763
            if damageType == "Physical" {
                // L1764
                let conversions = tbl! { total = 0.0, totalSkill = 0.0 };
                // L1765
                for damageTypeTo in dmgTypeList {
                    // L1766
                    conversions.set(
                        cat!(damageTypeTo, "skill"),
                        enemyDB.Sum(
                            BASE,
                            enemyCfg,
                            &[cat!(damageType, "DamageSkillConvertTo", damageTypeTo)],
                        ),
                    );
                    // L1767
                    conversions.set(
                        Name::from(damageTypeTo),
                        enemyDB.Sum(
                            BASE,
                            enemyCfg,
                            &[cat!(damageType, "DamageConvertTo", damageTypeTo)],
                        ),
                    );
                    // L1768
                    let v = conversions.num(n!("totalSkill"))
                        + conversions.num(cat!(damageTypeTo, "skill"));
                    conversions.set(n!("totalSkill"), v);
                    // L1769
                    let v =
                        conversions.num(n!("total")) + conversions.num(Name::from(damageTypeTo));
                    conversions.set(n!("total"), v);
                }
                // Cap the amount of conversion to 100%
                // L1772
                if conversions.num(n!("totalSkill")) > 100.0 {
                    // L1773
                    let mult = 100.0 / conversions.num(n!("totalSkill"));
                    // L1774
                    let v = conversions.num(n!("totalSkill")) * mult;
                    conversions.set(n!("totalSkill"), v);
                    // L1775
                    conversions.set(n!("total"), 0.0);
                    // L1776
                    for damageTypeTo in dmgTypeList {
                        // L1777
                        let v = conversions.num(cat!(damageTypeTo, "skill")) * mult;
                        conversions.set(cat!(damageTypeTo, "skill"), v);
                        // L1778
                        conversions.set(Name::from(damageTypeTo), 0.0);
                    }
                } else if conversions.num(n!("total")) + conversions.num(n!("totalSkill")) > 100.0 {
                    // L1780
                    // L1781
                    let mult =
                        (100.0 - conversions.num(n!("totalSkill"))) / conversions.num(n!("total"));
                    // L1782
                    let v = conversions.num(n!("total")) * mult;
                    conversions.set(n!("total"), v);
                    // L1783
                    for damageTypeTo in dmgTypeList {
                        // L1784
                        let v = conversions.num(Name::from(damageTypeTo)) * mult;
                        conversions.set(Name::from(damageTypeTo), v);
                    }
                }
                // L1787
                conversionTotal = conversions.num(n!("total")) + conversions.num(n!("totalSkill"));
                // Calculate the amount converted/gained as
                // L1789
                for damageTypeTo in dmgTypeList {
                    // L1790
                    let gainAsPercent = enemyDB.Sum(
                        BASE,
                        enemyCfg,
                        &[cat!(damageType, "DamageGainAs", damageTypeTo)],
                    ) / 100.0;
                    // L1791
                    let mut conversionPercent = conversions.num(Name::from(damageTypeTo)) / 100.0;
                    // L1792
                    let mut skillConversionPercent =
                        conversions.num(cat!(damageTypeTo, "skill")) / 100.0;
                    // L1793
                    if skillConversionPercent > 0.0 {
                        // L1794
                        if damageType == "Physical" && damageTypeTo != "Chaos" {
                            // L1795
                            let physBonus =
                                1.0 + monsterPhysConversionMulti(env.enemyLevel.get()) / 100.0;
                            // L1796
                            skillConversionPercent = skillConversionPercent * physBonus;
                        }
                        // L1798
                        conversionPercent = conversionPercent + skillConversionPercent;
                    }
                    // L1800
                    if gainAsPercent > 0.0 || conversionPercent > 0.0 {
                        // L1801
                        let t = enemyDamageConversion.get_or_new_table(Name::from(damageTypeTo));
                        // L1802
                        t.set(
                            Name::from(damageType),
                            enemyDamage * gainAsPercent + enemyDamage * conversionPercent,
                        );
                    }
                }
            }

            // L1807
            enemyOverwhelm = enemyOverwhelm
                + enemyDB.Sum(BASE, None, nm!["PhysicalOverwhelm"])
                + modDB.Sum(BASE, None, nm!["EnemyPhysicalOverwhelm"]);

            // L1809
            output.set(cat!(damageType, "EnemyPen"), enemyPen);
            // L1810
            output.set(cat!(damageType, "EnemyDamageMult"), enemyDamageMult);
            // L1811
            output.set(cat!(damageType, "EnemyOverwhelm"), enemyOverwhelm);
            // L1812
            let v = output.num(n!("totalEnemyDamageIn")) + enemyDamage;
            output.set(n!("totalEnemyDamageIn"), v);
            // L1813
            output.set(
                cat!(damageType, "EnemyDamage"),
                enemyDamage
                    * (1.0 - conversionTotal / 100.0)
                    * enemyDamageMult
                    * output.num(n!("EnemyCritEffect")),
            );
            // L1814
            let mut conversionExtra =
                -enemyDamage * enemyDamageMult * output.num(n!("EnemyCritEffect"))
                    + output.num(cat!(damageType, "EnemyDamage"));
            // L1815
            if enemyDamageConversion.truthy(Name::from(damageType)) {
                // L1816 (one source type, "Physical", is ever stored: no observable order)
                for (damageTypeFrom, enemyDamage) in
                    tab(&enemyDamageConversion, Name::from(damageType)).pairs()
                {
                    let damageTypeFrom: Name = damageTypeFrom
                        .name()
                        .unwrap_or_else(|| lua_error!("attempt to concatenate a non-string key"));
                    let enemyDamage = enemyDamage.num();
                    // L1817 missing taunt from allies
                    let enemyDamageMult = calcLib::r#mod(
                        &enemyDB,
                        None,
                        &names([
                            Some(n!("Damage")),
                            Some(cat!(damageType, "Damage")),
                            Some(cat!(damageTypeFrom, "Damage")),
                            isElemental(damageType).then(|| n!("ElementalDamage")),
                            isElemental(damageTypeFrom.as_str()).then(|| n!("ElementalDamage")),
                        ]),
                    );
                    // L1818
                    let v = output.num(cat!(damageType, "EnemyDamage"))
                        + enemyDamage * enemyDamageMult * output.num(n!("EnemyCritEffect"));
                    output.set(cat!(damageType, "EnemyDamage"), v);
                    // L1819
                    conversionExtra = conversionExtra
                        + enemyDamage * enemyDamageMult * output.num(n!("EnemyCritEffect"));
                }
            }
            // L1822
            let v =
                output.num(n!("totalEnemyDamage")) + output.num(cat!(damageType, "EnemyDamage"));
            output.set(n!("totalEnemyDamage"), v);
            // L1823
            if let Some(breakdown) = &breakdown {
                // L1824
                breakdown.set(
                    cat!(damageType, "EnemyDamage"),
                    tbl![
                        lfmt!("from %s: %d", sourceStr, enemyDamage),
                        lfmt!("* %.2f (modifiers to enemy damage)", enemyDamageMult),
                        lfmt!(
                            "* %.3f (enemy crit effect)",
                            output.num(n!("EnemyCritEffect"))
                        ),
                    ],
                );
                // L1829
                if conversionExtra != 0.0 {
                    // L1830
                    tab(breakdown, cat!(damageType, "EnemyDamage")).push(lfmt!(
                        "%s %d (enemy damage conversion)",
                        if conversionExtra > 0.0 { "+" } else { "-" },
                        if conversionExtra >= 0.0 {
                            conversionExtra
                        } else {
                            -conversionExtra
                        }
                    ));
                }
                // L1832
                tab(breakdown, cat!(damageType, "EnemyDamage"))
                    .push(lfmt!("= %d", output.num(cat!(damageType, "EnemyDamage"))));
                // L1833
                tab(&tab(breakdown, n!("totalEnemyDamage")), n!("rowList")).push(tbl! {
                    [n!("type")] => lfmt!("%s", damageType),
                    [n!("value")] => lfmt!("%d", enemyDamage),
                    [n!("mult")] => lfmt!("%.2f", enemyDamageMult),
                    [n!("crit")] => lfmt!("%.2f", output.num(n!("EnemyCritEffect"))),
                    [n!("convMult")] => lfmt!(
                        "%s%d",
                        if conversionExtra > 0.0 {
                            "+"
                        } else if conversionExtra < 0.0 {
                            "-"
                        } else {
                            ""
                        },
                        if conversionExtra >= 0.0 { conversionExtra } else { -conversionExtra }
                    ),
                    [n!("final")] => lfmt!("%d", output.num(cat!(damageType, "EnemyDamage"))),
                    [n!("from")] => lfmt!("%s", sourceStr),
                });
            }
        }
    }

    //Damage Taken as
    // L1847
    {
        // L1848
        let t = match actor.damageShiftTable.get() {
            Some(t) => {
                t.wipe();
                t
            }
            None => Table::new(),
        };
        actor.damageShiftTable.set(Some(t));
        // L1849
        let t = match actor.damageOverTimeShiftTable.get() {
            Some(t) => {
                t.wipe();
                t
            }
            None => Table::new(),
        };
        actor.damageOverTimeShiftTable.set(Some(t));
        // L1850
        for damageType in dmgTypeList {
            // Build damage shift tables
            // L1852
            let shiftTable = Table::new();
            // L1853
            let dotShiftTable = Table::new();
            // L1854
            let mut destTotal = 0.0;
            // L1855
            let mut dotDestinationTotal = 0.0;
            // L1856
            for destType in dmgTypeList {
                // L1857
                if destType != damageType {
                    // L1858
                    dotShiftTable.set(
                        Name::from(destType),
                        modDB.Sum(
                            BASE,
                            None,
                            &names([
                                Some(cat!(damageType, "DamageTakenAs", destType)),
                                isElemental(damageType)
                                    .then(|| cat!("ElementalDamageTakenAs", destType)),
                            ]),
                        ),
                    );
                    // L1859
                    dotDestinationTotal =
                        dotDestinationTotal + dotShiftTable.num(Name::from(destType));
                    // L1860
                    let v = dotShiftTable.num(Name::from(destType))
                        + modDB.Sum(
                            BASE,
                            None,
                            &names([
                                Some(cat!(damageType, "DamageFromHitsTakenAs", destType)),
                                isElemental(damageType)
                                    .then(|| cat!("ElementalDamageFromHitsTakenAs", destType)),
                            ]),
                        );
                    shiftTable.set(Name::from(destType), v);
                    // L1861
                    destTotal = destTotal + shiftTable.num(Name::from(destType));
                }
            }
            // L1864
            dotShiftTable.set(
                Name::from(damageType),
                m_max(100.0 - dotDestinationTotal, 0.0),
            );
            // L1865
            actor
                .damageOverTimeShiftTable
                .get()
                .expect("set at L1851")
                .set(Name::from(damageType), dotShiftTable.clone());
            // L1866
            shiftTable.set(Name::from(damageType), m_max(100.0 - destTotal, 0.0));
            // L1867
            actor
                .damageShiftTable
                .get()
                .expect("set at L1850")
                .set(Name::from(damageType), shiftTable.clone());

            //add same type damage
            // L1870
            let dst = actor.damageShiftTable.get().expect("set at L1850");
            let v = output.num(cat!(damageType, "EnemyDamage"))
                * tab(&dst, Name::from(damageType)).num(Name::from(damageType))
                / 100.0;
            output.set(cat!(damageType, "TakenDamage"), v);
            // L1871
            if let Some(breakdown) = &breakdown {
                // L1872
                breakdown.set(
                    cat!(damageType, "TakenDamage"),
                    tbl! {
                        label = "Taken",
                        rowList = tbl!(),
                        colList = tbl![col("Type", "type"), col("Value", "value")],
                    },
                );
                // L1880
                tab(
                    &tab(breakdown, cat!(damageType, "TakenDamage")),
                    n!("rowList"),
                )
                .push(typeValueRow(
                    lfmt!("%s", damageType),
                    lfmt!("%d", output.num(cat!(damageType, "TakenDamage"))),
                ));
            }
        }
        //converted damage types
        // L1887
        for damageType in dmgTypeList {
            // L1888
            for damageConvertedType in dmgTypeList {
                // L1889
                if damageType != damageConvertedType {
                    // L1890
                    let dst = actor.damageShiftTable.get().expect("set at L1850");
                    let damage = output.num(cat!(damageType, "EnemyDamage"))
                        * tab(&dst, Name::from(damageType)).num(Name::from(damageConvertedType))
                        / 100.0;
                    // L1891
                    let v = output.num(cat!(damageConvertedType, "TakenDamage")) + damage;
                    output.set(cat!(damageConvertedType, "TakenDamage"), v);
                    // L1892
                    if let Some(breakdown) = breakdown.as_ref().filter(|_| damage > 0.0) {
                        // L1893
                        tab(
                            &tab(breakdown, cat!(damageConvertedType, "TakenDamage")),
                            n!("rowList"),
                        )
                        .push(typeValueRow(lfmt!("%s", damageType), lfmt!("%d", damage)));
                    }
                }
            }
        }
        //total
        // L1902
        output.set(n!("totalTakenDamage"), 0.0);
        // L1903
        if let Some(breakdown) = &breakdown {
            // L1904
            breakdown.set(
                n!("totalTakenDamage"),
                tbl! {
                    label = "Total damage taken from the enemy after taken as",
                    rowList = tbl!(),
                    colList = tbl![col("Type", "type"), col("Value", "value")],
                },
            );
        }
        // L1913
        for damageType in dmgTypeList {
            // L1914
            let v =
                output.num(n!("totalTakenDamage")) + output.num(cat!(damageType, "TakenDamage"));
            output.set(n!("totalTakenDamage"), v);
            // L1915
            if let Some(breakdown) = &breakdown {
                // L1916
                tab(&tab(breakdown, n!("totalTakenDamage")), n!("rowList")).push(typeValueRow(
                    lfmt!("%s", damageType),
                    lfmt!("%d", output.num(cat!(damageType, "TakenDamage"))),
                ));
            }
        }
    }

    // Damage taken multipliers/Degen calculations
    // L1925
    output.set(n!("AnyTakenReflect"), false);
    // L1926
    for damageType in dmgTypeList {
        // L1927
        let mut baseTakenInc = modDB.Sum(
            INC,
            None,
            &[n!("DamageTaken"), cat!(damageType, "DamageTaken")],
        );
        // L1928
        let mut baseTakenMore =
            modDB.More(None, &[n!("DamageTaken"), cat!(damageType, "DamageTaken")]);
        // L1929
        if isElemental(damageType) {
            // L1930
            baseTakenInc = baseTakenInc + modDB.Sum(INC, None, nm!["ElementalDamageTaken"]);
            // L1931
            baseTakenMore = baseTakenMore * modDB.More(None, nm!["ElementalDamageTaken"]);
        }
        // L1933 Hit
        {
            // L1934
            let mut takenInc = baseTakenInc
                + modDB.Sum(
                    INC,
                    None,
                    &[
                        n!("DamageTakenWhenHit"),
                        cat!(damageType, "DamageTakenWhenHit"),
                    ],
                );
            // L1935
            let mut takenMore = baseTakenMore
                * modDB.More(
                    None,
                    &[
                        n!("DamageTakenWhenHit"),
                        cat!(damageType, "DamageTakenWhenHit"),
                    ],
                );
            // L1936
            if isElemental(damageType) {
                // L1937
                takenInc = takenInc + modDB.Sum(INC, None, nm!["ElementalDamageTakenWhenHit"]);
                // L1938
                takenMore = takenMore * modDB.More(None, nm!["ElementalDamageTakenWhenHit"]);
            }
            // L1940
            output.set(
                cat!(damageType, "TakenHitMult"),
                m_max((1.0 + takenInc / 100.0) * takenMore, 0.0),
            );

            // L1942
            for hitType in hitSourceList {
                // L1943
                let baseTakenIncType =
                    takenInc + modDB.Sum(INC, None, &[cat!(hitType, "DamageTaken")]);
                // L1944
                let baseTakenMoreType =
                    takenMore * modDB.More(None, &[cat!(hitType, "DamageTaken")]);
                // L1945
                output.set(
                    cat!(hitType, "TakenHitMult"),
                    m_max((1.0 + baseTakenIncType / 100.0) * baseTakenMoreType, 0.0),
                );
                // L1946
                let v = output.get(cat!(hitType, "TakenHitMult"));
                output.set(cat!(damageType, hitType, "TakenHitMult"), v);
            }
            // L1948
            {
                // Reflect
                // L1950
                takenInc = takenInc
                    + modDB.Sum(
                        INC,
                        None,
                        &[
                            n!("ReflectedDamageTaken"),
                            cat!(damageType, "ReflectedDamageTaken"),
                        ],
                    );
                // L1951
                takenMore = takenMore
                    * modDB.More(
                        None,
                        &[
                            n!("ReflectedDamageTaken"),
                            cat!(damageType, "ReflectedDamageTaken"),
                        ],
                    );
                // L1952
                if isElemental(damageType) {
                    // L1953
                    takenInc =
                        takenInc + modDB.Sum(INC, None, nm!["ElementalReflectedDamageTaken"]);
                    // L1954
                    takenMore = takenMore * modDB.More(None, nm!["ElementalReflectedDamageTaken"]);
                }
                // L1956
                output.set(
                    cat!(damageType, "TakenReflect"),
                    m_max((1.0 + takenInc / 100.0) * takenMore, 0.0),
                );
                // L1957
                if output.num(cat!(damageType, "TakenReflect"))
                    != output.num(cat!(damageType, "TakenHitMult"))
                {
                    // L1958 true --this needs a rework as well
                    output.set(n!("AnyTakenReflect"), false);
                }
            }
        }
        // L1962 Dot
        {
            // L1963
            let mut takenInc = baseTakenInc
                + modDB.Sum(
                    INC,
                    None,
                    &[
                        n!("DamageTakenOverTime"),
                        cat!(damageType, "DamageTakenOverTime"),
                    ],
                );
            // L1964
            let mut takenMore = baseTakenMore
                * modDB.More(
                    None,
                    &[
                        n!("DamageTakenOverTime"),
                        cat!(damageType, "DamageTakenOverTime"),
                    ],
                );
            // L1965
            if isElemental(damageType) {
                // L1966
                takenInc = takenInc + modDB.Sum(INC, None, nm!["ElementalDamageTakenOverTime"]);
                // L1967
                takenMore = takenMore * modDB.More(None, nm!["ElementalDamageTakenOverTime"]);
            }
            // L1969
            let resist = if modDB.Flag(None, &[cat!("SelfIgnore", damageType, "Resistance")]) {
                0.0
            } else {
                lor(output.get(cat!(damageType, "ResistOverTime")), || {
                    output.get(cat!(damageType, "Resist"))
                })
                .num()
            };
            // L1970
            let reduction = if modDB.Flag(
                None,
                &[cat!("SelfIgnore", "Base", damageType, "DamageReduction")],
            ) {
                0.0
            } else {
                output.num(cat!("Base", damageType, "DamageReduction"))
            };
            // L1971
            output.set(
                cat!(damageType, "TakenDotMult"),
                m_max(
                    (1.0 - resist / 100.0)
                        * (1.0 - reduction / 100.0)
                        * (1.0 + takenInc / 100.0)
                        * takenMore,
                    0.0,
                ),
            );
            // L1972
            if let Some(breakdown) = &breakdown {
                // L1973
                breakdown.set(cat!(damageType, "TakenDotMult"), tbl!());
                // L1974
                let lower = damageType.to_ascii_lowercase();
                breakdown.multiChain(
                    &tab(breakdown, cat!(damageType, "TakenDotMult")),
                    &tbl! {
                        tbl!["%.2f ^8(resistance)", 1.0 - resist / 100.0],
                        tbl!["%.2f ^8(%s damage reduction)", 1.0 - reduction / 100.0, lower],
                        tbl!["%.2f ^8(increased/reduced damage taken)", 1.0 + takenInc / 100.0],
                        tbl!["%.2f ^8(more/less damage taken)", takenMore];
                        label = "DoT Multiplier:",
                        total = lfmt!("= %.2f", output.num(cat!(damageType, "TakenDotMult"))),
                    },
                );
            }
        }
    }

    // Incoming hit damage multipliers
    // L1987
    output.set(n!("totalTakenHit"), 0.0);
    // L1988
    if let Some(breakdown) = &breakdown {
        // L1989
        breakdown.set(
            n!("totalTakenHit"),
            tbl! {
                label = "Total damage taken after mitigation",
                rowList = tbl!(),
                colList = tbl![
                    col("Type", "type"),
                    col("Incoming", "incoming"),
                    col("Mult", "mult"),
                    col("Value", "value"),
                ],
            },
        );
    }
    // L2000
    let enemyImpaleChance = {
        let impaleCfg = ModCfg::with_flags(
            if dcc == "Melee" || dcc == "Projectile" || dcc == "Average" {
                ModFlag::Attack
            } else {
                0
            },
            0,
        );
        enemyDB.Sum(BASE, Some(&*impaleCfg), nm!["ImpaleChance"])
            * (if dcc == "Average" { 0.5 } else { 1.0 })
            * (1.0 - output.num_or(n!("ImpaleAvoidChance"), 0.0))
    };
    // L2001
    for damageType in dmgTypeList {
        // Calculate incoming damage multiplier
        // L2003
        let resist = if modDB.Flag(None, &[cat!("SelfIgnore", damageType, "Resistance")]) {
            0.0
        } else {
            lor(output.get(cat!(damageType, "ResistWhenHit")), || {
                output.get(cat!(damageType, "Resist"))
            })
            .num()
        };
        // L2004
        let reduction = if modDB.Flag(
            None,
            &[cat!("SelfIgnore", "Base", damageType, "DamageReduction")],
        ) {
            0.0
        } else {
            lor(
                output.get(cat!("Base", damageType, "DamageReductionWhenHit")),
                || output.get(cat!("Base", damageType, "DamageReduction")),
            )
            .num()
        };
        // L2005
        let enemyPen = if modDB.Flag(
            None,
            &[
                cat!("SelfIgnore", damageType, "Resistance"),
                cat!("EnemyCannotPen", damageType, "Resistance"),
            ],
        ) {
            0.0
        } else {
            output.num(cat!(damageType, "EnemyPen"))
        };
        // L2006
        let enemyOverwhelm =
            if modDB.Flag(None, &[cat!("SelfIgnore", damageType, "DamageReduction")]) {
                0.0
            } else {
                output.num(cat!(damageType, "EnemyOverwhelm"))
            };
        // L2007
        let damage = output.num(cat!(damageType, "TakenDamage"));
        // L2008
        let mut impaleDamage = if enemyImpaleChance > 0.0 {
            if damageType == "Physical" {
                damage * data().misc.ImpaleStoredDamageBase
            } else {
                0.0
            }
        } else {
            0.0
        };
        // L2009
        let mut armourReduct = 0.0;
        // L2010
        let mut impaleArmourReduct = 0.0;
        // L2011
        let percentOfArmourApplies = m_min(
            if !modDB.Flag(
                None,
                &[cat!("ArmourDoesNotApplyTo", damageType, "DamageTaken")],
            ) {
                modDB.Sum(
                    BASE,
                    None,
                    &[cat!("ArmourAppliesTo", damageType, "DamageTaken")],
                )
            } else {
                0.0
            },
            100.0,
        );
        // L2012
        let mut effectiveAppliedArmour = (output.num(n!("Armour")) * percentOfArmourApplies
            / 100.0)
            * (1.0 + output.num(n!("ArmourDefense")));
        // L2013
        let physicalReductionBasedOnWard =
            damageType == "Physical" && modDB.Flag(None, nm!["PhysicalReductionBasedOnWard"]);
        // L2014
        if physicalReductionBasedOnWard {
            // L2015
            let multiplier =
                Val::from_value(&modDB.Override(None, nm!["PhysicalReductionBasedOnWardPercent"]))
                    .num()
                    / 100.0;
            // L2016
            effectiveAppliedArmour = output.num(n!("Ward")) * multiplier;
        }
        // L2018
        let resMult = 1.0 - (resist - enemyPen) / 100.0;
        // L2019
        let mut reductMult = 1.0;
        // L2020
        let mut takenFlat = modDB.Sum(
            BASE,
            None,
            &[
                n!("DamageTaken"),
                cat!(damageType, "DamageTaken"),
                n!("DamageTakenWhenHit"),
                cat!(damageType, "DamageTakenWhenHit"),
            ],
        );
        // L2021
        if dcc == "Melee" || dcc == "Projectile" {
            // L2022
            takenFlat = takenFlat
                + modDB.Sum(
                    BASE,
                    None,
                    &[
                        n!("DamageTakenFromAttacks"),
                        cat!(damageType, "DamageTakenFromAttacks"),
                        cat!(
                            damageType,
                            "DamageTakenFrom",
                            damageCategoryConfig,
                            "Attacks"
                        ),
                    ],
                );
        } else if dcc == "Spell" || dcc == "SpellProjectile" {
            // L2023
            // L2024
            takenFlat = takenFlat
                + modDB.Sum(
                    BASE,
                    None,
                    &[
                        n!("DamageTakenFromSpells"),
                        cat!(damageType, "DamageTakenFromSpells"),
                        cat!(damageType, "DamageTakenFromSpellProjectiles"),
                    ],
                );
        } else if dcc == "Average" {
            // L2025
            // L2026
            takenFlat = takenFlat
                + modDB.Sum(
                    BASE,
                    None,
                    &[
                        n!("DamageTakenFromAttacks"),
                        cat!(damageType, "DamageTakenFromAttacks"),
                    ],
                ) / 2.0
                + modDB.Sum(
                    BASE,
                    None,
                    &[cat!(damageType, "DamageTakenFromProjectileAttacks")],
                ) / 4.0
                + modDB.Sum(
                    BASE,
                    None,
                    &[
                        n!("DamageTakenFromSpells"),
                        cat!(damageType, "DamageTakenFromSpells"),
                    ],
                ) / 2.0
                + modDB.Sum(
                    BASE,
                    None,
                    &[
                        n!("DamageTakenFromSpellProjectiles"),
                        cat!(damageType, "DamageTakenFromSpellProjectiles"),
                    ],
                ) / 4.0;
        }
        // L2028
        output.set(cat!(damageType, "takenFlat"), takenFlat);
        // L2029
        if percentOfArmourApplies > 0.0 || physicalReductionBasedOnWard {
            // L2030
            armourReduct = armourReduction(effectiveAppliedArmour, damage * resMult);
            // L2031
            armourReduct = m_min(output.num(n!("DamageReductionMax")), armourReduct);
            // L2032
            if impaleDamage > 0.0 {
                // L2033
                impaleArmourReduct = m_min(
                    output.num(n!("DamageReductionMax")),
                    armourReduction(effectiveAppliedArmour, impaleDamage * resMult),
                );
            }
        }
        // L2036
        let totalReduct = m_min(
            output.num(n!("DamageReductionMax")),
            armourReduct + reduction,
        );
        // L2037
        reductMult = 1.0
            - m_max(
                m_min(
                    output.num(n!("DamageReductionMax")),
                    totalReduct - enemyOverwhelm,
                ),
                0.0,
            ) / 100.0;
        // L2038
        output.set(
            cat!(damageType, "DamageReduction"),
            100.0 - reductMult * 100.0,
        );
        // L2039
        if impaleDamage > 0.0 {
            // L2040
            impaleDamage = impaleDamage
                * resMult
                * (1.0
                    - m_max(
                        m_min(
                            output.num(n!("DamageReductionMax")),
                            m_min(
                                output.num(n!("DamageReductionMax")),
                                impaleArmourReduct + reduction,
                            ) - enemyOverwhelm,
                        ),
                        0.0,
                    ) / 100.0);
            // L2041
            impaleDamage = impaleDamage * enemyImpaleChance / 100.0
                * 5.0
                * output.num(cat!(damageType, "TakenReflect"));
        }
        // L2043
        if let Some(breakdown) = &breakdown {
            // L2044
            breakdown.set(cat!(damageType, "DamageReduction"), tbl!());
            let bdr = || tab(breakdown, cat!(damageType, "DamageReduction"));
            // L2045
            if armourReduct != 0.0 {
                // L2046
                if resMult != 1.0 {
                    // L2047
                    bdr().push(lfmt!(
                        "Enemy Hit Damage After Resistance: %d ^8(total incoming damage)",
                        damage * resMult
                    ));
                } else {
                    // L2049
                    bdr().push(lfmt!(
                        "Enemy Hit Damage: %d ^8(total incoming damage)",
                        damage
                    ));
                }
                // L2051
                if percentOfArmourApplies != 100.0 && !physicalReductionBasedOnWard {
                    // L2052
                    bdr().push(lfmt!(
                        "%d%% percent of armour applies",
                        percentOfArmourApplies
                    ));
                }
                // L2054
                bdr().push(lfmt!("Reduction from Armour: %d%%", armourReduct));
            }
            // L2056
            if reduction != 0.0 {
                // L2057
                bdr().push(lfmt!(
                    "Base %s Damage Reduction: %d%%",
                    damageType,
                    reduction
                ));
            }
            // L2059
            if enemyOverwhelm != 0.0 {
                // L2060
                bdr().push(lfmt!(
                    "Enemy Overwhelm %s Damage: %d%%",
                    damageType,
                    enemyOverwhelm
                ));
            }
            // L2062
            if (if armourReduct != 0.0 { 1.0 } else { 0.0 })
                + (if reduction != 0.0 { 1.0 } else { 0.0 })
                + (if enemyOverwhelm != 0.0 { 1.0 } else { 0.0 })
                >= 2.0
            {
                // L2063
                bdr().push(lfmt!(
                    "Total %s Damage Reduction: %d%%",
                    damageType,
                    100.0 - reductMult * 100.0
                ));
            }
        }
        // L2066
        let mut takenMult = output.num(cat!(damageType, "TakenHitMult"));
        // L2067
        let mut spellSuppressMult = 1.0;
        // L2068
        if dcc == "Melee" || dcc == "Projectile" {
            // L2069
            takenMult = output.num(cat!(damageType, "AttackTakenHitMult"));
        } else if dcc == "Spell" || dcc == "SpellProjectile" {
            // L2070
            // L2071
            takenMult = output.num(cat!(damageType, "SpellTakenHitMult"));
            // L2072
            spellSuppressMult = if output
                .get(n!("EffectiveSpellSuppressionChance"))
                .raw_eq(&Val::Num(100.0))
            {
                1.0 - output.num(n!("SpellSuppressionEffect")) / 100.0
            } else {
                1.0
            };
        } else if dcc == "Average" {
            // L2073
            // L2074
            takenMult = (output.num(cat!(damageType, "SpellTakenHitMult"))
                + output.num(cat!(damageType, "AttackTakenHitMult")))
                / 2.0;
            // L2075
            spellSuppressMult = if output
                .get(n!("EffectiveSpellSuppressionChance"))
                .raw_eq(&Val::Num(100.0))
            {
                1.0 - output.num(n!("SpellSuppressionEffect")) / 100.0 / 2.0
            } else {
                1.0
            };
        }
        // L2077
        output.set(
            cat!(damageType, "EffectiveAppliedArmour"),
            effectiveAppliedArmour,
        );
        // L2078
        output.set(cat!(damageType, "ResistTakenHitMulti"), resMult);
        // L2079
        let afterReductionMulti = takenMult * spellSuppressMult;
        // L2080
        output.set(
            cat!(damageType, "AfterReductionTakenHitMulti"),
            afterReductionMulti,
        );
        // L2081
        let baseMult = resMult * reductMult;
        // L2082
        output.set(
            cat!(damageType, "BaseTakenHitMult"),
            baseMult * afterReductionMulti,
        );
        // L2083
        let takenMultReflect = output.num(cat!(damageType, "TakenReflect"));
        // L2084
        let finalReflect = baseMult * takenMultReflect;
        // L2085
        output.set(
            cat!(damageType, "TakenHit"),
            m_max(damage * baseMult + takenFlat, 0.0) * takenMult * spellSuppressMult
                + impaleDamage,
        );
        // L2086
        let v = if damage > 0.0 {
            output.num(cat!(damageType, "TakenHit")) / damage
        } else {
            0.0
        };
        output.set(cat!(damageType, "TakenHitMult"), v);
        // L2087
        let v = output.num(n!("totalTakenHit")) + output.num(cat!(damageType, "TakenHit"));
        output.set(n!("totalTakenHit"), v);
        // L2088
        if output.truthy(n!("AnyTakenReflect")) {
            // L2089
            output.set(cat!(damageType, "TakenReflectMult"), finalReflect);
        }
        // L2091
        if let Some(breakdown) = &breakdown {
            // L2092
            breakdown.set(cat!(damageType, "TakenHitMult"), tbl!());
            let bth = || tab(breakdown, cat!(damageType, "TakenHitMult"));
            // L2093
            if resist != 0.0 {
                // L2094
                bth().push(lfmt!("Resistance: %.2f", 1.0 - resist / 100.0));
            }
            // L2096
            if enemyPen != 0.0 {
                // L2097
                bth().push(lfmt!("+ Enemy Pen: %.2f", enemyPen / 100.0));
            }
            // L2099
            if resist != 0.0 && enemyPen != 0.0 {
                // L2100
                bth().push(lfmt!("= %.2f", resMult));
            }
            // L2102
            if reduction != 0.0 {
                // L2103
                bth().push(lfmt!(
                    "Base %s Damage Taken: %.2f",
                    damageType,
                    1.0 - reduction / 100.0
                ));
            }
            // L2105
            if armourReduct != 0.0 {
                // L2106
                if resMult != 1.0 {
                    // L2107
                    bth().push(lfmt!(
                        "Enemy Hit Damage After Resistance: %d ^8(total incoming damage)",
                        damage * resMult
                    ));
                } else {
                    // L2109
                    bth().push(lfmt!(
                        "Enemy Hit Damage: %d ^8(total incoming damage)",
                        damage
                    ));
                }
                // L2111
                if percentOfArmourApplies != 100.0 && !physicalReductionBasedOnWard {
                    // L2112
                    bth().push(lfmt!(
                        "%d%% percent of armour applies",
                        percentOfArmourApplies
                    ));
                }
                // L2114
                bth().push(lfmt!(
                    "Reduction from Armour: %.2f",
                    1.0 - armourReduct / 100.0
                ));
            }
            // L2116
            if enemyOverwhelm != 0.0 {
                // L2117
                bth().push(lfmt!(
                    "+ Enemy Overwhelm %s Damage: %.2f",
                    damageType,
                    enemyOverwhelm / 100.0
                ));
            }
            // L2119
            if reduction != 0.0 || armourReduct != 0.0 || enemyOverwhelm != 0.0 {
                // L2120
                bth().push(lfmt!("= %.2f", reductMult));
            }
            // L2122
            if resMult != 1.0 && reductMult != 1.0 {
                // L2123
                bth().push(lfmt!("%.2f x %.2f = %.3f", resMult, reductMult, baseMult));
            }
            // L2125
            if takenFlat != 0.0 {
                // L2126
                bth().push(lfmt!("+ Flat: %d", takenFlat));
            }
            // L2128
            if takenMult != 1.0 {
                // L2129
                bth().push(lfmt!("x Taken: %.3f", takenMult));
            }
            // L2131
            if spellSuppressMult != 1.0 {
                // L2132
                bth().push(lfmt!("x Spell Suppression: %.3f", spellSuppressMult));
            }
            // L2134
            if impaleDamage != 0.0 {
                // L2135
                bth().push(lfmt!("+ Impale: %.1f", impaleDamage));
            }
            // L2137
            if takenMult != 1.0
                || takenFlat != 0.0
                || spellSuppressMult != 1.0
                || impaleDamage != 0.0
            {
                // L2138
                bth().push(lfmt!(
                    "= %.3f",
                    output.num(cat!(damageType, "TakenHitMult"))
                ));
            }
            // L2140
            if output.truthy(n!("AnyTakenReflect")) {
                // L2141
                breakdown.set(
                    cat!(damageType, "TakenReflectMult"),
                    tbl![lfmt!("Resistance: %.3f", 1.0 - resist / 100.0)],
                );
                let btr = || tab(breakdown, cat!(damageType, "TakenReflectMult"));
                // L2144
                if enemyPen > 0.0 {
                    // L2145
                    btr().push(lfmt!("Enemy Pen: %.2f", enemyPen));
                }
                // L2147
                btr().push(lfmt!("Taken: %.3f", takenMultReflect));
                // L2148
                btr().push(lfmt!("= %.3f", finalReflect));
            }
        }
    }
    Flow::Normal
}
