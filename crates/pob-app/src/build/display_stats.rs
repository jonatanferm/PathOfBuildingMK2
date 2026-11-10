//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/BuildDisplayStats.lua 1-293.
//! Owner: U21.
//!
//! @pob-file Modules/BuildDisplayStats.lua @16de4b82d
//!
//! Module: Build Display Stats. Loads the displayStats and extraSaveStats.
//!
//! This defines the stats in the side bar, and also which stats show in node/item comparisons.
//! This may be user-customisable in the future.
//!
//! Representation: one [`DisplayStat`] per Lua row, with PoB's field names. The closures
//! (`condFunc`, `warnFunc`, `suffix`, `suffixCondFunc`, `overCapStatCondFunc`) are [`StatFn`]s
//! taking `(value, output)` as Lua values and returning a Lua value, so `nil` operands raise the
//! Lua errors PoB raises (comparisons, indexing, concatenation). `color` holds the `colorCodes`
//! key; [`DisplayStat::color`] reads it from `cx.main.cc` at use time. PoB captures the value when
//! the module is first required; the keys used here (LIFE, MANA, ES, ...) are never changed by
//! `updateColorCode` (only POSITIVE, NEGATIVE and HIGHLIGHT are), so both read the same text.
//!
//! LUA-QUIRK: three rows repeat a key in their constructor (`flag = "poison", flag =
//! "notAverage"`, `flag = "impale", flag = "showAverage"`, ...). The later value wins in Lua; the
//! [`ds!`] macro assigns in source order, so the later value wins here too.
//!
//! The tables are built once per thread ([`displayStats`], [`minionDisplayStats`],
//! [`extraSaveStats`]), as `require` caches the module.

#![allow(non_snake_case, non_upper_case_globals)]

use std::rc::Rc;

use pob_lua::{lua_error, tostring_num, Name};
use pob_ui_core::color_codes::ColorCodes;

use crate::model::{self, Val};

// ---- Lua operators on values (the closures' operands) -----------------------------------------

/// `condFunc(value, output)` / `warnFunc(value, output)` / `suffix(value, output)`: Lua values in,
/// a Lua value out.
pub type StatFn = fn(&Val, &Val) -> Val;

/// `suffix`: a string or a function (`type(suffix) == "function"`).
#[derive(Clone, Copy)]
pub enum Suffix {
    Str(&'static str),
    Fn(StatFn),
}

/// Lua `a < b` (LuaJIT `lj_meta_comp`): numbers and strings compare, anything else raises.
pub fn lt(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x < y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() < y.as_bytes(),
        _ => comp_error(a, b),
    }
}

/// Lua `a <= b`.
pub fn le(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x <= y,
        (Val::Str(x), Val::Str(y)) => x.as_bytes() <= y.as_bytes(),
        _ => comp_error(a, b),
    }
}

#[track_caller]
fn comp_error(a: &Val, b: &Val) -> ! {
    let (t1, t2) = (a.lua_type(), b.lua_type());
    if t1 == t2 {
        lua_error!("attempt to compare two {t1} values")
    } else {
        lua_error!("attempt to compare {t1} with {t2}")
    }
}

/// Lua `a > b` (LuaJIT compiles it as `b < a`).
fn gt(a: &Val, b: impl Into<Val>) -> bool {
    lt(&b.into(), a)
}
/// Lua `a >= b` (`b <= a`).
fn ge(a: &Val, b: impl Into<Val>) -> bool {
    le(&b.into(), a)
}
/// Lua `a < b` with an owned right operand.
fn ltv(a: &Val, b: impl Into<Val>) -> bool {
    lt(a, &b.into())
}
/// Lua `a <= b` with an owned right operand.
fn lev(a: &Val, b: impl Into<Val>) -> bool {
    le(a, &b.into())
}
/// Lua `a == b`.
fn eq(a: &Val, b: impl Into<Val>) -> bool {
    a.raw_eq(&b.into())
}
/// Lua `a ~= b`.
fn ne(a: &Val, b: impl Into<Val>) -> bool {
    !eq(a, b)
}
/// `o.k` (indexing nil raises).
fn get(o: &Val, k: &str) -> Val {
    o.get(Name::from(k))
}
/// `x or 0`.
fn or0(x: Val) -> Val {
    if x.truthy() {
        x
    } else {
        Val::Num(0.0)
    }
}
/// A Lua boolean.
fn bv(b: bool) -> Val {
    Val::Bool(b)
}
/// `cond and "text"` (false when `cond` is false).
fn and_str(cond: bool, s: &str) -> Val {
    if cond {
        Val::from(s)
    } else {
        Val::Bool(false)
    }
}
/// `data.misc.DotDpsCap`.
fn dot_dps_cap() -> f64 {
    model::data().misc.DotDpsCap
}

/// `x.k` where `x` may be a model object stored in a dynamic table (`ReqStrItem.sourceItem` is
/// an `Item`, `.sourceGem` a gem instance; contracts.md#class-census: `Val::Obj`).
fn field_of(x: &Val, k: &str) -> Val {
    if let Some(item) = x.obj::<model::Item>() {
        return match k {
            "name" => Val::from(item.name.get()),
            _ => item.extra.get(Name::from(k)),
        };
    }
    if let Some(gem) = x.obj::<model::GemInstance>() {
        return match k {
            "nameSpec" => Val::from(gem.nameSpec.get()),
            _ => gem.extra.get(Name::from(k)),
        };
    }
    x.get(Name::from(k))
}

