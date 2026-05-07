//! English-text → `Mod` parser. Phase 3 expansion of the Phase 2 minimal parser.
//!
//! Supports:
//! - `+N to <Stat>` / `-N to <Stat>` / `+N% to <Element> Resistance` / `+N% to all Elemental Resistances`
//! - `N% increased <Stat>` / `N% reduced <Stat>` (Inc, optionally negated)
//! - `N% more <Stat>` / `N% less <Stat>` (More, optionally negated)
//! - Damage variants on Inc with keyword flags applied: `Fire`, `Cold`, `Lightning`,
//!   `Chaos`, `Physical`, `Elemental`, `Projectile`, `Spell`, `Attack`, `Area`, `Bow`, `Sword`,
//!   `Mace`, `Axe`, `Claw`, `Dagger`, `Staff`, `Wand`, `Two Handed`, `One Handed`.
//! - `Regenerate N <Pool> per second` (Base) / `Regenerate N% of <Pool> per second` (Inc on regen-rate stat)
//! - Range mods (`+(20-30) to Strength`) — collapse to *min*. Phase 4 will keep both bounds.
//! - Trailing `with Ailments`, `to Spells`, `to Attacks` (sets keyword/mod flags).
//! - Leading `Minions deal` / `Minions have` (sets a "minion" namespace prefix on the
//!   stat name — calc layer routes minion mods to a different ModDB).
//!
//! Non-goals (still — this is a port, not a rewrite):
//! - Per-charge / per-stat trailing scalings (`per Power Charge`, `per 10 Strength`) —
//!   adds the value verbatim without the multiplier tag. The mod is still useful as a
//!   non-zero base; the *scaling* is wrong by a factor. Tracked in
//!   `docs/divergences.md` (will create when first divergence shows).
//! - Conditional clauses (`while at full life`, `if you've killed recently`).
//!
//! See PoB `Modules/ModParser.lua` for the canonical 6.7k-line implementation.

use pob_data::{KeywordFlag, ModFlag};

use crate::modifier::{Mod, ModType, ModValue};

#[derive(Debug, Clone)]
pub struct ParsedMod {
    pub mod_: Mod,
}

/// Parse a single PoB-style mod line. Returns `None` if no rule matched.
pub fn parse_mod_line(line: &str) -> Option<ParsedMod> {
    let line = line.trim();
    if line.is_empty() {
        return None;
    }

    // Pre-flag: "Minions deal", "Minions have"
    let (minion_prefix, rest) = if let Some(r) = line.strip_prefix("Minions deal ") {
        (true, r)
    } else if let Some(r) = line.strip_prefix("Minions have ") {
        (true, r)
    } else {
        (false, line)
    };

    let parsed = try_parse_to(rest)
        .or_else(|| try_parse_inc_reduced(rest))
        .or_else(|| try_parse_more_less(rest))
        .or_else(|| try_parse_regenerate(rest))
        .or_else(|| try_parse_adds_x_to_y(rest))?;

    let mut mod_ = parsed.mod_;
    if minion_prefix {
        // Phase 3a: route minion mods to a "Minion" namespace by prefixing the stat.
        // The calc layer can then use modDB.iter_named("Minion:Damage") for minion stats.
        // Phase 4: replace with proper minion ModDB.
        mod_.name = format!("Minion:{}", mod_.name);
    }
    Some(ParsedMod { mod_ })
}

