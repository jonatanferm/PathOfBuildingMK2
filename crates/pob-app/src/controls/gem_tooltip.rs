//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/GemTooltip.lua 1-261 (the shared gem
//! tooltip renderer) and src/Modules/StatDescriber.lua 1-299 (`data.describeStats`, which the gem
//! tooltip needs and no unit owns; CCR-0108 §1). Owner: U29.
//!
//! @pob-file Classes/GemTooltip.lua @16de4b82d
//! @pob-file Modules/StatDescriber.lua @16de4b82d
//!
//! # Surface
//!
//! [`add_gem_tooltip`] is `GemTooltip.AddGemTooltip(tooltip, build, gemInstance, options)`.
//! `gemInstance` is one of the tables PoB passes (GemSelectControl.lua:598-611, :641-654,
//! ItemsTab.lua:4631-4636, PassiveTreeView.lua:1636-1641), as [`TooltipGemInstance`]: its
//! `gemData` is a real gem or the made-up `{ grantedEffect = skill, tags = { } }` of the item /
//! tree callers ([`TooltipGemData`]); `displayEffect` is the calc's skill effect, when the
//! instance is a socket group's gem. [`stat_describer::describe_stats`] is `data.describeStats`
//! (Data.lua:646).
//!
//! # Notes
//!
//! - `tooltip.gemIcon = …` (L221) writes a field nothing reads (grep of src at 16de4b82d: only
//!   this line names it), so the port has no field for it.
//! - L227/L228 read `gemInstance.gemData.VaalGem`, a key the data never sets (Data.lua spells it
//!   `vaalGem`): always nil, so `grantedEffect` is `primary` (LUA-QUIRK, kept).
//! - L33 `grantedEffect.levels[displayInstance.level] or {}` and the `cost` table go through
//!   `skills_tab::levelCostCleared` (SkillsTab.lua:1188 replaces `levels[l].cost` with `{}` at run
//!   time; STYLE.md "Runtime writes into data"). `grantedEffect.hasGlobalEffect` is not read here.
//! - `grantedEffect.statMap[stat]` (L182) is `skill_stat_map::statMapLookup`, which is the
//!   statMap's `__index` fallback to `data.skillStatMap` with its side effects (Data.lua:1033-1045).
//! - L188 (devModeAlt only) writes `source[1].value = lineMap[line]` into the shared data table
//!   before `modLib.formatMod`; the port formats a copy with that value (DIVERGENCE(immutable
//!   data): a later `formatMod` of the same mod prints the same text, so nothing differs on
//!   screen).
//! - ORDER: L50 / L69 `pairs(reservationMap)` builds a ", "-joined string, whose order would be
//!   PoB's per-process string-hash order; no skill level holds two of the four keys
//!   (`u29_data_invariants` checks every level of every granted effect), so (c): the order does not
//!   matter. L176 `pairs(stats)` sums into a table (order-free). StatDescriber L212/L228 fill and
//!   then fully sort `descOrdered` by (depth, order), which is a total order (one descriptor per
//!   first stat), so (c).
//! - L253 `primary.flavourText`: the schema's empty list stands for nil (no skill in the data has an
//!   empty `flavourText` table, grep of Data/Skills at 16de4b82d).
//! - Property closures that write fields (ui doc §7.3): none.

use std::cell::RefCell;
use std::rc::Rc;

use pob_lua::{lfmt, Name};
use pob_sg::Font;
use pob_ui_core::prelude::*;
use pob_ui_core::tooltip::tooltip::{Tooltip, TtColor};

use crate::app::CxApp;
use crate::build::stats;
use crate::model::gamedata::{GemDataRef, GrantedEffect, GrantedEffectRef, SkillLevelRef};
use crate::model::{BuildRef, Table};
use pob_calc::active_skill::{SkillEffect, SkillEffectRef};

// @pob-skip Classes/GemTooltip.lua:1-10 reason="file header comment, `local m_max = math.max` (unused) and the module table"
// @pob-skip Classes/GemTooltip.lua:206-206 reason="blank line between functions"
// @pob-skip Classes/GemTooltip.lua:260-261 reason="`return GemTooltip` (the module is this file)"

/// `gemInstance.gemData` as GemTooltip reads it: a gem of `data.gems`, or the made-up
/// `{ grantedEffect = skill, tags = { } }` of ItemsTab.lua:4628 / PassiveTreeView.lua:1634 (no
/// name, tagString, naturalMaxLevel or requirements: all nil).
#[derive(Clone)]
pub enum TooltipGemData {
    Gem(GemDataRef),
    MadeUp(GrantedEffectRef),
}

impl TooltipGemData {
    /// `gemData.grantedEffect`
    pub fn granted_effect(&self) -> &GrantedEffectRef {
        match self {
            TooltipGemData::Gem(g) => &g.grantedEffect,
            TooltipGemData::MadeUp(ge) => ge,
        }
    }
    /// `gemData.secondaryGrantedEffect`
    pub fn secondary_granted_effect(&self) -> Option<&GrantedEffectRef> {
        match self {
            TooltipGemData::Gem(g) => g.secondaryGrantedEffect.as_ref(),
            TooltipGemData::MadeUp(_) => None,
        }
    }
    /// `gemData.secondaryEffectName`
    pub fn secondary_effect_name(&self) -> Option<Name> {
        match self {
            TooltipGemData::Gem(g) => g.secondaryEffectName,
            TooltipGemData::MadeUp(_) => None,
        }
    }
    /// `gemData.tagString`
    pub fn tag_string(&self) -> Option<&str> {
        match self {
            TooltipGemData::Gem(g) => Some(&g.tagString),
            TooltipGemData::MadeUp(_) => None,
        }
    }
    /// `gemData.tags[t]` is truthy
    pub fn has_tag(&self, t: &str) -> bool {
        match self {
            TooltipGemData::Gem(g) => g.tags.contains(&Name::from(t)),
            TooltipGemData::MadeUp(_) => false,
        }
    }
    /// `gemData.naturalMaxLevel`
    pub fn natural_max_level(&self) -> Option<f64> {
        match self {
            TooltipGemData::Gem(g) => Some(g.naturalMaxLevel),
            TooltipGemData::MadeUp(_) => None,
        }
    }
    /// `gemData.reqStr` / `reqDex` / `reqInt`
    fn req(&self, f: fn(&crate::model::gamedata::GemData) -> f64) -> Option<f64> {
        match self {
            TooltipGemData::Gem(g) => Some(f(g)),
            TooltipGemData::MadeUp(_) => None,
        }
    }
}

