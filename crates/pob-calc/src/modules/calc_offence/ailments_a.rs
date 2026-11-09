//! @pob-file Modules/CalcOffence.lua @16de4b82d
//! offence ailment prelude and ailment loop body A, OF-G (CalcOffence.lua 4155-4989). The local
//! functions `checkWeapon1HFlags` (4308), `calcAverageSourceDamage` (4315) and `calcAilmentDamage`
//! (4333) cross into OF-H, so they take the ctx and the loop locals (§7.2).
//!
//! Owner: OF-G. The signatures of the three loop-local functions follow the Lua (CCR-0035: the
//! U0 stub had other parameter lists and return types).
//!
//! Notes on the port:
//! * `globalOutput`/`globalBreakdown` are assigned at 4165 and not reassigned in this segment, so
//!   the segment reads them through the handles it assigned (same tables as `OFFENCE_STATE`).
//! * The dot cfgs' `skillCond` metatable (4422, 4695) is `IndexMeta::OrChain` over the
//!   `skillCfg.skillCond` / `cfg.skillCond` handles (contracts.md#index-meta).
//! * The module-level locals `dmgTypeList` (35) and `dmgTypeFlags` (36-46) of OF-A's lines are
//!   read here as the private constants below (immutable data).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use super::helpers_setup::{calcAilmentSourceDamage, calcResistForType};
use super::{AilmentPassLocals, OffenceCtx};
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;

/// `dmgTypeList` (CalcOffence.lua:35; OF-A's module-level local, immutable).
#[allow(dead_code)] // read by calcAverageSourceDamage, whose callers are OF-H's
const dmgTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
/// `dmgTypeFlags.flags.Chaos` (CalcOffence.lua:44).
const dmgTypeFlags_Chaos: u32 = 0x10;

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

/// A local declared without a value and assigned in the sub-pass loop: nil in arithmetic raises.
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

/// `1 + (skillModList:Override(dotCfg, "DotMultiplier") or skillModList:Sum("BASE", dotCfg,
/// "DotMultiplier") + skillModList:Sum("BASE", dotCfg, <typeDotMultiplier>)) / 100` (4560/4563/4566,
/// 4848/4851/4854): one expression, evaluated left to right.
fn dotMultiOf(skillModList: &ModStore, dotCfg: &ModCfg, typeDotMultiplier: Name) -> f64 {
    let d = Some(dotCfg);
    let v = match ov(skillModList.Override(d, nm!["DotMultiplier"])) {
        Some(o) => o,
        None => {
            skillModList.Sum(ModType::BASE, d, nm!["DotMultiplier"])
                + skillModList.Sum(ModType::BASE, d, &[typeDotMultiplier])
        }
    };
    1.0 + v / 100.0
}

/// The dot cfg literal of 4413-4424 (bleed) / 4686-4697 (poison).
fn newDotCfg(skillCfg: &ModCfg, cfg: &ModCfg, keywordFlags: u32) -> Cfg {
    let c = ModCfg::default();
    // skillName = skillCfg.skillName, …
    c.skillName.set(skillCfg.skillName.get());
    c.skillPart.set(skillCfg.skillPart.get());
    c.skillTypes.set(skillCfg.skillTypes.get());
    c.summonSkillName.set(skillCfg.summonSkillName.get());
    c.slotName.set(skillCfg.slotName.get());
    // flags = bor(ModFlag.Dot, ModFlag.Ailment, band(cfg.flags, ModFlag.WeaponMask), band(cfg.flags, ModFlag.Melee) ~= 0 and ModFlag.MeleeHit or 0)
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
    // keywordFlags = bor(band(cfg.keywordFlags, bnot(KeywordFlag.Hit)), <kw>…)
    c.keywordFlags
        .set((cfg.keywordFlags.get() & !KeywordFlag::Hit) | keywordFlags);
    // skillCond = setmetatable({["CriticalStrike"] = true }, { __index = function(table, key) return skillCfg.skillCond[key] or cfg.skillCond[key] end } )
    let sc = Table::with_meta(IndexMeta::OrChain(smallvec::smallvec![
        skillCond(skillCfg),
        skillCond(cfg)
    ]));
    sc.set(n!("CriticalStrike"), true);
    c.skillCond.set(Some(sc));
    // skillDist = skillCfg.skillDist
    c.skillDist.set(skillCfg.skillDist.get());
    Rc::new(c)
}

/// `skillModList:NewMod("DotMultiplier", "BASE", v, mod.source, ModFlag.Ailment, {CriticalStrike},
/// handCondition, unpack(mod))` (4181). The constructor `{ …, select(tagStart, ...) }` stores the
/// varargs at 1..n: with `handCondition` nil, `mod[2]` is nil and the copied tags sit at 3.. where
/// `ipairs` never reaches them (kept as `extra["#n"]`, contracts.md#mod-quirks).
fn newPerfectAgonyMod(
    skillModList: &ModStore,
    value: f64,
    source: Option<&str>,
    handCondition: Option<Tag>,
    copied: &[Tag],
) {
    let crit = pob_gamedata::tag!({ type = Condition, var = "CriticalStrike" });
    let mut m = pob_gamedata::mod_tools::createMod(
        n!("DotMultiplier"),
        ModType::BASE,
        value,
        source,
        ModFlag::Ailment,
        0,
        &[],
    );
    m.tags.push(crit);
    match handCondition {
        Some(h) => {
            m.tags.push(h);
            m.tags.extend(copied.iter().cloned());
        }
        None => {
            let mut extra = pob_gamedata::ValueTable::new();
            for (i, t) in copied.iter().enumerate() {
                extra.set(
                    Name::from(format!("#{}", i + 3).as_str()),
                    Value::Table(Arc::new(pob_gamedata::modifier::tag_to_value_table(t))),
                );
            }
            if !extra.is_empty() {
                m.extra = Some(Box::new(extra));
            }
        }
    }
    skillModList.AddMod(Arc::new(m));
}

// ---- segments ---------------------------------------------------------------------------------

/// @pob Modules/CalcOffence.lua:4155-4163 h=64e58855
/// OF-G: CalcOffence.lua 4155-4163 (ailment prelude).
pub(crate) fn seg_ailmentPrelude(ctx: &mut OffenceCtx) -> Flow {
    // -- Ailment + Non Damaging Ailment Section
    // L4156
    ctx.ailmentData = Some(&data().nonDamagingAilment);
    // L4157
    for ailment in &data().ailmentTypeList {
        // L4158
        ctx.skillFlags
            .set(Name::from(pob_lua::string::lower(ailment).as_str()), false);
    }
    // L4160 (Flag returns true or nil)
    let v = ctx
        .skillModList
        .Flag(Some(&ctx.skillCfg), nm!["IgniteCanStack"]);
    ctx.skillFlags.set(
        n!("igniteCanStack"),
        if v { Val::Bool(true) } else { Val::Nil },
    );
    // L4161
    let v = ctx
        .skillModList
        .Flag(Some(&ctx.skillCfg), nm!["IgniteToChaos"]);
    ctx.skillFlags.set(
        n!("igniteToChaos"),
        if v { Val::Bool(true) } else { Val::Nil },
    );
    // L4162
    ctx.skillFlags.set(n!("impale"), false);
    Flow::Normal
}

