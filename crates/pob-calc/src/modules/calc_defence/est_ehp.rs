//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! defence segment DF-F (CalcDefence.lua 2612-3164): EHP.
//!
//! Owner: DF-F. `numberOfHitsToDie` (2612-2778) is the segment's local
//! function; it does not cross a segment boundary (calc doc §7.1), so it is a
//! private fn taking the ctx for its upvalues (`modDB`, `output`, `actor`).
//! The rest of the range (2779-3164) is `seg`.
#![allow(non_snake_case)]
// literal Lua statement shapes (`x = x + y`, `if … then return … else … end`,
// the `numHits ~= numHits` NaN test 2773, the duplicate `"n "` arms of 2898/2942)
#![allow(
    clippy::assign_op_pattern,
    clippy::redundant_else,
    clippy::eq_op,
    clippy::if_same_then_else
)]

use super::helpers::{buildAllyLifePools, reducePoolsByDamage};
use super::DefEstCtx;
use crate::modules::calc_perform_shared::actionSpeedMod;
use crate::prelude::*;

// Module-level locals of CalcDefence.lua (24, 28) used by this segment. Their
// `@pob` marker is DF-A's (helpers.rs, 7-43); these are private copies.
/// `isElemental` (CalcDefence.lua:24).
fn isElemental(damageType: Name) -> bool {
    damageType == n!("Fire") || damageType == n!("Cold") || damageType == n!("Lightning")
}

/// `dmgTypeList` (CalcDefence.lua:28).
fn dmgTypeList() -> [Name; 5] {
    [
        n!("Physical"),
        n!("Lightning"),
        n!("Cold"),
        n!("Fire"),
        n!("Chaos"),
    ]
}

/// `v == x` for a Lua value and a number (no coercion, never raises).
fn eq_num(v: &Val, x: f64) -> bool {
    matches!(v, Val::Num(n) if *n == x)
}

/// A number operand of `<`/`>` (anything else raises, as Lua does).
fn cmp_num(v: &Val) -> f64 {
    match v {
        Val::Num(n) => *n,
        other => lua_error!("attempt to compare {} with number", other.lua_type()),
    }
}

/// `t[k]` as an operand of `<`/`>`.
fn cmp(t: &Table, k: impl Into<Key>) -> f64 {
    cmp_num(&t.get(k))
}

/// `(a and "n " or (b and "n " or " "))..damageCategoryConfig` (2898, 2942).
fn article(damageCategoryConfig: Name) -> Name {
    cat!(
        if damageCategoryConfig == n!("Average") {
            "n "
        } else if damageCategoryConfig == n!("Untyped") {
            "n "
        } else {
            " "
        },
        damageCategoryConfig
    )
}

