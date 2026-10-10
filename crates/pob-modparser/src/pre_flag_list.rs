//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 1102-1317: preFlagList: flags/tags at the start of a line (7 closures).
//!
//! Owner: MP2. The entries were translated by scripts/port/lua2rs (unit T1) and reviewed
//! against the Lua (lua2rs translated every entry).
//! One `PatternEntry` per Lua key, in source order (no key is duplicated in this range).
//! Closure parameters follow the preFlagList call shape `f(unpack(preFlagCap))`
//! (ModParser.lua:6680-6682; calc doc §6 / CCR-0006 §5): #1 = `caps.cap[0]`.
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

/// A string capture/local that Lua would index or concatenate: nil raises (calc doc §3.2).
#[inline]
fn ns<'a>(v: Option<&'a str>, what: &str) -> &'a str {
    match v {
        Some(x) => x,
        None => lua_error!("attempt to {} (a nil value)", what),
    }
}

/// @pob Modules/ModParser.lua:1102-1155 h=2112ae7f
fn preFlagList_1104(v: &mut Vec<PatternEntry<TagVal>>) {
    // Weapon types
    v.push(PatternEntry {
        pat: "^axe attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Axe,
            ..Default::default()
        }),
        lua_line: 1104,
    });
    v.push(PatternEntry {
        pat: "^axe or sword attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Axe, ModFlag::Sword) })),
            ..Default::default()
        }),
        lua_line: 1105,
    });
    v.push(PatternEntry {
        pat: "^bow attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Bow,
            ..Default::default()
        }),
        lua_line: 1106,
    });
    v.push(PatternEntry {
        pat: "^claw attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Claw,
            ..Default::default()
        }),
        lua_line: 1107,
    });
    v.push(PatternEntry {
        pat: "^claw or dagger attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Claw, ModFlag::Dagger) })),
            ..Default::default()
        }),
        lua_line: 1108,
    });
    v.push(PatternEntry {
        pat: "^dagger attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Dagger,
            ..Default::default()
        }),
        lua_line: 1109,
    });
    v.push(PatternEntry {
        pat: "^mace or sceptre attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Mace,
            ..Default::default()
        }),
        lua_line: 1110,
    });
    v.push(PatternEntry {
        pat: "^mace, sceptre or staff attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Mace, ModFlag::Staff) })),
            ..Default::default()
        }),
        lua_line: 1111,
    });
    v.push(PatternEntry {
        pat: "^staff attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Staff,
            ..Default::default()
        }),
        lua_line: 1112,
    });
    v.push(PatternEntry {
        pat: "^sword attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Sword,
            ..Default::default()
        }),
        lua_line: 1113,
    });
    v.push(PatternEntry {
        pat: "^wand attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Wand,
            ..Default::default()
        }),
        lua_line: 1114,
    });
    v.push(PatternEntry {
        pat: "^unarmed attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Unarmed,
            ..Default::default()
        }),
        lua_line: 1115,
    });
    v.push(PatternEntry {
        pat: "^attacks with one handed weapons [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Weapon1H,
            ..Default::default()
        }),
        lua_line: 1116,
    });
    v.push(PatternEntry {
        pat: "^attacks with two handed weapons [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Weapon2H,
            ..Default::default()
        }),
        lua_line: 1117,
    });
    v.push(PatternEntry {
        pat: "^attacks with melee weapons [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::WeaponMelee,
            ..Default::default()
        }),
        lua_line: 1118,
    });
    v.push(PatternEntry {
        pat: "^melee weapon hits [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: bor(ModFlag::WeaponMelee, ModFlag::Hit),
            ..Default::default()
        }),
        lua_line: 1119,
    });
    v.push(PatternEntry {
        pat: "^attacks with one handed melee weapons [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: bor(ModFlag::Weapon1H, ModFlag::WeaponMelee),
            ..Default::default()
        }),
        lua_line: 1120,
    });
    v.push(PatternEntry {
        pat: "^attacks with two handed melee weapons [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: bor(ModFlag::Weapon2H, ModFlag::WeaponMelee),
            ..Default::default()
        }),
        lua_line: 1121,
    });
    v.push(PatternEntry {
        pat: "^attacks with ranged weapons [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::WeaponRanged,
            ..Default::default()
        }),
        lua_line: 1122,
    });
    // Damage types
    v.push(PatternEntry {
        pat: "^attack damage ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Attack,
            ..Default::default()
        }),
        lua_line: 1124,
    });
    v.push(PatternEntry {
        pat: "^hits deal ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Hit,
            ..Default::default()
        }),
        lua_line: 1125,
    });
    v.push(PatternEntry {
        pat: "^melee weapon damage",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::WeaponMelee,
            ..Default::default()
        }),
        lua_line: 1126,
    });
    v.push(PatternEntry {
        pat: "^deal ",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1127,
    });
    v.push(PatternEntry {
        pat: "^critical strikes deal ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CriticalStrike" })),
            ..Default::default()
        }),
        lua_line: 1128,
    });
    v.push(PatternEntry {
        pat: "^poisons you inflict with critical strikes have ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: bor(KeywordFlag::Poison, KeywordFlag::MatchAll),
            tag: Some(tag!({ type = Condition, var = "CriticalStrike" })),
            ..Default::default()
        }),
        lua_line: 1129,
    });
    // Add to minion
    v.push(PatternEntry {
        pat: "^minions ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            ..Default::default()
        }),
        lua_line: 1131,
    });
    v.push(PatternEntry {
        pat: "^minions [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            ..Default::default()
        }),
        lua_line: 1132,
    });
    v.push(PatternEntry {
        pat: "^while a unique enemy is in your presence, minions [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            playerTag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })),
            ..Default::default()
        }),
        lua_line: 1133,
    });
    v.push(PatternEntry {
        pat: "^while a pinnacle atlas boss is in your presence, minions [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            playerTag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })),
            ..Default::default()
        }),
        lua_line: 1134,
    });
    v.push(PatternEntry {
        pat: "^minions leech ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            ..Default::default()
        }),
        lua_line: 1135,
    });
    v.push(PatternEntry {
        pat: "^minions' attacks deal ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            flags: ModFlag::Attack,
            ..Default::default()
        }),
        lua_line: 1136,
    });
    v.push(PatternEntry {
        pat: "^minions created recently have ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            playerTag: Some(tag!({ type = Condition, var = "MinionsCreatedRecently" })),
            ..Default::default()
        }),
        lua_line: 1137,
    });
    v.push(PatternEntry {
        pat: "^golems [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillType, skillType = SkillType::Golem })),
            ..Default::default()
        }),
        lua_line: 1138,
    });
    v.push(PatternEntry {
        pat: "^summoned golems [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillType, skillType = SkillType::Golem })),
            ..Default::default()
        }),
        lua_line: 1139,
    });
    v.push(PatternEntry {
        pat: "^golem skills have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Golem })),
            ..Default::default()
        }),
        lua_line: 1140,
    });
    v.push(PatternEntry {
        pat: "^non%-minion skills have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Minion, neg = true })),
            ..Default::default()
        }),
        lua_line: 1141,
    });
    v.push(PatternEntry {
        pat: "^zombies [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1142,
    });
    v.push(PatternEntry {
        pat: "^raised zombies [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1143,
    });
    v.push(PatternEntry { pat: "^skeletons [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Skeletons", includeTransfigured = true })), ..Default::default() }), lua_line: 1144 });
    v.push(PatternEntry { pat: "^raging spirits [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Raging Spirit", includeTransfigured = true })), ..Default::default() }), lua_line: 1145 });
    v.push(PatternEntry { pat: "^summoned raging spirits [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Raging Spirit", includeTransfigured = true })), ..Default::default() }), lua_line: 1146 });
    v.push(PatternEntry {
        pat: "^r?a?i?s?e?d? ?spectres [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1147,
    });
    v.push(PatternEntry { pat: "^chaos golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Chaos Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1148 });
    v.push(PatternEntry { pat: "^summoned chaos golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Chaos Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1149 });
    v.push(PatternEntry { pat: "^flame golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Flame Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1150 });
    v.push(PatternEntry { pat: "^summoned flame golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Flame Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1151 });
    v.push(PatternEntry { pat: "^ice golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Ice Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1152 });
    v.push(PatternEntry { pat: "^summoned ice golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Ice Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1153 });
    v.push(PatternEntry { pat: "^lightning golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Lightning Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1154 });
    v.push(PatternEntry { pat: "^summoned lightning golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Lightning Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1155 });
}

/// @pob Modules/ModParser.lua:1156-1208 h=d6d4f75b
fn preFlagList_1156(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry { pat: "^stone golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Stone Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1156 });
    v.push(PatternEntry { pat: "^summoned stone golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Stone Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1157 });
    v.push(PatternEntry { pat: "^summoned carrion golems [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Carrion Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1158 });
    v.push(PatternEntry { pat: "^summoned skitterbots [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Carrion Golem", includeTransfigured = true })), ..Default::default() }), lua_line: 1159 });
    v.push(PatternEntry {
        pat: "^blink arrow and blink arrow clones [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Blink Arrow", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1160,
    });
    v.push(PatternEntry {
        pat: "^mirror arrow and mirror arrow clones [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Mirror Arrow", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1161,
    });
    v.push(PatternEntry { pat: "^animated weapons [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Animate Weapon", includeTransfigured = true })), ..Default::default() }), lua_line: 1162 });
    v.push(PatternEntry { pat: "^animated guardians? deals? ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Animate Guardian", includeTransfigured = true })), ..Default::default() }), lua_line: 1163 });
    v.push(PatternEntry { pat: "^summoned holy relics [hd][ae][va][el] ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Holy Relic", includeTransfigured = true })), ..Default::default() }), lua_line: 1164 });
    v.push(PatternEntry {
        pat: "^summoned reaper [dh][ea][as]l?s? ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Summon Reaper", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1165,
    });
    v.push(PatternEntry {
        pat: "^summoned arbalists [hgdf][aei][vair][eln] ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Arbalists" })),
            ..Default::default()
        }),
        lua_line: 1166,
    });
    v.push(PatternEntry {
        pat: "^summoned arbalists' attacks have ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Arbalists" })),
            ..Default::default()
        }),
        lua_line: 1167,
    });
    v.push(PatternEntry {
        pat: "^herald skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Herald })),
            ..Default::default()
        }),
        lua_line: 1168,
    });
    v.push(PatternEntry {
        pat: "^agony crawler deals ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillName, skillName = "Herald of Agony" })),
            ..Default::default()
        }),
        lua_line: 1169,
    });
    v.push(PatternEntry {
        pat: "^summoned agony crawler fires ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillName, skillName = "Herald of Agony" })),
            ..Default::default()
        }),
        lua_line: 1170,
    });
    v.push(PatternEntry {
        pat: "^sentinels of purity deal ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillName, skillName = "Herald of Purity" })),
            ..Default::default()
        }),
        lua_line: 1171,
    });
    v.push(PatternEntry {
        pat: "^summoned sentinels of absolution have ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Absolution", includeTransfigured = true }),
            ),
            ..Default::default()
        }),
        lua_line: 1172,
    });
    v.push(PatternEntry { pat: "^summoned sentinels have ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillNameList = &[n!("Herald of Purity"), n!("Dominating Blow"), n!("Absolution")][..], includeTransfigured = true })), ..Default::default() }), lua_line: 1173 });
    v.push(PatternEntry {
        pat: "^raised zombies' slam attack has ",
        val: TagVal::Spec(ModSpec {
            addToMinion: true,
            tag: Some(tag!({ type = SkillId, skillId = "ZombieSlam" })),
            ..Default::default()
        }),
        lua_line: 1174,
    });
    v.push(PatternEntry { pat: "^raised spectres, raised zombies, and summoned skeletons have ", val: TagVal::Spec(ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillNameList = &[n!("Raise Spectre"), n!("Raise Zombie"), n!("Summon Skeletons")][..], includeTransfigured = true })), ..Default::default() }), lua_line: 1175 });
    // Totem/trap/mine
    v.push(PatternEntry {
        pat: "^attacks used by totems have ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Attack,
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        }),
        lua_line: 1177,
    });
    v.push(PatternEntry {
        pat: "^totems have ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        }),
        lua_line: 1178,
    });
    v.push(PatternEntry {
        pat: "^skills used by totems [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        }),
        lua_line: 1179,
    });
    v.push(PatternEntry {
        pat: "^spells cast by totems [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Spell,
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        }),
        lua_line: 1180,
    });
    v.push(PatternEntry {
        pat: "^trap and mine damage ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: bor(KeywordFlag::Trap, KeywordFlag::Mine),
            ..Default::default()
        }),
        lua_line: 1181,
    });
    v.push(PatternEntry {
        pat: "^skills used by traps [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        }),
        lua_line: 1182,
    });
    v.push(PatternEntry {
        pat: "^skills which throw traps [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        }),
        lua_line: 1183,
    });
    v.push(PatternEntry {
        pat: "^skills used by mines [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Mine,
            ..Default::default()
        }),
        lua_line: 1184,
    });
    v.push(PatternEntry {
        pat: "^skills which throw mines ([hgd]?[ae]?[via]?[enl]?[ ]?)",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Mine,
            ..Default::default()
        }),
        lua_line: 1185,
    });
    v.push(PatternEntry {
        pat: "^skills used by your traps and mines ([hgd]?[ae]?[via]?[enl]?[ ]?)",
        val: TagVal::Spec(ModSpec {
            keywordFlags: bor(KeywordFlag::Trap, KeywordFlag::Mine),
            ..Default::default()
        }),
        lua_line: 1186,
    });
    // Local damage
    v.push(PatternEntry {
        pat: "^attacks with this weapon ",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
            ..Default::default()
        }),
        lua_line: 1188,
    });
    v.push(PatternEntry {
        pat: "^attacks with this weapon [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
            ..Default::default()
        }),
        lua_line: 1189,
    });
    v.push(PatternEntry {
        pat: "^hits with this weapon [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Hit,
            tagList: vec![
                tag!({ type = Condition, var = "{Hand}Attack" }),
                tag!({ type = SkillType, skillType = SkillType::Attack }),
            ],
            ..Default::default()
        }),
        lua_line: 1190,
    });
    // Skill types
    v.push(PatternEntry {
        pat: "^attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Attack,
            ..Default::default()
        }),
        lua_line: 1192,
    });
    v.push(PatternEntry {
        pat: "^attack skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Attack,
            ..Default::default()
        }),
        lua_line: 1193,
    });
    v.push(PatternEntry {
        pat: "^spells [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Spell,
            ..Default::default()
        }),
        lua_line: 1194,
    });
    v.push(PatternEntry {
        pat: "^spell skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Spell })),
            ..Default::default()
        }),
        lua_line: 1195,
    });
    v.push(PatternEntry {
        pat: "^projectile attack skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::RangedAttack })),
            ..Default::default()
        }),
        lua_line: 1196,
    });
    v.push(PatternEntry {
        pat: "^projectiles from attacks [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::RangedAttack })),
            ..Default::default()
        }),
        lua_line: 1197,
    });
    v.push(PatternEntry {
        pat: "^arrows [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = KeywordFlagAnd, keywordFlags = KeywordFlag::Arrow })),
            ..Default::default()
        }),
        lua_line: 1198,
    });
    v.push(PatternEntry {
        pat: "^bow skills [hdf][aei][var][el] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Bow,
            ..Default::default()
        }),
        lua_line: 1199,
    });
    v.push(PatternEntry {
        pat: "^projectiles [hdf][aei][var][el] ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Projectile,
            ..Default::default()
        }),
        lua_line: 1200,
    });
    v.push(PatternEntry {
        pat: "^projectiles ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Projectile,
            ..Default::default()
        }),
        lua_line: 1201,
    });
    v.push(PatternEntry {
        pat: "^melee attacks have ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Melee,
            ..Default::default()
        }),
        lua_line: 1202,
    });
    v.push(PatternEntry {
        pat: "^movement attack skills have ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Attack,
            keywordFlags: KeywordFlag::Movement,
            ..Default::default()
        }),
        lua_line: 1203,
    });
    v.push(PatternEntry {
        pat: "^travel skills have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Travel })),
            ..Default::default()
        }),
        lua_line: 1204,
    });
    v.push(PatternEntry {
        pat: "^link skills have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Link })),
            ..Default::default()
        }),
        lua_line: 1205,
    });
    v.push(PatternEntry {
        pat: "^lightning skills [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Lightning,
            ..Default::default()
        }),
        lua_line: 1206,
    });
    v.push(PatternEntry {
        pat: "^lightning spells [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Lightning,
            flags: ModFlag::Spell,
            ..Default::default()
        }),
        lua_line: 1207,
    });
    v.push(PatternEntry {
        pat: "^cold skills [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Cold,
            ..Default::default()
        }),
        lua_line: 1208,
    });
}

