//! @pob-file Modules/Calcs.lua @16de4b82d
//! Modules/Calcs.lua 1-872: buildOutput, calcFullDPS, getMiscCalculator, buildActiveSkill.
//! `infoDump` (38-88) is excluded (debug printing).
//!
//! Owner: CA. Signatures are U0's frozen contract (calc doc §5.10).
#![allow(non_snake_case)]

use crate::modules::calc_perform::perform;
use crate::modules::calc_setup::initEnv;
use crate::modules::common::cacheSkillUUID;
use crate::prelude::*;

pub use crate::late::MiscCalculator;

/// `calcFunc(override, useFullDPS)` (Calcs.lua:100-113; marker on [`getMiscCalculator`]): the
/// body of the closure [`MiscCalculator::call`] calls through the late seam (CCR-0131). Rust-only
/// name: the Lua closure has none.
pub fn misc_calculator_call(
    this: &MiscCalculator,
    r#override: Rc<Override>,
    useFullDPS: Option<bool>,
) -> Output {
    let build = &this.build;
    let usedFullDPS = this.usedFullDPS.get();
    // L101
    let (env, cachedPlayerDB, cachedEnemyDB, cachedMinionDB) =
        initEnv(build, Mode::CALCULATOR, Some(r#override.clone()), None);
    // L102
    perform(&env, false);
    // L103
    if (useFullDPS != Some(false) || build.viewMode.get() == n!("TREE")) && usedFullDPS {
        // prevent upcoming calculation from using Cached Data and thus forcing it to re-calculate new FullDPS roll-up
        // without this, FullDPS increase/decrease when for node/item/gem comparison would be all 0 as it would be comparing
        // A with A (due to cache reuse) instead of A with B
        // L107
        let fullDPS = calcFullDPS(
            build,
            Mode::CALCULATOR,
            r#override,
            Some(&SpecEnv {
                cachedPlayerDB,
                cachedEnemyDB,
                cachedMinionDB,
                env: None,
                accelerate: Accelerate::default(),
            }),
        );
        let output = env.player.output.get();
        // L108
        output.set(n!("SkillDPS"), fullDPS.skills.clone());
        // L109
        output.set(n!("FullDPS"), fullDPS.combinedDPS);
        // L110
        output.set(n!("FullDotDPS"), fullDPS.TotalDotDPS);
    }
    // L112
    env.player.output.get()
}

/// `calcs.calcFullDPS` result (Calcs.lua:146-160). `cullingDPS` (L348, set only when
/// `cullingMulti > 0`) lives in `extra`.
#[derive(Default)]
pub struct FullDPS {
    pub combinedDPS: f64,
    pub TotalDotDPS: f64,
    pub skills: Table,
    pub TotalPoisonDPS: f64,
    pub causticGroundDPS: f64,
    pub impaleDPS: f64,
    pub igniteDPS: f64,
    pub burningGroundDPS: f64,
    pub bleedDPS: f64,
    pub corruptingBloodDPS: f64,
    pub decayDPS: f64,
    pub dotDPS: f64,
    pub cullingMulti: f64,
    pub extra: Table,
}

// @pob Modules/Calcs.lua:6-22 h=6c1fcf44
// (module-level statements: pairs,ipairs,t_insert,s_format,m_min,m_ceil,calcs,calcs.breakdownModule,CallStat,CallStat,CallStat,C…)
// `calcs.breakdownModule = "Modules.CalcBreakdown"` is `crate::breakdown::newBreakdown`; the
// requires are this crate's module tree.

/// @pob Modules/Calcs.lua:25-35 h=a5dc713c
/// Get the average value of a table -- note this is unused
/// (`math.average(t)`; the slice holds the number values of `t`, which are the only ones counted).
pub fn average(t: &[f64]) -> f64 {
    // L26
    let mut sum = 0.0;
    // L27
    let mut count = 0.0;
    // L28 LUA-ORDER: pairs; the float sum follows the slice order
    for v in t {
        // L29 (every element is a number)
        // L30
        sum += *v;
        // L31
        count += 1.0;
    }
    // L34
    sum / count
}

// @pob-skip Modules/Calcs.lua:38-88 reason="infoDump: debug printing of an env (commented out at its only call site, Calcs.lua:867)"

/// @pob Modules/Calcs.lua:89-114 h=14afc9e0
/// Get calculator for other changes (adding/removing nodes, items, gems, etc)
/// → (calculator, base output). The calculator calls `initEnv(build, "CALCULATOR", override)`
/// without a specEnv ([`MiscCalculator::call`], L100-113).
pub fn getMiscCalculator(build: &BuildRef) -> (Rc<MiscCalculator>, Output) {
    // Run base calculation pass
    // L91
    let (env, cachedPlayerDB, cachedEnemyDB, cachedMinionDB) =
        initEnv(build, Mode::CALCULATOR, None, None);
    // L92
    perform(&env, false);
    // L93
    let fullDPS = calcFullDPS(
        build,
        Mode::CALCULATOR,
        Rc::new(Override::default()),
        Some(&SpecEnv {
            cachedPlayerDB,
            cachedEnemyDB,
            cachedMinionDB,
            env: None,
            accelerate: Accelerate::default(),
        }),
    );
    // L94
    let usedFullDPS = fullDPS.skills.len() > 0;
    // L95
    if usedFullDPS {
        let output = env.player.output.get();
        // L96
        output.set(n!("SkillDPS"), fullDPS.skills.clone());
        // L97
        output.set(n!("FullDPS"), fullDPS.combinedDPS);
        // L98
        output.set(n!("FullDotDPS"), fullDPS.TotalDotDPS);
    }
    // L100
    (
        Rc::new(MiscCalculator {
            build: build.clone(),
            usedFullDPS: Cell::new(usedFullDPS),
        }),
        env.player.output.get(),
    )
}

/// @pob Modules/Calcs.lua:116-140 h=197f3f34
/// → (count, enabled).
pub(crate) fn getActiveSkillCount(activeSkill: &ActiveSkillRef) -> (f64, bool) {
    // L117
    let Some(socketGroup) = activeSkill.socketGroup.get() else {
        // L118
        return (1.0, true);
    };
    // L119
    if let Some(groupCount) = socketGroup.groupCount.get() {
        // L120
        return (groupCount, true);
    }
    // L122
    let gemList = socketGroup.gemList.get();
    let grantedEffect = activeSkill.activeEffect.get().grantedEffect.get();
    // L123 `pairs` over the (hole-free) gem list visits its array part in index order
    for gemData in gemList.snapshot() {
        // L124
        if let Some(gd) = gemData.gemData.get() {
            // L125
            if gd.vaalGem {
                // L126
                if gd.grantedEffectList.first().is_some_and(|g| Arc::ptr_eq(&grantedEffect, g)) {
                    // L127
                    return (gemData.count.get().unwrap_or(1.0), gemData.enableGlobal1.get());
                // L128
                } else if gd.grantedEffectList.get(1).is_some_and(|g| Arc::ptr_eq(&grantedEffect, g)) {
                    // L129
                    return (gemData.count.get().unwrap_or(1.0), gemData.enableGlobal2.get());
                }
            } else {
                // L132
                if (Arc::ptr_eq(&grantedEffect, &gd.grantedEffect) && !gd.grantedEffect.support)
                    || gd
                        .secondaryGrantedEffect
                        .as_ref()
                        .is_some_and(|g| Arc::ptr_eq(&grantedEffect, g))
                {
                    // L133
                    return (gemData.count.get().unwrap_or(1.0), true);
                }
            }
        }
    }
    // L139
    (1.0, true)
}

/// `x and x > y` for a nil-able output value (`output.X and output.X > y`).
fn gt(v: &Val, y: f64) -> bool {
    v.truthy() && lua_gt(v, y)
}

/// Lua `x > y` with a number `y`: no string coercion; a non-number `x` raises LuaJIT's
/// "attempt to compare number with <type>" (`x > y` is compiled as `y < x`).
fn lua_gt(x: &Val, y: f64) -> bool {
    match x {
        Val::Num(n) => *n > y,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}

/// One `fullDPS.skills` row (`{ name = …, dps = …, count = …, trigger = …, skillPart = … }`;
/// nil fields are absent).
fn skill_row(name: Val, dps: f64, count: f64, trigger: Val, skillPart: Val) -> Table {
    let t = Table::new();
    t.set(n!("name"), name);
    t.set(n!("dps"), dps);
    t.set(n!("count"), count);
    t.set(n!("trigger"), trigger);
    t.set(n!("skillPart"), skillPart);
    t
}

/// One ailment row (`{ name = …, dps = …, count = 1, source = … }`).
fn total_row(name: &str, dps: f64, source: Option<&str>) -> Table {
    let t = Table::new();
    t.set(n!("name"), name);
    t.set(n!("dps"), dps);
    t.set(n!("count"), 1.0);
    if let Some(s) = source {
        t.set(n!("source"), s);
    }
    t
}

fn opt_str(v: Option<LStr>) -> Val {
    v.map_or(Val::Nil, Val::from)
}

/// `a .. b` where `b` is a nil-able string field (nil raises, as Lua's concat does).
fn concat_field(a: &str, b: Option<LStr>, field: &str) -> String {
    match b {
        Some(b) => format!("{a}{b}"),
        None => lua_error!("attempt to concatenate field '{field}' (a nil value)"),
    }
}

/// `actor.minionData.name` (nil minionData raises).
fn minion_name(actor: &ActorRef) -> Name {
    actor
        .minionData
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'minionData' (a nil value)"))
        .name
}

/// @pob Modules/Calcs.lua:142-354 h=59802c76
pub fn calcFullDPS(
    build: &BuildRef,
    mode: Mode,
    r#override: Rc<Override>,
    specEnv: Option<&SpecEnv>,
) -> FullDPS {
    // L143
    let (mut fullEnv, cachedPlayerDB, cachedEnemyDB, cachedMinionDB) =
        initEnv(build, mode, Some(r#override.clone()), specEnv);
    // L144
    let mut usedEnv: Option<EnvRef> = None;

    // L146
    let mut fullDPS = FullDPS {
        combinedDPS: 0.0,
        TotalDotDPS: 0.0,
        skills: Table::new(),
        TotalPoisonDPS: 0.0,
        causticGroundDPS: 0.0,
        impaleDPS: 0.0,
        igniteDPS: 0.0,
        burningGroundDPS: 0.0,
        bleedDPS: 0.0,
        corruptingBloodDPS: 0.0,
        decayDPS: 0.0,
        dotDPS: 0.0,
        cullingMulti: 0.0,
        extra: Table::new(),
    };

    // L162
    let mut bleedSource: String = String::new();
    // L163
    let mut corruptingBloodSource: String = String::new();
    // L164
    let mut igniteSource: String = String::new();
    // L165
    let mut burningGroundSource: String = String::new();
    // L166
    let mut causticGroundSource: String = String::new();

    // L168 `ipairs` over the list object of the first env, read live (fullEnv is reassigned below)
    for_ipairs!((_i, activeSkill) in fullEnv.player.activeSkillList.get() => {
        // L169
        if activeSkill.socketGroup.get().is_some_and(|g| g.includeInFullDPS.get()) {
            // L170
            let (mut activeSkillCount, enabled) = getActiveSkillCount(&activeSkill);
            // L171
            if enabled {
                // L172
                fullEnv.player.mainSkill.set(Some(activeSkill.clone()));
                // L173
                perform(&fullEnv, true);
                // L174
                usedEnv = Some(fullEnv.clone());
                let uenv = fullEnv.clone();
                let activeName: Name = activeSkill.activeEffect.get().grantedEffect.get().name;
                // L175
                let mut minionName: Option<String> = None;
                // L176
                if activeSkill.minion.get().is_some() || uenv.minion.get().is_some() {
                    let minion = uenv
                        .minion
                        .get()
                        .unwrap_or_else(|| lua_error!("attempt to index field 'minion' (a nil value)"));
                    let mout = minion.output.get();
                    // L177
                    if gt(&mout.get(n!("TotalDPS")), 0.0) {
                        // L178
                        minionName = Some(match activeSkill.minion.get() {
                            Some(m) => cat!(minion_name(&m).as_str(), ": ").to_string(),
                            None => cat!(minion_name(&minion).as_str(), ": ").to_string(),
                        });
                        // L179
                        let mn = minionName.clone().unwrap_or_default();
                        fullDPS.skills.push(skill_row(
                            Val::from(activeName),
                            mout.num(n!("TotalDPS")),
                            activeSkillCount,
                            opt_str(activeSkill.infoTrigger.get()),
                            Val::from(concat_field(&mn, activeSkill.skillPartName.get(), "skillPartName")),
                        ));
                        // L180
                        fullDPS.combinedDPS = fullDPS.combinedDPS + mout.num(n!("TotalDPS")) * activeSkillCount;
                    }
                    // L182
                    if gt(&mout.get(n!("BleedDPS")), fullDPS.bleedDPS) {
                        // L183
                        fullDPS.bleedDPS = mout.num(n!("BleedDPS"));
                        // L184
                        bleedSource = activeName.to_string();
                    }
                    // L186
                    if gt(&mout.get(n!("IgniteDPS")), fullDPS.igniteDPS) {
                        // L187
                        fullDPS.igniteDPS = mout.num(n!("IgniteDPS"));
                        // L188
                        igniteSource = activeName.to_string();
                    }
                    // L190
                    if gt(&mout.get(n!("PoisonDPS")), 0.0) {
                        // L191
                        fullDPS.TotalPoisonDPS = fullDPS.TotalPoisonDPS + mout.num(n!("TotalPoisonDPS")) * activeSkillCount;
                    }
                    // L193
                    if gt(&mout.get(n!("ImpaleDPS")), 0.0) {
                        // L194
                        fullDPS.impaleDPS = fullDPS.impaleDPS + mout.num(n!("ImpaleDPS")) * activeSkillCount;
                    }
                    // L196
                    if gt(&mout.get(n!("DecayDPS")), 0.0) {
                        // L197
                        fullDPS.decayDPS = fullDPS.decayDPS + mout.num(n!("DecayDPS"));
                    }
                    // L199
                    if gt(&mout.get(n!("TotalDot")), 0.0) {
                        // L200
                        fullDPS.dotDPS = fullDPS.dotDPS + mout.num(n!("TotalDot"));
                    }
                    // L202
                    if gt(&mout.get(n!("CullMultiplier")), 1.0)
                        && mout.num(n!("CullMultiplier")) > fullDPS.cullingMulti
                    {
                        // L203
                        fullDPS.cullingMulti = mout.num(n!("CullMultiplier"));
                    }
                    // This is a fix to prevent skills such as Absolution or Dominating Blow from being counted multiple times when increasing minions count
                    // L206 (the patterns have no magic characters: `:match` is a substring test)
                    if (activeName.as_str().contains("Absolution")
                        && fullEnv.modDB.get().Flag(None, nm!["Condition:AbsolutionSkillDamageCountedOnce"]))
                        || (activeName.as_str().contains("Dominating Blow")
                            && fullEnv.modDB.get().Flag(None, nm!["Condition:DominatingBlowSkillDamageCountedOnce"]))
                        || (activeName.as_str().contains("Holy Strike")
                            && fullEnv.modDB.get().Flag(None, nm!["Condition:HolyStrikeSkillDamageCountedOnce"]))
                    {
                        // L209
                        activeSkillCount = 1.0;
                        // L210
                        activeSkill.infoMessage2.set(Some(LStr::from("Skill Damage")));
                    }
                }

                // L214
                if let Some(mirage) = activeSkill.mirage.get() {
                    let mirageOutput = mirage
                        .table(n!("output"))
                        .unwrap_or_else(|| lua_error!("attempt to index field 'output' (a nil value)"));
                    let mirageName = format!("{activeName} (Mirage)");
                    // L215
                    let mirageCount = mirage.num_or(n!("count"), 1.0) * activeSkillCount;
                    // L216
                    if gt(&mirageOutput.get(n!("TotalDPS")), 0.0) {
                        // L217
                        let name = match mirage.get(n!("name")) {
                            Val::Str(s) => format!("{s} (Mirage)"),
                            Val::Num(x) => format!("{} (Mirage)", tostring_num(x)),
                            other => lua_error!("attempt to concatenate field 'name' (a {} value)", other.lua_type()),
                        };
                        fullDPS.skills.push(skill_row(
                            Val::from(name),
                            mirageOutput.num(n!("TotalDPS")),
                            mirageCount,
                            mirage.get(n!("infoTrigger")),
                            mirage.get(n!("skillPartName")),
                        ));
                        // L218
                        fullDPS.combinedDPS = fullDPS.combinedDPS + mirageOutput.num(n!("TotalDPS")) * mirageCount;
                    }
                    // L220
                    if gt(&mirageOutput.get(n!("BleedDPS")), fullDPS.bleedDPS) {
                        // L221
                        fullDPS.bleedDPS = mirageOutput.num(n!("BleedDPS"));
                        // L222
                        bleedSource = mirageName.clone();
                    }
                    // L224
                    if gt(&mirageOutput.get(n!("IgniteDPS")), fullDPS.igniteDPS) {
                        // L225
                        fullDPS.igniteDPS = mirageOutput.num(n!("IgniteDPS"));
                        // L226
                        igniteSource = mirageName.clone();
                    }
                    // L228
                    if gt(&mirageOutput.get(n!("PoisonDPS")), 0.0) {
                        // L229
                        fullDPS.TotalPoisonDPS = fullDPS.TotalPoisonDPS + mirageOutput.num(n!("TotalPoisonDPS")) * mirageCount;
                    }
                    // L231
                    if gt(&mirageOutput.get(n!("ImpaleDPS")), 0.0) {
                        // L232
                        fullDPS.impaleDPS = fullDPS.impaleDPS + mirageOutput.num(n!("ImpaleDPS")) * mirageCount;
                    }
                    // L234
                    if gt(&mirageOutput.get(n!("DecayDPS")), 0.0) {
                        // L235
                        fullDPS.decayDPS = fullDPS.decayDPS + mirageOutput.num(n!("DecayDPS"));
                    }
                    // L237
                    let dotCanStack = activeSkill.skillFlags.get().truthy(n!("DotCanStack"));
                    if gt(&mirageOutput.get(n!("TotalDot")), 0.0)
                        && (dotCanStack || {
                            let t = uenv.player.output.get().get(n!("TotalDot"));
                            t.truthy() && t.raw_eq(&Val::Num(0.0))
                        })
                    {
                        // L238
                        fullDPS.dotDPS = fullDPS.dotDPS
                            + mirageOutput.num(n!("TotalDot")) * (if dotCanStack { mirageCount } else { 1.0 });
                    }
                    // L240
                    if gt(&mirageOutput.get(n!("CullMultiplier")), 1.0)
                        && mirageOutput.num(n!("CullMultiplier")) > fullDPS.cullingMulti
                    {
                        // L241
                        fullDPS.cullingMulti = mirageOutput.num(n!("CullMultiplier"));
                    }
                    // L243
                    if gt(&mirageOutput.get(n!("BurningGroundDPS")), fullDPS.burningGroundDPS) {
                        // L244
                        fullDPS.burningGroundDPS = mirageOutput.num(n!("BurningGroundDPS"));
                        // L245
                        burningGroundSource = mirageName.clone();
                    }
                    // L247
                    if gt(&mirageOutput.get(n!("CausticGroundDPS")), fullDPS.causticGroundDPS) {
                        // L248
                        fullDPS.causticGroundDPS = mirageOutput.num(n!("CausticGroundDPS"));
                        // L249
                        causticGroundSource = mirageName.clone();
                    }
                }

                let pout = uenv.player.output.get();
                // L253
                if gt(&pout.get(n!("TotalDPS")), 0.0) {
                    // L254 `minionName and activeSkill.infoMessage2 or activeSkill.skillPartName`
                    let skillPart = match (&minionName, activeSkill.infoMessage2.get()) {
                        (Some(_), Some(m)) => Val::from(m),
                        _ => opt_str(activeSkill.skillPartName.get()),
                    };
                    fullDPS.skills.push(skill_row(
                        Val::from(activeName),
                        pout.num(n!("TotalDPS")),
                        activeSkillCount,
                        opt_str(activeSkill.infoTrigger.get()),
                        skillPart,
                    ));
                    // L255
                    fullDPS.combinedDPS = fullDPS.combinedDPS + pout.num(n!("TotalDPS")) * activeSkillCount;
                }
                // L257
                if gt(&pout.get(n!("BleedDPS")), fullDPS.bleedDPS) {
                    // L258
                    fullDPS.bleedDPS = pout.num(n!("BleedDPS"));
                    // L259
                    bleedSource = activeName.to_string();
                }
                // L261
                if gt(&pout.get(n!("CorruptingBloodDPS")), fullDPS.corruptingBloodDPS) {
                    // L262
                    fullDPS.corruptingBloodDPS = pout.num(n!("CorruptingBloodDPS"));
                    // L263
                    corruptingBloodSource = activeName.to_string();
                }
                // L265
                if gt(&pout.get(n!("IgniteDPS")), fullDPS.igniteDPS) {
                    // L266
                    fullDPS.igniteDPS = pout.num(n!("IgniteDPS"));
                    // L267
                    igniteSource = activeName.to_string();
                }
                // L269
                if gt(&pout.get(n!("BurningGroundDPS")), fullDPS.burningGroundDPS) {
                    // L270
                    fullDPS.burningGroundDPS = pout.num(n!("BurningGroundDPS"));
                    // L271
                    burningGroundSource = activeName.to_string();
                }
                // L273
                if gt(&pout.get(n!("PoisonDPS")), 0.0) {
                    // L274
                    fullDPS.TotalPoisonDPS = fullDPS.TotalPoisonDPS + pout.num(n!("TotalPoisonDPS")) * activeSkillCount;
                }
                // L276
                if gt(&pout.get(n!("CausticGroundDPS")), fullDPS.causticGroundDPS) {
                    // L277
                    fullDPS.causticGroundDPS = pout.num(n!("CausticGroundDPS"));
                    // L278
                    causticGroundSource = activeName.to_string();
                }
                // L280
                if gt(&pout.get(n!("ImpaleDPS")), 0.0) {
                    // L281
                    fullDPS.impaleDPS = fullDPS.impaleDPS + pout.num(n!("ImpaleDPS")) * activeSkillCount;
                }
                // L283
                if gt(&pout.get(n!("DecayDPS")), 0.0) {
                    // L284
                    fullDPS.decayDPS = fullDPS.decayDPS + pout.num(n!("DecayDPS"));
                }
                // L286
                if gt(&pout.get(n!("TotalDot")), 0.0) {
                    // L287
                    let dotCanStack = activeSkill.skillFlags.get().truthy(n!("DotCanStack"));
                    fullDPS.dotDPS = fullDPS.dotDPS
                        + pout.num(n!("TotalDot")) * (if dotCanStack { activeSkillCount } else { 1.0 });
                }
                // L289
                if gt(&pout.get(n!("CullMultiplier")), 1.0)
                    && pout.num(n!("CullMultiplier")) > fullDPS.cullingMulti
                {
                    // L290
                    fullDPS.cullingMulti = pout.num(n!("CullMultiplier"));
                }

                // Re-Build env calculator for new run
                // L294
                let accelerationTbl = Accelerate {
                    nodeAlloc: true,
                    requirementsItems: true,
                    requirementsGems: true,
                    skills: true,
                    everything: true,
                };
                // L301
                fullEnv = initEnv(
                    build,
                    mode,
                    Some(r#override.clone()),
                    Some(&SpecEnv {
                        cachedPlayerDB: cachedPlayerDB.clone(),
                        cachedEnemyDB: cachedEnemyDB.clone(),
                        cachedMinionDB: cachedMinionDB.clone(),
                        env: Some(fullEnv.clone()),
                        accelerate: accelerationTbl,
                    }),
                )
                .0;
            }
        }
    });
    let _ = usedEnv;

    // Re-Add ailment DPS components
    // L307
    fullDPS.TotalDotDPS = 0.0;
    // L308
    if fullDPS.bleedDPS > 0.0 {
        // L309
        fullDPS.skills.push(total_row("Best Bleed DPS", fullDPS.bleedDPS, Some(&bleedSource)));
        // L310
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.bleedDPS;
    }
    // L312
    if fullDPS.corruptingBloodDPS > 0.0 {
        // L313
        fullDPS.skills.push(total_row(
            "Corrupting Blood DPS",
            fullDPS.corruptingBloodDPS,
            Some(&corruptingBloodSource),
        ));
        // L314
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.corruptingBloodDPS;
    }
    // L316
    if fullDPS.igniteDPS > 0.0 {
        // L317
        fullDPS.skills.push(total_row("Best Ignite DPS", fullDPS.igniteDPS, Some(&igniteSource)));
        // L318
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.igniteDPS;
    }
    // L320
    if fullDPS.burningGroundDPS > 0.0 {
        // L321
        fullDPS.skills.push(total_row(
            "Best Burning Ground DPS",
            fullDPS.burningGroundDPS,
            Some(&burningGroundSource),
        ));
        // L322
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.burningGroundDPS;
    }
    // L324
    if fullDPS.TotalPoisonDPS > 0.0 {
        // L325
        fullDPS.TotalPoisonDPS = m_min(fullDPS.TotalPoisonDPS, data().misc.DotDpsCap);
        // L326
        fullDPS.skills.push(total_row("Full Poison DPS", fullDPS.TotalPoisonDPS, None));
        // L327
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.TotalPoisonDPS;
    }
    // L329
    if fullDPS.causticGroundDPS > 0.0 {
        // L330
        fullDPS.skills.push(total_row(
            "Best Caustic Ground DPS",
            fullDPS.causticGroundDPS,
            Some(&causticGroundSource),
        ));
        // L331
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.causticGroundDPS;
    }
    // L333
    if fullDPS.impaleDPS > 0.0 {
        // L334
        fullDPS.skills.push(total_row("Full Impale DPS", fullDPS.impaleDPS, None));
        // L335
        fullDPS.combinedDPS = fullDPS.combinedDPS + fullDPS.impaleDPS;
    }
    // L337
    if fullDPS.decayDPS > 0.0 {
        // L338
        fullDPS.skills.push(total_row("Full Decay DPS", fullDPS.decayDPS, None));
        // L339
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.decayDPS;
    }
    // L341
    if fullDPS.dotDPS > 0.0 {
        // L342
        fullDPS.skills.push(total_row("Full DoT DPS", fullDPS.dotDPS, None));
        // L343
        fullDPS.TotalDotDPS = fullDPS.TotalDotDPS + fullDPS.dotDPS;
    }
    // L345
    fullDPS.TotalDotDPS = m_min(fullDPS.TotalDotDPS, data().misc.DotDpsCap);
    // L346
    fullDPS.combinedDPS = fullDPS.combinedDPS + fullDPS.TotalDotDPS;
    // L347
    if fullDPS.cullingMulti > 0.0 {
        // L348
        let cullingDPS = fullDPS.combinedDPS * (fullDPS.cullingMulti - 1.0);
        fullDPS.extra.set(n!("cullingDPS"), cullingDPS);
        // L349
        fullDPS.skills.push(total_row("Full Culling DPS", cullingDPS, None));
        // L350
        fullDPS.combinedDPS = fullDPS.combinedDPS + cullingDPS;
    }

    // L353
    fullDPS
}

/// Test seam of [`buildActiveSkill`] (feature `probe` only; the shipped calc has no hook): an
/// L4 test that injects a segment's state stands the nested calc in with recorded GlobalCache
/// entries (TR-A / TR-B: `triggers:out`'s cache) and replays the call, since the injected build
/// is a placeholder the full `initEnv` cannot run on.
#[cfg(feature = "probe")]
pub mod replay_hook {
    use std::cell::RefCell;

    use crate::prelude::*;

    /// One `calcs.buildActiveSkill(env, mode, skill, targetUUID, limitedProcessingFlags)` call.
    pub struct Call<'a> {
        pub env: &'a EnvRef,
        pub mode: Mode,
        pub skill: &'a ActiveSkillRef,
        pub targetUUID: Option<Name>,
        pub limitedProcessingFlags: &'a [Name],
    }

    /// `true` when the hook replayed the call (the body is skipped), `false` to run it.
    pub type Hook = Box<dyn FnMut(&Call) -> bool>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Install (or with `None` remove) the hook; returns the previous one.
    pub fn set(h: Option<Hook>) -> Option<Hook> {
        HOOK.with(|c| c.replace(h))
    }

    pub(super) fn try_replay(call: &Call) -> bool {
        let Some(mut h) = HOOK.with(|c| c.borrow_mut().take()) else {
            return false;
        };
        let r = h(call);
        HOOK.with(|c| {
            let mut slot = c.borrow_mut();
            if slot.is_none() {
                *slot = Some(h);
            }
        });
        r
    }
}

/// @pob Modules/Calcs.lua:357-381 h=83413cac
/// Process active skill
pub fn buildActiveSkill(
    env: &EnvRef,
    mode: Mode,
    skill: &ActiveSkillRef,
    targetUUID: Option<Name>,
    limitedProcessingFlags: &[Name],
) {
    #[cfg(feature = "probe")]
    if replay_hook::try_replay(&replay_hook::Call {
        env,
        mode,
        skill,
        targetUUID,
        limitedProcessingFlags,
    }) {
        return;
    }
    // L358
    let (fullEnv, _, _, _) = initEnv(&env.build, mode, Some(env.r#override.clone()), None);
    // L359
    fullEnv.buildBreakdown.set(false);

    // env.limitedSkills contains a map of uuids that should be limited in calculation
    // this is in order to prevent infinite recursion loops
    // L363
    let limitedSkills = fullEnv.limitedSkills.get().unwrap_or_default();
    fullEnv.limitedSkills.set(Some(limitedSkills.clone()));
    // L364 LUA-ORDER: pairs; only sets keys, so the result does not depend on it
    for (uuid, _) in env.limitedSkills.get().unwrap_or_default().pairs() {
        // L365
        limitedSkills.set(uuid, true);
    }
    // L367
    for uuid in limitedProcessingFlags {
        // L368
        limitedSkills.set(*uuid, true);
    }

    // L371
    let targetUUID = targetUUID.unwrap_or_else(|| cacheSkillUUID(skill, env));
    // L372
    for_ipairs!((_i, activeSkill) in fullEnv.player.activeSkillList.get() => {
        // L373
        let activeSkillUUID = cacheSkillUUID(&activeSkill, &fullEnv);
        // L374
        if activeSkillUUID == targetUUID {
            // L375
            fullEnv.player.mainSkill.set(Some(activeSkill.clone()));
            // L376
            perform(&fullEnv, true);
            // L377
            return;
        }
    });
    // L380
    ConPrintf(&format!(
        "[calcs.buildActiveSkill] Failed to process skill: {}",
        skill.activeEffect.get().grantedEffect.get().name
    ));
}

/// `ConPrintf(...)`: PoB's console.
fn ConPrintf(s: &str) {
    eprintln!("{s}");
}

/// @pob Modules/Calcs.lua:384-870 h=60c7a3ba
/// Build output for display in the side bar or calcs tab
/// (The MAIN cost warnings L399-436, the MAIN used-tracking L437-600 and the CALCS lists
/// L601-867 are split into [`buildOutput_costWarnings`], [`buildOutput_main`] and
/// [`buildOutput_calcs`]; one marker covers the function.)
pub fn buildOutput(build: &BuildRef, mode: Mode) -> EnvRef {
    // Build output for selected main skill
    // L386
    let (env, cachedPlayerDB, cachedEnemyDB, cachedMinionDB) = initEnv(build, mode, None, None);
    // L387
    perform(&env, false);

    // L389
    let output = env.player.output.get();

    // Build output across all skills added to FullDPS skills
    // L392
    let fullDPS = calcFullDPS(
        build,
        Mode::CALCULATOR,
        Rc::new(Override::default()),
        Some(&SpecEnv {
            cachedPlayerDB,
            cachedEnemyDB,
            cachedMinionDB,
            env: None,
            accelerate: Accelerate::default(),
        }),
    );

    // Add Full DPS data to main `env`
    // L395
    env.player.output.get().set(n!("SkillDPS"), fullDPS.skills.clone());
    // L396
    env.player.output.get().set(n!("FullDPS"), fullDPS.combinedDPS);
    // L397
    env.player.output.get().set(n!("FullDotDPS"), fullDPS.TotalDotDPS);

    // L399
    if mode == Mode::MAIN {
        buildOutput_costWarnings(&env, mode, &output);
        buildOutput_main(&env, &output);
    // L601
    } else if mode == Mode::CALCS {
        buildOutput_calcs(build, &env, &output);
    }

    // L869
    env
}

/// The `{ pool = costResource }` literals of L407/L425 (`pairs` order: the entries write
/// disjoint output keys, so the order is not observable).
const COST_POOLS: [(&str, &str); 4] = [
    ("LifeUnreserved", "LifeCost"),
    ("ManaUnreserved", "ManaCost"),
    ("Rage", "RageCost"),
    ("EnergyShield", "ESCost"),
];
const PERCENT_COST_POOLS: [(&str, &str); 2] = [
    ("LifeUnreservedPercent", "LifePercentCost"),
    ("ManaUnreservedPercent", "ManaPercentCost"),
];

/// `output[k] = output[k] or {}` then `t_insert(output[k], v)`.
fn push_list(output: &Output, k: Name, v: Val) {
    let list = match output.get(k) {
        Val::Table(t) => t,
        Val::Nil | Val::Bool(false) => {
            let t = Table::new();
            output.set(k, t.clone());
            t
        }
        other => lua_error!("bad argument #1 to 'insert' (table expected, got {})", other.lua_type()),
    };
    list.push(v);
}

/// `a < b` on Lua values that must both be numbers.
fn lt_num(a: f64, b: &Val) -> bool {
    match b {
        Val::Num(b) => a < *b,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}

/// Calcs.lua:400-435 (body of [`buildOutput`]): the cost warnings, MAIN mode.
pub fn buildOutput_costWarnings(env: &EnvRef, mode: Mode, output: &Output) {
    // L400
    for_ipairs!((_i, skill) in env.player.activeSkillList.get() => {
        // L401
        let uuid = cacheSkillUUID(&skill, env);
        // L402
        if cachedData(mode, uuid).is_none() {
            // L403
            buildActiveSkill(env, mode, &skill, Some(uuid), &[]);
        }
        // L405
        if let Some(cached) = cachedData(mode, uuid) {
            let skillName = skill.activeEffect.get().grantedEffect.get().name;
            // L406 (`ModDB:Flag` returns `true` or nothing: a false flag leaves the key nil)
            output.set(
                n!("EnergyShieldProtectsMana"),
                if env.modDB.get().Flag(None, nm!["EnergyShieldProtectsMana"]) {
                    Val::Bool(true)
                } else {
                    Val::Nil
                },
            );
            // L407
            for (pool, costResource) in COST_POOLS {
                let pool = Name::from(pool);
                let costResource = Name::from(costResource);
                // L408
                let cachedCost = cached.Env.player.output.get().get(costResource);
                // L409
                if cachedCost.truthy() {
                    // L410
                    let esPart = if output.truthy(n!("EnergyShieldProtectsMana"))
                        && costResource == n!("ManaCost")
                    {
                        match output.get(n!("EnergyShield")) {
                            v if v.truthy() => v.num(),
                            _ => 0.0,
                        }
                    } else {
                        0.0
                    };
                    let totalPool = esPart + output.num0(pool);
                    // L411
                    if lt_num(totalPool, &cachedCost) {
                        // L412
                        let rawPool = pob_lua::gsub(pool.as_str(), "Unreserved$", "", None).0;
                        // L413
                        let reservation = match cached.Env.player.mainSkill.get() {
                            Some(ms) => ms.skillData.get().get(cat!(rawPool.as_str(), "ReservedPercent")),
                            None => Val::Nil,
                        };
                        // Skill has both cost and reservation check if there's available pool for raw cost before reservation
                        // L415
                        if !reservation.truthy()
                            || lt_num(
                                totalPool
                                    + m_ceil(
                                        output.num0(Name::from(rawPool.as_str())) * reservation.num() / 100.0,
                                    ),
                                &cachedCost,
                            )
                        {
                            // L416
                            if env
                                .player
                                .mainSkill
                                .get()
                                .is_some_and(|ms| ms.activeEffect.get().grantedEffect.get().name == skillName)
                            {
                                // L417
                                output.set(cat!(costResource.as_str(), "Warning"), true);
                            }
                            // L419-420
                            push_list(output, cat!(costResource.as_str(), "WarningList"), Val::from(skillName));
                        }
                    }
                }
            }
            // L425
            for (pool, costResource) in PERCENT_COST_POOLS {
                let pool = Name::from(pool);
                let costResource = Name::from(costResource);
                // L426
                let cachedCost = cached.Env.player.output.get().get(costResource);
                // L427
                if cachedCost.truthy() {
                    // L428
                    if lt_num(output.num0(pool), &cachedCost) {
                        // L429-430 LUA-QUIRK: the key is "<X>PercentCostPercentCostWarningList"
                        push_list(
                            output,
                            cat!(costResource.as_str(), "PercentCostWarningList"),
                            Val::from(skillName),
                        );
                    }
                }
            }
        }
    });
}

/// `addTo(out, var, mod)` (Calcs.lua:469-478).
fn addTo(out: &Table, var: Option<Name>, m: &ModRef) {
    // Do not count Base mods as mods being actually used as they are only used as descriptors for mods
    // L471
    if m.source.as_deref() == Some("Base") {
        // L472
        return;
    }
    // L474 (a nil `var` reads nil, then `out[nil] = { }` raises)
    let Some(var) = var else {
        lua_error!("table index is nil");
    };
    if !out.truthy(var) {
        // L475
        out.set(var, Table::new());
    }
    // L477
    match out.get(var) {
        Val::Table(t) => t.push(Val::Mod(m.clone())),
        other => lua_error!("bad argument #1 to 'insert' (table expected, got {})", other.lua_type()),
    }
}

/// `addVarTag(out, tag, mod)` (Calcs.lua:479-487).
fn addVarTag(out: &Table, tag: &Tag, m: &ModRef) {
    // L480
    if let Some(varList) = &tag.varList {
        // L481
        for var in varList.iter() {
            // L482
            addTo(out, Some(*var), m);
        }
    } else {
        // L485
        addTo(out, tag.var, m);
    }
}

/// `addStatTag(out, tag, mod)` (Calcs.lua:488-496).
fn addStatTag(out: &Table, tag: &Tag, m: &ModRef) {
    // L489 LUA-QUIRK: tests `varList`, iterates `statList`
    if tag.varList.is_some() {
        // L490
        let Some(statList) = &tag.statList else {
            lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)");
        };
        for var in statList.iter() {
            // L491
            addTo(out, Some(*var), m);
        }
    // L493
    } else if let Some(stat) = tag.stat {
        // L494
        addTo(out, Some(stat), m);
    }
}

/// `tag.type` (a non-table tag entry raises when indexed).
fn tag_type(tag: &Tag) -> Name {
    if tag.r#type == TagType::LuaNonTable {
        lua_error!("attempt to index local 'tag' (a number value)");
    }
    Name::from(tag.r#type.as_str())
}

/// The used-tracking tables of Calcs.lua:448-468 (the env's `*Used` fields).
struct Used {
    conditionsUsed: Table,
    enemyConditionsUsed: Table,
    minionConditionsUsed: Table,
    multipliersUsed: Table,
    enemyMultipliersUsed: Table,
    perStatsUsed: Table,
    enemyPerStatsUsed: Table,
    tagTypesUsed: Table,
    modsUsed: Table,
}

/// The `{ pattern = conditions }` literal of L502 (`pairs` order: each entry appends to its
/// own keys, so the order is not observable).
const DMG_TYPE_CONDITIONS: [(&str, &[&str]); 2] = [
    ("[fi][ig][rn][ei]t?e?", &["Ignited", "Burning"]),
    ("[cf][or][le][de]z?e?", &["Frozen"]),
];

/// `addModTags(actor, mod)` (Calcs.lua:497-544). `actor` may be nil (`env.minion` at L565).
fn addModTags(env: &EnvRef, u: &Used, actor: Option<&ActorRef>, m: &ModRef) {
    let isPlayer = actor.is_some_and(|a| Rc::ptr_eq(a, &env.player));
    // L498
    addTo(&u.modsUsed, Some(m.name), m);

    // Imply enemy conditionals based on damage type
    // Needed to preemptively show config options for elemental ailments
    // L502
    let lowerName = pob_lua::string::lower(m.name.as_str());
    for (dmgType, conditions) in DMG_TYPE_CONDITIONS {
        // L503
        if pob_lua::pattern::r#match(&lowerName, dmgType, None).is_some() {
            // L504
            for var in conditions {
                // L505
                addTo(&u.enemyConditionsUsed, Some(Name::from(*var)), m);
            }
        }
    }

    // L510
    for tag in m.tags.iter() {
        let ty = tag_type(tag);
        // L511
        addTo(&u.tagTypesUsed, Some(ty), m);
        // L512
        if tag.r#type == TagType::IgnoreCond {
            // L513
            break;
        // L514
        } else if tag.r#type == TagType::Condition {
            // L515
            if isPlayer {
                // L516
                addVarTag(&u.conditionsUsed, tag, m);
            } else {
                // L518
                addVarTag(&u.minionConditionsUsed, tag, m);
            }
        // L520
        } else if tag.r#type == TagType::ActorCondition && tag.var.is_some() {
            // L521
            if tag.actor == Some(n!("enemy")) {
                // L522
                addTo(&u.enemyConditionsUsed, tag.var, m);
            } else {
                // L524
                addTo(&u.conditionsUsed, tag.var, m);
            }
        // L526
        } else if tag.r#type == TagType::Multiplier || tag.r#type == TagType::MultiplierThreshold {
            // L527
            if tag.actor.is_none() {
                // L528
                if isPlayer {
                    // L529
                    addVarTag(&u.multipliersUsed, tag, m);
                }
            // L531
            } else if tag.actor == Some(n!("enemy")) {
                // L532
                addVarTag(&u.enemyMultipliersUsed, tag, m);
            }
        // L534
        } else if tag.r#type == TagType::PerStat || tag.r#type == TagType::StatThreshold {
            // L535
            if tag.actor.is_none() {
                // L536
                if isPlayer {
                    // L537
                    addStatTag(&u.perStatsUsed, tag, m);
                }
            // L539
            } else if tag.actor == Some(n!("enemy")) {
                // L540
                addStatTag(&u.enemyPerStatsUsed, tag, m);
            }
        }
    }
}

/// Calcs.lua:437-600 (body of [`buildOutput`]): MAIN mode's spec sums and used-tracking.
pub fn buildOutput_main(env: &EnvRef, output: &Output) {
    let modDB = env.modDB.get();
    // L437
    output.set(n!("ExtraPoints"), modDB.Sum(ModType::BASE, None, nm!["ExtraPoints"]));

    // L439
    let specCfg = ModCfg::new();
    specCfg.source.set(Some(n!("Tree")));
    // L442
    output.set(n!("Spec:LifeInc"), modDB.Sum(ModType::INC, Some(&specCfg), nm!["Life"]));
    // L443
    output.set(n!("Spec:ManaInc"), modDB.Sum(ModType::INC, Some(&specCfg), nm!["Mana"]));
    // L444
    output.set(
        n!("Spec:ArmourInc"),
        modDB.Sum(ModType::INC, Some(&specCfg), nm!["Armour", "ArmourAndEvasion"]),
    );
    // L445
    output.set(
        n!("Spec:EvasionInc"),
        modDB.Sum(ModType::INC, Some(&specCfg), nm!["Evasion", "ArmourAndEvasion"]),
    );
    // L446
    output.set(
        n!("Spec:EnergyShieldInc"),
        modDB.Sum(ModType::INC, Some(&specCfg), nm!["EnergyShield"]),
    );

    // L448
    let skillsUsed = Table::new();
    env.skillsUsed.set(Some(skillsUsed.clone()));
    // L449
    for_ipairs!((_i, activeSkill) in env.player.activeSkillList.get() => {
        // L450
        for_ipairs!((_j, skillEffect) in activeSkill.effectList.get() => {
            // L451
            skillsUsed.set(skillEffect.grantedEffect.get().name, true);
        });
        // L453
        if let Some(minion) = activeSkill.minion.get() {
            // L454
            for_ipairs!((_k, activeSkill) in minion.activeSkillList.get() => {
                // L455
                skillsUsed.set(activeSkill.activeEffect.get().grantedEffect.get().id, true);
            });
        }
    });

    // L460-468
    let u = Used {
        conditionsUsed: Table::new(),
        enemyConditionsUsed: Table::new(),
        minionConditionsUsed: Table::new(),
        multipliersUsed: Table::new(),
        enemyMultipliersUsed: Table::new(),
        perStatsUsed: Table::new(),
        enemyPerStatsUsed: Table::new(),
        tagTypesUsed: Table::new(),
        modsUsed: Table::new(),
    };
    env.conditionsUsed.set(Some(u.conditionsUsed.clone()));
    env.enemyConditionsUsed.set(Some(u.enemyConditionsUsed.clone()));
    env.minionConditionsUsed.set(Some(u.minionConditionsUsed.clone()));
    env.multipliersUsed.set(Some(u.multipliersUsed.clone()));
    env.enemyMultipliersUsed.set(Some(u.enemyMultipliersUsed.clone()));
    env.perStatsUsed.set(Some(u.perStatsUsed.clone()));
    env.enemyPerStatsUsed.set(Some(u.enemyPerStatsUsed.clone()));
    env.tagTypesUsed.set(Some(u.tagTypesUsed.clone()));
    env.modsUsed.set(Some(u.modsUsed.clone()));
    // L469-544: addTo, addVarTag, addStatTag, addModTags (module-level fns above)

    // L545 `ipairs({env.player, env.minion})` stops at a nil minion
    let mut actors: Vec<ActorRef> = vec![env.player.clone()];
    if let Some(m) = env.minion.get() {
        actors.push(m);
    }
    for actor in &actors {
        // L546 LUA-ORDER: `pairs(actor.modDB.mods)` (string keys; PoB's order is random per
        // process): names in `mod_names` order. Only the order of the mods inside each
        // `*Used[var]` list depends on it.
        let db = actor.modDB.get();
        for modName in db.mod_names() {
            // L547
            for m in db.mods_named(modName) {
                // L548
                addModTags(env, &u, Some(actor), &m);
            }
        }
    }
    // L552 `pairs` over the (hole-free) skill list visits its array part in index order
    for activeSkill in env.player.activeSkillList.get().snapshot() {
        // L553
        for m in activeSkill.baseSkillModList.get().ipairs() {
            // L554
            addModTags(env, &u, Some(&env.player), &m);
        }
        // L556
        for m in activeSkill.skillModList.get().ipairs() {
            // L557
            addTo(&u.modsUsed, Some(m.name), &m);
            // L558
            for tag in m.tags.iter() {
                // L559
                addTo(&u.tagTypesUsed, Some(tag_type(tag)), &m);
            }
        }
        // L562
        if let Some(minion) = activeSkill.minion.get() {
            // L563
            for activeSkill in minion.activeSkillList.get().snapshot() {
                // L564
                for m in activeSkill.baseSkillModList.get().ipairs() {
                    // L565
                    addModTags(env, &u, env.minion.get().as_ref(), &m);
                }
            }
        }
    }
    // L570 LUA-ORDER: `pairs(env.enemyDB.mods)`, as at L546
    let enemyDB = env.enemyDB.get();
    for modName in enemyDB.mod_names() {
        // L571
        for m in enemyDB.mods_named(modName) {
            // L572
            for tag in m.tags.iter() {
                // L573
                if tag.r#type == TagType::IgnoreCond {
                    // L574
                    break;
                // L575
                } else if tag.r#type == TagType::Condition {
                    // L576
                    addVarTag(&u.enemyConditionsUsed, tag, &m);
                // L577
                } else if tag.r#type == TagType::ActorCondition && tag.var.is_some() {
                    // L578
                    if tag.actor == Some(n!("enemy")) || tag.actor == Some(n!("player")) {
                        // L579
                        addTo(&u.conditionsUsed, tag.var, &m);
                    } else {
                        // L581
                        addTo(&u.enemyConditionsUsed, tag.var, &m);
                    }
                // L583
                } else if tag.r#type == TagType::Multiplier || tag.r#type == TagType::MultiplierThreshold {
                    // L584
                    if tag.actor.is_none() {
                        // L585
                        addVarTag(&u.enemyMultipliersUsed, tag, &m);
                    }
                } else if tag.r#type == TagType::LuaNonTable {
                    // `tag.type` on a non-table entry raises
                    tag_type(tag);
                }
            }
        }
    }
    // L591-600: commented-out ConPrintTable calls
}

