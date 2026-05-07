//! Passive tree data: classes, ascendancies, groups, nodes.
//!
//! Mirrors the structure of `src/TreeData/<version>/tree.lua` in the upstream PoB repo.
//! Loaded from the JSON dump produced by `pob-extract`.

use ahash::HashMap;
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Numeric node id used in `nodes`, in/out edges, and group node lists.
///
/// In the Lua source these are sometimes string-typed and sometimes int-typed depending on
/// where they appear. We canonicalise on `u32`. The synthetic `root` node uses `0`.
pub type NodeId = u32;

pub const ROOT_NODE_ID: NodeId = 0;

/// Top-level tree document.
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct PassiveTree {
    /// PoB tree version directory name (`3_25`, `3_25_alternate`, …).
    pub version: String,
    /// Internal name of this tree (`"Default"`, `"Ruthless"`, `"AlternateAscendancy"`, …).
    pub tree: String,
    pub classes: Vec<Class>,
    pub groups: HashMap<u32, Group>,
    pub nodes: HashMap<NodeId, Node>,
    #[serde(default)]
    pub jewel_slots: Vec<NodeId>,
    pub min_x: i32,
    pub min_y: i32,
    pub max_x: i32,
    pub max_y: i32,
    pub constants: TreeConstants,
    #[serde(default)]
    pub points: TreePoints,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct TreePoints {
    #[serde(default)]
    pub total_points: u32,
    #[serde(default)]
    pub ascendancy_points: u32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Class {
    pub name: String,
    pub base_str: i32,
    pub base_dex: i32,
    pub base_int: i32,
    #[serde(default)]
    pub ascendancies: Vec<Ascendancy>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Ascendancy {
    pub id: String,
    pub name: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub flavour_text: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub flavour_text_colour: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub flavour_text_rect: Option<Rect>,
}

#[derive(Debug, Clone, Copy, Serialize, Deserialize)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub width: f32,
    pub height: f32,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Group {
    pub x: f32,
    pub y: f32,
    /// Orbit indices used by nodes in this group (refers to `constants.orbit_radii`).
    #[serde(default)]
    pub orbits: SmallVec<[u8; 4]>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub background: Option<GroupBackground>,
    #[serde(default)]
    pub nodes: Vec<NodeId>,
    /// True iff this group exists only to host ascendancy nodes.
    #[serde(default)]
    pub is_proxy: bool,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct GroupBackground {
    pub image: String,
    #[serde(default)]
    pub is_half_image: bool,
    #[serde(default)]
    pub offset_x: Option<f32>,
    #[serde(default)]
    pub offset_y: Option<f32>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Node {
    pub id: NodeId,
    /// Display name. The synthetic root node has none.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub name: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub icon: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub ascendancy_name: Option<String>,
    #[serde(default)]
    pub stats: Vec<String>,
    #[serde(default)]
    pub reminder_text: Vec<String>,
    pub kind: NodeKind,
    /// Class index for class-start nodes.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub class_start_index: Option<u32>,
    /// Layout: group + orbit + orbitIndex (mirrors Lua field names).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub group: Option<u32>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub orbit: Option<u8>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub orbit_index: Option<u32>,
    #[serde(default, rename = "out")]
    pub out_edges: SmallVec<[NodeId; 4]>,
    #[serde(default, rename = "in")]
    pub in_edges: SmallVec<[NodeId; 4]>,
    /// Mastery effects for `Mastery` nodes (effect id → list of stat strings).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub mastery_effects: Vec<MasteryEffect>,
    /// Cluster jewel sockets carry an expansion radius (Small/Medium/Large).
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub expansion_jewel_size: Option<u8>,
    /// Marker for jewel-radius rings on the tree visualization.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub jewel_radius: Option<u8>,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize, Default)]
#[serde(rename_all = "snake_case")]
pub enum NodeKind {
    #[default]
    Normal,
    Notable,
    Keystone,
    Mastery,
    JewelSocket,
    /// `root` synthetic node. Its `out` edges point at every class-start.
    Root,
    /// Class start node — has no skill effect, only a class index.
    ClassStart,
    /// Ascendancy class start node.
    AscendancyStart,
    /// Tattoo override (placeable atlas of effects). Rare.
    Tattoo,
    /// Blighted/altered (legion/timeless) marker. Carries different rendering.
    Blighted,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MasteryEffect {
    pub effect: u32,
    #[serde(default)]
    pub stats: Vec<String>,
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub reminder_text: Vec<String>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct TreeConstants {
    pub skills_per_orbit: Vec<u32>,
    pub orbit_radii: Vec<i32>,
    #[serde(default)]
    pub classes: HashMap<String, u8>,
    #[serde(default)]
    pub character_attributes: HashMap<String, u8>,
    #[serde(default)]
    pub pss_centre_inner_radius: Option<i32>,
}
