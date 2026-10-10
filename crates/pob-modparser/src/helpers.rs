//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 1-71 and 2011-2129: module header, `getSimpleConv`, `conquerorList` and the mod
//! constructor helpers (`flag`, `dealNoNonDamageType`, `grantedExtraSkill`, `triggerExtraSkill`,
//! `extraSupport`, `explodeFunc`, `dmgTypes`). `firstToUpper` (13-15) is `pob_lua::string::firstToUpper`.
//!
//! Owner: MP0. The generated pattern lists (MP1-MP6, lua2rs output) call these with the
//! signatures of calc doc §6 / CCR-0006 §6: argument conversions such as `tonumber(level)` are
//! done at the call site.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use pob_gamedata::{data, JewelFuncSpec, Mod, ModType, Name, Tag, Value, ValueTable};
use pob_lua::n;
use pob_lua::pattern::{gsub, gsub_fn};
use pob_lua::string::{firstToUpper, lower, upper};
use std::sync::{Arc, LazyLock};

// @pob Modules/ModParser.lua:6-12 h=6fa8e6d6
// (module-level statements: pairs,ipairs,t_insert,band,bor,bnot,m_huge)
// local pairs = pairs / ipairs / t_insert = table.insert / band, bor, bnot = bit.* /
// m_huge = math.huge: the Rust code uses iterators, `Vec::push`, `pob_lua::bit` and `f64::INFINITY`.

/// A Lua table constructor `{ k1 = v1, k2 = v2, … }`: a `nil` value leaves the key absent.
/// Rust-only plumbing (the fields keep Lua source order; Lua string keys have no order).
pub fn vtab(fields: Vec<(Name, Value)>) -> Value {
    let mut t = ValueTable::new();
    for (k, v) in fields {
        if !v.is_nil() {
            t.fields.push((k, v));
        }
    }
    Value::Table(Arc::new(t))
}

/// `{ mod = m }` (Rust-only plumbing for the many `{ mod = mod(…) }` payloads).
pub fn mod_payload(m: Mod) -> Value {
    vtab(vec![(n!("mod"), Value::Mod(Arc::new(m)))])
}

