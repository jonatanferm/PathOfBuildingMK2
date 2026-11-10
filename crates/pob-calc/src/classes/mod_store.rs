//! @pob-file Classes/ModStore.lua @16de4b82d
//! Classes/ModStore.lua 1-362: the shared ModStore methods (calc doc §5.3). The struct and the
//! constructors are U0's (`classes/mod.rs`); `getActor` and `EvalMod` are MS-B's (`eval_mod.rs`).
//!
//! Owner: MS-A. Lua method dispatch (`self:SumInternal(...)` resolving to ModDB's or ModList's
//! method) is the Rust-only dispatcher of the same name below; the bodies are `*_DB`
//! (`mod_db.rs`) and `*_List` (`mod_list.rs`).
//!
//! Borrow rule §3.3: the store's `mods` are only borrowed for one short step at a time (one
//! mod handle is cloned out per step), never across a call into `EvalMod`, a parent store or
//! another table.
#![allow(non_snake_case)]
// Lua statement shapes are kept (`local value` then assigned per branch; `if … then return … else … end`).
#![allow(clippy::needless_late_init, clippy::redundant_else)]

use crate::prelude::*;
use pob_gamedata::mod_tools;
use smallvec::SmallVec;

use super::StoreMods;

// @pob Classes/ModStore.lua:6-38 h=6234244b
// (module-level statements: ipairs,pairs,select,t_insert,m_floor,m_ceil,m_min,m_max,m_modf,band,bor,mod_createMod,multiplierName…)
// multiplierName / conditionName are `pob_lua::{multiplierName, conditionName}` (memoised).

// ---- shared helpers for the *Internal bodies (Rust-only plumbing) ----

/// Lua `type(v)` of a mod value.
pub(crate) fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        Value::Bool(_) => "boolean",
        Value::Num(_) => "number",
        Value::Str(_) => "string",
        Value::Mod(_) | Value::Table(_) | Value::JewelFunc(_) => "table",
    }
}

/// A mod value used as an arithmetic operand (`result + mod.value`, `mod.value / 100`):
/// numbers, and numeric strings by Lua's coercion; anything else raises as Lua does.
pub(crate) fn arith(v: &Value, what: &str) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Str(s) => pob_lua::tonumber(s).unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on {what} (a string value)")
        }),
        other => lua_error!(
            "attempt to perform arithmetic on {what} (a {} value)",
            value_type(other)
        ),
    }
}

/// `x or 0` used as an arithmetic operand (`context:EvalMod(mod, cfg) or 0`).
pub(crate) fn or0(v: &Value, what: &str) -> f64 {
    if v.truthy() {
        arith(v, what)
    } else {
        0.0
    }
}

/// `band(flags, mod.flags) == mod.flags`. A tag table given as `flags` (two data mods,
/// SkillStatMap.lua:509; kept in `extra["flags"]` by the loader) makes Lua's `band` raise.
#[inline]
pub(crate) fn flags_match(flags: u32, m: &Mod) -> bool {
    if m.extra.is_some() {
        if let Some(v) = m.extra_get(n!("flags")) {
            lua_error!(
                "bad argument #2 to 'band' (number expected, got {})",
                value_type(v)
            );
        }
    }
    pob_lua::bit::band(flags, m.flags) == m.flags
}

/// `(not source or mod.source:match("[^:]+") == source)`: indexing a nil `mod.source`
/// raises when a source is asked for.
#[inline]
pub(crate) fn source_match(source: Option<&str>, m: &Mod) -> bool {
    match source {
        None => true,
        Some(source) => {
            if m.source.is_none() {
                lua_error!("attempt to index field 'source' (a nil value)");
            }
            m.source_category() == Some(source)
        }
    }
}

/// `data.highPrecisionMods[mod.name] and data.highPrecisionMods[mod.name][mod.type]`.
#[inline]
pub(crate) fn highPrecision(name: Name, ty: ModType) -> Option<f64> {
    data()
        .highPrecisionMods
        .get(&name)
        .and_then(|t| t.get(&ty))
        .map(|p| f64::from(*p))
}

/// `mod[k]` for the boolean marks ModDB writes into stored mods (`replaced`, `converted`).
#[inline]
pub(crate) fn mod_mark(m: &Mod, k: Name) -> bool {
    m.extra.is_some() && m.extra_get(k).is_some_and(Value::truthy)
}

/// `mod[k] = true` on a mod the caller owns (ModDB.lua:56 `mod.replaced = true`, :100
/// `mod.converted = true`). The callers (ReplaceMod/ConvertMod/ScaleAddMod) always pass a
/// fresh mod (§3.6), so `make_mut` writes in place; a shared handle would be copied.
pub(crate) fn set_mod_mark(m: &mut ModRef, k: Name) {
    let x = Arc::make_mut(m);
    let mut ex = x.extra.take().map(|b| *b).unwrap_or_default();
    ex.set(k, Value::Bool(true));
    x.extra = Some(Box::new(ex));
}

/// `cfg.flags or 0`, `cfg.keywordFlags or 0`, `cfg.source` (ModStore.lua 166-172 and the
/// five copies after it).
#[inline]
fn cfg_filter(cfg: Option<&ModCfg>) -> (u32, u32, Option<Name>) {
    // L166
    let mut flags = 0;
    let mut keywordFlags = 0;
    // L167
    let mut source = None;
    // L168
    if let Some(cfg) = cfg {
        // L169
        flags = cfg.flags.get();
        // L170
        keywordFlags = cfg.keywordFlags.get();
        // L171
        source = cfg.source.get();
    }
    (flags, keywordFlags, source)
}

/// Lua comparison operand (`val > (max or 0)`): only numbers compare with numbers.
fn cmp_operand(v: &Value) -> f64 {
    match v {
        Value::Num(n) => *n,
        other => lua_error!("attempt to compare number with {}", value_type(other)),
    }
}

// ---- Lua method dispatch (Rust-only plumbing) ----

impl ModStore {
    /// `self:ReplaceModInternal(mod)` (ModDB.lua:42 / ModList.lua:36).
    pub(crate) fn ReplaceModInternal(&self, m: ModRef) -> bool {
        match self.kind {
            StoreKind::ModDB => self.ReplaceModInternal_DB(m),
            StoreKind::ModList => self.ReplaceModInternal_List(m),
        }
    }

    /// `self:ConvertModInternal(oldName, mod)` (ModDB.lua:80 / ModList.lua:58).
    pub(crate) fn ConvertModInternal(&self, oldName: Name, m: ModRef) -> bool {
        match self.kind {
            StoreKind::ModDB => self.ConvertModInternal_DB(oldName, m),
            StoreKind::ModList => self.ConvertModInternal_List(oldName, m),
        }
    }

    /// `self:SumInternal(context, …)`.
    pub(crate) fn SumInternal(
        &self,
        context: &ModStore,
        modType: ModType,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        match self.kind {
            StoreKind::ModDB => {
                self.SumInternal_DB(context, modType, cfg, flags, keywordFlags, source, names)
            }
            StoreKind::ModList => {
                self.SumInternal_List(context, modType, cfg, flags, keywordFlags, source, names)
            }
        }
    }

