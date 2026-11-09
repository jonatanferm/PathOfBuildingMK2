//! @pob-file Modules/ModParser.lua @16de4b82d
//! specialModList entries, ModParser.lua 2130-3083 (calc doc §8): explode mods, keystones,
//! ascendancy notables (Ascendant … Saboteur/Trickster) and the first block of unique mods.
//! `special_a`..`special_d` concatenate, in source order, into `specialModList`
//! (`generated_lists.rs`, MP0, wraps every key in `^…$`).
//!
//! Owner: MP3. The entries were translated by scripts/port/lua2rs (unit T1) and reviewed
//! against the Lua; the six closures lua2rs could not translate (2302, 2380, 2621, 2629, 2898,
//! 3005) are hand-ported (marked "hand-ported"). One `PatternEntry` per Lua key, in source
//! order (the range has no duplicate keys, and none of its keys is redefined later in the
//! constructor), with one `@pob` marker per block of 50 entries.
//! Closure parameters follow the specialModList call shape (ModParser.lua:6662-6663; calc doc
//! §6 / CCR-0006 §5): `f(tonumber(cap[1]), unpack(cap))`, so #1 = `caps.num`, #k≥2 =
//! `caps.cap[k-2]`.
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
    data, tag, AStr, JewelFuncKind, JewelFuncSpec, KeywordFlag, Mod, ModFlag, ModType, ModType::*,
    Ramp, SkillType, Sockets, Tag, Value, ValueTable,
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

/// Lua `tonumber(v)` of a string-or-nil capture: `tonumber(nil)` raises.
fn lua_tonumber(v: Option<&str>) -> Option<f64> {
    match v {
        Some(s) => tonumber(s),
        None => lua_error!("bad argument #1 to 'tonumber' (value expected)"),
    }
}

/// The `{ radius = …, type = "Other", func = function(node, out, data) … end }` value of an
/// `ExtraJewelFunc` mod whose closure is defined inline (2622-2627, 2630-2634). The closure is
/// data (calc doc §6, `Value::JewelFunc`): `key` is its `"<file>:<a>-<b>"` (as the parse oracle
/// records it), `args` the pattern captures it closes over, as strings in capture order (the
/// upvalues are a subset of them: `dmgType` = args[1]; `val`, `attr` = args[1], args[2]).
/// Rust-only plumbing.
fn extra_jewel_func(key: &str, caps: &Caps, radius: String) -> Value {
    let mut args: smallvec::SmallVec<[Value; 4]> = smallvec::SmallVec::new();
    for c in caps.cap.iter() {
        match c {
            Some(s) => args.push(Value::from(*s)),
            None => break,
        }
    }
    Value::JewelFunc(Arc::new(JewelFuncSpec {
        key: Name::from(key),
        kind: JewelFuncKind::Other,
        args,
        r#type: Some(n!("Other")),
        radius: Some(Name::from(radius.as_str())),
    }))
}

