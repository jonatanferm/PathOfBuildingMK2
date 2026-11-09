//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 1320-2009: modTagList: per-charge, conditional and threshold tags (136 closures).
//!
//! Owner: MP2. The entries were translated by scripts/port/lua2rs (unit T1) and reviewed
//! against the Lua; the four closures lua2rs could not translate (1401, 1596, 2001, 2007)
//! are hand-ported (marked "hand-ported").
//! One `PatternEntry` per Lua key, in source order; the duplicated key ("when you warcry",
//! 1853/1858) keeps its last value and line (Lua table constructor semantics, `LUA-QUIRK`).
//! Closure parameters follow the modTagList call shape (ModParser.lua:6698-6712; calc doc §6 /
//! CCR-0006 §5): #1 = `caps.num` when `caps.cap[0]` holds a digit, else `caps.cap[0]`;
//! #k≥2 = `caps.cap[k-2]`.
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    unused_parens,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::types::*;
use pob_gamedata::{tag, KeywordFlag, ModFlag, ModType, Name, SkillType, Tag, Value, ValueTable};
use pob_lua::bit::bor;
use pob_lua::pattern::{gsub_fn, r#match};
use pob_lua::string::{firstToUpper, len, sub, upper};
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

/// The first parameter of a modTagList closure (ModParser.lua:6698-6702, CCR-0006 §5):
/// `tonumber(tagCap[1])` when `tagCap[1]:match("%d+")` (nil when that fails), else the string
/// `tagCap[1]` (Rust-only plumbing for the closures whose first capture may be either).
enum Arg1<'a> {
    Num(Option<f64>),
    Str(&'a str),
}

fn arg1<'a>(caps: &Caps<'a>) -> Arg1<'a> {
    let Some(c1) = caps.cap[0] else {
        // the driver indexes tagCap[1] before the call
        lua_error!("attempt to index field '?' (a nil value)");
    };
    if r#match(c1, "%d+", None).is_some() {
        Arg1::Num(caps.num)
    } else {
        Arg1::Str(c1)
    }
}

/// [`arg1`] indexed as a string (`param:gsub(…)`): a number or nil raises.
fn arg1_str<'a>(caps: &Caps<'a>, param: &str) -> &'a str {
    match arg1(caps) {
        Arg1::Str(s) => s,
        Arg1::Num(None) => lua_error!("attempt to index local '{}' (a nil value)", param),
        Arg1::Num(Some(_)) => lua_error!("attempt to index local '{}' (a number value)", param),
    }
}

/// Lua `tonumber(v)` of a string-or-nil capture: `tonumber(nil)` raises.
fn lua_tonumber(v: Option<&str>) -> Option<f64> {
    match v {
        Some(s) => tonumber(s),
        None => lua_error!("bad argument #1 to 'tonumber' (value expected)"),
    }
}

/// @pob Modules/ModParser.lua:1320-1371 h=710407b0
fn modTagList_1321(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "on enemies",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1321,
    });
    v.push(PatternEntry {
        pat: "while active",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1322,
    });
    v.push(PatternEntry {
        pat: "for (%d+) seconds",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1323,
    });
    v.push(PatternEntry {
        pat: "when you hit a unique enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })),
            ..Default::default()
        }),
        lua_line: 1324,
    });
    v.push(PatternEntry {
        pat: " on critical strike",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CriticalStrike" })),
            ..Default::default()
        }),
        lua_line: 1325,
    });
    v.push(PatternEntry {
        pat: "from critical strikes",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CriticalStrike" })),
            ..Default::default()
        }),
        lua_line: 1326,
    });
    v.push(PatternEntry {
        pat: "with critical strikes",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CriticalStrike" })),
            ..Default::default()
        }),
        lua_line: 1327,
    });
    v.push(PatternEntry {
        pat: "by enemies killed with a critical strike",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "CritRecently" }),
                tag!({ type = Condition, var = "KilledRecently" }),
            ],
            ..Default::default()
        }),
        lua_line: 1328,
    });
    v.push(PatternEntry {
        pat: "while affected by auras you cast",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByAura" })),
            ..Default::default()
        }),
        lua_line: 1329,
    });
    v.push(PatternEntry {
        pat: "for you and nearby allies",
        val: TagVal::Spec(ModSpec {
            newAura: true,
            ..Default::default()
        }),
        lua_line: 1330,
    });
    v.push(PatternEntry {
        pat: "to you and allies",
        val: TagVal::Spec(ModSpec {
            newAura: true,
            ..Default::default()
        }),
        lua_line: 1331,
    });
    // Multipliers
    v.push(PatternEntry {
        pat: "per power charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "PowerCharge" })),
            ..Default::default()
        }),
        lua_line: 1333,
    });
    v.push(PatternEntry {
        pat: "per frenzy charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "FrenzyCharge" })),
            ..Default::default()
        }),
        lua_line: 1334,
    });
    v.push(PatternEntry {
        pat: "per endurance charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "EnduranceCharge" })),
            ..Default::default()
        }),
        lua_line: 1335,
    });
    v.push(PatternEntry {
        pat: "per brine charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BrineCharge" })),
            ..Default::default()
        }),
        lua_line: 1336,
    });
    v.push(PatternEntry {
        pat: "per siphoning charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SiphoningCharge" })),
            ..Default::default()
        }),
        lua_line: 1337,
    });
    v.push(PatternEntry {
        pat: "per spirit charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SpiritCharge" })),
            ..Default::default()
        }),
        lua_line: 1338,
    });
    v.push(PatternEntry {
        pat: "per challenger charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ChallengerCharge" })),
            ..Default::default()
        }),
        lua_line: 1339,
    });
    v.push(PatternEntry {
        pat: "per maximum power charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "PowerChargeMax" })),
            ..Default::default()
        }),
        lua_line: 1340,
    });
    v.push(PatternEntry {
        pat: "per gale force",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GaleForce" })),
            ..Default::default()
        }),
        lua_line: 1341,
    });
    v.push(PatternEntry {
        pat: "per intensity",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Intensity" })),
            ..Default::default()
        }),
        lua_line: 1342,
    });
    v.push(PatternEntry {
        pat: "per brand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ActiveBrand" })),
            ..Default::default()
        }),
        lua_line: 1343,
    });
    v.push(PatternEntry { pat: "per brand, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "ActiveBrand", globalLimit = num, globalLimitKey = "ChipAway" })), ..Default::default() }
    }), lua_line: 1344 });
    v.push(PatternEntry {
        pat: "per blitz charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BlitzCharge" })),
            ..Default::default()
        }),
        lua_line: 1345,
    });
    v.push(PatternEntry {
        pat: "per ghost shroud",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GhostShroud" })),
            ..Default::default()
        }),
        lua_line: 1346,
    });
    v.push(PatternEntry {
        pat: "per crab barrier",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CrabBarrier" })),
            ..Default::default()
        }),
        lua_line: 1347,
    });
    v.push(PatternEntry {
        pat: "per rage",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Rage" })),
            ..Default::default()
        }),
        lua_line: 1348,
    });
    v.push(PatternEntry {
        pat: "per rage while you are not losing rage",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Rage" })),
            ..Default::default()
        }),
        lua_line: 1349,
    });
    v.push(PatternEntry {
        pat: "per (%d+) rage",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = Multiplier, var = "Rage", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1350,
    });
    v.push(PatternEntry {
        pat: "per mana burn",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ManaBurnStacks" })),
            ..Default::default()
        }),
        lua_line: 1351,
    });
    v.push(PatternEntry {
        pat: "per mana burn on you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ManaBurnStacks" })),
            ..Default::default()
        }),
        lua_line: 1352,
    });
    v.push(PatternEntry { pat: "per mana burn, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "ManaBurnStacks", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1353 });
    v.push(PatternEntry {
        pat: "per level",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Level" })),
            ..Default::default()
        }),
        lua_line: 1354,
    });
    v.push(PatternEntry {
        pat: "per (%d+) player levels",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = Multiplier, var = "Level", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1355,
    });
    v.push(PatternEntry {
        pat: "per defiance",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Defiance" })),
            ..Default::default()
        }),
        lua_line: 1356,
    });
    v.push(PatternEntry { pat: "per (%d+)%% (%a+) effect on enemy", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let effectName = ns(caps.cap[1], "index local 'effectName'");
        ModSpec { tag: Some(tag!({ type = Multiplier, var = cat!(firstToUpper(effectName), "Effect"), div = num, actor = "enemy" })), ..Default::default() }
    }), lua_line: 1357 });
    v.push(PatternEntry {
        pat: "for each equipped normal item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NormalItem" })),
            ..Default::default()
        }),
        lua_line: 1358,
    });
    v.push(PatternEntry {
        pat: "for each normal item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NormalItem" })),
            ..Default::default()
        }),
        lua_line: 1359,
    });
    v.push(PatternEntry {
        pat: "for each normal item you have equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NormalItem" })),
            ..Default::default()
        }),
        lua_line: 1360,
    });
    v.push(PatternEntry {
        pat: "for each equipped magic item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MagicItem" })),
            ..Default::default()
        }),
        lua_line: 1361,
    });
    v.push(PatternEntry {
        pat: "for each magic item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MagicItem" })),
            ..Default::default()
        }),
        lua_line: 1362,
    });
    v.push(PatternEntry {
        pat: "for each magic item you have equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MagicItem" })),
            ..Default::default()
        }),
        lua_line: 1363,
    });
    v.push(PatternEntry {
        pat: "for each equipped rare item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RareItem" })),
            ..Default::default()
        }),
        lua_line: 1364,
    });
    v.push(PatternEntry {
        pat: "for each rare item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RareItem" })),
            ..Default::default()
        }),
        lua_line: 1365,
    });
    v.push(PatternEntry {
        pat: "for each rare item you have equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RareItem" })),
            ..Default::default()
        }),
        lua_line: 1366,
    });
    v.push(PatternEntry {
        pat: "for each equipped unique item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "UniqueItem" })),
            ..Default::default()
        }),
        lua_line: 1367,
    });
    v.push(PatternEntry {
        pat: "for each unique item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "UniqueItem" })),
            ..Default::default()
        }),
        lua_line: 1368,
    });
    v.push(PatternEntry {
        pat: "for each unique item you have equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "UniqueItem" })),
            ..Default::default()
        }),
        lua_line: 1369,
    });
    v.push(PatternEntry {
        pat: "per elder item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ElderItem" })),
            ..Default::default()
        }),
        lua_line: 1370,
    });
    v.push(PatternEntry {
        pat: "per shaper item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ShaperItem" })),
            ..Default::default()
        }),
        lua_line: 1371,
    });
}

