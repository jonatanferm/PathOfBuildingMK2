//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 6151-6607: the jewel func tables (`jewelOtherFuncs`, `jewelSelfFuncs`,
//! `jewelSelfUnallocFuncs`, `jewelThresholdFuncs`), `getPerStat`, `getThreshold`, `jewelFuncList` and
//! `clusterJewelSkills`. The parser emits `Value::JewelFunc(JewelFuncSpec)`; the bodies run in
//! `pob_calc::modules::jewel_funcs::callJewelFunc`.
//!
//! Owner: MP7.
//!
//! # Closures as data
//!
//! Every value of a `jewel*Funcs` table is a [`JewelFuncSpec`]:
//!
//! * a helper's closure: `key` `"$getSimpleConv"` (helpers.rs), `"$getPerStat"`, `"$getThreshold"`,
//!   with the helper's upvalues in `args` (documented on each helper);
//! * a closure written inline in the table: `key` = its source range
//!   `"Modules/ModParser.lua:<a>-<b>"` (the parse oracle's `src` of that function), `args` empty
//!   (the closure has no upvalues). Two kinds exist: node funcs `function(node, out, data)` (run by
//!   `callJewelFunc`), and capture closures `function(num, …) return <node func> end`, which
//!   [`callCaptureClosure`] evaluates (Lua `v(cap1, …, cap5)`) into the node func they return:
//!   either a helper spec, or the inner closure's range with the captures it closes over in `args`;
//! * a table of node funcs (`jewelThresholdFuncs`, 6544-6547): `key` `"$list"`, `args` the funcs as
//!   `Value::JewelFunc`.
//!
//! `jewelFuncList` holds, per lowercased key, the spec the driver emits (`key` = the lowercased
//! key, `r#type` = its type; CCR-0014 §3). [`jewelFuncListSource`] maps that key back to the table
//! value `v` the jewelFuncList wrapper closes over (6554-6592), which `callJewelFunc` runs.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::helpers::{flag, getSimpleConv, mod_payload, r#mod, vtab};
use pob_gamedata::{
    data, JewelFuncKind, JewelFuncSpec, KeywordFlag, Mod, ModFlag, ModType, Name, SkillType, Tag,
    Value, ValueTable,
};
use pob_lua::bit::bor;
use pob_lua::n;
use pob_lua::string::lower;
use std::sync::{Arc, LazyLock};

// ---- Rust-only plumbing ---------------------------------------------------------------------

const SRC: &str = "Modules/ModParser.lua";

/// An inline closure of a jewel*Funcs table, by its source range (no upvalues).
fn closure(a: u32, b: u32) -> JewelFuncSpec {
    JewelFuncSpec {
        key: Name::from(format!("{SRC}:{a}-{b}").as_str()),
        kind: JewelFuncKind::Other,
        args: smallvec::SmallVec::new(),
        r#type: None,
        radius: None,
    }
}

/// An inner closure returned by a capture closure, with the upvalues it closes over.
fn inner(a: u32, b: u32, upvalues: &[Value]) -> JewelFuncSpec {
    let mut s = closure(a, b);
    s.args = upvalues.iter().cloned().collect();
    s
}

/// `{ "Str", "Int" }` (getThreshold's table `attrib`).
fn attrib_list(xs: &[&str]) -> Value {
    Value::Table(Arc::new(ValueTable {
        array: xs.iter().map(|s| Value::from(*s)).collect(),
        fields: Vec::new(),
    }))
}

fn e(k: &'static str, v: JewelFuncSpec) -> (&'static str, JewelFuncSpec) {
    (k, v)
}

/// Lua arithmetic on a value (`num / 100`): numbers, and strings `tonumber` accepts; anything
/// else raises (`attempt to perform arithmetic on local 'num' (a nil value)`).
fn arith(v: &Value, local: &str) -> f64 {
    match v {
        Value::Num(x) => *x,
        Value::Str(s) => match pob_lua::tonumber(s) {
            Some(x) => x,
            None => pob_lua::lua_error!(
                "attempt to perform arithmetic on local '{}' (a string value)",
                local
            ),
        },
        other => pob_lua::lua_error!(
            "attempt to perform arithmetic on local '{}' (a {} value)",
            local,
            other.lua_type()
        ),
    }
}

/// Capture `i` (0-based) of a `v(cap1, …, cap5)` call (`nil` when absent).
fn cap(caps: &[Value], i: usize) -> Value {
    caps.get(i).cloned().unwrap_or(Value::Nil)
}

// ---- the tables ---------------------------------------------------------------------------

