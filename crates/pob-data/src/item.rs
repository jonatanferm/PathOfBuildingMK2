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
