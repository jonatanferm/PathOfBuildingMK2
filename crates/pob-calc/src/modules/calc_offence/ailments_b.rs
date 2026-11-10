//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence ailment loop body B, OF-H (CalcOffence.lua 4990-5648): ignite, the non-damaging
//! ailments (chill/freeze/shock/scorch/brittle/sap), knockback, enemy stun and impale.
//!
//! Owner: OF-H.
//!
//! Notes on the port:
//! * `output`, `cfg`, `breakdown`, `pass` and `ailmentMode` are the ailment loop locals
//!   (`AilmentPassLocals`, assigned by OF-G at 4168-4171 / 4289); the function-level `output` /
//!   `breakdown` are shadowed for the whole segment and never read.
//! * `globalOutput` / `globalBreakdown` are module upvalues (CalcOffence.lua:65/67, assigned at
//!   4165 by OF-G and not reassigned here): read once from `OFFENCE_STATE`.
//! * The ignite dot cfg's `skillCond` metatable (5000) is `IndexMeta::OrChain` over the
//!   `skillCfg.skillCond` / `cfg.skillCond` handles (contracts.md#index-meta), as OF-G does for
//!   bleed/poison.
//! * The loop-local functions `checkWeapon1HFlags`, `calcAverageSourceDamage` and
//!   `calcAilmentDamage` (4308-4410) belong to OF-G (`ailments_a.rs`, CCR-0035 signatures);
//!   this file calls them through the alias `closures` (CCR-0049 §1).
//! * `isValueInArray` (Common.lua:569-575) is CT's `common::isValueInArray` (CCR-0049 §3).
//! * The `ailments` dictionary (5345-5376) is iterated with `pairs` (5416); Rust keeps the
//!   constructor order Chill, Shock, Scorch, Brittle, Sap (string keys, PoB's own order is random
//!   per process, calc doc §3.4). Every iteration writes only its own ailment's keys, so the
//!   order is not observable in values (`// LUA-ORDER`).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use super::ailments_a as closures; // OF-G's loop-local functions (4308-4410)
use super::helpers_setup::{calcAilmentSourceDamage, calcResistForType};
use super::{AilmentPassLocals, OffenceCtx};
use crate::modules::calc_defence::helpers::armourReductionF;
use crate::modules::calc_perform::actor_b::actionSpeedMod;
use crate::modules::calc_tools as calcLib;
use crate::modules::common::isValueInArray;
use crate::prelude::*;

/// `dmgTypeFlags.flags.Fire` (CalcOffence.lua:41; OF-A's module-level local, immutable).
const dmgTypeFlags_Fire: u32 = 0x08;

// ---- plumbing ---------------------------------------------------------------------------------

/// `colorCodes[name]`.
fn colorCode(name: &str) -> &'static str {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| lua_error!("colorCodes.{name} is nil"))
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

/// A local declared without a value and assigned later: nil in arithmetic raises.
fn nn(v: Option<f64>) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"))
}

/// `t_insert(t[k], v)` (indexing re-reads the field, as Lua does).
fn tins(t: &Table, k: Name, v: impl Into<Val>) {
    match t.table(k) {
        Some(x) => x.push(v),
        None => lua_error!("bad argument #1 to 'insert' (table expected, got nil)"),
    }
}

/// `t[k]` where `t[k]` is then indexed (nil raises).
fn sub(t: &Table, k: Name) -> Table {
    t.table(k)
        .unwrap_or_else(|| lua_error!("attempt to index field '{}' (a nil value)", k.as_str()))
}

/// `globalOutput.HitSpeed or globalOutput.Speed`.
fn hitSpeedOrSpeed(globalOutput: &Output) -> Val {
    lor(globalOutput.get(n!("HitSpeed")), || {
        globalOutput.get(n!("Speed"))
    })
}

/// `pass.label` in a concatenation (nil raises).
fn passLabel(pass: &Table) -> LStr {
    pass.str(n!("label"))
        .unwrap_or_else(|| lua_error!("attempt to concatenate field 'label' (a nil value)"))
}

/// `cfg.skillCond` of a cfg (indexing nil raises).
fn skillCond(cfg: &ModCfg) -> Table {
    cfg.skillCond
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'skillCond' (a nil value)"))
}

/// `globalOutput` where Lua indexes it (nil raises).
fn goOrRaise(globalOutput: &Option<Output>) -> Output {
    globalOutput
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'globalOutput' (a nil value)"))
}

/// `globalBreakdown.X = …` where Lua indexes `globalBreakdown` unguarded (nil raises).
fn gbOrRaise(globalBreakdown: &Option<Breakdown>) -> Breakdown {
    globalBreakdown
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'globalBreakdown' (a nil value)"))
}

/// `ailmentData[ailment]` (indexing nil raises).
fn ailmentDataOf(ctx: &OffenceCtx, ailment: Name) -> &'static pob_gamedata::AilmentData {
    let ad = ctx
        .ailmentData
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'ailmentData' (a nil value)"));
    ad.get(&ailment)
        .unwrap_or_else(|| lua_error!("attempt to index field '{}' (a nil value)", ailment.as_str()))
}

/// A sequence constructor `{ a, b, … }` of strings.
fn seq(items: Vec<String>) -> Table {
    let t = Table::new();
    for s in items {
        t.push(s);
    }
    t
}

/// `tostring(v)` for `%s` arguments (`lj_strfmt` `%s` calls `tostring` on non-strings).
fn tostr(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => lua_error!(
            "bad argument to 'format' (a {} value has no Rust text)",
            other.lua_type()
        ),
    }
}

/// `t[k]` of a 1-based Lua sequence indexed by a number (non-integer or out of range → nil).
fn seqIndex(v: &[f64], k: f64) -> Option<f64> {
    if k.fract() == 0.0 && k >= 1.0 && k <= v.len() as f64 {
        Some(v[k as usize - 1])
    } else {
        None
    }
}

/// `1 + (skillModList:Override(dotCfg, "DotMultiplier") or skillModList:Sum("BASE", dotCfg,
/// "DotMultiplier") + skillModList:Sum("BASE", dotCfg, "FireDotMultiplier")) / 100`
/// (5141/5144/5149): one expression, evaluated left to right.
fn igniteDotMulti(skillModList: &ModStore, dotCfg: &ModCfg) -> f64 {
    let d = Some(dotCfg);
    let v = match ov(skillModList.Override(d, nm!["DotMultiplier"])) {
        Some(o) => o,
        None => {
            skillModList.Sum(ModType::BASE, d, nm!["DotMultiplier"])
                + skillModList.Sum(ModType::BASE, d, nm!["FireDotMultiplier"])
        }
    };
    1.0 + v / 100.0
}

/// The ignite dot cfg literal of 4992-5002.
fn newIgniteCfg(skillCfg: &ModCfg, cfg: &ModCfg) -> Cfg {
    let c = ModCfg::default();
    // L4993-4997 skillName = skillCfg.skillName, …
    c.skillName.set(skillCfg.skillName.get());
    c.skillPart.set(skillCfg.skillPart.get());
    c.skillTypes.set(skillCfg.skillTypes.get());
    c.summonSkillName.set(skillCfg.summonSkillName.get());
    c.slotName.set(skillCfg.slotName.get());
    // L4998 flags = bor(ModFlag.Dot, ModFlag.Ailment, band(cfg.flags, ModFlag.WeaponMask), band(cfg.flags, ModFlag.Melee) ~= 0 and ModFlag.MeleeHit or 0)
    c.flags.set(
        ModFlag::Dot
            | ModFlag::Ailment
            | (cfg.flags.get() & ModFlag::WeaponMask)
            | if cfg.flags.get() & ModFlag::Melee != 0 {
                ModFlag::MeleeHit
            } else {
                0
            },
    );
    // L4999 keywordFlags = bor(band(cfg.keywordFlags, bnot(KeywordFlag.Hit)), KeywordFlag.Ignite, KeywordFlag.Ailment, KeywordFlag.FireDot)
    c.keywordFlags.set(
        (cfg.keywordFlags.get() & !KeywordFlag::Hit)
            | KeywordFlag::Ignite
            | KeywordFlag::Ailment
            | KeywordFlag::FireDot,
    );
    // L5000 skillCond = setmetatable({["CriticalStrike"] = true }, { __index = function(table, key) return skillCfg.skillCond[key] or cfg.skillCond[key] end } )
    let sc = Table::with_meta(IndexMeta::OrChain(smallvec::smallvec![
        skillCond(skillCfg),
        skillCond(cfg)
    ]));
    sc.set(n!("CriticalStrike"), true);
    c.skillCond.set(Some(sc));
    // L5001 skillDist = skillCfg.skillDist
    c.skillDist.set(skillCfg.skillDist.get());
    Rc::new(c)
}

/// One entry of the `ailments` dictionary (5345-5376). `effect`/`thresh` are the closures
/// `K * ((damage / enemyThreshold) ^ 0.4) * effectMod` and
/// `damage * ((K * effectMod / value) ^ 2.5)` with the entry's constant `K`.
struct AilmentDef {
    name: Name,
    effList: Vec<f64>,
    k: f64,
    ramping: bool,
}

impl AilmentDef {
    fn effect(&self, enemyThreshold: f64, damage: f64, effectMod: f64) -> f64 {
        self.k * lpow(damage / enemyThreshold, 0.4) * effectMod
    }
    fn thresh(&self, damage: f64, value: f64, effectMod: f64) -> f64 {
        damage * lpow(self.k * effectMod / value, 2.5)
    }
}

// ---- segment ----------------------------------------------------------------------------------

