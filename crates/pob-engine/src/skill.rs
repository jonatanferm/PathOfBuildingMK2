//! Active skill model — Phase 3d. Wraps the static `pob_data::Skill` plus the user's
//! choice of level + quality, and provides the helpers `perform_basic_stats` consumes.
//!
//! Cross-reference: `Modules/CalcActiveSkill.lua`. We're nowhere near full coverage —
//! this module exposes enough state to compute basic spell hit damage for a single
//! skill so the user can see DPS move when they allocate damage nodes.

use ahash::HashMap;
use pob_data::{Skill, SkillSet};

/// Library of skills loaded from `data/skills/*.json`. Phase 3d treats this as a
/// read-only lookup keyed by skill id (e.g. `"Arc"`, `"Fireball"`).
#[derive(Debug, Default, Clone)]
pub struct SkillRegistry {
    pub by_id: HashMap<String, Skill>,
}

impl SkillRegistry {
    pub fn from_files(files: impl IntoIterator<Item = SkillSet>) -> Self {
        let mut by_id: HashMap<String, Skill> = HashMap::default();
        for set in files {
            for (id, skill) in set {
                by_id.insert(id, skill);
            }
        }
        Self { by_id }
    }
    pub fn get(&self, id: &str) -> Option<&Skill> {
        self.by_id.get(id)
    }
    pub fn len(&self) -> usize {
        self.by_id.len()
    }
    pub fn is_empty(&self) -> bool {
        self.by_id.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = (&str, &Skill)> {
        self.by_id.iter().map(|(k, v)| (k.as_str(), v))
    }
    /// Filter to skill ids whose data has `grants_active_skill` semantics — i.e.
    /// non-support skills. Phase 3d picks active skills by checking the `baseFlags`
    /// table for `spell` / `attack` / `chaining` / etc., which is conservative.
    pub fn iter_active(&self) -> impl Iterator<Item = (&str, &Skill)> {
        self.by_id
            .iter()
            .filter(|(_, s)| {
                s.base_flags.get("spell").copied().unwrap_or(false)
                    || s.base_flags.get("attack").copied().unwrap_or(false)
            })
            .map(|(k, v)| (k.as_str(), v))
    }
}

/// User's choice of skill, level, and quality.
#[derive(Debug, Clone)]
pub struct MainSkill {
    pub skill_id: String,
    pub level: u32,
    pub quality: u32,
}

impl MainSkill {
    pub fn new(skill_id: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            level: 20,
            quality: 0,
        }
    }
}

/// Per-skill base damage. Phase 3d computes:
///
/// ```text
/// base_min = positional[1] * effectiveness(level)
/// base_max = positional[2] * effectiveness(level)
/// effectiveness(L) = baseEffectiveness + incrementalEffectiveness * (L - 1)
/// ```
///
/// Where `positional[1]` and `positional[2]` are the first two stat values in the level
/// entry, conventionally `spell_minimum_base_<element>_damage` and the corresponding
/// max. This is the formula PoB uses in `Modules/CalcActiveSkill.lua` (where it appears
/// as `effectiveness = grantedEffect.baseEffectiveness + grantedEffect.incrementalEffectiveness * (level - 1)`).
pub fn skill_base_damage(skill: &Skill, level: u32) -> (f64, f64) {
    let level = level.max(1);
    let effectiveness = skill.base_effectiveness
        + skill.incremental_effectiveness * f64::from(level.saturating_sub(1));
    let min = skill.positional(level, 1).unwrap_or(0.0) * effectiveness;
    let max = skill.positional(level, 2).unwrap_or(0.0) * effectiveness;
    (min, max)
}

/// Try to identify the dominant damage element keyword for a spell skill.
/// Reads the skill's `stats` list for the leading `spell_<element>_base_..._damage`
/// pattern. Returns `(stat_name_for_modifier_lookup, damage_label)`.
pub fn skill_damage_element(skill: &Skill) -> Option<(&'static str, &'static str)> {
    for stat in &skill.stats {
        if stat.contains("fire") {
            return Some(("FireDamage", "Fire"));
        }
        if stat.contains("cold") {
            return Some(("ColdDamage", "Cold"));
        }
        if stat.contains("lightning") {
            return Some(("LightningDamage", "Lightning"));
        }
        if stat.contains("chaos") {
            return Some(("ChaosDamage", "Chaos"));
        }
        if stat.contains("physical") {
            return Some(("PhysicalDamage", "Physical"));
        }
    }
    None
}
