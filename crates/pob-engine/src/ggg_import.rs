//! Issue #32: import a character directly from the GGG (Grinding Gear
//! Games) public character API.
//!
//! Upstream PoB hits three endpoints under `pathofexile.com/character-window/`:
//!
//! - `get-characters?accountName=…` — list of every character on the
//!   account, with `name`, `class`, `classId`, `ascendancyClass`,
//!   `level`, `league`. We use this to populate the character picker.
//! - `get-passive-skills?accountName=…&character=…` — allocated tree
//!   nodes (the `hashes` array, plus `hashes_ex` for cluster jewel
//!   subgraph nodes), `mastery_effects` (`{nodeId: encoded}`), and
//!   the live `items` array (jewels socketed in the tree).
//! - `get-items?accountName=…&character=…` — the equipped + inventory
//!   items, plus a `character` envelope echoing `name / class / classId
//!   / ascendancyClass / level / league`.
//!
//! This module is wasm-clean — it owns the JSON shapes and the
//! mapping back to a [`Character`], but does no networking. The
//! desktop app's `pob-ui::ggg_fetch` module wires `ureq` on top
//! and surfaces 401 / 403 / 404 / 429 as typed errors. Wasm callers
//! can use `web_sys::fetch` to retrieve the same JSON and feed it
//! through the same parsers (the GGG endpoints currently allow CORS
//! for browser-side reads, but that's not guaranteed long-term).
//!
//! The mapping is intentionally shallow for slice 1: we set class,
//! ascendancy, level, allocated tree nodes, and equipped items.
//! Skills are out of scope (the GGG endpoint surfaces gems under
//! `socketedItems`; mapping those into PoB skill groups is a
//! follow-up). Mastery effects, jewel data, and the cluster-jewel
//! subgraph (`hashes_ex`) are captured into the right shape but the
//! engine doesn't yet model them — the imported character matches
//! what `pob_diff` would compute against a PoB import within the
//! existing divergence budget.

use std::fmt::Write as _;

use crate::character::{Character, ClassRef};
use crate::item_parser::parse_item;
use pob_data::{NodeId, Slot};
use serde::Deserialize;

/// Mirrors the relevant subset of one entry returned by
/// `character-window/get-characters`.
#[derive(Debug, Clone, Deserialize)]
pub struct CharacterSummary {
    pub name: String,
    /// In-game class (subclass) name — e.g. `"Necromancer"`,
    /// `"Witch"`, `"Pathfinder"`. The legacy GGG API echoes the
    /// ascendancy in this field when the character has ascended,
    /// otherwise the base class.
    #[serde(default)]
    pub class: String,
    /// Numeric base-class id (0=Scion, 1=Marauder, 2=Ranger,
    /// 3=Witch, 4=Duelist, 5=Templar, 6=Shadow). Used when the
    /// `class` string is missing or not yet resolved.
    #[serde(default, rename = "classId")]
    pub class_id: Option<u32>,
    #[serde(default, rename = "ascendancyClass")]
    pub ascendancy_class: Option<u32>,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub league: String,
}

/// Top-level shape of `get-characters` (a JSON array of characters).
pub type CharacterList = Vec<CharacterSummary>;

/// Subset of `get-passive-skills`. We deliberately model only what
/// drives the import — the endpoint also returns `jewel_data`,
/// `skill_overrides`, etc. that we let `serde_json::Value` swallow
/// via `#[serde(default)]` Optionals on the typed fields above.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct PassiveSkillsResponse {
    /// Every allocated node id. PoB calls this `hashList`.
    #[serde(default)]
    pub hashes: Vec<u32>,
    /// Cluster-jewel subgraph nodes — extra ids that exist only on
    /// the jewel-spawned subtree. Captured for completeness; the
    /// engine doesn't yet allocate against them, but a later slice
    /// can pick them up alongside the cluster-jewel synthesis pass.
    #[serde(default)]
    pub hashes_ex: Vec<u32>,
    /// Numeric base-class id, mirrors `CharacterSummary::class_id`.
    #[serde(default)]
    pub character: Option<u32>,
    /// 1-based ascendancy index inside the chosen class.
    #[serde(default)]
    pub ascendancy: Option<u32>,
    /// Items socketed *into* the passive tree (jewels, abyss jewels,
    /// and cluster jewels). Equipped gear lives on the `get-items`
    /// response instead.
    #[serde(default)]
    pub items: Vec<GggItem>,
    /// `{nodeId: encoded}` map. The encoded value is `mastery |
    /// (effect << 16)` per PoB's `ImportPassiveTreeAndJewels`. We
    /// preserve it as raw JSON-friendly values so a follow-up slice
    /// can decode and apply it without breaking shape compatibility.
    #[serde(default)]
    pub mastery_effects: serde_json::Value,
}