/// @pob Modules/ModParser.lua:1209-1274 h=7a21dbb0
fn preFlagList_1209(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "^cold spells [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Cold,
            flags: ModFlag::Spell,
            ..Default::default()
        }),
        lua_line: 1209,
    });
    v.push(PatternEntry {
        pat: "^fire skills [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Fire,
            ..Default::default()
        }),
        lua_line: 1210,
    });
    v.push(PatternEntry {
        pat: "^fire spells [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Fire,
            flags: ModFlag::Spell,
            ..Default::default()
        }),
        lua_line: 1211,
    });
    v.push(PatternEntry {
        pat: "^chaos skills [hd][ae][va][el] a? ?",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Chaos,
            ..Default::default()
        }),
        lua_line: 1212,
    });
    v.push(PatternEntry {
        pat: "^vaal skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Vaal,
            ..Default::default()
        }),
        lua_line: 1213,
    });
    v.push(PatternEntry {
        pat: "^vaal skills require ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Vaal,
            ..Default::default()
        }),
        lua_line: 1214,
    });
    v.push(PatternEntry {
        pat: "^non%-aura vaal skills require ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Vaal,
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Aura, neg = true })),
            ..Default::default()
        }),
        lua_line: 1215,
    });
    v.push(PatternEntry {
        pat: "^brand skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Brand })),
            ..Default::default()
        }),
        lua_line: 1216,
    });
    v.push(PatternEntry {
        pat: "^channelling skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel })),
            ..Default::default()
        }),
        lua_line: 1217,
    });
    v.push(PatternEntry {
        pat: "^curse skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            keywordFlags: KeywordFlag::Curse,
            ..Default::default()
        }),
        lua_line: 1218,
    });
    v.push(PatternEntry {
        pat: "^hex skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Hex })),
            ..Default::default()
        }),
        lua_line: 1219,
    });
    v.push(PatternEntry {
        pat: "^mark skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Mark })),
            ..Default::default()
        }),
        lua_line: 1220,
    });
    v.push(PatternEntry {
        pat: "^melee skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Melee })),
            ..Default::default()
        }),
        lua_line: 1221,
    });
    v.push(PatternEntry {
        pat: "^guard skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Guard })),
            ..Default::default()
        }),
        lua_line: 1222,
    });
    v.push(PatternEntry {
        pat: "^banner skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Banner })),
            ..Default::default()
        }),
        lua_line: 1223,
    });
    v.push(PatternEntry {
        pat: "^nova spells [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Nova })),
            ..Default::default()
        }),
        lua_line: 1224,
    });
    v.push(PatternEntry {
        pat: "^area skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Area })),
            ..Default::default()
        }),
        lua_line: 1225,
    });
    v.push(PatternEntry {
        pat: "^aura skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Aura })),
            ..Default::default()
        }),
        lua_line: 1226,
    });
    v.push(PatternEntry {
        pat: "^prismatic skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::RandomElement })),
            ..Default::default()
        }),
        lua_line: 1227,
    });
    v.push(PatternEntry {
        pat: "^retaliation skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Retaliation })),
            ..Default::default()
        }),
        lua_line: 1228,
    });
    v.push(PatternEntry {
        pat: "^pact skills [hd][ae][va][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Pact })),
            ..Default::default()
        }),
        lua_line: 1229,
    });
    v.push(PatternEntry {
        pat: "^warcry skills have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Warcry })),
            ..Default::default()
        }),
        lua_line: 1230,
    });
    v.push(PatternEntry {
        pat: "^non%-curse aura skills have ",
        val: TagVal::Spec(ModSpec {
            tagList: vec![
                tag!({ type = SkillType, skillType = SkillType::Aura }),
                tag!({ type = SkillType, skillType = SkillType::AppliesCurse, neg = true }),
            ],
            ..Default::default()
        }),
        lua_line: 1231,
    });
    v.push(PatternEntry {
        pat: "^non%-channelling skills have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel, neg = true })),
            ..Default::default()
        }),
        lua_line: 1232,
    });
    v.push(PatternEntry {
        pat: "^non%-vaal skills deal ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Vaal, neg = true })),
            ..Default::default()
        }),
        lua_line: 1233,
    });
    v.push(PatternEntry {
        pat: "^skills [hgdf][aei][vari][eln] ",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1234,
    });
    // Slot specific
    v.push(PatternEntry {
        pat: "^left ring slot: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SlotNumber, num = 1.0 })),
            ..Default::default()
        }),
        lua_line: 1236,
    });
    v.push(PatternEntry {
        pat: "^right ring slot: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = SlotNumber, num = 2.0 })),
            ..Default::default()
        }),
        lua_line: 1237,
    });
    v.push(PatternEntry {
        pat: "^socketed gems ([hgd]?[ae]?[via]?[enl]?%s?)",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(tag!({ type = SocketedIn, slotName = "{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1238,
    });
    v.push(PatternEntry {
        pat: "^socketed skills [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(tag!({ type = SocketedIn, slotName = "{SlotName}" })),
            ..Default::default()
        }),
        lua_line: 1239,
    });
    v.push(PatternEntry {
        pat: "^socketed travel skills [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "travel" }),
            ),
            ..Default::default()
        }),
        lua_line: 1240,
    });
    v.push(PatternEntry {
        pat: "^socketed warcry skills [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "warcry" }),
            ),
            ..Default::default()
        }),
        lua_line: 1241,
    });
    v.push(PatternEntry {
        pat: "^socketed attacks [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "attack" }),
            ),
            ..Default::default()
        }),
        lua_line: 1242,
    });
    v.push(PatternEntry {
        pat: "^socketed spells [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "spell" }),
            ),
            ..Default::default()
        }),
        lua_line: 1243,
    });
    v.push(PatternEntry {
        pat: "^socketed curse gems [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "curse" }),
            ),
            ..Default::default()
        }),
        lua_line: 1244,
    });
    v.push(PatternEntry {
        pat: "^socketed melee gems [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "melee" }),
            ),
            ..Default::default()
        }),
        lua_line: 1245,
    });
    v.push(PatternEntry {
        pat: "^socketed golem gems [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "golem" }),
            ),
            ..Default::default()
        }),
        lua_line: 1246,
    });
    v.push(PatternEntry {
        pat: "^socketed golem skills [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "golem" }),
            ),
            ..Default::default()
        }),
        lua_line: 1247,
    });
    v.push(PatternEntry {
        pat: "^socketed golem skills have minions ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "golem" }),
            ),
            ..Default::default()
        }),
        lua_line: 1248,
    });
    v.push(PatternEntry {
        pat: "^socketed vaal skills [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(
                tag!({ type = SocketedIn, slotName = "{SlotName}", keyword = "vaal" }),
            ),
            ..Default::default()
        }),
        lua_line: 1249,
    });
    v.push(PatternEntry {
        pat: "^socketed projectile spells [hgdf][aei][viar][enl] ",
        val: TagVal::Spec(ModSpec {
            addToSkill: Some(tag!({ type = SocketedIn, slotName = "{SlotName}" })),
            tagList: vec![
                tag!({ type = SkillType, skillType = SkillType::Projectile }),
                tag!({ type = SkillType, skillType = SkillType::Spell }),
            ],
            ..Default::default()
        }),
        lua_line: 1250,
    });
    // Enemy modifiers
    v.push(PatternEntry {
        pat: "^enemies withered by you [th]a[vk]e ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = MultiplierThreshold, var = "WitheredStack", threshold = 1.0 })),
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1252,
    });
    v.push(PatternEntry { pat: "^enemies (%a+) by you take ", val: TagVal::Fn(|caps: &Caps| {
        let cond = ns(caps.cap[0], "index local 'cond'");
        ModSpec { tag: Some(tag!({ type = Condition, var = Name::from(&*gsub_fn(cond, "^%a", |c| Some(upper(c[0].as_str())), None).0) })), applyToEnemy: true, modSuffix: Some(n!("Taken")), ..Default::default() }
    }), lua_line: 1253 });
    v.push(PatternEntry { pat: "^enemies (%a+) by ", val: TagVal::Fn(|caps: &Caps| {
        let cond = ns(caps.cap[0], "index local 'cond'");
        ModSpec { tag: Some(tag!({ type = Condition, var = Name::from(&*gsub_fn(cond, "^%a", |c| Some(upper(c[0].as_str())), None).0) })), applyToEnemy: true, ..Default::default() }
    }), lua_line: 1256 });
    v.push(PatternEntry { pat: "^enemies (%a+) by you have ", val: TagVal::Fn(|caps: &Caps| {
        let cond = ns(caps.cap[0], "index local 'cond'");
        ModSpec { tag: Some(tag!({ type = Condition, var = Name::from(&*gsub_fn(cond, "^%a", |c| Some(upper(c[0].as_str())), None).0) })), applyToEnemy: true, ..Default::default() }
    }), lua_line: 1259 });
    v.push(PatternEntry {
        pat: "^while a pinnacle atlas boss is in your presence, enemies you've hit recently have ",
        val: TagVal::Fn(|caps: &Caps| ModSpec {
            playerTagList: vec![
                tag!({ type = Condition, var = "HitRecently" }),
                tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" }),
            ],
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1262,
    });
    v.push(PatternEntry {
        pat: "^while a unique enemy is in your presence, enemies you've hit recently have ",
        val: TagVal::Fn(|caps: &Caps| ModSpec {
            playerTagList: vec![
                tag!({ type = Condition, var = "HitRecently" }),
                tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" }),
            ],
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1265,
    });
    v.push(PatternEntry {
        pat: "^enemies you've hit recently have ",
        val: TagVal::Fn(|caps: &Caps| ModSpec {
            playerTag: Some(tag!({ type = Condition, var = "HitRecently" })),
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1268,
    });
    v.push(PatternEntry { pat: "^hits against enemies (%a+) by you have ", val: TagVal::Fn(|caps: &Caps| {
        let cond = ns(caps.cap[0], "index local 'cond'");
        ModSpec { tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = Name::from(&*gsub_fn(cond, "^%a", |c| Some(upper(c[0].as_str())), None).0) })), ..Default::default() }
    }), lua_line: 1271 });
    v.push(PatternEntry {
        pat: "^enemies shocked or frozen by you take ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, varList = &[n!("Shocked"), n!("Frozen")][..] })),
            applyToEnemy: true,
            modSuffix: Some(n!("Taken")),
            ..Default::default()
        }),
        lua_line: 1274,
    });
}

