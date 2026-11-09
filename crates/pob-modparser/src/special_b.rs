//! @pob-file Modules/ModParser.lua @16de4b82d
//! specialModList entries, ModParser.lua 3084-4036 (calc doc §8; provenance per block of about 50
//! entries). `special_a`..`special_d` concatenate, in source order, into `specialModList`.
//!
//! Owner: MP4. The entries were translated by scripts/port/lua2rs (unit T1) and reviewed
//! against the Lua; the three closures lua2rs could not translate (3223, 3272, 3713) are
//! hand-ported (marked "hand-ported").
//! One `PatternEntry` per Lua key, in source order, with the line that defines it.
//! Closure parameters follow the specialModList call shape (ModParser.lua:6662-6663; calc doc
//! §6 / CCR-0006 §5): `f(tonumber(cap[1]), unpack(cap))`, so #1 = `caps.num` and
//! #k≥2 = `caps.cap[k-2]`.
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    unused_mut,
    unused_parens,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::helpers::*;
use crate::types::*;
use pob_gamedata::{
    data, tag, AStr, KeywordFlag, Mod, ModFlag, ModType, ModType::*, Name, SkillType, Sockets, Tag,
    Value, ValueTable,
};
use pob_lua::bit::bor;
use pob_lua::pattern::{gsub, gsub_fn};
use pob_lua::string::{firstToUpper, len, lower, sub, upper};
use pob_lua::{cat, lua_error, n, tonumber};
use std::sync::{Arc, LazyLock};

/// Lua arithmetic/concatenation on a possibly-nil number: nil raises (calc doc §3.2).
#[inline]
fn nn(v: Option<f64>, what: &str) -> f64 {
    match v {
        Some(x) => x,
        None => lua_error!("attempt to {} (a nil value)", what),
    }
}

/// A string capture/local that Lua would index or concatenate: nil raises (calc doc §3.2).
#[inline]
fn ns<'a>(v: Option<&'a str>, what: &str) -> &'a str {
    match v {
        Some(x) => x,
        None => lua_error!("attempt to {} (a nil value)", what),
    }
}

/// Lua arithmetic on a string operand: coerced like tonumber, else raises (lj_strscan).
#[inline]
fn snum(s: &str, what: &str) -> f64 {
    match tonumber(s) {
        Some(x) => x,
        None => lua_error!("attempt to {} (a string value)", what),
    }
}

/// A Lua table constructor used as a mod value: nil fields are absent, trailing nils are not
/// part of the array, and string keys are kept in dump order (bytewise; pob-dump FORMAT.md).
fn vtab(mut array: Vec<Value>, fields: Vec<(Name, Value)>) -> Value {
    while matches!(array.last(), Some(Value::Nil)) {
        array.pop();
    }
    let mut fields: Vec<(Name, Value)> = fields
        .into_iter()
        .filter(|(_, v)| !matches!(v, Value::Nil))
        .collect();
    fields.sort_by(|a, b| a.0.as_str().as_bytes().cmp(b.0.as_str().as_bytes()));
    Value::Table(Arc::new(ValueTable { array, fields }))
}

/// @pob Modules/ModParser.lua:3084-3186 h=4afc3260
fn specialModList_3084(v: &mut Vec<PatternEntry<SpecialVal>>) {
    // maim effect increases and decreases should affect this, but none exist yet
    v.push(PatternEntry { pat: "while you have at least (%d+) searching eye jewels socketed: targets affected by maim you inflict cannot deal critical strikes maim you inflict causes hits against the target to have (%d+)%% more critical strike chance", val: SpecialVal::Fn(|caps: &Caps| {
        let jewels = caps.num;
        let crit = ns(caps.cap[1], "index local 'crit'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("NeverCrit"), None, 0, 0, &[tag!({ type = Condition, var = "Maimed" })])))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SearchingEyeJewel", threshold = jewels })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("SelfCritChance"), MORE, tonumber(crit), None, 0, 0, &[tag!({ type = Condition, var = "Maimed" })])))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SearchingEyeJewel", threshold = jewels })])])
    }), lua_line: 3084 });
    // intimidate effect increases and decreases should affect this, but none exist yet
    // LUA-QUIRK: modLib.createMod skips the number in the source slot, so the next number is the flags and the one after it the keywordFlags
    v.push(PatternEntry { pat: "while you have at least (%d+) murderous eye jewels socketed: intimidate you inflict applies an extra %+(%d+)%% increased attack damage taken intimidate you inflict causes targets to deal (%d+)%% less damage", val: SpecialVal::Fn(|caps: &Caps| {
        let jewels = caps.num;
        let attackDam = ns(caps.cap[1], "index local 'attackDam'");
        let lessDam = ns(caps.cap[2], "index local 'lessDam'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, tonumber(attackDam), None, ModFlag::Attack, 0, &[tag!({ type = Condition, var = "Intimidated" })])))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "MurderousEyeJewel", threshold = jewels })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), MORE, (-nn(tonumber(lessDam), "perform arithmetic on a nil value")), None, 0, 0, &[tag!({ type = Condition, var = "Intimidated" })])))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "MurderousEyeJewel", threshold = jewels })])])
    }), lua_line: 3091 });
    // Aul Bloodline
    v.push(PatternEntry { pat: "action speed cannot be modified to below base value if you have equipped boots with no socketed gems", val: SpecialVal::Mods(vec![r#mod(n!("MinimumActionSpeed"), MAX, 100.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }), tag!({ type = MultiplierThreshold, var = "SocketedGemsInBoots", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingBoots" })])]), lua_line: 3103 });
    v.push(PatternEntry { pat: "cannot be stunned if you have an equipped helmet with no socketed gems", val: SpecialVal::Mods(vec![flag(n!("StunImmune"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SocketedGemsInHelmet", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingHelmet" })])]), lua_line: 3104 });
    v.push(PatternEntry { pat: "elemental ailments cannot be inflicted on you if you have an equipped body armour with no socketed gems", val: SpecialVal::Mods(vec![flag(n!("ElementalAilmentImmune"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SocketedGemsInBody Armour", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingBody Armour" })])]), lua_line: 3105 });
    v.push(PatternEntry { pat: "take no extra damage from critical strikes if you have equipped gloves with no socketed gems", val: SpecialVal::Mods(vec![r#mod(n!("ReduceCritExtraDamage"), BASE, 100.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }), tag!({ type = MultiplierThreshold, var = "SocketedGemsInGloves", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingGloves" })])]), lua_line: 3106 });
    // Delirious Bloodline
    v.push(PatternEntry {
        pat: "while affected by glorious madness, inflict mania on nearby enemies every second",
        val: SpecialVal::Mods(vec![
            flag(
                n!("Condition:CanInflictMania"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:AfflictedByMania"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
        ]),
        lua_line: 3108,
    });
    // Lycia Bloodline
    v.push(PatternEntry { pat: "herald skills have (%d+)%% more buff effect for every (%d+)%% of maximum mana they reserve", val: SpecialVal::Fn(|caps: &Caps| {
        let num1 = ns(caps.cap[0], "index local 'num1'");
        let num2 = ns(caps.cap[1], "index local 'num2'");
        Some(vec![r#mod(n!("BuffEffect"), MORE, tonumber(num1), None, 0, 0, &[tag!({ type = PerStat, stat = "ManaReservedPercent", div = tonumber(num2) }), tag!({ type = SkillType, skillType = SkillType::Herald })])])
    }), lua_line: 3113 });
    v.push(PatternEntry { pat: "herald skills and minions from herald skills deal (%d+)%% more damage for every (%d+)%% of maximum life those skills reserve", val: SpecialVal::Fn(|caps: &Caps| {
        let num1 = ns(caps.cap[0], "index local 'num1'");
        let num2 = ns(caps.cap[1], "index local 'num2'");
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), MORE, tonumber(num1), None, 0, 0, &[tag!({ type = PerStat, stat = "LifeReservedPercent", div = tonumber(num2), actor = "parent" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Herald })]), r#mod(n!("Damage"), MORE, tonumber(num1), None, 0, 0, &[tag!({ type = PerStat, stat = "LifeReservedPercent", div = tonumber(num2) }), tag!({ type = SkillType, skillType = SkillType::Herald })])])
    }), lua_line: 3114 });
    // Oshabi Bloodline
    v.push(PatternEntry {
        pat: "unsealed spells gain (%d+)%% more damage each time their effects reoccur",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("MaxSealDamage"), MORE, num, None, 0, 0, &[])])
        }),
        lua_line: 3119,
    });
    v.push(PatternEntry { pat: "skills gain added chaos damage equal to (%d+)%% of life cost, if life cost is not higher than the maximum you could spend", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ChaosMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "LifeCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "LifeUnreserved", thresholdStat = "LifeCost" })]), r#mod(n!("ChaosMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "LifeCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "LifeUnreserved", thresholdStat = "LifeCost" })])])
    }), lua_line: 3120 });
    v.push(PatternEntry { pat: "lose all rage on reaching maximum rage and gain wild savagery for 1 second per 10 rage lost this way", val: SpecialVal::Mods(vec![flag(n!("WildSavagery"), None, 0, 0, &[])]), lua_line: 3124 });
    // Velka Bloodline
    v.push(PatternEntry {
        pat: "inflict barnacles on nearby enemies every second",
        val: SpecialVal::Mods(vec![flag(n!("CanInflictBarnacles"), None, 0, 0, &[])]),
        lua_line: 3126,
    });
    v.push(PatternEntry {
        pat: "drop brine ground while moving, lasting 4 seconds",
        val: SpecialVal::Mods(vec![flag(n!("CanCreateBrineGround"), None, 0, 0, &[])]),
        lua_line: 3127,
    });
    // Item local modifiers
    v.push(PatternEntry {
        pat: "has no sockets",
        val: SpecialVal::Mods(vec![flag(n!("NoSockets"), None, 0, 0, &[])]),
        lua_line: 3129,
    });
    v.push(PatternEntry {
        pat: "gems socketed always have the quality bonus from socket colour",
        val: SpecialVal::Mods(vec![flag(n!("SocketAlwaysMatches"), None, 0, 0, &[])]),
        lua_line: 3130,
    });
    v.push(PatternEntry {
        pat: "cannot have non%-abyssal sockets",
        val: SpecialVal::Mods(vec![flag(n!("NoSockets"), None, 0, 0, &[])]),
        lua_line: 3131,
    });
    // Display only Subsume The Source sockets removed by "NoSockets"
    v.push(PatternEntry {
        pat: "socketed %a+ abyssal jewels will be consumed",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3132,
    });
    // Display only
    v.push(PatternEntry {
        pat: "one modifier from consumed jewels will be retained",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3135,
    });
    // Display only. For Kalandra's Touch.
    v.push(PatternEntry {
        pat: "reflects your o[tp][hp][eo][rs]i?t?e? ring",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3138,
    });
    // Display only. For Dread Captain's Cutlass and supported by crafting UI
    v.push(PatternEntry {
        pat: "can be allflame crafted as if rare",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3141,
    });
    // Display only. For Dread Captain's Cutlass
    v.push(PatternEntry {
        pat: "cannot gain intangibility",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3144,
    });
    v.push(PatternEntry {
        pat: "has (%d+) sockets?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("SocketCount"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 3147,
    });
    v.push(PatternEntry {
        pat: "has (%d+) abyssal sockets?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AbyssalSocketCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3148,
    });
    v.push(PatternEntry {
        pat: "no physical damage",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("WeaponData"),
                LIST,
                vtab(vec![], vec![(n!("key"), Value::from("PhysicalMin"))]),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("WeaponData"),
                LIST,
                vtab(vec![], vec![(n!("key"), Value::from("PhysicalMax"))]),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("WeaponData"),
                LIST,
                vtab(vec![], vec![(n!("key"), Value::from("PhysicalDPS"))]),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 3149,
    });
    v.push(PatternEntry {
        pat: "has (%d+)%% increased elemental damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LocalElementalDamage"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3150,
    });
    v.push(PatternEntry {
        pat: "all attacks with this weapon are critical strikes",
        val: SpecialVal::Mods(vec![r#mod(
            n!("WeaponData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("CritChance")),
                    (n!("value"), Value::from(100.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3151,
    });
    v.push(PatternEntry {
        pat: "this weapon's critical strike chance is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("WeaponData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("CritChance")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3152,
    });
    v.push(PatternEntry {
        pat: "counts as dual wielding",
        val: SpecialVal::Mods(vec![r#mod(
            n!("WeaponData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("countsAsDualWielding")),
                    (n!("value"), Value::from(true)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3153,
    });
    v.push(PatternEntry {
        pat: "counts as all one handed melee weapon types",
        val: SpecialVal::Mods(vec![r#mod(
            n!("WeaponData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("countsAsAll1H")),
                    (n!("value"), Value::from(true)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3154,
    });
    v.push(PatternEntry {
        pat: "no block chance",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ArmourData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("BlockChance")),
                    (n!("value"), Value::from(0.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3155,
    });
    v.push(PatternEntry {
        pat: "no chance to block",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ArmourData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("BlockChance")),
                    (n!("value"), Value::from(0.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3156,
    });
    v.push(PatternEntry {
        pat: "has no energy shield",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ArmourData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("EnergyShield")),
                    (n!("value"), Value::from(0.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3157,
    });
    v.push(PatternEntry {
        pat: "hits can't be evaded",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotBeEvaded"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3158,
    });
    v.push(PatternEntry {
        pat: "causes bleeding on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3159,
    });
    v.push(PatternEntry {
        pat: "poisonous hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3160,
    });
    v.push(PatternEntry {
        pat: "attacks with this weapon deal double damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("DoubleDamageChance"),
            BASE,
            100.0,
            None,
            ModFlag::Hit,
            0,
            &[
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
        )]),
        lua_line: 3161,
    });
    v.push(PatternEntry { pat: "hits with this weapon gain (%d+)%% of physical damage as extra cold or lightning damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PhysicalDamageGainAsColdOrLightning"), BASE, num, None, ModFlag::Hit, 0, &[tag!({ type = Condition, var = "{Hand}Attack" }), tag!({ type = SkillType, skillType = SkillType::Attack })])])
    }), lua_line: 3162 });
    v.push(PatternEntry {
        pat: "hits with this weapon shock enemies as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ShockAsThoughDealing"),
                MORE,
                num,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 3165,
    });
    v.push(PatternEntry {
        pat: "hits with this weapon freeze enemies as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FreezeAsThoughDealing"),
                MORE,
                num,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 3166,
    });
    v.push(PatternEntry {
        pat: "ignites inflicted with this weapon deal (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                0,
                KeywordFlag::Ignite,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 3167,
    });
    v.push(PatternEntry {
        pat: "hits with this weapon always ignite, freeze, and shock",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyIgniteChance"),
                BASE,
                100.0,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            ),
            r#mod(
                n!("EnemyFreezeChance"),
                BASE,
                100.0,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            ),
            r#mod(
                n!("EnemyShockChance"),
                BASE,
                100.0,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            ),
        ]),
        lua_line: 3170,
    });
    v.push(PatternEntry {
        pat: "attacks with this weapon deal double damage to chilled enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("DoubleDamageChance"),
            BASE,
            100.0,
            None,
            ModFlag::Hit,
            0,
            &[
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" }),
            ],
        )]),
        lua_line: 3175,
    });
    v.push(PatternEntry {
        pat: "life leech from hits with this weapon applies instantly",
        val: SpecialVal::Mods(vec![r#mod(
            n!("InstantLifeLeech"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3176,
    });
    v.push(PatternEntry {
        pat: "life leech from hits with this weapon is instant",
        val: SpecialVal::Mods(vec![r#mod(
            n!("InstantLifeLeech"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3177,
    });
    v.push(PatternEntry {
        pat: "mana leech from hits with this weapon is instant",
        val: SpecialVal::Mods(vec![r#mod(
            n!("InstantManaLeech"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3178,
    });
    v.push(PatternEntry {
        pat: "life leech from melee damage is instant",
        val: SpecialVal::Mods(vec![r#mod(
            n!("InstantLifeLeech"),
            BASE,
            100.0,
            None,
            ModFlag::Melee,
            0,
            &[],
        )]),
        lua_line: 3179,
    });
    v.push(PatternEntry {
        pat: "gain life from leech instantly from hits with this weapon",
        val: SpecialVal::Mods(vec![r#mod(
            n!("InstantLifeLeech"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
        )]),
        lua_line: 3180,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of leech from hits with this weapon is instant per enemy power",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("InstantLifeLeech"),
                    BASE,
                    num,
                    None,
                    ModFlag::Hit,
                    0,
                    &[
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                        tag!({ type = Multiplier, var = "EnemyPower" }),
                    ],
                ),
                r#mod(
                    n!("InstantManaLeech"),
                    BASE,
                    num,
                    None,
                    ModFlag::Hit,
                    0,
                    &[
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                        tag!({ type = Multiplier, var = "EnemyPower" }),
                    ],
                ),
                r#mod(
                    n!("InstantEnergyShieldLeech"),
                    BASE,
                    num,
                    None,
                    ModFlag::Hit,
                    0,
                    &[
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                        tag!({ type = Multiplier, var = "EnemyPower" }),
                    ],
                ),
            ])
        }),
        lua_line: 3181,
    });
    v.push(PatternEntry {
        pat: "instant recovery",
        val: SpecialVal::Mods(vec![r#mod(
            n!("FlaskInstantRecovery"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3186,
    });
}

/// @pob Modules/ModParser.lua:3187-3262 h=faa75198
fn specialModList_3187(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "(%d+)%% of recovery applied instantly",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskInstantRecovery"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3187,
    });
    v.push(PatternEntry {
        pat: "instant recovery when on low life",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("FlaskLowLifeInstantRecovery"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("Dummy"),
                DUMMY,
                1.0,
                Some(""),
                0,
                0,
                &[tag!({ type = Condition, var = "LowLife" })],
            ),
        ]),
        lua_line: 3188,
    });
    v.push(PatternEntry {
        pat: "life flasks used while on low life apply recovery instantly",
        val: SpecialVal::Mods(vec![r#mod(
            n!("LifeFlaskInstantRecovery"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife" })],
        )]),
        lua_line: 3189,
    });
    v.push(PatternEntry {
        pat: "mana flasks used while on low mana apply recovery instantly",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ManaFlaskInstantRecovery"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowMana" })],
        )]),
        lua_line: 3190,
    });
    v.push(PatternEntry {
        pat: "has no attribute requirements",
        val: SpecialVal::Mods(vec![flag(n!("NoAttributeRequirements"), None, 0, 0, &[])]),
        lua_line: 3191,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed spell when you attack with this weapon",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellOnAttack")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3192,
    });
    v.push(PatternEntry { pat: "trigger a socketed spell when you attack with this weapon, with a ([%d%.]+) second cooldown", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportTriggerSpellOnAttack")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 3193 });
    v.push(PatternEntry {
        pat: "trigger a socketed spell when you use a skill",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellOnSkillUse")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3194,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed spell when you use a skill, with a (%d+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellOnSkillUse")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3195,
    });
    v.push(PatternEntry { pat: "trigger a socketed spell when you use a skill, with a (%d+) second cooldown and (%d+)%% more cost", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportTriggerSpellOnSkillUse")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 3196 });
    v.push(PatternEntry {
        pat: "trigger socketed spells when you focus",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellFromHelmet")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3197,
    });
    v.push(PatternEntry {
        pat: "trigger socketed spells when you focus, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellFromHelmet")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3198,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed spell when you attack with a bow",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellOnBowAttack")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3199,
    });
    v.push(PatternEntry {
        pat:
            "trigger a socketed spell when you attack with a bow, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellOnBowAttack")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3200,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed bow skill when you attack with a bow",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportTriggerBowSkillOnBowAttack"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3201,
    });
    v.push(PatternEntry { pat: "trigger a socketed bow skill when you attack with a bow, with a ([%d%.]+) second cooldown", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportTriggerBowSkillOnBowAttack")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 3202 });
    v.push(PatternEntry {
        pat: "trigger a socketed bow skill when you cast a spell while wielding a bow",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportTriggerBowSkillOnBowAttack"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3203,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to trigger socketed spell on kill, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerSpellOnKill")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3204,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to [c?t?][a?r?][s?i?][t?g?]g?e?r? socketed spells when you spend at least (%d+) mana to use a skill", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        Some(vec![r#mod(n!("KitavaTriggerChance"), BASE, num, Some("Kitava's Thirst"), 0, 0, &[]), r#mod(n!("KitavaRequiredManaCost"), BASE, tonumber(amount), Some("Kitava's Thirst"), 0, 0, &[]), r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportCastOnManaSpent")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])])
    }), lua_line: 3205 });
    v.push(PatternEntry { pat: "(%d+)%% chance to [c?t?][a?r?][s?i?][t?g?]g?e?r? socketed spells when you spend at least (%d+) mana on an upfront cost to use or trigger a skill, with a ([%d%.]+) second cooldown", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        Some(vec![r#mod(n!("KitavaTriggerChance"), BASE, num, Some("Kitava's Thirst"), 0, 0, &[]), r#mod(n!("KitavaRequiredManaCost"), BASE, tonumber(amount), Some("Kitava's Thirst"), 0, 0, &[]), r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportCastOnManaSpent")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])])
    }), lua_line: 3210 });
    v.push(PatternEntry { pat: "(%d+)%% chance to [c?t?][a?r?][s?i?][t?g?]g?e?r? socketed spells when you spend at least (%d+) life on an upfront cost to use or trigger a skill, with a ([%d%.]+) second cooldown", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        Some(vec![r#mod(n!("FoulbornKitavaTriggerChance"), BASE, num, Some("Kitava's Thirst"), 0, 0, &[]), r#mod(n!("FoulbornKitavaRequiredLifeCost"), BASE, tonumber(amount), Some("Kitava's Thirst"), 0, 0, &[]), r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportCastOnLifeSpent")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])])
    }), lua_line: 3215 });
    v.push(PatternEntry {
        pat: "trigger a socketed fire spell on hit, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("skillId"), Value::from("SupportTriggerFireSpellOnHit")),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3220,
    });
    // Socketed gem modifiers
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to level of socketed gems",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("all")),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            )])
        }),
        lua_line: 3222,
    });
    // hand-ported (lua2rs left it untranslated: assign-type: str); the Lua:
    // |     ["([%+%-]%d+)%%? to (%a+) of socketed ?([%a%- ]*) gems"] = function(num, _, property, type)
    // |         if type == "" then type = "all" end
    // |         return { mod("GemProperty", "LIST", { keyword = type, key = property, value = num }, { type = "SocketedIn", slotName = "{SlotName}" }) }
    // |     end,
    v.push(PatternEntry {
        pat: "([%+%-]%d+)%%? to (%a+) of socketed ?([%a%- ]*) gems",
        val: SpecialVal::Fn(|caps: &Caps| {
            // hand-ported
            let num = caps.num;
            let property = caps.cap[1];
            let mut r#type = caps.cap[2];
            // L3224
            if r#type == Some("") {
                r#type = Some("all");
            }
            // L3225
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from(r#type.map(Name::from))),
                        (n!("key"), Value::from(property.map(Name::from))),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            )])
        }),
        lua_line: 3223,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to level of socketed skill gems per socketed gem",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("grants_active_skill")),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                        (n!("keyOfScaledMod"), Value::from("value")),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = Multiplier, var = "SocketedGemsIn{SlotName}" }),
                ],
            )])
        }),
        lua_line: 3227,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+)%% to quality of all skill gems",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("grants_active_skill")),
                        (n!("key"), Value::from("quality")),
                        (n!("value"), Value::from(num)),
                        (n!("keyOfScaledMod"), Value::from("value")),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3228,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to level of all elemental skill gems if the stars are aligned",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (
                            n!("keywordList"),
                            Value::from(vtab(
                                vec![Value::from("elemental"), Value::from("grants_active_skill")],
                                vec![],
                            )),
                        ),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                        (n!("keyOfScaledMod"), Value::from("value")),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            )])
        }),
        lua_line: 3229,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to level of all elemental support gems if the stars are aligned",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (
                            n!("keywordList"),
                            Value::from(vtab(
                                vec![Value::from("elemental"), Value::from("support")],
                                vec![],
                            )),
                        ),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                        (n!("keyOfScaledMod"), Value::from("value")),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            )])
        }),
        lua_line: 3230,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to level of socketed active skill gems per (%d+) player levels",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("grants_active_skill")),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = Multiplier, var = "Level", div = tonumber(div) }),
                ],
            )])
        }),
        lua_line: 3231,
    });
    v.push(PatternEntry { pat: "([%+%-]%d+) to level of all (%a+) skill gems if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let element = ns(caps.cap[1], "index local 'element'");
        let thresh = ns(caps.cap[2], "index local 'thresh'");
        let influence = ns(caps.cap[3], "index local 'influence'");
        Some(vec![r#mod(n!("GemProperty"), LIST, vtab(vec![], vec![(n!("keyword"), Value::from(&*element)), (n!("key"), Value::from("level")), (n!("value"), Value::from(num)), (n!("keyOfScaledMod"), Value::from("value"))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 3232 });
    v.push(PatternEntry { pat: "([%+%-]%d+) to level of all (%a+) skill gems if a?t? ?l?e?a?s?t? ?(%d+) (%a+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let element = ns(caps.cap[1], "index local 'element'");
        let thresh = ns(caps.cap[2], "index local 'thresh'");
        let influence = ns(caps.cap[3], "index local 'influence'");
        let influence2 = ns(caps.cap[4], "index local 'influence2'");
        Some(vec![r#mod(n!("GemProperty"), LIST, vtab(vec![], vec![(n!("keyword"), Value::from(&*element)), (n!("key"), Value::from("level")), (n!("value"), Value::from(num)), (n!("keyOfScaledMod"), Value::from("value"))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), firstToUpper(influence2), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 3233 });
    v.push(PatternEntry { pat: "([%+%-]%d+) to level of all ?([%a%- ]*) support gems if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let element = ns(caps.cap[1], "index local 'element'");
        let thresh = ns(caps.cap[2], "index local 'thresh'");
        let influence = ns(caps.cap[3], "index local 'influence'");
        Some(vec![r#mod(n!("GemProperty"), LIST, vtab(vec![], vec![(n!("keyword"), Value::from(&*element)), (n!("key"), Value::from("level")), (n!("value"), Value::from(num)), (n!("keyOfScaledMod"), Value::from("value"))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 3234 });
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to level of socketed skill gems per (%d+) player levels",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("grants_active_skill")),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = Multiplier, var = "Level", div = tonumber(div) }),
                ],
            )])
        }),
        lua_line: 3235,
    });
    v.push(PatternEntry { pat: "([%+%-]%d+) to level of socketed gems while there is a single gem socketed in this item", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("GemProperty"), LIST, vtab(vec![], vec![(n!("keyword"), Value::from("all")), (n!("key"), Value::from("level")), (n!("value"), Value::from(num))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" }), tag!({ type = MultiplierThreshold, var = "SocketedGemsIn{SlotName}", threshold = 1.0, equals = true })])])
    }), lua_line: 3236 });
    v.push(PatternEntry {
        pat: "socketed gems fire an additional projectile",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("ProjectileCount"), BASE, 1.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3237,
    });
    v.push(PatternEntry {
        pat: "socketed gems fire (%d+) additional projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ProjectileCount"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            )])
        }),
        lua_line: 3238,
    });
    v.push(PatternEntry {
        pat: "socketed gems reserve no mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ManaReserved"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3239,
    });
    v.push(PatternEntry {
        pat: "socketed gems have no reservation",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Reserved"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3240,
    });
    v.push(PatternEntry {
        pat: "socketed skill gems get a (%d+)%% mana multiplier",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("SupportManaMultiplier"),
                            MORE,
                            (nn(num, "perform arithmetic on local 'num'") - 100.0),
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            )])
        }),
        lua_line: 3241,
    });
    v.push(PatternEntry {
        pat: "socketed skill gems get a (%d+)%% cost & reservation multiplier",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("SupportManaMultiplier"),
                            MORE,
                            (nn(num, "perform arithmetic on local 'num'") - 100.0),
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            )])
        }),
        lua_line: 3242,
    });
    v.push(PatternEntry {
        pat: "socketed gems have blood magic",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportBloodMagicUniquePrismGuardian"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3243,
    });
    v.push(PatternEntry {
        pat: "socketed gems cost and reserve life instead of mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportBloodMagicUniquePrismGuardian"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3244,
    });
    v.push(PatternEntry {
        pat: "socketed gems have elemental equilibrium",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Elemental Equilibrium",
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3245,
    });
    v.push(PatternEntry {
        pat: "socketed gems have secrets of suffering",
        val: SpecialVal::Mods(vec![
            flag(
                n!("CannotIgnite"),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            ),
            flag(
                n!("CannotChill"),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            ),
            flag(
                n!("CannotFreeze"),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            ),
            flag(
                n!("CannotShock"),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            ),
            flag(
                n!("CritAlwaysAltAilments"),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            ),
        ]),
        lua_line: 3246,
    });
    v.push(PatternEntry {
        pat: "socketed skills deal double damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(
                        n!("DoubleDamageChance"),
                        BASE,
                        100.0,
                        None,
                        0,
                        0,
                        &[],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3253,
    });
    v.push(PatternEntry {
        pat: "socketed gems gain (%d+)%% of physical damage as extra lightning damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("PhysicalDamageGainAsLightning"),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
            )])
        }),
        lua_line: 3254,
    });
    v.push(PatternEntry {
        pat: "socketed red gems get (%d+)%% physical damage as extra fire damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("PhysicalDamageGainAsFire"),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "strength" })],
            )])
        }),
        lua_line: 3255,
    });
    v.push(PatternEntry {
        pat: "socketed non%-channelling bow skills are triggered by snipe",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3256,
    });
    v.push(PatternEntry {
        pat: "grants level (%d+) snipe skill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("ExtraSkill"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("skillId"), Value::from("Snipe")),
                            (n!("level"), Value::from(num)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("ExtraSupport"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("skillId"), Value::from("ChannelledSnipeSupport")),
                            (n!("level"), Value::from(num)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
                ),
            ])
        }),
        lua_line: 3258,
    });
    v.push(PatternEntry {
        pat: "socketed triggered bow skills deal (%d+)%% less damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("Damage"),
                            MORE,
                            (-nn(num, "perform arithmetic on local 'num'")),
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "bow" }),
                    tag!({ type = SkillType, skillType = SkillType::Triggerable }),
                ],
            )])
        }),
        lua_line: 3262,
    });
}

