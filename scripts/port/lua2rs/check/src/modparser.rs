//! Stand-in for `pob-modparser` (calc doc §6 `types.rs` plus the MP0 `helpers.rs` signatures
//! lua2rs output calls; see docs/port/ccr/0006-lua2rs-modparser-shapes.md).

use crate::gamedata::{createMod, Mod, ModType, Tag, Value};
use pob_lua::Name;

pub enum Cap<'s> { Str(&'s str), Nil }
/// Lua closure params: #1 = num (tonumber(cap1)), #k>=2 = caps[k-2] (strings).
pub struct Caps<'s> { pub num: Option<f64>, pub cap: [Option<&'s str>; 5] }

/// The "misc" table (doc §6) plus the CCR-0006 fields.
#[derive(Default, Clone)]
pub struct ModSpec {
    pub flags: u32, pub keywordFlags: u32, pub tag: Option<Tag>, pub tagList: Vec<Tag>,
    pub addToMinion: bool, pub addToMinionTag: Option<Tag>, pub addToSkill: Option<Tag>, pub addToAura: bool,
    pub applyToEnemy: bool, pub newAura: bool, pub newAuraOnlyAllies: bool, pub affectedByAura: bool,
    pub modSuffix: Option<Name>, pub modName: Option<Vec<Name>>, pub modType: Option<ModType>,
    pub extra: Vec<(Name, Value)>,
    // CCR-0006
    pub playerTag: Option<Tag>, pub playerTagList: Vec<Tag>, pub actorEnemy: bool, pub onlyAddToBanners: bool,
}
pub enum TagVal { Spec(ModSpec), Fn(fn(&Caps) -> ModSpec) }
pub enum SpecialVal {
    Mods(Vec<Mod>),
    Fn(fn(&Caps) -> Option<Vec<Mod>>),
    Dyn(std::sync::Arc<dyn Fn(&Caps) -> Option<Vec<Mod>> + Send + Sync>),
}
pub struct PatternEntry<T> { pub pat: &'static str, pub val: T, pub lua_line: u32 }
pub struct PatternList<T>(pub Vec<PatternEntry<T>>);

// ---- helpers.rs [MP0] signatures (bodies are MP0's; these only type-check the calls)

/// ModParser.lua:2011 `local mod = modLib.createMod`.
pub use crate::gamedata::createMod as r#mod;
/// ModParser.lua:2012-2014.
pub fn flag(name: Name, source: Option<&str>, flags: u32, keywordFlags: u32, tags: &[Tag]) -> Mod {
    createMod(name, ModType::FLAG, true, source, flags, keywordFlags, tags)
}
/// ModParser.lua:2055-2072 `options`.
#[derive(Default)]
pub struct TriggerExtraSkillOptions {
    pub noSupports: Option<bool>, pub sourceSkill: Option<String>, pub triggerChance: Option<f64>,
    pub ignoreHexproof: bool, pub onCrit: bool,
}
pub fn triggerExtraSkill(name: &str, level: Option<f64>, options: Option<TriggerExtraSkillOptions>) -> Vec<Mod> { Vec::new() }
pub fn grantedExtraSkill(name: &str, level: Option<f64>, noSupports: Option<bool>) -> Option<Vec<Mod>> { None }
pub fn extraSupport(name: &str, level: Option<f64>, slot: Option<&str>, appliesToGrantedSkills: Option<bool>) -> Option<Vec<Mod>> { None }
/// A helper argument the Lua body uses both as a number and as a string (explodeFunc `amount`).
pub enum LuaArg<'a> { Nil, Num(f64), Str(&'a str) }
pub fn explodeFunc(chance: Option<f64>, amount: LuaArg, r#type: &str, tags: &[Tag]) -> Option<Vec<Mod>> { None }
pub fn dealNoNonDamageType(dmgType: &str, forMinion: bool) -> Option<Vec<Mod>> { None }
pub fn gemIdLookup(name: &str) -> Option<Name> { None }
pub fn dmgTypes(name: &str) -> Option<Name> { None }
