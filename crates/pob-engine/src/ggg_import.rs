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
//! Issue #194 follow-up: this module now imports skills (gems socketed
//! into equipped items via `socketedItems`), masteries (encoded in
//! `mastery_effects` as `mastery | (effect << 16)` per PoB), and
//! cluster-jewel subgraph nodes (`hashes_ex` mapped into the synth
//! NodeId namespace established by `cluster_synth`). Cluster jewel
//! items socketed into the tree (from `passive.items`) are now also
//! routed into `Character::jewels` so the synthesis pass can produce
//! the matching sub-graph at compute time.

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
    /// Issue #194: gems socketed into this item. PoE's API nests a
    /// per-gem item under `socketedItems`, with each entry carrying
    /// its own typeLine, properties (Level / Quality), and a `socket`
    /// index. We use this to build [`SocketGroup`]s on the imported
    /// character.
    #[serde(default, rename = "socketedItems")]
    pub socketed_items: Vec<SocketedGggItem>,
    /// `[{group, attr, sColour}]` — one entry per physical socket,
    /// used to bucket socketedItems into linked groups via the
    /// shared `group` field. Mirrors `item.sockets` in PoB's Lua.
    #[serde(default)]
    pub sockets: Vec<GggSocket>,
}

#[derive(Debug, Clone, Deserialize, Default)]
pub struct ItemProperty {
    #[serde(default)]
    pub name: String,
    #[serde(default)]
    pub values: Vec<serde_json::Value>,
}

/// One socket on a piece of equipment. PoE returns one entry per
/// physical socket; the `group` field is what links sockets together
/// (sockets sharing a `group` value form one linked socket group).
#[derive(Debug, Clone, Deserialize, Default)]
pub struct GggSocket {
    /// Linked-group id (0..N). Sockets with the same `group` are
    /// linked.
    #[serde(default)]
    pub group: u32,
    /// `S` / `D` / `I` / `G` / `A` / `DV` — the colour requirement.
    /// We don't act on this (the calc engine doesn't model
    /// off-colour socketing) but keep it in the schema for parity.
    #[serde(default, rename = "sColour")]
    pub s_colour: String,
}

/// A skill or support gem socketed into an equipped item. Mirrors
/// the `socketedItems[].…` shape returned by the GGG `get-items`
/// endpoint. PoB's reference parser is `Classes/ImportTab.lua:1281
/// — ImportSocketedItems`.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct SocketedGggItem {
    /// Inherited gem display name (often empty for non-rare gems).
    #[serde(default)]
    pub name: String,
    /// Base type — `"Fireball"`, `"Spell Echo Support"`, etc. Used
    /// to resolve the canonical skill_id via the gem registry.
    #[serde(default, rename = "typeLine")]
    pub type_line: String,
    /// Frame type — informational; the `support` flag is what we
    /// branch on.
    #[serde(default, rename = "frameType")]
    pub frame_type: u32,
    /// Index into the parent item's `sockets[]` array. Maps the
    /// gem to its linked-group id.
    #[serde(default)]
    pub socket: u32,
    /// Whether this is an Abyss jewel rather than an active/support
    /// gem. We skip those for skill-group purposes; the user can
    /// model them as items separately.
    #[serde(default, rename = "abyssJewel")]
    pub abyss_jewel: bool,
    /// Whether this is a support gem.
    #[serde(default)]
    pub support: bool,
    /// Properties — we scan for `Level` and `Quality`.
    #[serde(default)]
    pub properties: Vec<ItemProperty>,
    /// Per-gem hybrid data — used by transfigured / dual-skill gems
    /// (e.g. Stormbind). When present, `hybrid.baseTypeName` is the
    /// effective type to resolve.
    #[serde(default)]
    pub hybrid: Option<HybridGem>,
}

