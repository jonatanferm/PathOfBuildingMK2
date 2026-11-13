//! @pob-file Modules/ModParser.lua @16de4b82d
//! specialModList entries, ModParser.lua 4989-5942 (calc doc §8; provenance per block of about 50
//! entries). `special_a`..`special_d` concatenate, in source order, into `specialModList`.
//!
//! Owner: MP6. The entries were translated by scripts/port/lua2rs (unit T1) and checked
//! against PoB's own table under LuaJIT (crates/pob-oracle/tests/MP6.rs); the 11 closures
//! lua2rs could not translate (5104, 5355, 5509, 5830-5853) are hand-ported (marked
//! "hand-ported"). One `PatternEntry` per Lua key, in source order (560 entries: 261 static,
//! 299 closures; no key is defined twice in this range). The parse-level differences of this
//! range (5509, 5686/5689/5692, 5733/5734/5741) are `LUA-QUIRK`s listed in
//! docs/port/known_diffs.toml (MP6-1..3).
//! Closure parameters follow the specialModList call shape (ModParser.lua:6662-6663; calc doc
//! §6 / CCR-0006 §5): #1 = `caps.num` (`tonumber(cap[1])`), #k>=2 = `caps.cap[k-2]`.
#![allow(
    non_snake_case,
    non_upper_case_globals,
    unused_variables,
    unused_mut,
    unused_parens,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]
use crate::helpers::*;
use crate::types::{Caps, ModSpec, PatternEntry, PatternList, SpecialVal, TagVal};
use pob_gamedata::{
    data, tag, AStr, KeywordFlag, Mod, ModFlag, ModType, ModType::*, SkillType, Sockets, Tag,
    Value, ValueTable,
};
use pob_lua::bit::{band, bnot, bor};
use pob_lua::format::{tonumber, tostring_num};
use pob_lua::math::{lmod, lpow, m_abs, m_ceil, m_floor, m_huge, m_max, m_min, round};
use pob_lua::pattern::{gsub, gsub_fn};
use pob_lua::string::{firstToUpper, len, lower, sub, upper};
use pob_lua::{cat, lfmt, lua_error, n, nm, Name};
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

/// The body shared by the eight timeless-jewel closures of 5830-5853 (hand-ported; each Lua
/// closure is `function(num, _, name) return { mod("JewelData", "LIST", { key = "conqueredBy",
/// value = { id = num, conqueror = conquerorList[name:lower()] } }) } end`).
fn conquered_by(caps: &Caps) -> Option<Vec<Mod>> {
    let num = caps.num;
    let name = ns(caps.cap[1], "index local 'name'");
    let name = lower(name);
    let conqueror = conquerorList
        .iter()
        .find(|(k, _)| k.as_str() == name)
        .map_or(Value::Nil, |(_, v)| v.clone());
    Some(vec![r#mod(
        n!("JewelData"),
        LIST,
        vtab(
            vec![],
            vec![
                (n!("key"), Value::from("conqueredBy")),
                (
                    n!("value"),
                    vtab(
                        vec![],
                        vec![(n!("id"), Value::from(num)), (n!("conqueror"), conqueror)],
                    ),
                ),
            ],
        ),
        None,
        0,
        0,
        &[],
    )])
}

/// @pob Modules/ModParser.lua:4989-5070 h=e668f8f5
fn specialModList_4989(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "unaffected by (.+) if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let ailment = ns(caps.cap[0], "index local 'ailment'");
        let thresh = ns(caps.cap[1], "index local 'thresh'");
        let influence = ns(caps.cap[2], "index local 'influence'");
        Some(vec![r#mod(cat!("Self", gsub_fn(ailment, "^%l", |c| Some(upper(c[0].as_str())), None).0, "Effect"), MORE, -100.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 4989 });
    v.push(PatternEntry {
        pat: "the effect of chill on you is reversed",
        val: SpecialVal::Mods(vec![
            flag(n!("SelfChillEffectIsReversed"), None, 0, 0, &[]),
            r#mod(
                n!("Dummy"),
                DUMMY,
                1.0,
                Some(""),
                0,
                0,
                &[tag!({ type = Condition, var = "Chilled" })],
            ),
        ]),
        lua_line: 4992,
    });
    v.push(PatternEntry {
        pat: "your movement speed is (%d+)%% of its base value",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MovementSpeed"),
                OVERRIDE,
                (nn(num, "perform arithmetic on local 'num'") / 100.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4993,
    });
    v.push(PatternEntry {
        pat: "action speed cannot be modified to below (%d+)%% base value",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinimumActionSpeed"),
                MAX,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 4994,
    });
    v.push(PatternEntry { pat: "action speed cannot be slowed below base value if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let curse = ns(caps.cap[0], "index local 'curse'");
        Some(vec![r#mod(n!("MinimumActionSpeed"), MAX, 100.0, None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 4995 });
    v.push(PatternEntry { pat: "action speed cannot be modified to below base value if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let curse = ns(caps.cap[0], "index local 'curse'");
        Some(vec![r#mod(n!("MinimumActionSpeed"), MAX, 100.0, None, 0, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 4998 });
    v.push(PatternEntry {
        pat: "action speed cannot be modified to below base value while ignited",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinimumActionSpeed"),
            MAX,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Ignited" })],
        )]),
        lua_line: 5001,
    });
    v.push(PatternEntry { pat: "nearby allies' action speed cannot be modified to below base value", val: SpecialVal::Mods(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("onlyAllies"), Value::from(true)), (n!("mod"), Value::from(r#mod(n!("MinimumActionSpeed"), MAX, 100.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])))]), None, 0, 0, &[])]), lua_line: 5002 });
    v.push(PatternEntry {
        pat: "armour also applies to lightning damage taken from hits",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ArmourAppliesToLightningDamageTaken"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5003,
    });
    v.push(PatternEntry {
        pat: "lightning resistance does not affect lightning damage taken",
        val: SpecialVal::Mods(vec![flag(
            n!("SelfIgnoreLightningResistance"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5004,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased maximum life and reduced fire resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("Life"), INC, num, None, 0, 0, &[]),
                r#mod(
                    n!("FireResist"),
                    INC,
                    (-nn(num, "perform arithmetic on local 'num'")),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5005,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased maximum mana and reduced cold resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("Mana"), INC, num, None, 0, 0, &[]),
                r#mod(
                    n!("ColdResist"),
                    INC,
                    (-nn(num, "perform arithmetic on local 'num'")),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5009,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased global maximum energy shield and reduced lightning resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("EnergyShield"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Global })],
                ),
                r#mod(
                    n!("LightningResist"),
                    INC,
                    (-nn(num, "perform arithmetic on local 'num'")),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5013,
    });
    v.push(PatternEntry {
        pat: "phasing while on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:Phasing"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife" })],
        )]),
        lua_line: 5017,
    });
    v.push(PatternEntry {
        pat: "cannot be ignited while on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("IgniteImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife" })],
        )]),
        lua_line: 5018,
    });
    v.push(PatternEntry {
        pat: "ward does not break during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("Condition:WardNotBreak"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("Condition:UnbrokenWard"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 5019,
    });
    v.push(PatternEntry {
        pat: "stun threshold is based on energy shield instead of life",
        val: SpecialVal::Mods(vec![
            flag(
                n!("StunThresholdBasedOnEnergyShieldInsteadOfLife"),
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("StunThresholdEnergyShieldPercent"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 5020,
    });
    v.push(PatternEntry {
        pat: "stun threshold is based on (%d+)%% of your energy shield instead of life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(
                    n!("StunThresholdBasedOnEnergyShieldInsteadOfLife"),
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("StunThresholdEnergyShieldPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5024,
    });
    v.push(PatternEntry {
        pat: "stun threshold is based on (%d+)%% of your mana instead of life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("StunThresholdBasedOnManaInsteadOfLife"), None, 0, 0, &[]),
                r#mod(n!("StunThresholdManaPercent"), BASE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 5028,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of your energy shield is added to your stun threshold",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("AddESToStunThreshold"), None, 0, 0, &[]),
                r#mod(n!("ESToStunThresholdPercent"), BASE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 5032,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased armour per second you've been stationary, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("Armour"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "StationarySeconds", limit = (snum(limit, "perform arithmetic on a string") / nn(num, "perform arithmetic on local 'num'")) }), tag!({ type = Condition, var = "Stationary" })])])
    }), lua_line: 5036 });
    v.push(PatternEntry { pat: "(%d+)%% of damage from hits is taken from your r?a?i?s?e?d? ?spectres' life before you", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("takenFromSpectresBeforeYou"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 5039 });
    v.push(PatternEntry {
        pat: "(%d+)%% of damage from hits is taken from your nearest totem's life before you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("takenFromTotemsBeforeYou"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveTotem" })],
            )])
        }),
        lua_line: 5040,
    });
    v.push(PatternEntry {
        pat:
            "(%d+)%% of damage from hits is taken from void spawns' life before you per void spawn",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("takenFromVoidSpawnBeforeYou"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "ActiveVoidSpawnLimit" })],
            )])
        }),
        lua_line: 5041,
    });
    v.push(PatternEntry {
        pat: "(%a+) resistance cannot be penetrated",
        val: SpecialVal::Fn(|caps: &Caps| {
            let res = ns(caps.cap[0], "index local 'res'");
            Some(vec![flag(
                cat!(
                    "EnemyCannotPen",
                    gsub_fn(res, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                    "Resistance"
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5042,
    });
    // Knockback
    v.push(PatternEntry {
        pat: "cannot knock enemies back",
        val: SpecialVal::Mods(vec![flag(n!("CannotKnockback"), None, 0, 0, &[])]),
        lua_line: 5044,
    });
    v.push(PatternEntry {
        pat: "knocks back enemies if you get a critical strike with a staff",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyKnockbackChance"),
            BASE,
            100.0,
            None,
            ModFlag::Staff,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 5045,
    });
    v.push(PatternEntry {
        pat: "knocks back enemies if you get a critical strike with a bow",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyKnockbackChance"),
            BASE,
            100.0,
            None,
            ModFlag::Bow,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 5046,
    });
    v.push(PatternEntry {
        pat: "bow knockback at close range",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyKnockbackChance"),
            BASE,
            100.0,
            None,
            ModFlag::Bow,
            0,
            &[tag!({ type = Condition, var = "AtCloseRange" })],
        )]),
        lua_line: 5047,
    });
    v.push(PatternEntry {
        pat: "adds knockback during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyKnockbackChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5048,
    });
    v.push(PatternEntry {
        pat: "adds knockback to melee attacks during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyKnockbackChance"),
            BASE,
            100.0,
            None,
            ModFlag::Melee,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5049,
    });
    v.push(PatternEntry {
        pat: "knockback direction is reversed",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyKnockbackDistance"),
            MORE,
            -200.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5050,
    });
    // Culling
    v.push(PatternEntry {
        pat: "culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 5052,
    });
    v.push(PatternEntry {
        pat: "culling strike with melee weapons",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            ModFlag::WeaponMelee,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 5053,
    });
    v.push(PatternEntry {
        pat: "melee weapon attacks have culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            bor(ModFlag::Attack, ModFlag::WeaponMelee),
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 5054,
    });
    v.push(PatternEntry {
        pat: "culling strike during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "UsingFlask" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 5055,
    });
    v.push(PatternEntry {
        pat: "hits with this weapon have culling strike against bleeding enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })],
        )]),
        lua_line: 5056,
    });
    v.push(PatternEntry {
        pat: "you have culling strike against cursed enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
        )]),
        lua_line: 5057,
    });
    v.push(PatternEntry {
        pat: "critical strikes have culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CriticalCullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5058,
    });
    v.push(PatternEntry {
        pat: "your critical strikes have culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CriticalCullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5059,
    });
    v.push(PatternEntry {
        pat: "your spells have culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            ModFlag::Spell,
            0,
            &[],
        )]),
        lua_line: 5060,
    });
    v.push(PatternEntry {
        pat: "bow attacks have culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            bor(ModFlag::Attack, ModFlag::Bow),
            0,
            &[],
        )]),
        lua_line: 5061,
    });
    v.push(PatternEntry {
        pat: "culling strike against burning enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Burning" })],
        )]),
        lua_line: 5062,
    });
    v.push(PatternEntry {
        pat: "culling strike against frozen enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })],
        )]),
        lua_line: 5063,
    });
    v.push(PatternEntry {
        pat: "culling strike against marked enemy",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CullPercent"),
            MAX,
            10.0,
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })],
        )]),
        lua_line: 5064,
    });
    v.push(PatternEntry {
        pat: "nearby allies have culling strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("onlyAllies"), Value::from(true)),
                    (
                        n!("mod"),
                        Value::from(r#mod(n!("CullPercent"), MAX, 10.0, None, 0, 0, &[])),
                    ),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5065,
    });
    v.push(PatternEntry {
        pat: "hits that stun enemies have culling strike",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:maceMasteryStunCullSpecced"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5066,
    });
    // Intimidate
    v.push(PatternEntry {
        pat: "permanently intimidate enemies on block",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BlockedRecently" })],
        )]),
        lua_line: 5068,
    });
    v.push(PatternEntry { pat: "with a murderous eye jewel socketed, intimidate enemies for (%d) seconds on hit with attacks", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "HaveMurderousEyeJewelIn{SlotName}" })])]), lua_line: 5069 });
    v.push(PatternEntry {
        pat: "enemies taunted by your warcries are intimidated",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("Condition:Intimidated"),
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
        )]),
        lua_line: 5070,
    });
}