/// @pob Modules/ModParser.lua:1275-1317 h=79911465
fn preFlagList_1275(v: &mut Vec<PatternEntry<TagVal>>) {
    v.push(PatternEntry {
        pat: "^enemies affected by your spider's webs [thd][ae][avk][el] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(
                tag!({ type = MultiplierThreshold, var = "Spider's WebStack", threshold = 1.0 }),
            ),
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1275,
    });
    v.push(PatternEntry {
        pat: "^enemies you curse take ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Cursed" })),
            applyToEnemy: true,
            modSuffix: Some(n!("Taken")),
            ..Default::default()
        }),
        lua_line: 1276,
    });
    v.push(PatternEntry {
        pat: "^enemies you curse ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "Cursed" })),
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1277,
    });
    v.push(PatternEntry {
        pat: "^nearby enemies take ",
        val: TagVal::Spec(ModSpec {
            modSuffix: Some(n!("Taken")),
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1278,
    });
    v.push(PatternEntry {
        pat: "^nearby enemies have ",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1279,
    });
    v.push(PatternEntry {
        pat: "^nearby enemies deal ",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1280,
    });
    v.push(PatternEntry {
        pat: "^nearby enemies'? ",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1281,
    });
    v.push(PatternEntry {
        pat: "^nearby enemy monsters' ",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1282,
    });
    v.push(PatternEntry {
        pat: "against you",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            actorEnemy: true,
            ..Default::default()
        }),
        lua_line: 1283,
    });
    v.push(PatternEntry {
        pat: "^hits against you ",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            flags: ModFlag::Hit,
            ..Default::default()
        }),
        lua_line: 1284,
    });
    v.push(PatternEntry {
        pat: "^enemies near your totems deal ",
        val: TagVal::Spec(ModSpec {
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1285,
    });
    // Other
    v.push(PatternEntry {
        pat: "^your flasks grant ",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1287,
    });
    v.push(PatternEntry {
        pat: "^when hit, ",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1288,
    });
    v.push(PatternEntry {
        pat: "^you and allies [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            ..Default::default()
        }),
        lua_line: 1289,
    });
    v.push(PatternEntry {
        pat: "^auras from your skills grant ",
        val: TagVal::Spec(ModSpec {
            addToAura: true,
            ..Default::default()
        }),
        lua_line: 1290,
    });
    v.push(PatternEntry {
        pat: "^auras grant ",
        val: TagVal::Spec(ModSpec {
            addToAura: true,
            ..Default::default()
        }),
        lua_line: 1291,
    });
    v.push(PatternEntry {
        pat: "^banners also grant ",
        val: TagVal::Spec(ModSpec {
            addToAura: true,
            onlyAddToBanners: true,
            ..Default::default()
        }),
        lua_line: 1292,
    });
    v.push(PatternEntry {
        pat: "^you and nearby allies ",
        val: TagVal::Spec(ModSpec {
            newAura: true,
            ..Default::default()
        }),
        lua_line: 1293,
    });
    v.push(PatternEntry {
        pat: "^you and nearby allies [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            newAura: true,
            ..Default::default()
        }),
        lua_line: 1294,
    });
    v.push(PatternEntry {
        pat: "^nearby allies [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            newAura: true,
            newAuraOnlyAllies: true,
            ..Default::default()
        }),
        lua_line: 1295,
    });
    v.push(PatternEntry {
        pat: "^every rage also grants ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RageEffect" })),
            ..Default::default()
        }),
        lua_line: 1296,
    });
    v.push(PatternEntry {
        pat: "^each rage also grants ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "RageEffect" })),
            ..Default::default()
        }),
        lua_line: 1297,
    });
    v.push(PatternEntry {
        pat: "^you and allies affected by auras from your skills [hgd][ae][via][enl] ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "AffectedByAura" })),
            ..Default::default()
        }),
        lua_line: 1298,
    });
    v.push(PatternEntry {
        pat: "^take ",
        val: TagVal::Spec(ModSpec {
            modSuffix: Some(n!("Taken")),
            ..Default::default()
        }),
        lua_line: 1299,
    });
    v.push(PatternEntry {
        pat: "^marauder: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToMarauderStart" })),
            ..Default::default()
        }),
        lua_line: 1300,
    });
    v.push(PatternEntry {
        pat: "^duelist: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToDuelistStart" })),
            ..Default::default()
        }),
        lua_line: 1301,
    });
    v.push(PatternEntry {
        pat: "^ranger: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToRangerStart" })),
            ..Default::default()
        }),
        lua_line: 1302,
    });
    v.push(PatternEntry {
        pat: "^shadow: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToShadowStart" })),
            ..Default::default()
        }),
        lua_line: 1303,
    });
    v.push(PatternEntry {
        pat: "^witch: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToWitchStart" })),
            ..Default::default()
        }),
        lua_line: 1304,
    });
    v.push(PatternEntry {
        pat: "^templar: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToTemplarStart" })),
            ..Default::default()
        }),
        lua_line: 1305,
    });
    v.push(PatternEntry {
        pat: "^scion: ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "ConnectedToScionStart" })),
            ..Default::default()
        }),
        lua_line: 1306,
    });
    v.push(PatternEntry {
        pat: "^skills supported by spellslinger have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "SupportedBySpellslinger" })),
            ..Default::default()
        }),
        lua_line: 1307,
    });
    v.push(PatternEntry {
        pat: "^skills that have dealt a critical strike in the past 8 seconds deal ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "CritInPast8Sec" })),
            ..Default::default()
        }),
        lua_line: 1308,
    });
    v.push(PatternEntry { pat: "^blink arrow and mirror arrow have ", val: TagVal::Spec(ModSpec { tag: Some(tag!({ type = SkillName, skillNameList = &[n!("Blink Arrow"), n!("Mirror Arrow")][..], includeTransfigured = true })), ..Default::default() }), lua_line: 1309 });
    v.push(PatternEntry {
        pat: "attacks with energy blades ",
        val: TagVal::Spec(ModSpec {
            flags: ModFlag::Attack,
            tag: Some(tag!({ type = Condition, var = "AffectedByEnergyBlade" })),
            ..Default::default()
        }),
        lua_line: 1310,
    });
    v.push(PatternEntry {
        pat: "^for each nearby corpse, ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Multiplier, var = "NearbyCorpse" })),
            ..Default::default()
        }),
        lua_line: 1311,
    });
    v.push(PatternEntry {
        pat: "^enemies in your link beams have ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "BetweenYouAndLinkedTarget" })),
            applyToEnemy: true,
            ..Default::default()
        }),
        lua_line: 1312,
    });
    v.push(PatternEntry {
        pat: "^consecrated ground you create a?l?s?o? ?grants ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = Condition, var = "OnConsecratedGround" })),
            ..Default::default()
        }),
        lua_line: 1313,
    });
    // While in the presence of...
    v.push(PatternEntry {
        pat: "^while a unique enemy is in your presence, ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "RareOrUnique" })),
            ..Default::default()
        }),
        lua_line: 1315,
    });
    v.push(PatternEntry {
        pat: "^while a pinnacle atlas boss is in your presence, ",
        val: TagVal::Spec(ModSpec {
            tag: Some(tag!({ type = ActorCondition, actor = "enemy", var = "PinnacleBoss" })),
            ..Default::default()
        }),
        lua_line: 1316,
    });
}

/// Built once in source order (calc doc §6).
pub static preFlagList: LazyLock<PatternList<TagVal>> = LazyLock::new(|| {
    let mut v: Vec<PatternEntry<TagVal>> = Vec::with_capacity(190);
    preFlagList_1104(&mut v);
    preFlagList_1156(&mut v);
    preFlagList_1209(&mut v);
    preFlagList_1275(&mut v);
    PatternList(v)
});
