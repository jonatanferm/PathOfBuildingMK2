//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/ItemsTab.lua 1-5055.
//! Owner: U33. UI-U0 stub (ui doc §12): the file exists so the owner never edits module
//! declarations; its contents are the owner's.

pub mod add_implicit;
pub mod core;
pub mod craft_a;
pub mod craft_b;
pub mod ctor;
pub mod display_item;
pub mod item_db;
pub mod item_list;
pub mod set_list;
pub mod shared_list;
pub mod shared_set_list;
pub mod slot;
pub mod slot_helper;
pub mod tooltip;

/// `ItemsTab` (one Lua class split over U33-U38; ui doc §12.1 "Split structs").
#[derive(Default)]
pub struct ItemsTab {
    pub ctor: ctor::State,
    pub core: core::State,
    pub display_item: display_item::State,
    pub craft_a: craft_a::State,
    pub craft_b: craft_b::State,
    pub tooltip: tooltip::State,
}
