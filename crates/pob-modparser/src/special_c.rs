//! @pob-file Modules/ModParser.lua @16de4b82d
//! specialModList entries, ModParser.lua 4037-4988 (618 entries). `special_a`..`special_d`
//! concatenate, in source order, into `specialModList` (generated_lists.rs, MP0).
//!
//! Owner: MP5. All 618 entries (344 static tables, 274 closures) were translated by
//! scripts/port/lua2rs (unit T1) and reviewed against the Lua; none needed hand-porting.
//! One `PatternEntry` per Lua key, in source order, with a provenance marker per block of about
//! 50 entries. The range has no duplicate key (within it or against the other ranges).
//! Known parse-level differences (docs/port/known_diffs.toml, calc doc §6, CCR-0006 §7):
//! MP5-1, the tags of 4576-4580 that follow a nil `mod[1]` are dropped (ipairs never sees them);
//! MP5-2, `percent` of the PercentStat tags of 4216/4222 holds `tonumber` of the capture. Closure parameters follow the specialModList call shape
//! `f(tonumber(cap[1]), unpack(cap))` (ModParser.lua:6662-6663; calc doc §6 / CCR-0006 §5):
//! #1 = `caps.num`, #k≥2 = `caps.cap[k-2]`.
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
    data, tag, AStr, JewelFuncSpec, KeywordFlag, Mod, ModFlag, ModType, ModType::*, Name,
    SkillType, Sockets, Tag, Value, ValueTable,
};
use pob_lua::bit::{band, bnot, bor};
use pob_lua::math::{lmod, lpow, m_abs, m_ceil, m_floor, m_huge, m_max, m_min, round};
use pob_lua::pattern::{gsub, gsub_fn};
use pob_lua::string::{firstToUpper, len, lower, sub, upper};
use pob_lua::{cat, lfmt, lua_error, n, tonumber, tostring_num};
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

/// @pob Modules/ModParser.lua:4037-4118 h=5292cbbe
fn specialModList_4037(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "enemies cursed by you are hindered if (%d+)%% of curse duration expired", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Hindered"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "CurseExpired", threshold = num }), tag!({ type = ActorCondition, var = "Cursed" })])))]), None, 0, 0, &[])])
    }), lua_line: 4037 });
    v.push(PatternEntry {
        pat: "excommunicate enemies on melee hit for (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanExcommunicate"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4040,
    });
    v.push(PatternEntry {
        pat: "auras from your skills can only affect you",
        val: SpecialVal::Mods(vec![flag(n!("SelfAurasOnlyAffectYou"), None, 0, 0, &[])]),
        lua_line: 4041,
    });
    v.push(PatternEntry {
        pat: "auras from your skills which affect allies also affect enemies",
        val: SpecialVal::Mods(vec![flag(n!("AurasAffectEnemies"), None, 0, 0, &[])]),
        lua_line: 4042,
    });
    v.push(PatternEntry { pat: "aura buffs from skills have (%d+)%% increased effect on you for each herald affecting you", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillAuraEffectOnSelf"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "Herald" })])])
    }), lua_line: 4043 });
    v.push(PatternEntry { pat: "aura buffs from skills have (%d+)%% increased effect on you for each herald affecting you, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("SkillAuraEffectOnSelf"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "Herald", globalLimit = tonumber(limit), globalLimitKey = "PurposefulHarbinger" })])])
    }), lua_line: 4044 });
    v.push(PatternEntry { pat: "auras from your skills have (%d+)%% increased effect on you for each herald affecting you, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("SkillAuraEffectOnSelf"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "Herald", globalLimit = tonumber(limit), globalLimitKey = "PurposefulHarbinger" })])])
    }), lua_line: 4047 });
    v.push(PatternEntry { pat: "(%d+)%% increased area of effect per power charge, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("AreaOfEffect"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "PowerCharge", globalLimit = tonumber(limit), globalLimitKey = "VastPower" })])])
    }), lua_line: 4050 });
    v.push(PatternEntry { pat: "(%d+)%% increased area of effect per second you've been stationary, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("AreaOfEffect"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "StationarySeconds", globalLimit = tonumber(limit), globalLimitKey = "ExpansiveMight", limitTotal = true })])])
    }), lua_line: 4053 });
    v.push(PatternEntry { pat: "(%d+)%% increased chaos damage per (%d+) maximum mana, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        let limit = ns(caps.cap[2], "index local 'limit'");
        Some(vec![r#mod(n!("ChaosDamage"), INC, num, None, 0, 0, &[tag!({ type = PerStat, stat = "Mana", div = tonumber(div), globalLimit = tonumber(limit), globalLimitKey = "DarkIdeation" })])])
    }), lua_line: 4056 });
    v.push(PatternEntry { pat: "minions have %+(%d+)%% to damage over time multiplier per ghastly eye jewel affecting you, up to a maximum of %+(%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DotMultiplier"), BASE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "GhastlyEyeJewel", actor = "parent", globalLimit = tonumber(limit), globalLimitKey = "AmanamuGaze" })])))]), None, 0, 0, &[])])
    }), lua_line: 4059 });
    v.push(PatternEntry { pat: "(%d+)%% increased effect of arcane surge on you per hypnotic eye jewel affecting you, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("ArcaneSurgeEffect"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "HypnoticEyeJewel", globalLimit = tonumber(limit), globalLimitKey = "KurgalGaze" })])])
    }), lua_line: 4062 });
    v.push(PatternEntry { pat: "(%d+)%% increased main hand critical strike chance per murderous eye jewel affecting you, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("CritChance"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "MurderousEyeJewel", globalLimit = tonumber(limit), globalLimitKey = "TecrodGazeMainHand" }), tag!({ type = Condition, var = "MainHandAttack" })])])
    }), lua_line: 4065 });
    v.push(PatternEntry { pat: "%+(%d+)%% to off hand critical strike multiplier per murderous eye jewel affecting you, up to a maximum of %+(%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("CritMultiplier"), BASE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "MurderousEyeJewel", globalLimit = tonumber(limit), globalLimitKey = "TecrodGazeOffHand" }), tag!({ type = Condition, var = "OffHandAttack" })])])
    }), lua_line: 4068 });
    v.push(PatternEntry {
        pat: "nearby allies' damage with hits is lucky",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("onlyAllies"), Value::from(true)),
                    (
                        n!("mod"),
                        Value::from(flag(n!("LuckyHits"), None, 0, 0, &[])),
                    ),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4071,
    });
    v.push(PatternEntry {
        pat: "your damage with hits is lucky",
        val: SpecialVal::Mods(vec![flag(n!("LuckyHits"), None, 0, 0, &[])]),
        lua_line: 4072,
    });
    v.push(PatternEntry {
        pat: "your damage with hits is lucky while on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("LuckyHits"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife" })],
        )]),
        lua_line: 4073,
    });
    v.push(PatternEntry {
        pat: "damage with hits is unlucky",
        val: SpecialVal::Mods(vec![flag(n!("UnluckyHits"), None, 0, 0, &[])]),
        lua_line: 4074,
    });
    v.push(PatternEntry {
        pat: "chaos damage with hits is lucky",
        val: SpecialVal::Mods(vec![flag(n!("ChaosLuckyHits"), None, 0, 0, &[])]),
        lua_line: 4075,
    });
    v.push(PatternEntry {
        pat: "lightning damage with hits is lucky if you[' ]h?a?ve blocked spell damage recently",
        val: SpecialVal::Mods(vec![flag(
            n!("LightningLuckHits"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BlockedSpellRecently" })],
        )]),
        lua_line: 4076,
    });
    v.push(PatternEntry {
        pat: "cold damage with hits is lucky if you[' ]h?a?ve suppressed spell damage recently",
        val: SpecialVal::Mods(vec![flag(
            n!("ColdLuckyHits"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "SuppressedRecently" })],
        )]),
        lua_line: 4077,
    });
    v.push(PatternEntry {
        pat: "fire damage with hits is lucky if you[' ]h?a?ve blocked an attack recently",
        val: SpecialVal::Mods(vec![flag(
            n!("FireLuckyHits"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BlockedAttackRecently" })],
        )]),
        lua_line: 4078,
    });
    v.push(PatternEntry {
        pat: "elemental damage with hits is lucky while you are shocked",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalLuckHits"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Shocked" })],
        )]),
        lua_line: 4079,
    });
    v.push(PatternEntry {
        pat: "your lucky or unlucky effects are instead unexciting",
        val: SpecialVal::Mods(vec![flag(n!("Unexciting"), None, 0, 0, &[])]),
        lua_line: 4080,
    });
    v.push(PatternEntry {
        pat: "allies' aura buffs do not affect you",
        val: SpecialVal::Mods(vec![flag(
            n!("AlliesAurasCannotAffectSelf"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4081,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased effect of non%-curse auras from your skills on enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("DebuffEffect"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillType, skillType = SkillType::Aura }),
                        tag!({ type = SkillType, skillType = SkillType::AppliesCurse, neg = true }),
                    ],
                ),
                r#mod(
                    n!("AuraEffect"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = "Death Aura" })],
                ),
            ])
        }),
        lua_line: 4082,
    });
    v.push(PatternEntry {
        pat: "enemies can have 1 additional curse",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyCurseLimit"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4086,
    });
    v.push(PatternEntry {
        pat: "you can apply an additional curse",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyCurseLimit"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4087,
    });
    v.push(PatternEntry {
        pat: "you can apply an additional curse during effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyCurseLimit"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4088,
    });
    v.push(PatternEntry {
        pat: "you can apply an additional curse while affected by malevolence",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyCurseLimit"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByMalevolence" })],
        )]),
        lua_line: 4089,
    });
    v.push(PatternEntry { pat: "you can apply an additional curse while at maximum power charges", val: SpecialVal::Mods(vec![r#mod(n!("EnemyCurseLimit"), BASE, 1.0, None, 0, 0, &[tag!({ type = StatThreshold, stat = "PowerCharges", thresholdStat = "PowerChargesMax" })])]), lua_line: 4090 });
    v.push(PatternEntry { pat: "you can apply an additional curse if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let thresh = ns(caps.cap[0], "index local 'thresh'");
        let influence = ns(caps.cap[1], "index local 'influence'");
        Some(vec![r#mod(n!("EnemyCurseLimit"), BASE, 1.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4091 });
    v.push(PatternEntry {
        pat: "you can apply one fewer curse",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyCurseLimit"),
            BASE,
            -1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4092,
    });
    v.push(PatternEntry {
        pat: "curses on enemies in your chilling areas have (%d+)%% increased effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CurseEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "InChillingArea" })],
            )])
        }),
        lua_line: 4093,
    });
    v.push(PatternEntry {
        pat: "hexes you inflict have their effect increased by twice their doom instead",
        val: SpecialVal::Mods(vec![r#mod(n!("DoomEffect"), MORE, 100.0, None, 0, 0, &[])]),
        lua_line: 4094,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have an additional (%d+)%% chance to receive a critical strike",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("SelfExtraCritChance"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4095,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have (%-%d+)%% to all resistances",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("EnemyModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(n!("ElementalResist"), BASE, num, None, 0, 0, &[])),
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
                            Value::from(r#mod(n!("ChaosResist"), BASE, num, None, 0, 0, &[])),
                        )],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 4096,
    });
    v.push(PatternEntry { pat: "enemies ignited or chilled by you have (%-%d+)%% to elemental resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ElementalResist"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Ignited"), n!("Chilled")][..] })])])
    }), lua_line: 4100 });
    v.push(PatternEntry {
        pat: "reserves (%d+)%% of nearby enemy monsters' life",
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
                            n!("LifeReservationPercent"),
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
        lua_line: 4103,
    });
    v.push(PatternEntry {
        pat: "nearby enemy monsters have at least (%d+)%% of life reserved",
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
                            n!("LifeReservationPercent"),
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
        lua_line: 4104,
    });
    v.push(PatternEntry {
        pat: "your hits inflict decay, dealing (%d+) chaos damage per second for %d+ seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("decay")),
                        (n!("value"), Value::from(num)),
                        (n!("merge"), Value::from("MAX")),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4105,
    });
    // typo never existed except in some items generated by PoB
    v.push(PatternEntry { pat: "inflict decay on enemies you curse with hex or mark skills, dealing (%d+) chaos damage per second for %d+ seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("decay")), (n!("value"), Value::from(num)), (n!("merge"), Value::from("MAX"))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })])])
    }), lua_line: 4106 });
    v.push(PatternEntry { pat: "inflict decay on enemies you curse with hex skills, dealing (%d+) chaos damage per second for %d+ seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SkillData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("decay")), (n!("value"), Value::from(num)), (n!("merge"), Value::from("MAX"))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })])])
    }), lua_line: 4109 });
    v.push(PatternEntry {
        pat: "temporal chains has (%d+)%% reduced effect on you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CurseEffectOnSelf"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Temporal Chains" })],
            )])
        }),
        lua_line: 4112,
    });
    v.push(PatternEntry {
        pat: "unaffected by temporal chains",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Temporal Chains" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4113,
    });
    v.push(PatternEntry {
        pat: "targets are unaffected by your hexes",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Hex })],
        )]),
        lua_line: 4114,
    });
    v.push(PatternEntry {
        pat: "([%+%-][%d%.]+) seconds to cat's stealth duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PrimaryDuration"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Aspect of the Cat" })],
            )])
        }),
        lua_line: 4115,
    });
    v.push(PatternEntry {
        pat: "([%+%-][%d%.]+) seconds to cat's agility duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SecondaryDuration"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Aspect of the Cat" })],
            )])
        }),
        lua_line: 4116,
    });
    v.push(PatternEntry {
        pat: "([%+%-][%d%.]+) seconds to avian's might duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PrimaryDuration"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Aspect of the Avian" })],
            )])
        }),
        lua_line: 4117,
    });
    v.push(PatternEntry {
        pat: "([%+%-][%d%.]+) seconds to avian's flight duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SecondaryDuration"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Aspect of the Avian" })],
            )])
        }),
        lua_line: 4118,
    });
}

