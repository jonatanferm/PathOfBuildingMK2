//! @pob-file Data/Skills/act_str.lua @16de4b82d
//! Skill hooks of Data/Skills/act_str.lua (calc doc §8: `preDamageFunc`, `initialFunc`, `preSkillTypeFunc`,
//! `postCritFunc`, `explosiveArrowFunc`).
//!
//! Owner: SK-B. Signatures are U0's (frozen; change through a CCR); the bodies are ported
//! statement by statement from the Lua closures (calc doc §3, §8). Rust-only plumbing (field
//! access that raises Lua's index-of-nil error, `x or y` on mod values) is in the private
//! helpers at the end of the file.
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    unused_assignments,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

use super::{FnRow, SkillFuncImpl};
use pob_gamedata::tag;

/// Every skill hook of this file: id (closures.json), sha256 of its source lines, Rust body.
pub static ROWS: &[FnRow<SkillFuncImpl>] = &[
    FnRow {
        id: "data.skills.Earthquake.preDamageFunc",
        hash: "f83e1ae0bdaf20278a8f98f4c547f3f91a4fbe9bd6074350753a14d1bf68d7fd",
        f: SkillFuncImpl::Std(Earthquake_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FrozenSweep.preDamageFunc",
        hash: "2723a80de969128512021a766ddcc18b794345df65a8d02b7f248081e64f7073",
        f: SkillFuncImpl::Std(FrozenSweep_preDamageFunc),
    },
    FnRow {
        id: "data.skills.FrozenSweepAltX.preDamageFunc",
        hash: "6e21ef23c19d262d7b7f44d7bd8cb3d6592b9f54e97155a45950adf17c31dc4d",
        f: SkillFuncImpl::Std(FrozenSweepAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.HeraldOfAsh.preDamageFunc",
        hash: "b2dc72cf68544399285372853b14f33a4ea9ffd7a65c100bcb6c05e8251636d2",
        f: SkillFuncImpl::Std(HeraldOfAsh_preDamageFunc),
    },
    FnRow {
        id: "data.skills.InfernalBlow.preDamageFunc",
        hash: "51d48b99032e2cafffa213dfd711a775ddfa0608c6f184c6aceb084e9c16c0d2",
        f: SkillFuncImpl::Std(InfernalBlow_preDamageFunc),
    },
    FnRow {
        id: "data.skills.InfernalBlowAltX.preDamageFunc",
        hash: "e7d573845e95ae52fe78cb4fbf8db7fffad8426a8b1326ae229343ccaf7dcff1",
        f: SkillFuncImpl::Std(InfernalBlowAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MoltenShell.preDamageFunc",
        hash: "3a02403e817b375d2f14abed327b1bc6e16bec124cc220852b05eab7de952d01",
        f: SkillFuncImpl::Std(MoltenShell_preDamageFunc),
    },
    FnRow {
        id: "data.skills.VaalMoltenShell.preDamageFunc",
        hash: "87435ec8cd27eb64b0dde5d339e393bc127cbd034c313309a284591744107c8f",
        f: SkillFuncImpl::Std(VaalMoltenShell_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MoltenStrike.preDamageFunc",
        hash: "a54f809c7509641eed7fbd55b08bbf94ea474af7f4161f5a52c61860da4c1c6f",
        f: SkillFuncImpl::Std(MoltenStrike_preDamageFunc),
    },
    FnRow {
        id: "data.skills.MoltenStrikeAltX.preDamageFunc",
        hash: "1aa115a4b356ed97dea4dc33d1b6a8d907005bac936b9870758a4a51ec7e5c69",
        f: SkillFuncImpl::Std(MoltenStrikeAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.Perforate.preDamageFunc",
        hash: "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6",
        f: SkillFuncImpl::Std(Perforate_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PerforateAltX.preDamageFunc",
        hash: "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6",
        f: SkillFuncImpl::Std(PerforateAltX_preDamageFunc),
    },
    FnRow {
        id: "data.skills.PerforateAltY.preDamageFunc",
        hash: "14405ec5edd00d5194d9589ea73b42a0096d422d8fb6bacc1ec9b4e4cb4ff8c6",
        f: SkillFuncImpl::Std(PerforateAltY_preDamageFunc),
    },
    FnRow {
        id: "data.skills.RageVortex.preSkillTypeFunc",
        hash: "3c8202d2061670642148688e8d08947e4e8a3124e115251dc7861020e5dc1146",
        f: SkillFuncImpl::Std(RageVortex_preSkillTypeFunc),
    },
    FnRow {
        id: "data.skills.StaticStrike.preDamageFunc",
        hash: "30f986590c9224a1fd7f329b289eca6159a4a7058fbb725a25311d8b767fdfcb",
        f: SkillFuncImpl::Std(StaticStrike_preDamageFunc),
    },
];

/// @pob Data/Skills/act_str.lua:3363-3366 h=d130e236
/// `data.skills.Earthquake.preDamageFunc` (preDamageFunc)
pub(crate) fn Earthquake_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L3364
    let duration = m_floor(skillData.num(n!("duration")) * output.num(n!("DurationMod")) * 10.0);
    // L3365
    let v = skillModList.Sum(
        ModType::INC,
        Some(&*skillCfg),
        nm!["EarthquakeDurationIncDamage"],
    ) * duration;
    skillModList.NewMod(n!("Damage"), ModType::INC, v, "Skill:Earthquake", 0, 0, &[]);
}

/// @pob Data/Skills/act_str.lua:4679-4713 h=050930d4
/// `data.skills.FrozenSweep.preDamageFunc` (preDamageFunc)
pub(crate) fn FrozenSweep_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    let skillFlags = activeSkill.skillFlags.get();
    // L4680
    let mut parentSkill: Option<ActiveSkillRef> = None;
    // L4681-4682 (m_ceil, m_max)
    // L4683
    skillData.set(n!("showAverage"), false);
    // L4684
    skillFlags.set(n!("showAverage"), false);
    // L4685
    skillFlags.set(n!("notAverage"), true);

    // L4687
    let actor = actor_of(activeSkill);
    for_ipairs!((_, skill) in actor.activeSkillList.get() => {
        // L4688
        if skill.activeEffect.get().grantedEffect.get().name == n!("Frozen Legion")
            && socket_group_slot(&main_skill_of(&actor)) == socket_group_slot(activeSkill)
        {
            // L4689
            parentSkill = Some(skill);
            // L4690
            break;
        }
    });

    // L4694
    let parentSkill = parentSkill
        .unwrap_or_else(|| lua_error!("attempt to index local 'parentSkill' (a nil value)"));
    let parentModList = parentSkill.skillModList.get();
    let parentCfg = parentSkill.skillCfg.get();
    let parentData = parentSkill.skillData.get();
    if parentModList.Flag(Some(&*parentCfg), nm!["DisableSkill"])
        && !parentModList.Flag(Some(&*parentCfg), nm!["EnableSkill"])
    {
        // L4695
        return;
    }

    // L4698
    let v = parentData.get(n!("cooldown"));
    skillData.set(n!("cooldown"), v);
    // L4699
    let cooldownOverride =
        Val::from(parentModList.Override(Some(&*parentCfg), nm!["CooldownRecovery"]));
    // L4700
    let cooldown = lor(cooldownOverride, || {
        Val::from(
            (parentData.num(n!("cooldown"))
                + parentModList.Sum(ModType::BASE, Some(&*parentCfg), nm!["CooldownRecovery"]))
                / m_max(
                    0.0,
                    crate::modules::calc_tools::r#mod(
                        &parentModList,
                        Some(&*parentCfg),
                        nm!["CooldownRecovery"],
                    ),
                ),
        )
    })
    .num();
    // L4701
    output.set(
        n!("Cooldown"),
        m_ceil(cooldown * data().misc.ServerTickRate) / data().misc.ServerTickRate,
    );
    // L4702
    let v = output.get(n!("Cooldown"));
    skillData.set(n!("hitTimeOverride"), v);

    // L4704
    let maxStatues = parentData.num(n!("storedUses"))
        + parentModList.Sum(
            ModType::BASE,
            Some(&*parentCfg),
            nm!["AdditionalCooldownUses"],
        );
    // L4705
    let mut statuesWaitedFor: Option<f64> = None;

    // L4707
    if activeSkill.skillPart.get() == Some(1.0) {
        // L4708
        statuesWaitedFor = Some(1.0);
    } else if activeSkill.skillPart.get() == Some(2.0) {
        // L4710
        statuesWaitedFor = Some(maxStatues);
    }
    // L4712
    skillData.set(n!("averageBurstHits"), statuesWaitedFor);
}

/// @pob Data/Skills/act_str.lua:4797-4831 h=56a1ceed
/// `data.skills.FrozenSweepAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn FrozenSweepAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    let skillFlags = activeSkill.skillFlags.get();
    // L4798
    let mut parentSkill: Option<ActiveSkillRef> = None;
    // L4799-4800 (m_ceil, m_max)
    // L4801
    skillData.set(n!("showAverage"), false);
    // L4802
    skillFlags.set(n!("showAverage"), false);
    // L4803
    skillFlags.set(n!("notAverage"), true);

    // L4805
    let actor = actor_of(activeSkill);
    for_ipairs!((_, skill) in actor.activeSkillList.get() => {
        // L4806
        if skill.activeEffect.get().grantedEffect.get().name == n!("Frozen Legion of Rallying")
            && socket_group_slot(&main_skill_of(&actor)) == socket_group_slot(activeSkill)
        {
            // L4807
            parentSkill = Some(skill);
            // L4808
            break;
        }
    });

    // L4812
    let parentSkill = parentSkill
        .unwrap_or_else(|| lua_error!("attempt to index local 'parentSkill' (a nil value)"));
    let parentModList = parentSkill.skillModList.get();
    let parentCfg = parentSkill.skillCfg.get();
    let parentData = parentSkill.skillData.get();
    if parentModList.Flag(Some(&*parentCfg), nm!["DisableSkill"])
        && !parentModList.Flag(Some(&*parentCfg), nm!["EnableSkill"])
    {
        // L4813
        return;
    }

    // L4816
    let v = parentData.get(n!("cooldown"));
    skillData.set(n!("cooldown"), v);
    // L4817
    let cooldownOverride =
        Val::from(parentModList.Override(Some(&*parentCfg), nm!["CooldownRecovery"]));
    // L4818
    let cooldown = lor(cooldownOverride, || {
        Val::from(
            (parentData.num(n!("cooldown"))
                + parentModList.Sum(ModType::BASE, Some(&*parentCfg), nm!["CooldownRecovery"]))
                / m_max(
                    0.0,
                    crate::modules::calc_tools::r#mod(
                        &parentModList,
                        Some(&*parentCfg),
                        nm!["CooldownRecovery"],
                    ),
                ),
        )
    })
    .num();
    // L4819
    output.set(
        n!("Cooldown"),
        m_ceil(cooldown * data().misc.ServerTickRate) / data().misc.ServerTickRate,
    );
    // L4820
    let v = output.get(n!("Cooldown"));
    skillData.set(n!("hitTimeOverride"), v);

    // L4822
    let maxStatues = parentData.num(n!("storedUses"))
        + parentModList.Sum(
            ModType::BASE,
            Some(&*parentCfg),
            nm!["AdditionalCooldownUses"],
        );
    // L4823
    let mut statuesWaitedFor: Option<f64> = None;

    // L4825
    if activeSkill.skillPart.get() == Some(1.0) {
        // L4826
        statuesWaitedFor = Some(1.0);
    } else if activeSkill.skillPart.get() == Some(2.0) {
        // L4828
        statuesWaitedFor = Some(maxStatues);
    }
    // L4830
    skillData.set(n!("averageBurstHits"), statuesWaitedFor);
}

/// @pob Data/Skills/act_str.lua:5743-5745 h=1239adc2
/// `data.skills.HeraldOfAsh.preDamageFunc` (preDamageFunc)
pub(crate) fn HeraldOfAsh_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L5744
    let v = skillData.num_or(n!("hoaOverkill"), 0.0)
        * (1.0 + skillData.num(n!("hoaMoreBurn")) / 100.0)
        * skillData.num(n!("hoaOverkillPercent"));
    skillData.set(n!("FireDot"), v);
}