// Radius jewels that modify other nodes
/// @pob Modules/ModParser.lua:18-39 h=503b072a
/// Returns the jewel func `function(node, out, data)` as data (MP7 runs it): `key` is
/// `"$getSimpleConv"` and `args` are the upvalues in parameter order
/// `[srcList (array table), dst, type, remove, factor, srcType]` (`nil` for an absent
/// `factor`/`srcType`).
// srcType for conversions where src and dst types do not match, e.g. increased life on base resistances
pub fn getSimpleConv(
    srcList: &[&str],
    dst: &str,
    r#type: ModType,
    remove: bool,
    factor: Option<f64>,
    srcType: Option<ModType>,
) -> JewelFuncSpec {
    // L19: return function(node, out, data) ... end — the body (19-38) runs in
    // pob_calc::modules::jewel_funcs (MP7), keyed by "$getSimpleConv".
    let mut args: smallvec::SmallVec<[Value; 4]> = smallvec::SmallVec::new();
    args.push(Value::Table(Arc::new(ValueTable {
        array: srcList.iter().map(|s| Value::from(*s)).collect(),
        fields: Vec::new(),
    })));
    args.push(Value::from(dst));
    args.push(Value::from(r#type.as_str()));
    args.push(Value::Bool(remove));
    args.push(Value::from(factor));
    args.push(srcType.map_or(Value::Nil, |t| Value::from(t.as_str())));
    JewelFuncSpec {
        key: n!("$getSimpleConv"),
        kind: pob_gamedata::JewelFuncKind::Other,
        args,
        r#type: None,
        radius: None,
    }
}

/// @pob Modules/ModParser.lua:41-70 h=e4b94ab4
/// `conquerorList` (ModParser.lua 41-70): conqueror name → `{ id = …, type = … }`, in source order.
pub static conquerorList: LazyLock<Vec<(Name, Value)>> = LazyLock::new(|| {
    fn e(name: &str, id: Value, ty: &str) -> (Name, Value) {
        (
            Name::from(name),
            vtab(vec![(n!("id"), id), (n!("type"), Value::from(ty))]),
        )
    }
    let num = |x: f64| Value::Num(x);
    let s = |x: &str| Value::from(x);
    vec![
        e("xibaqua", num(1.0), "vaal"),
        e("zerphi", num(2.0), "vaal"),
        e("doryani", num(3.0), "vaal"),
        e("ahuana", s("2_v2"), "vaal"),
        e("deshret", num(1.0), "maraketh"),
        e("asenath", num(2.0), "maraketh"),
        e("nasima", num(3.0), "maraketh"),
        e("balbala", s("1_v2"), "maraketh"),
        e("cadiro", num(1.0), "eternal"),
        e("victario", num(2.0), "eternal"),
        e("chitus", num(3.0), "eternal"),
        e("caspiro", s("3_v2"), "eternal"),
        e("kaom", num(1.0), "karui"),
        e("rakiata", num(2.0), "karui"),
        e("kiloava", num(3.0), "karui"),
        e("akoya", s("3_v2"), "karui"),
        e("venarius", num(1.0), "templar"),
        e("dominus", num(2.0), "templar"),
        e("avarius", num(3.0), "templar"),
        e("maxarius", s("1_v2"), "templar"),
        e("vorana", num(1.0), "kalguur"),
        e("uhtred", num(2.0), "kalguur"),
        e("medved", num(3.0), "kalguur"),
        e("tecrod", num(1.0), "abyss_murderous"),
        e("ulaman", num(1.0), "abyss_searching"),
        e("kurgal", num(1.0), "abyss_hypnotic"),
        e("amanamu", num(1.0), "abyss_ghastly"),
        e("zorath", num(1.0), "abyss_special"),
    ]
});

/// @pob Modules/ModParser.lua:2011-2011 h=505e172d
/// `local mod = modLib.createMod` (CCR-0006 §6: generated lists call it with createMod's
/// argument dispatch already resolved).
pub use pob_gamedata::mod_tools::createMod as r#mod;

/// @pob Modules/ModParser.lua:2012-2014 h=880a0d5d
/// `flag(name, ...)` = `mod(name, "FLAG", true, ...)`.
pub fn flag(name: Name, source: Option<&str>, flags: u32, keywordFlags: u32, tags: &[Tag]) -> Mod {
    // L2013
    r#mod(name, ModType::FLAG, true, source, flags, keywordFlags, tags)
}

// @pob Modules/ModParser.lua:2015-2015 h=fae2d0b6
// (module-level statements: damageTypeList)
const damageTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];

// Makes the "deal no" modifiers for every damage type except the one being kept.
/// @pob Modules/ModParser.lua:2018-2031 h=3de6b0b1
pub fn dealNoNonDamageType(dmgType: &str, forMinion: bool) -> Option<Vec<Mod>> {
    // L2019
    let dmgType = firstToUpper(dmgType);
    // L2020: isValueInArray (Common.lua:569-575)
    if !damageTypeList.iter().any(|v| *v == dmgType) {
        // L2021
        return None;
    }
    // L2023
    let mut mods: Vec<Mod> = Vec::new();
    // L2024
    for damageType in damageTypeList {
        // L2025
        if damageType != dmgType {
            // L2026
            let dealNo = flag(pob_lua::cat!("DealNo", damageType), None, 0, 0, &[]);
            // L2027: forMinion and mod(…) or dealNo (the mod is a table, never falsy)
            mods.push(if forMinion {
                r#mod(
                    n!("MinionModifier"),
                    ModType::LIST,
                    mod_payload(dealNo),
                    None,
                    0,
                    0,
                    &[],
                )
            } else {
                dealNo
            });
        }
    }
    // L2030
    Some(mods)
}

/// @pob Modules/ModParser.lua:2033-2046 h=8e717654
/// The `gemIdLookup` table (built from `data.skills` at load): lowercased skill name → skill id.
pub fn gemIdLookup(name: &str) -> Option<Name> {
    GEM_ID_LOOKUP.get(name).copied()
}