/// @pob Modules/ModParser.lua:4119-4195 h=08cc3f7e
fn specialModList_4119(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "aspect of the spider can inflict spider's web on enemies an additional time",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(
                        n!("Multiplier:SpiderWebApplyStackMax"),
                        BASE,
                        1.0,
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
            &[tag!({ type = SkillName, skillName = "Aspect of the Spider" })],
        )]),
        lua_line: 4119,
    });
    v.push(PatternEntry {
        pat: "aspect of the avian also grants avian's might and avian's flight to nearby allies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("BuffAppliesToAllies"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Aspect of the Avian" })],
        )]),
        lua_line: 4120,
    });
    v.push(PatternEntry {
        pat: "marked enemy takes (%d+)%% increased damage",
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
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })],
            )])
        }),
        lua_line: 4121,
    });
    v.push(PatternEntry {
        pat: "marked enemy has (%d+)%% reduced accuracy rating",
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
                            n!("Accuracy"),
                            INC,
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
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })],
            )])
        }),
        lua_line: 4124,
    });
    v.push(PatternEntry {
        pat: "you are cursed with level (%d+) (%D+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let name = ns(caps.cap[1], "index local 'name'");
            Some(vec![r#mod(
                n!("ExtraCurse"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("skillId"), Value::from(gemIdLookup(name))),
                        (n!("level"), Value::from(num)),
                        (n!("applyToPlayer"), Value::from(true)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4127,
    });
    v.push(PatternEntry {
        pat: "you are cursed with (%D+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            Some(vec![r#mod(
                n!("ExtraCurse"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("skillId"), Value::from(gemIdLookup(skill))),
                        (n!("level"), Value::from(1.0)),
                        (n!("applyToPlayer"), Value::from(true)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4128,
    });
    v.push(PatternEntry { pat: "you are cursed with (%D+), with (%d+)%% increased effect", val: SpecialVal::Fn(|caps: &Caps| {
        let skill = ns(caps.cap[0], "index local 'skill'");
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("ExtraCurse"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from(gemIdLookup(skill))), (n!("level"), Value::from(1.0)), (n!("applyToPlayer"), Value::from(true))]), None, 0, 0, &[]), r#mod(n!("CurseEffectOnSelf"), INC, tonumber(num), None, 0, 0, &[tag!({ type = SkillName, skillName = Name::from(&*sub(&*gsub_fn(&*cat!(" ", skill), "%W%l", |c| Some(upper(c[0].as_str())), None).0, 2, None).into_owned()) })])])
    }), lua_line: 4129 });
    v.push(PatternEntry {
        pat: "you count as on low life while you are cursed with vulnerability",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:LowLife"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByVulnerability" })],
        )]),
        lua_line: 4133,
    });
    v.push(PatternEntry {
        pat: "you count as on full life while you are cursed with vulnerability",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:FullLife"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByVulnerability" })],
        )]),
        lua_line: 4134,
    });
    v.push(PatternEntry { pat: "if you consumed a corpse recently, you and nearby allies regenerate (%d+)%% of life per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "ConsumedCorpseRecently" })])])
    }), lua_line: 4135 });
    v.push(PatternEntry { pat: "if you have blocked recently, you and nearby allies regenerate (%d+)%% of life per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "BlockedRecently" })])])
    }), lua_line: 4136 });
    v.push(PatternEntry {
        pat: "you are at maximum chance to block attack damage if you have not blocked recently",
        val: SpecialVal::Mods(vec![flag(
            n!("MaxBlockIfNotBlockedRecently"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BlockedRecently", neg = true })],
        )]),
        lua_line: 4137,
    });
    v.push(PatternEntry {
        pat: "you are at maximum chance to block spell damage if you have not blocked recently",
        val: SpecialVal::Mods(vec![flag(
            n!("MaxSpellBlockIfNotBlockedRecently"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BlockedRecently", neg = true })],
        )]),
        lua_line: 4138,
    });
    v.push(PatternEntry {
        pat: "%+(%d+)%% chance to block attack damage if you have not blocked recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BlockChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently", neg = true })],
            )])
        }),
        lua_line: 4139,
    });
    v.push(PatternEntry {
        pat: "%+(%d+)%% chance to block spell damage if you have not blocked recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SpellBlockChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently", neg = true })],
            )])
        }),
        lua_line: 4140,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to block is lucky",
        val: SpecialVal::Mods(vec![
            flag(n!("BlockChanceIsLucky"), None, 0, 0, &[]),
            flag(n!("ProjectileBlockChanceIsLucky"), None, 0, 0, &[]),
            flag(n!("SpellBlockChanceIsLucky"), None, 0, 0, &[]),
            flag(n!("SpellProjectileBlockChanceIsLucky"), None, 0, 0, &[]),
        ]),
        lua_line: 4141,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to block attack damage is lucky",
        val: SpecialVal::Mods(vec![
            flag(n!("BlockChanceIsLucky"), None, 0, 0, &[]),
            flag(n!("ProjectileBlockChanceIsLucky"), None, 0, 0, &[]),
        ]),
        lua_line: 4142,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to block attack damage is unlucky",
        val: SpecialVal::Mods(vec![
            flag(n!("BlockChanceIsUnlucky"), None, 0, 0, &[]),
            flag(n!("ProjectileBlockChanceIsUnlucky"), None, 0, 0, &[]),
        ]),
        lua_line: 4143,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to block is unlucky",
        val: SpecialVal::Mods(vec![
            flag(n!("BlockChanceIsUnlucky"), None, 0, 0, &[]),
            flag(n!("ProjectileBlockChanceIsUnlucky"), None, 0, 0, &[]),
            flag(n!("SpellBlockChanceIsUnlucky"), None, 0, 0, &[]),
            flag(n!("SpellProjectileBlockChanceIsUnlucky"), None, 0, 0, &[]),
        ]),
        lua_line: 4144,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to block spell damage is lucky",
        val: SpecialVal::Mods(vec![
            flag(n!("SpellBlockChanceIsLucky"), None, 0, 0, &[]),
            flag(n!("SpellProjectileBlockChanceIsLucky"), None, 0, 0, &[]),
        ]),
        lua_line: 4145,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?chance to block spell damage is unlucky",
        val: SpecialVal::Mods(vec![
            flag(n!("SpellBlockChanceIsUnlucky"), None, 0, 0, &[]),
            flag(n!("SpellProjectileBlockChanceIsUnlucky"), None, 0, 0, &[]),
        ]),
        lua_line: 4146,
    });
    v.push(PatternEntry {
        pat: "your lucky or unlucky effects use the best or worst from three rolls instead of two",
        val: SpecialVal::Mods(vec![flag(n!("ExtremeLuck"), None, 0, 0, &[])]),
        lua_line: 4147,
    });
    v.push(PatternEntry {
        pat: "chance to block attack or spell damage is lucky if you've blocked recently",
        val: SpecialVal::Mods(vec![
            flag(
                n!("BlockChanceIsLucky"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
            flag(
                n!("ProjectileBlockChanceIsLucky"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
            flag(
                n!("SpellBlockChanceIsLucky"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
            flag(
                n!("SpellProjectileBlockChanceIsLucky"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
        ]),
        lua_line: 4148,
    });
    v.push(PatternEntry {
        pat: "([%d%.]+)%% of evasion rating is regenerated as life per second while focus?sed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeRegen"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Evasion", percent = num }),
                    tag!({ type = Condition, var = "Focused" }),
                ],
            )])
        }),
        lua_line: 4154,
    });
    v.push(PatternEntry {
        pat: "nearby allies have (%d+)%% increased defences per (%d+) strength you have",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("onlyAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("Defences"), INC, num, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "Str", div = tonumber(div) })],
            )])
        }),
        lua_line: 4155,
    });
    v.push(PatternEntry { pat: "nearby allies have %+(%d+)%% to critical strike multiplier per (%d+) dexterity you have", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("onlyAllies"), Value::from(true)), (n!("mod"), Value::from(r#mod(n!("CritMultiplier"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = PerStat, stat = "Dex", div = tonumber(div) })])])
    }), lua_line: 4156 });
    v.push(PatternEntry {
        pat: "nearby allies have (%d+)%% increased cast speed per (%d+) intelligence you have",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("onlyAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("Speed"), INC, num, None, ModFlag::Cast, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "Int", div = tonumber(div) })],
            )])
        }),
        lua_line: 4157,
    });
    v.push(PatternEntry {
        pat: "quicksilver flasks you use also apply to nearby allies",
        val: SpecialVal::Mods(vec![flag(
            n!("QuickSilverAppliesToAllies"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4158,
    });
    v.push(PatternEntry {
        pat: "you gain divinity for %d+ seconds on reaching maximum divine charges",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ElementalDamage"),
                MORE,
                75.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Divinity" })],
            ),
            r#mod(
                n!("ElementalDamageTaken"),
                MORE,
                -25.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Divinity" })],
            ),
        ]),
        lua_line: 4159,
    });
    v.push(PatternEntry {
        pat: "your nearby party members maximum endurance charges is equal to yours",
        val: SpecialVal::Mods(vec![flag(
            n!("PartyMemberMaximumEnduranceChargesEqualToYours"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4163,
    });
    v.push(PatternEntry {
        pat: "your maximum endurance charges is equal to your maximum frenzy charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MaximumEnduranceChargesIsMaximumFrenzyCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4164,
    });
    v.push(PatternEntry {
        pat: "your maximum frenzy charges is equal to your maximum power charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MaximumFrenzyChargesIsMaximumPowerCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4165,
    });
    v.push(PatternEntry {
        pat: "your curse limit is equal to your maximum power charges",
        val: SpecialVal::Mods(vec![flag(
            n!("CurseLimitIsMaximumPowerCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4166,
    });
    v.push(PatternEntry { pat: "consecrated ground you create grants (%d+)%% increased accuracy rating to you and allies", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ConsecratedGroundAlsoAccuracy"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "OnConsecratedGround" })])))]), None, 0, 0, &[])])
    }), lua_line: 4167 });
    v.push(PatternEntry { pat: "consecrated ground created during effect applies (%d+)%% increased damage taken to enemies", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTakenConsecratedGround"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "OnConsecratedGround" })])))]), None, 0, 0, &[tag!({ type = Condition, var = "UsingFlask" })])])
    }), lua_line: 4168 });
    v.push(PatternEntry { pat: "consecrated ground you create while affected by zealotry causes enemies to take (%d+)%% increased damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTakenConsecratedGround"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnConsecratedGround" }), tag!({ type = Condition, var = "AffectedByZealotry" })])])
    }), lua_line: 4169 });
    v.push(PatternEntry { pat: "if you've warcried recently, you and nearby allies have (%d+)%% increased attack, cast and movement speed", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Speed"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "UsedWarcryRecently" })]), r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("MovementSpeed"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "UsedWarcryRecently" })])])
    }), lua_line: 4170 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased movement speed while on full life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MovementSpeed"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "FullLife" })],
            )])
        }),
        lua_line: 4174,
    });
    v.push(PatternEntry {
        pat: "when you warcry, you and nearby allies gain onslaught for 4 seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Onslaught"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsedWarcryRecently" })],
        )]),
        lua_line: 4175,
    });
    v.push(PatternEntry { pat: "warcries grant arcane surge to you and allies, with (%d+)%% increased effect per (%d+) power, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        let limit = ns(caps.cap[2], "index local 'limit'");
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:ArcaneSurge"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "UsedWarcryRecently" })]), r#mod(n!("ArcaneSurgeEffect"), INC, num, None, 0, 0, &[tag!({ type = PerStat, stat = "WarcryPower", div = tonumber(div), globalLimit = tonumber(limit), globalLimitKey = "Brinerot Flag" }), tag!({ type = Condition, var = "UsedWarcryRecently" })])])
    }), lua_line: 4176 });
    v.push(PatternEntry {
        pat: "gain arcane surge after spending a total of (%d+) mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:ArcaneSurge"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "ManaSpentRecently", threshold = num })],
            )])
        }),
        lua_line: 4180,
    });
    v.push(PatternEntry {
        pat: "gain arcane surge after spending a total of (%d+) life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:ArcaneSurge"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "LifeSpentRecently", threshold = num })],
            )])
        }),
        lua_line: 4183,
    });
    v.push(PatternEntry { pat: "gain onslaught for (%d+) seconds on hit while at maximum frenzy charges", val: SpecialVal::Mods(vec![flag(n!("Onslaught"), None, 0, 0, &[tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" }), tag!({ type = Condition, var = "HitRecently" })])]), lua_line: 4186 });
    v.push(PatternEntry {
        pat: "enemies in your chilling areas take (%d+)%% increased lightning damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("LightningDamageTaken"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "InChillingArea" })],
            )])
        }),
        lua_line: 4187,
    });
    v.push(PatternEntry {
        pat: "warcries count as having (%d+) additional nearby enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:WarcryNearbyEnemies"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4188,
    });
    v.push(PatternEntry {
        pat: "enemies taunted by your warcries take (%d+)%% increased damage",
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
                            n!("DamageTaken"),
                            INC,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "Taunted" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsedWarcryRecently" })],
            )])
        }),
        lua_line: 4191,
    });
    v.push(PatternEntry {
        pat: "warcries have minimum of (%d+) power",
        val: SpecialVal::Mods(vec![flag(n!("CryWolfMinimumPower"), None, 0, 0, &[])]),
        lua_line: 4192,
    });
    v.push(PatternEntry {
        pat: "warcries have infinite power",
        val: SpecialVal::Mods(vec![flag(n!("WarcryInfinitePower"), None, 0, 0, &[])]),
        lua_line: 4193,
    });
    v.push(PatternEntry {
        pat: "your warcries do not grant buffs or charges to you",
        val: SpecialVal::Mods(vec![flag(n!("CannotGainWarcryBuffs"), None, 0, 0, &[])]),
        lua_line: 4194,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict corrosion on hit with attacks",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanCorrode"), None, 0, 0, &[])]),
        lua_line: 4195,
    });
}

