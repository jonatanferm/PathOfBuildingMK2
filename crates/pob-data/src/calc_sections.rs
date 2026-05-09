//! Calcs-tab section layout. Mirrors PoB's `Modules/CalcSections.lua` declarative table.
//!
//! Slice 1 of [#34](https://github.com/jonatanferm/PathOfBuildingMK2/issues/34) only
//! captures the top-level section + subsection headers — enough to drive section
//! ordering, grouping, and collapse defaults in the UI. The deep `data` row tree
//! (`modName` / `modType` / `format` / nested rows) is intentionally dropped so the
//! JSON stays small and we don't lock in a schema before [`CalcBreakdown.lua`] is ported.
//! Later slices will widen the schema row-by-row as breakdowns come online.
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
           "subsections":[{"label":"Attack/Cast Rate"}]}
        ]"#;
        let sections = load_calc_sections(json).expect("decode");
        assert_eq!(sections.len(), 2);
        assert_eq!(sections[0].id, "HitDamage");
        assert_eq!(sections[0].subsections.len(), 1);
        assert_eq!(sections[0].subsections[0].label, "Skill Hit Damage");
        assert!(!sections[1].subsections[0].default_collapsed);
    }
}