/// Subset of `get-items`. The endpoint nests the character envelope
/// under `character` and the actual gear under `items`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct ItemsResponse {
    #[serde(default)]
    pub character: Option<CharacterEnvelope>,
    #[serde(default)]
    pub items: Vec<GggItem>,
}

/// Mirrors `get-items.character` and the per-character entry on
/// `get-characters`. We treat any missing field as defaultable so
/// the parser is permissive across realm-specific shapes.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct CharacterEnvelope {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub class: String,
    #[serde(default, rename = "classId")]
    pub class_id: Option<u32>,
    #[serde(default, rename = "ascendancyClass")]
    pub ascendancy_class: Option<u32>,
    #[serde(default)]
    pub level: u32,
    #[serde(default)]
    pub league: String,
}

/// One equipped or socketed item from either `get-items` or
/// `get-passive-skills`. We capture only the subset the
/// item-paste-text rebuilder needs — name, type, sockets, and the
/// flattened mod arrays. Everything else is allowed to roll up
/// through the `serde(default)` permissiveness, including arbitrary
/// fields the GGG API may add later.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GggItem {
    /// Display name. Empty for non-rare bases.
    #[serde(default)]
    pub name: String,
    /// Base type label — e.g. `"Onyx Amulet"`. The "type-line".
    #[serde(default, rename = "typeLine")]
    pub type_line: String,
    /// PoE rarity index — 0=Normal, 1=Magic, 2=Rare, 3=Unique,
    /// 9 / 10 = Relic / Foiled-Relic.
    #[serde(default, rename = "frameType")]
    pub frame_type: u32,
    /// Inventory slot id from GGG: `Helm`, `BodyArmour`, `Gloves`,
    /// `Boots`, `Amulet`, `Ring`, `Ring2`, `Belt`, `Weapon`,
    /// `Offhand`, `Weapon2`, `Offhand2`, `Flask`, `PassiveJewels`,
    /// `MainInventory`, etc. We translate this into our `Slot`
    /// enum via [`ggg_slot_to_pob`]; unknown slots yield `None` and
    /// the item is skipped.
    #[serde(default, rename = "inventoryId")]
    pub inventory_id: String,
    /// Position-in-stash for flask / jewel layouts (we use this to
    /// disambiguate `Flask`'s `x` into Flask 1..5).
    #[serde(default)]
    pub x: u32,
    /// Item level.
    #[serde(default)]
    pub ilvl: u32,
    /// Quality. Not always present — flasks expose it under
    /// `properties` instead. We read both.
    #[serde(default)]
    pub quality: Option<u32>,
    #[serde(default)]
    pub corrupted: bool,
    #[serde(default)]
    pub mirrored: bool,
    #[serde(default, rename = "implicitMods")]
    pub implicit_mods: Vec<String>,
    #[serde(default, rename = "explicitMods")]
    pub explicit_mods: Vec<String>,
    #[serde(default, rename = "fracturedMods")]
    pub fractured_mods: Vec<String>,
    #[serde(default, rename = "craftedMods")]
    pub crafted_mods: Vec<String>,
    #[serde(default, rename = "enchantMods")]
    pub enchant_mods: Vec<String>,
    /// `[{name, values: [[string, type]], displayMode, type}]`. We
    /// scan it for `Quality` (used by flasks) but leave the rest as
    /// raw JSON noise.
    #[serde(default)]
    pub properties: Vec<ItemProperty>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ItemProperty {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub values: Vec<serde_json::Value>,
}

