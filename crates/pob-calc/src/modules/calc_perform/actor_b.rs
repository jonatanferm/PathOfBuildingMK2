//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! CalcPerform.lua 636-1285: actor helpers B and the ally life pool tables.
//!
//! Owner: PF-B. Signatures are U0's (frozen; change through a CCR).
//!
//! Rust-only plumbing (snake_case): the `lua_*` helpers at the bottom reproduce the Lua
//! value semantics the bodies need (`x or d` on a query result, `math.max`'s argument
//! check, `t[i]` on a level table) and the `probe_fns` module (feature `probe`) exposes the
//! crate-private helpers to the oracle tests (CCR-0095 §3).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::modules::calc_perform::actor_a::applyEnemyModifiers;
use crate::modules::calc_setup::initModDB;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::tag;

/// @pob Modules/CalcPerform.lua:636-977 h=f6f72d62
/// `skillCfg` at 915 is an undeclared global (CCR-0004): LUA-GLOBAL-NIL.
/// Process enemy modifiers and other buffs
pub(crate) fn doActorMisc(env: &EnvRef, actor: &ActorRef) {
    // L637 ---@type ModDB
    // L638
    let modDB = actor.modDB.get();
    // L639
    let enemyDB = lua_index(actor.enemy(), "enemy").modDB.get();
    // L640 ---@class Output
    // L641
    let output = actor.output.get();
    // L642
    let condList = modDB.conditions.clone();

    // L644 Add misc buffs/debuffs
    // L645
    if env.mode_combat.get() {
        // L646
        if lua_index(env.player.mainSkill.get(), "mainSkill")
            .baseSkillModList
            .get()
            .Flag(None, nm!["Cruelty"])
        {
            // L647
            let v = lua_or_val(modDB.Override(None, nm!["Cruelty"]), || Val::Num(40.0));
            modDB.multipliers.set(n!("Cruelty"), v);
        }
        // L649 Minimum Rage
        // L650
        if modDB.Sum(ModType::BASE, None, nm!["MinimumRage"]) > modDB.multipliers.num0(n!("Rage")) {
            // L651
            let v = modDB.Sum(ModType::BASE, None, nm!["MinimumRage"]);
            modDB.multipliers.set(n!("Rage"), v);
        }
        // L653 `A and actor.parent.output.FortificationStacks or env.partyMembers and B and
        // env.partyMembers.output.FortificationStacks or 0`
        let alliedFortify = {
            let mut v = Val::Nil;
            if modDB.Flag(None, nm!["YourFortifyEqualToParent"]) {
                v = lua_index(actor.parent(), "parent")
                    .output
                    .get()
                    .get(n!("FortificationStacks"));
            }
            if !v.truthy() {
                v = Val::Nil;
                if let Some(partyMembers) = env.partyMembers.get() {
                    if partyMembers
                        .modDB
                        .get()
                        .Flag(None, nm!["PartyMemberFortifyEqualToYours"])
                    {
                        v = partyMembers.output.get().get(n!("FortificationStacks"));
                    }
                }
            }
            if v.truthy() {
                v.num()
            } else {
                0.0
            }
        };
        // L654 Minimum Fortification from King Maker or Perfect Naval Officer spectres or Ally override
        // L655
        if modDB.Sum(ModType::BASE, None, nm!["MinimumFortification"]) > 0.0 || alliedFortify > 0.0
        {
            // L656
            condList.set(n!("Fortified"), true);
        }
        // L658 Fortify
        // L659
        if modDB.Flag(None, nm!["Fortified"])
            || modDB.Sum(ModType::BASE, None, nm!["Multiplier:Fortification"]) > 0.0
        {
            // L660
            let skillModList: ModList = match actor.mainSkill.get() {
                Some(mainSkill) => mainSkill.skillModList.get(),
                None => actor.modDB.get(),
            };
            // L661
            let skillCfg: Option<Cfg> = actor.mainSkill.get().map(|s| s.skillCfg.get());
            // L662
            let maxStacks = m_max(
                lua_or_num(modDB.Override(None, nm!["MaximumFortification"]), || {
                    modDB.Sum(
                        ModType::BASE,
                        skillCfg.as_deref(),
                        nm!["MaximumFortification"],
                    )
                }),
                alliedFortify,
            );
            // L663
            let minStacks = m_min(
                if modDB.Flag(None, nm!["Condition:HaveMaxFortification"]) {
                    maxStacks
                } else {
                    modDB.Sum(ModType::BASE, None, nm!["MinimumFortification"])
                },
                maxStacks,
            );
            // L664
            let stacks = m_min(
                lua_or_num(modDB.Override(None, nm!["FortificationStacks"]), || {
                    if minStacks > 0.0 {
                        minStacks
                    } else {
                        maxStacks
                    }
                }),
                maxStacks,
            );
            // L665
            let increasedDuration = skillModList.Sum(ModType::INC, None, nm!["FortifyDuration"]);
            // L666
            output.set(n!("MaximumFortification"), maxStacks);
            // L667
            output.set(n!("MinimumFortification"), minStacks);
            // L668
            output.set(
                n!("RemovableFortification"),
                m_min(
                    maxStacks - minStacks,
                    lua_or_num(modDB.Override(None, nm!["FortificationStacks"]), || {
                        maxStacks
                    }) - minStacks,
                ),
            );
            // L669
            output.set(n!("FortificationStacks"), stacks);
            // L670
            output.set(
                n!("FortificationStacksOver20"),
                m_min(m_max(0.0, stacks - 20.0), maxStacks - 20.0),
            );
            // L671
            output.set(
                n!("FortifyDuration"),
                lua_or_num(skillModList.Override(None, nm!["FortifyDuration"]), || {
                    data().misc.FortifyBaseDuration
                }) * (1.0 + increasedDuration / 100.0),
            );
            // L672 string allows shown for Willowgift mod
            output.set(n!("FortificationEffect"), "0");
            // L673
            if !modDB.Flag(None, nm!["Condition:NoFortificationMitigation"]) {
                // L674
                output.set(n!("FortificationEffect"), stacks);
                // L675
                modDB.NewMod(
                    n!("DamageTakenWhenHit"),
                    ModType::MORE,
                    -stacks,
                    "Fortification",
                    0,
                    0,
                    &[],
                );
            }
            // L677
            if stacks >= maxStacks {
                // L678
                modDB.NewMod(
                    n!("Condition:HaveMaximumFortification"),
                    ModType::FLAG,
                    true,
                    "",
                    0,
                    0,
                    &[],
                );
            }
            // L680
            let v = modDB.multipliers.num0(n!("BuffOnSelf")) + 1.0;
            modDB.multipliers.set(n!("BuffOnSelf"), v);
        }
        // L682
        if modDB.Flag(None, nm!["Onslaught"]) {
            // L683
            let effect: f64;
            // L684 Loop detects if a Silver flask is used to grant Onslaught. If statement adds flask effect to calculation if one is being used
            // L685
            let mut onslaughtFromFlask = false;
            // L686 This value is set to negative and not 0 or else reduced effect would not properly apply
            // L687
            let mut flaskEffectInc = -100.0;
            // L688 (LUA-ORDER: table-identity keys; the result is a max, so order-independent)
            for (item, _) in env.flasks.get().with(|f| f.pairs()) {
                // L689 (`match("Silver Flask")`: no magic characters, a plain substring search)
                if lua_index(item.baseName.get(), "baseName")
                    .as_str()
                    .contains("Silver Flask")
                {
                    // L690
                    onslaughtFromFlask = true;

                    // L692
                    let mut curFlaskEffectInc = lua_index(item.flaskData.get(), "flaskData")
                        .num(n!("effectInc"))
                        + modDB.Sum(
                            ModType::INC,
                            Some(&cfg_actor(n!("player"))),
                            nm!["FlaskEffect"],
                        );
                    // L693
                    if item.rarity.get() == n!("MAGIC") {
                        // L694
                        curFlaskEffectInc = curFlaskEffectInc
                            + modDB.Sum(
                                ModType::INC,
                                Some(&cfg_actor(n!("player"))),
                                nm!["MagicUtilityFlaskEffect"],
                            );
                    }

                    // L697
                    if flaskEffectInc < curFlaskEffectInc / 100.0 {
                        // L698
                        flaskEffectInc = curFlaskEffectInc / 100.0;
                    }
                }
            }
            // L702
            let onslaughtEffectInc = modDB.Sum(
                ModType::INC,
                None,
                nm!["OnslaughtEffect", "BuffEffectOnSelf"],
            ) / 100.0;
            // L703
            if onslaughtFromFlask {
                // L704
                effect = m_floor(20.0 * (1.0 + flaskEffectInc + onslaughtEffectInc));
            } else {
                // L706
                effect = m_floor(20.0 * (1.0 + onslaughtEffectInc));
            }
            // L708
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                effect,
                "Onslaught",
                ModFlag::Attack,
                0,
                &[],
            );
            // L709
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                effect,
                "Onslaught",
                ModFlag::Cast,
                0,
                &[],
            );
            // L710
            modDB.NewMod(
                n!("MovementSpeed"),
                ModType::INC,
                effect,
                "Onslaught",
                0,
                0,
                &[],
            );
        }
        // L712
        if modDB.conditions.truthy(n!("AffectedByArcaneSurge"))
            || modDB.Flag(None, nm!["Condition:ArcaneSurge"])
        {
            // L713
            modDB.conditions.set(n!("AffectedByArcaneSurge"), true);
            // L714
            let mut effect = 1.0
                + modDB.Sum(
                    ModType::INC,
                    None,
                    nm!["ArcaneSurgeEffect", "BuffEffectOnSelf"],
                ) / 100.0;
            // L715
            effect = effect
                + modDB.Sum(
                    ModType::INC,
                    Some(&cfg_actor(n!("player"))),
                    nm!["FlaskEffect"],
                ) / 100.0
                    * modDB.Sum(ModType::BASE, None, nm!["FlaskEffectToArcaneSurgeEffect"])
                    / 100.0;
            // L716
            modDB.NewMod(
                n!("ManaRegen"),
                ModType::INC,
                modDB.Max(None, nm!["ArcaneSurgeManaRegen"]).unwrap_or(30.0) * effect,
                "Arcane Surge",
                0,
                0,
                &[],
            );
            // L717
            let arcaneSurgeCastSpeed =
                modDB.Max(None, nm!["ArcaneSurgeCastSpeed"]).unwrap_or(20.0) * effect;
            // L718
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                arcaneSurgeCastSpeed,
                "Arcane Surge",
                ModFlag::Cast,
                0,
                &[],
            );
            // L719
            if modDB.Flag(None, nm!["ArcaneSurgeCastSpeedToMovementSpeed"]) {
                // L720
                modDB.NewMod(
                    n!("MovementSpeed"),
                    ModType::INC,
                    arcaneSurgeCastSpeed,
                    "Arcane Surge",
                    0,
                    0,
                    &[],
                );
            }
            // L722
            let arcaneSurgeDamage = modDB.Max(None, nm!["ArcaneSurgeDamage"]).unwrap_or(0.0);
            // L723
            if arcaneSurgeDamage != 0.0 {
                // L724
                modDB.NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    arcaneSurgeDamage * effect,
                    "Arcane Surge",
                    ModFlag::Spell,
                    0,
                    &[],
                );
            }
            // L726
            let prefix = "ArcaneSurgeAlso";
            // L727
            let additionalMods: [(Name, ModType); 3] = [
                (n!("LifeRegen"), ModType::INC),
                (n!("ManaRecoup"), ModType::BASE),
                (n!("ManaCostEfficiency"), ModType::INC),
            ];
            // L728 (LUA-ORDER: string keys, PoB's order is random per process; each pass adds a
            // mod of a different name, so the per-name mod lists do not depend on it)
            for (r#mod, modType) in additionalMods {
                // L729
                let value = modDB.Sum(modType, None, &[cat!(prefix, r#mod)]);
                // L730
                if value != 0.0 {
                    // L731
                    modDB.NewMod(r#mod, modType, value * effect, "Arcane Surge", 0, 0, &[]);
                }
            }
        }
        // L735
        if modDB.Flag(None, nm!["Fanaticism"])
            && actor
                .mainSkill
                .get()
                .is_some_and(|s| s.skillFlags.get().truthy(n!("selfCast")))
        {
            // L736
            let effect = m_floor(
                75.0 * (1.0 + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]) / 100.0),
            );
            // L737
            modDB.NewMod(
                n!("Speed"),
                ModType::MORE,
                effect,
                "Fanaticism",
                ModFlag::Cast,
                0,
                &[],
            );
            // L738
            modDB.NewMod(
                n!("Cost"),
                ModType::MORE,
                -effect,
                "Fanaticism",
                ModFlag::Cast,
                0,
                &[],
            );
            // L739
            modDB.NewMod(
                n!("AreaOfEffect"),
                ModType::INC,
                effect,
                "Fanaticism",
                ModFlag::Cast,
                0,
                &[],
            );
        }
        // L741
        if modDB.Flag(None, nm!["Condition:CanGainSpiritInfusion"]) {
            // L742 storing tags in local var to avoid repetition
            // L743
            let globalEffectTag = tag!({ type = GlobalEffect, effectType = "Buff", effectName = "Spirit Infusion", unscalable = true });
            // L744
            let multiplierTag = tag!({ type = Multiplier, var = "SpiritInfusion" });

            // L746
            modDB.NewMod(
                n!("EnergyShieldRechargeFaster"),
                ModType::INC,
                15.0,
                "Spirit Infusion",
                0,
                0,
                &[multiplierTag.clone(), globalEffectTag.clone()],
            );
            // L747 (`nil` flags: createMod keeps flags 0 and takes KeywordFlag.Spell, ModTools.lua:42-49)
            modDB.NewMod(
                n!("Damage"),
                ModType::MORE,
                5.0,
                "Spirit Infusion",
                0,
                KeywordFlag::Spell,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Channel }),
                    multiplierTag.clone(),
                    globalEffectTag.clone(),
                ],
            );
            // L748
            modDB.NewMod(
                n!("Cost"),
                ModType::MORE,
                10.0,
                "Spirit Infusion",
                0,
                KeywordFlag::Spell,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Channel }),
                    multiplierTag.clone(),
                    globalEffectTag.clone(),
                ],
            );
        }
        // L750
        if modDB.Flag(None, nm!["UnholyMight"]) {
            // L751
            let effect = 1.0 + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]) / 100.0;
            // L752
            modDB.NewMod(
                n!("PhysicalDamageConvertToChaos"),
                ModType::BASE,
                m_floor(100.0 * effect),
                "Unholy Might",
                0,
                0,
                &[],
            );
            // L753
            modDB.NewMod(
                n!("Condition:CanWither"),
                ModType::FLAG,
                true,
                "Unholy Might",
                0,
                0,
                &[],
            );
            // L754
            let value = modDB.Sum(ModType::BASE, None, nm!["UnholyMightAlsoChaosPenetration"]);
            // L755
            if value != 0.0 {
                // L756
                modDB.NewMod(
                    n!("ChaosPenetration"),
                    ModType::BASE,
                    value * effect,
                    "Unholy Might",
                    0,
                    0,
                    &[],
                );
            }
        }
        // L759
        if modDB.Flag(None, nm!["ShepherdOfSouls"]) {
            // L760
            modDB.NewMod(
                n!("SoulCost"),
                ModType::MORE,
                -80.0,
                "Shepherd of Souls",
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Vaal }),
                    tag!({ type = SkillType, skillType = SkillType::Aura, neg = true }),
                ],
            );
            // L761
            modDB.NewMod(
                n!("SoulCost"),
                ModType::INC,
                100.0,
                "Shepherd of Souls",
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Vaal }),
                    tag!({ type = SkillType, skillType = SkillType::Aura, neg = true }),
                    tag!({ type = Multiplier, var = "VaalSkillsUsedInPast8Seconds" }),
                ],
            );
        }
        // L763
        if modDB.Flag(None, nm!["ChaoticMight"]) {
            // L764
            let effect = m_floor(
                30.0 * (1.0 + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]) / 100.0),
            );
            // L765
            modDB.NewMod(
                n!("PhysicalDamageGainAsChaos"),
                ModType::BASE,
                effect,
                "Chaotic Might",
                0,
                0,
                &[],
            );
        }
        // L767
        if modDB.Flag(None, nm!["Tailwind"]) {
            // L768
            let effect = m_floor(
                8.0 * (1.0
                    + modDB.Sum(
                        ModType::INC,
                        None,
                        nm!["TailwindEffectOnSelf", "BuffEffectOnSelf"],
                    ) / 100.0),
            );
            // L769
            modDB.NewMod(
                n!("ActionSpeed"),
                ModType::INC,
                effect,
                "Tailwind",
                0,
                0,
                &[],
            );
        }
        // L771
        if modDB.Flag(None, nm!["Condition:TotemTailwind"]) {
            // L772
            modDB.NewMod(
                n!("TotemActionSpeed"),
                ModType::INC,
                8.0,
                "Tailwind",
                0,
                0,
                &[],
            );
        }
        // L774
        if modDB.Flag(None, nm!["Adrenaline"]) {
            // L775
            let effectMod = 1.0 + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]) / 100.0;
            // L776
            modDB.NewMod(
                n!("Damage"),
                ModType::INC,
                m_floor(100.0 * effectMod),
                "Adrenaline",
                0,
                0,
                &[],
            );
            // L777
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                m_floor(25.0 * effectMod),
                "Adrenaline",
                ModFlag::Attack,
                0,
                &[],
            );
            // L778
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                m_floor(25.0 * effectMod),
                "Adrenaline",
                ModFlag::Cast,
                0,
                &[],
            );
            // L779
            modDB.NewMod(
                n!("MovementSpeed"),
                ModType::INC,
                m_floor(25.0 * effectMod),
                "Adrenaline",
                0,
                0,
                &[],
            );
            // L780
            modDB.NewMod(
                n!("PhysicalDamageReduction"),
                ModType::BASE,
                m_floor(10.0 * effectMod),
                "Adrenaline",
                0,
                0,
                &[],
            );
        }
        // L782 config checked and ascendancy allocated, allows user to see dps change in passive tree
        // L783
        if modDB.Flag(None, nm!["Condition:WildSavagery"]) && modDB.Flag(None, nm!["WildSavagery"])
        {
            // L784
            modDB.NewMod(
                n!("PhysicalDamage"),
                ModType::INC,
                100.0,
                "Wild Savagery",
                0,
                0,
                &[],
            );
            // L785
            modDB.NewMod(
                n!("ActionSpeed"),
                ModType::INC,
                10.0,
                "Wild Savagery",
                0,
                0,
                &[],
            );
            // L786
            modDB.NewMod(
                n!("IgnoreEnemyPhysicalDamageReduction"),
                ModType::FLAG,
                true,
                "Wild Savagery",
                0,
                0,
                &[],
            );
            // L787
            modDB.NewMod(
                n!("StunImmune"),
                ModType::FLAG,
                true,
                "Wild Savagery",
                0,
                0,
                &[],
            );
        }
        // L789
        if modDB.Flag(None, nm!["Convergence"]) {
            // L790
            let effect = m_floor(
                30.0 * (1.0 + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]) / 100.0),
            );
            // L791
            modDB.NewMod(
                n!("ElementalDamage"),
                ModType::MORE,
                effect,
                "Convergence",
                0,
                0,
                &[],
            );
        }
        // L793
        if modDB.Flag(None, nm!["HerEmbrace"]) {
            // L794
            condList.set(n!("HerEmbrace"), true);
            // L795
            modDB.NewMod(
                n!("AvoidStun"),
                ModType::BASE,
                100.0,
                "Her Embrace",
                0,
                0,
                &[],
            );
            // L796
            modDB.NewMod(
                n!("PhysicalDamageGainAsFire"),
                ModType::BASE,
                123.0,
                "Her Embrace",
                ModFlag::Sword,
                0,
                &[],
            );
            // L797
            modDB.NewMod(
                n!("AvoidFreeze"),
                ModType::BASE,
                100.0,
                "Her Embrace",
                0,
                0,
                &[],
            );
            // L798
            modDB.NewMod(
                n!("AvoidChill"),
                ModType::BASE,
                100.0,
                "Her Embrace",
                0,
                0,
                &[],
            );
            // L799
            modDB.NewMod(
                n!("AvoidIgnite"),
                ModType::BASE,
                100.0,
                "Her Embrace",
                0,
                0,
                &[],
            );
            // L800
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                20.0,
                "Her Embrace",
                ModFlag::Attack,
                0,
                &[],
            );
            // L801
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                20.0,
                "Her Embrace",
                ModFlag::Cast,
                0,
                &[],
            );
            // L802
            modDB.NewMod(
                n!("MovementSpeed"),
                ModType::INC,
                20.0,
                "Her Embrace",
                0,
                0,
                &[],
            );
        }
        // L804
        if modDB.Flag(None, nm!["Condition:OnConsecratedGround"]) {
            // L805
            let effect =
                1.0 + modDB.Sum(ModType::INC, None, nm!["ConsecratedGroundEffect"]) / 100.0;
            // L806
            modDB.NewMod(
                n!("LifeRegenPercent"),
                ModType::BASE,
                5.0 * effect,
                "Consecrated Ground",
                0,
                0,
                &[],
            );
            // L807
            modDB.NewMod(
                n!("Accuracy"),
                ModType::INC,
                m_floor(
                    modDB.Sum(ModType::INC, None, nm!["ConsecratedGroundAlsoAccuracy"]) * effect,
                ),
                "Consecrated Ground",
                0,
                0,
                &[],
            );
        }
        // L809
        if modDB.Flag(None, nm!["Condition:PhantasmalMight"]) {
            // L810 slight hack to not double count the initial buff
            let v = modDB.multipliers.num0(n!("BuffOnSelf"))
                + output.num_or(n!("ActivePhantasmLimit"), 1.0)
                - 1.0;
            modDB.multipliers.set(n!("BuffOnSelf"), v);
        }
        // L812
        if modDB.Flag(None, nm!["Elusive"]) {
            // L813
            let maxSkillInc = modDB
                .Max(Some(&cfg_source(n!("Skill"))), nm!["ElusiveEffect"])
                .unwrap_or(0.0);
            // L814
            let mut inc = modDB.Sum(ModType::INC, None, nm!["ElusiveEffect", "BuffEffectOnSelf"]);
            // L815
            if lua_index(actor.mainSkill.get(), "mainSkill")
                .skillModList
                .get()
                .Flag(None, nm!["SupportedByNightblade"])
            {
                // L816
                inc = inc + modDB.Sum(ModType::INC, None, nm!["NightbladeSupportedElusiveEffect"]);
            }
            // L818
            inc = inc + maxSkillInc;
            // L819
            let elusiveEffectMod = (1.0 + inc / 100.0)
                * modDB.More(None, nm!["ElusiveEffect", "BuffEffectOnSelf"])
                * 100.0;
            // L820
            let elusiveEffectMinThreshold = lua_or_num(
                modDB.Override(None, nm!["ElusiveEffectMinThreshold"]),
                || 0.0,
            );
            // L821
            let elusiveEffectIncreaseDuration =
                modDB.Sum(ModType::BASE, None, nm!["ElusiveEffectIncreaseDuration"]);
            // L822
            let mut peakElusiveEffect = elusiveEffectMod;
            // L823
            if elusiveEffectIncreaseDuration > 0.0 {
                // L824
                let elusiveEffectChangeRate = 20.0
                    / (1.0 + modDB.Sum(ModType::INC, None, nm!["ElusiveEffectLossSlower"]) / 100.0);
                // L825
                peakElusiveEffect =
                    elusiveEffectMod + elusiveEffectChangeRate * elusiveEffectIncreaseDuration;
                // L826
                let elusiveEffectDecreaseDuration =
                    (peakElusiveEffect - elusiveEffectMinThreshold) / elusiveEffectChangeRate;
                // L827
                let totalElusiveEffectDuration =
                    elusiveEffectIncreaseDuration + elusiveEffectDecreaseDuration;
                // L828
                let averageIncreaseEffect = (elusiveEffectMod + peakElusiveEffect) / 2.0;
                // L829
                let averageDecreaseEffect = (peakElusiveEffect + elusiveEffectMinThreshold) / 2.0;
                // L830
                output.set(
                    n!("ElusiveEffectMod"),
                    (averageIncreaseEffect * elusiveEffectIncreaseDuration
                        + averageDecreaseEffect * elusiveEffectDecreaseDuration)
                        / totalElusiveEffectDuration,
                );
            } else {
                // L832
                output.set(
                    n!("ElusiveEffectMod"),
                    (elusiveEffectMod + elusiveEffectMinThreshold) / 2.0,
                );
            }
            // L834 if we want the max skill to not be noted as its own breakdown table entry, comment out below
            // L835
            modDB.NewMod(
                n!("ElusiveEffect"),
                ModType::INC,
                maxSkillInc,
                "Max Skill Effect",
                0,
                0,
                &[],
            );
            // L836 Override elusive effect if set.
            // L837
            if modDB.Override(None, nm!["ElusiveEffect"]).truthy() {
                // L838
                output.set(
                    n!("ElusiveEffectMod"),
                    m_min(
                        lua_checknum(&modDB.Override(None, nm!["ElusiveEffect"]), 1, "min"),
                        peakElusiveEffect,
                    ),
                );
            }
            // L840
            let effect = output.num(n!("ElusiveEffectMod")) / 100.0;
            // L841
            condList.set(n!("Elusive"), true);
            // L842
            modDB.NewMod(
                n!("AvoidAllDamageFromHitsChance"),
                ModType::BASE,
                m_floor(15.0 * effect),
                "Elusive",
                0,
                0,
                &[],
            );
            // L843
            modDB.NewMod(
                n!("MovementSpeed"),
                ModType::INC,
                m_floor(30.0 * effect),
                "Elusive",
                0,
                0,
                &[],
            );
        }
        // L845
        if modDB.Max(None, nm!["WitherEffectStack"]).is_some() {
            // L846
            modDB.NewMod(
                n!("Condition:CanWither"),
                ModType::FLAG,
                true,
                "Config",
                0,
                0,
                &[],
            );
            // L847
            let effect = modDB.Max(None, nm!["WitherEffectStack"]);
            // L848
            enemyDB.NewMod(
                n!("ChaosDamageTaken"),
                ModType::INC,
                effect,
                "Withered",
                0,
                0,
                &[tag!({ type = Multiplier, var = "WitheredStack", limit = 15.0 })],
            );
        }
        // L850
        if modDB.Flag(None, nm!["Condition:CanBeWithered"]) {
            // L851
            let effect = 6.0 * (100.0 + modDB.Sum(ModType::INC, None, nm!["WitherEffectOnSelf"]))
                / 100.0
                * modDB.More(None, nm!["WitherEffectOnSelf"]);
            // L852
            modDB.NewMod(
                n!("ChaosDamageTaken"),
                ModType::INC,
                effect,
                "Withered",
                0,
                0,
                &[tag!({ type = Multiplier, var = "WitheredStack", limit = 15.0 })],
            );
        }
        // L854
        if modDB.Flag(None, nm!["Excommunicated"]) {
            // L855
            modDB.NewMod(
                n!("ChaosDamage"),
                ModType::MORE,
                -100.0,
                "Excommunicated",
                0,
                0,
                &[],
            );
        }
        // L857
        if modDB.Flag(None, nm!["Blind"]) && !modDB.Flag(None, nm!["CannotBeBlinded"]) {
            // L858
            if !modDB.Flag(None, nm!["UnaffectedByBlind"]) {
                // L859
                let mut effect = 1.0
                    + modDB.Sum(ModType::INC, None, nm!["BlindEffect", "BuffEffectOnSelf"]) / 100.0;
                // L860 Override Blind effect if set.
                // L861
                if modDB.Override(None, nm!["BlindEffect"]).truthy() {
                    // L862
                    effect = m_min(
                        Val::from_value(&modDB.Override(None, nm!["BlindEffect"])).num() / 100.0,
                        effect,
                    );
                }
                // L864
                modDB.NewMod(
                    n!("Accuracy"),
                    ModType::MORE,
                    m_floor(-20.0 * effect),
                    "Blind",
                    0,
                    0,
                    &[],
                );
                // L865
                modDB.NewMod(
                    n!("Evasion"),
                    ModType::MORE,
                    m_floor(-20.0 * effect),
                    "Blind",
                    0,
                    0,
                    &[],
                );
            }
        }
        // L868
        if modDB.Flag(None, nm!["Chill"]) {
            // L869
            let ailmentData = &data().nonDamagingAilment;
            // L870 (LUA-QUIRK: `Override` returns no value when nothing matches, so this is
            // `math.max(sum)`; math.max never returns nil, so `or ailmentData.Chill.default`
            // is dead)
            let chillValue = lua_max_override(
                modDB.Sum(ModType::BASE, None, nm!["SelfChillOverride"]),
                modDB.Override(None, nm!["ChillVal"]),
            );
            // L871
            let totalChillSelfEffect = calcLib::r#mod(&modDB, None, nm!["SelfChillEffect"]);
            // L872-874
            let avoidChill = if modDB.Flag(None, nm!["ChillImmune", "ElementalAilmentImmune"]) {
                100.0
            } else {
                m_floor(m_min(
                    modDB.Sum(
                        ModType::BASE,
                        None,
                        nm!["AvoidChill", "AvoidAilments", "AvoidElementalAilments"],
                    ) + (if modDB.Flag(None, nm!["ShockAvoidAppliesToElementalAilments"]) {
                        modDB.Sum(ModType::BASE, None, nm!["AvoidShock"])
                    } else {
                        0.0
                    }) + (if modDB.Flag(None, nm!["SpellSuppressionAppliesToAilmentAvoidance"]) {
                        modDB.Sum(ModType::BASE, None, nm!["SpellSuppressionChance"]) / 2.0
                    } else {
                        0.0
                    }),
                    100.0,
                ))
            };

            // L876
            let effect = if avoidChill == 100.0 {
                0.0
            } else {
                m_min(
                    m_max(m_floor(chillValue * totalChillSelfEffect), 0.0),
                    lua_or_num(modDB.Override(None, nm!["ChillMax"]), || {
                        ailment(ailmentData, "Chill").max
                    }),
                )
            };
            // L877
            if modDB.Flag(None, nm!["SkitterbotBonechill"]) {
                // L878
                modDB.NewMod(
                    n!("ColdDamageTaken"),
                    ModType::INC,
                    effect
                        * (if modDB.Flag(None, nm!["SelfChillEffectIsReversed"]) {
                            -1.0
                        } else {
                            1.0
                        }),
                    "Bonechill",
                    0,
                    0,
                    &[],
                );
            }
            // L880
            modDB.NewMod(
                n!("ActionSpeed"),
                ModType::INC,
                effect
                    * (if modDB.Flag(None, nm!["SelfChillEffectIsReversed"]) {
                        1.0
                    } else {
                        -1.0
                    }),
                "Chill",
                0,
                0,
                &[],
            );
        }
        // L882
        if modDB.Flag(None, nm!["Shock"]) {
            // L883
            let ailmentData = &data().nonDamagingAilment;
            // L884 (LUA-QUIRK, see L870)
            let shockValue = lua_max_override(
                modDB.Sum(ModType::BASE, None, nm!["SelfShockOverride"]),
                modDB.Override(None, nm!["ShockVal"]),
            );
            // L885
            let totalShockSelfEffect = calcLib::r#mod(&modDB, None, nm!["SelfShockEffect"]);
            // L886-887
            let avoidShock = if modDB.Flag(None, nm!["ShockImmune", "ElementalAilmentImmune"]) {
                100.0
            } else {
                m_floor(m_min(
                    modDB.Sum(
                        ModType::BASE,
                        None,
                        nm!["AvoidShock", "AvoidAilments", "AvoidElementalAilments"],
                    ) + (if modDB.Flag(None, nm!["SpellSuppressionAppliesToAilmentAvoidance"]) {
                        modDB.Sum(ModType::BASE, None, nm!["SpellSuppressionChance"]) / 2.0
                    } else {
                        0.0
                    }),
                    100.0,
                ))
            };

            // L889
            let effect = if avoidShock == 100.0 {
                0.0
            } else {
                m_min(
                    m_max(m_floor(shockValue * totalShockSelfEffect), 0.0),
                    lua_or_num(modDB.Override(None, nm!["ShockMax"]), || {
                        ailment(ailmentData, "Shock").max
                    }),
                )
            };
            // L890
            modDB.NewMod(n!("DamageTaken"), ModType::INC, effect, "Shock", 0, 0, &[]);
        }
        // L892
        if modDB.Flag(None, nm!["Scorch"]) {
            // L893
            let ailmentData = &data().nonDamagingAilment;
            // L894 (LUA-QUIRK, see L870)
            let scorchValue = lua_max_override(
                modDB.Sum(ModType::BASE, None, nm!["SelfScorchOverride"]),
                modDB.Override(None, nm!["ScorchVal"]),
            );
            // L895
            let totalScorchSelfEffect = calcLib::r#mod(&modDB, None, nm!["SelfScorchEffect"]);
            // L896-898
            let avoidScorch = if modDB.Flag(None, nm!["ScorchImmune", "ElementalAilmentImmune"]) {
                100.0
            } else {
                m_floor(m_min(
                    modDB.Sum(
                        ModType::BASE,
                        None,
                        nm!["AvoidScorch", "AvoidAilments", "AvoidElementalAilments"],
                    ) + (if modDB.Flag(None, nm!["ShockAvoidAppliesToElementalAilments"]) {
                        modDB.Sum(ModType::BASE, None, nm!["AvoidShock"])
                    } else {
                        0.0
                    }) + (if modDB.Flag(None, nm!["SpellSuppressionAppliesToAilmentAvoidance"]) {
                        modDB.Sum(ModType::BASE, None, nm!["SpellSuppressionChance"]) / 2.0
                    } else {
                        0.0
                    }),
                    100.0,
                ))
            };
            // L899
            let effect = if avoidScorch == 100.0 {
                0.0
            } else {
                m_min(
                    m_max(m_floor(scorchValue * totalScorchSelfEffect), 0.0),
                    lua_or_num(modDB.Override(None, nm!["ScorchMax"]), || {
                        ailment(ailmentData, "Scorch").max
                    }),
                )
            };
            // L900
            modDB.NewMod(
                n!("ElementalResist"),
                ModType::BASE,
                -effect,
                "Scorch",
                0,
                0,
                &[],
            );
        }
        // L902
        if modDB.Flag(None, nm!["Freeze"]) {
            // L903
            let effect = m_max(
                m_floor(70.0 * calcLib::r#mod(&modDB, None, nm!["SelfChillEffect"])),
                0.0,
            );
            // L904
            modDB.NewMod(
                n!("ActionSpeed"),
                ModType::INC,
                -effect,
                "Freeze",
                0,
                0,
                &[],
            );
        }
        // L906
        if modDB.Flag(None, nm!["CanLeechLifeOnFullLife"]) && !modDB.Flag(None, nm!["GhostReaver"])
        {
            // L907
            condList.set(n!("Leeching"), true);
            // L908
            condList.set(n!("LeechingLife"), true);
        }
        // L910
        if modDB.Flag(None, nm!["CanLeechEnergyShieldOnFullEnergyShield"]) {
            // L911
            condList.set(n!("Leeching"), true);
            // L912
            condList.set(n!("LeechingEnergyShield"), true);
        }
        // L914
        if modDB.Flag(None, nm!["Condition:CanGainRage"])
            || modDB.Sum(ModType::BASE, None, nm!["RageRegen"]) > 0.0
        {
            // L915 LUA-GLOBAL-NIL: `skillCfg` is the nil global here (the local of 661 is out of
            // scope), so both queries run with a nil cfg (CCR-0004)
            let skillCfg: Option<&ModCfg> = None;
            let maxStacks = m_floor(
                modDB.Sum(ModType::BASE, skillCfg, nm!["MaximumRage"])
                    * modDB.More(skillCfg, nm!["MaximumRage"]),
            );
            // L916
            let minStacks = m_min(
                modDB.Sum(ModType::BASE, None, nm!["MinimumRage"]),
                maxStacks,
            );
            // L917
            let rageConfig = modDB.Sum(ModType::BASE, None, nm!["Multiplier:RageStack"]);
            // L918
            let stacks = m_max(
                m_min(rageConfig, maxStacks),
                if minStacks > 0.0 { minStacks } else { 0.0 },
            );
            // L919
            output.set(
                n!("RageEffect"),
                m_floor(stacks * calcLib::r#mod(&modDB, None, nm!["RageEffect"])),
            );
            // L920
            modDB.NewMod(
                n!("Multiplier:RageEffect"),
                ModType::BASE,
                val_to_value(&output.get(n!("RageEffect"))),
                "Base",
                0,
                0,
                &[],
            );
            // L921
            output.set(n!("Rage"), stacks);
            // L922
            output.set(n!("MaximumRage"), maxStacks);
            // L923
            modDB.NewMod(
                n!("Multiplier:Rage"),
                ModType::BASE,
                val_to_value(&output.get(n!("Rage"))),
                "Base",
                0,
                0,
                &[],
            );
            // L924
            if modDB.Flag(None, nm!["Condition:RageSpellDamage"]) {
                // L925
                modDB.NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    val_to_value(&output.get(n!("RageEffect"))),
                    "Rage",
                    ModFlag::Spell,
                    0,
                    &[],
                );
            } else {
                // L927
                modDB.NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    val_to_value(&output.get(n!("RageEffect"))),
                    "Rage",
                    ModFlag::Attack,
                    0,
                    &[],
                );
            }
            // L929
            if stacks == maxStacks {
                // L930
                modDB.NewMod(
                    n!("Condition:HaveMaximumRage"),
                    ModType::FLAG,
                    true,
                    "",
                    0,
                    0,
                    &[],
                );
            }
            // L932
            output.set(
                n!("InherentRageLossDelay"),
                2.0 + modDB.Sum(ModType::BASE, None, nm!["InherentRageLossDelay"]),
            );
            // L933
            output.set(
                n!("InherentRageLoss"),
                if !modDB.Flag(None, nm!["InherentRageLossIsPrevented"]) {
                    10.0 * (1.0 + modDB.Sum(ModType::INC, None, nm!["InherentRageLoss"]) / 100.0)
                } else {
                    0.0
                },
            );
        }
        // L935
        if env.configInput.num0(n!("multiplierManaBurnStacks")) > 0.0 {
            // L936
            let mut maxManaBurn = modDB.Sum(ModType::BASE, None, nm!["MaxManaBurnStacks"]);
            // L937
            if maxManaBurn == 0.0 {
                // L938
                maxManaBurn = 9999.0;
            }
            // L940
            let mut manaBurnStacks = m_min(
                env.configInput.num0(n!("multiplierManaBurnStacks")),
                maxManaBurn,
            );
            // L941
            modDB.NewMod(
                n!("Multiplier:ManaBurnStacks"),
                ModType::BASE,
                manaBurnStacks,
                "Config",
                0,
                0,
                &[],
            );
            // L942
            manaBurnStacks = manaBurnStacks
                + modDB.Sum(
                    ModType::BASE,
                    Some(&cfg_actor(n!("player"))),
                    nm!["EffectiveManaBurnStacks"],
                );
            // L943
            if modDB.Flag(None, nm!["Condition:WeepingWoundsInsteadOfManaBurn"]) {
                // L944
                modDB.NewMod(
                    n!("Multiplier:WeepingWoundsStacks"),
                    ModType::BASE,
                    manaBurnStacks,
                    "Config",
                    0,
                    0,
                    &[],
                );
            } else {
                // L946
                modDB.NewMod(
                    n!("Multiplier:EffectiveManaBurnStacks"),
                    ModType::BASE,
                    manaBurnStacks,
                    "Config",
                    0,
                    0,
                    &[],
                );
            }
        }
        // L949
        if modDB.Sum(ModType::BASE, None, nm!["CoveredInAshEffect"]) > 0.0 {
            // L950
            let effect = modDB.Sum(ModType::BASE, None, nm!["CoveredInAshEffect"]);
            // L951
            enemyDB.NewMod(
                n!("FireDamageTaken"),
                ModType::INC,
                m_min(effect, 20.0),
                "Covered in Ash",
                0,
                0,
                &[],
            );
        }
        // L953
        if modDB.Sum(ModType::BASE, None, nm!["CoveredInFrostEffect"]) > 0.0 {
            // L954
            let effect = modDB.Sum(ModType::BASE, None, nm!["CoveredInFrostEffect"]);
            // L955
            enemyDB.NewMod(
                n!("ColdDamageTaken"),
                ModType::INC,
                m_min(effect, 20.0),
                "Covered in Frost",
                0,
                0,
                &[],
            );
        }
        // L957
        if modDB.Flag(None, nm!["HasMalediction"]) {
            // L958
            modDB.NewMod(
                n!("DamageTaken"),
                ModType::INC,
                10.0,
                "Malediction",
                0,
                0,
                &[],
            );
            // L959
            modDB.NewMod(n!("Damage"), ModType::INC, -10.0, "Malediction", 0, 0, &[]);
        }
        // L961
        if modDB.Flag(None, nm!["HasMaddeningPresence"]) {
            // L962
            modDB.NewMod(
                n!("ActionSpeed"),
                ModType::INC,
                -10.0,
                "Maddening Presence",
                0,
                0,
                &[],
            );
            // L963
            modDB.NewMod(
                n!("Damage"),
                ModType::INC,
                -10.0,
                "Maddening Presence",
                0,
                0,
                &[],
            );
        }
        // L965
        if modDB.Flag(None, nm!["HasShapersPresence"]) {
            // L966
            modDB.NewMod(
                n!("BuffExpireFaster"),
                ModType::MORE,
                -20.0,
                "Shapers Presence",
                0,
                0,
                &[],
            );
        }
        // L968
        if modDB.Flag(None, nm!["Condition:CanHaveSoulEater"]) {
            // L969
            let max = lua_or_num(modDB.Override(None, nm!["SoulEaterMax"]), || {
                modDB.Sum(ModType::BASE, None, nm!["SoulEaterMax"])
            });
            // L970
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                5.0,
                "Base",
                ModFlag::Attack,
                0,
                &[tag!({ type = Multiplier, var = "SoulEaterStack", limit = max })],
            );
            // L971
            modDB.NewMod(
                n!("Speed"),
                ModType::INC,
                5.0,
                "Base",
                ModFlag::Cast,
                0,
                &[tag!({ type = Multiplier, var = "SoulEaterStack", limit = max })],
            );
        }
    }

    // L975 Process enemy modifiers
    // L976
    applyEnemyModifiers(actor, false);
}