/// @pob Modules/CalcOffence.lua:4990-5648 h=aab3d12d
/// OF-H: CalcOffence.lua 4990-5648 (incl. the loop `end`).
pub(crate) fn seg_igniteNonDmgImpale(ctx: &mut OffenceCtx, al: &mut AilmentPassLocals) -> Flow {
    let ctx: &OffenceCtx = ctx;
    let al: &AilmentPassLocals = al;
    let (globalOutput, globalBreakdown): (Option<Output>, Option<Breakdown>) =
        OFFENCE_STATE.with(|s| (s.globalOutput.get(), s.globalBreakdown.get()));

    let env = ctx.env.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let canDeal = ctx.canDeal.clone();
    let isAttack = ctx.isAttack;
    let debuffDurationMult = ctx.debuffDurationMult;
    let output = al.output.clone();
    let cfg = al.cfg.clone();
    let breakdown = al.breakdown.clone();
    let pass = al.pass.clone();
    let c: Option<&ModCfg> = Some(&cfg);
    let misc = &data().misc;
    let isCritMode = al.ailmentMode.as_ref().map(|m| m.as_str()) == Some("CRIT");

    // -- Calculate ignite chance and damage
    // L4991
    if canDeal.truthy(n!("Fire"))
        && (output.num(n!("IgniteChanceOnHit")) + output.num(n!("IgniteChanceOnCrit"))) > 0.0
    {
        let globalOutput = goOrRaise(&globalOutput);
        let isOffHand = pass.str(n!("label")).as_deref() == Some("Off Hand");
        // L4992
        let newCfg = newIgniteCfg(&skillCfg, &cfg);
        if !isOffHand {
            activeSkill.igniteCfg.set(Some(newCfg));
        } else {
            activeSkill.OHigniteCfg.set(Some(newCfg));
        }
        // L5003
        let dotCfg: Cfg = if !isOffHand {
            activeSkill.igniteCfg.get()
        } else {
            activeSkill.OHigniteCfg.get()
        }
        .unwrap_or_else(|| lua_error!("attempt to index local 'dotCfg' (a nil value)"));
        let d: Option<&ModCfg> = Some(&dotCfg);
        // L5004
        closures::checkWeapon1HFlags(ctx, al, &dotCfg);
        // L5005
        let mut sourceHitDmg: Option<f64> = None;
        let mut sourceCritDmg: Option<f64> = None;
        let mut sourceMaxCritDmg: Option<f64> = None;
        let mut sourceMinHitDmg: Option<f64> = None;
        let mut sourceMaxHitDmg: Option<f64> = None;
        // L5006
        if let Some(b) = &breakdown {
            // L5007
            b.set(n!("IgnitePhysical"), tbl!(damageTypes = Table::new()));
            // L5008
            b.set(n!("IgniteLightning"), tbl!(damageTypes = Table::new()));
            // L5009
            b.set(n!("IgniteCold"), tbl!(damageTypes = Table::new()));
            // L5010
            b.set(n!("IgniteFire"), tbl!(damageTypes = Table::new()));
            // L5011
            b.set(n!("IgniteChaos"), tbl!(damageTypes = Table::new()));
        }

        // L5014
        let v = output.num(n!("IgniteChanceOnHit")) * (1.0 - output.num(n!("CritChance")) / 100.0)
            + output.num(n!("IgniteChanceOnCrit")) * output.num(n!("CritChance")) / 100.0;
        globalOutput.set(n!("IgniteChancePerHit"), v);

        // -- For ignites we will be using a weighted average calculation
        // L5017
        let mut maxStacks = 1.0;
        // L5018
        if skillFlags.truthy(n!("igniteCanStack")) {
            // L5019
            maxStacks = match ov(skillModList.Override(c, nm!["IgniteStacks"])) {
                Some(v) => v,
                None => maxStacks + skillModList.Sum(ModType::BASE, c, nm!["IgniteStacks"]),
            };
        }
        // L5021
        let overrideStackPotential: Option<f64> =
            if ov(skillModList.Override(None, nm!["IgniteStackPotentialOverride"])).is_some() {
                Some(
                    nn(ov(skillModList.Override(None, nm!["IgniteStackPotentialOverride"])))
                        / maxStacks,
                )
            } else {
                None
            };
        // L5022
        globalOutput.set(n!("IgniteStacksMax"), maxStacks);

        // L5024
        let rateMod = (calcLib::r#mod(&skillModList, c, nm!["IgniteBurnFaster"])
            + enemyDB.Sum(ModType::INC, None, nm!["SelfIgniteBurnFaster"]) / 100.0)
            / calcLib::r#mod(&skillModList, c, nm!["IgniteBurnSlower"]);
        // L5025
        let durationBase = match ov(skillModList.Override(d, nm!["IgniteDurationBase"])) {
            Some(v) => v,
            None => misc.IgniteDurationBase,
        };
        // L5026
        let mut durationMod = m_max(
            calcLib::r#mod(
                &skillModList,
                d,
                nm![
                    "EnemyIgniteDuration",
                    "EnemyAilmentDuration",
                    "EnemyElementalAilmentDuration",
                    "DamagingAilmentDuration"
                ],
            ) * calcLib::r#mod(
                &enemyDB,
                None,
                nm![
                    "SelfIgniteDuration",
                    "SelfAilmentDuration",
                    "SelfElementalAilmentDuration"
                ],
            ),
            0.0,
        );
        // L5027
        durationMod = m_max(durationMod, 0.0);
        // L5028
        globalOutput.set(
            n!("IgniteDuration"),
            durationBase * durationMod / rateMod * debuffDurationMult,
        );
        // -- The chance any given hit applies ignite
        // L5030
        let igniteChance = output.num(n!("IgniteChanceOnHit")) / 100.0
            * (1.0 - output.num(n!("CritChance")) / 100.0)
            + output.num(n!("IgniteChanceOnCrit")) / 100.0 * output.num(n!("CritChance")) / 100.0;
        // -- The average number of ignites that will be active on the enemy at once
        // L5032
        let mut igniteStacks =
            output.num(n!("HitChance")) / 100.0 * igniteChance * skillData.num(n!("dpsMultiplier"));
        // L5033
        if !skillData.truthy(n!("triggeredOnDeath")) {
            // L5034
            if output.truthy(n!("Cooldown")) {
                // L5035
                igniteStacks = igniteStacks * globalOutput.num(n!("IgniteDuration"))
                    / m_max(
                        output.num(n!("Cooldown")),
                        lor(output.get(n!("HitTime")), || output.get(n!("Time"))).num(),
                    );
            } else if hitSpeedOrSpeed(&globalOutput).num() > 0.0 {
                // --assume skills with no cast, attack, or cooldown time are single cast
                // L5038
                igniteStacks = igniteStacks
                    * globalOutput.num(n!("IgniteDuration"))
                    * hitSpeedOrSpeed(&globalOutput).num();
            }
        }
        // -- ratio of ignites applied : max effective ignites
        // L5042
        globalOutput.set(
            n!("IgniteStackPotential"),
            match overrideStackPotential {
                Some(v) => v,
                None => igniteStacks / maxStacks,
            },
        );
        // L5043
        if let Some(gb) = &globalBreakdown {
            // L5044
            gb.set(
                n!("IgniteStackPotential"),
                seq(vec![
                    lfmt!(&format!(
                        "{}NOTE: Calculation uses a Weighted Avg formula",
                        colorCode("CUSTOM")
                    )),
                    lfmt!(""),
                ]),
            );
            let k = n!("IgniteStackPotential");
            // L5048
            if ov(skillModList.Override(None, nm!["IgniteStackPotentialOverride"])).is_some() {
                // L5049
                if maxStacks != 1.0 {
                    // L5050
                    tins(
                        gb,
                        k,
                        lfmt!(
                            "= %d / %d ^8(stack potential override / max ignite stacks)",
                            nn(ov(skillModList
                                .Override(None, nm!["IgniteStackPotentialOverride"]))),
                            maxStacks
                        ),
                    );
                    // L5051
                    tins(
                        gb,
                        k,
                        lfmt!("= %g ^8(stack potential)", overrideStackPotential),
                    );
                } else {
                    // L5053
                    tins(
                        gb,
                        k,
                        lfmt!("= %g ^8(stack potential override)", overrideStackPotential),
                    );
                }
            } else {
                // L5056
                tins(
                    gb,
                    k,
                    lfmt!("%.2f ^8(chance to hit)", output.num(n!("HitChance")) / 100.0),
                );
                // L5057
                tins(gb, k, lfmt!("* %.2f ^8(chance to ignite)", igniteChance));
                // L5058
                if skillData.truthy(n!("triggeredOnDeath")) {
                    // L5059
                    tins(gb, k, lfmt!("* 1 ^8Cast on Death override"));
                } else if output.truthy(n!("Cooldown")) {
                    // L5061
                    tins(
                        gb,
                        k,
                        lfmt!(
                            "* (%.2f / max(%.2f, %.2f) ^8(Ignite Duration / max(Cooldown, Cast Time))",
                            globalOutput.num(n!("IgniteDuration")),
                            output.num(n!("Cooldown")),
                            lor(output.get(n!("HitTime")), || output.get(n!("Time"))).num()
                        ),
                    );
                } else if hitSpeedOrSpeed(&globalOutput).num() > 0.0 {
                    // L5063
                    tins(
                        gb,
                        k,
                        lfmt!(
                            "* (%.2f / %.2f) ^8(Ignite Duration / Attack Time)",
                            globalOutput.num(n!("IgniteDuration")),
                            lor(globalOutput.get(n!("HitTime")), || output.get(n!("Time"))).num()
                        ),
                    );
                }
                // L5065
                if skillData.num(n!("dpsMultiplier")) != 1.0 {
                    // L5066
                    tins(
                        gb,
                        k,
                        lfmt!(
                            "* %g ^8(DPS multiplier for this skill)",
                            skillData.num(n!("dpsMultiplier"))
                        ),
                    );
                }
                // L5068
                tins(gb, k, lfmt!("/ %d ^8(max number of stacks)", maxStacks));
                // L5069
                tins(
                    gb,
                    k,
                    lfmt!("= %.2f", globalOutput.num(n!("IgniteStackPotential"))),
                );
            }
        }

        // -- the amount of damage each ignite does as % maximum
        // L5074
        let igniteRollAverage: f64;
        // L5075
        if globalOutput.num(n!("IgniteStackPotential")) > 1.0 {
            // -- shift damage towards top of range as only top ignites apply
            // L5077
            igniteRollAverage =
                (igniteStacks - (maxStacks - 1.0) / 2.0) / (igniteStacks + 1.0) * 100.0;
        } else {
            // -- assume middle of range for hit damage
            // L5080
            igniteRollAverage = 50.0;
        }
        // L5082
        globalOutput.set(n!("IgniteRollAverage"), igniteRollAverage);
        // L5083
        if let Some(gb) = &globalBreakdown {
            // L5084
            gb.set(
                n!("IgniteRollAverage"),
                seq(vec![
                    lfmt!(&format!(
                        "{}This is the average roll of an ignite affecting the enemy if you are constantly attacking",
                        colorCode("CUSTOM")
                    )),
                    lfmt!(
                        &format!(
                            "{}If attacking constantly, your average strongest Ignite currently achieves ^7%.2f%%{} of its max damage",
                            colorCode("CUSTOM"),
                            colorCode("CUSTOM")
                        ),
                        igniteRollAverage
                    ),
                    lfmt!(""),
                    lfmt!("Average Ignite Roll:"),
                ]),
            );
            let k = n!("IgniteRollAverage");
            // L5090
            if igniteStacks >= maxStacks {
                // L5091
                tins(
                    gb,
                    k,
                    lfmt!("%.2f - (%.2f - 1)/2", igniteStacks, maxStacks),
                );
                // L5092
                tins(gb, k, lfmt!("/ (%.2f + 1)", igniteStacks));
                // L5093
                tins(gb, k, lfmt!("= %.2f%%", igniteRollAverage));
            } else {
                // L5095
                tins(
                    gb,
                    k,
                    lfmt!("50%% (averaging <= %d ignites)", maxStacks),
                );
            }
        }

        // L5099
        for sub_pass in 1..=2 {
            // L5100
            skillCond(&dotCfg).set(n!("CriticalStrike"), sub_pass != 1);

            // L5102
            let mut totalMin = 0.0;
            let mut totalMax = 0.0;
            // `sub_pass == 1 and breakdown and breakdown.IgniteX` (false/nil both falsy)
            let bdArg = |k: Name| -> Option<Table> {
                if sub_pass == 1 {
                    breakdown.as_ref().and_then(|b| b.table(k))
                } else {
                    None
                }
            };
            // L5103
            if canDeal.truthy(n!("Physical")) && skillModList.Flag(c, nm!["PhysicalCanIgnite"]) {
                // L5104
                let (min, max) = calcAilmentSourceDamage(
                    &activeSkill,
                    &output,
                    &dotCfg,
                    bdArg(n!("IgnitePhysical")).as_ref(),
                    n!("Physical"),
                    dmgTypeFlags_Fire,
                );
                // L5105
                output.set(n!("IgnitePhysicalMin"), min);
                // L5106
                output.set(n!("IgnitePhysicalMax"), max);
                // L5107
                totalMin = totalMin + min;
                // L5108
                totalMax = totalMax + max;
            }
            // L5110
            if canDeal.truthy(n!("Lightning")) && skillModList.Flag(c, nm!["LightningCanIgnite"])
            {
                // L5111
                let (min, max) = calcAilmentSourceDamage(
                    &activeSkill,
                    &output,
                    &dotCfg,
                    bdArg(n!("IgniteLightning")).as_ref(),
                    n!("Lightning"),
                    dmgTypeFlags_Fire,
                );
                // L5112
                output.set(n!("IgniteLightningMin"), min);
                // L5113
                output.set(n!("IgniteLightningMax"), max);
                // L5114
                totalMin = totalMin + min;
                // L5115
                totalMax = totalMax + max;
            }
            // L5117
            if canDeal.truthy(n!("Cold")) && skillModList.Flag(c, nm!["ColdCanIgnite"]) {
                // L5118
                let (min, max) = calcAilmentSourceDamage(
                    &activeSkill,
                    &output,
                    &dotCfg,
                    bdArg(n!("IgniteCold")).as_ref(),
                    n!("Cold"),
                    dmgTypeFlags_Fire,
                );
                // L5119
                output.set(n!("IgniteColdMin"), min);
                // L5120
                output.set(n!("IgniteColdMax"), max);
                // L5121
                totalMin = totalMin + min;
                // L5122
                totalMax = totalMax + max;
            }
            // L5124
            if canDeal.truthy(n!("Fire")) && !skillModList.Flag(c, nm!["FireCannotIgnite"]) {
                // L5125
                let (min, max) = calcAilmentSourceDamage(
                    &activeSkill,
                    &output,
                    &dotCfg,
                    bdArg(n!("IgniteFire")).as_ref(),
                    n!("Fire"),
                    0,
                );
                // L5126
                output.set(n!("IgniteFireMin"), min);
                // L5127
                output.set(n!("IgniteFireMax"), max);
                // L5128
                totalMin = totalMin + min;
                // L5129
                totalMax = totalMax + max;
            }
            // L5131
            if canDeal.truthy(n!("Chaos")) && skillModList.Flag(c, nm!["ChaosCanIgnite"]) {
                // L5132
                let (min, max) = calcAilmentSourceDamage(
                    &activeSkill,
                    &output,
                    &dotCfg,
                    bdArg(n!("IgniteChaos")).as_ref(),
                    n!("Chaos"),
                    dmgTypeFlags_Fire,
                );
                // L5133
                output.set(n!("IgniteChaosMin"), min);
                // L5134
                output.set(n!("IgniteChaosMax"), max);
                // L5135
                totalMin = totalMin + min;
                // L5136
                totalMax = totalMax + max;
            }
            // L5138
            if sub_pass == 2 {
                // L5139
                if modDB.Flag(None, nm!["AilmentsAreNeverFromCrit"]) {
                    // L5140 -- force config to non-crit for dotMulti calculation
                    skillCond(&dotCfg).set(n!("CriticalStrike"), false);
                    // L5141
                    output.set(
                        n!("CritIgniteDotMulti"),
                        igniteDotMulti(&skillModList, &dotCfg),
                    );
                    // L5142 -- reset to true to avoid unintended side effects
                    skillCond(&dotCfg).set(n!("CriticalStrike"), true);
                } else {
                    // L5144
                    output.set(
                        n!("CritIgniteDotMulti"),
                        igniteDotMulti(&skillModList, &dotCfg),
                    );
                }
                // L5146
                sourceCritDmg = Some(
                    (totalMin + (totalMax - totalMin) * igniteRollAverage / 100.0)
                        * output.num(n!("CritIgniteDotMulti")),
                );
                // L5147
                sourceMaxCritDmg = Some(totalMax * output.num(n!("CritIgniteDotMulti")));
            } else {
                // L5149
                output.set(n!("IgniteDotMulti"), igniteDotMulti(&skillModList, &dotCfg));
                // L5150
                sourceHitDmg = Some(
                    (totalMin + (totalMax - totalMin) * igniteRollAverage / 100.0)
                        * output.num(n!("IgniteDotMulti")),
                );
                // L5151
                sourceMinHitDmg = Some(totalMin * output.num(n!("IgniteDotMulti")));
                // L5152
                sourceMaxHitDmg = Some(totalMax * output.num(n!("IgniteDotMulti")));
            }
            // L5154
            output.set(n!("IgniteTotalMin"), totalMin);
            // L5155
            output.set(n!("IgniteTotalMax"), totalMax);
        }
        // L5157
        if let Some(gb) = &globalBreakdown {
            let derivation = || {
                lfmt!(
                    "%.0f + (%.0f - %.0f) * %.2f%%",
                    output.num(n!("IgniteTotalMin")),
                    output.num(n!("IgniteTotalMax")),
                    output.num(n!("IgniteTotalMin")),
                    igniteRollAverage
                )
            };
            // L5158
            if sourceHitDmg == sourceCritDmg || output.num(n!("CritChance")) == 0.0 {
                // L5159
                gb.set(
                    n!("IgniteDPS"),
                    seq(vec![
                        lfmt!("Dmg Derivation:"),
                        derivation(),
                        lfmt!("^8min combined sources + (max combined sources - min combined sources) * average ignite roll"),
                        lfmt!("* %.2f ^8(Ignite DoT Multi)", output.num(n!("IgniteDotMulti"))),
                        lfmt!("= %.2f", nn(sourceHitDmg)),
                    ]),
                );
            } else {
                // L5167
                gb.set(
                    n!("IgniteDPS"),
                    seq(vec![
                        lfmt!("Non-Crit Dmg Derivation:"),
                        derivation(),
                        lfmt!("^8min combined sources + (max combined sources - min combined sources) * average ignite roll"),
                        lfmt!(
                            "* %.2f ^8(Ignite DoT Multi for Non-Crit)",
                            output.num(n!("IgniteDotMulti"))
                        ),
                        lfmt!("= %.2f", nn(sourceHitDmg)),
                        lfmt!(""),
                        lfmt!("Crit Dmg Derivation:"),
                        derivation(),
                        lfmt!("^8min combined sources + (max combined sources - min combined sources) * average ignite roll"),
                        lfmt!(
                            "* %.2f ^8(Ignite DoT Multi for Crit)",
                            output.num(n!("CritIgniteDotMulti"))
                        ),
                        lfmt!("= %.2f", nn(sourceCritDmg)),
                    ]),
                );
            }
        }
        // -- over-stacking ignite stacks increases the chance a critical ignite is present
        // L5183
        let ailmentCritChance = 100.0
            * (1.0 - m_pow(1.0 - output.num(n!("CritChance")) / 100.0, m_max(1.0, igniteStacks)));
        // L5184
        let baseMinVal = closures::calcAilmentDamage(
            ctx,
            al,
            n!("Ignite"),
            ailmentCritChance,
            nn(sourceMinHitDmg),
            0.0,
            true,
        ) * misc.IgnitePercentBase;
        // L5185
        let baseMaxVal = closures::calcAilmentDamage(
            ctx,
            al,
            n!("Ignite"),
            100.0,
            nn(sourceMaxHitDmg),
            nn(sourceMaxCritDmg),
            true,
        ) * misc.IgnitePercentBase
            * output.num(n!("RuthlessBlowAilmentEffect"))
            * output.num(n!("FistOfWarDamageEffect"))
            * globalOutput.num(n!("AilmentWarcryEffect"));
        // L5186
        let baseVal = closures::calcAilmentDamage(
            ctx,
            al,
            n!("Ignite"),
            ailmentCritChance,
            nn(sourceHitDmg),
            nn(sourceCritDmg),
            false,
        ) * misc.IgnitePercentBase
            * output.num(n!("RuthlessBlowAilmentEffect"))
            * output.num(n!("FistOfWarDamageEffect"))
            * globalOutput.num(n!("AilmentWarcryEffect"));
        // L5187
        if baseVal > 0.0 {
            // L5188
            skillFlags.set(n!("ignite"), true);
            // L5189
            let mut effMult = 1.0;
            // L5190
            if env.mode_effective.get() {
                // L5191
                if skillModList.Flag(c, nm!["IgniteToChaos"]) {
                    // L5192
                    let resist = calcResistForType(ctx, n!("Chaos"), d);
                    // L5193
                    let takenInc = enemyDB.Sum(
                        ModType::INC,
                        d,
                        nm![
                            "DamageTaken",
                            "DamageTakenOverTime",
                            "ChaosDamageTaken",
                            "ChaosDamageTakenOverTime"
                        ],
                    );
                    // L5194
                    let takenMore = enemyDB.More(
                        d,
                        nm![
                            "DamageTaken",
                            "DamageTakenOverTime",
                            "ChaosDamageTaken",
                            "ChaosDamageTakenOverTime"
                        ],
                    );
                    // L5195
                    effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
                    // L5196
                    globalOutput.set(n!("IgniteEffMult"), effMult);
                    // L5197
                    if let Some(b) = breakdown.as_ref().filter(|_| effMult != 1.0) {
                        // L5198
                        let sourceRes = if env.modDB.get().Flag(None, nm!["EnemyChaosResistEqualToYours"]) {
                            n!("Your Chaos Resistance")
                        } else if partyModDBFlag(&env, n!("EnemyChaosResistEqualToYours")) {
                            n!("Party Member Chaos Resistance")
                        } else {
                            n!("Chaos")
                        };
                        // L5199
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
                        gbOrRaise(&globalBreakdown).set(n!("IgniteEffMult"), t);
                    }
                } else {
                    // L5202
                    let resist = calcResistForType(ctx, n!("Fire"), d);
                    // L5203
                    let takenInc = enemyDB.Sum(
                        ModType::INC,
                        d,
                        nm![
                            "DamageTaken",
                            "DamageTakenOverTime",
                            "FireDamageTaken",
                            "FireDamageTakenOverTime",
                            "ElementalDamageTaken"
                        ],
                    );
                    // L5204
                    let takenMore = enemyDB.More(
                        d,
                        nm![
                            "DamageTaken",
                            "DamageTakenOverTime",
                            "FireDamageTaken",
                            "FireDamageTakenOverTime",
                            "ElementalDamageTaken"
                        ],
                    );
                    // L5205
                    effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
                    // L5206
                    globalOutput.set(n!("IgniteEffMult"), effMult);
                    // L5207
                    if let Some(b) = breakdown.as_ref().filter(|_| effMult != 1.0) {
                        // L5208
                        let sourceRes = if env.modDB.get().Flag(None, nm!["EnemyFireResistEqualToYours"]) {
                            n!("Your Fire Resistance")
                        } else if partyModDBFlag(&env, n!("EnemyFireResistEqualToYours")) {
                            n!("Party Member Fire Resistance")
                        } else {
                            n!("Fire")
                        };
                        // L5209
                        let t = b.effMult(
                            n!("Fire"),
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
                        gbOrRaise(&globalBreakdown).set(n!("IgniteEffMult"), t);
                    }
                }
            }
            // L5213
            let effectMod = calcLib::r#mod(&skillModList, d, nm!["AilmentEffect"]);
            // L5214
            let activeIgnites = m_min(igniteStacks, maxStacks);
            // L5215
            let IgniteDPSUncapped = baseVal * effectMod * rateMod * activeIgnites * effMult;
            // L5216
            let IgniteDPSCapped = m_min(IgniteDPSUncapped, misc.DotDpsCap);
            // L5217
            let MinIgniteDPSUncapped = baseMinVal * effectMod * rateMod * activeIgnites * effMult;
            // L5218
            let MinIgniteDPSCapped = m_min(MinIgniteDPSUncapped, misc.DotDpsCap);
            // L5219
            let MaxIgniteDPSUncapped = baseMaxVal * effectMod * rateMod * activeIgnites * effMult;
            // L5220
            let MaxIgniteDPSCapped = m_min(MaxIgniteDPSUncapped, misc.DotDpsCap);
            // L5221
            output.set(n!("IgniteDPS"), IgniteDPSCapped);
            // L5222
            let groundMult = m_max(
                skillModList
                    .Max(None, nm!["IgniteDpsAsBurningGround"])
                    .unwrap_or(0.0),
                enemyDB
                    .Max(None, nm!["IgniteDpsAsBurningGround"])
                    .unwrap_or(0.0),
            );
            // L5223
            if groundMult > 0.0 {
                // -- Always use fire eff multi
                // L5225
                let resist = calcResistForType(ctx, n!("Fire"), d);
                // L5226
                let takenInc = enemyDB.Sum(
                    ModType::INC,
                    d,
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "FireDamageTaken",
                        "FireDamageTakenOverTime",
                        "ElementalDamageTaken"
                    ],
                );
                // L5227
                let takenMore = enemyDB.More(
                    d,
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "FireDamageTaken",
                        "FireDamageTakenOverTime",
                        "ElementalDamageTaken"
                    ],
                );
                // L5228
                let fireEffMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
                // L5229
                let BurningGroundDPSUncapped =
                    baseVal * effectMod * rateMod * fireEffMult * groundMult / 100.0;
                // L5230
                let BurningGroundDPSCapped = m_min(BurningGroundDPSUncapped, misc.DotDpsCap);
                // L5231
                globalOutput.set(n!("BurningGroundDPS"), BurningGroundDPSCapped);
                // L5232
                globalOutput.set(n!("BurningGroundFromIgnite"), true);
                // L5233
                if let Some(gb) = &globalBreakdown {
                    // L5234
                    gb.set(
                        n!("BurningGroundDPS"),
                        seq(vec![
                            lfmt!(
                                "%.1f ^8(ignite damage per second)",
                                baseVal * effectMod * rateMod
                            ),
                            lfmt!("* %.1f%% ^8(percent as burning ground)", groundMult),
                            lfmt!("* %.3f ^8(effective DPS modifier)", fireEffMult),
                            lfmt!(
                                "= %.1f ^8per second",
                                globalOutput.num(n!("BurningGroundDPS"))
                            ),
                        ]),
                    );
                }
            }
            // L5242
            globalOutput.set(
                n!("IgniteDamage"),
                output.num(n!("IgniteDPS")) * globalOutput.num(n!("IgniteDuration")),
            );
            // L5243
            if skillFlags.truthy(n!("igniteCanStack")) {
                // L5244
                output.set(
                    n!("IgniteDamage"),
                    output.num(n!("IgniteDPS")) * globalOutput.num(n!("IgniteDuration")),
                );
                // L5245
                output.set(n!("IgniteStacksMax"), maxStacks);
                // L5246
                output.set(n!("TotalIgniteDPS"), output.get(n!("IgniteDPS")));
            }

            // L5249
            if let Some(b) = &breakdown {
                let k = n!("IgniteDPS");
                let neg = colorCode("NEGATIVE");
                // L5250
                if output.num(n!("RuthlessBlowAilmentEffect")) != 1.0 {
                    // L5251
                    tins(
                        b,
                        k,
                        lfmt!(
                            "x %.2f ^8(ruthless blow effect modifier)",
                            output.num(n!("RuthlessBlowAilmentEffect"))
                        ),
                    );
                }
                // L5253
                tins(b, k, "x 0.9 ^8(ignite deals 90% per second)");
                // L5254 (s_format("= %.1f", baseVal, 1): the extra argument is ignored)
                tins(b, k, lfmt!("= %.1f", baseVal));
                // L5255
                tins(b, k, "");
                // L5256
                tins(b, k, "Average Ignite DPS:");
                // L5257
                if baseVal != IgniteDPSUncapped {
                    // L5258
                    tins(b, k, lfmt!("%.1f ^8(base damage per second)", baseVal));
                }
                // L5260
                if effectMod != 1.0 {
                    // L5261
                    tins(b, k, lfmt!("x %.2f ^8(ailment effect modifier)", effectMod));
                }
                // L5263
                if rateMod != 1.0 {
                    // L5264
                    tins(b, k, lfmt!("x %.2f ^8(burn rate modifier)", rateMod));
                }
                // L5266
                if activeIgnites != 1.0 {
                    // L5267
                    tins(b, k, lfmt!("x %.2f ^8(avg ignite stacks)", activeIgnites));
                }
                // L5269
                if effMult != 1.0 {
                    // L5270
                    tins(
                        b,
                        k,
                        lfmt!("x %.3f ^8(effective DPS modifier from enemy debuffs)", effMult),
                    );
                }
                // L5272
                if output.num(n!("IgniteDPS")) != IgniteDPSUncapped {
                    // L5273
                    tins(
                        b,
                        k,
                        lfmt!("= %.1f ^8(Uncapped raw Ignite DPS)", IgniteDPSUncapped),
                    );
                    // L5274
                    tins(
                        b,
                        k,
                        lfmt!(
                            &format!(
                                "^8(Raw Ignite DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"
                            ),
                            IgniteDPSUncapped - IgniteDPSCapped,
                            (IgniteDPSUncapped - IgniteDPSCapped) / IgniteDPSCapped * 100.0
                        ),
                    );
                    // L5275
                    tins(b, k, lfmt!("= %d ^8(Capped Ignite DPS)", IgniteDPSCapped));
                } else {
                    // L5277
                    tins(
                        b,
                        k,
                        lfmt!("= %.1f ^8per second", output.num(n!("IgniteDPS"))),
                    );
                }
                // L5279
                tins(
                    b,
                    k,
                    lfmt!(
                        "%.2f%% of Maximum Ignite DPS",
                        output.num(n!("IgniteDPS")) / MaxIgniteDPSCapped * 100.0
                    ),
                );
                // L5280
                tins(b, k, "");
                // L5281
                tins(b, k, "DPS Range:");
                // L5282
                if MaxIgniteDPSCapped == MaxIgniteDPSUncapped
                    && MinIgniteDPSCapped == MinIgniteDPSUncapped
                {
                    // L5283
                    tins(
                        b,
                        k,
                        lfmt!(
                            "%.0f to %.0f ^8(Ignite DPS Range)",
                            MinIgniteDPSUncapped,
                            MaxIgniteDPSUncapped
                        ),
                    );
                } else {
                    // L5285
                    tins(
                        b,
                        k,
                        lfmt!(
                            "%.0f to %.0f ^8(Uncapped Ignite DPS Range)",
                            MinIgniteDPSUncapped,
                            MaxIgniteDPSUncapped
                        ),
                    );
                }
                // L5287
                if MinIgniteDPSCapped != MinIgniteDPSUncapped {
                    // L5288
                    tins(
                        b,
                        k,
                        lfmt!(
                            &format!(
                                "^8(Raw Min Ignite DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"
                            ),
                            MinIgniteDPSUncapped - MinIgniteDPSCapped,
                            (MinIgniteDPSUncapped - MinIgniteDPSCapped) / MinIgniteDPSCapped
                                * 100.0
                        ),
                    );
                }
                // L5290
                if MaxIgniteDPSCapped != MaxIgniteDPSUncapped {
                    // L5291
                    tins(
                        b,
                        k,
                        lfmt!(
                            &format!(
                                "^8(Raw Max Ignite DPS is {neg}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)"
                            ),
                            MaxIgniteDPSUncapped - MaxIgniteDPSCapped,
                            (MaxIgniteDPSUncapped - MaxIgniteDPSCapped) / MaxIgniteDPSCapped
                                * 100.0
                        ),
                    );
                }
                // L5293
                if MaxIgniteDPSCapped != MaxIgniteDPSUncapped
                    || MinIgniteDPSCapped != MinIgniteDPSUncapped
                {
                    // L5294
                    tins(
                        b,
                        k,
                        lfmt!(
                            "%.0f to %.0f ^8(Capped Ignite DPS Range)",
                            MinIgniteDPSCapped,
                            MaxIgniteDPSCapped
                        ),
                    );
                }
                // L5296
                if output.truthy(n!("CritIgniteDotMulti"))
                    && !output
                        .get(n!("CritIgniteDotMulti"))
                        .raw_eq(&output.get(n!("IgniteDotMulti")))
                {
                    // L5297
                    let chanceFromHit = output.num(n!("IgniteChanceOnHit")) / 100.0
                        * (1.0 - globalOutput.num(n!("CritChance")) / 100.0);
                    // L5298
                    let chanceFromCrit =
                        output.num(n!("IgniteChanceOnCrit")) / 100.0 * ailmentCritChance / 100.0;
                    // L5299
                    let totalFromHit = chanceFromHit / (chanceFromHit + chanceFromCrit);
                    // L5300
                    let totalFromCrit = chanceFromCrit / (chanceFromHit + chanceFromCrit);
                    // L5301
                    let t = b.critDot(
                        output.num(n!("IgniteDotMulti")),
                        output.num(n!("CritIgniteDotMulti")),
                        totalFromHit,
                        totalFromCrit,
                    );
                    b.set(n!("IgniteDotMulti"), t);
                    // L5302
                    let v = (output.num(n!("IgniteDotMulti")) * totalFromHit)
                        + (output.num(n!("CritIgniteDotMulti")) * totalFromCrit);
                    output.set(n!("IgniteDotMulti"), v);
                }
                // L5304
                if skillFlags.truthy(n!("igniteCanStack")) {
                    let k = n!("IgniteDamage");
                    // L5305
                    b.set(k, Table::new());
                    // L5306
                    if isAttack {
                        // L5307
                        tins(b, k, format!("{}:", passLabel(&pass)));
                    }
                    // L5309
                    tins(
                        b,
                        k,
                        lfmt!("%.1f ^8(damage per second)", output.num(n!("IgniteDPS"))),
                    );
                    // L5310
                    tins(
                        b,
                        k,
                        lfmt!(
                            "x %.2fs ^8(ignite duration)",
                            globalOutput.num(n!("IgniteDuration"))
                        ),
                    );
                    // L5311
                    tins(
                        b,
                        k,
                        lfmt!(
                            "= %.1f ^8damage per ignite stack",
                            output.num(n!("IgniteDamage"))
                        ),
                    );
                }
                // L5313
                if globalOutput.num(n!("IgniteDuration")) != misc.IgniteDurationBase {
                    let gb = gbOrRaise(&globalBreakdown);
                    let k = n!("IgniteDuration");
                    // L5314
                    gb.set(
                        k,
                        seq(vec![lfmt!("%.2fs ^8(base duration)", durationBase)]),
                    );
                    // L5317
                    if durationMod != 1.0 {
                        // L5318
                        tins(&gb, k, lfmt!("x %.2f ^8(duration modifier)", durationMod));
                    }
                    // L5320
                    if rateMod != 1.0 {
                        // L5321
                        tins(&gb, k, lfmt!("/ %.2f ^8(burn rate modifier)", rateMod));
                    }
                    // L5323
                    if debuffDurationMult != 1.0 {
                        // L5324
                        tins(
                            &gb,
                            k,
                            lfmt!(
                                "/ %.2f ^8(debuff expires slower/faster)",
                                1.0 / debuffDurationMult
                            ),
                        );
                    }
                    // L5326
                    tins(
                        &gb,
                        k,
                        lfmt!("= %.2fs", globalOutput.num(n!("IgniteDuration"))),
                    );
                }
            }
        }
    }

    // -- Calculate non-damaging ailments effect and duration modifiers
    // L5333
    let isBoss = !matches!(env.configInput.get(n!("enemyIsBoss")), Val::Str(ref s) if &**s == "None");
    let enemyLevel = env.enemyLevel.get();
    // L5334
    let enemyAilmentThreshold = seqIndex(&data().monsterAilmentThresholdTable, enemyLevel);
    // L5335
    let mut enemyMapLifeMult: Option<f64> = Some(1.0);
    // L5336
    let mut enemyMapAilmentMult: Option<f64> = Some(1.0);
    // L5337
    if enemyLevel >= 66.0 {
        // L5338
        let boss = if isBoss {
            seqIndex(&data().mapLevelBossLifeMult, enemyLevel)
        } else {
            None
        };
        enemyMapLifeMult = boss.or_else(|| {
            if enemyLevel.fract() == 0.0 {
                data().mapLevelLifeMult.get(&(enemyLevel as i64)).copied()
            } else {
                None
            }
        });
        // L5339
        let bossA = if isBoss {
            seqIndex(&data().mapLevelBossAilmentMult, enemyLevel)
        } else {
            None
        };
        enemyMapAilmentMult = bossA.or(enemyMapAilmentMult);
    }
    // L5341
    let enemyTypeMult = if isBoss { 7.68 } else { 1.0 };
    // L5342
    let enemyThreshold = nn(enemyAilmentThreshold) * enemyTypeMult * nn(enemyMapLifeMult)
        * enemyDB.More(None, nm!["Life"])
        * nn(enemyMapAilmentMult)
        * enemyDB.More(None, nm!["AilmentThreshold"]);

    // -- Defaulting ailments dictionary | Done to organize non-damaging ailments by type and have standardized effect and threshold calcs
    // L5345
    let mut ailments: Vec<AilmentDef> = vec![
        // L5346
        AilmentDef {
            name: n!("Chill"),
            effList: vec![10.0, 20.0],
            k: 50.0,
            ramping: lor(output.get(n!("HasBonechill")), || Val::Bool(false)).truthy(),
        },
        // L5352
        AilmentDef {
            name: n!("Shock"),
            effList: vec![10.0, 20.0, 40.0],
            k: 50.0,
            ramping: true,
        },
        // L5358
        AilmentDef {
            name: n!("Scorch"),
            effList: vec![5.0, 10.0, 20.0],
            k: 50.0,
            ramping: true,
        },
        // L5364
        AilmentDef {
            name: n!("Brittle"),
            effList: vec![2.0, 4.0],
            k: 10.0,
            ramping: true,
        },
        // L5370
        AilmentDef {
            name: n!("Sap"),
            effList: vec![5.0, 10.0],
            k: 100.0 / 3.0,
            ramping: false,
        },
    ];
    // -- Chilling Area like floor Frost has a different effect and must be done differently
    // L5378
    let skillTypes = activeSkill.skillTypes.get();
    if skillTypes.with(|s| s.has(SkillType::ChillingArea))
        || skillTypes.with(|s| s.has(SkillType::NonHitChill))
    {
        let chill = ailmentDataOf(ctx, n!("Chill"));
        // L5379
        skillFlags.set(n!("chill"), true);
        // L5380
        let incChill = skillModList.Sum(ModType::INC, c, nm!["EnemyChillEffect"]);
        // L5381
        let moreChill = skillModList.More(c, nm!["EnemyChillEffect"]);
        // L5382
        output.set(
            n!("ChillEffectMod"),
            (1.0 + incChill / 100.0)
                * moreChill
                * calcLib::r#mod(&enemyDB, None, nm!["SelfChillEffect"]),
        );
        // L5383
        let incChillDuration = skillModList.Sum(
            ModType::INC,
            c,
            nm![
                "EnemyChillDuration",
                "EnemyAilmentDuration",
                "EnemyElementalAilmentDuration"
            ],
        ) + enemyDB.Sum(
            ModType::INC,
            None,
            nm![
                "SelfChillDuration",
                "SelfAilmentDuration",
                "SelfElementalAilmentDuration"
            ],
        );
        // L5384
        let moreChillDuration = skillModList.More(
            c,
            nm![
                "EnemyChillDuration",
                "EnemyAilmentDuration",
                "EnemyElementalAilmentDuration"
            ],
        ) * enemyDB.More(
            None,
            nm![
                "SelfChillDuration",
                "SelfAilmentDuration",
                "SelfElementalAilmentDuration"
            ],
        );
        // L5385
        output.set(
            n!("ChillDurationMod"),
            (1.0 + incChillDuration / 100.0) * moreChillDuration,
        );
        // L5386
        let v = m_min(
            ov(skillModList.Override(None, nm!["ChillMax"])).unwrap_or(chill.max),
            m_floor(nn(chill.default) * output.num(n!("ChillEffectMod"))),
        );
        output.set(n!("ChillSourceEffect"), v);
        // L5387
        if let Some(b) = &breakdown {
            // L5388
            b.set(n!("DotChill"), Table::new());
            // L5389
            let chain = Table::new();
            chain.set(
                n!("label"),
                lfmt!(
                    "Effect of Chill: ^8(capped at %d%%)",
                    ov(skillModList.Override(None, nm!["ChillMax"])).unwrap_or(chill.max)
                ),
            );
            chain.set(n!("base"), lfmt!("%d%% ^8(base)", chill.default));
            chain.push(tbl!("%.2f ^8(increased/reduced effect of chill)", 1.0 + incChill / 100.0;));
            chain.push(tbl!("%.2f ^8(more/less effect of chill)", moreChill;));
            chain.set(
                n!("total"),
                lfmt!("= %.0f%%", output.num(n!("ChillSourceEffect"))),
            );
            b.multiChain(&sub(b, n!("DotChill")), &chain);
        }
    }
    // -- Crit ailments are done differently for Freeze
    // L5399
    if (output.num(n!("FreezeChanceOnHit")) + output.num(n!("FreezeChanceOnCrit"))) > 0.0 {
        // L5400
        if let Some(gb) = &globalBreakdown {
            // L5401
            gb.set(
                n!("FreezeDurationMod"),
                seq(vec![lfmt!(
                    "Ailment mode: %s ^8(can be changed in the Configuration tab)",
                    if isCritMode { "Crits Only" } else { "Average Damage" }
                )]),
            );
        }
        // L5405
        let critChance = output.num(n!("CritChance"));
        let (hitDmg, critDmg) = closures::calcAverageSourceDamage(ctx, al, n!("Freeze"));
        let baseVal =
            closures::calcAilmentDamage(ctx, al, n!("Freeze"), critChance, hitDmg, critDmg, false)
                * skillModList.More(c, nm!["FreezeAsThoughDealing"]);
        // L5406
        if baseVal > 0.0 {
            // L5407
            skillFlags.set(n!("freeze"), true);
            // L5408
            let v = 1.0
                + skillModList.Sum(
                    ModType::INC,
                    c,
                    nm![
                        "EnemyFreezeDuration",
                        "EnemyAilmentDuration",
                        "EnemyElementalAilmentDuration"
                    ],
                ) / 100.0
                + enemyDB.Sum(
                    ModType::INC,
                    None,
                    nm![
                        "SelfFreezeDuration",
                        "SelfElementalAilmentDuration",
                        "SelfAilmentDuration",
                        "HoarfrostFreezeDuration"
                    ],
                ) / 100.0;
            output.set(n!("FreezeDurationMod"), v);
            // L5409
            if let Some(b) = &breakdown {
                // L5410
                tins(
                    b,
                    n!("FreezeDPS"),
                    lfmt!(
                        "For freeze to apply for the minimum of 0.3 seconds, target must have no more than %.0f Ailment Threshold.",
                        baseVal * 20.0 * output.num(n!("FreezeDurationMod"))
                    ),
                );
                // L5411
                tins(
                    b,
                    n!("FreezeDPS"),
                    lfmt!("^8(Ailment Threshold is about equal to Life except on bosses where it is about half of their life)"),
                );
            }
        }
    }
    // -- Cycle through all non-damage ailments here, modifying them if needed
    // L5416 LUA-ORDER: `pairs(ailments)` over string keys (constructor order here, random in PoB)
    for val in ailments.iter_mut() {
        let ailment = val.name;
        // L5417
        if (output.num(cat!(ailment, "ChanceOnHit")) + output.num(cat!(ailment, "ChanceOnCrit")))
            > 0.0
        {
            // L5418
            if let Some(gb) = &globalBreakdown {
                // L5419
                gb.set(
                    cat!(ailment, "EffectMod"),
                    seq(vec![lfmt!(
                        "Ailment mode: %s ^8(can be changed in the Configuration tab)",
                        if isCritMode { "Crits Only" } else { "Average Damage" }
                    )]),
                );
            }
            // -- Sets the crit strike condition to match ailment mode.
            // L5424
            if isCritMode && !modDB.Flag(None, nm!["AilmentsAreNeverFromCrit"]) {
                // L5425
                skillCond(&cfg).set(n!("CriticalStrike"), true);
            } else {
                // L5427
                skillCond(&cfg).set(n!("CriticalStrike"), false);
            }

            // L5430
            let critChance = output.num(n!("CritChance"));
            let (hitDmg, critDmg) = closures::calcAverageSourceDamage(ctx, al, ailment);
            let damage =
                closures::calcAilmentDamage(ctx, al, ailment, critChance, hitDmg, critDmg, false)
                    * skillModList.More(c, &[cat!(ailment, "AsThoughDealing")]);
            // -- We check if there is a damage instance above 0 since if you deal 0 damage, you don't apply anything.
            // L5432
            if damage > 0.0 {
                let ad = ailmentDataOf(ctx, ailment);
                // L5433
                skillFlags.set(
                    Name::from(pob_lua::string::lower(ailment.as_str()).as_str()),
                    true,
                );
                // L5434
                let incDur = skillModList.Sum(
                    ModType::INC,
                    c,
                    &[
                        cat!("Enemy", ailment, "Duration"),
                        n!("EnemyElementalAilmentDuration"),
                        n!("EnemyAilmentDuration"),
                    ],
                ) + enemyDB.Sum(
                    ModType::INC,
                    None,
                    &[
                        cat!("Self", ailment, "Duration"),
                        n!("SelfElementalAilmentDuration"),
                        n!("SelfAilmentDuration"),
                    ],
                );
                // L5435
                let moreDur = skillModList.More(
                    c,
                    &[
                        cat!("Enemy", ailment, "Duration"),
                        n!("EnemyElementalAilmentDuration"),
                        n!("EnemyAilmentDuration"),
                    ],
                ) * enemyDB.More(
                    None,
                    &[
                        cat!("Self", ailment, "Duration"),
                        n!("SelfElementalAilmentDuration"),
                        n!("SelfAilmentDuration"),
                    ],
                );
                // L5436
                output.set(
                    cat!(ailment, "Duration"),
                    nn(ad.duration) * (1.0 + incDur / 100.0) * moreDur * debuffDurationMult,
                );
                // -- Line Controls Crit Conditional for Crit Mastery
                // L5438
                output.set(
                    cat!(ailment, "EffectMod"),
                    calcLib::r#mod(&skillModList, c, &[cat!("Enemy", ailment, "Effect")])
                        * calcLib::r#mod(&enemyDB, None, &[cat!("Self", ailment, "Effect")]),
                );
                // L5439
                if let Some(b) = &breakdown {
                    let go = goOrRaise(&globalOutput);
                    let dpsKey = cat!(ailment, "DPS");
                    // L5440
                    let maximum = go
                        .opt_num(cat!("Maximum", ailment))
                        .filter(|_| go.truthy(cat!("Maximum", ailment)))
                        .unwrap_or(ad.max);
                    // L5441
                    let current = m_max(
                        m_min(go.num_or(cat!("Current", ailment), 0.0), maximum),
                        0.0,
                    );
                    // L5442
                    let desired = m_max(
                        m_min(
                            enemyDB.Sum(ModType::BASE, None, &[cat!("Desired", ailment, "Val")]),
                            maximum,
                        ),
                        0.0,
                    );
                    // L5443
                    if ad.min != 0.0 {
                        // L5444
                        val.effList.push(ad.min);
                    }
                    // L5446
                    if enemyThreshold > 0.0 {
                        // L5447
                        let e = val.effect(
                            enemyThreshold,
                            damage,
                            output.num(cat!(ailment, "EffectMod")),
                        );
                        val.effList.push(e);
                    }
                    // L5449
                    if isValueInArray(&val.effList, &maximum).is_none() {
                        // L5450
                        val.effList.push(maximum);
                    }
                    // L5452
                    if current > 0.0 && isValueInArray(&val.effList, &current).is_none() {
                        // L5453
                        val.effList.push(current);
                    }
                    // L5455
                    if desired > 0.0 && isValueInArray(&val.effList, &desired).is_none() && current == 0.0 {
                        // L5456
                        val.effList.push(desired);
                    }
                    // L5458
                    let suffix = if current > 0.0 && val.ramping {
                        lfmt!(
                            " ^8(with a ^7%s%% ^8%s on the enemy)^7",
                            tostring_num(current).as_str(),
                            ailment
                        )
                    } else {
                        String::new()
                    };
                    sub(b, dpsKey).set(
                        n!("label"),
                        format!("Resulting ailment effect{suffix}"),
                    );
                    // L5459
                    sub(b, dpsKey).set(
                        n!("footer"),
                        lfmt!(
                            "^8(ailment threshold is about equal to life, except on bosses that have specific ailment thresholds)\n(the above table shows that when the enemy has X ailment threshold, you ^8%s for Y)",
                            pob_lua::string::lower(ailment.as_str()).as_str()
                        ),
                    );
                    // L5460
                    sub(b, dpsKey).set(n!("rowList"), Table::new());
                    // L5461
                    let colList = Table::new();
                    colList.push(tbl!(label = "Ailment Threshold", key = "thresh"));
                    colList.push(tbl!(label = format!("{} Effect", ailment.as_str()), key = "effect"));
                    sub(b, dpsKey).set(n!("colList"), colList);
                    // L5465
                    pob_lua::sort::sort_default(&mut val.effList);
                    // L5466
                    let effList = val.effList.clone();
                    for &v0 in effList.iter() {
                        let mut value = v0;
                        // L5467
                        let thresh =
                            val.thresh(damage, value, output.num(cat!(ailment, "EffectMod")));
                        // L5468
                        let decCheck = value / m_floor(value);
                        // L5469
                        let precision = ad.precision;
                        // L5470
                        value = m_floor(value * lpow(10.0, precision)) / lpow(10.0, precision);
                        // L5471
                        let valueFormat = format!("%.{}f%%", tostring_num(precision));
                        // L5472
                        let threshString = format!(
                            "{}{}",
                            lfmt!("%d", thresh),
                            if m_floor(thresh + 0.5) == m_floor(enemyThreshold + 0.5) {
                                lfmt!(
                                    " ^8(%s)",
                                    tostr(&env.configInput.get(n!("enemyIsBoss"))).as_str()
                                )
                            } else {
                                String::new()
                            }
                        );
                        // L5473
                        let mut labels: Vec<&str> = Vec::new();
                        // L5474
                        if decCheck == 1.0 && value != 0.0 {
                            // L5475
                            if value == current {
                                // L5476
                                labels.push("current");
                            }
                            // L5478
                            if value == desired {
                                // L5479
                                labels.push("desired");
                            }
                            // L5481
                            if value == maximum {
                                // L5482
                                labels.push("maximum");
                            }
                            // L5484
                            if value == ad.min {
                                // L5485
                                labels.push("minimum");
                            }
                        }
                        // L5488
                        let effect = format!(
                            "{}{}",
                            lfmt!(&valueFormat, value),
                            if !labels.is_empty() {
                                format!(" ^8({})", labels.join(", "))
                            } else {
                                String::new()
                            }
                        );
                        sub(&sub(b, dpsKey), n!("rowList"))
                            .push(tbl!(effect = effect, thresh = threshString));
                    }
                }
                // L5494
                if let Some(b) = breakdown
                    .as_ref()
                    .filter(|_| Some(output.num(cat!(ailment, "Duration"))) != ad.duration)
                {
                    let k = cat!(ailment, "Duration");
                    // L5495
                    b.set(k, Table::new());
                    // L5496
                    if isAttack {
                        // L5497
                        tins(b, k, format!("{}:", passLabel(&pass)));
                    }
                    // L5499
                    tins(b, k, lfmt!("%.2fs ^8(base duration)", ad.duration));
                    // L5500
                    if incDur != 0.0 {
                        // L5501
                        tins(
                            b,
                            k,
                            lfmt!("x %.2f ^8(increased/reduced duration)", 1.0 + incDur / 100.0),
                        );
                    }
                    // L5503
                    if moreDur != 1.0 {
                        // L5504
                        tins(b, k, lfmt!("x %.2f ^8(more/less duration)", moreDur));
                    }
                    // L5506
                    if debuffDurationMult != 1.0 {
                        // L5507
                        tins(
                            b,
                            k,
                            lfmt!(
                                "/ %.2f ^8(debuff expires slower/faster)",
                                1.0 / debuffDurationMult
                            ),
                        );
                    }
                    // L5509
                    tins(b, k, lfmt!("= %.2fs", output.num(cat!(ailment, "Duration"))));
                }
            }
        }
    }

    // -- Calculate knockback chance/distance
    // L5516
    let v = m_min(
        100.0,
        output.num(n!("KnockbackChanceOnHit")) * (1.0 - output.num(n!("CritChance")) / 100.0)
            + output.num(n!("KnockbackChanceOnCrit")) * output.num(n!("CritChance")) / 100.0
            + enemyDB.Sum(ModType::BASE, None, nm!["SelfKnockbackChance"]),
    );
    output.set(n!("KnockbackChance"), v);
    // L5517
    if output.num(n!("KnockbackChance")) > 0.0 {
        // L5518
        output.set(
            n!("KnockbackDistance"),
            round(
                4.0 * calcLib::r#mod(&skillModList, c, nm!["EnemyKnockbackDistance"]),
                None,
            ),
        );
        // L5519
        if let Some(b) = &breakdown {
            // L5520
            b.set(
                n!("KnockbackDistance"),
                tbl!(radius = output.get(n!("KnockbackDistance"))),
            );
        }
    }

    // -- Calculate enemy stun modifiers
    // L5527
    let enemyStunThresholdRed = -skillModList.Sum(ModType::INC, c, nm!["EnemyStunThreshold"]);
    // L5528
    if enemyStunThresholdRed > 75.0 {
        // L5529
        output.set(
            n!("EnemyStunThresholdMod"),
            1.0 - (75.0 + (enemyStunThresholdRed - 75.0) * 25.0 / (enemyStunThresholdRed - 50.0))
                / 100.0,
        );
    } else {
        // L5531
        output.set(
            n!("EnemyStunThresholdMod"),
            1.0 - enemyStunThresholdRed / 100.0,
        );
    }
    // L5533
    let base = lor(skillData.get(n!("baseStunDuration")), || Val::Num(0.35)).num();
    // L5534
    let incDur = skillModList.Sum(ModType::INC, c, nm!["EnemyStunDuration"]);
    // L5535
    let incDurCrit = skillModList.Sum(ModType::INC, c, nm!["EnemyStunDurationOnCrit"]);
    // L5536
    let moreDur = skillModList.More(c, nm!["EnemyStunDuration"]);
    // L5537
    let chanceToDouble = m_min(
        skillModList.Sum(ModType::BASE, c, nm!["DoubleEnemyStunDurationChance"])
            + enemyDB.Sum(ModType::BASE, c, nm!["SelfDoubleStunDurationChance"]),
        100.0,
    );
    // L5538
    let incRecov = enemyDB.Sum(ModType::INC, None, nm!["StunRecovery"]);
    // L5539
    let mut minimumStunDuration = base * moreDur / (1.0 + incRecov / 100.0);
    // L5540
    let mut maximumStunDuration = minimumStunDuration;
    // L5541
    output.set(n!("EnemyStunDuration"), minimumStunDuration);
    // L5542
    if incDurCrit != 0.0 && output.num(n!("CritChance")) != 0.0 {
        // L5543
        if output.num(n!("CritChance")) == 100.0 {
            // L5544
            minimumStunDuration = minimumStunDuration * (1.0 + (incDur + incDurCrit) / 100.0);
            // L5545
            maximumStunDuration = minimumStunDuration;
            // L5546
            output.set(n!("EnemyStunDuration"), minimumStunDuration);
        } else {
            // L5548
            minimumStunDuration = minimumStunDuration * (1.0 + incDur / 100.0);
            // L5549
            maximumStunDuration = maximumStunDuration * (1.0 + (incDur + incDurCrit) / 100.0);
            // L5550
            let v = output.num(n!("EnemyStunDuration"))
                * (1.0 + (incDur + incDurCrit * output.num(n!("CritChance")) / 100.0) / 100.0);
            output.set(n!("EnemyStunDuration"), v);
        }
    } else {
        // L5553
        minimumStunDuration = minimumStunDuration * (1.0 + incDur / 100.0);
        // L5554
        maximumStunDuration = minimumStunDuration;
        // L5555
        output.set(n!("EnemyStunDuration"), minimumStunDuration);
    }
    // L5557
    if chanceToDouble != 0.0 {
        // L5558
        if chanceToDouble == 100.0 {
            // L5559
            minimumStunDuration = minimumStunDuration * 2.0;
        }
        // L5561
        maximumStunDuration = maximumStunDuration * 2.0;
        // L5562
        let v = output.num(n!("EnemyStunDuration")) * (1.0 + chanceToDouble / 100.0);
        output.set(n!("EnemyStunDuration"), v);
    }
    // L5564
    if let Some(b) = &breakdown {
        // L5565
        if output.num(n!("EnemyStunDuration")) != base {
            let k = n!("EnemyStunDuration");
            // L5566
            b.set(k, seq(vec![lfmt!("%.2fs ^8(base duration)", base)]));
            // L5569
            if incDur != 0.0 || (incDurCrit != 0.0 && output.num(n!("CritChance")) != 0.0) {
                // L5570
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(increased/reduced stun duration)",
                        1.0 + (incDur + incDurCrit * output.num(n!("CritChance")) / 100.0) / 100.0
                    ),
                );
            }
            // L5572
            if moreDur != 1.0 {
                // L5573
                tins(b, k, lfmt!("x %.2f ^8(more/less stun duration)", moreDur));
            }
            // L5575
            if chanceToDouble != 0.0 {
                // L5576
                tins(
                    b,
                    k,
                    lfmt!(
                        "x %.2f ^8(chance to double stun duration)",
                        1.0 + chanceToDouble / 100.0
                    ),
                );
            }
            // L5578
            if incRecov != 0.0 {
                // L5579
                tins(
                    b,
                    k,
                    lfmt!(
                        "/ %.2f ^8(increased/reduced enemy stun recovery)",
                        1.0 + incRecov / 100.0
                    ),
                );
            }
            // L5581
            tins(b, k, lfmt!("= %.2fs", output.num(n!("EnemyStunDuration"))));
            // L5582
            if minimumStunDuration != maximumStunDuration {
                // L5583
                tins(
                    b,
                    k,
                    lfmt!(
                        "(minimum: %.2fs, maximum: %.2fs)",
                        minimumStunDuration,
                        maximumStunDuration
                    ),
                );
            }
            // L5585
            let enemy = ctx
                .actor
                .enemy()
                .unwrap_or_else(|| lua_error!("attempt to index local 'actor' (a nil value)"));
            let enemyActionSpeed = actionSpeedMod(&enemy);
            // L5586
            if enemyActionSpeed != 1.0 {
                // L5587
                tins(b, k, lfmt!("/ %.2f ^8(enemy action speed)", enemyActionSpeed));
                // L5588
                tins(
                    b,
                    k,
                    lfmt!(
                        "= %.2fs (note that for effects that care about duration this is ignored)",
                        output.num(n!("EnemyStunDuration")) / enemyActionSpeed
                    ),
                );
                // L5589
                if minimumStunDuration != maximumStunDuration {
                    // L5590
                    tins(
                        b,
                        k,
                        lfmt!(
                            "(minimum: %.2fs, maximum: %.2fs)",
                            minimumStunDuration / enemyActionSpeed,
                            maximumStunDuration / enemyActionSpeed
                        ),
                    );
                }
            }
        }
    }

    // -- Calculate impale chance and modifiers
    // L5597
    if canDeal.truthy(n!("Physical"))
        && (output.num(n!("ImpaleChance")) + output.num(n!("ImpaleChanceOnCrit"))) > 0.0
    {
        // L5598
        skillFlags.set(n!("impale"), true);
        // L5599
        let critChance = output.num(n!("CritChance")) / 100.0;
        // L5600
        let impaleChance = m_min(output.num(n!("ImpaleChance")) / 100.0, 1.0) * (1.0 - critChance)
            + m_min(output.num(n!("ImpaleChanceOnCrit")) / 100.0, 1.0) * critChance;
        // L5601
        let maxStacks = skillModList.Sum(ModType::BASE, c, nm!["ImpaleStacksMax"])
            * (1.0
                + skillModList.Sum(ModType::BASE, c, nm!["ImpaleAdditionalDurationChance"]) / 100.0);
        // L5602
        let configStacks = enemyDB.Sum(ModType::BASE, c, nm!["Multiplier:ImpaleStacks"]);
        // L5603
        let impaleStacks = m_min(maxStacks, configStacks);

        // L5605
        let baseStoredDamage = misc.ImpaleStoredDamageBase;
        // L5606
        let storedExpectedDamageIncOnBleed =
            skillModList.Sum(ModType::INC, c, nm!["ImpaleEffectOnBleed"])
                * m_min(
                    skillModList.Sum(ModType::BASE, c, nm!["BleedChance"]) / 100.0,
                    1.0,
                );
        // L5607
        let storedExpectedDamageInc = (skillModList.Sum(ModType::INC, c, nm!["ImpaleEffect"])
            + storedExpectedDamageIncOnBleed)
            / 100.0;
        // L5608
        let storedExpectedDamageMore = round(skillModList.More(c, nm!["ImpaleEffect"]), Some(2));
        // L5609
        let storedExpectedDamageModifier =
            (1.0 + storedExpectedDamageInc) * storedExpectedDamageMore;
        // L5610
        let impaleStoredDamage = baseStoredDamage * storedExpectedDamageModifier;
        // L5611 -- Source: https://www.reddit.com/r/pathofexile/comments/chgqqt/impale_and_armor_interaction/
        let impaleHitDamageMod = impaleStoredDamage * impaleStacks;

        // L5613
        let enemyArmour = m_max(calcLib::val(&enemyDB, n!("Armour"), None), 0.0);
        // L5614
        let impaleArmourReduction = armourReductionF(
            enemyArmour,
            impaleHitDamageMod * output.num(n!("impaleStoredHitAvg")),
        );
        // L5615
        let mut impaleResist = m_min(
            m_max(
                0.0,
                enemyDB.Sum(ModType::BASE, None, nm!["PhysicalDamageReduction"])
                    + skillModList.Sum(
                        ModType::BASE,
                        c,
                        nm!["EnemyImpalePhysicalDamageReduction"],
                    )
                    + impaleArmourReduction,
            ),
            misc.EnemyPhysicalDamageReductionCap,
        );
        // L5616
        if skillModList.Flag(c, nm!["IgnoreEnemyImpalePhysicalDamageReduction"]) {
            // L5617
            impaleResist = 0.0;
        }
        // L5619
        let impaleTakenCfg = ModCfg::with_flags(ModFlag::Hit, 0);
        let t: Option<&ModCfg> = Some(&impaleTakenCfg);
        // L5620
        let impaleTaken = (1.0
            + enemyDB.Sum(
                ModType::INC,
                t,
                nm!["DamageTaken", "PhysicalDamageTaken", "ReflectedDamageTaken"],
            ) / 100.0)
            * enemyDB.More(
                t,
                nm!["DamageTaken", "PhysicalDamageTaken", "ReflectedDamageTaken"],
            );
        // L5622
        let impaleDMGModifier =
            impaleHitDamageMod * (1.0 - impaleResist / 100.0) * impaleChance * impaleTaken;

        let globalOutput = goOrRaise(&globalOutput);
        // L5624
        globalOutput.set(n!("ImpaleStacksMax"), maxStacks);
        // L5625
        globalOutput.set(n!("ImpaleStacks"), impaleStacks);
        // --ImpaleStoredDamage should be named ImpaleEffect or similar
        // --Using the variable name ImpaleEffect breaks the calculations sidebar (?!)
        // L5628
        output.set(n!("ImpaleStoredDamage"), impaleStoredDamage * 100.0);
        // L5629
        output.set(n!("ImpaleModifier"), 1.0 + impaleDMGModifier);

        // L5631
        if let Some(b) = &breakdown {
            let k = n!("ImpaleStoredDamage");
            // L5632
            b.set(k, Table::new());
            // L5633
            tins(b, k, "10% ^8(base value)");
            // L5634
            tins(
                b,
                k,
                lfmt!(
                    "x %.2f ^8(increased effectiveness)",
                    storedExpectedDamageModifier
                ),
            );
            // L5635
            tins(
                b,
                k,
                lfmt!("= %.1f%%", output.num(n!("ImpaleStoredDamage"))),
            );

            let k = n!("ImpaleModifier");
            // L5637
            b.set(k, Table::new());
            // L5638
            tins(
                b,
                k,
                lfmt!(
                    "%d ^8(number of stacks, can be overridden in the Configuration tab)",
                    impaleStacks
                ),
            );
            // L5639
            tins(b, k, lfmt!("x %.3f ^8(stored damage)", impaleStoredDamage));
            // L5640
            tins(b, k, lfmt!("x %.2f ^8(impale chance)", impaleChance));
            // L5641
            tins(
                b,
                k,
                lfmt!(
                    "x %.2f ^8(impale enemy physical damage reduction)",
                    1.0 - impaleResist / 100.0
                ),
            );
            // L5642
            if impaleTaken != 1.0 {
                // L5643
                tins(b, k, lfmt!("x %.2f ^8(impale enemy damage taken)", impaleTaken));
            }
            // L5645
            tins(
                b,
                k,
                lfmt!("= %.3f ^8(impale damage multiplier)", impaleDMGModifier),
            );
        }
    }
    // L5648 (end of the ailment loop body)
    Flow::Normal
}

/// `env.partyMembers.modDB:Flag(nil, name)` (indexing a nil `partyMembers` raises).
fn partyModDBFlag(env: &EnvRef, name: Name) -> bool {
    let pm = env
        .partyMembers
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'partyMembers' (a nil value)"));
    pm.modDB.get().Flag(None, &[name])
}
