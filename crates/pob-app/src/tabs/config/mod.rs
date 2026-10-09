//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ConfigTab.lua 1-1362.
//! Owner: U41. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod core;
pub mod ctor;
pub mod custom_mod_block;
pub mod mod_browser;
pub mod set_list;
pub mod visibility;

/// `ConfigTab` (one Lua class split over U41, U42; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct ConfigTab {
    pub ctor: ctor::State,
    pub core: core::State,
}
