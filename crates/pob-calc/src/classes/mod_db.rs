//! @pob-file Classes/ModDB.lua @16de4b82d
//! Classes/ModDB.lua 1-370 (ModDB storage: mods by name). The public methods dispatch on
//! `ModStore::kind`; the ModDB bodies are the `*_DB` functions here.
//!
//! Owner: MS-A.
#![allow(non_snake_case)]
// Lua statement shapes are kept (`local value` then assigned per branch; `if … then return … else … end`).
#![allow(clippy::needless_late_init, clippy::redundant_else)]

use crate::prelude::*;
use pob_gamedata::MatchKeywordFlags;

use super::mod_store::{arith, flags_match, highPrecision, mod_mark, or0, set_mod_mark};
use super::StoreMods;

// @pob Classes/ModDB.lua:6-21 h=26072e59
// (module-level statements: ipairs,pairs,select,t_insert,t_remove,m_floor,m_min,m_max,m_modf,band,bor,mod_createMod,ModDBClass)

// ---- storage access (Rust-only plumbing; one short borrow per call, §3.3) ----

impl ModStore {
    /// `#self.mods[name]`, `None` when `self.mods[name]` is nil.
    #[inline]
    pub fn db_len(&self, name: Name) -> Option<usize> {
        match &*self.mods.borrow() {
            StoreMods::DB(map) => map.get(&name).map(Vec::len),
            StoreMods::List { .. } => unreachable!("ModDB body on a ModList"),
        }
    }

    /// `self.mods[name][i]` (1-based).
    #[inline]
    pub fn db_at(&self, name: Name, i1: usize) -> Option<ModRef> {
        match &*self.mods.borrow() {
            StoreMods::DB(map) => map.get(&name).and_then(|l| l.get(i1 - 1)).cloned(),
            StoreMods::List { .. } => unreachable!("ModDB body on a ModList"),
        }
    }

    /// `self.mods` as the DB map (leaf closure, §3.3).
    #[inline]
    fn db_with<R>(&self, f: impl FnOnce(&mut ahash::AHashMap<Name, Vec<ModRef>>) -> R) -> R {
        match &mut *self.mods.borrow_mut() {
            StoreMods::DB(map) => f(map),
            StoreMods::List { .. } => unreachable!("ModDB body on a ModList"),
        }
    }
}

/// The equality test of ReplaceModInternal / ConvertModInternal after the name:
/// `mod.type == curMod.type and mod.flags == curMod.flags and mod.keywordFlags ==
/// curMod.keywordFlags and mod.source == curMod.source`.
#[inline]
pub(crate) fn same_params(m: &Mod, curMod: &Mod) -> bool {
    m.r#type == curMod.r#type
        && m.flags == curMod.flags
        && m.extra_get(n!("flags")) == curMod.extra_get(n!("flags"))
        && m.keywordFlags == curMod.keywordFlags
        && m.source == curMod.source
}

/// @pob Classes/ModDB.lua:23-27 h=eda70c53
impl ModStore {
    /// `ModDB:ModDB(parent)`: `ModStore::ModDB` (U0) allocates; this runs the constructor body.
    #[allow(dead_code)]
    pub(crate) fn init_ModDB(&self, parent: Option<&ModStoreRef>) {
        // L24
        self.init_ModStore(parent);
        // L25
        *self.mods.borrow_mut() = StoreMods::DB(ahash::AHashMap::new());
        // L26 return self
    }
}

/// @pob Classes/ModDB.lua:29-35 h=fb01ff8f
impl ModStore {
    /// `self:AddMod(mod)`: ModDB's body; a ModList runs `AddMod_List` (ModList.lua:27).
    pub fn AddMod(&self, m: ModRef) {
        if self.kind == StoreKind::ModList {
            return self.AddMod_List(m);
        }
        // L30
        let name = m.name;
        self.db_with(|mods| {
            // L31-33
            // L34
            mods.entry(name).or_default().push(m);
        });
    }
}