/// @pob Modules/ModParser.lua:5071-5160 h=7a435c3c
fn specialModList_5071(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "intimidate enemies for (%d+) seconds on block while holding a shield",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "BlockedRecently" }),
                tag!({ type = Condition, var = "UsingShield" }),
            ],
        )]),
        lua_line: 5071,
    });
    v.push(PatternEntry { pat: "intimidate enemies on hit if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let curse = ns(caps.cap[0], "index local 'curse'");
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])))]), None, ModFlag::Hit, 0, &[tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 5072 });
    v.push(PatternEntry { pat: "intimidate enemies for (%d+) seconds on hit with attacks while at maximum endurance charges", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMax" }), tag!({ type = Condition, var = "HitRecently" })])]), lua_line: 5075 });
    // MultiplierThreshold is on RageStacks because Rage is only set in CalcPerform if Condition:CanGainRage is true, Bear's Girdle does not flag CanGainRage
    v.push(PatternEntry {
        pat: "nearby enemies are intimidated while you have rage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = MultiplierThreshold, var = "RageStack", threshold = 1.0 })],
        )]),
        lua_line: 5078,
    });
    v.push(PatternEntry {
        pat: "your hits intimidate enemies for (%d+) seconds while you are using pride",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Intimidated"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByPride" })],
        )]),
        lua_line: 5082,
    });
    // Flasks
    v.push(PatternEntry {
        pat: "flasks do not apply to you",
        val: SpecialVal::Mods(vec![flag(n!("FlasksDoNotApplyToPlayer"), None, 0, 0, &[])]),
        lua_line: 5084,
    });
    v.push(PatternEntry { pat: "flasks apply to your zombies and spectres", val: SpecialVal::Mods(vec![flag(n!("FlasksApplyToMinion"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Raise Zombie"), n!("Raise Spectre")][..], includeTransfigured = true })])]), lua_line: 5085 });
    v.push(PatternEntry { pat: "flasks apply to your raised zombies and spectres", val: SpecialVal::Mods(vec![flag(n!("FlasksApplyToMinion"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Raise Zombie"), n!("Raise Spectre")][..], includeTransfigured = true })])]), lua_line: 5086 });
    v.push(PatternEntry { pat: "flasks you use apply to your raised zombies and spectres", val: SpecialVal::Mods(vec![flag(n!("FlasksApplyToMinion"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Raise Zombie"), n!("Raise Spectre")][..], includeTransfigured = true })])]), lua_line: 5087 });
    v.push(PatternEntry {
        pat: "your minions use your flasks when summoned",
        val: SpecialVal::Mods(vec![flag(n!("FlasksApplyToMinion"), None, 0, 0, &[])]),
        lua_line: 5088,
    });
    v.push(PatternEntry { pat: "recover an additional (%d+)%% of flask's life recovery amount over 10 seconds if used while not on full life", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FlaskAdditionalLifeRecovery"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 5089 });
    v.push(PatternEntry {
        pat: "creates a smoke cloud on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5092,
    });
    v.push(PatternEntry {
        pat: "creates chilled ground on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5093,
    });
    v.push(PatternEntry {
        pat: "creates consecrated ground on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5094,
    });
    v.push(PatternEntry {
        pat: "removes bleeding on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5095,
    });
    v.push(PatternEntry {
        pat: "removes burning on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5096,
    });
    v.push(PatternEntry {
        pat: "removes all burning when used",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5097,
    });
    v.push(PatternEntry {
        pat: "removes curses on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5098,
    });
    v.push(PatternEntry {
        pat: "removes freeze and chill on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5099,
    });
    v.push(PatternEntry {
        pat: "removes poison on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5100,
    });
    v.push(PatternEntry {
        pat: "removes shock on use",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5101,
    });
    v.push(PatternEntry {
        pat: "g?a?i?n? ?unholy might during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("Condition:UnholyMight"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
            flag(
                n!("Condition:CanWither"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            ),
        ]),
        lua_line: 5102,
    });
    v.push(PatternEntry {
        pat: "zealot's oath during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ZealotsOath"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5103,
    });
    // hand-ported (lua2rs: value-operand: orexpr); the Lua:
    // |     ["grants level (%d+) (.+) curse aura during f?l?a?s?k? ?effect"] = function(num, _, skill) return { mod("ExtraCurse", "LIST", { skillId = gemIdLookup[skill:gsub(" skill","")] or "Unknown", level = num }, { type = "Condition", var = "UsingFlask" }) } end,
    v.push(PatternEntry {
        pat: "grants level (%d+) (.+) curse aura during f?l?a?s?k? ?effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            // hand-ported
            let num = caps.num;
            let skill = ns(caps.cap[1], "index local 'skill'");
            Some(vec![r#mod(
                n!("ExtraCurse"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (
                            n!("skillId"),
                            Value::from(
                                gemIdLookup(&gsub(skill, " skill", "", None).0)
                                    .unwrap_or(n!("Unknown")),
                            ),
                        ),
                        (n!("level"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingFlask" })],
            )])
        }),
        lua_line: 5104,
    });
    v.push(PatternEntry { pat: "shocks nearby enemies during f?l?a?s?k? ?effect, causing (%d+)%% increased damage taken", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ShockOverride"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "UsingFlask" })])])
    }), lua_line: 5105 });
    v.push(PatternEntry { pat: "during f?l?a?s?k? ?effect, (%d+)%% reduced damage taken of each element for which your uncapped elemental resistance is lowest", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LightningDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = StatThreshold, stat = "LightningResistTotal", thresholdStat = "ColdResistTotal", upper = true }), tag!({ type = StatThreshold, stat = "LightningResistTotal", thresholdStat = "FireResistTotal", upper = true })]), r#mod(n!("ColdDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = StatThreshold, stat = "ColdResistTotal", thresholdStat = "LightningResistTotal", upper = true }), tag!({ type = StatThreshold, stat = "ColdResistTotal", thresholdStat = "FireResistTotal", upper = true })]), r#mod(n!("FireDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = StatThreshold, stat = "FireResistTotal", thresholdStat = "LightningResistTotal", upper = true }), tag!({ type = StatThreshold, stat = "FireResistTotal", thresholdStat = "ColdResistTotal", upper = true })])])
    }), lua_line: 5108 });
    v.push(PatternEntry { pat: "during f?l?a?s?k? ?effect, damage penetrates (%d+)%% o?f? ?resistance of each element for which your uncapped elemental resistance is highest", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LightningPenetration"), BASE, num, None, 0, 0, &[tag!({ type = StatThreshold, stat = "LightningResistTotal", thresholdStat = "ColdResistTotal" }), tag!({ type = StatThreshold, stat = "LightningResistTotal", thresholdStat = "FireResistTotal" })]), r#mod(n!("ColdPenetration"), BASE, num, None, 0, 0, &[tag!({ type = StatThreshold, stat = "ColdResistTotal", thresholdStat = "LightningResistTotal" }), tag!({ type = StatThreshold, stat = "ColdResistTotal", thresholdStat = "FireResistTotal" })]), r#mod(n!("FirePenetration"), BASE, num, None, 0, 0, &[tag!({ type = StatThreshold, stat = "FireResistTotal", thresholdStat = "LightningResistTotal" }), tag!({ type = StatThreshold, stat = "FireResistTotal", thresholdStat = "ColdResistTotal" })])])
    }), lua_line: 5113 });
    v.push(PatternEntry {
        pat: "damage penetrates fire resistance equal to your overcapped fire resistance",
        val: SpecialVal::Mods(vec![flag(
            n!("FirePenIncreasedByUncappedFireRes"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5118,
    });
    v.push(PatternEntry { pat: "damage penetrates fire resistance equal to your overcapped fire resistance, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FirePenetration"), BASE, 1.0, None, 0, 0, &[tag!({ type = PerStat, stat = "FireResistOverCap", limit = num, limitTotal = true })])])
    }), lua_line: 5119 });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of life when you kill an enemy during f?l?a?s?k? ?effect",
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
                    tag!({ type = Condition, var = "UsingFlask" }),
                ],
            )])
        }),
        lua_line: 5120,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of mana when you kill an enemy during f?l?a?s?k? ?effect",
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
                    tag!({ type = Condition, var = "UsingFlask" }),
                ],
            )])
        }),
        lua_line: 5121,
    });
    v.push(PatternEntry {
        pat: "recover (%d+)%% of energy shield when you kill an enemy during f?l?a?s?k? ?effect",
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
                    tag!({ type = Condition, var = "UsingFlask" }),
                ],
            )])
        }),
        lua_line: 5122,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of maximum life taken as chaos damage per second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChaosDegen"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Life", percent = num })],
            )])
        }),
        lua_line: 5123,
    });
    v.push(PatternEntry {
        pat: "your critical strikes do not deal extra damage during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("NoCritMultiplier"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5124,
    });
    v.push(PatternEntry {
        pat: "grants perfect agony during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Perfect Agony",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5125,
    });
    v.push(PatternEntry {
        pat: "grants eldritch battery during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Eldritch Battery",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5126,
    });
    v.push(PatternEntry {
        pat: "eldritch battery during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Eldritch Battery",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5127,
    });
    v.push(PatternEntry {
        pat: "chaos damage t?a?k?e?n? ?does not bypass energy shield during effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ChaosNotBypassEnergyShield"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5128,
    });
    v.push(PatternEntry { pat: "when hit during effect, (%d+)%% of life loss from damage taken occurs over 4 seconds instead", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LifeLossPrevented"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "UsingFlask" })])])
    }), lua_line: 5129 });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?skills [ch][oa][sv][te] no mana c?o?s?t? ?during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ManaCost"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5130,
    });
    v.push(PatternEntry {
        pat: "life recovery from flasks also applies to energy shield during f?l?a?s?k? ?effect",
        val: SpecialVal::Mods(vec![flag(
            n!("LifeFlaskAppliesToEnergyShield"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 5131,
    });
    v.push(PatternEntry {
        pat: "profane ground you create inflicts malediction on enemies",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("HasMalediction"),
                        None,
                        0,
                        0,
                        &[tag!({ type = Condition, var = "OnProfaneGround" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5132,
    });
    v.push(PatternEntry {
        pat: "profane ground you create also affects you and your allies, granting chaotic might",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("Condition:ChaoticMight"),
                        None,
                        0,
                        0,
                        &[tag!({ type = Condition, var = "OnProfaneGround" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5133,
    });
    v.push(PatternEntry {
        pat: "raised beast spectres have farrul's farric presence",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("Accuracy"), INC, 80.0, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("CritChance"), INC, 120.0, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(
                                n!("ReduceCritExtraDamage"),
                                BASE,
                                100.0,
                                None,
                                0,
                                0,
                                &[],
                            )),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
        ]),
        lua_line: 5134,
    });
    v.push(PatternEntry {
        pat: "raised beast spectres have farrul's fertile presence",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("Damage"), INC, 100.0, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("LifeRegenPercent"), BASE, 3.0, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(flag(n!("StunImmune"), None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
        ]),
        lua_line: 5139,
    });
    v.push(PatternEntry {
        pat: "raised beast spectres have farrul's wild presence",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("Speed"), INC, 20.0, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("MovementSpeed"), INC, 20.0, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
            r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("fromAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(
                                n!("MinimumActionSpeed"),
                                MAX,
                                100.0,
                                None,
                                0,
                                0,
                                &[],
                            )),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveBeastSpectre" })],
            ),
        ]),
        lua_line: 5144,
    });
    v.push(PatternEntry {
        pat: "gain alchemist's genius when you use a flask",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveAlchemistGenius"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5149,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain alchemist's genius when you use a flask",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveAlchemistGenius"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5152,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% less flask charges gained from kills",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskChargesGained"),
                MORE,
                (-nn(num, "perform arithmetic on local 'num'")),
                Some("from Kills"),
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5155,
    });
    v.push(PatternEntry {
        pat: "flasks gain (%d+) charges? every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("FlaskChargesGenerated"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'")
                    / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5158,
    });
}

/// @pob Modules/ModParser.lua:5161-5243 h=a53cb77a
fn specialModList_5161(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "flasks gain a charge every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let div = ns(caps.cap[0], "index local 'div'");
            Some(vec![r#mod(
                n!("FlaskChargesGenerated"),
                BASE,
                (1.0 / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5161,
    });
    v.push(PatternEntry {
        pat: "while a unique enemy is in your presence, flasks gain a charge every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let div = ns(caps.cap[0], "index local 'div'");
            Some(vec![r#mod(
                n!("FlaskChargesGenerated"),
                BASE,
                (1.0 / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })],
            )])
        }),
        lua_line: 5164,
    });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, flasks gain a charge every (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let div = ns(caps.cap[0], "index local 'div'");
        Some(vec![r#mod(n!("FlaskChargesGenerated"), BASE, (1.0 / snum(div, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })])])
    }), lua_line: 5167 });
    v.push(PatternEntry {
        pat: "utility flasks gain (%d+) charges? every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("UtilityFlaskChargesGenerated"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'")
                    / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5170,
    });
    v.push(PatternEntry {
        pat: "iron flasks gain (%d+) charges? when your ward breaks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("IronFlaskChargesGeneratedOnWardBreak"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5173,
    });
    v.push(PatternEntry {
        pat: "life flasks gain (%d+) charges? every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("LifeFlaskChargesGenerated"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'")
                    / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5176,
    });
    v.push(PatternEntry {
        pat: "while on low life, life flasks gain (%d+) charges? every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("LifeFlaskChargesGenerated"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'")
                    / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "LowLife" })],
            )])
        }),
        lua_line: 5179,
    });
    v.push(PatternEntry {
        pat: "mana flasks gain (%d+) charges? every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("ManaFlaskChargesGenerated"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'")
                    / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5182,
    });
    v.push(PatternEntry {
        pat: "flasks gain (%d+) charges? per empty flask slot every (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("FlaskChargesGeneratedPerEmptyFlask"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'")
                    / snum(div, "perform arithmetic on a string")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5185,
    });
    v.push(PatternEntry {
        pat: "flasks gain (%d+) charges? per second if you've hit a unique enemy recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskChargesGenerated"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "HitRecently" }),
                    tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" }),
                ],
            )])
        }),
        lua_line: 5188,
    });
    v.push(PatternEntry {
        pat: "effect is not removed when unreserved mana is filled",
        val: SpecialVal::Mods(vec![flag(n!("ManaFlaskEffectNotRemoved"), None, 0, 0, &[])]),
        lua_line: 5191,
    });
    v.push(PatternEntry {
        pat: "life flask effects are not removed when unreserved life is filled",
        val: SpecialVal::Mods(vec![flag(n!("LifeFlaskEffectNotRemoved"), None, 0, 0, &[])]),
        lua_line: 5194,
    });
    v.push(PatternEntry {
        pat: "mana flask effects are not removed when unreserved mana is filled",
        val: SpecialVal::Mods(vec![flag(n!("ManaFlaskEffectNotRemoved"), None, 0, 0, &[])]),
        lua_line: 5197,
    });
    // Jewels
    v.push(PatternEntry { pat: "passives? ?s?k?i?l?l?s? in radius of ([%a%s']+) can be allocated without being connected to your tree", val: SpecialVal::Fn(|caps: &Caps| {
        let name = ns(caps.cap[0], "index local 'name'");
        Some(vec![r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("impossibleEscapeKeystone")), (n!("value"), Value::from(&*name))]), None, 0, 0, &[]), r#mod(n!("ImpossibleEscapeKeystones"), LIST, vtab(vec![], vec![(n!("key"), Value::from(&*name)), (n!("value"), Value::from(true))]), None, 0, 0, &[])])
    }), lua_line: 5201 });
    v.push(PatternEntry { pat: "passives? ?s?k?i?l?l?s? in radius can be allocated without being connected to your tree", val: SpecialVal::Mods(vec![r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("intuitiveLeapLike")), (n!("value"), Value::from(true))]), None, 0, 0, &[])]), lua_line: 5205 });
    v.push(PatternEntry { pat: "keystone passive skills in radius can be allocated without being connected to your tree", val: SpecialVal::Mods(vec![r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("intuitiveLeapLike")), (n!("value"), Value::from(true))]), None, 0, 0, &[]), r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("intuitiveLeapKeystoneOnly")), (n!("value"), Value::from(true))]), None, 0, 0, &[])]), lua_line: 5206 });
    v.push(PatternEntry {
        pat: "affects passives in small ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(6.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5210,
    });
    v.push(PatternEntry {
        pat: "affects passives in medium ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(7.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5211,
    });
    v.push(PatternEntry {
        pat: "affects passives in large ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(8.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5212,
    });
    v.push(PatternEntry {
        pat: "affects passives in very large ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(9.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5213,
    });
    v.push(PatternEntry {
        pat: "affects passives in massive ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(10.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5214,
    });
    v.push(PatternEntry {
        pat: "only affects passives in small ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(6.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5215,
    });
    v.push(PatternEntry {
        pat: "only affects passives in medium ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(7.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5216,
    });
    v.push(PatternEntry {
        pat: "only affects passives in large ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(8.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5217,
    });
    v.push(PatternEntry {
        pat: "only affects passives in very large ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(9.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5218,
    });
    v.push(PatternEntry {
        pat: "only affects passives in massive ring",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("radiusIndex")),
                    (n!("value"), Value::from(10.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5219,
    });
    v.push(PatternEntry {
        pat: "primordial",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Multiplier:PrimordialItem"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5220,
    });
    v.push(PatternEntry {
        pat: "spectres have a base duration of (%d+) seconds",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SkillData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("duration")),
                    (n!("value"), Value::from(6.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true })],
        )]),
        lua_line: 5221,
    });
    v.push(PatternEntry {
        pat: "flasks applied to you have (%d+)%% increased effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5222,
    });
    v.push(PatternEntry {
        pat: "flasks applied to you have (%d+)%% increased effect per level",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = ActorCondition, actor = "player" }),
                    tag!({ type = Multiplier, var = "Level" }),
                ],
            )])
        }),
        lua_line: 5223,
    });
    v.push(PatternEntry { pat: "equipped magic flasks have (%d+)%% increased effect on you if no flasks are adjacent to them", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MagicFlaskNoAdjacentEffect"), INC, num, None, 0, 0, &[])])
    }), lua_line: 5224 });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, flasks applied to you have (%d+)%% increased effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FlaskEffect"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" }), tag!({ type = ActorCondition, actor = "player" })])])
    }), lua_line: 5225 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, flasks applied to you have (%d+)%% increased effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FlaskEffect"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" }), tag!({ type = ActorCondition, actor = "player" })])])
    }), lua_line: 5226 });
    v.push(PatternEntry {
        pat: "magic utility flasks applied to you have (%d+)%% increased effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MagicUtilityFlaskEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5227,
    });
    v.push(PatternEntry {
        pat: "flasks applied to you have (%d+)%% reduced effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FlaskEffect"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5228,
    });
    v.push(PatternEntry {
        pat: "tinctures applied to you have (%d+)%% increased effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TinctureEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5229,
    });
    v.push(PatternEntry { pat: "tinctures applied to you have (%d+)%% increased effect if you've used a life flask recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("TinctureEffect"), INC, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "player" }), tag!({ type = Condition, var = "UsingLifeFlask" })])])
    }), lua_line: 5230 });
    v.push(PatternEntry {
        pat: "tinctures applied to you have (%d+)%% increased effect while affected by no flasks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TinctureEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = ActorCondition, actor = "player" }),
                    tag!({ type = Condition, var = "UsingFlask", neg = true }),
                ],
            )])
        }),
        lua_line: 5231,
    });
    v.push(PatternEntry { pat: "tinctures have (%d+)%% increased effect while at or above (%d+) stacks of mana burn", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let threshold = ns(caps.cap[1], "index local 'threshold'");
        Some(vec![r#mod(n!("TinctureEffect"), INC, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, varList = &[n!("ManaBurnStacks"), n!("WeepingWoundsStacks")][..], threshold = tonumber(threshold) })])])
    }), lua_line: 5232 });
    v.push(PatternEntry {
        pat: "tinctures applied to you have (%d+)%% reduced mana burn rate",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TinctureManaBurnRate"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5233,
    });
    v.push(PatternEntry {
        pat: "tinctures applied to you have (%d+)%% less mana burn rate",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TinctureManaBurnRate"),
                MORE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5234,
    });
    v.push(PatternEntry {
        pat: "the first (%d+) mana burn applied to you have no effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EffectiveManaBurnStacks"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "player" })],
            )])
        }),
        lua_line: 5235,
    });
    v.push(PatternEntry {
        pat: "tinctures deactivate when you have (%d+) or more mana burn",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MaxManaBurnStacks"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5236,
    });
    v.push(PatternEntry {
        pat: "tinctures inflict weeping wounds instead of mana burn",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:WeepingWoundsInsteadOfManaBurn"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5237,
    });
    v.push(PatternEntry {
        pat: "tincture effects also apply to ranged weapons",
        val: SpecialVal::Mods(vec![flag(n!("TinctureRangedWeapons"), None, 0, 0, &[])]),
        lua_line: 5238,
    });
    v.push(PatternEntry {
        pat: "you can have an additional tincture active",
        val: SpecialVal::Mods(vec![r#mod(n!("TinctureLimit"), BASE, 1.0, None, 0, 0, &[])]),
        lua_line: 5239,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased tincture cooldown recovery rate",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TinctureCooldownRecovery"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5240,
    });
    v.push(PatternEntry {
        pat: "adds (%d+) passive skills",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("JewelData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("clusterJewelNodeCount")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5241,
    });
    v.push(PatternEntry {
        pat: "1 added passive skill is a jewel socket",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("clusterJewelSocketCount")),
                    (n!("value"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5242,
    });
    v.push(PatternEntry {
        pat: "(%d+) added passive skills are jewel sockets",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("JewelData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("clusterJewelSocketCount")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5243,
    });
}

/// @pob Modules/ModParser.lua:5244-5300 h=5e9887bc
fn specialModList_5244(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "adds (%d+) jewel socket passive skills",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("JewelData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("clusterJewelSocketCountOverride")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5244,
    });
    v.push(PatternEntry {
        pat: "adds (%d+) small passive skills? which grants? nothing",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("JewelData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("clusterJewelNothingnessCount")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5245,
    });
    v.push(PatternEntry {
        pat: "added small passive skills grant nothing",
        val: SpecialVal::Mods(vec![r#mod(
            n!("JewelData"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("key"), Value::from("clusterJewelSmallsAreNothingness")),
                    (n!("value"), Value::from(true)),
                ],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5246,
    });
    v.push(PatternEntry {
        pat: "added small passive skills have (%d+)%% increased effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("JewelData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("clusterJewelIncEffect")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5247,
    });
    v.push(PatternEntry { pat: "this jewel's socket has (%d+)%% increased effect per allocated passive skill between it and your class' starting location", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("jewelIncEffectFromClassStart")), (n!("value"), Value::from(num))]), None, 0, 0, &[])])
    }), lua_line: 5248 });
    v.push(PatternEntry { pat: "(%d+)%% increased effect of jewel socket passive skills containing corrupted (m?r?ag?r?i?e?c?) jewels, if not from cluster jewels", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let rarity = ns(caps.cap[1], "index local 'rarity'");
        Some(vec![r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from(cat!("corrupted", firstToUpper(rarity), "JewelIncEffect"))), (n!("value"), Value::from(num))]), None, 0, 0, &[])])
    }), lua_line: 5249 });
    v.push(PatternEntry { pat: "(%d+)%% increased effect of jewel socket passive skills containing corrupted (m?r?ag?r?i?e?c?) jewels", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let rarity = ns(caps.cap[1], "index local 'rarity'");
        Some(vec![r#mod(n!("JewelData"), LIST, vtab(vec![], vec![(n!("key"), Value::from(cat!("corrupted", firstToUpper(rarity), "JewelIncEffect"))), (n!("value"), Value::from(num))]), None, 0, 0, &[])])
    }), lua_line: 5250 });
    // Misc
    v.push(PatternEntry {
        pat: "can't use chest armour",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CanNotUseItem"),
            Flag,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = DisablesItem, slotName = "Body Armour" })],
        )]),
        lua_line: 5252,
    });
    v.push(PatternEntry {
        pat: "can't use helmets?",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CanNotUseItem"),
            Flag,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = DisablesItem, slotName = "Helmet" })],
        )]),
        lua_line: 5253,
    });
    v.push(PatternEntry {
        pat: "can't use other rings",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("CanNotUseItem"),
                Flag,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = DisablesItem, slotName = "Ring 2" }),
                    tag!({ type = SlotNumber, num = 1.0 }),
                ],
            ),
            r#mod(
                n!("CanNotUseItem"),
                Flag,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = DisablesItem, slotName = "Ring 1" }),
                    tag!({ type = SlotNumber, num = 2.0 }),
                ],
            ),
        ]),
        lua_line: 5254,
    });
    v.push(PatternEntry {
        pat: "uses both hand slots",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("CanNotUseItem"),
                Flag,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = DisablesItem, slotName = "Weapon 2" }),
                    tag!({ type = SlotNumber, num = 1.0 }),
                ],
            ),
            r#mod(
                n!("CanNotUseItem"),
                Flag,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = DisablesItem, slotName = "Weapon 1" }),
                    tag!({ type = SlotNumber, num = 2.0 }),
                ],
            ),
        ]),
        lua_line: 5255,
    });
    v.push(PatternEntry {
        pat: "can't use flask in fifth slot",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CanNotUseItem"),
            Flag,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = DisablesItem, slotName = "Flask 5", excludeItemType = "Tincture" })],
        )]),
        lua_line: 5256,
    });
    v.push(PatternEntry { pat: "boneshatter has (%d+)%% chance to grant %+1 trauma", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraTrauma"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Boneshatter", includeTransfigured = true })])])
    }), lua_line: 5257 });
    v.push(PatternEntry { pat: "your minimum frenzy, endurance and power charges are equal to your maximum while you are stationary", val: SpecialVal::Mods(vec![flag(n!("MinimumFrenzyChargesIsMaximumFrenzyCharges"), None, 0, 0, &[tag!({ type = Condition, var = "Stationary" })]), flag(n!("MinimumEnduranceChargesIsMaximumEnduranceCharges"), None, 0, 0, &[tag!({ type = Condition, var = "Stationary" })]), flag(n!("MinimumPowerChargesIsMaximumPowerCharges"), None, 0, 0, &[tag!({ type = Condition, var = "Stationary" })])]), lua_line: 5258 });
    v.push(PatternEntry {
        pat: "minimum power charges equal to maximum while stationary",
        val: SpecialVal::Mods(vec![flag(
            n!("MinimumPowerChargesIsMaximumPowerCharges"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Stationary" })],
        )]),
        lua_line: 5263,
    });
    v.push(PatternEntry {
        pat: "minimum frenzy charges equal to maximum while stationary",
        val: SpecialVal::Mods(vec![flag(
            n!("MinimumFrenzyChargesIsMaximumFrenzyCharges"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Stationary" })],
        )]),
        lua_line: 5264,
    });
    v.push(PatternEntry {
        pat: "minimum endurance charges equal to maximum while stationary",
        val: SpecialVal::Mods(vec![flag(
            n!("MinimumEnduranceChargesIsMaximumEnduranceCharges"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Stationary" })],
        )]),
        lua_line: 5265,
    });
    v.push(PatternEntry {
        pat: "count as having maximum number of power charges",
        val: SpecialVal::Mods(vec![flag(n!("HaveMaximumPowerCharges"), None, 0, 0, &[])]),
        lua_line: 5266,
    });
    v.push(PatternEntry {
        pat: "count as having maximum number of frenzy charges",
        val: SpecialVal::Mods(vec![flag(n!("HaveMaximumFrenzyCharges"), None, 0, 0, &[])]),
        lua_line: 5267,
    });
    v.push(PatternEntry {
        pat: "count as having maximum number of endurance charges",
        val: SpecialVal::Mods(vec![flag(
            n!("HaveMaximumEnduranceCharges"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5268,
    });
    v.push(PatternEntry {
        pat: "leftmost (%d+) magic utility flasks constantly apply their flask effects to you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LeftActiveMagicUtilityFlasks"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5269,
    });
    v.push(PatternEntry {
        pat: "rightmost (%d+) magic utility flasks constantly apply their flask effects to you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("RightActiveMagicUtilityFlasks"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5270,
    });
    v.push(PatternEntry {
        pat: "marauder: melee skills have (%d+)%% increased area of effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "ConnectedToMarauderStart" }),
                    tag!({ type = SkillType, skillType = SkillType::Melee }),
                ],
            )])
        }),
        lua_line: 5271,
    });
    v.push(PatternEntry {
        pat: "intelligence provides no bonus to energy shield",
        val: SpecialVal::Mods(vec![flag(n!("NoIntBonusToES"), None, 0, 0, &[])]),
        lua_line: 5272,
    });
    v.push(PatternEntry {
        pat: "intelligence provides no inherent bonus to energy shield",
        val: SpecialVal::Mods(vec![flag(n!("NoIntBonusToES"), None, 0, 0, &[])]),
        lua_line: 5273,
    });
    v.push(PatternEntry {
        pat: "gain accuracy rating equal to your intelligence",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Accuracy"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = PerStat, stat = "Int" })],
        )]),
        lua_line: 5274,
    });
    v.push(PatternEntry {
        pat: "intelligence is added to accuracy rating with wands",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Accuracy"),
            BASE,
            1.0,
            None,
            ModFlag::Wand,
            0,
            &[tag!({ type = PerStat, stat = "Int" })],
        )]),
        lua_line: 5275,
    });
    v.push(PatternEntry {
        pat: "dexterity's accuracy bonus instead grants %+(%d+) to accuracy rating per dexterity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DexAccBonusOverride"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5276,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased accuracy rating against marked enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })],
            )])
        }),
        lua_line: 5277,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% more accuracy rating against marked enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })],
            )])
        }),
        lua_line: 5278,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) to accuracy against bleeding enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })],
            )])
        }),
        lua_line: 5279,
    });
    v.push(PatternEntry {
        pat: "cannot recover energy shield to above armour",
        val: SpecialVal::Mods(vec![flag(n!("ArmourESRecoveryCap"), None, 0, 0, &[])]),
        lua_line: 5280,
    });
    v.push(PatternEntry {
        pat: "cannot recover energy shield to above evasion rating",
        val: SpecialVal::Mods(vec![flag(n!("EvasionESRecoveryCap"), None, 0, 0, &[])]),
        lua_line: 5281,
    });
    v.push(PatternEntry {
        pat: "warcries exert (%d+) additional attacks?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraExertedAttacks"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5282,
    });
    v.push(PatternEntry {
        pat: "battlemage's cry exerts (%d+) additional attack",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BattlemageExertedAttacks"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5283,
    });
    v.push(PatternEntry {
        pat: "rallying cry exerts (%d+) additional attack",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("RallyingExertedAttacks"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5284,
    });
    v.push(PatternEntry {
        pat: "warcries have (%d+)%% chance to exert (%d+) additional attacks?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let var = ns(caps.cap[1], "index local 'var'");
            Some(vec![r#mod(
                n!("ExtraExertedAttacks"),
                BASE,
                ((nn(num, "perform arithmetic on local 'num'")
                    * snum(var, "perform arithmetic on a string"))
                    / 100.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5285,
    });
    v.push(PatternEntry {
        pat: "skills deal (%d+)%% more damage for each warcry exerting them",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EchoesExertAverageIncrease"),
                MORE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5286,
    });
    v.push(PatternEntry {
        pat: "iron will",
        val: SpecialVal::Mods(vec![flag(n!("IronWill"), None, 0, 0, &[])]),
        lua_line: 5287,
    });
    v.push(PatternEntry {
        pat: "iron reflexes while stationary",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Iron Reflexes",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Stationary" })],
        )]),
        lua_line: 5288,
    });
    v.push(PatternEntry { pat: "you have iron reflexes while at maximum frenzy charges", val: SpecialVal::Mods(vec![r#mod(n!("Keystone"), LIST, "Iron Reflexes", None, 0, 0, &[tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })])]), lua_line: 5289 });
    v.push(PatternEntry {
        pat: "you have zealot's oath if you haven't been hit recently",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Zealot's Oath",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "BeenHitRecently", neg = true })],
        )]),
        lua_line: 5290,
    });
    v.push(PatternEntry {
        pat: "deal no physical damage",
        val: SpecialVal::Mods(vec![flag(n!("DealNoPhysical"), None, 0, 0, &[])]),
        lua_line: 5291,
    });
    v.push(PatternEntry {
        pat: "deal no cold damage",
        val: SpecialVal::Mods(vec![flag(n!("DealNoCold"), None, 0, 0, &[])]),
        lua_line: 5292,
    });
    v.push(PatternEntry {
        pat: "deal no fire damage",
        val: SpecialVal::Mods(vec![flag(n!("DealNoFire"), None, 0, 0, &[])]),
        lua_line: 5293,
    });
    v.push(PatternEntry {
        pat: "deal no lightning damage",
        val: SpecialVal::Mods(vec![flag(n!("DealNoLightning"), None, 0, 0, &[])]),
        lua_line: 5294,
    });
    v.push(PatternEntry {
        pat: "deal no elemental damage",
        val: SpecialVal::Mods(vec![
            flag(n!("DealNoLightning"), None, 0, 0, &[]),
            flag(n!("DealNoCold"), None, 0, 0, &[]),
            flag(n!("DealNoFire"), None, 0, 0, &[]),
        ]),
        lua_line: 5295,
    });
    v.push(PatternEntry {
        pat: "deal no chaos damage",
        val: SpecialVal::Mods(vec![flag(n!("DealNoChaos"), None, 0, 0, &[])]),
        lua_line: 5296,
    });
    v.push(PatternEntry {
        pat: "deal no damage",
        val: SpecialVal::Mods(vec![flag(n!("DealNoDamage"), None, 0, 0, &[])]),
        lua_line: 5297,
    });
    v.push(PatternEntry { pat: "you can't deal damage with skills yourself", val: SpecialVal::Mods(vec![flag(n!("DealNoDamage"), None, 0, 0, &[tag!({ type = SkillType, skillTypeList = &[SkillType::SummonsTotem, SkillType::RemoteMined, SkillType::Trapped][..], neg = true }), tag!({ type = Condition, var = "usedByMirage", neg = true })])]), lua_line: 5298 });
}