/// @pob Modules/ModParser.lua:1372-1421 h=a3c8e00f
fn modTagList_1372(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "per elder or shaper item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ShaperOrElderItem" })),
            ..Default::default()
        }),
        lua_line: 1372,
    });
    v.push(PatternEntry { pat: "if (%d+) (%a+) items are equipped", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let effectName = ns(caps.cap[1], "index local 'effectName'");
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = cat!(firstToUpper(effectName), "Item"), threshold = num })), ..Default::default() }
    }), lua_line: 1373 });
    v.push(PatternEntry {
        pat: "for each corrupted item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CorruptedItem" })),
            ..Default::default()
        }),
        lua_line: 1374,
    });
    v.push(PatternEntry {
        pat: "for each equipped corrupted item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CorruptedItem" })),
            ..Default::default()
        }),
        lua_line: 1375,
    });
    v.push(PatternEntry {
        pat: "for each uncorrupted item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NonCorruptedItem" })),
            ..Default::default()
        }),
        lua_line: 1376,
    });
    v.push(PatternEntry {
        pat: "per equipped claw",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ClawItem" })),
            ..Default::default()
        }),
        lua_line: 1377,
    });
    v.push(PatternEntry {
        pat: "per equipped dagger",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "DaggerItem" })),
            ..Default::default()
        }),
        lua_line: 1378,
    });
    v.push(PatternEntry {
        pat: "per equipped axe",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AxeItem" })),
            ..Default::default()
        }),
        lua_line: 1379,
    });
    v.push(PatternEntry {
        pat: "per equipped ring",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RingItem" })),
            ..Default::default()
        }),
        lua_line: 1380,
    });
    v.push(PatternEntry {
        pat: "per equipped flask",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "FlaskItem" })),
            ..Default::default()
        }),
        lua_line: 1381,
    });
    v.push(PatternEntry {
        pat: "per equipped sword",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SwordItem" })),
            ..Default::default()
        }),
        lua_line: 1382,
    });
    v.push(PatternEntry {
        pat: "per equipped jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "JewelItem" })),
            ..Default::default()
        }),
        lua_line: 1383,
    });
    v.push(PatternEntry {
        pat: "per equipped mace",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MaceItem" })),
            ..Default::default()
        }),
        lua_line: 1384,
    });
    v.push(PatternEntry {
        pat: "per equipped sceptre",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SceptreItem" })),
            ..Default::default()
        }),
        lua_line: 1385,
    });
    v.push(PatternEntry {
        pat: "per equipped wand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "WandItem" })),
            ..Default::default()
        }),
        lua_line: 1386,
    });
    v.push(PatternEntry {
        pat: "per claw",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ClawItem" })),
            ..Default::default()
        }),
        lua_line: 1387,
    });
    v.push(PatternEntry {
        pat: "per dagger",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "DaggerItem" })),
            ..Default::default()
        }),
        lua_line: 1388,
    });
    v.push(PatternEntry {
        pat: "per axe",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AxeItem" })),
            ..Default::default()
        }),
        lua_line: 1389,
    });
    v.push(PatternEntry {
        pat: "per ring",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RingItem" })),
            ..Default::default()
        }),
        lua_line: 1390,
    });
    v.push(PatternEntry {
        pat: "per flask",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "FlaskItem" })),
            ..Default::default()
        }),
        lua_line: 1391,
    });
    v.push(PatternEntry {
        pat: "per sword",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SwordItem" })),
            ..Default::default()
        }),
        lua_line: 1392,
    });
    v.push(PatternEntry {
        pat: "per jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "JewelItem" })),
            ..Default::default()
        }),
        lua_line: 1393,
    });
    v.push(PatternEntry {
        pat: "per mace",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MaceItem" })),
            ..Default::default()
        }),
        lua_line: 1394,
    });
    v.push(PatternEntry {
        pat: "per sceptre",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SceptreItem" })),
            ..Default::default()
        }),
        lua_line: 1395,
    });
    v.push(PatternEntry {
        pat: "per wand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "WandItem" })),
            ..Default::default()
        }),
        lua_line: 1396,
    });
    v.push(PatternEntry {
        pat: "per abyssa?l? jewel affecting you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AbyssJewel" })),
            ..Default::default()
        }),
        lua_line: 1397,
    });
    v.push(PatternEntry {
        pat: "for each herald b?u?f?f?s?k?i?l?l? ?affecting you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Herald" })),
            ..Default::default()
        }),
        lua_line: 1398,
    });
    v.push(PatternEntry {
        pat: "for each of your aura or herald skills affecting you",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Multiplier, varList = &[n!("Herald"), n!("AuraAffectingSelf")][..] }),
            ),
            ..Default::default()
        }),
        lua_line: 1399,
    });
    v.push(PatternEntry {
        pat: "for each type of abyssa?l? jewel affecting you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AbyssJewelType" })),
            ..Default::default()
        }),
        lua_line: 1400,
    });
    // hand-ported (lua2rs: modtag-param1-num-or-string); `type` is the string capture unless it
    // holds a digit, then `tonumber` of it, which `type:gsub` cannot index
    v.push(PatternEntry { pat: "per (.+) eye jewel affecting you, up to a maximum of %+?(%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let r#type = arg1_str(caps, "type");
        let num = caps.cap[1];
        ModSpec { tag: Some(tag!({ type = Multiplier, var = cat!(gsub_fn(r#type, "^%l", |c| Some(upper(c[0].as_str())), None).0, "EyeJewel"), limit = lua_tonumber(num), limitTotal = true })), ..Default::default() }
    }), lua_line: 1401 });
    v.push(PatternEntry {
        pat: "per sextant affecting the area",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "Sextant" })),
            ..Default::default()
        }),
        lua_line: 1402,
    });
    v.push(PatternEntry {
        pat: "per buff on you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BuffOnSelf" })),
            ..Default::default()
        }),
        lua_line: 1403,
    });
    v.push(PatternEntry {
        pat: "per hit suppressed recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "HitsSuppressedRecently" })),
            ..Default::default()
        }),
        lua_line: 1404,
    });
    v.push(PatternEntry {
        pat: "per curse on enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CurseOnEnemy" })),
            ..Default::default()
        }),
        lua_line: 1405,
    });
    v.push(PatternEntry {
        pat: "for each curse on enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CurseOnEnemy" })),
            ..Default::default()
        }),
        lua_line: 1406,
    });
    v.push(PatternEntry {
        pat: "for each curse on the enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CurseOnEnemy" })),
            ..Default::default()
        }),
        lua_line: 1407,
    });
    v.push(PatternEntry {
        pat: "per curse on you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CurseOnSelf" })),
            ..Default::default()
        }),
        lua_line: 1408,
    });
    v.push(PatternEntry {
        pat: "per poison on you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "PoisonStack" })),
            ..Default::default()
        }),
        lua_line: 1409,
    });
    v.push(PatternEntry {
        pat: "for each poison on you",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "PoisonStack" })),
            ..Default::default()
        }),
        lua_line: 1410,
    });
    v.push(PatternEntry { pat: "for each poison on you up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "PoisonStack", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1411 });
    v.push(PatternEntry { pat: "per poison on you, up to (%d+) per second", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "PoisonStack", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1412 });
    v.push(PatternEntry {
        pat: "for each poison you have inflicted recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "PoisonAppliedRecently" })),
            ..Default::default()
        }),
        lua_line: 1413,
    });
    v.push(PatternEntry {
        pat: "per withered debuff on enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Multiplier, var = "WitheredStack", actor = "enemy", limit = 15.0 }),
            ),
            ..Default::default()
        }),
        lua_line: 1414,
    });
    v.push(PatternEntry { pat: "for each poison you have inflicted recently, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "PoisonAppliedRecently", globalLimit = num, globalLimitKey = "DurationPerPoisonRecently" })), ..Default::default() }
    }), lua_line: 1415 });
    v.push(PatternEntry { pat: "for each time you have shocked a non%-shocked enemy recently, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "ShockedNonShockedEnemyRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1416 });
    v.push(PatternEntry {
        pat: "for each shocked enemy you've killed recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ShockedEnemyKilledRecently" })),
            ..Default::default()
        }),
        lua_line: 1417,
    });
    v.push(PatternEntry { pat: "per enemy killed recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "EnemyKilledRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1418 });
    v.push(PatternEntry { pat: "per (%d+) rampage kills", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "Rampage", div = num, limit = (1000.0 / nn(num, "perform arithmetic on local 'num'")), limitTotal = true })), ..Default::default() }
    }), lua_line: 1419 });
    v.push(PatternEntry { pat: "per minion, up to ?a? ?m?a?x?i?m?u?m? ?o?f? (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "SummonedMinion", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1420 });
    v.push(PatternEntry {
        pat: "per minion from your non%-vaal skills",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NonVaalSummonedMinion" })),
            ..Default::default()
        }),
        lua_line: 1421,
    });
}

/// @pob Modules/ModParser.lua:1422-1472 h=48499a24
fn modTagList_1422(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "per minion",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SummonedMinion" })),
            ..Default::default()
        }),
        lua_line: 1422,
    });
    v.push(PatternEntry { pat: "for each enemy you or your minions have killed recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, varList = &[n!("EnemyKilledRecently"), n!("EnemyKilledByMinionsRecently")][..], limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1423 });
    v.push(PatternEntry { pat: "for each enemy you or your minions have killed recently, up to (%d+)%% per second", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, varList = &[n!("EnemyKilledRecently"), n!("EnemyKilledByMinionsRecently")][..], limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1424 });
    v.push(PatternEntry {
        pat: "for each (%d+) total mana y?o?u? ?h?a?v?e? ?spent recently",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = Multiplier, var = "ManaSpentRecently", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1425,
    });
    v.push(PatternEntry { pat: "for each (%d+) total mana you have spent recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "ManaSpentRecently", div = num, limit = tonumber(limit), limitTotal = true })), ..Default::default() }
    }), lua_line: 1426 });
    v.push(PatternEntry { pat: "per (%d+) mana spent recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "ManaSpentRecently", div = num, limit = tonumber(limit), limitTotal = true })), ..Default::default() }
    }), lua_line: 1427 });
    v.push(PatternEntry {
        pat: "for each time you've blocked in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BlockedPast10Sec" })),
            ..Default::default()
        }),
        lua_line: 1428,
    });
    v.push(PatternEntry { pat: "per enemy killed by you or your totems recently", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Multiplier, varList = &[n!("EnemyKilledRecently"), n!("EnemyKilledByTotemsRecently")][..] })), ..Default::default() }), lua_line: 1429 });
    v.push(PatternEntry { pat: "per nearby enemy, up to %+?(%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "NearbyEnemies", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1430 });
    v.push(PatternEntry {
        pat: "per enemy in close range",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "AtCloseRange" }),
                tag!({ type = Multiplier, var = "NearbyEnemies" }),
            ],
            ..Default::default()
        }),
        lua_line: 1431,
    });
    v.push(PatternEntry {
        pat: "per red socket",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RedSocketIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1432,
    });
    v.push(PatternEntry {
        pat: "per green socket on main hand weapon",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GreenSocketInWeapon 1" })),
            ..Default::default()
        }),
        lua_line: 1433,
    });
    v.push(PatternEntry {
        pat: "per green socket on",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GreenSocketInWeapon 1" })),
            ..Default::default()
        }),
        lua_line: 1434,
    });
    v.push(PatternEntry {
        pat: "per red socket on main hand weapon",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RedSocketInWeapon 1" })),
            ..Default::default()
        }),
        lua_line: 1435,
    });
    v.push(PatternEntry {
        pat: "per red socket on equipped staff",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Multiplier, var = "RedSocketInWeapon 1" }),
                tag!({ type = Condition, var = "UsingStaff" }),
            ],
            ..Default::default()
        }),
        lua_line: 1436,
    });
    v.push(PatternEntry {
        pat: "per blue socket on equipped staff",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Multiplier, var = "BlueSocketInWeapon 1" }),
                tag!({ type = Condition, var = "UsingStaff" }),
            ],
            ..Default::default()
        }),
        lua_line: 1437,
    });
    v.push(PatternEntry {
        pat: "per green socket",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GreenSocketIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1438,
    });
    v.push(PatternEntry {
        pat: "per blue socket",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BlueSocketIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1439,
    });
    v.push(PatternEntry {
        pat: "per white socket",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "WhiteSocketIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1440,
    });
    v.push(PatternEntry {
        pat: "for each unlinked socket in equipped two handed weapon",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Multiplier, var = "UnlinkedSocketInWeapon 1" }),
                tag!({ type = Condition, var = "UsingTwoHandedWeapon" }),
            ],
            ..Default::default()
        }),
        lua_line: 1441,
    });
    v.push(PatternEntry {
        pat: "for each empty red socket on any equipped item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "EmptyRedSocketsInAnySlot" })),
            ..Default::default()
        }),
        lua_line: 1442,
    });
    v.push(PatternEntry {
        pat: "for each empty green socket on any equipped item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "EmptyGreenSocketsInAnySlot" })),
            ..Default::default()
        }),
        lua_line: 1443,
    });
    v.push(PatternEntry {
        pat: "for each empty blue socket on any equipped item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "EmptyBlueSocketsInAnySlot" })),
            ..Default::default()
        }),
        lua_line: 1444,
    });
    v.push(PatternEntry {
        pat: "for each empty white socket on any equipped item",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "EmptyWhiteSocketsInAnySlot" })),
            ..Default::default()
        }),
        lua_line: 1445,
    });
    v.push(PatternEntry {
        pat: "per socketed gem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SocketedGemsIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1446,
    });
    v.push(PatternEntry {
        pat: "per socketed red gem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SocketedRedGemsIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1447,
    });
    v.push(PatternEntry {
        pat: "per socketed green gem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SocketedGreenGemsIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1448,
    });
    v.push(PatternEntry {
        pat: "per socketed blue gem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SocketedBlueGemsIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1449,
    });
    v.push(PatternEntry {
        pat: "per socketed murderous eye jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MurderousEyeJewelIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1450,
    });
    v.push(PatternEntry {
        pat: "per socketed searching eye jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "SearchingEyeJewelIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1451,
    });
    v.push(PatternEntry {
        pat: "per socketed hypnotic eye jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "HypnoticEyeJewelIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1452,
    });
    v.push(PatternEntry {
        pat: "per socketed ghastly eye jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GhastlyEyeJewelIn{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1453,
    });
    v.push(PatternEntry {
        pat: "for each impale on enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ImpaleStacks", actor = "enemy" })),
            ..Default::default()
        }),
        lua_line: 1454,
    });
    v.push(PatternEntry {
        pat: "per impale on enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ImpaleStacks", actor = "enemy" })),
            ..Default::default()
        }),
        lua_line: 1455,
    });
    v.push(PatternEntry {
        pat: "per grasping vine",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GraspingVinesCount" })),
            ..Default::default()
        }),
        lua_line: 1456,
    });
    v.push(PatternEntry {
        pat: "per fragile regrowth",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "FragileRegrowthCount" })),
            ..Default::default()
        }),
        lua_line: 1457,
    });
    v.push(PatternEntry {
        pat: "per bark",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BarkskinStacks" })),
            ..Default::default()
        }),
        lua_line: 1458,
    });
    v.push(PatternEntry {
        pat: "per bark below maximum",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MissingBarkskinStacks" })),
            ..Default::default()
        }),
        lua_line: 1459,
    });
    v.push(PatternEntry {
        pat: "per allocated mastery passive skill",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AllocatedMastery" })),
            ..Default::default()
        }),
        lua_line: 1460,
    });
    v.push(PatternEntry {
        pat: "per allocated notable passive skill",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AllocatedNotable" })),
            ..Default::default()
        }),
        lua_line: 1461,
    });
    v.push(PatternEntry {
        pat: "for each different type of mastery you have allocated",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AllocatedMasteryType" })),
            ..Default::default()
        }),
        lua_line: 1462,
    });
    v.push(PatternEntry {
        pat: "per grand spectrum",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "GrandSpectrum" })),
            ..Default::default()
        }),
        lua_line: 1463,
    });
    v.push(PatternEntry { pat: "per second you've been stationary, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "StationarySeconds", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1464 });
    v.push(PatternEntry {
        pat: "per elemental ailment you've inflicted recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "AppliedAilmentsRecently" })),
            ..Default::default()
        }),
        lua_line: 1465,
    });
    // Per stat
    v.push(PatternEntry {
        pat: "per (%d+)%% of maximum mana they reserve",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ManaReservedPercent", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1467,
    });
    v.push(PatternEntry {
        pat: "for each (%d+)%% of life reserved",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "LifeReservedPercent", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1468,
    });
    v.push(PatternEntry {
        pat: "per (%d+) strength",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Str", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1469,
    });
    v.push(PatternEntry {
        pat: "per dexterity",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "Dex" })),
            ..Default::default()
        }),
        lua_line: 1470,
    });
    v.push(PatternEntry {
        pat: "per (%d+) dexterity",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Dex", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1471,
    });
    v.push(PatternEntry {
        pat: "per (%d+) intelligence",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Int", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1472,
    });
}