#[derive(Debug)]
pub enum GggImportError {
    /// The JSON document didn't deserialise into the expected shape.
    Json(String),
    /// The endpoint responded with `false` (PoB's "Failed to retrieve
    /// character data" path) or an empty body.
    Empty,
}

impl std::fmt::Display for GggImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Json(e) => write!(f, "GGG JSON parse failed: {e}"),
            Self::Empty => write!(f, "GGG response was empty / 'false'"),
        }
    }
}

impl std::error::Error for GggImportError {}

/// Parse the `get-characters` response.
///
/// GGG returns either a JSON array (success) or the literal `false`
/// when the account doesn't exist or has no characters. The latter
/// surfaces as `GggImportError::Empty`.
pub fn parse_character_list(json: &str) -> Result<CharacterList, GggImportError> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "false" {
        return Err(GggImportError::Empty);
    }
    serde_json::from_str(trimmed).map_err(|e| GggImportError::Json(e.to_string()))
}

/// Parse a `get-passive-skills` JSON document.
pub fn parse_passive_skills(json: &str) -> Result<PassiveSkillsResponse, GggImportError> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "false" {
        return Err(GggImportError::Empty);
    }
    serde_json::from_str(trimmed).map_err(|e| GggImportError::Json(e.to_string()))
}

/// Parse a `get-items` JSON document.
pub fn parse_items(json: &str) -> Result<ItemsResponse, GggImportError> {
    let trimmed = json.trim();
    if trimmed.is_empty() || trimmed == "false" {
        return Err(GggImportError::Empty);
    }
    serde_json::from_str(trimmed).map_err(|e| GggImportError::Json(e.to_string()))
}

/// Build a [`Character`] out of the three GGG responses.
///
/// `summary` is optional — when provided it acts as a fallback for
/// `class` / `level` if the items envelope is missing them. The
/// passive-skills response drives the allocated nodes; the items
/// response drives equipped gear. Skills are not yet imported (see
/// the module-level docstring for the slice-1 scope statement).
pub fn build_character(
    summary: Option<&CharacterSummary>,
    passive: &PassiveSkillsResponse,
    items_resp: &ItemsResponse,
) -> Character {
    let envelope = items_resp.character.as_ref();

    // Resolve class / ascendancy / level. Priority: items.character
    // (most authoritative when present) → summary → defaults.
    let class_name = envelope
        .map(|e| e.class.as_str())
        .filter(|s| !s.is_empty())
        .or_else(|| summary.map(|s| s.class.as_str()).filter(|s| !s.is_empty()))
        .unwrap_or("");
    let class_id = envelope
        .and_then(|e| e.class_id)
        .or_else(|| summary.and_then(|s| s.class_id))
        .or(passive.character);
    let ascendancy_index = envelope
        .and_then(|e| e.ascendancy_class)
        .or_else(|| summary.and_then(|s| s.ascendancy_class))
        .or(passive.ascendancy);
    let level = envelope
        .map(|e| e.level)
        .filter(|n| *n > 0)
        .or_else(|| summary.map(|s| s.level).filter(|n| *n > 0))
        .unwrap_or(1);

    let (resolved_class, resolved_ascendancy) = resolve_class(class_name, class_id);

    let mut character =
        Character::new(resolved_class.unwrap_or_else(ClassRef::scion), level.max(1));
    // The `class` field above is the ascendancy name when the
    // character has ascended — which is what we want on
    // `character.ascendancy`. Keep `character.class` as the base
    // class so the tree-tab class anchor lines up with what the
    // user sees in PoB. If neither resolves cleanly, leave the
    // ascendancy unset and let the user pick later.
    character.ascendancy = resolved_ascendancy.or_else(|| {
        // Some realm responses leave `class` empty but still ship
        // `ascendancyClass` — fall back to "Ascendancy 1/2/3" so we
        // signal the user picked an ascendancy without inventing a
        // name. UI surfaces this as a disambiguation prompt; the
        // calc engine falls back to base-class output.
        ascendancy_index
            .filter(|n| *n > 0)
            .map(|n| format!("Ascendancy {n}"))
    });

    // Allocated tree nodes — primary `hashes`, plus the cluster
    // subgraph `hashes_ex`. We drop duplicates via the underlying
    // `HashSet`. Out-of-range ids (e.g. a node id from a tree
    // version we don't have loaded) are still inserted; the
    // tree-view filters them at render time.
    for id in passive.hashes.iter().copied() {
        character.allocated.insert(id as NodeId);
    }
    for id in passive.hashes_ex.iter().copied() {
        character.allocated.insert(id as NodeId);
    }

    // Equipped gear. Translate each `inventoryId` to a `Slot`,
    // synthesise the PoE-format paste text the existing item parser
    // already accepts, and equip the result. We swallow per-slot
    // failures so an exotic item the parser doesn't yet handle
    // doesn't block the whole import.
    for ggg_item in &items_resp.items {
        let Some(slot) = ggg_slot_to_pob(&ggg_item.inventory_id, ggg_item.x) else {
            continue;
        };
        let raw = render_item_paste(ggg_item);
        if let Ok(item) = parse_item(&raw) {
            character.items.equip(slot, item);
        }
    }

    character
}