/// @pob Modules/ModParser.lua:5301-5403 h=7dc8a909
fn specialModList_5301(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "deal no non%-elemental damage",
        val: SpecialVal::Mods(vec![
            flag(n!("DealNoPhysical"), None, 0, 0, &[]),
            flag(n!("DealNoChaos"), None, 0, 0, &[]),
        ]),
        lua_line: 5301,
    });
    v.push(PatternEntry {
        pat: "deal no non%-(%a+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmgType = ns(caps.cap[0], "index local 'dmgType'");
            dealNoNonDamageType(dmgType, false)
        }),
        lua_line: 5302,
    });
    v.push(PatternEntry {
        pat: "cannot deal non%-(%a+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let dmgType = ns(caps.cap[0], "index local 'dmgType'");
            dealNoNonDamageType(dmgType, false)
        }),
        lua_line: 5303,
    });
    v.push(PatternEntry {
        pat: "deal no physical or elemental damage",
        val: SpecialVal::Mods(vec![
            flag(n!("DealNoPhysical"), None, 0, 0, &[]),
            flag(n!("DealNoCold"), None, 0, 0, &[]),
            flag(n!("DealNoFire"), None, 0, 0, &[]),
            flag(n!("DealNoLightning"), None, 0, 0, &[]),
        ]),
        lua_line: 5304,
    });
    v.push(PatternEntry {
        pat: "deal no damage when not on low life",
        val: SpecialVal::Mods(vec![flag(
            n!("DealNoDamage"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "LowLife", neg = true })],
        )]),
        lua_line: 5305,
    });
    v.push(PatternEntry {
        pat: "spell skills deal no damage",
        val: SpecialVal::Mods(vec![flag(
            n!("DealNoDamage"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Spell })],
        )]),
        lua_line: 5306,
    });
    v.push(PatternEntry {
        pat: "attacks have blood magic",
        val: SpecialVal::Mods(vec![flag(
            n!("CostLifeInsteadOfMana"),
            None,
            ModFlag::Attack,
            0,
            &[],
        )]),
        lua_line: 5307,
    });
    v.push(PatternEntry {
        pat: "attacks cost life instead of mana",
        val: SpecialVal::Mods(vec![flag(
            n!("CostLifeInsteadOfMana"),
            None,
            ModFlag::Attack,
            0,
            &[],
        )]),
        lua_line: 5308,
    });
    v.push(PatternEntry {
        pat: "attack skills cost life instead of (%d+)%% of mana cost",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("HybridManaAndLifeCost_Life"),
                BASE,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 5309,
    });
    v.push(PatternEntry {
        pat: "skills cost energy shield instead of mana or life",
        val: SpecialVal::Mods(vec![flag(n!("CostESInsteadOfManaOrLife"), None, 0, 0, &[])]),
        lua_line: 5312,
    });
    v.push(PatternEntry {
        pat: "spells have an additional life cost equal to (%d+)%% of your maximum life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeCostBase"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num, floor = true }),
                    tag!({ type = SkillType, skillType = SkillType::Spell }),
                ],
            )])
        }),
        lua_line: 5313,
    });
    v.push(PatternEntry { pat: "spells have added spell damage equal to (%d+)%% of physical damage of your equipped two handed weapon", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("WeaponPhysAppliesToSpells"), None, 0, 0, &[]), r#mod(n!("WeaponPhysAppliesToSpellsPercent"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "UsingTwoHandedWeapon" })])])
    }), lua_line: 5316 });
    v.push(PatternEntry {
        pat: "spells cost %+(%d+)%% of life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeCostBase"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num, floor = true }),
                    tag!({ type = SkillType, skillType = SkillType::Spell }),
                ],
            )])
        }),
        lua_line: 5320,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed elemental spell on block, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportTriggerElementalSpellOnBlock"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5323,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed spell when you block, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportUniqueVirulenceSpellsCastOnBlock"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5324,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to cast a? ?socketed lightning spells? on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportUniqueMjolnerLightningSpellsCastOnHit"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5325,
    });
    v.push(PatternEntry {
        pat: "cast a socketed lightning spell on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportUniqueMjolnerLightningSpellsCastOnHit"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5326,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed lightning spell on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportUniqueMjolnerLightningSpellsCastOnHit"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5327,
    });
    v.push(PatternEntry {
        pat: "trigger a socketed lightning spell on hit, with a ([%d%.]+) second cooldown",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from("SupportUniqueMjolnerLightningSpellsCastOnHit"),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5328,
    });
    v.push(PatternEntry { pat: "trigger a socketed spell when a hit from this weapon freezes a target, with a ([%d%.]+) second cooldown", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportTriggerSpellOnBowAttackFreezeHit")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 5329 });
    v.push(PatternEntry { pat: "trigger a socketed spell on unarmed melee critical strike, with a ([%d%.]+) second cooldown", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportTriggerSpellOnUnarmedMeleeCriticalHit")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 5330 });
    v.push(PatternEntry {
        pat: "[ct][ar][si][tg]g?e?r? a socketed cold s[pk][ei]ll on melee critical strike",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSupport"),
            LIST,
            vtab(
                vec![],
                vec![
                    (
                        n!("skillId"),
                        Value::from(
                            "SupportUniqueCosprisMaliceColdSpellsCastOnMeleeCriticalStrike",
                        ),
                    ),
                    (n!("level"), Value::from(1.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 5331,
    });
    v.push(PatternEntry { pat: "[ct][ar][si][tg]g?e?r? a socketed cold s[pk][ei]ll on melee critical strike, with a ([%d%.]+) second cooldown", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportUniqueCosprisMaliceColdSpellsCastOnMeleeCriticalStrike")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 5334 });
    v.push(PatternEntry { pat: "you cannot cast socketed hex curse skills inflict socketed hexes on enemies that trigger your traps", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportCurseOnTrapTriggered")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 5337 });
    v.push(PatternEntry {
        pat: "your curses can apply to hexproof enemies",
        val: SpecialVal::Mods(vec![flag(n!("CursesIgnoreHexproof"), None, 0, 0, &[])]),
        lua_line: 5338,
    });
    v.push(PatternEntry {
        pat: "your hexes can affect hexproof enemies",
        val: SpecialVal::Mods(vec![flag(n!("CursesIgnoreHexproof"), None, 0, 0, &[])]),
        lua_line: 5339,
    });
    v.push(PatternEntry {
        pat: "([%a%s]+) can affect hexproof enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let name = ns(caps.cap[0], "index local 'name'");
            Some(vec![r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("ignoreHexproof")),
                        (n!("value"), Value::from(true)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillId, skillId = gemIdLookup(name) })],
            )])
        }),
        lua_line: 5340,
    });
    v.push(PatternEntry {
        pat: "hexes from socketed skills can apply (%d) additional curses",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("SocketedCursesHexLimitValue"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
                flag(
                    n!("SocketedCursesAdditionalLimit"),
                    None,
                    0,
                    0,
                    &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
                ),
            ])
        }),
        lua_line: 5343,
    });
    // This is being changed from ignoreHexLimit to SocketedCursesAdditionalLimit due to patch 3.16.0, which states that legacy versions "will be affected by this Curse Limit change,
    // though they will only have 20% less Curse Effect of Curses triggered with Summon Doedreâs Effigy."
    // Legacy versions will still show that "Hexes from Socketed Skills ignore Curse limit", but will instead have an internal limit of 5 to match the current functionality.
    v.push(PatternEntry {
        pat: "hexes from socketed skills ignore curse limit",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![
                r#mod(
                    n!("SocketedCursesHexLimitValue"),
                    BASE,
                    5.0,
                    None,
                    0,
                    0,
                    &[],
                ),
                flag(
                    n!("SocketedCursesAdditionalLimit"),
                    None,
                    0,
                    0,
                    &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
                ),
            ])
        }),
        lua_line: 5347,
    });
    v.push(PatternEntry {
        pat: "reserves (%d+)%% of life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraLifeReserved"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5348,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of cold damage taken as lightning",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ColdDamageTakenAsLightning"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5349,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of fire damage taken as lightning",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FireDamageTakenAsLightning"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5350,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of fire and lightning damage from hits taken as cold damage during effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("FireDamageFromHitsTakenAsCold"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "UsingFlask" })],
                ),
                r#mod(
                    n!("LightningDamageFromHitsTakenAsCold"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "UsingFlask" })],
                ),
            ])
        }),
        lua_line: 5351,
    });
    // hand-ported (lua2rs: and-or: optname and optname); the Lua:
    // |     ["(%d+)%% of (%a+) and (%a+) damage from hits taken as (%a+) damage"] = function(num, _, type1, type2, type3)
    // |         local fromType1 = dmgTypes[type1]
    // |         local fromType2 = dmgTypes[type2]
    // |         local toType = dmgTypes[type3]
    // |         return fromType1 and fromType2 and toType and {
    // |             mod(fromType1 .. "DamageFromHitsTakenAs" .. toType, "BASE", num),
    // |             mod(fromType2 .. "DamageFromHitsTakenAs" .. toType, "BASE", num),
    // |         }
    // |     end,
    v.push(PatternEntry {
        pat: "(%d+)%% of (%a+) and (%a+) damage from hits taken as (%a+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            // hand-ported
            let num = caps.num;
            let type1 = caps.cap[1];
            let type2 = caps.cap[2];
            let type3 = caps.cap[3];
            // L5356: dmgTypes[nil] is nil (indexing a table with nil reads nil)
            let fromType1 = type1.and_then(dmgTypes);
            // L5357
            let fromType2 = type2.and_then(dmgTypes);
            // L5358
            let toType = type3.and_then(dmgTypes);
            // L5359: fromType1 and fromType2 and toType and { … }
            let (Some(fromType1), Some(fromType2), Some(toType)) = (fromType1, fromType2, toType)
            else {
                return None;
            };
            Some(vec![
                r#mod(
                    cat!(fromType1, "DamageFromHitsTakenAs", toType),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    cat!(fromType2, "DamageFromHitsTakenAs", toType),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5355,
    });
    v.push(PatternEntry {
        pat: "items and gems have (%d+)%% reduced attribute requirements",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GlobalAttributeRequirements"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5364,
    });
    v.push(PatternEntry {
        pat: "items and gems have (%d+)%% increased attribute requirements",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GlobalAttributeRequirements"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5365,
    });
    v.push(PatternEntry {
        pat: "mana reservation of herald skills is always (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("ManaReservationPercentForced")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Herald })],
            )])
        }),
        lua_line: 5366,
    });
    v.push(PatternEntry {
        pat: "([%a%s]+) reserves no mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let name = ns(caps.cap[0], "index local 'name'");
            Some(vec![
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("manaReservationFlat")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("lifeReservationFlat")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("manaReservationPercent")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("lifeReservationPercent")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
            ])
        }),
        lua_line: 5367,
    });
    v.push(PatternEntry {
        pat: "([%a%s]+) has no reservation",
        val: SpecialVal::Fn(|caps: &Caps| {
            let name = ns(caps.cap[0], "index local 'name'");
            Some(vec![
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("manaReservationFlat")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("lifeReservationFlat")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("manaReservationPercent")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("lifeReservationPercent")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
            ])
        }),
        lua_line: 5373,
    });
    v.push(PatternEntry {
        pat: "([%a%s]+) has no reservation if cast as an aura",
        val: SpecialVal::Fn(|caps: &Caps| {
            let name = ns(caps.cap[0], "index local 'name'");
            Some(vec![
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("manaReservationFlat")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Aura }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("lifeReservationFlat")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Aura }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("manaReservationPercent")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Aura }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
                r#mod(
                    n!("SkillData"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (n!("key"), Value::from("lifeReservationPercent")),
                            (n!("value"), Value::from(0.0)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillId, skillId = gemIdLookup(name) }),
                        tag!({ type = SkillType, skillType = SkillType::Aura }),
                        tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                    ],
                ),
            ])
        }),
        lua_line: 5379,
    });
    v.push(PatternEntry {
        pat: "banner skills reserve no mana",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("manaReservationPercent")),
                        (n!("value"), Value::from(0.0)),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Banner }),
                    tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                ],
            ),
            r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("lifeReservationPercent")),
                        (n!("value"), Value::from(0.0)),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Banner }),
                    tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                ],
            ),
        ]),
        lua_line: 5385,
    });
    v.push(PatternEntry {
        pat: "banner skills have no reservation",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("manaReservationPercent")),
                        (n!("value"), Value::from(0.0)),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Banner }),
                    tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                ],
            ),
            r#mod(
                n!("SkillData"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("lifeReservationPercent")),
                        (n!("value"), Value::from(0.0)),
                    ],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Banner }),
                    tag!({ type = SkillType, skillType = SkillType::Blessing, neg = true }),
                ],
            ),
        ]),
        lua_line: 5389,
    });
    v.push(PatternEntry {
        pat: "placed banners also grant (%d+)%% increased attack damage to you and allies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraAuraEffect"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Damage"), INC, num, None, ModFlag::Attack, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "BannerPlanted" }),
                    tag!({ type = SkillType, skillType = SkillType::Banner }),
                ],
            )])
        }),
        lua_line: 5393,
    });
    v.push(PatternEntry { pat: "banners also cause enemies to take (%d+)%% increased damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAuraDebuffEffect"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTaken"), INC, num, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "AuraDebuff", unscalable = true })])))]), None, 0, 0, &[tag!({ type = Condition, var = "BannerPlanted" }), tag!({ type = SkillType, skillType = SkillType::Banner })])])
    }), lua_line: 5394 });
    v.push(PatternEntry { pat: "dread banner grants an additional %+(%d+) to maximum fortification when placing the banner", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("MaximumFortification"), BASE, num, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Buff" })])))]), None, 0, 0, &[tag!({ type = Condition, var = "BannerPlanted" }), tag!({ type = SkillName, skillName = "Dread Banner" })])])
    }), lua_line: 5395 });
    v.push(PatternEntry {
        pat: "your aura skills are disabled",
        val: SpecialVal::Mods(vec![flag(
            n!("DisableSkill"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Aura })],
        )]),
        lua_line: 5396,
    });
    v.push(PatternEntry {
        pat: "your blessing skills are disabled",
        val: SpecialVal::Mods(vec![flag(
            n!("DisableSkill"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Blessing })],
        )]),
        lua_line: 5397,
    });
    v.push(PatternEntry {
        pat: "your spells are disabled",
        val: SpecialVal::Mods(vec![
            flag(
                n!("DisableSkill"),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Spell })],
            ),
            flag(n!("ForceEnableCurseApplication"), None, 0, 0, &[]),
        ]),
        lua_line: 5398,
    });
    v.push(PatternEntry {
        pat: "your warcries are disabled",
        val: SpecialVal::Mods(vec![flag(
            n!("DisableSkill"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Warcry })],
        )]),
        lua_line: 5402,
    });
    v.push(PatternEntry {
        pat: "your travel skills are disabled",
        val: SpecialVal::Mods(vec![flag(
            n!("DisableSkill"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Travel })],
        )]),
        lua_line: 5403,
    });
}