/// @pob Classes/ModDB.lua:42-71 h=287b0e79
impl ModStore {
    /// Replaces an existing matching mod with a new mod.
    /// If no matching mod exists, then the function returns false
    pub(crate) fn ReplaceModInternal_DB(&self, m: ModRef) -> bool {
        let mut m = m;
        // L43
        let name = m.name;
        // L48: Find the index of the existing mod, if it is in the table
        let found = self.db_with(|mods| {
            // L44-46
            let modList = mods.entry(name).or_default();
            // L50
            let mut modIndex: i64 = -1;
            // L51
            for (i, curMod) in modList.iter().enumerate() {
                // L53
                if m.name == curMod.name
                    && same_params(&m, curMod)
                    && !mod_mark(curMod, n!("replaced"))
                {
                    // L54
                    modIndex = i as i64 + 1;
                    // L55
                    set_mod_mark(&mut m, n!("replaced"));
                    // L56
                    break;
                }
            }
            // L60: Add or replace the mod
            // L61
            if modIndex > 0 {
                // L62
                modList[modIndex as usize - 1] = m.clone();
                // L63
                return true;
            }
            false
        });
        if found {
            return true;
        }
        // L66
        if let Some(parent) = self.parent.get() {
            // L67
            return parent.ReplaceModInternal(m);
        }
        // L70
        false
    }
}

/// @pob Classes/ModDB.lua:80-110 h=11ad37e7
impl ModStore {
    /// Converts an existing mod with oldName to a new mod with a different name.
    /// Moves the mod from the old name's bucket to the new name's bucket.
    /// If no matching mod exists, then the function returns false
    pub(crate) fn ConvertModInternal_DB(&self, oldName: Name, m: ModRef) -> bool {
        let mut m = m;
        // L81
        if self.db_len(oldName).is_none() {
            // L82
            if let Some(parent) = self.parent.get() {
                // L83
                return parent.ConvertModInternal(oldName, m);
            }
            // L85
            return false;
        }
        // L88
        let converted = self.db_with(|mods| {
            let oldList = mods.get_mut(&oldName).expect("checked above");
            // L89
            for i in 0..oldList.len() {
                // L90
                let curMod = &oldList[i];
                // L91
                if oldName == curMod.name
                    && same_params(&m, curMod)
                    && !mod_mark(curMod, n!("converted"))
                {
                    // Remove from old name's bucket
                    // L93
                    oldList.remove(i);
                    // Add to new name's bucket
                    // L95
                    let newName = m.name;
                    // L96-98
                    let newList = mods.entry(newName).or_default();
                    // L99
                    set_mod_mark(&mut m, n!("converted"));
                    // L100
                    newList.push(m.clone());
                    // L101
                    return true;
                }
            }
            false
        });
        if converted {
            return true;
        }
        // L105
        if let Some(parent) = self.parent.get() {
            // L106
            return parent.ConvertModInternal(oldName, m);
        }
        // L109
        false
    }
}

/// @pob Classes/ModDB.lua:112-121 h=96951221
impl ModStore {
    /// `self:AddList(modList)`: ModDB's body; a ModList runs `AddList_List` (ModList.lua:88).
    pub fn AddList(&self, modList: &[ModRef]) {
        if self.kind == StoreKind::ModList {
            return self.AddList_List(modList);
        }
        // L113
        self.db_with(|mods| {
            // L114
            for m in modList {
                // L115
                let name = m.name;
                // L116-118
                // L119
                mods.entry(name).or_default().push(m.clone());
            }
        });
    }
}

/// @pob Classes/ModDB.lua:123-134 h=94daa6cf
impl ModStore {
    /// ModDB only: a ModList has no AddDB method, and a ModList argument has no `mods`.
    pub fn AddDB(&self, modDB: &ModStore) {
        if self.kind == StoreKind::ModList {
            // LUA-QUIRK: ModList.lua defines no AddDB.
            lua_error!("attempt to call method 'AddDB' (a nil value)");
        }
        // L125: `pairs(modDB.mods)` (snapshot first: `modDB` may be `self`)
        let src: Vec<(Name, Vec<ModRef>)> = match &*modDB.mods.borrow() {
            // LUA-ORDER: PoB's name order is random; it only decides which bucket is
            // created first, which no query observes.
            StoreMods::DB(map) => map.iter().map(|(k, v)| (*k, v.clone())).collect(),
            StoreMods::List { .. } => {
                lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
            }
        };
        // L124
        self.db_with(|mods| {
            for (modName, modList) in src {
                // L126-129
                let modsName = mods.entry(modName).or_default();
                // L130
                for m in modList {
                    // L131
                    modsName.push(m);
                }
            }
        });
    }
}

