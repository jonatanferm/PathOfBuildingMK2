//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 913-1099: modFlagList: flags/keywordFlags/tags suffixes.
//!
//! Owner: MP2. The entries were translated by scripts/port/lua2rs (unit T1) and reviewed
//! against the Lua (lua2rs translated every entry).
//! One `PatternEntry` per Lua key, in source order (no key is duplicated in this range);
//! every value is a static table. `scan` runs over this list with `plain = true`.
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

/// @pob Modules/ModParser.lua:913-964 h=b3b676fe
fn modFlagList_915(v: &mut Vec<PatternEntry<ModSpec>>) {
    // Weapon types
    v.push(PatternEntry {
        pat: "with axes",
        val: ModSpec {
            flags: bor(ModFlag::Axe, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 915,
    });
    v.push(PatternEntry {
        pat: "to axe attacks",
        val: ModSpec {
            flags: bor(ModFlag::Axe, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 916,
    });
    v.push(PatternEntry {
        pat: "with axe attacks",
        val: ModSpec {
            flags: bor(ModFlag::Axe, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 917,
    });
    v.push(PatternEntry {
        pat: "with axes or swords",
        val: ModSpec {
            flags: ModFlag::Hit,
            tag: Some(tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Axe, ModFlag::Sword) })),
            ..Default::default()
        },
        lua_line: 918,
    });
    v.push(PatternEntry {
        pat: "with bows",
        val: ModSpec {
            flags: bor(ModFlag::Bow, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 919,
    });
    v.push(PatternEntry {
        pat: "to bow attacks",
        val: ModSpec {
            flags: bor(ModFlag::Bow, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 920,
    });
    v.push(PatternEntry {
        pat: "with bow attacks",
        val: ModSpec {
            flags: bor(ModFlag::Bow, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 921,
    });
    v.push(PatternEntry {
        pat: "with claws",
        val: ModSpec {
            flags: bor(ModFlag::Claw, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 922,
    });
    v.push(PatternEntry {
        pat: "with claws or daggers",
        val: ModSpec {
            flags: ModFlag::Hit,
            tag: Some(tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Claw, ModFlag::Dagger) })),
            ..Default::default()
        },
        lua_line: 923,
    });
    v.push(PatternEntry {
        pat: "to claw attacks",
        val: ModSpec {
            flags: bor(ModFlag::Claw, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 924,
    });
    v.push(PatternEntry {
        pat: "with claw attacks",
        val: ModSpec {
            flags: bor(ModFlag::Claw, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 925,
    });
    v.push(PatternEntry {
        pat: "claw attacks",
        val: ModSpec {
            flags: bor(ModFlag::Claw, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 926,
    });
    v.push(PatternEntry {
        pat: "dealt with claws",
        val: ModSpec {
            flags: bor(ModFlag::Claw, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 927,
    });
    v.push(PatternEntry {
        pat: "with daggers",
        val: ModSpec {
            flags: bor(ModFlag::Dagger, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 928,
    });
    v.push(PatternEntry {
        pat: "to dagger attacks",
        val: ModSpec {
            flags: bor(ModFlag::Dagger, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 929,
    });
    v.push(PatternEntry {
        pat: "with dagger attacks",
        val: ModSpec {
            flags: bor(ModFlag::Dagger, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 930,
    });
    v.push(PatternEntry {
        pat: "with maces",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 931,
    });
    v.push(PatternEntry {
        pat: "to mace attacks",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 932,
    });
    v.push(PatternEntry {
        pat: "with mace attacks",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 933,
    });
    v.push(PatternEntry {
        pat: "with maces and sceptres",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 934,
    });
    v.push(PatternEntry {
        pat: "with maces or sceptres",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 935,
    });
    v.push(PatternEntry {
        pat: "with maces, sceptres or staves",
        val: ModSpec {
            flags: ModFlag::Hit,
            tag: Some(tag!({ type = ModFlagOr, modFlags = bor(ModFlag::Mace, ModFlag::Staff) })),
            ..Default::default()
        },
        lua_line: 936,
    });
    v.push(PatternEntry {
        pat: "to mace and sceptre attacks",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 937,
    });
    v.push(PatternEntry {
        pat: "to mace or sceptre attacks",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 938,
    });
    v.push(PatternEntry {
        pat: "with mace or sceptre attacks",
        val: ModSpec {
            flags: bor(ModFlag::Mace, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 939,
    });
    v.push(PatternEntry {
        pat: "with staves",
        val: ModSpec {
            flags: bor(ModFlag::Staff, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 940,
    });
    v.push(PatternEntry {
        pat: "to staff attacks",
        val: ModSpec {
            flags: bor(ModFlag::Staff, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 941,
    });
    v.push(PatternEntry {
        pat: "with staff attacks",
        val: ModSpec {
            flags: bor(ModFlag::Staff, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 942,
    });
    v.push(PatternEntry {
        pat: "with swords",
        val: ModSpec {
            flags: bor(ModFlag::Sword, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 943,
    });
    v.push(PatternEntry {
        pat: "to sword attacks",
        val: ModSpec {
            flags: bor(ModFlag::Sword, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 944,
    });
    v.push(PatternEntry {
        pat: "with sword attacks",
        val: ModSpec {
            flags: bor(ModFlag::Sword, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 945,
    });
    v.push(PatternEntry {
        pat: "with wands",
        val: ModSpec {
            flags: bor(ModFlag::Wand, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 946,
    });
    v.push(PatternEntry {
        pat: "to wand attacks",
        val: ModSpec {
            flags: bor(ModFlag::Wand, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 947,
    });
    v.push(PatternEntry {
        pat: "with wand attacks",
        val: ModSpec {
            flags: bor(ModFlag::Wand, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 948,
    });
    v.push(PatternEntry {
        pat: "unarmed",
        val: ModSpec {
            flags: bor(ModFlag::Unarmed, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 949,
    });
    v.push(PatternEntry {
        pat: "unarmed melee",
        val: ModSpec {
            flags: bor(bor(ModFlag::Unarmed, ModFlag::Melee), ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 950,
    });
    v.push(PatternEntry {
        pat: "with unarmed attacks",
        val: ModSpec {
            flags: bor(ModFlag::Unarmed, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 951,
    });
    v.push(PatternEntry {
        pat: "with unarmed melee attacks",
        val: ModSpec {
            flags: bor(ModFlag::Unarmed, ModFlag::Melee),
            ..Default::default()
        },
        lua_line: 952,
    });
    v.push(PatternEntry {
        pat: "with unarmed melee attack",
        val: ModSpec {
            flags: bor(ModFlag::Unarmed, ModFlag::Melee),
            ..Default::default()
        },
        lua_line: 953,
    });
    v.push(PatternEntry {
        pat: "to unarmed attacks",
        val: ModSpec {
            flags: bor(ModFlag::Unarmed, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 954,
    });
    v.push(PatternEntry {
        pat: "to unarmed melee hits",
        val: ModSpec {
            flags: bor(bor(ModFlag::Unarmed, ModFlag::Melee), ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 955,
    });
    v.push(PatternEntry {
        pat: "with melee weapons",
        val: ModSpec {
            flags: ModFlag::WeaponMelee,
            ..Default::default()
        },
        lua_line: 956,
    });
    // Tincture mod doesn't care about it being a Hit
    v.push(PatternEntry {
        pat: "with melee weapon attacks",
        val: ModSpec {
            flags: ModFlag::WeaponMelee,
            ..Default::default()
        },
        lua_line: 957,
    });
    v.push(PatternEntry {
        pat: "from melee weapons",
        val: ModSpec {
            flags: ModFlag::WeaponMelee,
            ..Default::default()
        },
        lua_line: 958,
    });
    v.push(PatternEntry {
        pat: "from melee weapon attacks",
        val: ModSpec {
            flags: bor(ModFlag::WeaponMelee, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 959,
    });
    v.push(PatternEntry {
        pat: "melee weapon",
        val: ModSpec {
            flags: ModFlag::WeaponMelee,
            ..Default::default()
        },
        lua_line: 960,
    });
    v.push(PatternEntry {
        pat: "with one handed weapons",
        val: ModSpec {
            flags: bor(ModFlag::Weapon1H, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 961,
    });
    v.push(PatternEntry {
        pat: "with one handed melee weapons",
        val: ModSpec {
            flags: bor(bor(ModFlag::Weapon1H, ModFlag::WeaponMelee), ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 962,
    });
    v.push(PatternEntry {
        pat: "with two handed weapons",
        val: ModSpec {
            flags: bor(ModFlag::Weapon2H, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 963,
    });
    v.push(PatternEntry {
        pat: "with two handed melee weapons",
        val: ModSpec {
            flags: bor(bor(ModFlag::Weapon2H, ModFlag::WeaponMelee), ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 964,
    });
}

/// @pob Modules/ModParser.lua:965-1015 h=2a02e8d3
fn modFlagList_965(v: &mut Vec<PatternEntry<ModSpec>>) {
    v.push(PatternEntry {
        pat: "with ranged weapons",
        val: ModSpec {
            flags: bor(ModFlag::WeaponRanged, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 965,
    });
    // Skill types
    v.push(PatternEntry {
        pat: "spell",
        val: ModSpec {
            flags: ModFlag::Spell,
            ..Default::default()
        },
        lua_line: 967,
    });
    v.push(PatternEntry {
        pat: "for spells",
        val: ModSpec {
            flags: ModFlag::Spell,
            ..Default::default()
        },
        lua_line: 968,
    });
    v.push(PatternEntry {
        pat: "for spell damage",
        val: ModSpec {
            flags: ModFlag::Spell,
            ..Default::default()
        },
        lua_line: 969,
    });
    v.push(PatternEntry {
        pat: "with spell damage",
        val: ModSpec {
            flags: ModFlag::Spell,
            ..Default::default()
        },
        lua_line: 970,
    });
    v.push(PatternEntry {
        pat: "with spells",
        val: ModSpec {
            keywordFlags: KeywordFlag::Spell,
            ..Default::default()
        },
        lua_line: 971,
    });
    v.push(PatternEntry {
        pat: "with spell skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Spell,
            ..Default::default()
        },
        lua_line: 972,
    });
    v.push(PatternEntry {
        pat: "with triggered spells",
        val: ModSpec {
            keywordFlags: KeywordFlag::Spell,
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Triggered })),
            ..Default::default()
        },
        lua_line: 973,
    });
    v.push(PatternEntry {
        pat: "by spells",
        val: ModSpec {
            keywordFlags: KeywordFlag::Spell,
            ..Default::default()
        },
        lua_line: 974,
    });
    v.push(PatternEntry {
        pat: "by your spells",
        val: ModSpec {
            keywordFlags: KeywordFlag::Spell,
            ..Default::default()
        },
        lua_line: 975,
    });
    v.push(PatternEntry {
        pat: "with attacks",
        val: ModSpec {
            keywordFlags: KeywordFlag::Attack,
            ..Default::default()
        },
        lua_line: 976,
    });
    v.push(PatternEntry {
        pat: "by attacks",
        val: ModSpec {
            keywordFlags: KeywordFlag::Attack,
            ..Default::default()
        },
        lua_line: 977,
    });
    v.push(PatternEntry {
        pat: "by your attacks",
        val: ModSpec {
            keywordFlags: KeywordFlag::Attack,
            ..Default::default()
        },
        lua_line: 978,
    });
    v.push(PatternEntry {
        pat: "with attack skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Attack,
            ..Default::default()
        },
        lua_line: 979,
    });
    v.push(PatternEntry {
        pat: "for attacks",
        val: ModSpec {
            flags: ModFlag::Attack,
            ..Default::default()
        },
        lua_line: 980,
    });
    v.push(PatternEntry {
        pat: "for attack damage",
        val: ModSpec {
            flags: ModFlag::Attack,
            ..Default::default()
        },
        lua_line: 981,
    });
    v.push(PatternEntry {
        pat: "weapon",
        val: ModSpec {
            flags: ModFlag::Weapon,
            ..Default::default()
        },
        lua_line: 982,
    });
    v.push(PatternEntry {
        pat: "with weapons",
        val: ModSpec {
            flags: ModFlag::Weapon,
            ..Default::default()
        },
        lua_line: 983,
    });
    v.push(PatternEntry {
        pat: "melee",
        val: ModSpec {
            flags: ModFlag::Melee,
            ..Default::default()
        },
        lua_line: 984,
    });
    v.push(PatternEntry {
        pat: "with melee attacks",
        val: ModSpec {
            flags: ModFlag::Melee,
            ..Default::default()
        },
        lua_line: 985,
    });
    v.push(PatternEntry {
        pat: "with melee critical strikes",
        val: ModSpec {
            flags: ModFlag::Melee,
            tag: Some(tag!({ type = Condition, var = "CriticalStrike" })),
            ..Default::default()
        },
        lua_line: 986,
    });
    v.push(PatternEntry {
        pat: "with melee skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Melee })),
            ..Default::default()
        },
        lua_line: 987,
    });
    v.push(PatternEntry {
        pat: "with bow skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Bow,
            ..Default::default()
        },
        lua_line: 988,
    });
    v.push(PatternEntry {
        pat: "on melee hit",
        val: ModSpec {
            flags: bor(ModFlag::Melee, ModFlag::Hit),
            ..Default::default()
        },
        lua_line: 989,
    });
    v.push(PatternEntry {
        pat: "on hit",
        val: ModSpec {
            flags: ModFlag::Hit,
            ..Default::default()
        },
        lua_line: 990,
    });
    v.push(PatternEntry {
        pat: "with hits",
        val: ModSpec {
            keywordFlags: KeywordFlag::Hit,
            ..Default::default()
        },
        lua_line: 991,
    });
    v.push(PatternEntry {
        pat: "with hits against nearby enemies",
        val: ModSpec {
            keywordFlags: KeywordFlag::Hit,
            ..Default::default()
        },
        lua_line: 992,
    });
    v.push(PatternEntry {
        pat: "with hits and ailments",
        val: ModSpec {
            keywordFlags: bor(KeywordFlag::Hit, KeywordFlag::Ailment),
            ..Default::default()
        },
        lua_line: 993,
    });
    v.push(PatternEntry {
        pat: "with ailments",
        val: ModSpec {
            flags: ModFlag::Ailment,
            ..Default::default()
        },
        lua_line: 994,
    });
    v.push(PatternEntry {
        pat: "with ailments from attack skills",
        val: ModSpec {
            flags: ModFlag::Ailment,
            keywordFlags: KeywordFlag::Attack,
            ..Default::default()
        },
        lua_line: 995,
    });
    v.push(PatternEntry {
        pat: "with poison",
        val: ModSpec {
            keywordFlags: KeywordFlag::Poison,
            ..Default::default()
        },
        lua_line: 996,
    });
    v.push(PatternEntry {
        pat: "with bleeding",
        val: ModSpec {
            keywordFlags: KeywordFlag::Bleed,
            ..Default::default()
        },
        lua_line: 997,
    });
    v.push(PatternEntry {
        pat: "for ailments",
        val: ModSpec {
            flags: ModFlag::Ailment,
            ..Default::default()
        },
        lua_line: 998,
    });
    v.push(PatternEntry {
        pat: "for poison",
        val: ModSpec {
            keywordFlags: bor(KeywordFlag::Poison, KeywordFlag::MatchAll),
            ..Default::default()
        },
        lua_line: 999,
    });
    v.push(PatternEntry {
        pat: "for bleeding",
        val: ModSpec {
            keywordFlags: KeywordFlag::Bleed,
            ..Default::default()
        },
        lua_line: 1000,
    });
    v.push(PatternEntry {
        pat: "for ignite",
        val: ModSpec {
            keywordFlags: KeywordFlag::Ignite,
            ..Default::default()
        },
        lua_line: 1001,
    });
    v.push(PatternEntry {
        pat: "against damage over time",
        val: ModSpec {
            flags: ModFlag::Dot,
            ..Default::default()
        },
        lua_line: 1002,
    });
    v.push(PatternEntry {
        pat: "area",
        val: ModSpec {
            flags: ModFlag::Area,
            ..Default::default()
        },
        lua_line: 1003,
    });
    v.push(PatternEntry {
        pat: "mine",
        val: ModSpec {
            keywordFlags: KeywordFlag::Mine,
            ..Default::default()
        },
        lua_line: 1004,
    });
    v.push(PatternEntry {
        pat: "with mines",
        val: ModSpec {
            keywordFlags: KeywordFlag::Mine,
            ..Default::default()
        },
        lua_line: 1005,
    });
    v.push(PatternEntry {
        pat: "trap",
        val: ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        },
        lua_line: 1006,
    });
    v.push(PatternEntry {
        pat: "with traps",
        val: ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        },
        lua_line: 1007,
    });
    v.push(PatternEntry {
        pat: "for traps",
        val: ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        },
        lua_line: 1008,
    });
    v.push(PatternEntry {
        pat: "skills which throw traps have",
        val: ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        },
        lua_line: 1009,
    });
    v.push(PatternEntry {
        pat: "that place mines or throw traps",
        val: ModSpec {
            keywordFlags: bor(KeywordFlag::Mine, KeywordFlag::Trap),
            ..Default::default()
        },
        lua_line: 1010,
    });
    v.push(PatternEntry {
        pat: "that throw mines",
        val: ModSpec {
            keywordFlags: KeywordFlag::Mine,
            ..Default::default()
        },
        lua_line: 1011,
    });
    v.push(PatternEntry {
        pat: "that throw traps",
        val: ModSpec {
            keywordFlags: KeywordFlag::Trap,
            ..Default::default()
        },
        lua_line: 1012,
    });
    v.push(PatternEntry {
        pat: "brand",
        val: ModSpec {
            keywordFlags: KeywordFlag::Brand,
            ..Default::default()
        },
        lua_line: 1013,
    });
    v.push(PatternEntry {
        pat: "totem",
        val: ModSpec {
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        },
        lua_line: 1014,
    });
    v.push(PatternEntry {
        pat: "with totem skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        },
        lua_line: 1015,
    });
}

/// @pob Modules/ModParser.lua:1016-1065 h=2c988a3a
fn modFlagList_1016(v: &mut Vec<PatternEntry<ModSpec>>) {
    v.push(PatternEntry {
        pat: "for skills used by totems",
        val: ModSpec {
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        },
        lua_line: 1016,
    });
    v.push(PatternEntry {
        pat: "totem skills that cast an aura",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Aura })),
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        },
        lua_line: 1017,
    });
    v.push(PatternEntry {
        pat: "aura skills that summon totems",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Aura })),
            keywordFlags: KeywordFlag::Totem,
            ..Default::default()
        },
        lua_line: 1018,
    });
    v.push(PatternEntry {
        pat: "of aura skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Aura })),
            ..Default::default()
        },
        lua_line: 1019,
    });
    v.push(PatternEntry {
        pat: "curse skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Curse,
            ..Default::default()
        },
        lua_line: 1020,
    });
    v.push(PatternEntry {
        pat: "of curse skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Curse,
            ..Default::default()
        },
        lua_line: 1021,
    });
    v.push(PatternEntry {
        pat: "with curse skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Curse,
            ..Default::default()
        },
        lua_line: 1022,
    });
    v.push(PatternEntry {
        pat: "of curse aura skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Aura })),
            keywordFlags: KeywordFlag::Curse,
            ..Default::default()
        },
        lua_line: 1023,
    });
    v.push(PatternEntry {
        pat: "of curse auras",
        val: ModSpec {
            keywordFlags: bor(
                bor(KeywordFlag::Curse, KeywordFlag::Aura),
                KeywordFlag::MatchAll,
            ),
            ..Default::default()
        },
        lua_line: 1024,
    });
    v.push(PatternEntry {
        pat: "of hex skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Hex })),
            ..Default::default()
        },
        lua_line: 1025,
    });
    v.push(PatternEntry {
        pat: "with hex skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Hex })),
            ..Default::default()
        },
        lua_line: 1026,
    });
    v.push(PatternEntry {
        pat: "by retaliation skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Retaliation })),
            ..Default::default()
        },
        lua_line: 1027,
    });
    v.push(PatternEntry {
        pat: "of herald skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Herald })),
            ..Default::default()
        },
        lua_line: 1028,
    });
    v.push(PatternEntry {
        pat: "with herald skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Herald })),
            ..Default::default()
        },
        lua_line: 1029,
    });
    v.push(PatternEntry {
        pat: "with hits from herald skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Herald })),
            keywordFlags: KeywordFlag::Hit,
            ..Default::default()
        },
        lua_line: 1030,
    });
    v.push(PatternEntry {
        pat: "minion skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Minion })),
            ..Default::default()
        },
        lua_line: 1031,
    });
    v.push(PatternEntry {
        pat: "with minion skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Minion })),
            ..Default::default()
        },
        lua_line: 1032,
    });
    v.push(PatternEntry {
        pat: "of minion skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Minion })),
            ..Default::default()
        },
        lua_line: 1033,
    });
    v.push(PatternEntry {
        pat: "link skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Link })),
            ..Default::default()
        },
        lua_line: 1034,
    });
    v.push(PatternEntry {
        pat: "of link skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Link })),
            ..Default::default()
        },
        lua_line: 1035,
    });
    v.push(PatternEntry {
        pat: "for curses",
        val: ModSpec {
            keywordFlags: KeywordFlag::Curse,
            ..Default::default()
        },
        lua_line: 1036,
    });
    v.push(PatternEntry {
        pat: "for hexes",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Hex })),
            ..Default::default()
        },
        lua_line: 1037,
    });
    v.push(PatternEntry {
        pat: "warcry",
        val: ModSpec {
            keywordFlags: KeywordFlag::Warcry,
            ..Default::default()
        },
        lua_line: 1038,
    });
    v.push(PatternEntry {
        pat: "vaal",
        val: ModSpec {
            keywordFlags: KeywordFlag::Vaal,
            ..Default::default()
        },
        lua_line: 1039,
    });
    v.push(PatternEntry {
        pat: "vaal skill",
        val: ModSpec {
            keywordFlags: KeywordFlag::Vaal,
            ..Default::default()
        },
        lua_line: 1040,
    });
    v.push(PatternEntry {
        pat: "with vaal skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Vaal,
            ..Default::default()
        },
        lua_line: 1041,
    });
    v.push(PatternEntry {
        pat: "with non-vaal skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Vaal, neg = true })),
            ..Default::default()
        },
        lua_line: 1042,
    });
    v.push(PatternEntry {
        pat: "with movement skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Movement,
            ..Default::default()
        },
        lua_line: 1043,
    });
    v.push(PatternEntry {
        pat: "of movement skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Movement,
            ..Default::default()
        },
        lua_line: 1044,
    });
    v.push(PatternEntry {
        pat: "of movement skills used",
        val: ModSpec {
            keywordFlags: KeywordFlag::Movement,
            ..Default::default()
        },
        lua_line: 1045,
    });
    v.push(PatternEntry {
        pat: "of travel skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Travel })),
            ..Default::default()
        },
        lua_line: 1046,
    });
    v.push(PatternEntry {
        pat: "of banner skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Banner })),
            ..Default::default()
        },
        lua_line: 1047,
    });
    v.push(PatternEntry {
        pat: "lightning skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Lightning,
            ..Default::default()
        },
        lua_line: 1048,
    });
    v.push(PatternEntry {
        pat: "with lightning skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Lightning,
            ..Default::default()
        },
        lua_line: 1049,
    });
    v.push(PatternEntry {
        pat: "with cold skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Cold,
            ..Default::default()
        },
        lua_line: 1050,
    });
    v.push(PatternEntry {
        pat: "with fire skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Fire,
            ..Default::default()
        },
        lua_line: 1051,
    });
    v.push(PatternEntry {
        pat: "with elemental skills",
        val: ModSpec {
            keywordFlags: bor(
                bor(KeywordFlag::Lightning, KeywordFlag::Cold),
                KeywordFlag::Fire,
            ),
            ..Default::default()
        },
        lua_line: 1052,
    });
    v.push(PatternEntry {
        pat: "with chaos skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Chaos,
            ..Default::default()
        },
        lua_line: 1053,
    });
    v.push(PatternEntry {
        pat: "with physical skills",
        val: ModSpec {
            keywordFlags: KeywordFlag::Physical,
            ..Default::default()
        },
        lua_line: 1054,
    });
    v.push(PatternEntry {
        pat: "with channelling skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel })),
            ..Default::default()
        },
        lua_line: 1055,
    });
    v.push(PatternEntry {
        pat: "channelling",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel })),
            ..Default::default()
        },
        lua_line: 1056,
    });
    v.push(PatternEntry {
        pat: "channelling skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel })),
            ..Default::default()
        },
        lua_line: 1057,
    });
    v.push(PatternEntry {
        pat: "non-channelling",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel, neg = true })),
            ..Default::default()
        },
        lua_line: 1058,
    });
    v.push(PatternEntry {
        pat: "non-channelling skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Channel, neg = true })),
            ..Default::default()
        },
        lua_line: 1059,
    });
    v.push(PatternEntry {
        pat: "with brand skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Brand })),
            ..Default::default()
        },
        lua_line: 1060,
    });
    v.push(PatternEntry {
        pat: "for stance skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Stance })),
            ..Default::default()
        },
        lua_line: 1061,
    });
    v.push(PatternEntry {
        pat: "of stance skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Stance })),
            ..Default::default()
        },
        lua_line: 1062,
    });
    v.push(PatternEntry {
        pat: "mark skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Mark })),
            ..Default::default()
        },
        lua_line: 1063,
    });
    v.push(PatternEntry {
        pat: "of mark skills",
        val: ModSpec {
            tag: Some(tag!({ type = SkillType, skillType = SkillType::Mark })),
            ..Default::default()
        },
        lua_line: 1064,
    });
    v.push(PatternEntry {
        pat: "with skills that cost life",
        val: ModSpec {
            tag: Some(tag!({ type = StatThreshold, stat = "LifeCost", threshold = 1.0 })),
            ..Default::default()
        },
        lua_line: 1065,
    });
}