/// @pob Modules/CalcPerform.lua:980-1139 h=d188062b
/// Process charges
pub(crate) fn doActorCharges(env: &EnvRef, actor: &ActorRef) {
    // L981
    let modDB = actor.modDB.get();
    // L982 ---@class Output
    // L983
    let output = actor.output.get();

    // L985 Calculate current and maximum charges
    // L986
    output.set(
        n!("PowerChargesMin"),
        m_max(modDB.Sum(ModType::BASE, None, nm!["PowerChargesMin"]), 0.0),
    );
    // L987
    output.set(
        n!("PowerChargesMax"),
        lua_or_val(modDB.Override(None, nm!["PowerChargesMax"]), || {
            Val::Num(m_max(
                modDB.Sum(ModType::BASE, None, nm!["PowerChargesMax"]),
                0.0,
            ))
        }),
    );
    // L988
    output.set(
        n!("PowerChargesDuration"),
        m_floor(
            modDB.Sum(ModType::BASE, None, nm!["ChargeDuration"])
                * calcLib::r#mod(&modDB, None, nm!["PowerChargesDuration", "ChargeDuration"]),
        ),
    );
    // L989
    if modDB.Flag(None, nm!["MaximumFrenzyChargesIsMaximumPowerCharges"]) {
        // L990
        let source = first_mod_source(&modDB, n!("MaximumFrenzyChargesIsMaximumPowerCharges"));
        // L991
        replace_mod_override(
            &modDB,
            n!("FrenzyChargesMax"),
            output.get(n!("PowerChargesMax")),
            source,
        );
    }
    // L993
    output.set(
        n!("FrenzyChargesMin"),
        m_max(modDB.Sum(ModType::BASE, None, nm!["FrenzyChargesMin"]), 0.0),
    );
    // L994
    output.set(
        n!("FrenzyChargesMax"),
        lua_or_val(modDB.Override(None, nm!["FrenzyChargesMax"]), || {
            Val::Num(m_max(
                if modDB.Flag(None, nm!["MaximumFrenzyChargesIsMaximumPowerCharges"]) {
                    lua_and_or_num(output.get(n!("PowerChargesMax")), || {
                        modDB.Sum(ModType::BASE, None, nm!["FrenzyChargesMax"])
                    })
                } else {
                    modDB.Sum(ModType::BASE, None, nm!["FrenzyChargesMax"])
                },
                0.0,
            ))
        }),
    );
    // L995
    output.set(
        n!("FrenzyChargesDuration"),
        m_floor(
            modDB.Sum(ModType::BASE, None, nm!["ChargeDuration"])
                * calcLib::r#mod(&modDB, None, nm!["FrenzyChargesDuration", "ChargeDuration"]),
        ),
    );
    // L996
    if modDB.Flag(None, nm!["MaximumEnduranceChargesIsMaximumFrenzyCharges"]) {
        // L997
        let source = first_mod_source(&modDB, n!("MaximumEnduranceChargesIsMaximumFrenzyCharges"));
        // L998
        replace_mod_override(
            &modDB,
            n!("EnduranceChargesMax"),
            output.get(n!("FrenzyChargesMax")),
            source,
        );
    }
    // L1000
    output.set(
        n!("EnduranceChargesMin"),
        m_max(
            modDB.Sum(ModType::BASE, None, nm!["EnduranceChargesMin"]),
            0.0,
        ),
    );
    // L1001
    output.set(
        n!("EnduranceChargesMax"),
        lua_or_val(modDB.Override(None, nm!["EnduranceChargesMax"]), || {
            let partyMembers = lua_index(env.partyMembers.get(), "partyMembers");
            Val::Num(m_max(
                if partyMembers
                    .modDB
                    .get()
                    .Flag(None, nm!["PartyMemberMaximumEnduranceChargesEqualToYours"])
                {
                    lua_and_or_num(
                        partyMembers.output.get().get(n!("EnduranceChargesMax")),
                        || enduranceMaxFallback(&modDB, &output),
                    )
                } else {
                    enduranceMaxFallback(&modDB, &output)
                },
                0.0,
            ))
        }),
    );
    // L1002
    output.set(
        n!("EnduranceChargesDuration"),
        m_floor(
            modDB.Sum(ModType::BASE, None, nm!["ChargeDuration"])
                * calcLib::r#mod(
                    &modDB,
                    None,
                    nm!["EnduranceChargesDuration", "ChargeDuration"],
                ),
        ),
    );
    // L1003
    output.set(
        n!("SiphoningChargesMax"),
        m_max(
            modDB.Sum(ModType::BASE, None, nm!["SiphoningChargesMax"]),
            0.0,
        ),
    );
    // L1004
    output.set(
        n!("ChallengerChargesMax"),
        m_max(
            modDB.Sum(ModType::BASE, None, nm!["ChallengerChargesMax"]),
            0.0,
        ),
    );
    // L1005
    output.set(
        n!("BlitzChargesMax"),
        m_max(modDB.Sum(ModType::BASE, None, nm!["BlitzChargesMax"]), 0.0),
    );
    // L1006
    output.set(
        n!("InspirationChargesMax"),
        m_max(
            modDB.Sum(ModType::BASE, None, nm!["InspirationChargesMax"]),
            0.0,
        ),
    );
    // L1007
    output.set(
        n!("CrabBarriersMax"),
        m_max(modDB.Sum(ModType::BASE, None, nm!["CrabBarriersMax"]), 0.0),
    );
    // L1008 `m_max(A and (B and EnduranceChargesMax or EnduranceChargesMin) or 0, 0)`
    output.set(
        n!("BrutalChargesMin"),
        m_max(
            if modDB.Flag(
                None,
                nm!["MinimumEnduranceChargesEqualsMinimumBrutalCharges"],
            ) {
                let inner = if modDB.Flag(
                    None,
                    nm!["MinimumEnduranceChargesIsMaximumEnduranceCharges"],
                ) {
                    lua_or_get(output.get(n!("EnduranceChargesMax")), || {
                        output.get(n!("EnduranceChargesMin"))
                    })
                } else {
                    output.get(n!("EnduranceChargesMin"))
                };
                inner.num_or(0.0)
            } else {
                0.0
            },
            0.0,
        ),
    );
    // L1009
    output.set(
        n!("BrutalChargesMax"),
        m_max(
            if modDB.Flag(
                None,
                nm!["MaximumEnduranceChargesEqualsMaximumBrutalCharges"],
            ) {
                output.get(n!("EnduranceChargesMax")).num_or(0.0)
            } else {
                0.0
            },
            0.0,
        ),
    );
    // L1010
    output.set(
        n!("BrineChargesMax"),
        m_max(output.num_or(n!("EnduranceChargesMax"), 0.0), 0.0),
    );
    // L1011
    output.set(
        n!("AbsorptionChargesMin"),
        m_max(
            if modDB.Flag(
                None,
                nm!["MinimumPowerChargesEqualsMinimumAbsorptionCharges"],
            ) {
                let inner = if modDB.Flag(None, nm!["MinimumPowerChargesIsMaximumPowerCharges"]) {
                    lua_or_get(output.get(n!("PowerChargesMax")), || {
                        output.get(n!("PowerChargesMin"))
                    })
                } else {
                    output.get(n!("PowerChargesMin"))
                };
                inner.num_or(0.0)
            } else {
                0.0
            },
            0.0,
        ),
    );
    // L1012
    output.set(
        n!("AbsorptionChargesMax"),
        m_max(
            if modDB.Flag(
                None,
                nm!["MaximumPowerChargesEqualsMaximumAbsorptionCharges"],
            ) {
                output.get(n!("PowerChargesMax")).num_or(0.0)
            } else {
                0.0
            },
            0.0,
        ),
    );
    // L1013
    output.set(
        n!("AfflictionChargesMin"),
        m_max(
            if modDB.Flag(
                None,
                nm!["MinimumFrenzyChargesEqualsMinimumAfflictionCharges"],
            ) {
                let inner = if modDB.Flag(None, nm!["MinimumFrenzyChargesIsMaximumFrenzyCharges"]) {
                    lua_or_get(output.get(n!("FrenzyChargesMax")), || {
                        output.get(n!("FrenzyChargesMin"))
                    })
                } else {
                    output.get(n!("FrenzyChargesMin"))
                };
                inner.num_or(0.0)
            } else {
                0.0
            },
            0.0,
        ),
    );
    // L1014
    output.set(
        n!("AfflictionChargesMax"),
        m_max(
            if modDB.Flag(
                None,
                nm!["MaximumFrenzyChargesEqualsMaximumAfflictionCharges"],
            ) {
                output.get(n!("FrenzyChargesMax")).num_or(0.0)
            } else {
                0.0
            },
            0.0,
        ),
    );
    // L1015
    output.set(
        n!("BloodChargesMax"),
        m_max(modDB.Sum(ModType::BASE, None, nm!["BloodChargesMax"]), 0.0),
    );
    // L1016
    output.set(
        n!("SpiritChargesMax"),
        m_max(modDB.Sum(ModType::BASE, None, nm!["SpiritChargesMax"]), 0.0),
    );
    // L1017
    output.set(
        n!("SpiritInfusionsMax"),
        modDB.Sum(ModType::BASE, None, nm!["SpiritInfusionsMax"])
            + (if modDB.Flag(None, nm!["Condition:CanGainSpiritInfusion"]) {
                10.0
            } else {
                0.0
            }),
    );

    // L1019 Initialize Charges
    // L1020
    output.set(n!("PowerCharges"), 0.0);
    // L1021
    output.set(n!("FrenzyCharges"), 0.0);
    // L1022
    output.set(n!("EnduranceCharges"), 0.0);
    // L1023
    output.set(n!("SiphoningCharges"), 0.0);
    // L1024
    output.set(n!("ChallengerCharges"), 0.0);
    // L1025
    output.set(n!("BlitzCharges"), 0.0);
    // L1026
    output.set(n!("InspirationCharges"), 0.0);
    // L1027
    output.set(n!("GhostShrouds"), 0.0);
    // L1028
    output.set(n!("BrutalCharges"), 0.0);
    // L1029
    output.set(n!("BrineCharges"), 0.0);
    // L1030
    output.set(n!("AbsorptionCharges"), 0.0);
    // L1031
    output.set(n!("AfflictionCharges"), 0.0);
    // L1032
    output.set(n!("BloodCharges"), 0.0);
    // L1033
    output.set(n!("SpiritCharges"), 0.0);
    // L1034
    output.set(n!("SpiritInfusions"), 0.0);

    // L1036 Conditionally over-write Charge values
    // L1037
    if modDB.Flag(None, nm!["MinimumFrenzyChargesIsMaximumFrenzyCharges"]) {
        // L1038
        output.set(n!("FrenzyChargesMin"), output.get(n!("FrenzyChargesMax")));
    }
    // L1040
    if modDB.Flag(
        None,
        nm!["MinimumEnduranceChargesIsMaximumEnduranceCharges"],
    ) {
        // L1041
        output.set(
            n!("EnduranceChargesMin"),
            output.get(n!("EnduranceChargesMax")),
        );
    }
    // L1043
    if modDB.Flag(None, nm!["MinimumPowerChargesIsMaximumPowerCharges"]) {
        // L1044
        output.set(n!("PowerChargesMin"), output.get(n!("PowerChargesMax")));
    }
    // L1046
    if modDB.Flag(None, nm!["UsePowerCharges"]) {
        // L1047
        output.set(
            n!("PowerCharges"),
            lua_or_val(modDB.Override(None, nm!["PowerCharges"]), || {
                output.get(n!("PowerChargesMax"))
            }),
        );
    }
    // L1049
    if modDB.Flag(None, nm!["PowerChargesConvertToAbsorptionCharges"]) {
        // L1050 we max with possible Power Charge Override from Config since Absorption Charges won't have their own config entry
        // L1051 and are converted from Power Charges
        // L1052
        output.set(
            n!("AbsorptionCharges"),
            m_max(
                output.num(n!("PowerCharges")),
                m_min(
                    output.num(n!("AbsorptionChargesMax")),
                    output.num(n!("AbsorptionChargesMin")),
                ),
            ),
        );
        // L1053
        output.set(n!("PowerCharges"), 0.0);
    } else {
        // L1055
        output.set(
            n!("PowerCharges"),
            m_max(
                output.num(n!("PowerCharges")),
                m_min(
                    output.num(n!("PowerChargesMax")),
                    output.num(n!("PowerChargesMin")),
                ),
            ),
        );
    }
    // L1057
    output.set(
        n!("RemovablePowerCharges"),
        m_max(
            output.num(n!("PowerCharges")) - output.num(n!("PowerChargesMin")),
            0.0,
        ),
    );
    // L1058
    if modDB.Flag(None, nm!["UseFrenzyCharges"]) {
        // L1059
        output.set(
            n!("FrenzyCharges"),
            lua_or_val(modDB.Override(None, nm!["FrenzyCharges"]), || {
                output.get(n!("FrenzyChargesMax"))
            }),
        );
    }
    // L1061
    if modDB.Flag(None, nm!["FrenzyChargesConvertToAfflictionCharges"]) {
        // L1062 we max with possible Power Charge Override from Config since Absorption Charges won't have their own config entry
        // L1063 and are converted from Power Charges
        // L1064
        output.set(
            n!("AfflictionCharges"),
            m_max(
                output.num(n!("FrenzyCharges")),
                m_min(
                    output.num(n!("AfflictionChargesMax")),
                    output.num(n!("AfflictionChargesMin")),
                ),
            ),
        );
        // L1065
        output.set(n!("FrenzyCharges"), 0.0);
    } else {
        // L1067
        output.set(
            n!("FrenzyCharges"),
            m_max(
                output.num(n!("FrenzyCharges")),
                m_min(
                    output.num(n!("FrenzyChargesMax")),
                    output.num(n!("FrenzyChargesMin")),
                ),
            ),
        );
    }
    // L1069
    output.set(
        n!("RemovableFrenzyCharges"),
        m_max(
            output.num(n!("FrenzyCharges")) - output.num(n!("FrenzyChargesMin")),
            0.0,
        ),
    );
    // L1070
    if modDB.Flag(None, nm!["UseEnduranceCharges"]) {
        // L1071
        output.set(
            n!("EnduranceCharges"),
            lua_or_val(modDB.Override(None, nm!["EnduranceCharges"]), || {
                output.get(n!("EnduranceChargesMax"))
            }),
        );
        // L1072
        if modDB.Flag(None, nm!["CanGainBrineCharges"]) {
            // L1073
            output.set(n!("BrineCharges"), output.get(n!("BrineChargesMax")));
        }
    }
    // L1076
    if modDB.Flag(None, nm!["EnduranceChargesConvertToBrutalCharges"]) {
        // L1077 we max with possible Endurance Charge Override from Config since Brutal Charges won't have their own config entry
        // L1078 and are converted from Endurance Charges
        // L1079
        output.set(
            n!("BrutalCharges"),
            m_max(
                output.num(n!("EnduranceCharges")),
                m_min(
                    output.num(n!("BrutalChargesMax")),
                    output.num(n!("BrutalChargesMin")),
                ),
            ),
        );
        // L1080
        output.set(n!("EnduranceCharges"), 0.0);
    } else {
        // L1082
        output.set(
            n!("EnduranceCharges"),
            m_max(
                output.num(n!("EnduranceCharges")),
                m_min(
                    output.num(n!("EnduranceChargesMax")),
                    output.num(n!("EnduranceChargesMin")),
                ),
            ),
        );
    }
    // L1084
    output.set(
        n!("RemovableEnduranceCharges"),
        m_max(
            output.num(n!("EnduranceCharges")) - output.num(n!("EnduranceChargesMin")),
            0.0,
        ),
    );
    // L1085
    if modDB.Flag(None, nm!["UseSiphoningCharges"]) {
        // L1086
        output.set(
            n!("SiphoningCharges"),
            lua_or_val(modDB.Override(None, nm!["SiphoningCharges"]), || {
                output.get(n!("SiphoningChargesMax"))
            }),
        );
    }
    // L1088
    if modDB.Flag(None, nm!["UseChallengerCharges"]) {
        // L1089
        output.set(
            n!("ChallengerCharges"),
            lua_or_val(modDB.Override(None, nm!["ChallengerCharges"]), || {
                output.get(n!("ChallengerChargesMax"))
            }),
        );
    }
    // L1091
    if modDB.Flag(None, nm!["UseBlitzCharges"]) {
        // L1092
        output.set(
            n!("BlitzCharges"),
            lua_or_val(modDB.Override(None, nm!["BlitzCharges"]), || {
                output.get(n!("BlitzChargesMax"))
            }),
        );
    }
    // L1094
    if Rc::ptr_eq(actor, &env.player) {
        // L1095
        output.set(
            n!("InspirationCharges"),
            lua_or_val(modDB.Override(None, nm!["InspirationCharges"]), || {
                output.get(n!("InspirationChargesMax"))
            }),
        );
    }
    // L1097
    if modDB.Flag(None, nm!["UseGhostShrouds"]) {
        // L1098
        output.set(
            n!("GhostShrouds"),
            lua_or_val(modDB.Override(None, nm!["GhostShrouds"]), || Val::Num(3.0)),
        );
    }
    // L1100
    output.set(
        n!("BloodCharges"),
        m_min(
            lua_or_get_num(modDB.Override(None, nm!["BloodCharges"]), || {
                output.get(n!("BloodChargesMax"))
            }),
            output.num(n!("BloodChargesMax")),
        ),
    );
    // L1101
    output.set(
        n!("SpiritCharges"),
        m_min(
            lua_or_num(modDB.Override(None, nm!["SpiritCharges"]), || 0.0),
            output.num(n!("SpiritChargesMax")),
        ),
    );
    // L1102
    output.set(
        n!("SpiritInfusions"),
        m_min(
            lua_or_num(modDB.Override(None, nm!["SpiritInfusion"]), || 0.0),
            output.num(n!("SpiritInfusionsMax")),
        ),
    );

    // L1105
    output.set(
        n!("CrabBarriers"),
        m_min(
            lua_or_get_num(modDB.Override(None, nm!["CrabBarriers"]), || {
                output.get(n!("CrabBarriersMax"))
            }),
            output.num(n!("CrabBarriersMax")),
        ),
    );
    // L1106
    if modDB.Flag(None, nm!["HaveMaximumPowerCharges"]) {
        // L1107
        output.set(n!("PowerCharges"), output.get(n!("PowerChargesMax")));
    }
    // L1109
    if modDB.Flag(None, nm!["HaveMaximumFrenzyCharges"]) {
        // L1110
        output.set(n!("FrenzyCharges"), output.get(n!("FrenzyChargesMax")));
    }
    // L1112
    if modDB.Flag(None, nm!["HaveMaximumEnduranceCharges"]) {
        // L1113
        output.set(
            n!("EnduranceCharges"),
            output.get(n!("EnduranceChargesMax")),
        );
    }
    // L1115
    output.set(
        n!("TotalCharges"),
        output.num(n!("PowerCharges"))
            + output.num(n!("FrenzyCharges"))
            + output.num(n!("EnduranceCharges")),
    );
    // L1116
    output.set(
        n!("RemovableTotalCharges"),
        output.num(n!("RemovableEnduranceCharges"))
            + output.num(n!("RemovableFrenzyCharges"))
            + output.num(n!("RemovablePowerCharges")),
    );
    // L1117
    modDB
        .multipliers
        .set(n!("PowerCharge"), output.get(n!("PowerCharges")));
    // L1118
    modDB
        .multipliers
        .set(n!("PowerChargeMax"), output.get(n!("PowerChargesMax")));
    // L1119
    modDB.multipliers.set(
        n!("RemovablePowerCharge"),
        output.get(n!("RemovablePowerCharges")),
    );
    // L1120
    modDB
        .multipliers
        .set(n!("FrenzyCharge"), output.get(n!("FrenzyCharges")));
    // L1121
    modDB.multipliers.set(
        n!("RemovableFrenzyCharge"),
        output.get(n!("RemovableFrenzyCharges")),
    );
    // L1122
    modDB
        .multipliers
        .set(n!("EnduranceCharge"), output.get(n!("EnduranceCharges")));
    // L1123
    modDB.multipliers.set(
        n!("RemovableEnduranceCharge"),
        output.get(n!("RemovableEnduranceCharges")),
    );
    // L1124
    modDB
        .multipliers
        .set(n!("TotalCharges"), output.get(n!("TotalCharges")));
    // L1125
    modDB.multipliers.set(
        n!("RemovableTotalCharges"),
        output.get(n!("RemovableTotalCharges")),
    );
    // L1126
    modDB
        .multipliers
        .set(n!("SiphoningCharge"), output.get(n!("SiphoningCharges")));
    // L1127
    modDB
        .multipliers
        .set(n!("ChallengerCharge"), output.get(n!("ChallengerCharges")));
    // L1128
    modDB
        .multipliers
        .set(n!("BlitzCharge"), output.get(n!("BlitzCharges")));
    // L1129
    modDB.multipliers.set(
        n!("InspirationCharge"),
        output.get(n!("InspirationCharges")),
    );
    // L1130
    modDB
        .multipliers
        .set(n!("GhostShroud"), output.get(n!("GhostShrouds")));
    // L1131
    modDB
        .multipliers
        .set(n!("CrabBarrier"), output.get(n!("CrabBarriers")));
    // L1132
    modDB
        .multipliers
        .set(n!("BrutalCharge"), output.get(n!("BrutalCharges")));
    // L1133
    modDB
        .multipliers
        .set(n!("BrineCharge"), output.get(n!("BrineCharges")));
    // L1134
    modDB
        .multipliers
        .set(n!("AbsorptionCharge"), output.get(n!("AbsorptionCharges")));
    // L1135
    modDB
        .multipliers
        .set(n!("AfflictionCharge"), output.get(n!("AfflictionCharges")));
    // L1136
    modDB
        .multipliers
        .set(n!("BloodCharge"), output.get(n!("BloodCharges")));
    // L1137
    modDB
        .multipliers
        .set(n!("SpiritCharge"), output.get(n!("SpiritCharges")));
    // L1138
    modDB
        .multipliers
        .set(n!("SpiritInfusion"), output.get(n!("SpiritInfusions")));
}

