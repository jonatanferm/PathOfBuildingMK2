//! Item base types: weapons, armours, flasks, jewels, accessories.
//!
//! Mirrors the structure of `src/Data/Bases/*.lua` in the upstream PoB repo.

use ahash::HashSet;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

/// One file under `Data/Bases/` produces one of these. The string key is the canonical
/// in-game item name (e.g. `"Sabre"`, `"Plate Vest"`).
pub type ItemBaseSet = IndexMap<String, ItemBase>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ItemBase {
    /// E.g. "One Handed Sword", "Body Armour", "Flask".
    #[serde(rename = "type")]
    pub r#type: String,
    /// Sub-classification. E.g. "Armour" / "Evasion" / "Energy Shield" for body, "Life" / "Mana" for flasks.
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub sub_type: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub socket_limit: Option<u8>,
    #[serde(default)]
    pub tags: HashSet<String>,
    #[serde(default, skip_serializing_if = "IndexMap::is_empty")]
    pub influence_tags: IndexMap<String, String>,
    #[serde(skip_serializing_if = "Option::is_none", default)]
    pub implicit: Option<String>,
    /// One sub-array per implicit mod listing tags (e.g. `["resource", "mana"]`).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub implicit_mod_types: Vec<Vec<String>>,
    #[serde(default)]
    pub req: ItemReq,
    /// Equipment-class-specific stats.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon: Option<WeaponStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub armour: Option<ArmourStats>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub flask: Option<FlaskStats>,
}

#[derive(Debug, Clone, Copy)]
pub enum ItemBaseKind<'a> {
    Weapon(&'a WeaponStats),
    Armour(&'a ArmourStats),
    Flask(&'a FlaskStats),
    Accessory,
}

impl ItemBase {
    #[must_use]
    pub fn kind(&self) -> ItemBaseKind<'_> {
        if let Some(w) = self.weapon.as_ref() {
            ItemBaseKind::Weapon(w)
        } else if let Some(a) = self.armour.as_ref() {
            ItemBaseKind::Armour(a)
        } else if let Some(f) = self.flask.as_ref() {
            ItemBaseKind::Flask(f)
        } else {
            ItemBaseKind::Accessory
        }
    }
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ItemReq {
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub level: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub str: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub dex: Option<u32>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub int: Option<u32>,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct WeaponStats {
    #[serde(default)]
    pub physical_min: f32,
    #[serde(default)]
    pub physical_max: f32,
    #[serde(default)]
    pub crit_chance_base: f32,
    #[serde(default)]
    pub attack_rate_base: f32,
    #[serde(default)]
    pub range: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct ArmourStats {
    #[serde(default)]
    pub armour_base_min: f32,
    #[serde(default)]
    pub armour_base_max: f32,
    #[serde(default)]
    pub evasion_base_min: f32,
    #[serde(default)]
    pub evasion_base_max: f32,
    #[serde(default)]
    pub energy_shield_base_min: f32,
    #[serde(default)]
    pub energy_shield_base_max: f32,
    #[serde(default)]
    pub ward_base_min: f32,
    #[serde(default)]
    pub ward_base_max: f32,
    #[serde(default)]
    pub block_chance_base: f32,
    #[serde(default)]
    pub movement_penalty: f32,
}

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct FlaskStats {
    #[serde(default)]
    pub life: Option<f32>,
    #[serde(default)]
    pub mana: Option<f32>,
    #[serde(default)]
    pub duration: f32,
    #[serde(default)]
    pub charges_used: u32,
    #[serde(default)]
    pub charges_max: u32,
}