/// @pob Modules/ModParser.lua:1066-1099 h=0584f9fc
fn modFlagList_1066(v: &mut Vec<PatternEntry<ModSpec>>) {
    v.push(PatternEntry {
        pat: "minion",
        val: ModSpec {
            addToMinion: true,
            ..Default::default()
        },
        lua_line: 1066,
    });
    v.push(PatternEntry {
        pat: "zombie",
        val: ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true }),
            ),
            ..Default::default()
        },
        lua_line: 1067,
    });
    v.push(PatternEntry {
        pat: "raised zombie",
        val: ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Zombie", includeTransfigured = true }),
            ),
            ..Default::default()
        },
        lua_line: 1068,
    });
    v.push(PatternEntry { pat: "skeleton", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Skeletons", includeTransfigured = true })), ..Default::default() }, lua_line: 1069 });
    v.push(PatternEntry {
        pat: "spectre",
        val: ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true }),
            ),
            ..Default::default()
        },
        lua_line: 1070,
    });
    v.push(PatternEntry {
        pat: "raised spectre",
        val: ModSpec {
            addToMinion: true,
            addToMinionTag: Some(
                tag!({ type = SkillName, skillName = "Raise Spectre", includeTransfigured = true }),
            ),
            ..Default::default()
        },
        lua_line: 1071,
    });
    v.push(PatternEntry {
        pat: "golem",
        val: ModSpec {
            addToMinion: true,
            addToMinionTag: Some(tag!({ type = SkillType, skillType = SkillType::Golem })),
            ..Default::default()
        },
        lua_line: 1072,
    });
    v.push(PatternEntry { pat: "chaos golem", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Chaos Golem", includeTransfigured = true })), ..Default::default() }, lua_line: 1073 });
    v.push(PatternEntry { pat: "flame golem", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Flame Golem", includeTransfigured = true })), ..Default::default() }, lua_line: 1074 });
    v.push(PatternEntry { pat: "increased flame golem", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Flame Golem", includeTransfigured = true })), ..Default::default() }, lua_line: 1075 });
    v.push(PatternEntry { pat: "ice golem", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Ice Golem", includeTransfigured = true })), ..Default::default() }, lua_line: 1076 });
    v.push(PatternEntry { pat: "lightning golem", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Lightning Golem", includeTransfigured = true })), ..Default::default() }, lua_line: 1077 });
    v.push(PatternEntry { pat: "stone golem", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Summon Stone Golem", includeTransfigured = true })), ..Default::default() }, lua_line: 1078 });
    v.push(PatternEntry { pat: "animated guardian", val: ModSpec { addToMinion: true, addToMinionTag: Some(tag!({ type = SkillName, skillName = "Animate Guardian", includeTransfigured = true })), ..Default::default() }, lua_line: 1079 });
    // Damage types
    v.push(PatternEntry {
        pat: "with physical damage",
        val: ModSpec {
            tag: Some(tag!({ type = Condition, var = "PhysicalHasDamage" })),
            ..Default::default()
        },
        lua_line: 1081,
    });
    v.push(PatternEntry {
        pat: "with lightning damage",
        val: ModSpec {
            tag: Some(tag!({ type = Condition, var = "LightningHasDamage" })),
            ..Default::default()
        },
        lua_line: 1082,
    });
    v.push(PatternEntry {
        pat: "with cold damage",
        val: ModSpec {
            tag: Some(tag!({ type = Condition, var = "ColdHasDamage" })),
            ..Default::default()
        },
        lua_line: 1083,
    });
    v.push(PatternEntry {
        pat: "with fire damage",
        val: ModSpec {
            tag: Some(tag!({ type = Condition, var = "FireHasDamage" })),
            ..Default::default()
        },
        lua_line: 1084,
    });
    v.push(PatternEntry {
        pat: "with chaos damage",
        val: ModSpec {
            tag: Some(tag!({ type = Condition, var = "ChaosHasDamage" })),
            ..Default::default()
        },
        lua_line: 1085,
    });
    // Other
    v.push(PatternEntry {
        pat: "global",
        val: ModSpec {
            tag: Some(tag!({ type = Global })),
            ..Default::default()
        },
        lua_line: 1087,
    });
    v.push(PatternEntry {
        pat: "from equipped shield",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotName = "Weapon 2" })),
            ..Default::default()
        },
        lua_line: 1088,
    });
    v.push(PatternEntry { pat: "from equipped armour items", val: ModSpec { tag: Some(tag!({ type = SlotName, slotNameList = &[n!("Helmet"), n!("Body Armour"), n!("Gloves"), n!("Boots"), n!("Weapon 2")][..] })), ..Default::default() }, lua_line: 1089 });
    v.push(PatternEntry {
        pat: "from equipped helmet",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotName = "Helmet" })),
            ..Default::default()
        },
        lua_line: 1090,
    });
    v.push(PatternEntry {
        pat: "from equipped gloves and boots",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotNameList = &[n!("Gloves"), n!("Boots")][..] })),
            ..Default::default()
        },
        lua_line: 1091,
    });
    v.push(PatternEntry {
        pat: "from equipped boots and gloves",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotNameList = &[n!("Gloves"), n!("Boots")][..] })),
            ..Default::default()
        },
        lua_line: 1092,
    });
    v.push(PatternEntry {
        pat: "from equipped helmet and gloves",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotNameList = &[n!("Helmet"), n!("Gloves")][..] })),
            ..Default::default()
        },
        lua_line: 1093,
    });
    v.push(PatternEntry {
        pat: "from equipped helmet and boots",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotNameList = &[n!("Helmet"), n!("Boots")][..] })),
            ..Default::default()
        },
        lua_line: 1094,
    });
    v.push(PatternEntry {
        pat: "from your equipped body armour",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotName = "Body Armour" })),
            ..Default::default()
        },
        lua_line: 1095,
    });
    v.push(PatternEntry {
        pat: "from equipped body armour",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotName = "Body Armour" })),
            ..Default::default()
        },
        lua_line: 1096,
    });
    v.push(PatternEntry {
        pat: "from body armour",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotName = "Body Armour" })),
            ..Default::default()
        },
        lua_line: 1097,
    });
    v.push(PatternEntry {
        pat: "from your body armour",
        val: ModSpec {
            tag: Some(tag!({ type = SlotName, slotName = "Body Armour" })),
            ..Default::default()
        },
        lua_line: 1098,
    });
}

/// Built once in source order (calc doc §6).
pub static modFlagList: LazyLock<PatternList<ModSpec>> = LazyLock::new(|| {
    let mut v: Vec<PatternEntry<ModSpec>> = Vec::with_capacity(181);
    modFlagList_915(&mut v);
    modFlagList_965(&mut v);
    modFlagList_1016(&mut v);
    modFlagList_1066(&mut v);
    PatternList(v)
});