/// @pob Modules/ModParser.lua:5404-5511 h=ada4972a
fn specialModList_5404(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "aura skills other than ([%a%s]+) are disabled",
        val: SpecialVal::Fn(|caps: &Caps| {
            let name = ns(caps.cap[0], "index local 'name'");
            Some(vec![
                flag(
                    n!("DisableSkill"),
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = SkillType, skillType = SkillType::Aura }),
                        tag!({ type = SkillType, skillType = SkillType::RemoteMined, neg = true }),
                    ],
                ),
                flag(
                    n!("EnableSkill"),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = Name::from(name) })],
                ),
            ])
        }),
        lua_line: 5404,
    });
    v.push(PatternEntry {
        pat: "travel skills other than ([%a%s]+) are disabled",
        val: SpecialVal::Fn(|caps: &Caps| {
            let name = ns(caps.cap[0], "index local 'name'");
            Some(vec![
                flag(
                    n!("DisableSkill"),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillType, skillType = SkillType::Travel })],
                ),
                flag(
                    n!("EnableSkill"),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillId, skillId = gemIdLookup(name) })],
                ),
            ])
        }),
        lua_line: 5408,
    });
    v.push(PatternEntry { pat: "strength's damage bonus instead grants (%d+)%% increased melee physical damage per (%d+) strength", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let perStr = ns(caps.cap[1], "index local 'perStr'");
        Some(vec![r#mod(n!("StrDmgBonusRatioOverride"), BASE, (nn(num, "perform arithmetic on local 'num'") / nn(tonumber(perStr), "perform arithmetic on a nil value")), None, 0, 0, &[])])
    }), lua_line: 5412 });
    v.push(PatternEntry { pat: "while in her embrace, take ([%d%.]+)%% of your total maximum life and energy shield as fire damage per second per level", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FireDegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num }), tag!({ type = Multiplier, var = "Level" }), tag!({ type = Condition, var = "HerEmbrace" })]), r#mod(n!("FireDegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num }), tag!({ type = Multiplier, var = "Level" }), tag!({ type = Condition, var = "HerEmbrace" })])])
    }), lua_line: 5413 });
    v.push(PatternEntry {
        pat: "gain her embrace for %d+ seconds when you ignite an enemy",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanGainHerEmbrace"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5417,
    });
    v.push(PatternEntry { pat: "when you cast a spell, sacrifice all mana to gain added maximum lightning damage equal to (%d+)%% of sacrificed mana for 4 seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("Condition:HaveManaStorm"), None, 0, 0, &[]), r#mod(n!("LightningMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaUnreserved", percent = num }), tag!({ type = Condition, var = "SacrificeManaForLightning" })])])
    }), lua_line: 5418 });
    // typo never existed except in some items generated by PoB
    v.push(PatternEntry { pat: "attacks with this weapon have added maximum lightning damage equal to (%d+)%% of your energy shield", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LightningMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num }), tag!({ type = Condition, var = "{Hand}Attack" }), tag!({ type = SkillType, skillType = SkillType::Attack })])])
    }), lua_line: 5422 });
    v.push(PatternEntry { pat: "attacks with this weapon have added maximum lightning damage equal to (%d+)%% of your maximum energy shield", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LightningMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num }), tag!({ type = Condition, var = "{Hand}Attack" }), tag!({ type = SkillType, skillType = SkillType::Attack })])])
    }), lua_line: 5425 });
    v.push(PatternEntry { pat: "attacks with this weapon have added fire damage equal to (%d+)%% of player's maximum life", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FireMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num, actor = "parent" }), tag!({ type = Condition, var = "{Hand}Attack" }), tag!({ type = SkillType, skillType = SkillType::Attack })]), r#mod(n!("FireMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Life", percent = num, actor = "parent" }), tag!({ type = Condition, var = "{Hand}Attack" }), tag!({ type = SkillType, skillType = SkillType::Attack })])])
    }), lua_line: 5428 });
    v.push(PatternEntry {
        pat:
            "adds (%d+)%% of your maximum energy shield as cold damage to attacks with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("ColdMin"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "EnergyShield", percent = num }),
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                    ],
                ),
                r#mod(
                    n!("ColdMax"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "EnergyShield", percent = num }),
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                    ],
                ),
            ])
        }),
        lua_line: 5432,
    });
    v.push(PatternEntry { pat: "attacks with this weapon have added maximum lightning damage equal to (%d+)%% of player'?s? maximum energy shield", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LightningMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShield", percent = num, actor = "parent" }), tag!({ type = Condition, var = "{Hand}Attack" }), tag!({ type = SkillType, skillType = SkillType::Attack })])])
    }), lua_line: 5436 });
    v.push(PatternEntry {
        pat: "adds (%d+)%% of your maximum mana as fire damage to attacks with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("FireMin"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "Mana", percent = num }),
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                    ],
                ),
                r#mod(
                    n!("FireMax"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "Mana", percent = num }),
                        tag!({ type = Condition, var = "{Hand}Attack" }),
                        tag!({ type = SkillType, skillType = SkillType::Attack }),
                    ],
                ),
            ])
        }),
        lua_line: 5439,
    });
    v.push(PatternEntry {
        pat: "gain added chaos damage equal to (%d+)%% of ward",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("ChaosMin"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[tag!({ type = PercentStat, stat = "Ward", percent = num })],
                ),
                r#mod(
                    n!("ChaosMax"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[tag!({ type = PercentStat, stat = "Ward", percent = num })],
                ),
            ])
        }),
        lua_line: 5443,
    });
    v.push(PatternEntry { pat: "while you have unbroken ward, your next non%-channelling attack you use yourself breaks your ward to gain added cold damage equal to (%d+)%% of ward", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ColdMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Ward", percent = num }), tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }), tag!({ type = SkillType, skillType = SkillType::Attack }), tag!({ type = Condition, var = "WardNotBreak", neg = true }), tag!({ type = Condition, var = "UnbrokenWard" })]), r#mod(n!("ColdMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Ward", percent = num }), tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }), tag!({ type = SkillType, skillType = SkillType::Attack }), tag!({ type = Condition, var = "WardNotBreak", neg = true }), tag!({ type = Condition, var = "UnbrokenWard" })])])
    }), lua_line: 5447 });
    v.push(PatternEntry {
        pat: "spells deal added chaos damage equal to (%d+)%% of your maximum life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("ChaosMin"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "Life", percent = num }),
                        tag!({ type = SkillType, skillType = SkillType::Spell }),
                    ],
                ),
                r#mod(
                    n!("ChaosMax"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[
                        tag!({ type = PercentStat, stat = "Life", percent = num }),
                        tag!({ type = SkillType, skillType = SkillType::Spell }),
                    ],
                ),
            ])
        }),
        lua_line: 5451,
    });
    v.push(PatternEntry {
        pat: "every 16 seconds you gain iron reflexes for 8 seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:HaveArborix"), None, 0, 0, &[])]),
        lua_line: 5455,
    });
    v.push(PatternEntry {
        pat: "every 16 seconds you gain elemental overload for 8 seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:HaveAugyre"), None, 0, 0, &[])]),
        lua_line: 5458,
    });
    v.push(PatternEntry {
        pat: "every 8 seconds, gain avatar of fire for 4 seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:HaveVulconus"), None, 0, 0, &[])]),
        lua_line: 5461,
    });
    v.push(PatternEntry { pat: "when hit, gain a random movement speed modifier from 40%% reduced to 100%% increased until hit again", val: SpecialVal::Mods(vec![flag(n!("Condition:HaveGamblesprint"), None, 0, 0, &[])]), lua_line: 5464 });
    v.push(PatternEntry { pat: "trigger socketed curse spell when you cast a curse spell, with a ([%d%.]+) second cooldown", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSupport"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SupportUniqueCastCurseOnCurse")), (n!("level"), Value::from(1.0))]), None, 0, 0, &[tag!({ type = SocketedIn, slotName = "{SlotName}" })])]), lua_line: 5467 });
    v.push(PatternEntry {
        pat: "modifiers to attributes instead apply to omniscience",
        val: SpecialVal::Mods(vec![flag(n!("Omniscience"), None, 0, 0, &[])]),
        lua_line: 5470,
    });
    v.push(PatternEntry {
        pat: "attribute requirements can be satisfied by (%d+)%% of omniscience",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("OmniAttributeRequirements"), INC, num, None, 0, 0, &[]),
                flag(n!("OmniscienceRequirements"), None, 0, 0, &[]),
            ])
        }),
        lua_line: 5471,
    });
    v.push(PatternEntry {
        pat: "you have far shot while you do not have iron reflexes",
        val: SpecialVal::Mods(vec![flag(
            n!("FarShot"),
            None,
            0,
            0,
            &[tag!({ type = Condition, neg = true, var = "HaveIronReflexes" })],
        )]),
        lua_line: 5475,
    });
    v.push(PatternEntry {
        pat: "you have resolute technique while you do not have elemental overload",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Resolute Technique",
            None,
            0,
            0,
            &[tag!({ type = Condition, neg = true, var = "HaveElementalOverload" })],
        )]),
        lua_line: 5476,
    });
    v.push(PatternEntry {
        pat: "hits ignore enemy monster fire resistance while you are ignited",
        val: SpecialVal::Mods(vec![flag(
            n!("IgnoreFireResistance"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Ignited" })],
        )]),
        lua_line: 5477,
    });
    v.push(PatternEntry {
        pat: "your hits can't be evaded by blinded enemies",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotBeEvaded"),
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "Blinded" })],
        )]),
        lua_line: 5478,
    });
    v.push(PatternEntry {
        pat: "blind does not affect your chance to hit",
        val: SpecialVal::Mods(vec![flag(n!("UnaffectedByBlind"), None, 0, 0, &[])]),
        lua_line: 5479,
    });
    v.push(PatternEntry {
        pat: "unaffected by blind",
        val: SpecialVal::Mods(vec![flag(n!("UnaffectedByBlind"), None, 0, 0, &[])]),
        lua_line: 5480,
    });
    v.push(PatternEntry {
        pat: "enemies blinded by you while you are blinded have malediction",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("HasMalediction"),
                        None,
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Blinded" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "Blinded" }),
                tag!({ type = Condition, var = "CannotBeBlinded", neg = true }),
            ],
        )]),
        lua_line: 5481,
    });
    v.push(PatternEntry {
        pat: "enemies blinded by you have malediction",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("HasMalediction"),
                        None,
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Blinded" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5482,
    });
    v.push(PatternEntry {
        pat: "enemies ignited by you during effect have malediction",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(
                        n!("HasMalediction"),
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
        )]),
        lua_line: 5483,
    });
    v.push(PatternEntry {
        pat: "skills which throw traps have blood magic",
        val: SpecialVal::Mods(vec![flag(
            n!("CostLifeInsteadOfMana"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Trapped })],
        )]),
        lua_line: 5484,
    });
    v.push(PatternEntry {
        pat: "skills which throw traps cost life instead of mana",
        val: SpecialVal::Mods(vec![flag(
            n!("CostLifeInsteadOfMana"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Trapped })],
        )]),
        lua_line: 5485,
    });
    v.push(PatternEntry {
        pat: "strength provides no bonus to maximum life",
        val: SpecialVal::Mods(vec![flag(n!("NoStrBonusToLife"), None, 0, 0, &[])]),
        lua_line: 5486,
    });
    v.push(PatternEntry {
        pat: "strength provides no inherent bonus to maximum life",
        val: SpecialVal::Mods(vec![flag(n!("NoStrBonusToLife"), None, 0, 0, &[])]),
        lua_line: 5487,
    });
    v.push(PatternEntry {
        pat: "intelligence provides no bonus to maximum mana",
        val: SpecialVal::Mods(vec![flag(n!("NoIntBonusToMana"), None, 0, 0, &[])]),
        lua_line: 5488,
    });
    v.push(PatternEntry {
        pat: "intelligence provides no inherent bonus to maximum mana",
        val: SpecialVal::Mods(vec![flag(n!("NoIntBonusToMana"), None, 0, 0, &[])]),
        lua_line: 5489,
    });
    v.push(PatternEntry {
        pat: "with a ghastly eye jewel socketed, minions have %+(%d+) to accuracy rating",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Accuracy"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "HaveGhastlyEyeJewelIn{SlotName}" })],
            )])
        }),
        lua_line: 5490,
    });
    v.push(PatternEntry { pat: "with a ghastly eye jewel socketed, minions have (%d+)%% chance to gain unholy might on hit with spells", val: SpecialVal::Fn(|caps: &Caps| Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:UnholyMight"), None, 0, 0, &[tag!({ type = Condition, var = "HitSpellRecently" })])))]), None, 0, 0, &[tag!({ type = Condition, var = "HaveGhastlyEyeJewelIn{SlotName}" })])])), lua_line: 5491 });
    v.push(PatternEntry {
        pat: "with a hypnotic eye jewel socketed, gain arcane surge on hit with spells",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![flag(
                n!("Condition:ArcaneSurge"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "HitSpellRecently" }),
                    tag!({ type = Condition, var = "HaveHypnoticEyeJewelIn{SlotName}" }),
                ],
            )])
        }),
        lua_line: 5494,
    });
    v.push(PatternEntry { pat: "hits ignore enemy monster chaos resistance if all equipped items are shaper items", val: SpecialVal::Mods(vec![flag(n!("IgnoreChaosResistance"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "NonShaperItem", upper = true, threshold = 0.0 })])]), lua_line: 5495 });
    v.push(PatternEntry { pat: "hits ignore enemy monster chaos resistance if all equipped items are elder items", val: SpecialVal::Mods(vec![flag(n!("IgnoreChaosResistance"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "NonElderItem", upper = true, threshold = 0.0 })])]), lua_line: 5496 });
    v.push(PatternEntry { pat: "your hits ignore enemy monster (%a+) resistances? if all equipped rings are (%a+) rings", val: SpecialVal::Fn(|caps: &Caps| {
        let damageType = ns(caps.cap[0], "index local 'damageType'");
        let baseType = ns(caps.cap[1], "index local 'baseType'");
        Some(vec![flag(cat!("Ignore", firstToUpper(damageType), "Resistance"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(baseType), "RingEquipped"), threshold = 2.0 })])])
    }), lua_line: 5497 });
    v.push(PatternEntry {
        pat: "the stars are aligned if you have 6 influence types among other equipped items",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:StarsAreAligned"),
            None,
            0,
            0,
            &[
                tag!({ type = MultiplierThreshold, var = "ShaperItem", threshold = 2.0 }),
                tag!({ type = MultiplierThreshold, var = "ElderItem", threshold = 2.0 }),
                tag!({ type = MultiplierThreshold, var = "WarlordItem", threshold = 2.0 }),
                tag!({ type = MultiplierThreshold, var = "HunterItem", threshold = 2.0 }),
                tag!({ type = MultiplierThreshold, var = "CrusaderItem", threshold = 2.0 }),
                tag!({ type = MultiplierThreshold, var = "RedeemerItem", threshold = 2.0 }),
            ],
        )]),
        lua_line: 5498,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on critical hit with attacks, no more than once every [%d%.]+ seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 5502,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on critical hit with attacks",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 5505,
    });
    v.push(PatternEntry {
        pat: "warcry skills' cooldown time is (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CooldownRecovery"),
                OVERRIDE,
                num,
                None,
                0,
                KeywordFlag::Warcry,
                &[],
            )])
        }),
        lua_line: 5508,
    });
    // hand-ported (lua2rs: tag-field-skilltype: nil); the Lua:
    // |     ["non%-instant warcries you use yourself have no cooldown"] = function(num) return { mod("CooldownRecovery", "OVERRIDE", 0, nil, 0, KeywordFlag.Warcry, { type = "SkillType", skillTypeList = { SkillType.Instant, SkillType.Totem, SkillType.Triggered }, neg = true }) } end,
    v.push(PatternEntry { pat: "non%-instant warcries you use yourself have no cooldown", val: SpecialVal::Fn(|caps: &Caps| {
        // hand-ported
        let num = caps.num;
        // LUA-QUIRK: SkillType.Totem does not exist (it is a ModFlag/KeywordFlag name), so the
        // Lua list is { SkillType.Instant, nil, SkillType.Triggered }; its only reader,
        // ModStore.lua:866, iterates it with pairs, which sees Instant and Triggered.
        Some(vec![r#mod(n!("CooldownRecovery"), OVERRIDE, 0.0, None, 0, KeywordFlag::Warcry, &[tag!({ type = SkillType, skillTypeList = &[SkillType::Instant, SkillType::Triggered][..], neg = true })])])
    }), lua_line: 5509 });
    v.push(PatternEntry {
        pat: "non%-instant warcries ignore their cooldown when used",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![r#mod(
                n!("CooldownRecovery"),
                OVERRIDE,
                0.0,
                None,
                0,
                KeywordFlag::Warcry,
                &[tag!({ type = SkillType, skillType = SkillType::Instant, neg = true })],
            )])
        }),
        lua_line: 5510,
    });
    v.push(PatternEntry {
        pat: "warcry skills have (%+%d+) seconds to cooldown",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CooldownRecovery"),
                BASE,
                num,
                None,
                0,
                KeywordFlag::Warcry,
                &[],
            )])
        }),
        lua_line: 5511,
    });
}

