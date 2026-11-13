//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! CalcOffence.lua 1-347 (module header, helpers) and segment OF-A 362-1036 (setup, incl. the
//! `return` at 371). The local functions `calcAreaOfEffect` (374), `calcResistForType` (494) and
//! `runSkillFunc` (509) cross segments, so they take the ctx (§7.2).
//!
//! Owner: OF-A.
//!
//! Contract notes (CCR-0041): `calcRadiusBreakpoints` keeps its frozen `f64` signature (NaN for
//! each Lua nil) and `calcRadiusBreakpointsOpt` returns the `Option`s (Lua returns four nils
//! when the radius is 0; `breakdown.area` takes them as such, CCR-0091);
//! `setMoltenStrikeTertiaryRadiusBreakdown` takes `label: Option<&str>`; `runSkillFunc` offers
//! each call to the probe build's skill-hook handler first (`pob_calc::probe`, feature `probe`),
//! which the L4 tests use to replay the recorded hook deltas.
#![allow(non_snake_case)]
// helpers and module constants used by the later OF segments
#![allow(dead_code)]
// literal Lua statement shapes (`x = x * y`, `x = x + 1`)
#![allow(
    clippy::assign_op_pattern,
    clippy::float_cmp,
    clippy::neg_cmp_op_on_partial_ord
)]

use std::cell::RefCell;
use std::collections::HashMap;
use std::sync::LazyLock;

use super::OffenceCtx;
use crate::data_fns::{self, SkillFuncImpl, SkillHook};
use crate::modules::calc_perform_shared::actionSpeedMod;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::mod_tools;

// @pob Modules/CalcOffence.lua:7-67 h=3d397894
// (module-level statements: calcs,pairs,ipairs,unpack,t_insert,t_remove,m_abs,m_floor,m_ceil,m_min,m_max,m_sqrt,m_pow,m_huge,bor…)
//
// `tempTable1..3` (27-29) are scratch tables of buildConversionTable (OF-C, module_state.rs
// docs); `globalOutput`/`globalBreakdown` (65/67) live in `OFFENCE_STATE`;
// `ailmentTypeList`/`elementalAilmentTypeList` (48/50) are `data().ailmentTypeList` /
// `data().elementalAilmentTypeList` (immutable data, read where used).

/// `isElemental[damageType]` (L31: `{ Fire = true, Cold = true, Lightning = true }`).
pub(crate) fn isElemental(damageType: Name) -> bool {
    matches!(damageType.as_str(), "Fire" | "Cold" | "Lightning")
}

/// L34: List of all damage types, ordered according to the conversion sequence
pub(crate) static dmgTypeList: LazyLock<[Name; 5]> = LazyLock::new(|| {
    [
        n!("Physical"),
        n!("Lightning"),
        n!("Cold"),
        n!("Fire"),
        n!("Chaos"),
    ]
});

/// L36 `dmgTypeFlags.order`.
pub(crate) static dmgTypeFlags_order: LazyLock<[Name; 6]> = LazyLock::new(|| {
    [
        n!("Physical"),
        n!("Lightning"),
        n!("Cold"),
        n!("Fire"),
        n!("Elemental"),
        n!("Chaos"),
    ]
});

/// L37-44 `dmgTypeFlags.flags[damageType]` (nil for any other key).
pub(crate) fn dmgTypeFlags_flags(damageType: Name) -> Option<u32> {
    match damageType.as_str() {
        "Physical" => Some(0x01),
        "Lightning" => Some(0x02),
        "Cold" => Some(0x04),
        "Fire" => Some(0x08),
        "Elemental" => Some(0x0E),
        "Chaos" => Some(0x10),
        _ => None,
    }
}

thread_local! {
    /// L53-63 `damageStatsForTypes`: the `__index` memo, keyed by the flag set
    /// (contracts.md#index-meta; a per-thread memo, like the one Lua VM).
    static DAMAGE_STATS_FOR_TYPES: RefCell<HashMap<u32, Rc<[Name]>>> = RefCell::new(HashMap::new());
}

