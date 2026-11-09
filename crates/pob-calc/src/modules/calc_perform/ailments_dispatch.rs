//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! perform segment PF-G (CalcPerform.lua 3497-3990): ailment effects, enemy exposures,
//! ally Life pools, defence/offence/triggers/mirages dispatch, party-tab buff exports,
//! main-skill gem level/quality, cacheData.
//!
//! Owner: PF-G.
// literal Lua shapes (`x = x * y`, `&*s` names)
#![allow(
    non_snake_case,
    clippy::nonminimal_bool,
    clippy::too_many_lines,
    clippy::assign_op_pattern,
    clippy::explicit_auto_deref
)]

use pob_gamedata::mod_tools::createMod;
use pob_gamedata::tag;

use super::actor_a::{doActorAttribsConditions, doActorLifeMana};
use super::actor_b::{
    addMinionModifiers, allyLifePools, doActorCharges, doActorMisc, initMinionModDB,
};
use super::PerformCtx;
use crate::calcs;
use crate::classes::mod_store::arith;
use crate::lua_table::TableField;
use crate::modules::calc_tools as calcLib;
use crate::modules::common::{cacheData, cacheSkillUUID};
use crate::prelude::*;

// ---- callee replay seam (test-only, feature `probe`; CCR-0039) ---------------------------------

/// CCR-0039: the L4 test replays callees that are not ported yet (calc doc §9.2 "Replay":
/// "PF-G calling `offence`") by applying their recorded `in → out` state delta in place of the
/// call. The frozen code has no interception point, so until CCR-0039 moves this seam into the
/// facade it lives here, compiled only with the oracle's `probe` feature (the shipped calc has
/// no hook). A hook returns `Some(ret)` when it replayed the call (`ret` = the callee's packed
/// results `{ n = k, … }`, or nil), `None` to let the Rust callee run.
#[cfg(feature = "probe")]
pub mod replay_hook {
    use std::cell::RefCell;

    use crate::actor::ActorRef;
    use crate::env::EnvRef;
    use crate::lua_table::Val;

    /// `(callee, env, actor) -> Some(ret)` when replayed. `callee` is the function probe id
    /// (`defence`, `buildDefenceEstimations`, `triggers`, `mirages`, `offence`,
    /// `doActorCharges`, `doActorMisc`); `actor` is the actor the call is for.
    pub type Hook = Box<dyn FnMut(&str, &EnvRef, &ActorRef) -> Option<Val>>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Install (or with `None` remove) the hook; returns the previous one.
    pub fn set(h: Option<Hook>) -> Option<Hook> {
        HOOK.with(|c| c.replace(h))
    }

    pub(super) fn try_replay(callee: &str, env: &EnvRef, actor: &ActorRef) -> Option<Val> {
        let mut h = HOOK.with(|c| c.borrow_mut().take())?;
        let r = h(callee, env, actor);
        HOOK.with(|c| {
            let mut slot = c.borrow_mut();
            if slot.is_none() {
                *slot = Some(h);
            }
        });
        r
    }
}

/// Run a callee, or (feature `probe`, hook installed) replay it. Returns the callee's packed
/// results as a Lua value (`Nil` when it returns nothing). CCR-0039.
#[allow(unused_variables)]
fn callee(name: &'static str, env: &EnvRef, actor: &ActorRef, f: impl FnOnce() -> Val) -> Val {
    #[cfg(feature = "probe")]
    if let Some(ret) = replay_hook::try_replay(name, env, actor) {
        return ret;
    }
    f()
}

// ---- Lua value helpers (plumbing) ---------------------------------------------------------------

/// `tostring(v)` of an output value (`"Resist=" .. tostring(output.X)`).
fn tostr(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        other => lua_error!("PF-G: tostring of a {} value", other.lua_type()),
    }
}

/// `t[k]` that Lua then indexes (nil → "attempt to index a nil value").
fn tindex(t: &Table, k: Name, what: &str) -> Table {
    t.table(k)
        .unwrap_or_else(|| lua_error!("attempt to index field '{what}' (a nil value)"))
}

/// A `pool.life` / `pool.redirect` / `pool.limit` string as a name (`nil` → None).
fn tname(t: &Table, k: Name) -> Option<Name> {
    t.str(k).map(|s| Name::from(&*s))
}

