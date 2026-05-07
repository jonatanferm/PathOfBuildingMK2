//! Tiny English-text → `Mod` parser. Phase 2 covers a deliberately narrow slice of the
//! 6.7k-line `Modules/ModParser.lua`:
//!
//! - `+N to <Stat>` (Base)
//! - `-N to <Stat>` (Base)
//! - `N% increased <Stat>` (Inc)
//! - `N% reduced <Stat>` (Inc, negated)
//! - `N% more <Stat>` (More)
//! - `N% less <Stat>` (More, negated)
//! - `+N% to <Element> Resistance` (Base on `<Element>Resist`)
//!
//! Range mods (`+(20-30) to Strength`) collapse to their *minimum* — Phase 3 will surface
//! both bounds. Conditional / per-multiplier suffixes are not yet parsed.
//!
//! Stat names are mapped via [`stat_name`]: a small lookup table that covers the stats
//! Phase 2's perform pass touches.
//!
//! Returning `Vec` here (not `SmallVec`) because callers always own the resulting mods.

use crate::modifier::{Mod, ModType, ModValue};

/// Result of parsing one line. The line might emit zero (unparseable) or one mod.
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

    // 1. "+N to <stat>" / "+N% to <stat>" / "-N to <stat>"
    if let Some(p) = try_parse_to(line) {
        return Some(p);
    }
    // 2. "N% increased <stat>" / "N% reduced <stat>"
    if let Some(p) = try_parse_inc_reduced(line) {
        return Some(p);
    }
    // 3. "N% more <stat>" / "N% less <stat>"
    if let Some(p) = try_parse_more_less(line) {
        return Some(p);
    }
    None
}

