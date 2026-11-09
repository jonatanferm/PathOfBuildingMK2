//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence segment OF-D (CalcOffence.lua 2174-2605): the first pass loop (accuracy, hit chance,
//! enemy block, attack/cast speed, trauma, hit time), the DPS multiplier, the main/off hand
//! combination of hit chance and speed, the hit time breakdown and `quantityMultiplier`.
//!
//! Owner: OF-D. Cross-segment calls: `combineStat` (OF-C, CalcOffence.lua:2066) and
//! `calcs.hitChance` (CalcDefence.lua:58).
#![allow(non_snake_case)]
// literal Lua statement shapes (`x = x * y`, `1 / (a + b)`)
#![allow(unused_parens, clippy::assign_op_pattern, clippy::neg_cmp_op_on_partial_ord)]

use super::costs_passes::combineStat;
use super::OffenceCtx;
use crate::modules::calc_defence::helpers::hitChance;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;

/// `globalOutput` (CalcOffence.lua:65), the module upvalue assigned at 2176 (§7.3).
fn globalOutput() -> Output {
    OFFENCE_STATE
        .with(|s| s.globalOutput.get())
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'globalOutput' (a nil value)"))
}

/// `colorCodes[name]` (Global.lua).
fn colorCode(name: &str) -> &'static str {
    pob_gamedata::colorCodes
        .iter()
        .find(|(k, _)| *k == name)
        .map(|(_, v)| *v)
        .unwrap_or_else(|| lua_error!("attempt to concatenate field '{name}' (a nil value)"))
}

/// The text of a value in a Lua `..` concatenation (numbers as `%.14g`).
fn concat_str(v: &Val) -> String {
    match v {
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => lua_error!(
            "attempt to concatenate a {} value",
            other.lua_type()
        ),
    }
}

/// `t[k]` of a value that is a table or a breakdown object (`breakdown.MainHand`).
fn index(v: &Val, k: Name) -> Val {
    match v.obj::<Breakdown>() {
        Some(b) => b.get(k),
        None => v.get(k),
    }
}

/// `pass.source`: a table (a weapon data copy or `skillData`); indexing anything else raises.
fn source_table(source: &Val) -> Table {
    source.table().unwrap_or_else(|| {
        lua_error!(
            "attempt to index local 'source' (a {} value)",
            source.lua_type()
        )
    })
}

/// `activeSkill.activeEffect.grantedEffect.castTime`.
fn castTime(ctx: &OffenceCtx) -> Option<f64> {
    ctx.activeSkill.activeEffect.get().grantedEffect.get().castTime
}

/// `activeSkill.skillTypes[t]`.
fn hasSkillType(ctx: &OffenceCtx, t: SkillType::T) -> bool {
    ctx.activeSkill.skillTypes.get().with(|s| s.has(t))
}