/// `socketedItems[i].hybrid` shape — present on transfigured /
/// dual-skill gems. PoB's `ImportSocketedItems` falls back to
/// `hybrid.baseTypeName` to look the gem up.
#[derive(Debug, Clone, Deserialize, Default)]
pub struct HybridGem {
    #[serde(default, rename = "baseTypeName")]
    pub base_type_name: String,
    #[serde(default, rename = "isVaalGem")]
    pub is_vaal_gem: bool,
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
/// response drives equipped gear. Skills inside `socketedItems`
/// land on `Character::skill_groups` with raw (unresolved) typeLines
/// as their `skill_id`. Use [`build_character_with_skills`] to
/// resolve canonical PoB skill ids via a gem-registry lookup.
pub fn build_character(
    summary: Option<&CharacterSummary>,
    passive: &PassiveSkillsResponse,
    items_resp: &ItemsResponse,
) -> Character {
    build_character_with_skills(summary, passive, items_resp, |type_line| {
        // Default lookup: fall back to a permissive identifier-style
        // transformation when no gem registry is supplied. Strip
        // spaces, drop the trailing " Support" qualifier (collapsed
        // into a `Support` prefix per PoB's data convention), and
        // hand back what we have. This is a useful approximation
        // for stat-display and round-trip but won't always match the
        // canonical id (e.g. "Spell Echo Support" should resolve to
        // "SupportSpellEcho" but this fallback yields
        // "SpellEchoSupport"). Callers with a real `GemSet` should
        // use `build_character_with_skills` and an exact lookup.
        Some(default_skill_id_from_type_line(type_line))
    })
}

/// Like [`build_character`] but threads a gem-name resolver through
/// to the skill-group construction. `gem_lookup(type_line)` should
/// return the canonical `granted_effect_id` (a.k.a. PoB skill id)
/// for the given gem typeLine, or `None` to skip the gem.
///
/// Wasm-clean — the engine doesn't load the gem registry, the
/// caller does.
pub fn build_character_with_skills<F>(
    summary: Option<&CharacterSummary>,
    passive: &PassiveSkillsResponse,
    items_resp: &ItemsResponse,
    mut gem_lookup: F,
) -> Character
where
    F: FnMut(&str) -> Option<String>,
{
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
    // doesn't block the whole import. Each item's `socketedItems`
    // also yields one `SocketGroup` per linked-group on the gear,
    // labelled with the slot for UI clarity.
    let mut skill_groups: Vec<crate::character::SocketGroup> = Vec::new();
    for ggg_item in &items_resp.items {
        let Some(slot) = ggg_slot_to_pob(&ggg_item.inventory_id, ggg_item.x) else {
            continue;
        };
        let raw = render_item_paste(ggg_item);
        if let Ok(item) = parse_item(&raw) {
            character.items.equip(slot, item);
        }
        let groups = build_socket_groups_for_item(ggg_item, slot, &mut gem_lookup);
        skill_groups.extend(groups);
    }

    // Issue #194 (slice 2): mastery selections. The GGG payload
    // encodes each pick as `mastery | (effect << 16)`. We decode
    // back to `(mastery_node_id, effect_id)` pairs and feed them
    // into `Character::mastery_selections`. Unknown shape (e.g.
    // empty `[]`) collapses to "no selections", matching PoB.
    for (mastery_node, effect_id) in decode_mastery_effects(&passive.mastery_effects) {
        character.mastery_selections.insert(mastery_node, effect_id);
    }

    // Pick a "main" socket group: the largest one wins, matching
    // PoB's `GuessMainSocketGroup`. The user can re-pick later from
    // the Skills tab; this is just a sensible default so the calc
    // engine has something to crunch immediately after import.
    if !skill_groups.is_empty() {
        let (main_idx, _) = skill_groups
            .iter()
            .enumerate()
            .max_by_key(|(_, g)| g.gems.len())
            .unwrap();
        character.main_socket_group = (main_idx + 1) as u32;
        character.skill_groups = skill_groups;
        character.sync_main_skill();
    }

    character
}

/// Issue #194 (slice 2): default `(typeLine -> skill_id)` resolver
/// when the caller has no real gem registry. Strips spaces and
/// punctuation, leaving an identifier-style id. Imperfect — see
/// [`build_character`] for the docstring caveat — but a useful
/// approximation for many vanilla active-skill gems where the
/// granted_effect_id literally matches the gem name with spaces
/// stripped (e.g. `"Fireball"` → `"Fireball"`).
pub fn default_skill_id_from_type_line(type_line: &str) -> String {
    let mut out = String::with_capacity(type_line.len());
    for ch in type_line.chars() {
        if ch.is_ascii_alphanumeric() {
            out.push(ch);
        }
    }
    out
}

/// Build one [`SocketGroup`](crate::character::SocketGroup) per
/// linked socket group on `item`. The `socketedItems[].socket`
/// index is used to bucket gems via the parent item's `sockets[]`
/// list; gems with the same `sockets[s].group` end up in the same
/// linked group. Mirrors `ImportTab.lua:1277-1356`.
fn build_socket_groups_for_item<F>(
    item: &GggItem,
    slot: Slot,
    gem_lookup: &mut F,
) -> Vec<crate::character::SocketGroup>
where
    F: FnMut(&str) -> Option<String>,
{
    use std::collections::BTreeMap;

    if item.socketed_items.is_empty() {
        return Vec::new();
    }
    // Order linked groups stably by their first-seen group id so
    // the resulting `SocketGroup` order is deterministic.
    let mut groups: BTreeMap<u32, crate::character::SocketGroup> = BTreeMap::new();
    let slot_label = ggg_slot_label_for_ui(slot, item.x);
    for gem in &item.socketed_items {
        if gem.abyss_jewel {
            continue;
        }
        // Resolve the gem's typeLine (or the hybrid baseTypeName
        // for transfigured / Vaal forms) into a skill_id.
        let type_line = gem
            .hybrid
            .as_ref()
            .map(|h| h.base_type_name.as_str())
            .filter(|s| !s.is_empty())
            .unwrap_or(gem.type_line.as_str());
        if type_line.is_empty() {
            continue;
        }
        let Some(skill_id) = gem_lookup(type_line) else {
            continue;
        };
        if skill_id.is_empty() {
            continue;
        }
        let (level, quality) = parse_gem_level_and_quality(&gem.properties);
        let group_id = item
            .sockets
            .get(gem.socket as usize)
            .map(|s| s.group)
            .unwrap_or(gem.socket);
        let group = groups
            .entry(group_id)
            .or_insert_with(|| crate::character::SocketGroup {
                label: slot_label.clone(),
                gems: Vec::new(),
                main_active_skill_index: 1,
                enabled: true,
            });
        group.gems.push(crate::skill::MainSkill {
            skill_id,
            level: level.clamp(1, 40),
            quality: quality.min(100),
            quality_id: crate::skill::QualityId::Default,
            enabled: true,
        });
    }
    // Pick `main_active_skill_index` to point at the first
    // non-support gem in each group (PoB's default). Falls back to
    // 1 if every gem is a support.
    let mut out: Vec<crate::character::SocketGroup> = groups.into_values().collect();
    for group in &mut out {
        if let Some(idx) = group
            .gems
            .iter()
            .position(|g| !is_support_skill_id(&g.skill_id))
        {
            group.main_active_skill_index = (idx + 1) as u32;
        }
    }
    out.retain(|g| !g.gems.is_empty());
    out
}

/// Heuristic — a skill_id starting with "Support" is a support gem.
/// Mirrors how PoB's `gemForBaseName` table maps `* Support`
/// typeLines to `Support*` ids.
fn is_support_skill_id(id: &str) -> bool {
    id.starts_with("Support")
}

/// Pull `Level` and `Quality` numeric values out of the gem's
/// `properties` array. PoE returns them as the first value of the
/// matching property entry. Defaults: level 20, quality 0.
fn parse_gem_level_and_quality(props: &[ItemProperty]) -> (u32, u32) {
    let mut level = 20u32;
    let mut quality = 0u32;
    for prop in props {
        let Some(s) = prop
            .values
            .first()
            .and_then(|v| v.as_array())
            .and_then(|a| a.first())
            .and_then(|s| s.as_str())
        else {
            continue;
        };
        match prop.name.as_str() {
            "Level" => {
                if let Some(n) = parse_leading_u32(s) {
                    level = n;
                }
            }
            "Quality" => {
                if let Some(n) = parse_leading_u32(s.trim_start_matches('+').trim_end_matches('%'))
                {
                    quality = n;
                }
            }
            _ => {}
        }
    }
    (level, quality)
}

fn parse_leading_u32(s: &str) -> Option<u32> {
    let mut acc = 0u32;
    let mut any = false;
    for ch in s.chars() {
        if let Some(d) = ch.to_digit(10) {
            acc = acc.saturating_mul(10).saturating_add(d);
            any = true;
        } else if any {
            break;
        }
    }
    if any {
        Some(acc)
    } else {
        None
    }
}

/// Friendly per-slot label for the SocketGroup picker. Matches the
/// PoB-XML slot label scheme so a re-export round-trips with the
/// expected value.
fn ggg_slot_label_for_ui(slot: Slot, flask_x: u32) -> String {
    match slot {
        Slot::Helmet => "Helmet".to_owned(),
        Slot::BodyArmour => "Body Armour".to_owned(),
        Slot::Gloves => "Gloves".to_owned(),
        Slot::Boots => "Boots".to_owned(),
        Slot::Amulet => "Amulet".to_owned(),
        Slot::Ring1 => "Ring 1".to_owned(),
        Slot::Ring2 => "Ring 2".to_owned(),
        Slot::Belt => "Belt".to_owned(),
        Slot::Weapon1 => "Weapon 1".to_owned(),
        Slot::Weapon2 => "Weapon 2".to_owned(),
        Slot::Weapon1Swap => "Weapon 1 Swap".to_owned(),
        Slot::Weapon2Swap => "Weapon 2 Swap".to_owned(),
        Slot::Flask1 | Slot::Flask2 | Slot::Flask3 | Slot::Flask4 | Slot::Flask5 => {
            format!("Flask {}", flask_x + 1)
        }
    }
}

/// Issue #194 (slice 2): decode the GGG `mastery_effects` payload
/// into `(mastery_node_id, effect_id)` pairs.
///
/// The GGG endpoint encodes each pick as `mastery | (effect << 16)`
/// — see PoB's `Classes/ImportTab.lua:698-708`. The wire form is
/// **either** an object (newer responses, keyed by the mastery
/// node id with a string-encoded numeric value) **or** a flat
/// numeric array (older responses where the index *is* the
/// mastery node id). We accept both shapes; anything we can't
/// crack yields an empty result so the import still succeeds.
pub fn decode_mastery_effects(value: &serde_json::Value) -> Vec<(NodeId, u32)> {
    let mut out = Vec::new();
    let parse_encoded = |encoded: u64| -> (NodeId, u32) {
        let mastery_id = (encoded & 0xFFFF) as NodeId;
        let effect_id = ((encoded >> 16) & 0xFFFF) as u32;
        (mastery_id, effect_id)
    };
    match value {
        serde_json::Value::Object(map) => {
            for (key, val) in map {
                // Two valid shapes for `val`:
                //   "1234567890" — string-encoded `mastery |
                //                  (effect << 16)`. Most common.
                //   123456789    — bare numeric.
                // The key is sometimes the mastery node id and
                // sometimes redundant — we trust the encoded
                // value's low 16 bits regardless.
                let encoded_opt = match val {
                    serde_json::Value::String(s) => s.parse::<u64>().ok(),
                    serde_json::Value::Number(n) => n.as_u64(),
                    _ => None,
                };
                if let Some(encoded) = encoded_opt {
                    let (mastery_id, effect_id) = parse_encoded(encoded);
                    if mastery_id != 0 {
                        out.push((mastery_id, effect_id));
                        continue;
                    }
                }
                // Fall back to (key, value-as-effect) if the
                // encoded form failed to decode.
                if let Ok(node_id) = key.parse::<NodeId>() {
                    let effect = val.as_u64().unwrap_or(0) as u32;
                    if effect != 0 {
                        out.push((node_id, effect));
                    }
                }
            }
        }
        serde_json::Value::Array(arr) => {
            for entry in arr {
                let encoded = match entry {
                    serde_json::Value::String(s) => s.parse::<u64>().ok(),
                    serde_json::Value::Number(n) => n.as_u64(),
                    _ => None,
                };
                if let Some(enc) = encoded {
                    let (mastery_id, effect_id) = parse_encoded(enc);
                    if mastery_id != 0 {
                        out.push((mastery_id, effect_id));
                    }
                }
            }
        }
        _ => {}
    }
    out
}

/// Issue #194 (slice 3): socket the jewels listed in `passive.items`
/// onto `character`. Each entry's `x` field indexes into
/// `tree.jewel_slots`; the resulting tree node id receives the
/// jewel. Cluster jewels (whose host socket is a Large jewel
/// socket — `expansion_jewel_size == Some(2)`) land in
/// `Character::jewels` so the cluster-synth pass picks them up;
/// other jewels (radius / abyss / timeless) land in
/// `Character::socketed_jewels`.
///
/// Returns the number of jewels socketed. Items the parser can't
/// crack (or whose `x` is out of range) are silently skipped — a
/// best-effort import is more useful than a hard fail.
pub fn apply_passive_jewels(
    character: &mut Character,
    tree: &pob_data::PassiveTree,
    passive: &PassiveSkillsResponse,
) -> usize {
    let mut count = 0;
    for ggg_item in &passive.items {
        // PoB's mapping: `tree.jewel_slots[x + 1]` (1-based). Our
        // tree stores the same list 0-based, so we use `x` directly.
        let Some(host_node) = tree.jewel_slots.get(ggg_item.x as usize).copied() else {
            continue;
        };
        let raw = render_item_paste(ggg_item);
        let Ok(item) = parse_item(&raw) else {
            continue;
        };
        let is_cluster_socket = tree
            .nodes
            .get(&host_node)
            .map(|n| n.expansion_jewel_size == Some(2))
            .unwrap_or(false);
        if is_cluster_socket {
            character.jewels.insert(host_node, item);
        } else {
            character.socketed_jewels.socket(host_node, item);
        }
        count += 1;
    }
    count
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
        // Issue #194 (slice 2): masteries from the fixture come
        // through as `(node_id, effect_id)` pairs.
        // 3289145 = 12345 + (50 << 16) → (12345, 50)
        // 5397408 = 23456 + (82 << 16) → (23456, 82)
        assert_eq!(c.mastery_selections.get(&12345).copied(), Some(50));
        assert_eq!(c.mastery_selections.get(&23456).copied(), Some(82));
    }