fn try_parse_to(line: &str) -> Option<ParsedMod> {
    // Strip optional leading sign — produces (signed value, rest)
    let (sign, rest) = if let Some(r) = line.strip_prefix('+') {
        (1.0, r)
    } else if let Some(r) = line.strip_prefix('-') {
        (-1.0, r)
    } else {
        return None;
    };
    let (n, rest) = consume_number(rest)?;
    let value = sign * n;

    // Optional `%` for resistances etc.
    let (is_percent, rest) = if let Some(r) = rest.strip_prefix('%') {
        (true, r)
    } else {
        (false, rest)
    };
    let rest = rest.trim_start();
    let rest = rest.strip_prefix("to ")?;
    let stat_text = rest.trim();

    // Resistance specialisation: "to Fire Resistance"
    if is_percent {
        if let Some(elem) = stat_text.strip_suffix(" Resistance") {
            let stat = format!("{elem}Resist");
            return Some(ParsedMod {
                mod_: Mod {
                    name: stat,
                    kind: ModType::Base,
                    value: ModValue::Number(value),
                    ..Mod::base("", 0.0)
                },
            });
        }
        if stat_text == "all Elemental Resistances" {
            // PoB encodes this as three separate base mods. We emit one Mod with name
            // "ElementalResist" — the calc layer expands it.
            return Some(ParsedMod {
                mod_: Mod::base("ElementalResist", value),
            });
        }
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
    let stat = stat_name(rest.trim())?;
    Some(ParsedMod {
        mod_: Mod::inc(stat, sign * n),
    })
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
    let stat = stat_name(rest.trim())?;
    Some(ParsedMod {
        mod_: Mod::more(stat, sign * n),
    })
}

/// Consume a numeric token at the start of `s`. Supports plain integers, decimals, and
/// `(min-max)` ranges (collapsed to `min` for Phase 2). Returns `(value, remainder)`.
fn consume_number(s: &str) -> Option<(f64, &str)> {
    let s = s.trim_start();
    if let Some(rest) = s.strip_prefix('(') {
        // (min-max)
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

/// Map English stat phrasing onto the canonical stat names used by the calc engine.
/// Returns `None` for unrecognised names — callers should treat unparsed lines as
/// "we'll model this later".
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

        // Resists ("+x% to Fire Resistance" handled in try_parse_to; bare Resistance
        // here is for "increased Fire Resistance" and similar.)
        "Fire Resistance" => "FireResist",
        "Cold Resistance" => "ColdResist",
        "Lightning Resistance" => "LightningResist",
        "Chaos Resistance" => "ChaosResist",

        // Defences (incs)
        "Armour" => "Armour",
        "Evasion Rating" => "Evasion",
        "Evasion" => "Evasion",
        "Block Chance" => "BlockChance",

        // Damages
        "Physical Damage" => "PhysicalDamage",
        "Fire Damage" => "FireDamage",
        "Cold Damage" => "ColdDamage",
        "Lightning Damage" => "LightningDamage",
        "Chaos Damage" => "ChaosDamage",
        "Damage" => "Damage",

        // Speeds
        "Attack Speed" => "AttackSpeed",
        "Cast Speed" => "CastSpeed",
        "Movement Speed" => "MovementSpeed",
        "Action Speed" => "ActionSpeed",

        // Other common
        "Critical Strike Chance" => "CritChance",
        "Critical Strike Multiplier" => "CritMultiplier",
        "Accuracy Rating" => "Accuracy",
        "Accuracy" => "Accuracy",
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
    fn plus_to_strength() {
        let m = parse("+10 to Strength");
        assert_eq!(m.kind, ModType::Base);
        assert_eq!(m.name, "Strength");
        assert_eq!(m.value.as_f64(), Some(10.0));
    }

    #[test]
    fn negative_to_strength() {
        let m = parse("-5 to Strength");
        assert_eq!(m.value.as_f64(), Some(-5.0));
    }

    #[test]
    fn percent_to_fire_resistance() {
        let m = parse("+25% to Fire Resistance");
        assert_eq!(m.kind, ModType::Base);
        assert_eq!(m.name, "FireResist");
        assert_eq!(m.value.as_f64(), Some(25.0));
    }

    #[test]
    fn increased_life() {
        let m = parse("8% increased maximum Life");
        assert_eq!(m.kind, ModType::Inc);
        assert_eq!(m.name, "Life");
        assert_eq!(m.value.as_f64(), Some(8.0));
    }

    #[test]
    fn reduced_mana() {
        let m = parse("10% reduced Mana");
        assert_eq!(m.kind, ModType::Inc);
        assert_eq!(m.value.as_f64(), Some(-10.0));
    }

    #[test]
    fn more_damage() {
        let m = parse("40% more Damage");
        assert_eq!(m.kind, ModType::More);
        assert_eq!(m.name, "Damage");
        assert_eq!(m.value.as_f64(), Some(40.0));
    }

    #[test]
    fn less_attack_speed() {
        let m = parse("15% less Attack Speed");
        assert_eq!(m.kind, ModType::More);
        assert_eq!(m.value.as_f64(), Some(-15.0));
    }

    #[test]
    fn range_collapses_to_min() {
        // Real PoB lines look like "+(20-30) to Strength".
        let m = parse("+(20-30) to Strength");
        assert_eq!(m.value.as_f64(), Some(20.0));
    }

    #[test]
    fn unknown_stat_returns_none() {
        assert!(parse_mod_line("+10 to MagicalSparkleDamage").is_none());
    }

    #[test]
    fn empty_line_returns_none() {
        assert!(parse_mod_line("").is_none());
        assert!(parse_mod_line("   ").is_none());
    }

    #[test]
    fn realistic_passive_stats() {
        // From 3_25 tree: random sampled passive nodes that should parse.
        for line in [
            "+10 to Strength",
            "+10 to Dexterity",
            "+10 to Intelligence",
            "+5 to all Attributes",
            "8% increased maximum Life",
            "+12% to Fire Resistance",
            "10% increased Physical Damage",
            "15% increased Critical Strike Chance",
        ] {
            assert!(parse_mod_line(line).is_some(), "should parse: {line:?}");
        }
    }
}