    /// `self:MoreInternal(context, …)`.
    pub(crate) fn MoreInternal(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        match self.kind {
            StoreKind::ModDB => {
                self.MoreInternal_DB(context, cfg, flags, keywordFlags, source, names)
            }
            StoreKind::ModList => {
                self.MoreInternal_List(context, cfg, flags, keywordFlags, source, names)
            }
        }
    }

    /// `self:FlagInternal(context, …)` (Lua returns `true` or nil; nil is `false` here).
    pub(crate) fn FlagInternal(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        match self.kind {
            StoreKind::ModDB => {
                self.FlagInternal_DB(context, cfg, flags, keywordFlags, source, names)
            }
            StoreKind::ModList => {
                self.FlagInternal_List(context, cfg, flags, keywordFlags, source, names)
            }
        }
    }

    /// `self:OverrideInternal(context, …)`.
    pub(crate) fn OverrideInternal(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> Value {
        match self.kind {
            StoreKind::ModDB => {
                self.OverrideInternal_DB(context, cfg, flags, keywordFlags, source, names)
            }
            StoreKind::ModList => {
                self.OverrideInternal_List(context, cfg, flags, keywordFlags, source, names)
            }
        }
    }

    /// `self:ListInternal(context, result, …)`.
    pub(crate) fn ListInternal(
        &self,
        context: &ModStore,
        result: &mut Vec<Value>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        match self.kind {
            StoreKind::ModDB => {
                self.ListInternal_DB(context, result, cfg, flags, keywordFlags, source, names);
            }
            StoreKind::ModList => {
                self.ListInternal_List(context, result, cfg, flags, keywordFlags, source, names);
            }
        }
    }

    /// `self:TabulateInternal(context, result, …)`.
    pub(crate) fn TabulateInternal(
        &self,
        context: &ModStore,
        result: &mut Vec<TabRow>,
        modType: Option<ModType>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        match self.kind {
            StoreKind::ModDB => self.TabulateInternal_DB(
                context,
                result,
                modType,
                cfg,
                flags,
                keywordFlags,
                source,
                names,
            ),
            StoreKind::ModList => self.TabulateInternal_List(
                context,
                result,
                modType,
                cfg,
                flags,
                keywordFlags,
                source,
                names,
            ),
        }
    }

    /// `self:HasModInternal(…)`: ModList has no such method, so Lua raises there.
    pub(crate) fn HasModInternal(
        &self,
        modType: ModType,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        match self.kind {
            StoreKind::ModDB => self.HasModInternal_DB(modType, flags, keywordFlags, source, names),
            // LUA-QUIRK: ModList.lua defines no HasModInternal.
            StoreKind::ModList => {
                lua_error!("attempt to call method 'HasModInternal' (a nil value)")
            }
        }
    }
}

/// @pob Classes/ModStore.lua:40-46 h=abc6532b
impl ModStore {
    /// `ModStore:ModStore(parent)`. `ModStore::ModDB`/`ModList` (U0) allocate the store with
    /// exactly this state; this re-runs the body on an existing store. The `multipliers` and
    /// `conditions` tables are fixed handles (contracts.md#modstore), so `= { }` empties them.
    #[allow(dead_code)]
    pub(crate) fn init_ModStore(&self, parent: Option<&ModStoreRef>) {
        // L41
        self.parent.set(parent.cloned());
        // L42
        self.actor
            .set(parent.map_or_else(Weak::new, |p| p.actor.get()));
        // L43
        self.multipliers.wipe();
        // L44
        self.conditions.wipe();
        // L45 return self
    }
}

/// @pob Classes/ModStore.lua:56-91 h=664c3a90
impl ModStore {
    pub fn ScaleAddMod(&self, m: &ModRef, scale: f64, replace: bool) {
        // L57
        let mut unscalable = false;
        // L58
        for effects in &m.tags {
            // L59
            if tag_unscalable(effects) {
                // L60
                unscalable = true;
                // L61
                break;
            }
        }
        // L64
        if scale == 1.0 || unscalable {
            // L65
            self.AddMod(m.clone());
        } else {
            // L67
            let mut scaledMod: Mod = (**m).clone();
            // L68 local subMod = scaledMod
            // `subMod` is either `scaledMod` itself or the copied `scaledMod.value.mod`; the
            // inner copy is rebuilt into `scaledMod.value` once scaled.
            let mut subModInner: Option<Value> = None;
            // L69
            if value_type(&scaledMod.value) == "table" {
                let vt = value_index(&scaledMod.value, n!("mod"));
                // L70
                if vt.truthy() {
                    // L71
                    subModInner = Some(vt);
                } else if value_index(&scaledMod.value, n!("keyOfScaledMod")).truthy() {
                    // L72-73
                    let key = value_index(&scaledMod.value, n!("keyOfScaledMod"));
                    let cur = value_index_by(&scaledMod.value, &key);
                    let scaled = round(arith(&cur, "field '?'") * scale, Some(2));
                    scaledMod.value = value_with(&scaledMod.value, &key, Value::Num(scaled));
                }
            }
            match subModInner {
                None => {
                    // L76
                    if let Value::Num(v) = scaledMod.value {
                        scaledMod.value =
                            Value::Num(scale_number(scaledMod.name, scaledMod.r#type, v, scale));
                    }
                }
                Some(inner) => {
                    // L76 on `subMod = scaledMod.value.mod`
                    let scaledInner = match &inner {
                        Value::Mod(sub) => match sub.value {
                            Value::Num(v) => {
                                let mut sub2: Mod = (**sub).clone();
                                sub2.value =
                                    Value::Num(scale_number(sub.name, sub.r#type, v, scale));
                                Some(Value::Mod(Arc::new(sub2)))
                            }
                            _ => None,
                        },
                        Value::Table(t) => match t.get(n!("value")) {
                            Value::Num(v) => {
                                // a mod-shaped table the loader kept as a plain table
                                let name = t.get(n!("name"));
                                let ty = t.get(n!("type"));
                                let precision = scale_precision_dyn(name, ty, *v);
                                let nv = scale_with_precision(*v, scale, precision);
                                Some(Value::Table(Arc::new(t.with(n!("value"), Value::Num(nv)))))
                            }
                            _ => None,
                        },
                        // `subMod.value` on a string is nil (string methods), so no scaling.
                        Value::Str(_) => None,
                        Value::JewelFunc(_) => None,
                        other => lua_error!(
                            "attempt to index local 'subMod' (a {} value)",
                            value_type(other)
                        ),
                    };
                    if let Some(si) = scaledInner {
                        scaledMod.value = value_with(&scaledMod.value, &Value::from("mod"), si);
                    }
                }
            }
            let scaledMod: ModRef = Arc::new(scaledMod);
            // L85
            if replace {
                // L86
                self.ReplaceModInternal(scaledMod);
            } else {
                // L88
                self.AddMod(scaledMod);
            }
        }
    }
}

/// `effects.unscalable` for one tag of `ipairs(mod)`. A non-table tag entry (TagType::LuaNonTable)
/// is indexed as Lua would: a string yields nil, anything else raises.
fn tag_unscalable(t: &Tag) -> bool {
    if t.r#type == TagType::LuaNonTable {
        let v = t.extra_get(n!("$value")).cloned().unwrap_or(Value::Nil);
        return match v {
            Value::Str(_) => false,
            Value::Mod(m) => mod_mark(&m, n!("unscalable")),
            Value::Table(tt) => tt.get(n!("unscalable")).truthy(),
            Value::JewelFunc(_) => false,
            other => lua_error!(
                "attempt to index local 'effects' (a {} value)",
                value_type(&other)
            ),
        };
    }
    t.unscalable || t.extra_get(n!("unscalable")).is_some_and(Value::truthy)
}

/// ModStore.lua 77-83 on a number `subMod.value`:
/// `precision = (hpm[name] and hpm[name][type]) or ((m_floor(v) ~= v) and data.defaultHighPrecision) or nil`.
fn scale_number(name: Name, ty: ModType, v: f64, scale: f64) -> f64 {
    // L77
    let precision = highPrecision(name, ty)
        .or_else(|| (m_floor(v) != v).then(|| f64::from(data().defaultHighPrecision)));
    scale_with_precision(v, scale, precision)
}

/// [`scale_number`] for a mod-shaped plain table (`name`/`type` read dynamically).
fn scale_precision_dyn(name: &Value, ty: &Value, v: f64) -> Option<f64> {
    let hp = match (name, ty) {
        (Value::Str(n), Value::Str(t)) => {
            ModType::parse(t).and_then(|t| highPrecision(Name::from(n), t))
        }
        _ => None,
    };
    hp.or_else(|| (m_floor(v) != v).then(|| f64::from(data().defaultHighPrecision)))
}

/// ModStore.lua 78-83.
fn scale_with_precision(v: f64, scale: f64, precision: Option<f64>) -> f64 {
    // L78
    if let Some(precision) = precision {
        // L79
        let power = lpow(10.0, precision);
        // L80
        m_floor(v * scale * power) / power
    } else {
        // L82
        m_modf(round(v * scale, Some(2))).0
    }
}

/// `v[k]` on a mod value that is a Lua table (a mod, a plain table or a jewel func table).
fn value_index(v: &Value, k: Name) -> Value {
    match v {
        Value::Table(t) => t.get(k).clone(),
        Value::Mod(m) => mod_field(m, k),
        _ => Value::Nil,
    }
}

/// `v[key]` with a dynamic key (string → field, number → array index).
fn value_index_by(v: &Value, key: &Value) -> Value {
    match (v, key) {
        (_, Value::Str(s)) => value_index(v, Name::from(s)),
        (Value::Table(t), Value::Num(i)) if *i >= 1.0 && i.fract() == 0.0 => {
            t.index(*i as usize).clone()
        }
        _ => Value::Nil,
    }
}

/// A copy of table value `v` with `v[key] = x` (the fresh `copyTable` the caller owns).
fn value_with(v: &Value, key: &Value, x: Value) -> Value {
    match (v, key) {
        (Value::Table(t), Value::Str(s)) => Value::Table(Arc::new(t.with(Name::from(s), x))),
        (Value::Table(t), Value::Num(i)) if *i >= 1.0 && i.fract() == 0.0 => {
            let mut t2 = (**t).clone();
            let i = *i as usize;
            if i <= t2.array.len() {
                t2.array[i - 1] = x;
            } else {
                t2.fields.push((
                    Name::from(format!("#{}", tostring_num(i as f64)).as_str()),
                    x,
                ));
            }
            Value::Table(Arc::new(t2))
        }
        (Value::Mod(m), Value::Str(s)) => {
            let mut m2: Mod = (**m).clone();
            let k = Name::from(s);
            if k == n!("value") {
                m2.value = x;
            } else {
                let mut ex = m2.extra.take().map(|b| *b).unwrap_or_default();
                ex.set(k, x);
                m2.extra = Some(Box::new(ex));
            }
            Value::Mod(Arc::new(m2))
        }
        _ => lua_error!("attempt to index a {} value", value_type(v)),
    }
}

/// `mod[k]` for the standard and extra keys of a mod table.
fn mod_field(m: &Mod, k: Name) -> Value {
    match k.as_str() {
        "name" => Value::Str(AStr::from(m.name.as_str())),
        "type" => match m.r#type {
            ModType::Nil => Value::Nil,
            t => Value::Str(AStr::from(t.as_str())),
        },
        "value" => m.value.clone(),
        "flags" => m
            .extra_get(n!("flags"))
            .cloned()
            .unwrap_or(Value::Num(f64::from(m.flags))),
        "keywordFlags" => Value::Num(f64::from(m.keywordFlags)),
        "source" => m.source.clone().map_or(Value::Nil, Value::Str),
        _ => m.extra_get(k).cloned().unwrap_or(Value::Nil),
    }
}

/// @pob Classes/ModStore.lua:93-97 h=d852294b
impl ModStore {
    pub fn CopyList(&self, modList: &[ModRef]) {
        // L94
        for m in modList {
            // L95
            self.AddMod(Arc::new((**m).clone()));
        }
    }
}

/// @pob Classes/ModStore.lua:99-107 h=ffb52d14
impl ModStore {
    pub fn ScaleAddList(&self, modList: &[ModRef], scale: f64, replace: bool) {
        // L100
        if scale == 1.0 {
            // L101
            self.AddList(modList);
        } else {
            // L103
            for m in modList {
                // L104
                self.ScaleAddMod(m, scale, replace);
            }
        }
    }
}

/// The `source` argument of [`ModStore::NewMod`] / [`ModStore::ReplaceMod`]: a string (any
/// `&T: AsRef<str>`) or a possibly-nil `Option<&str>` (a mod's own `source` field, or no source
/// argument at all; ModTools.lua:36-51). CCR-0063.
pub trait ModSourceArg<'s> {
    fn mod_source(self) -> Option<&'s str>;
}

impl<'s, T: AsRef<str> + ?Sized> ModSourceArg<'s> for &'s T {
    fn mod_source(self) -> Option<&'s str> {
        Some(self.as_ref())
    }
}

