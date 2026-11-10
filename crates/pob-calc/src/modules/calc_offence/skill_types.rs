//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence segment OF-B (CalcOffence.lua 1037-1691): momentum stacks, the skill type stats
//! (minions, chains, projectiles, melee range, area, aura/reservation/curse/warcry/link/herald
//! effects, traps, cooldown, stored uses, mines, totems, brands, warcries, corpses), the skill
//! durations and the skill uptimes.
//!
//! Owner: OF-B. The callees `calcAreaOfEffect`, `runSkillFunc`, `calcAreaMod`, `calcRadius`,
//! `calcRadiusBreakpoints`, `calcSkillCooldown`, `calcSkillDuration`, `calcWarcryCastTime` and
//! `calcTotemLife` are OF-A's (helpers_setup.rs).
#![allow(non_snake_case)]
// literal Lua statement shapes (`x = x * y`, `local x` then assignment, `not (a or b)`)
#![allow(
    clippy::assign_op_pattern,
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use super::helpers_setup::{
    calcAreaMod, calcAreaOfEffect, calcRadius, calcRadiusBreakpoints, calcSkillCooldown,
    calcSkillDuration, calcTotemLife, calcWarcryCastTime, runSkillFunc,
};
use super::OffenceCtx;
use crate::data_fns::SkillHook;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::Ramp;
use pob_lua::format::FmtArg;

