//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/TreeTab.lua 1-2888.
//! Owner: U25. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod core;
pub mod mastery;
pub mod notable_db;
pub mod popups;
pub mod power;
pub mod power_report_list;
pub mod spec_list;
pub mod timeless;
pub mod timeless_list;
pub mod timeless_socket;

/// `TreeTab` (one Lua class split over U25, U26, U27; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct TreeTab {
    pub core: core::State,
    pub power: power::State,
    pub timeless: timeless::State,
}
