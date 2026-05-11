//! Item types — the user's equipped gear.
//!
//! Mirrors the structure of `Classes/Item.lua` in the upstream PoB repo, narrowed to
//! what we model in Phase 3b: name, rarity, base type reference, mod lines, sockets,
//! quality. Currency / crafting metadata is preserved verbatim where it appears in the
//! paste so we can round-trip it; the calc engine just reads `mod_lines`.

use ahash::HashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum Rarity {
    Normal,
    Magic,
    Rare,
    Unique,
    Relic,
}

impl Rarity {
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s.to_ascii_uppercase().as_str() {
            "NORMAL" => Self::Normal,
            "MAGIC" => Self::Magic,
            "RARE" => Self::Rare,
            "UNIQUE" => Self::Unique,
            "RELIC" => Self::Relic,
            _ => return None,
        })
    }
}

/// Equipment slot. Mirrors the keys PoB uses on its item-set.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Slot {
    Helmet,
    BodyArmour,
    Gloves,
    Boots,
    Amulet,
    Ring1,
    Ring2,
    Belt,
    Weapon1,
    Weapon2,
    /// Issue #109: swap-weapon set's main hand. The "X-key swap"
    /// pair PoB stores in `<Slot name="Weapon 1 Swap" …>` per
    /// ItemSet. Caster off-hand-buff stacking + brand swap-trap
    /// builds rely on this; the calc engine reads it when
    /// `ConfigState::use_second_weapon_set` is on.
    Weapon1Swap,
    /// Swap-weapon set's off-hand counterpart of `Weapon1Swap`.
    Weapon2Swap,
    Flask1,
    Flask2,
    Flask3,
    Flask4,
    Flask5,
}

impl Slot {
    pub fn all() -> &'static [Self] {
        &[
            Self::Helmet,
            Self::BodyArmour,
            Self::Gloves,
            Self::Boots,
            Self::Amulet,
            Self::Ring1,
            Self::Ring2,
            Self::Belt,
            Self::Weapon1,
            Self::Weapon2,
            Self::Weapon1Swap,
            Self::Weapon2Swap,
            Self::Flask1,
            Self::Flask2,
            Self::Flask3,
            Self::Flask4,
            Self::Flask5,
        ]
    }
    pub fn label(self) -> &'static str {
        match self {
            Self::Helmet => "Helmet",
            Self::BodyArmour => "Body Armour",
            Self::Gloves => "Gloves",
            Self::Boots => "Boots",
            Self::Amulet => "Amulet",
            Self::Ring1 => "Ring (1)",
            Self::Ring2 => "Ring (2)",
            Self::Belt => "Belt",
            Self::Weapon1 => "Weapon",
            Self::Weapon2 => "Off-hand",
            Self::Weapon1Swap => "Weapon (Swap)",
            Self::Weapon2Swap => "Off-hand (Swap)",
            Self::Flask1 => "Flask 1",
            Self::Flask2 => "Flask 2",
            Self::Flask3 => "Flask 3",
            Self::Flask4 => "Flask 4",
            Self::Flask5 => "Flask 5",
        }
    }
}

/// Where a mod line appeared in the item paste.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum ModSection {
    Implicit,
    Explicit,
    Enchant,
    Crafted,
    Corrupted,
    Fractured,
    Veiled,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ModLine {
    pub line: String,
    pub section: ModSection,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Item {
    pub name: String,
    pub base_name: String,
    pub rarity: Rarity,
    #[serde(default)]
    pub item_level: u32,
    #[serde(default)]
    pub quality: u32,
    /// Tags from the base — copied through here so the calc engine can match
    /// `Condition:UsingShield` etc. without re-resolving the base.
    #[serde(default)]
    pub tags: HashSet<String>,
    pub mod_lines: Vec<ModLine>,
    /// Sockets as a string of color letters: `R`, `G`, `B`, `W`. `-` = link, ` ` = unlinked.
    /// Mirrors PoB's `socketString`. Phase 3b stores it; calc effects come later.
    #[serde(default)]
    pub sockets: String,
    /// Raw paste text, kept for round-trip.
    #[serde(default)]
    pub raw: String,
    #[serde(default)]
    pub corrupted: bool,
    #[serde(default)]
    pub mirrored: bool,
}

impl Item {
    /// Iterate every mod line, yielding `(section, text)`.
    pub fn iter_mod_lines(&self) -> impl Iterator<Item = (&ModSection, &str)> {
        self.mod_lines.iter().map(|m| (&m.section, m.line.as_str()))
    }
}

/// Colour of a single socket. `White` accepts any gem colour.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum SocketColor {
    Red,
    Green,
    Blue,
    White,
    /// Abyss socket — only fits abyssal jewels.
    Abyss,
}

