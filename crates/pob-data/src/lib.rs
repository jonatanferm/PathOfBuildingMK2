//! Data types for Path of Building MK2.
//!
//! Types here are shared by the engine, the UI, and the extractor. No `std::fs` here either —
//! loaders take readers/strings, callers handle I/O.

pub mod bases;
pub mod calc_sections;
pub mod cluster_jewel_mods;
pub mod cluster_jewels;
pub mod flags;
pub mod gem;
pub mod item;
pub mod jewel_radius;
pub mod load;
pub mod minions;
pub mod monster_tables;
pub mod skill;
pub mod sprites;
pub mod tattoos;
pub mod timeless_jewels;
pub mod tree;

pub use bases::{ArmourStats, FlaskStats, ItemBase, ItemBaseKind, ItemReq, WeaponStats};
pub use calc_sections::{load_calc_sections, CalcRow, CalcSection, CalcSubsection};
pub use cluster_jewel_mods::{load_cluster_jewel_mods, ClusterMod, ClusterModSet};
pub use cluster_jewels::{load_cluster_jewels, ClusterJewelData, ClusterJewelType, ClusterSkill};
pub use flags::{KeywordFlag, ModFlag, SkillType};
pub use gem::Gem;
pub use item::{Item, ItemSet, ModLine, ModSection, Rarity, Slot};
pub use jewel_radius::{
    max_outer, radii_for_tree_version, radius_index_for_label, JewelRadiusInfo, RADII_3_15,
    RADII_3_16,
};
pub use load::{load_bases, load_gems, load_passive_tree, load_skill_file, load_tree_index};
pub use minions::{load_minions, MinionData, MinionType};
pub use skill::{Skill, SkillSet};
pub use tattoos::{load_tattoos, Tattoo, TattooSet};
pub use timeless_jewels::{
    load_timeless_jewels, ConquerorKeystone, TimelessConqueror, TimelessJewelConfig,
    TimelessJewelData,
};
pub use tree::{
    Ascendancy, Class, Group, GroupBackground, MasteryEffect, Node, NodeId, NodeKind, PassiveTree,
    Rect, TreeConstants, TreePoints, ROOT_NODE_ID,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid data: {0}")]
    Invalid(String),
}