/// The `(Flag("MaximumEnduranceChargesIsMaximumFrenzyCharges") and output.FrenzyChargesMax or
/// Sum("BASE", nil, "EnduranceChargesMax"))` operand of CalcPerform.lua:1001.
fn enduranceMaxFallback(modDB: &ModDB, output: &Output) -> f64 {
    if modDB.Flag(None, nm!["MaximumEnduranceChargesIsMaximumFrenzyCharges"]) {
        lua_and_or_num(output.get(n!("FrenzyChargesMax")), || {
            modDB.Sum(ModType::BASE, None, nm!["EnduranceChargesMax"])
        })
    } else {
        modDB.Sum(ModType::BASE, None, nm!["EnduranceChargesMax"])
    }
}

/// `modDB.mods[name][1].source` (CalcPerform.lua:990/997): raises when the store holds no
/// such mod itself (a mod in the parent DB satisfies the `Flag` but is not in `modDB.mods`).
fn first_mod_source(modDB: &ModDB, name: Name) -> Option<pob_gamedata::AStr> {
    match modDB.mods_named(name).first() {
        Some(m) => m.source.clone(),
        None => lua_error!("attempt to index a nil value"),
    }
}

/// `modDB:ReplaceMod(name, "OVERRIDE", value, source)` (ModStore.lua:125-130) where `source`
/// may be nil: createMod then sets no source (ModTools.lua:38-41), which `ReplaceMod`'s
/// `&str` parameter cannot say, so the body is spelled out here.
fn replace_mod_override(modDB: &ModDB, name: Name, value: Val, source: Option<pob_gamedata::AStr>) {
    let value = val_to_value(&value);
    let m: ModRef = Arc::new(pob_gamedata::mod_tools::createMod(
        name,
        ModType::OVERRIDE,
        value,
        source.as_deref(),
        0,
        0,
        &[],
    ));
    if !modDB.ReplaceModInternal(m.clone()) {
        modDB.AddMod(m);
    }
}

