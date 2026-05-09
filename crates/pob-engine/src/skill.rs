//! Active skill model — Phase 3d. Wraps the static `pob_data::Skill` plus the user's
//! choice of level + quality, and provides the helpers `perform_basic_stats` consumes.
//!
//! Cross-reference: `Modules/CalcActiveSkill.lua`. We're nowhere near full coverage —
//! this module exposes enough state to compute basic spell hit damage for a single
//! skill so the user can see DPS move when they allocate damage nodes.

use ahash::HashMap;
use pob_data::{Skill, SkillSet};
use serde::{Deserialize, Serialize};

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

    /// Issue #36: variant-discovery helper. Given a skill id (e.g.
    /// `"Fireball"`), returns every gem-data entry that shares the
    /// same "base" — the default (`Fireball`), the Vaal counterpart
    /// (`VaalFireball`), and any alt-quality reworks
    /// (`FireballAltX` / `FireballAltY` / `FireballAltA` /
    /// `FireballAltB` / `FireballAltC`).
    ///
    /// The returned list is sorted alphabetically and includes the
    /// input id when it exists in the registry. Gems without
    /// alternates simply yield a single-element list.
    pub fn variants_of(&self, skill_id: &str) -> Vec<&str> {
        let base = base_skill_id(skill_id);
        let mut out: Vec<&str> = self
            .by_id
            .keys()
            .filter(|k| base_skill_id(k) == base)
            .map(String::as_str)
            .collect();
        out.sort_unstable();
        out
    }
}

/// Strip Vaal- prefix and AltX/AltY/AltA/AltB/AltC suffixes to recover
/// a canonical "base" skill id used by `variants_of`. Public so the UI
/// can label a variant group by its base name.
#[must_use]
pub fn base_skill_id(skill_id: &str) -> &str {
    // Trailing alt-quality suffix.
    for suffix in ["AltX", "AltY", "AltA", "AltB", "AltC"] {
        if let Some(stripped) = skill_id.strip_suffix(suffix) {
            return base_skill_id(stripped);
        }
    }
    // Vaal- prefix.
    if let Some(stripped) = skill_id.strip_prefix("Vaal") {
        // Some gems are *natively* Vaal-named (the side-cast Vaal of an
        // active skill). Strip the prefix only when the rest is also a
        // valid identifier — the recursive call handles further
        // suffixes if any.
        return base_skill_id(stripped);
    }
    skill_id
}

/// Issue #36: alt-quality variant selector. Each gem can be flagged
/// `Default` / `Anomalous` / `Divergent` / `Phantasmal`; PoB's XML
/// persists the choice as `<Gem ... qualityId="Anomalous"/>`. Maps onto
/// the upstream Alt-skill suffix convention:
///
/// - `Default`    → the bare skill id (e.g. `"Arc"`)
/// - `Anomalous`  → the `AltX` variant (`"ArcAltX"`)
/// - `Divergent`  → the `AltY` variant (`"ArcAltY"`)
/// - `Phantasmal` → the `AltZ` variant (`"ArcAltZ"`)
///
/// When the requested variant doesn't exist in the registry the engine
/// falls back to the gem's bare id — the alt picker stays visible but
/// has no effect. Test fixture: a Phantasmal-quality Cleave with no
/// `CleaveAltZ` shipped should compute identically to Default.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum QualityId {
    #[default]
    Default,
    Anomalous,
    Divergent,
    Phantasmal,
}

impl QualityId {
    /// PoB's persisted attribute name on `<Gem qualityId="…"/>`. Used
    /// by both `pob_export` and the build-file format so the wire
    /// representation stays stable.
    pub fn as_pob_name(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Anomalous => "Anomalous",
            Self::Divergent => "Divergent",
            Self::Phantasmal => "Phantasmal",
        }
    }

    /// Inverse of `as_pob_name`. Recognises both the canonical PoB
    /// names and the integer aliases (`"0"`..`"3"`) PoB legacy data
    /// occasionally uses. Anything unrecognised maps to `Default`
    /// — keeps imports tolerant.
    pub fn from_pob_name(name: &str) -> Self {
        match name {
            "Anomalous" | "1" => Self::Anomalous,
            "Divergent" | "2" => Self::Divergent,
            "Phantasmal" | "3" => Self::Phantasmal,
            _ => Self::Default,
        }
    }

    /// Skill-id suffix this quality maps onto. `None` for `Default`
    /// — the engine then uses the gem's bare id. The Alt-suffix
    /// convention is upstream PoB's: `AltX` / `AltY` / `AltZ` are
    /// reserved for the three alt-quality reworks of an active
    /// skill, mirrored in `Data/Skills/*.lua` and the corresponding
    /// extracted JSON.
    pub fn alt_suffix(self) -> Option<&'static str> {
        match self {
            Self::Default => None,
            Self::Anomalous => Some("AltX"),
            Self::Divergent => Some("AltY"),
            Self::Phantasmal => Some("AltZ"),
        }
    }

    /// Human-readable label for the UI dropdown.
    pub fn display(self) -> &'static str {
        match self {
            Self::Default => "Default",
            Self::Anomalous => "Anomalous",
            Self::Divergent => "Divergent",
            Self::Phantasmal => "Phantasmal",
        }
    }

    /// All four variants, in display order.
    pub fn all() -> [Self; 4] {
        [
            Self::Default,
            Self::Anomalous,
            Self::Divergent,
            Self::Phantasmal,
        ]
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
    /// Issue #36: alt-quality variant. Defaults to `QualityId::Default`
    /// (legacy gems), so existing builds round-trip unchanged.
    pub quality_id: QualityId,
    pub enabled: bool,
}

