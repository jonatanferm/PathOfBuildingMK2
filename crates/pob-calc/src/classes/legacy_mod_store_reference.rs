//! Port of `Classes/ModStore.lua`, `Classes/ModDB.lua` and `Classes/ModList.lua`.
//!
//! PoB stores reach the rest of the calc through `self.actor` (outputs,
//! weapons, items, minion data, other actors) and `self.parent`. Here a store
//! holds an [`ActorId`] and an optional parent [`DbId`], and every query takes
//! an [`EvalEnv`] that resolves them — so the same code serves the real calc
//! env and small test fixtures.

use ahash::AHashMap;

use crate::flags::match_keyword_flags;
use crate::modifier::{Mod, ModType, Sockets, Tag, TagType, Value};

/// Index of an actor (player, enemy, minion, …) in the calc env.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash, Default)]
pub struct ActorId(pub u32);

/// Index of a [`ModDb`] in the calc env (parents are resolved through it).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub struct DbId(pub u32);

/// What `ItemCondition` tags need from an equipped item.
pub trait ItemView {
    fn name(&self) -> &str;
    fn rarity(&self) -> &str;
    fn item_type(&self) -> &str;
    fn corrupted(&self) -> bool;
    fn shaper(&self) -> bool;
    fn elder(&self) -> bool;
    /// `item:FindModifierSubstring(substring, itemSlotName)`.
    fn find_modifier_substring(&self, substring: &str, slot: &str) -> bool;
}

/// Weapon data flags consulted by `Condition` tags (Varunastra-style
/// "counts as all one-handed" weapons record which `Using…` conditions they
/// added: `weaponData.Added<Var>`).
pub trait WeaponView {
    fn counts_as_all_1h(&self) -> bool;
    /// `weaponData["Added" .. var]`: `None` when absent.
    fn added_condition(&self, var: &str) -> Option<bool>;
}

/// The actor-side state a store reads while evaluating tags.
pub trait ActorView {
    /// `actor.output[stat]`.
    fn output(&self, stat: &str) -> Option<f64>;
    fn weapon(&self, index: u8) -> Option<&dyn WeaponView>;
    /// `actor.itemList[slot]`.
    fn item(&self, slot: &str) -> Option<&dyn ItemView>;
    /// Every `(slot, item)` in `actor.itemList`.
    fn items(&self) -> Vec<(&str, &dyn ItemView)>;
    /// `actor.minionData.monsterTags`.
    fn monster_tags(&self) -> Option<&[String]>;
    /// `ModStore:GetStat` special cases: `ManaReservedPercent` /
    /// `LifeReservedPercent` for the skill named in `cfg` (None → not
    /// applicable), `ManaUnreserved` NaN / negative handling.
    fn special_stat(&self, stat: &str, cfg: Option<&ModCfg<'_>>) -> Option<f64>;
}

/// Everything a store query may need beyond the store itself.
pub trait EvalEnv {
    fn db(&self, id: DbId) -> &ModDb;
    fn actor(&self, id: ActorId) -> &dyn ActorView;
    /// `getActor(self, actorType)`: `"player"`, `"enemy"`, `"parent"`, `"minion"`.
    fn related_actor(&self, from: ActorId, rel: &str) -> Option<ActorId>;
    /// The db of an actor (`actor.modDB`).
    fn actor_db(&self, id: ActorId) -> DbId;
    /// `data.highPrecisionMods[name][type]`.
    fn high_precision(&self, name: &str, ty: ModType) -> Option<i32>;
    /// `calcLib.getGameIdFromGemName(name, true)`.
    fn game_id_from_gem_name(&self, name: &str) -> String;
    /// `calcLib.gemIsType(gem, keyword)`.
    fn gem_is_type(&self, gem: &GemRef, keyword: &str) -> bool;
}

/// `cfg.skillGem` — the gem a socketed-in / keyword check looks at.
#[derive(Debug, Clone, Default)]
pub struct GemRef {
    pub name: String,
    pub tags: Vec<String>,
}

/// PoB's query `cfg` table. All fields optional; `Default` is `nil`.
#[derive(Clone, Default)]
pub struct ModCfg<'a> {
    pub flags: u32,
    pub keyword_flags: u32,
    pub source: Option<&'a str>,
    pub skill_name: Option<&'a str>,
    pub summon_skill_name: Option<&'a str>,
    /// `cfg.skillGrantedEffect.id`.
    pub skill_id: Option<&'a str>,
    /// `cfg.skillGrantedEffect.baseFlags` (or `cfg.baseFlags`).
    pub base_flags: Option<&'a [String]>,
    pub skill_part: Option<f64>,
    pub skill_types: Option<&'a ahash::AHashSet<u32>>,
    pub skill_cond: Option<&'a AHashMap<String, bool>>,
    pub skill_stats: Option<&'a AHashMap<String, f64>>,
    pub skill_dist: Option<f64>,
    pub slot_name: Option<&'a str>,
    pub socket_num: Option<f64>,
    pub socket_color: Option<&'a str>,
    pub strength_gems: Option<f64>,
    pub dexterity_gems: Option<f64>,
    pub intelligence_gems: Option<f64>,
    pub skill_gem: Option<&'a GemRef>,
    pub item: Option<&'a dyn ItemView>,
    pub actor: Option<&'a str>,
}

/// `{ value, mod }` rows returned by `Tabulate`.
#[derive(Debug, Clone)]
pub struct TabRow<'m> {
    pub value: Value,
    pub md: &'m Mod,
}

/// A hash-indexed modifier database (`ModDB`).
#[derive(Debug, Clone, Default)]
pub struct ModDb {
    pub mods: AHashMap<String, Vec<Mod>>,
    pub conditions: AHashMap<String, bool>,
    pub multipliers: AHashMap<String, f64>,
    pub parent: Option<DbId>,
    pub actor: ActorId,
}

/// A flat modifier list (`ModList`), e.g. an active skill's `skillModList`.
#[derive(Debug, Clone, Default)]
pub struct ModList {
    pub mods: Vec<Mod>,
    pub conditions: AHashMap<String, bool>,
    pub multipliers: AHashMap<String, f64>,
    pub parent: Option<DbId>,
    pub actor: ActorId,
}