/// @pob Modules/ModParser.lua:1473-1522 h=53cb6623
fn modTagList_1473(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "per (%d+) omniscience",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Omni", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1473,
    });
    v.push(PatternEntry { pat: "per (%d+) total attributes", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = PerStat, statList = &[n!("Str"), n!("Dex"), n!("Int")][..], div = num })), ..Default::default() }
    }), lua_line: 1474 });
    v.push(PatternEntry {
        pat: "per (%d+) of your lowest attribute",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "LowestAttribute", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1475,
    });
    v.push(PatternEntry {
        pat: "per (%d+) reserved life",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "LifeReserved", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1476,
    });
    v.push(PatternEntry {
        pat: "per (%d+) unreserved maximum mana",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ManaUnreserved", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1477,
    });
    v.push(PatternEntry { pat: "per (%d+) unreserved maximum mana, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "ManaUnreserved", div = num, limit = tonumber(limit), limitTotal = true })), ..Default::default() }
    }), lua_line: 1478 });
    v.push(PatternEntry {
        pat: "per (%d+) armour",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Armour", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1479,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Evasion", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1480,
    });
    v.push(PatternEntry { pat: "per (%d+) evasion rating, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "Evasion", div = num, limit = tonumber(limit), limitTotal = true })), ..Default::default() }
    }), lua_line: 1481 });
    v.push(PatternEntry {
        pat: "per (%d+) maximum energy shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShield", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1482,
    });
    v.push(PatternEntry {
        pat: "per (%d+) player maximum energy shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = PerStat, stat = "EnergyShield", div = num, actor = "player" }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1483,
    });
    v.push(PatternEntry {
        pat: "per (%d+) maximum life",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Life", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1484,
    });
    v.push(PatternEntry {
        pat: "per (%d+) of maximum life or maximum mana, whichever is lower",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = PerStat, stat = "LowestOfMaximumLifeAndMaximumMana", div = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1485,
    });
    v.push(PatternEntry {
        pat: "per (%d+) player maximum life",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Life", div = num, actor = "player" })),
                ..Default::default()
            }
        }),
        lua_line: 1486,
    });
    v.push(PatternEntry {
        pat: "per (%d+) maximum mana",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Mana", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1487,
    });
    v.push(PatternEntry { pat: "per (%d+) maximum mana, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "Mana", div = num, limit = tonumber(limit), limitTotal = true })), ..Default::default() }
    }), lua_line: 1488 });
    v.push(PatternEntry { pat: "per (%d+) maximum mana, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "Mana", div = num, limit = tonumber(limit), limitTotal = true })), ..Default::default() }
    }), lua_line: 1489 });
    v.push(PatternEntry {
        pat: "per soul required",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "SoulCost" })),
            ..Default::default()
        }),
        lua_line: 1490,
    });
    v.push(PatternEntry {
        pat: "per (%d+) accuracy rating",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Accuracy", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1491,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% block chance",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "BlockChance", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1492,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% chance to block on equipped shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ShieldBlockChance", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1493,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% chance to block attack damage",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "BlockChance", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1494,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% chance to block spell damage",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "SpellBlockChance", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1495,
    });
    v.push(PatternEntry {
        pat: "per (%d+) of the lowest of armour and evasion rating",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "LowestOfArmourAndEvasion", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1496,
    });
    v.push(PatternEntry {
        pat: "per (%d+) energy shield on equipped gloves",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnGloves", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1497,
    });
    v.push(PatternEntry {
        pat: "per (%d+) maximum energy shield on helmet",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnHelmet", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1498,
    });
    v.push(PatternEntry {
        pat: "per (%d+) maximum energy shield on equipped helmet",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnHelmet", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1499,
    });
    v.push(PatternEntry {
        pat: "per (%d+) energy shield on equipped helmet",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnHelmet", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1500,
    });
    v.push(PatternEntry {
        pat: "per (%d+) energy shield on equipped boots",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnBoots", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1501,
    });
    v.push(PatternEntry {
        pat: "per (%d+) energy shield on equipped body armour",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnBody Armour", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1502,
    });
    v.push(PatternEntry {
        pat: "per (%d+) maximum energy shield on equipped shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnWeapon 2", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1503,
    });
    v.push(PatternEntry {
        pat: "per (%d+) maximum energy shield on shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EnergyShieldOnWeapon 2", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1504,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating on equipped gloves",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnGloves", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1505,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating on equipped helmet",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnHelmet", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1506,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion on equipped boots",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnBoots", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1507,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion on boots",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnBoots", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1508,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating on equipped boots",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnBoots", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1509,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating on body armour",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnBody Armour", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1510,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating on equipped body armour",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnBody Armour", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1511,
    });
    v.push(PatternEntry {
        pat: "per (%d+) evasion rating on equipped shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "EvasionOnWeapon 2", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1512,
    });
    v.push(PatternEntry {
        pat: "per (%d+) armour on gloves",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ArmourOnGloves", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1513,
    });
    v.push(PatternEntry {
        pat: "per (%d+) armour on equipped gloves",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ArmourOnGloves", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1514,
    });
    v.push(PatternEntry {
        pat: "per (%d+) armour on equipped helmet",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ArmourOnHelmet", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1515,
    });
    v.push(PatternEntry {
        pat: "per (%d+) armour on equipped boots",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ArmourOnBoots", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1516,
    });
    v.push(PatternEntry {
        pat: "per (%d+) armour on equipped body armour",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ArmourOnBody Armour", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1517,
    });
    v.push(PatternEntry {
        pat: "per (%d+) armour on equipped shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ArmourOnWeapon 2", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1518,
    });
    v.push(PatternEntry { pat: "per (%d+) armour or evasion rating on shield", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = PerStat, statList = &[n!("ArmourOnWeapon 2"), n!("EvasionOnWeapon 2")][..], div = num })), ..Default::default() }
    }), lua_line: 1519 });
    v.push(PatternEntry { pat: "per (%d+) armour or evasion rating on equipped shield", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = PerStat, statList = &[n!("ArmourOnWeapon 2"), n!("EvasionOnWeapon 2")][..], div = num })), ..Default::default() }
    }), lua_line: 1520 });
    v.push(PatternEntry {
        pat: "per (%d+)%% cold resistance",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ColdResist", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1521,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% fire resistance",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "FireResist", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1522,
    });
}

/// @pob Modules/ModParser.lua:1523-1573 h=faa925e9
fn modTagList_1523(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "per (%d+)%% lightning resistance",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "LightningResist", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1523,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% chaos resistance",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ChaosResist", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1524,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% cold resistance above 75%%",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "ColdResistOver75", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1525,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% lightning resistance above 75%%",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "LightningResistOver75", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1526,
    });
    v.push(PatternEntry {
        pat: "per (%d+)%% fire resistance above 75%%",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "FireResistOver75", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1527,
    });
    v.push(PatternEntry { pat: "per (%d+)%% fire, cold, or lightning resistance above 75%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = PerStat, statList = &[n!("FireResistOver75"), n!("ColdResistOver75"), n!("LightningResistOver75")][..], div = num })), ..Default::default() }
    }), lua_line: 1528 });
    v.push(PatternEntry {
        pat: "per (%d+) devotion",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = PerStat, stat = "Devotion", actor = "parent", div = num })),
                ..Default::default()
            }
        }),
        lua_line: 1529,
    });
    v.push(PatternEntry { pat: "per (%d+)%% missing fire resistance, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "MissingFireResist", div = num, globalLimit = tonumber(limit), globalLimitKey = "ReplicaNebulisFire" })), ..Default::default() }
    }), lua_line: 1530 });
    v.push(PatternEntry { pat: "per (%d+)%% missing cold resistance, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "MissingColdResist", div = num, globalLimit = tonumber(limit), globalLimitKey = "ReplicaNebulisCold" })), ..Default::default() }
    }), lua_line: 1531 });
    v.push(PatternEntry { pat: "per (%d+)%% missing fire, cold, or lightning resistance, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        let limit = ns(caps.cap[1], "index local 'limit'");
        ModSpec { tag: Some(tag!({ type = PerStat, statList = &[n!("MissingFireResist"), n!("MissingColdResist"), n!("MissingLightningResist")][..], div = num, globalLimit = tonumber(limit), globalLimitKey = "ReplicaNebulisCold" })), ..Default::default() }
    }), lua_line: 1532 });
    v.push(PatternEntry {
        pat: "per endurance, frenzy or power charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "TotalCharges" })),
            ..Default::default()
        }),
        lua_line: 1533,
    });
    v.push(PatternEntry {
        pat: "per fortification",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "FortificationStacks" })),
            ..Default::default()
        }),
        lua_line: 1534,
    });
    v.push(PatternEntry {
        pat: "per two fortification on you",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = PerStat, stat = "FortificationStacks", div = 2.0, actor = "player" }),
            ),
            ..Default::default()
        }),
        lua_line: 1535,
    });
    v.push(PatternEntry {
        pat: "per fortification above 20",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "FortificationStacksOver20" })),
            ..Default::default()
        }),
        lua_line: 1536,
    });
    v.push(PatternEntry {
        pat: "per totem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "TotemsSummoned" })),
            ..Default::default()
        }),
        lua_line: 1537,
    });
    v.push(PatternEntry {
        pat: "per summoned totem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "TotemsSummoned" })),
            ..Default::default()
        }),
        lua_line: 1538,
    });
    v.push(PatternEntry {
        pat: "for each summoned totem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "TotemsSummoned" })),
            ..Default::default()
        }),
        lua_line: 1539,
    });
    v.push(PatternEntry {
        pat: "per maximum number of summoned totems",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveTotemLimit" })),
            ..Default::default()
        }),
        lua_line: 1540,
    });
    v.push(PatternEntry {
        pat: "for each time they have chained",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "Chain" })),
            ..Default::default()
        }),
        lua_line: 1541,
    });
    v.push(PatternEntry {
        pat: "for each time it has chained",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "Chain" })),
            ..Default::default()
        }),
        lua_line: 1542,
    });
    v.push(PatternEntry {
        pat: "for each summoned golem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveGolemLimit" })),
            ..Default::default()
        }),
        lua_line: 1543,
    });
    v.push(PatternEntry {
        pat: "for each golem you have summoned",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveGolemLimit" })),
            ..Default::default()
        }),
        lua_line: 1544,
    });
    v.push(PatternEntry {
        pat: "per summoned golem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveGolemLimit" })),
            ..Default::default()
        }),
        lua_line: 1545,
    });
    v.push(PatternEntry {
        pat: "per summoned sentinel of purity",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveSentinelOfPurityLimit" })),
            ..Default::default()
        }),
        lua_line: 1546,
    });
    v.push(PatternEntry {
        pat: "per summoned void spawn",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveVoidSpawnLimit" })),
            ..Default::default()
        }),
        lua_line: 1547,
    });
    v.push(PatternEntry {
        pat: "per summoned skeleton",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveSkeletonLimit" })),
            ..Default::default()
        }),
        lua_line: 1548,
    });
    v.push(PatternEntry {
        pat: "per skeleton you own",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveSkeletonLimit", actor = "parent" })),
            ..Default::default()
        }),
        lua_line: 1549,
    });
    v.push(PatternEntry {
        pat: "per summoned raging spirit",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveRagingSpiritLimit" })),
            ..Default::default()
        }),
        lua_line: 1550,
    });
    v.push(PatternEntry {
        pat: "per summoned phantasm",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActivePhantasmLimit" })),
            ..Default::default()
        }),
        lua_line: 1551,
    });
    v.push(PatternEntry {
        pat: "per animated weapon",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = PerStat, stat = "ActiveAnimatedWeaponLimit", actor = "parent" }),
            ),
            ..Default::default()
        }),
        lua_line: 1552,
    });
    v.push(PatternEntry {
        pat: "for each raised zombie",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveZombieLimit" })),
            ..Default::default()
        }),
        lua_line: 1553,
    });
    v.push(PatternEntry {
        pat: "per zombie you own",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveZombieLimit", actor = "parent" })),
            ..Default::default()
        }),
        lua_line: 1554,
    });
    v.push(PatternEntry {
        pat: "per raised zombie",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveZombieLimit" })),
            ..Default::default()
        }),
        lua_line: 1555,
    });
    v.push(PatternEntry {
        pat: "per raised spectre",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveSpectreLimit" })),
            ..Default::default()
        }),
        lua_line: 1556,
    });
    v.push(PatternEntry {
        pat: "per spectre you own",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ActiveSpectreLimit", actor = "parent" })),
            ..Default::default()
        }),
        lua_line: 1557,
    });
    v.push(PatternEntry {
        pat: "for each remaining chain",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "ChainRemaining" })),
            ..Default::default()
        }),
        lua_line: 1558,
    });
    v.push(PatternEntry { pat: "for each remaining chain, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let limit = caps.num;
        ModSpec { tag: Some(tag!({ type = PerStat, stat = "ChainRemaining", globalLimit = limit, globalLimitKey = "FollowThrough" })), ..Default::default() }
    }), lua_line: 1559 });
    v.push(PatternEntry {
        pat: "for each enemy pierced",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "PiercedCount" })),
            ..Default::default()
        }),
        lua_line: 1560,
    });
    v.push(PatternEntry {
        pat: "for each time they've pierced",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = PerStat, stat = "PiercedCount" })),
            ..Default::default()
        }),
        lua_line: 1561,
    });
    // Stat conditions
    v.push(PatternEntry {
        pat: "with (%d+) or more strength",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Str", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1563,
    });
    v.push(PatternEntry {
        pat: "with at least (%d+) strength",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Str", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1564,
    });
    v.push(PatternEntry {
        pat: "w?h?i[lf]e? you have at least (%d+) strength",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Str", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1565,
    });
    v.push(PatternEntry {
        pat: "w?h?i[lf]e? you have at least (%d+) dexterity",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Dex", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1566,
    });
    v.push(PatternEntry {
        pat: "w?h?i[lf]e? you have at least (%d+) intelligence",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Int", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1567,
    });
    v.push(PatternEntry { pat: "w?h?i[lf]e? strength is below (%d+)", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "Str", threshold = (nn(num, "perform arithmetic on local 'num'") - 1.0), upper = true })), ..Default::default() }
    }), lua_line: 1568 });
    v.push(PatternEntry { pat: "w?h?i[lf]e? dexterity is below (%d+)", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "Dex", threshold = (nn(num, "perform arithmetic on local 'num'") - 1.0), upper = true })), ..Default::default() }
    }), lua_line: 1569 });
    v.push(PatternEntry { pat: "w?h?i[lf]e? intelligence is below (%d+)", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "Int", threshold = (nn(num, "perform arithmetic on local 'num'") - 1.0), upper = true })), ..Default::default() }
    }), lua_line: 1570 });
    v.push(PatternEntry {
        pat: "at least (%d+) intelligence",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Int", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1571,
    });
    v.push(PatternEntry {
        pat: "if dexterity is higher than intelligence",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DexHigherThanInt" })),
            ..Default::default()
        }),
        lua_line: 1572,
    });
    v.push(PatternEntry {
        pat: "if strength is higher than intelligence",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "StrHigherThanInt" })),
            ..Default::default()
        }),
        lua_line: 1573,
    });
}