/// Translate a GGG `inventoryId` (+ stash `x` for flasks) to our
/// `Slot` enum. Returns `None` for slots we don't model — most
/// importantly `MainInventory`, `Stash`, `PassiveJewels`, and
/// `BrequelGrafts*` (graft slots aren't in the current `Slot`
/// schema). PoB's `slotMap` table in `Classes/ImportTab.lua:985-986`
/// is the canonical reference for this mapping.
pub fn ggg_slot_to_pob(inventory_id: &str, x: u32) -> Option<Slot> {
    Some(match inventory_id {
        "Helm" => Slot::Helmet,
        "BodyArmour" => Slot::BodyArmour,
        "Gloves" => Slot::Gloves,
        "Boots" => Slot::Boots,
        "Amulet" => Slot::Amulet,
        "Ring" => Slot::Ring1,
        "Ring2" => Slot::Ring2,
        "Belt" => Slot::Belt,
        "Weapon" => Slot::Weapon1,
        "Offhand" => Slot::Weapon2,
        "Weapon2" => Slot::Weapon1Swap,
        "Offhand2" => Slot::Weapon2Swap,
        "Flask" => match x {
            0 => Slot::Flask1,
            1 => Slot::Flask2,
            2 => Slot::Flask3,
            3 => Slot::Flask4,
            4 => Slot::Flask5,
            _ => return None,
        },
        _ => return None,
    })
}

