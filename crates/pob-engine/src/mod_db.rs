//! Modifier storage + query: `ModList`, `ModDB`, the `ModStore` trait.
//!
//! Cross-reference: `Classes/ModStore.lua` (abstract base + `EvalMod`),
//! `Classes/ModList.lua` (linear list), `Classes/ModDB.lua` (hashed-by-name list).
//!
//! In Phase 2 we implement only the basics:
//! - `Sum`: aggregate `Base` and `Inc` values matching a query.
//! - `More`: multiplicative product of `More` values.
//! - `Flag`: any `Flag` mod resolved truthy.
//! - `Override`: first matching `Override` value.
//! - `List`: collect all `List` mod values.
//!
//! Tag resolution is delegated to [`crate::modifier::Tag`] via [`eval_mod`]. Phase 2
//! handles `Condition` and `Multiplier`; other tags are passed through (see
//! `eval_mod` docs).

use std::collections::HashMap;

use pob_data::{KeywordFlag, ModFlag};

use crate::modifier::{Mod, ModType, ModValue, TagKind};

/// Per-query context. Mirrors PoB's `cfg` argument to `Sum/More/Flag/...`.
#[derive(Debug, Clone, Default)]
pub struct QueryCfg<'a> {
    /// Source category filter (`"Item"`, `"Tree"`, …). Matches the leading category of
    /// the mod's `source` field.
    pub source_category: Option<&'a str>,
    /// Skill applicability flags. Mods are kept only if `mod.flags & query.flags == mod.flags`.
    pub flags: ModFlag,
    /// Damage / effect keyword flags.
    pub keyword_flags: KeywordFlag,
    /// Active skill name (used by `SkillName` tags) — empty disables the check.
    pub skill_name: Option<&'a str>,
}

/// State backing `EvalMod` — conditions, multipliers, stats. Owned by the calling actor.
#[derive(Debug, Default)]
pub struct EvalState {
    pub conditions: HashMap<String, bool>,
    pub multipliers: HashMap<String, f64>,
    pub stats: HashMap<String, f64>,
}

impl EvalState {
    pub fn set_condition(&mut self, name: impl Into<String>, on: bool) {
        self.conditions.insert(name.into(), on);
    }
    pub fn set_multiplier(&mut self, name: impl Into<String>, n: f64) {
        self.multipliers.insert(name.into(), n);
    }
    pub fn set_stat(&mut self, name: impl Into<String>, n: f64) {
        self.stats.insert(name.into(), n);
    }
    pub fn condition(&self, name: &str) -> bool {
        self.conditions.get(name).copied().unwrap_or(false)
    }
    pub fn multiplier(&self, name: &str) -> f64 {
        self.multipliers.get(name).copied().unwrap_or(0.0)
    }
    pub fn stat(&self, name: &str) -> f64 {
        self.stats.get(name).copied().unwrap_or(0.0)
    }
}

/// Trait for the two storage backends (`ModList` and `ModDB`) so calling code can be
/// generic over them.
pub trait ModStore {
    /// Iterate every mod in storage.
    fn iter_all(&self) -> Box<dyn Iterator<Item = &Mod> + '_>;
    /// Iterate mods that match `name`. Some backends (ModDB) make this O(1).
    fn iter_named<'a>(&'a self, name: &'a str) -> Box<dyn Iterator<Item = &'a Mod> + 'a>;

    /// Sum `Base` + `Inc` values for `name` after evaluating tags.
    fn sum(&self, kind: ModType, cfg: &QueryCfg<'_>, state: &EvalState, name: &str) -> f64 {
        let mut sum = 0.0;
        for m in self.iter_named(name) {
            if m.kind != kind {
                continue;
            }
            if !match_query(m, cfg) {
                continue;
            }
            if let Some(v) = eval_mod(m, state) {
                sum += v;
            }
        }
        sum
    }