impl<'s> ModSourceArg<'s> for Option<&'s str> {
    fn mod_source(self) -> Option<&'s str> {
        self
    }
}

/// @pob Classes/ModStore.lua:109-111 h=04a2b779
impl ModStore {
    /// `source` may be nil (`None`): `createMod` then leaves `mod.source` nil
    /// (ModTools.lua:36-51). Any string-like argument (`&str`, `&String`, `&AStr`, …) is
    /// `Some`. CCR-0063.
    pub fn NewMod<'s>(
        &self,
        name: Name,
        ty: ModType,
        value: impl Into<Value>,
        source: impl ModSourceArg<'s>,
        flags: u32,
        keywordFlags: u32,
        tags: &[Tag],
    ) {
        // L110
        self.AddMod(Arc::new(mod_tools::createMod(
            name,
            ty,
            value,
            source.mod_source(),
            flags,
            keywordFlags,
            tags,
        )));
    }
}

/// @pob Classes/ModStore.lua:125-130 h=2fc96789
impl ModStore {
    /// Replaces an existing matching mod with a new mod. A mod is considered the same if the
    /// name, type, flags, keywordFlags, and source exactly match. If no matching mod exists,
    /// the mod is added instead.
    ///
    /// `source` may be nil (`None`), as for [`ModStore::NewMod`]. CCR-0063.
    pub fn ReplaceMod<'s>(
        &self,
        name: Name,
        ty: ModType,
        value: impl Into<Value>,
        source: impl ModSourceArg<'s>,
        flags: u32,
        keywordFlags: u32,
        tags: &[Tag],
    ) {
        // L126
        let m: ModRef = Arc::new(mod_tools::createMod(
            name,
            ty,
            value,
            source.mod_source(),
            flags,
            keywordFlags,
            tags,
        ));
        // L127
        if !self.ReplaceModInternal(m.clone()) {
            // L128
            self.AddMod(m);
        }
    }
}

