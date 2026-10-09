//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/CompareTab.lua 1-5036.
//! Owner: U46. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod calcs_helpers;
pub mod draw_a;
pub mod draw_b;
pub mod entry;
pub mod import;
pub mod init;
pub mod layout;
pub mod power;
pub mod power_list;

/// `CompareTab` (one Lua class split over U46-U50; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct CompareTab {
    pub init: init::State,
    pub layout: layout::State,
    pub power: power::State,
    pub draw_a: draw_a::State,
    pub draw_b: draw_b::State,
}
