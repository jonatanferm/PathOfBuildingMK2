//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! perform segment PF-D (CalcPerform.lua 1943-2307): attributes, pools, charges.
//!
//! Owner: PF-D. The callees of this range that other units own (`doActorAttribsConditions`,
//! `doActorLifeMana`, `doActorLifeManaReservation`, `getCachedOutputValue` (PF-A),
//! `addMinionModifiers`, `doActorCharges` (PF-B), `mergeTinctures` (PF-C)) are called through
//! [`callee`] / [`cached_output`], which let the oracle's L4 test (feature `probe`) replay each
//! call's recorded state delta instead (CCR-0093); without an installed hook they are plain calls.
#![allow(non_snake_case)]
// `x = x * y` keeps the Lua statement form (STYLE §3.1 rule 3).
#![allow(clippy::assign_op_pattern)]

use pob_gamedata::mod_tools::{createMod, setSource};

use super::PerformCtx;
use super::{actor_a, actor_b, setup_flasks};
use crate::modules::{calc_tools as calcLib, common::cacheSkillUUID};
use crate::prelude::*;

/// Callee replay for the L4 test (calc doc §9.2 "Replay"; CCR-0093). Rust-only plumbing,
/// compiled only with the oracle's `probe` feature.
#[cfg(feature = "probe")]
pub mod callee_replay {
    use std::cell::RefCell;

    use crate::lua_table::Val;

    /// Called with the call site (`"pfd:1944"`, …, `"getCachedOutputValue"`): `Some(ret)` when
    /// it replayed the callee (its return values; empty for procedures), `None` to run the Rust
    /// callee.
    pub type Hook = Box<dyn FnMut(&str) -> Option<Vec<Val>>>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Install (or with `None` remove) the hook; returns the previous one.
    pub fn set(h: Option<Hook>) -> Option<Hook> {
        HOOK.with(|c| c.replace(h))
    }

    /// The hook's answer for `site` (it is taken out while it runs, so it may not re-enter).
    pub(super) fn replay(site: &str) -> Option<Vec<Val>> {
        let mut h = HOOK.with(|c| c.borrow_mut().take())?;
        let r = h(site);
        HOOK.with(|c| {
            let mut b = c.borrow_mut();
            if b.is_none() {
                *b = Some(h);
            }
        });
        r
    }
}

/// Rust-only plumbing (CCR-0093): a call of a callee another unit ports.
#[inline]
fn callee(site: &'static str, f: impl FnOnce()) {
    #[cfg(feature = "probe")]
    if callee_replay::replay(site).is_some() {
        return;
    }
    let _ = site;
    f();
}

/// Rust-only plumbing (CCR-0093): `getCachedOutputValue(env, activeSkill, ...)`.
fn cached_output(env: &EnvRef, activeSkill: &ActiveSkillRef, names: &[Name]) -> Vec<Val> {
    #[cfg(feature = "probe")]
    if let Some(ret) = callee_replay::replay("getCachedOutputValue") {
        return ret;
    }
    actor_a::getCachedOutputValue(env, activeSkill, names)
}

/// The `i`-th (0-based) return value of a multi-value call (missing → nil).
fn ret(v: &[Val], i: usize) -> Val {
    v.get(i).cloned().unwrap_or(Val::Nil)
}

/// `colorCodes[name]` (Global.lua); a nil code raises when concatenated, as in Lua.
fn color_code(name: &str) -> &'static str {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| lua_error!("attempt to concatenate field '{name}' (a nil value)"))
}

/// `actor[k]` for the computed-key actor fields (`actor.extra`).
fn actor_num(actor: &Actor, k: Name) -> f64 {
    actor.extra.num(k)
}

/// `activeSkill.activeEffect.grantedEffectLevel` (indexing nil raises).
fn grantedEffectLevel(activeSkill: &ActiveSkill) -> SkillLevelRef {
    activeSkill
        .activeEffect
        .get()
        .grantedEffectLevel
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'grantedEffectLevel' (a nil value)"))
}

/// `activeSkill.skillData[k] or activeSkill.activeEffect.grantedEffectLevel[k] or 0` (the level
/// is only indexed when the skillData value is falsy).
fn data_or_level(skillData: &Table, activeSkill: &ActiveSkill, k: Name) -> f64 {
    let v = skillData.get(k);
    if v.truthy() {
        return v.num();
    }
    let l = Val::from_value(grantedEffectLevel(activeSkill).get(k));
    if l.truthy() {
        return l.num();
    }
    0.0
}

/// `grantedEffectLevel.cost` is truthy. CCR-0093: the typed `SkillLevel.cost` cannot tell a
/// missing `cost` from `cost = { }` (minion/spectre levels only); both read as absent here.
fn has_cost(level: &pob_gamedata::SkillLevel) -> bool {
    !level.cost.is_empty()
}

/// `grantedEffectLevel.cost[resource] or 0`.
fn cost_of(level: &pob_gamedata::SkillLevel, resource: &str) -> f64 {
    level
        .cost
        .iter()
        .find(|(k, _)| k.as_str() == resource)
        .map_or(0.0, |(_, v)| *v)
}

/// `name.."Reserved"` etc. for the two pools.
struct PoolValues {
    baseFlat: f64,
    basePercent: f64,
    resourceMore: f64,
    genericMore: f64,
    more: f64,
    inc: f64,
    efficiency: f64,
    efficiencyMore: f64,
    reservedFlat: f64,
    reservedPercent: f64,
}

