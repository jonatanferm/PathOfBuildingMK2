//! Build share codes — base64(deflate(json)). PoB's native format is XML, but for the
//! Rust port we ship a simpler JSON-based code first; we can wire in PoB-format import
//! later (via quick-xml, plumbed at the data layer). Phase 5 ships this MK2 format so
//! users can round-trip a build between sessions.
//!
//! Format: `MK2|<base64>` where `<base64>` = url-safe base64 of zlib-compressed JSON.

use base64::engine::general_purpose::URL_SAFE_NO_PAD;
use base64::Engine;
use flate2::read::ZlibDecoder;
use flate2::write::ZlibEncoder;
use flate2::Compression;
use std::io::{Read, Write};

use crate::character::{Character, CharacterSnapshot};

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
}
