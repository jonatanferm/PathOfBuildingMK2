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
    // Issue #221: collected variant metadata. `variants` is the ordered
    // list of display names; `selected_variant` is the 1-based active
    // index from a `Selected Variant:` line.
    let mut variants: Vec<String> = Vec::new();
    let mut selected_variant: Option<u32> = None;

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
            i += 1;
        }
    } else if let Some(n) = first.get(i) {
        // Magic / Normal: leave name = "" and use the line as the base.
        base_name = strip_magic_affixes(n);
        name.clear();
        i += 1;
    }
    // Issue #221: PoB emits `Variant: <name>` and `Selected Variant: N`
    // immediately after the name/base lines but before the first
    // `--------`. Scan the rest of the first section for them — the
    // shared loop below handles the same patterns in later sections so
    // that hand-edited pastes (which sometimes put them after the
    // metadata separator) round-trip too.
    while let Some(line) = first.get(i) {
        if let Some(rest) = line.strip_prefix("Variant:") {
            let name = rest.trim();
            if !name.is_empty() {
                variants.push(name.to_owned());
            }
        } else if let Some(rest) = line.strip_prefix("Selected Variant:") {
            selected_variant = parse_first_int(rest);
        } else if !(line.starts_with("Has Alt Variant")
            || line.starts_with("Selected Alt Variant")
            || *line == "Has Variants"
            || line.starts_with("Selected Variants"))
        {
            // Unknown metadata line in the header — leave it for the
            // existing skip-everything-we-don't-recognise behaviour
            // (the parser is intentionally permissive about extra
            // PoE-game lines like "Unidentified" / requirement
            // headers).
        }
        i += 1;
    }

    // Sweep remaining sections for metadata + mod lines.
    let mut explicit_section_started = false;
    let mut mod_lines: Vec<ModLine> = Vec::new();
    for section in sections.iter().skip(1) {
        let section_is_mod_section = looks_like_mod_section(section);
        // If any line in this mod section has an explicit (implicit) / (crafted) / …
        // suffix, the rest of the section defaults to explicit. Otherwise the first
        // mod section is treated as all-implicit.
        let any_self_tagged = section.iter().any(|l| strip_mod_suffix(l).1.is_some());
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
            // Issue #221: variant metadata. `Variant: <name>` lines feed
            // the variant-name list; `Selected Variant: N` picks the
            // active 1-based index. PoB emits these in the metadata
            // section above the mod sections; we accept them anywhere
            // for resilience against hand-edited paste text. The
            // `Has Alt Variant`/`Selected Alt Variant` family is
            // skipped — PoB uses those for items that rotate two
            // independent axes (anointment + aura combo on
            // Doryani's), which MK2 doesn't model yet.
            if let Some(rest) = line.strip_prefix("Variant:") {
                let name = rest.trim();
                if !name.is_empty() {
                    variants.push(name.to_owned());
                }
                continue;
            }
            if let Some(rest) = line.strip_prefix("Selected Variant:") {
                selected_variant = parse_first_int(rest);
                continue;
            }
            if line.starts_with("Has Alt Variant")
                || line.starts_with("Selected Alt Variant")
                || line == "Has Variants"
                || line.starts_with("Selected Variants")
            {
                continue;
            }

            if !section_is_mod_section {
                continue;
            }

            // Issue #221: strip a leading `{variant:N,M,…}` prefix off the
            // mod line. PoB uses a chain of `{key:val}` prefixes
            // (`{variant:1,3}{range:0.5}+10 to Strength`); we currently
            // only consume the variant gate and drop any other
            // bracketed prefixes verbatim, since the calc engine
            // doesn't model `range` / `tags` yet. Lines without a
            // variant prefix get `variant_list = None` (= "applies to
            // every variant"), matching PoB's default.
            let (raw_line, variant_list) = split_variant_prefix(line);
            let (clean, suffix_section) = strip_mod_suffix(raw_line);
            let section_kind = suffix_section.unwrap_or({
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
                variant_list,
            });
        }
        if section_is_mod_section {
            // The first mod section is implicits; subsequent ones are explicits unless a
            // `(implicit)` suffix says otherwise.
            explicit_section_started = true;
        }
    }

    // Issue #221: clamp the selected variant against the actual variant
    // count (PoB does this in `BuildModListForItem`). A bogus `Selected
    // Variant: 9` on a 2-variant item resolves to variant 2 instead of
    // silently filtering every gated mod line.
    let variant = if variants.is_empty() {
        None
    } else {
        selected_variant.map(|v| v.clamp(1, u32::try_from(variants.len()).unwrap_or(u32::MAX)))
    };

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
        variants,
        variant,
    })
}

