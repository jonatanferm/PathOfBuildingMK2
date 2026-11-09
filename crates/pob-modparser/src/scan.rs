//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 6616-6992: `scan` and the local `parseMod(line, order)`.
//!
//! Owner: MP0.
//!
//! CCR-0014: `scan` takes a `&'static PatternList<T>` (it returns `&'static T`) and returns its
//! captures as an owned [`ScanCaps`]: Lua captures come from `line:lower()`, a string `scan`
//! creates, so they cannot borrow from the caller's `line` (the frozen `Caps<'l>` return type).
//! [`ScanCaps::caps`] lends the frozen [`Caps`] the pattern closures take.
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::generated_lists::{
    baseCostTypes, costTypes, degenTypes, flagTypes, lookup, penTypes, preSkillNameList,
    regenTypes, skillNameList, specialModList, suffixTypes, unsupportedModList, FlagTypeVal,
};
use crate::helpers::{dmgTypes, mod_payload, r#mod, vtab};
use crate::types::*;
use pob_gamedata::{
    tag, JewelFuncKind, JewelFuncSpec, KeywordFlag, Mod, ModType, Name, SkillType, Tag, Value,
    ValueTable,
};
use pob_lua::bit::bor;
use pob_lua::pattern::{find, r#match, Capture};
use pob_lua::string::lower;
use pob_lua::{cat, lua_error, n, tonumber};
use std::sync::{Arc, LazyLock};

/// The captures of a [`scan`] match: `cap1`..`cap5` of `lineLower:find(pattern)` (Lua strings
/// from the lowercased line) and `num = tonumber(cap1)` (the specialModList call shape,
/// ModParser.lua:6663). CCR-0014.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct ScanCaps {
    pub num: Option<f64>,
    pub cap: [Option<String>; 5],
}

impl ScanCaps {
    /// The frozen closure argument (calc doc §6).
    pub fn caps(&self) -> Caps<'_> {
        Caps {
            num: self.num,
            cap: [
                self.cap[0].as_deref(),
                self.cap[1].as_deref(),
                self.cap[2].as_deref(),
                self.cap[3].as_deref(),
                self.cap[4].as_deref(),
            ],
        }
    }

    /// `caps[i]` (1-based, Lua `cap[i]`).
    pub fn get(&self, i1: usize) -> Option<&str> {
        self.cap.get(i1.wrapping_sub(1))?.as_deref()
    }
}

/// A capture as the Lua string `find` returns (a position capture becomes its number's text).
fn cap_string(c: &Capture<'_>) -> String {
    c.to_str().into_owned()
}