/// @pob Modules/CalcOffence.lua:2174-2605 h=5aa62b9a
/// OF-D: CalcOffence.lua 2174-2605.
pub(crate) fn seg_speed(ctx: &mut OffenceCtx) -> Flow {
    use ModType::{BASE, INC};
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let env = ctx.env.clone();
    let isAttack = ctx.isAttack;
    let skillCfg_ = Some(&*skillCfg);

    // L2174 Calculate how often you hit (speed, accuracy, block, etc)
    // L2175
    let passList = ctx.passList.clone();
    for_ipairs!((_, pass) in passList => {
        let pass: Table = pass;
        // L2176
        OFFENCE_STATE.with(|s| {
            s.globalOutput.set(Some(ctx.output.clone()));
            s.globalBreakdown.set(ctx.breakdown.clone());
        });
        // L2177
        let source = pass.get(n!("source"));
        // L2178 ---@class Output
        // L2179
        let output: Output = pass.table(n!("output")).unwrap_or_else(|| {
            lua_error!("attempt to index local 'output' (a nil value)")
        });
        // L2180
        let cfg: Option<Cfg> = pass.obj::<ModCfg>(n!("cfg"));
        let cfg_ = cfg.as_deref();
        // L2181
        let breakdown: Option<Rc<Breakdown>> = pass.obj::<Breakdown>(n!("breakdown"));

        // L2183
        if skillData.truthy(n!("averageBurstHits")) {
            // L2184
            output.set(n!("AverageBurstHits"), skillData.get(n!("averageBurstHits")));
        } else if output.truthy(n!("Repeats")) && output.num(n!("Repeats")) > 1.0 {
            // L2185
            // L2186
            output.set(n!("AverageBurstHits"), output.get(n!("Repeats")));
        }

        // L2189 Calculate hit chance
        // L2190
        let base = skillModList.Sum(BASE, cfg_, nm!["Accuracy"]);
        // L2191
        let baseVsEnemy = skillModList.Sum(BASE, cfg_, nm!["Accuracy", "AccuracyVsEnemy"]);
        // L2192
        let inc = skillModList.Sum(INC, cfg_, nm!["Accuracy"]);
        // L2193
        let incVsEnemy = skillModList.Sum(INC, cfg_, nm!["Accuracy", "AccuracyVsEnemy"]);
        // L2194 LUA-QUIRK: "MORE" is passed as the cfg; `cfg` lands in the name list and matches nothing
        let more = skillModList.More(Some(ModCfg::STRING_CFG()), nm!["Accuracy"]);
        // L2195 LUA-QUIRK: as 2194
        let moreVsEnemy =
            skillModList.More(Some(ModCfg::STRING_CFG()), nm!["Accuracy", "AccuracyVsEnemy"]);

        // L2197
        output.set(
            n!("Accuracy"),
            m_max(0.0, m_floor(base * (1.0 + inc / 100.0) * more)),
        );
        // L2198
        let mut accuracyVsEnemy =
            m_max(0.0, m_floor(baseVsEnemy * (1.0 + incVsEnemy / 100.0) * moreVsEnemy));
        // L2199
        if let Some(breakdown) = &breakdown {
            // L2200
            breakdown.set(n!("Accuracy"), Table::new());
            // L2201
            breakdown.multiChain(
                &breakdown.table(n!("Accuracy")).unwrap_or_default(),
                &tbl! {
                    tbl!["%.2f ^8(increased/reduced)", 1.0 + inc / 100.0],
                    tbl!["%.2f ^8(more/less)", more];
                    base = tbl!["%g ^8(base)", base],
                    total = lfmt!("= %g", output.opt_num(n!("Accuracy"))),
                },
            );
            // L2207
            if !output.get(n!("Accuracy")).raw_eq(&Val::Num(accuracyVsEnemy)) {
                // L2208
                breakdown
                    .table(n!("Accuracy"))
                    .unwrap_or_default()
                    .push(lfmt!(""));
                // L2209
                breakdown.multiChain(
                    &breakdown.table(n!("Accuracy")).unwrap_or_default(),
                    &tbl! {
                        tbl!["%.2f ^8(increased/reduced)", 1.0 + incVsEnemy / 100.0],
                        tbl!["%.2f ^8(more/less)", moreVsEnemy];
                        label = "Effective Accuracy vs Enemy",
                        base = tbl!["%g ^8(base)", baseVsEnemy],
                        total = lfmt!("= %g", accuracyVsEnemy),
                    },
                );
            }
        }
        // L2218
        if skillModList.Flag(None, nm!["Condition:OffHandAccuracyIsMainHandAccuracy"])
            && pass.str(n!("label")).as_deref() == Some("Main Hand")
        {
            // L2219
            ctx.storedMainHandAccuracy = output.opt_num(n!("Accuracy"));
            // L2220
            ctx.storedMainHandAccuracyVsEnemy = Some(accuracyVsEnemy);
        } else if skillModList.Flag(None, nm!["Condition:OffHandAccuracyIsMainHandAccuracy"])
            && pass.str(n!("label")).as_deref() == Some("Off Hand")
            && ctx.storedMainHandAccuracy.is_some()
        {
            // L2221
            // L2222
            output.set(n!("Accuracy"), ctx.storedMainHandAccuracy);
            // L2223 (both locals are assigned together at 2219-2220)
            accuracyVsEnemy = ctx.storedMainHandAccuracyVsEnemy.unwrap_or_else(|| {
                lua_error!("attempt to compare nil with number")
            });
            // L2224
            if let Some(breakdown) = &breakdown {
                // L2225
                breakdown.set(
                    n!("Accuracy"),
                    tbl![format!(
                        "Using Main Hand Accuracy due to Mastery: {}",
                        concat_str(&output.get(n!("Accuracy")))
                    )],
                );
            }
        }
        // L2230
        if !isAttack
            || skillModList.Flag(cfg_, nm!["CannotBeEvaded"])
            || skillData.truthy(n!("cannotBeEvaded"))
            || (env.mode_effective.get() && enemyDB.Flag(None, nm!["CannotEvade"]))
        {
            // L2231
            output.set(n!("AccuracyHitChance"), 100.0);
        } else {
            // L2233
            let enemyEvasion = m_max(round(calcLib::val(&enemyDB, n!("Evasion"), None), None), 0.0);
            // L2234
            output.set(
                n!("AccuracyHitChance"),
                hitChance(enemyEvasion, accuracyVsEnemy)
                    * calcLib::r#mod(&skillModList, cfg_, nm!["HitChance"]),
            );
            // L2235
            if let Some(breakdown) = &breakdown {
                // L2236
                breakdown.set(
                    n!("AccuracyHitChance"),
                    tbl![
                        format!(
                            "Enemy level: {}{}",
                            tostring_num(env.enemyLevel.get()),
                            if env.configInput.truthy(n!("enemyLevel")) {
                                " ^8(overridden from the Configuration tab"
                            } else {
                                " ^8(can be overridden in the Configuration tab)"
                            }
                        ),
                        format!("Enemy evasion: {}", tostring_num(enemyEvasion)),
                        format!(
                            "Approximate hit chance: {}%",
                            concat_str(&output.get(n!("AccuracyHitChance")))
                        )
                    ],
                );
            }
        }
        // L2243 enemy block chance
        // L2244
        output.set(
            n!("enemyBlockChance"),
            m_max(
                m_min(enemyDB.Sum(BASE, cfg_, nm!["BlockChance"]), 100.0)
                    - skillModList.Sum(BASE, cfg_, nm!["reduceEnemyBlock"]),
                0.0,
            ),
        );
        // L2245
        if enemyDB.Flag(None, nm!["CannotBlockAttacks"]) && isAttack {
            // L2246
            output.set(n!("enemyBlockChance"), 0.0);
        }

        // L2249
        output.set(
            n!("HitChance"),
            output.num(n!("AccuracyHitChance"))
                * (1.0 - output.num(n!("enemyBlockChance")) / 100.0),
        );
        // L2250
        if output.num(n!("enemyBlockChance")) > 0.0 && !isAttack {
            // L2251
            globalOutput().set(n!("enemyHasSpellBlock"), true);
        }
        // L2253
        if let Some(breakdown) = breakdown.as_ref().filter(|_| output.num(n!("enemyBlockChance")) > 0.0) {
            // L2254
            if output.num(n!("AccuracyHitChance")) < 100.0 {
                // L2255
                breakdown.set(
                    n!("HitChance"),
                    tbl![
                        format!("Accuracy Hit Chance: {}%", concat_str(&output.get(n!("AccuracyHitChance")))),
                        format!("Enemy Block Chance: {}%", concat_str(&output.get(n!("enemyBlockChance")))),
                        format!("Approximate hit chance: {}%", concat_str(&output.get(n!("HitChance"))))
                    ],
                );
            } else {
                // L2261
                breakdown.set(
                    n!("HitChance"),
                    tbl![
                        format!("Enemy Block Chance: {}%", concat_str(&output.get(n!("enemyBlockChance")))),
                        format!("Approximate hit chance: {}%", concat_str(&output.get(n!("HitChance"))))
                    ],
                );
            }
        }

        // L2268 Check Precise Technique Keystone condition per pass as MH/OH might have different values
        // L2269
        let condName = {
            let label = pass.str(n!("label")).unwrap_or_else(|| {
                lua_error!("attempt to index field 'label' (a nil value)")
            });
            format!("{}AccRatingHigherThanMaxLife", label.replace(' ', ""))
        };
        // L2270
        let accuracy = if output.truthy(n!("Accuracy")) {
            output.num(n!("Accuracy"))
        } else {
            0.0
        };
        skillModList.conditions.set(
            Name::from(condName.as_str()),
            accuracy > env.player.output.get().num(n!("Life")),
        );

        // L2272 Calculate attack/cast speed
        // L2273
        if castTime(ctx) == Some(0.0)
            && !skillData.truthy(n!("castTimeOverride"))
            && !skillData.truthy(n!("triggered"))
        {
            // L2274
            output.set(n!("Time"), 0.0);
            // L2275
            output.set(n!("Speed"), 0.0);
        } else if skillData.truthy(n!("timeOverride")) {
            // L2276
            // L2277
            output.set(n!("Time"), skillData.get(n!("timeOverride")));
            // L2278
            output.set(n!("Speed"), 1.0 / output.num(n!("Time")));
        } else if skillData.truthy(n!("fixedCastTime")) {
            // L2279
            // L2280
            output.set(n!("Time"), castTime(ctx));
            // L2281
            output.set(n!("Speed"), 1.0 / output.num(n!("Time")));
        } else if skillData.truthy(n!("triggerTime")) && skillData.truthy(n!("triggered")) {
            // L2282
            // L2283
            let activeSkillsLinked = skillModList.Sum(BASE, cfg_, nm!["ActiveSkillsLinkedToTrigger"]);
            // L2284
            if activeSkillsLinked > 0.0 {
                // L2285
                output.set(
                    n!("Time"),
                    skillData.num(n!("triggerTime"))
                        / (1.0 + skillModList.Sum(INC, cfg_, nm!["CooldownRecovery"]) / 100.0)
                        * activeSkillsLinked,
                );
            } else {
                // L2287
                output.set(
                    n!("Time"),
                    skillData.num(n!("triggerTime"))
                        / (1.0 + skillModList.Sum(INC, cfg_, nm!["CooldownRecovery"]) / 100.0),
                );
            }
            // L2289
            output.set(n!("TriggerTime"), output.get(n!("Time")));
            // L2290
            output.set(n!("Speed"), 1.0 / output.num(n!("Time")));
        } else if skillData.truthy(n!("triggerRate")) && skillData.truthy(n!("triggered")) {
            // L2291
            // L2292
            output.set(n!("Time"), 1.0 / skillData.num(n!("triggerRate")));
            // L2293
            output.set(n!("TriggerTime"), output.get(n!("Time")));
            // L2294
            output.set(n!("Speed"), skillData.get(n!("triggerRate")));
            // L2295
            skillData.set(n!("showAverage"), false);
        } else {
            // L2297
            let baseTime: f64;
            // L2298
            if isAttack {
                // L2299
                if skillData.truthy(n!("attackSpeedMultiplier"))
                    && source_table(&source).truthy(n!("AttackRate"))
                {
                    // L2300
                    let source = source_table(&source);
                    source.set(
                        n!("AttackRate"),
                        source.num(n!("AttackRate"))
                            * (1.0 + skillData.num(n!("attackSpeedMultiplier")) / 100.0),
                    );
                }
                // L2302
                if skillData.truthy(n!("castTimeOverridesAttackTime")) {
                    // L2303 Skill is overriding weapon attack speed
                    // L2304
                    baseTime = Val::from(castTime(ctx)).num()
                        / (1.0 + source_table(&source).num0(n!("AttackSpeedInc")) / 100.0);
                } else if calcLib::r#mod(&skillModList, skillCfg_, nm!["SkillAttackTime"]) > 0.0 {
                    // L2305
                    // L2306
                    baseTime = (1.0 / source_table(&source).num_or(n!("AttackRate"), 1.0)
                        + skillModList.Sum(BASE, cfg_, nm!["Speed"]))
                        * calcLib::r#mod(&skillModList, skillCfg_, nm!["SkillAttackTime"]);
                } else {
                    // L2308
                    baseTime = 1.0 / source_table(&source).num_or(n!("AttackRate"), 1.0)
                        + skillModList.Sum(BASE, cfg_, nm!["Speed"]);
                }
            } else {
                // L2311
                baseTime = lor(
                    lor(skillData.get(n!("castTimeOverride")), || Val::from(castTime(ctx))),
                    || Val::Num(1.0),
                )
                .num();
            }
            // L2313
            let more = skillModList.More(cfg_, nm!["Speed"]);
            // L2314
            output.set(
                n!("Repeats"),
                lor(globalOutput().get(n!("Repeats")), || Val::Num(1.0)),
            );

            // L2316 Calculates the max number of trauma stacks you can sustain
            // L2317
            if skillModList.Flag(None, nm!["HasTrauma"]) {
                // L2318
                let effectiveAttackRateCap = data().misc.ServerTickRate * output.num(n!("Repeats"));
                // L2319
                let duration = skillModList.Sum(BASE, cfg_, nm!["TraumaDuration"])
                    * calcLib::r#mod(&skillModList, skillCfg_, nm!["Duration"]);
                // L2320
                let traumaPerAttack =
                    1.0 + m_min(skillModList.Sum(BASE, cfg_, nm!["ExtraTrauma"]), 100.0) / 100.0;
                // L2321
                let incAttackSpeedPerTrauma = skillModList.Sum(INC, skillCfg_, nm!["SpeedPerTrauma"]);
                // L2322 compute trauma using an exact form.
                // L2323
                let configTrauma = skillModList.Sum(BASE, skillCfg_, nm!["Multiplier:TraumaStacks"]);
                // L2324 remove trauma attack speed added by config.
                let inc = skillModList.Sum(INC, cfg_, nm!["Speed"]) - incAttackSpeedPerTrauma * configTrauma;
                // L2325
                let attackSpeedBeforeInc =
                    1.0 / baseTime * globalOutput().num(n!("ActionSpeedMod")) * more;
                // L2326
                let incAttackSpeedPerTraumaCap = (effectiveAttackRateCap
                    - attackSpeedBeforeInc * (1.0 + inc / 100.0))
                    / attackSpeedBeforeInc
                    * 100.0;
                // L2327
                let traumaRateBeforeInc = traumaPerAttack
                    * (output.num(n!("HitChance")) / 100.0)
                    * attackSpeedBeforeInc
                    / output.num(n!("Repeats"));
                // L2328
                let mut trauma = traumaRateBeforeInc * (1.0 + inc / 100.0)
                    / (1.0 / duration - traumaRateBeforeInc * incAttackSpeedPerTrauma / 100.0);
                // L2329
                let traumaBreakdown = trauma;
                // L2330
                let mut invalid = false;
                // L2331 invalid long term trauma generation as maximum attack rate is once per tick.
                if trauma < 0.0 || incAttackSpeedPerTrauma * trauma > incAttackSpeedPerTraumaCap {
                    // L2332
                    trauma = traumaPerAttack
                        * (output.num(n!("HitChance")) / 100.0)
                        * effectiveAttackRateCap
                        / output.num(n!("Repeats"))
                        * duration;
                    // L2333
                    invalid = true;
                }
                // L2335 halve trauma rate when dual wielding so pass 2 doesn't double your trauma rate
                if skillFlags.truthy(n!("bothWeaponAttack")) {
                    // L2336
                    trauma = trauma / 2.0;
                }
                // L2338
                skillModList.NewMod(
                    n!("Multiplier:SustainableTraumaStacks"),
                    BASE,
                    trauma,
                    "Maximum Sustainable Trauma Stacks",
                    0,
                    0,
                    &[],
                );
                // L2339
                if let Some(breakdown) = &breakdown {
                    // L2340
                    ctx.storedSustainedTraumaBreakdown = Table::new();
                    let stb = ctx.storedSustainedTraumaBreakdown.clone();
                    // L2341
                    if incAttackSpeedPerTrauma == 0.0 {
                        // L2342
                        breakdown.multiChain(
                            &stb,
                            &tbl! {
                                tbl!["%.2f ^8(trauma per attack)", traumaPerAttack],
                                tbl!["%.2f ^8(chance to hit)", (output.num(n!("HitChance")) / 100.0)],
                                tbl!["%.2f ^8(duration)", duration];
                                label = "Trauma",
                                base = tbl!["%.2f ^8(attack speed)", attackSpeedBeforeInc * (1.0 + inc / 100.0)],
                                noTotal = true,
                            },
                        );
                        // L2350
                        if output.num(n!("Repeats")) != 1.0 {
                            // L2351
                            stb.push(lfmt!("/ %.2f ^8(repeats)", output.opt_num(n!("Repeats"))));
                        }
                    } else {
                        // L2354
                        breakdown.multiChain(
                            &stb,
                            &tbl! {
                                tbl!["%.2f ^8(more/less)", more],
                                tbl!["%.2f ^8(action speed modifier)", globalOutput().get(n!("ActionSpeedMod"))];
                                label = "Attack Speed before increased Attack Speed",
                                base = tbl!["%.2f ^8(base)", 1.0 / baseTime],
                                total = lfmt!("= %.2f ^8attacks per second", attackSpeedBeforeInc),
                            },
                        );
                        // L2361
                        stb.push("");
                        // L2362
                        breakdown.multiChain(
                            &stb,
                            &tbl! {
                                tbl!["%.2f ^8(trauma per attack)", traumaPerAttack],
                                tbl!["%.2f ^8(chance to hit)", (output.num(n!("HitChance")) / 100.0)];
                                label = "Trauma per second before increased Attack Speed",
                                base = tbl!["%.2f ^8(base)", attackSpeedBeforeInc],
                                noTotal = true,
                            },
                        );
                        // L2369
                        if output.num(n!("Repeats")) != 1.0 {
                            // L2370
                            stb.push(lfmt!("/ %.2f ^8(repeats)", output.opt_num(n!("Repeats"))));
                        }
                        // L2372
                        stb.push(lfmt!("= %.2f ^8trauma per second", traumaRateBeforeInc));
                        // L2373
                        stb.push("");
                        // L2374
                        stb.push("Trauma");
                        // L2375
                        stb.push(lfmt!("%.2f ^8(base)", traumaRateBeforeInc));
                        // L2376
                        stb.push(lfmt!("x %.2f ^8(increased/reduced)", (1.0 + inc / 100.0)));
                        // L2377
                        stb.push(lfmt!(
                            "/ %.4f ^8(1 / duration - trauma per second * increased attack speed per trauma / 100)",
                            (1.0 / duration - traumaRateBeforeInc * incAttackSpeedPerTrauma / 100.0)
                        ));
                    }
                    // L2379
                    let fmt = format!(
                        "= {}%d ^8trauma",
                        if invalid { colorCode("NEGATIVE") } else { "" }
                    );
                    stb.push(lfmt!(fmt.as_str(), traumaBreakdown));
                    // L2380
                    if invalid {
                        // L2381
                        stb.push("");
                        // L2382
                        stb.push("Attack Speed exceeds cap; Recalculating");
                        // L2383
                        breakdown.multiChain(
                            &stb,
                            &tbl! {
                                tbl!["%.2f ^8(trauma per attack)", traumaPerAttack],
                                tbl!["%.2f ^8(chance to hit)", (output.num(n!("HitChance")) / 100.0)],
                                tbl!["%.2f ^8(duration)", (duration)];
                                base = tbl!["%.2f ^8(base)", effectiveAttackRateCap],
                                noTotal = true,
                            },
                        );
                        // L2390
                        if output.num(n!("Repeats")) != 1.0 {
                            // L2391
                            stb.push(lfmt!("/ %.2f ^8(repeats)", output.opt_num(n!("Repeats"))));
                        }
                        // L2393
                        stb.push(lfmt!("= %d ^8trauma", trauma));
                    }
                }
            }
            // L2397
            if skillModList.Sum(BASE, skillCfg_, nm!["Multiplier:TraumaStacks"]) == 0.0 {
                // L2398
                skillModList.NewMod(
                    n!("Multiplier:TraumaStacks"),
                    BASE,
                    skillModList.Sum(BASE, skillCfg_, nm!["Multiplier:SustainableTraumaStacks"]),
                    "Maximum Sustainable Trauma Stacks",
                    0,
                    0,
                    &[],
                );
            }
            // L2400
            let inc = skillModList.Sum(INC, cfg_, nm!["Speed"]);

            // L2402
            if skillFlags.truthy(n!("warcry")) {
                // L2403
                output.set(n!("Speed"), 1.0 / output.num(n!("WarcryCastTime")));
            } else {
                // L2405
                output.set(
                    n!("Speed"),
                    1.0 / (baseTime / round((1.0 + inc / 100.0) * more, Some(2))
                        + skillModList.Sum(BASE, cfg_, nm!["TotalAttackTime"])
                        + skillModList.Sum(BASE, cfg_, nm!["TotalCastTime"])),
                );
            }
            // L2408
            output.set(n!("CastRate"), output.get(n!("Speed")));
            // L2409
            if skillFlags.truthy(n!("selfCast")) {
                // L2410 Self-cast skill; apply action speed
                // L2411
                output.set(
                    n!("Speed"),
                    output.num(n!("Speed")) * globalOutput().num(n!("ActionSpeedMod")),
                );
                // L2412
                output.set(n!("CastRate"), output.get(n!("Speed")));
            }
            // L2414
            if skillFlags.truthy(n!("totem")) {
                // L2415 Totem skill. Apply action speed
                // L2416
                let totemActionSpeed = 1.0 + (modDB.Sum(INC, None, nm!["TotemActionSpeed"]) / 100.0);
                // L2417
                output.set(n!("TotemActionSpeed"), totemActionSpeed);
                // L2418
                output.set(n!("Speed"), output.num(n!("Speed")) * totemActionSpeed);
                // L2419
                output.set(n!("CastRate"), output.get(n!("Speed")));
                // L2420
                if skillData.truthy(n!("totemFireOnce")) {
                    // L2421
                    output.set(
                        n!("HitTime"),
                        1.0 / output.num(n!("Speed")) + globalOutput().num(n!("TotemPlacementTime")),
                    );
                    // L2422
                    output.set(n!("HitSpeed"), 1.0 / output.num(n!("HitTime")));
                }
            }
            // L2425
            if globalOutput().truthy(n!("Cooldown")) {
                // L2426
                output.set(n!("Cooldown"), globalOutput().get(n!("Cooldown")));
                // L2427
                output.set(
                    n!("Speed"),
                    m_min(
                        output.num(n!("Speed")),
                        1.0 / output.num(n!("Cooldown")) * output.num(n!("Repeats")),
                    ),
                );
            }
            // L2429
            if output.truthy(n!("Cooldown")) && skillFlags.truthy(n!("selfCast")) {
                // L2430
                skillFlags.set(n!("notAverage"), true);
                // L2431
                skillFlags.set(n!("showAverage"), false);
                // L2432
                skillData.set(n!("showAverage"), false);
            }
            // L2434
            if !hasSkillType(ctx, SkillType::Channel) {
                // L2435
                output.set(
                    n!("Speed"),
                    m_min(
                        output.num(n!("Speed")),
                        data().misc.ServerTickRate * output.num(n!("Repeats")),
                    ),
                );
            }
            // L2437
            if output.get(n!("Speed")).raw_eq(&Val::Num(0.0)) {
                // L2438
                output.set(n!("Time"), 0.0);
            } else {
                // L2440
                output.set(n!("Time"), 1.0 / output.num(n!("Speed")));
            }
            // L2442
            if let Some(breakdown) = &breakdown {
                // L2443
                breakdown.set(n!("Speed"), Table::new());
                // L2444
                let actionSpeed = lor(
                    lor(
                        land(skillFlags.get(n!("totem")), || output.get(n!("TotemActionSpeed"))),
                        || land(skillFlags.get(n!("selfCast")), || globalOutput().get(n!("ActionSpeedMod"))),
                    ),
                    || Val::Num(1.0),
                );
                breakdown.multiChain(
                    &breakdown.table(n!("Speed")).unwrap_or_default(),
                    &tbl! {
                        tbl!["%.2f ^8(increased/reduced)", 1.0 + inc / 100.0],
                        tbl!["%.2f ^8(more/less)", more],
                        tbl!["%.2f ^8(action speed modifier)", actionSpeed];
                        base = tbl!["%.2f ^8(base)", 1.0 / baseTime],
                        total = lfmt!("= %.2f ^8casts per second", output.opt_num(n!("CastRate"))),
                    },
                );
                // L2451
                if output.truthy(n!("Cooldown"))
                    && (1.0 / output.num(n!("Cooldown"))) < output.num(n!("CastRate"))
                {
                    let speed = breakdown.table(n!("Speed")).unwrap_or_default();
                    // L2452
                    speed.push(lfmt!("\n"));
                    // L2453
                    speed.push(lfmt!("1 / %.2f ^8(skill cooldown)", output.opt_num(n!("Cooldown"))));
                    // L2454
                    if output.num(n!("Repeats")) > 1.0 {
                        // L2455
                        speed.push(lfmt!("x %d ^8(repeat count)", output.opt_num(n!("Repeats"))));
                    }
                    // L2457
                    speed.push(lfmt!(
                        "= %.2f ^8(casts per second)",
                        output.num(n!("Repeats")) / output.num(n!("Cooldown"))
                    ));
                    // L2458
                    speed.push(lfmt!("\n"));
                    // L2459
                    speed.push(lfmt!("= %.2f ^8(lower of cast rates)", output.opt_num(n!("Speed"))));
                }
            }
            // L2462
            if let Some(breakdown) = breakdown
                .as_ref()
                .filter(|_| calcLib::r#mod(&skillModList, skillCfg_, nm!["SkillAttackTime"]) > 0.0)
            {
                // L2463
                breakdown.set(n!("Time"), Table::new());
                // L2464
                breakdown.multiChain(
                    &breakdown.table(n!("Time")).unwrap_or_default(),
                    &tbl! {
                        tbl!["%.2f ^8(total modifier)", calcLib::r#mod(&skillModList, skillCfg_, nm!["SkillAttackTime"])];
                        base = tbl![
                            "%.2f ^8(base)",
                            1.0 / (output.num(n!("Speed"))
                                * calcLib::r#mod(&skillModList, skillCfg_, nm!["SkillAttackTime"]))
                        ],
                        total = lfmt!("= %.2f ^8seconds per attack", output.opt_num(n!("Time"))),
                    },
                );
            }
        }
        // L2471
        if skillData.truthy(n!("hitTimeOverride")) && !skillData.truthy(n!("triggeredOnDeath")) {
            // L2472
            output.set(n!("HitTime"), skillData.get(n!("hitTimeOverride")));
            // L2473
            output.set(n!("HitSpeed"), 1.0 / output.num(n!("HitTime")));
            // L2474 Brands always have hitTimeOverride
            // L2475
            if hasSkillType(ctx, SkillType::Brand)
                && !skillModList.Flag(None, nm!["UnlimitedBrandDuration"])
            {
                // L2476
                output.set(
                    n!("BrandTicks"),
                    m_floor(output.num(n!("Duration")) * output.num(n!("HitSpeed"))),
                );
            }
        } else if skillData.truthy(n!("hitTimeMultiplier"))
            && output.truthy(n!("Time"))
            && !skillData.truthy(n!("triggeredOnDeath"))
        {
            // L2478
            // L2479
            output.set(
                n!("HitTime"),
                output.num(n!("Time")) * skillData.num(n!("hitTimeMultiplier")),
            );
            // L2480
            if output.truthy(n!("Cooldown")) && skillData.truthy(n!("triggered")) {
                // L2481
                output.set(
                    n!("HitSpeed"),
                    1.0 / (m_max(output.num(n!("HitTime")), output.num(n!("Cooldown")))),
                );
            } else if output.truthy(n!("Cooldown")) {
                // L2482
                // L2483
                output.set(
                    n!("HitSpeed"),
                    1.0 / (output.num(n!("HitTime")) + output.num(n!("Cooldown"))),
                );
            } else {
                // L2485
                output.set(n!("HitSpeed"), 1.0 / output.num(n!("HitTime")));
            }
        }
    });
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    // L2489 Other Misc DPS multipliers (like custom source)
    // L2490
    skillData.set(
        n!("dpsMultiplier"),
        (skillData.num_or(n!("dpsMultiplier"), 1.0))
            * (1.0 + skillModList.Sum(INC, skillCfg_, nm!["DPS"]) / 100.0)
            * skillModList.More(skillCfg_, nm!["DPS"]),
    );
    // L2491
    if hasSkillType(ctx, SkillType::Brand) && !skillData.truthy(n!("countsAttachedBrandsInDamage")) {
        // L2492
        skillData.set(
            n!("dpsMultiplier"),
            skillData.num(n!("dpsMultiplier")) * output.num(n!("AttachedBrandCount")),
        );
        // L2493
        output.set(
            n!("SkillDPSMultiplier"),
            (output.num_or(n!("SkillDPSMultiplier"), 1.0)) * output.num(n!("AttachedBrandCount")),
        );
    }
    // L2495
    if env.configInput.str(n!("repeatMode")).as_deref() == Some("FINAL")
        || skillModList.Flag(None, nm!["OnlyFinalRepeat"])
    {
        // L2496
        skillData.set(
            n!("dpsMultiplier"),
            skillData.num(n!("dpsMultiplier")) / (output.num_or(n!("Repeats"), 1.0)),
        );
    }
    // L2498
    if skillModList.Flag(None, nm!["TriggeredBySnipe"]) {
        // L2499
        skillFlags.set(n!("channelRelease"), true);
    }
    // L2501
    if let Some(breakdown) = &breakdown {
        // L2502
        breakdown.set(n!("SustainableTrauma"), ctx.storedSustainedTraumaBreakdown.clone());
    }
    // L2504 (`Flag` returns true or nil, so the key is nil without trauma)
    output.set(
        n!("SustainableTrauma"),
        if skillModList.Flag(None, nm!["HasTrauma"]) {
            Val::Num(skillModList.Sum(BASE, skillCfg_, nm!["Multiplier:SustainableTraumaStacks"]))
        } else {
            Val::Nil
        },
    );
    // L2505 Mantra of Flames buff count
    // L2506 LUA-GLOBAL-NIL: no `cfg` is in scope here (the global `cfg` is nil)
    modDB.multipliers.set(
        n!("BuffOnSelf"),
        (modDB.multipliers.num0(n!("BuffOnSelf")))
            + skillModList.Sum(BASE, None, nm!["Multiplier:TraumaStacks"]),
    );
    // L2507 LUA-GLOBAL-NIL: as 2506
    modDB.multipliers.set(
        n!("BuffOnSelf"),
        (modDB.multipliers.num0(n!("BuffOnSelf")))
            + skillModList.Sum(BASE, None, nm!["Multiplier:VoltaxicWaitingStages"]),
    );
    // L2508
    if isAttack {
        // L2509 Combine hit chance and attack speed
        // L2510
        combineStat(ctx, n!("AccuracyHitChance"), n!("AVERAGE"), None);
        // L2511
        combineStat(ctx, n!("HitChance"), n!("AVERAGE"), None);
        // L2512
        combineStat(ctx, n!("Speed"), n!("HARMONICMEAN"), None);
        // L2513
        combineStat(ctx, n!("HitSpeed"), n!("OR"), None);
        // L2514
        combineStat(ctx, n!("HitTime"), n!("OR"), None);
        // L2515
        if output.get(n!("Speed")).raw_eq(&Val::Num(0.0)) {
            // L2516
            output.set(n!("Time"), 0.0);
        } else {
            // L2518
            output.set(n!("Time"), 1.0 / output.num(n!("Speed")));
        }

        // L2521
        if output.num(n!("Time")) > 1.0 {
            // L2522 (no source: the mod's source is nil)
            modDB.AddMod(Arc::new(pob_gamedata::mod_tools::createMod(
                n!("Condition:OneSecondAttackTime"),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[],
            )));
        }
        // L2524
        if skillModList.Flag(None, nm!["UseOffhandAttackSpeed"])
            && !skillFlags.truthy(n!("forceMainHand"))
        {
            let offHand = output.get(n!("OffHand"));
            // L2525
            output.set(n!("Speed"), offHand.get(n!("Speed")));
            // L2526
            output.set(n!("Time"), output.get(n!("OffHand")).get(n!("Time")));
            // L2527
            if let Some(breakdown) = &breakdown {
                // L2528
                breakdown.set(
                    n!("Speed"),
                    tbl![
                        "Use Offhand Weapon Attack Speed:",
                        lfmt!("= %.2f", output.opt_num(n!("Speed")))
                    ],
                );
            }
        } else if skillFlags.truthy(n!("bothWeaponAttack")) {
            // L2533
            // L2534
            if let Some(breakdown) = &breakdown {
                // L2535
                breakdown.set(
                    n!("Speed"),
                    tbl![
                        "Both weapons:",
                        lfmt!(
                            "2 / (1 / %.2f + 1 / %.2f)",
                            output.get(n!("MainHand")).get(n!("Speed")).opt_num(),
                            output.get(n!("OffHand")).get(n!("Speed")).opt_num()
                        ),
                        lfmt!("= %.2f", output.opt_num(n!("Speed")))
                    ],
                );
            }
        }
        // L2542
        if let Some(breakdown) = &breakdown {
            // L2543
            if skillFlags.truthy(n!("bothWeaponAttack")) {
                // L2544
                breakdown.set(
                    n!("HitChance"),
                    tbl![
                        "Both weapons:",
                        lfmt!(
                            "%.2f%% ^8(main hand)",
                            output.get(n!("MainHand")).get(n!("HitChance")).opt_num()
                        ),
                        lfmt!(
                            "%.2f%% ^8(off hand)",
                            output.get(n!("OffHand")).get(n!("HitChance")).opt_num()
                        ),
                        lfmt!("= %.2f%% ^8(average)", output.opt_num(n!("HitChance")))
                    ],
                );
            } else {
                // L2551
                let handBreakdown = lor(
                    land(skillFlags.get(n!("weapon1Attack")), || breakdown.get(n!("MainHand"))),
                    || breakdown.get(n!("OffHand")),
                );
                // L2552
                breakdown.set(
                    n!("HitChance"),
                    lor(index(&handBreakdown, n!("HitChance")), || {
                        index(&handBreakdown, n!("AccuracyHitChance"))
                    }),
                );
                // L2553
                breakdown.set(
                    n!("Speed"),
                    lor(breakdown.get(n!("Speed")), || index(&handBreakdown, n!("Speed"))),
                );
            }
        }
        // L2556
        if skillData.truthy(n!("hitTimeOverride")) && !skillData.truthy(n!("triggeredOnDeath")) {
            // L2557
            output.set(n!("HitTime"), skillData.get(n!("hitTimeOverride")));
            // L2558
            output.set(n!("HitSpeed"), 1.0 / output.num(n!("HitTime")));
        } else if skillData.truthy(n!("hitTimeMultiplier"))
            && output.truthy(n!("Time"))
            && !skillData.truthy(n!("triggeredOnDeath"))
        {
            // L2559
            // L2560
            output.set(
                n!("HitTime"),
                output.num(n!("Time")) * skillData.num(n!("hitTimeMultiplier")),
            );
            // L2561
            if output.truthy(n!("Cooldown")) && skillData.truthy(n!("triggered")) {
                // L2562
                output.set(
                    n!("HitSpeed"),
                    1.0 / (m_max(output.num(n!("HitTime")), output.num(n!("Cooldown")))),
                );
            } else if output.truthy(n!("Cooldown")) {
                // L2563
                // L2564
                output.set(
                    n!("HitSpeed"),
                    1.0 / (output.num(n!("HitTime")) + output.num(n!("Cooldown"))),
                );
            } else {
                // L2566
                output.set(
                    n!("HitSpeed"),
                    m_min(1.0 / output.num(n!("HitTime")), data().misc.ServerTickRate),
                );
            }
        }
    }
    // L2570
    if let Some(breakdown) = &breakdown {
        // L2571
        if skillData.truthy(n!("hitTimeOverride")) && !skillData.truthy(n!("triggeredOnDeath")) {
            // L2572
            breakdown.set(n!("HitSpeed"), Table::new());
            let hitSpeed = breakdown.table(n!("HitSpeed")).unwrap_or_default();
            // L2573
            hitSpeed.push(lfmt!("1 / %.2f ^8(hit time override)", output.opt_num(n!("HitTime"))));
            // L2574
            hitSpeed.push(lfmt!("= %.2f", output.opt_num(n!("HitSpeed"))));
        } else if skillData.truthy(n!("hitTimeMultiplier"))
            && output.truthy(n!("Time"))
            && !skillData.truthy(n!("triggeredOnDeath"))
        {
            // L2575
            // L2576
            breakdown.set(n!("HitTime"), Table::new());
            let hitTime = breakdown.table(n!("HitTime")).unwrap_or_default();
            // L2577
            let hitTimeMultiplier = skillData.num(n!("hitTimeMultiplier"));
            if m_floor(hitTimeMultiplier) != hitTimeMultiplier {
                // L2578
                let fmt = format!(
                    "{}NOTE: First stage has a %.2fx channel time multiplier",
                    colorCode("CUSTOM")
                );
                hitTime.push(lfmt!(
                    fmt.as_str(),
                    skillData.num(n!("hitTimeMultiplier"))
                        - m_floor(skillData.num(n!("hitTimeMultiplier")))
                ));
            }
            // L2580
            if isAttack {
                // L2581
                hitTime.push(lfmt!("%.2f ^8(attack time)", output.opt_num(n!("Time"))));
            } else {
                // L2583
                hitTime.push(lfmt!("%.2f ^8(cast time)", output.opt_num(n!("Time"))));
            }
            // L2585
            hitTime.push(lfmt!(
                "x %.2f ^8(channel time multiplier)",
                skillData.opt_num(n!("hitTimeMultiplier"))
            ));
            // L2586
            hitTime.push(lfmt!("= %.2f", output.opt_num(n!("HitTime"))));
            // L2587
            breakdown.set(n!("HitSpeed"), Table::new());
            let hitSpeed = breakdown.table(n!("HitSpeed")).unwrap_or_default();
            // L2588
            if output.truthy(n!("Cooldown")) && skillData.truthy(n!("triggered")) {
                // L2589
                hitSpeed.push(lfmt!(
                    "1 / min(%.2f, %.2f) ^8min(hit time, cooldown)",
                    output.opt_num(n!("HitTime")),
                    output.opt_num(n!("Cooldown"))
                ));
            } else if output.truthy(n!("Cooldown")) {
                // L2590
                // L2591
                hitSpeed.push(lfmt!(
                    "1 / (%.2f + %.2f) ^8(hit time + cooldown)",
                    output.opt_num(n!("HitTime")),
                    output.opt_num(n!("Cooldown"))
                ));
            } else {
                // L2593
                hitSpeed.push(lfmt!("1 / %.2f ^8(hit time)", output.opt_num(n!("HitTime"))));
            }
            // L2595
            hitSpeed.push(lfmt!("= %.2f", output.opt_num(n!("HitSpeed"))));
        }
    }

    // L2599 Grab quantity multiplier
    // L2600
    ctx.quantityMultiplier = m_max(
        ctx.activeSkill.skillModList.get().Sum(
            BASE,
            Some(&*ctx.activeSkill.skillCfg.get()),
            nm!["QuantityMultiplier"],
        ),
        1.0,
    );
    // L2601
    if ctx.quantityMultiplier > 1.0 {
        // L2602
        output.set(n!("QuantityMultiplier"), ctx.quantityMultiplier);
    }

    // L2605 Calculate damage (exerts, crits, ruthless, DPS, etc)
    Flow::Normal
}
