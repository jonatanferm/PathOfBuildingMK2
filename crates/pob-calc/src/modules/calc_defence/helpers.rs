//! @pob-file Modules/CalcDefence.lua @16de4b82d
//! CalcDefence.lua 1-650: module header and defence helpers.
//!
//! Owner: DF-A. Signatures are U0's (frozen; change through a CCR).
#![allow(non_snake_case, non_upper_case_globals)]
// literal Lua shapes (`a = a + b`, `x >= 0 and x or 0`, …)
#![allow(
    clippy::assign_op_pattern,
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::many_single_char_names
)]
#![allow(unused_assignments)]
// module-level lists and helpers used by DF-B..G
#![allow(dead_code)]

use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::mod_tools;

// @pob Modules/CalcDefence.lua:7-43 h=fbb8bcde
// (module-level statements: calcs,pairs,ipairs,t_insert,m_min,m_max,m_abs,m_floor,m_ceil,m_sqrt,m_modf,m_huge,s_format,tempTable…)
// L22 `local tempTable1 = { }` is a scratch table that does not escape (calc doc §6:
// CalcDefence 862 `wipeTable(tempTable1)`), so DF-B keeps it as a local.

/// L24 `local isElemental = { Fire = true, Cold = true, Lightning = true }`.
pub(crate) fn isElemental(t: &str) -> bool {
    matches!(t, "Fire" | "Cold" | "Lightning")
}

// -- List of all damage types, ordered according to the conversion sequence
/// L27
pub(crate) const hitSourceList: [&str; 2] = ["Attack", "Spell"];
/// L28
pub(crate) const dmgTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

/// L30
pub(crate) const resistTypeList: [&str; 4] = ["Fire", "Cold", "Lightning", "Chaos"];

/// One row of `allyLifePoolList` (absent fields are nil).
pub(crate) struct AllyLifePool {
    pub key: &'static str,
    pub life: &'static str,
    pub mitigation: &'static str,
    pub redirect: Option<&'static str>,
    pub fallback: Option<&'static str>,
    pub meleeOnly: bool,
    pub label: &'static str,
}

const fn ally(
    key: &'static str,
    life: &'static str,
    mitigation: &'static str,
    redirect: Option<&'static str>,
    fallback: Option<&'static str>,
    meleeOnly: bool,
    label: &'static str,
) -> AllyLifePool {
    AllyLifePool {
        key,
        life,
        mitigation,
        redirect,
        fallback,
        meleeOnly,
        label,
    }
}

// -- Damage is shifted to external allies before Frost Shield, so this is also the order in which pools are drained.
/// L33-43
pub(crate) static allyLifePoolList: [AllyLifePool; 9] = [
    ally(
        "minion",
        "TotalMinionLife",
        "MinionAllyDamageMitigation",
        Some("takenFromMinionBeforeYou"),
        Some("Multiplier:MinionLife"),
        false,
        "Minion Life",
    ),
    ally(
        "radianceSentinel",
        "TotalRadianceSentinelLife",
        "RadianceSentinelAllyDamageMitigation",
        Some("takenFromRadianceSentinelBeforeYou"),
        None,
        false,
        "Total Sentinel of Radiance Life",
    ),
    ally(
        "spectres",
        "TotalSpectreLife",
        "SpectreAllyDamageMitigation",
        Some("takenFromSpectresBeforeYou"),
        None,
        false,
        "Total Spectre Life",
    ),
    ally(
        "totems",
        "TotalTotemLife",
        "TotemAllyDamageMitigation",
        Some("takenFromTotemsBeforeYou"),
        None,
        false,
        "Nearest Totem Life",
    ),
    ally(
        "vaalRejuvenationTotems",
        "TotalVaalRejuvenationTotemLife",
        "VaalRejuvenationTotemAllyDamageMitigation",
        Some("takenFromVaalRejuvenationTotemsBeforeYou"),
        None,
        false,
        "Total Vaal Rejuvenation Totem Life",
    ),
    ally(
        "voidSpawn",
        "TotalVoidSpawnLife",
        "VoidSpawnAllyDamageMitigation",
        Some("takenFromVoidSpawnBeforeYou"),
        None,
        false,
        "Total Void Spawn Life",
    ),
    ally(
        "stoneGolem",
        "TotalStoneGolemLife",
        "StoneGolemAllyDamageMitigation",
        Some("takenFromStoneGolemBeforeYou"),
        None,
        true,
        "Total Stone Golem Life",
    ),
    ally(
        "soulLink",
        "AlliedEnergyShield",
        "SoulLinkMitigation",
        None,
        None,
        false,
        "Total Allied Energy shield",
    ),
    ally(
        "frostShield",
        "FrostShieldLife",
        "FrostShieldDamageMitigation",
        None,
        None,
        false,
        "Frost Shield Life",
    ),
];

// ---- Lua operand helpers (plumbing) ------------------------------------------------------------

/// A comparison operand (`x > 0`): only a number compares with a number; anything
/// else raises "attempt to compare …" as in Lua.
fn cmpnum(v: &Val) -> f64 {
    match v {
        Val::Num(n) => *n,
        other => lua_error!("attempt to compare {} with number", other.lua_type()),
    }
}

/// `x ~= 0` on a dynamic value (nil and non-numbers are never equal to 0).
fn ne0(v: &Val) -> bool {
    !matches!(v, Val::Num(n) if *n == 0.0)
}

/// A mod value or query result used in arithmetic (`mod.value * rate`).
fn vnum(v: &Value) -> f64 {
    match v.num() {
        Some(n) => n,
        None => match v.as_str().and_then(pob_lua::tonumber) {
            Some(n) => n,
            None => lua_error!("attempt to perform arithmetic on a {} value", v.lua_type()),
        },
    }
}

/// `modDB:Override(…) or <fallback>`: the override when it is truthy.
fn override_or(v: Value, fallback: impl FnOnce() -> f64) -> f64 {
    if v.truthy() {
        vnum(&v)
    } else {
        fallback()
    }
}

/// `t[k]` used as a table (index of a non-table raises).
fn index_table(v: Val, what: &str) -> Option<Table> {
    match v {
        Val::Table(t) => Some(t),
        Val::Nil | Val::Bool(false) => None,
        other => lua_error!("attempt to index {what} (a {} value)", other.lua_type()),
    }
}

/// `t.k` where `t` must be a table.
fn sub_table(t: &Table, k: Name, what: &str) -> Table {
    match index_table(t.get(k), what) {
        Some(t) => t,
        None => lua_error!("attempt to index field '{what}' (a nil value)"),
    }
}

/// `colorCodes.<name>`.
fn colorCode(name: &str) -> &'static str {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| lua_error!("attempt to concatenate field '{name}' (a nil value)"))
}

/// `tempDamage >= 1 and tempDamage or nil`.
fn atLeastOne(x: f64) -> Val {
    if x >= 1.0 {
        Val::Num(x)
    } else {
        Val::Nil
    }
}

/// `actor.modDB` / `actor.output`.
fn actor_parts(actor: &ActorRef) -> (Output, ModDB) {
    (actor.output.get(), actor.modDB.get())
}

// ---- ported functions --------------------------------------------------------------------------

/// @pob Modules/CalcDefence.lua:45-55 h=0fa67014
pub(crate) fn buildAllyLifePools(output: &Output) -> Table {
    // L46
    let pools = Table::new();
    // L47
    for ally in &allyLifePoolList {
        // L48
        let life = output.get(Name::from(ally.life));
        // L49
        let mitigation = output.get(Name::from(ally.mitigation));
        // L50
        if life.truthy() && cmpnum(&life) > 0.0 && mitigation.truthy() && cmpnum(&mitigation) > 0.0
        {
            // L51
            pools.set(
                Name::from(ally.key),
                tbl! { remaining = life.clone(), percent = m_min(mitigation.num(), 100.0) / 100.0 },
            );
        }
    }
    // L54
    pools
}