/// @pob Modules/CalcOffence.lua:4165-4989 h=cb5977f2
/// OF-G: CalcOffence.lua 4165-4989 (sets `globalOutput` at 4165).
pub(crate) fn seg_bleedPoison(ctx: &mut OffenceCtx, al: &mut AilmentPassLocals) -> Flow {
    // --Calculate ailments and debuffs (poison, bleed, ignite, impale, exposure, etc)
    // L4165
    OFFENCE_STATE.with(|s| {
        s.globalOutput.set(Some(ctx.output.clone()));
        s.globalBreakdown.set(ctx.breakdown.clone());
    });
    let globalOutput: Output = ctx.output.clone();
    let globalBreakdown: Option<Breakdown> = ctx.breakdown.clone();
    // L4166
    let _source = al.pass.get(n!("source"));
    // ---@class Output
    // L4168
    al.output = al
        .pass
        .table(n!("output"))
        .unwrap_or_else(|| lua_error!("attempt to index local 'output' (a nil value)"));
    // L4169
    al.cfg = al
        .pass
        .obj::<ModCfg>(n!("cfg"))
        .unwrap_or_else(|| lua_error!("attempt to index local 'cfg' (a nil value)"));
    // ---@class Breakdown
    // L4171
    al.breakdown = al
        .pass
        .obj::<Breakdown>(n!("breakdown"))
        .map(|b| (*b).clone());

    let env = ctx.env.clone();
    let activeSkill = ctx.activeSkill.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let skillModList = ctx.skillModList.clone();
    let skillData = ctx.skillData.clone();
    let skillFlags = ctx.skillFlags.clone();
    let skillCfg = ctx.skillCfg.clone();
    let canDeal = ctx.canDeal.clone();
    let output = al.output.clone();
    let cfg = al.cfg.clone();
    let breakdown = al.breakdown.clone();
    let pass = al.pass.clone();
    let c: Option<&ModCfg> = Some(&cfg);
    let misc = &data().misc;
    let label = pass.str(n!("label"));
    let isOffHand = label.as_deref() == Some("Off Hand");

    // L4173 do -- Perfect Agony
    {
        // L4174
        let handCondition: Option<Tag> = if isOffHand {
            Some(pob_gamedata::tag!({ type = Condition, var = "OffHandAttack" }))
        } else if label.as_deref() == Some("Main Hand") {
            Some(pob_gamedata::tag!({ type = Condition, var = "MainHandAttack" }))
        } else {
            None
        };
        // -- Note: This section is the legacy implementation of 'Perfect Agony'
        // L4176
        if skillModList.Sum(ModType::BASE, None, nm!["CritMultiplierAppliesToDegen"]) > 0.0 {
            // L4177
            for value in skillModList.Tabulate(Some(ModType::BASE), c, nm!["CritMultiplier"]) {
                // L4178
                let r#mod = value.r#mod.clone();
                // L4179
                if r#mod.source.as_deref() != Some("Base") {
                    // -- The global base Crit Multi doesn't apply to ailments with Perfect Agony
                    // L4180
                    let v = r#mod.value.num().unwrap_or_else(|| {
                        lua_error!(
                            "attempt to perform arithmetic on field 'value' (a {} value)",
                            r#mod.value.lua_type()
                        )
                    });
                    newPerfectAgonyMod(
                        &skillModList,
                        m_floor(v / 2.0),
                        r#mod.source.as_deref(),
                        handCondition.clone(),
                        &r#mod.tags,
                    );
                }
            }
        }

        // L4185
        if skillModList.Flag(None, nm!["DotMultiplierIsCritMultiplier"]) {
            // -- On enemy crit multiplier effects also apply for Perfect Agony: https://www.pathofexile.com/forum/view-thread/3532389#13
            // L4187
            let mut multiOverride = ov(skillModList.Override(c, nm!["CritMultiplier"]));
            // L4188
            if let Some(m) = multiOverride {
                // L4189
                multiOverride = Some(m - 100.0);
            }
            // L4191
            let base = match multiOverride {
                Some(m) => m,
                None => skillModList.Sum(ModType::BASE, c, nm!["CritMultiplier"]),
            };
            let v = base + enemyDB.Sum(ModType::BASE, c, nm!["SelfCritMultiplier"]);
            let mut tags: Vec<Tag> =
                vec![pob_gamedata::tag!({ type = Condition, var = "CriticalStrike" })];
            if let Some(h) = handCondition.clone() {
                tags.push(h);
            }
            skillModList.NewMod(
                n!("DotMultiplier"),
                ModType::OVERRIDE,
                v,
                "Perfect Agony",
                ModFlag::Ailment,
                0,
                &tags,
            );
        }
    }

    // -- Calculate chance to inflict secondary dots/status effects
    // L4196
    skillCond(&cfg).set(n!("CriticalStrike"), true);
    // L4197
    if !skillFlags.truthy(n!("attack")) || skillModList.Flag(c, nm!["CannotBleed"]) {
        // L4198
        output.set(n!("BleedChanceOnCrit"), 0.0);
    } else {
        // L4200
        let v = m_min(
            100.0,
            skillModList.Sum(ModType::BASE, c, nm!["BleedChance"])
                + enemyDB.Sum(ModType::BASE, None, nm!["SelfBleedChance"]),
        );
        output.set(n!("BleedChanceOnCrit"), v);
    }
    // L4202
    if !skillFlags.truthy(n!("hit")) || skillModList.Flag(c, nm!["CannotPoison"]) {
        // L4203
        output.set(n!("PoisonChanceOnCrit"), 0.0);
    } else {
        // L4205
        let v = m_min(
            100.0,
            skillModList.Sum(ModType::BASE, c, nm!["PoisonChance"])
                + enemyDB.Sum(ModType::BASE, None, nm!["SelfPoisonChance"]),
        );
        output.set(n!("PoisonChanceOnCrit"), v);
    }
    // L4207
    if !skillFlags.truthy(n!("hit")) {
        // L4208
        output.set(n!("ImpaleChanceOnCrit"), 0.0);
    } else {
        // L4210
        let v = if env.mode_effective.get() {
            m_min(
                100.0,
                skillModList.Sum(ModType::BASE, c, nm!["ImpaleChance"]),
            )
        } else {
            0.0
        };
        output.set(n!("ImpaleChanceOnCrit"), v);
    }
    // L4212
    if !skillFlags.truthy(n!("hit")) || skillModList.Flag(c, nm!["CannotKnockback"]) {
        // L4213
        output.set(n!("KnockbackChanceOnCrit"), 0.0);
    } else {
        // L4215
        let v = skillModList.Sum(ModType::BASE, c, nm!["EnemyKnockbackChance"]);
        output.set(n!("KnockbackChanceOnCrit"), v);
    }
    // L4217
    skillCond(&cfg).set(n!("CriticalStrike"), false);
    // L4218
    if !skillFlags.truthy(n!("attack")) || skillModList.Flag(c, nm!["CannotBleed"]) {
        // L4219
        output.set(n!("BleedChanceOnHit"), 0.0);
    } else {
        // L4221
        let v = m_min(
            100.0,
            skillModList.Sum(ModType::BASE, c, nm!["BleedChance"])
                + enemyDB.Sum(ModType::BASE, None, nm!["SelfBleedChance"]),
        );
        output.set(n!("BleedChanceOnHit"), v);
    }
    // L4223
    if !skillFlags.truthy(n!("hit")) || skillModList.Flag(c, nm!["CannotPoison"]) {
        // L4224
        output.set(n!("PoisonChanceOnHit"), 0.0);
        // L4225
        output.set(n!("ChaosPoisonChance"), 0.0);
    } else {
        // L4227
        let v = m_min(
            100.0,
            skillModList.Sum(ModType::BASE, c, nm!["PoisonChance"])
                + enemyDB.Sum(ModType::BASE, None, nm!["SelfPoisonChance"]),
        );
        output.set(n!("PoisonChanceOnHit"), v);
        // L4228
        let v = m_min(
            100.0,
            skillModList.Sum(ModType::BASE, c, nm!["ChaosPoisonChance"]),
        );
        output.set(n!("ChaosPoisonChance"), v);
    }
    // -- Elemental Ailment Affliction Chance | Elemental Ailment Additionals
    // L4231
    for &ailment in &data().elementalAilmentTypeList {
        // L4232
        let mut chance = skillModList.Sum(ModType::BASE, c, &[cat!("Enemy", ailment, "Chance")])
            + enemyDB.Sum(ModType::BASE, None, &[cat!("Self", ailment, "Chance")]);
        // L4233 (`A and 100 or 0 + B` is `A and 100 or (0 + B)`)
        let avoidSum = if enemyDB.Flag(
            None,
            &[cat!(ailment, "Immune"), n!("ElementalAilmentImmune")],
        ) {
            100.0
        } else {
            0.0 + enemyDB.Sum(
                ModType::BASE,
                None,
                &[
                    cat!("Avoid", ailment),
                    n!("AvoidAilments"),
                    n!("AvoidElementalAilments"),
                ],
            )
        };
        let avoid = 1.0 - m_min(avoidSum, 100.0) / 100.0;
        // L4234
        if ailment.as_str() == "Chill" {
            // L4235
            chance = 100.0;
        }
        // L4237
        chance = chance * avoid;
        // -- Warden's Oath of Summer Scorch Chance
        // L4239
        if ailment.as_str() == "Ignite" && env.modDB.get().Flag(None, nm!["IgniteCanScorch"]) {
            // L4240
            output.set(n!("ScorchChance"), m_min(100.0, chance));
            // L4241
            skillModList.NewMod(
                n!("EnemyScorchChance"),
                ModType::BASE,
                chance,
                "Ignite Chance",
                0,
                0,
                &[],
            );
        }
        // L4243
        if skillFlags.truthy(n!("hit"))
            && !(skillModList.Flag(c, &[cat!("Cannot", ailment)]) || avoid == 0.0)
        {
            // L4244
            output.set(cat!(ailment, "ChanceOnHit"), m_min(100.0, chance));
            // L4245 -- e.g. Painseeker
            let alt = skillModList.Flag(c, &[cat!("CritsDontAlways", ailment)]) || {
                // L4246 -- e.g. Secrets of Suffering
                let ad = ctx.ailmentData.unwrap_or_else(|| {
                    lua_error!("attempt to index upvalue 'ailmentData' (a nil value)")
                });
                ad.get(&ailment).is_some_and(|a| a.alt)
                    && !skillModList.Flag(c, nm!["CritAlwaysAltAilments"])
            };
            if alt {
                // L4247
                let v = output.get(cat!(ailment, "ChanceOnHit"));
                output.set(cat!(ailment, "ChanceOnCrit"), v);
            } else {
                // L4249
                output.set(cat!(ailment, "ChanceOnCrit"), 100.0);
            }
        } else {
            // L4252
            output.set(cat!(ailment, "ChanceOnHit"), 0.0);
            // L4253
            output.set(cat!(ailment, "ChanceOnCrit"), 0.0);
        }
        // -- Warden's Oath of Summer Scorch on Crit Chance
        // L4256
        if ailment.as_str() == "Scorch"
            && env.modDB.get().Flag(None, nm!["IgniteCanScorch"])
            && avoid != 0.0
        {
            // L4257
            output.set(n!("ScorchChanceOnCrit"), 100.0);
        }
        // L4259
        let onHit = output.num(cat!(ailment, "ChanceOnHit"));
        let onCrit = if skillModList.Flag(c, nm!["NeverCrit"]) {
            0.0
        } else {
            output.num(cat!(ailment, "ChanceOnCrit"))
        };
        if (onHit + onCrit) > 0.0 {
            // L4260
            skillFlags.set(cat!("inflict", ailment), true);
        }
    }
    // L4263
    if !skillFlags.truthy(n!("hit")) || skillModList.Flag(c, nm!["CannotKnockback"]) {
        // L4264
        output.set(n!("KnockbackChanceOnHit"), 0.0);
    } else {
        // L4266
        let v = skillModList.Sum(ModType::BASE, c, nm!["EnemyKnockbackChance"]);
        output.set(n!("KnockbackChanceOnHit"), v);
    }
    // L4268
    let v = if env.mode_effective.get() {
        m_min(
            100.0,
            skillModList.Sum(ModType::BASE, c, nm!["ImpaleChance"]),
        )
    } else {
        0.0
    };
    output.set(n!("ImpaleChance"), v);
    // L4269
    if skillModList.Sum(ModType::BASE, c, nm!["FireExposureChance"]) > 0.0 {
        // L4270
        skillFlags.set(n!("applyFireExposure"), true);
    }
    // L4272
    if skillModList.Sum(ModType::BASE, c, nm!["ColdExposureChance"]) > 0.0 {
        // L4273
        skillFlags.set(n!("applyColdExposure"), true);
    }
    // L4275
    if skillModList.Sum(ModType::BASE, c, nm!["LightningExposureChance"]) > 0.0 {
        // L4276
        skillFlags.set(n!("applyLightningExposure"), true);
    }
    // L4278
    if env.mode_effective.get() {
        // L4279
        for &ailment in &data().ailmentTypeList {
            // L4280
            let mult = if enemyDB.Flag(None, &[cat!(ailment, "Immune")]) {
                0.0
            } else {
                1.0 - enemyDB.Sum(ModType::BASE, None, &[cat!("Avoid", ailment)]) / 100.0
            };
            // L4281
            let v = output.num(cat!(ailment, "ChanceOnHit")) * mult;
            output.set(cat!(ailment, "ChanceOnHit"), v);
            // L4282
            let v = output.num(cat!(ailment, "ChanceOnCrit")) * mult;
            output.set(cat!(ailment, "ChanceOnCrit"), v);
            // L4283
            if ailment.as_str() == "Poison" {
                // L4284
                let v = output.num(n!("ChaosPoisonChance")) * mult;
                output.set(n!("ChaosPoisonChance"), v);
            }
        }
    }

    // L4289
    let ailmentMode: Name = match env.configInput.get(n!("ailmentMode")) {
        Val::Nil | Val::Bool(false) => n!("AVERAGE"),
        Val::Str(s) => Name::from(&*s),
        other => lua_error!("ailmentMode is a {}", other.lua_type()),
    };
    al.ailmentMode = Some(ailmentMode);
    // L4290
    if ailmentMode.as_str() == "CRIT" || modDB.Flag(None, nm!["AilmentsOnlyFromCrit"]) {
        // L4291
        for &ailment in &data().ailmentTypeList {
            // L4292
            output.set(cat!(ailment, "ChanceOnHit"), 0.0);
        }
    }

    // -- Note: "Elemental Overload" and "Perfect Agony" interactions as per wiki: https://www.poewiki.net/wiki/Elemental_Overload#Mechanics
    // -- As of 3.25 EO also disables chance-based modifiers exclusive to critical strikes like "Adder's Touch"
    // -- Perfect Agony + Elemental Overload completely disables ailment application
    // L4299
    if modDB.Flag(None, nm!["AilmentsAreNeverFromCrit"]) {
        // L4300
        for &ailment in &data().ailmentTypeList {
            // L4301
            let v = if modDB.Flag(None, nm!["AilmentsOnlyFromCrit"]) {
                Val::Num(0.0)
            } else {
                output.get(cat!(ailment, "ChanceOnHit"))
            };
            output.set(cat!(ailment, "ChanceOnCrit"), v);
        }
    }

    // -- address Weapon1H interaction with Ailment for nodes like Sleight of Hand
    // -- bit-and on cfg.flags confirms if the skill has the 1H flag
    // -- if so bit-or on the targetCfg (e.g. dotCfg) to guarantee for calculations like Sum("INC") and breakdown
    // L4308 local function checkWeapon1HFlags: `checkWeapon1HFlags` below
    // ---Calculates normal and crit damage to be used in non-damaging ailment calculations
    // L4315 local function calcAverageSourceDamage: `calcAverageSourceDamage` below
    // -- Calculate the inflict chance and base damage of a secondary effect (bleed/poison/ignite/shock/freeze)
    // L4333 local function calcAilmentDamage: `calcAilmentDamage` below

    let ctx: &OffenceCtx = ctx;
    let al: &AilmentPassLocals = al;

    // -- Calculate bleeding chance and damage
    // L4413
    if canDeal.truthy(n!("Physical"))
        && (output.num(n!("BleedChanceOnHit")) + output.num(n!("BleedChanceOnCrit"))) > 0.0
    {
        // L4414
        let newCfg = newDotCfg(
            &skillCfg,
            &cfg,
            KeywordFlag::Bleed | KeywordFlag::Ailment | KeywordFlag::PhysicalDot,
        );
        if !isOffHand {
            activeSkill.bleedCfg.set(Some(newCfg));
        } else {
            activeSkill.OHbleedCfg.set(Some(newCfg));
        }
        // L4425
        let dotCfg: Cfg = if !isOffHand {
            activeSkill.bleedCfg.get()
        } else {
            activeSkill.OHbleedCfg.get()
        }
        .unwrap_or_else(|| lua_error!("attempt to index local 'dotCfg' (a nil value)"));
        let d: Option<&ModCfg> = Some(&dotCfg);
        // L4426
        checkWeapon1HFlags(ctx, al, &dotCfg);
        // L4427
        let mut avgCritBleedDmg: Option<f64> = None;
        let mut sourceMinCritDmg: Option<f64> = None;
        let mut sourceMaxCritDmg: Option<f64> = None;
        let mut avgHitBleedDmg: Option<f64> = None;
        let mut sourceMinHitDmg: Option<f64> = None;
        let mut sourceMaxHitDmg: Option<f64> = None;
        let _ = (&mut sourceMinCritDmg, &mut sourceMaxHitDmg);
        // L4428
        if let Some(b) = &breakdown {
            // L4429
            b.set(n!("BleedPhysical"), tbl!(damageTypes = Table::new()));
        }

        // -- For bleeds we will be using a weighted average calculation
        // L4433
        let configStacks = enemyDB.Sum(ModType::BASE, None, nm!["Multiplier:BleedStacks"]);
        // L4434
        let maxStacks = match ov(skillModList.Override(c, nm!["BleedStacksMax"])) {
            Some(v) => v,
            None => skillModList.Sum(ModType::BASE, c, nm!["BleedStacksMax"]),
        };
        // L4435
        let overrideStackPotential: Option<f64> =
            match ov(skillModList.Override(None, nm!["BleedStackPotentialOverride"])) {
                Some(_) => Some(
                    nn(ov(
                        skillModList.Override(None, nm!["BleedStackPotentialOverride"])
                    )) / maxStacks,
                ),
                None => None,
            };
        // L4436
        globalOutput.set(n!("BleedStacksMax"), maxStacks);
        // L4437
        let durationBase = match ov(skillModList.Override(d, nm!["BleedDurationBase"])) {
            Some(v) => v,
            None => {
                let s = if skillData.truthy(n!("bleedDurationIsSkillDuration")) {
                    skillData.get(n!("duration"))
                } else {
                    Val::Nil
                };
                if s.truthy() {
                    s.num()
                } else {
                    misc.BleedDurationBase
                }
            }
        };
        // L4438
        let mut durationMod = calcLib::r#mod(
            &skillModList,
            d,
            &names([
                Some(n!("EnemyBleedDuration")),
                Some(n!("EnemyAilmentDuration")),
                Some(n!("DamagingAilmentDuration")),
                skillData
                    .truthy(n!("bleedIsSkillEffect"))
                    .then(|| n!("Duration")),
            ]),
        ) * calcLib::r#mod(
            &enemyDB,
            None,
            nm!["SelfBleedDuration", "SelfAilmentDuration"],
        ) / calcLib::r#mod(&enemyDB, d, nm!["BleedExpireRate"]);
        // L4439
        durationMod = m_max(durationMod, 0.0);
        // L4440
        let rateMod = calcLib::r#mod(&skillModList, c, nm!["BleedFaster"])
            + enemyDB.Sum(ModType::INC, None, nm!["SelfBleedFaster"]) / 100.0;
        // L4441
        globalOutput.set(
            n!("BleedDuration"),
            durationBase * durationMod / rateMod * ctx.debuffDurationMult,
        );

        // -- The chance any given hit applies bleed
        // L4444
        let bleedChance = output.num(n!("BleedChanceOnHit")) / 100.0
            * (1.0 - output.num(n!("CritChance")) / 100.0)
            + output.num(n!("BleedChanceOnCrit")) / 100.0 * output.num(n!("CritChance")) / 100.0;
        // -- The average number of bleeds that will be active on the enemy at once
        // L4446
        let mut bleedStacks =
            output.num(n!("HitChance")) / 100.0 * bleedChance * skillData.num(n!("dpsMultiplier"));
        // L4447
        if hitSpeedOrSpeed(&globalOutput).num() > 0.0 {
            // --assume skills with no cast, attack, or cooldown time are single cast
            // L4449
            bleedStacks = bleedStacks
                * globalOutput.num(n!("BleedDuration"))
                * hitSpeedOrSpeed(&globalOutput).num();
        }
        // L4451
        let activeTotems = match ov(env.modDB.get().Override(None, nm!["TotemsSummoned"])) {
            Some(v) => v,
            None => skillModList.Sum(
                ModType::BASE,
                Some(&skillCfg),
                nm!["ActiveTotemLimit", "ActiveBallistaLimit"],
            ),
        };
        // L4452
        if skillFlags.truthy(n!("totem")) {
            // L4453
            bleedStacks = bleedStacks * activeTotems;
        }
        // L4455
        if configStacks > 0.0 {
            // L4456
            bleedStacks = configStacks;
        }

        // L4459
        if bleedStacks < 1.0 && overrideStackPotential.unwrap_or(0.0) <= 1.0 {
            // L4460
            skillModList.NewMod(
                n!("Condition:SingleBleed"),
                ModType::FLAG,
                true,
                "bleed",
                0,
                0,
                &[],
            );
        }

        // -- ratio of bleeds applied : max effective bleeds
        // L4464
        globalOutput.set(
            n!("BleedStackPotential"),
            match overrideStackPotential {
                Some(v) => v,
                None => bleedStacks / maxStacks,
            },
        );

        // L4466
        if let Some(gb) = &globalBreakdown {
            // L4467
            gb.set(
                n!("BleedStackPotential"),
                tbl!["Ratio of bleed stacks to maximum active bleeds:"],
            );
            // L4468
            if let Some(osp) = overrideStackPotential {
                // L4469
                tins(
                    gb,
                    n!("BleedStackPotential"),
                    lfmt!("= %g ^8(stack potential override)", osp),
                );
            } else {
                // L4471
                tins(
                    gb,
                    n!("BleedStackPotential"),
                    lfmt!(
                        "%.2f ^8(chance to hit)",
                        output.num(n!("HitChance")) / 100.0
                    ),
                );
                // L4472
                tins(
                    gb,
                    n!("BleedStackPotential"),
                    lfmt!("* %.2f ^8(chance to bleed)", bleedChance),
                );
                // L4473
                if hitSpeedOrSpeed(&globalOutput).num() > 0.0 {
                    // L4474
                    let time = lor(output.get(n!("HitTime")), || output.get(n!("Time")));
                    tins(
                        gb,
                        n!("BleedStackPotential"),
                        lfmt!(
                            "* (%.2f / %.2f) ^8(Bleed duration / Attack Time)",
                            globalOutput.num(n!("BleedDuration")),
                            time.opt_num()
                        ),
                    );
                }
                // L4476
                if skillData.num(n!("dpsMultiplier")) != 1.0 {
                    // L4477
                    tins(
                        gb,
                        n!("BleedStackPotential"),
                        lfmt!(
                            "* %g ^8(DPS multiplier for this skill)",
                            skillData.num(n!("dpsMultiplier"))
                        ),
                    );
                }
                // L4479
                if skillFlags.truthy(n!("totem")) {
                    // L4480
                    tins(
                        gb,
                        n!("BleedStackPotential"),
                        lfmt!("* %d ^8(active number of Totems)", activeTotems),
                    );
                }
                // L4482
                tins(
                    gb,
                    n!("BleedStackPotential"),
                    lfmt!("/ %d ^8(max number of stacks)", maxStacks),
                );
                // L4483
                tins(
                    gb,
                    n!("BleedStackPotential"),
                    lfmt!("= %.2f", globalOutput.num(n!("BleedStackPotential"))),
                );
            }
        }

        // -- the amount of damage each bleed does as % maximum
        // L4488
        let bleedRollAverage: f64;
        // L4489
        if globalOutput.num(n!("BleedStackPotential")) > 1.0 {
            // -- shift damage towards top of range as only top bleeds apply
            // L4491
            bleedRollAverage =
                (bleedStacks - (maxStacks - 1.0) / 2.0) / (bleedStacks + 1.0) * 100.0;
        } else {
            // -- assume middle of range for hit damage
            // L4494
            bleedRollAverage = 50.0;
        }
        // L4496
        globalOutput.set(n!("BleedRollAverage"), bleedRollAverage);
        // L4497
        if let Some(gb) = &globalBreakdown {
            // L4498
            let custom = colorCode("CUSTOM");
            gb.set(
                n!("BleedRollAverage"),
                tbl![
                    lfmt!(&format!(
                        "{custom}This is the average roll of a Bleed affecting the enemy if you are constantly attacking"
                    )),
                    lfmt!(
                        &format!(
                            "{custom}If attacking constantly, your average strongest bleed currently achieves ^7%.2f%%{custom} of its max damage"
                        ),
                        bleedRollAverage
                    ),
                    lfmt!(""),
                    lfmt!("Average Bleed Roll:"),
                ],
            );
            // L4504
            if bleedStacks >= maxStacks {
                // L4505
                tins(
                    gb,
                    n!("BleedRollAverage"),
                    lfmt!("%.2f - (%.2f - 1)/2", bleedStacks, maxStacks),
                );
                // L4506
                tins(
                    gb,
                    n!("BleedRollAverage"),
                    lfmt!("/ (%.2f + 1)", bleedStacks),
                );
                // L4507
                tins(
                    gb,
                    n!("BleedRollAverage"),
                    lfmt!("= %.2f%%", bleedRollAverage),
                );
            } else {
                // L4509
                tins(
                    gb,
                    n!("BleedRollAverage"),
                    lfmt!("50%% (averaging <= %d bleeds)", maxStacks),
                );
            }
        }

        // L4513
        for sub_pass in 1..=2 {
            // L4514
            skillCond(&dotCfg).set(n!("CriticalStrike"), sub_pass != 1);

            // L4516
            let bdArg = if sub_pass == 1 {
                breakdown
                    .as_ref()
                    .and_then(|b| b.table(n!("BleedPhysical")))
            } else {
                None
            };
            let (min, max) = calcAilmentSourceDamage(
                &activeSkill,
                &output,
                &dotCfg,
                bdArg.as_ref(),
                n!("Physical"),
                0,
            );
            // L4517
            output.set(n!("BleedPhysicalMin"), min);
            // L4518
            output.set(n!("BleedPhysicalMax"), max);
            // L4519
            if sub_pass == 2 {
                // L4520
                if modDB.Flag(None, nm!["AilmentsAreNeverFromCrit"]) {
                    // L4521 -- force config to non-crit for dotMulti calculation
                    skillCond(&dotCfg).set(n!("CriticalStrike"), false);
                    // L4522
                    let v = dotMultiOf(&skillModList, &dotCfg, n!("PhysicalDotMultiplier"));
                    output.set(n!("CritBleedDotMulti"), v);
                    // L4523 -- reset to true to avoid unintended side effects
                    skillCond(&dotCfg).set(n!("CriticalStrike"), true);
                } else {
                    // L4525
                    let v = dotMultiOf(&skillModList, &dotCfg, n!("PhysicalDotMultiplier"));
                    output.set(n!("CritBleedDotMulti"), v);
                }
                // L4527
                sourceMinCritDmg = Some(min * output.num(n!("CritBleedDotMulti")));
                // L4528
                sourceMaxCritDmg = Some(max * output.num(n!("CritBleedDotMulti")));
                // L4529
                avgCritBleedDmg = Some(
                    nn(sourceMinCritDmg)
                        + (nn(sourceMaxCritDmg) - nn(sourceMinCritDmg)) * bleedRollAverage / 100.0,
                );
            } else {
                // L4531
                let v = dotMultiOf(&skillModList, &dotCfg, n!("PhysicalDotMultiplier"));
                output.set(n!("BleedDotMulti"), v);
                // L4532
                sourceMinHitDmg = Some(min * output.num(n!("BleedDotMulti")));
                // L4533
                sourceMaxHitDmg = Some(max * output.num(n!("BleedDotMulti")));
                // L4534
                avgHitBleedDmg = Some(
                    nn(sourceMinHitDmg)
                        + (nn(sourceMaxHitDmg) - nn(sourceMinHitDmg)) * bleedRollAverage / 100.0,
                );
            }
        }

        // L4538
        if let Some(b) = &breakdown {
            // L4539
            b.set(n!("BleedDPS"), Table::new());
            // L4540
            tins(
                b,
                n!("BleedDPS"),
                lfmt!(&format!(
                    "{}NOTE: Calculation uses new Weighted Avg formula for critical hits",
                    colorCode("CUSTOM")
                )),
            );
        }

        // L4543
        if let Some(gb) = &globalBreakdown {
            // L4544
            if avgHitBleedDmg == avgCritBleedDmg || output.num(n!("CritChance")) == 0.0 {
                // L4545
                gb.set(
                    n!("BleedDPS"),
                    tbl![
                        lfmt!("Dmg Derivation:"),
                        lfmt!(
                            "%.0f + (%.0f - %.0f) * %.2f%%",
                            output.num(n!("BleedPhysicalMin")),
                            output.num(n!("BleedPhysicalMax")),
                            output.num(n!("BleedPhysicalMin")),
                            bleedRollAverage
                        ),
                        lfmt!("^8min source phys + (max source phys - min source phys) * average bleed roll"),
                        lfmt!("* %.2f ^8(Bleed DoT Multi)", output.num(n!("BleedDotMulti"))),
                        lfmt!("= %.2f", nn(avgHitBleedDmg)),
                    ],
                );
            } else {
                // L4553
                gb.set(
                    n!("BleedDPS"),
                    tbl![
                        lfmt!("Non-Crit Dmg Derivation:"),
                        lfmt!(
                            "%.0f + (%.0f - %.0f) * %.2f%%",
                            output.num(n!("BleedPhysicalMin")),
                            output.num(n!("BleedPhysicalMax")),
                            output.num(n!("BleedPhysicalMin")),
                            bleedRollAverage
                        ),
                        lfmt!("^8min source phys + (max source phys - min source phys) * average bleed roll"),
                        lfmt!(
                            "* %.2f ^8(Bleed DoT Multi for Non-Crit)",
                            output.num(n!("BleedDotMulti"))
                        ),
                        lfmt!("= %.0f", nn(avgHitBleedDmg)),
                        lfmt!(""),
                        lfmt!("Crit Dmg Derivation:"),
                        lfmt!(
                            "%.0f + (%.0f - %.0f) * %.2f%%",
                            output.num(n!("BleedPhysicalMin")),
                            output.num(n!("BleedPhysicalMax")),
                            output.num(n!("BleedPhysicalMin")),
                            bleedRollAverage
                        ),
                        lfmt!("^8min source phys + (max source phys - min source phys) * average bleed roll"),
                        lfmt!(
                            "* %.2f ^8(Bleed DoT Multi for Crit)",
                            output.num(n!("CritBleedDotMulti"))
                        ),
                        lfmt!("= %.2f", nn(avgCritBleedDmg)),
                    ],
                );
            }
        }
        // L4568
        let bs = skillData.get(n!("bleedBasePercent"));
        let basePercent = if bs.truthy() {
            bs.num()
        } else {
            misc.BleedPercentBase
        };
        // -- over-stacking bleed stacks increases the chance a critical bleed is present
        // L4570
        let ailmentCritChance = 100.0
            * (1.0
                - m_pow(
                    1.0 - output.num(n!("CritChance")) / 100.0,
                    m_max(globalOutput.num(n!("BleedStackPotential")), 1.0),
                ));

        // L4572
        let baseMinVal = calcAilmentDamage(
            ctx,
            al,
            n!("Bleed"),
            ailmentCritChance,
            nn(sourceMinHitDmg),
            0.0,
            true,
        ) * basePercent
            / 100.0;
        // L4573
        let baseMaxVal = calcAilmentDamage(
            ctx,
            al,
            n!("Bleed"),
            100.0,
            nn(sourceMaxHitDmg),
            nn(sourceMaxCritDmg),
            true,
        ) * basePercent
            / 100.0
            * output.num(n!("RuthlessBlowAilmentEffect"))
            * output.num(n!("FistOfWarDamageEffect"))
            * globalOutput.num(n!("AilmentWarcryEffect"));
        // L4574
        let averageBaseBleedDps = calcAilmentDamage(
            ctx,
            al,
            n!("Bleed"),
            ailmentCritChance,
            nn(avgHitBleedDmg),
            nn(avgCritBleedDmg),
            false,
        );
        // L4575
        let baseBleedDps = averageBaseBleedDps * basePercent / 100.0
            * output.num(n!("RuthlessBlowAilmentEffect"))
            * output.num(n!("FistOfWarDamageEffect"))
            * globalOutput.num(n!("AilmentWarcryEffect"));
        // L4576
        if baseBleedDps > 0.0 {
            // L4577
            skillFlags.set(n!("bleed"), true);
            // L4578
            skillFlags.set(n!("duration"), true);
            // L4579
            let mut effMult = 1.0;
            // L4580
            if env.mode_effective.get() {
                // L4581
                let resist = m_min(
                    m_max(
                        0.0,
                        enemyDB.Sum(ModType::BASE, None, nm!["PhysicalDamageReduction"]),
                    ),
                    misc.EnemyPhysicalDamageReductionCap,
                );
                // L4582
                let takenInc = enemyDB.Sum(
                    ModType::INC,
                    d,
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "PhysicalDamageTaken",
                        "PhysicalDamageTakenOverTime"
                    ],
                );
                // L4583
                let takenMore = enemyDB.More(
                    d,
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "PhysicalDamageTaken",
                        "PhysicalDamageTakenOverTime"
                    ],
                );
                // L4584
                effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
                // L4585
                globalOutput.set(n!("BleedEffMult"), effMult);
                // L4586
                if let Some(b) = breakdown.as_ref().filter(|_| effMult != 1.0) {
                    // L4587
                    let t = b.effMult(
                        n!("Physical"),
                        resist,
                        0.0,
                        takenInc,
                        effMult,
                        takenMore,
                        None,
                        true,
                        None,
                        None,
                    );
                    gbOrRaise(&globalBreakdown).set(n!("BleedEffMult"), t);
                }
            }
            // L4590
            let effectMod = calcLib::r#mod(&skillModList, d, nm!["AilmentEffect"]);
            // L4591
            let activeBleeds = m_min(bleedStacks, maxStacks);
            // L4592
            output.set(
                n!("BaseBleedDPS"),
                baseBleedDps * effectMod * rateMod * activeBleeds * effMult,
            );
            // L4593
            let BleedDPSUncapped = output.num(n!("BaseBleedDPS"));
            // L4594
            let BleedDPSCapped = m_min(BleedDPSUncapped, misc.DotDpsCap);
            // L4595
            let MinBleedDPSUncapped = baseMinVal * effectMod * rateMod * activeBleeds * effMult;
            // L4596
            let MinBleedDPSCapped = m_min(MinBleedDPSUncapped, misc.DotDpsCap);
            // L4597
            let MaxBleedDPSUncapped = baseMaxVal * effectMod * rateMod * activeBleeds * effMult;
            // L4598
            let MaxBleedDPSCapped = m_min(MaxBleedDPSUncapped, misc.DotDpsCap);
            // L4599
            output.set(n!("BleedDPS"), BleedDPSCapped);
            // L4600
            globalOutput.set(n!("BleedStacks"), bleedStacks);
            // L4601
            globalOutput.set(
                n!("BleedDamage"),
                output.num(n!("BaseBleedDPS")) * globalOutput.num(n!("BleedDuration")),
            );
            // L4602
            if let Some(b) = &breakdown {
                // L4603
                if output.truthy(n!("CritBleedDotMulti"))
                    && (!output
                        .get(n!("CritBleedDotMulti"))
                        .raw_eq(&output.get(n!("BleedDotMulti"))))
                {
                    // L4604
                    let chanceFromHit = output.num(n!("BleedChanceOnHit")) / 100.0
                        * (1.0 - globalOutput.num(n!("CritChance")) / 100.0);
                    // L4605
                    let chanceFromCrit =
                        output.num(n!("BleedChanceOnCrit")) / 100.0 * ailmentCritChance / 100.0;
                    // L4606
                    let totalFromHit = chanceFromHit / (chanceFromHit + chanceFromCrit);
                    // L4607
                    let totalFromCrit = chanceFromCrit / (chanceFromHit + chanceFromCrit);
                    // L4608
                    let t = b.critDot(
                        output.num(n!("BleedDotMulti")),
                        output.num(n!("CritBleedDotMulti")),
                        totalFromHit,
                        totalFromCrit,
                    );
                    b.set(n!("BleedDotMulti"), t);
                    // L4609
                    let v = (output.num(n!("BleedDotMulti")) * totalFromHit)
                        + (output.num(n!("CritBleedDotMulti")) * totalFromCrit);
                    output.set(n!("BleedDotMulti"), v);
                }
                // L4611
                tins(
                    b,
                    n!("BleedDPS"),
                    lfmt!(
                        "x %.2f ^8(bleed deals %d%% per second)",
                        basePercent / 100.0,
                        basePercent
                    ),
                );
                // L4612
                if effectMod != 1.0 {
                    // L4613
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("x %.2f ^8(ailment effect modifier)", effectMod),
                    );
                }
                // L4615
                if output.num(n!("RuthlessBlowAilmentEffect")) != 1.0 {
                    // L4616
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "x %.2f ^8(ruthless blow effect modifier)",
                            output.num(n!("RuthlessBlowAilmentEffect"))
                        ),
                    );
                }
                // L4618
                if output.num(n!("FistOfWarDamageEffect")) != 1.0 {
                    // L4619
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "x %.2f ^8(fist of war effect modifier)",
                            output.num(n!("FistOfWarDamageEffect"))
                        ),
                    );
                }
                // L4621
                if globalOutput.num(n!("AilmentWarcryEffect")) > 1.0 {
                    // L4622
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "x %.2f ^8(combined ailment warcry effect modifier)",
                            globalOutput.num(n!("AilmentWarcryEffect"))
                        ),
                    );
                }
                // L4624
                tins(b, n!("BleedDPS"), lfmt!("= %.1f", baseBleedDps));
                // L4625
                tins(b, n!("BleedDPS"), "");
                // L4626
                tins(b, n!("BleedDPS"), "Total Bleed DPS:");
                // L4627
                if baseBleedDps != BleedDPSUncapped {
                    // L4628
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("%.1f ^8(base damage per second)", baseBleedDps),
                    );
                }
                // L4630
                if effectMod != 1.0 {
                    // L4631
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("x %.2f ^8(ailment effect modifier)", effectMod),
                    );
                }
                // L4633
                if rateMod != 1.0 {
                    // L4634
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("x %.2f ^8(damage rate modifier)", rateMod),
                    );
                }
                // L4636
                if activeBleeds != 1.0 {
                    // L4637
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("x %.2f ^8(avg bleed stacks)", activeBleeds),
                    );
                }
                // L4639
                if effMult != 1.0 {
                    // L4640
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "x %.3f ^8(effective DPS modifier from enemy debuffs)",
                            effMult
                        ),
                    );
                }
                // L4642
                if output.num(n!("BleedDPS")) != BleedDPSUncapped {
                    // L4643
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("= %.1f ^8(Uncapped raw Bleed DPS)", BleedDPSUncapped),
                    );
                    // L4644
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            &format!(
                                "^8(Raw Bleed DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                colorCode("NEGATIVE")
                            ),
                            BleedDPSUncapped - BleedDPSCapped,
                            (BleedDPSUncapped - BleedDPSCapped) / BleedDPSCapped * 100.0
                        ),
                    );
                    // L4645
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("= %d ^8(Capped Bleed DPS)", BleedDPSCapped),
                    );
                } else {
                    // L4647
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!("= %.1f ^8per second", output.num(n!("BleedDPS"))),
                    );
                }
                // L4649
                tins(
                    b,
                    n!("BleedDPS"),
                    lfmt!(
                        "%.2f%% of Maximum Bleed DPS",
                        output.num(n!("BleedDPS")) / MaxBleedDPSCapped * 100.0
                    ),
                );
                // L4650
                tins(b, n!("BleedDPS"), "");
                // L4651
                tins(b, n!("BleedDPS"), "DPS Range:");
                // L4652
                if MaxBleedDPSCapped == MaxBleedDPSUncapped
                    && MinBleedDPSCapped == MinBleedDPSUncapped
                {
                    // L4653
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "%.0f to %.0f ^8(Bleed DPS Range)",
                            MinBleedDPSUncapped,
                            MaxBleedDPSUncapped
                        ),
                    );
                } else {
                    // L4655
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "%.0f to %.0f ^8(Uncapped Bleed DPS Range)",
                            MinBleedDPSUncapped,
                            MaxBleedDPSUncapped
                        ),
                    );
                }
                // L4657
                if MinBleedDPSCapped != MinBleedDPSUncapped {
                    // L4658
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            &format!(
                                "^8(Raw Min Bleed DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                colorCode("NEGATIVE")
                            ),
                            MinBleedDPSUncapped - MinBleedDPSCapped,
                            (MinBleedDPSUncapped - MinBleedDPSCapped) / MinBleedDPSCapped * 100.0
                        ),
                    );
                }
                // L4660
                if MaxBleedDPSCapped != MaxBleedDPSUncapped {
                    // L4661
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            &format!(
                                "^8(Raw Max Bleed DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                colorCode("NEGATIVE")
                            ),
                            MaxBleedDPSUncapped - MaxBleedDPSCapped,
                            (MaxBleedDPSUncapped - MaxBleedDPSCapped) / MaxBleedDPSCapped * 100.0
                        ),
                    );
                }
                // L4663
                if MaxBleedDPSCapped != MaxBleedDPSUncapped
                    || MinBleedDPSCapped != MinBleedDPSUncapped
                {
                    // L4664
                    tins(
                        b,
                        n!("BleedDPS"),
                        lfmt!(
                            "%.0f to %.0f ^8(Capped Bleed DPS Range)",
                            MinBleedDPSCapped,
                            MaxBleedDPSCapped
                        ),
                    );
                }
                // L4666
                if globalOutput.num(n!("BleedDuration")) != durationBase {
                    let gb = gbOrRaise(&globalBreakdown);
                    // L4667
                    gb.set(
                        n!("BleedDuration"),
                        tbl![lfmt!("%.2fs ^8(base duration)", durationBase)],
                    );
                    // L4670
                    if durationMod != 1.0 {
                        // L4671
                        tins(
                            &gb,
                            n!("BleedDuration"),
                            lfmt!("x %.2f ^8(duration modifier)", durationMod),
                        );
                    }
                    // L4673
                    if rateMod != 1.0 {
                        // L4674
                        tins(
                            &gb,
                            n!("BleedDuration"),
                            lfmt!("/ %.2f ^8(damage rate modifier)", rateMod),
                        );
                    }
                    // L4676
                    if ctx.debuffDurationMult != 1.0 {
                        // L4677
                        tins(
                            &gb,
                            n!("BleedDuration"),
                            lfmt!(
                                "/ %.2f ^8(debuff expires slower/faster)",
                                1.0 / ctx.debuffDurationMult
                            ),
                        );
                    }
                    // L4679
                    tins(
                        &gb,
                        n!("BleedDuration"),
                        lfmt!("= %.2fs", globalOutput.num(n!("BleedDuration"))),
                    );
                }
            }
        }
    }

    // -- Calculate poison chance and damage
    // L4686
    if canDeal.truthy(n!("Chaos"))
        && (output.num(n!("PoisonChanceOnHit"))
            + output.num(n!("PoisonChanceOnCrit"))
            + output.num(n!("ChaosPoisonChance")))
            > 0.0
    {
        // L4687
        let newCfg = newDotCfg(
            &skillCfg,
            &cfg,
            KeywordFlag::Poison | KeywordFlag::Ailment | KeywordFlag::ChaosDot,
        );
        if !isOffHand {
            activeSkill.poisonCfg.set(Some(newCfg));
        } else {
            activeSkill.OHpoisonCfg.set(Some(newCfg));
        }
        // L4698
        let dotCfg: Cfg = if !isOffHand {
            activeSkill.poisonCfg.get()
        } else {
            activeSkill.OHpoisonCfg.get()
        }
        .unwrap_or_else(|| lua_error!("attempt to index local 'dotCfg' (a nil value)"));
        let d: Option<&ModCfg> = Some(&dotCfg);
        // L4699
        checkWeapon1HFlags(ctx, al, &dotCfg);
        // L4700
        let mut sourceHitDmg: Option<f64> = None;
        let mut sourceCritDmg: Option<f64> = None;
        let mut sourceMaxCritDmg: Option<f64> = None;
        let mut sourceMinHitDmg: Option<f64> = None;
        let mut sourceMaxHitDmg: Option<f64> = None;
        // L4701
        if let Some(b) = &breakdown {
            // L4702
            b.set(n!("PoisonPhysical"), tbl!(damageTypes = Table::new()));
            // L4703
            b.set(n!("PoisonLightning"), tbl!(damageTypes = Table::new()));
            // L4704
            b.set(n!("PoisonCold"), tbl!(damageTypes = Table::new()));
            // L4705
            b.set(n!("PoisonFire"), tbl!(damageTypes = Table::new()));
            // L4706
            b.set(n!("PoisonChaos"), tbl!(damageTypes = Table::new()));
        }
        // L4708
        let rateMod = calcLib::r#mod(&skillModList, c, nm!["PoisonFaster"])
            + enemyDB.Sum(ModType::INC, None, nm!["SelfPoisonFaster"]) / 100.0;
        // L4709
        let durationBase = match ov(skillModList.Override(d, nm!["PoisonDurationBase"])) {
            Some(v) => v,
            None => {
                let s = if skillData.truthy(n!("poisonDurationIsSkillDuration")) {
                    skillData.get(n!("duration"))
                } else {
                    Val::Nil
                };
                if s.truthy() {
                    s.num()
                } else {
                    misc.PoisonDurationBase
                }
            }
        };
        // L4710
        let mut durationMod = calcLib::r#mod(
            &skillModList,
            d,
            &names([
                Some(n!("EnemyPoisonDuration")),
                Some(n!("EnemyAilmentDuration")),
                Some(n!("DamagingAilmentDuration")),
                skillData
                    .truthy(n!("poisonIsSkillEffect"))
                    .then(|| n!("Duration")),
            ]),
        ) * calcLib::r#mod(
            &enemyDB,
            None,
            nm!["SelfPoisonDuration", "SelfAilmentDuration"],
        );
        // L4711
        durationMod = m_max(durationMod, 0.0);
        // L4712
        globalOutput.set(
            n!("PoisonDuration"),
            durationBase * durationMod / rateMod * ctx.debuffDurationMult,
        );
        // -- The chance any given hit applies poison
        // L4714
        let chaosPoisonChance = if output.num_or(n!("ChaosHitAverage"), 0.0) > 0.0 {
            output.num(n!("ChaosPoisonChance"))
        } else {
            0.0
        };
        // L4715
        let poisonChanceOnHit = m_min(
            100.0,
            output.num(n!("PoisonChanceOnHit")) + chaosPoisonChance,
        );
        // L4716
        let poisonChanceOnCrit = m_min(
            100.0,
            output.num(n!("PoisonChanceOnCrit")) + chaosPoisonChance,
        );
        // L4717
        let poisonChance = poisonChanceOnHit / 100.0 * (1.0 - output.num(n!("CritChance")) / 100.0)
            + poisonChanceOnCrit / 100.0 * output.num(n!("CritChance")) / 100.0;

        // -- Handling of "inflict x additional poisons"
        // L4720
        let mut additionalPoisonStacks = 1.0;
        // L4721
        if !skillModList.Flag(None, nm!["CannotMultiplePoison"]) {
            // L4722
            additionalPoisonStacks =
                1.0 + m_min(
                    skillModList.Sum(ModType::BASE, c, nm!["AdditionalPoisonChance"]) / 100.0,
                    1.0,
                ) + (skillModList.Sum(ModType::BASE, c, nm!["AdditionalPoisonStacks"]));
        }

        // -- Calculate average number of poisons that will be active on the enemy at once
        // L4726
        let poisonStackLimit = skillModList.Min(c, nm!["PoisonStackLimit"]);
        // L4727
        let mut PoisonStacks = output.num(n!("HitChance")) / 100.0
            * poisonChance
            * additionalPoisonStacks
            * skillData.num(n!("dpsMultiplier"))
            * skillData.num_or(n!("stackMultiplier"), 1.0)
            * ctx.quantityMultiplier;
        // L4728
        let mut uncappedPoisonStacks: Option<f64> = None;
        // L4729
        if hitSpeedOrSpeed(&globalOutput).num() > 0.0 {
            // --assume skills with no cast, attack, or cooldown time are single cast
            // L4731
            PoisonStacks = PoisonStacks
                * globalOutput.num(n!("PoisonDuration"))
                * hitSpeedOrSpeed(&globalOutput).num();

            // -- If stack limit exists, avg. poison stack is more complicated
            // L4734
            if let Some(limit) = poisonStackLimit.filter(|&l| l > 0.0 && PoisonStacks > l) {
                // -- Calc how many hits will poison before limit is reached and theoretical max poison stacks, which is different from `poisonStackLimit` due to "additional" poison mechanics
                // L4736
                let numPoisoningHits = m_ceil(limit / additionalPoisonStacks);
                // L4737
                let maxPoisonStacks = numPoisoningHits * additionalPoisonStacks;

                // -- Only use `maxPoisonStacks` if original value exceeds it
                // L4740
                uncappedPoisonStacks = Some(m_max(PoisonStacks, maxPoisonStacks));
                // L4741
                PoisonStacks = m_min(PoisonStacks, maxPoisonStacks);
            }
        }
        // L4744
        if PoisonStacks < additionalPoisonStacks
            && env.configInput.num_or(n!("multiplierPoisonOnEnemy"), 0.0) == 0.0
        {
            // L4745
            skillModList.NewMod(
                n!("Condition:NonPoisonedOnly"),
                ModType::FLAG,
                true,
                "Calculation",
                0,
                0,
                &[],
            );
        }
        // L4747
        if let Some(gb) = &globalBreakdown {
            // L4748
            gb.set(n!("PoisonStacks"), Table::new());
            // L4749
            let chain = tbl![
                tbl!["%.2f ^8(poison chance)", poisonChance],
                tbl!["%.2f ^8(hit chance)", output.num(n!("HitChance")) / 100.0],
                tbl!["%.2f ^8(avg. # of poisons inflicted)", additionalPoisonStacks],
                tbl!["%.2f ^8(hits per second)", hitSpeedOrSpeed(&globalOutput)],
                tbl![
                    "%g ^8(dps multiplier for this skill)",
                    lor(skillData.get(n!("dpsMultiplier")), || Val::Num(1.0))
                ],
                tbl![
                    "%g ^8(stack multiplier for this skill)",
                    lor(skillData.get(n!("stackMultiplier")), || Val::Num(1.0))
                ],
                tbl!["%g ^8(quantity multiplier for this skill)", ctx.quantityMultiplier];
                base = tbl![
                    "%.2fs ^8(poison duration)",
                    globalOutput.get(n!("PoisonDuration"))
                ],
                total = lfmt!("= %.2f", PoisonStacks)
            ];
            let out = gb.table(n!("PoisonStacks")).unwrap_or_else(|| {
                lua_error!("attempt to index field 'PoisonStacks' (a nil value)")
            });
            gb.multiChain(&out, &chain);
            // L4760
            if skillModList.Flag(None, nm!["Condition:NonPoisonedOnly"]) {
                // L4761
                tins(gb, n!("PoisonStacks"), "Assuming 'non-Poisoned' Enemy");
                // L4762
                if PoisonStacks < additionalPoisonStacks {
                    // L4763
                    tins(
                        gb,
                        n!("PoisonStacks"),
                        "^8(time between hits is longer than poison duration)",
                    );
                } else {
                    // L4765
                    tins(
                        gb,
                        n!("PoisonStacks"),
                        format!(
                            "^8(affected by poison stack limit of: {})",
                            tostring_num(additionalPoisonStacks)
                        ),
                    );
                }
            } else if let Some(limit) = poisonStackLimit.filter(|&l| PoisonStacks >= l) {
                // L4768
                tins(
                    gb,
                    n!("PoisonStacks"),
                    format!(
                        "^8(affected by poison stack limit of: {})",
                        tostring_num(limit)
                    ),
                );
                // L4769
                if let Some(u) = uncappedPoisonStacks {
                    // L4770
                    tins(
                        gb,
                        n!("PoisonStacks"),
                        format!("^8(uncapped poison stacks: {})", lfmt!("%.2f", u)),
                    );
                }
            }
        }
        // L4774
        for sub_pass in 1..=2 {
            // L4775
            skillCond(&dotCfg).set(n!("CriticalStrike"), sub_pass != 1);

            // L4777
            let mut totalMin = 0.0;
            let mut totalMax = 0.0;
            // L4778 do
            {
                // L4779
                let bdArg = if sub_pass == 1 {
                    breakdown.as_ref().and_then(|b| b.table(n!("PoisonChaos")))
                } else {
                    None
                };
                let (min, max) = calcAilmentSourceDamage(
                    &activeSkill,
                    &output,
                    &dotCfg,
                    bdArg.as_ref(),
                    n!("Chaos"),
                    0,
                );
                // L4780
                output.set(n!("PoisonChaosMin"), min);
                // L4781
                output.set(n!("PoisonChaosMax"), max);
                // L4782
                totalMin = totalMin + min;
                // L4783
                totalMax = totalMax + max;
            }
            // L4785
            let mut nonChaosMult = 1.0;
            // L4786
            if output.num(n!("ChaosPoisonChance")) > 0.0 && output.num(n!("PoisonChaosMax")) > 0.0 {
                // -- Additional chance for chaos
                // L4788
                let chance = if sub_pass == 2 {
                    n!("PoisonChanceOnCrit")
                } else {
                    n!("PoisonChanceOnHit")
                };
                // L4789
                let chaosChance = m_min(
                    100.0,
                    output.num(chance) + output.num(n!("ChaosPoisonChance")),
                );
                // L4790
                nonChaosMult = output.num(chance) / chaosChance;
                // L4791
                output.set(chance, chaosChance);
            }
            // L4793, L4800, L4807, L4814
            for (ty, can, key) in [
                (
                    n!("Lightning"),
                    Some(n!("LightningCanPoison")),
                    n!("PoisonLightning"),
                ),
                (n!("Cold"), Some(n!("ColdCanPoison")), n!("PoisonCold")),
                (n!("Fire"), Some(n!("FireCanPoison")), n!("PoisonFire")),
                (n!("Physical"), None, n!("PoisonPhysical")),
            ] {
                let allowed = canDeal.truthy(ty)
                    && match can {
                        Some(f) => skillModList.Flag(c, &[f]),
                        None => true,
                    };
                if allowed {
                    // L4794 / L4801 / L4808 / L4815
                    let bdArg = if sub_pass == 1 {
                        breakdown.as_ref().and_then(|b| b.table(key))
                    } else {
                        None
                    };
                    let (min, max) = calcAilmentSourceDamage(
                        &activeSkill,
                        &output,
                        &dotCfg,
                        bdArg.as_ref(),
                        ty,
                        dmgTypeFlags_Chaos,
                    );
                    // L4795-4796 (output.Poison<Type>Min / Max)
                    output.set(cat!(key, "Min"), min);
                    output.set(cat!(key, "Max"), max);
                    // L4797
                    totalMin = totalMin + min * nonChaosMult;
                    // L4798
                    totalMax = totalMax + max * nonChaosMult;
                }
            }
            // L4821
            if sub_pass == 2 {
                // L4822
                if modDB.Flag(None, nm!["AilmentsAreNeverFromCrit"]) {
                    // L4823 -- force config to non-crit for dotMulti calculation
                    skillCond(&dotCfg).set(n!("CriticalStrike"), false);
                    // L4824
                    let v = dotMultiOf(&skillModList, &dotCfg, n!("ChaosDotMultiplier"));
                    output.set(n!("CritPoisonDotMulti"), v);
                    // L4825 -- reset to true to avoid unintended side effects
                    skillCond(&dotCfg).set(n!("CriticalStrike"), true);
                } else {
                    // L4827
                    let v = dotMultiOf(&skillModList, &dotCfg, n!("ChaosDotMultiplier"));
                    output.set(n!("CritPoisonDotMulti"), v);
                }
                // L4829
                sourceCritDmg =
                    Some((totalMin + totalMax) / 2.0 * output.num(n!("CritPoisonDotMulti")));
                // L4830
                sourceMaxCritDmg = Some(totalMax * output.num(n!("CritPoisonDotMulti")));
            } else {
                // L4832
                let v = dotMultiOf(&skillModList, &dotCfg, n!("ChaosDotMultiplier"));
                output.set(n!("PoisonDotMulti"), v);
                // L4833
                sourceHitDmg = Some((totalMin + totalMax) / 2.0 * output.num(n!("PoisonDotMulti")));
                // L4834
                sourceMinHitDmg = Some(totalMin * output.num(n!("PoisonDotMulti")));
                // L4835
                sourceMaxHitDmg = Some(totalMax * output.num(n!("PoisonDotMulti")));
            }
        }
        // L4838
        if let Some(gb) = &globalBreakdown {
            // L4839
            gb.set(
                n!("PoisonDPS"),
                tbl![lfmt!(
                    "Ailment mode: %s ^8(can be changed in the Configuration tab)",
                    if ailmentMode.as_str() == "CRIT" {
                        "Crits Only"
                    } else {
                        "Average Damage"
                    }
                )],
            );
        }
        // L4843
        let baseMinVal = calcAilmentDamage(
            ctx,
            al,
            n!("Poison"),
            output.num(n!("CritChance")),
            nn(sourceMinHitDmg),
            0.0,
            true,
        ) * misc.PoisonPercentBase;
        // L4844
        let baseMaxVal = calcAilmentDamage(
            ctx,
            al,
            n!("Poison"),
            100.0,
            nn(sourceMaxHitDmg),
            nn(sourceMaxCritDmg),
            true,
        ) * misc.PoisonPercentBase
            * output.num(n!("RuthlessBlowAilmentEffect"))
            * output.num(n!("FistOfWarDamageEffect"))
            * globalOutput.num(n!("AilmentWarcryEffect"));
        // L4845
        let baseVal = calcAilmentDamage(
            ctx,
            al,
            n!("Poison"),
            output.num(n!("CritChance")),
            nn(sourceHitDmg),
            nn(sourceCritDmg),
            false,
        ) * misc.PoisonPercentBase
            * output.num(n!("RuthlessBlowAilmentEffect"))
            * output.num(n!("FistOfWarDamageEffect"))
            * globalOutput.num(n!("AilmentWarcryEffect"));
        // L4846
        if baseVal > 0.0 {
            // L4847
            skillFlags.set(n!("poison"), true);
            // L4848
            skillFlags.set(n!("duration"), true);
            // L4849
            let mut effMult = 1.0;
            // L4850
            if env.mode_effective.get() {
                // L4851
                let resist = calcResistForType(ctx, n!("Chaos"), d);
                // L4852
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
                // L4853
                let takenMore = enemyDB.More(
                    d,
                    nm![
                        "DamageTaken",
                        "DamageTakenOverTime",
                        "ChaosDamageTaken",
                        "ChaosDamageTakenOverTime"
                    ],
                );
                // L4854
                effMult = (1.0 - resist / 100.0) * (1.0 + takenInc / 100.0) * takenMore;
                // L4855
                globalOutput.set(n!("PoisonEffMult"), effMult);
                // L4856
                if let Some(b) = breakdown.as_ref().filter(|_| effMult != 1.0) {
                    // L4857
                    let sourceRes = if env
                        .modDB
                        .get()
                        .Flag(None, nm!["EnemyChaosResistEqualToYours"])
                    {
                        n!("Your Chaos Resistance")
                    } else if env
                        .partyMembers
                        .get()
                        .unwrap_or_else(|| {
                            lua_error!("attempt to index field 'partyMembers' (a nil value)")
                        })
                        .modDB
                        .get()
                        .Flag(None, nm!["EnemyChaosResistEqualToYours"])
                    {
                        n!("Party Member Chaos Resistance")
                    } else {
                        n!("Chaos")
                    };
                    // L4858
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
                    gbOrRaise(&globalBreakdown).set(n!("PoisonEffMult"), t);
                }
            }
            // L4861
            if skillModList.Flag(None, nm!["Condition:NonPoisonedOnly"]) {
                // L4862
                PoisonStacks = m_min(additionalPoisonStacks, PoisonStacks);
            }
            // L4864
            globalOutput.set(n!("PoisonStacks"), PoisonStacks);
            // L4865
            let effectMod = calcLib::r#mod(&skillModList, d, nm!["AilmentEffect"]);
            // L4866
            let singlePoisonDPSUncapped =
                m_min(baseVal * effectMod * rateMod * effMult, misc.DotDpsCap);
            // L4867
            let singlePoisonDPSCapped = m_min(singlePoisonDPSUncapped, misc.DotDpsCap);
            // L4868
            output.set(n!("PoisonDPS"), singlePoisonDPSCapped);
            // L4869
            output.set(
                n!("PoisonDamage"),
                singlePoisonDPSCapped * globalOutput.num(n!("PoisonDuration")),
            );
            // L4870
            let MinPoisonDPSUncapped = baseMinVal * effectMod * rateMod * effMult;
            // L4871
            let MinPoisonDPSCapped = m_min(MinPoisonDPSUncapped, misc.DotDpsCap);
            // L4872
            let MaxPoisonDPSUncapped = baseMaxVal * effectMod * rateMod * effMult;
            // L4873
            let MaxPoisonDPSCapped = m_min(MaxPoisonDPSUncapped, misc.DotDpsCap);
            // L4874
            let groundMult = m_max(
                skillModList
                    .Max(None, nm!["PoisonDpsAsCausticGround"])
                    .unwrap_or(0.0),
                enemyDB
                    .Max(None, nm!["PoisonDpsAsCausticGround"])
                    .unwrap_or(0.0),
            );
            // L4875
            if groundMult > 0.0 {
                // L4876
                let CausticGroundDPSUncapped =
                    baseVal * effectMod * rateMod * effMult * groundMult / 100.0;
                // L4877
                let CausticGroundDPSCapped = m_min(CausticGroundDPSUncapped, misc.DotDpsCap);
                // L4878
                output.set(n!("CausticGroundDPS"), CausticGroundDPSCapped);
                // L4879
                globalOutput.set(n!("CausticGroundFromPoison"), true);
                // L4880
                if let Some(b) = &breakdown {
                    // L4881
                    b.set(
                        n!("CausticGroundDPS"),
                        tbl![
                            lfmt!(
                                "%.1f ^8(single poison damage per second)",
                                baseVal * effectMod * rateMod
                            ),
                            lfmt!("* %.1f%% ^8(percent as Caustic ground)", groundMult),
                            lfmt!("* %.3f ^8(effect mult)", effMult),
                            lfmt!("= %.1f ^8per second", output.num(n!("CausticGroundDPS"))),
                        ],
                    );
                }
            }
            // L4889
            let mut PoisonDPSUncapped: Option<f64> = None;
            // L4890
            let mut PoisonDPSCapped: Option<f64> = None;
            // L4891
            if skillData.truthy(n!("showAverage")) {
                // L4892
                let v = output.get(n!("PoisonDamage"));
                output.set(n!("TotalPoisonAverageDamage"), v);
                // L4893
                let v = output.get(n!("PoisonDPS"));
                output.set(n!("TotalPoisonDPS"), v);
            } else {
                // L4895
                PoisonDPSUncapped = Some(singlePoisonDPSCapped * PoisonStacks);
                // L4896
                PoisonDPSCapped = Some(m_min(nn(PoisonDPSUncapped), misc.DotDpsCap));
                // L4897
                output.set(n!("TotalPoisonDPS"), nn(PoisonDPSCapped));
            }
            // L4899
            if let Some(b) = &breakdown {
                // L4900
                if output.truthy(n!("CritPoisonDotMulti"))
                    && (!output
                        .get(n!("CritPoisonDotMulti"))
                        .raw_eq(&output.get(n!("PoisonDotMulti"))))
                {
                    // L4901
                    let chanceFromHit = output.num(n!("PoisonChanceOnHit")) / 100.0
                        * (1.0 - globalOutput.num(n!("CritChance")) / 100.0);
                    // L4902
                    let chanceFromCrit = output.num(n!("PoisonChanceOnCrit")) / 100.0
                        * globalOutput.num(n!("CritChance"))
                        / 100.0;
                    // L4903
                    let totalFromHit = chanceFromHit / (chanceFromHit + chanceFromCrit);
                    // L4904
                    let totalFromCrit = chanceFromCrit / (chanceFromHit + chanceFromCrit);
                    // L4905
                    let t = b.critDot(
                        output.num(n!("PoisonDotMulti")),
                        output.num(n!("CritPoisonDotMulti")),
                        totalFromHit,
                        totalFromCrit,
                    );
                    b.set(n!("PoisonDotMulti"), t);
                    // L4906
                    let v = (output.num(n!("PoisonDotMulti")) * totalFromHit)
                        + (output.num(n!("CritPoisonDotMulti")) * totalFromCrit);
                    output.set(n!("PoisonDotMulti"), v);
                }
                // L4908
                if output.num(n!("RuthlessBlowAilmentEffect")) != 1.0 {
                    // L4909
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            "x %.2f ^8(ruthless blow effect modifier)",
                            output.num(n!("RuthlessBlowAilmentEffect"))
                        ),
                    );
                }
                // L4911
                tins(b, n!("PoisonDPS"), "x 0.30 ^8(poison deals 30% per second)");
                // L4912 (the extra argument is ignored by string.format)
                tins(b, n!("PoisonDPS"), lfmt!("= %.1f", baseVal, 1.0));
                // L4913
                tins(b, n!("PoisonDPS"), "");
                // L4914
                tins(b, n!("PoisonDPS"), "Poison DPS:");
                // L4915
                if baseVal != singlePoisonDPSUncapped {
                    // L4916
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!("%.1f ^8(base damage per second)", baseVal),
                    );
                }
                // L4918
                if effectMod != 1.0 {
                    // L4919
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!("x %.2f ^8(ailment effect modifier)", effectMod),
                    );
                }
                // L4921
                if rateMod != 1.0 {
                    // L4922
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!("x %.2f ^8(damage rate modifier)", rateMod),
                    );
                }
                // L4924
                if effMult != 1.0 {
                    // L4925
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            "x %.3f ^8(effective DPS modifier from enemy debuffs)",
                            effMult
                        ),
                    );
                }
                // L4927
                if output.num(n!("PoisonDPS")) != singlePoisonDPSUncapped {
                    // L4928
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            "= %.1f ^8(Uncapped raw Poison DPS)",
                            singlePoisonDPSUncapped
                        ),
                    );
                    // L4929
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            &format!(
                                "^8(Raw Poison DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                colorCode("NEGATIVE")
                            ),
                            singlePoisonDPSUncapped - singlePoisonDPSCapped,
                            (singlePoisonDPSUncapped - singlePoisonDPSCapped)
                                / singlePoisonDPSCapped
                                * 100.0
                        ),
                    );
                    // L4930
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!("= %d ^8(Capped Poison DPS)", singlePoisonDPSCapped),
                    );
                } else {
                    // L4932
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!("= %.1f ^8per second", output.num(n!("PoisonDPS"))),
                    );
                }
                // L4934
                tins(
                    b,
                    n!("PoisonDPS"),
                    lfmt!(
                        "%.2f%% of Maximum Poison DPS",
                        output.num(n!("PoisonDPS")) / MaxPoisonDPSCapped * 100.0
                    ),
                );
                // L4935
                tins(b, n!("PoisonDPS"), "");
                // L4936
                tins(b, n!("PoisonDPS"), "DPS Range:");
                // L4937
                if MaxPoisonDPSCapped == MaxPoisonDPSUncapped
                    && MinPoisonDPSCapped == MinPoisonDPSUncapped
                {
                    // L4938
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            "%.0f to %.0f ^8(Poison DPS Range)",
                            MinPoisonDPSUncapped,
                            MaxPoisonDPSUncapped
                        ),
                    );
                } else {
                    // L4940
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            "%.0f to %.0f ^8(Uncapped Poison DPS Range)",
                            MinPoisonDPSUncapped,
                            MaxPoisonDPSUncapped
                        ),
                    );
                }
                // L4942
                if MinPoisonDPSCapped != MinPoisonDPSUncapped {
                    // L4943
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            &format!(
                                "^8(Raw Min Poison DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                colorCode("NEGATIVE")
                            ),
                            MinPoisonDPSUncapped - MinPoisonDPSCapped,
                            (MinPoisonDPSUncapped - MinPoisonDPSCapped) / MinPoisonDPSCapped
                                * 100.0
                        ),
                    );
                }
                // L4945
                if MaxPoisonDPSCapped != MaxPoisonDPSUncapped {
                    // L4946
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            &format!(
                                "^8(Raw Max Poison DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                colorCode("NEGATIVE")
                            ),
                            MaxPoisonDPSUncapped - MaxPoisonDPSCapped,
                            (MaxPoisonDPSUncapped - MaxPoisonDPSCapped) / MaxPoisonDPSCapped
                                * 100.0
                        ),
                    );
                }
                // L4948
                if MaxPoisonDPSCapped != MaxPoisonDPSUncapped
                    || MinPoisonDPSCapped != MinPoisonDPSUncapped
                {
                    // L4949
                    tins(
                        b,
                        n!("PoisonDPS"),
                        lfmt!(
                            "%.0f to %.0f ^8(Capped Poison DPS Range)",
                            MinPoisonDPSCapped,
                            MaxPoisonDPSCapped
                        ),
                    );
                }
                // L4951
                if !skillData.truthy(n!("showAverage")) && PoisonStacks != 1.0 {
                    // L4952
                    b.set(
                        n!("TotalPoisonDPS"),
                        tbl![
                            lfmt!(
                                "%.1f ^8single poison damage per second",
                                singlePoisonDPSCapped
                            ),
                            lfmt!("x %.2f ^8(poison stacks)", PoisonStacks),
                        ],
                    );
                    // L4956
                    if Some(output.num(n!("TotalPoisonDPS"))) != PoisonDPSUncapped {
                        // L4957
                        tins(
                            b,
                            n!("TotalPoisonDPS"),
                            lfmt!("= %.1f ^8(Uncapped raw Poison DPS)", nn(PoisonDPSUncapped)),
                        );
                        // L4958
                        tins(
                            b,
                            n!("TotalPoisonDPS"),
                            lfmt!(
                                &format!(
                                    "^8(Raw Poison DPS is {}overcapped ^8by^7 %.0f ^8:^7 %.1f%%^8)",
                                    colorCode("NEGATIVE")
                                ),
                                nn(PoisonDPSUncapped) - nn(PoisonDPSCapped),
                                (nn(PoisonDPSUncapped) - nn(PoisonDPSCapped)) / nn(PoisonDPSCapped)
                                    * 100.0
                            ),
                        );
                        // L4959
                        tins(
                            b,
                            n!("TotalPoisonDPS"),
                            lfmt!("= %d ^8(Capped Poison DPS)", nn(PoisonDPSCapped)),
                        );
                    } else {
                        // L4961
                        tins(
                            b,
                            n!("TotalPoisonDPS"),
                            lfmt!("= %.1f ^8per second", nn(PoisonDPSCapped)),
                        );
                    }
                }
                // L4964
                if globalOutput.num(n!("PoisonDuration")) != 2.0 {
                    let gb = gbOrRaise(&globalBreakdown);
                    // L4965
                    gb.set(
                        n!("PoisonDuration"),
                        tbl![lfmt!("%.2fs ^8(base duration)", durationBase)],
                    );
                    // L4968
                    if durationMod != 1.0 {
                        // L4969
                        tins(
                            &gb,
                            n!("PoisonDuration"),
                            lfmt!("x %.2f ^8(duration modifier)", durationMod),
                        );
                    }
                    // L4971
                    if rateMod != 1.0 {
                        // L4972
                        tins(
                            &gb,
                            n!("PoisonDuration"),
                            lfmt!("/ %.2f ^8(damage rate modifier)", rateMod),
                        );
                    }
                    // L4974
                    if ctx.debuffDurationMult != 1.0 {
                        // L4975
                        tins(
                            &gb,
                            n!("PoisonDuration"),
                            lfmt!(
                                "/ %.2f ^8(debuff expires slower/faster)",
                                1.0 / ctx.debuffDurationMult
                            ),
                        );
                    }
                    // L4977
                    tins(
                        &gb,
                        n!("PoisonDuration"),
                        lfmt!("= %.2fs", globalOutput.num(n!("PoisonDuration"))),
                    );
                }
                // L4979
                b.set(n!("PoisonDamage"), Table::new());
                // L4980
                if ctx.isAttack {
                    // L4981
                    tins(b, n!("PoisonDamage"), format!("{}:", passLabel(&pass)));
                }
                // L4983
                tins(
                    b,
                    n!("PoisonDamage"),
                    lfmt!("%.1f ^8(damage per second)", singlePoisonDPSCapped),
                );
                // L4984
                tins(
                    b,
                    n!("PoisonDamage"),
                    lfmt!(
                        "x %.2fs ^8(poison duration)",
                        globalOutput.num(n!("PoisonDuration"))
                    ),
                );
                // L4985
                tins(
                    b,
                    n!("PoisonDamage"),
                    lfmt!(
                        "= %.1f ^8damage per poison stack",
                        output.num(n!("PoisonDamage"))
                    ),
                );
            }
        }
    }
    Flow::Normal
}