/// The `gemInstance` table GemTooltip reads: `gemData`, `level`, `quality`, `displayEffect`.
#[derive(Clone)]
pub struct TooltipGemInstance {
    pub gem_data: TooltipGemData,
    pub level: f64,
    pub quality: f64,
    /// `gemInstance.displayEffect` (the calc's effect of a socket group's gem; nil otherwise)
    pub display_effect: Option<SkillEffectRef>,
}

impl TooltipGemInstance {
    /// A socket group's gem (`socketGroup.gemList[i]`, which has its `gemData`).
    pub fn of_gem(gem: &crate::model::GemInstance, gem_data: GemDataRef) -> TooltipGemInstance {
        TooltipGemInstance {
            gem_data: TooltipGemData::Gem(gem_data),
            level: gem.level.get(),
            quality: gem.quality.get(),
            display_effect: gem.displayEffect.get(),
        }
    }
}

/// `GemToolTipOptions` (L207-208).
#[derive(Clone, Copy, Default)]
pub struct GemTooltipOptions {
    pub skip_requirements: bool,
}

/// `displayInstance = gemInstance.displayEffect or gemInstance` (L32).
enum DisplayInstance<'a> {
    Effect(&'a SkillEffectRef),
    Gem(&'a TooltipGemInstance),
}

impl DisplayInstance<'_> {
    fn level(&self) -> f64 {
        match self {
            DisplayInstance::Effect(e) => e.level.get(),
            DisplayInstance::Gem(g) => g.level,
        }
    }
    /// `displayInstance.<k> or 0` for the quality sources (a gem table has none of them).
    fn quality_source(&self, k: &str) -> f64 {
        match self {
            DisplayInstance::Effect(e) => match k {
                "itemQuality" => e.itemQuality.get(),
                "supportQuality" => e.supportQuality.get(),
                "globalQuality" => e.globalQuality.get(),
                _ => e.extra.opt_num(Name::from(k)),
            }
            .unwrap_or(0.0),
            DisplayInstance::Gem(_) => 0.0,
        }
    }
    /// The skill instance `calcLib.buildSkillInstanceStats` reads (`quality`, `level`,
    /// `actorLevel`): the effect itself, or a gem table's `level`/`quality` (no `actorLevel`).
    fn as_skill_instance(&self, ge: &GrantedEffectRef) -> SkillEffectRef {
        match self {
            DisplayInstance::Effect(e) => (*e).clone(),
            DisplayInstance::Gem(g) => SkillEffect::new(ge.clone(), g.level, g.quality),
        }
    }
}

/// `grantedEffect.levels[level]` (nil when absent).
fn level_of(ge: &GrantedEffect, level: f64) -> Option<SkillLevelRef> {
    ge.levels.get(level).cloned()
}

/// `grantedEffectLevel[k]` as a number (nil: absent, or `levels[l]` was nil and Lua indexed `{}`).
fn lvl_num(l: Option<&SkillLevelRef>, k: &str) -> Option<f64> {
    l.and_then(|l| l.num(Name::from(k)))
}

/// `grantedEffectLevel.cost[r]` (SkillsTab.lua:1188 may have replaced `cost` with `{}`).
fn lvl_cost(ge: &GrantedEffect, level: f64, l: Option<&SkillLevelRef>, r: Name) -> Option<f64> {
    let l = l?;
    if pob_calc::model::skills_tab::levelCostCleared(ge.id, level) {
        return None;
    }
    l.cost.iter().find(|(n, _)| *n == r).map(|(_, v)| *v)
}

/// lua: GemTooltip.lua:11-13 getFontSizes
/// @pob Classes/GemTooltip.lua:11-13 h=09c518a2
fn get_font_sizes(cx: &Cx) -> (f64, f64) {
    // L12
    (
        if cx.main.show_flavour_text {
            18.0
        } else {
            16.0
        },
        if cx.main.show_flavour_text {
            24.0
        } else {
            20.0
        },
    )
}

/// lua: GemTooltip.lua:16-21 reservationMap (in Lua's constructor order; ORDER: see the module
/// notes).
/// @pob Classes/GemTooltip.lua:16-21 h=169e0266
const RESERVATION_MAP: [(&str, &str); 4] = [
    ("manaReservationFlat", "Mana"),
    ("manaReservationPercent", "ManaPercent"),
    ("lifeReservationFlat", "Life"),
    ("lifeReservationPercent", "LifePercent"),
];

/// L50-56 / L69-75: `reservation = (reservation and (reservation .. ", ") or "") ..
/// data.costs[isValueInArrayPred(data.costs, …)].ResourceString:gsub("{0}", ("%d"):format(v))`.
fn reservation_text(l: Option<&SkillLevelRef>) -> Option<String> {
    let costs = &crate::model::data().costs;
    let mut reservation: Option<String> = None;
    // ORDER: (c), see the module notes
    for (name, res) in RESERVATION_MAP {
        if let Some(v) = lvl_num(l, name) {
            let i = costs
                .list
                .iter()
                .position(|c| &*c.Resource == res)
                .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
            let s = pob_lua::gsub(&costs.list[i].ResourceString, "{0}", &lfmt!("%d", v), None).0;
            reservation = Some(format!(
                "{}{s}",
                reservation.map_or_else(String::new, |r| r + ", ")
            ));
        }
    }
    reservation
}

