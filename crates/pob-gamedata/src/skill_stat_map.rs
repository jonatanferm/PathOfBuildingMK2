//! @pob-file Modules/Data.lua @16de4b82d
//! Skill stat maps: the `makeSkillMod` family, `processMod` and the
//! `data.skillStatMapMeta.__index` fallback (Modules/Data.lua 51-93,
//! 1033-1045). Owner: CT. Signatures frozen by U0 (calc doc §4.4).
//!
//! `data` is immutable, but the Lua fallback has two lasting effects inside one
//! PoB process (one Lua VM = one thread here): the processed copy is stored
//! into `grantedEffect.statMap[stat]` (a memo), and `processMod` sets
//! `grantedEffect.hasGlobalEffect = true` when a fallback mod carries a
//! `GlobalEffect` tag. Both live in thread-local state: the memo in
//! [`statMapLookup`], the flag behind [`hasGlobalEffect`] (CCR-0091: readers of
//! `grantedEffect.hasGlobalEffect` must call it instead of reading the field).
#![allow(non_snake_case)]

use std::cell::RefCell;
use std::sync::Arc;

use crate::generated::global::SkillType;
use crate::modifier::{Mod, ModType, Tag, TagType};
use crate::schema::skills::{GrantedEffect, StatMapEntry, StatMapItem};
use crate::value::{AStr, Value, ValueTable};
use crate::Name;

/// @pob Modules/Data.lua:51-61 h=599a6ebd
/// `makeSkillMod(modName, modType, modVal, flags, keywordFlags, ...)` (no source).
pub fn makeSkillMod(
    modName: Name,
    modType: ModType,
    modVal: Value,
    flags: Option<u32>,
    keywordFlags: Option<u32>,
    tags: &[Tag],
) -> Mod {
    // L53
    Mod {
        name: modName,
        r#type: modType,
        value: modVal,
        flags: flags.unwrap_or(0),
        keywordFlags: keywordFlags.unwrap_or(0),
        source: None,
        tags: tags.iter().cloned().collect(),
        extra: None,
    }
}

/// @pob Modules/Data.lua:62-64 h=bc3b1740
pub fn makeFlagMod(modName: Name, tags: &[Tag]) -> Mod {
    // L63
    makeSkillMod(
        modName,
        ModType::FLAG,
        Value::Bool(true),
        Some(0),
        Some(0),
        tags,
    )
}

/// @pob Modules/Data.lua:65-67 h=6d9f8760
pub fn makeSkillDataMod(dataKey: Name, dataValue: Value, tags: &[Tag]) -> Mod {
    let mut value = ValueTable::new();
    value.set(pob_lua::n!("key"), Value::from(dataKey));
    value.set(pob_lua::n!("value"), dataValue);
    // L66
    makeSkillMod(
        pob_lua::n!("SkillData"),
        ModType::LIST,
        Value::Table(Arc::new(value)),
        Some(0),
        Some(0),
        tags,
    )
}

/// @pob Modules/Data.lua:68-93 h=4cd5727d
/// `processMod(grantedEffect, mod, statName)`: Lua mutates `mod` (a fresh
/// copy of a `data.skillStatMap` entry, §3.6) and `grantedEffect.hasGlobalEffect`.
/// The Rust port returns the processed mod and whether the effect gained a
/// global effect; `data` is immutable, so the caller ([`statMapLookup`])
/// memoises the result per (granted effect, stat), as Lua's `t[key] = map` does.
pub fn processMod(grantedEffect: &GrantedEffect, m: &Mod, statName: Option<Name>) -> (Mod, bool) {
    let mut r#mod = m.clone();
    let mut hasGlobalEffect = false;
    // L69
    r#mod.source = Some(grantedEffect.modSource.clone());
    // L70
    if let Some(inner) = r#mod.value.inner_mod() {
        // L71
        let mut inner = (**inner).clone();
        inner.source = Some(AStr::from(format!("Skill:{}", grantedEffect.id).as_str()));
        if let Value::Table(t) = &r#mod.value {
            r#mod.value = Value::Table(Arc::new(
                t.with(pob_lua::n!("mod"), Value::Mod(Arc::new(inner))),
            ));
        }
    }

    // L74
    for tag in &r#mod.tags {
        // L75: `tag.type` on a non-table tag (flags passed as a tag, SkillStatMap.lua:1735)
        // raises in Lua
        if tag.r#type == TagType::LuaNonTable {
            pob_lua::lua_error!("attempt to index local 'tag' (a number value)");
        }
        if tag.r#type == TagType::GlobalEffect {
            // L76
            hasGlobalEffect = true;
            // L77
            break;
        }
    }

    // L81
    let mut notMinionStat = false;
    // L82 (`grantedEffect.notMinionStat` is nil when the skill has none; an empty list iterates
    // nothing, which is the same)
    if let Some(statName) = statName {
        if !grantedEffect.notMinionStat.is_empty()
            && (grantedEffect.support || grantedEffect.skillTypes.has(SkillType::Buff))
        {
            // L83
            for notMinionStatName in &grantedEffect.notMinionStat {
                // L84
                if *notMinionStatName == statName {
                    // L85
                    notMinionStat = true;
                }
            }
        }
    }
    // L89
    if notMinionStat {
        // L90
        r#mod.tags.push(Tag {
            actor: Some(pob_lua::n!("parent")),
            neg: true,
            ..Tag::new(TagType::ActorCondition)
        });
    }
    (r#mod, hasGlobalEffect)
}