/// @pob Modules/ModParser.lua:6151-6366 h=86149ab2
/// `jewelOtherFuncs` (6151-6366): key → func, in source order.
pub static jewelOtherFuncs: LazyLock<Vec<(&'static str, JewelFuncSpec)>> = LazyLock::new(|| {
    let B = ModType::BASE;
    let I = ModType::INC;
    vec![
        // L6152
        e("Strength from Passives in Radius is Transformed to Dexterity", getSimpleConv(&["Str"], "Dex", B, true, None, None)),
        // L6153
        e("Dexterity from Passives in Radius is Transformed to Strength", getSimpleConv(&["Dex"], "Str", B, true, None, None)),
        // L6154
        e("Strength from Passives in Radius is Transformed to Intelligence", getSimpleConv(&["Str"], "Int", B, true, None, None)),
        // L6155
        e("Intelligence from Passives in Radius is Transformed to Strength", getSimpleConv(&["Int"], "Str", B, true, None, None)),
        // L6156
        e("Dexterity from Passives in Radius is Transformed to Intelligence", getSimpleConv(&["Dex"], "Int", B, true, None, None)),
        // L6157
        e("Intelligence from Passives in Radius is Transformed to Dexterity", getSimpleConv(&["Int"], "Dex", B, true, None, None)),
        // L6158
        e("Increases and Reductions to Life in Radius are Transformed to apply to Energy Shield", getSimpleConv(&["Life"], "EnergyShield", I, true, None, None)),
        // L6159
        e("Increases and Reductions to Evasion Rating in Radius are Transformed to apply to Armour", getSimpleConv(&["Evasion"], "Armour", I, true, None, None)),
        // L6160
        e("Increases and Reductions to Energy Shield in Radius are Transformed to apply to Armour at (%d+)%% of their value", closure(6160, 6162)),
        // L6163
        e("Increases and Reductions to Life in Radius are Transformed to apply to Mana at (%d+)%% of their value", closure(6163, 6165)),
        // L6166
        e("Increases and Reductions to Physical Damage in Radius are Transformed to apply to Cold Damage", getSimpleConv(&["PhysicalDamage"], "ColdDamage", I, true, None, None)),
        // L6167
        e("Increases and Reductions to Cold Damage in Radius are Transformed to apply to Physical Damage", getSimpleConv(&["ColdDamage"], "PhysicalDamage", I, true, None, None)),
        // L6168
        e("Increases and Reductions to other Damage Types in Radius are Transformed to apply to Fire Damage", getSimpleConv(&["PhysicalDamage", "ColdDamage", "LightningDamage", "ChaosDamage", "ElementalDamage"], "FireDamage", I, true, None, None)),
        // L6169
        e("Passives granting Lightning Resistance or all Elemental Resistances in Radius also grant Chance to Block Spells? ?D?a?m?a?g?e? at (%d+)%% of its value", closure(6169, 6171)),
        // L6172
        e("Passives granting Lightning Resistance or all Elemental Resistances in Radius also grant increased Maximum Energy Shield at (%d+)%% of its value", closure(6172, 6174)),
        // L6175
        e("Passives granting Lightning Resistance or all Elemental Resistances in Radius also grant Lightning Damage converted to Chaos Damage at (%d+)%% of its value", closure(6175, 6177)),
        // L6178
        e("Passives granting Cold Resistance or all Elemental Resistances in Radius also grant Chance to Dodge Attacks? ?H?i?t?s? at (%d+)%% of its value", closure(6178, 6180)),
        // L6181
        e("Passives granting Cold Resistance or all Elemental Resistances in Radius also grant Chance to Suppress Spell Damage at (%d+)%% of its value", closure(6181, 6183)),
        // L6184
        e("Passives granting Cold Resistance or all Elemental Resistances in Radius also grant increased Maximum Mana at (%d+)%% of its value", closure(6184, 6186)),
        // L6187
        e("Passives granting Cold Resistance or all Elemental Resistances in Radius also grant Cold Damage converted to Chaos Damage at (%d+)%% of its value", closure(6187, 6189)),
        // L6190
        e("Passives granting Fire Resistance or all Elemental Resistances in Radius also grant Chance to Block Attack Damage at (%d+)%% of its value", closure(6190, 6192)),
        // L6193
        e("Passives granting Fire Resistance or all Elemental Resistances in Radius also grant Chance to Block at (%d+)%% of its value", closure(6193, 6195)),
        // L6196
        e("Passives granting Fire Resistance or all Elemental Resistances in Radius also grant increased Maximum Life at (%d+)%% of its value", closure(6196, 6198)),
        // L6199
        e("Passives granting Fire Resistance or all Elemental Resistances in Radius also grant Fire Damage converted to Chaos Damage at (%d+)%% of its value", closure(6199, 6201)),
        // L6202
        e("Melee and Melee Weapon Type modifiers in Radius are Transformed to Bow Modifiers", closure(6202, 6230)),
        // L6231
        e("(%d+)%% increased Effect of non%-Keystone Passive Skills in Radius", closure(6231, 6237)),
        // L6238
        e("Notable Passive Skills in Radius grant nothing", closure(6238, 6242)),
        // L6243
        e("(%d+)%% increased effect of Tattoos in Radius", closure(6243, 6249)),
        // L6250
        e("Allocated Small Passive Skills in Radius grant nothing", closure(6250, 6254)),
        // L6255
        e("Allocated Notable Passive Skills in Radius grant nothing", closure(6255, 6259)),
        // L6260
        e("Passive Skills in Radius also grant: Traps and Mines deal (%d+) to (%d+) added Physical Damage", closure(6260, 6267)),
        // L6268
        e("Passive Skills in Radius also grant: (%d+)%% increased Unarmed Attack Speed with Melee Skills", closure(6268, 6274)),
        // L6275
        e("Passive Skills in Radius also grant (%d+)%% increased Global Critical Strike Chance", closure(6275, 6281)),
        // L6282
        e("Passive Skills in Radius also grant %+(%d+) to Maximum Life", closure(6282, 6288)),
        // L6289
        e("Passive Skills in Radius also grant %+(%d+) to Maximum Mana", closure(6289, 6295)),
        // L6296
        e("Passive Skills in Radius also grant (%d+)%% increased Energy Shield", closure(6296, 6302)),
        // L6303
        e("Passive Skills in Radius also grant (%d+)%% increased Armour", closure(6303, 6309)),
        // L6310
        e("Passive Skills in Radius also grant (%d+)%% increased Evasion Rating", closure(6310, 6316)),
        // L6317
        e("Passive Skills in Radius also grant %+(%d+) to all Attributes", closure(6317, 6326)),
        // L6327
        e("Passive Skills in Radius also grant %+(%d+)%% to Chaos Resistance", closure(6327, 6333)),
        // L6334
        e("Passive Skills in Radius also grant (%d+)%% increased (%w+) Damage", closure(6334, 6340)),
        // L6341
        e("Notable Passive Skills in Radius are Transformed to instead grant: (%d+)%% increased Mana Cost of Skills and (%d+)%% increased Spell Damage", closure(6341, 6349)),
        // L6350
        e("Notable Passive Skills in Radius are Transformed to instead grant: Minions take (%d+)%% increased Damage", closure(6350, 6357)),
        // L6358
        e("Notable Passive Skills in Radius are Transformed to instead grant: Minions have (%d+)%% reduced Movement Speed", closure(6358, 6365)),
    ]
});

// Radius jewels that modify the jewel itself based on nearby allocated nodes
/// @pob Modules/ModParser.lua:6369-6377 h=00a599ce
/// Returns the closure `function(node, out, data)` as data: `key` `"$getPerStat"`, `args` the
/// upvalues `[dst, modType, flags, stat, factor]` (the body runs in pob-calc).
pub fn getPerStat(
    dst: &str,
    modType: ModType,
    flags: u32,
    stat: &str,
    factor: f64,
) -> JewelFuncSpec {
    // L6370: return function(node, out, data) ... end
    let mut args: smallvec::SmallVec<[Value; 4]> = smallvec::SmallVec::new();
    args.push(Value::from(dst));
    args.push(Value::from(modType.as_str()));
    args.push(Value::Num(flags as f64));
    args.push(Value::from(stat));
    args.push(Value::Num(factor));
    JewelFuncSpec {
        key: n!("$getPerStat"),
        kind: JewelFuncKind::Other,
        args,
        r#type: None,
        radius: None,
    }
}

