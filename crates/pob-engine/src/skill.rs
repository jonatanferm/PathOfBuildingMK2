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

/// PoE's universal damage-by-level constants (`Data/Misc.lua`).
const SKILL_DAMAGE_BASE_EFFECTIVENESS: f64 = 3.885209;
const SKILL_DAMAGE_INCREMENTAL_EFFECTIVENESS: f64 = 0.360246;

/// Per-skill base damage. Mirrors `Modules/CalcTools.lua:198-205` (statInterpolation == 3
/// — the "effectiveness interpolation" path used by spells like Arc, Fireball, etc.):
///
/// ```text
/// available_effectiveness = (SkillDamageBaseEff + SkillDamageIncrEff * (L_char - 1))
///   * baseEffectiveness
///   * (1 + incrementalEffectiveness) ^ (L_char - 1)
/// stat_value = level_data[index] * available_effectiveness
/// ```
///
/// Where `L_char` is the *character* level (not gem level) and `level_data[1..=2]` is
/// the gem-level-indexed scalar pair.
///
/// Returns `(min, max)` damage *before* `damageEffectiveness`. The caller multiplies by
/// `damageEffectiveness` to get the final hit base damage.
pub fn skill_base_damage(skill: &Skill, gem_level: u32, character_level: u32) -> (f64, f64) {
    let gem_level = gem_level.max(1);
    let l = character_level.max(1);
    let l_minus_1 = f64::from(l - 1);
    let base = SKILL_DAMAGE_BASE_EFFECTIVENESS + SKILL_DAMAGE_INCREMENTAL_EFFECTIVENESS * l_minus_1;
    let available_effectiveness = base
        * skill.base_effectiveness.max(1.0)
        * (1.0 + skill.incremental_effectiveness).powf(l_minus_1);
    let min = skill.positional(gem_level, 1).unwrap_or(0.0) * available_effectiveness;
    let max = skill.positional(gem_level, 2).unwrap_or(0.0) * available_effectiveness;
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
