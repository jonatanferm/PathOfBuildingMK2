//! @pob-file Modules/CalcActiveSkill.lua @16de4b82d
//! Modules/CalcActiveSkill.lua 1-934: active skills, their mod lists and minion skills.
//! `mergeLevelCache` (CalcActiveSkill.lua:21) becomes a thread-local memo keyed by (Arc pointer of the data mod, value bits) (§5.6).
//!
//! Owner: AS.
//!
//! Contract gaps worked around here (CCR-0040; each site is marked):
//! * `grantedEffect.minionList = { }` (Raise Spectre) and an absent `minionList` both load as an
//!   empty `Vec`; [`minionList_is_set`] tells them apart.
//! * `t_remove(skillModList, i)` / `skillModList[i]` (CalcActiveSkill.lua:805-869) need
//!   positional access to a ModList, which the ModStore API lacks ([`list_at`] / [`list_remove`]).
//! * `newSkill.actor = skill.actor` (CalcActiveSkill.lua:181) is a strong reference in Lua, but
//!   `ActiveSkill.actor` is a `Weak` (calc doc §3.3 rule 4). CCR-0113: `copyActiveSkill` keeps a
//!   foreign actor (one that is not `env.player`) alive in `newSkill.extra["actorKeep"]` as
//!   `Val::Obj(ActorRef)`, see [`actorKeepKey`].
//! * `activeSkill.summonSkill` (CalcActiveSkill.lua:89) is a `Weak` back-pointer (CCR-0135, calc
//!   doc §3.3 rule 4; a strong one closed the cycle summoning skill → `minion` → minion skill →
//!   `summonSkill`). `copyActiveSkill` keeps a foreign summoning skill (one whose actor is not
//!   `env.player`) alive in `newSkill.extra["summonSkillKeep"]`, see [`summonSkillKeepKey`].
#![allow(non_snake_case)]
// Lua statement shapes are kept (locals assigned per branch, `if … then … else … end`).
#![allow(
    clippy::needless_late_init,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::too_many_lines,
    clippy::cognitive_complexity
)]

use std::cell::RefCell;
use std::collections::HashMap;

use crate::classes::StoreMods;
use crate::model::GemInstanceRef;
use crate::modules::calc_tools::{
    buildSkillInstanceStats, canGrantedEffectSupportActiveSkill, levels_len, validateGemLevel,
};
use crate::modules::common::isValueInArray;
use crate::prelude::*;
use pob_gamedata::mod_tools::{compareModParams, createMod};
use pob_gamedata::{statMapLookup, NameSet, StatMapItem, WeaponTypeInfo};

// @pob Modules/CalcActiveSkill.lua:7-21 h=0d514f10
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_floor,m_min,m_max,bor,band,bnot,mergeLevelCache)

// ---- Rust-only plumbing ----------------------------------------------------------------------

/// `mergeLevelCache[mod][value]` key: the value as a Lua table key.
#[derive(Clone, PartialEq, Eq, Hash)]
enum LevelKey {
    /// A number (`-0` and `0` are the same key, as in Lua).
    Num(u64),
    Bool(bool),
    Str(AStr),
    /// A table value: by identity.
    Ptr(usize),
}

thread_local! {
    /// CalcActiveSkill.lua:21 `mergeLevelCache`: (mod identity, value) → the merged copy. The
    /// original `ModRef` is kept so its pointer cannot be reused while it is a key.
    static MERGE_LEVEL_CACHE: RefCell<HashMap<(usize, LevelKey), (ModRef, ModRef)>> =
        RefCell::new(HashMap::new());
}

/// Forget `mergeLevelCache` (a freshly booted PoB process; tests).
pub fn reset_merge_level_cache() {
    MERGE_LEVEL_CACHE.with(|c| c.borrow_mut().clear());
}

fn level_key(v: &Value) -> LevelKey {
    match v {
        Value::Num(n) => {
            if n.is_nan() {
                lua_error!("table index is NaN");
            }
            LevelKey::Num((n + 0.0).to_bits())
        }
        Value::Bool(b) => LevelKey::Bool(*b),
        Value::Str(s) => LevelKey::Str(s.clone()),
        Value::Mod(m) => LevelKey::Ptr(Arc::as_ptr(m) as *const () as usize),
        Value::Table(t) => LevelKey::Ptr(Arc::as_ptr(t) as *const () as usize),
        Value::JewelFunc(j) => LevelKey::Ptr(Arc::as_ptr(j) as *const () as usize),
        Value::Nil => lua_error!("table index is nil"),
    }
}

/// `skillModList[i]` (1-based) of a ModList. CCR-0040: the ModStore API has no positional read.
fn list_at(ml: &ModStore, i1: usize) -> Option<ModRef> {
    match &*ml.mods.borrow() {
        StoreMods::List { list, .. } => list.get(i1.wrapping_sub(1)).cloned(),
        StoreMods::DB(_) => lua_error!("attempt to index a ModDB as a list"),
    }
}

/// `t_remove(skillModList, i)` on a ModList (LuaJIT `table.remove`: shifts the rest down).
/// CCR-0040: the ModStore API has no positional remove; this keeps MS-A's name index sorted.
fn list_remove(ml: &ModStore, i1: usize) {
    match &mut *ml.mods.borrow_mut() {
        StoreMods::List { list, index } => {
            if i1 < 1 || i1 > list.len() {
                return;
            }
            let pos = (i1 - 1) as u32;
            let removed = list.remove(i1 - 1);
            if let Some(p) = index.get_mut(&removed.name) {
                p.retain(|x| *x != pos);
                if p.is_empty() {
                    index.remove(&removed.name);
                }
            }
            for p in index.values_mut() {
                for x in p.iter_mut() {
                    if *x > pos {
                        *x -= 1;
                    }
                }
            }
        }
        StoreMods::DB(_) => lua_error!("attempt to remove from a ModDB as a list"),
    }
}

/// `grantedEffect.minionList` is non-nil. CCR-0040: the schema loads `minionList = { }` and an
/// absent key alike as an empty `Vec`; at the pin the only empty lists are Raise Spectre's
/// (`RaiseSpectre`, `RaiseSpectreAltX`; checked against the dump by tests/AS.rs).
pub fn minionList_is_set(ge: &GrantedEffect) -> bool {
    !ge.minionList.is_empty() || matches!(ge.id.as_str(), "RaiseSpectre" | "RaiseSpectreAltX")
}

fn has_type(t: &SkillTypes, ty: SkillType::T) -> bool {
    t.with(|s| s.has(ty))
}

fn modflag(name: &str) -> Option<u32> {
    ModFlag::by_name(name)
}

/// `t[k]` → a Lua table key (strings only here: stat names, skill data keys).
fn val_key(v: &Val) -> Key {
    match v {
        Val::Str(s) => Key::S(Name::from(&**s)),
        Val::Num(n) => Key::from(*n),
        Val::Bool(b) => Key::B(*b),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!("unsupported table key of type {}", other.lua_type()),
    }
}

/// `t[k]` read (FUP2, FOLLOWUPS "FUP1" item 6): a nil or NaN key reads nil (`lj_tab_get` finds
/// nothing), where a store at it raises ([`val_key`]); the same rule as item.rs `tget`.
fn read_at(t: &Table, k: &Val) -> Val {
    match k {
        Val::Nil => Val::Nil,
        Val::Num(x) => Key::try_num(*x).map_or(Val::Nil, |k| t.get(k)),
        other => t.get(val_key(other)),
    }
}

fn value_key(v: &Value) -> Key {
    val_key(&Val::from_value(v))
}

fn src_or_err(activeEffect: &SkillEffect) -> GemInstanceRef {
    activeEffect
        .srcInstance
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'srcInstance' (a nil value)"))
}

/// `activeEffect.srcInstance and not (activeEffect.gemData and activeEffect.gemData.secondaryGrantedEffect)`.
fn src_not_secondary(activeEffect: &SkillEffect) -> Option<GemInstanceRef> {
    let src = activeEffect.srcInstance.get()?;
    if activeEffect
        .gemData
        .get()
        .is_some_and(|g| g.secondaryGrantedEffect.is_some())
    {
        return None;
    }
    Some(src)
}

/// `t[i]` on a Lua list indexed by a number (nil unless `i` is a positive integer in range).
fn index_f64<T>(v: &[T], i: f64) -> Option<&T> {
    if i >= 1.0 && i.fract() == 0.0 && i <= v.len() as f64 {
        v.get(i as usize - 1)
    } else {
        None
    }
}

fn u32_key(i: f64) -> Option<u32> {
    (i >= 0.0 && i.fract() == 0.0 && i <= f64::from(u32::MAX)).then_some(i as u32)
}

fn lstr(s: &str) -> Option<LStr> {
    Some(LStr::from(s))
}

/// `activeEffect[key] = activeEffect[key] + v` (CalcActiveSkill.lua:567).
fn effect_add(effect: &SkillEffect, key: &Value, v: f64) {
    match key.as_str() {
        Some("level") => effect.level.set(effect.level.get() + v),
        Some("quality") => effect.quality.set(effect.quality.get() + v),
        Some(k) => {
            let k = Name::from(k);
            let cur = effect.extra.get(k).num();
            effect.extra.set(k, cur + v);
        }
        None => lua_error!("table index is nil"),
    }
}

fn arith(v: &Value) -> f64 {
    match v {
        Value::Num(n) => *n,
        other => Val::from_value(other).num(),
    }
}

// ---- the port ----------------------------------------------------------------------------------