/// @pob Classes/ModStore.lua:138-143 h=60847b01
impl ModStore {
    /// Converts an existing mod to a new name, replacing it in the store. Finds a mod matching
    /// oldName with the same type, flags, keywordFlags, and source as the new mod. If no
    /// matching mod exists, the new mod is added instead.
    pub fn ConvertMod(
        &self,
        oldName: Name,
        name: Name,
        ty: ModType,
        value: impl Into<Value>,
        source: &str,
        flags: u32,
        keywordFlags: u32,
        tags: &[Tag],
    ) {
        // L139
        let m: ModRef = Arc::new(mod_tools::createMod(
            name,
            ty,
            value,
            Some(source),
            flags,
            keywordFlags,
            tags,
        ));
        // L140
        if !self.ConvertModInternal(oldName, m.clone()) {
            // L141
            self.AddMod(m);
        }
    }
}

/// @pob Classes/ModStore.lua:145-159 h=d72a3c2d
impl ModStore {
    /// Lua returns what the delegate returns: `Flag` gives `true` or nil, `Max` a number or
    /// nil, `List` a fresh table. `"MIN"` falls through to `Sum` (no MIN branch in Lua).
    pub fn Combine(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> Val {
        // L146
        if modType == ModType::MORE {
            // L147
            Val::Num(self.More(cfg, names))
        } else if modType == ModType::FLAG {
            // L149 (FlagInternal returns true or nil)
            if self.Flag(cfg, names) {
                Val::Bool(true)
            } else {
                Val::Nil
            }
        } else if modType == ModType::OVERRIDE {
            // L151
            Val::from_value(&self.Override(cfg, names))
        } else if modType == ModType::LIST {
            // L153
            let t = Table::new();
            for v in self.List(cfg, names) {
                t.push(Val::from_value(&v));
            }
            Val::Table(t)
        } else if modType == ModType::MAX {
            // L155
            self.Max(cfg, names).map_or(Val::Nil, Val::Num)
        } else {
            // L157
            Val::Num(self.Sum(modType, cfg, names))
        }
    }
}

/// @pob Classes/ModStore.lua:165-174 h=3b896425
impl ModStore {
    pub fn Sum(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> f64 {
        // L166-172
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L173
        self.SumInternal(
            self,
            modType,
            cfg,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        )
    }
}

/// @pob Classes/ModStore.lua:179-188 h=ea04c772
impl ModStore {
    pub fn More(&self, cfg: Option<&ModCfg>, names: &[Name]) -> f64 {
        // L180-186
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L187
        self.MoreInternal(
            self,
            cfg,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        )
    }
}

/// @pob Classes/ModStore.lua:190-199 h=51887969
impl ModStore {
    /// Lua returns `true` or nil; nil is `false`.
    pub fn Flag(&self, cfg: Option<&ModCfg>, names: &[Name]) -> bool {
        // L191-197
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L198
        self.FlagInternal(
            self,
            cfg,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        )
    }
}

/// @pob Classes/ModStore.lua:204-213 h=b70d870c
impl ModStore {
    /// `Value::Nil` = nil.
    pub fn Override(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Value {
        // L205-211
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L212
        self.OverrideInternal(
            self,
            cfg,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        )
    }
}

/// @pob Classes/ModStore.lua:218-229 h=19bfdfa0
impl ModStore {
    pub fn List(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Vec<Value> {
        // L219-225
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L226
        let mut result = Vec::new();
        // L227
        self.ListInternal(
            self,
            &mut result,
            cfg,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        );
        // L228
        result
    }
}

/// @pob Classes/ModStore.lua:235-246 h=91103769
impl ModStore {
    pub fn Tabulate(
        &self,
        modType: Option<ModType>,
        cfg: Option<&ModCfg>,
        names: &[Name],
    ) -> Vec<TabRow> {
        // L236-242
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L243
        let mut result = Vec::new();
        // L244
        self.TabulateInternal(
            self,
            &mut result,
            modType,
            cfg,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        );
        // L245
        result
    }
}

/// @pob Classes/ModStore.lua:248-257 h=49b95c53
impl ModStore {
    /// nil unless some value > 0 (ModStore.lua:248-257, §3.5).
    pub fn Max(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Option<f64> {
        // L249
        let mut max: Option<f64> = None;
        // L250
        for value in self.Tabulate(Some(ModType::MAX), cfg, names) {
            // L251
            let val = self.EvalMod(&value.r#mod, cfg, None);
            // L252
            let val = cmp_operand(&val);
            if val > max.unwrap_or(0.0) {
                // L253
                max = Some(val);
            }
        }
        // L256
        max
    }
}

/// @pob Classes/ModStore.lua:259-268 h=62eb650f
impl ModStore {
    pub fn Min(&self, cfg: Option<&ModCfg>, names: &[Name]) -> Option<f64> {
        // L260
        let mut min: Option<f64> = None;
        // L261
        for value in self.Tabulate(Some(ModType::MIN), cfg, names) {
            // L262
            let val = self.EvalMod(&value.r#mod, cfg, None);
            // L263
            match min {
                // L264 (`min = val`: a nil val leaves min nil)
                None => {
                    min = match val {
                        Value::Nil => None,
                        Value::Num(v) => Some(v),
                        // LUA-QUIRK: Lua would keep a non-number as `min` (and raise on the next
                        // comparison); the f64 signature cannot hold it.
                        other => {
                            lua_error!("attempt to compare {} with number", value_type(&other))
                        }
                    };
                }
                Some(cur) => {
                    let v = cmp_operand(&val);
                    if v < cur {
                        // L264
                        min = Some(v);
                    }
                }
            }
        }
        // L267
        min
    }
}

/// @pob Classes/ModStore.lua:278-287 h=c61b0ba6
impl ModStore {
    /// Checks if a mod exists with the given properties. Useful for determining if the other
    /// aggregate functions will find anything to aggregate.
    /// ModDB only: on a ModList Lua errors (no HasModInternal), so this raises LuaError.
    pub fn HasMod(&self, modType: ModType, cfg: Option<&ModCfg>, names: &[Name]) -> bool {
        // L279-285
        let (flags, keywordFlags, source) = cfg_filter(cfg);
        // L286
        self.HasModInternal(
            modType,
            flags,
            keywordFlags,
            source.as_ref().map(Name::as_str),
            names,
        )
    }
}

/// @pob Classes/ModStore.lua:293-295 h=1f193e4f
impl ModStore {
    /// Lua returns the first truthy operand; any truthy value is `true` here.
    pub fn GetCondition(&self, var: Name, cfg: Option<&ModCfg>, noMod: bool) -> bool {
        // L294
        self.conditions.get(var).truthy()
            || self
                .parent
                .get()
                .is_some_and(|p| p.GetCondition(var, cfg, true))
            || (!noMod && self.Flag(cfg, &[conditionName(var)]))
    }
}

/// @pob Classes/ModStore.lua:301-303 h=bd51854d
impl ModStore {
    pub fn GetMultiplier(&self, var: Name, cfg: Option<&ModCfg>, noMod: bool) -> f64 {
        // L302
        if !noMod {
            let o = self.Override(cfg, &[multiplierName(var)]);
            if o.truthy() {
                return arith(&o, "an OVERRIDE multiplier");
            }
        }
        let a = self.multipliers.num_or(var, 0.0);
        let b = match self.parent.get() {
            Some(p) => p.GetMultiplier(var, cfg, true),
            None => 0.0,
        };
        let c = if noMod {
            0.0
        } else {
            self.Sum(ModType::BASE, cfg, &[multiplierName(var)])
        };
        a + b + c
    }
}

/// @pob Classes/ModStore.lua:308-357 h=62a1c1f8
impl ModStore {
    pub fn GetStat(&self, stat: Name, cfg: Option<&ModCfg>) -> f64 {
        // Checks if any buff in buffList matches
        // Was needed for skills that provide multiple buffs (e.g. Herald of Agony) and can't be accesses with `buffList[1]`
        // L311
        let isNameInBuffList = |buffList: &Shared<Vec<Table>>, name: Option<Name>| -> bool {
            // L312
            for buff in buffList.snapshot() {
                // L313
                let bn = buff.get(n!("name"));
                let eq = match (&bn, name) {
                    (Val::Nil, None) => true,
                    (Val::Str(s), Some(n)) => &**s == n.as_str(),
                    _ => false,
                };
                if eq {
                    return true;
                }
            }
            // L315
            false
        };
        // `self.actor` is `{ }` when the store was made without a parent (a dangling Weak).
        let actor = self.actor.get().upgrade();
        let actor_output = |what: &str| -> Table {
            match &actor {
                Some(a) => a.output.get(),
                None => lua_error!("attempt to index field 'output' (a nil value){what}"),
            }
        };
        // L317
        if stat == n!("ManaReservedPercent") {
            // L318
            let mut reservedPercentMana = 0.0;
            // Check if mana is 0 (i.e. from Blood Magic) to avoid division by 0.
            // L320
            let totalMana = actor_output("").get(n!("Mana"));
            // L321
            if matches!(totalMana, Val::Num(n) if n == 0.0) {
                return 0.0;
            } else {
                // L322
                let list = actor.as_ref().map(|a| a.activeSkillList.get());
                for activeSkill in list.map(|l| l.snapshot()).unwrap_or_default() {
                    // L323
                    if reservation_skill_matches(&activeSkill, cfg, &isNameInBuffList) {
                        // L324
                        let manaBase = activeSkill
                            .skillData
                            .get()
                            .num_or(n!("ManaReservedBase"), 0.0);
                        // L325
                        reservedPercentMana = m_floor(manaBase / totalMana.num() * 100.0);
                        // L326
                        break;
                    }
                }
                // L329
                return m_min(reservedPercentMana, 100.0); //Don't let people get more than 100% reservation for aura effect.
            }
        }
        // L332
        if stat == n!("LifeReservedPercent") {
            // L333
            let mut reservedPercentLife = 0.0;
            // L334
            let totalLife = actor_output("").get(n!("Life"));
            // L335
            if matches!(totalLife, Val::Num(n) if n == 0.0) {
                return 0.0;
            } else {
                // L336
                let list = actor.as_ref().map(|a| a.activeSkillList.get());
                for activeSkill in list.map(|l| l.snapshot()).unwrap_or_default() {
                    // L337
                    if reservation_skill_matches(&activeSkill, cfg, &isNameInBuffList) {
                        // L338
                        let lifeBase = activeSkill
                            .skillData
                            .get()
                            .num_or(n!("LifeReservedBase"), 0.0);
                        // L339
                        reservedPercentLife = m_floor(lifeBase / totalLife.num() * 100.0);
                        // L340
                        break;
                    }
                }
                // L343
                return m_min(reservedPercentLife, 100.0); //Don't let people get more than 100% reservation for aura effect.
            }
        }
        // if ReservationEfficiency is -100, ManaUnreserved is nan which breaks everything if Arcane Cloak is enabled
        // L347
        if stat == n!("ManaUnreserved") && {
            let v = actor_output("").get(stat);
            !v.raw_eq(&v)
        } {
            // 0% reserved = total mana
            // L349
            stat_number(&actor_output("").get(n!("Mana")))
        } else if stat == n!("ManaUnreserved") && {
            let v = actor_output("").get(stat);
            !v.is_nil() && lua_lt_num(&v, 0.0)
        } {
            // This reverse engineers how much mana is unreserved before efficiency for accurate Arcane Cloak calcs
            // L352
            let output = actor_output("");
            let efficiency = actor
                .as_ref()
                .map_or(Val::Nil, |a| a.extra.get(n!("ManaEfficiency")));
            let reservedPercentBeforeEfficiency =
                (m_abs(output.get(n!("ManaUnreservedPercent")).num()) + 100.0)
                    * ((100.0 + efficiency.num()) / 100.0);
            // L353
            output.get(n!("Mana")).num() * (m_ceil(reservedPercentBeforeEfficiency) / 100.0)
        } else {
            // L355
            if let Some(a) = &actor {
                let v = a.output.get().get(stat);
                if v.truthy() {
                    return stat_number(&v);
                }
            }
            if let Some(cfg) = cfg {
                if let Some(skillStats) = cfg.skillStats.get() {
                    let v = skillStats.get(stat);
                    if v.truthy() {
                        return stat_number(&v);
                    }
                }
            }
            0.0
        }
    }
}

/// ModStore.lua:323/337: `activeSkill.skillTypes[SkillType.HasReservation] and not
/// activeSkill.skillFlags.disable and activeSkill.buffList and activeSkill.buffList[1] and cfg
/// and (isNameInBuffList(buffList, cfg.skillName) or isNameInBuffList(buffList, cfg.summonSkillName))`.
fn reservation_skill_matches(
    activeSkill: &ActiveSkillRef,
    cfg: Option<&ModCfg>,
    isNameInBuffList: &dyn Fn(&Shared<Vec<Table>>, Option<Name>) -> bool,
) -> bool {
    if !activeSkill
        .skillTypes
        .get()
        .with(|s| s.has(SkillType::HasReservation))
    {
        return false;
    }
    if activeSkill.skillFlags.get().truthy(n!("disable")) {
        return false;
    }
    let buffList = activeSkill.buffList.get();
    if buffList.is_empty() {
        return false;
    }
    let Some(cfg) = cfg else {
        return false;
    };
    isNameInBuffList(&buffList, cfg.skillName.get())
        || isNameInBuffList(&buffList, cfg.summonSkillName.get())
}

/// A stat value returned from GetStat: numbers (and numeric strings); the f64 signature cannot
/// carry anything else, so other values raise (LUA-QUIRK: Lua would return them).
fn stat_number(v: &Val) -> f64 {
    match v {
        Val::Num(n) => *n,
        other => other.num(),
    }
}

/// `v < x` for a Lua value and a number (raises unless `v` is a number).
fn lua_lt_num(v: &Val, x: f64) -> bool {
    match v {
        Val::Num(n) => *n < x,
        other => lua_error!("attempt to compare {} with number", other.lua_type()),
    }
}

// ---- Rust-only accessors (oracle, injection, UI) ----

impl ModStore {
    /// The mods stored under `name` (ModDB) / with that name (ModList), in store order.
    pub fn mods_named(&self, name: Name) -> Vec<ModRef> {
        match &*self.mods.borrow() {
            StoreMods::DB(map) => map.get(&name).cloned().unwrap_or_default(),
            StoreMods::List { list, index } => index
                .get(&name)
                .map(|p| p.iter().map(|&i| list[i as usize].clone()).collect())
                .unwrap_or_default(),
        }
    }

    /// Every mod, in store order. ModDB: the frozen storage is an unordered hash map, so the
    /// names come in sorted order (CCR-0014 proposes an insertion-ordered map); mods within a
    /// name keep their order.
    pub fn all_mods(&self) -> Vec<ModRef> {
        match &*self.mods.borrow() {
            StoreMods::DB(map) => {
                // CCR-0014: name order is not insertion order with the frozen AHashMap.
                let mut names: Vec<&Name> = map.keys().collect();
                names.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                names
                    .into_iter()
                    .flat_map(|n| map[n].iter().cloned())
                    .collect()
            }
            StoreMods::List { list, .. } => list.clone(),
        }
    }

    /// The names that have a mod bucket (ModDB, including empty buckets) or a mod (ModList).
    pub fn mod_names(&self) -> Vec<Name> {
        match &*self.mods.borrow() {
            StoreMods::DB(map) => {
                let mut v: Vec<Name> = map.keys().copied().collect();
                v.sort_by(|a, b| a.as_str().cmp(b.as_str()));
                v
            }
            StoreMods::List { list, .. } => {
                let mut seen: SmallVec<[Name; 16]> = SmallVec::new();
                for m in list {
                    if !seen.contains(&m.name) {
                        seen.push(m.name);
                    }
                }
                seen.into_vec()
            }
        }
    }
}

// ---- unit tests (MS-A; the oracle comparison is crates/pob-oracle/tests/MS-A.rs) ----
//
// These cover the paths that need neither `data` (More/ScaleAddMod read
// `data.highPrecisionMods`) nor EvalMod (MS-B) nor createMod (CT).
#[cfg(test)]
mod tests {
    use super::*;

    fn md(
        name: &str,
        ty: ModType,
        value: impl Into<Value>,
        flags: u32,
        kw: u32,
        source: Option<&str>,
    ) -> Mod {
        Mod {
            name: Name::from(name),
            r#type: ty,
            value: value.into(),
            flags,
            keywordFlags: kw,
            source: source.map(AStr::from),
            tags: smallvec::SmallVec::new(),
            extra: None,
        }
    }

    fn arc(m: Mod) -> ModRef {
        Arc::new(m)
    }

    fn err<R>(f: impl FnOnce() -> R) -> String {
        match pob_lua::error::catch(f) {
            Ok(_) => panic!("expected a LuaError"),
            Err(e) => e.0,
        }
    }

    #[test]
    fn sum_flag_override_list_over_parent_chain() {
        for kind in [StoreKind::ModDB, StoreKind::ModList] {
            let parent = ModStore::ModDB(None);
            let store = match kind {
                StoreKind::ModDB => ModStore::ModDB(Some(parent.clone())),
                StoreKind::ModList => ModStore::ModList(Some(parent.clone())),
            };
            store.AddMod(arc(md(
                "Life",
                ModType::BASE,
                10.0,
                0,
                0,
                Some("Item:1:Ring"),
            )));
            store.AddMod(arc(md("Life", ModType::INC, 5.0, 0, 0, Some("Tree:7"))));
            store.AddMod(arc(md(
                "Life",
                ModType::BASE,
                3.0,
                ModFlag::Attack,
                0,
                Some("Tree:7"),
            )));
            parent.AddMod(arc(md("Life", ModType::BASE, 100.0, 0, 0, Some("Base"))));
            parent.AddMod(arc(md("Mana", ModType::BASE, 7.0, 0, 0, Some("Base"))));
            let none = None;
            assert_eq!(store.Sum(ModType::BASE, none, nm!["Life"]), 110.0);
            assert_eq!(store.Sum(ModType::BASE, none, nm!["Life", "Mana"]), 117.0);
            let atk = ModCfg::with_flags(ModFlag::Attack, 0);
            assert_eq!(store.Sum(ModType::BASE, Some(&atk), nm!["Life"]), 113.0);
            let src = ModCfg::new();
            src.source.set(Some(n!("Tree")));
            assert_eq!(store.Sum(ModType::BASE, Some(&src), nm!["Life"]), 0.0);
            assert_eq!(store.Sum(ModType::INC, Some(&src), nm!["Life"]), 5.0);
            // "MIN" has no Combine branch: it is a Sum of MIN mods
            assert_eq!(store.Combine(ModType::MIN, none, nm!["Life"]).num(), 0.0);
            // Flag: true or nil (false), parent consulted last
            parent.AddMod(arc(md(
                "Onslaught",
                ModType::FLAG,
                true,
                0,
                0,
                Some("Base"),
            )));
            store.AddMod(arc(md(
                "Onslaught",
                ModType::FLAG,
                false,
                0,
                0,
                Some("Base"),
            )));
            assert!(store.Flag(none, nm!["Onslaught"]));
            assert!(!store.Flag(none, nm!["Nope"]));
            assert!(matches!(
                store.Combine(ModType::FLAG, none, nm!["Nope"]),
                Val::Nil
            ));
            // Override: first truthy in name order, then parent
            parent.AddMod(arc(md("X", ModType::OVERRIDE, 1.0, 0, 0, None)));
            assert_eq!(store.Override(none, nm!["X"]), Value::Num(1.0));
            store.AddMod(arc(md("Y", ModType::OVERRIDE, 0.0, 0, 0, None)));
            assert_eq!(store.Override(none, nm!["Y", "X"]), Value::Num(0.0));
            // List: store order then parent; false values are dropped
            store.AddMod(arc(md("L", ModType::LIST, "a", 0, 0, None)));
            store.AddMod(arc(md("L", ModType::LIST, false, 0, 0, None)));
            parent.AddMod(arc(md("L", ModType::LIST, "b", 0, 0, None)));
            store.AddMod(arc(md("L", ModType::LIST, "c", 0, 0, None)));
            let l: Vec<String> = store
                .List(none, nm!["L"])
                .iter()
                .map(|v| v.as_str().unwrap().to_owned())
                .collect();
            assert_eq!(l, ["a", "c", "b"]);
            // Tabulate: zero values dropped except OVERRIDE; nil modType matches every type
            store.AddMod(arc(md("Z", ModType::BASE, 0.0, 0, 0, None)));
            store.AddMod(arc(md("Z", ModType::OVERRIDE, 0.0, 0, 0, None)));
            store.AddMod(arc(md("Z", ModType::INC, 2.0, 0, 0, None)));
            let rows = store.Tabulate(None, none, nm!["Z"]);
            let types: Vec<ModType> = rows.iter().map(|r| r.r#mod.r#type).collect();
            assert_eq!(types, [ModType::OVERRIDE, ModType::INC]);
            assert_eq!(store.Tabulate(Some(ModType::BASE), none, nm!["Z"]).len(), 0);
        }
    }

    #[test]
    fn source_filter_raises_on_nil_source_except_db_sum() {
        let cfg = ModCfg::new();
        cfg.source.set(Some(n!("Item")));
        let db = ModStore::ModDB(None);
        db.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, None)));
        db.AddMod(arc(md("A", ModType::FLAG, true, 0, 0, None)));
        // ModDB.SumInternal guards `mod.source and …`
        assert_eq!(db.Sum(ModType::BASE, Some(&cfg), nm!["A"]), 0.0);
        assert!(err(|| db.Flag(Some(&cfg), nm!["A"])).contains("source"));
        // ModList.SumInternal does not
        let ml = ModStore::ModList(None);
        ml.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, None)));
        assert!(err(|| ml.Sum(ModType::BASE, Some(&cfg), nm!["A"])).contains("source"));
        // the type test comes first: no error when the type differs
        assert_eq!(ml.Sum(ModType::INC, Some(&cfg), nm!["A"]), 0.0);
    }