/// The Lua local `gemIdLookup` (ModParser.lua:2033-2046), built once.
static GEM_ID_LOOKUP: LazyLock<ahash::AHashMap<String, Name>> = LazyLock::new(|| {
    // L2033
    let mut gemIdLookup: ahash::AHashMap<String, Name> = ahash::AHashMap::new();
    gemIdLookup.insert(
        "power charge on critical strike".to_owned(),
        Name::from("SupportPowerChargeOnCritical"),
    );
    // L2036
    let mut skillIds: Vec<Name> = Vec::new();
    // L2037: for id in pairs(data.skills) (the order is sorted next)
    for id in data().skills.keys() {
        // L2038
        skillIds.push(*id);
    }
    // L2040: table.sort(skillIds) — ids are unique, LuaJIT compares strings bytewise
    skillIds.sort_by(|a, b| a.as_str().as_bytes().cmp(b.as_str().as_bytes()));
    // L2041
    for id in &skillIds {
        // L2042
        let grantedEffect = &data().skills[id];
        // L2043
        if !grantedEffect.hidden || grantedEffect.fromItem || grantedEffect.fromTree {
            // L2044
            gemIdLookup.insert(lower(grantedEffect.name.as_str()), grantedEffect.id);
        }
    }
    gemIdLookup
});

/// @pob Modules/ModParser.lua:2047-2054 h=3a2daa78
/// `level` is already `tonumber(level)` (CCR-0006 §6).
pub fn grantedExtraSkill(
    name: &str,
    level: Option<f64>,
    noSupports: Option<bool>,
) -> Option<Vec<Mod>> {
    // L2048
    let name = gsub(name, " skill", "", None).0;
    // L2049
    if let Some(skillId) = gemIdLookup(&name) {
        // L2050
        return Some(vec![r#mod(
            n!("ExtraSkill"),
            ModType::LIST,
            vtab(vec![
                (n!("skillId"), Value::from(skillId)),
                (n!("level"), Value::from(level)),
                (n!("noSupports"), noSupports.map_or(Value::Nil, Value::Bool)),
            ]),
            None,
            0,
            0,
            &[],
        )]);
    }
    None
}

/// `triggerExtraSkill`'s `options` table (ModParser.lua:2055-2072; CCR-0006 §6).
#[derive(Default, Clone, Debug)]
pub struct TriggerExtraSkillOptions {
    pub noSupports: Option<bool>,
    pub sourceSkill: Option<String>,
    pub triggerChance: Option<f64>,
    pub ignoreHexproof: bool,
    pub onCrit: bool,
}

/// @pob Modules/ModParser.lua:2055-2072 h=9f9268e5
/// `options.triggerChance` is already `tonumber(options.triggerChance)` (CCR-0006 §6).
pub fn triggerExtraSkill(
    name: &str,
    level: Option<f64>,
    options: Option<TriggerExtraSkillOptions>,
) -> Vec<Mod> {
    // L2056
    let mut options = options.unwrap_or_default();
    // L2057
    let name = gsub(name, " skill", "", None).0;
    // L2058
    if let Some(sourceSkill) = &options.sourceSkill {
        // L2059
        options.sourceSkill = Some(gsub(sourceSkill, " skill", "", None).0);
    }
    // L2061
    let mut mods: Vec<Mod> = Vec::new();
    // L2062
    if let Some(skillId) = gemIdLookup(&name) {
        // L2063
        mods.push(r#mod(
            n!("ExtraSkill"),
            ModType::LIST,
            vtab(vec![
                (n!("skillId"), Value::from(skillId)),
                (n!("level"), Value::from(level)),
                (
                    n!("noSupports"),
                    options.noSupports.map_or(Value::Nil, Value::Bool),
                ),
                (n!("triggered"), Value::Bool(true)),
                (
                    n!("source"),
                    options
                        .sourceSkill
                        .as_deref()
                        .map_or(Value::Nil, Value::from),
                ),
                (n!("triggerChance"), Value::from(options.triggerChance)),
            ]),
            None,
            0,
            0,
            &[],
        ));
    }
    // L2065
    if options.ignoreHexproof {
        // L2066
        mods.push(r#mod(
            n!("SkillData"),
            ModType::LIST,
            vtab(vec![
                (n!("key"), Value::from("ignoreHexproof")),
                (n!("value"), Value::Bool(true)),
            ]),
            None,
            0,
            0,
            &[pob_gamedata::tag!({ type = SkillId, skillId = gemIdLookup(&name) })],
        ));
    }
    // L2068
    if options.onCrit {
        // L2069
        mods.push(r#mod(
            n!("ExtraSkillMod"),
            ModType::LIST,
            mod_payload(r#mod(
                n!("SkillData"),
                ModType::LIST,
                vtab(vec![
                    (n!("key"), Value::from("triggerOnCrit")),
                    (n!("value"), Value::Bool(true)),
                ]),
                None,
                0,
                0,
                &[],
            )),
            None,
            0,
            0,
            &[pob_gamedata::tag!({ type = SkillId, skillId = gemIdLookup(&name) })],
        ));
    }
    // L2071
    mods
}

