//! Parse an item from PoB / PoE copy-paste text.
//!
//! Mirrors `Classes/Item.lua:298+` (`ParseRaw`). The PoE format separates sections with
//! `--------` lines:
//!
//! ```text
//! Item Class: Amulets
//! Rarity: RARE
//! Soul Charm
//! Onyx Amulet
//! --------
//! Quality: +20% (augmented)
//! --------
//! Requirements:
//! Level: 70
//! --------
//! Item Level: 84
//! --------
//! +10 to all Attributes
//! --------
//! +62 to maximum Life
//! +39% to all Elemental Resistances
//! 20% increased Light Radius
//! --------
//! ```
//!
//! The first section after `Rarity:` carries name + base. Subsequent sections that are
//! made up of mod lines are: implicits (first such section), then explicits.
//! The `(implicit)`/`(crafted)`/`(enchant)`/`(corrupted)` suffix on a line overrides the
//! section classification for that line.
//!
//! Phase 3b is intentionally permissive — unrecognised metadata lines (`Sockets:`,
//! `Talisman Tier:`, `Allocates …`) are skipped without error. We surface mod lines that
//! parse with the existing ModParser into an item ModList; the rest are kept as text on
//! the Item but not turned into mods.

use indexmap::IndexSet;
use pob_data::{Item, ItemSet, ModLine, ModSection, Rarity, Slot};

#[derive(Debug)]
pub enum ParseError {
    Empty,
    MissingRarity,
}

impl std::fmt::Display for ParseError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Empty => write!(f, "empty item paste"),
            Self::MissingRarity => write!(f, "item paste missing `Rarity:` line"),
        }
    }
}

impl std::error::Error for ParseError {}

pub fn parse_item(raw: &str) -> Result<Item, ParseError> {
    let raw_lines: Vec<&str> = raw
        .lines()
        .map(str::trim_end)
        .filter(|l| !l.is_empty())
        .collect();
    if raw_lines.is_empty() {
        return Err(ParseError::Empty);
    }

    // Group into sections separated by lines of dashes.
    let sections: Vec<&[&str]> = split_sections(&raw_lines);
    if sections.is_empty() {
        return Err(ParseError::Empty);
    }

    let mut rarity = Rarity::Rare;
    let mut name = String::new();
    let mut base_name = String::new();
    let mut quality = 0u32;
    let mut item_level = 0u32;
    let mut corrupted = false;
    let mut mirrored = false;
    let mut sockets = String::new();

    // Walk the first section: "Item Class: ...", "Rarity: ...", name(s).
    let first = sections[0];
    let mut i = 0usize;
    if first.get(i).is_some_and(|l| l.starts_with("Item Class:")) {
        i += 1;
    }
    let rarity_line = first.get(i).copied().ok_or(ParseError::MissingRarity)?;
    let rarity_word = rarity_line
        .strip_prefix("Rarity:")
        .ok_or(ParseError::MissingRarity)?
        .trim();
    rarity = Rarity::parse(rarity_word).unwrap_or(rarity);
    i += 1;
    // For Rare/Unique/Relic the next two lines are name + base. For Normal/Magic the
    // single line carries the base + (magic) prefix/suffix wrapping.
    if matches!(rarity, Rarity::Rare | Rarity::Unique | Rarity::Relic) {
        if let Some(n) = first.get(i) {
            name = (*n).to_owned();
            i += 1;
        }
        if let Some(b) = first.get(i) {
            base_name = (*b).to_owned();
        }
    } else if let Some(n) = first.get(i) {
        // Magic / Normal: leave name = "" and use the line as the base.
        base_name = strip_magic_affixes(n);
        name.clear();
    }

    // Sweep remaining sections for metadata + mod lines.
    let mut explicit_section_started = false;
    let mut mod_lines: Vec<ModLine> = Vec::new();
    for section in sections.iter().skip(1) {
        let section_is_mod_section = looks_like_mod_section(section);
        // If any line in this mod section has an explicit (implicit) / (crafted) / …
        // suffix, the rest of the section defaults to explicit. Otherwise the first
        // mod section is treated as all-implicit.
        let any_self_tagged = section
            .iter()
            .any(|l| strip_mod_suffix(l).1.is_some());
        for &line in *section {
            if let Some(rest) = line.strip_prefix("Quality:") {
                quality = parse_first_int(rest).unwrap_or(0);
                continue;
            }
            if let Some(rest) = line.strip_prefix("Item Level:") {
                item_level = parse_first_int(rest).unwrap_or(0);
                continue;
            }
            if let Some(rest) = line.strip_prefix("Sockets:") {
                sockets = rest.trim().to_owned();
                continue;
            }
            if line == "Corrupted" {
                corrupted = true;
                continue;
            }
            if line == "Mirrored" {
                mirrored = true;
                continue;
            }
            if line == "Unidentified" || line == "Split" || line == "Foiled" {
                continue;
            }
            if line.starts_with("Requirements")
                || line.starts_with("Level:")
                || line.starts_with("Str:")
                || line.starts_with("Dex:")
                || line.starts_with("Int:")
                || line.starts_with("Talisman Tier:")
                || line.starts_with("League:")
                || line.starts_with("Note:")
            {
                continue;
            }

            if !section_is_mod_section {
                continue;
            }

            let (clean, suffix_section) = strip_mod_suffix(line);
            let section_kind = suffix_section.unwrap_or_else(|| {
                if any_self_tagged {
                    ModSection::Explicit
                } else if !explicit_section_started {
                    ModSection::Implicit
                } else {
                    ModSection::Explicit
                }
            });
            mod_lines.push(ModLine {
                line: clean.to_owned(),
                section: section_kind,
            });
        }
        if section_is_mod_section {
            // The first mod section is implicits; subsequent ones are explicits unless a
            // `(implicit)` suffix says otherwise.
            explicit_section_started = true;
        }
    }

    Ok(Item {
        name,
        base_name,
        rarity,
        item_level,
        quality,
        tags: IndexSet::<String>::new().into_iter().collect(),
        mod_lines,
        sockets,
        raw: raw.to_owned(),
        corrupted,
        mirrored,
    })
}

