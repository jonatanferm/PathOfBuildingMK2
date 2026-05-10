//! Import a build saved or shared from upstream Path of Building Community.
//!
//! Two entry points:
//! - [`import_pob_xml`] — parse an XML document directly. Use this when you've already
//!   read a `.xml` build file off disk.
//! - [`import_pob_code`] — decode a `xnd…`-style PoB share code (zlib-deflate of XML,
//!   base64-encoded). Use this when the user pastes a `pobb.in` or pob.cool string.
//!
//! Phase 5 minimum: parse class, ascendancy, level, allocated nodes from the active spec,
//! and notes. Items, skills, and config require more involved parsers — they're tracked
//! in `docs/divergences.md` as the next chunk.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine as _;
use flate2::read::ZlibDecoder;
use std::io::Read;
use std::str;

use quick_xml::events::Event;
use quick_xml::reader::Reader;

use crate::character::{Character, ClassRef};
use crate::item_parser::parse_item;
use crate::skill::MainSkill;
use pob_data::{NodeId, Slot};

#[derive(Debug)]
pub enum PobImportError {
    Decode(String),
    Xml(String),
    NotPob,
}

impl std::fmt::Display for PobImportError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::Decode(e) => write!(f, "decode failed: {e}"),
            Self::Xml(e) => write!(f, "xml parse failed: {e}"),
            Self::NotPob => write!(f, "input is not a PathOfBuilding XML"),
        }
    }
}

impl std::error::Error for PobImportError {}

/// Issue #33: map a build-share URL to the raw-endpoint URL the
/// existing share-code path can decode. Returns `None` for non-URL
/// inputs (so the caller's existing "decode as code" path still
/// runs unchanged) or for hosts we don't recognise.
///
/// Mirrors upstream PoB's `Classes/BuildSiteTools.lua` host table.
/// The HTTP fetch itself is intentionally out of scope here — this
/// is the pure URL-recognition + endpoint-rewrite half. The host UI
/// or a fetcher slice can wire `reqwest` / `ureq` on top (see
/// `pob-ui/src/share_url_fetch.rs`).
///
/// Recognised hosts (case-insensitive on scheme + domain):
/// - `pobb.in/<id>`         → `https://pobb.in/<id>/raw`
/// - `pob.cool/<id>`        → `https://pob.cool/<id>/raw`
/// - `pastebin.com/<id>`    → `https://pastebin.com/raw/<id>`
/// - `pastebin.com/raw/<id>`→ idempotent (returned as-is, normalised)
/// - `poe.ninja/pob/<id>`   → `https://poe.ninja/pob/<id>/raw`
/// - `poeplanner.com/<id>`  → `https://poeplanner.com/api/build/<id>/pob`
///
/// IDs are constrained to ASCII alphanumerics (plus `-` and `_`) so
/// a stray path segment (`pobb.in/about`) doesn't accidentally
/// re-target.
#[must_use]
pub fn resolve_share_url(input: &str) -> Option<String> {
    let trimmed = input.trim();
    if !(trimmed.starts_with("http://") || trimmed.starts_with("https://")) {
        return None;
    }
    // Reject any internal whitespace — a real URL won't contain
    // unescaped spaces, and `split_whitespace().next()` would
    // otherwise silently truncate `https://pobb.in/has spaces`
    // into `https://pobb.in/has` and accept it.
    if trimmed.chars().any(char::is_whitespace) {
        return None;
    }
    // Strip the scheme so the host/path matcher is uniform.
    let rest = trimmed
        .strip_prefix("https://")
        .or_else(|| trimmed.strip_prefix("http://"))?;
    // Drop fragment and query.
    let rest = rest.split('#').next()?;
    let rest = rest.split('?').next()?;
    // Tolerate a `www.` prefix on any of the supported hosts.
    let rest = rest.strip_prefix("www.").unwrap_or(rest);
    let (host, path) = rest.split_once('/')?;
    let path = path.trim_matches('/');
    if path.is_empty() {
        return None;
    }
    let host = host.to_ascii_lowercase();

    // Multi-segment path hosts (poe.ninja, pastebin's already-resolved
    // /raw form). Match these *before* the single-segment id check so
    // the strict path validators below don't reject legal inputs.
    match host.as_str() {
        "poe.ninja" => {
            // poe.ninja serves PoB builds at /pob/<id> with a /raw
            // sibling. The slug is alphanumeric.
            let rest = path.strip_prefix("pob/")?;
            // Strip trailing /raw if the user already pasted the raw URL.
            let id = rest.strip_suffix("/raw").unwrap_or(rest);
            if !is_share_id(id) {
                return None;
            }
            return Some(format!("https://poe.ninja/pob/{id}/raw"));
        }
        "pastebin.com" => {
            // Accept already-resolved /raw/<id> idempotently as well
            // as the canonical /<id> form.
            let id = path.strip_prefix("raw/").unwrap_or(path);
            if !is_share_id(id) || id.contains('/') {
                return None;
            }
            return Some(format!("https://pastebin.com/raw/{id}"));
        }
        _ => {}
    }

    // Single-segment hosts. Treat the entire path as the id; reject
    // anything with internal slashes to avoid clobbering /about-style
    // pages.
    let id = path.strip_suffix("/raw").unwrap_or(path);
    if id.contains('/') {
        return None;
    }
    if !is_share_id(id) {
        return None;
    }
    match host.as_str() {
        "pobb.in" => Some(format!("https://pobb.in/{id}/raw")),
        "pob.cool" => Some(format!("https://pob.cool/{id}/raw")),
        "poeplanner.com" => Some(format!("https://poeplanner.com/api/build/{id}/pob")),
        _ => None,
    }
}

