//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/SkillsTab.lua 1-1502.
//! Owner: U30. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod core;
pub mod gem_slots;
pub mod minion_list;
pub mod minion_search_list;
pub mod skill_list;
pub mod skill_set_list;

/// `SkillsTab` (one Lua class split over U30, U31; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct SkillsTab {
    pub core: core::State,
    pub gem_slots: gem_slots::State,
}
