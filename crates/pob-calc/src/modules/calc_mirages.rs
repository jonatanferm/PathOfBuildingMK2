//! @pob-file Modules/CalcMirages.lua @16de4b82d
//! CalcMirages.lua 1-427: mirage skills (the `MirageConfig` type is U0's, §8.1).
//! Handles mirages that use player skills.
//!
//! Owner: MI.
//!
//! The configs are built inline in `mirages`, as Lua builds them, with the closures' shared
//! upvalues (`usedSkillBestDps`, `maxMirageWarriors`, the Tawhoa cooldown locals) in `Rc<Cell>`s.
//!
//! Callees that are other units' and not ported at MI's commit (`copyActiveSkill` → `initEnv`
//! CS2/CS3, `buildActiveSkill` CA, `isTriggered` / `calcMultiSpellRotationImpact` TR-A) and the
//! nested `perform` go through [`replay_hook`] (feature `probe` only; CCR-0077 §1 pattern), so
//! the L4 test (`crates/pob-oracle/tests/MI.rs`) can replay them.
#![allow(
    non_snake_case,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::assign_op_pattern
)]

use std::cell::RefCell;

use pob_gamedata::mod_tools::createMod;

use crate::calcs_facade::calcs;
use crate::classes::mod_store::arith;
use crate::modules::calc_tools as calcLib;
use crate::modules::common::cacheSkillUUID;
use crate::prelude::*;

/// The config each mirage family builds inline (CalcMirages.lua 60-427), with the closures as boxed
/// Rust closures (calc doc §8.1).
pub struct MirageConfig {
    pub calcMainSkillOffence: bool,
    pub mirageSkill: Option<ActiveSkillRef>,
    pub compareFunc: Option<
        Box<
            dyn Fn(
                &ActiveSkillRef,
                &EnvRef,
                &MirageConfig,
                Option<ActiveSkillRef>,
            ) -> Option<ActiveSkillRef>,
        >,
    >,
    pub preCalcFunc: Box<dyn Fn(&EnvRef, &ActiveSkillRef, &EnvRef)>,
    pub postCalcFunc: Box<dyn Fn(&EnvRef, &ActiveSkillRef, &EnvRef)>,
    pub mirageSkillNotFoundFunc: Box<dyn Fn(&EnvRef, &MirageConfig)>,
}

// ---- callee replay seam (test-only, feature `probe`; CCR-0077 §1, CCR-0090) ----------------------

/// The L4 test replaces the callees of this module that cannot run on the injected state (other
/// units' stubs, or a nested calc whose env a dump does not carry) by recorded results. The
/// shipped calc has no hook: without `probe` every call goes straight to the callee.
#[cfg(feature = "probe")]
pub mod replay_hook {
    use std::cell::RefCell;

    use crate::prelude::*;