/// Translate a base-class index (0..=6) and class name to the
/// `(ClassRef, Option<ascendancy>)` pair we want on the resulting
/// `Character`. PoB / PoE share a class ordering — we mirror it
/// here so a `classId=3` import lands on Witch even when the JSON
/// doesn't include the `class` string.
fn resolve_class(class_name: &str, class_id: Option<u32>) -> (Option<ClassRef>, Option<String>) {
    // Base classes in canonical PoE order.
    const BASE_CLASSES: [&str; 7] = [
        "Scion", "Marauder", "Ranger", "Witch", "Duelist", "Templar", "Shadow",
    ];
    // Every ascendancy name we recognise → the base class it
    // belongs to. Mirrors PoB's `classColor` table in
    // `Classes/ImportTab.lua:570-595` (every alt-tree ascendancy is
    // included so a 3.x or PoE2 import still resolves).
    const ASCENDANCY_TO_BASE: &[(&str, &str)] = &[
        // Witch
        ("Necromancer", "Witch"),
        ("Occultist", "Witch"),
        ("Elementalist", "Witch"),
        ("Harbinger", "Witch"),
        ("Herald", "Witch"),
        ("Bog Shaman", "Witch"),
        // Templar
        ("Guardian", "Templar"),
        ("Inquisitor", "Templar"),
        ("Hierophant", "Templar"),
        ("Architect of Chaos", "Templar"),
        ("Polytheist", "Templar"),
        ("Puppeteer", "Templar"),
        // Shadow
        ("Assassin", "Shadow"),
        ("Trickster", "Shadow"),
        ("Saboteur", "Shadow"),
        ("Surfcaster", "Shadow"),
        ("Servant of Arakaali", "Shadow"),
        ("Blind Prophet", "Shadow"),
        // Duelist
        ("Gladiator", "Duelist"),
        ("Slayer", "Duelist"),
        ("Champion", "Duelist"),
        ("Gambler", "Duelist"),
        ("Paladin", "Duelist"),
        ("Aristocrat", "Duelist"),
        // Ranger
        ("Raider", "Ranger"),
        ("Pathfinder", "Ranger"),
        ("Deadeye", "Ranger"),
        ("Warden", "Ranger"),
        ("Daughter of Oshabi", "Ranger"),
        ("Whisperer", "Ranger"),
        ("Wildspeaker", "Ranger"),
        // Marauder
        ("Juggernaut", "Marauder"),
        ("Berserker", "Marauder"),
        ("Chieftain", "Marauder"),
        ("Antiquarian", "Marauder"),
        ("Behemoth", "Marauder"),
        ("Ancestral Commander", "Marauder"),
        // Scion
        ("Ascendant", "Scion"),
        ("Reliquarian", "Scion"),
        ("Scavenger", "Scion"),
    ];

    if !class_name.is_empty() {
        // If the JSON's class string is itself a base class, use
        // it directly with no ascendancy.
        if BASE_CLASSES.contains(&class_name) {
            return (Some(ClassRef(class_name.to_owned())), None);
        }
        // Otherwise look it up in the ascendancy → base table.
        if let Some(&(_, base)) = ASCENDANCY_TO_BASE.iter().find(|&&(a, _)| a == class_name) {
            return (Some(ClassRef(base.to_owned())), Some(class_name.to_owned()));
        }
        // Unknown class string — fall through to the id-based
        // fallback below so we still resolve a base class.
    }
    if let Some(id) = class_id {
        if let Some(name) = BASE_CLASSES.get(id as usize) {
            return (Some(ClassRef((*name).to_owned())), None);
        }
    }
    (None, None)
}

/// Render a GGG `GggItem` into the PoE copy-paste text format the
/// existing `item_parser::parse_item` accepts. Mirrors the field
/// fan-out PoB does in `Classes/ImportTab.lua:1005-1235`. We emit:
///
/// ```text
/// Rarity: <rarity>
/// <name>           ; only for Rare / Unique / Relic
/// <base type>
/// --------
/// Quality: +Q% (augmented)
/// --------
/// Item Level: N
/// --------
/// <implicit lines>
/// --------
/// <explicit + crafted + fractured lines>
/// --------
/// Corrupted
/// ```
///
/// Mod-line tagging mirrors the parser: `(crafted)` / `(implicit)`
/// suffixes flip the section, otherwise the first dash-separated
/// section after the metadata block is treated as implicits and
/// subsequent ones as explicits.
fn render_item_paste(item: &GggItem) -> String {
    let mut out = String::new();
    let rarity_word = match item.frame_type {
        0 => "NORMAL",
        1 => "MAGIC",
        2 => "RARE",
        3 => "UNIQUE",
        9 | 10 => "RELIC",
        _ => "RARE",
    };
    out.push_str("Rarity: ");
    out.push_str(rarity_word);
    out.push('\n');
    if !item.name.is_empty() {
        out.push_str(&item.name);
        out.push('\n');
    }
    if !item.type_line.is_empty() {
        out.push_str(&item.type_line);
        out.push('\n');
    }
    out.push_str("--------\n");

    // Quality. PoB stores it on the item directly for gear, on a
    // `properties` entry for flasks. We accept either.
    let quality = item.quality.unwrap_or_else(|| {
        item.properties
            .iter()
            .find(|p| p.name == "Quality")
            .and_then(|p| p.values.first())
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
            .and_then(|s| {
                s.trim_start_matches(['+', '-'])
                    .trim_end_matches('%')
                    .parse::<u32>()
                    .ok()
            })
            .unwrap_or(0)
    });
    if quality > 0 {
        let _ = write!(out, "Quality: +{quality}% (augmented)\n--------\n");
    }

    if item.ilvl > 0 {
        let _ = write!(out, "Item Level: {}\n--------\n", item.ilvl);
    }

    // Enchant + implicit + explicit chains. Each becomes its own
    // dash-separated section so the existing parser tags them
    // correctly. Empty lists are skipped to avoid producing
    // adjacent `--------` separators (the parser tolerates them but
    // the test fixtures get noisy).
    for line in &item.enchant_mods {
        out.push_str(line.trim());
        out.push_str(" (enchant)\n");
    }
    if !item.enchant_mods.is_empty() {
        out.push_str("--------\n");
    }

    for line in &item.implicit_mods {
        out.push_str(line.trim());
        out.push('\n');
    }
    if !item.implicit_mods.is_empty() {
        out.push_str("--------\n");
    }

    let mut any_explicit = false;
    for line in &item.fractured_mods {
        out.push_str(line.trim());
        out.push_str(" (fractured)\n");
        any_explicit = true;
    }
    for line in &item.explicit_mods {
        out.push_str(line.trim());
        out.push('\n');
        any_explicit = true;
    }
    for line in &item.crafted_mods {
        out.push_str(line.trim());
        out.push_str(" (crafted)\n");
        any_explicit = true;
    }
    if any_explicit {
        out.push_str("--------\n");
    }

    if item.corrupted {
        out.push_str("Corrupted\n");
    }
    if item.mirrored {
        out.push_str("Mirrored\n");
    }

    out
}