/// @pob Data/Skills/act_str.lua:7115-7123 h=f991cf74
/// `data.skills.InfernalBlow.preDamageFunc` (preDamageFunc)
pub(crate) fn InfernalBlow_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L7116
    let effect = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["DebuffEffect"]);
    // L7117
    if activeSkill.skillPart.get() == Some(2.0) || activeSkill.skillPart.get() == Some(3.0) {
        // L7118
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            effect,
            "Skill:InfernalBlow",
            0,
            0,
            &[tag!({ type = Multiplier, var = "DebuffStack", base = -100.0 + effect })],
        );
    }
    // L7120
    if activeSkill.skillPart.get() == Some(3.0) {
        // L7121
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), 1.0 / 6.0);
    }
}

/// @pob Data/Skills/act_str.lua:7241-7249 h=53b6c5c2
/// `data.skills.InfernalBlowAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn InfernalBlowAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    // L7242
    let effect = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["DebuffEffect"]);
    // L7243
    if activeSkill.skillPart.get() == Some(2.0) || activeSkill.skillPart.get() == Some(3.0) {
        // L7244
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            effect,
            "Skill:InfernalBlowAltX",
            0,
            0,
            &[tag!({ type = Multiplier, var = "DebuffStack", base = -100.0 + effect })],
        );
    }
    // L7246
    if activeSkill.skillPart.get() == Some(3.0) {
        // L7247
        activeSkill
            .skillData
            .get()
            .set(n!("dpsMultiplier"), 1.0 / 6.0);
    }
}

