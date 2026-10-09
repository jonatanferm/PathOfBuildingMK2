//! The ModParser contract (calc doc §6): captures, the "misc" spec tables the
//! pattern lists return, and the pattern lists themselves. Owner: U0 (frozen).
//! The functions `scan` and `parseMod` are declared (with these types) in
//! `scan.rs` / `mod_cache.rs` (MP0).
#![allow(non_snake_case)]

use pob_gamedata::{Mod, ModType, Name, Tag, Value};

/// A capture as Lua sees it: a string, or nil (an unmatched optional group).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Cap<'s> {
    Str(&'s str),
    Nil,
}

/// The arguments a pattern closure receives, in three call shapes (CCR-0006 §5,
/// calc doc §6):
/// * specialModList `f(tonumber(cap[1]), unpack(cap))` (ModParser.lua:6662-6663):
///   #1 = `num`, #k≥2 = `cap[k-2]`;
/// * preFlagList `f(unpack(preFlagCap))` (6680-6682): #1 = `cap[0]` (a string),
///   #k≥2 = `cap[k-1]`;
/// * modTagList `f(tonumber(c1), …)` when `c1` matches `%d+`, else `f(c1, …)`
///   (6698-6712): #1 = `num` or `cap[0]`, #k≥2 = `cap[k-2]`.
///
/// `scan` fills `num = tonumber(cap1)` for specialModList and, for modTagList,
/// whenever cap1 contains a digit.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Caps<'s> {
    pub num: Option<f64>,
    pub cap: [Option<&'s str>; 5],
}

/// The Lua "misc" table returned by form/tag/flag lists. The driver merges
/// every key of every matched table into `misc` (ModParser.lua:6912-6914,
/// later tables win) and reads the keys typed below (U0 census of 6640-6992,
/// docs/port/contracts.md#modspec); any other key goes to `extra`.
#[derive(Default, Clone, Debug)]
pub struct ModSpec {
    pub flags: u32,
    pub keywordFlags: u32,
    pub tag: Option<Tag>,
    pub tagList: Vec<Tag>,
    pub addToMinion: bool,
    pub addToMinionTag: Option<Tag>,
    pub addToSkill: Option<Tag>,
    pub addToAura: bool,
    pub onlyAddToBanners: bool,
    pub applyToEnemy: bool,
    pub actorEnemy: bool,
    pub newAura: bool,
    pub newAuraOnlyAllies: bool,
    pub affectedByAura: bool,
    pub playerTag: Option<Tag>,
    pub playerTagList: Vec<Tag>,
    pub modSuffix: Option<Name>,
    pub modName: Option<Vec<Name>>,
    pub modType: Option<ModType>,
    /// The modNameList entry was a table (shared and mutated in place by the DOUBLED form,
    /// ModParser.lua:6858-6863), not a string (CCR-0006 §1).
    pub modNameIsTable: bool,
    /// The array part `data[1], data[2], …` of a `modExtraTags` table that gives
    /// several mods different tags (ModParser.lua:6888-6907).
    pub entries: Vec<ModSpec>,
    pub extra: Vec<(Name, Value)>,
}

/// A modTagList / preFlagList value: a static spec or a closure of the captures.
pub enum TagVal {
    Spec(ModSpec),
    Fn(fn(&Caps) -> ModSpec),
}

/// A specialModList value: static mods, a closure, or a generated closure
/// (keystones 5943-5955, skill-name loops) capturing data.
pub enum SpecialVal {
    Mods(Vec<Mod>),
    Fn(fn(&Caps) -> Option<Vec<Mod>>),
    Dyn(std::sync::Arc<dyn Fn(&Caps) -> Option<Vec<Mod>> + Send + Sync>),
}

/// One `["pattern"] = value` entry, with the ModParser.lua line that defines it.
pub struct PatternEntry<T> {
    pub pat: &'static str,
    pub val: T,
    pub lua_line: u32,
}

/// A pattern table, built once (`LazyLock`) in source order.
pub struct PatternList<T>(pub Vec<PatternEntry<T>>);

impl<T> Default for PatternList<T> {
    fn default() -> Self {
        PatternList(Vec::new())
    }
}

impl<T> PatternList<T> {
    pub fn len(&self) -> usize {
        self.0.len()
    }
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}