fn split_sections<'a>(lines: &'a [&'a str]) -> Vec<&'a [&'a str]> {
    let mut out = Vec::new();
    let mut start = 0;
    for (i, line) in lines.iter().enumerate() {
        if is_section_separator(line) {
            if start < i {
                out.push(&lines[start..i]);
            }
            start = i + 1;
        }
    }
    if start < lines.len() {
        out.push(&lines[start..]);
    }
    out
}

fn is_section_separator(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c == '-')
}

fn parse_first_int(s: &str) -> Option<u32> {
    let mut chars = s.chars().peekable();
    while matches!(chars.peek(), Some(c) if !c.is_ascii_digit()) {
        chars.next();
    }
    let mut digits = String::new();
    while let Some(&c) = chars.peek() {
        if c.is_ascii_digit() {
            digits.push(c);
            chars.next();
        } else {
            break;
        }
    }
    digits.parse().ok()
}

/// Heuristic: a section is a mod section if it contains at least one line that doesn't
/// look like metadata. "Section" here = the slice between two `--------` separators.
fn looks_like_mod_section(section: &[&str]) -> bool {
    section.iter().any(|line| {
        let l = line.trim();
        !(l.starts_with("Quality:")
            || l.starts_with("Item Level:")
            || l.starts_with("Sockets:")
            || l.starts_with("Requirements")
            || l.starts_with("Level:")
            || l.starts_with("Str:")
            || l.starts_with("Dex:")
            || l.starts_with("Int:")
            || l.starts_with("Note:")
            || l == "Corrupted"
            || l == "Mirrored"
            || l == "Unidentified"
            || l == "Foiled")
    })
}

/// Strip trailing `(implicit)` / `(crafted)` / etc. from a mod line. Returns the cleaned
/// line plus the override section if any.
fn strip_mod_suffix(line: &str) -> (&str, Option<ModSection>) {
    if let Some(stripped) = line.strip_suffix(" (implicit)") {
        return (stripped, Some(ModSection::Implicit));
    }
    if let Some(stripped) = line.strip_suffix(" (enchant)") {
        return (stripped, Some(ModSection::Enchant));
    }
    if let Some(stripped) = line.strip_suffix(" (crafted)") {
        return (stripped, Some(ModSection::Crafted));
    }
    if let Some(stripped) = line.strip_suffix(" (fractured)") {
        return (stripped, Some(ModSection::Fractured));
    }
    if let Some(stripped) = line.strip_suffix(" (corrupted)") {
        return (stripped, Some(ModSection::Corrupted));
    }
    if let Some(stripped) = line.strip_suffix(" (veiled)") {
        return (stripped, Some(ModSection::Veiled));
    }
    (line, None)
}

/// For magic items, the line carries `<prefix> <base> <suffix>`. We approximate the base
/// as the longest known suffix-trim heuristic; in Phase 3b we just strip a trailing "of
/// X" if present, leaving the rest.
fn strip_magic_affixes(line: &str) -> String {
    if let Some(idx) = line.rfind(" of ") {
        return line[..idx].to_owned();
    }
    line.to_owned()
}