/// L52 Magic table for caching the modifier name sets used in calcDamage()
pub(crate) fn damageStatsForTypes(k: u32) -> Rc<[Name]> {
    if let Some(v) = DAMAGE_STATS_FOR_TYPES.with(|m| m.borrow().get(&k).cloned()) {
        return v;
    }
    // L54
    let mut modNames: Vec<Name> = vec![n!("Damage")];
    // L55
    for r#type in dmgTypeFlags_order.iter().copied() {
        // L56
        let flag = dmgTypeFlags_flags(r#type).unwrap_or(0);
        // L57
        if k & flag != 0 {
            // L58
            modNames.push(cat!(r#type, "Damage"));
        }
    }
    // L61
    let modNames: Rc<[Name]> = modNames.into();
    DAMAGE_STATS_FOR_TYPES.with(|m| m.borrow_mut().insert(k, modNames.clone()));
    // L62
    modNames
}

/// `conversionTable[a][b]` as a number (nil raises, as the comparison/arithmetic would).
fn conv_get(t: &Table, a: Name) -> Table {
    t.table(a)
        .unwrap_or_else(|| lua_error!("attempt to index field '{a}' (a nil value)"))
}

/// `round(x)` of Common.lua.
fn round0(x: f64) -> f64 {
    round(x, None)
}

/// `tostring(x)` as Lua's `..` writes a number.
fn tos(x: f64) -> String {
    tostring_num(x)
}

/// `a .. b` where `a`/`b` are Lua values (numbers or strings; anything else raises).
fn concat_val(v: &Val) -> String {
    match v {
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// Calculate min/max damage for the given damage type
/// @pob Modules/CalcOffence.lua:70-141 h=7343ee16
/// → (min, max).
pub fn calcDamage(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    cfg: &Cfg,
    breakdown: Option<&Table>,
    damageType: Name,
    typeFlags: u32,
    convDst: Option<Name>,
) -> (f64, f64) {
    // L71
    let skillModList = activeSkill.skillModList.get();

    // L73
    let typeFlags = typeFlags
        | dmgTypeFlags_flags(damageType)
            .unwrap_or_else(|| lua_error!("bad argument #2 to 'bor' (number expected, got nil)"));

    // L75-77 Calculate conversions
    let mut addMin = 0.0;
    let mut addMax = 0.0;
    let conversionTable = cfg
        .conversionTable
        .get()
        .or_else(|| activeSkill.conversionTable.get())
        .unwrap_or_else(|| lua_error!("attempt to index local 'conversionTable' (a nil value)"));
    // L78
    for otherType in dmgTypeList.iter().copied() {
        // L79
        if otherType == damageType {
            // L80 Damage can only be converted from damage types that precede this one in the conversion sequence, so stop here
            break;
        }
        // L83
        let convMult = conv_get(&conversionTable, otherType).num(damageType);
        // L84
        if convMult > 0.0 {
            // L85 Damage is being converted/gained from the other damage type
            // L86
            let (min, max) = calcDamage(
                activeSkill,
                output,
                cfg,
                breakdown,
                otherType,
                typeFlags,
                Some(damageType),
            );
            // L87
            addMin = addMin + min * convMult;
            // L88
            addMax = addMax + max * convMult;
        }
    }
    // L91
    let baseMin = output.get(cat!(damageType, "MinBase"));
    // L92
    let baseMax = output.get(cat!(damageType, "MaxBase"));
    // L93
    if baseMin.raw_eq(&Val::Num(0.0)) && baseMax.raw_eq(&Val::Num(0.0)) {
        // L94 No base damage for this type, don't need to calculate modifiers
        // L95
        if let Some(breakdown) = breakdown {
            if addMin != 0.0 || addMax != 0.0 {
                // L96
                let row = Table::new();
                row.set(n!("source"), damageType);
                // L98 (the condition is the enclosing one: always true here)
                row.set(
                    n!("convSrc"),
                    if addMin != 0.0 || addMax != 0.0 {
                        Val::from(format!(
                            "{} to {}",
                            tos(round0(addMin)),
                            tos(round0(addMax))
                        ))
                    } else {
                        Val::Bool(false)
                    },
                );
                // L99
                row.set(
                    n!("total"),
                    format!("{} to {}", tos(round0(addMin)), tos(round0(addMax))),
                );
                // L100
                if let Some(convDst) = convDst {
                    let c = conv_get(&conv_get(&conversionTable, damageType), n!("conversion"))
                        .num(convDst);
                    row.set(
                        n!("convDst"),
                        lfmt!("%d%% to %s", c * 100.0, convDst.as_str()),
                    );
                }
                // L101
                if let Some(convDst) = convDst {
                    let g =
                        conv_get(&conv_get(&conversionTable, damageType), n!("gain")).num(convDst);
                    row.set(
                        n!("gainDst"),
                        lfmt!("%d%% gained as %s", g * 100.0, convDst.as_str()),
                    );
                }
                let damageTypes = breakdown.table(n!("damageTypes")).unwrap_or_else(|| {
                    lua_error!("bad argument #1 to 'insert' (table expected, got nil)")
                });
                damageTypes.push(row);
            }
        }
        // L104
        if convDst.is_some() {
            // L105
            return (addMin, addMax);
        }
        // L107
        return (round0(addMin), round0(addMax));
    }
    let baseMin_n = baseMin.num();
    let baseMax_n = baseMax.num();

    // L110 Combine modifiers
    let modNames = damageStatsForTypes(typeFlags);
    // L111
    let inc = 1.0 + skillModList.Sum(ModType::INC, Some(&**cfg), &modNames) / 100.0;
    // L112
    let more = skillModList.More(Some(&**cfg), &modNames);
    // L113
    let genericMoreMinDamage = skillModList.More(Some(&**cfg), nm!["MinDamage"]);
    // L114
    let genericMoreMaxDamage = skillModList.More(Some(&**cfg), nm!["MaxDamage"]);
    // L115
    let moreMinDamage = skillModList.More(Some(&**cfg), &[cat!("Min", damageType, "Damage")]);
    // L116
    let moreMaxDamage = skillModList.More(Some(&**cfg), &[cat!("Max", damageType, "Damage")]);
    // L117
    let incMinDamage = 1.0
        + skillModList.Sum(
            ModType::INC,
            Some(&**cfg),
            &[cat!("Min", damageType, "Damage")],
        ) / 100.0;
    // L118
    let incMaxDamage = 1.0
        + skillModList.Sum(
            ModType::INC,
            Some(&**cfg),
            &[cat!("Max", damageType, "Damage")],
        ) / 100.0;

    // L120
    if let Some(breakdown) = breakdown {
        // L121
        let row = Table::new();
        row.set(n!("source"), damageType);
        // L123
        row.set(
            n!("base"),
            format!("{} to {}", concat_val(&baseMin), concat_val(&baseMax)),
        );
        // L124
        row.set(
            n!("inc"),
            if inc != 1.0 {
                Val::from(format!("x {}", tos(inc)))
            } else {
                Val::Bool(false)
            },
        );
        // L125
        row.set(
            n!("more"),
            if more != 1.0 {
                Val::from(format!("x {}", tos(more)))
            } else {
                Val::Bool(false)
            },
        );
        // L126
        row.set(
            n!("convSrc"),
            if addMin != 0.0 || addMax != 0.0 {
                Val::from(format!(
                    "{} to {}",
                    tos(round0(addMin)),
                    tos(round0(addMax))
                ))
            } else {
                Val::Bool(false)
            },
        );
        // L127
        row.set(
            n!("total"),
            format!(
                "{} to {}",
                tos(round0(baseMin_n * inc * more + addMin)),
                tos(round0(baseMax_n * inc * more + addMax))
            ),
        );
        // L128
        if let Some(convDst) = convDst {
            let c =
                conv_get(&conv_get(&conversionTable, damageType), n!("conversion")).num(convDst);
            row.set(
                n!("convDst"),
                if c > 0.0 {
                    Val::from(lfmt!("%d%% to %s", c * 100.0, convDst.as_str()))
                } else {
                    Val::Bool(false)
                },
            );
        }
        // L129
        if let Some(convDst) = convDst {
            let g = conv_get(&conv_get(&conversionTable, damageType), n!("gain")).num(convDst);
            row.set(
                n!("gainDst"),
                if g > 0.0 {
                    Val::from(lfmt!("%d%% gained as %s", g * 100.0, convDst.as_str()))
                } else {
                    Val::Bool(false)
                },
            );
        }
        let damageTypes = breakdown
            .table(n!("damageTypes"))
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'insert' (table expected, got nil)"));
        damageTypes.push(row);
    }

    // L133
    let min =
        ((baseMin_n * inc * more) * genericMoreMinDamage + addMin) * moreMinDamage * incMinDamage;
    // L134
    let max =
        ((baseMax_n * inc * more) * genericMoreMaxDamage + addMax) * moreMaxDamage * incMaxDamage;
    // L135
    if convDst.is_some() {
        // L136 Converted paths stay fractional until they reach their final damage type.
        // L137
        return (min, max);
    }
    // L139
    (round0(min), round0(max))
}

/// @pob Modules/CalcOffence.lua:143-154 h=4efa567c
/// → (min, max).
pub fn calcAilmentSourceDamage(
    activeSkill: &ActiveSkillRef,
    output: &Output,
    cfg: &Cfg,
    breakdown: Option<&Table>,
    damageType: Name,
    typeFlags: u32,
) -> (f64, f64) {
    // L144
    let (min, max) = calcDamage(
        activeSkill,
        output,
        cfg,
        breakdown,
        damageType,
        typeFlags,
        None,
    );
    // L145
    let conversionTable = cfg
        .conversionTable
        .get()
        .or_else(|| activeSkill.conversionTable.get())
        .unwrap_or_else(|| lua_error!("attempt to index local 'conversionTable' (a nil value)"));
    // L146
    let convMult = conv_get(&conversionTable, damageType).num(n!("mult"));
    // L147
    if let Some(breakdown) = breakdown {
        if convMult != 1.0 {
            // L148
            breakdown.push("Source damage:");
            // L149
            breakdown.push(lfmt!("%d to %d ^8(total damage)", min, max));
            // L150
            breakdown.push(lfmt!(
                "x %g ^8(%g%% converted to other damage types)",
                convMult,
                (1.0 - convMult) * 100.0
            ));
            // L151
            breakdown.push(lfmt!("= %d to %d", min * convMult, max * convMult));
        }
    }
    // L153
    (min * convMult, max * convMult)
}

/// Calculates the area percentage after the client integer rounding stages.
/// @pob Modules/CalcOffence.lua:160-166 h=9488e182
pub fn calcAreaMod(incArea: f64, moreArea: f64) -> f64 {
    // L161
    let baseAreaPercent = roundSymmetric(incArea * 100.0, None);
    // L162
    let finalAreaPercent = roundSymmetric(m_max(0.0, moreArea) * 100.0, None);
    // L163
    let mut areaPercent = baseAreaPercent * finalAreaPercent / 100.0;
    // L164
    areaPercent = if areaPercent >= 0.0 {
        m_floor(areaPercent)
    } else {
        m_ceil(areaPercent)
    };
    // L165
    m_max(0.0, areaPercent) / 100.0
}

/// Calculates skill radius
/// @pob Modules/CalcOffence.lua:173-178 h=c17d4edb
pub fn calcRadius(baseRadius: f64, areaMod: f64, radiusMod: Option<f64>) -> f64 {
    // L174
    let mut radiusPercent = m_floor(100.0 * m_sqrt(areaMod));
    // L175 Final radius modifiers apply after Area of Effect has been converted to radius.
    // L176
    radiusPercent = m_floor(radiusPercent * radiusMod.unwrap_or(1.0));
    // L177
    m_floor(baseRadius * radiusPercent / 100.0)
}

/// Calculates the tertiary radius for Molten Strike, correctly handling the deadzone.
/// @pob Modules/CalcOffence.lua:185-190 h=266f4623
pub fn calcMoltenStrikeTertiaryRadius(
    baseRadius: f64,
    deadzoneRadius: f64,
    areaMod: f64,
    speedMod: f64,
) -> f64 {
    // L186 For now, we assume that PoE only rounds at the end.
    // L187
    let maxDistIgnoringSpeed = m_sqrt(
        baseRadius * baseRadius * areaMod - deadzoneRadius * deadzoneRadius * (areaMod - 1.0),
    );
    // L188
    // L189
    m_floor((maxDistIgnoringSpeed - deadzoneRadius) * speedMod + deadzoneRadius)
}

/// `calcRadiusBreakpoints` with the frozen U0 signature (OF-B's callers use it): each Lua nil
/// (a radius that is not positive) comes back as NaN. CCR-0041 §1 / CCR-0043 propose
/// returning the `Option`s of [`calcRadiusBreakpointsOpt`] instead; OF-A's own callers use
/// that one.
pub fn calcRadiusBreakpoints(
    baseRadius: f64,
    incArea: f64,
    moreArea: f64,
    radiusMod: Option<f64>,
) -> (f64, f64, f64, f64) {
    // CCR-0041
    let (inc, more, red, less) = calcRadiusBreakpointsOpt(baseRadius, incArea, moreArea, radiusMod);
    let nil = |x: Option<f64>| x.unwrap_or(f64::NAN);
    (nil(inc), nil(more), nil(red), nil(less))
}

/// Calculates modifiers needed to reach the next and previous radius breakpoints
/// @pob Modules/CalcOffence.lua:198-224 h=748821af
/// → (incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint); all nil
/// when the radius is not positive (CCR-0041 §1: the frozen signature returns plain numbers,
/// see [`calcRadiusBreakpoints`]).
pub fn calcRadiusBreakpointsOpt(
    baseRadius: f64,
    incArea: f64,
    moreArea: f64,
    radiusMod: Option<f64>,
) -> (Option<f64>, Option<f64>, Option<f64>, Option<f64>) {
    // L199
    let radius = calcRadius(baseRadius, calcAreaMod(incArea, moreArea), radiusMod);
    // L200
    let mut incAreaBreakpoint: Option<f64> = None;
    let mut redAreaBreakpoint: Option<f64> = None;
    let mut moreAreaBreakpoint: Option<f64> = None;
    let mut lessAreaBreakpoint: Option<f64> = None;
    // L201
    if radius > 0.0 {
        // L202
        let mut bp = 0.0;
        // L203
        loop {
            // L204
            bp = bp + 1.0;
            // L205
            let newRadius = calcRadius(
                baseRadius,
                calcAreaMod(incArea + bp / 100.0, moreArea),
                radiusMod,
            );
            // L206
            if newRadius > radius {
                break;
            }
        }
        incAreaBreakpoint = Some(bp);
        // L207
        let mut bp = 0.0;
        // L208
        loop {
            // L209
            bp = bp + 1.0;
            // L210
            let newRadius = calcRadius(
                baseRadius,
                calcAreaMod(incArea - bp / 100.0, moreArea),
                radiusMod,
            );
            // L211
            if newRadius < radius {
                break;
            }
        }
        redAreaBreakpoint = Some(bp);
        // L212
        let mut bp = 0.0;
        // L213
        loop {
            // L214
            bp = bp + 1.0;
            // L215
            let newRadius = calcRadius(
                baseRadius,
                calcAreaMod(incArea, moreArea * (1.0 + bp / 100.0)),
                radiusMod,
            );
            // L216
            if newRadius > radius {
                break;
            }
        }
        moreAreaBreakpoint = Some(bp);
        // L217
        let mut bp = 0.0;
        // L218
        loop {
            // L219
            bp = bp + 1.0;
            // L220
            let newRadius = calcRadius(
                baseRadius,
                calcAreaMod(incArea, moreArea * (1.0 - bp / 100.0)),
                radiusMod,
            );
            // L221
            if newRadius < radius {
                break;
            }
        }
        lessAreaBreakpoint = Some(bp);
    }
    // L223
    (
        incAreaBreakpoint,
        moreAreaBreakpoint,
        redAreaBreakpoint,
        lessAreaBreakpoint,
    )
}

/// Computes and sets the breakdown for Molten Strike's tertiary radius.
/// @pob Modules/CalcOffence.lua:235-294 h=3529b74f
pub fn setMoltenStrikeTertiaryRadiusBreakdown(
    breakdown: &Table,
    deadzoneRadius: f64,
    baseRadius: f64,
    label: Option<&str>,
    incArea: f64,
    moreArea: f64,
    incSpd: f64,
    moreSpd: f64,
) {
    // L236-240 nil -> 1 (no multiplier): every caller passes numbers
    // L241-249 Helper that calculates the tertiary radius with incremental modifiers to the 4 relevant pools.
    // This helps declutter the code below.
    let calc = |extraIncAoePct: f64,
                extraMoreAoePct: f64,
                extraIncSpdPct: f64,
                extraMoreSpdPct: f64|
     -> (f64, f64, f64) {
        // L244
        let areaMod = round(
            round(
                (incArea + extraIncAoePct / 100.0) * moreArea * (1.0 + extraMoreAoePct / 100.0),
                Some(10),
            ),
            Some(2),
        );
        // L245
        let speedMod = round(
            round(
                (incSpd + extraIncSpdPct / 100.0) * moreSpd * (1.0 + extraMoreSpdPct / 100.0),
                Some(10),
            ),
            Some(2),
        );
        // L246
        let dist = calcMoltenStrikeTertiaryRadius(baseRadius, deadzoneRadius, areaMod, speedMod);
        // L247
        (dist, areaMod, speedMod)
    };
    // L249-250 Current settings.
    let (currentDist, currentAreaMod, currentSpeedMod) = calc(0.0, 0.0, 0.0, 0.0);
    // L251-255 Create the detailed breakdown. This includes:
    //  * the complete formula as an algebraic expression (ignoring rounding),
    //  * the final value,
    //  * breakpoints on the 4 modifier pools (increased vs. more crossed with aoe and projectile speed), and
    //  * the input variables for the algebraic expression.
    // L256
    let breakdownRadius = match breakdown.get(n!("AreaOfEffectRadiusTertiary")) {
        Val::Table(t) => t,
        v if v.truthy() => lua_error!(
            "attempt to index local 'breakdownRadius' (a {} value)",
            v.lua_type()
        ),
        _ => Table::new(),
    };
    // L257
    breakdown.set(n!("AreaOfEffectRadiusTertiary"), breakdownRadius.clone());
    // L258 (`t_insert(t, nil)` stores nil at `#t + 1`)
    breakdownRadius.push(label.map_or(Val::Nil, Val::from));
    // L259
    breakdownRadius.push(" = (sqrt(R*R*a - r*r*(a-1)) - r) * s + r");
    // L260
    breakdownRadius.push(lfmt!(" = %d", currentDist));
    // L261
    if currentDist > 0.0 {
        // L262-268 Helper for finding one tertiary radius breakpoint value. This is a little slower than what
        // we do in the generic calcRadiusBreakpoints, but this approach requires a lot less code and
        // should be more maintainable given that we need to search for 8 different breakpoints.
        let findBreakpoint = |sign: f64, argIdx: usize| -> f64 {
            // L268 starter args for the calc function
            let mut args = [0.0_f64; 4];
            // L269
            loop {
                // L270 increment or decrement the desired arg
                args[argIdx - 1] = args[argIdx - 1] + sign;
                // L271
                let (newDist, _, _) = calc(args[0], args[1], args[2], args[3]);
                // L272 stop once we've hit a new radius breakpoint
                if (newDist != currentDist) || (newDist == 0.0) {
                    break;
                }
            }
            // L273 remove the sign since we want all positive numbers
            args[argIdx - 1] * sign
        };
        // L275
        let (a, b) = (findBreakpoint(1.0, 1), findBreakpoint(1.0, 2));
        breakdownRadius.push(lfmt!(
            "^8Next AoE breakpoint: %d%% increased or %d%% more",
            a,
            b
        ));
        // L276
        let (a, b) = (findBreakpoint(1.0, 3), findBreakpoint(1.0, 4));
        breakdownRadius.push(lfmt!(
            "^8Next Proj. Speed breakpoint: %d%% increased or %d%% more",
            a,
            b
        ));
        // L277
        let (a, b) = (findBreakpoint(-1.0, 1), findBreakpoint(-1.0, 2));
        breakdownRadius.push(lfmt!(
            "^8Previous AoE breakpoint: %d%% increased or %d%% more",
            a,
            b
        ));
        // L278
        let (a, b) = (findBreakpoint(-1.0, 3), findBreakpoint(-1.0, 4));
        breakdownRadius.push(lfmt!(
            "^8Previous Proj. Speed breakpoint: %d%% increased or %d%% more",
            a,
            b
        ));
    }
    // L280-281 This is the input variable table.
    breakdownRadius.set(n!("label"), "Inputs");
    // L282
    breakdownRadius.set(n!("rowList"), Table::new());
    // L283-287
    let colList = Table::new();
    colList.push(tbl! { label = "Variable", key = "name" });
    colList.push(tbl! { label = "Value", key = "value" });
    colList.push(tbl! { label = "Description", key = "description" });
    breakdownRadius.set(n!("colList"), colList);
    let rowList = breakdownRadius
        .table(n!("rowList"))
        .unwrap_or_else(|| lua_error!("bad argument #1 to 'insert' (table expected, got nil)"));
    // L288
    rowList.push(tbl! { name = "r", value = lfmt!("%d", deadzoneRadius), description = "fixed deadzone radius" });
    // L289
    rowList.push(
        tbl! { name = "R", value = lfmt!("%d", baseRadius), description = "base outer radius" },
    );
    // L290
    rowList.push(tbl! { name = "a", value = lfmt!("%.2f", currentAreaMod), description = "net AoE multiplier (scales area)" });
    // L291
    rowList.push(tbl! { name = "s", value = lfmt!("%.2f", currentSpeedMod), description = "net projectile speed multiplier (scales range)" });
    // L292-293 Trigger the inclusion of the radius display.
    breakdownRadius.set(n!("radius"), currentDist);
}

/// @pob Modules/CalcOffence.lua:296-309 h=d3323536
/// Global `calcSkillCooldown` → (cooldown, rounded, addedCooldown).
pub fn calcSkillCooldown(
    skillModList: &ModList,
    skillCfg: Option<&ModCfg>,
    skillData: &Table,
) -> (f64, bool, Option<f64>) {
    // L297
    let cooldownOverride = skillModList.Override(skillCfg, nm!["CooldownRecovery"]);
    // L298
    let addedCooldown = skillModList.Sum(ModType::BASE, skillCfg, nm!["CooldownRecovery"]);
    // L299
    let mut cooldown = match Val::from_value(&cooldownOverride) {
        v if v.truthy() => v.num(),
        _ => {
            (skillData.num_or(n!("cooldown"), 0.0) + addedCooldown)
                / m_max(
                    0.0,
                    calcLib::r#mod(skillModList, skillCfg, nm!["CooldownRecovery"]),
                )
        }
    };
    // L300 If a skill can store extra uses and has a cooldown, it doesn't round the cooldown value to server ticks
    // L301
    let mut rounded = false;
    // L302
    if (skillData.truthy(n!("storedUses")) && skillData.num(n!("storedUses")) > 1.0)
        || (skillData.truthy(n!("VaalStoredUses")) && skillData.num(n!("VaalStoredUses")) > 1.0)
        || skillModList.Sum(ModType::BASE, skillCfg, nm!["AdditionalCooldownUses"]) > 0.0
    {
        // L303
        (cooldown, rounded, None)
    } else {
        // L305
        cooldown = m_ceil(cooldown * data().misc.ServerTickRate) / data().misc.ServerTickRate;
        // L306
        rounded = true;
        // L307
        (cooldown, rounded, Some(addedCooldown))
    }
}

/// @pob Modules/CalcOffence.lua:311-320 h=685a9ce0
pub fn calcWarcryCastTime(
    skillModList: &ModList,
    skillCfg: Option<&ModCfg>,
    skillData: &Table,
    actor: &ActorRef,
) -> f64 {
    // L312
    let baseSpeed = 1.0 / skillModList.Sum(ModType::BASE, skillCfg, nm!["WarcryCastTime"]);
    // L313
    let mut warcryCastTime = baseSpeed
        * calcLib::r#mod(skillModList, skillCfg, nm!["WarcrySpeed"])
        * actionSpeedMod(actor);
    // L314
    warcryCastTime = m_min(warcryCastTime, data().misc.ServerTickRate);
    // L315
    warcryCastTime = 1.0 / warcryCastTime;
    // L316
    if skillModList.Flag(skillCfg, nm!["InstantWarcry"])
        || skillData.truthy(n!("triggeredByAutoexertion"))
    {
        // L317
        warcryCastTime = 0.0;
    }
    // L319
    warcryCastTime
}

/// @pob Modules/CalcOffence.lua:322-335 h=84318871
/// Global `calcSkillDuration`.
///
/// `env = None` is a Lua table without `mode_effective` (Data/Skills/act_int.lua:21113 passes
/// `{}`); `enemyDB` is read only when `env.mode_effective` is true (CCR-0046).
pub fn calcSkillDuration(
    skillModList: &ModList,
    skillCfg: Option<&ModCfg>,
    skillData: &Table,
    env: Option<&EnvRef>,
    enemyDB: Option<&ModDB>,
) -> f64 {
    // L323
    let mut durationMod = calcLib::r#mod(
        skillModList,
        skillCfg,
        &names([
            Some(n!("Duration")),
            Some(n!("PrimaryDuration")),
            skillData
                .truthy(n!("mineDurationAppliesToSkill"))
                .then(|| n!("MineDuration")),
        ]),
    );
    // L324
    durationMod = m_max(durationMod, 0.0);
    // L325
    let durationBase = skillData.num_or(n!("duration"), 0.0)
        + skillModList.Sum(ModType::BASE, skillCfg, nm!["Duration", "PrimaryDuration"]);
    // L326
    let mut duration = durationBase * durationMod;
    // L327
    let mut debuffDurationMult = 1.0;
    // L328
    if env.is_some_and(|e| e.mode_effective.get()) {
        // L329 (a nil `enemyDB` raises inside calcLib.mod: `modStore:Sum`, CalcTools.lua:15)
        let enemyDB = enemyDB
            .unwrap_or_else(|| lua_error!("attempt to index local 'modStore' (a nil value)"));
        debuffDurationMult = 1.0
            / m_max(
                data().misc.BuffExpirationSlowCap,
                calcLib::r#mod(enemyDB, skillCfg, nm!["BuffExpireFaster"]),
            );
    }
    // L331
    if skillData.truthy(n!("debuff")) {
        // L332
        duration = duration * debuffDurationMult;
    }
    // L334
    duration
}

/// Keep defensive Totem pools in step with the Totem Life shown in the skill calculations.
/// @pob Modules/CalcOffence.lua:338-342 h=76364366
/// → (life, lifeMod).
pub fn calcTotemLife(env: &EnvRef, activeSkill: &ActiveSkillRef) -> (f64, f64) {
    // L339
    let skillModList = activeSkill.skillModList.get();
    let skillCfg = activeSkill.skillCfg.get();
    let lifeMod = calcLib::r#mod(&skillModList, Some(&*skillCfg), nm!["TotemLife"]);
    // L340
    let totemLevel = activeSkill.skillData.get().get(n!("totemLevel"));
    let allyLife = match totemLevel {
        Val::Num(l) if l >= 1.0 && l.fract() == 0.0 => {
            env.data.monsterAllyLifeTable.get(l as usize - 1).copied()
        }
        _ => None,
    }
    .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
    let totemMult = activeSkill
        .skillTotemId
        .get()
        .filter(|id| id.fract() == 0.0)
        .and_then(|id| env.data.totemLifeMult.get(&(id as i64)).copied())
        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
    let life = round0(m_floor(allyLife * totemMult) * lifeMod);
    // L341
    (life, lifeMod)
}

// ---- OF-A segment ------------------------------------------------------------------------------

/// The value of a `{ mod = … }` / `{ key = …, value = … }` LIST payload as a calc value.
fn payload(v: &Value) -> Val {
    Val::from_value(v)
}

/// The `mod` of a `{ mod = … }` LIST payload.
fn payload_mod(v: &Value) -> ModRef {
    match payload(v).get(n!("mod")) {
        Val::Mod(m) => m,
        other => lua_error!(
            "attempt to index field 'mod' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `mod.value` of a stored mod as a number (Tabulate rows of numeric types).
fn mod_num(m: &Mod) -> f64 {
    Val::from_value(&m.value).num()
}

/// `store:NewMod(name, type, value, mod.source, flags, keywordFlags, tags…)` with the
/// source taken from another mod (nil when that mod has none, as Lua passes it on).
fn new_mod_src(
    store: &ModStore,
    name: Name,
    ty: ModType,
    value: impl Into<Value>,
    source: Option<&str>,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) {
    match source {
        Some(s) => store.NewMod(name, ty, value, s, flags, keywordFlags, tags),
        // ModStore.lua:110 `self:AddMod(mod_createMod(...))` with a nil source
        None => store.AddMod(Arc::new(mod_tools::createMod(
            name,
            ty,
            value,
            None,
            flags,
            keywordFlags,
            tags,
        ))),
    }
}

/// A Lua value passed as a mod value (`output.X` may be nil).
fn val_value(v: &Val) -> Value {
    match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(n) => Value::Num(*n),
        Val::Str(s) => Value::Str(AStr::from(&**s)),
        Val::Mod(m) => Value::Mod(m.clone()),
        Val::Data(d) => d.clone(),
        other => lua_error!(
            "a {} value cannot be stored as a mod value",
            other.lua_type()
        ),
    }
}

/// `actor.itemList[slot] and actor.itemList[slot].weaponData and actor.itemList[slot].weaponData[1]`.
fn slot_has_weapon(actor: &Actor, slotName: Name) -> bool {
    let item = actor.item_in(slotName);
    match item {
        Some(item) => match item.weaponData.get() {
            Some(wd) => wd.truthy(1_i64),
            None => false,
        },
        None => false,
    }
}

/// `t.type == s`.
fn type_is(t: &Table, s: &str) -> bool {
    t.get(n!("type")).raw_eq(&Val::Str(LStr::from(s)))
}

/// `env.configInput[k] == s`.
fn cfg_is(env: &Env, k: Name, s: &str) -> bool {
    env.configInput.get(k).raw_eq(&Val::Str(LStr::from(s)))
}

/// @pob Modules/CalcOffence.lua:362-1036 h=14e4320f
/// OF-A: CalcOffence.lua 362-1036 (`Flow::Return` at 371).
pub fn seg_setup(ctx: &mut OffenceCtx) -> Flow {
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
    let cfg = Some(&*skillCfg);

    // L362
    if skillData.truthy(n!("showAverage")) {
        // L363
        skillFlags.set(n!("showAverage"), true);
    } else {
        // L365
        skillFlags.set(n!("notAverage"), true);
    }

    // L368
    if skillFlags.truthy(n!("disable")) {
        // L369 Skill is disabled
        // L370
        output.set(n!("CombinedDPS"), 0.0);
        // L371
        return Flow::Return;
    }

    // L374-493 `calcAreaOfEffect`, L494-507 `calcResistForType`, L509-514 `runSkillFunc`:
    // the ctx-taking fns below.

    // L516
    runSkillFunc(ctx, SkillHook::initialFunc);

    // L518
    let skillCond = skillCfg
        .skillCond
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'skillCond' (a nil value)"));
    skillCond.set(n!("SkillIsTriggered"), skillData.get(n!("triggered")));
    // L519
    if skillCond.truthy(n!("SkillIsTriggered")) {
        // L520
        skillFlags.set(n!("triggered"), true);
    }
    // L522
    skillCond.set(
        n!("SkillIsFocused"),
        skillData.get(n!("chanceToTriggerOnFocus")),
    );
    // L523
    if skillCond.truthy(n!("SkillIsFocused")) {
        // L524
        skillFlags.set(n!("focused"), true);
    }

    // L527 Update skill data
    for value in skillModList.List(cfg, nm!["SkillData"]) {
        let value = payload(&value);
        let key = value.get(n!("key"));
        let key: Key = match key {
            Val::Str(s) => Key::S(Name::from(&s)),
            Val::Num(k) => Key::from(k),
            Val::Bool(b) => Key::B(b),
            other => lua_error!("table index is {}", other.lua_type()),
        };
        // L529
        if value.get(n!("merge")).raw_eq(&Val::Str(LStr::from("MAX"))) {
            // L530
            let v = m_max(value.get(n!("value")).num(), skillData.get(key).num_or(0.0));
            skillData.set(key, v);
        } else {
            // L532
            skillData.set(key, value.get(n!("value")));
        }
    }

    // L536 Add addition stat bonuses
    if skillModList.Flag(None, nm!["IronGrip"]) {
        // L538
        skillModList.NewMod(
            n!("PhysicalDamage"),
            ModType::INC,
            actor.strDmgBonus.get(),
            "Strength",
            ModFlag::Attack | ModFlag::Projectile,
            0,
            &[],
        );
    }
    // L540
    if skillModList.Flag(None, nm!["IronWill"]) {
        // L541
        skillModList.NewMod(
            n!("Damage"),
            ModType::INC,
            actor.strDmgBonus.get(),
            "Strength",
            ModFlag::Spell,
            0,
            &[],
        );
    }

    // L544
    if skillModList.Flag(None, nm!["TransfigurationOfBody"]) {
        // L545
        let v = m_floor(
            skillModList.Sum(ModType::INC, None, nm!["Life"]) * data().misc.Transfiguration,
        );
        skillModList.NewMod(
            n!("Damage"),
            ModType::INC,
            v,
            "Transfiguration of Body",
            ModFlag::Attack,
            0,
            &[],
        );
    }
    // L547
    if skillModList.Flag(None, nm!["TransfigurationOfMind"]) {
        // L548
        let v = m_floor(
            skillModList.Sum(ModType::INC, None, nm!["Mana"]) * data().misc.Transfiguration,
        );
        skillModList.NewMod(
            n!("Damage"),
            ModType::INC,
            v,
            "Transfiguration of Mind",
            0,
            0,
            &[],
        );
    }
    // L550
    if skillModList.Flag(None, nm!["TransfigurationOfSoul"]) {
        // L551
        let v = m_floor(
            skillModList.Sum(ModType::INC, None, nm!["EnergyShield"]) * data().misc.Transfiguration,
        );
        skillModList.NewMod(
            n!("Damage"),
            ModType::INC,
            v,
            "Transfiguration of Soul",
            ModFlag::Spell,
            0,
            &[],
        );
    }

    // L554
    if modDB.Flag(None, nm!["Elusive"]) && skillModList.Flag(None, nm!["SupportedByNightblade"]) {
        // L555
        let elusiveEffect = output.num(n!("ElusiveEffectMod")) / 100.0;
        // L556
        let nightbladeMulti =
            skillModList.Sum(ModType::BASE, None, nm!["NightbladeElusiveCritMultiplier"]);
        // L557
        skillModList.NewMod(
            n!("CritMultiplier"),
            ModType::BASE,
            m_floor(nightbladeMulti * elusiveEffect),
            "Nightblade",
            0,
            0,
            &[],
        );
    }

    // L560 set other limits
    let v = skillModList.Sum(ModType::BASE, cfg, nm!["ActiveTrapLimit"]);
    output.set(n!("ActiveTrapLimit"), v);
    // L562
    let v = skillModList.Sum(ModType::BASE, cfg, nm!["ActiveMineLimit"]);
    output.set(n!("ActiveMineLimit"), v);

    // L564 set flask scaling
    let itemModDB = env.itemModDB.get();
    // L565
    output.set(
        n!("LifeFlaskRecovery"),
        itemModDB.multipliers.get(n!("LifeFlaskRecovery")),
    );
    // L566
    output.set(
        n!("LifeFlaskCharges"),
        itemModDB.multipliers.get(n!("LifeFlaskCharges")),
    );

    // L568
    if modDB.conditions.truthy(n!("AffectedByEnergyBlade")) {
        // L569
        let dmgMod = calcLib::r#mod(&skillModList, cfg, nm!["EnergyBladeDamage"]);
        // L570
        let speedMod = calcLib::r#mod(&skillModList, cfg, nm!["EnergyBladeAttackSpeed"]);
        // L571 LUA-ORDER: `pairs` over a two-entry string-keyed literal; the iterations write
        // disjoint actor fields, so the order is not observable.
        for (slotName, weaponData) in [
            (n!("Weapon 1"), &actor.weaponData1),
            (n!("Weapon 2"), &actor.weaponData2),
        ] {
            let wd = weaponData.get();
            // L572
            let base = wd
                .str(n!("name"))
                .and_then(|name| data().itemBases.get(&*name).cloned());
            if let Some(base) =
                base.filter(|_| slot_has_weapon(&actor, slotName) && wd.truthy(n!("name")))
            {
                // L573
                let weaponBaseData = base.weapon.unwrap_or_else(|| {
                    lua_error!("attempt to index local 'weaponBaseData' (a nil value)")
                });
                // L574
                wd.set(n!("CritChance"), weaponBaseData.CritChanceBase);
                // L575
                wd.set(n!("AttackRate"), weaponBaseData.AttackRateBase * speedMod);
                // L576
                wd.set(n!("Range"), weaponBaseData.Range);
                // L577
                for damageType in dmgTypeList.iter().copied() {
                    let baseMin = match damageType.as_str() {
                        "Physical" => weaponBaseData.PhysicalMin,
                        _ => 0.0,
                    };
                    let baseMax = match damageType.as_str() {
                        "Physical" => weaponBaseData.PhysicalMax,
                        _ => 0.0,
                    };
                    // L578
                    let v = baseMin
                        + m_floor(
                            skillModList.Sum(
                                ModType::BASE,
                                cfg,
                                &[cat!("EnergyBladeMin", damageType)],
                            ) * dmgMod,
                        );
                    wd.set(cat!(damageType, "Min"), v);
                    // L579
                    let v = baseMax
                        + m_floor(
                            skillModList.Sum(
                                ModType::BASE,
                                cfg,
                                &[cat!("EnergyBladeMax", damageType)],
                            ) * dmgMod,
                        );
                    wd.set(cat!(damageType, "Max"), v);
                }
            }
        }
    }

    // L585 account for Battlemage
    // L586 Note: we check conditions of Main Hand weapon using actor.itemList as actor.weaponData1 is populated with unarmed values when no weapon slotted.
    // L587
    if skillModList.Flag(None, nm!["Battlemage"]) && slot_has_weapon(&actor, n!("Weapon 1")) {
        // L589
        let multiplier = skillModList
            .Max(cfg, nm!["MainHandWeaponDamageAppliesToSpells"])
            .unwrap_or(100.0)
            / 100.0;
        let weaponData1 = actor.weaponData1.get();
        // L590
        for damageType in dmgTypeList.iter().copied() {
            // L591
            skillModList.NewMod(
                cat!(damageType, "Min"),
                ModType::BASE,
                m_floor(weaponData1.num0(cat!(damageType, "Min")) * multiplier),
                "Battlemage",
                ModFlag::Spell,
                0,
                &[],
            );
            // L592
            skillModList.NewMod(
                cat!(damageType, "Max"),
                ModType::BASE,
                m_floor(weaponData1.num0(cat!(damageType, "Max")) * multiplier),
                "Battlemage",
                ModFlag::Spell,
                0,
                &[],
            );
        }
    }
    // L595
    let weapon1info = actor
        .weaponData1
        .get()
        .str(n!("type"))
        .and_then(|t| env.data.weaponTypeInfo.get(&Name::from(&t)));
    // L596
    let weapon2info = actor
        .weaponData2
        .get()
        .str(n!("type"))
        .and_then(|t| env.data.weaponTypeInfo.get(&Name::from(&t)));
    // L597 -- account for Spellblade
    // L598 Note: we check conditions of Main Hand weapon using actor.itemList as actor.weaponData1 is populated with unarmed values when no weapon slotted.
    // L599
    let spellbladeMulti = skillModList.Max(cfg, nm!["OneHandWeaponDamageAppliesToSpells"]);
    // L600
    let spellblade = spellbladeMulti.is_some() && slot_has_weapon(&actor, n!("Weapon 1")) && {
        let info = weapon1info
            .unwrap_or_else(|| lua_error!("attempt to index local 'weapon1info' (a nil value)"));
        info.melee && info.oneHand
    };
    if spellblade {
        let spellbladeMulti = spellbladeMulti.expect("checked");
        // L601
        let multiplier = spellbladeMulti / 100.0 * if weapon2info.is_some() { 0.6 } else { 1.0 };
        let weaponData1 = actor.weaponData1.get();
        // L602
        for damageType in dmgTypeList.iter().copied() {
            // L603
            skillModList.NewMod(
                cat!(damageType, "Min"),
                ModType::BASE,
                m_floor(weaponData1.num0(cat!(damageType, "Min")) * multiplier),
                "Spellblade Main Hand",
                ModFlag::Spell,
                0,
                &[],
            );
            // L604
            skillModList.NewMod(
                cat!(damageType, "Max"),
                ModType::BASE,
                m_floor(weaponData1.num0(cat!(damageType, "Max")) * multiplier),
                "Spellblade Main Hand",
                ModFlag::Spell,
                0,
                &[],
            );
        }
        // L606
        if weapon2info.is_some() {
            let weaponData2 = actor.weaponData2.get();
            // L607
            for damageType in dmgTypeList.iter().copied() {
                // L608
                skillModList.NewMod(
                    cat!(damageType, "Min"),
                    ModType::BASE,
                    m_floor(weaponData2.num0(cat!(damageType, "Min")) * multiplier),
                    "Spellblade Off Hand",
                    ModFlag::Spell,
                    0,
                    &[],
                );
                // L609
                skillModList.NewMod(
                    cat!(damageType, "Max"),
                    ModType::BASE,
                    m_floor(weaponData2.num0(cat!(damageType, "Max")) * multiplier),
                    "Spellblade Off Hand",
                    ModFlag::Spell,
                    0,
                    &[],
                );
            }
        }
    }
    // L614
    if skillModList.Flag(None, nm!["MinionDamageAppliesToPlayer"])
        || skillModList.Flag(cfg, nm!["MinionDamageAppliesToPlayer"])
    {
        // L615 Minion Damage conversion from Spiritual Aid and The Scourge
        // L616
        let multiplier = skillModList
            .Max(cfg, nm!["ImprovedMinionDamageAppliesToPlayer"])
            .unwrap_or(100.0)
            / 100.0;
        // L617
        for value in skillModList.List(cfg, nm!["MinionModifier"]) {
            let vmod = payload_mod(&value);
            // L618
            if vmod.name == n!("Damage") && vmod.r#type == ModType::INC {
                // L619
                let r#mod = vmod;
                // L620
                let modifiers = calcLib::getConvertedModTags(&r#mod, multiplier, true);
                // L621
                new_mod_src(
                    &skillModList,
                    n!("Damage"),
                    ModType::INC,
                    mod_num(&r#mod) * multiplier,
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &modifiers,
                );
            }
        }
    }
    // L625
    if skillModList.Flag(None, nm!["MinionAttackSpeedAppliesToPlayer"]) {
        // L626 Minion Attack Speed conversion from Spiritual Command
        // L627
        let multiplier = skillModList
            .Max(cfg, nm!["ImprovedMinionAttackSpeedAppliesToPlayer"])
            .unwrap_or(100.0)
            / 100.0;
        // L628
        for value in skillModList.List(cfg, nm!["MinionModifier"]) {
            let vmod = payload_mod(&value);
            // L629
            if vmod.name == n!("Speed")
                && vmod.r#type == ModType::INC
                && (vmod.flags == 0 || vmod.flags & ModFlag::Attack != 0)
            {
                // L630
                let modifiers = calcLib::getConvertedModTags(&vmod, multiplier, true);
                // L631
                new_mod_src(
                    &skillModList,
                    n!("Speed"),
                    ModType::INC,
                    mod_num(&vmod) * multiplier,
                    vmod.source.as_deref(),
                    ModFlag::Attack,
                    vmod.keywordFlags,
                    &modifiers,
                );
            }
        }
    }
    // L635
    if skillModList.Flag(None, nm!["MinionCastSpeedAppliesToPlayer"]) {
        // L636 Minion Cast Speed conversion from Spinehail
        // L637
        let multiplier = skillModList
            .Max(cfg, nm!["ImprovedMinionCastSpeedAppliesToPlayer"])
            .unwrap_or(100.0)
            / 100.0;
        // L638
        for value in skillModList.List(cfg, nm!["MinionModifier"]) {
            let vmod = payload_mod(&value);
            // L639
            if vmod.name == n!("Speed")
                && vmod.r#type == ModType::INC
                && (vmod.flags == 0 || vmod.flags & ModFlag::Cast != 0)
            {
                // L640
                let modifiers = calcLib::getConvertedModTags(&vmod, multiplier, true);
                // L641
                new_mod_src(
                    &skillModList,
                    n!("Speed"),
                    ModType::INC,
                    mod_num(&vmod) * multiplier,
                    vmod.source.as_deref(),
                    ModFlag::Cast,
                    vmod.keywordFlags,
                    &modifiers,
                );
            }
        }
    }
    // L645
    if skillModList.Flag(None, nm!["EvasionAppliesToSpellDamage"]) {
        // L646 The Unblinking Eye evasion rating to spell damage conversion
        // L647
        for value in
            skillModList.Tabulate(Some(ModType::INC), Some(&ModCfg::default()), nm!["Evasion"])
        {
            // L648
            let r#mod = &value.r#mod;
            // L649
            new_mod_src(
                &skillModList,
                n!("Damage"),
                r#mod.r#type,
                r#mod.value.clone(),
                r#mod.source.as_deref(),
                ModFlag::Spell,
                r#mod.keywordFlags,
                &r#mod.tags,
            );
        }
    }
    // L652
    if skillModList.Flag(None, nm!["SpellDamageAppliesToAttacks"])
        || skillModList.Flag(cfg, nm!["SpellDamageAppliesToAttacks"])
    {
        // L653 Spell Damage conversion from Crown of Eyes, Kinetic Bolt, and the Wandslinger notable
        // L654
        let multiplier = skillModList
            .Max(cfg, nm!["ImprovedSpellDamageAppliesToAttacks"])
            .unwrap_or(100.0)
            / 100.0;
        // L655
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(ModFlag::Spell, 0)),
            nm!["Damage"],
        ) {
            // L656
            let r#mod = &value.r#mod;
            // L657
            if r#mod.flags & ModFlag::Spell != 0 {
                // L658
                let modifiers = calcLib::getConvertedModTags(r#mod, multiplier, false);
                // L659
                new_mod_src(
                    &skillModList,
                    n!("Damage"),
                    ModType::INC,
                    mod_num(r#mod) * multiplier,
                    r#mod.source.as_deref(),
                    (r#mod.flags & !ModFlag::Spell) | ModFlag::Attack,
                    r#mod.keywordFlags,
                    &modifiers,
                );
                // L660 Prevent double-dipping from converted strength's damage bonus
                if r#mod.source.as_deref() == Some("Strength") {
                    // L661
                    skillModList.ReplaceMod(
                        n!("PhysicalDamage"),
                        ModType::INC,
                        0.0,
                        "Strength",
                        ModFlag::Melee,
                        0,
                        &[],
                    );
                    // L662
                    skillModList.ReplaceMod(
                        n!("PhysicalDamage"),
                        ModType::INC,
                        0.0,
                        "Strength",
                        ModFlag::Attack | ModFlag::Projectile,
                        0,
                        &[],
                    );
                }
            }
        }
    }
    // L667
    if skillModList.Flag(None, nm!["CastSpeedAppliesToAttacks"]) {
        // L668 Get all increases for this; assumption is that multiple sources would not stack, so find the max
        // L669
        let multiplier = skillModList
            .Max(cfg, nm!["ImprovedCastSpeedAppliesToAttacks"])
            .unwrap_or(100.0)
            / 100.0;
        // L670
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(ModFlag::Cast, 0)),
            nm!["Speed"],
        ) {
            // L671
            let r#mod = &value.r#mod;
            // L672-673 Add a new mod for all mods that are cast only
            // Replace this with a single mod for the sum?
            // L674
            if r#mod.flags & ModFlag::Cast != 0 {
                // L675
                let modifiers = calcLib::getConvertedModTags(r#mod, multiplier, false);
                // L676
                new_mod_src(
                    &skillModList,
                    n!("Speed"),
                    ModType::INC,
                    mod_num(r#mod) * multiplier,
                    r#mod.source.as_deref(),
                    (r#mod.flags & !ModFlag::Cast) | ModFlag::Attack,
                    r#mod.keywordFlags,
                    &modifiers,
                );
            }
        }
    }
    // L680
    if skillModList.Flag(None, nm!["ProjectileSpeedAppliesToBowDamage"]) {
        // L681 Bow mastery projectile speed to damage with bows conversion
        // L682
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&ModCfg::default()),
            nm!["ProjectileSpeed"],
        ) {
            // L683
            let r#mod = &value.r#mod;
            // L684
            new_mod_src(
                &skillModList,
                n!("Damage"),
                r#mod.r#type,
                r#mod.value.clone(),
                r#mod.source.as_deref(),
                ModFlag::Bow | ModFlag::Hit,
                r#mod.keywordFlags,
                &r#mod.tags,
            );
        }
    }
    // L687
    if skillModList.Flag(None, nm!["ClawDamageAppliesToUnarmed"]) {
        // L688 Claw Damage conversion from Rigwald's Curse
        // L689
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(
                ModFlag::Claw | ModFlag::Hit,
                KeywordFlag::Hit,
            )),
            nm!["Damage"],
        ) {
            // L690
            let r#mod = &value.r#mod;
            // L691
            if r#mod.flags & ModFlag::Claw != 0 {
                // L692
                new_mod_src(
                    &skillModList,
                    n!("Damage"),
                    r#mod.r#type,
                    r#mod.value.clone(),
                    r#mod.source.as_deref(),
                    (r#mod.flags & !ModFlag::Claw) | ModFlag::Unarmed | ModFlag::Melee,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
        }
    }
    // L696
    if skillModList.Flag(None, nm!["ClawAttackSpeedAppliesToUnarmed"]) {
        // L697 Claw Attack Speed conversion from Rigwald's Curse
        // L698
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(
                ModFlag::Claw | ModFlag::Attack | ModFlag::Hit,
                0,
            )),
            nm!["Speed"],
        ) {
            // L699
            let r#mod = &value.r#mod;
            // L700
            if r#mod.flags & ModFlag::Claw != 0 && r#mod.flags & ModFlag::Attack != 0 {
                // L701
                new_mod_src(
                    &skillModList,
                    n!("Speed"),
                    r#mod.r#type,
                    r#mod.value.clone(),
                    r#mod.source.as_deref(),
                    (r#mod.flags & !ModFlag::Claw) | ModFlag::Unarmed,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
        }
    }
    // L705
    if skillModList.Flag(None, nm!["ClawCritChanceAppliesToUnarmed"]) {
        // L706 Claw Crit Chance conversion from Rigwald's Curse
        // L707
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(ModFlag::Claw | ModFlag::Hit, 0)),
            nm!["CritChance"],
        ) {
            // L708
            let r#mod = &value.r#mod;
            // L709
            if r#mod.flags & ModFlag::Claw != 0 {
                // L710
                new_mod_src(
                    &skillModList,
                    n!("CritChance"),
                    r#mod.r#type,
                    r#mod.value.clone(),
                    r#mod.source.as_deref(),
                    (r#mod.flags & !ModFlag::Claw) | ModFlag::Unarmed,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
        }
    }
    // L714
    if skillModList.Flag(None, nm!["ClawCritChanceAppliesToMinions"]) {
        // L715 Claw Crit Chance conversion from Law of the Wilds
        // L716
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(ModFlag::Claw | ModFlag::Hit, 0)),
            nm!["CritChance"],
        ) {
            // L717
            let r#mod = &value.r#mod;
            // L718
            if r#mod.flags & ModFlag::Claw != 0 {
                // L719
                let minion = env
                    .minion
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'minion' (a nil value)"));
                new_mod_src(
                    &minion.modDB.get(),
                    n!("CritChance"),
                    r#mod.r#type,
                    r#mod.value.clone(),
                    r#mod.source.as_deref(),
                    0,
                    0,
                    &[],
                );
            }
        }
    }
    // L723
    if skillModList.Flag(None, nm!["ClawCritMultiplierAppliesToMinions"]) {
        // L724 Claw Crit Multi conversion from Law of the Wilds
        // L725
        for value in skillModList.Tabulate(
            Some(ModType::BASE),
            Some(&*ModCfg::with_flags(ModFlag::Claw | ModFlag::Hit, 0)),
            nm!["CritMultiplier"],
        ) {
            // L726
            let r#mod = &value.r#mod;
            // L727
            if r#mod.flags & ModFlag::Claw != 0 {
                // L728
                let minion = env
                    .minion
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'minion' (a nil value)"));
                new_mod_src(
                    &minion.modDB.get(),
                    n!("CritMultiplier"),
                    r#mod.r#type,
                    r#mod.value.clone(),
                    r#mod.source.as_deref(),
                    0,
                    0,
                    &[],
                );
            }
        }
    }
    // L732-779: one INC/BASE mod from the first FLAG mod's source.
    for (flag, name, ty, outKey) in [
        // L732
        (
            n!("CritChanceIncreasedByUncappedLightningRes"),
            n!("CritChance"),
            ModType::INC,
            n!("LightningResistTotal"),
        ),
        // L739
        (
            n!("CritChanceIncreasedByLightningRes"),
            n!("CritChance"),
            ModType::INC,
            n!("LightningResist"),
        ),
        // L746
        (
            n!("CritChanceIncreasedByOvercappedLightningRes"),
            n!("CritChance"),
            ModType::INC,
            n!("LightningResistOverCap"),
        ),
        // L753
        (
            n!("CritChanceIncreasedBySpellSuppressChance"),
            n!("CritChance"),
            ModType::INC,
            n!("SpellSuppressionChance"),
        ),
        // L760
        (
            n!("FirePenIncreasedByUncappedFireRes"),
            n!("FirePenetration"),
            ModType::BASE,
            n!("FireResistOverCap"),
        ),
    ] {
        if skillModList.Flag(None, &[flag]) {
            // L733/740/747/754/761
            #[allow(clippy::never_loop)]
            for value in modDB.Tabulate(Some(ModType::FLAG), None, &[flag]) {
                // L734
                let r#mod = &value.r#mod;
                // L735
                new_mod_src(
                    &skillModList,
                    name,
                    ty,
                    val_value(&output.get(outKey)),
                    r#mod.source.as_deref(),
                    0,
                    0,
                    &[],
                );
                // L736
                break;
            }
        }
    }
    // L767
    if skillModList.Flag(None, nm!["LightRadiusAppliesToAccuracy"]) {
        // L768 Light Radius conversion from Corona Solaris
        // L769
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&ModCfg::default()),
            nm!["LightRadius"],
        ) {
            // L770
            let r#mod = &value.r#mod;
            // L771
            new_mod_src(
                &skillModList,
                n!("Accuracy"),
                ModType::INC,
                r#mod.value.clone(),
                r#mod.source.as_deref(),
                r#mod.flags,
                r#mod.keywordFlags,
                &r#mod.tags,
            );
        }
    }
    // L774
    if skillModList.Flag(None, nm!["LightRadiusAppliesToAreaOfEffect"]) {
        // L775 Light Radius conversion from Wreath of Phrecia
        // L776
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&ModCfg::default()),
            nm!["LightRadius"],
        ) {
            // L777
            let r#mod = &value.r#mod;
            // L778
            new_mod_src(
                &skillModList,
                n!("AreaOfEffect"),
                ModType::INC,
                m_floor(mod_num(r#mod) / 2.0),
                r#mod.source.as_deref(),
                r#mod.flags,
                r#mod.keywordFlags,
                &r#mod.tags,
            );
        }
    }
    // L781
    if skillModList.Flag(None, nm!["LightRadiusAppliesToDamage"]) {
        // L782 Light Radius conversion from Wreath of Phrecia
        // L783
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&ModCfg::default()),
            nm!["LightRadius"],
        ) {
            // L784
            let r#mod = &value.r#mod;
            // L785
            new_mod_src(
                &skillModList,
                n!("Damage"),
                ModType::INC,
                r#mod.value.clone(),
                r#mod.source.as_deref(),
                r#mod.flags,
                r#mod.keywordFlags,
                &r#mod.tags,
            );
        }
    }
    // L788
    if skillModList.Flag(None, nm!["CastSpeedAppliesToTrapThrowingSpeed"]) {
        // L789 Cast Speed conversion from Slavedriver's Hand
        // L790
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(ModFlag::Cast, 0)),
            nm!["Speed"],
        ) {
            // L791
            let r#mod = &value.r#mod;
            // L792
            if r#mod.flags == 0 || r#mod.flags & ModFlag::Cast != 0 {
                // L793
                new_mod_src(
                    &skillModList,
                    n!("TrapThrowingSpeed"),
                    ModType::INC,
                    r#mod.value.clone(),
                    r#mod.source.as_deref(),
                    r#mod.flags & !ModFlag::Cast & !ModFlag::Attack,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
        }
    }
    // L797
    if skillData.truthy(n!("arrowSpeedAppliesToAreaOfEffect")) {
        // L798 Arrow Speed conversion for Galvanic Arrow
        // L799
        for value in skillModList.Tabulate(
            Some(ModType::INC),
            Some(&*ModCfg::with_flags(ModFlag::Bow, 0)),
            nm!["ProjectileSpeed"],
        ) {
            // L800
            let r#mod = &value.r#mod;
            // L801
            new_mod_src(
                &skillModList,
                n!("AreaOfEffect"),
                ModType::INC,
                r#mod.value.clone(),
                r#mod.source.as_deref(),
                r#mod.flags,
                r#mod.keywordFlags,
                &r#mod.tags,
            );
        }
    }
    // L804
    if skillModList.Flag(None, nm!["SequentialProjectiles"])
        && !skillModList.Flag(None, nm!["OneShotProj"])
        && !skillModList.Flag(None, nm!["NoAdditionalProjectiles"])
        && !skillModList.Flag(None, nm!["SingleProjectile"])
        && !skillModList.Flag(None, nm!["TriggeredBySnipe"])
    {
        // L805 Applies DPS multiplier based on projectile count
        // L806
        let v = skillModList.Sum(ModType::BASE, cfg, nm!["ProjectileCount"]);
        skillData.set(n!("dpsMultiplier"), v);
    }

    // L809-818 `repeatSkillTypesCheck` (local function, defined and used here)
    let repeatSkillTypesCheck = |activeSkillTypes: &SkillTypes| -> bool {
        // L810
        let excludeSkillTypes = [
            SkillType::Instant,
            SkillType::Channel,
            SkillType::Triggered,
            SkillType::Retaliation,
            SkillType::NonRepeatable,
        ];
        // L811
        for r#type in excludeSkillTypes {
            // L812
            if activeSkillTypes.with(|s| s.has(r#type)) {
                // L813
                return false;
            }
        }
        // L816
        !skillModList.Flag(None, nm!["CannotRepeat"])
            && activeSkillTypes.with(|s| s.has(SkillType::Attack) || s.has(SkillType::Spell))
    };
    // L818
    let v = 1.0
        + if repeatSkillTypesCheck(&activeSkill.skillTypes.get()) {
            skillModList.Sum(ModType::BASE, cfg, nm!["RepeatCount"])
        } else {
            0.0
        };
    output.set(n!("Repeats"), v);
    // L819
    if output.num(n!("Repeats")) > 1.0 {
        // L820
        output.set(n!("RepeatCount"), output.get(n!("Repeats")));
        // L821 handle all the multipliers from Repeats
        // L822
        if !cfg_is(&env, n!("repeatMode"), "NONE") {
            let average = || cfg_is(&env, n!("repeatMode"), "AVERAGE");
            // L823
            for value in
                skillModList.Tabulate(Some(ModType::INC), cfg, nm!["RepeatFinalAreaOfEffect"])
            {
                // L824
                let r#mod = &value.r#mod;
                // L825
                let mut modValue = mod_num(r#mod);
                // L826
                if average() {
                    // L827
                    modValue = modValue / output.num(n!("Repeats"));
                }
                // L829
                new_mod_src(
                    &skillModList,
                    n!("AreaOfEffect"),
                    ModType::INC,
                    modValue,
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
            // L831
            for value in
                skillModList.Tabulate(Some(ModType::INC), cfg, nm!["RepeatPerRepeatAreaOfEffect"])
            {
                // L832
                let r#mod = &value.r#mod;
                // L833
                let mut modValue = mod_num(r#mod) * (output.num(n!("Repeats")) - 1.0);
                // L834
                if average() {
                    // L835
                    modValue = modValue / 2.0;
                }
                // L837
                new_mod_src(
                    &skillModList,
                    n!("AreaOfEffect"),
                    ModType::INC,
                    modValue,
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
            // L839
            for value in skillModList.Tabulate(
                Some(ModType::BASE),
                cfg,
                nm!["RepeatFinalDoubleDamageChance"],
            ) {
                // L840
                let r#mod = &value.r#mod;
                // L841
                let mut modValue = mod_num(r#mod);
                // L842
                if average() {
                    // L843
                    modValue = modValue / output.num(n!("Repeats"));
                }
                // L845
                new_mod_src(
                    &skillModList,
                    n!("DoubleDamageChance"),
                    ModType::BASE,
                    modValue,
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
            // L847
            let mut DamageFinalMoreValueTotal = 1.0;
            // L848
            let mut DamageMoreValueTotal = 0.0;
            // L849
            for value in skillModList.Tabulate(Some(ModType::MORE), cfg, nm!["RepeatFinalDamage"]) {
                // L850
                let r#mod = &value.r#mod;
                // L851
                let mut modValue = mod_num(r#mod);
                // L852
                DamageFinalMoreValueTotal = DamageFinalMoreValueTotal * (1.0 + modValue / 100.0);
                // L853
                DamageMoreValueTotal = DamageMoreValueTotal + modValue;
                // L854
                if average() && !skillModList.Flag(None, nm!["OnlyFinalRepeat"]) {
                    // L855
                    modValue = modValue / output.num(n!("Repeats"));
                }
                // L857
                new_mod_src(
                    &skillModList,
                    n!("Damage"),
                    ModType::MORE,
                    modValue,
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }
            // L859
            for value in
                skillModList.Tabulate(Some(ModType::MORE), cfg, nm!["RepeatPerRepeatDamage"])
            {
                // L860
                let r#mod = &value.r#mod;
                // L861
                let mut modValue = mod_num(r#mod) * (output.num(n!("Repeats")) - 1.0);
                // L862
                if average() {
                    // L863
                    if DamageFinalMoreValueTotal != 1.0 {
                        // L864-866 sum from 0 to num Repeats the damage each one does, multiplied by the other repeat multipliers,
                        // divide the total by the average other repeat multipliers and divide by number of repeats
                        // eg greater echo with 20Q div echo is (100 + 130 + 160 + 190*1.6)/1.15/4 - 100 = 50.87% more damage
                        // L867
                        let repeats = output.num(n!("Repeats"));
                        let mv = mod_num(r#mod);
                        modValue = ((100.0 + mv * (repeats - 2.0) / 2.0) * (repeats - 1.0)
                            + (100.0 + mv * (output.num(n!("Repeats")) - 1.0))
                                * DamageFinalMoreValueTotal)
                            / (output.num(n!("Repeats")) + DamageMoreValueTotal / 100.0)
                            - 100.0;
                    } else {
                        // L869
                        modValue = modValue / 2.0;
                    }
                }
                // L872
                new_mod_src(
                    &skillModList,
                    n!("Damage"),
                    ModType::MORE,
                    modValue,
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &r#mod.tags,
                );
            }

            // L875
            let mut lastMod: Option<ModRef> = None;
            // L876
            DamageFinalMoreValueTotal = DamageMoreValueTotal;
            // L877
            for (count, word) in [(2.0, "One"), (3.0, "Two"), (4.0, "Three")] {
                // L878
                if count > output.num(n!("Repeats")) {
                    // L879
                    break;
                } else if average() {
                    // L881
                    for value in skillModList.Tabulate(
                        Some(ModType::MORE),
                        cfg,
                        &[cat!("Repeat", word, "Damage")],
                    ) {
                        // L882
                        DamageMoreValueTotal = DamageMoreValueTotal + mod_num(&value.r#mod);
                        // L883
                        lastMod = Some(value.r#mod.clone());
                    }
                } else if count == output.num(n!("Repeats")) {
                    // L886
                    for value in skillModList.Tabulate(
                        Some(ModType::MORE),
                        cfg,
                        &[cat!("Repeat", word, "Damage")],
                    ) {
                        // L887
                        let m = &value.r#mod;
                        new_mod_src(
                            &skillModList,
                            n!("Damage"),
                            ModType::MORE,
                            m.value.clone(),
                            m.source.as_deref(),
                            m.flags,
                            m.keywordFlags,
                            &m.tags,
                        );
                    }
                }
            }
            // L891
            if average() {
                // L892
                if let Some(lastMod) = &lastMod {
                    // L893
                    let repeats = output.num(n!("Repeats"));
                    new_mod_src(
                        &skillModList,
                        n!("Damage"),
                        ModType::MORE,
                        (DamageMoreValueTotal / repeats + 100.0)
                            / (1.0 + DamageFinalMoreValueTotal / output.num(n!("Repeats")) / 100.0)
                            - 100.0,
                        lastMod.source.as_deref(),
                        lastMod.flags,
                        lastMod.keywordFlags,
                        &lastMod.tags,
                    );
                }
            }
            // L896
            if skillModList.Flag(None, nm!["FinalRepeatSumsDamage"]) {
                // L897
                for value in
                    skillModList.Tabulate(Some(ModType::FLAG), cfg, nm!["FinalRepeatSumsDamage"])
                {
                    // L898
                    let m = &value.r#mod;
                    new_mod_src(
                        &skillModList,
                        n!("Damage"),
                        ModType::MORE,
                        (100.0 * output.num(n!("Repeats")) + DamageFinalMoreValueTotal)
                            / (1.0 + DamageFinalMoreValueTotal / 100.0)
                            - 100.0,
                        m.source.as_deref(),
                        m.flags,
                        m.keywordFlags,
                        &m.tags,
                    );
                }
            }
            // L901
            if skillFlags.truthy(n!("trap")) || skillFlags.truthy(n!("mine")) {
                // L902
                skillModList.NewMod(
                    n!("DPS"),
                    ModType::MORE,
                    (output.num(n!("Repeats")) - 1.0) * 100.0,
                    "Repeat Count",
                    0,
                    0,
                    &[],
                );
            }
        }
    }
    // L906
    if skillModList.Flag(None, nm!["WeaponPhysAppliesToSpells"]) {
        // L907 Phys from weapon to Spells from Runegraft
        // L908 (`Sum` always returns a number, so `or 100` never applies)
        let mult =
            skillModList.Sum(ModType::BASE, cfg, nm!["WeaponPhysAppliesToSpellsPercent"]) / 100.0;
        // L909 (`actor.weaponData1` is always a table)
        let weaponData1 = actor.weaponData1.get();
        // L910
        skillModList.NewMod(
            n!("PhysicalMin"),
            ModType::BASE,
            m_floor(weaponData1.num0(n!("PhysicalMin")) * mult),
            "Runegraft of the Spellbound",
            ModFlag::Spell,
            0,
            &[],
        );
        // L911
        skillModList.NewMod(
            n!("PhysicalMax"),
            ModType::BASE,
            m_floor(weaponData1.num0(n!("PhysicalMax")) * mult),
            "Runegraft of the Spellbound",
            ModFlag::Spell,
            0,
            &[],
        );
    }
    // L914
    if skillData.truthy(n!("gainPercentBaseWandDamageToSpells")) {
        // L915
        let mult = skillData.num(n!("gainPercentBaseWandDamageToSpells")) / 100.0;
        let weaponData1 = actor.weaponData1.get();
        let weaponData2 = actor.weaponData2.get();
        // L916
        if type_is(&weaponData1, "Wand") && type_is(&weaponData2, "Wand") {
            // L917
            for damageType in dmgTypeList.iter().copied() {
                // L918
                skillModList.NewMod(
                    cat!(damageType, "Min"),
                    ModType::BASE,
                    m_floor(
                        (weaponData1.num0(cat!(damageType, "Min"))
                            + weaponData2.num0(cat!(damageType, "Min")))
                            / 2.0
                            * mult,
                    ),
                    "Spellslinger",
                    ModFlag::Spell,
                    0,
                    &[],
                );
                // L919
                skillModList.NewMod(
                    cat!(damageType, "Max"),
                    ModType::BASE,
                    m_floor(
                        (weaponData1.num0(cat!(damageType, "Max"))
                            + weaponData2.num0(cat!(damageType, "Max")))
                            / 2.0
                            * mult,
                    ),
                    "Spellslinger",
                    ModFlag::Spell,
                    0,
                    &[],
                );
            }
        } else if type_is(&weaponData1, "Wand") {
            // L922
            for damageType in dmgTypeList.iter().copied() {
                // L923
                skillModList.NewMod(
                    cat!(damageType, "Min"),
                    ModType::BASE,
                    m_floor(weaponData1.num0(cat!(damageType, "Min")) * mult),
                    "Spellslinger",
                    ModFlag::Spell,
                    0,
                    &[],
                );
                // L924
                skillModList.NewMod(
                    cat!(damageType, "Max"),
                    ModType::BASE,
                    m_floor(weaponData1.num0(cat!(damageType, "Max")) * mult),
                    "Spellslinger",
                    ModFlag::Spell,
                    0,
                    &[],
                );
            }
        } else if type_is(&weaponData2, "Wand") {
            // L927
            for damageType in dmgTypeList.iter().copied() {
                // L928
                skillModList.NewMod(
                    cat!(damageType, "Min"),
                    ModType::BASE,
                    m_floor(weaponData2.num0(cat!(damageType, "Min")) * mult),
                    "Spellslinger",
                    ModFlag::Spell,
                    0,
                    &[],
                );
                // L929
                skillModList.NewMod(
                    cat!(damageType, "Max"),
                    ModType::BASE,
                    m_floor(weaponData2.num0(cat!(damageType, "Max")) * mult),
                    "Spellslinger",
                    ModFlag::Spell,
                    0,
                    &[],
                );
            }
        }
    }
    // L933
    if skillData.truthy(n!("gainPercentBaseDaggerDamageToSpells")) {
        let weaponData1 = actor.weaponData1.get();
        let weaponData2 = actor.weaponData2.get();
        // L934
        let weapon1IsDagger =
            weaponData1.truthy(n!("AddedUsingDagger")) || type_is(&weaponData1, "Dagger");
        // L935
        let weapon2IsDagger =
            weaponData2.truthy(n!("AddedUsingDagger")) || type_is(&weaponData2, "Dagger");
        // L936
        let mult = skillData.num(n!("gainPercentBaseDaggerDamageToSpells")) / 100.0
            * if weapon1IsDagger && weapon2IsDagger {
                0.5
            } else {
                1.0
            };
        // `(isDagger and weaponData[k] or 0)`
        let part = |isDagger: bool, wd: &Table, k: Name| -> f64 {
            if isDagger {
                wd.num0(k)
            } else {
                0.0
            }
        };
        // L937
        for damageType in dmgTypeList.iter().copied() {
            // L938
            let baseMin = part(weapon1IsDagger, &weaponData1, cat!(damageType, "Min"))
                + part(weapon2IsDagger, &weaponData2, cat!(damageType, "Min"));
            // L939
            let baseMax = part(weapon1IsDagger, &weaponData1, cat!(damageType, "Max"))
                + part(weapon2IsDagger, &weaponData2, cat!(damageType, "Max"));
            // L940
            skillModList.NewMod(
                cat!(damageType, "Min"),
                ModType::BASE,
                m_floor(baseMin * mult),
                "Blade Blast of Dagger Detonation",
                ModFlag::Spell,
                0,
                &[],
            );
            // L941
            skillModList.NewMod(
                cat!(damageType, "Max"),
                ModType::BASE,
                m_floor(baseMax * mult),
                "Blade Blast of Dagger Detonation",
                ModFlag::Spell,
                0,
                &[],
            );
        }
    }

    // L945
    if skillModList.Flag(None, nm!["HasSeals"])
        && activeSkill
            .skillTypes
            .get()
            .with(|s| s.has(SkillType::CanRapidFire))
        && !skillModList.Flag(None, nm!["NoRepeatBonuses"])
    {
        // L946 Applies DPS multiplier based on seals count
        // L947
        let v = skillModList.Sum(ModType::BASE, cfg, nm!["SealGainFrequency"])
            / calcLib::r#mod(&skillModList, cfg, nm!["SealGainFrequency"]);
        output.set(n!("SealCooldown"), v);
        // L948
        let v = skillModList.Sum(ModType::BASE, cfg, nm!["SealCount"]);
        output.set(n!("SealMax"), v);
        // L949
        output.set(n!("AverageBurstHits"), output.get(n!("SealMax")));
        // L950
        let v = output.num(n!("SealCooldown")) * output.num(n!("SealMax"));
        output.set(n!("TimeMaxSeals"), v);

        // L952
        if !skillData.truthy(n!("hitTimeOverride")) {
            let castTime = || {
                activeSkill
                    .activeEffect
                    .get()
                    .grantedEffect
                    .get()
                    .castTime
                    .unwrap_or_else(|| {
                        lua_error!(
                            "attempt to perform arithmetic on field 'castTime' (a nil value)"
                        )
                    })
            };
            let mainSkillData = || {
                env.player
                    .mainSkill
                    .get()
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index field 'mainSkill' (a nil value)")
                    })
                    .skillData
                    .get()
            };
            // L953
            if skillModList.Flag(None, nm!["UseMaxUnleash"]) {
                // L954
                for value in skillModList.Tabulate(
                    Some(ModType::INC),
                    Some(&ModCfg::default()),
                    nm!["MaxSealCrit"],
                ) {
                    // L955
                    let r#mod = &value.r#mod;
                    // L956
                    new_mod_src(
                        &skillModList,
                        n!("CritChance"),
                        ModType::INC,
                        r#mod.value.clone(),
                        r#mod.source.as_deref(),
                        r#mod.flags,
                        r#mod.keywordFlags,
                        &r#mod.tags,
                    );
                }
                // L958
                for value in skillModList.Tabulate(
                    Some(ModType::MORE),
                    Some(&ModCfg::default()),
                    nm!["MaxSealDamage"],
                ) {
                    // L959
                    let r#mod = &value.r#mod;
                    // L960
                    new_mod_src(
                        &skillModList,
                        n!("Damage"),
                        ModType::MORE,
                        mod_num(r#mod) * output.num(n!("SealMax")),
                        r#mod.source.as_deref(),
                        r#mod.flags,
                        r#mod.keywordFlags,
                        &r#mod.tags,
                    );
                }
                // L962
                let v = 1.0
                    + output.num(n!("SealMax"))
                        * calcLib::r#mod(&skillModList, cfg, nm!["SealRepeatPenalty"]);
                mainSkillData().set(n!("dpsMultiplier"), v);
                // L963
                let v = m_max(
                    output.num(n!("TimeMaxSeals")),
                    1.0 / castTime()
                        * 1.1
                        * calcLib::r#mod(&skillModList, cfg, nm!["Speed"])
                        * output.num(n!("ActionSpeedMod")),
                );
                mainSkillData().set(n!("hitTimeOverride"), v);
            } else {
                // L965
                let v = 1.0
                    + 1.0
                        / output.num(n!("SealCooldown"))
                        / (1.0 / castTime()
                            * 1.1
                            * calcLib::r#mod(&skillModList, cfg, nm!["Speed"])
                            * output.num(n!("ActionSpeedMod")))
                        * calcLib::r#mod(&skillModList, cfg, nm!["SealRepeatPenalty"]);
                mainSkillData().set(n!("dpsMultiplier"), v);
            }
        }

        // L969
        if let Some(breakdown) = &breakdown {
            // L970
            let t = Table::new();
            breakdown.set(n!("SealGainTime"), t.clone());
            // L971-977
            let chain = Table::new();
            chain.set(n!("label"), "Gain frequency:");
            chain.set(
                n!("base"),
                tbl![
                    "%.2fs ^8(base gain frequency)",
                    skillModList.Sum(ModType::BASE, cfg, nm!["SealGainFrequency"])
                ],
            );
            chain.push(tbl![
                "%.2f ^8(increased/reduced gain frequency)",
                1.0 + skillModList.Sum(ModType::INC, cfg, nm!["SealGainFrequency"]) / 100.0
            ]);
            chain.push(tbl![
                "%.2f ^8(action speed modifier)",
                output.get(n!("ActionSpeedMod"))
            ]);
            chain.set(
                n!("total"),
                lfmt!("= %.2fs ^8per Seal", output.num(n!("SealCooldown"))),
            );
            breakdown.multiChain(&t, &chain);
        }
    }
    // L980
    let physMode: Val = match env.configInput.get(n!("physMode")) {
        v if v.truthy() => v,
        _ => Val::from("AVERAGE"),
    };
    let physModeIs = |s: &str| physMode.raw_eq(&Val::Str(LStr::from(s)));
    // L981 (mods by identity)
    let mut processedRandomMods: Vec<ModRef> = Vec::new();
    let processed = |l: &Vec<ModRef>, m: &ModRef| l.iter().any(|x| Arc::ptr_eq(x, m));
    // L982 `ipairs({ skillCfg, activeSkill.weapon1Cfg, activeSkill.weapon2Cfg })` stops at the first nil
    for cfg in [
        Some(skillCfg.clone()),
        activeSkill.weapon1Cfg.get(),
        activeSkill.weapon2Cfg.get(),
    ] {
        let Some(cfg) = cfg else { break };
        let cfg = Some(&*cfg);
        // L983
        if skillModList.Sum(
            ModType::BASE,
            cfg,
            nm![
                "PhysicalDamageGainAsRandom",
                "PhysicalDamageConvertToRandom",
                "PhysicalDamageGainAsColdOrLightning"
            ],
        ) > 0.0
        {
            // L984
            skillFlags.set(n!("randomPhys"), true);
            // L985
            for value in
                skillModList.Tabulate(Some(ModType::BASE), cfg, nm!["PhysicalDamageGainAsRandom"])
            {
                // L986
                let r#mod = &value.r#mod;
                // L987
                if !processed(&processedRandomMods, r#mod) {
                    // L988
                    processedRandomMods.push(r#mod.clone());
                    // L989
                    let effVal = mod_num(r#mod) / 3.0;
                    let add = |name: Name, v: Value| {
                        new_mod_src(
                            &skillModList,
                            name,
                            ModType::BASE,
                            v,
                            r#mod.source.as_deref(),
                            r#mod.flags,
                            r#mod.keywordFlags,
                            &r#mod.tags,
                        );
                    };
                    // L990
                    if physModeIs("AVERAGE") {
                        // L991
                        add(n!("PhysicalDamageGainAsFire"), Value::Num(effVal));
                        // L992
                        add(n!("PhysicalDamageGainAsCold"), Value::Num(effVal));
                        // L993
                        add(n!("PhysicalDamageGainAsLightning"), Value::Num(effVal));
                    } else if physModeIs("FIRE") {
                        // L995
                        add(n!("PhysicalDamageGainAsFire"), r#mod.value.clone());
                    } else if physModeIs("COLD") {
                        // L997
                        add(n!("PhysicalDamageGainAsCold"), r#mod.value.clone());
                    } else if physModeIs("LIGHTNING") {
                        // L999
                        add(n!("PhysicalDamageGainAsLightning"), r#mod.value.clone());
                    }
                }
            }
            // L1003
            for value in skillModList.Tabulate(
                Some(ModType::BASE),
                cfg,
                nm!["PhysicalDamageConvertToRandom"],
            ) {
                // L1004
                let r#mod = &value.r#mod;
                // L1005
                if !processed(&processedRandomMods, r#mod) {
                    // L1006
                    processedRandomMods.push(r#mod.clone());
                    // L1007
                    let effVal = mod_num(r#mod) / 3.0;
                    let add = |name: Name, v: Value| {
                        new_mod_src(
                            &skillModList,
                            name,
                            ModType::BASE,
                            v,
                            r#mod.source.as_deref(),
                            r#mod.flags,
                            r#mod.keywordFlags,
                            &r#mod.tags,
                        );
                    };
                    // L1008
                    if physModeIs("AVERAGE") {
                        // L1009
                        add(n!("PhysicalDamageConvertToFire"), Value::Num(effVal));
                        // L1010
                        add(n!("PhysicalDamageConvertToCold"), Value::Num(effVal));
                        // L1011
                        add(n!("PhysicalDamageConvertToLightning"), Value::Num(effVal));
                    } else if physModeIs("FIRE") {
                        // L1013
                        add(n!("PhysicalDamageConvertToFire"), r#mod.value.clone());
                    } else if physModeIs("COLD") {
                        // L1015
                        add(n!("PhysicalDamageConvertToCold"), r#mod.value.clone());
                    } else if physModeIs("LIGHTNING") {
                        // L1017
                        add(n!("PhysicalDamageConvertToLightning"), r#mod.value.clone());
                    }
                }
            }
            // L1021
            for value in skillModList.Tabulate(
                Some(ModType::BASE),
                cfg,
                nm!["PhysicalDamageGainAsColdOrLightning"],
            ) {
                // L1022
                let r#mod = &value.r#mod;
                // L1023
                if !processed(&processedRandomMods, r#mod) {
                    // L1024
                    processedRandomMods.push(r#mod.clone());
                    // L1025
                    let effVal = mod_num(r#mod) / 2.0;
                    let add = |name: Name, v: Value| {
                        new_mod_src(
                            &skillModList,
                            name,
                            ModType::BASE,
                            v,
                            r#mod.source.as_deref(),
                            r#mod.flags,
                            r#mod.keywordFlags,
                            &r#mod.tags,
                        );
                    };
                    // L1026
                    if physModeIs("AVERAGE") || physModeIs("FIRE") {
                        // L1027
                        add(n!("PhysicalDamageGainAsCold"), Value::Num(effVal));
                        // L1028
                        add(n!("PhysicalDamageGainAsLightning"), Value::Num(effVal));
                    } else if physModeIs("COLD") {
                        // L1030
                        add(n!("PhysicalDamageGainAsCold"), r#mod.value.clone());
                    } else if physModeIs("LIGHTNING") {
                        // L1032
                        add(n!("PhysicalDamageGainAsLightning"), r#mod.value.clone());
                    }
                }
            }
        }
    }
    Flow::Normal
}

// Local functions declared in OF-A that cross segments (§7.2). Their provenance is part of the
// OF-A segment marker above (CalcOffence.lua 374-493, 494-508, 509-514).

/// `calcAreaOfEffect(skillModList, skillCfg, skillData, skillFlags, output, breakdown)`
/// (CalcOffence.lua:374-493; `actor` is the offence upvalue).
pub fn calcAreaOfEffect(
    ctx: &OffenceCtx,
    skillModList: &ModList,
    skillCfg: &Cfg,
    skillData: &Table,
    skillFlags: &Table,
    output: &Output,
    breakdown: Option<&Breakdown>,
) {
    let actor = &ctx.actor;
    let cfg = Some(&**skillCfg);
    let label = |k: Name| skillData.str(k);
    // L375
    let (incArea, moreArea) = calcLib::mods(
        skillModList,
        cfg,
        nm!["AreaOfEffect", "AreaOfEffectPrimary"],
    );
    // L376
    output.set(n!("AreaOfEffectMod"), calcAreaMod(incArea, moreArea));
    // L377
    let radiusMod = calcLib::r#mod(skillModList, cfg, nm!["AreaOfEffectRadius"]);
    // L378
    if skillData.truthy(n!("radiusIsWeaponRange")) {
        // L379
        let mut range = 0.0;
        let weaponRange = |r: Option<f64>| {
            r.unwrap_or_else(|| lua_error!("bad argument #2 to 'max' (number expected, got nil)"))
        };
        // L380
        if skillFlags.truthy(n!("weapon1Attack")) {
            // L381
            range = m_max(range, weaponRange(actor.weaponRange1.get()));
        }
        // L383
        if skillFlags.truthy(n!("weapon2Attack")) {
            // L384
            range = m_max(range, weaponRange(actor.weaponRange2.get()));
        }
        // L386
        skillData.set(n!("radius"), range + 2.0);
    }
    // L388
    if skillData.truthy(n!("radius")) {
        // L389
        skillFlags.set(n!("area"), true);
        // L390
        let mut baseRadius = skillData.num(n!("radius"))
            + skillData.num_or(n!("radiusExtra"), 0.0)
            + skillModList.Sum(ModType::BASE, cfg, nm!["AreaOfEffect"]);
        // L391
        let v = calcRadius(
            baseRadius,
            output.num(n!("AreaOfEffectMod")),
            Some(radiusMod),
        );
        output.set(n!("AreaOfEffectRadius"), v);
        // L392
        let v = output.num(n!("AreaOfEffectRadius")) / 10.0;
        output.set(n!("AreaOfEffectRadiusMetres"), v);
        // L393
        if let Some(breakdown) = breakdown {
            // L394
            let (incAreaBreakpoint, moreAreaBreakpoint, redAreaBreakpoint, lessAreaBreakpoint) =
                calcRadiusBreakpointsOpt(baseRadius, incArea, moreArea, Some(radiusMod));
            // L395
            let t = breakdown.area(
                baseRadius,
                output.num(n!("AreaOfEffectMod")),
                output.num(n!("AreaOfEffectRadius")),
                incAreaBreakpoint,
                moreAreaBreakpoint,
                redAreaBreakpoint,
                lessAreaBreakpoint,
                label(n!("radiusLabel")).as_deref(),
                Some(radiusMod),
            );
            breakdown.set(n!("AreaOfEffectRadius"), t);
        }
        // L397
        if skillData.truthy(n!("radiusSecondary")) {
            // L398
            let (incAreaSecondary, moreAreaSecondary) = calcLib::mods(
                skillModList,
                cfg,
                nm!["AreaOfEffect", "AreaOfEffectSecondary"],
            );
            // L399
            output.set(
                n!("AreaOfEffectModSecondary"),
                calcAreaMod(incAreaSecondary, moreAreaSecondary),
            );
            // L400
            baseRadius =
                skillData.num(n!("radiusSecondary")) + skillData.num_or(n!("radiusExtra"), 0.0);
            // L401
            let v = calcRadius(
                baseRadius,
                output.num(n!("AreaOfEffectModSecondary")),
                Some(radiusMod),
            );
            output.set(n!("AreaOfEffectRadiusSecondary"), v);
            // L402
            let v = output.num(n!("AreaOfEffectRadiusSecondary")) / 10.0;
            output.set(n!("AreaOfEffectRadiusSecondaryMetres"), v);
            // L403
            if let Some(breakdown) = breakdown {
                // L404
                let mut bps: (Option<f64>, Option<f64>, Option<f64>, Option<f64>) =
                    (None, None, None, None);
                // L405
                if !skillData.truthy(n!("projectileSpeedAppliesToMSAreaOfEffect")) {
                    // L406
                    bps = calcRadiusBreakpointsOpt(
                        baseRadius,
                        incAreaSecondary,
                        moreAreaSecondary,
                        Some(radiusMod),
                    );
                }
                let (
                    incAreaBreakpointSecondary,
                    moreAreaBreakpointSecondary,
                    redAreaBreakpointSecondary,
                    lessAreaBreakpointSecondary,
                ) = bps;
                // L408
                let t = breakdown.area(
                    baseRadius,
                    output.num(n!("AreaOfEffectModSecondary")),
                    output.num(n!("AreaOfEffectRadiusSecondary")),
                    incAreaBreakpointSecondary,
                    moreAreaBreakpointSecondary,
                    redAreaBreakpointSecondary,
                    lessAreaBreakpointSecondary,
                    label(n!("radiusSecondaryLabel")).as_deref(),
                    Some(radiusMod),
                );
                breakdown.set(n!("AreaOfEffectRadiusSecondary"), t);
            }
        }
        // L411
        if skillData.truthy(n!("radiusTertiary")) {
            // L412
            let (incAreaTertiary, moreAreaTertiary) = calcLib::mods(
                skillModList,
                cfg,
                nm!["AreaOfEffect", "AreaOfEffectTertiary"],
            );
            // L413
            output.set(
                n!("AreaOfEffectModTertiary"),
                calcAreaMod(incAreaTertiary, moreAreaTertiary),
            );
            // L414
            baseRadius =
                skillData.num(n!("radiusTertiary")) + skillData.num_or(n!("radiusExtra"), 0.0);
            // L415
            if skillData.truthy(n!("projectileSpeedAppliesToMSAreaOfEffect")) {
                // L416
                let (incSpeedTertiary, moreSpeedTertiary) =
                    calcLib::mods(skillModList, cfg, nm!["ProjectileSpeed"]);
                // L417
                output.set(
                    n!("SpeedModTertiary"),
                    round(
                        round(incSpeedTertiary * moreSpeedTertiary, Some(10)),
                        Some(2),
                    ),
                );
                // L418
                let v = calcMoltenStrikeTertiaryRadius(
                    baseRadius,
                    skillData.num(n!("radiusSecondary")),
                    output.num(n!("AreaOfEffectModTertiary")),
                    output.num(n!("SpeedModTertiary")),
                );
                output.set(n!("AreaOfEffectRadiusTertiary"), v);
                // L419
                if let Some(breakdown) = breakdown {
                    // L420-423
                    let lbl = label(n!("radiusTertiaryLabel"));
                    setMoltenStrikeTertiaryRadiusBreakdown(
                        breakdown,
                        skillData.num(n!("radiusSecondary")),
                        baseRadius,
                        lbl.as_deref(),
                        incAreaTertiary,
                        moreAreaTertiary,
                        incSpeedTertiary,
                        moreSpeedTertiary,
                    );
                }
            } else if skillData.truthy(n!("radiusTertiaryBaseMargin")) {
                // L425 Currently only on explosive trap in the form of "Smaller explosions have between 30% reduced and 30% increased base radius at random"
                // L426
                let margin = skillData.num(n!("radiusTertiaryBaseMargin")) / 100.0;
                // L427
                let marginWidth = skillData.num(n!("radiusTertiaryBaseMargin")) * 2.0 + 1.0;
                // L428 Calculate all the possible base radii
                // L429
                let baseRadiiOccurrences = Table::new();
                // L430 (numeric for: the control variable accumulates the step)
                let (start, limit, step) = (1.0 - margin, 1.0 + margin + 0.01, 0.01);
                let mut deviation = start;
                while deviation <= limit {
                    // L431
                    let radiusForDeviation = m_floor(baseRadius * deviation);
                    // L432
                    let v = baseRadiiOccurrences.num_or(radiusForDeviation, 0.0) + 1.0;
                    baseRadiiOccurrences.set(radiusForDeviation, v);
                    deviation = deviation + step;
                }
                // L434 Calculate the modified radius for each base radius
                // L435
                let mut sumOfRandomRadii = 0.0;
                // L436
                let radiusForBaseRadius = Table::new();
                // L437
                let radiiOccurrences = Table::new();
                // L438 LUA-ORDER: integer keys; the sum is of integers (exact) and the tables
                // filled here are keyed by value, so the order is not observable.
                for (adjustedBaseRadius, occurrenceCount) in baseRadiiOccurrences.pairs() {
                    let adjustedBaseRadius = adjustedBaseRadius.to_val().num();
                    let occurrenceCount = occurrenceCount.num();
                    // L439
                    let radiusForDeviation = calcRadius(
                        adjustedBaseRadius,
                        output.num(n!("AreaOfEffectModTertiary")),
                        Some(radiusMod),
                    );
                    // L440
                    radiusForBaseRadius.set(adjustedBaseRadius, radiusForDeviation);
                    // L441
                    sumOfRandomRadii = sumOfRandomRadii + radiusForDeviation * occurrenceCount;
                    // L442
                    let v = radiiOccurrences.num_or(radiusForDeviation, 0.0) + occurrenceCount;
                    radiiOccurrences.set(radiusForDeviation, v);
                }
                // L444
                output.set(
                    n!("AreaOfEffectRadiusTertiary"),
                    sumOfRandomRadii / marginWidth,
                );
                // L445
                output.set(
                    n!("AreaOfEffectRadiusTertiaryOccurrences"),
                    radiiOccurrences,
                );
                // L446
                if let Some(breakdown) = breakdown {
                    // L447
                    let out = Table::new();
                    // L448
                    let mut incAreaBreakpointTertiary = m_huge;
                    let mut moreAreaBreakpointTertiary = m_huge;
                    let mut redAreaBreakpointTertiary = m_huge;
                    let mut lessAreaBreakpointTertiary = m_huge;
                    // L449
                    out.push(skillData.get(n!("radiusTertiaryLabel")));
                    // L450
                    out.push(lfmt!(
                        "R ^8(base radius)^7 x %.2f ^8(square root of area of effect modifier)",
                        m_floor(100.0 * m_sqrt(output.num(n!("AreaOfEffectModTertiary")))) / 100.0
                    ));
                    // L451
                    if radiusMod != 1.0 {
                        // L452
                        out.push(lfmt!("x %.2f ^8(final radius modifier)", radiusMod));
                    }
                    // L454
                    let mut baseRadii: Vec<f64> = Vec::new();
                    // L455 LUA-ORDER: sorted right below
                    for (adjustedBaseRadius, _) in baseRadiiOccurrences.pairs() {
                        // L456
                        baseRadii.push(adjustedBaseRadius.to_val().num());
                    }
                    // L458 (distinct numeric keys: the comparator gives a total order)
                    baseRadii.sort_by(|a, b| a.partial_cmp(b).expect("finite radii"));
                    // L459
                    for adjustedBaseRadius in baseRadii {
                        // L460
                        out.push(lfmt!(
                            "%.1f%% ^8chance of^7 %.1fm ^8base radius resulting in^7 %.1fm ^8final radius",
                            baseRadiiOccurrences.num(adjustedBaseRadius) / marginWidth * 100.0,
                            adjustedBaseRadius / 10.0,
                            radiusForBaseRadius.num(adjustedBaseRadius) / 10.0
                        ));
                        // L461
                        let (i, m, r, l) = calcRadiusBreakpointsOpt(
                            adjustedBaseRadius,
                            incAreaTertiary,
                            moreAreaTertiary,
                            Some(radiusMod),
                        );
                        let nil_min = |x: Option<f64>| {
                            x.unwrap_or_else(|| {
                                lua_error!("bad argument #2 to 'min' (number expected, got nil)")
                            })
                        };
                        // L462
                        incAreaBreakpointTertiary = m_min(incAreaBreakpointTertiary, nil_min(i));
                        // L463
                        moreAreaBreakpointTertiary = m_min(moreAreaBreakpointTertiary, nil_min(m));
                        // L464
                        redAreaBreakpointTertiary = m_min(redAreaBreakpointTertiary, nil_min(r));
                        // L465
                        lessAreaBreakpointTertiary = m_min(lessAreaBreakpointTertiary, nil_min(l));
                    }
                    // L467
                    out.push(lfmt!(
                        "^8Next closest 0.1m breakpoint: %d%% increased AoE / a %d%% more AoE multiplier",
                        incAreaBreakpointTertiary,
                        moreAreaBreakpointTertiary
                    ));
                    // L468
                    out.push(lfmt!(
                        "^8Previous closest 0.1m breakpoint: %d%% reduced AoE / a %d%% less AoE multiplier",
                        redAreaBreakpointTertiary,
                        lessAreaBreakpointTertiary
                    ));
                    // L469
                    out.push(lfmt!(
                        "On average, the radius is %.2fm",
                        output.num(n!("AreaOfEffectRadiusTertiary")) / 10.0
                    ));
                    // L470
                    breakdown.set(n!("AreaOfEffectRadiusTertiary"), out);
                }
            } else {
                // L473
                let v = calcRadius(
                    baseRadius,
                    output.num(n!("AreaOfEffectModTertiary")),
                    Some(radiusMod),
                );
                output.set(n!("AreaOfEffectRadiusTertiary"), v);
                // L474
                if let Some(breakdown) = breakdown {
                    // L475
                    let (
                        incAreaBreakpointTertiary,
                        moreAreaBreakpointTertiary,
                        redAreaBreakpointTertiary,
                        lessAreaBreakpointTertiary,
                    ) = calcRadiusBreakpointsOpt(
                        baseRadius,
                        incAreaTertiary,
                        moreAreaTertiary,
                        Some(radiusMod),
                    );
                    // L476
                    let t = breakdown.area(
                        baseRadius,
                        output.num(n!("AreaOfEffectModTertiary")),
                        output.num(n!("AreaOfEffectRadiusTertiary")),
                        incAreaBreakpointTertiary,
                        moreAreaBreakpointTertiary,
                        redAreaBreakpointTertiary,
                        lessAreaBreakpointTertiary,
                        label(n!("radiusTertiaryLabel")).as_deref(),
                        Some(radiusMod),
                    );
                    breakdown.set(n!("AreaOfEffectRadiusTertiary"), t);
                }
            }
            // L479
            let v = output.num(n!("AreaOfEffectRadiusTertiary")) / 10.0;
            output.set(n!("AreaOfEffectRadiusTertiaryMetres"), v);
        }
    }
    // L482
    if let Some(breakdown) = breakdown {
        // L483
        let t = Table::new();
        breakdown.set(n!("AreaOfEffectMod"), t.clone());
        // L484
        if output.num(n!("AreaOfEffectMod")) != 1.0 {
            // L485-489
            let chain = Table::new();
            chain.push(tbl![
                "%.2f ^8(increased/reduced)",
                1.0 + skillModList.Sum(ModType::INC, cfg, nm!["AreaOfEffect"]) / 100.0
            ]);
            chain.push(tbl![
                "%.2f ^8(more/less)",
                skillModList.More(cfg, nm!["AreaOfEffect"])
            ]);
            chain.set(
                n!("total"),
                lfmt!("= %.2f", output.num(n!("AreaOfEffectMod"))),
            );
            breakdown.multiChain(&t, &chain);
        }
    }
}

/// `calcResistForType(damageType, cfg)` (CalcOffence.lua:494-508; `env` and `enemyDB` are the
/// offence upvalues).
pub fn calcResistForType(ctx: &OffenceCtx, damageType: Name, cfg: Option<&ModCfg>) -> f64 {
    let env = &ctx.env;
    let enemyDB = &ctx.enemyDB;
    // L495
    let mut resist = Val::from_value(&enemyDB.Override(cfg, &[cat!(damageType, "Resist")]));
    // L496
    let maxResist = if enemyDB.Flag(None, nm!["DoNotChangeMaxResFromConfig"]) {
        data().misc.EnemyMaxResist
    } else {
        m_min(
            m_max(
                env.configInput.num_or(
                    cat!("enemy", damageType, "Resist"),
                    data().misc.EnemyMaxResist,
                ),
                data().misc.EnemyMaxResist,
            ),
            data().misc.MaxResistCap,
        )
    };
    // L497
    if !resist.truthy() {
        // L498
        if env
            .modDB
            .get()
            .Flag(None, &[cat!("Enemy", damageType, "ResistEqualToYours")])
        {
            // L499
            resist = env.player.output.get().get(cat!(damageType, "Resist"));
        } else if env
            .partyMembers
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'partyMembers' (a nil value)"))
            .modDB
            .get()
            .Flag(None, &[cat!("Enemy", damageType, "ResistEqualToYours")])
        {
            // L501
            let pm = env.partyMembers.get().expect("checked");
            resist = pm.output.get().get(cat!(damageType, "Resist"));
        } else {
            // L503
            let elemental = isElemental(damageType).then(|| n!("ElementalResist"));
            let ns = names([Some(cat!(damageType, "Resist")), elemental]);
            resist = Val::Num(
                enemyDB.Sum(ModType::BASE, cfg, &ns)
                    * m_max(calcLib::r#mod(enemyDB, cfg, &ns), 0.0),
            );
        }
    }
    // L506
    let resist = match resist {
        Val::Num(n) => n,
        other => lua_error!(
            "bad argument #1 to 'min' (number expected, got {})",
            other.lua_type()
        ),
    };
    m_max(m_min(resist, maxResist), data().misc.ResistFloor)
}

/// `runSkillFunc(name)`: calls `activeSkill.activeEffect.grantedEffect[name]` with the offence
/// top-level `output`/`breakdown` (CalcOffence.lua:509-514).
///
/// Probe builds first offer the call to the installed skill-hook handler
/// (`pob_calc::probe::set_skill_hook_handler`, CCR-0041), which the L4 tests use to replay the
/// hook's recorded `runSkillFunc:in → :out` delta; without a handler (and in non-probe builds)
/// this is exactly the Lua function.
pub fn runSkillFunc(ctx: &OffenceCtx, name: SkillHook) {
    #[cfg(feature = "probe")]
    if crate::probe::skill_hook_handled(&ctx.activeSkill, name) {
        return;
    }
    // L510
    let ge = ctx.activeSkill.activeEffect.get().grantedEffect.get();
    let func = name.of(&ge).cloned();
    // L511
    if let Some(func) = func {
        // L512
        match data_fns::skillFunc(&func) {
            SkillFuncImpl::Std(f) => f(&ctx.activeSkill, &ctx.output, ctx.breakdown.as_ref()),
            SkillFuncImpl::ExplosiveArrow(_) => lua_error!(
                "runSkillFunc: {name:?} of {} is an explosiveArrowFunc",
                ge.name
            ),
        }
    }
}
