//! @pob-file Modules/CalcTriggers.lua @16de4b82d
//! CalcTriggers.lua 1-901: trigger helpers and handlers.
//!
//! Owner: TR-A.
//!
//! Notes:
//! * `activeSkill.triggeredBy` is a skill effect read through
//!   `calc_active_skill::triggeredBy` (CCR-0030 #1). Its run-time keys (`mainSkill`,
//!   `ignoresTickRate`, `activationFreqMore`/`Inc`, `attachedBrandCount`, written by configTable
//!   arms, TR-B) live in the effect's `extra` table.
//! * `grantedEffect.fromItem` is read through `init_env_skills::grantedEffect_fromItem`
//!   (CCR-0083 §1: CalcSetup.lua:1637 writes it into the global data).
//! * `TriggerConfig` cannot hold `triggeredSkillCond`, a closure `comparer` with a nil
//!   source/rate, or `sourceWeapon = true` (CCR-0087). The handlers read the config through the
//!   accessors below (`triggeredSkillCond()`, `comparer_fn()`, `sourceWeapon()`); configTable
//!   arms (TR-B) store them with the matching setters. The other untyped keys the handler reads
//!   live in `config.extra` under their Lua names: `triggeredSkills`, `trigRate`,
//!   `allowTriggered`, `sourceName`, `ignoreSourceRate`, `stagesAreOverlaps`, `overlaps`,
//!   `customTriggerName` (census of CalcTriggers.lua 394-900).
//! * `simBreakdown` (CalcTriggers.lua:814) is an implicit global (§7.3); every read follows a
//!   write in the same handler, so it is a function local (calc doc §7.3, CCR-0004).
#![allow(non_snake_case)]
// Lua locals kept verbatim even where unread (`csi` 577, `triggerName` 138)
#![allow(unused_variables, unused_assignments)]
// literal Lua statement shapes
#![allow(
    clippy::assign_op_pattern,
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::type_complexity,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::len_zero,
    clippy::unnecessary_unwrap
)]

use super::TriggerConfig;
use crate::global_cache::cachedData as cache_get;
use crate::modules::calc_active_skill::triggeredBy;
use crate::modules::calc_setup::init_env_skills::grantedEffect_fromItem;
use crate::modules::calc_tools as calcLib;
use crate::modules::calcs::buildActiveSkill;
use crate::modules::common::cacheSkillUUID;
use crate::prelude::*;
use pob_gamedata::mod_tools::createMod;
use pob_lua::format::FmtArg;

// @pob Modules/CalcTriggers.lua:8-21 h=5c4fa880
// (module-level statements: calcs,ipairs,t_insert,t_remove,m_min,m_max,m_ceil,m_floor,m_modf,s_format,m_huge,bor,band)

// ---- config values the frozen `TriggerConfig` cannot hold (CCR-0087) -----------------------------

/// A `function(env, skill)` predicate of a trigger config (`triggerSkillCond`,
/// `triggeredSkillCond`). Only its truthiness is used.
pub type SkillCondFn = Rc<dyn Fn(&EnvRef, &ActiveSkillRef) -> bool>;

/// A `comparer = function(env, uuid, source, triggerRate)` of a trigger config: `source` and
/// `triggerRate` may be nil (findTriggerSkill's first candidate), and the arms' comparers close
/// over locals (CalcTriggers.lua:1090, 1104, 1318, 1354, 1482).
pub type ComparerFn = Rc<dyn Fn(&EnvRef, Name, Option<&ActiveSkillRef>, Option<f64>) -> bool>;

struct CondBox(SkillCondFn);
struct ComparerBox(ComparerFn);

/// CCR-0087: typed access to the config keys `TriggerConfig` has no usable field for.
impl TriggerConfig {
    /// `config.triggeredSkillCond = f`.
    pub fn set_triggeredSkillCond(&mut self, f: SkillCondFn) {
        self.extra
            .set(n!("triggeredSkillCond"), Val::Obj(Rc::new(CondBox(f))));
    }

    /// `config.triggeredSkillCond`.
    pub fn triggeredSkillCond(&self) -> Option<SkillCondFn> {
        self.extra
            .obj::<CondBox>(n!("triggeredSkillCond"))
            .map(|b| b.0.clone())
    }

    /// `config.comparer = f`.
    pub fn set_comparer(&mut self, f: ComparerFn) {
        self.extra
            .set(n!("comparer"), Val::Obj(Rc::new(ComparerBox(f))));
    }

    /// `config.comparer`. The typed `comparer` field (a plain fn of a non-nil source and rate)
    /// cannot express the Lua comparers; it must stay `None` (CCR-0087).
    pub fn comparer_fn(&self) -> Option<ComparerFn> {
        debug_assert!(
            self.comparer.is_none(),
            "CCR-0087: store trigger comparers with TriggerConfig::set_comparer"
        );
        self.extra
            .obj::<ComparerBox>(n!("comparer"))
            .map(|b| b.0.clone())
    }

    /// `config.sourceWeapon` (truthiness; the arms write `sourceWeapon = true`, CalcTriggers.lua
    /// 1575-1580). Either the typed field or `extra.sourceWeapon`.
    pub fn sourceWeapon(&self) -> bool {
        self.sourceWeapon.is_some() || self.extra.truthy(n!("sourceWeapon"))
    }
}

// ---- small Lua shapes ---------------------------------------------------------------------------

/// `skill.skillTypes[t]` (truthiness).
fn has_type(skill: &ActiveSkillRef, t: SkillType::T) -> bool {
    skill.skillTypes.get().with(|s| s.has(t))
}

/// `skill.activeEffect.grantedEffect`.
fn ge_of(skill: &ActiveSkillRef) -> GrantedEffectRef {
    skill.activeEffect.get().grantedEffect.get()
}

