//! @pob-file Classes/ModList.lua @16de4b82d
//! Classes/ModList.lua 1-249 (ModList storage: a flat list). The public methods dispatch on
//! `ModStore::kind`; the ModList bodies are the `*_List` functions here.
//!
//! The list keeps a name → positions index (D14). Every query loops names outside and the
//! list inside, keeping only mods of that name (ModList.lua:103-107), so walking the
//! positions of one name in ascending order visits exactly the mods Lua's `for i = 1, #self`
//! keeps, in the same order.
//!
//! Owner: MS-A.
#![allow(non_snake_case)]
// Lua statement shapes are kept (`local value` then assigned per branch; `if … then return … else … end`).
#![allow(clippy::needless_late_init, clippy::redundant_else)]

use crate::prelude::*;
use pob_gamedata::MatchKeywordFlags;

use super::mod_db::same_params;
use super::mod_store::{arith, flags_match, highPrecision, or0, source_match};
use super::StoreMods;

// @pob Classes/ModList.lua:6-20 h=27e6b307
// (module-level statements: ipairs,pairs,select,t_insert,m_floor,m_min,m_max,m_modf,band,bor,mod_createMod,ModListClass)

// ---- storage access (Rust-only plumbing; one short borrow per call, §3.3) ----

type Index = ahash::AHashMap<Name, smallvec::SmallVec<[u32; 2]>>;

impl ModStore {
    /// The list and its index (leaf closure, §3.3).
    #[inline]
    fn list_with<R>(&self, f: impl FnOnce(&mut Vec<ModRef>, &mut Index) -> R) -> R {
        match &mut *self.mods.borrow_mut() {
            StoreMods::List { list, index } => f(list, index),
            StoreMods::DB(_) => unreachable!("ModList body on a ModDB"),
        }
    }

    /// How many mods named `name` the list holds.
    #[inline]
    pub(crate) fn list_named_len(&self, name: Name) -> usize {
        match &*self.mods.borrow() {
            StoreMods::List { index, .. } => index.get(&name).map_or(0, |p| p.len()),
            StoreMods::DB(_) => unreachable!("ModList body on a ModDB"),
        }
    }

    /// The `k`-th (0-based) mod named `name`, in list order.
    #[inline]
    pub(crate) fn list_named_at(&self, name: Name, k: usize) -> Option<ModRef> {
        match &*self.mods.borrow() {
            StoreMods::List { list, index } => index
                .get(&name)
                .and_then(|p| p.get(k))
                .map(|&i| list[i as usize].clone()),
            StoreMods::DB(_) => unreachable!("ModList body on a ModDB"),
        }
    }

    /// `self[i]` (1-based).
    #[inline]
    fn list_at(&self, i1: usize) -> Option<ModRef> {
        match &*self.mods.borrow() {
            StoreMods::List { list, .. } => list.get(i1.wrapping_sub(1)).cloned(),
            StoreMods::DB(_) => unreachable!("ModList body on a ModDB"),
        }
    }
}

/// `t_insert(self, mod)`.
fn list_push(list: &mut Vec<ModRef>, index: &mut Index, m: ModRef) {
    let pos = u32::try_from(list.len()).expect("ModList over 2^32 mods");
    index.entry(m.name).or_default().push(pos);
    list.push(m);
}

/// `self[i] = mod` (0-based `i`, an existing slot): moves the position between name entries
/// when the name changes (ConvertModInternal), keeping each entry sorted.
fn list_set(list: &mut [ModRef], index: &mut Index, i: usize, m: ModRef) {
    let old = list[i].name;
    let new = m.name;
    if old != new {
        let pos = i as u32;
        if let Some(p) = index.get_mut(&old) {
            p.retain(|x| *x != pos);
            if p.is_empty() {
                index.remove(&old);
            }
        }
        let p = index.entry(new).or_default();
        let at = p.partition_point(|x| *x < pos);
        p.insert(at, pos);
    }
    list[i] = m;
}