/// The mods of a buff / debuff list value (a ModList object or a plain list of mods).
fn mods_of(v: &Val) -> Vec<ModRef> {
    if let Some(store) = v.obj::<ModStore>() {
        return store.ipairs();
    }
    match v {
        Val::Table(t) => {
            let mut out = Vec::new();
            for_ipairs!((_i, m) in t => {
                match m {
                    Val::Mod(m) => out.push(m),
                    Val::Data(Value::Mod(m)) => out.push(m),
                    other => lua_error!("a mod list holds a {} value", other.lua_type()),
                }
            });
            out
        }
        other => lua_error!("bad argument #1 to 'ipairs' (table expected, got {})", other.lua_type()),
    }
}

/// `value and value ~= 0` for an EvalMod result.
fn nonzero(v: &Value) -> bool {
    match v {
        Value::Nil | Value::Bool(false) => false,
        Value::Num(x) => *x != 0.0,
        _ => true,
    }
}

/// `{ mod = mod, value = value }`.
fn mod_row(m: &ModRef, value: &Value) -> Table {
    let t = Table::new();
    t.set(n!("mod"), Val::Mod(m.clone()));
    t.set(n!("value"), Val::from_value(value));
    t
}

/// `t_insert(list, s_format(fmt, output[key]))` when `output[key] > 0` (nil raises).
fn charges(list: &mut Vec<String>, output: &Output, key: Name, fmt: &str) {
    if lua_gt(&output.get(key), 0.0) {
        list.push(lfmt!(fmt, output.num(key)));
    }
}