/// Validate a build-share id. We accept ASCII alphanumerics plus
/// `-` and `_` so URL-safe base62-style ids round-trip; everything
/// else (dots, spaces, slashes) is rejected so a stray /about-style
/// path segment can't masquerade as an id.
fn is_share_id(id: &str) -> bool {
    !id.is_empty()
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

pub fn import_pob_code(code: &str) -> Result<Character, PobImportError> {
    // PoB shares use both `+/=` (standard base64) and `-_` (url-safe). Try url-safe first.
    let stripped = code.trim();
    let raw = decode_loose_base64(stripped).ok_or_else(|| {
        PobImportError::Decode("input did not decode as base64 (any variant)".into())
    })?;
    // Decompress
    let mut dec = ZlibDecoder::new(raw.as_slice());
    let mut xml_bytes = Vec::new();
    dec.read_to_end(&mut xml_bytes)
        .map_err(|e| PobImportError::Decode(format!("zlib: {e}")))?;
    let xml =
        String::from_utf8(xml_bytes).map_err(|e| PobImportError::Decode(format!("utf-8: {e}")))?;
    import_pob_xml(&xml)
}

fn decode_loose_base64(s: &str) -> Option<Vec<u8>> {
    let bytes = s.as_bytes();
    if let Ok(v) = URL_SAFE_NO_PAD.decode(bytes) {
        return Some(v);
    }
    if let Ok(v) = base64::engine::general_purpose::URL_SAFE.decode(bytes) {
        return Some(v);
    }
    if let Ok(v) = base64::engine::general_purpose::STANDARD_NO_PAD.decode(bytes) {
        return Some(v);
    }
    if let Ok(v) = base64::engine::general_purpose::STANDARD.decode(bytes) {
        return Some(v);
    }
    None
}

pub fn import_pob_xml(xml: &str) -> Result<Character, PobImportError> {
    let mut reader = Reader::from_str(xml);
    reader.config_mut().trim_text(true);

    let mut character = Character::new(ClassRef::scion(), 1);
    let mut found_root = false;
    let mut depth_stack: Vec<String> = Vec::new();
    let mut buf = Vec::new();
    let mut active_spec_pending: Option<Vec<NodeId>> = None;
    let mut active_spec_class: Option<String> = None;
    let mut active_spec_ascend: Option<String> = None;
    let mut notes_collect = String::new();
    let mut in_notes = false;

    // Items / skills / config require multi-element traversal. We collect raw
    // bodies + attributes here and reconcile after the parse loop finishes.
    // Items: id → ItemSpec (raw paste body + variant info)
    let mut items_by_id: std::collections::HashMap<u32, ItemSpec> =
        std::collections::HashMap::new();
    let mut current_item_id: Option<u32> = None;
    let mut current_item_body = String::new();
    // ItemSet → slot bindings. Issue #90: PoB stores multiple
    // `<ItemSet>` blocks, each pointing back at items by id. We capture
    // every set so saved-loadout names round-trip; the slot map for the
    // *active* set drives `character.items`.
    //
    // Issue #195: jewel sockets land as `<Slot name="Jewel <NodeId>" itemId="…"/>`
    // alongside the usual gear slots. We capture them on a parallel
    // `JewelMap` keyed by the parsed node id; only the active set's
    // jewels are materialised onto `character.{jewels, socketed_jewels}`
    // since those collections are build-level in MK2 (per-set jewel
    // sockets aren't surfaced yet).
    type SlotMap = std::collections::HashMap<String, u32>;
    type JewelMap = std::collections::HashMap<NodeId, u32>;
    let mut item_sets: Vec<(u32, Option<String>, SlotMap, JewelMap)> = Vec::new();
    let mut active_item_set_id: Option<u32> = None;
    let mut current_item_set: Option<(u32, Option<String>, SlotMap, JewelMap)> = None;
    // Skills: capture all skill groups; track which has mainActiveSkill.
    let mut skill_groups: Vec<SkillGroup> = Vec::new();
    let mut current_skill_group: Option<SkillGroup> = None;
    let mut main_socket_group: Option<u32> = None;
    let mut socket_group_index: u32 = 0;
    // Config: name/value pairs.
    let mut in_config = false;
    // Issue #98: tattoos. PoB stores them as `<Spec> <Overrides> <Override
    // nodeId="…">mod text</Override> </Overrides> </Spec>`. We accumulate
    // (nodeId, body) pairs here and write them onto `character.tattoo_overrides`
    // after the parse loop, ignoring blocks under non-active specs.
    let mut current_override_node_id: Option<u32> = None;
    let mut current_override_body = String::new();
    let mut tattoo_overrides_pending: std::collections::HashMap<u32, String> =
        std::collections::HashMap::new();

    loop {
        match reader.read_event_into(&mut buf) {
            Ok(Event::Eof) => break,
            Ok(Event::Start(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                handle_start_attrs(
                    &name,
                    &e,
                    &mut character,
                    &mut active_spec_pending,
                    &mut active_spec_class,
                    &mut active_spec_ascend,
                    &mut found_root,
                    &mut main_socket_group,
                    &mut active_item_set_id,
                )?;
                match name.as_str() {
                    "Notes" => {
                        in_notes = true;
                        notes_collect.clear();
                    }
                    "Item" => {
                        // <Item id="N" variant="...">raw text</Item>
                        let id = attr_str(&e, "id").and_then(|s| s.parse::<u32>().ok());
                        if let Some(id) = id {
                            current_item_id = Some(id);
                            current_item_body.clear();
                        }
                    }
                    "Skill" => {
                        // <Skill mainActiveSkill="1" enabled="true" ...>
                        socket_group_index += 1;
                        let main_idx = attr_str(&e, "mainActiveSkill")
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or(0);
                        let enabled = attr_str(&e, "enabled").is_none_or(|s| s != "false");
                        current_skill_group = Some(SkillGroup {
                            index: socket_group_index,
                            main_active_skill_index: main_idx,
                            enabled,
                            gems: Vec::new(),
                        });
                    }
                    "ItemSet" => {
                        // Issue #90: capture every <ItemSet> so multi-loadout
                        // builds round-trip. Each block carries an id, an
                        // optional human-readable title, and a slot→itemId
                        // map; we collect them all and reconcile after the
                        // parse loop using `active_item_set_id`.
                        let id = attr_str(&e, "id")
                            .and_then(|s| s.parse::<u32>().ok())
                            .unwrap_or((item_sets.len() + 1) as u32);
                        let title = attr_str(&e, "title").filter(|s| !s.is_empty());
                        current_item_set = Some((id, title, SlotMap::new(), JewelMap::new()));
                        // Issue #109: PoB writes `useSecondWeaponSet`
                        // on the per-set element; mirror it onto the
                        // build-level toggle so a swap-on build
                        // imports correctly even when the parent
                        // `<Items>` element doesn't carry the flag.
                        if let Some(v) = attr_str(&e, "useSecondWeaponSet") {
                            character.config.use_second_weapon_set = v == "true";
                        }
                    }
                    "Config" => in_config = true,
                    "Override" => {
                        // <Override nodeId="…" icon="…" activeEffectImage="…"
                        // dn="…">mod text lines</Override>. We only need the
                        // nodeId attribute and the body text for the calc
                        // pipeline; metadata fields are display-only.
                        current_override_node_id =
                            attr_str(&e, "nodeId").and_then(|s| s.parse::<u32>().ok());
                        current_override_body.clear();
                    }
                    _ => {}
                }
                depth_stack.push(name);
            }
            Ok(Event::Empty(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                handle_start_attrs(
                    &name,
                    &e,
                    &mut character,
                    &mut active_spec_pending,
                    &mut active_spec_class,
                    &mut active_spec_ascend,
                    &mut found_root,
                    &mut main_socket_group,
                    &mut active_item_set_id,
                )?;
                match name.as_str() {
                    "Slot" => {
                        // ItemSet → Slot mapping. PoB nests Slot inside
                        // ItemSet (current schema); some legacy files place
                        // Slot directly inside Items, in which case we
                        // synthesise a default set on first sight.
                        //
                        // Issue #195: PoB writes jewel sockets as
                        // `<Slot name="Jewel <NodeId>" itemId="…"/>` (where
                        // `<NodeId>` is the tree-socket node id, e.g.
                        // `Jewel 32763`). They land in the per-set
                        // `JewelMap` and get materialised onto
                        // `character.{jewels, socketed_jewels}` after
                        // items_by_id is fully populated.
                        let slot_name = attr_str(&e, "name").unwrap_or_default();
                        let item_id = attr_str(&e, "itemId").and_then(|s| s.parse::<u32>().ok());
                        if let (false, Some(id)) = (slot_name.is_empty(), item_id) {
                            if id > 0 {
                                let jewel_node_id = parse_jewel_slot_name(&slot_name);
                                if let Some((_, _, ref mut slots, ref mut jewels)) =
                                    current_item_set
                                {
                                    if let Some(node_id) = jewel_node_id {
                                        jewels.insert(node_id, id);
                                    } else {
                                        slots.insert(slot_name, id);
                                    }
                                } else if item_sets.is_empty() {
                                    // Legacy: <Slot> outside any <ItemSet>.
                                    let mut slots = SlotMap::new();
                                    let mut jewels = JewelMap::new();
                                    if let Some(node_id) = jewel_node_id {
                                        jewels.insert(node_id, id);
                                    } else {
                                        slots.insert(slot_name, id);
                                    }
                                    item_sets.push((1, None, slots, jewels));
                                }
                            }
                        }
                    }
                    "Gem" => {
                        if let Some(group) = current_skill_group.as_mut() {
                            group.gems.push(GemSpec {
                                skill_id: attr_str(&e, "skillId").unwrap_or_default(),
                                level: attr_str(&e, "level")
                                    .and_then(|s| s.parse::<u32>().ok())
                                    .unwrap_or(20),
                                quality: attr_str(&e, "quality")
                                    .and_then(|s| s.parse::<u32>().ok())
                                    .unwrap_or(0),
                                quality_id: attr_str(&e, "qualityId")
                                    .as_deref()
                                    .map(crate::skill::QualityId::from_pob_name)
                                    .unwrap_or_default(),
                                enabled: attr_str(&e, "enabled").is_none_or(|s| s != "false"),
                            });
                        }
                    }
                    "Input" if in_config => {
                        // <Input name="..." string="..."/> or boolean="true" or number="N"
                        let name = attr_str(&e, "name").unwrap_or_default();
                        if name.is_empty() {
                            continue;
                        }
                        if let Some(s) = attr_str(&e, "string") {
                            apply_config_string(&mut character, &name, &s);
                        } else if let Some(b) = attr_str(&e, "boolean") {
                            character.config.conditions.insert(name, b == "true");
                        } else if let Some(n) = attr_str(&e, "number") {
                            if let Ok(num) = n.parse::<f64>() {
                                apply_config_number(&mut character, &name, num);
                            }
                        }
                    }
                    _ => {}
                }
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                match name.as_str() {
                    "Notes" => {
                        in_notes = false;
                        character.notes = std::mem::take(&mut notes_collect);
                    }
                    "Item" => {
                        if let Some(id) = current_item_id.take() {
                            items_by_id.insert(
                                id,
                                ItemSpec {
                                    raw: std::mem::take(&mut current_item_body),
                                },
                            );
                        }
                    }
                    "Skill" => {
                        if let Some(g) = current_skill_group.take() {
                            skill_groups.push(g);
                        }
                    }
                    "ItemSet" => {
                        if let Some(set) = current_item_set.take() {
                            item_sets.push(set);
                        }
                    }
                    "Config" => in_config = false,
                    "Override" => {
                        if let Some(node_id) = current_override_node_id.take() {
                            let body = std::mem::take(&mut current_override_body);
                            tattoo_overrides_pending.insert(node_id, body);
                        }
                    }
                    _ => {}
                }
                depth_stack.pop();
            }
            Ok(Event::Text(t)) => {
                if in_notes {
                    if let Ok(s) = t.unescape() {
                        notes_collect.push_str(&s);
                    }
                } else if current_override_node_id.is_some() {
                    if let Ok(s) = t.unescape() {
                        if !current_override_body.is_empty() {
                            current_override_body.push('\n');
                        }
                        current_override_body.push_str(&s);
                    }
                } else if current_item_id.is_some() {
                    if let Ok(s) = t.unescape() {
                        if !current_item_body.is_empty() {
                            current_item_body.push('\n');
                        }
                        current_item_body.push_str(&s);
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if in_notes {
                    notes_collect.push_str(&String::from_utf8_lossy(&t));
                } else if current_item_id.is_some() {
                    if !current_item_body.is_empty() {
                        current_item_body.push('\n');
                    }
                    current_item_body.push_str(&String::from_utf8_lossy(&t));
                }
            }
            Err(e) => return Err(PobImportError::Xml(e.to_string())),
            _ => {}
        }
        buf.clear();
    }

    if !found_root {
        return Err(PobImportError::NotPob);
    }
    let _ = depth_stack;

    if let Some(nodes) = active_spec_pending {
        character.allocated = nodes.into_iter().collect();
    }
    // Issue #98: install captured tattoo overrides. PoB strips the
    // outer XML escaping when reading; we round-trip the body verbatim
    // so multi-line mod text survives. Empty entries are dropped.
    if !tattoo_overrides_pending.is_empty() {
        for (node_id, body) in tattoo_overrides_pending {
            let trimmed = body.trim();
            if !trimmed.is_empty() {
                character
                    .tattoo_overrides
                    .insert(node_id, trimmed.to_owned());
            }
        }
    }
    // Spec-level class attribute is sometimes a name (`className`) and sometimes a
    // numeric class id (`classId`). Only override the Build-level value when the spec
    // gives a non-numeric name, since the numeric id requires a tree-version-keyed
    // lookup we don't bother with for Phase 5.
    if let Some(c) = active_spec_class.filter(|s| !s.is_empty() && !is_numeric(s)) {
        character.class = ClassRef(c);
    }
    if let Some(a) = active_spec_ascend.filter(|s| !s.is_empty() && s != "None" && !is_numeric(s)) {
        character.ascendancy = Some(a);
    }

    // Issue #90: round-trip every <ItemSet>. Build an ItemSet for each
    // captured (id, title, slot→itemId) entry, then install the active
    // one onto `character.items` and the rest under `character.item_sets`
    // (preserving title for display). Parse failures swallow per-slot —
    // exotic items the parser doesn't handle yet shouldn't block import.
    let materialise_set = |slots: &SlotMap| -> pob_data::ItemSet {
        let mut set = pob_data::ItemSet::default();
        for (slot_name, item_id) in slots {
            let Some(slot) = pob_slot_from_name(slot_name) else {
                continue;
            };
            let Some(spec) = items_by_id.get(item_id) else {
                continue;
            };
            if let Ok(item) = parse_item(spec.raw.trim()) {
                set.equip(slot, item);
            }
        }
        set
    };

    if !item_sets.is_empty() {
        // Pick the active set: prefer the one PoB tagged via
        // `activeItemSet`, else fall back to id == 1, else the first one
        // we saw. The remaining sets become saved loadouts.
        let active_id = active_item_set_id
            .or_else(|| {
                item_sets
                    .iter()
                    .find(|(id, _, _, _)| *id == 1)
                    .map(|(id, _, _, _)| *id)
            })
            .or_else(|| item_sets.first().map(|(id, _, _, _)| *id));
        let mut saved: Vec<crate::character::NamedItemSet> = Vec::new();
        for (idx, (id, title, slots, jewels)) in item_sets.iter().enumerate() {
            let materialised = materialise_set(slots);
            if Some(*id) == active_id {
                character.items = materialised;
                // Issue #195: materialise jewel sockets onto the
                // build-level collections only for the active set.
                // Per-set jewel sockets aren't surfaced in MK2's UI;
                // saved loadouts that ship their own jewels would need
                // a parallel `NamedItemSet::jewels` field, which is
                // tracked separately.
                materialise_jewels(jewels, &items_by_id, &mut character);
            } else {
                let display = title.clone().unwrap_or_else(|| {
                    // PoB doesn't always emit titles; default to a stable
                    // ordinal so the UI still surfaces every set.
                    format!("Set {}", idx + 1)
                });
                saved.push(crate::character::NamedItemSet {
                    name: display,
                    items: materialised,
                });
            }
        }
        character.item_sets = saved;
    }

    // Pick the main skill: prefer the explicit mainActiveSkill within
    // mainSocketGroup, otherwise fall back to the first enabled gem in the
    // first enabled group.
    let main_group_idx = main_socket_group.unwrap_or(1);
    let main_group = skill_groups
        .iter()
        .find(|g| g.index == main_group_idx)
        .or_else(|| {
            skill_groups
                .iter()
                .find(|g| g.enabled && !g.gems.is_empty())
        });
    if let Some(group) = main_group {
        let gem_idx = if group.main_active_skill_index >= 1 {
            (group.main_active_skill_index as usize).saturating_sub(1)
        } else {
            0
        };
        let gem = group.gems.get(gem_idx).or_else(|| group.gems.first());
        if let Some(gem) = gem {
            if !gem.skill_id.is_empty() {
                let mut ms = MainSkill::new(gem.skill_id.clone());
                ms.level = gem.level.clamp(1, 40);
                ms.quality = gem.quality.min(100);
                ms.quality_id = gem.quality_id;
                character.main_skill = Some(ms);
            }
        }
    }
    // Persist all skill groups so the UI can render the multi-gem layout
    // and let the user toggle the main skill / disable groups.
    character.main_socket_group = main_group_idx;
    character.skill_groups = skill_groups
        .into_iter()
        .map(|g| crate::character::SocketGroup {
            label: String::new(),
            enabled: g.enabled,
            main_active_skill_index: g.main_active_skill_index.max(1),
            gems: g
                .gems
                .into_iter()
                .map(|gem| {
                    let mut ms = MainSkill::new(gem.skill_id);
                    ms.level = gem.level.clamp(1, 40);
                    ms.quality = gem.quality.min(100);
                    ms.quality_id = gem.quality_id;
                    ms.enabled = gem.enabled;
                    ms
                })
                .collect(),
        })
        .collect();

    Ok(character)
}

#[derive(Debug)]
struct ItemSpec {
    raw: String,
}

#[derive(Debug)]
struct SkillGroup {
    index: u32,
    main_active_skill_index: u32,
    enabled: bool,
    gems: Vec<GemSpec>,
}

#[derive(Debug)]
struct GemSpec {
    skill_id: String,
    level: u32,
    quality: u32,
    /// Issue #36: alt-quality variant from `<Gem qualityId="…"/>`.
    /// Defaults to `Default` for legacy PoB exports without the
    /// attribute.
    quality_id: crate::skill::QualityId,
    /// PoB persists the toggle but pob-engine doesn't yet differentiate disabled
    /// gems from absent ones. Kept on the parsed shape so callers that read this
    /// later don't need a re-import.
    #[allow(dead_code)]
    enabled: bool,
}

fn attr_str(e: &quick_xml::events::BytesStart<'_>, key: &str) -> Option<String> {
    for attr in e.attributes().with_checks(false).flatten() {
        let k = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
        if k == key {
            if let Ok(v) = attr.unescape_value() {
                return Some(v.into_owned());
            }
        }
    }
    None
}

pub(crate) fn pob_slot_from_name(name: &str) -> Option<Slot> {
    // PoB slot names: "Helmet", "Body Armour", "Gloves", "Boots", "Amulet",
    // "Ring 1", "Ring 2", "Belt", "Weapon 1", "Weapon 2", "Flask 1".."Flask 5".
    Some(match name {
        "Helmet" | "Helm" => Slot::Helmet,
        "Body Armour" | "BodyArmour" => Slot::BodyArmour,
        "Gloves" => Slot::Gloves,
        "Boots" => Slot::Boots,
        "Amulet" => Slot::Amulet,
        "Ring 1" | "Ring1" => Slot::Ring1,
        "Ring 2" | "Ring2" => Slot::Ring2,
        "Belt" => Slot::Belt,
        "Weapon 1" | "Weapon1" | "Weapon" => Slot::Weapon1,
        "Weapon 2" | "Weapon2" | "Off-hand" => Slot::Weapon2,
        // Issue #109: swap-set weapon slots. PoB writes them as
        // "Weapon 1 Swap" / "Weapon 2 Swap" inside `<ItemSet>`.
        "Weapon 1 Swap" | "Weapon1Swap" => Slot::Weapon1Swap,
        "Weapon 2 Swap" | "Weapon2Swap" => Slot::Weapon2Swap,
        "Flask 1" | "Flask1" => Slot::Flask1,
        "Flask 2" | "Flask2" => Slot::Flask2,
        "Flask 3" | "Flask3" => Slot::Flask3,
        "Flask 4" | "Flask4" => Slot::Flask4,
        "Flask 5" | "Flask5" => Slot::Flask5,
        _ => return None,
    })
}

/// Issue #195: PoB writes jewel sockets as `<Slot name="Jewel <NodeId>" itemId="…"/>`.
/// `<NodeId>` is the literal tree-socket node id (e.g. `Jewel 32763`). Returns the
/// parsed node id when the name matches, `None` otherwise.
pub(crate) fn parse_jewel_slot_name(name: &str) -> Option<NodeId> {
    let rest = name.strip_prefix("Jewel ")?;
    rest.parse::<NodeId>().ok()
}

/// Issue #195: install captured jewel-slot bindings onto `character.{jewels,
/// socketed_jewels}`. The two collections are split by item base name —
/// cluster jewels route through `cluster_synth` (`character.jewels`) while
/// vanilla / radius / timeless / abyss jewels route through `apply_radius_jewels`
/// (`character.socketed_jewels`). Items the parser can't handle are dropped
/// silently, mirroring the regular gear-slot materialisation policy.
fn materialise_jewels(
    jewels: &std::collections::HashMap<NodeId, u32>,
    items_by_id: &std::collections::HashMap<u32, ItemSpec>,
    character: &mut Character,
) {
    for (node_id, item_id) in jewels {
        let Some(spec) = items_by_id.get(item_id) else {
            continue;
        };
        let Ok(item) = parse_item(spec.raw.trim()) else {
            continue;
        };
        if item.base_name.ends_with("Cluster Jewel") {
            character.jewels.insert(*node_id, item);
        } else {
            character.socketed_jewels.socket(*node_id, item);
        }
    }
}

/// Inverse of `pob_slot_from_name`: the canonical PoB-XML slot label for a `Slot`.
/// Used by `pob_export` so import / export agree on the wire format.
pub(crate) fn pob_slot_to_name(slot: Slot) -> &'static str {
    match slot {
        Slot::Helmet => "Helmet",
        Slot::BodyArmour => "Body Armour",
        Slot::Gloves => "Gloves",
        Slot::Boots => "Boots",
        Slot::Amulet => "Amulet",
        Slot::Ring1 => "Ring 1",
        Slot::Ring2 => "Ring 2",
        Slot::Belt => "Belt",
        Slot::Weapon1 => "Weapon 1",
        Slot::Weapon2 => "Weapon 2",
        Slot::Weapon1Swap => "Weapon 1 Swap",
        Slot::Weapon2Swap => "Weapon 2 Swap",
        Slot::Flask1 => "Flask 1",
        Slot::Flask2 => "Flask 2",
        Slot::Flask3 => "Flask 3",
        Slot::Flask4 => "Flask 4",
        Slot::Flask5 => "Flask 5",
    }
}

fn apply_config_string(c: &mut Character, name: &str, value: &str) {
    // Common Config Input string keys map to ConfigState booleans / multipliers.
    // Anything we don't recognise is preserved as a condition flag (presence-only).
    match name {
        "enemyIsBoss" => {
            // "None" / "Boss" / "Pinnacle" / "Uber" — we don't model the variants
            // yet but a non-None value flips a condition.
            c.config
                .conditions
                .insert("EnemyIsBoss".to_owned(), value != "None");
        }
        _ => {
            // Generic catch-all so per-skill string toggles persist as conditions.
            c.config
                .conditions
                .insert(format!("Cfg:{name}"), !value.is_empty() && value != "None");
        }
    }
}

fn apply_config_number(c: &mut Character, name: &str, value: f64) {
    match name {
        "enemyLevel" => c.config.enemy_level = value as u32,
        "enemyFireResist" => c.config.enemy_fire_resist = value as i32,
        "enemyColdResist" => c.config.enemy_cold_resist = value as i32,
        "enemyLightningResist" => c.config.enemy_lightning_resist = value as i32,
        "enemyChaosResist" => c.config.enemy_chaos_resist = value as i32,
        "enemyEvasion" => c.config.enemy_evasion = (value as i32).max(0) as u32,
        "enemyArmour" => c.config.enemy_armour = value as u32,
        "enemyBlockChance" => {
            c.config.enemy_block_chance = (value as i32).clamp(0, 75) as u32;
        }
        "enemyDodgeChance" => {
            c.config.enemy_dodge_chance = (value as i32).clamp(0, 75) as u32;
        }
        "enemySuppressionChance" => {
            c.config.enemy_suppression_chance = (value as i32).clamp(0, 100) as u32;
        }
        "projectileNumberHitting" | "projectilesHitTarget" => {
            c.config.projectiles_hitting_target = (value as i32).max(0) as u32;
        }
        "powerCharges" => {
            c.config.multipliers.insert("PowerCharge".into(), value);
        }
        "frenzyCharges" => {
            c.config.multipliers.insert("FrenzyCharge".into(), value);
        }
        "enduranceCharges" => {
            c.config.multipliers.insert("EnduranceCharge".into(), value);
        }
        _ => {
            c.config.multipliers.insert(format!("Cfg:{name}"), value);
        }
    }
}

fn is_numeric(s: &str) -> bool {
    !s.is_empty() && s.chars().all(|c| c.is_ascii_digit())
}

fn handle_start_attrs(
    name: &str,
    e: &quick_xml::events::BytesStart<'_>,
    character: &mut Character,
    active_spec_pending: &mut Option<Vec<NodeId>>,
    active_spec_class: &mut Option<String>,
    active_spec_ascend: &mut Option<String>,
    found_root: &mut bool,
    main_socket_group: &mut Option<u32>,
    active_item_set_id: &mut Option<u32>,
) -> Result<(), PobImportError> {
    match name {
        "PathOfBuilding" => {
            *found_root = true;
        }
        "Build" => {
            for attr in e.attributes().with_checks(false).flatten() {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let val = attr
                    .unescape_value()
                    .map_err(|err| PobImportError::Xml(err.to_string()))?
                    .into_owned();
                match key.as_str() {
                    "level" => {
                        if let Ok(n) = val.parse::<u32>() {
                            character.level = n.max(1);
                        }
                    }
                    "className" if !val.is_empty() => {
                        character.class = ClassRef(val);
                    }
                    "ascendClassName" if !val.is_empty() && val != "None" => {
                        character.ascendancy = Some(val);
                    }
                    "bandit" => {
                        if let Some(b) = crate::character::Bandit::from_pob_name(&val) {
                            character.bandit = b;
                        }
                    }
                    "mainSocketGroup" => {
                        if let Ok(n) = val.parse::<u32>() {
                            *main_socket_group = Some(n);
                        }
                    }
                    _ => {}
                }
            }
        }
        "Items" => {
            // PoB pins the active loadout via `<Items activeItemSet="N">`.
            // Capture it so the post-parse reconciliation can pick the
            // right `<ItemSet>` to install as the live items.
            for attr in e.attributes().with_checks(false).flatten() {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let val = attr
                    .unescape_value()
                    .map_err(|err| PobImportError::Xml(err.to_string()))?
                    .into_owned();
                if key == "activeItemSet" {
                    if let Ok(n) = val.parse::<u32>() {
                        *active_item_set_id = Some(n);
                    }
                } else if key == "useSecondWeaponSet" {
                    // Issue #109: PoB stores this per-ItemSet, but
                    // MK2 lifts it to a build-level toggle. The
                    // `<Items>`-level attribute is what we emit on
                    // export; on import we accept either location and
                    // last-write wins (PoB doesn't enforce
                    // consistency between sets either).
                    character.config.use_second_weapon_set = val == "true";
                }
            }
        }
        "Spec" => {
            let mut nodes: Option<Vec<NodeId>> = None;
            let mut class_attr: Option<String> = None;
            let mut ascend_attr: Option<String> = None;
            for attr in e.attributes().with_checks(false).flatten() {
                let key = String::from_utf8_lossy(attr.key.as_ref()).into_owned();
                let val = attr
                    .unescape_value()
                    .map_err(|err| PobImportError::Xml(err.to_string()))?
                    .into_owned();
                match key.as_str() {
                    "nodes" => {
                        let parsed: Vec<NodeId> = val
                            .split(|c: char| c.is_whitespace() || c == ',')
                            .filter_map(|s| s.parse::<NodeId>().ok())
                            .collect();
                        if !parsed.is_empty() {
                            nodes = Some(parsed);
                        }
                    }
                    "classId" | "className" => class_attr = Some(val),
                    "ascendClassId" | "ascendClassName" => ascend_attr = Some(val),
                    _ => {}
                }
            }
            if active_spec_pending.is_none() && nodes.is_some() {
                *active_spec_pending = nodes;
                *active_spec_class = class_attr;
                *active_spec_ascend = ascend_attr;
            }
        }
        _ => {}
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SAMPLE_XML: &str = r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
    <Build level="92" targetVersion="3_0" className="Witch" ascendClassName="Occultist"/>
    <Tree activeSpec="1">
        <Spec classId="3" ascendClassId="3" nodes="59530,55156,57264,2151"/>
    </Tree>
    <Notes>This is a test build.
Multi-line.</Notes>
</PathOfBuilding>"#;

    #[test]
    fn parses_basic_pob_xml() {
        let c = import_pob_xml(SAMPLE_XML).unwrap();
        assert_eq!(c.class.0, "Witch");
        assert_eq!(c.ascendancy.as_deref(), Some("Occultist"));
        assert_eq!(c.level, 92);
        assert!(c.allocated.contains(&59530));
        assert!(c.allocated.contains(&2151));
        assert_eq!(c.allocated.len(), 4);
        assert!(c.notes.contains("test build"));
        assert!(c.notes.contains("Multi-line."));
    }

    #[test]
    fn rejects_non_pob_xml() {
        let xml = "<root><item /></root>";
        assert!(matches!(import_pob_xml(xml), Err(PobImportError::NotPob)));
    }

    #[test]
    fn share_code_round_trip() {
        // Compress + base64-encode the same XML the way PoB does and verify round-trip.
        use base64::engine::general_purpose::URL_SAFE_NO_PAD;
        use flate2::write::ZlibEncoder;
        use flate2::Compression;
        use std::io::Write;

        let mut compressed = Vec::new();
        let mut enc = ZlibEncoder::new(&mut compressed, Compression::default());
        enc.write_all(SAMPLE_XML.as_bytes()).unwrap();
        enc.finish().unwrap();
        let code = URL_SAFE_NO_PAD.encode(&compressed);
        let c = import_pob_code(&code).unwrap();
        assert_eq!(c.class.0, "Witch");
        assert_eq!(c.level, 92);
    }

    #[test]
    fn imports_items_and_equips_first_set() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
    <Build level="90" className="Witch" mainSocketGroup="1"/>
    <Tree activeSpec="1">
        <Spec classId="3" ascendClassId="0" nodes=""/>
    </Tree>
    <Items>
        <Item id="1" variant="1">Rarity: RARE
Soul Charm
Onyx Amulet
--------
Quality: +20% (augmented)
--------
+10 to all Attributes
+62 to maximum Life
+39% to all Elemental Resistances
--------</Item>
        <ItemSet id="1" useSecondWeaponSet="false">
            <Slot name="Amulet" itemId="1" active="true"/>
        </ItemSet>
    </Items>
    <Notes/>
</PathOfBuilding>"#;
        let c = import_pob_xml(xml).expect("import");
        let amulet = c
            .items
            .get(pob_data::Slot::Amulet)
            .expect("amulet equipped");
        assert_eq!(amulet.base_name, "Onyx Amulet");
    }

    #[test]
    fn imports_main_skill_from_skill_group() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
    <Build level="90" className="Witch" mainSocketGroup="1"/>
    <Tree activeSpec="1">
        <Spec classId="3" ascendClassId="0" nodes=""/>
    </Tree>
    <Skills>
        <Skill enabled="true" mainActiveSkill="1" includeInFullDPS="true">
            <Gem skillId="Arc" level="20" quality="20" enabled="true"/>
        </Skill>
    </Skills>
    <Notes/>
</PathOfBuilding>"#;
        let c = import_pob_xml(xml).expect("import");
        let main = c.main_skill.as_ref().expect("main skill set");
        assert_eq!(main.skill_id, "Arc");
        assert_eq!(main.level, 20);
        assert_eq!(main.quality, 20);
    }

    #[test]
    fn imports_config_enemy_resists() {
        let xml = r#"<?xml version="1.0" encoding="UTF-8"?>
<PathOfBuilding>
    <Build level="90" className="Witch"/>
    <Tree activeSpec="1">
        <Spec classId="3" ascendClassId="0" nodes=""/>
    </Tree>
    <Config>
        <Input name="enemyFireResist" number="40"/>
        <Input name="enemyChaosResist" number="25"/>
        <Input name="enemyIsBoss" string="Pinnacle"/>
    </Config>
    <Notes/>
</PathOfBuilding>"#;
        let c = import_pob_xml(xml).expect("import");
        assert_eq!(c.config.enemy_fire_resist, 40);
        assert_eq!(c.config.enemy_chaos_resist, 25);
        assert_eq!(c.config.conditions.get("EnemyIsBoss").copied(), Some(true));
    }

    // Issue #33 (slice 1): URL recogniser. Each supported host
    // maps to its raw-endpoint URL; unrecognised hosts and
    // non-URLs return `None` so the existing share-code fallback
    // path runs unchanged.
    #[test]
    fn resolve_share_url_recognises_pobb_in() {
        assert_eq!(
            resolve_share_url("https://pobb.in/abc123"),
            Some("https://pobb.in/abc123/raw".to_owned())
        );
        // www. prefix tolerated; trailing slash stripped.
        assert_eq!(
            resolve_share_url("https://www.pobb.in/XYZ-789/"),
            Some("https://pobb.in/XYZ-789/raw".to_owned())
        );
        // Query string and fragment dropped.
        assert_eq!(
            resolve_share_url("https://pobb.in/foo?utm=src#x"),
            Some("https://pobb.in/foo/raw".to_owned())
        );
    }

    #[test]
    fn resolve_share_url_recognises_pastebin() {
        assert_eq!(
            resolve_share_url("https://pastebin.com/abc123"),
            Some("https://pastebin.com/raw/abc123".to_owned())
        );
        // Already-resolved /raw/<id> stays resolved (idempotent).
        assert_eq!(
            resolve_share_url("https://pastebin.com/raw/abc123"),
            Some("https://pastebin.com/raw/abc123".to_owned())
        );
    }

    #[test]
    fn resolve_share_url_recognises_poeplanner() {
        assert_eq!(
            resolve_share_url("https://poeplanner.com/abcXYZ123"),
            Some("https://poeplanner.com/api/build/abcXYZ123/pob".to_owned())
        );
    }

    #[test]
    fn resolve_share_url_recognises_pob_cool() {
        // pob.cool is a community pobb.in alternative with the same
        // /<id>/raw shape. Mirror pobb.in tests.
        assert_eq!(
            resolve_share_url("https://pob.cool/abc123"),
            Some("https://pob.cool/abc123/raw".to_owned())
        );
        assert_eq!(
            resolve_share_url("https://www.pob.cool/XYZ-789/"),
            Some("https://pob.cool/XYZ-789/raw".to_owned())
        );
        // Idempotent — already-resolved /raw URL.
        assert_eq!(
            resolve_share_url("https://pob.cool/abc123/raw"),
            Some("https://pob.cool/abc123/raw".to_owned())
        );
    }

    #[test]
    fn resolve_share_url_recognises_poe_ninja_pob() {
        // poe.ninja serves PoB shares at /pob/<id> with a /raw sibling.
        assert_eq!(
            resolve_share_url("https://poe.ninja/pob/abc123"),
            Some("https://poe.ninja/pob/abc123/raw".to_owned())
        );
        // Trailing slash + already-resolved /raw both tolerated.
        assert_eq!(
            resolve_share_url("https://poe.ninja/pob/abc123/"),
            Some("https://poe.ninja/pob/abc123/raw".to_owned())
        );
        assert_eq!(
            resolve_share_url("https://poe.ninja/pob/abc123/raw"),
            Some("https://poe.ninja/pob/abc123/raw".to_owned())
        );
        // poe.ninja ladder/build pages (non-/pob path) shouldn't match.
        assert_eq!(resolve_share_url("https://poe.ninja/builds/league"), None);
    }

    #[test]
    fn resolve_share_url_idempotent_for_pobb_in_raw() {
        // Pasting an already-resolved /raw URL should resolve to
        // itself (no double `/raw/raw`).
        assert_eq!(
            resolve_share_url("https://pobb.in/abc123/raw"),
            Some("https://pobb.in/abc123/raw".to_owned())
        );
    }

    #[test]
    fn resolve_share_url_rejects_non_urls_and_unknown_hosts() {
        // Non-URL → None (caller's existing share-code path
        // handles it).
        assert_eq!(resolve_share_url("xnd_abc123"), None);
        assert_eq!(resolve_share_url(""), None);
        assert_eq!(resolve_share_url("not a url"), None);
        // Unknown host.
        assert_eq!(resolve_share_url("https://github.com/foo/bar"), None);
        // Pobb.in but not a single-id path (e.g. about page).
        assert_eq!(resolve_share_url("https://pobb.in/about/team"), None);
        // Empty id after the host.
        assert_eq!(resolve_share_url("https://pobb.in/"), None);
        // ID with disallowed characters (so we don't cargo-cult
        // arbitrary path segments into the raw endpoint).
        assert_eq!(resolve_share_url("https://pobb.in/has spaces"), None);
        assert_eq!(resolve_share_url("https://pobb.in/has.dots"), None);
        // poe.ninja without the /pob prefix.
        assert_eq!(resolve_share_url("https://poe.ninja/abc123"), None);
        // pob.cool with multi-segment path beyond /raw.
        assert_eq!(resolve_share_url("https://pob.cool/abc/def"), None);
    }
}