/// @pob Modules/ModParser.lua:2130-2240 h=c2baa617
fn specialModList_2132(v: &mut Vec<PatternEntry<SpecialVal>>) {
    // Explode mods
    // Obliteration, Unspeakable Gifts (chaos cluster), synth implicit mod, current crusader body mod, Ngamahu Warmonger tattoo
    v.push(PatternEntry { pat: "enemies you kill have a (%d+)%% chance to explode, dealing a (.+) of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[])
    }), lua_line: 2132 });
    // Hinekora, Death's Fury 3.22
    v.push(PatternEntry { pat: "enemies you kill have ?a? ?(%d+)%% chance to explode, dealing (%d+)%% of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[])
    }), lua_line: 2135 });
    // Hinekora, Death's Fury 3.23
    v.push(PatternEntry { pat: "enemies you or your totems kill have (%d+)%% chance to explode, dealing (%d+)%% of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[])
    }), lua_line: 2138 });
    // Sublime Vision
    v.push(PatternEntry { pat: "enemies you kill while using pride have (%d+)%% chance to explode, dealing a (.+) of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[tag!({ type = Condition, var = "AffectedByPride" })])
    }), lua_line: 2141 });
    // Oriath's End
    v.push(PatternEntry { pat: "enemies you kill during effect have a (%d+)%% chance to explode, dealing a (.+) of their maximum life as damage of a random element", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        explodeFunc(chance, LuaArg::Str(amount), "randomElement", &[tag!({ type = Condition, var = "UsingFlask" })])
    }), lua_line: 2144 });
    // Beacon of Madness
    v.push(PatternEntry { pat: "enemies you kill while affected by glorious madness have a (%d+)%% chance to explode, dealing a (.+) of their life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })])
    }), lua_line: 2147 });
    // Devastator (attack clusters)
    v.push(PatternEntry { pat: "enemies killed with attack hits have a (%d+)%% chance to explode, dealing a (.+) of their life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[])
    }), lua_line: 2150 });
    // Explosive Force (wand clusters)
    v.push(PatternEntry { pat: "enemies killed with wand hits have a (%d+)%% chance to explode, dealing a (.+) of their life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[tag!({ type = Condition, var = "UsingWand" })])
    }), lua_line: 2153 });
    // Profane Bloom
    v.push(PatternEntry { pat: "cursed enemies you or your minions kill have a (%d+)%% chance to explode, dealing a (.+) of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let chance = caps.num;
        let amount = ns(caps.cap[1], "index local 'amount'");
        let r#type = ns(caps.cap[2], "index local 'type'");
        explodeFunc(chance, LuaArg::Str(amount), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })])
    }), lua_line: 2156 });
    // legacy synth, legacy crusader
    v.push(PatternEntry {
        pat: "enemies you kill explode, dealing (%d+)%% of their life as (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let amount = caps.num;
            let r#type = ns(caps.cap[1], "index local 'type'");
            explodeFunc(
                Some(100.0),
                amount.map_or(LuaArg::Nil, LuaArg::Num),
                r#type,
                &[],
            )
        }),
        lua_line: 2159,
    });
    // Quecholli
    v.push(PatternEntry {
        pat: "enemies killed explode dealing (%d+)%% of their life as (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let amount = caps.num;
            let r#type = ns(caps.cap[1], "index local 'type'");
            explodeFunc(
                Some(100.0),
                amount.map_or(LuaArg::Nil, LuaArg::Num),
                r#type,
                &[],
            )
        }),
        lua_line: 2162,
    });
    // Sporeguard
    v.push(PatternEntry { pat: "enemies on fungal ground you kill explode, dealing (%d+)%% of their life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "OnFungalGround" })])
    }), lua_line: 2165 });
    // Shaper 2H mace mod
    v.push(PatternEntry { pat: "enemies killed with attack or spell hits explode, dealing (%d+)%% of their life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[])
    }), lua_line: 2168 });
    // Inpulsa's Broken Heart
    v.push(PatternEntry { pat: "shocked enemies you kill explode, dealing (%d+)%% of their life as (.+) damage which cannot shock", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })])
    }), lua_line: 2171 });
    // Inpulsa's Broken Heart
    v.push(PatternEntry { pat: "ignited enemies you kill explode, dealing (%d+)%% of their life as (.+) damage which cannot ignite", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })])
    }), lua_line: 2174 });
    // Haemophilia
    v.push(PatternEntry { pat: "bleeding enemies you kill explode, dealing (%d+)%% of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })])
    }), lua_line: 2177 });
    // Haemophilia
    v.push(PatternEntry { pat: "burning enemies you kill have a (%d+)%% chance to explode, dealing a (.+) of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "Burning" })])
    }), lua_line: 2180 });
    v.push(PatternEntry {
        pat: "non-aura curses you inflict are not removed from dying enemies",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2183,
    });
    v.push(PatternEntry {
        pat: "enemies near corpses affected by your curses are blinded",
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
            &[
                tag!({ type = MultiplierThreshold, var = "NearbyCorpse", threshold = 1.0 }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" }),
            ],
        )]),
        lua_line: 2184,
    });
    // Asenath's Gentle Touch
    v.push(PatternEntry { pat: "enemies killed near corpses affected by your curses explode, dealing (%d+)%% of their life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = MultiplierThreshold, var = "NearbyCorpse", threshold = 1.0 }), tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })])
    }), lua_line: 2185 });
    // Al Dhih
    v.push(PatternEntry { pat: "enemies taunted by your warcries explode on death, dealing (%d+)%% of their maximum life as (.+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let amount = caps.num;
        let r#type = ns(caps.cap[1], "index local 'type'");
        explodeFunc(Some(100.0), amount.map_or(LuaArg::Nil, LuaArg::Num), r#type, &[tag!({ type = ActorCondition, actor = "enemy", var = "Taunted" }), tag!({ type = Condition, var = "UsedWarcryRecently" })])
    }), lua_line: 2188 });
    // Crucible weapon mod
    v.push(PatternEntry {
        pat: "totems explode on death, dealing (%d+)%% of their life as (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let amount = caps.num;
            let r#type = ns(caps.cap[1], "index local 'type'");
            explodeFunc(
                Some(100.0),
                amount.map_or(LuaArg::Nil, LuaArg::Num),
                r#type,
                &[],
            )
        }),
        lua_line: 2191,
    });
    // Ruthless Berserker node
    v.push(PatternEntry {
        pat: "nearby corpses explode when you warcry, dealing (%d+)%% of their life as (.+) damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let amount = caps.num;
            let r#type = ns(caps.cap[1], "index local 'type'");
            explodeFunc(
                Some(100.0),
                amount.map_or(LuaArg::Nil, LuaArg::Num),
                r#type,
                &[],
            )
        }),
        lua_line: 2194,
    });
    // Keystones
    v.push(PatternEntry {
        pat: "(%d+) rage regenerated for every (%d+) mana regeneration per second",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![
                r#mod(
                    n!("RageRegen"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = PerStat, stat = "ManaRegen", div = tonumber(div) })],
                ),
                flag(n!("Condition:CanGainRage"), None, 0, 0, &[]),
            ])
        }),
        lua_line: 2198,
    });
    v.push(PatternEntry {
        pat: "(%w+) recovery from regeneration is not applied",
        val: SpecialVal::Fn(|caps: &Caps| {
            let r#type = ns(caps.cap[0], "index local 'type'");
            Some(vec![flag(
                cat!("UnaffectedBy", firstToUpper(r#type), "Regen"),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2202,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% less damage taken for every (%d+)%% life recovery per second from leech",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let div = ns(caps.cap[1], "index local 'div'");
            Some(vec![r#mod(
                n!("DamageTaken"),
                MORE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "MaxLifeLeechRatePercent", div = tonumber(div) })],
            )])
        }),
        lua_line: 2203,
    });
    v.push(PatternEntry {
        pat: "(%w+) recovery from non%-instant leech is not applied",
        val: SpecialVal::Fn(|caps: &Caps| {
            let r#type = ns(caps.cap[0], "index local 'type'");
            Some(vec![flag(
                cat!("UnaffectedByNonInstant", firstToUpper(r#type), "Leech"),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2206,
    });
    v.push(PatternEntry { pat: "(%d+)%% additional physical damage reduction for every (%d+)%% life recovery per second from leech", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        Some(vec![r#mod(n!("PhysicalDamageReduction"), BASE, num, None, 0, 0, &[tag!({ type = PerStat, stat = "MaxLifeLeechRatePercent", div = tonumber(div) }), tag!({ type = Condition, var = "Leeching" })])])
    }), lua_line: 2207 });
    v.push(PatternEntry { pat: "modifiers to chance to suppress spell damage instead apply to chance to dodge spell hits at 50%% of their value", val: SpecialVal::Mods(vec![flag(n!("ConvertSpellSuppressionToSpellDodge"), None, 0, 0, &[]), r#mod(n!("SpellSuppressionChance"), OVERRIDE, 0.0, Some("Acrobatics"), 0, 0, &[])]), lua_line: 2210 });
    v.push(PatternEntry { pat: "chance to evade hits is based off of (%d+)%% of your ward instead of your evasion rating", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("EvadeChanceBasedOnWard"), None, 0, 0, &[]), r#mod(n!("EvadeChanceBasedOnWardPercent"), OVERRIDE, num, Some("Black Scythe Training"), 0, 0, &[])])
    }), lua_line: 2214 });
    v.push(PatternEntry { pat: "physical damage reduction from hits is based off of (%d+)%% of your ward instead of your armour", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("PhysicalReductionBasedOnWard"), None, 0, 0, &[]), r#mod(n!("PhysicalReductionBasedOnWardPercent"), OVERRIDE, num, Some("Black Scythe Training"), 0, 0, &[])])
    }), lua_line: 2218 });
    v.push(PatternEntry {
        pat: "maximum chance to dodge spell hits is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SpellDodgeChanceMax"),
                OVERRIDE,
                num,
                Some("Acrobatics"),
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2222,
    });
    v.push(PatternEntry {
        pat: "dexterity provides no bonus to evasion rating",
        val: SpecialVal::Mods(vec![flag(n!("NoDexBonusToEvasion"), None, 0, 0, &[])]),
        lua_line: 2223,
    });
    v.push(PatternEntry {
        pat: "dexterity provides no inherent bonus to evasion rating",
        val: SpecialVal::Mods(vec![flag(n!("NoDexBonusToEvasion"), None, 0, 0, &[])]),
        lua_line: 2224,
    });
    v.push(PatternEntry {
        pat: "strength's damage bonus applies to all spell damage as well",
        val: SpecialVal::Mods(vec![flag(n!("IronWill"), None, 0, 0, &[])]),
        lua_line: 2225,
    });
    v.push(PatternEntry {
        pat: "your hits can't be evaded",
        val: SpecialVal::Mods(vec![flag(n!("CannotBeEvaded"), None, 0, 0, &[])]),
        lua_line: 2226,
    });
    v.push(PatternEntry {
        pat: "your melee hits can't be evaded while wielding a sword",
        val: SpecialVal::Mods(vec![flag(
            n!("CannotBeEvaded"),
            None,
            bor(ModFlag::Melee, ModFlag::Hit),
            0,
            &[tag!({ type = Condition, var = "UsingSword" })],
        )]),
        lua_line: 2227,
    });
    v.push(PatternEntry {
        pat: "minion hits can't be evaded",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinionModifier"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("CannotBeEvaded"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2228,
    });
    v.push(PatternEntry {
        pat: "never deal critical strikes",
        val: SpecialVal::Mods(vec![
            flag(n!("NeverCrit"), None, 0, 0, &[]),
            flag(n!("Condition:NeverCrit"), None, 0, 0, &[]),
        ]),
        lua_line: 2229,
    });
    v.push(PatternEntry {
        pat: "minions never deal critical strikes",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("MinionModifier"),
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
                n!("MinionModifier"),
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
        lua_line: 2230,
    });
    v.push(PatternEntry {
        pat: "never deal critical strikes with spells",
        val: SpecialVal::Mods(vec![
            flag(n!("NeverCrit"), None, ModFlag::Spell, 0, &[]),
            flag(n!("Condition:NeverCrit"), None, ModFlag::Spell, 0, &[]),
        ]),
        lua_line: 2231,
    });
    v.push(PatternEntry {
        pat: "never deal critical strikes with attacks",
        val: SpecialVal::Mods(vec![
            flag(n!("NeverCrit"), None, ModFlag::Attack, 0, &[]),
            flag(n!("Condition:NeverCrit"), None, ModFlag::Attack, 0, &[]),
        ]),
        lua_line: 2232,
    });
    v.push(PatternEntry {
        pat: "cannot deal critical strikes",
        val: SpecialVal::Mods(vec![
            flag(n!("NeverCrit"), None, 0, 0, &[]),
            flag(n!("Condition:NeverCrit"), None, 0, 0, &[]),
        ]),
        lua_line: 2233,
    });
    v.push(PatternEntry {
        pat: "cannot deal critical strikes with spells",
        val: SpecialVal::Mods(vec![
            flag(n!("NeverCrit"), None, ModFlag::Spell, 0, &[]),
            flag(n!("Condition:NeverCrit"), None, ModFlag::Spell, 0, &[]),
        ]),
        lua_line: 2234,
    });
    v.push(PatternEntry {
        pat: "cannot deal critical strikes with attacks",
        val: SpecialVal::Mods(vec![
            flag(n!("NeverCrit"), None, ModFlag::Attack, 0, &[]),
            flag(n!("Condition:NeverCrit"), None, ModFlag::Attack, 0, &[]),
        ]),
        lua_line: 2235,
    });
    v.push(PatternEntry {
        pat: "no critical strike multiplier",
        val: SpecialVal::Mods(vec![flag(n!("NoCritMultiplier"), None, 0, 0, &[])]),
        lua_line: 2236,
    });
    v.push(PatternEntry {
        pat: "ailments never count as being from critical strikes",
        val: SpecialVal::Mods(vec![flag(n!("AilmentsAreNeverFromCrit"), None, 0, 0, &[])]),
        lua_line: 2237,
    });
    v.push(PatternEntry { pat: "the increase to physical damage from strength applies to projectile attacks as well as melee attacks", val: SpecialVal::Mods(vec![flag(n!("IronGrip"), None, 0, 0, &[])]), lua_line: 2238 });
    v.push(PatternEntry {
        pat: "strength%'s damage bonus applies to projectile attack damage as well as melee damage",
        val: SpecialVal::Mods(vec![flag(n!("IronGrip"), None, 0, 0, &[])]),
        lua_line: 2239,
    });
    v.push(PatternEntry {
        pat:
            "converts all evasion rating to armour%. dexterity provides no bonus to evasion rating",
        val: SpecialVal::Mods(vec![
            flag(n!("NoDexBonusToEvasion"), None, 0, 0, &[]),
            flag(n!("IronReflexes"), None, 0, 0, &[]),
        ]),
        lua_line: 2240,
    });
}

/// @pob Modules/ModParser.lua:2241-2343 h=309c7e58
fn specialModList_2241(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "30%% chance to dodge attack hits%. 50%% less armour, 30%% less energy shield, 30%% less chance to block spell and attack damage", val: SpecialVal::Mods(vec![r#mod(n!("AttackDodgeChance"), BASE, 30.0, None, 0, 0, &[]), r#mod(n!("Armour"), MORE, -50.0, None, 0, 0, &[]), r#mod(n!("EnergyShield"), MORE, -30.0, None, 0, 0, &[]), r#mod(n!("BlockChance"), MORE, -30.0, None, 0, 0, &[]), r#mod(n!("SpellBlockChance"), MORE, -30.0, None, 0, 0, &[])]), lua_line: 2241 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased blind effect",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("BlindEffect"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2248,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased effect of blind from melee weapons",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("BlindEffect"), INC, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2249,
    });
    v.push(PatternEntry { pat: "%+(%d+)%% chance to block spell damage for each (%d+)%% overcapped chance to block attack damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        Some(vec![r#mod(n!("SpellBlockChance"), BASE, num, None, 0, 0, &[tag!({ type = PerStat, stat = "BlockChanceOverCap", div = tonumber(div) })])])
    }), lua_line: 2250 });
    v.push(PatternEntry {
        pat: "maximum life becomes 1, immune to chaos damage",
        val: SpecialVal::Mods(vec![
            flag(n!("ChaosInoculation"), None, 0, 0, &[]),
            r#mod(n!("ChaosDamageTaken"), MORE, -100.0, None, 0, 0, &[]),
        ]),
        lua_line: 2251,
    });
    v.push(PatternEntry {
        pat: "life regeneration is applied to energy shield instead",
        val: SpecialVal::Mods(vec![flag(n!("ZealotsOath"), None, 0, 0, &[])]),
        lua_line: 2255,
    });
    v.push(PatternEntry {
        pat: "life leeched per second is doubled",
        val: SpecialVal::Mods(vec![r#mod(
            n!("LifeLeechRate"),
            MORE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2256,
    });
    v.push(PatternEntry {
        pat: "life regeneration has no effect",
        val: SpecialVal::Mods(vec![flag(n!("NoLifeRegen"), None, 0, 0, &[])]),
        lua_line: 2257,
    });
    v.push(PatternEntry {
        pat: "energy shield recharge instead applies to life",
        val: SpecialVal::Mods(vec![flag(
            n!("EnergyShieldRechargeAppliesToLife"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2258,
    });
    v.push(PatternEntry { pat: "blade vortex and blade blast deal no non%-physical damage", val: SpecialVal::Mods(vec![flag(n!("DealNoLightning"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blade Vortex"), n!("Blade Blast")][..], includeTransfigured = true })]), flag(n!("DealNoCold"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blade Vortex"), n!("Blade Blast")][..], includeTransfigured = true })]), flag(n!("DealNoFire"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blade Vortex"), n!("Blade Blast")][..], includeTransfigured = true })]), flag(n!("DealNoChaos"), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blade Vortex"), n!("Blade Blast")][..], includeTransfigured = true })])]), lua_line: 2259 });
    v.push(PatternEntry {
        pat: "(%d+)%% of physical, cold and lightning damage converted to fire damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("PhysicalDamageConvertToFire"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(
                    n!("LightningDamageConvertToFire"),
                    BASE,
                    num,
                    None,
                    0,
                    0,
                    &[],
                ),
                r#mod(n!("ColdDamageConvertToFire"), BASE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 2264,
    });
    v.push(PatternEntry {
        pat: "all elemental damage converted to chaos damage",
        val: SpecialVal::Mods(vec![
            r#mod(n!("ColdDamageConvertToChaos"), BASE, 100.0, None, 0, 0, &[]),
            r#mod(n!("FireDamageConvertToChaos"), BASE, 100.0, None, 0, 0, &[]),
            r#mod(
                n!("LightningDamageConvertToChaos"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 2269,
    });
    v.push(PatternEntry {
        pat: "removes all mana%. spend life instead of mana for skills",
        val: SpecialVal::Mods(vec![
            r#mod(n!("Mana"), MORE, -100.0, None, 0, 0, &[]),
            flag(n!("CostLifeInsteadOfMana"), None, 0, 0, &[]),
        ]),
        lua_line: 2274,
    });
    v.push(PatternEntry {
        pat: "removes all mana",
        val: SpecialVal::Mods(vec![r#mod(n!("Mana"), MORE, -100.0, None, 0, 0, &[])]),
        lua_line: 2275,
    });
    v.push(PatternEntry {
        pat: "removes all energy shield",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnergyShield"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2276,
    });
    v.push(PatternEntry {
        pat: "skills cost life instead of mana",
        val: SpecialVal::Mods(vec![flag(n!("CostLifeInsteadOfMana"), None, 0, 0, &[])]),
        lua_line: 2277,
    });
    v.push(PatternEntry {
        pat: "skills reserve life instead of mana",
        val: SpecialVal::Mods(vec![flag(n!("BloodMagicReserved"), None, 0, 0, &[])]),
        lua_line: 2278,
    });
    v.push(PatternEntry {
        pat: "your skills that throw mines reserve life instead of mana",
        val: SpecialVal::Mods(vec![flag(
            n!("BloodMagicReserved"),
            None,
            0,
            KeywordFlag::Mine,
            &[],
        )]),
        lua_line: 2279,
    });
    v.push(PatternEntry {
        pat: "curse skills cost life instead of mana",
        val: SpecialVal::Mods(vec![flag(
            n!("CostLifeInsteadOfMana"),
            None,
            0,
            KeywordFlag::Curse,
            &[],
        )]),
        lua_line: 2280,
    });
    v.push(PatternEntry {
        pat: "curse aura skills reserve life instead of mana",
        val: SpecialVal::Mods(vec![flag(
            n!("BloodMagicReserved"),
            None,
            0,
            KeywordFlag::Curse,
            &[tag!({ type = SkillType, skillType = SkillType::Aura })],
        )]),
        lua_line: 2281,
    });
    v.push(PatternEntry {
        pat: "your travel skills critically strike once every 3 uses",
        val: SpecialVal::Mods(vec![flag(
            n!("Every3UseCrit"),
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Travel })],
        )]),
        lua_line: 2282,
    });
    v.push(PatternEntry {
        pat: "non%-aura skills cost no mana or life while focus?sed",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("ManaCost"),
                MORE,
                -100.0,
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "Focused" }),
                    tag!({ type = SkillType, skillType = SkillType::Aura, neg = true }),
                ],
            ),
            r#mod(
                n!("LifeCost"),
                MORE,
                -100.0,
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "Focused" }),
                    tag!({ type = SkillType, skillType = SkillType::Aura, neg = true }),
                ],
            ),
        ]),
        lua_line: 2283,
    });
    v.push(PatternEntry {
        pat: "spend life instead of mana for effects of skills",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2287,
    });
    v.push(PatternEntry {
        pat: "skills cost %+(%d+) rage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("RageCostBase"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2288,
    });
    v.push(PatternEntry {
        pat: "warcries cost %+(%d+)%% of life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeCostBase"),
                BASE,
                1.0,
                None,
                0,
                KeywordFlag::Warcry,
                &[tag!({ type = PercentStat, stat = "Life", percent = num, floor = true })],
            )])
        }),
        lua_line: 2289,
    });
    v.push(PatternEntry {
        pat: "vaal skills used during effect have (%d+)%% reduced soul gain prevention duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SoulGainPreventionDuration"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = SkillType, skillType = SkillType::Vaal }),
                ],
            )])
        }),
        lua_line: 2290,
    });
    v.push(PatternEntry { pat: "vaal volcanic fissure and vaal molten strike have (%d+)%% reduced soul gain prevention duration", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("SoulGainPreventionDuration"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Volcanic Fissure"), n!("Molten Strike")][..], includeTransfigured = true }), tag!({ type = SkillType, skillType = SkillType::Vaal })])])
    }), lua_line: 2291 });
    v.push(PatternEntry {
        pat: "vaal skills can store %+(%d+) uses?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AdditionalUses"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Vaal })],
            )])
        }),
        lua_line: 2292,
    });
    v.push(PatternEntry {
        pat: "vaal attack skills cost rage instead of requiring souls to use",
        val: SpecialVal::Mods(vec![flag(
            n!("CostRageInsteadOfSouls"),
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Vaal })],
        )]),
        lua_line: 2293,
    });
    v.push(PatternEntry {
        pat: "vaal attack skills you use yourself cost rage instead of requiring souls",
        val: SpecialVal::Mods(vec![flag(
            n!("CostRageInsteadOfSouls"),
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Vaal })],
        )]),
        lua_line: 2294,
    });
    v.push(PatternEntry {
        pat: "you cannot gain rage during soul gain prevention",
        val: SpecialVal::Mods(vec![r#mod(
            n!("RageRegen"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "SoulGainPrevention" })],
        )]),
        lua_line: 2295,
    });
    v.push(PatternEntry { pat: "hits that deal elemental damage remove exposure to those elements and inflict exposure to other elements exposure inflicted this way applies (%-%d+)%% to resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![flag(n!("ElementalEquilibrium"), None, 0, 0, &[]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireExposure"), BASE, num, None, 0, 0, &[tag!({ type = Condition, varList = &[n!("HitByColdDamage"), n!("HitByLightningDamage")][..] })])))]), None, 0, 0, &[]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ColdExposure"), BASE, num, None, 0, 0, &[tag!({ type = Condition, varList = &[n!("HitByFireDamage"), n!("HitByLightningDamage")][..] })])))]), None, 0, 0, &[]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LightningExposure"), BASE, num, None, 0, 0, &[tag!({ type = Condition, varList = &[n!("HitByFireDamage"), n!("HitByColdDamage")][..] })])))]), None, 0, 0, &[])])
    }), lua_line: 2296 });
    // hand-ported (lua2rs: assign-type): `plus` is `tonumber(cap[1])` ("+10" → 10), `minus` the
    // string capture, reassigned to `tonumber(minus)` before use
    v.push(PatternEntry { pat: "enemies you hit with elemental damage temporarily get (%+%d+)%% resistance to those elements and (%-%d+)%% resistance to other elements", val: SpecialVal::Fn(|caps: &Caps| {
        let plus = caps.num;
        // L2303
        let minus = lua_tonumber(caps.cap[1]);
        // L2304
        Some(vec![
            flag(n!("ElementalEquilibrium"), None, 0, 0, &[]),
            flag(n!("ElementalEquilibriumLegacy"), None, 0, 0, &[]),
            r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireResist"), BASE, plus, None, 0, 0, &[tag!({ type = Condition, var = "HitByFireDamage" })])))]), None, 0, 0, &[]),
            r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireResist"), BASE, minus, None, 0, 0, &[tag!({ type = Condition, var = "HitByFireDamage", neg = true }), tag!({ type = Condition, varList = &[n!("HitByColdDamage"), n!("HitByLightningDamage")][..] })])))]), None, 0, 0, &[]),
            r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ColdResist"), BASE, plus, None, 0, 0, &[tag!({ type = Condition, var = "HitByColdDamage" })])))]), None, 0, 0, &[]),
            r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ColdResist"), BASE, minus, None, 0, 0, &[tag!({ type = Condition, var = "HitByColdDamage", neg = true }), tag!({ type = Condition, varList = &[n!("HitByFireDamage"), n!("HitByLightningDamage")][..] })])))]), None, 0, 0, &[]),
            r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LightningResist"), BASE, plus, None, 0, 0, &[tag!({ type = Condition, var = "HitByLightningDamage" })])))]), None, 0, 0, &[]),
            r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LightningResist"), BASE, minus, None, 0, 0, &[tag!({ type = Condition, var = "HitByLightningDamage", neg = true }), tag!({ type = Condition, varList = &[n!("HitByFireDamage"), n!("HitByColdDamage")][..] })])))]), None, 0, 0, &[]),
        ])
    }), lua_line: 2302 });
    v.push(PatternEntry { pat: "projectile attack hits deal up to 30%% more damage to targets at the start of their movement, dealing less damage to targets as the projectile travels farther", val: SpecialVal::Mods(vec![flag(n!("PointBlank"), None, 0, 0, &[])]), lua_line: 2315 });
    v.push(PatternEntry {
        pat: "leech energy shield instead of life",
        val: SpecialVal::Mods(vec![flag(n!("GhostReaver"), None, 0, 0, &[])]),
        lua_line: 2316,
    });
    v.push(PatternEntry { pat: "minions explode when reduced to low life, dealing 33%% of their maximum life as fire damage to surrounding enemies", val: SpecialVal::Mods(vec![r#mod(n!("ExtraMinionSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("MinionInstability"))]), None, 0, 0, &[])]), lua_line: 2317 });
    v.push(PatternEntry { pat: "minions explode when reduced to low life, dealing 33%% of their life as fire damage to surrounding enemies", val: SpecialVal::Mods(vec![r#mod(n!("ExtraMinionSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("MinionInstability"))]), None, 0, 0, &[])]), lua_line: 2318 });
    // The node itself is detected by the code that handles it
    v.push(PatternEntry {
        pat: "all bonuses from an equipped shield apply to your minions instead of you",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2319,
    });
    v.push(PatternEntry {
        pat: "spend energy shield before mana for skill m?a?n?a? ?costs",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2320,
    });
    v.push(PatternEntry {
        pat: "you have perfect agony if you've dealt a critical strike recently",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Keystone"),
            LIST,
            "Perfect Agony",
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CritRecently" })],
        )]),
        lua_line: 2321,
    });
    v.push(PatternEntry {
        pat: "energy shield protects mana instead of life",
        val: SpecialVal::Mods(vec![flag(n!("EnergyShieldProtectsMana"), None, 0, 0, &[])]),
        lua_line: 2322,
    });
    v.push(PatternEntry { pat: "modifiers to critical strike multiplier also apply to damage over time multiplier for ailments from critical strikes at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritMultiplierAppliesToDegen"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 2323 });
    v.push(PatternEntry {
        pat: "damage over time multiplier for ailments is equal to critical strike multiplier",
        val: SpecialVal::Mods(vec![flag(
            n!("DotMultiplierIsCritMultiplier"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2324,
    });
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber (known diff MP3-1, calc doc §6)
    v.push(PatternEntry { pat: "%+(%d+)%% to cold damage over time multiplier for each (%d+)%% overcapped cold resistance", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let val = ns(caps.cap[1], "index local 'val'");
        Some(vec![r#mod(n!("ColdDotMultiplier"), BASE, num, None, 0, 0, &[tag!({ type = PerStat, stat = "ColdResistOverCap", div = snum(val, "perform arithmetic on tag.div") })])])
    }), lua_line: 2325 });
    v.push(PatternEntry {
        pat: "your bleeding does not deal extra damage while the enemy is moving",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:NoExtraBleedDamageToMovingEnemy"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2326,
    });
    v.push(PatternEntry { pat: "your bleeding does not deal extra damage while the enemy is moving and cannot be aggravated", val: SpecialVal::Mods(vec![flag(n!("Condition:NoExtraBleedDamageToMovingEnemy"), None, 0, 0, &[]), flag(n!("Condition:CannotAggravate"), None, 0, 0, &[])]), lua_line: 2327 });
    v.push(PatternEntry { pat: "you and enemies in your presence count as moving while affected by elemental ailments", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("Condition:Moving"), None, 0, 0, &[tag!({ type = Condition, varList = &[n!("Frozen"), n!("Chilled"), n!("Shocked"), n!("Ignited"), n!("Scorched"), n!("Brittle"), n!("Sapped")][..] })])))]), None, 0, 0, &[]), flag(n!("Condition:Moving"), None, 0, 0, &[tag!({ type = Condition, varList = &[n!("Frozen"), n!("Chilled"), n!("Shocked"), n!("Ignited"), n!("Scorched"), n!("Brittle"), n!("Sapped")][..] })])]), lua_line: 2328 });
    v.push(PatternEntry {
        pat: "you can inflict bleeding on an enemy up to (%d+) times?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(n!("BleedStacksMax"), OVERRIDE, num, None, 0, 0, &[]),
                flag(n!("Condition:HaveCrimsonDance"), None, 0, 0, &[]),
            ])
        }),
        lua_line: 2332,
    });
    v.push(PatternEntry { pat: "your minions spread caustic ground on death, dealing (%d+)%% of their maximum life as chaos damage per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraMinionSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("SiegebreakerCausticGround"))]), None, 0, 0, &[]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Multiplier:SiegebreakerCausticGroundPercent"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[])])
    }), lua_line: 2336 });
    v.push(PatternEntry { pat: "your minions spread burning ground on death, dealing (%d+)%% of their maximum life as fire damage per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraMinionSkill"), LIST, vtab(vec![], vec![(n!("skillId"), Value::from("ReplicaSiegebreakerBurningGround"))]), None, 0, 0, &[]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Multiplier:SiegebreakerBurningGroundPercent"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[])])
    }), lua_line: 2340 });
}

