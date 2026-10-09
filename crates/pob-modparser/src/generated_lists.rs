//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 5943-6150: the generated specialModList entries (keystones, anchoring), the suffix,
//! penetration, resource, regen/degen/cost and flag type tables, and the skill name lists.
//!
//! Owner: MP0. Each Lua table becomes a [`PatternList`] built once (`LazyLock`) with Lua's
//! assignment semantics: writing an existing key replaces its value and records the writing
//! line, which is the `lua_line` [`crate::scan`] uses for full ties and the line the parse
//! oracle reports (`data/dump/parse_oracle.jsonl`).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::helpers::{flag, mod_payload, r#mod};
use crate::types::*;
use pob_gamedata::{data, tag, KeywordFlag, ModType, Name, SkillType, Value, ValueTable};
use pob_lua::pattern::{gsub, gsub_fn};
use pob_lua::string::{firstToUpper, lower, upper};
use pob_lua::{cat, n};
use std::sync::{Arc, LazyLock};

/// A Lua table under construction (Rust-only plumbing): `t[k] = v` replaces the value of an
/// existing key in place (keeping its slot) and records the writing line; a new key is
/// appended. Key order carries no meaning (Lua's `pairs` order is random for string keys).
pub struct TableBuilder<T> {
    entries: Vec<PatternEntry<T>>,
    index: ahash::AHashMap<&'static str, usize>,
}

impl<T> Default for TableBuilder<T> {
    fn default() -> Self {
        Self {
            entries: Vec::new(),
            index: ahash::AHashMap::new(),
        }
    }
}

impl<T> TableBuilder<T> {
    pub fn new() -> Self {
        Self::default()
    }
    /// `t[pat] = val` at ModParser.lua line `lua_line`.
    pub fn set(&mut self, pat: &'static str, val: T, lua_line: u32) {
        if let Some(&i) = self.index.get(pat) {
            self.entries[i].val = val;
            self.entries[i].lua_line = lua_line;
        } else {
            self.index.insert(pat, self.entries.len());
            self.entries.push(PatternEntry { pat, val, lua_line });
        }
    }
    /// `t[pat] = val` for a computed key (interned, so it lives for the process).
    pub fn set_computed(&mut self, pat: &str, val: T, lua_line: u32) {
        self.set(Name::from(pat).as_str(), val, lua_line);
    }
    pub fn finish(self) -> PatternList<T> {
        PatternList(self.entries)
    }
}

/// The keystone and generated entries need the interned key text as `&'static str`.
fn key(s: String) -> &'static str {
    Name::from(s.as_str()).as_str()
}

/// The source-order concatenation of `special_a`..`special_d` (ModParser.lua 2130-5942, the
/// literal `specialModList` constructor; MP3-MP6 drop overwritten duplicate keys), completed
/// by [`build_special_mod_list`] and the generated skill entries of 6124-6147.
pub static specialModList: LazyLock<PatternList<SpecialVal>> = LazyLock::new(|| {
    let mut literal = crate::special_a::entries();
    literal.extend(crate::special_b::entries());
    literal.extend(crate::special_c::entries());
    literal.extend(crate::special_d::entries());
    complete_special_mod_list(literal)
});

/// The whole `specialModList` from its literal entries: [`build_special_mod_list`] (keystones,
/// anchoring) then the gem loop's writes (6124-6147).
pub fn complete_special_mod_list(
    literal: Vec<PatternEntry<SpecialVal>>,
) -> PatternList<SpecialVal> {
    let mut t = build_special_mod_list(literal);
    for (pat, val, line) in SKILL_LISTS.special.iter() {
        t.set(pat, val.clone_val(), *line);
    }
    t.finish()
}

impl SpecialVal {
    /// The same Lua value stored under a second key (Rust-only plumbing).
    pub fn clone_val(&self) -> SpecialVal {
        match self {
            SpecialVal::Mods(m) => SpecialVal::Mods(m.clone()),
            SpecialVal::Fn(f) => SpecialVal::Fn(*f),
            SpecialVal::Dyn(f) => SpecialVal::Dyn(f.clone()),
        }
    }
}

