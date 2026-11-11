//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! CalcSetup.lua 1-120: module header and `calcs.initModDB`.
//!
//! Owner: CS0.
#![allow(non_snake_case, clippy::too_many_lines)]

use crate::prelude::*;
use pob_gamedata::tag;

// @pob Modules/CalcSetup.lua:7-16 h=78bada9b
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_min,m_max,tempTable1)

/// `data.characterConstants[key]`. Every key initModDB reads is a field of P1's strict
/// `characterConstants` struct; a missing one would be a nil value in Lua (and an arithmetic
/// error for the two `/ 60` reads), so it raises here.
fn cc(key: &str) -> f64 {
    match data().characterConstants.get(key) {
        Some(v) => v,
        None => lua_error!("data.characterConstants[\"{}\"] is nil", key),
    }
}

/// Initialise modifier database with stats and conditions common to all actors
/// @pob Modules/CalcSetup.lua:19-120 h=42328b0c
#[rustfmt::skip]
pub fn initModDB(env: &EnvRef, modDB: &ModDB) {
    // L20
    modDB.NewMod(n!("FireResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L21
    modDB.NewMod(n!("ColdResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L22
    modDB.NewMod(n!("LightningResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L23
    modDB.NewMod(n!("ChaosResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L24
    modDB.NewMod(n!("TotemFireResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L25
    modDB.NewMod(n!("TotemColdResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L26
    modDB.NewMod(n!("TotemLightningResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L27
    modDB.NewMod(n!("TotemChaosResistMax"), ModType::BASE, cc("base_maximum_all_resistances_%"), "Base", 0, 0, &[]);
    // L28
    modDB.NewMod(n!("BlockChanceMax"), ModType::BASE, cc("maximum_block_%"), "Base", 0, 0, &[]);
    // L29
    modDB.NewMod(n!("SpellBlockChanceMax"), ModType::BASE, cc("base_maximum_spell_block_%"), "Base", 0, 0, &[]);
    // L30
    modDB.NewMod(n!("SpellDodgeChanceMax"), ModType::BASE, 75.0, "Base", 0, 0, &[]);
    // L31
    modDB.NewMod(n!("ChargeDuration"), ModType::BASE, 10.0, "Base", 0, 0, &[]);
    // L32
    modDB.NewMod(n!("PowerChargesMax"), ModType::BASE, cc("max_power_charges"), "Base", 0, 0, &[]);
    // L33
    modDB.NewMod(n!("FrenzyChargesMax"), ModType::BASE, cc("max_frenzy_charges"), "Base", 0, 0, &[]);
    // L34
    modDB.NewMod(n!("EnduranceChargesMax"), ModType::BASE, cc("max_endurance_charges"), "Base", 0, 0, &[]);
    // L35
    modDB.NewMod(n!("SiphoningChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L36
    modDB.NewMod(n!("ChallengerChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L37
    modDB.NewMod(n!("BlitzChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L38
    modDB.NewMod(n!("InspirationChargesMax"), ModType::BASE, cc("maximum_righteous_charges"), "Base", 0, 0, &[]);
    // L39
    modDB.NewMod(n!("CrabBarriersMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L40
    modDB.NewMod(n!("BrutalChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L41
    modDB.NewMod(n!("BrineChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L42
    modDB.NewMod(n!("PhysicalDamageGainAsCold"), ModType::BASE, cc("physical_damage_%_to_add_as_cold_per_brine_charge"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "BrineCharge" })]);
    // L43
    modDB.NewMod(n!("PhysicalDamageGainAsLightning"), ModType::BASE, cc("physical_damage_%_to_add_as_lightning_per_brine_charge"), "Base", 0, 0, &[tag!({ type = Multiplier, var = "BrineCharge" })]);
    // L44
    modDB.NewMod(n!("AbsorptionChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L45
    modDB.NewMod(n!("AfflictionChargesMax"), ModType::BASE, 0.0, "Base", 0, 0, &[]);
    // L46
    modDB.NewMod(n!("BloodChargesMax"), ModType::BASE, cc("maximum_blood_scythe_charges"), "Base", 0, 0, &[]);
    // L47
    modDB.NewMod(n!("MaxLifeLeechRate"), ModType::BASE, cc("maximum_life_leech_rate_%_per_minute") / 60.0, "Base", 0, 0, &[]);
    // L48
    modDB.NewMod(n!("MaxManaLeechRate"), ModType::BASE, cc("maximum_mana_leech_rate_%_per_minute") / 60.0, "Base", 0, 0, &[]);
    // L49
    modDB.NewMod(n!("ImpaleStacksMax"), ModType::BASE, cc("impaled_debuff_number_of_reflected_hits"), "Base", 0, 0, &[]);
    // L50
    modDB.NewMod(n!("SoulEaterMax"), ModType::BASE, cc("soul_eater_maximum_stacks"), "Base", 0, 0, &[]);
    // L51
    modDB.NewMod(n!("BleedStacksMax"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
    // L52
    modDB.NewMod(n!("MaxEnergyShieldLeechRate"), ModType::BASE, 10.0, "Base", 0, 0, &[]);
    // L53
    modDB.NewMod(n!("MaxLifeLeechInstance"), ModType::BASE, cc("maximum_life_leech_amount_per_leech_%_max_life"), "Base", 0, 0, &[]);
    // L54
    modDB.NewMod(n!("MaxManaLeechInstance"), ModType::BASE, cc("maximum_mana_leech_amount_per_leech_%_max_mana"), "Base", 0, 0, &[]);
    // L55
    modDB.NewMod(n!("MaxEnergyShieldLeechInstance"), ModType::BASE, cc("maximum_energy_shield_leech_amount_per_leech_%_max_energy_shield"), "Base", 0, 0, &[]);
    // L56
    modDB.NewMod(n!("TrapThrowingTime"), ModType::BASE, 0.6, "Base", 0, 0, &[]);
    // L57
    modDB.NewMod(n!("MineLayingTime"), ModType::BASE, 0.3, "Base", 0, 0, &[]);
    // L58
    modDB.NewMod(n!("WarcryCastTime"), ModType::BASE, 0.8, "Base", 0, 0, &[]);
    // L59
    modDB.NewMod(n!("TotemPlacementTime"), ModType::BASE, 0.6, "Base", 0, 0, &[]);
    // L60
    modDB.NewMod(n!("BallistaPlacementTime"), ModType::BASE, 0.5, "Base", 0, 0, &[]);
    // L61
    modDB.NewMod(n!("ActiveTotemLimit"), ModType::BASE, cc("base_number_of_totems_allowed"), "Base", 0, 0, &[]);
    // L62
    modDB.NewMod(n!("ShockStacksMax"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
    // L63
    modDB.NewMod(n!("ScorchStacksMax"), ModType::BASE, 1.0, "Base", 0, 0, &[]);
    // L64
    modDB.NewMod(n!("MovementSpeed"), ModType::INC, -30.0, "Base", 0, 0, &[tag!({ type = Condition, var = "Maimed" })]);
    // L65
    modDB.NewMod(n!("DamageTaken"), ModType::INC, 10.0, "Base", ModFlag::Attack, 0, &[tag!({ type = Condition, var = "Intimidated" })]);
    // L66
    modDB.NewMod(n!("DamageTaken"), ModType::INC, 10.0, "Base", ModFlag::Attack, 0, &[tag!({ type = Condition, var = "Intimidated", neg = true }), tag!({ type = Condition, var = "Party:Intimidated" })]);
    // L67
    modDB.NewMod(n!("DamageTaken"), ModType::INC, 10.0, "Base", ModFlag::Spell, 0, &[tag!({ type = Condition, var = "Unnerved" })]);
    // L68
    modDB.NewMod(n!("DamageTaken"), ModType::INC, 10.0, "Base", ModFlag::Spell, 0, &[tag!({ type = Condition, var = "Unnerved", neg = true }), tag!({ type = Condition, var = "Party:Unnerved" })]);
    // L69
    modDB.NewMod(n!("Damage"), ModType::MORE, -10.0, "Base", 0, 0, &[tag!({ type = Condition, var = "Debilitated" }), tag!({ type = GlobalEffect, effectName = "Debilitated", effectType = "Debuff" })]);
    // L70
    modDB.NewMod(n!("MovementSpeed"), ModType::MORE, -20.0, "Base", 0, 0, &[tag!({ type = Condition, var = "Debilitated" }), tag!({ type = GlobalEffect, effectName = "Debilitated", effectType = "Debuff" })]);
    // L71
    modDB.NewMod(n!("Damage"), ModType::MORE, -10.0, "Base", 0, 0, &[tag!({ type = Condition, var = "MalignantMadness" }), tag!({ type = GlobalEffect, effectName = "Malignant Madness", effectType = "Debuff" })]);
    // L72
    modDB.NewMod(n!("ActionSpeed"), ModType::MORE, -10.0, "Base", 0, 0, &[tag!({ type = Condition, var = "MalignantMadness" }), tag!({ type = GlobalEffect, effectName = "Malignant Madness", effectType = "Debuff" })]);
    // L73
    modDB.NewMod(n!("Condition:Burning"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = IgnoreCond }), tag!({ type = Condition, var = "Ignited" })]);
    // L74
    modDB.NewMod(n!("Condition:Poisoned"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = IgnoreCond }), tag!({ type = MultiplierThreshold, var = "PoisonStack", threshold = 1.0 })]);
    // L75
    modDB.NewMod(n!("Blind"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Blinded" })]);
    // L76
    modDB.NewMod(n!("Chill"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Chilled" })]);
    // L77
    modDB.NewMod(n!("Freeze"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Frozen" })]);
    // L78
    modDB.NewMod(n!("Fortify"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Fortify" })]);
    // L79
    modDB.NewMod(n!("Fortified"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Fortified" })]);
    // L80
    modDB.NewMod(n!("Excommunicated"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Excommunicated" })]);
    // L81
    modDB.NewMod(n!("Fanaticism"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Fanaticism" })]);
    // L82
    modDB.NewMod(n!("Onslaught"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Onslaught" })]);
    // L83
    modDB.NewMod(n!("UnholyMight"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "UnholyMight" })]);
    // L84
    modDB.NewMod(n!("ChaoticMight"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "ChaoticMight" })]);
    // L85
    modDB.NewMod(n!("Tailwind"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Tailwind" })]);
    // L86
    modDB.NewMod(n!("Adrenaline"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Adrenaline" })]);
    // L87
    modDB.NewMod(n!("AccelerationShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "AccelerationShrine" })]);
    // L88
    modDB.NewMod(n!("BrutalShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "BrutalShrine" })]);
    // L89
    modDB.NewMod(n!("DiamondShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "DiamondShrine" })]);
    // L90
    modDB.NewMod(n!("DivineShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "DivineShrine" })]);
    // L91
    modDB.NewMod(n!("EchoingShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "EchoingShrine" })]);
    // L92
    modDB.NewMod(n!("GloomShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "GloomShrine" })]);
    // L93
    modDB.NewMod(n!("GreaterFreezingShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "GreaterFreezingShrine" })]);
    // L94
    modDB.NewMod(n!("GreaterShockingShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "GreaterShockingShrine" })]);
    // L95
    modDB.NewMod(n!("GreaterSkeletalShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "GreaterSkeletalShrine" })]);
    // L96
    modDB.NewMod(n!("ImpenetrableShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "ImpenetrableShrine" })]);
    // L97
    modDB.NewMod(n!("MassiveShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "MassiveShrine" })]);
    // L98
    modDB.NewMod(n!("ReplenishingShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "ReplenishingShrine" })]);
    // L99
    modDB.NewMod(n!("ResistanceShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "ResistanceShrine" })]);
    // L100
    modDB.NewMod(n!("ResonatingShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "ResonatingShrine" })]);
    // L101
    modDB.NewMod(n!("LesserAccelerationShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LesserAccelerationShrine" }), tag!({ type = Condition, var = "AccelerationShrine", neg = true })]);
    // L102
    modDB.NewMod(n!("LesserBrutalShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LesserBrutalShrine" }), tag!({ type = Condition, var = "BrutalShrine", neg = true })]);
    // L103
    modDB.NewMod(n!("LesserImpenetrableShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LesserImpenetrableShrine" }), tag!({ type = Condition, var = "ImpenetrableShrine", neg = true })]);
    // L104
    modDB.NewMod(n!("LesserMassiveShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LesserMassiveShrine" }), tag!({ type = Condition, var = "MassiveShrine", neg = true })]);
    // L105
    modDB.NewMod(n!("LesserReplenishingShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LesserReplenishingShrine" }), tag!({ type = Condition, var = "ReplenishingShrine", neg = true })]);
    // L106
    modDB.NewMod(n!("LesserResistanceShrine"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LesserResistanceShrine" }), tag!({ type = Condition, var = "ResistanceShrine", neg = true })]);
    // L107
    modDB.NewMod(n!("BloodShrineOfRats"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "BloodShrineOfRats" })]);
    // L108
    modDB.NewMod(n!("BloodShrineOfLocusts"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "BloodShrineOfLocusts" })]);
    // L109
    modDB.NewMod(n!("BloodShrineOfToads"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "BloodShrineOfToads" })]);
    // L110
    modDB.NewMod(n!("BloodShrineOfCrows"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "BloodShrineOfCrows" })]);
    // L111
    modDB.NewMod(n!("BloodShrineOfBats"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "BloodShrineOfBats" })]);
    // L112
    modDB.NewMod(n!("AlchemistsGenius"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "AlchemistsGenius" })]);
    // L113
    modDB.NewMod(n!("LuckyHits"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "LuckyHits" })]);
    // L114
    modDB.NewMod(n!("Convergence"), ModType::FLAG, true, "Base", 0, 0, &[tag!({ type = Condition, var = "Convergence" })]);
    // L115
    modDB.NewMod(n!("PhysicalDamageReduction"), ModType::BASE, -15.0, "Base", 0, 0, &[tag!({ type = Condition, var = "Crushed" })]);
    // L116
    modDB.NewMod(n!("CritChanceCap"), ModType::BASE, 100.0, "Base", 0, 0, &[]);

    // L117
    modDB.conditions.set(n!("Buffed"), Val::Bool(env.mode_buffs.get()));
    // L118
    modDB.conditions.set(n!("Combat"), Val::Bool(env.mode_combat.get()));
    // L119
    modDB.conditions.set(n!("Effective"), Val::Bool(env.mode_effective.get()));
}
