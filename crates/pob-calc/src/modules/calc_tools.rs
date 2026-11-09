//! @pob-file Modules/CalcTools.lua @16de4b82d
//! `calcLib` (Modules/CalcTools.lua 1-303). Owner: CT.
#![allow(non_snake_case)]
// literal Lua shapes (`local gameId` then assignment, `… or false`)
#![allow(
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::explicit_auto_deref
)]

use crate::prelude::*;
use pob_gamedata::{GemData, LevelTable};

// @pob Modules/CalcTools.lua:6-13 h=f046369b
// (module-level statements: pairs,t_insert,t_remove,m_floor,m_min,m_max,calcLib)

/// @pob Modules/CalcTools.lua:16-18 h=6d716a94
/// Calculate and combine INC/MORE modifiers for the given modifier names
pub fn r#mod(modStore: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> f64 {
    // L17
    (1.0 + (modStore.Sum(ModType::INC, cfg, names)) / 100.0) * modStore.More(cfg, names)
}

/// @pob Modules/CalcTools.lua:25-29 h=9621759b
/// Calculates additive and multiplicative modifiers for specified modifier names
/// → (inc, more)
pub fn mods(modStore: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> (f64, f64) {
    // L26
    let inc = 1.0 + modStore.Sum(ModType::INC, cfg, names) / 100.0;
    // L27
    let more = modStore.More(cfg, names);
    // L28
    (inc, more)
}

/// @pob Modules/CalcTools.lua:32-39 h=d5d937d8
/// Calculate value
pub fn val(modStore: &ModStore, name: Name, cfg: Option<&ModCfg>) -> f64 {
    // L33
    let baseVal = modStore.Sum(ModType::BASE, cfg, &[name]);
    // L34
    if baseVal != 0.0 {
        // L35
        baseVal * r#mod(modStore, cfg, &[name])
    } else {
        // L37
        0.0
    }
}

/// What `validateGemLevel` reads and writes: a gem instance of the build model
/// (SkillsTab.lua:1225) or a skill effect table (CalcActiveSkill.lua:54/513,
/// CalcSetup.lua:1460/1716). CCR-0014: the frozen signature took only
/// `&GemInstance`; the calc callers pass skill effects.
pub trait GemLevelTarget {
    /// `gemInstance.grantedEffect`.
    fn lua_grantedEffect(&self) -> Option<GrantedEffectRef>;
    /// `gemInstance.gemData`.
    fn lua_gemData(&self) -> Option<GemDataRef>;
    /// `gemInstance.level`.
    fn lua_level(&self) -> f64;
    /// `gemInstance.level = v`.
    fn set_lua_level(&self, v: f64);
}

impl GemLevelTarget for GemInstance {
    fn lua_grantedEffect(&self) -> Option<GrantedEffectRef> {
        self.grantedEffect.get()
    }
    fn lua_gemData(&self) -> Option<GemDataRef> {
        self.gemData.get()
    }
    fn lua_level(&self) -> f64 {
        self.level.get()
    }
    fn set_lua_level(&self, v: f64) {
        self.level.set(v);
    }
}

impl GemLevelTarget for SkillEffect {
    fn lua_grantedEffect(&self) -> Option<GrantedEffectRef> {
        Some(self.grantedEffect.get())
    }
    fn lua_gemData(&self) -> Option<GemDataRef> {
        self.gemData.get()
    }
    fn lua_level(&self) -> f64 {
        self.level.get()
    }
    fn set_lua_level(&self, v: f64) {
        self.level.set(v);
    }
}

/// `next(grantedEffect.levels)`: the first key LuaJIT's `next` returns. The
/// data files build `levels` with a constructor of explicit `[n] = { … }`
/// entries whose values are tables, which LuaJIT's parser does not put in a
/// template: it emits `TNEW` with no array part and `hsize2hbits(#entries)`
/// hash bits, then one `TSET` per entry in source (ascending) order, so every
/// level lives in the hash part. `LjIntMap` replays that layout.
pub fn levels_next_key(levels: &LevelTable) -> Option<f64> {
    let n = levels.0.iter().filter(|l| l.is_some()).count();
    let mut t: LjIntMap<()> = LjIntMap::with_sizes(0, n as u32);
    for (i, l) in levels.0.iter().enumerate() {
        if l.is_some() {
            t.set(i as i64 + 1, Some(()));
        }
    }
    t.keys().first().map(|k| *k as f64)
}

/// `#grantedEffect.levels`: with every level in the hash part (see
/// [`levels_next_key`]), LuaJIT's `lj_tab_len` is `unbound_search(t, 0)`:
/// doubling probes from 1, then a binary search for a border.
pub fn levels_len(levels: &LevelTable) -> usize {
    let has = |k: usize| levels.get(k as f64).is_some();
    let mut i = 0usize;
    let mut j = 1usize;
    while has(j) {
        i = j;
        j *= 2;
    }
    while j - i > 1 {
        let m = (i + j) / 2;
        if has(m) {
            i = m;
        } else {
            j = m;
        }
    }
    i
}

/// @pob Modules/CalcTools.lua:42-58 h=742b293c
/// Validate the level of the given gem
pub fn validateGemLevel<G: GemLevelTarget + ?Sized>(gemInstance: &G) {
    // L43
    let grantedEffect = match gemInstance.lua_grantedEffect() {
        Some(ge) => ge,
        None => match gemInstance.lua_gemData() {
            Some(gd) => gd.grantedEffect.clone(),
            None => lua_error!("attempt to index field 'gemData' (a nil value)"),
        },
    };
    // L44
    if grantedEffect.levels.get(gemInstance.lua_level()).is_none() {
        // L45 Try limiting to the level range of the skill
        // L46
        gemInstance.set_lua_level(m_max(1.0, gemInstance.lua_level()));
        // L47
        if levels_len(&grantedEffect.levels) > 0 {
            // L48
            gemInstance.set_lua_level(m_min(
                levels_len(&grantedEffect.levels) as f64,
                gemInstance.lua_level(),
            ));
        }
    }
    // L51
    if grantedEffect.levels.get(gemInstance.lua_level()).is_none() {
        if let Some(gemData) = gemInstance.lua_gemData() {
            // `gemData.naturalMaxLevel` is always set in the data
            // L52
            gemInstance.set_lua_level(gemData.naturalMaxLevel);
        }
    }
    // L54
    if grantedEffect.levels.get(gemInstance.lua_level()).is_none() {
        // L55 That failed, so just grab any level
        // L56 (every data.skills entry has a level at the pin, so `next` is never nil here)
        if let Some(k) = levels_next_key(&grantedEffect.levels) {
            gemInstance.set_lua_level(k);
        }
    }
}

/// A Lua array used as the RPN stack of `doesTypeExpressionMatch`: the
/// values `1..#stack` plus `stack[0]`, which `stack[#stack] = …` writes when the
/// stack is empty (the hash/slot-0 key no `#` or `ipairs` sees).
struct LuaBoolStack {
    arr: Vec<bool>,
    slot0: Option<bool>,
}

impl LuaBoolStack {
    /// `stack[#stack]`.
    fn top(&self) -> Option<bool> {
        match self.arr.last() {
            Some(b) => Some(*b),
            None => self.slot0,
        }
    }
    /// `stack[#stack] = v` (nil shrinks the array by one).
    fn set_top(&mut self, v: Option<bool>) {
        if self.arr.is_empty() {
            self.slot0 = v;
        } else {
            match v {
                Some(b) => *self.arr.last_mut().expect("non-empty") = b,
                None => {
                    self.arr.pop();
                }
            }
        }
    }
    /// `t_remove(stack)`.
    fn remove(&mut self) -> Option<bool> {
        self.arr.pop()
    }
}

/// @pob Modules/CalcTools.lua:61-82 h=6c445537
/// Evaluate a skill type postfix expression
pub fn doesTypeExpressionMatch(
    checkTypes: &[SkillTypeExpr],
    skillTypes: &SkillTypeSet,
    minionTypes: Option<&SkillTypeSet>,
) -> bool {
    // L62
    let mut stack = LuaBoolStack {
        arr: Vec::new(),
        slot0: None,
    };
    // L63 (pairs over an array: 1..n)
    for skillType in checkTypes {
        match *skillType {
            // L64
            SkillTypeExpr::OR => {
                // L65
                let other = stack.remove();
                // L66
                let a = stack.top();
                stack.set_top(if a == Some(true) { a } else { other });
            }
            // L67
            SkillTypeExpr::AND => {
                // L68
                let other = stack.remove();
                // L69
                let a = stack.top();
                stack.set_top(if a == Some(true) { other } else { a });
            }
            // L70
            SkillTypeExpr::NOT => {
                // L71
                let a = stack.top();
                stack.set_top(Some(a != Some(true)));
            }
            SkillTypeExpr::Type(t) => {
                // L73
                stack
                    .arr
                    .push(skillTypes.has(t) || minionTypes.is_some_and(|m| m.has(t)) || false);
            }
        }
    }
    // L76
    for val in &stack.arr {
        // L77
        if *val {
            // L78
            return true;
        }
    }
    // L81
    false
}

/// @pob Modules/CalcTools.lua:86-161 h=b06e1609
/// Check if given support skill can support the given active skill
/// `imbuedSupport` is always nil at the pin's call sites (CalcActiveSkill.lua:116/134/148, CalcTriggers.lua:233).
pub fn canGrantedEffectSupportActiveSkill(
    grantedEffect: &GrantedEffect,
    activeSkill: &ActiveSkill,
    imbuedSupport: Option<&Val>,
    appliesToGrantedSkills: bool,
) -> bool {
    let activeEffect = activeSkill.activeEffect.get();
    let activeGrantedEffect = activeEffect.grantedEffect.get();
    // L87 (`grantedEffect.unsupported` is never set in the data: no schema field)
    if activeGrantedEffect.cannotBeSupported {
        // L88
        return false;
    }
    // L90
    if grantedEffect.supportGemsOnly && activeEffect.gemData.get().is_none() {
        // L91
        return false;
    }

    // L94 Special case for things like Forbidden Shako or Hungry Loop with  for example Prismatic Burst and another compatible support
    // L95
    if !appliesToGrantedSkills
        && grantedEffect.fromItem
        && grantedEffect.support
        && (activeGrantedEffect.fromItem
            || pob_lua::string::sub(&activeGrantedEffect.modSource, 1, Some("Item".len() as i64))
                == "Item"
            || activeEffect
                .srcInstance
                .get()
                .is_some_and(|s| s.fromItem.get()))
    {
        // L96
        return false;
    }

    // L99
    let effectiveSkillTypes: SkillTypeSet;
    // L100
    let effectiveMinionTypes: Option<SkillTypeSet>;
    let summonSkill = activeSkill.summonSkill.get();
    // L101 Use the skillTypes from the gem so it ignores any support added types
    if imbuedSupport.is_some_and(Val::truthy) {
        // L102
        effectiveSkillTypes = match &summonSkill {
            Some(s) => s.activeEffect.get().grantedEffect.get().skillTypes,
            None => activeGrantedEffect.skillTypes,
        };
        // L103
        effectiveMinionTypes = if grantedEffect.ignoreMinionTypes {
            None
        } else {
            summonSkill
                .as_ref()
                .and_then(|s| s.activeEffect.get().grantedEffect.get().minionSkillTypes)
                .or(activeGrantedEffect.minionSkillTypes)
        };
    } else {
        // L105
        effectiveSkillTypes = match &summonSkill {
            Some(s) => s.skillTypes.get().with(|t| *t),
            None => activeSkill.skillTypes.get().with(|t| *t),
        };
        // L106
        effectiveMinionTypes = if grantedEffect.ignoreMinionTypes {
            None
        } else {
            summonSkill
                .as_ref()
                .and_then(|s| s.minionSkillTypes.get())
                .or_else(|| activeSkill.minionSkillTypes.get())
                .map(|t| t.with(|t| *t))
        };
    }

    // L109 if the activeSkill is a Minion's skill like "Default Attack", use minion's skillTypes instead for exclusions
    // L110 otherwise compare support to activeSkill directly
    // L111
    if !grantedEffect.excludeSkillTypes.is_empty()
        && doesTypeExpressionMatch(&grantedEffect.excludeSkillTypes, &effectiveSkillTypes, None)
    {
        // L112
        return false;
    }
    let actor = || {
        activeSkill
            .actor()
            .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"))
    };
    // L114
    if grantedEffect.isTrigger {
        let a = actor();
        let enemy = a
            .enemy
            .get()
            .upgrade()
            .unwrap_or_else(|| lua_error!("attempt to index field 'enemy' (a nil value)"));
        let player = enemy.player.get().upgrade();
        if !player.is_some_and(|p| Rc::ptr_eq(&p, &a)) {
            // L115
            return false;
        }
    }
    // L117 Special case for Sacred Wisps, i.e. Wisps Support has a weaponType of Wand so it should only match with Active Skills that at least have Wand as a weaponType.
    // L118 Super special case for Varunastra, e.g. allow Nightblade to support Smite.
    // L119
    let actorHasAllOneHand = {
        let a = actor();
        a.weaponData1.get().truthy(n!("countsAsAll1H"))
            || a.weaponData2.get().truthy(n!("countsAsAll1H"))
    };
    // L120
    if let Some(weaponTypes) = &grantedEffect.weaponTypes {
        // L121 Build a lookup of the active skill's weapon types
        // L122
        let mut activeTypeLookup: Vec<Name> = Vec::new();
        // L123
        if let Some(activeTypes) = &activeGrantedEffect.weaponTypes {
            // L124
            for activeType in activeTypes.iter() {
                // L125
                activeTypeLookup.push(*activeType);
            }
        } else if effectiveSkillTypes.has(SkillType::SkillGrantedBySupport) {
            // L127
            // L128 Skills granted by supports do not have their own weaponTypes, so check the equipped weapons instead.
            let a = actor();
            // L129
            let t1 = a.weaponData1.get().get(n!("type"));
            if t1.truthy() {
                // L130
                activeTypeLookup.push(lookup_key(&t1));
            }
            // L132
            let t2 = a.weaponData2.get().get(n!("type"));
            if t2.truthy() {
                // L133
                let k = lookup_key(&t2);
                if !activeTypeLookup.contains(&k) {
                    activeTypeLookup.push(k);
                }
            }
        }
        // L136 If the support expects a weapon type but the active skill doesn't have any (e.g. shield skills), it's not a match.
        // L137
        if activeTypeLookup.is_empty() {
            // L138
            return false;
        }
        // L140 Varunastra counts as every one-handed melee weapon type, but notably not Wand or Shield.
        // L141
        if actorHasAllOneHand {
            // L142-146
            for t in [
                n!("Claw"),
                n!("Dagger"),
                n!("One Handed Axe"),
                n!("One Handed Mace"),
                n!("One Handed Sword"),
            ] {
                if !activeTypeLookup.contains(&t) {
                    activeTypeLookup.push(t);
                }
            }
        }
        // L148
        let mut typeMatch = false;
        // L149
        for grantedType in weaponTypes.iter() {
            // L150
            if activeTypeLookup.contains(grantedType) {
                // L151
                typeMatch = true;
                // L152
                break;
            }
        }
        // L155 No match, does not support the active skill
        // L156
        if !typeMatch {
            // L157
            return false;
        }
    }
    // L160
    grantedEffect.requireSkillTypes.is_empty()
        || doesTypeExpressionMatch(
            &grantedEffect.requireSkillTypes,
            &effectiveSkillTypes,
            effectiveMinionTypes.as_ref(),
        )
}

/// A weapon `type` value used as a table key (`activeTypeLookup[type] = true`).
fn lookup_key(v: &Val) -> Name {
    match v {
        Val::Str(s) => Name::from(&**s),
        Val::Num(x) => Name::from(tostring_num(*x).as_str()),
        other => Name::from(format!("{other:?}").as_str()),
    }
}

/// @pob Modules/CalcTools.lua:164-176 h=898869a0
/// Check if given gem is of the given type ("all", "strength", "melee", etc)
/// `gem` is a gem data table (`data.gems[id]`).
pub fn gemIsType(gem: &GemData, r#type: &str, includeTransfigured: bool) -> bool {
    let tags = |k: &str| gem.tags.contains(&Name::from(k));
    let lname = || pob_lua::string::lower(&gem.name);
    // L165
    r#type == "all"
        // L166
        || (r#type == "elemental" && (tags("fire") || tags("cold") || tags("lightning")))
        // L167
        || (r#type == "aoe" && tags("area"))
        // L168
        || (r#type == "trap or mine" && (tags("trap") || tags("mine")))
        // L169
        || ((r#type == "active skill" || r#type == "grants_active_skill" || r#type == "skill")
            && tags("grants_active_skill")
            && !tags("support"))
        // L170
        || (r#type == "non-vaal" && !tags("vaal"))
        // L171
        || (r#type == "non-exceptional" && !tags("exceptional"))
        // L172
        || (r#type == lname())
        // L173
        || (r#type == pob_lua::gsub(&lname(), "^vaal ", "", None).0)
        // L174
        || (includeTransfigured && isGemIdSame(&gem.name, r#type, true))
        // L175
        || ((r#type != "active skill" && r#type != "grants_active_skill" && r#type != "skill")
            && tags(r#type))
}

/// @pob Modules/CalcTools.lua:179-189 h=1517a652
/// In-game formula
pub fn getGemStatRequirement(level: f64, isSupport: bool, multi: f64) -> f64 {
    // L180
    if multi == 0.0 {
        // L181
        return 0.0;
    }
    // L183
    let mut statType = 0.7;
    // L184
    if isSupport {
        // L185
        statType = 0.5;
    }
    // L187
    let req = round(
        (20.0 + (level - 3.0) * 3.0) * lpow(multi / 100.0, 0.9) * statType,
        None,
    );
    // L188
    if req < 14.0 {
        0.0
    } else {
        req
    }
}

/// @pob Modules/CalcTools.lua:192-251 h=c4d7b24e
/// Build table of stats for the given skill instance
/// → `{ [stat] = value }`.
pub fn buildSkillInstanceStats(
    skillInstance: &SkillEffect,
    grantedEffect: &GrantedEffect,
) -> Table {
    // L193
    let stats = Table::new();
    // L194
    if skillInstance.quality.get() > 0.0 && !grantedEffect.qualityStats.is_empty() {
        // L195
        let qualityStats = &grantedEffect.qualityStats;
        // L196
        for stat in qualityStats {
            // L197
            let v = stats.num_or(stat.0, 0.0) + m_modf(stat.1 * skillInstance.quality.get()).0;
            stats.set(stat.0, v);
        }
    }
    // L200
    let level = grantedEffect.levels.get(skillInstance.level.get());
    // L201
    let mut availableEffectiveness: Option<f64> = None;
    // L202 (`level.levelRequirement` is typed f64 in the schema: 0 where the data has none,
    // only on levels of skills without stats, so the `or 1` cannot differ observably)
    let actorLevel = skillInstance
        .actorLevel
        .get()
        .or_else(|| level.map(|l| l.levelRequirement))
        .unwrap_or(1.0);
    // L203
    for (index0, stat) in grantedEffect.stats.iter().enumerate() {
        let index = index0 + 1;
        // L204 Static value used as default (assumes statInterpolation == 1)
        // L205
        let mut statValue = level
            .and_then(|l| l.values.get(index0).copied())
            .unwrap_or(1.0);
        // L206
        if let Some(level) = level {
            let interp = level.statInterpolation.get(index0).copied();
            // L207
            if interp == Some(3) {
                // L208 Effectiveness interpolation
                // L209
                if availableEffectiveness.is_none() {
                    // L210-212
                    availableEffectiveness = Some(
                        (data().gameConstants.SkillDamageBaseEffectiveness
                            + data().gameConstants.SkillDamageIncrementalEffectiveness
                                * (actorLevel - 1.0))
                            * grantedEffect.baseEffectiveness.unwrap_or(1.0)
                            * lpow(
                                1.0 + grantedEffect.incrementalEffectiveness.unwrap_or(0.0),
                                actorLevel - 1.0,
                            ),
                    );
                }
                // L214
                let li =
                    level.values.get(index0).copied().unwrap_or_else(|| {
                        lua_error!("attempt to perform arithmetic on a nil value")
                    });
                statValue = round(availableEffectiveness.expect("set above") * li, None);
            } else if interp == Some(2) {
                // L215
                // L216 Linear interpolation; I'm actually just guessing how this works

                // L218 Order the levels, since sometimes they skip around
                // L219
                let mut orderedLevels: Vec<f64> = Vec::new();
                // L220
                let mut currentLevelIndex: Option<usize> = None;
                // L221
                for (i, l) in grantedEffect.levels.0.iter().enumerate() {
                    if l.is_some() {
                        // L222
                        orderedLevels.push((i + 1) as f64);
                    }
                }
                // L224 (distinct numbers: the sorted order is unique)
                orderedLevels.sort_by(f64::total_cmp);
                // L225
                for (idx, level) in orderedLevels.iter().enumerate() {
                    // L226
                    if skillInstance.level.get() == *level {
                        // L227
                        currentLevelIndex = Some(idx + 1);
                    }
                }

                let lvl = |k: f64| {
                    grantedEffect
                        .levels
                        .get(k)
                        .cloned()
                        .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
                };
                let stat_at = |l: &SkillLevelRef| {
                    l.values.get(index0).copied().unwrap_or_else(|| {
                        lua_error!("attempt to perform arithmetic on a nil value")
                    })
                };
                // L231
                if orderedLevels.len() > 1 {
                    let currentLevelIndex = currentLevelIndex.unwrap_or_else(|| {
                        lua_error!("attempt to perform arithmetic on local 'currentLevelIndex' (a nil value)")
                    });
                    // L232
                    let nextLevelIndex =
                        m_min((currentLevelIndex + 1) as f64, orderedLevels.len() as f64) as usize;
                    // L233
                    let nextReq = lvl(orderedLevels[nextLevelIndex - 1]).levelRequirement;
                    // L234
                    let prevReq = lvl(orderedLevels[nextLevelIndex - 2]).levelRequirement;
                    // L235
                    let nextStat = stat_at(&lvl(orderedLevels[nextLevelIndex - 1]));
                    // L236
                    let prevStat = stat_at(&lvl(orderedLevels[nextLevelIndex - 2]));
                    // L237
                    statValue = round(
                        prevStat
                            + (nextStat - prevStat) * (actorLevel - prevReq) / (nextReq - prevReq),
                        None,
                    );
                } else {
                    // L239
                    let cur = currentLevelIndex
                        .and_then(|i| orderedLevels.get(i - 1).copied())
                        .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
                    statValue = round(stat_at(&lvl(cur)), None);
                }
            }
        }
        // L243
        let v = stats.num_or(*stat, 0.0) + statValue;
        stats.set(*stat, v);
        let _ = index;
    }
    // L245
    if !grantedEffect.constantStats.is_empty() {
        // L246
        for stat in &grantedEffect.constantStats {
            // L247
            let v = stats.num_or(stat.0, 0.0) + stat.1;
            stats.set(stat.0, v);
        }
    }
    // L250
    stats
}

/// @pob Modules/CalcTools.lua:258-273 h=fd97bae8
/// Correct the tags on conversion with multipliers so they carry over correctly
/// → the converted tags (`modifiers[k]`, k = tag index).
pub fn getConvertedModTags(r#mod: &Mod, multiplier: f64, minionMods: bool) -> Vec<Tag> {
    // L259
    let mut modifiers: Vec<Tag> = Vec::new();
    // L260
    for value in &r#mod.tags {
        // a non-table tag (flags passed among the tags) raises on the first index below
        if value.r#type == TagType::LuaNonTable {
            lua_error!("attempt to index local 'value' (a number value)");
        }
        // L261
        if minionMods
            && value.r#type == TagType::ActorCondition
            && value.actor == Some(n!("parent"))
        {
            // L262
            modifiers.push(Tag {
                var: value.var,
                ..Tag::new(TagType::Condition)
            });
        } else if value.limitTotal {
            // L263
            // L264 LimitTotal can apply to 'per stat' or 'multiplier', so just copy the whole and update the limit
            // L265
            let mut copy = value.clone();
            // L266 (`limit` holds tonumber of a string limit; the product is a number)
            let limit = copy.limit.unwrap_or_else(|| {
                lua_error!("attempt to perform arithmetic on field 'limit' (a nil value)")
            });
            copy.limit = Some(limit * multiplier);
            if let Some(ex) = &mut copy.extra {
                ex.set(n!("limit"), Value::Nil);
                if ex.is_empty() {
                    copy.extra = None;
                }
            }
            // L267
            modifiers.push(copy);
        } else {
            // L269
            modifiers.push(value.clone());
        }
    }
    // L272
    modifiers
}

/// @pob Modules/CalcTools.lua:279-292 h=6b1f70e7
/// Get the gameId from the gemName which will be the same as the base gem for transfigured gems
pub fn getGameIdFromGemName(gemName: Option<&str>, dropVaal: bool) -> Option<Name> {
    // L280
    let Some(gemName) = gemName else {
        // L281
        return None;
    };
    let d = data();
    // L283
    let gemId = *d
        .gemForBaseName
        .get(pob_lua::string::lower(gemName).as_str())?;
    // L284 (`if not gemId then return end`, above)
    let gem = |id: Option<Name>| {
        id.and_then(|id| d.gems.get(&id).cloned())
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
    };
    // L285
    let gameId;
    // L286
    if dropVaal && gem(Some(gemId)).vaalGem {
        // L287
        gameId = gem(d.gemVaalGemIdForBaseGemId.get(&gemId).copied()).gameId;
    } else {
        // L289
        gameId = gem(Some(gemId)).gameId;
    }
    // L291
    Some(gameId)
}

/// @pob Modules/CalcTools.lua:299-303 h=68ee89c2
/// Use getGameIdFromGemName to get gameId from the gemName and passed in type. Return true if they're the same and not nil
/// Lua returns `gemNameId and typeId and gemNameId == typeId` (nil, false or true); callers use it as a condition.
pub fn isGemIdSame(gemName: &str, typeName: &str, dropVaal: bool) -> bool {
    // L300
    let gemNameId = getGameIdFromGemName(Some(gemName), dropVaal);
    // L301
    let typeId = getGameIdFromGemName(Some(typeName), dropVaal);
    // L302
    matches!((gemNameId, typeId), (Some(a), Some(b)) if a == b)
}
