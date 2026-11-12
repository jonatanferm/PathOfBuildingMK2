//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence segment OF-C (CalcOffence.lua 1692-2173): skill costs, Sacrificial Zeal, the
//! `preDamageFunc` hook, corpse/monster explosions, `canDeal`, the damage conversion tables
//! (`buildConversionTable`), the damage passes (`passList`) and the local function
//! `combineStat` (2066-2170), which crosses into OF-D/OF-F/OF-I (§7.2).
//!
//! Owner: OF-C.
//!
//! Notes:
//! * `tempTable1..3` (CalcOffence.lua:27-29) are scratch tables that never escape
//!   `buildConversionTable` (module_state.rs), so they are fresh locals here.
//! * `isElemental` / `dmgTypeList` (CalcOffence.lua:31/34) are OF-A's module-level
//!   locals (their provenance is OF-A's 7-67 marker); this file reads them through the
//!   private copies below.
//! * `combineStat`'s vararg is a stat name, not a table (CCR-0055).
#![allow(non_snake_case)]
// Lua locals kept verbatim even where unread (`mainPortion` 2132, `globalTotal` 1969)
#![allow(unused_variables, unused_assignments)]
// literal Lua statement shapes (`x = x + y`, `local x; x = …`)
#![allow(
    clippy::assign_op_pattern,
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::collapsible_else_if,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::no_effect_underscore_binding
)]

use super::helpers_setup::runSkillFunc;
use super::OffenceCtx;
use crate::data_fns::SkillHook;
use crate::lua_table::TableField;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_lua::format::{lua_format, FmtArg};

/// `isElemental` (CalcOffence.lua:31): `{ Fire = true, Cold = true, Lightning = true }`.
fn isElemental(damageType: Name) -> bool {
    damageType == n!("Fire") || damageType == n!("Cold") || damageType == n!("Lightning")
}

/// `dmgTypeList` (CalcOffence.lua:34), ordered according to the conversion sequence.
fn dmgTypeList() -> [Name; 5] {
    [
        n!("Physical"),
        n!("Lightning"),
        n!("Cold"),
        n!("Fire"),
        n!("Chaos"),
    ]
}