/// `globalBreakdown.X = …` where Lua indexes `globalBreakdown` unguarded (nil raises).
fn gbOrRaise(globalBreakdown: &Option<Breakdown>) -> Breakdown {
    globalBreakdown
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index upvalue 'globalBreakdown' (a nil value)"))
}

// Loop-local functions declared in OF-G (provenance: the OF-G segment marker above).

/// `checkWeapon1HFlags(targetCfg)` (CalcOffence.lua:4308-4310): `targetCfg.flags =
/// bor(targetCfg.flags, band(cfg.flags, ModFlag.Weapon1H))`, `cfg` being the pass cfg.
pub(crate) fn checkWeapon1HFlags(ctx: &OffenceCtx, al: &AilmentPassLocals, targetCfg: &ModCfg) {
    let _ = ctx;
    // L4309
    targetCfg
        .flags
        .set(targetCfg.flags.get() | (al.cfg.flags.get() & ModFlag::Weapon1H));
}

/// `calcAverageSourceDamage(ailment)` (CalcOffence.lua:4315-4331).
/// Calculates normal and crit damage to be used in non-damaging ailment calculations
/// → (average hit damage, average crit damage).
#[allow(dead_code)] // called by OF-H (ailments_b.rs)
pub(crate) fn calcAverageSourceDamage(
    ctx: &OffenceCtx,
    al: &AilmentPassLocals,
    ailment: Name,
) -> (f64, f64) {
    let output = &al.output;
    let c: Option<&ModCfg> = Some(&al.cfg);
    // L4316
    let mut sourceHitDmg = 0.0;
    let mut sourceCritDmg = 0.0;
    // L4317
    for ty in dmgTypeList {
        let r#type = Name::from(ty);
        // L4318
        let can = ctx.canDeal.truthy(r#type) && {
            let ad = ctx.ailmentData.unwrap_or_else(|| {
                lua_error!("attempt to index upvalue 'ailmentData' (a nil value)")
            });
            let a = ad
                .get(&ailment)
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
            // L4319
            if r#type == a.associatedType {
                // L4320
                !ctx.skillModList.Flag(c, &[cat!(r#type, "Cannot", ailment)])
            } else {
                // L4322
                ctx.skillModList.Flag(c, &[cat!(r#type, "Can", ailment)])
            }
        };
        if can {
            // L4325
            sourceHitDmg = sourceHitDmg + output.num(cat!(r#type, "HitAverage"));
            // L4326
            sourceCritDmg = sourceCritDmg + output.num(cat!(r#type, "CritAverage"));
        }
    }
    // L4329
    (sourceHitDmg, sourceCritDmg)
}

/// `calcAilmentDamage(type, sourceCritChance, sourceHitDmg, sourceCritDmg, hideFromBreakdown)`
/// (CalcOffence.lua:4333-4400). Calculate the inflict chance and base damage of a secondary
/// effect (bleed/poison/ignite/shock/freeze); returns `baseVal`.
pub(crate) fn calcAilmentDamage(
    ctx: &OffenceCtx,
    al: &AilmentPassLocals,
    r#type: Name,
    sourceCritChance: f64,
    sourceHitDmg: f64,
    sourceCritDmg: f64,
    hideFromBreakdown: bool,
) -> f64 {
    let output = &al.output;
    let breakdown = &al.breakdown;
    // L4334
    let chanceOnHit = output.num(cat!(r#type, "ChanceOnHit"));
    let chanceOnCrit = output.num(cat!(r#type, "ChanceOnCrit"));
    // -- Use sourceCritChance to factor in chance a critical ailment is present
    // L4336
    let chanceFromHit = chanceOnHit * (1.0 - sourceCritChance / 100.0);
    // L4337
    let chanceFromCrit = chanceOnCrit * sourceCritChance / 100.0;
    // L4338
    let chance = chanceFromHit + chanceFromCrit;
    // L4339
    output.set(cat!(r#type, "Chance"), chance);
    // L4340
    let baseFromHit = sourceHitDmg * chanceFromHit / (chanceFromHit + chanceFromCrit);
    // L4341
    let baseFromCrit = sourceCritDmg * chanceFromCrit / (chanceFromHit + chanceFromCrit);
    // L4342
    let baseVal = baseFromHit + baseFromCrit;
    // L4343
    let sourceMult = ctx
        .skillModList
        .More(Some(&al.cfg), &[cat!(r#type, "AsThoughDealing")]);
    // L4344
    if let Some(b) = breakdown
        .as_ref()
        .filter(|_| chance != 0.0 && !hideFromBreakdown)
    {
        // L4345
        let key = cat!(r#type, "Chance");
        let breakdownChance = b.table(key).unwrap_or_else(Table::new);
        // L4346
        b.set(key, breakdownChance.clone());
        // L4347
        if breakdownChance.get(1_i64).truthy() {
            // L4348
            breakdownChance.push("");
        }
        // L4350
        if ctx.isAttack {
            // L4351
            breakdownChance.push(format!("{}:", passLabel(&al.pass)));
        }
        // L4353
        breakdownChance.push(lfmt!("Chance on Non-crit: %d%%", chanceOnHit));
        // L4354
        breakdownChance.push(lfmt!("Chance on Crit: %d%%", chanceOnCrit));
        // L4355
        if chanceOnHit != chanceOnCrit {
            // L4356
            breakdownChance.push("Combined chance:");
            // L4357
            breakdownChance.push(lfmt!(
                "%d x (1 - %.4f) ^8(chance from non-crits)",
                chanceOnHit,
                sourceCritChance / 100.0
            ));
            // L4358
            breakdownChance.push(lfmt!(
                "+ %d x %.4f ^8(chance from crits)",
                chanceOnCrit,
                sourceCritChance / 100.0
            ));
            // L4359
            let chancePerHit = chanceOnHit * (1.0 - sourceCritChance / 100.0)
                + chanceOnCrit * sourceCritChance / 100.0;
            // L4360
            breakdownChance.push(lfmt!("= %.2f", chancePerHit));
        }
    }
    // L4363
    if let Some(b) = breakdown
        .as_ref()
        .filter(|_| baseVal > 0.0 && !hideFromBreakdown)
    {
        // L4364
        let key = cat!(r#type, "DPS");
        let breakdownDPS = b.table(key).unwrap_or_else(Table::new);
        // L4365
        b.set(key, breakdownDPS.clone());
        // L4366
        if breakdownDPS.get(1_i64).truthy() {
            // L4367
            breakdownDPS.push("");
        }
        // L4369
        if ctx.isAttack {
            // L4370
            breakdownDPS.push(format!("{}:", passLabel(&al.pass)));
        }
        // L4372
        if sourceHitDmg == sourceCritDmg || output.num(n!("CritChance")) == 0.0 {
            // L4373
            breakdownDPS.push("Total damage:");
            // L4374
            breakdownDPS.push(lfmt!("%.1f ^8(source damage)", sourceHitDmg));
            // L4375
            if sourceMult > 1.0 {
                // L4376
                breakdownDPS.push(lfmt!(
                    "x %.2f ^8(inflicting as though dealing more damage)",
                    sourceMult
                ));
                // L4377
                breakdownDPS.push(lfmt!("= %.1f", baseVal * sourceMult));
            }
        } else {
            // L4380
            if baseFromHit > 0.0 {
                // L4381
                breakdownDPS.push("Damage from Non-crits:");
                // L4382
                breakdownDPS.push(lfmt!("%.1f ^8(source damage from non-crits)", sourceHitDmg));
                // L4383
                breakdownDPS.push(lfmt!(
                    "x %.3f ^8(portion of instances created by non-crits)",
                    chanceFromHit / (chanceFromHit + chanceFromCrit)
                ));
                // L4384
                if sourceMult == 1.0 || baseFromCrit != 0.0 {
                    // L4385
                    breakdownDPS.push(lfmt!("= %.1f", baseFromHit));
                }
            }
            // L4388
            if baseFromCrit > 0.0 {
                // L4389
                breakdownDPS.push("Damage from Crits:");
                // L4390
                breakdownDPS.push(lfmt!("%.1f ^8(source damage from crits)", sourceCritDmg));
                // L4391
                breakdownDPS.push(lfmt!(
                    "x %.3f ^8(portion of instances created by crits)",
                    chanceFromCrit / (chanceFromHit + chanceFromCrit)
                ));
                // L4392
                if sourceMult == 1.0 || baseFromHit != 0.0 {
                    // L4393
                    breakdownDPS.push(lfmt!("= %.1f", baseFromCrit));
                }
            }
            // L4396
            if baseFromHit > 0.0 && baseFromCrit > 0.0 {
                // L4397
                breakdownDPS.push("Total damage:");
                // L4398
                breakdownDPS.push(lfmt!("%.1f + %.1f", baseFromHit, baseFromCrit));
                // L4399
                if sourceMult == 1.0 {
                    // L4400
                    breakdownDPS.push(lfmt!("= %.1f", baseVal));
                }
            }
            // L4403
            if sourceMult > 1.0 {
                // L4404
                breakdownDPS.push(lfmt!(
                    "x %.2f ^8(inflicting as though dealing more damage)",
                    sourceMult
                ));
                // L4405
                breakdownDPS.push(lfmt!("= %.1f", baseVal * sourceMult));
            }
        }
    }
    // L4409
    baseVal
}