/// @pob Modules/ModParser.lua:2073-2106 h=b72fdb0b
/// `level` is already `tonumber(level)` (CCR-0006 §6; Lua's `level = tonumber(level)` at 2087).
pub fn extraSupport(
    name: &str,
    level: Option<f64>,
    slot: Option<&str>,
    appliesToGrantedSkills: Option<bool>,
) -> Option<Vec<Mod>> {
    // L2074: gemIdLookup[name] or gemIdLookup[name:gsub("^increased ","")] or gemIdLookup[name:gsub(" support$","")]
    // (an index expression keeps only gsub's first result)
    let skillId = gemIdLookup(name)
        .or_else(|| gemIdLookup(&gsub(name, "^increased ", "", None).0))
        .or_else(|| gemIdLookup(&gsub(name, " support$", "", None).0));

    // L2076
    let slot: String = if slot == Some("main hand") || slot == Some("main hand weapon") {
        // L2077
        "Weapon 1".to_owned()
    } else if slot == Some("off hand") || slot == Some("off hand weapon") {
        // L2079
        "Weapon 2".to_owned()
    } else if let Some(slot) = slot {
        // L2081: string.gsub(" "..slot, "%W%l", string.upper):sub(2)
        let s = gsub_fn(
            &format!(" {slot}"),
            "%W%l",
            |c| Some(upper(&c[0].to_str())),
            None,
        )
        .0;
        pob_lua::string::sub(&s, 2, None).into_owned()
    } else {
        // L2083
        "{SlotName}".to_owned()
    };

    // L2086
    let slotTag = pob_gamedata::tag!({ type = SocketedIn, slotName = slot.as_str() });
    // L2087: level = tonumber(level) — done by the caller
    // L2088
    if let Some(skillId) = skillId {
        // L2089
        let Some(skill) = data().skills.get(&skillId) else {
            pob_lua::lua_error!("attempt to index a nil value (field '?')");
        };
        let gemId = data()
            .gemForBaseName
            .get(lower(&format!("{} Support", skill.name)).as_str())
            .copied();
        // L2090
        if let Some(gemId) = gemId {
            let Some(gem) = data().gems.get(&gemId) else {
                pob_lua::lua_error!("attempt to index a nil value (field '?')");
            };
            // L2091
            let mut mods = vec![r#mod(
                n!("ExtraSupport"),
                ModType::LIST,
                vtab(vec![
                    (n!("skillId"), Value::from(gem.grantedEffectId)),
                    (n!("level"), Value::from(level)),
                    (
                        n!("appliesToGrantedSkills"),
                        appliesToGrantedSkills.map_or(Value::Nil, Value::Bool),
                    ),
                ]),
                None,
                0,
                0,
                std::slice::from_ref(&slotTag),
            )];
            // L2092
            if let Some(secondary) = &gem.secondaryGrantedEffect {
                // L2093
                if secondary.support {
                    // L2094
                    mods.push(r#mod(
                        n!("ExtraSupport"),
                        ModType::LIST,
                        vtab(vec![
                            (n!("skillId"), Value::from(gem.secondaryGrantedEffectId)),
                            (n!("level"), Value::from(level)),
                            (
                                n!("appliesToGrantedSkills"),
                                appliesToGrantedSkills.map_or(Value::Nil, Value::Bool),
                            ),
                        ]),
                        None,
                        0,
                        0,
                        std::slice::from_ref(&slotTag),
                    ));
                } else {
                    // L2096
                    mods.push(r#mod(
                        n!("ExtraSkill"),
                        ModType::LIST,
                        vtab(vec![
                            (n!("skillId"), Value::from(gem.secondaryGrantedEffectId)),
                            (n!("level"), Value::from(level)),
                        ]),
                        None,
                        0,
                        0,
                        &[],
                    ));
                }
            }
            // L2099
            return Some(mods);
        } else {
            // L2101
            return Some(vec![r#mod(
                n!("ExtraSupport"),
                ModType::LIST,
                vtab(vec![
                    (n!("skillId"), Value::from(skillId)),
                    (n!("level"), Value::from(level)),
                    (
                        n!("appliesToGrantedSkills"),
                        appliesToGrantedSkills.map_or(Value::Nil, Value::Bool),
                    ),
                ]),
                None,
                0,
                0,
                std::slice::from_ref(&slotTag),
            )]);
        }
    }
    None
}