/// @pob Modules/ModParser.lua:1574-1629 h=c9396890
fn modTagList_1574(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "w?h?i[lf]e? you have at least (%d+) maximum energy shield",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "EnergyShield", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1574,
    });
    v.push(PatternEntry {
        pat: "against targets they pierce",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1575,
    });
    v.push(PatternEntry {
        pat: "against pierced targets",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1576,
    });
    v.push(PatternEntry {
        pat: "to targets they pierce",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "PierceCount", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1577,
    });
    v.push(PatternEntry { pat: "that fire a single projectile", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "ProjectileCount", threshold = 1.0, upper = true })), ..Default::default() }), lua_line: 1578 });
    v.push(PatternEntry {
        pat: "w?h?i[lf]e? you have at least (%d+) devotion",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "Devotion", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1579,
    });
    v.push(PatternEntry {
        pat: "while you have at least (%d+) rage",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = MultiplierThreshold, var = "Rage", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1580,
    });
    v.push(PatternEntry {
        pat: "while affected by a unique abyss jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = MultiplierThreshold, var = "UniqueAbyssJewels", threshold = 1.0 }),
            ),
            ..Default::default()
        }),
        lua_line: 1581,
    });
    v.push(PatternEntry {
        pat: "while affected by a rare abyss jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = MultiplierThreshold, var = "RareAbyssJewels", threshold = 1.0 }),
            ),
            ..Default::default()
        }),
        lua_line: 1582,
    });
    v.push(PatternEntry {
        pat: "while affected by a magic abyss jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = MultiplierThreshold, var = "MagicAbyssJewels", threshold = 1.0 }),
            ),
            ..Default::default()
        }),
        lua_line: 1583,
    });
    v.push(PatternEntry {
        pat: "while affected by a normal abyss jewel",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = MultiplierThreshold, var = "NormalAbyssJewels", threshold = 1.0 }),
            ),
            ..Default::default()
        }),
        lua_line: 1584,
    });
    v.push(PatternEntry {
        pat: "while you have at least (%d+) nearby all[yi]e?s?",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = MultiplierThreshold, var = "NearbyAlly", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1585,
    });
    // Slot conditions
    v.push(PatternEntry {
        pat: "when in main hand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SlotNumber, num = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1587,
    });
    v.push(PatternEntry {
        pat: "whi?l?en? in off hand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SlotNumber, num = 2.0 })),
            ..Default::default()
        }),
        lua_line: 1588,
    });
    v.push(PatternEntry {
        pat: "in main hand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = InSlot, num = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1589,
    });
    v.push(PatternEntry {
        pat: "in off hand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = InSlot, num = 2.0 })),
            ..Default::default()
        }),
        lua_line: 1590,
    });
    v.push(PatternEntry {
        pat: "w?i?t?h? main hand",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "MainHandAttack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
            ..Default::default()
        }),
        lua_line: 1591,
    });
    v.push(PatternEntry {
        pat: "w?i?t?h? off ?hand",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "OffHandAttack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
            ..Default::default()
        }),
        lua_line: 1592,
    });
    v.push(PatternEntry {
        pat: "[fi]?[rn]?[of]?[ml]?[ i]?[hc]?[it]?[te]?[sd]? ? with this weapon",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
            ..Default::default()
        }),
        lua_line: 1593,
    });
    v.push(PatternEntry {
        pat: "if your o[tp][hp][eo][rs]i?t?e? ring is a shaper item",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = ItemCondition, itemSlot = "Ring {OtherSlotNum}", shaperCond = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1594,
    });
    v.push(PatternEntry {
        pat: "if your o[tp][hp][eo][rs]i?t?e? ring is an elder item",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = ItemCondition, itemSlot = "Ring {OtherSlotNum}", elderCond = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1595,
    });
    // hand-ported (lua2rs: and-or): `(slot == "right" and 2 or slot == "left" and 1)` is 2, 1 or
    // false; concatenating false raises
    v.push(PatternEntry { pat: "if you have a (%a+) (%a+) in (%a+) slot", val: TagVal::Fn(|caps: &Caps| {
        let rarity = ns(caps.cap[0], "index local 'rarity'");
        let item = ns(caps.cap[1], "index local 'item'");
        let slot = caps.cap[2];
        let up = |s: &str| gsub_fn(s, "^%l", |c| Some(upper(c[0].as_str())), None).0;
        let rarityUp = up(rarity);
        let itemUp = up(item);
        let slotNum: f64 = if slot == Some("right") {
            2.0
        } else if slot == Some("left") {
            1.0
        } else {
            lua_error!("attempt to concatenate a boolean value")
        };
        ModSpec { tag: Some(tag!({ type = Condition, var = cat!(rarityUp, "ItemIn", itemUp, " ", slotNum) })), ..Default::default() }
    }), lua_line: 1596 });
    v.push(PatternEntry {
        pat: "of skills supported by spellslinger",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SupportedBySpellslinger" })),
            ..Default::default()
        }),
        lua_line: 1597,
    });
    // Equipment conditions
    v.push(PatternEntry {
        pat: "while holding a (%w+)",
        val: TagVal::Fn(|caps: &Caps| {
            let gear = ns(caps.cap[0], "index local 'gear'");
            ModSpec {
                tag: Some(
                    tag!({ type = Condition, varList = &[cat!("Using", firstToUpper(gear))][..] }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1599,
    });
    v.push(PatternEntry { pat: "while holding a (%w+) or (%w+)", val: TagVal::Fn(|caps: &Caps| {
        let g1 = ns(caps.cap[0], "index local 'g1'");
        let g2 = ns(caps.cap[1], "index local 'g2'");
        ModSpec { tag: Some(tag!({ type = Condition, varList = &[cat!("Using", firstToUpper(g1)), cat!("Using", firstToUpper(g2))][..] })), ..Default::default() }
    }), lua_line: 1602 });
    v.push(PatternEntry {
        pat: "while holding a fishing rod",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFishing" })),
            ..Default::default()
        }),
        lua_line: 1605,
    });
    v.push(PatternEntry {
        pat: "while your off hand is empty",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OffHandIsEmpty" })),
            ..Default::default()
        }),
        lua_line: 1606,
    });
    v.push(PatternEntry {
        pat: "with shields",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingShield" })),
            ..Default::default()
        }),
        lua_line: 1607,
    });
    v.push(PatternEntry {
        pat: "while dual wielding",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DualWielding" })),
            ..Default::default()
        }),
        lua_line: 1608,
    });
    v.push(PatternEntry {
        pat: "while dual wielding claws",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DualWieldingClaws" })),
            ..Default::default()
        }),
        lua_line: 1609,
    });
    v.push(PatternEntry {
        pat: "while dual wielding or holding a shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Condition, varList = &[n!("DualWielding"), n!("UsingShield")][..] }),
            ),
            ..Default::default()
        }),
        lua_line: 1610,
    });
    v.push(PatternEntry {
        pat: "while wielding an axe",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingAxe" })),
            ..Default::default()
        }),
        lua_line: 1611,
    });
    v.push(PatternEntry {
        pat: "while wielding an axe or sword",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Condition, varList = &[n!("UsingAxe"), n!("UsingSword")][..] }),
            ),
            ..Default::default()
        }),
        lua_line: 1612,
    });
    v.push(PatternEntry {
        pat: "while wielding a bow",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingBow" })),
            ..Default::default()
        }),
        lua_line: 1613,
    });
    v.push(PatternEntry {
        pat: "while wielding a claw",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingClaw" })),
            ..Default::default()
        }),
        lua_line: 1614,
    });
    v.push(PatternEntry {
        pat: "while wielding a dagger",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingDagger" })),
            ..Default::default()
        }),
        lua_line: 1615,
    });
    v.push(PatternEntry {
        pat: "while wielding a claw or dagger",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Condition, varList = &[n!("UsingClaw"), n!("UsingDagger")][..] }),
            ),
            ..Default::default()
        }),
        lua_line: 1616,
    });
    v.push(PatternEntry {
        pat: "while wielding a mace",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingMace" })),
            ..Default::default()
        }),
        lua_line: 1617,
    });
    v.push(PatternEntry {
        pat: "while wielding a mace or sceptre",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingMace" })),
            ..Default::default()
        }),
        lua_line: 1618,
    });
    v.push(PatternEntry {
        pat: "while wielding a mace, sceptre or staff",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Condition, varList = &[n!("UsingMace"), n!("UsingStaff")][..] }),
            ),
            ..Default::default()
        }),
        lua_line: 1619,
    });
    v.push(PatternEntry {
        pat: "while wielding a staff",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingStaff" })),
            ..Default::default()
        }),
        lua_line: 1620,
    });
    v.push(PatternEntry {
        pat: "while wielding a sword",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingSword" })),
            ..Default::default()
        }),
        lua_line: 1621,
    });
    v.push(PatternEntry {
        pat: "while wielding a melee weapon",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingMeleeWeapon" })),
            ..Default::default()
        }),
        lua_line: 1622,
    });
    v.push(PatternEntry {
        pat: "while wielding a one handed weapon",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingOneHandedWeapon" })),
            ..Default::default()
        }),
        lua_line: 1623,
    });
    v.push(PatternEntry {
        pat: "while wielding a two handed weapon",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingTwoHandedWeapon" })),
            ..Default::default()
        }),
        lua_line: 1624,
    });
    v.push(PatternEntry {
        pat: "while wielding a two handed melee weapon",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "UsingTwoHandedWeapon" }),
                tag!({ type = Condition, var = "UsingMeleeWeapon" }),
            ],
            ..Default::default()
        }),
        lua_line: 1625,
    });
    v.push(PatternEntry {
        pat: "while wielding a wand",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingWand" })),
            ..Default::default()
        }),
        lua_line: 1626,
    });
    v.push(PatternEntry {
        pat: "while wielding two different weapon types",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "WieldingDifferentWeaponTypes" })),
            ..Default::default()
        }),
        lua_line: 1627,
    });
    v.push(PatternEntry {
        pat: "while unarmed",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Unarmed" })),
            ..Default::default()
        }),
        lua_line: 1628,
    });
    v.push(PatternEntry {
        pat: "while you are unencumbered",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Unencumbered" })),
            ..Default::default()
        }),
        lua_line: 1629,
    });
}

