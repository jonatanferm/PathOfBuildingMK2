//! Build share codes — base64(deflate(json)). PoB's native format is XML, but for the
//! Rust port we ship a simpler JSON-based code first; we can wire in PoB-format import
//! later (via quick-xml, plumbed at the data layer). Phase 5 ships this MK2 format so
//! users can round-trip a build between sessions.
//!
//! Format: `MK2|<base64>` where `<base64>` = url-safe base64 of zlib-compressed JSON.
//!
//! Issue #212 (slice 2): a sibling `MK2SET|<base64>` format covers a
//! single `NamedItemSet`, so users can copy one loadout out of build A
//! and paste it into build B without bringing the whole character
//! along. Uses the same compress + url-safe-base64 transport so a
//! future binary format swap (e.g. CBOR) can flip both share kinds in
//! one PR.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

use crate::character::{Character, CharacterSnapshot, NamedItemSet};

pub fn export_code(character: &Character) -> Result<String, ShareError> {
    let snap = CharacterSnapshot::from_character(character);
    let json = serde_json::to_vec(&snap).map_err(ShareError::Json)?;
    let mut compressed = Vec::with_capacity(json.len() / 2);
    let mut enc = ZlibEncoder::new(&mut compressed, Compression::best());
    enc.write_all(&json).map_err(ShareError::Io)?;
    enc.finish().map_err(ShareError::Io)?;
    let b64 = URL_SAFE_NO_PAD.encode(&compressed);
    Ok(format!("MK2|{b64}"))
}

pub fn import_code(code: &str) -> Result<Character, ShareError> {
    let body = code
        .trim()
        .strip_prefix("MK2|")
        .ok_or(ShareError::WrongPrefix)?;
    let compressed = URL_SAFE_NO_PAD
        .decode(body.as_bytes())
        .map_err(|e| ShareError::Decode(e.to_string()))?;
    let mut dec = ZlibDecoder::new(compressed.as_slice());
    let mut json = Vec::new();
    dec.read_to_end(&mut json).map_err(ShareError::Io)?;
    let snap: CharacterSnapshot = serde_json::from_slice(&json).map_err(ShareError::Json)?;
    Ok(snap.into_character())
}

#[derive(Debug)]
pub enum ShareError {
    WrongPrefix,
    Decode(String),
    Io(std::io::Error),
    Json(serde_json::Error),
}

impl std::fmt::Display for ShareError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            Self::WrongPrefix => write!(f, "build code must start with `MK2|`"),
            Self::Decode(e) => write!(f, "base64 decode failed: {e}"),
            Self::Io(e) => write!(f, "compression i/o: {e}"),
            Self::Json(e) => write!(f, "JSON: {e}"),
        }
    }
}

impl std::error::Error for ShareError {}

/// Issue #212: serialise a single [`NamedItemSet`] into the
/// `MK2SET|<base64>` clipboard format. The body is url-safe base64 of
/// zlib-compressed JSON — same shape as [`export_code`] but scoped to
/// one loadout. Use this as the payload of "Export to clipboard" in
/// the Items-tab manage popup.
pub fn export_item_set(set: &NamedItemSet) -> Result<String, ShareError> {
    let json = serde_json::to_vec(set).map_err(ShareError::Json)?;
    let mut compressed = Vec::with_capacity(json.len() / 2);
    let mut enc = ZlibEncoder::new(&mut compressed, Compression::best());
    enc.write_all(&json).map_err(ShareError::Io)?;
    enc.finish().map_err(ShareError::Io)?;
    let b64 = URL_SAFE_NO_PAD.encode(&compressed);
    Ok(format!("MK2SET|{b64}"))
}