/// @pob Modules/ModParser.lua:2344-2453 h=3f68f87a
fn specialModList_2344(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "you can have an additional brand attached to an enemy",
        val: SpecialVal::Mods(vec![r#mod(
            n!("BrandsAttachedLimit"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2344,
    });
    v.push(PatternEntry { pat: "gain (%d+) grasping vines each second while stationary", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Multiplier:GraspingVinesCount"), BASE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "StationarySeconds", limit = 10.0, limitTotal = true }), tag!({ type = Condition, var = "Stationary" })])])
    }), lua_line: 2345 });
    v.push(PatternEntry { pat: "all damage inflicts poison against enemies affected by at least (%d+) grasping vines", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PoisonChance"), BASE, 100.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "GraspingVinesAffectingEnemy", threshold = num })]), flag(n!("FireCanPoison"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "GraspingVinesAffectingEnemy", threshold = num })]), flag(n!("ColdCanPoison"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "GraspingVinesAffectingEnemy", threshold = num })]), flag(n!("LightningCanPoison"), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "GraspingVinesAffectingEnemy", threshold = num })])])
    }), lua_line: 2348 });
    v.push(PatternEntry {
        pat: "attack projectiles always inflict bleeding and maim, and knock back enemies",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("BleedChance"),
                BASE,
                100.0,
                None,
                bor(ModFlag::Attack, ModFlag::Projectile),
                0,
                &[],
            ),
            r#mod(
                n!("EnemyKnockbackChance"),
                BASE,
                100.0,
                None,
                bor(ModFlag::Attack, ModFlag::Projectile),
                0,
                &[],
            ),
        ]),
        lua_line: 2354,
    });
    v.push(PatternEntry {
        pat: "projectiles cannot pierce, fork or chain",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotPierce"), None, ModFlag::Projectile, 0, &[]),
            flag(n!("CannotChain"), None, ModFlag::Projectile, 0, &[]),
            flag(n!("CannotFork"), None, ModFlag::Projectile, 0, &[]),
        ]),
        lua_line: 2358,
    });
    v.push(PatternEntry {
        pat: "projectiles cannot continue after colliding with targets",
        val: SpecialVal::Mods(vec![
            flag(n!("CannotPierce"), None, ModFlag::Projectile, 0, &[]),
            flag(n!("CannotChain"), None, ModFlag::Projectile, 0, &[]),
            flag(n!("CannotFork"), None, ModFlag::Projectile, 0, &[]),
            flag(n!("CannotSplit"), None, ModFlag::Projectile, 0, &[]),
        ]),
        lua_line: 2363,
    });
    v.push(PatternEntry {
        pat: "critical strikes inflict scorch, brittle and sapped",
        val: SpecialVal::Mods(vec![flag(n!("CritAlwaysAltAilments"), None, 0, 0, &[])]),
        lua_line: 2369,
    });
    v.push(PatternEntry {
        pat: "chance to block attack damage is doubled",
        val: SpecialVal::Mods(vec![r#mod(n!("BlockChance"), MORE, 100.0, None, 0, 0, &[])]),
        lua_line: 2370,
    });
    v.push(PatternEntry {
        pat: "chance to block spell damage is doubled",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SpellBlockChance"),
            MORE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2371,
    });
    v.push(PatternEntry {
        pat: "you take (%d+)%% of damage from blocked hits",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("BlockEffect"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2372,
    });
    v.push(PatternEntry {
        pat: "ignore attribute requirements",
        val: SpecialVal::Mods(vec![flag(
            n!("IgnoreAttributeRequirements"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2373,
    });
    v.push(PatternEntry {
        pat: "ignore attribute requirements of gems socketed in blue sockets",
        val: SpecialVal::Mods(vec![r#mod(
            n!("GemProperty"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("keyword"), Value::from("all")),
                    (n!("key"), Value::from("req")),
                    (n!("value"), Value::from(0.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}", socketColor = "B" })],
        )]),
        lua_line: 2374,
    });
    v.push(PatternEntry {
        pat: "ignore attribute requirements of socketed gems",
        val: SpecialVal::Mods(vec![r#mod(
            n!("GemProperty"),
            LIST,
            vtab(
                vec![],
                vec![
                    (n!("keyword"), Value::from("all")),
                    (n!("key"), Value::from("req")),
                    (n!("value"), Value::from(0.0)),
                ],
            ),
            None,
            0,
            0,
            &[tag!({ type = SocketedIn, slotName = "{SlotName}" })],
        )]),
        lua_line: 2375,
    });
    v.push(PatternEntry {
        pat: "gain no inherent bonuses from attributes",
        val: SpecialVal::Mods(vec![flag(n!("NoAttributeBonuses"), None, 0, 0, &[])]),
        lua_line: 2376,
    });
    v.push(PatternEntry {
        pat: "gain no inherent bonuses from strength",
        val: SpecialVal::Mods(vec![flag(
            n!("NoStrengthAttributeBonuses"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2377,
    });
    v.push(PatternEntry {
        pat: "gain no inherent bonuses from dexterity",
        val: SpecialVal::Mods(vec![flag(
            n!("NoDexterityAttributeBonuses"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2378,
    });
    v.push(PatternEntry {
        pat: "gain no inherent bonuses from intelligence",
        val: SpecialVal::Mods(vec![flag(
            n!("NoIntelligenceAttributeBonuses"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2379,
    });
    // hand-ported (lua2rs: assign-type): `slot` is reassigned before use
    v.push(PatternEntry {
        pat: "gain no (.+) from equipped (.+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let stat = ns(caps.cap[0], "concatenate local 'stat'");
            let mut slot = caps.cap[1];
            // L2381
            if slot == Some("shield") {
                slot = Some("Weapon 2");
            }
            let slot = ns(slot, "concatenate local 'slot'");
            // L2382
            Some(vec![flag(
                cat!(
                    "GainNo",
                    gsub(
                        &sub(
                            &gsub_fn(
                                &cat!(" ", stat),
                                "%W%l",
                                |c| Some(upper(c[0].as_str())),
                                None
                            )
                            .0,
                            2,
                            None
                        ),
                        " ",
                        "",
                        None
                    )
                    .0
                    .as_str(),
                    "From",
                    sub(
                        &gsub_fn(
                            &cat!(" ", slot),
                            "%W%l",
                            |c| Some(upper(c[0].as_str())),
                            None
                        )
                        .0,
                        2,
                        None
                    )
                    .as_ref()
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2380,
    });
    // Allows override of "chaos damage does not bypass energy shield" and similar mods
    v.push(PatternEntry {
        pat: "all damage taken bypasses energy shield",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("PhysicalEnergyShieldBypass"),
                OVERRIDE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("LightningEnergyShieldBypass"),
                OVERRIDE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("ColdEnergyShieldBypass"),
                OVERRIDE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("FireEnergyShieldBypass"),
                OVERRIDE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
            r#mod(
                n!("ChaosEnergyShieldBypass"),
                OVERRIDE,
                100.0,
                None,
                0,
                0,
                &[],
            ),
        ]),
        lua_line: 2387,
    });
    v.push(PatternEntry {
        pat: "physical damage taken bypasses energy shield",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PhysicalEnergyShieldBypass"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2394,
    });
    v.push(PatternEntry {
        pat: "auras from your skills do not affect allies",
        val: SpecialVal::Mods(vec![flag(
            n!("SelfAuraSkillsCannotAffectAllies"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2397,
    });
    v.push(PatternEntry {
        pat: "auras from your skills have (%d+)%% more effect on you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SkillAuraEffectOnSelf"),
                MORE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2398,
    });
    v.push(PatternEntry {
        pat: "auras from your skills have (%d+)%% increased effect on you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SkillAuraEffectOnSelf"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2399,
    });
    v.push(PatternEntry { pat: "increases and reductions to mana regeneration rate instead apply to rage regeneration rate", val: SpecialVal::Mods(vec![flag(n!("ManaRegenToRageRegen"), None, 0, 0, &[])]), lua_line: 2400 });
    v.push(PatternEntry {
        pat: "increases and reductions to maximum energy shield instead apply to ward",
        val: SpecialVal::Mods(vec![flag(n!("EnergyShieldToWard"), None, 0, 0, &[])]),
        lua_line: 2401,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of damage taken bypasses ward",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("WardBypass"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2402,
    });
    v.push(PatternEntry {
        pat: "ward has a (%d+)%% chance to not break",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("WardAvoidBreakChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2403,
    });
    v.push(PatternEntry {
        pat:
            "damage taken bypasses unbroken ward if the hit deals less damage than (%d+)%% of ward",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("WardBypassBelowPercent"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2404,
    });
    v.push(PatternEntry {
        pat: "maximum energy shield is (%d+)",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShield"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2405,
    });
    v.push(PatternEntry { pat: "while not on full life, sacrifice ([%d%.]+)%% of mana per second to recover that much life", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ManaDegenPercent"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "FullLife", neg = true })]), r#mod(n!("LifeRecovery"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Mana", percent = num }), tag!({ type = Condition, var = "FullLife", neg = true })])])
    }), lua_line: 2406 });
    // Override as increased maximum is always global
    v.push(PatternEntry {
        pat: "(%d+)%% increased maximum energy shield",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShield"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Global })],
            )])
        }),
        lua_line: 2410,
    });
    v.push(PatternEntry {
        pat: "you are blind",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:Blinded"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CannotBeBlinded", neg = true })],
        )]),
        lua_line: 2411,
    });
    v.push(PatternEntry { pat: "armour applies to fire, cold and lightning damage taken from hits instead of physical damage", val: SpecialVal::Mods(vec![r#mod(n!("ArmourAppliesToFireDamageTaken"), BASE, 100.0, None, 0, 0, &[]), r#mod(n!("ArmourAppliesToColdDamageTaken"), BASE, 100.0, None, 0, 0, &[]), r#mod(n!("ArmourAppliesToLightningDamageTaken"), BASE, 100.0, None, 0, 0, &[]), flag(n!("ArmourDoesNotApplyToPhysicalDamageTaken"), None, 0, 0, &[])]), lua_line: 2412 });
    v.push(PatternEntry {
        pat: "(%d+)%% of armour also applies to chaos damage taken from hits",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ArmourAppliesToChaosDamageTaken"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2418,
    });
    v.push(PatternEntry {
        pat: "armour also applies to chaos damage taken from hits",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![r#mod(
                n!("ArmourAppliesToChaosDamageTaken"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2419,
    });
    v.push(PatternEntry {
        pat: "maximum damage reduction for any damage type is (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("DamageReductionMax"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2420,
    });
    v.push(PatternEntry {
        pat: "gain additional elemental damage reduction equal to half your chaos resistance",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ElementalDamageReduction"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = PerStat, stat = "ChaosResist", div = 2.0 })],
        )]),
        lua_line: 2421,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% of maximum mana is converted to twice that much armour",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ManaConvertToArmour"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2424,
    });
    v.push(PatternEntry {
        pat: "life recovery from flasks also applies to energy shield",
        val: SpecialVal::Mods(vec![flag(
            n!("LifeFlaskAppliesToEnergyShield"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2427,
    });
    v.push(PatternEntry {
        pat: "increase to cast speed from arcane surge also applies to movement speed",
        val: SpecialVal::Mods(vec![flag(
            n!("ArcaneSurgeCastSpeedToMovementSpeed"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2428,
    });
    v.push(PatternEntry {
        pat: "arcane surge also grants (%d+)%% increased life regeneration rate to you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ArcaneSurgeAlsoLifeRegen"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2429,
    });
    v.push(PatternEntry { pat: "increases and reductions to effect of flasks applied to you also applies to effect of arcane surge on you at (%d+)%% of their value", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FlaskEffectToArcaneSurgeEffect"), BASE, num, None, 0, 0, &[])])
    }), lua_line: 2430 });
    v.push(PatternEntry {
        pat: "non%-instant mana recovery from flasks is also recovered as life",
        val: SpecialVal::Mods(vec![flag(n!("ManaFlaskAppliesToLife"), None, 0, 0, &[])]),
        lua_line: 2431,
    });
    v.push(PatternEntry {
        pat: "life leech effects recover energy shield instead while on full life",
        val: SpecialVal::Mods(vec![flag(
            n!("ImmortalAmbition"),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "FullLife" }),
                tag!({ type = Condition, var = "LeechingLife" }),
            ],
        )]),
        lua_line: 2432,
    });
    v.push(PatternEntry {
        pat: "shepherd of souls",
        val: SpecialVal::Mods(vec![flag(n!("ShepherdOfSouls"), None, 0, 0, &[])]),
        lua_line: 2433,
    });
    v.push(PatternEntry {
        pat: "you have shepherd of souls if at least (%d+) corrupted items are equipped",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![flag(
                n!("ShepherdOfSouls"),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "CorruptedItem", threshold = num })],
            )])
        }),
        lua_line: 2436,
    });
    v.push(PatternEntry {
        pat: "you have everlasting sacrifice if at least (%d+) corrupted items are equipped",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![flag(
                n!("Condition:EverlastingSacrifice"),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "CorruptedItem", threshold = num })],
            )])
        }),
        lua_line: 2439,
    });
    // Hollow Palm 3 suffixes
    v.push(PatternEntry { pat: "adds (%d+) to (%d+) attack physical damage to melee skills per (%d+) dexterity while you are unencumbered", val: SpecialVal::Fn(|caps: &Caps| {
        let min = ns(caps.cap[0], "index local 'min'");
        let max = ns(caps.cap[1], "index local 'max'");
        let dex = ns(caps.cap[2], "index local 'dex'");
        Some(vec![r#mod(n!("PhysicalMin"), BASE, tonumber(min), None, ModFlag::Melee, KeywordFlag::Attack, &[tag!({ type = PerStat, stat = "Dex", div = tonumber(dex) }), tag!({ type = Condition, var = "Unencumbered" })]), r#mod(n!("PhysicalMax"), BASE, tonumber(max), None, ModFlag::Melee, KeywordFlag::Attack, &[tag!({ type = PerStat, stat = "Dex", div = tonumber(dex) }), tag!({ type = Condition, var = "Unencumbered" })])])
    }), lua_line: 2442 });
    // Eclipse Solaris
    v.push(PatternEntry { pat: "adds (%d+) to (%d+) fire damage to attacks for every (%d+)%% your light radius is above base value", val: SpecialVal::Fn(|caps: &Caps| {
        let min = ns(caps.cap[0], "index local 'min'");
        let max = ns(caps.cap[1], "index local 'max'");
        let lightRadius = ns(caps.cap[2], "index local 'lightRadius'");
        Some(vec![r#mod(n!("FireMin"), BASE, tonumber(min), None, ModFlag::Attack, 0, &[tag!({ type = PerStat, stat = "LightRadiusInc", div = tonumber(lightRadius) })]), r#mod(n!("FireMax"), BASE, tonumber(max), None, ModFlag::Attack, 0, &[tag!({ type = PerStat, stat = "LightRadiusInc", div = tonumber(lightRadius) })])])
    }), lua_line: 2446 });
    v.push(PatternEntry {
        pat: "(%d+)%% more attack damage if accuracy rating is higher than maximum life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Damage"),
                    MORE,
                    num,
                    Some("Damage"),
                    ModFlag::Attack,
                    0,
                    &[
                        tag!({ type = Condition, var = "MainHandAccRatingHigherThanMaxLife" }),
                        tag!({ type = Condition, var = "MainHandAttack" }),
                    ],
                ),
                r#mod(
                    n!("Damage"),
                    MORE,
                    num,
                    Some("Damage"),
                    ModFlag::Attack,
                    0,
                    &[
                        tag!({ type = Condition, var = "OffHandAccRatingHigherThanMaxLife" }),
                        tag!({ type = Condition, var = "OffHandAttack" }),
                    ],
                ),
            ])
        }),
        lua_line: 2450,
    });
}