/// @pob Modules/CalcPerform.lua:1141-1151 h=c94cebd4
pub fn actionSpeedMod(actor: &ActorRef) -> f64 {
    // L1142
    let modDB = actor.modDB.get();
    // L1143
    let minimumActionSpeed = modDB.Max(None, nm!["MinimumActionSpeed"]).unwrap_or(0.0);
    // L1144
    let maximumActionSpeedReduction = modDB.Max(None, nm!["MaximumActionSpeedReduction"]);
    // L1145
    let mut actionSpeedMod = 1.0
        + (m_max(
            -data().misc.TemporalChainsEffectCap,
            modDB.Sum(ModType::INC, None, nm!["TemporalChainsActionSpeed"]),
        ) + modDB.Sum(ModType::INC, None, nm!["ActionSpeed"]))
            / 100.0;
    // L1146
    actionSpeedMod = m_max(minimumActionSpeed / 100.0, actionSpeedMod);
    // L1147
    if let Some(maximumActionSpeedReduction) = maximumActionSpeedReduction {
        // L1148
        actionSpeedMod = m_min(
            (100.0 - maximumActionSpeedReduction) / 100.0,
            actionSpeedMod,
        );
    }
    // L1150
    actionSpeedMod
}

/// @pob Modules/CalcPerform.lua:1154-1244 h=fd93e0f2
/// Build the same modifier database for the selected minion and any minion used as a defensive Life pool.
///
/// CCR-0095 §1: the minion fields `type`, `lifeTable`, `itemSet` and `uses` (a table) have no
/// typed `Actor` field yet; they are read from `minion.extra` until the CCR lands.
pub(crate) fn initMinionModDB(env: &EnvRef, activeSkill: &ActiveSkillRef, output: Option<&Output>) {
    // L1155
    let modDB = env.modDB.get();
    // L1156
    let minion = lua_index(activeSkill.minion.get(), "minion");
    // L1157
    minion.output.set(match output {
        Some(o) => o.clone(),
        None => Table::new(),
    });
    // L1158
    minion
        .modDB
        .get()
        .multipliers
        .set(n!("Level"), minion.level.get());
    // L1159
    initModDB(env, &minion.modDB.get());
    // L1160
    let minionData = lua_index(minion.minionData.get(), "minionData");
    let mut baseLife =
        lua_level_index(&minion_lifeTable(&minion), minion.level.get()) * minionData.life;
    // L1161
    if minion.hostile.get() {
        // L1162
        baseLife = baseLife * map_level_life_mult(env);
    }
    // L1164
    minion.modDB.get().NewMod(
        n!("Life"),
        ModType::BASE,
        m_floor(baseLife),
        "Base",
        0,
        0,
        &[],
    );
    // L1165
    if let Some(energyShield) = minionData.energyShield {
        // L1166
        let esTable = if minion.hostile.get() {
            minion_lifeTable(&minion)
        } else {
            data_table(&env.data.monsterAllyLifeTable)
        };
        // L1167
        let mut baseES =
            lua_level_index(&esTable, minion.level.get()) * minionData.life * energyShield;
        // L1168
        if minion.hostile.get() {
            // L1169
            baseES = baseES * map_level_life_mult(env);
        }
        // L1171
        minion.modDB.get().NewMod(
            n!("EnergyShield"),
            ModType::BASE,
            m_floor(baseES),
            "Base",
            0,
            0,
            &[],
        );
    }
    // L1173
    minion.modDB.get().NewMod(
        n!("Armour"),
        ModType::BASE,
        round(
            lua_level_index(
                &data_table(&env.data.monsterArmourTable),
                minion.level.get(),
            ) * minionData.armour.unwrap_or(1.0),
            None,
        ),
        "Base",
        0,
        0,
        &[],
    );
    // L1174
    minion.modDB.get().NewMod(
        n!("Evasion"),
        ModType::BASE,
        round(
            lua_level_index(
                &data_table(&env.data.monsterEvasionTable),
                minion.level.get(),
            ) * minionData.evasion.unwrap_or(1.0),
            None,
        ),
        "Base",
        0,
        0,
        &[],
    );
    // L1175
    if modDB.Flag(None, nm!["MinionAccuracyEqualsAccuracy"]) {
        // L1176
        minion.modDB.get().NewMod(
            n!("Accuracy"),
            ModType::BASE,
            calcLib::val(&modDB, n!("Accuracy"), None)
                + calcLib::val(&modDB, n!("Dex"), None)
                    * lua_or_num(modDB.Override(None, nm!["DexAccBonusOverride"]), || {
                        data().misc.AccuracyPerDexBase
                    }),
            "Player",
            0,
            0,
            &[],
        );
    } else {
        // L1178
        minion.modDB.get().NewMod(
            n!("CannotBeEvaded"),
            ModType::FLAG,
            1.0,
            "Minion Attacks always hit",
            0,
            0,
            &[],
        );
    }
    let mc = |k: &str| env.data.monsterConstants.get(k);
    // L1180
    minion.modDB.get().NewMod(
        n!("CritMultiplier"),
        ModType::BASE,
        lua_arith(mc("base_critical_strike_multiplier")) - 100.0,
        "Base",
        0,
        0,
        &[],
    );
    // L1181
    minion.modDB.get().NewMod(
        n!("DotMultiplier"),
        ModType::BASE,
        mc("critical_ailment_dot_multiplier_+"),
        "Base",
        0,
        0,
        &[tag!({ type = Condition, var = "CriticalStrike" })],
    );
    // L1182
    minion.modDB.get().NewMod(
        n!("FireResist"),
        ModType::BASE,
        minionData.fireResist,
        "Base",
        0,
        0,
        &[],
    );
    // L1183
    minion.modDB.get().NewMod(
        n!("ColdResist"),
        ModType::BASE,
        minionData.coldResist,
        "Base",
        0,
        0,
        &[],
    );
    // L1184
    minion.modDB.get().NewMod(
        n!("LightningResist"),
        ModType::BASE,
        minionData.lightningResist,
        "Base",
        0,
        0,
        &[],
    );
    // L1185
    minion.modDB.get().NewMod(
        n!("ChaosResist"),
        ModType::BASE,
        minionData.chaosResist,
        "Base",
        0,
        0,
        &[],
    );
    // L1186
    minion.modDB.get().NewMod(
        n!("CritChance"),
        ModType::INC,
        mc("critical_strike_chance_+%_per_power_charge"),
        "Base",
        0,
        0,
        &[tag!({ type = Multiplier, var = "PowerCharge" })],
    );
    // L1187
    minion.modDB.get().NewMod(
        n!("Speed"),
        ModType::INC,
        mc("base_attack_speed_+%_per_frenzy_charge"),
        "Base",
        ModFlag::Attack,
        0,
        &[tag!({ type = Multiplier, var = "FrenzyCharge" })],
    );
    // L1188
    minion.modDB.get().NewMod(
        n!("Speed"),
        ModType::INC,
        mc("base_cast_speed_+%_per_frenzy_charge"),
        "Base",
        ModFlag::Cast,
        0,
        &[tag!({ type = Multiplier, var = "FrenzyCharge" })],
    );
    // L1189
    minion.modDB.get().NewMod(
        n!("Damage"),
        ModType::MORE,
        mc("object_inherent_damage_+%_final_per_frenzy_charge"),
        "Base",
        0,
        0,
        &[tag!({ type = Multiplier, var = "FrenzyCharge" })],
    );
    // L1190
    minion.modDB.get().NewMod(
        n!("PhysicalDamageReduction"),
        ModType::BASE,
        mc("physical_damage_reduction_%_per_endurance_charge"),
        "Base",
        0,
        0,
        &[tag!({ type = Multiplier, var = "EnduranceCharge" })],
    );
    // L1191
    minion.modDB.get().NewMod(
        n!("ElementalDamageReduction"),
        ModType::BASE,
        mc("elemental_damage_reduction_%_per_endurance_charge_if_player_minion"),
        "Base",
        0,
        0,
        &[tag!({ type = Multiplier, var = "EnduranceCharge" })],
    );
    // L1192
    minion
        .modDB
        .get()
        .NewMod(n!("ProjectileCount"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
    // L1193
    minion.modDB.get().NewMod(
        n!("MaximumFortification"),
        ModType::BASE,
        mc("base_max_fortification"),
        "Base",
        0,
        0,
        &[],
    );
    // L1194
    minion.modDB.get().NewMod(
        n!("Damage"),
        ModType::MORE,
        200.0,
        "Base",
        0,
        KeywordFlag::Bleed,
        &[tag!({ type = ActorCondition, actor = "enemy", var = "Moving" })],
    );
    // L1195
    for r#mod in minionData.modList.iter() {
        // L1196
        minion.modDB.get().AddMod(r#mod.clone());
    }
    // L1198
    for r#mod in activeSkill.extraSkillModList.get().snapshot() {
        // L1199
        minion.modDB.get().AddMod(r#mod);
    }
    // L1201
    if let Some(aegisModList) = env.aegisModList.get() {
        // L1202
        let aegisItem = env.aegisItem.get();
        minion.itemList.with_mut(|l| match aegisItem {
            Some(item) => {
                l.insert(n!("Weapon 3"), item);
            }
            None => {
                l.shift_remove(&n!("Weapon 3"));
            }
        });
        // L1203
        minion.modDB.get().AddList(&aegisModList.ipairs());
    }
    // L1205
    if let Some(theIronMass) = env.theIronMass.get() {
        if minion_type(&minion) == Some(n!("RaisedSkeleton")) {
            // L1206
            minion.modDB.get().AddList(&theIronMass.ipairs());
        }
    }
    // L1208
    if activeSkill
        .skillData
        .get()
        .truthy(n!("minionUseBowAndQuiver"))
    {
        // L1209
        if env
            .player
            .weaponData1
            .get()
            .get(n!("type"))
            .raw_eq(&Val::from("Bow"))
        {
            // L1210
            let weapon1 = lua_index(item_in(&env.player, n!("Weapon 1")), "Weapon 1");
            add_list_opt(&minion.modDB.get(), weapon1.slotModList[0].get());
        }
        // L1212
        if let Some(weapon2) = item_in(&env.player, n!("Weapon 2")) {
            if weapon2.r#type.get() == n!("Quiver") {
                // L1213 (a nil modList raises either way, ModStore.lua:99-107: at scale 1 the
                // ModDB `AddList(nil)` reaches `ipairs(nil)` (ModDB.lua:111-120 has no nil
                // guard), otherwise `#modList` raises)
                let scale = m_max(
                    modDB.Sum(ModType::BASE, None, nm!["WidowHailMultiplier"]),
                    1.0,
                );
                match weapon2.modList.get() {
                    Some(l) => minion.modDB.get().ScaleAddList(&l.ipairs(), scale, false),
                    None if scale == 1.0 => {
                        lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
                    }
                    None => lua_error!("attempt to get length of local 'modList' (a nil value)"),
                }
            }
        }
        // L1215
        if modDB.Flag(None, nm!["BlinkAndMirrorUseGloves"]) {
            if let Some(gloves) = item_in(&env.player, n!("Gloves")) {
                // L1216
                add_list_opt(&minion.modDB.get(), gloves.modList.get());
            }
        }
    }
    // L1219
    if activeSkill
        .skillData
        .get()
        .truthy(n!("minionUseMainHandWeapon"))
    {
        // L1220
        let weapon1 = lua_index(item_in(&env.player, n!("Weapon 1")), "Weapon 1");
        add_list_opt(&minion.modDB.get(), weapon1.slotModList[0].get());
    }
    // L1222
    // CCR-0095: `minion.itemSet` / `minion.uses` are read from `minion.extra`
    let itemSet = minion.extra.get(n!("itemSet"));
    let uses = minion.extra.get(n!("uses"));
    if itemSet.truthy() || uses.truthy() {
        // L1223 (LUA-ORDER: string keys; each slot fills its own itemList key, and the AddList
        // order only orders mods of the same name from different slots)
        let slots: Vec<(Name, Table)> = env
            .build
            .itemsTab
            .slots
            .with(|s| s.iter().map(|(k, v)| (*k, v.clone())).collect());
        for (slotName, slot) in slots {
            let mut slotName = slotName;
            // L1224
            if uses.get(slotName).truthy() {
                // L1225
                let item: Option<ItemRef>;
                // L1226
                if itemSet.truthy() {
                    // L1227
                    if slot.get(n!("weaponSet")).raw_eq(&Val::Num(1.0))
                        && itemSet.get(n!("useSecondWeaponSet")).truthy()
                    {
                        // L1228
                        slotName = cat!(slotName, " Swap");
                    }
                    // L1230
                    let selItemId = itemSet.get(slotName).get(n!("selItemId"));
                    item = lua_item_by_id(env, &selItemId);
                } else {
                    // L1232
                    item = item_in(&env.player, slotName);
                }
                // L1234
                if let Some(item) = item {
                    // L1235
                    minion.itemList.with_mut(|l| {
                        l.insert(slotName, item.clone());
                    });
                    // L1236
                    let list = match item.modList.get() {
                        Some(l) => Some(l),
                        None => match slot.get(n!("slotNum")).opt_num() {
                            Some(n) if n == 1.0 => item.slotModList[0].get(),
                            Some(n) if n == 2.0 => item.slotModList[1].get(),
                            _ => None,
                        },
                    };
                    add_list_opt(&minion.modDB.get(), list);
                }
            }
        }
    }
    // L1241
    if modDB.Sum(ModType::BASE, None, nm!["StrengthAddedToMinions"]) > 0.0 {
        // L1242
        minion.modDB.get().NewMod(
            n!("Str"),
            ModType::BASE,
            round(
                calcLib::val(&modDB, n!("Str"), None)
                    * modDB.Sum(ModType::BASE, None, nm!["StrengthAddedToMinions"])
                    / 100.0,
                None,
            ),
            "Player",
            0,
            0,
            &[],
        );
    }
}

/// @pob Modules/CalcPerform.lua:1247-1253 h=ad6137e8
/// Add modifiers granted to the minion by its skill or an active buff.
pub(crate) fn addMinionModifiers(modList: &ModList, skillCfg: Option<&ModCfg>, minion: &ActorRef) {
    // L1248
    for value in modList.List(skillCfg, nm!["MinionModifier"]) {
        // L1249
        let vtype = value.get(n!("type"));
        if !vtype.truthy()
            || minion_type(minion).map_or(false, |t| vtype.as_str() == Some(t.as_str()))
        {
            // L1250
            match value.get(n!("mod")) {
                Value::Mod(m) => minion.modDB.get().AddMod(m.clone()),
                _ => lua_error!("attempt to index a nil value (local 'mod')"),
            }
        }
    }
}

/// @pob Modules/CalcPerform.lua:1256-1272 h=093bd951
/// The ally life pool tables (CalcPerform.lua 1256-1272): `minionLifePoolBySkill`,
/// `spectreLifePool`, `companionshipLifePool`, `totemLifePoolByStat`, `allyLifeRedirects`.
/// Module-level locals: every call returns the same tables (PF-G compares pools by
/// identity, CalcPerform.lua:3705/3733/3742).
pub(crate) fn allyLifePools() -> Table {
    thread_local! {
        static POOLS: Table = build_ally_life_pools();
    }
    POOLS.with(Table::clone)
}

fn build_ally_life_pools() -> Table {
    // L1255 These are the minions whose Life can be spent before the player's.
    // L1256
    let minionLifePoolBySkill = Table::new();
    // L1257
    minionLifePoolBySkill.set(
        n!("Summon Sentinel of Radiance"),
        tbl! { life = "TotalRadianceSentinelLife", redirect = "takenFromRadianceSentinelBeforeYou", source = "Sentinel of Radiance" },
    );
    // L1258
    minionLifePoolBySkill.set(
        n!("Summon Void Spawn"),
        tbl! { life = "TotalVoidSpawnLife", redirect = "takenFromVoidSpawnBeforeYou", source = "Void Spawns", limit = "ActiveVoidSpawnLimit" },
    );
    // L1259
    minionLifePoolBySkill.set(
        n!("Summon Stone Golem of Safeguarding"),
        tbl! { life = "TotalStoneGolemLife", redirect = "takenFromStoneGolemBeforeYou", source = "Stone Golem" },
    );
    // L1261
    let spectreLifePool = tbl! { life = "TotalSpectreLife", redirect = "takenFromSpectresBeforeYou", source = "Spectres", limit = "ActiveSpectreLimit" };
    // L1262
    let companionshipLifePool =
        tbl! { life = "TotalMinionLife", redirect = "takenFromMinionBeforeYou", source = "Minion" };
    // L1263
    let totemLifePoolByStat = Table::new();
    // L1264
    totemLifePoolByStat.set(
        n!("TotalTotemLife"),
        tbl! { redirect = "takenFromTotemsBeforeYou", source = "Totem" },
    );
    // L1265
    totemLifePoolByStat.set(
        n!("TotalVaalRejuvenationTotemLife"),
        tbl! { redirect = "takenFromVaalRejuvenationTotemsBeforeYou", source = "Vaal Rejuvenation Totem" },
    );
    // L1267
    let allyLifeRedirects = Table::new();
    allyLifeRedirects.push(spectreLifePool.get(n!("redirect")));
    allyLifeRedirects.push(companionshipLifePool.get(n!("redirect")));
    // L1268 (the outer table is a sequence; LUA-ORDER: the inner pairs run over string keys,
    // PoB's order is random per process; the list only feeds `Sum(..., unpack(...))`)
    for lifePools in [&minionLifePoolBySkill, &totemLifePoolByStat] {
        // L1269
        for (_, pool) in lifePools.pairs() {
            // L1270
            allyLifeRedirects.push(pool.get(n!("redirect")));
        }
    }
    let t = Table::new();
    t.set(n!("minionLifePoolBySkill"), minionLifePoolBySkill);
    t.set(n!("spectreLifePool"), spectreLifePool);
    t.set(n!("companionshipLifePool"), companionshipLifePool);
    t.set(n!("totemLifePoolByStat"), totemLifePoolByStat);
    t.set(n!("allyLifeRedirects"), allyLifeRedirects);
    t
}

// ---- Rust-only plumbing ---------------------------------------------------------------------

/// `x.field` on a possibly-nil `x`: Lua raises "attempt to index a nil value".
fn lua_index<T>(x: Option<T>, field: &str) -> T {
    match x {
        Some(v) => v,
        None => lua_error!("attempt to index a nil value (field '{field}')"),
    }
}

/// `q or d` where `q` is a ModStore query result and the result is used as a number.
fn lua_or_num(v: Value, d: impl FnOnce() -> f64) -> f64 {
    if v.truthy() {
        Val::from_value(&v).num()
    } else {
        d()
    }
}

/// `q or d` where the result is stored as is (`output.X = modDB:Override(…) or …`).
fn lua_or_val(v: Value, d: impl FnOnce() -> Val) -> Val {
    if v.truthy() {
        Val::from_value(&v)
    } else {
        d()
    }
}

/// `q or output.X` used as a number (`m_min(q or output.X, …)`).
fn lua_or_get_num(v: Value, d: impl FnOnce() -> Val) -> f64 {
    lua_or_val(v, d).num()
}

/// `a or b` on two calc values.
fn lua_or_get(a: Val, b: impl FnOnce() -> Val) -> Val {
    if a.truthy() {
        a
    } else {
        b()
    }
}

/// `cond and a or b` where `cond` already held: `a` when truthy, else `b`, as a number.
fn lua_and_or_num(a: Val, b: impl FnOnce() -> f64) -> f64 {
    if a.truthy() {
        a.num()
    } else {
        b()
    }
}

/// `math.max` / `math.min` argument check (`lj_lib_checknum`): a number or a numeric
/// string, else "bad argument #n to 'max' (number expected, got nil)".
fn lua_checknum(v: &Value, arg: u32, fname: &str) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Str(s) => match pob_lua::tonumber(s) {
            Some(n) => n,
            None => lua_error!("bad argument #{arg} to '{fname}' (number expected, got string)"),
        },
        other => lua_error!(
            "bad argument #{arg} to '{fname}' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// `m_max(a, modDB:Override(…))`: `Override` falls off the end of `OverrideInternal` when no
/// mod matches (ModDB.lua:229-251, ModList.lua:177-197), i.e. returns *no value*, so the call
/// is `math.max(a)` = `a`; a found value is never nil or false.
fn lua_max_override(a: f64, v: Value) -> f64 {
    if v.is_nil() {
        a
    } else {
        m_max(a, lua_checknum(&v, 2, "max"))
    }
}

/// A number from data used in arithmetic (`nil` raises).
fn lua_arith(v: Option<f64>) -> f64 {
    match v {
        Some(n) => n,
        None => lua_error!("attempt to perform arithmetic on a nil value"),
    }
}

/// A calc value as a mod value (`NewMod`/`ReplaceMod` take it as is).
fn val_to_value(v: &Val) -> Value {
    match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(n) => Value::Num(*n),
        Val::Str(s) => Value::Str(pob_gamedata::AStr::from(&**s)),
        Val::Mod(m) => Value::Mod(m.clone()),
        Val::Data(d) => d.clone(),
        _ => lua_error!("PF-B: a table value cannot become a mod value here"),
    }
}

/// `{ actor = a }` (a fresh cfg literal).
fn cfg_actor(a: Name) -> ModCfg {
    let c = ModCfg::default();
    c.actor.set(Some(a));
    c
}

/// `{ source = s }` (a fresh cfg literal).
fn cfg_source(s: Name) -> ModCfg {
    let c = ModCfg::default();
    c.source.set(Some(s));
    c
}

/// `data.nonDamagingAilment[name]` (always present at the pin).
fn ailment<'a>(
    ailmentData: &'a indexmap::IndexMap<Name, pob_gamedata::AilmentData>,
    name: &str,
) -> &'a pob_gamedata::AilmentData {
    lua_index(ailmentData.get(&Name::from(name)), name)
}