/// `actor.mainSkill` (an index of nil raises).
fn main_skill(actor: &ActorRef) -> ActiveSkillRef {
    actor
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `skill.triggeredBy` where the Lua indexes it (nil raises).
fn trig_by(skill: &ActiveSkillRef) -> SkillEffectRef {
    triggeredBy(skill)
        .unwrap_or_else(|| lua_error!("attempt to index field 'triggeredBy' (a nil value)"))
}

/// `effect.grantedEffect.levels[effect.level].cooldown` (a missing level raises).
fn level_cooldown(effect: &SkillEffect) -> Option<f64> {
    let grantedEffect = effect.grantedEffect.get();
    match grantedEffect.levels.get(effect.level.get()) {
        Some(l) => l.num(n!("cooldown")),
        None => lua_error!("attempt to index a nil value (field '?')"),
    }
}

/// `GlobalCache.cachedData[env.mode][uuid]` (a nil uuid reads nil).
fn cache(env: &EnvRef, uuid: Option<Name>) -> Option<Rc<CachedData>> {
    uuid.and_then(|u| cache_get(env.mode, u))
}

/// `GlobalCache.cachedData[env.mode][uuid]` where the Lua indexes the entry.
fn cache_req(env: &EnvRef, uuid: Option<Name>) -> Rc<CachedData> {
    cache(env, uuid).unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"))
}

/// `entry.Env.player.output`.
fn cache_output(e: &CachedData) -> Output {
    e.Env.player.output.get()
}

/// `entry.HitSpeed or entry.Speed`.
fn hit_speed(e: &CachedData) -> Val {
    lor(e.HitSpeed.clone(), || e.Speed.clone())
}

/// A Lua number that is used in arithmetic (nil raises).
fn num(v: Option<f64>, what: &str) -> f64 {
    v.unwrap_or_else(|| lua_error!("attempt to perform arithmetic on {} (a nil value)", what))
}

/// A Lua value as a `string.format` argument (`%f` of nil raises inside `lua_format`).
fn fa(v: &Val) -> FmtArg<'_> {
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

/// `t[k]`, which the Lua indexes or passes to `t_insert` (nil raises).
fn tab(t: &Table, k: Name) -> Table {
    match t.get(k) {
        Val::Table(x) => x,
        other => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `t_insert(t[k], v)`.
fn ins(t: &Table, k: Name, v: impl Into<Val>) {
    tab(t, k).push(v);
}

/// `t_insert(t[k], pos, v)`.
fn ins_at(t: &Table, k: Name, pos: i64, v: impl Into<Val>) {
    tab(t, k).insert(pos, v);
}

/// `a .. b` where `a` may be nil (raises).
fn concat_name(a: Option<Name>, b: &str) -> String {
    match a {
        Some(a) => format!("{a}{b}"),
        None => lua_error!("attempt to concatenate field 'triggerName' (a nil value)"),
    }
}

/// `actor == env.minion`.
fn is_env_minion(env: &EnvRef, actor: &ActorRef) -> bool {
    env.minion.get().is_some_and(|m| Rc::ptr_eq(&m, actor))
}

/// `skill.socketGroup.slot` (a nil socket group raises).
fn group_slot(skill: &ActiveSkillRef) -> Option<Name> {
    match skill.socketGroup.get() {
        Some(g) => g.slot.get(),
        None => lua_error!("attempt to index field 'socketGroup' (a nil value)"),
    }
}

/// `breakdown.SimData = { rowList = { }, colList = { … } }` and its rows from
/// `simBreakdown.rates` (CalcTriggers.lua 355-377 / 847-869).
fn sim_data(breakdown: &Table, simBreakdown: &Table) {
    breakdown.set(
        n!("SimData"),
        tbl! {
            rowList = Table::new(),
            colList = tbl![
                tbl! { label = "Rate", key = "rate" },
                tbl! { label = "Skill Name", key = "skillName" },
                tbl! { label = "Slot Name", key = "slotName" },
                tbl! { label = "Gem Index", key = "gemIndex" },
            ],
        },
    );
    for_ipairs!((_, rateData) in tab(simBreakdown, n!("rates")) => {
        let rateData = rateData
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'rateData'"));
        let t = Table::new();
        let name = rateData
            .str(n!("name"))
            .unwrap_or_else(|| lua_error!("bad argument #1 to 'gmatch' (string expected, got nil)"));
        for caps in pob_lua::gmatch(&name, "([^_]+)") {
            t.push(caps[0].as_str());
        }
        let row = tbl! {
            rate = round(rateData.get(n!("rate")).num(), Some(2)),
            skillName = t.get(1),
            slotName = t.get(2),
            gemIndex = t.get(3),
        };
        ins2(breakdown, n!("SimData"), n!("rowList"), row);
    });
}

/// `t_insert(t[k1][k2], v)`.
fn ins2(t: &Table, k1: Name, k2: Name, v: impl Into<Val>) {
    tab(&tab(t, k1), k2).push(v);
}

// ---- CalcTriggers.lua 24-134 ----------------------------------------------------------------------

/// `NewMod(name, type, value.mod.value, value.mod.source, value.mod.flags,
/// value.mod.keywordFlags, unpack(value.mod))`: the source may be nil (CCR-0063).
fn new_mod_copy(store: &ModStore, name: Name, ty: ModType, m: &Mod) {
    store.AddMod(Arc::new(createMod(
        name,
        ty,
        m.value.clone(),
        m.source.as_deref(),
        m.flags,
        m.keywordFlags,
        &m.tags,
    )));
}

/// @pob Modules/CalcTriggers.lua:24-31 h=8cdaa24e
/// Add trigger-based damage modifiers
pub fn addTriggerIncMoreMods(activeSkill: &ActiveSkillRef, sourceSkill: &ActiveSkillRef) {
    // L25
    for value in activeSkill.skillModList.get().Tabulate(
        Some(ModType::INC),
        Some(&sourceSkill.skillCfg.get()),
        &[n!("TriggeredDamage")],
    ) {
        // L26
        new_mod_copy(
            &activeSkill.skillModList.get(),
            n!("Damage"),
            ModType::INC,
            &value.r#mod,
        );
    }
    // L28
    for value in activeSkill.skillModList.get().Tabulate(
        Some(ModType::MORE),
        Some(&sourceSkill.skillCfg.get()),
        &[n!("TriggeredDamage")],
    ) {
        // L29
        new_mod_copy(
            &activeSkill.skillModList.get(),
            n!("Damage"),
            ModType::MORE,
            &value.r#mod,
        );
    }
}

/// @pob Modules/CalcTriggers.lua:33-39 h=8db65e26
pub fn slotMatch(env: &EnvRef, skill: &ActiveSkillRef) -> bool {
    let mainSkill = main_skill(&env.player);
    let mainEffect = mainSkill.activeEffect.get();
    let skillEffect = skill.activeEffect.get();
    // L34
    let mut fromItem = grantedEffect_fromItem(&mainEffect.grantedEffect.get())
        || grantedEffect_fromItem(&skillEffect.grantedEffect.get());
    // L35
    fromItem = fromItem
        || mainEffect
            .srcInstance
            .get()
            .is_some_and(|s| s.fromItem.get())
        || skillEffect
            .srcInstance
            .get()
            .is_some_and(|s| s.fromItem.get());
    // L36
    let match1 = fromItem
        && match skill.socketGroup.get() {
            Some(g) => {
                let mainSlot = group_slot(&mainSkill);
                g.slot.get() == mainSlot
            }
            None => false,
        };
    // L37
    let match2 = !grantedEffect_fromItem(&mainEffect.grantedEffect.get())
        && match (skill.socketGroup.get(), mainSkill.socketGroup.get()) {
            (Some(a), Some(b)) => Rc::ptr_eq(&a, &b),
            (None, None) => true,
            _ => false,
        };
    // L38
    match1 || match2
}

/// @pob Modules/CalcTriggers.lua:41-43 h=f5df59e5
/// Global `isTriggered(skill)` (Lua returns the first truthy operand; callers test it).
pub fn isTriggered(skill: &ActiveSkillRef) -> bool {
    let skillData = skill.skillData.get();
    // L42 (`grantedEffect.triggered` is never set: no data key and no run-time write)
    skillData.truthy(n!("triggeredByUnique"))
        || skillData.truthy(n!("triggered"))
        || has_type(skill, SkillType::InbuiltTrigger)
        || has_type(skill, SkillType::Triggered)
        || skill
            .activeEffect
            .get()
            .srcInstance
            .get()
            .is_some_and(|s| s.triggered.get())
}

/// @pob Modules/CalcTriggers.lua:45-63 h=c820cea2
/// → (addsCastTime, csi).
pub fn processAddedCastTime(
    skill: &ActiveSkillRef,
    breakdown: Option<&Table>,
) -> (Option<f64>, Option<f64>) {
    let skillModList = skill.skillModList.get();
    let skillCfg = skill.skillCfg.get();
    // L46
    if skillModList.Flag(
        Some(&skillCfg),
        &[n!("SpellCastTimeAddedToCooldownIfTriggered")],
    ) {
        // L47
        let baseCastTime = lor(skill.skillData.get().get(n!("castTimeOverride")), || {
            Val::from(ge_of(skill).castTime)
        })
        .num_or(1.0);
        // L48
        let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), &[n!("Speed")]);
        // L49
        let more = skillModList.More(Some(&skillCfg), &[n!("Speed")]);
        // L50
        let csi = round((1.0 + inc / 100.0) * more, Some(2));
        // L51
        let addsCastTime = baseCastTime / csi;
        // L52
        skill.skillFlags.get().set(n!("addsCastTime"), true);
        // L53
        if let Some(breakdown) = breakdown {
            // L54
            breakdown.set(
                n!("AddedCastTime"),
                tbl![
                    lfmt!(
                        "%.2f ^8(base cast time of %s)",
                        baseCastTime,
                        ge_of(skill).name
                    ),
                    lfmt!("%.2f ^8(increased/reduced)", 1.0 + inc / 100.0),
                    lfmt!("%.2f ^8(more/less)", more),
                    lfmt!("= %.2f ^8cast time", addsCastTime),
                ],
            );
        }
        // L61
        return (Some(addsCastTime), Some(csi));
    }
    (None, None)
}

/// @pob Modules/CalcTriggers.lua:65-67 h=c64d7c16
pub fn packageSkillDataForSimulation(skill: &ActiveSkillRef, env: &EnvRef) -> Table {
    let skillModList = skill.skillModList.get();
    let skillCfg = skill.skillCfg.get();
    // L66 (fields evaluated in constructor order; `processAddedCastTime` truncated to one value)
    let uuid = cacheSkillUUID(skill, env);
    let cd = skill.skillData.get().get(n!("cooldown"));
    let cdOverride =
        Val::from_value(&skillModList.Override(Some(&skillCfg), &[n!("CooldownRecovery")]));
    let addsCastTime = processAddedCastTime(skill, None).0;
    let icdr = calcLib::r#mod(&skillModList, Some(&skillCfg), &[n!("CooldownRecovery")]);
    let addedCooldown = skillModList.Sum(ModType::BASE, Some(&skillCfg), &[n!("CooldownRecovery")]);
    tbl! {
        uuid = uuid,
        cd = cd,
        cdOverride = cdOverride,
        addsCastTime = addsCastTime,
        icdr = icdr,
        addedCooldown = addedCooldown,
    }
}

/// @pob Modules/CalcTriggers.lua:69-72 h=615d5921
/// CCR-0087: `triggerRate` may be nil (the first candidate of findTriggerSkill).
pub fn defaultComparer(
    env: &EnvRef,
    uuid: Name,
    source: Option<&ActiveSkillRef>,
    triggerRate: Option<f64>,
) -> bool {
    // L70
    let cachedSpeed = hit_speed(&cache_req(env, Some(uuid)));
    // L71
    (source.is_none() && cachedSpeed.truthy())
        || (cachedSpeed.truthy() && cachedSpeed.num() > triggerRate.unwrap_or(0.0))
}

/// @pob Modules/CalcTriggers.lua:75-87 h=0a8c3102
/// Identify the trigger action skill for trigger conditions, take highest Attack Per Second
/// → (source, triggerRate, uuid). CCR-0087: `comparer` is a closure over a nil-able source/rate.
pub fn findTriggerSkill(
    env: &EnvRef,
    skill: &ActiveSkillRef,
    source: Option<ActiveSkillRef>,
    triggerRate: Option<f64>,
    comparer: Option<ComparerFn>,
) -> (Option<ActiveSkillRef>, Option<f64>, Option<Name>) {
    // L76
    let comparer = comparer.unwrap_or_else(|| Rc::new(defaultComparer) as ComparerFn);

    // L78
    let uuid = cacheSkillUUID(skill, env);
    // L79
    if cache(env, Some(uuid)).is_none() || env.mode == Mode::CALCULATOR {
        // L80
        buildActiveSkill(env, env.mode, skill, Some(uuid), &[]);
    }

    // L83
    if cache(env, Some(uuid)).is_some()
        && comparer(env, uuid, source.as_ref(), triggerRate)
        && !skill.skillFlags.get().truthy(n!("disable"))
        && !skill
            .skillCfg
            .get()
            .skillCond
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'skillCond' (a nil value)"))
            .truthy(n!("usedByMirage"))
        && !has_type(skill, SkillType::OtherThingUsesSkill)
    {
        // L84
        let e = cache_req(env, Some(uuid));
        return (Some(skill.clone()), hit_speed(&e).opt_num(), Some(uuid));
    }
    // L86
    let sourceUUID = source.as_ref().map(|s| cacheSkillUUID(s, env));
    (source, triggerRate, sourceUUID)
}

/// @pob Modules/CalcTriggers.lua:91-134 h=6bdf7133
/// Calculate the impact other skills and source rate to trigger cooldown alignment have on the trigger rate
/// for more details regarding the implementation see comments of #4599 and #5428
/// Global `calcMultiSpellRotationImpact` → (mainRate, trigRateTable). `triggerCD` nil is
/// passed as 0 (the body reads only `triggerCD or 0`); `actor` nil as `env.player`.
pub fn calcMultiSpellRotationImpact(
    env: &EnvRef,
    skillRotation: &Table,
    sourceRate: f64,
    triggerCD: f64,
    chance: Option<f64>,
    actor: &ActorRef,
) -> (f64, Table) {
    let misc = &data().misc;
    // L92
    let mut rotationIndex: f64 = 1.0;
    // L93
    let mut next_trigger: f64 = 0.0;
    // L94
    let triggerIncrement = 1.0 / sourceRate;
    // L95 Simulate 1000 attacks
    let SIM_TIME = triggerIncrement * 1000.0;
    // L96
    let chance = chance.unwrap_or(100.0);
    // L97
    let skillCount = skillRotation.len() as f64;
    // L98 (`actor or env.player`: the callers pass the resolved actor)

    // L100
    for_ipairs!((_, skill) in skillRotation => {
        let skill = skill
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'skill'"));
        // L101
        let a = lor(skill.get(n!("cdOverride")), || {
            Val::Num(
                (skill.get(n!("cd")).num_or(0.0) + skill.get(n!("addedCooldown")).num_or(0.0))
                    / skill.get(n!("icdr")).num_or(1.0),
            )
        })
        .num();
        let b = (triggerCD + skill.get(n!("addsCastTime")).num_or(0.0))
            / skill.get(n!("icdr")).num_or(1.0);
        skill.set(n!("cd"), m_max(a, b));
        // L102
        skill.set(n!("next_trig"), 0.0);
        // L103
        skill.set(n!("count"), 0.0);
    });

    let at = |i: f64| -> Table {
        skillRotation
            .table(i)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
    };
    // L106
    while next_trigger < SIM_TIME {
        // L107
        let mut currentIndex = rotationIndex;
        // L108
        loop {
            // L109 Skill at current index off cooldown, Trigger it.
            if at(currentIndex).get(n!("next_trig")).num() <= next_trigger {
                // L110
                let s = at(currentIndex);
                s.set(n!("count"), s.get(n!("count")).num() + 1.0);
                // L111 Cooldown starts at the beginning of current tick and ends at the next tick after cooldown expiration
                // L112
                let s = at(currentIndex);
                let next = ceil_b(
                    floor_b(next_trigger, misc.ServerTickTime) + s.get(n!("cd")).num(),
                    misc.ServerTickTime,
                );
                s.set(n!("next_trig"), next);
                // L113
                break;
            }
            // L115 Current skill on cooldown, try the next one.
            currentIndex = lmod(currentIndex, skillCount) + 1.0;
            // L116 All skills checked, trigger wasted
            if currentIndex == rotationIndex {
                break;
            }
        }
        // L117 Move on to the next skill in rotation
        rotationIndex = lmod(rotationIndex, skillCount) + 1.0;
        // L118
        next_trigger = next_trigger + triggerIncrement;
    }

    // L121
    let trigRateTable = tbl! { simTime = SIM_TIME, rates = Table::new() };
    // L122
    let mut mainRate = 0.0;
    // L123
    for_ipairs!((_, sd) in skillRotation => {
        let sd = sd
            .table()
            .unwrap_or_else(|| lua_error!("attempt to index local 'sd'"));
        // L124 Account for trigger chance. Adds the expected value of a geometric distribution where p = chance multiplied by triggerIncrement
        // L125 This allows for O(1) estimation of trigger chance impact on trigger rate as number of triggers approaches infinity
        // L126 Credit to Logik and Quickstick. More info in prs linked here: https://github.com/PathOfBuildingCommunity/PathOfBuilding/pull/7244 and on discord.
        // L127
        let rate = 1.0
            / (SIM_TIME / sd.get(n!("count")).num() + (triggerIncrement / chance * 100.0)
                - triggerIncrement);
        let rates = tab(&trigRateTable, n!("rates"));
        rates.push(tbl! { name = sd.get(n!("uuid")), rate = rate });
        // L128
        if Val::from(cacheSkillUUID(&main_skill(actor), env)).raw_eq(&sd.get(n!("uuid"))) {
            // L129
            mainRate = rates
                .table(rates.len() as i64)
                .unwrap_or_else(|| lua_error!("attempt to index a nil value"))
                .get(n!("rate"))
                .num();
        }
    });

    // L133
    (mainRate, trigRateTable)
}

// ---- handlers -------------------------------------------------------------------------------------

/// @pob Modules/CalcTriggers.lua:136-220 h=f6f674ce
/// `helmetFocusHandler(env)` (the config is unused; Lua returns nothing).
pub fn helmetFocusHandler(env: &EnvRef, config: &mut TriggerConfig) -> Option<Table> {
    let mainSkill = main_skill(&env.player);
    let skillFlags = mainSkill.skillFlags.get();
    // L137
    if !skillFlags.truthy(n!("minion"))
        && !skillFlags.truthy(n!("disable"))
        && triggeredBy(&mainSkill).is_some()
    {
        let skillData = mainSkill.skillData.get();
        let skillModList = mainSkill.skillModList.get();
        let skillCfg = mainSkill.skillCfg.get();
        // L138
        let triggerName = "Focus";
        // L139
        skillData.set(n!("triggered"), true);
        // L140 ---@class Output
        // L141
        let output = env.player.output.get();
        // L142 ---@class Breakdown
        // L143
        let breakdown = env.player.breakdown.get().map(|b| b.t.clone());
        // L144
        let triggerCD = level_cooldown(&trig_by(&mainSkill));
        // L145
        let triggeredCD = skillData.get(n!("cooldown"));

        // L147
        let icdrFocus = calcLib::r#mod(
            &skillModList,
            Some(&skillCfg),
            &[n!("FocusCooldownRecovery")],
        );
        // L148
        let icdrSkill = calcLib::r#mod(&skillModList, Some(&skillCfg), &[n!("CooldownRecovery")]);

        // L150 Skills trigger only on activation
        // L151 Next possible activation will be duration + cooldown
        // L152 cooldown is in milliseconds
        // L153
        let skillFocus = env
            .data
            .skills
            .get(&n!("Focus"))
            .cloned()
            .unwrap_or_else(|| lua_error!("attempt to index local 'skillFocus' (a nil value)"));
        // L154
        let focusDuration = skillFocus
            .constantStats
            .first()
            .map(|s| s.1)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            / 1000.0;
        let focusLevel1Cooldown = skillFocus
            .levels
            .get(1.0)
            .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
            .num(n!("cooldown"));
        // L155
        let focusCD = num(focusLevel1Cooldown, "field 'cooldown'") / icdrFocus;
        // L156
        let focusTotalCD = focusDuration + focusCD;

        // L158 skill cooldown should still apply to focus triggers
        // L159
        let modActionCooldown = m_max(
            triggeredCD.num_or(0.0),
            triggerCD.unwrap_or(0.0) / icdrSkill,
        );
        // L160
        let rateCapAdjusted =
            m_ceil(modActionCooldown * data().misc.ServerTickRate) / data().misc.ServerTickRate;
        // L161
        let mut triggerRate = m_huge;
        // L162
        if rateCapAdjusted != 0.0 {
            // L163
            triggerRate = 1.0 / rateCapAdjusted;
        }

        // L166
        output.set(n!("TriggerRateCap"), triggerRate);
        // L167
        output.set(n!("SkillTriggerRate"), 1.0 / focusTotalCD);

        // L169
        if let Some(breakdown) = &breakdown {
            let tickLine = if skillData.truthy(n!("ignoresTickRate")) {
                String::new()
            } else {
                lfmt!("%.3f ^8(adjusted for server tick rate)", rateCapAdjusted)
            };
            // L170
            if triggeredCD.truthy() {
                // L171
                breakdown.set(
                    n!("TriggerRateCap"),
                    tbl![
                        lfmt!(
                            "%.2f ^8(base cooldown of triggered skill)",
                            fa(&triggeredCD)
                        ),
                        lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrSkill),
                        lfmt!(
                            "= %.4f ^8(final cooldown of triggered skill)",
                            triggeredCD.num() / icdrSkill
                        ),
                        "",
                        lfmt!("%.2f ^8(base cooldown of trigger)", triggerCD),
                        lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrSkill),
                        lfmt!(
                            "= %.4f ^8(final cooldown of trigger)",
                            num(triggerCD, "local 'triggerCD'") / icdrSkill
                        ),
                        "",
                        lfmt!(
                            "%.3f ^8(biggest of trigger cooldown and triggered skill cooldown)",
                            modActionCooldown
                        ),
                        "",
                        tickLine,
                        "",
                        "Trigger rate:",
                        lfmt!("1 / %.3f", rateCapAdjusted),
                        lfmt!("= %.2f ^8per second", triggerRate),
                    ],
                );
            } else {
                // L189
                breakdown.set(
                    n!("TriggerRateCap"),
                    tbl![
                        "Triggered skill has no base cooldown",
                        "",
                        lfmt!("%.2f ^8(base cooldown of trigger)", triggerCD),
                        lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrSkill),
                        lfmt!(
                            "= %.4f ^8(final cooldown of trigger)",
                            num(triggerCD, "local 'triggerCD'") / icdrSkill
                        ),
                        "",
                        tickLine,
                        "",
                        "Trigger rate:",
                        lfmt!("1 / %.3f", rateCapAdjusted),
                        lfmt!("= %.3f ^8per second", triggerRate),
                    ],
                );
            }
            // L203
            breakdown.set(
                n!("SkillTriggerRate"),
                tbl![
                    lfmt!("%.2f ^8(focus base cooldown)", focusLevel1Cooldown),
                    lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdrFocus),
                    lfmt!(
                        "+ %.2f ^8(skills are only triggered on activation thus we add focus duration)",
                        focusDuration
                    ),
                    lfmt!(
                        "= %.2f ^8(effective skill cooldown for trigger purposes)",
                        focusTotalCD
                    ),
                    "",
                    lfmt!(
                        "%.3f casts per second ^8(Assuming player uses focus exactly when its cooldown of %.2fs expires)",
                        1.0 / focusTotalCD,
                        focusTotalCD
                    ),
                ],
            );
        }

        // L213 Account for Trigger-related INC/MORE modifiers
        // L214
        addTriggerIncMoreMods(&mainSkill, &mainSkill);
        // L215
        mainSkill
            .infoMessage
            .set(Some(LStr::from("Assuming perfect focus Re-Use")));
        // L216
        mainSkill.infoTrigger.set(Some(LStr::from(triggerName)));
        // L217
        skillData.set(n!("triggerRate"), output.get(n!("SkillTriggerRate")));
        // L218
        skillFlags.set(n!("globalTrigger"), true);
    }
    None
}

