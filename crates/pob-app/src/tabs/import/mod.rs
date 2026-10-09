//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ImportTab.lua 1-1932.
//! Owner: U43. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod accounts;
pub mod build_sites;
pub mod character;
pub mod core;
pub mod poe_api;

/// `ImportTab` (one Lua class split over U43, U44; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct ImportTab {
    pub core: core::State,
    pub character: character::State,
}