/// @pob Modules/CalcOffence.lua:1037-1691 h=77ee8685
/// OF-B: CalcOffence.lua 1037-1691.
pub(crate) fn seg_skillTypeStats(ctx: &mut OffenceCtx) -> Flow {
    // The ctx fields this segment reads (handles: every write goes through them).
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skill_cfg_rc = ctx.skillCfg.clone();
    let skillCfg: Option<&ModCfg> = Some(&skill_cfg_rc);
    let misc = &data().misc;
    let has_type = |t: SkillType::T| activeSkill.skillTypes.get().with(|s| s.has(t));

    // L1037 momentum stacks
    // L1038
    if skillModList.Flag(None, nm!["SupportedByMomentum"]) {
        // L1039
        let maxMomentumStacks = skillModList.Sum(ModType::BASE, skillCfg, nm!["MomentumStacksMax"]);
        // L1040
        let extraMomentumStacks =
            skillModList.Sum(ModType::BASE, skillCfg, nm!["MomentumStacksExtra"]);
        // L1041
        if maxMomentumStacks > 0.0 {
            // L1042
            if !modDB.HasMod(ModType::BASE, None, nm!["Multiplier:MomentumStacks"]) {
                // L1043
                modDB.NewMod(
                    n!("Multiplier:MomentumStacks"),
                    ModType::BASE,
                    m_min(
                        (maxMomentumStacks + extraMomentumStacks) / 2.0,
                        maxMomentumStacks,
                    ),
                    "Config",
                    0,
                    0,
                    &[pob_gamedata::tag!({ type = Condition, var = "Combat" })],
                );
            } else if modDB.Sum(ModType::BASE, None, nm!["Multiplier:MomentumStacks"])
                > maxMomentumStacks
            {
                // L1044
                // L1045
                modDB.ReplaceMod(
                    n!("Multiplier:MomentumStacks"),
                    ModType::BASE,
                    maxMomentumStacks,
                    "Config",
                    0,
                    0,
                    &[pob_gamedata::tag!({ type = Condition, var = "Combat" })],
                );
            }
        } else if modDB.HasMod(ModType::BASE, None, nm!["Multiplier:MomentumStacks"]) {
            // L1047
            // L1048
            modDB.ReplaceMod(
                n!("Multiplier:MomentumStacks"),
                ModType::BASE,
                0.0,
                "Config",
                0,
                0,
                &[],
            );
        }
    }

    // L1052
    ctx.isAttack = skillFlags.truthy(n!("attack"));

    // L1054
    runSkillFunc(ctx, SkillHook::preSkillTypeFunc);

    // L1056 Calculate skill type stats
    // L1057
    if let Some(minion) = activeSkill.minion.get() {
        // L1058
        let limit = match minion.minionData.get() {
            Some(md) => md.limit,
            None => lua_error!("attempt to index field 'minionData' (a nil value)"),
        };
        if let Some(limit) = limit {
            // L1059
            let ov = Val::from_value(&env.modDB.get().Override(None, &[limit]));
            let v = if ov.truthy() {
                ov.num()
            } else {
                calcLib::val(&skillModList, limit, skillCfg)
                    * skillModList.More(skillCfg, nm!["ActiveMinionLimit"])
            };
            output.set(n!("ActiveMinionLimit"), m_floor(v));
        }
        // L1061
        output.set(
            n!("SummonedMinionsPerCast"),
            m_floor(calcLib::val(
                &skillModList,
                n!("MinionPerCastCount"),
                skillCfg,
            )),
        );
        // L1062
        if output.num(n!("SummonedMinionsPerCast")) == 0.0 {
            // L1063
            output.set(n!("SummonedMinionsPerCast"), 1.0);
        }
    }
    // L1066
    if skillFlags.truthy(n!("chaining")) {
        // L1067
        if skillModList.Flag(skillCfg, nm!["CannotChain"])
            || skillModList.Flag(skillCfg, nm!["NoAdditionalChains"])
        {
            // L1068
            output.set(n!("ChainMaxString"), "Cannot chain");
        } else {
            // L1070
            let v = skillModList.Sum(
                ModType::BASE,
                skillCfg,
                &names([
                    Some(n!("ChainCountMax")),
                    (!skillFlags.truthy(n!("projectile"))).then(|| n!("BeamChainCountMax")),
                ]),
            );
            output.set(n!("ChainMax"), v);
            // L1071
            if skillModList.Flag(skillCfg, nm!["AdditionalProjectilesAddChainsInstead"]) {
                // L1072
                let projCount = if skillModList.Flag(skillCfg, nm!["SingleProjectile"]) {
                    0.0
                } else {
                    m_floor(
                        (skillModList.Sum(ModType::BASE, skillCfg, nm!["ProjectileCount"]) - 1.0)
                            * skillModList.More(skillCfg, nm!["ProjectileCount"]),
                    )
                };
                // L1073
                let v = output.num(n!("ChainMax")) + projCount;
                output.set(n!("ChainMax"), v);
            }
            // L1075
            let v = output.num(n!("ChainMax")) * skillModList.More(skillCfg, nm!["ChainCountMax"]);
            output.set(n!("ChainMax"), v);
            // L1076
            output.set(n!("ChainMaxString"), output.get(n!("ChainMax")));
            // L1077
            let v = m_min(
                output.num(n!("ChainMax")),
                skillModList.Sum(ModType::BASE, skillCfg, nm!["ChainCount"]),
            );
            output.set(n!("Chain"), v);
            // L1078
            let v = m_max(0.0, output.num(n!("ChainMax")) - output.num(n!("Chain")));
            output.set(n!("ChainRemaining"), v);
        }
    }
    // L1081
    if skillFlags.truthy(n!("projectile")) {
        // L1082
        if skillModList.Flag(None, nm!["PointBlank"]) {
            // L1083
            skillModList.NewMod(
                n!("Damage"),
                ModType::MORE,
                30.0,
                "Point Blank",
                ModFlag::Attack | ModFlag::Projectile,
                0,
                &[distance_ramp(&[[10.0, 1.0], [35.0, 0.0], [120.0, -1.0]])],
            );
        }
        // L1085
        if skillModList.Flag(None, nm!["FarShot"]) {
            // L1086
            skillModList.NewMod(
                n!("Damage"),
                ModType::MORE,
                100.0,
                "Far Shot",
                ModFlag::Attack | ModFlag::Projectile,
                0,
                &[distance_ramp(&[[10.0, -0.2], [25.0, 0.0], [70.0, 0.6]])],
            );
        }
        // L1088
        if skillModList.Flag(skillCfg, nm!["NoAdditionalProjectiles"])
            || skillModList.Flag(skillCfg, nm!["SingleProjectile"])
        {
            // L1089
            output.set(n!("ProjectileCount"), 1.0);
        } else {
            // L1091
            let projMin = skillModList.Sum(ModType::BASE, skillCfg, nm!["ProjectileCountMinimum"]);
            // L1092
            let projMax = lor(
                Val::from_value(&skillModList.Override(skillCfg, nm!["ProjectileCountMaximum"])),
                || Val::Num(m_huge),
            );
            // L1093
            let projBase = skillModList.Sum(ModType::BASE, skillCfg, nm!["ProjectileCount"]);
            // L1094
            let projMore = skillModList.More(skillCfg, nm!["ProjectileCount"]);
            // L1095
            let proj = m_floor(projBase * projMore);
            // L1096
            output.set(
                n!("ProjectileCount"),
                m_max(m_min(proj, lua_math_arg(&projMax, 2, "min")), projMin),
            );
        }
        // L1098
        if skillModList.Flag(skillCfg, nm!["AdditionalProjectilesAddBouncesInstead"]) {
            // L1099
            let projBase = if skillModList.Flag(skillCfg, nm!["SingleProjectile"]) {
                0.0
            } else {
                skillModList.Sum(ModType::BASE, skillCfg, nm!["ProjectileCount"])
                    + skillModList.Sum(ModType::BASE, skillCfg, nm!["BounceCount"])
                    - 1.0
            };
            // L1100
            let projMore = skillModList.More(skillCfg, nm!["ProjectileCount"]);
            // L1101
            output.set(n!("BounceCount"), m_floor(projBase * projMore));
        }
        // L1103
        if skillModList.Flag(skillCfg, nm!["CannotSplit"]) || has_type(SkillType::ProjectileNumber)
        {
            // L1104
            if breakdown.is_some() {
                // L1105
                let SplitCount = skillModList.Sum(ModType::BASE, skillCfg, nm!["SplitCount"])
                    + enemyDB.Sum(ModType::BASE, skillCfg, nm!["SelfSplitCount"]);
                // L1106
                if SplitCount > 0.0 {
                    // L1107
                    output.set(n!("SplitCountString"), "Cannot split");
                }
            }
        } else {
            // L1111
            let v = skillModList.Sum(ModType::BASE, skillCfg, nm!["SplitCount"])
                + enemyDB.Sum(ModType::BASE, skillCfg, nm!["SelfSplitCount"]);
            output.set(n!("SplitCount"), v);
            // L1112
            if skillModList.Flag(skillCfg, nm!["AdditionalProjectilesAddSplitsInstead"]) {
                // L1113
                let addedSplits = if skillModList.Flag(skillCfg, nm!["SingleProjectile"]) {
                    0.0
                } else {
                    m_floor(
                        (skillModList.Sum(ModType::BASE, skillCfg, nm!["ProjectileCount"]) - 1.0)
                            * skillModList.More(skillCfg, nm!["ProjectileCount"]),
                    )
                };
                // L1114
                let v = output.num(n!("SplitCount")) + addedSplits;
                output.set(n!("SplitCount"), v);
            }
            // L1116
            if skillModList.Flag(skillCfg, nm!["AdditionalChainsAddSplitsInstead"]) {
                // L1117
                let v = output.num(n!("SplitCount"))
                    + skillModList.Sum(ModType::BASE, skillCfg, nm!["ChainCountMax"]);
                output.set(n!("SplitCount"), v);
            }
            // L1119
            output.set(n!("SplitCountString"), output.get(n!("SplitCount")));
        }
        // L1121
        if skillModList.Flag(skillCfg, nm!["CannotFork"]) {
            // L1122
            output.set(n!("ForkCountString"), "Cannot fork");
        } else if skillModList.Flag(skillCfg, nm!["ForkOnce"]) {
            // L1123
            // L1124
            skillFlags.set(n!("forking"), true);
            // L1125
            if skillModList.Flag(skillCfg, nm!["ForkTwice"]) {
                // L1126
                output.set(
                    n!("ForkCountMax"),
                    m_min(
                        skillModList.Sum(ModType::BASE, skillCfg, nm!["ForkCountMax"]),
                        2.0,
                    ),
                );
            } else {
                // L1128
                output.set(
                    n!("ForkCountMax"),
                    m_min(
                        skillModList.Sum(ModType::BASE, skillCfg, nm!["ForkCountMax"]),
                        1.0,
                    ),
                );
            }
            // L1130
            let v = m_min(
                output.num(n!("ForkCountMax")),
                skillModList.Sum(ModType::BASE, skillCfg, nm!["ForkedCount"]),
            );
            output.set(n!("ForkedCount"), v);
            // L1131
            output.set(n!("ForkCountString"), output.get(n!("ForkCountMax")));
            // L1132
            let v = m_max(
                0.0,
                output.num(n!("ForkCountMax")) - output.num(n!("ForkedCount")),
            );
            output.set(n!("ForkRemaining"), v);
        } else {
            // L1134
            output.set(n!("ForkCountString"), "0");
        }
        // L1136
        if skillModList.Flag(skillCfg, nm!["CannotPierce"]) {
            // L1137
            output.set(n!("PierceCount"), 0.0);
            // L1138
            output.set(n!("PierceCountString"), "Cannot pierce");
        } else {
            // L1140
            if skillModList.Flag(skillCfg, nm!["PierceAllTargets"])
                || enemyDB.Flag(None, nm!["AlwaysPierceSelf"])
            {
                // L1141
                output.set(n!("PierceCount"), 100.0);
                // L1142
                output.set(n!("PierceCountString"), "All targets");
            } else {
                // L1144
                output.set(
                    n!("PierceCount"),
                    skillModList.Sum(ModType::BASE, skillCfg, nm!["PierceCount"]),
                );
                // L1145
                output.set(n!("PierceCountString"), output.get(n!("PierceCount")));
            }
            // L1147
            if output.num(n!("PierceCount")) > 0.0 {
                // L1148
                skillFlags.set(n!("piercing"), true);
            }
            // L1150
            let v = m_min(
                output.num(n!("PierceCount")),
                skillModList.Sum(ModType::BASE, skillCfg, nm!["PiercedCount"]),
            );
            output.set(n!("PiercedCount"), v);
        }
        // L1152
        output.set(
            n!("ProjectileSpeedMod"),
            calcLib::r#mod(&skillModList, skillCfg, nm!["ProjectileSpeed"]),
        );
        // L1153
        if let Some(bd) = &breakdown {
            // L1154
            bd.set(
                n!("ProjectileSpeedMod"),
                bd.r#mod(&skillModList, skillCfg, nm!["ProjectileSpeed"]),
            );
        }
    }
    // L1157
    if skillFlags.truthy(n!("melee")) {
        // L1158
        if skillFlags.truthy(n!("weapon1Attack")) {
            // L1159
            let range = actor.weaponData1.get().get(n!("range"));
            let w1 = activeSkill.weapon1Cfg.get();
            let v = if range.truthy() {
                range.num()
                    + skillModList.Sum(ModType::BASE, w1.as_deref(), nm!["MeleeWeaponRange"])
                    + 10.0
                        * skillModList.Sum(
                            ModType::BASE,
                            w1.as_deref(),
                            nm!["MeleeWeaponRangeMetre"],
                        )
            } else {
                6.0 + skillModList.Sum(ModType::BASE, skillCfg, nm!["UnarmedRange"])
                    + 10.0 * skillModList.Sum(ModType::BASE, skillCfg, nm!["UnarmedRangeMetre"])
            };
            actor.weaponRange1.set(Some(v));
        }
        // L1161
        if skillFlags.truthy(n!("weapon2Attack")) {
            // L1162
            let range = actor.weaponData2.get().get(n!("range"));
            let w2 = activeSkill.weapon2Cfg.get();
            let v = if range.truthy() {
                range.num()
                    + skillModList.Sum(ModType::BASE, w2.as_deref(), nm!["MeleeWeaponRange"])
                    + 10.0
                        * skillModList.Sum(
                            ModType::BASE,
                            w2.as_deref(),
                            nm!["MeleeWeaponRangeMetre"],
                        )
            } else {
                6.0 + skillModList.Sum(ModType::BASE, skillCfg, nm!["UnarmedRange"])
                    + 10.0 * skillModList.Sum(ModType::BASE, skillCfg, nm!["UnarmedRangeMetre"])
            };
            actor.weaponRange2.set(Some(v));
        }
        // L1164
        if has_type(SkillType::MeleeSingleTarget) {
            // L1165
            let mut range = 100.0;
            // L1166
            if skillFlags.truthy(n!("weapon1Attack")) {
                // L1167
                range = m_min(range, opt_math_arg(actor.weaponRange1.get(), 2, "min"));
            }
            // L1169
            if skillFlags.truthy(n!("weapon2Attack")) {
                // L1170
                range = m_min(range, opt_math_arg(actor.weaponRange2.get(), 2, "min"));
            }
            // L1172
            output.set(n!("WeaponRange"), range + 2.0);
            // L1173
            output.set(n!("WeaponRangeMetre"), output.num(n!("WeaponRange")) / 10.0);
            // L1174
            if let Some(bd) = &breakdown {
                // L1175
                bd.set(
                    n!("WeaponRange"),
                    tbl! { radius = output.get(n!("WeaponRange")) },
                );
            }

            // L1180
            let baseStrikeCount = 1.0;
            // L1181
            output.set(
                n!("StrikeTargets"),
                baseStrikeCount
                    + skillModList.Sum(ModType::BASE, skillCfg, nm!["AdditionalStrikeTarget"]),
            );
        }
    }
    // L1184
    if skillFlags.truthy(n!("area"))
        || skillData.truthy(n!("radius"))
        || (skillFlags.truthy(n!("mine")) && has_type(SkillType::Aura))
    {
        // L1185
        calcAreaOfEffect(
            ctx,
            &skillModList,
            &skill_cfg_rc,
            &skillData,
            &skillFlags,
            &output,
            breakdown.as_ref(),
        );
    }
    // L1187
    if has_type(SkillType::Aura) {
        // L1188
        let aura_names = names([
            Some(n!("AuraEffect")),
            (!(skillData.truthy(n!("auraCannotAffectSelf"))
                || has_type(SkillType::AuraAffectsEnemies)))
            .then(|| n!("SkillAuraEffectOnSelf")),
        ]);
        output.set(
            n!("AuraEffectMod"),
            calcLib::r#mod(&skillModList, skillCfg, &aura_names),
        );
        // L1189
        if let Some(bd) = &breakdown {
            // L1190
            let aura_names = names([
                Some(n!("AuraEffect")),
                (!(skillData.truthy(n!("auraCannotAffectSelf"))
                    || has_type(SkillType::AuraAffectsEnemies)))
                .then(|| n!("SkillAuraEffectOnSelf")),
            ]);
            bd.set(
                n!("AuraEffectMod"),
                bd.r#mod(&skillModList, skillCfg, &aura_names),
            );
        }
    }
    // L1193
    if has_type(SkillType::HasReservation) && !has_type(SkillType::ReservationBecomesCost) {
        // L1194 `for _, pool in ipairs({"Life", "Mana"})` over a fresh literal list
        for pool in ["Life", "Mana"] {
            // L1195
            output.set(cat!(pool, "ReservedMod"), 0.0);
            // L1196
            if calcLib::r#mod(&skillModList, skillCfg, nm!["SupportManaMultiplier"]) > 0.0
                && calcLib::r#mod(
                    &skillModList,
                    skillCfg,
                    &[cat!(pool, "Reserved"), n!("Reserved")],
                ) > 0.0
            {
                // L1197
                let v = calcLib::r#mod(
                    &skillModList,
                    skillCfg,
                    &[cat!(pool, "Reserved"), n!("Reserved")],
                ) * floor(
                    calcLib::r#mod(&skillModList, skillCfg, nm!["SupportManaMultiplier"]),
                    Some(4),
                ) / m_max(
                    0.0,
                    calcLib::r#mod(
                        &skillModList,
                        skillCfg,
                        &[
                            cat!(pool, "ReservationEfficiency"),
                            n!("ReservationEfficiency"),
                        ],
                    ),
                );
                output.set(cat!(pool, "ReservedMod"), v);
            }
            // L1199
            if let Some(bd) = &breakdown {
                let res_names = [
                    cat!(pool, "Reserved"),
                    n!("Reserved"),
                    n!("SupportManaMultiplier"),
                ];
                // L1200
                let inc = skillModList.Sum(ModType::INC, skillCfg, &res_names);
                // L1201
                let more = skillModList.More(skillCfg, &res_names);
                // L1202
                if inc != 0.0 && more != 1.0 {
                    // L1203
                    bd.set(
                        cat!(pool, "ReservedMod"),
                        tbl![
                            lfmt!("%.2f ^8(increased/reduced)", 1.0 + inc / 100.0),
                            lfmt!("x %.2f ^8(more/less)", more),
                            lfmt!(
                                "/ %.2f ^8(reservation efficiency)",
                                calcLib::r#mod(
                                    &skillModList,
                                    skillCfg,
                                    &[
                                        cat!(pool, "ReservationEfficiency"),
                                        n!("ReservationEfficiency")
                                    ],
                                )
                            ),
                            lfmt!("= %.2f", fmt_val(&output.get(cat!(pool, "ReservedMod")))),
                        ],
                    );
                }
            }
        }
    }
    // L1213
    if has_type(SkillType::Hex) || has_type(SkillType::Mark) {
        // L1214
        output.set(
            n!("CurseEffectMod"),
            calcLib::r#mod(&skillModList, skillCfg, nm!["CurseEffect"]),
        );
        // L1215
        if let Some(bd) = &breakdown {
            // L1216
            bd.set(
                n!("CurseEffectMod"),
                bd.r#mod(&skillModList, skillCfg, nm!["CurseEffect"]),
            );
        }
    }
    // L1219
    if has_type(SkillType::Warcry) {
        // L1220
        let full_duration = calcSkillDuration(
            &skillModList,
            skillCfg,
            &activeSkill.skillData.get(),
            Some(&env),
            Some(&enemyDB),
        );
        // L1221
        let cooldownOverride =
            Val::from_value(&skillModList.Override(skillCfg, nm!["CooldownRecovery"]));
        // L1222
        let actual_cooldown = lor(cooldownOverride, || {
            Val::Num(
                (activeSkill.skillData.get().num(n!("cooldown"))
                    + skillModList.Sum(ModType::BASE, skillCfg, nm!["CooldownRecovery"]))
                    / calcLib::r#mod(&skillModList, skillCfg, nm!["CooldownRecovery"]),
            )
        });
        // L1223
        let uptime = if env.modDB.get().Flag(None, nm!["Condition:WarcryMaxHit"]) {
            1.0
        } else {
            m_min(full_duration / actual_cooldown.num(), 1.0)
        };
        // L1224
        let unscaledEffect =
            calcLib::r#mod(&skillModList, skillCfg, nm!["WarcryEffect", "BuffEffect"]);
        // L1225
        output.set(n!("WarcryEffectMod"), unscaledEffect * uptime);
        // L1226
        if let Some(bd) = &breakdown {
            // L1227
            bd.set(
                n!("WarcryEffectMod"),
                tbl![lfmt!("%.2f ^8(effect modifiers)", unscaledEffect)],
            );
            // L1230
            if env.modDB.get().Flag(None, nm!["Condition:WarcryMaxHit"]) || uptime != 1.0 {
                // L1231
                if env.modDB.get().Flag(None, nm!["Condition:WarcryMaxHit"]) {
                    // L1232
                    bd_list(bd, n!("WarcryEffectMod"))
                        .push("* 100% uptime (WarcryMaxHit Override)");
                } else if uptime != 1.0 {
                    // L1233
                    // L1234
                    bd_list(bd, n!("WarcryEffectMod"))
                        .push(lfmt!("* %.2f%% ^8(uptime)", uptime * 100.0));
                }
                // L1236
                bd_list(bd, n!("WarcryEffectMod"))
                    .push(lfmt!("= %.1f%%", output.num(n!("WarcryEffectMod")) * 100.0));
            }
        }
    }
    // L1240
    if has_type(SkillType::Link) {
        // L1241
        output.set(
            n!("LinkEffectMod"),
            calcLib::r#mod(&skillModList, skillCfg, nm!["LinkEffect", "BuffEffect"]),
        );
        // L1242
        if let Some(bd) = &breakdown {
            // L1243
            bd.set(
                n!("LinkEffectMod"),
                bd.r#mod(&skillModList, skillCfg, nm!["LinkEffect", "BuffEffect"]),
            );
        }
    }
    // L1246
    if has_type(SkillType::Buff) && has_type(SkillType::Herald) {
        // L1247
        output.set(
            n!("HeraldBuffEffectMod"),
            calcLib::r#mod(
                &skillModList,
                skillCfg,
                nm!["BuffEffect", "BuffEffectOnSelf"],
            ),
        );
        // L1248
        if let Some(bd) = &breakdown {
            // L1249 (the player's modDB, as in the Lua)
            bd.set(
                n!("HeraldBuffEffectMod"),
                bd.r#mod(
                    &env.modDB.get(),
                    skillCfg,
                    nm!["BuffEffect", "BuffEffectOnSelf"],
                ),
            );
        }
    }
    // L1252
    if (skillFlags.truthy(n!("trap")) || skillFlags.truthy(n!("mine")))
        && !(skillData.truthy(n!("trapCooldown")) || skillData.truthy(n!("cooldown")))
    {
        // L1253
        skillFlags.set(n!("notAverage"), true);
        // L1254
        skillFlags.set(n!("showAverage"), false);
        // L1255
        skillData.set(n!("showAverage"), false);
    }
    // L1257
    if skillFlags.truthy(n!("trap")) {
        // L1258
        let mut baseSpeed =
            1.0 / skillModList.Sum(ModType::BASE, skillCfg, nm!["TrapThrowingTime"]);
        // L1259
        let timeMod = calcLib::r#mod(&skillModList, skillCfg, nm!["SkillTrapThrowingTime"]);
        // L1260
        if timeMod > 0.0 {
            // L1261
            baseSpeed = baseSpeed * (1.0 / timeMod);
        }
        // L1263
        output.set(
            n!("TrapThrowingSpeed"),
            baseSpeed
                * calcLib::r#mod(&skillModList, skillCfg, nm!["TrapThrowingSpeed"])
                * output.num(n!("ActionSpeedMod")),
        );
        // L1264
        let mut trapThrowCount = calcLib::val(&skillModList, n!("TrapThrowCount"), skillCfg);
        // L1265
        if skillData.truthy(n!("trapCooldown")) || skillData.truthy(n!("cooldown")) {
            // L1266
            trapThrowCount = 1.0;
        }
        // L1268
        output.set(
            n!("TrapThrowCount"),
            lor(
                Val::from_value(&env.modDB.get().Override(None, nm!["TrapThrowCount"])),
                || Val::Num(trapThrowCount),
            ),
        );
        // L1269
        output.set(
            n!("TrapThrowingSpeed"),
            m_min(output.num(n!("TrapThrowingSpeed")), misc.ServerTickRate),
        );
        // L1270
        output.set(
            n!("TrapThrowingTime"),
            1.0 / output.num(n!("TrapThrowingSpeed")),
        );
        // L1271
        skillData.set(
            n!("timeOverride"),
            output.num(n!("TrapThrowingTime")) / output.num(n!("TrapThrowCount")),
        );
        // L1272
        if let Some(bd) = &breakdown {
            // L1273
            bd.set(n!("TrapThrowingSpeed"), Table::new());
            // L1274
            let chain = tbl![
                tbl![
                    "%.2f ^8(increased/reduced throwing speed)",
                    1.0 + skillModList.Sum(ModType::INC, skillCfg, nm!["TrapThrowingSpeed"]) / 100.0
                ],
                tbl![
                    "%.2f ^8(more/less throwing speed)",
                    skillModList.More(skillCfg, nm!["TrapThrowingSpeed"])
                ],
                tbl!["%.2f ^8(action speed modifier)", output.get(n!("ActionSpeedMod"))];
                label = "Throwing rate:",
                base = tbl!["%.2f ^8(base throwing rate)", baseSpeed],
                total = lfmt!("= %.2f ^8per second", fmt_val(&output.get(n!("TrapThrowingSpeed")))),
            ];
            bd.multiChain(&bd_list(bd, n!("TrapThrowingSpeed")), &chain);
        }
        // L1283
        if let Some(bd) = breakdown.as_ref().filter(|_| timeMod > 0.0) {
            // L1284
            bd.set(n!("TrapThrowingTime"), Table::new());
            // L1285
            let chain = tbl![
                tbl!["%.2f ^8(total modifier)", timeMod];
                label = "Throwing time:",
                base = tbl![
                    "%.2f ^8(base throwing time)",
                    1.0 / (output.num(n!("TrapThrowingSpeed")) * timeMod)
                ],
                total = lfmt!(
                    "= %.2f ^8seconds per throw",
                    fmt_val(&output.get(n!("TrapThrowingTime")))
                ),
            ];
            bd.multiChain(&bd_list(bd, n!("TrapThrowingTime")), &chain);
        }

        // L1293
        let baseCooldown = lor(skillData.get(n!("trapCooldown")), || {
            skillData.get(n!("cooldown"))
        });
        // L1294
        if baseCooldown.truthy()
            || skillModList.Sum(ModType::BASE, skillCfg, nm!["CooldownRecovery"]) != 0.0
        {
            // L1295
            if baseCooldown.truthy() {
                // L1296
                output.set(
                    n!("TrapCooldown"),
                    baseCooldown.num()
                        / calcLib::r#mod(&skillModList, skillCfg, nm!["CooldownRecovery"]),
                );
                // L1297
                output.set(
                    n!("TrapCooldown"),
                    m_ceil(output.num(n!("TrapCooldown")) * misc.ServerTickRate)
                        / misc.ServerTickRate,
                );
            } else {
                // L1298 Assign Trap Cooldown if the trap/skill does not have cooldown but gain cooldown elsewhere
                // L1299
                let (cooldown, _, _) = calcSkillCooldown(&skillModList, skillCfg, &skillData);
                // L1300
                output.set(n!("TrapCooldown"), cooldown);
            }
            // L1302
            if let Some(bd) = &breakdown {
                // L1303
                let base = lor(
                    lor(skillData.get(n!("trapCooldown")), || {
                        skillData.get(n!("cooldown"))
                    }),
                    || Val::Num(4.0),
                );
                bd.set(
                    n!("TrapCooldown"),
                    tbl![
                        lfmt!("%.2fs ^8(base)", fmt_val(&base)),
                        lfmt!(
                            "/ %.2f ^8(increased/reduced cooldown recovery)",
                            1.0 + skillModList.Sum(ModType::INC, skillCfg, nm!["CooldownRecovery"])
                                / 100.0
                        ),
                        "rounded up to nearest server tick",
                        lfmt!("= %.3fs", fmt_val(&output.get(n!("TrapCooldown")))),
                    ],
                );
            }
        }
        // L1311
        let (incArea, moreArea) =
            calcLib::mods(&skillModList, skillCfg, nm!["TrapTriggerAreaOfEffect"]);
        // L1312
        let areaMod = calcAreaMod(incArea, moreArea);
        // L1313
        output.set(
            n!("TrapTriggerRadius"),
            calcRadius(misc.TrapTriggerRadiusBase, areaMod, None),
        );
        // L1314
        output.set(
            n!("TrapTriggerRadiusMetre"),
            output.num(n!("TrapTriggerRadius")) / 10.0,
        );
        // L1315
        if let Some(bd) = &breakdown {
            // L1316
            let (incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint) =
                radius_breakpoints(misc.TrapTriggerRadiusBase, incArea, moreArea);
            // L1317
            bd.set(
                n!("TrapTriggerRadius"),
                bd.area(
                    misc.TrapTriggerRadiusBase,
                    areaMod,
                    output.num(n!("TrapTriggerRadius")),
                    incAreaBreakpoint,
                    moreAreaBreakpoint,
                    redAreaBreakpoint,
                    lessAreaBreakpoint,
                    None,
                    None,
                ),
            );
        }
    } else if skillData.truthy(n!("cooldown"))
        || skillModList.Sum(ModType::BASE, skillCfg, nm!["CooldownRecovery"]) != 0.0
    {
        // L1319
        // L1320
        let (cooldown, rounded, addedCooldown) =
            calcSkillCooldown(&skillModList, skillCfg, &skillData);
        // L1321
        output.set(n!("Cooldown"), cooldown);
        // L1322
        if let Some(bd) = &breakdown {
            // L1323 `skillData.cooldown or 0 + addedCooldown`
            let base = lor(skillData.get(n!("cooldown")), || {
                Val::Num(0.0 + opt_arith(addedCooldown, "addedCooldown"))
            });
            bd.set(
                n!("Cooldown"),
                tbl![
                    lfmt!("%.2fs ^8(base)", fmt_val(&base)),
                    lfmt!(
                        "/ %.2f ^8(increased/reduced cooldown recovery)",
                        1.0 + skillModList.Sum(ModType::INC, skillCfg, nm!["CooldownRecovery"])
                            / 100.0
                    ),
                ],
            );
            // L1327
            if rounded {
                // L1328
                bd_list(bd, n!("Cooldown")).push(lfmt!("rounded up to nearest server tick"));
            }
            // L1330
            bd_list(bd, n!("Cooldown"))
                .push(lfmt!("= %.3fs", fmt_val(&output.get(n!("Cooldown")))));
        }
    }
    // L1333
    if skillData.truthy(n!("storedUses")) {
        // L1334
        let baseUses = skillData.num(n!("storedUses"));
        // L1335
        let additionalUses = skillModList.Sum(
            ModType::BASE,
            skillCfg,
            nm!["AdditionalCooldownUses", "AdditionalUses"],
        );
        // L1336
        output.set(n!("StoredUses"), baseUses + additionalUses);
        // L1337
        if let Some(bd) = &breakdown {
            // L1338
            bd.set(
                n!("StoredUses"),
                tbl![lfmt!(
                    "%d ^8(skill use%s)",
                    baseUses,
                    if baseUses == 1.0 { "" } else { "s" }
                )],
            );
            // L1339
            if additionalUses != 0.0 {
                // L1340
                bd_list(bd, n!("StoredUses")).push(lfmt!(
                    "+ %d ^8(additional use%s)",
                    additionalUses,
                    if additionalUses == 1.0 { "" } else { "s" }
                ));
                // L1341
                let total = output.get(n!("StoredUses"));
                bd_list(bd, n!("StoredUses")).push(lfmt!(
                    "= %d ^8(total use%s)",
                    fmt_val(&total),
                    if total.raw_eq(&Val::Num(1.0)) {
                        ""
                    } else {
                        "s"
                    }
                ));
            }
        }
    }
    // L1345
    if skillFlags.truthy(n!("mine")) {
        // L1346
        let mut baseSpeed = 1.0 / skillModList.Sum(ModType::BASE, skillCfg, nm!["MineLayingTime"]);
        // L1347
        let timeMod = calcLib::r#mod(&skillModList, skillCfg, nm!["SkillMineThrowingTime"]);
        // L1348
        if timeMod > 0.0 {
            // L1349
            baseSpeed = baseSpeed * (1.0 / timeMod);
        }
        // L1351
        output.set(
            n!("MineLayingSpeed"),
            baseSpeed
                * calcLib::r#mod(&skillModList, skillCfg, nm!["MineLayingSpeed"])
                * output.num(n!("ActionSpeedMod")),
        );
        // L1352 Calculate additional mine throw
        // L1353
        let mut mineThrowCount = calcLib::val(&skillModList, n!("MineThrowCount"), skillCfg);
        // L1354
        if skillData.truthy(n!("trapCooldown")) || skillData.truthy(n!("cooldown")) {
            // L1355
            mineThrowCount = 1.0;
        }
        // L1357
        output.set(
            n!("MineThrowCount"),
            lor(
                Val::from_value(&env.modDB.get().Override(None, nm!["MineThrowCount"])),
                || Val::Num(mineThrowCount),
            ),
        );
        // L1358
        if output.num(n!("MineThrowCount")) >= 1.0 {
            // L1359 Throwing Mines takes 10% more time for each *additional* Mine thrown
            // L1360
            output.set(
                n!("MineLayingSpeed"),
                output.num(n!("MineLayingSpeed"))
                    / (1.0 + (output.num(n!("MineThrowCount")) - 1.0) * 0.1),
            );
        }

        // L1363
        output.set(
            n!("MineLayingSpeed"),
            m_min(output.num(n!("MineLayingSpeed")), misc.ServerTickRate),
        );
        // L1364
        output.set(
            n!("MineLayingTime"),
            1.0 / output.num(n!("MineLayingSpeed")),
        );

        // L1366 Trap mine interaction where the Character throws mines, mine throws traps
        // L1367
        if skillFlags.truthy(n!("trap")) {
            // L1368
            skillData.set(
                n!("timeOverride"),
                output.num(n!("MineLayingTime"))
                    / output.num(n!("MineThrowCount"))
                    / output.num(n!("TrapThrowCount")),
            );
        } else {
            // L1370
            skillData.set(
                n!("timeOverride"),
                output.num(n!("MineLayingTime")) / output.num(n!("MineThrowCount")),
            );
        }

        // L1373
        if let Some(bd) = &breakdown {
            // L1374
            bd.set(n!("MineLayingTime"), Table::new());
            // L1375
            let chain = tbl![
                tbl![
                    "%.2f ^8(increased/reduced throwing speed)",
                    1.0 + skillModList.Sum(ModType::INC, skillCfg, nm!["MineLayingSpeed"]) / 100.0
                ],
                tbl![
                    "%.2f ^8(more/less throwing speed)",
                    skillModList.More(skillCfg, nm!["MineLayingSpeed"])
                ],
                tbl!["%.2f ^8(action speed modifier)", output.get(n!("ActionSpeedMod"))],
                tbl![
                    "%.2f ^8(additional mine thrown)",
                    1.0 / (1.0 + (output.num(n!("MineThrowCount")) - 1.0) * 0.1)
                ];
                label = "Throwing rate:",
                base = tbl!["%.2f ^8(base throwing rate)", baseSpeed],
                total = lfmt!("= %.2f ^8per second", fmt_val(&output.get(n!("MineLayingSpeed")))),
            ];
            bd.multiChain(&bd_list(bd, n!("MineLayingTime")), &chain);
        }
        // L1385
        if let Some(bd) = breakdown.as_ref().filter(|_| timeMod > 0.0) {
            // L1386
            bd.set(n!("MineThrowingTime"), Table::new());
            // L1387
            let chain = tbl![
                tbl!["%.2f ^8(total modifier)", timeMod];
                label = "Throwing time:",
                base = tbl![
                    "%.2f ^8(base throwing time)",
                    1.0 / (output.num(n!("MineLayingSpeed")) * timeMod)
                ],
                total = lfmt!(
                    "= %.2f ^8seconds per throw",
                    fmt_val(&output.get(n!("MineLayingTime")))
                ),
            ];
            bd.multiChain(&bd_list(bd, n!("MineThrowingTime")), &chain);
        }

        // L1395
        let (incArea, moreArea) =
            calcLib::mods(&skillModList, skillCfg, nm!["MineDetonationAreaOfEffect"]);
        // L1396
        let areaMod = calcAreaMod(incArea, moreArea);
        // L1397
        output.set(
            n!("MineDetonationRadius"),
            calcRadius(misc.MineDetonationRadiusBase, areaMod, None),
        );
        // L1398
        output.set(
            n!("MineDetonationRadiusMetre"),
            output.num(n!("MineDetonationRadius")) / 10.0,
        );
        // L1399
        if let Some(bd) = &breakdown {
            // L1400
            let (incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint) =
                radius_breakpoints(misc.MineDetonationRadiusBase, incArea, moreArea);
            // L1401
            bd.set(
                n!("MineDetonationRadius"),
                bd.area(
                    misc.MineDetonationRadiusBase,
                    areaMod,
                    output.num(n!("MineDetonationRadius")),
                    incAreaBreakpoint,
                    moreAreaBreakpoint,
                    redAreaBreakpoint,
                    lessAreaBreakpoint,
                    None,
                    None,
                ),
            );
        }
        // L1403
        if has_type(SkillType::Aura) {
            // L1404
            output.set(
                n!("MineAuraRadius"),
                calcRadius(
                    misc.MineAuraRadiusBase,
                    output.num(n!("AreaOfEffectMod")),
                    None,
                ),
            );
            // L1405
            output.set(
                n!("MineAuraRadiusMetre"),
                output.num(n!("MineAuraRadius")) / 10.0,
            );
            // L1406
            if let Some(bd) = &breakdown {
                // L1407
                let (incArea, moreArea) =
                    calcLib::mods(&skillModList, skillCfg, nm!["AreaOfEffect"]);
                // L1408
                let (incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint) =
                    radius_breakpoints(misc.MineAuraRadiusBase, incArea, moreArea);
                // L1409
                bd.set(
                    n!("MineAuraRadius"),
                    bd.area(
                        misc.MineAuraRadiusBase,
                        output.num(n!("AreaOfEffectMod")),
                        output.num(n!("MineAuraRadius")),
                        incAreaBreakpoint,
                        moreAreaBreakpoint,
                        redAreaBreakpoint,
                        lessAreaBreakpoint,
                        None,
                        None,
                    ),
                );
            }
        }
    }
    // L1413
    if skillFlags.truthy(n!("totem")) {
        // L1414
        let baseSpeed;
        // L1415
        if skillFlags.truthy(n!("ballista")) {
            // L1416
            baseSpeed =
                1.0 / skillModList.Sum(ModType::BASE, skillCfg, nm!["BallistaPlacementTime"]);
        } else {
            // L1418
            baseSpeed = 1.0 / skillModList.Sum(ModType::BASE, skillCfg, nm!["TotemPlacementTime"]);
        }
        // L1420
        output.set(
            n!("TotemPlacementSpeed"),
            baseSpeed
                * calcLib::r#mod(&skillModList, skillCfg, nm!["TotemPlacementSpeed"])
                * output.num(n!("ActionSpeedMod")),
        );
        // L1421
        output.set(
            n!("TotemPlacementTime"),
            1.0 / output.num(n!("TotemPlacementSpeed")),
        );
        // L1422
        if let Some(bd) = &breakdown {
            // L1423
            bd.set(n!("TotemPlacementTime"), Table::new());
            // L1424
            let chain = tbl![
                tbl![
                    "%.2f ^8(increased/reduced placement speed)",
                    1.0 + skillModList.Sum(ModType::INC, skillCfg, nm!["TotemPlacementSpeed"])
                        / 100.0
                ],
                tbl![
                    "%.2f ^8(more/less placement speed)",
                    skillModList.More(skillCfg, nm!["TotemPlacementSpeed"])
                ],
                tbl!["%.2f ^8(action speed modifier)", output.get(n!("ActionSpeedMod"))];
                label = "Placement speed:",
                base = tbl!["%.2f ^8(base placement speed)", baseSpeed],
                total = lfmt!(
                    "= %.2f ^8per second",
                    fmt_val(&output.get(n!("TotemPlacementSpeed")))
                ),
            ];
            bd.multiChain(&bd_list(bd, n!("TotemPlacementTime")), &chain);
        }
        // L1433
        output.set(
            n!("ActiveTotemLimit"),
            skillModList.Sum(
                ModType::BASE,
                skillCfg,
                nm!["ActiveTotemLimit", "ActiveBallistaLimit"],
            ),
        );
        // L1434
        output.set(
            n!("TotemsSummoned"),
            lor(
                Val::from_value(&env.modDB.get().Override(None, nm!["TotemsSummoned"])),
                || output.get(n!("ActiveTotemLimit")),
            ),
        );
        // L1435
        if let Some(bd) = &breakdown {
            // L1436
            let line = format!(
                "Totems Summoned: {}{}",
                concat_val(&output.get(n!("TotemsSummoned"))),
                if env.configInput.truthy(n!("TotemsSummoned")) {
                    " ^8(overridden from the Configuration tab)"
                } else {
                    " ^8(can be overridden in the Configuration tab)"
                }
            );
            bd.set(n!("ActiveTotemLimit"), tbl![line]);
        }
        // L1440
        let (life, lifeMod) = calcTotemLife(&env, &activeSkill);
        output.set(n!("TotemLife"), life);
        output.set(n!("TotemLifeMod"), lifeMod);
        // L1441
        output.set(
            n!("TotemEnergyShield"),
            skillModList.Sum(ModType::BASE, skillCfg, nm!["TotemEnergyShield"]),
        );
        // L1442
        output.set(
            n!("TotemBlockChance"),
            skillModList.Sum(ModType::BASE, skillCfg, nm!["TotemBlockChance"]),
        );
        // L1443
        output.set(
            n!("TotemArmour"),
            skillModList.Sum(ModType::BASE, skillCfg, nm!["TotemArmour"]),
        );
        // L1444
        if let Some(bd) = &breakdown {
            // L1445
            bd.set(
                n!("TotemLifeMod"),
                bd.r#mod(&skillModList, skillCfg, nm!["TotemLife"]),
            );
            // L1446
            let totemLevel = skillData.get(n!("totemLevel"));
            let ally_life = Val::from(lua_index_f64s(&env.data.monsterAllyLifeTable, &totemLevel));
            let totem_mult = Val::from(
                activeSkill
                    .skillTotemId
                    .get()
                    .and_then(|id| lua_index_int_map(&env.data.totemLifeMult, id)),
            );
            bd.set(
                n!("TotemLife"),
                tbl![
                    format!("Totem level: {}", concat_val(&totemLevel)),
                    format!(
                        "{} ^8(base life for a level {} monster)",
                        concat_val(&ally_life),
                        concat_val(&totemLevel)
                    ),
                    format!(
                        "x {} ^8(life multiplier for this totem type)",
                        concat_val(&totem_mult)
                    ),
                    format!(
                        "x {} ^8(totem life modifier)",
                        concat_val(&output.get(n!("TotemLifeMod")))
                    ),
                    format!("= {}", concat_val(&output.get(n!("TotemLife")))),
                ],
            );
            // L1453
            bd.set(
                n!("TotemEnergyShield"),
                bd.r#mod(&skillModList, skillCfg, nm!["TotemEnergyShield"]),
            );
            // L1454
            bd.set(
                n!("TotemBlockChance"),
                bd.r#mod(&skillModList, skillCfg, nm!["TotemBlockChance"]),
            );
            // L1455
            bd.set(
                n!("TotemArmour"),
                bd.r#mod(&skillModList, skillCfg, nm!["TotemArmour"]),
            );
        }
    }
    // L1458
    if has_type(SkillType::Brand) {
        // L1459
        output.set(
            n!("BrandAttachmentRange"),
            misc.BrandAttachmentRangeBase
                * calcLib::r#mod(&skillModList, skillCfg, nm!["BrandAttachmentRange"]),
        );
        // L1460
        output.set(
            n!("BrandAttachmentRangeMetre"),
            output.num(n!("BrandAttachmentRange")) / 10.0,
        );
        // L1461
        output.set(
            n!("ActiveBrandLimit"),
            skillModList.Sum(ModType::BASE, skillCfg, nm!["ActiveBrandLimit"]),
        );
        // L1462
        output.set(
            n!("AttachedBrandCount"),
            skillData.get(n!("attachedBrandCount")),
        );
        // L1463
        if let Some(bd) = &breakdown {
            // L1464
            bd.set(
                n!("BrandAttachmentRange"),
                tbl! { radius = output.get(n!("BrandAttachmentRange")) },
            );
        }
    }

    // L1468
    if skillFlags.truthy(n!("warcry")) {
        // L1469
        output.set(
            n!("WarcryCastTime"),
            calcWarcryCastTime(&skillModList, skillCfg, &skillData, &actor),
        );
    }

    // L1472
    if skillFlags.truthy(n!("corpse")) {
        // L1473
        output.set(
            n!("CorpseLevel"),
            skillModList.Sum(ModType::BASE, skillCfg, nm!["CorpseLevel"]),
        );
        // L1474
        let corpse_level = lor(output.get(n!("CorpseLevel")), || Val::Num(1.0));
        let variety = skillData.get(n!("corpseMonsterVariety"));
        let base_life = arith_val(
            &Val::from(lua_index_f64s(&env.data.monsterLifeTable, &corpse_level)),
            "field '?'",
        );
        output.set(
            n!("BaseCorpseLife"),
            base_life
                * lua_index_variety(&env.data.monsterVarietyLifeMult, &variety).unwrap_or(1.0)
                * lua_index_int_map(&env.data.mapLevelLifeMult, env.enemyLevel.get())
                    .unwrap_or(1.0),
        );
        // L1475 (`Sum` never returns nil, so `or 0` never applies)
        output.set(
            n!("CorpseLifeInc"),
            1.0 + skillModList.Sum(ModType::INC, skillCfg, nm!["CorpseLife"]) / 100.0,
        );
        // L1476
        output.set(
            n!("CorpseLife"),
            output.num(n!("BaseCorpseLife")) * output.num(n!("CorpseLifeInc")),
        );
        // L1477
        if let Some(bd) = &breakdown {
            // L1478
            let corpse_level = lor(output.get(n!("CorpseLevel")), || Val::Num(1.0));
            let level_or_na = lor(output.get(n!("CorpseLevel")), || Val::from("n/a"));
            let variety_mult =
                lua_index_variety(&env.data.monsterVarietyLifeMult, &variety).unwrap_or(1.0);
            let map_mult =
                lua_index_int_map(&env.data.mapLevelLifeMult, env.enemyLevel.get()).unwrap_or(1.0);
            bd.set(
                n!("CorpseLife"),
                tbl![
                    lfmt!(
                        "%d ^8(base life of a level %d monster)",
                        fmt_val(&Val::from(lua_index_f64s(
                            &env.data.monsterLifeTable,
                            &corpse_level
                        ))),
                        fmt_val(&level_or_na)
                    ),
                    lfmt!(
                        "x %.2f ^8(%s variety multiplier)",
                        variety_mult,
                        fmt_val(&variety)
                    ),
                    lfmt!(
                        "x %.2f ^8(map level %d monster life multiplier from config)",
                        map_mult,
                        env.enemyLevel.get()
                    ),
                    lfmt!(
                        " = %d ^8(base corpse life)",
                        fmt_val(&output.get(n!("BaseCorpseLife")))
                    ),
                    lfmt!(""),
                    lfmt!(
                        "x %.2f ^8(corpse maximum life increases)",
                        fmt_val(&output.get(n!("CorpseLifeInc")))
                    ),
                    lfmt!(" = %d", fmt_val(&output.get(n!("CorpseLife")))),
                ],
            );
        }
    }

    // L1490 Skill duration
    // L1491
    ctx.debuffDurationMult = 1.0;
    // L1492
    if env.mode_effective.get() {
        // L1493
        ctx.debuffDurationMult = 1.0
            / m_max(
                misc.BuffExpirationSlowCap,
                calcLib::r#mod(&enemyDB, skillCfg, nm!["BuffExpireFaster"]),
            );
    }
    let debuffDurationMult = ctx.debuffDurationMult;
    // L1495
    {
        let mine_names = |a: Name, b: Option<Name>| {
            names([
                Some(a),
                b,
                skillData
                    .truthy(n!("mineDurationAppliesToSkill"))
                    .then(|| n!("MineDuration")),
            ])
        };
        // L1496
        output.set(
            n!("DurationMod"),
            calcLib::r#mod(
                &skillModList,
                skillCfg,
                &mine_names(n!("Duration"), Some(n!("PrimaryDuration"))),
            ),
        );
        // L1497
        output.set(n!("DurationMod"), m_max(output.num(n!("DurationMod")), 0.0));
        // L1498
        if let Some(bd) = &breakdown {
            // L1499
            bd.set(
                n!("DurationMod"),
                bd.r#mod(
                    &skillModList,
                    skillCfg,
                    &mine_names(n!("Duration"), Some(n!("PrimaryDuration"))),
                ),
            );
            // L1500
            if bd.truthy(n!("DurationMod")) && skillData.truthy(n!("durationSecondary")) {
                // L1501
                bd_list(bd, n!("DurationMod")).insert(1, "Primary duration:");
            }
        }
        // L1504
        let mut durationBase = skillData.num0(n!("duration"))
            + skillModList.Sum(ModType::BASE, skillCfg, nm!["Duration", "PrimaryDuration"]);
        // L1505
        if durationBase > 0.0
            && !(match activeSkill.minion.get() {
                Some(minion) => skillModList.Flag(
                    skillCfg,
                    &[cat!(
                        concat_val(&minion.r#type.get().map_or(Val::Nil, |t| Val::from(t.as_str()))),
                        "PermanentDuration"
                    )],
                ),
                None => false,
            })
        {
            // L1506
            output.set(n!("Duration"), durationBase * output.num(n!("DurationMod")));
            // L1507
            if skillData.truthy(n!("debuff")) {
                // L1508
                output.set(
                    n!("Duration"),
                    output.num(n!("Duration")) * debuffDurationMult,
                );
            }
            // L1510
            output.set(
                n!("Duration"),
                m_ceil(output.num(n!("Duration")) * misc.ServerTickRate) / misc.ServerTickRate,
            );
            // L1511
            if let Some(bd) = breakdown
                .as_ref()
                .filter(|_| output.num(n!("Duration")) != durationBase)
            {
                // L1512
                bd.set(n!("Duration"), tbl![lfmt!("%.2fs ^8(base)", durationBase)]);
                // L1515
                if output.num(n!("DurationMod")) != 1.0 {
                    // L1516
                    bd_list(bd, n!("Duration")).push(lfmt!(
                        "x %.4f ^8(duration modifier)",
                        output.num(n!("DurationMod"))
                    ));
                }
                // L1518
                if skillData.truthy(n!("debuff")) && debuffDurationMult != 1.0 {
                    // L1519
                    bd_list(bd, n!("Duration")).push(lfmt!(
                        "/ %.3f ^8(debuff expires slower/faster)",
                        1.0 / debuffDurationMult
                    ));
                }
                // L1521
                bd_list(bd, n!("Duration")).push(lfmt!("rounded up to nearest server tick"));
                // L1522
                bd_list(bd, n!("Duration")).push(lfmt!("= %.3fs", output.num(n!("Duration"))));
            }
        }
        // L1525
        durationBase = skillData.num0(n!("durationSecondary"))
            + skillModList.Sum(
                ModType::BASE,
                skillCfg,
                nm!["Duration", "SecondaryDuration"],
            );
        // L1526
        if durationBase > 0.0 {
            // L1527
            let mut durationMod = calcLib::r#mod(
                &skillModList,
                skillCfg,
                &mine_names(n!("Duration"), Some(n!("SecondaryDuration"))),
            );
            // L1528
            durationMod = m_max(durationMod, 0.0);
            // L1529
            output.set(n!("DurationSecondary"), durationBase * durationMod);
            // L1530
            if skillData.truthy(n!("debuffSecondary")) {
                // L1531
                output.set(
                    n!("DurationSecondary"),
                    output.num(n!("DurationSecondary")) * debuffDurationMult,
                );
            }
            // L1533
            output.set(
                n!("DurationSecondary"),
                m_ceil(output.num(n!("DurationSecondary")) * misc.ServerTickRate)
                    / misc.ServerTickRate,
            );
            // L1534
            if let Some(bd) = breakdown
                .as_ref()
                .filter(|_| output.num(n!("DurationSecondary")) != durationBase)
            {
                // L1535
                bd.set(
                    n!("SecondaryDurationMod"),
                    bd.r#mod(
                        &skillModList,
                        skillCfg,
                        &mine_names(n!("Duration"), Some(n!("SecondaryDuration"))),
                    ),
                );
                // L1536
                if bd.truthy(n!("SecondaryDurationMod")) {
                    // L1537
                    bd_list(bd, n!("SecondaryDurationMod")).insert(1, "Secondary duration:");
                }
                // L1539
                bd.set(
                    n!("DurationSecondary"),
                    tbl![lfmt!("%.2fs ^8(base)", durationBase)],
                );
                // L1542 (tests output.DurationMod, as in the Lua)
                if output.num(n!("DurationMod")) != 1.0 {
                    // L1543
                    bd_list(bd, n!("DurationSecondary"))
                        .push(lfmt!("x %.4f ^8(duration modifier)", durationMod));
                }
                // L1545
                if skillData.truthy(n!("debuffSecondary")) && debuffDurationMult != 1.0 {
                    // L1546
                    bd_list(bd, n!("DurationSecondary")).push(lfmt!(
                        "/ %.3f ^8(debuff expires slower/faster)",
                        1.0 / debuffDurationMult
                    ));
                }
                // L1548
                bd_list(bd, n!("DurationSecondary"))
                    .push(lfmt!("rounded up to nearest server tick"));
                // L1549
                bd_list(bd, n!("DurationSecondary"))
                    .push(lfmt!("= %.3fs", output.num(n!("DurationSecondary"))));
            }
        }
        // L1552
        durationBase = skillData.num0(n!("durationTertiary"))
            + skillModList.Sum(ModType::BASE, skillCfg, nm!["Duration", "TertiaryDuration"]);
        // L1553
        if durationBase > 0.0 {
            // L1554
            let mut durationMod = calcLib::r#mod(
                &skillModList,
                skillCfg,
                &mine_names(n!("Duration"), Some(n!("TertiaryDuration"))),
            );
            // L1555
            durationMod = m_max(durationMod, 0.0);
            // L1556
            output.set(n!("DurationTertiary"), durationBase * durationMod);
            // L1557
            if skillData.truthy(n!("debuffTertiary")) {
                // L1558
                output.set(
                    n!("DurationTertiary"),
                    output.num(n!("DurationTertiary")) * debuffDurationMult,
                );
            }
            // L1560
            output.set(
                n!("DurationTertiary"),
                m_ceil(output.num(n!("DurationTertiary")) * misc.ServerTickRate)
                    / misc.ServerTickRate,
            );
            // L1561
            if let Some(bd) = breakdown
                .as_ref()
                .filter(|_| output.num(n!("DurationTertiary")) != durationBase)
            {
                // L1562
                bd.set(
                    n!("TertiaryDurationMod"),
                    bd.r#mod(
                        &skillModList,
                        skillCfg,
                        &mine_names(n!("Duration"), Some(n!("TertiaryDuration"))),
                    ),
                );
                // L1563
                if bd.truthy(n!("TertiaryDurationMod")) {
                    // L1564
                    bd_list(bd, n!("TertiaryDurationMod")).insert(1, "Tertiary duration:");
                }
                // L1566
                bd.set(
                    n!("DurationTertiary"),
                    tbl![lfmt!("%.2fs ^8(base)", durationBase)],
                );
                // L1569 (tests output.DurationMod, as in the Lua)
                if output.num(n!("DurationMod")) != 1.0 {
                    // L1570
                    bd_list(bd, n!("DurationTertiary"))
                        .push(lfmt!("x %.4f ^8(duration modifier)", durationMod));
                }
                // L1572
                if skillData.truthy(n!("debuffTertiary")) && debuffDurationMult != 1.0 {
                    // L1573
                    bd_list(bd, n!("DurationTertiary")).push(lfmt!(
                        "/ %.3f ^8(debuff expires slower/faster)",
                        1.0 / debuffDurationMult
                    ));
                }
                // L1575
                bd_list(bd, n!("DurationTertiary"))
                    .push(lfmt!("rounded up to nearest server tick"));
                // L1576
                bd_list(bd, n!("DurationTertiary"))
                    .push(lfmt!("= %.3fs", output.num(n!("DurationTertiary"))));
            }
        }
        // L1579
        durationBase = skillData.num0(n!("auraDuration"));
        // L1580
        if durationBase > 0.0 {
            // L1581
            let mut durationMod = calcLib::r#mod(&skillModList, skillCfg, nm!["Duration"]);
            // L1582
            durationMod = m_max(durationMod, 0.0);
            // L1583
            output.set(n!("AuraDuration"), durationBase * durationMod);
            // L1584
            output.set(
                n!("AuraDuration"),
                m_ceil(output.num(n!("AuraDuration")) * misc.ServerTickRate) / misc.ServerTickRate,
            );
            // L1585
            if let Some(bd) = breakdown
                .as_ref()
                .filter(|_| output.num(n!("AuraDuration")) != durationBase)
            {
                // L1586
                bd.set(
                    n!("AuraDuration"),
                    tbl![
                        lfmt!("%.2fs ^8(base)", durationBase),
                        lfmt!("x %.4f ^8(duration modifier)", durationMod),
                        "rounded up to nearest server tick",
                        lfmt!("= %.3fs", output.num(n!("AuraDuration"))),
                    ],
                );
            }
        }
        // L1594
        durationBase = skillData.num0(n!("reserveDuration"));
        // L1595
        if durationBase > 0.0 {
            // L1596
            let mut durationMod = calcLib::r#mod(&skillModList, skillCfg, nm!["Duration"]);
            // L1597
            durationMod = m_max(durationMod, 0.0);
            // L1598
            output.set(n!("ReserveDuration"), durationBase * durationMod);
            // L1599
            output.set(
                n!("ReserveDuration"),
                m_ceil(output.num(n!("ReserveDuration")) * misc.ServerTickRate)
                    / misc.ServerTickRate,
            );
            // L1600
            if let Some(bd) = breakdown
                .as_ref()
                .filter(|_| output.num(n!("ReserveDuration")) != durationBase)
            {
                // L1601
                bd.set(
                    n!("ReserveDuration"),
                    tbl![
                        lfmt!("%.2fs ^8(base)", durationBase),
                        lfmt!("x %.4f ^8(duration modifier)", durationMod),
                        "rounded up to nearest server tick",
                        lfmt!("= %.3fs", output.num(n!("ReserveDuration"))),
                    ],
                );
            }
        }
        // L1609
        durationBase = skillData.num0(n!("soulPreventionDuration"));
        // L1610
        if durationBase > 0.0 {
            // L1611
            let soul_names = names([
                Some(n!("SoulGainPreventionDuration")),
                skillData
                    .truthy(n!("skillEffectAppliesToSoulGainPrevention"))
                    .then(|| n!("Duration")),
                skillData
                    .truthy(n!("mineDurationAppliesToSkill"))
                    .then(|| n!("MineDuration")),
            ]);
            let mut durationMod = calcLib::r#mod(&skillModList, skillCfg, &soul_names);
            // L1612
            durationMod = m_max(durationMod, 0.0);
            // L1613
            output.set(n!("SoulGainPreventionDuration"), durationBase * durationMod);
            // L1614
            output.set(
                n!("SoulGainPreventionDuration"),
                m_max(
                    m_ceil(output.num(n!("SoulGainPreventionDuration")) * misc.ServerTickRate),
                    1.0,
                ) / misc.ServerTickRate,
            );
            // L1615
            if let Some(bd) = breakdown
                .as_ref()
                .filter(|_| output.num(n!("SoulGainPreventionDuration")) != durationBase)
            {
                // L1616
                bd.set(
                    n!("SoulGainPreventionDuration"),
                    tbl![
                        lfmt!("%.2fs ^8(base)", durationBase),
                        lfmt!("x %.4f ^8(duration modifier)", durationMod),
                        lfmt!("rounded up to nearest server tick"),
                        lfmt!("= %.3fs", output.num(n!("SoulGainPreventionDuration"))),
                    ],
                );
            }
        }
        // L1624
        output.set(
            n!("TotemDurationMod"),
            calcLib::r#mod(&skillModList, skillCfg, nm!["TotemDuration"]),
        );
        // L1625
        output.set(
            n!("TotemDurationMod"),
            m_max(output.num(n!("TotemDurationMod")), 0.0),
        );
        // L1626
        let TotemDurationBase = skillModList.Sum(ModType::BASE, skillCfg, nm!["TotemDuration"]);
        // L1627
        output.set(
            n!("TotemDuration"),
            m_ceil(TotemDurationBase * output.num(n!("TotemDurationMod")) * misc.ServerTickRate)
                / misc.ServerTickRate,
        );
        // L1628
        if let Some(bd) = &breakdown {
            // L1629
            bd.set(
                n!("TotemDurationMod"),
                bd.r#mod(&skillModList, skillCfg, nm!["TotemDuration"]),
            );
            // L1630
            bd.set(
                n!("TotemDuration"),
                tbl![lfmt!("%.2fs ^8(base)", TotemDurationBase)],
            );
            // L1633
            if output.num(n!("TotemDurationMod")) != 1.0 {
                // L1634
                bd_list(bd, n!("TotemDuration")).push(lfmt!(
                    "x %.4f ^8(duration modifier)",
                    output.num(n!("TotemDurationMod"))
                ));
            }
            // L1636
            bd_list(bd, n!("TotemDuration")).push(lfmt!("rounded up to nearest server tick"));
            // L1637
            bd_list(bd, n!("TotemDuration"))
                .push(lfmt!("= %.3fs", output.num(n!("TotemDuration"))));
        }
        // L1639
        output.set(
            n!("ImpaleDurationMod"),
            calcLib::r#mod(&skillModList, skillCfg, nm!["ImpaleDuration"]),
        );
        // L1640
        output.set(
            n!("ImpaleDurationMod"),
            m_max(output.num(n!("ImpaleDurationMod")), 0.0),
        );
        // L1641
        let ImpaleDurationBase = data().characterConstants.impaled_debuff_base_duration_ms / 1000.0;
        // L1642
        output.set(
            n!("ImpaleDuration"),
            m_ceil(
                ImpaleDurationBase
                    * output.num(n!("ImpaleDurationMod"))
                    * misc.ServerTickRate
                    * debuffDurationMult,
            ) / misc.ServerTickRate,
        );
        // L1643
        if let Some(bd) = &breakdown {
            // L1644
            bd.set(
                n!("ImpaleDurationMod"),
                bd.r#mod(&skillModList, skillCfg, nm!["ImpaleDuration"]),
            );
            // L1645
            bd.set(
                n!("ImpaleDuration"),
                tbl![lfmt!("%.2fs ^8(base)", ImpaleDurationBase)],
            );
            // L1648
            if debuffDurationMult != 1.0 {
                // L1649
                bd_list(bd, n!("ImpaleDuration")).push(lfmt!(
                    "/ %.3f ^8(debuff expires slower/faster)",
                    1.0 / debuffDurationMult
                ));
            }
            // L1651
            if output.num(n!("ImpaleDurationMod")) != 1.0 {
                // L1652
                bd_list(bd, n!("ImpaleDuration")).push(lfmt!(
                    "x %.4f ^8(duration modifier)",
                    output.num(n!("ImpaleDurationMod"))
                ));
            }
            // L1654
            bd_list(bd, n!("ImpaleDuration")).push(lfmt!("rounded up to nearest server tick"));
            // L1655
            bd_list(bd, n!("ImpaleDuration"))
                .push(lfmt!("= %.3fs", output.num(n!("ImpaleDuration"))));
        }
    }

    // L1659 Skill uptime
    // L1660
    {
        // L1661 exclude vaal skills as we currently don't support soul generation or gain prevention.
        if !has_type(SkillType::Vaal) {
            // L1662
            let cooldown = output.num0(n!("Cooldown"));
            // L1663 `ipairs` over a fresh literal list ("reserveDuration" is spelled as in the Lua)
            for durationType in [
                "Duration",
                "DurationSecondary",
                "DurationTertiary",
                "AuraDuration",
                "reserveDuration",
            ] {
                // L1664
                let duration = output.num0(Name::from(durationType));
                // L1665
                if duration != 0.0 && cooldown != 0.0 {
                    // L1666
                    #[allow(unused_assignments)]
                    let mut uptime = 1.0;
                    // L1667
                    if skillModList.Flag(skillCfg, nm!["NoCooldownRecoveryInDuration"]) {
                        // L1668
                        uptime = duration / (cooldown + duration);
                    } else {
                        // L1670
                        uptime = duration / (cooldown);
                    }
                    // L1672
                    uptime = m_min(uptime, 1.0);
                    // L1673
                    output.set(cat!(durationType, "Uptime"), uptime * 100.0);
                    // L1674
                    if let Some(bd) = &breakdown {
                        // L1675
                        if skillModList.Flag(skillCfg, nm!["NoCooldownRecoveryInDuration"]) {
                            // L1676
                            bd.set(
                                cat!(durationType, "Uptime"),
                                tbl![
                                    lfmt!("%.2fs / (%.2fs + %.2fs)", duration, cooldown, duration),
                                    lfmt!("= %d%%", output.num(cat!(durationType, "Uptime"))),
                                ],
                            );
                        } else {
                            // L1681
                            bd.set(
                                cat!(durationType, "Uptime"),
                                tbl![
                                    lfmt!("%.2fs / %.2fs", duration, cooldown),
                                    lfmt!("= %d%%", output.num(cat!(durationType, "Uptime"))),
                                ],
                            );
                        }
                    }
                }
            }
        }
    }
    Flow::Normal
}