/// @pob Modules/ModParser.lua:2454-2570 h=8d824a8a
fn specialModList_2454(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "your hexes have infinite duration",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Duration"),
            BASE,
            m_huge,
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::AppliesCurse })],
        )]),
        lua_line: 2454,
    });
    v.push(PatternEntry { pat: "(%d+)%% chance to defend with double your armour for each time you've been hit by an enemy recently, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let numChance = caps.num;
        let cap = ns(caps.cap[1], "index local 'cap'");
        Some(vec![r#mod(n!("ArmourDefense"), MAX, 100.0, Some("Armour Mastery: Max Calc"), 0, 0, &[tag!({ type = Condition, var = "ArmourMax" })]), r#mod(n!("ArmourDefense"), MAX, (m_min((nn(numChance, "perform arithmetic on local 'numChance'") / 100.0), 1.0) * 100.0), Some("Armour Mastery: Average Calc"), 0, 0, &[tag!({ type = Condition, var = "ArmourAvg" }), tag!({ type = Multiplier, var = "BeenHitRecently", limit = (snum(cap, "perform arithmetic on a string") / nn(numChance, "perform arithmetic on local 'numChance'")) })]), r#mod(n!("ArmourDefense"), MAX, (m_min(m_floor((nn(numChance, "perform arithmetic on local 'numChance'") / 100.0)), 1.0) * 100.0), Some("Armour Mastery: Min Calc"), 0, 0, &[tag!({ type = Condition, var = "ArmourMax", neg = true }), tag!({ type = Condition, var = "ArmourAvg", neg = true }), tag!({ type = Multiplier, var = "BeenHitRecently", limit = (snum(cap, "perform arithmetic on a string") / nn(numChance, "perform arithmetic on local 'numChance'")) })])])
    }), lua_line: 2455 });
    v.push(PatternEntry { pat: "(%d+)%% chance to defend with (%d+)%% of armour for each time you've been hit by an enemy recently, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let numChance = caps.num;
        let numArmourMultiplier = ns(caps.cap[1], "index local 'numArmourMultiplier'");
        let cap = ns(caps.cap[2], "index local 'cap'");
        Some(vec![r#mod(n!("ArmourDefense"), MAX, (nn(tonumber(numArmourMultiplier), "perform arithmetic on a nil value") - 100.0), Some("Armour Mastery: Max Calc"), 0, 0, &[tag!({ type = Condition, var = "ArmourMax" })]), r#mod(n!("ArmourDefense"), MAX, (m_min((nn(numChance, "perform arithmetic on local 'numChance'") / 100.0), 1.0) * (nn(tonumber(numArmourMultiplier), "perform arithmetic on a nil value") - 100.0)), Some("Armour Mastery: Average Calc"), 0, 0, &[tag!({ type = Condition, var = "ArmourAvg" }), tag!({ type = Multiplier, var = "BeenHitRecently", limit = (snum(cap, "perform arithmetic on a string") / nn(numChance, "perform arithmetic on local 'numChance'")) })]), r#mod(n!("ArmourDefense"), MAX, (m_min(m_floor((nn(numChance, "perform arithmetic on local 'numChance'") / 100.0)), 1.0) * (nn(tonumber(numArmourMultiplier), "perform arithmetic on a nil value") - 100.0)), Some("Armour Mastery: Min Calc"), 0, 0, &[tag!({ type = Condition, var = "ArmourMax", neg = true }), tag!({ type = Condition, var = "ArmourAvg", neg = true }), tag!({ type = Multiplier, var = "BeenHitRecently", limit = (snum(cap, "perform arithmetic on a string") / nn(numChance, "perform arithmetic on local 'numChance'")) })])])
    }), lua_line: 2460 });
    // Legacy support
    v.push(PatternEntry {
        pat: "(%d+)%% chance to defend with double armour",
        val: SpecialVal::Fn(|caps: &Caps| {
            let numChance = caps.num;
            Some(vec![
                r#mod(
                    n!("ArmourDefense"),
                    MAX,
                    100.0,
                    Some("Armour Mastery: Max Calc"),
                    0,
                    0,
                    &[tag!({ type = Condition, var = "ArmourMax" })],
                ),
                r#mod(
                    n!("ArmourDefense"),
                    MAX,
                    (m_min(
                        (nn(numChance, "perform arithmetic on local 'numChance'") / 100.0),
                        1.0,
                    ) * 100.0),
                    Some("Armour Mastery: Average Calc"),
                    0,
                    0,
                    &[tag!({ type = Condition, var = "ArmourAvg" })],
                ),
                r#mod(
                    n!("ArmourDefense"),
                    MAX,
                    (m_min(
                        m_floor((nn(numChance, "perform arithmetic on local 'numChance'") / 100.0)),
                        1.0,
                    ) * 100.0),
                    Some("Armour Mastery: Min Calc"),
                    0,
                    0,
                    &[
                        tag!({ type = Condition, var = "ArmourMax", neg = true }),
                        tag!({ type = Condition, var = "ArmourAvg", neg = true }),
                    ],
                ),
            ])
        }),
        lua_line: 2466,
    });
    // Masteries
    v.push(PatternEntry { pat: "hits have (%d+)%% chance to treat enemy monster elemental resistance values as inverted", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("HitsInvertEleResChance"), CHANCE, (nn(num, "perform arithmetic on local 'num'") / 100.0), None, 0, 0, &[])])
    }), lua_line: 2472 });
    v.push(PatternEntry {
        pat: "off hand accuracy is equal to main hand accuracy while wielding a sword",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:OffHandAccuracyIsMainHandAccuracy"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingSword" })],
        )]),
        lua_line: 2475,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased accuracy rating at close range",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AtCloseRange" })],
            )])
        }),
        lua_line: 2476,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% more accuracy rating at close range",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AtCloseRange" })],
            )])
        }),
        lua_line: 2477,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased accuracy rating against unique enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })],
            )])
        }),
        lua_line: 2478,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% more accuracy rating against unique enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AccuracyVsEnemy"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })],
            )])
        }),
        lua_line: 2479,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to defend with (%d+)%% of armour",
        val: SpecialVal::Fn(|caps: &Caps| {
            let numChance = caps.num;
            let numArmourMultiplier = ns(caps.cap[1], "index local 'numArmourMultiplier'");
            Some(vec![
                r#mod(
                    n!("ArmourDefense"),
                    MAX,
                    (nn(
                        tonumber(numArmourMultiplier),
                        "perform arithmetic on a nil value",
                    ) - 100.0),
                    Some("Armour Mastery: Max Calc"),
                    0,
                    0,
                    &[tag!({ type = Condition, var = "ArmourMax" })],
                ),
                r#mod(
                    n!("ArmourDefense"),
                    MAX,
                    (m_min(
                        (nn(numChance, "perform arithmetic on local 'numChance'") / 100.0),
                        1.0,
                    ) * (nn(
                        tonumber(numArmourMultiplier),
                        "perform arithmetic on a nil value",
                    ) - 100.0)),
                    Some("Armour Mastery: Average Calc"),
                    0,
                    0,
                    &[tag!({ type = Condition, var = "ArmourAvg" })],
                ),
                r#mod(
                    n!("ArmourDefense"),
                    MAX,
                    (m_min(
                        m_floor((nn(numChance, "perform arithmetic on local 'numChance'") / 100.0)),
                        1.0,
                    ) * (nn(
                        tonumber(numArmourMultiplier),
                        "perform arithmetic on a nil value",
                    ) - 100.0)),
                    Some("Armour Mastery: Min Calc"),
                    0,
                    0,
                    &[
                        tag!({ type = Condition, var = "ArmourMax", neg = true }),
                        tag!({ type = Condition, var = "ArmourAvg", neg = true }),
                    ],
                ),
            ])
        }),
        lua_line: 2480,
    });
    v.push(PatternEntry {
        pat: "defend with (%d+)%% of armour while not on low energy shield",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ArmourDefense"),
                MAX,
                (nn(num, "perform arithmetic on local 'num'") - 100.0),
                Some("Armour and Energy Shield Mastery"),
                0,
                0,
                &[tag!({ type = Condition, var = "LowEnergyShield", neg = true })],
            )])
        }),
        lua_line: 2485,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased armour and energy shield from equipped body armour if equipped helmet, gloves and boots all have armour and energy shield", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Body ArmourESAndArmour"), INC, num, None, 0, 0, &[tag!({ type = StatThreshold, stat = "ArmourOnGloves", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "EnergyShieldOnGloves", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "ArmourOnHelmet", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "EnergyShieldOnHelmet", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "ArmourOnBoots", threshold = 1.0 }), tag!({ type = StatThreshold, stat = "EnergyShieldOnBoots", threshold = 1.0 })])])
    }), lua_line: 2488 });
    v.push(PatternEntry {
        pat: "brands have (%d+)%% increased area of effect if (%d+)%% of attached duration expired",
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
                    tag!({ type = Condition, var = "BrandLastHalf" }),
                    tag!({ type = SkillType, skillType = SkillType::Brand }),
                ],
            )])
        }),
        lua_line: 2498,
    });
    v.push(PatternEntry {
        pat: "corrupted blood cannot be inflicted on you",
        val: SpecialVal::Mods(vec![flag(n!("CorruptedBloodImmune"), None, 0, 0, &[])]),
        lua_line: 2499,
    });
    v.push(PatternEntry {
        pat: "you cannot be hindered",
        val: SpecialVal::Mods(vec![flag(n!("HinderImmune"), None, 0, 0, &[])]),
        lua_line: 2500,
    });
    v.push(PatternEntry {
        pat: "you cannot be maimed",
        val: SpecialVal::Mods(vec![flag(n!("MaimImmune"), None, 0, 0, &[])]),
        lua_line: 2501,
    });
    v.push(PatternEntry {
        pat: "you cannot be impaled",
        val: SpecialVal::Mods(vec![flag(n!("ImpaleImmune"), None, 0, 0, &[])]),
        lua_line: 2502,
    });
    // Exerted Attacks
    v.push(PatternEntry {
        pat: "exerted attacks deal (%d+)%% increased damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExertIncrease"),
                INC,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 2504,
    });
    v.push(PatternEntry {
        pat: "exerted attacks have (%d+)%% chance to deal double damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExertDoubleDamageChance"),
                BASE,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 2505,
    });
    // Ascendant
    v.push(PatternEntry {
        pat: "grants (%d+) passive skill points?",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ExtraPoints"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2507,
    });
    v.push(PatternEntry {
        pat: "can allocate passives from the %a+'s starting point",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2508,
    });
    v.push(PatternEntry { pat: "projectiles gain damage as they travel farther, dealing up to (%d+)%% increased damage with hits to targets", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, bor(ModFlag::Hit, ModFlag::Projectile), 0, &[tag!({ type = DistanceRamp, ramp = &[[35.0, 0.0], [70.0, 1.0]][..] })])])
    }), lua_line: 2509 });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain elusive on kill",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeElusive"), None, 0, 0, &[])]),
        lua_line: 2510,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments while on consecrated ground",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "OnConsecratedGround" })],
        )]),
        lua_line: 2513,
    });
    // Assassin
    v.push(PatternEntry {
        pat: "poison you inflict with critical strikes deals (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                0,
                KeywordFlag::Poison,
                &[tag!({ type = Condition, var = "CriticalStrike" })],
            )])
        }),
        lua_line: 2515,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain elusive on critical strike",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanBeElusive"), None, 0, 0, &[])]),
        lua_line: 2516,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% more damage while there is at most one rare or unique enemy nearby",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AtMostOneNearbyRareOrUniqueEnemy" })],
            )])
        }),
        lua_line: 2519,
    });
    // Legacy variant pre-3.26
    v.push(PatternEntry { pat: "(%d+)%% reduced damage taken while there are at least two rare or unique enemies nearby", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "NearbyRareOrUniqueEnemies", threshold = 2.0 })])])
    }), lua_line: 2520 });
    v.push(PatternEntry { pat: "(%d+)%% less damage taken while there are at least two rare or unique enemies nearby", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("DamageTaken"), MORE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "NearbyRareOrUniqueEnemies", threshold = 2.0 })])])
    }), lua_line: 2521 });
    v.push(PatternEntry {
        pat: "you take no extra damage from critical strikes while elusive",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ReduceCritExtraDamage"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Elusive" })],
        )]),
        lua_line: 2522,
    });
    v.push(PatternEntry {
        pat: "mark skills cost no mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ManaCost"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = SkillType, skillType = SkillType::Mark })],
        )]),
        lua_line: 2523,
    });
    // Berserker
    v.push(PatternEntry {
        pat: "gain %d+ rage when you kill an enemy",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2525,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage when you use a warcry",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2528,
    });
    v.push(PatternEntry {
        pat: "you and nearby party members gain %d+ rage when you warcry",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2531,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on hit with attacks, no more than once every [%d%.]+ seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2534,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on attack hit",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2537,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on hit with axes or swords",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2540,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on melee hit",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2543,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on melee weapon hit",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2546,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on hit with axes",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2549,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage when hit by an enemy",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2552,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage on hit with retaliation skills",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2555,
    });
    v.push(PatternEntry {
        pat: "gain %d+ rage when you use a life flask",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2558,
    });
    v.push(PatternEntry { pat: "while a unique enemy is in your presence, gain %d+ rage on hit with attacks, no more than once every [%d%.]+ seconds", val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])]), lua_line: 2561 });
    v.push(PatternEntry { pat: "while a pinnacle atlas boss is in your presence, gain %d+ rage on hit with attacks, no more than once every [%d%.]+ seconds", val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })])]), lua_line: 2564 });
    v.push(PatternEntry {
        pat: "maximum rage is halved",
        val: SpecialVal::Mods(vec![r#mod(n!("MaximumRage"), MORE, -50.0, None, 0, 0, &[])]),
        lua_line: 2567,
    });
    v.push(PatternEntry {
        pat: "inherent effects from having rage are tripled",
        val: SpecialVal::Mods(vec![r#mod(n!("RageEffect"), MORE, 200.0, None, 0, 0, &[])]),
        lua_line: 2568,
    });
    v.push(PatternEntry {
        pat: "inherent effects from having rage are doubled",
        val: SpecialVal::Mods(vec![r#mod(n!("RageEffect"), MORE, 100.0, None, 0, 0, &[])]),
        lua_line: 2569,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned while you have at least (%d+) rage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![flag(
                n!("StunImmune"),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "Rage", threshold = num })],
            )])
        }),
        lua_line: 2570,
    });
}