/// Encode a (cleaned) account name and character name pair into the
/// query-string PoE expects for the `character-window` endpoints.
///
/// PoB cleans the account name by stripping spaces and normalising
/// the discriminator (`#1234` → `%231234`). We mirror that here so
/// callers can pass the user input verbatim and get back a
/// well-formed URL fragment.
pub fn encode_account_name(account: &str) -> String {
    let mut cleaned = account.trim().replace(' ', "");
    // Replace `name-1234` with `name#1234` (PoB does the same; the
    // `-` form is what poe.com URLs use, the `#` form is what GGG's
    // game client and the auth cookie use).
    if let Some(idx) = cleaned.rfind('-') {
        let (head, tail) = cleaned.split_at(idx);
        if tail.len() > 1 && tail[1..].chars().all(|c| c.is_ascii_digit()) {
            cleaned = format!("{head}#{}", &tail[1..]);
        }
    }
    cleaned.replace('#', "%23")
}

/// Percent-encode a path segment for the GGG `?character=…` query
/// parameter. Reserved characters keep their literal interpretation
/// (`?`, `&`, `=`, `#`, `+`, `%`, space). Mirrors PoB's
/// `urlEncode`, narrowed to what the GGG endpoints care about.
pub fn url_encode_segment(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for byte in s.as_bytes() {
        let c = *byte;
        match c {
            b'A'..=b'Z' | b'a'..=b'z' | b'0'..=b'9' | b'-' | b'_' | b'.' | b'~' => {
                out.push(c as char);
            }
            _ => {
                let _ = write!(out, "%{c:02X}");
            }
        }
    }
    out
}

/// Build the URL for the `get-characters?accountName=…` endpoint.
/// `realm` is `"pc"` / `"xbox"` / `"sony"` (case-insensitive) — PoB
/// passes whatever the realm dropdown picks; only `"pc"` is
/// guaranteed to work for non-account-bound profiles. We default to
/// `pc` when the input is empty.
pub fn get_characters_url(account: &str, realm: &str) -> String {
    let realm = realm_or_default(realm);
    format!(
        "https://www.pathofexile.com/character-window/get-characters?accountName={}&realm={realm}",
        encode_account_name(account),
    )
}

/// `get-passive-skills?accountName=…&character=…&realm=…`.
pub fn get_passive_skills_url(account: &str, character: &str, realm: &str) -> String {
    let realm = realm_or_default(realm);
    format!(
        "https://www.pathofexile.com/character-window/get-passive-skills?accountName={}&character={}&realm={realm}",
        encode_account_name(account),
        url_encode_segment(character),
    )
}

/// `get-items?accountName=…&character=…&realm=…`.
pub fn get_items_url(account: &str, character: &str, realm: &str) -> String {
    let realm = realm_or_default(realm);
    format!(
        "https://www.pathofexile.com/character-window/get-items?accountName={}&character={}&realm={realm}",
        encode_account_name(account),
        url_encode_segment(character),
    )
}