/// A helper argument the Lua body uses both as a number and as a string (`explodeFunc`'s
/// `amount`; CCR-0006 §6).
#[derive(Clone, Copy, Debug)]
pub enum LuaArg<'a> {
    Nil,
    Num(f64),
    Str(&'a str),
}

/// @pob Modules/ModParser.lua:2108-2119 h=095f070d
/// `explodeFunc(chance, amount, type, ...)` (extra tags as varargs).
pub fn explodeFunc(
    chance: Option<f64>,
    amount: LuaArg,
    r#type: &str,
    tags: &[Tag],
) -> Option<Vec<Mod>> {
    // L2109: tonumber(amount) or (amount == "tenth" and 10) or (amount == "quarter" and 25)
    let amountNumber = match amount {
        // tonumber(nil) raises "bad argument #1 to 'tonumber' (value expected)"
        LuaArg::Nil => pob_lua::lua_error!("bad argument #1 to 'tonumber' (value expected)"),
        LuaArg::Num(x) => Some(x),
        LuaArg::Str(s) => pob_lua::tonumber(s).or(match s {
            "tenth" => Some(10.0),
            "quarter" => Some(25.0),
            _ => None,
        }),
    };
    // L2110
    let Some(amountNumber) = amountNumber else {
        // L2111
        return None;
    };
    // L2113-2114: local amounts = {}; amounts[type] = amountNumber (never read)
    // L2115: chance / 100 (nil chance raises)
    let Some(chance) = chance else {
        pob_lua::lua_error!("attempt to perform arithmetic on local 'chance' (a nil value)");
    };
    Some(vec![
        // L2116
        r#mod(
            n!("ExplodeMod"),
            ModType::LIST,
            vtab(vec![
                (n!("type"), Value::from(firstToUpper(r#type))),
                (n!("chance"), Value::Num(chance / 100.0)),
                (n!("amount"), Value::Num(amountNumber)),
                (n!("keyOfScaledMod"), Value::from("chance")),
            ]),
            None,
            0,
            0,
            tags,
        ),
        // L2117
        flag(n!("CanExplode"), None, 0, 0, &[]),
    ])
}

/// @pob Modules/ModParser.lua:2121-2127 h=53fc767e
/// The `dmgTypes` table: damage type word → mod name stem.
pub fn dmgTypes(name: &str) -> Option<Name> {
    match name {
        "physical" => Some(n!("Physical")),
        "lightning" => Some(n!("Lightning")),
        "cold" => Some(n!("Cold")),
        "fire" => Some(n!("Fire")),
        "chaos" => Some(n!("Chaos")),
        _ => None,
    }
}