/// Issue #212: inverse of [`export_item_set`]. Strict `MK2SET|`
/// prefix gate so a stale paste from the *build* clipboard format
/// (`MK2|`) returns a clean `WrongPrefix` rather than landing as a
/// partial item set. Returns the decoded set; the caller chooses
/// whether to append it to `character.item_sets` or replace one in
/// place.
pub fn import_item_set(code: &str) -> Result<NamedItemSet, ShareError> {
    let body = code
        .trim()
        .strip_prefix("MK2SET|")
        .ok_or(ShareError::WrongPrefix)?;
    let compressed = URL_SAFE_NO_PAD
        .decode(body.as_bytes())
        .map_err(|e| ShareError::Decode(e.to_string()))?;
    let mut dec = ZlibDecoder::new(compressed.as_slice());
    let mut json = Vec::new();
    dec.read_to_end(&mut json).map_err(ShareError::Io)?;
    let set: NamedItemSet = serde_json::from_slice(&json).map_err(ShareError::Json)?;
    Ok(set)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::ClassRef;

    #[test]
    fn round_trip() {
        let mut c = Character::new(ClassRef::ranger(), 67);
        c.allocated.insert(101);
        c.allocated.insert(202);
        c.notes = "test build".to_owned();
        let code = export_code(&c).unwrap();
        let back = import_code(&code).unwrap();
        assert_eq!(back.class.0, "Ranger");
        assert_eq!(back.level, 67);
        assert!(back.allocated.contains(&101));
        assert!(back.allocated.contains(&202));
        assert_eq!(back.notes, "test build");
    }

    #[test]
    fn rejects_bad_prefix() {
        assert!(matches!(
            import_code("not a code"),
            Err(ShareError::WrongPrefix)
        ));
    }

    fn mk_item(name: &str, base: &str) -> pob_data::Item {
        pob_data::Item {
            name: name.to_owned(),
            base_name: base.to_owned(),
            rarity: pob_data::Rarity::Rare,
            item_level: 84,
            quality: 20,
            tags: ahash::HashSet::default(),
            mod_lines: vec![pob_data::ModLine::new(
                "+50 to maximum Life",
                pob_data::ModSection::Explicit,
            )],
            sockets: String::new(),
            raw: format!("Rarity: RARE\n{name}\n{base}\n--------\n+50 to maximum Life\n"),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    fn mk_named_set(label: &str) -> NamedItemSet {
        let mut items = pob_data::ItemSet::new();
        items.equip(pob_data::Slot::Amulet, mk_item("Soul Charm", "Onyx Amulet"));
        items.equip(pob_data::Slot::Helmet, mk_item("Storm Cowl", "Iron Hat"));
        NamedItemSet {
            name: label.to_owned(),
            items,
        }
    }

    #[test]
    fn item_set_share_round_trips() {
        // Issue #212: an item set survives a round trip through the
        // MK2SET clipboard format with the same name, slots, and per
        // -slot mod lines + raw text intact.
        let set = mk_named_set("Mapping");
        let code = export_item_set(&set).expect("export");
        assert!(
            code.starts_with("MK2SET|"),
            "expected MK2SET prefix, got {code:?}",
        );
        let back = import_item_set(&code).expect("import");
        assert_eq!(back.name, "Mapping");
        let amulet = back
            .items
            .get(pob_data::Slot::Amulet)
            .expect("amulet slot present");
        assert_eq!(amulet.name, "Soul Charm");
        assert_eq!(amulet.base_name, "Onyx Amulet");
        assert!(amulet
            .mod_lines
            .iter()
            .any(|m| m.line.contains("+50 to maximum Life")));
        let helmet = back
            .items
            .get(pob_data::Slot::Helmet)
            .expect("helmet slot present");
        assert_eq!(helmet.name, "Storm Cowl");
    }

    #[test]
    fn item_set_share_rejects_wrong_prefix() {
        // A stale build-share code (MK2|...) must not silently land
        // as half a parsed item set — the prefix check guards that.
        let stub_build = export_code(&Character::new(ClassRef::ranger(), 1)).unwrap();
        assert!(stub_build.starts_with("MK2|"));
        assert!(matches!(
            import_item_set(&stub_build),
            Err(ShareError::WrongPrefix)
        ));
        // Random garbage also fails the prefix check cleanly.
        assert!(matches!(
            import_item_set("not a code"),
            Err(ShareError::WrongPrefix)
        ));
    }

    #[test]
    fn item_set_share_rejects_bad_base64() {
        // Right prefix, but the body isn't base64 — Decode variant.
        let code = "MK2SET|@@not-base64@@";
        assert!(matches!(import_item_set(code), Err(ShareError::Decode(_))));
    }

    #[test]
    fn item_set_share_trims_whitespace_around_pasted_code() {
        // Clipboard paste often pulls in trailing whitespace from the
        // copy source. Confirm we trim before stripping the prefix
        // (mirrors the existing `import_code` contract).
        let set = mk_named_set("Bossing");
        let code = export_item_set(&set).unwrap();
        let padded = format!("\n  {code}\n\n");
        let back = import_item_set(&padded).expect("import with padding");
        assert_eq!(back.name, "Bossing");
    }
}