/// `mod.source:match("[^:]+") == source` gate shared by every query body.
#[inline]
fn matches(
    m: &Mod,
    ty: Option<ModType>,
    flags: u32,
    keywordFlags: u32,
    source: Option<&str>,
) -> bool {
    ty.map_or(true, |t| m.r#type == t)
        && flags_match(flags, m)
        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
        && source_match(source, m)
}

/// @pob Classes/ModList.lua:22-25 h=835d6b1e
impl ModStore {
    /// `ModList:ModList(parent)`: `ModStore::ModList` (U0) allocates; this runs the constructor body.
    #[allow(dead_code)]
    pub(crate) fn init_ModList(&self, parent: Option<&ModStoreRef>) {
        // L23
        self.init_ModStore(parent);
        // L24 return self
    }
}

/// @pob Classes/ModList.lua:27-29 h=67b4b724
impl ModStore {
    pub(crate) fn AddMod_List(&self, m: ModRef) {
        // L28
        self.list_with(|list, index| list_push(list, index, m));
    }
}

/// @pob Classes/ModList.lua:36-50 h=e02d7c3c
impl ModStore {
    /// Replaces an existing matching mod with a new mod.
    /// If no matching mod exists, then the function returns false
    pub(crate) fn ReplaceModInternal_List(&self, m: ModRef) -> bool {
        // L37: Find the index of the existing mod, if it is in the table
        let replaced = self.list_with(|list, index| {
            // L38
            for i in 0..list.len() {
                let curMod = &list[i];
                // L39
                if m.name == curMod.name && same_params(&m, curMod) {
                    // L40
                    list_set(list, index, i, m.clone());
                    // L41
                    return true;
                }
            }
            false
        });
        if replaced {
            return true;
        }
        // L45
        if let Some(parent) = self.parent.get() {
            // L46
            return parent.ReplaceModInternal(m);
        }
        // L49
        false
    }
}

/// @pob Classes/ModList.lua:58-71 h=f3adb218
impl ModStore {
    /// Converts an existing mod with oldName to a new mod with a different name.
    /// If no matching mod exists, then the function returns false
    pub(crate) fn ConvertModInternal_List(&self, oldName: Name, m: ModRef) -> bool {
        let converted = self.list_with(|list, index| {
            // L59
            for i in 0..list.len() {
                let curMod = &list[i];
                // L60
                if oldName == curMod.name && same_params(&m, curMod) {
                    // L61
                    list_set(list, index, i, m.clone());
                    // L62
                    return true;
                }
            }
            false
        });
        if converted {
            return true;
        }
        // L66
        if let Some(parent) = self.parent.get() {
            // L67
            return parent.ConvertModInternal(oldName, m);
        }
        // L70
        false
    }
}

/// @pob Classes/ModList.lua:73-86 h=21dc2f7c
impl ModStore {
    /// ModList only (ModList.lua:73).
    pub fn MergeMod(&self, m: ModRef, skipNonAdditive: bool) {
        if self.kind == StoreKind::ModDB {
            // LUA-QUIRK: ModDB.lua defines no MergeMod.
            lua_error!("attempt to call method 'MergeMod' (a nil value)");
        }
        // L74
        if m.r#type == ModType::BASE || m.r#type == ModType::INC || m.r#type == ModType::MORE {
            // L75
            let n = self.len();
            for i in 1..=n {
                let Some(cur) = self.list_at(i) else { break };
                // L76
                if pob_gamedata::mod_tools::compareModParams(&cur, &m) {
                    // L77-78: `self[i] = copyTable(self[i], true)`, then its value is updated
                    let mut merged: Mod = (*cur).clone();
                    merged.value = Value::Num(
                        arith(&cur.value, "field 'value'") + arith(&m.value, "field 'value'"),
                    );
                    let merged = Arc::new(merged);
                    self.list_with(|list, index| list_set(list, index, i - 1, merged));
                    // L79
                    return;
                }
            }
        }
        // L83
        if !skipNonAdditive {
            // L84
            self.AddMod(m);
        }
    }
}

/// @pob Classes/ModList.lua:88-94 h=210fa5c6
impl ModStore {
    pub(crate) fn AddList_List(&self, modList: &[ModRef]) {
        // L89 (`if modList`: a slice is always present)
        self.list_with(|list, index| {
            // L90
            for m in modList {
                // L91
                list_push(list, index, m.clone());
            }
        });
    }
}

/// @pob Classes/ModList.lua:96-98 h=a23cd7cc
impl ModStore {
    /// ModList only (ModList.lua:96).
    pub fn MergeNewMod(&self, m: Mod) {
        // L97: `mod_createMod(...)` is built by the caller (createMod's varargs).
        self.MergeMod(Arc::new(m), false);
    }
}