/// @pob Modules/ModParser.lua:3263-3345 h=48c6e5c3
fn specialModList_3263(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "socketed vaal skills require (%d+)%% less souls per use",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("SoulCost"),
                            MORE,
                            (-nn(num, "perform arithmetic on local 'num'")),
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = SkillType, skillType = SkillType::Vaal }),
                ],
            )])
        }),
        lua_line: 3263,
    });
    v.push(PatternEntry {
        pat: "hits from socketed vaal skills ignore enemy monster resistances",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("IgnoreElementalResistances"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = SkillType, skillType = SkillType::Vaal }),
                ],
            ),
            r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("IgnoreChaosResistance"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = SkillType, skillType = SkillType::Vaal }),
                ],
            ),
        ]),
        lua_line: 3264,
    });
    v.push(PatternEntry {
        pat: "hits from socketed vaal skills ignore enemy monster physical damage reduction",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("IgnoreEnemyPhysicalDamageReduction"),
                        None,
                        0,
                        0,
                        &[],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                tag!({ type = SkillType, skillType = SkillType::Vaal }),
            ],
        )]),
        lua_line: 3268,
    });
    v.push(PatternEntry {
        pat: "socketed vaal skills grant elusive when used",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeElusive"), None, 0, 0, &[])]),
        lua_line: 3269,
    });
    v.push(PatternEntry {
        pat: "damage with hits from socketed vaal skills is lucky",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("LuckyHits"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                tag!({ type = SkillType, skillType = SkillType::Vaal }),
            ],
        )]),
        lua_line: 3270,
    });
    // Global gem modifiers
    // hand-ported (lua2rs left it untranslated: local-type: table); the Lua:
    // |     ["([%+%-]%d+)%%? to (%a+) of all (.+) gems"] = function(num, _, property, skill)
    // |         if gemIdLookup[skill] then
    // |             return { mod("GemProperty", "LIST", {keyword = skill, key = "level", value = num, keyOfScaledMod = "value" }) }
    // |         end
    // |         local wordList = {}
    // |         for tag in skill:gmatch("%S+") do
    // |             table.insert(wordList, tag)
    // |         end
    // |         return { mod("GemProperty", "LIST", {keywordList = wordList, key = property, value = num, keyOfScaledMod = "value" }) }
    // |     end,
    v.push(PatternEntry {
        pat: "([%+%-]%d+)%%? to (%a+) of all (.+) gems",
        val: SpecialVal::Fn(|caps: &Caps| {
            // hand-ported
            let num = caps.num;
            let property = caps.cap[1];
            let skill = caps.cap[2];
            // L3273: gemIdLookup[skill] (the capture of `(.+)` is never nil)
            if gemIdLookup(ns(skill, "index local 'skill'")).is_some() {
                // L3274
                return Some(vec![r#mod(
                    n!("GemProperty"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("keyword"), Value::from(skill.map(Name::from))),
                            (n!("key"), Value::from("level")),
                            (n!("value"), Value::from(num)),
                            (n!("keyOfScaledMod"), Value::from("value")),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[],
                )]);
            }
            // L3276
            let mut wordList: Vec<Value> = Vec::new();
            // L3277
            for tag in pob_lua::pattern::gmatch(ns(skill, "index local 'skill'"), "%S+") {
                // L3278: table.insert(wordList, tag)
                wordList.push(Value::from(tag[0].as_str()));
            }
            // L3280
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keywordList"), vtab(wordList, vec![])),
                        (n!("key"), Value::from(property.map(Name::from))),
                        (n!("value"), Value::from(num)),
                        (n!("keyOfScaledMod"), Value::from("value")),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3272,
    });
    v.push(PatternEntry {
        pat: "gems socketed in red sockets have [%+%-](%d+) to level",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("all")),
                        (n!("key"), Value::from("level")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}", socketColor = "R" })],
            )])
        }),
        lua_line: 3282,
    });
    v.push(PatternEntry {
        pat: "gems socketed in green sockets have [%+%-](%d+)%% to quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GemProperty"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("keyword"), Value::from("all")),
                        (n!("key"), Value::from("quality")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SocketedIn, slotName = "{SlotName}", socketColor = "G" })],
            )])
        }),
        lua_line: 3283,
    });
    v.push(PatternEntry { pat: "%+(%d+)%% to fire resistance when socketed with a red gem", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SocketProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("FireResist"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "strength", sockets = Sockets::List(Arc::from(&[1.0][..])) })])])
    }), lua_line: 3284 });
    v.push(PatternEntry { pat: "%+(%d+)%% to cold resistance when socketed with a green gem", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SocketProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("ColdResist"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "dexterity", sockets = Sockets::List(Arc::from(&[1.0][..])) })])])
    }), lua_line: 3285 });
    v.push(PatternEntry { pat: "%+(%d+)%% to lightning resistance when socketed with a blue gem", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SocketProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("LightningResist"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "intelligence", sockets = Sockets::List(Arc::from(&[1.0][..])) })])])
    }), lua_line: 3286 });
    // Doomsower, Lion Sword
    v.push(PatternEntry { pat: "attack skills gain (%d+)%% of physical damage as extra fire damage per socketed red gem", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SocketProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("PhysicalDamageGainAsFire"), BASE, num, None, ModFlag::Attack, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "strength", sockets = Sockets::List(Arc::from(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0][..])) })])])
    }), lua_line: 3288 });
    v.push(PatternEntry { pat: "(%d+)%% of damage taken recouped as life per socketed red gem", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SocketProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("LifeRecoup"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "strength", sockets = Sockets::List(Arc::from(&[1.0, 2.0, 3.0, 4.0, 5.0, 6.0][..])) })])])
    }), lua_line: 3289 });
    v.push(PatternEntry { pat: "you have vaal pact while all socketed gems are red", val: SpecialVal::Mods(vec![r#mod(n!("GroupProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("Keystone"), LIST, "Vaal Pact", None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", socketColor = "R", sockets = Sockets::All })])]), lua_line: 3290 });
    v.push(PatternEntry { pat: "you have immortal ambition while all socketed gems are red", val: SpecialVal::Mods(vec![r#mod(n!("GroupProperty"), LIST, vtab(vec![], vec![(n!("value"), Value::from(r#mod(n!("Keystone"), LIST, "Immortal Ambition", None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}", socketColor = "R", sockets = Sockets::All })])]), lua_line: 3291 });
    // Mahuxotl's Machination Steel Kite Shield
    v.push(PatternEntry {
        pat: "everlasting sacrifice",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:EverlastingSacrifice"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3293,
    });
    // Self hit dmg
    v.push(PatternEntry {
        pat: "take (%d+) (.+) damage when you ignite an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmg = caps.num;
            let dmgType = ns(caps.cap[1], "index local 'dmgType'");
            Some(vec![r#mod(
                n!("EyeOfInnocenceSelfDamage"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("baseDamage"), Value::from(dmg)),
                        (n!("damageType"), Value::from(&*dmgType)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3297,
    });
    v.push(PatternEntry {
        pat: "(%d+) (.+) damage taken on minion death",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmg = caps.num;
            let dmgType = ns(caps.cap[1], "index local 'dmgType'");
            Some(vec![r#mod(
                n!("HeartboundLoopSelfDamage"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("baseDamage"), Value::from(dmg)),
                        (n!("damageType"), Value::from(&*dmgType)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3300,
    });
    v.push(PatternEntry {
        pat: "take (%d+) (.+) damage when herald of thunder hits an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmg = caps.num;
            let dmgType = ns(caps.cap[1], "index local 'dmgType'");
            Some(vec![r#mod(
                n!("StormSecretSelfDamage"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("baseDamage"), Value::from(dmg)),
                        (n!("damageType"), Value::from(&*dmgType)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3303,
    });
    v.push(PatternEntry {
        pat: "take (%d+) (.+) damage when you use a skill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmg = caps.num;
            let dmgType = ns(caps.cap[1], "index local 'dmgType'");
            Some(vec![r#mod(
                n!("EnmitysEmbraceSelfDamage"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("baseDamage"), Value::from(dmg)),
                        (n!("damageType"), Value::from(&*dmgType)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3306,
    });
    v.push(PatternEntry {
        pat: "your skills deal you (%d+)%% of mana cost as (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmgMult = caps.num;
            let dmgType = ns(caps.cap[1], "index local 'dmgType'");
            Some(vec![r#mod(
                n!("ScoldsBridleSelfDamage"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("dmgMult"), Value::from(dmgMult)),
                        (n!("damageType"), Value::from(&*dmgType)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3309,
    });
    v.push(PatternEntry {
        pat:
            "your skills deal you (%d+)%% of mana spent on upfront skill mana costs as (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmgMult = caps.num;
            let dmgType = ns(caps.cap[1], "index local 'dmgType'");
            Some(vec![r#mod(
                n!("ScoldsBridleSelfDamage"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("dmgMult"), Value::from(dmgMult)),
                        (n!("damageType"), Value::from(&*dmgType)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3312,
    });
    v.push(PatternEntry { pat: "when you attack, take (%d+)%% of life as (.+) damage for each warcry exerting the attack", val: SpecialVal::Fn(|caps: &Caps| {
        let dmgMult = caps.num;
        let dmgType = ns(caps.cap[1], "index local 'dmgType'");
        Some(vec![r#mod(n!("EchoesOfCreationSelfDamage"), LIST, vtab(vec![], vec![(n!("dmgMult"), Value::from(dmgMult)), (n!("damageType"), Value::from(&*dmgType))]), None, 0, 0, &[])])
    }), lua_line: 3315 });
    // Extra skill/support
    v.push(PatternEntry {
        pat: "grants (%D+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            grantedExtraSkill(skill, Some(1.0), None)
        }),
        lua_line: 3319,
    });
    v.push(PatternEntry {
        pat: "grants level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            grantedExtraSkill(skill, num, None)
        }),
        lua_line: 3320,
    });
    v.push(PatternEntry {
        pat: "grants level (%d+) (.+), which will be used by shaper memory",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            grantedExtraSkill(skill, num, None)
        }),
        lua_line: 3321,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) when equipped",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3322,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3323,
    });
    v.push(PatternEntry {
        pat: "use level (%d+) (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3324,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) when you attack",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3325,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) when you deal a critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3326,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) when hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3327,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) when you kill an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3328,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? level (%d+) (.+) when you use a skill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3329,
    });
    v.push(PatternEntry {
        pat: "(.+) can trigger level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let sourceSkill = ns(caps.cap[0], "index local 'sourceSkill'");
            let num = ns(caps.cap[1], "index local 'num'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(num),
                Some(TriggerExtraSkillOptions {
                    sourceSkill: Some(sourceSkill.to_string()),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3330,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you use a skill while you have a spirit charge",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3331,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you hit an enemy while cursed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3332,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you hit a bleeding enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3333,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you attack with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3334,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you hit a rare or unique enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3335,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you hit a rare or unique enemy and have no mark",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3336,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you hit a frozen enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3337,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you kill a frozen enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3338,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you consume a corpse",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            if (skill == "summon phantasm skill") {
                Some(triggerExtraSkill(
                    "triggered summon phantasm skill",
                    num,
                    None,
                ))
            } else {
                Some(triggerExtraSkill(skill, num, None))
            }
        }),
        lua_line: 3339,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you attack with a bow",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3340,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3341,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when animated guardian kills an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3342,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you lose cat's stealth",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3343,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when your trap is triggered",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3344,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on hit with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3345,
    });
}

/// @pob Modules/ModParser.lua:3346-3406 h=fbf54109
fn specialModList_3346(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on melee hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3346,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on melee hit while cursed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3347,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on melee hit with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3348,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) every [%d%.]+ seconds while phasing",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3349,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you gain avian's might or avian's flight",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3350,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on melee hit if you have at least (%d+) strength",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3351,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on critical strike with cleave or reave",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                num,
                Some(TriggerExtraSkillOptions {
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3352,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on melee critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                num,
                Some(TriggerExtraSkillOptions {
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3353,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on critical strike against marked unique enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                num,
                Some(TriggerExtraSkillOptions {
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3354,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                num,
                Some(TriggerExtraSkillOptions {
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3355,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you take a critical strike from a unique enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3356,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you suppress spell damage from a unique enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3357,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when you block damage from a unique enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3358,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) on taking a savage hit from a unique enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3359,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when a totem dies while a unique enemy is in your presence",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3360,
    });
    v.push(PatternEntry { pat: "trigger level (%d+) (.+) when you use a travel skill while a unique enemy is in your presence", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, None))
    }), lua_line: 3361 });
    v.push(PatternEntry { pat: "trigger level (%d+) (.+) when you reach maximum rage while a unique enemy is in your presence", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, None))
    }), lua_line: 3362 });
    v.push(PatternEntry { pat: "trigger level (%d+) (.+) when you reach low life while a unique enemy is in your presence", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, None))
    }), lua_line: 3363 });
    v.push(PatternEntry { pat: "trigger level (%d+) (.+) when energy shield recharge starts while a unique enemy is in your presence", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, None))
    }), lua_line: 3364 });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) when your ward breaks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3365,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) once every second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3366,
    });
    v.push(PatternEntry {
        pat: "triggers level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3367,
    });
    v.push(PatternEntry { pat: "trigger level (%d+) (.+) on attack critical strike against a rare or unique enemy and y?o?u? ?have no mark", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, Some(TriggerExtraSkillOptions { onCrit: true, ..Default::default() })))
    }), lua_line: 3368 });
    v.push(PatternEntry {
        pat: "triggers level (%d+) (.+) when equipped",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3369,
    });
    v.push(PatternEntry {
        pat: "triggers level (%d+) (.+) when allocated",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3370,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to attack with level (%d+) (.+) on melee hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let level = ns(caps.cap[1], "index local 'level'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(level),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3371,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to trigger level (%d+) (.+) when animated weapon kills an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let level = ns(caps.cap[1], "index local 'level'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(level),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3372,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to trigger level (%d+) (.+) on melee hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let level = ns(caps.cap[1], "index local 'level'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(level),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3373,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to trigger level (%d+) (.+) [ow][nh]e?n? ?y?o?u? kill ?a?n? ?e?n?e?m?y?", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let level = ns(caps.cap[1], "index local 'level'");
        let skill = ns(caps.cap[2], "index local 'skill'");
        Some(triggerExtraSkill(skill, tonumber(level), Some(TriggerExtraSkillOptions { triggerChance: chance, ..Default::default() })))
    }), lua_line: 3374 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to trigger level (%d+) (.+) when you use a socketed skill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let level = ns(caps.cap[1], "index local 'level'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(level),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3375,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to trigger level (%d+) (.+) when you gain avian's might or avian's flight", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let level = ns(caps.cap[1], "index local 'level'");
        let skill = ns(caps.cap[2], "index local 'skill'");
        Some(triggerExtraSkill(skill, tonumber(level), Some(TriggerExtraSkillOptions { triggerChance: chance, ..Default::default() })))
    }), lua_line: 3376 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to trigger level (%d+) (.+) on critical strike with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let level = ns(caps.cap[1], "index local 'level'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(level),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3377,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to trigger level (%d+) (.+) when you or a nearby ally kill an enemy, or hit a rare or unique enemy", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let level = ns(caps.cap[1], "index local 'level'");
        let skill = ns(caps.cap[2], "index local 'skill'");
        Some(triggerExtraSkill(skill, tonumber(level), Some(TriggerExtraSkillOptions { triggerChance: chance, ..Default::default() })))
    }), lua_line: 3378 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to trigger (.+) when you kill an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3379,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to [ct][ar][si][tg]g?e?r? level (%d+) (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let level = ns(caps.cap[1], "index local 'level'");
            let skill = ns(caps.cap[2], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                tonumber(level),
                Some(TriggerExtraSkillOptions {
                    triggerChance: chance,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3380,
    });
    v.push(PatternEntry {
        pat: "attack with level (%d+) (.+) when you kill a bleeding enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3381,
    });
    v.push(PatternEntry {
        pat: "triggers? level (%d+) (.+) when you kill a bleeding enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3382,
    });
    v.push(PatternEntry {
        pat: "curse enemies with (%D+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3383,
    });
    v.push(PatternEntry { pat: "curse enemies with (%D+) on %a+, with (%d+)%% increased effect", val: SpecialVal::Fn(|caps: &Caps| {
        let skill = ns(caps.cap[0], "index local 'skill'");
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("ExtraSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from(gemIdLookup(skill))), (n!("level"), Value::from(1.0)), (n!("noSupports"), Value::from(true)), (n!("triggered"), Value::from(true))]), None, 0, 0, &[]), r#mod(n!("CurseEffect"), INC, tonumber(num), None, 0, 0, &[tag!({ type = SkillName, skillName = Name::from(&*sub(&*gsub_fn(&*cat!(" ", skill), "%W%l", |c| Some(upper(c[0].as_str())), None).0, 2, None).into_owned()) })])])
    }), lua_line: 3384 });
    v.push(PatternEntry { pat: "curse enemies with (%D+) on %a+, with (%d+)%% reduced effect", val: SpecialVal::Fn(|caps: &Caps| {
        let skill = ns(caps.cap[0], "index local 'skill'");
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("ExtraSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from(gemIdLookup(skill))), (n!("level"), Value::from(1.0)), (n!("noSupports"), Value::from(true)), (n!("triggered"), Value::from(true))]), None, 0, 0, &[]), r#mod(n!("CurseEffect"), INC, (-nn(tonumber(num), "perform arithmetic on a nil value")), None, 0, 0, &[tag!({ type = SkillName, skillName = Name::from(&*sub(&*gsub_fn(&*cat!(" ", skill), "%W%l", |c| Some(upper(c[0].as_str())), None).0, 2, None).into_owned()) })])])
    }), lua_line: 3388 });
    v.push(PatternEntry { pat: "%d+%% chance to curse n?o?n?%-?c?u?r?s?e?d? ?enemies with (%D+) on %a+, with (%d+)%% increased effect", val: SpecialVal::Fn(|caps: &Caps| {
        let skill = ns(caps.cap[0], "index local 'skill'");
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("ExtraSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from(gemIdLookup(skill))), (n!("level"), Value::from(1.0)), (n!("noSupports"), Value::from(true)), (n!("triggered"), Value::from(true))]), None, 0, 0, &[]), r#mod(n!("CurseEffect"), INC, tonumber(num), None, 0, 0, &[tag!({ type = SkillName, skillName = Name::from(&*sub(&*gsub_fn(&*cat!(" ", skill), "%W%l", |c| Some(upper(c[0].as_str())), None).0, 2, None).into_owned()) })])])
    }), lua_line: 3392 });
    v.push(PatternEntry {
        pat: "%d+%% chance to curse n?o?n?%-?c?u?r?s?e?d? ?enemies with (%D+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(vec![r#mod(
                n!("ExtraSkill"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("skillId"), Value::from(gemIdLookup(skill))),
                        (n!("level"), Value::from(1.0)),
                        (n!("noSupports"), Value::from(true)),
                        (n!("triggered"), Value::from(true)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3396,
    });
    v.push(PatternEntry {
        pat: "curse enemies with level (%d+) (%D+) on %a+, which can apply to hexproof enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                num,
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ignoreHexproof: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3399,
    });
    v.push(PatternEntry {
        pat: "curse enemies with level (%d+) (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                num,
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3400,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3401,
    });
    v.push(PatternEntry {
        pat: "[at][tr][ti][ag][cg][ke]r? (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3402,
    });
    v.push(PatternEntry {
        pat: "[at][tr][ti][ag][cg][ke]r? with (.+) on %a+",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3403,
    });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? (.+) when hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3404,
    });
    v.push(PatternEntry {
        pat: "[at][tr][ti][ag][cg][ke]r? (.+) when hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3405,
    });
    v.push(PatternEntry {
        pat: "[at][tr][ti][ag][cg][ke]r? with (.+) when hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3406,
    });
}

/// @pob Modules/ModParser.lua:3407-3487 h=6b219bc3
fn specialModList_3407(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r?s? (.+) when your skills or minions kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3407,
    });
    v.push(PatternEntry {
        pat: "[at][tr][ti][ag][cg][ke]r? (.+) when you take a critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3408,
    });
    v.push(PatternEntry {
        pat: "[at][tr][ti][ag][cg][ke]r? with (.+) when you take a critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3409,
    });
    v.push(PatternEntry {
        pat: "trigger commandment of inferno on critical strike",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ExtraSkill"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (
                            n!("skillId"),
                            Value::from("UniqueEnchantmentOfInfernoOnCrit"),
                        ),
                        (n!("level"), Value::from(1.0)),
                        (n!("noSupports"), Value::from(true)),
                        (n!("triggered"), Value::from(true)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("ExtraSkillMod"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("SkillData"),
                            LIST,
                            vtab(
                                vec![],
                                vec![
                                    (n!("key"), Value::from("triggerOnCrit")),
                                    (n!("value"), Value::from(true)),
                                ],
                            ),
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillId, skillId = "UniqueEnchantmentOfInfernoOnCrit" })],
            ),
        ]),
        lua_line: 3410,
    });
    v.push(PatternEntry {
        pat: "trigger (.+) on critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    onCrit: true,
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3413,
    });
    v.push(PatternEntry {
        pat: "triggers? (.+) when you take a critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(triggerExtraSkill(
                skill,
                Some(1.0),
                Some(TriggerExtraSkillOptions {
                    noSupports: Some(true),
                    ..Default::default()
                }),
            ))
        }),
        lua_line: 3414,
    });
    v.push(PatternEntry {
        pat: "socketed [%a+]* ?gems a?r?e? ?supported by level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let support = ns(caps.cap[1], "index local 'support'");
            extraSupport(support, num, None, None)
        }),
        lua_line: 3415,
    });
    v.push(PatternEntry {
        pat: "socketed [%a+]* ?spells a?r?e? ?supported by level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let support = ns(caps.cap[1], "index local 'support'");
            extraSupport(support, num, None, None)
        }),
        lua_line: 3416,
    });
    v.push(PatternEntry {
        pat: "skills from equipped (.+) are supported by level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let slot = ns(caps.cap[0], "index local 'slot'");
            let level = ns(caps.cap[1], "index local 'level'");
            let support = ns(caps.cap[2], "index local 'support'");
            extraSupport(support, tonumber(level), Some(slot), None)
        }),
        lua_line: 3417,
    });
    // despite the wording, this can support item granted skills
    v.push(PatternEntry {
        pat: "skills socketed in your (.+) are supported by level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let slot = ns(caps.cap[0], "index local 'slot'");
            let level = ns(caps.cap[1], "index local 'level'");
            let support = ns(caps.cap[2], "index local 'support'");
            extraSupport(support, tonumber(level), Some(slot), Some(true))
        }),
        lua_line: 3419,
    });
    v.push(PatternEntry {
        pat: "skills granted by your passive tree are supported by level (%d+) (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let level = ns(caps.cap[0], "index local 'level'");
            let support = ns(caps.cap[1], "index local 'support'");
            extraSupport(support, tonumber(level), Some("passive tree"), Some(true))
        }),
        lua_line: 3420,
    });
    v.push(PatternEntry { pat: "socketed support gems can also support skills from y?o?u?r? ?e?q?u?i?p?p?e?d? ?([%a%s]+)", val: SpecialVal::Fn(|caps: &Caps| {
        let itemSlotName = ns(caps.cap[0], "index local 'itemSlotName'");
        // L3422
        let mut targetItemSlotName = "Body Armour".to_string();
        // L3423
        if (itemSlotName == "main hand") {
            // L3424
            targetItemSlotName = "Weapon 1".to_string();
        }
        // L3426
        return Some(vec![r#mod(n!("LinkedSupport"), LIST, vtab(vec![], vec![(n!("targetSlotName"), Value::from(&*targetItemSlotName))]), None, 0, 0, &[])]);
    }), lua_line: 3421 });
    v.push(PatternEntry {
        pat: "socketed hex curse skills are triggered by doedre's effigy when summoned",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportCursePillarTriggerCurses"),
                    ),
                    (n!("level"), Value::from(20.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 3428,
    });
    v.push(PatternEntry {
        pat: "socketed projectile spells have %+([%d%.]+) seconds to cooldown",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CooldownRecovery"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SocketedIn, slotName = "{SlotName}" }),
                    tag!({ type = SkillType, skillType = SkillType::Projectile }),
                    tag!({ type = SkillType, skillType = SkillType::Spell }),
                ],
            )])
        }),
        lua_line: 3429,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3432,
    });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+), (.+) or (.+) every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill1 = ns(caps.cap[1], "index local 'skill1'");
            let skill2 = ns(caps.cap[2], "index local 'skill2'");
            let skill3 = ns(caps.cap[3], "index local 'skill3'");
            Some(vec![
                r#mod(
                    n!("ExtraSkill"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("skillId"), Value::from(gemIdLookup(skill1))),
                            (n!("level"), Value::from(num)),
                            (n!("triggered"), Value::from(true)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("ExtraSkill"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("skillId"), Value::from(gemIdLookup(skill2))),
                            (n!("level"), Value::from(num)),
                            (n!("triggered"), Value::from(true)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("ExtraSkill"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("skillId"), Value::from(gemIdLookup(skill3))),
                            (n!("level"), Value::from(num)),
                            (n!("triggered"), Value::from(true)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 3433,
    });
    v.push(PatternEntry { pat: "offering skills triggered this way also affect you", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("SkillData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("buffNotPlayer")), (n!("value"), Value::from(false))]), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Bone Offering"), n!("Flesh Offering"), n!("Spirit Offering")][..] })])]), lua_line: 3438 });
    v.push(PatternEntry {
        pat: "trigger level (%d+) (.+) after spending a total of (%d+) mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3439,
    });
    v.push(PatternEntry {
        pat: "consumes a void charge to trigger level (%d+) (.+) when you fire arrows",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(triggerExtraSkill(skill, num, None))
        }),
        lua_line: 3440,
    });
    v.push(PatternEntry { pat: "consumes a void charge to trigger level (%d+) (.+) when you fire arrows with a non%-triggered skill", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, None))
    }), lua_line: 3441 });
    v.push(PatternEntry {
        pat: "your hits treat cold resistance as (%d+)%% higher than actual value",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ColdPenetration"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                KeywordFlag::Hit,
                &[],
            )])
        }),
        lua_line: 3442,
    });
    // Conversion
    v.push(PatternEntry {
        pat: "increases and reductions to minion damage also affects? you",
        val: SpecialVal::Mods(vec![flag(
            n!("MinionDamageAppliesToPlayer"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3446,
    });
    v.push(PatternEntry {
        pat:
            "increases and reductions to minion damage also affects? you at (%d+)%% of their value",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("MinionDamageAppliesToPlayer"), None, 0, 0, &[]),
                r#mod(
                    n!("ImprovedMinionDamageAppliesToPlayer"),
                    MAX,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 3447,
    });
    v.push(PatternEntry { pat: "increases and reductions to minion damage also affect dominating blow and absolution at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("MinionDamageAppliesToPlayer"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Dominating Blow"), n!("Absolution")][..], includeTransfigured = true })]), r#mod(n!("ImprovedMinionDamageAppliesToPlayer"), MAX, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Dominating Blow"), n!("Absolution")][..], includeTransfigured = true })])])
    }), lua_line: 3448 });
    v.push(PatternEntry {
        pat: "increases and reductions to minion attack speed also affects? you",
        val: SpecialVal::Mods(vec![flag(
            n!("MinionAttackSpeedAppliesToPlayer"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3452,
    });
    v.push(PatternEntry {
        pat: "increases and reductions to minion cast speed also affects? you",
        val: SpecialVal::Mods(vec![flag(
            n!("MinionCastSpeedAppliesToPlayer"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3453,
    });
    v.push(PatternEntry { pat: "increases and reductions to minion maximum life also apply to you at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("MinionLifeAppliesToPlayer"), None, 0, 0, &[]), r#mod(n!("ImprovedMinionLifeAppliesToPlayer"), MAX, num, None, 0, 0, &[])])
    }), lua_line: 3454 });
    v.push(PatternEntry { pat: "increases and reductions to cast speed apply to attack speed at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("CastSpeedAppliesToAttacks"), None, 0, 0, &[]), r#mod(n!("ImprovedCastSpeedAppliesToAttacks"), MAX, num, None, 0, 0, &[])])
    }), lua_line: 3455 });
    v.push(PatternEntry {
        pat: "increases and reductions to cast speed apply to attack speed",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![flag(n!("CastSpeedAppliesToAttacks"), None, 0, 0, &[])])
        }),
        lua_line: 3456,
    });
    v.push(PatternEntry {
        pat: "increases and reductions to spell damage also apply to attacks",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellDamageAppliesToAttacks"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3457,
    });
    v.push(PatternEntry {
        pat: "increases and reductions to your evasion rating also apply to your spell damage",
        val: SpecialVal::Mods(vec![flag(
            n!("EvasionAppliesToSpellDamage"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3458,
    });
    v.push(PatternEntry {
        pat: "arcane might",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellDamageAppliesToAttacks"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3459,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% arcane might",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("SpellDamageAppliesToAttacks"), None, 0, 0, &[]),
                r#mod(
                    n!("ImprovedSpellDamageAppliesToAttacks"),
                    MAX,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 3460,
    });
    v.push(PatternEntry {
        pat: "attacks have (%d+)%% arcane might",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("SpellDamageAppliesToAttacks"), None, 0, 0, &[]),
                r#mod(
                    n!("ImprovedSpellDamageAppliesToAttacks"),
                    MAX,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 3461,
    });
    v.push(PatternEntry {
        pat: "attacks have (%d+)%% arcane might while wielding a wand",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(
                    n!("SpellDamageAppliesToAttacks"),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "UsingWand" })],
                ),
                r#mod(
                    n!("ImprovedSpellDamageAppliesToAttacks"),
                    MAX,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "UsingWand" })],
                ),
            ])
        }),
        lua_line: 3462,
    });
    v.push(PatternEntry {
        pat: "retaliation skills have (%d+)%% arcane might",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(
                    n!("SpellDamageAppliesToAttacks"),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillType, skillType = SkillType::Retaliation })],
                ),
                r#mod(
                    n!("ImprovedSpellDamageAppliesToAttacks"),
                    MAX,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillType, skillType = SkillType::Retaliation })],
                ),
            ])
        }),
        lua_line: 3463,
    });
    v.push(PatternEntry {
        pat: "attacks have arcane might while wielding a wand",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellDamageAppliesToAttacks"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingWand" })],
        )]),
        lua_line: 3467,
    });
    v.push(PatternEntry { pat: "increases and reductions to spell damage also apply to attacks at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("SpellDamageAppliesToAttacks"), None, 0, 0, &[]), r#mod(n!("ImprovedSpellDamageAppliesToAttacks"), MAX, num, None, 0, 0, &[])])
    }), lua_line: 3468 });
    v.push(PatternEntry { pat: "increases and reductions to spell damage also apply to attack damage with retaliation skills at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("SpellDamageAppliesToAttacks"), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Retaliation })]), r#mod(n!("ImprovedSpellDamageAppliesToAttacks"), MAX, num, None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Retaliation })])])
    }), lua_line: 3469 });
    v.push(PatternEntry {
        pat: "increases and reductions to spell damage also apply to attacks while wielding a wand",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellDamageAppliesToAttacks"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingWand" })],
        )]),
        lua_line: 3473,
    });
    v.push(PatternEntry { pat: "increases and reductions to maximum mana also apply to shock effect at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("ManaAppliesToShockEffect"), None, 0, 0, &[]), r#mod(n!("ImprovedManaAppliesToShockEffect"), MAX, num, None, 0, 0, &[])])
    }), lua_line: 3474 });
    v.push(PatternEntry { pat: "increases and reductions to (%a+) damage also apply to effect of auras from (%a+) skills at (%d+)%% of their value, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let element = ns(caps.cap[0], "index local 'element'");
        let skillElement = ns(caps.cap[1], "index local 'skillElement'");
        let effect = ns(caps.cap[2], "index local 'effect'");
        let limit = ns(caps.cap[3], "index local 'limit'");
        Some(vec![flag(cat!(gsub_fn(element, "^%l", |c| Some(upper(c[0].as_str())), None).0, "DamageAppliesTo", cat!(gsub_fn(skillElement, "^%l", |c| Some(upper(c[0].as_str())), None).0, "AuraEffect")), None, 0, 0, &[]), r#mod(cat!(cat!("Improved", gsub_fn(element, "^%l", |c| Some(upper(c[0].as_str())), None).0), "DamageAppliesTo", cat!(gsub_fn(skillElement, "^%l", |c| Some(upper(c[0].as_str())), None).0, "AuraEffect")), BASE, tonumber(effect), None, 0, 0, &[]), r#mod(cat!(gsub_fn(element, "^%l", |c| Some(upper(c[0].as_str())), None).0, "DamageAppliesTo", cat!(gsub_fn(skillElement, "^%l", |c| Some(upper(c[0].as_str())), None).0, "AuraEffectLimit")), MAX, tonumber(limit), None, 0, 0, &[])])
    }), lua_line: 3475 });
    v.push(PatternEntry {
        pat: "modifiers to claw damage also apply to unarmed",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawDamageAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3480,
    });
    v.push(PatternEntry {
        pat: "modifiers to claw damage also apply to unarmed attack damage",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawDamageAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3481,
    });
    v.push(PatternEntry {
        pat: "modifiers to claw damage also apply to unarmed attack damage with melee skills",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawDamageAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3482,
    });
    v.push(PatternEntry {
        pat: "modifiers to claw attack speed also apply to unarmed",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawAttackSpeedAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3483,
    });
    v.push(PatternEntry {
        pat: "modifiers to claw attack speed also apply to unarmed attack speed",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawAttackSpeedAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3484,
    });
    v.push(PatternEntry {
        pat: "modifiers to claw attack speed also apply to unarmed attack speed with melee skills",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawAttackSpeedAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3485,
    });
    v.push(PatternEntry {
        pat: "modifiers to claw critical strike chance also apply to unarmed",
        val: SpecialVal::Mods(vec![flag(
            n!("ClawCritChanceAppliesToUnarmed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3486,
    });
    v.push(PatternEntry { pat: "modifiers to claw critical strike chance also apply to unarmed attack critical strike chance", val: SpecialVal::Mods(vec![flag(n!("ClawCritChanceAppliesToUnarmed"), None, 0, 0, &[])]), lua_line: 3487 });
}

/// @pob Modules/ModParser.lua:3488-3575 h=c4345f68
fn specialModList_3488(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "modifiers to claw critical strike chance also apply to unarmed critical strike chance with melee skills", val: SpecialVal::Mods(vec![flag(n!("ClawCritChanceAppliesToUnarmed"), None, 0, 0, &[])]), lua_line: 3488 });
    v.push(PatternEntry {
        pat: "increases and reductions to light radius also apply to accuracy",
        val: SpecialVal::Mods(vec![flag(
            n!("LightRadiusAppliesToAccuracy"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3489,
    });
    v.push(PatternEntry { pat: "increases and reductions to light radius also apply to area of effect at 50%% of their value", val: SpecialVal::Mods(vec![flag(n!("LightRadiusAppliesToAreaOfEffect"), None, 0, 0, &[])]), lua_line: 3490 });
    v.push(PatternEntry {
        pat: "increases and reductions to light radius also apply to damage",
        val: SpecialVal::Mods(vec![flag(
            n!("LightRadiusAppliesToDamage"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3491,
    });
    v.push(PatternEntry {
        pat: "increases and reductions to cast speed also apply to trap throwing speed",
        val: SpecialVal::Mods(vec![flag(
            n!("CastSpeedAppliesToTrapThrowingSpeed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3492,
    });
    v.push(PatternEntry { pat: "increases and reductions to armour also apply to energy shield recharge rate at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("ArmourAppliesToEnergyShieldRecharge"), None, 0, 0, &[]), r#mod(n!("ImprovedArmourAppliesToEnergyShieldRecharge"), MAX, num, None, 0, 0, &[])])
    }), lua_line: 3493 });
    v.push(PatternEntry {
        pat: "increases and reductions to projectile speed also apply to damage with bows",
        val: SpecialVal::Mods(vec![flag(
            n!("ProjectileSpeedAppliesToBowDamage"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3494,
    });
    v.push(PatternEntry { pat: "modifiers to maximum (%a+) resistance also apply to maximum (%a+) and (%a+) resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let resFrom = ns(caps.cap[0], "index local 'resFrom'");
        let resTo1 = ns(caps.cap[1], "index local 'resTo1'");
        let resTo2 = ns(caps.cap[2], "index local 'resTo2'");
        Some(vec![r#mod(cat!(gsub_fn(resFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0, "MaxResConvertTo", gsub_fn(resTo1, "^%l", |c| Some(upper(c[0].as_str())), None).0), BASE, 100.0, None, 0, 0, &[]), r#mod(cat!(gsub_fn(resFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0, "MaxResConvertTo", gsub_fn(resTo2, "^%l", |c| Some(upper(c[0].as_str())), None).0), BASE, 100.0, None, 0, 0, &[])])
    }), lua_line: 3495 });
    v.push(PatternEntry { pat: "modifiers to (%a+) resistance also apply to (%a+) and (%a+) resistances at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let resFrom = ns(caps.cap[0], "index local 'resFrom'");
        let resTo1 = ns(caps.cap[1], "index local 'resTo1'");
        let resTo2 = ns(caps.cap[2], "index local 'resTo2'");
        let rate = ns(caps.cap[3], "index local 'rate'");
        Some(vec![r#mod(cat!(gsub_fn(resFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0, "ResConvertTo", gsub_fn(resTo1, "^%l", |c| Some(upper(c[0].as_str())), None).0), BASE, tonumber(rate), None, 0, 0, &[]), r#mod(cat!(gsub_fn(resFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0, "ResConvertTo", gsub_fn(resTo2, "^%l", |c| Some(upper(c[0].as_str())), None).0), BASE, tonumber(rate), None, 0, 0, &[])])
    }), lua_line: 3499 });
    v.push(PatternEntry {
        pat: "gain (%d+)%% of bow physical damage as extra damage of each element",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("PhysicalDamageGainAsLightning"),
                    BASE,
                    num,
                    None,
                    ModFlag::Bow,
                    0,
                    &[],
                ),
                r#mod(
                    n!("PhysicalDamageGainAsCold"),
                    BASE,
                    num,
                    None,
                    ModFlag::Bow,
                    0,
                    &[],
                ),
                r#mod(
                    n!("PhysicalDamageGainAsFire"),
                    BASE,
                    num,
                    None,
                    ModFlag::Bow,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 3503,
    });
    v.push(PatternEntry {
        pat: "gain (%d+)%% of weapon physical damage as extra damage of each element",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("PhysicalDamageGainAsLightning"),
                    BASE,
                    num,
                    None,
                    ModFlag::Weapon,
                    0,
                    &[],
                ),
                r#mod(
                    n!("PhysicalDamageGainAsCold"),
                    BASE,
                    num,
                    None,
                    ModFlag::Weapon,
                    0,
                    &[],
                ),
                r#mod(
                    n!("PhysicalDamageGainAsFire"),
                    BASE,
                    num,
                    None,
                    ModFlag::Weapon,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 3508,
    });
    v.push(PatternEntry {
        pat: "gain (%d+)%% of physical damage as extra damage of each element per spirit charge",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("PhysicalDamageGainAsLightning"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "SpiritCharge" })],
                ),
                r#mod(
                    n!("PhysicalDamageGainAsCold"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "SpiritCharge" })],
                ),
                r#mod(
                    n!("PhysicalDamageGainAsFire"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "SpiritCharge" })],
                ),
            ])
        }),
        lua_line: 3513,
    });
    v.push(PatternEntry { pat: "gain (%d+)%% of physical damage as extra damage of each element if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let thresh = ns(caps.cap[1], "index local 'thresh'");
        let influence = ns(caps.cap[2], "index local 'influence'");
        Some(vec![r#mod(n!("PhysicalDamageGainAsLightning"), BASE, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })]), r#mod(n!("PhysicalDamageGainAsCold"), BASE, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })]), r#mod(n!("PhysicalDamageGainAsFire"), BASE, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 3518 });
    v.push(PatternEntry {
        pat: "gain (%d+)%% of weapon physical damage as extra damage of an? r?a?n?d?o?m? ?element",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PhysicalDamageGainAsRandom"),
                BASE,
                num,
                None,
                ModFlag::Weapon,
                0,
                &[],
            )])
        }),
        lua_line: 3523,
    });
    v.push(PatternEntry {
        pat: "gain (%d+)%% of physical damage as extra damage of a random element",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PhysicalDamageGainAsRandom"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3524,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance for hits to deal (%d+)%% of physical damage as extra damage of a random element", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let physPercent = ns(caps.cap[1], "index local 'physPercent'");
        Some(vec![r#mod(n!("PhysicalDamageGainAsRandom"), BASE, ((nn(num, "perform arithmetic on local 'num'") * snum(physPercent, "perform arithmetic on a string")) / 100.0), None, 0, 0, &[])])
    }), lua_line: 3525 });
    v.push(PatternEntry { pat: "gain (%d+)%% of physical damage as a random element if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let curse = ns(caps.cap[1], "index local 'curse'");
        Some(vec![r#mod(n!("PhysicalDamageGainAsRandom"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 3526 });
    v.push(PatternEntry { pat: "gain (%d+)%% of physical damage as extra damage of a random element while you are ignited", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PhysicalDamageGainAsRandom"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "Ignited" })])])
    }), lua_line: 3527 });
    v.push(PatternEntry { pat: "(%d+)%% of physical damage from hits with this weapon is converted to a random element", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PhysicalDamageConvertToRandom"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "{Hand}Attack" })])])
    }), lua_line: 3528 });
    v.push(PatternEntry {
        pat: "(%d+)%% of physical damage converted to a random element",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PhysicalDamageConvertToRandom"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3529,
    });
    v.push(PatternEntry {
        pat: "nearby enemies convert (%d+)%% of their (%a+) damage to (%a+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let damageFrom = ns(caps.cap[1], "index local 'damageFrom'");
            let damageTo = ns(caps.cap[2], "index local 'damageTo'");
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            cat!(
                                gsub_fn(damageFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                "DamageConvertTo",
                                gsub_fn(damageTo, "^%l", |c| Some(upper(c[0].as_str())), None).0
                            ),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3530,
    });
    v.push(PatternEntry {
        pat: "enemies ignited by you have (%d+)%% of (%a+) damage they deal converted to (%a+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let damageFrom = ns(caps.cap[1], "index local 'damageFrom'");
            let damageTo = ns(caps.cap[2], "index local 'damageTo'");
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            cat!(
                                gsub_fn(damageFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                "DamageConvertTo",
                                gsub_fn(damageTo, "^%l", |c| Some(upper(c[0].as_str())), None).0
                            ),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Ignited" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3531,
    });
    v.push(PatternEntry {
        pat: "enemies shocked by you have (%d+)%% of (%a+) damage they deal converted to (%a+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let damageFrom = ns(caps.cap[1], "index local 'damageFrom'");
            let damageTo = ns(caps.cap[2], "index local 'damageTo'");
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            cat!(
                                gsub_fn(damageFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                "DamageConvertTo",
                                gsub_fn(damageTo, "^%l", |c| Some(upper(c[0].as_str())), None).0
                            ),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Shocked" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3532,
    });
    v.push(PatternEntry {
        pat: "enemies poisoned by you have (%d+)%% of (%a+) damage they deal converted to (%a+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let damageFrom = ns(caps.cap[1], "index local 'damageFrom'");
            let damageTo = ns(caps.cap[2], "index local 'damageTo'");
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            cat!(
                                gsub_fn(damageFrom, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                "DamageConvertTo",
                                gsub_fn(damageTo, "^%l", |c| Some(upper(c[0].as_str())), None).0
                            ),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Poisoned" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3533,
    });
    v.push(PatternEntry { pat: "shield crush and spectral shield throw do not gain added physical damage based on armour or evasion on shield", val: SpecialVal::Mods(vec![flag(n!("Condition:ShieldThrowCrushNoArmourEvasion"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Spectral Shield Throw"), n!("Shield Crush")][..], includeTransfigured = true })])]), lua_line: 3534 });
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber
    v.push(PatternEntry { pat: "shield crush and spectral shield throw gains (%d+) to (%d+) added lightning damage per (%d+) energy shield on shield", val: SpecialVal::Fn(|caps: &Caps| {
        let min = ns(caps.cap[0], "index local 'min'");
        let max = ns(caps.cap[1], "index local 'max'");
        let num = ns(caps.cap[2], "index local 'num'");
        Some(vec![r#mod(n!("LightningMin"), BASE, &*min, None, 0, 0, &[tag!({ type = Condition, var = "OffHandAttack" }), tag!({ type = PerStat, stat = "EnergyShieldOnWeapon 2", div = snum(num, "perform arithmetic on tag.div") }), tag!({ type = SkillName, skillNameList = &[n!("Spectral Shield Throw"), n!("Shield Crush")][..], includeTransfigured = true })]), r#mod(n!("LightningMax"), BASE, &*max, None, 0, 0, &[tag!({ type = Condition, var = "OffHandAttack" }), tag!({ type = PerStat, stat = "EnergyShieldOnWeapon 2", div = snum(num, "perform arithmetic on tag.div") }), tag!({ type = SkillName, skillNameList = &[n!("Spectral Shield Throw"), n!("Shield Crush")][..], includeTransfigured = true })])])
    }), lua_line: 3535 });
    v.push(PatternEntry { pat: "(%d+)%% of shield crush and spectral shield throw physical damage converted to lightning damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillPhysicalDamageConvertToLightning"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Spectral Shield Throw"), n!("Shield Crush")][..], includeTransfigured = true })])])
    }), lua_line: 3539 });
    v.push(PatternEntry { pat: "(%d+)%% of exsanguinate and reap physical damage converted to fire damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillPhysicalDamageConvertToFire"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Exsanguinate"), n!("Reap")][..], includeTransfigured = true })])])
    }), lua_line: 3542 });
    v.push(PatternEntry {
        pat: "%-(%d+)%% of toxic rain physical damage converted to chaos damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SkillPhysicalDamageConvertToChaos"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Toxic Rain", includeTransfigured = true })],
            )])
        }),
        lua_line: 3545,
    });
    v.push(PatternEntry { pat: "cobra lash and venom gyre have %-(%d+)%% of physical damage converted to chaos damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillPhysicalDamageConvertToChaos"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Cobra Lash"), n!("Venom Gyre")][..] })])])
    }), lua_line: 3548 });
    v.push(PatternEntry { pat: "(%d+)%% of consecrated path and purifying flame fire damage converted to chaos damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillFireDamageConvertToChaos"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Consecrated Path"), n!("Purifying Flame")][..], includeTransfigured = true })])])
    }), lua_line: 3551 });
    v.push(PatternEntry { pat: "(%d+)%% of manabond and stormbind lightning damage converted to cold damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillLightningDamageConvertToCold"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Manabond"), n!("Stormbind")][..], includeTransfigured = true })])])
    }), lua_line: 3554 });
    v.push(PatternEntry { pat: "exsanguinate debuffs deal fire damage per second instead of physical damage per second", val: SpecialVal::Mods(vec![flag(n!("Condition:ExsanguinateDebuffIsFireDamage"), None, 0, 0, &[tag!({ type = SkillName, skillName = "Exsanguinate", includeTransfigured = true })])]), lua_line: 3557 });
    v.push(PatternEntry {
        pat: "reap debuffs deal fire damage per second instead of physical damage per second",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:ReapDebuffIsFireDamage"),
            None,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Reap" })],
        )]),
        lua_line: 3558,
    });
    // Crit
    v.push(PatternEntry {
        pat: "your critical strike chance is lucky",
        val: SpecialVal::Mods(vec![flag(n!("CritChanceLucky"), None, 0, 0, &[])]),
        lua_line: 3560,
    });
    v.push(PatternEntry {
        pat: "your critical strike chance is lucky while on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("CritChanceLucky"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife" })],
        )]),
        lua_line: 3561,
    });
    v.push(PatternEntry {
        pat: "your critical strike chance is lucky while focus?sed",
        val: SpecialVal::Mods(vec![flag(
            n!("CritChanceLucky"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Focused" })],
        )]),
        lua_line: 3562,
    });
    v.push(PatternEntry {
        pat: "your critical strikes do not deal extra damage",
        val: SpecialVal::Mods(vec![flag(n!("NoCritMultiplier"), None, 0, 0, &[])]),
        lua_line: 3563,
    });
    v.push(PatternEntry {
        pat: "critical strikes do not deal extra damage",
        val: SpecialVal::Mods(vec![flag(n!("NoCritMultiplier"), None, 0, 0, &[])]),
        lua_line: 3564,
    });
    v.push(PatternEntry {
        pat: "critical strikes with this weapon do not deal extra damage",
        val: SpecialVal::Mods(vec![flag(
            n!("NoCritMultiplier"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3565,
    });
    v.push(PatternEntry {
        pat: "minion critical strikes do not deal extra damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("NoCritMultiplier"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3566,
    });
    v.push(PatternEntry {
        pat: "lightning damage with non%-critical strikes is lucky",
        val: SpecialVal::Mods(vec![flag(n!("LightningNoCritLucky"), None, 0, 0, &[])]),
        lua_line: 3567,
    });
    v.push(PatternEntry {
        pat: "your damage with critical strikes is lucky",
        val: SpecialVal::Mods(vec![flag(n!("CritLucky"), None, 0, 0, &[])]),
        lua_line: 3568,
    });
    v.push(PatternEntry {
        pat: "spell critical strike chance bifurcates",
        val: SpecialVal::Mods(vec![flag(
            n!("BifurcateCrit"),
            None,
            ModFlag::Spell,
            0,
            &[],
        )]),
        lua_line: 3569,
    });
    v.push(PatternEntry {
        pat: "critical strikes deal no damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Damage"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 3570,
    });
    v.push(PatternEntry {
        pat: "critical strike chance is increased by uncapped lightning resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("CritChanceIncreasedByUncappedLightningRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3571,
    });
    v.push(PatternEntry {
        pat: "critical strike chance is increased by lightning resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("CritChanceIncreasedByLightningRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3572,
    });
    v.push(PatternEntry {
        pat: "critical strike chance is increased by overcapped lightning resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("CritChanceIncreasedByOvercappedLightningRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3573,
    });
    v.push(PatternEntry { pat: "barrage and frenzy have (%d+)%% increased critical strike chance per endurance charge", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritChance"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "EnduranceCharge" }), tag!({ type = SkillName, skillNameList = &[n!("Barrage"), n!("Frenzy")][..], includeTransfigured = true })])])
    }), lua_line: 3574 });
    v.push(PatternEntry {
        pat: "non%-critical strikes deal (%d+)%% damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                (-100.0 + nn(num, "perform arithmetic on local 'num'")),
                None,
                ModFlag::Hit,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike", neg = true })],
            )])
        }),
        lua_line: 3575,
    });
}