/// @pob Classes/ModDB.lua:136-162 h=09eea2a7
impl ModStore {
    pub(crate) fn SumInternal_DB(
        &self,
        context: &ModStore,
        modType: ModType,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        // L137
        let mut result = 0.0;
        // L138
        let mut globalLimits: Option<GlobalLimits> = None;
        // L139
        for &name in names {
            // L140-141
            if let Some(n) = self.db_len(name) {
                // L142
                for i in 1..=n {
                    // L143
                    let Some(m) = self.db_at(name, i) else { break };
                    // L144
                    if m.r#type == modType
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && source.map_or(true, |source| m.source_category() == Some(source))
                    {
                        // L145
                        if !m.tags.is_empty() {
                            // L146-148
                            let gl = globalLimits.get_or_insert_with(GlobalLimits::default);
                            // L149
                            let value = or0(&context.EvalMod(&m, cfg, Some(gl)), "local 'value'");
                            // L150
                            result += value;
                        } else {
                            // L152
                            result += arith(&m.value, "field 'value'");
                        }
                    }
                }
            }
        }
        // L158
        if let Some(parent) = self.parent.get() {
            // L159
            result += parent.SumInternal(context, modType, cfg, flags, keywordFlags, source, names);
        }
        // L161
        result
    }
}