    /// One callee call of `calcs.mirages` / `calculateMirage`.
    pub enum Call<'a> {
        /// `calcs.copyActiveSkill(env, mode, skill)` (CalcMirages.lua:40).
        copyActiveSkill {
            env: &'a EnvRef,
            mode: Mode,
            skill: &'a ActiveSkillRef,
        },
        /// `calcs.perform(newEnv)` (CalcMirages.lua:52).
        perform { env: &'a EnvRef },
        /// `calcs.buildActiveSkill(env, mode, skill, targetUUID, limitedProcessingFlags)`
        /// (CalcMirages.lua:128, 188, 362).
        buildActiveSkill {
            env: &'a EnvRef,
            mode: Mode,
            skill: &'a ActiveSkillRef,
            targetUUID: Option<Name>,
            limitedProcessingFlags: &'a [Name],
        },
        /// `isTriggered(skill)` (CalcMirages.lua:185).
        isTriggered { skill: &'a ActiveSkillRef },
        /// `calcMultiSpellRotationImpact(env, skillRotation, sourceRate, triggerCD)`
        /// (CalcMirages.lua:217).
        calcMultiSpellRotationImpact {
            env: &'a EnvRef,
            skillRotation: &'a Table,
            sourceRate: f64,
            triggerCD: f64,
        },
        /// Not a callee: General's Cry's `cooldown` before the `m_max` (CalcMirages.lua:376, the
        /// `calcSkillCooldown` result of 369 or the initial 1), so the L4 test can check it
        /// against PoB's even where the cached attack time dominates. The hook answers `None`.
        generalsCryCooldown { cooldown: f64 },
    }

    /// A replayed call's result.
    pub enum Ret {
        /// `copyActiveSkill` → (newSkill, newEnv).
        Copy(ActiveSkillRef, EnvRef),
        /// `perform` / `buildActiveSkill` (no results).
        Unit,
        /// `isTriggered` (the result's truthiness).
        Bool(bool),
        /// `calcMultiSpellRotationImpact` → (mainRate, trigRateTable).
        Rate(f64, Table),
    }

    /// `Some(ret)` when the hook replayed the call, `None` to run the callee.
    pub type Hook = Box<dyn FnMut(&Call) -> Option<Ret>>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Install (or with `None` remove) the hook; returns the previous one.
    pub fn set(h: Option<Hook>) -> Option<Hook> {
        HOOK.with(|c| c.replace(h))
    }

    pub(super) fn try_replay(call: &Call) -> Option<Ret> {
        let mut h = HOOK.with(|c| c.borrow_mut().take())?;
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

/// `calcs.copyActiveSkill(env, mode, skill)`, or its replay.
fn call_copyActiveSkill(
    env: &EnvRef,
    mode: Mode,
    skill: &ActiveSkillRef,
) -> (ActiveSkillRef, EnvRef) {
    #[cfg(feature = "probe")]
    {
        use replay_hook::{Call, Ret};
        match replay_hook::try_replay(&Call::copyActiveSkill { env, mode, skill }) {
            Some(Ret::Copy(s, e)) => return (s, e),
            Some(_) => lua_error!("MI replay: copyActiveSkill replayed with the wrong result"),
            None => {}
        }
    }
    calcs::copyActiveSkill(env, mode, skill)
}

/// `calcs.perform(env)` (skipEHP nil), or its replay.
fn call_perform(env: &EnvRef) {
    #[cfg(feature = "probe")]
    {
        use replay_hook::Call;
        if replay_hook::try_replay(&Call::perform { env }).is_some() {
            return;
        }
    }
    calcs::perform(env, false);
}

/// `calcs.buildActiveSkill(env, mode, skill, targetUUID, limitedProcessingFlags)`, or its replay.
fn call_buildActiveSkill(
    env: &EnvRef,
    mode: Mode,
    skill: &ActiveSkillRef,
    targetUUID: Option<Name>,
    limitedProcessingFlags: &[Name],
) {
    #[cfg(feature = "probe")]
    {
        use replay_hook::Call;
        let call = Call::buildActiveSkill {
            env,
            mode,
            skill,
            targetUUID,
            limitedProcessingFlags,
        };
        if replay_hook::try_replay(&call).is_some() {
            return;
        }
    }
    calcs::buildActiveSkill(env, mode, skill, targetUUID, limitedProcessingFlags);
}

/// Hand General's Cry's pre-`m_max` `cooldown` to the L4 test (see
/// `replay_hook::Call::generalsCryCooldown`); a no-op without `probe`.
fn observe_generalsCryCooldown(cooldown: f64) {
    #[cfg(feature = "probe")]
    {
        let _ = replay_hook::try_replay(&replay_hook::Call::generalsCryCooldown { cooldown });
    }
    #[cfg(not(feature = "probe"))]
    let _ = cooldown;
}

/// `isTriggered(skill)` (its truthiness), or its replay.
fn call_isTriggered(skill: &ActiveSkillRef) -> bool {
    #[cfg(feature = "probe")]
    {
        use replay_hook::{Call, Ret};
        match replay_hook::try_replay(&Call::isTriggered { skill }) {
            Some(Ret::Bool(b)) => return b,
            Some(_) => lua_error!("MI replay: isTriggered replayed with the wrong result"),
            None => {}
        }
    }
    crate::calcs_facade::isTriggered(skill)
}

/// `calcMultiSpellRotationImpact(env, skillRotation, sourceRate, triggerCD)` (chance and actor
/// nil: the callee uses `chance or 100` and `actor or env.player`), or its replay.
fn call_calcMultiSpellRotationImpact(
    env: &EnvRef,
    skillRotation: &Table,
    sourceRate: f64,
    triggerCD: f64,
) -> (f64, Table) {
    #[cfg(feature = "probe")]
    {
        use replay_hook::{Call, Ret};
        let call = Call::calcMultiSpellRotationImpact {
            env,
            skillRotation,
            sourceRate,
            triggerCD,
        };
        match replay_hook::try_replay(&call) {
            Some(Ret::Rate(r, t)) => return (r, t),
            Some(_) => {
                lua_error!("MI replay: calcMultiSpellRotationImpact replayed with the wrong result")
            }
            None => {}
        }
    }
    // `actor or env.player` with actor nil
    crate::calcs_facade::calcMultiSpellRotationImpact(
        env,
        skillRotation,
        sourceRate,
        triggerCD,
        None,
        &env.player,
    )
}

// ---- Lua value helpers (plumbing) ----------------------------------------------------------------

/// `skill.skillCfg.skillCond` (indexing nil raises).
fn skillCond(skill: &ActiveSkill) -> Table {
    skill
        .skillCfg
        .get()
        .skillCond
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'skillCond' (a nil value)"))
}

/// `env.player.mainSkill` (indexing nil raises).
fn mainSkill(env: &Env) -> ActiveSkillRef {
    env.player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `skill.skillTypes[t]`.
fn has_type(skill: &ActiveSkill, t: SkillType::T) -> bool {
    skill.skillTypes.get().with(|s| s.has(t))
}

/// `skill.skillTypes[t] = true`.
fn set_type(skill: &ActiveSkill, t: SkillType::T) {
    skill.skillTypes.get().with_mut(|s| s.set(t, true));
}

/// `skill.skillTypes[SkillType.Totem]`: `SkillType.Totem` is nil at the pin (LUA-QUIRK; see
/// docs/port/known_diffs.toml, ModParser.lua:5509), and reading `t[nil]` gives nil.
const SKILLTYPE_TOTEM_NIL: bool = false;

/// `skill.activeEffect.grantedEffect.name`.
fn grantedEffectName(skill: &ActiveSkill) -> Name {
    skill.activeEffect.get().grantedEffect.get().name
}

/// Lua `a > b` (numbers only here; anything else raises like Lua).
fn gt(a: &Val, b: &Val) -> bool {
    match (a, b) {
        (Val::Num(x), Val::Num(y)) => x > y,
        _ => lua_error!(
            "attempt to compare {} with {}",
            b.lua_type(),
            a.lua_type()
        ),
    }
}

/// Lua `x ~= 0`.
fn ne0(v: &Val) -> bool {
    !matches!(v, Val::Num(x) if *x == 0.0)
}

/// `GlobalCache.cachedData[mode][uuid]` that Lua then indexes (nil raises).
fn cached(mode: Mode, uuid: Name) -> Rc<CachedData> {
    cachedData(mode, uuid).unwrap_or_else(|| {
        lua_error!("attempt to index field '?' (a nil value)")
    })
}

/// `skillModList:NewMod(name, mod.type, mod.value, mod.source, mod.flags, mod.keywordFlags)` with
/// the fields of a tabulated mod (no tags are passed; a nil source stays nil, as in createMod).
fn newModFrom(store: &ModStore, name: Name, m: &Mod) {
    store.AddMod(Arc::new(createMod(
        name,
        m.r#type,
        m.value.clone(),
        m.source.as_deref(),
        m.flags,
        m.keywordFlags,
        &[],
    )));
}

/// `mirage.minion = { }; mirage.minion.output = newEnv.minion.output` (newEnv.minion nil raises).
fn newEnvMinion(newEnv: &Env) -> ActorRef {
    newEnv
        .minion
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'minion' (a nil value)"))
}

/// A breakdown stored in a plain table (`mirage.breakdown = newEnv.player.breakdown`).
fn breakdownVal(b: Option<Breakdown>) -> Val {
    match b {
        Some(b) => Val::Obj(Rc::new(b)),
        None => Val::Nil,
    }
}

// @pob Modules/CalcMirages.lua:8-22 h=31864e9a
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_min,m_max,m_ceil,m_floor,m_modf,s_format,m_huge,bor,band)

/// @pob Modules/CalcMirages.lua:24-58 h=83929a90
/// → `not config.calcMainSkillOffence` (nil when no mirage skill).
///
/// `config` is an `Option` (Lua's `if not config then return end`: `calcs.mirages` passes nil when
/// no family matched and on the General's Cry path). The function is file-local (no other caller);
/// U0's stub took `&MirageConfig` (CCR-0090 §2).
pub(crate) fn calculateMirage(env: &EnvRef, config: Option<&MirageConfig>) -> Option<bool> {
    // L25
    let Some(config) = config else {
        // L26
        return None;
    };

    // L29
    let mut mirageSkill = config.mirageSkill.clone();

    // L31
    if let Some(compareFunc) = &config.compareFunc {
        // L32
        let list: Vec<ActiveSkillRef> = env.player.activeSkillList.get().with(|v| v.clone());
        for skill in &list {
            // L33
            if !skillCond(skill).truthy(n!("usedByMirage")) {
                // L34
                mirageSkill = compareFunc(skill, env, config, mirageSkill);
            }
        }
    }

    // L39
    if let Some(mirageSkill) = mirageSkill {
        // L40
        let (newSkill, newEnv) = call_copyActiveSkill(env, Mode::CALCULATOR, &mirageSkill);
        // L41
        skillCond(&newSkill).set(n!("usedByMirage"), true);
        // L42
        newSkill.skillFlags.get().set(n!("multiPart"), Val::Nil);
        // L43
        newSkill.skillFlags.get().set(n!("haveMinion"), Val::Nil);
        // L44
        if newEnv.limitedSkills.get().is_none() {
            newEnv.limitedSkills.set(Some(Table::new()));
        }
        // L45
        let limitedSkills = newEnv.limitedSkills.get().unwrap_or_default();
        limitedSkills.set(cacheSkillUUID(&newSkill, &newEnv), true);
        // L46
        let storedUses = mainSkill(env).skillData.get().get(n!("storedUses"));
        newSkill.skillData.get().set(n!("mirageUses"), storedUses);
        // L47
        set_type(&newSkill, SkillType::OtherThingUsesSkill);

        // L49
        (config.preCalcFunc)(env, &newSkill, &newEnv);

        // L51
        newEnv.player.mainSkill.set(Some(newSkill.clone()));
        // L52
        call_perform(&newEnv);
        // L53
        (config.postCalcFunc)(env, &newSkill, &newEnv);
    } else {
        // L55
        (config.mirageSkillNotFoundFunc)(env, config);
    }
    // L57
    Some(!config.calcMainSkillOffence)
}

/// @pob Modules/CalcMirages.lua:60-428 h=e1da88f5
/// The result's truthiness (`nil` → false): perform skips the main skill's offence when it is
/// true (CalcPerform.lua:3798). U0's stub returned `()` (CCR-0077 §2, CCR-0090 §1).
pub fn mirages(env: &EnvRef) -> bool {
    // L61
    let config: Option<MirageConfig>;

    // L63
    {
        let ms = mainSkill(env);
        if skillCond(&ms).truthy(n!("usedByMirage")) || ms.skillFlags.get().truthy(n!("disable")) {
            // L64
            return false;
        }
    }

    let ms = mainSkill(env);
    // L67
    if ms.skillData.get().truthy(n!("triggeredByMirageArcher")) {
        // L68
        config = Some(MirageConfig {
            // L69
            calcMainSkillOffence: true,
            // L70
            mirageSkill: Some(ms.clone()),
            compareFunc: None,
            // L71
            preCalcFunc: Box::new(|env, newSkill, _newEnv| {
                let ms = mainSkill(env);
                // L72
                let moreDamage = newSkill.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&newSkill.skillCfg.get()),
                    nm!["MirageArcherLessDamage"],
                );
                // L73
                let moreAttackSpeed = newSkill.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&newSkill.skillCfg.get()),
                    nm!["MirageArcherLessAttackSpeed"],
                );
                // L74
                let mirageCount = newSkill.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&ms.skillCfg.get()),
                    nm!["MirageArcherMaxCount"],
                );

                // L76
                ms.mirage.set(Some(Table::new()));
                let mirage = ms.mirage.get().unwrap_or_default();
                // L77
                mirage.set(n!("name"), grantedEffectName(newSkill));
                // L78
                mirage.set(n!("count"), mirageCount);

                // L80
                if ms.infoMessage.get().is_none() {
                    // L81
                    let msg = cat!(
                        tostring_num(mirageCount).as_str(),
                        " Mirage Archers using ",
                        grantedEffectName(newSkill).as_str()
                    );
                    ms.infoMessage.set(Some(LStr::from(msg.as_str())));
                }

                // L84 Add new modifiers to new skill (which already has all the old skill's modifiers)
                // L85 (`env.player.mainSkill.ModFlags` / `.KeywordFlags` are nil fields: flags 0)
                newSkill.skillModList.get().NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    moreDamage,
                    "Mirage Archer",
                    0,
                    0,
                    &[],
                );
                // L86
                newSkill.skillModList.get().NewMod(
                    n!("Speed"),
                    ModType::MORE,
                    moreAttackSpeed,
                    "Mirage Archer",
                    0,
                    0,
                    &[],
                );

                // L88 Does not use player resources
                // L89
                newSkill.skillModList.get().NewMod(
                    n!("HasNoCost"),
                    ModType::FLAG,
                    true,
                    "Used by mirage",
                    0,
                    0,
                    &[],
                );

                // L91
                if newSkill.skillPartName.get().is_some() {
                    // L92
                    mirage.set(n!("skillPart"), newSkill.skillPart.get());
                    // L93
                    mirage.set(
                        n!("skillPartName"),
                        newSkill.skillPartName.get().map_or(Val::Nil, Val::Str),
                    );
                    // L94
                    mirage.set(n!("infoMessage2"), grantedEffectName(newSkill));
                } else {
                    // L96
                    mirage.set(n!("skillPartName"), Val::Nil);
                }
            }),
            // L99
            postCalcFunc: Box::new(|env, newSkill, newEnv| {
                let ms = mainSkill(env);
                let mirage = ms
                    .mirage
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'mirage' (a nil value)"));
                // L100
                mirage.set(n!("output"), newEnv.player.output.get());
                // L101
                ms.skillFlags.get().set(n!("mirageArcher"), true);
                // L102
                if newSkill.minion.get().is_some() {
                    // L103
                    let t = Table::new();
                    mirage.set(n!("minion"), t.clone());
                    // L104
                    t.set(n!("output"), newEnvMinion(newEnv).output.get());
                }

                // L107
                if let Some(b) = newEnv.player.breakdown.get() {
                    // L108
                    mirage.set(n!("breakdown"), breakdownVal(Some(b)));
                    // L109
                    if newSkill.minion.get().is_some() {
                        // L110
                        let minion = mirage.table(n!("minion")).unwrap_or_else(|| {
                            lua_error!("attempt to index field 'minion' (a nil value)")
                        });
                        minion.set(
                            n!("breakdown"),
                            breakdownVal(newEnvMinion(newEnv).breakdown.get()),
                        );
                    }
                }
            }),
            // L114
            mirageSkillNotFoundFunc: Box::new(|env, _config| {
                let ms = mainSkill(env);
                // L115
                if ms.infoMessage2.get().is_none() {
                    // L116
                    ms.infoMessage2
                        .set(Some(LStr::from("No Mirage Archer active skill found")));
                }
            }),
        });
    } else if grantedEffectName(&ms) == n!("Reflection") {
        // L120
        // L121
        let usedSkillBestDps: Rc<RefCell<Val>> = Rc::new(RefCell::new(Val::Nil));
        // L122
        let maxMirageWarriors = Rc::new(Cell::new(ms.skillModList.get().Sum(
            ModType::BASE,
            Some(&ms.skillCfg.get()),
            nm!["SaviourMirageWarriorMaxCount"],
        )));
        let mmw_pre = maxMirageWarriors.clone();
        let mmw_post = maxMirageWarriors.clone();
        // L123
        config = Some(MirageConfig {
            calcMainSkillOffence: false,
            mirageSkill: None,
            // L124
            compareFunc: Some(Box::new(move |skill, env, _config, mirageSkill| {
                let ms = mainSkill(env);
                let swordOneHand = ModFlag::Sword | ModFlag::Weapon1H;
                // L125
                if !Rc::ptr_eq(skill, &ms)
                    && has_type(skill, SkillType::Attack)
                    // LUA-QUIRK: `SkillType.Totem` does not exist (Data/Global.lua defines
                    // SummonsTotem), so `skill.skillTypes[nil]` is nil and `not …` is true
                    && !SKILLTYPE_TOTEM_NIL
                    && !has_type(skill, SkillType::SummonsTotem)
                    && skill.skillCfg.get().flags.get() & swordOneHand == swordOneHand
                    && !skillCond(skill).truthy(n!("usedByMirage"))
                {
                    // L126
                    let uuid = cacheSkillUUID(skill, env);
                    // L127
                    if cachedData(env.mode, uuid).is_none() {
                        // L128
                        call_buildActiveSkill(env, env.mode, skill, Some(uuid), &[]);
                    }

                    // L131
                    if let Some(c) = cachedData(env.mode, uuid) {
                        if c.CritChance.truthy() && gt(&c.CritChance, &Val::Num(0.0)) {
                            // L132
                            if mirageSkill.is_none() {
                                // L133
                                *usedSkillBestDps.borrow_mut() = c.TotalDPS.clone();
                                // L134
                                return Some(c.ActiveSkill.clone());
                            // L135
                            } else if gt(&c.TotalDPS, &usedSkillBestDps.borrow().clone()) {
                                // L136
                                *usedSkillBestDps.borrow_mut() = c.TotalDPS.clone();
                                // L137
                                return Some(c.ActiveSkill.clone());
                            }
                        }
                    }
                }
                // L141
                mirageSkill
            })),
            // L143
            preCalcFunc: Box::new(move |env, newSkill, _newEnv| {
                let ms = mainSkill(env);
                // L144
                let moreDamage = ms.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&ms.skillCfg.get()),
                    nm!["SaviourMirageWarriorLessDamage"],
                );
                // L145 Add new modifiers to new skill (which already has all the old skill's modifiers)
                // L146
                newSkill.skillModList.get().NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    moreDamage,
                    "The Saviour",
                    0,
                    0,
                    &[],
                );
                // L147
                let itemList = &env.player.itemList;
                let w1 = itemList.with(|l| l.get(&n!("Weapon 1")).cloned());
                let w2 = itemList.with(|l| l.get(&n!("Weapon 2")).cloned());
                if let (Some(w1), Some(w2)) = (w1, w2) {
                    if w1.name.get() == w2.name.get() {
                        // L148
                        mmw_pre.set(mmw_pre.get() / 2.0);
                    }
                }
                // L150
                newSkill.skillModList.get().NewMod(
                    n!("QuantityMultiplier"),
                    ModType::BASE,
                    mmw_pre.get(),
                    "The Saviour Mirage Warriors",
                    0,
                    0,
                    &[],
                );
                // L151 Does not use player resources
                // L152
                newSkill.skillModList.get().NewMod(
                    n!("HasNoCost"),
                    ModType::FLAG,
                    true,
                    "Used by mirage",
                    0,
                    0,
                    &[],
                );
            }),
            // L154
            postCalcFunc: Box::new(move |env, newSkill, newEnv| {
                // L155
                env.player.mainSkill.set(Some(newSkill.clone()));
                // L156
                let msg = cat!(
                    tostring_num(mmw_post.get()).as_str(),
                    " Mirage Warriors using ",
                    grantedEffectName(newSkill).as_str()
                );
                mainSkill(env)
                    .infoMessage
                    .set(Some(LStr::from(msg.as_str())));
                // L157
                env.player.output.set(newEnv.player.output.get());
                // L158
                let b = newEnv.player.breakdown.get().or_else(|| env.player.breakdown.get());
                env.player.breakdown.set(b);
            }),
            // L160
            mirageSkillNotFoundFunc: Box::new(|env, _config| {
                let ms = mainSkill(env);
                // L161
                ms.disableReason
                    .set(Some(LStr::from("No Saviour active skill found")));
                // L162
                ms.skillFlags.get().set(n!("disable"), true);
            }),
        });
    } else if grantedEffectName(&ms) == n!("Tawhoa's Chosen") {
        // L165
        // L166-178: the closures' shared locals
        let st = Rc::new(TawhoaLocals::default());
        let st_cmp = st.clone();
        let st_pre = st.clone();
        let st_post = st.clone();

        // L180
        config = Some(MirageConfig {
            calcMainSkillOffence: false,
            mirageSkill: None,
            // L181
            compareFunc: Some(Box::new(move |skill, env, _config, mirageSkill| {
                let st = &st_cmp;
                let ms = mainSkill(env);
                // L182
                let isDisabled = skill.skillFlags.get().truthy(n!("disable"));
                // L183
                let skillTypeMatch = (has_type(skill, SkillType::Slam)
                    || has_type(skill, SkillType::Melee))
                    && has_type(skill, SkillType::Attack);
                // L184
                let skillTypeExcludes = has_type(skill, SkillType::Vaal)
                    // LUA-QUIRK: `skill.skillTypes[SkillType.Totem]` is `skillTypes[nil]` = nil
                    || SKILLTYPE_TOTEM_NIL
                    || has_type(skill, SkillType::SummonsTotem);
                // L185
                if !Rc::ptr_eq(skill, &ms)
                    && !call_isTriggered(skill)
                    && !isDisabled
                    && skillTypeMatch
                    && !skillTypeExcludes
                    && !skillCond(skill).truthy(n!("usedByMirage"))
                {
                    // L186
                    let uuid = cacheSkillUUID(skill, env);
                    // L187
                    if cachedData(env.mode, uuid).is_none() || env.mode == Mode::CALCULATOR {
                        // L188
                        call_buildActiveSkill(env, env.mode, skill, Some(uuid), &[]);
                    }

                    // L191
                    let take = mirageSkill.is_none()
                        || cachedData(env.mode, uuid).is_some_and(|c| {
                            gt(&c.TotalDPS, &st.usedSkillBestDps.borrow().clone())
                        });
                    if take {
                        let c = cached(env.mode, uuid);
                        // L192
                        *st.usedSkillBestDps.borrow_mut() = c.TotalDPS.clone();
                        // L193
                        *st.EffectiveSourceRate.borrow_mut() = c.Speed.clone();
                        // L194
                        return Some(c.ActiveSkill.clone());
                    }
                }
                // L197
                mirageSkill
            })),
            // L199
            preCalcFunc: Box::new(move |env, newSkill, _newEnv| {
                let st = &st_pre;
                let ms = mainSkill(env);
                let tickRate = data().misc.ServerTickRate;
                // L200
                let icdrSkill = calcLib::r#mod(
                    &newSkill.skillModList.get(),
                    Some(&newSkill.skillCfg.get()),
                    nm!["CooldownRecovery"],
                );
                st.icdrSkill.set(icdrSkill);

                // L202
                let triggeredCD = lor(newSkill.skillData.get().get(n!("cooldown")), || {
                    Val::Num(0.0)
                })
                .num();
                st.triggeredCD.set(triggeredCD);
                // L203
                let triggeredCDAdjusted = triggeredCD / icdrSkill;
                st.triggeredCDAdjusted.set(triggeredCDAdjusted);
                // L204
                let triggeredCDTickRounded = m_ceil(triggeredCDAdjusted * tickRate) / tickRate;

                // L206
                let triggerCD =
                    lor(ms.skillData.get().get(n!("cooldown")), || Val::Num(0.0)).num();
                st.triggerCD.set(triggerCD);
                // L207
                let triggerCDAdjusted = triggerCD / icdrSkill;
                st.triggerCDAdjusted.set(triggerCDAdjusted);
                // L208
                let triggerCDTickRounded = m_ceil(triggerCDAdjusted * tickRate) / tickRate;

                // L210 (`x or 0` of two numbers)
                let actionCooldown = m_max(triggeredCDTickRounded, triggerCDTickRounded);
                st.actionCooldown.set(actionCooldown);

                // L212
                let mut TriggerRateCap = m_huge;
                // L213
                if actionCooldown != 0.0 {
                    // L214
                    TriggerRateCap = 1.0 / actionCooldown;
                }
                st.TriggerRateCap.set(TriggerRateCap);

                // L217 (`a ~= 0 and f(...) or 0`: f's first result is a number, never falsy)
                let EffectiveSourceRate = st.EffectiveSourceRate.borrow().clone();
                let SkillTriggerRate = if ne0(&EffectiveSourceRate) {
                    let rotation = Table::new();
                    let entry = Table::new();
                    entry.set(n!("uuid"), cacheSkillUUID(&ms, env));
                    entry.set(n!("cd"), triggeredCD);
                    entry.set(n!("icdr"), icdrSkill);
                    rotation.push(entry);
                    call_calcMultiSpellRotationImpact(
                        env,
                        &rotation,
                        EffectiveSourceRate.num(),
                        triggerCD,
                    )
                    .0
                } else {
                    0.0
                };
                st.SkillTriggerRate.set(SkillTriggerRate);

                // L219 Override attack speed with trigger rate
                // L220
                newSkill
                    .skillData
                    .get()
                    .set(n!("triggerRate"), SkillTriggerRate);
                // L221
                newSkill.skillData.get().set(n!("triggered"), true);
                // L222
                newSkill.skillFlags.get().set(n!("triggered"), true);

                // L224 Does not use player resources
                // L225
                newSkill.skillModList.get().NewMod(
                    n!("HasNoCost"),
                    ModType::FLAG,
                    true,
                    "Used by Tawhoa's Chosen",
                    0,
                    0,
                    &[],
                );

                // L227
                let moreDamage = ms.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&ms.skillCfg.get()),
                    nm!["ChieftainMirageChieftainMoreDamage"],
                );
                // L228 Add new modifiers to new skill (which already has all the old skill's modifiers)
                // L229
                newSkill.skillModList.get().NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    moreDamage,
                    "Tawhoa's Chosen",
                    0,
                    0,
                    &[],
                );
            }),
            // L231
            postCalcFunc: Box::new(move |env, newSkill, newEnv| {
                let st = &st_post;
                // L232
                env.player.mainSkill.set(Some(newSkill.clone()));
                // L233
                let msg = cat!(
                    "Tawhoa's Chosen using ",
                    grantedEffectName(newSkill).as_str()
                );
                mainSkill(env)
                    .infoMessage
                    .set(Some(LStr::from(msg.as_str())));

                // L235
                env.player.output.set(newEnv.player.output.get());
                let output = env.player.output.get();
                // L236
                output.set(n!("Speed"), st.SkillTriggerRate.get());
                // L237
                output.set(n!("TriggerRateCap"), st.TriggerRateCap.get());
                // L238
                output.set(
                    n!("EffectiveSourceRate"),
                    st.EffectiveSourceRate.borrow().clone(),
                );
                // L239
                output.set(n!("SkillTriggerRate"), st.SkillTriggerRate.get());

                // L241
                if let Some(breakdown) = newEnv.player.breakdown.get() {
                    let triggeredCD = st.triggeredCD.get();
                    let icdrSkill = st.icdrSkill.get();
                    let triggeredCDAdjusted = st.triggeredCDAdjusted.get();
                    let triggerCD = st.triggerCD.get();
                    let triggerCDAdjusted = st.triggerCDAdjusted.get();
                    let actionCooldown = st.actionCooldown.get();
                    let TriggerRateCap = st.TriggerRateCap.get();
                    let lines: Vec<String> = if triggeredCD != 0.0 {
                        // L242-243
                        vec![
                            lfmt!("%.2f ^8(base cooldown of triggered skill)", triggeredCD),
                            lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrSkill),
                            lfmt!(
                                "= %.2f ^8(final cooldown of triggered skill)",
                                triggeredCDAdjusted
                            ),
                            String::new(),
                            lfmt!("%.2f ^8(Tawhoa's Chosen base cooldown)", triggerCD),
                            lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrSkill),
                            lfmt!("= %.2f ^8(effective trigger cooldown)", triggerCDAdjusted),
                            String::new(),
                            lfmt!(
                                "%.2f ^8(biggest of trigger cooldown and triggered skill cooldown)",
                                m_max(triggerCDAdjusted, triggeredCDAdjusted)
                            ),
                            String::new(),
                            lfmt!("%.2f ^8(adjusted for server tick rate)", actionCooldown),
                            String::new(),
                            "Trigger rate:".to_owned(),
                            lfmt!("1 / %.3f", actionCooldown),
                            lfmt!("= %.2f ^8per second", TriggerRateCap),
                        ]
                    } else {
                        // L261
                        vec![
                            "Triggered skill has no base cooldown".to_owned(),
                            String::new(),
                            lfmt!("%.2f ^8(Tawhoa's Chosen base cooldown)", triggerCD),
                            lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrSkill),
                            lfmt!("= %.2f ^8(effective trigger cooldown)", triggerCDAdjusted),
                            String::new(),
                            lfmt!("%.2f ^8(adjusted for server tick rate)", actionCooldown),
                            String::new(),
                            "Trigger rate:".to_owned(),
                            lfmt!("1 / %.3f", actionCooldown),
                            lfmt!("= %.2f ^8per second", TriggerRateCap),
                        ]
                    };
                    let t = Table::new();
                    for l in lines {
                        t.push(l);
                    }
                    breakdown.set(n!("TriggerRateCap"), t);

                    // L276
                    env.player.breakdown.set(newEnv.player.breakdown.get());
                }
            }),
            // L279
            mirageSkillNotFoundFunc: Box::new(|env, _config| {
                let ms = mainSkill(env);
                // L280
                ms.disableReason
                    .set(Some(LStr::from("No Tawhoa's Chosen active skill found")));
                // L281
                ms.skillFlags.get().set(n!("disable"), true);
            }),
        });
    } else if ms.skillData.get().truthy(n!("triggeredBySacredWisps")) {
        // L284
        // L285
        config = Some(MirageConfig {
            // L286
            calcMainSkillOffence: true,
            // L287
            mirageSkill: Some(ms.clone()),
            compareFunc: None,
            // L288
            preCalcFunc: Box::new(|env, newSkill, _newEnv| {
                let ms = mainSkill(env);
                // L289
                let lessDamage = newSkill.skillModList.get().Sum(
                    ModType::BASE,
                    Some(&ms.skillCfg.get()),
                    nm!["SacredWispsLessDamage"],
                );
                // L290
                let mut wispsMaxCount: Option<f64> = None;
                // L291
                let mut wispsCastChance: Option<f64> = None;

                // L293 Find Wisps summoning skill for cast chance and wisp count
                // L294
                let list: Vec<ActiveSkillRef> =
                    env.player.activeSkillList.get().with(|v| v.clone());
                for skill in &list {
                    // L295
                    if grantedEffectName(skill) == n!("Summon Sacred Wisps") {
                        // L296
                        wispsCastChance = Some(skill.skillModList.get().Sum(
                            ModType::BASE,
                            Some(&ms.skillCfg.get()),
                            nm!["SacredWispsChance"],
                        ));
                        // L297
                        wispsMaxCount = Some(skill.skillModList.get().Sum(
                            ModType::BASE,
                            Some(&ms.skillCfg.get()),
                            nm!["SacredWispsMaxCount"],
                        ));
                        // L298
                        break;
                    }
                }

                // L302
                ms.mirage.set(Some(Table::new()));
                let mirage = ms.mirage.get().unwrap_or_default();
                // L303
                mirage.set(n!("name"), grantedEffectName(newSkill));
                // L304
                mirage.set(n!("count"), wispsMaxCount);

                // L306
                if ms.infoMessage.get().is_none() {
                    // L307 (`tostring(nil)` is "nil")
                    let count = wispsMaxCount.map_or_else(|| "nil".to_owned(), tostring_num);
                    let msg = cat!(
                        count.as_str(),
                        " Sacred Wisps using ",
                        grantedEffectName(newSkill).as_str()
                    );
                    ms.infoMessage.set(Some(LStr::from(msg.as_str())));
                }

                // L310 Add new modifiers to new skill (which already has all the old skill's modifiers)
                // L311
                newSkill.skillModList.get().NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    lessDamage,
                    "Used by Sacred Wisps",
                    0,
                    0,
                    &[],
                );
                // L312
                let wispsCastChance = Val::from(wispsCastChance).num();
                newSkill.skillModList.get().NewMod(
                    n!("Speed"),
                    ModType::MORE,
                    wispsCastChance - 100.0,
                    "Sacred Wisps cast chance",
                    0,
                    0,
                    &[],
                );

                // L314 Does not use player resources
                // L315
                newSkill.skillModList.get().NewMod(
                    n!("HasNoCost"),
                    ModType::FLAG,
                    true,
                    "Used by Sacred Wisps",
                    0,
                    0,
                    &[],
                );

                // L317
                if newSkill.skillPartName.get().is_some() {
                    // L318
                    mirage.set(n!("skillPart"), newSkill.skillPart.get());
                    // L319
                    mirage.set(
                        n!("skillPartName"),
                        newSkill.skillPartName.get().map_or(Val::Nil, Val::Str),
                    );
                    // L320
                    mirage.set(n!("infoMessage2"), grantedEffectName(newSkill));
                } else {
                    // L322
                    mirage.set(n!("skillPartName"), Val::Nil);
                }
            }),
            // L325
            postCalcFunc: Box::new(|env, newSkill, newEnv| {
                let ms = mainSkill(env);
                let mirage = ms
                    .mirage
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'mirage' (a nil value)"));
                // L326
                mirage.set(n!("output"), newEnv.player.output.get());
                // L327
                ms.skillFlags.get().set(n!("wisp"), true);
                // L328
                if newSkill.minion.get().is_some() {
                    // L329
                    let t = Table::new();
                    mirage.set(n!("minion"), t.clone());
                    // L330
                    t.set(n!("output"), newEnvMinion(newEnv).output.get());
                }

                // L333
                if let Some(b) = newEnv.player.breakdown.get() {
                    // L334
                    mirage.set(n!("breakdown"), breakdownVal(Some(b)));
                    // L335
                    if newSkill.minion.get().is_some() {
                        // L336
                        let minion = mirage.table(n!("minion")).unwrap_or_else(|| {
                            lua_error!("attempt to index field 'minion' (a nil value)")
                        });
                        minion.set(
                            n!("breakdown"),
                            breakdownVal(newEnvMinion(newEnv).breakdown.get()),
                        );
                    }
                }
            }),
            // L340
            mirageSkillNotFoundFunc: Box::new(|env, _config| {
                let ms = mainSkill(env);
                // L341
                if ms.infoMessage2.get().is_none() {
                    // L342
                    ms.infoMessage2
                        .set(Some(LStr::from("No active skill for Sacred Wisps found")));
                }
            }),
        });
    } else if ms.skillData.get().truthy(n!("triggeredByGeneralsCry")) {
        // L346
        // L347
        let mut maxMirageWarriors = 0.0;
        // L348
        let mut cooldown = 1.0;
        // L349
        let mut generalsCryActiveSkill: Option<ActiveSkillRef> = None;
        // L350
        let uuid = cacheSkillUUID(&ms, env);

        // L352 Prevent infinite recursion
        // L353
        if env
            .limitedSkills
            .get()
            .is_some_and(|l| l.truthy(uuid))
        {
            // L354
            return false;
        }

        // L357
        set_type(&ms, SkillType::Triggered);
        // L358
        skillCond(&ms).set(n!("usedByMirage"), true);
        // L359
        set_type(&ms, SkillType::OtherThingUsesSkill);

        // L361
        if cachedData(env.mode, uuid).is_none() || env.mode == Mode::CALCULATOR {
            // L362
            call_buildActiveSkill(env, env.mode, &ms, Some(uuid), &[uuid]);
        }
        // L364
        let mainSkillOutputCache = cached(env.mode, uuid).Env.player.output.get();

        // L366 Find the active General's Cry gem to get active properties
        // L367
        let list: Vec<ActiveSkillRef> = env.player.activeSkillList.get().with(|v| v.clone());
        let groupSlot = |s: &ActiveSkill| {
            s.socketGroup
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'socketGroup' (a nil value)"))
                .slot
                .get()
        };
        for skill in &list {
            // L368
            if grantedEffectName(skill) == n!("General's Cry") && groupSlot(skill) == groupSlot(&ms)
            {
                // L369
                cooldown = crate::calcs_facade::calcSkillCooldown(
                    &skill.skillModList.get(),
                    Some(&skill.skillCfg.get()),
                    &skill.skillData.get(),
                )
                .0;
                // L370
                generalsCryActiveSkill = Some(skill.clone());
                // L371
                break;
            }
        }

        observe_generalsCryCooldown(cooldown);

        // L375 Scale dps with mirage quantity
        // L376
        let gc = generalsCryActiveSkill.unwrap_or_else(|| {
            lua_error!("attempt to index local 'generalsCryActiveSkill' (a nil value)")
        });
        let msModList = ms.skillModList.get();
        let msCfg = ms.skillCfg.get();
        for value in gc.skillModList.get().Tabulate(
            Some(ModType::BASE),
            Some(&gc.skillCfg.get()),
            nm!["GeneralsCryDoubleMaxCount"],
        ) {
            // L377
            let r#mod = &value.r#mod;
            // L378
            newModFrom(&msModList, n!("QuantityMultiplier"), r#mod);
            // L379
            maxMirageWarriors = maxMirageWarriors + arith(&r#mod.value, "mod.value");
        }

        // L382 Scale cooldown to have maximum number of Mirages at once. 0.3s for first mirage then 0.2s for each extra
        // L383
        let mut mirageSpawnTime = 0.3 + 0.2 * maxMirageWarriors;
        // L384
        if has_type(&ms, SkillType::Channel) {
            // L385
            mirageSpawnTime = mirageSpawnTime + 1.0;
        } else {
            // L387
            ms.skillData.get().set(n!("hitTimeOverride"), 1.0);
        }

        // L390 This is so that it's consistent with the info message but removing this could make it more accurate numbers wise
        // L391
        mirageSpawnTime = round(mirageSpawnTime, Some(2));

        // L393 Scale dps with GC's cooldown / attack time
        // L394 TODO This should use the contact point of the animation instead of the total attack time
        // L395
        let attackTime = lor(mainSkillOutputCache.get(n!("HitTime")), || {
            mainSkillOutputCache.get(n!("Time"))
        });
        let attackTime = match attackTime {
            Val::Num(x) => x,
            other => lua_error!(
                "bad argument #2 to 'max' (number expected, got {})",
                other.lua_type()
            ),
        };
        cooldown = m_max(cooldown, attackTime);
        // L396
        msModList.NewMod(
            n!("DPS"),
            ModType::MORE,
            (1.0 / cooldown - 1.0) * 100.0,
            "General's Cry Cooldown",
            0,
            0,
            &[],
        );

        // L398 Does not use player resources
        // L399
        msModList.NewMod(
            n!("HasNoCost"),
            ModType::FLAG,
            true,
            "Used by mirage",
            0,
            0,
            &[],
        );

        // L401 Supported Attacks Count as Exerted
        // L402
        for value in msModList.Tabulate(Some(ModType::INC), Some(&msCfg), nm!["ExertIncrease"]) {
            // L403
            let r#mod = &value.r#mod;
            // L404
            newModFrom(&msModList, n!("Damage"), r#mod);
        }
        // L406
        for value in msModList.Tabulate(Some(ModType::MORE), Some(&msCfg), nm!["ExertIncrease"]) {
            // L407
            let r#mod = &value.r#mod;
            // L408
            newModFrom(&msModList, n!("Damage"), r#mod);
        }
        // L410
        for value in msModList.Tabulate(
            Some(ModType::MORE),
            Some(&msCfg),
            nm!["ExertAttackIncrease"],
        ) {
            // L411
            let r#mod = &value.r#mod;
            // L412
            newModFrom(&msModList, n!("Damage"), r#mod);
        }
        // L414
        for value in msModList.Tabulate(
            Some(ModType::MORE),
            Some(&msCfg),
            nm!["OverexertionExertAverageIncrease"],
        ) {
            // L415
            let r#mod = &value.r#mod;
            // L416
            newModFrom(&msModList, n!("Damage"), r#mod);
        }
        // L418
        for value in msModList.Tabulate(
            Some(ModType::BASE),
            Some(&msCfg),
            nm!["ExertDoubleDamageChance"],
        ) {
            // L419
            let r#mod = &value.r#mod;
            // L420
            newModFrom(&msModList, n!("DoubleDamageChance"), r#mod);
        }

        // L423
        let msg = cat!(
            tostring_num(maxMirageWarriors).as_str(),
            " GC Mirages using ",
            grantedEffectName(&ms).as_str()
        );
        ms.infoMessage.set(Some(LStr::from(msg.as_str())));
        // L424
        let msg2 = cat!(
            tostring_num(mirageSpawnTime).as_str(),
            "s for ",
            tostring_num(maxMirageWarriors).as_str(),
            " Mirages to finish Attacking"
        );
        ms.infoMessage2.set(Some(LStr::from(msg2.as_str())));
        config = None;
    } else {
        config = None;
    }

    // L427
    calculateMirage(env, config.as_ref()).unwrap_or(false)
}

/// The locals of CalcMirages.lua 166-178 the Tawhoa's Chosen closures share (upvalues).
#[derive(Default)]
struct TawhoaLocals {
    usedSkillBestDps: RefCell<Val>,
    EffectiveSourceRate: RefCell<Val>,
    triggerCD: Cell<f64>,
    triggerCDAdjusted: Cell<f64>,
    triggeredCD: Cell<f64>,
    triggeredCDAdjusted: Cell<f64>,
    icdrSkill: Cell<f64>,
    TriggerRateCap: Cell<f64>,
    actionCooldown: Cell<f64>,
    SkillTriggerRate: Cell<f64>,
}