/// @pob Modules/ModParser.lua:3576-3697 h=646771bb
fn specialModList_3576(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "non%-critical strikes deal no damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Damage"),
            MORE,
            -100.0,
            None,
            ModFlag::Hit,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike", neg = true })],
        )]),
        lua_line: 3576,
    });
    v.push(PatternEntry {
        pat: "non%-critical strikes deal (%d+)%% less damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                ModFlag::Hit,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike", neg = true })],
            )])
        }),
        lua_line: 3577,
    });
    v.push(PatternEntry {
        pat: "spell skills always deal critical strikes on final repeat",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellSkillsAlwaysDealCriticalStrikesOnFinalRepeat"),
            None,
            ModFlag::Spell,
            0,
            &[],
        )]),
        lua_line: 3578,
    });
    v.push(PatternEntry {
        pat: "spell skills cannot deal critical strikes except on final repeat",
        val: SpecialVal::Mods(vec![
            flag(
                n!("SpellSkillsCannotDealCriticalStrikesExceptOnFinalRepeat"),
                None,
                ModFlag::Spell,
                0,
                &[],
            ),
            flag(
                n!(""),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "alwaysFinalRepeat" })],
            ),
        ]),
        lua_line: 3579,
    });
    v.push(PatternEntry { pat: "critical strikes penetrate (%d+)%% of enemy elemental resistances while affected by zealotry", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ElementalPenetration"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "CriticalStrike" }), tag!({ type = Condition, var = "AffectedByZealotry" })])])
    }), lua_line: 3580 });
    v.push(PatternEntry {
        pat: "attack critical strikes ignore enemy monster elemental resistances",
        val: SpecialVal::Mods(vec![flag(
            n!("IgnoreElementalResistances"),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "CriticalStrike" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
        )]),
        lua_line: 3581,
    });
    v.push(PatternEntry {
        pat: "treats enemy monster elemental resistance values as inverted",
        val: SpecialVal::Mods(vec![r#mod(
            n!("HitsInvertEleResChance"),
            CHANCE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3582,
    });
    v.push(PatternEntry {
        pat: "treats enemy monster chaos resistance values as inverted",
        val: SpecialVal::Mods(vec![r#mod(
            n!("HitsInvertChaosResChance"),
            CHANCE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "{Hand}Attack" })],
        )]),
        lua_line: 3583,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+)%% to critical strike multiplier if you've shattered an enemy recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritMultiplier"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShatteredEnemyRecently" })],
            )])
        }),
        lua_line: 3584,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain a flask charge when you deal a critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskChargeOnCritChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3585,
    });
    v.push(PatternEntry {
        pat: "gain a flask charge when you deal a critical strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("FlaskChargeOnCritChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3586,
    });
    v.push(PatternEntry {
        pat: "gain a flask charge when you deal a critical strike while affected by precision",
        val: SpecialVal::Mods(vec![r#mod(
            n!("FlaskChargeOnCritChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByPrecision" })],
        )]),
        lua_line: 3587,
    });
    v.push(PatternEntry { pat: "gain a flask charge when you deal a critical strike while at maximum frenzy charges", val: SpecialVal::Mods(vec![r#mod(n!("FlaskChargeOnCritChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })])]), lua_line: 3588 });
    v.push(PatternEntry { pat: "on non%-channelling attack, set a life flask with greater than %d+%% of maximum charges remaining to (%d+)%% for each charge removed this way, that attack gains %+(%d+)%% to damage over time multiplier", val: SpecialVal::Fn(|caps: &Caps| {
        let chargesConsumedPercent = caps.num;
        let damage = ns(caps.cap[1], "index local 'damage'");
        Some(vec![r#mod(n!("DotMultiplier"), BASE, tonumber(damage), None, 0, 0, &[tag!({ type = PercentStat, stat = "LifeFlaskCharges", percent = (100.0 - nn(chargesConsumedPercent, "perform arithmetic on local 'chargesConsumedPercent'")), floor = true }), tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }), tag!({ type = SkillType, skillType = SkillType::Attack })])])
    }), lua_line: 3589 });
    v.push(PatternEntry {
        pat: "enemies poisoned by you cannot deal critical strikes",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(
                            n!("NeverCrit"),
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Poisoned" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(
                            n!("Condition:NeverCrit"),
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Poisoned" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 3592,
    });
    v.push(PatternEntry {
        pat: "marked enemy cannot deal critical strikes",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(
                            n!("NeverCrit"),
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Marked" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(
                            n!("Condition:NeverCrit"),
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Marked" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 3593,
    });
    v.push(PatternEntry {
        pat: "marked enemy cannot evade attacks",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("CannotEvade"),
                        None,
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Marked" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3594,
    });
    v.push(PatternEntry {
        pat: "hits against you cannot be critical strikes if you've been stunned recently",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("NeverCrit"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StunnedRecently" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:NeverCrit"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StunnedRecently" })],
            ),
        ]),
        lua_line: 3595,
    });
    v.push(PatternEntry {
        pat: "nearby enemies cannot deal critical strikes",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("NeverCrit"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:NeverCrit"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 3596,
    });
    v.push(PatternEntry {
        pat: "hits against you are always critical strikes",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("AlwaysCrit"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:AlwaysCrit"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 3597,
    });
    v.push(PatternEntry {
        pat: "your hits are always critical strikes",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CritChance"),
            OVERRIDE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3598,
    });
    v.push(PatternEntry {
        pat: "all hits are critical strikes while holding a fishing rod",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CritChance"),
            OVERRIDE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFishing" })],
        )]),
        lua_line: 3599,
    });
    v.push(PatternEntry { pat: "all hits with your next non%-channelling attack within (%d+) seconds of taking a critical strike will be critical strikes", val: SpecialVal::Mods(vec![r#mod(n!("CritChance"), OVERRIDE, 100.0, None, 0, 0, &[tag!({ type = Condition, var = "BeenCritRecently" }), tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }), tag!({ type = SkillType, skillType = SkillType::Attack })])]), lua_line: 3600 });
    v.push(PatternEntry {
        pat: "hits have (%d+)%% increased critical strike chance against you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyCritChance"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3601,
    });
    v.push(PatternEntry {
        pat: "stuns from critical strikes have (%d+)%% increased duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyStunDurationOnCrit"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3602,
    });
    // Generic Ailments
    v.push(PatternEntry { pat: "enemies take (%d+)%% increased damage for each type of ailment you have inflicted on them", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Scorched" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Brittle" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Sapped" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })])])
    }), lua_line: 3604 });
    v.push(PatternEntry { pat: "(%d+)%% chance to deal double damage against enemies for each type of ailment you have inflicted on them", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Scorched" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Brittle" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Sapped" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })]), r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })])])
    }), lua_line: 3615 });
    // Elemental Ailments
    v.push(PatternEntry { pat: "(%d+)%% increased elemental damage with hits and ailments for each type of elemental ailment on enemy", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })]), r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })]), r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })]), r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })]), r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Scorched" })]), r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Brittle" })]), r#mod(n!("ElementalDamage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "Sapped" })])])
    }), lua_line: 3627 });
    v.push(PatternEntry {
        pat: "your shocks can increase damage taken by up to a maximum of (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ShockMax"), OVERRIDE, num, None, 0, 0, &[])])
        }),
        lua_line: 3636,
    });
    v.push(PatternEntry {
        pat: "%+(%d+)%% to maximum effect of shock",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ShockMax"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 3637,
    });
    v.push(PatternEntry {
        pat: "your (%w+) damage can (%w+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let element = ns(caps.cap[0], "index local 'element'");
            let ailment = ns(caps.cap[1], "index local 'ailment'");
            Some(vec![flag(
                cat!(firstToUpper(element), "Can", firstToUpper(ailment)),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3638,
    });
    v.push(PatternEntry {
        pat: "your (%w+) damage cannot (%w+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let element = ns(caps.cap[0], "index local 'element'");
            let ailment = ns(caps.cap[1], "index local 'ailment'");
            Some(vec![flag(
                cat!(firstToUpper(element), "Cannot", firstToUpper(ailment)),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3641,
    });
    v.push(PatternEntry {
        pat: "your elemental damage can shock",
        val: SpecialVal::Mods(vec![
            flag(n!("ColdCanShock"), None, 0, 0, &[]),
            flag(n!("FireCanShock"), None, 0, 0, &[]),
        ]),
        lua_line: 3644,
    });
    v.push(PatternEntry {
        pat: "all y?o?u?r? ?damage can freeze",
        val: SpecialVal::Mods(vec![
            flag(n!("PhysicalCanFreeze"), None, 0, 0, &[]),
            flag(n!("LightningCanFreeze"), None, 0, 0, &[]),
            flag(n!("FireCanFreeze"), None, 0, 0, &[]),
            flag(n!("ChaosCanFreeze"), None, 0, 0, &[]),
        ]),
        lua_line: 3645,
    });
    v.push(PatternEntry {
        pat: "all damage with maces and sceptres inflicts chill",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingMace" })],
            ),
            flag(
                n!("LightningCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingMace" })],
            ),
            flag(
                n!("FireCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingMace" })],
            ),
            flag(
                n!("ChaosCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingMace" })],
            ),
        ]),
        lua_line: 3646,
    });
    v.push(PatternEntry { pat: "all damage from lightning strike and frost blades hits can ignite", val: SpecialVal::Mods(vec![flag(n!("PhysicalCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Strike"), n!("Frost Blades")][..], includeTransfigured = true })]), flag(n!("ColdCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Strike"), n!("Frost Blades")][..], includeTransfigured = true })]), flag(n!("LightningCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Strike"), n!("Frost Blades")][..], includeTransfigured = true })]), flag(n!("ChaosCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Strike"), n!("Frost Blades")][..], includeTransfigured = true })])]), lua_line: 3652 });
    v.push(PatternEntry { pat: "all damage from lightning arrow and ice shot hits can ignite", val: SpecialVal::Mods(vec![flag(n!("PhysicalCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Arrow"), n!("Ice Shot")][..], includeTransfigured = true })]), flag(n!("ColdCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Arrow"), n!("Ice Shot")][..], includeTransfigured = true })]), flag(n!("LightningCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Arrow"), n!("Ice Shot")][..], includeTransfigured = true })]), flag(n!("ChaosCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Arrow"), n!("Ice Shot")][..], includeTransfigured = true })])]), lua_line: 3658 });
    v.push(PatternEntry { pat: "all damage from shock nova and storm call hits can ignite", val: SpecialVal::Mods(vec![flag(n!("PhysicalCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Shock Nova"), n!("Storm Call")][..], includeTransfigured = true })]), flag(n!("ColdCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Shock Nova"), n!("Storm Call")][..], includeTransfigured = true })]), flag(n!("LightningCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Shock Nova"), n!("Storm Call")][..], includeTransfigured = true })]), flag(n!("ChaosCanIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Shock Nova"), n!("Storm Call")][..], includeTransfigured = true })])]), lua_line: 3664 });
    v.push(PatternEntry {
        pat: "your fire damage can shock but not ignite",
        val: SpecialVal::Mods(vec![
            flag(n!("FireCanShock"), None, 0, 0, &[]),
            flag(n!("FireCannotIgnite"), None, 0, 0, &[]),
        ]),
        lua_line: 3670,
    });
    v.push(PatternEntry {
        pat: "your cold damage can ignite but not freeze or chill",
        val: SpecialVal::Mods(vec![
            flag(n!("ColdCanIgnite"), None, 0, 0, &[]),
            flag(n!("ColdCannotFreeze"), None, 0, 0, &[]),
            flag(n!("ColdCannotChill"), None, 0, 0, &[]),
        ]),
        lua_line: 3671,
    });
    v.push(PatternEntry {
        pat: "your lightning damage can freeze but not shock",
        val: SpecialVal::Mods(vec![
            flag(n!("LightningCanFreeze"), None, 0, 0, &[]),
            flag(n!("LightningCannotShock"), None, 0, 0, &[]),
        ]),
        lua_line: 3672,
    });
    v.push(PatternEntry {
        pat: "your physical damage can ignite during effect",
        val: SpecialVal::Mods(vec![flag(n!("PhysicalCanIgnite"), None, 0, 0, &[])]),
        lua_line: 3673,
    });
    v.push(PatternEntry {
        pat: "chaos damage can ignite, chill and shock",
        val: SpecialVal::Mods(vec![
            flag(n!("ChaosCanIgnite"), None, 0, 0, &[]),
            flag(n!("ChaosCanChill"), None, 0, 0, &[]),
            flag(n!("ChaosCanShock"), None, 0, 0, &[]),
        ]),
        lua_line: 3674,
    });
    v.push(PatternEntry {
        pat: "you always ignite while burning",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyIgniteChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Burning" })],
        )]),
        lua_line: 3675,
    });
    v.push(PatternEntry {
        pat: "critical strikes do not a?l?w?a?y?s?i?n?h?e?r?e?n?t?l?y? freeze",
        val: SpecialVal::Mods(vec![flag(n!("CritsDontAlwaysFreeze"), None, 0, 0, &[])]),
        lua_line: 3676,
    });
    v.push(PatternEntry {
        pat: "cannot inflict elemental ailments",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotIgnite"), None, 0, 0, &[]),
            flag(n!("CannotChill"), None, 0, 0, &[]),
            flag(n!("CannotFreeze"), None, 0, 0, &[]),
            flag(n!("CannotShock"), None, 0, 0, &[]),
            flag(n!("CannotScorch"), None, 0, 0, &[]),
            flag(n!("CannotBrittle"), None, 0, 0, &[]),
            flag(n!("CannotSap"), None, 0, 0, &[]),
        ]),
        lua_line: 3677,
    });
    v.push(PatternEntry {
        pat: "non%-critical strikes cannot inflict ailments",
        val: SpecialVal::Mods(vec![flag(n!("AilmentsOnlyFromCrit"), None, 0, 0, &[])]),
        lua_line: 3686,
    });
    v.push(PatternEntry { pat: "flameblast and incinerate cannot inflict elemental ailments", val: SpecialVal::Mods(vec![flag(n!("CannotIgnite"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })]), flag(n!("CannotChill"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })]), flag(n!("CannotFreeze"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })]), flag(n!("CannotShock"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })]), flag(n!("CannotScorch"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })]), flag(n!("CannotBrittle"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })]), flag(n!("CannotSap"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flameblast"), n!("Incinerate")][..], includeTransfigured = true })])]), lua_line: 3687 });
    v.push(PatternEntry {
        pat: "you can inflict up to (%d+) ignites on an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("IgniteCanStack"), None, 0, 0, &[]),
                r#mod(n!("IgniteStacks"), OVERRIDE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 3696,
    });
    v.push(PatternEntry {
        pat: "you can inflict an additional ignite on [ea][an]c?h? enemy",
        val: SpecialVal::Mods(vec![
            flag(n!("IgniteCanStack"), None, 0, 0, &[]),
            r#mod(n!("IgniteStacks"), BASE, 1.0, None, 0, 0, &[]),
        ]),
        lua_line: 3697,
    });
}

/// @pob Modules/ModParser.lua:3698-3816 h=16805272
fn specialModList_3698(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "enemies chilled by you take (%d+)%% increased burning damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("FireDamageTakenOverTime"),
                            INC,
                            num,
                            None,
                            0,
                            0,
                            &[],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })],
            )])
        }),
        lua_line: 3698,
    });
    v.push(PatternEntry {
        pat: "damaging ailments deal damage (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("IgniteBurnFaster"), INC, num, None, 0, 0, &[]),
                r#mod(n!("BleedFaster"), INC, num, None, 0, 0, &[]),
                r#mod(n!("PoisonFaster"), INC, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 3699,
    });
    v.push(PatternEntry { pat: "damaging ailments you inflict deal damage (%d+)%% faster while affected by malevolence", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("IgniteBurnFaster"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByMalevolence" })]), r#mod(n!("BleedFaster"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByMalevolence" })]), r#mod(n!("PoisonFaster"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByMalevolence" })])])
    }), lua_line: 3700 });
    v.push(PatternEntry { pat: "(%d+)%% increased damage with damaging ailments you inflict while you are affected by the same ailment", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, KeywordFlag::Bleed, &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" }), tag!({ type = Condition, var = "Bleeding" })]), r#mod(n!("Damage"), INC, num, None, 0, KeywordFlag::Ignite, &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" }), tag!({ type = Condition, var = "Ignited" })]), r#mod(n!("Damage"), INC, num, None, 0, KeywordFlag::Poison, &[tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" }), tag!({ type = Condition, var = "Poisoned" })])])
    }), lua_line: 3705 });
    v.push(PatternEntry {
        pat: "ignited enemies burn (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("IgniteBurnFaster"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3710,
    });
    // Overrides the base duration of a damaging ailment (the only ailments with a fixed base duration
    // consumed by the calcs; freeze duration is derived from damage and chill/shock durations are not modelled)
    // hand-ported (lua2rs left it untranslated: and-or: orexpr and str); the Lua:
    // |     ["base (%a+) duration is ([%d%.]+) seconds?"] = function(_, ailment, num)
    // |         local ailmentName = (ailment == "bleeding" or ailment == "bleed") and "Bleed"
    // |             or (ailment == "ignite" or ailment == "poison") and firstToUpper(ailment)
    // |         if ailmentName then
    // |             return { mod(ailmentName.."DurationBase", "OVERRIDE", tonumber(num)) }
    // |         end
    // |     end,
    v.push(PatternEntry {
        pat: "base (%a+) duration is ([%d%.]+) seconds?",
        val: SpecialVal::Fn(|caps: &Caps| {
            // hand-ported
            let ailment = caps.cap[0];
            let num = caps.cap[1];
            // L3714-3715: `(a or b) and "Bleed" or (c or d) and firstToUpper(ailment)`; false when neither
            let ailmentName: Option<String> =
                if ailment == Some("bleeding") || ailment == Some("bleed") {
                    Some("Bleed".to_owned())
                } else if ailment == Some("ignite") || ailment == Some("poison") {
                    Some(firstToUpper(ns(ailment, "index local 'ailment'")))
                } else {
                    None
                };
            // L3716
            if let Some(ailmentName) = ailmentName {
                // L3717
                return Some(vec![r#mod(
                    cat!(ailmentName.as_str(), "DurationBase"),
                    OVERRIDE,
                    tonumber(ns(num, "index local 'num'")),
                    None,
                    0,
                    0,
                    &[],
                )]);
            }
            // L3719: falls off the end (nil)
            None
        }),
        lua_line: 3713,
    });
    v.push(PatternEntry {
        pat: "ignited enemies burn (%d+)%% slower",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("IgniteBurnSlower"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3720,
    });
    v.push(PatternEntry {
        pat: "enemies ignited by an attack burn (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("IgniteBurnFaster"),
                INC,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 3721,
    });
    v.push(PatternEntry {
        pat: "ignites you inflict with attacks deal damage (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("IgniteBurnFaster"),
                INC,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 3722,
    });
    v.push(PatternEntry {
        pat: "ignites you inflict deal damage (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("IgniteBurnFaster"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3723,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance for ignites inflicted with lightning strike or frost blades to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Ignite, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Strike"), n!("Frost Blades")][..], includeTransfigured = true })])])
    }), lua_line: 3724 });
    v.push(PatternEntry { pat: "(%d+)%% chance for ignites inflicted with lightning arrow or ice shot to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Ignite, &[tag!({ type = SkillName, skillNameList = &[n!("Lightning Arrow"), n!("Ice Shot")][..], includeTransfigured = true })])])
    }), lua_line: 3727 });
    v.push(PatternEntry { pat: "(%d+)%% chance for ignites inflicted with shock nova or storm call to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Ignite, &[tag!({ type = SkillName, skillNameList = &[n!("Shock Nova"), n!("Storm Call")][..], includeTransfigured = true })])])
    }), lua_line: 3730 });
    v.push(PatternEntry {
        pat: "enemies ignited by you during f?l?a?s?k? ?effect take (%d+)%% increased damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })],
            )])
        }),
        lua_line: 3733,
    });
    v.push(PatternEntry {
        pat: "enemies ignited by you take chaos damage instead of fire damage from ignite",
        val: SpecialVal::Mods(vec![flag(n!("IgniteToChaos"), None, 0, 0, &[])]),
        lua_line: 3734,
    });
    v.push(PatternEntry {
        pat: "enemies chilled by your hits are shocked",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ShockBase"),
                BASE,
                data().nonDamagingAilment[&n!("Shock")].default,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "ChilledByYourHits" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(
                            n!("Condition:Shocked"),
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "ChilledByYourHits" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 3735,
    });
    v.push(PatternEntry {
        pat: "cannot inflict ignite",
        val: SpecialVal::Mods(vec![flag(n!("CannotIgnite"), None, 0, 0, &[])]),
        lua_line: 3739,
    });
    v.push(PatternEntry {
        pat: "cannot inflict freeze or chill",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotFreeze"), None, 0, 0, &[]),
            flag(n!("CannotChill"), None, 0, 0, &[]),
        ]),
        lua_line: 3740,
    });
    v.push(PatternEntry {
        pat: "cannot inflict shock",
        val: SpecialVal::Mods(vec![flag(n!("CannotShock"), None, 0, 0, &[])]),
        lua_line: 3741,
    });
    v.push(PatternEntry {
        pat: "cannot ignite, chill, freeze or shock",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotIgnite"), None, 0, 0, &[]),
            flag(n!("CannotChill"), None, 0, 0, &[]),
            flag(n!("CannotFreeze"), None, 0, 0, &[]),
            flag(n!("CannotShock"), None, 0, 0, &[]),
        ]),
        lua_line: 3742,
    });
    v.push(PatternEntry {
        pat: "shock enemies as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ShockAsThoughDealing"),
                MORE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3743,
    });
    v.push(PatternEntry {
        pat: "chill enemies as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChillAsThoughDealing"),
                MORE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3744,
    });
    v.push(PatternEntry {
        pat: "inflict non%-damaging ailments as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("ShockAsThoughDealing"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("ChillAsThoughDealing"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("FreezeAsThoughDealing"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("ScorchAsThoughDealing"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("BrittleAsThoughDealing"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("SapAsThoughDealing"), MORE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 3745,
    });
    v.push(PatternEntry {
        pat: "non%-damaging elemental ailments you inflict have (%d+)%% more effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("EnemyShockEffect"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("EnemyChillEffect"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("EnemyFreezeEffect"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("EnemyScorchEffect"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("EnemyBrittleEffect"), MORE, num, None, 0, 0, &[]),
                r#mod(n!("EnemySapEffect"), MORE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 3753,
    });
    v.push(PatternEntry { pat: "immun[ei]t?y? to elemental ailments while on consecrated ground if you have at least (%d+) devotion", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("ElementalAilmentImmune"), None, 0, 0, &[tag!({ type = Condition, var = "OnConsecratedGround" }), tag!({ type = StatThreshold, stat = "Devotion", threshold = num })])])
    }), lua_line: 3761 });
    v.push(PatternEntry {
        pat: "freeze enemies as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FreezeAsThoughDealing"),
                MORE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3762,
    });
    v.push(PatternEntry {
        pat: "freeze chilled enemies as though dealing (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FreezeAsThoughDealing"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })],
            )])
        }),
        lua_line: 3763,
    });
    v.push(PatternEntry { pat: "manabond and stormbind freeze enemies as though dealing (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FreezeAsThoughDealing"), MORE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Manabond"), n!("Stormbind")][..], includeTransfigured = true })])])
    }), lua_line: 3764 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict brittle on enemies when you block their damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyBrittleChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3765,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict sap on enemies when you block their damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemySapChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3766,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict scorch on enemies when you block their damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyScorchChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3767,
    });
    v.push(PatternEntry {
        pat: "scorch enemies in close range when you block",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyScorchChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3768,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to shock attackers for (%d+) seconds on block",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ShockBase"),
            BASE,
            data().nonDamagingAilment[&n!("Shock")].default,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3769,
    });
    v.push(PatternEntry {
        pat: "shock attackers for (%d+) seconds on block",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ShockBase"),
                BASE,
                data().nonDamagingAilment[&n!("Shock")].default,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:Shocked"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
        ]),
        lua_line: 3770,
    });
    v.push(PatternEntry {
        pat: "shock nearby enemies for (%d+) seconds when you focus",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ShockBase"),
                BASE,
                data().nonDamagingAilment[&n!("Shock")].default,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Focused" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:Shocked"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Focused" })],
            ),
        ]),
        lua_line: 3774,
    });
    v.push(PatternEntry {
        pat: "shock yourself for (%d+) seconds when you focus",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ShockBase"),
                BASE,
                data().nonDamagingAilment[&n!("Shock")].default,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Focused" })],
            ),
            flag(
                n!("Condition:Shocked"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Focused" })],
            ),
        ]),
        lua_line: 3778,
    });
    v.push(PatternEntry {
        pat: "drops shocked ground while moving, lasting (%d+) seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ShockBase"),
            BASE,
            data().nonDamagingAilment[&n!("Shock")].default,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "OnShockedGround" })],
        )]),
        lua_line: 3782,
    });
    v.push(PatternEntry {
        pat: "drops scorched ground while moving, lasting (%d+) seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ScorchBase"),
            BASE,
            data().nonDamagingAilment[&n!("Scorch")].default,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "OnScorchedGround" })],
        )]),
        lua_line: 3783,
    });
    v.push(PatternEntry {
        pat: "drops brittle ground while moving, lasting (%d+) seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BrittleBase"),
            BASE,
            data().nonDamagingAilment[&n!("Brittle")].default,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "OnBrittleGround" })],
        )]),
        lua_line: 3784,
    });
    v.push(PatternEntry {
        pat: "drops sapped ground while moving, lasting (%d+) seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SapBase"),
            BASE,
            data().nonDamagingAilment[&n!("Sap")].default,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "OnSappedGround" })],
        )]),
        lua_line: 3785,
    });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, drops shocked ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("ShockBase"), BASE, data().nonDamagingAilment[&n!("Shock")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnShockedGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])]), lua_line: 3786 });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, drops scorched ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("ScorchBase"), BASE, data().nonDamagingAilment[&n!("Scorch")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnScorchedGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])]), lua_line: 3787 });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, drops brittle ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("BrittleBase"), BASE, data().nonDamagingAilment[&n!("Brittle")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnBrittleGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])]), lua_line: 3788 });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, drops sapped ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("SapBase"), BASE, data().nonDamagingAilment[&n!("Sap")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnSappedGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])]), lua_line: 3789 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, drops shocked ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("ShockBase"), BASE, data().nonDamagingAilment[&n!("Shock")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnShockedGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })])]), lua_line: 3790 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, drops scorched ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("ScorchBase"), BASE, data().nonDamagingAilment[&n!("Scorch")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnScorchedGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })])]), lua_line: 3791 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, drops brittle ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("BrittleBase"), BASE, data().nonDamagingAilment[&n!("Brittle")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnBrittleGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })])]), lua_line: 3792 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, drops sapped ground while moving, lasting (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("SapBase"), BASE, data().nonDamagingAilment[&n!("Sap")].default, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnSappedGround" }), tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })])]), lua_line: 3793 });
    v.push(PatternEntry {
        pat: "%+(%d+)%% chance to ignite, freeze, shock, and poison cursed enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("EnemyIgniteChance"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
                ),
                r#mod(
                    n!("EnemyFreezeChance"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
                ),
                r#mod(
                    n!("EnemyShockChance"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
                ),
                r#mod(
                    n!("PoisonChance"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
                ),
            ])
        }),
        lua_line: 3794,
    });
    v.push(PatternEntry { pat: "you have scorching conflux, brittle conflux and sapping conflux while your two highest attributes are equal", val: SpecialVal::Mods(vec![r#mod(n!("EnemyScorchChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), r#mod(n!("EnemyBrittleChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), r#mod(n!("EnemySapChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("PhysicalCanScorch"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("LightningCanScorch"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("ColdCanScorch"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("ChaosCanScorch"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("PhysicalCanBrittle"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("LightningCanBrittle"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("FireCanBrittle"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("ChaosCanBrittle"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("PhysicalCanSap"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("ColdCanSap"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("FireCanSap"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })]), flag(n!("ChaosCanSap"), None, 0, 0, &[tag!({ type = Condition, var = "TwoHighestAttributesEqual" })])]), lua_line: 3800 });
}

/// @pob Modules/ModParser.lua:3817-3912 h=0979ecf1
fn specialModList_3817(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "all damage from cold snap and creeping frost can sap", val: SpecialVal::Mods(vec![flag(n!("PhysicalCanSap"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Cold Snap"), n!("Creeping Frost")][..], includeTransfigured = true })]), flag(n!("ColdCanSap"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Cold Snap"), n!("Creeping Frost")][..], includeTransfigured = true })]), flag(n!("FireCanSap"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Cold Snap"), n!("Creeping Frost")][..], includeTransfigured = true })]), flag(n!("ChaosCanSap"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Cold Snap"), n!("Creeping Frost")][..], includeTransfigured = true })])]), lua_line: 3817 });
    v.push(PatternEntry { pat: "always inflict scorch, brittle and sapped with elemental hit and wild strike hits", val: SpecialVal::Mods(vec![r#mod(n!("EnemyScorchChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Elemental Hit"), n!("Wild Strike")][..], includeTransfigured = true })]), r#mod(n!("EnemyBrittleChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Elemental Hit"), n!("Wild Strike")][..], includeTransfigured = true })]), r#mod(n!("EnemySapChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Elemental Hit"), n!("Wild Strike")][..], includeTransfigured = true })])]), lua_line: 3823 });
    v.push(PatternEntry {
        pat: "hits with prismatic skills always ?i?n?f?l?i?c?t? (%w+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let r#type = ns(caps.cap[0], "index local 'type'");
            Some(vec![r#mod(
                cat!("Enemy", firstToUpper(r#type), "Chance"),
                BASE,
                100.0,
                None,
                ModFlag::Hit,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::RandomElement })],
            )])
        }),
        lua_line: 3828,
    });
    v.push(PatternEntry {
        pat: "critical strikes do not [ia][np][fp]l[iy]c?t? non%-damaging ailments",
        val: SpecialVal::Mods(vec![
            flag(n!("CritsDontAlwaysChill"), None, 0, 0, &[]),
            flag(n!("CritsDontAlwaysFreeze"), None, 0, 0, &[]),
            flag(n!("CritsDontAlwaysShock"), None, 0, 0, &[]),
        ]),
        lua_line: 3831,
    });
    v.push(PatternEntry {
        pat: "critical strikes do not inherently ignite",
        val: SpecialVal::Mods(vec![flag(n!("CritsDontAlwaysIgnite"), None, 0, 0, &[])]),
        lua_line: 3836,
    });
    v.push(PatternEntry {
        pat: "always scorch while affected by anger",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyScorchChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByAnger" })],
        )]),
        lua_line: 3837,
    });
    v.push(PatternEntry {
        pat: "always inflict brittle while affected by hatred",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyBrittleChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByHatred" })],
        )]),
        lua_line: 3838,
    });
    v.push(PatternEntry {
        pat: "always sap while affected by wrath",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemySapChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByWrath" })],
        )]),
        lua_line: 3839,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to sap enemies in chilling areas",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemySapChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "InChillingArea" })],
            )])
        }),
        lua_line: 3840,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance for cold snap and creeping frost to sap enemies in chilling areas", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemySapChance"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Cold Snap"), n!("Creeping Frost")][..], includeTransfigured = true }), tag!({ type = ActorCondition, actor = "enemy", var = "InChillingArea" })])])
    }), lua_line: 3841 });
    v.push(PatternEntry { pat: "drops burning ground while moving, dealing (%d+) fire damage per second for %d+ seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DropsBurningGround"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 3842 });
    v.push(PatternEntry {
        pat: "take (%d+) fire damage per second while flame%-touched",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FireDegen"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByApproachingFlames" })],
            )])
        }),
        lua_line: 3843,
    });
    v.push(PatternEntry {
        pat: "gain adrenaline when you become flame%-touched",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:Adrenaline"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByApproachingFlames" })],
        )]),
        lua_line: 3844,
    });
    v.push(PatternEntry {
        pat: "lose adrenaline when you cease to be flame%-touched",
        val: SpecialVal::Mods(vec![]),
        lua_line: 3845,
    });
    v.push(PatternEntry {
        pat: "modifiers to ignite duration on you apply to all elemental ailments",
        val: SpecialVal::Mods(vec![flag(
            n!("IgniteDurationAppliesToElementalAilments"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3846,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased duration of ailments of types you haven't inflicted recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("EnemyFreezeDuration"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "FrozenEnemyRecently", neg = true })],
                ),
                r#mod(
                    n!("EnemyChillDuration"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "FrozenEnemyRecently", neg = true })],
                ),
                r#mod(
                    n!("EnemyIgniteDuration"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "IgnitedEnemyRecently", neg = true })],
                ),
                r#mod(
                    n!("EnemyShockDuration"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "ShockedEnemyRecently", neg = true })],
                ),
                r#mod(
                    n!("EnemyBleedDuration"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "CausedBleedingRecently", neg = true })],
                ),
                r#mod(
                    n!("EnemyPoisonDuration"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "PoisonedEnemyRecently", neg = true })],
                ),
            ])
        }),
        lua_line: 3847,
    });
    // typo / old wording change
    v.push(PatternEntry {
        pat: "chance to avoid being shocked applies to all elemental ailments",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockAvoidAppliesToElementalAilments"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3855,
    });
    v.push(PatternEntry {
        pat: "modifiers to chance to avoid being shocked apply to all elemental ailments",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockAvoidAppliesToElementalAilments"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3856,
    });
    v.push(PatternEntry { pat: "enemies permanently take (%d+)%% increased damage for each second they've ever been frozen by you, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "FrozenByYou" }), tag!({ type = Multiplier, var = "FrozenByYouSeconds", limit = (snum(limit, "perform arithmetic on a string") / nn(num, "perform arithmetic on local 'num'")) })])))]), None, 0, 0, &[])])
    }), lua_line: 3857 });
    v.push(PatternEntry { pat: "enemies permanently take (%d+)%% increased damage for each second they've ever been chilled by you, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "ChilledByYou" }), tag!({ type = Multiplier, var = "ChilledByYouSeconds", limit = (snum(limit, "perform arithmetic on a string") / nn(num, "perform arithmetic on local 'num'")) })])))]), None, 0, 0, &[])])
    }), lua_line: 3858 });
    v.push(PatternEntry { pat: "modifiers to chance to suppress spell damage also apply to chance to avoid elemental ailments at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SpellSuppressionAppliesToAilmentAvoidancePercent"), BASE, num, None, 0, 0, &[]), flag(n!("SpellSuppressionAppliesToAilmentAvoidance"), None, 0, 0, &[])])
    }), lua_line: 3859 });
    v.push(PatternEntry { pat: "modifiers to chance to suppress spell damage also apply to chance to defend with (%d+)%% of armour at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let armourPercentage = caps.num;
        let effectiveness = ns(caps.cap[1], "index local 'effectiveness'");
        Some(vec![r#mod(n!("SpellSuppressionAppliesToChanceToDefendWithArmourPercentArmour"), MAX, armourPercentage, None, 0, 0, &[]), r#mod(n!("SpellSuppressionAppliesToChanceToDefendWithArmourPercent"), MAX, tonumber(effectiveness), None, 0, 0, &[]), flag(n!("SpellSuppressionAppliesToChanceToDefendWithArmour"), None, 0, 0, &[])])
    }), lua_line: 3863 });
    v.push(PatternEntry {
        pat: "enemies chilled by your hits have damage taken increased by chill effect",
        val: SpecialVal::Mods(vec![flag(n!("ChillEffectIncDamageTaken"), None, 0, 0, &[])]),
        lua_line: 3868,
    });
    v.push(PatternEntry {
        pat: "enemies chilled by your hits have cold damage taken increased by chill effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillEffectIncColdDamageTaken"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3869,
    });
    v.push(PatternEntry {
        pat: "enemies in your chilling areas have cold damage taken increased by chill effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillingAreaIncColdDamageTaken"),
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "InChillingArea" })],
        )]),
        lua_line: 3870,
    });
    v.push(PatternEntry { pat: "left ring slot: your chilling skitterbot's aura applies socketed h?e?x? ?curse instead", val: SpecialVal::Mods(vec![flag(n!("SkitterbotsCannotChill"), None, 0, 0, &[tag!({ type = SlotNumber, num = 1.0 })])]), lua_line: 3871 });
    v.push(PatternEntry { pat: "right ring slot: your shocking skitterbot's aura applies socketed h?e?x? ?curse instead", val: SpecialVal::Mods(vec![flag(n!("SkitterbotsCannotShock"), None, 0, 0, &[tag!({ type = SlotNumber, num = 2.0 })])]), lua_line: 3872 });
    v.push(PatternEntry {
        pat: "summon skitterbots also summons a scorching skitterbot",
        val: SpecialVal::Mods(vec![flag(n!("ScorchingSkitterbot"), None, 0, 0, &[])]),
        lua_line: 3873,
    });
    v.push(PatternEntry {
        pat: "summoned skitterbots' auras affect you as well as enemies",
        val: SpecialVal::Mods(vec![flag(n!("SkitterbotAffectPlayer"), None, 0, 0, &[])]),
        lua_line: 3874,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased effect of non%-damaging ailments inflicted by summoned skitterbots",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SkitterbotAilmentEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3875,
    });
    // Bleed
    v.push(PatternEntry {
        pat: "melee attacks cause bleeding",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            ModFlag::Melee,
            0,
            &[],
        )]),
        lua_line: 3877,
    });
    v.push(PatternEntry {
        pat: "attacks cause bleeding when hitting cursed enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
        )]),
        lua_line: 3878,
    });
    v.push(PatternEntry {
        pat: "melee critical strikes cause bleeding",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            ModFlag::Melee,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 3879,
    });
    v.push(PatternEntry {
        pat: "causes bleeding on melee critical strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            ModFlag::Melee,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 3880,
    });
    v.push(PatternEntry {
        pat: "melee critical strikes have (%d+)%% chance to cause bleeding",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BleedChance"),
                BASE,
                num,
                None,
                ModFlag::Melee,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            )])
        }),
        lua_line: 3881,
    });
    v.push(PatternEntry {
        pat: "attacks always inflict bleeding while you have cat's stealth",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = Condition, var = "AffectedByCat'sStealth" })],
        )]),
        lua_line: 3882,
    });
    v.push(PatternEntry {
        pat: "you have crimson dance while you have cat's stealth",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Crimson Dance",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByCat'sStealth" })],
        )]),
        lua_line: 3883,
    });
    v.push(PatternEntry {
        pat: "you have crimson dance if you have dealt a critical strike recently",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Crimson Dance",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CritRecently" })],
        )]),
        lua_line: 3884,
    });
    v.push(PatternEntry {
        pat: "bleeding you inflict deals damage (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("BleedFaster"), INC, num, None, 0, 0, &[])])
        }),
        lua_line: 3885,
    });
    v.push(PatternEntry {
        pat: "bleeding you inflict on non%-bleeding enemies deals (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                0,
                KeywordFlag::Bleed,
                &[tag!({ type = Condition, var = "SingleBleed" })],
            )])
        }),
        lua_line: 3886,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance for bleeding inflicted with this weapon to deal (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let more = ns(caps.cap[1], "index local 'more'");
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                ((nn(tonumber(more), "perform arithmetic on a nil value")
                    * nn(num, "perform arithmetic on local 'num'"))
                    / 100.0),
                None,
                0,
                KeywordFlag::Bleed,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 3889,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance for bleeding inflicted with cobra lash or venom gyre to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Bleed, &[tag!({ type = SkillName, skillNameList = &[n!("Cobra Lash"), n!("Venom Gyre")][..] })])])
    }), lua_line: 3892 });
    v.push(PatternEntry {
        pat: "bleeding you inflict deals damage (%d+)%% faster per frenzy charge",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BleedFaster"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "FrenzyCharge" })],
            )])
        }),
        lua_line: 3895,
    });
    v.push(PatternEntry { pat: "rain of arrows and toxic rain deal (%d+)%% more damage with bleeding", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, KeywordFlag::Bleed, &[tag!({ type = SkillName, skillNameList = &[n!("Rain of Arrows"), n!("Toxic Rain")][..], includeTransfigured = true })])])
    }), lua_line: 3896 });
    // Impale and Bleed
    v.push(PatternEntry {
        pat: "(%d+)%% increased effect of impales inflicted by hits that also inflict bleeding",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ImpaleEffectOnBleed"),
                INC,
                num,
                None,
                0,
                KeywordFlag::Hit,
                &[],
            )])
        }),
        lua_line: 3900,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance for blade vortex and blade blast to impale enemies on hit", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ImpaleChance"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blade Vortex"), n!("Blade Blast")][..], includeTransfigured = true })])])
    }), lua_line: 3903 });
    v.push(PatternEntry {
        pat: "critical strikes with spells inflict impale",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ImpaleChance"),
            BASE,
            100.0,
            None,
            ModFlag::Spell,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 3904,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance on hitting an enemy for all impales on that enemy to last for an additional hit", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ImpaleAdditionalDurationChance"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 3905 });
    v.push(PatternEntry { pat: "projectiles gain impale effect as they travel farther, causing impales they inflict to have up to (%d+)%% increased effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ImpaleEffect"), INC, num, None, ModFlag::Projectile, 0, &[tag!({ type = DistanceRamp, ramp = &[[35.0, 0.0], [70.0, 1.0]][..] })])])
    }), lua_line: 3908 });
    // Poison and Bleed
    v.push(PatternEntry {
        pat: "(%d+)%% increased damage with bleeding inflicted on poisoned enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                INC,
                num,
                None,
                0,
                KeywordFlag::Bleed,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })],
            )])
        }),
        lua_line: 3910,
    });
}