/// @pob Modules/ModParser.lua:1630-1688 h=90f87893
fn modTagList_1630(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "equipped bow",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingBow" })),
            ..Default::default()
        }),
        lua_line: 1630,
    });
    v.push(PatternEntry { pat: "if equipped ([%a%s]+) has an ([%a%s]+) modifier", val: TagVal::Fn(|caps: &Caps| {
        let itemSlotName = ns(caps.cap[0], "index local 'itemSlotName'");
        let conditionSubstring = ns(caps.cap[1], "index local 'conditionSubstring'");
        ModSpec { tag: Some(tag!({ type = ItemCondition, searchCond = &*conditionSubstring, itemSlot = Name::from(itemSlotName) })), ..Default::default() }
    }), lua_line: 1631 });
    v.push(PatternEntry { pat: "if both equipped ([%a%s]+) have a?n? ?([%a%s]+) modifiers?", val: TagVal::Fn(|caps: &Caps| {
        let itemSlotName = ns(caps.cap[0], "index local 'itemSlotName'");
        let conditionSubstring = ns(caps.cap[1], "index local 'conditionSubstring'");
        ModSpec { tag: Some(tag!({ type = ItemCondition, searchCond = &*conditionSubstring, itemSlot = Name::from(&*sub(itemSlotName, 1, Some((((len(itemSlotName) as f64) - 1.0) as i64))).into_owned()), bothSlots = true })), ..Default::default() }
    }), lua_line: 1632 });
    v.push(PatternEntry { pat: "if both equipped left and right ([%a%s]+) have a?n? ?([%a%s]+) modifiers?", val: TagVal::Fn(|caps: &Caps| {
        let itemSlotName = ns(caps.cap[0], "index local 'itemSlotName'");
        let conditionSubstring = ns(caps.cap[1], "index local 'conditionSubstring'");
        ModSpec { tag: Some(tag!({ type = ItemCondition, searchCond = &*conditionSubstring, itemSlot = Name::from(&*sub(itemSlotName, 1, Some((((len(itemSlotName) as f64) - 1.0) as i64))).into_owned()), bothSlots = true })), ..Default::default() }
    }), lua_line: 1633 });
    v.push(PatternEntry {
        pat: "if your equipped staff has a red and blue socket",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = MultiplierThreshold, var = "RedSocketInWeapon 1", threshold = 1.0 }),
                tag!({ type = MultiplierThreshold, var = "BlueSocketInWeapon 1", threshold = 1.0 }),
                tag!({ type = Condition, var = "UsingStaff" }),
            ],
            ..Default::default()
        }),
        lua_line: 1634,
    });
    v.push(PatternEntry { pat: "if there are no ([%a%s]+) modifiers on equipped ([%a%s]+)", val: TagVal::Fn(|caps: &Caps| {
        let conditionSubstring = ns(caps.cap[0], "index local 'conditionSubstring'");
        let itemSlotName = ns(caps.cap[1], "index local 'itemSlotName'");
        ModSpec { tag: Some(tag!({ type = ItemCondition, searchCond = &*conditionSubstring, itemSlot = Name::from(itemSlotName), neg = true })), ..Default::default() }
    }), lua_line: 1635 });
    v.push(PatternEntry { pat: "if there are no (%a+) modifiers on other equipped items", val: TagVal::Fn(|caps: &Caps| {
        let conditionSubstring = ns(caps.cap[0], "index local 'conditionSubstring'");
        ModSpec { tag: Some(tag!({ type = ItemCondition, searchCond = &*conditionSubstring, itemSlot = "{SlotName}", allSlots = true, excludeSelf = true, neg = true })), ..Default::default() }
    }), lua_line: 1636 });
    v.push(PatternEntry {
        pat: "if corrupted",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = ItemCondition, itemSlot = "{SlotName}", corruptedCond = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1637,
    });
    v.push(PatternEntry {
        pat: "with a normal item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "NormalItem", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1638,
    });
    v.push(PatternEntry {
        pat: "with a magic item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "MagicItem", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1639,
    });
    v.push(PatternEntry {
        pat: "with a rare item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "RareItem", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1640,
    });
    v.push(PatternEntry {
        pat: "with a unique item equipped",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "UniqueItem", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1641,
    });
    v.push(PatternEntry { pat: "if you wear no corrupted items", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "CorruptedItem", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1642 });
    v.push(PatternEntry { pat: "if no worn items are corrupted", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "CorruptedItem", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1643 });
    v.push(PatternEntry { pat: "if no equipped items are corrupted", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "CorruptedItem", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1644 });
    v.push(PatternEntry { pat: "if all worn items are corrupted", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "NonCorruptedItem", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1645 });
    v.push(PatternEntry { pat: "if all equipped items are corrupted", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "NonCorruptedItem", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1646 });
    v.push(PatternEntry {
        pat: "if equipped shield has at least (%d+)%% chance to block",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = StatThreshold, stat = "ShieldBlockChance", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1647,
    });
    v.push(PatternEntry {
        pat: "if you have (%d+) primordial items socketed or equipped",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = MultiplierThreshold, var = "PrimordialItem", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1648,
    });
    v.push(PatternEntry {
        pat: "if equipped helmet, body armour, gloves, and boots all have armour",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = StatThreshold, stat = "ArmourOnHelmet", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "ArmourOnBody Armour", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "ArmourOnGloves", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "ArmourOnBoots", threshold = 1.0 }),
            ],
            ..Default::default()
        }),
        lua_line: 1649,
    });
    v.push(PatternEntry {
        pat: "if equipped helmet, body armour, gloves, and boots all have evasion rating",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = StatThreshold, stat = "EvasionOnHelmet", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "EvasionOnBody Armour", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "EvasionOnGloves", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "EvasionOnBoots", threshold = 1.0 }),
            ],
            ..Default::default()
        }),
        lua_line: 1654,
    });
    // Player status conditions
    v.push(PatternEntry {
        pat: "if used while on low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LowLife" })),
            ..Default::default()
        }),
        lua_line: 1660,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? on low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LowLife" })),
            ..Default::default()
        }),
        lua_line: 1661,
    });
    v.push(PatternEntry {
        pat: "on reaching low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LowLife" })),
            ..Default::default()
        }),
        lua_line: 1662,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? not on low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LowLife", neg = true })),
            ..Default::default()
        }),
        lua_line: 1663,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? on low mana",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LowMana" })),
            ..Default::default()
        }),
        lua_line: 1664,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? not on low mana",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LowMana", neg = true })),
            ..Default::default()
        }),
        lua_line: 1665,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? on full life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "FullLife" })),
            ..Default::default()
        }),
        lua_line: 1666,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? not on full life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "FullLife", neg = true })),
            ..Default::default()
        }),
        lua_line: 1667,
    });
    v.push(PatternEntry { pat: "wh[ie][ln]e? no life is reserved", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "LifeReserved", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1668 });
    v.push(PatternEntry { pat: "wh[ie][ln]e? no mana is reserved", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "ManaReserved", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1669 });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? on full energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "FullEnergyShield" })),
            ..Default::default()
        }),
        lua_line: 1670,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? not on full energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "FullEnergyShield", neg = true })),
            ..Default::default()
        }),
        lua_line: 1671,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? you have energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveEnergyShield" })),
            ..Default::default()
        }),
        lua_line: 1672,
    });
    v.push(PatternEntry {
        pat: "wh[ie][ln]e? you have no energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveEnergyShield", neg = true })),
            ..Default::default()
        }),
        lua_line: 1673,
    });
    v.push(PatternEntry {
        pat: "if you have energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveEnergyShield" })),
            ..Default::default()
        }),
        lua_line: 1674,
    });
    v.push(PatternEntry {
        pat: "while stationary",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Stationary" })),
            ..Default::default()
        }),
        lua_line: 1675,
    });
    v.push(PatternEntry {
        pat: "while you are stationary",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "player", var = "Stationary" })),
            ..Default::default()
        }),
        lua_line: 1676,
    });
    v.push(PatternEntry {
        pat: "while moving",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Moving" })),
            ..Default::default()
        }),
        lua_line: 1677,
    });
    v.push(PatternEntry {
        pat: "while channelling",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Channelling" })),
            ..Default::default()
        }),
        lua_line: 1678,
    });
    v.push(PatternEntry {
        pat: "while channelling snipe",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Channelling" })),
            ..Default::default()
        }),
        lua_line: 1679,
    });
    v.push(PatternEntry {
        pat: "after channelling for (%d+) seconds?",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = MultiplierThreshold, var = "ChannellingTime", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1680,
    });
    v.push(PatternEntry {
        pat: "if you've been channelling for at least (%d+) seconds?",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = MultiplierThreshold, var = "ChannellingTime", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1681,
    });
    v.push(PatternEntry {
        pat: "if you've inflicted exposure recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AppliedExposureRecently" })),
            ..Default::default()
        }),
        lua_line: 1682,
    });
    v.push(PatternEntry { pat: "while you have no power charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "PowerCharges", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1683 });
    v.push(PatternEntry { pat: "while you have no frenzy charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "FrenzyCharges", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1684 });
    v.push(PatternEntry { pat: "while you have no endurance charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "EnduranceCharges", threshold = 0.0, upper = true })), ..Default::default() }), lua_line: 1685 });
    v.push(PatternEntry {
        pat: "while you have a power charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "PowerCharges", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1686,
    });
    v.push(PatternEntry {
        pat: "while you have a frenzy charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "FrenzyCharges", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1687,
    });
    v.push(PatternEntry {
        pat: "while you have an endurance charge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "EnduranceCharges", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1688,
    });
}

/// @pob Modules/ModParser.lua:1689-1738 h=2a01a4cd
fn modTagList_1689(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry { pat: "while at maximum power charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "PowerCharges", thresholdStat = "PowerChargesMax" })), ..Default::default() }), lua_line: 1689 });
    v.push(PatternEntry { pat: "while at maximum frenzy charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })), ..Default::default() }), lua_line: 1690 });
    v.push(PatternEntry { pat: "while on full frenzy charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMax" })), ..Default::default() }), lua_line: 1691 });
    v.push(PatternEntry { pat: "while at maximum endurance charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMax" })), ..Default::default() }), lua_line: 1692 });
    v.push(PatternEntry { pat: "while at minimum endurance charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "EnduranceCharges", thresholdStat = "EnduranceChargesMin", upper = true })), ..Default::default() }), lua_line: 1693 });
    v.push(PatternEntry { pat: "while at minimum power charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "PowerCharges", thresholdStat = "PowerChargesMin", upper = true })), ..Default::default() }), lua_line: 1694 });
    v.push(PatternEntry { pat: "while at minimum frenzy charges", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = StatThreshold, stat = "FrenzyCharges", thresholdStat = "FrenzyChargesMin", upper = true })), ..Default::default() }), lua_line: 1695 });
    v.push(PatternEntry {
        pat: "while at maximum rage",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveMaximumRage" })),
            ..Default::default()
        }),
        lua_line: 1696,
    });
    v.push(PatternEntry {
        pat: "while at maximum fortification",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveMaximumFortification" })),
            ..Default::default()
        }),
        lua_line: 1697,
    });
    v.push(PatternEntry {
        pat: "while you have at least (%d+) crab barriers",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(tag!({ type = StatThreshold, stat = "CrabBarriers", threshold = num })),
                ..Default::default()
            }
        }),
        lua_line: 1698,
    });
    v.push(PatternEntry {
        pat: "while you have at least (%d+) fortification",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = StatThreshold, stat = "FortificationStacks", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1699,
    });
    v.push(PatternEntry {
        pat: "while you have at least (%d+) total endurance, frenzy and power charges",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = MultiplierThreshold, var = "TotalCharges", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1700,
    });
    v.push(PatternEntry {
        pat: "while you have a totem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveTotem" })),
            ..Default::default()
        }),
        lua_line: 1701,
    });
    v.push(PatternEntry {
        pat: "while you have at least one nearby ally",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "NearbyAlly", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1702,
    });
    v.push(PatternEntry {
        pat: "while you have a linked target",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "LinkedTargets", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1703,
    });
    v.push(PatternEntry {
        pat: "while you have fortify",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Fortified" })),
            ..Default::default()
        }),
        lua_line: 1704,
    });
    v.push(PatternEntry {
        pat: "while you have phasing",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Phasing" })),
            ..Default::default()
        }),
        lua_line: 1705,
    });
    v.push(PatternEntry {
        pat: "while you have unbroken ward",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UnbrokenWard" })),
            ..Default::default()
        }),
        lua_line: 1706,
    });
    v.push(PatternEntry {
        pat: "while your ward is broken",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UnbrokenWard", neg = true })),
            ..Default::default()
        }),
        lua_line: 1707,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve suppressed spell damage recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SuppressedRecently" })),
            ..Default::default()
        }),
        lua_line: 1708,
    });
    v.push(PatternEntry {
        pat: "while you have elusive",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Elusive" })),
            ..Default::default()
        }),
        lua_line: 1709,
    });
    v.push(PatternEntry {
        pat: "while physical aegis is depleted",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "PhysicalAegisDepleted" })),
            ..Default::default()
        }),
        lua_line: 1710,
    });
    v.push(PatternEntry {
        pat: "during onslaught",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Onslaught" })),
            ..Default::default()
        }),
        lua_line: 1711,
    });
    v.push(PatternEntry {
        pat: "while you have onslaught",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Onslaught" })),
            ..Default::default()
        }),
        lua_line: 1712,
    });
    v.push(PatternEntry {
        pat: "while phasing",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Phasing" })),
            ..Default::default()
        }),
        lua_line: 1713,
    });
    v.push(PatternEntry {
        pat: "while you have tailwind",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Tailwind" })),
            ..Default::default()
        }),
        lua_line: 1714,
    });
    v.push(PatternEntry {
        pat: "while elusive",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Elusive" })),
            ..Default::default()
        }),
        lua_line: 1715,
    });
    v.push(PatternEntry {
        pat: "gain elusive",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = Condition, varList = &[n!("CanBeElusive"), n!("Elusive")][..] }),
            ),
            ..Default::default()
        }),
        lua_line: 1716,
    });
    v.push(PatternEntry {
        pat: "while you have arcane surge",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByArcaneSurge" })),
            ..Default::default()
        }),
        lua_line: 1717,
    });
    v.push(PatternEntry {
        pat: "while you have cat's stealth",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByCat'sStealth" })),
            ..Default::default()
        }),
        lua_line: 1718,
    });
    v.push(PatternEntry {
        pat: "while you have cat's agility",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByCat'sAgility" })),
            ..Default::default()
        }),
        lua_line: 1719,
    });
    v.push(PatternEntry {
        pat: "while you have avian's might",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByAvian'sMight" })),
            ..Default::default()
        }),
        lua_line: 1720,
    });
    v.push(PatternEntry {
        pat: "while you have avian's flight",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByAvian'sFlight" })),
            ..Default::default()
        }),
        lua_line: 1721,
    });
    v.push(PatternEntry { pat: "while affected by aspect of the cat", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("AffectedByCat'sStealth"), n!("AffectedByCat'sAgility")][..] })), ..Default::default() }), lua_line: 1722 });
    v.push(PatternEntry {
        pat: "while affected by a non%-vaal guard skill",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByNonVaalGuardSkill" })),
            ..Default::default()
        }),
        lua_line: 1723,
    });
    v.push(PatternEntry {
        pat: "if a non%-vaal guard buff was lost recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LostNonVaalBuffRecently" })),
            ..Default::default()
        }),
        lua_line: 1724,
    });
    v.push(PatternEntry {
        pat: "while affected by a guard skill buff",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByGuardSkill" })),
            ..Default::default()
        }),
        lua_line: 1725,
    });
    v.push(PatternEntry {
        pat: "while affected by a herald",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByHerald" })),
            ..Default::default()
        }),
        lua_line: 1726,
    });
    v.push(PatternEntry {
        pat: "while fortified",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Fortified" })),
            ..Default::default()
        }),
        lua_line: 1727,
    });
    v.push(PatternEntry {
        pat: "while in blood stance",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BloodStance" })),
            ..Default::default()
        }),
        lua_line: 1728,
    });
    v.push(PatternEntry {
        pat: "while in sand stance",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SandStance" })),
            ..Default::default()
        }),
        lua_line: 1729,
    });
    v.push(PatternEntry {
        pat: "while you have a bestial minion",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveBestialMinion" })),
            ..Default::default()
        }),
        lua_line: 1730,
    });
    v.push(PatternEntry {
        pat: "while you have infusion",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "InfusionActive" })),
            ..Default::default()
        }),
        lua_line: 1731,
    });
    v.push(PatternEntry {
        pat: "while focus?sed",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Focused" })),
            ..Default::default()
        }),
        lua_line: 1732,
    });
    v.push(PatternEntry {
        pat: "while leeching",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Leeching" })),
            ..Default::default()
        }),
        lua_line: 1733,
    });
    v.push(PatternEntry {
        pat: "while leeching life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LeechingLife" })),
            ..Default::default()
        }),
        lua_line: 1734,
    });
    v.push(PatternEntry {
        pat: "while leeching energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LeechingEnergyShield" })),
            ..Default::default()
        }),
        lua_line: 1735,
    });
    v.push(PatternEntry {
        pat: "while leeching mana",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "LeechingMana" })),
            ..Default::default()
        }),
        lua_line: 1736,
    });
    v.push(PatternEntry {
        pat: "while using a flask",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask" })),
            ..Default::default()
        }),
        lua_line: 1737,
    });
    v.push(PatternEntry {
        pat: "during effect",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask" })),
            ..Default::default()
        }),
        lua_line: 1738,
    });
}