// ---- Rust-only plumbing ---------------------------------------------------------------------------

/// The `{ type = "DistanceRamp", ramp = { … } }` tag of CalcOffence.lua 1083/1086.
fn distance_ramp(points: &[[f64; 2]]) -> Tag {
    let mut t = Tag::new(TagType::DistanceRamp);
    t.ramp = Some(Ramp::Points(Arc::from(points.to_vec())));
    t
}

/// `breakdown[k]` as the table a following `t_insert(breakdown[k], …)` writes into
/// (`t_insert` on nil raises, as in Lua).
fn bd_list(bd: &Breakdown, k: Name) -> Table {
    match bd.get(k) {
        Val::Table(t) => t,
        v => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            v.lua_type()
        ),
    }
}

/// A Lua value as a `string.format` argument (`%d` / `%.2f` / `%s`).
fn fmt_val(v: &Val) -> FmtArg<'_> {
    match v {
        Val::Num(n) => FmtArg::Num(*n),
        Val::Str(s) => FmtArg::Str(s),
        Val::Bool(b) => FmtArg::Bool(*b),
        Val::Nil => FmtArg::Nil,
        other => lua_error!(
            "bad argument to 'format' (a {} value has no Rust text)",
            other.lua_type()
        ),
    }
}

/// The text `..` makes of a value (numbers via `tostring`; nil, booleans and tables raise).
fn concat_val(v: &Val) -> String {
    match v {
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// A value used in arithmetic (`nil` raises).
fn arith_val(v: &Val, what: &str) -> f64 {
    if v.is_nil() {
        lua_error!("attempt to perform arithmetic on {what} (a nil value)");
    }
    v.num()
}

/// An optional number used in arithmetic (`nil` raises).
fn opt_arith(v: Option<f64>, local: &str) -> f64 {
    v.unwrap_or_else(|| {
        lua_error!("attempt to perform arithmetic on local '{local}' (a nil value)")
    })
}

/// A number argument of `math.min`/`math.max` (anything else raises).
fn lua_math_arg(v: &Val, arg: u32, f: &str) -> f64 {
    match v {
        Val::Num(n) => *n,
        Val::Str(s) => pob_lua::tonumber(s).unwrap_or_else(|| {
            lua_error!("bad argument #{arg} to '{f}' (number expected, got string)")
        }),
        other => lua_error!(
            "bad argument #{arg} to '{f}' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// An optional field as a `math.min`/`math.max` argument (`nil` raises).
fn opt_math_arg(v: Option<f64>, arg: u32, f: &str) -> f64 {
    v.unwrap_or_else(|| lua_error!("bad argument #{arg} to '{f}' (number expected, got nil)"))
}

/// `list[k]` of a 1-based Lua array (`nil` unless `k` is an integer in `1..=#list`).
fn lua_index_f64s(list: &[f64], k: &Val) -> Option<f64> {
    let k = k.opt_num()?;
    if k.fract() != 0.0 || k < 1.0 || k > list.len() as f64 {
        return None;
    }
    list.get(k as usize - 1).copied()
}

/// `t[k]` of an integer-keyed data table (`nil` unless `k` is an integer key of it).
fn lua_index_int_map(t: &std::collections::BTreeMap<i64, f64>, k: f64) -> Option<f64> {
    if k.fract() != 0.0 || !k.is_finite() {
        return None;
    }
    t.get(&(k as i64)).copied()
}

/// `data.monsterVarietyLifeMult[variety]` (`nil` for a nil or unknown variety).
fn lua_index_variety(t: &indexmap::IndexMap<AStr, f64>, k: &Val) -> Option<f64> {
    let k = k.str()?;
    t.get(&*k).copied()
}

/// `calcRadiusBreakpoints(baseRadius, incArea, moreArea)` with the four nils it returns for a
/// zero radius (CalcOffence.lua:198-224; `breakdown.area` then prints no breakpoint lines).
/// CCR-0043: the frozen OF-A signature returns four numbers; until it returns `Option`s the
/// nil case is recognised here by the same test the Lua makes (`radius > 0`, line 201), with
/// the same pure helpers.
fn radius_breakpoints(
    baseRadius: f64,
    incArea: f64,
    moreArea: f64,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    // CCR-0043
    let radius = calcRadius(baseRadius, calcAreaMod(incArea, moreArea), None);
    let (inc, more, red, less) = calcRadiusBreakpoints(baseRadius, incArea, moreArea, None);
    if radius > 0.0 {
        (Some(inc), Some(more), Some(red), Some(less))
    } else {
        (None, None, None, None)
    }
}
