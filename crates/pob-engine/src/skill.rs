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
                    || s.base_flags.get("aura").copied().unwrap_or(false)
                    || s.base_flags.get("herald").copied().unwrap_or(false)
            })
            .map(|(k, v)| (k.as_str(), v))
    }
}

/// User's choice of skill, level, and quality. Doubles as the per-gem entry in
/// `SocketGroup`, with `enabled` letting the UI toggle individual supports
/// without removing them.
#[derive(Debug, Clone)]
pub struct MainSkill {
    pub skill_id: String,
    pub level: u32,
    pub quality: u32,
    pub enabled: bool,
}

impl MainSkill {
    pub fn new(skill_id: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            level: 20,
            quality: 0,
            enabled: true,
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
    // PoB uses `actorLevel = skillInstance.actorLevel or level.levelRequirement
    // or 1` (CalcTools.lua:198-205). The character level is NOT used directly —
    // this was a long-standing bug that caused our DPS to be ~2.5x too high
    // since (1 + incEff)^(L_char - 1) blows up at L_char=90 vs L_char=70.
    let actor_level = skill.level_requirement(gem_level).max(1);
    let l_minus_1 = f64::from(actor_level - 1);
    let base = SKILL_DAMAGE_BASE_EFFECTIVENESS + SKILL_DAMAGE_INCREMENTAL_EFFECTIVENESS * l_minus_1;
    let available_effectiveness = base
        * skill.base_effectiveness.max(1.0)
        * (1.0 + skill.incremental_effectiveness).powf(l_minus_1);
    let min = skill.positional(gem_level, 1).unwrap_or(0.0) * available_effectiveness;
    let max = skill.positional(gem_level, 2).unwrap_or(0.0) * available_effectiveness;
    let _ = character_level;
    (min, max)
}

/// Iterate (stat_id, stat_value) pairs from constantStats + qualityStats *(quality
/// scales linearly so we contribute `quality_pct × scale_per_quality` per entry)*.
/// `stats` (positional, level-indexed via statInterpolation) is *not* included here —
/// that's the per-level damage data which the dedicated `skill_base_damage` handles.
pub fn iter_skill_stats(
    skill: &Skill,
    quality: u32,
) -> impl Iterator<Item = (String, f64)> + '_ {
    let q = f64::from(quality);
    let constant = skill.constant_stats.iter().filter_map(|v| {
        let arr = v.as_array()?;
        let id = arr.first()?.as_str()?.to_owned();
        let val = arr.get(1)?.as_f64()?;
        Some((id, val))
    });
    let quality_iter = skill.quality_stats.iter().filter_map(move |v| {
        let arr = v.as_array()?;
        let id = arr.first()?.as_str()?.to_owned();
        let scale = arr.get(1)?.as_f64()?;
        Some((id, scale * q))
    });
    constant.chain(quality_iter)
}

/// Convert the inert `__kind: "mod"` table that pob-extract emits for a `mod(...)`
/// call into a real `crate::Mod`. Returns `None` if the value isn't a mod recording.
pub fn parse_extractor_mod(v: &serde_json::Value, value: f64) -> Option<crate::Mod> {
    use crate::{Mod, ModType, ModValue, Tag, TagKind};
    use pob_data::{KeywordFlag, ModFlag};

    let obj = v.as_object()?;
    if obj.get("__kind")?.as_str()? != "mod" {
        return None;
    }
    let name = obj.get("name")?.as_str()?.to_owned();
    let kind_str = obj.get("type")?.as_str()?;
    let kind = match kind_str {
        "BASE" => ModType::Base,
        "INC" => ModType::Inc,
        "MORE" => ModType::More,
        "OVERRIDE" => ModType::Override,
        "FLAG" => ModType::Flag,
        "LIST" => ModType::List,
        _ => return None,
    };
    let flags_bits = obj.get("flags").and_then(serde_json::Value::as_u64).unwrap_or(0) as u32;
    let kw_bits = obj
        .get("keywordFlags")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
    let mut m = Mod {
        name,
        kind,
        value: ModValue::Number(value),
        flags: ModFlag::from_bits_retain(flags_bits),
        keyword_flags: KeywordFlag::from_bits_retain(kw_bits),
        source: None,
        tags: smallvec::SmallVec::new(),
    };
    // Trailing tags are stored as numeric-key entries on the object: "1", "2", ...
    let mut tag_keys: Vec<u32> = obj
        .keys()
        .filter_map(|k| k.parse::<u32>().ok())
        .collect();
    tag_keys.sort_unstable();
    for k in tag_keys {
        let tag_v = &obj[&k.to_string()];
        let Some(tag_obj) = tag_v.as_object() else {
            continue;
        };
        let Some(t) = tag_obj.get("type").and_then(serde_json::Value::as_str) else {
            continue;
        };
        let kind_opt: Option<TagKind> = match t {
            "Condition" => tag_obj
                .get("var")
                .and_then(serde_json::Value::as_str)
                .map(|var| TagKind::Condition {
                    var: var.to_owned(),
                    neg: tag_obj
                        .get("neg")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                }),
            "Multiplier" => tag_obj
                .get("var")
                .and_then(serde_json::Value::as_str)
                .map(|var| TagKind::Multiplier {
                    var: var.to_owned(),
                    limit: tag_obj.get("limit").and_then(serde_json::Value::as_f64),
                    limit_total: tag_obj
                        .get("limitTotal")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                    div: tag_obj.get("div").and_then(serde_json::Value::as_f64),
                    actor: tag_obj
                        .get("actor")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                }),
            "PerStat" => tag_obj
                .get("stat")
                .and_then(serde_json::Value::as_str)
                .map(|stat| TagKind::PerStat {
                    stat: stat.to_owned(),
                    div: tag_obj.get("div").and_then(serde_json::Value::as_f64),
                    actor: tag_obj
                        .get("actor")
                        .and_then(serde_json::Value::as_str)
                        .map(str::to_owned),
                }),
            "ActorCondition" => {
                let var = tag_obj.get("var").and_then(serde_json::Value::as_str)?;
                let actor = tag_obj
                    .get("actor")
                    .and_then(serde_json::Value::as_str)?
                    .to_owned();
                Some(TagKind::ActorCondition {
                    var: var.to_owned(),
                    actor,
                    neg: tag_obj
                        .get("neg")
                        .and_then(serde_json::Value::as_bool)
                        .unwrap_or(false),
                })
            }
            _ => Some(TagKind::Unknown(tag_v.clone())),
        };
        if let Some(kind) = kind_opt {
            m.tags.push(Tag { kind });
        }
    }
    Some(m)
}

/// Produce all the `Mod`s a skill grants from its statMap + constantStats + qualityStats
/// data. Each emitted Mod is sourced as `Source::Skill(skill.name)`.
pub fn skill_mods(skill: &Skill, quality: u32) -> Vec<crate::Mod> {
    let mut out = Vec::new();
    for (stat_id, value) in iter_skill_stats(skill, quality) {
        // statMap is keyed by stat id; values are arrays of mod recordings.
        let Some(stat_map) = skill_stat_map(skill).get(&stat_id) else {
            continue;
        };
        let Some(arr) = stat_map.as_array() else {
            continue;
        };
        for entry in arr {
            if let Some(mut m) = parse_extractor_mod(entry, value) {
                m.source = Some(crate::Source::Skill(skill.name.clone()));
                out.push(m);
            }
        }
    }
    out
}

fn skill_stat_map(skill: &Skill) -> &indexmap::IndexMap<String, serde_json::Value> {
    &skill.stat_map
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