/// @pob Modules/ModParser.lua:1739-1788 h=531425de
fn modTagList_1739(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "during flask effect",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask" })),
            ..Default::default()
        }),
        lua_line: 1739,
    });
    v.push(PatternEntry {
        pat: "during any flask effect",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask" })),
            ..Default::default()
        }),
        lua_line: 1740,
    });
    v.push(PatternEntry {
        pat: "while under no flask effects",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask", neg = true })),
            ..Default::default()
        }),
        lua_line: 1741,
    });
    v.push(PatternEntry {
        pat: "while affected by no flasks",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask", neg = true })),
            ..Default::default()
        }),
        lua_line: 1742,
    });
    v.push(PatternEntry {
        pat: "during effect of any mana flask",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingManaFlask" })),
            ..Default::default()
        }),
        lua_line: 1743,
    });
    v.push(PatternEntry {
        pat: "during effect of any life flask",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingLifeFlask" })),
            ..Default::default()
        }),
        lua_line: 1744,
    });
    v.push(PatternEntry {
        pat: "if you've used a life flask in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingLifeFlask" })),
            ..Default::default()
        }),
        lua_line: 1745,
    });
    v.push(PatternEntry {
        pat: "if you've used a mana flask in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingManaFlask" })),
            ..Default::default()
        }),
        lua_line: 1746,
    });
    v.push(PatternEntry { pat: "during effect of any life or mana flask", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("UsingManaFlask"), n!("UsingLifeFlask")][..] })), ..Default::default() }), lua_line: 1747 });
    v.push(PatternEntry {
        pat: "while you have an active tincture",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingTincture" })),
            ..Default::default()
        }),
        lua_line: 1748,
    });
    v.push(PatternEntry {
        pat: "while you have a tincture active",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingTincture" })),
            ..Default::default()
        }),
        lua_line: 1749,
    });
    v.push(PatternEntry {
        pat: "with at least one (%w+) grafted to you",
        val: TagVal::Fn(|caps: &Caps| {
            let graft = ns(caps.cap[0], "index local 'graft'");
            ModSpec {
                tag: Some(tag!({ type = Condition, var = cat!("Using", firstToUpper(graft)) })),
                ..Default::default()
            }
        }),
        lua_line: 1750,
    });
    v.push(PatternEntry {
        pat: "while on consecrated ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnConsecratedGround" })),
            ..Default::default()
        }),
        lua_line: 1751,
    });
    v.push(PatternEntry {
        pat: "while on caustic ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnCausticGround" })),
            ..Default::default()
        }),
        lua_line: 1752,
    });
    v.push(PatternEntry {
        pat: "when you create consecrated ground",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1753,
    });
    v.push(PatternEntry {
        pat: "on burning ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnBurningGround" })),
            ..Default::default()
        }),
        lua_line: 1754,
    });
    v.push(PatternEntry {
        pat: "while on burning ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnBurningGround" })),
            ..Default::default()
        }),
        lua_line: 1755,
    });
    v.push(PatternEntry {
        pat: "on chilled ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnChilledGround" })),
            ..Default::default()
        }),
        lua_line: 1756,
    });
    v.push(PatternEntry {
        pat: "on shocked ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnShockedGround" })),
            ..Default::default()
        }),
        lua_line: 1757,
    });
    v.push(PatternEntry {
        pat: "while in a caustic cloud",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnCausticCloud" })),
            ..Default::default()
        }),
        lua_line: 1758,
    });
    v.push(PatternEntry {
        pat: "while blinded",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "Blinded" }),
                tag!({ type = Condition, var = "CannotBeBlinded", neg = true }),
            ],
            ..Default::default()
        }),
        lua_line: 1759,
    });
    v.push(PatternEntry {
        pat: "while burning",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Burning" })),
            ..Default::default()
        }),
        lua_line: 1760,
    });
    v.push(PatternEntry {
        pat: "while ignited",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Ignited" })),
            ..Default::default()
        }),
        lua_line: 1761,
    });
    v.push(PatternEntry {
        pat: "while you are ignited",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Ignited" })),
            ..Default::default()
        }),
        lua_line: 1762,
    });
    v.push(PatternEntry {
        pat: "while chilled",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1763,
    });
    v.push(PatternEntry {
        pat: "while you are chilled",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1764,
    });
    v.push(PatternEntry {
        pat: "while frozen",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Frozen" })),
            ..Default::default()
        }),
        lua_line: 1765,
    });
    v.push(PatternEntry {
        pat: "while shocked",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1766,
    });
    v.push(PatternEntry {
        pat: "while you are shocked",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1767,
    });
    v.push(PatternEntry {
        pat: "while you are bleeding",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Bleeding" })),
            ..Default::default()
        }),
        lua_line: 1768,
    });
    v.push(PatternEntry { pat: "while not ignited, frozen or shocked", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("Ignited"), n!("Frozen"), n!("Shocked")][..], neg = true })), ..Default::default() }), lua_line: 1769 });
    v.push(PatternEntry {
        pat: "while bleeding",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Bleeding" })),
            ..Default::default()
        }),
        lua_line: 1770,
    });
    v.push(PatternEntry {
        pat: "while poisoned",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Poisoned" })),
            ..Default::default()
        }),
        lua_line: 1771,
    });
    v.push(PatternEntry {
        pat: "wh[ei][nl][ e] ?you are poisoned",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Poisoned" })),
            ..Default::default()
        }),
        lua_line: 1772,
    });
    v.push(PatternEntry {
        pat: "while cursed",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Cursed" })),
            ..Default::default()
        }),
        lua_line: 1773,
    });
    v.push(PatternEntry {
        pat: "while not cursed",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Cursed", neg = true })),
            ..Default::default()
        }),
        lua_line: 1774,
    });
    v.push(PatternEntry {
        pat: "while there is only one nearby enemy",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Multiplier, var = "NearbyEnemies", limit = 1.0 }),
                tag!({ type = Condition, var = "OnlyOneNearbyEnemy" }),
            ],
            ..Default::default()
        }),
        lua_line: 1775,
    });
    v.push(PatternEntry {
        pat: "while at least (%d+) enemies are nearby",
        val: TagVal::Fn(|caps: &Caps| {
            let num = caps.num;
            ModSpec {
                tag: Some(
                    tag!({ type = MultiplierThreshold, var = "NearbyEnemies", threshold = num }),
                ),
                ..Default::default()
            }
        }),
        lua_line: 1776,
    });
    v.push(PatternEntry { pat: "while t?h?e?r?e? ?i?s? ?a rare or unique enemy i?s? ?nearby", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("NearbyRareOrUniqueEnemy"), n!("RareOrUnique")][..] })), ..Default::default() }), lua_line: 1777 });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HitRecently" })),
            ..Default::default()
        }),
        lua_line: 1778,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve hit an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HitRecently" })),
            ..Default::default()
        }),
        lua_line: 1779,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve hit with your main hand weapon recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HitRecentlyWithWeapon" })),
            ..Default::default()
        }),
        lua_line: 1780,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve hit with your off hand weapon recently",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "HitRecentlyWithWeapon" }),
                tag!({ type = Condition, var = "DualWielding" }),
            ],
            ..Default::default()
        }),
        lua_line: 1781,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve hit a cursed enemy recently",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "HitRecently" }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" }),
            ],
            ..Default::default()
        }),
        lua_line: 1782,
    });
    v.push(PatternEntry { pat: "when you or your totems hit an enemy with a spell", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("HitSpellRecently"), n!("TotemsHitSpellRecently")][..] })), ..Default::default() }), lua_line: 1783 });
    v.push(PatternEntry {
        pat: "on hit with spells",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HitSpellRecently" })),
            ..Default::default()
        }),
        lua_line: 1784,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve crit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritRecently" })),
            ..Default::default()
        }),
        lua_line: 1785,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve dealt a critical strike recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritRecently" })),
            ..Default::default()
        }),
        lua_line: 1786,
    });
    v.push(PatternEntry {
        pat: "when you deal a critical strike",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritRecently" })),
            ..Default::default()
        }),
        lua_line: 1787,
    });
    // Replica Kongor's
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve dealt a critical strike with this weapon recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritRecently" })),
            ..Default::default()
        }),
        lua_line: 1788,
    });
}

/// @pob Modules/ModParser.lua:1789-1838 h=5f174b01
fn modTagList_1789(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve crit in the past 8 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritInPast8Sec" })),
            ..Default::default()
        }),
        lua_line: 1789,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve dealt a crit in the past 8 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritInPast8Sec" })),
            ..Default::default()
        }),
        lua_line: 1790,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve dealt a critical strike in the past 8 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritInPast8Sec" })),
            ..Default::default()
        }),
        lua_line: 1791,
    });
    v.push(PatternEntry {
        pat: "if you haven't crit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1792,
    });
    v.push(PatternEntry {
        pat: "if you haven't dealt a critical strike recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1793,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve dealt a non%-critical strike recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "NonCritRecently" })),
            ..Default::default()
        }),
        lua_line: 1794,
    });
    v.push(PatternEntry {
        pat: "if your skills have dealt a critical strike recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SkillCritRecently" })),
            ..Default::default()
        }),
        lua_line: 1795,
    });
    v.push(PatternEntry {
        pat: "if you dealt a critical strike with a herald skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritWithHeraldSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1796,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve dealt a critical strike with a two handed melee weapon recently",
        val: TagVal::Spec(ModSpec {
            flags: bor(ModFlag::Weapon2H, ModFlag::WeaponMelee),
            tag: Some(tag!({ type = Condition, var = "CritRecently" })),
            ..Default::default()
        }),
        lua_line: 1797,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve killed recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledRecently" })),
            ..Default::default()
        }),
        lua_line: 1798,
    });
    v.push(PatternEntry {
        pat: "on killing taunted enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledTauntedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1799,
    });
    v.push(PatternEntry {
        pat: "on kill",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledRecently" })),
            ..Default::default()
        }),
        lua_line: 1800,
    });
    v.push(PatternEntry {
        pat: "on melee kill",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::WeaponMelee,
            tag: Some(tag!({ type = Condition, var = "KilledRecently" })),
            ..Default::default()
        }),
        lua_line: 1801,
    });
    v.push(PatternEntry {
        pat: "when you kill an enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledRecently" })),
            ..Default::default()
        }),
        lua_line: 1802,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve killed an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledRecently" })),
            ..Default::default()
        }),
        lua_line: 1803,
    });
    v.push(PatternEntry { pat: "if you[' ]h?a?ve killed at least (%d) enemies recently", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "EnemyKilledRecently", threshold = num })), ..Default::default() }
    }), lua_line: 1804 });
    v.push(PatternEntry {
        pat: "if you haven't killed recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1805,
    });
    v.push(PatternEntry { pat: "if you or your totems have killed recently", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("KilledRecently"), n!("TotemsKilledRecently")][..] })), ..Default::default() }), lua_line: 1806 });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve thrown a trap or mine recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "TrapOrMineThrownRecently" })),
            ..Default::default()
        }),
        lua_line: 1807,
    });
    v.push(PatternEntry {
        pat: "on throwing a trap",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "TrapOrMineThrownRecently" })),
            ..Default::default()
        }),
        lua_line: 1808,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve killed a maimed enemy recently",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "KilledRecently" }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Maimed" }),
            ],
            ..Default::default()
        }),
        lua_line: 1809,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve killed a cursed enemy recently",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "KilledRecently" }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" }),
            ],
            ..Default::default()
        }),
        lua_line: 1810,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve killed a bleeding enemy recently",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "KilledRecently" }),
                tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" }),
            ],
            ..Default::default()
        }),
        lua_line: 1811,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve killed an enemy affected by your damage over time recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "KilledAffectedByDotRecently" })),
            ..Default::default()
        }),
        lua_line: 1812,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve frozen an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "FrozenEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1813,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve chilled an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ChilledEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1814,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve ignited an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "IgnitedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1815,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve shocked an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ShockedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1816,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve stunned an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "StunnedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1817,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve stunned an enemy with a two handed melee weapon recently",
        val: TagVal::Spec(ModSpec {
            flags: bor(ModFlag::Weapon2H, ModFlag::WeaponMelee),
            tag: Some(tag!({ type = Condition, var = "StunnedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1818,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve been hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitRecently" })),
            ..Default::default()
        }),
        lua_line: 1819,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve been hit by an attack recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitByAttackRecently" })),
            ..Default::default()
        }),
        lua_line: 1820,
    });
    v.push(PatternEntry {
        pat: "if you were hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitRecently" })),
            ..Default::default()
        }),
        lua_line: 1821,
    });
    v.push(PatternEntry {
        pat: "if you were damaged by a hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitRecently" })),
            ..Default::default()
        }),
        lua_line: 1822,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taken a critical strike recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenCritRecently" })),
            ..Default::default()
        }),
        lua_line: 1823,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taken a savage hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenSavageHitRecently" })),
            ..Default::default()
        }),
        lua_line: 1824,
    });
    v.push(PatternEntry {
        pat: "if you have ?n[o']t been hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1825,
    });
    v.push(PatternEntry {
        pat: "if you have ?n[o']t been hit by an attack recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitByAttackRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1826,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taken no damage from hits recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1827,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taken fire damage from a hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HitByFireDamageRecently" })),
            ..Default::default()
        }),
        lua_line: 1828,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taken fire damage from an enemy hit recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "TakenFireDamageFromEnemyHitRecently" })),
            ..Default::default()
        }),
        lua_line: 1829,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taken spell damage recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HitBySpellDamageRecently" })),
            ..Default::default()
        }),
        lua_line: 1830,
    });
    v.push(PatternEntry {
        pat: "if you haven't taken damage recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BeenHitRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1831,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve blocked recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedRecently" })),
            ..Default::default()
        }),
        lua_line: 1832,
    });
    v.push(PatternEntry {
        pat: "if you haven't blocked recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1833,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve blocked an attack recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedAttackRecently" })),
            ..Default::default()
        }),
        lua_line: 1834,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve blocked attack damage recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedAttackRecently" })),
            ..Default::default()
        }),
        lua_line: 1835,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve blocked a spell recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedSpellRecently" })),
            ..Default::default()
        }),
        lua_line: 1836,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve blocked spell damage recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedSpellRecently" })),
            ..Default::default()
        }),
        lua_line: 1837,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve blocked damage from a unique enemy in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BlockedHitFromUniqueEnemyInPast10Sec" })),
            ..Default::default()
        }),
        lua_line: 1838,
    });
}