/// @pob Modules/CalcActiveSkill.lua:22-50 h=b49f410e
/// Merge level modifier with given mod list
pub(crate) fn mergeLevelMod(modList: &ModList, r#mod: &ModRef, value: Option<&Value>) {
    // L23
    let value = match value {
        Some(v) if v.truthy() => v,
        _ => {
            // L24
            modList.AddMod(r#mod.clone());
            // L25
            return;
        }
    };
    let key = (Arc::as_ptr(r#mod) as *const () as usize, level_key(value));
    // L27-29 (`mergeLevelCache[mod] = mergeLevelCache[mod] or { }`: the per-mod table is implicit)
    // L30
    let cached = MERGE_LEVEL_CACHE.with(|c| c.borrow().get(&key).map(|(_, m)| m.clone()));
    if let Some(cached) = cached {
        // L31
        modList.AddMod(cached);
    } else {
        // L32 (`elseif value`: always true here)
        // L33
        let mut newMod: Mod = (**r#mod).clone();
        // L34
        match &r#mod.value {
            Value::Table(t) => {
                // L35
                let mut nv: ValueTable = (**t).clone();
                // L36
                let inner = nv.get(n!("mod")).clone();
                if inner.truthy() {
                    // L37-38
                    match inner {
                        Value::Mod(m) => {
                            let mut mm: Mod = (*m).clone();
                            mm.value = value.clone();
                            nv.set(n!("mod"), Value::Mod(Arc::new(mm)));
                        }
                        Value::Table(it) => {
                            nv.set(
                                n!("mod"),
                                Value::Table(Arc::new(it.with(n!("value"), value.clone()))),
                            );
                        }
                        other => lua_error!(
                            "attempt to index field 'mod' (a {} value)",
                            other.lua_type()
                        ),
                    }
                } else {
                    // L40
                    nv.set(n!("value"), value.clone());
                }
                newMod.value = Value::Table(Arc::new(nv));
            }
            Value::Mod(m) => {
                // L35-40: a mod used as the value; a mod table has no `mod` key, so `.value` is set
                let mut mm: Mod = (**m).clone();
                mm.value = value.clone();
                newMod.value = Value::Mod(Arc::new(mm));
            }
            Value::JewelFunc(_) => {
                lua_error!("mergeLevelMod: jewel func values are never level-merged")
            }
            _ => {
                // L43
                newMod.value = value.clone();
            }
        }
        let newMod = Arc::new(newMod);
        // L45
        MERGE_LEVEL_CACHE.with(|c| c.borrow_mut().insert(key, (r#mod.clone(), newMod.clone())));
        // L46
        modList.AddMod(newMod);
    }
    // L47-49 (`else modList:AddMod(mod)`: unreachable, value is truthy here)
}

/// @pob Modules/CalcActiveSkill.lua:53-79 h=28ec5dde
/// Merge skill modifiers with given mod list
pub fn mergeSkillInstanceMods(
    env: &EnvRef,
    modList: &ModList,
    skillEffect: &SkillEffectRef,
    extraStats: Option<&Table>,
) {
    let _ = env;
    // L54
    validateGemLevel(&**skillEffect);
    // L55
    let grantedEffect = skillEffect.grantedEffect.get();
    // L56
    let stats = buildSkillInstanceStats(skillEffect, &grantedEffect);
    // L57
    if let Some(extraStats) = extraStats {
        if extraStats.truthy(1i64) {
            // L58 (pairs over a list: 1..n in order)
            for (_, stat) in extraStats.pairs() {
                // L59
                let k = stat.get(n!("key"));
                let v = read_at(&stats, &k).num_or(0.0) + stat.get(n!("value")).num();
                stats.set(val_key(&k), v);
            }
        }
    }
    // L62 LUA-ORDER: pairs over the string-keyed stats table (PoB's order is per-process;
    // the AddMod order into the skill mod list follows it)
    for (stat, statValue) in stats.pairs() {
        let Some(statName) = stat.name() else {
            continue;
        };
        // L63
        let map = statMapLookup(&grantedEffect, statName);
        // L64
        if let Some(map) = map {
            // L65 Some mods need different scalars for different stats, but the same value.  Putting them in a group allows this
            // L66
            for modOrGroup in &map.items {
                match modOrGroup {
                    // L67 Found a mod, since all mods have names
                    StatMapItem::Mod(r#mod) => {
                        // L68-69
                        let v = match &map.value {
                            Some(v) if v.truthy() => v.clone(),
                            _ => Value::Num(
                                statValue.num() * map.mult.unwrap_or(1.0) / map.div.unwrap_or(1.0)
                                    + map.base.unwrap_or(0.0),
                            ),
                        };
                        mergeLevelMod(modList, r#mod, Some(&v));
                    }
                    StatMapItem::Group {
                        mods,
                        value,
                        mult,
                        div,
                        base,
                    } => {
                        // L71
                        for r#mod in mods {
                            // L72
                            let v = match value {
                                Some(v) if v.truthy() => v.clone(),
                                _ => Value::Num(
                                    statValue.num() * mult.unwrap_or(1.0) / div.unwrap_or(1.0)
                                        + base.unwrap_or(0.0),
                                ),
                            };
                            mergeLevelMod(modList, r#mod, Some(&v));
                        }
                    }
                }
            }
        }
    }
    // L78
    modList.AddList(&grantedEffect.baseMods);
}

/// @pob Modules/CalcActiveSkill.lua:83-164 h=6a08350d
/// `calcs.createActiveSkill(activeEffect, supportList, actor, socketGroup, summonSkill)`.
/// Create an active skill using the given active gem and list of support gems
/// It will determine the base flag set, and check which of the support gems can support this skill
pub fn createActiveSkill(
    activeEffect: SkillEffectRef,
    supportList: &Shared<Vec<SkillEffectRef>>,
    actor: ActorWeak,
    socketGroup: Option<SocketGroupRef>,
    summonSkill: Option<ActiveSkillRef>,
) -> ActiveSkillRef {
    // L84-92 ---@class ActiveSkill
    let activeSkill = ActiveSkill::new(activeEffect.clone(), supportList.clone(), actor);
    // CCR-0135: a Weak back-pointer (the summoning skill owns the minion that owns this skill).
    activeSkill.summonSkill.set(summonSkill.as_ref().map(Rc::downgrade));
    activeSkill.socketGroup.set(socketGroup);

    // L95
    let activeGrantedEffect = activeEffect.grantedEffect.get();

    // L97 Initialise skill types
    // L98
    activeSkill
        .skillTypes
        .set(Shared::new(activeGrantedEffect.skillTypes));
    // L99
    if let Some(mst) = activeGrantedEffect.minionSkillTypes {
        // L100
        activeSkill.minionSkillTypes.set(Some(Shared::new(mst)));
    }

    // L103 Initialise skill flag set ('attack', 'projectile', etc)
    // L104 ---@class SkillFlags
    // L105 LUA-ORDER: copyTable of a string-keyed set (PoB's key order is per-process); sorted here
    let skillFlags = Table::new();
    {
        let mut flags: Vec<Name> = activeGrantedEffect.baseFlags.iter().copied().collect();
        flags.sort_by(|a, b| a.as_str().cmp(b.as_str()));
        for f in flags {
            skillFlags.set(f, true);
        }
    }
    // L106
    activeSkill.skillFlags.set(skillFlags.clone());
    // L107
    {
        let st = activeSkill.skillTypes.get();
        let hit = lor(skillFlags.get(n!("hit")), || {
            if has_type(&st, SkillType::Attack)
                || has_type(&st, SkillType::Damage)
                || has_type(&st, SkillType::Projectile)
            {
                Val::Bool(true)
            } else {
                Val::Nil
            }
        });
        skillFlags.set(n!("hit"), hit);
    }

    // L109 Process support skills
    // L110
    activeSkill
        .effectList
        .set(Shared::new(vec![activeEffect.clone()]));
    // L111
    let mut rejectedSupportsIndices: Vec<Option<usize>> = Vec::new();

    // L113
    for_ipairs!((index, supportEffect) in supportList => {
        // L114 Pass 1: Add skill types from compatible supports
        // L115
        let sge = supportEffect.grantedEffect.get();
        if sge.support {
            // L116
            if canGrantedEffectSupportActiveSkill(&sge, &activeSkill, None, supportEffect.appliesToGrantedSkills.get()) {
                // L117
                for skillType in &sge.addSkillTypes {
                    // L118
                    activeSkill.skillTypes.get().with_mut(|s| s.set(*skillType, true));
                }
            }
        } else {
            // L122 LUA-QUIRK: the `else` belongs to `if support`, so the list collects the
            // non-support entries (the retry loop below can never add one of them)
            rejectedSupportsIndices.push(Some(index));
        }
    });

    // L126 loop over rejected supports until none are added.
    // L127 Makes sure that all skillType flags that should be added are added regardless of support gem order in group
    // L128
    let mut notAddedNewSupport;
    // L129
    loop {
        // L130
        notAddedNewSupport = true;
        // L131
        pob_lua::ipairs::ipairs_live(
            &mut rejectedSupportsIndices,
            |index, supportEffectIndex, list| {
                // L132
                let Some(supportEffect) = supportList.get(supportEffectIndex) else {
                    lua_error!("attempt to index local 'supportEffect' (a nil value)");
                };
                // L133
                let sge = supportEffect.grantedEffect.get();
                if sge.support {
                    // L134
                    if canGrantedEffectSupportActiveSkill(
                        &sge,
                        &activeSkill,
                        None,
                        supportEffect.appliesToGrantedSkills.get(),
                    ) {
                        // L135
                        notAddedNewSupport = false;
                        // L136
                        list[index - 1] = None;
                        // L137
                        for skillType in &sge.addSkillTypes {
                            // L138
                            activeSkill
                                .skillTypes
                                .get()
                                .with_mut(|s| s.set(*skillType, true));
                        }
                    }
                }
            },
        );
        // L143
        if notAddedNewSupport {
            break;
        }
    }

    // L145
    for_ipairs!((_i, supportEffect) in supportList => {
        // L146 Pass 2: Add all compatible supports
        // L147
        let sge = supportEffect.grantedEffect.get();
        if sge.support {
            // L148
            if canGrantedEffectSupportActiveSkill(&sge, &activeSkill, None, supportEffect.appliesToGrantedSkills.get()) {
                // L149
                activeSkill.effectList.get().push(supportEffect.clone());
                // L150
                if let (Some(isSupporting), Some(src)) = (supportEffect.isSupporting.get(), activeEffect.srcInstance.get()) {
                    // L151
                    isSupporting.with_mut(|s| {
                        s.insert(&src);
                    });
                }
                // L153
                if let (Some(addFlags), None) = (&sge.addFlags, &summonSkill) {
                    // L154 Support skill adds flags to supported skills (eg. Remote Mine adds 'mine')
                    // L155 (pairs over a set: the order is not observable)
                    for k in addFlags.iter() {
                        // L156
                        skillFlags.set(*k, true);
                    }
                }
            }
        }
    });

    // L163
    activeSkill
}

/// CCR-0113: the `newSkill.extra` key under which [`copyActiveSkill`] keeps the copied skill's
/// foreign actor alive (`Val::Obj(ActorRef)`). Rust-only; PoB has no such key.
pub fn actorKeepKey() -> Name {
    n!("actorKeep")
}

/// CCR-0135: the `newSkill.extra` key under which [`copyActiveSkill`] keeps the copied skill's
/// foreign summoning skill alive (`Val::Obj(ActiveSkillRef)`). Rust-only; PoB has no such key.
pub fn summonSkillKeepKey() -> Name {
    n!("summonSkillKeep")
}

/// @pob Modules/CalcActiveSkill.lua:167-192 h=0b95f5c3
/// → (newSkill, newEnv).
/// Copy an Active Skill
pub fn copyActiveSkill(
    env: &EnvRef,
    mode: Mode,
    skill: &ActiveSkillRef,
) -> (ActiveSkillRef, EnvRef) {
    let sae = skill.activeEffect.get();
    // L168-172
    let activeEffect =
        SkillEffect::new(sae.grantedEffect.get(), sae.level.get(), sae.quality.get());

    // L174
    if let Some(src) = sae.srcInstance.get() {
        // L175
        activeEffect.level.set(src.level.get());
        // L176
        activeEffect.quality.set(src.quality.get());
        // L177
        activeEffect.srcInstance.set(Some(src.clone()));
        // L178
        activeEffect.gemData.set(src.gemData.get());
    }

    // L181
    let newSkill = createActiveSkill(
        activeEffect,
        &skill.supportList.get(),
        skill.actor.get(),
        skill.socketGroup.get(),
        skill.summonSkill(),
    );
    // CCR-0113: Lua's `skill.actor` reference (L181) is strong. The source skill may come from
    // another env (a `GlobalCache` entry's skill, CalcMirages.lua:134/137) whose only owner is
    // that cache entry; `cacheData` can overwrite the entry (CalcPerform.lua:3989) while the copy
    // lives on as `env.player.mainSkill`, and the `Weak` would then die. Keep the foreign actor
    // alive on the copy. An actor that is `env.player` is owned by `env` already, and a strong
    // reference to it would close an `Rc` cycle (`postCalcFunc` stores the copy in
    // `env.player.mainSkill`, CalcMirages.lua:155/232).
    if let Some(actor) = skill.actor() {
        if !Rc::ptr_eq(&actor, &env.player) {
            newSkill.extra.set(actorKeepKey(), Val::Obj(actor));
        }
    }
    // CCR-0135: likewise `skill.summonSkill` (L181) is strong in Lua and a `Weak` here. A
    // summoning skill from another env (the source is a cached env's minion skill) is owned only
    // by that env; keep it alive on the copy. It never reaches the copy, so this closes no cycle.
    // One whose actor is `env.player` is owned by `env` already.
    if let Some(summon) = skill.summonSkill() {
        if !summon.actor().is_some_and(|a| Rc::ptr_eq(&a, &env.player)) {
            newSkill.extra.set(summonSkillKeepKey(), Val::Obj(summon));
        }
    }
    // L182
    let (newEnv, _, _, _) =
        crate::modules::calc_setup::initEnv(&env.build, mode, Some(env.r#override.clone()), None);
    // L183
    buildActiveSkillModList(&newEnv, &newSkill);
    // L184
    newSkill
        .skillModList
        .set(ModStore::ModList(Some(newSkill.baseSkillModList.get())));
    // L185
    if let Some(minion) = newSkill.minion.get() {
        // L186
        minion.modDB.set(ModStore::ModDB(None));
        // L187
        minion.modDB.get().actor.set(Rc::downgrade(&minion));
        // L188
        createMinionSkills(env, &newSkill);
        // L189
        let name = minion
            .mainSkill
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
            .activeEffect
            .get()
            .grantedEffect
            .get()
            .name;
        newSkill.skillPartName.set(lstr(name.as_str()));
    }
    // L191
    (newSkill, newEnv)
}

/// @pob Modules/CalcActiveSkill.lua:195-226 h=4e145083
/// → the weapon flags, or nil when the weapon type is unknown (second result: `info`).
/// Get weapon flags and info for given weapon
///
/// CCR-0040: `weaponTypes` is the list of weapon-type sets (CalcActiveSkill.lua:296-300), not one
/// set as U0's stub had it (the function is file-local; no other caller).
pub(crate) fn getWeaponFlags(
    env: &EnvRef,
    weaponData: &Table,
    weaponTypes: Option<&[NameSet]>,
) -> (Option<u32>, Option<WeaponTypeInfo>) {
    let wtype = weaponData.get(n!("type"));
    let wname = wtype.str().map(|s| Name::from(&*s));
    // L196
    let info = wname.and_then(|t| env.data.weaponTypeInfo.get(&t).cloned());
    // L197
    let Some(info) = info else {
        // L198
        return (None, None);
    };
    // L200
    if let Some(weaponTypes) = weaponTypes {
        // L201
        for types in weaponTypes {
            let has = |k: &str| types.contains(&Name::from(k));
            // L202-203
            if !wname.is_some_and(|t| types.contains(&t))
                && (!weaponData.truthy(n!("countsAsAll1H"))
                    || !(has("Claw")
                        || has("Dagger")
                        || has("One Handed Axe")
                        || has("One Handed Mace")
                        || has("One Handed Sword")))
            {
                // L204
                return (None, Some(info));
            }
        }
    }
    let bor_req = |f: Option<u32>, g: u32| -> u32 {
        f.unwrap_or_else(|| lua_error!("bad argument #1 to 'bor' (number expected, got nil)")) | g
    };
    // L208
    let mut flags = modflag(info.flag.as_str());
    // L209
    if weaponData.truthy(n!("countsAsAll1H")) {
        // L210
        flags =
            Some(ModFlag::Axe | ModFlag::Claw | ModFlag::Dagger | ModFlag::Mace | ModFlag::Sword);
    }
    // L212
    if !(wname.is_some_and(|t| t.as_str() == "None")) {
        // L213
        flags = Some(bor_req(flags, ModFlag::Weapon));
        // L214
        if info.oneHand {
            // L215
            flags = Some(bor_req(flags, ModFlag::Weapon1H));
        } else {
            // L217
            flags = Some(bor_req(flags, ModFlag::Weapon2H));
        }
        // L219
        if info.melee {
            // L220
            flags = Some(bor_req(flags, ModFlag::WeaponMelee));
        } else {
            // L222
            flags = Some(bor_req(flags, ModFlag::WeaponRanged));
        }
    }
    // L225
    (flags, Some(info))
}

/// @pob Modules/CalcActiveSkill.lua:229-875 h=35d34a41
/// Build list of modifiers for given active skill
#[allow(unused_assignments)] // `local effectiveRange = 0` is overwritten on both branches (L234)
pub fn buildActiveSkillModList(env: &EnvRef, activeSkill: &ActiveSkillRef) {
    // L230
    let skillTypes = activeSkill.skillTypes.get();
    // L231
    let skillFlags = activeSkill.skillFlags.get();
    // L232
    let activeEffect = activeSkill.activeEffect.get();
    // L233
    let activeGrantedEffect = activeEffect.grantedEffect.get();
    // L234
    let mut effectiveRange = Val::Num(0.0);
    let actor = || {
        activeSkill
            .actor()
            .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"))
    };
    let isMainSkill = || {
        env.player
            .mainSkill
            .get()
            .is_some_and(|m| Rc::ptr_eq(&m, activeSkill))
    };
    let summon = activeSkill.summonSkill();

    // L236 Set mode flags
    // L237
    if env.mode_buffs.get() {
        // L238
        skillFlags.set(n!("buffs"), true);
    }
    // L240
    if env.mode_combat.get() {
        // L241
        skillFlags.set(n!("combat"), true);
    }
    // L243
    if env.mode_effective.get() {
        // L244
        skillFlags.set(n!("effective"), true);
    }

    // L247 Handle multipart skills
    // L248
    let activeGemParts = &activeGrantedEffect.parts;
    // L249
    if activeGemParts.len() > 1 {
        let n_parts = activeGemParts.len() as f64;
        // L250
        if env.mode == Mode::CALCS && isMainSkill() {
            let src = src_or_err(&activeEffect);
            // L251
            src.skillPartCalcs.set(Some(m_min(
                n_parts,
                src.skillPartCalcs.get().unwrap_or(1.0),
            )));
            // L252
            activeSkill.skillPart.set(src.skillPartCalcs.get());
        } else {
            let src = src_or_err(&activeEffect);
            // L254
            src.skillPart
                .set(Some(m_min(n_parts, src.skillPart.get().unwrap_or(1.0))));
            // L255
            activeSkill.skillPart.set(src.skillPart.get());
        }
        // L257
        let part = activeSkill
            .skillPart
            .get()
            .and_then(|p| index_f64(activeGemParts, p))
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'pairs' (table expected, got nil)"));
        // L258 (`name` is a string: neither branch)
        for (k, v) in &part.flags {
            // L259
            if *v {
                // L260
                skillFlags.set(*k, true);
            } else {
                // L261-262
                skillFlags.set(*k, Val::Nil);
            }
        }
        // L265
        activeSkill.skillPartName.set(lstr(&part.name));
        // L266
        skillFlags.set(n!("multiPart"), activeGemParts.len() > 1);
    } else if let Some(src) = src_not_secondary(&activeEffect) {
        // L267
        // L268
        src.skillPart.set(None);
        // L269
        src.skillPartCalcs.set(None);
    }

    // L272
    if (has_type(&skillTypes, SkillType::RequiresShield) || skillFlags.truthy(n!("shieldAttack")))
        && summon.is_none()
        && actor()
            .itemList()
            .with(|m| m.get(&n!("Weapon 2")).cloned())
            .map_or(true, |it| it.r#type.get() != n!("Shield"))
    {
        // L273 Skill requires a shield to be equipped
        // L274
        skillFlags.set(n!("disable"), true);
        // L275
        activeSkill
            .disableReason
            .set(lstr("This skill requires a Shield"));
    }

    // L278
    if has_type(&skillTypes, SkillType::RequiresStaff)
        && summon.is_none()
        && actor()
            .itemList()
            .with(|m| m.get(&n!("Weapon 1")).cloned())
            .map_or(true, |it| it.r#type.get() != n!("Staff"))
    {
        // L279 Skill requires a staff to be equipped
        // L280
        skillFlags.set(n!("disable"), true);
        // L281
        activeSkill
            .disableReason
            .set(lstr("This skill requires a Staff"));
    }

    // L284
    if skillFlags.truthy(n!("shieldAttack")) {
        // L285 Special handling for Spectral Shield Throw
        // L286
        skillFlags.set(n!("weapon2Attack"), true);
        // L287
        activeSkill.weapon2Flags.set(Some(0));
    } else {
        // L289 Set weapon flags
        // L290
        if let Some(sg) = activeSkill.socketGroup.get() {
            if skillFlags.truthy(n!("forceSourceWeapon")) && sg.sourceItem.get().is_some() {
                // L291 Some item-granted attacks must use the weapon that grants the skill
                // L292
                let sourceSlot = sg
                    .slot
                    .get()
                    .map_or(String::new(), |s| s.as_str().to_owned());
                // L293 (`:match("^Weapon 1") ~= nil`)
                skillFlags.set(
                    n!("forceMainHand"),
                    pob_lua::pattern::r#match(&sourceSlot, "^Weapon 1", None).is_some(),
                );
                // L294
                skillFlags.set(
                    n!("forceOffHand"),
                    pob_lua::pattern::r#match(&sourceSlot, "^Weapon 2", None).is_some(),
                );
            }
        }
        // L296
        let mut weaponTypes: Vec<NameSet> = Vec::new();
        if let Some(wt) = &activeGrantedEffect.weaponTypes {
            weaponTypes.push(wt.clone());
        }
        // L297 (pairs over a list: in order)
        for skillEffect in activeSkill.effectList.get().with(Clone::clone) {
            let ge = skillEffect.grantedEffect.get();
            // L298
            if ge.support {
                if let Some(wt) = &ge.weaponTypes {
                    // L299
                    weaponTypes.push(wt.clone());
                }
            }
        }
        // L302
        let mut weapon1Flags: Option<u32> = None;
        let mut weapon1Info: Option<WeaponTypeInfo> = None;
        // L303
        if !skillFlags.truthy(n!("forceOffHand")) {
            // L304
            (weapon1Flags, weapon1Info) = getWeaponFlags(
                env,
                &actor().weaponData1.get(),
                Some(weaponTypes.as_slice()),
            );
        }
        // L306
        if weapon1Flags.is_none() && summon.is_some() {
            // L307 Minion skills seem to ignore weapon types
            // L308
            let none = env
                .data
                .weaponTypeInfo
                .get(&n!("None"))
                .cloned()
                .unwrap_or_else(|| lua_error!("attempt to index field 'None' (a nil value)"));
            weapon1Flags = modflag(none.flag.as_str());
            weapon1Info = Some(none);
        }
        // L310
        if let Some(w1f) = weapon1Flags {
            // L311
            if skillFlags.truthy(n!("attack")) || skillFlags.truthy(n!("dotFromAttack")) {
                // L312
                activeSkill.weapon1Flags.set(Some(w1f));
                // L313
                skillFlags.set(n!("weapon1Attack"), true);
                let info = weapon1Info.as_ref().expect("info with flags");
                // L314
                if info.melee && skillFlags.truthy(n!("melee")) {
                    // L315
                    skillFlags.set(n!("projectile"), Val::Nil);
                } else if !info.melee && skillFlags.truthy(n!("projectile")) {
                    // L316
                    // L317
                    skillFlags.set(n!("melee"), Val::Nil);
                }
            }
        } else if (has_type(&skillTypes, SkillType::DualWieldOnly)
            || skillFlags.truthy(n!("forceMainHand"))
            || weapon1Info.is_some())
            && summon.is_none()
        {
            // L320
            // L321 Skill requires a compatible main hand weapon
            // L322
            skillFlags.set(n!("disable"), true);
            // L323
            activeSkill
                .disableReason
                .set(lstr("Main Hand weapon is not usable with this skill"));
        }
        // L325
        if !skillFlags.truthy(n!("forceMainHand")) {
            let a = actor();
            // L326
            let (weapon2Flags, weapon2Info) =
                getWeaponFlags(env, &a.weaponData2.get(), Some(weaponTypes.as_slice()));
            // L327
            if let Some(w2f) = weapon2Flags {
                let wd1 = a.weaponData1.get();
                let wd2 = a.weaponData2.get();
                // L328
                if has_type(&skillTypes, SkillType::DualWieldRequiresDifferentTypes)
                    && wd1.get(n!("type")).raw_eq(&wd2.get(n!("type")))
                    && !(wd2.truthy(n!("countsAsAll1H")) || wd1.truthy(n!("countsAsAll1H")))
                {
                    // L329 Skill requires a different compatible off hand weapon to main hand weapon
                    // L330
                    skillFlags.set(n!("disable"), true);
                    // L331
                    if activeSkill.disableReason.get().is_none() {
                        activeSkill
                            .disableReason
                            .set(lstr("Weapon Types Need to be Different"));
                    }
                } else if skillFlags.truthy(n!("attack")) || skillFlags.truthy(n!("dotFromAttack"))
                {
                    // L332
                    // L333
                    activeSkill.weapon2Flags.set(Some(w2f));
                    // L334
                    skillFlags.set(n!("weapon2Attack"), true);
                }
            } else if (has_type(&skillTypes, SkillType::DualWieldOnly) || weapon2Info.is_some())
                && summon.is_none()
            {
                // L336
                // L337 Skill requires a compatible off hand weapon
                // L338
                skillFlags.set(n!("disable"), true);
                // L339
                if activeSkill.disableReason.get().is_none() {
                    activeSkill
                        .disableReason
                        .set(lstr("Off Hand weapon is not usable with this skill"));
                }
            } else if skillFlags.truthy(n!("disable")) {
                // L340
                // L341 Neither weapon is compatible
                // L342
                activeSkill
                    .disableReason
                    .set(lstr("No usable weapon equipped"));
            }
        }
        // L345
        if skillFlags.truthy(n!("attack")) {
            // L346
            let both = land(skillFlags.get(n!("weapon1Attack")), || {
                skillFlags.get(n!("weapon2Attack"))
            });
            skillFlags.set(n!("bothWeaponAttack"), both);
        }
    }

    // L350 Apply stat-map flagged skill flags.
    // L351 LUA-ORDER: pairs over the stats table (setting flags: the order is not observable)
    for (stat, statValue) in buildSkillInstanceStats(&activeEffect, &activeGrantedEffect).pairs() {
        let Some(statName) = stat.name() else {
            continue;
        };
        // L352
        let map = statMapLookup(&activeGrantedEffect, statName);
        // L353
        let nonzero = !matches!(statValue, Val::Num(x) if x == 0.0);
        if nonzero {
            if let Some(skillFlag) = map.as_ref().and_then(|m| m.skillFlag) {
                // L354
                skillFlags.set(skillFlag, true);
            }
        }
    }
    // L357 Build skill mod flag set
    // L358
    let mut skillModFlags: u32 = 0;
    // L359
    if skillFlags.truthy(n!("hit")) {
        // L360
        skillModFlags |= ModFlag::Hit;
    }
    // L362
    if skillFlags.truthy(n!("attack")) {
        // L363
        skillModFlags |= ModFlag::Attack;
    } else {
        // L365
        skillModFlags |= ModFlag::Cast;
        // L366
        if skillFlags.truthy(n!("spell")) {
            // L367
            skillModFlags |= ModFlag::Spell;
        }
    }
    // L370
    if skillFlags.truthy(n!("melee")) {
        // L371
        skillModFlags |= ModFlag::Melee;
    } else if skillFlags.truthy(n!("projectile")) {
        // L372
        // L373
        skillModFlags |= ModFlag::Projectile;
        // L374
        skillFlags.set(n!("chaining"), true);
    }
    // L376
    if skillFlags.truthy(n!("area")) {
        // L377
        skillModFlags |= ModFlag::Area;
    }

    // L380 Build skill keyword flag set
    // L381
    let mut skillKeywordFlags: u32 = 0;
    // L382
    if skillFlags.truthy(n!("hit")) {
        // L383
        skillKeywordFlags |= KeywordFlag::Hit;
    }
    // L385-417
    for (st, kf) in [
        (SkillType::Aura, KeywordFlag::Aura),
        (SkillType::AppliesCurse, KeywordFlag::Curse),
        (SkillType::Warcry, KeywordFlag::Warcry),
        (SkillType::Movement, KeywordFlag::Movement),
        (SkillType::Vaal, KeywordFlag::Vaal),
        (SkillType::Lightning, KeywordFlag::Lightning),
        (SkillType::Cold, KeywordFlag::Cold),
        (SkillType::Fire, KeywordFlag::Fire),
        (SkillType::Chaos, KeywordFlag::Chaos),
        (SkillType::Physical, KeywordFlag::Physical),
    ] {
        if has_type(&skillTypes, st) {
            skillKeywordFlags |= kf;
        }
    }
    // L415
    if skillFlags.truthy(n!("weapon1Attack")) {
        let w1 = activeSkill
            .weapon1Flags
            .get()
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'band' (number expected, got nil)"));
        if w1 & ModFlag::Bow != 0 {
            // L416
            skillKeywordFlags |= KeywordFlag::Bow;
        }
    }
    // L418
    if skillFlags.truthy(n!("brand")) {
        // L419
        skillKeywordFlags |= KeywordFlag::Brand;
    }
    // L421
    if skillFlags.truthy(n!("arrow")) {
        // L422
        skillKeywordFlags |= KeywordFlag::Arrow;
    }
    // L424
    if skillFlags.truthy(n!("totem")) {
        // L425
        skillKeywordFlags |= KeywordFlag::Totem;
    } else if skillFlags.truthy(n!("trap")) {
        // L426
        // L427
        skillKeywordFlags |= KeywordFlag::Trap;
    } else if skillFlags.truthy(n!("mine")) {
        // L428
        // L429
        skillKeywordFlags |= KeywordFlag::Mine;
    } else if !has_type(&skillTypes, SkillType::Triggered) {
        // L430
        // L431
        skillFlags.set(n!("selfCast"), true);
    }
    // L433
    if has_type(&skillTypes, SkillType::Attack) {
        // L434
        skillKeywordFlags |= KeywordFlag::Attack;
    }
    // L436
    if has_type(&skillTypes, SkillType::Spell) && !skillFlags.truthy(n!("cast")) {
        // L437
        skillKeywordFlags |= KeywordFlag::Spell;
    }

    // L440 Get skill totem ID for totem skills
    // L441 This is used to calculate totem life
    // L442
    if skillFlags.truthy(n!("totem")) {
        // L443
        activeSkill
            .skillTotemId
            .set(activeGrantedEffect.skillTotemId);
        // L444
        if activeSkill.skillTotemId.get().is_none() {
            // L445
            if activeGrantedEffect.color == Some(2) {
                // L446
                activeSkill.skillTotemId.set(Some(2.0));
            } else if activeGrantedEffect.color == Some(3) {
                // L447
                // L448
                activeSkill.skillTotemId.set(Some(3.0));
            } else {
                // L450
                activeSkill.skillTotemId.set(Some(1.0));
            }
        }
    }

    // L455 Calculate Distance for meleeDistance or projectileDistance (for melee proximity, e.g. Impact)
    // L456
    if skillFlags.truthy(n!("melee")) {
        // L457
        effectiveRange = lor(env.configInput.get(n!("meleeDistance")), || {
            env.configPlaceholder.get(n!("meleeDistance"))
        });
    } else {
        // L459
        effectiveRange = lor(env.configInput.get(n!("projectileDistance")), || {
            env.configPlaceholder.get(n!("projectileDistance"))
        });
    }

    // L462 Build config structure for modifier searches
    // L463 ---@class ModCfg
    // L464-478
    let gemCfg = activeSkill.activeEffect.get().gemCfg.get();
    let skillCfg = ModCfg::new();
    skillCfg.flags.set(
        skillModFlags
            | activeSkill
                .weapon1Flags
                .get()
                .or(activeSkill.weapon2Flags.get())
                .unwrap_or(0),
    );
    skillCfg.keywordFlags.set(skillKeywordFlags);
    // This allows modifiers that target specific skills to also apply to their Vaal counterpart
    skillCfg.skillName.set(Some(Name::from(
        pob_lua::pattern::gsub(activeGrantedEffect.name.as_str(), "^Vaal ", "", None)
            .0
            .as_str(),
    )));
    skillCfg.summonSkillName.set(
        summon
            .as_ref()
            .map(|s| s.activeEffect.get().grantedEffect.get().name),
    );
    skillCfg.skillGem.set(activeEffect.gemData.get());
    skillCfg
        .skillGrantedEffect
        .set(Some(activeGrantedEffect.clone()));
    skillCfg.skillPart.set(activeSkill.skillPart.get());
    skillCfg.skillTypes.set(Some(activeSkill.skillTypes.get()));
    skillCfg.skillCond.set(Some(Table::new()));
    // `env.mode_effective and effectiveRange` (false → nil: every reader tests truthiness)
    skillCfg.skillDist.set(if env.mode_effective.get() {
        effectiveRange.opt_num()
    } else {
        None
    });
    skillCfg.slotName.set(
        activeSkill
            .slotName
            .get()
            .or_else(|| gemCfg.as_ref().and_then(|c| c.slotName.get())),
    );
    skillCfg
        .socketColor
        .set(gemCfg.as_ref().and_then(|c| c.socketColor.get()));
    skillCfg
        .socketNum
        .set(gemCfg.as_ref().and_then(|c| c.socketNum.get()));
    activeSkill.skillCfg.set(skillCfg.clone());

    // L480
    if skillFlags.truthy(n!("weapon1Attack")) {
        // L481
        let w1 = skillCfg.copyTable(true);
        // L482
        let cond = Table::with_meta(IndexMeta::Table(
            skillCfg.skillCond.get().expect("skillCond set above"),
        ));
        cond.set(n!("MainHandAttack"), true);
        w1.skillCond.set(Some(cond));
        // L483
        w1.flags.set(
            skillModFlags
                | activeSkill.weapon1Flags.get().unwrap_or_else(|| {
                    lua_error!("bad argument #2 to 'bor' (number expected, got nil)")
                }),
        );
        activeSkill.weapon1Cfg.set(Some(w1));
    }
    // L485
    if skillFlags.truthy(n!("weapon2Attack")) {
        // L486
        let w2 = skillCfg.copyTable(true);
        // L487
        let cond = Table::with_meta(IndexMeta::Table(
            skillCfg.skillCond.get().expect("skillCond set above"),
        ));
        cond.set(n!("OffHandAttack"), true);
        w2.skillCond.set(Some(cond));
        // L488
        w2.flags.set(
            skillModFlags
                | activeSkill.weapon2Flags.get().unwrap_or_else(|| {
                    lua_error!("bad argument #2 to 'bor' (number expected, got nil)")
                }),
        );
        activeSkill.weapon2Cfg.set(Some(w2));
    }

    // L491 Initialise skill modifier list
    // L492
    let skillModList = ModStore::ModList(Some(actor().modDB.get()));
    // L493
    activeSkill.skillModList.set(skillModList.clone());
    // L494
    activeSkill.baseSkillModList.set(skillModList.clone());

    // L496 The damage fixup stat applies x% less base Attack Damage and x% more base Attack Speed as confirmed by Openarl Jan 4th 2024
    // L497 Implemented in this manner as the stat exists on the minion not the skills
    // L498
    if let Some(damageFixup) = activeSkill
        .actor()
        .and_then(|a| a.minionData.get())
        .and_then(|m| m.damageFixup)
    {
        // L499
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            -100.0 * damageFixup,
            "Damage Fixup",
            ModFlag::Attack,
            0,
            &[],
        );
        // L500
        skillModList.NewMod(
            n!("Speed"),
            ModType::MORE,
            100.0 * damageFixup,
            "Damage Fixup",
            ModFlag::Attack,
            0,
            &[],
        );
    }

    // L503 Mods which apply curses are not disabled by Gruthkul's Pelt
    // L504
    let curseApplicationSkill = activeSkill
        .socketGroup
        .get()
        .is_some_and(|g| g.sourceItem.get().is_some())
        && activeSkill.skillFlags.get().truthy(n!("curse"))
        && activeSkill
            .activeEffect
            .get()
            .srcInstance
            .get()
            .is_some_and(|s| s.noSupports.get() && s.triggered.get());
    // L505
    if skillModList.Flag(Some(&skillCfg), nm!["DisableSkill"])
        && !(skillModList.Flag(Some(&skillCfg), nm!["EnableSkill"])
            || (curseApplicationSkill
                && skillModList.Flag(None, nm!["ForceEnableCurseApplication"])))
    {
        // L506
        skillFlags.set(n!("disable"), true);
        // L507
        activeSkill
            .disableReason
            .set(lstr("Skills of this type are disabled"));
    }

    // L510
    if skillFlags.truthy(n!("disable")) {
        // L511
        skillFlags.wipe();
        // L512
        skillFlags.set(n!("disable"), true);
        // L513
        validateGemLevel(&*activeEffect);
        // L514
        activeEffect.grantedEffectLevel.set(
            activeGrantedEffect
                .levels
                .get(activeEffect.level.get())
                .cloned(),
        );
        // L515
        return;
    }

    let skillData = activeSkill.skillData.get();
    // L518 Add support gem modifiers to skill mod list
    // L519 (pairs over a list: in order)
    for skillEffect in activeSkill.effectList.get().with(Clone::clone) {
        let sge = skillEffect.grantedEffect.get();
        // L520
        if sge.support {
            // L521
            mergeSkillInstanceMods(env, &skillModList, &skillEffect, None);
            // L522
            let level = sge
                .levels
                .get(skillEffect.level.get())
                .cloned()
                .unwrap_or_else(|| lua_error!("attempt to index local 'level' (a nil value)"));
            // L523
            let manaMultiplier = level.get(n!("manaMultiplier"));
            if manaMultiplier.truthy() {
                // L524
                skillModList.NewMod(
                    n!("SupportManaMultiplier"),
                    ModType::MORE,
                    manaMultiplier.clone(),
                    &sge.modSource,
                    0,
                    0,
                    &[],
                );
            }
            // L526
            let mrp = level.get(n!("manaReservationPercent"));
            if mrp.truthy() {
                // L527
                skillData.set(n!("manaReservationPercent"), Val::from_value(mrp));
            }
            // L529 Handle multiple triggers situation and if triggered by a trigger skill save a reference to the trigger.
            // L530 (`skillEffect.grantedEffect.addSkillTypes and …`: every support has an
            // `addSkillTypes` table at the pin, so only the disable test remains; tests/AS.rs
            // census)
            let r#match = !skillFlags.truthy(n!("disable"));
            // L531
            if r#match && sge.isTrigger {
                // L532
                if activeSkill.triggeredBy.get().is_some() {
                    // L533
                    skillFlags.set(n!("disable"), true);
                    // L534
                    activeSkill
                        .disableReason
                        .set(lstr("This skill is supported by more than one trigger"));
                } else {
                    // L536
                    activeSkill.triggeredBy.set(Some(skillEffect.clone()));
                }
            }
            // L539
            let pvp = level.get(n!("PvPDamageMultiplier"));
            if pvp.truthy() {
                // L540
                skillModList.NewMod(
                    n!("PvpDamageMultiplier"),
                    ModType::MORE,
                    pvp.clone(),
                    &sge.modSource,
                    0,
                    0,
                    &[],
                );
            }
            // L542
            let su = level.get(n!("storedUses"));
            if su.truthy() {
                // L543
                skillData.set(n!("storedUses"), Val::from_value(su));
            }
            // L545
            let vsu = level.get(n!("vaalStoredUses"));
            if vsu.truthy() {
                // L546 LUA-QUIRK: `storedUses or 0 + vaalStoredUses` is `storedUses or (0 + vaalStoredUses)`
                let v = lor(skillData.get(n!("storedUses")), || {
                    Val::Num(0.0 + arith(vsu))
                });
                skillData.set(n!("storedUses"), v);
            }
        }
    }

    let srcInstance = activeSkill.activeEffect.get().srcInstance.get();
    // L551 Apply gem/quality modifiers from support gems
    // L552
    skillModList.NewMod(
        n!("GemLevel"),
        ModType::BASE,
        srcInstance
            .as_ref()
            .map_or(activeEffect.level.get(), |s| s.level.get()),
        "Max Level",
        0,
        0,
        &[],
    );
    // L553
    skillModList.NewMod(
        n!("GemQuality"),
        ModType::BASE,
        srcInstance
            .as_ref()
            .map_or(activeEffect.quality.get(), |s| s.quality.get()),
        "Max Quality",
        0,
        0,
        &[],
    );
    // L554 matching socket bonus
    // L555
    let socketMatches = match srcInstance.as_ref() {
        Some(s) if s.extra.truthy(n!("matchesSocket")) => true,
        _ => activeEffect.extra.truthy(n!("matchesSocket")),
    };
    // L556
    if socketMatches {
        // L557
        skillModList.NewMod(
            n!("GemSocketQuality"),
            ModType::BASE,
            data().misc.MatchingSocketQualityBonus,
            "Socket Quality",
            0,
            0,
            &[],
        );
    }
    // L559
    for supportProperty in skillModList.Tabulate(
        Some(ModType::LIST),
        Some(&skillCfg),
        nm!["SupportedGemProperty"],
    ) {
        // L560
        let value = &supportProperty.value;
        // L561
        let gemData = activeSkill.activeEffect.get().gemData.get();
        if value.get(n!("keyword")).as_str() == Some("grants_active_skill")
            && gemData
                .as_ref()
                .is_some_and(|g| !g.tags.contains(&n!("support")))
        {
            let key = value.get(n!("key"));
            let vv = arith(value.get(n!("value")));
            // L562 save quality increases for use in tooltips
            // L563
            if key.as_str() == Some("quality") {
                // L564
                activeEffect
                    .supportQuality
                    .set(Some(activeEffect.supportQuality.get().unwrap_or(0.0) + vv));
            }
            // L566
            effect_add(&activeEffect, key, vv);
            // L567 (`NewMod(name, "BASE", v, source, tag)`: the tag is not a number, so it is a tag)
            let keyName = key
                .as_str()
                .unwrap_or_else(|| lua_error!("attempt to index field 'key' (a nil value)"));
            let m = &supportProperty.r#mod;
            let tags: Vec<Tag> = if m.source.is_some() {
                m.tags.first().cloned().into_iter().collect()
            } else {
                // a nil source shifts the tag to `[2]`, which `ipairs` never reaches
                Vec::new()
            };
            skillModList.AddMod(Arc::new(createMod(
                cat!(
                    "GemSupport",
                    pob_lua::string::firstToUpper(keyName).as_str()
                ),
                ModType::BASE,
                value.get(n!("value")).clone(),
                m.source.as_deref(),
                0,
                0,
                &tags,
            )));
        }
    }

    // L571 (the rows are `{ value = …, mod = … }` tables, CalcSetup.lua:413-414)
    if let Some(gpi) = activeSkill.activeEffect.get().gemPropertyInfo.get() {
        for_ipairs!((_i, gemProperty) in gpi => {
            // L572
            let value = gemProperty.get(n!("value"));
            let m = match gemProperty.get(n!("mod")) {
                Val::Mod(m) => m,
                other => lua_error!("attempt to index field 'mod' (a {} value)", other.lua_type()),
            };
            let key = value.get(n!("key"));
            let keyName = key
                .str()
                .unwrap_or_else(|| lua_error!("attempt to index field 'key' (a nil value)"));
            let tags: Vec<Tag> = if m.source.is_some() {
                m.tags.first().cloned().into_iter().collect()
            } else {
                Vec::new()
            };
            let v: Value = match value.get(n!("value")) {
                Val::Num(n) => Value::Num(n),
                Val::Bool(b) => Value::Bool(b),
                Val::Str(s) => Value::Str(AStr::from(&*s)),
                Val::Nil => Value::Nil,
                Val::Data(d) => d,
                Val::Mod(m) => Value::Mod(m),
                other => lua_error!("GemItem value of type {}", other.lua_type()),
            };
            // L573
            skillModList.AddMod(Arc::new(createMod(
                cat!("GemItem", pob_lua::string::firstToUpper(&keyName).as_str()),
                ModType::BASE,
                v,
                m.source.as_deref(),
                0,
                0,
                &tags,
            )));
        });
    }

    // L576 Add active gem modifiers
    // L577
    {
        let a = actor();
        activeEffect
            .actorLevel
            .set(if a.minionData.get().is_some() {
                Some(a.level.get())
            } else {
                None
            });
    }
    // L578
    {
        let extra = Table::new();
        for v in skillModList.List(Some(&skillCfg), nm!["ExtraSkillStat"]) {
            extra.push(Val::from_value(&v));
        }
        mergeSkillInstanceMods(env, &skillModList, &activeEffect, Some(&extra));
    }
    // L579
    activeEffect.grantedEffectLevel.set(
        activeGrantedEffect
            .levels
            .get(activeEffect.level.get())
            .cloned(),
    );

    // L581 Add extra modifiers from granted effect level
    // L582
    let level = activeEffect
        .grantedEffectLevel
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'level' (a nil value)"));
    // L583
    skillData.set(
        n!("CritChance"),
        Val::from_value(level.get(n!("critChance"))),
    );
    // L584
    let dm = level.get(n!("damageMultiplier"));
    if dm.truthy() {
        // L585
        skillModList.NewMod(
            n!("Damage"),
            ModType::MORE,
            dm.clone(),
            &activeEffect.grantedEffect.get().modSource,
            ModFlag::Attack,
            0,
            &[],
        );
    }
    // L587-597
    for k in [
        n!("attackTime"),
        n!("attackSpeedMultiplier"),
        n!("cooldown"),
        n!("storedUses"),
    ] {
        let v = level.get(k);
        if v.truthy() {
            skillData.set(k, Val::from_value(v));
        }
    }
    // L599
    let vsu = level.get(n!("vaalStoredUses"));
    if vsu.truthy() {
        // L600 LUA-QUIRK: `storedUses or (0 + vaalStoredUses)`
        let v = lor(skillData.get(n!("storedUses")), || {
            Val::Num(0.0 + arith(vsu))
        });
        skillData.set(n!("storedUses"), v);
    }
    // L602
    let spd = level.get(n!("soulPreventionDuration"));
    if spd.truthy() {
        // L603
        skillData.set(n!("soulPreventionDuration"), Val::from_value(spd));
    }
    // L605
    let pvp = level.get(n!("PvPDamageMultiplier"));
    if pvp.truthy() {
        // L606
        skillModList.NewMod(
            n!("PvpDamageMultiplier"),
            ModType::MORE,
            pvp.clone(),
            &activeEffect.grantedEffect.get().modSource,
            0,
            0,
            &[],
        );
    }

    // L609 Add extra modifiers from other sources
    // L610
    let extraSkillModList: Shared<Vec<ModRef>> = Shared::new(Vec::new());
    activeSkill.extraSkillModList.set(extraSkillModList.clone());
    // L611
    for value in skillModList.List(Some(&skillCfg), nm!["ExtraSkillMod"]) {
        let m = value.inner_mod().cloned().unwrap_or_else(|| {
            lua_error!("attempt to index a nil value (ExtraSkillMod value.mod)")
        });
        // L612
        skillModList.AddMod(m.clone());
        // L613
        extraSkillModList.push(m);
    }

    // L616 Find totem level
    // L617
    if skillFlags.truthy(n!("totem")) {
        // L618
        skillData.set(n!("totemLevel"), level.levelRequirement);
    }

    // L621 Add active mine multiplier
    // L622
    if skillFlags.truthy(n!("mine")) {
        let src = src_or_err(&activeEffect);
        // L623
        activeSkill.activeMineCount.set(if env.mode == Mode::CALCS {
            src.skillMineCountCalcs.get()
        } else {
            src.skillMineCount.get()
        });
        // L624
        if let Some(amc) = activeSkill.activeMineCount.get() {
            if amc > 0.0 {
                // L625
                skillModList.NewMod(
                    n!("Multiplier:ActiveMineCount"),
                    ModType::BASE,
                    amc,
                    "Base",
                    0,
                    0,
                    &[],
                );
                // L626
                let mults = env.enemy.modDB.get().multipliers.clone();
                let v = m_max(amc, mults.num_or(n!("ActiveMineCount"), 0.0));
                mults.set(n!("ActiveMineCount"), v);
            }
        }
    } else if let Some(src) = src_not_secondary(&activeEffect) {
        // L628
        // L629
        src.skillMineCountCalcs.set(None);
        // L630
        src.skillMineCount.set(None);
    }

    // L634 Determine if it possible to have a stage on this skill based upon skill parts.
    // L635
    let mut noPotentialStage = true;
    // L636 (`parts` nil and `{ }` both skip the loop)
    // L637
    for part in &activeEffect.grantedEffect.get().parts {
        // L638
        if part.flags.iter().any(|(k, v)| *v && k.as_str() == "stages") {
            // L639
            noPotentialStage = false;
            // L640
            break;
        }
    }

    let stageName = pob_lua::pattern::gsub(activeGrantedEffect.name.as_str(), "%s+", "", None).0;
    // L645
    let limit = skillModList.Sum(
        ModType::BASE,
        Some(&skillCfg),
        &[cat!("Multiplier:", stageName.as_str(), "MaxStages")],
    );
    // L646
    skillData.set(
        n!("stagesMax"),
        if limit > 0.0 {
            Val::Num(limit)
        } else {
            Val::Nil
        },
    );
    // L647
    if limit > 0.0 {
        // L648
        skillFlags.set(n!("multiStage"), true);
        // L649
        let src = src_or_err(&activeEffect);
        let first = if env.mode == Mode::CALCS {
            src.skillStageCountCalcs.get()
        } else {
            src.skillStageCount.get()
        };
        let first = match first {
            Some(v) => v,
            None => lor(skillData.get(n!("stagesMax")), || Val::Num(1.0)).num(),
        };
        let minimum = 1.0
            + skillModList.Sum(
                ModType::BASE,
                Some(&skillCfg),
                &[cat!("Multiplier:", stageName.as_str(), "MinimumStage")],
            );
        activeSkill
            .activeStageCount
            .set(Some(m_max(first, minimum)));
        // L650
        if let Some(asc) = activeSkill.activeStageCount.get() {
            if asc > 0.0 {
                // L651
                skillModList.NewMod(
                    cat!("Multiplier:", stageName.as_str(), "Stage"),
                    ModType::BASE,
                    m_min(limit, asc),
                    "Base",
                    0,
                    0,
                    &[],
                );
                // L652
                activeSkill.activeStageCount.set(Some(
                    activeSkill.activeStageCount.get().unwrap_or(0.0) - 1.0,
                ));
                // L653
                skillModList.NewMod(
                    cat!("Multiplier:", stageName.as_str(), "StageAfterFirst"),
                    ModType::BASE,
                    m_min(
                        limit - 1.0,
                        activeSkill.activeStageCount.get().expect("set above"),
                    ),
                    "Base",
                    0,
                    0,
                    &[],
                );
            }
        }
    } else if noPotentialStage {
        if let Some(src) = src_not_secondary(&activeEffect) {
            // L655
            // L656
            src.skillStageCountCalcs.set(None);
            // L657
            src.skillStageCount.set(None);
        }
    }

    // L660 Extract skill data
    // L661
    for value in env.modDB.get().List(Some(&skillCfg), nm!["SkillData"]) {
        // L662
        let k = value_key(value.get(n!("key")));
        skillData.set(k, Val::from_value(value.get(n!("value"))));
    }
    // L664
    for value in skillModList.List(Some(&skillCfg), nm!["SkillData"]) {
        // L665
        let k = value_key(value.get(n!("key")));
        skillData.set(k, Val::from_value(value.get(n!("value"))));
    }

    // L668 Create minion
    // L669
    let mut minionList: Vec<Name>;
    let mut isSpectre = false;
    // L670
    let minionSupportLevel = Table::new();
    // L671 (CCR-0040: `minionList = { }` vs nil)
    if minionList_is_set(&activeGrantedEffect) {
        // L672
        if !activeGrantedEffect.minionList.is_empty() {
            // L673
            minionList = activeGrantedEffect.minionList.clone();
        } else {
            // L675
            minionList = env.build.spectreList.get().with(Clone::clone);
            // L676
            isSpectre = true;
        }
    } else {
        // L679
        minionList = Vec::new();
    }
    // L681
    for skillEffect in activeSkill.effectList.get().with(Clone::clone) {
        let sge = skillEffect.grantedEffect.get();
        // L682
        if sge.support {
            // L683 (`addMinionList` nil and `{ }` both skip the loop)
            for minionType in &sge.addMinionList {
                // L684
                if isValueInArray(&minionList, minionType).is_none() {
                    // L685
                    let lr = sge
                        .levels
                        .get(skillEffect.level.get())
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index a nil value (levels[level])")
                        })
                        .levelRequirement;
                    minionSupportLevel.set(*minionType, lr);
                    // L686
                    minionList.push(*minionType);
                }
            }
        }
    }
    // L691
    activeSkill.minionList.set(Shared::new(minionList.clone()));
    // L692
    if !minionList.is_empty() && actor().minionData.get().is_none() {
        // L693
        let minionType: Option<Name>;
        // L694
        if env.mode == Mode::CALCS && isMainSkill() {
            let src = src_or_err(&activeEffect);
            // L695
            let index = src
                .skillMinionCalcs
                .get()
                .and_then(|v| isValueInArray(&minionList, &v))
                .unwrap_or(1);
            // L696
            minionType = minionList.get(index - 1).copied();
            // L697
            src.skillMinionCalcs.set(minionType);
        } else {
            let src = src_or_err(&activeEffect);
            // L699
            let index = src
                .skillMinion
                .get()
                .and_then(|v| isValueInArray(&minionList, &v))
                .unwrap_or(1);
            // L700
            minionType = minionList.get(index - 1).copied();
            // L701
            src.skillMinion.set(minionType);
        }
        // L703
        if let Some(minionType) = minionType {
            // L704
            let minion = Actor::new();
            // `{ }`: no `itemList` until L722 (`Actor::new` gives one)
            minion.itemList.set(None);
            // L705
            activeSkill.minion.set(Some(minion.clone()));
            // L706
            skillFlags.set(n!("haveMinion"), true);
            // L707
            minion.r#type.set(Some(minionType));
            // L708
            minion
                .minionData
                .set(env.data.minions.get(&minionType).cloned());
            // L709
            minion
                .hostile
                .set(minion.minionData.get().is_some_and(|d| d.hostile));
            // L710
            if minion.hostile.get() {
                // L711
                minion.parent.set(Rc::downgrade(&env.enemy));
                // L712
                minion.enemy.set(Rc::downgrade(&env.player));
            } else {
                // L714
                minion.parent.set(Rc::downgrade(&env.player));
                // L715
                minion.enemy.set(Rc::downgrade(&env.enemy));
            }
            // L717-719
            let mlevel: f64 = if skillData.truthy(n!("minionLevelIsEnemyLevel")) {
                env.enemyLevel.get()
            } else if skillData.truthy(n!("minionLevelIsPlayerLevel")) {
                // `env.build and env.build.characterLevel or …`: the build and its level are always set
                m_min(
                    env.build.characterLevel.get(),
                    skillData.get(n!("minionLevelIsPlayerLevel")).num(),
                )
            } else {
                lor(minionSupportLevel.get(minionType), || {
                    lor(skillData.get(n!("minionLevel")), || {
                        Val::Num(
                            activeEffect
                                .grantedEffectLevel
                                .get()
                                .unwrap_or_else(|| {
                                    lua_error!(
                                        "attempt to index field 'grantedEffectLevel' (a nil value)"
                                    )
                                })
                                .levelRequirement,
                        )
                    })
                })
                .num()
            };
            minion.level.set(mlevel);
            // L720 fix minion level between 1 and 100
            // L721
            minion
                .level
                .set(m_min(m_max(minion.level.get(), 1.0), 100.0));
            // L722
            minion
                .itemList
                .set(Some(Shared::new(indexmap::IndexMap::new())));
            // L723
            minion.uses.set(activeGrantedEffect.minionUses.clone());
            let minionData = minion
                .minionData
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'minionData' (a nil value)"));
            // L724
            let lifeTable: &'static [f64] = if minionData.hostile {
                // L725
                &env.data.monsterLifeTable
            } else {
                // L727
                if minionData
                    .lifeScaling
                    .is_some_and(|s| s.as_str() == "AltLife1")
                {
                    &env.data.monsterLifeTable2
                } else if minionData
                    .lifeScaling
                    .is_some_and(|s| s.as_str() == "AltLife2")
                {
                    &env.data.monsterLifeTable3
                } else if isSpectre {
                    &env.data.monsterLifeTable
                } else {
                    &env.data.monsterAllyLifeTable
                }
            };
            minion.lifeTable.set(Some(lifeTable));
            // L729
            let attackTime = minionData.attackTime;
            // L730
            let damageTable: &'static [f64] = if isSpectre || minionData.hostile {
                &env.data.monsterDamageTable
            } else {
                &env.data.monsterAllyDamageTable
            };
            // L731
            let mut damage = index_f64(damageTable, minion.level.get())
                .copied()
                .unwrap_or_else(|| {
                    lua_error!("attempt to perform arithmetic on a nil value (damageTable[level])")
                })
                * minionData.damage;
            // L732 minions with this flag do not factor attack time into their base damage
            if !minionData.baseDamageIgnoresAttackSpeed {
                // L733
                damage *= attackTime;
            }
            let itemsTab = &env.build.itemsTab;
            let itemSetAt = |k: Option<f64>| -> Option<Table> {
                k.and_then(u32_key)
                    .and_then(|k| itemsTab.itemSets.with(|m| m.get(&k).cloned()))
            };
            let firstSet = || {
                itemsTab
                    .itemSetOrderList
                    .with(|v| v.first().map(|x| f64::from(*x)))
            };
            // L735
            if activeGrantedEffect.minionHasItemSet {
                // L736
                if env.mode == Mode::CALCS && isMainSkill() {
                    let src = src_or_err(&activeEffect);
                    // L737
                    if itemSetAt(src.skillMinionItemSetCalcs.get()).is_none() {
                        // L738
                        src.skillMinionItemSetCalcs.set(firstSet());
                    }
                    // L740
                    minion
                        .itemSet
                        .set(itemSetAt(src.skillMinionItemSetCalcs.get()));
                } else {
                    let src = src_or_err(&activeEffect);
                    // L742
                    if itemSetAt(src.skillMinionItemSet.get()).is_none() {
                        // L743
                        src.skillMinionItemSet.set(firstSet());
                    }
                    // L745
                    minion.itemSet.set(itemSetAt(src.skillMinionItemSet.get()));
                }
            } else if let Some(src) = src_not_secondary(&activeEffect) {
                // L747
                // L748
                src.skillMinionItemSetCalcs.set(None);
                // L749
                src.skillMinionItemSet.set(None);
            }
            let playerWd1 = env.player.weaponData1.get();
            // L751
            if (skillData.truthy(n!("minionUseBowAndQuiver"))
                && playerWd1.get(n!("type")).str().as_deref() == Some("Bow"))
                || skillData.truthy(n!("minionUseMainHandWeapon"))
            {
                // L752
                minion.weaponData1.set(playerWd1.clone());
            } else if env.theIronMass.get().is_some() && minionType.as_str() == "RaisedSkeleton" {
                // L753
                // L754
                minion.weaponData1.set(playerWd1.clone());
            } else {
                // L756-763
                let wd = Table::new();
                wd.set(n!("type"), minionData.weaponType1.unwrap_or(n!("None")));
                wd.set(n!("AttackRate"), 1.0 / attackTime);
                wd.set(n!("CritChance"), 5.0);
                wd.set(
                    n!("PhysicalMin"),
                    round(damage * (1.0 - minionData.damageSpread), None),
                );
                wd.set(
                    n!("PhysicalMax"),
                    round(damage * (1.0 + minionData.damageSpread), None),
                );
                wd.set(n!("range"), minionData.attackRange);
                minion.weaponData1.set(wd);
            }
            // L765
            minion.weaponData2.set(Table::new());
            // L766
            if let Some(uses) = minion.uses.get() {
                let itemSet = minion.itemSet.get();
                let slotItem = |slot: &str, swap: &str| -> Option<ItemRef> {
                    let set = itemSet.as_ref().expect("checked by the caller");
                    let key = if set.truthy(n!("useSecondWeaponSet")) {
                        Name::from(swap)
                    } else {
                        Name::from(slot)
                    };
                    let selItemId = set
                        .get(key)
                        .get(n!("selItemId"))
                        .opt_num()
                        .and_then(u32_key);
                    selItemId.and_then(|id| itemsTab.items.with(|m| m.get(&id).cloned()))
                };
                // L767
                if uses.contains(&n!("Weapon 1")) {
                    // L768
                    if itemSet.is_some() {
                        // L769
                        let item = slotItem("Weapon 1", "Weapon 1 Swap");
                        // L770
                        if let Some(wd) = item.and_then(|i| i.weaponData.get()) {
                            // L771 LUA-QUIRK: a weapon item always has weaponData[1]; nil is kept as is
                            if let Some(w) = wd.table(1i64) {
                                minion.weaponData1.set(w);
                            }
                        }
                    } else {
                        // L774
                        minion.weaponData1.set(playerWd1.clone());
                    }
                }
                // L777
                if uses.contains(&n!("Weapon 2")) {
                    // L778
                    if itemSet.is_some() {
                        // L779
                        let item = slotItem("Weapon 2", "Weapon 2 Swap");
                        // L780
                        if let Some(wd) = item.and_then(|i| i.weaponData.get()) {
                            // L781
                            if let Some(w) = wd.table(2i64) {
                                minion.weaponData2.set(w);
                            }
                        }
                    } else {
                        // L784
                        minion.weaponData2.set(env.player.weaponData2.get());
                    }
                }
            }
            // L788
            if !isSpectre
                && skillModList.Flag(
                    Some(&skillCfg),
                    nm!["NonSpectreMinionsUseParentMainHandAttackTime"],
                )
                && playerWd1.truthy(n!("AttackRate"))
                && playerWd1.num(n!("AttackRate")) > 0.0
            {
                // L789 The weapon data may be shared with the player, an item set, or another actor.
                // L790 Copy it before replacing only the minion's base attack rate.
                // L791
                minion
                    .weaponData1
                    .set(minion.weaponData1.get().copyTable(false));
                // L792
                minion
                    .weaponData1
                    .get()
                    .set(n!("AttackRate"), playerWd1.get(n!("AttackRate")));
            }
        }
    } else if let Some(src) = src_not_secondary(&activeEffect) {
        // L795
        // L796
        src.skillMinionCalcs.set(None);
        // L797
        src.skillMinion.set(None);
        // L798
        src.skillMinionItemSetCalcs.set(None);
        // L799
        src.skillMinionItemSet.set(None);
        // L800
        src.skillMinionSkill.set(None);
        // L801
        src.skillMinionSkillCalcs.set(None);
    }

    // L804 Separate global effect modifiers (mods that can affect defensive stats or other skills)
    // L805
    let mut i = 1usize;
    let buffList = activeSkill.buffList.get();
    // L806 (CCR-0040: positional ModList access)
    while let Some(cur) = list_at(&skillModList, i) {
        // L807
        let mut effectType: Option<Name> = None;
        let mut effectName: Option<Name> = None;
        let mut effectTag: Option<Tag> = None;
        // L808
        for tag in &cur.tags {
            if tag.r#type == TagType::LuaNonTable {
                lua_error!("attempt to index local 'tag' (a number value)");
            }
            // L809
            if tag.r#type == TagType::GlobalEffect {
                // L810
                effectType = tag.effectType;
                // L811
                effectName = Some(tag.effectName.unwrap_or(activeGrantedEffect.name));
                // L812
                effectTag = Some(tag.clone());
                // L813
                break;
            }
        }
        // L816
        let condFails = match effectTag.as_ref().and_then(|t| t.modCond) {
            Some(modCond) => !skillModList.GetCondition(modCond, Some(&skillCfg), false),
            None => false,
        };
        if condFails {
            // L817
            list_remove(&skillModList, i);
        } else if let Some(effectType) = effectType {
            // L818
            let effectTag = effectTag.expect("set with effectType");
            // L819
            let mut buff: Option<Table> = None;
            // L820
            for skillBuff in buffList.with(Clone::clone) {
                // L821
                if skillBuff.str(n!("type")).as_deref() == Some(effectType.as_str())
                    && skillBuff.str(n!("name")).as_deref() == effectName.map(|n| n.as_str())
                {
                    // L822
                    buff = Some(skillBuff);
                    // L823
                    break;
                }
            }
            // L826
            let buff = match buff {
                Some(b) => b,
                None => {
                    // L827-839
                    let b = Table::new();
                    b.set(n!("type"), effectType);
                    if let Some(n) = effectName {
                        b.set(n!("name"), n);
                    }
                    if effectTag.allowTotemBuff {
                        b.set(n!("allowTotemBuff"), true);
                    }
                    if let Some(c) = effectTag.effectCond {
                        b.set(n!("cond"), c);
                    }
                    if let Some(c) = effectTag.effectEnemyCond {
                        b.set(n!("enemyCond"), c);
                    }
                    if let Some(c) = effectTag.effectStackVar {
                        b.set(n!("stackVar"), c);
                    }
                    if let Some(c) = effectTag.effectStackLimit {
                        b.set(n!("stackLimit"), c);
                    }
                    if let Some(c) = effectTag.extra_get(n!("effectStackLimitVar")) {
                        b.set(n!("stackLimitVar"), Val::from_value(c));
                    }
                    if effectTag.applyNotPlayer {
                        b.set(n!("applyNotPlayer"), true);
                    }
                    if effectTag.applyMinions {
                        b.set(n!("applyMinions"), true);
                    }
                    b.set(n!("modList"), Table::new());
                    // L840
                    if cur.source.as_deref() == Some(&*activeGrantedEffect.modSource) {
                        // L841 Inherit buff configuration from the active skill
                        // L842
                        b.set(n!("activeSkillBuff"), true);
                        // L843
                        let v = lor(b.get(n!("applyNotPlayer")), || {
                            skillData.get(n!("buffNotPlayer"))
                        });
                        b.set(n!("applyNotPlayer"), v);
                        // L844
                        let v = lor(b.get(n!("applyMinions")), || {
                            skillData.get(n!("buffMinions"))
                        });
                        b.set(n!("applyMinions"), v);
                        // L845
                        b.set(n!("applyAllies"), skillData.get(n!("buffAllies")));
                        // L846
                        b.set(n!("allowTotemBuff"), skillData.get(n!("allowTotemBuff")));
                    }
                    // L848
                    buffList.push(b.clone());
                    b
                }
            };
            // L850
            let mut r#match = false;
            // L851
            let modList = buff
                .table(n!("modList"))
                .unwrap_or_else(|| lua_error!("attempt to index field 'modList' (a nil value)"));
            // L852
            let n_mods = modList.len();
            for d in 1..=n_mods {
                // L853
                let destMod = match modList.get(d as i64) {
                    Val::Mod(m) => m,
                    other => lua_error!("buff modList entry of type {}", other.lua_type()),
                };
                // L854
                if compareModParams(&cur, &destMod)
                    && (destMod.r#type == ModType::BASE || destMod.r#type == ModType::INC)
                {
                    // L855
                    let mut copy: Mod = (*destMod).clone();
                    // L856
                    copy.value = Value::Num(arith(&destMod.value) + arith(&cur.value));
                    // L857
                    modList.set(d as i64, Val::Mod(Arc::new(copy)));
                    // L858
                    r#match = true;
                    // L859
                    break;
                }
            }
            // L862
            if !r#match {
                // L863
                modList.push(Val::Mod(cur.clone()));
            }
            // L865
            list_remove(&skillModList, i);
        } else {
            // L867
            i += 1;
        }
    }

    // L871
    if !buffList.is_empty() {
        // L872 Add to auxiliary skill list
        // L873
        env.auxSkillList.get().push(activeSkill.clone());
    }
}

/// @pob Modules/CalcActiveSkill.lua:878-934 h=b0ce6385
/// Initialise the active skill's minion skills
pub fn createMinionSkills(env: &EnvRef, activeSkill: &ActiveSkillRef) {
    // L879
    let activeEffect = activeSkill.activeEffect.get();
    // L880
    let minion = activeSkill
        .minion
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'minion' (a nil value)"));
    // L881
    let minionData = minion
        .minionData
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index local 'minionData' (a nil value)"));

    // L883
    let activeSkillList: Shared<Vec<ActiveSkillRef>> = Shared::new(Vec::new());
    minion.activeSkillList.set(activeSkillList.clone());
    // L884
    let mut skillIdList: Vec<Name> = Vec::new();
    // L885
    for skillId in &minionData.skillList {
        // L886
        if env.data.skills.contains_key(skillId) {
            // L887
            skillIdList.push(*skillId);
        }
    }
    // L890
    let minionType = minion.r#type.get();
    for skill in activeSkill
        .skillModList
        .get()
        .List(Some(&activeSkill.skillCfg.get()), nm!["ExtraMinionSkill"])
    {
        // L891
        let ml = skill.get(n!("minionList"));
        let ok = match ml {
            Value::Nil | Value::Bool(false) => true,
            Value::Table(t) => {
                let names: Vec<Name> = t
                    .array
                    .iter()
                    .filter_map(|v| v.as_str().map(Name::from))
                    .collect();
                minionType.is_some_and(|v| isValueInArray(&names, &v).is_some())
            }
            other => lua_error!(
                "bad argument #1 to 'ipairs' (table expected, got {})",
                other.lua_type()
            ),
        };
        if ok {
            // L892 (`t_insert(t, nil)` appends nothing)
            if let Some(id) = skill.get(n!("skillId")).as_str() {
                skillIdList.push(Name::from(id));
            }
        }
    }
    // L895
    if skillIdList.is_empty() {
        // L896 Not ideal, but let's avoid horrible crashes if a spectre has no skills for some reason
        // L897
        skillIdList.push(n!("Melee"));
    }
    // L899
    for skillId in &skillIdList {
        // L900-904
        let ge =
            env.data.skills.get(skillId).cloned().unwrap_or_else(|| {
                lua_error!("attempt to index field 'grantedEffect' (a nil value)")
            });
        let activeEffect = SkillEffect::new(ge.clone(), 1.0, 0.0);
        // L905
        if levels_len(&ge.levels) > 1 {
            // L906
            for (i0, levelData) in ge.levels.0.iter().enumerate() {
                let Some(levelData) = levelData else { break };
                // L907
                if levelData.levelRequirement > minion.level.get() {
                    // L908
                    break;
                } else {
                    // L910
                    activeEffect.level.set((i0 + 1) as f64);
                }
            }
        }
        // L914
        let minionSkill = createActiveSkill(
            activeEffect,
            &activeSkill.supportList.get(),
            Rc::downgrade(&minion),
            None,
            Some(activeSkill.clone()),
        );
        // L915
        buildActiveSkillModList(env, &minionSkill);
        let flags = minionSkill.skillFlags.get();
        // L916
        flags.set(n!("minion"), true);
        // L917
        flags.set(n!("minionSkill"), true);
        // L918
        flags.set(n!("haveMinion"), true);
        // L919
        flags.set(
            n!("spectre"),
            activeSkill.skillFlags.get().get(n!("spectre")),
        );
        // L920
        let sd = activeSkill.skillData.get();
        minionSkill.skillData.get().set(
            n!("damageEffectiveness"),
            1.0 + sd.get(n!("minionDamageEffectiveness")).num_or(0.0) / 100.0,
        );
        // L921
        activeSkillList.push(minionSkill);
    }
    // L923
    let skillIndex: f64;
    let n_list = activeSkillList.len() as f64;
    // L924
    if env.mode == Mode::CALCS {
        let src = src_or_err(&activeEffect);
        // L925
        skillIndex = m_max(
            m_min(src.skillMinionSkillCalcs.get().unwrap_or(1.0), n_list),
            1.0,
        );
        // L926
        src.skillMinionSkillCalcs.set(Some(skillIndex));
    } else {
        let src = src_or_err(&activeEffect);
        // L928
        skillIndex = m_max(
            m_min(src.skillMinionSkill.get().unwrap_or(1.0), n_list),
            1.0,
        );
        // L929
        if env.mode == Mode::MAIN {
            // L930
            src.skillMinionSkill.set(Some(skillIndex));
        }
    }
    // L933
    minion
        .mainSkill
        .set(activeSkillList.with(|v| index_f64(v, skillIndex).cloned()));
}

#[cfg(test)]
mod fup2_key_tests {
    use super::*;

    /// FOLLOWUPS "FUP1" item 6: CalcActiveSkill.lua:59
    /// `stats[stat.key] = (stats[stat.key] or 0) + stat.value` reads nil at a NaN (or nil) key,
    /// so a nil `stat.value` raises the arithmetic error first, as in Lua; with a number value
    /// the store raises "table index is NaN".
    #[test]
    fn nan_and_nil_keys_read_nil() {
        let stats = Table::new();
        stats.set(n!("base_skill_effect_duration"), 1000.0);
        assert!(read_at(&stats, &Val::Num(f64::NAN)).is_nil());
        assert!(read_at(&stats, &Val::Nil).is_nil());
        assert_eq!(read_at(&stats, &Val::from("base_skill_effect_duration")).num(), 1000.0);
        let k = Val::Num(f64::NAN);
        let e = pob_lua::error::catch(|| read_at(&stats, &k).num_or(0.0) + Val::Nil.num()).unwrap_err();
        assert!(e.0.contains("arithmetic on a nil value"), "{}", e.0);
        let e = pob_lua::error::catch(|| stats.set(val_key(&k), 1.0)).unwrap_err();
        assert!(e.0.contains("table index is NaN"), "{}", e.0);
    }
}