/// @pob Modules/ModParser.lua:6378-6408 h=56aa6580
/// `jewelSelfFuncs` (6378-6408).
pub static jewelSelfFuncs: LazyLock<Vec<(&'static str, JewelFuncSpec)>> = LazyLock::new(|| {
    let B = ModType::BASE;
    let I = ModType::INC;
    vec![
        // L6379
        e("Adds 1 to maximum Life per 3 Intelligence in Radius", getPerStat("Life", B, 0, "Int", 1.0 / 3.0)),
        // L6380
        e("Adds 1 to Maximum Life per 3 Intelligence Allocated in Radius", getPerStat("Life", B, 0, "Int", 1.0 / 3.0)),
        // L6381
        e("1% increased Evasion Rating per 3 Dexterity Allocated in Radius", getPerStat("Evasion", I, 0, "Dex", 1.0 / 3.0)),
        // L6382
        e("1% increased Claw Physical Damage per 3 Dexterity Allocated in Radius", getPerStat("PhysicalDamage", I, ModFlag::Claw, "Dex", 1.0 / 3.0)),
        // L6383
        e("1% increased Melee Physical Damage while Unarmed per 3 Dexterity Allocated in Radius", getPerStat("PhysicalDamage", I, ModFlag::Unarmed, "Dex", 1.0 / 3.0)),
        // L6384
        e("3% increased Totem Life per 10 Strength in Radius", getPerStat("TotemLife", I, 0, "Str", 3.0 / 10.0)),
        // L6385
        e("3% increased Totem Life per 10 Strength Allocated in Radius", getPerStat("TotemLife", I, 0, "Str", 3.0 / 10.0)),
        // L6386
        e("Adds 1 maximum Lightning Damage to Attacks per 1 Dexterity Allocated in Radius", getPerStat("LightningMax", B, ModFlag::Attack, "Dex", 1.0)),
        // L6387
        e("5% increased Chaos damage per 10 Intelligence from Allocated Passives in Radius", getPerStat("ChaosDamage", I, 0, "Int", 5.0 / 10.0)),
        // L6388
        e("Dexterity and Intelligence from passives in Radius count towards Strength Melee Damage bonus", closure(6388, 6395)),
        // L6396
        e("-1 Strength per 1 Strength on Allocated Passives in Radius", getPerStat("Str", B, 0, "Str", -1.0)),
        // L6397
        e("1% additional Physical Damage Reduction per 10 Strength on Allocated Passives in Radius", getPerStat("PhysicalDamageReduction", B, 0, "Str", 1.0 / 10.0)),
        // L6398
        e("2% increased Life Recovery Rate per 10 Strength on Allocated Passives in Radius", getPerStat("LifeRecoveryRate", I, 0, "Str", 2.0 / 10.0)),
        // L6399
        e("3% increased Life Recovery Rate per 10 Strength on Allocated Passives in Radius", getPerStat("LifeRecoveryRate", I, 0, "Str", 3.0 / 10.0)),
        // L6400
        e("-1 Intelligence per 1 Intelligence on Allocated Passives in Radius", getPerStat("Int", B, 0, "Int", -1.0)),
        // L6401
        e("0.4% of Energy Shield Regenerated per Second for every 10 Intelligence on Allocated Passives in Radius", getPerStat("EnergyShieldRegenPercent", B, 0, "Int", 0.4 / 10.0)),
        // L6402
        e("regenerate 0.4% of energy shield per second for every 10 Intelligence on Allocated Passives in Radius", getPerStat("EnergyShieldRegenPercent", B, 0, "Int", 0.4 / 10.0)),
        // L6403
        e("2% increased Mana Recovery Rate per 10 Intelligence on Allocated Passives in Radius", getPerStat("ManaRecoveryRate", I, 0, "Int", 2.0 / 10.0)),
        // L6404
        e("3% increased Mana Recovery Rate per 10 Intelligence on Allocated Passives in Radius", getPerStat("ManaRecoveryRate", I, 0, "Int", 3.0 / 10.0)),
        // L6405
        e("-1 Dexterity per 1 Dexterity on Allocated Passives in Radius", getPerStat("Dex", B, 0, "Dex", -1.0)),
        // L6406
        e("2% increased Movement Speed per 10 Dexterity on Allocated Passives in Radius", getPerStat("MovementSpeed", I, 0, "Dex", 2.0 / 10.0)),
        // L6407
        e("3% increased Movement Speed per 10 Dexterity on Allocated Passives in Radius", getPerStat("MovementSpeed", I, 0, "Dex", 3.0 / 10.0)),
    ]
});

/// @pob Modules/ModParser.lua:6409-6452 h=e3b44d48
/// `jewelSelfUnallocFuncs` (6409-6452).
pub static jewelSelfUnallocFuncs: LazyLock<Vec<(&'static str, JewelFuncSpec)>> = LazyLock::new(
    || {
        let B = ModType::BASE;
        let I = ModType::INC;
        vec![
            // L6410
            e("+5% to Critical Strike Multiplier per 10 Strength on Unallocated Passives in Radius", getPerStat("CritMultiplier", B, 0, "Str", 5.0 / 10.0)),
            // L6411
            e("+7% to Critical Strike Multiplier per 10 Strength on Unallocated Passives in Radius", getPerStat("CritMultiplier", B, 0, "Str", 7.0 / 10.0)),
            // L6412
            e("2% reduced Life Recovery Rate per 10 Strength on Unallocated Passives in Radius", getPerStat("LifeRecoveryRate", I, 0, "Str", -2.0 / 10.0)),
            // L6413
            e("+15 to maximum Mana per 10 Dexterity on Unallocated Passives in Radius", getPerStat("Mana", B, 0, "Dex", 15.0 / 10.0)),
            // L6414
            e("+100 to Accuracy Rating per 10 Intelligence on Unallocated Passives in Radius", getPerStat("Accuracy", B, 0, "Int", 100.0 / 10.0)),
            // L6415
            e("+125 to Accuracy Rating per 10 Intelligence on Unallocated Passives in Radius", getPerStat("Accuracy", B, 0, "Int", 125.0 / 10.0)),
            // L6416
            e("2% reduced Mana Recovery Rate per 10 Intelligence on Unallocated Passives in Radius", getPerStat("ManaRecoveryRate", I, 0, "Int", -2.0 / 10.0)),
            // L6417
            e("+3% to Damage over Time Multiplier per 10 Intelligence on Unallocated Passives in Radius", getPerStat("DotMultiplier", B, 0, "Int", 3.0 / 10.0)),
            // L6418
            e("2% reduced Movement Speed per 10 Dexterity on Unallocated Passives in Radius", getPerStat("MovementSpeed", I, 0, "Dex", -2.0 / 10.0)),
            // L6419
            e("+125 to Accuracy Rating per 10 Dexterity on Unallocated Passives in Radius", getPerStat("Accuracy", B, 0, "Dex", 125.0 / 10.0)),
            // L6420
            e("Grants all bonuses of Unallocated Small Passive Skills in Radius", closure(6420, 6435)),
            // L6436
            e("Grants all bonuses of Unallocated Notable Passive Skills in Radius", closure(6436, 6451)),
        ]
    },
);