/// @pob Modules/ModParser.lua:2571-2674 h=72a26425
fn specialModList_2571(v: &mut Vec<PatternEntry<SpecialVal>>) {
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber (known diff MP3-1, calc doc §6)
    v.push(PatternEntry { pat: "(%d+)%% less damage taken per (%d+) rage, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        let limit = ns(caps.cap[2], "index local 'limit'");
        Some(vec![r#mod(n!("DamageTaken"), MORE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = Multiplier, var = "Rage", div = snum(div, "perform arithmetic on tag.div"), limit = (-nn(tonumber(limit), "perform arithmetic on a nil value")), limitNegTotal = true })])])
    }), lua_line: 2571 });
    v.push(PatternEntry {
        pat: "lose ([%d%.]+)%% of life per second per rage while you are not losing rage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeDegen"),
                BASE,
                1.0,
                None,
                0,
                0,
                &[
                    tag!({ type = PercentStat, stat = "Life", percent = num }),
                    tag!({ type = Multiplier, var = "RageEffect" }),
                ],
            )])
        }),
        lua_line: 2574,
    });
    v.push(PatternEntry { pat: "if you've warcried recently, you and nearby allies have (%d+)%% increased attack speed", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Speed"), INC, num, None, ModFlag::Attack, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "UsedWarcryRecently" })])])
    }), lua_line: 2575 });
    v.push(PatternEntry { pat: "gain (%d+)%% increased armour per (%d+) power for 8 seconds when you warcry, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        let limit = ns(caps.cap[2], "index local 'limit'");
        Some(vec![r#mod(n!("Armour"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "WarcryPower", div = tonumber(div), globalLimit = tonumber(limit), globalLimitKey = "WarningCall" }), tag!({ type = Condition, var = "UsedWarcryInPast8Seconds" })])])
    }), lua_line: 2576 });
    v.push(PatternEntry {
        pat: "warcries grant (%d+) rage per (%d+) power if you have less than (%d+) rage",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2579,
    });
    v.push(PatternEntry {
        pat: "warcries grant (%d+) rage per (%d+) enemy power, up to (%d+)",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanGainRage"), None, 0, 0, &[])]),
        lua_line: 2582,
    });
    v.push(PatternEntry {
        pat: "exerted attacks deal (%d+)%% more attack damage if a warcry sacrificed rage recently",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExertAttackIncrease"),
                MORE,
                num,
                None,
                ModFlag::Attack,
                0,
                &[],
            )])
        }),
        lua_line: 2585,
    });
    v.push(PatternEntry {
        pat: "deal (%d+)%% less damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2586,
    });
    v.push(PatternEntry {
        pat: "warcries exert twice as many attacks",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraExertedAttacks"),
            MORE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2587,
    });
    // Champion
    v.push(PatternEntry {
        pat: "cannot be stunned while you have fortify",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Fortified" })],
        )]),
        lua_line: 2589,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned while fortified",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Fortified" })],
        )]),
        lua_line: 2590,
    });
    v.push(PatternEntry { pat: "you cannot be stunned while at maximum endurance charges", val: SpecialVal::Mods(vec![flag(n!("StunImmune"), None, 0, 0, &[tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMax" })])]), lua_line: 2591 });
    v.push(PatternEntry {
        pat: "fortify",
        val: SpecialVal::Mods(vec![flag(n!("Condition:Fortified"), None, 0, 0, &[])]),
        lua_line: 2592,
    });
    v.push(PatternEntry {
        pat: "you have your maximum fortification",
        val: SpecialVal::Mods(vec![
            flag(n!("Condition:Fortified"), None, 0, 0, &[]),
            flag(n!("Condition:HaveMaxFortification"), None, 0, 0, &[]),
        ]),
        lua_line: 2593,
    });
    v.push(PatternEntry {
        pat: "you have (%d+) fortification",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MinimumFortification"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2594,
    });
    v.push(PatternEntry { pat: "nearby allies count as having fortification equal to yours", val: SpecialVal::Mods(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("onlyAllies"), Value::from(true)), (n!("mod"), Value::from(r#mod(n!("YourFortifyEqualToParent"), FLAG, true, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])))]), None, 0, 0, &[])]), lua_line: 2595 });
    v.push(PatternEntry {
        pat: "enemies taunted by you cannot evade attacks",
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
                        &[tag!({ type = Condition, var = "Taunted" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2596,
    });
    v.push(PatternEntry {
        pat: "if you've impaled an enemy recently, you and nearby allies have %+(%d+) to armour",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraAura"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(n!("Armour"), BASE, num, None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ImpaledRecently" })],
            )])
        }),
        lua_line: 2597,
    });
    v.push(PatternEntry {
        pat: "your hits permanently intimidate enemies that are on full life",
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
                        &[tag!({ type = Condition, var = "ChampionIntimidate" })],
                    )),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2598,
    });
    v.push(PatternEntry { pat: "you and allies affected by your placed banners regenerate ([%d%.]+)%% of life per second for each stage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByPlacedBanner" }), tag!({ type = Multiplier, var = "BannerValour" })])))]), None, 0, 0, &[])])
    }), lua_line: 2599 });
    v.push(PatternEntry { pat: "you and allies near your banner regenerate ([%d%.]+)%% of life per second for each valour consumed for that banner", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LifeRegenPercent"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByPlacedBanner" }), tag!({ type = Multiplier, var = "BannerValour" })])))]), None, 0, 0, &[])])
    }), lua_line: 2602 });
    // Chieftain
    v.push(PatternEntry {
        pat: "enemies near your totems take (%d+)%% increased physical and fire damage",
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
                            Value::from(r#mod(
                                n!("PhysicalDamageTaken"),
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
                    &[],
                ),
                r#mod(
                    n!("EnemyModifier"),
                    LIST,
                    vtab(
                        vec![],
                        vec![(
                            n!("mod"),
                            Value::from(r#mod(n!("FireDamageTaken"), INC, num, None, 0, 0, &[])),
                        )],
                    ),
                    None,
                    0,
                    0,
                    &[],
                ),
            ])
        }),
        lua_line: 2606,
    });
    v.push(PatternEntry { pat: "every (%d+) seconds, gain (%d+)%% of physical damage as extra fire damage for (%d+) seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = ns(caps.cap[1], "index local 'num'");
        Some(vec![r#mod(n!("PhysicalDamageGainAsFire"), BASE, tonumber(num), None, 0, 0, &[tag!({ type = Condition, var = "NgamahuFlamesAdvance" })])])
    }), lua_line: 2610 });
    v.push(PatternEntry { pat: "(%d+)%% more damage for each endurance charge lost recently, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, 0, &[tag!({ type = Multiplier, var = "EnduranceChargesLostRecently", limit = tonumber(limit), limitTotal = true })])])
    }), lua_line: 2613 });
    v.push(PatternEntry { pat: "nearby enemy monsters have no fire resistance against damage over time while you are stationary", val: SpecialVal::Mods(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireResist"), OVERRIDE, 0.0, None, ModFlag::Dot, 0, &[tag!({ type = ActorCondition, actor = "player", var = "Stationary" })])))]), None, 0, 0, &[])]), lua_line: 2616 });
    v.push(PatternEntry {
        pat: "(%d+)%% more damage if you've lost an endurance charge in the past 8 seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "LostEnduranceChargeInPast8Sec" })],
            )])
        }),
        lua_line: 2619,
    });
    v.push(PatternEntry { pat: "trigger level (%d+) (.+) when you attack with a non%-vaal slam or strike skill near an enemy", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let skill = ns(caps.cap[1], "index local 'skill'");
        Some(triggerExtraSkill(skill, num, None))
    }), lua_line: 2620 });
    // Ignore Timeless Jewels
    // hand-ported (lua2rs: nested-function): the jewel closure (2622-2627) becomes data, keyed by
    // its source range; its body runs in pob_calc::modules::jewel_funcs (MP7)
    v.push(PatternEntry { pat: "non%-unique jewels cause increases and reductions to other damage types in a (%a+) radius to be transformed to apply to (%a+) damage", val: SpecialVal::Fn(|caps: &Caps| {
        let radius = ns(caps.cap[0], "index local 'radius'");
        let dmgType = caps.cap[1];
        // L2622
        Some(vec![r#mod(
            n!("ExtraJewelFunc"),
            LIST,
            extra_jewel_func("Modules/ModParser.lua:2622-2627", caps, firstToUpper(radius)),
            None,
            0,
            0,
            &[
                tag!({ type = ItemCondition, itemSlot = "{SlotName}", rarityCond = "UNIQUE", neg = true }),
                tag!({ type = ItemCondition, itemSlot = "{SlotName}", rarityCond = "RELIC", neg = true }),
            ],
        )])
    }), lua_line: 2621 });
    // hand-ported (lua2rs: nested-function): the jewel closure (2630-2634) becomes data, keyed by
    // its source range; its body runs in pob_calc::modules::jewel_funcs (MP7)
    v.push(PatternEntry { pat: "non%-unique jewels cause small and notable passive skills in a (%a+) radius to also grant %+(%d+) to (%a+)", val: SpecialVal::Fn(|caps: &Caps| {
        let radius = ns(caps.cap[0], "index local 'radius'");
        let val = caps.cap[1];
        let attr = caps.cap[2];
        // L2630
        Some(vec![r#mod(
            n!("ExtraJewelFunc"),
            LIST,
            extra_jewel_func(
                "Modules/ModParser.lua:2630-2634",
                caps,
                gsub_fn(radius, "^%l", |c| Some(upper(c[0].as_str())), None).0,
            ),
            None,
            0,
            0,
            &[
                tag!({ type = ItemCondition, itemSlot = "{SlotName}", rarityCond = "UNIQUE", neg = true }),
                tag!({ type = ItemCondition, itemSlot = "{SlotName}", rarityCond = "RELIC", neg = true }),
            ],
        )])
    }), lua_line: 2629 });
    // Deadeye
    v.push(PatternEntry {
        pat: "projectiles pierce all nearby targets",
        val: SpecialVal::Mods(vec![flag(n!("PierceAllTargets"), None, 0, 0, &[])]),
        lua_line: 2637,
    });
    v.push(PatternEntry {
        pat: "gain %+(%d+) life when you hit a bleeding enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("LifeOnHit"),
                BASE,
                num,
                None,
                ModFlag::Hit,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })],
            )])
        }),
        lua_line: 2638,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased blink arrow and mirror arrow cooldown recovery speed", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CooldownRecovery"), INC, num, None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Blink Arrow"), n!("Mirror Arrow")][..], includeTransfigured = true })])])
    }), lua_line: 2639 });
    v.push(PatternEntry {
        pat: "critical strikes which inflict bleeding also inflict rupture",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanInflictRupture"),
            None,
            0,
            0,
            &[tag!({ type = Condition, neg = true, var = "NeverCrit" })],
        )]),
        lua_line: 2642,
    });
    v.push(PatternEntry {
        pat: "gain %d+ gale force when you use a skill",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanGainGaleForce"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2645,
    });
    v.push(PatternEntry {
        pat: "if you've used a skill recently, you and nearby allies have tailwind",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Tailwind"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsedSkillRecently" })],
        )]),
        lua_line: 2648,
    });
    v.push(PatternEntry {
        pat: "you and nearby allies have tailwind",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("Condition:Tailwind"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2649,
    });
    v.push(PatternEntry {
        pat: "projectiles deal (%d+)%% more damage for each remaining chain",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = PerStat, stat = "ChainRemaining" })],
            )])
        }),
        lua_line: 2650,
    });
    v.push(PatternEntry { pat: "projectiles deal (%d+)%% increased damage with hits and ailments for each remaining chain", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), INC, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = PerStat, stat = "ChainRemaining" }), tag!({ type = SkillType, skillType = SkillType::Projectile })])])
    }), lua_line: 2651 });
    v.push(PatternEntry {
        pat: "projectiles deal (%d+)%% increased damage for each remaining chain",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                INC,
                num,
                None,
                ModFlag::Projectile,
                0,
                &[tag!({ type = PerStat, stat = "ChainRemaining" })],
            )])
        }),
        lua_line: 2652,
    });
    v.push(PatternEntry {
        pat: "far shot",
        val: SpecialVal::Mods(vec![flag(n!("FarShot"), None, 0, 0, &[])]),
        lua_line: 2653,
    });
    v.push(PatternEntry { pat: "projectiles gain damage as they travel farther, dealing up to (%d+)%% more damage with hits and ailments", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = DistanceRamp, ramp = &[[35.0, 0.0], [70.0, 1.0]][..] })])])
    }), lua_line: 2654 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased mirage archer duration",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MirageArcherDuration"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2655,
    });
    v.push(PatternEntry {
        pat: "([%-%+]%d+) to maximum number of summoned mirage archers",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MirageArcherMaxCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2656,
    });
    v.push(PatternEntry {
        pat: "([%-%+]%d+) to maximum number of sacred wisps",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SacredWispsMaxCount"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2657,
    });
    // Elementalist
    v.push(PatternEntry {
        pat: "gain (%d+)%% increased area of effect for %d+ seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "PendulumOfDestructionAreaOfEffect" })],
            )])
        }),
        lua_line: 2659,
    });
    v.push(PatternEntry {
        pat: "gain (%d+)%% increased elemental damage for %d+ seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElementalDamage"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "PendulumOfDestructionElementalDamage" })],
            )])
        }),
        lua_line: 2660,
    });
    v.push(PatternEntry { pat: "for each element you've been hit by damage of recently, (%d+)%% increased damage of that element", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FireDamage"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "HitByFireDamageRecently" })]), r#mod(n!("ColdDamage"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "HitByColdDamageRecently" })]), r#mod(n!("LightningDamage"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "HitByLightningDamageRecently" })])])
    }), lua_line: 2661 });
    v.push(PatternEntry { pat: "for each element you've been hit by damage of recently, (%d+)%% reduced damage taken of that element", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("FireDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = Condition, var = "HitByFireDamageRecently" })]), r#mod(n!("ColdDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = Condition, var = "HitByColdDamageRecently" })]), r#mod(n!("LightningDamageTaken"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[tag!({ type = Condition, var = "HitByLightningDamageRecently" })])])
    }), lua_line: 2666 });
    v.push(PatternEntry {
        pat: "gain convergence when you hit a unique enemy, no more than once every %d+ seconds",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanGainConvergence"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2671,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased area of effect while you don't have convergence",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, neg = true, var = "Convergence" })],
            )])
        }),
        lua_line: 2674,
    });
}