/// @pob Data/Skills/act_str.lua:7599-7603 h=35162e38
/// `data.skills.MoltenShell.preDamageFunc` (preDamageFunc)
pub(crate) fn MoltenShell_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L7600
    let add = skillData.num_or(n!("MoltenShellDamageMitigated"), 0.0)
        * skillData.num(n!("moltenShellReflect"))
        / 100.0;
    // L7601
    skillData.set(n!("FireMin"), add);
    // L7602
    skillData.set(n!("FireMax"), add);
}

/// @pob Data/Skills/act_str.lua:7701-7705 h=ec07950c
/// `data.skills.VaalMoltenShell.preDamageFunc` (preDamageFunc)
pub(crate) fn VaalMoltenShell_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L7702
    let add = skillData.num_or(n!("VaalMoltenShellDamageMitigated"), 0.0)
        * skillData.num(n!("moltenShellReflect"))
        / 100.0;
    // L7703
    skillData.set(n!("FireMin"), add);
    // L7704
    skillData.set(n!("FireMax"), add);
}

/// @pob Data/Skills/act_str.lua:7832-7882 h=23096fcc
/// `data.skills.MoltenStrike.preDamageFunc` (preDamageFunc)
pub(crate) fn MoltenStrike_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L7833
    let skillCfg = activeSkill.skillCfg.get();
    // L7834
    let skillData = activeSkill.skillData.get();
    // L7835
    let skillPart = activeSkill.skillPart.get();
    // L7836
    let skillModList = activeSkill.skillModList.get();
    // L7837-7838 (t_insert, s_format)

    // melee part doesn't need to calc balls
    // L7841
    if skillPart == Some(1.0) {
        // L7842
        return;
    }

    // L7845
    let enemyRadius = override_or_sum(&skillModList, &skillCfg, n!("EnemyRadius"));
    // L7846
    let ballRadius = output.num(n!("AreaOfEffectRadius"));
    // L7847
    let innerRadius = output.num(n!("AreaOfEffectRadiusSecondary"));
    // L7848
    let outerRadius = output.num(n!("AreaOfEffectRadiusTertiary"));

    // logic adapted from MoldyDwarf's calculator
    // L7851
    let hitRange = enemyRadius + ballRadius - innerRadius;
    // L7852
    let landingRange = outerRadius - innerRadius;
    // L7853
    let overlapChance = m_min(1.0, hitRange / landingRange);
    // L7854
    output.set(n!("OverlapChance"), overlapChance * 100.0);

    // L7856
    if let Some(breakdown) = breakdown {
        // L7857
        let oc = Table::new();
        breakdown.set(n!("OverlapChance"), oc.clone());
        // L7858
        oc.push("Chance for individual balls to land on the enemy:");
        // L7859
        oc.push("^8= (area where a ball can land on enemy) / (total area)");
        // L7860
        oc.push("^8= (enemy radius + ball radius - min travel) / (max travel - min travel)");
        // L7861-7862
        oc.push(lfmt!(
            "^8= (^7%d^8 + ^7%d^8 - ^7%d) / (^7%d^8 - ^7%d)",
            enemyRadius,
            ballRadius,
            innerRadius,
            outerRadius,
            innerRadius
        ));
        // L7863
        oc.push(lfmt!("^8=^7 %.2f^8%%", output.num(n!("OverlapChance"))));
    }

    // L7866
    let numProjectiles = output.get(n!("ProjectileCount"));
    // L7867
    let mut dpsMult = 1.0;
    // L7868
    if skillPart == Some(3.0) {
        // L7869
        dpsMult = overlapChance * numProjectiles.num();
        // L7870
        if let Some(breakdown) = breakdown {
            // L7871
            let sm = Table::new();
            breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
            // L7872
            sm.push("DPS multiplier");
            // L7873
            sm.push("^8= number of projectiles * overlap chance");
            // L7874
            sm.push(lfmt!(
                "^8=^7 %d^8 *^7 %.3f^8",
                numProjectiles.num(),
                overlapChance
            ));
            // L7875
            sm.push(lfmt!("^8=^7 %.3f", dpsMult));
        }
    }
    // L7878
    if dpsMult != 1.0 {
        // L7879
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMult;
        skillData.set(n!("dpsMultiplier"), v);
        // L7880
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMult;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_str.lua:8011-8082 h=7f66fb3d
/// `data.skills.MoltenStrikeAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn MoltenStrikeAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L8012
    let skillCfg = activeSkill.skillCfg.get();
    // L8013
    let skillData = activeSkill.skillData.get();
    // L8014
    let skillPart = activeSkill.skillPart.get();
    // L8015
    let skillModList = activeSkill.skillModList.get();
    // L8016-8017 (t_insert, s_format)

    // melee part doesn't need to calc balls
    // L8020
    if skillPart == Some(1.0) {
        // L8021
        return;
    }

    // L8024
    let enemyRadius = override_or_sum(&skillModList, &skillCfg, n!("EnemyRadius"));
    // L8025
    let ballRadius = output.num(n!("AreaOfEffectRadius"));
    // L8026
    let innerRadius = output.num(n!("AreaOfEffectRadiusSecondary"));
    // L8027
    let outerRadius = output.num(n!("AreaOfEffectRadiusTertiary"));

    // logic adapted from MoldyDwarf's calculator
    // L8030
    let hitRange = enemyRadius + ballRadius - innerRadius;
    // L8031
    let landingRange = outerRadius - innerRadius;
    // L8032
    let overlapChance = m_min(1.0, hitRange / landingRange);
    // L8033
    output.set(n!("OverlapChance"), overlapChance * 100.0);

    // L8035
    if let Some(breakdown) = breakdown {
        // L8036
        let oc = Table::new();
        breakdown.set(n!("OverlapChance"), oc.clone());
        // L8037
        oc.push("Chance for individual balls to land on the enemy:");
        // L8038
        oc.push("^8= (area where a ball can land on enemy) / (total area)");
        // L8039
        oc.push("^8= (enemy radius + ball radius - min travel) / (max travel - min travel)");
        // L8040-8041
        oc.push(lfmt!(
            "^8= (^7%d^8 + ^7%d^8 - ^7%d) / (^7%d^8 - ^7%d)",
            enemyRadius,
            ballRadius,
            innerRadius,
            outerRadius,
            innerRadius
        ));
        // L8042
        oc.push(lfmt!("^8=^7 %.2f^8%%", output.num(n!("OverlapChance"))));
    }

    // L8045
    let numProjectiles = output.get(n!("ProjectileCount"));
    // L8046
    let mut dpsMult = 1.0;
    // L8047
    if skillPart == Some(3.0) || skillPart == Some(5.0) || skillPart == Some(6.0) {
        // L8048
        dpsMult = overlapChance * numProjectiles.num();

        // L8050
        if skillPart != Some(6.0) {
            // L8051
            if let Some(breakdown) = breakdown {
                // L8052
                let sm = Table::new();
                breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
                // L8053
                sm.push("DPS multiplier");
                // L8054
                sm.push("^8= number of projectiles * overlap chance");
                // L8055
                sm.push(lfmt!(
                    "^8=^7 %d^8 *^7 %.3f^8",
                    numProjectiles.num(),
                    overlapChance
                ));
                // L8056
                sm.push(lfmt!("^8=^7 %.3f", dpsMult));
            }
        } else {
            // zenith: make an effective dpsMult for the weighted average of normal and 5th attack balls
            // L8060
            let fifthAttackMulti = 1.0 + skillData.num(n!("FifthStrikeDamage")) / 100.0;
            // L8061
            let fifthAttackOverallMulti = fifthAttackMulti
                * overlapChance
                * (numProjectiles.num() + skillData.num(n!("FifthStrikeProjectiles")));
            // L8062
            dpsMult = 0.8 * dpsMult + 0.2 * fifthAttackOverallMulti;

            // L8064
            if let Some(breakdown) = breakdown {
                // L8065
                let sm = Table::new();
                breakdown.set(n!("SkillDPSMultiplier"), sm.clone());
                // L8066
                sm.push("Weighted average DPS multiplier for balls");
                // L8067
                sm.push("^8= (0.8 * balls dps) + (0.2 * 5th attack balls dps)");
                // L8068-8069
                sm.push(lfmt!(
                    concat!(
                        "^8= (0.8 * normal ball hit * overlap chance * number of projectiles) ",
                        "+ (0.2 * ball hit * 5th attack multiplier * overlap chance * (number of projectiles + %d))"
                    ),
                    skillData.num(n!("FifthStrikeProjectiles"))
                ));
                // L8070-8071
                sm.push(lfmt!(
                    concat!(
                        "^8= ball hit * overlap chance * (0.8 * number of projectiles ",
                        "+ 0.2 * 5th attack multiplier * (number of projectiles + %d))"
                    ),
                    skillData.num(n!("FifthStrikeProjectiles"))
                ));
                // L8072-8073
                sm.push(lfmt!(
                    "^8= ball hit * ^7%.3f ^8* (0.8 * ^7%d ^8+ 0.2 * ^7%.1f ^8* ^7%d^8)",
                    overlapChance,
                    numProjectiles.num(),
                    fifthAttackMulti,
                    numProjectiles.num() + skillData.num(n!("FifthStrikeProjectiles"))
                ));
                // L8074
                sm.push(lfmt!("^8= ball hit * ^7 %.3f", dpsMult));
            }
        }
    }
    // L8078
    if dpsMult != 1.0 {
        // L8079
        let v = skillData.num_or(n!("dpsMultiplier"), 1.0) * dpsMult;
        skillData.set(n!("dpsMultiplier"), v);
        // L8080
        let v = output.num_or(n!("SkillDPSMultiplier"), 1.0) * dpsMult;
        output.set(n!("SkillDPSMultiplier"), v);
    }
}