/// The mods of a `modList` value: a ModList object (`ipairs(modList)`), or an array table of
/// mods.
fn modlist_ipairs(v: &Val) -> Vec<ModRef> {
    if let Some(store) = v.obj::<ModStore>() {
        return store.ipairs();
    }
    match v {
        Val::Table(t) => {
            let mut out = Vec::new();
            for_ipairs!((_, m) in t => {
                match m {
                    Val::Mod(m) => out.push(m),
                    other => lua_error!("PF-G: a modList entry that is a {}", other.lua_type()),
                }
            });
            out
        }
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `store:NewMod(name, type, value, source, …)` where `source` may be nil (createMod then
/// leaves `source` unset, ModTools.lua:38-41): `NewMod`'s own body (ModStore.lua:110) with
/// an optional source.
fn new_mod(
    store: &ModStore,
    name: Name,
    ty: ModType,
    value: impl Into<Value>,
    source: Option<&str>,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) {
    store.AddMod(Arc::new(createMod(
        name,
        ty,
        value,
        source,
        flags,
        keywordFlags,
        tags,
    )));
}

/// `enemyDB.mods[name] and enemyDB.mods[name][1].value` as a truth value (an empty bucket
/// raises "attempt to index a nil value", as `[1]` is nil).
fn first_mod_value_truthy(db: &ModStore, name: Name) -> bool {
    match db.db_len(name) {
        None => false,
        Some(_) => match db.db_at(name, 1) {
            Some(m) => m.value.truthy(),
            None => lua_error!("attempt to index field '?' (a nil value)"),
        },
    }
}

/// `"Condition:" .. tag.var` (a nil var raises).
fn cat_var(prefix: &str, var: Option<Name>) -> Name {
    match var {
        Some(v) => cat!(prefix, v.as_str()),
        None => lua_error!("attempt to concatenate field 'var' (a nil value)"),
    }
}

/// Mine aura preStack (CalcPerform.lua 3876-3886 / 3892-3902): the copied aura's mods with the
/// `ActiveMineCount` stack tag removed and the value multiplied by `stackCount`. §3.6: Lua
/// mutates the deep copy made by `copyTable` at 3870/3888; here each mod is rebuilt and the
/// copy's `modList` gets a fresh ModList holding the rebuilt mods (the shared original is
/// untouched).
fn prestack_mine_mods(aura: &Table, stackCount: f64) {
    let modList = aura.get(n!("modList"));
    let mut rebuilt: Vec<ModRef> = Vec::new();
    // L3877 / L3893
    for m in modlist_ipairs(&modList) {
        let mut out: Option<Mod> = None;
        // L3878 / L3894
        for (i, tag) in m.tags.iter().enumerate() {
            // L3879 / L3895
            if tag.effectStackVar.map_or("", |s| s.as_str()) == "ActiveMineCount" {
                let mut nm = (*m).clone();
                // L3880 / L3896
                nm.tags[i].effectStackVar = None;
                // L3881 / L3897
                nm.value = Value::Num(arith(&m.value, "field 'value'") * stackCount);
                out = Some(nm);
                // L3882 / L3898
                break;
            }
        }
        rebuilt.push(out.map_or_else(|| m.clone(), Arc::new));
    }
    if modList.obj::<ModStore>().is_some() {
        let fresh = ModStore::ModList(None);
        for m in rebuilt {
            fresh.AddMod(m);
        }
        aura.set(n!("modList"), Val::Obj(fresh as Rc<dyn std::any::Any>));
    } else if let Val::Table(t) = &modList {
        // the deep copy's own list: replace its entries in place
        for (i, m) in rebuilt.into_iter().enumerate() {
            t.set(i as i64 + 1, Val::Mod(m));
        }
    }
}

/// @pob Modules/CalcPerform.lua:3497-3990 h=8e00c6d3
/// PF-G: CalcPerform.lua 3497-3990 (ailment effects, defence/offence/triggers/mirages dispatch, cacheData).
pub(crate) fn seg(ctx: &mut PerformCtx) -> Flow {
    let env = ctx.env.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let output = ctx.output.clone();
    let breakdown = ctx.breakdown.clone();
    let hasGuaranteedBonechill = ctx.hasGuaranteedBonechill;
    let player = env.player.clone();
    let mainSkill = || {
        player
            .mainSkill
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
    };

    // L3497 Calculate maximum and apply the strongest non-damaging ailments
    // L3498
    let ailmentData = &env.data.nonDamagingAilment;
    // L3499-3570: `local ailments = { Chill = { condition, mods = function(num) … }, … }`.
    // Each `mods` closure is the arm of `ailment_mods` below (same statements, same order).
    let ailment_mods = |ailment: &str, num: f64| -> Vec<Mod> {
        match ailment {
            "Chill" => {
                // L3503
                let mut mods = vec![createMod(
                    n!("ActionSpeed"),
                    ModType::INC,
                    -num,
                    Some("Chill"),
                    0,
                    0,
                    &[tag!({ type = Condition, var = "Chilled" })],
                )];
                // L3504
                if modDB.Flag(None, nm!["ChillEffectIncDamageTaken"]) {
                    // L3505
                    mods.push(createMod(
                        n!("DamageTaken"),
                        ModType::INC,
                        num,
                        Some("Ahuana's Bite"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Chilled" })],
                    ));
                } else if modDB.Flag(None, nm!["ChillEffectIncColdDamageTaken"]) {
                    // L3506-3507
                    mods.push(createMod(
                        n!("ColdDamageTaken"),
                        ModType::INC,
                        num,
                        Some("Chilled by Hits"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Chilled" })],
                    ));
                } else if modDB.Flag(None, nm!["ChillingAreaIncColdDamageTaken"]) {
                    // L3508-3509
                    mods.push(createMod(
                        n!("ColdDamageTaken"),
                        ModType::INC,
                        num,
                        Some("Chilling Area"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Chilled" })],
                    ));
                } else if output.truthy(n!("HasBonechill"))
                    && (hasGuaranteedBonechill
                        || enemyDB.Sum(ModType::BASE, None, nm!["ChillVal"]) > 0.0)
                {
                    // L3510-3511
                    mods.push(createMod(
                        n!("ColdDamageTaken"),
                        ModType::INC,
                        num,
                        Some("Bonechill"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Chilled" })],
                    ));
                }
                // L3513
                if modDB.Flag(None, nm!["ChillEffectLessDamageDealt"]) {
                    // L3514
                    mods.push(createMod(
                        n!("Damage"),
                        ModType::MORE,
                        -num / 2.0,
                        Some("Shaper of Winter"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Chilled" })],
                    ));
                }
                // L3516
                mods
            }
            "Shock" => {
                // L3522
                let mut mods = Vec::new();
                // L3523
                if modDB.Flag(None, nm!["ShockCanStack"]) {
                    // L3524
                    let limit = {
                        let o = modDB.Override(None, nm!["ShockStacksMax"]);
                        if o.truthy() {
                            arith(&o, "a value")
                        } else {
                            modDB.Sum(ModType::BASE, None, nm!["ShockStacksMax"])
                        }
                    };
                    mods.push(createMod(
                        n!("DamageTaken"),
                        ModType::INC,
                        num,
                        Some("Shock"),
                        0,
                        0,
                        &[
                            tag!({ type = Condition, var = "Shocked" }),
                            tag!({ type = Multiplier, var = "ShockStacks", limit = limit }),
                        ],
                    ));
                    // L3525
                    let stacks = enemyDB.Sum(ModType::BASE, None, nm!["Multiplier:ShockStacks"]);
                    let max = {
                        let o = modDB.Override(None, nm!["ShockStacksMax"]);
                        if o.truthy() {
                            arith(&o, "a value")
                        } else {
                            modDB.Sum(ModType::BASE, None, nm!["ShockStacksMax"])
                        }
                    };
                    let v = num * m_min(stacks, max);
                    output.set(n!("CurrentShock"), v);
                    // L3526
                    if breakdown.is_some() {
                        // L3527
                        let limit = {
                            let o = modDB.Override(None, nm!["ShockStacksMax"]);
                            if o.truthy() {
                                arith(&o, "a value")
                            } else {
                                modDB.Sum(ModType::BASE, None, nm!["ShockStacksMax"])
                            }
                        };
                        mods.push(createMod(
                            n!("DamageTakenByShock"),
                            ModType::INC,
                            num,
                            Some("Shock Stacks"),
                            0,
                            0,
                            &[
                                tag!({ type = Condition, var = "Shocked" }),
                                tag!({ type = Multiplier, var = "ShockStacks", limit = limit }),
                            ],
                        ));
                    }
                } else {
                    // L3530
                    mods.push(createMod(
                        n!("DamageTaken"),
                        ModType::INC,
                        num,
                        Some("Shock"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Shocked" })],
                    ));
                }
                // L3532
                mods
            }
            "Scorch" => {
                // L3538
                let mut mods = Vec::new();
                // L3539
                if modDB.Flag(None, nm!["ScorchCanStack"]) {
                    // L3540
                    let limit = {
                        let o = modDB.Override(None, nm!["ScorchStacksMax"]);
                        if o.truthy() {
                            arith(&o, "a value")
                        } else {
                            modDB.Sum(ModType::BASE, None, nm!["ScorchStacksMax"])
                        }
                    };
                    mods.push(createMod(
                        n!("ElementalResist"),
                        ModType::BASE,
                        -num,
                        Some("Scorch"),
                        0,
                        0,
                        &[
                            tag!({ type = Condition, var = "Scorched" }),
                            tag!({ type = Multiplier, var = "ScorchStacks", limit = limit }),
                        ],
                    ));
                    // L3541
                    let stacks = enemyDB.Sum(ModType::BASE, None, nm!["Multiplier:ScorchStacks"]);
                    let max = {
                        let o = modDB.Override(None, nm!["ScorchStacksMax"]);
                        if o.truthy() {
                            arith(&o, "a value")
                        } else {
                            modDB.Sum(ModType::BASE, None, nm!["ScorchStacksMax"])
                        }
                    };
                    let v = num * m_min(stacks, max);
                    output.set(n!("CurrentScorch"), v);
                    // L3542
                    if breakdown.is_some() {
                        // L3543
                        let limit = {
                            let o = modDB.Override(None, nm!["ScorchStacksMax"]);
                            if o.truthy() {
                                arith(&o, "a value")
                            } else {
                                modDB.Sum(ModType::BASE, None, nm!["ScorchStacksMax"])
                            }
                        };
                        mods.push(createMod(
                            n!("ElementalResistByScorch"),
                            ModType::BASE,
                            -num,
                            Some("Scorch Stacks"),
                            0,
                            0,
                            &[
                                tag!({ type = Condition, var = "Scorched" }),
                                tag!({ type = Multiplier, var = "ScorchStacks", limit = limit }),
                            ],
                        ));
                    }
                } else {
                    // L3546
                    mods.push(createMod(
                        n!("ElementalResist"),
                        ModType::BASE,
                        -num,
                        Some("Scorch"),
                        0,
                        0,
                        &[tag!({ type = Condition, var = "Scorched" })],
                    ));
                }
                // L3548
                mods
            }
            // L3554
            "Brittle" => vec![createMod(
                n!("SelfCritChance"),
                ModType::BASE,
                num,
                Some("Brittle"),
                0,
                0,
                &[tag!({ type = Condition, var = "Brittle" })],
            )],
            // L3560
            "Sap" => vec![createMod(
                n!("Damage"),
                ModType::MORE,
                -num,
                Some("Sap"),
                0,
                0,
                &[tag!({ type = Condition, var = "Sapped" })],
            )],
            _ => unreachable!("PF-G: no ailment {ailment}"),
        }
    };

    // L3565 LUA-ORDER: `pairs(ailments)` over a string-keyed table (PoB's order is random per
    // process, D8); insertion order of the constructor here. Only the order of mods within
    // `DamageTaken` (Chill/Shock) and `Damage` MORE (Chill/Sap) of enemyDB can depend on it.
    for (ailment, condition) in [
        ("Chill", "Chilled"),
        ("Shock", "Shocked"),
        ("Scorch", "Scorched"),
        ("Brittle", "Brittle"),
        ("Sap", "Sapped"),
    ] {
        let aBase = cat!(ailment, "Base");
        let aOverride = cat!(ailment, "Override");
        let aMinimum = cat!(ailment, "Minimum");
        // L3566-3568
        // LUA-QUIRK: `enemyDB:Sum(…) > 0 or modDB:Sum(…)` is always true (a Sum is a number,
        // and every number is truthy); the second Sum only runs when the first test fails.
        if (enemyDB.Sum(ModType::BASE, None, &[cat!(ailment, "Val")]) > 0.0 || {
            let _ = modDB.Sum(ModType::BASE, None, &[aBase, aOverride, aMinimum]);
            true
        }) && !(enemyDB.Flag(None, &[cat!("Condition:Already", condition)])
            || enemyDB.Flag(
                None,
                &[cat!(ailment, "Immune"), n!("ElementalAilmentImmune")],
            )
            || enemyDB.Sum(
                ModType::BASE,
                None,
                &[
                    cat!("Avoid", ailment),
                    n!("AvoidAilments"),
                    n!("AvoidElementalAilments"),
                ],
            ) >= 100.0)
        {
            // L3569
            let mut r#override = 0.0;
            // L3570
            let mut minimum = 0.0;
            // L3571
            for_ipairs!((_, value) in modDB.Tabulate(Some(ModType::BASE), None, &[aBase, aOverride, aMinimum]) => {
                // L3572
                let r#mod = value.r#mod.clone();
                // L3573
                let mut effect = arith(&r#mod.value, "local 'effect'");
                // L3574
                let scalesWithSource = r#mod.name == aBase || r#mod.name == aMinimum;
                // L3575
                if r#mod.name == aOverride {
                    // L3576
                    new_mod(&enemyDB, cat!("Condition:", condition), ModType::FLAG, true, r#mod.source.as_deref(), 0, 0, &[]);
                }
                // L3578
                if scalesWithSource {
                    // L3579 If the main skill can inflict the ailment, the ailment is inflicted with a hit, and we have a node allocated that checks what our highest damage is, then
                    // L3580 use the skill's ailment modifiers
                    // L3581 if not, use the generic modifiers
                    // L3582 Scorch/Sap/Brittle do not have guaranteed sources from hits, and therefore will only end up in this bit of code if it's not supposed to apply the skillModList, which is bad
                    // L3583
                    let ms = mainSkill();
                    if ailment != "Scorch"
                        && ailment != "Sap"
                        && ailment != "Brittle"
                        && !ms.skillModList.get().Flag(None, &[cat!("Cannot", ailment)])
                        && ms.skillFlags.get().truthy(n!("hit"))
                        && modDB.Flag(None, nm!["ChecksHighestDamage"])
                    {
                        // L3584
                        effect = effect * calcLib::r#mod(&ms.skillModList.get(), None, &[cat!("Enemy", ailment, "Effect")]);
                    } else {
                        // L3586
                        effect = effect * calcLib::r#mod(&modDB, None, &[cat!("Enemy", ailment, "Effect")]);
                    }
                }
                // L3589
                effect = effect * calcLib::r#mod(&enemyDB, None, &[cat!("Self", ailment, "Effect")]);
                // L3590
                if scalesWithSource {
                    // L3591
                    new_mod(&modDB, aOverride, ModType::BASE, effect, r#mod.source.as_deref(), r#mod.flags, r#mod.keywordFlags, &r#mod.tags);
                    // L3592
                    if r#mod.name == aMinimum {
                        // L3593
                        minimum = minimum + effect;
                    }
                }
                // L3596
                r#override = m_max(m_max(r#override, effect), minimum);
            });
            // L3598
            let aMax = cat!(ailment, "Max");
            let mut maxAilment = {
                let o = modDB.Override(None, &[aMax]);
                if o.truthy() {
                    arith(&o, "local 'maxAilment'")
                } else {
                    0.0
                }
            };
            // L3599
            if !modDB.Override(None, &[aMax]).truthy() {
                // L3600
                for_ipairs!((_, skill) in player.activeSkillList.get() => {
                    // L3601
                    let skillMax = {
                        let o = modDB.Override(None, &[aMax]);
                        if o.truthy() {
                            arith(&o, "local 'skillMax'")
                        } else {
                            let ad = ailmentData
                                .get(&Name::from(ailment))
                                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"));
                            ad.max + skill.baseSkillModList.get().Sum(ModType::BASE, None, &[aMax])
                        }
                    };
                    // L3602
                    maxAilment = if skillMax > maxAilment { skillMax } else { maxAilment };
                });
            }
            // L3605
            output.set(cat!("Maximum", ailment), maxAilment);
            // L3606
            let precision = ailmentData
                .get(&Name::from(ailment))
                .unwrap_or_else(|| lua_error!("attempt to index field '?' (a nil value)"))
                .precision;
            let cur = m_floor(
                m_min(
                    m_max(
                        r#override,
                        enemyDB.Sum(ModType::BASE, None, &[cat!(ailment, "Val")]),
                    ),
                    output.num(cat!("Maximum", ailment)),
                ) * lpow(10.0, precision),
            ) / lpow(10.0, precision);
            output.set(cat!("Current", ailment), cur);
            // L3607
            for r#mod in ailment_mods(ailment, output.num(cat!("Current", ailment))) {
                // L3608
                enemyDB.AddMod(Arc::new(r#mod));
            }
            // L3610 Prevents ailment from applying doubly for minions
            // (the 4th argument is the tag table, so the mod has no source: ModTools.lua:38-49)
            new_mod(
                &enemyDB,
                cat!("Condition:Already", condition),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[tag!({ type = Condition, var = condition })],
            );
        }
    }

    // L3614 Update chill and shock multipliers
    // L3615
    let chillEffectMultiplier = enemyDB.Sum(ModType::BASE, None, nm!["Multiplier:ChillEffect"]);
    // L3616
    if output.truthy(n!("CurrentChill")) && chillEffectMultiplier < output.num(n!("CurrentChill")) {
        // L3617
        enemyDB.NewMod(
            n!("Multiplier:ChillEffect"),
            ModType::BASE,
            output.num(n!("CurrentChill")) - chillEffectMultiplier,
            "",
            0,
            0,
            &[],
        );
    }
    // L3619
    let shockEffectMultiplier = enemyDB.Sum(ModType::BASE, None, nm!["Multiplier:ShockEffect"]);
    // L3620
    if output.truthy(n!("CurrentShock")) && shockEffectMultiplier < output.num(n!("CurrentShock")) {
        // L3621
        enemyDB.NewMod(
            n!("Multiplier:ShockEffect"),
            ModType::BASE,
            output.num(n!("CurrentShock")) - shockEffectMultiplier,
            "",
            0,
            0,
            &[],
        );
    }

    // L3624
    let enemy = env.enemy.clone();
    callee("doActorCharges", &env, &enemy, || {
        doActorCharges(&env, &enemy);
        Val::Nil
    });
    // L3625
    callee("doActorMisc", &env, &enemy, || {
        doActorMisc(&env, &enemy);
        Val::Nil
    });

    // L3627
    let treeVersion = env.spec.get().treeVersion.get();
    let (major, minor) = match pob_lua::lmatch(&treeVersion, "(%d+)_(%d+)", None) {
        Some(c) => (c[0].num(), c[1].num()),
        None => (None, None),
    };

    // L3629 Apply exposures
    // L3630
    for element in ["Fire", "Cold", "Lightning"] {
        let tonumber = |x: Option<f64>| {
            x.unwrap_or_else(|| lua_error!("bad argument #1 to 'tonumber' (value expected)"))
        };
        // L3631-3636
        if tonumber(major) <= 3.0 && tonumber(minor) <= 15.0 // Elemental Equilibrium pre-3.16 does not remove Exposure effects
            || !modDB.Flag(None, nm!["ElementalEquilibrium"]) // if Elemental Equilibrium isn't active we just process Exposure normally
            || element == "Fire" && !enemyDB.Flag(None, nm!["Condition:HitByFireDamage"])
            || element == "Cold" && !enemyDB.Flag(None, nm!["Condition:HitByColdDamage"])
            || element == "Lightning" && !enemyDB.Flag(None, nm!["Condition:HitByLightningDamage"])
        {
            // L3636
            let mut min = m_huge;
            // L3637
            let mut source: Option<AStr> = Some(AStr::from(""));
            // L3638
            for_ipairs!((_, r#mod) in enemyDB.Tabulate(Some(ModType::BASE), None, &[cat!(element, "Exposure")]) => {
                let v = arith(&r#mod.value, "field 'value'");
                // L3639
                if v < min {
                    // L3640
                    min = v;
                    // L3641
                    source = r#mod.r#mod.source.clone();
                }
            });
            // L3644
            if min != m_huge {
                // L3646 Modify the magnitude of all exposures
                for_ipairs!((_, r#mod) in modDB.Tabulate(Some(ModType::BASE), None, &[n!("ExtraExposure"), cat!("Extra", element, "Exposure")]) => {
                    // L3647
                    min = min + arith(&r#mod.value, "field 'value'");
                });
                // L3649 Scale the resulting magnitude by increased effect of Exposure you inflict
                // L3650
                let exposureEffectInc = modDB.Sum(
                    ModType::INC,
                    None,
                    &[n!("ExposureEffect"), cat!(element, "ExposureEffect")],
                );
                // L3651
                if exposureEffectInc != 0.0 {
                    // L3652
                    min = min * (1.0 + exposureEffectInc / 100.0);
                }
                // L3654
                enemyDB.NewMod(
                    cat!("Condition:Has", element, "Exposure"),
                    ModType::FLAG,
                    true,
                    "",
                    0,
                    0,
                    &[],
                );
                // L3655
                // LUA-QUIRK: `ModStore:Override` returns no value at all when no OVERRIDE mod
                // matches (ModDB.lua:248-251 falls off the end), so the call as the last
                // argument makes this `m_min(min)` = `min`; only a found value is compared.
                let exposureMin = modDB.Override(None, nm!["ExposureMin"]);
                let resist = match &exposureMin {
                    Value::Nil => min,
                    v => m_min(
                        min,
                        v.num().unwrap_or_else(|| {
                            lua_error!(
                                "bad argument #2 to 'min' (number expected, got {})",
                                v.lua_type()
                            )
                        }),
                    ),
                };
                new_mod(
                    &enemyDB,
                    cat!(element, "Resist"),
                    ModType::BASE,
                    resist,
                    source.as_deref(),
                    0,
                    0,
                    &[],
                );
                // L3656
                modDB.NewMod(
                    n!("Condition:AppliedExposureRecently"),
                    ModType::FLAG,
                    true,
                    "",
                    0,
                    0,
                    &[],
                );
            }
        }
    }

    // L3661 Handle consecrated ground effects on enemies
    // L3662
    if enemyDB.Flag(None, nm!["Condition:OnConsecratedGround"]) {
        // L3663
        let effect = 1.0 + modDB.Sum(ModType::INC, None, nm!["ConsecratedGroundEffect"]) / 100.0;
        // L3664
        enemyDB.NewMod(
            n!("DamageTaken"),
            ModType::INC,
            m_floor(enemyDB.Sum(ModType::INC, None, nm!["DamageTakenConsecratedGround"]) * effect),
            "Consecrated Ground",
            0,
            0,
            &[],
        );
    }

    // L3667 Full DPS and builds without a supported redirect do not need ally Life.
    // The ally Life pool tables are PF-B's module-level locals (CalcPerform.lua 1256-1272),
    // reached through `allyLifePools()` (keys: the five local names; CCR-0039 §3).
    let pools = allyLifePools();
    let minionLifePoolBySkill = pools.table(n!("minionLifePoolBySkill"));
    let spectreLifePool = pools.table(n!("spectreLifePool"));
    let companionshipLifePool = pools.table(n!("companionshipLifePool"));
    let totemLifePoolByStat = pools.table(n!("totemLifePoolByStat"));
    let allyLifeRedirects: Vec<Name> = pools
        .table(n!("allyLifeRedirects"))
        .map(|t| {
            t.ipairs()
                .into_iter()
                .filter_map(|(_, v)| v.str().map(|s| Name::from(&*s)))
                .collect()
        })
        .unwrap_or_default();
    let is_spectre_pool = |p: &Table| spectreLifePool.as_ref().is_some_and(|s| s.ptr_eq(p));
    // L3668
    let needsAllyLife = !ctx.skipEHP && modDB.Sum(ModType::BASE, None, &allyLifeRedirects) != 0.0;
    // L3669
    player.allyLifeList.set(Some(Table::new()));
    let allyLifeList = || {
        player
            .allyLifeList
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'allyLifeList' (a nil value)"))
    };
    // L3670
    if needsAllyLife {
        // L3671
        let calculatedLifePool = Table::new();
        // L3672
        let mut spectreCount = 0.0;
        // L3673 `local firstSpectre` = { life = …, entry = … }
        let mut firstSpectre: Option<(f64, Table)> = None;
        // L3674
        let spectreLimit = m_min(
            output.num_or(n!("ActiveSpectreLimit"), 0.0),
            ctx.summonedMinionOverride.unwrap_or(m_huge),
        );
        // L3675
        for_ipairs!((_, activeSkill) in player.activeSkillList.get() => {
            // L3676
            let minion = activeSkill.minion.get();
            let skillFlags = activeSkill.skillFlags.get();
            // L3677
            let Some(minion) = minion else { continue; };
            if skillFlags.truthy(n!("disable")) || activeSkill.skillTypes.get().with(|t| t.has(SkillType::MinionsAreUndamagable)) {
                continue;
            }
            // L3678
            let grantedEffectName = activeSkill.activeEffect.get().grantedEffect.get().name;
            let skillPool: Option<Table> = match (skillFlags.truthy(n!("spectre")), &spectreLifePool) {
                (true, Some(s)) => Some(s.clone()),
                _ => minionLifePoolBySkill.as_ref().and_then(|t| t.table(grantedEffectName)),
            };
            // L3679
            let mut pools: Vec<Table> = skillPool.iter().cloned().collect();

            // L3681 Companionship is stored on the supported skill, which identifies the one minion it affects.
            // L3682
            let mut hasCompanionship = false;
            let companionRedirect = companionshipLifePool.as_ref().and_then(|p| tname(p, n!("redirect")));
            // L3683
            for_ipairs!((_, buff) in activeSkill.buffList.get() => {
                // L3684
                for r#mod in modlist_ipairs(&buff.get(n!("modList"))) {
                    // L3685
                    if Some(r#mod.name) == companionRedirect {
                        // L3686
                        hasCompanionship = true;
                        // L3687
                        break;
                    }
                }
                // L3690
                if hasCompanionship {
                    // L3691
                    break;
                }
            });
            // L3694
            if hasCompanionship {
                // L3695
                if let Some(c) = &companionshipLifePool {
                    pools.push(c.clone());
                }
                // L3696
                if let Some(sp) = &skillPool {
                    let spRedirect = tname(sp, n!("redirect"));
                    if modDB.Sum(ModType::BASE, None, &names([spRedirect])) != 0.0
                        && modDB.Sum(ModType::BASE, None, &names([companionRedirect])) != 0.0
                    {
                        // L3697 Both redirects spend the same minion's Life, so they must share one pool.
                        // L3698
                        pools = companionshipLifePool.iter().cloned().collect();
                        // L3699
                        let life = sp.str(n!("life")).unwrap_or_else(|| lua_error!("attempt to concatenate field 'life' (a nil value)"));
                        modDB.NewMod(cat!("MinionLifeShares", &*life), ModType::FLAG, true, "Companionship", 0, 0, &[]);
                    }
                }
            }

            // L3703
            let mut minionLife = Val::Nil;
            // L3704
            for pool in &pools {
                let poolLife = tname(pool, n!("life"));
                // L3705-3707
                let canAdd = ((is_spectre_pool(pool) && spectreCount < spectreLimit)
                    || (!is_spectre_pool(pool) && !poolLife.is_some_and(|l| calculatedLifePool.truthy(l))))
                    && modDB.Sum(ModType::BASE, None, &names([tname(pool, n!("redirect"))])) != 0.0
                    && !modDB.Override(None, &names([poolLife])).truthy();
                // L3708
                if canAdd {
                    // L3709
                    if !minionLife.truthy() {
                        // L3710
                        if !env.minion.get().is_some_and(|m| Rc::ptr_eq(&m, &minion)) {
                            // L3711
                            initMinionModDB(&env, &activeSkill, None);
                            // L3712
                            let skillCfg = activeSkill.skillCfg.get();
                            addMinionModifiers(&activeSkill.skillModList.get(), Some(&skillCfg), &minion);
                            // L3713 LUA-ORDER: string-keyed `buffs` (insertion order here)
                            for (_, modList) in ctx.buffs.pairs() {
                                // L3714
                                let ml = modList.obj::<ModStore>().unwrap_or_else(|| lua_error!("PF-G: buffs entry is not a ModList"));
                                addMinionModifiers(&ml, Some(&skillCfg), &minion);
                            }
                            // L3716 LUA-ORDER: string-keyed `minionBuffs`
                            for (_, modList) in ctx.minionBuffs.pairs() {
                                // L3717
                                minion.modDB.get().AddList(&modlist_ipairs(&modList));
                            }
                            // L3719
                            for name in minion.modDB.get().List(None, nm!["Keystone"]) {
                                let Some(name) = name.as_str().map(Name::from) else { continue; };
                                let keystoneMap = env.spec.get().tree.get().keystoneMap.clone();
                                // L3720
                                if keystoneMap.truthy(name) {
                                    // L3721
                                    let node = keystoneMap.obj::<Node>(name).unwrap_or_else(|| lua_error!("PF-G: keystoneMap entry is not a node"));
                                    minion.modDB.get().AddList(&node.modList.get().ipairs());
                                }
                            }
                            // L3724
                            doActorAttribsConditions(&env, &minion);
                        } else {
                            // L3726
                            doActorLifeMana(&minion);
                        }
                        // L3728
                        minionLife = minion.output.get().get(n!("Life"));
                    }

                    // L3731 Void Spawn redirects scale from their summon limit, so their Life pool uses that same limit.
                    // L3732
                    let mut count = match tname(pool, n!("limit")) {
                        Some(limit) => {
                            let o = output.get(limit);
                            if o.truthy() { o.num() } else { calcLib::val(&activeSkill.skillModList.get(), limit, None) }
                        }
                        None => 1.0,
                    };
                    // L3733
                    if is_spectre_pool(pool) {
                        // L3734
                        count = 1.0;
                        // L3735
                        spectreCount = spectreCount + 1.0;
                    }
                    // L3737
                    let life = minionLife.num() * count;
                    // L3738
                    let poolLife = poolLife.unwrap_or_else(|| lua_error!("PF-G: pool.life is nil"));
                    new_mod(&modDB, poolLife, ModType::BASE, life, pool.str(n!("source")).as_deref(), 0, 0, &[]);
                    // L3739
                    let list = allyLifeList();
                    let _ = list.get_or_new_table(poolLife);
                    // L3740
                    let minionName = minion.minionData.get().map(|d| d.name).unwrap_or_else(|| lua_error!("attempt to index field 'minionData' (a nil value)"));
                    let entry = Table::from_ctor(vec![
                        TableField::named(n!("name"), false, minionName),
                        TableField::named(n!("life"), false, life),
                        TableField::named(n!("count"), false, count),
                    ]);
                    // L3741
                    tindex(&list, poolLife, "?").push(entry.clone());
                    // L3742
                    if is_spectre_pool(pool) {
                        // L3743
                        if firstSpectre.is_none() {
                            firstSpectre = Some((minionLife.num(), entry));
                        }
                    } else {
                        // L3745
                        calculatedLifePool.set(poolLife, true);
                    }
                }
            }
        });
        // L3751
        if let Some((fsLife, fsEntry)) = &firstSpectre {
            if spectreCount < spectreLimit {
                // L3752 Each Raise Spectre group represents one Spectre; the first group fills any slots left over.
                // L3753
                let count = spectreLimit - spectreCount;
                // L3754
                let life = fsLife * count;
                // L3755
                fsEntry.set(n!("count"), fsEntry.num(n!("count")) + count);
                // L3756
                fsEntry.set(n!("life"), fsEntry.num(n!("life")) + life);
                // L3757
                let sp = spectreLifePool.as_ref().unwrap_or_else(|| {
                    lua_error!("attempt to index upvalue 'spectreLifePool' (a nil value)")
                });
                new_mod(
                    &modDB,
                    tname(sp, n!("life")).unwrap_or_else(|| lua_error!("PF-G: pool.life is nil")),
                    ModType::BASE,
                    life,
                    sp.str(n!("source")).as_deref(),
                    0,
                    0,
                    &[],
                );
            }
        }

        // L3760
        let mut normalTotem: Option<Table> = None;
        // L3761
        let mut vaalRejuvenationTotem: Option<Table> = None;
        // L3762
        for_ipairs!((_, activeSkill) in player.activeSkillList.get() => {
            let skillFlags = activeSkill.skillFlags.get();
            // L3763
            if skillFlags.truthy(n!("totem"))
                && !skillFlags.truthy(n!("disable"))
                && activeSkill.skillData.get().truthy(n!("totemLevel"))
                && activeSkill.skillTotemId.get().is_some()
            {
                // L3764
                let (life, _) = calcs::calcTotemLife(&env, &activeSkill);
                // L3765
                let name = activeSkill.activeEffect.get().grantedEffect.get().name;
                let totem = Table::from_ctor(vec![
                    TableField::named(n!("name"), false, name),
                    TableField::named(n!("life"), false, life),
                ]);
                // L3766
                if name == "Vaal Rejuvenation Totem" {
                    // L3767
                    vaalRejuvenationTotem = Some(totem);
                } else if normalTotem.as_ref().map_or(true, |t| life > t.num(n!("life"))) {
                    // L3768-3769
                    normalTotem = Some(totem);
                }
            }
        });

        // L3774 PoB cannot know which Totem is nearest, so use the eligible type with the most Life.
        // L3775
        let nearestTotem: Option<Table> = match &vaalRejuvenationTotem {
            Some(v)
                if normalTotem
                    .as_ref()
                    .map_or(true, |n| v.num(n!("life")) > n.num(n!("life"))) =>
            {
                Some(v.clone())
            }
            _ => normalTotem.clone(),
        };
        // L3776
        let totemPool = totemLifePoolByStat
            .as_ref()
            .and_then(|t| t.table(n!("TotalTotemLife")))
            .unwrap_or_else(|| lua_error!("attempt to index local 'totemPool' (a nil value)"));
        // L3777
        let vaalPool = totemLifePoolByStat
            .as_ref()
            .and_then(|t| t.table(n!("TotalVaalRejuvenationTotemLife")))
            .unwrap_or_else(|| lua_error!("attempt to index local 'vaalPool' (a nil value)"));
        // L3778
        let totemLifeOverride = modDB.Override(None, nm!["TotalTotemLife"]);
        // L3779
        let totemRedirect = modDB.Sum(
            ModType::BASE,
            None,
            &names([tname(&totemPool, n!("redirect"))]),
        );
        // L3780
        let nearestIsVaal = nearestTotem
            .as_ref()
            .is_some_and(|n| vaalRejuvenationTotem.as_ref().is_some_and(|v| v.ptr_eq(n)))
            && !totemLifeOverride.truthy();
        // L3781
        if let Some(nearest) = &nearestTotem {
            if !nearestIsVaal && totemRedirect != 0.0 && !totemLifeOverride.truthy() {
                // L3782
                new_mod(
                    &modDB,
                    n!("TotalTotemLife"),
                    ModType::BASE,
                    nearest.num(n!("life")),
                    totemPool.str(n!("source")).as_deref(),
                    0,
                    0,
                    &[],
                );
                // L3783
                allyLifeList().set(n!("TotalTotemLife"), tbl![nearest.clone()]);
            }
        }
        // L3785
        if let Some(vaal) = &vaalRejuvenationTotem {
            if (modDB.Sum(
                ModType::BASE,
                None,
                &names([tname(&vaalPool, n!("redirect"))]),
            ) != 0.0
                || nearestIsVaal && totemRedirect != 0.0)
                && !modDB
                    .Override(None, nm!["TotalVaalRejuvenationTotemLife"])
                    .truthy()
            {
                // L3786
                new_mod(
                    &modDB,
                    n!("TotalVaalRejuvenationTotemLife"),
                    ModType::BASE,
                    vaal.num(n!("life")),
                    vaalPool.str(n!("source")).as_deref(),
                    0,
                    0,
                    &[],
                );
                // L3787
                allyLifeList().set(n!("TotalVaalRejuvenationTotemLife"), tbl![vaal.clone()]);
            }
        }
    }

    // L3791 Defence/offence calculations
    // L3792
    callee("defence", &env, &player, || {
        calcs::defence(&env, &player);
        Val::Nil
    });
    // L3793
    if !ctx.skipEHP {
        // L3794
        callee("buildDefenceEstimations", &env, &player, || {
            calcs::buildDefenceEstimations(&env, &player);
            Val::Nil
        });
    }

    // L3797
    callee("triggers", &env, &player, || {
        calcs::triggers(&env, Some(&player));
        Val::Nil
    });
    // L3798
    // CCR-0039 §2: `calcs.mirages(env)` returns a value Lua tests (`return not
    // config.calcMainSkillOffence`, CalcMirages.lua:57) but the frozen Rust signature returns
    // `()`; until the CCR is applied the Rust call counts as returning nil.
    let miragesRet = callee("mirages", &env, &player, || {
        calcs::mirages(&env);
        Val::Nil
    });
    let miragesRet = match &miragesRet {
        Val::Table(r) => r.get(1),
        other => other.clone(),
    };
    if !miragesRet.truthy() {
        // L3799
        let ms = mainSkill();
        callee("offence", &env, &player, || {
            calcs::offence(&env, &player, &ms);
            Val::Nil
        });
    }

    // L3802
    if let Some(minion) = env.minion.get() {
        // L3803
        callee("defence", &env, &minion, || {
            calcs::defence(&env, &minion);
            Val::Nil
        });
        // L3804 main.build.calcsTab.input.showMinion and -- should be disabled unless "calcsTab.input.showMinion" is true
        if !ctx.skipEHP {
            // L3805
            callee("buildDefenceEstimations", &env, &minion, || {
                calcs::buildDefenceEstimations(&env, &minion);
                Val::Nil
            });
        }
        // L3807
        callee("triggers", &env, &minion, || {
            calcs::triggers(&env, Some(&minion));
            Val::Nil
        });
        // L3808
        let ms = minion
            .mainSkill
            .get()
            .unwrap_or_else(|| lua_error!("bad argument #3 to 'offence' (mainSkill is nil)"));
        callee("offence", &env, &minion, || {
            calcs::offence(&env, &minion, &ms);
            Val::Nil
        });
    }

    // L3811 Export modifiers to enemy conditions and stats for party tab
    // L3812
    if ctx.partyTabEnableExportBuffs {
        let buffExports = ctx.buffExports.clone();
        let find = |s: &str, p: &str| pob_lua::find(s, p, None, false).is_some();
        // L3813 LUA-ORDER: `pairs(enemyDB.mods)` (string keys; random in PoB, sorted here). Only
        // the insertion order of `buffExports.EnemyConditions` / `EnemyMods` keys depends on it.
        for k in enemyDB.mod_names() {
            let ks = k.as_str();
            // L3814
            if find(ks, "Condition") && !find(ks, "Party") {
                // L3815
                tindex(&buffExports, n!("EnemyConditions"), "EnemyConditions").set(k, true);
            } else if (find(ks, "Resist") && !find(ks, "Totem") && !find(ks, "Max"))
                || find(ks, "Damage")
                || find(ks, "ActionSpeed")
                || find(ks, "SelfCrit")
                || (find(ks, "Multiplier") && !find(ks, "Max") && !find(ks, "Impale"))
            {
                // L3817
                for v in enemyDB.mods_named(k) {
                    // L3818
                    let src = || {
                        v.source.clone().unwrap_or_else(|| {
                            lua_error!("attempt to index field 'source' (a nil value)")
                        })
                    };
                    let isZero = matches!(v.value, Value::Num(x) if x == 0.0);
                    if !v.extra_get(n!("party")).is_some_and(Value::truthy)
                        && !isZero
                        && v.source.as_deref() != Some("EnemyConfig")
                        && v.source.as_deref() != Some("Base")
                        && !find(&src(), "Delirious")
                        && !find(&src(), "^Party")
                    {
                        // L3819
                        let mut skipValue = false;
                        // L3820
                        for tag in &v.tags {
                            // L3821
                            if matches!(
                                tag.effectType.as_deref(),
                                Some("Curse") | Some("AuraDebuff")
                            ) {
                                // L3822
                                skipValue = true;
                                // L3823
                                break;
                            }
                        }
                        // L3826-3828
                        let pass = !skipValue
                            && match v.tags.first() {
                                None => true,
                                Some(t1) => {
                                    (t1.r#type != TagType::Condition
                                        || (t1.var.is_some()
                                            && first_mod_value_truthy(
                                                &enemyDB,
                                                cat_var("Condition:", t1.var),
                                            ))
                                        || (t1.varList.is_some()
                                            && first_mod_value_truthy(
                                                &enemyDB,
                                                cat_var(
                                                    "Condition:",
                                                    t1.varList
                                                        .as_ref()
                                                        .and_then(|l| l.first().copied()),
                                                ),
                                            )))
                                        && (t1.r#type != TagType::Multiplier
                                            || first_mod_value_truthy(
                                                &enemyDB,
                                                cat_var("Multiplier:", t1.var),
                                            ))
                                }
                            };
                        if pass {
                            let enemyMods = tindex(&buffExports, n!("EnemyMods"), "EnemyMods");
                            // L3829
                            let cur = enemyMods.get(k);
                            if cur.truthy() {
                                // L3830
                                let multi =
                                    matches!(&cur, Val::Table(t) if t.truthy(n!("MultiStat")));
                                if !multi {
                                    // L3831
                                    enemyMods.set(
                                        k,
                                        Table::from_ctor(vec![
                                            TableField::named(n!("MultiStat"), true, true),
                                            TableField::item(false, cur.clone()),
                                        ]),
                                    );
                                }
                                // L3833
                                match enemyMods.get(k) {
                                    Val::Table(t) => t.push(Val::Mod(v.clone())),
                                    _ => lua_error!("bad argument #1 to 'insert' (table expected)"),
                                }
                            } else {
                                // L3835
                                enemyMods.set(k, Val::Mod(v.clone()));
                            }
                        }
                    }
                }
            }
        }

        let playerMods = tindex(&buffExports, n!("PlayerMods"), "PlayerMods");
        // L3843
        for damageType in ["Physical", "Lightning", "Cold", "Fire", "Chaos"] {
            // L3844
            if env
                .modDB
                .get()
                .Flag(None, &[cat!("Enemy", damageType, "ResistEqualToYours")])
                && output.truthy(cat!(damageType, "Resist"))
            {
                // L3845
                playerMods.set(cat!("Enemy", damageType, "ResistEqualToYours"), true);
                // L3846
                playerMods.set(
                    cat!(
                        damageType,
                        "Resist=",
                        tostr(&output.get(cat!(damageType, "Resist"))).as_str()
                    ),
                    true,
                );
            }
        }
        // L3849
        if env
            .modDB
            .get()
            .Flag(None, nm!["PartyMemberMaximumEnduranceChargesEqualToYours"])
            && output.truthy(n!("EnduranceChargesMax"))
        {
            // L3850
            playerMods.set(n!("PartyMemberMaximumEnduranceChargesEqualToYours"), true);
            // L3851
            playerMods.set(
                cat!(
                    "EnduranceChargesMax=",
                    tostr(&output.get(n!("EnduranceChargesMax"))).as_str()
                ),
                true,
            );
        }

        // L3854
        playerMods.set(
            cat!(
                "MovementSpeedMod|percent|max=",
                tostring_num(output.num(n!("MovementSpeedMod")) * 100.0).as_str()
            ),
            true,
        );

        // L3856
        let auras = tindex(&buffExports, n!("Aura"), "Aura");
        let extraAura = tindex(&auras, n!("extraAura"), "extraAura");
        for r#mod in modlist_ipairs(&extraAura.get(n!("modList"))) {
            // L3857 leaving comment to make it easier for future similar mods
            // L3858 if mod.name:match("Parent") then
            // L3859 	ConPrintTable(mod)
            // L3860 end
            // L3861
            if r#mod.name == "YourFortifyEqualToParent" {
                // L3862
                playerMods.set(n!("PartyMemberFortifyEqualToYours"), true);
                // L3863
                playerMods.set(
                    cat!(
                        "FortificationStacks=",
                        tostring_num(output.num_or(n!("FortificationStacks"), 0.0)).as_str()
                    ),
                    true,
                );
            }
        }

        // L3867 preStack Mine auras
        // L3868 LUA-ORDER: string-keyed `buffExports.Aura` (insertion order here); each entry is
        // handled on its own, so the result does not depend on it.
        for (auraKey, _) in auras.pairs() {
            let Some(auraName) = auraKey.name() else {
                continue;
            };
            let an = auraName.as_str();
            // L3869
            if pob_lua::lmatch(an, "Mine", None).is_some()
                && pob_lua::lmatch(an, " Limit", None).is_none()
            {
                // L3870
                let copied = tindex(&auras, auraName, "?").copyTable(false);
                auras.set(auraName, copied);
                // L3871
                let aura = tindex(&auras, auraName, "?");
                // L3872
                let enemyMods = tindex(&buffExports, n!("EnemyMods"), "EnemyMods");
                let stackKey = cat!("Multiplier:", an, "Stack");
                let stackCount = match enemyMods.get(stackKey) {
                    Val::Nil | Val::Bool(false) => 0.0,
                    Val::Mod(m) => {
                        let v = Val::from_value(&m.value);
                        if v.truthy() {
                            v.num()
                        } else {
                            0.0
                        }
                    }
                    Val::Table(t) => {
                        let v = t.get(n!("value"));
                        if v.truthy() {
                            v.num()
                        } else {
                            0.0
                        }
                    }
                    other => lua_error!("attempt to index a {} value", other.lua_type()),
                };
                // L3873
                enemyMods.set(stackKey, Val::Nil);
                // L3874
                if stackCount == 0.0 {
                    // L3875
                    auras.set(auraName, Val::Nil);
                } else {
                    // L3876-3886
                    prestack_mine_mods(&aura, stackCount);
                }
                // L3887
                let limitName = cat!(an, " Limit");
                if auras.truthy(limitName) {
                    // L3888
                    let copied = tindex(&auras, limitName, "?").copyTable(false);
                    auras.set(limitName, copied);
                    // L3889
                    let aura = tindex(&auras, limitName, "?");
                    // L3890
                    if stackCount == 0.0 {
                        // L3891
                        auras.set(limitName, Val::Nil);
                    } else {
                        // L3892-3902
                        prestack_mine_mods(&aura, stackCount);
                    }
                }
            }
        }

        // L3907 LUA-ORDER: string-keyed `buffExports.Link`; only sets keys of PlayerMods.
        let links = tindex(&buffExports, n!("Link"), "Link");
        for (linkKey, link) in links.pairs() {
            // L3908
            if linkKey.name().is_some_and(|n| n == "Flame Link") {
                // L3909
                playerMods.set(cat!("Life=", tostr(&output.get(n!("Life"))).as_str()), true);
            }
            // L3911
            for r#mod in modlist_ipairs(&link.get(n!("modList"))) {
                // L3912
                if pob_lua::lmatch(&r#mod.name, "Parent", None).is_some() {
                    // L3913
                    if r#mod.name == "MaximumBlockAttackChanceIsEqualToParent" {
                        // L3914
                        playerMods.set(
                            cat!(
                                "BlockChanceMax=",
                                tostr(&output.get(n!("BlockChanceMax"))).as_str()
                            ),
                            true,
                        );
                    } else if r#mod.name == "BlockAttackChanceIsEqualToParent" {
                        // L3915-3916
                        playerMods.set(
                            cat!(
                                "BlockChance=",
                                tostr(&output.get(n!("BlockChance"))).as_str()
                            ),
                            true,
                        );
                    } else if r#mod.name == "MaximumLifeLeechIsEqualToPartyMember" {
                        // L3917-3918
                        playerMods.set(
                            cat!(
                                "MaxLifeLeechRatePercent=",
                                tostr(&output.get(n!("MaxLifeLeechRatePercent"))).as_str()
                            ),
                            true,
                        );
                    } else if r#mod.name == "TakenFromParentESBeforeYou" {
                        // L3919-3920
                        playerMods.set(
                            cat!(
                                "EnergyShieldRecoveryCap=",
                                tostr(&output.get(n!("EnergyShieldRecoveryCap"))).as_str()
                            ),
                            true,
                        );
                    } else if r#mod.name == "MainHandCritIsEqualToParent" {
                        // L3921-3922
                        if output.truthy(n!("MainHand")) {
                            // L3923
                            let mh = tindex(&output, n!("MainHand"), "MainHand");
                            playerMods.set(
                                cat!(
                                    "MainHand.CritChance=",
                                    tostr(&mh.get(n!("CritChance"))).as_str()
                                ),
                                true,
                            );
                        } else {
                            // L3924-3925 `elseif env.player.weaponData1 then`: weaponData1 is a
                            // table on every actor once initEnv ran, so always truthy
                            playerMods.set(
                                cat!(
                                    "MainHand.CritChance=",
                                    tostr(&player.weaponData1.get().get(n!("CritChance"))).as_str()
                                ),
                                true,
                            );
                        }
                    }
                }
            }
        }
        // L3931
        env.build.partyTab.setBuffExports(&buffExports);
    }

    // L3935
    if let Some(mainSkill) = player.mainSkill.get() {
        // L3936 (`local mainSkill = env.player.mainSkill`)
        let activeEffect = mainSkill.activeEffect.get();
        // L3937
        if activeEffect.srcInstance.get().is_some() {
            let skillCfg = mainSkill.skillCfg.get();
            let skillModList = mainSkill.skillModList.get();
            // L3939 calculate Gem Level of MainSkill
            // L3940 (`activeEffect.level` is a number on every skill effect, so always truthy)
            {
                // L3941
                let baseLevel = skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemLevel"]);
                // L3942
                let totalItemLevel =
                    skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemItemLevel"]);
                // L3943
                let totalSupportLevel =
                    skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemSupportLevel"]);

                // L3945
                output.set(n!("GemHasLevel"), true);
                // L3946
                output.set(
                    n!("GemLevel"),
                    baseLevel + totalSupportLevel + totalItemLevel,
                );

                // L3948
                if let Some(bd) = player.breakdown.get() {
                    // L3949
                    let t = Table::new();
                    bd.t.set(n!("GemLevel"), t.clone());
                    // L3950
                    t.push(lfmt!("%d ^8(level from gem)", baseLevel));
                    // L3951
                    if totalSupportLevel > 0.0 {
                        // L3952
                        t.push(lfmt!("+ %d ^8(level from support)", totalSupportLevel));
                    }
                    // L3954
                    if totalItemLevel > 0.0 {
                        // L3955
                        t.push(lfmt!("+ %d ^8(level from items)", totalItemLevel));
                    }
                    // L3957
                    t.push(lfmt!("= %d", output.num(n!("GemLevel"))));
                }
            }

            // L3961 calculate Gem Quality of MainSkill
            // L3962 (`activeEffect.quality` is a number on every skill effect, so always truthy)
            {
                // L3963
                let baseQuality =
                    skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemQuality"]);
                // L3964
                let totalItemQuality =
                    skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemItemQuality"]);
                // L3965
                let totalSupportQuality =
                    skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemSupportQuality"]);
                // L3966
                let socketQuality =
                    skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["GemSocketQuality"]);

                // L3968
                output.set(n!("GemHasQuality"), true);
                // L3969
                output.set(
                    n!("GemQuality"),
                    baseQuality + totalSupportQuality + totalItemQuality + socketQuality,
                );

                // L3971
                if let Some(bd) = player.breakdown.get() {
                    // L3972
                    let t = Table::new();
                    bd.t.set(n!("GemQuality"), t.clone());
                    // L3973
                    t.push(lfmt!("%d ^8(quality from gem)", baseQuality));
                    // L3974
                    if totalSupportQuality > 0.0 {
                        // L3975
                        t.push(lfmt!("+ %d ^8(quality from support)", totalSupportQuality));
                    }
                    // L3977
                    if totalItemQuality > 0.0 {
                        // L3978
                        t.push(lfmt!("+ %d ^8(quality from items)", totalItemQuality));
                    }
                    // L3980
                    if socketQuality > 0.0 {
                        // L3981
                        t.push(lfmt!("+ %d ^8(quality from socket)", socketQuality));
                    }
                    // L3983
                    t.push(lfmt!("= %d", output.num(n!("GemQuality"))));
                }
            }
        }
    }

    // L3989
    let ms = mainSkill();
    cacheData(cacheSkillUUID(&ms, &env), &env);
    Flow::Normal
}