impl SocketColor {
    /// Single-letter PoB shorthand: `R` `G` `B` `W` `A`.
    /// Returns `None` for any other character.
    pub fn from_letter(c: char) -> Option<Self> {
        match c.to_ascii_uppercase() {
            'R' => Some(Self::Red),
            'G' => Some(Self::Green),
            'B' => Some(Self::Blue),
            'W' => Some(Self::White),
            'A' => Some(Self::Abyss),
            _ => None,
        }
    }

    pub fn letter(self) -> char {
        match self {
            Self::Red => 'R',
            Self::Green => 'G',
            Self::Blue => 'B',
            Self::White => 'W',
            Self::Abyss => 'A',
        }
    }

    /// Cycle to the next colour in `R -> G -> B -> W -> R`. Abyss sockets
    /// are fixed and never cycle (they're determined by the item base).
    pub fn cycle_next(self) -> Self {
        match self {
            Self::Red => Self::Green,
            Self::Green => Self::Blue,
            Self::Blue => Self::White,
            Self::White => Self::Red,
            Self::Abyss => Self::Abyss,
        }
    }
}

/// One linked group of sockets parsed from a PoB socket string.
///
/// PoB encodes sockets as colour letters joined by `-` for links inside a
/// group and ` ` (space) between groups: `"R-G-B G-W"` is two groups, the
/// first a 3-link of red/green/blue, the second a 2-link of green/white.
#[derive(Debug, Clone, PartialEq, Eq, Default)]
pub struct SocketGroup {
    pub colors: Vec<SocketColor>,
}

impl SocketGroup {
    pub fn len(&self) -> usize {
        self.colors.len()
    }
    pub fn is_empty(&self) -> bool {
        self.colors.is_empty()
    }
}

/// Parse a PoB socket string into linked groups.
///
/// The grammar is intentionally permissive — unknown letters are skipped,
/// repeated separators collapse, and an empty input returns an empty list.
/// This mirrors `Item:NormaliseRaritySockets` in the upstream PoB which is
/// equally forgiving when a paste comes in malformed.
///
/// Examples:
/// - `""` -> `[]`
/// - `"R"` -> one 1-link `[R]`
/// - `"R-G-B"` -> one 3-link `[R, G, B]`
/// - `"R-G-B G-W"` -> 3-link + 2-link
/// - `"A A"` -> two abyss 1-links (jewel sockets on a belt)
pub fn parse_socket_string(s: &str) -> Vec<SocketGroup> {
    let mut groups: Vec<SocketGroup> = Vec::new();
    let mut current = SocketGroup::default();
    for c in s.chars() {
        if let Some(color) = SocketColor::from_letter(c) {
            current.colors.push(color);
        } else if c == ' ' || c == ',' {
            // group break
            if !current.is_empty() {
                groups.push(std::mem::take(&mut current));
            }
        }
        // `-` is a link separator: a no-op since the next colour just
        // joins the current group. Any other character (digits, garbage)
        // is silently skipped.
    }
    if !current.is_empty() {
        groups.push(current);
    }
    groups
}

/// Re-emit a list of groups back to PoB's socket-string format.
/// Round-trips with `parse_socket_string` for any input it produced.
pub fn render_socket_groups(groups: &[SocketGroup]) -> String {
    let mut out = String::new();
    for (gi, group) in groups.iter().enumerate() {
        if gi > 0 {
            out.push(' ');
        }
        for (ci, color) in group.colors.iter().enumerate() {
            if ci > 0 {
                out.push('-');
            }
            out.push(color.letter());
        }
    }
    out
}