/// @pob Modules/ModParser.lua:4196-4315 h=b167ce06
fn specialModList_4196(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict withered for (%d+) seconds on hit",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 4196,
    });
    v.push(PatternEntry {
        pat: "melee weapon hits inflict (%d+) withered debuffs for (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 4197,
    });
    v.push(PatternEntry { pat: "inflict withered for (%d+) seconds on hit if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let curse = ns(caps.cap[1], "index local 'curse'");
        Some(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 4198 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict withered for (%d+) seconds on hit with this weapon",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 4199,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict withered for (%d+) seconds on hit against cursed enemies",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanWither"),
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
        )]),
        lua_line: 4200,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to inflict withered for two seconds on hit if there are (%d+) or fewer withered debuffs on enemy", val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]), lua_line: 4201 });
    v.push(PatternEntry {
        pat: "inflict withered for (%d+) seconds on hit with this weapon",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 4202,
    });
    v.push(PatternEntry {
        pat: "chaos skills inflict up to (%d+) withered debuffs on hit for (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 4203,
    });
    v.push(PatternEntry {
        pat: "minions have (%d+)%% chance to inflict withered on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:CanWither"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4204,
    });
    v.push(PatternEntry { pat: "enemies take (%d+)%% increased elemental damage from your hits for each withered you have inflicted on them", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ElementalDamageTaken"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "WitheredStack", limit = 15.0 })])))]), None, 0, 0, &[])])
    }), lua_line: 4205 });
    v.push(PatternEntry {
        pat: "your hits cannot penetrate or ignore elemental resistances",
        val: SpecialVal::Mods(vec![flag(n!("CannotElePenIgnore"), None, 0, 0, &[])]),
        lua_line: 4206,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have malediction",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("HasMalediction"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4207,
    });
    v.push(PatternEntry {
        pat: "gain shaper's presence for 10 seconds when you kill a rare or unique enemy",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("HasShapersPresence"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "KilledUniqueEnemy" })],
        )]),
        lua_line: 4208,
    });
    v.push(PatternEntry {
        pat: "gain maddening presence for 10 seconds when you kill a rare or unique enemy",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("HasMaddeningPresence"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "KilledUniqueEnemy" })],
        )]),
        lua_line: 4209,
    });
    v.push(PatternEntry { pat: "elemental damage you deal with hits is resisted by lowest elemental resistance instead", val: SpecialVal::Mods(vec![flag(n!("ElementalDamageUsesLowestResistance"), None, 0, 0, &[])]), lua_line: 4210 });
    v.push(PatternEntry {
        pat: "you take (%d+) chaos damage per second for 3 seconds on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChaosDegen"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "KilledLast3Seconds" })],
            )])
        }),
        lua_line: 4211,
    });
    v.push(PatternEntry { pat: "regenerate (%d+) life per second for each (%d+)%% uncapped fire resistance", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let percent = ns(caps.cap[1], "index local 'percent'");
        Some(vec![r#mod(n!("LifeRegen"), BASE, num, None, 0, 0, &[tag!({ type = PerStat, stat = "FireResistTotal", div = (1.0 / snum(percent, "perform arithmetic on a string")) })])])
    }), lua_line: 4212 });
    v.push(PatternEntry {
        pat: "regenerate (%d+) life over 1 second for each spell you cast",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeRegen"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CastLast1Seconds" })],
            )])
        }),
        lua_line: 4213,
    });
    v.push(PatternEntry {
        pat: "and nearby allies regenerate (%d+) life per second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("LifeRegen"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "KilledPoisonedLast2Seconds" })],
            )])
        }),
        lua_line: 4214,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased life regeneration rate",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("LifeRegen"), INC, num, None, 0, 0, &[])])
        }),
        lua_line: 4215,
    });
    // LUA-QUIRK: tag.percent holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber
    v.push(PatternEntry { pat: "every (%d+) seconds, regenerate life equal to (%d+)%% of armour and evasion rating over (%d+) second", val: SpecialVal::Fn(|caps: &Caps| {
        let interval = caps.num;
        let percent = ns(caps.cap[1], "index local 'percent'");
        let duration = ns(caps.cap[2], "index local 'duration'");
        Some(vec![r#mod(n!("LifeRegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstFull" }), tag!({ type = PercentStat, stat = "Armour", percent = snum(percent, "perform arithmetic on tag.percent") })]), r#mod(n!("LifeRegen"), BASE, ((1.0 / nn(interval, "perform arithmetic on local 'interval'")) * snum(duration, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstAvg" }), tag!({ type = PercentStat, stat = "Armour", percent = snum(percent, "perform arithmetic on tag.percent") })]), r#mod(n!("LifeRegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstFull" }), tag!({ type = PercentStat, stat = "Evasion", percent = snum(percent, "perform arithmetic on tag.percent") })]), r#mod(n!("LifeRegen"), BASE, ((1.0 / nn(interval, "perform arithmetic on local 'interval'")) * snum(duration, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstAvg" }), tag!({ type = PercentStat, stat = "Evasion", percent = snum(percent, "perform arithmetic on tag.percent") })])])
    }), lua_line: 4216 });
    // LUA-QUIRK: tag.percent holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber
    v.push(PatternEntry { pat: "every (%d+) seconds, regenerate energy shield equal to (%d+)%% of evasion rating over (%d+) second", val: SpecialVal::Fn(|caps: &Caps| {
        let interval = caps.num;
        let percent = ns(caps.cap[1], "index local 'percent'");
        let duration = ns(caps.cap[2], "index local 'duration'");
        Some(vec![r#mod(n!("EnergyShieldRegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstFull" }), tag!({ type = PercentStat, stat = "Evasion", percent = snum(percent, "perform arithmetic on tag.percent") })]), r#mod(n!("EnergyShieldRegen"), BASE, ((1.0 / nn(interval, "perform arithmetic on local 'interval'")) * snum(duration, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstAvg" }), tag!({ type = PercentStat, stat = "Evasion", percent = snum(percent, "perform arithmetic on tag.percent") })])])
    }), lua_line: 4222 });
    v.push(PatternEntry {
        pat: "regenerate (%d+)%% of life per second for each different ailment affecting you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Bleeding" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Ignited" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Scorched" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Chilled" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Frozen" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Brittle" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Shocked" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Sapped" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Poisoned" })],
                ),
            ])
        }),
        lua_line: 4226,
    });
    v.push(PatternEntry {
        pat: "fire skills have a (%d+)%% chance to apply fire exposure on hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FireExposureChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4237,
    });
    v.push(PatternEntry {
        pat: "cold skills have a (%d+)%% chance to apply cold exposure on hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ColdExposureChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4238,
    });
    v.push(PatternEntry {
        pat: "lightning skills have a (%d+)%% chance to apply lightning exposure on hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LightningExposureChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4239,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to inflict cold exposure on hit with cold damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ColdExposureChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4240,
    });
    v.push(PatternEntry {
        pat: "socketed skills apply fire, cold and lightning exposure on hit",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("FireExposureChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
            r#mod(
                n!("ColdExposureChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
            r#mod(
                n!("LightningExposureChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
        ]),
        lua_line: 4241,
    });
    v.push(PatternEntry {
        pat: "inflict fire, cold, and lightning exposure on hit",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("FireExposureChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
            r#mod(
                n!("ColdExposureChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
            r#mod(
                n!("LightningExposureChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
        ]),
        lua_line: 4246,
    });
    v.push(PatternEntry {
        pat: "inflict fire exposure on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("FireExposureChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        )]),
        lua_line: 4251,
    });
    v.push(PatternEntry {
        pat: "inflict cold exposure on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ColdExposureChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        )]),
        lua_line: 4254,
    });
    v.push(PatternEntry {
        pat: "inflict lightning exposure on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("LightningExposureChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        )]),
        lua_line: 4257,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have fire exposure",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("FireExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        )]),
        lua_line: 4260,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have cold exposure",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("ColdExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        )]),
        lua_line: 4263,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have lightning exposure",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("LightningExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Effective" })],
        )]),
        lua_line: 4266,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have fire exposure while at maximum rage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("FireExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "Effective" }),
                tag!({ type = Condition, var = "HaveMaximumRage" }),
            ],
        )]),
        lua_line: 4269,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have fire exposure while you are affected by herald of ash",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("FireExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "Effective" }),
                tag!({ type = Condition, var = "AffectedByHeraldofAsh" }),
            ],
        )]),
        lua_line: 4272,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have cold exposure while you are affected by herald of ice",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("ColdExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "Effective" }),
                tag!({ type = Condition, var = "AffectedByHeraldofIce" }),
            ],
        )]),
        lua_line: 4275,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have lightning exposure while you are affected by herald of thunder",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("LightningExposure"), BASE, -10.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "Effective" }),
                tag!({ type = Condition, var = "AffectedByHeraldofThunder" }),
            ],
        )]),
        lua_line: 4278,
    });
    v.push(PatternEntry { pat: "(%a+) exposure on hit if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let exposureType = ns(caps.cap[0], "index local 'exposureType'");
        let curse = ns(caps.cap[1], "index local 'curse'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(firstToUpper(exposureType), "Exposure"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*firstToUpper(curse), " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) }), tag!({ type = Condition, var = "Effective" })])])
    }), lua_line: 4281 });
    v.push(PatternEntry { pat: "inflict (%a+) exposure on hit if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let exposureType = ns(caps.cap[0], "index local 'exposureType'");
        let curse = ns(caps.cap[1], "index local 'curse'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(firstToUpper(exposureType), "Exposure"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*firstToUpper(curse), " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) }), tag!({ type = Condition, var = "Effective" })])])
    }), lua_line: 4284 });
    v.push(PatternEntry {
        pat: "inflict fire, cold and lightning exposure on nearby enemies when used",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("FireExposure"), BASE, -10.0, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "Effective" }),
                    tag!({ type = Condition, var = "UsingFlask" }),
                ],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ColdExposure"), BASE, -10.0, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "Effective" }),
                    tag!({ type = Condition, var = "UsingFlask" }),
                ],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("LightningExposure"), BASE, -10.0, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "Effective" }),
                    tag!({ type = Condition, var = "UsingFlask" }),
                ],
            ),
        ]),
        lua_line: 4287,
    });
    v.push(PatternEntry {
        pat: "enemies near your linked targets have fire, cold and lightning exposure",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("FireExposure"),
                            BASE,
                            -10.0,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "NearLinkedTarget" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("ColdExposure"),
                            BASE,
                            -10.0,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "NearLinkedTarget" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("LightningExposure"),
                            BASE,
                            -10.0,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "NearLinkedTarget" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Effective" })],
            ),
        ]),
        lua_line: 4292,
    });
    v.push(PatternEntry {
        pat: "inflict (%w+) exposure on hit, applying %-(%d+)%% to (%w+) resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let element1 = ns(caps.cap[0], "index local 'element1'");
            let num = ns(caps.cap[1], "index local 'num'");
            let element2 = ns(caps.cap[2], "index local 'element2'");
            Some(vec![
                r#mod(
                    cat!(firstToUpper(element1), "ExposureChance"),
                    BASE,
                    100.0,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Effective" })],
                ),
                r#mod(
                    n!("EnemyModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(
                                cat!(firstToUpper(element2), "Exposure"),
                                BASE,
                                (-snum(num, "perform arithmetic on a string")),
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
                    &[tag!({ type = Condition, var = "Effective" })],
                ),
            ])
        }),
        lua_line: 4297,
    });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, inflict (%w+) exposure on hit, applying %-(%d+)%% to (%w+) resistance", val: SpecialVal::Fn(|caps: &Caps| {
        let element1 = ns(caps.cap[0], "index local 'element1'");
        let num = ns(caps.cap[1], "index local 'num'");
        let element2 = ns(caps.cap[2], "index local 'element2'");
        Some(vec![r#mod(cat!(firstToUpper(element1), "ExposureChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" }), tag!({ type = Condition, var = "Effective" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(firstToUpper(element2), "Exposure"), BASE, (-snum(num, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = Condition, var = "RareOrUnique" })])))]), None, 0, 0, &[tag!({ type = Condition, var = "Effective" })])])
    }), lua_line: 4301 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, inflict (%w+) exposure on hit, applying %-(%d+)%% to (%w+) resistance", val: SpecialVal::Fn(|caps: &Caps| {
        let element1 = ns(caps.cap[0], "index local 'element1'");
        let num = ns(caps.cap[1], "index local 'num'");
        let element2 = ns(caps.cap[2], "index local 'element2'");
        Some(vec![r#mod(cat!(firstToUpper(element1), "ExposureChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" }), tag!({ type = Condition, var = "Effective" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(firstToUpper(element2), "Exposure"), BASE, (-snum(num, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = Condition, var = "PinnacleBoss" })])))]), None, 0, 0, &[tag!({ type = Condition, var = "Effective" })])])
    }), lua_line: 4305 });
    v.push(PatternEntry { pat: "inflict fire exposure on hit against enemies with (%d+) cinderflame, applying %-(%d+)%% to (%w+) resistance", val: SpecialVal::Fn(|caps: &Caps| {
        let cinderflame = caps.num;
        let num = ns(caps.cap[1], "index local 'num'");
        let element = ns(caps.cap[2], "index local 'element'");
        Some(vec![r#mod(n!("FireExposureChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = Condition, var = "Effective" }), tag!({ type = MultiplierThreshold, var = "CinderflameStacks", threshold = cinderflame })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(firstToUpper(element), "Exposure"), BASE, (-snum(num, "perform arithmetic on a string")), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Effective" }), tag!({ type = MultiplierThreshold, var = "CinderflameStacks", threshold = cinderflame })])])
    }), lua_line: 4309 });
    v.push(PatternEntry {
        pat: "fire exposure you inflict applies an extra (%-?%d+)%% to fire resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraFireExposure"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4313,
    });
    v.push(PatternEntry {
        pat: "cold exposure you inflict applies an extra (%-?%d+)%% to cold resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraColdExposure"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4314,
    });
    v.push(PatternEntry {
        pat: "lightning exposure you inflict applies an extra (%-?%d+)%% to lightning resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraLightningExposure"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4315,
    });
}

/// @pob Modules/ModParser.lua:4316-4372 h=c819aa99
fn specialModList_4316(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "exposure you inflict applies at least (%-%d+)%% to the affected resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExposureMin"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4316,
    });
    v.push(PatternEntry {
        pat: "modifiers to minimum endurance charges instead apply to minimum brutal charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MinimumEnduranceChargesEqualsMinimumBrutalCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4317,
    });
    v.push(PatternEntry {
        pat: "modifiers to minimum frenzy charges instead apply to minimum affliction charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MinimumFrenzyChargesEqualsMinimumAfflictionCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4318,
    });
    v.push(PatternEntry {
        pat: "modifiers to minimum power charges instead apply to minimum absorption charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MinimumPowerChargesEqualsMinimumAbsorptionCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4319,
    });
    v.push(PatternEntry {
        pat: "maximum brutal charges is equal to maximum endurance charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MaximumEnduranceChargesEqualsMaximumBrutalCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4320,
    });
    v.push(PatternEntry {
        pat: "maximum affliction charges is equal to maximum frenzy charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MaximumFrenzyChargesEqualsMaximumAfflictionCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4321,
    });
    v.push(PatternEntry {
        pat: "maximum absorption charges is equal to maximum power charges",
        val: SpecialVal::Mods(vec![flag(
            n!("MaximumPowerChargesEqualsMaximumAbsorptionCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4322,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain a brine charge instead of an endurance charge",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("CanGainBrineCharges"), None, 0, 0, &[]),
                r#mod(n!("BrineChargeGainChance"), BASE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 4323,
    });
    v.push(PatternEntry {
        pat: "gain brutal charges instead of endurance charges",
        val: SpecialVal::Mods(vec![flag(
            n!("EnduranceChargesConvertToBrutalCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4324,
    });
    v.push(PatternEntry {
        pat: "gain affliction charges instead of frenzy charges",
        val: SpecialVal::Mods(vec![flag(
            n!("FrenzyChargesConvertToAfflictionCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4325,
    });
    v.push(PatternEntry {
        pat: "gain absorption charges instead of power charges",
        val: SpecialVal::Mods(vec![flag(
            n!("PowerChargesConvertToAbsorptionCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4326,
    });
    v.push(PatternEntry {
        pat: "regenerate (%d+)%% life over one second when hit while sane",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeRegenPercent"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "Insane", neg = true }),
                    tag!({ type = Condition, var = "BeenHitRecently" }),
                ],
            )])
        }),
        lua_line: 4327,
    });
    v.push(PatternEntry {
        pat: "you count as on low (%a+) while at (%d+)%% of maximum (%a+) or below",
        val: SpecialVal::Fn(|caps: &Caps| {
            let resourceType = ns(caps.cap[0], "index local 'resourceType'");
            let numStr = ns(caps.cap[1], "index local 'numStr'");
            Some(vec![r#mod(
                cat!(
                    "Low",
                    gsub_fn(resourceType, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                    "Percentage"
                ),
                BASE,
                (nn(tonumber(numStr), "perform arithmetic on a nil value") / 100.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4330,
    });
    v.push(PatternEntry {
        pat: "you count as on full (%a+) while at (%d+)%% of maximum (%a+) or above",
        val: SpecialVal::Fn(|caps: &Caps| {
            let resourceType = ns(caps.cap[0], "index local 'resourceType'");
            let numStr = ns(caps.cap[1], "index local 'numStr'");
            Some(vec![r#mod(
                cat!(
                    "Full",
                    gsub_fn(resourceType, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                    "Percentage"
                ),
                BASE,
                (nn(tonumber(numStr), "perform arithmetic on a nil value") / 100.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4331,
    });
    v.push(PatternEntry { pat: "(%d+)%% more maximum life if you have at least (%d+) life masteries allocated", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let thresh = ns(caps.cap[1], "index local 'thresh'");
        Some(vec![r#mod(n!("Life"), MORE, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "AllocatedLifeMastery", threshold = tonumber(thresh) })])])
    }), lua_line: 4332 });
    v.push(PatternEntry {
        pat: "left ring slot: cover enemies in ash for (%d+) seconds when you ignite them",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CoveredInAshEffect"),
            BASE,
            20.0,
            None,
            0,
            0,
            &[
                tag!({ type = SlotNumber, num = 1.0 }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" }),
            ],
        )]),
        lua_line: 4335,
    });
    v.push(PatternEntry {
        pat: "right ring slot: cover enemies in frost for (%d+) seconds when you freeze them",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CoveredInFrostEffect"),
            BASE,
            20.0,
            None,
            0,
            0,
            &[
                tag!({ type = SlotNumber, num = 2.0 }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" }),
            ],
        )]),
        lua_line: 4336,
    });
    v.push(PatternEntry {
        pat: "nearby enemies are covered in ash",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CoveredInAshEffect"),
            BASE,
            20.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4337,
    });
    v.push(PatternEntry { pat: "nearby enemies are covered in ash if you haven't moved in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CoveredInAshEffect"), BASE, 20.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "StationarySeconds", threshold = num }), tag!({ type = Condition, var = "Stationary" })])])
    }), lua_line: 4338 });
    v.push(PatternEntry {
        pat: "your warcries cover enemies in ash for (%d+) seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CoveredInAshEffect"),
            BASE,
            20.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsedWarcryRecently" })],
        )]),
        lua_line: 4339,
    });
    v.push(PatternEntry { pat: "enemies near targets you shatter have (%d+)%% chance to be covered in frost for (%d+) seconds", val: SpecialVal::Mods(vec![r#mod(n!("CoveredInFrostEffect"), BASE, 20.0, None, 0, 0, &[tag!({ type = Condition, var = "ShatteredEnemyRecently" })])]), lua_line: 4340 });
    v.push(PatternEntry {
        pat: "([%a%s]+) has (%d+)%% increased effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let skill = ns(caps.cap[0], "index local 'skill'");
            let num = ns(caps.cap[1], "index local 'num'");
            Some(vec![r#mod(
                n!("BuffEffect"),
                INC,
                &*num,
                None,
                0,
                0,
                &[tag!({ type = SkillId, skillId = gemIdLookup(skill) })],
            )])
        }),
        lua_line: 4341,
    });
    v.push(PatternEntry {
        pat: "debuffs on you expire (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SelfDebuffExpirationRate"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4342,
    });
    v.push(PatternEntry {
        pat: "debuffs on you expire (%d+)%% slower",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SelfDebuffExpirationRate"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4343,
    });
    v.push(PatternEntry {
        pat: "debuffs on you expire (%d+)%% faster while affected by haste",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SelfDebuffExpirationRate"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByHaste" })],
            )])
        }),
        lua_line: 4344,
    });
    v.push(PatternEntry {
        pat: "warcries debilitate enemies for (%d+) seconds?",
        val: SpecialVal::Mods(vec![r#mod(
            n!("DebilitateChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4345,
    });
    v.push(PatternEntry {
        pat: "debilitate enemies for (%d+) seconds? when you suppress their spell damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("DebilitateChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4346,
    });
    v.push(PatternEntry {
        pat: "debilitate nearby enemies for (%d+) seconds? when f?l?a?s?k? ?effect ends",
        val: SpecialVal::Mods(vec![r#mod(
            n!("DebilitateChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4347,
    });
    v.push(PatternEntry {
        pat: "counterattacks have a (%d+)%% chance to debilitate on hit for (%d+) seconds?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DebilitateChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4348,
    });
    v.push(PatternEntry {
        pat: "retaliation skills debilitate enemies for (%d+) seconds on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("DebilitateChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4349,
    });
    v.push(PatternEntry {
        pat: "eat a soul when you hit a unique enemy, no more than once every second",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveSoulEater"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4350,
    });
    v.push(PatternEntry { pat: "eat a soul when you hit a rare or unique enemy, no more than once every [%d%.]+ seconds", val: SpecialVal::Mods(vec![flag(n!("Condition:CanHaveSoulEater"), None, 0, 0, &[])]), lua_line: 4351 });
    v.push(PatternEntry { pat: "(%d+)%% chance to gain soul eater for (%d+) seconds on killing blow against rare and unique enemies with double strike or dual strike", val: SpecialVal::Mods(vec![flag(n!("Condition:CanHaveSoulEater"), None, 0, 0, &[])]), lua_line: 4352 });
    v.push(PatternEntry { pat: "when (%d+)%% of your hex's duration expires on an enemy, eat (%d+) soul per enemy power", val: SpecialVal::Mods(vec![flag(n!("Condition:CanHaveSoulEater"), None, 0, 0, &[])]), lua_line: 4353 });
    v.push(PatternEntry {
        pat: "eat (%d+) souls when you kill a rare or unique enemy with this weapon",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveSoulEater"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4354,
    });
    v.push(PatternEntry {
        pat: "maximum (%d+) eaten souls",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SoulEaterMax"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4355,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d+) to maximum number of eaten souls",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("SoulEaterMax"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 4356,
    });
    // This boot enchant gives a buff that applies both stats individually
    v.push(PatternEntry {
        pat: "(%d+)%% increased attack and cast speed if you've killed recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Speed"),
                    INC,
                    num,
                    None,
                    ModFlag::Cast,
                    0,
                    &[tag!({ type = Condition, var = "KilledRecently" })],
                ),
                r#mod(
                    n!("Speed"),
                    INC,
                    num,
                    None,
                    ModFlag::Attack,
                    0,
                    &[tag!({ type = Condition, var = "KilledRecently" })],
                ),
            ])
        }),
        lua_line: 4357,
    });
    v.push(PatternEntry {
        pat: "gain adrenaline for 1 second when you change stance",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:Adrenaline"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "StanceChangeLastSecond" })],
        )]),
        lua_line: 4361,
    });
    v.push(PatternEntry { pat: "with a searching eye jewel socketed, maim enemies for (%d) seconds on hit with attacks", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Maimed"), None, ModFlag::Attack, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "HaveSearchingEyeJewelIn{SlotName}" })])]), lua_line: 4362 });
    v.push(PatternEntry { pat: "with a searching eye jewel socketed, blind enemies for (%d) seconds on hit with attacks", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Blinded"), None, ModFlag::Attack, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "HaveSearchingEyeJewelIn{SlotName}" })])]), lua_line: 4363 });
    v.push(PatternEntry {
        pat: "enemies maimed by you take (%d+)%% increased damage over time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("DamageTakenOverTime"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Maimed" })],
            )])
        }),
        lua_line: 4364,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased defences while you have at least four linked targets",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Defences"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "LinkedTargets", threshold = 4.0 })],
            )])
        }),
        lua_line: 4365,
    });
    v.push(PatternEntry {
        pat: "your movement speed is equal to the highest movement speed among linked players",
        val: SpecialVal::Mods(vec![flag(
            n!("MovementSpeedEqualHighestLinkedPlayers"),
            None,
            0,
            0,
            &[tag!({ type = MultiplierThreshold, var = "LinkedTargets", threshold = 1.0 })],
        )]),
        lua_line: 4366,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased movement speed while you have at least two linked targets",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MovementSpeed"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "LinkedTargets", threshold = 2.0 })],
            )])
        }),
        lua_line: 4367,
    });
    v.push(PatternEntry { pat: "link skills have (%d+)%% increased buff effect if you have linked to a target recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("BuffEffect"), INC, num, None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Link }), tag!({ type = Condition, var = "LinkedRecently" })])])
    }), lua_line: 4368 });
    v.push(PatternEntry {
        pat: "link skills can target damageable minions",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanLinkToMinions"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "HaveDamageableMinion" })],
        )]),
        lua_line: 4369,
    });
    v.push(PatternEntry {
        pat: "your linked minions take (%d+)%% less damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("DamageTaken"),
                            MORE,
                            (-nn(num, "perform arithmetic on local 'num'")),
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "AffectedByLink" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4370,
    });
    v.push(PatternEntry {
        pat: "curses are inflicted on you instead of linked targets",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraLinkEffect"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("CurseImmune"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4371,
    });
    v.push(PatternEntry {
        pat: "elemental ailments are inflicted on you instead of linked targets",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraLinkEffect"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("ElementalAilmentImmune"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4372,
    });
}

