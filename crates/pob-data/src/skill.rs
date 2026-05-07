//! Skill data types — what a single granted skill effect looks like, with its level
//! progression. Mirrors `Data/Skills/*.lua`.

use ahash::HashSet;
use indexmap::IndexMap;
use serde::{Deserialize, Serialize};
use serde_json::Value;

pub type SkillSet = IndexMap<String, Skill>;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Skill {
    /// Human-readable name (e.g. `"Arc"`).
    pub name: String,
    /// Base gem name. Often same as `name` for non-alternate-quality skills.
    #[serde(default, rename = "baseTypeName")]
    pub base_type_name: String,
    /// Color: 1 = Str (red), 2 = Dex (green), 3 = Int (blue).
    #[serde(default)]
    pub color: u8,
    #[serde(default)]
    pub description: String,
    #[serde(default, rename = "castTime")]
    pub cast_time: f32,
    /// Mapping `stat_id_string -> bool` for properties like `spell`, `chaining`,
    /// `attack`, `area`, `melee`. The presence of a key means that flag is set.
    #[serde(default, rename = "baseFlags")]
    pub base_flags: IndexMap<String, bool>,
    /// `[stat_id, scale_per_quality]` entries.
    #[serde(default, rename = "qualityStats")]
    pub quality_stats: Vec<Value>,
    /// `[stat_id, value]` entries that apply at every level regardless of gem level.
    #[serde(default, rename = "constantStats")]
    pub constant_stats: Vec<Value>,
    /// Ordered stat ids that map to the positional values in each level entry.
    #[serde(default)]
    pub stats: Vec<String>,
    /// Stats that should NOT propagate to a minion the gem creates.
    #[serde(default, rename = "notMinionStat")]
    pub not_minion_stat: Vec<String>,
    /// SkillType ids (numbers) the skill participates in.
    #[serde(default, rename = "skillTypes")]
    pub skill_types: IndexMap<String, bool>,
    /// Per-level data, ordered by gem level (level 1 at index 0). Each entry is a JSON
    /// object with positional and named fields — see `SkillLevel` for accessors.
    #[serde(default)]
    pub levels: Vec<Value>,
    /// Effectiveness multiplier baseline (PoB's `baseEffectiveness`).
    #[serde(default, rename = "baseEffectiveness")]
    pub base_effectiveness: f64,
    #[serde(default, rename = "incrementalEffectiveness")]
    pub incremental_effectiveness: f64,
}

impl Skill {
    /// Set of skill-type ids parsed as `u8`s. Allocates — call sparingly.
    pub fn skill_type_ids(&self) -> HashSet<u8> {
        let mut out: HashSet<u8> = HashSet::default();
        for (k, v) in &self.skill_types {
            if !*v {
                continue;
            }
            if let Ok(n) = k.parse::<u8>() {
                out.insert(n);
            }
        }
        out
    }

    /// Get the level entry for a 1-based gem level, clamped to the available range.
    pub fn level_data(&self, level: u32) -> Option<&Value> {
        if self.levels.is_empty() {
            return None;
        }
        let idx = (level.saturating_sub(1) as usize).min(self.levels.len() - 1);
        self.levels.get(idx)
    }

    /// `damageEffectiveness` from a level entry (default 1.0).
    pub fn damage_effectiveness(&self, level: u32) -> f64 {
        self.level_data(level)
            .and_then(|v| v.get("damageEffectiveness"))
            .and_then(Value::as_f64)
            .unwrap_or(1.0)
    }

    /// `critChance` (in percent, 0..100).
    pub fn crit_chance(&self, level: u32) -> f64 {
        self.level_data(level)
            .and_then(|v| v.get("critChance"))
            .and_then(Value::as_f64)
            .unwrap_or(5.0)
    }

    /// `cost` table; returns the matching resource cost or 0.
    pub fn cost(&self, level: u32, resource: &str) -> f64 {
        self.level_data(level)
            .and_then(|v| v.get("cost"))
            .and_then(|c| c.get(resource))
            .and_then(Value::as_f64)
            .unwrap_or(0.0)
    }

    /// Get a positional stat value from the level entry by 1-based index.
    /// PoB extracts these as numeric string keys (`"1"`, `"2"`, …) in JSON.
    pub fn positional(&self, level: u32, idx: u32) -> Option<f64> {
        let entry = self.level_data(level)?;
        let key = idx.to_string();
        entry.get(&key).and_then(Value::as_f64)
    }
}