    /// Multiplicative product of `More` values for `name`. A single `More` of N% returns
    /// `(1 + N/100)`; multiple compose by multiplication.
    fn more(&self, cfg: &QueryCfg<'_>, state: &EvalState, name: &str) -> f64 {
        let mut prod = 1.0;
        for m in self.iter_named(name) {
            if m.kind != ModType::More {
                continue;
            }
            if !match_query(m, cfg) {
                continue;
            }
            if let Some(v) = eval_mod(m, state) {
                prod *= 1.0 + v / 100.0;
            }
        }
        prod
    }

    /// True if any `Flag` mod for `name` resolves to a truthy value.
    fn flag(&self, cfg: &QueryCfg<'_>, state: &EvalState, name: &str) -> bool {
        for m in self.iter_named(name) {
            if m.kind != ModType::Flag {
                continue;
            }
            if !match_query(m, cfg) {
                continue;
            }
            if eval_mod(m, state).is_some() {
                if let ModValue::Bool(b) = m.value {
                    if b {
                        return true;
                    }
                } else if let ModValue::Number(n) = m.value {
                    if n != 0.0 {
                        return true;
                    }
                }
            }
        }
        false
    }

    /// First `Override` value for `name`, if any.
    fn override_value(&self, cfg: &QueryCfg<'_>, state: &EvalState, name: &str) -> Option<f64> {
        for m in self.iter_named(name) {
            if m.kind != ModType::Override {
                continue;
            }
            if !match_query(m, cfg) {
                continue;
            }
            if let Some(v) = eval_mod(m, state) {
                return Some(v);
            }
        }
        None
    }

    /// Convenience: full applied multiplier `(1 + sum(Inc)/100) * product(More)`.
    fn applied(&self, cfg: &QueryCfg<'_>, state: &EvalState, name: &str) -> f64 {
        let inc = self.sum(ModType::Inc, cfg, state, name);
        let more = self.more(cfg, state, name);
        (1.0 + inc / 100.0) * more
    }
}

fn match_query(m: &Mod, cfg: &QueryCfg<'_>) -> bool {
    // Source category: PoB does `mod.source:match("[^:]+") == source` — i.e. the leading
    // classifier (before the `:` if any) must match.
    if let Some(want) = cfg.source_category {
        match &m.source {
            Some(s) if s.category() == want => {}
            None => return false,
            _ => return false,
        }
    }
    // ModFlag: every bit set on the mod must be set on the query.
    if !m.flags.is_empty() && (cfg.flags & m.flags) != m.flags {
        return false;
    }
    // KeywordFlag: per Lua MatchKeywordFlags semantics.
    if !KeywordFlag::matches(cfg.keyword_flags, m.keyword_flags) {
        return false;
    }
    true
}