/// @pob Modules/ModParser.lua:1839-1888 h=5f719789
fn modTagList_1839(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve attacked recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AttackedRecently" })),
            ..Default::default()
        }),
        lua_line: 1839,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve cast a spell recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CastSpellRecently" })),
            ..Default::default()
        }),
        lua_line: 1840,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve been stunned while casting recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "StunnedWhileCastingRecently" })),
            ..Default::default()
        }),
        lua_line: 1841,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve consumed a corpse recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConsumedCorpseRecently" })),
            ..Default::default()
        }),
        lua_line: 1842,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve cursed an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CursedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1843,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve cast a mark spell recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CastMarkRecently" })),
            ..Default::default()
        }),
        lua_line: 1844,
    });
    v.push(PatternEntry {
        pat: "if you have ?n[o']t consumed a corpse recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConsumedCorpseRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1845,
    });
    v.push(PatternEntry {
        pat: "for each corpse consumed recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "CorpseConsumedRecently" })),
            ..Default::default()
        }),
        lua_line: 1846,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve taunted an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "TauntedEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1847,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1848,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a travel skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedTravelSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1849,
    });
    v.push(PatternEntry { pat: "for each skill you've used recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "SkillUsedRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1850 });
    v.push(PatternEntry {
        pat: "for each different non%-instant spell you[' ]h?a?ve cast recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NonInstantSpellCastRecently" })),
            ..Default::default()
        }),
        lua_line: 1851,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a warcry recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedWarcryRecently" })),
            ..Default::default()
        }),
        lua_line: 1852,
    });
    // LUA-QUIRK: duplicate key "when you warcry" (L1853) is overwritten by L1858; the Lua table keeps the last value
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve warcried recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedWarcryRecently" })),
            ..Default::default()
        }),
        lua_line: 1854,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve not warcried recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedWarcryRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1855,
    });
    v.push(PatternEntry {
        pat: "for each time you[' ]h?a?ve warcried recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "WarcryUsedRecently" })),
            ..Default::default()
        }),
        lua_line: 1856,
    });
    v.push(PatternEntry {
        pat: "for each warcry exerting them",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "ExertingWarcryCount" })),
            ..Default::default()
        }),
        lua_line: 1857,
    });
    v.push(PatternEntry {
        pat: "when you warcry",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedWarcryRecently" })),
            ..Default::default()
        }),
        lua_line: 1858,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve warcried in the past 8 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedWarcryInPast8Seconds" })),
            ..Default::default()
        }),
        lua_line: 1859,
    });
    v.push(PatternEntry { pat: "for each second you've been affected by a warcry buff, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "AffectedByWarcryBuffDuration", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1860 });
    v.push(PatternEntry { pat: "for each of your mines detonated recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "MineDetonatedRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1861 });
    v.push(PatternEntry { pat: "[fp][oe]r ?e?a?c?h? mine detonated recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "MineDetonatedRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1862 });
    v.push(PatternEntry { pat: "[fp][oe]r ?e?a?c?h? mine detonated recently, up to (%d+)%% per second", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "MineDetonatedRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1863 });
    v.push(PatternEntry {
        pat: "[fp][oe]r ?e?a?c?h? mine detonated recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "MineDetonatedRecently" })),
            ..Default::default()
        }),
        lua_line: 1864,
    });
    v.push(PatternEntry { pat: "for each of your traps triggered recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "TrapTriggeredRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1865 });
    v.push(PatternEntry { pat: "for each trap triggered recently, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "TrapTriggeredRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1866 });
    v.push(PatternEntry { pat: "for each trap triggered recently, up to (%d+)%% per second", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "TrapTriggeredRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1867 });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a fire skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedFireSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1868,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a cold skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedColdSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1869,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a fire skill in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedFireSkillInPast10Sec" })),
            ..Default::default()
        }),
        lua_line: 1870,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a cold skill in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedColdSkillInPast10Sec" })),
            ..Default::default()
        }),
        lua_line: 1871,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a lightning skill in the past 10 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedLightningSkillInPast10Sec" })),
            ..Default::default()
        }),
        lua_line: 1872,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve summoned a totem recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SummonedTotemRecently" })),
            ..Default::default()
        }),
        lua_line: 1873,
    });
    v.push(PatternEntry {
        pat: "when you summon a totem",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SummonedTotemRecently" })),
            ..Default::default()
        }),
        lua_line: 1874,
    });
    v.push(PatternEntry {
        pat: "if you summoned a golem in the past 8 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SummonedGolemInPast8Sec" })),
            ..Default::default()
        }),
        lua_line: 1875,
    });
    v.push(PatternEntry {
        pat: "if you haven't summoned a totem in the past 2 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "NoSummonedTotemsInPastTwoSeconds" })),
            ..Default::default()
        }),
        lua_line: 1876,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a minion skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedMinionSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1877,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a movement skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedMovementSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1878,
    });
    v.push(PatternEntry {
        pat: "when you use a movement skill",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedMovementSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1879,
    });
    v.push(PatternEntry {
        pat: "if you haven't cast dash recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CastDashRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1880,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve cast dash recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CastDashRecently" })),
            ..Default::default()
        }),
        lua_line: 1881,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a vaal skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedVaalSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1882,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a socketed vaal skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedVaalSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1883,
    });
    v.push(PatternEntry {
        pat: "when you use a vaal skill",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedVaalSkillRecently" })),
            ..Default::default()
        }),
        lua_line: 1884,
    });
    v.push(PatternEntry {
        pat: "if you haven't used a brand skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedBrandRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1885,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a brand skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedBrandRecently" })),
            ..Default::default()
        }),
        lua_line: 1886,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve used a retaliation skill recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedRetaliationRecently" })),
            ..Default::default()
        }),
        lua_line: 1887,
    });
    v.push(PatternEntry { pat: "if you[' ]h?a?ve spent (%d+) total mana recently", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "ManaSpentRecently", threshold = num })), ..Default::default() }
    }), lua_line: 1888 });
}

/// @pob Modules/ModParser.lua:1889-1941 h=9014b140
fn modTagList_1889(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve spent life recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = MultiplierThreshold, var = "LifeSpentRecently", threshold = 1.0 }),
            ),
            ..Default::default()
        }),
        lua_line: 1889,
    });
    v.push(PatternEntry { pat: "for %d+ seconds after spending a total of (%d+) mana", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "ManaSpentRecently", threshold = num })), ..Default::default() }
    }), lua_line: 1890 });
    v.push(PatternEntry {
        pat: "if you've impaled an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ImpaledRecently" })),
            ..Default::default()
        }),
        lua_line: 1891,
    });
    v.push(PatternEntry {
        pat: "if you've changed stance recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ChangedStanceRecently" })),
            ..Default::default()
        }),
        lua_line: 1892,
    });
    v.push(PatternEntry {
        pat: "if you've gained a power charge recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "GainedPowerChargeRecently" })),
            ..Default::default()
        }),
        lua_line: 1893,
    });
    v.push(PatternEntry {
        pat: "if you haven't gained a power charge recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "GainedPowerChargeRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1894,
    });
    v.push(PatternEntry {
        pat: "if you haven't gained a frenzy charge recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "GainedFrenzyChargeRecently", neg = true })),
            ..Default::default()
        }),
        lua_line: 1895,
    });
    v.push(PatternEntry {
        pat: "if you've stopped taking damage over time recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "StoppedTakingDamageOverTimeRecently" })),
            ..Default::default()
        }),
        lua_line: 1896,
    });
    v.push(PatternEntry {
        pat: "if you've used an amethyst flask recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedAmethystFlaskRecently" })),
            ..Default::default()
        }),
        lua_line: 1897,
    });
    v.push(PatternEntry {
        pat: "if you've used a ruby flask recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedRubyFlaskRecently" })),
            ..Default::default()
        }),
        lua_line: 1898,
    });
    v.push(PatternEntry {
        pat: "if you've used a sapphire flask recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedSapphireFlaskRecently" })),
            ..Default::default()
        }),
        lua_line: 1899,
    });
    v.push(PatternEntry {
        pat: "if you've used a topaz flask recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsedTopazFlaskRecently" })),
            ..Default::default()
        }),
        lua_line: 1900,
    });
    v.push(PatternEntry {
        pat: "during soul gain prevention",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SoulGainPrevention" })),
            ..Default::default()
        }),
        lua_line: 1901,
    });
    v.push(PatternEntry {
        pat: "if you detonated mines recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DetonatedMinesRecently" })),
            ..Default::default()
        }),
        lua_line: 1902,
    });
    v.push(PatternEntry {
        pat: "if you detonated a mine recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DetonatedMinesRecently" })),
            ..Default::default()
        }),
        lua_line: 1903,
    });
    v.push(PatternEntry {
        pat: "if you[' ]h?a?ve detonated a mine recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DetonatedMinesRecently" })),
            ..Default::default()
        }),
        lua_line: 1904,
    });
    v.push(PatternEntry {
        pat: "when your mine is detonated targeting an enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "DetonatedMinesRecently" })),
            ..Default::default()
        }),
        lua_line: 1905,
    });
    v.push(PatternEntry {
        pat: "when your trap is triggered by an enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "TriggeredTrapsRecently" })),
            ..Default::default()
        }),
        lua_line: 1906,
    });
    v.push(PatternEntry {
        pat: "if energy shield recharge has started recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "EnergyShieldRechargeRecently" })),
            ..Default::default()
        }),
        lua_line: 1907,
    });
    v.push(PatternEntry {
        pat: "if energy shield recharge has started in the past 2 seconds",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "EnergyShieldRechargePastTwoSec" })),
            ..Default::default()
        }),
        lua_line: 1908,
    });
    v.push(PatternEntry {
        pat: "when cast on frostbolt",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CastOnFrostbolt" })),
            ..Default::default()
        }),
        lua_line: 1909,
    });
    v.push(PatternEntry { pat: "branded enemy's", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "BrandsAttachedToEnemy", threshold = 1.0 })), ..Default::default() }), lua_line: 1910 });
    v.push(PatternEntry { pat: "to enemies they're attached to", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = MultiplierThreshold, var = "BrandsAttachedToEnemy", threshold = 1.0 })), ..Default::default() }), lua_line: 1911 });
    v.push(PatternEntry { pat: "for each hit you've taken recently up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "BeenHitRecently", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1912 });
    v.push(PatternEntry {
        pat: "per enemy hit taken recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "BeenHitRecently" })),
            ..Default::default()
        }),
        lua_line: 1913,
    });
    v.push(PatternEntry { pat: "for each nearby enemy, up to (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "NearbyEnemies", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1914 });
    v.push(PatternEntry { pat: "for each nearby enemy, up to a maximum of (%d+)%%", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = Multiplier, var = "NearbyEnemies", limit = num, limitTotal = true })), ..Default::default() }
    }), lua_line: 1915 });
    v.push(PatternEntry {
        pat: "while you have iron reflexes",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveIronReflexes" })),
            ..Default::default()
        }),
        lua_line: 1916,
    });
    v.push(PatternEntry {
        pat: "while you do not have iron reflexes",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveIronReflexes", neg = true })),
            ..Default::default()
        }),
        lua_line: 1917,
    });
    v.push(PatternEntry {
        pat: "while you have elemental overload",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveElementalOverload" })),
            ..Default::default()
        }),
        lua_line: 1918,
    });
    v.push(PatternEntry {
        pat: "while you do not have elemental overload",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveElementalOverload", neg = true })),
            ..Default::default()
        }),
        lua_line: 1919,
    });
    v.push(PatternEntry {
        pat: "while you have resolute technique",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveResoluteTechnique" })),
            ..Default::default()
        }),
        lua_line: 1920,
    });
    v.push(PatternEntry {
        pat: "while you do not have resolute technique",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveResoluteTechnique", neg = true })),
            ..Default::default()
        }),
        lua_line: 1921,
    });
    v.push(PatternEntry {
        pat: "while you have avatar of fire",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveAvatarOfFire" })),
            ..Default::default()
        }),
        lua_line: 1922,
    });
    v.push(PatternEntry {
        pat: "while you do not have avatar of fire",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "HaveAvatarOfFire", neg = true })),
            ..Default::default()
        }),
        lua_line: 1923,
    });
    v.push(PatternEntry { pat: "if you have a summoned golem", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("HavePhysicalGolem"), n!("HaveLightningGolem"), n!("HaveColdGolem"), n!("HaveFireGolem"), n!("HaveChaosGolem"), n!("HaveCarrionGolem")][..] })), ..Default::default() }), lua_line: 1924 });
    v.push(PatternEntry { pat: "while you have a summoned golem", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = Condition, varList = &[n!("HavePhysicalGolem"), n!("HaveLightningGolem"), n!("HaveColdGolem"), n!("HaveFireGolem"), n!("HaveChaosGolem"), n!("HaveCarrionGolem")][..] })), ..Default::default() }), lua_line: 1925 });
    v.push(PatternEntry {
        pat: "if a minion has died recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "MinionsDiedRecently" })),
            ..Default::default()
        }),
        lua_line: 1926,
    });
    v.push(PatternEntry {
        pat: "if a minion has been killed recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "MinionsDiedRecently" })),
            ..Default::default()
        }),
        lua_line: 1927,
    });
    v.push(PatternEntry {
        pat: "while you have sacrificial zeal",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SacrificialZeal" })),
            ..Default::default()
        }),
        lua_line: 1928,
    });
    v.push(PatternEntry {
        pat: "while sane",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Insane", neg = true })),
            ..Default::default()
        }),
        lua_line: 1929,
    });
    v.push(PatternEntry {
        pat: "while insane",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Insane" })),
            ..Default::default()
        }),
        lua_line: 1930,
    });
    v.push(PatternEntry {
        pat: "while you have defiance",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "Defiance", threshold = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1931,
    });
    v.push(PatternEntry {
        pat: "while affected by glorious madness",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByGloriousMadness" })),
            ..Default::default()
        }),
        lua_line: 1932,
    });
    v.push(PatternEntry {
        pat: "if you have reserved life and mana",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = StatThreshold, stat = "LifeReserved", threshold = 1.0 }),
                tag!({ type = StatThreshold, stat = "ManaReserved", threshold = 1.0 }),
            ],
            ..Default::default()
        }),
        lua_line: 1933,
    });
    v.push(PatternEntry {
        pat: "if you've shattered an enemy recently",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ShatteredEnemyRecently" })),
            ..Default::default()
        }),
        lua_line: 1936,
    });
    v.push(PatternEntry {
        pat: "while affected by no flasks?",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask", neg = true })),
            ..Default::default()
        }),
        lua_line: 1937,
    });
    v.push(PatternEntry {
        pat: "while affected by flasks?",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "UsingFlask" })),
            ..Default::default()
        }),
        lua_line: 1938,
    });
    // Enemy status conditions
    v.push(PatternEntry {
        pat: "at close range",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AtCloseRange" })),
            ..Default::default()
        }),
        lua_line: 1940,
    });
    v.push(PatternEntry {
        pat: "not at close range",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AtCloseRange", neg = true })),
            ..Default::default()
        }),
        lua_line: 1941,
    });
}