// Scan a line for the earliest and longest match from the pattern list
// If a match is found, returns the corresponding value from the pattern list, plus the remainder of the line and a table of captures
/// @pob Modules/ModParser.lua:6616-6638 h=7a700992
/// ModParser.lua:6616 `scan()`: lowest start, then longest end, then longest pattern; full ties →
/// lowest `lua_line`, then the bytewise-smallest pattern (§3.4; PoB's choice depends on `pairs()`
/// order). Returns (value, the line with the match removed, captures); `None` is Lua's
/// `nil, line` (the caller keeps its line). CCR-0014 signature.
pub fn scan<T>(
    line: &str,
    list: &'static PatternList<T>,
    plain: bool,
) -> Option<(&'static T, String, ScanCaps)>
where
    T: Sync + Send + 'static,
{
    // L6617-6619
    let mut best: Option<(usize, usize, &'static PatternEntry<T>)> = None;
    let mut bestCaps: Option<ScanCaps> = None;
    // L6620
    let lineLower = lower(line);
    // L6621: for pattern, patternVal in pairs(patternList) — LUA-ORDER: only full ties depend
    // on the order; they are resolved by (lua_line, pattern)
    for e in &list.0 {
        // L6622
        let Some((index, endIndex, caps)) = find(&lineLower, e.pat, Some(1), plain) else {
            continue;
        };
        // L6623
        let better = match &best {
            None => true,
            Some((bestIndex, bestEndIndex, bestE)) => {
                index < *bestIndex
                    || (index == *bestIndex
                        && (endIndex > *bestEndIndex
                            || (endIndex == *bestEndIndex
                                && (e.pat.len() > bestE.pat.len()
                                    || (e.pat.len() == bestE.pat.len()
                                        && (e.lua_line, e.pat.as_bytes())
                                            < (bestE.lua_line, bestE.pat.as_bytes()))))))
            }
        };
        if better {
            // L6624-6630
            best = Some((index, endIndex, e));
            let mut c = ScanCaps::default();
            for (i, cap) in caps.iter().take(5).enumerate() {
                c.cap[i] = Some(cap_string(cap));
            }
            bestCaps = Some(c);
        }
    }
    // L6633
    let (bestStart, bestEnd, e) = best?;
    let mut caps = bestCaps.unwrap_or_default();
    // tonumber(cap[1]) for the specialModList call shape (6663)
    caps.num = caps.cap[0].as_deref().and_then(tonumber);
    // L6634: line:sub(1, bestStart - 1) .. line:sub(bestEnd + 1, -1)
    let b = line.as_bytes();
    let mut rest: Vec<u8> = Vec::with_capacity(b.len());
    rest.extend_from_slice(&b[..bestStart - 1]);
    rest.extend_from_slice(&b[bestEnd.min(b.len())..]);
    let rest = match String::from_utf8(rest) {
        Ok(s) => s,
        Err(e) => String::from_utf8_lossy(e.as_bytes()).into_owned(),
    };
    Some((&e.val, rest, caps))
}

// ---------------------------------------------------------------------------
// The local parseMod(line, order)
// ---------------------------------------------------------------------------

/// The driver's dynamically typed `modValue` local.
#[derive(Clone, Debug)]
enum MVal {
    Nil,
    Num(f64),
    Str(String),
    Bool(bool),
    /// `{ tonumber(formCap[1]), tonumber(formCap[2]) }` or DOUBLED's `{ 100, 1 }` (holes kept).
    Pair([Option<f64>; 2]),
    /// A `flagTypes` value (FLAG form, through `formCap[1]`).
    Flag(FlagTypeVal),
}

impl MVal {
    /// `-modValue` (arithmetic on a string coerces it; anything else raises).
    fn neg(&self) -> MVal {
        match self {
            MVal::Num(x) => MVal::Num(-x),
            MVal::Str(s) => match tonumber(s) {
                Some(x) => MVal::Num(-x),
                None => {
                    lua_error!("attempt to perform arithmetic on local 'modValue' (a string value)")
                }
            },
            MVal::Nil => {
                lua_error!("attempt to perform arithmetic on local 'modValue' (a nil value)")
            }
            MVal::Bool(_) => {
                lua_error!("attempt to perform arithmetic on local 'modValue' (a boolean value)")
            }
            MVal::Pair(_) | MVal::Flag(_) => {
                lua_error!("attempt to perform arithmetic on local 'modValue' (a table value)")
            }
        }
    }

    fn pair_value(p: &[Option<f64>; 2]) -> Value {
        // a table constructor `{ a, b }`: a trailing nil is no element; a leading nil is a hole
        let mut array = Vec::new();
        match p {
            [Some(a), Some(b)] => {
                array.push(Value::Num(*a));
                array.push(Value::Num(*b));
            }
            [Some(a), None] => array.push(Value::Num(*a)),
            [None, Some(_)] | [None, None] => {}
        }
        let mut t = ValueTable {
            array,
            fields: Vec::new(),
        };
        if let [None, Some(b)] = p {
            t.fields.push((n!("#2"), Value::Num(*b)));
        }
        Value::Table(Arc::new(t))
    }

    /// The mod's `value`: `type(modValue) == "table" and modValue[i] or modValue`.
    fn value_for(&self, i1: usize) -> Value {
        match self {
            MVal::Nil => Value::Nil,
            MVal::Num(x) => Value::Num(*x),
            MVal::Str(s) => Value::from(s.as_str()),
            MVal::Bool(b) => Value::Bool(*b),
            MVal::Pair(p) => match p.get(i1 - 1).copied().flatten() {
                Some(x) => Value::Num(x),
                // LUA-QUIRK: modValue[i] is nil, so `and/or` yields the table itself
                None => Self::pair_value(p),
            },
            // FLAG form replaces modValue before use; a flagTypes table as a value
            MVal::Flag(FlagTypeVal::Name(s)) => Value::from(*s),
            MVal::Flag(FlagTypeVal::Table {
                name,
                value,
                r#type,
            }) => vtab(vec![
                (n!("name"), Value::from(*name)),
                (n!("value"), Value::Num(*value)),
                (n!("type"), Value::from(r#type.as_str())),
            ]),
        }
    }
}

/// The driver's `modName` local.
#[derive(Clone, Debug)]
enum MName {
    Nil,
    Str(Name),
    /// A table of names built by the driver or copied from regen/degen/cost types.
    Table(Vec<Name>),
    /// A modNameList entry (the shared Lua table; see [`doubled_override`]).
    Spec(&'static ModSpec),
}

/// The names `ipairs(modName)` sees for a modNameList entry, including a DOUBLED overwrite of
/// `modName[2]` made by an earlier parse (ModParser.lua:6860).
fn spec_names(spec: &'static ModSpec) -> Vec<Name> {
    let mut names: Vec<Name> = spec.modName.clone().unwrap_or_default();
    if spec.modNameIsTable {
        if let Some(n2) = doubled_override(spec) {
            if names.len() >= 2 {
                names[1] = n2;
            } else if names.len() == 1 {
                names.push(n2);
            }
        }
    }
    names
}

thread_local! {
    /// LUA-ALIAS: `modName[2] = "Multiplier:"..modName[1].."Doubled"` (6860) writes into the
    /// modNameList table itself, so the overwrite persists for every later parse in this Lua
    /// state (CCR-0006 §1). Keyed by the entry's address; one map per thread (one PoB VM per
    /// thread).
    static DOUBLED_OVERRIDES: std::cell::RefCell<ahash::AHashMap<usize, Name>> =
        std::cell::RefCell::new(ahash::AHashMap::new());
}

fn doubled_override(spec: &'static ModSpec) -> Option<Name> {
    DOUBLED_OVERRIDES.with(|m| m.borrow().get(&(spec as *const ModSpec as usize)).copied())
}

fn set_doubled_override(spec: &'static ModSpec, name: Name) {
    DOUBLED_OVERRIDES.with(|m| {
        m.borrow_mut().insert(spec as *const ModSpec as usize, name);
    });
}

/// The driver's `modType` local.
#[derive(Clone, Copy, Debug)]
enum MType {
    One(ModType),
    /// DOUBLED's `{ "MORE", "OVERRIDE" }`.
    Pair([ModType; 2]),
}

/// A matched list value as a `misc` source table (`pairs` copies every key).
fn merge_misc(misc: &mut ModSpec, data: &ModSpec) {
    if data.tag.is_some() {
        misc.tag = data.tag.clone();
    }
    if !data.tagList.is_empty() {
        misc.tagList = data.tagList.clone();
    }
    if data.addToMinion {
        misc.addToMinion = true;
    }
    if data.addToMinionTag.is_some() {
        misc.addToMinionTag = data.addToMinionTag.clone();
    }
    if data.addToSkill.is_some() {
        misc.addToSkill = data.addToSkill.clone();
    }
    if data.addToAura {
        misc.addToAura = true;
    }
    if data.onlyAddToBanners {
        misc.onlyAddToBanners = true;
    }
    if data.applyToEnemy {
        misc.applyToEnemy = true;
    }
    if data.actorEnemy {
        misc.actorEnemy = true;
    }
    if data.newAura {
        misc.newAura = true;
    }
    if data.newAuraOnlyAllies {
        misc.newAuraOnlyAllies = true;
    }
    if data.affectedByAura {
        misc.affectedByAura = true;
    }
    if data.playerTag.is_some() {
        misc.playerTag = data.playerTag.clone();
    }
    if !data.playerTagList.is_empty() {
        misc.playerTagList = data.playerTagList.clone();
    }
    if data.modSuffix.is_some() {
        misc.modSuffix = data.modSuffix;
    }
    if data.modName.is_some() {
        misc.modName = data.modName.clone();
    }
    if data.modType.is_some() {
        misc.modType = data.modType;
    }
    for (k, v) in &data.extra {
        if let Some(slot) = misc.extra.iter_mut().find(|(mk, _)| mk == k) {
            slot.1 = v.clone();
        } else {
            misc.extra.push((*k, v.clone()));
        }
    }
}

/// Lua `table.insert(t, unpack(xs))` on a mod's tag array.
fn insert_unpacked(tags: &mut smallvec::SmallVec<[Tag; 1]>, xs: &[Tag]) {
    match xs.len() {
        1 => tags.push(xs[0].clone()),
        // insert(t, pos, v) with a table as pos
        2 => lua_error!("bad argument #2 to 'insert' (number expected, got table)"),
        _ => lua_error!("wrong number of arguments to 'insert'"),
    }
}

/// `{ func = patternVal.func(cap1, …), type = patternVal.type }` / the jewelFuncList table: the
/// closure as data (`args` = the captures it was called with; none for an exact key).
fn jewel_func_value(spec: &JewelFuncSpec, caps: Option<&ScanCaps>) -> Value {
    let mut s = spec.clone();
    if let Some(c) = caps {
        s.args = c
            .cap
            .iter()
            .flatten()
            .map(|x| Value::from(x.as_str()))
            .collect();
    }
    let ty = s.r#type.unwrap_or_else(|| match s.kind {
        JewelFuncKind::Other => n!("Other"),
        JewelFuncKind::Self_ => n!("Self"),
        JewelFuncKind::SelfUnalloc => n!("SelfUnalloc"),
        JewelFuncKind::Threshold => n!("Threshold"),
    });
    s.r#type = Some(ty);
    vtab(vec![
        (n!("func"), Value::JewelFunc(Arc::new(s))),
        (n!("type"), Value::from(ty)),
    ])
}

/// `jewelFuncList[lineLower]` (an index over MP7's list).
static JEWEL_FUNC_INDEX: LazyLock<ahash::AHashMap<&'static str, usize>> = LazyLock::new(|| {
    let mut m = ahash::AHashMap::new();
    for (i, (k, _)) in crate::jewel_func_list::jewelFuncList.iter().enumerate() {
        m.insert(*k, i);
    }
    m
});

/// `modTag(tonumber(tagCap[1]), unpack(tagCap))` if `tagCap[1]:match("%d+")`, else
/// `modTag(tagCap[1], unpack(tagCap))` (the modTagList call shape, CCR-0006 §5).
fn call_mod_tag(val: &'static TagVal, tagCap: &ScanCaps) -> ModSpec {
    match val {
        TagVal::Spec(s) => s.clone(),
        TagVal::Fn(f) => {
            let Some(c1) = tagCap.cap[0].as_deref() else {
                lua_error!("attempt to index field '?' (a nil value)");
            };
            let mut caps = tagCap.clone();
            caps.num = if r#match(c1, "%d+", None).is_some() {
                tonumber(c1)
            } else {
                None
            };
            f(&caps.caps())
        }
    }
}