impl MainSkill {
    pub fn new(skill_id: impl Into<String>) -> Self {
        Self {
            skill_id: skill_id.into(),
            level: 20,
            quality: 0,
            quality_id: QualityId::Default,
            enabled: true,
        }
    }
}

/// Resolve the actual `Skill` to consult for `qualityStats` given a
/// `(skill_id, quality_id)` pair. When the user picks `Anomalous` /
/// `Divergent` / `Phantasmal` we look up `<base>AltX` / `<base>AltY` /
/// `<base>AltZ` in the registry; if it's not present (most skills
/// don't ship alt-variant data), returns the bare-id skill so the
/// caller still gets the default `qualityStats`.
///
/// Returns `(skill_for_quality_stats, alt_resolved)` — the second
/// flag tells the caller whether the alt-variant was actually
/// consulted. Used by the UI to grey out alt-quality picks that have
/// no upstream data.
pub fn skill_for_quality<'a>(
    registry: &'a SkillRegistry,
    skill_id: &str,
    quality_id: QualityId,
) -> (Option<&'a Skill>, bool) {
    if quality_id == QualityId::Default {
        return (registry.get(skill_id), false);
    }
    let Some(suffix) = quality_id.alt_suffix() else {
        return (registry.get(skill_id), false);
    };
    // The bare skill id is the base — we want `<base><suffix>`. If the
    // user already started from an Alt id, reuse the existing `base_skill_id`
    // helper so we don't double-suffix (e.g. `"ArcAltX" + "AltY"` is wrong;
    // the right answer is `"ArcAltY"`).
    let base = base_skill_id(skill_id);
    let alt_id = format!("{base}{suffix}");
    if let Some(s) = registry.get(&alt_id) {
        return (Some(s), true);
    }
    // Fallback to the bare skill — alt picker has no effect for this gem.
    (registry.get(skill_id), false)
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
///
/// Issue #36: when `quality_source` is `Some(other_skill)` (the alt-quality
/// variant resolved via `skill_for_quality`), this function consults the
/// alt skill's `quality_stats` instead of `skill.quality_stats`. The
/// `constantStats` always come from the active skill itself — alt quality
/// only swaps the *quality* line, not the constant baseline. Pass `None`
/// for the legacy "use the active skill's own qualityStats" path.
pub fn iter_skill_stats<'a>(
    skill: &'a Skill,
    quality: u32,
    quality_source: Option<&'a Skill>,
) -> impl Iterator<Item = (String, f64)> + 'a {
    let q = f64::from(quality);
    let constant = skill.constant_stats.iter().filter_map(|v| {
        let arr = v.as_array()?;
        let id = arr.first()?.as_str()?.to_owned();
        let val = arr.get(1)?.as_f64()?;
        Some((id, val))
    });
    let quality_skill = quality_source.unwrap_or(skill);
    let quality_iter = quality_skill.quality_stats.iter().filter_map(move |v| {
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
    let flags_bits = obj
        .get("flags")
        .and_then(serde_json::Value::as_u64)
        .unwrap_or(0) as u32;
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
    let mut tag_keys: Vec<u32> = obj.keys().filter_map(|k| k.parse::<u32>().ok()).collect();
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
    skill_mods_with_quality(skill, quality, None)
}

/// Issue #36: alt-quality variant of `skill_mods`. When `quality_source`
/// is `Some(alt_skill)` (the resolved `<base>AltX/Y/Z` variant) the
/// `quality_stats` line is sourced from that alt skill instead of the
/// active skill's own `quality_stats`. The `statMap` still belongs to
/// the active skill so the alt's stat ids resolve through the same set
/// of mod recordings.
///
/// Falls back to `global_skill_stat_mods` for stats not in the active
/// skill's per-skill `statMap` — same path as the legacy `skill_mods`.
pub fn skill_mods_with_quality(
    skill: &Skill,
    quality: u32,
    quality_source: Option<&Skill>,
) -> Vec<crate::Mod> {
    let mut out = Vec::new();
    for (stat_id, value) in iter_skill_stats(skill, quality, quality_source) {
        // Per-skill statMap is the primary source. Each entry is an array of mod
        // recordings.
        if let Some(arr) = skill_stat_map(skill)
            .get(&stat_id)
            .and_then(|v| v.as_array())
        {
            for entry in arr {
                if let Some(mut m) = parse_extractor_mod(entry, value) {
                    m.source = Some(crate::Source::Skill(skill.name.clone()));
                    out.push(m);
                }
            }
            continue;
        }
        // Fallback: PoB's global SkillStatMap (`src/Data/SkillStatMap.lua`) catches
        // common stats that aren't carried per-skill — currently only the small set
        // of ailment-chance stats most skills use. This avoids re-extracting that
        // entire 1.5k-line table; we hand-port the entries we need.
        for m in global_skill_stat_mods(&stat_id, value, &skill.name) {
            out.push(m);
        }
    }
    out
}

/// Hand-ported subset of PoB's `Data/SkillStatMap.lua`. Returns the BASE/INC mods a
/// stat id contributes when a skill's per-skill statMap doesn't already own it.
/// Currently scoped to ailment-chance entries used by Phase 6d. Add more here as
/// the calc engine starts consuming additional stat names.
fn global_skill_stat_mods(stat_id: &str, value: f64, skill_name: &str) -> Vec<crate::Mod> {
    use crate::{Mod, ModType, ModValue, Source};
    use pob_data::{KeywordFlag, ModFlag};

    let mk = |name: &str, kind: ModType| Mod {
        name: name.to_owned(),
        kind,
        value: ModValue::Number(value),
        flags: ModFlag::empty(),
        keyword_flags: KeywordFlag::empty(),
        source: Some(Source::Skill(skill_name.to_owned())),
        tags: smallvec::SmallVec::new(),
    };

    match stat_id {
        // Ailment chance from skill data (e.g. Fireball's 25% base chance to ignite).
        "base_chance_to_ignite_%" | "always_ignite" => vec![mk("IgniteChance", ModType::Base)],
        "base_chance_to_poison_on_hit_%" | "global_poison_on_hit" => {
            vec![mk("PoisonChance", ModType::Base)]
        }
        "bleed_on_hit_with_attacks_%" => vec![mk("BleedChance", ModType::Base)],
        "base_chance_to_shock_%" | "always_shock" => vec![mk("EnemyShockChance", ModType::Base)],
        "base_chance_to_freeze_%" | "always_freeze" => vec![mk("EnemyFreezeChance", ModType::Base)],
        "chance_to_freeze_shock_ignite_%" => vec![
            mk("EnemyFreezeChance", ModType::Base),
            mk("EnemyShockChance", ModType::Base),
            mk("IgniteChance", ModType::Base),
        ],

        // Ailment-flavoured `more` damage multipliers PoB labels as "ailment damage final".
        "active_skill_bleeding_damage_+%_final" => vec![mk("BleedDamage", ModType::More)],
        "active_skill_poison_damage_+%_final" => vec![mk("PoisonDamage", ModType::More)],
        "active_skill_ignite_damage_+%_final" => vec![mk("IgniteDamage", ModType::More)],
        "active_skill_poison_duration_+%_final" => vec![mk("PoisonDuration", ModType::More)],

        // Faster ailments — PoB key for bleed is `BleedFaster` etc.
        "faster_bleed_%" => vec![mk("BleedFaster", ModType::Inc)],

        _ => Vec::new(),
    }
}

/// Produce the buff mods an aura/herald skill grants its allies — i.e. the
/// `Mod`s a player picks up while the aura is reserved. This walks the same
/// statMap as `skill_mods` but also iterates `stats[]` × per-level positionals
/// (Hatred's `physical_damage_%_to_add_as_cold = 39 @ L20`, Wrath's
/// `wrath_aura_spell_lightning_damage_+%_final = 21 @ L20`, etc.). Per-level
/// values are taken raw — PoB's `statInterpolation` types 1 (linear) and 3
/// (effectiveness) both end up at the same number in PoB's pre-extracted
/// JSON, since the extractor already resolves the interpolation during data
/// generation. Each emitted `Mod` is sourced as `Source::Skill(skill.name)`.
pub fn aura_buff_mods(skill: &Skill, gem_level: u32, quality: u32) -> Vec<crate::Mod> {
    aura_buff_mods_with_quality(skill, gem_level, quality, None)
}

/// Issue #36: alt-quality variant of `aura_buff_mods`. Substitutes the
/// alt-quality `quality_stats` exactly like `skill_mods_with_quality`;
/// per-level `stats[]` positionals stay with the active skill (so the
/// aura's per-level numbers don't get clobbered by an alt with a
/// different progression).
pub fn aura_buff_mods_with_quality(
    skill: &Skill,
    gem_level: u32,
    quality: u32,
    quality_source: Option<&Skill>,
) -> Vec<crate::Mod> {
    let mut out = skill_mods_with_quality(skill, quality, quality_source);
    // Per-level positional stats. The extractor-emitted values in `levels[L][i+1]`
    // are already post-interpolation, so we use them directly.
    for (i, stat_id) in skill.stats.iter().enumerate() {
        let Some(value) = skill.positional(gem_level, (i + 1) as u32) else {
            continue;
        };
        let Some(stat_map) = skill_stat_map(skill).get(stat_id) else {
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