/// @pob Modules/ModParser.lua:4373-4431 h=d5cdf061
fn specialModList_4373(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "non%-unique utility flasks you use apply to linked targets",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraLinkEffect"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(
                        n!("ParentNonUniqueFlasksAppliedToYou"),
                        FLAG,
                        true,
                        None,
                        0,
                        0,
                        &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4373,
    });
    v.push(PatternEntry {
        pat: "non%-curse auras from your skills only apply to you and linked targets",
        val: SpecialVal::Mods(vec![flag(
            n!("SelfAurasAffectYouAndLinkedTarget"),
            None,
            0,
            0,
            &[
                tag!({ type = SkillType, skillType = SkillType::Aura }),
                tag!({ type = SkillType, skillType = SkillType::AppliesCurse, neg = true }),
            ],
        )]),
        lua_line: 4374,
    });
    v.push(PatternEntry {
        pat: "linked targets always count as in range of non%-curse auras from your skills",
        val: SpecialVal::Mods(vec![]),
        lua_line: 4375,
    });
    v.push(PatternEntry {
        pat: "gain unholy might on block for (%d) seconds",
        val: SpecialVal::Mods(vec![
            flag(
                n!("Condition:UnholyMight"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
            flag(
                n!("Condition:CanWither"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BlockedRecently" })],
            ),
        ]),
        lua_line: 4376,
    });
    v.push(PatternEntry {
        pat: "your warcries inflict hallowing flame",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanInflictHallowingFlame"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsedWarcryRecently" })],
        )]),
        lua_line: 4377,
    });
    v.push(PatternEntry {
        pat: "attacks with this weapon inflict hallowing flame on hit",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanInflictHallowingFlame"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4378,
    });
    v.push(PatternEntry {
        pat: "inflict hallowing flame on hit while on consecrated ground",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanInflictHallowingFlame"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "OnConsecratedGround" })],
        )]),
        lua_line: 4379,
    });
    v.push(PatternEntry {
        pat: "inflict hallowing flame on melee hit",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanInflictHallowingFlame"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4380,
    });
    // Traps, Mines
    v.push(PatternEntry {
        pat: "traps and mines deal (%d+)%-(%d+) additional physical damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let min = ns(caps.cap[0], "index local 'min'");
            let max = ns(caps.cap[1], "index local 'max'");
            Some(vec![
                r#mod(
                    n!("PhysicalMin"),
                    BASE,
                    tonumber(min),
                    None,
                    0,
                    bor(KeywordFlag::Trap, KeywordFlag::Mine),
                    &[],
                ),
                r#mod(
                    n!("PhysicalMax"),
                    BASE,
                    tonumber(max),
                    None,
                    0,
                    bor(KeywordFlag::Trap, KeywordFlag::Mine),
                    &[],
                ),
            ])
        }),
        lua_line: 4382,
    });
    v.push(PatternEntry {
        pat: "traps and mines deal (%d+) to (%d+) additional physical damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let min = ns(caps.cap[0], "index local 'min'");
            let max = ns(caps.cap[1], "index local 'max'");
            Some(vec![
                r#mod(
                    n!("PhysicalMin"),
                    BASE,
                    tonumber(min),
                    None,
                    0,
                    bor(KeywordFlag::Trap, KeywordFlag::Mine),
                    &[],
                ),
                r#mod(
                    n!("PhysicalMax"),
                    BASE,
                    tonumber(max),
                    None,
                    0,
                    bor(KeywordFlag::Trap, KeywordFlag::Mine),
                    &[],
                ),
            ])
        }),
        lua_line: 4383,
    });
    v.push(PatternEntry { pat: "each mine applies (%d+)%% increased damage taken to enemies near it, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "ActiveMineCount", limit = (snum(limit, "perform arithmetic on a string") / nn(num, "perform arithmetic on local 'num'")) })])))]), None, 0, 0, &[])])
    }), lua_line: 4384 });
    v.push(PatternEntry { pat: "each mine applies (%d+)%% reduced damage dealt to enemies near it, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = Multiplier, var = "ActiveMineCount", limit = (snum(limit, "perform arithmetic on a string") / nn(num, "perform arithmetic on local 'num'")) })])))]), None, 0, 0, &[])])
    }), lua_line: 4385 });
    v.push(PatternEntry { pat: "stormblast, icicle and pyroclast mine have (%d+)%% increased aura effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AuraEffect"), INC, num, None, 0, KeywordFlag::Mine, &[tag!({ type = SkillName, skillNameList = &[n!("Stormblast Mine"), n!("Icicle Mine"), n!("Pyroclast Mine")][..], includeTransfigured = true })])])
    }), lua_line: 4386 });
    v.push(PatternEntry { pat: "stormblast, icicle and pyroclast mine deal no damage", val: SpecialVal::Mods(vec![flag(n!("DealNoDamage"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Stormblast Mine"), n!("Icicle Mine"), n!("Pyroclast Mine")][..], includeTransfigured = true })])]), lua_line: 4389 });
    v.push(PatternEntry {
        pat: "can have up to (%d+) additional traps? placed at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveTrapLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4390,
    });
    v.push(PatternEntry {
        pat: "can have (%d+) fewer traps placed at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveTrapLimit"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4391,
    });
    v.push(PatternEntry {
        pat: "can have up to (%d+) additional remote mines? placed at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveMineLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4392,
    });
    // Additional trap & mine throw
    v.push(PatternEntry {
        pat: "throw an additional trap",
        val: SpecialVal::Mods(vec![r#mod(
            n!("TrapThrowCount"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4394,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to throw up to (%d+) additional traps?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let num = ns(caps.cap[1], "index local 'num'");
            Some(vec![r#mod(
                n!("TrapThrowCount"),
                BASE,
                ((nn(tonumber(num), "perform arithmetic on a nil value")
                    * nn(chance, "perform arithmetic on local 'chance'"))
                    / 100.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4395,
    });
    v.push(PatternEntry {
        pat: "throw an additional mine",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MineThrowCount"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4396,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to throw up to (%d+) additional mines?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let num = ns(caps.cap[1], "index local 'num'");
            Some(vec![r#mod(
                n!("MineThrowCount"),
                BASE,
                ((nn(tonumber(num), "perform arithmetic on a nil value")
                    * nn(chance, "perform arithmetic on local 'chance'"))
                    / 100.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4397,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to throw up to (%d+) additional traps? or mines?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let num = ns(caps.cap[1], "index local 'num'");
            Some(vec![
                r#mod(
                    n!("TrapThrowCount"),
                    BASE,
                    ((nn(tonumber(num), "perform arithmetic on a nil value")
                        * nn(chance, "perform arithmetic on local 'chance'"))
                        / 100.0),
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("MineThrowCount"),
                    BASE,
                    ((nn(tonumber(num), "perform arithmetic on a nil value")
                        * nn(chance, "perform arithmetic on local 'chance'"))
                        / 100.0),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 4398,
    });
    v.push(PatternEntry {
        pat: "skills which throw traps throw up to (%d+) additional traps?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TrapThrowCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4402,
    });
    // Totems
    v.push(PatternEntry {
        pat: "can have up to (%d+) additional totems? summoned at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveTotemLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4404,
    });
    v.push(PatternEntry {
        pat: "attack skills can have (%d+) additional totems? summoned at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveTotemLimit"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Attack,
                &[],
            )])
        }),
        lua_line: 4405,
    });
    v.push(PatternEntry { pat: "can [hs][au][vm][em]o?n? 1 additional siege ballista totem per (%d+) dexterity", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ActiveBallistaLimit"), BASE, 1.0, None, 0, 0, &[tag!({ type = SkillName, skillName = "Siege Ballista", includeTransfigured = true }), tag!({ type = PerStat, stat = "Dex", div = num })])])
    }), lua_line: 4406 });
    v.push(PatternEntry {
        pat: "totems fire (%d+) additional projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Totem,
                &[],
            )])
        }),
        lua_line: 4407,
    });
    v.push(PatternEntry {
        pat: "([%d%.]+)%% of damage dealt by y?o?u?r? ?totems is leeched to you as life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DamageLifeLeechToPlayer"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Totem,
                &[],
            )])
        }),
        lua_line: 4408,
    });
    v.push(PatternEntry {
        pat: "([%d%.]+)%% of damage dealt by y?o?u?r? ?mines is leeched to you as life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DamageLifeLeechToPlayer"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Mine,
                &[],
            )])
        }),
        lua_line: 4409,
    });
    v.push(PatternEntry {
        pat: "you can cast an additional brand",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ActiveBrandLimit"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4410,
    });
    v.push(PatternEntry {
        pat: "you can cast (%d+) additional brands",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveBrandLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4411,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased damage while you are wielding a bow and have a totem",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                INC,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "HaveTotem" }),
                    tag!({ type = Condition, var = "UsingBow" }),
                ],
            )])
        }),
        lua_line: 4412,
    });
    v.push(PatternEntry {
        pat: "each totem applies (%d+)%% increased damage taken to enemies near it",
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
                            n!("DamageTaken"),
                            INC,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Multiplier, var = "TotemsSummoned" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4413,
    });
    v.push(PatternEntry {
        pat: "totems gain %+(%d+)%% to (%w+) resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let resistance = ns(caps.cap[1], "index local 'resistance'");
            Some(vec![r#mod(
                cat!("Totem", firstToUpper(resistance), "Resist"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4414,
    });
    v.push(PatternEntry {
        pat: "totems gain %+(%d+)%% to all elemental resistances",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TotemElementalResist"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4415,
    });
    v.push(PatternEntry { pat: "rejuvenation totem also grants mana regeneration equal to 15%% of its life regeneration", val: SpecialVal::Mods(vec![flag(n!("Condition:RejuvenationTotemManaRegen"), None, 0, 0, &[])]), lua_line: 4416 });
    // Minions
    v.push(PatternEntry {
        pat: "your strength is added to your minions",
        val: SpecialVal::Mods(vec![r#mod(
            n!("StrengthAddedToMinions"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4418,
    });
    v.push(PatternEntry {
        pat: "half of your strength is added to your minions",
        val: SpecialVal::Mods(vec![r#mod(
            n!("StrengthAddedToMinions"),
            BASE,
            50.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4419,
    });
    v.push(PatternEntry {
        pat: "minions gain added resistances equal to (%d+)%% of your resistances",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ResistanceAddedToMinions"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4420,
    });
    v.push(PatternEntry {
        pat: "minions' accuracy rating is equal to yours",
        val: SpecialVal::Mods(vec![flag(
            n!("MinionAccuracyEqualsAccuracy"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4421,
    });
    v.push(PatternEntry {
        pat: "minions poison enemies on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(n!("PoisonChance"), BASE, 100.0, None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4422,
    });
    v.push(PatternEntry {
        pat: "minions have (%d+)%% chance to poison enemies on hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("PoisonChance"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4423,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased minion damage if you have hit recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HitRecently" })],
            )])
        }),
        lua_line: 4424,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased minion damage if you've used a minion skill recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsedMinionSkillRecently" })],
            )])
        }),
        lua_line: 4425,
    });
    v.push(PatternEntry { pat: "minions deal (%d+)%% increased damage if you have warcried recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "UsedWarcryRecently" })])))]), None, 0, 0, &[])])
    }), lua_line: 4426 });
    v.push(PatternEntry {
        pat: "minions deal (%d+)%% increased damage if you've used a minion skill recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsedMinionSkillRecently" })],
            )])
        }),
        lua_line: 4427,
    });
    v.push(PatternEntry {
        pat: "minions deal (%d+)%% more damage while they are on low life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("Damage"),
                            MORE,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "LowLife" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4428,
    });
    v.push(PatternEntry { pat: "minions have (%d+)%% increased attack and cast speed if you or your minions have killed recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Speed"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, varList = &[n!("KilledRecently"), n!("MinionsKilledRecently")][..] })])])
    }), lua_line: 4429 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased minion attack speed per (%d+) dexterity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Speed"), INC, num, None, ModFlag::Attack, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "Dex", div = tonumber(div) })],
            )])
        }),
        lua_line: 4430,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased minion movement speed per (%d+) dexterity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("MovementSpeed"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "Dex", div = tonumber(div) })],
            )])
        }),
        lua_line: 4431,
    });
}

