//! The `Mod` data carrier — Rust port of PoB's mod tables.
//!
//! Cross-reference: `Modules/ModTools.lua:20-46` (canonical helper), `Classes/ModStore.lua`
//! (`EvalMod` is the eventual consumer).
//!
//! ## Shape
//!
//! Every modifier carries:
//! 1. A `name` — the stat being modified (`"Life"`, `"FireDamage"`, …).
//! 2. A `kind` (`type` in Lua — renamed to avoid the keyword clash). One of `Base`, `Inc`,
//!    `More`, `Override`, `Flag`, `List`, `Max`, `Min`.
//! 3. A `value` — usually a scalar; sometimes a damage range, list element, or override
//!    payload.
//! 4. `flags` and `keyword_flags` — bitsets for skill / damage applicability.
//! 5. `source` — provenance for the breakdown view.
//! 6. Zero or more `Tag`s — conditional / scaling / filtering modifiers that `EvalMod`
//!    resolves at query time.

use pob_data::{KeywordFlag, ModFlag};
use serde::{Deserialize, Serialize};
use smallvec::SmallVec;

/// Modifier "type" in PoB-speak. Renamed to avoid the `type` keyword.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Serialize, Deserialize)]
#[serde(rename_all = "UPPERCASE")]
pub enum ModType {
    /// Additive base value. Multiple `Base` mods sum.
    Base,
    /// Increased / reduced (additive percent). Multiple `Inc` mods sum, then a single
    /// (1 + inc/100) factor is applied.
    Inc,
    /// More / less (multiplicative percent). Each `More` mod becomes its own
    /// (1 + more/100) factor.
    More,
    /// Replaces the computed value. The first `Override` wins.
    Override,
    /// Boolean — at least one `Flag` mod resolved truthy means the flag is set.
    Flag,
    /// Unstructured list; consumer interprets values.
    List,
    /// Cap (upper bound, take min of all `Max` values).
    Max,
    /// Floor (lower bound, take max of all `Min` values).
    Min,
}

/// Most mods carry a single number. A few carry a damage range or a structured payload —
/// e.g. an `Override` with a string key, or a `List` element with arbitrary content.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(untagged)]
pub enum ModValue {
    Number(f64),
    /// Inclusive damage range, used for `Adds N to M <element> Damage`.
    Range {
        min: f64,
        max: f64,
    },
    /// Boolean flag (used with `ModType::Flag`).
    Bool(bool),
    /// Free-form string payload (used with `ModType::List` for keystone names, etc.).
    Str(String),
}

impl ModValue {
    pub fn as_f64(&self) -> Option<f64> {
        match self {
            ModValue::Number(n) => Some(*n),
            ModValue::Range { min, max } => Some((min + max) * 0.5),
            ModValue::Bool(b) => Some(if *b { 1.0 } else { 0.0 }),
            ModValue::Str(_) => None,
        }
    }

    pub fn as_range(&self) -> Option<(f64, f64)> {
        match self {
            ModValue::Range { min, max } => Some((*min, *max)),
            ModValue::Number(n) => Some((*n, *n)),
            _ => None,
        }
    }
}

impl From<f64> for ModValue {
    fn from(n: f64) -> Self {
        ModValue::Number(n)
    }
}
impl From<i32> for ModValue {
    fn from(n: i32) -> Self {
        ModValue::Number(f64::from(n))
    }
}
impl From<bool> for ModValue {
    fn from(b: bool) -> Self {
        ModValue::Bool(b)
    }
}

/// Where a mod came from. Used for breakdowns and source-filtered queries.
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub enum Source {
    Tree,
    Passive(u32),
    Ascendancy(String),
    /// Slot-indexed item (1-based, like PoB).
    Item(u32),
    Skill(String),
    Buff(String),
    Config(String),
    /// Anything else (gives a string for forensic display).
    Other(String),
}

impl Source {
    /// PoB convention: extract the leading classifier (`"Item"`, `"Tree"`, `"Buff"`, …).
    pub fn category(&self) -> &'static str {
        match self {
            Source::Tree | Source::Passive(_) => "Tree",
            Source::Ascendancy(_) => "Ascendancy",
            Source::Item(_) => "Item",
            Source::Skill(_) => "Skill",
            Source::Buff(_) => "Buff",
            Source::Config(_) => "Config",
            Source::Other(_) => "Other",
        }
    }
}