/// A Lua value as a `string.format` argument (`%d` of nil raises inside `lua_format`).
fn fmt_arg(v: &Val) -> FmtArg<'_> {
    match v {
        Val::Num(n) => FmtArg::Num(*n),
        Val::Str(s) => FmtArg::Str(s),
        Val::Bool(b) => FmtArg::Bool(*b),
        Val::Nil => FmtArg::Nil,
        other => lua_error!(
            "bad argument to 'format' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// `t_insert(t[k], s)` where `t[k]` must be a table.
fn insert_at(t: &Table, k: Name, s: impl Into<Val>) {
    match t.get(k) {
        Val::Table(x) => x.push(s),
        other => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `t[k]` must be a table (index of a nil field raises).
fn tab(t: &Table, k: Name) -> Table {
    match t.get(k) {
        Val::Table(x) => x,
        other => lua_error!(
            "attempt to index field '{}' (a {} value)",
            k.as_str(),
            other.lua_type()
        ),
    }
}

/// `val.type`-style string field compared with a literal.
fn str_is(t: &Table, k: Name, s: &str) -> bool {
    t.get(k).str().as_deref() == Some(s)
}

/// `s:gsub(pat, repl)` (first result) for the plain patterns of this segment
/// ("Life", "ES", "Minute", "DPS": no magic characters, so a plain replace-all).
fn gsub_plain(s: &str, pat: &str, repl: &str) -> Name {
    Name::from(s.replace(pat, repl).as_str())
}

/// `data.monsterLifeTable[level]` (nil for a non-integral or out-of-range level).
fn monsterLifeTable(level: f64) -> Val {
    let t = &data().monsterLifeTable;
    if level.fract() == 0.0 && level >= 1.0 && level <= t.len() as f64 {
        Val::Num(t[level as usize - 1])
    } else {
        Val::Nil
    }
}

/// A Lua table constructor of one cost entry (all fields constants).
fn cost_entry(
    r#type: &str,
    upfront: bool,
    percent: bool,
    text: &str,
    unaffected: bool,
    raw: bool,
) -> Table {
    let mut f = vec![
        TableField::named(n!("type"), true, r#type),
        TableField::named(n!("upfront"), true, upfront),
        TableField::named(n!("percent"), true, percent),
    ];
    if unaffected {
        f.push(TableField::named(
            n!("unaffectedByGenericCostMults"),
            true,
            true,
        ));
    }
    f.push(TableField::named(n!("text"), true, text));
    f.push(TableField::named(n!("baseCost"), true, 0.0));
    if raw {
        f.push(TableField::named(n!("baseCostRaw"), true, 0.0));
    }
    f.push(TableField::named(n!("totalCost"), true, 0.0));
    f.push(TableField::named(n!("baseCostNoMult"), true, 0.0));
    f.push(TableField::named(n!("finalBaseCost"), true, 0.0));
    Table::from_ctor(f)
}

/// @pob Modules/CalcOffence.lua:1692-2173 h=b7eb396e
/// OF-C: CalcOffence.lua 1692-2173.
pub fn seg_costsAndPasses(ctx: &mut OffenceCtx) -> Flow {
    let env = ctx.env.clone();
    let actor = ctx.actor.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let isAttack = ctx.isAttack;
    let cfg0: Option<&ModCfg> = Some(&*skillCfg);

    // Calculate costs (may be slightly off due to rounding differences)
    // L1693
    let costs = Table::from_ctor(vec![
        TableField::named(
            n!("order"),
            false,
            tbl!(
                "Mana",
                "Life",
                "ES",
                "Soul",
                "Rage",
                "ManaPercent",
                "LifePercent",
                "ManaPerMinute",
                "LifePerMinute",
                "ManaPercentPerMinute",
                "LifePercentPerMinute",
                "ESPerMinute",
                "ESPercentPerMinute"
            ),
        ),
        TableField::bracket(
            n!("Mana"),
            true,
            false,
            cost_entry("Mana", true, false, "mana", false, true),
        ),
        TableField::bracket(
            n!("Life"),
            true,
            false,
            cost_entry("Life", true, false, "life", false, false),
        ),
        TableField::bracket(
            n!("ES"),
            true,
            false,
            cost_entry("ES", true, false, "ES", false, false),
        ),
        TableField::bracket(
            n!("Soul"),
            true,
            false,
            cost_entry("Soul", true, false, "soul", true, false),
        ),
        TableField::bracket(
            n!("Rage"),
            true,
            false,
            cost_entry("Rage", true, false, "rage", false, false),
        ),
        TableField::bracket(
            n!("ManaPercent"),
            true,
            false,
            cost_entry("Mana", true, true, "mana", false, false),
        ),
        TableField::bracket(
            n!("LifePercent"),
            true,
            false,
            cost_entry("Life", true, true, "life", false, false),
        ),
        TableField::bracket(
            n!("ManaPerMinute"),
            true,
            false,
            cost_entry("Mana", false, false, "mana/s", false, false),
        ),
        TableField::bracket(
            n!("LifePerMinute"),
            true,
            false,
            cost_entry("Life", false, false, "life/s", false, false),
        ),
        TableField::bracket(
            n!("ManaPercentPerMinute"),
            true,
            false,
            cost_entry("Mana", false, true, "mana/s", false, false),
        ),
        TableField::bracket(
            n!("LifePercentPerMinute"),
            true,
            false,
            cost_entry("Life", false, true, "life/s", false, false),
        ),
        TableField::bracket(
            n!("ESPerMinute"),
            true,
            false,
            cost_entry("ES", false, false, "ES/s", false, false),
        ),
        TableField::bracket(
            n!("ESPercentPerMinute"),
            true,
            false,
            cost_entry("ES", false, true, "ES/s", false, false),
        ),
    ]);
    ctx.costs = costs.clone();

    // L1711
    if !skillModList.Flag(cfg0, nm!["HasNoCost"]) {
        //Support cost multipliers are calculated first and rounded down after 4 digits
        // L1713
        let mut mult = floor(
            skillModList.More(cfg0, nm!["SupportManaMultiplier"]),
            Some(4),
        );
        // First pass to calculate base costs. Used for cost conversion (e.g. Petrified Blood)
        // L1715
        let additionalLifeCost =
            skillModList.Sum(ModType::BASE, cfg0, nm!["ManaCostAsLifeCost"]) / 100.0; // Extra cost (e.g. Petrified Blood) calculations
                                                                                      // L1716
        let additionalESCost =
            skillModList.Sum(ModType::BASE, cfg0, nm!["ManaCostAsEnergyShieldCost"]) / 100.0; // Extra cost (e.g. Replica Covenant) calculations
                                                                                              // L1717
        let hybridLifeCost =
            skillModList.Sum(ModType::BASE, cfg0, nm!["HybridManaAndLifeCost_Life"]) / 100.0; // Life/Mana mastery
                                                                                              // L1718
        for_ipairs!((_i, resource) in tab(&costs, n!("order")) => {
            let resource: Name = match resource.str() {
                Some(s) => Name::from(&s),
                None => lua_error!("attempt to concatenate a {} value", resource.lua_type()),
            };
            // L1719
            let val = tab(&costs, resource);
            // L1720
            let skillCost = {
                let o = Val::from_value(&skillModList.Override(
                    cfg0,
                    &[cat!("Base", resource, "CostOverride")],
                ));
                lor(o, || {
                    let activeEffect = activeSkill.activeEffect.get();
                    let gel = match activeEffect.grantedEffectLevel.get() {
                        Some(g) => g,
                        None => lua_error!("attempt to index field 'grantedEffectLevel' (a nil value)"),
                    };
                    // CCR-0019 §8 (CCR-0133 §2): SkillsTab.lua:1190 replaced `levels[gemInstance.level].cost`
                    // with `{}` in PoB's global data; `grantedEffectLevel` is
                    // `activeGrantedEffect.levels[activeEffect.level]` (CalcActiveSkill.lua:514/579),
                    // so it is that table when the effect and level are the cleared ones.
                    if crate::model::skills_tab::levelCostCleared(
                        activeEffect.grantedEffect.get().id,
                        activeEffect.level.get(),
                    ) {
                        return Val::Nil;
                    }
                    // `gel.cost and gel.cost[resource] or nil` (an absent cost table reads as empty)
                    match gel.cost.iter().find(|(k, _)| *k == resource) {
                        Some((_, c)) => Val::Num(*c),
                        None => Val::Nil,
                    }
                })
            };
            // L1721
            let mut baseCost = round(
                if skillCost.truthy() {
                    let divisor = match data().costs.by_resource(resource) {
                        Some(c) => c.Divisor,
                        None => lua_error!("attempt to index field '?' (a nil value)"),
                    };
                    skillCost.num() / divisor
                } else {
                    0.0
                },
                Some(2),
            );
            // L1722
            let baseCostNoMult =
                skillModList.Sum(ModType::BASE, cfg0, &[cat!(resource, "CostNoMult")]); // Flat cost from gem e.g. Divine Blessing
            // L1723
            let mut divineBlessingCorrection = 0.0;
            // L1724
            if val.truthy(n!("upfront")) {
                // L1725
                baseCost = baseCost
                    + skillModList.Sum(ModType::BASE, cfg0, &[cat!(resource, "CostBase")]); // Rage Cost
                // L1726
                let v = skillModList.Sum(
                    ModType::BASE,
                    cfg0,
                    &[cat!(resource, "Cost"), n!("Cost")],
                );
                val.set(n!("totalCost"), v);
                // L1727
                if resource == n!("Mana")
                    && activeSkill
                        .skillTypes
                        .get()
                        .with(|t| t.has(SkillType::ReservationBecomesCost))
                    && val.get(n!("percent")).raw_eq(&Val::Bool(false))
                    && !skillModList.Flag(cfg0, nm!["CostESInsteadOfManaOrLife"])
                    && !skillModList.Flag(cfg0, nm!["CostLifeInsteadOfMana"])
                {
                    // Divine Blessing / Totem auras
                    let text = val.get(n!("text"));
                    let text = match text.str() {
                        Some(s) => s,
                        None => lua_error!("attempt to concatenate field 'text' (a {} value)", text.lua_type()),
                    };
                    let gel = || match activeSkill.activeEffect.get().grantedEffectLevel.get() {
                        Some(g) => g,
                        None => lua_error!("attempt to index field 'grantedEffectLevel' (a nil value)"),
                    };
                    // L1728
                    let kFlat = cat!(text, "ReservationFlat");
                    let reservedFlat = lor(
                        lor(activeSkill.skillData.get().get(kFlat), || {
                            Val::from_value(gel().get(kFlat))
                        }),
                        || Val::Num(0.0),
                    );
                    // L1729
                    baseCost = baseCost + reservedFlat.num();
                    // L1730
                    let kPct = cat!(text, "ReservationPercent");
                    let reservedPercent = lor(
                        lor(activeSkill.skillData.get().get(kPct), || {
                            Val::from_value(gel().get(kPct))
                        }),
                        || Val::Num(0.0),
                    )
                    .num();
                    // L1731
                    baseCost = baseCost + m_floor(output.num0(resource) * reservedPercent / 100.0);
                    //Divine Blessing / Totem aura skills that have a percent reservation, round instead of floor the value. This corrects the final result if it would round up
                    // L1733
                    divineBlessingCorrection = round(
                        output.num0(resource) * reservedPercent / 100.0 * mult,
                        None,
                    ) - m_floor(output.num0(resource) * reservedPercent / 100.0 * mult);
                }
            }
            // L1736
            val.set(n!("baseCost"), val.num(n!("baseCost")) + baseCost);
            // L1737
            val.set(
                n!("baseCostNoMult"),
                val.num(n!("baseCostNoMult")) + baseCostNoMult,
            );
            // L1738
            val.set(
                n!("finalBaseCost"),
                (m_floor(val.num(n!("baseCost")) * mult) + val.num(n!("baseCostNoMult")))
                    + divineBlessingCorrection,
            );
            // L1739
            let raw = land(val.get(n!("baseCostRaw")), || {
                Val::Num(
                    val.num(n!("baseCost")) * mult
                        + val.num(n!("baseCostNoMult"))
                        + divineBlessingCorrection,
                )
            });
            val.set(n!("baseCostRaw"), raw);
            // L1740
            if str_is(&val, n!("type"), "Life") {
                // L1741
                let manaType = gsub_plain(resource.as_str(), "Life", "Mana");
                let costsMana = || tab(&costs, manaType);
                // L1742
                if skillModList.Flag(cfg0, nm!["CostLifeInsteadOfMana"]) {
                    // Blood Magic / Lifetap
                    // L1743
                    if val.truthy(n!("upfront")) {
                        // L1744
                        let manaTotalCost =
                            skillModList.Sum(ModType::BASE, cfg0, &[cat!(manaType, "Cost")]);
                        // L1745
                        val.set(
                            n!("totalCost"),
                            val.num(n!("totalCost")) + m_max(0.0, manaTotalCost),
                        );
                        // L1746
                        let m = costsMana();
                        m.set(n!("totalCost"), m.num(n!("totalCost")) - manaTotalCost);
                    }
                    // L1748
                    val.set(
                        n!("baseCost"),
                        val.num(n!("baseCost")) + costsMana().num(n!("baseCost")),
                    );
                    // L1749
                    val.set(
                        n!("baseCostNoMult"),
                        val.num(n!("baseCostNoMult")) + costsMana().num(n!("baseCostNoMult")),
                    );
                    // L1750
                    val.set(
                        n!("finalBaseCost"),
                        val.num(n!("finalBaseCost")) + costsMana().num(n!("finalBaseCost")),
                    );
                    // L1751
                    costsMana().set(n!("baseCost"), 0.0);
                    // L1752
                    costsMana().set(n!("baseCostRaw"), 0.0);
                    // L1753
                    costsMana().set(n!("finalBaseCost"), 0.0);
                    // L1754
                    costsMana().set(n!("baseCostNoMult"), 0.0);
                } else if (additionalLifeCost > 0.0 || hybridLifeCost > 0.0)
                    && !skillModList.Flag(cfg0, nm!["CostESInsteadOfManaOrLife"])
                {
                    // L1756
                    val.set(n!("baseCost"), costsMana().get(n!("baseCost")));
                    // L1757
                    val.set(
                        n!("finalBaseCost"),
                        val.num(n!("finalBaseCost"))
                            + round(
                                costsMana().num(n!("finalBaseCost"))
                                    * (hybridLifeCost + additionalLifeCost),
                                None,
                            ),
                    );
                    // L1758
                    if val.truthy(n!("upfront")) && hybridLifeCost > 0.0 {
                        // Only positive flat mana cost is converted, and it is rounded separately from base cost.
                        // L1760
                        val.set(
                            n!("totalCost"),
                            val.num(n!("totalCost"))
                                + round(
                                    m_max(
                                        0.0,
                                        skillModList.Sum(
                                            ModType::BASE,
                                            cfg0,
                                            &[cat!(manaType, "Cost")],
                                        ),
                                    ) * hybridLifeCost,
                                    None,
                                ),
                        );
                    }
                }
            } else if str_is(&val, n!("type"), "ES") {
                // L1764
                let manaType = gsub_plain(resource.as_str(), "ES", "Mana");
                // L1765
                let lifeType = gsub_plain(resource.as_str(), "ES", "Life");
                let costsMana = || tab(&costs, manaType);
                let costsLife = || tab(&costs, lifeType);
                // L1766
                if skillModList.Flag(cfg0, nm!["CostESInsteadOfManaOrLife"]) {
                    // Whispers of Infinity
                    // L1767
                    if val.truthy(n!("upfront")) {
                        // L1768
                        let manaTotalCost =
                            skillModList.Sum(ModType::BASE, cfg0, &[cat!(manaType, "Cost")]);
                        // L1769
                        let lifeTotalCost =
                            skillModList.Sum(ModType::BASE, cfg0, &[cat!(lifeType, "Cost")]);
                        // L1770
                        val.set(
                            n!("totalCost"),
                            val.num(n!("totalCost"))
                                + m_max(0.0, manaTotalCost)
                                + m_max(0.0, lifeTotalCost),
                        );
                        // L1771
                        let m = costsMana();
                        m.set(n!("totalCost"), m.num(n!("totalCost")) - manaTotalCost);
                        // L1772
                        let l = costsLife();
                        l.set(n!("totalCost"), l.num(n!("totalCost")) - lifeTotalCost);
                    }
                    // L1774
                    val.set(
                        n!("baseCost"),
                        val.num(n!("baseCost"))
                            + costsMana().num(n!("baseCost"))
                            + costsLife().num(n!("baseCost")),
                    );
                    // L1775
                    val.set(
                        n!("baseCostNoMult"),
                        val.num(n!("baseCostNoMult"))
                            + costsMana().num(n!("baseCostNoMult"))
                            + costsLife().num(n!("baseCostNoMult")),
                    );
                    // L1776
                    val.set(
                        n!("finalBaseCost"),
                        val.num(n!("finalBaseCost"))
                            + costsMana().num(n!("finalBaseCost"))
                            + costsLife().num(n!("finalBaseCost")),
                    );
                    // L1777
                    costsMana().set(n!("baseCost"), 0.0);
                    // L1778
                    costsMana().set(n!("baseCostRaw"), 0.0);
                    // L1779
                    costsMana().set(n!("finalBaseCost"), 0.0);
                    // L1780
                    costsMana().set(n!("baseCostNoMult"), 0.0);
                    // L1781
                    costsLife().set(n!("baseCost"), 0.0);
                    // L1782
                    costsLife().set(n!("baseCostRaw"), 0.0);
                    // L1783
                    costsLife().set(n!("finalBaseCost"), 0.0);
                    // L1784
                    costsLife().set(n!("baseCostNoMult"), 0.0);
                } else if additionalESCost > 0.0 {
                    // L1786
                    val.set(n!("baseCost"), costsMana().get(n!("baseCost")));
                    // L1787
                    val.set(
                        n!("finalBaseCost"),
                        val.num(n!("finalBaseCost"))
                            + round(costsMana().num(n!("finalBaseCost")) * additionalESCost, None),
                    );
                }
            } else if str_is(&val, n!("type"), "Rage") {
                // L1790
                if skillModList.Flag(cfg0, nm!["CostRageInsteadOfSouls"]) {
                    // Hateforge
                    let soul = || tab(&costs, n!("Soul"));
                    // L1791
                    val.set(n!("baseCost"), soul().get(n!("baseCost")));
                    // L1792
                    val.set(
                        n!("baseCostNoMult"),
                        val.num(n!("baseCostNoMult")) + soul().num(n!("baseCostNoMult")),
                    );
                    // L1793
                    val.set(n!("finalBaseCost"), soul().get(n!("baseCost")));
                    // L1794
                    mult = 1.0;
                    // L1795
                    soul().set(n!("baseCost"), 0.0);
                    // L1796
                    soul().set(n!("baseCostNoMult"), 0.0);
                    // L1797
                    soul().set(n!("finalBaseCost"), 0.0);
                }
            }
        });
        // L1801
        for_ipairs!((_i, resource) in tab(&costs, n!("order")) => {
            let resource: Name = match resource.str() {
                Some(s) => Name::from(&s),
                None => lua_error!("attempt to concatenate a {} value", resource.lua_type()),
            };
            // L1802
            let val = tab(&costs, resource);
            // L1803
            let resource = if val.truthy(n!("upfront")) {
                resource
            } else {
                gsub_plain(resource.as_str(), "Minute", "Second")
            };
            // L1804
            let hasCost = val.num(n!("baseCost")) > 0.0
                || val.num(n!("totalCost")) > 0.0
                || val.num(n!("baseCostNoMult")) > 0.0
                || val.num(n!("finalBaseCost")) > 0.0;
            // L1805
            output.set(cat!(resource, "HasCost"), hasCost);
            // L1806
            let costName = cat!(resource, "Cost");
            // L1807
            let costNameRaw = cat!(costName, "Raw");
            // L1808
            let mut moreType = 1.0;
            // L1809
            let mut moreCost = 1.0;
            // L1810
            let mut inc = 0.0;
            let vtype = val.get(n!("type"));
            let vtype: Name = match vtype.str() {
                Some(s) => Name::from(&s),
                None => lua_error!("attempt to concatenate field 'type' (a {} value)", vtype.lua_type()),
            };
            // L1811
            let costEfficiency = calcLib::r#mod(
                &skillModList,
                cfg0,
                &[cat!(vtype, "CostEfficiency"), n!("CostEfficiency")],
            );
            // L1812
            if !val.truthy(n!("unaffectedByGenericCostMults")) {
                // L1813
                output.set(costName, val.get(n!("finalBaseCost")));
                // L1814
                moreType = skillModList.More(cfg0, &[cat!(vtype, "Cost")]);
                // L1815
                moreCost = skillModList.More(cfg0, nm!["Cost"]);
                // L1816
                inc = skillModList.Sum(ModType::INC, cfg0, &[cat!(vtype, "Cost"), n!("Cost")]);
                // L1817
                let raw = land(val.get(n!("baseCostRaw")), || {
                    Val::Num(m_max(
                        0.0,
                        m_max(
                            0.0,
                            (1.0 + inc / 100.0) * val.num(n!("baseCostRaw")) * moreType * moreCost
                                / costEfficiency,
                        ) + val.num(n!("totalCost")),
                    ))
                });
                output.set(costNameRaw, raw);
                // L1818
                if inc < 0.0 {
                    // L1819
                    let v = m_max(0.0, m_ceil((1.0 + inc / 100.0) * output.num(costName)));
                    output.set(costName, v);
                } else {
                    // L1821
                    let v = m_max(0.0, m_floor((1.0 + inc / 100.0) * output.num(costName)));
                    output.set(costName, v);
                }
                // L1823
                if moreType < 1.0 {
                    // L1824
                    let v = m_max(0.0, m_ceil(moreType * output.num(costName)));
                    output.set(costName, v);
                } else {
                    // L1826
                    let v = m_max(0.0, m_floor(moreType * output.num(costName)));
                    output.set(costName, v);
                }
                // L1828
                if moreCost < 1.0 {
                    // L1829
                    let v = m_max(0.0, m_ceil(moreCost * output.num(costName)));
                    output.set(costName, v);
                } else {
                    // L1831
                    let v = m_max(0.0, m_floor(moreCost * output.num(costName)));
                    output.set(costName, v);
                }
                // Apply cost efficiency (similar to reservation efficiency)
                // L1834
                let v = m_max(0.0, output.num(costName) / costEfficiency);
                output.set(costName, v);
                // L1835
                if val.truthy(n!("upfront")) && !val.truthy(n!("percent")) {
                    // The game stores upfront resource costs as whole numbers after efficiency.
                    // L1837
                    let v = round(output.num(costName), None);
                    output.set(costName, v);
                }
                // L1839
                let v = m_max(0.0, output.num(costName) + val.num(n!("totalCost")));
                output.set(costName, v);
                // L1840
                if vtype == n!("Mana") && hybridLifeCost > 0.0 {
                    // Life/Mana Mastery
                    // L1841
                    let v = m_max(0.0, m_floor((1.0 - hybridLifeCost) * output.num(costName)));
                    output.set(costName, v);
                    // L1842
                    let raw = land(output.get(costNameRaw), || {
                        Val::Num(m_max(0.0, (1.0 - hybridLifeCost) * output.num(costNameRaw)))
                    });
                    output.set(costNameRaw, raw);
                }
            } else {
                // L1845
                moreType = skillModList.More(cfg0, &[cat!(vtype, "Cost")]);
                // L1846
                inc = skillModList.Sum(ModType::INC, cfg0, &[cat!(vtype, "Cost")]);
                // L1847
                let v = m_floor(val.num(n!("baseCost")) + val.num(n!("baseCostNoMult")));
                output.set(costName, v);
                // L1848
                let v = m_max(0.0, (1.0 + inc / 100.0) * output.num(costName));
                output.set(costName, v);
                // L1849
                let v = m_max(0.0, moreType * output.num(costName));
                output.set(costName, v);
                // Apply cost efficiency for unaffected costs too
                // L1851
                let v = m_max(0.0, output.num(costName) / costEfficiency);
                output.set(costName, v);
                // L1852
                if val.truthy(n!("upfront")) && !val.truthy(n!("percent")) {
                    // The game stores upfront resource costs as whole numbers after efficiency.
                    // L1854
                    let v = round(output.num(costName), None);
                    output.set(costName, v);
                }
                // L1856
                let v = m_max(0.0, output.num(costName) + val.num(n!("totalCost")));
                output.set(costName, v);
                // L1857
                let raw = land(val.get(n!("baseCostRaw")), || {
                    Val::Num(m_max(
                        0.0,
                        m_max(
                            0.0,
                            (1.0 + inc / 100.0)
                                * (val.num(n!("baseCostRaw")) + val.num(n!("baseCostNoMult")))
                                * moreType
                                / costEfficiency,
                        ) + val.num(n!("totalCost")),
                    ))
                });
                output.set(costNameRaw, raw);
            }
            // L1859
            if let Some(breakdown) = breakdown.as_ref().filter(|_| hasCost) {
                let text = val.get(n!("text"));
                let text: LStr = match text.str() {
                    Some(s) => s,
                    None => lua_error!("attempt to concatenate field 'text' (a {} value)", text.lua_type()),
                };
                let percent = val.truthy(n!("percent"));
                // L1860
                let f = format!(
                    "%.2f{} ^8(base {} cost)",
                    if percent { "%%" } else { "" },
                    text
                );
                breakdown.set(
                    costName,
                    tbl!(lua_format(&f, &[fmt_arg(&val.get(n!("baseCost")))])),
                );
                // L1863
                if mult != 1.0 {
                    // L1864
                    insert_at(breakdown, costName, lfmt!("x %.4f ^8(cost multiplier)", mult));
                }
                // L1866
                if val.num(n!("baseCostNoMult")) != 0.0 {
                    // L1867
                    let f = format!("+ %d ^8(additional {text} cost)");
                    insert_at(
                        breakdown,
                        costName,
                        lua_format(&f, &[fmt_arg(&val.get(n!("baseCostNoMult")))]),
                    );
                }
                // L1869
                if vtype == n!("Life")
                    && (hybridLifeCost + additionalLifeCost) != 0.0
                    && !skillModList.Flag(cfg0, nm!["CostLifeInsteadOfMana"])
                {
                    // L1870
                    insert_at(
                        breakdown,
                        costName,
                        lfmt!(
                            "x %.2f ^8(mana cost conversion)",
                            hybridLifeCost + additionalLifeCost
                        ),
                    );
                }
                // L1872
                if vtype == n!("ES")
                    && additionalESCost != 0.0
                    && !skillModList.Flag(cfg0, nm!["CostLifeInsteadOfMana"])
                    && !skillModList.Flag(cfg0, nm!["CostESInsteadOfManaOrLife"])
                {
                    // L1873
                    insert_at(
                        breakdown,
                        costName,
                        lfmt!("x %.2f ^8(mana cost conversion)", additionalESCost),
                    );
                }
                // L1875
                if inc != 0.0 {
                    // L1876
                    let f = format!("x %.2f ^8(increased/reduced {text} cost)");
                    insert_at(
                        breakdown,
                        costName,
                        lua_format(&f, &[FmtArg::Num(1.0 + inc / 100.0)]),
                    );
                }
                // L1878
                if moreCost != 1.0 {
                    // L1879
                    insert_at(breakdown, costName, lfmt!("x %.2f ^8(more/less cost)", moreCost));
                }
                // L1881
                if moreType != 1.0 {
                    // L1882
                    let f = format!("x %.2f ^8(more/less {text} cost)");
                    insert_at(breakdown, costName, lua_format(&f, &[FmtArg::Num(moreType)]));
                }
                // L1884
                if costEfficiency != 1.0 {
                    // L1885
                    let f = format!("/ %.2f ^8({text} cost efficiency)");
                    insert_at(
                        breakdown,
                        costName,
                        lua_format(&f, &[FmtArg::Num(costEfficiency)]),
                    );
                }
                // L1887
                if val.num(n!("totalCost")) != 0.0 {
                    // L1888
                    let f = format!("%+d ^8(total {text} cost)");
                    insert_at(
                        breakdown,
                        costName,
                        lua_format(&f, &[fmt_arg(&val.get(n!("totalCost")))]),
                    );
                }
                // L1890
                if vtype == n!("Mana") && hybridLifeCost > 0.0 {
                    // L1891
                    insert_at(
                        breakdown,
                        costName,
                        lfmt!(
                            "x %.2f ^8(%d%% paid for with life)",
                            1.0 - hybridLifeCost,
                            hybridLifeCost * 100.0
                        ),
                    );
                }
                // L1893
                let f = format!(
                    "= %{}{}",
                    if val.truthy(n!("upfront")) { "d" } else { ".2f" },
                    if percent { "%%" } else { "" }
                );
                insert_at(
                    breakdown,
                    costName,
                    lua_format(&f, &[fmt_arg(&output.get(costName))]),
                );
            }
        });
    }
    // Eldritch Battery adds maximum Energy Shield to the Mana available for payable-cost damage.
    // L1898
    let v = output.num0(n!("ManaUnreserved"))
        + if modDB.Flag(None, nm!["EnergyShieldProtectsMana"]) {
            output.num0(n!("EnergyShield"))
        } else {
            0.0
        };
    output.set(n!("ManaCostPayablePool"), v);

    // account for Sacrificial Zeal
    // Note: Sacrificial Zeal grants Added Spell Physical Damage equal to 25% of the Skill's Mana Cost, and causes you to take Physical Damage over Time, for 4 seconds
    // L1902
    if skillModList.Flag(None, nm!["Condition:SacrificialZeal"]) && output.truthy(n!("ManaHasCost"))
    {
        // L1903
        let multiplier = 0.25;
        // L1904
        skillModList.NewMod(
            n!("PhysicalMin"),
            ModType::BASE,
            m_floor(output.num(n!("ManaCost")) * multiplier),
            "Sacrificial Zeal",
            ModFlag::Spell,
            0,
            &[],
        );
        // L1905
        skillModList.NewMod(
            n!("PhysicalMax"),
            ModType::BASE,
            m_floor(output.num(n!("ManaCost")) * multiplier),
            "Sacrificial Zeal",
            ModFlag::Spell,
            0,
            &[],
        );
    }

    // L1908
    runSkillFunc(ctx, SkillHook::preDamageFunc);

    // L1910
    if activeSkill
        .skillTypes
        .get()
        .with(|t| t.has(SkillType::Brand))
    {
        // L1911
        let damageLabel = if skillData.truthy(n!("countsAttachedBrandsInDamage")) {
            "Average Damage"
        } else {
            "DPS"
        };
        // L1912
        let count = output.get(n!("AttachedBrandCount"));
        let s = lfmt!(
            "%s for %d attached Brand%s",
            damageLabel,
            fmt_arg(&count),
            if count.raw_eq(&Val::Num(1.0)) {
                ""
            } else {
                "s"
            }
        );
        activeSkill.infoMessage.set(Some(LStr::from(s.as_str())));
    }

    // Handle corpse and enemy explosions
    // L1916
    let monsterLife = lor(skillData.get(n!("corpseLife")), || {
        // `env.enemyLevel and data.monsterLifeTable[env.enemyLevel] or 100` (enemyLevel is a number)
        lor(monsterLifeTable(env.enemyLevel.get()), || Val::Num(100.0))
    })
    .num();
    ctx.monsterLife = monsterLife;
    // L1917
    if skillData.truthy(n!("explodeCorpse"))
        && lor(skillData.get(n!("corpseLife")), || {
            Val::Num(env.enemyLevel.get())
        })
        .truthy()
    {
        // L1918
        let damageType = lor(skillData.get(n!("corpseExplosionDamageType")), || {
            Val::from("Fire")
        });
        let damageType: Name = match damageType.str() {
            Some(s) => Name::from(&s),
            None => lua_error!(
                "attempt to concatenate local 'damageType' (a {} value)",
                damageType.lua_type()
            ),
        };
        // L1919
        let v = monsterLife
            * lor(skillData.get(n!("corpseExplosionLifeMultiplier")), || {
                skillData.get(n!("selfFireExplosionLifeMultiplier"))
            })
            .num();
        skillData.set(cat!(damageType, "BonusMin"), v);
        // L1920
        let v = monsterLife
            * lor(skillData.get(n!("corpseExplosionLifeMultiplier")), || {
                skillData.get(n!("selfFireExplosionLifeMultiplier"))
            })
            .num();
        skillData.set(cat!(damageType, "BonusMax"), v);
    }
    // L1922
    if skillFlags.truthy(n!("monsterExplode")) {
        // L1923
        for damageType in dmgTypeList() {
            // L1924
            let percentage = skillData.get(cat!(damageType, "EffectiveExplodePercentage"));
            // L1925
            let base = percentage.num_or(0.0) * monsterLife / 100.0;
            // L1926
            skillData.set(cat!(damageType, "Min"), base);
            // L1927
            skillData.set(cat!(damageType, "Max"), base);
        }
    }

    // Cache global damage disabling flags
    // L1932
    let canDeal = Table::new();
    // L1933
    for damageType in dmgTypeList() {
        // L1934
        canDeal.set(
            damageType,
            !skillModList.Flag(cfg0, &[cat!("DealNo", damageType), n!("DealNoDamage")]),
        );
    }
    ctx.canDeal = canDeal;

    // Calculate damage conversion percentages
    // L1989
    activeSkill
        .conversionTable
        .set(Some(buildConversionTable(&skillModList, cfg0)));
    // L1990
    if let Some(w1) = activeSkill.weapon1Cfg.get() {
        // L1991
        w1.conversionTable
            .set(Some(buildConversionTable(&skillModList, Some(&*w1))));
    }
    // L1993
    if let Some(w2) = activeSkill.weapon2Cfg.get() {
        // L1994
        w2.conversionTable
            .set(Some(buildConversionTable(&skillModList, Some(&*w2))));
    }

    // Configure damage passes
    // L1998
    let passList: Shared<Vec<Table>> = Shared::new(Vec::new());
    ctx.passList = passList.clone();
    // L1999
    if isAttack {
        // L2000
        output.set(n!("MainHand"), Table::new());
        // L2001
        output.set(n!("OffHand"), Table::new());
        // L2002
        output.set(
            n!("PreciseTechnique"),
            env.keystonesAdded.get().get(n!("Precise Technique")),
        );
        // L2003
        let critOverride =
            Val::from_value(&skillModList.Override(cfg0, nm!["WeaponBaseCritChance"]));
        // L2004
        if skillFlags.truthy(n!("weapon1Attack")) {
            let outMain = tab(&output, n!("MainHand"));
            // L2005
            if let Some(breakdown) = breakdown.as_ref() {
                // L2006
                breakdown.set(
                    n!("MainHand"),
                    newSubBreakdown(&skillModList, &outMain).to_val(),
                );
            }
            // L2008
            match activeSkill.weapon1Cfg.get() {
                Some(c) => c.skillStats.set(Some(outMain.clone())),
                None => lua_error!("attempt to index field 'weapon1Cfg' (a nil value)"),
            }
            // L2009
            let source = actor.weaponData1.get().copyTable(false);
            // L2010
            if critOverride.truthy()
                && source.truthy(n!("type"))
                && !source.get(n!("type")).raw_eq(&Val::from("None"))
            {
                // L2011
                source.set(n!("CritChance"), critOverride.clone());
            }
            // L2013
            passList.push(tbl! {
                label = "Main Hand",
                source = source,
                cfg = cfg_val(activeSkill.weapon1Cfg.get()),
                output = output.get(n!("MainHand")),
                breakdown = match breakdown.as_ref() {
                    Some(b) => b.get(n!("MainHand")),
                    None => Val::Nil,
                },
            });
        }
        // L2021
        if skillFlags.truthy(n!("weapon2Attack")) {
            let outOff = tab(&output, n!("OffHand"));
            // L2022
            if let Some(breakdown) = breakdown.as_ref() {
                // L2023
                breakdown.set(
                    n!("OffHand"),
                    newSubBreakdown(&skillModList, &outOff).to_val(),
                );
            }
            // L2025
            match activeSkill.weapon2Cfg.get() {
                Some(c) => c.skillStats.set(Some(outOff.clone())),
                None => lua_error!("attempt to index field 'weapon2Cfg' (a nil value)"),
            }
            // L2026
            let source = actor.weaponData2.get().copyTable(false);
            // L2027
            if critOverride.truthy()
                && source.truthy(n!("type"))
                && !source.get(n!("type")).raw_eq(&Val::from("None"))
            {
                // L2028
                source.set(n!("CritChance"), critOverride.clone());
            }
            // L2030
            if skillData.truthy(n!("CritChance")) {
                // L2031
                source.set(n!("CritChance"), skillData.get(n!("CritChance")));
            }
            // L2033
            if skillData.truthy(n!("setOffHandPhysicalMin"))
                && skillData.truthy(n!("setOffHandPhysicalMax"))
            {
                // L2034
                source.set(
                    n!("PhysicalMin"),
                    skillData.get(n!("setOffHandPhysicalMin")),
                );
                // L2035
                source.set(
                    n!("PhysicalMax"),
                    skillData.get(n!("setOffHandPhysicalMax")),
                );
            }
            // L2037
            if skillData.truthy(n!("setOffHandFireMin"))
                && skillData.truthy(n!("setOffHandFireMax"))
            {
                // L2038
                source.set(n!("FireMin"), skillData.get(n!("setOffHandFireMin")));
                // L2039
                source.set(n!("FireMax"), skillData.get(n!("setOffHandFireMax")));
            }
            // L2041
            if skillData.truthy(n!("setOffHandColdMin"))
                && skillData.truthy(n!("setOffHandColdMax"))
            {
                // L2042
                source.set(n!("ColdMin"), skillData.get(n!("setOffHandColdMin")));
                // L2043
                source.set(n!("ColdMax"), skillData.get(n!("setOffHandColdMax")));
            }
            // L2045
            if skillData.truthy(n!("attackTime")) {
                // L2046
                source.set(n!("AttackRate"), 1000.0 / skillData.num(n!("attackTime")));
            }
            // L2048
            passList.push(tbl! {
                label = "Off Hand",
                source = source,
                cfg = cfg_val(activeSkill.weapon2Cfg.get()),
                output = output.get(n!("OffHand")),
                breakdown = match breakdown.as_ref() {
                    Some(b) => b.get(n!("OffHand")),
                    None => Val::Nil,
                },
            });
        }
    } else {
        // L2057
        passList.push(tbl! {
            label = "Skill",
            source = skillData.clone(),
            cfg = cfg_val(Some(skillCfg.clone())),
            output = output.clone(),
            breakdown = match breakdown.as_ref() {
                Some(b) => b.to_val(),
                None => Val::Nil,
            },
        });
    }

    // `local function combineStat(stat, mode, ...)` (L2066-2170): [`combineStat`].

    // L2171
    ctx.storedMainHandAccuracy = None;
    // L2172
    ctx.storedMainHandAccuracyVsEnemy = None;
    // L2173
    ctx.storedSustainedTraumaBreakdown = Table::new();
    Flow::Normal
}

/// A cfg stored in a Lua table (`pass.cfg`): the `ModCfg` object, or nil.
fn cfg_val(c: Option<Cfg>) -> Val {
    match c {
        Some(c) => Val::Obj(c),
        None => Val::Nil,
    }
}

/// `require(calcs.breakdownModule)(skillModList, output.X)` (CalcOffence.lua:2006/2023): the
/// module function called without `actor`, so the instance's `actor` upvalue is nil.
fn newSubBreakdown(modDB: &ModList, output: &Output) -> Breakdown {
    Breakdown {
        t: Table::new(),
        modDB: modDB.clone(),
        output: output.clone(),
        actor: Weak::new(),
    }
}

trait BreakdownVal {
    fn to_val(&self) -> Val;
}

impl BreakdownVal for Breakdown {
    /// A breakdown instance stored in a Lua table (an object value, as the probe schema writes it).
    fn to_val(&self) -> Val {
        Val::Obj(Rc::new(self.clone()))
    }
}

/// `local function buildConversionTable(cfg)` (CalcOffence.lua:1938-1987; a non-crossing local
/// function, so private; its provenance is part of the OF-C segment marker). `skillModList` is
/// the upvalue.
fn buildConversionTable(skillModList: &ModList, cfg: Option<&ModCfg>) -> Table {
    // L1939
    let conversionTable = Table::new();
    let dmgTypeList = dmgTypeList();
    // L1940
    for damageTypeIndex in 1..=4usize {
        // L1941
        let damageType = dmgTypeList[damageTypeIndex - 1];
        // `wipeTable(tempTable1..3)`: scratch tables that never escape (module_state.rs)
        // L1942
        let globalConv = Table::new();
        // L1943
        let skillConv = Table::new();
        // L1944
        let add = Table::new();
        // L1945
        let mut globalTotal = 0.0;
        let mut skillTotal = 0.0;
        // L1946
        for otherTypeIndex in (damageTypeIndex + 1)..=5usize {
            // For all possible destination types, check for global and skill conversions
            // L1948
            let otherType = dmgTypeList[otherTypeIndex - 1];
            // L1949
            globalConv.set(
                otherType,
                m_max(
                    skillModList.Sum(
                        ModType::BASE,
                        cfg,
                        &names([
                            Some(cat!(damageType, "DamageConvertTo", otherType)),
                            isElemental(damageType)
                                .then(|| cat!("ElementalDamageConvertTo", otherType)),
                            (damageType != n!("Chaos"))
                                .then(|| cat!("NonChaosDamageConvertTo", otherType)),
                        ]),
                    ),
                    0.0,
                ),
            );
            // L1950
            globalTotal = globalTotal + globalConv.num(otherType);
            // L1951
            skillConv.set(
                otherType,
                m_max(
                    skillModList.Sum(
                        ModType::BASE,
                        cfg,
                        &[cat!("Skill", damageType, "DamageConvertTo", otherType)],
                    ),
                    0.0,
                ),
            );
            // L1952
            skillTotal = skillTotal + skillConv.num(otherType);
            // L1953
            add.set(
                otherType,
                m_max(
                    skillModList.Sum(
                        ModType::BASE,
                        cfg,
                        &names([
                            Some(cat!(damageType, "DamageGainAs", otherType)),
                            isElemental(damageType)
                                .then(|| cat!("ElementalDamageGainAs", otherType)),
                            (damageType != n!("Chaos"))
                                .then(|| cat!("NonChaosDamageGainAs", otherType)),
                        ]),
                    ),
                    0.0,
                ),
            );
        }
        // L1955
        if skillTotal > 100.0 {
            // Skill conversion exceeds 100%, scale it down and remove non-skill conversions
            // L1957
            let factor = 100.0 / skillTotal;
            // L1958
            for (r#type, val) in skillConv.pairs() {
                // L1959
                skillConv.set(r#type, val.num() * factor);
            }
            // L1961
            for (r#type, _val) in globalConv.pairs() {
                // L1962
                globalConv.set(r#type, 0.0);
            }
        } else if globalTotal + skillTotal > 100.0 {
            // Conversion exceeds 100%, scale down non-skill conversions
            // L1966
            let factor = (100.0 - skillTotal) / globalTotal;
            // L1967
            for (r#type, val) in globalConv.pairs() {
                // L1968
                globalConv.set(r#type, val.num() * factor);
            }
            // L1970
            globalTotal = globalTotal * factor;
        }
        // L1972
        let dmgTable = tbl! { conversion = Table::new(), gain = Table::new() };
        // L1973
        let mut convertedTotal = 0.0;
        // L1974
        for (r#type, _v) in globalConv.pairs() {
            // The game stores each scaled conversion destination as a whole percent.
            // L1976
            let conversion = round(globalConv.num(r#type) + skillConv.num(r#type), None);
            // L1977
            tab(&dmgTable, n!("conversion")).set(r#type, conversion / 100.0);
            // L1978
            tab(&dmgTable, n!("gain")).set(r#type, add.num(r#type) / 100.0);
            // L1979
            dmgTable.set(r#type, (conversion + add.num(r#type)) / 100.0);
            // L1980
            convertedTotal = convertedTotal + conversion;
        }
        // L1982
        dmgTable.set(n!("mult"), 1.0 - m_min(convertedTotal / 100.0, 1.0));
        // L1983
        conversionTable.set(damageType, dmgTable);
    }
    // L1985
    conversionTable.set(n!("Chaos"), tbl! { mult = 1 });
    // L1986
    conversionTable
}

/// `combineStat(stat, mode, ...)` (CalcOffence.lua:2066-2170), declared in OF-C, used after the
/// pass loops (§7.2). Its provenance is part of the OF-C segment marker above. Upvalues:
/// `output`, `breakdown`, `skillFlags`, `skillData` (the ctx) and `globalOutput`
/// (`OFFENCE_STATE`). `extra` is the first vararg (`output.MainHand[...]`), a stat name
/// (CCR-0055: the frozen stub took `Option<&Table>`).
// called after the pass loops by OF-D (2510), OF-F (3959) and OF-I (5652)
#[allow(dead_code)]
pub fn combineStat(ctx: &OffenceCtx, stat: Name, mode: Name, extra: Option<Name>) {
    let output = &ctx.output;
    let breakdown = ctx.breakdown.as_ref();
    let skillFlags = &ctx.skillFlags;
    let skillData = &ctx.skillData;
    // `output.MainHand` / `output.OffHand` (indexing a nil field raises)
    let mh = || tab(output, n!("MainHand"));
    let oh = || tab(output, n!("OffHand"));
    // `t[...]`: t[nil] reads nil
    let at_extra = |t: &Table| match extra {
        Some(k) => t.get(k),
        None => Val::Nil,
    };
    // Combine stats from Main Hand and Off Hand according to the mode
    // L2068
    if mode == n!("OR") || !skillFlags.truthy(n!("bothWeaponAttack")) {
        // L2069
        let v = lor(mh().get(stat), || oh().get(stat));
        output.set(stat, v);
    } else if mode == n!("ADD") {
        // L2071
        let v = mh().num0(stat) + oh().num0(stat);
        output.set(stat, v);
    } else if mode == n!("AVERAGE") {
        // L2073
        let v = (mh().num0(stat) + oh().num0(stat)) / 2.0;
        output.set(stat, v);
    } else if mode == n!("HARMONICMEAN") {
        // L2075
        if mh().get(stat).raw_eq(&Val::Num(0.0)) || oh().get(stat).raw_eq(&Val::Num(0.0)) {
            // L2076
            output.set(stat, 0.0);
        } else {
            // L2078
            let v = 2.0 / ((1.0 / mh().num(stat)) + (1.0 / oh().num(stat)));
            output.set(stat, v);
        }
    } else if mode == n!("CHANCE") {
        // L2081
        if mh().truthy(stat) && oh().truthy(stat) {
            // L2082
            let mut mainChance = at_extra(&mh()).num() * mh().num(n!("HitChance"));
            // L2083
            let mut offChance = at_extra(&oh()).num() * oh().num(n!("HitChance"));
            // L2084
            if skillData.truthy(n!("doubleHitsWhenDualWielding")) {
                // L2085
                mainChance = mainChance / 10000.0;
                // L2086
                offChance = offChance / 10000.0;
                // L2087
                let v = mh().num(stat) * mainChance + oh().num(stat) * offChance;
                output.set(stat, v);
                // L2088
                if let Some(breakdown) = breakdown {
                    // L2089
                    if !breakdown.truthy(stat) {
                        // L2090
                        breakdown.set(stat, Table::new());
                    }
                    // L2092
                    insert_at(breakdown, stat, "Contribution from Main Hand:");
                    // L2093
                    insert_at(breakdown, stat, lfmt!("%.1f", mh().num(stat)));
                    // L2094
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!("x %.3f ^8(chance of main hand)", mainChance),
                    );
                    // L2095
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!("= %.1f", mh().num(stat) * mainChance),
                    );
                    // L2096
                    insert_at(breakdown, stat, "Contribution from Off Hand:");
                    // L2097
                    insert_at(breakdown, stat, lfmt!("%.1f", oh().num(stat)));
                    // L2098
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!("x %.3f ^8(chance of  off hand)", offChance),
                    );
                    // L2099
                    insert_at(breakdown, stat, lfmt!("= %.1f", oh().num(stat) * offChance));
                    // L2100
                    insert_at(breakdown, stat, "Total:");
                    // L2101
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!(
                            "%.1f + %.1f",
                            mh().num(stat) * mainChance,
                            oh().num(stat) * offChance
                        ),
                    );
                    // L2102
                    insert_at(breakdown, stat, lfmt!("= %.1f", output.num(stat)));
                }
            } else {
                // L2105
                let mainPortion = mainChance / (mainChance + offChance);
                // L2106
                let offPortion = offChance / (mainChance + offChance);
                // L2107
                let v = mh().num(stat) * mainPortion + oh().num(stat) * offPortion;
                output.set(stat, v);
                // L2108
                if let Some(breakdown) = breakdown {
                    // L2109
                    if !breakdown.truthy(stat) {
                        // L2110
                        breakdown.set(stat, Table::new());
                    }
                    // L2112
                    insert_at(breakdown, stat, "Contribution from Main Hand:");
                    // L2113
                    insert_at(breakdown, stat, lfmt!("%.1f", mh().num(stat)));
                    // L2114
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!(
                            "x %.3f ^8(portion of instances created by main hand)",
                            mainPortion
                        ),
                    );
                    // L2115
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!("= %.1f", mh().num(stat) * mainPortion),
                    );
                    // L2116
                    insert_at(breakdown, stat, "Contribution from Off Hand:");
                    // L2117
                    insert_at(breakdown, stat, lfmt!("%.1f", oh().num(stat)));
                    // L2118
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!(
                            "x %.3f ^8(portion of instances created by off hand)",
                            offPortion
                        ),
                    );
                    // L2119
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!("= %.1f", oh().num(stat) * offPortion),
                    );
                    // L2120
                    insert_at(breakdown, stat, "Total:");
                    // L2121
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!(
                            "%.1f + %.1f",
                            mh().num(stat) * mainPortion,
                            oh().num(stat) * offPortion
                        ),
                    );
                    // L2122
                    insert_at(breakdown, stat, lfmt!("= %.1f", output.num(stat)));
                }
            }
        } else {
            // L2126
            let v = lor(mh().get(stat), || oh().get(stat));
            output.set(stat, v);
        }
    } else if mode == n!("CHANCE_AILMENT") {
        // L2129
        if mh().truthy(stat) && oh().truthy(stat) {
            // L2130
            let mainChance = at_extra(&mh()).num() * mh().num(n!("HitChance"));
            // L2131
            let offChance = at_extra(&oh()).num() * oh().num(n!("HitChance"));
            // L2132
            let mainPortion = mainChance / (mainChance + offChance);
            // L2133
            let offPortion = offChance / (mainChance + offChance);
            // L2134
            let maxInstance = m_max(mh().num(stat), oh().num(stat));
            // L2135
            let minInstance = m_min(mh().num(stat), oh().num(stat));
            // L2136
            let stackName = cat!(gsub_plain(stat.as_str(), "DPS", ""), "Stacks");
            // L2137
            let globalOutput = match OFFENCE_STATE.with(|s| s.globalOutput.get()) {
                Some(g) => g,
                None => lua_error!("attempt to index upvalue 'globalOutput' (a nil value)"),
            };
            let maxInstanceStacks = m_min(
                1.0,
                globalOutput.num_or(stackName, 1.0)
                    / globalOutput.num_or(cat!(stackName, "Max"), 1.0),
            );
            // L2138
            let v = maxInstance * maxInstanceStacks + minInstance * (1.0 - maxInstanceStacks);
            output.set(stat, v);
            // L2139
            if let Some(breakdown) = breakdown {
                // L2140
                if !breakdown.truthy(stat) {
                    breakdown.set(stat, Table::new());
                }
                // L2141
                insert_at(breakdown, stat, lfmt!(""));
                // L2142
                insert_at(
                    breakdown,
                    stat,
                    lfmt!(
                        "%.2f%% of ailment stacks use maximum damage",
                        maxInstanceStacks * 100.0
                    ),
                );
                // L2143
                insert_at(
                    breakdown,
                    stat,
                    lfmt!(
                        "Max Damage comes from %s",
                        if mh().num(stat) >= oh().num(stat) {
                            "Main Hand"
                        } else {
                            "Off Hand"
                        }
                    ),
                );
                // L2144
                insert_at(
                    breakdown,
                    stat,
                    lfmt!("= %.1f", maxInstance * maxInstanceStacks),
                );
                // L2145
                if maxInstanceStacks < 1.0 {
                    // L2146
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!(
                            "%.2f%% of ailment stacks use non-maximum damage",
                            (1.0 - maxInstanceStacks) * 100.0
                        ),
                    );
                    // L2147
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!("= %.1f", minInstance * (1.0 - maxInstanceStacks)),
                    );
                }
                // L2149
                insert_at(breakdown, stat, "");
                // L2150
                insert_at(breakdown, stat, "Total:");
                // L2151
                if maxInstanceStacks < 1.0 {
                    // L2152
                    insert_at(
                        breakdown,
                        stat,
                        lfmt!(
                            "%.1f + %.1f",
                            maxInstance * maxInstanceStacks,
                            minInstance * (1.0 - maxInstanceStacks)
                        ),
                    );
                }
                // L2154
                insert_at(breakdown, stat, lfmt!("= %.1f", output.num(stat)));
            }
        } else {
            // L2157
            let v = lor(mh().get(stat), || oh().get(stat));
            output.set(stat, v);
            // L2158
            if let Some(breakdown) = breakdown {
                // L2159
                if !breakdown.truthy(stat) {
                    breakdown.set(stat, Table::new());
                }
                // L2160
                insert_at(
                    breakdown,
                    stat,
                    lfmt!(
                        "All ailment stacks comes from %s",
                        if mh().truthy(stat) {
                            "Main Hand"
                        } else {
                            "Off Hand"
                        }
                    ),
                );
            }
        }
    } else if mode == n!("DPS") {
        // L2164
        let v = mh().num0(stat) + oh().num0(stat);
        output.set(stat, v);
        // L2165
        if !skillData.truthy(n!("doubleHitsWhenDualWielding")) {
            // L2166
            let v = output.num(stat) / 2.0;
            output.set(stat, v);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn list(mods: &[(&str, f64)]) -> ModList {
        let l = ModStore::ModList(None);
        for (name, v) in mods {
            l.NewMod(Name::from(*name), ModType::BASE, *v, "test", 0, 0, &[]);
        }
        l
    }

    fn get(t: &Table, path: &[&str]) -> f64 {
        let mut cur = Val::Table(t.clone());
        for p in path {
            cur = cur.table().expect("table").get(Name::from(*p));
        }
        cur.num()
    }

    /// buildConversionTable (1938-1987): global conversion scaled down to the room skill
    /// conversion leaves, elemental / non-chaos aliases, gains, the whole-percent rounding.
    #[test]
    fn conversion_table_scaling_aliases_gains() {
        let l = list(&[
            ("PhysicalDamageConvertToFire", 60.0),
            ("SkillPhysicalDamageConvertToCold", 50.0),
            ("PhysicalDamageGainAsChaos", 20.0),
            ("ElementalDamageConvertToChaos", 30.0),
            ("NonChaosDamageGainAsFire", 10.0),
        ]);
        let t = buildConversionTable(&l, None);
        // Physical: global 60 + skill 50 > 100 -> global scaled by (100 - 50) / 60
        let fire = round(60.0 * ((100.0 - 50.0) / 60.0) + 0.0, None);
        assert_eq!(get(&t, &["Physical", "conversion", "Fire"]), fire / 100.0);
        assert_eq!(get(&t, &["Physical", "conversion", "Cold"]), 50.0 / 100.0);
        assert_eq!(get(&t, &["Physical", "conversion", "Lightning"]), 0.0);
        assert_eq!(get(&t, &["Physical", "gain", "Chaos"]), 20.0 / 100.0);
        assert_eq!(get(&t, &["Physical", "gain", "Fire"]), 10.0 / 100.0);
        assert_eq!(get(&t, &["Physical", "Fire"]), (fire + 10.0) / 100.0);
        assert_eq!(get(&t, &["Physical", "Chaos"]), (0.0 + 20.0) / 100.0);
        assert_eq!(
            get(&t, &["Physical", "mult"]),
            1.0 - m_min((0.0 + 50.0 + fire + 0.0) / 100.0, 1.0)
        );
        // Lightning / Cold / Fire: ElementalDamageConvertToChaos; NonChaos gains
        for dt in ["Lightning", "Cold", "Fire"] {
            assert_eq!(get(&t, &[dt, "conversion", "Chaos"]), 0.3, "{dt}");
            assert_eq!(
                get(&t, &[dt, "mult"]),
                1.0 - m_min(30.0 / 100.0, 1.0),
                "{dt}"
            );
        }
        assert_eq!(get(&t, &["Lightning", "Fire"]), (0.0 + 10.0) / 100.0);
        // Chaos converts to nothing
        assert_eq!(get(&t, &["Chaos", "mult"]), 1.0);
        assert!(t.table(n!("Chaos")).unwrap().get(n!("conversion")).is_nil());
    }

    #[test]
    fn conversion_table_skill_over_100_drops_global() {
        let l = list(&[
            ("SkillFireDamageConvertToChaos", 150.0),
            ("FireDamageConvertToChaos", 40.0),
        ]);
        let t = buildConversionTable(&l, None);
        assert_eq!(get(&t, &["Fire", "conversion", "Chaos"]), 1.0);
        assert_eq!(get(&t, &["Fire", "mult"]), 0.0);
    }

    #[test]
    fn conversion_table_rounds_each_destination() {
        let l = list(&[
            ("PhysicalDamageConvertToLightning", 33.3),
            ("PhysicalDamageConvertToCold", 33.3),
            ("PhysicalDamageConvertToFire", 33.4),
        ]);
        let t = buildConversionTable(&l, None);
        for dt in ["Lightning", "Cold", "Fire"] {
            assert_eq!(get(&t, &["Physical", "conversion", dt]), 33.0 / 100.0);
        }
        assert_eq!(
            get(&t, &["Physical", "mult"]),
            1.0 - m_min((33.0 + 33.0 + 33.0 + 0.0) / 100.0, 1.0)
        );
    }
}