/// @pob Modules/ModParser.lua:4432-4529 h=82d48648
fn specialModList_4432(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "minions deal (%d+)%% increased damage per (%d+) dexterity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "Dex", div = tonumber(div) })],
            )])
        }),
        lua_line: 4432,
    });
    v.push(PatternEntry {
        pat: "minions have (%d+)%% chance to deal double damage while they are on full life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("DoubleDamageChance"),
                            BASE,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "FullLife" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4433,
    });
    v.push(PatternEntry { pat: "minions have (%d+)%% chance to deal double damage per fortification on you", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = PerStat, stat = "FortificationStacks", actor = "parent" })])))]), None, 0, 0, &[])])
    }), lua_line: 4434 });
    v.push(PatternEntry { pat: "(%d+)%% increased golem damage for each type of golem you have summoned", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HavePhysicalGolem" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HaveLightningGolem" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HaveColdGolem" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HaveFireGolem" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HaveChaosGolem" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HaveCarrionGolem" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })])])
    }), lua_line: 4435 });
    v.push(PatternEntry {
        pat: "can summon up to (%d) additional golems? at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveGolemLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4443,
    });
    v.push(PatternEntry {
        pat: "%+(%d) to maximum number of sentinels of purity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveSentinelOfPurityLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4444,
    });
    v.push(PatternEntry {
        pat: "if you have 3 primordial jewels, can summon up to (%d) additional golems? at a time",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveGolemLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "PrimordialItem", threshold = 3.0 })],
            )])
        }),
        lua_line: 4445,
    });
    v.push(PatternEntry {
        pat: "golems regenerate (%d)%% of their maximum life per second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Golem })],
            )])
        }),
        lua_line: 4446,
    });
    v.push(PatternEntry {
        pat: "summoned golems regenerate (%d)%% of their life per second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Golem })],
            )])
        }),
        lua_line: 4447,
    });
    v.push(PatternEntry { pat: "summoned carrion golems impale on hit if you have the same number of them as summoned chaos golems", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ImpaleChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "CarrionEqualChaosGolem" }), tag!({ type = ActorCondition, actor = "parent", var = "HaveChaosGolem" })])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Carrion Golem", includeTransfigured = true })])]), lua_line: 4448 });
    v.push(PatternEntry { pat: "summoned chaos golems impale on hit if you have the same number of them as summoned stone golems", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ImpaleChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "ChaosEqualStoneGolem" }), tag!({ type = ActorCondition, actor = "parent", var = "HavePhysicalGolem" })])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Chaos Golem", includeTransfigured = true })])]), lua_line: 4451 });
    v.push(PatternEntry { pat: "summoned stone golems impale on hit if you have the same number of them as summoned carrion golems", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ImpaleChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "StoneEqualCarrionGolem" }), tag!({ type = ActorCondition, actor = "parent", var = "HaveCarrionGolem" })])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Stone Golem", includeTransfigured = true })])]), lua_line: 4454 });
    v.push(PatternEntry { pat: "maximum life of summoned elemental golems is doubled", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Life"), MORE, 100.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem }), tag!({ type = SkillType, skillTypeList = &[SkillType::Lightning, SkillType::Cold, SkillType::Fire][..] })])]), lua_line: 4457 });
    v.push(PatternEntry { pat: "golems summoned in the past 8 seconds deal (%d+)%% increased damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "SummonedGolemInPast8Sec" })])))]), None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Golem })])])
    }), lua_line: 4458 });
    v.push(PatternEntry { pat: "raised zombies and spectres gain adrenaline for 8 seconds when raised", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Adrenaline"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "SummonedSpectreInPast8Sec" }), tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Adrenaline"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "SummonedZombieInPast8Sec" }), tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true })])]), lua_line: 4459 });
    v.push(PatternEntry { pat: "raised spectres fire (%d+) additional projectiles", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ProjectileCount"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true })])])
    }), lua_line: 4463 });
    v.push(PatternEntry {
        pat: "gain onslaught for 10 seconds when you cast socketed golem skill",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![flag(
                n!("Condition:Onslaught"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "SummonedGolemInPast10Sec" })],
            )])
        }),
        lua_line: 4464,
    });
    v.push(PatternEntry { pat: "s?u?m?m?o?n?e?d? ?raging spirits' hits always ignite", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("EnemyIgniteChance"), BASE, 100.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Raging Spirit", includeTransfigured = true })])]), lua_line: 4465 });
    v.push(PatternEntry {
        pat: "raised zombies have avatar of fire",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(
                        n!("Keystone"),
                        LIST,
                        "Avatar of Fire",
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
            &[tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true })],
        )]),
        lua_line: 4466,
    });
    v.push(PatternEntry { pat: "raised zombies take ([%d%.]+)%% of their maximum life per second as fire damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireDegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num })])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true })])])
    }), lua_line: 4467 });
    v.push(PatternEntry {
        pat: "maximum number of summoned raging spirits is (%d+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActiveRagingSpiritLimit"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4468,
    });
    v.push(PatternEntry {
        pat: "maximum number of summoned phantasms is (%d+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ActivePhantasmLimit"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4469,
    });
    v.push(PatternEntry { pat: "summoned raging spirits have diamond shrine and massive shrine buffs", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:DiamondShrine"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Raging Spirit", includeTransfigured = true })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:MassiveShrine"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Raging Spirit", includeTransfigured = true })])]), lua_line: 4470 });
    v.push(PatternEntry {
        pat: "summoned phantasms have diamond shrine and massive shrine buffs",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:DiamondShrine"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Phantasm" })],
            ),
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:MassiveShrine"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Phantasm" })],
            ),
        ]),
        lua_line: 4474,
    });
    v.push(PatternEntry {
        pat: "minions deal no non%-(%a+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmgType = ns(caps.cap[0], "index local 'dmgType'");
            dealNoNonDamageType(dmgType, true)
        }),
        lua_line: 4478,
    });
    v.push(PatternEntry {
        pat: "minions convert (%d+)%% of (.+) damage to (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let source = ns(caps.cap[1], "index local 'source'");
            let target = ns(caps.cap[2], "index local 'target'");
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            cat!(
                                gsub_fn(source, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                "DamageConvertTo",
                                gsub_fn(target, "^%l", |c| Some(upper(c[0].as_str())), None).0
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
        lua_line: 4479,
    });
    v.push(PatternEntry { pat: "summoned skeletons have avatar of fire", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Keystone"), LIST, "Avatar of Fire", None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Skeletons", includeTransfigured = true })])]), lua_line: 4482 });
    v.push(PatternEntry { pat: "summoned skeletons take ([%d%.]+)%% of their maximum life per second as fire damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireDegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num })])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Skeletons", includeTransfigured = true })])])
    }), lua_line: 4483 });
    v.push(PatternEntry { pat: "summoned skeletons have (%d+)%% chance to wither enemies for (%d+) seconds on hit", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:CanWither"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Skeletons", includeTransfigured = true })])]), lua_line: 4484 });
    v.push(PatternEntry { pat: "summoned skeletons have (%d+)%% of physical damage converted to chaos damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("PhysicalDamageConvertToChaos"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Skeletons", includeTransfigured = true })])])
    }), lua_line: 4485 });
    v.push(PatternEntry { pat: "summoned skeletons gain added chaos damage equal to (%d+)%% of maximum energy shield on your equipped shield", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShieldOnWeapon 2", actor = "parent", percent = num })])))]), None, 0, 0, &[]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShieldOnWeapon 2", actor = "parent", percent = num })])))]), None, 0, 0, &[])])
    }), lua_line: 4486 });
    v.push(PatternEntry { pat: "skeletons gain added chaos damage equal to (%d+)%% of maximum energy shield on your equipped shield", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShieldOnWeapon 2", actor = "parent", percent = num })])))]), None, 0, 0, &[]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShieldOnWeapon 2", actor = "parent", percent = num })])))]), None, 0, 0, &[])])
    }), lua_line: 4490 });
    v.push(PatternEntry { pat: "minions convert (%d+)%% of (.+) damage to (.+) damage per (.+) socket", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let source = ns(caps.cap[1], "index local 'source'");
        let target = ns(caps.cap[2], "index local 'target'");
        let socketColor = ns(caps.cap[3], "index local 'socketColor'");
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(gsub_fn(source, "^%l", |c| Some(upper(c[0].as_str())), None).0, "DamageConvertTo", gsub_fn(target, "^%l", |c| Some(upper(c[0].as_str())), None).0), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Multiplier, var = cat!(gsub_fn(socketColor, "^%l", |c| Some(upper(c[0].as_str())), None).0, "SocketIn{SlotName}") })])])
    }), lua_line: 4494 });
    v.push(PatternEntry {
        pat: "minions convert (%d+)%% of (.+) damage to (.+) damage per socketed (%a+) gem",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let source = ns(caps.cap[1], "index local 'source'");
            let target = ns(caps.cap[2], "index local 'target'");
            let colour = ns(caps.cap[3], "index local 'colour'");
            // L4498
            if (((colour == "red") || (colour == "green")) || (colour == "blue")) {
                // L4499
                let mut var = lfmt!(
                    "Socketed%sGemsIn{SlotName}",
                    &*gsub_fn(colour, "^%l", |c| Some(upper(c[0].as_str())), None).0
                );
                // L4500
                return Some(vec![r#mod(
                    n!("MinionModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(
                                cat!(
                                    gsub_fn(source, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                    "DamageConvertTo",
                                    gsub_fn(target, "^%l", |c| Some(upper(c[0].as_str())), None).0
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
                    &[tag!({ type = Multiplier, var = Name::from(&*var) })],
                )]);
            }
            None
        }),
        lua_line: 4497,
    });
    v.push(PatternEntry {
        pat: "minions have a (%d+)%% chance to impale on hit with attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ImpaleChance"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4505,
    });
    v.push(PatternEntry {
        pat: "minions have (%d+)%% chance to impale on attack hit per socketed ghastly eye jewel",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ImpaleChance"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "GhastlyEyeJewelIn{SlotName}" })],
            )])
        }),
        lua_line: 4506,
    });
    v.push(PatternEntry {
        pat: "minions from herald skills deal (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Damage"), MORE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Herald })],
            )])
        }),
        lua_line: 4507,
    });
    v.push(PatternEntry {
        pat: "minions have (%d+)%% increased movement speed for each herald affecting you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("MovementSpeed"),
                            INC,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Multiplier, var = "Herald", actor = "parent" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4508,
    });
    v.push(PatternEntry { pat: "minions deal (%d+)%% increased damage while you are affected by a herald", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "AffectedByHerald" })])))]), None, 0, 0, &[])])
    }), lua_line: 4509 });
    v.push(PatternEntry { pat: "minions have (%d+)%% increased attack and cast speed while you are affected by a herald", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Speed"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "AffectedByHerald" })])))]), None, 0, 0, &[])])
    }), lua_line: 4510 });
    v.push(PatternEntry {
        pat: "minions have unholy might",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:UnholyMight"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:CanWither"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 4511,
    });
    v.push(PatternEntry { pat: "summoned skeleton warriors a?n?d? ?s?o?l?d?i?e?r?s? ?deal triple damage with this weapon if you've hit with this weapon recently", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("TripleDamageChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = ActorCondition, actor = "parent", var = "HitRecentlyWithWeapon" })])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Skeletons" }), tag!({ type = ItemCondition, itemSlot = "Weapon 1", nameCond = "The Iron Mass, Gladius" })])]), lua_line: 4515 });
    // just make the mod blue, handled in CalcSetup
    v.push(PatternEntry { pat: "summoned skeleton warriors a?n?d? ?s?o?l?d?i?e?r?s? ?wield a? ?c?o?p?y? ?o?f? ?this weapon while in your main hand", val: SpecialVal::Mods(vec![]), lua_line: 4518 });
    v.push(PatternEntry {
        pat: "each summoned phantasm grants you phantasmal might",
        val: SpecialVal::Mods(vec![flag(n!("Condition:PhantasmalMight"), None, 0, 0, &[])]),
        lua_line: 4519,
    });
    v.push(PatternEntry { pat: "minions have (%d+)%% increased critical strike chance per maximum power charge you have", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("CritChance"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, actor = "parent", var = "PowerChargeMax" })])))]), None, 0, 0, &[])])
    }), lua_line: 4520 });
    v.push(PatternEntry { pat: "minions' base attack critical strike chance is equal to the critical strike chance of your main hand weapon", val: SpecialVal::Mods(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("AttackCritIsEqualToParentMainHand"), None, ModFlag::Attack, 0, &[])))]), None, 0, 0, &[])]), lua_line: 4521 });
    v.push(PatternEntry { pat: "non%-spectre minions' base attack time is equal to the attack time of your main hand weapon", val: SpecialVal::Mods(vec![flag(n!("NonSpectreMinionsUseParentMainHandAttackTime"), None, 0, 0, &[])]), lua_line: 4522 });
    v.push(PatternEntry { pat: "minions can hear the whispers for 5 seconds after they deal a critical strike", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Speed"), INC, 50.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }), tag!({ type = Condition, neg = true, var = "NeverCrit" })])))]), None, 0, 0, &[]), r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), INC, 50.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }), tag!({ type = Condition, neg = true, var = "NeverCrit" })])))]), None, 0, 0, &[]), r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosDegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = 20.0 }), tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }), tag!({ type = Condition, neg = true, var = "NeverCrit" })])))]), None, 0, 0, &[])]), lua_line: 4523 });
    v.push(PatternEntry {
        pat: "chaos damage t?a?k?e?n? ?does not bypass minions' energy shield",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("ChaosNotBypassEnergyShield"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4528,
    });
    v.push(PatternEntry {
        pat: "while minions have energy shield, their hits ignore monster elemental resistances",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("IgnoreElementalResistances"),
                        None,
                        0,
                        0,
                        &[tag!({ type = StatThreshold, stat = "EnergyShield", threshold = 1.0 })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4529,
    });
}

/// @pob Modules/ModParser.lua:4530-4591 h=1e4ce8e0
fn specialModList_4530(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "summoned arbalists' projectiles pierce (%d+) additional targets",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("PierceCount"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
            )])
        }),
        lua_line: 4530,
    });
    v.push(PatternEntry {
        pat: "summoned arbalists' projectiles fork",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("ForkOnce"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
            ),
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ForkCountMax"), BASE, 1.0, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
            ),
        ]),
        lua_line: 4531,
    });
    v.push(PatternEntry {
        pat: "summoned arbalists' projectiles chain %+(%d+) times",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ChainCountMax"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
            )])
        }),
        lua_line: 4535,
    });
    v.push(PatternEntry {
        pat: "summoned arbalists have (%d+)%% chance to inflict (.+) exposure on hit",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let r#type = ns(caps.cap[1], "index local 'type'");
            Some(vec![r#mod(
                cat!(
                    gsub_fn(r#type, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                    "ExposureChance"
                ),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
            )])
        }),
        lua_line: 4536,
    });
    v.push(PatternEntry {
        pat: "summoned arbalists convert (%d+)%% of (.+) damage to (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let source = ns(caps.cap[1], "index local 'source'");
            let target = ns(caps.cap[2], "index local 'target'");
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            cat!(
                                gsub_fn(source, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                                "DamageConvertTo",
                                gsub_fn(target, "^%l", |c| Some(upper(c[0].as_str())), None).0
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
                &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
            )])
        }),
        lua_line: 4539,
    });
    v.push(PatternEntry {
        pat: "summoned arbalists have (%d+)%% chance to freeze, shock, and ignite",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("MinionModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(n!("EnemyFreezeChance"), BASE, num, None, 0, 0, &[])),
                        )],
                    ),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
                ),
                r#mod(
                    n!("MinionModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(n!("EnemyShockChance"), BASE, num, None, 0, 0, &[])),
                        )],
                    ),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
                ),
                r#mod(
                    n!("MinionModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(n!("EnemyIgniteChance"), BASE, num, None, 0, 0, &[])),
                        )],
                    ),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = "Summon Arbalists" })],
                ),
            ])
        }),
        lua_line: 4542,
    });
    // typo never existed except in some items generated by PoB
    v.push(PatternEntry {
        pat: "skeleton warriors are permanent minions and follow you",
        val: SpecialVal::Mods(vec![flag(
            n!("RaisedSkeletonPermanentDuration"),
            None,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Summon Skeletons" })],
        )]),
        lua_line: 4547,
    });
    v.push(PatternEntry {
        pat: "summoned skeleton warriors are permanent and follow you",
        val: SpecialVal::Mods(vec![flag(
            n!("RaisedSkeletonPermanentDuration"),
            None,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Summon Skeletons" })],
        )]),
        lua_line: 4548,
    });
    v.push(PatternEntry {
        pat: "your blink and mirror arrow clones use your gloves",
        val: SpecialVal::Mods(vec![flag(n!("BlinkAndMirrorUseGloves"), None, 0, 0, &[])]),
        lua_line: 4549,
    });
    // Projectiles
    v.push(PatternEntry {
        pat: "skills chain %+(%d) times",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ChainCountMax"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 4551,
    });
    v.push(PatternEntry {
        pat: "projectiles chain an additional time",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChainCountMax"),
            BASE,
            1.0,
            None,
            ModFlag::Projectile,
            0,
            &[],
        )]),
        lua_line: 4552,
    });
    v.push(PatternEntry {
        pat: "projectiles chain %+(%d) times",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChainCountMax"),
                BASE,
                num,
                None,
                ModFlag::Projectile,
                0,
                &[],
            )])
        }),
        lua_line: 4553,
    });
    v.push(PatternEntry {
        pat: "arrows chain %+(%d) times",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChainCountMax"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[],
            )])
        }),
        lua_line: 4554,
    });
    v.push(PatternEntry { pat: "skills chain an additional time while at maximum frenzy charges", val: SpecialVal::Mods(vec![r#mod(n!("ChainCountMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })])]), lua_line: 4555 });
    v.push(PatternEntry {
        pat: "attacks chain an additional time when in main hand",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChainCountMax"),
            BASE,
            1.0,
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = SlotNumber, num = 1.0 })],
        )]),
        lua_line: 4556,
    });
    v.push(PatternEntry {
        pat: "attacks fire an additional projectile when in off hand",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = SlotNumber, num = 2.0 })],
        )]),
        lua_line: 4557,
    });
    v.push(PatternEntry {
        pat: "projectiles chain %+(%d) times while you have phasing",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChainCountMax"),
                BASE,
                num,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = Condition, var = "Phasing" })],
            )])
        }),
        lua_line: 4558,
    });
    v.push(PatternEntry {
        pat: "projectiles can chain from any number of additional targets in close range",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChainCountMax"),
            BASE,
            m_huge,
            None,
            ModFlag::Projectile,
            0,
            &[tag!({ type = Condition, var = "AtCloseRange" })],
        )]),
        lua_line: 4559,
    });
    v.push(PatternEntry {
        pat: "projectiles split towards %+(%d) targets",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("SplitCount"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 4560,
    });
    v.push(PatternEntry {
        pat: "adds an additional arrow",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            0,
            KeywordFlag::Arrow,
            &[],
        )]),
        lua_line: 4561,
    });
    v.push(PatternEntry {
        pat: "(%d+) additional arrows",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[],
            )])
        }),
        lua_line: 4562,
    });
    v.push(PatternEntry {
        pat: "bow attacks fire an additional arrow",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Bow,
            0,
            &[],
        )]),
        lua_line: 4563,
    });
    v.push(PatternEntry {
        pat: "bow attacks fire (%d+) additional arrows",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[],
            )])
        }),
        lua_line: 4564,
    });
    v.push(PatternEntry {
        pat: "bow attacks fire (%d+) additional arrows if you haven't cast dash recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[tag!({ type = Condition, var = "CastDashRecently", neg = true })],
            )])
        }),
        lua_line: 4565,
    });
    v.push(PatternEntry {
        pat: "wand attacks fire an additional projectile",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Wand,
            0,
            &[],
        )]),
        lua_line: 4566,
    });
    v.push(PatternEntry {
        pat: "skills fire an additional projectile",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4567,
    });
    v.push(PatternEntry { pat: "skills fire an additional projectile if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let thresh = ns(caps.cap[0], "index local 'thresh'");
        let influence = ns(caps.cap[1], "index local 'influence'");
        Some(vec![r#mod(n!("ProjectileCount"), BASE, 1.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4568 });
    v.push(PatternEntry {
        pat: "spells [hf][ai][vr]e an additional projectile",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Spell,
            0,
            &[],
        )]),
        lua_line: 4569,
    });
    v.push(PatternEntry {
        pat: "spells [hf][ai][vr]e (%d+) additional projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                ModFlag::Spell,
                0,
                &[],
            )])
        }),
        lua_line: 4570,
    });
    v.push(PatternEntry {
        pat: "attacks fire an additional projectile",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Attack,
            0,
            &[],
        )]),
        lua_line: 4571,
    });
    v.push(PatternEntry {
        pat: "attacks [hf][ai][vr]e (%d+) additional projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 4572,
    });
    v.push(PatternEntry {
        pat: "attacks [hf][ai][vr]e (%d+) additional projectiles? when in off hand",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                ModFlag::Attack,
                0,
                &[tag!({ type = SlotNumber, num = 2.0 })],
            )])
        }),
        lua_line: 4573,
    });
    v.push(PatternEntry {
        pat: "fire at most 1 projectile",
        val: SpecialVal::Mods(vec![flag(n!("SingleProjectile"), None, 0, 0, &[])]),
        lua_line: 4574,
    });
    v.push(PatternEntry {
        pat: "attacks have an additional projectile when in off hand",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = SlotNumber, num = 2.0 })],
        )]),
        lua_line: 4575,
    });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry {
        pat: "caustic arrow and scourge arrow fire (%d+)%% more projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                MORE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4576,
    });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry {
        pat: "essence drain and soulrend fire (%d+) additional projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4577,
    });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry {
        pat: "(%d+)%% reduced essence drain and soulrend projectile speed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileSpeed"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4578,
    });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry {
        pat: "tornado shot fires an additional secondary projectile",
        val: SpecialVal::Mods(vec![r#mod(
            n!("tornadoShotSecondaryProjectiles"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4579,
    });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry {
        pat: "tornado shot fires 2 additional secondary projectiles",
        val: SpecialVal::Mods(vec![r#mod(
            n!("tornadoShotSecondaryProjectiles"),
            BASE,
            2.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4580,
    });
    v.push(PatternEntry {
        pat: "projectiles pierce an additional target",
        val: SpecialVal::Mods(vec![r#mod(n!("PierceCount"), BASE, 1.0, None, 0, 0, &[])]),
        lua_line: 4581,
    });
    v.push(PatternEntry {
        pat: "projectiles pierce (%d+) targets?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("PierceCount"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 4582,
    });
    v.push(PatternEntry {
        pat: "projectiles pierce (%d+) additional targets?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("PierceCount"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 4583,
    });
    v.push(PatternEntry {
        pat: "projectiles pierce (%d+) additional targets while you have phasing",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PierceCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Phasing" })],
            )])
        }),
        lua_line: 4584,
    });
    v.push(PatternEntry { pat: "projectiles pierce (%d+) additional targets if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = ns(caps.cap[0], "index local 'num'");
        let thresh = ns(caps.cap[1], "index local 'thresh'");
        let influence = ns(caps.cap[2], "index local 'influence'");
        Some(vec![r#mod(n!("PierceCount"), BASE, &*num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4585 });
    v.push(PatternEntry {
        pat: "projectiles pierce all targets while you have phasing",
        val: SpecialVal::Mods(vec![flag(
            n!("PierceAllTargets"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Phasing" })],
        )]),
        lua_line: 4586,
    });
    v.push(PatternEntry {
        pat: "projectiles pierce all burning enemies",
        val: SpecialVal::Mods(vec![flag(
            n!("PierceAllTargets"),
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Burning" })],
        )]),
        lua_line: 4587,
    });
    v.push(PatternEntry {
        pat: "arrows pierce an additional target",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PierceCount"),
            BASE,
            1.0,
            None,
            0,
            KeywordFlag::Arrow,
            &[],
        )]),
        lua_line: 4588,
    });
    v.push(PatternEntry {
        pat: "arrows pierce (%d+) additional targets",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PierceCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[],
            )])
        }),
        lua_line: 4589,
    });
    v.push(PatternEntry {
        pat: "arrows pierce one target",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PierceCount"),
            BASE,
            1.0,
            None,
            0,
            KeywordFlag::Arrow,
            &[],
        )]),
        lua_line: 4590,
    });
    v.push(PatternEntry {
        pat: "arrows pierce (%d+) targets?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PierceCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[],
            )])
        }),
        lua_line: 4591,
    });
}