/// @pob Classes/ModDB.lua:164-204 h=c1f2fb20
impl ModStore {
    pub(crate) fn MoreInternal_DB(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> f64 {
        // L165
        let mut result = 1.0;
        // L166
        let mut modPrecision: Option<f64> = None;
        // L167
        let mut globalLimits: Option<GlobalLimits> = None;
        // L168
        for &name in names {
            // L169
            let len = self.db_len(name);
            // L170
            let mut modResult = 1.0; //The more multipliers for each mod are computed to the nearest percent then applied.
                                     // L171
            if let Some(n) = len {
                // L172
                for i in 1..=n {
                    // L173
                    let Some(m) = self.db_at(name, i) else { break };
                    // L174
                    if m.r#type == ModType::MORE
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && super::mod_store::source_match(source, &m)
                    {
                        // L175
                        let value;
                        // L176
                        if !m.tags.is_empty() {
                            // L177-179
                            let gl = globalLimits.get_or_insert_with(GlobalLimits::default);
                            // L180
                            value = or0(&context.EvalMod(&m, cfg, Some(gl)), "local 'value'");
                        } else {
                            // L182
                            value = or0(&m.value, "local 'value'");
                        }
                        // L184
                        modResult *= 1.0 + value / 100.0;
                        // L185
                        if let Some(mp) = modPrecision {
                            // L186
                            modPrecision =
                                Some(m_max(mp, highPrecision(m.name, m.r#type).unwrap_or(mp)));
                        } else {
                            // L188
                            modPrecision = highPrecision(m.name, m.r#type);
                        }
                    }
                }
            }
            // L193
            if let Some(modPrecision) = modPrecision {
                // L194
                let power = lpow(10.0, modPrecision);
                // L195
                result = m_floor(result * modResult * power) / power;
            } else {
                // L197
                result *= round(modResult, Some(2));
            }
        }
        // L200
        if let Some(parent) = self.parent.get() {
            // L201
            result *= parent.MoreInternal(context, cfg, flags, keywordFlags, source, names);
        }
        // L203
        result
    }
}

/// @pob Classes/ModDB.lua:206-227 h=c2d7c64d
impl ModStore {
    /// `true`, or Lua's nil as `false`.
    pub(crate) fn FlagInternal_DB(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        // L207
        for &name in names {
            // L208-209
            if let Some(n) = self.db_len(name) {
                // L210
                for i in 1..=n {
                    // L211
                    let Some(m) = self.db_at(name, i) else { break };
                    // L212
                    if m.r#type == ModType::FLAG
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && super::mod_store::source_match(source, &m)
                    {
                        // L213
                        if !m.tags.is_empty() {
                            // L214
                            if context.EvalMod(&m, cfg, None).truthy() {
                                // L215
                                return true;
                            }
                        } else if m.value.truthy() {
                            // L217-218
                            return true;
                        }
                    }
                }
            }
        }
        // L224
        if let Some(parent) = self.parent.get() {
            // L225
            return parent.FlagInternal(context, cfg, flags, keywordFlags, source, names);
        }
        false
    }
}

/// @pob Classes/ModDB.lua:229-251 h=4ad09d43
impl ModStore {
    pub(crate) fn OverrideInternal_DB(
        &self,
        context: &ModStore,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> Value {
        // L230
        for &name in names {
            // L231-232
            if let Some(n) = self.db_len(name) {
                // L233
                for i in 1..=n {
                    // L234
                    let Some(m) = self.db_at(name, i) else { break };
                    // L235
                    if m.r#type == ModType::OVERRIDE
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && super::mod_store::source_match(source, &m)
                    {
                        // L236
                        if !m.tags.is_empty() {
                            // L237
                            let value = context.EvalMod(&m, cfg, None);
                            // L238
                            if value.truthy() {
                                // L239
                                return value;
                            }
                        } else if m.value.truthy() {
                            // L241-242
                            return m.value.clone();
                        }
                    }
                }
            }
        }
        // L248
        if let Some(parent) = self.parent.get() {
            // L249
            return parent.OverrideInternal(context, cfg, flags, keywordFlags, source, names);
        }
        Value::Nil
    }
}

/// @pob Classes/ModDB.lua:253-276 h=14edd639
impl ModStore {
    pub(crate) fn ListInternal_DB(
        &self,
        context: &ModStore,
        result: &mut Vec<Value>,
        cfg: Option<&ModCfg>,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) {
        // L254
        for &name in names {
            // L255-256
            if let Some(n) = self.db_len(name) {
                // L257
                for i in 1..=n {
                    // L258
                    let Some(m) = self.db_at(name, i) else { break };
                    // L259
                    if m.r#type == ModType::LIST
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && super::mod_store::source_match(source, &m)
                    {
                        // L260 local value
                        // L261
                        if !m.tags.is_empty() {
                            // L262: `or nullValue` reads an undeclared global
                            let value = context.EvalMod(&m, cfg, None); // LUA-GLOBAL-NIL nullValue
                                                                        // L263
                            if value.truthy() {
                                // L264
                                result.push(value);
                            }
                        } else if m.value.truthy() {
                            // L266-267
                            result.push(m.value.clone());
                        }
                    }
                }
            }
        }
        // L273
        if let Some(parent) = self.parent.get() {
            // L274
            parent.ListInternal(context, result, cfg, flags, keywordFlags, source, names);
        }
    }
}

/// @pob Classes/ModDB.lua:278-306 h=062d225c
impl ModStore {
    pub(crate) fn TabulateInternal_DB(
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
        // L279
        let mut globalLimits: Option<GlobalLimits> = None;
        // L280
        for &modName in names {
            // L282-283
            if let Some(n) = self.db_len(modName) {
                // L284
                for i in 1..=n {
                    // L285
                    let Some(m) = self.db_at(modName, i) else {
                        break;
                    };
                    // L286
                    if modType.map_or(true, |t| m.r#type == t)
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && super::mod_store::source_match(source, &m)
                    {
                        // L287
                        let value;
                        // L288
                        if !m.tags.is_empty() {
                            // L289-291
                            let gl = globalLimits.get_or_insert_with(GlobalLimits::default);
                            // L292
                            value = context.EvalMod(&m, cfg, Some(gl));
                        } else {
                            // L294
                            value = m.value.clone();
                        }
                        // L296
                        if value.truthy()
                            && (value != Value::Num(0.0) || m.r#type == ModType::OVERRIDE)
                        {
                            // L297
                            result.push(TabRow { value, r#mod: m });
                        }
                    }
                }
            }
        }
        // L303
        if let Some(parent) = self.parent.get() {
            // L304
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

/// @pob Classes/ModDB.lua:315-334 h=bbffab01
impl ModStore {
    /// Checks if a mod exists with the given properties
    pub(crate) fn HasModInternal_DB(
        &self,
        modType: ModType,
        flags: u32,
        keywordFlags: u32,
        source: Option<&str>,
        names: &[Name],
    ) -> bool {
        // L316
        for &name in names {
            // L317-318
            if let Some(n) = self.db_len(name) {
                // L319
                for i in 1..=n {
                    // L320
                    let Some(m) = self.db_at(name, i) else { break };
                    // L321
                    if m.r#type == modType
                        && flags_match(flags, &m)
                        && MatchKeywordFlags(keywordFlags, m.keywordFlags)
                        && super::mod_store::source_match(source, &m)
                    {
                        // L322
                        return true;
                    }
                }
            }
        }
        // L327
        if let Some(parent) = self.parent.get() {
            // L328
            let parentResult = parent.HasModInternal(modType, flags, keywordFlags, source, names);
            // L329
            if parentResult {
                // L330
                return true;
            }
        }
        // L333
        false
    }
}

/// @pob Classes/ModDB.lua:336-370 h=2097509e
impl ModStore {
    /// `modDB:Print()` / `modList:Print()` (ModList.lua:245). `ConPrintf` writes to stderr.
    pub fn Print(&self) {
        if self.kind == StoreKind::ModList {
            return self.Print_List();
        }
        // L337
        eprintln!("=== Modifiers ===");
        // L338-341
        let mut modNames: Vec<Name> = self.db_with(|mods| mods.keys().copied().collect());
        // L342
        pob_lua::sort::sort(&mut modNames, |a, b| a.as_str() < b.as_str());
        // L343
        for modName in modNames {
            // L344
            eprintln!("{}", lfmt!("'%s':", modName));
            // L345
            for m in self.mods_named(modName) {
                // L346
                eprintln!(
                    "\t{} = {}|{}|{}|{}|{}",
                    pob_gamedata::mod_tools::formatValue(&m.value),
                    m.r#type.as_str(),
                    pob_gamedata::mod_tools::formatFlags(m.flags, ModFlag::ENTRIES),
                    pob_gamedata::mod_tools::formatFlags(m.keywordFlags, KeywordFlag::ENTRIES),
                    pob_gamedata::mod_tools::formatTags(&m.tags),
                    m.source.as_deref().unwrap_or("?")
                );
            }
        }
        // L349
        eprintln!("=== Conditions ===");
        // L350
        let mut nameList: Vec<Name> = Vec::new();
        // L351
        for (name, value) in self.conditions.pairs() {
            // L352
            if value.truthy() {
                if let Some(name) = name.name() {
                    // L353
                    nameList.push(name);
                }
            }
        }
        // L356
        pob_lua::sort::sort(&mut nameList, |a, b| a.as_str() < b.as_str());
        // L357
        for name in &nameList {
            // L358
            eprintln!("{name}");
        }
        // L360
        eprintln!("=== Multipliers ===");
        // L361
        nameList.clear();
        // L362
        for (name, value) in self.multipliers.pairs() {
            // L363
            if value.num() > 0.0 {
                if let Some(name) = name.name() {
                    // L364
                    nameList.push(name);
                }
            }
        }
        // L367
        pob_lua::sort::sort(&mut nameList, |a, b| a.as_str() < b.as_str());
        // L368
        for name in &nameList {
            // L369
            eprintln!(
                "{}",
                lfmt!("%s = %d", *name, self.multipliers.get(*name).num())
            );
        }
    }
}