/// @pob Modules/CalcTriggers.lua:222-392 h=804aedd9
/// `CWCHandler(env)` (the config is unused; Lua returns nothing).
pub fn CWCHandler(env: &EnvRef, config: &mut TriggerConfig) -> Option<Table> {
    let player = &env.player;
    let mainSkill = main_skill(player);
    // L223
    if !mainSkill.skillFlags.get().truthy(n!("minion"))
        && !mainSkill.skillFlags.get().truthy(n!("disable"))
    {
        // L224
        let triggeredSkills = Table::new();
        // L225
        let mut source: Option<ActiveSkillRef> = None;
        // L226
        let triggerName = "Cast While Channeling";
        // L227
        let output = player.output.get();
        // L228 ---@class Breakdown
        // L229
        let breakdown = player.breakdown.get().map(|b| b.t.clone());
        // L230
        for_ipairs!((_, skill) in player.activeSkillList.get() => {
            // L231
            let slotMatch = slotMatch(env, &skill);
            // L232
            if source.is_none() {
                // L233
                let gemData = trig_by(&mainSkill).gemData.get();
                let canSupport = match gemData {
                    Some(gd) => calcLib::canGrantedEffectSupportActiveSkill(
                        &gd.grantedEffect,
                        &skill,
                        None,
                        false,
                    ),
                    None => false,
                };
                // L234
                if skill.skillData.get().truthy(n!("triggerTime"))
                    && canSupport
                    && !Rc::ptr_eq(&skill, &mainSkill)
                    && slotMatch
                    && !isTriggered(&skill)
                {
                    // L235
                    source = Some(skill.clone());
                }
            }
            // L238
            if skill.skillData.get().truthy(n!("triggeredWhileChannelling")) && slotMatch {
                // L239
                triggeredSkills.push(packageSkillDataForSimulation(&skill, env));
            }
        });
        let mainData = mainSkill.skillData.get();
        // L242
        let Some(source) = source.filter(|_| triggeredSkills.len() >= 1) else {
            // L243
            mainData.set(n!("triggered"), Val::Nil);
            // L244
            mainSkill
                .infoMessage2
                .set(Some(LStr::from("DPS reported assuming Self-Cast")));
            // L245
            mainSkill.infoMessage.set(Some(LStr::from(
                lfmt!("No %s Triggering Skill Found", triggerName).as_str(),
            )));
            // L246
            mainSkill.infoTrigger.set(Some(LStr::from("")));
            return None;
        };
        let skillModList = mainSkill.skillModList.get();
        let skillCfg = mainSkill.skillCfg.get();
        // L248 (`name or "Triggered"`: a granted effect always has a name)
        let triggeredName = ge_of(&mainSkill).name;

        // L250
        output.set(
            n!("addsCastTime"),
            processAddedCastTime(&mainSkill, breakdown.as_ref()).0,
        );

        // L252 (`calcLib.mod(...) or 1`: a number is never falsy)
        let icdr = calcLib::r#mod(&skillModList, Some(&skillCfg), &[n!("CooldownRecovery")]);
        // L253
        let triggerIntervalV = source.skillData.get().get(n!("triggerTime"));
        let triggerInterval = triggerIntervalV.num();
        // L254
        let triggerRateOfTrigger = 1.0 / triggerInterval;
        // L255
        let triggeredCD = mainData.get(n!("cooldown"));
        // L256
        let cooldownOverride =
            Val::from_value(&skillModList.Override(Some(&skillCfg), &[n!("CooldownRecovery")]));

        // L258
        if cooldownOverride.truthy() {
            // L259
            mainSkill.skillFlags.get().set(n!("hasOverride"), true);
        }

        // L262
        let triggeredTotalCooldown = lor(cooldownOverride.clone(), || {
            Val::Num(
                m_max(
                    triggeredCD.num_or(0.0),
                    output.get(n!("addsCastTime")).num_or(0.0),
                ) / icdr,
            )
        })
        .num();
        // L263
        let triggeredCDAdjusted = m_ceil(triggeredTotalCooldown * data().misc.ServerTickRate)
            / data().misc.ServerTickRate;
        // L264
        let effCDTriggeredSkill =
            m_ceil(triggeredCDAdjusted * triggerRateOfTrigger) / triggerRateOfTrigger;

        // L266
        let simBreakdown: Table;
        // L267
        output.set(
            n!("TriggerRateCap"),
            m_min(1.0 / effCDTriggeredSkill, triggerRateOfTrigger),
        );
        // L268
        let (rate, sim) = calcMultiSpellRotationImpact(
            env,
            &triggeredSkills,
            triggerRateOfTrigger,
            0.0,
            None,
            player,
        );
        output.set(n!("SkillTriggerRate"), rate);
        simBreakdown = sim;
        // L269
        let triggerBotsEffective = player.modDB.get().Flag(None, &[n!("HaveTriggerBots")])
            && has_type(&mainSkill, SkillType::Spell);
        // L270
        if triggerBotsEffective {
            // L271
            output.set(
                n!("SkillTriggerRate"),
                2.0 * output.get(n!("SkillTriggerRate")).num(),
            );
        }

        // L274
        if let Some(breakdown) = &breakdown {
            let sourceName = ge_of(&source).name;
            // L275
            if triggeredCD.truthy() || cooldownOverride.truthy() {
                // L276
                breakdown.set(
                    n!("TriggerRateCap"),
                    tbl![
                        lfmt!(
                            "Cast While Channeling triggers %s every %.2fs while channeling %s ",
                            triggeredName,
                            fa(&triggerIntervalV),
                            sourceName
                        ),
                        "",
                        lfmt!(
                            "%.2f ^8(base cooldown of triggered skill)",
                            fa(&triggeredCD)
                        ),
                        lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdr),
                        lfmt!(
                            "= %.4f ^8(final cooldown of triggered skill)",
                            triggeredCD.num() / icdr
                        ),
                        "",
                    ],
                );
                // L284
                if !cooldownOverride.is_nil() {
                    let trc = tab(breakdown, n!("TriggerRateCap"));
                    // L285
                    trc.set(
                        4,
                        lfmt!(
                            "%.2f ^8(hard override of cooldown of %s)",
                            fa(&cooldownOverride),
                            triggeredName
                        ),
                    );
                    // L286
                    trc.remove(Some(5));
                    // L287
                    trc.remove(Some(5));
                }
            } else {
                // L290
                breakdown.set(
                    n!("TriggerRateCap"),
                    tbl![
                        lfmt!(
                            "Cast While Channeling triggers %s every %.2fs while channeling %s ",
                            triggeredName,
                            fa(&triggerIntervalV),
                            sourceName
                        ),
                        "",
                        format!("{triggeredName} has no base cooldown or cooldown override"),
                        "",
                    ],
                );
            }

            // L298
            let extraIncreaseNeeded = |affectedCD: f64| -> Option<f64> {
                // L299
                if !cooldownOverride.truthy() {
                    // L300
                    let nextBreakpoint = effCDTriggeredSkill - triggerInterval;
                    // L301
                    let timeOverBreakpoint = triggeredTotalCooldown - nextBreakpoint;
                    // L302
                    let alreadyReducedTime = triggeredTotalCooldown * icdr - triggeredTotalCooldown;
                    // L303
                    if timeOverBreakpoint < affectedCD {
                        // L304
                        let divNeeded =
                            affectedCD / (affectedCD - timeOverBreakpoint - alreadyReducedTime);
                        // L305
                        let incTotal = m_ceil((divNeeded - 1.0) * 100.0);
                        // L306
                        return Some(incTotal - (icdr - 1.0) * 100.0);
                    }
                }
                None
            };

            // L311
            let addsCastTimeV = output.get(n!("addsCastTime"));
            if addsCastTimeV.truthy() {
                let addsCastTime = addsCastTimeV.num();
                // L312
                ins(
                    breakdown,
                    n!("TriggerRateCap"),
                    "Cast While Channeling had no base cooldown",
                );
                // L313
                ins(
                    breakdown,
                    n!("TriggerRateCap"),
                    lfmt!(
                        "+ %.2f ^8(%s adds cast time as cooldown to trigger)",
                        addsCastTime,
                        triggeredName
                    ),
                );
                // L314
                ins(
                    breakdown,
                    n!("TriggerRateCap"),
                    lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdr),
                );
                // L315
                ins(
                    breakdown,
                    n!("TriggerRateCap"),
                    lfmt!(
                        "= %.4f ^8(final cooldown of triggered skill)",
                        addsCastTime / icdr
                    ),
                );
                // L316
                ins(breakdown, n!("TriggerRateCap"), "");
                // L317
                ins(
                    breakdown,
                    n!("TriggerRateCap"),
                    lfmt!(
                        "%.3f ^8(adjusted for server tick rate)",
                        triggeredCDAdjusted
                    ),
                );
                // L318
                ins(breakdown, n!("TriggerRateCap"), "");

                // L320
                let extraCSINeeded = extraIncreaseNeeded(addsCastTime);
                // L321
                let extraICDRNeeded = extraIncreaseNeeded(triggeredTotalCooldown * icdr);
                // L322
                if let Some(extraICDRNeeded) = extraICDRNeeded {
                    // L323
                    ins(
                        breakdown,
                        n!("TriggerRateCap"),
                        lfmt!(
                            "^8(extra ICDR of %d%% would reach next breakpoint)",
                            extraICDRNeeded
                        ),
                    );
                }
                // L325
                if let Some(extraCSINeeded) = extraCSINeeded {
                    // L326
                    ins(
                        breakdown,
                        n!("TriggerRateCap"),
                        lfmt!(
                            "^8(extra Cast Rate Increase of %d%% would reach next breakpoint)",
                            extraCSINeeded
                        ),
                    );
                    // L327
                    ins(breakdown, n!("TriggerRateCap"), "");
                }
            } else {
                // L330
                let extraICDRNeeded = extraIncreaseNeeded(triggeredTotalCooldown * icdr);
                // L331
                if let Some(extraICDRNeeded) = extraICDRNeeded {
                    // L332
                    ins(
                        breakdown,
                        n!("TriggerRateCap"),
                        lfmt!(
                            "^8(extra ICDR of %d%% would reach next breakpoint)",
                            extraICDRNeeded
                        ),
                    );
                    // L333
                    ins(breakdown, n!("TriggerRateCap"), "");
                }
            }

            let triggerRateCap = output.get(n!("TriggerRateCap")).num();
            // L337
            ins(breakdown, n!("TriggerRateCap"), "Trigger rate:");
            // L338
            ins(
                breakdown,
                n!("TriggerRateCap"),
                lfmt!(
                    "1 / %.3f ^8(trigger rate adjusted for triggering interval)",
                    1.0 / triggerRateCap
                ),
            );
            // L339
            ins(
                breakdown,
                n!("TriggerRateCap"),
                lfmt!(
                    "= %.2f ^8 %s casts per second",
                    triggerRateCap,
                    triggeredName
                ),
            );

            // L341 Hide Skill Trigger Rate breakdown if there's only one skill to av
            // L342
            if triggeredSkills.len() > 1 {
                let skillTriggerRate = output.get(n!("SkillTriggerRate")).num();
                // L343
                breakdown.set(
                    n!("SkillTriggerRate"),
                    tbl![
                        lfmt!(
                            "%.2f ^8(%s triggers per second)",
                            triggerRateOfTrigger,
                            triggerName
                        ),
                        lfmt!(
                            "/ %.2f ^8(Estimated impact of linked spells)",
                            triggerRateOfTrigger / skillTriggerRate
                        ),
                        lfmt!(
                            "= %.2f ^8%s casts per second",
                            skillTriggerRate,
                            triggeredName
                        ),
                    ],
                );
                // L348
                if triggerBotsEffective {
                    // L349
                    ins_at(
                        breakdown,
                        n!("SkillTriggerRate"),
                        3,
                        "x 2 ^8(Trigger bots effectively cause the skill to trigger twice)",
                    );
                }
                // L351
                if simBreakdown.truthy(n!("extraSimInfo")) {
                    // L352
                    ins(breakdown, n!("SkillTriggerRate"), "");
                    // L353
                    ins(
                        breakdown,
                        n!("SkillTriggerRate"),
                        simBreakdown.get(n!("extraSimInfo")),
                    );
                }
                // L355-377
                sim_data(breakdown, &simBreakdown);
            }
        }

        // L381 Account for Trigger-related INC/MORE modifiers
        // L382
        addTriggerIncMoreMods(&mainSkill, &mainSkill);
        // L383
        player
            .output
            .get()
            .set(n!("ChannelTimeToTrigger"), triggerIntervalV);
        // L384
        mainData.set(n!("triggered"), true);
        // L385
        mainSkill.skillFlags.get().set(n!("globalTrigger"), true);
        // L386
        mainData.set(n!("triggerRate"), output.get(n!("SkillTriggerRate")));
        // L387
        mainData.set(n!("triggerSourceUUID"), cacheSkillUUID(&source, env));
        // L388
        mainSkill.infoMessage.set(Some(LStr::from(
            format!("{}'s Trigger: {}", triggerName, ge_of(&source).name).as_str(),
        )));
        // L389
        let infoTrigger = mainSkill
            .infoTrigger
            .get()
            .unwrap_or_else(|| LStr::from(triggerName));
        player.infoTrigger.set(Some(infoTrigger));
    }
    None
}

