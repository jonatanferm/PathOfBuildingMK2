//! @pob-file Modules/CalcFormat.lua @16de4b82d
//! @pob-file Modules/Common.lua @16de4b82d
//! Modules/CalcFormat.lua: calc section cell formatting. Owner: CT.
//!
//! `formatCalcVal` calls Common.lua's `formatNumSep` (803-828), which no unit's
//! range covers and which reads the UI settings `main.showThousandsSeparators`,
//! `main.thousandsSeparator` and `main.decimalSeparator`. It is ported here
//! ([`formatNumSep`]) with those settings in a thread-local
//! ([`NumSepSettings`], PoB's defaults from Main.lua:108-112) that the UI sets
//! (CCR-0014).
#![allow(non_snake_case)]
// literal Lua statement shapes (`x = x * y`, `a and b` checks)
#![allow(
    clippy::assign_op_pattern,
    clippy::explicit_auto_deref,
    clippy::unnecessary_unwrap
)]

use std::cell::RefCell;

use crate::prelude::*;
use pob_lua::pattern::{gsub_bytes_with, Repl};

// @pob Modules/CalcFormat.lua:10-10 h=04510cb8
// (module-level statements: t_insert)

/// `main.showThousandsSeparators`, `main.thousandsSeparator`, `main.decimalSeparator`.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct NumSepSettings {
    pub showThousandsSeparators: bool,
    pub thousandsSeparator: String,
    pub decimalSeparator: String,
}

impl Default for NumSepSettings {
    /// Main.lua:108-112.
    fn default() -> Self {
        NumSepSettings {
            showThousandsSeparators: true,
            thousandsSeparator: ",".to_owned(),
            decimalSeparator: ".".to_owned(),
        }
    }
}

thread_local! {
    static NUM_SEP: RefCell<NumSepSettings> = RefCell::new(NumSepSettings::default());
}

/// Set the `main.*` number-separator settings `formatNumSep` reads (the UI's options).
pub fn set_num_sep_settings(s: NumSepSettings) {
    NUM_SEP.with(|n| *n.borrow_mut() = s);
}

/// The current `main.*` number-separator settings.
pub fn num_sep_settings() -> NumSepSettings {
    NUM_SEP.with(|n| n.borrow().clone())
}

/// `utf8.reverse` (lua-utf8): reverse by code points.
fn utf8_reverse(s: &str) -> String {
    s.chars().rev().collect()
}

/// `utf8.gsub(s, p, repl)` on ASCII patterns: byte `gsub` (the patterns only
/// match ASCII; the replacement may contain any UTF-8).
fn byte_gsub(s: &str, p: &str, repl: &str) -> String {
    let (b, _) = gsub_bytes_with(s.as_bytes(), p.as_bytes(), Repl::Str(repl.as_bytes()), None);
    String::from_utf8(b).unwrap_or_else(|e| String::from_utf8_lossy(e.as_bytes()).into_owned())
}

/// @pob Modules/Common.lua:803-828 h=6dee6380
/// Formats "1234.56" -> "1,234.5" (outside every unit's scope range; CCR-0014)
pub fn formatNumSep(str: &str) -> String {
    let main = num_sep_settings();
    // Common.lua:804
    pob_lua::gsub_fn(
        str,
        "(%^?x?%x?%x?%x?%x?%x?%x?-?%d+%.?%d+)",
        |c| {
            let m = c[0].to_str().into_owned();
            // :805
            let colour = pob_lua::lmatch(&m, "(^x%x%x%x%x%x%x)", None)
                .or_else(|| pob_lua::lmatch(&m, "(%^%d)", None))
                .map_or(String::new(), |c| c[0].to_str().into_owned());
            // :806
            let s1 = pob_lua::gsub(&m, "(^x%x%x%x%x%x%x)", "", None).0;
            let str = pob_lua::gsub(&s1, "(%^%d)", "", None).0;
            // :807 return if we have an invalid color code or a completely stripped number.
            if str.is_empty() || (colour.is_empty() && pob_lua::lmatch(&m, "%^", None).is_some()) {
                // :808
                return Some(m);
            }
            // :810
            let f = pob_lua::find(&str, "(-?)(%d+)(%.?%d*)", None, false).unwrap_or_else(|| {
                lua_error!("bad argument #1 to 'reverse' (string expected, got nil)")
            });
            let caps = &f.2;
            let minus = caps[0].to_str().into_owned();
            let mut integer = caps[1].to_str().into_owned();
            let fraction = caps[2].to_str().into_owned();
            // :811
            if main.showThousandsSeparators {
                // :812 (`rev1kSep` is an implicit global in Lua)
                let rev1kSep = utf8_reverse(&main.thousandsSeparator);
                // :813
                integer = utf8_reverse(&byte_gsub(
                    &utf8_reverse(&integer),
                    "(%d%d%d)",
                    &format!("%1{rev1kSep}"),
                ));
                // :814 There will be leading separators if the number of digits are divisible by 3
                // :815 This checks for their presence and removes them
                // :816 Don't use patterns here because thousandsSeparator can be a pattern control character, and will crash if used
                // :817
                if !main.thousandsSeparator.is_empty() {
                    // :818 (plain find; a match at byte 0 is code point 1)
                    let thousandsSeparator = integer.find(rev1kSep.as_str());
                    // :819
                    if thousandsSeparator == Some(0) {
                        // :820 utf8.sub(integer, 2): drop the first code point
                        let mut it = integer.chars();
                        it.next();
                        integer = it.collect();
                    }
                }
            } else {
                // :824
                integer = utf8_reverse(&byte_gsub(&utf8_reverse(&integer), "(%d%d%d)", "%1"));
            }
            // :826
            Some(colour + &minus + &integer + &byte_gsub(&fraction, "%.", &main.decimalSeparator))
        },
        None,
    )
    .0
}

