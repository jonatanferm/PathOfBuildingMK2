//! @pob-file Modules/Data.lua @16de4b82d
//! Skill stat maps: the `makeSkillMod` family, `processMod` and the
//! `data.skillStatMapMeta.__index` fallback (Modules/Data.lua 51-93,
//! 1033-1045). Owner: CT. Signatures frozen by U0 (calc doc §4.4).
#![allow(non_snake_case, unused_variables)]

use std::sync::Arc;

use crate::modifier::{Mod, ModType, Tag};
use crate::schema::skills::{GrantedEffect, StatMapEntry};
use crate::unported;
use crate::value::Value;
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
    let () = unported!("Modules/Data.lua:51-61");
    Mod {
        name: modName,
        r#type: modType,
        value: modVal,
        flags: 0,
        keywordFlags: 0,
        source: None,
        tags: smallvec::SmallVec::new(),
        extra: None,
    }
}

/// @pob Modules/Data.lua:62-64 h=bc3b1740
pub fn makeFlagMod(modName: Name, tags: &[Tag]) -> Mod {
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
    let () = unported!("Modules/Data.lua:65-67");
    makeSkillMod(
        pob_lua::n!("SkillData"),
        ModType::LIST,
        Value::Nil,
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
    let () = unported!("Modules/Data.lua:68-93");
    (m.clone(), false)
}

/// @pob Modules/Data.lua:1033-1045 h=0c1720bd
/// `grantedEffect.statMap[stat]` including the `__index` fallback to
/// `data.skillStatMap` + processMod (each top-level element, a group passed as
/// if it were a mod, as Lua does). Memoised per (granted effect id, stat).
pub fn statMapLookup(ge: &GrantedEffect, stat: Name) -> Option<Arc<StatMapEntry>> {
    if let Some(e) = ge.statMap.0.get(&stat) {
        return Some(e.clone());
    }
    unported!("Modules/Data.lua:1033-1045")
}

// @pob-skip Modules/Data.lua:1046-1077 reason="skill loading and the processMod post-processing loop run at dump time (pob-dump); the dumped data.skills already carry id, modSource, processed mods, hasGlobalEffect and the statMap metatable"