/// @pob Modules/CalcDefence.lua:58-64 h=61fc1712
/// Calculate hit chance
pub fn hitChance(evasion: f64, accuracy: f64) -> f64 {
    // L59
    if accuracy < 0.0 {
        // L60
        return 5.0;
    }
    // L62
    let rawChance = accuracy / (accuracy + lpow(evasion / 5.0, 0.9)) * 125.0;
    // L63
    m_max(m_min(round(rawChance, None), 100.0), 5.0)
}

/// @pob Modules/CalcDefence.lua:67-72 h=6dcb2e6c
/// Calculate damage reduction from armour, float
pub fn armourReductionF(armour: f64, raw: f64) -> f64 {
    // L68
    if armour == 0.0 && raw == 0.0 {
        // L69
        return 0.0;
    }
    // L71
    armour / (armour + raw * 5.0) * 100.0
}

/// @pob Modules/CalcDefence.lua:75-77 h=f9d1d27f
/// Calculate damage reduction from armour, int
pub fn armourReduction(armour: f64, raw: f64) -> f64 {
    // L76
    round(armourReductionF(armour, raw), None)
}

/// @pob Modules/CalcDefence.lua:84-143 h=e1281963
/// Based on code from FR and BS found in act_*.txt
/// → (damageBreakdown, totalDamageTaken).
pub fn applyDmgTakenConversion(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
    sourceType: Name,
    baseDmg: f64,
) -> (Table, f64) {
    let skillModList = activeSkill.skillModList.get();
    let el = isElemental(sourceType.as_str());
    // L85
    let damageBreakdown = Table::new();
    // L86
    let mut totalDamageTaken = 0.0;
    // L87 (`shiftTable[damageType]`, keyed by the five dmgTypeList entries)
    let mut shiftTable = [0.0_f64; 5];
    // L88
    let mut totalTakenAs = 0.0;
    // L89
    for (i, damageType) in dmgTypeList.iter().copied().enumerate() {
        // L90
        shiftTable[i] = skillModList.Sum(
            ModType::BASE,
            None,
            &names([
                Some(cat!(sourceType, "DamageTakenAs", damageType)),
                Some(cat!(sourceType, "DamageFromHitsTakenAs", damageType)),
                el.then(|| cat!("ElementalDamageTakenAs", damageType)),
                el.then(|| cat!("ElementalDamageFromHitsTakenAs", damageType)),
            ]),
        );
        // L91
        totalTakenAs = totalTakenAs + shiftTable[i];
    }
    // L93
    for (i, damageType) in dmgTypeList.iter().copied().enumerate() {
        // L94
        let damageTakenAs = if damageType == sourceType.as_str() {
            m_max(1.0 - totalTakenAs / 100.0, 0.0)
        } else {
            shiftTable[i] / 100.0
        };

        // L96
        if damageTakenAs != 0.0 {
            // L97
            let damage = baseDmg * damageTakenAs;

            // L99
            let mut baseTakenInc = skillModList.Sum(
                ModType::INC,
                None,
                &[
                    n!("DamageTaken"),
                    cat!(damageType, "DamageTaken"),
                    n!("DamageTakenWhenHit"),
                    cat!(damageType, "DamageTakenWhenHit"),
                ],
            );
            // L100
            let mut baseTakenMore = skillModList.More(
                None,
                &[
                    n!("DamageTaken"),
                    cat!(damageType, "DamageTaken"),
                    n!("DamageTakenWhenHit"),
                    cat!(damageType, "DamageTakenWhenHit"),
                ],
            );
            // L101
            if damageType == "Lightning" || damageType == "Cold" || damageType == "Fire" {
                // L102
                baseTakenInc = baseTakenInc
                    + skillModList.Sum(
                        ModType::INC,
                        None,
                        nm!["ElementalDamageTaken", "ElementalDamageTakenWhenHit"],
                    );
                // L103
                baseTakenMore = baseTakenMore
                    * skillModList.More(
                        None,
                        nm!["ElementalDamageTaken", "ElementalDamageTakenWhenHit"],
                    );
            }
            // L105
            let damageTakenMods = m_max((1.0 + baseTakenInc / 100.0) * baseTakenMore, 0.0);
            // L106
            let reduction = if skillModList.Flag(
                None,
                &[cat!("SelfIgnore", "Base", damageType, "DamageReduction")],
            ) {
                Val::Num(0.0)
            } else {
                lor(
                    output.get(cat!("Base", damageType, "DamageReductionWhenHit")),
                    || output.get(cat!("Base", damageType, "DamageReduction")),
                )
            };
            // L107
            let resist = if skillModList.Flag(None, &[cat!("SelfIgnore", damageType, "Resistance")])
            {
                Val::Num(0.0)
            } else {
                lor(output.get(cat!(damageType, "ResistWhenHit")), || {
                    output.get(cat!(damageType, "Resist"))
                })
            };
            // L108
            let mut armourReduct = 0.0;
            // L109
            let resMult = 1.0 - resist.num() / 100.0;
            // L110
            let mut reductMult: f64 = 1.0;

            // L112
            let percentOfArmourApplies = m_min(
                if !skillModList.Flag(
                    None,
                    &[cat!("ArmourDoesNotApplyTo", damageType, "DamageTaken")],
                ) {
                    skillModList.Sum(
                        ModType::BASE,
                        None,
                        &[cat!("ArmourAppliesTo", damageType, "DamageTaken")],
                    )
                } else {
                    0.0
                },
                100.0,
            );
            // L113
            let physicalReductionBasedOnWard = damageType == "Physical"
                && skillModList.Flag(None, nm!["PhysicalReductionBasedOnWard"]);
            // L114
            if percentOfArmourApplies > 0.0 || physicalReductionBasedOnWard {
                // L115
                let mut effArmour = (output.num(n!("Armour")) * percentOfArmourApplies / 100.0)
                    * (1.0 + output.num(n!("ArmourDefense")));
                // L116
                if physicalReductionBasedOnWard {
                    // L117
                    let multiplier = vnum(
                        &skillModList.Override(None, nm!["PhysicalReductionBasedOnWardPercent"]),
                    ) / 100.0;
                    // L118
                    effArmour = output.num(n!("Ward")) * multiplier;
                }
                // L120
                let effDamage = damage * resMult;
                // L121
                armourReduct = round(
                    if effArmour != 0.0 && damage * resMult != 0.0 {
                        effArmour / (effArmour + effDamage * 5.0) * 100.0
                    } else {
                        0.0
                    },
                    None,
                );
                // L122
                armourReduct = m_min(output.num(n!("DamageReductionMax")), armourReduct);
            }
            // L124
            reductMult = (1.0
                - m_max(
                    m_min(
                        output.num(n!("DamageReductionMax")),
                        armourReduct + reduction.num(),
                    ),
                    0.0,
                ) / 100.0)
                * damageTakenMods;
            // L125
            let combinedMult = resMult * reductMult;
            // L126
            let finalDamage = damage * combinedMult;
            // L127
            totalDamageTaken = totalDamageTaken + finalDamage;

            // L129
            if breakdown.is_some() {
                // L130
                damageBreakdown.push(format!("{damageType} Damage Taken"));
                // L131
                if damageTakenAs != 1.0 {
                    // L132
                    damageBreakdown.push(lfmt!(
                        "^8=^7 %d^8 (Base Damage)^7 * %.2f^8 (Damage taken as %s)",
                        baseDmg,
                        damageTakenAs,
                        damageType
                    ));
                }
                // L134
                if combinedMult != 1.0 {
                    // L135
                    damageBreakdown.push(lfmt!(
                        "^8=^7 %d^8 (%s Damage)^7 * %.4f^8 (Damage taken multi)",
                        damage,
                        damageType,
                        combinedMult
                    ));
                }
                // L137
                damageBreakdown.push(lfmt!(
                    "^8=^7 %d^8 (%s Damage taken)",
                    finalDamage,
                    damageType
                ));
                // L138
                damageBreakdown.push(lfmt!(""));
            }
        }
    }
    // L142
    (damageBreakdown, totalDamageTaken)
}