/// A monster level table (`data.monster*Table`, a Lua sequence) as an indexable value.
fn data_table(t: &[f64]) -> Val {
    let out = Table::new();
    for v in t {
        out.push(*v);
    }
    Val::Table(out)
}

/// `t[level]` used in arithmetic: nil (a missing or non-integer level) raises.
fn lua_level_index(t: &Val, level: f64) -> f64 {
    t.get(level).num()
}

/// `(env.data.mapLevelLifeMult[env.enemyLevel] or 1)`.
fn map_level_life_mult(env: &EnvRef) -> f64 {
    let lvl = env.enemyLevel.get();
    if lvl.fract() == 0.0 {
        env.data
            .mapLevelLifeMult
            .get(&(lvl as i64))
            .copied()
            .unwrap_or(1.0)
    } else {
        1.0
    }
}

/// `minion.type` (CCR-0095 §1: no typed field; the value is in `minion.extra`).
fn minion_type(minion: &ActorRef) -> Option<Name> {
    // CCR-0095
    minion.extra.str(n!("type")).map(|s| Name::from(&*s))
}

/// `minion.lifeTable` (CCR-0095 §1: one of the `data.monster*LifeTable` sequences, kept in
/// `minion.extra` until a typed field exists).
fn minion_lifeTable(minion: &ActorRef) -> Val {
    // CCR-0095
    minion.extra.get(n!("lifeTable"))
}