/// Either kind of store, as the `context` of a query.
#[derive(Clone, Copy)]
pub enum Store<'s> {
    Db(&'s ModDb),
    List(&'s ModList),
}

fn query_matches(m: &Mod, flags: u32, keyword_flags: u32, source: Option<&str>) -> bool {
    flags & m.flags == m.flags
        && match_keyword_flags(keyword_flags, m.keyword_flags)
        && source.is_none_or(|s| m.source_category() == Some(s))
}

/// Lua `round(x, 2)` from `Modules/Common.lua`: `floor(x * 10^dec + 0.5) / 10^dec`.
#[must_use]
pub fn round(x: f64, dec: i32) -> f64 {
    let p = 10f64.powi(dec);
    (x * p + 0.5).floor() / p
}

impl ModDb {
    #[must_use]
    pub fn new(actor: ActorId) -> Self {
        Self {
            actor,
            ..Self::default()
        }
    }

    pub fn add_mod(&mut self, m: Mod) {
        self.mods.entry(m.name.clone()).or_default().push(m);
    }

    pub fn add_list(&mut self, list: impl IntoIterator<Item = Mod>) {
        for m in list {
            self.add_mod(m);
        }
    }

    /// `AddDB`: copy every mod of another db in.
    pub fn add_db(&mut self, other: &ModDb) {
        for (name, list) in &other.mods {
            self.mods
                .entry(name.clone())
                .or_default()
                .extend(list.iter().cloned());
        }
    }

    /// `NewMod(...)`.
    pub fn new_mod(&mut self, name: &str, ty: ModType, value: impl Into<Value>, source: &str) {
        self.add_mod(Mod::new(name, ty, value).source(source));
    }

    /// `ReplaceModInternal` for this db only (parents aren't mutated here —
    /// callers own them). Returns whether a match was replaced.
    pub fn replace_mod_local(&mut self, mut m: Mod) -> Result<(), Mod> {
        let list = self.mods.entry(m.name.clone()).or_default();
        for cur in list.iter_mut() {
            if m.name == cur.name
                && m.ty == cur.ty
                && m.flags == cur.flags
                && m.keyword_flags == cur.keyword_flags
                && m.source == cur.source
                && !cur.replaced
            {
                m.replaced = true;
                *cur = m;
                return Ok(());
            }
        }
        Err(m)
    }

    /// `ReplaceMod`: replace a matching mod, or add it.
    pub fn replace_mod(&mut self, m: Mod) {
        if let Err(m) = self.replace_mod_local(m) {
            self.add_mod(m);
        }
    }

    /// `ConvertMod(oldName, …)` for this db: move the first matching mod named
    /// `old_name` to `m`'s name. Adds `m` when nothing matched.
    pub fn convert_mod(&mut self, old_name: &str, mut m: Mod) {
        if let Some(list) = self.mods.get_mut(old_name) {
            if let Some(i) = list.iter().position(|cur| {
                cur.name == old_name
                    && m.ty == cur.ty
                    && m.flags == cur.flags
                    && m.keyword_flags == cur.keyword_flags
                    && m.source == cur.source
                    && !cur.converted
            }) {
                list.remove(i);
                m.converted = true;
            }
        }
        self.add_mod(m);
    }

    #[must_use]
    pub fn store(&self) -> Store<'_> {
        Store::Db(self)
    }
}

impl ModList {
    #[must_use]
    pub fn new(actor: ActorId, parent: Option<DbId>) -> Self {
        Self {
            actor,
            parent,
            ..Self::default()
        }
    }

    pub fn add_mod(&mut self, m: Mod) {
        self.mods.push(m);
    }

    pub fn add_list(&mut self, list: impl IntoIterator<Item = Mod>) {
        self.mods.extend(list);
    }

    #[must_use]
    pub fn store(&self) -> Store<'_> {
        Store::List(self)
    }

    /// `ReplaceMod` on this list.
    pub fn replace_mod(&mut self, mut m: Mod) {
        for cur in &mut self.mods {
            if m.name == cur.name
                && m.ty == cur.ty
                && m.flags == cur.flags
                && m.keyword_flags == cur.keyword_flags
                && m.source == cur.source
                && !cur.replaced
            {
                m.replaced = true;
                *cur = m;
                return;
            }
        }
        self.mods.push(m);
    }
}

/// Mods named `name` in one store level (no parents).
fn level_mods<'s>(store: Store<'s>, name: &'s str) -> Box<dyn Iterator<Item = &'s Mod> + 's> {
    match store {
        Store::Db(db) => match db.mods.get(name) {
            Some(v) => Box::new(v.iter()),
            None => Box::new(std::iter::empty()),
        },
        Store::List(list) => Box::new(list.mods.iter().filter(move |m| m.name == name)),
    }
}

impl<'s> Store<'s> {
    #[must_use]
    pub fn parent(self) -> Option<DbId> {
        match self {
            Store::Db(d) => d.parent,
            Store::List(l) => l.parent,
        }
    }

    #[must_use]
    pub fn actor(self) -> ActorId {
        match self {
            Store::Db(d) => d.actor,
            Store::List(l) => l.actor,
        }
    }

