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
use pob_data::NodeId;

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
    let xml = String::from_utf8(xml_bytes)
        .map_err(|e| PobImportError::Decode(format!("utf-8: {e}")))?;
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
                )?;
                if name == "Notes" {
                    in_notes = true;
                    notes_collect.clear();
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
                )?;
            }
            Ok(Event::End(e)) => {
                let name = String::from_utf8_lossy(e.name().as_ref()).into_owned();
                if name == "Notes" {
                    in_notes = false;
                    character.notes = std::mem::take(&mut notes_collect);
                }
                depth_stack.pop();
            }
            Ok(Event::Text(t)) => {
                if in_notes {
                    if let Ok(s) = t.unescape() {
                        notes_collect.push_str(&s);
                    }
                }
            }
            Ok(Event::CData(t)) => {
                if in_notes {
                    notes_collect.push_str(&String::from_utf8_lossy(&t));
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
    // Spec-level class attribute is sometimes a name (`className`) and sometimes a
    // numeric class id (`classId`). Only override the Build-level value when the spec
    // gives a non-numeric name, since the numeric id requires a tree-version-keyed
    // lookup we don't bother with for Phase 5.
    if let Some(c) = active_spec_class.filter(|s| !s.is_empty() && !is_numeric(s)) {
        character.class = ClassRef(c);
    }
    if let Some(a) = active_spec_ascend
        .filter(|s| !s.is_empty() && s != "None" && !is_numeric(s))
    {
        character.ascendancy = Some(a);
    }

    Ok(character)
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
                    "className" => {
                        if !val.is_empty() {
                            character.class = ClassRef(val);
                        }
                    }
                    "ascendClassName" => {
                        if !val.is_empty() && val != "None" {
                            character.ascendancy = Some(val);
                        }
                    }
                    _ => {}
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
}