/// @pob Modules/ModParser.lua:1942-1991 h=a8e8bbe7
fn modTagList_1942(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "against rare and unique enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })),
            ..Default::default()
        }),
        lua_line: 1942,
    });
    v.push(PatternEntry {
        pat: "by s?l?a?i?n? rare [ao][nr]d? unique enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })),
            ..Default::default()
        }),
        lua_line: 1943,
    });
    v.push(PatternEntry {
        pat: "against unique enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })),
            ..Default::default()
        }),
        lua_line: 1944,
    });
    v.push(PatternEntry {
        pat: "against enemies on full life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "FullLife" })),
            ..Default::default()
        }),
        lua_line: 1945,
    });
    v.push(PatternEntry {
        pat: "against enemies that are on full life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "FullLife" })),
            ..Default::default()
        }),
        lua_line: 1946,
    });
    v.push(PatternEntry {
        pat: "against enemies on low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "LowLife" })),
            ..Default::default()
        }),
        lua_line: 1947,
    });
    v.push(PatternEntry {
        pat: "against enemies that are on low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "LowLife" })),
            ..Default::default()
        }),
        lua_line: 1948,
    });
    v.push(PatternEntry {
        pat: "against enemies that are not on low life",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = ActorCondition, actor = "enemy", var = "LowLife", neg = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1949,
    });
    v.push(PatternEntry {
        pat: "to enemies which have energy shield",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "HaveEnergyShield" })),
            keywordFlags: bor(KeywordFlag::Hit, KeywordFlag::Ailment),
            ..Default::default()
        }),
        lua_line: 1950,
    });
    v.push(PatternEntry {
        pat: "against cursed enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })),
            ..Default::default()
        }),
        lua_line: 1951,
    });
    v.push(PatternEntry {
        pat: "against stunned enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Stunned" })),
            ..Default::default()
        }),
        lua_line: 1952,
    });
    v.push(PatternEntry {
        pat: "on cursed enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })),
            ..Default::default()
        }),
        lua_line: 1953,
    });
    v.push(PatternEntry {
        pat: "of cursed enemies'",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })),
            ..Default::default()
        }),
        lua_line: 1954,
    });
    v.push(PatternEntry {
        pat: "when hitting cursed enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })),
            keywordFlags: KeywordFlag::Hit,
            ..Default::default()
        }),
        lua_line: 1955,
    });
    v.push(PatternEntry {
        pat: "from cursed enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Cursed" })),
            ..Default::default()
        }),
        lua_line: 1956,
    });
    v.push(PatternEntry {
        pat: "against marked enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })),
            ..Default::default()
        }),
        lua_line: 1957,
    });
    v.push(PatternEntry {
        pat: "when hitting marked enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })),
            keywordFlags: KeywordFlag::Hit,
            ..Default::default()
        }),
        lua_line: 1958,
    });
    v.push(PatternEntry {
        pat: "from marked enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Marked" })),
            ..Default::default()
        }),
        lua_line: 1959,
    });
    v.push(PatternEntry {
        pat: "against taunted enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Taunted" })),
            ..Default::default()
        }),
        lua_line: 1960,
    });
    v.push(PatternEntry {
        pat: "against bleeding enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })),
            ..Default::default()
        }),
        lua_line: 1961,
    });
    v.push(PatternEntry {
        pat: "you inflict on bleeding enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })),
            ..Default::default()
        }),
        lua_line: 1962,
    });
    v.push(PatternEntry {
        pat: "to bleeding enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })),
            ..Default::default()
        }),
        lua_line: 1963,
    });
    v.push(PatternEntry {
        pat: "from bleeding enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Bleeding" })),
            ..Default::default()
        }),
        lua_line: 1964,
    });
    v.push(PatternEntry {
        pat: "against poisoned enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })),
            ..Default::default()
        }),
        lua_line: 1965,
    });
    v.push(PatternEntry {
        pat: "you inflict on poisoned enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })),
            ..Default::default()
        }),
        lua_line: 1966,
    });
    v.push(PatternEntry {
        pat: "to poisoned enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Poisoned" })),
            ..Default::default()
        }),
        lua_line: 1967,
    });
    v.push(PatternEntry { pat: "against enemies affected by (%d+) or more poisons", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, actor = "enemy", var = "PoisonStack", threshold = num })), ..Default::default() }
    }), lua_line: 1968 });
    v.push(PatternEntry { pat: "against enemies affected by at least (%d+) poisons", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, actor = "enemy", var = "PoisonStack", threshold = num })), ..Default::default() }
    }), lua_line: 1969 });
    v.push(PatternEntry {
        pat: "against hindered enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Hindered" })),
            ..Default::default()
        }),
        lua_line: 1970,
    });
    v.push(PatternEntry {
        pat: "against maimed enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Maimed" })),
            ..Default::default()
        }),
        lua_line: 1971,
    });
    v.push(PatternEntry {
        pat: "you inflict on maimed enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Maimed" })),
            ..Default::default()
        }),
        lua_line: 1972,
    });
    v.push(PatternEntry {
        pat: "against blinded enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Blinded" })),
            ..Default::default()
        }),
        lua_line: 1973,
    });
    v.push(PatternEntry {
        pat: "against excommunicated enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Excommunicated" })),
            ..Default::default()
        }),
        lua_line: 1974,
    });
    v.push(PatternEntry {
        pat: "from blinded enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Blinded" })),
            ..Default::default()
        }),
        lua_line: 1975,
    });
    v.push(PatternEntry {
        pat: "against burning enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Burning" })),
            ..Default::default()
        }),
        lua_line: 1976,
    });
    v.push(PatternEntry {
        pat: "against ignited enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })),
            ..Default::default()
        }),
        lua_line: 1977,
    });
    v.push(PatternEntry {
        pat: "to ignited enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Ignited" })),
            ..Default::default()
        }),
        lua_line: 1978,
    });
    v.push(PatternEntry {
        pat: "against shocked enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1979,
    });
    v.push(PatternEntry {
        pat: "you inflict on shocked enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1980,
    });
    v.push(PatternEntry {
        pat: "to shocked enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1981,
    });
    v.push(PatternEntry {
        pat: "inflicted on shocked enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1982,
    });
    v.push(PatternEntry {
        pat: "enemies which are shocked",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 1983,
    });
    v.push(PatternEntry {
        pat: "against frozen enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })),
            ..Default::default()
        }),
        lua_line: 1984,
    });
    v.push(PatternEntry {
        pat: "to frozen enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })),
            ..Default::default()
        }),
        lua_line: 1985,
    });
    v.push(PatternEntry {
        pat: "against chilled enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1986,
    });
    v.push(PatternEntry {
        pat: "you inflict on chilled enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1987,
    });
    v.push(PatternEntry {
        pat: "to chilled enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1988,
    });
    v.push(PatternEntry {
        pat: "inflicted on chilled enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1989,
    });
    v.push(PatternEntry {
        pat: "enemies which are chilled",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Chilled" })),
            ..Default::default()
        }),
        lua_line: 1990,
    });
    v.push(PatternEntry { pat: "against chilled or frozen enemies", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Chilled"), n!("Frozen")][..] })), ..Default::default() }), lua_line: 1991 });
}

/// @pob Modules/ModParser.lua:1992-2009 h=7a27a9f8
fn modTagList_1992(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry { pat: "against frozen, shocked or ignited enemies", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Frozen"), n!("Shocked"), n!("Ignited")][..] })), ..Default::default() }), lua_line: 1992 });
    v.push(PatternEntry { pat: "against enemies affected by elemental ailments", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Frozen"), n!("Chilled"), n!("Shocked"), n!("Ignited"), n!("Scorched"), n!("Brittle"), n!("Sapped")][..] })), ..Default::default() }), lua_line: 1993 });
    v.push(PatternEntry { pat: "against enemies affected by ailments", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Frozen"), n!("Chilled"), n!("Shocked"), n!("Ignited"), n!("Scorched"), n!("Brittle"), n!("Sapped"), n!("Poisoned"), n!("Bleeding")][..] })), ..Default::default() }), lua_line: 1994 });
    v.push(PatternEntry { pat: "against enemies that are affected by elemental ailments", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Frozen"), n!("Chilled"), n!("Shocked"), n!("Ignited"), n!("Scorched"), n!("Brittle"), n!("Sapped")][..] })), ..Default::default() }), lua_line: 1995 });
    v.push(PatternEntry { pat: "against enemies that are affected by no elemental ailments", val: TagVal::Spec(ModSpec { tagList: vec![tag!({ type = ActorCondition, actor = "enemy", varList = &[n!("Frozen"), n!("Chilled"), n!("Shocked"), n!("Ignited"), n!("Scorched"), n!("Brittle"), n!("Sapped")][..], neg = true }), tag!({ type = Condition, var = "Effective" })], ..Default::default() }), lua_line: 1996 });
    v.push(PatternEntry { pat: "against enemies affected by (%d+) spider's webs", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, actor = "enemy", var = "Spider's WebStack", threshold = num })), ..Default::default() }
    }), lua_line: 1997 });
    v.push(PatternEntry {
        pat: "against enemies on consecrated ground",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = ActorCondition, actor = "enemy", var = "OnConsecratedGround" }),
            ),
            ..Default::default()
        }),
        lua_line: 1998,
    });
    v.push(PatternEntry { pat: "against enemies with a higher percentage of their life remaining than you", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "HigherLifePercentThanPlayer" })), ..Default::default() }), lua_line: 1999 });
    v.push(PatternEntry { pat: "if (%d+)%% of curse duration expired", val: TagVal::Fn(|caps: &Caps| {
        let num = caps.num;
        ModSpec { tag: Some(tag!({ type = MultiplierThreshold, actor = "enemy", var = "CurseExpired", threshold = num })), ..Default::default() }
    }), lua_line: 2000 });
    // hand-ported (lua2rs: modtag-param1-num-or-string); a capture with a digit reaches
    // firstToUpper as a number or nil, which `str:gsub` cannot index (Common.lua:14)
    v.push(PatternEntry { pat: "against enemies with (%w+) exposure", val: TagVal::Fn(|caps: &Caps| {
        let element = arg1_str(caps, "str");
        ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = cat!("Has", cat!(firstToUpper(element), "Exposure")) })), ..Default::default() }
    }), lua_line: 2001 });
    v.push(PatternEntry {
        pat: "by s?l?a?i?n? ?frozen enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Frozen" })),
            ..Default::default()
        }),
        lua_line: 2002,
    });
    v.push(PatternEntry {
        pat: "by s?l?a?i?n? ?shocked enemies",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })),
            ..Default::default()
        }),
        lua_line: 2003,
    });
    // Enemy multipliers
    v.push(PatternEntry {
        pat: "per freeze, shock [ao][nr]d? ignite on enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "FreezeShockIgniteOnEnemy" })),
            ..Default::default()
        }),
        lua_line: 2005,
    });
    v.push(PatternEntry {
        pat: "per poison affecting enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, actor = "enemy", var = "PoisonStack" })),
            ..Default::default()
        }),
        lua_line: 2006,
    });
    // hand-ported (lua2rs: modtag-param1-num-or-string): `num` is `tonumber` of the capture when
    // it holds a digit (nil for e.g. "1.2.3"), else the capture string itself
    v.push(PatternEntry { pat: "per poison affecting enemy, up to %+([%d%.]+)%%", val: TagVal::Fn(|caps: &Caps| {
        let mut t = tag!({ type = Multiplier, actor = "enemy", var = "PoisonStack", limitTotal = true });
        match arg1(caps) {
            Arg1::Num(num) => t.limit = num,
            Arg1::Str(s) => {
                // LUA-QUIRK: `limit` holds a non-numeric string (only "." runs can match): kept
                // as the raw Lua value in `extra`, the Tag convention for string-valued number keys
                let mut ex = ValueTable::new();
                ex.fields.push((n!("limit"), Value::from(s)));
                t.extra = Some(Box::new(ex));
            }
        }
        ModSpec { tag: Some(t), ..Default::default() }
    }), lua_line: 2007 });
    v.push(PatternEntry {
        pat: "for each spider's web on the enemy",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, actor = "enemy", var = "Spider's WebStack" })),
            ..Default::default()
        }),
        lua_line: 2008,
    });
}

/// Built once in source order (calc doc §6).
pub static modTagList: LazyLock<PatternList<TagVal>> = LazyLock::new(|| {
    let mut v: Vec<PatternEntry<TagVal>> = Vec::with_capacity(665);
    modTagList_1321(&mut v);
    modTagList_1372(&mut v);
    modTagList_1422(&mut v);
    modTagList_1473(&mut v);
    modTagList_1523(&mut v);
    modTagList_1574(&mut v);
    modTagList_1630(&mut v);
    modTagList_1689(&mut v);
    modTagList_1739(&mut v);
    modTagList_1789(&mut v);
    modTagList_1839(&mut v);
    modTagList_1889(&mut v);
    modTagList_1942(&mut v);
    modTagList_1992(&mut v);
    PatternList(v)
});