/// Map slot → equipped item. The simplest possible item-set.
#[derive(Debug, Default, Clone, Serialize, Deserialize)]
pub struct ItemSet {
    #[serde(default)]
    pub items: ahash::HashMap<Slot, Item>,
}

impl ItemSet {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn equip(&mut self, slot: Slot, item: Item) {
        self.items.insert(slot, item);
    }
    pub fn unequip(&mut self, slot: Slot) {
        self.items.remove(&slot);
    }
    pub fn get(&self, slot: Slot) -> Option<&Item> {
        self.items.get(&slot)
    }
    pub fn iter(&self) -> impl Iterator<Item = (&Slot, &Item)> {
        self.items.iter()
    }
}

#[cfg(test)]
mod sockets_tests {
    use super::*;

    fn group(colors: &[SocketColor]) -> SocketGroup {
        SocketGroup {
            colors: colors.to_vec(),
        }
    }

    #[test]
    fn empty_string_yields_no_groups() {
        assert!(parse_socket_string("").is_empty());
        assert!(parse_socket_string("   ").is_empty());
    }

    #[test]
    fn single_socket_is_one_group_one_color() {
        assert_eq!(parse_socket_string("R"), vec![group(&[SocketColor::Red])]);
    }

    #[test]
    fn linked_three_link_is_one_group() {
        assert_eq!(
            parse_socket_string("R-G-B"),
            vec![group(&[
                SocketColor::Red,
                SocketColor::Green,
                SocketColor::Blue
            ])]
        );
    }

    #[test]
    fn space_separates_linked_groups() {
        assert_eq!(
            parse_socket_string("R-G-B G-W"),
            vec![
                group(&[SocketColor::Red, SocketColor::Green, SocketColor::Blue]),
                group(&[SocketColor::Green, SocketColor::White]),
            ]
        );
    }

    #[test]
    fn lowercase_letters_accepted() {
        assert_eq!(
            parse_socket_string("r-g b"),
            vec![
                group(&[SocketColor::Red, SocketColor::Green]),
                group(&[SocketColor::Blue]),
            ]
        );
    }

    #[test]
    fn abyss_sockets_round_trip() {
        let s = "A A";
        let parsed = parse_socket_string(s);
        assert_eq!(
            parsed,
            vec![group(&[SocketColor::Abyss]), group(&[SocketColor::Abyss]),]
        );
        assert_eq!(render_socket_groups(&parsed), s);
    }

    #[test]
    fn unknown_characters_silently_skipped() {
        // Digits and stray punctuation should not panic or produce groups.
        assert_eq!(
            parse_socket_string("R 1 G"),
            vec![group(&[SocketColor::Red]), group(&[SocketColor::Green]),]
        );
    }

    #[test]
    fn render_round_trips_full_six_link() {
        let s = "R-G-B-R-G-B";
        let parsed = parse_socket_string(s);
        assert_eq!(parsed.len(), 1);
        assert_eq!(parsed[0].len(), 6);
        assert_eq!(render_socket_groups(&parsed), s);
    }

    #[test]
    fn cycle_next_walks_rgbw_then_wraps() {
        assert_eq!(SocketColor::Red.cycle_next(), SocketColor::Green);
        assert_eq!(SocketColor::Green.cycle_next(), SocketColor::Blue);
        assert_eq!(SocketColor::Blue.cycle_next(), SocketColor::White);
        assert_eq!(SocketColor::White.cycle_next(), SocketColor::Red);
    }

    #[test]
    fn cycle_next_holds_abyss_fixed() {
        // Abyss sockets are determined by the base — cycling shouldn't
        // accidentally turn an abyss socket into a colour-gem socket.
        assert_eq!(SocketColor::Abyss.cycle_next(), SocketColor::Abyss);
    }

    #[test]
    fn from_letter_rejects_non_socket_chars() {
        assert!(SocketColor::from_letter('-').is_none());
        assert!(SocketColor::from_letter(' ').is_none());
        assert!(SocketColor::from_letter('Q').is_none());
    }
}