/// `actor.breakdown` (nil raises when indexed).
fn breakdown_of(actor: &ActorRef) -> Breakdown {
    actor
        .breakdown
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'breakdown' (a nil value)"))
}

/// The modDB flags of L656-753 with their CombatList labels, in Lua order.
const PLAYER_COMBAT_FLAGS: [(&str, &str); 25] = [
    ("Fortify", "Fortify"),
    ("Onslaught", "Onslaught"),
    ("UnholyMight", "Unholy Might"),
    ("ChaoticMight", "Chaotic Might"),
    ("Tailwind", "Tailwind"),
    ("Adrenaline", "Adrenaline"),
    ("AlchemistsGenius", "Alchemist's Genius"),
    ("HerEmbrace", "Her Embrace"),
    ("AccelerationShrine", "Acceleration Shrine"),
    ("BrutalShrine", "Brutal Shrine"),
    ("DiamondShrine", "Diamond Shrine"),
    ("DivineShrine", "Divine Shrine"),
    ("EchoingShrine", "Echoing Shrine"),
    ("GloomShrine", "Gloom Shrine"),
    ("GreaterFreezingShrine", "Greater Freezing Shrine"),
    ("GreaterShockingShrine", "Greater Shocking Shrine"),
    ("GreaterSkeletalShrine", "Greater Skeletal Shrine"),
    ("ImpenetrableShrine", "Impenetrable Shrine"),
    ("MassiveShrine", "Massive Shrine"),
    ("ReplenishingShrine", "Replenishing Shrine"),
    ("ResistanceShrine", "Resistance Shrine"),
    ("ResonatingShrine", "Resonating Shrine"),
    ("LesserAccelerationShrine", "Lesser Acceleration Shrine"),
    ("LesserBrutalShrine", "Lesser Brutal Shrine"),
    ("LesserImpenetrableShrine", "Lesser Impenetrable Shrine"),
];
/// L728-753, continuing [`PLAYER_COMBAT_FLAGS`].
const PLAYER_COMBAT_FLAGS_2: [(&str, &str); 8] = [
    ("LesserMassiveShrine", "Lesser Massive Shrine"),
    ("LesserReplenishingShrine", "Lesser Replenishing Shrine"),
    ("LesserResistanceShrine", "Lesser Resistance Shrine"),
    ("BloodShrineOfRats", "Blood Shrine of Rats"),
    ("BloodShrineOfLocusts", "Blood Shrine of Locusts"),
    ("BloodShrineOfToads", "Blood Shrine of Toads"),
    ("BloodShrineOfCrows", "Blood Shrine of Crows"),
    ("BloodShrineOfBats", "Blood Shrine of Bats"),
];
/// The minion modDB flags of L823-843, in Lua order.
const MINION_COMBAT_FLAGS: [(&str, &str); 7] = [
    ("Fortify", "Fortify"),
    ("Onslaught", "Onslaught"),
    ("UnholyMight", "Unholy Might"),
    ("ChaoticMight", "Chaotic Might"),
    ("Tailwind", "Tailwind"),
    ("DiamondShrine", "Diamond Shrine"),
    ("MassiveShrine", "Massive Shrine"),
];