/// Apply the parsed item's mods to a `ModList` using the existing `mod_parser`.
/// `slot_index` is used for the source attribution.
pub fn item_mods_into_modlist(
    item: &Item,
    slot_index: u32,
    out: &mut crate::ModList,
) -> usize {
    let mut produced = 0usize;
    for ml in &item.mod_lines {
        if let Some(parsed) = crate::mod_parser::parse_mod_line(&ml.line) {
            let m = parsed.mod_.with_source(crate::Source::Item(slot_index));
            out.add(m);
            produced += 1;
        }
    }
    produced
}

/// Add every item's mods from an `ItemSet` to a target ModDB. Also seeds known base-type
/// implicits: a shield's intrinsic block chance, an armour's intrinsic armour rating, etc.
/// Without this, a "Wooden Shield" equipped with no explicit affixes wouldn't actually
/// add block chance — the calc layer would only see modifier strings, not the weapon /
/// armour numbers from the base.
pub fn apply_item_set(item_set: &ItemSet, db: &mut crate::ModDB) -> ItemApplyReport {
    apply_item_set_with_bases(item_set, db, None)
}

/// Same as `apply_item_set` but also looks up canonical bases when a `bases` map is
/// provided so seeded implicits use real numbers (armour, evasion, ES, weapon damage,
/// block chance, etc.).
pub fn apply_item_set_with_bases(
    item_set: &ItemSet,
    db: &mut crate::ModDB,
    bases: Option<&pob_data::bases::ItemBaseSet>,
) -> ItemApplyReport {
    let mut report = ItemApplyReport::default();
    for (slot, item) in item_set.iter() {
        let slot_index = slot_to_index(*slot);
        let base = bases.and_then(|b| b.get(&item.base_name));

        // Heuristic implicits if no canonical base lookup is available; fall back when
        // a canonical base is present.
        if let Some(b) = base {
            // Armour from canonical base — average of min/max.
            if let Some(a) = b.armour.as_ref() {
                let armour = (a.armour_base_min + a.armour_base_max) * 0.5;
                let evasion = (a.evasion_base_min + a.evasion_base_max) * 0.5;
                let es = (a.energy_shield_base_min + a.energy_shield_base_max) * 0.5;
                let ward = (a.ward_base_min + a.ward_base_max) * 0.5;
                if armour > 0.0 {
                    db.add(
                        crate::Mod::base("Armour", f64::from(armour))
                            .with_source(crate::Source::Item(slot_index)),
                    );
                }
                if evasion > 0.0 {
                    db.add(
                        crate::Mod::base("Evasion", f64::from(evasion))
                            .with_source(crate::Source::Item(slot_index)),
                    );
                }
                if es > 0.0 {
                    db.add(
                        crate::Mod::base("EnergyShield", f64::from(es))
                            .with_source(crate::Source::Item(slot_index)),
                    );
                }
                if ward > 0.0 {
                    db.add(
                        crate::Mod::base("Ward", f64::from(ward))
                            .with_source(crate::Source::Item(slot_index)),
                    );
                }
                if a.block_chance_base > 0.0 {
                    db.add(
                        crate::Mod::base("BlockChance", f64::from(a.block_chance_base))
                            .with_source(crate::Source::Item(slot_index)),
                    );
                }
            }
            // Weapon stats for the main- or off-hand weapon. Stored under
            // Weapon{1,2}{Min,Max,AttackRate,CritChance,Range} so the calc layer can
            // pull them when computing attack-skill DPS. Use static keys to avoid
            // re-formatting these strings on every compute pass.
            if let Some(w) = b.weapon.as_ref() {
                let keys: &[&'static str] = match *slot {
                    pob_data::Slot::Weapon1 => &[
                        "Weapon1PhysicalMin",
                        "Weapon1PhysicalMax",
                        "Weapon1AttackRate",
                        "Weapon1CritChance",
                        "Weapon1Range",
                    ],
                    pob_data::Slot::Weapon2 => &[
                        "Weapon2PhysicalMin",
                        "Weapon2PhysicalMax",
                        "Weapon2AttackRate",
                        "Weapon2CritChance",
                        "Weapon2Range",
                    ],
                    _ => &[],
                };
                if !keys.is_empty() {
                    let values = [
                        f64::from(w.physical_min),
                        f64::from(w.physical_max),
                        f64::from(w.attack_rate_base),
                        f64::from(w.crit_chance_base),
                        f64::from(w.range),
                    ];
                    for (k, &v) in keys.iter().zip(values.iter()) {
                        if v > 0.0 {
                            db.add(
                                crate::Mod::base(*k, v)
                                    .with_source(crate::Source::Item(slot_index)),
                            );
                        }
                    }
                }
            }
        } else if item.base_name.contains("Shield") || item.base_name.contains("Buckler") {
            // No base lookup available — fall back to a generic 20% block chance.
            db.add(
                crate::Mod::base("BlockChance", 20.0)
                    .with_source(crate::Source::Item(slot_index)),
            );
        }

        for ml in &item.mod_lines {
            if let Some(parsed) = crate::mod_parser::parse_mod_line(&ml.line) {
                let m = parsed.mod_.with_source(crate::Source::Item(slot_index));
                db.add(m);
                report.parsed += 1;
            } else {
                report.unparsed += 1;
            }
        }
    }
    report
}