/// @pob Data/Skills/act_str.lua:8304-8306 h=8534ec2b
/// `data.skills.Perforate.preDamageFunc` (preDamageFunc)
pub(crate) fn Perforate_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L8305
    let v = m_max(skillData.num_or(n!("dpsMultiplier"), 1.0), 1.0);
    skillData.set(n!("dpsMultiplier"), v);
}

/// @pob Data/Skills/act_str.lua:8400-8402 h=8534ec2b
/// `data.skills.PerforateAltX.preDamageFunc` (preDamageFunc)
pub(crate) fn PerforateAltX_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L8305
    let v = m_max(skillData.num_or(n!("dpsMultiplier"), 1.0), 1.0);
    skillData.set(n!("dpsMultiplier"), v);
}

/// @pob Data/Skills/act_str.lua:8501-8503 h=8534ec2b
/// `data.skills.PerforateAltY.preDamageFunc` (preDamageFunc)
pub(crate) fn PerforateAltY_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let skillData = activeSkill.skillData.get();
    // L8305
    let v = m_max(skillData.num_or(n!("dpsMultiplier"), 1.0), 1.0);
    skillData.set(n!("dpsMultiplier"), v);
}

/// @pob Data/Skills/act_str.lua:9082-9091 h=e93d90b1
/// `data.skills.RageVortex.preSkillTypeFunc` (preSkillTypeFunc)
pub(crate) fn RageVortex_preSkillTypeFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L9083
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        let skillData = activeSkill.skillData.get();
        // L9084
        let maxRage = skillModList.Sum(ModType::BASE, Some(&*skillCfg), nm!["MaximumRage"]);
        // L9085
        let rageVortexSacrificePercentage =
            skillData.num(n!("MaxRageVortexSacrificePercentage")) / 100.0;
        // L9086
        let configOverride = skillModList.Sum(
            ModType::BASE,
            Some(&*skillCfg),
            nm!["Multiplier:RageSacrificedStacks"],
        );
        // L9087
        let maxSacrificedRage = m_floor(rageVortexSacrificePercentage * maxRage);
        // L9088 (`(configOverride > 0 and configOverride)`: a number is truthy)
        let stacks = m_min(
            if configOverride > 0.0 {
                configOverride
            } else {
                maxSacrificedRage
            },
            maxSacrificedRage,
        );
        // L9089
        skillModList.NewMod(
            n!("Multiplier:RageSacrificed"),
            ModType::BASE,
            stacks,
            "Skill:RageVortex",
            0,
            0,
            &[],
        );
    }
}