    #[test]
    fn decode_mastery_effects_handles_object_form() {
        let v: serde_json::Value =
            serde_json::from_str(r#"{"12345":"3289145","23456":"5397408"}"#).unwrap();
        let mut decoded = decode_mastery_effects(&v);
        decoded.sort_unstable();
        assert_eq!(decoded, vec![(12345, 50), (23456, 82)]);
    }

    #[test]
    fn decode_mastery_effects_handles_array_form() {
        let v: serde_json::Value = serde_json::from_str(r#"["3289145","5397408"]"#).unwrap();
        let mut decoded = decode_mastery_effects(&v);
        decoded.sort_unstable();
        assert_eq!(decoded, vec![(12345, 50), (23456, 82)]);
    }

    #[test]
    fn decode_mastery_effects_tolerates_empty() {
        let v: serde_json::Value = serde_json::from_str("{}").unwrap();
        assert!(decode_mastery_effects(&v).is_empty());
        let v: serde_json::Value = serde_json::from_str("[]").unwrap();
        assert!(decode_mastery_effects(&v).is_empty());
        assert!(decode_mastery_effects(&serde_json::Value::Null).is_empty());
    }

    #[test]
    fn build_character_extracts_socket_groups_from_socketed_items() {
        let raw_items = include_str!("../tests/fixtures/ggg_character_items.json");
        let items = parse_items(raw_items).unwrap();
        let c = build_character(None, &PassiveSkillsResponse::default(), &items);
        // BodyArmour has Fireball + Spell Echo Support in one
        // linked group → one SocketGroup, two gems.
        assert_eq!(c.skill_groups.len(), 1);
        let group = &c.skill_groups[0];
        assert_eq!(group.gems.len(), 2);
        // Default lookup: typeLine with spaces stripped.
        assert_eq!(group.gems[0].skill_id, "Fireball");
        assert_eq!(group.gems[1].skill_id, "SpellEchoSupport");
        assert_eq!(group.gems[0].level, 20);
        assert_eq!(group.gems[0].quality, 20);
        // main_active_skill_index points at Fireball (the
        // non-support gem) — index 1 (1-based).
        assert_eq!(group.main_active_skill_index, 1);
        assert_eq!(c.main_socket_group, 1);
        assert_eq!(c.main_skill.as_ref().unwrap().skill_id, "Fireball");
    }

    #[test]
    fn build_character_with_skills_uses_custom_lookup() {
        let raw_items = include_str!("../tests/fixtures/ggg_character_items.json");
        let items = parse_items(raw_items).unwrap();
        let c = build_character_with_skills(
            None,
            &PassiveSkillsResponse::default(),
            &items,
            |type_line| {
                // Pretend we have a real gem registry that maps
                // `Spell Echo Support` to its canonical id.
                Some(match type_line {
                    "Fireball" => "Fireball".to_owned(),
                    "Spell Echo Support" => "SupportSpellEcho".to_owned(),
                    _ => return None,
                })
            },
        );
        let group = c
            .skill_groups
            .first()
            .expect("BodyArmour socket group built");
        assert_eq!(group.gems[0].skill_id, "Fireball");
        assert_eq!(group.gems[1].skill_id, "SupportSpellEcho");
    }

    #[test]
    fn build_socket_groups_returns_empty_when_no_socketed_items() {
        let json = r#"{
            "items": [
                {"typeLine":"Onyx Amulet","frameType":2,"inventoryId":"Amulet"}
            ]
        }"#;
        let items = parse_items(json).unwrap();
        let c = build_character(None, &PassiveSkillsResponse::default(), &items);
        assert!(c.skill_groups.is_empty());
    }

    #[test]
    fn apply_passive_jewels_routes_cluster_jewels_to_character_jewels() {
        // Synthesise a tiny tree with two jewel slots: one Large
        // (cluster) socket + one regular socket. Then run a
        // PassiveSkillsResponse with one cluster jewel + one
        // ordinary jewel and verify routing.
        use ahash::HashMapExt;
        use pob_data::{Group, Node, NodeKind, PassiveTree};
        use smallvec::SmallVec;

        let mut nodes = ahash::HashMap::new();
        nodes.insert(
            1000,
            Node {
                id: 1000,
                name: Some("Large Jewel Socket".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::JewelSocket,
                class_start_index: None,
                group: Some(7),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: SmallVec::new(),
                in_edges: SmallVec::new(),
                mastery_effects: vec![],
                expansion_jewel_size: Some(2),
                jewel_radius: None,
            },
        );
        nodes.insert(
            2000,
            Node {
                id: 2000,
                name: Some("Jewel Socket".into()),
                icon: None,
                ascendancy_name: None,
                stats: vec![],
                reminder_text: vec![],
                kind: NodeKind::JewelSocket,
                class_start_index: None,
                group: Some(8),
                orbit: Some(0),
                orbit_index: Some(0),
                out_edges: SmallVec::new(),
                in_edges: SmallVec::new(),
                mastery_effects: vec![],
                expansion_jewel_size: None,
                jewel_radius: Some(2),
            },
        );
        let mut groups = ahash::HashMap::new();
        groups.insert(
            7,
            Group {
                x: 0.0,
                y: 0.0,
                orbits: SmallVec::new(),
                background: None,
                nodes: vec![1000],
                is_proxy: false,
            },
        );
        groups.insert(
            8,
            Group {
                x: 100.0,
                y: 0.0,
                orbits: SmallVec::new(),
                background: None,
                nodes: vec![2000],
                is_proxy: false,
            },
        );
        let tree = PassiveTree {
            version: "test".into(),
            tree: "Default".into(),
            classes: vec![],
            groups,
            nodes,
            jewel_slots: vec![1000, 2000],
            min_x: 0,
            min_y: 0,
            max_x: 200,
            max_y: 200,
            constants: pob_data::TreeConstants {
                skills_per_orbit: vec![1, 6, 16, 16, 40, 72, 72],
                orbit_radii: vec![0, 82, 162, 335, 493, 662, 846],
                classes: ahash::HashMap::default(),
                character_attributes: ahash::HashMap::default(),
                pss_centre_inner_radius: None,
            },
            points: pob_data::TreePoints::default(),
        };

        let passive_json = r#"{
            "hashes": [],
            "items": [
                {
                    "name": "",
                    "typeLine": "Large Cluster Jewel",
                    "frameType": 1,
                    "inventoryId": "PassiveJewels",
                    "x": 0,
                    "ilvl": 84,
                    "explicitMods": ["Adds 8 Passive Skills"]
                },
                {
                    "name": "",
                    "typeLine": "Crimson Jewel",
                    "frameType": 1,
                    "inventoryId": "PassiveJewels",
                    "x": 1,
                    "ilvl": 84,
                    "explicitMods": ["+5% increased Attack Damage"]
                }
            ]
        }"#;
        let passive = parse_passive_skills(passive_json).unwrap();
        let mut character = Character::new(ClassRef::witch(), 90);
        let count = apply_passive_jewels(&mut character, &tree, &passive);
        assert_eq!(count, 2);
        assert!(
            character.jewels.contains_key(&1000),
            "cluster jewel should land on Large socket 1000"
        );
        assert_eq!(character.socketed_jewels.len(), 1);
        assert!(
            character.socketed_jewels.get(2000).is_some(),
            "ordinary jewel should land on regular socket 2000"
        );
    }

    #[test]
    fn default_skill_id_strips_spaces_and_punctuation() {
        assert_eq!(default_skill_id_from_type_line("Fireball"), "Fireball");
        assert_eq!(
            default_skill_id_from_type_line("Spell Echo Support"),
            "SpellEchoSupport"
        );
        assert_eq!(
            default_skill_id_from_type_line("Cast on Critical Strike Support"),
            "CastonCriticalStrikeSupport"
        );
        assert_eq!(
            default_skill_id_from_type_line("Lightning Arrow"),
            "LightningArrow"
        );
    }
}
