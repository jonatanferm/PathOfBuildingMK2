//! Skill-gem metadata. Mirrors the structure of `src/Data/Gems.lua`.
//!
//! Gem *behaviour* (the granted skill effects and their level scaling) lives in
//! `src/Data/Skills/*.lua` and is **not** modelled in Phase 1 — those files contain Lua
//! function references for stat→mod conversion that need a richer extractor. See
//! `docs/decisions/0002-skills-deferred.md`.

use ahash::HashSet;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// Map keyed by full metadata id (e.g. `"Metadata/Items/Gems/SkillGemFireball"`).
pub type GemSet = IndexMap<String, Gem>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Gem {
    pub name: String,
    pub base_type_name: String,
    pub game_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub variant_id: Option<String>,
    pub granted_effect_id: String,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub secondary_granted_effect_id: Option<String>,
    #[serde(default)]
    pub vaal_gem: bool,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default)]
    pub tag_string: String,
    #[serde(default)]
    pub req_str: u32,
    #[serde(default)]
    pub req_dex: u32,
    #[serde(default)]
    pub req_int: u32,
    #[serde(default)]
    pub natural_max_level: u32,
}
