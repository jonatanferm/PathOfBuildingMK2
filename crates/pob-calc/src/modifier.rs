//! PoB's modifier model (`modLib.createMod` tables), as typed Rust.
//!
//! A PoB mod is a Lua table `{ name, type, value, flags, keywordFlags, source,
//! [1] = tag, [2] = tag, … }`. Tags are tables with a `type` plus type-specific
//! fields. Values are usually numbers but may be booleans, strings, or tables
//! (`{ mod = <mod> }`, `{ key = …, value = … }`, …), so [`Value`] mirrors a
//! Lua value closely enough to round-trip everything PoB stores.

use std::fmt;

use smallvec::SmallVec;

/// `mod.type`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum ModType {
    Base,
    Inc,
    More,
    Flag,
    Override,
    List,
    Max,
    Min,
    /// PoB's distinct `"Flag"` type (only `CanNotUseItem`, tabulated by
    /// CalcSetup) — not the same as `"FLAG"`.
    FlagCanNotUse,
    /// `"CHANCE"` (a handful of skill stat maps).
    Chance,
    /// `"DUMMY"` placeholders in skill stat maps.
    Dummy,
}

impl ModType {
    #[must_use]
    pub fn parse(s: &str) -> Option<Self> {
        Some(match s {
            "BASE" => Self::Base,
            "INC" => Self::Inc,
            "MORE" => Self::More,
            "FLAG" => Self::Flag,
            "OVERRIDE" => Self::Override,
            "LIST" => Self::List,
            "MAX" => Self::Max,
            "MIN" => Self::Min,
            "Flag" => Self::FlagCanNotUse,
            "CHANCE" => Self::Chance,
            "DUMMY" => Self::Dummy,
            _ => return None,
        })
    }

    #[must_use]
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Base => "BASE",
            Self::Inc => "INC",
            Self::More => "MORE",
            Self::Flag => "FLAG",
            Self::Override => "OVERRIDE",
            Self::List => "LIST",
            Self::Max => "MAX",
            Self::Min => "MIN",
            Self::FlagCanNotUse => "Flag",
            Self::Chance => "CHANCE",
            Self::Dummy => "DUMMY",
        }
    }
}

impl fmt::Display for ModType {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.write_str(self.as_str())
    }
}

/// A Lua-like value. `Table` covers PoB's structured values (`{ mod = … }`,
/// `{ key = …, value = … }`, arrays).
#[derive(Debug, Clone, PartialEq, Default)]
pub enum Value {
    #[default]
    Nil,
    Bool(bool),
    Num(f64),
    Str(String),
    Mod(Box<Mod>),
    Table(Box<Table>),
}

/// A Lua table: string-keyed fields (in insertion order) plus an array part.
/// Equality ignores field order, like Lua tables.
#[derive(Debug, Clone, Default)]
pub struct Table {
    pub fields: Vec<(String, Value)>,
    pub array: Vec<Value>,
}

impl PartialEq for Table {
    fn eq(&self, other: &Self) -> bool {
        self.array == other.array
            && self.fields.len() == other.fields.len()
            && self.fields.iter().all(|(k, v)| other.get(k) == Some(v))
    }
}

impl Table {
    #[must_use]
    pub fn get(&self, key: &str) -> Option<&Value> {
        self.fields.iter().find(|(k, _)| k == key).map(|(_, v)| v)
    }

    pub fn get_mut(&mut self, key: &str) -> Option<&mut Value> {
        self.fields
            .iter_mut()
            .find(|(k, _)| k == key)
            .map(|(_, v)| v)
    }

    pub fn set(&mut self, key: impl Into<String>, value: Value) {
        let key = key.into();
        if let Some(slot) = self.get_mut(&key) {
            *slot = value;
        } else {
            self.fields.push((key, value));
        }
    }

    #[must_use]
    pub fn with(mut self, key: impl Into<String>, value: impl Into<Value>) -> Self {
        self.set(key, value.into());
        self
    }
}

impl Value {
    #[must_use]
    pub fn num(&self) -> Option<f64> {
        match self {
            Self::Num(n) => Some(*n),
            _ => None,
        }
    }

    /// Numeric value, treating non-numbers as 0 (Lua `x or 0` idiom).
    #[must_use]
    pub fn num_or_zero(&self) -> f64 {
        self.num().unwrap_or(0.0)
    }