/// @pob Classes/ModList.lua:101-120 h=6fa85692
impl ModStore {
    pub(crate) fn SumInternal_List(
        &self,
        context: &ModStore,
        modType: ModType,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        // L102
        let mut result = 0.0;
        // L103
        for &modName in names {
            // L105
            for k in 0..self.list_named_len(modName) {
                // L106
                let Some(m) = self.list_named_at(modName, k) else {
                    break;
                };
                // L107
                if matches(&m, Some(modType), flags, keywordFlags, source) {
                    // L108
                    if !m.tags.is_empty() {
                        // L109
                        result += or0(&context.EvalMod(&m, cfg, None), "a nil value");
                    } else {
                        // L111
                        result += arith(&m.value, "field 'value'");
                    }
                }
            }
        }
        // L116
        if let Some(parent) = self.parent.get() {
            // L117
            result += parent.SumInternal(context, modType, cfg, flags, keywordFlags, source, names);
        }
        // L119
        result
    }
}

/// @pob Classes/ModList.lua:122-154 h=0b941555
impl ModStore {
    pub(crate) fn MoreInternal_List(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        // L123
        let mut result = 1.0;
        // L124
        let mut modPrecision: Option<f64> = None;
        // L125
        for &modName in names {
            // L126
            let mut modResult = 1.0; //The more multipliers for each mod are computed to the nearest percent then applied.
                                     // L128
            for k in 0..self.list_named_len(modName) {
                // L129
                let Some(m) = self.list_named_at(modName, k) else {
                    break;
                };
                // L130
                if matches(&m, Some(ModType::MORE), flags, keywordFlags, source) {
                    // L131
                    if !m.tags.is_empty() {
                        // L132
                        modResult *=
                            1.0 + or0(&context.EvalMod(&m, cfg, None), "a nil value") / 100.0;
                    } else {
                        // L134
                        modResult *= 1.0 + arith(&m.value, "field 'value'") / 100.0;
                    }
                    // L136
                    if let Some(mp) = modPrecision {
                        // L137
                        modPrecision =
                            Some(m_max(mp, highPrecision(m.name, m.r#type).unwrap_or(mp)));
                    } else {
                        // L139
                        modPrecision = highPrecision(m.name, m.r#type);
                    }
                }
            }
            // L143
            if let Some(modPrecision) = modPrecision {
                // L144
                let power = lpow(10.0, modPrecision);
                // L145
                result = m_floor(result * modResult * power) / power;
            } else {
                // L147
                result *= round(modResult, Some(2));
            }
        }
        // L150
        if let Some(parent) = self.parent.get() {
            // L151
            result *= parent.MoreInternal(context, cfg, flags, keywordFlags, source, names);
        }
        // L153
        result
    }
}

/// @pob Classes/ModList.lua:156-175 h=57b1b000
impl ModStore {
    /// `true`, or Lua's nil as `false`.
    pub(crate) fn FlagInternal_List(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        // L157
        for &modName in names {
            // L159
            for k in 0..self.list_named_len(modName) {
                // L160
                let Some(m) = self.list_named_at(modName, k) else {
                    break;
                };
                // L161
                if matches(&m, Some(ModType::FLAG), flags, keywordFlags, source) {
                    // L162
                    if !m.tags.is_empty() {
                        // L163
                        if context.EvalMod(&m, cfg, None).truthy() {
                            // L164
                            return true;
                        }
                    } else if m.value.truthy() {
                        // L166-167
                        return true;
                    }
                }
            }
        }
        // L172
        if let Some(parent) = self.parent.get() {
            // L173
            return parent.FlagInternal(context, cfg, flags, keywordFlags, source, names);
        }
        false
    }
}

/// @pob Classes/ModList.lua:177-197 h=f54918f6
impl ModStore {
    pub(crate) fn OverrideInternal_List(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> Value {
        // L178
        for &modName in names {
            // L180
            for k in 0..self.list_named_len(modName) {
                // L181
                let Some(m) = self.list_named_at(modName, k) else {
                    break;
                };
                // L182
                if matches(&m, Some(ModType::OVERRIDE), flags, keywordFlags, source) {
                    // L183
                    if !m.tags.is_empty() {
                        // L184
                        let value = context.EvalMod(&m, cfg, None);
                        // L185
                        if value.truthy() {
                            // L186
                            return value;
                        }
                    } else if m.value.truthy() {
                        // L188-189
                        return m.value.clone();
                    }
                }
            }
        }
        // L194
        if let Some(parent) = self.parent.get() {
            // L195
            return parent.OverrideInternal(context, cfg, flags, keywordFlags, source, names);
        }
        Value::Nil
    }
}

/// @pob Classes/ModList.lua:199-220 h=749601ee
impl ModStore {
    pub(crate) fn ListInternal_List(
        &self,
        context: &ModStore,
        result: &mut Vec<Value>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        // L200
        for &modName in names {
            // L202
            for k in 0..self.list_named_len(modName) {
                // L203
                let Some(m) = self.list_named_at(modName, k) else {
                    break;
                };
                // L204
                if matches(&m, Some(ModType::LIST), flags, keywordFlags, source) {
                    // L205 local value
                    // L206
                    if !m.tags.is_empty() {
                        // L207: `or nullValue` reads an undeclared global
                        let value = context.EvalMod(&m, cfg, None); // LUA-GLOBAL-NIL nullValue
                                                                    // L208
                        if value.truthy() {
                            // L209
                            result.push(value);
                        }
                    } else if m.value.truthy() {
                        // L211-212
                        result.push(m.value.clone());
                    }
                }
            }
        }
        // L217
        if let Some(parent) = self.parent.get() {
            // L218
            parent.ListInternal(context, result, cfg, flags, keywordFlags, source, names);
        }
    }
}

/// @pob Classes/ModList.lua:222-243 h=3e42aaaa
impl ModStore {
    pub(crate) fn TabulateInternal_List(
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
        // L223
        for &modName in names {
            // L225
            for k in 0..self.list_named_len(modName) {
                // L226
                let Some(m) = self.list_named_at(modName, k) else {
                    break;
                };
                // L227
                if matches(&m, modType, flags, keywordFlags, source) {
                    // L228
                    let value;
                    // L229
                    if !m.tags.is_empty() {
                        // L230
                        value = context.EvalMod(&m, cfg, None);
                    } else {
                        // L232
                        value = m.value.clone();
                    }
                    // L234
                    if value.truthy() && (value != Value::Num(0.0) || m.r#type == ModType::OVERRIDE)
                    {
                        // L235
                        result.push(TabRow { value, r#mod: m });
                    }
                }
            }
        }
        // L240
        if let Some(parent) = self.parent.get() {
            // L241
            parent.TabulateInternal(
                context,
                result,
                modType,
                cfg,
                flags,
                keywordFlags,
                source,
                names,
            );
        }
    }
}

/// @pob Classes/ModList.lua:245-249 h=94a08b7a
impl ModStore {
    /// `ConPrintf` writes to stderr.
    pub(crate) fn Print_List(&self) {
        // L246
        for m in self.ipairs() {
            // L247
            eprintln!(
                "{}|{}",
                pob_gamedata::mod_tools::formatMod(&m),
                m.source.as_deref().unwrap_or("?")
            );
        }
    }
}

impl ModStore {
    /// `#modList` (ModList; a ModDB has no array part, so 0).
    pub fn len(&self) -> usize {
        match &*self.mods.borrow() {
            StoreMods::List { list, .. } => list.len(),
            StoreMods::DB(_) => 0,
        }
    }

    pub fn is_empty(&self) -> bool {
        self.len() == 0
    }

    /// `for _, mod in ipairs(modList)` (ModList): `Arc` clones in list order (a ModDB has no
    /// array part).
    pub fn ipairs(&self) -> Vec<ModRef> {
        match &*self.mods.borrow() {
            StoreMods::List { list, .. } => list.clone(),
            StoreMods::DB(_) => Vec::new(),
        }
    }

    /// `wipeTable(modDB.mods)` (ModDB) / `wipeTable(modList)` (ModList: CalcSetup.lua:175/223/354,
    /// PartyTab). Lua's `wipeTable(modList)` clears every key of the object, so a wiped ModList
    /// also loses `parent`; its `actor`, `multipliers` and `conditions` keys become nil in Lua,
    /// here the dangling actor `{ }` and empty tables (LUA-QUIRK: Lua would raise on a later
    /// `GetCondition`/`GetMultiplier`; no PoB code queries those after a wipe).
    pub fn wipe(&self) {
        match &mut *self.mods.borrow_mut() {
            StoreMods::DB(map) => {
                map.clear();
                return;
            }
            StoreMods::List { list, index } => {
                list.clear();
                index.clear();
            }
        }
        self.parent.set(None);
        self.actor.set(Weak::new());
        self.multipliers.wipe();
        self.conditions.wipe();
    }
}