/// lua: GemTooltip.lua:23-205 addCommonGemInfo(tooltip, build, gemInstance, grantedEffect,
/// addLevel, addReq, mergeStatsFrom)
/// @pob Classes/GemTooltip.lua:23-205 h=33cc0d7f
#[allow(clippy::too_many_lines)]
fn add_common_gem_info(
    cx: &mut Cx,
    tooltip: &mut Tooltip,
    build: &BuildRef,
    gem_instance: &TooltipGemInstance,
    granted_effect: &GrantedEffectRef,
    add_level: bool,
    add_req: bool,
    merge_stats_from: Option<&GrantedEffectRef>,
) {
    let fs = Some(Font::FontinSc);
    // L31
    let font_size_big = if cx.main.show_flavour_text {
        18.0
    } else {
        16.0
    };
    // L32
    let display_instance = match &gem_instance.display_effect {
        Some(e) => DisplayInstance::Effect(e),
        None => DisplayInstance::Gem(gem_instance),
    };
    let display_level = display_instance.level();
    // L33
    let gel = level_of(granted_effect, display_level);
    let gel = gel.as_ref();
    let cc = cx.main.cc.clone();
    // L34
    if add_level {
        // L37-L39
        let delta = if display_level > gem_instance.level {
            format!(
                " ({}+{}^7)",
                cc.MAGIC,
                pob_lua::tostring(display_level - gem_instance.level)
            )
        } else if display_level < gem_instance.level {
            format!(
                " ({}-{}^7)",
                cc.WARNING,
                pob_lua::tostring(gem_instance.level - display_level)
            )
        } else {
            String::new()
        };
        // L40
        let max = if gem_instance.level
            >= gem_instance
                .gem_data
                .natural_max_level()
                .unwrap_or(f64::INFINITY)
        {
            " (Max)"
        } else {
            ""
        };
        // L35-L41
        let s = lfmt!(
            "^x7F7F7FLevel: ^7%d%s%s",
            gem_instance.level,
            delta.as_str(),
            max
        );
        tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
    }
    // L43
    if granted_effect.support {
        // L44
        if let Some(mm) = lvl_num(gel, "manaMultiplier") {
            // L45-L47
            let s = lfmt!("^x7F7F7FCost & Reservation Multiplier: ^7%d%%", mm + 100.0);
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L49-L56
        let reservation = reservation_text(gel);
        // L57
        if let Some(reservation) = reservation {
            // L58
            let s = format!("^x7F7F7FReservation Override: ^7{reservation}");
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L60
        if let Some(cooldown) = lvl_num(gel, "cooldown") {
            // L61
            let mut string = lfmt!("^x7F7F7FCooldown Time: ^7%.2f sec", cooldown);
            // L62
            if let Some(uses) = lvl_num(gel, "storedUses").filter(|u| *u > 1.0) {
                // L63
                string += &lfmt!(" (%d uses)", uses);
            }
            // L65
            tooltip.add_line_ex(cx, font_size_big, &string, fs, None, None);
        }
    } else {
        // L68-L75
        let reservation = reservation_text(gel);
        // L76
        if let Some(reservation) = reservation {
            // L77
            let s = format!("^x7F7F7FReservation: ^7{reservation}");
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L79
        let mut cost: Option<String> = None;
        // L80
        for res in &crate::model::data().costs.list {
            // L81
            if let Some(c) = lvl_cost(granted_effect, display_level, gel, res.Resource) {
                // L82-L84
                let n = lfmt!("%g", pob_lua::round(c / res.Divisor, Some(2)));
                let s = pob_lua::gsub(&res.ResourceString, "{0}", &n, None).0;
                cost = Some(format!(
                    "{}{s}",
                    cost.map_or_else(String::new, |c| c + ", ")
                ));
            }
        }
        // L87
        if let Some(cost) = cost {
            // L88
            let s = format!("^x7F7F7FCost: ^7{cost}");
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L90
        if let Some(cooldown) = lvl_num(gel, "cooldown") {
            // L91 (the extra "FONTIN SC" argument to string.format is unused)
            let mut string = lfmt!("^x7F7F7FCooldown Time: ^7%.2f sec", cooldown);
            // L92
            if let Some(uses) = lvl_num(gel, "storedUses").filter(|u| *u > 1.0) {
                // L93
                string += &lfmt!(" (%d uses)", uses);
            }
            // L95
            tooltip.add_line_ex(cx, font_size_big, &string, fs, None, None);
        }
        // L97
        if let Some(vsu) = lvl_num(gel, "vaalStoredUses") {
            // L98-L100
            let soul = lvl_cost(granted_effect, display_level, gel, Name::from("Soul"))
                .unwrap_or_else(|| {
                    lua_error!("attempt to perform arithmetic on field 'Soul' (a nil value)")
                });
            let s = lfmt!(
                "^x7F7F7FCan Store ^7%d ^x7F7F7FUse (%d Souls)",
                vsu,
                vsu * soul
            );
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L102
        if let Some(spd) = lvl_num(gel, "soulPreventionDuration") {
            // L103-L105
            let s = lfmt!("^x7F7F7FSoul Gain Prevention: ^7%d sec", spd);
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L107
        if gem_instance.gem_data.has_tag("attack") {
            // L108
            if let Some(asm) = lvl_num(gel, "attackSpeedMultiplier") {
                // L109-L111
                let s = lfmt!("^x7F7F7FAttack Speed: ^7%d%% of base", asm + 100.0);
                tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
            }
            // L113
            if let Some(at) = lvl_num(gel, "attackTime") {
                // L114-L115
                let s = lfmt!("^x7F7F7FAttack Time: ^7%.2f sec", at / 1000.0);
                tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
            }
            // L117
            if let Some(bm) = lvl_num(gel, "baseMultiplier") {
                // L118-L120
                let s = lfmt!("^x7F7F7FAttack Damage: ^7%g%% of base", bm * 100.0);
                tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
            }
        } else {
            // L123
            let cast_time = granted_effect
                .castTime
                .unwrap_or_else(|| lua_error!("attempt to compare nil with number"));
            if cast_time > 0.0 {
                // L124-L125
                let s = lfmt!("^x7F7F7FCast Time: ^7%.2f sec", cast_time);
                tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
            } else {
                // L127
                tooltip.add_line_ex(
                    cx,
                    font_size_big,
                    "^x7F7F7FCast Time: ^7Instant",
                    fs,
                    None,
                    None,
                );
            }
        }
        // L130
        if let Some(cc_) = lvl_num(gel, "critChance") {
            // L131-L132
            let s = lfmt!("^x7F7F7FCritical Strike Chance: ^7%.2f%%", cc_);
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L134
        if let Some(de) = lvl_num(gel, "damageEffectiveness") {
            // L135-L137
            let s = lfmt!("^x7F7F7FEffectiveness of Added Damage: ^7%d%%", de * 100.0);
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
    }
    // L140
    if add_req {
        // L141
        if gem_instance.quality > 0.0 {
            // L142
            let s = lfmt!(
                "^x7F7F7FQuality: +%s%d%%",
                cc.MAGIC.as_str(),
                gem_instance.quality
            );
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L144-L146
        let format_quality = |number: f64, suffix: &str| {
            format!(
                "{}{}",
                cc.MAGIC,
                lfmt!("+%d%% Quality from %s", number, suffix)
            )
        };
        // L147
        let q = display_instance.quality_source("itemQuality");
        if q > 0.0 {
            // L148
            tooltip.add_line_ex(
                cx,
                font_size_big,
                &format_quality(q, "Item"),
                fs,
                None,
                None,
            );
        }
        // L150
        let q = display_instance.quality_source("supportQuality");
        if q > 0.0 {
            // L151
            tooltip.add_line_ex(
                cx,
                font_size_big,
                &format_quality(q, "Support"),
                fs,
                None,
                None,
            );
        }
        // L153
        let q = display_instance.quality_source("globalQuality");
        if q > 0.0 {
            // L154
            tooltip.add_line_ex(
                cx,
                font_size_big,
                &format_quality(q, "Global Modifiers"),
                fs,
                None,
                None,
            );
        }
        // L156
        let q = display_instance.quality_source("socketQuality");
        if q > 0.0 {
            // L157
            tooltip.add_line_ex(
                cx,
                font_size_big,
                &format_quality(q, "Socket Colour"),
                fs,
                None,
                None,
            );
        }
    }
    // L160
    tooltip.add_separator(cx, Some(10.0));
    // L161
    if add_req {
        // L162-L163
        let req_level =
            level_of(granted_effect, gem_instance.level).map_or(1.0, |l| l.levelRequirement);
        let gd = &gem_instance.gem_data;
        let num = |v: Option<f64>| {
            v.unwrap_or_else(|| {
                lua_error!("attempt to perform arithmetic on local 'multi' (a nil value)")
            })
        };
        // L164
        let req_str = pob_calc::modules::calc_tools::getGemStatRequirement(
            req_level,
            granted_effect.support,
            num(gd.req(|g| g.reqStr)),
        );
        // L165
        let req_dex = pob_calc::modules::calc_tools::getGemStatRequirement(
            req_level,
            granted_effect.support,
            num(gd.req(|g| g.reqDex)),
        );
        // L166
        let req_int = pob_calc::modules::calc_tools::getGemStatRequirement(
            req_level,
            granted_effect.support,
            num(gd.req(|g| g.reqInt)),
        );
        // L167
        stats::add_requirements_to_tooltip(
            cx,
            tooltip,
            Some(req_level),
            Some(req_str),
            Some(req_dex),
            Some(req_int),
            None,
            None,
            None,
        );
    }
    // L169
    if let Some(desc) = &granted_effect.description {
        // L170
        let s = format!("{}{desc}", cc.GEM);
        tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
    }
    // L172 (`build.data.describeStats` is always set: Data.lua:646)
    {
        // L173
        tooltip.add_separator(cx, Some(10.0));
        // L174
        let stats_t = pob_calc::modules::calc_tools::buildSkillInstanceStats(
            &display_instance.as_skill_instance(granted_effect),
            granted_effect,
        );
        // L175
        if let Some(merge) = merge_stats_from {
            // L176
            let extra = pob_calc::modules::calc_tools::buildSkillInstanceStats(
                &display_instance.as_skill_instance(merge),
                merge,
            );
            for (stat, val) in extra.pairs() {
                // L177
                let v = stats_t.get(stat);
                let base = if v.is_nil() { 0.0 } else { v.num() };
                stats_t.set(stat, base + val.num());
            }
        }
        // L180
        let scope = granted_effect
            .statDescriptionScope
            .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
        let (descriptions, line_map) =
            stat_describer::describe_stats(cx, &stats_t, scope.as_str(), false);
        let _ = build;
        let dev_mode_alt = cx.app().launch.dev_mode_alt;
        // L181
        for line in descriptions {
            let mut line = line;
            let stat = line_map.get(&line).copied();
            // L182 (`grantedEffect.statMap[k]` already falls back to data.skillStatMap; a nil key
            // is a nil read)
            let source = stat.and_then(|s| {
                pob_gamedata::skill_stat_map::statMapLookup(granted_effect, s)
                    .or_else(|| crate::model::data().skillStatMap.get(&s).cloned())
            });
            // L183
            if let Some(source) = source {
                // L184
                if dev_mode_alt {
                    // L185
                    let mut dev_text = stat.map(|s| s.to_string()).unwrap_or_default();
                    // L186
                    if let Some(first) = source.items.first() {
                        let m = match first {
                            pob_gamedata::schema::skills::StatMapItem::Mod(m) => m,
                            pob_gamedata::schema::skills::StatMapItem::Group { .. } => {
                                lua_error!("attempt to concatenate field 'name' (a nil value)")
                            }
                        };
                        // L187-L189 DIVERGENCE(immutable data): format a copy holding the value
                        let mut m = (**m).clone();
                        if !m.value.truthy() {
                            m.value = pob_gamedata::Value::from(stat.unwrap_or_else(|| {
                                lua_error!("attempt to concatenate a nil value")
                            }));
                        }
                        // L190
                        dev_text = pob_gamedata::mod_tools::formatMod(&m);
                    }
                    // L192
                    line = format!("{line} ^2{dev_text}");
                }
                // L194
                let s = format!("{}{line}", cc.MAGIC);
                tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
            } else {
                // L196
                if dev_mode_alt {
                    // L197
                    let s =
                        stat.unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
                    line = format!("{line} ^1{s}");
                }
                // L199
                let line = format!("{}{line}", cc.UNSUPPORTED);
                // L200
                let line = if cx.main.not_supported_mod_tooltips {
                    format!("{line}{}", cx.app().main.not_supported_tooltip_text)
                } else {
                    line
                };
                // L201
                tooltip.add_line_ex(cx, font_size_big, &line, fs, None, None);
            }
        }
    }
}

/// lua: GemTooltip.lua:207-259 GemTooltip.AddGemTooltip(tooltip, build, gemInstance, options)
/// @pob Classes/GemTooltip.lua:207-259 h=5142d07a
pub fn add_gem_tooltip(
    cx: &mut Cx,
    tooltip: &mut Tooltip,
    build: &BuildRef,
    gem_instance: &TooltipGemInstance,
    options: Option<GemTooltipOptions>,
) {
    let fs = Some(Font::FontinSc);
    // L214
    let options = options.unwrap_or_default();
    // L215
    let (font_size_big, font_size_title) = get_font_sizes(cx);
    // L216
    let skip_requirements = options.skip_requirements;
    let cc = cx.main.cc.clone();
    // L217
    tooltip.center = true;
    // L218
    tooltip.color = Some(TtColor::Esc(cc.GEM.clone()));
    // L219
    tooltip.max_width = Some(600.0);
    // L220
    tooltip.tooltip_header = Some("GEM".to_owned());
    // L221 tooltip.gemIcon = …: never read (module notes)

    // L223
    let primary = gem_instance.gem_data.granted_effect().clone();
    // L224
    let secondary = gem_instance.gem_data.secondary_granted_effect().cloned();

    // L226
    if let Some(secondary) = secondary
        .clone()
        .filter(|s| !s.support || gem_instance.gem_data.secondary_effect_name().is_some())
    {
        // L227 LUA-QUIRK: `gemData.VaalGem` is never set (module notes)
        let granted_effect = primary.clone();
        // L228
        let granted_effect_secondary = secondary;
        // L229
        let s = format!("{}{}", cc.GEM, granted_effect.name);
        tooltip.add_line_ex(cx, font_size_title, &s, fs, None, None);
        // L230
        tooltip.add_separator(cx, Some(10.0));
        // L231
        let tag_string = gem_instance.gem_data.tag_string().unwrap_or_else(|| {
            lua_error!("attempt to concatenate field 'tagString' (a nil value)")
        });
        let s = format!("^x7F7F7F{tag_string}");
        tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        // L232
        add_common_gem_info(
            cx,
            tooltip,
            build,
            gem_instance,
            &granted_effect,
            true,
            !skip_requirements,
            None,
        );
        // L233
        tooltip.add_separator(cx, Some(10.0));
        // L234-L235
        let name = gem_instance
            .gem_data
            .secondary_effect_name()
            .unwrap_or(granted_effect_secondary.name);
        let s = format!("{}{name}", cc.GEM);
        tooltip.add_line_ex(cx, font_size_title, &s, fs, None, None);
        // L236
        tooltip.add_separator(cx, Some(10.0));
        // L237
        add_common_gem_info(
            cx,
            tooltip,
            build,
            gem_instance,
            &granted_effect_secondary,
            false,
            false,
            None,
        );
    } else {
        // L239
        let granted_effect = gem_instance.gem_data.granted_effect().clone();
        // L240
        let s = format!("{}{}", cc.GEM, granted_effect.name);
        tooltip.add_line_ex(cx, font_size_title, &s, fs, None, None);
        // L241
        tooltip.add_separator(cx, Some(10.0));
        // L242
        if granted_effect.legacy {
            // L243
            let s = format!("{}Legacy Gem", cc.WARNING);
            tooltip.add_line_ex(cx, font_size_title, &s, fs, None, None);
            // L244
            let s = format!("{}Gem only exists in Standard League", cc.WARNING);
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
            // L245
            tooltip.add_separator(cx, Some(10.0));
        }
        // L247
        if let Some(tag_string) = gem_instance.gem_data.tag_string() {
            // L248
            let s = format!("^x7F7F7F{tag_string}");
            tooltip.add_line_ex(cx, font_size_big, &s, fs, None, None);
        }
        // L250-L251
        let merge = secondary.as_ref().filter(|s| s.support);
        add_common_gem_info(
            cx,
            tooltip,
            build,
            gem_instance,
            &granted_effect,
            true,
            !skip_requirements,
            merge,
        );
    }
    // L253
    if !primary.flavourText.is_empty() && cx.main.show_flavour_text {
        // L254
        tooltip.add_separator(cx, Some(10.0));
        // L255
        for line in &primary.flavourText {
            // L256
            let s = format!("{}{line}", cc.UNIQUE);
            tooltip.add_line_ex(
                cx,
                font_size_big,
                &s,
                Some(Font::FontinScItalic),
                None,
                None,
            );
        }
    }
}

// ---- Modules/StatDescriber.lua --------------------------------------------------------------

/// `data.describeStats` (Data.lua:646 `LoadModule("Modules/StatDescriber")`): a port of
/// Modules/StatDescriber.lua (CCR-0108 §1: no unit owns it; GemTooltip needs it, TreeTab.lua:2007
/// too). The descriptions are the dumped `StatDescriptions.<scope>` tables
/// (`data().statDescriptions`), typed once per scope.
pub mod stat_describer {
    use super::*;
    use pob_gamedata::DataValue;
    use std::collections::HashMap;

    // @pob-skip Modules/StatDescriber.lua:1-5 reason="file header comment"
    // @pob-skip Modules/StatDescriber.lua:12-12 reason="blank line"
    // @pob-skip Modules/StatDescriber.lua:30-30 reason="blank line"
    // @pob-skip Modules/StatDescriber.lua:52-52 reason="blank line"
    // @pob-skip Modules/StatDescriber.lua:206-206 reason="blank line"

    /// One bound of a `limit` pair: a number, `"#"` (any) or `"!"` (not equal).
    #[derive(Clone, Debug, PartialEq)]
    pub enum Limit {
        Num(f64),
        Str(String),
    }

    /// `{ k = …, v = … }` (a description function and the stat index it applies to).
    #[derive(Clone, Debug)]
    pub struct Special {
        pub k: Option<String>,
        pub v: Option<f64>,
    }

    /// One description line of a language list: `{ limit = { {a, b}, … }, text = …,
    /// gem_quality = …, [1] = special, … }`.
    #[derive(Clone, Debug)]
    pub struct Desc {
        pub limit: Vec<(Limit, Limit)>,
        pub text: String,
        pub gem_quality: bool,
        pub specials: Vec<Special>,
    }

    /// `scope[i]`: `{ stats = { … }, [1] = { desc, … } }` (only the English list is read).
    #[derive(Clone, Debug)]
    pub struct Descriptor {
        pub stats: Vec<Name>,
        pub lang: Option<Vec<Desc>>,
    }

    /// The dumped scope table without `scopeList` (`scope[s]` → index, `scope[i]` → descriptor).
    #[derive(Debug)]
    pub struct ScopeData {
        pub name: String,
        pub parent: Option<String>,
        pub index: HashMap<String, f64>,
        pub descriptors: Vec<Option<Rc<Descriptor>>>,
    }

    impl ScopeData {
        /// `scope[s]` for a stat name.
        fn order(&self, s: &str) -> Option<f64> {
            self.index.get(s).copied()
        }
        /// `scope[i]`
        fn descriptor(&self, i: f64) -> Option<&Rc<Descriptor>> {
            if i >= 1.0 && i.fract() == 0.0 {
                self.descriptors
                    .get(i as usize - 1)
                    .and_then(Option::as_ref)
            } else {
                None
            }
        }
    }

    /// A loaded scope: its data and `scope.scopeList` (itself first, then its parents').
    #[derive(Clone)]
    pub struct Scope {
        pub data: Rc<ScopeData>,
        pub scope_list: Vec<Rc<ScopeData>>,
    }

    fn limit_of(v: &DataValue) -> Limit {
        match v {
            DataValue::Num(n) => Limit::Num(*n),
            DataValue::Str(s) => Limit::Str(s.to_string()),
            _ => Limit::Str(String::new()),
        }
    }

    fn desc_of(v: &DataValue) -> Desc {
        let t = v
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index a non-table description"));
        let limit = t
            .field("limit")
            .table()
            .map(|l| {
                l.seq
                    .iter()
                    .map(|p| (limit_of(p.field_index(1)), limit_of(p.field_index(2))))
                    .collect()
            })
            .unwrap_or_default();
        Desc {
            limit,
            text: t.field("text").as_str().unwrap_or_default().to_owned(),
            gem_quality: t.field("gem_quality").truthy(),
            specials: t
                .seq
                .iter()
                .map(|s| Special {
                    k: s.field("k").as_str().map(str::to_owned),
                    v: s.field("v").num(),
                })
                .collect(),
        }
    }

    trait FieldIndex {
        fn field_index(&self, i: usize) -> &DataValue;
    }
    impl FieldIndex for DataValue {
        fn field_index(&self, i: usize) -> &DataValue {
            static NIL: DataValue = DataValue::Nil;
            match self {
                DataValue::Table(t) => t.index(i as f64),
                _ => &NIL,
            }
        }
    }

    fn scope_data_of(name: &str) -> ScopeData {
        // L15 LoadModule("Data/StatDescriptions/"..scopeName)
        let v = crate::model::data()
            .statDescriptions
            .scope(name)
            .unwrap_or_else(|| {
                lua_error!("Data/StatDescriptions/{name}.lua: No such file or directory")
            });
        let t = v
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'scope'"));
        let mut index = HashMap::new();
        let mut parent = None;
        for (k, v) in &t.fields {
            match v {
                DataValue::Num(n) => {
                    index.insert(k.to_string(), *n);
                }
                DataValue::Str(s) if &**k == "parent" => parent = Some(s.to_string()),
                _ => {}
            }
        }
        let descriptors = t
            .seq
            .iter()
            .map(|d| {
                let dt = d.table()?;
                Some(Rc::new(Descriptor {
                    stats: dt
                        .field("stats")
                        .table()
                        .map(|s| {
                            s.seq
                                .iter()
                                .filter_map(|x| x.as_str().map(Name::from))
                                .collect()
                        })
                        .unwrap_or_default(),
                    lang: dt
                        .seq
                        .first()
                        .and_then(DataValue::table)
                        .map(|l| l.seq.iter().map(desc_of).collect()),
                }))
            })
            .collect();
        ScopeData {
            name: name.to_owned(),
            parent,
            index,
            descriptors,
        }
    }

    thread_local! {
        /// L11 `local scopes = { }`
        static SCOPES: RefCell<HashMap<String, Scope>> = RefCell::new(HashMap::new());
    }

    /// lua: StatDescriber.lua:6-11 (the `pairs`/`ipairs`/`t_insert`/`s_format` locals and
    /// `scopes`, the [`SCOPES`] cache)
    /// @pob Modules/StatDescriber.lua:6-11 h=683274f8
    pub fn forget_scopes() {
        SCOPES.with(|s| s.borrow_mut().clear());
    }

    /// lua: StatDescriber.lua:13-29 getScope(scopeName)
    /// @pob Modules/StatDescriber.lua:13-29 h=f79745b9
    pub fn get_scope(scope_name: &str) -> Scope {
        // L14
        if let Some(s) = SCOPES.with(|s| s.borrow().get(scope_name).cloned()) {
            // L27
            return s;
        }
        // L15-L16
        let data = Rc::new(scope_data_of(scope_name));
        // L17
        let mut scope_list = if let Some(parent) = data.parent.clone() {
            // L18
            let parent_scope = get_scope(&parent);
            // L19
            parent_scope.scope_list.clone()
        } else {
            // L21
            Vec::new()
        };
        // L23
        scope_list.insert(0, data.clone());
        let scope = Scope { data, scope_list };
        // L24
        SCOPES.with(|s| s.borrow_mut().insert(scope_name.to_owned(), scope.clone()));
        // L25
        scope
    }

    /// A described value: a number, or the gem name `display_indexable_non_active_support`
    /// stores (L200).
    #[derive(Clone, Debug, PartialEq)]
    pub enum SV {
        Num(f64),
        Str(String),
    }

    impl SV {
        #[track_caller]
        fn n(&self) -> f64 {
            match self {
                SV::Num(n) => *n,
                SV::Str(_) => lua_error!("attempt to perform arithmetic on a string value"),
            }
        }
        fn arg(&self) -> pob_lua::FmtArg<'_> {
            match self {
                SV::Num(n) => pob_lua::FmtArg::Num(*n),
                SV::Str(s) => pob_lua::FmtArg::Str(s),
            }
        }
    }

    /// `val[i] = { min = …, max = …, fmt = … }`
    #[derive(Clone, Debug)]
    pub struct V {
        pub min: SV,
        pub max: SV,
        pub fmt: String,
    }

    /// `a < b` / `a > b` of a value with a limit bound (numbers; a string is Lua's compare error).
    #[track_caller]
    fn lnum(l: &Limit) -> f64 {
        match l {
            Limit::Num(n) => *n,
            Limit::Str(_) => lua_error!("attempt to compare number with string"),
        }
    }

    /// lua: StatDescriber.lua:31-51 matchLimit(lang, val, quality)
    /// @pob Modules/StatDescriber.lua:31-51 h=4aa72226
    fn match_limit<'a>(lang: &'a [Desc], val: &[V], quality: bool) -> Option<&'a Desc> {
        // L32
        for desc in lang {
            // L33 Skip gem_quality lines for regular mods
            if quality || !desc.gem_quality {
                // L34
                let mut r#match = true;
                // L35
                for (i0, limit) in desc.limit.iter().enumerate() {
                    let v = val
                        .get(i0)
                        .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
                    // L36
                    if limit.0 == Limit::Str("!".to_owned()) {
                        // L37
                        if v.min == SV::Num(lnum(&limit.1)) {
                            // L38
                            r#match = false;
                            // L39
                            break;
                        }
                    } else if (limit.1 != Limit::Str("#".to_owned()) && v.min.n() > lnum(&limit.1))
                        || (limit.0 != Limit::Str("#".to_owned()) && v.min.n() < lnum(&limit.0))
                    {
                        // L41-L42
                        r#match = false;
                        // L43
                        break;
                    }
                }
                // L46
                if r#match {
                    // L47
                    return Some(desc);
                }
            }
        }
        None
    }

    /// lua: StatDescriber.lua:53-205 (`indexableNonActiveSupports = require("Data.PearlSupports")`
    /// and applySpecial(val, spec))
    /// @pob Modules/StatDescriber.lua:53-205 h=0e786647
    #[allow(clippy::too_many_lines)]
    fn apply_special(cx: &mut Cx, val: &mut [V], spec: &Special) {
        let k = spec.k.as_deref();
        let Some(k) = k else {
            // L202 elseif spec.k then … (nil: nothing)
            return;
        };
        let len = val.len();
        let iv = || {
            let v = spec.v.unwrap_or(f64::NAN);
            if v >= 1.0 && v.fract() == 0.0 && (v as usize) <= len {
                v as usize - 1
            } else {
                lua_error!("attempt to index field '?' (a nil value)")
            }
        };
        let round = pob_lua::round;
        // `val[spec.v].min = f(min); val[spec.v].max = f(max)` (min first, as the Lua writes it)
        let both = |val: &mut [V], f: &dyn Fn(f64) -> f64, fmt: Option<&str>| {
            let i = iv();
            val[i].min = SV::Num(f(val[i].min.n()));
            val[i].max = SV::Num(f(val[i].max.n()));
            if let Some(fmt) = fmt {
                val[i].fmt = fmt.to_owned();
            }
        };
        match k {
            // L55-L56
            "negate" => {
                let i = iv();
                let (mn, mx) = (val[i].min.n(), val[i].max.n());
                val[i].max = SV::Num(-mn);
                val[i].min = SV::Num(-mx);
            }
            // L57-L58
            "invert_chance" => {
                let i = iv();
                let (mn, mx) = (val[i].min.n(), val[i].max.n());
                val[i].max = SV::Num(100.0 - mn);
                val[i].min = SV::Num(100.0 - mx);
            }
            // L59-L60
            "negate_and_double" => {
                let i = iv();
                let (mn, mx) = (val[i].min.n(), val[i].max.n());
                val[i].max = SV::Num(-2.0 * mn);
                val[i].min = SV::Num(-2.0 * mx);
            }
            // L61-L63
            "divide_by_two_0dp" => both(val, &|x| x / 2.0, None),
            // L64-L66
            "divide_by_ten_0dp" => both(val, &|x| x / 10.0, None),
            // L67-L69
            "divide_by_fifteen_0dp" => both(val, &|x| x / 15.0, None),
            // L70-L73
            "divide_by_four" => both(val, &|x| x / 4.0, Some("g")),
            // L74-L77
            "divide_by_five" => both(val, &|x| x / 5.0, Some("g")),
            // L78-L81
            "divide_by_six" => both(val, &|x| x / 6.0, Some("g")),
            // L82-L85
            "divide_by_ten_1dp_if_required" | "divide_by_ten_1dp" => {
                both(val, &|x| round(x / 10.0, Some(1)), Some("g"));
            }
            // L86-L89
            "divide_by_twelve" => both(val, &|x| x / 12.0, Some("g")),
            // L90-L93
            "divide_by_twenty" => both(val, &|x| x / 20.0, Some("g")),
            // L94-L97
            "divide_by_one_hundred" => both(val, &|x| x / 100.0, Some("g")),
            // L98-L101
            "divide_by_one_hundred_1dp" => both(val, &|x| round(x / 100.0, Some(1)), Some("g")),
            // L102-L105
            "divide_by_one_hundred_2dp_if_required" | "divide_by_one_hundred_2dp" => {
                both(val, &|x| round(x / 100.0, Some(2)), Some("g"));
            }
            // L106-L109
            "divide_by_one_hundred_and_negate" => {
                both(val, &|x| -round(x / 100.0, Some(1)), Some("g"));
            }
            // L110-L112 -- O_O
            "divide_by_twenty_then_double_0dp" => both(val, &|x| round(x / 20.0, None) * 2.0, None),
            // L113-L116
            "divide_by_one_thousand" => both(val, &|x| x / 1000.0, Some("g")),
            // L117-L120
            "per_minute_to_per_second" => both(val, &|x| round(x / 60.0, Some(1)), Some("g")),
            // L121-L124
            "permyriad_per_minute_to_%_per_second" => {
                both(val, &|x| round(x / 60.0 / 100.0, Some(1)), Some("g"));
            }
            // L125-L127
            "per_minute_to_per_second_0dp" => both(val, &|x| x / 60.0, None),
            // L128-L131
            "per_minute_to_per_second_1dp" => both(val, &|x| round(x / 60.0, Some(1)), Some("g")),
            // L132-L135
            "per_minute_to_per_second_2dp" => both(val, &|x| round(x / 60.0, Some(2)), Some("g")),
            // L136-L139
            "per_minute_to_per_second_2dp_if_required" => {
                both(val, &|x| round(x / 60.0, Some(2)), Some("g"));
            }
            // L140-L143
            "milliseconds_to_seconds" => both(val, &|x| x / 1000.0, Some("g")),
            // L144-L146
            "milliseconds_to_seconds_0dp" => both(val, &|x| x / 1000.0, None),
            // L147-L150
            "milliseconds_to_seconds_1dp" => both(val, &|x| round(x / 1000.0, Some(1)), Some("g")),
            // L151-L154
            "milliseconds_to_seconds_2dp" => both(val, &|x| round(x / 1000.0, Some(2)), Some("g")),
            // L155-L158
            "milliseconds_to_seconds_2dp_if_required" => {
                both(val, &|x| round(x / 1000.0, Some(2)), Some("g"));
            }
            // L159-L162
            "deciseconds_to_seconds" => both(val, &|x| x / 10.0, Some(".2f")),
            // L163-L166
            "locations_to_metres" => both(val, &|x| x / 10.0, Some("g")),
            // L167-L169
            "30%_of_value" => both(val, &|x| x * 0.3, None),
            // L170-L172
            "60%_of_value" => both(val, &|x| x * 0.6, None),
            // L173-L175
            "multiplicative_damage_modifier" => both(val, &|x| 100.0 + x, None),
            // L176-L179
            "multiplicative_permyriad_damage_modifier" => {
                both(val, &|x| 100.0 + round(x / 100.0, Some(1)), Some("g"));
            }
            // L180-L182
            "multiply_by_four" => both(val, &|x| x * 4.0, None),
            // L183-L185
            "times_one_point_five" => both(val, &|x| x * 1.5, None),
            // L186-L188
            "times_twenty" => both(val, &|x| x * 20.0, None),
            // L189-L191
            "double" => both(val, &|x| x * 2.0, None),
            // L192-L194
            "plus_two_hundred" => both(val, &|x| x + 200.0, None),
            // L195
            "reminderstring" | "canonical_line" | "canonical_stat" | "_stat" => {}
            // L196
            "display_indexable_non_active_support" => {
                let i = iv();
                // L197 indexableNonActiveSupports[val[spec.v].min]
                let min = val[i].min.n();
                let gem = if min >= 1.0 && min.fract() == 0.0 {
                    crate::model::data().pearlSupports.get(min as usize - 1)
                } else {
                    None
                };
                // L198
                let gem_text = gem.map_or_else(String::new, |g| {
                    pob_lua::gsub(&g.baseItemName, " Support", "", None).0
                });
                // L199
                val[i].fmt = "s".to_owned();
                // L200
                val[i].min = SV::Str(gem_text.clone());
                // L201
                val[i].max = SV::Str(gem_text);
            }
            // L202-L203
            _ => cx
                .sys
                .platform
                .con_print(&lfmt!("Unknown description function: %s", k)),
        }
    }

    /// The `{n}`-style replacement of L258-L263 / L265-L270 / L272-L277.
    fn fmt_val(v: &V) -> String {
        if v.min == v.max {
            pob_lua::lua_format(&format!("%{}", v.fmt), &[v.min.arg()])
        } else {
            pob_lua::lua_format(&format!("(%{0}-%{0})", v.fmt), &[v.min.arg(), v.max.arg()])
        }
    }

    /// `val[tonumber(n)+1]` (a missing entry is a nil index).
    #[track_caller]
    fn val_at<'v>(val: &'v [V], n: &[u8]) -> &'v V {
        let i = std::str::from_utf8(n)
            .ok()
            .and_then(pob_lua::tonumber)
            .unwrap_or(f64::NAN)
            + 1.0;
        if i >= 1.0 && i.fract() == 0.0 && (i as usize) <= val.len() {
            &val[i as usize - 1]
        } else {
            lua_error!("attempt to index local 'v' (a nil value)")
        }
    }

    /// lua: StatDescriber.lua:207-299 the module's function `(stats, scopeName, quality)`:
    /// the description lines and `lineMap` (line → the last present stat of its descriptor).
    /// `stats` maps stat names to numbers (the `{min, max}` table form is not used by the
    /// callers this port has).
    /// @pob Modules/StatDescriber.lua:207-299 h=dbfa6f8d
    #[allow(clippy::too_many_lines)]
    pub fn describe_stats(
        cx: &mut Cx,
        stats: &Table,
        scope_name: &str,
        quality: bool,
    ) -> (Vec<String>, HashMap<String, Name>) {
        struct Entry {
            depth: f64,
            order: f64,
            description: Rc<Descriptor>,
        }
        // L208
        let root_scope = get_scope(scope_name);

        // L210 Figure out which descriptions we need, and identify them by the first stat that they describe
        // L211
        let mut describe_stats: indexmap::IndexMap<Name, Entry> = indexmap::IndexMap::new();
        // L212 ORDER: (c), module notes
        for (s, v) in stats.pairs() {
            let Some(s) = s.name() else { continue };
            // L213 (numbers only: see the doc comment)
            let n = v.opt_num();
            if n.is_some_and(|n| n != 0.0) {
                // L214
                for (depth0, scope) in root_scope.scope_list.iter().enumerate() {
                    // L215
                    if let Some(order) = scope.order(s.as_str()) {
                        // L216
                        let descriptor = scope
                            .descriptor(order)
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index local 'descriptor' (a nil value)")
                            })
                            .clone();
                        // L217
                        if descriptor.lang.is_some() {
                            // L218
                            let first = *descriptor
                                .stats
                                .first()
                                .unwrap_or_else(|| lua_error!("table index is nil"));
                            describe_stats.insert(
                                first,
                                Entry {
                                    depth: (depth0 + 1) as f64,
                                    order,
                                    description: descriptor,
                                },
                            );
                        }
                        // L220
                        break;
                    }
                }
            }
        }

        // L226 Sort them by depth/order
        // L227-L230
        let mut desc_ordered: Vec<Entry> = describe_stats.into_values().collect();
        // L231
        pob_lua::sort(&mut desc_ordered, |a, b| {
            if a.depth != b.depth {
                a.depth > b.depth
            } else {
                a.order < b.order
            }
        });

        // L233 Describe the stats
        // L234
        let mut out: Vec<String> = Vec::new();
        // L235
        let mut line_map: HashMap<String, Name> = HashMap::new();
        // L236
        for descriptor in &desc_ordered {
            // L237
            let mut val: Vec<V> = Vec::new();
            // L238
            let mut stat: Option<Name> = None;
            // L239
            for s in &descriptor.description.stats {
                let sv = stats.get(*s);
                // L240
                if sv.truthy() {
                    // L241-L245 (a number)
                    let n = sv.num();
                    val.push(V {
                        min: SV::Num(n),
                        max: SV::Num(n),
                        fmt: String::new(),
                    });
                    // L246
                    stat = Some(*s);
                } else {
                    // L248
                    val.push(V {
                        min: SV::Num(0.0),
                        max: SV::Num(0.0),
                        fmt: String::new(),
                    });
                }
                // L250
                val.last_mut().expect("pushed").fmt = "d".to_owned();
            }
            // L252
            let lang = descriptor.description.lang.as_deref().unwrap_or(&[]);
            let desc = match_limit(lang, &val, quality).cloned();
            // L253
            if let Some(desc) = desc {
                // L254
                for spec in &desc.specials {
                    // L255
                    apply_special(cx, &mut val, spec);
                }
                // L257-L263
                let s1 = pob_lua::gsub_fn(
                    &desc.text,
                    "{(%d)}",
                    |c| Some(fmt_val(val_at(&val, c[0].as_bytes()))),
                    None,
                )
                .0;
                // L264-L270
                let first = || {
                    val.first()
                        .unwrap_or_else(|| lua_error!("attempt to index local 'v' (a nil value)"))
                };
                let s2 = pob_lua::gsub_fn(&s1, "{}", |_| Some(fmt_val(first())), None).0;
                // L271-L277
                let s3 = pob_lua::gsub_fn(&s2, "{:%+?d}", |_| Some(fmt_val(first())), None).0;
                // L278-L291
                let s4 = pob_lua::gsub_fn(
                    &s3,
                    "{(%d):(%+?)d}",
                    |c| {
                        let v = val_at(&val, c[0].as_bytes());
                        let fmt = c[1].to_str().into_owned();
                        Some(if v.min == v.max {
                            pob_lua::lua_format(&format!("%{fmt}{}", v.fmt), &[v.min.arg()])
                        } else if fmt == "+" {
                            if v.max.n() < 0.0 {
                                lfmt!("-(%d-%d)", -v.min.n(), -v.max.n())
                            } else {
                                lfmt!("+(%d-%d)", v.min.n(), v.max.n())
                            }
                        } else {
                            pob_lua::lua_format(
                                &format!("(%{fmt}{0}-%{fmt}{0})", v.fmt),
                                &[v.min.arg(), v.max.arg()],
                            )
                        })
                    },
                    None,
                )
                .0;
                let stat_desc = pob_lua::gsub(&s4, "%%%%", "%%", None).0;
                // L292
                let src = format!("{stat_desc}\\n");
                for c in pob_lua::gmatch(&src, r"([^\]+)\n") {
                    let line = c[0].to_str().into_owned();
                    // L293
                    out.push(line.clone());
                    // L294 (a nil `stat` removes the key)
                    match stat {
                        Some(s) => {
                            line_map.insert(line, s);
                        }
                        None => {
                            line_map.remove(&line);
                        }
                    }
                }
            }
        }
        // L298
        (out, line_map)
    }
}