    #[test]
    fn table_flags_raise_like_band() {
        let mut m = md("A", ModType::BASE, 1.0, 0, 0, Some("S"));
        let mut ex = ValueTable::new();
        ex.set(n!("flags"), Value::Table(Arc::new(ValueTable::new())));
        m.extra = Some(Box::new(ex));
        let ml = ModStore::ModList(None);
        ml.AddMod(arc(m));
        assert!(err(|| ml.Sum(ModType::BASE, None, nm!["A"])).contains("band"));
        assert_eq!(ml.Sum(ModType::INC, None, nm!["A"]), 0.0);
    }

    #[test]
    fn has_mod_on_mod_list_raises() {
        let ml = ModStore::ModList(None);
        ml.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, None)));
        assert!(err(|| ml.HasMod(ModType::BASE, None, nm!["A"])).contains("HasModInternal"));
        let parent = ModStore::ModDB(None);
        let db = ModStore::ModDB(Some(parent.clone()));
        parent.AddMod(arc(md("A", ModType::BASE, 0.0, 0, 0, None)));
        assert!(db.HasMod(ModType::BASE, None, nm!["A"]));
        assert!(!db.HasMod(ModType::INC, None, nm!["A"]));
        // a ModDB whose parent is a ModList reaches ModList's missing method
        let db2 = ModStore::ModDB(Some(ml));
        assert!(err(|| db2.HasMod(ModType::BASE, None, nm!["B"])).contains("HasModInternal"));
    }

    #[test]
    fn replace_recurses_into_parent_and_marks_db_mods() {
        // ModDB: the replacement gets `replaced = true`, so a second identical replace finds no
        // unreplaced match (ReplaceMod would then add a duplicate).
        let parent = ModStore::ModDB(None);
        let db = ModStore::ModDB(Some(parent.clone()));
        parent.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, Some("S"))));
        assert!(db.ReplaceModInternal(arc(md("A", ModType::BASE, 2.0, 0, 0, Some("S")))));
        assert_eq!(parent.Sum(ModType::BASE, None, nm!["A"]), 2.0);
        assert!(mod_mark(&parent.mods_named(n!("A"))[0], n!("replaced")));
        // L44-46: the child got an empty bucket
        assert_eq!(db.db_len(n!("A")), Some(0));
        assert!(!db.ReplaceModInternal(arc(md("A", ModType::BASE, 3.0, 0, 0, Some("S")))));
        assert_eq!(parent.Sum(ModType::BASE, None, nm!["A"]), 2.0);
        // different source: no match
        assert!(!db.ReplaceModInternal(arc(md("A", ModType::BASE, 3.0, 0, 0, Some("T")))));
        // ModList: no mark; a ModList child of a ModDB parent recurses into it
        let p2 = ModStore::ModDB(None);
        p2.AddMod(arc(md("B", ModType::INC, 1.0, 0, 0, Some("S"))));
        let ml = ModStore::ModList(Some(p2.clone()));
        ml.AddMod(arc(md("C", ModType::INC, 1.0, 0, 0, Some("S"))));
        assert!(ml.ReplaceModInternal(arc(md("B", ModType::INC, 4.0, 0, 0, Some("S")))));
        assert_eq!(p2.Sum(ModType::INC, None, nm!["B"]), 4.0);
        assert!(ml.ReplaceModInternal(arc(md("C", ModType::INC, 5.0, 0, 0, Some("S")))));
        assert!(ml.ReplaceModInternal(arc(md("C", ModType::INC, 6.0, 0, 0, Some("S")))));
        assert_eq!(ml.Sum(ModType::INC, None, nm!["C"]), 6.0);
        assert_eq!(ml.len(), 1);
        assert!(!mod_mark(&ml.ipairs()[0], n!("replaced")));
    }

    #[test]
    fn convert_moves_buckets_and_recurses_into_parent() {
        let parent = ModStore::ModDB(None);
        let db = ModStore::ModDB(Some(parent.clone()));
        parent.AddMod(arc(md("Old", ModType::BASE, 1.0, 0, 0, Some("S"))));
        parent.AddMod(arc(md("Old", ModType::BASE, 2.0, 0, 0, Some("S"))));
        // no bucket in the child: straight to the parent
        assert!(db.ConvertModInternal(
            n!("Old"),
            arc(md("New", ModType::BASE, 5.0, 0, 0, Some("S")))
        ));
        assert_eq!(parent.Sum(ModType::BASE, None, nm!["Old"]), 2.0);
        assert_eq!(parent.Sum(ModType::BASE, None, nm!["New"]), 5.0);
        assert!(mod_mark(&parent.mods_named(n!("New"))[0], n!("converted")));
        // child has the bucket but no match (type differs): parent
        db.AddMod(arc(md("Old", ModType::INC, 1.0, 0, 0, Some("S"))));
        assert!(db.ConvertModInternal(
            n!("Old"),
            arc(md("New", ModType::BASE, 6.0, 0, 0, Some("S")))
        ));
        assert_eq!(parent.Sum(ModType::BASE, None, nm!["Old"]), 0.0);
        assert_eq!(db.Sum(ModType::BASE, None, nm!["New"]), 11.0);
        assert!(!db.ConvertModInternal(
            n!("Old"),
            arc(md("New", ModType::BASE, 6.0, 0, 0, Some("S")))
        ));
        // a converted mod is not converted again (same name in and out)
        let db3 = ModStore::ModDB(None);
        db3.AddMod(arc(md("Q", ModType::BASE, 1.0, 0, 0, None)));
        assert!(db3.ConvertModInternal(n!("Q"), arc(md("Q", ModType::BASE, 2.0, 0, 0, None))));
        assert!(!db3.ConvertModInternal(n!("Q"), arc(md("Q", ModType::BASE, 3.0, 0, 0, None))));
        assert_eq!(db3.Sum(ModType::BASE, None, nm!["Q"]), 2.0);
        // ModList: in place, the name index follows
        let ml = ModStore::ModList(None);
        ml.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, None)));
        ml.AddMod(arc(md("Old", ModType::BASE, 2.0, 0, 0, None)));
        ml.AddMod(arc(md("A", ModType::BASE, 4.0, 0, 0, None)));
        assert!(ml.ConvertModInternal(n!("Old"), arc(md("A", ModType::BASE, 8.0, 0, 0, None))));
        let names: Vec<&str> = ml.ipairs().iter().map(|m| m.name.as_str()).collect();
        assert_eq!(names, ["A", "A", "A"]);
        let order: Vec<f64> = ml
            .mods_named(n!("A"))
            .iter()
            .map(|m| m.value.num().unwrap())
            .collect();
        assert_eq!(order, [1.0, 8.0, 4.0]);
        assert_eq!(ml.Sum(ModType::BASE, None, nm!["Old"]), 0.0);
        assert_eq!(ml.Sum(ModType::BASE, None, nm!["A"]), 13.0);
        // ModList child, ModDB parent
        let p = ModStore::ModDB(None);
        p.AddMod(arc(md("Old", ModType::BASE, 2.0, 0, 0, None)));
        let ml2 = ModStore::ModList(Some(p.clone()));
        assert!(ml2.ConvertModInternal(n!("Old"), arc(md("New", ModType::BASE, 3.0, 0, 0, None))));
        assert_eq!(p.Sum(ModType::BASE, None, nm!["New"]), 3.0);
    }

    #[test]
    fn conditions_and_multipliers_walk_the_parent() {
        let parent = ModStore::ModDB(None);
        let db = ModStore::ModDB(Some(parent.clone()));
        parent.conditions.set(n!("Moving"), true);
        assert!(db.GetCondition(n!("Moving"), None, false));
        db.AddMod(arc(md(
            "Condition:Fortified",
            ModType::FLAG,
            true,
            0,
            0,
            None,
        )));
        assert!(db.GetCondition(n!("Fortified"), None, false));
        assert!(!db.GetCondition(n!("Fortified"), None, true));
        // the parent is asked with noMod = true, but the child's own Flag walks the chain
        parent.AddMod(arc(md(
            "Condition:Leeching",
            ModType::FLAG,
            true,
            0,
            0,
            None,
        )));
        assert!(db.GetCondition(n!("Leeching"), None, false));
        assert!(!db.GetCondition(n!("Leeching"), None, true));
        parent.multipliers.set(n!("Rage"), 10.0);
        db.multipliers.set(n!("Rage"), 1.0);
        db.AddMod(arc(md("Multiplier:Rage", ModType::BASE, 5.0, 0, 0, None)));
        parent.AddMod(arc(md("Multiplier:Rage", ModType::BASE, 100.0, 0, 0, None)));
        // own 1 + parent (10, noMod) + Sum over the chain (5 + 100)
        assert_eq!(db.GetMultiplier(n!("Rage"), None, false), 116.0);
        db.AddMod(arc(md(
            "Multiplier:Rage",
            ModType::OVERRIDE,
            3.0,
            0,
            0,
            None,
        )));
        assert_eq!(db.GetMultiplier(n!("Rage"), None, false), 3.0);
        assert_eq!(db.GetMultiplier(n!("Rage"), None, true), 11.0);
    }

    #[test]
    fn get_stat_reads_actor_output_then_skill_stats() {
        let db = ModStore::ModDB(None);
        // no actor: `self.actor.output` is nil, so skillStats / 0
        let cfg = ModCfg::new();
        let stats = Table::new();
        stats.set(n!("Str"), 12.0);
        cfg.skillStats.set(Some(stats));
        assert_eq!(db.GetStat(n!("Str"), Some(&cfg)), 12.0);
        assert_eq!(db.GetStat(n!("Dex"), None), 0.0);
        assert!(err(|| db.GetStat(n!("ManaReservedPercent"), None)).contains("output"));
        let actor = Actor::new();
        db.actor.set(Rc::downgrade(&actor));
        let out = actor.output.get();
        out.set(n!("Str"), 0.0);
        // 0 is truthy in Lua
        assert_eq!(db.GetStat(n!("Str"), Some(&cfg)), 0.0);
        out.set(n!("Mana"), 200.0);
        out.set(n!("ManaUnreserved"), f64::NAN);
        assert_eq!(db.GetStat(n!("ManaUnreserved"), None), 200.0);
        out.set(n!("ManaUnreserved"), -10.0);
        out.set(n!("ManaUnreservedPercent"), -5.0);
        actor.extra.set(n!("ManaEfficiency"), 20.0);
        // (5 + 100) * 1.2 = 126 → ceil 126 → 200 * 1.26
        let expect = 200.0 * (m_ceil((5.0 + 100.0) * ((100.0 + 20.0) / 100.0)) / 100.0);
        assert_eq!(db.GetStat(n!("ManaUnreserved"), None), expect);
        out.set(n!("Life"), 0.0);
        assert_eq!(db.GetStat(n!("LifeReservedPercent"), None), 0.0);
        out.set(n!("Mana"), 0.0);
        assert_eq!(db.GetStat(n!("ManaReservedPercent"), None), 0.0);
        out.set(n!("Mana"), 300.0);
        assert_eq!(db.GetStat(n!("ManaReservedPercent"), None), 0.0);
    }

    #[test]
    fn copy_scale1_merge_wipe_and_add_db() {
        let src = ModStore::ModList(None);
        src.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, None)));
        src.AddMod(arc(md("B", ModType::BASE, 2.0, 0, 0, None)));
        let dst = ModStore::ModList(None);
        dst.CopyList(&src.ipairs());
        assert!(!Arc::ptr_eq(&dst.ipairs()[0], &src.ipairs()[0]));
        dst.ScaleAddList(&src.ipairs(), 1.0, false);
        assert!(Arc::ptr_eq(&dst.ipairs()[2], &src.ipairs()[0]));
        // unscalable tags keep the mod as is (no data needed)
        let mut un = md("U", ModType::BASE, 3.0, 0, 0, None);
        let mut t = Tag::new(TagType::Condition);
        t.unscalable = true;
        un.tags.push(t);
        let un = arc(un);
        dst.ScaleAddMod(&un, 0.5, false);
        assert!(Arc::ptr_eq(dst.ipairs().last().unwrap(), &un));
        // AddDB (ModDB only), including into itself
        let db = ModStore::ModDB(None);
        db.AddList(&src.ipairs());
        let db2 = ModStore::ModDB(None);
        db2.AddDB(&db);
        db2.AddDB(&db2);
        assert_eq!(db2.Sum(ModType::BASE, None, nm!["A", "B"]), 6.0);
        assert!(err(|| src.AddDB(&db)).contains("AddDB"));
        assert!(err(|| db.AddDB(&src)).contains("table expected"));
        assert!(err(|| db.MergeMod(un.clone(), false)).contains("MergeMod"));
        // MergeMod skipNonAdditive: FLAG mods are not added
        let ml = ModStore::ModList(None);
        ml.MergeMod(arc(md("F", ModType::FLAG, true, 0, 0, None)), true);
        assert_eq!(ml.len(), 0);
        ml.MergeMod(arc(md("F", ModType::FLAG, true, 0, 0, None)), false);
        assert_eq!(ml.len(), 1);
        // wipe
        let p = ModStore::ModDB(None);
        let w = ModStore::ModList(Some(p));
        w.AddMod(arc(md("A", ModType::BASE, 1.0, 0, 0, None)));
        w.wipe();
        assert_eq!(w.len(), 0);
        assert!(w.parent.get().is_none());
        assert_eq!(w.Sum(ModType::BASE, None, nm!["A"]), 0.0);
        db.wipe();
        assert!(db.all_mods().is_empty());
        assert_eq!(db.len(), 0);
    }
}