/// The pattern tables `parseMod` reads that other units own (Rust-only plumbing): the statics
/// by default ([`ParserLists::statics`]); tests may substitute other builds of the same tables.
pub struct ParserLists {
    pub jewelFuncList: &'static [(&'static str, JewelFuncSpec)],
    pub specialModList: &'static PatternList<SpecialVal>,
    pub preFlagList: &'static PatternList<TagVal>,
    pub formList: &'static PatternList<Name>,
    pub modTagList: &'static PatternList<TagVal>,
    pub modNameList: &'static PatternList<ModSpec>,
    pub modFlagList: &'static PatternList<ModSpec>,
}

impl ParserLists {
    /// The crate's own tables.
    pub fn statics() -> ParserLists {
        ParserLists {
            jewelFuncList: crate::jewel_func_list::jewelFuncList.as_slice(),
            specialModList: &specialModList,
            preFlagList: &crate::pre_flag_list::preFlagList,
            formList: &crate::form_list::formList,
            modTagList: &crate::mod_tag_list::modTagList,
            modNameList: &crate::mod_name_list::modNameList,
            modFlagList: &crate::mod_flag_list::modFlagList,
        }
    }
}

/// @pob Modules/ModParser.lua:6640-6992 h=c2f6fd73
/// The local `parseMod(line, order)` (ModParser.lua:6640): one pass. → (mods, unparsed remainder).
pub fn parseModPass(line: &str, order: u8) -> (Option<Vec<Mod>>, Option<String>) {
    parseModPassWith(&ParserLists::statics(), line, order)
}

