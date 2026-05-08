//! Write a `Character` as a Path of Building Community-compatible XML document.
//!
//! Inverse of [`crate::pob_import`]. Produces a document that PoB can open: it has the
//! `<PathOfBuilding>` root, a `<Build>` element with class / ascendancy / level, a
//! single `<Tree>` containing a `<Spec>` with `nodes="id1,id2,..."`, and a `<Notes>`
//! element with the user's notes.
//!
//! Items, skills, and config are NOT written in this minimum viable export — those need
//! richer per-element serialisation that's fundamentally a Phase-5 follow-up. Tracked in
//! `docs/divergences.md`.

use crate::character::Character;

pub fn export_pob_xml(character: &Character) -> String {
    let class = xml_escape(&character.class.0);
    let ascendancy = character
        .ascendancy
        .as_deref()
        .filter(|s| !s.is_empty())
        .map(xml_escape)
        .unwrap_or_else(|| "None".to_owned());
    let class_id = class_name_to_id(&character.class.0);

    let mut nodes_str = String::new();
    let mut sorted: Vec<_> = character.allocated.iter().copied().collect();
    sorted.sort_unstable();
    for (i, id) in sorted.iter().enumerate() {
        if i > 0 {
            nodes_str.push(',');
        }
        nodes_str.push_str(&id.to_string());
    }

    let notes = xml_escape(&character.notes);

    format!(
        concat!(
            "<?xml version=\"1.0\" encoding=\"UTF-8\"?>\n",
            "<PathOfBuilding>\n",
            "    <Build level=\"{level}\" targetVersion=\"3_0\" className=\"{class}\" ascendClassName=\"{asc}\" mainSocketGroup=\"1\"/>\n",
            "    <Tree activeSpec=\"1\">\n",
            "        <Spec masteryEffects=\"\" treeVersion=\"3_25\" classId=\"{class_id}\" ascendClassId=\"0\" nodes=\"{nodes}\"/>\n",
            "    </Tree>\n",
            "    <Notes>{notes}</Notes>\n",
            "    <Items/>\n",
            "    <Skills/>\n",
            "    <Config/>\n",
            "</PathOfBuilding>\n"
        ),
        level = character.level.max(1),
        class = class,
        asc = ascendancy,
        class_id = class_id,
        nodes = nodes_str,
        notes = notes
    )
}

fn class_name_to_id(class: &str) -> u32 {
    match class {
        "Scion" => 0,
        "Marauder" => 1,
        "Ranger" => 2,
        "Witch" => 3,
        "Duelist" => 4,
        "Templar" => 5,
        "Shadow" => 6,
        _ => 0,
    }
}

/// Encode the `xml(deflate(bytes))` PoB share-code format.
pub fn export_pob_code(character: &Character) -> Result<String, std::io::Error> {
    use base64::engine::general_purpose::URL_SAFE_NO_PAD;
    use base64::Engine as _;
    use flate2::write::ZlibEncoder;
    use flate2::Compression;
    use std::io::Write;

    let xml = export_pob_xml(character);
    let mut compressed = Vec::with_capacity(xml.len() / 2);
    let mut enc = ZlibEncoder::new(&mut compressed, Compression::default());
    enc.write_all(xml.as_bytes())?;
    enc.finish()?;
    Ok(URL_SAFE_NO_PAD.encode(&compressed))
}

fn xml_escape(s: &str) -> String {
    let mut out = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            '<' => out.push_str("&lt;"),
            '>' => out.push_str("&gt;"),
            '&' => out.push_str("&amp;"),
            '"' => out.push_str("&quot;"),
            '\'' => out.push_str("&apos;"),
            _ => out.push(c),
        }
    }
    out
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::character::ClassRef;

    #[test]
    fn round_trip_through_pob_xml() {
        let mut c = Character::new(ClassRef::witch(), 92);
        c.ascendancy = Some("Occultist".into());
        c.allocated.insert(101);
        c.allocated.insert(202);
        c.allocated.insert(303);
        c.notes = "Build summary <with> & special characters.".into();

        let xml = export_pob_xml(&c);
        let imported = crate::pob_import::import_pob_xml(&xml).unwrap();

        assert_eq!(imported.class.0, "Witch");
        assert_eq!(imported.ascendancy.as_deref(), Some("Occultist"));
        assert_eq!(imported.level, 92);
        assert_eq!(imported.allocated.len(), 3);
        assert!(imported.allocated.contains(&101));
        assert!(imported.allocated.contains(&303));
        assert_eq!(
            imported.notes,
            "Build summary <with> & special characters."
        );
    }

    #[test]
    fn round_trip_through_pob_code() {
        let mut c = Character::new(ClassRef::ranger(), 67);
        c.allocated.insert(50);
        let code = export_pob_code(&c).unwrap();
        let imported = crate::pob_import::import_pob_code(&code).unwrap();
        assert_eq!(imported.class.0, "Ranger");
        assert_eq!(imported.level, 67);
        assert!(imported.allocated.contains(&50));
    }
}
