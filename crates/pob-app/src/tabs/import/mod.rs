//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ImportTab.lua 1-1932.
//! Owner: U43 (the struct and its `Widget` impl; U43 owns the class's `Draw`, ui doc §12.1).
//!
//! The tab is a free node owned by the build node (`self.importTab`, a field of the build);
//! BuildMode reaches it through `BuildState::tabs.import` and the seam functions in
//! [`core`]. `self.controls.<key>` are the node's children under the Lua keys.

pub mod accounts;
pub mod build_sites;
pub mod character;
pub mod core;
pub mod poe_api;

use pob_ui_core::prelude::*;

/// `ImportTab` (one Lua class split over U43, U44; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct ImportTab {
    pub core: core::State,
    pub character: character::State,
}

/// `newClass("ImportTab", "ControlHost", "Control")` (ImportTab.lua:474): drawn by BuildMode
/// through `core::draw_tab`, so it has no `Draw` capability of its own in the control loop.
impl Widget for ImportTab {
    const NAME: &'static str = "ImportTab";
    const CAPS: Caps = Caps::empty();
}