/// @pob Modules/ModParser.lua:2675-2876 h=be6694d6
fn specialModList_2675(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "exposure you inflict applies an extra (%-?%d+)%% to the affected resistance",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ExtraExposure"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2675,
    });
    v.push(PatternEntry {
        pat: "cannot take reflected elemental damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ElementalReflectedDamageTaken"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 2676,
    });
    v.push(PatternEntry {
        pat: "every %d+ seconds:",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2677,
    });
    v.push(PatternEntry {
        pat: "gain chilling conflux for %d seconds",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            flag(
                n!("LightningCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            flag(
                n!("FireCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            flag(
                n!("ChaosCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
        ]),
        lua_line: 2678,
    });
    v.push(PatternEntry {
        pat: "gain shocking conflux for %d seconds",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyShockChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("PhysicalCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("ColdCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("FireCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("ChaosCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
        ]),
        lua_line: 2684,
    });
    v.push(PatternEntry {
        pat: "gain igniting conflux for %d seconds",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyIgniteChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("PhysicalCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("LightningCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("ColdCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("ChaosCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
        ]),
        lua_line: 2691,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain elemental conflux for %d seconds when you kill an enemy",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            flag(
                n!("LightningCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            flag(
                n!("FireCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            flag(
                n!("ChaosCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ChillingConflux" })],
            ),
            r#mod(
                n!("EnemyShockChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("PhysicalCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("ColdCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("FireCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            flag(
                n!("ChaosCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ShockingConflux" })],
            ),
            r#mod(
                n!("EnemyIgniteChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("PhysicalCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("LightningCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("ColdCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
            flag(
                n!("ChaosCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "IgnitingConflux" })],
            ),
        ]),
        lua_line: 2698,
    });
    v.push(PatternEntry {
        pat: "you have elemental conflux if the stars are aligned",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("LightningCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("FireCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("ChaosCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            r#mod(
                n!("EnemyShockChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("PhysicalCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("ColdCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("FireCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("ChaosCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            r#mod(
                n!("EnemyIgniteChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("PhysicalCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("LightningCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("ColdCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
            flag(
                n!("ChaosCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "StarsAreAligned" })],
            ),
        ]),
        lua_line: 2714,
    });
    v.push(PatternEntry {
        pat: "gain chilling, shocking and igniting conflux for %d seconds",
        val: SpecialVal::Mods(vec![]),
        lua_line: 2730,
    });
    v.push(PatternEntry {
        pat: "you have igniting, chilling and shocking conflux while affected by glorious madness",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("LightningCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("FireCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("ChaosCanChill"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            r#mod(
                n!("EnemyIgniteChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("PhysicalCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("LightningCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("ColdCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("ChaosCanIgnite"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            r#mod(
                n!("EnemyShockChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("PhysicalCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("ColdCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("FireCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
            flag(
                n!("ChaosCanShock"),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
            ),
        ]),
        lua_line: 2731,
    });
    v.push(PatternEntry {
        pat: "all damage from critical strikes can apply lightning ailments during effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanShock"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ColdCanShock"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("FireCanShock"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ChaosCanShock"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("PhysicalCanSap"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ColdCanSap"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("FireCanSap"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ChaosCanSap"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
        ]),
        lua_line: 2747,
    });
    v.push(PatternEntry {
        pat: "all damage from critical strikes can apply cold ailments during effect",
        val: SpecialVal::Mods(vec![
            flag(
                n!("PhysicalCanChill"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("LightningCanChill"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("FireCanChill"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ChaosCanChill"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("PhysicalCanFreeze"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("LightningCanFreeze"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("FireCanFreeze"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ChaosCanFreeze"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("PhysicalCanBrittle"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("LightningCanBrittle"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("FireCanBrittle"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
            flag(
                n!("ChaosCanBrittle"),
                None,
                0,
                0,
                &[
                    tag!({ type = Condition, var = "UsingFlask" }),
                    tag!({ type = Condition, neg = true, var = "NeverCrit" }),
                ],
            ),
        ]),
        lua_line: 2757,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments while affected by glorious madness",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByGloriousMadness" })],
        )]),
        lua_line: 2771,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments while focus?sed",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Focused" })],
        )]),
        lua_line: 2772,
    });
    v.push(PatternEntry {
        pat: "summoned golems are immune to elemental damage",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Elemancer"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Golem })],
            ),
            r#mod(
                n!("MinionModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("ElementalDamageTaken"),
                            MORE,
                            -100.0,
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
                &[tag!({ type = SkillType, skillType = SkillType::Golem })],
            ),
        ]),
        lua_line: 2773,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% increased golem damage per summoned golem",
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
                &[
                    tag!({ type = SkillType, skillType = SkillType::Golem }),
                    tag!({ type = PerStat, stat = "ActiveGolemLimit" }),
                ],
            )])
        }),
        lua_line: 2777,
    });
    v.push(PatternEntry {
        pat: "shocks from your hits always increase damage taken by at least (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ShockMinimum"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2778,
    });
    v.push(PatternEntry {
        pat: "chills from your hits always reduce action speed by at least (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ChillBase"), BASE, num, None, 0, 0, &[])])
        }),
        lua_line: 2779,
    });
    v.push(PatternEntry { pat: "(%d+)%% more damage with ignites you inflict with hits for which the highest damage type is fire", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, KeywordFlag::Ignite, &[tag!({ type = Condition, var = "FireIsHighestDamageType" })]), flag(n!("ChecksHighestDamage"), None, 0, 0, &[])])
    }), lua_line: 2780 });
    v.push(PatternEntry { pat: "(%d+)%% more effect of cold ailments you inflict with hits for which the highest damage type is cold", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyChillEffect"), MORE, num, None, 0, 0, &[tag!({ type = Condition, var = "ColdIsHighestDamageType" })]), r#mod(n!("EnemyBrittleEffect"), MORE, num, None, 0, 0, &[tag!({ type = Condition, var = "ColdIsHighestDamageType" })]), flag(n!("ChecksHighestDamage"), None, 0, 0, &[])])
    }), lua_line: 2784 });
    v.push(PatternEntry { pat: "(%d+)%% more effect of lightning ailments you inflict with hits if the highest damage type is lightning", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyShockEffect"), MORE, num, None, 0, 0, &[tag!({ type = Condition, var = "LightningIsHighestDamageType" })]), r#mod(n!("EnemySapEffect"), MORE, num, None, 0, 0, &[tag!({ type = Condition, var = "LightningIsHighestDamageType" })]), flag(n!("ChecksHighestDamage"), None, 0, 0, &[])])
    }), lua_line: 2789 });
    v.push(PatternEntry {
        pat: "your chills can reduce action speed by up to a maximum of (%d+)%%",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ChillMax"), OVERRIDE, num, None, 0, 0, &[])])
        }),
        lua_line: 2794,
    });
    v.push(PatternEntry {
        pat: "your hits always ignite",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyIgniteChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2795,
    });
    v.push(PatternEntry {
        pat: "hits always ignite",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyIgniteChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2796,
    });
    v.push(PatternEntry {
        pat: "your hits always shock",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyShockChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2797,
    });
    v.push(PatternEntry {
        pat: "hits always shock",
        val: SpecialVal::Mods(vec![r#mod(
            n!("EnemyShockChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2798,
    });
    v.push(PatternEntry {
        pat: "always freeze, shock and ignite",
        val: SpecialVal::Mods(vec![
            r#mod(n!("EnemyFreezeChance"), BASE, 100.0, None, 0, 0, &[]),
            r#mod(n!("EnemyShockChance"), BASE, 100.0, None, 0, 0, &[]),
            r#mod(n!("EnemyIgniteChance"), BASE, 100.0, None, 0, 0, &[]),
        ]),
        lua_line: 2799,
    });
    v.push(PatternEntry {
        pat: "all damage with hits can ignite",
        val: SpecialVal::Mods(vec![
            flag(n!("PhysicalCanIgnite"), None, 0, 0, &[]),
            flag(n!("ColdCanIgnite"), None, 0, 0, &[]),
            flag(n!("LightningCanIgnite"), None, 0, 0, &[]),
            flag(n!("ChaosCanIgnite"), None, 0, 0, &[]),
        ]),
        lua_line: 2804,
    });
    v.push(PatternEntry {
        pat: "all damage can ignite",
        val: SpecialVal::Mods(vec![
            flag(n!("PhysicalCanIgnite"), None, 0, 0, &[]),
            flag(n!("ColdCanIgnite"), None, 0, 0, &[]),
            flag(n!("LightningCanIgnite"), None, 0, 0, &[]),
            flag(n!("ChaosCanIgnite"), None, 0, 0, &[]),
        ]),
        lua_line: 2810,
    });
    v.push(PatternEntry {
        pat: "all damage with hits can chill",
        val: SpecialVal::Mods(vec![
            flag(n!("PhysicalCanChill"), None, 0, 0, &[]),
            flag(n!("FireCanChill"), None, 0, 0, &[]),
            flag(n!("LightningCanChill"), None, 0, 0, &[]),
            flag(n!("ChaosCanChill"), None, 0, 0, &[]),
        ]),
        lua_line: 2816,
    });
    v.push(PatternEntry {
        pat: "all damage with hits can shock",
        val: SpecialVal::Mods(vec![
            flag(n!("PhysicalCanShock"), None, 0, 0, &[]),
            flag(n!("FireCanShock"), None, 0, 0, &[]),
            flag(n!("ColdCanShock"), None, 0, 0, &[]),
            flag(n!("ChaosCanShock"), None, 0, 0, &[]),
        ]),
        lua_line: 2822,
    });
    v.push(PatternEntry {
        pat: "all damage can shock",
        val: SpecialVal::Mods(vec![
            flag(n!("PhysicalCanShock"), None, 0, 0, &[]),
            flag(n!("FireCanShock"), None, 0, 0, &[]),
            flag(n!("ColdCanShock"), None, 0, 0, &[]),
            flag(n!("ChaosCanShock"), None, 0, 0, &[]),
        ]),
        lua_line: 2828,
    });
    v.push(PatternEntry {
        pat: "other aegis skills are disabled",
        val: SpecialVal::Mods(vec![
            flag(
                n!("DisableSkill"),
                None,
                0,
                0,
                &[tag!({ type = SkillType, skillType = SkillType::Aegis })],
            ),
            flag(
                n!("EnableSkill"),
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillId = "Primal Aegis" })],
            ),
        ]),
        lua_line: 2834,
    });
    v.push(PatternEntry {
        pat: "primal aegis can take (%d+) elemental damage per allocated notable passive skill",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElementalAegisValue"),
                MAX,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = Multiplier, var = "AllocatedNotable" }),
                    tag!({ type = GlobalEffect, effectType = "Buff", unscalable = true }),
                ],
            )])
        }),
        lua_line: 2838,
    });
    v.push(PatternEntry {
        pat: "enemies chilled by your hits lessen their damage dealt by half of chill effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ChillEffectLessDamageDealt"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2839,
    });
    // Gladiator
    v.push(PatternEntry {
        pat: "chance to block spell damage is equal to chance to block attack damage",
        val: SpecialVal::Mods(vec![flag(
            n!("SpellBlockChanceIsBlockChance"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2841,
    });
    v.push(PatternEntry { pat: "maximum chance to block spell damage is equal to maximum chance to block attack damage", val: SpecialVal::Mods(vec![flag(n!("SpellBlockChanceMaxIsBlockChanceMax"), None, 0, 0, &[])]), lua_line: 2842 });
    v.push(PatternEntry {
        pat: "attack damage is lucky if you[' ]h?a?ve blocked in the past (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(
            n!("LuckyHits"),
            None,
            ModFlag::Attack,
            0,
            &[tag!({ type = Condition, var = "BlockedRecently" })],
        )]),
        lua_line: 2843,
    });
    v.push(PatternEntry { pat: "attack damage while dual wielding is lucky if you[' ]h?a?ve blocked in the past (%d+) seconds", val: SpecialVal::Mods(vec![flag(n!("LuckyHits"), None, ModFlag::Attack, 0, &[tag!({ type = Condition, var = "BlockedRecently" }), tag!({ type = Condition, var = "DualWielding" })])]), lua_line: 2846 });
    v.push(PatternEntry { pat: "hits ignore enemy monster physical damage reduction if you[' ]h?a?ve blocked in the past (%d+) seconds", val: SpecialVal::Mods(vec![flag(n!("IgnoreEnemyPhysicalDamageReduction"), None, 0, 0, &[tag!({ type = Condition, var = "BlockedRecently" })])]), lua_line: 2849 });
    v.push(PatternEntry {
        pat: "(%d+)%% more attack and movement speed per challenger charge",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Speed"),
                    MORE,
                    num,
                    None,
                    ModFlag::Attack,
                    0,
                    &[tag!({ type = Multiplier, var = "ChallengerCharge" })],
                ),
                r#mod(
                    n!("MovementSpeed"),
                    MORE,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = Multiplier, var = "ChallengerCharge" })],
                ),
            ])
        }),
        lua_line: 2852,
    });
    v.push(PatternEntry {
        pat: "gain (%d+)%% chance to block from equipped shield instead of the shield's value",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ReplaceShieldBlock"),
                OVERRIDE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "UsingShield" })],
            )])
        }),
        lua_line: 2856,
    });
    v.push(PatternEntry { pat: "deal (%d+)%% more damage with hits and ailments to rare and unique enemies for each second they've ever been in your presence, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = Multiplier, var = "EnemyPresenceSeconds", actor = "enemy", limit = tonumber(limit) }), tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])])
    }), lua_line: 2859 });
    // LUA-QUIRK: tag.div holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber (known diff MP3-1, calc doc §6)
    v.push(PatternEntry { pat: "deal (%d+)%% more damage with hits and ailments to rare and unique enemies for every (%d+) seconds they've ever been in your presence, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let div = ns(caps.cap[1], "index local 'div'");
        let limit = ns(caps.cap[2], "index local 'limit'");
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = Multiplier, var = "EnemyPresenceSeconds", actor = "enemy", limit = tonumber(limit), div = snum(div, "perform arithmetic on tag.div") }), tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })])])
    }), lua_line: 2862 });
    v.push(PatternEntry { pat: "(%d+)%% more damage with hits and ailments against enemies that are on low life while you are wielding an axe", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Damage"), MORE, num, None, 0, bor(KeywordFlag::Hit, KeywordFlag::Ailment), &[tag!({ type = ActorCondition, actor = "enemy", var = "LowLife" }), tag!({ type = Condition, var = "UsingAxe" })])])
    }), lua_line: 2865 });
    v.push(PatternEntry {
        pat: "retaliation skills have (%d+)%% increased speed",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                r#mod(
                    n!("Speed"),
                    INC,
                    num,
                    None,
                    0,
                    0,
                    &[tag!({ type = SkillType, skillType = SkillType::Retaliation })],
                ),
                r#mod(
                    n!("WarcrySpeed"),
                    INC,
                    num,
                    None,
                    0,
                    KeywordFlag::Warcry,
                    &[tag!({ type = SkillType, skillType = SkillType::Retaliation })],
                ),
            ])
        }),
        lua_line: 2868,
    });
    // Guardian
    v.push(PatternEntry {
        pat: "grants armour equal to (%d+)%% of your reserved life to you and nearby allies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GrantReservedLifeAsAura"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("Armour"),
                            BASE,
                            (nn(num, "perform arithmetic on local 'num'") / 100.0),
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
        lua_line: 2873,
    });
    v.push(PatternEntry {
        pat: "grants armour equal to (%d+)%% of your reserved mana to you and nearby allies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("GrantReservedManaAsAura"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(r#mod(
                            n!("Armour"),
                            BASE,
                            (nn(num, "perform arithmetic on local 'num'") / 100.0),
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
        lua_line: 2874,
    });
    v.push(PatternEntry { pat: "grants maximum energy shield equal to (%d+)%% of your reserved mana to you and nearby allies", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("GrantReservedManaAsAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("EnergyShield"), BASE, (nn(num, "perform arithmetic on local 'num'") / 100.0), None, 0, 0, &[])))]), None, 0, 0, &[])])
    }), lua_line: 2875 });
    v.push(PatternEntry {
        pat: "warcries cost no mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ManaCost"),
            MORE,
            -100.0,
            None,
            0,
            KeywordFlag::Warcry,
            &[],
        )]),
        lua_line: 2876,
    });
}

