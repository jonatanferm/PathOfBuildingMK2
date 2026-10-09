//! Tree data: `GameVersions` (GameVersions.lua) and the per-version tree
//! files, which are loaded lazily ([`crate::tree_bytes`]) and parsed by the
//! PassiveTree class (PT) from their [`DataValue`](crate::DataValue) form.
#![allow(non_snake_case)]

use serde::Deserialize;

use crate::loader;
use crate::Name;

/// `treeVersions[ver]`.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct TreeVersionMeta {
    pub display: Name,
    pub num: f64,
    #[serde(default)]
    pub url: Option<Name>,
}

/// The `GameVersions` globals.
#[derive(Debug, Clone, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct GameVersions {
    pub defaultTreeVersion: Name,
    pub latestTreeVersion: Name,
    pub legacyTargetVersion: Name,
    pub liveTargetVersion: Name,
    #[serde(deserialize_with = "loader::int_map")]
    pub poePlannerVersions: std::collections::BTreeMap<i64, Name>,
    pub treeVersionList: Vec<Name>,
    pub treeVersions: indexmap::IndexMap<Name, TreeVersionMeta>,
}