    #[must_use]
    pub fn as_str(&self) -> Option<&str> {
        match self {
            Self::Str(s) => Some(s),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_table(&self) -> Option<&Table> {
        match self {
            Self::Table(t) => Some(t),
            _ => None,
        }
    }

    pub fn as_table_mut(&mut self) -> Option<&mut Table> {
        match self {
            Self::Table(t) => Some(t),
            _ => None,
        }
    }

    #[must_use]
    pub fn as_mod(&self) -> Option<&Mod> {
        match self {
            Self::Mod(m) => Some(m),
            _ => None,
        }
    }

    /// Lua truthiness: everything except `nil` and `false`.
    #[must_use]
    pub fn truthy(&self) -> bool {
        !matches!(self, Self::Nil | Self::Bool(false))
    }

    #[must_use]
    pub fn is_nil(&self) -> bool {
        matches!(self, Self::Nil)
    }

    /// `value.mod` of a `{ mod = … }` table value.
    #[must_use]
    pub fn inner_mod(&self) -> Option<&Mod> {
        self.as_table()?.get("mod")?.as_mod()
    }

    pub fn inner_mod_mut(&mut self) -> Option<&mut Mod> {
        match self.as_table_mut()?.get_mut("mod")? {
            Value::Mod(m) => Some(m),
            _ => None,
        }
    }

    #[must_use]
    pub fn table(fields: Vec<(&str, Value)>) -> Self {
        Self::Table(Box::new(Table {
            fields: fields.into_iter().map(|(k, v)| (k.to_owned(), v)).collect(),
            array: Vec::new(),
        }))
    }
}

impl From<f64> for Value {
    fn from(n: f64) -> Self {
        Self::Num(n)
    }
}
impl From<i32> for Value {
    fn from(n: i32) -> Self {
        Self::Num(f64::from(n))
    }
}
impl From<bool> for Value {
    fn from(b: bool) -> Self {
        Self::Bool(b)
    }
}
impl From<&str> for Value {
    fn from(s: &str) -> Self {
        Self::Str(s.to_owned())
    }
}
impl From<String> for Value {
    fn from(s: String) -> Self {
        Self::Str(s)
    }
}
impl From<Mod> for Value {
    fn from(m: Mod) -> Self {
        Self::Mod(Box::new(m))
    }
}
impl From<Table> for Value {
    fn from(t: Table) -> Self {
        Self::Table(Box::new(t))
    }
}

/// `tag.type`.
#[derive(Debug, Clone, PartialEq, Eq, Hash)]
pub enum TagType {
    Condition,
    ActorCondition,
    Multiplier,
    MultiplierThreshold,
    PerStat,
    PercentStat,
    StatThreshold,
    DistanceRamp,
    MeleeProximity,
    Limit,
    ItemCondition,
    SocketedIn,
    SkillName,
    SkillId,
    SkillPart,
    SkillType,
    BaseFlag,
    SlotName,
    SlotNumber,
    ModFlagOr,
    KeywordFlagAnd,
    MonsterTag,
    GlobalEffect,
    InSlot,
    DisablesItem,
    IgnoreCond,
    Other(String),
}

impl TagType {
    #[must_use]
    pub fn parse(s: &str) -> Self {
        match s {
            "Condition" => Self::Condition,
            "ActorCondition" => Self::ActorCondition,
            "Multiplier" => Self::Multiplier,
            "MultiplierThreshold" => Self::MultiplierThreshold,
            "PerStat" => Self::PerStat,
            "PercentStat" => Self::PercentStat,
            "StatThreshold" => Self::StatThreshold,
            "DistanceRamp" => Self::DistanceRamp,
            "MeleeProximity" => Self::MeleeProximity,
            "Limit" => Self::Limit,
            "ItemCondition" => Self::ItemCondition,
            "SocketedIn" => Self::SocketedIn,
            "SkillName" => Self::SkillName,
            "SkillId" => Self::SkillId,
            "SkillPart" => Self::SkillPart,
            "SkillType" => Self::SkillType,
            "BaseFlag" => Self::BaseFlag,
            "SlotName" => Self::SlotName,
            "SlotNumber" => Self::SlotNumber,
            "ModFlagOr" => Self::ModFlagOr,
            "KeywordFlagAnd" => Self::KeywordFlagAnd,
            "MonsterTag" => Self::MonsterTag,
            "GlobalEffect" => Self::GlobalEffect,
            "InSlot" => Self::InSlot,
            "DisablesItem" => Self::DisablesItem,
            "IgnoreCond" => Self::IgnoreCond,
            other => Self::Other(other.to_owned()),
        }
    }

    #[must_use]
    pub fn as_str(&self) -> &str {
        match self {
            Self::Condition => "Condition",
            Self::ActorCondition => "ActorCondition",
            Self::Multiplier => "Multiplier",
            Self::MultiplierThreshold => "MultiplierThreshold",
            Self::PerStat => "PerStat",
            Self::PercentStat => "PercentStat",
            Self::StatThreshold => "StatThreshold",
            Self::DistanceRamp => "DistanceRamp",
            Self::MeleeProximity => "MeleeProximity",
            Self::Limit => "Limit",
            Self::ItemCondition => "ItemCondition",
            Self::SocketedIn => "SocketedIn",
            Self::SkillName => "SkillName",
            Self::SkillId => "SkillId",
            Self::SkillPart => "SkillPart",
            Self::SkillType => "SkillType",
            Self::BaseFlag => "BaseFlag",
            Self::SlotName => "SlotName",
            Self::SlotNumber => "SlotNumber",
            Self::ModFlagOr => "ModFlagOr",
            Self::KeywordFlagAnd => "KeywordFlagAnd",
            Self::MonsterTag => "MonsterTag",
            Self::GlobalEffect => "GlobalEffect",
            Self::InSlot => "InSlot",
            Self::DisablesItem => "DisablesItem",
            Self::IgnoreCond => "IgnoreCond",
            Self::Other(s) => s,
        }
    }
}

/// `tag.sockets` for `SocketedIn`: `"all"`, a count, or a list of socket numbers.
#[derive(Debug, Clone, PartialEq)]
pub enum Sockets {
    All,
    Count(f64),
    List(Vec<f64>),
}

/// One tag of a mod. Field names follow PoB's (`tag.limitVar` → `limit_var`).
/// Fields PoB sets that aren't modelled explicitly land in `extra`.
#[derive(Debug, Clone, PartialEq)]
#[allow(clippy::struct_field_names)]
pub struct Tag {
    pub ty: TagType,
    pub var: Option<String>,
    pub var_list: Option<Vec<String>>,
    pub neg: bool,
    pub actor: Option<String>,
    pub stat: Option<String>,
    pub stat_list: Option<Vec<String>>,
    pub div: Option<f64>,
    pub div_var: Option<String>,
    pub base: Option<f64>,
    pub no_floor: bool,
    pub invert: bool,
    pub floor: bool,
    pub limit: Option<f64>,
    pub limit_var: Option<String>,
    pub limit_stat: Option<String>,
    pub limit_total: bool,
    pub limit_neg_total: bool,
    pub limit_actor: Option<String>,
    pub percent: Option<f64>,
    pub percent_var: Option<String>,
    pub threshold: Option<f64>,
    pub threshold_var: Option<String>,
    pub threshold_stat: Option<String>,
    pub threshold_actor: Option<String>,
    pub threshold_percent: Option<f64>,
    pub threshold_percent_var: Option<String>,
    pub upper: bool,
    pub equals: bool,
    /// `DistanceRamp`: `{{dist, mult}, …}`; `MeleeProximity`: `{max, min}` (one row).
    pub ramp: Option<Vec<Vec<f64>>>,
    pub item_slot: Option<String>,
    pub all_slots: bool,
    pub both_slots: bool,
    pub exclude_self: bool,
    pub search_cond: Option<String>,
    pub rarity_cond: Option<String>,
    pub corrupted_cond: Option<bool>,
    pub shaper_cond: Option<bool>,
    pub elder_cond: Option<bool>,
    pub name_cond: Option<String>,
    pub slot_name: Option<String>,
    pub slot_name_list: Option<Vec<String>>,
    pub keyword: Option<String>,
    pub socket_color: Option<String>,
    pub sockets: Option<Sockets>,
    pub skill_name: Option<String>,
    pub skill_name_list: Option<Vec<String>>,
    pub include_transfigured: bool,
    pub summon_skill: bool,
    pub skill_id: Option<String>,
    pub skill_part: Option<f64>,
    pub skill_part_list: Option<Vec<f64>>,
    pub skill_type: Option<u32>,
    pub skill_type_list: Option<Vec<u32>>,
    pub base_flag: Option<String>,
    pub mod_flags: u32,
    pub keyword_flags: u32,
    pub monster_tag: Option<String>,
    pub monster_tag_list: Option<Vec<String>>,
    pub global_limit: Option<f64>,
    pub global_limit_key: Option<String>,
    pub num: Option<f64>,
    // GlobalEffect
    pub effect_type: Option<String>,
    pub effect_name: Option<String>,
    pub effect_cond: Option<String>,
    pub effect_enemy_cond: Option<String>,
    pub effect_stack_var: Option<String>,
    pub effect_stack_limit: Option<f64>,
    pub mod_cond: Option<String>,
    pub unscalable: bool,
    pub allow_totem_buff: bool,
    pub apply_not_player: bool,
    pub apply_minions: bool,
    pub extra: Vec<(String, Value)>,
}

impl Tag {
    #[must_use]
    pub fn new(ty: TagType) -> Self {
        Self {
            ty,
            var: None,
            var_list: None,
            neg: false,
            actor: None,
            stat: None,
            stat_list: None,
            div: None,
            div_var: None,
            base: None,
            no_floor: false,
            invert: false,
            floor: false,
            limit: None,
            limit_var: None,
            limit_stat: None,
            limit_total: false,
            limit_neg_total: false,
            limit_actor: None,
            percent: None,
            percent_var: None,
            threshold: None,
            threshold_var: None,
            threshold_stat: None,
            threshold_actor: None,
            threshold_percent: None,
            threshold_percent_var: None,
            upper: false,
            equals: false,
            ramp: None,
            item_slot: None,
            all_slots: false,
            both_slots: false,
            exclude_self: false,
            search_cond: None,
            rarity_cond: None,
            corrupted_cond: None,
            shaper_cond: None,
            elder_cond: None,
            name_cond: None,
            slot_name: None,
            slot_name_list: None,
            keyword: None,
            socket_color: None,
            sockets: None,
            skill_name: None,
            skill_name_list: None,
            include_transfigured: false,
            summon_skill: false,
            skill_id: None,
            skill_part: None,
            skill_part_list: None,
            skill_type: None,
            skill_type_list: None,
            base_flag: None,
            mod_flags: 0,
            keyword_flags: 0,
            monster_tag: None,
            monster_tag_list: None,
            global_limit: None,
            global_limit_key: None,
            num: None,
            effect_type: None,
            effect_name: None,
            effect_cond: None,
            effect_enemy_cond: None,
            effect_stack_var: None,
            effect_stack_limit: None,
            mod_cond: None,
            unscalable: false,
            allow_totem_buff: false,
            apply_not_player: false,
            apply_minions: false,
            extra: Vec::new(),
        }
    }

    // Short constructors for the common shapes.

    #[must_use]
    pub fn condition(var: &str) -> Self {
        Self {
            var: Some(var.to_owned()),
            ..Self::new(TagType::Condition)
        }
    }

    #[must_use]
    pub fn condition_neg(var: &str) -> Self {
        Self {
            neg: true,
            ..Self::condition(var)
        }
    }

    #[must_use]
    pub fn actor_condition(actor: &str, var: &str) -> Self {
        Self {
            actor: Some(actor.to_owned()),
            var: Some(var.to_owned()),
            ..Self::new(TagType::ActorCondition)
        }
    }

    #[must_use]
    pub fn multiplier(var: &str) -> Self {
        Self {
            var: Some(var.to_owned()),
            ..Self::new(TagType::Multiplier)
        }
    }

    #[must_use]
    pub fn per_stat(stat: &str, div: f64) -> Self {
        Self {
            stat: Some(stat.to_owned()),
            div: Some(div),
            ..Self::new(TagType::PerStat)
        }
    }

    #[must_use]
    pub fn skill_name(name: &str) -> Self {
        Self {
            skill_name: Some(name.to_owned()),
            ..Self::new(TagType::SkillName)
        }
    }

    #[must_use]
    pub fn skill_type(ty: u32) -> Self {
        Self {
            skill_type: Some(ty),
            ..Self::new(TagType::SkillType)
        }
    }

    #[must_use]
    pub fn global_effect(effect_type: &str) -> Self {
        Self {
            effect_type: Some(effect_type.to_owned()),
            ..Self::new(TagType::GlobalEffect)
        }
    }
}

/// A modifier. `tags` are the table's array part (`mod[1]`, `mod[2]`, …).
#[derive(Debug, Clone, PartialEq)]
pub struct Mod {
    pub name: String,
    pub ty: ModType,
    pub value: Value,
    pub flags: u32,
    pub keyword_flags: u32,
    pub source: Option<String>,
    pub tags: SmallVec<[Tag; 1]>,
    /// ReplaceMod bookkeeping (`mod.replaced` / `mod.converted` in PoB).
    pub replaced: bool,
    pub converted: bool,
}

impl Mod {
    /// `modLib.createMod(name, type, value, source?, flags?, keywordFlags?, tags…)`.
    #[must_use]
    pub fn new(name: impl Into<String>, ty: ModType, value: impl Into<Value>) -> Self {
        Self {
            name: name.into(),
            ty,
            value: value.into(),
            flags: 0,
            keyword_flags: 0,
            source: None,
            tags: SmallVec::new(),
            replaced: false,
            converted: false,
        }
    }

    #[must_use]
    pub fn source(mut self, source: impl Into<String>) -> Self {
        self.source = Some(source.into());
        self
    }

    #[must_use]
    pub fn flags(mut self, flags: u32, keyword_flags: u32) -> Self {
        self.flags = flags;
        self.keyword_flags = keyword_flags;
        self
    }

    #[must_use]
    pub fn tag(mut self, tag: Tag) -> Self {
        self.tags.push(tag);
        self
    }

    /// `modLib.setSource`: also sets the source of a wrapped `{ mod = … }`.
    pub fn set_source(&mut self, source: &str) {
        self.source = Some(source.to_owned());
        if let Some(inner) = self.value.inner_mod_mut() {
            inner.source = Some(source.to_owned());
        }
    }

    /// Leading source category: `mod.source:match("[^:]+")`.
    #[must_use]
    pub fn source_category(&self) -> Option<&str> {
        let s = self.source.as_deref()?;
        Some(s.split(':').next().unwrap_or(s))
    }
}

fn fmt_num(n: f64) -> String {
    if n.fract() == 0.0 && n.abs() < 1e15 {
        format!("{}", n as i64)
    } else {
        format!("{n}")
    }
}

impl fmt::Display for Value {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Value::Nil => f.write_str("nil"),
            Value::Bool(b) => write!(f, "{b}"),
            Value::Num(n) => f.write_str(&fmt_num(*n)),
            Value::Str(s) => f.write_str(s),
            Value::Mod(m) => write!(f, "[{m}]"),
            Value::Table(t) => {
                let mut parts: Vec<String> =
                    t.fields.iter().map(|(k, v)| format!("{k}={v}")).collect();
                parts.sort();
                if !t.array.is_empty() {
                    parts.push(
                        t.array
                            .iter()
                            .map(ToString::to_string)
                            .collect::<Vec<_>>()
                            .join(","),
                    );
                }
                write!(f, "{{{}}}", parts.join("/"))
            }
        }
    }
}

impl fmt::Display for Tag {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // Compact: only set fields, via the Debug repr filtered to non-defaults.
        let dbg = format!("{self:?}");
        let inner = dbg.trim_start_matches("Tag {").trim_end_matches('}');
        let parts: Vec<&str> = inner
            .split(", ")
            .map(str::trim)
            .filter(|p| {
                !(p.ends_with(": None")
                    || p.ends_with(": false")
                    || p.ends_with(": 0")
                    || p.ends_with(": []")
                    || p.starts_with("ty:"))
            })
            .collect();
        write!(f, "{}{{{}}}", self.ty.as_str(), parts.join(","))
    }
}

impl fmt::Display for Mod {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(
            f,
            "{} = {}|{}|{:#x}|{:#x}",
            self.value, self.name, self.ty, self.flags, self.keyword_flags
        )?;
        for t in &self.tags {
            write!(f, "|{t}")?;
        }
        if let Some(s) = &self.source {
            write!(f, " ({s})")?;
        }
        Ok(())
    }
}
