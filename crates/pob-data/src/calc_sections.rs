//! Calcs-tab section layout. Mirrors PoB's `Modules/CalcSections.lua` declarative table.
//!
//! Slice 1 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34) added the
//! top-level section + subsection headers; slice 2 widens the schema with a flat list of
//! per-subsection rows: label, an optional `output_key` extracted from the row's
//! `{N:output:Foo}` format string, and the `haveOutput` visibility gate. Mod-breakdown
//! references (`modName` / `modType` / `cfg`) and matrix-shape per-element formats are
//! still dropped — those wait until [`CalcBreakdown.lua`] is ported in a later slice.
//!
//! [`CalcBreakdown.lua`]: https://github.com/PathOfBuildingCommunity/PathOfBuilding/blob/dev/src/Modules/CalcBreakdown.lua

use serde::{Deserialize, Serialize};

/// One Calcs-tab section. `group` is PoB's column-group number:
/// `1 = Offence`, `2 = Core`, `3 = Defence`.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcSection {
    pub id: String,
    pub width: u8,
    pub group: u8,
    pub color: String,
    pub subsections: Vec<CalcSubsection>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcSubsection {
    pub label: String,
    #[serde(default)]
    pub default_collapsed: bool,
    #[serde(default)]
    pub rows: Vec<CalcRow>,
}

/// One row inside a subsection. `output_key` is the `{N:output:Key}` token pulled from the
/// row's primary format string (depth-first across nested sub-rows). `have_output` mirrors
/// the upstream `haveOutput = "Key"` field that gates row visibility on a non-zero output.
///
/// `flag` / `not_flag` capture the `flag = "spell"` / `notFlag = "attack"` style skill
/// gating — stored as already-flattened comma strings (so `flagList = {"x", "y"}` becomes
/// `"x,y"`). The renderer can then split on `,` if it wants per-tag matching.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct CalcRow {
    pub label: String,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub output_key: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub have_output: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub format: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flag: Option<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub not_flag: Option<String>,
}

pub fn load_calc_sections(json: &str) -> serde_json::Result<Vec<CalcSection>> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_minimal_payload() {
        let json = r#"[
          {"id":"HitDamage","width":3,"group":1,"color":"^xE07030",
           "subsections":[{"label":"Skill Hit Damage","default_collapsed":false}]},
          {"id":"Speed","width":1,"group":1,"color":"^xE07030",
           "subsections":[{"label":"Attack/Cast Rate","rows":[
             {"label":"Attacks per second","output_key":"Speed","format":"{2:output:Speed}"}
           ]}]}
        ]"#;
        let sections = load_calc_sections(json).expect("decode");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].id, "HitDamage");
        // Old payloads without `rows` deserialise to an empty vec via #[serde(default)].
        assert!(sections[0].subsections[0].rows.is_empty());
        let speed = &sections[1].subsections[0];
        assert_eq!(speed.rows.len(), 1);
        assert_eq!(speed.rows[0].label, "Attacks per second");
        assert_eq!(speed.rows[0].output_key.as_deref(), Some("Speed"));
    }

    #[test]
    fn deserializes_minimal_row() {
        let row: CalcRow = serde_json::from_str(r#"{"label":"X"}"#).unwrap();
        assert_eq!(row.label, "X");
        assert!(row.output_key.is_none());
        assert!(row.format.is_none());
    }
}
