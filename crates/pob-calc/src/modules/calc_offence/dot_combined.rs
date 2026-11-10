//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence DoT and combined DPS, OF-I (CalcOffence.lua 5649-6263): the attack combination of the
//! secondary effect stats (`combineStat`), Decay, burning ground from `DropsBurningGround`, the
//! skill DoT components (`dotCfg`, the per-type DoT loop, stacking / ground DoTs), cost per second
//! for link skills, self-hit damage sources, and the combined DPS estimate (impale, mirage, total
//! DoT, culling, reservation).
//!
//! Owner: OF-I.
//!
//! Notes on the port:
//! * `isElemental` (31) and `dmgTypeList` (34) are OF-A's module-level locals (immutable); this
//!   file reads them through the private copies below, as OF-C and OF-G do.
//! * `combineStat`'s vararg is a stat name (CCR-0055, OF-C).
//! * `activeSkill["dot"..damageType.."Cfg"]` (5812) is not a typed `ActiveSkill` field, so the cfg
//!   handle is stored in `activeSkill.extra` as an `Obj` (exported as a cfg by the probe).
//! * The self-hit handlers (5962-6071) are Lua closures in a string-keyed table iterated with
//!   `pairs` (`LUA-ORDER`): Rust runs them in constructor order. Their upvalues are `env` and
//!   `skillCfg`; their parameters shadow `activeSkill`, `output` and `breakdown` with the same
//!   values.
#![allow(non_snake_case)]
// Lua locals declared with a value that every branch reassigns (`local resist = 0`, `useSpeed = 1`)
#![allow(unused_assignments)]
// literal Lua statement shapes (`x = x + y`, `local x; x = …`, `if a then … elseif …`)
#![allow(
    clippy::assign_op_pattern,
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::type_complexity
)]

use super::costs_passes::combineStat;
use super::helpers_setup::{calcResistForType, runSkillFunc};
use super::OffenceCtx;
use crate::data_fns::SkillHook;
use crate::lua_table::TableField;
use crate::modules::calc_defence::applyDmgTakenConversion;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_lua::format::{lua_format, FmtArg};

// ---- module-level locals of OF-A's lines (immutable) -------------------------------------------

/// `isElemental[damageType]` (CalcOffence.lua:31): `{ Fire = true, Cold = true, Lightning = true }`.
fn isElemental(damageType: Name) -> bool {
    damageType == n!("Fire") || damageType == n!("Cold") || damageType == n!("Lightning")
}

/// `dmgTypeList` (CalcOffence.lua:34), ordered according to the conversion sequence.
fn dmgTypeList() -> Vec<Name> {
    vec![
        n!("Physical"),
        n!("Lightning"),
        n!("Cold"),
        n!("Fire"),
        n!("Chaos"),
    ]
}

// ---- plumbing ---------------------------------------------------------------------------------

/// A Lua value as a `string.format` argument (`%d`/`%f` of nil raises inside `lua_format`).
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

/// `t[k]` that must be a table (indexing a nil field raises).
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