/// @pob Modules/CalcDefence.lua:150-177 h=b88dfa45
/// Calculates the taken damages from enemy outgoing damage
/// → (receivedDamageSum, damages).
pub fn takenHitFromDamage(rawDamage: f64, damageType: Name, actor: &ActorRef) -> (f64, Table) {
    // L152-153
    let (output, modDB) = actor_parts(actor);
    // L154
    let damageMitigationMultiplierForType = |damage: f64, r#type: Name| -> f64 {
        // L155
        let totalResistMult = output.get(cat!(r#type, "ResistTakenHitMulti"));
        // L156
        let effectiveAppliedArmour = output.get(cat!(r#type, "EffectiveAppliedArmour"));
        // L157
        let armourDRPercent =
            armourReductionF(effectiveAppliedArmour.num(), damage * totalResistMult.num());
        // L158
        let flatDRPercent = if modDB.Flag(
            None,
            &[cat!("SelfIgnore", "Base", r#type, "DamageReduction")],
        ) {
            Val::Num(0.0)
        } else {
            lor(
                output.get(cat!("Base", r#type, "DamageReductionWhenHit")),
                || output.get(cat!("Base", r#type, "DamageReduction")),
            )
        };
        // L159
        let totalDRPercent = m_min(
            output.num(n!("DamageReductionMax")),
            armourDRPercent + flatDRPercent.num(),
        );
        // L160
        let enemyOverwhelmPercent =
            if modDB.Flag(None, &[cat!("SelfIgnore", r#type, "DamageReduction")]) {
                Val::Num(0.0)
            } else {
                output.get(cat!(r#type, "EnemyOverwhelm"))
            };
        // L161
        let totalDRMulti = 1.0
            - m_max(
                m_min(
                    output.num(n!("DamageReductionMax")),
                    totalDRPercent - enemyOverwhelmPercent.num(),
                ),
                0.0,
            ) / 100.0;
        // L162
        totalResistMult.num() * totalDRMulti
    };
    // L164
    let mut receivedDamageSum = 0.0;
    // L165
    let damages = Table::new();
    // L166
    let Some(shiftTables) = actor.damageShiftTable.get() else {
        lua_error!("attempt to index field 'damageShiftTable' (a nil value)")
    };
    let Some(shift) = index_table(shiftTables.get(damageType), "damageShiftTable") else {
        lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
    };
    // LUA-ORDER: string keys; `receivedDamageSum` is summed in pairs order (random per
    // PoB process, calc doc §3.4).
    for (damageConvertedType, convertPercent) in shift.pairs() {
        let Some(damageConvertedType) = damageConvertedType.name() else {
            lua_error!("takenHitFromDamage: non-string damage type key")
        };
        // L167
        let takenFlat = output.get(cat!(damageConvertedType, "takenFlat"));
        // L168
        if cmpnum(&convertPercent) > 0.0 || ne0(&takenFlat) {
            // L169
            let convertedDamage = rawDamage * convertPercent.num() / 100.0;
            // L170
            let vaalArctic = m_min(
                -modDB.Sum(ModType::MORE, None, nm!["VaalArcticArmourMitigation"]) / 100.0,
                1.0,
            );
            // L171
            let reducedDamage = round(
                m_max(
                    convertedDamage
                        * damageMitigationMultiplierForType(convertedDamage, damageConvertedType)
                        + takenFlat.num(),
                    0.0,
                ) * output.num(cat!(damageConvertedType, "AfterReductionTakenHitMulti")),
                None,
            ) * (1.0 - vaalArctic);
            // L172
            receivedDamageSum = receivedDamageSum + reducedDamage;
            // L173
            damages.set(
                damageConvertedType,
                if reducedDamage > 0.0 || cmpnum(&convertPercent) > 0.0 {
                    Val::Num(reducedDamage)
                } else {
                    Val::Nil
                },
            );
        }
    }
    // L176
    (receivedDamageSum, damages)
}

/// @pob Modules/CalcDefence.lua:179-183 h=735ad761
pub(crate) fn calcLifeHitPoolWithLossPrevention(
    life: f64,
    maxLife: f64,
    lifeLossPrevented: f64,
    lifeLossBelowHalfPrevented: f64,
) -> f64 {
    // L180
    let halfLife = maxLife * 0.5;
    // L181
    let aboveLow = m_max(life - halfLife, 0.0);
    // L182
    aboveLow / (1.0 - lifeLossPrevented / 100.0)
        + m_min(life, halfLife)
            / (1.0 - lifeLossBelowHalfPrevented / 100.0)
            / (1.0 - lifeLossPrevented / 100.0)
}

/// @pob Modules/CalcDefence.lua:190-424 h=8c0e4fc8
/// Helper function that reduces pools according to damage taken
pub fn reducePoolsByDamage(
    poolTable: Option<&Table>,
    damageTable: &Table,
    actor: &ActorRef,
) -> Table {
    // L192-193
    let (output, modDB) = actor_parts(actor);
    // L194
    let poolTbl = poolTable.cloned().unwrap_or_default();

    // L196
    let mut damageTotal = 0.0;
    // L197
    for (damageType, damage) in damageTable.pairs() {
        // L198
        damageTable.set(
            damageType,
            if cmpnum(&damage) > 0.0 {
                Val::Num(m_ceil(damage.num()))
            } else {
                Val::Nil
            },
        );
        // L199
        damageTotal = damageTotal + damageTable.get(damageType).num_or(0.0);
    }

    // L202
    let mut alliesTakenBeforeYou = index_table(
        poolTbl.get(n!("AlliesTakenBeforeYou")),
        "AlliesTakenBeforeYou",
    );
    // L203
    if alliesTakenBeforeYou.is_none() {
        // L204
        alliesTakenBeforeYou = Some(buildAllyLifePools(&output));
    }
    let alliesTakenBeforeYou = alliesTakenBeforeYou.unwrap_or_default();

    // L207
    let damageTakenThatCanBeRecouped = index_table(
        poolTbl.get(n!("damageTakenThatCanBeRecouped")),
        "damageTakenThatCanBeRecouped",
    )
    .unwrap_or_default();
    // L208
    let mut aegis = index_table(poolTbl.get(n!("Aegis")), "aegis");
    // L209
    if aegis.is_none() {
        // L210-213
        let t = tbl! {
            shared = output.num_or(n!("sharedAegis"), 0.0),
            sharedElemental = output.num_or(n!("sharedElementalAegis"), 0.0)
        };
        // L214
        for (damageType, _) in damageTable.pairs() {
            let Some(dt) = damageType.name() else {
                lua_error!("attempt to concatenate a non-string damage type")
            };
            // L215
            t.set(dt, output.num_or(cat!(dt, "Aegis"), 0.0));
        }
        aegis = Some(t);
    }
    let aegis = aegis.unwrap_or_default();
    // L218
    let mut guard = index_table(poolTbl.get(n!("Guard")), "guard");
    // L219
    if guard.is_none() {
        // L220
        let t = tbl! { shared = output.num_or(n!("sharedGuardAbsorb"), 0.0) };
        // L221
        for (damageType, _) in damageTable.pairs() {
            let Some(dt) = damageType.name() else {
                lua_error!("attempt to concatenate a non-string damage type")
            };
            // L222
            t.set(dt, output.num_or(cat!(dt, "GuardAbsorb"), 0.0));
        }
        guard = Some(t);
    }
    let guard = guard.unwrap_or_default();

    // L226
    let ward = lor(
        lor(poolTbl.get(n!("Ward")), || output.get(n!("Ward"))),
        || Val::Num(0.0),
    );
    // L227
    let wardActiveChance = lor(poolTbl.get(n!("WardActiveChance")), || {
        Val::Num(if cmpnum(&ward) > 0.0 { 1.0 } else { 0.0 })
    });
    // L228
    let wardAvoidBreakChance = if modDB.Flag(None, nm!["Condition:WardNotBreak"]) {
        1.0
    } else {
        m_min(
            modDB.Sum(ModType::BASE, None, nm!["WardAvoidBreakChance"]) / 100.0,
            1.0,
        )
    };
    // L229
    let wardBypassBelow = modDB.Sum(ModType::BASE, None, nm!["WardBypassBelowPercent"]) / 100.0;

    // L231
    let energyShield = lor(poolTbl.get(n!("EnergyShield")), || {
        output.get(n!("EnergyShieldRecoveryCap"))
    });
    // L232
    let mana = lor(
        lor(poolTbl.get(n!("Mana")), || output.get(n!("ManaUnreserved"))),
        || Val::Num(0.0),
    );
    // L233
    let life = lor(
        lor(poolTbl.get(n!("Life")), || {
            output.get(n!("LifeRecoverable"))
        }),
        || Val::Num(0.0),
    );
    // L234
    let lifeLossBelowHalfPrevented =
        modDB.Sum(ModType::BASE, None, nm!["LifeLossBelowHalfPrevented"]);
    // L235
    let mut LifeLossLostOverTime = poolTbl.num_or(n!("LifeLossLostOverTime"), 0.0);
    // L236
    let mut LifeBelowHalfLossLostOverTime =
        poolTbl.num_or(n!("LifeBelowHalfLossLostOverTime"), 0.0);
    // L237
    let mut overkillDamage = 0.0;

    // L239
    let mut ward: f64 = if cmpnum(&ward) >= 0.0 {
        ward.num()
    } else {
        0.0
    };
    // L240
    let wardBeforeHit = ward;
    // L241
    let mut energyShield: f64 = if cmpnum(&energyShield) >= 0.0 {
        energyShield.num()
    } else {
        0.0
    };
    // L242
    let mut mana: f64 = if cmpnum(&mana) >= 0.0 {
        mana.num()
    } else {
        0.0
    };
    // L243
    let mut life: f64 = if cmpnum(&life) >= 0.0 {
        life.num()
    } else {
        0.0
    };

    // -- Initializing MoM(+EB(+Bypass)) pools here saves logic later to avoid overusing pools for every damageType
    // L246
    let resourcesLostToTypeDamage = tbl! {
        Physical = Table::new(),
        Lightning = Table::new(),
        Cold = Table::new(),
        Fire = Table::new(),
        Chaos = Table::new()
    };
    // L247
    let lifeHitPoolInitial = calcLifeHitPoolWithLossPrevention(
        life,
        output.num(n!("Life")),
        output.num(n!("preventedLifeLoss")),
        lifeLossBelowHalfPrevented,
    );
    // L248
    let MoMPoolsRemaining = Table::new();
    // L249
    let esPoolsRemaining = Table::new();
    // L250
    for (damageType, _) in damageTable.pairs() {
        let Some(dt) = damageType.name() else {
            lua_error!("attempt to concatenate a non-string damage type")
        };
        // L251
        let MoMEffect = m_min(
            output.num(n!("sharedMindOverMatter")) + output.num(cat!(dt, "MindOverMatter")),
            100.0,
        ) / 100.0;
        // L252
        let maxMoM = lifeHitPoolInitial / (1.0 - MoMEffect) - lifeHitPoolInitial;
        // L253
        let MoMPool = if MoMEffect < 1.0 {
            m_min(maxMoM, mana)
        } else {
            mana
        };
        // L254
        let lifePlusMoMHitPool = lifeHitPoolInitial + MoMPool;
        // L255 (`/ 100 or 0`: the quotient is never nil)
        let esBypass = output.num(cat!(dt, "EnergyShieldBypass")) / 100.0;
        // L256
        let esPool = if esBypass > 0.0 {
            m_min(
                lifePlusMoMHitPool / esBypass - lifePlusMoMHitPool,
                energyShield,
            )
        } else {
            energyShield
        };
        // L257
        if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) && esBypass < 1.0 {
            // L258
            esPoolsRemaining.set(
                dt,
                m_floor(m_min_n(&[
                    maxMoM * (1.0 - esBypass),
                    energyShield,
                    lifePlusMoMHitPool / (1.0 - (1.0 - esBypass) * MoMEffect) - lifePlusMoMHitPool,
                ])),
            );
            // L259
            MoMPoolsRemaining.set(
                dt,
                m_floor(m_min(maxMoM - esPoolsRemaining.num(dt), MoMPool)),
            );
        } else {
            // L261
            esPoolsRemaining.set(dt, m_floor(esPool));
            // L262
            MoMPoolsRemaining.set(dt, m_floor(MoMPool));
        }
    }

    // L266
    for damageType in dmgTypeList {
        let dt = Name::from(damageType);
        let rl = resourcesLostToTypeDamage
            .table(dt)
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
        // L267
        let damageRemainder = damageTable.get(dt);
        // L268
        if damageRemainder.truthy() {
            let mut damageRemainder = damageRemainder.num();
            // L269
            for ally in &allyLifePoolList {
                // L270
                let allyValues =
                    index_table(alliesTakenBeforeYou.get(Name::from(ally.key)), "allyValues");
                // L271
                if let Some(allyValues) = allyValues {
                    let allyDamageType = allyValues.get(n!("damageType"));
                    if !allyDamageType.truthy() || allyDamageType.raw_eq(&Val::from(dt)) {
                        // L272
                        if cmpnum(&allyValues.get(n!("remaining"))) > 0.0 {
                            // L273
                            let tempDamage = m_min(
                                damageRemainder * allyValues.num(n!("percent")),
                                allyValues.num(n!("remaining")),
                            );
                            // L274
                            allyValues.set(
                                n!("remaining"),
                                m_floor(allyValues.num(n!("remaining")) - tempDamage),
                            );
                            // L275
                            damageRemainder = damageRemainder - tempDamage;
                            // L276
                            rl.set(Name::from(ally.key), atLeastOne(tempDamage));
                        }
                    }
                }
            }
            // -- frost shield / soul link / other taken before you does not count as you taking damage
            // L281
            damageTakenThatCanBeRecouped.set(
                dt,
                damageTakenThatCanBeRecouped.num_or(dt, 0.0) + damageRemainder,
            );
            // L282
            if cmpnum(&aegis.get(dt)) > 0.0 {
                // L283
                let tempDamage = m_min(damageRemainder, aegis.num(dt));
                // L284
                aegis.set(dt, aegis.num(dt) - tempDamage);
                // L285
                damageRemainder = damageRemainder - tempDamage;
                // L286
                rl.set(n!("aegis"), atLeastOne(tempDamage));
            }
            // L288
            if isElemental(damageType) && cmpnum(&aegis.get(n!("sharedElemental"))) > 0.0 {
                // L289
                let tempDamage = m_min(damageRemainder, aegis.num(n!("sharedElemental")));
                // L290
                aegis.set(
                    n!("sharedElemental"),
                    aegis.num(n!("sharedElemental")) - tempDamage,
                );
                // L291
                damageRemainder = damageRemainder - tempDamage;
                // L292
                rl.set(n!("sharedElementalAegis"), atLeastOne(tempDamage));
            }
            // L294
            if cmpnum(&aegis.get(n!("shared"))) > 0.0 {
                // L295
                let tempDamage = m_min(damageRemainder, aegis.num(n!("shared")));
                // L296
                aegis.set(n!("shared"), aegis.num(n!("shared")) - tempDamage);
                // L297
                damageRemainder = damageRemainder - tempDamage;
                // L298
                rl.set(n!("sharedAegis"), atLeastOne(tempDamage));
            }
            // L300
            if cmpnum(&guard.get(dt)) > 0.0 {
                // L301
                let tempDamage = m_min(
                    damageRemainder * output.num(cat!(dt, "GuardAbsorbRate")) / 100.0,
                    guard.num(dt),
                );
                // L302
                guard.set(dt, m_floor(guard.num(dt) - tempDamage));
                // L303
                damageRemainder = damageRemainder - tempDamage;
                // L304
                rl.set(n!("guard"), atLeastOne(tempDamage));
            }
            // L306
            if cmpnum(&guard.get(n!("shared"))) > 0.0 {
                // L307
                let tempDamage = m_min(
                    damageRemainder * output.num(n!("sharedGuardAbsorbRate")) / 100.0,
                    guard.num(n!("shared")),
                );
                // L308
                guard.set(n!("shared"), m_floor(guard.num(n!("shared")) - tempDamage));
                // L309
                damageRemainder = damageRemainder - tempDamage;
                // L310
                rl.set(n!("sharedGuard"), atLeastOne(tempDamage));
            }
            // L312
            if ward > 0.0
                && (wardBypassBelow == 0.0 || damageTotal >= wardBeforeHit * wardBypassBelow)
            {
                // L313
                let mut tempDamage = m_min(
                    damageRemainder
                        * (1.0 - (modDB.Sum(ModType::BASE, None, nm!["WardBypass"])) / 100.0),
                    ward,
                );
                // L314
                ward = ward - tempDamage;
                // L315
                tempDamage = tempDamage * wardActiveChance.num();
                // L316
                damageRemainder = damageRemainder - tempDamage;
                // L317
                rl.set(n!("ward"), atLeastOne(tempDamage));
            }
            // L319
            let MoMEffect = m_min(
                output.num(n!("sharedMindOverMatter")) + output.num(cat!(dt, "MindOverMatter")),
                100.0,
            ) / 100.0;
            // L320
            let esBypass = output.num(cat!(dt, "EnergyShieldBypass")) / 100.0;
            // L321
            let esPool = esPoolsRemaining.get(dt);
            // L322
            if energyShield > 0.0
                && !modDB.Flag(None, nm!["EnergyShieldProtectsMana"])
                && esBypass < 1.0
            {
                // L323
                let tempDamage = m_min(damageRemainder * (1.0 - esBypass), esPool.num());
                // L324
                for (damageType2, _) in damageTable.pairs() {
                    // L325
                    esPoolsRemaining.set(
                        damageType2,
                        m_max(esPoolsRemaining.num(damageType2) - tempDamage, 0.0),
                    );
                }
                // L327
                energyShield = energyShield - tempDamage;
                // L328
                damageRemainder = damageRemainder - tempDamage;
                // L329
                rl.set(n!("energyShield"), atLeastOne(tempDamage));
            }
            // L331
            if MoMEffect > 0.0 {
                // L332
                let mut MoMDamage = m_ceil(damageRemainder * MoMEffect);
                // L333
                if modDB.Flag(None, nm!["EnergyShieldProtectsMana"])
                    && energyShield > 0.0
                    && esBypass < 1.0
                {
                    // L334
                    let tempDamage = m_ceil(m_min(MoMDamage * (1.0 - esBypass), esPool.num()));
                    // L335
                    for (damageType2, _) in damageTable.pairs() {
                        // L336
                        esPoolsRemaining.set(
                            damageType2,
                            m_floor(m_max(esPoolsRemaining.num(damageType2) - tempDamage, 0.0)),
                        );
                    }
                    // L338
                    energyShield = energyShield - tempDamage;
                    // L339
                    damageRemainder = damageRemainder - tempDamage;
                    // L340
                    MoMDamage = MoMDamage - tempDamage;
                    // L341
                    rl.set(n!("energyShield"), atLeastOne(tempDamage));
                }
                // L343
                let tempDamage = m_ceil(m_min(MoMDamage, MoMPoolsRemaining.num(dt)));
                // L344
                for (damageType2, _) in damageTable.pairs() {
                    // L345
                    MoMPoolsRemaining.set(
                        damageType2,
                        m_floor(m_max(MoMPoolsRemaining.num(damageType2) - tempDamage, 0.0)),
                    );
                }
                // L347
                mana = mana - tempDamage;
                // L348
                damageRemainder = damageRemainder - tempDamage;
                // L349
                rl.set(n!("mana"), atLeastOne(tempDamage));
            }
            // L351
            if cmpnum(&output.get(n!("preventedLifeLossTotal"))) > 0.0 {
                // L352
                let halfLife = output.num(n!("Life")) * 0.5;
                // L353
                let lifeOverHalfLife = m_max(life - halfLife, 0.0);
                // L354
                let preventPercent = output.num(n!("preventedLifeLoss")) / 100.0;
                // L355
                let poolAboveLow = lifeOverHalfLife / (1.0 - preventPercent);
                // L356
                let preventBelowHalfPercent = lifeLossBelowHalfPrevented / 100.0;
                // L357
                let damageThatLifeCanStillTake = poolAboveLow
                    + m_max(m_min(life, halfLife), 0.0)
                        / (1.0 - preventBelowHalfPercent)
                        / (1.0 - output.num(n!("preventedLifeLoss")) / 100.0);
                // L358
                if damageThatLifeCanStillTake < damageRemainder {
                    // L359
                    overkillDamage = overkillDamage + damageRemainder - damageThatLifeCanStillTake;
                    // L360
                    damageRemainder = damageThatLifeCanStillTake;
                }
                // L362
                if ne0(&output.get(n!("preventedLifeLossBelowHalf"))) {
                    // L363
                    let damageToSplit = m_min(damageRemainder, poolAboveLow);
                    // L364
                    let lostLife = damageToSplit * (1.0 - preventPercent);
                    // L365
                    let preventedLoss = damageToSplit * preventPercent;
                    // L366
                    damageRemainder = damageRemainder - damageToSplit;
                    // L367
                    LifeLossLostOverTime = LifeLossLostOverTime + preventedLoss;
                    // L368
                    life = life - lostLife;
                    // L369
                    rl.set(n!("life"), lostLife);
                    // L370
                    rl.set(n!("lifeLossPrevented"), preventedLoss);
                    // L371
                    if life <= halfLife {
                        // L372
                        let unspecificallyLowLifePreventedDamage = damageRemainder * preventPercent;
                        // L373
                        LifeLossLostOverTime =
                            LifeLossLostOverTime + unspecificallyLowLifePreventedDamage;
                        // L374
                        damageRemainder = damageRemainder - unspecificallyLowLifePreventedDamage;
                        // L375
                        let specificallyLowLifePreventedDamage =
                            damageRemainder * preventBelowHalfPercent;
                        // L376
                        LifeBelowHalfLossLostOverTime =
                            LifeBelowHalfLossLostOverTime + specificallyLowLifePreventedDamage;
                        // L377
                        damageRemainder = damageRemainder - specificallyLowLifePreventedDamage;
                        // L378
                        rl.set(
                            n!("lifeLossPrevented"),
                            rl.num(n!("lifeLossPrevented"))
                                + unspecificallyLowLifePreventedDamage
                                + specificallyLowLifePreventedDamage,
                        );
                    }
                    // L380
                    rl.set(
                        n!("lifeLossPrevented"),
                        atLeastOne(rl.num(n!("lifeLossPrevented"))),
                    );
                } else {
                    // L382
                    let tempDamage = damageRemainder * output.num(n!("preventedLifeLoss")) / 100.0;
                    // L383
                    LifeLossLostOverTime = LifeLossLostOverTime + tempDamage;
                    // L384
                    damageRemainder = damageRemainder - tempDamage;
                    // L385
                    rl.set(n!("lifeLossPrevented"), atLeastOne(tempDamage));
                }
            }
            // L388
            if life > 0.0 {
                // L389
                let tempDamage = m_min(damageRemainder, life);
                // L390
                life = life - tempDamage;
                // L391
                damageRemainder = damageRemainder - tempDamage;
                // L392
                rl.set(
                    n!("life"),
                    rl.num_or(n!("life"), 0.0) + (if tempDamage > 0.0 { tempDamage } else { 0.0 }),
                );
            }
            // L394
            overkillDamage = overkillDamage + damageRemainder;
            // L395
            rl.set(n!("overkill"), atLeastOne(damageRemainder));
        }
    }

    // L399
    let mut MoMPoolRemaining = m_huge;
    // L400
    let mut esPoolRemaining = m_huge;
    // L401
    // LUA-ORDER: string keys (random per PoB process, calc doc §3.4). The minimum only
    // depends on the order when a pool is NaN (`m_min` keeps a NaN unless a later value
    // replaces it), e.g. a 100% MoM with no life left.
    for (damageType, _) in damageTable.pairs() {
        // L402
        MoMPoolRemaining = m_min(MoMPoolRemaining, MoMPoolsRemaining.num(damageType));
        // L403
        esPoolRemaining = m_min(esPoolRemaining, esPoolsRemaining.num(damageType));
    }
    // L405-406
    let hitPoolRemaining = (if life >= 1.0 {
        calcLifeHitPoolWithLossPrevention(
            life,
            output.num(n!("Life")),
            output.num(n!("preventedLifeLoss")),
            lifeLossBelowHalfPrevented,
        )
    } else {
        0.0
    }) + (if MoMPoolRemaining != m_huge {
        MoMPoolRemaining
    } else {
        0.0
    }) + (if esPoolRemaining != m_huge {
        esPoolRemaining
    } else {
        0.0
    });

    // L408-423
    tbl! {
        AlliesTakenBeforeYou = alliesTakenBeforeYou,
        Aegis = aegis,
        Guard = guard,
        Ward = m_floor(if ward < wardBeforeHit {
            if wardAvoidBreakChance > 0.0 { wardBeforeHit } else { 0.0 }
        } else {
            ward
        }),
        WardActiveChance = if ward < wardBeforeHit {
            Val::Num(wardActiveChance.num() * wardAvoidBreakChance)
        } else {
            wardActiveChance
        },
        EnergyShield = m_floor(energyShield),
        Mana = m_floor(mana),
        Life = m_floor(life),
        damageTakenThatCanBeRecouped = damageTakenThatCanBeRecouped,
        LifeLossLostOverTime = m_ceil(LifeLossLostOverTime),
        LifeBelowHalfLossLostOverTime = m_ceil(LifeBelowHalfLossLostOverTime),
        OverkillDamage = m_ceil(overkillDamage),
        hitPoolRemaining = m_floor(hitPoolRemaining),
        resourcesLostToTypeDamage = resourcesLostToTypeDamage
    }
}

/// @pob Modules/CalcDefence.lua:431-478 h=a0f88e59
/// Inserts breakdown of resources drained by a hit
pub(crate) fn incomingDamageBreakdown(
    breakdownTable: &Table,
    poolsRemaining: &Table,
    output: &Output,
) -> Table {
    let rltd = sub_table(
        poolsRemaining,
        n!("resourcesLostToTypeDamage"),
        "resourcesLostToTypeDamage",
    );
    // --region Breakdown inserts
    // L433
    for ally in &allyLifePoolList {
        // L434
        let pool = index_table(
            sub_table(
                poolsRemaining,
                n!("AlliesTakenBeforeYou"),
                "AlliesTakenBeforeYou",
            )
            .get(Name::from(ally.key)),
            "pool",
        );
        // L435
        if let Some(pool) = pool {
            // L436
            let life = output.get(Name::from(ally.life));
            // L437
            breakdownTable.push(lfmt!(
                &format!("\t%d {}{} ^7(%d remaining)", colorCode("GEM"), ally.label),
                life.num() - pool.num(n!("remaining")),
                pool.num(n!("remaining"))
            ));
        }
    }
    // L440
    for damageType in dmgTypeList {
        let dt = Name::from(damageType);
        let rl = sub_table(&rltd, dt, damageType);
        // L441
        if rl.truthy(n!("aegis")) {
            // L442
            breakdownTable.push(lfmt!(
                &format!("\t%d {}%s Aegis charge ^7(%d remaining)", colorCode("GEM")),
                rl.num(n!("aegis")),
                damageType,
                sub_table(poolsRemaining, n!("Aegis"), "Aegis").num(dt)
            ));
        }
    }
    // L445
    let sharedElementalAegis = output.get(n!("sharedElementalAegis"));
    if sharedElementalAegis.truthy()
        && !sharedElementalAegis
            .raw_eq(&sub_table(poolsRemaining, n!("Aegis"), "Aegis").get(n!("sharedElemental")))
    {
        // L446
        let aegis = sub_table(poolsRemaining, n!("Aegis"), "Aegis");
        breakdownTable.push(lfmt!(
            &format!(
                "\t%d {}Elemental Aegis charge ^7(%d remaining)",
                colorCode("GEM")
            ),
            sharedElementalAegis.num() - aegis.num(n!("sharedElemental")),
            aegis.num(n!("sharedElemental"))
        ));
    }
    // L448
    let sharedAegis = output.get(n!("sharedAegis"));
    if sharedAegis.truthy() && cmpnum(&sharedAegis) > 0.0 {
        // L449
        let aegis = sub_table(poolsRemaining, n!("Aegis"), "Aegis");
        breakdownTable.push(lfmt!(
            &format!(
                "\t%d {}Shared Aegis charge ^7(%d remaining)",
                colorCode("GEM")
            ),
            sharedAegis.num() - aegis.num(n!("shared")),
            aegis.num(n!("shared"))
        ));
    }
    // L451
    for damageType in dmgTypeList {
        let dt = Name::from(damageType);
        let rl = sub_table(&rltd, dt, damageType);
        // L452
        if rl.truthy(n!("guard")) {
            // L453
            breakdownTable.push(lfmt!(
                &format!(
                    "\n\t%d {}%s Guard charge ^7(%d remaining)",
                    colorCode("SCOURGE")
                ),
                rl.num(n!("guard")),
                damageType,
                sub_table(poolsRemaining, n!("Guard"), "Guard").num(dt)
            ));
        }
    }
    // L456
    let sharedGuardAbsorb = output.get(n!("sharedGuardAbsorb"));
    if sharedGuardAbsorb.truthy() && cmpnum(&sharedGuardAbsorb) > 0.0 {
        // L457
        let guard = sub_table(poolsRemaining, n!("Guard"), "Guard");
        breakdownTable.push(lfmt!(
            &format!(
                "\t%d {}Shared Guard charge ^7(%d remaining)",
                colorCode("SCOURGE")
            ),
            sharedGuardAbsorb.num() - guard.num(n!("shared")),
            guard.num(n!("shared"))
        ));
    }
    // L459
    let Ward = output.get(n!("Ward"));
    if Ward.truthy() && cmpnum(&Ward) > 0.0 {
        // L460
        breakdownTable.push(lfmt!(
            &format!("\t%d {}Ward", colorCode("WARD")),
            Ward.num()
        ));
    }
    // L462
    let EnergyShieldRecoveryCap = output.get(n!("EnergyShieldRecoveryCap"));
    if !EnergyShieldRecoveryCap.raw_eq(&poolsRemaining.get(n!("EnergyShield")))
        && EnergyShieldRecoveryCap.truthy()
        && cmpnum(&EnergyShieldRecoveryCap) > 0.0
    {
        // L463
        breakdownTable.push(lfmt!(
            &format!("\t%d {}Energy Shield ^7(%d remaining)", colorCode("ES")),
            EnergyShieldRecoveryCap.num() - poolsRemaining.num(n!("EnergyShield")),
            poolsRemaining.num(n!("EnergyShield"))
        ));
    }
    // L465
    let ManaUnreserved = output.get(n!("ManaUnreserved"));
    if !ManaUnreserved.raw_eq(&poolsRemaining.get(n!("Mana")))
        && ManaUnreserved.truthy()
        && cmpnum(&ManaUnreserved) > 0.0
    {
        // L466
        breakdownTable.push(lfmt!(
            &format!("\t%d {}Mana ^7(%d remaining)", colorCode("MANA")),
            ManaUnreserved.num() - poolsRemaining.num(n!("Mana")),
            poolsRemaining.num(n!("Mana"))
        ));
    }
    // L468
    if poolsRemaining.num(n!("LifeLossLostOverTime"))
        + poolsRemaining.num(n!("LifeBelowHalfLossLostOverTime"))
        > 0.0
    {
        // L469
        breakdownTable.push(lfmt!(
            &format!("\t%d {}Life ^7Loss Prevented", colorCode("LIFE")),
            poolsRemaining.num(n!("LifeLossLostOverTime"))
                + poolsRemaining.num(n!("LifeBelowHalfLossLostOverTime"))
        ));
    }
    // L471
    breakdownTable.push(lfmt!(
        &format!("\t%d {}Life ^7(%d remaining)", colorCode("LIFE")),
        output.num(n!("LifeRecoverable")) - poolsRemaining.num(n!("Life")),
        poolsRemaining.num(n!("Life"))
    ));
    // L472
    if cmpnum(&poolsRemaining.get(n!("OverkillDamage"))) >= 1.0 {
        // L473
        breakdownTable.push(lfmt!(
            &format!("\t%d {}Overkill damage", colorCode("NEGATIVE")),
            poolsRemaining.num(n!("OverkillDamage"))
        ));
    }
    // --endregion

    // L477
    breakdownTable.clone()
}

/// Test access to the module-local `incomingDamageBreakdown` (DF-A acceptance,
/// `crates/pob-oracle/tests/DF-A.rs`).
#[doc(hidden)]
pub fn incoming_damage_breakdown_probe(
    breakdownTable: &Table,
    poolsRemaining: &Table,
    output: &Output,
) -> Table {
    incomingDamageBreakdown(breakdownTable, poolsRemaining, output)
}

/// @pob Modules/CalcDefence.lua:481-508 h=d72afd51
/// Performs defensive calculations used by conditionals
#[allow(unused_variables)]
pub fn defenceForConditionals(env: &EnvRef, actor: &ActorRef) {
    // L482
    let modDB = actor.modDB.get();
    // L484
    let output = actor.output.get();

    // -- Armour defence types for conditionals
    // L487
    for slot in [
        "Helmet",
        "Gloves",
        "Boots",
        "Body Armour",
        "Weapon 2",
        "Weapon 3",
    ] {
        // L488
        let item = actor.itemList().with(|m| m.get(&Name::from(slot)).cloned());
        let armourData = item.and_then(|i| i.armourData.get());
        // L489
        if let Some(armourData) = armourData {
            // L490
            let wardBase = land(
                Val::Bool(!modDB.Flag(None, &[cat!("GainNoWardFrom", slot)])),
                || armourData.get(n!("Ward")),
            );
            let wardBase = lor(wardBase, || Val::Num(0.0));
            // L491
            if cmpnum(&wardBase) > 0.0 {
                // L492
                output.set(cat!("WardOn", slot), wardBase);
            }
            // L494
            let energyShieldBase = lor(
                land(
                    Val::Bool(!modDB.Flag(None, &[cat!("GainNoEnergyShieldFrom", slot)])),
                    || armourData.get(n!("EnergyShield")),
                ),
                || Val::Num(0.0),
            );
            // L495
            if cmpnum(&energyShieldBase) > 0.0 {
                // L496
                output.set(cat!("EnergyShieldOn", slot), energyShieldBase);
            }
            // L498
            let armourBase = lor(
                land(
                    Val::Bool(!modDB.Flag(None, &[cat!("GainNoArmourFrom", slot)])),
                    || armourData.get(n!("Armour")),
                ),
                || Val::Num(0.0),
            );
            // L499
            if cmpnum(&armourBase) > 0.0 {
                // L500
                output.set(cat!("ArmourOn", slot), armourBase);
            }
            // L502
            let evasionBase = lor(
                land(
                    Val::Bool(!modDB.Flag(None, &[cat!("GainNoEvasionFrom", slot)])),
                    || armourData.get(n!("Evasion")),
                ),
                || Val::Num(0.0),
            );
            // L503
            if cmpnum(&evasionBase) > 0.0 {
                // L504
                output.set(cat!("EvasionOn", slot), evasionBase);
            }
        }
    }
}

/// `modDB:NewMod(name, type, value, source)` where `source` may be nil (a mod's own
/// `mod.source`; ModStore.lua:109-111 → `modLib.createMod`).
fn new_mod_src(modDB: &ModDB, name: Name, ty: ModType, value: f64, source: Option<&str>) {
    match source {
        Some(s) => modDB.NewMod(name, ty, value, s, 0, 0, &[]),
        None => modDB.AddMod(Arc::new(mod_tools::createMod(
            name,
            ty,
            value,
            None,
            0,
            0,
            &[],
        ))),
    }
}

/// @pob Modules/CalcDefence.lua:512-648 h=ef34d9c5
/// Performs resistance calculations
pub fn resistances(actor: &ActorRef) {
    // L513
    let modDB = actor.modDB.get();
    // L515
    let output = actor.output.get();
    // L525
    let breakdown = actor.breakdown.get();
    // L526
    output.set(n!("PhysicalResist"), 0.0);

    // -- Process Resistance conversion mods
    // L529
    for resFrom in resistTypeList {
        // L530
        let mut maxRes: Option<f64> = None;
        // L531
        for resTo in resistTypeList {
            // L532
            let conversionRate = modDB.Sum(
                ModType::BASE,
                None,
                &[cat!(resFrom, "MaxResConvertTo", resTo)],
            ) / 100.0;
            // L533
            if conversionRate != 0.0 {
                // L534
                if maxRes.is_none() {
                    // L535
                    let mut m = 0.0;
                    // L536
                    for r#mod in
                        modDB.Tabulate(Some(ModType::BASE), None, &[cat!(resFrom, "ResistMax")])
                    {
                        // L537
                        if r#mod.r#mod.source.as_deref() != Some("Base") {
                            // L538
                            m = m + vnum(&r#mod.value);
                        }
                    }
                    maxRes = Some(m);
                }
                let mr = maxRes.unwrap_or_default();
                // L542
                if mr != 0.0 {
                    // L543
                    modDB.NewMod(
                        cat!(resTo, "ResistMax"),
                        ModType::BASE,
                        mr * conversionRate,
                        &format!("{resFrom} To {resTo} Max Resistance Conversion"),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
    }

    // L549
    for resFrom in resistTypeList {
        // L550
        let mut res: Option<f64> = None;
        // L551
        for resTo in resistTypeList {
            // L552
            let conversionRate =
                modDB.Sum(ModType::BASE, None, &[cat!(resFrom, "ResConvertTo", resTo)]) / 100.0;
            // L553
            if conversionRate != 0.0 {
                // L554
                if res.is_none() {
                    // L555
                    let mut r = 0.0;
                    // L556
                    for r#mod in
                        modDB.Tabulate(Some(ModType::BASE), None, &[cat!(resFrom, "Resist")])
                    {
                        // L557
                        if r#mod.r#mod.source.as_deref() != Some("Base") {
                            // L558
                            r = r + vnum(&r#mod.value);
                        }
                    }
                    res = Some(r);
                }
                let rs = res.unwrap_or_default();
                // L562
                if rs != 0.0 {
                    // L563
                    modDB.NewMod(
                        cat!(resTo, "Resist"),
                        ModType::BASE,
                        rs * conversionRate,
                        &format!("{resFrom} To {resTo} Resistance Conversion"),
                        0,
                        0,
                        &[],
                    );
                }
                // L565
                for r#mod in modDB.Tabulate(Some(ModType::INC), None, &[cat!(resFrom, "Resist")]) {
                    // L566
                    new_mod_src(
                        &modDB,
                        cat!(resTo, "Resist"),
                        ModType::INC,
                        vnum(&r#mod.value) * conversionRate,
                        r#mod.r#mod.source.as_deref(),
                    );
                }
                // L568
                for r#mod in modDB.Tabulate(Some(ModType::MORE), None, &[cat!(resFrom, "Resist")]) {
                    // L569
                    new_mod_src(
                        &modDB,
                        cat!(resTo, "Resist"),
                        ModType::MORE,
                        vnum(&r#mod.value) * conversionRate,
                        r#mod.r#mod.source.as_deref(),
                    );
                }
            }
        }
    }

    // -- Highest Maximum Elemental Resistance for Melding of the Flesh
    // L576
    if modDB.Flag(None, nm!["ElementalResistMaxIsHighestResistMax"]) {
        // L577
        let mut highestResistMax = 0.0;
        // L578
        let mut highestResistMaxType = "";
        // L579
        for elem in resistTypeList {
            // L580
            let resistMax = override_or(modDB.Override(None, &[cat!(elem, "ResistMax")]), || {
                m_min(
                    data().misc.MaxResistCap,
                    modDB.Sum(
                        ModType::BASE,
                        None,
                        &names([
                            Some(cat!(elem, "ResistMax")),
                            isElemental(elem).then(|| n!("ElementalResistMax")),
                        ]),
                    ),
                )
            });
            // L581
            if resistMax > highestResistMax && isElemental(elem) {
                // L582
                highestResistMax = resistMax;
                // L583
                highestResistMaxType = elem;
            }
        }
        // L586
        for elem in resistTypeList {
            // L587
            if isElemental(elem) {
                // L588
                modDB.NewMod(
                    cat!(elem, "ResistMax"),
                    ModType::OVERRIDE,
                    highestResistMax,
                    &format!("{highestResistMaxType} Melding of the Flesh"),
                    0,
                    0,
                    &[],
                );
            }
        }
    }

    // L593
    for elem in resistTypeList {
        let el = isElemental(elem);
        // L594
        let mut min: f64;
        let mut max: f64;
        let mut total: Option<f64>;
        let mut dotTotal: Option<f64> = None;
        let mut totemTotal: Option<f64>;
        let mut totemMax: f64;
        // L595
        min = data().misc.ResistFloor;
        // L596
        max = override_or(modDB.Override(None, &[cat!(elem, "ResistMax")]), || {
            m_min(
                data().misc.MaxResistCap,
                modDB.Sum(
                    ModType::BASE,
                    None,
                    &names([
                        Some(cat!(elem, "ResistMax")),
                        el.then(|| n!("ElementalResistMax")),
                    ]),
                ),
            )
        });
        // L597
        total = {
            let v = modDB.Override(None, &[cat!(elem, "Resist")]);
            v.truthy().then(|| vnum(&v))
        };
        // L598
        totemMax = override_or(
            modDB.Override(None, &[cat!("Totem", elem, "ResistMax")]),
            || {
                m_min(
                    data().misc.MaxResistCap,
                    modDB.Sum(
                        ModType::BASE,
                        None,
                        &names([
                            Some(cat!("Totem", elem, "ResistMax")),
                            el.then(|| n!("TotemElementalResistMax")),
                        ]),
                    ),
                )
            },
        );
        // L599
        totemTotal = {
            let v = modDB.Override(None, &[cat!("Totem", elem, "Resist")]);
            v.truthy().then(|| vnum(&v))
        };
        // L600
        if total.is_none() {
            let resNames = names([
                Some(cat!(elem, "Resist")),
                el.then(|| n!("ElementalResist")),
            ]);
            // L601
            let base = modDB.Sum(ModType::BASE, None, &resNames);
            // L602
            let inc = m_max(calcLib::r#mod(&modDB, None, &resNames), 0.0);
            // L603
            total = Some(base * inc);
            // L604
            let dotCfg = ModCfg::with_flags(ModFlag::Dot, 0);
            let dotBase = modDB.Sum(ModType::BASE, Some(&*dotCfg), &resNames);
            // L605
            dotTotal = Some(dotBase * inc);
        }
        // L607
        if totemTotal.is_none() {
            let totemNames = names([
                Some(cat!("Totem", elem, "Resist")),
                el.then(|| n!("TotemElementalResist")),
            ]);
            // L608
            let base = modDB.Sum(ModType::BASE, None, &totemNames);
            // L609
            totemTotal = Some(base * m_max(calcLib::r#mod(&modDB, None, &totemNames), 0.0));
        }

        // -- Fractional resistances are truncated
        // L613
        let total = m_modf(total.unwrap_or_default()).0;
        // L614
        let dotTotal = match dotTotal {
            Some(d) => m_modf(d).0,
            None => total,
        };
        // L615
        let totemTotal = m_modf(totemTotal.unwrap_or_default()).0;
        // L616
        min = m_modf(min).0;
        // L617
        max = m_modf(max).0;
        // L618
        totemMax = m_modf(totemMax).0;

        // L620
        let r#final = m_max(m_min(total, max), min);
        // L621
        let dotFinal = m_max(m_min(dotTotal, max), min);
        // L622
        let totemFinal = m_max(m_min(totemTotal, totemMax), min);

        // L625
        output.set(cat!(elem, "Resist"), r#final);
        // L626
        output.set(cat!(elem, "ResistTotal"), total);
        // L627
        output.set(cat!(elem, "ResistOverCap"), m_max(0.0, total - max));
        // L628
        output.set(cat!(elem, "ResistOver75"), m_max(0.0, r#final - 75.0));
        // L629
        output.set(cat!("Missing", elem, "Resist"), m_max(0.0, max - r#final));
        // L630
        output.set(cat!(elem, "ResistOverTime"), dotFinal);
        // L631
        output.set(cat!("Totem", elem, "Resist"), totemFinal);
        // L632
        output.set(cat!("Totem", elem, "ResistTotal"), totemTotal);
        // L633
        output.set(
            cat!("Totem", elem, "ResistOverCap"),
            m_max(0.0, totemTotal - totemMax),
        );
        // L634
        output.set(
            cat!("MissingTotem", elem, "Resist"),
            m_max(0.0, totemMax - totemFinal),
        );
        // L635
        if let Some(breakdown) = &breakdown {
            // L636-640
            breakdown.set(
                cat!(elem, "Resist"),
                tbl![
                    format!("Min: {}%", tostring_num(min)),
                    format!("Max: {}%", tostring_num(max)),
                    format!("Total: {}%", tostring_num(total))
                ],
            );
            // L641-645
            breakdown.set(
                cat!("Totem", elem, "Resist"),
                tbl![
                    format!("Min: {}%", tostring_num(min)),
                    format!("Max: {}%", tostring_num(totemMax)),
                    format!("Total: {}%", tostring_num(totemTotal))
                ],
            );
        }
    }
}
