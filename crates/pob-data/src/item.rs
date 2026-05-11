//! Item types — the user's equipped gear.
//!
//! Mirrors the structure of `Classes/Item.lua` in the upstream PoB repo, narrowed to
//! what we model in Phase 3b: name, rarity, base type reference, mod lines, sockets,
//! quality. Currency / crafting metadata is preserved verbatim where it appears in the
//! paste so we can round-trip it; the calc engine just reads `mod_lines`.

use ahash::HashSet;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "UPPERCASE")]
pub enum Rarity {
    #[default]
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
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum ModSection {
    Implicit,
    #[default]
    Explicit,
    Enchant,
    Crafted,
    Corrupted,
    Fractured,
    Veiled,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ModLine {
    pub line: String,
    pub section: ModSection,
    /// Issue #221: variant gate parsed from a `{variant:N,M,…}` prefix on the
    /// mod line. `None` means the line applies to every variant of the item
    /// (PoB's default for non-prefixed lines); `Some(list)` means it applies
    /// only when the active variant index is in `list` (1-based, matching
    /// PoB's `Selected Variant:` numbering).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant_list: Option<Vec<u32>>,
}

impl ModLine {
    /// Construct a mod line that applies to every variant — the common case
    /// for items without any `{variant:…}` prefixes. Slot for variant-gated
    /// lines is the public `variant_list` field on the returned struct.
    pub fn new(line: impl Into<String>, section: ModSection) -> Self {
        Self {
            line: line.into(),
            section,
            variant_list: None,
        }
    }

    /// Issue #221: does this mod line apply for the given 1-based active
    /// variant index? Returns `true` when the line has no variant gate
    /// (PoB's "applies to every variant" default) or when `active` matches
    /// one of the listed variant ids.
    ///
    /// `active = None` means "no variant picked" — in PoB this is treated as
    /// "show the first variant", so we accept any gated line that lists
    /// variant 1, plus every ungated line.
    #[must_use]
    pub fn applies_to_variant(&self, active: Option<u32>) -> bool {
        match &self.variant_list {
            None => true,
            Some(list) => list.contains(&active.unwrap_or(1)),
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
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
    /// Issue #221: ordered list of variant display names for uniques whose
    /// mod set rotates between league / map / sextant / passive choices
    /// (Watcher's Eye, Maven's Invitation, Volkuur's Guidance, Doryani's
    /// Catalyst, …). Empty means the item has no variants — the common
    /// case. PoB serialises one `Variant: <name>` line per entry.
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub variants: Vec<String>,
    /// Issue #221: 1-based active variant index, mirroring PoB's
    /// `Selected Variant:` line. `None` means no selection — treated as
    /// "variant 1" by [`ModLine::applies_to_variant`] so freshly-imported
    /// items render their first variant by default.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub variant: Option<u32>,
}

impl Item {
    /// Iterate every mod line, yielding `(section, text)`. Includes lines
    /// from every variant — callers that want the active-variant subset
    /// should use [`Self::iter_active_mod_lines`] instead.
    pub fn iter_mod_lines(&self) -> impl Iterator<Item = (&ModSection, &str)> {
        self.mod_lines.iter().map(|m| (&m.section, m.line.as_str()))
    }

    /// Issue #221: iterate only the mod lines that apply to the item's
    /// currently selected variant. Lines without a `{variant:…}` gate are
    /// always yielded; gated lines are yielded only when `self.variant`
    /// matches one of the listed variant ids. For items with no variants
    /// (`self.variants.is_empty()`) this iterator yields every line.
    pub fn iter_active_mod_lines(&self) -> impl Iterator<Item = &ModLine> {
        let active = self.variant;
        self.mod_lines
            .iter()
            .filter(move |m| m.applies_to_variant(active))
    }

    /// Issue #221: switch the active variant index, keeping `raw` in
    /// sync so a subsequent PoB-XML export reflects the new pick (the
    /// exporter embeds `raw` verbatim).
    ///
    /// `variant` is clamped to `1..=variants.len()`; passing a value
    /// outside that range, or calling this on an item without variants,
    /// is a no-op. The raw paste text is updated by rewriting (or
    /// inserting) the `Selected Variant:` line — we never reorder the
    /// rest of the document, so user formatting like comment blocks and
    /// custom whitespace survives untouched.
    pub fn set_active_variant(&mut self, variant: u32) {
        if self.variants.is_empty() {
            return;
        }
        let max = u32::try_from(self.variants.len()).unwrap_or(u32::MAX);
        let clamped = variant.clamp(1, max);
        if self.variant == Some(clamped) {
            return;
        }
        self.variant = Some(clamped);

        // Find an existing `Selected Variant:` line in the raw text and
        // rewrite its value. If the raw has none, insert after the last
        // `Variant:` line — PoB writes them adjacent and we want to
        // preserve that grouping. If even that's missing (item parsed
        // by hand from a fragment), no-op the raw rewrite; the
        // in-memory `self.variant` still updates so the calc engine
        // picks up the change.
        if self.raw.is_empty() {
            return;
        }
        let new_line = format!("Selected Variant: {clamped}");
        let mut rewrote = false;
        let updated: String = self
            .raw
            .lines()
            .map(|l| {
                if !rewrote && l.trim_start().starts_with("Selected Variant:") {
                    rewrote = true;
                    new_line.clone()
                } else {
                    l.to_owned()
                }
            })
            .collect::<Vec<_>>()
            .join("\n");
        let updated = if rewrote {
            updated
        } else {
            // Insert immediately after the last `Variant:` line.
            let lines: Vec<&str> = self.raw.lines().collect();
            let last_variant_idx = lines
                .iter()
                .rposition(|l| l.trim_start().starts_with("Variant:"));
            match last_variant_idx {
                Some(pos) => {
                    let mut out: Vec<String> = lines.iter().map(|s| (*s).to_owned()).collect();
                    out.insert(pos + 1, new_line);
                    out.join("\n")
                }
                None => self.raw.clone(),
            }
        };
        // Preserve a trailing newline if the original had one.
        let preserve_trailing_nl = self.raw.ends_with('\n');
        self.raw = if preserve_trailing_nl && !updated.ends_with('\n') {
            format!("{updated}\n")
        } else {
            updated
        };
    }

    /// Issue #221: replace any existing enchant mod lines on this
    /// item with the supplied set. The picker UI hands a flat
    /// `&[String]` of the chosen tier's mod text; this helper:
    ///
    /// 1. Drops every existing [`ModSection::Enchant`] entry from
    ///    `mod_lines` (so re-picking a different enchant doesn't
    ///    stack onto the previous one).
    /// 2. Pushes the new lines as `Enchant` mod entries (with no
    ///    variant gate — enchants don't vary).
    /// 3. Rewrites `raw` to drop old `<mod> (enchant)` lines and
    ///    append the new ones with the `(enchant)` suffix so a
    ///    PoB-XML round trip preserves them. The rest of the raw
    ///    text (name, base, item-level, requirements, explicits) is
    ///    untouched.
    ///
    /// Slot-agnostic: works for helmet, glove, boot — any slot that
    /// accepts an enchant. Passing an empty slice clears the
    /// existing enchant. No-op if the item has neither existing
    /// enchants nor new lines to add.
    pub fn apply_enchant(&mut self, lines: &[String]) {
        let had_old = self
            .mod_lines
            .iter()
            .any(|m| m.section == ModSection::Enchant);
        if !had_old && lines.is_empty() {
            return;
        }
        self.mod_lines.retain(|m| m.section != ModSection::Enchant);
        for line in lines {
            self.mod_lines.push(ModLine {
                line: line.clone(),
                section: ModSection::Enchant,
                variant_list: None,
            });
        }

        // Rewrite raw: drop existing enchant lines, append the new
        // ones with the `(enchant)` suffix so the parser
        // re-categorises them correctly on round-trip. Skip the
        // rewrite when `raw` is empty (synthetic items have no raw
        // body to preserve).
        if self.raw.is_empty() {
            return;
        }
        let preserve_trailing_nl = self.raw.ends_with('\n');
        let mut kept: Vec<String> = self
            .raw
            .lines()
            .filter(|l| !l.trim_end().ends_with(" (enchant)"))
            .map(str::to_owned)
            .collect();
        for line in lines {
            kept.push(format!("{line} (enchant)"));
        }
        let mut joined = kept.join("\n");
        if preserve_trailing_nl && !joined.ends_with('\n') {
            joined.push('\n');
        }
        self.raw = joined;
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
    /// Mutable accessor for the slot's item — used by the UI to swap a
    /// variant pick or other per-item state without round-tripping
    /// through `equip(unequip-then-equip)` (which would lose the slot's
    /// metadata if any).
    pub fn get_mut(&mut self, slot: Slot) -> Option<&mut Item> {
        self.items.get_mut(&slot)
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

#[cfg(test)]
mod variant_tests {
    use super::*;

    fn watchers_eye_raw() -> String {
        r"Rarity: UNIQUE
Watcher's Eye
Prismatic Jewel
Variant: Anger
Variant: Hatred
Selected Variant: 1
--------
Limited to: 1
--------
Item Level: 84
--------
+50 to maximum Mana
{variant:1}1% of Damage Leeched as Life while affected by Anger
{variant:2}10% increased Cold Damage while affected by Hatred
--------
"
        .to_owned()
    }

    fn mk_watchers_eye() -> Item {
        let raw = watchers_eye_raw();
        Item {
            name: "Watcher's Eye".into(),
            base_name: "Prismatic Jewel".into(),
            rarity: Rarity::Unique,
            item_level: 84,
            quality: 0,
            tags: HashSet::default(),
            mod_lines: vec![
                ModLine::new("+50 to maximum Mana", ModSection::Explicit),
                ModLine {
                    line: "1% of Damage Leeched as Life while affected by Anger".into(),
                    section: ModSection::Explicit,
                    variant_list: Some(vec![1]),
                },
                ModLine {
                    line: "10% increased Cold Damage while affected by Hatred".into(),
                    section: ModSection::Explicit,
                    variant_list: Some(vec![2]),
                },
            ],
            sockets: String::new(),
            raw,
            corrupted: false,
            mirrored: false,
            variants: vec!["Anger".into(), "Hatred".into()],
            variant: Some(1),
        }
    }

    #[test]
    fn applies_to_variant_treats_none_gate_as_universal() {
        let line = ModLine::new("+10 to maximum Life", ModSection::Explicit);
        assert!(line.applies_to_variant(None));
        assert!(line.applies_to_variant(Some(1)));
        assert!(line.applies_to_variant(Some(7)));
    }

    #[test]
    fn applies_to_variant_treats_none_active_as_variant_one() {
        // PoB shows the first variant when no `Selected Variant:` line
        // is present; mirror that for `iter_active_mod_lines` on items
        // we synthesised without an explicit pick.
        let gated_to_one = ModLine {
            line: "{stripped}".into(),
            section: ModSection::Explicit,
            variant_list: Some(vec![1]),
        };
        let gated_to_two = ModLine {
            line: "{stripped}".into(),
            section: ModSection::Explicit,
            variant_list: Some(vec![2]),
        };
        assert!(gated_to_one.applies_to_variant(None));
        assert!(!gated_to_two.applies_to_variant(None));
    }

    #[test]
    fn set_active_variant_clamps_and_updates_raw() {
        let mut item = mk_watchers_eye();
        item.set_active_variant(2);
        assert_eq!(item.variant, Some(2));
        assert!(item.raw.contains("Selected Variant: 2"));
        assert!(!item.raw.contains("Selected Variant: 1"));

        // Clamps an out-of-range pick.
        item.set_active_variant(99);
        assert_eq!(item.variant, Some(2));
        assert!(item.raw.contains("Selected Variant: 2"));

        // Clamps zero to 1.
        item.set_active_variant(0);
        assert_eq!(item.variant, Some(1));
        assert!(item.raw.contains("Selected Variant: 1"));
    }

    #[test]
    fn set_active_variant_inserts_line_when_missing_from_raw() {
        let mut item = mk_watchers_eye();
        // Strip the `Selected Variant:` line; the helper should re-add
        // it next to the existing `Variant:` block.
        item.raw = item
            .raw
            .lines()
            .filter(|l| !l.starts_with("Selected Variant:"))
            .collect::<Vec<_>>()
            .join("\n");
        item.set_active_variant(2);
        assert!(
            item.raw.contains("Selected Variant: 2"),
            "expected inserted line, got: {raw:?}",
            raw = item.raw,
        );
        // Inserted right after the last Variant: line.
        let lines: Vec<&str> = item.raw.lines().collect();
        let last_variant = lines
            .iter()
            .rposition(|l| l.starts_with("Variant:"))
            .unwrap();
        assert!(
            lines[last_variant + 1].starts_with("Selected Variant:"),
            "Selected Variant line should follow the variant block; got:\n{}",
            lines.join("\n")
        );
    }

    #[test]
    fn set_active_variant_noop_on_non_variant_item() {
        let mut item = Item {
            base_name: "Onyx Amulet".into(),
            raw: "Rarity: RARE\nFoo\nOnyx Amulet\n".into(),
            ..Item::default()
        };
        let before = item.raw.clone();
        item.set_active_variant(3);
        assert_eq!(item.variant, None);
        assert_eq!(item.raw, before);
    }

    #[test]
    fn iter_active_mod_lines_yields_unfiltered_for_no_variant_item() {
        let item = Item {
            mod_lines: vec![
                ModLine::new("+10 to Life", ModSection::Explicit),
                ModLine::new("+20 to Strength", ModSection::Explicit),
            ],
            ..Item::default()
        };
        let active: Vec<_> = item.iter_active_mod_lines().collect();
        assert_eq!(active.len(), 2);
    }
}

#[cfg(test)]
mod enchant_tests {
    use super::*;
    use crate::enchants::{HelmetEnchant, HelmetEnchantTier};

    fn mk_helmet() -> Item {
        let raw = "Rarity: RARE\nViper Bonnet\nIron Hat\n--------\nItem Level: 80\n--------\n+50 to maximum Life\n+30 to all Elemental Resistances\n";
        Item {
            name: "Viper Bonnet".into(),
            base_name: "Iron Hat".into(),
            rarity: Rarity::Rare,
            item_level: 80,
            quality: 0,
            tags: HashSet::default(),
            mod_lines: vec![
                ModLine::new("+50 to maximum Life", ModSection::Explicit),
                ModLine::new("+30 to all Elemental Resistances", ModSection::Explicit),
            ],
            sockets: String::new(),
            raw: raw.into(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    #[test]
    fn apply_enchant_appends_new_enchant_lines() {
        // Issue #221 (picker slice): a fresh enchant pick lands on
        // an unenchanted helmet by appending `(enchant)`-suffixed
        // lines to raw and pushing ModSection::Enchant entries to
        // mod_lines.
        let mut item = mk_helmet();
        let lines = vec![
            "20% increased Sentinel of Absolution Duration".to_owned(),
            "8% increased Absolution Cast Speed".to_owned(),
        ];
        item.apply_enchant(&lines);

        let enchants: Vec<_> = item
            .mod_lines
            .iter()
            .filter(|m| m.section == ModSection::Enchant)
            .collect();
        assert_eq!(enchants.len(), 2);
        assert!(enchants[0].line.contains("Sentinel of Absolution"));
        // raw text carries the suffixed lines so PoB-XML round trip
        // re-parses them as enchants.
        assert!(item
            .raw
            .contains("20% increased Sentinel of Absolution Duration (enchant)"));
        assert!(item
            .raw
            .contains("8% increased Absolution Cast Speed (enchant)"));
        // The original explicit lines survive untouched.
        assert!(item.raw.contains("+50 to maximum Life"));
        assert!(item.raw.contains("+30 to all Elemental Resistances"));
    }

    #[test]
    fn apply_enchant_replaces_existing_enchant_block() {
        // Re-picking a different enchant drops the previous lines
        // before adding the new ones — no stacking.
        let mut item = mk_helmet();
        item.apply_enchant(&["+10 to Strength".to_owned()]);
        assert!(item.raw.contains("+10 to Strength (enchant)"));

        item.apply_enchant(&["+20 to Intelligence".to_owned()]);
        assert!(item.raw.contains("+20 to Intelligence (enchant)"));
        assert!(!item.raw.contains("+10 to Strength (enchant)"));

        let enchants: Vec<_> = item
            .mod_lines
            .iter()
            .filter(|m| m.section == ModSection::Enchant)
            .collect();
        assert_eq!(enchants.len(), 1);
        assert_eq!(enchants[0].line, "+20 to Intelligence");
    }

    #[test]
    fn apply_enchant_clears_existing_when_empty_input() {
        // Passing an empty slice removes the existing enchant — used
        // by a "Remove enchant" affordance on the picker.
        let mut item = mk_helmet();
        item.apply_enchant(&["+10 to Strength".to_owned()]);
        assert!(item.raw.contains("+10 to Strength (enchant)"));

        item.apply_enchant(&[]);
        assert!(!item.raw.contains("(enchant)"));
        assert!(!item
            .mod_lines
            .iter()
            .any(|m| m.section == ModSection::Enchant));
    }

    #[test]
    fn apply_enchant_noop_on_unenchanted_item_with_empty_input() {
        // Belt-and-braces: an "apply empty enchant" on an already
        // empty-enchanted helmet leaves both raw and mod_lines
        // untouched.
        let mut item = mk_helmet();
        let before_raw = item.raw.clone();
        let before_lines = item.mod_lines.len();
        item.apply_enchant(&[]);
        assert_eq!(item.raw, before_raw);
        assert_eq!(item.mod_lines.len(), before_lines);
    }

    #[test]
    fn apply_enchant_preserves_trailing_newline_in_raw() {
        // The variant rewrite path preserves a trailing newline; mirror
        // that contract here so PoB-XML embed of the raw doesn't drop
        // it on the floor.
        let mut item = mk_helmet();
        assert!(item.raw.ends_with('\n'));
        item.apply_enchant(&["+10 to Strength".to_owned()]);
        assert!(item.raw.ends_with('\n'));
    }

    #[test]
    fn helmet_enchant_tier_lines_returns_correct_vector() {
        let enchant = HelmetEnchant {
            merciless: vec!["+1 Merciless".to_owned()],
            endgame: vec!["+2 Endgame".to_owned()],
        };
        assert_eq!(
            HelmetEnchantTier::Merciless.lines(&enchant),
            &["+1 Merciless".to_owned()][..],
        );
        assert_eq!(
            HelmetEnchantTier::Endgame.lines(&enchant),
            &["+2 Endgame".to_owned()][..],
        );
    }
}