#[derive(Debug, Default, Clone, Copy)]
pub struct ItemApplyReport {
    pub parsed: u32,
    pub unparsed: u32,
}

pub fn slot_to_index(slot: Slot) -> u32 {
    match slot {
        Slot::Helmet => 1,
        Slot::BodyArmour => 2,
        Slot::Gloves => 3,
        Slot::Boots => 4,
        Slot::Amulet => 5,
        Slot::Ring1 => 6,
        Slot::Ring2 => 7,
        Slot::Belt => 8,
        Slot::Weapon1 => 9,
        Slot::Weapon2 => 10,
        Slot::Flask1 => 11,
        Slot::Flask2 => 12,
        Slot::Flask3 => 13,
        Slot::Flask4 => 14,
        Slot::Flask5 => 15,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_RARE_AMULET: &str = r"Item Class: Amulets
Rarity: RARE
Soul Charm
Onyx Amulet
--------
Quality: +20% (augmented)
--------
Requirements:
Level: 70
--------
Item Level: 84
--------
+10 to all Attributes
--------
+62 to maximum Life
+39% to all Elemental Resistances
20% increased Light Radius
--------";

    #[test]
    fn parses_rare_amulet_basics() {
        let item = parse_item(SAMPLE_RARE_AMULET).unwrap();
        assert_eq!(item.rarity, Rarity::Rare);
        assert_eq!(item.name, "Soul Charm");
        assert_eq!(item.base_name, "Onyx Amulet");
        assert_eq!(item.quality, 20);
        assert_eq!(item.item_level, 84);
        // 1 implicit + 3 explicit = 4 mod lines
        assert_eq!(item.mod_lines.len(), 4);
        assert_eq!(item.mod_lines[0].section, ModSection::Implicit);
        assert_eq!(item.mod_lines[0].line, "+10 to all Attributes");
        assert!(item.mod_lines[1..]
            .iter()
            .all(|m| m.section == ModSection::Explicit));
    }

    #[test]
    fn item_mods_apply_to_modlist() {
        let item = parse_item(SAMPLE_RARE_AMULET).unwrap();
        let mut list = crate::ModList::new();
        let n = item_mods_into_modlist(&item, 5, &mut list);
        // +10 to all Attributes, +62 to Life, +39% to all Resistances → 3 parse cleanly;
        // the 'increased Light Radius' line parses as Inc on LightRadius too.
        assert!(n >= 3, "expected at least 3 parsed mods, got {n}");
    }

    #[test]
    fn item_set_applies_mods_to_db() {
        let item = parse_item(SAMPLE_RARE_AMULET).unwrap();
        let mut set = pob_data::ItemSet::new();
        set.equip(Slot::Amulet, item);
        let mut db = crate::ModDB::new();
        let report = apply_item_set(&set, &mut db);
        assert!(report.parsed >= 3);
    }

    #[test]
    fn corrupted_flag_picked_up() {
        let raw = format!("{SAMPLE_RARE_AMULET}\nCorrupted");
        let item = parse_item(&raw).unwrap();
        assert!(item.corrupted);
    }

    #[test]
    fn implicit_suffix_overrides_section() {
        let raw = r"Rarity: RARE
Soul Charm
Onyx Amulet
--------
Item Level: 84
--------
+10 to all Attributes (implicit)
+62 to maximum Life
+39% to all Elemental Resistances
--------";
        let item = parse_item(raw).unwrap();
        let implicits: Vec<_> = item
            .mod_lines
            .iter()
            .filter(|m| m.section == ModSection::Implicit)
            .collect();
        assert_eq!(implicits.len(), 1);
        assert_eq!(implicits[0].line, "+10 to all Attributes");
    }
}