/// [`parseModPass`] over the given tables.
pub fn parseModPassWith(
    lists: &ParserLists,
    line: &str,
    order: u8,
) -> (Option<Vec<Mod>>, Option<String>) {
    let mut line: String = line.to_owned();
    // -- Check if this is a special modifier
    // L6642
    let lineLower = lower(&line);
    // L6643: for pattern, patternVal in pairs(jewelFuncList) — LUA-ORDER: the first pattern
    // whose match has a capture wins (the parse oracle records no line with two candidates)
    for (pattern, patternVal) in lists.jewelFuncList.iter() {
        // L6644
        if let Some((_, _, caps)) = find(&lineLower, pattern, Some(1), false) {
            // L6645
            if let Some(cap1) = caps.first() {
                let mut c = ScanCaps::default();
                c.cap[0] = Some(cap_string(cap1));
                for (i, cap) in caps.iter().take(5).enumerate().skip(1) {
                    c.cap[i] = Some(cap_string(cap));
                }
                // L6646
                return (
                    Some(vec![r#mod(
                        n!("JewelFunc"),
                        ModType::LIST,
                        jewel_func_value(patternVal, Some(&c)),
                        None,
                        0,
                        0,
                        &[],
                    )]),
                    None,
                );
            }
        }
    }
    // L6649
    let jewelFunc = if std::ptr::eq(
        lists.jewelFuncList,
        crate::jewel_func_list::jewelFuncList.as_slice(),
    ) {
        JEWEL_FUNC_INDEX
            .get(lineLower.as_str())
            .map(|&i| &lists.jewelFuncList[i].1)
    } else {
        lists
            .jewelFuncList
            .iter()
            .find(|(k, _)| *k == lineLower)
            .map(|(_, v)| v)
    };
    // L6650
    if let Some(jewelFunc) = jewelFunc {
        // L6651
        return (
            Some(vec![r#mod(
                n!("JewelFunc"),
                ModType::LIST,
                jewel_func_value(jewelFunc, None),
                None,
                0,
                0,
                &[],
            )]),
            None,
        );
    }
    // L6653: clusterJewelSkills (MP7's table, CCR-0014 §2)
    let clusterJewelSkill = crate::jewel_func_list::clusterJewelSkills.get(lineLower.as_str());
    // L6654
    if let Some(clusterJewelSkill) = clusterJewelSkill {
        // L6655
        return (Some(clusterJewelSkill.clone()), None);
    }
    // L6657
    if unsupportedModList(&lineLower) {
        // L6658
        return (Some(Vec::new()), Some(line));
    }
    // L6660
    let special = scan(&line, lists.specialModList, false);
    // L6661
    if let Some((specialMod, specialLine, cap)) = &special {
        if specialLine.is_empty() {
            // L6662
            match specialMod {
                // L6663: specialMod(tonumber(cap[1]), unpack(cap))
                SpecialVal::Fn(f) => return (f(&cap.caps()), None),
                SpecialVal::Dyn(f) => return (f(&cap.caps()), None),
                // L6665: copyTable(specialMod)
                SpecialVal::Mods(m) => return (Some(m.clone()), None),
            }
        }
    }

    // -- Check for add-to-cluster-jewel special
    // L6670
    let addToCluster = r#match(&line, "^Added Small Passive Skills also grant: (.+)$", None)
        .map(|c| cap_string(&c[0]));
    // L6671
    if let Some(addToCluster) = addToCluster {
        // L6672
        return (
            Some(vec![r#mod(
                n!("AddToClusterJewelNode"),
                ModType::LIST,
                Value::from(addToCluster),
                None,
                0,
                0,
                &[],
            )]),
            None,
        );
    }

    // L6675
    line.push(' ');

    // -- Check for a flag/tag specification at the start of the line
    // L6678-6679
    let mut preFlag: Option<ModSpec> = None;
    if let Some((v, rest, preFlagCap)) = scan(&line, lists.preFlagList, false) {
        line = rest;
        // L6680
        preFlag = Some(match v {
            TagVal::Spec(s) => s.clone(),
            // L6681: preFlag(unpack(preFlagCap))
            TagVal::Fn(f) => f(&preFlagCap.caps()),
        });
    }

    // -- Check for skill name at the start of the line
    // L6685-6686
    let mut skillTag: Option<&'static ModSpec> = None;
    if let Some((v, rest, _)) = scan(&line, *preSkillNameList, false) {
        skillTag = Some(v);
        line = rest;
    }

    // -- Scan for modifier form
    // L6689-6690
    let (modForm, formCap): (&'static Name, ScanCaps) = match scan(&line, lists.formList, false) {
        Some((v, rest, caps)) => {
            line = rest;
            (v, caps)
        }
        None => {
            // L6692
            return (None, Some(line));
        }
    };
    let modForm: &str = modForm.as_str();

    // -- Check for tags (per-charge, conditionals)
    // L6696-6697
    let mut modTag: Option<ModSpec> = None;
    let mut modTag2: Option<ModSpec> = None;
    if let Some((v, rest, tagCap)) = scan(&line, lists.modTagList, false) {
        line = rest;
        // L6698-6703
        modTag = Some(call_mod_tag(v, &tagCap));
    }
    // L6705
    if modTag.is_some() {
        // L6706
        if let Some((v, rest, tagCap)) = scan(&line, lists.modTagList, false) {
            line = rest;
            // L6707-6712
            modTag2 = Some(call_mod_tag(v, &tagCap));
        }
    }
    // -- Scan for modifier name and skill name
    // L6716
    let mut modName: MName;
    // L6717
    if order == 2 && skillTag.is_none() {
        // L6718
        if let Some((v, rest, _)) = scan(&line, *skillNameList, false) {
            skillTag = Some(v);
            line = rest;
        }
    }
    // a modNameList result (`modName, line = scan(line, modNameList, true)`)
    let scanModNameList = |line: &mut String| -> MName {
        match scan(line, lists.modNameList, true) {
            Some((v, rest, _)) => {
                *line = rest;
                MName::Spec(v)
            }
            None => MName::Nil,
        }
    };
    // `modName, line = scan(line, <types>, true)` on a regen/cost table
    let value_name = |v: &Value| -> MName {
        match v {
            Value::Str(s) => MName::Str(Name::from(&**s)),
            Value::Table(t) => MName::Table(
                t.array
                    .iter()
                    .map(|x| Name::from(x.as_str().unwrap_or_default()))
                    .collect(),
            ),
            _ => MName::Nil,
        }
    };
    // the FLAG form's `formCap[1]` (a flagTypes value)
    let mut formCap1Flag: Option<FlagTypeVal> = None;
    // L6720
    if modForm == "PEN" {
        // L6721
        match scan(&line, &penTypes, true) {
            Some((v, rest, _)) => {
                modName = MName::Str(*v);
                line = rest;
            }
            None => {
                // L6723
                return (Some(Vec::new()), Some(line));
            }
        }
        // L6725-6726
        let _ = scanModNameList(&mut line);
    } else if modForm == "BASECOST" {
        // L6728
        match scan(&line, &baseCostTypes, true) {
            Some((v, rest, _)) => {
                modName = value_name(v);
                line = rest;
            }
            None => {
                // L6730
                return (Some(Vec::new()), Some(line));
            }
        }
        // L6732-6733
        let _ = scanModNameList(&mut line);
    } else if modForm == "TOTALCOST" {
        // L6735
        match scan(&line, &costTypes, true) {
            Some((v, rest, _)) => {
                modName = value_name(v);
                line = rest;
            }
            None => {
                // L6737
                return (Some(Vec::new()), Some(line));
            }
        }
        // L6739-6740
        let _ = scanModNameList(&mut line);
    } else if modForm == "FLAG" {
        // L6742: formCap[1], line = scan(line, flagTypes, false)
        match scan(&line, &flagTypes, false) {
            Some((v, rest, _)) => {
                formCap1Flag = Some(v.clone());
                line = rest;
            }
            None => {
                // L6743-6744
                return (None, Some(line));
            }
        }
        // L6746
        modName = scanModNameList(&mut line);
    } else {
        // L6748
        modName = scanModNameList(&mut line);
    }
    // L6750
    if order == 1 && skillTag.is_none() {
        // L6751
        if let Some((v, rest, _)) = scan(&line, *skillNameList, false) {
            skillTag = Some(v);
            line = rest;
        }
    }

    // -- Scan for flags
    // L6755-6756
    let mut modFlag: Option<ModSpec> = None;
    if let Some((v, rest, _)) = scan(&line, lists.modFlagList, true) {
        modFlag = Some(v.clone());
        line = rest;
    }

    // -- Find modifier value and type according to form
    // L6760: local modValue = tonumber(formCap[1]) or formCap[1]
    let mut modValue: MVal = if let Some(f) = &formCap1Flag {
        MVal::Flag(f.clone())
    } else {
        match formCap.cap[0].as_deref() {
            Some(s) => match tonumber(s) {
                Some(x) => MVal::Num(x),
                None => MVal::Str(s.to_owned()),
            },
            None => MVal::Nil,
        }
    };
    // L6761
    let mut modType: MType = MType::One(ModType::BASE);
    // L6762
    let mut modSuffix: Option<Name> = None;
    // L6763
    let mut modExtraTags: Option<ModSpec> = None;
    // `modSuffix, line = scan(line, suffixTypes, true)`
    let scanSuffix = |line: &mut String| -> Option<Name> {
        let (v, rest, _) = scan(line, &suffixTypes, true)?;
        *line = rest;
        Some(*v)
    };
    // `{ tag = { type = "Condition", var = "{Hand}Attack" } }`
    let handAttack = || ModSpec {
        tag: Some(tag!({ type = Condition, var = "{Hand}Attack" })),
        ..Default::default()
    };
    // `local damageType = dmgTypes[formCap[k]]`
    let capDmgType = |formCap: &ScanCaps, k: usize| formCap.get(k).and_then(dmgTypes);
    // L6764
    if modForm == "INC" {
        // L6765
        modType = MType::One(ModType::INC);
    } else if modForm == "RED" {
        // L6767
        modValue = modValue.neg();
        // L6768
        modType = MType::One(ModType::INC);
    } else if modForm == "MORE" {
        // L6770
        modType = MType::One(ModType::MORE);
    } else if modForm == "LESS" {
        // L6772
        modValue = modValue.neg();
        // L6773
        modType = MType::One(ModType::MORE);
    } else if modForm == "BASE" {
        // L6775
        modSuffix = scanSuffix(&mut line);
    } else if modForm == "GAIN" {
        // L6777
        modType = MType::One(ModType::BASE);
        // L6778
        modSuffix = scanSuffix(&mut line);
    } else if modForm == "LOSE" {
        // L6780
        modValue = modValue.neg();
        // L6781
        modType = MType::One(ModType::BASE);
        // L6782
        modSuffix = scanSuffix(&mut line);
    } else if modForm == "GRANTS" {
        // -- local
        // L6784
        modType = MType::One(ModType::BASE);
        // L6785: modFlag = modFlag
        // L6786
        modExtraTags = Some(handAttack());
        // L6787
        modSuffix = scanSuffix(&mut line);
    } else if modForm == "GRANTS_GLOBAL" {
        // L6789
        modType = MType::One(ModType::BASE);
        // L6790: modFlag = modFlag
        // L6791
        modSuffix = scanSuffix(&mut line);
    } else if modForm == "REMOVES" {
        // -- local
        // L6793
        modValue = modValue.neg();
        // L6794
        modType = MType::One(ModType::BASE);
        // L6795: modFlag = modFlag
        // L6796
        modExtraTags = Some(handAttack());
        // L6797
        modSuffix = scanSuffix(&mut line);
    } else if modForm == "CHANCE" {
    } else if modForm == "REGENPERCENT" {
        // L6800
        modName = lookup(&regenTypes, formCap.get(2)).map_or(MName::Nil, value_name);
        // L6801
        modSuffix = Some(n!("Percent"));
    } else if modForm == "REGENFLAT" {
        // L6803
        modName = lookup(&regenTypes, formCap.get(2)).map_or(MName::Nil, value_name);
    } else if modForm == "DEGENPERCENT" {
        // L6805: modValue = modValue
        // L6806
        modName = lookup(&degenTypes, formCap.get(2)).map_or(MName::Nil, value_name);
        // L6807
        modSuffix = Some(n!("Percent"));
    } else if modForm == "DEGENFLAT" {
        // L6809: modValue = modValue
        // L6810
        modName = lookup(&degenTypes, formCap.get(2)).map_or(MName::Nil, value_name);
    } else if modForm == "DEGEN" {
        // L6812
        let Some(damageType) = capDmgType(&formCap, 2) else {
            // L6814
            return (Some(Vec::new()), Some(line));
        };
        // L6816
        modName = MName::Str(cat!(damageType, "Degen"));
        // L6817
        modSuffix = Some(n!(""));
    } else if modForm == "DMG" {
        // L6819
        let Some(damageType) = capDmgType(&formCap, 3) else {
            // L6821
            return (Some(Vec::new()), Some(line));
        };
        // L6823
        modValue = MVal::Pair([
            formCap.get(1).and_then(tonumber),
            formCap.get(2).and_then(tonumber),
        ]);
        // L6824
        modName = MName::Table(vec![cat!(damageType, "Min"), cat!(damageType, "Max")]);
    } else if modForm == "DMGATTACKS" {
        // L6826
        let Some(damageType) = capDmgType(&formCap, 3) else {
            // L6828
            return (Some(Vec::new()), Some(line));
        };
        // L6830
        modValue = MVal::Pair([
            formCap.get(1).and_then(tonumber),
            formCap.get(2).and_then(tonumber),
        ]);
        // L6831
        modName = MName::Table(vec![cat!(damageType, "Min"), cat!(damageType, "Max")]);
        // L6832
        if modFlag.is_none() {
            modFlag = Some(ModSpec {
                keywordFlags: KeywordFlag::Attack,
                ..Default::default()
            });
        }
    } else if modForm == "DMGSPELLS" {
        // L6834
        let Some(damageType) = capDmgType(&formCap, 3) else {
            // L6836
            return (Some(Vec::new()), Some(line));
        };
        // L6838
        modValue = MVal::Pair([
            formCap.get(1).and_then(tonumber),
            formCap.get(2).and_then(tonumber),
        ]);
        // L6839
        modName = MName::Table(vec![cat!(damageType, "Min"), cat!(damageType, "Max")]);
        // L6840
        if modFlag.is_none() {
            modFlag = Some(ModSpec {
                keywordFlags: KeywordFlag::Spell,
                ..Default::default()
            });
        }
    } else if modForm == "DMGBOTH" {
        // L6842
        let Some(damageType) = capDmgType(&formCap, 3) else {
            // L6844
            return (Some(Vec::new()), Some(line));
        };
        // L6846
        modValue = MVal::Pair([
            formCap.get(1).and_then(tonumber),
            formCap.get(2).and_then(tonumber),
        ]);
        // L6847
        modName = MName::Table(vec![cat!(damageType, "Min"), cat!(damageType, "Max")]);
        // L6848
        if modFlag.is_none() {
            modFlag = Some(ModSpec {
                keywordFlags: bor(KeywordFlag::Attack, KeywordFlag::Spell),
                ..Default::default()
            });
        }
    } else if modForm == "FLAG" {
        // L6850-6852
        match &modValue {
            MVal::Flag(FlagTypeVal::Table {
                name,
                value,
                r#type,
            }) => {
                modName = MName::Str(*name);
                modType = MType::One(*r#type);
                modValue = MVal::Num(*value);
            }
            MVal::Flag(FlagTypeVal::Name(name)) => {
                modName = MName::Str(*name);
                modType = MType::One(ModType::FLAG);
                modValue = MVal::Bool(true);
            }
            // formCap[1] is always a flagTypes value here (6742-6745)
            _ => unreachable!("FLAG form without a flagTypes value"),
        }
    } else if modForm == "OVERRIDE" {
        // L6854
        modType = MType::One(ModType::OVERRIDE);
    } else if modForm == "DOUBLED" {
        // L6856
        let modNameString: Option<Name>;
        // -- Need to assign two mod names. One actual "MORE" mod and one multiplier with a limit to prevent applying more than once
        // L6858
        match &modName {
            MName::Spec(spec) if spec.modNameIsTable => {
                // L6859
                let names = spec_names(spec);
                let Some(first) = names.first().copied() else {
                    lua_error!("attempt to concatenate local 'modNameString' (a nil value)");
                };
                modNameString = Some(first);
                // L6860: LUA-ALIAS — writes modName[2] of the shared modNameList table
                set_doubled_override(spec, cat!("Multiplier:", first, "Doubled"));
            }
            MName::Table(names) => {
                // a table built by the driver (regen/cost types are not DOUBLED forms)
                let mut names = names.clone();
                let Some(first) = names.first().copied() else {
                    lua_error!("attempt to concatenate local 'modNameString' (a nil value)");
                };
                modNameString = Some(first);
                let n2 = cat!("Multiplier:", first, "Doubled");
                if names.len() >= 2 {
                    names[1] = n2;
                } else {
                    names.push(n2);
                }
                modName = MName::Table(names);
            }
            _ => {
                // L6862: modNameString = modName (a string entry or nil)
                let s = match &modName {
                    MName::Str(s) => Some(*s),
                    MName::Spec(spec) => spec.modName.as_ref().and_then(|v| v.first().copied()),
                    _ => None,
                };
                modNameString = s;
                // L6863: modName = modName and {modName, "Multiplier:" .. modName .. "Doubled"}
                modName = match s {
                    Some(s) => MName::Table(vec![s, cat!("Multiplier:", s, "Doubled")]),
                    None => MName::Nil,
                };
            }
        }
        // L6865
        if !matches!(modName, MName::Nil) {
            let modNameString = modNameString.unwrap_or_default();
            // L6866
            modType = MType::Pair([ModType::MORE, ModType::OVERRIDE]);
            // L6867
            modValue = MVal::Pair([Some(100.0), Some(1.0)]);
            // L6868-6869: modExtraTags = { tag = true }; modExtraTags[1] = { tag = … }
            modExtraTags = Some(ModSpec {
                // stands for `tag = true` (never read as a tag: data[1].tag wins at 6880)
                tag: Some(Tag::default()),
                entries: vec![ModSpec {
                    tag: Some(tag!({
                        type = Multiplier,
                        var = cat!(modNameString, "Doubled"),
                        globalLimit = 100.0,
                        globalLimitKey = cat!(modNameString, "DoubledLimit")
                    })),
                    ..Default::default()
                }],
                ..Default::default()
            });
        }
    }

    // L6873
    if matches!(modName, MName::Nil) {
        // L6874
        return (Some(Vec::new()), Some(line));
    }

    // -- Combine flags and tags
    // L6878
    let mut flags: u32 = 0;
    // L6879
    let mut keywordFlags: u32 = 0;
    // L6880
    let mut tagList: Vec<Tag> = Vec::new();
    // -- need this in case of multiple mods with separate tags
    // L6881
    let mut modTagList: Option<Vec<Vec<Tag>>> = None;
    // L6882
    let mut misc = ModSpec::default();
    // L6883: for _, data in pairs({ modName, preFlag, modFlag, modTag, modTag2, skillTag, modExtraTags })
    // (an array with holes: pairs visits the non-nil slots in index order)
    let modNameSpec: Option<&ModSpec> = match &modName {
        MName::Spec(s) => Some(s),
        _ => None,
    };
    let sources: [Option<&ModSpec>; 7] = [
        modNameSpec,
        preFlag.as_ref(),
        modFlag.as_ref(),
        modTag.as_ref(),
        modTag2.as_ref(),
        skillTag,
        modExtraTags.as_ref(),
    ];
    // a regen/cost/DMG names table is a table without flags or tags: only its array keys reach
    // misc, and misc reads none of them
    for data in sources.into_iter().flatten() {
        // L6884: if type(data) == "table" then
        // L6885
        flags = bor(flags, data.flags);
        // L6886
        keywordFlags = bor(keywordFlags, data.keywordFlags);
        // L6887
        if data.tag.is_some() {
            // -- Special handling for multiple mods with different tags within the same modExtraTags
            // L6888
            if data.entries.first().is_some_and(|e| e.tag.is_some()) {
                // L6889
                let mut l: Vec<Vec<Tag>> = Vec::new();
                // L6890
                for entry in &data.entries {
                    // L6891
                    let mut t = Vec::new();
                    // L6892
                    if let Some(tag) = &entry.tag {
                        t.push(tag.clone());
                    }
                    l.push(t);
                }
                modTagList = Some(l);
            } else {
                // L6895
                tagList.push(data.tag.clone().unwrap_or_default());
            }
        } else if !data.tagList.is_empty() {
            // L6897
            // -- Special handling for multiple mods with different tags within the same modExtraTags
            // L6898
            if data.entries.first().is_some_and(|e| !e.tagList.is_empty()) {
                // L6899
                let mut l: Vec<Vec<Tag>> = Vec::new();
                // L6900
                for entry in &data.entries {
                    // L6901-6904
                    l.push(entry.tagList.clone());
                }
                modTagList = Some(l);
            } else {
                // L6907-6909
                for tag in &data.tagList {
                    tagList.push(tag.clone());
                }
            }
        }
        // L6912-6914
        merge_misc(&mut misc, data);
    }

    // -- Generate modifier list
    // L6919
    let nameList: Vec<Name> = match &modName {
        MName::Str(s) => vec![*s],
        MName::Table(v) => v.clone(),
        MName::Spec(spec) => spec_names(spec),
        MName::Nil => unreachable!(),
    };
    // L6920
    let mut modList: Vec<Mod> = Vec::new();
    // L6921
    for (i0, name) in nameList.iter().enumerate() {
        let i = i0 + 1;
        // L6922-6929
        let suffix: Name = modSuffix.or(misc.modSuffix).unwrap_or_default();
        let r#type = match modType {
            MType::One(t) => t,
            MType::Pair(p) => match p.get(i0) {
                Some(t) => *t,
                // LUA-QUIRK: modType[i] is nil for a 3rd name, so `and/or` stores the
                // { "MORE", "OVERRIDE" } table as the type; ModType cannot hold a table
                None => ModType::Nil,
            },
        };
        let mut m = Mod {
            name: cat!(*name, suffix),
            r#type,
            value: modValue.value_for(i),
            flags,
            keywordFlags,
            source: None,
            tags: tagList.iter().cloned().collect(),
            extra: None,
        };
        // L6930
        if let Some(l) = &modTagList {
            if let Some(t) = l.get(i0) {
                insert_unpacked(&mut m.tags, t);
            }
        }
        modList.push(m);
    }
    // L6932
    if !modList.is_empty() {
        // -- Special handling for various modifier types
        // L6934
        if misc.addToAura {
            // L6935
            if misc.onlyAddToBanners {
                // L6936
                for effectMod in modList.iter_mut() {
                    // L6937
                    *effectMod = r#mod(
                        n!("ExtraAuraEffect"),
                        ModType::LIST,
                        mod_payload(effectMod.clone()),
                        None,
                        0,
                        0,
                        &[tag!({ type = SkillType, skillType = SkillType::Banner })],
                    );
                }
            } else {
                // -- Modifiers that add effects to your auras
                // L6941
                for effectMod in modList.iter_mut() {
                    // L6942
                    *effectMod = r#mod(
                        n!("ExtraAuraEffect"),
                        ModType::LIST,
                        mod_payload(effectMod.clone()),
                        None,
                        0,
                        0,
                        &[],
                    );
                }
            }
        } else if misc.newAura {
            // -- Modifiers that add extra auras
            // L6947
            for effectMod in modList.iter_mut() {
                // L6948-6952
                let mut inner = effectMod.clone();
                let tagList: Vec<Tag> = std::mem::take(&mut inner.tags).into_vec();
                // L6953
                *effectMod = r#mod(
                    n!("ExtraAura"),
                    ModType::LIST,
                    vtab(vec![
                        (n!("mod"), Value::Mod(Arc::new(inner))),
                        (
                            n!("onlyAllies"),
                            if misc.newAuraOnlyAllies {
                                Value::Bool(true)
                            } else {
                                Value::Nil
                            },
                        ),
                    ]),
                    None,
                    0,
                    0,
                    &tagList,
                );
            }
        } else if misc.addToMinion {
            // -- Minion modifiers
            // L6957
            for effectMod in modList.iter_mut() {
                // L6958
                let mut tagList: Vec<Tag> = Vec::new();
                // L6959
                if let Some(t) = &misc.playerTag {
                    tagList.push(t.clone());
                }
                // L6960
                if let Some(t) = &misc.addToMinionTag {
                    tagList.push(t.clone());
                }
                // L6961-6965
                for t in &misc.playerTagList {
                    tagList.push(t.clone());
                }
                // L6966
                *effectMod = r#mod(
                    n!("MinionModifier"),
                    ModType::LIST,
                    mod_payload(effectMod.clone()),
                    None,
                    0,
                    0,
                    &tagList,
                );
            }
        } else if let Some(addToSkill) = &misc.addToSkill {
            // -- Skill enchants or socketed gem modifiers that add additional effects
            // L6970
            for effectMod in modList.iter_mut() {
                // L6971
                *effectMod = r#mod(
                    n!("ExtraSkillMod"),
                    ModType::LIST,
                    mod_payload(effectMod.clone()),
                    None,
                    0,
                    0,
                    std::slice::from_ref(addToSkill),
                );
            }
        } else if misc.applyToEnemy {
            // L6974
            for effectMod in modList.iter_mut() {
                // L6975
                let mut tagList: Vec<Tag> = Vec::new();
                // L6976
                if let Some(t) = &misc.playerTag {
                    tagList.push(t.clone());
                }
                // L6977-6981
                for t in &misc.playerTagList {
                    tagList.push(t.clone());
                }
                // L6982
                let mut newMod = effectMod.clone();
                // L6983
                if !effectMod.tags.is_empty() && misc.actorEnemy {
                    // L6984-6985: newMod = copyTable(effectMod); newMod[1]["actor"] = "enemy"
                    newMod.tags[0].actor = Some(n!("enemy"));
                }
                // L6987
                *effectMod = r#mod(
                    n!("EnemyModifier"),
                    ModType::LIST,
                    mod_payload(newMod),
                    None,
                    0,
                    0,
                    &tagList,
                );
            }
        }
    }
    // L6991: return modList, line:match("%S") and line
    let hasNonSpace = r#match(&line, "%S", None).is_some();
    let extra = hasNonSpace.then_some(line);
    (Some(modList), extra)
}