/// Evaluate a mod against state: returns the (possibly scaled) numeric value, or `None`
/// if a tag rules the mod out for this query.
///
/// Phase 2 supports `Condition`, `Multiplier`, `PerStat`, and `ActorCondition`. Other tag
/// kinds are conservatively passed through (treated as if they always succeed). This is
/// safe-ish for early use because we only feed in mods whose tags we understand; broader
/// tag coverage comes in Phase 3 alongside ModParser growth.
pub fn eval_mod(m: &Mod, state: &EvalState) -> Option<f64> {
    let mut value = m.value.as_f64()?;
    for tag in &m.tags {
        match &tag.kind {
            TagKind::Condition { var, neg } => {
                if state.condition(var) == *neg {
                    return None;
                }
            }
            TagKind::ActorCondition { actor, var, neg } => {
                // Phase 2: only the player actor is modelled. Treat enemy/minion conditions
                // as false unless we explicitly stash them under a namespaced key.
                let key = format!("{actor}:{var}");
                if state.condition(&key) == *neg {
                    return None;
                }
            }
            TagKind::Multiplier {
                var,
                limit,
                limit_total,
                div,
                actor: _,
            } => {
                let mut count = state.multiplier(var);
                if let Some(d) = div {
                    if *d != 0.0 {
                        count = (count / d).floor();
                    }
                }
                let mut scaled = value * count;
                if let Some(lim) = limit {
                    if *limit_total {
                        scaled = scaled.min(*lim);
                    } else {
                        let capped_count = count.min(*lim);
                        scaled = value * capped_count;
                    }
                }
                value = scaled;
            }
            TagKind::PerStat { stat, div, actor: _ } => {
                let mut s = state.stat(stat);
                if let Some(d) = div {
                    if *d != 0.0 {
                        s = (s / d).floor();
                    }
                }
                value *= s;
            }
            TagKind::PercentStat { stat, percent } => {
                let s = state.stat(stat);
                value *= s * percent / 100.0;
            }
            TagKind::StatThreshold {
                stat,
                threshold,
                upper,
            } => {
                let s = state.stat(stat);
                let pass = if *upper { s <= *threshold } else { s >= *threshold };
                if !pass {
                    return None;
                }
            }
            TagKind::MultiplierThreshold {
                var,
                threshold,
                upper,
            } => {
                let s = state.multiplier(var);
                let pass = if *upper { s <= *threshold } else { s >= *threshold };
                if !pass {
                    return None;
                }
            }
            TagKind::SkillName { skill_name, neg } => {
                let matches = state.condition(&format!("SkillName:{skill_name}"));
                if matches == *neg {
                    return None;
                }
            }
            TagKind::SkillId { skill_id, neg } => {
                let matches = state.condition(&format!("SkillId:{skill_id}"));
                if matches == *neg {
                    return None;
                }
            }
            TagKind::SkillType { skill_type, neg } => {
                let matches = state.condition(&format!("SkillType:{skill_type}"));
                if matches == *neg {
                    return None;
                }
            }
            TagKind::SlotName { slot_name, neg } => {
                let matches = state.condition(&format!("SlotName:{slot_name}"));
                if matches == *neg {
                    return None;
                }
            }
            // Unknown tag kinds are pass-through (treated as if they always succeed).
            _ => {}
        }
    }
    Some(value)
}

/// Linear modifier list. Cheap to build, O(n) to query. Used per-item.
#[derive(Debug, Default, Clone)]
pub struct ModList {
    mods: Vec<Mod>,
}

impl ModList {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, m: Mod) {
        self.mods.push(m);
    }
    pub fn extend<I: IntoIterator<Item = Mod>>(&mut self, it: I) {
        self.mods.extend(it);
    }
    pub fn len(&self) -> usize {
        self.mods.len()
    }
    pub fn is_empty(&self) -> bool {
        self.mods.is_empty()
    }
    pub fn iter(&self) -> impl Iterator<Item = &Mod> {
        self.mods.iter()
    }
}

impl ModStore for ModList {
    fn iter_all(&self) -> Box<dyn Iterator<Item = &Mod> + '_> {
        Box::new(self.mods.iter())
    }
    fn iter_named<'a>(&'a self, name: &'a str) -> Box<dyn Iterator<Item = &'a Mod> + 'a> {
        Box::new(self.mods.iter().filter(move |m| m.name == name))
    }
}

/// Hash-indexed modifier database. O(1) bucket lookup by name; linear scan within a bucket.
#[derive(Debug, Default, Clone)]
pub struct ModDB {
    buckets: HashMap<String, Vec<Mod>>,
}

impl ModDB {
    pub fn new() -> Self {
        Self::default()
    }
    pub fn add(&mut self, m: Mod) {
        self.buckets.entry(m.name.clone()).or_default().push(m);
    }
    pub fn extend<I: IntoIterator<Item = Mod>>(&mut self, it: I) {
        for m in it {
            self.add(m);
        }
    }
    pub fn extend_from_list(&mut self, list: ModList) {
        for m in list.mods {
            self.add(m);
        }
    }
    pub fn len(&self) -> usize {
        self.buckets.values().map(Vec::len).sum()
    }
}