/// @pob Modules/ModParser.lua:5943-5953 h=9ee4a21b
/// The keystone entries (5943-5948) and the `"^"..k.."$"` anchoring of every key so far
/// (5949-5953), applied to the literal `specialModList` entries. Wrapped keys keep the line of
/// the entry they wrap. The generated skill entries (6124-6147) are added after this by
/// [`specialModList`] (their keys are not anchored).
pub fn build_special_mod_list(literal: Vec<PatternEntry<SpecialVal>>) -> TableBuilder<SpecialVal> {
    // the Lua local `specialModList` (a static of that name exists, so `t` here)
    let mut t: TableBuilder<SpecialVal> = TableBuilder::new();
    for e in literal {
        t.set(e.pat, e.val, e.lua_line);
    }
    // L5943
    for name in &data().keystones {
        // L5944
        let have = gsub(
            &gsub_fn(
                &firstToUpper(name.as_str()),
                " %l",
                |c| Some(upper(&c[0].to_str())),
                None,
            )
            .0,
            " ",
            "",
            None,
        )
        .0;
        t.set(
            key(lower(name.as_str())),
            SpecialVal::Mods(vec![
                r#mod(n!("Keystone"), ModType::LIST, *name, None, 0, 0, &[]),
                flag(cat!("Condition:Have", have.as_str()), None, 0, 0, &[]),
            ]),
            5944,
        );
    }
    // L5946
    for name in &data().clusterJewels.keystones {
        // L5947
        t.set(
            key(lower(name.as_str())),
            SpecialVal::Mods(vec![r#mod(
                n!("Keystone"),
                ModType::LIST,
                *name,
                None,
                0,
                0,
                &[],
            )]),
            5947,
        );
    }
    // L5949
    let oldList = t;
    // L5950
    let mut t: TableBuilder<SpecialVal> = TableBuilder::new();
    // L5951: for k, v in pairs(oldList) — every key is new, so the order is irrelevant
    for e in oldList.entries {
        // L5952
        t.set(key(format!("^{}$", e.pat)), e.val, e.lua_line);
    }
    t
}

// Modifiers that are recognised but unsupported
/// @pob Modules/ModParser.lua:5955-5960 h=32c46b18
/// `unsupportedModList[lineLower]` (a set).
pub fn unsupportedModList(lineLower: &str) -> bool {
    matches!(
        lineLower,
        "properties are doubled while in a breach" | "mirrored" | "split"
    )
}

/// `pairs(unsupportedModList)` size (the parse oracle's `listSizes`).
pub const unsupportedModListLen: usize = 3;

