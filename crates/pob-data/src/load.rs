//! JSON loaders. Each function takes a string slice (or anything that derefs to one) so
//! callers control their own I/O. The runtime crates use these; extraction is upstream.

use crate::{Gem, ItemBase, LoadError, PassiveTree};
use indexmap::IndexMap;

pub fn load_passive_tree(json: &str) -> Result<PassiveTree, LoadError> {
    Ok(serde_json::from_str(json)?)
}

pub fn load_bases(json: &str) -> Result<IndexMap<String, ItemBase>, LoadError> {
    Ok(serde_json::from_str(json)?)
}

pub fn load_gems(json: &str) -> Result<IndexMap<String, Gem>, LoadError> {
    Ok(serde_json::from_str(json)?)
}

pub fn load_tree_index(json: &str) -> Result<Vec<String>, LoadError> {
    Ok(serde_json::from_str(json)?)
}