/// The right operand of `..` (a string or a number; anything else raises).
#[track_caller]
fn concat_operand(x: &Val) -> String {
    match x {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// The `warnFunc` of `ReqStr` / `ReqDex` / `ReqInt` / `ReqOmni`:
/// `"You do not meet the <attr> requirement of " .. (o.<item>.source == "Item" and
/// o.<item>.sourceItem.name or o.<item>.source == "Gem" and o.<item>.sourceGem.nameSpec)`.
fn req_warning(o: &Val, attr: &str, item: &str) -> Val {
    let req_item = get(o, item);
    let a = if eq(&get(&req_item, "source"), "Item") {
        field_of(&get(&req_item, "sourceItem"), "name")
    } else {
        Val::Bool(false)
    };
    let name = if a.truthy() {
        a
    } else if eq(&get(&req_item, "source"), "Gem") {
        field_of(&get(&req_item, "sourceGem"), "nameSpec")
    } else {
        Val::Bool(false)
    };
    Val::from(format!(
        "You do not meet the {attr} requirement of {}",
        concat_operand(&name)
    ))
}

// ---- the row type -----------------------------------------------------------------------------

/// One sidebar stat row (also drives node/item comparisons). An empty `{}` is a separator.
/// Field names and meanings are BuildDisplayStats.lua:20-46's `---@class DisplayStat`.
#[derive(Clone, Copy)]
pub struct DisplayStat {
    /// Output value name, e.g. "Life", "TotalDPS", "MainHand"
    pub stat: Option<&'static str>,
    /// Nested output field when `stat` is a table, e.g. "Accuracy" for MainHand.Accuracy
    pub childStat: Option<&'static str>,
    /// Display label shown before the value
    pub label: Option<&'static str>,
    /// C-style format spec applied to the value, e.g. "d", ".1f", ".2f%%", "+d%%"
    pub fmt: Option<&'static str>,
    /// Override key for the sidebar breakdown
    pub breakdown: Option<&'static str>,
    /// Modifier names to show in the breakdown popup
    pub modNames: Option<&'static [&'static str]>,
    /// A list of breakdown sections which shouldn't be shown
    pub ignoredSections: Option<&'static [&'static str]>,
    /// The `colorCodes` key of the label colour (see the module doc)
    pub color: Option<&'static str>,
    /// Only show when the main skill has this skill flag
    pub flag: Option<&'static str>,
    /// Hide when the main skill has this skill flag
    pub notFlag: Option<&'static str>,
    /// Extra condition gating whether the row is shown
    pub condFunc: Option<StatFn>,
    /// Produces a warning message for the warnings panel
    pub warnFunc: Option<StatFn>,
    /// Colour the value red when `warnFunc` returns truthy
    pub warnColor: bool,
    /// Parenthesised suffix appended after the value
    pub suffix: Option<Suffix>,
    /// Gates whether `suffix` is shown
    pub suffixCondFunc: Option<StatFn>,
    /// Output name of the "over cap" counterpart shown alongside (e.g. resistances)
    pub overCapStat: Option<&'static str>,
    /// Gates whether the over-cap value is shown
    pub overCapStatCondFunc: Option<StatFn>,
    /// Allow compact formatting (e.g. 712.3K) when the user setting is on
    pub compactValue: bool,
    /// Include a percentage delta in node/item comparisons
    pub compPercent: bool,
    /// Treat a decrease as an improvement in comparisons
    pub lowerIsBetter: bool,
    /// Multiply the value by 100 (fractional value shown as a percentage)
    pub pc: bool,
    /// Value is a modifier: scale by 100 and subtract 100 (shows the +/- portion)
    pub r#mod: bool,
    /// Compute/consider the stat but don't render a sidebar row
    pub hideStat: bool,
    /// Use the muted "immune" colour instead of red for a zero/negative value
    pub chaosInoc: bool,
    /// Associated resource pool name (e.g. "ManaUnreserved") for cost rows
    pub pool: Option<&'static str>,
    /// Output name rendered inline for a label-only row
    pub labelStat: Option<&'static str>,
    /// Static parenthesised note for a label-only row
    pub val: Option<&'static str>,
}

impl DisplayStat {
    /// `{ }` (every field nil).
    pub const EMPTY: DisplayStat = DisplayStat {
        stat: None,
        childStat: None,
        label: None,
        fmt: None,
        breakdown: None,
        modNames: None,
        ignoredSections: None,
        color: None,
        flag: None,
        notFlag: None,
        condFunc: None,
        warnFunc: None,
        warnColor: false,
        suffix: None,
        suffixCondFunc: None,
        overCapStat: None,
        overCapStatCondFunc: None,
        compactValue: false,
        compPercent: false,
        lowerIsBetter: false,
        pc: false,
        r#mod: false,
        hideStat: false,
        chaosInoc: false,
        pool: None,
        labelStat: None,
        val: None,
    };

    /// `statData.color`: the colour text (`colorCodes[key]`), nil when unset.
    pub fn color(&self, cc: &ColorCodes) -> Option<String> {
        self.color.map(|k| {
            cc.get(k)
                .cloned()
                .unwrap_or_else(|| lua_error!("colorCodes.{k} is nil"))
        })
    }
}