/// Issue #221: split a leading variant prefix off a mod line. Returns the
/// post-prefix text and the parsed variant ids (`None` when the line had
/// no `{variant:…}` prefix). Other `{key:val}` prefixes are passed
/// through verbatim — the calc engine doesn't model `range` / `tags`
/// yet, and `pob_export` reconstitutes any stripped prefix when needed.
fn split_variant_prefix(line: &str) -> (&str, Option<Vec<u32>>) {
    let trimmed = line.trim_start();
    let rest = match trimmed.strip_prefix("{variant:") {
        Some(r) => r,
        None => return (line, None),
    };
    let Some(end) = rest.find('}') else {
        return (line, None);
    };
    let body = &rest[..end];
    let after = &rest[end + 1..];
    let mut ids = Vec::new();
    for tok in body.split(',') {
        let tok = tok.trim();
        if let Ok(n) = tok.parse::<u32>() {
            if n > 0 {
                ids.push(n);
            }
        }
    }
    if ids.is_empty() {
        // Malformed prefix (`{variant:}` / `{variant:abc}`) — preserve the
        // raw text rather than silently filtering the line out of every
        // variant.
        return (line, None);
    }
    (after, Some(ids))
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
/// `slot_index` is used for the source attribution. Item mods also pick up a
/// `SlotName` tag so the eval system can filter them — today the SlotName
/// condition is set whenever a slot is occupied, but the tag opens the door
/// to future active-weapon-only scoping.
pub fn item_mods_into_modlist(item: &Item, slot_index: u32, out: &mut crate::ModList) -> usize {
    let mut produced = 0usize;
    let slot_name = slot_name_for_index(slot_index);
    // Issue #221: only emit mods for lines that apply to the active
    // variant. Items without variants short-circuit via
    // `ModLine::applies_to_variant` returning true for the `None` gate.
    for ml in item.iter_active_mod_lines() {
        if let Some(parsed) = crate::mod_parser::parse_mod_line(&ml.line) {
            let mut m = parsed.mod_.with_source(crate::Source::Item(slot_index));
            if let Some(name) = slot_name {
                m.tags.push(crate::Tag {
                    kind: crate::TagKind::SlotName {
                        slot_name: name.to_owned(),
                        neg: false,
                    },
                });
            }
            out.add(m);
            produced += 1;
        }
    }
    produced
}

/// Look up a slot's PoB-style name from its 1-based slot index. Used to
/// stamp a `SlotName` tag onto item mods.
fn slot_name_for_index(slot_index: u32) -> Option<&'static str> {
    use pob_data::Slot::{
        Amulet, Belt, BodyArmour, Boots, Flask1, Flask2, Flask3, Flask4, Flask5, Gloves, Helmet,
        Ring1, Ring2, Weapon1, Weapon2,
    };
    let slot = match slot_index {
        1 => Helmet,
        2 => BodyArmour,
        3 => Gloves,
        4 => Boots,
        5 => Amulet,
        6 => Ring1,
        7 => Ring2,
        8 => Belt,
        9 => Weapon1,
        10 => Weapon2,
        11 => Flask1,
        12 => Flask2,
        13 => Flask3,
        14 => Flask4,
        15 => Flask5,
        _ => return None,
    };
    Some(crate::pob_import::pob_slot_to_name(slot))
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
                crate::Mod::base("BlockChance", 20.0).with_source(crate::Source::Item(slot_index)),
            );
        }

        let slot_name = slot_name_for_index(slot_index);
        // Issue #221: variant-gated mods only contribute to the build
        // when the gate includes the active variant. Lines without a
        // gate (the vast majority — every non-Watcher's-Eye / non
        // Maven's-Invitation item) iterate unchanged.
        for ml in item.iter_active_mod_lines() {
            if let Some(parsed) = crate::mod_parser::parse_mod_line(&ml.line) {
                let mut m = parsed.mod_.with_source(crate::Source::Item(slot_index));
                if let Some(name) = slot_name {
                    m.tags.push(crate::Tag {
                        kind: crate::TagKind::SlotName {
                            slot_name: name.to_owned(),
                            neg: false,
                        },
                    });
                }
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
        // Issue #109: swap-set slots get their own index slots so
        // they don't collide with the live pair when something keys
        // contributions by `slot_to_index`.
        Slot::Weapon1Swap => 16,
        Slot::Weapon2Swap => 17,
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

    // Issue #221: variants. Fixture mirrors a Watcher's Eye paste with two
    // recorded variants (Anger / Hatred) — the canonical multi-variant
    // unique. PoB serialises this as a `Variant:` line per name and a
    // single `Selected Variant: N` picking the active 1-based index;
    // mod lines that vary across variants carry a `{variant:N,M}`
    // prefix listing the variants they apply to.
    const SAMPLE_WATCHERS_EYE: &str = r"Rarity: UNIQUE
Watcher's Eye
Prismatic Jewel
Variant: Anger
Variant: Hatred
Selected Variant: 2
--------
Limited to: 1
--------
Item Level: 84
--------
+50 to maximum Mana
{variant:1}1% of Damage Leeched as Life while affected by Anger
{variant:2}10% increased Cold Damage while affected by Hatred
{variant:2}+1.2% to Critical Strike Chance while affected by Hatred
+25% to all Elemental Resistances
--------";

    #[test]
    fn parses_variant_metadata_into_names_and_selected_index() {
        let item = parse_item(SAMPLE_WATCHERS_EYE).unwrap();
        assert_eq!(item.variants, vec!["Anger".to_owned(), "Hatred".to_owned()]);
        assert_eq!(item.variant, Some(2));
    }

    #[test]
    fn parses_variant_prefix_into_variant_list_and_strips_text() {
        let item = parse_item(SAMPLE_WATCHERS_EYE).unwrap();
        // The Anger leech mod is gated on variant 1 only.
        let anger = item
            .mod_lines
            .iter()
            .find(|m| m.line.contains("Leeched as Life"))
            .expect("Anger mod present");
        assert_eq!(anger.variant_list, Some(vec![1]));
        assert!(
            !anger.line.starts_with("{variant"),
            "expected variant prefix to be stripped, got `{}`",
            anger.line
        );

        // The Hatred crit-chance mod is gated on variant 2.
        let hatred = item
            .mod_lines
            .iter()
            .find(|m| m.line.contains("Critical Strike Chance"))
            .expect("Hatred mod present");
        assert_eq!(hatred.variant_list, Some(vec![2]));

        // The shared +25% resists line is ungated.
        let resists = item
            .mod_lines
            .iter()
            .find(|m| m.line.contains("Elemental Resistances"))
            .expect("shared mod present");
        assert_eq!(resists.variant_list, None);
    }

    #[test]
    fn iter_active_mod_lines_filters_by_selected_variant() {
        let item = parse_item(SAMPLE_WATCHERS_EYE).unwrap();
        // Active variant is 2 (Hatred). The Anger-gated line must be
        // skipped; the two Hatred lines and every ungated line must
        // remain.
        let active: Vec<_> = item
            .iter_active_mod_lines()
            .map(|m| m.line.clone())
            .collect();
        assert!(
            !active.iter().any(|l| l.contains("Anger")),
            "expected Anger-gated mod to be filtered out, got {active:?}"
        );
        assert!(active
            .iter()
            .any(|l| l.contains("Hatred") && l.contains("Cold")));
        assert!(active.iter().any(|l| l.contains("Critical Strike Chance")));
        assert!(active.iter().any(|l| l.contains("Elemental Resistances")));
        assert!(active.iter().any(|l| l.contains("maximum Mana")));
    }

    #[test]
    fn variant_list_supports_multi_id_prefix() {
        let raw = r"Rarity: UNIQUE
Test Jewel
Cobalt Jewel
Variant: A
Variant: B
Variant: C
Selected Variant: 3
--------
Item Level: 84
--------
{variant:1,3}+10 to Strength
{variant:2}+10 to Dexterity
--------";
        let item = parse_item(raw).unwrap();
        let strength = item
            .mod_lines
            .iter()
            .find(|m| m.line.contains("Strength"))
            .expect("strength mod present");
        assert_eq!(strength.variant_list, Some(vec![1, 3]));
        // Active variant 3 picks up the {variant:1,3} line via membership.
        let active: Vec<_> = item
            .iter_active_mod_lines()
            .map(|m| m.line.clone())
            .collect();
        assert!(active.iter().any(|l| l.contains("Strength")));
        assert!(!active.iter().any(|l| l.contains("Dexterity")));
    }

    #[test]
    fn parser_clamps_selected_variant_to_existing_range() {
        let raw = r"Rarity: UNIQUE
Test Jewel
Cobalt Jewel
Variant: A
Variant: B
Selected Variant: 9
--------
Item Level: 84
--------
+10 to maximum Life
--------";
        let item = parse_item(raw).unwrap();
        // 9 is out of range for a 2-variant item; PoB clamps to the
        // highest variant id so the user still sees one of the real
        // variants instead of every gated mod silently disappearing.
        assert_eq!(item.variant, Some(2));
    }

    #[test]
    fn parser_leaves_variants_empty_for_non_variant_items() {
        let item = parse_item(SAMPLE_RARE_AMULET).unwrap();
        assert!(item.variants.is_empty());
        assert_eq!(item.variant, None);
        // Backwards-compat: iter_active_mod_lines yields everything.
        assert_eq!(item.iter_active_mod_lines().count(), item.mod_lines.len());
    }

    #[test]
    fn apply_item_set_filters_inactive_variant_mods_from_moddb() {
        // Issue #221: end-to-end smoke. Equip a two-variant unique
        // amulet on the active item set and confirm the mods landed
        // in the ModDB belong to the active variant only.
        //
        // The shared explicit (`+50 to maximum Life`) is always
        // present; the Anger-gated 1% leech line and the Hatred-gated
        // 10% cold damage line never overlap. Selecting variant 2
        // (Hatred) means the cold mod is present and the leech mod
        // is filtered out.
        let raw = r"Rarity: UNIQUE
Test Eye
Onyx Amulet
Variant: Anger
Variant: Hatred
Selected Variant: 2
--------
Item Level: 84
--------
+50 to maximum Life
{variant:1}1% of Damage Leeched as Life while affected by Anger
{variant:2}10% increased Cold Damage while affected by Hatred
--------";
        use crate::ModStore as _;
        let item = parse_item(raw).unwrap();
        let mut set = pob_data::ItemSet::new();
        set.equip(Slot::Amulet, item);
        let mut db = crate::ModDB::new();
        apply_item_set(&set, &mut db);

        // ModDB carries the +50 Life base and the Hatred-gated Cold
        // INC, but not the Anger-gated Leech INC. `Mod` exposes its
        // identity through `name`, category through `kind`, and the
        // scalar value through `value.as_f64()`.
        let life: f64 = db
            .iter_all()
            .filter(|m| m.name == "Life" && matches!(m.kind, crate::ModType::Base))
            .filter_map(|m| m.value.as_f64())
            .sum();
        assert!((life - 50.0).abs() < 1e-9, "expected +50 Life, got {life}");

        let cold_inc: f64 = db
            .iter_all()
            .filter(|m| m.name == "ColdDamage" && matches!(m.kind, crate::ModType::Inc))
            .filter_map(|m| m.value.as_f64())
            .sum();
        assert!(
            (cold_inc - 10.0).abs() < 1e-9,
            "expected 10% increased Cold Damage (active Hatred variant), got {cold_inc}"
        );

        // No leech contribution from the gated Anger line. We do a
        // case-insensitive contains over every mod's `name` so the
        // test stays valid whether `mod_parser` ends up routing leech
        // through `LifeLeechRate`, `DamageLifeLeech`, or any future
        // rename.
        let leech_any = db
            .iter_all()
            .any(|m| m.name.to_ascii_lowercase().contains("leech"));
        assert!(
            !leech_any,
            "expected no leech mod from filtered Anger variant"
        );
    }

    #[test]
    fn malformed_variant_prefix_falls_through_as_plain_text() {
        let raw = r"Rarity: RARE
Test
Onyx Amulet
--------
Item Level: 84
--------
{variant:}+10 to Strength
--------";
        let item = parse_item(raw).unwrap();
        // A `{variant:}` with no ids isn't a real variant gate — PoB
        // would treat the line as if the prefix were absent (it would
        // not strip the prefix); MK2 keeps the raw text so a human can
        // spot the bad paste. The key invariant is that we don't
        // silently filter the line out of every variant by recording
        // `Some(vec![])`.
        let mod_ = &item.mod_lines[0];
        assert_eq!(mod_.variant_list, None);
        assert!(
            mod_.line.contains("{variant:}"),
            "expected raw prefix preserved, got `{}`",
            mod_.line
        );
    }
}