/// @pob Data/Skills/act_str.lua:10726-10730 h=a455bfbd
/// `data.skills.StaticStrike.preDamageFunc` (preDamageFunc)
pub(crate) fn StaticStrike_preDamageFunc(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    // L10727
    if activeSkill.skillPart.get() == Some(2.0) {
        let skillData = activeSkill.skillData.get();
        let skillCfg = activeSkill.skillCfg.get();
        // L10728
        let v = skillData.num(n!("repeatFrequency"))
            / (1.0
                + activeSkill.skillModList.get().Sum(
                    ModType::INC,
                    Some(&*skillCfg),
                    nm!["StaticStrikeFrequency"],
                ) / 100.0);
        skillData.set(n!("hitTimeOverride"), v);
    }
}

// ---- helpers ------------------------------------------------------------------------------

/// `activeSkill.actor` (a dangling back-pointer is Lua's index of a nil value).
fn actor_of(activeSkill: &ActiveSkillRef) -> ActorRef {
    activeSkill
        .actor()
        .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"))
}

/// `skillModList:Override(skillCfg, name) or skillModList:Sum("BASE", skillCfg, name)`, used in
/// arithmetic (a non-number raises, as in Lua).
fn override_or_sum(skillModList: &ModList, skillCfg: &Cfg, name: Name) -> f64 {
    lor(
        Val::from(skillModList.Override(Some(&**skillCfg), &[name])),
        || Val::from(skillModList.Sum(ModType::BASE, Some(&**skillCfg), &[name])),
    )
    .num()
}

/// `actor.mainSkill` (nil raises on the next index, as in Lua).
fn main_skill_of(actor: &ActorRef) -> ActiveSkillRef {
    actor
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `skill.socketGroup.slot` (a nil `socketGroup` raises, as in Lua).
fn socket_group_slot(skill: &ActiveSkillRef) -> Option<Name> {
    skill
        .socketGroup
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'socketGroup' (a nil value)"))
        .slot
        .get()
}