/// @pob Modules/ModParser.lua:4592-4681 h=fceee504
fn specialModList_4592(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "always pierce with arrows",
        val: SpecialVal::Mods(vec![flag(
            n!("PierceAllTargets"),
            None,
            0,
            KeywordFlag::Arrow,
            &[],
        )]),
        lua_line: 4592,
    });
    v.push(PatternEntry {
        pat: "arrows always pierce",
        val: SpecialVal::Mods(vec![flag(
            n!("PierceAllTargets"),
            None,
            0,
            KeywordFlag::Arrow,
            &[],
        )]),
        lua_line: 4593,
    });
    v.push(PatternEntry {
        pat: "arrows pierce all targets",
        val: SpecialVal::Mods(vec![flag(
            n!("PierceAllTargets"),
            None,
            0,
            KeywordFlag::Arrow,
            &[],
        )]),
        lua_line: 4594,
    });
    v.push(PatternEntry {
        pat: "arrows that pierce cause bleeding",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BleedChance"),
            BASE,
            100.0,
            None,
            ModFlag::Projectile,
            KeywordFlag::Arrow,
            &[tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })],
        )]),
        lua_line: 4595,
    });
    v.push(PatternEntry {
        pat: "arrows that pierce have (%d+)%% chance to [ic][na][fu][ls][ie]c?t? bleeding",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BleedChance"),
                BASE,
                num,
                None,
                ModFlag::Projectile,
                KeywordFlag::Arrow,
                &[tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })],
            )])
        }),
        lua_line: 4596,
    });
    v.push(PatternEntry {
        pat: "arrows that pierce deal (%d+)%% increased damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                INC,
                num,
                None,
                ModFlag::Projectile,
                KeywordFlag::Arrow,
                &[tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })],
            )])
        }),
        lua_line: 4597,
    });
    v.push(PatternEntry {
        pat: "projectiles gain (%d+)%% of non%-chaos damage as extra chaos damage per chain",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("NonChaosDamageGainAsChaos"),
                BASE,
                num,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = PerStat, stat = "Chain" })],
            )])
        }),
        lua_line: 4598,
    });
    v.push(PatternEntry {
        pat:
            "projectiles that have chained gain (%d+)%% of non%-chaos damage as extra chaos damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("NonChaosDamageGainAsChaos"),
                BASE,
                num,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = StatThreshold, stat = "Chain", threshold = 1.0 })],
            )])
        }),
        lua_line: 4599,
    });
    v.push(PatternEntry {
        pat: "left ring slot: projectiles from spells cannot chain",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotChain"),
            None,
            bor(ModFlag::Spell, ModFlag::Projectile),
            0,
            &[tag!({ type = SlotNumber, num = 1.0 })],
        )]),
        lua_line: 4600,
    });
    v.push(PatternEntry {
        pat: "left ring slot: projectiles from spells fork",
        val: SpecialVal::Mods(vec![
            flag(
                n!("ForkOnce"),
                None,
                bor(ModFlag::Spell, ModFlag::Projectile),
                0,
                &[tag!({ type = SlotNumber, num = 1.0 })],
            ),
            r#mod(
                n!("ForkCountMax"),
                BASE,
                1.0,
                None,
                bor(ModFlag::Spell, ModFlag::Projectile),
                0,
                &[tag!({ type = SlotNumber, num = 1.0 })],
            ),
        ]),
        lua_line: 4601,
    });
    v.push(PatternEntry {
        pat: "right ring slot: projectiles from spells chain %+1 times",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChainCountMax"),
            BASE,
            1.0,
            None,
            bor(ModFlag::Spell, ModFlag::Projectile),
            0,
            &[tag!({ type = SlotNumber, num = 2.0 })],
        )]),
        lua_line: 4602,
    });
    v.push(PatternEntry {
        pat: "right ring slot: projectiles from spells cannot fork",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotFork"),
            None,
            bor(ModFlag::Spell, ModFlag::Projectile),
            0,
            &[tag!({ type = SlotNumber, num = 2.0 })],
        )]),
        lua_line: 4603,
    });
    v.push(PatternEntry {
        pat: "projectiles from spells cannot pierce",
        val: SpecialVal::Mods(vec![flag(n!("CannotPierce"), None, ModFlag::Spell, 0, &[])]),
        lua_line: 4604,
    });
    v.push(PatternEntry {
        pat: "projectiles fork",
        val: SpecialVal::Mods(vec![
            flag(n!("ForkOnce"), None, ModFlag::Projectile, 0, &[]),
            r#mod(
                n!("ForkCountMax"),
                BASE,
                1.0,
                None,
                ModFlag::Projectile,
                0,
                &[],
            ),
        ]),
        lua_line: 4605,
    });
    v.push(PatternEntry {
        pat: "projectiles from attacks fork",
        val: SpecialVal::Mods(vec![
            flag(
                n!("ForkOnce"),
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::RangedAttack })],
            ),
            r#mod(
                n!("ForkCountMax"),
                BASE,
                1.0,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::RangedAttack })],
            ),
        ]),
        lua_line: 4606,
    });
    v.push(PatternEntry {
        pat: "projectiles from attacks fork an additional time",
        val: SpecialVal::Mods(vec![
            flag(
                n!("ForkTwice"),
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::RangedAttack })],
            ),
            r#mod(
                n!("ForkCountMax"),
                BASE,
                1.0,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::RangedAttack })],
            ),
        ]),
        lua_line: 4607,
    });
    v.push(PatternEntry {
        pat: "projectiles from attacks can fork (%d+) additional times?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(
                    n!("ForkTwice"),
                    None,
                    ModFlag::Projectile,
                    0,
                    &[tag!({ type = SkillType, skillType = SkillType::RangedAttack })],
                ),
                r#mod(
                    n!("ForkCountMax"),
                    BASE,
                    num,
                    None,
                    ModFlag::Projectile,
                    0,
                    &[tag!({ type = SkillType, skillType = SkillType::RangedAttack })],
                ),
            ])
        }),
        lua_line: 4608,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased critical strike chance with arrows that fork", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritChance"), INC, num, None, 0, KeywordFlag::Arrow, &[tag!({ type = StatThreshold, stat = "ForkRemaining", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "PierceCount", threshold = 0.0, upper = true })])])
    }), lua_line: 4612 });
    v.push(PatternEntry {
        pat: "arrows that pierce have %+(%d+)%% to critical strike multiplier",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritMultiplier"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })],
            )])
        }),
        lua_line: 4615,
    });
    v.push(PatternEntry {
        pat: "arrows pierce all targets after forking",
        val: SpecialVal::Mods(vec![flag(
            n!("PierceAllTargets"),
            None,
            0,
            KeywordFlag::Arrow,
            &[tag!({ type = StatThreshold, stat = "ForkedCount", threshold = 1.0 })],
        )]),
        lua_line: 4617,
    });
    v.push(PatternEntry { pat: "modifiers to number of projectiles instead apply to the number of targets projectiles split towards", val: SpecialVal::Mods(vec![flag(n!("NoAdditionalProjectiles"), None, 0, 0, &[]), flag(n!("AdditionalProjectilesAddSplitsInstead"), None, 0, 0, &[])]), lua_line: 4618 });
    v.push(PatternEntry { pat: "modifiers to number of projectiles do not apply to fireball and rolling magma", val: SpecialVal::Mods(vec![flag(n!("NoAdditionalProjectiles"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Fireball"), n!("Rolling Magma")][..] })])]), lua_line: 4622 });
    v.push(PatternEntry {
        pat: "attack skills fire an additional projectile while wielding a claw or dagger",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ProjectileCount"),
            BASE,
            1.0,
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Claw, ModFlag::Dagger) })],
        )]),
        lua_line: 4623,
    });
    v.push(PatternEntry { pat: "skills fire (%d+) additional projectiles for 4 seconds after you consume a total of 12 steel shards", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ProjectileCount"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "Consumed12SteelShardsRecently" })])])
    }), lua_line: 4624 });
    v.push(PatternEntry {
        pat: "bow attacks sacrifice a random damageable minion to fire (%d+) additional arrows?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ProjectileCount"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Arrow,
                &[
                    tag!({ type = Condition, var = "SacrificeMinionOnAttack" }),
                    tag!({ type = Condition, var = "HaveDamageableMinion" }),
                    tag!({ type = SkillType, skillType = SkillType::Triggered, neg = true }),
                    tag!({ type = SkillType, skillType = SkillType::SummonsTotem, neg = true }),
                ],
            )])
        }),
        lua_line: 4625,
    });
    v.push(PatternEntry {
        pat: "non%-projectile chaining lightning skills chain %+(%d+) times",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChainCountMax"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Projectile, neg = true }),
                    tag!({ type = SkillType, skillType = SkillType::Chains }),
                    tag!({ type = SkillType, skillType = SkillType::Lightning }),
                ],
            )])
        }),
        lua_line: 4628,
    });
    v.push(PatternEntry { pat: "arrows gain damage as they travel farther, dealing up to (%d+)%% increased damage with hits to targets", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, ModFlag::Hit, KeywordFlag::Arrow, &[tag!({ type = DistanceRamp, ramp = &[[35.0, 0.0], [70.0, 1.0]][..] })])])
    }), lua_line: 4629 });
    v.push(PatternEntry { pat: "arrows gain critical strike chance as they travel farther, up to (%d+)%% increased critical strike chance", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritChance"), INC, num, None, 0, KeywordFlag::Arrow, &[tag!({ type = DistanceRamp, ramp = &[[35.0, 0.0], [70.0, 1.0]][..] })])])
    }), lua_line: 4630 });
    v.push(PatternEntry { pat: "projectiles deal (%d+)%% increased damage with hits to targets at the start of their movement, reducing to (%d+)%% as they travel farther", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, bor(ModFlag::Hit, ModFlag::Projectile), 0, &[tag!({ type = DistanceRamp, ramp = &[[35.0, 1.0], [70.0, 0.0]][..] })])])
    }), lua_line: 4631 });
    v.push(PatternEntry { pat: "projectiles deal (%d+)%% increased damage with hits and ailments for each time they have chained", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = PerStat, stat = "Chain" }), tag!({ type = SkillType, skillType = SkillType::Projectile })])])
    }), lua_line: 4632 });
    v.push(PatternEntry { pat: "projectiles deal (%d+)%% increased damage with hits and ailments for each enemy pierced", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = PerStat, stat = "PiercedCount" }), tag!({ type = SkillType, skillType = SkillType::Projectile })])])
    }), lua_line: 4633 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased bonuses gained from equipped (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = ns(caps.cap[0], "index local 'num'");
            let slot = ns(caps.cap[1], "index local 'slot'");
            Some(vec![r#mod(
                cat!("EffectOfBonusesFrom", firstToUpper(slot)),
                INC,
                tonumber(num),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4634,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% reduced bonuses gained from equipped (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = ns(caps.cap[0], "index local 'num'");
            let slot = ns(caps.cap[1], "index local 'slot'");
            Some(vec![r#mod(
                cat!("EffectOfBonusesFrom", firstToUpper(slot)),
                INC,
                (-snum(num, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4635,
    });
    // Strike Skills
    v.push(PatternEntry {
        pat: "non%-vaal strike skills target (%d+) additional nearby enem[yi]e?s?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AdditionalStrikeTarget"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::MeleeSingleTarget }),
                    tag!({ type = SkillType, skillType = SkillType::Vaal, neg = true }),
                ],
            )])
        }),
        lua_line: 4637,
    });
    // Leech/Gain on Hit/Kill
    v.push(PatternEntry {
        pat: "cannot leech life",
        val: SpecialVal::Mods(vec![flag(n!("CannotLeechLife"), None, 0, 0, &[])]),
        lua_line: 4639,
    });
    v.push(PatternEntry {
        pat: "cannot leech mana",
        val: SpecialVal::Mods(vec![flag(n!("CannotLeechMana"), None, 0, 0, &[])]),
        lua_line: 4640,
    });
    v.push(PatternEntry {
        pat: "cannot leech when on low life",
        val: SpecialVal::Mods(vec![
            flag(
                n!("CannotLeechLife"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "LowLife" })],
            ),
            flag(
                n!("CannotLeechMana"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "LowLife" })],
            ),
        ]),
        lua_line: 4641,
    });
    v.push(PatternEntry {
        pat: "cannot leech life from critical strikes",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotLeechLife"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 4645,
    });
    v.push(PatternEntry {
        pat: "leech applies instantly on critical strike",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("InstantLifeLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
            r#mod(
                n!("InstantManaLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
            r#mod(
                n!("InstantEnergyShieldLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
        ]),
        lua_line: 4646,
    });
    v.push(PatternEntry {
        pat: "gain life and mana from leech instantly on critical strike",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("InstantLifeLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
            r#mod(
                n!("InstantManaLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
            r#mod(
                n!("InstantEnergyShieldLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
        ]),
        lua_line: 4651,
    });
    v.push(PatternEntry {
        pat: "leech applies instantly during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("InstantLifeLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            r#mod(
                n!("InstantManaLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            r#mod(
                n!("InstantEnergyShieldLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 4656,
    });
    v.push(PatternEntry {
        pat: "gain life and mana from leech instantly during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("InstantLifeLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            r#mod(
                n!("InstantManaLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 4661,
    });
    v.push(PatternEntry {
        pat: "life and mana leech are instant during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("InstantLifeLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            r#mod(
                n!("InstantManaLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 4665,
    });
    v.push(PatternEntry {
        pat: "life and mana leech from critical strikes are instant",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("InstantLifeLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
            r#mod(
                n!("InstantManaLeech"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            ),
        ]),
        lua_line: 4669,
    });
    v.push(PatternEntry { pat: "with 5 corrupted items equipped: life leech recovers based on your chaos damage instead", val: SpecialVal::Mods(vec![flag(n!("LifeLeechBasedOnChaosDamage"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "CorruptedItem", threshold = 5.0 })])]), lua_line: 4673 });
    v.push(PatternEntry {
        pat: "you have vaal pact if you've dealt a critical strike recently",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Vaal Pact",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CritRecently" })],
        )]),
        lua_line: 4674,
    });
    v.push(PatternEntry { pat: "you have vaal pact while at maximum endurance charges", val: SpecialVal::Mods(vec![r#mod(n!("Keystone"), LIST, "Vaal Pact", None, 0, 0, &[tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMax" })])]), lua_line: 4675 });
    v.push(PatternEntry {
        pat: "you have vaal pact while focus?sed",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Vaal Pact",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Focused" })],
        )]),
        lua_line: 4676,
    });
    v.push(PatternEntry { pat: "gain (%d+) energy shield for each enemy you hit which is affected by a spider's web", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnergyShieldOnHit"), BASE, num, None, ModFlag::Hit, 0, &[tag!({ type = MultiplierThreshold, actor = "enemy", var = "Spider's WebStack", threshold = 1.0 })])])
    }), lua_line: 4677 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain (%d+) life on hit with attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            let amount = ns(caps.cap[1], "index local 'amount'");
            Some(vec![
                r#mod(
                    n!("LifeOnHit"),
                    BASE,
                    ((snum(amount, "perform arithmetic on a string")
                        * nn(chance, "perform arithmetic on local 'chance'"))
                        / 100.0),
                    None,
                    bor(ModFlag::Attack, ModFlag::Hit),
                    0,
                    &[tag!({ type = Condition, var = "AverageResourceGain" })],
                ),
                r#mod(
                    n!("LifeOnHit"),
                    BASE,
                    tonumber(amount),
                    None,
                    bor(ModFlag::Attack, ModFlag::Hit),
                    0,
                    &[tag!({ type = Condition, var = "MaxResourceGain" })],
                ),
            ])
        }),
        lua_line: 4678,
    });
}