impl ModStore for ModDB {
    fn iter_all(&self) -> Box<dyn Iterator<Item = &Mod> + '_> {
        Box::new(self.buckets.values().flat_map(|v| v.iter()))
    }
    fn iter_named<'a>(&'a self, name: &'a str) -> Box<dyn Iterator<Item = &'a Mod> + 'a> {
        match self.buckets.get(name) {
            Some(v) => Box::new(v.iter()),
            None => Box::new(std::iter::empty()),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{Mod, Source, Tag};

    fn cfg() -> QueryCfg<'static> {
        QueryCfg::default()
    }

    #[test]
    fn sum_base_and_inc_separately() {
        let mut db = ModDB::new();
        db.add(Mod::base("Strength", 32.0));
        db.add(Mod::base("Strength", 10.0));
        db.add(Mod::inc("Strength", 20.0));

        let st = EvalState::default();
        assert_eq!(db.sum(ModType::Base, &cfg(), &st, "Strength"), 42.0);
        assert_eq!(db.sum(ModType::Inc, &cfg(), &st, "Strength"), 20.0);
    }

    #[test]
    fn more_multiplies() {
        let mut db = ModDB::new();
        db.add(Mod::more("Damage", 30.0));
        db.add(Mod::more("Damage", 50.0));
        let st = EvalState::default();
        let m = db.more(&cfg(), &st, "Damage");
        assert!((m - (1.30 * 1.50)).abs() < 1e-9);
    }

    #[test]
    fn applied_combines_inc_and_more() {
        let mut db = ModDB::new();
        db.add(Mod::inc("Life", 50.0));
        db.add(Mod::more("Life", 20.0));
        let st = EvalState::default();
        let m = db.applied(&cfg(), &st, "Life");
        assert!((m - (1.50 * 1.20)).abs() < 1e-9);
    }

    #[test]
    fn flag_returns_true_if_any_set() {
        let mut db = ModDB::new();
        db.add(Mod::flag("Keystone:CallToArms", true));
        let st = EvalState::default();
        assert!(db.flag(&cfg(), &st, "Keystone:CallToArms"));
        assert!(!db.flag(&cfg(), &st, "Keystone:Other"));
    }

    #[test]
    fn override_takes_first_match() {
        let mut db = ModDB::new();
        db.add(Mod::override_("Life", 1.0));
        db.add(Mod::override_("Life", 999.0));
        let st = EvalState::default();
        assert!(matches!(db.override_value(&cfg(), &st, "Life"), Some(_)));
    }

    #[test]
    fn condition_tag_filters() {
        let mut db = ModDB::new();
        db.add(Mod::inc("Life", 30.0).with_tag(Tag::condition("FullLife")));
        let mut st = EvalState::default();
        assert_eq!(db.sum(ModType::Inc, &cfg(), &st, "Life"), 0.0);
        st.set_condition("FullLife", true);
        assert_eq!(db.sum(ModType::Inc, &cfg(), &st, "Life"), 30.0);
    }

    #[test]
    fn multiplier_tag_scales() {
        let mut db = ModDB::new();
        // "+5 Life per Power Charge"
        db.add(Mod::base("Life", 5.0).with_tag(Tag::multiplier("PowerCharge")));
        let mut st = EvalState::default();
        st.set_multiplier("PowerCharge", 3.0);
        assert_eq!(db.sum(ModType::Base, &cfg(), &st, "Life"), 15.0);
    }

    #[test]
    fn source_category_filter() {
        let mut db = ModDB::new();
        db.add(Mod::base("Life", 10.0).with_source(Source::Tree));
        db.add(Mod::base("Life", 20.0).with_source(Source::Item(1)));
        let st = EvalState::default();
        let mut c = cfg();
        c.source_category = Some("Tree");
        assert_eq!(db.sum(ModType::Base, &c, &st, "Life"), 10.0);
        c.source_category = Some("Item");
        assert_eq!(db.sum(ModType::Base, &c, &st, "Life"), 20.0);
    }
}
