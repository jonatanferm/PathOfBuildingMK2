//! Skill data types — what a single granted skill effect looks like, with its level
//! progression. Mirrors `Data/Skills/*.lua`.

use ahash::HashSet;
use indexmap::IndexMap;
use serde::{Deserialize, Deserializer, Serialize};
use serde_json::Value;

/// Deserialise a `Vec<T>` that may appear as an empty `{}` in the JSON. The extractor
/// emits empty Lua tables as empty JSON objects (Lua can't distinguish empty array from
/// empty map), so this normaliser accepts both shapes for "should be a list" fields.
fn de_lenient_vec<'de, D, T>(deserializer: D) -> Result<Vec<T>, D::Error>
where
    D: Deserializer<'de>,
    T: serde::de::DeserializeOwned + Default,
{
    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(Vec::new()),
        Value::Object(m) if m.is_empty() => Ok(Vec::new()),
        Value::Array(a) => a
            .into_iter()
            .map(|v| serde_json::from_value(v).map_err(serde::de::Error::custom))
            .collect(),
        Value::Object(m) => {
            // Sparse int-keyed object — convert to a Vec with index = key - 1.
            // Lua arrays are 1-indexed; the extractor preserves that.
            let mut entries: Vec<(usize, Value)> = m
                .into_iter()
                .filter_map(|(k, v)| k.parse::<usize>().ok().map(|i| (i, v)))
                .collect();
            entries.sort_by_key(|(i, _)| *i);
            let max_idx = entries.last().map(|(i, _)| *i).unwrap_or(0);
            let mut out: Vec<T> = (0..max_idx).map(|_| T::default()).collect();
            for (i, v) in entries {
                let item: T = serde_json::from_value(v).map_err(serde::de::Error::custom)?;
                if i >= 1 && i - 1 < out.len() {
                    out[i - 1] = item;
                } else if i == 0 {
                    // 0-keyed entry — Lua sometimes uses [0]; ignore (or push at 0?)
                }
            }
            Ok(out)
        }
        other => Err(serde::de::Error::custom(format!(
            "expected sequence or empty object, got {other}"
        ))),
    }
}

fn de_lenient_indexmap<'de, D>(deserializer: D) -> Result<IndexMap<String, bool>, D::Error>
where
    D: Deserializer<'de>,
{
    let value: Value = Deserialize::deserialize(deserializer)?;
    match value {
        Value::Null => Ok(IndexMap::new()),
        Value::Array(a) => {
            // The extractor sometimes promotes a Lua dict to a JSON array when keys
            // happen to be 1..N consecutive (e.g. spectre skill types as [true, true]).
            // Recover by indexing back to "1", "2", ...
            Ok(a.into_iter()
                .enumerate()
                .filter_map(|(i, v)| {
                    serde_json::from_value::<bool>(v).ok().map(|b| ((i + 1).to_string(), b))
                })
                .collect())
        }
        Value::Object(m) => m
            .into_iter()
            .map(|(k, v)| {
                serde_json::from_value::<bool>(v)
                    .map(|b| (k, b))
                    .map_err(serde::de::Error::custom)
            })
            .collect(),
        other => Err(serde::de::Error::custom(format!(
            "expected map or array, got {other}"
        ))),
    }
}

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
    #[serde(default, rename = "baseFlags", deserialize_with = "de_lenient_indexmap")]
    pub base_flags: IndexMap<String, bool>,
    /// `[stat_id, scale_per_quality]` entries.
    #[serde(default, rename = "qualityStats", deserialize_with = "de_lenient_vec")]
    pub quality_stats: Vec<Value>,
    /// `[stat_id, value]` entries that apply at every level regardless of gem level.
    #[serde(default, rename = "constantStats", deserialize_with = "de_lenient_vec")]
    pub constant_stats: Vec<Value>,
    /// Ordered stat ids that map to the positional values in each level entry.
    #[serde(default, deserialize_with = "de_lenient_vec")]
    pub stats: Vec<String>,
    /// Stats that should NOT propagate to a minion the gem creates.
    #[serde(default, rename = "notMinionStat", deserialize_with = "de_lenient_vec")]
    pub not_minion_stat: Vec<String>,
    /// SkillType ids (numbers) the skill participates in.
    #[serde(default, rename = "skillTypes", deserialize_with = "de_lenient_indexmap")]
    pub skill_types: IndexMap<String, bool>,
    /// Per-level data, ordered by gem level (level 1 at index 0). Each entry is a JSON
    /// object with positional and named fields — see `SkillLevel` for accessors.
    #[serde(default, deserialize_with = "de_lenient_vec")]
    pub levels: Vec<Value>,
    /// `statMap[stat_id] = [<mod-recording>...]`. Each mod recording is the inert
    /// table that pob-extract emits for a `mod()` call in the skill data — see
    /// `pob_engine::skill::parse_extractor_mod`.
    #[serde(default, rename = "statMap")]
    pub stat_map: indexmap::IndexMap<String, Value>,
    /// `true` for support gems (PoB's top-level `support` flag). Supports
    /// don't have `baseFlags` of their own; their effect is to inject mods
    /// into the linked active skill's calc.
    #[serde(default)]
    pub support: bool,
    /// Skill-type filters that determine which active skills a support can
    /// link to. Empty `addSkillTypes` means "any skill type matching the
    /// support's intent" (e.g. attack-only or spell-only).
    #[serde(default, rename = "addSkillTypes", deserialize_with = "de_lenient_indexmap")]
    pub add_skill_types: IndexMap<String, bool>,
    #[serde(default, rename = "excludeSkillTypes", deserialize_with = "de_lenient_indexmap")]
    pub exclude_skill_types: IndexMap<String, bool>,
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

    /// `levelRequirement` — the character level at which a gem of this gem-level
    /// becomes equippable. Defaults to 1 if absent. Used as the "actor level" for
    /// effectiveness interpolation in `skill_base_damage`, matching PoB's
    /// `actorLevel = level.levelRequirement` fallback.
    pub fn level_requirement(&self, level: u32) -> u32 {
        self.level_data(level)
            .and_then(|v| v.get("levelRequirement"))
            .and_then(Value::as_u64)
            .map(|n| n as u32)
            .unwrap_or(1)
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