thread_local! {
    /// `grantedEffect.statMap[key] = map` (L1038), per granted effect id.
    static STAT_MAP_MEMO: RefCell<ahash::AHashMap<(Name, Name), Arc<StatMapEntry>>> =
        RefCell::new(ahash::AHashMap::new());
    /// Granted effects whose `hasGlobalEffect` processMod set at lookup time (L76).
    static GLOBAL_EFFECT_SET: RefCell<ahash::AHashSet<Name>> = RefCell::new(ahash::AHashSet::new());
}

/// `grantedEffect.hasGlobalEffect` as PoB sees it now: the loaded value, or
/// `true` once a [`statMapLookup`] fallback processed a `GlobalEffect` mod for
/// this granted effect (Data.lua:76 mutates the shared data table; CCR-0091).
pub fn hasGlobalEffect(ge: &GrantedEffect) -> bool {
    ge.hasGlobalEffect || GLOBAL_EFFECT_SET.with(|s| s.borrow().contains(&ge.id))
}

/// Forget the lookup memo and the runtime `hasGlobalEffect` flags: the state of
/// a freshly booted PoB process (tests; the oracle runs one process per build).
pub fn reset_stat_map_state() {
    STAT_MAP_MEMO.with(|m| m.borrow_mut().clear());
    GLOBAL_EFFECT_SET.with(|s| s.borrow_mut().clear());
}

/// @pob Modules/Data.lua:1033-1045 h=0c1720bd
/// `grantedEffect.statMap[stat]` including the `__index` fallback to
/// `data.skillStatMap` + processMod (each top-level element, a group passed as
/// if it were a mod, as Lua does). Memoised per (granted effect id, stat).
pub fn statMapLookup(ge: &GrantedEffect, stat: Name) -> Option<Arc<StatMapEntry>> {
    // rawget: the skill's own entries (processed when the data was loaded)
    if let Some(e) = ge.statMap.0.get(&stat) {
        return Some(e.clone());
    }
    if let Some(e) = STAT_MAP_MEMO.with(|m| m.borrow().get(&(ge.id, stat)).cloned()) {
        return Some(e);
    }
    let key = stat;
    // L1035
    let map = crate::data::data().skillStatMap.get(&key).cloned();
    // L1036
    let map = map?;
    // L1037 copyTable(map)
    let mut map = (*map).clone();
    let mut gained = false;
    // L1039
    for i in 0..map.items.len() {
        let m = match &map.items[i] {
            StatMapItem::Mod(m) => m.clone(),
            StatMapItem::Group { .. } => continue,
        };
        // L1040. processMod raises on a non-table tag met before any GlobalEffect tag
        // (L74-75); Lua has already stored the copy (L1038) and set this mod's source
        // (L69-72), so a later lookup returns that partly processed map.
        let raises = m
            .tags
            .iter()
            .find(|t| matches!(t.r#type, TagType::LuaNonTable | TagType::GlobalEffect))
            .is_some_and(|t| t.r#type == TagType::LuaNonTable);
        if raises {
            let mut partial = (*m).clone();
            partial.tags.clear();
            let (mut partial, _) = processMod(ge, &partial, None);
            partial.tags = m.tags.clone();
            map.items[i] = StatMapItem::Mod(Arc::new(partial));
            let map = Arc::new(map);
            STAT_MAP_MEMO.with(|mm| mm.borrow_mut().insert((ge.id, key), map));
            if gained {
                GLOBAL_EFFECT_SET.with(|s| s.borrow_mut().insert(ge.id));
            }
            pob_lua::lua_error!("attempt to index local 'tag' (a number value)");
        }
        let (processed, g) = processMod(ge, &m, Some(key));
        gained |= g;
        map.items[i] = StatMapItem::Mod(Arc::new(processed));
    }
    for modOrGroup in &map.items {
        match modOrGroup {
            StatMapItem::Mod(_) => {}
            StatMapItem::Group { mods, .. } => {
                // L1040 with a group as `mod`: `group.source = …` sets a key no reader uses;
                // `group.value.mod` is never a mod; `ipairs(group)` sees the group's mods,
                // whose `type` is a mod type, never "GlobalEffect". Only the notMinionStat
                // branch would append a tag table to the group, which the typed group cannot
                // hold: data.skillStatMap has no groups at the pin (census in tests/CT.rs).
                let _ = mods;
            }
        }
    }
    let map = Arc::new(map);
    // L1038 t[key] = map
    STAT_MAP_MEMO.with(|m| m.borrow_mut().insert((ge.id, key), map.clone()));
    if gained {
        GLOBAL_EFFECT_SET.with(|s| s.borrow_mut().insert(ge.id));
    }
    // L1042
    Some(map)
}

// @pob-skip Modules/Data.lua:1046-1077 reason="skill loading and the processMod post-processing loop run at dump time (pob-dump); the dumped data.skills already carry id, modSource, processed mods, hasGlobalEffect and the statMap metatable"