impl PoolValues {
    fn new() -> Self {
        PoolValues {
            baseFlat: 0.0,
            basePercent: 0.0,
            resourceMore: 0.0,
            genericMore: 0.0,
            more: 0.0,
            inc: 0.0,
            efficiency: 0.0,
            efficiencyMore: 0.0,
            reservedFlat: 0.0,
            reservedPercent: 0.0,
        }
    }
}

/// `x ~= v and ("x "..x)` (the breakdown row cells: false or a string).
fn x_cell(cond: bool, s: impl FnOnce() -> Name) -> Val {
    if cond {
        Val::from(s())
    } else {
        Val::Bool(false)
    }
}

/// @pob Modules/CalcPerform.lua:1943-2307 h=507e2504
/// PF-D: CalcPerform.lua 1943-2307 (attributes, pools, charges).
pub(crate) fn seg(ctx: &mut PerformCtx) -> Flow {
    let env = ctx.env.clone();
    let modDB = ctx.modDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();

    // L1943 Calculate attributes and life/mana pools
    // L1944
    callee("pfd:1944", || {
        actor_a::doActorAttribsConditions(&env, &env.player)
    });
    // L1945
    callee("pfd:1945", || actor_a::doActorLifeMana(&env.player));
    // L1946
    if let Some(minion) = env.minion.get() {
        // L1947
        if minion.hostile.get() {
            // L1948
            for value in env.modDB.get().Tabulate(None, None, &[n!("EnemyModifier")]) {
                // L1949
                let v = Val::from_value(&value.value);
                let r#mod = land(v.clone(), || v.get(n!("mod")));
                // L1950
                if r#mod.truthy() {
                    let Val::Mod(r#mod) = r#mod else {
                        lua_error!("EnemyModifier value.mod is a {} value", r#mod.lua_type())
                    };
                    // L1951
                    let copy: Mod = (*r#mod).clone();
                    // L1952
                    let source = r#mod.source.clone().or_else(|| value.r#mod.source.clone());
                    let sourced = match source {
                        Some(s) => setSource(&copy, &s),
                        None => {
                            let mut c = copy;
                            c.source = None;
                            c
                        }
                    };
                    minion.modDB.get().AddMod(Arc::new(sourced));
                }
            }
        }
        // L1956
        if !minion.hostile.get() {
            // L1957
            callee("pfd:1957", || {
                let mainSkill = env.player.mainSkill.get().unwrap_or_else(|| {
                    lua_error!("attempt to index field 'mainSkill' (a nil value)")
                });
                let skillCfg = mainSkill.skillCfg.get();
                actor_b::addMinionModifiers(
                    &mainSkill.skillModList.get(),
                    Some(&*skillCfg),
                    &minion,
                );
            });
            // L1958
            for name in minion.modDB.get().List(None, &[n!("Keystone")]) {
                let name = Val::from_value(&name);
                let key = match &name {
                    Val::Str(s) => Key::S(Name::from(s)),
                    Val::Num(x) => Key::from(*x),
                    Val::Bool(b) => Key::B(*b),
                    other => lua_error!("Keystone LIST value is a {} value", other.lua_type()),
                };
                let keystoneMap = env.spec.get().tree.get().keystoneMap.clone();
                // L1959
                if keystoneMap.truthy(key) {
                    // L1960
                    let node = keystoneMap
                        .obj::<Node>(key)
                        .unwrap_or_else(|| lua_error!("keystoneMap entry is not a node"));
                    minion.modDB.get().AddList(&node.modList.get().ipairs());
                }
            }
        }
        // L1964
        callee("pfd:1964", || {
            actor_a::doActorAttribsConditions(&env, &minion)
        });
    }

    // L1967 Calculate skill life and mana reservations
    // L1968
    env.player.extra.set(n!("reserved_LifeBase"), 0.0);
    // L1969
    env.player.extra.set(
        n!("reserved_LifePercent"),
        modDB.Sum(ModType::BASE, None, &[n!("ExtraLifeReserved")]),
    );
    // L1970
    env.player.extra.set(n!("reserved_ManaBase"), 0.0);
    // L1971
    env.player.extra.set(n!("reserved_ManaPercent"), 0.0);
    // L1972
    env.player.extra.set(
        n!("uncancellable_LifeReservation"),
        modDB.Sum(ModType::BASE, None, &[n!("ExtraLifeReserved")]),
    );
    // L1973
    env.player.extra.set(
        n!("uncancellable_ManaReservation"),
        modDB.Sum(ModType::BASE, None, &[n!("ExtraManaReserved")]),
    );
    // L1974
    if let Some(breakdown) = &breakdown {
        // L1975
        breakdown.set(n!("LifeReserved"), tbl! { reservations = Table::new() });
        // L1976
        breakdown.set(n!("ManaReserved"), tbl! { reservations = Table::new() });
    }
    // L1978
    for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
        let skillTypes = activeSkill.skillTypes.get().with(|t| *t);
        let skillData = activeSkill.skillData.get();
        let skillFlags = activeSkill.skillFlags.get();
        // L1979
        if (skillTypes.has(SkillType::HasReservation) || skillData.truthy(n!("triggeredByAutoexertion")))
            && !skillTypes.has(SkillType::ReservationBecomesCost)
            && !skillFlags.truthy(n!("disable"))
        {
            // L1980
            let skillModList = activeSkill.skillModList.get();
            // L1981
            let skillCfg = activeSkill.skillCfg.get();
            let cfg = Some(&*skillCfg);
            // L1982
            let mult = floor(skillModList.More(cfg, &[n!("SupportManaMultiplier")]), Some(4));
            // L1983
            let mut pool = [(n!("Mana"), PoolValues::new()), (n!("Life"), PoolValues::new())];
            // L1984
            pool[0].1.baseFlat = data_or_level(&skillData, &activeSkill, n!("manaReservationFlat"));
            // L1985
            if skillModList.Flag(cfg, &[n!("ManaCostGainAsReservation")]) && has_cost(&grantedEffectLevel(&activeSkill)) {
                // L1986
                pool[0].1.baseFlat = skillModList.Sum(ModType::BASE, cfg, &[n!("ManaCostBase")])
                    + cost_of(&grantedEffectLevel(&activeSkill), "Mana");
            }
            // L1988
            pool[0].1.basePercent = data_or_level(&skillData, &activeSkill, n!("manaReservationPercent"));
            // L1989
            pool[1].1.baseFlat = data_or_level(&skillData, &activeSkill, n!("lifeReservationFlat"));
            // L1990
            if skillModList.Flag(cfg, &[n!("LifeCostGainAsReservation")]) && has_cost(&grantedEffectLevel(&activeSkill)) {
                // L1991
                pool[1].1.baseFlat = skillModList.Sum(ModType::BASE, cfg, &[n!("LifeCostBase")])
                    + cost_of(&grantedEffectLevel(&activeSkill), "Life");
            }
            // L1993
            pool[1].1.basePercent = data_or_level(&skillData, &activeSkill, n!("lifeReservationPercent"));
            // L1994
            if skillModList.Flag(cfg, &[n!("BloodMagicReserved")])
                && !skillData.truthy(n!("ManaReservationPercentForced"))
            {
                // L1995
                pool[1].1.baseFlat = pool[1].1.baseFlat + pool[0].1.baseFlat;
                // L1996
                pool[0].1.baseFlat = 0.0;
                // L1997
                skillData.set(n!("LifeReservationFlatForced"), skillData.get(n!("ManaReservationFlatForced")));
                // L1998
                skillData.set(n!("ManaReservationFlatForced"), Val::Nil);
                // L1999
                pool[1].1.basePercent = pool[1].1.basePercent + pool[0].1.basePercent;
                // L2000
                pool[0].1.basePercent = 0.0;
                // L2001
                skillData.set(
                    n!("LifeReservationPercentForced"),
                    skillData.get(n!("ManaReservationPercentForced")),
                );
                // L2002
                skillData.set(n!("ManaReservationPercentForced"), Val::Nil);
            }
            // L2004 LUA-ORDER: `pairs(pool)` over { Mana, Life }; each pass reads and writes only
            // its own pool's keys, so the order is not observable. The port visits the
            // constructor order.
            for (name, values) in pool.iter_mut() {
                let name = *name;
                // L2005
                values.resourceMore = skillModList.More(cfg, &[cat!(name, "Reserved")]);
                // L2006
                values.genericMore = skillModList.More(cfg, &[n!("Reserved")]);
                // L2007
                values.more = values.resourceMore * values.genericMore;
                // L2008
                values.inc = skillModList.Sum(ModType::INC, cfg, &[cat!(name, "Reserved"), n!("Reserved")]);
                // L2009
                values.efficiency = m_max(
                    skillModList.Sum(
                        ModType::INC,
                        cfg,
                        &[cat!(name, "ReservationEfficiency"), n!("ReservationEfficiency")],
                    ),
                    -100.0,
                );
                // L2010
                values.efficiencyMore = skillModList.More(
                    cfg,
                    &[cat!(name, "ReservationEfficiency"), n!("ReservationEfficiency")],
                );
                // L2011 used for Arcane Cloak calculations in ModStore.GetStat
                // L2012
                env.player.extra.set(cat!(name, "Efficiency"), values.efficiency);
                // L2013
                let flatForced = skillData.get(cat!(name, "ReservationFlatForced"));
                if flatForced.truthy() {
                    // L2014
                    values.reservedFlat = flatForced.num();
                } else {
                    // L2016
                    let baseFlatVal = m_modf(values.baseFlat * mult).0;
                    // L2017
                    values.reservedFlat = 0.0;
                    // L2018
                    if baseFlatVal != 0.0 {
                        // L2019 Reservation modifiers use separate integer stages before efficiency.
                        // L2020
                        let mut modifiedFlat = baseFlatVal + m_modf(baseFlatVal * values.inc / 100.0).0;
                        // L2021
                        modifiedFlat = modifiedFlat + m_modf(modifiedFlat * (values.resourceMore - 1.0)).0;
                        // L2022
                        modifiedFlat = modifiedFlat + m_modf(modifiedFlat * (values.genericMore - 1.0)).0;
                        // L2023
                        values.reservedFlat = m_max(
                            round(
                                modifiedFlat / (1.0 + values.efficiency / 100.0) / values.efficiencyMore,
                                Some(0),
                            ),
                            0.0,
                        );
                    }
                }
                // L2026
                let percentForced = skillData.get(cat!(name, "ReservationPercentForced"));
                if percentForced.truthy() {
                    // L2027
                    values.reservedPercent = percentForced.num();
                } else {
                    // L2029
                    let basePercentVal = m_modf(values.basePercent * 100.0 * mult).0;
                    // L2030
                    values.reservedPercent = 0.0;
                    // L2031
                    if basePercentVal != 0.0 {
                        // L2032
                        let mut modifiedPercent =
                            basePercentVal + m_modf(basePercentVal * values.inc / 100.0).0;
                        // L2033
                        modifiedPercent =
                            modifiedPercent + m_modf(modifiedPercent * (values.resourceMore - 1.0)).0;
                        // L2034
                        modifiedPercent =
                            modifiedPercent + m_modf(modifiedPercent * (values.genericMore - 1.0)).0;
                        // L2035
                        values.reservedPercent = m_max(
                            round(
                                modifiedPercent / (1.0 + values.efficiency / 100.0) / values.efficiencyMore,
                                Some(0),
                            ) / 100.0,
                            0.0,
                        );
                    }
                }
                // L2038
                if let Some(activeMineCount) = activeSkill.activeMineCount.get() {
                    // L2039
                    values.reservedFlat = values.reservedFlat * activeMineCount;
                    // L2040
                    values.reservedPercent = values.reservedPercent * activeMineCount;
                }
                // L2042 Blood Sacrament increases reservation per stage channelled
                // L2043
                if activeSkill.skillCfg.get().skillName.get().is_some_and(|s| s.as_str() == "Blood Sacrament") {
                    if let Some(activeStageCount) = activeSkill.activeStageCount.get() {
                        // L2044
                        values.reservedFlat = values.reservedFlat * (activeStageCount + 1.0);
                        // L2045
                        values.reservedPercent = values.reservedPercent * (activeStageCount + 1.0);
                    }
                }
                // L2047
                if values.reservedFlat != 0.0 {
                    // L2048
                    skillData.set(cat!(name, "ReservedBase"), values.reservedFlat);
                    // L2049
                    let k = cat!("reserved_", name, "Base");
                    let v = actor_num(&env.player, k) + values.reservedFlat;
                    env.player.extra.set(k, v);
                    // L2050
                    if let Some(breakdown) = &breakdown {
                        // L2051
                        let row = tbl! {
                            skillName = activeSkill.activeEffect.get().grantedEffect.get().name,
                            base = values.baseFlat,
                            mult = x_cell(mult != 1.0, || cat!("x ", mult)),
                            more = x_cell(values.more != 1.0, || cat!("x ", values.more)),
                            inc = x_cell(values.inc != 0.0, || cat!("x ", 1.0 + values.inc / 100.0)),
                            efficiency = x_cell(values.efficiency != 0.0, || {
                                cat!("x ", round(100.0 / (100.0 + values.efficiency), Some(4)))
                            }),
                            efficiencyMore = x_cell(values.efficiencyMore != 1.0, || {
                                cat!("x ", values.efficiencyMore)
                            }),
                            total = values.reservedFlat,
                        };
                        reservations(breakdown, name).push(row);
                    }
                }
                // L2063
                if values.reservedPercent != 0.0 {
                    // L2064
                    skillData.set(cat!(name, "ReservedPercent"), values.reservedPercent);
                    // L2065
                    let reservedBase = values.reservedFlat
                        + m_ceil(output.num(name) * values.reservedPercent / 100.0);
                    skillData.set(cat!(name, "ReservedBase"), reservedBase);
                    // L2066
                    let k = cat!("reserved_", name, "Percent");
                    let v = actor_num(&env.player, k) + values.reservedPercent;
                    env.player.extra.set(k, v);
                    // L2067
                    if let Some(breakdown) = &breakdown {
                        // L2068
                        let row = tbl! {
                            skillName = activeSkill.activeEffect.get().grantedEffect.get().name,
                            base = cat!(values.basePercent, "%"),
                            mult = x_cell(mult != 1.0, || cat!("x ", mult)),
                            more = x_cell(values.more != 1.0, || cat!("x ", values.more)),
                            inc = x_cell(values.inc != 0.0, || cat!("x ", 1.0 + values.inc / 100.0)),
                            efficiency = x_cell(values.efficiency != 0.0, || {
                                cat!("x ", round(100.0 / (100.0 + values.efficiency), Some(4)))
                            }),
                            efficiencyMore = x_cell(values.efficiencyMore != 1.0, || {
                                cat!("x ", values.efficiencyMore)
                            }),
                            total = cat!(values.reservedPercent, "%"),
                        };
                        reservations(breakdown, name).push(row);
                    }
                }
                // L2080
                if skillModList.Flag(cfg, &[n!("HasUncancellableReservation")]) {
                    // L2081
                    let k = cat!("uncancellable_", name, "Reservation");
                    let v = actor_num(&env.player, k) + values.reservedPercent;
                    env.player.extra.set(k, v);
                }
            }
            // L2084
            if skillData.truthy(n!("ManaReservationPercentForced"))
                && skillData.get(n!("ManaReservedBase")).raw_eq(&Val::Num(0.0))
            {
                // L2085
                skillFlags.set(n!("disable"), true);
                // L2086
                activeSkill.disableReason.set(Some(LStr::from("This skill requires reserving Mana")));
            }
        }
    });

    // L2091 Set the life/mana reservations (hold off on GrantReserved"..pool.."AsAura)
    // L2092
    callee("pfd:2092", || {
        actor_a::doActorLifeManaReservation(
            &env.player,
            !modDB.Flag(None, &[n!("ManaIncreasedByOvercappedLightningRes")]),
        );
    });

    // L2094
    if env.mode_combat.get() {
        // L2095
        callee("pfd:2095", || {
            let tinctures = env.tinctures.get().with(Clone::clone);
            setup_flasks::mergeTinctures(ctx, &tinctures);
        });
    }

    // L2098 Process attribute requirements
    // L2099
    {
        // L2100
        let reqMult = calcLib::r#mod(&modDB, None, &[n!("GlobalAttributeRequirements")]);
        // L2101
        let omniRequirements = if modDB.Flag(None, &[n!("OmniscienceRequirements")]) {
            Some(calcLib::r#mod(
                &modDB,
                None,
                &[n!("OmniAttributeRequirements")],
            ))
        } else {
            None
        };
        // L2102
        let ignoreAttrReq = modDB.Flag(None, &[n!("IgnoreAttributeRequirements")]);
        // L2103
        let attrTable: &[Name] = if omniRequirements.is_some() {
            nm!["Omni", "Str", "Dex", "Int"]
        } else {
            nm!["Str", "Dex", "Int"]
        };
        // L2104
        for_ipairs!((_, attr) in attrTable => {
            // L2105
            let breakdownAttr = if omniRequirements.is_some() { n!("Omni") } else { attr };
            // L2106
            if let Some(breakdown) = &breakdown {
                // L2107
                breakdown.set(
                    cat!("Req", attr),
                    tbl! {
                        source = "",
                        rowList = Table::new(),
                        colList = tbl![
                            tbl! { label = attr, key = "req" },
                            tbl! { label = "Source", key = "source" },
                            tbl! { label = "Source Name", key = "sourceName" },
                        ],
                    },
                );
            }
            // L2117
            let mut out_val = 0.0;
            let mut out_source: Option<Table> = None;
            // L2118
            for_ipairs!((_, reqSource) in env.requirementsTable.get() => {
                let reqSource = reqSource
                    .table()
                    .unwrap_or_else(|| lua_error!("attempt to index local 'reqSource' (a {} value)", reqSource.lua_type()));
                // L2119
                let reqAttr = reqSource.get(attr);
                if reqAttr.truthy() && reqAttr.num() > 0.0 {
                    // L2120
                    let mut req = m_floor(reqAttr.num() * reqMult);
                    // L2121
                    if let Some(omniRequirements) = omniRequirements {
                        // L2122
                        let omniReqMult = 1.0 / (omniRequirements - 1.0);
                        // L2123
                        let attributereq = m_floor(reqAttr.num() * reqMult);
                        // L2124
                        req = m_floor(attributereq * omniReqMult);
                    }
                    // L2126
                    if req > out_val {
                        // L2127
                        out_val = req;
                        // L2128
                        out_source = Some(reqSource.clone());
                    }
                    // L2130
                    if let Some(breakdown) = &breakdown {
                        // L2131
                        let row = tbl! {
                            req = if req > output.num(breakdownAttr) {
                                Val::from(cat!(color_code("NEGATIVE"), req))
                            } else {
                                Val::Num(req)
                            },
                            reqNum = req,
                            source = reqSource.get(n!("source")),
                        };
                        let source = reqSource.get(n!("source"));
                        // L2136
                        if source.str().is_some_and(|s| &*s == "Item") {
                            // L2137
                            let item = reqSource.obj::<Item>(n!("sourceItem")).unwrap_or_else(|| {
                                lua_error!("attempt to index local 'item' (a nil value)")
                            });
                            // L2138
                            row.set(
                                n!("sourceName"),
                                cat!(color_code(item.rarity.get().as_str()), &*item.name.get()),
                            );
                            // L2139
                            row.set(
                                n!("sourceNameTooltip"),
                                Val::Obj(Rc::new(SourceNameTooltip {
                                    build: Rc::downgrade(&env.build),
                                    item: item.clone(),
                                    sourceSlot: reqSource.get(n!("sourceSlot")),
                                })),
                            );
                        } else if source.str().is_some_and(|s| &*s == "Gem") {
                            // L2143
                            let gem = reqSource.get(n!("sourceGem"));
                            let field = |k: Name| -> Val {
                                match &gem {
                                    Val::Obj(_) => {
                                        let g = gem.obj::<GemInstance>().unwrap_or_else(|| {
                                            lua_error!("sourceGem is not a gem instance")
                                        });
                                        match k.as_str() {
                                            "nameSpec" => Val::Str(g.nameSpec.get()),
                                            "level" => Val::Num(g.level.get()),
                                            "quality" => Val::Num(g.quality.get()),
                                            _ => g.extra.get(k),
                                        }
                                    }
                                    Val::Table(t) => t.get(k),
                                    other => lua_error!(
                                        "attempt to index field 'sourceGem' (a {} value)",
                                        other.lua_type()
                                    ),
                                }
                            };
                            // `%s` is `tostring` (LuaJIT lj_strfmt); `%d` needs a number
                            let s_arg = |v: Val| -> String {
                                match v {
                                    Val::Str(s) => s.to_string(),
                                    Val::Num(x) => tostring_num(x),
                                    Val::Nil => "nil".to_owned(),
                                    Val::Bool(b) => b.to_string(),
                                    other => lua_error!(
                                        "bad argument to 'format' (string expected, got {})",
                                        other.lua_type()
                                    ),
                                }
                            };
                            let color = s_arg(field(n!("color")));
                            let nameSpec = s_arg(field(n!("nameSpec")));
                            let level = field(n!("level")).num();
                            let quality = field(n!("quality")).num();
                            row.set(
                                n!("sourceName"),
                                lfmt!("%s%s ^7%d/%d", color.as_str(), nameSpec.as_str(), level, quality)
                                    .as_str(),
                            );
                        }
                        // L2145
                        breakdown
                            .table(cat!("Req", breakdownAttr))
                            .unwrap_or_else(|| lua_error!("attempt to index field 'Req{breakdownAttr}' (a nil value)"))
                            .table(n!("rowList"))
                            .unwrap_or_else(|| lua_error!("attempt to index field 'rowList' (a nil value)"))
                            .push(row);
                    }
                }
            });
            // L2149
            if ignoreAttrReq {
                // L2150
                out_val = 0.0;
            }
            // L2152
            output.set(cat!("Req", attr, "String"), 0.0);
            // L2153
            if out_val > output.num0(cat!("Req", breakdownAttr)) {
                // L2154
                output.set(cat!("Req", breakdownAttr, "String"), out_val);
                // L2155
                output.set(cat!("Req", breakdownAttr), out_val);
                // L2156
                output.set(cat!("Req", breakdownAttr, "Item"), Val::from(out_source.clone()));
                // L2157
                if breakdown.is_some() {
                    // L2158
                    let s = if out_val > output.num0(breakdownAttr) {
                        Val::from(cat!(color_code("NEGATIVE"), out_val))
                    } else {
                        Val::Num(out_val)
                    };
                    output.set(cat!("Req", breakdownAttr, "String"), s);
                }
            }
        });
    }

    // L2164 Calculate number of active heralds and auras affecting self
    // L2165
    if env.mode_buffs.get() {
        // L2166
        let heraldList = Table::new();
        // L2167
        let auraList = Table::new();
        // L2168
        for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
            let skillTypes = activeSkill.skillTypes.get().with(|t| *t);
            let skillName = activeSkill.skillCfg.get().skillName.get();
            let skillKey = |what: &str| -> Key {
                match skillName {
                    Some(s) => Key::S(s),
                    None => lua_error!("{what}: table index is nil"),
                }
            };
            let listed = |t: &Table| skillName.is_some_and(|s| t.truthy(s));
            // L2169
            if skillTypes.has(SkillType::Herald) && !listed(&heraldList) {
                // L2170
                heraldList.set(skillKey("heraldList"), true);
                // L2171
                let v = modDB.multipliers.num0(n!("Herald")) + 1.0;
                modDB.multipliers.set(n!("Herald"), v);
                // L2172
                modDB.conditions.set(n!("AffectedByHerald"), true);
            } else if skillTypes.has(SkillType::Aura)
                && !skillTypes.has(SkillType::AuraAffectsEnemies)
                && !activeSkill.skillData.get().truthy(n!("auraCannotAffectSelf"))
                && !listed(&auraList)
            {
                // L2174
                auraList.set(skillKey("auraList"), true);
                // L2175
                let v = modDB.multipliers.num0(n!("AuraAffectingSelf")) + 1.0;
                modDB.multipliers.set(n!("AuraAffectingSelf"), v);
            }
        });
    }

    // L2180
    if modDB.Flag(None, &[n!("ManaAppliesToShockEffect")]) {
        // L2181 Maximum Mana conversion from Lightning Mastery
        // L2182
        let multiplier = modDB
            .Max(None, &[n!("ImprovedManaAppliesToShockEffect")])
            .unwrap_or(100.0)
            / 100.0;
        // L2183
        for value in modDB.Tabulate(Some(ModType::INC), None, &[n!("Mana")]) {
            // L2184
            let r#mod = &value.r#mod;
            // L2185
            let modifiers = calcLib::getConvertedModTags(r#mod, multiplier, false);
            // L2186
            let modValue = r#mod.value.num().unwrap_or_else(|| {
                lua_error!(
                    "attempt to perform arithmetic on field 'value' (a {} value)",
                    r#mod.value.lua_type()
                )
            });
            modDB.AddMod(Arc::new(createMod(
                n!("EnemyShockEffect"),
                ModType::INC,
                m_floor(modValue * multiplier),
                r#mod.source.as_deref(),
                r#mod.flags,
                r#mod.keywordFlags,
                &modifiers,
            )));
        }
    }

    // L2190 Calculate charges early to enable usage of stats that depend on charge count
    // L2191
    callee("pfd:2191", || actor_b::doActorCharges(&env, &env.player));

    // L2193 Combine buffs/debuffs
    // L2194
    ctx.buffs = Table::new();
    let buffs = ctx.buffs.clone();
    // L2195
    env.buffs.set(buffs.clone());
    // L2196
    ctx.guards = Table::new();
    // L2197
    ctx.minionBuffs = Table::new();
    let minionBuffs = ctx.minionBuffs.clone();
    // L2198
    env.minionBuffs.set(minionBuffs.clone());
    // L2199
    ctx.debuffs = Table::new();
    // L2200
    env.debuffs.set(ctx.debuffs.clone());
    // L2201
    ctx.curses = Table::new();
    // L2202
    ctx.minionCurses = tbl! { limit = 1.0 };
    let minionCurses = ctx.minionCurses.clone();
    // L2205
    ctx.linkSkills = Table::new();
    // L2206
    ctx.allyBuffs = env
        .partyMembers
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'partyMembers' (a nil value)"))
        .extra
        .table(n!("Aura"));
    // L2207
    ctx.buffExports = tbl! {
        Aura = Table::new(),
        Curse = Table::new(),
        Warcry = Table::new(),
        Link = Table::new(),
        EnemyMods = Table::new(),
        EnemyConditions = Table::new(),
        PlayerMods = Table::new(),
    };
    // L2208
    let mut hasActiveSpectreSkill = false;
    // L2209
    let activeSpectreList = Table::new();
    // L2210
    for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
        // L2211
        if !activeSkill.skillFlags.get().truthy(n!("disable")) {
            // L2212
            let skillId = activeSkill.activeEffect.get().grantedEffect.get().id;
            // L2213
            if pob_lua::lmatch(skillId.as_str(), "^RaiseSpectre", None).is_some() {
                // L2214
                hasActiveSpectreSkill = true;
                // L2215
                if let Some(minion) = activeSkill.minion.get() {
                    if let Some(r#type) = minion.r#type.get() {
                        // L2216
                        activeSpectreList.push(Val::from(r#type));
                    }
                }
            }
        }
    });
    // L2221
    if hasActiveSpectreSkill {
        // `env.spec.build` is the build that owns the spec (PassiveSpec.lua:16), i.e. `env.build`
        // (CalcSetup.lua:501: `env.spec = override.spec or build.spec`); a spec without a back-pointer
        // (the model's default spec) reads it from the env.
        let build = env
            .spec
            .get()
            .build
            .upgrade()
            .unwrap_or_else(|| env.build.clone());
        let spectreList = build.spectreList.get();
        // L2222
        let n = spectreList.len();
        for spectreId in 1..=n {
            // L2223
            let mut isThisSpectreActive = false;
            let spectreName = spectreList.get(spectreId);
            // L2224
            for_ipairs!((_, spectreType) in activeSpectreList => {
                // L2225
                if spectreName.is_some_and(|s| Val::from(s).raw_eq(&spectreType)) {
                    // L2226
                    isThisSpectreActive = true;
                    // L2227
                    break;
                }
            });
            // L2230
            if isThisSpectreActive {
                // L2231
                let spectreData = spectreName
                    .and_then(|s| env.data.minions.get(&s).cloned())
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index local 'spectreData' (a nil value)")
                    });
                // L2232
                if !modDB.conditions.truthy(n!("HaveBeastSpectre")) {
                    // L2233 Change to grab from monster family using CorpseTypeTags.dat if other monster families are needed in the future
                    // L2234 (`ipairs(nil)` raises: CCR-0015 §3)
                    let monsterTags = spectreData.monsterTags.as_ref().unwrap_or_else(|| {
                        lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
                    });
                    for_ipairs!((_, tagName) in monsterTags => {
                        // L2235
                        if tagName.as_str() == "beast" {
                            // L2236
                            modDB.conditions.set(n!("HaveBeastSpectre"), true);
                            // L2237
                            break;
                        }
                    });
                }
                // L2241
                for modId in 1..=spectreData.modList.len() {
                    // L2242
                    let modData = spectreData.modList[modId - 1].clone();
                    // L2243
                    if modData.name.as_str() == "EnemyCurseLimit" {
                        // L2244
                        let limit = modData.value.num().unwrap_or_else(|| {
                            lua_error!(
                                "attempt to perform arithmetic on field 'value' (a {} value)",
                                modData.value.lua_type()
                            )
                        }) + 1.0;
                        minionCurses.set(n!("limit"), limit);
                        // L2245
                        break;
                    } else if modData.name.as_str() == "AllyModifier"
                        && modData.r#type == ModType::LIST
                    {
                        // L2247
                        let buffsSpectre = spectre_list(&buffs);
                        // L2248
                        let minionBuffsSpectre = spectre_list(&minionBuffs);
                        // L2249
                        for modValue in value_pairs(&modData.value) {
                            // L2250
                            let copyModValue = spectre_copy(&modValue, &spectreData.name);
                            // L2252
                            minionBuffsSpectre.AddMod(copyModValue.clone());
                            // L2253
                            buffsSpectre.AddMod(copyModValue);
                        }
                    } else if modData.name.as_str() == "MinionModifier"
                        && modData.r#type == ModType::LIST
                    {
                        // L2256
                        let minionBuffsSpectre = spectre_list(&minionBuffs);
                        // L2257
                        for modValue in value_pairs(&modData.value) {
                            // L2258
                            let copyModValue = spectre_copy(&modValue, &spectreData.name);
                            // L2260
                            minionBuffsSpectre.AddMod(copyModValue);
                        }
                    } else if modData.name.as_str() == "PlayerModifier"
                        && modData.r#type == ModType::LIST
                    {
                        // L2263
                        let buffsSpectre = spectre_list(&buffs);
                        // L2264
                        for modValue in value_pairs(&modData.value) {
                            // L2265
                            let copyModValue = spectre_copy(&modValue, &spectreData.name);
                            // L2267
                            buffsSpectre.AddMod(copyModValue);
                        }
                    }
                }
            }
        }
    }

    // L2275 To support maximum sustainable stages for the following skills we need to get the data from already
    // L2276 computed cached versions to satisfy the order of operations.
    // L2277 See: https://github.com/PathOfBuildingCommunity/PathOfBuilding/pull/5164
    // L2278
    for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
        // L2279
        let limited = || {
            env.limitedSkills
                .get()
                .is_some_and(|l| l.truthy(cacheSkillUUID(&activeSkill, &env)))
        };
        if !activeSkill.skillFlags.get().truthy(n!("disable")) && !limited() {
            let grantedEffect = activeSkill.activeEffect.get().grantedEffect.get();
            let geName = grantedEffect.name;
            let skillPart2 = activeSkill.skillPart.get() == Some(2.0);
            // L2280
            if (geName.as_str() == "Blight"
                || geName.as_str() == "Blight of Contagion"
                || geName.as_str() == "Blight of Atrophy")
                && skillPart2
            {
                // L2281
                let r = cached_output(&env, &activeSkill, nm!["Speed", "Duration"]);
                let (rate, duration) = (ret(&r, 0), ret(&r, 1));
                // L2282
                let mainSkill = env.player.mainSkill.get().unwrap_or_else(|| {
                    lua_error!("attempt to index field 'mainSkill' (a nil value)")
                });
                let mainCfg = mainSkill.skillCfg.get();
                let baseMaxStages = activeSkill.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&*mainCfg),
                    &[n!("BlightBaseMaxStages")],
                );
                // L2283
                let maximum = m_min(m_floor(rate.num() * duration.num()) - 1.0, baseMaxStages - 1.0);
                let stripped = pob_lua::gsub(geName.as_str(), "%s+", "", None).0;
                // L2284
                activeSkill.skillModList.get().NewMod(
                    cat!("Multiplier:", stripped.as_str(), "MaxStages"),
                    ModType::BASE,
                    maximum,
                    "Base",
                    0,
                    0,
                    &[],
                );
                // L2285
                activeSkill.skillModList.get().NewMod(
                    cat!("Multiplier:", stripped.as_str(), "StageAfterFirst"),
                    ModType::BASE,
                    maximum,
                    "Base",
                    0,
                    0,
                    &[],
                );
            }
            // L2287
            if geName.as_str() == "Penance Brand of Dissipation" && skillPart2 {
                // L2288 HitSpeed is the brand activation frequency
                let r = cached_output(&env, &activeSkill, nm!["HitSpeed", "Duration"]);
                let (activation_frequency, duration) = (ret(&r, 0), ret(&r, 1));
                // L2289
                let ticks = m_max(
                    m_min(
                        m_floor(lor(activation_frequency, || Val::Num(0.0)).num() * duration.num()) - 1.0,
                        19.0,
                    ),
                    0.0,
                );
                // L2290
                activeSkill.skillModList.get().NewMod(
                    n!("Multiplier:PenanceBrandofDissipationMaxStages"),
                    ModType::BASE,
                    ticks,
                    "Base",
                    0,
                    0,
                    &[],
                );
                // L2291
                activeSkill.skillModList.get().NewMod(
                    n!("Multiplier:PenanceBrandofDissipationStageAfterFirst"),
                    ModType::BASE,
                    ticks,
                    "Base",
                    0,
                    0,
                    &[],
                );
            }
            // L2293
            if (geName.as_str() == "Scorching Ray" || geName.as_str() == "Scorching Ray of Immolation")
                && skillPart2
            {
                // L2294
                let maximum = 7.0;
                let stripped = pob_lua::gsub(geName.as_str(), "%s+", "", None).0;
                // L2295
                activeSkill.skillModList.get().NewMod(
                    cat!("Multiplier:", stripped.as_str(), "MaxStages"),
                    ModType::BASE,
                    maximum,
                    "Base",
                    0,
                    0,
                    &[],
                );
                // L2296
                activeSkill.skillModList.get().NewMod(
                    cat!("Multiplier:", stripped.as_str(), "StageAfterFirst"),
                    ModType::BASE,
                    maximum,
                    "Base",
                    0,
                    0,
                    &[],
                );
            }
            // L2298
            if geName.as_str() == "Earthquake of Amplification" && skillPart2 {
                // L2299
                let duration = ret(&cached_output(&env, &activeSkill, nm!["Duration"]), 0);
                // L2300
                let durationMulti = m_floor(duration.num() * 10.0);
                // L2301
                activeSkill.skillModList.get().NewMod(
                    n!("Multiplier:100msEarthquakeDuration"),
                    ModType::BASE,
                    durationMulti,
                    "Skill:EarthquakeAltX",
                    0,
                    0,
                    &[],
                );
            }
        }
    });

    // L2306
    ctx.appliedCombustion = false;
    // L2307
    ctx.warcryList = Table::new();
    Flow::Normal
}