/// `config.triggerName` as a `%s` argument (nil formats as "nil").
fn fname(n: Option<Name>) -> FmtArg<'static> {
    n.map_or(FmtArg::Nil, |n| FmtArg::Str(n.as_str()))
}

/// `t[k]` that the Lua indexes (nil raises).
fn idx(t: &Table, k: Name) -> Table {
    t.table(k)
        .unwrap_or_else(|| lua_error!("attempt to index field '{}' (a nil value)", k.as_str()))
}

/// `skill.activeEffect.srcInstance.skillStageCount` (a nil srcInstance raises).
fn src_stage_count(skill: &ActiveSkillRef) -> Val {
    match skill.activeEffect.get().srcInstance.get() {
        Some(g) => Val::from(g.skillStageCount.get()),
        None => lua_error!("attempt to index field 'srcInstance' (a nil value)"),
    }
}

/// `config.stagesAreOverlaps and env.player.mainSkill.skillPart == config.stagesAreOverlaps and
/// env.player.mainSkill.activeEffect.srcInstance.skillStageCount` (CalcTriggers.lua:798, 821).
fn stages_as_overlaps(config: &TriggerConfig, playerMain: &ActiveSkillRef) -> Val {
    let soa = config.extra.get(n!("stagesAreOverlaps"));
    if !soa.truthy() {
        return soa;
    }
    if !Val::from(playerMain.skillPart.get()).raw_eq(&soa) {
        return Val::Bool(false);
    }
    src_stage_count(playerMain)
}