/// @pob Modules/ModParser.lua:2877-2961 h=a61eddd3
fn specialModList_2877(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "%+(%d+)%% chance to block attack damage for %d seconds? every %d seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BlockChance"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BastionOfHopeActive" })],
            )])
        }),
        lua_line: 2877,
    });
    v.push(PatternEntry {
        pat: "if you've blocked in the past %d+ seconds, you and nearby allies cannot be stunned",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ExtraAura"),
            LIST,
            vtab(
                vec![],
                vec![(
                    n!("mod"),
                    Value::from(flag(n!("StunImmune"), None, 0, 0, &[])),
                )],
            ),
            None,
            0,
            0,
            &[
                tag!({ type = Condition, var = "BlockedRecently" }),
                tag!({ type = GlobalEffect, effectType = "Global", unscalable = true }),
            ],
        )]),
        lua_line: 2878,
    });
    v.push(PatternEntry { pat: "if you've attacked recently, you and nearby allies have %+(%d+)%% chance to block attack damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("BlockChance"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "AttackedRecently" })])])
    }), lua_line: 2879 });
    v.push(PatternEntry { pat: "if you've cast a spell recently, you and nearby allies have %+(%d+)%% chance to block spell damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("SpellBlockChance"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "CastSpellRecently" })])])
    }), lua_line: 2880 });
    v.push(PatternEntry { pat: "while there is at least one nearby ally, you and nearby allies deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("Damage"), MORE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "NearbyAlly", threshold = 1.0 })])])
    }), lua_line: 2881 });
    v.push(PatternEntry {
        pat: "while there are at least five nearby allies, you and nearby allies have onslaught",
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
            &[tag!({ type = MultiplierThreshold, var = "NearbyAlly", threshold = 5.0 })],
        )]),
        lua_line: 2882,
    });
    v.push(PatternEntry { pat: "linked targets and allies in your link beams have %+(%d+)%% to all maximum elemental resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("onlyAllies"), Value::from(true)), (n!("mod"), Value::from(r#mod(n!("ElementalResistMax"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "AffectedByLink", neg = true })])))]), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "LinkedTargets", threshold = 1.0 })]), r#mod(n!("ExtraLinkEffect"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ElementalResistMax"), BASE, num, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })])))]), None, 0, 0, &[])])
    }), lua_line: 2883 });
    v.push(PatternEntry {
        pat: "enemies in your link beams cannot apply elemental ailments",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = ActorCondition, actor = "enemy", var = "BetweenYouAndLinkedTarget" })],
        )]),
        lua_line: 2887,
    });
    v.push(PatternEntry {
        pat:
            "(%d+)%% of damage from hits is taken from your sentinel of radiance's life before you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("takenFromRadianceSentinelBeforeYou"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2888,
    });
    v.push(PatternEntry {
        pat: "you can inflict %+(%d+) hallowing flame on enemies",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Multiplier:HallowingFlameMax"),
                BASE,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2889,
    });
    v.push(PatternEntry { pat: "gain (%d+)%% of (%a+) damage as extra (%a+) damage for each of your hallowing flames that have been removed by an allied hit recently, up to (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let fromType = ns(caps.cap[1], "index local 'fromType'");
        let destType = ns(caps.cap[2], "index local 'destType'");
        let limit = ns(caps.cap[3], "index local 'limit'");
        Some(vec![r#mod(cat!(gsub_fn(fromType, "^%l", |c| Some(upper(c[0].as_str())), None).0, "DamageGainAs", gsub_fn(destType, "^%l", |c| Some(upper(c[0].as_str())), None).0), BASE, num, None, 0, 0, &[tag!({ type = ActorCondition, actor = "enemy", var = "HallowingFlame" }), tag!({ type = Multiplier, var = "HallowingFlameStacksRemovedByAlly", limit = (nn(tonumber(limit), "perform arithmetic on a nil value") / nn(num, "perform arithmetic on local 'num'")) })])])
    }), lua_line: 2890 });
    v.push(PatternEntry {
        pat: "(%d+)%% increased magnitude of hallowing flame you inflict",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("HallowingFlameMagnitude"),
                INC,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2891,
    });
    // Hierophant
    v.push(PatternEntry {
        pat:
            "you and your totems regenerate ([%d%.]+)%% of life per second for each summoned totem",
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
                    &[tag!({ type = PerStat, stat = "TotemsSummoned" })],
                ),
                r#mod(
                    n!("LifeRegenPercent"),
                    BASE,
                    num,
                    None,
                    0,
                    KeywordFlag::Totem,
                    &[],
                ),
            ])
        }),
        lua_line: 2893,
    });
    v.push(PatternEntry {
        pat: "enemies take (%d+)%% increased damage for each of your brands attached to them",
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
                            &[tag!({ type = Multiplier, var = "BrandsAttached" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2897,
    });
    // hand-ported (lua2rs: local-type): `mods[i] = …` over an ipairs loop is a push
    v.push(PatternEntry {
        pat:
            "non%-damaging ailments have (%d+)%% reduced effect on you while you have arcane surge",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            // L2899
            let mut mods: Vec<Mod> = Vec::new();
            // L2900
            for ailment in &data().nonDamagingAilmentTypeList {
                // L2901
                mods.push(r#mod(
                    cat!("Self", ailment.as_str(), "Effect"),
                    INC,
                    -nn(num, "perform arithmetic on local 'num'"),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "AffectedByArcaneSurge" })],
                ));
            }
            // L2903
            Some(mods)
        }),
        lua_line: 2898,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments while you have arcane surge",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "AffectedByArcaneSurge" })],
        )]),
        lua_line: 2905,
    });
    v.push(PatternEntry {
        pat:
            "brands have (%d+)%% more activation frequency if (%d+)%% of attached duration expired",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("BrandActivationFrequency"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "BrandLastQuarter" })],
            )])
        }),
        lua_line: 2906,
    });
    v.push(PatternEntry {
        pat: "arcane surge a?l?s?o? ?grants (%d+)%% more spell damage to you",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ArcaneSurgeDamage"),
                MAX,
                num,
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2907,
    });
    // Inquisitor
    v.push(PatternEntry {
        pat: "critical strikes ignore enemy monster elemental resistances",
        val: SpecialVal::Mods(vec![flag(
            n!("IgnoreElementalResistances"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "CriticalStrike" })],
        )]),
        lua_line: 2909,
    });
    v.push(PatternEntry {
        pat: "non%-critical strikes penetrate (%d+)%% of enemy elemental resistances",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElementalPenetration"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "CriticalStrike", neg = true })],
            )])
        }),
        lua_line: 2910,
    });
    v.push(PatternEntry {
        pat: "consecrated ground you create applies (%d+)%% increased damage taken to enemies",
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
                            n!("DamageTakenConsecratedGround"),
                            INC,
                            num,
                            None,
                            0,
                            0,
                            &[tag!({ type = Condition, var = "OnConsecratedGround" })],
                        )),
                    )],
                ),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2911,
    });
    v.push(PatternEntry {
        pat: "you have consecrated ground around you while stationary",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:OnConsecratedGround"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Stationary" })],
        )]),
        lua_line: 2912,
    });
    v.push(PatternEntry { pat: "consecrated ground you create grants immun[ei]t?y? to elemental ailments to you and allies", val: SpecialVal::Mods(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("ElementalAilmentImmune"), None, 0, 0, &[tag!({ type = Condition, var = "OnConsecratedGround" })])))]), None, 0, 0, &[])]), lua_line: 2913 });
    v.push(PatternEntry {
        pat: "gain fanaticism for (%d+) seconds on reaching maximum fanatic charges",
        val: SpecialVal::Mods(vec![flag(
            n!("Condition:CanGainFanaticism"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2914,
    });
    v.push(PatternEntry { pat: "(%d+)%% increased critical strike chance per point of strength or intelligence, whichever is lower", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("CritChance"), INC, num, None, 0, 0, &[tag!({ type = PerStat, stat = "Str" }), tag!({ type = Condition, var = "IntHigherThanStr" })]), r#mod(n!("CritChance"), INC, num, None, 0, 0, &[tag!({ type = PerStat, stat = "Int" }), tag!({ type = Condition, neg = true, var = "IntHigherThanStr" })])])
    }), lua_line: 2917 });
    v.push(PatternEntry { pat: "consecrated ground you create causes life regeneration to also recover energy shield for you and allies", val: SpecialVal::Mods(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(flag(n!("LifeRegenerationRecoversEnergyShield"), None, 0, 0, &[tag!({ type = Condition, var = "OnConsecratedGround" })])))]), None, 0, 0, &[])]), lua_line: 2921 });
    // LUA-QUIRK: tag.limit holds a digits-only capture string in Lua; EvalMod only uses it in arithmetic, which coerces it like tonumber (known diff MP3-1, calc doc §6)
    v.push(PatternEntry { pat: "(%d+)%% more attack damage for each non%-instant spell you've cast in the past 8 seconds, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let max = ns(caps.cap[1], "index local 'max'");
        Some(vec![r#mod(n!("Damage"), MORE, num, None, ModFlag::Attack, 0, &[tag!({ type = Multiplier, var = "CastLast8Seconds", limit = snum(max, "perform arithmetic on tag.limit"), limitTotal = true })])])
    }), lua_line: 2922 });
    // Juggernaut
    v.push(PatternEntry {
        pat: "action speed cannot be modified to below base value",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinimumActionSpeed"),
            MAX,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 2926,
    });
    v.push(PatternEntry {
        pat: "movement speed cannot be modified to below base value",
        val: SpecialVal::Mods(vec![flag(
            n!("MovementSpeedCannotBeBelowBase"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 2927,
    });
    v.push(PatternEntry {
        pat: "you cannot be slowed to below base speed",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinimumActionSpeed"),
            MAX,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 2928,
    });
    v.push(PatternEntry {
        pat: "cannot be slowed to below base speed",
        val: SpecialVal::Mods(vec![r#mod(
            n!("MinimumActionSpeed"),
            MAX,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 2929,
    });
    v.push(PatternEntry {
        pat: "gain accuracy rating equal to your strength",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Accuracy"),
            BASE,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = PerStat, stat = "Str" })],
        )]),
        lua_line: 2930,
    });
    v.push(PatternEntry {
        pat: "gain accuracy rating equal to twice your strength",
        val: SpecialVal::Mods(vec![r#mod(
            n!("Accuracy"),
            BASE,
            2.0,
            None,
            0,
            0,
            &[tag!({ type = PerStat, stat = "Str" })],
        )]),
        lua_line: 2931,
    });
    // Necromancer
    v.push(PatternEntry { pat: "your offering skills also affect you", val: SpecialVal::Mods(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("SkillData"), LIST, vtab(vec![], vec![(n!("key"), Value::from("buffNotPlayer")), (n!("value"), Value::from(false))]), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Bone Offering"), n!("Flesh Offering"), n!("Spirit Offering"), n!("Blood Offering")][..] })])]), lua_line: 2933 });
    v.push(PatternEntry { pat: "your offerings have (%d+)%% reduced effect on you", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("BuffEffectOnPlayer"), INC, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Bone Offering"), n!("Flesh Offering"), n!("Spirit Offering"), n!("Blood Offering")][..] })])])
    }), lua_line: 2934 });
    v.push(PatternEntry { pat: "your offerings have (%d+)%% increased effect on you", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ExtraSkillMod"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("BuffEffectOnPlayer"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Bone Offering"), n!("Flesh Offering"), n!("Spirit Offering"), n!("Blood Offering")][..] })])])
    }), lua_line: 2935 });
    v.push(PatternEntry { pat: "if you've consumed a corpse recently, you and your minions have (%d+)%% increased area of effect", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("AreaOfEffect"), INC, num, None, 0, 0, &[tag!({ type = Condition, var = "ConsumedCorpseRecently" })]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("AreaOfEffect"), INC, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "ConsumedCorpseRecently" })])])
    }), lua_line: 2936 });
    v.push(PatternEntry {
        pat: "with at least one nearby corpse, you and nearby allies deal (%d+)%% more damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ExtraAura"),
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
                &[tag!({ type = MultiplierThreshold, var = "NearbyCorpse", threshold = 1.0 })],
            )])
        }),
        lua_line: 2937,
    });
    v.push(PatternEntry {
        pat: "with at least one nearby corpse, nearby enemies deal (%d+)%% reduced damage",
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
                            n!("Damage"),
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
                &[tag!({ type = MultiplierThreshold, var = "NearbyCorpse", threshold = 1.0 })],
            )])
        }),
        lua_line: 2938,
    });
    v.push(PatternEntry { pat: "for each nearby corpse, you and nearby allies regenerate ([%d%.]+)%% of energy shield per second, up to ([%d%.]+)%% per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("EnergyShieldRegenPercent"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Multiplier, var = "NearbyCorpse", limit = tonumber(limit), limitTotal = true })])])
    }), lua_line: 2939 });
    v.push(PatternEntry { pat: "for each nearby corpse, you and nearby allies regenerate (%d+) mana per second, up to (%d+) per second", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("ExtraAura"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ManaRegen"), BASE, num, None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Multiplier, var = "NearbyCorpse", limit = tonumber(limit), limitTotal = true })])])
    }), lua_line: 2940 });
    v.push(PatternEntry { pat: "(%d+)%% increased attack and cast speed for each corpse consumed recently, up to a maximum of (%d+)%%", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        Some(vec![r#mod(n!("Speed"), INC, num, None, 0, 0, &[tag!({ type = Multiplier, var = "CorpseConsumedRecently", limit = (snum(limit, "perform arithmetic on a string") / nn(num, "perform arithmetic on local 'num'")) })])])
    }), lua_line: 2941 });
    v.push(PatternEntry {
        pat: "enemies near corpses you spawned recently are chilled and shocked",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyModifier"),
                LIST,
                vtab(
                    vec![],
                    vec![(
                        n!("mod"),
                        Value::from(flag(n!("Condition:Chilled"), None, 0, 0, &[])),
                    )],
                ),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "SpawnedCorpseRecently" })],
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
                &[tag!({ type = Condition, var = "SpawnedCorpseRecently" })],
            ),
            r#mod(
                n!("ChillBase"),
                BASE,
                data().nonDamagingAilment[&n!("Chill")].default,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "SpawnedCorpseRecently" })],
            ),
            r#mod(
                n!("ShockBase"),
                BASE,
                data().nonDamagingAilment[&n!("Shock")].default,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "SpawnedCorpseRecently" })],
            ),
        ]),
        lua_line: 2942,
    });
    v.push(PatternEntry {
        pat: "regenerate (%d+)%% of energy shield over 2 seconds when you consume a corpse",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("EnergyShieldRegenPercent"),
                BASE,
                (nn(num, "perform arithmetic on local 'num'") / 2.0),
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "ConsumedCorpseInPast2Sec" })],
            )])
        }),
        lua_line: 2948,
    });
    v.push(PatternEntry { pat: "regenerate (%d+)%% of mana over 2 seconds when you consume a corpse", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ManaRegen"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "Mana", percent = (nn(num, "perform arithmetic on local 'num'") / 2.0) }), tag!({ type = Condition, var = "ConsumedCorpseInPast2Sec" })])])
    }), lua_line: 2949 });
    v.push(PatternEntry {
        pat: "corpses you spawn have (%d+)%% increased maximum life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("CorpseLife"), INC, num, None, 0, 0, &[])])
        }),
        lua_line: 2950,
    });
    v.push(PatternEntry {
        pat: "corpses you spawn have (%d+)%% reduced maximum life",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("CorpseLife"),
                INC,
                (-nn(num, "perform arithmetic on local 'num'")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2951,
    });
    v.push(PatternEntry { pat: "minions gain added physical damage equal to (%d+)%% of maximum energy shield on your equipped helmet", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("PhysicalMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShieldOnHelmet", actor = "parent", percent = num })])))]), None, 0, 0, &[]), r#mod(n!("MinionModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("PhysicalMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "EnergyShieldOnHelmet", actor = "parent", percent = num })])))]), None, 0, 0, &[])])
    }), lua_line: 2952 });
    // Occultist
    v.push(PatternEntry { pat: "when you kill an enemy, for each curse on that enemy, gain (%d+)%% of non%-chaos damage as extra chaos damage for 4 seconds", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("NonChaosDamageGainAsChaos"), BASE, num, None, 0, 0, &[tag!({ type = Condition, var = "KilledRecently" }), tag!({ type = Multiplier, var = "CurseOnEnemy" })])])
    }), lua_line: 2958 });
    v.push(PatternEntry {
        pat: "cannot be stunned while you have energy shield",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "HaveEnergyShield" })],
        )]),
        lua_line: 2961,
    });
}

