//! `ModFlag` / `KeywordFlag` bit values, verbatim from PoB's `Data/Global.lua`.

#![allow(non_upper_case_globals)]

pub mod mod_flag {
    // Damage modes
    pub const Attack: u32 = 0x0000_0001;
    pub const Spell: u32 = 0x0000_0002;
    pub const Hit: u32 = 0x0000_0004;
    pub const Dot: u32 = 0x0000_0008;
    pub const Cast: u32 = 0x0000_0010;
    // Damage sources
    pub const Melee: u32 = 0x0000_0100;
    pub const Area: u32 = 0x0000_0200;
    pub const Projectile: u32 = 0x0000_0400;
    pub const SourceMask: u32 = 0x0000_0600;
    pub const Ailment: u32 = 0x0000_0800;
    pub const MeleeHit: u32 = 0x0000_1000;
    pub const Weapon: u32 = 0x0000_2000;
    // Weapon types
    pub const Axe: u32 = 0x0001_0000;
    pub const Bow: u32 = 0x0002_0000;
    pub const Claw: u32 = 0x0004_0000;
    pub const Dagger: u32 = 0x0008_0000;
    pub const Mace: u32 = 0x0010_0000;
    pub const Staff: u32 = 0x0020_0000;
    pub const Sword: u32 = 0x0040_0000;
    pub const Wand: u32 = 0x0080_0000;
    pub const Unarmed: u32 = 0x0100_0000;
    pub const Fishing: u32 = 0x0200_0000;
    // Weapon classes
    pub const WeaponMelee: u32 = 0x0400_0000;
    pub const WeaponRanged: u32 = 0x0800_0000;
    pub const Weapon1H: u32 = 0x1000_0000;
    pub const Weapon2H: u32 = 0x2000_0000;
    pub const WeaponMask: u32 = 0x2FFF_0000;

    /// Name → value, for data files and the mod parser.
    pub const ALL: &[(&str, u32)] = &[
        ("Attack", Attack),
        ("Spell", Spell),
        ("Hit", Hit),
        ("Dot", Dot),
        ("Cast", Cast),
        ("Melee", Melee),
        ("Area", Area),
        ("Projectile", Projectile),
        ("SourceMask", SourceMask),
        ("Ailment", Ailment),
        ("MeleeHit", MeleeHit),
        ("Weapon", Weapon),
        ("Axe", Axe),
        ("Bow", Bow),
        ("Claw", Claw),
        ("Dagger", Dagger),
        ("Mace", Mace),
        ("Staff", Staff),
        ("Sword", Sword),
        ("Wand", Wand),
        ("Unarmed", Unarmed),
        ("Fishing", Fishing),
        ("WeaponMelee", WeaponMelee),
        ("WeaponRanged", WeaponRanged),
        ("Weapon1H", Weapon1H),
        ("Weapon2H", Weapon2H),
        ("WeaponMask", WeaponMask),
    ];

    #[must_use]
    pub fn by_name(name: &str) -> Option<u32> {
        ALL.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

pub mod keyword_flag {
    // Skill keywords
    pub const Aura: u32 = 0x0000_0001;
    pub const Curse: u32 = 0x0000_0002;
    pub const Warcry: u32 = 0x0000_0004;
    pub const Movement: u32 = 0x0000_0008;
    pub const Physical: u32 = 0x0000_0010;
    pub const Fire: u32 = 0x0000_0020;
    pub const Cold: u32 = 0x0000_0040;
    pub const Lightning: u32 = 0x0000_0080;
    pub const Chaos: u32 = 0x0000_0100;
    pub const Vaal: u32 = 0x0000_0200;
    pub const Bow: u32 = 0x0000_0400;
    pub const Arrow: u32 = 0x0000_0800;
    // Skill types
    pub const Trap: u32 = 0x0000_1000;
    pub const Mine: u32 = 0x0000_2000;
    pub const Totem: u32 = 0x0000_4000;
    pub const Minion: u32 = 0x0000_8000;
    pub const Attack: u32 = 0x0001_0000;
    pub const Spell: u32 = 0x0002_0000;
    pub const Hit: u32 = 0x0004_0000;
    pub const Ailment: u32 = 0x0008_0000;
    pub const Brand: u32 = 0x0010_0000;
    // Other effects
    pub const Poison: u32 = 0x0020_0000;
    pub const Bleed: u32 = 0x0040_0000;
    pub const Ignite: u32 = 0x0080_0000;
    // Damage over Time types
    pub const PhysicalDot: u32 = 0x0100_0000;
    pub const LightningDot: u32 = 0x0200_0000;
    pub const ColdDot: u32 = 0x0400_0000;
    pub const FireDot: u32 = 0x0800_0000;
    pub const ChaosDot: u32 = 0x1000_0000;
    /// Match *all* of the mod's keyword flags instead of any.
    pub const MatchAll: u32 = 0x4000_0000;

    pub const ALL: &[(&str, u32)] = &[
        ("Aura", Aura),
        ("Curse", Curse),
        ("Warcry", Warcry),
        ("Movement", Movement),
        ("Physical", Physical),
        ("Fire", Fire),
        ("Cold", Cold),
        ("Lightning", Lightning),
        ("Chaos", Chaos),
        ("Vaal", Vaal),
        ("Bow", Bow),
        ("Arrow", Arrow),
        ("Trap", Trap),
        ("Mine", Mine),
        ("Totem", Totem),
        ("Minion", Minion),
        ("Attack", Attack),
        ("Spell", Spell),
        ("Hit", Hit),
        ("Ailment", Ailment),
        ("Brand", Brand),
        ("Poison", Poison),
        ("Bleed", Bleed),
        ("Ignite", Ignite),
        ("PhysicalDot", PhysicalDot),
        ("LightningDot", LightningDot),
        ("ColdDot", ColdDot),
        ("FireDot", FireDot),
        ("ChaosDot", ChaosDot),
        ("MatchAll", MatchAll),
    ];

    #[must_use]
    pub fn by_name(name: &str) -> Option<u32> {
        ALL.iter().find(|(n, _)| *n == name).map(|(_, v)| *v)
    }
}

/// `MatchKeywordFlags` from `Data/Global.lua`: does a query's keyword flags
/// satisfy a mod's? Any-of by default, all-of when the mod has `MatchAll`.
#[must_use]
pub fn match_keyword_flags(keyword_flags: u32, mod_keyword_flags: u32) -> bool {
    let match_all = mod_keyword_flags & keyword_flag::MatchAll != 0;
    let mod_masked = mod_keyword_flags & !keyword_flag::MatchAll;
    let keyword_masked = keyword_flags & !keyword_flag::MatchAll;
    if match_all {
        keyword_masked & mod_masked == mod_masked
    } else {
        mod_masked == 0 || keyword_masked & mod_masked != 0
    }
}

#[cfg(test)]
mod tests {
    use super::keyword_flag as kw;
    use super::*;

    #[test]
    fn keyword_matching_any_and_all() {
        assert!(match_keyword_flags(0, 0));
        assert!(match_keyword_flags(kw::Fire, kw::Fire | kw::Cold));
        assert!(!match_keyword_flags(kw::Chaos, kw::Fire | kw::Cold));
        let all = kw::Fire | kw::Attack | kw::MatchAll;
        assert!(match_keyword_flags(kw::Fire | kw::Attack | kw::Hit, all));
        assert!(!match_keyword_flags(kw::Fire, all));
    }
}