/// Tag kinds. Mirrors `Classes/ModStore.lua:312-903` (`EvalMod`).
///
/// Phase 2 implements only the `Condition` and `Multiplier` cases. The rest are listed so
/// the type compiles against parsed-but-unused mods; `mod_db::EvalMod` returns
/// `Some(value)` for unknown tag kinds without scaling, which is incorrect long-term but
/// safe for the tagged subset of mods we currently exercise.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(tag = "kind")]
pub enum TagKind {
    /// `{type = "Condition", var = X}` — applies iff modDB.conditions[X] is truthy.
    Condition { var: String, neg: bool },
    /// `{type = "ActorCondition", actor = X, var = Y}` — looks up `Y` on a different actor.
    ActorCondition {
        actor: String,
        var: String,
        neg: bool,
    },
    /// `{type = "Multiplier", var = X, limit?, limit_total?, div?}` — value scales by an
    /// integer counter (e.g. `PowerCharge` count). `limit` caps the multiplier; `div`
    /// divides the count first (e.g. "per 10 strength").
    Multiplier {
        var: String,
        limit: Option<f64>,
        limit_total: bool,
        div: Option<f64>,
        actor: Option<String>,
    },
    /// `{type = "PerStat", stat = X, div?}` — value scales by a numeric stat.
    PerStat {
        stat: String,
        div: Option<f64>,
        actor: Option<String>,
    },
    /// `{type = "PercentStat", stat = X, percent = N}` — value scales by N% of stat.
    PercentStat { stat: String, percent: f64 },
    /// `{type = "StatThreshold", stat = X, threshold = N, upper = bool}`.
    StatThreshold {
        stat: String,
        threshold: f64,
        upper: bool,
    },
    /// `{type = "MultiplierThreshold", var = X, threshold = N, upper = bool}`.
    MultiplierThreshold {
        var: String,
        threshold: f64,
        upper: bool,
    },
    /// `{type = "SkillType", skillType = N}`.
    SkillType { skill_type: u8, neg: bool },
    /// `{type = "SkillName", skillName = "..." }`.
    SkillName { skill_name: String, neg: bool },
    /// `{type = "SkillId", skillId = "..." }`.
    SkillId { skill_id: String, neg: bool },
    /// `{type = "SlotName", slotName = "..." }`.
    SlotName { slot_name: String, neg: bool },
    /// Unrecognised tag — preserved so we don't drop data we don't yet understand.
    /// Stored as a JSON value for forensic display.
    Unknown(serde_json::Value),
}

/// One tag attached to a `Mod`. Just `TagKind` for now, but reserved for future fields.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Tag {
    #[serde(flatten)]
    pub kind: TagKind,
}

impl Tag {
    pub fn condition(var: impl Into<String>) -> Self {
        Self {
            kind: TagKind::Condition {
                var: var.into(),
                neg: false,
            },
        }
    }
    pub fn multiplier(var: impl Into<String>) -> Self {
        Self {
            kind: TagKind::Multiplier {
                var: var.into(),
                limit: None,
                limit_total: false,
                div: None,
                actor: None,
            },
        }
    }
    pub fn per_stat(stat: impl Into<String>) -> Self {
        Self {
            kind: TagKind::PerStat {
                stat: stat.into(),
                div: None,
                actor: None,
            },
        }
    }
}

/// One modifier. Sized so the common case fits in two cache lines.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
pub struct Mod {
    /// Stat name, interned-style (cloned strings for now; intern later if profiling says so).
    pub name: String,
    pub kind: ModType,
    pub value: ModValue,
    #[serde(default)]
    pub flags: ModFlag,
    #[serde(default)]
    pub keyword_flags: KeywordFlag,
    #[serde(default)]
    pub source: Option<Source>,
    /// Up to 4 tags inline before spilling.
    #[serde(default, skip_serializing_if = "SmallVec::is_empty")]
    pub tags: SmallVec<[Tag; 2]>,
}

impl Mod {
    pub fn base(name: impl Into<String>, value: impl Into<ModValue>) -> Self {
        Self {
            name: name.into(),
            kind: ModType::Base,
            value: value.into(),
            flags: ModFlag::empty(),
            keyword_flags: KeywordFlag::empty(),
            source: None,
            tags: SmallVec::new(),
        }
    }
    pub fn inc(name: impl Into<String>, percent: impl Into<ModValue>) -> Self {
        Self {
            kind: ModType::Inc,
            ..Self::base(name, percent)
        }
    }
    pub fn more(name: impl Into<String>, percent: impl Into<ModValue>) -> Self {
        Self {
            kind: ModType::More,
            ..Self::base(name, percent)
        }
    }
    pub fn flag(name: impl Into<String>, on: bool) -> Self {
        Self {
            kind: ModType::Flag,
            value: ModValue::Bool(on),
            ..Self::base(name, 0.0)
        }
    }
    pub fn override_(name: impl Into<String>, value: impl Into<ModValue>) -> Self {
        Self {
            kind: ModType::Override,
            ..Self::base(name, value)
        }
    }

    pub fn with_source(mut self, source: Source) -> Self {
        self.source = Some(source);
        self
    }
    pub fn with_flags(mut self, flags: ModFlag) -> Self {
        self.flags = flags;
        self
    }
    pub fn with_keyword_flags(mut self, kf: KeywordFlag) -> Self {
        self.keyword_flags = kf;
        self
    }
    pub fn with_tag(mut self, tag: Tag) -> Self {
        self.tags.push(tag);
        self
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builders_compose() {
        let m = Mod::inc("Life", 10.0)
            .with_source(Source::Tree)
            .with_tag(Tag::condition("FullLife"));
        assert_eq!(m.kind, ModType::Inc);
        assert_eq!(m.name, "Life");
        assert_eq!(m.value.as_f64(), Some(10.0));
        assert_eq!(m.tags.len(), 1);
        assert!(matches!(m.source, Some(Source::Tree)));
    }

    #[test]
    fn flag_mod_value_is_bool() {
        let m = Mod::flag("Condition:KilledRecently", true);
        assert_eq!(m.kind, ModType::Flag);
        assert!(matches!(m.value, ModValue::Bool(true)));
    }

    #[test]
    fn json_round_trip() {
        let m = Mod::inc("FireDamage", 25.0)
            .with_source(Source::Item(2))
            .with_tag(Tag::multiplier("PowerCharge"));
        let json = serde_json::to_string(&m).unwrap();
        let back: Mod = serde_json::from_str(&json).unwrap();
        assert_eq!(m, back);
    }
}