    fn conditions(self) -> &'s AHashMap<String, bool> {
        match self {
            Store::Db(d) => &d.conditions,
            Store::List(l) => &l.conditions,
        }
    }

    fn multipliers(self) -> &'s AHashMap<String, f64> {
        match self {
            Store::Db(d) => &d.multipliers,
            Store::List(l) => &l.multipliers,
        }
    }

    /// This store followed by its parent chain.
    fn chain<'e>(self, env: &'e dyn EvalEnv) -> impl Iterator<Item = Store<'e>> + 'e
    where
        's: 'e,
    {
        let mut next: Option<Store<'e>> = Some(self);
        std::iter::from_fn(move || {
            let cur = next?;
            next = cur.parent().map(|p| Store::Db(env.db(p)));
            Some(cur)
        })
    }

    // ---- aggregate queries -------------------------------------------------

    /// `Sum(modType, cfg, …names)`.
    pub fn sum(
        self,
        env: &dyn EvalEnv,
        ty: ModType,
        cfg: Option<&ModCfg<'_>>,
        names: &[&str],
    ) -> f64 {
        let (flags, kw, source) = cfg_parts(cfg);
        let mut result = 0.0;
        for store in self.chain(env) {
            let mut limits: Option<AHashMap<String, f64>> = None;
            for name in names {
                for m in level_mods(store, name) {
                    if m.ty == ty && query_matches(m, flags, kw, source) {
                        if m.tags.is_empty() {
                            result += m.value.num_or_zero();
                        } else {
                            let limits = limits.get_or_insert_with(AHashMap::default);
                            result += self
                                .eval_mod(env, m, cfg, Some(limits))
                                .map_or(0.0, |v| v.num_or_zero());
                        }
                    }
                }
            }
        }
        result
    }

    /// `More(cfg, …names)`: product of `(1 + v/100)`, rounded per name the
    /// way PoB does (2 decimals, or the mod's high-precision digits).
    pub fn more(self, env: &dyn EvalEnv, cfg: Option<&ModCfg<'_>>, names: &[&str]) -> f64 {
        let (flags, kw, source) = cfg_parts(cfg);
        let mut result = 1.0;
        for store in self.chain(env) {
            let mut level_result = 1.0;
            let mut limits: Option<AHashMap<String, f64>> = None;
            let mut precision: Option<i32> = None;
            for name in names {
                let mut mod_result = 1.0;
                for m in level_mods(store, name) {
                    if m.ty == ModType::More && query_matches(m, flags, kw, source) {
                        let value = if m.tags.is_empty() {
                            m.value.num_or_zero()
                        } else {
                            let limits = limits.get_or_insert_with(AHashMap::default);
                            self.eval_mod(env, m, cfg, Some(limits))
                                .map_or(0.0, |v| v.num_or_zero())
                        };
                        mod_result *= 1.0 + value / 100.0;
                        let p = env.high_precision(&m.name, m.ty);
                        precision = match (precision, p) {
                            (Some(a), Some(b)) => Some(a.max(b)),
                            (Some(a), None) => Some(a),
                            (None, p) => p,
                        };
                    }
                }
                if let Some(p) = precision {
                    let power = 10f64.powi(p);
                    level_result = (level_result * mod_result * power).floor() / power;
                } else {
                    level_result *= round(mod_result, 2);
                }
            }
            result *= level_result;
        }
        result
    }

    /// `Flag(cfg, …names)`.
    pub fn flag(self, env: &dyn EvalEnv, cfg: Option<&ModCfg<'_>>, names: &[&str]) -> bool {
        let (flags, kw, source) = cfg_parts(cfg);
        for store in self.chain(env) {
            for name in names {
                for m in level_mods(store, name) {
                    if m.ty == ModType::Flag && query_matches(m, flags, kw, source) {
                        if m.tags.is_empty() {
                            if m.value.truthy() {
                                return true;
                            }
                        } else if self.eval_mod(env, m, cfg, None).is_some_and(|v| v.truthy()) {
                            return true;
                        }
                    }
                }
            }
        }
        false
    }

    /// `Override(cfg, …names)`: first matching override value.
    pub fn override_value(
        self,
        env: &dyn EvalEnv,
        cfg: Option<&ModCfg<'_>>,
        names: &[&str],
    ) -> Option<Value> {
        let (flags, kw, source) = cfg_parts(cfg);
        for store in self.chain(env) {
            for name in names {
                for m in level_mods(store, name) {
                    if m.ty == ModType::Override && query_matches(m, flags, kw, source) {
                        if m.tags.is_empty() {
                            if m.value.truthy() {
                                return Some(m.value.clone());
                            }
                        } else if let Some(v) = self.eval_mod(env, m, cfg, None) {
                            if v.truthy() {
                                return Some(v);
                            }
                        }
                    }
                }
            }
        }
        None
    }

    /// Numeric `Override`.
    pub fn override_num(
        self,
        env: &dyn EvalEnv,
        cfg: Option<&ModCfg<'_>>,
        names: &[&str],
    ) -> Option<f64> {
        self.override_value(env, cfg, names).and_then(|v| v.num())
    }

    /// `List(cfg, …names)`.
    pub fn list(self, env: &dyn EvalEnv, cfg: Option<&ModCfg<'_>>, names: &[&str]) -> Vec<Value> {
        let (flags, kw, source) = cfg_parts(cfg);
        let mut out = Vec::new();
        for store in self.chain(env) {
            for name in names {
                for m in level_mods(store, name) {
                    if m.ty == ModType::List && query_matches(m, flags, kw, source) {
                        if m.tags.is_empty() {
                            if m.value.truthy() {
                                out.push(m.value.clone());
                            }
                        } else if let Some(v) = self.eval_mod(env, m, cfg, None) {
                            out.push(v);
                        }
                    }
                }
            }
        }
        out
    }

    /// `Tabulate(modType, cfg, …names)`; `ty = None` matches every type.
    pub fn tabulate<'e>(
        self,
        env: &'e dyn EvalEnv,
        ty: Option<ModType>,
        cfg: Option<&ModCfg<'_>>,
        names: &[&str],
    ) -> Vec<TabRow<'e>>
    where
        's: 'e,
    {
        let (flags, kw, source) = cfg_parts(cfg);
        let mut out = Vec::new();
        for store in self.chain(env) {
            let mut limits: Option<AHashMap<String, f64>> = None;
            for name in names {
                // `level_mods` borrows `name` for the iterator's lifetime;
                // collect first so the rows can outlive this loop.
                let mods: Vec<&'e Mod> = match store {
                    Store::Db(db) => db
                        .mods
                        .get(*name)
                        .map(|v| v.iter().collect())
                        .unwrap_or_default(),
                    Store::List(list) => list.mods.iter().filter(|m| m.name == *name).collect(),
                };
                for m in mods {
                    if ty.is_none_or(|t| m.ty == t) && query_matches(m, flags, kw, source) {
                        let value = if m.tags.is_empty() {
                            Some(m.value.clone())
                        } else {
                            let limits = limits.get_or_insert_with(AHashMap::default);
                            self.eval_mod(env, m, cfg, Some(limits))
                        };
                        if let Some(value) = value {
                            let keep = match &value {
                                Value::Num(n) => *n != 0.0 || m.ty == ModType::Override,
                                Value::Nil | Value::Bool(false) => false,
                                _ => true,
                            };
                            if keep {
                                out.push(TabRow { value, md: m });
                            }
                        }
                    }
                }
            }
        }
        out
    }

    /// `Max(cfg, …names)`.
    pub fn max(self, env: &dyn EvalEnv, cfg: Option<&ModCfg<'_>>, names: &[&str]) -> Option<f64> {
        let mut max: Option<f64> = None;
        for row in self.tabulate(env, Some(ModType::Max), cfg, names) {
            let v = self
                .eval_mod(env, row.md, cfg, None)
                .map_or(0.0, |v| v.num_or_zero());
            if v > max.unwrap_or(0.0) {
                max = Some(v);
            }
        }
        max
    }

    /// `Min(cfg, …names)`.
    pub fn min(self, env: &dyn EvalEnv, cfg: Option<&ModCfg<'_>>, names: &[&str]) -> Option<f64> {
        let mut min: Option<f64> = None;
        for row in self.tabulate(env, Some(ModType::Min), cfg, names) {
            let v = self
                .eval_mod(env, row.md, cfg, None)
                .map_or(0.0, |v| v.num_or_zero());
            if min.is_none_or(|m| v < m) {
                min = Some(v);
            }
        }
        min
    }

    /// `HasMod(modType, cfg, …names)` — ignores tags.
    pub fn has_mod(
        self,
        env: &dyn EvalEnv,
        ty: ModType,
        cfg: Option<&ModCfg<'_>>,
        names: &[&str],
    ) -> bool {
        let (flags, kw, source) = cfg_parts(cfg);
        self.chain(env).any(|store| {
            names.iter().any(|name| {
                level_mods(store, name).any(|m| m.ty == ty && query_matches(m, flags, kw, source))
            })
        })
    }

    // ---- conditions / multipliers / stats ----------------------------------

    /// `GetCondition(var, cfg, noMod)`.
    pub fn get_condition(
        self,
        env: &dyn EvalEnv,
        var: &str,
        cfg: Option<&ModCfg<'_>>,
        no_mod: bool,
    ) -> bool {
        if self.conditions().get(var).copied().unwrap_or(false) {
            return true;
        }
        if let Some(p) = self.parent() {
            if Store::Db(env.db(p)).get_condition(env, var, cfg, true) {
                return true;
            }
        }
        !no_mod && self.flag(env, cfg, &[&format!("Condition:{var}")])
    }

    /// `GetMultiplier(var, cfg, noMod)`.
    pub fn get_multiplier(
        self,
        env: &dyn EvalEnv,
        var: &str,
        cfg: Option<&ModCfg<'_>>,
        no_mod: bool,
    ) -> f64 {
        let name = format!("Multiplier:{var}");
        if !no_mod {
            if let Some(v) = self.override_num(env, cfg, &[&name]) {
                return v;
            }
        }
        let own = self.multipliers().get(var).copied().unwrap_or(0.0);
        let parent = self.parent().map_or(0.0, |p| {
            Store::Db(env.db(p)).get_multiplier(env, var, cfg, true)
        });
        let summed = if no_mod {
            0.0
        } else {
            self.sum(env, ModType::Base, cfg, &[&name])
        };
        own + parent + summed
    }

    /// `GetStat(stat, cfg)`.
    pub fn get_stat(self, env: &dyn EvalEnv, stat: &str, cfg: Option<&ModCfg<'_>>) -> f64 {
        let actor = env.actor(self.actor());
        if let Some(v) = actor.special_stat(stat, cfg) {
            return v;
        }
        actor
            .output(stat)
            .or_else(|| {
                cfg.and_then(|c| c.skill_stats)
                    .and_then(|s| s.get(stat).copied())
            })
            .unwrap_or(0.0)
    }

    // ---- EvalMod -------------------------------------------------------------

    /// `EvalMod(mod, cfg, globalLimits)`: apply every tag; `None` = the mod
    /// doesn't apply (Lua `return` with no value).
    #[allow(clippy::too_many_lines)]
    pub fn eval_mod(
        self,
        env: &dyn EvalEnv,
        m: &Mod,
        cfg: Option<&ModCfg<'_>>,
        global_limits: Option<&mut AHashMap<String, f64>>,
    ) -> Option<Value> {
        let mut value = m.value.clone();
        let this = self;
        let actor_db = |name: &str| -> Option<Store<'_>> {
            env.related_actor(this.actor(), name)
                .map(|id| Store::Db(env.db(env.actor_db(id))))
        };
        for tag in &m.tags {
            match &tag.ty {
                TagType::Multiplier => {
                    let mut target = self;
                    let mut limit_target = self;
                    if let Some(a) = &tag.limit_actor {
                        limit_target = actor_db(a)?;
                    }
                    if let Some(a) = &tag.actor {
                        target = actor_db(a)?;
                    }
                    let base = if let Some(list) = &tag.var_list {
                        list.iter()
                            .map(|v| target.get_multiplier(env, v, cfg, false))
                            .sum()
                    } else {
                        target.get_multiplier(env, tag.var.as_deref().unwrap_or(""), cfg, false)
                    };
                    let div = match &tag.div_var {
                        Some(dv) => self.get_multiplier(env, dv, cfg, false),
                        None => tag.div.unwrap_or(1.0),
                    };
                    let mut mult = if tag.no_floor {
                        base / div
                    } else {
                        (base / div + 0.0001).floor()
                    };
                    let (mut limit_total, mut limit_neg_total) = (None, None);
                    if tag.limit.is_some() || tag.limit_var.is_some() || tag.limit_stat.is_some() {
                        let limit = tag
                            .limit
                            .or_else(|| {
                                tag.limit_var
                                    .as_ref()
                                    .map(|v| limit_target.get_multiplier(env, v, cfg, false))
                            })
                            .or_else(|| {
                                tag.limit_stat
                                    .as_ref()
                                    .map(|s| limit_target.get_stat(env, s, cfg))
                            })
                            .unwrap_or(0.0);
                        if tag.limit_total {
                            limit_total = Some(limit);
                        } else if tag.limit_neg_total {
                            limit_neg_total = Some(limit);
                        } else {
                            mult = mult.min(limit);
                        }
                    }
                    if tag.invert && mult != 0.0 {
                        mult = 1.0 / mult;
                    }
                    scale_value(&mut value, |v| {
                        let mut v = v * mult + tag.base.unwrap_or(0.0);
                        if let Some(l) = limit_total {
                            v = v.min(l);
                        }
                        if let Some(l) = limit_neg_total {
                            v = v.max(l);
                        }
                        v
                    });
                }
                TagType::MultiplierThreshold => {
                    let mut target = self;
                    let mut threshold_target = self;
                    if let Some(a) = &tag.threshold_actor {
                        threshold_target = actor_db(a)?;
                    }
                    if let Some(a) = &tag.actor {
                        target = actor_db(a)?;
                    }
                    let mult: f64 = if let Some(list) = &tag.var_list {
                        list.iter()
                            .map(|v| target.get_multiplier(env, v, cfg, false))
                            .sum()
                    } else {
                        target.get_multiplier(env, tag.var.as_deref().unwrap_or(""), cfg, false)
                    };
                    let threshold = match tag.threshold {
                        Some(t) => t,
                        None => {
                            let t = if tag.threshold_actor.is_some() {
                                threshold_target
                            } else {
                                target
                            };
                            t.get_multiplier(
                                env,
                                tag.threshold_var.as_deref().unwrap_or(""),
                                cfg,
                                false,
                            )
                        }
                    };
                    if (tag.upper && mult > threshold)
                        || (tag.equals && mult != threshold)
                        || (!tag.upper && mult < threshold)
                    {
                        return None;
                    }
                }
                TagType::PerStat => {
                    let target = tag.actor.as_deref().and_then(actor_db).unwrap_or(self);
                    let base: f64 = if let Some(list) = &tag.stat_list {
                        list.iter().map(|s| target.get_stat(env, s, cfg)).sum()
                    } else {
                        target.get_stat(env, tag.stat.as_deref().unwrap_or(""), cfg)
                    };
                    let div = match &tag.div_var {
                        Some(dv) => self.get_multiplier(env, dv, cfg, false),
                        None => tag.div.unwrap_or(1.0),
                    };
                    let mut mult = (base / div + 0.0001).floor();
                    let mut limit_total = None;
                    if tag.limit.is_some() || tag.limit_var.is_some() {
                        let limit = tag.limit.unwrap_or_else(|| {
                            self.get_multiplier(
                                env,
                                tag.limit_var.as_deref().unwrap_or(""),
                                cfg,
                                false,
                            )
                        });
                        if tag.limit_total {
                            limit_total = Some(limit);
                        } else {
                            mult = mult.min(limit);
                        }
                    }
                    scale_value(&mut value, |v| {
                        let v = v * mult + tag.base.unwrap_or(0.0);
                        limit_total.map_or(v, |l| v.min(l))
                    });
                }
                TagType::PercentStat => {
                    let target = tag.actor.as_deref().and_then(actor_db).unwrap_or(self);
                    let base: f64 = if let Some(list) = &tag.stat_list {
                        list.iter().map(|s| target.get_stat(env, s, cfg)).sum()
                    } else {
                        target.get_stat(env, tag.stat.as_deref().unwrap_or(""), cfg)
                    };
                    let percent = tag.percent.unwrap_or_else(|| {
                        self.get_multiplier(
                            env,
                            tag.percent_var.as_deref().unwrap_or(""),
                            cfg,
                            false,
                        )
                    });
                    let mut mult = base * (percent / 100.0);
                    if tag.floor {
                        mult = mult.floor();
                    }
                    let mut limit_total = None;
                    if tag.limit.is_some() || tag.limit_var.is_some() {
                        let limit = tag.limit.unwrap_or_else(|| {
                            self.get_multiplier(
                                env,
                                tag.limit_var.as_deref().unwrap_or(""),
                                cfg,
                                false,
                            )
                        });
                        if tag.limit_total {
                            limit_total = Some(limit);
                        } else {
                            mult = mult.min(limit);
                        }
                    }
                    scale_value(&mut value, |v| {
                        let v = (v * mult + tag.base.unwrap_or(0.0)).ceil();
                        limit_total.map_or(v, |l| v.min(l))
                    });
                }
                TagType::StatThreshold => {
                    let stat: f64 = if let Some(list) = &tag.stat_list {
                        list.iter().map(|s| self.get_stat(env, s, cfg)).sum()
                    } else {
                        self.get_stat(env, tag.stat.as_deref().unwrap_or(""), cfg)
                    };
                    let mut threshold = tag.threshold.unwrap_or_else(|| {
                        self.get_stat(env, tag.threshold_stat.as_deref().unwrap_or(""), cfg)
                    });
                    if tag.threshold_percent.is_some() || tag.threshold_percent_var.is_some() {
                        let pct = tag.threshold_percent.unwrap_or_else(|| {
                            self.get_multiplier(
                                env,
                                tag.threshold_percent_var.as_deref().unwrap_or(""),
                                cfg,
                                false,
                            )
                        });
                        threshold *= pct / 100.0;
                    }
                    if (tag.upper && stat > threshold) || (!tag.upper && stat < threshold) {
                        return None;
                    }
                }
                TagType::DistanceRamp => {
                    let dist = cfg?.skill_dist?;
                    let ramp = tag.ramp.as_ref()?;
                    let first = ramp.first()?;
                    let last = ramp.last()?;
                    let factor = if dist <= first[0] {
                        first[1]
                    } else if dist >= last[0] {
                        last[1]
                    } else {
                        let mut f = 1.0;
                        for w in ramp.windows(2) {
                            let (dat, next) = (&w[0], &w[1]);
                            if dist <= next[0] {
                                f = dat[1]
                                    + (next[1] - dat[1]) * (dist - dat[0]) / (next[0] - dat[0]);
                                break;
                            }
                        }
                        f
                    };
                    scale_value(&mut value, |v| v * factor);
                }
                TagType::MeleeProximity => {
                    let dist = cfg?.skill_dist?;
                    let max_bonus = tag
                        .ramp
                        .as_ref()
                        .and_then(|r| r.first())
                        .and_then(|r| r.first().copied())?;
                    let factor = if dist <= 15.0 {
                        max_bonus
                    } else if (16.0..=39.0).contains(&dist) {
                        max_bonus - (max_bonus / 25.0) * (dist - 15.0)
                    } else if dist >= 40.0 {
                        0.0
                    } else {
                        1.0
                    };
                    scale_value(&mut value, |v| v * factor);
                }
                TagType::Limit => {
                    let limit = tag.limit.unwrap_or_else(|| {
                        self.get_multiplier(env, tag.limit_var.as_deref().unwrap_or(""), cfg, false)
                    });
                    scale_value(&mut value, |v| v.min(limit));
                }
                TagType::Condition => {
                    let actor = env.actor(self.actor());
                    let all_one_h = [1u8, 2]
                        .into_iter()
                        .filter_map(|i| actor.weapon(i))
                        .find(|w| w.counts_as_all_1h());
                    let check = |var: &str| -> Option<bool> {
                        // `Some(result)` for a decided var; `None` → the tag fails outright.
                        if tag.neg {
                            if let Some(w) = all_one_h {
                                if let Some(added) = w.added_condition(var) {
                                    return if added { Some(false) } else { None };
                                }
                            }
                        }
                        Some(
                            self.get_condition(env, var, cfg, false)
                                || cfg
                                    .and_then(|c| c.skill_cond)
                                    .is_some_and(|sc| sc.get(var).copied().unwrap_or(false)),
                        )
                    };
                    let mut matched = false;
                    if let Some(list) = &tag.var_list {
                        for var in list {
                            if check(var)? {
                                matched = true;
                                break;
                            }
                        }
                    } else {
                        matched = check(tag.var.as_deref().unwrap_or(""))?;
                    }
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::ActorCondition => {
                    let target = match &tag.actor {
                        Some(a) => actor_db(a),
                        None => Some(self),
                    };
                    let mut matched = false;
                    if let (Some(target), true) =
                        (target, tag.var.is_some() || tag.var_list.is_some())
                    {
                        if let Some(list) = &tag.var_list {
                            matched = list
                                .iter()
                                .any(|v| target.get_condition(env, v, cfg, false));
                        } else {
                            matched = target.get_condition(
                                env,
                                tag.var.as_deref().unwrap_or(""),
                                cfg,
                                false,
                            );
                        }
                    } else if let (Some(a), Some(c)) = (&tag.actor, cfg) {
                        matched = c.actor == Some(a.as_str());
                    }
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::ItemCondition => {
                    if !self.item_condition(env, tag, cfg) {
                        return None;
                    }
                }
                TagType::SocketedIn => {
                    let cfg = cfg?;
                    if tag.slot_name.is_none()
                        && tag.keyword.is_none()
                        && tag.socket_color.is_none()
                    {
                        return None;
                    }
                    let mut checks: Vec<bool> = Vec::new();
                    if let Some(slot) = &tag.slot_name {
                        checks.push(cfg.slot_name == Some(slot.as_str()));
                    }
                    if let Some(kw) = &tag.keyword {
                        checks.push(cfg.skill_gem.is_some_and(|g| env.gem_is_type(g, kw)));
                    } else if tag.socket_color.is_some() && tag.sockets != Some(Sockets::All) {
                        checks.push(tag.socket_color.as_deref() == cfg.socket_color);
                    }
                    if let Some(sockets) = &tag.sockets {
                        let count = match tag.socket_color.as_deref() {
                            Some("R") => cfg.strength_gems,
                            Some("G") => cfg.dexterity_gems,
                            Some("B") => cfg.intelligence_gems,
                            _ => None,
                        }
                        .unwrap_or(0.0);
                        match sockets {
                            Sockets::All => {
                                let total = cfg.intelligence_gems.unwrap_or(0.0)
                                    + cfg.dexterity_gems.unwrap_or(0.0)
                                    + cfg.strength_gems.unwrap_or(0.0);
                                checks.push(total == count && total > 0.0);
                            }
                            Sockets::List(list) => match cfg.socket_num {
                                Some(n) => checks.push(list.contains(&n)),
                                None => return None,
                            },
                            Sockets::Count(n) => checks.push(count < *n),
                        }
                    }
                    for v in checks {
                        if (!tag.neg && !v) || (tag.neg && v) {
                            return None;
                        }
                    }
                }
                TagType::SkillName => {
                    let mut matched = if tag.include_transfigured {
                        let src = if tag.summon_skill {
                            cfg.and_then(|c| c.summon_skill_name)
                        } else {
                            cfg.and_then(|c| c.skill_name)
                        };
                        let id = src
                            .map(|n| env.game_id_from_gem_name(n))
                            .unwrap_or_default();
                        if let Some(list) = &tag.skill_name_list {
                            list.iter().any(|n| env.game_id_from_gem_name(n) == id)
                        } else {
                            tag.skill_name
                                .as_deref()
                                .is_some_and(|n| env.game_id_from_gem_name(n) == id)
                        }
                    } else {
                        let src = if tag.summon_skill {
                            cfg.and_then(|c| c.summon_skill_name)
                        } else {
                            cfg.and_then(|c| c.skill_name)
                        }
                        .unwrap_or("")
                        .to_lowercase();
                        if let Some(list) = &tag.skill_name_list {
                            list.iter().any(|n| n.to_lowercase() == src)
                        } else {
                            tag.skill_name
                                .as_deref()
                                .is_some_and(|n| n.to_lowercase() == src)
                        }
                    };
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::SkillId => {
                    let id = cfg?.skill_id?;
                    if Some(id) != tag.skill_id.as_deref() {
                        return None;
                    }
                }
                TagType::SkillPart => {
                    let cfg = cfg?;
                    let mut matched = if let Some(list) = &tag.skill_part_list {
                        cfg.skill_part.is_some_and(|p| list.contains(&p))
                    } else {
                        tag.skill_part == cfg.skill_part
                    };
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::SkillType => {
                    let types = cfg.and_then(|c| c.skill_types);
                    let mut matched = if let Some(list) = &tag.skill_type_list {
                        types.is_some_and(|t| list.iter().any(|x| t.contains(x)))
                    } else {
                        types.is_some_and(|t| tag.skill_type.is_some_and(|x| t.contains(&x)))
                    };
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::BaseFlag => {
                    let mut matched = cfg
                        .and_then(|c| c.base_flags)
                        .is_some_and(|f| tag.base_flag.as_ref().is_some_and(|b| f.contains(b)));
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::SlotName => {
                    let cfg = cfg?;
                    let mut matched = if let Some(list) = &tag.slot_name_list {
                        cfg.slot_name.is_some_and(|s| list.iter().any(|x| x == s))
                    } else {
                        tag.slot_name.as_deref() == cfg.slot_name
                    };
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                TagType::ModFlagOr => {
                    let cfg = cfg?;
                    if cfg.flags & tag.mod_flags == 0 {
                        return None;
                    }
                }
                TagType::KeywordFlagAnd => {
                    let cfg = cfg?;
                    if cfg.keyword_flags & tag.keyword_flags != tag.keyword_flags {
                        return None;
                    }
                }
                TagType::MonsterTag => {
                    let tags = env.actor(self.actor()).monster_tags()?;
                    let mut matched = tags.iter().any(|t| {
                        let t = t.to_lowercase();
                        if let Some(list) = &tag.monster_tag_list {
                            list.iter().any(|n| n.to_lowercase() == t)
                        } else {
                            tag.monster_tag
                                .as_deref()
                                .is_some_and(|n| n.to_lowercase() == t)
                        }
                    });
                    if tag.neg {
                        matched = !matched;
                    }
                    if !matched {
                        return None;
                    }
                }
                // Tags consumed elsewhere (GlobalEffect, SlotNumber, InSlot, …)
                // don't gate evaluation.
                _ => {}
            }
        }

        // Global limits.
        if let Some(limits) = global_limits {
            for tag in &m.tags {
                if let (Some(limit), Some(key)) = (tag.global_limit, &tag.global_limit_key) {
                    let mut v = value.num_or_zero();
                    let used = limits.entry(key.clone()).or_insert(0.0);
                    if *used + v > limit {
                        v = limit - *used;
                    }
                    *used += v;
                    value = Value::Num(v);
                }
            }
        }
        Some(value)
    }

    fn item_condition(self, env: &dyn EvalEnv, tag: &Tag, cfg: Option<&ModCfg<'_>>) -> bool {
        let actor = env.actor(self.actor());
        // "body armour" → "Body Armour", trimmed.
        let item_slot: String = tag
            .item_slot
            .as_deref()
            .unwrap_or("")
            .to_lowercase()
            .split(' ')
            .map(|w| {
                let mut c = w.chars();
                c.next().map_or_else(String::new, |f| {
                    f.to_uppercase().collect::<String>() + c.as_str()
                })
            })
            .collect::<Vec<_>>()
            .join(" ")
            .trim()
            .to_owned();
        let mut items: Vec<(String, &dyn ItemView)> = Vec::new();
        if tag.all_slots {
            items = actor
                .items()
                .into_iter()
                .map(|(s, i)| (s.to_owned(), i))
                .collect();
        } else if tag.both_slots {
            let s1 = format!("{item_slot} 1");
            let s2 = format!("{item_slot} 2");
            let mut i1 = actor.item(&s1);
            let mut i2 = actor.item(&s2);
            if i1.is_some_and(|i| i.name().contains("Kalandra's Touch")) {
                i1 = i2;
            }
            if i2.is_some_and(|i| i.name().contains("Kalandra's Touch")) {
                i2 = i1;
            }
            if let (Some(a), Some(b)) = (i1, i2) {
                items = vec![(s1, a), (s2, b)];
            }
        } else {
            let mut item = actor.item(&item_slot).or_else(|| cfg.and_then(|c| c.item));
            if item.is_some_and(|i| i.name().contains("Kalandra's Touch")) {
                let swapped = if let Some(stem) = item_slot.strip_suffix('1') {
                    format!("{stem}2")
                } else if let Some(stem) = item_slot.strip_suffix('2') {
                    format!("{stem}1")
                } else {
                    item_slot.clone()
                };
                item = actor.item(&swapped);
            }
            if let Some(item) = item.or_else(|| cfg.and_then(|c| c.item)) {
                items = vec![(item_slot.clone(), item)];
            }
        }
        let mut matches: Vec<bool> = Vec::new();
        if let Some(search) = &tag.search_cond {
            for (slot, item) in &items {
                let include = (!tag.all_slots
                    || (item.item_type() != "Jewel" && item.item_type() != "Graft"))
                    && *slot != item_slot
                    || !tag.exclude_self;
                if include {
                    matches.push(
                        item.find_modifier_substring(&search.to_lowercase(), &slot.to_lowercase()),
                    );
                }
            }
        }
        if let Some(r) = &tag.rarity_cond {
            matches.extend(items.iter().map(|(_, i)| i.rarity() == r));
        }
        if let Some(c) = tag.corrupted_cond {
            matches.extend(items.iter().map(|(_, i)| i.corrupted() == c));
        }
        if let Some(c) = tag.shaper_cond {
            matches.extend(items.iter().map(|(_, i)| i.shaper() == c));
        }
        if let Some(c) = tag.elder_cond {
            matches.extend(items.iter().map(|(_, i)| i.elder() == c));
        }
        if let Some(n) = &tag.name_cond {
            let n = n.to_lowercase();
            matches.extend(items.iter().map(|(_, i)| i.name().to_lowercase() == n));
        }
        let matched = !matches.iter().any(|b| *b == tag.neg);
        matched && (!items.is_empty() || tag.neg)
    }
}

fn cfg_parts<'c>(cfg: Option<&ModCfg<'c>>) -> (u32, u32, Option<&'c str>) {
    cfg.map_or((0, 0, None), |c| (c.flags, c.keyword_flags, c.source))
}

/// Apply `f` to a numeric value, or to `value.mod.value` / `value.value` of
/// a table value (PoB's per-tag scaling of structured values).
fn scale_value(value: &mut Value, f: impl Fn(f64) -> f64) {
    match value {
        Value::Num(n) => *n = f(*n),
        Value::Table(t) => {
            if let Some(Value::Mod(inner)) = t.get_mut("mod") {
                if let Value::Num(n) = &mut inner.value {
                    *n = f(*n);
                }
            } else if let Some(Value::Num(n)) = t.get_mut("value") {
                *n = f(*n);
            }
        }
        // Lua would error on arithmetic with nil/bool; PoB never does this.
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::modifier::{Mod, ModType as T, Tag};

    struct TestActor {
        output: AHashMap<String, f64>,
    }
    impl ActorView for TestActor {
        fn output(&self, stat: &str) -> Option<f64> {
            self.output.get(stat).copied()
        }
        fn weapon(&self, _: u8) -> Option<&dyn WeaponView> {
            None
        }
        fn item(&self, _: &str) -> Option<&dyn ItemView> {
            None
        }
        fn items(&self) -> Vec<(&str, &dyn ItemView)> {
            Vec::new()
        }
        fn monster_tags(&self) -> Option<&[String]> {
            None
        }
        fn special_stat(&self, _: &str, _: Option<&ModCfg<'_>>) -> Option<f64> {
            None
        }
    }
    struct TestEnv {
        dbs: Vec<ModDb>,
        actors: Vec<TestActor>,
    }
    impl EvalEnv for TestEnv {
        fn db(&self, id: DbId) -> &ModDb {
            &self.dbs[id.0 as usize]
        }
        fn actor(&self, id: ActorId) -> &dyn ActorView {
            &self.actors[id.0 as usize]
        }
        fn related_actor(&self, _: ActorId, rel: &str) -> Option<ActorId> {
            (rel == "enemy").then_some(ActorId(1))
        }
        fn actor_db(&self, id: ActorId) -> DbId {
            DbId(id.0)
        }
        fn high_precision(&self, name: &str, ty: ModType) -> Option<i32> {
            (name == "CritChance" && ty == ModType::Base).then_some(2)
        }
        fn game_id_from_gem_name(&self, name: &str) -> String {
            name.to_owned()
        }
        fn gem_is_type(&self, _: &GemRef, _: &str) -> bool {
            false
        }
    }
    fn env() -> TestEnv {
        TestEnv {
            dbs: vec![ModDb::new(ActorId(0)), ModDb::new(ActorId(1))],
            actors: vec![
                TestActor {
                    output: [("Str".to_owned(), 105.0)].into_iter().collect(),
                },
                TestActor {
                    output: AHashMap::default(),
                },
            ],
        }
    }

    #[test]
    fn sum_more_and_rounding() {
        let mut e = env();
        e.dbs[0].add_mod(Mod::new("Life", T::Base, 50.0));
        e.dbs[0].add_mod(Mod::new("Life", T::Inc, 10.0));
        e.dbs[0].add_mod(Mod::new("Life", T::More, 10.0));
        e.dbs[0].add_mod(Mod::new("Life", T::More, 10.0));
        let s = e.dbs[0].store();
        assert_eq!(s.sum(&e, T::Base, None, &["Life"]), 50.0);
        assert_eq!(s.sum(&e, T::Inc, None, &["Life"]), 10.0);
        // 1.1 * 1.1 = 1.21 → rounded to 2 decimals.
        assert_eq!(s.more(&e, None, &["Life"]), 1.21);
    }

    #[test]
    fn conditions_multipliers_and_per_stat() {
        let mut e = env();
        e.dbs[0].add_mod(Mod::new("Damage", T::Inc, 20.0).tag(Tag::condition("Leeching")));
        e.dbs[0].add_mod(Mod::new("Damage", T::Inc, 2.0).tag(Tag::multiplier("PowerCharge")));
        e.dbs[0].add_mod(Mod::new("Damage", T::Inc, 1.0).tag(Tag::per_stat("Str", 10.0)));
        e.dbs[0].multipliers.insert("PowerCharge".into(), 3.0);
        let s = e.dbs[0].store();
        // 2*3 + floor(105/10)=10
        assert_eq!(s.sum(&e, T::Inc, None, &["Damage"]), 16.0);
        e.dbs[0].conditions.insert("Leeching".into(), true);
        let s = e.dbs[0].store();
        assert_eq!(s.sum(&e, T::Inc, None, &["Damage"]), 36.0);
    }

    #[test]
    fn enemy_actor_condition_and_global_limit() {
        let mut e = env();
        e.dbs[1].conditions.insert("Shocked".into(), true);
        e.dbs[0].add_mod(
            Mod::new("Damage", T::Inc, 30.0).tag(Tag::actor_condition("enemy", "Shocked")),
        );
        let mut limited = Tag::multiplier("Rage");
        limited.global_limit = Some(50.0);
        limited.global_limit_key = Some("K".into());
        e.dbs[0].add_mod(Mod::new("Speed", T::Inc, 20.0).tag(limited.clone()));
        e.dbs[0].add_mod(Mod::new("Speed", T::Inc, 20.0).tag(limited));
        e.dbs[0].multipliers.insert("Rage".into(), 2.0);
        let s = e.dbs[0].store();
        assert_eq!(s.sum(&e, T::Inc, None, &["Damage"]), 30.0);
        // 40 + 40 capped at 50 total.
        assert_eq!(s.sum(&e, T::Inc, None, &["Speed"]), 50.0);
    }

    #[test]
    fn parent_chain_and_condition_flag() {
        let mut e = env();
        e.dbs[0].add_mod(Mod::new("Condition:Onslaught", T::Flag, true));
        e.dbs[0].add_mod(Mod::new("Speed", T::Inc, 10.0));
        let mut list = ModList::new(ActorId(0), Some(DbId(0)));
        list.add_mod(Mod::new("Speed", T::Inc, 5.0).tag(Tag::condition("Onslaught")));
        let s = list.store();
        assert_eq!(s.sum(&e, T::Inc, None, &["Speed"]), 15.0);
        assert!(s.get_condition(&e, "Onslaught", None, false));
    }
}