/// @pob Modules/ModParser.lua:3913-4022 h=bf1ec606
fn specialModList_3914(v: &mut Vec<PatternEntry<SpecialVal>>) {
    // Poison
    v.push(PatternEntry {
        pat: "y?o?u?r? ?fire damage can poison",
        val: SpecialVal::Mods(vec![flag(n!("FireCanPoison"), None, 0, 0, &[])]),
        lua_line: 3914,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?cold damage can poison",
        val: SpecialVal::Mods(vec![flag(n!("ColdCanPoison"), None, 0, 0, &[])]),
        lua_line: 3915,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?lightning damage can poison",
        val: SpecialVal::Mods(vec![flag(n!("LightningCanPoison"), None, 0, 0, &[])]),
        lua_line: 3916,
    });
    v.push(PatternEntry {
        pat: "all damage from hits can poison",
        val: SpecialVal::Mods(vec![
            flag(n!("FireCanPoison"), None, 0, 0, &[]),
            flag(n!("ColdCanPoison"), None, 0, 0, &[]),
            flag(n!("LightningCanPoison"), None, 0, 0, &[]),
        ]),
        lua_line: 3917,
    });
    v.push(PatternEntry {
        pat: "all damage can poison",
        val: SpecialVal::Mods(vec![
            flag(n!("FireCanPoison"), None, 0, 0, &[]),
            flag(n!("ColdCanPoison"), None, 0, 0, &[]),
            flag(n!("LightningCanPoison"), None, 0, 0, &[]),
        ]),
        lua_line: 3922,
    });
    v.push(PatternEntry {
        pat: "all damage with triggered spells can poison",
        val: SpecialVal::Mods(vec![
            flag(
                n!("FireCanPoison"),
                None,
                0,
                KeywordFlag::Spell,
                &[tag!({ type = SkillType, skillType = SkillType::Triggered })],
            ),
            flag(
                n!("ColdCanPoison"),
                None,
                0,
                KeywordFlag::Spell,
                &[tag!({ type = SkillType, skillType = SkillType::Triggered })],
            ),
            flag(
                n!("LightningCanPoison"),
                None,
                0,
                KeywordFlag::Spell,
                &[tag!({ type = SkillType, skillType = SkillType::Triggered })],
            ),
        ]),
        lua_line: 3927,
    });
    v.push(PatternEntry {
        pat: "all damage from hits with this weapon can poison",
        val: SpecialVal::Mods(vec![
            flag(
                n!("FireCanPoison"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            ),
            flag(
                n!("ColdCanPoison"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            ),
            flag(
                n!("LightningCanPoison"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            ),
        ]),
        lua_line: 3932,
    });
    v.push(PatternEntry {
        pat: "all damage inflicts poison while affected by glorious madness",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("PoisonChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("FireCanPoison"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("ColdCanPoison"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("LightningCanPoison"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
        ]),
        lua_line: 3937,
    });
    v.push(PatternEntry { pat: "all damage from blast rain and artillery ballista hits can poison", val: SpecialVal::Mods(vec![flag(n!("FireCanPoison"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blast Rain"), n!("Artillery Ballista")][..] })]), flag(n!("ColdCanPoison"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blast Rain"), n!("Artillery Ballista")][..] })]), flag(n!("LightningCanPoison"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blast Rain"), n!("Artillery Ballista")][..] })])]), lua_line: 3943 });
    v.push(PatternEntry { pat: "all damage from hits with freezing pulse and eye of winter can poison", val: SpecialVal::Mods(vec![flag(n!("FireCanPoison"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Freezing Pulse"), n!("Eye of Winter")][..], includeTransfigured = true })]), flag(n!("ColdCanPoison"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Freezing Pulse"), n!("Eye of Winter")][..], includeTransfigured = true })]), flag(n!("LightningCanPoison"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Freezing Pulse"), n!("Eye of Winter")][..], includeTransfigured = true })])]), lua_line: 3948 });
    v.push(PatternEntry {
        pat: "your chaos damage poisons enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChaosPoisonChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3953,
    });
    v.push(PatternEntry {
        pat: "your chaos damage has (%d+)%% chance to poison enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChaosPoisonChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3954,
    });
    v.push(PatternEntry {
        pat: "melee attacks poison on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            ModFlag::Melee,
            0,
            &[],
        )]),
        lua_line: 3955,
    });
    v.push(PatternEntry {
        pat: "triggered spells poison on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            0,
            KeywordFlag::Spell,
            &[tag!({ type = SkillType, skillType = SkillType::Triggered })],
        )]),
        lua_line: 3956,
    });
    v.push(PatternEntry {
        pat: "melee critical strikes have (%d+)%% chance to poison the enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PoisonChance"),
                BASE,
                num,
                None,
                ModFlag::Melee,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            )])
        }),
        lua_line: 3957,
    });
    v.push(PatternEntry {
        pat: "critical strikes with daggers have a (%d+)%% chance to poison the enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PoisonChance"),
                BASE,
                num,
                None,
                ModFlag::Dagger,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            )])
        }),
        lua_line: 3958,
    });
    v.push(PatternEntry {
        pat: "critical strikes with daggers poison the enemy",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            ModFlag::Dagger,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 3959,
    });
    v.push(PatternEntry {
        pat: "poison cursed enemies on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
        )]),
        lua_line: 3960,
    });
    v.push(PatternEntry {
        pat: "always poison on hit against cursed enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
        )]),
        lua_line: 3961,
    });
    v.push(PatternEntry { pat: "wh[ie][ln]e? at maximum frenzy charges, attacks poison enemies", val: SpecialVal::Mods(vec![r#mod(n!("PoisonChance"), BASE, 100.0, None, ModFlag::Attack, 0, &[tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })])]), lua_line: 3962 });
    v.push(PatternEntry {
        pat: "traps and mines have a (%d+)%% chance to poison on hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PoisonChance"),
                BASE,
                num,
                None,
                0,
                bor(KeywordFlag::Trap, KeywordFlag::Mine),
                &[],
            )])
        }),
        lua_line: 3963,
    });
    v.push(PatternEntry {
        pat: "poisons you inflict deal damage (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("PoisonFaster"), INC, num, None, 0, 0, &[])])
        }),
        lua_line: 3964,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance for poisons inflicted with this weapon to deal (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let more = ns(caps.cap[1], "index local 'more'");
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                ((nn(tonumber(more), "perform arithmetic on a nil value")
                    * nn(num, "perform arithmetic on local 'num'"))
                    / 100.0),
                None,
                0,
                KeywordFlag::Poison,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 3965,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance for poisons inflicted with blast rain or artillery balls?i?s?ta to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Poison, &[tag!({ type = SkillName, skillNameList = &[n!("Blast Rain"), n!("Artillery Ballista")][..] })])])
    }), lua_line: 3968 });
    v.push(PatternEntry { pat: "(%d+)%% chance for poisons inflicted with freezing pulse and eye of winter to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Poison, &[tag!({ type = SkillName, skillNameList = &[n!("Freezing Pulse"), n!("Eye of Winter")][..], includeTransfigured = true })])])
    }), lua_line: 3971 });
    v.push(PatternEntry {
        pat: "poisons you inflict on non%-poisoned enemies deal (%d+)%% increased damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                INC,
                num,
                None,
                0,
                KeywordFlag::Poison,
                &[tag!({ type = Condition, var = "NonPoisonedOnly" })],
            )])
        }),
        lua_line: 3975,
    });
    v.push(PatternEntry { pat: "poisons inflicted by sunder or ground slam on non%-poisoned enemies deal (%d+)%% increased damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, KeywordFlag::Poison, &[tag!({ type = Condition, var = "NonPoisonedOnly" }), tag!({ type = SkillName, skillNameList = &[n!("Sunder"), n!("Ground Slam")][..], includeTransfigured = true })])])
    }), lua_line: 3978 });
    v.push(PatternEntry {
        pat: "poisons on you expire (%d+)%% slower",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SelfPoisonDebuffExpirationRate"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3981,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to inflict an additional poison on the same target when you inflict poison", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AdditionalPoisonChance"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 3982 });
    v.push(PatternEntry { pat: "inflict (%d+) additional poisons? on the same target when you inflict poisons? with this weapon", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AdditionalPoisonStacks"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "{Hand}Attack" })])])
    }), lua_line: 3983 });
    v.push(PatternEntry {
        pat: "cannot poison enemies with at least (%d+) poisons? on them",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PoisonStackLimit"),
                MIN,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3984,
    });
    v.push(PatternEntry {
        pat: "cannot inflict multiple poisons in the same hit",
        val: SpecialVal::Mods(vec![flag(n!("CannotMultiplePoison"), None, 0, 0, &[])]),
        lua_line: 3985,
    });
    v.push(PatternEntry {
        pat: "wither on hit with this weapon against enemies with at least (%d+) poisons on them",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 3986,
    });
    // Suppression
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to suppress spell damage is lucky",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellSuppressionChanceIsLucky"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3988,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to suppress spell damage is unlucky",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellSuppressionChanceIsUnlucky"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3989,
    });
    v.push(PatternEntry {
        pat: "prevent %+(%d+)%% of suppressed spell damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SpellSuppressionEffect"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3990,
    });
    v.push(PatternEntry {
        pat: "prevent %+(%d+)%% of suppressed spell damage per hit suppressed recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SpellSuppressionEffect"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "HitsSuppressedRecently" })],
            )])
        }),
        lua_line: 3991,
    });
    v.push(PatternEntry { pat: "prevent %+(%d+)%% of suppressed spell damage if you have not suppressed spell damage recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SpellSuppressionEffect"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "SuppressedRecently", neg = true })])])
    }), lua_line: 3994 });
    v.push(PatternEntry { pat: "inflict fire, cold and lightning exposure on enemies when you suppress their spell damage", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireExposure"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Effective" }), tag!({ type = Condition, var = "SuppressedRecently" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ColdExposure"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Effective" }), tag!({ type = Condition, var = "SuppressedRecently" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LightningExposure"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Effective" }), tag!({ type = Condition, var = "SuppressedRecently" })])]), lua_line: 3997 });
    v.push(PatternEntry {
        pat: "critical strike chance is increased by chance to suppress spell damage",
        val: SpecialVal::Mods(vec![flag(
            n!("CritChanceIncreasedBySpellSuppressChance"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4002,
    });
    v.push(PatternEntry {
        pat: "you take (%d+)%% reduced extra damage from suppressed critical strikes",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReduceSuppressedCritExtraDamage"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4003,
    });
    v.push(PatternEntry { pat: "+(%d+)%% chance to suppress spell damage if your e?q?u?i?p?p?e?d? ?boots, helmet and gloves have evasion", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SpellSuppressionChance"), BASE, num, None, 0, 0, &[tag!({ type = StatThreshold, stat = "EvasionOnBoots", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "EvasionOnHelmet", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "EvasionOnGloves", threshold = 1.0 })])])
    }), lua_line: 4004 });
    v.push(PatternEntry {
        pat: "evasion rating is doubled against projectile attacks",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileEvasion"),
            MORE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4011,
    });
    v.push(PatternEntry {
        pat: "evasion rating is doubled against melee attacks",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MeleeEvasion"),
            MORE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4012,
    });
    v.push(PatternEntry {
        pat: "+(%d+)%% chance to suppress spell damage for each dagger you're wielding",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("SpellSuppressionChance"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "UsingDagger" })],
                ),
                r#mod(
                    n!("SpellSuppressionChance"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "DualWieldingDaggers" })],
                ),
            ])
        }),
        lua_line: 4013,
    });
    // Buffs/debuffs
    v.push(PatternEntry {
        pat: "phasing",
        val: SpecialVal::Mods(vec![flag(n!("Condition:Phasing"), None, 0, 0, &[])]),
        lua_line: 4018,
    });
    v.push(PatternEntry {
        pat: "onslaught",
        val: SpecialVal::Mods(vec![flag(n!("Condition:Onslaught"), None, 0, 0, &[])]),
        lua_line: 4019,
    });
    v.push(PatternEntry {
        pat: "rampage",
        val: SpecialVal::Mods(vec![flag(n!("Condition:Rampage"), None, 0, 0, &[])]),
        lua_line: 4020,
    });
    v.push(PatternEntry {
        pat: "soul eater",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveSoulEater"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4021,
    });
    v.push(PatternEntry {
        pat: "unholy might",
        val: SpecialVal::Mods(vec![
            flag(n!("Condition:UnholyMight"), None, 0, 0, &[]),
            flag(n!("Condition:CanWither"), None, 0, 0, &[]),
        ]),
        lua_line: 4022,
    });
}