/// `t_insert(t[k], v)` (the field is re-read, as Lua does).
fn tins(t: &Table, k: Name, v: impl Into<Val>) {
    match t.get(k) {
        Val::Table(x) => x.push(v),
        other => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `x or y` where `x` is a mod store `Override` result: a number, or nil (falls through).
fn ov(v: Value) -> Option<f64> {
    match v {
        Value::Nil | Value::Bool(false) => None,
        Value::Num(n) => Some(n),
        other => lua_error!(
            "attempt to perform arithmetic on a {} value",
            other.lua_type()
        ),
    }
}

/// `store:Override(cfg, "DotMultiplier") or store:Sum("BASE", cfg, "DotMultiplier") +
/// store:Sum("BASE", cfg, <typeDotMultiplier>)` (5725, 5762, 5845): one expression, left to right.
fn dotMultiplierOf(store: &ModStore, cfg: &ModCfg, typeDotMultiplier: Name) -> f64 {
    let c = Some(cfg);
    match ov(store.Override(c, nm!["DotMultiplier"])) {
        Some(o) => o,
        None => {
            store.Sum(ModType::BASE, c, nm!["DotMultiplier"])
                + store.Sum(ModType::BASE, c, &[typeDotMultiplier])
        }
    }
}

/// `env.modDB:Flag(nil, "Enemy"..dt.."ResistEqualToYours") and "Your "..dt.." Resistance" or
/// (env.partyMembers.modDB:Flag(nil, "Enemy"..dt.."ResistEqualToYours") and "Party Member "..dt..
/// " Resistance" or dt)` (5719, 5835).
fn sourceResOf(env: &EnvRef, damageType: Name) -> Name {
    let flag = cat!("Enemy", damageType, "ResistEqualToYours");
    if env.modDB.get().Flag(None, &[flag]) {
        cat!("Your ", damageType, " Resistance")
    } else if env
        .partyMembers
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'partyMembers' (a nil value)"))
        .modDB
        .get()
        .Flag(None, &[flag])
    {
        cat!("Party Member ", damageType, " Resistance")
    } else {
        damageType
    }
}

/// The cfg literal `{ skillName = skillCfg.skillName, skillPart = skillCfg.skillPart, skillTypes =
/// skillCfg.skillTypes, summonSkillName = skillCfg.summonSkillName, slotName = skillCfg.slotName,
/// flags = …, keywordFlags = … }` (5701-5709, 5772-5780).
fn skillDotCfgLiteral(skillCfg: &ModCfg, flags: u32, keywordFlags: u32) -> Cfg {
    let c = ModCfg::default();
    c.skillName.set(skillCfg.skillName.get());
    c.skillPart.set(skillCfg.skillPart.get());
    c.skillTypes.set(skillCfg.skillTypes.get());
    c.summonSkillName.set(skillCfg.summonSkillName.get());
    c.slotName.set(skillCfg.slotName.get());
    c.flags.set(flags);
    c.keywordFlags.set(keywordFlags);
    Rc::new(c)
}

/// `activeSkill.skillTypes[t]`.
fn hasSkillType(activeSkill: &ActiveSkillRef, t: SkillType::T) -> bool {
    activeSkill.skillTypes.get().with(|s| s.has(t))
}

/// `string.gsub(" "..value.damageType, "%W%l", string.upper):sub(2)` (5968 and the other
/// handlers): capitalises each word of the mod's damage type.
fn capitaliseDamageType(value: &Value) -> Name {
    let damageType = match value.get(n!("damageType")) {
        Value::Str(s) => s.to_string(),
        Value::Num(x) => tostring_num(*x),
        other => lua_error!(
            "attempt to concatenate field 'damageType' (a {} value)",
            other.lua_type()
        ),
    };
    let src = format!(" {damageType}");
    let (s, _) = pob_lua::pattern::gsub_fn(
        &src,
        "%W%l",
        |c| Some(pob_lua::string::upper(&c[0].to_str())),
        None,
    );
    Name::from(pob_lua::string::sub(&s, 2, None).as_ref())
}

/// `(dmgVal or 0) + value.baseDamage` operand (`value.baseDamage` nil raises).
fn baseDamageOf(value: &Value) -> f64 {
    value.get(n!("baseDamage")).num().unwrap_or_else(|| {
        lua_error!("attempt to perform arithmetic on field 'baseDamage' (a nil value)")
    })
}

// ---- self hit handlers (5962-6071) -------------------------------------------------------------

/// A self-hit handler: `function(activeSkill, output, breakdown)` with the upvalues `env` and
/// `skillCfg`; `Some((dmgBreakdown, dmgTaken))` is its two return values, `None` a bare return.
type SelfHitHandler = fn(
    env: &EnvRef,
    skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)>;

/// `["Heartbound Loop"]` (5963-5978).
fn handlerHeartboundLoop(
    _env: &EnvRef,
    _skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L5964
    if activeSkill.activeEffect.get().grantedEffect.get().name == n!("Summon Skeletons") {
        // L5965
        let mut dmgType: Option<Name> = None;
        let mut dmgVal: Option<f64> = None;
        // L5966
        let list = activeSkill
            .skillModList
            .get()
            .List(None, nm!["HeartboundLoopSelfDamage"]);
        for_ipairs!((_i, value) in list => {
            // Combines dmg taken from both ring accounting for catalysts
            // L5967
            dmgVal = Some(dmgVal.unwrap_or(0.0) + baseDamageOf(&value));
            // This assumes both rings deal the same damage type
            // L5968
            dmgType = Some(capitaliseDamageType(&value));
        });
        // L5970
        if let (Some(dmgType), Some(dmgVal)) = (dmgType, dmgVal) {
            // L5971
            let (dmgBreakdown, totalDmgTaken) =
                applyDmgTakenConversion(activeSkill, output, breakdown, dmgType, dmgVal);
            // L5972
            dmgBreakdown.insert(1, lfmt!("Heartbound Loop base damage: %d", dmgVal));
            // L5973
            dmgBreakdown.insert(2, lfmt!(""));
            // L5974
            let perCast = output.get(n!("SummonedMinionsPerCast"));
            dmgBreakdown.push(lua_format(
                "Total Heartbound Loop damage taken per cast/attack: %.2f * %d ^8(minions per cast)^7 = %.2f",
                &[
                    FmtArg::Num(totalDmgTaken),
                    fmt_arg(&perCast),
                    FmtArg::Num(totalDmgTaken * perCast.num()),
                ],
            ));
            // L5975
            return Some((
                dmgBreakdown,
                totalDmgTaken * output.num(n!("SummonedMinionsPerCast")),
            ));
        }
    }
    None
}

/// `["Storm Secret"]` (5979-5994).
fn handlerStormSecret(
    _env: &EnvRef,
    _skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L5980
    if activeSkill.activeEffect.get().grantedEffect.get().name == n!("Herald of Thunder") {
        // L5981
        let mut dmgType: Option<Name> = None;
        let mut dmgVal: Option<f64> = None;
        // L5982
        let list = activeSkill
            .skillModList
            .get()
            .List(None, nm!["StormSecretSelfDamage"]);
        for_ipairs!((_i, value) in list => {
            // Combines dmg taken from both rings accounting for catalysts
            // L5983
            dmgVal = Some(dmgVal.unwrap_or(0.0) + baseDamageOf(&value));
            // This assumes both rings deal the same damage type
            // L5984
            dmgType = Some(capitaliseDamageType(&value));
        });
        // L5986
        if let (Some(dmgType), Some(dmgVal)) = (dmgType, dmgVal) {
            // L5987
            let (dmgBreakdown, totalDmgTaken) =
                applyDmgTakenConversion(activeSkill, output, breakdown, dmgType, dmgVal);
            // L5988
            dmgBreakdown.insert(1, lfmt!("Storm Secret base damage: %d", dmgVal));
            // L5989
            dmgBreakdown.insert(2, lfmt!(""));
            // L5990
            dmgBreakdown.push(lfmt!(
                "Total Storm Secret damage taken per Herald of Thunder Hit: %.2f",
                totalDmgTaken
            ));
            // L5991
            return Some((dmgBreakdown, totalDmgTaken));
        }
    }
    None
}

/// `["Eye of Innocence"]` (5995-6009).
fn handlerEyeOfInnocence(
    _env: &EnvRef,
    _skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L5996
    let mut dmgType: Option<Name> = None;
    let mut dmgVal = Val::Nil;
    // L5997
    let list = activeSkill
        .skillModList
        .get()
        .List(None, nm!["EyeOfInnocenceSelfDamage"]);
    for_ipairs!((_i, value) in list => {
        // L5998
        dmgVal = Val::from_value(value.get(n!("baseDamage")));
        // L5999
        dmgType = Some(capitaliseDamageType(&value));
        // L6000
        break; // Only one mod of this kind is expected here
    });
    // L6002
    if activeSkill.skillFlags.get().truthy(n!("ignite")) && dmgType.is_some() && dmgVal.truthy() {
        let dmgType = dmgType.expect("checked");
        // L6003
        let (dmgBreakdown, totalDmgTaken) =
            applyDmgTakenConversion(activeSkill, output, breakdown, dmgType, dmgVal.num());
        // L6004
        dmgBreakdown.insert(
            1,
            lua_format("Eye of Innocence base damage: %d", &[fmt_arg(&dmgVal)]),
        );
        // L6005
        dmgBreakdown.insert(2, lfmt!(""));
        // L6006
        dmgBreakdown.push(lfmt!(
            "Total Eye of Innocence damage taken per cast/attack: %.2f ",
            totalDmgTaken
        ));
        // L6007
        return Some((dmgBreakdown, totalDmgTaken));
    }
    None
}

/// `["Echoes of Creation"]` (6010-6028); upvalue `env`.
fn handlerEchoesOfCreation(
    env: &EnvRef,
    _skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L6011
    let mut dmgType: Option<Name> = None;
    let mut dmgMult = Val::Nil;
    // L6012
    let list = activeSkill
        .skillModList
        .get()
        .List(None, nm!["EchoesOfCreationSelfDamage"]);
    for_ipairs!((_i, value) in list => {
        // L6013
        dmgMult = Val::from_value(value.get(n!("dmgMult")));
        // L6014
        dmgType = Some(capitaliseDamageType(&value));
        // L6015
        break; // Only one mod of this kind is expected here
    });
    // L6017
    let mut averageWarcryCount = output.num(n!("GlobalWarcryUptimeRatio")) / 100.0;
    // L6018
    if activeSkill
        .skillModList
        .get()
        .Flag(None, nm!["Condition:WarcryMaxHit"])
    {
        // L6019
        averageWarcryCount =
            env.modDB
                .get()
                .Sum(ModType::BASE, None, nm!["Multiplier:ExertingWarcryCount"]);
    }
    // L6021 (`averageWarcryCount` is always a number, so truthy)
    if dmgType.is_some() && dmgMult.truthy() {
        let dmgType = dmgType.expect("checked");
        // L6022
        let (dmgBreakdown, totalDmgTaken) = applyDmgTakenConversion(
            activeSkill,
            output,
            breakdown,
            dmgType,
            output.num0(n!("Life")) * dmgMult.num() / 100.0 * averageWarcryCount,
        );
        // L6023
        dmgBreakdown.insert(
            1,
            lua_format(
                "Echoes of Creation base damage: %d ^8(Life)^7 * %d%% * %.2f ^8(Avg. Warcry Count)^7 = %.2f",
                &[
                    FmtArg::Num(output.num0(n!("Life"))),
                    fmt_arg(&dmgMult),
                    FmtArg::Num(averageWarcryCount),
                    FmtArg::Num(output.num0(n!("Life")) * dmgMult.num() / 100.0 * averageWarcryCount),
                ],
            ),
        );
        // L6024
        dmgBreakdown.insert(2, lfmt!(""));
        // L6025
        dmgBreakdown.push(lfmt!(
            "Total Echoes of Creation damage taken per Attack: %.2f ",
            totalDmgTaken
        ));
        // L6026
        return Some((dmgBreakdown, totalDmgTaken));
    }
    None
}

/// `["Scold's Bridle"]` (6029-6043).
fn handlerScoldsBridle(
    _env: &EnvRef,
    _skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L6030
    let mut dmgType: Option<Name> = None;
    let mut dmgMult = Val::Nil;
    // L6031
    let list = activeSkill
        .skillModList
        .get()
        .List(None, nm!["ScoldsBridleSelfDamage"]);
    for_ipairs!((_i, value) in list => {
        // L6032
        dmgMult = Val::from_value(value.get(n!("dmgMult")));
        // L6033
        dmgType = Some(capitaliseDamageType(&value));
        // L6034
        break; // Only one mod of this kind is expected here
    });
    // L6036
    if output.truthy(n!("ManaHasCost")) && dmgType.is_some() && dmgMult.truthy() {
        let dmgType = dmgType.expect("checked");
        // L6037
        let (dmgBreakdown, totalDmgTaken) = applyDmgTakenConversion(
            activeSkill,
            output,
            breakdown,
            dmgType,
            output.num0(n!("ManaCost")) * dmgMult.num() / 100.0,
        );
        // L6038
        dmgBreakdown.insert(
            1,
            lua_format(
                "Scold's Bridle base damage: %d ^8(Mana Cost)^7 * %d%% = %.2f",
                &[
                    FmtArg::Num(output.num0(n!("ManaCost"))),
                    fmt_arg(&dmgMult),
                    FmtArg::Num(output.num0(n!("ManaCost")) * dmgMult.num() / 100.0),
                ],
            ),
        );
        // L6039
        dmgBreakdown.insert(2, lfmt!(""));
        // L6040
        dmgBreakdown.push(lfmt!(
            "Total Scold's Bridle damage taken per cast/attack: %.2f ",
            totalDmgTaken
        ));
        // L6041
        return Some((dmgBreakdown, totalDmgTaken));
    }
    None
}

/// `["Trauma"]` (6044-6056); upvalue `skillCfg`.
fn handlerTrauma(
    _env: &EnvRef,
    skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L6045
    let dmgType = n!("Physical");
    // L6046
    let currentTraumaStacks = m_max(
        activeSkill
            .skillModList
            .get()
            .Sum(ModType::BASE, None, nm!["Multiplier:TraumaStacks"]),
        1.0,
    );
    // L6047
    let damagePerTrauma =
        activeSkill
            .skillModList
            .get()
            .Sum(ModType::BASE, None, nm!["TraumaSelfDamageTakenLife"]);
    // L6048 (`false` or a number; a number is truthy, 0 included)
    let dmgVal = if activeSkill
        .baseSkillModList
        .get()
        .Flag(None, nm!["HasTrauma"])
    {
        Some(damagePerTrauma * currentTraumaStacks)
    } else {
        None
    };
    // L6049 (`dmgType` is the literal "Physical")
    if let Some(dmgVal) = dmgVal {
        // L6050
        let (dmgBreakdown, totalDmgTaken) =
            applyDmgTakenConversion(activeSkill, output, breakdown, dmgType, dmgVal);
        // L6051
        let sustainable = activeSkill.skillModList.get().Sum(
            ModType::BASE,
            Some(&**skillCfg),
            nm!["Multiplier:SustainableTraumaStacks"],
        ) == currentTraumaStacks;
        dmgBreakdown.insert(
            1,
            lfmt!(
                "%d ^8(base %s damage)^7 * %.2f ^8(%s trauma)^7 = %.2f %s damage",
                damagePerTrauma,
                dmgType,
                currentTraumaStacks,
                if sustainable {
                    "sustainable"
                } else {
                    "current"
                },
                dmgVal,
                dmgType
            ),
        );
        // L6052
        dmgBreakdown.insert(2, lfmt!(""));
        // L6053
        dmgBreakdown.push(lfmt!(
            "Total Trauma damage taken per cast/attack: %.2f ",
            totalDmgTaken
        ));
        // L6054
        return Some((dmgBreakdown, totalDmgTaken));
    }
    None
}

/// `["Enmity's Embrace"]` (6057-6070).
fn handlerEnmitysEmbrace(
    _env: &EnvRef,
    _skillCfg: &Cfg,
    activeSkill: &ActiveSkillRef,
    output: &Output,
    breakdown: Option<&Breakdown>,
) -> Option<(Table, f64)> {
    // L6058
    let mut dmgType: Option<Name> = None;
    let mut dmgVal: Option<f64> = None;
    // L6059
    let list = activeSkill
        .skillModList
        .get()
        .List(None, nm!["EnmitysEmbraceSelfDamage"]);
    for_ipairs!((_i, value) in list => {
        // Combines dmg taken from both rings accounting for catalysts
        // L6060
        dmgVal = Some(dmgVal.unwrap_or(0.0) + baseDamageOf(&value));
        // This assumes both rings deal the same damage type
        // L6061
        dmgType = Some(capitaliseDamageType(&value));
    });
    // L6063
    if let (Some(dmgType), Some(dmgVal)) = (dmgType, dmgVal) {
        // L6064
        let (dmgBreakdown, totalDmgTaken) =
            applyDmgTakenConversion(activeSkill, output, breakdown, dmgType, dmgVal);
        // L6065
        dmgBreakdown.insert(1, lfmt!("Enmity's Embrace base damage: %d", dmgVal));
        // L6066
        dmgBreakdown.insert(2, lfmt!(""));
        // L6067
        dmgBreakdown.push(lfmt!(
            "Total Enmity's Embrace damage taken per cast/attack: %.2f ",
            totalDmgTaken
        ));
        // L6068
        return Some((dmgBreakdown, totalDmgTaken));
    }
    None
}

// ---- the segment ------------------------------------------------------------------------------

/// @pob Modules/CalcOffence.lua:5649-6263 h=6a1ec530
/// OF-I: CalcOffence.lua 5649-6263 (incl. the closing `end`).
pub(crate) fn seg_dotCombined(ctx: &mut OffenceCtx) -> Flow {
    // the segment assigns no ctx local (it writes through the handles), so a shared view suffices
    let ctx: &OffenceCtx = ctx;
    let env = ctx.env.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let isAttack = ctx.isAttack;
    let debuffDurationMult = ctx.debuffDurationMult;
    let costs = ctx.costs.clone();
    let monsterLife = ctx.monsterLife;
    let canDeal = ctx.canDeal.clone();
    let quantityMultiplier = ctx.quantityMultiplier;
    let misc = &data().misc;
    let cs = |stat: Name, mode: Name, extra: Option<Name>| combineStat(ctx, stat, mode, extra);

    // Combine secondary effect stats
    // L5651
    if isAttack {
        // L5652
        cs(n!("BleedChance"), n!("AVERAGE"), None);
        // L5653
        cs(
            n!("BleedDPS"),
            n!("CHANCE_AILMENT"),
            Some(n!("BleedChance")),
        );
        // L5654
        cs(n!("PoisonChance"), n!("AVERAGE"), None);
        // L5655
        cs(n!("PoisonDPS"), n!("CHANCE"), Some(n!("PoisonChance")));
        // L5656
        cs(
            n!("CausticGroundDPS"),
            n!("CHANCE"),
            Some(n!("PoisonChance")),
        );
        // L5657
        cs(n!("TotalPoisonDPS"), n!("DPS"), None);
        // L5658
        cs(n!("PoisonDamage"), n!("CHANCE"), Some(n!("PoisonChance")));
        // L5659
        if skillData.truthy(n!("showAverage")) {
            // L5660
            cs(n!("TotalPoisonAverageDamage"), n!("DPS"), None);
        } else {
            // L5662
            cs(n!("TotalPoisonStacks"), n!("DPS"), None);
        }
        // L5664
        cs(n!("IgniteChance"), n!("AVERAGE"), None);
        // L5665
        cs(
            n!("IgniteDPS"),
            n!("CHANCE_AILMENT"),
            Some(n!("IgniteChance")),
        );
        // L5666
        if skillFlags.truthy(n!("igniteCanStack")) {
            // L5667
            cs(n!("IgniteDamage"), n!("CHANCE"), Some(n!("IgniteChance")));
            // L5668
            if skillData.truthy(n!("showAverage")) {
                // L5669
                cs(n!("TotalIgniteAverageDamage"), n!("DPS"), None);
                // L5670
                cs(n!("IgniteStacksMax"), n!("DPS"), None);
                // L5671
                cs(n!("TotalIgniteDPS"), n!("DPS"), None);
            } else {
                // L5673
                cs(n!("IgniteStacksMax"), n!("DPS"), None);
                // L5674
                cs(n!("TotalIgniteDPS"), n!("DPS"), None);
            }
        }
        // L5677
        cs(n!("ChillEffectMod"), n!("AVERAGE"), None);
        // L5678
        cs(n!("ChillDuration"), n!("AVERAGE"), None);
        // L5679
        cs(n!("ShockChance"), n!("AVERAGE"), None);
        // L5680
        cs(n!("ShockDuration"), n!("AVERAGE"), None);
        // L5681
        cs(n!("ShockEffectMod"), n!("AVERAGE"), None);
        // L5682
        cs(n!("FreezeChance"), n!("AVERAGE"), None);
        // L5683
        cs(n!("FreezeDurationMod"), n!("AVERAGE"), None);
        // L5684
        cs(n!("ScorchChance"), n!("AVERAGE"), None);
        // L5685
        cs(n!("ScorchEffectMod"), n!("AVERAGE"), None);
        // L5686
        cs(n!("ScorchDuration"), n!("AVERAGE"), None);
        // L5687
        cs(n!("BrittleChance"), n!("AVERAGE"), None);
        // L5688
        cs(n!("BrittleEffectMod"), n!("AVERAGE"), None);
        // L5689
        cs(n!("BrittleDuration"), n!("AVERAGE"), None);
        // L5690
        cs(n!("SapChance"), n!("AVERAGE"), None);
        // L5691
        cs(n!("SapEffectMod"), n!("AVERAGE"), None);
        // L5692
        cs(n!("SapDuration"), n!("AVERAGE"), None);
        // L5693
        cs(n!("ImpaleChance"), n!("AVERAGE"), None);
        // L5694
        cs(n!("ImpaleStoredDamage"), n!("AVERAGE"), None);
        // L5695
        cs(n!("ImpaleModifier"), n!("CHANCE"), Some(n!("ImpaleChance")));
    }

    // L5698
    if skillFlags.truthy(n!("hit")) && skillData.truthy(n!("decay")) && canDeal.truthy(n!("Chaos"))
    {
        // Calculate DPS for Essence of Delirium's Decay effect
        // L5700
        skillFlags.set(n!("decay"), true);
        // L5701
        activeSkill.decayCfg.set(Some(skillDotCfgLiteral(
            &skillCfg,
            ModFlag::Dot,
            (skillCfg.keywordFlags.get() & !KeywordFlag::Hit) | KeywordFlag::ChaosDot,
        )));
        // L5710
        let dotCfg = activeSkill
            .decayCfg
            .get()
            .expect("decayCfg was assigned at 5701");
        // L5711
        let mut effMult = 1.0;
        // L5712
        if env.mode_effective.get() {
            // L5713
            let resist = calcResistForType(ctx, n!("Chaos"), Some(&*dotCfg));
            // L5714
            let takenInc = enemyDB.Sum(
                ModType::INC,
                None,
                nm![
                    "DamageTaken",
                    "DamageTakenOverTime",
                    "ChaosDamageTaken",
                    "ChaosDamageTakenOverTime"
                ],
            );
            // L5715
            let takenMore = enemyDB.More(
                None,
                nm![
                    "DamageTaken",
                    "DamageTakenOverTime",
                    "ChaosDamageTaken",
                    "ChaosDamageTakenOverTime"
                ],
            );
            // L5716
            effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
            // L5717
            output.set(n!("DecayEffMult"), effMult);
            // L5718
            if let Some(b) = breakdown.as_ref().filter(|_| effMult != 1.0) {
                // L5719
                let sourceRes = sourceResOf(&env, n!("Chaos"));
                // L5720
                let t = b.effMult(
                    n!("Chaos"),
                    resist,
                    0.0,
                    takenInc,
                    effMult,
                    takenMore,
                    Some(sourceRes),
                    true,
                    None,
                    None,
                );
                b.set(n!("DecayEffMult"), t);
            }
        }
        // L5723
        let inc = skillModList.Sum(ModType::INC, Some(&*dotCfg), nm!["Damage", "ChaosDamage"]);
        // L5724
        let more = skillModList.More(Some(&*dotCfg), nm!["Damage", "ChaosDamage"]);
        // L5725
        let mult = dotMultiplierOf(&skillModList, &dotCfg, n!("ChaosDotMultiplier"));
        // L5726
        let decay = skillData.num(n!("decay"));
        output.set(
            n!("DecayDPS"),
            decay * (1.0 + inc / 100.0) * more * (1.0 + mult / 100.0) * effMult,
        );
        // L5727
        output.set(n!("DecayDuration"), 8.0 * debuffDurationMult);
        // L5728
        if let Some(b) = breakdown.as_ref() {
            // L5729
            b.set(n!("DecayDPS"), Table::new());
            // L5730
            b.dot(
                &tab(b, n!("DecayDPS")),
                skillData.num(n!("decay")),
                inc,
                more,
                Some(mult),
                None,
                None,
                Some(effMult),
                output.num(n!("DecayDPS")),
            );
            // L5731
            if output.num(n!("DecayDuration")) != 8.0 {
                // L5732
                b.set(
                    n!("DecayDuration"),
                    Table::from_ctor(vec![TableField::item(
                        false,
                        lfmt!("%.2fs ^8(base duration)", 8.0),
                    )]),
                );
                // L5735
                if debuffDurationMult != 1.0 {
                    // L5736
                    tins(
                        b,
                        n!("DecayDuration"),
                        lfmt!(
                            "/ %.2f ^8(debuff expires slower/faster)",
                            1.0 / debuffDurationMult
                        ),
                    );
                }
                // L5738
                tins(
                    b,
                    n!("DecayDuration"),
                    lfmt!("= %.2fs", output.num(n!("DecayDuration"))),
                );
            }
        }
    }

    // L5743
    let baseDropsBurningGround = modDB.Sum(ModType::BASE, None, nm!["DropsBurningGround"]);
    // L5744
    if baseDropsBurningGround > 0.0 {
        // L5745
        if canDeal.truthy(n!("Fire")) {
            // L5746
            let dotCfg = ModCfg::with_flags(ModFlag::Dot, 0);
            // L5750
            let dotTakenCfg = dotCfg.copyTable(true);
            // L5751
            let dotTypeCfg = dotCfg.copyTable(true);
            // L5752
            dotTypeCfg
                .keywordFlags
                .set(dotTypeCfg.keywordFlags.get() | KeywordFlag::FireDot);
            // L5753
            let mut effMult = 1.0;
            // L5754
            if env.mode_effective.get() {
                // L5755
                let resist = calcResistForType(ctx, n!("Fire"), Some(&*dotTypeCfg));
                // L5756
                let takenInc = enemyDB.Sum(
                    ModType::INC,
                    Some(&*dotTakenCfg),
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "FireDamageTaken",
                        "FireDamageTakenOverTime",
                        "ElementalDamageTaken"
                    ],
                );
                // L5757
                let takenMore = enemyDB.More(
                    Some(&*dotTakenCfg),
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "FireDamageTaken",
                        "FireDamageTakenOverTime",
                        "ElementalDamageTaken"
                    ],
                );
                // L5758
                effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
            }
            // L5760
            let inc = modDB.Sum(
                ModType::INC,
                Some(&*dotTypeCfg),
                nm!["Damage", "FireDamage", "ElementalDamage"],
            );
            // L5761
            let more = modDB.More(
                Some(&*dotTypeCfg),
                nm!["Damage", "FireDamage", "ElementalDamage"],
            );
            // L5762
            let mult = dotMultiplierOf(&modDB, &dotTypeCfg, n!("FireDotMultiplier"));
            // L5763
            let total = baseDropsBurningGround
                * (1.0 + inc / 100.0)
                * more
                * (1.0 + mult / 100.0)
                * effMult;
            // L5764
            if !output.truthy(n!("BurningGroundDPS")) || output.num(n!("BurningGroundDPS")) < total
            {
                // L5765
                output.set(n!("BurningGroundDPS"), total);
                // L5766
                output.set(n!("BurningGroundFromIgnite"), false);
            }
        }
    }

    // Calculate skill DOT components
    // L5772
    let dotCfg = skillDotCfgLiteral(
        &skillCfg,
        ModFlag::Dot | skillCfg.flags.get(),
        skillCfg.keywordFlags.get() & !KeywordFlag::Hit,
    );
    // L5781
    if (dotCfg.flags.get() | ModFlag::Area) == dotCfg.flags.get()
        && !skillData.truthy(n!("dotIsArea"))
    {
        // L5782
        dotCfg.flags.set(dotCfg.flags.get() & !ModFlag::Area);
    }
    // L5784
    if (dotCfg.flags.get() | ModFlag::Projectile) == dotCfg.flags.get()
        && !skillData.truthy(n!("dotIsProjectile"))
    {
        // L5785
        dotCfg.flags.set(dotCfg.flags.get() & !ModFlag::Projectile);
    }
    // L5787
    if (dotCfg.flags.get() | ModFlag::Spell) == dotCfg.flags.get()
        && !skillData.truthy(n!("dotIsSpell"))
    {
        // L5788
        dotCfg.flags.set(dotCfg.flags.get() & !ModFlag::Spell);
    }
    // L5790
    if (dotCfg.flags.get() | ModFlag::Attack) == dotCfg.flags.get()
        && !skillData.truthy(n!("dotIsAttack"))
    {
        // L5791
        dotCfg.flags.set(dotCfg.flags.get() & !ModFlag::Attack);
    }
    // L5793
    if (dotCfg.flags.get() | ModFlag::Hit) == dotCfg.flags.get()
        && !skillData.truthy(n!("dotIsHit"))
    {
        // L5794
        dotCfg.flags.set(dotCfg.flags.get() & !ModFlag::Hit);
    }

    // spell_damage_modifiers_apply_to_skill_dot does not apply to enemy damage taken
    // L5798
    let dotTakenCfg = dotCfg.copyTable(true);
    // L5799
    if skillData.truthy(n!("dotIsSpell")) {
        // L5800
        dotTakenCfg
            .flags
            .set(dotTakenCfg.flags.get() & !ModFlag::Spell);
    }

    // L5803
    activeSkill.dotCfg.set(Some(dotCfg.clone()));
    // L5804
    output.set(n!("TotalDotInstance"), 0.0);

    // L5806 (no grantedEffect defines `preDotFunc` at the pin; the call still iterates the hooks)
    runSkillFunc(ctx, SkillHook::preDotFunc);

    // -Section Handles Generic Damage over time [DOT]
    // L5809
    for_ipairs!((_i, damageType) in dmgTypeList() => {
        // L5810
        let dotTypeCfg = dotCfg.copyTable(true);
        // L5811
        let kw = KeywordFlag::by_name(cat!(damageType, "Dot").as_str())
            .unwrap_or_else(|| lua_error!("attempt to perform bitwise operation on a nil value"));
        dotTypeCfg.keywordFlags.set(dotTypeCfg.keywordFlags.get() | kw);
        // L5812
        activeSkill.extra.set(
            cat!("dot", damageType, "Cfg"),
            Val::Obj(dotTypeCfg.clone() as Rc<dyn std::any::Any>),
        );
        // L5813
        let baseVal: f64;
        // L5814
        if canDeal.truthy(damageType) {
            // L5815
            baseVal = skillData.num_or(cat!(damageType, "Dot"), 0.0);
        } else {
            // L5817
            baseVal = 0.0;
        }
        // L5819
        if baseVal > 0.0 || output.num_or(cat!(damageType, "Dot"), 0.0) > 0.0 {
            // L5820
            skillFlags.set(n!("dot"), true);
            // L5821
            let mut effMult = 1.0;
            //Section handles Enemy Damage Taken based on Configs
            // L5823
            if env.mode_effective.get() {
                // L5824
                let mut resist = 0.0;
                // L5825
                let takenInc = enemyDB.Sum(
                    ModType::INC,
                    Some(&*dotTakenCfg),
                    &[
                        n!("DamageTaken"),
                        n!("DamageTakenOverTime"),
                        cat!(damageType, "DamageTaken"),
                        cat!(damageType, "DamageTakenOverTime"),
                    ],
                ) + if isElemental(damageType) {
                    enemyDB.Sum(ModType::INC, Some(&*dotTakenCfg), nm!["ElementalDamageTaken"])
                } else {
                    0.0
                };
                // L5826
                let takenMore = enemyDB.More(
                    Some(&*dotTakenCfg),
                    &[
                        n!("DamageTaken"),
                        n!("DamageTakenOverTime"),
                        cat!(damageType, "DamageTaken"),
                        cat!(damageType, "DamageTakenOverTime"),
                    ],
                ) * if isElemental(damageType) {
                    enemyDB.More(Some(&*dotTakenCfg), nm!["ElementalDamageTaken"])
                } else {
                    1.0
                };
                // L5827
                if damageType == n!("Physical") {
                    // L5828
                    resist = m_max(
                        0.0,
                        m_min(
                            enemyDB.Sum(ModType::BASE, None, nm!["PhysicalDamageReduction"]),
                            misc.EnemyPhysicalDamageReductionCap,
                        ),
                    );
                } else {
                    // L5830
                    resist = calcResistForType(ctx, damageType, Some(&*dotTypeCfg));
                }
                // L5832
                effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
                // L5833
                output.set(cat!(damageType, "DotEffMult"), effMult);
                // L5834
                if let Some(b) = breakdown.as_ref().filter(|_| effMult != 1.0) {
                    // L5835
                    let sourceRes = sourceResOf(&env, damageType);
                    // L5836
                    let t = b.effMult(
                        damageType,
                        resist,
                        0.0,
                        takenInc,
                        effMult,
                        takenMore,
                        Some(sourceRes),
                        true,
                        None,
                        None,
                    );
                    b.set(cat!(damageType, "DotEffMult"), t);
                }
            }
            //Variables below calculate DOT damage
            let typeNames = names([
                Some(n!("Damage")),
                Some(cat!(damageType, "Damage")),
                isElemental(damageType).then(|| n!("ElementalDamage")),
            ]);
            // L5840
            let mut inc = skillModList.Sum(ModType::INC, Some(&*dotTypeCfg), &typeNames);
            // L5841
            if skillModList.Flag(None, nm!["dotIsHeraldOfAsh"]) {
                // L5842
                inc = m_max(
                    inc - skillModList.Sum(ModType::INC, Some(&*skillCfg), &typeNames),
                    0.0,
                );
            }
            // L5844
            let more = skillModList.More(Some(&*dotTypeCfg), &typeNames);
            // L5845
            let mult =
                dotMultiplierOf(&skillModList, &dotTypeCfg, cat!(damageType, "DotMultiplier"));
            // L5846 (`false`/nil, or the aura effect mod)
            let aura = if hasSkillType(&activeSkill, SkillType::Aura)
                && !hasSkillType(&activeSkill, SkillType::RemoteMined)
                && !hasSkillType(&activeSkill, SkillType::Banner)
            {
                Some(calcLib::r#mod(
                    &skillModList,
                    Some(&*dotTypeCfg),
                    nm!["AuraEffect"],
                ))
            } else {
                None
            };
            // L5847
            let total = baseVal
                * (1.0 + inc / 100.0)
                * more
                * (1.0 + mult / 100.0)
                * aura.unwrap_or(1.0)
                * effMult;
            // L5848
            let dotKey = cat!(damageType, "Dot");
            if output.get(dotKey).raw_eq(&Val::Num(0.0)) || !output.truthy(dotKey) {
                // L5849
                output.set(dotKey, total);
                // L5850
                let v = m_min(output.num(n!("TotalDotInstance")) + total, misc.DotDpsCap);
                output.set(n!("TotalDotInstance"), v);
            } else {
                // L5852
                let v = m_min(
                    output.num(n!("TotalDotInstance")) + total + output.num_or(dotKey, 0.0),
                    misc.DotDpsCap,
                );
                output.set(n!("TotalDotInstance"), v);
            }
            // L5854
            if let Some(b) = breakdown.as_ref() {
                // L5855
                b.set(dotKey, Table::new());
                // L5856
                b.dot(
                    &tab(b, dotKey),
                    baseVal,
                    inc,
                    more,
                    Some(mult),
                    None,
                    aura,
                    Some(effMult),
                    total,
                );
            }
        }
    });

    // L5861
    if skillModList.Flag(None, nm!["DotCanStack"]) {
        // L5862
        skillFlags.set(n!("DotCanStack"), true);
        // L5863
        let mut speed = output.get(n!("Speed"));
        // Check if skill is being triggered via Mine (e.g., Blastchain Mine Support) or Trap
        // if "yes", you cannot use output.Speed but rather should use output.MineLayingSpeed or output.TrapThrowingSpeed
        // L5866
        if dotCfg.keywordFlags.get() & KeywordFlag::Mine != 0 {
            // L5867
            speed = output.get(n!("MineLayingSpeed"));
        } else if dotCfg.keywordFlags.get() & KeywordFlag::Trap != 0 {
            // L5868
            // L5869
            speed = output.get(n!("TrapThrowingSpeed"));
        }
        // L5871
        let v = m_min(
            output.num(n!("TotalDotInstance"))
                * speed.num()
                * output.num(n!("Duration"))
                * skillData.num(n!("dpsMultiplier"))
                * quantityMultiplier,
            misc.DotDpsCap,
        );
        output.set(n!("TotalDot"), v);
        // L5872
        output.set(n!("TotalDotCalcSection"), output.get(n!("TotalDot")));
        // L5873
        if let Some(b) = breakdown.as_ref() {
            // L5874
            let t = Table::from_ctor(vec![
                TableField::item(
                    false,
                    lua_format(
                        "%.1f ^8(Damage per Instance)",
                        &[fmt_arg(&output.get(n!("TotalDotInstance")))],
                    ),
                ),
                TableField::item(
                    false,
                    lua_format("x %.2f ^8(hits per second)", &[fmt_arg(&speed)]),
                ),
                TableField::item(
                    false,
                    lua_format(
                        "x %.2f ^8(skill duration)",
                        &[fmt_arg(&output.get(n!("Duration")))],
                    ),
                ),
            ]);
            b.set(n!("TotalDot"), t);
            // L5879
            if !skillData.get(n!("dpsMultiplier")).raw_eq(&Val::Num(1.0)) {
                // L5880
                tins(
                    b,
                    n!("TotalDot"),
                    lua_format(
                        "x %g ^8(DPS multiplier for this skill)",
                        &[fmt_arg(&skillData.get(n!("dpsMultiplier")))],
                    ),
                );
            }
            // L5882
            if quantityMultiplier > 1.0 {
                // L5883
                tins(
                    b,
                    n!("TotalDot"),
                    lfmt!(
                        "x %g ^8(quantity multiplier for this skill)",
                        quantityMultiplier
                    ),
                );
            }
            // L5885
            tins(
                b,
                n!("TotalDot"),
                lua_format("= %.1f", &[fmt_arg(&output.get(n!("TotalDot")))]),
            );
        }
    } else if skillModList.Flag(None, nm!["dotIsBurningGround"]) {
        // L5887
        // L5888
        output.set(n!("TotalDot"), 0.0);
        // L5889
        output.set(
            n!("TotalDotCalcSection"),
            output.get(n!("TotalDotInstance")),
        );
        // L5890
        if !output.truthy(n!("BurningGroundDPS"))
            || output.num(n!("BurningGroundDPS")) < output.num(n!("TotalDotInstance"))
        {
            // L5891
            let v = m_max(
                output.num_or(n!("BurningGroundDPS"), 0.0),
                output.num(n!("TotalDotInstance")),
            );
            output.set(n!("BurningGroundDPS"), v);
            // L5892
            output.set(n!("BurningGroundFromIgnite"), false);
        }
    } else if skillModList.Flag(None, nm!["dotIsCausticGround"]) {
        // L5894
        // L5895
        output.set(n!("TotalDot"), 0.0);
        // L5896
        output.set(
            n!("TotalDotCalcSection"),
            output.get(n!("TotalDotInstance")),
        );
        // L5897
        if !output.truthy(n!("CausticGroundDPS"))
            || output.num(n!("CausticGroundDPS")) < output.num(n!("TotalDotInstance"))
        {
            // L5898
            let v = m_max(
                output.num_or(n!("CausticGroundDPS"), 0.0),
                output.num(n!("TotalDotInstance")),
            );
            output.set(n!("CausticGroundDPS"), v);
            // L5899
            output.set(n!("CausticGroundFromPoison"), false);
        }
    } else if skillModList.Flag(None, nm!["dotIsCorruptingBlood"]) {
        // L5901
        // L5902
        output.set(n!("TotalDot"), 0.0);
        // L5903
        output.set(
            n!("TotalDotCalcSection"),
            output.get(n!("TotalDotInstance")),
        );
        // L5904
        if !output.truthy(n!("CorruptingBloodDPS"))
            || output.num(n!("CorruptingBloodDPS")) < output.num(n!("TotalDotInstance"))
        {
            // L5905
            let v = m_max(
                output.num_or(n!("CorruptingBloodDPS"), 0.0),
                output.num(n!("TotalDotInstance")),
            );
            output.set(n!("CorruptingBloodDPS"), v);
        }
    } else {
        // L5908
        if skillModList.Flag(None, nm!["DotCanStackAsTotems"]) && skillFlags.truthy(n!("totem")) {
            // L5909
            skillFlags.set(n!("DotCanStack"), true);
        }
        // L5911 (`a and b and c or 1`)
        let attachedBrandCount = {
            let v = if hasSkillType(&activeSkill, SkillType::Brand)
                && !skillData.truthy(n!("countsAttachedBrandsInDamage"))
            {
                output.get(n!("AttachedBrandCount"))
            } else {
                Val::Nil
            };
            lor(v, || Val::Num(1.0))
        };
        // L5912
        let v = if attachedBrandCount.num() > 1.0 {
            Val::Num(m_min(
                output.num(n!("TotalDotInstance")) * attachedBrandCount.num(),
                misc.DotDpsCap,
            ))
        } else {
            output.get(n!("TotalDotInstance"))
        };
        output.set(n!("TotalDot"), v);
        // L5913
        output.set(n!("TotalDotCalcSection"), output.get(n!("TotalDot")));
    }

    //Calculates and displays cost per second for skills that don't already have one (link skills)
    // L5917
    for_ipairs!((_i, resource) in tab(&costs, n!("order")) => {
        let resource: Name = match resource.str() {
            Some(s) => Name::from(&s),
            None => lua_error!("attempt to concatenate local 'resource' (a {} value)", resource.lua_type()),
        };
        // L5918
        let val = tab(&costs, resource);
        // L5919
        let EB = env.modDB.get().Flag(None, nm!["EnergyShieldProtectsMana"]);
        // L5920 (`EB and skillModList:Sum(…)`: a number is truthy, 0 included; LUA-QUIRK)
        if val.truthy(n!("upfront"))
            && output.truthy(cat!(resource, "HasCost"))
            && output.num(cat!(resource, "Cost")) > 0.0
            && !(output.truthy(cat!(resource, "PerSecondHasCost"))
                && !(EB && {
                    let _ = skillModList.Sum(
                        ModType::BASE,
                        Some(&*skillCfg),
                        nm!["ManaCostAsEnergyShieldCost"],
                    );
                    true
                }))
            && (output.num(n!("Speed")) > 0.0 || output.truthy(n!("Cooldown")))
        {
            // L5921
            let mut usedResource = resource;

            // L5923
            if EB && resource == n!("Mana") {
                // L5924
                usedResource = n!("ES");
            }

            // L5927
            let repeats = lor(output.get(n!("Repeats")), || Val::Num(1.0));
            // L5928
            let mut useSpeed = Val::Num(1.0);
            // L5929
            let timeType: &str;
            // L5930
            if skillFlags.truthy(n!("trap")) || skillFlags.truthy(n!("mine")) {
                // L5931
                let preSpeed = lor(output.get(n!("TrapThrowingSpeed")), || {
                    output.get(n!("MineLayingSpeed"))
                });
                // L5932
                let cooldown = lor(output.get(n!("TrapCooldown")), || output.get(n!("Cooldown")));
                // L5933
                useSpeed = if cooldown.truthy() && cooldown.num() > 0.0 {
                    Val::Num(1.0 / cooldown.num())
                } else {
                    preSpeed
                };
                // L5934
                timeType = if skillFlags.truthy(n!("trap")) {
                    "trap throwing"
                } else {
                    "mine laying"
                };
            } else if skillFlags.truthy(n!("totem")) {
                // L5935
                // L5936
                let v = if output.truthy(n!("Cooldown")) && output.num(n!("Cooldown")) > 0.0 {
                    if output.num(n!("TotemPlacementSpeed")) > 0.0 {
                        output.num(n!("TotemPlacementSpeed"))
                    } else {
                        1.0 / output.num(n!("Cooldown"))
                    }
                } else {
                    output.num(n!("TotemPlacementSpeed"))
                };
                useSpeed = Val::Num(v / repeats.num());
                // L5937
                timeType = "totem placement";
            // nil check until ailment pass for skills like Vortex
            } else if skillModList.Flag(None, nm!["HasSeals"])
                && skillModList.Flag(None, nm!["UseMaxUnleash"])
                && env
                    .player
                    .mainSkill
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
                    .skillData
                    .get()
                    .truthy(n!("hitTimeOverride"))
            {
                // L5939
                // L5940
                let hitTimeOverride = env
                    .player
                    .mainSkill
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
                    .skillData
                    .get()
                    .num(n!("hitTimeOverride"));
                useSpeed = Val::Num(hitTimeOverride / repeats.num());
                // L5941
                timeType = "full unleash";
            } else {
                // L5943
                let v = if output.truthy(n!("Cooldown")) && output.num(n!("Cooldown")) > 0.0 {
                    if output.num(n!("Speed")) > 0.0 {
                        output.num(n!("Speed"))
                    } else {
                        1.0 / output.num(n!("Cooldown"))
                    }
                } else {
                    output.num(n!("Speed"))
                };
                useSpeed = Val::Num(v / repeats.num());
                // L5944
                timeType = if skillData.truthy(n!("triggered")) {
                    "trigger"
                } else if skillFlags.truthy(n!("totem")) {
                    "totem placement"
                } else if skillFlags.truthy(n!("attack")) {
                    "attack"
                } else {
                    "cast"
                };
            }

            // L5947
            output.set(cat!(usedResource, "PerSecondHasCost"), true);
            // L5948
            let v = output.num_or(cat!(usedResource, "PerSecondCost"), 0.0)
                + output.num(cat!(resource, "Cost")) * useSpeed.num();
            output.set(cat!(usedResource, "PerSecondCost"), v);

            // L5950
            if let Some(b) = breakdown.as_ref() {
                // L5951
                let copy = tab(b, cat!(resource, "Cost")).copyTable(false);
                b.set(cat!(usedResource, "PerSecondCost"), copy);
                // L5952
                tab(b, cat!(usedResource, "PerSecondCost")).remove(None);
                // L5953
                let fmt = format!("x %.2f ^8({timeType} speed)");
                tins(
                    b,
                    cat!(usedResource, "PerSecondCost"),
                    lua_format(&fmt, &[fmt_arg(&useSpeed)]),
                );
                // L5954
                tins(
                    b,
                    cat!(usedResource, "PerSecondCost"),
                    lfmt!(
                        "= %.2f per second",
                        output.num(cat!(usedResource, "PerSecondCost"))
                    ),
                );
            }
        }
    });

    // Self hit dmg calcs
    // L5960
    {
        // Handler functions for self hit sources
        // L5962 (string keys; constructor order, see the module notes)
        let nameToHandler: [(&str, SelfHitHandler); 7] = [
            ("Heartbound Loop", handlerHeartboundLoop),
            ("Storm Secret", handlerStormSecret),
            ("Eye of Innocence", handlerEyeOfInnocence),
            ("Echoes of Creation", handlerEchoesOfCreation),
            ("Scold's Bridle", handlerScoldsBridle),
            ("Trauma", handlerTrauma),
            ("Enmity's Embrace", handlerEnmitysEmbrace),
        ];
        // L6072 LUA-ORDER: `pairs` over string keys; the order shows in `output.SelfHitDamage`'s
        // float sum and in the line order of `breakdown.SelfHitDamage` when several sources apply.
        for (_name, sourceFunc) in nameToHandler {
            // L6073
            let r = sourceFunc(&env, &skillCfg, &activeSkill, &output, breakdown.as_ref());
            let (selfHitBreakdown, dmgTaken) = match r {
                Some((t, d)) => (Some(t), Some(d)),
                None => (None, None),
            };
            // L6074
            if let Some(dmgTaken) = dmgTaken {
                // L6075
                let v = output.num_or(n!("SelfHitDamage"), 0.0) + dmgTaken;
                output.set(n!("SelfHitDamage"), v);
            }
            // L6077
            if let (Some(b), Some(selfHitBreakdown)) = (breakdown.as_ref(), selfHitBreakdown) {
                // L6078
                let v = lor(b.get(n!("SelfHitDamage")), || Val::Table(Table::new()));
                b.set(n!("SelfHitDamage"), v);
                // L6079
                for_ipairs!((_j, line) in selfHitBreakdown => {
                    // L6080
                    tins(b, n!("SelfHitDamage"), line);
                });
                // L6082
                tins(b, n!("SelfHitDamage"), "");
            }
        }

        // Special handling for self hit skills
        // These need to be handled higher up in this file using runFuncs for correct DPS calcs
        // L6088 LUA-QUIRK: `ipairs` over a table without an array part runs zero iterations.
        let selfHitSkills = Table::from_ctor(vec![TableField::bracket(
            "FRDamageTaken",
            true,
            true,
            "Forbidden Rite",
        )]);
        for_ipairs!((selfHitSkill, _displayName) in selfHitSkills => {
            let selfHitSkill = selfHitSkill as i64;
            // L6089
            if output.truthy(selfHitSkill) {
                // L6090
                let v = output.num_or(n!("SelfHitDamage"), 0.0) + output.num(selfHitSkill);
                output.set(n!("SelfHitDamage"), v);
            }
            // L6092
            if let Some(b) = breakdown.as_ref().filter(|b| b.truthy(selfHitSkill)) {
                // L6093
                let v = lor(b.get(n!("SelfHitDamage")), || Val::Table(Table::new()));
                b.set(n!("SelfHitDamage"), v);
                // L6094
                let lines = match b.get(selfHitSkill) {
                    Val::Table(t) => t,
                    other => lua_error!("bad argument #1 to 'ipairs' (table expected, got {})", other.lua_type()),
                };
                for_ipairs!((_j, line) in lines => {
                    // L6095
                    tins(b, n!("SelfHitDamage"), line);
                });
                // L6097
                tins(b, n!("SelfHitDamage"), "");
            }
        });

        // L6101
        if let Some(b) = breakdown.as_ref().filter(|b| b.truthy(n!("SelfHitDamage"))) {
            // L6102 Remove new line at the end
            let t = tab(b, n!("SelfHitDamage"));
            t.set(t.len() as i64, Val::Nil);
        }
    }

    // Calculate combined DPS estimate, including DoTs
    // L6107
    let baseDPS = output.get(if skillData.truthy(n!("showAverage")) {
        n!("AverageDamage")
    } else {
        n!("TotalDPS")
    });
    // L6108
    output.set(n!("CombinedDPS"), baseDPS.clone());
    // L6109
    output.set(n!("CombinedAvg"), baseDPS.clone());
    // L6110
    if skillFlags.truthy(n!("dot")) {
        // L6111
        output.set(
            n!("WithDotDPS"),
            baseDPS.num() + output.num_or(n!("TotalDot"), 0.0),
        );
    }
    // L6113
    if quantityMultiplier > 1.0 && output.truthy(n!("TotalPoisonDPS")) {
        // L6114
        let v = m_min(
            output.num(n!("TotalPoisonDPS")) * quantityMultiplier,
            misc.DotDpsCap,
        );
        output.set(n!("TotalPoisonDPS"), v);
    }
    // L6116
    if skillData.truthy(n!("showAverage")) {
        // L6117
        let v = output.num(n!("CombinedAvg")) + output.num_or(n!("TotalPoisonAverageDamage"), 0.0);
        output.set(n!("CombinedAvg"), v);
        // L6118
        output.set(
            n!("WithPoisonDPS"),
            baseDPS.num() + output.num_or(n!("TotalPoisonAverageDamage"), 0.0),
        );
    } else {
        // L6120
        output.set(
            n!("WithPoisonDPS"),
            baseDPS.num() + output.num_or(n!("TotalPoisonDPS"), 0.0),
        );
    }
    // L6122
    if skillFlags.truthy(n!("ignite")) {
        // L6123
        if skillFlags.truthy(n!("igniteCanStack")) {
            // L6124
            if skillData.truthy(n!("showAverage")) {
                // L6125
                let v = output.num(n!("CombinedDPS")) + output.num(n!("IgniteDamage"));
                output.set(n!("CombinedAvg"), v);
            } else {
                // L6127
                output.set(
                    n!("WithIgniteDPS"),
                    baseDPS.num() + output.num(n!("TotalIgniteDPS")),
                );
            }
        } else if skillData.truthy(n!("showAverage")) {
            // L6129
            // L6130
            output.set(
                n!("WithIgniteDPS"),
                baseDPS.num() + output.num(n!("IgniteDamage")),
            );
            // L6131
            let v = output.num(n!("CombinedAvg")) + output.num(n!("IgniteDamage"));
            output.set(n!("CombinedAvg"), v);
        } else {
            // L6133
            output.set(
                n!("WithIgniteDPS"),
                baseDPS.num() + output.num(n!("IgniteDPS")),
            );
        }
    } else {
        // L6136
        output.set(n!("WithIgniteDPS"), baseDPS.clone());
    }
    // L6138
    if skillFlags.truthy(n!("monsterExplode")) {
        // L6139
        output.set(
            n!("CombinedAvgToMonsterLife"),
            output.num(n!("CombinedAvg")) / monsterLife * 100.0,
        );
    }
    // L6141
    if skillFlags.truthy(n!("bleed")) {
        // L6142
        if skillData.truthy(n!("showAverage")) {
            // L6143
            output.set(
                n!("WithBleedDPS"),
                baseDPS.num() + output.num(n!("BleedDamage")),
            );
            // L6144
            let v = output.num(n!("CombinedAvg")) + output.num(n!("BleedDamage"));
            output.set(n!("CombinedAvg"), v);
        } else {
            // L6146
            output.set(
                n!("WithBleedDPS"),
                baseDPS.num() + output.num(n!("BleedDPS")),
            );
        }
    } else {
        // L6149
        output.set(n!("WithBleedDPS"), baseDPS.clone());
    }
    // L6151
    if skillFlags.truthy(n!("impale")) {
        // L6152
        let mut mainHandImpaleDPS: Option<f64> = None;
        let mut offHandImpaleDPS: Option<f64> = None;
        // L6153
        if skillFlags.truthy(n!("attack"))
            && skillData.truthy(n!("doubleHitsWhenDualWielding"))
            && skillFlags.truthy(n!("bothWeaponAttack"))
        {
            // separately combine
            let mh = tab(&output, n!("MainHand"));
            let oh = tab(&output, n!("OffHand"));
            // L6155
            mainHandImpaleDPS = Some(
                mh.num(n!("impaleStoredHitAvg"))
                    * (mh.num_or(n!("ImpaleModifier"), 1.0) - 1.0)
                    * mh.num(n!("HitChance"))
                    / 100.0
                    * skillData.num(n!("dpsMultiplier")),
            );
            // L6156
            offHandImpaleDPS = Some(
                oh.num(n!("impaleStoredHitAvg"))
                    * (oh.num_or(n!("ImpaleModifier"), 1.0) - 1.0)
                    * oh.num(n!("HitChance"))
                    / 100.0
                    * skillData.num(n!("dpsMultiplier")),
            );
            // L6157
            output.set(
                n!("ImpaleDPS"),
                mainHandImpaleDPS.expect("set") + offHandImpaleDPS.expect("set"),
            );
        } else {
            // L6159
            output.set(
                n!("ImpaleDPS"),
                output.num(n!("impaleStoredHitAvg"))
                    * (output.num_or(n!("ImpaleModifier"), 1.0) - 1.0)
                    * output.num(n!("HitChance"))
                    / 100.0
                    * skillData.num(n!("dpsMultiplier")),
            );
        }
        // L6161
        if output.num(n!("ImpaleDuration")) <= 0.0 {
            // L6162
            output.set(n!("ImpaleDPS"), 0.0);
        }
        // L6164
        if skillData.truthy(n!("showAverage")) {
            // L6165
            output.set(
                n!("WithImpaleDPS"),
                output.num(n!("AverageDamage")) + output.num(n!("ImpaleDPS")),
            );
            // L6166
            let v = output.num(n!("CombinedAvg")) + output.num(n!("ImpaleDPS"));
            output.set(n!("CombinedAvg"), v);
        } else {
            // L6168
            skillFlags.set(n!("notAverage"), true);
            // L6169
            let rate = lor(output.get(n!("HitSpeed")), || output.get(n!("Speed")));
            let v = output.num(n!("ImpaleDPS")) * rate.num();
            output.set(n!("ImpaleDPS"), v);
            // L6170
            output.set(
                n!("WithImpaleDPS"),
                output.num(n!("TotalDPS")) + output.num(n!("ImpaleDPS")),
            );
        }
        // L6172
        if quantityMultiplier > 1.0 {
            // L6173
            let v = output.num(n!("ImpaleDPS")) * quantityMultiplier;
            output.set(n!("ImpaleDPS"), v);
        }
        // L6175
        let v = output.num(n!("CombinedDPS")) + output.num(n!("ImpaleDPS"));
        output.set(n!("CombinedDPS"), v);
        // L6176
        if let Some(b) = breakdown.as_ref() {
            let k = n!("ImpaleDPS");
            // L6177
            b.set(k, Table::new());
            // L6178
            if skillFlags.truthy(n!("attack"))
                && skillData.truthy(n!("doubleHitsWhenDualWielding"))
                && skillFlags.truthy(n!("bothWeaponAttack"))
            {
                let mh = tab(&output, n!("MainHand"));
                let oh = tab(&output, n!("OffHand"));
                let mhv = Val::from(mainHandImpaleDPS);
                let ohv = Val::from(offHandImpaleDPS);
                // L6179
                tins(b, k, lfmt!("Main Hand:"));
                // L6180
                tins(
                    b,
                    k,
                    lua_format(
                        "%.2f ^8(MH average physical hit before mitigation)",
                        &[fmt_arg(&mh.get(n!("impaleStoredHitAvg")))],
                    ),
                );
                // L6181
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(MH chance to hit)",
                        mh.num(n!("HitChance")) / 100.0
                    ),
                );
                // L6182
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(MH impale damage multiplier)\n",
                        mh.num_or(n!("ImpaleModifier"), 1.0) - 1.0
                    ),
                );
                // L6183
                tins(b, k, lua_format("= %.2f", &[fmt_arg(&mhv)]));
                // L6184
                tins(b, k, lfmt!("Off Hand:"));
                // L6185
                tins(
                    b,
                    k,
                    lua_format(
                        "%.2f ^8(OH average physical hit before mitigation)",
                        &[fmt_arg(&oh.get(n!("impaleStoredHitAvg")))],
                    ),
                );
                // L6186
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(OH chance to hit)",
                        oh.num(n!("HitChance")) / 100.0
                    ),
                );
                // L6187
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(OH impale damage multiplier)",
                        oh.num_or(n!("ImpaleModifier"), 1.0) - 1.0
                    ),
                );
                // L6188
                tins(b, k, lua_format("= %.2f", &[fmt_arg(&ohv)]));
                // L6189
                tins(b, k, lfmt!("Combined total:"));
                // L6190
                tins(
                    b,
                    k,
                    lua_format("%.2f + %.2f", &[fmt_arg(&mhv), fmt_arg(&ohv)]),
                );
            } else {
                // L6192
                tins(
                    b,
                    k,
                    lua_format(
                        "%.2f ^8(average physical hit before mitigation)",
                        &[fmt_arg(&output.get(n!("impaleStoredHitAvg")))],
                    ),
                );
                // L6193
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(chance to hit)",
                        output.num(n!("HitChance")) / 100.0
                    ),
                );
                // L6194
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(impale damage multiplier)",
                        output.num_or(n!("ImpaleModifier"), 1.0) - 1.0
                    ),
                );
            }
            // L6196
            if skillFlags.truthy(n!("notAverage")) {
                // L6197
                let line = if output.truthy(n!("HitSpeed")) {
                    lua_format(
                        "x %.2f ^8(hit rate)",
                        &[fmt_arg(&output.get(n!("HitSpeed")))],
                    )
                } else {
                    lua_format(
                        "x %.2f ^8(%s rate)",
                        &[
                            fmt_arg(&output.get(n!("Speed"))),
                            FmtArg::Str(if skillFlags.truthy(n!("attack")) {
                                "attack"
                            } else {
                                "cast"
                            }),
                        ],
                    )
                };
                tins(b, k, line);
            }
            // L6199
            if !skillData.get(n!("dpsMultiplier")).raw_eq(&Val::Num(1.0)) {
                // L6200
                tins(
                    b,
                    k,
                    lua_format(
                        "x %g ^8(dps multiplier for this skill)",
                        &[fmt_arg(&skillData.get(n!("dpsMultiplier")))],
                    ),
                );
            }
            // L6202
            if quantityMultiplier > 1.0 {
                // L6203
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %g ^8(quantity multiplier for this skill)",
                        quantityMultiplier
                    ),
                );
            }
            // L6205
            if output.num(n!("ImpaleDuration")) <= 0.0 {
                // L6206
                tins(b, k, lfmt!("x 0 ^8(no Impale Duration)"));
            }
            // L6208
            tins(b, k, lfmt!("= %.1f", output.num(n!("ImpaleDPS"))));
        }
    }

    // L6212
    let mut bestCull = 1.0;
    // L6213
    let mirage = activeSkill.mirage.get();
    let mirageOutput = mirage.as_ref().and_then(|m| m.table(n!("output")));
    if let (Some(mirage), Some(mo)) = (
        mirage.as_ref(),
        mirageOutput.filter(|o| o.truthy(n!("TotalDPS"))),
    ) {
        // L6214
        let mirageCount = lor(mirage.get(n!("count")), || Val::Num(1.0)).num();
        // L6215
        output.set(n!("MirageDPS"), mo.num(n!("TotalDPS")) * mirageCount);
        // L6216
        let v = output.num(n!("CombinedDPS")) + mo.num(n!("TotalDPS")) * mirageCount;
        output.set(n!("CombinedDPS"), v);
        // L6217
        output.set(n!("MirageBurningGroundDPS"), mo.get(n!("BurningGroundDPS")));
        // L6218
        output.set(n!("MirageCausticGroundDPS"), mo.get(n!("CausticGroundDPS")));

        // L6220
        if mo.truthy(n!("IgniteDPS"))
            && mo.num(n!("IgniteDPS")) > output.num_or(n!("IgniteDPS"), 0.0)
        {
            // L6221
            let v = output.num(n!("MirageDPS")) + mo.num(n!("IgniteDPS"));
            output.set(n!("MirageDPS"), v);
            // L6222
            output.set(n!("IgniteDPS"), 0.0);
        }
        // L6224
        if mo.truthy(n!("BleedDPS")) && mo.num(n!("BleedDPS")) > output.num_or(n!("BleedDPS"), 0.0)
        {
            // L6225
            let v = output.num(n!("MirageDPS")) + mo.num(n!("BleedDPS"));
            output.set(n!("MirageDPS"), v);
            // L6226
            output.set(n!("BleedDPS"), 0.0);
        }

        // L6229
        if mo.truthy(n!("PoisonDPS")) {
            // L6230
            let v = output.num(n!("MirageDPS")) + mo.num(n!("PoisonDPS")) * mirageCount;
            output.set(n!("MirageDPS"), v);
            // L6231
            let v = output.num(n!("CombinedDPS")) + mo.num(n!("PoisonDPS")) * mirageCount;
            output.set(n!("CombinedDPS"), v);
        }
        // L6233
        if mo.truthy(n!("ImpaleDPS")) {
            // L6234
            let v = output.num(n!("MirageDPS")) + mo.num(n!("ImpaleDPS")) * mirageCount;
            output.set(n!("MirageDPS"), v);
            // L6235
            let v = output.num(n!("CombinedDPS")) + mo.num(n!("ImpaleDPS")) * mirageCount;
            output.set(n!("CombinedDPS"), v);
        }
        // L6237
        if mo.truthy(n!("DecayDPS")) {
            // L6238
            let v = output.num(n!("MirageDPS")) + mo.num(n!("DecayDPS"));
            output.set(n!("MirageDPS"), v);
            // L6239
            let v = output.num(n!("CombinedDPS")) + mo.num(n!("DecayDPS"));
            output.set(n!("CombinedDPS"), v);
        }
        // L6241
        if mo.truthy(n!("TotalDot"))
            && (skillFlags.truthy(n!("DotCanStack"))
                || !output.truthy(n!("TotalDot"))
                || output.get(n!("TotalDot")).raw_eq(&Val::Num(0.0)))
        {
            let stackMult = if skillFlags.truthy(n!("DotCanStack")) {
                mirageCount
            } else {
                1.0
            };
            // L6242
            let v = output.num(n!("MirageDPS")) + mo.num(n!("TotalDot")) * stackMult;
            output.set(n!("MirageDPS"), v);
            // L6243
            let stackMult = if skillFlags.truthy(n!("DotCanStack")) {
                mirageCount
            } else {
                1.0
            };
            let v = output.num(n!("CombinedDPS")) + mo.num(n!("TotalDot")) * stackMult;
            output.set(n!("CombinedDPS"), v);
        }
        // L6245
        if mo.num(n!("CullMultiplier")) > 1.0 {
            // L6246
            bestCull = mo.num(n!("CullMultiplier"));
        }
    }

    // L6250
    let TotalDotDPS = output.num_or(n!("TotalDot"), 0.0)
        + output.num_or(n!("TotalPoisonDPS"), 0.0)
        + m_max(
            output.num_or(n!("CausticGroundDPS"), 0.0),
            output.num_or(n!("MirageCausticGroundDPS"), 0.0),
        )
        + lor(
            lor(output.get(n!("TotalIgniteDPS")), || {
                output.get(n!("IgniteDPS"))
            }),
            || Val::Num(0.0),
        )
        .num()
        + m_max(
            output.num_or(n!("BurningGroundDPS"), 0.0),
            output.num_or(n!("MirageBurningGroundDPS"), 0.0),
        )
        + output.num_or(n!("BleedDPS"), 0.0)
        + output.num_or(n!("CorruptingBloodDPS"), 0.0)
        + output.num_or(n!("DecayDPS"), 0.0);
    // L6251
    output.set(n!("TotalDotDPS"), m_min(TotalDotDPS, misc.DotDpsCap));
    // L6252
    if output.num(n!("TotalDotDPS")) != TotalDotDPS {
        // L6253
        output.set(n!("showTotalDotDPS"), true);
    }
    // L6255
    if !skillData.truthy(n!("showAverage")) {
        // L6256
        let v = output.num(n!("CombinedDPS")) + output.num(n!("TotalDotDPS"));
        output.set(n!("CombinedDPS"), v);
    }

    // L6259
    bestCull = m_max(bestCull, output.num(n!("CullMultiplier")));
    // L6260
    output.set(
        n!("CullingDPS"),
        output.num(n!("CombinedDPS")) * (bestCull - 1.0),
    );
    // L6261
    output.set(
        n!("ReservationDPS"),
        output.num(n!("CombinedDPS")) * (output.num(n!("ReservationDpsMultiplier")) - 1.0),
    );
    // L6262
    output.set(
        n!("CombinedDPS"),
        output.num(n!("CombinedDPS")) * bestCull * output.num(n!("ReservationDpsMultiplier")),
    );
    // L6263
    Flow::Normal
}

#[cfg(test)]
mod tests {
    use super::*;

    fn dt(s: &str) -> Value {
        Value::Table(Arc::new(
            pob_gamedata::ValueTable::new().with(n!("damageType"), Value::Str(AStr::from(s))),
        ))
    }

    /// `string.gsub(" "..t, "%W%l", string.upper):sub(2)` on the damage types the self-hit mods
    /// carry (expected values are what Lua 5.1 patterns give).
    #[test]
    fn capitalise_damage_type() {
        assert_eq!(capitaliseDamageType(&dt("fire")).as_str(), "Fire");
        assert_eq!(capitaliseDamageType(&dt("physical")).as_str(), "Physical");
        assert_eq!(capitaliseDamageType(&dt("Chaos")).as_str(), "Chaos");
        // every lower-case letter after a non-word character is upper-cased
        assert_eq!(
            capitaliseDamageType(&dt("cold lightning")).as_str(),
            "Cold Lightning"
        );
        assert_eq!(capitaliseDamageType(&dt("")).as_str(), "");
    }
}