fn realm_or_default(realm: &str) -> &str {
    match realm.trim().to_ascii_lowercase().as_str() {
        "" | "pc" => "pc",
        "xbox" => "xbox",
        "sony" | "ps" | "ps4" | "playstation" => "sony",
        // Fall through unchanged for anything else; GGG echoes the
        // realm code back in the validation error so the caller can
        // surface a useful message.
        _ => "pc",
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn parse_character_list_handles_false() {
        assert!(matches!(
            parse_character_list("false"),
            Err(GggImportError::Empty)
        ));
        assert!(matches!(
            parse_character_list(""),
            Err(GggImportError::Empty)
        ));
    }

    #[test]
    fn parse_character_list_round_trips() {
        let json = r#"[
            {"name":"Hero","class":"Necromancer","classId":3,"ascendancyClass":1,"level":92,"league":"Standard"},
            {"name":"Sidekick","class":"Witch","classId":3,"ascendancyClass":0,"level":40,"league":"Standard"}
        ]"#;
        let list = parse_character_list(json).unwrap();
        assert_eq!(list.len(), 2);
        assert_eq!(list[0].name, "Hero");
        assert_eq!(list[0].class, "Necromancer");
        assert_eq!(list[0].class_id, Some(3));
        assert_eq!(list[0].ascendancy_class, Some(1));
        assert_eq!(list[0].level, 92);
    }

    #[test]
    fn resolve_class_handles_base_string() {
        let (class, asc) = resolve_class("Witch", Some(3));
        assert_eq!(class.unwrap().0, "Witch");
        assert_eq!(asc, None);
    }

    #[test]
    fn resolve_class_handles_ascendancy_string() {
        let (class, asc) = resolve_class("Necromancer", Some(3));
        assert_eq!(class.unwrap().0, "Witch");
        assert_eq!(asc.as_deref(), Some("Necromancer"));
    }

    #[test]
    fn resolve_class_falls_back_to_id() {
        let (class, asc) = resolve_class("", Some(1));
        assert_eq!(class.unwrap().0, "Marauder");
        assert_eq!(asc, None);
    }

    #[test]
    fn resolve_class_unknown_returns_none() {
        let (class, asc) = resolve_class("NotAClass", None);
        assert!(class.is_none());
        assert!(asc.is_none());
    }

    #[test]
    fn ggg_slot_to_pob_maps_known_slots() {
        assert_eq!(ggg_slot_to_pob("Helm", 0), Some(Slot::Helmet));
        assert_eq!(ggg_slot_to_pob("BodyArmour", 0), Some(Slot::BodyArmour));
        assert_eq!(ggg_slot_to_pob("Weapon", 0), Some(Slot::Weapon1));
        assert_eq!(ggg_slot_to_pob("Offhand", 0), Some(Slot::Weapon2));
        assert_eq!(ggg_slot_to_pob("Weapon2", 0), Some(Slot::Weapon1Swap));
        assert_eq!(ggg_slot_to_pob("Offhand2", 0), Some(Slot::Weapon2Swap));
        assert_eq!(ggg_slot_to_pob("Ring", 0), Some(Slot::Ring1));
        assert_eq!(ggg_slot_to_pob("Ring2", 0), Some(Slot::Ring2));
        assert_eq!(ggg_slot_to_pob("Flask", 0), Some(Slot::Flask1));
        assert_eq!(ggg_slot_to_pob("Flask", 4), Some(Slot::Flask5));
        assert_eq!(ggg_slot_to_pob("MainInventory", 0), None);
        assert_eq!(ggg_slot_to_pob("PassiveJewels", 0), None);
        assert_eq!(ggg_slot_to_pob("Flask", 5), None);
    }

    #[test]
    fn encode_account_name_normalises_discriminator() {
        assert_eq!(encode_account_name("Hero#1234"), "Hero%231234");
        assert_eq!(encode_account_name("Hero-1234"), "Hero%231234");
        assert_eq!(encode_account_name(" Hero #1234 "), "Hero%231234");
        assert_eq!(encode_account_name("plain"), "plain");
        // Don't get tripped up by hyphenated names without a numeric
        // suffix.
        assert_eq!(encode_account_name("hero-bandit"), "hero-bandit");
    }

    #[test]
    fn url_encode_segment_escapes_specials() {
        assert_eq!(url_encode_segment("plain"), "plain");
        assert_eq!(url_encode_segment("a b"), "a%20b");
        assert_eq!(url_encode_segment("a+b"), "a%2Bb");
        assert_eq!(url_encode_segment("a/b"), "a%2Fb");
        assert_eq!(url_encode_segment("ÄTest"), "%C3%84Test");
    }

    #[test]
    fn get_characters_url_format() {
        assert_eq!(
            get_characters_url("Hero#1234", "pc"),
            "https://www.pathofexile.com/character-window/get-characters?accountName=Hero%231234&realm=pc",
        );
    }

    #[test]
    fn get_passive_skills_url_format() {
        assert_eq!(
            get_passive_skills_url("Hero#1234", "MyChar", ""),
            "https://www.pathofexile.com/character-window/get-passive-skills?accountName=Hero%231234&character=MyChar&realm=pc",
        );
    }

    #[test]
    fn build_character_from_minimal_responses() {
        let summary = CharacterSummary {
            name: "Hero".into(),
            class: "Necromancer".into(),
            class_id: Some(3),
            ascendancy_class: Some(1),
            level: 92,
            league: "Standard".into(),
        };
        let passive = PassiveSkillsResponse {
            hashes: vec![59530, 55156, 57264],
            hashes_ex: vec![2151],
            character: Some(3),
            ascendancy: Some(1),
            ..Default::default()
        };
        let items = ItemsResponse::default();
        let c = build_character(Some(&summary), &passive, &items);
        assert_eq!(c.class.0, "Witch");
        assert_eq!(c.ascendancy.as_deref(), Some("Necromancer"));
        assert_eq!(c.level, 92);
        assert!(c.allocated.contains(&59530));
        assert!(c.allocated.contains(&2151));
    }

    #[test]
    fn build_character_equips_items_from_get_items() {
        let json = r#"{
            "items": [
                {
                    "name": "Soul Charm",
                    "typeLine": "Onyx Amulet",
                    "frameType": 2,
                    "inventoryId": "Amulet",
                    "ilvl": 84,
                    "implicitMods": ["+10 to all Attributes"],
                    "explicitMods": ["+62 to maximum Life", "+39% to all Elemental Resistances"]
                }
            ],
            "character": {
                "name": "Hero",
                "class": "Witch",
                "classId": 3,
                "level": 90
            }
        }"#;
        let items = parse_items(json).unwrap();
        let passive = PassiveSkillsResponse::default();
        let c = build_character(None, &passive, &items);
        assert_eq!(c.class.0, "Witch");
        assert_eq!(c.level, 90);
        let amulet = c
            .items
            .get(Slot::Amulet)
            .expect("amulet equipped from GGG response");
        assert_eq!(amulet.base_name, "Onyx Amulet");
        assert_eq!(amulet.name, "Soul Charm");
    }

    #[test]
    fn build_character_skips_unknown_slots() {
        let json = r#"{
            "items": [
                {"typeLine":"Stash thing","frameType":0,"inventoryId":"MainInventory"}
            ]
        }"#;
        let items = parse_items(json).unwrap();
        let c = build_character(None, &PassiveSkillsResponse::default(), &items);
        assert_eq!(c.items.iter().count(), 0);
    }

    #[test]
    fn build_character_imports_full_fixture() {
        let raw = include_str!("../tests/fixtures/ggg_character_passive.json");
        let passive = parse_passive_skills(raw).unwrap();
        let raw_items = include_str!("../tests/fixtures/ggg_character_items.json");
        let items = parse_items(raw_items).unwrap();
        let c = build_character(None, &passive, &items);
        assert_eq!(c.class.0, "Witch");
        assert_eq!(c.ascendancy.as_deref(), Some("Necromancer"));
        assert_eq!(c.level, 92);
        assert!(c.allocated.contains(&59530));
        assert!(c.items.get(Slot::Amulet).is_some());
        assert!(c.items.get(Slot::BodyArmour).is_some());
    }
}
