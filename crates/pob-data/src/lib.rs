//! Data types for Path of Building MK2.
//!
//! Types here are shared by the engine, the UI, and the extractor. No `std::fs` here either —
//! loaders take readers/strings, callers handle I/O.

pub mod bases;
pub mod flags;
pub mod gem;
pub mod load;
pub mod tree;

pub use bases::{ArmourStats, FlaskStats, ItemBase, ItemBaseKind, ItemReq, WeaponStats};
pub use flags::{KeywordFlag, ModFlag, SkillType};
pub use gem::Gem;
pub use load::{load_bases, load_gems, load_passive_tree, load_tree_index};
pub use tree::{
    Ascendancy, Class, Group, GroupBackground, MasteryEffect, Node, NodeId, NodeKind,
    PassiveTree, Rect, TreeConstants, TreePoints, ROOT_NODE_ID,
};

use thiserror::Error;

#[derive(Debug, Error)]
pub enum LoadError {
    #[error("invalid JSON: {0}")]
    Json(#[from] serde_json::Error),
    #[error("invalid data: {0}")]
    Invalid(String),
}