/// `actor.itemList[slotName]`.
fn item_in(actor: &ActorRef, slotName: Name) -> Option<ItemRef> {
    actor.itemList.with(|l| l.get(&slotName).cloned())
}

/// `env.build.itemsTab.items[id]`.
fn lua_item_by_id(env: &EnvRef, id: &Val) -> Option<ItemRef> {
    let id = id.opt_num()?;
    if id < 0.0 || id.fract() != 0.0 || id > f64::from(u32::MAX) {
        return None;
    }
    env.build
        .itemsTab
        .items
        .with(|m| m.get(&(id as u32)).cloned())
}

/// `minion.modDB:AddList(list)` on a ModDB (CalcActiveSkill.lua:186). ModDBClass:AddList
/// (ModDB.lua:111-120) has no `if modList` guard (only ModListClass:AddList, ModList.lua:88-93,
/// has one), so a nil list raises in `ipairs`.
fn add_list_opt(modDB: &ModDB, list: Option<ModList>) {
    match list {
        Some(list) => modDB.AddList(&list.ipairs()),
        None => lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"),
    }
}

/// The crate-private helpers of this file, for the oracle tests (`crates/pob-oracle/tests/PF-B.rs`;
/// CCR-0095 §3). Same signatures, no logic of their own.
#[cfg(feature = "probe")]
pub mod probe_fns {
    use super::*;