/// helper function that iteratively reduces pools until life hits 0 to determine the number of hits it would take with given damage to die
/// @pob Modules/CalcDefence.lua:2612-2778 h=128c3ae5
fn numberOfHitsToDie(c: &DefEstCtx, DamageIn: &Table) -> f64 {
    let modDB = &c.modDB;
    let output = &c.output;
    let actor = &c.actor;
    let misc = &data().misc;
    // L2614
    let mut numHits = 0.0;
    // L2615
    DamageIn.set(n!("cycles"), lor(DamageIn.get(n!("cycles")), || Val::Num(1.0)));
    // L2616
    DamageIn.set(
        n!("iterations"),
        lor(DamageIn.get(n!("iterations")), || Val::Num(0.0)),
    );

    // check damage in isn't 0 and that ward doesn't mitigate all damage
    // L2619
    for damageType in dmgTypeList() {
        // L2620
        numHits = numHits + DamageIn.num(damageType);
    }
    // L2622
    if numHits == 0.0 {
        // L2623
        return m_huge;
    }

    // L2626
    let wardAvoidBreakChance = if modDB.Flag(None, nm!["Condition:WardNotBreak"]) {
        1.0
    } else {
        m_min(modDB.Sum(ModType::BASE, None, nm!["WardAvoidBreakChance"]) / 100.0, 1.0)
    };
    // L2627
    if wardAvoidBreakChance == 1.0
        && cmp(output, n!("Ward")) > 0.0
        && numHits < cmp(output, n!("Ward"))
    {
        // L2628
        return m_huge;
    } else {
        // L2630
        numHits = 0.0;
    }

    // L2633
    let mut ward = output.num0(n!("Ward"));
    // don't apply non-perma ward for speed up calcs as it won't zero it correctly per hit
    // L2635
    if wardAvoidBreakChance < 1.0 && cmp(DamageIn, n!("cycles")) > 1.0 {
        // L2636
        ward = 0.0;
    }
    // L2638
    let aegis = Table::new();
    // L2639
    aegis.set(n!("shared"), output.num0(n!("sharedAegis")));
    // L2640
    aegis.set(n!("sharedElemental"), output.num0(n!("sharedElementalAegis")));
    // L2641
    let guard = Table::new();
    // L2642
    guard.set(n!("shared"), output.num0(n!("sharedGuardAbsorb")));
    // L2643
    for damageType in dmgTypeList() {
        // L2644
        aegis.set(damageType, output.num0(cat!(damageType, "Aegis")));
        // L2645
        guard.set(damageType, output.num0(cat!(damageType, "GuardAbsorb")));
    }
    // L2647
    let alliesTakenBeforeYou = buildAllyLifePools(output);

    // L2649
    let mut poolTable = tbl! {
        AlliesTakenBeforeYou = alliesTakenBeforeYou,
        Aegis = aegis,
        Guard = guard,
        Ward = ward,
        EnergyShield = output.get(n!("EnergyShieldRecoveryCap")),
        Mana = output.num0(n!("ManaUnreserved")),
        Life = output.num0(n!("LifeRecoverable")),
        LifeLossLostOverTime = output.num0(n!("LifeLossLostOverTime")),
        LifeBelowHalfLossLostOverTime = output.num0(n!("LifeBelowHalfLossLostOverTime")),
        damageTakenThatCanBeRecouped = Table::new(),
    };

    // L2662
    if eq_num(&DamageIn.get(n!("cycles")), 1.0) {
        // L2663
        DamageIn.set(
            n!("TrackRecoupable"),
            lor(DamageIn.get(n!("TrackRecoupable")), || Val::Bool(false)),
        );
        // L2664
        DamageIn.set(
            n!("TrackLifeLossOverTime"),
            lor(DamageIn.get(n!("TrackLifeLossOverTime")), || Val::Bool(false)),
        );
    } else {
        // L2666
        DamageIn.set(n!("TrackRecoupable"), false);
        // L2667
        DamageIn.set(n!("TrackLifeLossOverTime"), false);
    }
    // L2669 (`… or 0` is dead: Sum always returns a number)
    DamageIn.set(
        n!("WardBypass"),
        lor(DamageIn.get(n!("WardBypass")), || {
            Val::Num(modDB.Sum(ModType::BASE, None, nm!["WardBypass"]))
        }),
    );

    // L2671
    let mut VaalArcticArmourHitsLeft = output.get(n!("VaalArcticArmourLife"));
    // L2672
    if cmp(DamageIn, n!("cycles")) > 1.0 {
        // L2673
        VaalArcticArmourHitsLeft = Val::Num(0.0);
    }

    // L2676
    let mut iterationMultiplier = 1.0;
    // L2677
    let mut damageTotal = 0.0;
    // L2678
    let maxDamage = misc.ehpCalcMaxDamage;
    // L2679
    let maxIterations = misc.ehpCalcMaxIterationsToCalc;
    // L2680
    while cmp(&poolTable, n!("Life")) > 0.0 && cmp(DamageIn, n!("iterations")) < maxIterations {
        // L2681
        DamageIn.set(n!("iterations"), DamageIn.num(n!("iterations")) + 1.0);
        // L2682
        let mut Damage = Table::new();
        // L2683
        damageTotal = 0.0;
        // L2684
        let VaalArcticArmourMultiplier = if cmp_num(&VaalArcticArmourHitsLeft) > 0.0 {
            1.0 - output.num(n!("VaalArcticArmourMitigation"))
                * m_min(VaalArcticArmourHitsLeft.num() / iterationMultiplier, 1.0)
        } else {
            1.0
        };
        // L2685
        VaalArcticArmourHitsLeft = Val::Num(VaalArcticArmourHitsLeft.num() - iterationMultiplier);
        // L2686
        for damageType in dmgTypeList() {
            // L2687
            let damage = DamageIn.num0(damageType);
            // L2688
            Damage.set(
                damageType,
                if damage > 0.0 {
                    Val::Num(damage * iterationMultiplier * VaalArcticArmourMultiplier)
                } else {
                    Val::Nil
                },
            );
            // L2689
            damageTotal = damageTotal + damage;
        }
        // L2691
        if (DamageIn.truthy(n!("GainWhenHit"))
            || DamageIn.truthy(n!("MissingLifeBeforeEnemyHit"))
            || DamageIn.truthy(n!("MissingManaBeforeEnemyHit")))
            && (iterationMultiplier > 1.0 || cmp(DamageIn, n!("cycles")) > 1.0)
        {
            // L2692
            let gainMult = iterationMultiplier * DamageIn.num(n!("cycles"));
            // L2693
            if DamageIn.truthy(n!("MissingLifeBeforeEnemyHit")) {
                // L2694
                let v = m_min(
                    poolTable.num(n!("Life"))
                        + DamageIn.num(n!("MissingLifeBeforeEnemyHit"))
                            * (output.num0(n!("LifeUnreserved")) - poolTable.num(n!("Life")))
                            * (gainMult - 1.0)
                            / 100.0,
                    gainMult * output.num(n!("LifeRecoverable")),
                );
                poolTable.set(n!("Life"), v);
            }
            // L2696
            if DamageIn.truthy(n!("MissingManaBeforeEnemyHit")) {
                // L2697
                let v = m_min(
                    poolTable.num(n!("Mana"))
                        + DamageIn.num(n!("MissingManaBeforeEnemyHit"))
                            * (output.num0(n!("ManaUnreserved")) - poolTable.num(n!("Mana")))
                            * (gainMult - 1.0)
                            / 100.0,
                    gainMult * output.num0(n!("ManaUnreserved")),
                );
                poolTable.set(n!("Mana"), v);
            }
            // L2699
            if DamageIn.truthy(n!("GainWhenHit")) {
                // L2700
                let v = m_min(
                    poolTable.num(n!("Life")) + DamageIn.num(n!("LifeWhenHit")) * (gainMult - 1.0),
                    gainMult * output.num0(n!("LifeRecoverable")),
                );
                poolTable.set(n!("Life"), v);
                // L2701
                let v = m_min(
                    poolTable.num(n!("Mana")) + DamageIn.num(n!("ManaWhenHit")) * (gainMult - 1.0),
                    gainMult * output.num0(n!("ManaUnreserved")),
                );
                poolTable.set(n!("Mana"), v);
                // L2702
                let v = m_min(
                    poolTable.num(n!("EnergyShield"))
                        + DamageIn.num(n!("EnergyShieldWhenHit")) * (gainMult - 1.0),
                    gainMult * output.num(n!("EnergyShieldRecoveryCap")),
                );
                poolTable.set(n!("EnergyShield"), v);
            }
        }
        // L2705
        if DamageIn.truthy(n!("MissingLifeBeforeEnemyHit")) && cmp(&poolTable, n!("Life")) > 0.0 {
            // L2706
            let v = m_min(
                poolTable.num(n!("Life"))
                    + DamageIn.num(n!("MissingLifeBeforeEnemyHit"))
                        * (output.num0(n!("LifeUnreserved")) - poolTable.num(n!("Life")))
                        / 100.0,
                output.num0(n!("LifeRecoverable")),
            );
            poolTable.set(n!("Life"), v);
        }
        // L2708
        if DamageIn.truthy(n!("MissingManaBeforeEnemyHit")) && cmp(&poolTable, n!("Mana")) > 0.0 {
            // L2709
            let v = m_min(
                poolTable.num(n!("Mana"))
                    + DamageIn.num(n!("MissingManaBeforeEnemyHit"))
                        * (output.num0(n!("ManaUnreserved")) - poolTable.num(n!("Mana")))
                        / 100.0,
                output.num0(n!("ManaUnreserved")),
            );
            poolTable.set(n!("Mana"), v);
        }
        // L2711
        poolTable = reducePoolsByDamage(Some(&poolTable), &Damage, actor);

        // If still living and the amount of damage exceeds maximum threshold we survived infinite number of hits.
        // L2714
        if cmp(&poolTable, n!("Life")) > 0.0 && damageTotal >= maxDamage {
            // L2715
            return m_huge;
        }
        // L2717
        if DamageIn.truthy(n!("GainWhenHit")) && cmp(&poolTable, n!("Life")) > 0.0 {
            // L2718
            let v = m_min(
                poolTable.num(n!("Life")) + DamageIn.num(n!("LifeWhenHit")),
                output.num0(n!("LifeRecoverable")),
            );
            poolTable.set(n!("Life"), v);
            // L2719
            let v = m_min(
                poolTable.num(n!("Mana")) + DamageIn.num(n!("ManaWhenHit")),
                output.num0(n!("ManaUnreserved")),
            );
            poolTable.set(n!("Mana"), v);
            // L2720
            let v = m_min(
                poolTable.num(n!("EnergyShield")) + DamageIn.num(n!("EnergyShieldWhenHit")),
                output.num(n!("EnergyShieldRecoveryCap")),
            );
            poolTable.set(n!("EnergyShield"), v);
        }
        // L2722
        iterationMultiplier = 1.0;
        // to speed it up, run recursively but accelerated
        // MoM/life-loss-prevention mechanics can collapse too many hits into one
        // resulting in eHP jumps so we slow the acceleration.
        // L2726
        let speedUp = if DamageIn.truthy(n!("LimitEHPSpeedup")) {
            4.0
        } else {
            misc.ehpCalcSpeedUp
        };
        // L2727
        DamageIn.set(
            n!("cyclesRan"),
            lor(DamageIn.get(n!("cyclesRan")), || Val::Bool(false)),
        );
        // L2728
        let wardAvoidBreakActive = wardAvoidBreakChance < 1.0
            && cmp_num(&lor(poolTable.get(n!("WardActiveChance")), || Val::Num(0.0))) > 0.01;
        // L2729
        if !DamageIn.truthy(n!("cyclesRan"))
            && !wardAvoidBreakActive
            && cmp(&poolTable, n!("Life")) > 0.0
            && cmp(DamageIn, n!("iterations")) < maxIterations
        {
            // L2730
            Damage = Table::new();
            // L2731
            for damageType in dmgTypeList() {
                // L2732
                Damage.set(damageType, DamageIn.num(damageType) * speedUp);
            }
            // L2734
            Damage.set(n!("LimitEHPSpeedup"), DamageIn.get(n!("LimitEHPSpeedup")));
            // L2735
            if DamageIn.truthy(n!("GainWhenHit")) {
                // L2736
                Damage.set(n!("GainWhenHit"), true);
                // L2737
                Damage.set(n!("LifeWhenHit"), DamageIn.get(n!("LifeWhenHit")));
                // L2738
                Damage.set(n!("ManaWhenHit"), DamageIn.get(n!("ManaWhenHit")));
                // L2739
                Damage.set(
                    n!("EnergyShieldWhenHit"),
                    DamageIn.get(n!("EnergyShieldWhenHit")),
                );
            }
            // L2741
            Damage.set(n!("cycles"), DamageIn.num(n!("cycles")) * speedUp);
            // L2742
            Damage.set(n!("iterations"), DamageIn.get(n!("iterations")));
            // L2743
            iterationMultiplier = m_max((numberOfHitsToDie(c, &Damage) - 1.0) * speedUp - 1.0, 1.0);
            // L2744
            if iterationMultiplier == m_huge {
                // avoid unnecessary calculations if we know we survive infinite hits.
                // L2745
                return m_huge;
            }
            // L2747
            DamageIn.set(n!("iterations"), Damage.get(n!("iterations")));
            // L2748
            DamageIn.set(n!("cyclesRan"), true);
        }
        // L2750
        numHits = numHits + iterationMultiplier;
    }
    // L2752
    if DamageIn.truthy(n!("TrackRecoupable")) {
        let recouped = match poolTable.get(n!("damageTakenThatCanBeRecouped")) {
            Val::Table(t) => t,
            other => lua_error!(
                "bad argument #1 to 'pairs' (table expected, got {})",
                other.lua_type()
            ),
        };
        // L2753
        for (damageType, recoupable) in recouped.pairs() {
            // L2754
            let k = match damageType {
                Key::S(s) => cat!(s, "RecoupableDamageTaken"),
                Key::I(i) => cat!(i, "RecoupableDamageTaken"),
                Key::B(_) => lua_error!("attempt to concatenate a boolean value"),
                Key::F(f) => cat!(f.get(), "RecoupableDamageTaken"),
            };
            let v = output.num(k) + recoupable.num();
            output.set(k, v);
        }
    }
    // L2757
    if DamageIn.truthy(n!("TrackLifeLossOverTime")) {
        // L2758
        let v = output.num(n!("LifeLossLostOverTime")) + poolTable.num(n!("LifeLossLostOverTime"));
        output.set(n!("LifeLossLostOverTime"), v);
        // L2759
        let v = output.num(n!("LifeBelowHalfLossLostOverTime"))
            + poolTable.num(n!("LifeBelowHalfLossLostOverTime"));
        output.set(n!("LifeBelowHalfLossLostOverTime"), v);
    }

    // L2762
    if eq_num(&poolTable.get(n!("Life")), 0.0) && eq_num(&DamageIn.get(n!("cycles")), 1.0) {
        // Don't count overkill damage and only on final pass as to not break speedup.
        // L2763
        numHits = numHits - poolTable.num(n!("OverkillDamage")) / damageTotal;
    }
    // Recalculate total hit damage
    // L2766
    damageTotal = 0.0;
    // L2767
    for damageType in dmgTypeList() {
        // L2768
        damageTotal = damageTotal + DamageIn.num(damageType) * numHits;
    }
    // L2770
    if cmp(&poolTable, n!("Life")) >= 0.0 && damageTotal >= maxDamage {
        // If still living and the amount of damage exceeds maximum threshold we survived infinite number of hits.
        // L2771
        return m_huge;
    }
    // L2773
    if numHits != numHits {
        // L2774
        return 0.0;
    }
    // L2776
    m_max(numHits, 0.0)
}