/// @pob Modules/ModParser.lua:5512-5611 h=743140a2
fn specialModList_5512(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "(%d+)%% increased total power counted by warcries",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("WarcryPower"), INC, num, None, 0, 0, &[])])
        }),
        lua_line: 5512,
    });
    v.push(PatternEntry {
        pat: "warcries have a minimum of (%d+) power",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinimumWarcryPower"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5513,
    });
    v.push(PatternEntry {
        pat: "stance skills have (%+%d+) seconds to cooldown",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CooldownRecovery"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Stance })],
            )])
        }),
        lua_line: 5514,
    });
    v.push(PatternEntry {
        pat: "using warcries is instant",
        val: SpecialVal::Mods(vec![flag(n!("InstantWarcry"), None, 0, 0, &[])]),
        lua_line: 5515,
    });
    v.push(PatternEntry {
        pat: "attacks with axes or swords grant (%d+) rage on hit, no more than once every second",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanGainRage"),
            None,
            0,
            0,
            &[tag!({ type = Condition, varList = &[n!("UsingAxe"), n!("UsingSword")][..] })],
        )]),
        lua_line: 5516,
    });
    v.push(PatternEntry { pat: "regenerate (%d+) rage per second for every (%d+) life recovery per second from regeneration", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        Some(vec![r#mod(n!("RageRegen"), BASE, num, None, 0, 0, &[tag!({ type = PercentStat, stat = "LifeRegen", percent = ((nn(num, "perform arithmetic on local 'num'") / snum(div, "perform arithmetic on a string")) * 100.0) })]), flag(n!("Condition:CanGainRage"), None, 0, 0, &[])])
    }), lua_line: 5519 });
    v.push(PatternEntry {
        pat: "when you lose temporal chains you gain maximum rage",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 5523,
    });
    v.push(PatternEntry { pat: "with a murderous eye jewel socketed, melee attacks grant (%d+) rage on hit, no more than once every second", val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[tag!({ type = Condition, var = "HaveMurderousEyeJewelIn{SlotName}" })])]), lua_line: 5524 });
    v.push(PatternEntry {
        pat: "gain %d+ rage after spending a total of %d+ mana",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 5525,
    });
    v.push(PatternEntry {
        pat: "rage grants cast speed instead of attack speed",
        val: SpecialVal::Mods(vec![flag(n!("Condition:RageCastSpeed"), None, 0, 0, &[])]),
        lua_line: 5526,
    });
    v.push(PatternEntry {
        pat: "rage grants spell damage instead of attack damage",
        val: SpecialVal::Mods(vec![flag(n!("Condition:RageSpellDamage"), None, 0, 0, &[])]),
        lua_line: 5527,
    });
    v.push(PatternEntry {
        pat: "inherent loss of rage is (%d+)%% slower",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("InherentRageLoss"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5528,
    });
    v.push(PatternEntry {
        pat: "inherent loss of rage is (%d+)%% faster",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("InherentRageLoss"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5529,
    });
    v.push(PatternEntry {
        pat: "inherent rage loss starts (%d+) seconds? later",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("InherentRageLossDelay"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5530,
    });
    v.push(PatternEntry {
        pat: "your critical strike multiplier is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritMultiplier"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5531,
    });
    v.push(PatternEntry {
        pat: "base critical strike chance for attacks with weapons is ([%d%.]+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("WeaponBaseCritChance"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5532,
    });
    // old wordings
    v.push(PatternEntry { pat: "base critical strike chance of spells is the critical strike chance of y?o?u?r? ?main hand weapon", val: SpecialVal::Mods(vec![flag(n!("BaseCritFromMainHand"), None, ModFlag::Spell, 0, &[])]), lua_line: 5533 });
    v.push(PatternEntry {
        pat: "base spell critical strike chance of spells is equal to that of main hand weapon",
        val: SpecialVal::Mods(vec![flag(
            n!("BaseCritFromMainHand"),
            None,
            ModFlag::Spell,
            0,
            &[],
        )]),
        lua_line: 5534,
    });
    v.push(PatternEntry {
        pat: "critical strike chance is (%d+)%% for hits with this weapon",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritChance"),
                OVERRIDE,
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
        lua_line: 5535,
    });
    v.push(PatternEntry {
        pat: "hits with this weapon have %+(%d+)%% to critical strike multiplier per enemy power",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritMultiplier"),
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
            )])
        }),
        lua_line: 5536,
    });
    v.push(PatternEntry {
        pat: "maximum critical strike chance is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritChanceCap"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5537,
    });
    v.push(PatternEntry {
        pat: "allocates (.+) if you have the matching modifiers? on forbidden (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let ascendancy = ns(caps.cap[0], "index local 'ascendancy'");
            let side = ns(caps.cap[1], "index local 'side'");
            Some(vec![r#mod(
                n!("GrantedAscendancyNode"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("side"), Value::from(&*side)),
                        (n!("name"), Value::from(&*ascendancy)),
                    ],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5540,
    });
    v.push(PatternEntry {
        pat: "allocates (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let passive = ns(caps.cap[0], "index local 'passive'");
            Some(vec![r#mod(
                n!("GrantedPassive"),
                LIST,
                &*passive,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5541,
    });
    v.push(PatternEntry {
        pat: "battlemage",
        val: SpecialVal::Mods(vec![
            flag(n!("Battlemage"), None, 0, 0, &[]),
            r#mod(
                n!("MainHandWeaponDamageAppliesToSpells"),
                MAX,
                100.0,
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 5542,
    });
    v.push(PatternEntry {
        pat: "transfiguration of body",
        val: SpecialVal::Mods(vec![flag(n!("TransfigurationOfBody"), None, 0, 0, &[])]),
        lua_line: 5543,
    });
    v.push(PatternEntry {
        pat: "transfiguration of mind",
        val: SpecialVal::Mods(vec![flag(n!("TransfigurationOfMind"), None, 0, 0, &[])]),
        lua_line: 5544,
    });
    v.push(PatternEntry {
        pat: "transfiguration of soul",
        val: SpecialVal::Mods(vec![flag(n!("TransfigurationOfSoul"), None, 0, 0, &[])]),
        lua_line: 5545,
    });
    v.push(PatternEntry { pat: "offering skills have (%d+)%% increased duration", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Duration"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Bone Offering"), n!("Flesh Offering"), n!("Spirit Offering"), n!("Blood Offering")][..] })])])
    }), lua_line: 5546 });
    v.push(PatternEntry { pat: "offering skills have (%d+)%% reduced duration", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Duration"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Bone Offering"), n!("Flesh Offering"), n!("Spirit Offering"), n!("Blood Offering")][..] })])])
    }), lua_line: 5549 });
    v.push(PatternEntry {
        pat: "enemies have %-(%d+)%% to total physical damage reduction against your hits",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyPhysicalDamageReduction"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5552,
    });
    v.push(PatternEntry { pat: "enemies you impale have %-(%d+)%% to total physical damage reduction against impale hits", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyImpalePhysicalDamageReduction"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[])])
    }), lua_line: 5555 });
    v.push(PatternEntry {
        pat: "hits with this weapon overwhelm (%d+)%% physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyPhysicalDamageReduction"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 5558,
    });
    v.push(PatternEntry {
        pat: "overwhelm (%d+)%% physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyPhysicalDamageReduction"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5561,
    });
    v.push(PatternEntry {
        pat: "hits have (%d+)%% chance to ignore enemy physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChanceToIgnoreEnemyPhysicalDamageReduction"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5564,
    });
    v.push(PatternEntry {
        pat: "hits with this weapon have (%d+)%% chance to ignore enemy physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ChanceToIgnoreEnemyPhysicalDamageReduction"),
                BASE,
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
        lua_line: 5567,
    });
    v.push(PatternEntry {
        pat: "hits with this weapon ignore enemy physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![r#mod(
                n!("ChanceToIgnoreEnemyPhysicalDamageReduction"),
                BASE,
                100.0,
                None,
                ModFlag::Hit,
                0,
                &[
                    tag!({ type = Condition, var = "{Hand}Attack" }),
                    tag!({ type = SkillType, skillType = SkillType::Attack }),
                ],
            )])
        }),
        lua_line: 5570,
    });
    v.push(PatternEntry {
        pat: "hits against you overwhelm (%d+)%% of physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyPhysicalOverwhelm"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5573,
    });
    v.push(PatternEntry { pat: "impale damage dealt to enemies impaled by you overwhelms (%d+)%% physical damage reduction", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyImpalePhysicalDamageReduction"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[])])
    }), lua_line: 5576 });
    v.push(PatternEntry {
        pat:
            "impale damage dealt to enemies impaled by you ignores enemy physical damage reduction",
        val: SpecialVal::Mods(vec![flag(
            n!("IgnoreEnemyImpalePhysicalDamageReduction"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5579,
    });
    // MultiplierThreshold is on RageStacks because Rage is only set in CalcPerform if Condition:CanGainRage is true, Bear's Girdle does not flag CanGainRage
    v.push(PatternEntry {
        pat: "nearby enemies are crushed while you have ?a?t? least (%d+) rage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:Crushed"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "RageStack", threshold = num })],
            )])
        }),
        lua_line: 5582,
    });
    v.push(PatternEntry {
        pat: "you are crushed",
        val: SpecialVal::Mods(vec![flag(n!("Condition:Crushed"), None, 0, 0, &[])]),
        lua_line: 5586,
    });
    v.push(PatternEntry {
        pat: "nearby enemies are crushed",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Crushed"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5587,
    });
    v.push(PatternEntry {
        pat: "crush enemies on hit with maces and sceptres",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Crushed"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingMace" })],
        )]),
        lua_line: 5588,
    });
    v.push(PatternEntry { pat: "you have fungal ground around you while stationary", val: SpecialVal::Mods(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosResist"), BASE, 25.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, varList = &[n!("OnFungalGround"), n!("Stationary")][..] })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChaosResist"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("OnFungalGround"), n!("Stationary")][..] })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ElementalResist"), BASE, -10.0, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("OnFungalGround"), n!("Stationary")][..] })])]), lua_line: 5589 });
    v.push(PatternEntry {
        pat: "create fungal ground instead of consecrated ground",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CreateFungalGround"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5594,
    });
    v.push(PatternEntry {
        pat: "create profane ground instead of consecrated ground",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CreateProfaneGround"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5597,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to create profane ground on critical strike if intelligence is your highest attribute", val: SpecialVal::Mods(vec![flag(n!("Condition:CreateProfaneGround"), None, 0, 0, &[tag!({ type = Condition, var = "IntHighestAttribute" })])]), lua_line: 5600 });
    v.push(PatternEntry { pat: "consecrated path and purifying flame create profane ground instead of consecrated ground", val: SpecialVal::Mods(vec![flag(n!("Condition:CreateProfaneGround"), None, 0, 0, &[])]), lua_line: 5603 });
    v.push(PatternEntry { pat: "you gain added cold damage instead of added damage of other types if dexterity exceeds both other attributes", val: SpecialVal::Mods(vec![flag(n!("AllAddedDamageAsCold"), None, 0, 0, &[tag!({ type = Condition, var = "DexSingleHighestAttribute" })])]), lua_line: 5606 });
    v.push(PatternEntry { pat: "you gain added lightn?ing damage instead of added damage of other types if intelligence exceeds both other attributes", val: SpecialVal::Mods(vec![flag(n!("AllAddedDamageAsLightning"), None, 0, 0, &[tag!({ type = Condition, var = "IntSingleHighestAttribute" })])]), lua_line: 5609 });
}