/// @pob Modules/CalcTriggers.lua:394-900 h=8a0c6dbc
pub fn defaultTriggerHandler(env: &EnvRef, config: &mut TriggerConfig) -> Option<Table> {
    let ESR = n!("EffectiveSourceRate");
    let TRC = n!("TriggerRateCap");
    let STR = n!("SkillTriggerRate");
    let misc = &data().misc;
    // L395
    let actor = config
        .actor
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index field 'actor' (a nil value)"));
    // L396
    let output = actor.output.get();
    // L397 ---@class Breakdown
    // L398
    let breakdown = actor.breakdown.get().map(|b| b.t.clone());
    // L399
    let mut source = config.source.clone();
    // L400
    let triggeredSkills = config
        .extra
        .table(n!("triggeredSkills"))
        .unwrap_or_default();
    // L401
    let mut trigRate: Option<f64> = config.extra.get(n!("trigRate")).opt_num();
    // L402
    let mut uuid: Option<Name> = None;
    // L403
    let sourceWeapon = config.sourceWeapon();
    let mainSkill = main_skill(&actor);
    let mainData = mainSkill.skillData.get();
    let mainFlags = mainSkill.skillFlags.get();
    // L404 Only attacks using the granting weapon can activate source-weapon triggers
    // L405
    let sourceWeaponFlag: Option<Name> = if sourceWeapon {
        group_slot(&mainSkill).map(|slot| {
            if slot.as_str().starts_with("Weapon 2") {
                n!("weapon2Attack")
            } else {
                n!("weapon1Attack")
            }
        })
    } else {
        None
    };
    let triggerSkillCond = config.triggerSkillCond.clone();
    let triggeredSkillCond = config.triggeredSkillCond();

    // L407 Find trigger skill and triggered skills
    // L408
    if triggeredSkillCond.is_some() || triggerSkillCond.is_some() {
        // L409
        for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
            // L410
            if triggerSkillCond.as_ref().is_some_and(|f| f(env, &skill))
                && sourceWeaponFlag.map_or(true, |f| skill.skillFlags.get().truthy(f))
                && (!isTriggered(&skill)
                    || mainFlags.truthy(n!("globalTrigger"))
                    || config.extra.truthy(n!("allowTriggered")))
                && !Rc::ptr_eq(&skill, &mainSkill)
            {
                // L411
                let r = findTriggerSkill(env, &skill, source.take(), trigRate, config.comparer_fn());
                source = r.0;
                trigRate = r.1;
                uuid = r.2;
            }
            // L413
            if triggeredSkillCond.as_ref().is_some_and(|f| f(env, &skill)) {
                // L414
                triggeredSkills.push(packageSkillDataForSimulation(&skill, env));
            }
        });
    }
    // L418
    if triggeredSkills.len() > 0 || triggeredSkillCond.is_none() {
        // L419
        if source.is_none()
            && !(mainFlags.truthy(n!("globalTrigger")) && triggeredSkillCond.is_some())
        {
            // L420
            mainData.set(n!("triggered"), Val::Nil);
            // L421
            mainSkill
                .infoMessage2
                .set(Some(LStr::from("DPS reported assuming Self-Cast")));
            // L422
            mainSkill.infoMessage.set(Some(LStr::from(
                lfmt!("No %s Triggering Skill Found", fname(config.triggerName)).as_str(),
            )));
            // L423
            mainSkill.infoTrigger.set(Some(LStr::from("")));
        } else {
            // L425
            mainData.set(n!("triggered"), true);

            // L427 Account for Arcanist Brand using activation frequency
            // L428
            if breakdown.is_some() && mainData.truthy(n!("triggeredByBrand")) {
                let bd = breakdown.as_ref().unwrap();
                let tb = trig_by(&mainSkill);
                let tbMain = tb
                    .extra
                    .obj::<ActiveSkill>(n!("mainSkill"))
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index field 'mainSkill' (a nil value)")
                    });
                // L429
                bd.set(
                    ESR,
                    tbl![
                        lfmt!(
                            "%.2f ^8(base activation cooldown of %s)",
                            fa(&tbMain.skillData.get().get(n!("repeatFrequency"))),
                            fname(config.triggerName)
                        ),
                        lfmt!(
                            "* %.2f ^8(more activation frequency)",
                            fa(&tb.extra.get(n!("activationFreqMore")))
                        ),
                        lfmt!(
                            "* %.2f ^8(increased activation frequency)",
                            fa(&tb.extra.get(n!("activationFreqInc")))
                        ),
                        lfmt!(
                            "* %d ^8(attached Brands)",
                            fa(&tb.extra.get(n!("attachedBrandCount")))
                        ),
                        lfmt!(
                            "= %.2f ^8(activation rate of %s)",
                            trigRate,
                            ge_of(&tbMain).name
                        ),
                    ],
                );
            } else if let Some(bd) = &breakdown {
                // L437
                bd.set(ESR, Table::new());
                // L438
                if trigRate.is_some() && source.is_some() {
                    // L439
                    if config.assumingEveryHitKills {
                        // L440
                        ins(bd, ESR, "Assuming every attack kills");
                    }
                    // L442
                    let sourceName = lor(config.extra.get(n!("sourceName")), || {
                        Val::from(ge_of(source.as_ref().unwrap()).name)
                    });
                    ins(
                        bd,
                        ESR,
                        lfmt!(
                            "%.2f ^8(%s %s)",
                            trigRate,
                            fa(&sourceName),
                            if config.useCastRate {
                                "cast rate"
                            } else {
                                "attack rate"
                            }
                        ),
                    );
                }
            }

            // L446 Dual wield triggers
            // L447
            let sourceWeaponTrigger = sourceWeapon
                && source
                    .as_ref()
                    .is_some_and(|s| s.skillFlags.get().truthy(n!("bothWeaponAttack")));
            // L448
            let itemSupportTrigger = triggeredBy(&mainSkill).is_some_and(|tb| {
                let ge = tb.grantedEffect.get();
                ge.support && grantedEffect_fromItem(&ge)
            });
            // L449
            if trigRate.is_some()
                && source.as_ref().is_some_and(|src| {
                    env.player.weaponData1.get().truthy(n!("type"))
                        && env.player.weaponData2.get().truthy(n!("type"))
                        && !src.skillData.get().truthy(n!("doubleHitsWhenDualWielding"))
                        && (has_type(src, SkillType::Melee) || has_type(src, SkillType::Attack))
                })
                && (sourceWeaponTrigger || itemSupportTrigger)
            {
                // L450
                trigRate = Some(num(trigRate, "local 'trigRate'") / 2.0);
                // L451
                if let Some(bd) = &breakdown {
                    // L452
                    ins_at(bd, ESR, 2, lfmt!("/ 2 ^8(due to dual wielding)"));
                }
            }

            // L456
            let ignoresTickRate = lor(mainData.get(n!("ignoresTickRate")), || {
                let storedUses = mainData.get(n!("storedUses"));
                land(storedUses.clone(), || Val::Bool(storedUses.num() > 1.0))
            });
            mainData.set(n!("ignoresTickRate"), ignoresTickRate);

            // L458 Account for source unleash
            // L459
            if source.as_ref().is_some_and(|src| {
                cache(env, uuid).is_some()
                    && src.skillModList.get().Flag(None, &[n!("HasSeals")])
                    && has_type(src, SkillType::CanRapidFire)
            }) {
                // L460
                let unleashDpsMult = cache_req(env, uuid)
                    .ActiveSkill
                    .skillData
                    .get()
                    .get(n!("dpsMultiplier"))
                    .num_or(1.0);
                // L461
                trigRate = Some(num(trigRate, "local 'trigRate'") * unleashDpsMult);
                // L462
                mainFlags.set(n!("HasSeals"), true);
                // L463
                mainData.set(n!("ignoresTickRate"), true);
                // L464
                if let Some(bd) = &breakdown {
                    // L465
                    ins(
                        bd,
                        ESR,
                        lfmt!("x %.2f ^8(multiplier from Unleash)", unleashDpsMult),
                    );
                }
            }

            // L469 Account for skills that can hit multiple times per use
            // L470
            if source.as_ref().is_some_and(|src| {
                cache(env, uuid).is_some()
                    && src.skillPartName.get().is_some_and(|p| {
                        pob_lua::lmatch(&p, "(.*)All(.*)Projectiles(.*)", None).is_some()
                    })
                    && src.skillFlags.get().truthy(n!("projectile"))
            }) {
                // L471
                let multiHitDpsMult = cache_output(&cache_req(env, uuid))
                    .get(n!("ProjectileCount"))
                    .num_or(1.0);
                // L472
                trigRate = Some(num(trigRate, "local 'trigRate'") * multiHitDpsMult);
                // L473
                if let Some(bd) = &breakdown {
                    // L474
                    ins(
                        bd,
                        ESR,
                        lfmt!(
                            "x %.2f ^8(%d projectiles hit)",
                            multiHitDpsMult,
                            multiHitDpsMult
                        ),
                    );
                }
            }

            // L478 Special handling for Kitava's Thirst
            // L479 Repeated hits do not consume mana and do not trigger Kitava's thirst
            // L480
            if mainData.truthy(n!("triggeredByManaSpent")) {
                let src = source
                    .as_ref()
                    .unwrap_or_else(|| lua_error!("attempt to index local 'source' (a nil value)"));
                // L481
                let repeats = 1.0
                    + src
                        .skillModList
                        .get()
                        .Sum(ModType::BASE, None, &[n!("RepeatCount")]);
                // L482
                trigRate = Some(num(trigRate, "local 'trigRate'") / repeats);
                // L483
                if breakdown.is_some() && repeats > 1.0 {
                    // L484
                    ins(
                        breakdown.as_ref().unwrap(),
                        ESR,
                        lfmt!(
                            "/%d ^8(repeated attacks/casts do not count as they don't use mana)",
                            repeats
                        ),
                    );
                }
            }

            // L488 Battlemage's Cry uptime
            // L489
            if mainData.truthy(n!("triggeredByBattleMageCry"))
                && cache(env, uuid).is_some()
                && source
                    .as_ref()
                    .is_some_and(|s| has_type(s, SkillType::Melee))
            {
                let o = cache_output(&cache_req(env, uuid));
                // L490
                let battleMageExertsCount = o.get(n!("BattleCryExertsCount"));
                // L491
                let battleMageDuration = ceil_b(
                    o.get(n!("BattleMageCryDuration")).num(),
                    misc.ServerTickTime,
                );
                // L492
                let battleMageCastTime = o.get(n!("BattleMageCryCastTime"));
                // L493
                let battleMageCooldown = ceil_b(
                    o.get(n!("BattleMageCryCooldown")).num(),
                    misc.ServerTickTime,
                );

                // L495 Cap the number of hits that happen during the duration
                // L496
                let battleMageHits = m_max(
                    m_min(
                        num(trigRate, "local 'trigRate'") * battleMageDuration,
                        battleMageExertsCount.num(),
                    ),
                    0.0,
                );

                // L498
                if let Some(bd) = &breakdown {
                    // L499
                    ins(bd, ESR, lfmt!("^8(min(%.2f * %.2f, %d) = %.2f average number of exerted attacks capped by exerted count)", trigRate, battleMageDuration, fa(&battleMageExertsCount), battleMageHits));
                    // L500
                    ins(bd, ESR, lfmt!("= %.2f / (%.2f + %.2f) ^8(the calculated number of exerted attacks happens every cooldown + duration)", battleMageHits, fa(&battleMageCastTime), battleMageCooldown));
                }

                // L503 The hits happen every battlemage cooldown + duration
                // L504
                trigRate = Some(battleMageHits / (battleMageCastTime.num() + battleMageCooldown));
            }

            // L507 Infernal Cry uptime
            // L508
            if ge_of(&mainSkill).name.as_str() == "Combust"
                && cache(env, uuid).is_some()
                && source
                    .as_ref()
                    .is_some_and(|s| has_type(s, SkillType::Melee))
            {
                let o = cache_output(&cache_req(env, uuid));
                // L509
                let infernalCryExertsCount = o.get(n!("InfernalExertsCount"));
                // L510
                let infernalCryDuration =
                    ceil_b(o.get(n!("InfernalCryDuration")).num(), misc.ServerTickTime);
                // L511
                let infernalCryCastTime = o.get(n!("InfernalCryCastTime"));
                // L512
                let infernalCryCooldown =
                    ceil_b(o.get(n!("InfernalCryCooldown")).num(), misc.ServerTickTime);

                // L514 Cap the number of hits that happen during the duration
                // L515
                let infernalCryHits = m_max(
                    m_min(
                        num(trigRate, "local 'trigRate'") * infernalCryDuration,
                        infernalCryExertsCount.num(),
                    ),
                    0.0,
                );

                // L517
                if let Some(bd) = &breakdown {
                    // L518
                    ins(bd, ESR, lfmt!("^8(min(%.2f * %.2f, %d) = %.2f average number of exerted attacks capped by exerted count)", trigRate, infernalCryDuration, fa(&infernalCryExertsCount), infernalCryHits));
                    // L519
                    ins(bd, ESR, lfmt!("= %.2f / (%.2f + %.2f) ^8(the calculated number of exerted attacks happens every cooldown + duration)", infernalCryHits, fa(&infernalCryCastTime), infernalCryCooldown));
                }

                // L522 The hits happen every Infernal Cry cooldown + duration
                // L523
                trigRate =
                    Some(infernalCryHits / (infernalCryCastTime.num() + infernalCryCooldown));
            }

            // L526 Handling for mana spending rate for Manaforged Arrows Support
            // L527
            if mainData.truthy(n!("triggeredByManaforged"))
                && num(trigRate, "local 'trigRate'") > 0.0
            {
                // L528
                let triggeredUUID = cacheSkillUUID(&mainSkill, env);
                // L529
                if cache(env, Some(triggeredUUID)).is_none() {
                    // L530
                    buildActiveSkill(
                        env,
                        env.mode,
                        &mainSkill,
                        Some(triggeredUUID),
                        &[triggeredUUID],
                    );
                }
                // L532
                let triggeredManaCost = cache_output(&cache_req(env, Some(triggeredUUID)))
                    .get(n!("ManaCostRaw"))
                    .num_or(0.0);
                // L533
                if triggeredManaCost > 0.0 {
                    let threshold = mainData.get(n!("ManaForgedArrowsPercentThreshold"));
                    // L534
                    let manaSpentThreshold = triggeredManaCost * threshold.num();
                    // L535
                    let sourceManaCost = cache_output(&cache_req(env, uuid))
                        .get(n!("ManaCostRaw"))
                        .num_or(0.0);
                    // L536
                    if sourceManaCost > 0.0 {
                        // L537
                        if let Some(bd) = &breakdown {
                            let first = idx(bd, ESR).get(1);
                            // L538
                            bd.set(
                                ESR,
                                tbl![
                                    lfmt!("%.4f ^8(Mana cost of trigger source)", sourceManaCost),
                                    lfmt!(""),
                                    lfmt!("%.4f ^8(Mana Cost of triggered)", triggeredManaCost),
                                    lfmt!(
                                        "%.2f ^8(Manaforged threshold multiplier)",
                                        fa(&threshold)
                                    ),
                                    lfmt!(
                                        "= %.4f ^8(Manaforged trigger threshold)",
                                        manaSpentThreshold
                                    ),
                                    lfmt!(""),
                                    lfmt!(
                                        "%.4f ^8(Manaforged trigger threshold)",
                                        manaSpentThreshold
                                    ),
                                    lfmt!("/ %.4f ^8(Mana cost of trigger source)", sourceManaCost),
                                    lfmt!(
                                        "= %.2f ^8(Skill usages required)",
                                        manaSpentThreshold / sourceManaCost
                                    ),
                                    lfmt!(""),
                                    first,
                                    lfmt!(
                                        "/ ceil(%.2f) ^8(%d Skill usages required)",
                                        manaSpentThreshold / sourceManaCost,
                                        m_ceil(manaSpentThreshold / sourceManaCost)
                                    ),
                                ],
                            );
                        }
                        // L553
                        trigRate = Some(
                            num(trigRate, "local 'trigRate'")
                                / m_ceil(manaSpentThreshold / sourceManaCost),
                        );
                    } else {
                        // L555
                        if let Some(bd) = &breakdown {
                            // L556
                            ins(
                                bd,
                                ESR,
                                lfmt!("Source skill has no mana cost", fa(&output.get(ESR))),
                            );
                        }
                        // L558
                        trigRate = Some(0.0);
                    }
                }
            }

            let skillModList = mainSkill.skillModList.get();
            let skillCfg = mainSkill.skillCfg.get();
            // L563 (`calcLib.mod(...) or 1`: a number is never falsy)
            let icdr = calcLib::r#mod(&skillModList, Some(&skillCfg), &[n!("CooldownRecovery")]);
            // L564
            let mut addedCooldown =
                Some(skillModList.Sum(ModType::BASE, Some(&skillCfg), &[n!("CooldownRecovery")]));
            // L565
            addedCooldown = addedCooldown.filter(|&a| a != 0.0);
            // L566
            let cooldownOverride =
                Val::from_value(&skillModList.Override(Some(&skillCfg), &[n!("CooldownRecovery")]))
                    .opt_num();
            // L567
            let mut triggerCD: Option<f64> = if triggeredBy(&mainSkill).is_some() {
                level_cooldown(&trig_by(&main_skill(&env.player)))
            } else {
                None
            };
            // L568
            if triggerCD.is_none() {
                let src = source
                    .as_ref()
                    .unwrap_or_else(|| lua_error!("attempt to index local 'source' (a nil value)"));
                triggerCD = triggeredBy(src).and_then(|tb| level_cooldown(&tb));
            }
            // L569
            let triggeredCD: Option<f64> = mainData.get(n!("cooldown")).opt_num();

            // L571
            if mainData.truthy(n!("triggeredByBrand")) {
                let tb = trig_by(&mainSkill);
                let tbMain = tb
                    .extra
                    .obj::<ActiveSkill>(n!("mainSkill"))
                    .unwrap_or_else(|| {
                        lua_error!("attempt to index field 'mainSkill' (a nil value)")
                    });
                // L572
                triggerCD = Some(
                    tbMain.skillData.get().get(n!("repeatFrequency")).num()
                        / tb.extra.get(n!("activationFreqMore")).num()
                        / tb.extra.get(n!("activationFreqInc")).num(),
                );
                // L573 cancels out division by icdr lower, brand activation rate is not affected by icdr
                triggerCD = Some(triggerCD.unwrap() * icdr);
            }

            // L576 (`grantedEffect and …`: a skill always has a granted effect)
            let triggeredName: Name = if !is_env_minion(env, &actor) {
                ge_of(&mainSkill).name
            } else {
                n!("Triggered skill")
            };
            // L577
            let csi: Option<f64>;
            // L578
            let (addsCastTime, c) =
                processAddedCastTime(&main_skill(&env.player), breakdown.as_ref());
            output.set(n!("addsCastTime"), addsCastTime);
            csi = c;

            // L580
            let triggeredCDAdjusted =
                (triggeredCD.unwrap_or(0.0) + addedCooldown.unwrap_or(0.0)) / icdr;
            // L581
            let triggerCDAdjusted = (triggerCD.unwrap_or(0.0) + addsCastTime.unwrap_or(0.0)) / icdr;
            // L582
            let triggeredCDTickRounded = if mainData.truthy(n!("ignoresTickRate")) {
                triggeredCDAdjusted
            } else {
                m_ceil(triggeredCDAdjusted * misc.ServerTickRate) / misc.ServerTickRate
            };
            // L583
            let triggerCDTickRounded = if triggeredBy(&mainSkill)
                .is_some_and(|tb| tb.extra.truthy(n!("ignoresTickRate")))
            {
                triggerCDAdjusted
            } else {
                m_ceil(triggerCDAdjusted * misc.ServerTickRate) / misc.ServerTickRate
            };
            // L584
            let actionCooldown = cooldownOverride.unwrap_or_else(|| {
                m_max(
                    triggerCD.unwrap_or(0.0) + addsCastTime.unwrap_or(0.0),
                    triggeredCD.unwrap_or(0.0) + addedCooldown.unwrap_or(0.0),
                )
            });
            // L585
            let actionCooldownAdjusted =
                cooldownOverride.unwrap_or_else(|| m_max(triggerCDAdjusted, triggeredCDAdjusted));
            // L586
            let actionCooldownTickRounded = match cooldownOverride {
                Some(co) => m_ceil(co * misc.ServerTickRate) / misc.ServerTickRate,
                None => m_max(triggerCDTickRounded, triggeredCDTickRounded),
            };

            // L588
            let sourceIsMain = source.as_ref().is_some_and(|s| Rc::ptr_eq(s, &mainSkill));
            let rateCap = if sourceIsMain {
                lor(mainData.get(n!("triggerRateCapOverride")), || {
                    Val::Num(m_huge)
                })
            } else {
                Val::Num(m_huge)
            };
            output.set(TRC, rateCap);
            // L589
            if actionCooldownTickRounded != 0.0 {
                // L590
                output.set(TRC, 1.0 / actionCooldownTickRounded);
            }
            let doomBlastSource = env.build.configTab.input.get(n!("doomBlastSource"));
            let isDoomBlast = config
                .triggerName
                .is_some_and(|t| t.as_str() == "Doom Blast");
            // L592
            if isDoomBlast && doomBlastSource.str().as_deref() == Some("expiration") {
                // L593
                let expirationRate = 1.0
                    / cache_output(&cache_req(env, uuid))
                        .get(n!("Duration"))
                        .num();
                // L594
                if breakdown.as_ref().is_some_and(|bd| bd.truthy(ESR)) {
                    let bd = breakdown.as_ref().unwrap();
                    // L595
                    let d = cache_output(&cache_req(env, uuid)).get(n!("Duration"));
                    idx(bd, ESR).set(1, lfmt!("1 / %.2f ^8(source curse duration)", fa(&d)));
                }
                // L597
                if expirationRate > num(trigRate, "local 'trigRate'") {
                    // L598
                    env.player.modDB.get().NewMod(
                        n!("UsesCurseOverlaps"),
                        ModType::FLAG,
                        true,
                        "Config",
                        0,
                        0,
                        &[],
                    );
                    // L599
                    if breakdown.as_ref().is_some_and(|bd| bd.truthy(ESR)) {
                        let bd = breakdown.as_ref().unwrap();
                        // L600
                        ins_at(bd, ESR, 2, lfmt!("max(%.2f, %.2f) ^8(If a curse expires instantly curse expiration is equivalent to curse replacement)", expirationRate, trigRate));
                        // L601
                        let srcName = match &source {
                            Some(s) => ge_of(s).name,
                            None => lua_error!("attempt to index local 'source' (a nil value)"),
                        };
                        ins_at(
                            bd,
                            ESR,
                            2,
                            lfmt!("%.2f ^8(%s cast rate)", trigRate, srcName),
                        );
                    }
                } else {
                    // L604
                    trigRate = Some(expirationRate);
                }
            } else if isDoomBlast && doomBlastSource.str().as_deref() == Some("hexblast") {
                // L607
                let mut hexBlast: Option<ActiveSkillRef> = None;
                let mut rate: Option<f64> = None;
                // L608
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L609
                    if ge_of(&skill).name.as_str() == "Hexblast"
                        && !isTriggered(&skill)
                        && !Rc::ptr_eq(&skill, &mainSkill)
                    {
                        // L610
                        let r = findTriggerSkill(env, &skill, hexBlast.take(), rate, None);
                        hexBlast = r.0;
                        rate = r.1;
                        uuid = r.2;
                    }
                });
                // L613
                if hexBlast.is_some() {
                    // L614
                    if let Some(bd) = &breakdown {
                        // L615
                        idx(bd, ESR).set(
                            1,
                            lfmt!(
                                "1 / (%.2f + %.2f) ^8(sum of triggered curse and hexblast cast time)",
                                1.0 / num(trigRate, "local 'trigRate'"),
                                1.0 / num(rate, "local 'rate'")
                            ),
                        );
                    }
                    // L617
                    trigRate = Some(
                        1.0 / (1.0 / num(trigRate, "local 'trigRate'")
                            + 1.0 / num(rate, "local 'rate'")),
                    );
                }
            }

            // L621
            if breakdown.as_ref().is_some_and(|bd| !bd.truthy(TRC)) {
                let bd = breakdown.as_ref().unwrap();
                // L622
                bd.set(TRC, Table::new());

                // L624
                if let Some(cooldownOverride) = cooldownOverride {
                    // L625
                    ins(
                        bd,
                        TRC,
                        lfmt!(
                            "%.2f ^8(hard override of cooldown of %s)",
                            cooldownOverride,
                            triggeredName
                        ),
                    );
                } else if triggeredCDAdjusted == 0.0 {
                    // L627
                    ins(
                        bd,
                        TRC,
                        format!("{triggeredName} has no base cooldown or cooldown override"),
                    );
                } else {
                    // L628 triggeredCDAdjusted ~= 0 triggered skill has some kind of cooldown
                    // L629
                    if let Some(triggeredCD) = triggeredCD {
                        // L630
                        ins(
                            bd,
                            TRC,
                            lfmt!("%.2f ^8(base cooldown of triggered skill)", triggeredCD),
                        );
                    } else {
                        // L632
                        ins(
                            bd,
                            TRC,
                            format!("{triggeredName} has no base cooldown or cooldown override"),
                        );
                    }
                    // L634
                    if let Some(addedCooldown) = addedCooldown {
                        // L635
                        ins(
                            bd,
                            TRC,
                            lfmt!("+ %.2f ^8(flat added cooldown)", addedCooldown),
                        );
                    }
                    // L637
                    ins(
                        bd,
                        TRC,
                        lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdr),
                    );
                    // L638
                    ins(
                        bd,
                        TRC,
                        lfmt!(
                            "= %.4f ^8(final cooldown of triggered skill)",
                            triggeredCDAdjusted
                        ),
                    );
                }

                // L641
                ins(bd, TRC, "");

                // L643 Minion triggers have internal triggers
                if !is_env_minion(env, &actor) {
                    // L644
                    if triggerCDAdjusted == 0.0 {
                        // L645
                        ins(
                            bd,
                            TRC,
                            lfmt!("Trigger rate based on %s cooldown", triggeredName),
                        );
                    } else {
                        // L646 triggerCDAdjusted ~= 0 trigger has some kind of cooldown
                        // L647
                        if let Some(triggerCD) = triggerCD {
                            // L648
                            ins(
                                bd,
                                TRC,
                                lfmt!(
                                    "%.2f ^8(base cooldown of %s)",
                                    triggerCD,
                                    fname(config.triggerName)
                                ),
                            );
                        } else {
                            // L650
                            ins(
                                bd,
                                TRC,
                                concat_name(config.triggerName, " has no base cooldown"),
                            );
                        }
                        // L652
                        if let Some(addsCastTime) = addsCastTime {
                            // L653
                            ins(
                                bd,
                                TRC,
                                lfmt!(
                                    "+ %.2f ^8(this skill adds cast time to cooldown when triggered)",
                                    addsCastTime
                                ),
                            );
                        }
                        // L655
                        ins(
                            bd,
                            TRC,
                            lfmt!("/ %.2f ^8(increased/reduced cooldown recovery)", icdr),
                        );
                        // L656
                        ins(
                            bd,
                            TRC,
                            lfmt!("= %.4f ^8(final cooldown of trigger)", triggerCDAdjusted),
                        );
                    }
                }

                // L660
                ins(bd, TRC, "");

                // L662
                if triggeredCDAdjusted != 0.0 && triggerCDAdjusted != 0.0 {
                    // L663
                    ins(
                        bd,
                        TRC,
                        lfmt!(
                            "%.3f ^8(biggest of trigger cooldown and triggered skill cooldown)",
                            actionCooldownAdjusted
                        ),
                    );
                }

                // L666
                let displayCooldownRounding = (!mainData.truthy(n!("ignoresTickRate"))
                    && triggeredCDAdjusted != 0.0)
                    || (triggeredBy(&mainSkill)
                        .is_some_and(|tb| !tb.extra.truthy(n!("ignoresTickRate")))
                        && triggerCDAdjusted != 0.0);
                // L667
                if displayCooldownRounding {
                    // L668
                    ins(
                        bd,
                        TRC,
                        lfmt!(
                            "%.3f ^8(adjusted for server tick rate)",
                            actionCooldownTickRounded
                        ),
                    );
                }

                // L671
                let extraIncreaseNeeded = |affectedCD: f64| -> Option<f64> {
                    // L672
                    if cooldownOverride.is_none() {
                        // L673
                        let nextBreakpoint = actionCooldownTickRounded - misc.ServerTickTime;
                        // L674
                        let timeOverBreakpoint = actionCooldownAdjusted - nextBreakpoint;
                        // L675
                        let alreadyReducedTime = actionCooldown - actionCooldownAdjusted;
                        // L676
                        if timeOverBreakpoint < affectedCD {
                            // L677
                            let divNeeded =
                                affectedCD / (affectedCD - timeOverBreakpoint - alreadyReducedTime);
                            // L678
                            let incTotal = m_ceil((divNeeded - 1.0) * 100.0);
                            // L679
                            return Some(incTotal - (icdr - 1.0) * 100.0);
                        }
                    }
                    None
                };

                // L684
                let extraICDRNeeded = extraIncreaseNeeded(actionCooldown);
                // L685
                if let Some(extraICDRNeeded) = extraICDRNeeded {
                    // L686
                    ins(
                        bd,
                        TRC,
                        lfmt!(
                            "^8(extra ICDR of %d%% would reach next breakpoint)",
                            extraICDRNeeded
                        ),
                    );
                }

                // L689
                let extraCSIncNeeded = addsCastTime.and_then(extraIncreaseNeeded);
                // L690
                if let Some(extraCSIncNeeded) = extraCSIncNeeded {
                    // L691
                    ins(
                        bd,
                        TRC,
                        lfmt!(
                            "^8(extra ICS  of %d%% would reach next breakpoint)",
                            extraCSIncNeeded
                        ),
                    );
                }

                // L694
                ins(bd, TRC, "");

                // L696
                if !(triggeredCD.is_some() || triggerCD.is_some() || cooldownOverride.is_some()) {
                    // L697
                    ins(bd, TRC, "Assuming cast on every kill/attack/hit");
                } else {
                    // L699
                    ins(bd, TRC, "Trigger rate:");
                    // L700
                    ins(bd, TRC, lfmt!("1 / %.3f", actionCooldownTickRounded));
                    // L701
                    ins(bd, TRC, lfmt!("= %.2f ^8per second", fa(&output.get(TRC))));
                }
            }

            let playerMain = main_skill(&env.player);
            // L705
            if ge_of(&playerMain).name.as_str() == "Doom Blast"
                && doomBlastSource.str().as_deref() == Some("vixen")
            {
                // L706
                let gloves = env.player.itemList.with(|l| l.get(&n!("Gloves")).cloned());
                if !gloves.is_some_and(|g| {
                    g.title
                        .get()
                        .is_some_and(|t| pob_lua::lmatch(&t, "Vixen's Entrapment", None).is_some())
                }) {
                    // L707
                    output.set(n!("VixenModeNoVixenGlovesWarn"), true);
                }

                // L710
                env.player.modDB.get().NewMod(
                    n!("UsesCurseOverlaps"),
                    ModType::FLAG,
                    true,
                    "Config",
                    0,
                    0,
                    &[],
                );
                // L711
                let vixens = env
                    .data
                    .skills
                    .get(&n!("SupportUniqueCastCurseOnCurse"))
                    .cloned();
                // L712
                let vixensCD: Option<f64> = vixens.map(|v| {
                    let cd = v
                        .levels
                        .get(1.0)
                        .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                        .num(n!("cooldown"));
                    num(cd, "field 'cooldown'") / icdr
                });
                // L713 (`vixensCD` nil reads as 0 inside: `triggerCD or 0`)
                let rotation = tbl![tbl! {
                    uuid = cacheSkillUUID(&playerMain, env),
                    icdr = icdr,
                }];
                let rate = calcMultiSpellRotationImpact(
                    env,
                    &rotation,
                    num(trigRate, "local 'sourceRate'"),
                    vixensCD.unwrap_or(0.0),
                    None,
                    &env.player,
                )
                .0;
                output.set(ESR, rate);
                // L714
                output.set(
                    n!("VixensTooMuchCastSpeedWarn"),
                    num(vixensCD, "local 'vixensCD'") > (1.0 / num(trigRate, "local 'trigRate'")),
                );
                // L715
                if let Some(bd) = &breakdown {
                    let vixensCD = num(vixensCD, "local 'vixensCD'");
                    // L716
                    ins(
                        bd,
                        ESR,
                        lfmt!(
                            "%.2f / %.2f = %.2f ^8(Vixen's trigger cooldown)",
                            vixensCD * icdr,
                            icdr,
                            vixensCD
                        ),
                    );
                    // L717
                    ins(bd, ESR, lfmt!("%.2f ^8(Simulated trigger rate of a curse socketed in Vixen's given ^7%.2f ^8CD and ^7%.2f ^8source rate)", fa(&output.get(ESR)), vixensCD, trigRate));
                }
            } else if trigRate.is_some()
                && !mainFlags.truthy(n!("globalTrigger"))
                && !config.extra.truthy(n!("ignoreSourceRate"))
            {
                // L720
                output.set(ESR, trigRate);
            } else {
                // L722
                output.set(ESR, output.get(TRC));
                // L723
                mainFlags.set(n!("globalTrigger"), true);
            }

            // L726
            if breakdown.is_some() && !mainData.truthy(n!("sourceRateIsFinal")) {
                // L727
                ins(
                    breakdown.as_ref().unwrap(),
                    ESR,
                    lfmt!("= %.2f ^8(Effective source rate)", fa(&output.get(ESR))),
                );
            }

            // L730
            let skillName: Name = match &source {
                Some(s) => ge_of(s).name,
                None => match triggeredBy(&mainSkill) {
                    Some(tb) => tb.grantedEffect.get().name,
                    None => ge_of(&mainSkill).name,
                },
            };

            // L732
            if !output.get(ESR).raw_eq(&Val::Num(0.0))
                && !playerMain.skillFlags.get().truthy(n!("skipEffectiveRate"))
            {
                // L733
                let mut triggerChance = 100.0;
                // L734
                let mut triggerChanceBreakdown: Vec<String> = Vec::new();

                // L736 Accuracy and crit chance
                // L737
                if source.as_ref().is_some_and(|s| {
                    has_type(s, SkillType::Melee) || has_type(s, SkillType::Attack)
                }) && cache(env, uuid).is_some()
                    && !config.triggerOnUse
                {
                    let src = source.as_ref().unwrap();
                    let srcName = ge_of(src).name;
                    let dualWield = env.player.weaponData1.get().truthy(n!("type"))
                        && env.player.weaponData2.get().truthy(n!("type"))
                        && src.skillData.get().truthy(n!("doubleHitsWhenDualWielding"));

                    // L739
                    let sourceHitChance = cache_req(env, uuid).HitChance.num_or(0.0);
                    // L740
                    if sourceHitChance != 100.0 {
                        // L741 Some skills hit with both weapons at the same time. Each weapon rolls accuracy and crit independently
                        // L742
                        if dualWield {
                            let o = cache_output(&cache_req(env, uuid));
                            // L743
                            let mainHandHit = idx(&o, n!("MainHand")).get(n!("HitChance")).num();
                            // L744
                            let offHandHit = idx(&o, n!("OffHand")).get(n!("HitChance")).num();
                            // L745
                            let bothHit = mainHandHit * offHandHit / 100.0;
                            // L746
                            let mainHandMiss = 100.0 - mainHandHit;
                            // L747
                            let offHandMiss = 100.0 - offHandHit;
                            // L748
                            let effectiveHitChance = bothHit
                                + mainHandHit * offHandMiss / 100.0
                                + mainHandMiss * offHandHit / 100.0;
                            // L749
                            triggerChance = triggerChance * effectiveHitChance / 100.0;
                            // L750
                            if breakdown.is_some() {
                                // L751
                                triggerChanceBreakdown.push(lfmt!("x %.2f%% ^8(%s effective hit chance for skills that hit with both weapons)", effectiveHitChance, srcName));
                            }
                        } else {
                            // L754
                            triggerChance = triggerChance * sourceHitChance / 100.0;
                            // L755
                            if breakdown.is_some() {
                                // L756
                                triggerChanceBreakdown.push(lfmt!(
                                    "x %.2f%% ^8(%s hit chance)",
                                    sourceHitChance,
                                    srcName
                                ));
                            }
                        }
                    }
                    // L760
                    if mainData.truthy(n!("triggerOnCrit")) {
                        // L761
                        let onCritChance = lor(mainData.get(n!("chanceToTriggerOnCrit")), || {
                            match cache(env, uuid) {
                                Some(e) => main_skill(&e.Env.player)
                                    .skillData
                                    .get()
                                    .get(n!("chanceToTriggerOnCrit")),
                                None => Val::Nil,
                            }
                        });
                        // L762
                        if config.triggerChance.is_none() {
                            config.triggerChance =
                                lor(mainData.get(n!("chanceToTriggerOnCrit")), || onCritChance)
                                    .opt_num();
                        }

                        // L764
                        let sourceCritChance = cache_req(env, uuid).CritChance.num_or(0.0);
                        // L765
                        if sourceCritChance != 100.0 {
                            // L766 Some skills hit with both weapons at the same time. Each weapon rolls accuracy and crit independently
                            // L767
                            if dualWield {
                                let o = cache_output(&cache_req(env, uuid));
                                // L768
                                let mainHandCrit =
                                    idx(&o, n!("MainHand")).get(n!("CritChance")).num();
                                // L769
                                let offHandCrit =
                                    idx(&o, n!("OffHand")).get(n!("CritChance")).num();
                                // L770
                                let bothHit = mainHandCrit * offHandCrit / 100.0;
                                // L771
                                let mainHandMiss = 100.0 - mainHandCrit;
                                // L772
                                let offHandMiss = 100.0 - offHandCrit;
                                // L773
                                let effectiveCritChance = bothHit
                                    + mainHandCrit * offHandMiss / 100.0
                                    + mainHandMiss * offHandCrit / 100.0;
                                // L774
                                triggerChance = triggerChance * effectiveCritChance / 100.0;
                                // L775
                                if breakdown.is_some() {
                                    // L776
                                    triggerChanceBreakdown.push(lfmt!("x %.2f%% ^8(%s effective crit chance for skills that hit with both weapons)", effectiveCritChance, srcName));
                                }
                            } else {
                                // L779
                                triggerChance = triggerChance * sourceCritChance / 100.0;
                                // L780
                                if breakdown.is_some() {
                                    // L781
                                    triggerChanceBreakdown.push(lfmt!(
                                        "x %.2f%% ^8(%s crit chance)",
                                        sourceCritChance,
                                        srcName
                                    ));
                                }
                            }
                        }
                    }
                }

                // L788 Trigger chance
                // L789
                if let Some(tc) = config.triggerChance.filter(|&tc| tc != 100.0) {
                    // L790
                    triggerChance = triggerChance * tc / 100.0;
                    // L791
                    if breakdown.is_some() {
                        // L792
                        triggerChanceBreakdown.push(lfmt!("x %.2f%% ^8(chance to trigger)", tc));
                    }
                }

                // L796 If the current triggered skill ignores tick rate and is the only triggered skill by this trigger use charge based calcs
                // L797 (`triggeredSkills[1] == packageSkillDataForSimulation(…)` compares a fresh
                // table: always false, but the call runs, with its `addsCastTime` flag write)
                if mainData.truthy(n!("ignoresTickRate"))
                    && (triggeredSkillCond.is_none()
                        || (triggeredSkills.len() == 1 && {
                            let fresh = packageSkillDataForSimulation(&mainSkill, env);
                            triggeredSkills.get(1).raw_eq(&Val::Table(fresh))
                        }))
                {
                    // L798
                    let overlaps = lor(stages_as_overlaps(config, &playerMain), || {
                        config.extra.get(n!("overlaps"))
                    });
                    // L799
                    output.set(
                        STR,
                        m_min(
                            output.get(TRC).num(),
                            output.get(ESR).num() * overlaps.num_or(1.0),
                        ),
                    );
                    // L800
                    if let Some(bd) = &breakdown {
                        // L801
                        if overlaps.truthy() {
                            // L802
                            bd.set(
                                STR,
                                tbl![lfmt!(
                                    "min(%.2f, %.2f *  %d) ^8(%d overlaps)",
                                    fa(&output.get(TRC)),
                                    fa(&output.get(ESR)),
                                    fa(&overlaps),
                                    fa(&overlaps)
                                )],
                            );
                        } else {
                            // L806
                            bd.set(
                                STR,
                                tbl![lfmt!(
                                    "min(%.2f, %.2f)",
                                    fa(&output.get(TRC)),
                                    fa(&output.get(ESR))
                                )],
                            );
                        }
                    }
                } else if mainFlags.truthy(n!("globalTrigger")) && triggeredSkillCond.is_none() {
                    // L811 Trigger does not use source rate breakpoints for one reason or another
                    // L812
                    output.set(STR, output.get(ESR));
                } else {
                    // L813 Triggers like Cast on Crit go through simulation to calculate the trigger rate of each skill in the trigger group
                    // L814 (`simBreakdown`: implicit global, kept local, CCR-0004)
                    let rotation = if triggeredSkillCond.is_some() {
                        triggeredSkills.clone()
                    } else {
                        tbl![packageSkillDataForSimulation(&mainSkill, env)]
                    };
                    let simTriggerCD = if !mainData.truthy(n!("triggeredByBrand")) {
                        triggerCD.or(triggeredCD)
                    } else {
                        None
                    };
                    let (rate, simBreakdown) = calcMultiSpellRotationImpact(
                        env,
                        &rotation,
                        output.get(ESR).num(),
                        simTriggerCD.unwrap_or(0.0),
                        Some(triggerChance),
                        &actor,
                    );
                    output.set(STR, rate);
                    // L815
                    let triggerBotsEffective =
                        actor.modDB.get().Flag(None, &[n!("HaveTriggerBots")])
                            && has_type(&mainSkill, SkillType::Spell);
                    // L816
                    if triggerBotsEffective {
                        // L817
                        output.set(STR, 2.0 * output.get(STR).num());
                    }

                    // L820 stagesAreOverlaps is the skill part which makes the stages behave as overlaps
                    // L821
                    let hits_per_cast =
                        lor(stages_as_overlaps(config, &playerMain), || Val::Num(1.0)).num();
                    // L822
                    output.set(STR, hits_per_cast * output.get(STR).num());
                    // L823
                    if let Some(bd) = &breakdown {
                        let esr = output.get(ESR).num();
                        let skillTriggerRate = output.get(STR).num();
                        let rateLabel = if mainData.truthy(n!("triggeredByBrand")) {
                            let srcName = match &source {
                                Some(s) => ge_of(s).name,
                                None => lua_error!("attempt to index local 'source' (a nil value)"),
                            };
                            lfmt!("%s activations per second", srcName)
                        } else if trigRate.is_none() {
                            lfmt!("%s triggers per second", skillName)
                        } else {
                            "Effective source rate".to_owned()
                        };
                        // L824
                        bd.set(
                            STR,
                            tbl![
                                lfmt!("%.2f ^8(%s)", esr, rateLabel.as_str()),
                                lfmt!("/ %.2f ^8(Estimated impact of skill rotation, cooldown alignment and trigger chance)", m_max(esr / skillTriggerRate, 1.0)),
                                lfmt!("= %.2f ^8per second", skillTriggerRate),
                            ],
                        );
                        // L829
                        if triggerBotsEffective {
                            // L830
                            ins_at(
                                bd,
                                STR,
                                3,
                                "x 2 ^8(Trigger bots effectively cause the skill to trigger twice)",
                            );
                        }
                        // L832
                        if hits_per_cast > 1.0 {
                            // L833
                            ins_at(
                                bd,
                                STR,
                                3,
                                lfmt!("x %.2f ^8(hits per triggered skill cast)", hits_per_cast),
                            );
                        }
                        // L835
                        if triggerChance != 100.0 {
                            // L836
                            ins_at(bd, STR, 1, "");
                            // L837
                            ins_at(
                                bd,
                                STR,
                                1,
                                lfmt!("= %.2f%% ^8(Effective chance to trigger)", triggerChance),
                            );
                            // L838
                            for line in &triggerChanceBreakdown {
                                // L839
                                ins_at(bd, STR, 1, line.as_str());
                            }
                            // L841
                            ins_at(bd, STR, 1, "100% ^8(Base chance)");
                        }
                        // L843
                        if simBreakdown.truthy(n!("extraSimInfo")) {
                            // L844
                            ins(bd, STR, "");
                            // L845
                            ins(bd, STR, simBreakdown.get(n!("extraSimInfo")));
                        }
                        // L847-869
                        sim_data(bd, &simBreakdown);
                        // L870
                        let simTime = simBreakdown.get(n!("simTime"));
                        ins(
                            bd,
                            n!("SimData"),
                            lfmt!(
                                "Simulation duration: %.2f ^8(In game source skill usage duration in seconds)",
                                fa(&simTime),
                                fa(&simTime)
                            ),
                        );
                    }
                }
            } else {
                // L874
                if let Some(bd) = &breakdown {
                    // L875
                    bd.set(
                        STR,
                        tbl![lfmt!(
                            "The trigger needs to be triggered for any skill to be triggered."
                        )],
                    );
                }
                // L879
                output.set(STR, 0.0);
            }
            // L881
            mainData.set(n!("triggerRate"), output.get(STR));

            // L883 Account for Trigger-related INC/MORE modifiers
            // L884
            output.set(n!("Speed"), mainData.get(n!("triggerRate")));
            // L885
            addTriggerIncMoreMods(&mainSkill, source.as_ref().unwrap_or(&mainSkill));
            // L886
            if let Some(src) = source.as_ref().filter(|s| !Rc::ptr_eq(s, &mainSkill)) {
                // L887
                mainData.set(n!("triggerSourceUUID"), cacheSkillUUID(src, env));
                // L888
                let srcName = ge_of(src).name;
                let prefix = match config.extra.get(n!("customTriggerName")) {
                    v if v.truthy() => v.str().map_or_else(
                        || lua_error!("attempt to concatenate field 'customTriggerName'"),
                        |s| s.to_string(),
                    ),
                    _ => {
                        let base = match config.triggerName {
                            Some(t) if t != srcName => t,
                            _ => triggeredName,
                        };
                        format!(
                            "{base}{}",
                            if is_env_minion(env, &actor) {
                                "'s attack Trigger: "
                            } else {
                                "'s Trigger: "
                            }
                        )
                    }
                };
                mainSkill
                    .infoMessage
                    .set(Some(LStr::from(format!("{prefix}{srcName}").as_str())));
            } else {
                // L890
                let msg = match triggeredBy(&mainSkill) {
                    Some(tb) => tb.grantedEffect.get().name.as_str().to_owned(),
                    None => concat_name(config.triggerName, " Trigger"),
                };
                mainSkill.infoMessage.set(Some(LStr::from(msg.as_str())));
            }
            // L892
            if mainData.truthy(n!("triggeredByBrand")) {
                // L893
                let attachedBrandCount = trig_by(&mainSkill).extra.get(n!("attachedBrandCount"));
                // L894
                let msg = format!(
                    "{}:{}",
                    mainSkill.infoMessage.get().as_deref().unwrap_or(""),
                    lfmt!(
                        "%d attached Brand%s",
                        fa(&attachedBrandCount),
                        if attachedBrandCount.raw_eq(&Val::Num(1.0)) {
                            ""
                        } else {
                            "s"
                        }
                    )
                );
                mainSkill.infoMessage.set(Some(LStr::from(msg.as_str())));
            }

            // L897
            mainSkill
                .infoTrigger
                .set(config.triggerName.map(|t| LStr::from(t.as_str())));
        }
    }
    None
}
