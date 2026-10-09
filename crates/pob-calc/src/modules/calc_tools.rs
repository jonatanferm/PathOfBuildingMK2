//! @pob-file Modules/CalcTools.lua @16de4b82d
//! `calcLib` (Modules/CalcTools.lua 1-303).
//!
//! Owner: CT. Stub created by U0: signatures are final (frozen; change through a CCR),
//! bodies are `unported!` until the owner ports them (calc doc D9, §10.2).
#![allow(
    non_snake_case,
    unused_variables,
    unused_imports,
    dead_code,
    clippy::all,
    clippy::pedantic
)]

use crate::prelude::*;

// @pob Modules/CalcTools.lua:6-13 h=f046369b
// (module-level statements: pairs,t_insert,t_remove,m_floor,m_min,m_max,calcLib)

/// @pob Modules/CalcTools.lua:16-18 h=6d716a94
pub fn r#mod(modStore: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> f64 {
    unported!("Modules/CalcTools.lua:16-18")
}

/// @pob Modules/CalcTools.lua:25-29 h=9621759b
/// → (inc, more)
pub fn mods(modStore: &ModStore, cfg: Option<&ModCfg>, names: &[Name]) -> (f64, f64) {
    unported!("Modules/CalcTools.lua:25-29")
}

/// @pob Modules/CalcTools.lua:32-39 h=d5d937d8
pub fn val(modStore: &ModStore, name: Name, cfg: Option<&ModCfg>) -> f64 {
    unported!("Modules/CalcTools.lua:32-39")
}

/// @pob Modules/CalcTools.lua:42-58 h=742b293c
pub fn validateGemLevel(gemInstance: &GemInstance) {
    unported!("Modules/CalcTools.lua:42-58")
}

/// @pob Modules/CalcTools.lua:61-82 h=6c445537
pub fn doesTypeExpressionMatch(
    checkTypes: &[SkillTypeExpr],
    skillTypes: &SkillTypeSet,
    minionTypes: Option<&SkillTypeSet>,
) -> bool {
    unported!("Modules/CalcTools.lua:61-82")
}

/// @pob Modules/CalcTools.lua:86-161 h=b06e1609
/// `imbuedSupport` is always nil at the pin's call sites (CalcActiveSkill.lua:116/134/148, CalcTriggers.lua:233).
pub fn canGrantedEffectSupportActiveSkill(
    grantedEffect: &GrantedEffect,
    activeSkill: &ActiveSkill,
    imbuedSupport: Option<&Val>,
    appliesToGrantedSkills: bool,
) -> bool {
    unported!("Modules/CalcTools.lua:86-161")
}

/// @pob Modules/CalcTools.lua:164-176 h=898869a0
/// `gem` is a gem data table (`data.gems[id]`).
pub fn gemIsType(gem: &pob_gamedata::GemData, r#type: &str, includeTransfigured: bool) -> bool {
    unported!("Modules/CalcTools.lua:164-176")
}

/// @pob Modules/CalcTools.lua:179-189 h=1517a652
pub fn getGemStatRequirement(level: f64, isSupport: bool, multi: f64) -> f64 {
    unported!("Modules/CalcTools.lua:179-189")
}

/// @pob Modules/CalcTools.lua:192-251 h=c4d7b24e
/// → `{ [stat] = value }`.
pub fn buildSkillInstanceStats(
    skillInstance: &SkillEffect,
    grantedEffect: &GrantedEffect,
) -> Table {
    unported!("Modules/CalcTools.lua:192-251")
}

/// @pob Modules/CalcTools.lua:258-273 h=fd97bae8
/// → the converted tags (`modifiers[k]`, k = tag index).
pub fn getConvertedModTags(r#mod: &Mod, multiplier: f64, minionMods: bool) -> Vec<Tag> {
    unported!("Modules/CalcTools.lua:258-273")
}

/// @pob Modules/CalcTools.lua:279-292 h=6b1f70e7
pub fn getGameIdFromGemName(gemName: Option<&str>, dropVaal: bool) -> Option<Name> {
    unported!("Modules/CalcTools.lua:279-292")
}

/// @pob Modules/CalcTools.lua:299-303 h=68ee89c2
/// Lua returns `gemNameId and typeId and gemNameId == typeId` (nil, false or true); callers use it as a condition.
pub fn isGemIdSame(gemName: &str, typeName: &str, dropVaal: bool) -> bool {
    unported!("Modules/CalcTools.lua:299-303")
}