/// @pob Modules/ModParser.lua:2962-3052 h=b2b43698
fn specialModList_2962(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry {
        pat: "every second, inflict withered on nearby enemies for (%d+) seconds",
        val: SpecialVal::Mods(vec![flag(n!("Condition:CanWither"), None, 0, 0, &[])]),
        lua_line: 2962,
    });
    v.push(PatternEntry {
        pat: "nearby hindered enemies deal (%d+)%% reduced damage over time",
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
                            n!("DamageOverTime"),
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
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Hindered" })],
            )])
        }),
        lua_line: 2963,
    });
    v.push(PatternEntry {
        pat: "nearby chilled enemies deal (%d+)%% reduced damage with hits",
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
                            n!("Damage"),
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
                &[tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })],
            )])
        }),
        lua_line: 2964,
    });
    v.push(PatternEntry {
        pat: "gain spirit infusion every ?(%d?)%.?(%d?) seconds? while channelling a spell",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![flag(
                n!("Condition:CanGainSpiritInfusion"),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 2965,
    });
    // Pathfinder
    v.push(PatternEntry {
        pat: "always poison on hit while using a flask",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PoisonChance"),
            BASE,
            100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 2967,
    });
    v.push(PatternEntry { pat: "poisons you inflict during any flask effect have (%d+)%% chance to deal (%d+)%% more damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let more = ns(caps.cap[1], "index local 'more'");
        Some(vec![r#mod(n!("Damage"), MORE, ((nn(tonumber(more), "perform arithmetic on a nil value") * nn(num, "perform arithmetic on local 'num'")) / 100.0), None, 0, KeywordFlag::Poison, &[tag!({ type = Condition, var = "UsingFlask" })])])
    }), lua_line: 2968 });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments during any flask effect",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "UsingFlask" })],
        )]),
        lua_line: 2969,
    });
    v.push(PatternEntry { pat: "grant bonuses to non%-channelling skills you use by consuming (%d+) charges from a flask of each of the following types, if possible:", val: SpecialVal::Mods(vec![]), lua_line: 2970 });
    v.push(PatternEntry {
        pat: "if diamond flask charges are consumed, (%d+)%% increased critical strike chance",
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
                    tag!({ type = SkillType, skillType = SkillType::Triggered, neg = true }),
                    tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }),
                    tag!({ type = Condition, var = "HaveDiamondFlask" }),
                ],
            )])
        }),
        lua_line: 2971,
    });
    v.push(PatternEntry {
        pat: "if bismuth flask charges are consumed, penetrate (%d+)%% elemental resistances",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElementalPenetration"),
                BASE,
                num,
                None,
                0,
                0,
                &[
                    tag!({ type = SkillType, skillType = SkillType::Triggered, neg = true }),
                    tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }),
                    tag!({ type = Condition, var = "HaveBismuthFlask" }),
                ],
            )])
        }),
        lua_line: 2974,
    });
    v.push(PatternEntry { pat: "if amethyst flask charges are consumed, (%d+)%% of physical damage as extra chaos damage", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PhysicalDamageGainAsChaos"), BASE, num, None, 0, 0, &[tag!({ type = SkillType, skillType = SkillType::Triggered, neg = true }), tag!({ type = SkillType, skillType = SkillType::Channel, neg = true }), tag!({ type = Condition, var = "HaveAmethystFlask" })])])
    }), lua_line: 2977 });
    // Raider
    v.push(PatternEntry {
        pat: "nearby enemies have (%d+)%% less accuracy rating while you have phasing",
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
                &[tag!({ type = Condition, var = "Phasing" })],
            )])
        }),
        lua_line: 2981,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to elemental ailments while phasing",
        val: SpecialVal::Mods(vec![flag(
            n!("ElementalAilmentImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Phasing" })],
        )]),
        lua_line: 2982,
    });
    v.push(PatternEntry { pat: "nearby enemies have fire, cold and lightning exposure while you have phasing, applying %-(%d+)%% to those resistances", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("FireExposure"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Phasing" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("ColdExposure"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Phasing" })]), r#mod(n!("EnemyModifier"), LIST, vtab(vec![], vec![(n!("mod"), Value::from(r#mod(n!("LightningExposure"), BASE, (-nn(num, "perform arithmetic on local 'num'")), None, 0, 0, &[])))]), None, 0, 0, &[tag!({ type = Condition, var = "Phasing" })])])
    }), lua_line: 2983 });
    v.push(PatternEntry {
        pat: "nearby enemies have fire, cold and lightning exposure while you have phasing",
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
                &[tag!({ type = Condition, var = "Phasing" })],
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
                &[tag!({ type = Condition, var = "Phasing" })],
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
                &[tag!({ type = Condition, var = "Phasing" })],
            ),
        ]),
        lua_line: 2988,
    });
    // Saboteur
    v.push(PatternEntry {
        pat: "hits have (%d+)%% chance to deal (%d+)%% more area damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let more = ns(caps.cap[1], "index local 'more'");
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                ((nn(num, "perform arithmetic on local 'num'")
                    * snum(more, "perform arithmetic on a string"))
                    / 100.0),
                None,
                bor(ModFlag::Area, ModFlag::Hit),
                0,
                &[],
            )])
        }),
        lua_line: 2994,
    });
    v.push(PatternEntry {
        pat: "immun[ei]t?y? to ignite and shock",
        val: SpecialVal::Mods(vec![
            flag(n!("IgniteImmune"), None, 0, 0, &[]),
            flag(n!("ShockImmune"), None, 0, 0, &[]),
        ]),
        lua_line: 2997,
    });
    v.push(PatternEntry {
        pat: "you gain (%d+)%% increased damage for each trap",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("Damage"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "ActiveTrapLimit" })],
            )])
        }),
        lua_line: 3001,
    });
    v.push(PatternEntry {
        pat: "you gain (%d+)%% increased area of effect for each mine",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("AreaOfEffect"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "ActiveMineLimit" })],
            )])
        }),
        lua_line: 3002,
    });
    v.push(PatternEntry {
        pat: "triggers level (%d+) summon triggerbots when allocated",
        val: SpecialVal::Mods(vec![flag(n!("HaveTriggerBots"), None, 0, 0, &[])]),
        lua_line: 3003,
    });
    // Slayer
    // hand-ported (lua2rs: tag-field-ramp): `ramp = {1,0}` is a flat ramp list
    v.push(PatternEntry {
        pat: "deal up to (%d+)%% more melee damage to enemies, based on proximity",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let mut proximity = tag!({ type = MeleeProximity });
            proximity.ramp = Some(Ramp::Flat(Arc::from(&[1.0, 0.0][..])));
            Some(vec![r#mod(
                n!("Damage"),
                MORE,
                num,
                None,
                bor(ModFlag::Attack, ModFlag::Melee),
                0,
                &[proximity],
            )])
        }),
        lua_line: 3005,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned while leeching",
        val: SpecialVal::Mods(vec![flag(
            n!("StunImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Leeching" })],
        )]),
        lua_line: 3006,
    });
    v.push(PatternEntry {
        pat: "you are immune to bleeding while leeching",
        val: SpecialVal::Mods(vec![flag(
            n!("BleedImmune"),
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Leeching" })],
        )]),
        lua_line: 3007,
    });
    v.push(PatternEntry {
        pat: "life leech effects are not removed at full life",
        val: SpecialVal::Mods(vec![flag(n!("CanLeechLifeOnFullLife"), None, 0, 0, &[])]),
        lua_line: 3008,
    });
    v.push(PatternEntry {
        pat: "life leech effects are not removed when unreserved life is filled",
        val: SpecialVal::Mods(vec![flag(n!("CanLeechLifeOnFullLife"), None, 0, 0, &[])]),
        lua_line: 3009,
    });
    v.push(PatternEntry {
        pat: "energy shield leech effects from attacks are not removed at full energy shield",
        val: SpecialVal::Mods(vec![flag(
            n!("CanLeechEnergyShieldOnFullEnergyShield"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3010,
    });
    v.push(PatternEntry {
        pat: "cannot take reflected physical damage",
        val: SpecialVal::Mods(vec![r#mod(
            n!("PhysicalReflectedDamageTaken"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = GlobalEffect, effectType = "Global", unscalable = true })],
        )]),
        lua_line: 3011,
    });
    v.push(PatternEntry {
        pat: "gain (%d+)%% increased movement speed for 20 seconds when you kill an enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("MovementSpeed"),
                INC,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "KilledRecently" })],
            )])
        }),
        lua_line: 3012,
    });
    v.push(PatternEntry { pat: "gain (%d+)%% increased attack speed for 20 seconds when you kill a rare or unique enemy", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Speed"), INC, num, None, ModFlag::Attack, 0, &[tag!({ type = Condition, var = "KilledUniqueEnemy" })])])
    }), lua_line: 3013 });
    v.push(PatternEntry {
        pat: "kill enemies that have (%d+)%% or lower life when hit by your skills",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("CullPercent"), MAX, num, None, 0, 0, &[])])
        }),
        lua_line: 3014,
    });
    v.push(PatternEntry {
        pat: "you are unaffected by bleeding while leeching",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfBleedEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[tag!({ type = Condition, var = "Leeching" })],
        )]),
        lua_line: 3015,
    });
    // Trickster
    v.push(PatternEntry {
        pat: "(%d+)%% chance to gain (%d+)%% of non%-chaos damage with hits as extra chaos damage",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let perc = ns(caps.cap[1], "index local 'perc'");
            Some(vec![r#mod(
                n!("NonChaosDamageGainAsChaos"),
                BASE,
                ((nn(num, "perform arithmetic on local 'num'") / 100.0)
                    * nn(tonumber(perc), "perform arithmetic on a nil value")),
                None,
                0,
                0,
                &[],
            )])
        }),
        lua_line: 3017,
    });
    v.push(PatternEntry {
        pat: "movement skills cost no mana",
        val: SpecialVal::Mods(vec![r#mod(
            n!("ManaCost"),
            MORE,
            -100.0,
            None,
            0,
            KeywordFlag::Movement,
            &[],
        )]),
        lua_line: 3018,
    });
    v.push(PatternEntry {
        pat: "cannot be stunned while you have ghost shrouds",
        val: SpecialVal::Fn(|caps: &Caps| {
            Some(vec![flag(
                n!("StunImmune"),
                None,
                0,
                0,
                &[tag!({ type = MultiplierThreshold, var = "GhostShroud", threshold = 1.0 })],
            )])
        }),
        lua_line: 3019,
    });
    v.push(PatternEntry {
        pat: "your action speed is at least (%d+)%% of base value",
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
        lua_line: 3020,
    });
    v.push(PatternEntry {
        pat: "nearby enemy monsters' action speed is at most (%d+)%% of base value",
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
                            n!("MaximumActionSpeedReduction"),
                            MAX,
                            (100.0 - nn(num, "perform arithmetic on local 'num'")),
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
        lua_line: 3021,
    });
    v.push(PatternEntry {
        pat: "prevent %+(%d+)%% of suppressed spell damage while on full energy shield",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SpellSuppressionEffect"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "FullEnergyShield" })],
            )])
        }),
        lua_line: 3022,
    });
    v.push(PatternEntry {
        pat: "energy shield leech effects are not removed when energy shield is filled",
        val: SpecialVal::Mods(vec![flag(
            n!("CanLeechEnergyShieldOnFullEnergyShield"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3023,
    });
    v.push(PatternEntry {
        pat: "take (%d+)%% less damage from hits for (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let duration = ns(caps.cap[1], "index local 'duration'");
            Some(vec![
                r#mod(
                    n!("DamageTakenWhenHit"),
                    MORE,
                    (-nn(num, "perform arithmetic on local 'num'")),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "HeartstopperHIT" })],
                ),
                r#mod(
                    n!("DamageTakenWhenHit"),
                    MORE,
                    (((-nn(num, "perform arithmetic on local 'num'"))
                        * nn(tonumber(duration), "perform arithmetic on a nil value"))
                        / 10.0),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "HeartstopperAVERAGE" })],
                ),
            ])
        }),
        lua_line: 3024,
    });
    v.push(PatternEntry {
        pat: "take (%d+)%% less damage over time for (%d+) seconds",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            let duration = ns(caps.cap[1], "index local 'duration'");
            Some(vec![
                r#mod(
                    n!("DamageTakenOverTime"),
                    MORE,
                    (-nn(num, "perform arithmetic on local 'num'")),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "HeartstopperDOT" })],
                ),
                r#mod(
                    n!("DamageTakenOverTime"),
                    MORE,
                    (((-nn(num, "perform arithmetic on local 'num'"))
                        * nn(tonumber(duration), "perform arithmetic on a nil value"))
                        / 10.0),
                    None,
                    0,
                    0,
                    &[tag!({ type = Condition, var = "HeartstopperAVERAGE" })],
                ),
            ])
        }),
        lua_line: 3028,
    });
    // Warden
    v.push(PatternEntry {
        pat: "prevent %+(%d+)%% of suppressed spell damage per bark below maximum",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("SpellSuppressionEffect"),
                BASE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Multiplier, var = "MissingBarkskinStacks" })],
            )])
        }),
        lua_line: 3033,
    });
    v.push(PatternEntry {
        pat: "hits that would ignite instead scorch",
        val: SpecialVal::Mods(vec![
            flag(n!("IgniteCanScorch"), None, 0, 0, &[]),
            flag(n!("CannotIgnite"), None, 0, 0, &[]),
        ]),
        lua_line: 3036,
    });
    v.push(PatternEntry {
        pat: "you can inflict an additional scorch on each enemy",
        val: SpecialVal::Mods(vec![
            flag(n!("ScorchCanStack"), None, 0, 0, &[]),
            r#mod(n!("ScorchStacksMax"), BASE, 1.0, None, 0, 0, &[]),
        ]),
        lua_line: 3037,
    });
    v.push(PatternEntry {
        pat: "maximum effect of shock is (%d+)%% increased damage taken",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(n!("ShockMax"), OVERRIDE, num, None, 0, 0, &[])])
        }),
        lua_line: 3038,
    });
    v.push(PatternEntry {
        pat: "you can apply up to (%d+) shocks to each enemy",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![
                flag(n!("ShockCanStack"), None, 0, 0, &[]),
                r#mod(n!("ShockStacksMax"), OVERRIDE, num, None, 0, 0, &[]),
            ])
        }),
        lua_line: 3039,
    });
    v.push(PatternEntry {
        pat: "hits that fail to freeze due to insufficient freeze duration inflict hoarfrost",
        val: SpecialVal::Mods(vec![flag(n!("HitsCanInflictHoarfrost"), None, 0, 0, &[])]),
        lua_line: 3040,
    });
    v.push(PatternEntry {
        pat: "your hits always inflict freeze, shock and ignite while unbound",
        val: SpecialVal::Mods(vec![
            r#mod(
                n!("EnemyFreezeChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Unbound" })],
            ),
            r#mod(
                n!("EnemyShockChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Unbound" })],
            ),
            r#mod(
                n!("EnemyIgniteChance"),
                BASE,
                100.0,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Unbound" })],
            ),
        ]),
        lua_line: 3041,
    });
    v.push(PatternEntry {
        pat: "(%d+)%% more elemental damage while unbound",
        val: SpecialVal::Fn(|caps: &Caps| {
            let num = caps.num;
            Some(vec![r#mod(
                n!("ElementalDamage"),
                MORE,
                num,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = "Unbound" })],
            )])
        }),
        lua_line: 3046,
    });
    // Warden (Affliction)
    v.push(PatternEntry { pat: "defences from equipped body armour are doubled if it has no socketed gems", val: SpecialVal::Mods(vec![r#mod(n!("Defences"), MORE, 100.0, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SocketedGemsInBody Armour", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingBody Armour" }), tag!({ type = SlotName, slotName = "Body Armour" }), tag!({ type = Multiplier, var = "OathOfTheMajiDoubled", globalLimit = 100.0, globalLimitKey = "OathOfTheMajiLimit" })]), r#mod(n!("Multiplier:OathOfTheMajiDoubled"), OVERRIDE, 1.0, None, 0, 0, &[tag!({ type = SlotName, slotName = "Body Armour" })])]), lua_line: 3048 });
    v.push(PatternEntry { pat: "([%+%-]%d+)%% to all elemental resistances if you have an equipped helmet with no socketed gems", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("ElementalResist"), BASE, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SocketedGemsInHelmet", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingHelmet" })])])
    }), lua_line: 3052 });
}

/// @pob Modules/ModParser.lua:3053-3083 h=f887d1e1
fn specialModList_3053(v: &mut Vec<PatternEntry<SpecialVal>>) {
    v.push(PatternEntry { pat: "(%d+)%% increased maximum life if you have equipped gloves with no socketed gems", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("Life"), INC, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SocketedGemsInGloves", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingGloves" })])])
    }), lua_line: 3053 });
    v.push(PatternEntry { pat: "(%d+)%% increased movement speed if you have equipped boots with no socketed gems", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("MovementSpeed"), INC, num, None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "SocketedGemsInBoots", threshold = 0.0, upper = true }), tag!({ type = Condition, var = "UsingBoots" })])])
    }), lua_line: 3054 });
    // Warlock
    v.push(PatternEntry { pat: "spells you cast yourself gain added physical damage equal to (%d+)%% of life cost, if life cost is not higher than the maximum you could spend", val: SpecialVal::Fn(|caps: &Caps| {
        let num = caps.num;
        Some(vec![r#mod(n!("PhysicalMin"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "LifeCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "LifeUnreserved", thresholdStat = "LifeCost" })]), r#mod(n!("PhysicalMax"), BASE, 1.0, None, 0, 0, &[tag!({ type = PercentStat, stat = "LifeCost", percent = num, floor = true }), tag!({ type = StatThreshold, stat = "LifeUnreserved", thresholdStat = "LifeCost" })])])
    }), lua_line: 3056 });
    v.push(PatternEntry {
        pat: "gain maximum life instead of maximum energy shield from equipped armour items",
        val: SpecialVal::Mods(vec![flag(n!("ConvertArmourESToLife"), None, 0, 0, &[])]),
        lua_line: 3060,
    });
    // Ritualist Bloodline
    v.push(PatternEntry {
        pat: "unaffected by bleeding",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfBleedEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3062,
    });
    v.push(PatternEntry {
        pat: "unaffected by poison",
        val: SpecialVal::Mods(vec![r#mod(
            n!("SelfPoisonEffect"),
            MORE,
            -100.0,
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3063,
    });
    v.push(PatternEntry {
        pat: "can't use amulets",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CanNotUseItem"),
            Flag,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = DisablesItem, slotName = "Amulet" })],
        )]),
        lua_line: 3064,
    });
    v.push(PatternEntry {
        pat: "can't use belts",
        val: SpecialVal::Mods(vec![r#mod(
            n!("CanNotUseItem"),
            Flag,
            1.0,
            None,
            0,
            0,
            &[tag!({ type = DisablesItem, slotName = "Belt" })],
        )]),
        lua_line: 3065,
    });
    v.push(PatternEntry {
        pat: "%+1 ring slot",
        val: SpecialVal::Mods(vec![flag(n!("AdditionalRingSlot"), None, 0, 0, &[])]),
        lua_line: 3066,
    });
    v.push(PatternEntry {
        pat: "utility flasks are disabled",
        val: SpecialVal::Mods(vec![flag(
            n!("UtilityFlasksDoNotApplyToPlayer"),
            None,
            0,
            0,
            &[],
        )]),
        lua_line: 3067,
    });
    // Abyssal Bloodline
    v.push(PatternEntry {
        pat: "totems have (%d+)%% more life per maximum number of summoned totems",
        val: SpecialVal::Fn(|caps: &Caps| {
            let n = caps.num;
            Some(vec![r#mod(
                n!("TotemLife"),
                MORE,
                n,
                None,
                0,
                0,
                &[tag!({ type = PerStat, stat = "ActiveTotemLimit" })],
            )])
        }),
        lua_line: 3069,
    });
    v.push(PatternEntry { pat: "while you have at least (%d+) hypnotic eye jewels socketed: arcane surge also grants (%d+)%% of damage taken recouped as mana to you arcane surge also grants (%d+)%% increased mana cost efficiency to you", val: SpecialVal::Fn(|caps: &Caps| {
        let jewels = caps.num;
        let recoup = ns(caps.cap[1], "index local 'recoup'");
        let costEfficiency = ns(caps.cap[2], "index local 'costEfficiency'");
        Some(vec![r#mod(n!("ArcaneSurgeAlsoManaRecoup"), BASE, tonumber(recoup), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "HypnoticEyeJewel", threshold = jewels })]), r#mod(n!("ArcaneSurgeAlsoManaCostEfficiency"), INC, tonumber(costEfficiency), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "HypnoticEyeJewel", threshold = jewels })])])
    }), lua_line: 3072 });
    v.push(PatternEntry { pat: "while you have at least (%d+) ghastly eye jewels socketed: unholy might you grant also causes target's damage to penetrate (%d+)%% chaos resistance unholy might you grant also applies (%d+)%% increased effect of withered", val: SpecialVal::Fn(|caps: &Caps| {
        let jewels = caps.num;
        let pen = ns(caps.cap[1], "index local 'pen'");
        let wither = ns(caps.cap[2], "index local 'wither'");
        Some(vec![r#mod(n!("UnholyMightAlsoChaosPenetration"), BASE, tonumber(pen), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "GhastlyEyeJewel", threshold = jewels })]), r#mod(n!("UnholyMightAlsoWitherEffect"), INC, tonumber(wither), None, 0, 0, &[tag!({ type = MultiplierThreshold, var = "GhastlyEyeJewel", threshold = jewels })])])
    }), lua_line: 3078 });
}

/// Pushes the specialModList entries of Modules/ModParser.lua:2130-3083 (413 entries) in source order.
pub fn push_entries(v: &mut Vec<PatternEntry<SpecialVal>>) {
    specialModList_2132(v);
    specialModList_2241(v);
    specialModList_2344(v);
    specialModList_2454(v);
    specialModList_2571(v);
    specialModList_2675(v);
    specialModList_2877(v);
    specialModList_2962(v);
    specialModList_3053(v);
}

/// The entries of ModParser.lua 2130-3083, in source order (413 entries; the per-block
/// `@pob` markers above cover the range).
pub fn entries() -> Vec<PatternEntry<SpecialVal>> {
    let mut v: Vec<PatternEntry<SpecialVal>> = Vec::with_capacity(413);
    push_entries(&mut v);
    v
}
