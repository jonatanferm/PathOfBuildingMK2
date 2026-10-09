//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CalcsTab.lua 1-773.
//! Owner: U39. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod breakdown;
pub mod core;
pub mod section;
pub mod sections_data;

/// `CalcsTab` (UI half; Load/Save 163-225 and 441-773 are calc:BM-A/calc:PWR; ui doc §12.1).
#[derive(Default)]
pub struct CalcsTab {
    pub core: core::State,
}