fn try_parse_to(line: &str) -> Option<ParsedMod> {
    let (sign, rest) = if let Some(r) = line.strip_prefix('+') {
        (1.0, r)
    } else if let Some(r) = line.strip_prefix('-') {
        (-1.0, r)
    } else {
        return None;
    };
    let (n, rest) = consume_number(rest)?;
    let value = sign * n;

    let (is_percent, rest) = if let Some(r) = rest.strip_prefix('%') {
        (true, r)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("to ")?;
    let stat_text = rest.trim();

    if is_percent {
        // "maximum <Element> Resistance" — check first so "maximum Cold Resistance" doesn't
        // get parsed as a generic " Resistance" suffix.
        if let Some(elem_part) = stat_text.strip_prefix("maximum ") {
            if let Some(elem) = elem_part.strip_suffix(" Resistance") {
                return Some(ParsedMod {
                    mod_: Mod::base(format!("{elem}ResistMax"), value),
                });
            }
        }
        if let Some(elem) = stat_text.strip_suffix(" Resistance") {
            return Some(ParsedMod {
                mod_: Mod::base(format!("{elem}Resist"), value),
            });
        }
        if let Some(elem) = stat_text.strip_suffix(" Resistances") {
            if elem.contains("Elemental") || stat_text.starts_with("all Resistances") {
                return Some(ParsedMod {
                    mod_: Mod::base("ElementalResist", value),
                });
            }
        }
        // "to Critical Strike Multiplier" etc. — fall through to stat_name.
    }

    let stat = stat_name(stat_text)?;
    Some(ParsedMod {
        mod_: Mod::base(stat, value),
    })
}

fn try_parse_inc_reduced(line: &str) -> Option<ParsedMod> {
    let (n, rest) = consume_number(line)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    let (sign, rest) = if let Some(r) = rest.strip_prefix("increased ") {
        (1.0, r)
    } else if let Some(r) = rest.strip_prefix("reduced ") {
        (-1.0, r)
    } else {
        return None;
    };
    parse_stat_with_decorators(rest.trim(), ModType::Inc, sign * n)
}

fn try_parse_more_less(line: &str) -> Option<ParsedMod> {
    let (n, rest) = consume_number(line)?;
    let rest = rest.strip_prefix('%')?.trim_start();
    let (sign, rest) = if let Some(r) = rest.strip_prefix("more ") {
        (1.0, r)
    } else if let Some(r) = rest.strip_prefix("less ") {
        (-1.0, r)
    } else {
        return None;
    };
    parse_stat_with_decorators(rest.trim(), ModType::More, sign * n)
}

fn try_parse_regenerate(line: &str) -> Option<ParsedMod> {
    // "Regenerate N <Pool> per second"  → Base on <Pool>Regen
    // "Regenerate N% of <Pool> per second" → Base on <Pool>RegenPercent
    let rest = line.strip_prefix("Regenerate ")?;
    let (n, rest) = consume_number(rest)?;
    let (is_percent, rest) = if let Some(r) = rest.strip_prefix('%') {
        (true, r)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();
    let rest = if is_percent {
        rest.strip_prefix("of ")?
    } else {
        rest
    };
    // Strip the trailing "per second"
    let rest = rest.trim_end_matches(" per second");
    let pool = stat_name(rest.trim())?;
    let stat = if is_percent {
        format!("{pool}RegenPercent")
    } else {
        format!("{pool}Regen")
    };
    Some(ParsedMod {
        mod_: Mod::base(stat, n),
    })
}

fn try_parse_adds_x_to_y(line: &str) -> Option<ParsedMod> {
    // "Adds N to M <Element> Damage [to Attacks/Spells]"
    let rest = line.strip_prefix("Adds ")?;
    let (lo, rest) = consume_number(rest)?;
    let rest = rest.strip_prefix(" to ")?;
    let (hi, rest) = consume_number(rest)?;
    let rest = rest.trim_start_matches(' ');
    // Try "<Element> Damage [...]"
    let (stat, _flags, kw, mflags) = damage_with_decorators(rest)?;
    let mut m = Mod {
        name: stat,
        kind: ModType::Base,
        value: ModValue::Range { min: lo, max: hi },
        flags: mflags,
        keyword_flags: kw,
        source: None,
        tags: smallvec::SmallVec::new(),
    };
    m.flags |= ModFlag::empty();
    Some(ParsedMod { mod_: m })
}

/// Parse a stat phrase that may carry decorators after a base stat:
///
/// `<base_stat> [to Attacks|Spells] [with Ailments]` or compositions like
/// `Fire Damage`, `Projectile Damage`, `Two Handed Melee Damage`.
///
/// Returns a `Mod` with kind/value set, plus stat name + keyword/mod flags.
fn parse_stat_with_decorators(text: &str, kind: ModType, value: f64) -> Option<ParsedMod> {
    // Trailing decorators
    let mut text = text.to_string();
    let mut extra_flags = ModFlag::empty();
    let mut extra_keywords = KeywordFlag::empty();

    if let Some(stripped) = text.strip_suffix(" to Attacks") {
        extra_flags |= ModFlag::ATTACK;
        text = stripped.to_string();
    } else if let Some(stripped) = text.strip_suffix(" to Spells") {
        extra_flags |= ModFlag::SPELL;
        text = stripped.to_string();
    }
    if let Some(stripped) = text.strip_suffix(" with Ailments") {
        extra_keywords |= KeywordFlag::AILMENT;
        text = stripped.to_string();
    }
    if let Some(stripped) = text.strip_suffix(" with Bows") {
        extra_flags |= ModFlag::BOW;
        text = stripped.to_string();
    } else if let Some(stripped) = text.strip_suffix(" with Two Handed Weapons") {
        extra_flags |= ModFlag::WEAPON_2H;
        text = stripped.to_string();
    } else if let Some(stripped) = text.strip_suffix(" with One Handed Weapons") {
        extra_flags |= ModFlag::WEAPON_1H;
        text = stripped.to_string();
    }

    // Try damage decorators first (longest match): they may set flags/keyword_flags.
    if let Some((stat, flags, kw, mflags)) = damage_with_decorators(&text) {
        let mut m = match kind {
            ModType::Inc => Mod::inc(stat, value),
            ModType::More => Mod::more(stat, value),
            ModType::Base => Mod::base(stat, value),
            _ => Mod::inc(stat, value),
        };
        m.flags = flags | mflags | extra_flags;
        m.keyword_flags = kw | extra_keywords;
        return Some(ParsedMod { mod_: m });
    }

    let stat = stat_name(&text)?;
    let mut m = match kind {
        ModType::Inc => Mod::inc(stat, value),
        ModType::More => Mod::more(stat, value),
        ModType::Base => Mod::base(stat, value),
        _ => Mod::inc(stat, value),
    };
    m.flags |= extra_flags;
    m.keyword_flags |= extra_keywords;
    Some(ParsedMod { mod_: m })
}

/// Recognise damage-related stats with their flag/keyword decorators.
///
/// Returns (canonical stat name, **mod-required ModFlag**, KeywordFlag, **mandatory ModFlag**).
/// The third item is the keyword the *mod* carries (e.g. `Fire` for "Fire Damage").
fn damage_with_decorators(text: &str) -> Option<(String, ModFlag, KeywordFlag, ModFlag)> {
    // Strip trailing " Damage"
    let prefix = text.strip_suffix(" Damage")?;

    // Possible prefixes (longest first to disambiguate multi-word prefixes).
    // Ordered longest-first: "Two Handed Melee" must be checked before "Melee" so that
    // "Two Handed Melee Damage" doesn't lose its 2H qualifier.
    let table: &[(&str, KeywordFlag, ModFlag)] = &[
        ("Two Handed Melee", KeywordFlag::empty(), ModFlag::WEAPON_2H | ModFlag::MELEE),
        ("One Handed Melee", KeywordFlag::empty(), ModFlag::WEAPON_1H | ModFlag::MELEE),
        ("Two Handed", KeywordFlag::empty(), ModFlag::WEAPON_2H),
        ("One Handed", KeywordFlag::empty(), ModFlag::WEAPON_1H),
        ("Lightning", KeywordFlag::LIGHTNING, ModFlag::empty()),
        ("Elemental", KeywordFlag::empty(), ModFlag::empty()),
        ("Projectile", KeywordFlag::empty(), ModFlag::PROJECTILE),
        ("Physical", KeywordFlag::PHYSICAL, ModFlag::empty()),
        ("Dagger", KeywordFlag::empty(), ModFlag::DAGGER),
        ("Attack", KeywordFlag::empty(), ModFlag::ATTACK),
        ("Chaos", KeywordFlag::CHAOS, ModFlag::empty()),
        ("Spell", KeywordFlag::empty(), ModFlag::SPELL),
        ("Sword", KeywordFlag::empty(), ModFlag::SWORD),
        ("Staff", KeywordFlag::empty(), ModFlag::STAFF),
        ("Melee", KeywordFlag::empty(), ModFlag::MELEE),
        ("Claw", KeywordFlag::empty(), ModFlag::CLAW),
        ("Cold", KeywordFlag::COLD, ModFlag::empty()),
        ("Wand", KeywordFlag::empty(), ModFlag::WAND),
        ("Area", KeywordFlag::empty(), ModFlag::AREA),
        ("Mace", KeywordFlag::empty(), ModFlag::MACE),
        ("Fire", KeywordFlag::FIRE, ModFlag::empty()),
        ("Bow", KeywordFlag::empty(), ModFlag::BOW),
        ("Axe", KeywordFlag::empty(), ModFlag::AXE),
    ];

    let trimmed_prefix = prefix.trim();
    if trimmed_prefix.is_empty() {
        // Bare "Damage" — generic damage mod, no flags.
        return Some(("Damage".to_owned(), ModFlag::empty(), KeywordFlag::empty(), ModFlag::empty()));
    }

    for (label, kw, mf) in table {
        if let Some(_remainder) = trimmed_prefix.strip_suffix(*label) {
            // Compose the canonical stat name. Convention used internally:
            //   "FireDamage", "ProjectileDamage", "TwoHandedMeleeDamage", "Damage".
            let stat = if *label == "Elemental" {
                "ElementalDamage".to_owned()
            } else {
                format!("{}{}", label.replace(' ', ""), "Damage")
            };
            return Some((stat, ModFlag::empty(), *kw, *mf));
        }
    }
    None
}

fn consume_number(s: &str) -> Option<(f64, &str)> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('(') {
        let (a, rest) = consume_simple_number(rest)?;
        let rest = rest.strip_prefix('-')?;
        let (_b, rest) = consume_simple_number(rest)?;
        let rest = rest.strip_prefix(')')?;
        return Some((a, rest));
    }
    consume_simple_number(s)
}

fn consume_simple_number(s: &str) -> Option<(f64, &str)> {
    let mut end = 0;
    let bytes = s.as_bytes();
    while end < bytes.len() {
        let c = bytes[end];
        if c.is_ascii_digit() || c == b'.' {
            end += 1;
        } else {
            break;
        }
    }
    if end == 0 {
        return None;
    }
    let n = s[..end].parse::<f64>().ok()?;
    Some((n, &s[end..]))
}

/// Map English stat phrasing onto canonical stat names. The damage stats are *not* in
/// here — they're handled by [`damage_with_decorators`] which carries the keyword/mod
/// flag information.
pub fn stat_name(text: &str) -> Option<String> {
    let canon = match text {
        // Attributes
        "Strength" => "Strength",
        "Dexterity" => "Dexterity",
        "Intelligence" => "Intelligence",
        "all Attributes" => "AllAttributes",

        // Pools
        "maximum Life" => "Life",
        "Life" => "Life",
        "maximum Mana" => "Mana",
        "Mana" => "Mana",
        "maximum Energy Shield" => "EnergyShield",
        "Energy Shield" => "EnergyShield",
        "Ward" => "Ward",
        "Rage" => "Rage",
        "Spirit" => "Spirit",

        // Resists
        "Fire Resistance" => "FireResist",
        "Cold Resistance" => "ColdResist",
        "Lightning Resistance" => "LightningResist",
        "Chaos Resistance" => "ChaosResist",
        "Elemental Resistances" => "ElementalResist",
        "all Elemental Resistances" => "ElementalResist",

        // Defences
        "Armour" => "Armour",
        "Armour and Evasion" => "ArmourAndEvasion",
        "Armour, Evasion and Energy Shield" => "Defences",
        "Armour and Energy Shield" => "ArmourAndEnergyShield",
        "Evasion and Energy Shield" => "EvasionAndEnergyShield",
        "Evasion Rating" => "Evasion",
        "Evasion" => "Evasion",
        "Block Chance" => "BlockChance",
        "Spell Block Chance" => "SpellBlockChance",
        "Chance to Block" => "BlockChance",
        "Chance to Block Spell Damage" => "SpellBlockChance",
        "Chance to Suppress Spell Damage" => "SpellSuppressionChance",

        // Regen
        "Life Regeneration Rate" => "LifeRegen",
        "Mana Regeneration Rate" => "ManaRegen",
        "Energy Shield Recharge Rate" => "EnergyShieldRecharge",

        // Speeds
        "Attack Speed" => "AttackSpeed",
        "Cast Speed" => "CastSpeed",
        "Movement Speed" => "MovementSpeed",
        "Action Speed" => "ActionSpeed",
        "Attack and Cast Speed" => "AttackAndCastSpeed",

        // Crit / accuracy
        "Critical Strike Chance" => "CritChance",
        "Critical Strike Multiplier" => "CritMultiplier",
        "Accuracy Rating" => "Accuracy",
        "Accuracy" => "Accuracy",
        "Global Accuracy Rating" => "GlobalAccuracy",

        // Skill metrics
        "Area of Effect" => "AreaOfEffect",
        "Cooldown Recovery Rate" => "CooldownRecovery",
        "Skill Effect Duration" => "SkillEffectDuration",
        "Effect of Buffs on you" => "BuffEffectOnSelf",
        "Effect of non-Curse Auras from your Skills" => "AuraEffect",
        "Mana Reservation Efficiency" => "ManaReservationEfficiency",
        "Life Reservation Efficiency" => "LifeReservationEfficiency",
        "Reservation Efficiency" => "ReservationEfficiency",
        "Projectile Speed" => "ProjectileSpeed",

        // Drops / quantity
        "Rarity of Items found" => "ItemRarity",
        "Quantity of Items found" => "ItemQuantity",

        // Charges
        "Power Charges" => "PowerCharges",
        "Frenzy Charges" => "FrenzyCharges",
        "Endurance Charges" => "EnduranceCharges",
        "maximum Power Charges" => "PowerChargesMax",
        "maximum Frenzy Charges" => "FrenzyChargesMax",
        "maximum Endurance Charges" => "EnduranceChargesMax",

        // Misc
        "Stun Threshold" => "StunThreshold",
        "Stun Duration on Enemies" => "EnemyStunDuration",
        "Light Radius" => "LightRadius",
        "Damage" => "Damage",

        _ => return None,
    };
    Some(canon.to_owned())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn parse(line: &str) -> Mod {
        parse_mod_line(line).unwrap_or_else(|| panic!("failed: {line:?}")).mod_
    }

    #[test]
    fn elemental_damage() {
        let m = parse("10% increased Elemental Damage");
        assert_eq!(m.kind, ModType::Inc);
        assert_eq!(m.name, "ElementalDamage");
        assert_eq!(m.value.as_f64(), Some(10.0));
    }

    #[test]
    fn fire_damage_carries_keyword() {
        let m = parse("20% increased Fire Damage");
        assert_eq!(m.name, "FireDamage");
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
    }

    #[test]
    fn projectile_damage_carries_modflag() {
        let m = parse("15% increased Projectile Damage");
        assert_eq!(m.name, "ProjectileDamage");
        assert!(m.flags.contains(ModFlag::PROJECTILE));
    }

    #[test]
    fn two_handed_melee_damage() {
        let m = parse("12% increased Two Handed Melee Damage");
        assert_eq!(m.name, "TwoHandedMeleeDamage");
        assert!(m.flags.contains(ModFlag::WEAPON_2H));
        assert!(m.flags.contains(ModFlag::MELEE));
    }

    #[test]
    fn fire_damage_with_ailments() {
        let m = parse("10% increased Fire Damage with Ailments");
        assert_eq!(m.name, "FireDamage");
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
        assert!(m.keyword_flags.contains(KeywordFlag::AILMENT));
    }

    #[test]
    fn damage_to_attacks() {
        let m = parse("10% increased Fire Damage to Attacks");
        assert_eq!(m.name, "FireDamage");
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
        assert!(m.flags.contains(ModFlag::ATTACK));
    }

    #[test]
    fn mana_regeneration_rate() {
        let m = parse("20% increased Mana Regeneration Rate");
        assert_eq!(m.name, "ManaRegen");
    }

    #[test]
    fn area_of_effect() {
        let m = parse("8% increased Area of Effect");
        assert_eq!(m.name, "AreaOfEffect");
    }

    #[test]
    fn flat_life_regen() {
        let m = parse("Regenerate 2 Life per second");
        assert_eq!(m.name, "LifeRegen");
        assert_eq!(m.kind, ModType::Base);
        assert_eq!(m.value.as_f64(), Some(2.0));
    }

    #[test]
    fn percent_life_regen() {
        let m = parse("Regenerate 0.5% of Life per second");
        assert_eq!(m.name, "LifeRegenPercent");
        assert_eq!(m.value.as_f64(), Some(0.5));
    }

    #[test]
    fn minion_prefix() {
        let m = parse("Minions deal 10% increased Damage");
        assert_eq!(m.name, "Minion:Damage");
    }

    #[test]
    fn adds_fire_damage_range() {
        let m = parse("Adds 10 to 20 Fire Damage");
        assert_eq!(m.name, "FireDamage");
        assert!(matches!(m.value, ModValue::Range { min: 10.0, max: 20.0 }));
        assert!(m.keyword_flags.contains(KeywordFlag::FIRE));
    }

    #[test]
    fn maximum_resist() {
        let m = parse("+1% to maximum Cold Resistance");
        assert_eq!(m.name, "ColdResistMax");
    }

    #[test]
    fn empty_line_returns_none() {
        assert!(parse_mod_line("").is_none());
    }

    #[test]
    fn unknown_returns_none() {
        assert!(parse_mod_line("This is not a real mod line").is_none());
    }
}
