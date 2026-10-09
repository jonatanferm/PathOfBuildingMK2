//! The typed `data` schema (calc doc §4.4). Every struct is
//! `#[serde(deny_unknown_fields)]` (or a hand-written strict deserializer);
//! the field lists are the U0 census of the real dump
//! (docs/port/contracts.md#schema).

pub mod affixes;
pub mod bases;
pub mod calc_sections;
pub mod config_defs;
pub mod gems;
pub mod item_data;
pub mod minions;
pub mod misc;
pub mod mod_map_defs;
pub mod skills;
pub mod stat_descriptions;
pub mod tree;
pub mod uniques;
