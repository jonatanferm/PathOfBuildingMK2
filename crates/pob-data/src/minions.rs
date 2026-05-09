//! Per-minion-type base stats — mirrors PoB's `Data/Minions.lua`.
//!
//! Foundation slice for [#20](https://github.com/jonatanferm/PathOfBuildingMK2/issues/20)
//! (parallel minion calc env). Captures the shared base for every summonable creature
//! (Raised Zombie, Skeleton Warrior, every Spectre type, every Golem, …) so a future
//! `MinionState` perform pass has the data it needs to compute Minion Life / Resists /
//! DPS without re-reading PoB's Lua at runtime.
//!
//! The `mod_list` recordings are kept as raw `serde_json::Value`s for now; they share
//! the same shape as skill `statMap` entries (`{__kind: "mod", name, type, value, …}`)
//! so [`crate::skill::parse_extractor_mod`] can decode them once the perform pass needs
//! them.

use indexmap::IndexMap;
use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Default, Serialize, Deserialize)]
pub struct MinionData {
    pub minions: IndexMap<String, MinionType>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct MinionType {
    pub name: String,
    #[serde(default)]
    pub monster_tags: Vec<String>,
    /// Life multiplier vs the per-area base (PoB applies this against the active monster
    /// life ladder; the runtime calc is `area_base_life × life × player-side mods`).
    #[serde(default)]
    pub life: f64,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub energy_shield: Option<f64>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub armour: Option<f64>,
    /// Slice 14 of #20: evasion multiplier vs the per-level monster evasion
    /// table. Defaults to `None` for minions whose Lua entry omits it, in
    /// which case PoB treats the multiplier as 1.0. Mirrors the same
    /// `(armour or 1)` pattern PoB uses for armour.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub evasion: Option<f64>,
    #[serde(default)]
    pub fire_resist: i32,
    #[serde(default)]
    pub cold_resist: i32,
    #[serde(default)]
    pub lightning_resist: i32,
    #[serde(default)]
    pub chaos_resist: i32,
    /// Per-hit damage multiplier; `damage_spread` is the ± fraction.
    #[serde(default)]
    pub damage: f64,
    #[serde(default)]
    pub damage_spread: f64,
    #[serde(default)]
    pub attack_time: f64,
    #[serde(default)]
    pub attack_range: f64,
    #[serde(default)]
    pub accuracy: f64,
    /// `ActiveZombieLimit` etc. — drives how many concurrent minions can exist.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub limit: Option<String>,
    /// Skill ids the minion casts (the perform pass picks one as the minion's main skill).
    #[serde(default)]
    pub skill_list: Vec<String>,
    /// Raw `mod()` recordings. Same shape as skill `statMap` entries; decode with
    /// `pob_engine::skill::parse_extractor_mod` when the perform pass needs them.
    #[serde(default)]
    pub mod_list: Vec<serde_json::Value>,
    /// Slice 9 of #20: which monster-life table to use (`None` = ally life table,
    /// `"AltLife1"` / `"AltLife2"` for the variants). Spectres carry this; standard
    /// minions in `Data/Minions.lua` don't.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub life_scaling: Option<String>,
    /// Spectre-only: the weapon-base type the spectre wields (e.g. `"Wand"`).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_type1: Option<String>,
    /// Spectre-only: off-hand / shield slot weapon type.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub weapon_type2: Option<String>,
    /// Spectre-only: when `true`, the spectre's per-hit damage doesn't scale with its
    /// attack speed (used for skills that fire at a fixed cadence).
    #[serde(default)]
    pub base_damage_ignores_attack_speed: bool,
}

pub fn load_minions(json: &str) -> serde_json::Result<MinionData> {
    serde_json::from_str(json)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn round_trips_minimal_payload() {
        let json = r#"{
          "minions": {
            "RaisedZombie": {
              "name": "Raised Zombie",
              "monster_tags": ["undead", "zombie", "melee"],
              "life": 3.75,
              "fire_resist": 40, "cold_resist": 40, "lightning_resist": 40, "chaos_resist": 20,
              "damage": 1.65, "damage_spread": 0.4,
              "attack_time": 1.17, "attack_range": 11.0, "accuracy": 3.4,
              "limit": "ActiveZombieLimit",
              "skill_list": ["Melee", "ZombieSlam"]
            }
          }
        }"#;
        let data = load_minions(json).expect("decode");
        assert_eq!(data.minions.len(), 1);
        let zombie = data.minions.get("RaisedZombie").unwrap();
        assert_eq!(zombie.name, "Raised Zombie");
        assert_eq!(zombie.life, 3.75);
        assert_eq!(zombie.fire_resist, 40);
        assert_eq!(zombie.limit.as_deref(), Some("ActiveZombieLimit"));
        assert_eq!(zombie.skill_list.len(), 2);
        assert!(zombie.energy_shield.is_none());
        assert!(zombie.mod_list.is_empty());
    }

    #[test]
    fn defaults_cover_optional_fields() {
        let data: MinionData = serde_json::from_str(r#"{"minions":{"X":{"name":"X"}}}"#).unwrap();
        let x = data.minions.get("X").unwrap();
        assert_eq!(x.name, "X");
        assert!(x.monster_tags.is_empty());
        assert_eq!(x.life, 0.0);
        assert!(x.armour.is_none());
        assert!(x.limit.is_none());
    }
}