/// `breakdown[name.."Reserved"].reservations`.
fn reservations(breakdown: &Breakdown, name: Name) -> Table {
    breakdown
        .table(cat!(name, "Reserved"))
        .and_then(|t| t.table(n!("reservations")))
        .unwrap_or_else(|| lua_error!("attempt to index field '{name}Reserved' (a nil value)"))
}

/// `t["Spectre"] = t["Spectre"] or new("ModList"):ModList()`; returns the list.
fn spectre_list(t: &Table) -> ModList {
    let v = t.get(n!("Spectre"));
    if v.truthy() {
        return v.obj::<ModStore>().unwrap_or_else(|| {
            lua_error!("buffs.Spectre is a {} value, not a ModList", v.lua_type())
        });
    }
    let list = ModStore::ModList(None);
    t.set(n!("Spectre"), Val::Obj(list.clone()));
    list
}

/// `pairs(modData.value)` values (a `{ mod = … }` payload: array part, then keys in order).
fn value_pairs(v: &Value) -> Vec<Value> {
    match v {
        Value::Table(t) => t
            .array
            .iter()
            .cloned()
            .chain(t.fields.iter().map(|(_, x)| x.clone()))
            .filter(|x| !x.is_nil())
            .collect(),
        Value::Mod(_) => lua_error!("bad argument #1 to 'pairs' (table expected, got mod payload)"),
        other => lua_error!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `local copyModValue = copyTable(modValue); copyModValue.source = "Spectre:"..spectreData.name`.
fn spectre_copy(modValue: &Value, spectreName: &Name) -> ModRef {
    let Value::Mod(m) = modValue else {
        lua_error!(
            "attempt to index local 'copyModValue' (a {} value)",
            modValue.lua_type()
        )
    };
    // L2250 (copyTable: a fresh copy; §3.6)
    let mut copyModValue: Mod = (**m).clone();
    // L2251
    copyModValue.source = Some(AStr::from(cat!("Spectre:", spectreName).as_str()));
    Arc::new(copyModValue)
}

/// `function(tooltip) env.build.itemsTab:AddItemTooltip(tooltip, item, reqSource.sourceSlot) end`
/// (CalcPerform.lua:2139-2141): a UI closure stored in a requirements breakdown row, kept as the
/// values it captures; the UI calls `itemsTab:AddItemTooltip` with them (CCR-0093).
pub struct SourceNameTooltip {
    /// `env.build` (weak: the build owns the env that owns this breakdown, §3.3 rule 4).
    pub build: Weak<Build>,
    pub item: ItemRef,
    pub sourceSlot: Val,
}

impl SourceNameTooltip {
    /// The Lua closure this value stands for (the dump's `$fn` marker).
    pub const SRC: &'static str = "Modules/CalcPerform.lua:2139-2141";
}