/// `build.calcsTab.mainEnv.multipliersUsed[var]` (a nil mainEnv / multipliersUsed raises).
fn mainEnv_multiplierUsed(build: &BuildRef, var: Name) -> bool {
    let mainEnv = build
        .calcsTab
        .mainEnv
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainEnv' (a nil value)"));
    mainEnv
        .multipliersUsed
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'multipliersUsed' (a nil value)"))
        .truthy(var)
}

/// Calcs.lua:602-866 (body of [`buildOutput`]): CALCS mode's buff, combat and curse lists.
pub fn buildOutput_calcs(build: &BuildRef, env: &EnvRef, output: &Output) {
    let modDB = env.modDB.get();
    // L602
    let mut buffList: Vec<String> = Vec::new();
    // L603
    let mut combatList: Vec<String> = Vec::new();
    // L604
    let mut curseList: Vec<String> = Vec::new();
    // L605-634
    charges(&mut combatList, output, n!("PowerCharges"), "%d Power Charges");
    charges(&mut combatList, output, n!("AbsorptionCharges"), "%d Absorption Charges");
    charges(&mut combatList, output, n!("FrenzyCharges"), "%d Frenzy Charges");
    charges(&mut combatList, output, n!("AfflictionCharges"), "%d Affliction Charges");
    charges(&mut combatList, output, n!("EnduranceCharges"), "%d Endurance Charges");
    charges(&mut combatList, output, n!("BrutalCharges"), "%d Brutal Charges");
    charges(&mut combatList, output, n!("BrineCharges"), "%d Brine Charges");
    charges(&mut combatList, output, n!("SiphoningCharges"), "%d Siphoning Charges");
    charges(&mut combatList, output, n!("ChallengerCharges"), "%d Challenger Charges");
    charges(&mut combatList, output, n!("BlitzCharges"), "%d Blitz Charges");
    // L635
    if mainEnv_multiplierUsed(build, n!("InspirationCharge")) {
        // L636 (`%d` of a nil raises)
        combatList.push(lfmt!("%d Inspiration Charges", output.num(n!("InspirationCharges"))));
    }
    // L638
    charges(&mut combatList, output, n!("GhostShrouds"), "%d Ghost Shrouds");
    // L641
    charges(&mut combatList, output, n!("CrabBarriers"), "%d Crab Barriers");
    // L644
    if mainEnv_multiplierUsed(build, n!("BloodCharge")) {
        // L645
        combatList.push(lfmt!("%d Blood Charges", output.num(n!("BloodCharges"))));
    }
    // L647
    if mainEnv_multiplierUsed(build, n!("SpiritCharge")) {
        // L648
        combatList.push(lfmt!("%d Spirit Charges", output.num(n!("SpiritCharges"))));
    }
    // L650
    if mainEnv_multiplierUsed(build, n!("SpiritInfusion")) {
        // L651
        combatList.push(lfmt!("%d Spirit Infusions", output.num(n!("SpiritInfusions"))));
    }
    // L653
    let mainSkill = env
        .player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
    if mainSkill.baseSkillModList.get().Flag(None, nm!["Cruelty"]) {
        // L654
        combatList.push("Cruelty".to_owned());
    }
    // L656-753
    for (flag, label) in PLAYER_COMBAT_FLAGS.iter().chain(PLAYER_COMBAT_FLAGS_2.iter()) {
        if modDB.Flag(None, &[Name::from(*flag)]) {
            combatList.push((*label).to_owned());
        }
    }
    // L755 LUA-ORDER: pairs; sorted at L761
    for (name, _) in env.buffs.get().pairs() {
        // L756
        let Some(name) = name.name() else {
            lua_error!("attempt to compare number with string");
        };
        buffList.push(name.to_string());
    }
    // L758
    if modDB.Flag(None, nm!["Elusive"]) {
        // L759
        combatList.push("Elusive".to_owned());
    }
    // L761
    pob_lua::sort::sort_default(&mut buffList);
    // L762
    let playerBreakdown = breakdown_of(&env.player);
    let skillBuffs = tbl! { modList = Table::new() };
    playerBreakdown.set(n!("SkillBuffs"), skillBuffs.clone());
    let skillBuffsModList = skillBuffs.table(n!("modList")).unwrap_or_default();
    // L763
    for name in &buffList {
        // L764
        for m in mods_of(&env.buffs.get().get(Name::from(name.as_str()))) {
            // L765
            let value = modDB.EvalMod(&m, None, None);
            // L766
            if nonzero(&value) {
                // L767
                skillBuffsModList.push(mod_row(&m, &value));
            }
        }
    }
    // L774
    let skillDebuffs = tbl! { modList = Table::new() };
    playerBreakdown.set(n!("SkillDebuffs"), skillDebuffs.clone());
    let skillDebuffsModList = skillDebuffs.table(n!("modList")).unwrap_or_default();
    // L775 LUA-ORDER: pairs; sorted at L778
    for (name, _modList) in env.debuffs.get().pairs() {
        // L776
        let Some(name) = name.name() else {
            lua_error!("attempt to compare number with string");
        };
        curseList.push(name.to_string());
    }
    // L778
    pob_lua::sort::sort_default(&mut curseList);
    let enemyModDB = env.enemy.modDB.get();
    // L779
    for index in 0..curseList.len() {
        let name = curseList[index].clone();
        let debuff = env.debuffs.get().get(Name::from(name.as_str()));
        // L780
        for m in mods_of(&debuff) {
            // L781
            let value = enemyModDB.EvalMod(&m, None, None);
            // L782
            if nonzero(&value) {
                // L783
                skillDebuffsModList.push(mod_row(&m, &value));
            }
        }
        // L789
        let debuffList = debuff
            .obj::<ModStore>()
            .unwrap_or_else(|| lua_error!("attempt to call method 'Sum' (a nil value)"));
        let stackCount = debuffList.Sum(
            ModType::BASE,
            None,
            &[cat!("Multiplier:", name.as_str(), "Stack")],
        );
        // L790
        if stackCount > 0.0 {
            // L791
            curseList[index] = format!(
                "{name} ({} stack{})",
                tostring_num(stackCount),
                if stackCount > 1.0 { "s" } else { "" }
            );
        }
    }
    // L794
    for_ipairs!((_i, slot) in env.curseSlots.get() => {
        let Val::Table(slot) = slot else {
            lua_error!("attempt to index local 'slot' (a {} value)", slot.lua_type());
        };
        // L795
        match slot.get(n!("name")) {
            Val::Str(s) => curseList.push(s.to_string()),
            Val::Num(x) => curseList.push(tostring_num(x)),
            // `t_insert(curseList, nil)` appends nothing
            Val::Nil => {}
            // any other value makes `table.concat` (L810) raise
            other => lua_error!("invalid value (at index {}) in table for 'concat' ({})", curseList.len() + 1, other.lua_type()),
        }
        // L796
        let slotModList = slot.get(n!("modList"));
        if slotModList.truthy() {
            // L797
            for m in mods_of(&slotModList) {
                // L798
                let value = enemyModDB.EvalMod(&m, None, None);
                // L799
                if nonzero(&value) {
                    // L800
                    skillDebuffsModList.push(mod_row(&m, &value));
                }
            }
        }
    });
    // L808
    output.set(n!("BuffList"), buffList.join(", "));
    // L809
    output.set(n!("CombatList"), combatList.join(", "));
    // L810
    output.set(n!("CurseList"), curseList.join(", "));
    // L811
    if let Some(minion) = env.minion.get() {
        let minionOutput = output
            .table(n!("Minion"))
            .unwrap_or_else(|| lua_error!("attempt to index field 'Minion' (a nil value)"));
        let minionModDB = minion.modDB.get();
        // L812
        let mut buffList: Vec<String> = Vec::new();
        // L813
        let mut combatList: Vec<String> = Vec::new();
        // L814-822
        charges(&mut combatList, &minionOutput, n!("PowerCharges"), "%d Power Charges");
        charges(&mut combatList, &minionOutput, n!("FrenzyCharges"), "%d Frenzy Charges");
        charges(&mut combatList, &minionOutput, n!("EnduranceCharges"), "%d Endurance Charges");
        // L823-843
        for (flag, label) in MINION_COMBAT_FLAGS {
            if minionModDB.Flag(None, &[Name::from(flag)]) {
                combatList.push(label.to_owned());
            }
        }
        // L844 LUA-ORDER: pairs; sorted at L847
        for (name, _) in env.minionBuffs.get().pairs() {
            // L845
            let Some(name) = name.name() else {
                lua_error!("attempt to compare number with string");
            };
            buffList.push(name.to_string());
        }
        // L847
        pob_lua::sort::sort_default(&mut buffList);
        // L848
        let minionBreakdown = breakdown_of(&minion);
        let minionSkillBuffs = tbl! { modList = Table::new() };
        minionBreakdown.set(n!("SkillBuffs"), minionSkillBuffs.clone());
        let minionSkillBuffsModList = minionSkillBuffs.table(n!("modList")).unwrap_or_default();
        // L849
        for name in &buffList {
            // L850
            for m in mods_of(&env.minionBuffs.get().get(Name::from(name.as_str()))) {
                // L851
                let value = minionModDB.EvalMod(&m, None, None);
                // L852
                if nonzero(&value) {
                    // L853
                    minionSkillBuffsModList.push(mod_row(&m, &value));
                }
            }
        }
        // L860
        minionBreakdown.set(n!("SkillDebuffs"), playerBreakdown.get(n!("SkillDebuffs")));
        // L861
        minionOutput.set(n!("BuffList"), buffList.join(", "));
        // L862
        minionOutput.set(n!("CombatList"), combatList.join(", "));
        // L863
        minionOutput.set(n!("CurseList"), output.get(n!("CurseList")));
    }

    // L866 -- infoDump(env)
}

// @pob Modules/Calcs.lua:872-872 h=ee9dedbd
// (module-level statements: Return)