/// @pob Modules/CalcFormat.lua:12-14 h=a194b6fe
pub fn formatCalcVal(val: f64, p: Option<f64>) -> String {
    // L13
    formatNumSep(&tostring_num(round(val, p.map(|p| p as i32))))
}

/// A `gsub` replacement value returned by a callback: strings and numbers
/// replace (numbers via `tostring`), nil/false keep the match, others raise.
fn repl_text(v: &Val) -> Option<String> {
    match v {
        Val::Nil | Val::Bool(false) => None,
        Val::Str(s) => Some(s.to_string()),
        Val::Num(n) => Some(tostring_num(*n)),
        other => lua_error!("invalid replacement value (a {})", other.lua_type()),
    }
}

/// `t[k]` where `k` may be nil (reading `t[nil]` gives nil).
fn index_opt(t: &Val, k: Option<f64>) -> Val {
    match k {
        Some(k) => t.get(Key::from(k)),
        None => match t {
            Val::Table(_) | Val::Data(_) | Val::Mod(_) | Val::Str(_) => Val::Nil,
            other => lua_error!("attempt to index a {} value", other.lua_type()),
        },
    }
}

/// `actor.mainSkill[name]` for a cfg name (`"skill" .. "Cfg"`, `"weapon1" .. "Cfg"`, …).
fn main_skill_cfg(skill: &ActiveSkill, name: &str) -> Option<Cfg> {
    match name {
        "skillCfg" => Some(skill.skillCfg.get()),
        "weapon1Cfg" | "weapon2Cfg" | "bleedCfg" | "poisonCfg" | "igniteCfg" | "OHbleedCfg"
        | "OHpoisonCfg" | "OHigniteCfg" | "dotCfg" | "decayCfg" => {
            skill.cfg_field(Name::from(name)).get()
        }
        // any other key of the activeSkill table is not a cfg table; nil keeps `or { }`
        _ => None,
    }
}