/// @pob Modules/ModParser.lua:4682-4743 h=82a2edff
fn specialModList_4682(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "(%d+) life gained for each cursed enemy hit by your attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnHit"),
                BASE,
                num,
                None,
                bor(ModFlag::Attack, ModFlag::Hit),
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
            )])
        }),
        lua_line: 4682,
    });
    v.push(PatternEntry {
        pat: "gain (%d+) life per cursed enemy hit with attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnHit"),
                BASE,
                num,
                None,
                bor(ModFlag::Attack, ModFlag::Hit),
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
            )])
        }),
        lua_line: 4683,
    });
    v.push(PatternEntry {
        pat: "gain (%d+) life for each ignited enemy hit with attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnHit"),
                BASE,
                num,
                None,
                bor(ModFlag::Attack, ModFlag::Hit),
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })],
            )])
        }),
        lua_line: 4684,
    });
    v.push(PatternEntry {
        pat: "(%d+) mana gained for each cursed enemy hit by your attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnHit"),
                BASE,
                num,
                None,
                bor(ModFlag::Attack, ModFlag::Hit),
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
            )])
        }),
        lua_line: 4685,
    });
    v.push(PatternEntry {
        pat: "gain (%d+) mana per cursed enemy hit with attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnHit"),
                BASE,
                num,
                None,
                bor(ModFlag::Attack, ModFlag::Hit),
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
            )])
        }),
        lua_line: 4686,
    });
    v.push(PatternEntry {
        pat: "gain (%d+) life per blinded enemy hit with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnHit"),
                BASE,
                num,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = ActorCondition, actor = "enemy", var = "Blinded" }),
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                ],
            )])
        }),
        lua_line: 4687,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of life on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Life", percent = num })],
            )])
        }),
        lua_line: 4688,
    });
    v.push(PatternEntry {
        pat:
            "recover (%d+)%% of life on kill for each different type of mastery you have allocated",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num }),
                    tag!({ type = Multiplier, var = "AllocatedMasteryType" }),
                ],
            )])
        }),
        lua_line: 4689,
    });
    v.push(PatternEntry { pat: "recover (%d+)%% of life on killing a poisoned enemy", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LifeOnKill"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num }), tag!({ type = MultiplierThreshold, actor = "enemy", var = "PoisonStack", threshold = 1.0 })])])
    }), lua_line: 4690 });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of life on killing a chilled enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num }),
                    tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" }),
                ],
            )])
        }),
        lua_line: 4691,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of life when you kill a cursed enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num }),
                    tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" }),
                ],
            )])
        }),
        lua_line: 4692,
    });
    v.push(PatternEntry { pat: "recover (%d+)%% of life per withered debuff on each enemy you kill", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LifeOnKill"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num }), tag!({ type = Multiplier, var = "WitheredStack", actor = "enemy", limit = 15.0 })])])
    }), lua_line: 4693 });
    v.push(PatternEntry { pat: "minions recover (%d+)%% of life on killing a poisoned enemy", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LifeOnKill"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num }), tag!({ type = MultiplierThreshold, actor = "enemy", var = "PoisonStack", threshold = 1.0 })])))]), None, 0, 0, &[])])
    }), lua_line: 4694 });
    v.push(PatternEntry {
        pat: "minions recover (%d+)%% of their life when they block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("LifeOnBlock"),
                            BASE,
                            1.0,
                            None,
                            0,
                            0,
                            &[tag!({ type = PercentStat, stat = "Life", percent = num })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4695,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of mana when you kill a cursed enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Mana", percent = num }),
                    tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" }),
                ],
            )])
        }),
        lua_line: 4696,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of energy shield when you kill a cursed enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "EnergyShield", percent = num }),
                    tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" }),
                ],
            )])
        }),
        lua_line: 4697,
    });
    v.push(PatternEntry { pat: "recover (%d+)%% of life on kill if you've spent life recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LifeOnKill"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num }), tag!({ type = MultiplierThreshold, var = "LifeSpentRecently", threshold = 1.0 })])])
    }), lua_line: 4698 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to recover all life when you kill an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let chance = caps.num;
            Some(vec![
                r#mod(
                    n!("LifeOnKill"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "Life", percent = chance }),
                        tag!({ type = Condition, var = "AverageResourceGain" }),
                    ],
                ),
                r#mod(
                    n!("LifeOnKill"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "Life", percent = 100.0 }),
                        tag!({ type = Condition, var = "MaxResourceGain" }),
                    ],
                ),
            ])
        }),
        lua_line: 4699,
    });
    v.push(PatternEntry {
        pat: "lose (%d+)%% of life on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                -1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Life", percent = num })],
            )])
        }),
        lua_line: 4703,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) life gained on killing ignited enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })],
            )])
        }),
        lua_line: 4704,
    });
    v.push(PatternEntry {
        pat: "gain (%d+) life per ignited enemy killed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnKill"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })],
            )])
        }),
        lua_line: 4705,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of mana on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Mana", percent = num })],
            )])
        }),
        lua_line: 4706,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of mana on kill while you have a tincture active",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Mana", percent = num }),
                    tag!({ type = Condition, var = "UsingTincture" }),
                ],
            )])
        }),
        lua_line: 4707,
    });
    v.push(PatternEntry {
        pat:
            "recover (%d+)%% of mana on kill for each different type of mastery you have allocated",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Mana", percent = num }),
                    tag!({ type = Multiplier, var = "AllocatedMasteryType" }),
                ],
            )])
        }),
        lua_line: 4708,
    });
    v.push(PatternEntry {
        pat: "lose (%d+)%% of mana on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnKill"),
                BASE,
                -1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Mana", percent = num })],
            )])
        }),
        lua_line: 4709,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) mana gained on killing a frozen enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnKill"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })],
            )])
        }),
        lua_line: 4710,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of energy shield on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnKill"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num })],
            )])
        }),
        lua_line: 4711,
    });
    v.push(PatternEntry { pat: "recover (%d+)%% of energy shield on kill for each different type of mastery you have allocated", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnergyShieldOnKill"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num }), tag!({ type = Multiplier, var = "AllocatedMasteryType" })])])
    }), lua_line: 4712 });
    v.push(PatternEntry {
        pat: "lose (%d+)%% of energy shield on kill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnKill"),
                BASE,
                -1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num })],
            )])
        }),
        lua_line: 4713,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) energy shield gained on killing a shocked enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnKill"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })],
            )])
        }),
        lua_line: 4714,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) energy shield gained on kill per level",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnKill"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "Level" })],
            )])
        }),
        lua_line: 4715,
    });
    // Defences
    v.push(PatternEntry {
        pat: "chaos damage t?a?k?e?n? ?does not bypass energy shield",
        val: SpecialVal::Mods(vec![flag(
            n!("ChaosNotBypassEnergyShield"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4717,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of chaos damage t?a?k?e?n? ?does not bypass energy shield",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChaosEnergyShieldBypass"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4718,
    });
    v.push(PatternEntry {
        pat: "chaos damage t?a?k?e?n? ?does not bypass energy shield while not on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("ChaosNotBypassEnergyShield"),
            None,
            0,
            0,
            &[tag!({ type = Condition, varList = &[n!("LowLife")][..], neg = true })],
        )]),
        lua_line: 4719,
    });
    v.push(PatternEntry { pat: "chaos damage t?a?k?e?n? ?does not bypass energy shield while not on low life or low mana", val: SpecialVal::Mods(vec![flag(n!("ChaosNotBypassEnergyShield"), None, 0, 0, &[tag!({ type = Condition, varList = &[n!("LowLife"), n!("LowMana")][..], neg = true })])]), lua_line: 4720 });
    v.push(PatternEntry {
        pat: "chaos damage t?a?k?e?n? ?does not bypass energy shield while not on low mana",
        val: SpecialVal::Mods(vec![flag(
            n!("ChaosNotBypassEnergyShield"),
            None,
            0,
            0,
            &[tag!({ type = Condition, varList = &[n!("LowMana")][..], neg = true })],
        )]),
        lua_line: 4721,
    });
    v.push(PatternEntry {
        pat: "chaos damage is taken from mana before life",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChaosDamageTakenFromManaBeforeLife"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4722,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of physical damage is taken from mana before life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PhysicalDamageTakenFromManaBeforeLife"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4723,
    });
    v.push(PatternEntry {
        pat: "minions take (%d+)%% increased damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
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
                &[],
            )])
        }),
        lua_line: 4724,
    });
    v.push(PatternEntry {
        pat: "minions take (%d+)%% reduced damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("DamageTaken"),
                            INC,
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
                &[],
            )])
        }),
        lua_line: 4725,
    });
    v.push(PatternEntry { pat: "you and your minions take (%d+)%% reduced reflected (%w+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        Some(vec![r#mod(cat!(gsub_fn(r#type, "^%l", |c| Some(upper(c[0].as_str())), None).0, "ReflectedDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(cat!(gsub_fn(r#type, "^%l", |c| Some(upper(c[0].as_str())), None).0, "ReflectedDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])))]), None, 0, 0, &[])])
    }), lua_line: 4726 });
    v.push(PatternEntry {
        pat: "(%d+)%% reduced reflected damage taken during effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReflectedDamageTaken"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
            )])
        }),
        lua_line: 4730,
    });
    v.push(PatternEntry { pat: "you and your minions take (%d+)%% reduced reflected damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ReflectedDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ReflectedDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])))]), None, 0, 0, &[])])
    }), lua_line: 4733 });
    v.push(PatternEntry {
        pat: "damage cannot be reflected",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ReflectedDamageTaken"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 4737,
    });
    v.push(PatternEntry {
        pat: "immune to reflected damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ReflectedDamageTaken"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 4738,
    });
    v.push(PatternEntry { pat: "cannot take reflected (%a+) damage if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let element = ns(caps.cap[0], "index local 'element'");
        let thresh = ns(caps.cap[1], "index local 'thresh'");
        let influence = ns(caps.cap[2], "index local 'influence'");
        Some(vec![r#mod(cat!(firstToUpper(element), "ReflectedDamageTaken"), MORE, -100.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4739 });
    v.push(PatternEntry { pat: "you have mind over matter while at maximum power charges", val: SpecialVal::Mods(vec![r#mod(n!("Keystone"), LIST, "Mind Over Matter", None, 0, 0, &[tag!({ type = StatThreshold, stat = "PowerCharges", thresholdStat = "PowerChargesMax" })])]), lua_line: 4740 });
    v.push(PatternEntry {
        pat: "cannot evade enemy attacks",
        val: SpecialVal::Mods(vec![flag(n!("CannotEvade"), None, 0, 0, &[])]),
        lua_line: 4741,
    });
    v.push(PatternEntry {
        pat: "attacks cannot hit you",
        val: SpecialVal::Mods(vec![flag(n!("AlwaysEvade"), None, 0, 0, &[])]),
        lua_line: 4742,
    });
    v.push(PatternEntry {
        pat: "attacks against you always hit",
        val: SpecialVal::Mods(vec![flag(n!("CannotEvade"), None, 0, 0, &[])]),
        lua_line: 4743,
    });
}

/// @pob Modules/ModParser.lua:4744-4818 h=70d23214
fn specialModList_4744(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "cannot block",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotBlockAttacks"), None, 0, 0, &[]),
            flag(n!("CannotBlockSpells"), None, 0, 0, &[]),
        ]),
        lua_line: 4744,
    });
    v.push(PatternEntry {
        pat: "cannot block while you have no energy shield",
        val: SpecialVal::Mods(vec![
            flag(
                n!("CannotBlockAttacks"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveEnergyShield", neg = true })],
            ),
            flag(
                n!("CannotBlockSpells"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveEnergyShield", neg = true })],
            ),
        ]),
        lua_line: 4745,
    });
    v.push(PatternEntry {
        pat: "cannot block attacks",
        val: SpecialVal::Mods(vec![flag(n!("CannotBlockAttacks"), None, 0, 0, &[])]),
        lua_line: 4746,
    });
    v.push(PatternEntry {
        pat: "cannot block attack damage",
        val: SpecialVal::Mods(vec![flag(n!("CannotBlockAttacks"), None, 0, 0, &[])]),
        lua_line: 4747,
    });
    v.push(PatternEntry {
        pat: "cannot block spells",
        val: SpecialVal::Mods(vec![flag(n!("CannotBlockSpells"), None, 0, 0, &[])]),
        lua_line: 4748,
    });
    v.push(PatternEntry {
        pat: "cannot block spell damage",
        val: SpecialVal::Mods(vec![flag(n!("CannotBlockSpells"), None, 0, 0, &[])]),
        lua_line: 4749,
    });
    v.push(PatternEntry {
        pat: "monsters cannot block your attacks",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("CannotBlockAttacks"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4750,
    });
    v.push(PatternEntry {
        pat: "damage t?a?k?e?n? from blocked hits cannot bypass energy shield",
        val: SpecialVal::Mods(vec![flag(
            n!("BlockedDamageDoesntBypassES"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "EVBypass", neg = true })],
        )]),
        lua_line: 4751,
    });
    v.push(PatternEntry {
        pat: "damage t?a?k?e?n? from unblocked hits always bypasses energy shield",
        val: SpecialVal::Mods(vec![flag(
            n!("UnblockedDamageDoesBypassES"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "EVBypass", neg = true })],
        )]),
        lua_line: 4752,
    });
    v.push(PatternEntry {
        pat: "recover (%d+) life when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("LifeOnBlock"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 4753,
    });
    v.push(PatternEntry {
        pat: "recover (%d+) energy shield when you block spell damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnSpellBlock"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4754,
    });
    v.push(PatternEntry {
        pat: "recover (%d+) energy shield when you suppress spell damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnSuppress"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4755,
    });
    v.push(PatternEntry {
        pat: "recover (%d+) life when you suppress spell damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnSuppress"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4756,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of life when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Life", percent = num })],
            )])
        }),
        lua_line: 4757,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of life when you block attack damage while wielding a staff",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num }),
                    tag!({ type = Condition, var = "UsingStaff" }),
                ],
            )])
        }),
        lua_line: 4758,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of your maximum mana when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Mana", percent = num })],
            )])
        }),
        lua_line: 4759,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of energy shield when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num })],
            )])
        }),
        lua_line: 4760,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of energy shield when you block spell damage while wielding a staff",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnSpellBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "EnergyShield", percent = num }),
                    tag!({ type = Condition, var = "UsingStaff" }),
                ],
            )])
        }),
        lua_line: 4761,
    });
    v.push(PatternEntry {
        pat: "replenishes energy shield by (%d+)%% of armour when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Armour", percent = num })],
            )])
        }),
        lua_line: 4762,
    });
    v.push(PatternEntry {
        pat: "recover energy shield equal to (%d+)%% of armour when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Armour", percent = num })],
            )])
        }),
        lua_line: 4763,
    });
    v.push(PatternEntry {
        pat: "recover energy shield equal to (%d+)%% of evasion rating when you block",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldOnBlock"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Evasion", percent = num })],
            )])
        }),
        lua_line: 4764,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of damage taken while affected by clarity recouped as mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaRecoup"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByClarity" })],
            )])
        }),
        lua_line: 4765,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of damage taken while frozen recouped as life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeRecoup"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Frozen" })],
            )])
        }),
        lua_line: 4766,
    });
    v.push(PatternEntry {
        pat: "recoup effects instead occur over 3 seconds",
        val: SpecialVal::Mods(vec![flag(n!("3SecondRecoup"), None, 0, 0, &[])]),
        lua_line: 4767,
    });
    v.push(PatternEntry {
        pat: "life recoup effects instead occur over 3 seconds",
        val: SpecialVal::Mods(vec![flag(n!("3SecondLifeRecoup"), None, 0, 0, &[])]),
        lua_line: 4768,
    });
    v.push(PatternEntry {
        pat: "recoup energy shield instead of life",
        val: SpecialVal::Mods(vec![flag(
            n!("EnergyShieldRecoupInsteadOfLife"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4769,
    });
    v.push(PatternEntry {
        pat: "life recoup also recovers mana",
        val: SpecialVal::Mods(vec![flag(n!("AddLifeRecoupToManaRecoup"), None, 0, 0, &[])]),
        lua_line: 4770,
    });
    v.push(PatternEntry {
        pat: "damage taken recouped as (%a+) is also recouped as energy shield",
        val: SpecialVal::Fn(|caps: &Caps| {
            let r#type = ns(caps.cap[0], "index local 'type'");
            Some(vec![flag(
                cat!("Add", firstToUpper(r#type), "RecoupToEnergyShieldRecoup"),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4771,
    });
    v.push(PatternEntry { pat: "([%d%.]+)%% of physical damage prevented from hits in the past (%d+) seconds is regenerated as life per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let duration = ns(caps.cap[1], "index local 'duration'");
        Some(vec![r#mod(n!("PhysicalDamageMitigatedLifePseudoRecoup"), BASE, (nn(num, "perform arithmetic on local 'num'") * snum(duration, "perform arithmetic on a string")), None, 0, 0, &[]), r#mod(n!("PhysicalDamageMitigatedLifePseudoRecoupDuration"), BASE, &*duration, None, 0, 0, &[])])
    }), lua_line: 4774 });
    v.push(PatternEntry { pat: "([%d%.]+)%% of physical damage prevented from hits recently is regenerated as energy shield per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PhysicalDamageMitigatedEnergyShieldPseudoRecoup"), BASE, (nn(num, "perform arithmetic on local 'num'") * 4.0), None, 0, 0, &[])])
    }), lua_line: 4778 });
    v.push(PatternEntry { pat: "([%d%.]+)%% of physical damage prevented recently is regenerated as energy shield per second if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let thresh = ns(caps.cap[1], "index local 'thresh'");
        let influence = ns(caps.cap[2], "index local 'influence'");
        Some(vec![r#mod(n!("PhysicalDamageMitigatedEnergyShieldPseudoRecoup"), BASE, (nn(num, "perform arithmetic on local 'num'") * 4.0), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4779 });
    v.push(PatternEntry {
        pat: "cannot leech or regenerate mana",
        val: SpecialVal::Mods(vec![
            flag(n!("NoManaRegen"), None, 0, 0, &[]),
            flag(n!("CannotLeechMana"), None, 0, 0, &[]),
        ]),
        lua_line: 4782,
    });
    v.push(PatternEntry {
        pat: "right ring slot: you cannot regenerate mana",
        val: SpecialVal::Mods(vec![flag(
            n!("NoManaRegen"),
            None,
            0,
            0,
            &[tag!({ type = SlotNumber, num = 2.0 })],
        )]),
        lua_line: 4783,
    });
    v.push(PatternEntry {
        pat: "y?o?u? ?cannot recharge energy shield",
        val: SpecialVal::Mods(vec![flag(n!("NoEnergyShieldRecharge"), None, 0, 0, &[])]),
        lua_line: 4784,
    });
    v.push(PatternEntry {
        pat: "you cannot regenerate energy shield",
        val: SpecialVal::Mods(vec![flag(n!("NoEnergyShieldRegen"), None, 0, 0, &[])]),
        lua_line: 4785,
    });
    v.push(PatternEntry {
        pat: "cannot recharge or regenerate energy shield",
        val: SpecialVal::Mods(vec![
            flag(n!("NoEnergyShieldRecharge"), None, 0, 0, &[]),
            flag(n!("NoEnergyShieldRegen"), None, 0, 0, &[]),
        ]),
        lua_line: 4786,
    });
    v.push(PatternEntry {
        pat: "left ring slot: you cannot recharge or regenerate energy shield",
        val: SpecialVal::Mods(vec![
            flag(
                n!("NoEnergyShieldRecharge"),
                None,
                0,
                0,
                &[tag!({ type = SlotNumber, num = 1.0 })],
            ),
            flag(
                n!("NoEnergyShieldRegen"),
                None,
                0,
                0,
                &[tag!({ type = SlotNumber, num = 1.0 })],
            ),
        ]),
        lua_line: 4787,
    });
    v.push(PatternEntry {
        pat: "cannot gain energy shield",
        val: SpecialVal::Mods(vec![flag(n!("CannotGainEnergyShield"), None, 0, 0, &[])]),
        lua_line: 4788,
    });
    v.push(PatternEntry {
        pat: "cannot gain life",
        val: SpecialVal::Mods(vec![flag(n!("CannotGainLife"), None, 0, 0, &[])]),
        lua_line: 4789,
    });
    v.push(PatternEntry {
        pat: "cannot gain mana",
        val: SpecialVal::Mods(vec![flag(n!("CannotGainMana"), None, 0, 0, &[])]),
        lua_line: 4790,
    });
    v.push(PatternEntry {
        pat: "cannot recover life other than from leech",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotRecoverLifeOutsideLeech"), None, 0, 0, &[]),
            flag(n!("NoLifeRegen"), None, 0, 0, &[]),
        ]),
        lua_line: 4791,
    });
    v.push(PatternEntry {
        pat: "cannot gain energy shield during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotGainEnergyShield"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4795,
    });
    v.push(PatternEntry {
        pat: "cannot gain life during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotGainLife"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4796,
    });
    v.push(PatternEntry {
        pat: "cannot gain mana during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotGainMana"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4797,
    });
    v.push(PatternEntry {
        pat: "life that would be lost by taking damage is instead reserved",
        val: SpecialVal::Mods(vec![flag(n!("DamageInsteadReservesLife"), None, 0, 0, &[])]),
        lua_line: 4798,
    });
    v.push(PatternEntry {
        pat: "you have no armour or energy shield",
        val: SpecialVal::Mods(vec![
            r#mod(n!("Armour"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("EnergyShield"), MORE, -100.0, None, 0, 0, &[]),
        ]),
        lua_line: 4799,
    });
    v.push(PatternEntry {
        pat: "you have no armour or maximum energy shield",
        val: SpecialVal::Mods(vec![
            r#mod(n!("Armour"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("EnergyShield"), MORE, -100.0, None, 0, 0, &[]),
        ]),
        lua_line: 4803,
    });
    v.push(PatternEntry {
        pat: "defences are zero",
        val: SpecialVal::Mods(vec![
            r#mod(n!("Armour"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("EnergyShield"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("Evasion"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("Ward"), MORE, -100.0, None, 0, 0, &[]),
        ]),
        lua_line: 4807,
    });
    v.push(PatternEntry {
        pat: "you have no intelligence",
        val: SpecialVal::Mods(vec![r#mod(n!("Int"), MORE, -100.0, None, 0, 0, &[])]),
        lua_line: 4813,
    });
    v.push(PatternEntry {
        pat: "you have no dexterity",
        val: SpecialVal::Mods(vec![r#mod(n!("Dex"), MORE, -100.0, None, 0, 0, &[])]),
        lua_line: 4816,
    });
}

/// @pob Modules/ModParser.lua:4819-4889 h=8e583bd6
fn specialModList_4819(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "you have no strength",
        val: SpecialVal::Mods(vec![r#mod(n!("Str"), MORE, -100.0, None, 0, 0, &[])]),
        lua_line: 4819,
    });
    v.push(PatternEntry {
        pat: "physical damage reduction is zero",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("PhysicalDamageReduction"),
                OVERRIDE,
                0.0,
                None,
                0,
                0,
                &[],
            ),
            flag(
                n!("ArmourDoesNotApplyToPhysicalDamageTaken"),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 4822,
    });
    v.push(PatternEntry {
        pat: "elemental resistances are zero",
        val: SpecialVal::Mods(vec![
            r#mod(n!("FireResist"), OVERRIDE, 0.0, None, 0, 0, &[]),
            r#mod(n!("ColdResist"), OVERRIDE, 0.0, None, 0, 0, &[]),
            r#mod(n!("LightningResist"), OVERRIDE, 0.0, None, 0, 0, &[]),
        ]),
        lua_line: 4826,
    });
    v.push(PatternEntry {
        pat: "chaos resistance is zero",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ChaosResist"),
            OVERRIDE,
            0.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4831,
    });
    v.push(PatternEntry {
        pat: "nearby enemies' chaos resistance is (%d+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("ChaosResist"), OVERRIDE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4834,
    });
    v.push(PatternEntry {
        pat: "your maximum resistances are (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("FireResistMax"), OVERRIDE, num, None, 0, 0, &[]),
                r#mod(n!("ColdResistMax"), OVERRIDE, num, None, 0, 0, &[]),
                r#mod(n!("LightningResistMax"), OVERRIDE, num, None, 0, 0, &[]),
                r#mod(n!("ChaosResistMax"), OVERRIDE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 4837,
    });
    v.push(PatternEntry {
        pat: "fire resistance is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FireResist"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4843,
    });
    v.push(PatternEntry {
        pat: "cold resistance is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ColdResist"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4844,
    });
    v.push(PatternEntry {
        pat: "lightning resistance is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LightningResist"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4845,
    });
    v.push(PatternEntry {
        pat:
            "elemental resistances are capped by your highest maximum elemental resistance instead",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalResistMaxIsHighestResistMax"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4846,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have (%d+)%% increased fire and cold resistances",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("EnemyModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(n!("FireResist"), INC, num, None, 0, 0, &[])),
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
                            Value::from(r#mod(n!("ColdResist"), INC, num, None, 0, 0, &[])),
                        )],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 4847,
    });
    v.push(PatternEntry {
        pat: "nearby enemies are blinded while physical aegis is not depleted",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Blinded"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "PhysicalAegisDepleted", neg = true })],
        )]),
        lua_line: 4851,
    });
    v.push(PatternEntry {
        pat: "maximum energy shield is increased by chance to block spell damage",
        val: SpecialVal::Mods(vec![flag(
            n!("EnergyShieldIncreasedByChanceToBlockSpellDamage"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4852,
    });
    v.push(PatternEntry {
        pat: "maximum energy shield is increased by chaos resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("EnergyShieldIncreasedByChaosResistance"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4853,
    });
    v.push(PatternEntry {
        pat: "armour is increased by uncapped fire resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("ArmourIncreasedByUncappedFireRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4854,
    });
    v.push(PatternEntry {
        pat: "armour is increased by overcapped fire resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("ArmourIncreasedByOvercappedFireRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4855,
    });
    v.push(PatternEntry {
        pat: "minion life is increased by t?h?e?i?r? ?overcapped fire resistance",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(r#mod(
                        n!("Life"),
                        INC,
                        1.0,
                        None,
                        0,
                        0,
                        &[tag!({ type = PerStat, stat = "FireResistOverCap", div = 1.0 })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4856,
    });
    v.push(PatternEntry {
        pat: "totem life is increased by t?h?e?i?r? ?overcapped fire resistance",
        val: SpecialVal::Mods(vec![r#mod(
            n!("TotemLife"),
            INC,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = PerStat, stat = "TotemFireResistOverCap", div = 1.0 })],
        )]),
        lua_line: 4857,
    });
    v.push(PatternEntry {
        pat: "evasion rating is increased by uncapped cold resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("EvasionRatingIncreasedByUncappedColdRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4858,
    });
    v.push(PatternEntry {
        pat: "evasion rating is increased by overcapped cold resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("EvasionRatingIncreasedByOvercappedColdRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4859,
    });
    v.push(PatternEntry {
        pat: "reflects (%d+) physical damage to melee attackers",
        val: SpecialVal::Mods(vec![]),
        lua_line: 4860,
    });
    v.push(PatternEntry {
        pat: "ignore all movement penalties from armour",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:IgnoreMovementPenalties"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4861,
    });
    v.push(PatternEntry {
        pat: "gain armour equal to your reserved mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Armour"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = PerStat, stat = "ManaReserved", div = 1.0 })],
        )]),
        lua_line: 4862,
    });
    v.push(PatternEntry {
        pat: "gain ward instead of (%d+)%% of armour and evasion rating from equipped body armour",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("ConvertBodyArmourArmourEvasionToWard"), None, 0, 0, &[]),
                r#mod(
                    n!("BodyArmourArmourEvasionToWardPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 4863,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased armour per (%d+) reserved mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let mana = ns(caps.cap[1], "index local 'mana'");
            Some(vec![r#mod(
                n!("Armour"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "ManaReserved", div = tonumber(mana) })],
            )])
        }),
        lua_line: 4864,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned",
        val: SpecialVal::Mods(vec![flag(n!("StunImmune"), None, 0, 0, &[])]),
        lua_line: 4865,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned while bleeding",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Bleeding" })],
        )]),
        lua_line: 4866,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned when on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife" })],
        )]),
        lua_line: 4867,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned if you haven't been hit recently",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BeenHitRecently", neg = true })],
        )]),
        lua_line: 4868,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned if you have at least (%d+) crab barriers",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![flag(
                n!("StunImmune"),
                None,
                0,
                0,
                &[tag!({ type = StatThreshold, stat = "CrabBarriers", threshold = num })],
            )])
        }),
        lua_line: 4869,
    });
    v.push(PatternEntry { pat: "cannot be stunned if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let thresh = ns(caps.cap[0], "index local 'thresh'");
        let influence = ns(caps.cap[1], "index local 'influence'");
        Some(vec![flag(n!("StunImmune"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4870 });
    v.push(PatternEntry {
        pat: "cannot be blinded",
        val: SpecialVal::Mods(vec![
            flag(n!("Condition:CannotBeBlinded"), None, 0, 0, &[]),
            flag(n!("BlindImmune"), None, 0, 0, &[]),
        ]),
        lua_line: 4871,
    });
    v.push(PatternEntry {
        pat: "cannot be blinded while affected by precision",
        val: SpecialVal::Mods(vec![
            flag(
                n!("Condition:CannotBeBlinded"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByPrecision" })],
            ),
            flag(
                n!("BlindImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByPrecision" })],
            ),
        ]),
        lua_line: 4872,
    });
    v.push(PatternEntry {
        pat: "cannot be knocked back",
        val: SpecialVal::Mods(vec![flag(n!("KnockbackImmune"), None, 0, 0, &[])]),
        lua_line: 4873,
    });
    v.push(PatternEntry {
        pat: "cannot be shocked",
        val: SpecialVal::Mods(vec![flag(n!("ShockImmune"), None, 0, 0, &[])]),
        lua_line: 4874,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to shock",
        val: SpecialVal::Mods(vec![flag(n!("ShockImmune"), None, 0, 0, &[])]),
        lua_line: 4875,
    });
    v.push(PatternEntry {
        pat: "cannot be frozen",
        val: SpecialVal::Mods(vec![flag(n!("FreezeImmune"), None, 0, 0, &[])]),
        lua_line: 4876,
    });
    v.push(PatternEntry { pat: "cannot be frozen if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let thresh = ns(caps.cap[0], "index local 'thresh'");
        let influence = ns(caps.cap[1], "index local 'influence'");
        Some(vec![flag(n!("FreezeImmune"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4877 });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to freeze",
        val: SpecialVal::Mods(vec![flag(n!("FreezeImmune"), None, 0, 0, &[])]),
        lua_line: 4878,
    });
    v.push(PatternEntry {
        pat: "cannot be chilled",
        val: SpecialVal::Mods(vec![flag(n!("ChillImmune"), None, 0, 0, &[])]),
        lua_line: 4879,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to chill",
        val: SpecialVal::Mods(vec![flag(n!("ChillImmune"), None, 0, 0, &[])]),
        lua_line: 4880,
    });
    v.push(PatternEntry {
        pat: "cannot be ignited",
        val: SpecialVal::Mods(vec![flag(n!("IgniteImmune"), None, 0, 0, &[])]),
        lua_line: 4881,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to ignite",
        val: SpecialVal::Mods(vec![flag(n!("IgniteImmune"), None, 0, 0, &[])]),
        lua_line: 4882,
    });
    v.push(PatternEntry {
        pat: "immune to ignite while affected by purity of fire",
        val: SpecialVal::Mods(vec![flag(
            n!("IgniteImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByPurityofFire" })],
        )]),
        lua_line: 4883,
    });
    v.push(PatternEntry {
        pat: "immune to freeze while affected by purity of ice",
        val: SpecialVal::Mods(vec![flag(
            n!("FreezeImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByPurityofIce" })],
        )]),
        lua_line: 4884,
    });
    v.push(PatternEntry {
        pat: "immune to shock while affected by purity of lightning",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByPurityofLightning" })],
        )]),
        lua_line: 4885,
    });
    v.push(PatternEntry {
        pat: "critical strikes against you do not inherently inflict elemental ailments",
        val: SpecialVal::Mods(vec![flag(
            n!("CritsOnYouDontAlwaysApplyElementalAilments"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4886,
    });
    v.push(PatternEntry { pat: "cannot be ignited while at maximum endurance charges", val: SpecialVal::Mods(vec![flag(n!("IgniteImmune"), None, 0, 0, &[tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMax" }), tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])]), lua_line: 4887 });
    v.push(PatternEntry {
        pat: "grants immunity to ignite for (%d+) seconds if used while ignited",
        val: SpecialVal::Mods(vec![flag(
            n!("IgniteImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4888,
    });
    v.push(PatternEntry {
        pat: "grants immunity to bleeding for (%d+) seconds if used while bleeding",
        val: SpecialVal::Mods(vec![flag(
            n!("BleedImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4889,
    });
}

/// @pob Modules/ModParser.lua:4890-4966 h=8136264c
fn specialModList_4890(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "grants immunity to corrupted blood for (%d+) seconds if used while affected by corrupted blood", val: SpecialVal::Mods(vec![flag(n!("CorruptedBloodImmune"), None, 0, 0, &[tag!({ type = Condition, var = "UsingFlask" })])]), lua_line: 4890 });
    v.push(PatternEntry {
        pat: "grants immunity to poison for (%d+) seconds if used while poisoned",
        val: SpecialVal::Mods(vec![flag(
            n!("PoisonImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4891,
    });
    v.push(PatternEntry {
        pat: "grants immunity to freeze for (%d+) seconds if used while frozen",
        val: SpecialVal::Mods(vec![flag(
            n!("FreezeImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4892,
    });
    v.push(PatternEntry {
        pat: "grants immunity to chill for (%d+) seconds if used while chilled",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4893,
    });
    v.push(PatternEntry {
        pat: "grants immunity to shock for (%d+) seconds if used while shocked",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4894,
    });
    v.push(PatternEntry { pat: "cannot be chilled while at maximum frenzy charges", val: SpecialVal::Mods(vec![flag(n!("ChillImmune"), None, 0, 0, &[tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })])]), lua_line: 4895 });
    v.push(PatternEntry { pat: "cannot be shocked while at maximum power charges", val: SpecialVal::Mods(vec![flag(n!("ShockImmune"), None, 0, 0, &[tag!({ type = StatThreshold, stat = "PowerCharges", thresholdStat = "PowerChargesMax" })])]), lua_line: 4896 });
    v.push(PatternEntry { pat: "you cannot be shocked while at maximum endurance charges", val: SpecialVal::Mods(vec![flag(n!("ShockImmune"), None, 0, 0, &[tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMax" })])]), lua_line: 4897 });
    v.push(PatternEntry {
        pat: "you cannot be shocked while chilled",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Chilled" })],
        )]),
        lua_line: 4898,
    });
    v.push(PatternEntry {
        pat: "you cannot be shocked while frozen",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Frozen" })],
        )]),
        lua_line: 4899,
    });
    v.push(PatternEntry {
        pat: "cannot be shocked while chilled",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Chilled" })],
        )]),
        lua_line: 4900,
    });
    v.push(PatternEntry {
        pat: "cannot be shocked if intelligence is higher than strength",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "IntHigherThanStr" })],
        )]),
        lua_line: 4901,
    });
    v.push(PatternEntry {
        pat: "cannot be frozen if dexterity is higher than intelligence",
        val: SpecialVal::Mods(vec![flag(
            n!("FreezeImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "DexHigherThanInt" })],
        )]),
        lua_line: 4902,
    });
    v.push(PatternEntry {
        pat: "cannot be frozen if energy shield recharge has started recently",
        val: SpecialVal::Mods(vec![flag(
            n!("FreezeImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "EnergyShieldRechargeRecently" })],
        )]),
        lua_line: 4903,
    });
    v.push(PatternEntry {
        pat: "cannot be ignited if strength is higher than dexterity",
        val: SpecialVal::Mods(vec![flag(
            n!("IgniteImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "StrHigherThanDex" })],
        )]),
        lua_line: 4904,
    });
    v.push(PatternEntry {
        pat: "cannot be chilled while burning",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Burning" })],
        )]),
        lua_line: 4905,
    });
    v.push(PatternEntry {
        pat: "cannot be chilled while you have onslaught",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Onslaught" })],
        )]),
        lua_line: 4906,
    });
    v.push(PatternEntry {
        pat: "cannot be chilled during onslaught",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Onslaught" })],
        )]),
        lua_line: 4907,
    });
    v.push(PatternEntry {
        pat: "cannot be frozen or chilled if you've used a fire skill recently",
        val: SpecialVal::Mods(vec![
            flag(
                n!("FreezeImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsedFireSkillRecently" })],
            ),
            flag(
                n!("ChillImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsedFireSkillRecently" })],
            ),
        ]),
        lua_line: 4908,
    });
    v.push(PatternEntry {
        pat: "cannot be inflicted with bleeding",
        val: SpecialVal::Mods(vec![flag(n!("BleedImmune"), None, 0, 0, &[])]),
        lua_line: 4909,
    });
    v.push(PatternEntry {
        pat: "bleeding cannot be inflicted on you",
        val: SpecialVal::Mods(vec![flag(n!("BleedImmune"), None, 0, 0, &[])]),
        lua_line: 4910,
    });
    v.push(PatternEntry {
        pat: "you are immune to bleeding",
        val: SpecialVal::Mods(vec![flag(n!("BleedImmune"), None, 0, 0, &[])]),
        lua_line: 4911,
    });
    v.push(PatternEntry {
        pat: "immune to bleeding if equipped helmet has higher armour than evasion rating",
        val: SpecialVal::Mods(vec![flag(
            n!("BleedImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "HelmetArmourHigherThanEvasion" })],
        )]),
        lua_line: 4912,
    });
    v.push(PatternEntry {
        pat: "immune to poison if equipped helmet has higher evasion rating than armour",
        val: SpecialVal::Mods(vec![flag(
            n!("PoisonImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "HelmetEvasionHigherThanArmour" })],
        )]),
        lua_line: 4913,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to bleeding and corrupted blood during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("BleedImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("CorruptedBloodImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 4914,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to poison",
        val: SpecialVal::Mods(vec![flag(n!("PoisonImmune"), None, 0, 0, &[])]),
        lua_line: 4918,
    });
    v.push(PatternEntry {
        pat: "cannot be poisoned",
        val: SpecialVal::Mods(vec![flag(n!("PoisonImmune"), None, 0, 0, &[])]),
        lua_line: 4919,
    });
    v.push(PatternEntry {
        pat: "cannot be poisoned while bleeding",
        val: SpecialVal::Mods(vec![flag(
            n!("PoisonImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Bleeding" })],
        )]),
        lua_line: 4920,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to poison during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("PoisonImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4921,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to shock during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ShockImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4922,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to freeze and chill during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("FreezeImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("ChillImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 4923,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to freeze and chill while ignited",
        val: SpecialVal::Mods(vec![
            flag(
                n!("FreezeImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Ignited" })],
            ),
            flag(
                n!("ChillImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Ignited" })],
            ),
        ]),
        lua_line: 4927,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to ignite during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("IgniteImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4931,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to bleeding during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("BleedImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4932,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to curses during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("CurseImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 4933,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to curses while channelling",
        val: SpecialVal::Mods(vec![flag(
            n!("CurseImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Channelling" })],
        )]),
        lua_line: 4934,
    });
    v.push(PatternEntry { pat: "you are unaffected by (.+) if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let ailment = ns(caps.cap[0], "index local 'ailment'");
        let curse = ns(caps.cap[1], "index local 'curse'");
        Some(vec![r#mod(cat!("Self", gsub_fn(ailment, "^%l", |c| Some(upper(c[0].as_str())), None).0, "Effect"), MORE, -100.0, None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) }), tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])])
    }), lua_line: 4935 });
    v.push(PatternEntry { pat: "immun[ei]t?y? to (%a-)s? if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let ailment = ns(caps.cap[0], "index local 'ailment'");
        let curse = ns(caps.cap[1], "index local 'curse'");
        Some(vec![flag(cat!(gsub_fn(ailment, "^%l", |c| Some(upper(c[0].as_str())), None).0, "Immune"), None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 4938 });
    // typo / old wording change
    v.push(PatternEntry { pat: "when you kill an enemy affected by a non%-aura hex, become immune to curses for remaining hex duration", val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeCurseImmune"), None, 0, 0, &[])]), lua_line: 4941 });
    v.push(PatternEntry { pat: "when you kill an enemy cursed with a non%-aura hex, become immune to curses for remaining hex duration", val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeCurseImmune"), None, 0, 0, &[])]), lua_line: 4944 });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to freeze, chill, curses and stuns during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("FreezeImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("ChillImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("CurseImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("StunImmune"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 4947,
    });
    // This mod doesn't work the way it should. It prevents self-chill among other issues.
    // Since we don't currently really do anything with enemy ailment infliction, this should probably be removed
    // ["cursed enemies cannot inflict elemental ailments on you"] = {
    // mod("AvoidElementalAilments", "BASE", 100, { type = "ActorCondition", actor = "enemy", var = "Cursed" }, { type = "GlobalEffect", effectType = "Global", unscalable = true }),
    // },
    v.push(PatternEntry {
        pat: "enemies inflict elemental ailments on you instead of nearby allies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("onlyAllies"), Value::from(true)),
                    (
                        n!("mod"),
                        Value::from(flag(n!("ElementalAilmentImmune"), None, 0, 0, &[])),
                    ),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4958,
    });
    v.push(PatternEntry {
        pat: "unaffected by curses",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 4959,
    });
    v.push(PatternEntry {
        pat: "you are immune to curses",
        val: SpecialVal::Mods(vec![flag(n!("CurseImmune"), None, 0, 0, &[])]),
        lua_line: 4960,
    });
    v.push(PatternEntry {
        pat: "unaffected by curses while affected by zealotry",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "AffectedByZealotry" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4961,
    });
    v.push(PatternEntry {
        pat: "unaffected by vulnerability while affected by determination",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Vulnerability" }),
                tag!({ type = Condition, var = "AffectedByDetermination" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4962,
    });
    v.push(PatternEntry {
        pat: "unaffected by enfeeble while affected by grace",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Enfeeble" }),
                tag!({ type = Condition, var = "AffectedByGrace" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4963,
    });
    v.push(PatternEntry {
        pat: "unaffected by temporal chains while affected by haste",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Temporal Chains" }),
                tag!({ type = Condition, var = "AffectedByHaste" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4964,
    });
    v.push(PatternEntry {
        pat: "unaffected by elemental weakness while affected by purity of elements",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Elemental Weakness" }),
                tag!({ type = Condition, var = "AffectedByPurityofElements" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4965,
    });
    v.push(PatternEntry {
        pat: "unaffected by flammability while affected by purity of fire",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Flammability" }),
                tag!({ type = Condition, var = "AffectedByPurityofFire" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4966,
    });
}

/// @pob Modules/ModParser.lua:4967-4988 h=1adf9500
fn specialModList_4967(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "unaffected by frostbite while affected by purity of ice",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Frostbite" }),
                tag!({ type = Condition, var = "AffectedByPurityofIce" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4967,
    });
    v.push(PatternEntry {
        pat: "unaffected by conductivity while affected by purity of lightning",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CurseEffectOnSelf"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Conductivity" }),
                tag!({ type = Condition, var = "AffectedByPurityofLightning" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4968,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to curses while you have at least (%d+) rage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![flag(
                n!("CurseImmune"),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "Rage", threshold = num })],
            )])
        }),
        lua_line: 4969,
    });
    v.push(PatternEntry {
        pat: "you cannot be cursed with silence",
        val: SpecialVal::Mods(vec![flag(n!("SilenceImmune"), None, 0, 0, &[])]),
        lua_line: 4970,
    });
    v.push(PatternEntry {
        pat: "unaffected by ignite",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfIgniteEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4971,
    });
    v.push(PatternEntry {
        pat: "unaffected by chill",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfChillEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4972,
    });
    v.push(PatternEntry {
        pat: "unaffected by chill while leeching mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfChillEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LeechingMana" })],
        )]),
        lua_line: 4973,
    });
    v.push(PatternEntry {
        pat: "unaffected by chill while channelling",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfChillEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Channelling" })],
        )]),
        lua_line: 4974,
    });
    v.push(PatternEntry {
        pat: "unaffected by freeze",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfFreezeEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4975,
    });
    v.push(PatternEntry {
        pat: "unaffected by shock",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfShockEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4976,
    });
    v.push(PatternEntry {
        pat: "unaffected by shock while leeching energy shield",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfShockEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LeechingEnergyShield" })],
        )]),
        lua_line: 4977,
    });
    v.push(PatternEntry {
        pat: "unaffected by shock while channelling",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfShockEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Channelling" })],
        )]),
        lua_line: 4978,
    });
    v.push(PatternEntry {
        pat: "unaffected by scorch",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfScorchEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4979,
    });
    v.push(PatternEntry {
        pat: "unaffected by brittle",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfBrittleEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4980,
    });
    v.push(PatternEntry {
        pat: "unaffected by sap",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfSapEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 4981,
    });
    v.push(PatternEntry {
        pat: "unaffected by damaging ailments",
        val: SpecialVal::Mods(vec![
            r#mod(n!("SelfBleedEffect"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("SelfIgniteEffect"), MORE, -100.0, None, 0, 0, &[]),
            r#mod(n!("SelfPoisonEffect"), MORE, -100.0, None, 0, 0, &[]),
        ]),
        lua_line: 4982,
    });
    v.push(PatternEntry {
        pat: "unaffected by bleeding while affected by malevolence",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfBleedEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "AffectedByMalevolence" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4987,
    });
    v.push(PatternEntry {
        pat: "unaffected by poison while affected by malevolence",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfPoisonEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "AffectedByMalevolence" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 4988,
    });
}

/// The entries of ModParser.lua 4037-4988 (618 entries), in source order (the blocks above
/// carry the provenance markers).
pub fn entries() -> Vec<PatternEntry<SpecialVal>> {
    let mut v: Vec<PatternEntry<SpecialVal>> = Vec::with_capacity(618);
    specialModList_4037(&mut v);
    specialModList_4119(&mut v);
    specialModList_4196(&mut v);
    specialModList_4316(&mut v);
    specialModList_4373(&mut v);
    specialModList_4432(&mut v);
    specialModList_4530(&mut v);
    specialModList_4592(&mut v);
    specialModList_4682(&mut v);
    specialModList_4744(&mut v);
    specialModList_4819(&mut v);
    specialModList_4890(&mut v);
    specialModList_4967(&mut v);
    v
}