/// @pob Modules/ModParser.lua:4023-4036 h=fe3efa9f
fn specialModList_4023(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "chaotic might",
        val: SpecialVal::Mods(vec![flag(n!("Condition:ChaoticMight"), None, 0, 0, &[])]),
        lua_line: 4023,
    });
    v.push(PatternEntry {
        pat: "elusive",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeElusive"), None, 0, 0, &[])]),
        lua_line: 4024,
    });
    v.push(PatternEntry {
        pat: "adrenaline",
        val: SpecialVal::Mods(vec![flag(n!("Condition:Adrenaline"), None, 0, 0, &[])]),
        lua_line: 4025,
    });
    v.push(PatternEntry {
        pat: "arcane surge",
        val: SpecialVal::Mods(vec![flag(n!("Condition:ArcaneSurge"), None, 0, 0, &[])]),
        lua_line: 4026,
    });
    v.push(PatternEntry {
        pat: "your aura buffs do not affect allies",
        val: SpecialVal::Mods(vec![flag(
            n!("SelfAurasCannotAffectAllies"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4027,
    });
    v.push(PatternEntry { pat: "your curses have (%d+)%% increased effect if (%d+)%% of curse duration expired", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("CurseEffect"), INC, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, actor = "enemy", var = "CurseExpired", threshold = tonumber(limit) }), tag!({ type = SkillType, skillType = SkillType::Hex })])])
    }), lua_line: 4028 });
    v.push(PatternEntry { pat: "non%-aura hexes expire upon reaching (%d+)%% of base effect non%-aura hexes gain (%d+)%% increased effect per second", val: SpecialVal::Fn(|caps: &Caps| {
        let limit = caps.num;
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("CurseEffect"), INC, tonumber(num), None, 0, 0, &[tag!({ type = Multiplier, actor = "enemy", var = "CurseDurationExpired", limit = limit, limitTotal = true }), tag!({ type = SkillType, skillType = SkillType::Aura, neg = true }), tag!({ type = SkillType, skillType = SkillType::Hex })])])
    }), lua_line: 4031 });
    v.push(PatternEntry { pat: "enemies cursed by you have malediction if (%d+)%% of curse duration expired", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("HasMalediction"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "CurseExpired", threshold = num }), tag!({ type = ActorCondition, var = "Cursed" })])))]), None, 0, 0, &[])])
    }), lua_line: 4034 });
}

/// (Provenance: the per-block `@pob` markers above.)
/// The entries of ModParser.lua 3084-4036, in source order (508 entries).
pub fn entries() -> Vec<PatternEntry<SpecialVal>> {
    let mut v = Vec::with_capacity(508);
    specialModList_3084(&mut v);
    specialModList_3187(&mut v);
    specialModList_3263(&mut v);
    specialModList_3346(&mut v);
    specialModList_3407(&mut v);
    specialModList_3488(&mut v);
    specialModList_3576(&mut v);
    specialModList_3698(&mut v);
    specialModList_3817(&mut v);
    specialModList_3914(&mut v);
    specialModList_4023(&mut v);
    v
}