/// @pob Modules/CalcDefence.lua:2779-3164 h=7f4fe209
/// DF-F: CalcDefence.lua 2612-3164 (2612-2778 is `numberOfHitsToDie`).
pub fn seg(ctx: &mut DefEstCtx) -> Flow {
    let c: &DefEstCtx = ctx;
    let env = &c.env;
    let actor = &c.actor;
    let modDB = &c.modDB;
    let enemyDB = &c.enemyDB;
    let output = &c.output;
    let breakdown = c.breakdown.as_ref();
    let damageCategoryConfig = c.damageCategoryConfig;
    let misc = &data().misc;
    let configInput = &env.configInput;

    // L2779
    if damageCategoryConfig != n!("DamageOverTime") {
        // number of damaging hits needed to be taken to die
        // L2781
        {
            // L2782
            let DamageIn = Table::new();
            // L2783
            for damageType in dmgTypeList() {
                // L2784
                DamageIn.set(damageType, output.get(cat!(damageType, "TakenHit")));
            }
            // L2786
            DamageIn.set(
                n!("LimitEHPSpeedup"),
                cmp(output, n!("preventedLifeLossTotal")) > 0.0,
            );
            // L2787
            let v = numberOfHitsToDie(c, &DamageIn);
            output.set(n!("NumberOfDamagingHits"), v);
        }

        // L2790
        {
            // L2791
            let DamageIn = Table::new();
            // L2792
            let mut BlockChance = output.num(n!("EffectiveBlockChance")) / 100.0;
            // L2793
            if damageCategoryConfig != n!("Melee") && damageCategoryConfig != n!("Untyped") {
                // L2794
                BlockChance =
                    output.num(cat!("Effective", damageCategoryConfig, "BlockChance")) / 100.0;
            }
            // L2796
            if enemyDB.Flag(None, nm!["CannotBeBlocked"]) {
                // L2797
                BlockChance = 0.0;
            }
            // L2799
            let blockEffect = 1.0 - BlockChance * output.num(n!("BlockEffect")) / 100.0;
            // L2800
            let mut suppressChance = 0.0;
            // L2801
            let mut suppressionEffect = 1.0;
            // L2802
            let mut ExtraAvoidChance = 0.0;
            // L2803
            let mut averageAvoidChance = 0.0;
            // L2804
            if !configInput.truthy(n!("DisableEHPGainOnBlock"))
                && cmp(output, n!("NumberOfDamagingHits")) > 1.0
            {
                // L2805
                DamageIn.set(n!("LifeWhenHit"), output.num(n!("LifeOnBlock")) * BlockChance);
                // L2806
                DamageIn.set(n!("ManaWhenHit"), output.num(n!("ManaOnBlock")) * BlockChance);
                // L2807
                DamageIn.set(
                    n!("EnergyShieldWhenHit"),
                    output.num(n!("EnergyShieldOnBlock")) * BlockChance,
                );
                // L2808
                if damageCategoryConfig == n!("Spell") || damageCategoryConfig == n!("SpellProjectile")
                {
                    // L2809
                    let v = DamageIn.num(n!("EnergyShieldWhenHit"))
                        + output.num(n!("EnergyShieldOnSpellBlock")) * BlockChance;
                    DamageIn.set(n!("EnergyShieldWhenHit"), v);
                } else if damageCategoryConfig == n!("Average") {
                    // L2811
                    let v = DamageIn.num(n!("EnergyShieldWhenHit"))
                        + output.num(n!("EnergyShieldOnSpellBlock")) / 2.0 * BlockChance;
                    DamageIn.set(n!("EnergyShieldWhenHit"), v);
                }
            }
            // suppression
            // L2815
            if damageCategoryConfig == n!("Spell")
                || damageCategoryConfig == n!("SpellProjectile")
                || damageCategoryConfig == n!("Average")
            {
                // L2816
                suppressChance = output.num(n!("EffectiveSpellSuppressionChance")) / 100.0;
            }
            // We include suppression in damage reduction if it is 100% otherwise we handle it here.
            // L2819
            if suppressChance < 1.0 {
                // L2820
                if damageCategoryConfig == n!("Average") {
                    // L2821
                    suppressChance = suppressChance / 2.0;
                }
                // L2823
                let v = DamageIn.num0(n!("EnergyShieldWhenHit"))
                    + output.num(n!("EnergyShieldOnSuppress")) * suppressChance;
                DamageIn.set(n!("EnergyShieldWhenHit"), v);
                // L2824
                let v = DamageIn.num0(n!("LifeWhenHit"))
                    + output.num(n!("LifeOnSuppress")) * suppressChance;
                DamageIn.set(n!("LifeWhenHit"), v);
                // L2825
                suppressionEffect =
                    1.0 - suppressChance * output.num(n!("SpellSuppressionEffect")) / 100.0;
            } else {
                // L2827
                let v = DamageIn.num0(n!("EnergyShieldWhenHit"))
                    + output.num(n!("EnergyShieldOnSuppress"))
                        * (if damageCategoryConfig == n!("Average") { 0.5 } else { 1.0 });
                DamageIn.set(n!("EnergyShieldWhenHit"), v);
                // L2828
                let v = DamageIn.num0(n!("LifeWhenHit"))
                    + output.num(n!("LifeOnSuppress"))
                        * (if damageCategoryConfig == n!("Average") { 0.5 } else { 1.0 });
                DamageIn.set(n!("LifeWhenHit"), v);
            }
            // extra avoid chance
            // L2831
            if damageCategoryConfig == n!("Projectile") || damageCategoryConfig == n!("SpellProjectile")
            {
                // L2832
                ExtraAvoidChance = ExtraAvoidChance + output.num(n!("AvoidProjectilesChance"));
            } else if damageCategoryConfig == n!("Average") {
                // L2834
                ExtraAvoidChance =
                    ExtraAvoidChance + output.num(n!("AvoidProjectilesChance")) / 2.0;
            }
            // gain when hit (currently just gain on block/suppress, and Defiance of Destiny)
            // L2837
            if !configInput.truthy(n!("DisableEHPGainOnBlock"))
                && cmp(output, n!("NumberOfDamagingHits")) > 1.0
            {
                // L2838
                DamageIn.set(
                    n!("MissingLifeBeforeEnemyHit"),
                    modDB.Sum(ModType::BASE, None, nm!["MissingLifeBeforeEnemyHit"]),
                );
                // L2839
                DamageIn.set(
                    n!("MissingManaBeforeEnemyHit"),
                    modDB.Sum(ModType::BASE, None, nm!["MissingManaBeforeEnemyHit"]),
                );
                // L2840
                if DamageIn.num0(n!("LifeWhenHit")) != 0.0
                    || DamageIn.num0(n!("ManaWhenHit")) != 0.0
                    || !eq_num(&DamageIn.get(n!("EnergyShieldWhenHit")), 0.0)
                    || !eq_num(&DamageIn.get(n!("MissingLifeBeforeEnemyHit")), 0.0)
                    || !eq_num(&DamageIn.get(n!("MissingManaBeforeEnemyHit")), 0.0)
                {
                    // L2841
                    DamageIn.set(n!("GainWhenHit"), true);
                }
            } else {
                // L2844
                DamageIn.set(n!("LifeWhenHit"), 0.0);
                // L2845
                DamageIn.set(n!("ManaWhenHit"), 0.0);
                // L2846
                DamageIn.set(n!("EnergyShieldWhenHit"), 0.0);
            }
            // L2848
            for damageType in dmgTypeList() {
                // Emperor's Vigilance (this needs to fail with divine flesh as it can't override it, hence the check for high bypass)
                // L2850
                if modDB.Flag(None, nm!["BlockedDamageDoesntBypassES"])
                    && (cmp(output, cat!(damageType, "EnergyShieldBypass")) < 100.0
                        && damageType != n!("Chaos"))
                {
                    // L2851
                    DamageIn.set(
                        cat!(damageType, "EnergyShieldBypass"),
                        output.num(cat!(damageType, "EnergyShieldBypass")) * (1.0 - BlockChance),
                    );
                }
                // L2853
                let mut AvoidChance = 0.0;
                // L2854
                if output.truthy(n!("specificTypeAvoidance")) {
                    // L2855
                    AvoidChance = m_min(
                        output.num(cat!("Avoid", damageType, "DamageChance")) + ExtraAvoidChance,
                        misc.AvoidChanceCap,
                    );
                    // unlucky config to lower the value of block, dodge, evade etc for ehp
                    // L2857
                    let worstOf = lor(configInput.get(n!("EHPUnluckyWorstOf")), || Val::Num(1.0));
                    // L2858
                    if cmp_num(&worstOf) > 1.0 {
                        // L2859
                        AvoidChance = AvoidChance / 100.0 * AvoidChance;
                        // L2860
                        if eq_num(&worstOf, 4.0) {
                            // L2861
                            AvoidChance = AvoidChance / 100.0 * AvoidChance;
                        }
                    }
                    // L2864
                    averageAvoidChance = averageAvoidChance + AvoidChance;
                }
                // L2866
                DamageIn.set(
                    damageType,
                    output.num(cat!(damageType, "TakenHit"))
                        * (blockEffect * suppressionEffect * (1.0 - AvoidChance / 100.0)),
                );
            }
            // recoup initialisation
            // L2869
            if cmp(output, n!("anyRecoup")) > 0.0 {
                // L2870
                DamageIn.set(n!("TrackRecoupable"), true);
                // L2871
                for damageType in dmgTypeList() {
                    // L2872
                    output.set(cat!(damageType, "RecoupableDamageTaken"), 0.0);
                }
            }
            // taken over time degen initialisation
            // L2876
            if cmp(output, n!("preventedLifeLossTotal")) > 0.0 {
                // L2877
                DamageIn.set(n!("TrackLifeLossOverTime"), true);
                // L2878
                output.set(n!("LifeLossLostOverTime"), 0.0);
                // L2879
                output.set(n!("LifeBelowHalfLossLostOverTime"), 0.0);
            }
            // L2881
            DamageIn.set(
                n!("LimitEHPSpeedup"),
                lor(DamageIn.get(n!("TrackRecoupable")), || {
                    lor(DamageIn.get(n!("TrackLifeLossOverTime")), || {
                        DamageIn.get(n!("GainWhenHit"))
                    })
                }),
            );
            // L2882
            averageAvoidChance = averageAvoidChance / 5.0;
            // L2883
            output.set(
                n!("ConfiguredDamageChance"),
                100.0 * (blockEffect * suppressionEffect * (1.0 - averageAvoidChance / 100.0)),
            );
            // L2884
            let v = if !eq_num(&output.get(n!("ConfiguredDamageChance")), 100.0)
                || DamageIn.truthy(n!("TrackRecoupable"))
                || DamageIn.truthy(n!("TrackLifeLossOverTime"))
                || DamageIn.truthy(n!("GainWhenHit"))
            {
                Val::Num(numberOfHitsToDie(c, &DamageIn))
            } else {
                output.get(n!("NumberOfDamagingHits"))
            };
            output.set(n!("NumberOfMitigatedDamagingHits"), v);
            // L2885
            if let Some(breakdown) = breakdown {
                // L2886
                let b = tbl![lfmt!("%.2f ^8(chance for block to fail)", 1.0 - BlockChance)];
                breakdown.set(n!("ConfiguredDamageChance"), b.clone());
                // L2889
                if output.truthy(n!("ShowBlockEffect")) {
                    // L2890
                    b.push(lfmt!(
                        "x %.2f ^8(block effect)",
                        output.num(n!("BlockEffect")) / 100.0
                    ));
                }
                // L2892
                if suppressionEffect != 1.0 {
                    // L2893
                    b.push(lfmt!("x %.3f ^8(suppression effect)", suppressionEffect));
                }
                // L2895
                if averageAvoidChance > 0.0 {
                    // L2896
                    b.push(lfmt!(
                        "x %.2f ^8(chance for avoidance to fail)",
                        1.0 - averageAvoidChance / 100.0
                    ));
                }
                // L2898
                b.push(lfmt!(
                    "= %.1f%% ^8(of damage taken from a%s hit)",
                    output.num(n!("ConfiguredDamageChance")),
                    article(damageCategoryConfig)
                ));
            }
        }

        // chance to not be hit breakdown
        // L2903
        {
            // L2904
            output.set(
                n!("TotalNumberOfHits"),
                output.num(n!("NumberOfMitigatedDamagingHits"))
                    / (1.0 - output.num(n!("ConfiguredNotHitChance")) / 100.0),
            );
            // L2905
            if let Some(breakdown) = breakdown {
                // L2906
                let b = Table::new();
                breakdown.set(n!("ConfiguredNotHitChance"), b.clone());
                // L2907
                if damageCategoryConfig == n!("Melee") || damageCategoryConfig == n!("Projectile") {
                    // L2908
                    if cmp(output, cat!(damageCategoryConfig, "EvadeChance")) > 0.0 {
                        // L2909
                        b.push(lfmt!(
                            "%.2f ^8(chance for evasion to fail)",
                            1.0 - output.num(cat!(damageCategoryConfig, "EvadeChance")) / 100.0
                        ));
                    }
                    // L2911
                    if cmp(output, n!("AttackDodgeChance")) > 0.0 {
                        // L2912
                        b.push(lfmt!(
                            "x %.2f ^8(chance for dodge to fail)",
                            1.0 - output.num(n!("AttackDodgeChance")) / 100.0
                        ));
                    }
                    // L2914
                    if damageCategoryConfig == n!("Projectile")
                        && !output.truthy(n!("specificTypeAvoidance"))
                        && cmp(output, n!("AvoidProjectilesChance")) > 0.0
                    {
                        // L2915
                        b.push(lfmt!(
                            "x %.2f ^8(chance for avoidance to fail)",
                            1.0 - output.num(n!("AvoidProjectilesChance")) / 100.0
                        ));
                    }
                } else if damageCategoryConfig == n!("Spell")
                    || damageCategoryConfig == n!("SpellProjectile")
                {
                    // L2918
                    if cmp(output, n!("SpellDodgeChance")) > 0.0 {
                        // L2919
                        b.push(lfmt!(
                            "%.2f ^8(chance for dodge to fail)",
                            1.0 - output.num(n!("SpellDodgeChance")) / 100.0
                        ));
                    }
                    // L2921
                    if damageCategoryConfig == n!("SpellProjectile")
                        && !output.truthy(n!("specificTypeAvoidance"))
                        && cmp(output, n!("AvoidProjectilesChance")) > 0.0
                    {
                        // L2922
                        b.push(lfmt!(
                            "x %.2f ^8(chance for avoidance to fail)",
                            1.0 - output.num(n!("AvoidProjectilesChance")) / 100.0
                        ));
                    }
                } else if damageCategoryConfig == n!("Average") {
                    // L2925
                    if cmp(output, n!("MeleeEvadeChance")) > 0.0
                        || cmp(output, n!("ProjectileEvadeChance")) > 0.0
                    {
                        // L2926
                        b.push(lfmt!(
                            "%.2f ^8(chance for evasion to fail, only applies to the attack portion)",
                            1.0 - (output.num(n!("MeleeEvadeChance"))
                                + output.num(n!("ProjectileEvadeChance")))
                                / 2.0
                                / 100.0
                        ));
                    }
                    // L2928
                    if cmp(output, n!("AttackDodgeChance")) > 0.0
                        || cmp(output, n!("SpellDodgeChance")) > 0.0
                    {
                        // L2929
                        b.push(lfmt!(
                            "x%.2f ^8(chance for dodge to fail)",
                            1.0 - (output.num(n!("AttackDodgeChance"))
                                + output.num(n!("SpellDodgeChance")))
                                / 2.0
                                / 100.0
                        ));
                    }
                    // L2931
                    if !output.truthy(n!("specificTypeAvoidance"))
                        && cmp(output, n!("AvoidProjectilesChance")) > 0.0
                    {
                        // L2932
                        b.push(lfmt!(
                            "x %.2f ^8(chance for avoidance to fail)",
                            1.0 - output.num(n!("AvoidProjectilesChance")) / 2.0 / 100.0
                        ));
                    }
                }
                // L2935
                if cmp(output, n!("AvoidAllDamageFromHitsChance")) > 0.0 {
                    // L2936
                    b.push(lfmt!(
                        "x %.2f ^8(chance for damage avoidance to fail)",
                        1.0 - output.num(n!("AvoidAllDamageFromHitsChance")) / 100.0
                    ));
                }
                // L2938
                let worstOf = lor(configInput.get(n!("EHPUnluckyWorstOf")), || Val::Num(1.0));
                // L2939
                if cmp_num(&worstOf) > 1.0 {
                    // L2940
                    b.push(lfmt!("unlucky worst of %d", worstOf.num()));
                }
                // L2942
                b.push(lfmt!(
                    "= %d%% ^8(chance to be hit by a%s hit)",
                    100.0 - output.num(n!("ConfiguredNotHitChance")),
                    article(damageCategoryConfig)
                ));
                // L2943
                breakdown.set(
                    n!("TotalNumberOfHits"),
                    tbl![
                        lfmt!(
                            "%.2f ^8(Number of mitigated hits)",
                            output.num(n!("NumberOfMitigatedDamagingHits"))
                        ),
                        lfmt!(
                            "/ %.2f ^8(Chance to even be hit)",
                            1.0 - output.num(n!("ConfiguredNotHitChance")) / 100.0
                        ),
                        lfmt!(
                            "= %.2f ^8(total average number of hits you can take)",
                            output.num(n!("TotalNumberOfHits"))
                        ),
                    ],
                );
            }
        }

        // effective hit pool
        // L2952
        output.set(
            n!("TotalEHP"),
            output.num(n!("TotalNumberOfHits")) * output.num(n!("totalEnemyDamageIn")),
        );
        // L2953
        if let Some(breakdown) = breakdown {
            // L2954
            breakdown.set(
                n!("TotalEHP"),
                tbl![
                    lfmt!(
                        "%.2f ^8(total average number of hits you can take)",
                        output.num(n!("TotalNumberOfHits"))
                    ),
                    lfmt!(
                        "x %d ^8(total incoming damage)",
                        output.num(n!("totalEnemyDamageIn"))
                    ),
                    lfmt!(
                        "= %d ^8(total damage you can take)",
                        output.num(n!("TotalEHP"))
                    ),
                ],
            );
        }

        // survival time
        // L2962
        {
            // L2963
            let v = lor(configInput.get(n!("enemySpeed")), || {
                lor(env.configPlaceholder.get(n!("enemySpeed")), || Val::Num(700.0))
            })
            .num()
                / (1.0 + enemyDB.Sum(ModType::INC, None, nm!["Speed"]) / 100.0);
            output.set(n!("enemySkillTime"), v);
            // L2964
            let enemyActionSpeed = match actor.enemy() {
                Some(enemy) => actionSpeedMod(&enemy),
                None => lua_error!("attempt to index local 'actor' (a nil value)"),
            };
            // L2965
            let v = output.num(n!("enemySkillTime")) / 1000.0 / enemyActionSpeed;
            output.set(n!("enemySkillTime"), v);
            // L2966
            output.set(
                n!("EHPSurvivalTime"),
                output.num(n!("TotalNumberOfHits")) * output.num(n!("enemySkillTime")),
            );
            // L2967
            if let Some(breakdown) = breakdown {
                // L2968
                breakdown.set(
                    n!("EHPSurvivalTime"),
                    tbl![
                        lfmt!(
                            "%.2f ^8(total average number of hits you can take)",
                            output.num(n!("TotalNumberOfHits"))
                        ),
                        lfmt!(
                            "x %.2f ^8enemy attack/cast time",
                            output.num(n!("enemySkillTime"))
                        ),
                        lfmt!(
                            "= %.2f seconds ^8(total time it would take to die)",
                            output.num(n!("EHPSurvivalTime"))
                        ),
                    ],
                );
            }
        }
    }

    // recoup
    // L2978
    if cmp(output, n!("anyRecoup")) > 0.0 && damageCategoryConfig != n!("DamageOverTime") {
        // L2979
        let mut totalDamage = 0.0;
        // L2980
        let mut totalElementalDamage = 0.0;
        // L2981
        let totalPhysicalDamageMitigated = output.num(n!("NumberOfMitigatedDamagingHits"))
            * (output.num(n!("PhysicalTakenDamage")) - output.num(n!("PhysicalTakenHit")));
        // L2982 (`extraPseudoRecoup[recoupTypeList[j]] = { amount, duration }`, by recoupTypeList index)
        let mut extraPseudoRecoup: [Option<(f64, f64)>; 3] = [None; 3];
        // L2983
        for damageType in dmgTypeList() {
            // L2984
            totalDamage = totalDamage + output.num(cat!(damageType, "RecoupableDamageTaken"));
            // L2985
            if isElemental(damageType) {
                // L2986
                totalElementalDamage =
                    totalElementalDamage + output.num(cat!(damageType, "RecoupableDamageTaken"));
            }
        }
        // L2989
        let recoupTypeList = [n!("Life"), n!("Mana"), n!("EnergyShield")];
        // L2990
        for (i0, recoupType) in recoupTypeList.into_iter().enumerate() {
            // L2991
            let recoupTime = if modDB.Flag(None, &[cat!("3Second", recoupType, "Recoup")])
                || modDB.Flag(None, nm!["3SecondRecoup"])
            {
                3.0
            } else {
                4.0
            };
            let kTotalRecovery = cat!("Total", recoupType, "RecoupRecovery");
            let kTotalPseudo = cat!("Total", recoupType, "PseudoRecoup");
            // L2992
            output.set(
                kTotalRecovery,
                output.num0(cat!(recoupType, "Recoup")) / 100.0 * totalDamage,
            );
            // L2993
            if output.num0(cat!("Elemental", recoupType, "Recoup")) > 0.0
                && totalElementalDamage > 0.0
            {
                // L2994
                let v = output.num(kTotalRecovery)
                    + output.num(cat!("Elemental", recoupType, "Recoup")) / 100.0
                        * totalElementalDamage;
                output.set(kTotalRecovery, v);
            }
            // L2996
            for damageType in dmgTypeList() {
                // L2997
                if output.num0(cat!(damageType, recoupType, "Recoup")) > 0.0
                    && cmp(output, cat!(damageType, "RecoupableDamageTaken")) > 0.0
                {
                    // L2998
                    let v = output.num(kTotalRecovery)
                        + output.num(cat!(damageType, recoupType, "Recoup")) / 100.0
                            * output.num(cat!(damageType, "RecoupableDamageTaken"));
                    output.set(kTotalRecovery, v);
                }
            }
            // L3001
            output.set(
                kTotalPseudo,
                output.num0(cat!("PhysicalDamageMitigated", recoupType, "PseudoRecoup")) / 100.0
                    * totalPhysicalDamageMitigated,
            );
            // L3002
            let PseudoRecoupDuration = output.num_or(
                cat!("PhysicalDamageMitigated", recoupType, "PseudoRecoupDuration"),
                4.0,
            );
            // Pious Path
            // L3004
            if !eq_num(&output.get(kTotalPseudo), 0.0) {
                // L3005
                for j0 in (i0 + 1)..recoupTypeList.len() {
                    let other = recoupTypeList[j0];
                    // L3006
                    if modDB.Flag(None, &[cat!(recoupType, "RegenerationRecovers", other)])
                        && !modDB.Flag(None, &[cat!("UnaffectedBy", other, "Regen")])
                        && !modDB.Flag(None, &[cat!("No", other, "Regen")])
                        && !modDB.Flag(None, &[cat!("CannotGain", other)])
                    {
                        // L3007
                        extraPseudoRecoup[j0] = Some((
                            output.num(kTotalPseudo) * output.num(cat!(other, "RecoveryRateMod"))
                                / output.num(cat!(recoupType, "RecoveryRateMod")),
                            PseudoRecoupDuration,
                        ));
                    }
                }
            }
            // L3011
            let v = if !modDB.Flag(None, &[cat!("UnaffectedBy", recoupType, "Regen")]) {
                output.get(kTotalPseudo)
            } else {
                Val::Num(0.0)
            };
            output.set(kTotalPseudo, v);
            let extra = extraPseudoRecoup[i0];
            // L3012
            output.set(
                cat!(recoupType, "RecoupRecoveryMax"),
                output.num(kTotalRecovery) / recoupTime
                    + output.num(kTotalPseudo) / PseudoRecoupDuration
                    + (match extra {
                        Some((a, d)) => a / d,
                        None => 0.0,
                    }),
            );
            // L3013
            output.set(
                cat!(recoupType, "RecoupRecoveryAvg"),
                output.num(kTotalRecovery) / (output.num(n!("EHPSurvivalTime")) + recoupTime)
                    + output.num(kTotalPseudo)
                        / (output.num(n!("EHPSurvivalTime")) + PseudoRecoupDuration)
                    + (match extra {
                        Some((a, d)) => a / (output.num(n!("EHPSurvivalTime")) + d),
                        None => 0.0,
                    }),
            );
            // L3014
            if let Some(breakdown) = breakdown {
                // L3015
                let mut multipleTypes = 0.0;
                // L3016
                let bMax = Table::new();
                breakdown.set(cat!(recoupType, "RecoupRecoveryMax"), bMax.clone());
                // L3017
                if output.num0(cat!(recoupType, "Recoup")) > 0.0 {
                    // L3018
                    bMax.push(lfmt!(
                        "%d ^8(total damage taken during ehp calcs)",
                        totalDamage
                    ));
                    // L3019
                    bMax.push(lfmt!(
                        "* %.2f ^8(percent of damage recouped)",
                        output.num(cat!(recoupType, "Recoup")) / 100.0
                    ));
                    // L3020
                    multipleTypes = multipleTypes + 1.0;
                }
                // L3022
                if output.num0(cat!("Elemental", recoupType, "Recoup")) > 0.0
                    && totalElementalDamage > 0.0
                {
                    // L3023
                    if multipleTypes > 0.0 {
                        // L3024
                        bMax.push(lfmt!(""));
                    }
                    // L3026
                    bMax.push(lfmt!(
                        "%s%d ^8(total elemental damage taken during ehp calcs)",
                        if multipleTypes > 0.0 { "+" } else { "" },
                        totalDamage
                    ));
                    // L3027
                    bMax.push(lfmt!(
                        "* %.2f ^8(percent of damage recouped)",
                        output.num(cat!("Elemental", recoupType, "Recoup")) / 100.0
                    ));
                    // L3028
                    multipleTypes = multipleTypes + 1.0;
                }
                // L3030
                for damageType in dmgTypeList() {
                    // L3031
                    if output.num0(cat!(damageType, recoupType, "Recoup")) > 0.0
                        && cmp(output, cat!(damageType, "RecoupableDamageTaken")) > 0.0
                    {
                        // L3032
                        if multipleTypes > 0.0 {
                            // L3033
                            bMax.push(lfmt!(""));
                        }
                        // L3035
                        bMax.push(lfmt!(
                            "%s%d ^8(total %s damage taken during ehp calcs)",
                            if multipleTypes > 0.0 { "+" } else { "" },
                            totalDamage,
                            damageType
                        ));
                        // L3036
                        bMax.push(lfmt!(
                            "* %.2f ^8(percent of damage recouped)",
                            output.num(cat!(damageType, recoupType, "Recoup")) / 100.0
                        ));
                        // L3037
                        multipleTypes = multipleTypes + 1.0;
                    }
                }
                // L3040
                bMax.push(lfmt!(
                    "= %d ^8(total damage recoup amount)",
                    output.num(kTotalRecovery)
                ));
                // L3041
                let bAvg = bMax.copyTable(false);
                breakdown.set(cat!(recoupType, "RecoupRecoveryAvg"), bAvg.clone());
                // L3042
                bMax.push(lfmt!("/ %.2f ^8(over %d seconds)", recoupTime, recoupTime));
                // L3043
                if cmp(output, kTotalPseudo) > 0.0 {
                    // L3044
                    bMax.push(lfmt!(
                        "+ %.2f ^8(total damage mitigated pseudo recoup amount)",
                        output.num(kTotalPseudo)
                    ));
                    // L3045
                    bMax.push(lfmt!(
                        "/ %.2f ^8(over %d seconds)",
                        PseudoRecoupDuration,
                        PseudoRecoupDuration
                    ));
                }
                // L3047
                if let Some((a, d)) = extra {
                    // L3048
                    bMax.push(lfmt!(
                        "+ %.2f ^8(total damage mitigated pseudo recoup amount)",
                        a
                    ));
                    // L3049
                    bMax.push(lfmt!("/ %.2f ^8(over %d seconds)", d, d));
                }
                // L3051
                bMax.push(lfmt!(
                    "= %.2f per second ^8",
                    output.num(cat!(recoupType, "RecoupRecoveryMax"))
                ));
                // L3052
                bAvg.push(lfmt!(
                    "/ %.2f ^8(total time of the recoup (survival time + %d seconds))",
                    output.num(n!("EHPSurvivalTime")) + recoupTime,
                    recoupTime
                ));
                // L3053
                if cmp(output, kTotalPseudo) > 0.0 {
                    // L3054
                    bAvg.push(lfmt!(
                        "+ %.2f ^8(total damage mitigated pseudo recoup amount)",
                        output.num(kTotalPseudo)
                    ));
                    // L3055
                    bAvg.push(lfmt!(
                        "/ %.2f ^8(total time of the recoup (survival time + %d seconds)",
                        output.num(n!("EHPSurvivalTime")) + PseudoRecoupDuration,
                        PseudoRecoupDuration
                    ));
                }
                // L3057
                if let Some((a, d)) = extra {
                    // L3058
                    bAvg.push(lfmt!(
                        "+ %.2f ^8(total damage mitigated pseudo recoup amount)",
                        a
                    ));
                    // L3059
                    bAvg.push(lfmt!(
                        "/ %.2f ^8(total time of the recoup (survival time + %d seconds)",
                        output.num(n!("EHPSurvivalTime")) + d,
                        d
                    ));
                }
                // L3061
                bAvg.push(lfmt!(
                    "= %.2f per second ^8",
                    output.num(cat!(recoupType, "RecoupRecoveryAvg"))
                ));
            }
        }
    }

    // petrified blood "degen"
    // L3067
    if cmp(output, n!("preventedLifeLossTotal")) > 0.0
        && (output.truthy(n!("LifeLossLostOverTime"))
            && output.truthy(n!("LifeBelowHalfLossLostOverTime")))
    {
        // L3068
        let LifeLossBelowHalfLost =
            modDB.Sum(ModType::BASE, None, nm!["LifeLossBelowHalfLost"]) / 100.0;
        // L3069
        output.set(
            n!("LifeLossLostMax"),
            (output.num(n!("LifeLossLostOverTime"))
                + output.num(n!("LifeBelowHalfLossLostOverTime")) * LifeLossBelowHalfLost)
                / 4.0,
        );
        // L3070
        output.set(
            n!("LifeLossLostAvg"),
            (output.num(n!("LifeLossLostOverTime"))
                + output.num(n!("LifeBelowHalfLossLostOverTime")) * LifeLossBelowHalfLost)
                / (output.num(n!("EHPSurvivalTime")) + 4.0),
        );
        // L3071
        if let Some(breakdown) = breakdown {
            // L3072
            let bMax = Table::new();
            breakdown.set(n!("LifeLossLostMax"), bMax.clone());
            // L3073
            if !eq_num(&output.get(n!("LifeLossLostOverTime")), 0.0) {
                // L3074
                bMax.push(lfmt!(
                    "( %d ^8(total damage prevented by Progenesis)",
                    output.num(n!("LifeLossLostOverTime"))
                ));
            }
            // L3076
            if !eq_num(&output.get(n!("LifeBelowHalfLossLostOverTime")), 0.0) {
                // L3077
                bMax.push(lfmt!(
                    "%s %d ^8(total damage prevented by petrified blood)",
                    if !eq_num(&output.get(n!("LifeLossLostOverTime")), 0.0) { "+" } else { "(" },
                    output.num(n!("LifeBelowHalfLossLostOverTime"))
                ));
                // L3078
                bMax.push(lfmt!(
                    "* %.2f ^8(percent of damage taken from petrified blood)",
                    LifeLossBelowHalfLost
                ));
            }
            // L3080
            bMax.push(lfmt!(") / %.2f ^8(over 4 seconds)", 4.0));
            // L3081
            bMax.push(lfmt!("= %.2f per second", output.num(n!("LifeLossLostMax"))));
            // L3082
            let bAvg = Table::new();
            breakdown.set(n!("LifeLossLostAvg"), bAvg.clone());
            // L3083
            if !eq_num(&output.get(n!("LifeLossLostOverTime")), 0.0) {
                // L3084
                bAvg.push(lfmt!(
                    "( %d ^8(total damage prevented by Progenesis)",
                    output.num(n!("LifeLossLostOverTime"))
                ));
            }
            // L3086
            if !eq_num(&output.get(n!("LifeBelowHalfLossLostOverTime")), 0.0) {
                // L3087
                bAvg.push(lfmt!(
                    "%s %d ^8(total damage prevented by petrified blood)",
                    if !eq_num(&output.get(n!("LifeLossLostOverTime")), 0.0) { "+" } else { "(" },
                    output.num(n!("LifeBelowHalfLossLostOverTime"))
                ));
                // L3088
                bAvg.push(lfmt!(
                    "* %.2f ^8(percent of damage taken from petrified blood)",
                    LifeLossBelowHalfLost
                ));
            }
            // L3090
            bAvg.push(lfmt!(
                ") / %.2f ^8(total time of the degen (survival time + 4 seconds))",
                output.num(n!("EHPSurvivalTime")) + 4.0
            ));
            // L3091
            bAvg.push(lfmt!("= %.2f per second", output.num(n!("LifeLossLostAvg"))));
        }
    }

    // net recovery over time from enemy hits
    // L3096
    if output.num0(n!("LifeRecoupRecoveryAvg")) > 0.0
        || cmp(output, n!("preventedLifeLossTotal")) > 0.0
    {
        // L3097
        output.set(
            n!("netLifeRecoupAndLossLostOverTimeMax"),
            output.num0(n!("LifeRecoupRecoveryMax")) - output.num0(n!("LifeLossLostMax")),
        );
        // L3098
        output.set(
            n!("netLifeRecoupAndLossLostOverTimeAvg"),
            output.num0(n!("LifeRecoupRecoveryAvg")) - output.num0(n!("LifeLossLostAvg")),
        );
        // L3099
        if output.num0(n!("LifeRecoupRecoveryAvg")) > 0.0
            && cmp(output, n!("preventedLifeLossTotal")) > 0.0
        {
            // L3100
            output.set(n!("showNetRecoup"), true);
            // L3101
            if let Some(breakdown) = breakdown {
                // L3102
                breakdown.set(
                    n!("netLifeRecoupAndLossLostOverTimeMax"),
                    tbl![
                        lfmt!(
                            "%.2f ^8(total life recouped per second)",
                            output.num(n!("LifeRecoupRecoveryMax"))
                        ),
                        lfmt!(
                            "- %.2f ^8(total life taken over time per second)",
                            output.num(n!("LifeLossLostMax"))
                        ),
                        lfmt!(
                            "= %.2f per second",
                            output.num(n!("netLifeRecoupAndLossLostOverTimeMax"))
                        ),
                    ],
                );
                // L3107
                breakdown.set(
                    n!("netLifeRecoupAndLossLostOverTimeAvg"),
                    tbl![
                        lfmt!(
                            "%.2f ^8(total life recouped per second)",
                            output.num(n!("LifeRecoupRecoveryAvg"))
                        ),
                        lfmt!(
                            "- %.2f ^8(total life taken over time per second)",
                            output.num(n!("LifeLossLostAvg"))
                        ),
                        lfmt!(
                            "= %.2f per second",
                            output.num(n!("netLifeRecoupAndLossLostOverTimeAvg"))
                        ),
                    ],
                );
            }
        }
    }

    // pvp
    // L3117
    if configInput.truthy(n!("PvpScaling")) {
        // L3118
        let PvpTvalue = output.num_or(n!("enemySkillTime"), 1.0);
        // L3119
        let PvpMultiplier = configInput.num_or(n!("enemyMultiplierPvpDamage"), 100.0) / 100.0;

        // L3121
        let PvpNonElemental1 = misc.PvpNonElemental1;
        // L3122
        let PvpNonElemental2 = misc.PvpNonElemental2;
        // L3123
        let PvpElemental1 = misc.PvpElemental1;
        // L3124
        let PvpElemental2 = misc.PvpElemental2;

        // L3126
        let percentageNonElemental = (output.num(n!("PhysicalTakenHit"))
            + output.num(n!("ChaosTakenHit")))
            / output.num(n!("totalTakenHit"));
        // L3127
        let percentageElemental = 1.0 - percentageNonElemental;
        // L3128
        let portionNonElemental = lpow(
            output.num(n!("totalTakenHit")) / PvpTvalue / PvpNonElemental2,
            PvpNonElemental1,
        ) * PvpTvalue
            * PvpNonElemental2
            * percentageNonElemental;
        // L3129
        let portionElemental = lpow(
            output.num(n!("totalTakenHit")) / PvpTvalue / PvpElemental2,
            PvpElemental1,
        ) * PvpTvalue
            * PvpElemental2
            * percentageElemental;
        // L3130 (`portion or 0` is dead: both are numbers)
        output.set(
            n!("PvPTotalTakenHit"),
            (portionNonElemental + portionElemental) * PvpMultiplier,
        );

        // L3132
        if let Some(breakdown) = breakdown {
            // L3133
            let b = Table::new();
            breakdown.set(n!("PvPTotalTakenHit"), b.clone());
            // L3134
            let percentBoth = (percentageNonElemental > 0.0) && (percentageElemental > 0.0);
            // L3135
            b.push(lfmt!(
                "Pvp Formula is (D/(T*M))^E*T*%s, where D is the damage, T is the time taken,",
                if percentBoth { "M*P" } else { "M" }
            ));
            // L3136
            b.push(lfmt!(
                " M is the multiplier%s",
                if percentBoth {
                    ", E is the exponent and P is the percentage of that type (ele or non ele)"
                } else {
                    " and E is the exponent"
                }
            ));
            // L3137
            if percentBoth {
                // L3138
                b.push(lfmt!(
                    "(M= %.1f for ele and %.1f for non-ele)(E= %.2f for ele and %.2f for non-ele)",
                    PvpElemental2,
                    PvpNonElemental2,
                    PvpElemental1,
                    PvpNonElemental1
                ));
                // L3139
                b.push(lfmt!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f * %.2f = %.1f",
                    output.num(n!("totalTakenHit")),
                    PvpTvalue,
                    PvpNonElemental2,
                    PvpNonElemental1,
                    PvpTvalue,
                    PvpNonElemental2,
                    percentageNonElemental,
                    portionNonElemental
                ));
                // L3140
                b.push(lfmt!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f * %.2f = %.1f",
                    output.num(n!("totalTakenHit")),
                    PvpTvalue,
                    PvpElemental2,
                    PvpElemental1,
                    PvpTvalue,
                    PvpElemental2,
                    percentageElemental,
                    portionElemental
                ));
                // L3141
                b.push(lfmt!(
                    "(portionNonElemental + portionElemental)%s",
                    if PvpMultiplier != 1.0 { " * PvP multiplier" } else { " " }
                ));
                // L3142
                if PvpMultiplier != 1.0 {
                    // L3143
                    b.push(lfmt!(
                        "(%.1f + %.1f) * %g",
                        portionNonElemental,
                        portionElemental,
                        PvpMultiplier
                    ));
                } else {
                    // L3145
                    b.push(lfmt!("%.1f + %.1f", portionNonElemental, portionElemental));
                }
            } else if percentageElemental <= 0.0 {
                // L3148
                b.push(lfmt!(
                    "(M= %.1f for non-ele)(E= %.2f for non-ele)",
                    PvpNonElemental2,
                    PvpNonElemental1
                ));
                // L3149
                b.push(lfmt!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f = %.1f",
                    output.num(n!("totalTakenHit")),
                    PvpTvalue,
                    PvpNonElemental2,
                    PvpNonElemental1,
                    PvpTvalue,
                    PvpNonElemental2,
                    portionNonElemental
                ));
                // L3150
                if PvpMultiplier != 1.0 {
                    // L3151
                    b.push(lfmt!(
                        "%.1f * %g ^8(portionNonElemental * PvP multiplier)",
                        portionNonElemental,
                        PvpMultiplier
                    ));
                }
            } else if percentageNonElemental <= 0.0 {
                // L3154
                b.push(lfmt!(
                    "(M= %.1f for ele)(E= %.2f for ele)",
                    PvpElemental2,
                    PvpElemental1
                ));
                // L3155
                b.push(lfmt!(
                    "(%.1f / (%.2f * %.1f)) ^ %.2f * %.2f * %.1f = %.1f",
                    output.num(n!("totalTakenHit")),
                    PvpTvalue,
                    PvpElemental2,
                    PvpElemental1,
                    PvpTvalue,
                    PvpElemental2,
                    portionElemental
                ));
                // L3156
                if PvpMultiplier != 1.0 {
                    // L3157
                    b.push(lfmt!(
                        "%.1f * %g ^8(portionElemental * PvP multiplier)",
                        portionElemental,
                        PvpMultiplier
                    ));
                }
            }
            // L3160
            b.push(lfmt!("= %.1f", output.num(n!("PvPTotalTakenHit"))));
        }
    }

    // maximum hit taken (3164: the comment opening DF-G's block)
    Flow::Normal
}