fn names_list(entries: &[(&'static str, &str, u32)]) -> PatternList<Name> {
    let mut t = TableBuilder::new();
    for (pat, v, line) in entries {
        t.set(pat, Name::from(*v), *line);
    }
    t.finish()
}

// Special lookups used for various modifier forms
/// @pob Modules/ModParser.lua:5962-5999 h=8a236116
pub static suffixTypes: LazyLock<PatternList<Name>> = LazyLock::new(|| {
    names_list(&[
        ("as extra lightning damage", "GainAsLightning", 5964),
        ("added as lightning damage", "GainAsLightning", 5965),
        ("gained as extra lightning damage", "GainAsLightning", 5966),
        ("as extra cold damage", "GainAsCold", 5967),
        ("added as cold damage", "GainAsCold", 5968),
        ("gained as extra cold damage", "GainAsCold", 5969),
        ("as extra fire damage", "GainAsFire", 5970),
        ("added as fire damage", "GainAsFire", 5971),
        ("gained as extra fire damage", "GainAsFire", 5972),
        ("as extra chaos damage", "GainAsChaos", 5973),
        ("added as chaos damage", "GainAsChaos", 5974),
        ("gained as extra chaos damage", "GainAsChaos", 5975),
        ("converted to lightning", "ConvertToLightning", 5976),
        ("converted to lightning damage", "ConvertToLightning", 5977),
        ("converted to cold damage", "ConvertToCold", 5978),
        ("converted to fire damage", "ConvertToFire", 5979),
        ("converted to fire", "ConvertToFire", 5980),
        ("converted to chaos damage", "ConvertToChaos", 5981),
        ("added as energy shield", "GainAsEnergyShield", 5982),
        ("as extra maximum energy shield", "GainAsEnergyShield", 5983),
        ("converted to energy shield", "ConvertToEnergyShield", 5984),
        ("as extra armour", "GainAsArmour", 5985),
        ("as physical damage", "AsPhysical", 5986),
        ("as lightning damage", "AsLightning", 5987),
        ("as cold damage", "AsCold", 5988),
        ("as fire damage", "AsFire", 5989),
        ("as fire", "AsFire", 5990),
        ("as chaos damage", "AsChaos", 5991),
        ("leeched as life and mana", "Leech", 5992),
        ("leeched as life", "LifeLeech", 5993),
        ("is leeched as life", "LifeLeech", 5994),
        ("leeched as mana", "ManaLeech", 5995),
        ("is leeched as mana", "ManaLeech", 5996),
        ("leeched as energy shield", "EnergyShieldLeech", 5997),
        ("is leeched as energy shield", "EnergyShieldLeech", 5998),
    ])
});

/// @pob Modules/ModParser.lua:6000-6007 h=c93b560a
pub static penTypes: LazyLock<PatternList<Name>> = LazyLock::new(|| {
    names_list(&[
        ("lightning resistance", "LightningPenetration", 6001),
        ("cold resistance", "ColdPenetration", 6002),
        ("fire resistance", "FirePenetration", 6003),
        ("elemental resistance", "ElementalPenetration", 6004),
        ("elemental resistances", "ElementalPenetration", 6005),
        ("chaos resistance", "ChaosPenetration", 6006),
    ])
});

/// A Lua array of strings `{ "Life", "Mana" }` as a [`Value`] (Rust-only plumbing).
fn str_array(xs: &[&str]) -> Value {
    Value::Table(Arc::new(ValueTable {
        array: xs.iter().map(|s| Value::from(*s)).collect(),
        fields: Vec::new(),
    }))
}

/// @pob Modules/ModParser.lua:6008-6034 h=082c2371
/// `resourceTypes` after the `do … end` block that adds a `"maximum "..k` key for every key
/// (6026-6034; those entries record the writing line 6032). A value is a string or an array of
/// strings; the `maximum` keys share the value of the key they copy.
pub static resourceTypes: LazyLock<PatternList<Value>> = LazyLock::new(|| {
    // L6008
    // the Lua local `resourceTypes`
    let mut rt: TableBuilder<Value> = TableBuilder::new();
    rt.set("life", Value::from("Life"), 6009);
    rt.set("mana", Value::from("Mana"), 6010);
    rt.set("energy shield", Value::from("EnergyShield"), 6011);
    rt.set("life and mana", str_array(&["Life", "Mana"]), 6012);
    rt.set(
        "life and energy shield",
        str_array(&["Life", "EnergyShield"]),
        6013,
    );
    rt.set(
        "life, mana and energy shield",
        str_array(&["Life", "Mana", "EnergyShield"]),
        6014,
    );
    rt.set(
        "life, energy shield and mana",
        str_array(&["Life", "Mana", "EnergyShield"]),
        6015,
    );
    rt.set("mana and life", str_array(&["Life", "Mana"]), 6016);
    rt.set(
        "mana and energy shield",
        str_array(&["Mana", "EnergyShield"]),
        6017,
    );
    rt.set(
        "mana, life and energy shield",
        str_array(&["Life", "Mana", "EnergyShield"]),
        6018,
    );
    rt.set(
        "mana, energy shield and life",
        str_array(&["Life", "Mana", "EnergyShield"]),
        6019,
    );
    rt.set(
        "energy shield and life",
        str_array(&["Life", "EnergyShield"]),
        6020,
    );
    rt.set(
        "energy shield and mana",
        str_array(&["Mana", "EnergyShield"]),
        6021,
    );
    rt.set(
        "energy shield, life and mana",
        str_array(&["Life", "Mana", "EnergyShield"]),
        6022,
    );
    rt.set(
        "energy shield, mana and life",
        str_array(&["Life", "Mana", "EnergyShield"]),
        6023,
    );
    rt.set("rage", Value::from("Rage"), 6024);
    // L6026: do
    {
        // L6027
        let mut maximumResourceTypes: Vec<(String, Value)> = Vec::new();
        // L6028: for resource, values in pairs(resourceTypes)
        for e in &rt.entries {
            // L6029
            maximumResourceTypes.push((format!("maximum {}", e.pat), e.val.clone()));
        }
        // L6031
        for (resource, values) in maximumResourceTypes {
            // L6032
            rt.set(key(resource), values, 6032);
        }
    }
    rt.finish()
});

/// @pob Modules/ModParser.lua:6035-6048 h=37d44b85
pub fn appendMod(inputTable: &[(Name, Value)], string: &str) -> Vec<(Name, Value)> {
    // L6036
    let mut table: Vec<(Name, Value)> = Vec::new();
    // L6037: for subLine, mods in pairs(inputTable) (a new table per key; order irrelevant)
    for (subLine, mods) in inputTable {
        // L6038
        if let Value::Str(mods) = mods {
            // L6039
            table.push((*subLine, Value::from(format!("{mods}{string}"))));
        } else {
            // L6041
            let mut t = ValueTable::new();
            // L6042: for _, mod in ipairs(mods)
            if let Value::Table(mods) = mods {
                for m in &mods.array {
                    // L6043: mod..string (every element is a string)
                    let Value::Str(m) = m else {
                        pob_lua::lua_error!("attempt to concatenate local 'mod' (a table value)");
                    };
                    t.array.push(Value::from(format!("{m}{string}")));
                }
            } else {
                pob_lua::lua_error!("bad argument #1 to 'ipairs' (table expected)");
            }
            table.push((*subLine, Value::Table(Arc::new(t))));
        }
    }
    // L6047
    table
}

/// `appendMod(resourceTypes, string)` as a pattern table: each key keeps the line of the
/// `resourceTypes` entry it was copied from (Rust-only plumbing).
fn append_resource(string: &str) -> PatternList<Value> {
    let input: Vec<(Name, Value)> = resourceTypes
        .0
        .iter()
        .map(|e| (Name::from(e.pat), e.val.clone()))
        .collect();
    let out = appendMod(&input, string);
    let mut t = TableBuilder::new();
    for ((k, v), e) in out.into_iter().zip(resourceTypes.0.iter()) {
        t.set(k.as_str(), v, e.lua_line);
    }
    t.finish()
}

// @pob Modules/ModParser.lua:6049-6052 h=f3e6f59d
// (module-level statements: regenTypes, degenTypes, costTypes, baseCostTypes)
/// `regenTypes` (6049): looked up by key, `regenTypes[formCap[2]]`.
pub static regenTypes: LazyLock<PatternList<Value>> = LazyLock::new(|| append_resource("Regen"));
/// `degenTypes` (6050).
pub static degenTypes: LazyLock<PatternList<Value>> = LazyLock::new(|| append_resource("Degen"));
/// `costTypes` (6051): scanned (plain) by the TOTALCOST form.
pub static costTypes: LazyLock<PatternList<Value>> = LazyLock::new(|| append_resource("Cost"));
/// `baseCostTypes` (6052): scanned (plain) by the BASECOST form.
pub static baseCostTypes: LazyLock<PatternList<Value>> =
    LazyLock::new(|| append_resource("CostNoMult"));

/// `t[k]` on a key-value pattern table (Rust-only plumbing for `regenTypes[formCap[2]]`).
pub fn lookup<T>(t: &'static PatternList<T>, k: Option<&str>) -> Option<&'static T> {
    let k = k?;
    t.0.iter().find(|e| e.pat == k).map(|e| &e.val)
}

/// A `flagTypes` value: a mod name, or the table `{ name = …, value = …, type = … }` (6073).
#[derive(Clone, Debug, PartialEq)]
pub enum FlagTypeVal {
    Name(Name),
    Table {
        name: Name,
        value: f64,
        r#type: ModType,
    },
}

/// @pob Modules/ModParser.lua:6053-6093 h=f449d80b
/// The duplicate keys at 6078/6079 overwrite 6065/6066 (same values; the later line is kept).
pub static flagTypes: LazyLock<PatternList<FlagTypeVal>> = LazyLock::new(|| {
    let mut t: TableBuilder<FlagTypeVal> = TableBuilder::new();
    let mut s =
        |pat: &'static str, v: &str, line: u32| t.set(pat, FlagTypeVal::Name(Name::from(v)), line);
    s("phasing", "Condition:Phasing", 6054);
    s("onslaught", "Condition:Onslaught", 6055);
    s("rampage", "Condition:Rampage", 6056);
    s("soul eater", "Condition:CanHaveSoulEater", 6057);
    s("adrenaline", "Condition:Adrenaline", 6058);
    s("elusive", "Condition:CanBeElusive", 6059);
    s("arcane surge", "Condition:ArcaneSurge", 6060);
    s("fortify", "Condition:Fortified", 6061);
    s("fortified", "Condition:Fortified", 6062);
    s("unholy might", "Condition:UnholyMight", 6063);
    s("chaotic might", "Condition:ChaoticMight", 6064);
    s(
        "lesser brutal shrine buff",
        "Condition:LesserBrutalShrine",
        6065,
    );
    s(
        "lesser massive shrine buff",
        "Condition:LesserMassiveShrine",
        6066,
    );
    s("tailwind", "Condition:Tailwind", 6067);
    s("intimidated", "Condition:Intimidated", 6068);
    s("crushed", "Condition:Crushed", 6069);
    s("chilled", "Condition:Chilled", 6070);
    s("blinded", "Condition:Blinded", 6071);
    s("no life regeneration", "NoLifeRegen", 6072);
    drop(s);
    t.set(
        "hexproof",
        FlagTypeVal::Table {
            name: n!("CurseEffectOnSelf"),
            value: -100.0,
            r#type: ModType::MORE,
        },
        6073,
    );
    let mut s =
        |pat: &'static str, v: &str, line: u32| t.set(pat, FlagTypeVal::Name(Name::from(v)), line);
    s(
        "hindered,? with (%d+)%% reduced movement speed",
        "Condition:Hindered",
        6074,
    );
    s("unnerved", "Condition:Unnerved", 6075);
    s("malediction", "HasMalediction", 6076);
    s("debilitated", "Condition:Debilitated", 6077);
    // LUA-QUIRK: duplicate keys of 6065/6066; the table keeps these (equal) values
    s(
        "lesser brutal shrine buff",
        "Condition:LesserBrutalShrine",
        6078,
    );
    s(
        "lesser massive shrine buff",
        "Condition:LesserMassiveShrine",
        6079,
    );
    s(
        "acceleration shrine buff",
        "Condition:AccelerationShrine",
        6080,
    );
    s("brutal shrine buff", "Condition:BrutalShrine", 6081);
    s("diamond shrine buff", "Condition:DiamondShrine", 6082);
    s("echoing shrine buff", "Condition:EchoingShrine", 6083);
    s("gloom shrine buff", "Condition:GloomShrine", 6084);
    s(
        "greater freezing shrine buff",
        "Condition:GreaterFreezingShrine",
        6085,
    );
    s(
        "greater shocking shrine buff",
        "Condition:GreaterShockingShrine",
        6086,
    );
    s(
        "greater skeletal shrine buff",
        "Condition:GreaterSkeletalShrine",
        6087,
    );
    s(
        "impenetrable shrine buff",
        "Condition:ImpenetrableShrine",
        6088,
    );
    s("massive shrine buff", "Condition:MassiveShrine", 6089);
    s(
        "replenishing shrine buff",
        "Condition:ReplenishingShrine",
        6090,
    );
    s("resistance shrine buff", "Condition:ResistanceShrine", 6091);
    s("resonating shrine buff", "Condition:ResonatingShrine", 6092);
    drop(s);
    t.finish()
});

/// The three tables the gem loop (6100-6149) writes.
struct SkillLists {
    skillNameList: PatternList<ModSpec>,
    preSkillNameList: PatternList<ModSpec>,
    /// `specialModList` writes, in loop order: (key, value, line).
    special: Vec<(&'static str, SpecialVal, u32)>,
}

/// `{ type = "SkillName", skillName = skillName, includeTransfigured = true }`.
fn skillNameTag(skillName: Name) -> pob_gamedata::Tag {
    tag!({ type = SkillName, skillName = skillName, includeTransfigured = true })
}

fn tagSpec(t: pob_gamedata::Tag) -> ModSpec {
    ModSpec {
        tag: Some(t),
        ..Default::default()
    }
}

/// `{ addToSkill = <SkillName tag>, tag = { type = "GlobalEffect", effectType = "Buff" } }`.
fn buffSpec(skillName: Name) -> ModSpec {
    ModSpec {
        addToSkill: Some(skillNameTag(skillName)),
        tag: Some(tag!({ type = GlobalEffect, effectType = "Buff" })),
        ..Default::default()
    }
}

/// @pob Modules/ModParser.lua:6094-6149 h=7455cf15
/// Build active skill name lookup: `skillNameList`, `preSkillNameList` and the skill-specific
/// `specialModList` entries (closures capture `skillName`).
static SKILL_LISTS: LazyLock<SkillLists> = LazyLock::new(|| {
    // L6096
    let mut snl: TableBuilder<ModSpec> = TableBuilder::new();
    // -- Sigh.
    snl.set(
        " corpse cremation ",
        tagSpec(skillNameTag(n!("Cremation"))),
        6097,
    );
    // L6099
    let mut psnl: TableBuilder<ModSpec> = TableBuilder::new();
    let mut special: Vec<(&'static str, SpecialVal, u32)> = Vec::new();
    // L6100: for gemId, gemData in pairs(data.gems) — a key written twice gets the same value
    // (it depends only on skillName), so the order does not matter
    for (_gemId, gemData) in &data().gems {
        // L6101
        let grantedEffect = &gemData.grantedEffect;
        // L6102
        if !grantedEffect.hidden && !grantedEffect.support {
            // L6103
            let skillName: Name = grantedEffect.name;
            let sl = lower(skillName.as_str());
            // L6104
            snl.set_computed(&format!(" {sl} "), tagSpec(skillNameTag(skillName)), 6104);
            // L6105
            psnl.set_computed(&format!("^{sl} "), tagSpec(skillNameTag(skillName)), 6105);
            // L6106
            psnl.set_computed(
                &format!("^{sl} has ?a? "),
                tagSpec(skillNameTag(skillName)),
                6106,
            );
            // L6107
            psnl.set_computed(
                &format!("^{sl} deals "),
                tagSpec(skillNameTag(skillName)),
                6107,
            );
            // L6108
            psnl.set_computed(
                &format!("^{sl} damage "),
                tagSpec(skillNameTag(skillName)),
                6108,
            );
            // L6109
            if gemData.tags.contains(&n!("totem")) {
                // L6110
                psnl.set_computed(
                    &format!("^{sl} totem deals "),
                    tagSpec(skillNameTag(skillName)),
                    6110,
                );
                // L6111
                psnl.set_computed(&format!("^{sl} totem grants "), buffSpec(skillName), 6111);
            }
            // L6113
            if grantedEffect.skillTypes.has(SkillType::Buff)
                || grantedEffect.baseFlags.contains(&n!("buff"))
            {
                // L6114
                psnl.set_computed(&format!("^{sl} grants "), buffSpec(skillName), 6114);
                // L6115
                psnl.set_computed(
                    &format!("^{sl} grants a?n? ?additional "),
                    buffSpec(skillName),
                    6115,
                );
            }
            // L6117
            if gemData.tags.contains(&n!("aura")) || gemData.tags.contains(&n!("herald")) {
                // "AffectedBy"..skillName:gsub(" ","")
                let var = cat!(
                    "AffectedBy",
                    gsub(skillName.as_str(), " ", "", None).0.as_str()
                );
                // L6118
                snl.set_computed(
                    &format!("while affected by {sl}"),
                    tagSpec(tag!({ type = Condition, var = var })),
                    6118,
                );
                // L6119
                snl.set_computed(
                    &format!("while using {sl}"),
                    tagSpec(tag!({ type = Condition, var = var })),
                    6119,
                );
            }
            // L6121
            if gemData.tags.contains(&n!("curse")) {
                // "SelfCast"..skillName:gsub("^%l", string.upper):gsub(" %l", string.upper):gsub(" ", ""):gsub(" ","")
                let up = |s: &str, p: &str| gsub_fn(s, p, |c| Some(upper(&c[0].to_str())), None).0;
                let v = up(skillName.as_str(), "^%l");
                let v = up(&v, " %l");
                let v = gsub(&v, " ", "", None).0;
                let v = gsub(&v, " ", "", None).0;
                // L6122
                snl.set_computed(
                    &format!("if you've cast {sl} in the past (%d+) seconds"),
                    tagSpec(tag!({ type = Condition, var = cat!("SelfCast", v.as_str()) })),
                    6122,
                );
            }
            // L6124
            if gemData.tags.contains(&n!("mine")) {
                // L6125
                special.push((
                    key(format!("^{sl} has (%d+)%% increased throwing speed")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("MineLayingSpeed"),
                                ModType::INC,
                                num,
                                None,
                                0,
                                0,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6125,
                ));
            }
            // L6127
            if gemData.tags.contains(&n!("trap")) {
                // L6128
                special.push((
                    key(format!("(%d+)%% increased {sl} throwing speed")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("TrapThrowingSpeed"),
                                ModType::INC,
                                num,
                                None,
                                0,
                                0,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6128,
                ));
            }
            // L6130
            if gemData.tags.contains(&n!("chaining")) {
                // L6131
                special.push((
                    key(format!("^{sl} chains an additional time")),
                    SpecialVal::Mods(vec![r#mod(
                        n!("ExtraSkillMod"),
                        ModType::LIST,
                        mod_payload(r#mod(
                            n!("ChainCountMax"),
                            ModType::BASE,
                            1.0,
                            None,
                            0,
                            0,
                            &[],
                        )),
                        None,
                        0,
                        0,
                        &[skillNameTag(skillName)],
                    )]),
                    6131,
                ));
                // L6132
                special.push((
                    key(format!("^{sl} chains an additional (%d+) times")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("ChainCountMax"),
                                ModType::BASE,
                                num,
                                None,
                                0,
                                0,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6132,
                ));
                // L6133
                special.push((
                    key(format!("^{sl} chains (%d+) additional times")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("ChainCountMax"),
                                ModType::BASE,
                                num,
                                None,
                                0,
                                0,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6133,
                ));
            }
            // L6135
            if gemData.tags.contains(&n!("bow")) {
                // L6136
                special.push((
                    key(format!("^{sl} fires an additional arrow")),
                    SpecialVal::Dyn(Arc::new(move |_caps: &Caps| {
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("ProjectileCount"),
                                ModType::BASE,
                                1.0,
                                None,
                                0,
                                KeywordFlag::Arrow,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6136,
                ));
                // L6137
                special.push((
                    key(format!("^{sl} fires (%d+) additional arrows?")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("ProjectileCount"),
                                ModType::BASE,
                                num,
                                None,
                                0,
                                KeywordFlag::Arrow,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6137,
                ));
            }
            // L6139
            if gemData.tags.contains(&n!("projectile")) {
                // L6140
                special.push((
                    key(format!("^{sl} pierces an additional target")),
                    SpecialVal::Mods(vec![r#mod(
                        n!("PierceCount"),
                        ModType::BASE,
                        1.0,
                        None,
                        0,
                        0,
                        &[skillNameTag(skillName)],
                    )]),
                    6140,
                ));
                // L6141
                special.push((
                    key(format!("^{sl} pierces (%d+) additional targets?")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("PierceCount"),
                            ModType::BASE,
                            num,
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6141,
                ));
            }
            // L6143
            if gemData.tags.contains(&n!("bow")) || gemData.tags.contains(&n!("projectile")) {
                // L6144
                special.push((
                    key(format!("^{sl} fires an additional projectile")),
                    SpecialVal::Mods(vec![r#mod(
                        n!("ExtraSkillMod"),
                        ModType::LIST,
                        mod_payload(r#mod(
                            n!("ProjectileCount"),
                            ModType::BASE,
                            1.0,
                            None,
                            0,
                            0,
                            &[],
                        )),
                        None,
                        0,
                        0,
                        &[skillNameTag(skillName)],
                    )]),
                    6144,
                ));
                // L6145
                special.push((
                    key(format!("^{sl} fires (%d+) additional projectiles")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("ProjectileCount"),
                                ModType::BASE,
                                num,
                                None,
                                0,
                                0,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6145,
                ));
                // L6146
                special.push((
                    key(format!("^{sl} fires (%d+) additional shard projectiles")),
                    SpecialVal::Dyn(Arc::new(move |caps: &Caps| {
                        let num = caps.num;
                        Some(vec![r#mod(
                            n!("ExtraSkillMod"),
                            ModType::LIST,
                            mod_payload(r#mod(
                                n!("ProjectileCount"),
                                ModType::BASE,
                                num,
                                None,
                                0,
                                0,
                                &[],
                            )),
                            None,
                            0,
                            0,
                            &[skillNameTag(skillName)],
                        )])
                    })),
                    6146,
                ));
            }
        }
    }
    SkillLists {
        skillNameList: snl.finish(),
        preSkillNameList: psnl.finish(),
        special,
    }
});

/// `skillNameList` (6096-6098, 6104, 6118-6122).
pub static skillNameList: LazyLock<&'static PatternList<ModSpec>> =
    LazyLock::new(|| &SKILL_LISTS.skillNameList);
/// `preSkillNameList` (6099, 6105-6115).
pub static preSkillNameList: LazyLock<&'static PatternList<ModSpec>> =
    LazyLock::new(|| &SKILL_LISTS.preSkillNameList);

/// The generated `specialModList` writes of the gem loop (6124-6147), as (key, line), for tests.
pub fn generatedSkillSpecialKeys() -> Vec<(&'static str, u32)> {
    SKILL_LISTS
        .special
        .iter()
        .map(|(k, _, l)| (*k, *l))
        .collect()
}