/// @pob Modules/ModParser.lua:5612-5702 h=5c9d18ab
fn specialModList_5612(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "elemental hit's added damage cannot be replaced this way",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5612,
    });
    v.push(PatternEntry { pat: "you have consecrated ground around you while stationary if strength is your highest attribute", val: SpecialVal::Mods(vec![flag(n!("Condition:OnConsecratedGround"), None, 0, 0, &[tag!({ type = Condition, var = "StrHighestAttribute" }), tag!({ type = Condition, var = "Stationary" })])]), lua_line: 5613 });
    v.push(PatternEntry { pat: "consecrated ground around you while stationary if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let thresh = ns(caps.cap[0], "index local 'thresh'");
        let influence = ns(caps.cap[1], "index local 'influence'");
        Some(vec![flag(n!("Condition:OnConsecratedGround"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 5616 });
    v.push(PatternEntry {
        pat: "you count as dual wielding while you are unencumbered",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:DualWielding"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Unencumbered" })],
        )]),
        lua_line: 5617,
    });
    v.push(PatternEntry {
        pat: "dual wielding does not inherently grant chance to block attack damage",
        val: SpecialVal::Mods(vec![flag(n!("Condition:NoInherentBlock"), None, 0, 0, &[])]),
        lua_line: 5618,
    });
    v.push(PatternEntry {
        pat: "inherent attack speed bonus from dual wielding is doubled while wielding two claws",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:DoubledInherentDualWieldingSpeed"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "DualWieldingClaws" })],
        )]),
        lua_line: 5619,
    });
    v.push(PatternEntry {
        pat: "inherent bonuses from dual wielding are doubled",
        val: SpecialVal::Mods(vec![
            flag(
                n!("Condition:DoubledInherentDualWieldingSpeed"),
                None,
                0,
                0,
                &[],
            ),
            flag(
                n!("Condition:DoubledInherentDualWieldingBlock"),
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 5622,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% reduced enemy chance to block sword attacks",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("reduceEnemyBlock"),
                BASE,
                num,
                None,
                ModFlag::Sword,
                0,
                &[],
            )])
        }),
        lua_line: 5626,
    });
    v.push(PatternEntry {
        pat: "you do not inherently take less damage for having fortification",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:NoFortificationMitigation"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5627,
    });
    v.push(PatternEntry {
        pat: "skills supported by intensify have %+(%d) to maximum intensity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:IntensityLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5628,
    });
    v.push(PatternEntry {
        pat: "spells which can gain intensity have %+(%d) to maximum intensity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:IntensityLimit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5629,
    });
    v.push(PatternEntry { pat: "final repeat of spells has (%d+)%% increased area of effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("RepeatFinalAreaOfEffect"), INC, num, None, ModFlag::Spell, 0, &[tag!({ type = Condition, var = "CastOnFrostbolt", neg = true }), tag!({ type = Condition, varList = &[n!("averageRepeat"), n!("alwaysFinalRepeat")][..] })])])
    }), lua_line: 5630 });
    v.push(PatternEntry {
        pat: "hexes you inflict have ([%+%-]%d+) to maximum doom",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("MaxDoom"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 5631,
    });
    v.push(PatternEntry { pat: "while stationary, gain (%d+)%% increased area of effect every second, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("AreaOfEffect"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "StationarySeconds", globalLimit = tonumber(limit), globalLimitKey = "ExpansiveMight" }), tag!({ type = Condition, var = "Stationary" })])])
    }), lua_line: 5632 });
    v.push(PatternEntry { pat: "fireball and rolling magma have (%d+)%% more area of effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AreaOfEffect"), MORE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Fireball"), n!("Rolling Magma")][..] })])])
    }), lua_line: 5635 });
    v.push(PatternEntry {
        pat: "attack skills have added lightning damage equal to (%d+)%% of maximum mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("LightningMin"),
                    BASE,
                    1.0,
                    None,
                    ModFlag::Attack,
                    0,
                    &[tag!({ type = PercentStat, stat = "Mana", percent = num })],
                ),
                r#mod(
                    n!("LightningMax"),
                    BASE,
                    1.0,
                    None,
                    ModFlag::Attack,
                    0,
                    &[tag!({ type = PercentStat, stat = "Mana", percent = num })],
                ),
            ])
        }),
        lua_line: 5636,
    });
    v.push(PatternEntry { pat: "arc and crackling lance gains added cold damage equal to (%d+)%% of mana cost, if mana cost is not higher than the maximum you could spend", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ColdMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "ManaCostPayablePool", thresholdStat = "ManaCost" }), tag!({ type = SkillName, skillNameList = &[n!("Arc"), n!("Crackling Lance")][..], includeTransfigured = true })]), r#mod(n!("ColdMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "ManaCostPayablePool", thresholdStat = "ManaCost" }), tag!({ type = SkillName, skillNameList = &[n!("Arc"), n!("Crackling Lance")][..], includeTransfigured = true })])])
    }), lua_line: 5640 });
    v.push(PatternEntry { pat: "forbidden rite and dark pact gains added chaos damage equal to (%d+)%% of mana cost, if mana cost is not higher than the maximum you could spend", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ChaosMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "ManaCostPayablePool", thresholdStat = "ManaCost" }), tag!({ type = SkillName, skillNameList = &[n!("Forbidden Rite"), n!("Dark Bargain")][..], includeTransfigured = true })]), r#mod(n!("ChaosMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "ManaCostPayablePool", thresholdStat = "ManaCost" }), tag!({ type = SkillName, skillNameList = &[n!("Forbidden Rite"), n!("Dark Bargain")][..], includeTransfigured = true })])])
    }), lua_line: 5644 });
    v.push(PatternEntry { pat: "skills gain added chaos damage equal to (%d+)%% of mana cost, if mana cost is not higher than the maximum you could spend", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ChaosMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "ManaCostPayablePool", thresholdStat = "ManaCost" })]), r#mod(n!("ChaosMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "ManaCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "ManaCostPayablePool", thresholdStat = "ManaCost" })])])
    }), lua_line: 5648 });
    v.push(PatternEntry {
        pat: "herald of thunder's storms hit enemies with (%d+)%% increased frequency",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("HeraldStormFrequency"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5652,
    });
    v.push(PatternEntry {
        pat: "storms hit enemies with (%d+)%% increased frequency",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("HeraldStormFrequency"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5653,
    });
    v.push(PatternEntry {
        pat: "your critical strikes have a (%d+)%% chance to deal double damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DoubleDamageChanceOnCrit"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5654,
    });
    v.push(PatternEntry { pat: "elemental skills deal triple damage", val: SpecialVal::Mods(vec![r#mod(n!("TripleDamageChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = SkillType, skillTypeList = &[SkillType::Cold, SkillType::Fire, SkillType::Lightning][..] })])]), lua_line: 5655 });
    v.push(PatternEntry { pat: "deal triple damage with elemental skills", val: SpecialVal::Mods(vec![r#mod(n!("TripleDamageChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = SkillType, skillTypeList = &[SkillType::Cold, SkillType::Fire, SkillType::Lightning][..] })])]), lua_line: 5656 });
    v.push(PatternEntry {
        pat: "skills supported by unleash have %+(%d) to maximum number of seals",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("SealCount"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 5657,
    });
    v.push(PatternEntry {
        pat: "left ring slot: skills supported by unleash have %+(%d) to maximum number of seals",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SealCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SlotNumber, num = 1.0 })],
            )])
        }),
        lua_line: 5658,
    });
    v.push(PatternEntry {
        pat: "skills supported by unleash have (%d+)%% increased seal gain frequency",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SealGainFrequency"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5659,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased critical strike chance with spells which remove the maximum number of seals", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MaxSealCrit"), INC, num, None, 0, 0, &[])])
    }), lua_line: 5660 });
    v.push(PatternEntry {
        pat: "gain elusive on critical strike",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeElusive"), None, 0, 0, &[])]),
        lua_line: 5661,
    });
    v.push(PatternEntry {
        pat: "gain a random shrine buff every (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveRegularShrines"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5662,
    });
    v.push(PatternEntry {
        pat: "gain a random shrine buff for (%d+) seconds when you kill a rare or unique enemy",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveRegularShrines"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5663,
    });
    v.push(PatternEntry {
        pat: "gain a random blood shrine buff every (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanHaveBloodShrines"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5664,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain elusive when you block while dual wielding",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanBeElusive"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "DualWielding" })],
        )]),
        lua_line: 5665,
    });
    v.push(PatternEntry {
        pat: "elusive on you reduces in effect (%d+)%% slower",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElusiveEffectLossSlower"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5666,
    });
    v.push(PatternEntry {
        pat: "elusive's effect on you is increased instead for the first (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElusiveEffectIncreaseDuration"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5667,
    });
    v.push(PatternEntry {
        pat: "elusive is removed from you at (%d+)%% effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElusiveEffectMinThreshold"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5668,
    });
    v.push(PatternEntry {
        pat: "nearby enemies have (%a+) resistance equal to yours",
        val: SpecialVal::Fn(|caps: &Caps| {
            let res = ns(caps.cap[0], "index local 'res'");
            Some(vec![flag(
                cat!(
                    "Enemy",
                    gsub_fn(res, "^%l", |c| Some(upper(c[0].as_str())), None).0,
                    "ResistEqualToYours"
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5669,
    });
    v.push(PatternEntry { pat: "for each nearby corpse, regenerate ([%d%.]+)%% life per second, up to ([%d%.]+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "NearbyCorpse", limit = tonumber(limit), limitTotal = true })])])
    }), lua_line: 5670 });
    v.push(PatternEntry { pat: "gain sacrificial zeal when you use a skill, dealing you %d+%% of the skill's mana cost as physical damage per second", val: SpecialVal::Mods(vec![flag(n!("SacrificialZeal"), None, 0, 0, &[])]), lua_line: 5671 });
    v.push(PatternEntry {
        pat: "skills gain a base life cost equal to (%d+)%% of base mana cost",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaCostAsLifeCost"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5674,
    });
    v.push(PatternEntry {
        pat: "skills gain a base energy shield cost equal to (%d+)%% of base mana cost",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaCostAsEnergyShieldCost"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5677,
    });
    v.push(PatternEntry {
        pat: "skills cost life instead of (%d+)%% of mana cost",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("HybridManaAndLifeCost_Life"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5680,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased cost of arc and crackling lance", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Cost"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Arc"), n!("Crackling Lance")][..], includeTransfigured = true })])])
    }), lua_line: 5683 });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry {
        pat: "hits overwhelm (%d+)%% of physical damage reduction while you have sacrificial zeal",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyPhysicalDamageReduction"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5686,
    });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry { pat: "(%d+)%% chance for hits to ignore enemy physical damage reduction while you have sacrificial zeal", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ChanceToIgnoreEnemyPhysicalDamageReduction"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 5689 });
    // LUA-QUIRK: modLib.createMod stores nil at mod[1] (a nil source/flags slot that is not a string/number), so ipairs(mod) never reaches the 1 tag(s) after it; they are dropped
    v.push(PatternEntry { pat: "hits have (%d+)%% chance to ignore enemy physical damage reduction while you have sacrificial zeal", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ChanceToIgnoreEnemyPhysicalDamageReduction"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 5692 });
    v.push(PatternEntry {
        pat: "minions attacks overwhelm (%d+)%% physical damage reduction",
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
                            n!("EnemyPhysicalDamageReduction"),
                            BASE,
                            (-nn(num, "perform arithmetic on local 'num'")),
                            None,
                            0,
                            0,
                            &[tag!({ type = SkillType, skillType = SkillType::Attack })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5695,
    });
    v.push(PatternEntry {
        pat: "minions hits have (%d+)%% chance to ignore enemy physical damage reduction",
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
                            n!("ChanceToIgnoreEnemyPhysicalDamageReduction"),
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
        lua_line: 5698,
    });
    v.push(PatternEntry {
        pat: "focus has (%d+)%% increased cooldown recovery rate",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FocusCooldownRecovery"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Focused" })],
            )])
        }),
        lua_line: 5701,
    });
    v.push(PatternEntry {
        pat: "focus has (%d+)%% reduced cooldown recovery rate",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("FocusCooldownRecovery"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Focused" })],
            )])
        }),
        lua_line: 5702,
    });
}