/// A `DisplayStat` constructor in Lua's `{ k = v, ... }` form; later keys win.
macro_rules! ds {
    ($($k:ident = $v:expr),* $(,)?) => {{
        #[allow(unused_mut)]
        let mut d = DisplayStat::EMPTY;
        $( ds!(@set d $k $v); )*
        d
    }};
    (@set $d:ident condFunc $v:expr) => { $d.condFunc = Some($v as StatFn) };
    (@set $d:ident warnFunc $v:expr) => { $d.warnFunc = Some($v as StatFn) };
    (@set $d:ident suffixCondFunc $v:expr) => { $d.suffixCondFunc = Some($v as StatFn) };
    (@set $d:ident overCapStatCondFunc $v:expr) => { $d.overCapStatCondFunc = Some($v as StatFn) };
    (@set $d:ident suffix $v:expr) => { $d.suffix = Some($v) };
    (@set $d:ident mod $v:expr) => { $d.r#mod = $v };
    (@set $d:ident $k:ident $v:expr) => { $d.$k = ($v).into() };
}

// ---- the tables -------------------------------------------------------------------------------

/// lua: BuildDisplayStats.lua:11-14 `local function maxHitSuffix(value, output)`
/// @pob Modules/BuildDisplayStats.lua:11-14 h=2c210f55
fn maxHitSuffix(value: &Val, output: &Val) -> Val {
    // L13
    if eq(value, f64::INFINITY) {
        Val::from("Immune")
    } else if get(output, "GuardSkillActive").truthy() {
        Val::from("Guard")
    } else {
        Val::Nil
    }
}

/// lua: BuildDisplayStats.lua:16 `local defenseIgnoredSections = { "Base from Gear", "Inc. from Tree" }`
/// @pob Modules/BuildDisplayStats.lua:16-16 h=fcc2ba32
const defenseIgnoredSections: &[&str] = &["Base from Gear", "Inc. from Tree"];

thread_local! {
    static DISPLAY_STATS: Rc<Vec<DisplayStat>> = Rc::new(build_displayStats());
    static MINION_DISPLAY_STATS: Rc<Vec<DisplayStat>> = Rc::new(build_minionDisplayStats());
}

/// `displayStatsModule.displayStats` (Build.lua:432 `self.displayStats`).
pub fn displayStats() -> Rc<Vec<DisplayStat>> {
    DISPLAY_STATS.with(Rc::clone)
}

/// `displayStatsModule.minionDisplayStats` (Build.lua:433 `self.minionDisplayStats`).
pub fn minionDisplayStats() -> Rc<Vec<DisplayStat>> {
    MINION_DISPLAY_STATS.with(Rc::clone)
}

/// lua: BuildDisplayStats.lua:50-248 `local displayStats = { ... }`
/// @pob Modules/BuildDisplayStats.lua:50-248 h=1922561f
#[allow(clippy::too_many_lines)]
fn build_displayStats() -> Vec<DisplayStat> {
    vec![
        // L51
        ds! { stat = "ActiveMinionLimit", label = "Active Minion Limit", fmt = "d" },
        // L52
        ds! { stat = "AverageHit", label = "Average Hit", fmt = ".1f", compactValue = true, compPercent = true },
        // L53
        ds! { stat = "PvpAverageHit", label = "PvP Average Hit", fmt = ".1f", compactValue = true, compPercent = true, flag = "isPvP" },
        // L54
        ds! { stat = "AverageDamage", label = "Average Damage", fmt = ".1f", compactValue = true, compPercent = true, flag = "attack" },
        // L55
        ds! { stat = "AverageDamage", label = "Average Damage", fmt = ".1f", compactValue = true, compPercent = true, flag = "monsterExplode", condFunc = |_v, o| bv(ne(&get(o, "HitChance"), 100.0)) },
        // L56
        ds! { stat = "AverageBurstDamage", label = "Average Burst Damage", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| {
            let h = get(o, "AverageBurstHits");
            bv(h.truthy() && gt(&h, 1.0) && gt(v, 0.0))
        } },
        // L57
        ds! { stat = "PvpAverageDamage", label = "PvP Average Damage", fmt = ".1f", compactValue = true, compPercent = true, flag = "attackPvP" },
        // L58
        ds! { stat = "Speed", label = "Attack Rate", fmt = ".2f", compPercent = true, flag = "attack", condFunc = |v, o| bv(gt(v, 0.0) && eq(&or0(get(o, "TriggerTime")), 0.0)), breakdown = "Speed", modNames = &["Speed"][..], ignoredSections = &["Inc. Cast Speed", "More Cast Speed"][..] },
        // L59
        ds! { stat = "KineticFusilladeMaxEffectiveAPS", label = "Max Eff. KF Attack Rate", fmt = ".2f", compPercent = true, flag = "attack", condFunc = |v, _o| bv(gt(v, 0.0)) },
        // L60
        ds! { stat = "Speed", label = "Cast Rate", fmt = ".2f", compPercent = true, flag = "spell", condFunc = |v, o| bv(gt(v, 0.0) && eq(&or0(get(o, "TriggerTime")), 0.0)), modNames = &["Speed"][..] },
        // L61
        ds! { stat = "Speed", label = "Effective Trigger Rate", fmt = ".2f", compPercent = true, notFlag = "skipEffectiveRate", condFunc = |_v, o| bv(ne(&or0(get(o, "TriggerTime")), 0.0)) },
        // L62
        ds! { stat = "WarcryCastTime", label = "Cast Time", fmt = ".2fs", compPercent = true, lowerIsBetter = true, flag = "warcry" },
        // L63
        ds! { stat = "HitSpeed", label = "Hit Rate", fmt = ".2f", compPercent = true, condFunc = |_v, o| bv(!get(o, "TriggerTime").truthy()) },
        // L64
        ds! { stat = "HitTime", label = "Channel Time", fmt = ".2fs", compPercent = true, flag = "channelRelease", lowerIsBetter = true, condFunc = |_v, o| bv(!get(o, "TriggerTime").truthy()) },
        // L65
        ds! { stat = "ChannelTimeToTrigger", label = "Channel Time", fmt = ".2fs", compPercent = true, lowerIsBetter = true },
        // L66
        ds! { stat = "TrapThrowingTime", label = "Trap Throwing Time", fmt = ".2fs", compPercent = true, lowerIsBetter = true },
        // L67
        ds! { stat = "TrapCooldown", label = "Trap Cooldown", fmt = ".3fs", lowerIsBetter = true },
        // L68
        ds! { stat = "MineLayingTime", label = "Mine Throwing Time", fmt = ".2fs", compPercent = true, lowerIsBetter = true },
        // L69
        ds! { stat = "TrapThrowCount", label = "Avg. Traps per Throw", fmt = ".2f" },
        // L70
        ds! { stat = "MineThrowCount", label = "Avg. Mines per Throw", fmt = ".2f" },
        // L71
        ds! { stat = "TotemPlacementTime", label = "Totem Placement Time", fmt = ".2fs", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| bv(!get(o, "TriggerTime").truthy()) },
        // L72
        ds! { stat = "PreEffectiveCritChance", label = "Crit Chance", fmt = ".2f%%", flag = "hit", breakdown = "PreEffectiveCritChance" },
        // L73
        ds! { stat = "CritChance", label = "Effective Crit Chance", fmt = ".2f%%", flag = "hit", condFunc = |v, o| bv(ne(v, get(o, "PreEffectiveCritChance"))), breakdown = "CritChance" },
        // L74
        ds! { stat = "CritMultiplier", label = "Crit Multiplier", fmt = "d%%", pc = true, condFunc = |_v, o| bv(gt(&or0(get(o, "CritChance")), 0.0)), breakdown = "CritMultiplier" },
        // L75
        ds! { stat = "HitChance", label = "Hit Chance", fmt = ".0f%%", flag = "attack", breakdown = "HitChance" },
        // L76
        ds! { stat = "HitChance", label = "Hit Chance", fmt = ".0f%%", condFunc = |_v, o| get(o, "enemyHasSpellBlock") },
        // L77
        ds! { stat = "TotalDPS", label = "Hit DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "notAverage" },
        // L78
        ds! { stat = "PvpTotalDPS", label = "PvP Hit DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "notAveragePvP" },
        // L79
        ds! { stat = "TotalDPS", label = "Hit DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "showAverage", condFunc = |_v, o| bv(ne(&or0(get(o, "TriggerTime")), 0.0)) },
        // L80
        ds! { stat = "TotalDot", label = "DoT DPS", fmt = ".1f", compactValue = true, compPercent = true },
        // L81
        ds! { stat = "WithDotDPS", label = "Total DPS inc. DoT", fmt = ".1f", compactValue = true, compPercent = true, flag = "notAverage", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L82
        ds! { stat = "BleedDPS", label = "Bleed DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Bleed DPS exceeds in game limit") },
        // L83
        ds! { stat = "CorruptingBloodDPS", label = "Corrupting Blood DPS", compactValue = true, fmt = ".1f", compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Corrupting Blood DPS exceeds in game limit") },
        // L84
        ds! { stat = "BleedDamage", label = "Total Damage per Bleed", fmt = ".1f", compactValue = true, compPercent = true, flag = "showAverage" },
        // L85
        ds! { stat = "WithBleedDPS", label = "Total DPS inc. Bleed", fmt = ".1f", compactValue = true, compPercent = true, flag = "notAverage", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0)) },
        // L86
        ds! { stat = "IgniteDPS", label = "Ignite DPS", fmt = ".1f", compPercent = true, compactValue = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Ignite DPS exceeds in game limit") },
        // L87
        ds! { stat = "IgniteDamage", label = "Total Damage per Ignite", fmt = ".1f", compactValue = true, compPercent = true, flag = "showAverage" },
        // L88
        ds! { stat = "BurningGroundDPS", label = "Burning Ground DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Burning Ground DPS exceeds in game limit") },
        // L89
        ds! { stat = "MirageBurningGroundDPS", label = "Mirage Burning Ground DPS", compactValue = true, fmt = ".1f", compPercent = true, condFunc = |v, o| bv(ne(v, get(o, "BurningGroundDPS"))), warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Mirage Burning Ground DPS exceeds in game limit") },
        // L90
        ds! { stat = "WithIgniteDPS", label = "Total DPS inc. Ignite", fmt = ".1f", compactValue = true, compPercent = true, flag = "notAverage", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L91
        ds! { stat = "WithIgniteAverageDamage", label = "Average Dmg. inc. Ignite", compactValue = true, fmt = ".1f", compPercent = true },
        // L92
        ds! { stat = "PoisonDPS", label = "Single Poison DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Poison DPS exceeds in game limit") },
        // L93
        ds! { stat = "CausticGroundDPS", label = "Caustic Ground DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Caustic Ground DPS exceeds in game limit") },
        // L94
        ds! { stat = "MirageCausticGroundDPS", label = "Mirage Caustic Ground DPS", compactValue = true, fmt = ".1f", compPercent = true, condFunc = |v, o| bv(ne(v, get(o, "CausticGroundDPS"))), warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Mirage Caustic Ground DPS exceeds in game limit") },
        // L95
        ds! { stat = "PoisonDamage", label = "Total Damage per Poison", fmt = ".1f", compactValue = true, compPercent = true },
        // L96 (LUA-QUIRK: `flag = "poison", flag = "notAverage"`; the later key wins)
        ds! { stat = "WithPoisonDPS", label = "Total DPS inc. Poison", fmt = ".1f", compactValue = true, compPercent = true, flag = "poison", flag = "notAverage", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L97
        ds! { stat = "DecayDPS", label = "Decay DPS", fmt = ".1f", compactValue = true, compPercent = true },
        // L98
        ds! { stat = "TotalDotDPS", label = "Total DoT DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| {
            let ign = get(o, "TotalIgniteDPS");
            let ign = if ign.truthy() { ign } else { get(o, "IgniteDPS") };
            bv(get(o, "showTotalDotDPS").truthy()
                || (ne(v, get(o, "TotalDot"))
                    && ne(v, get(o, "TotalPoisonDPS"))
                    && ne(v, get(o, "CausticGroundDPS"))
                    && ne(v, ign)
                    && ne(v, get(o, "BurningGroundDPS"))
                    && ne(v, get(o, "BleedDPS"))
                    && ne(v, get(o, "CorruptingBloodDPS"))
                    && ne(v, get(o, "MirageCausticGroundDPS"))
                    && ne(v, get(o, "MirageBurningGroundDPS"))))
        }, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "DoT DPS exceeds in game limit") },
        // L99 (LUA-QUIRK: `flag = "impale", flag = "showAverage"`)
        ds! { stat = "ImpaleDPS", label = "Impale Damage", fmt = ".1f", compactValue = true, compPercent = true, flag = "impale", flag = "showAverage" },
        // L100 (LUA-QUIRK: `flag = "impale", flag = "showAverage"`)
        ds! { stat = "WithImpaleDPS", label = "Damage inc. Impale", fmt = ".1f", compactValue = true, compPercent = true, flag = "impale", flag = "showAverage", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L101 (LUA-QUIRK: `flag = "impale", flag = "notAverage"`)
        ds! { stat = "ImpaleDPS", label = "Impale DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "impale", flag = "notAverage" },
        // L102 (LUA-QUIRK: `flag = "impale", flag = "notAverage"`)
        ds! { stat = "WithImpaleDPS", label = "Total DPS inc. Impale", compactValue = true, fmt = ".1f", compPercent = true, flag = "impale", flag = "notAverage", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L103
        ds! { stat = "MirageDPS", label = "Total Mirage DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "mirageArcher", condFunc = |v, _o| bv(gt(v, 0.0)) },
        // L104
        ds! { stat = "MirageDPS", label = "Total Wisp DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "wisp", condFunc = |v, _o| bv(gt(v, 0.0)) },
        // L105
        ds! { stat = "CullingDPS", label = "Culling DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |_v, o| bv(gt(&or0(get(o, "CullingDPS")), 0.0)) },
        // L106
        ds! { stat = "ReservationDPS", label = "Reservation DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |_v, o| bv(gt(&or0(get(o, "ReservationDPS")), 0.0)) },
        // L107
        ds! { stat = "CombinedDPS", label = "Combined DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "notAverage", condFunc = |v, o| {
            let sum = or0(get(o, "TotalDPS")).num() + or0(get(o, "TotalDot")).num();
            bv(ne(v, sum)
                && ne(v, get(o, "WithImpaleDPS"))
                && (get(o, "showTotalDotDPS").truthy()
                    || (ne(v, get(o, "WithPoisonDPS"))
                        && ne(v, get(o, "WithIgniteDPS"))
                        && ne(v, get(o, "WithBleedDPS")))))
        } },
        // L108
        ds! { stat = "CombinedAvg", label = "Combined Total Damage", fmt = ".1f", compactValue = true, compPercent = true, flag = "showAverage", condFunc = |v, o| {
            bv((ne(v, get(o, "AverageDamage")) && eq(&or0(get(o, "TotalDot")), 0.0))
                && (ne(v, get(o, "WithPoisonDPS"))
                    || ne(v, get(o, "WithIgniteDPS"))
                    || ne(v, get(o, "WithBleedDPS"))))
        } },
        // L109
        ds! { stat = "ExplodeChance", label = "Total Explode Chance", fmt = ".0f%%" },
        // L110
        ds! { stat = "CombinedAvgToMonsterLife", label = "Enemy Life Equivalent", fmt = ".1f%%" },
        // L111
        ds! { stat = "Cooldown", label = "Skill Cooldown", fmt = ".3fs", notFlag = "skipEffectiveRate", lowerIsBetter = true },
        // L112
        ds! { stat = "SealCooldown", label = "Seal Gain Frequency", fmt = ".2fs", lowerIsBetter = true },
        // L113
        ds! { stat = "SealMax", label = "Max Number of Seals", fmt = "d" },
        // L114
        ds! { stat = "TimeMaxSeals", label = "Time to Gain Max Seals", fmt = ".2fs", lowerIsBetter = true },
        // L115
        ds! { stat = "AreaOfEffectRadiusMetres", label = "AoE Radius", fmt = ".1fm", breakdown = "AreaOfEffectRadius", modNames = &["AreaOfEffect"][..] },
        // L116
        ds! { stat = "BrandAttachmentRangeMetre", label = "Attachment Range", fmt = ".1fm", flag = "brand" },
        // L117
        ds! { stat = "BrandTicks", label = "Activations per Brand", fmt = "d", flag = "brand" },
        // L118
        ds! { stat = "ManaCost", label = "Mana Cost", fmt = "d", color = "MANA", pool = "ManaUnreserved", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ManaHasCost") },
        // L119
        ds! { stat = "ManaPercentCost", label = "Mana Cost", fmt = "d%%", color = "MANA", pool = "ManaUnreservedPercent", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ManaPercentHasCost") },
        // L120
        ds! { stat = "ManaPerSecondCost", label = "Mana Cost per second", fmt = ".2f", color = "MANA", pool = "ManaUnreserved", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ManaPerSecondHasCost") },
        // L121
        ds! { stat = "ManaPercentPerSecondCost", label = "Mana Cost per second", fmt = ".2f%%", color = "MANA", pool = "ManaUnreservedPercent", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ManaPercentPerSecondHasCost") },
        // L122
        ds! { stat = "LifeCost", label = "Life Cost", fmt = "d", color = "LIFE", pool = "LifeUnreserved", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "LifeHasCost") },
        // L123
        ds! { stat = "LifePercentCost", label = "Life Cost", fmt = "d%%", color = "LIFE", pool = "LifeUnreservedPercent", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "LifePercentHasCost") },
        // L124
        ds! { stat = "LifePerSecondCost", label = "Life Cost per second", fmt = ".2f", color = "LIFE", pool = "LifeUnreserved", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "LifePerSecondHasCost") },
        // L125
        ds! { stat = "LifePercentPerSecondCost", label = "Life Cost per second", fmt = ".2f%%", color = "LIFE", pool = "LifeUnreservedPercent", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "LifePercentPerSecondHasCost") },
        // L126
        ds! { stat = "ESCost", label = "Energy Shield Cost", fmt = "d", color = "ES", pool = "EnergyShield", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ESHasCost") },
        // L127
        ds! { stat = "ESPerSecondCost", label = "ES Cost per second", fmt = ".2f", color = "ES", pool = "EnergyShield", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ESPerSecondHasCost") },
        // L128
        ds! { stat = "ESPercentPerSecondCost", label = "ES Cost per second", fmt = ".2f%%", color = "ES", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "ESPercentPerSecondHasCost") },
        // L129
        ds! { stat = "RageCost", label = "Rage Cost", fmt = "d", color = "RAGE", pool = "Rage", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "RageHasCost") },
        // L130
        ds! { stat = "RagePerSecondCost", label = "Rage Cost per second", fmt = ".2f", color = "RAGE", pool = "Rage", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "RagePerSecondHasCost") },
        // L131
        ds! { stat = "SoulCost", label = "Soul Cost", fmt = "d", color = "RAGE", pool = "Soul", compPercent = true, lowerIsBetter = true, condFunc = |_v, o| get(o, "SoulHasCost") },
        // L132
        ds! {},
        // L133
        ds! { stat = "Str", label = "Strength", color = "STRENGTH", fmt = "d", breakdown = "Str" },
        // L134
        ds! { stat = "ReqStr", label = "Strength Required", color = "STRENGTH", fmt = "d", lowerIsBetter = true, condFunc = |v, o| bv(gt(v, get(o, "Str"))), warnFunc = |_v, o| req_warning(o, "Strength", "ReqStrItem") },
        // L135
        ds! { stat = "Dex", label = "Dexterity", color = "DEXTERITY", fmt = "d", breakdown = "Dex" },
        // L136
        ds! { stat = "ReqDex", label = "Dexterity Required", color = "DEXTERITY", fmt = "d", lowerIsBetter = true, condFunc = |v, o| bv(gt(v, get(o, "Dex"))), warnFunc = |_v, o| req_warning(o, "Dexterity", "ReqDexItem") },
        // L137
        ds! { stat = "Int", label = "Intelligence", color = "INTELLIGENCE", fmt = "d", breakdown = "Int" },
        // L138
        ds! { stat = "ReqInt", label = "Intelligence Required", color = "INTELLIGENCE", fmt = "d", lowerIsBetter = true, condFunc = |v, o| bv(gt(v, get(o, "Int"))), warnFunc = |_v, o| req_warning(o, "Intelligence", "ReqIntItem") },
        // L139
        ds! { stat = "Omni", label = "Omniscience", color = "RARE", fmt = "d" },
        // L140
        ds! { stat = "ReqOmni", label = "Omniscience Required", color = "RARE", fmt = "d", lowerIsBetter = true, condFunc = |v, o| bv(gt(v, or0(get(o, "Omni")))), warnFunc = |_v, o| req_warning(o, "Omniscience", "ReqOmniItem") },
        // L141
        ds! {},
        // L142
        ds! { stat = "Devotion", label = "Devotion", color = "RARE", fmt = "d" },
        // L143
        ds! {},
        // L144
        ds! { stat = "TotalEHP", label = "Effective Hit Pool", fmt = ".0f", suffix = Suffix::Str("Guard"), compactValue = true, compPercent = true, suffixCondFunc = |_v, o| get(o, "GuardSkillActive") },
        // L145
        ds! { stat = "PvPTotalTakenHit", label = "PvP Hit Taken", fmt = ".1f", flag = "isPvP", compactValue = true, lowerIsBetter = true },
        // L146
        ds! { stat = "PhysicalMaximumHitTaken", label = "Phys Max Hit", fmt = ".0f", compactValue = true, color = "PHYS", compPercent = true, suffix = Suffix::Fn(maxHitSuffix) },
        // L147
        ds! { stat = "LightningMaximumHitTaken", label = "Elemental Max Hit", fmt = ".0f", compactValue = true, color = "LIGHTNING", compPercent = true, suffix = Suffix::Fn(maxHitSuffix), condFunc = |_v, o| {
            let l = get(o, "LightningMaximumHitTaken");
            bv(eq(&l, get(o, "ColdMaximumHitTaken")) && eq(&l, get(o, "FireMaximumHitTaken")))
        } },
        // L148
        ds! { stat = "FireMaximumHitTaken", label = "Fire Max Hit", fmt = ".0f", compactValue = true, color = "FIRE", compPercent = true, suffix = Suffix::Fn(maxHitSuffix), condFunc = elemental_max_hits_differ },
        // L149
        ds! { stat = "ColdMaximumHitTaken", label = "Cold Max Hit", fmt = ".0f", compactValue = true, color = "COLD", compPercent = true, suffix = Suffix::Fn(maxHitSuffix), condFunc = elemental_max_hits_differ },
        // L150
        ds! { stat = "LightningMaximumHitTaken", label = "Lightning Max Hit", fmt = ".0f", compactValue = true, color = "LIGHTNING", compPercent = true, suffix = Suffix::Fn(maxHitSuffix), condFunc = elemental_max_hits_differ },
        // L151
        ds! { stat = "ChaosMaximumHitTaken", label = "Chaos Max Hit", fmt = ".0f", compactValue = true, color = "CHAOS", compPercent = true, suffix = Suffix::Fn(maxHitSuffix) },
        // L152
        ds! {},
        // L153
        ds! { stat = "MainHand", childStat = "Accuracy", label = "MH Accuracy", fmt = "d", condFunc = |_v, o| get(o, "PreciseTechnique"), warnFunc = |v, o| and_str(ltv(v, get(o, "Life")), "You do not have enough Accuracy for Precise Technique"), warnColor = true },
        // L154
        ds! { stat = "OffHand", childStat = "Accuracy", label = "OH Accuracy", fmt = "d", condFunc = |_v, o| get(o, "PreciseTechnique"), warnFunc = |v, o| and_str(ltv(v, get(o, "Life")), "You do not have enough Accuracy for Precise Technique"), warnColor = true },
        // L155
        ds! { stat = "Life", label = "Total Life", fmt = "d", compactValue = true, color = "LIFE", compPercent = true, modNames = &["Life"][..], ignoredSections = &["Base from Gear", "Inc. from Tree"][..] },
        // L156
        ds! { stat = "Spec:LifeInc", label = "%Inc Life from Tree", fmt = "d%%", color = "LIFE", condFunc = |v, o| bv(gt(v, 0.0) && gt(&get(o, "Life"), 1.0)) },
        // L157
        ds! { stat = "LifeUnreserved", label = "Unreserved Life", fmt = "d", color = "LIFE", condFunc = |v, o| bv(ltv(v, get(o, "Life"))), compPercent = true, warnFunc = |v, _o| and_str(lev(v, 0.0), "Your unreserved Life is below 1") },
        // L158
        ds! { stat = "LifeRecoverable", label = "Life Recoverable", fmt = "d", color = "LIFE", condFunc = |v, o| bv(ltv(v, get(o, "LifeUnreserved"))) },
        // L159
        ds! { stat = "LifeUnreservedPercent", label = "Unreserved Life", fmt = "d%%", color = "LIFE", condFunc = |v, _o| bv(ltv(v, 100.0)) },
        // L160
        ds! { stat = "LifeRegenRecovery", label = "Life Regen", fmt = ".1f", color = "LIFE", condFunc = |_v, o| bv(lev(&get(o, "LifeRecovery"), 0.0) && ne(&get(o, "LifeRegenRecovery"), 0.0)), breakdown = "LifeRegenRecovery" },
        // L161
        ds! { stat = "LifeRegenRecovery", label = "Life Recovery", fmt = ".1f", color = "LIFE", condFunc = |_v, o| bv(gt(&get(o, "LifeRecovery"), 0.0) && ne(&get(o, "LifeRegenRecovery"), 0.0)), breakdown = "LifeRegenRecovery" },
        // L162
        ds! { stat = "LifeRecharge", label = "Life Recharge", fmt = ".1f", color = "LIFE", condFunc = |v, _o| bv(gt(v, 0.0)) },
        // L163
        ds! { stat = "LifeLeechGainRate", label = "Life Leech/On Hit Rate", fmt = ".1f", color = "LIFE", compPercent = true, breakdown = "LifeLeech" },
        // L164
        ds! { stat = "LifeLeechGainPerHit", label = "Life Leech/Gain per Hit", fmt = ".1f", color = "LIFE", compPercent = true },
        // L165
        ds! {},
        // L166
        ds! { stat = "Mana", label = "Total Mana", fmt = "d", compactValue = true, color = "MANA", compPercent = true, modNames = &["Mana"][..], ignoredSections = &["Base from Gear", "Inc. from Tree"][..] },
        // L167
        ds! { stat = "Spec:ManaInc", label = "%Inc Mana from Tree", color = "MANA", fmt = "d%%" },
        // L168
        ds! { stat = "ManaUnreserved", label = "Unreserved Mana", fmt = "d", color = "MANA", condFunc = |v, o| bv(ltv(v, get(o, "Mana"))), compPercent = true, warnFunc = |v, _o| and_str(ltv(v, 0.0), "Your unreserved Mana is negative") },
        // L169
        ds! { stat = "ManaUnreservedPercent", label = "Unreserved Mana", fmt = "d%%", color = "MANA", condFunc = |v, _o| bv(ltv(v, 100.0)), breakdown = "ManaReserved" },
        // L170
        ds! { stat = "ManaRegenRecovery", label = "Mana Regen", fmt = ".1f", color = "MANA", condFunc = |_v, o| bv(lev(&get(o, "ManaRecovery"), 0.0) && ne(&get(o, "ManaRegenRecovery"), 0.0)) },
        // L171
        ds! { stat = "ManaRegenRecovery", label = "Mana Recovery", fmt = ".1f", color = "MANA", condFunc = |_v, o| bv(gt(&get(o, "ManaRecovery"), 0.0) && ne(&get(o, "ManaRegenRecovery"), 0.0)) },
        // L172
        ds! { stat = "ManaLeechGainRate", label = "Mana Leech/On Hit Rate", fmt = ".1f", color = "MANA", compPercent = true },
        // L173
        ds! { stat = "ManaLeechGainPerHit", label = "Mana Leech/Gain per Hit", fmt = ".1f", color = "MANA", compPercent = true },
        // L174
        ds! {},
        // the breakdown for armour, es and ev doesn't have anything relevant, and
        // we are better off ignoring it and just showing relevant mods
        // L177
        ds! { stat = "EnergyShield", label = "Energy Shield", fmt = "d", compactValue = true, color = "ES", compPercent = true, modNames = &["EnergyShield", "Defences"][..], ignoredSections = defenseIgnoredSections },
        // L178
        ds! { stat = "EnergyShieldRecoveryCap", label = "Recoverable ES", color = "ES", fmt = "d", condFunc = |_v, o| get(o, "CappingES") },
        // L179
        ds! { stat = "Spec:EnergyShieldInc", label = "%Inc ES from Tree", color = "ES", fmt = "d%%" },
        // L180
        ds! { stat = "EnergyShieldRegenRecovery", label = "ES Regen", color = "ES", fmt = ".1f", condFunc = |_v, o| bv(lev(&get(o, "EnergyShieldRecovery"), 0.0) && ne(&get(o, "EnergyShieldRegenRecovery"), 0.0)) },
        // L181
        ds! { stat = "EnergyShieldRegenRecovery", label = "ES Recovery", color = "ES", fmt = ".1f", condFunc = |_v, o| bv(gt(&get(o, "EnergyShieldRecovery"), 0.0) && ne(&get(o, "EnergyShieldRegenRecovery"), 0.0)) },
        // L182
        ds! { stat = "EnergyShieldLeechGainRate", label = "ES Leech/On Hit Rate", color = "ES", fmt = ".1f", compPercent = true },
        // L183
        ds! { stat = "EnergyShieldLeechGainPerHit", label = "ES Leech/Gain per Hit", color = "ES", fmt = ".1f", compPercent = true },
        // L184
        ds! {},
        // L185
        ds! { stat = "Ward", label = "Ward", fmt = "d", compactValue = true, color = "WARD", compPercent = true, modNames = &["Ward"][..], ignoredSections = defenseIgnoredSections },
        // L186
        ds! {},
        // L187
        ds! { stat = "Rage", label = "Rage", fmt = "d", color = "RAGE", compPercent = true },
        // L188
        ds! { stat = "RageRegenRecovery", label = "Rage Regen", fmt = ".1f", color = "RAGE", compPercent = true },
        // L189
        ds! {},
        // L190
        ds! { stat = "TotalBuildDegen", label = "Total Degen", fmt = ".1f", lowerIsBetter = true },
        // L191
        ds! { stat = "TotalNetRegen", label = "Total Net Recovery", fmt = "+.1f" },
        // L192
        ds! { stat = "NetLifeRegen", label = "Net Life Recovery", fmt = "+.1f", color = "LIFE" },
        // L193
        ds! { stat = "NetManaRegen", label = "Net Mana Recovery", fmt = "+.1f", color = "MANA" },
        // L194
        ds! { stat = "NetEnergyShieldRegen", label = "Net ES Recovery", fmt = "+.1f", color = "ES" },
        // L195
        ds! {},
        // L196
        ds! { stat = "Evasion", label = "Evasion rating", fmt = "d", color = "EVASION", compPercent = true, modNames = &["Evasion", "ArmourAndEvasion", "Defences"][..], ignoredSections = defenseIgnoredSections },
        // L197
        ds! { stat = "Spec:EvasionInc", label = "%Inc Evasion from Tree", color = "EVASION", fmt = "d%%" },
        // L198
        ds! { stat = "MeleeEvadeChance", label = "Evade Chance", fmt = "d%%", color = "EVASION", condFunc = |v, o| bv(gt(v, 0.0) && eq(&get(o, "MeleeEvadeChance"), get(o, "ProjectileEvadeChance"))) },
        // L199
        ds! { stat = "MeleeEvadeChance", label = "Melee Evade Chance", fmt = "d%%", color = "EVASION", condFunc = |v, o| bv(gt(v, 0.0) && ne(&get(o, "MeleeEvadeChance"), get(o, "ProjectileEvadeChance"))) },
        // L200
        ds! { stat = "ProjectileEvadeChance", label = "Projectile Evade Chance", fmt = "d%%", color = "EVASION", condFunc = |v, o| bv(gt(v, 0.0) && ne(&get(o, "MeleeEvadeChance"), get(o, "ProjectileEvadeChance"))) },
        // L201
        ds! {},
        // L202
        ds! { stat = "Armour", label = "Armour", fmt = "d", compPercent = true, modNames = &["Armour", "ArmourAndEvasion", "Defences", "ArmourDefense"][..], ignoredSections = defenseIgnoredSections },
        // L203
        ds! { stat = "Spec:ArmourInc", label = "%Inc Armour from Tree", fmt = "d%%" },
        // L204
        ds! { stat = "PhysicalDamageReduction", label = "Phys. Damage Reduction", fmt = "d%%", condFunc = |_v, _o| bv(true) },
        // L205
        ds! {},
        // L206
        ds! { stat = "EffectiveBlockChance", label = "Block Chance", fmt = ".3f%%", overCapStat = "BlockChanceOverCap", breakdown = "BlockChance" },
        // L207
        ds! { stat = "EffectiveSpellBlockChance", label = "Spell Block Chance", fmt = ".3f%%", overCapStat = "SpellBlockChanceOverCap", breakdown = "SpellBlockChance" },
        // L208
        ds! { stat = "AttackDodgeChance", label = "Attack Dodge Chance", fmt = "d%%", overCapStat = "AttackDodgeChanceOverCap" },
        // L209
        ds! { stat = "SpellDodgeChance", label = "Spell Dodge Chance", fmt = "d%%", overCapStat = "SpellDodgeChanceOverCap" },
        // L210
        ds! { stat = "EffectiveSpellSuppressionChance", label = "Spell Suppression Chance", fmt = ".2f%%", overCapStat = "SpellSuppressionChanceOverCap", breakdown = "", modNames = &["SpellSuppressionChance"][..] },
        // L211
        ds! {},
        // L212
        ds! { stat = "FireResist", label = "Fire Resistance", fmt = "d%%", color = "FIRE", condFunc = |_v, _o| bv(true), overCapStat = "FireResistOverCap" },
        // L213
        ds! { stat = "FireResistOverCap", label = "Fire Res. Over Max", fmt = "d%%", hideStat = true },
        // L214
        ds! { stat = "ColdResist", label = "Cold Resistance", fmt = "d%%", color = "COLD", condFunc = |_v, _o| bv(true), overCapStat = "ColdResistOverCap" },
        // L215
        ds! { stat = "ColdResistOverCap", label = "Cold Res. Over Max", fmt = "d%%", hideStat = true },
        // L216
        ds! { stat = "LightningResist", label = "Lightning Resistance", fmt = "d%%", color = "LIGHTNING", condFunc = |_v, _o| bv(true), overCapStat = "LightningResistOverCap" },
        // L217
        ds! { stat = "LightningResistOverCap", label = "Lightning Res. Over Max", fmt = "d%%", hideStat = true },
        // L218
        ds! { stat = "ChaosResist", label = "Chaos Resistance", fmt = "d%%", color = "CHAOS", overCapStat = "ChaosResistOverCap", overCapStatCondFunc = |_v, o| bv(!get(o, "ChaosInoculation").truthy()), suffix = Suffix::Str("Immune"), suffixCondFunc = |_v, o| get(o, "ChaosInoculation") },
        // L219
        ds! { stat = "ChaosResistOverCap", label = "Chaos Res. Over Max", fmt = "d%%", hideStat = true },
        // L220
        ds! {},
        // L221
        ds! { stat = "EffectiveMovementSpeedMod", label = "Movement Speed Modifier", fmt = "+d%%", mod = true, condFunc = |_v, _o| bv(true) },
        // L222-242 (a commented-out block of "potentially useful mods")
        // L243
        ds! {},
        // L244
        ds! { stat = "FullDPS", label = "Full DPS", fmt = ".1f", compactValue = true, color = "CURRENCY", compPercent = true },
        // L245
        ds! { stat = "FullDotDPS", label = "Full Dot DPS", fmt = ".1f", compactValue = true, color = "CURRENCY", compPercent = true, condFunc = |v, _o| bv(ge(v, dot_dps_cap())), warnFunc = |_v, _o| Val::from("Full Dot DPS exceeds in game limit") },
        // L246
        ds! {},
        // L247
        ds! { stat = "SkillDPS", label = "Skill DPS", compactValue = true, condFunc = |_v, _o| bv(true) },
    ]
}

/// The `condFunc` of the Fire/Cold/Lightning max hit rows (BuildDisplayStats.lua:148-150):
/// `o.LightningMaximumHitTaken ~= o.ColdMaximumHitTaken or o.LightningMaximumHitTaken ~= o.FireMaximumHitTaken`.
fn elemental_max_hits_differ(_v: &Val, o: &Val) -> Val {
    let l = get(o, "LightningMaximumHitTaken");
    bv(ne(&l, get(o, "ColdMaximumHitTaken")) || ne(&l, get(o, "FireMaximumHitTaken")))
}

/// lua: BuildDisplayStats.lua:250-279 `local minionDisplayStats = { ... }`
/// @pob Modules/BuildDisplayStats.lua:250-279 h=63e3ab06
fn build_minionDisplayStats() -> Vec<DisplayStat> {
    vec![
        // L251
        ds! { stat = "AverageDamage", label = "Average Damage", fmt = ".1f", compactValue = true, compPercent = true },
        // L252
        ds! { stat = "Speed", label = "Attack/Cast Rate", fmt = ".2f", compPercent = true, condFunc = |v, o| bv(gt(v, 0.0) && eq(&or0(get(o, "TriggerTime")), 0.0)) },
        // L253
        ds! { stat = "HitSpeed", label = "Hit Rate", fmt = ".2f" },
        // L254
        ds! { stat = "Speed", label = "Effective Trigger Rate", fmt = ".2f", compPercent = true, notFlag = "skipEffectiveRate", condFunc = |_v, o| bv(ne(&or0(get(o, "TriggerTime")), 0.0)) },
        // L255
        ds! { stat = "TotalDPS", label = "Hit DPS", fmt = ".1f", compactValue = true, compPercent = true },
        // L256
        ds! { stat = "TotalDot", label = "DoT DPS", fmt = ".1f", compactValue = true, compPercent = true },
        // L257
        ds! { stat = "WithDotDPS", label = "Total DPS inc. DoT", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L258
        ds! { stat = "BleedDPS", label = "Bleed DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Minion Bleed DPS exceeds in game limit") },
        // L259
        ds! { stat = "WithBleedDPS", label = "Total DPS inc. Bleed", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0)) },
        // L260
        ds! { stat = "IgniteDPS", label = "Ignite DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Minion Ignite DPS exceeds in game limit") },
        // L261
        ds! { stat = "WithIgniteDPS", label = "Total DPS inc. Ignite", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L262
        ds! { stat = "PoisonDPS", label = "Poison DPS", fmt = ".1f", compactValue = true, compPercent = true, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Minion Poison dps exceeds in game limit") },
        // L263
        ds! { stat = "PoisonDamage", label = "Total Damage per Poison", fmt = ".1f", compactValue = true, compPercent = true },
        // L264
        ds! { stat = "WithPoisonDPS", label = "Total DPS inc. Poison", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "ImpaleDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L265
        ds! { stat = "DecayDPS", label = "Decay DPS", fmt = ".1f", compactValue = true, compPercent = true },
        // L266
        ds! { stat = "TotalDotDPS", label = "Total DoT DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| {
            let ign = get(o, "TotalIgniteDPS");
            let ign = if ign.truthy() { ign } else { get(o, "IgniteDPS") };
            bv(ne(v, get(o, "TotalDot"))
                && ne(v, get(o, "ImpaleDPS"))
                && ne(v, get(o, "TotalPoisonDPS"))
                && ne(v, ign)
                && ne(v, get(o, "BleedDPS")))
        }, warnFunc = |v, _o| and_str(ge(v, dot_dps_cap()), "Minion DoT DPS exceeds in game limit") },
        // L267
        ds! { stat = "ImpaleDPS", label = "Impale DPS", fmt = ".1f", compactValue = true, compPercent = true, flag = "impale" },
        // L268
        ds! { stat = "WithImpaleDPS", label = "Total DPS inc. Impale", fmt = ".1f", compactValue = true, compPercent = true, flag = "impale", condFunc = |v, o| bv(ne(v, get(o, "TotalDPS")) && eq(&or0(get(o, "TotalDot")), 0.0) && eq(&or0(get(o, "IgniteDPS")), 0.0) && eq(&or0(get(o, "PoisonDPS")), 0.0) && eq(&or0(get(o, "BleedDPS")), 0.0)) },
        // L269
        ds! { stat = "CullingDPS", label = "Culling DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |_v, o| bv(gt(&or0(get(o, "CullingDPS")), 0.0)) },
        // L270
        ds! { stat = "ReservationDPS", label = "Reservation DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |_v, o| bv(gt(&or0(get(o, "ReservationDPS")), 0.0)) },
        // L271
        ds! { stat = "CombinedDPS", label = "Combined DPS", fmt = ".1f", compactValue = true, compPercent = true, condFunc = |v, o| {
            let sum = or0(get(o, "TotalDPS")).num() + or0(get(o, "TotalDot")).num();
            bv(ne(v, sum)
                && ne(v, get(o, "WithImpaleDPS"))
                && ne(v, get(o, "WithPoisonDPS"))
                && ne(v, get(o, "WithIgniteDPS"))
                && ne(v, get(o, "WithBleedDPS")))
        } },
        // L272
        ds! { stat = "Cooldown", label = "Skill Cooldown", fmt = ".3fs", notFlag = "skipEffectiveRate", lowerIsBetter = true },
        // L273
        ds! { stat = "Life", label = "Total Life", fmt = ".1f", compactValue = true, color = "LIFE", compPercent = true, modNames = &["Life"][..], ignoredSections = &["Base from Gear", "Inc. from Tree"][..] },
        // L274
        ds! { stat = "LifeRegenRecovery", label = "Life Recovery", fmt = ".1f", color = "LIFE" },
        // L275
        ds! { stat = "LifeLeechGainRate", label = "Life Leech/On Hit Rate", fmt = ".1f", color = "LIFE", compPercent = true },
        // L276
        ds! { stat = "EnergyShield", label = "Energy Shield", fmt = "d", compactValue = true, color = "ES", compPercent = true, modNames = &["EnergyShield", "Defences"][..], ignoredSections = defenseIgnoredSections },
        // L277
        ds! { stat = "EnergyShieldRegenRecovery", label = "ES Recovery", fmt = ".1f", color = "ES" },
        // L278
        ds! { stat = "EnergyShieldLeechGainRate", label = "ES Leech/On Hit Rate", fmt = ".1f", color = "ES", compPercent = true },
    ]
}

/// lua: BuildDisplayStats.lua:282-291 `local extraSaveStats = { ... }`: extra stats saved to the
/// xml if not already saved there, mostly for 3rd party tools.
/// @pob Modules/BuildDisplayStats.lua:282-291 h=667fbde1
pub const extraSaveStats: &[&str] = &[
    "PowerCharges",
    "PowerChargesMax",
    "FrenzyCharges",
    "FrenzyChargesMax",
    "EnduranceCharges",
    "EnduranceChargesMax",
    "ActiveTotemLimit",
    "ActiveMinionLimit",
];

// lua: BuildDisplayStats.lua:293 `return { displayStats = ..., minionDisplayStats = ...,
// extraSaveStats = ... }`: the three accessors above.
// @pob Modules/BuildDisplayStats.lua:293-293 h=d5acee3c

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn table_sizes_and_quirks() {
        // 174 rows in displayStats (BuildDisplayStats.lua:51-247 without the comment block)
        let d = build_displayStats();
        assert_eq!(d.len(), 174);
        assert_eq!(build_minionDisplayStats().len(), 28);
        // L96: the later `flag` wins
        let wp = d.iter().find(|s| s.stat == Some("WithPoisonDPS")).unwrap();
        assert_eq!(wp.flag, Some("notAverage"));
        let imp: Vec<_> = d.iter().filter(|s| s.stat == Some("ImpaleDPS")).collect();
        assert_eq!(imp[0].flag, Some("showAverage"));
        assert_eq!(imp[1].flag, Some("notAverage"));
    }

    #[test]
    fn comparisons_raise_like_lua() {
        let r = std::panic::catch_unwind(|| gt(&Val::Num(1.0), Val::Nil));
        let msg = pob_host::panic_message(&*r.unwrap_err());
        assert!(msg.contains("attempt to compare nil with number"), "{msg}");
        assert!(gt(&Val::Num(2.0), 1.0));
        assert!(!ne(&Val::Num(2.0), 2.0));
        assert!(ne(&Val::Num(2.0), Val::Nil));
    }
}