/// @pob Modules/CalcFormat.lua:16-74 h=2ccb71c0
/// `colData` is the calc section column table (UI data, `CalcSections`).
pub fn formatCalcStr(str: &str, actor: Option<&ActorRef>, colData: &Val) -> String {
    // L17
    let Some(actor) = actor else {
        return String::new();
    };
    let output = actor.output.get();
    // L18
    let str = pob_lua::gsub_fn(
        str,
        "{output:([%a%.:]+)}",
        |caps| {
            let c = caps[0].to_str().into_owned();
            // L19
            let m = pob_lua::lmatch(&c, "^(%a+)%.(%a+)$", None);
            // L20
            if let Some(m) = m {
                let (ns, var) = (m[0].to_str(), m[1].to_str());
                // L21
                let o = output.get(Name::from(&*ns));
                let v = if o.truthy() {
                    o.get(Name::from(&*var))
                } else {
                    o
                };
                repl_text(&lor(v, || Val::from("")))
            } else {
                // L23
                repl_text(&lor(output.get(Name::from(c.as_str())), || Val::from("")))
            }
        },
        None,
    )
    .0;
    // L26
    let str = pob_lua::gsub_fn(
        &str,
        "{(%d+):output:([%a%.:]+)}",
        |caps| {
            let p = caps[0].num();
            let c = caps[1].to_str().into_owned();
            // L27
            let m = pob_lua::lmatch(&c, "^(%a+)%.(%a+)$", None);
            // L28
            if let Some(m) = m {
                let (ns, var) = (m[0].to_str(), m[1].to_str());
                // L29
                let o = output.get(Name::from(&*ns));
                let v = if o.truthy() {
                    o.get(Name::from(&*var))
                } else {
                    o
                };
                Some(formatCalcVal(lor(v, || Val::Num(0.0)).num(), p))
            } else {
                // L31
                Some(formatCalcVal(
                    lor(output.get(Name::from(c.as_str())), || Val::Num(0.0)).num(),
                    p,
                ))
            }
        },
        None,
    )
    .0;
    // L34
    let str = pob_lua::gsub_fn(
        &str,
        "{(%d+):mod:([%d,]+)}",
        |caps| {
            let p = caps[0].num();
            let n = caps[1].to_str().into_owned();
            // L35
            let mut numList: Vec<f64> = Vec::new();
            // L36
            for num in pob_lua::gmatch(&n, "%d+") {
                // L37 (tonumber of a digit run is never nil)
                if let Some(x) = num[0].num() {
                    numList.push(x);
                }
            }
            // L39
            let first = index_opt(colData, numList.first().copied());
            if !first.truthy() || !first.get(n!("modType")).truthy() {
                // L40
                return Some("?".to_owned());
            }
            // L42
            let modType = first.get(n!("modType"));
            let isMore = matches!(&modType, Val::Str(s) if &**s == "MORE");
            // L43
            let mut modTotal = if isMore { 1.0 } else { 0.0 };
            // L44
            for num in &numList {
                // L45
                let sectionData = colData.get(Key::from(*num));
                // L46
                if !sectionData.truthy() {
                    break;
                }
                let mainSkill = actor.mainSkill.get();
                // L47
                let cfgName = sectionData.get(n!("cfg"));
                let modCfg: Cfg = if cfgName.truthy() {
                    let name = match &cfgName {
                        Val::Str(s) => format!("{s}Cfg"),
                        Val::Num(x) => format!("{}Cfg", tostring_num(*x)),
                        other => lua_error!(
                            "attempt to concatenate field 'cfg' (a {} value)",
                            other.lua_type()
                        ),
                    };
                    mainSkill
                        .as_ref()
                        .and_then(|s| main_skill_cfg(s, &name))
                        .unwrap_or_else(ModCfg::new)
                } else {
                    ModCfg::new()
                };
                // L48
                let modSource = sectionData.get(n!("modSource"));
                if modSource.truthy() {
                    // L49 (writes through to an aliased skill cfg, as in Lua)
                    modCfg.source.set(modSource.str().map(|s| Name::from(&*s)));
                }
                // L51
                let sActor = sectionData.get(n!("actor"));
                if sActor.truthy() {
                    // L52
                    modCfg.actor.set(sActor.str().map(|s| Name::from(&*s)));
                }
                // L54
                // L55
                let enemy = actor.enemy.get().upgrade();
                let modStore: Option<ModStoreRef> =
                    if sectionData.get(n!("enemy")).truthy() && enemy.is_some() {
                        Some(enemy.expect("checked").modDB.get())
                    } else if cfgName.truthy() && mainSkill.is_some() {
                        Some(mainSkill.as_ref().expect("checked").skillModList.get())
                    } else {
                        Some(actor.modDB.get())
                    };
                // L56
                let Some(modStore) = modStore else { break };
                let sType = sectionData.get(n!("modType"));
                let ty = sType
                    .str()
                    .and_then(|s| ModType::parse(&s))
                    .unwrap_or_else(|| {
                        lua_error!("calc section modType {sType:?} is not a mod type")
                    });
                let modName = sectionData.get(n!("modName"));
                // L57
                let modVal = match &modName {
                    Val::Table(_) | Val::Data(_) => {
                        // L58
                        let mut names: Vec<Name> = Vec::new();
                        let mut i = 1_i64;
                        loop {
                            let v = modName.get(i);
                            match v {
                                Val::Nil => break,
                                Val::Str(s) => names.push(Name::from(&*s)),
                                other => names.push(Name::from(format!("{other:?}").as_str())),
                            }
                            i += 1;
                        }
                        modStore.Combine(ty, Some(&*modCfg), &names)
                    }
                    // L60
                    Val::Str(s) => modStore.Combine(ty, Some(&*modCfg), &[Name::from(&**s)]),
                    _ => modStore.Combine(ty, Some(&*modCfg), &[]),
                };
                // L62
                if isMore {
                    // L63
                    modTotal = modTotal * modVal.num();
                } else {
                    // L65
                    modTotal = modTotal + modVal.num();
                }
            }
            // L68
            if isMore {
                // L69
                modTotal = (modTotal - 1.0) * 100.0;
            }
            // L71
            Some(formatCalcVal(modTotal, p))
        },
        None,
    )
    .0;
    // L73
    str
}