/// @pob Modules/ModParser.lua:5703-5777 h=c8a0c017
fn specialModList_5703(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "(%d+)%% more frozen legion and general's cry cooldown recovery rate", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CooldownRecovery"), MORE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Frozen Legion"), n!("General's Cry")][..], includeTransfigured = true })])])
    }), lua_line: 5703 });
    v.push(PatternEntry { pat: "flamethrower, seismic and lightning spire trap have (%d+)%% increased cooldown recovery rate", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CooldownRecovery"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flamethrower Trap"), n!("Seismic Trap"), n!("Lightning Spire Trap")][..], includeTransfigured = true })])])
    }), lua_line: 5704 });
    v.push(PatternEntry { pat: "flamethrower, seismic and lightning spire trap have %-(%d+) cooldown uses?", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AdditionalCooldownUses"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Flamethrower Trap"), n!("Seismic Trap"), n!("Lightning Spire Trap")][..], includeTransfigured = true })])])
    }), lua_line: 5705 });
    v.push(PatternEntry {
        pat: "right ring slot: shockwave has %+(%d+) to cooldown uses?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AdditionalCooldownUses"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SkillName, skillName = "Shockwave" }),
                    tag!({ type = SlotNumber, num = 2.0 }),
                ],
            )])
        }),
        lua_line: 5706,
    });
    v.push(PatternEntry {
        pat: "flameblast starts with (%d+) additional stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:FlameblastMinimumStage"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = GlobalEffect, effectType = "Buff", unscalable = true })],
            )])
        }),
        lua_line: 5707,
    });
    v.push(PatternEntry {
        pat: "incinerate starts with (%d+) additional stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:IncinerateMinimumStage"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = GlobalEffect, effectType = "Buff", unscalable = true })],
            )])
        }),
        lua_line: 5708,
    });
    v.push(PatternEntry { pat: "%+([%d%.]+) seconds to flameblast and incinerate cooldown", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("SkillData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("cooldown")), (n!("value"), Value::from(0.0))]), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Incinerate"), n!("Flameblast")][..], includeTransfigured = true })]), r#mod(n!("CooldownRecovery"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Incinerate"), n!("Flameblast")][..], includeTransfigured = true })])])
    }), lua_line: 5709 });
    v.push(PatternEntry { pat: "(%d+)%% chance to deal double damage with attacks if attack time is longer than 1 second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "OneSecondAttackTime" })])])
    }), lua_line: 5712 });
    v.push(PatternEntry { pat: "elusive also grants %+(%d+)%% to critical strike multiplier for skills supported by nightblade", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("NightbladeElusiveCritMultiplier"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 5715 });
    v.push(PatternEntry {
        pat: "skills supported by nightblade have (%d+)%% increased effect of elusive",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("NightbladeSupportedElusiveEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5716,
    });
    v.push(PatternEntry {
        pat: "nearby enemies are scorched",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:Scorched"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            ),
            r#mod(n!("ScorchBase"), BASE, 10.0, None, 0, 0, &[]),
        ]),
        lua_line: 5717,
    });
    v.push(PatternEntry {
        pat: "hits have (%d+)%% chance to ignore enemy monster physical damage reduction",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("PartialIgnoreEnemyPhysicalDamageReduction"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5721,
    });
    v.push(PatternEntry { pat: "attacks you use yourself have (%d+)%% more attack speed", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Speed"), MORE, num, None, ModFlag::Attack, 0, &[tag!({ type = SkillType, neg = true, skillTypeList = &[SkillType::SummonsTotem, SkillType::RemoteMined, SkillType::Trapped, SkillType::Triggered][..] }), tag!({ type = Condition, neg = true, var = "usedByMirage" })])])
    }), lua_line: 5724 });
    v.push(PatternEntry { pat: "attacks you use yourself repeat an additional time", val: SpecialVal::Mods(vec![r#mod(n!("RepeatCount"), BASE, 1.0, None, ModFlag::Attack, 0, &[tag!({ type = SkillType, neg = true, skillTypeList = &[SkillType::SummonsTotem, SkillType::RemoteMined, SkillType::Trapped, SkillType::Triggered][..] }), tag!({ type = Condition, neg = true, var = "usedByMirage" }), tag!({ type = Condition, varList = &[n!("averageRepeat"), n!("alwaysFinalRepeat")][..] })])]), lua_line: 5727 });
    v.push(PatternEntry {
        pat: "final repeat of attack skills deals (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("RepeatFinalDamage"),
                MORE,
                num,
                None,
                0,
                KeywordFlag::Attack,
                &[],
            )])
        }),
        lua_line: 5730,
    });
    v.push(PatternEntry { pat: "non%-travel attack skills repeat an additional time", val: SpecialVal::Mods(vec![r#mod(n!("RepeatCount"), BASE, 1.0, None, 0, KeywordFlag::Attack, &[tag!({ type = SkillType, skillType = SkillType::Travel, neg = true }), tag!({ type = Condition, varList = &[n!("averageRepeat"), n!("alwaysFinalRepeat")][..] })])]), lua_line: 5731 });
    v.push(PatternEntry { pat: "viper strike and pestilent strike deal (%d+)%% increased attack damage per frenzy charge", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, ModFlag::Attack, 0, &[tag!({ type = Multiplier, var = "FrenzyCharge" }), tag!({ type = SkillName, skillNameList = &[n!("Viper Strike"), n!("Pestilent Strike")][..], includeTransfigured = true })])])
    }), lua_line: 5732 });
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber
    v.push(PatternEntry { pat: "shield charge and chain hook have (%d+)%% increased attack speed per (%d+) rampage kills", val: SpecialVal::Fn(|caps: &Caps| {
        let inc = caps.num;
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("Speed"), INC, inc, None, ModFlag::Attack, 0, &[tag!({ type = Multiplier, var = "Rampage", div = snum(num, "perform arithmetic on tag.div"), limit = (1000.0 / snum(num, "perform arithmetic on a string")), limitTotal = true }), tag!({ type = SkillName, skillNameList = &[n!("Shield Charge"), n!("Chain Hook")][..], includeTransfigured = true })])])
    }), lua_line: 5733 });
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber
    v.push(PatternEntry { pat: "tectonic slam and infernal blow deal (%d+)%% increased attack damage per (%d+) armour", val: SpecialVal::Fn(|caps: &Caps| {
        let inc = caps.num;
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("Damage"), INC, inc, None, ModFlag::Attack, 0, &[tag!({ type = PerStat, stat = "Armour", div = snum(num, "perform arithmetic on tag.div") }), tag!({ type = SkillName, skillNameList = &[n!("Tectonic Slam"), n!("Infernal Blow")][..], includeTransfigured = true })])])
    }), lua_line: 5734 });
    v.push(PatternEntry { pat: "frozen sweep deals (%d+)%% less damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), MORE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = SkillName, skillName = "Frozen Sweep", includeTransfigured = true })])])
    }), lua_line: 5735 });
    v.push(PatternEntry { pat: "ice trap and lightning trap damage penetrates (%d+)%% of enemy elemental resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("LightningPenetration"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Ice Trap"), n!("Lightning Trap")][..], includeTransfigured = true })]), r#mod(n!("ColdPenetration"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Ice Trap"), n!("Lightning Trap")][..], includeTransfigured = true })]), r#mod(n!("FirePenetration"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Ice Trap"), n!("Lightning Trap")][..], includeTransfigured = true })])])
    }), lua_line: 5736 });
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber
    v.push(PatternEntry { pat: "volatile dead and cremation penetrate (%d+)%% fire resistance per (%d+) dexterity", val: SpecialVal::Fn(|caps: &Caps| {
        let inc = caps.num;
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("FirePenetration"), BASE, inc, None, 0, 0, &[tag!({ type = PerStat, stat = "Dex", div = snum(num, "perform arithmetic on tag.div") }), tag!({ type = SkillName, skillNameList = &[n!("Volatile Dead"), n!("Cremation")][..], includeTransfigured = true })])])
    }), lua_line: 5741 });
    v.push(PatternEntry { pat: "regenerate (%d+) mana per second while any enemy is in your righteous fire or scorching ray", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ManaRegen"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "InRFOrScorchingRay" })])])
    }), lua_line: 5742 });
    v.push(PatternEntry { pat: "%+(%d+)%% to wave of conviction damage over time multiplier per ([%d%.]+) seconds of duration expired", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("WaveOfConvictionDurationDotMulti"), INC, num, None, 0, 0, &[])])
    }), lua_line: 5743 });
    v.push(PatternEntry { pat: "when an enemy hit deals elemental damage to you, their resistance to those elements becomes zero for (%d+) seconds", val: SpecialVal::Mods(vec![flag(n!("Condition:HaveTrickstersSmile"), None, 0, 0, &[])]), lua_line: 5744 });
    // Conditional Player Quantity / Rarity
    v.push(PatternEntry {
        pat: "(%d+)%% increased quantity of items dropped by slain normal enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LootQuantityNormalEnemies"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5746,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased rarity of items dropped by slain magic enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LootRarityMagicEnemies"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5747,
    });
    // Pantheon: Soul of Tukohama support
    v.push(PatternEntry { pat: "while stationary, gain ([%d%.]+)%% of life regenerated per second every second, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "StationarySeconds", limit = tonumber(limit), limitTotal = true }), tag!({ type = Condition, var = "Stationary" })])])
    }), lua_line: 5749 });
    // Pantheon: Soul of Ryslatha support
    v.push(PatternEntry { pat: "life flasks gain (%d+) charges? every (%d+) seconds if you haven't used a life flask recently", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        Some(vec![r#mod(n!("LifeFlaskChargesGenerated"), BASE, (nn(num, "perform arithmetic on local 'num'") / snum(div, "perform arithmetic on a string")), None, 0, 0, &[tag!({ type = Condition, var = "UsingLifeFlask", neg = true })])])
    }), lua_line: 5753 });
    // Skill-specific enchantment modifiers
    v.push(PatternEntry {
        pat: "(%d+)%% increased decoy totem life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("TotemLife"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Decoy Totem" })],
            )])
        }),
        lua_line: 5757,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased ice spear critical strike chance in second form",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CritChance"),
                INC,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SkillName, skillName = "Ice Spear", includeTransfigured = true }),
                    tag!({ type = SkillPart, skillPartList = &[2.0, 4.0][..] }),
                ],
            )])
        }),
        lua_line: 5758,
    });
    v.push(PatternEntry { pat: "shock nova ring deals (%d+)%% increased damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Shock Nova", includeTransfigured = true }), tag!({ type = SkillPart, skillPart = 1.0 })])])
    }), lua_line: 5759 });
    v.push(PatternEntry {
        pat: "enemies affected by bear trap take (%d+)%% increased damage from trap or mine hits",
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
                            n!("EnemyModifier"),
                            LIST,
                            vtab(
                                vec![],
                                vec![(
                                    n!("mod"),
                                    Value::from(r#mod(
                                        n!("TrapMineDamageTaken"),
                                        INC,
                                        num,
                                        None,
                                        0,
                                        0,
                                        &[tag!({ type = GlobalEffect, effectType = "Debuff" })],
                                    )),
                                )],
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
                &[tag!({ type = SkillName, skillName = "Bear Trap", includeTransfigured = true })],
            )])
        }),
        lua_line: 5760,
    });
    v.push(PatternEntry { pat: "blade vortex has %+(%d+)%% to critical strike multiplier for each blade", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritMultiplier"), BASE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "BladeVortexBlade" }), tag!({ type = SkillName, skillName = "Blade Vortex", includeTransfigured = true })])])
    }), lua_line: 5761 });
    v.push(PatternEntry {
        pat: "burning arrow has (%d+)%% increased debuff effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DebuffEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Burning Arrow" })],
            )])
        }),
        lua_line: 5762,
    });
    v.push(PatternEntry { pat: "double strike has a (%d+)%% chance to deal double damage to bleeding enemies", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DoubleDamageChance"), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" }), tag!({ type = SkillName, skillName = "Double Strike", includeTransfigured = true })])])
    }), lua_line: 5763 });
    v.push(PatternEntry {
        pat: "frost bomb has (%d+)%% increased debuff duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SecondaryDuration"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Frost Bomb" })],
            )])
        }),
        lua_line: 5764,
    });
    v.push(PatternEntry {
        pat: "incinerate has %+(%d+) to maximum stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:IncinerateMaxStages"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Incinerate" })],
            )])
        }),
        lua_line: 5765,
    });
    v.push(PatternEntry {
        pat: "perforate creates %+(%d+) spikes?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:PerforateMaxSpikes"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5766,
    });
    v.push(PatternEntry { pat: "scourge arrow has (%d+)%% chance to poison per stage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PoisonChance"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Scourge Arrow", includeTransfigured = true }), tag!({ type = Multiplier, var = "ScourgeArrowStage" })])])
    }), lua_line: 5767 });
    v.push(PatternEntry {
        pat: "winter orb has %+(%d+) maximum stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:WinterOrbMaxStages"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5768,
    });
    v.push(PatternEntry { pat: "summoned holy relics have (%d+)%% increased buff effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("BuffEffect"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Summon Holy Relic", includeTransfigured = true })])])
    }), lua_line: 5769 });
    v.push(PatternEntry {
        pat: "%+(%d+) to maximum virulence",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:VirulenceStacksMax"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5770,
    });
    v.push(PatternEntry {
        pat: "winter orb has (%d+)%% increased area of effect per stage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SkillName, skillName = "Winter Orb" }),
                    tag!({ type = Multiplier, var = "WinterOrbStage" }),
                ],
            )])
        }),
        lua_line: 5771,
    });
    v.push(PatternEntry {
        pat: "wintertide brand has %+(%d+) to maximum stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:WintertideBrandMaxStages"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Wintertide Brand" })],
            )])
        }),
        lua_line: 5772,
    });
    v.push(PatternEntry {
        pat: "wave of conviction's exposure applies (%-%d+)%% elemental resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillStat"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (
                            n!("key"),
                            Value::from("purge_expose_resist_%_matching_highest_element_damage"),
                        ),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Wave of Conviction" })],
            )])
        }),
        lua_line: 5773,
    });
    v.push(PatternEntry {
        pat: "wave of conviction's exposure applies an extra (%-%d+)%% to elemental resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillStat"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (
                            n!("key"),
                            Value::from("purge_expose_resist_%_matching_highest_element_damage"),
                        ),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Wave of Conviction" })],
            )])
        }),
        lua_line: 5774,
    });
    v.push(PatternEntry {
        pat: "arcane cloak spends an additional (%d+)%% of current mana",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillStat"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("arcane_cloak_consume_%_of_mana")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Arcane Cloak" })],
            )])
        }),
        lua_line: 5775,
    });
    v.push(PatternEntry {
        pat: "arcane cloak grants life regeneration equal to (%d+)%% of mana spent per second",
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
                            n!("LifeRegen"),
                            BASE,
                            (nn(num, "perform arithmetic on local 'num'") / 100.0),
                            None,
                            0,
                            0,
                            &[
                                tag!({ type = Multiplier, var = "ArcaneCloakConsumedMana" }),
                                tag!({ type = GlobalEffect, effectType = "Buff" }),
                            ],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Arcane Cloak" })],
            )])
        }),
        lua_line: 5776,
    });
    v.push(PatternEntry { pat: "caustic arrow has (%d+)%% chance to inflict withered on hit for (%d+) seconds base duration", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:CanWither"), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Caustic Arrow", includeTransfigured = true })])]), lua_line: 5777 });
}

/// @pob Modules/ModParser.lua:5778-5828 h=2de6268a
fn specialModList_5778(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "venom gyre has a (%d+)%% chance to inflict withered for (%d+) seconds on hit",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraSkillMod"),
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
            &[tag!({ type = SkillName, skillName = "Venom Gyre" })],
        )]),
        lua_line: 5778,
    });
    v.push(PatternEntry { pat: "sigil of power's buff also grants (%d+)%% increased critical strike chance per stage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritChance"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "SigilOfPowerStage", limit = 4.0 }), tag!({ type = GlobalEffect, effectType = "Buff", effectName = "Sigil of Power" })])])
    }), lua_line: 5779 });
    v.push(PatternEntry {
        pat: "cobra lash chains (%d+) additional times",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillMod"),
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
                &[tag!({ type = SkillName, skillName = "Cobra Lash" })],
            )])
        }),
        lua_line: 5780,
    });
    v.push(PatternEntry {
        pat: "general's cry has ([%+%-]%d) to maximum number of mirage warriors",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GeneralsCryDoubleMaxCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5781,
    });
    v.push(PatternEntry {
        pat: "([%+%-]%d) to maximum blade flurry stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Multiplier:BladeFlurryMaxStages"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("Multiplier:BladeFlurryofIncisionMaxStages"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5782,
    });
    v.push(PatternEntry {
        pat: "steelskin buff can take (%d+)%% increased amount of damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillStat"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("steelskin_damage_limit_+%")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Steelskin" })],
            )])
        }),
        lua_line: 5783,
    });
    v.push(PatternEntry {
        pat: "hydrosphere has (%d+)%% increased pulse frequency",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("HydroSphereFrequency"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5784,
    });
    v.push(PatternEntry {
        pat: "void sphere has (%d+)%% increased pulse frequency",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("VoidSphereFrequency"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5785,
    });
    v.push(PatternEntry { pat: "shield crush central wave has (%d+)%% more area of effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AreaOfEffect"), MORE, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Shield Crush", includeTransfigured = true }), tag!({ type = SkillPart, skillPart = 2.0 })])])
    }), lua_line: 5786 });
    v.push(PatternEntry {
        pat: "storm rain has (%d+)%% increased beam frequency",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("StormRainBeamFrequency"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5787,
    });
    v.push(PatternEntry {
        pat: "voltaxic burst deals (%d+)%% increased damage per ([%d%.]+) seconds of duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("VoltaxicDurationIncDamage"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5788,
    });
    v.push(PatternEntry {
        pat: "earthquake deals (%d+)%% increased damage per ([%d%.]+) seconds duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EarthquakeDurationIncDamage"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5789,
    });
    v.push(PatternEntry { pat: "consecrated ground from holy flame totem applies (%d+)%% increased damage taken to enemies", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("DamageTakenConsecratedGround"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "OnConsecratedGround" })])))]), None, 0, 0, &[])])
    }), lua_line: 5790 });
    v.push(PatternEntry { pat: "consecrated ground from purifying flame applies (%d+)%% increased damage taken to enemies", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillStat"), LIST, vtab(vec![], vec![(n!("key"), Value::from("consecrated_ground_enemy_damage_taken_+%")), (n!("value"), Value::from(num))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Purifying Flame", includeTransfigured = true })])])
    }), lua_line: 5791 });
    v.push(PatternEntry { pat: "enemies drenched by hydrosphere have cold and lightning exposure, applying (%-%d+)%% to resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillStat"), LIST, vtab(vec![], vec![(n!("key"), Value::from("water_sphere_cold_lightning_exposure_%")), (n!("value"), Value::from(num))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Hydrosphere" })])])
    }), lua_line: 5792 });
    v.push(PatternEntry {
        pat: "frost shield has %+(%d+) to maximum life per stage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraSkillStat"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("key"), Value::from("frost_globe_health_per_stage")),
                        (n!("value"), Value::from(num)),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Frost Shield" })],
            )])
        }),
        lua_line: 5793,
    });
    v.push(PatternEntry {
        pat: "flame wall grants (%d+) to (%d+) added fire damage to projectiles",
        val: SpecialVal::Fn(|caps: &Caps| {
            let min = ns(caps.cap[0], "index local 'min'");
            let max = ns(caps.cap[1], "index local 'max'");
            Some(vec![
                r#mod(
                    n!("ExtraSkillStat"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (
                                n!("key"),
                                Value::from("flame_wall_minimum_added_fire_damage"),
                            ),
                            (n!("value"), Value::from(&*min)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = "Flame Wall" })],
                ),
                r#mod(
                    n!("ExtraSkillStat"),
                    LIST,
                    vtab(
                        vec![],
                        vec![
                            (
                                n!("key"),
                                Value::from("flame_wall_maximum_added_fire_damage"),
                            ),
                            (n!("value"), Value::from(&*max)),
                        ],
                    ),
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillName, skillName = "Flame Wall" })],
                ),
            ])
        }),
        lua_line: 5794,
    });
    v.push(PatternEntry { pat: "plague bearer buff grants %+(%d+)%% to poison damage over time multiplier while infecting", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillStat"), LIST, vtab(vec![], vec![(n!("key"), Value::from("corrosive_shroud_poison_dot_multiplier_+_while_aura_active")), (n!("value"), Value::from(num))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Plague Bearer" })])])
    }), lua_line: 5795 });
    v.push(PatternEntry { pat: "(%d+)%% increased lightning trap lightning ailment effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillStat"), LIST, vtab(vec![], vec![(n!("key"), Value::from("shock_effect_+%")), (n!("value"), Value::from(num))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Lightning Trap", includeTransfigured = true })])])
    }), lua_line: 5796 });
    v.push(PatternEntry { pat: "wild strike's beam chains an additional (%d+) times", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ChainCountMax"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillName = "Wild Strike", includeTransfigured = true }), tag!({ type = SkillPart, skillPart = 4.0 })])])
    }), lua_line: 5797 });
    v.push(PatternEntry {
        pat: "energy blades have (%d+)%% increased attack speed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyBladeAttackSpeed"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5798,
    });
    v.push(PatternEntry {
        pat: "ensnaring arrow has (%d+)%% increased debuff effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DebuffEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Ensnaring Arrow" })],
            )])
        }),
        lua_line: 5799,
    });
    v.push(PatternEntry {
        pat: "unearth spawns corpses with ([%+%-]%d) level",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CorpseLevel"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Unearth" })],
            )])
        }),
        lua_line: 5800,
    });
    v.push(PatternEntry {
        pat: "seismic trap releases an additional wave",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MaximumWaves"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Seismic Trap", includeTransfigured = true })],
        )]),
        lua_line: 5801,
    });
    v.push(PatternEntry { pat: "lightning spire trap strikes an additional area", val: SpecialVal::Mods(vec![r#mod(n!("MaximumWaves"), BASE, 1.0, None, 0, 0, &[tag!({ type = SkillName, skillName = "Lightning Spire Trap", includeTransfigured = true })])]), lua_line: 5802 });
    v.push(PatternEntry { pat: "explosive trap causes (%d+) additional smaller explosions", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SmallExplosions"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Explosive Trap"), n!("Explosive Trap of Swells")][..] })])])
    }), lua_line: 5803 });
    v.push(PatternEntry { pat: "frozen sweep deals (%d+)%% increased damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Frozen Sweep", includeTransfigured = true })])])
    }), lua_line: 5804 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased attack speed with snipe",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Speed"),
                INC,
                num,
                None,
                ModFlag::Attack,
                0,
                &[tag!({ type = SkillName, skillName = "Snipe" })],
            )])
        }),
        lua_line: 5805,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) to maximum snipe stages",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:SnipeStagesMax"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = GlobalEffect, effectType = "Buff", unscalable = true })],
            )])
        }),
        lua_line: 5806,
    });
    v.push(PatternEntry { pat: "chain hook has %+([%d%.]+) metres? to radius per (%d+) rage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let rage = ns(caps.cap[1], "index local 'rage'");
        Some(vec![r#mod(n!("AreaOfEffect"), BASE, (nn(num, "perform arithmetic on local 'num'") * 10.0), None, 0, 0, &[tag!({ type = PerStat, stat = "Rage", div = tonumber(rage) }), tag!({ type = SkillName, skillName = "Chain Hook", includeTransfigured = true })])])
    }), lua_line: 5807 });
    v.push(PatternEntry {
        pat: "%+([%d%.]+) metres? to discharge radius",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'") * 10.0),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Discharge", includeTransfigured = true })],
            )])
        }),
        lua_line: 5808,
    });
    // Alternate Quality
    v.push(PatternEntry {
        pat: "quality does not increase physical damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("AlternateQualityWeapon"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5810,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased critical strike chance per 4%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AlternateQualityLocalCritChancePer4Quality"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5811,
    });
    v.push(PatternEntry {
        pat: "grants (%d+)%% increased accuracy per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("Accuracy"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5812,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased attack speed per 8%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AlternateQualityLocalAttackSpeedPer8Quality"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5813,
    });
    v.push(PatternEntry {
        pat: "%+(%d+) weapon range per 10%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AlternateQualityLocalWeaponRangePer10Quality"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5814,
    });
    v.push(PatternEntry {
        pat: "%+([%d%.]+) metres? to weapon range per 10%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AlternateQualityLocalWeaponRangePer10Quality"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'") * 10.0),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5815,
    });
    v.push(PatternEntry {
        pat: "grants (%d+)%% increased elemental damage per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("ElementalDamage"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5816,
    });
    v.push(PatternEntry {
        pat: "grants (%d+)%% increased area of effect per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5817,
    });
    v.push(PatternEntry {
        pat: "quality does not increase defences",
        val: SpecialVal::Mods(vec![r#mod(
            n!("AlternateQualityArmour"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5818,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+) to maximum life per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("Life"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5819,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+) to maximum mana per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("Mana"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5820,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+) to strength per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("Str"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5821,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+) to dexterity per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("Dex"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5822,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+) to intelligence per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("Int"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5823,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+)%% to fire resistance per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("FireResist"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5824,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+)%% to cold resistance per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("ColdResist"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5825,
    });
    v.push(PatternEntry {
        pat: "grants %+(%d+)%% to lightning resistance per (%d+)%% quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("LightningResist"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "QualityOn{SlotName}", div = tonumber(div) })],
            )])
        }),
        lua_line: 5826,
    });
    v.push(PatternEntry {
        pat: "%+(%d+)%% to quality",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("Quality"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 5827,
    });
    v.push(PatternEntry { pat: "infernal blow debuff deals an additional (%d+)%% of damage per charge", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DebuffEffect"), BASE, num, None, 0, 0, &[tag!({ type = SkillName, skillName = "Infernal Blow", includeTransfigured = true })])])
    }), lua_line: 5828 });
}