// Radius jewels with bonuses conditional upon attributes of nearby nodes
/// @pob Modules/ModParser.lua:6455-6479 h=51ba613c
/// Returns the closure as data: `key` `"$getThreshold"`, `args` the upvalues
/// `[attrib (a string or an array table), baseMod (Value::Mod)]`; `value` is `baseMod.value`.
/// `...` (flags, keywordFlags, tags) is passed resolved (createMod's dispatch, CCR-0006 §6;
/// CCR-0039: the frozen stub took `attrib: &str` and no flags).
pub fn getThreshold(
    attrib: Value,
    name: &str,
    modType: ModType,
    value: Value,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) -> JewelFuncSpec {
    // L6456
    let baseMod = r#mod(
        Name::from(name),
        modType,
        value,
        Some(""),
        flags,
        keywordFlags,
        tags,
    );
    // L6457: return function(node, out, data) ... end
    let mut args: smallvec::SmallVec<[Value; 4]> = smallvec::SmallVec::new();
    args.push(attrib);
    args.push(Value::Mod(Arc::new(baseMod)));
    JewelFuncSpec {
        key: n!("$getThreshold"),
        kind: JewelFuncKind::Threshold,
        args,
        r#type: None,
        radius: None,
    }
}

/// @pob Modules/ModParser.lua:6480-6549 h=1e16641b
/// `jewelThresholdFuncs` (6480-6549).
pub static jewelThresholdFuncs: LazyLock<Vec<(&'static str, JewelFuncSpec)>> = LazyLock::new(
    || {
        vec![
            // L6481
            e("With at least 40 Dexterity in Radius, Frost Blades Melee Damage Penetrates 15% Cold Resistance", getThreshold(Value::from("Dex"), "ColdPenetration", ModType::BASE, Value::Num(15.0), ModFlag::Melee, 0, &[tag!({ type = SkillName, skillName = "Frost Blades", includeTransfigured = true })])),
            // L6482
            e("With at least 40 Dexterity in Radius, Melee Damage dealt by Frost Blades Penetrates 15% Cold Resistance", getThreshold(Value::from("Dex"), "ColdPenetration", ModType::BASE, Value::Num(15.0), ModFlag::Melee, 0, &[tag!({ type = SkillName, skillName = "Frost Blades", includeTransfigured = true })])),
            // L6483
            e("With at least 40 Dexterity in Radius, Frost Blades has 25% increased Projectile Speed", getThreshold(Value::from("Dex"), "ProjectileSpeed", ModType::INC, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Frost Blades", includeTransfigured = true })])),
            // L6484
            e("With at least 40 Dexterity in Radius, Ice Shot has 25% increased Area of Effect", getThreshold(Value::from("Dex"), "AreaOfEffect", ModType::INC, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Ice Shot" })])),
            // L6485
            e("Ice Shot Pierces 5 additional Targets with 40 Dexterity in Radius", getThreshold(Value::from("Dex"), "PierceCount", ModType::BASE, Value::Num(5.0), 0, 0, &[tag!({ type = SkillName, skillName = "Ice Shot" })])),
            // L6486
            e("With at least 40 Dexterity in Radius, Ice Shot Pierces 3 additional Targets", getThreshold(Value::from("Dex"), "PierceCount", ModType::BASE, Value::Num(3.0), 0, 0, &[tag!({ type = SkillName, skillName = "Ice Shot" })])),
            // L6487
            e("With at least 40 Dexterity in Radius, Ice Shot Pierces 5 additional Targets", getThreshold(Value::from("Dex"), "PierceCount", ModType::BASE, Value::Num(5.0), 0, 0, &[tag!({ type = SkillName, skillName = "Ice Shot" })])),
            // L6488
            e("With at least 40 Intelligence in Radius, Frostbolt fires 2 additional Projectiles", getThreshold(Value::from("Int"), "ProjectileCount", ModType::BASE, Value::Num(2.0), 0, 0, &[tag!({ type = SkillName, skillName = "Frostbolt" })])),
            // L6489
            e("With at least 40 Intelligence in Radius, Rolling Magma fires an additional Projectile", getThreshold(Value::from("Int"), "ProjectileCount", ModType::BASE, Value::Num(1.0), 0, 0, &[tag!({ type = SkillName, skillName = "Rolling Magma" })])),
            // L6490
            e("With at least 40 Intelligence in Radius, Rolling Magma has 10% increased Area of Effect per Chain", getThreshold(Value::from("Int"), "AreaOfEffect", ModType::INC, Value::Num(10.0), 0, 0, &[tag!({ type = SkillName, skillName = "Rolling Magma" }), tag!({ type = PerStat, stat = "Chain" })])),
            // L6491
            e("With at least 40 Intelligence in Radius, Rolling Magma deals 40% more damage per chain", getThreshold(Value::from("Int"), "Damage", ModType::MORE, Value::Num(40.0), 0, 0, &[tag!({ type = SkillName, skillName = "Rolling Magma" }), tag!({ type = PerStat, stat = "Chain" })])),
            // L6492
            e("With at least 40 Intelligence in Radius, Rolling Magma deals 50% less damage", getThreshold(Value::from("Int"), "Damage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillName, skillName = "Rolling Magma" })])),
            // L6493
            e("With at least 40 Dexterity in Radius, Shrapnel Shot has 25% increased Area of Effect", getThreshold(Value::from("Dex"), "AreaOfEffect", ModType::INC, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Shrapnel Shot" })])),
            // L6494
            e("With at least 40 Dexterity in Radius, Shrapnel Shot's cone has a 50% chance to deal Double Damage", getThreshold(Value::from("Dex"), "DoubleDamageChance", ModType::BASE, Value::Num(50.0), 0, 0, &[tag!({ type = SkillName, skillName = "Shrapnel Shot" }), tag!({ type = SkillPart, skillPart = 2.0 })])),
            // L6495
            e("With at least 40 Dexterity in Radius, Galvanic Arrow deals 50% increased Area Damage", getThreshold(Value::from("Dex"), "Damage", ModType::INC, Value::Num(50.0), 0, 0, &[tag!({ type = SkillName, skillName = "Galvanic Arrow", includeTransfigured = true }), tag!({ type = SkillPart, skillPart = 2.0 })])),
            // L6496
            e("With at least 40 Dexterity in Radius, Galvanic Arrow has 25% increased Area of Effect", getThreshold(Value::from("Dex"), "AreaOfEffect", ModType::INC, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Galvanic Arrow", includeTransfigured = true })])),
            // L6497
            e("With at least 40 Intelligence in Radius, Freezing Pulse fires 2 additional Projectiles", getThreshold(Value::from("Int"), "ProjectileCount", ModType::BASE, Value::Num(2.0), 0, 0, &[tag!({ type = SkillName, skillName = "Freezing Pulse" })])),
            // L6498
            e("With at least 40 Intelligence in Radius, 25% increased Freezing Pulse Damage if you've Shattered an Enemy Recently", getThreshold(Value::from("Int"), "Damage", ModType::INC, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Freezing Pulse" }), tag!({ type = Condition, var = "ShatteredEnemyRecently" })])),
            // L6499
            e("With at least 40 Dexterity in Radius, Ethereal Knives fires 10 additional Projectiles", getThreshold(Value::from("Dex"), "ProjectileCount", ModType::BASE, Value::Num(10.0), 0, 0, &[tag!({ type = SkillName, skillName = "Ethereal Knives", includeTransfigured = true })])),
            // L6500
            e("With at least 40 Dexterity in Radius, Ethereal Knives fires 5 additional Projectiles", getThreshold(Value::from("Dex"), "ProjectileCount", ModType::BASE, Value::Num(5.0), 0, 0, &[tag!({ type = SkillName, skillName = "Ethereal Knives", includeTransfigured = true })])),
            // L6501
            e("With at least 40 Strength in Radius, Molten Strike fires 2 additional Projectiles", getThreshold(Value::from("Str"), "ProjectileCount", ModType::BASE, Value::Num(2.0), 0, 0, &[tag!({ type = SkillName, skillName = "Molten Strike", includeTransfigured = true })])),
            // L6502
            e("With at least 40 Strength in Radius, Molten Strike has 25% increased Area of Effect", getThreshold(Value::from("Str"), "AreaOfEffect", ModType::INC, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Molten Strike", includeTransfigured = true })])),
            // L6503
            e("With at least 40 Strength in Radius, Molten Strike Projectiles Chain +1 time", getThreshold(Value::from("Str"), "ChainCountMax", ModType::BASE, Value::Num(1.0), 0, 0, &[tag!({ type = SkillName, skillName = "Molten Strike", includeTransfigured = true })])),
            // L6504
            e("With at least 40 Strength in Radius, Molten Strike fires 50% less Projectiles", getThreshold(Value::from("Str"), "ProjectileCount", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillName, skillName = "Molten Strike", includeTransfigured = true })])),
            // L6505
            e("With at least 40 Strength in Radius, 25% of Glacial Hammer Physical Damage converted to Cold Damage", getThreshold(Value::from("Str"), "SkillPhysicalDamageConvertToCold", ModType::BASE, Value::Num(25.0), 0, 0, &[tag!({ type = SkillName, skillName = "Glacial Hammer", includeTransfigured = true })])),
            // L6506
            e("With at least 40 Strength in Radius, Heavy Strike has a 20% chance to deal Double Damage", getThreshold(Value::from("Str"), "DoubleDamageChance", ModType::BASE, Value::Num(20.0), 0, 0, &[tag!({ type = SkillName, skillName = "Heavy Strike" })])),
            // L6507
            e("With at least 40 Strength in Radius, Heavy Strike has a 20% chance to deal Double Damage.", getThreshold(Value::from("Str"), "DoubleDamageChance", ModType::BASE, Value::Num(20.0), 0, 0, &[tag!({ type = SkillName, skillName = "Heavy Strike" })])),
            // L6508
            e("With at least 40 Strength in Radius, Cleave has +1 to Radius per Nearby Enemy, up to +10", getThreshold(Value::from("Str"), "AreaOfEffect", ModType::BASE, Value::Num(1.0), 0, 0, &[tag!({ type = Multiplier, var = "NearbyEnemies", limit = 10.0 }), tag!({ type = SkillName, skillName = "Cleave", includeTransfigured = true })])),
            // L6509
            e("With at least 40 Strength in Radius, Cleave has +0.1 metres to Radius per Nearby Enemy, up to a maximum of +1 metre", getThreshold(Value::from("Str"), "AreaOfEffect", ModType::BASE, Value::Num(1.0), 0, 0, &[tag!({ type = Multiplier, var = "NearbyEnemies", limit = 10.0 }), tag!({ type = SkillName, skillName = "Cleave", includeTransfigured = true })])),
            // L6510
            e("With at least 40 Strength in Radius, Cleave grants Fortify on Hit", getThreshold(Value::from("Str"), "ExtraSkillMod", ModType::LIST, mod_payload(flag(n!("Condition:Fortified"), None, 0, 0, &[])), 0, 0, &[tag!({ type = SkillName, skillName = "Cleave", includeTransfigured = true })])),
            // L6511
            e("With at least 40 Strength in Radius, Hits with Cleave Fortify", getThreshold(Value::from("Str"), "ExtraSkillMod", ModType::LIST, mod_payload(flag(n!("Condition:Fortified"), None, 0, 0, &[])), 0, 0, &[tag!({ type = SkillName, skillName = "Cleave", includeTransfigured = true })])),
            // L6512
            e("With at least 40 Dexterity in Radius, Dual Strike has a 20% chance to deal Double Damage with the Main-Hand Weapon", getThreshold(Value::from("Dex"), "DoubleDamageChance", ModType::BASE, Value::Num(20.0), 0, 0, &[tag!({ type = SkillName, skillName = "Dual Strike", includeTransfigured = true }), tag!({ type = Condition, var = "MainHandAttack" })])),
            // L6513
            e("With at least 40 Dexterity in Radius, Dual Strike has (%d+)%% increased Attack Speed while wielding a Claw", closure(6513, 6513)),
            // L6514
            e("With at least 40 Dexterity in Radius, Dual Strike has %+(%d+)%% to Critical Strike Multiplier while wielding a Dagger", closure(6514, 6514)),
            // L6515
            e("With at least 40 Dexterity in Radius, Dual Strike has (%d+)%% increased Accuracy Rating while wielding a Sword", closure(6515, 6515)),
            // L6516
            e("With at least 40 Dexterity in Radius, Dual Strike Hits Intimidate Enemies for 4 seconds while wielding an Axe", getThreshold(Value::from("Dex"), "EnemyModifier", ModType::LIST, mod_payload(flag(n!("Condition:Intimidated"), None, 0, 0, &[])), 0, 0, &[tag!({ type = Condition, var = "UsingAxe" })])),
            // L6517
            e("With at least 40 Intelligence in Radius, Raised Zombies' Slam Attack has 100% increased Cooldown Recovery Speed", getThreshold(Value::from("Int"), "MinionModifier", ModType::LIST, mod_payload(r#mod(n!("CooldownRecovery"), ModType::INC, 100.0, None, 0, 0, &[tag!({ type = SkillId, skillId = "ZombieSlam" })])), 0, 0, &[])),
            // L6518
            e("With at least 40 Intelligence in Radius, Raised Zombies' Slam Attack deals 30% increased Damage", getThreshold(Value::from("Int"), "MinionModifier", ModType::LIST, mod_payload(r#mod(n!("Damage"), ModType::INC, 30.0, None, 0, 0, &[tag!({ type = SkillId, skillId = "ZombieSlam" })])), 0, 0, &[])),
            // L6519
            e("With at least 40 Dexterity in Radius, Viper Strike deals 2% increased Attack Damage for each Poison on the Enemy", getThreshold(Value::from("Dex"), "Damage", ModType::INC, Value::Num(2.0), ModFlag::Attack, 0, &[tag!({ type = SkillName, skillName = "Viper Strike", includeTransfigured = true }), tag!({ type = Multiplier, actor = "enemy", var = "PoisonStack" })])),
            // L6520
            e("With at least 40 Dexterity in Radius, Viper Strike deals 2% increased Damage with Hits and Poison for each Poison on the Enemy", getThreshold(Value::from("Dex"), "Damage", ModType::INC, Value::Num(2.0), 0, bor(KeywordFlag::Hit, KeywordFlag::Poison), &[tag!({ type = SkillName, skillName = "Viper Strike", includeTransfigured = true }), tag!({ type = Multiplier, actor = "enemy", var = "PoisonStack" })])),
            // L6521
            e("With at least 40 Intelligence in Radius, Spark fires 2 additional Projectiles", getThreshold(Value::from("Int"), "ProjectileCount", ModType::BASE, Value::Num(2.0), 0, 0, &[tag!({ type = SkillName, skillName = "Spark", includeTransfigured = true })])),
            // L6522
            e("With at least 40 Intelligence in Radius, Blight has 50% increased Hinder Duration", getThreshold(Value::from("Int"), "SecondaryDuration", ModType::INC, Value::Num(50.0), 0, 0, &[tag!({ type = SkillName, skillName = "Blight", includeTransfigured = true })])),
            // L6523
            e("With at least 40 Intelligence in Radius, Enemies Hindered by Blight take 25% increased Chaos Damage", getThreshold(Value::from("Int"), "ExtraSkillMod", ModType::LIST, mod_payload(r#mod(n!("ChaosDamageTaken"), ModType::INC, 25.0, None, 0, 0, &[tag!({ type = GlobalEffect, effectType = "Debuff", effectName = "Hinder" })])), 0, 0, &[tag!({ type = SkillName, skillName = "Blight", includeTransfigured = true }), tag!({ type = ActorCondition, actor = "enemy", var = "Hindered" })])),
            // L6524
            e("With 40 Intelligence in Radius, 20% of Glacial Cascade Physical Damage Converted to Cold Damage", getThreshold(Value::from("Int"), "SkillPhysicalDamageConvertToCold", ModType::BASE, Value::Num(20.0), 0, 0, &[tag!({ type = SkillName, skillName = "Glacial Cascade", includeTransfigured = true })])),
            // L6525
            e("With at least 40 Intelligence in Radius, 20% of Glacial Cascade Physical Damage Converted to Cold Damage", getThreshold(Value::from("Int"), "SkillPhysicalDamageConvertToCold", ModType::BASE, Value::Num(20.0), 0, 0, &[tag!({ type = SkillName, skillName = "Glacial Cascade", includeTransfigured = true })])),
            // L6526
            e("With 40 total Intelligence and Dexterity in Radius, Elemental Hit and Wild Strike deal 50% less Fire Damage", getThreshold(attrib_list(&["Int", "Dex"]), "FireDamage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Elemental Hit"), n!("Wild Strike")][..], includeTransfigured = true })])),
            // L6527
            e("With 40 total Strength and Intelligence in Radius, Elemental Hit and Wild Strike deal 50% less Cold Damage", getThreshold(attrib_list(&["Str", "Int"]), "ColdDamage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Elemental Hit"), n!("Wild Strike")][..], includeTransfigured = true })])),
            // L6528
            e("With 40 total Dexterity and Strength in Radius, Elemental Hit and Wild Strike deal 50% less Lightning Damage", getThreshold(attrib_list(&["Dex", "Str"]), "LightningDamage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillName, skillNameList = &[n!("Elemental Hit"), n!("Wild Strike")][..], includeTransfigured = true })])),
            // L6529
            e("With 40 total Intelligence and Dexterity in Radius, Prismatic Skills deal 50% less Fire Damage", getThreshold(attrib_list(&["Int", "Dex"]), "FireDamage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillType, skillType = SkillType::RandomElement })])),
            // L6530
            e("With 40 total Strength and Intelligence in Radius, Prismatic Skills deal 50% less Cold Damage", getThreshold(attrib_list(&["Str", "Int"]), "ColdDamage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillType, skillType = SkillType::RandomElement })])),
            // L6531
            e("With 40 total Dexterity and Strength in Radius, Prismatic Skills deal 50% less Lightning Damage", getThreshold(attrib_list(&["Dex", "Str"]), "LightningDamage", ModType::MORE, Value::Num(-50.0), 0, 0, &[tag!({ type = SkillType, skillType = SkillType::RandomElement })])),
            // L6532
            e("With 40 total Dexterity and Strength in Radius, Spectral Shield Throw Chains +4 times", getThreshold(attrib_list(&["Dex", "Str"]), "ChainCountMax", ModType::BASE, Value::Num(4.0), 0, 0, &[tag!({ type = SkillName, skillName = "Spectral Shield Throw", includeTransfigured = true })])),
            // L6533
            e("With 40 total Dexterity and Strength in Radius, Spectral Shield Throw fires 75% less Shard Projectiles", getThreshold(attrib_list(&["Dex", "Str"]), "ProjectileCount", ModType::MORE, Value::Num(-75.0), 0, 0, &[tag!({ type = SkillName, skillName = "Spectral Shield Throw", includeTransfigured = true })])),
            // L6534
            e("With at least 40 Intelligence in Radius, Blight inflicts Withered for 2 seconds", getThreshold(Value::from("Int"), "ExtraSkillMod", ModType::LIST, mod_payload(r#mod(n!("Condition:CanWither"), ModType::FLAG, true, None, 0, 0, &[])), 0, 0, &[tag!({ type = SkillName, skillName = "Blight", includeTransfigured = true })])),
            // L6535
            e("With at least 40 Intelligence in Radius, Blight has 30% reduced Cast Speed", getThreshold(Value::from("Int"), "Speed", ModType::INC, Value::Num(-30.0), 0, 0, &[tag!({ type = SkillName, skillName = "Blight", includeTransfigured = true })])),
            // L6536
            e("With at least 40 Intelligence in Radius, Fireball cannot ignite", getThreshold(Value::from("Int"), "ExtraSkillMod", ModType::LIST, mod_payload(flag(n!("CannotIgnite"), None, 0, 0, &[])), 0, 0, &[tag!({ type = SkillName, skillName = "Fireball" })])),
            // L6537
            e("With at least 40 Intelligence in Radius, Fireball has %+(%d+)%% chance to inflict scorch", closure(6537, 6537)),
            // L6538
            e("With at least 40 Intelligence in Radius, Discharge has 60% less Area of Effect", getThreshold(Value::from("Int"), "AreaOfEffect", ModType::MORE, Value::Num(-60.0), 0, 0, &[tag!({ type = SkillName, skillName = "Discharge", includeTransfigured = true })])),
            // L6539
            e("With at least 40 Intelligence in Radius, Discharge Cooldown is 250 ms", getThreshold(Value::from("Int"), "CooldownRecovery", ModType::OVERRIDE, Value::Num(0.25), 0, 0, &[tag!({ type = SkillName, skillName = "Discharge", includeTransfigured = true })])),
            // L6540
            e("With at least 40 Intelligence in Radius, Discharge deals 60% less Damage", getThreshold(Value::from("Int"), "Damage", ModType::MORE, Value::Num(-60.0), 0, 0, &[tag!({ type = SkillName, skillName = "Discharge", includeTransfigured = true })])),
            // L6541
            e("With at least 40 Intelligence in Radius, (%d+)%% of Damage taken Recouped as Mana if you've Warcried Recently", closure(6541, 6541)),
            // L6542
            e("With at least 40 Intelligence in Radius, Fireball Projectiles gain Radius as they travel farther, up to %+(%d+) Radius", closure(6542, 6542)),
            // L6543
            e("With at least 40 Intelligence in Radius, Projectiles gain radius as they travel farther, up to a maximum of %+([%d%.]+) metres? to radius", closure(6543, 6543)),
            // L6544
            e("With at least 40 Intelligence in Radius, Raised Spectres have a 50% chance to gain Soul Eater for 20 seconds on Kill", {
                // L6545
                let a = getThreshold(Value::from("Int"), "MinionModifier", ModType::LIST, mod_payload(r#mod(n!("Condition:CanHaveSoulEater"), ModType::FLAG, true, None, 0, 0, &[])), 0, 0, &[tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true })]);
                // L6546
                let b = getThreshold(Value::from("Int"), "Condition:MinionCanHaveSoulEater", ModType::FLAG, Value::Bool(true), 0, 0, &[]);
                let mut args: smallvec::SmallVec<[Value; 4]> = smallvec::SmallVec::new();
                args.push(Value::JewelFunc(Arc::new(a)));
                args.push(Value::JewelFunc(Arc::new(b)));
                JewelFuncSpec { key: n!("$list"), kind: JewelFuncKind::Threshold, args, r#type: None, radius: None }
            }),
            // L6548: -- [""] = getThreshold("", "", "", , { type = "SkillName", skillName = "" }),
        ]
    },
);

/// `v(cap1, cap2, cap3, cap4, cap5)` for a capture closure value `v` of a jewel*Funcs table: the
/// node func it returns (a helper spec, or the inner closure with its upvalues). `None` when `v`
/// is not a capture closure. `caps` are Lua values: the pattern captures (strings) at parse time
/// (6562 / 6644-6646), or `node, out, data` when an exact-key `"Other"` wrapper calls `v` with
/// them (6562 again; `callJewelFunc` passes `node` as a table, `nil` for the finish call).
/// Arithmetic on a capture raises as in Lua (`num / 100` with `num` a table).
pub fn callCaptureClosure(v: &JewelFuncSpec, caps: &[Value]) -> Option<JewelFuncSpec> {
    let I = ModType::INC;
    let B = ModType::BASE;
    let num = cap(caps, 0);
    let lrc: &[&str] = &["LightningResist", "ElementalResist"];
    let crc: &[&str] = &["ColdResist", "ElementalResist"];
    let frc: &[&str] = &["FireResist", "ElementalResist"];
    let k = v.key.as_str().strip_prefix("Modules/ModParser.lua:")?;
    Some(match k {
        // L6161
        "6160-6162" => getSimpleConv(
            &["EnergyShield"],
            "Armour",
            I,
            true,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6164
        "6163-6165" => getSimpleConv(
            &["Life"],
            "Mana",
            I,
            true,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6170
        "6169-6171" => getSimpleConv(
            lrc,
            "SpellBlockChance",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6173
        "6172-6174" => getSimpleConv(
            lrc,
            "EnergyShield",
            I,
            false,
            Some(arith(&num, "num") / 100.0),
            Some(B),
        ),
        // L6176
        "6175-6177" => getSimpleConv(
            lrc,
            "LightningDamageConvertToChaos",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6179
        "6178-6180" => getSimpleConv(
            crc,
            "AttackDodgeChance",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6182
        "6181-6183" => getSimpleConv(
            crc,
            "SpellSuppressionChance",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6185
        "6184-6186" => getSimpleConv(
            crc,
            "Mana",
            I,
            false,
            Some(arith(&num, "num") / 100.0),
            Some(B),
        ),
        // L6188
        "6187-6189" => getSimpleConv(
            crc,
            "ColdDamageConvertToChaos",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6191
        "6190-6192" => getSimpleConv(
            frc,
            "BlockChance",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6194
        "6193-6195" => getSimpleConv(
            frc,
            "BlockChance",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6197
        "6196-6198" => getSimpleConv(
            frc,
            "Life",
            I,
            false,
            Some(arith(&num, "num") / 100.0),
            Some(B),
        ),
        // L6200
        "6199-6201" => getSimpleConv(
            frc,
            "FireDamageConvertToChaos",
            B,
            false,
            Some(arith(&num, "num") / 100.0),
            None,
        ),
        // L6232: return function(node, out, data) … end (upvalue num)
        "6231-6237" => inner(6232, 6236, &[num]),
        // L6244
        "6243-6249" => inner(6244, 6248, &[num]),
        // L6261 (upvalues min, max)
        "6260-6267" => inner(6261, 6266, &[num, cap(caps, 1)]),
        // L6269
        "6268-6274" => inner(6269, 6273, &[num]),
        // L6276
        "6275-6281" => inner(6276, 6280, &[num]),
        // L6283
        "6282-6288" => inner(6283, 6287, &[num]),
        // L6290
        "6289-6295" => inner(6290, 6294, &[num]),
        // L6297
        "6296-6302" => inner(6297, 6301, &[num]),
        // L6304
        "6303-6309" => inner(6304, 6308, &[num]),
        // L6311
        "6310-6316" => inner(6311, 6315, &[num]),
        // L6318
        "6317-6326" => inner(6318, 6325, &[num]),
        // L6328
        "6327-6333" => inner(6328, 6332, &[num]),
        // L6335 (upvalues num, type)
        "6334-6340" => inner(6335, 6339, &[num, cap(caps, 1)]),
        // L6342 (upvalues num1, num2)
        "6341-6349" => inner(6342, 6348, &[num, cap(caps, 1)]),
        // L6351
        "6350-6357" => inner(6351, 6356, &[num]),
        // L6359
        "6358-6365" => inner(6359, 6364, &[num]),
        // L6513
        "6513-6513" => getThreshold(
            Value::from("Dex"),
            "Speed",
            ModType::INC,
            num,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Dual Strike", includeTransfigured = true }),
                tag!({ type = Condition, var = "UsingClaw" }),
            ],
        ),
        // L6514
        "6514-6514" => getThreshold(
            Value::from("Dex"),
            "CritMultiplier",
            ModType::BASE,
            num,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Dual Strike", includeTransfigured = true }),
                tag!({ type = Condition, var = "UsingDagger" }),
            ],
        ),
        // L6515
        "6515-6515" => getThreshold(
            Value::from("Dex"),
            "Accuracy",
            ModType::INC,
            num,
            0,
            0,
            &[
                tag!({ type = SkillName, skillName = "Dual Strike", includeTransfigured = true }),
                tag!({ type = Condition, var = "UsingSword" }),
            ],
        ),
        // L6537
        "6537-6537" => getThreshold(
            Value::from("Int"),
            "EnemyScorchChance",
            ModType::BASE,
            num,
            0,
            0,
            &[tag!({ type = SkillName, skillName = "Fireball" })],
        ),
        // L6541
        "6541-6541" => getThreshold(
            Value::from("Int"),
            "ManaRecoup",
            ModType::BASE,
            num,
            0,
            0,
            &[tag!({ type = Condition, var = "UsedWarcryRecently" })],
        ),
        // L6542
        "6542-6542" => getThreshold(
            Value::from("Int"),
            "AreaOfEffect",
            ModType::BASE,
            num,
            0,
            0,
            &[tag!({ type = DistanceRamp, ramp = &[[0.0, 0.0], [50.0, 1.0]][..] })],
        ),
        // L6543
        "6543-6543" => getThreshold(
            Value::from("Int"),
            "AreaOfEffect",
            ModType::BASE,
            Value::Num(arith(&num, "num") * 10.0),
            0,
            0,
            &[tag!({ type = DistanceRamp, ramp = &[[0.0, 0.0], [50.0, 1.0]][..] })],
        ),
        _ => return None,
    })
}

/// The `{ func = …, type = … }` table of one jewelFuncList key (6555/6573/6576/6582/6591).
fn list_entry(k: &str, kind: JewelFuncKind, ty: Name) -> (&'static str, JewelFuncSpec) {
    let key = Name::from(lower(k).as_str());
    (
        key.as_str(),
        JewelFuncSpec {
            key,
            kind,
            args: smallvec::SmallVec::new(),
            r#type: Some(ty),
            radius: None,
        },
    )
}

/// The jewelFuncList entries with the table value each one wraps (Rust-only: built once).
static JEWEL_FUNC_LIST: LazyLock<Vec<(&'static str, JewelFuncSpec, &'static JewelFuncSpec)>> =
    LazyLock::new(|| {
        // L6552
        let mut jewelFuncList_t: Vec<(&'static str, JewelFuncSpec, &'static JewelFuncSpec)> =
            Vec::new();
        let mut set = |(k, spec): (&'static str, JewelFuncSpec), v: &'static JewelFuncSpec| {
            // `jewelFuncList[k:lower()] = …`: a repeated key replaces the earlier value in place
            if let Some(slot) = jewelFuncList_t.iter_mut().find(|(x, _, _)| *x == k) {
                *slot = (k, spec, v);
            } else {
                jewelFuncList_t.push((k, spec, v));
            }
        };
        // -- Jewels that modify nodes
        // L6554: for k, v in pairs(jewelOtherFuncs) do — LUA-ORDER: source order (no two keys
        // lowercase alike, so only the pattern loop at 6643 sees the order)
        for (k, v) in jewelOtherFuncs.iter() {
            // L6555-6570: { func = function(cap1, …, cap5) … end, type = "Other" } (the wrapper
            // body runs in pob_calc::modules::jewel_funcs::callJewelFunc)
            set(list_entry(k, JewelFuncKind::Other, n!("Other")), v);
        }
        // L6572
        for (k, v) in jewelSelfFuncs.iter() {
            // L6573
            set(list_entry(k, JewelFuncKind::Self_, n!("Self")), v);
        }
        // L6575
        for (k, v) in jewelSelfUnallocFuncs.iter() {
            // L6576
            set(
                list_entry(k, JewelFuncKind::SelfUnalloc, n!("SelfUnalloc")),
                v,
            );
        }
        // -- Threshold Jewels
        // L6579
        for (k, v) in jewelThresholdFuncs.iter() {
            // L6580: if type(v) == "table" then (the "$list" wrapper 6582-6589 runs in pob-calc)
            // -- Allow one threshold line to apply multiple independent threshold funcs
            // L6582 / L6591
            set(list_entry(k, JewelFuncKind::Threshold, n!("Threshold")), v);
        }
        jewelFuncList_t
    });

/// @pob Modules/ModParser.lua:6551-6593 h=76f86909
/// `jewelFuncList` (6551-6593): lowercased key → `{ func, type }`, as the spec the driver emits.
pub static jewelFuncList: LazyLock<Vec<(&'static str, JewelFuncSpec)>> = LazyLock::new(|| {
    JEWEL_FUNC_LIST
        .iter()
        .map(|(k, spec, _)| (*k, spec.clone()))
        .collect()
});

/// The table value `v` each jewelFuncList entry wraps (6554-6592), by lowercased key.
pub static jewelFuncListSource: LazyLock<ahash::AHashMap<&'static str, &'static JewelFuncSpec>> =
    LazyLock::new(|| JEWEL_FUNC_LIST.iter().map(|(k, _, v)| (*k, *v)).collect());

/// @pob Modules/ModParser.lua:6595-6607 h=6b94a241
/// `clusterJewelSkills` (6595-6607): lowercased enchant / notable / keystone line → mods (CCR-0014 §2).
pub static clusterJewelSkills: LazyLock<ahash::AHashMap<String, Vec<Mod>>> = LazyLock::new(|| {
    // -- Generate list of cluster jewel skills
    // L6596
    let mut clusterJewelSkills_t: ahash::AHashMap<String, Vec<Mod>> = ahash::AHashMap::new();
    // L6597: for baseName, jewel in pairs(data.clusterJewels.jewels) do — LUA-ORDER: a repeated
    // enchant line keeps the last writer; the dump's enchants are distinct
    for (_baseName, jewel) in &data().clusterJewels.jewels {
        // L6598
        for (skillId, skill) in &jewel.skills {
            let enchant: Vec<&str> = skill.enchant.iter().map(|s| &**s).collect();
            // L6599
            clusterJewelSkills_t.insert(
                lower(&enchant.join(" ")),
                vec![r#mod(
                    n!("JewelData"),
                    ModType::LIST,
                    vtab(vec![
                        (n!("key"), Value::from("clusterJewelSkill")),
                        (n!("value"), Value::from(*skillId)),
                    ]),
                    None,
                    0,
                    0,
                    &[],
                )],
            );
        }
    }
    // L6602
    for (notable, _) in &data().clusterJewels.notableSortOrder {
        // L6603
        clusterJewelSkills_t.insert(
            format!("1 added passive skill is {}", lower(notable.as_str())),
            vec![r#mod(
                n!("ClusterJewelNotable"),
                ModType::LIST,
                *notable,
                None,
                0,
                0,
                &[],
            )],
        );
    }
    // L6605
    for keystone in &data().clusterJewels.keystones {
        // L6606
        clusterJewelSkills_t.insert(
            format!("adds {}", lower(keystone.as_str())),
            vec![r#mod(
                n!("JewelData"),
                ModType::LIST,
                vtab(vec![
                    (n!("key"), Value::from("clusterJewelKeystone")),
                    (n!("value"), Value::from(*keystone)),
                ]),
                None,
                0,
                0,
                &[],
            )],
        );
    }
    clusterJewelSkills_t
});