    pub fn doActorMisc(env: &EnvRef, actor: &ActorRef) {
        super::doActorMisc(env, actor)
    }
    pub fn doActorCharges(env: &EnvRef, actor: &ActorRef) {
        super::doActorCharges(env, actor)
    }
    pub fn initMinionModDB(env: &EnvRef, activeSkill: &ActiveSkillRef, output: Option<&Output>) {
        super::initMinionModDB(env, activeSkill, output)
    }
    pub fn addMinionModifiers(modList: &ModList, skillCfg: Option<&ModCfg>, minion: &ActorRef) {
        super::addMinionModifiers(modList, skillCfg, minion)
    }
    pub fn allyLifePools() -> Table {
        super::allyLifePools()
    }
}

#[cfg(all(test, not(target_arch = "wasm32")))]
mod tests {
    use super::*;

    /// ModDBClass:AddList(nil) raises (ModDB.lua:111-120 has no nil guard).
    #[test]
    fn add_list_opt_nil_raises() {
        let db = ModStore::ModDB(None);
        let r = pob_lua::error::catch(|| add_list_opt(&db, None));
        assert_eq!(
            r.unwrap_err().0,
            "bad argument #1 to 'ipairs' (table expected, got nil)"
        );
        let list = ModStore::ModList(None);
        assert!(pob_lua::error::catch(|| add_list_opt(&db, Some(list))).is_ok());
    }
}