/// @pob Modules/ModParser.lua:5829-5918 h=6b8a3aa2
fn specialModList_5830(v: &mut Vec<PatternEntry<SpecialVal>>) {
    // Legion modifiers
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["bathed in the blood of (%d+) sacrificed in the name of (.+)"] =  function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "bathed in the blood of (%d+) sacrificed in the name of (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5830,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["carved to glorify (%d+) new faithful converted by high templar (.+)"] =  function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "carved to glorify (%d+) new faithful converted by high templar (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5833,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["commanded leadership over (%d+) warriors under (.+)"] =  function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "commanded leadership over (%d+) warriors under (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5836,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["commissioned (%d+) coins to commemorate (.+)"] =  function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "commissioned (%d+) coins to commemorate (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5839,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["denoted service of (%d+) dekhara in the akhara of (.+)"] =  function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "denoted service of (%d+) dekhara in the akhara of (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5842,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["remembrancing (%d+) songworthy deeds by the line of (.+)"] =  function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "remembrancing (%d+) songworthy deeds by the line of (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5845,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["subjugating (%d+) souls in the thrall of (.+)"] = function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "subjugating (%d+) souls in the thrall of (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5848,
    });
    // hand-ported (lua2rs: index: conquerorList[name:lower()]); the Lua:
    // |     ["binding (%d+) souls to phylacteries to sustain (.+)"] = function(num, _, name)
    // |         return { mod("JewelData", "LIST",
    // |                 { key = "conqueredBy", value = { id = num, conqueror = conquerorList[name:lower()] } }) } end,
    v.push(PatternEntry {
        pat: "binding (%d+) souls to phylacteries to sustain (.+)",
        val: SpecialVal::Fn(conquered_by),
        lua_line: 5851,
    });
    v.push(PatternEntry {
        pat: "passives in radius are conquered by the (%D+)",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5854,
    });
    v.push(PatternEntry {
        pat: "passives affected are conquered by the abyssal",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5855,
    });
    v.push(PatternEntry {
        pat: "historic",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5856,
    });
    // Tattoos
    v.push(PatternEntry {
        pat: "+(%d+) to maximum life per allocated journey tattoo of the body",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Life"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "JourneyTattooBody" })],
                ),
                r#mod(
                    n!("Multiplier:JourneyTattooBody"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5858,
    });
    v.push(PatternEntry {
        pat: "+(%d+) to maximum energy shield per allocated journey tattoo of the soul",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("EnergyShield"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "JourneyTattooSoul" })],
                ),
                r#mod(
                    n!("Multiplier:JourneyTattooSoul"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5862,
    });
    v.push(PatternEntry {
        pat: "+(%d+) to maximum mana per allocated journey tattoo of the mind",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Mana"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "JourneyTattooMind" })],
                ),
                r#mod(
                    n!("Multiplier:JourneyTattooMind"),
                    BASE,
                    1.0,
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 5866,
    });
    // Display-only modifiers
    v.push(PatternEntry {
        pat: "extra gore",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5871,
    });
    v.push(PatternEntry {
        pat: "prefixes:",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5872,
    });
    v.push(PatternEntry {
        pat: "suffixes:",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5873,
    });
    v.push(PatternEntry {
        pat: "while your passive skill tree connects to a class' starting location, you gain:",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5874,
    });
    v.push(PatternEntry { pat: "socketed lightning spells [hd][ae][va][el] (%d+)%% increased spell damage if triggered", val: SpecialVal::Mods(vec![]), lua_line: 5875 });
    v.push(PatternEntry {
        pat: "manifeste?d? dancing dervishe?s? disables both weapon slots",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5876,
    });
    v.push(PatternEntry {
        pat: "manifeste?d? dancing dervishe?s? dies? when rampage ends",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5877,
    });
    v.push(PatternEntry {
        pat: "survival",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5878,
    });
    v.push(PatternEntry {
        pat: "you can have two different banners at the same time",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5879,
    });
    v.push(PatternEntry {
        pat: "[%+%-](%d+) prefix modifiers? allowed",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5880,
    });
    v.push(PatternEntry {
        pat: "[%+%-](%d+) suffix modifiers? allowed",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5881,
    });
    v.push(PatternEntry {
        pat: "can have a second enchantment modifier",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5882,
    });
    v.push(PatternEntry {
        pat: "can have (%d+) additional enchantment modifiers",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5883,
    });
    v.push(PatternEntry {
        pat: "this item can be anointed by cassia",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5884,
    });
    v.push(PatternEntry {
        pat: "can be anointed",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5885,
    });
    v.push(PatternEntry {
        pat: "implicit modifiers cannot be changed",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5886,
    });
    v.push(PatternEntry {
        pat: "has a crucible passive skill tree",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5887,
    });
    v.push(PatternEntry {
        pat: "has elder, shaper and all conqueror influences",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5888,
    });
    v.push(PatternEntry {
        pat: "has a two handed sword crucible passive skill tree",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5889,
    });
    v.push(PatternEntry {
        pat: "has a crucible passive skill tree with only support passive skills",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5890,
    });
    v.push(PatternEntry {
        pat: "crucible passive skill tree is removed if this modifier is removed",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5891,
    });
    v.push(PatternEntry {
        pat: "all sockets are white",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5892,
    });
    v.push(PatternEntry {
        pat: "cannot roll (%a+) modifiers",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5893,
    });
    v.push(PatternEntry {
        pat: "cannot roll modifiers of non%-(%a+) damage types",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5894,
    });
    v.push(PatternEntry {
        pat: "every (%d+) seconds, regenerate (%d+)%% of life over one second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let percent = ns(caps.cap[1], "index local 'percent'");
            Some(vec![
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    tonumber(percent),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "LifeRegenBurstFull" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    (nn(tonumber(percent), "perform arithmetic on a nil value")
                        / nn(num, "perform arithmetic on local 'num'")),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "LifeRegenBurstAvg" })],
                ),
            ])
        }),
        lua_line: 5895,
    });
    v.push(PatternEntry { pat: "every (%d+) seconds, regenerate (%d+)%% of life over one second if (%d+) (%a+) items are equipped", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let percent = ns(caps.cap[1], "index local 'percent'");
        let thresh = ns(caps.cap[2], "index local 'thresh'");
        let influence = ns(caps.cap[3], "index local 'influence'");
        Some(vec![r#mod(n!("LifeRegenPercent"), BASE, tonumber(percent), None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstFull" }), tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })]), r#mod(n!("LifeRegenPercent"), BASE, (nn(tonumber(percent), "perform arithmetic on a nil value") / nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = Condition, var = "LifeRegenBurstAvg" }), tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(influence), "Item"), threshold = tonumber(thresh) })])])
    }), lua_line: 5899 });
    v.push(PatternEntry {
        pat: "take no extra damage from critical strikes",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ReduceCritExtraDamage"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 5903,
    });
    v.push(PatternEntry { pat: "take no extra damage from critical strikes if you've cast (.-) in the past (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let curse = ns(caps.cap[0], "index local 'curse'");
        Some(vec![r#mod(n!("ReduceCritExtraDamage"), BASE, 100.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }), tag!({ type = Condition, var = cat!("SelfCast", gsub(&*gsub_fn(&*gsub_fn(curse, "^%l", |c| Some(upper(c[0].as_str())), None).0, " %l", |c| Some(upper(c[0].as_str())), None).0, " ", "", None).0) })])])
    }), lua_line: 5904 });
    v.push(PatternEntry {
        pat: "take no extra damage from critical strikes if you have a magic ring in left slot",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ReduceCritExtraDamage"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
                tag!({ type = Condition, var = "MagicItemInRing 1" }),
            ],
        )]),
        lua_line: 5907,
    });
    v.push(PatternEntry {
        pat:
            "take no extra damage from critical strikes if energy shield recharge started recently",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ReduceCritExtraDamage"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "EnergyShieldRechargeRecently" })],
        )]),
        lua_line: 5910,
    });
    v.push(PatternEntry { pat: "you take (%d+)%% reduced extra damage from critical strikes while affected by determination", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ReduceCritExtraDamage"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByDetermination" })])])
    }), lua_line: 5911 });
    v.push(PatternEntry {
        pat: "you take (%d+)%% reduced extra damage from critical strikes",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReduceCritExtraDamage"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5914,
    });
    v.push(PatternEntry {
        pat: "you take (%d+)%% increased extra damage from critical strikes",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReduceCritExtraDamage"),
                BASE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 5915,
    });
    v.push(PatternEntry { pat: "you take (%d+)%% reduced extra damage from critical strikes while you have no power charges", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ReduceCritExtraDamage"), BASE, num, None, 0, 0, &[tag!({ type = StatThreshold, stat = "PowerCharges", threshold = 0.0, upper = true })])])
    }), lua_line: 5916 });
    v.push(PatternEntry {
        pat: "you take (%d+)%% reduced extra damage from critical strikes by poisoned enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReduceCritExtraDamage"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })],
            )])
        }),
        lua_line: 5917,
    });
    v.push(PatternEntry {
        pat: "you take (%d+)%% reduced extra damage from critical strikes by cursed enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReduceCritExtraDamage"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })],
            )])
        }),
        lua_line: 5918,
    });
}

/// @pob Modules/ModParser.lua:5919-5942 h=e731d6b5
fn specialModList_5919(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "nearby allies have (%d+)%% chance to block attack damage per (%d+) strength you have",
        val: SpecialVal::Fn(|caps: &Caps| {
            let block = caps.num;
            let str = ns(caps.cap[1], "index local 'str'");
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![
                        (n!("onlyAllies"), Value::from(true)),
                        (
                            n!("mod"),
                            Value::from(r#mod(n!("BlockChance"), BASE, block, None, 0, 0, &[])),
                        ),
                    ],
                ),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "Str", div = tonumber(str) })],
            )])
        }),
        lua_line: 5919,
    });
    v.push(PatternEntry {
        pat: "physical skills have (%d+)%% increased duration per (%d+) intelligence",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num1 = caps.num;
            let num2 = ns(caps.cap[1], "index local 'num2'");
            Some(vec![r#mod(
                n!("Duration"),
                INC,
                num1,
                None,
                0,
                KeywordFlag::Physical,
                &[tag!({ type = PerStat, stat = "Int", div = tonumber(num2) })],
            )])
        }),
        lua_line: 5922,
    });
    v.push(PatternEntry {
        pat: "y?o?u?r? ?maximum energy shield is equal to (%d+)%% of y?o?u?r? ?maximum life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShield"),
                OVERRIDE,
                1.0,
                None,
                0,
                0,
                &[tag!({ type = PercentStat, stat = "Life", percent = num })],
            )])
        }),
        lua_line: 5925,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments while bleeding",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Bleeding" })],
        )]),
        lua_line: 5928,
    });
    v.push(PatternEntry {
        pat: "mana is increased by (%d+)%% of overcapped lightning resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("ManaIncreasedByOvercappedLightningRes"), None, 0, 0, &[]),
                r#mod(
                    n!("Mana"),
                    INC,
                    (nn(num, "perform arithmetic on local 'num'") / 100.0),
                    None,
                    0,
                    0,
                    &[tag!({ type = PerStat, stat = "LightningResistOverCap" })],
                ),
            ])
        }),
        lua_line: 5929,
    });
    // handled in item parsing
    v.push(PatternEntry {
        pat: "%d+%% [ir][ne][cd][ru][ec][ae][sd]e?d? ?[%a%s]* modifier magnitudes",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5931,
    });
    v.push(PatternEntry {
        pat: "%d+%% [ir][ne][cd][ru][ec][ae][sd]e?d? effect of [sp][ur][fe]fixes",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5932,
    });
    v.push(PatternEntry {
        pat: "[%a%s]* modifier magnitudes are doubled",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5933,
    });
    // handled in items tab corrupt menu
    // currently this is not correct, but the
    // vestigial implicits in-game only contain this line. it is probably safe
    // to assume that GGG will add a line to the vestigial mod which will
    // instead say how many implicits the item can have. in that case, the two
    // below modifiers should be swapped.
    v.push(PatternEntry {
        pat: "can be modified while corrupted",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CorruptImplicitCount"),
            BASE,
            5.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 5940,
    });
    v.push(PatternEntry {
        pat: "can have up to 5 implicit modifiers while item has this modifier",
        val: SpecialVal::Mods(vec![]),
        lua_line: 5941,
    });
}

/// Pushes the specialModList entries of Modules/ModParser.lua:4989-5942 (560 entries) in source order.
fn push_entries(v: &mut Vec<PatternEntry<SpecialVal>>) {
    specialModList_4989(v);
    specialModList_5071(v);
    specialModList_5161(v);
    specialModList_5244(v);
    specialModList_5301(v);
    specialModList_5404(v);
    specialModList_5512(v);
    specialModList_5612(v);
    specialModList_5703(v);
    specialModList_5778(v);
    specialModList_5830(v);
    specialModList_5919(v);
}

/// The entries of ModParser.lua 4989-5942, in source order.
pub fn entries() -> Vec<PatternEntry<SpecialVal>> {
    let mut v = Vec::new();
    push_entries(&mut v);
    v
}
