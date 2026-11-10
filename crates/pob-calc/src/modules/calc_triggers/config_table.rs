//! @pob-file Modules/CalcTriggers.lua @16de4b82d
//! CalcTriggers.lua 902-1627: `configTable` (82 arms, as one match in Lua order) and `calcs.triggers`.
//!
//! Owner: TR-B.
//!
//! Notes:
//! * `configTable[key] and configTable[key](env)`: a key without an arm and an arm that returns
//!   nil are both `None` (the caller only tests the result).
//! * The configs are built per CCR-0087: `triggeredSkillCond`, the closure `comparer` and
//!   `sourceWeapon = true` are typed `TriggerConfig` fields, and the other untyped keys
//!   (`trigRate`, `uuid`, `triggeredSkills`, `overlaps`, `customTriggerName`, `sourceName`,
//!   `ignoreSourceRate`) are `extra` entries under their Lua names. A Lua table constructor
//!   field whose value is nil is not stored.
//! * `triggeredBy.mainSkill` (an active skill stored on the triggering skill effect) is
//!   `triggeredBy.extra.mainSkill` as a `Val::Obj` (the handlers read it there, TR-A).
//! * `NewMod` with a tag table in the source position (`DealNo*` mods of Snipe, 1449-1453) has a
//!   nil source (ModTools.lua:36-41): `NewMod(…, None, …)` (CCR-0063).
#![allow(non_snake_case)]
// closures keep the Lua parameter lists `function(env, skill)`
#![allow(unused_variables)]
// literal Lua statement shapes
#![allow(
    clippy::nonminimal_bool,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::too_many_lines,
    clippy::float_cmp,
    clippy::needless_late_init,
    clippy::redundant_else,
    clippy::len_zero,
    clippy::explicit_auto_deref
)]

use super::handlers::{
    defaultComparer, defaultTriggerHandler, findTriggerSkill, helmetFocusHandler, isTriggered,
    packageSkillDataForSimulation, slotMatch, CWCHandler, ComparerFn, SkillCondFn,
};
use super::TriggerConfig;
use crate::modules::calc_setup::init_env_skills::grantedEffect_fromItem;
use crate::modules::calc_tools as calcLib;
use crate::modules::calcs::buildActiveSkill;
use crate::modules::common::cacheSkillUUID;
use crate::prelude::*;
use pob_gamedata::tag;

/// The `configTable` keys, in Lua order (CalcTriggers.lua 902-1582).
pub const CONFIG_TABLE_KEYS: [&str; 82] = [
    "law of the wilds",
    "the rippling thoughts",
    "the surging thoughts",
    "the hidden blade",
    "replica eternity shroud",
    "shroud of the lightless",
    "limbsplit",
    "the cauteriser",
    "duskblight",
    "lioneye's paws",
    "replica lioneye's paws",
    "moonbender's wing",
    "ngamahu's flame",
    "cameria's avarice",
    "starcaller",
    "uul-netol's embrace",
    "rigwald's crest",
    "jorrhast's blacksteel",
    "ashcaller",
    "arakaali's fang",
    "sporeguard",
    "mark of the elder",
    "mark of the shaper",
    "poet's pen",
    "maloney's mechanism",
    "asenath's chant",
    "vixen's entrapment",
    "flames of judgement",
    "storm of judgement",
    "trigger craft",
    "kitava's thirst",
    "foulborn kitava's thirst",
    "mjolner",
    "wing of the wyvern",
    "cospri's malice",
    "seven teachings",
    "squirming terror",
    "kinetic flux",
    "cast on critical strike",
    "cast on melee kill",
    "nova",
    "cast when damage taken",
    "cast when stunned",
    "cast on ward break",
    "spellslinger",
    "call to arms",
    "automation",
    "autoexertion",
    "seize the flesh",
    "fissure",
    "mark on hit",
    "hextouch",
    "oskarm",
    "tempest shield",
    "shattershard",
    "battlemage's cry",
    "arcanist brand",
    "cast on death",
    "combust",
    "prismatic burst",
    "voidstorm",
    "shockwave",
    "void shockwave",
    "falling crystal",
    "call the pyre",
    "manaforged arrows",
    "doom blast",
    "cast while channelling",
    "focus",
    "snipe",
    "avenging flame",
    "intuitive link",
    "svalinn cast on block",
    "festering resentment cast on block",
    "hex on trap",
    "supporttriggerfirespellonhit",
    "ghostly artillery",
    "replica gifts from above",
    "BloodShrineUniqueTriggeredExplodingToad",
    "bursting toad",
    "TriggeredMoltenStrike",
    "FieryImpactHeistMaceImplicit",
];

// ---- oracle plumbing (feature `probe`): which arms ran -------------------------------------------

#[cfg(feature = "probe")]
thread_local! {
    static ARMS_RUN: std::cell::RefCell<Vec<(Name, bool)>> = const { std::cell::RefCell::new(Vec::new()) };
}

#[cfg(feature = "probe")]
fn arm_called(key: Name) -> Option<usize> {
    CONFIG_TABLE_KEYS.contains(&key.as_str()).then(|| {
        ARMS_RUN.with(|a| {
            let mut a = a.borrow_mut();
            a.push((key, false));
            a.len() - 1
        })
    })
}

#[cfg(feature = "probe")]
fn arm_returned(call: Option<usize>) {
    if let Some(i) = call {
        ARMS_RUN.with(|a| a.borrow_mut()[i].1 = true);
    }
}

#[cfg(not(feature = "probe"))]
#[inline]
fn arm_called(_key: Name) -> Option<usize> {
    None
}

#[cfg(not(feature = "probe"))]
#[inline]
fn arm_returned(_call: Option<usize>) {}

/// The configTable arms called since the last call, each with whether it returned a config
/// (oracle tests; feature `probe`).
#[cfg(feature = "probe")]
pub fn take_arms_run() -> Vec<(Name, bool)> {
    ARMS_RUN.with(|a| std::mem::take(&mut *a.borrow_mut()))
}

// ---- small Lua shapes ---------------------------------------------------------------------------

/// `env.player.mainSkill` (an index of nil raises).
fn pmain(env: &EnvRef) -> ActiveSkillRef {
    env.player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `skill.skillTypes[t]` (truthiness).
fn has(skill: &ActiveSkillRef, t: SkillType::T) -> bool {
    skill.skillTypes.get().with(|s| s.has(t))
}

/// `skill.skillCfg.flags`.
fn flags(skill: &ActiveSkillRef) -> u32 {
    skill.skillCfg.get().flags.get()
}

/// `skill.activeEffect.grantedEffect.name`.
fn ge_name(skill: &ActiveSkillRef) -> Name {
    skill.activeEffect.get().grantedEffect.get().name
}

/// `skill.socketGroup.slot` (a nil socket group raises).
fn slot(skill: &ActiveSkillRef) -> Option<Name> {
    match skill.socketGroup.get() {
        Some(g) => g.slot.get(),
        None => lua_error!("attempt to index field 'socketGroup' (a nil value)"),
    }
}

/// `skill.triggeredBy` where the Lua indexes it (nil raises).
fn trig_by(skill: &ActiveSkillRef) -> SkillEffectRef {
    skill.triggeredBy.get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'triggeredBy' (a nil value)"))
}

/// `skill.triggeredBy.mainSkill` where the Lua indexes it (nil raises).
fn trig_by_main(skill: &ActiveSkillRef) -> ActiveSkillRef {
    trig_by(skill)
        .extra
        .obj::<ActiveSkill>(n!("mainSkill"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `GlobalCache.cachedData[env.mode][uuid]` where the Lua indexes the entry (nil raises).
fn cache_req(env: &EnvRef, uuid: Name) -> Rc<CachedData> {
    cachedData(env.mode, uuid)
        .unwrap_or_else(|| lua_error!("attempt to index a nil value (field '?')"))
}

/// A `function(env, skill)` predicate of a config.
fn cond(f: impl Fn(&EnvRef, &ActiveSkillRef) -> bool + 'static) -> Option<SkillCondFn> {
    Some(Rc::new(f))
}

/// A `comparer = function(env, uuid, source, triggerRate)` of a config.
fn comparer(
    f: impl Fn(&EnvRef, Name, Option<&ActiveSkillRef>, Option<f64>) -> bool + 'static,
) -> ComparerFn {
    Rc::new(f)
}

/// `{ … }`: an empty config.
fn cfg() -> TriggerConfig {
    TriggerConfig::default()
}

/// `function(env, skill) return (skill.skillTypes[SkillType.Melee] or skill.skillTypes[SkillType.Attack]) end`
fn melee_or_attack() -> Option<SkillCondFn> {
    cond(|env, skill| has(skill, SkillType::Melee) || has(skill, SkillType::Attack))
}

/// `function(env, skill) return (skill.skillTypes[SkillType.Damage] or skill.skillTypes[SkillType.Attack]) end`
fn damage_or_attack() -> Option<SkillCondFn> {
    cond(|env, skill| has(skill, SkillType::Damage) || has(skill, SkillType::Attack))
}

// ---- configTable --------------------------------------------------------------------------------

/// @pob Modules/CalcTriggers.lua:902-902 h=38fddc97
/// `configTable[key](env)`: one arm per Lua key, in Lua order (provenance per arm).
pub(crate) fn configTable(key: Name, env: &EnvRef) -> Option<TriggerConfig> {
    let call = arm_called(key);
    let config = configTable_arm(key, env);
    if config.is_some() {
        arm_returned(call);
    }
    config
}

/// The body of `configTable[key](env)` (the arms' markers are on the match arms).
fn configTable_arm(key: Name, env: &EnvRef) -> Option<TriggerConfig> {
    match key.as_str() {
        // @pob Modules/CalcTriggers.lua:903-909 h=ffe5a3de
        "law of the wilds" => {
            // L904
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    !has(skill, SkillType::SummonsTotem)
                        && (has(skill, SkillType::Melee) || has(skill, SkillType::Attack))
                        && flags(skill) & ModFlag::Claw > 0
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:910-918 h=546a590b
        "the rippling thoughts" => {
            // L911
            if ge_name(&pmain(env)) == "Storm Cascade" {
                // L912
                return Some(TriggerConfig {
                    triggerSkillCond: cond(|env, skill| {
                        has(skill, SkillType::Melee) || has(skill, SkillType::Attack)
                    }),
                    ..cfg()
                });
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:919-928 h=3f684933
        "the surging thoughts" => {
            // L920
            if ge_name(&pmain(env)) == "Storm Cascade" {
                // L921
                return Some(TriggerConfig {
                    triggerOnUse: true,
                    triggerSkillCond: cond(|env, skill| {
                        has(skill, SkillType::Melee) || has(skill, SkillType::Attack)
                    }),
                    ..cfg()
                });
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:929-943 h=5519f910
        "the hidden blade" => {
            let mainSkill = pmain(env);
            // L930
            mainSkill.skillFlags.get().set(n!("globalTrigger"), true);
            // L931
            mainSkill
                .skillData
                .get()
                .set(n!("triggerRateCapOverride"), 2.0);
            // L932
            if env
                .player
                .modDB
                .get()
                .Flag(None, &[n!("Condition:Phasing")])
            {
                // L933
                if let Some(breakdown) = env.player.breakdown.get() {
                    let triggerRateCapOverride = mainSkill
                        .skillData
                        .get()
                        .get(n!("triggerRateCapOverride"))
                        .opt_num();
                    // L934
                    breakdown.t.set(
                        n!("TriggerRateCap"),
                        tbl![
                            lfmt!(
                                "%.2f ^8(Unseen Strike from The Hidden Blade has no cooldown but is still triggered)",
                                triggerRateCapOverride
                            ),
                            lfmt!("= %.2f", triggerRateCapOverride),
                        ],
                    );
                }
                // L939
                return Some(TriggerConfig {
                    source: Some(mainSkill.clone()),
                    ..cfg()
                });
            }
            // L941
            mainSkill.skillFlags.get().set(n!("disable"), true);
            // L942
            mainSkill
                .disableReason
                .set(Some(LStr::from("This skill is requires you to be phasing")));
            None
        }
        // @pob Modules/CalcTriggers.lua:944-947 h=40f21abf
        "replica eternity shroud" => {
            // L945
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L946
            Some(TriggerConfig {
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:948-951 h=a581f984
        "shroud of the lightless" => {
            // L949
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L950
            Some(TriggerConfig {
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:952-954 h=d6130a57
        "limbsplit" => {
            // L953
            Some(TriggerConfig {
                triggerName: Some(n!("Gore Shockwave")),
                triggerSkillCond: melee_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:955-957 h=ab1862ce
        "the cauteriser" => {
            // L956
            Some(TriggerConfig {
                triggerName: Some(n!("Gore Shockwave")),
                triggerSkillCond: melee_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:958-960 h=e1e78e44
        "duskblight" => {
            // L959
            Some(TriggerConfig {
                triggerName: Some(n!("Stalking Pustule")),
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:961-966 h=83eefe98
        "lioneye's paws" => {
            // L962 Due to the way the triggerExtraSkill function in mod parser works this trigger does not use the custom trigger skill (RainOfArrowsOnAttackingWithBow)
            // L963 the normal version is used here instead. The stats are the same but the normal version does not have cooldown.
            // L964
            pmain(env).skillData.get().set(n!("cooldown"), 1.0);
            // L965
            Some(TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && flags(skill) & ModFlag::Bow > 0
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:967-972 h=39820d65
        "replica lioneye's paws" => {
            // L968 Due to the way the triggerExtraSkill function in mod parser works this trigger does not use the custom trigger skill (RainOfArrowsOnAttackingWithBow)
            // L969 the normal version is used here instead. The stats are the same but the normal version does not have cooldown.
            // L970
            pmain(env).skillData.get().set(n!("cooldown"), 1.0);
            // L971
            Some(TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && flags(skill) & ModFlag::Bow > 0
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:973-977 h=ce6c6c90
        "moonbender's wing" => {
            // L974 Similar situation to "Replica Lioneye's Paws"
            // L975
            pmain(env).skillData.get().set(n!("cooldown"), 1.0);
            // L976
            Some(TriggerConfig {
                triggerName: Some(n!("Lightning Warp")),
                triggerSkillCond: melee_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:978-980 h=c762fa89
        "ngamahu's flame" => {
            // L979
            Some(TriggerConfig {
                triggerName: Some(n!("Molten Burst")),
                triggerSkillCond: melee_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:981-983 h=b42fb6bd
        "cameria's avarice" => {
            // L982
            Some(TriggerConfig {
                triggerName: Some(n!("Icicle Burst")),
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:984-986 h=d396dcc4
        "starcaller" => {
            // L985
            Some(TriggerConfig {
                triggerName: Some(n!("Starfall")),
                triggerSkillCond: melee_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:987-989 h=adb0693e
        "uul-netol's embrace" => {
            // L988
            Some(TriggerConfig {
                triggerName: Some(n!("Bone Nova")),
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:990-993 h=753fad62
        "rigwald's crest" => {
            // L991
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L992
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:994-997 h=f1738e38
        "jorrhast's blacksteel" => {
            // L995
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L996
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:998-1001 h=028759b5
        "ashcaller" => {
            // L999
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1000
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1002-1004 h=2af7e825
        "arakaali's fang" => {
            // L1003
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1005-1007 h=3308a63c
        "sporeguard" => {
            // L1006
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1008-1010 h=f944bfa9
        "mark of the elder" => {
            // L1009
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1011-1013 h=50fc57f5
        "mark of the shaper" => {
            // L1012
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerSkillCond: damage_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1014-1022 h=98b79a56
        "poet's pen" => {
            // L1015
            let mut config = TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    (has(skill, SkillType::Damage) || has(skill, SkillType::Attack))
                        && flags(skill) & ModFlag::Wand > 0
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByUnique"))
                    && slot(&pmain(env)) == slot(skill)
                    && has(skill, SkillType::Spell)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1023-1035 h=4f3cff20
        "maloney's mechanism" => {
            // L1024
            let slotName = pmain(env).skillCfg.get().slotName.get();
            let item = slotName.and_then(|s| env.player.item_in(s));
            let modSource = match item {
                Some(item) => item.modSource.get(),
                None => lua_error!("attempt to index field '?' (a nil value)"),
            };
            let modSource = modSource
                .unwrap_or_else(|| lua_error!("attempt to index field 'modSource' (a nil value)"));
            let uniqueTriggerName: Option<String> =
                pob_lua::find(&modSource, ".*:.*:(.*),.*", None, false)
                    .map(|(_, _, caps)| caps[0].as_str().to_owned());
            // L1025
            let uniqueTriggerName = uniqueTriggerName.unwrap_or_else(|| {
                lua_error!("attempt to index local 'uniqueTriggerName' (a nil value)")
            });
            let isReplica = pob_lua::lmatch(&uniqueTriggerName, "Replica.", None).is_some();
            // L1026
            let mut config = TriggerConfig {
                triggerOnUse: true,
                triggerName: Some(Name::from(uniqueTriggerName.as_str())),
                useCastRate: isReplica,
                triggerSkillCond: cond(move |env, skill| {
                    let attack = has(skill, SkillType::Attack)
                        && (flags(skill) & ModFlag::Bow > 0)
                        && !isReplica;
                    let spell = has(skill, SkillType::Spell) && isReplica;
                    attack || spell
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByUnique"))
                    && slot(&pmain(env)) == slot(skill)
                    && has(skill, SkillType::RangedAttack)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1036-1044 h=47da73b0
        "asenath's chant" => {
            // L1037
            let mut config = TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    (has(skill, SkillType::Damage) || has(skill, SkillType::Attack))
                        && flags(skill) & ModFlag::Bow > 0
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByUnique"))
                    && slot(&pmain(env)) == slot(skill)
                    && has(skill, SkillType::Spell)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1045-1050 h=ebb2f417
        "vixen's entrapment" => {
            // L1046
            Some(TriggerConfig {
                useCastRate: true,
                triggerSkillCond: cond(|env, skill| has(skill, SkillType::Hex)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1051-1056 h=1c1d6895
        "flames of judgement" => {
            // L1052
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1053
            let mut config = TriggerConfig {
                triggerName: Some(ge_name(&pmain(env))),
                triggerSkillCond: cond(|env, skill| ge_name(skill) == "Queen's Demand"),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByUnique"))
                    && slot(&pmain(env)) == slot(skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1057-1062 h=9456ceef
        "storm of judgement" => {
            // L1058
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1059
            let mut config = TriggerConfig {
                triggerName: Some(ge_name(&pmain(env))),
                triggerSkillCond: cond(|env, skill| ge_name(skill) == "Queen's Demand"),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByUnique"))
                    && slot(&pmain(env)) == slot(skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1063-1085 h=a1c69421
        "trigger craft" => {
            let mainSkill = pmain(env);
            // L1064
            if mainSkill.skillData.get().truthy(n!("triggeredByCraft")) {
                // L1065
                let mut trigRate: Option<f64> = None;
                let mut source: Option<ActiveSkillRef> = None;
                let mut uuid: Option<Name> = None;
                let mut useCastRate: Option<bool> = None;
                let triggeredSkills: Table;
                // L1066
                triggeredSkills = Table::new();
                // L1067
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L1068
                    if (has(&skill, SkillType::Damage)
                        || has(&skill, SkillType::Attack)
                        || has(&skill, SkillType::Spell))
                        && !skill.skillFlags.get().truthy(n!("aura"))
                        && !Rc::ptr_eq(&skill, &pmain(env))
                        && !skill.skillData.get().truthy(n!("triggeredByCraft"))
                        && !grantedEffect_fromItem(&skill.activeEffect.get().grantedEffect.get())
                        && !isTriggered(&skill)
                    {
                        // L1069
                        let r = findTriggerSkill(env, &skill, source.take(), trigRate, None);
                        source = r.0;
                        trigRate = r.1;
                        uuid = r.2;
                        let skillFlags = skill.skillFlags.get();
                        let activeEffect = skill.activeEffect.get();
                        let grantedEffect = activeEffect.grantedEffect.get();
                        // L1070
                        if (skillFlags.truthy(n!("totem"))
                            || skillFlags.truthy(n!("golem"))
                            || skillFlags.truthy(n!("banner"))
                            || skillFlags.truthy(n!("ballista")))
                            && grantedEffect.castTime.is_some()
                        {
                            let castTime = grantedEffect.castTime.unwrap_or_default();
                            // L1071 (`levels` is never nil in the typed data)
                            // L1072
                            let cooldown = match grantedEffect.levels.get(activeEffect.level.get()) {
                                Some(l) => l.num(n!("cooldown")),
                                None => lua_error!("attempt to index a nil value (field '?')"),
                            };
                            trigRate = Some(1.0 / (castTime + cooldown.unwrap_or(0.0)));
                            // L1076
                            useCastRate = Some(true);
                        }
                    }
                    // L1079
                    if skill.skillData.get().truthy(n!("triggeredByCraft"))
                        && slot(&pmain(env)) == slot(&skill)
                    {
                        // L1080
                        triggeredSkills.push(packageSkillDataForSimulation(&skill, env));
                    }
                });
                // L1083
                let config = TriggerConfig {
                    source,
                    useCastRate: useCastRate.unwrap_or(false),
                    ..cfg()
                };
                config.extra.set(n!("trigRate"), trigRate);
                config
                    .extra
                    .set(n!("uuid"), uuid.map_or(Val::Nil, Val::from));
                config.extra.set(n!("triggeredSkills"), triggeredSkills);
                return Some(config);
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1086-1099 h=69ed8615
        "kitava's thirst" => {
            // L1087
            let requiredManaCost =
                env.player
                    .modDB
                    .get()
                    .Sum(ModType::BASE, None, &[n!("KitavaRequiredManaCost")]);
            // L1088
            let mut config = TriggerConfig {
                triggerChance: Some(env.player.modDB.get().Sum(
                    ModType::BASE,
                    None,
                    &[n!("KitavaTriggerChance")],
                )),
                triggerName: Some(n!("Kitava's Thirst")),
                triggerSkillCond: cond(|env, skill| {
                    true
                    // Filtering done by skill() in SkillStatMap, comparer and default excludes
                }),
                ..cfg()
            };
            config.comparer = Some(comparer(move |env, uuid, source, triggerRate| {
                let cachedSpeed = lor(cache_req(env, uuid).HitSpeed.clone(), || {
                    cache_req(env, uuid).Speed.clone()
                });
                let cachedManaCost = cache_req(env, uuid).ManaCost.clone();
                ((source.is_none() && cachedSpeed.truthy())
                    || (cachedSpeed.truthy() && cachedSpeed.num() > triggerRate.unwrap_or(0.0)))
                    && (lor(cachedManaCost, || Val::Num(0.0)).num() >= requiredManaCost)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1100-1113 h=33efb4eb
        "foulborn kitava's thirst" => {
            // L1101
            let requiredLifeCost = env.player.modDB.get().Sum(
                ModType::BASE,
                None,
                &[n!("FoulbornKitavaRequiredLifeCost")],
            );
            // L1102
            let mut config = TriggerConfig {
                triggerChance: Some(env.player.modDB.get().Sum(
                    ModType::BASE,
                    None,
                    &[n!("FoulbornKitavaTriggerChance")],
                )),
                triggerName: Some(n!("Foulborn Kitava's Thirst")),
                triggerSkillCond: cond(|env, skill| {
                    true
                    // Filtering done by skill() in SkillStatMap, comparer and default excludes
                }),
                ..cfg()
            };
            config.comparer = Some(comparer(move |env, uuid, source, triggerRate| {
                let cachedSpeed = lor(cache_req(env, uuid).HitSpeed.clone(), || {
                    cache_req(env, uuid).Speed.clone()
                });
                let cachedLifeCost = cache_req(env, uuid).LifeCost.clone();
                ((source.is_none() && cachedSpeed.truthy())
                    || (cachedSpeed.truthy() && cachedSpeed.num() > triggerRate.unwrap_or(0.0)))
                    && (lor(cachedLifeCost, || Val::Num(0.0)).num() >= requiredLifeCost)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1114-1121 h=f8f22bcd
        "mjolner" => {
            // L1115
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    (has(skill, SkillType::Damage) || has(skill, SkillType::Attack))
                        && flags(skill) & (ModFlag::Mace | ModFlag::Weapon1H) > 0
                        && !slotMatch(env, skill)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByMjolner")) && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1122-1129 h=ff8e9cce
        "wing of the wyvern" => {
            // L1123
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    (has(skill, SkillType::Damage) || has(skill, SkillType::Attack))
                        && flags(skill) & ModFlag::Bow > 0
                        && !slotMatch(env, skill)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByUnique")) && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1130-1135 h=d4196ee6
        "cospri's malice" => {
            // L1131
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Melee)
                        && flags(skill) & (ModFlag::Sword | ModFlag::Weapon1H) > 0
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByCospris"))
                    && slot(&pmain(env)) == slot(skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1136-1139 h=d699aa70
        "seven teachings" => {
            // L1137
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Melee)
                        && flags(skill) & (ModFlag::Unarmed | ModFlag::Melee)
                            == (ModFlag::Unarmed | ModFlag::Melee)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill
                    .skillData
                    .get()
                    .truthy(n!("triggeredBySevenTeachings"))
                    && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1140-1151 h=85753ab9
        "squirming terror" => {
            // L1141
            if env
                .player
                .modDB
                .get()
                .Flag(None, &[n!("Condition:KilledRecently")])
            {
                // L1142
                let mut config = TriggerConfig {
                    assumingEveryHitKills: true,
                    triggerSkillCond: cond(|env, skill| {
                        has(skill, SkillType::Attack) && has(skill, SkillType::Melee)
                    }),
                    ..cfg()
                };
                config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                    skill
                        .skillData
                        .get()
                        .truthy(n!("triggeredBySquirmingTerror"))
                        && slotMatch(env, skill)
                }));
                return Some(config);
            } else {
                // L1148
                pmain(env)
                    .infoMessage2
                    .set(Some(LStr::from("DPS reported assuming Self-Cast")));
                // L1149
                pmain(env)
                    .infoMessage
                    .set(Some(LStr::from("Squirming Terror requires recent kills")));
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1152-1155 h=b964c465
        "kinetic flux" => {
            // L1153
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && slotMatch(env, skill)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByKineticFlux")) && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1156-1159 h=15415b6a
        "cast on critical strike" => {
            // L1157
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && slotMatch(env, skill)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByCoc")) && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1160-1171 h=223e039b
        "cast on melee kill" => {
            // L1161
            if env
                .player
                .modDB
                .get()
                .Flag(None, &[n!("Condition:KilledRecently")])
            {
                // L1162
                let mut config = TriggerConfig {
                    assumingEveryHitKills: true,
                    triggerSkillCond: cond(|env, skill| {
                        has(skill, SkillType::Attack)
                            && has(skill, SkillType::Melee)
                            && slotMatch(env, skill)
                    }),
                    ..cfg()
                };
                config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                    skill.skillData.get().truthy(n!("triggeredByMeleeKill"))
                        && slotMatch(env, skill)
                }));
                return Some(config);
            } else {
                // L1168
                pmain(env)
                    .infoMessage2
                    .set(Some(LStr::from("DPS reported assuming Self-Cast")));
                // L1169
                pmain(env)
                    .infoMessage
                    .set(Some(LStr::from("Cast on Melee Kill requires recent kills")));
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1172-1179 h=7890510e
        "nova" => {
            // L1173
            if let Some(minion) = env.minion.get() {
                if let Some(minionMain) = minion.mainSkill.get() {
                    // L1174
                    let config = TriggerConfig {
                        triggerName: Some(n!("Summon Holy Relic")),
                        actor: Some(minion.clone()),
                        triggerSkillCond: cond(|env, skill| has(skill, SkillType::Attack)),
                        ..cfg()
                    };
                    let uuid = cacheSkillUUID(&minionMain, env);
                    let cd = minionMain.skillData.get().get(n!("cooldown"));
                    config
                        .extra
                        .set(n!("triggeredSkills"), tbl![tbl! { uuid = uuid, cd = cd }]);
                    return Some(config);
                }
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1180-1194 h=22032a87
        "cast when damage taken" => {
            let mainSkill = pmain(env);
            // L1181
            if mainSkill
                .skillData
                .get()
                .truthy(n!("triggeredByDamageTaken"))
            {
                // L1182
                let thresholdMod =
                    calcLib::r#mod(&mainSkill.skillModList.get(), None, &[n!("CWDTThreshold")]);
                // L1183
                let triggeredByDamageTaken =
                    mainSkill.skillData.get().get(n!("triggeredByDamageTaken"));
                env.player.output.get().set(
                    n!("CWDTThreshold"),
                    triggeredByDamageTaken.num() * thresholdMod,
                );
                // L1184
                if let Some(breakdown) = env.player.breakdown.get().filter(|_| {
                    !env.player
                        .output
                        .get()
                        .get(n!("CWDTThreshold"))
                        .raw_eq(&mainSkill.skillData.get().get(n!("triggeredByDamageTaken")))
                }) {
                    let triggeredByDamageTaken =
                        mainSkill.skillData.get().get(n!("triggeredByDamageTaken"));
                    // L1185
                    breakdown.t.set(
                        n!("CWDTThreshold"),
                        tbl![
                            lfmt!("%.2f ^8(base threshold)", triggeredByDamageTaken.opt_num()),
                            lfmt!("x %.2f ^8(threshold modifier)", thresholdMod),
                            lfmt!(
                                "= %.2f",
                                env.player.output.get().get(n!("CWDTThreshold")).opt_num()
                            ),
                        ],
                    );
                }
                // L1191
                mainSkill.skillFlags.get().set(n!("globalTrigger"), true);
                // L1192
                return Some(TriggerConfig {
                    source: Some(pmain(env)),
                    ..cfg()
                });
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1195-1199 h=baf28ebb
        "cast when stunned" => {
            // L1196
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1197
            Some(TriggerConfig {
                triggerChance: pmain(env)
                    .skillData
                    .get()
                    .get(n!("chanceToTriggerOnStun"))
                    .opt_num(),
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1200-1204 h=64450120
        "cast on ward break" => {
            // L1201
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1202
            Some(TriggerConfig {
                triggerChance: pmain(env)
                    .skillData
                    .get()
                    .get(n!("chanceToTriggerOnWardBreak"))
                    .opt_num(),
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1205-1217 h=af86ba6d
        "spellslinger" => {
            // L1206
            if ge_name(&pmain(env)) == "Spellslinger" {
                // L1207
                pmain(env)
                    .skillFlags
                    .get()
                    .set(n!("skipEffectiveRate"), true);
            }
            // L1209 Spell slinger adds a cooldown with its support part
            // L1210
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1211
            Some(TriggerConfig {
                triggerName: Some(n!("Spellslinger")),
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    let isWandProjectileAttack = has(skill, SkillType::Attack)
                        && has(skill, SkillType::Projectile)
                        && flags(skill) & ModFlag::Wand > 0;
                    isWandProjectileAttack
                        && !skill.skillData.get().truthy(n!("triggeredBySpellSlinger"))
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1218-1235 h=2dfa7935
        // This is for backwards compatibility only
        "call to arms" => {
            // L1219
            if ge_name(&pmain(env)) == "Call to Arms" {
                // L1220
                pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
                // L1221
                pmain(env)
                    .skillFlags
                    .get()
                    .set(n!("skipEffectiveRate"), true);
            } else {
                // L1222 Needed to get the cooldown form the active part
                // L1223
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L1224
                    if ge_name(&skill) == "Call to Arms" {
                        // L1225
                        trig_by(&pmain(env))
                            .grantedEffect
                            .set(skill.activeEffect.get().grantedEffect.get());
                        // L1226
                        break;
                    }
                });
            }
            // L1230
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1231
            pmain(env).skillData.get().set(n!("ignoresTickRate"), true);
            // L1232
            Some(TriggerConfig {
                triggerOnUse: true,
                useCastRate: true,
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1236-1251 h=3f13e2e0
        "automation" => {
            // L1237
            if ge_name(&pmain(env)) == "Automation" {
                // L1238
                pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
                // L1239
                pmain(env)
                    .skillFlags
                    .get()
                    .set(n!("skipEffectiveRate"), true);
            } else {
                // L1240 Needed to get the cooldown form the active part
                // L1241
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L1242
                    if ge_name(&skill) == "Automation" {
                        // L1243
                        trig_by(&pmain(env))
                            .grantedEffect
                            .set(skill.activeEffect.get().grantedEffect.get());
                        // L1244
                        break;
                    }
                });
            }
            // L1248
            Some(TriggerConfig {
                triggerOnUse: true,
                useCastRate: true,
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1252-1273 h=5668395c
        "autoexertion" => {
            // L1253
            if ge_name(&pmain(env)) == "Autoexertion" {
                // L1254
                pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
                // L1255
                pmain(env)
                    .skillFlags
                    .get()
                    .set(n!("skipEffectiveRate"), true);
            } else {
                // L1256 Needed to get the cooldown from the active part
                // L1257 Autoexertion has cooldown as part of its support part
                // L1258 Not really sure which one should apply here
                // L1259 Applying the one from the active part to be consistent
                // L1260 with skills like Automation and Spellslinger
                // L1261
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L1262
                    if ge_name(&skill) == "Autoexertion" {
                        // L1263
                        trig_by(&pmain(env))
                            .grantedEffect
                            .set(skill.activeEffect.get().grantedEffect.get());
                        // L1264
                        break;
                    }
                });
            }
            // L1268
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1269
            pmain(env).skillData.get().set(n!("ignoresTickRate"), true);
            // L1270
            Some(TriggerConfig {
                triggerOnUse: true,
                useCastRate: true,
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1274-1276 h=eef76400
        "seize the flesh" => {
            // L1275
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Warcry) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1277-1279 h=f7e75794
        "fissure" => {
            // L1278
            Some(TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Slam) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1280-1282 h=b3dfd0ee
        "mark on hit" => {
            // L1281
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| has(skill, SkillType::Attack)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1283-1288 h=d87e3d7e
        "hextouch" => {
            // L1284
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1285
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1289-1294 h=153d42b8
        "oskarm" => {
            // L1290
            pmain(env)
                .skillData
                .get()
                .set(n!("sourceRateIsFinal"), true);
            // L1291
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| has(skill, SkillType::Attack)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1295-1298 h=0f3a602f
        "tempest shield" => {
            // L1296
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1297
            Some(TriggerConfig {
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1299-1314 h=929b708f
        "shattershard" => {
            // L1300
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1301
            let uuid = cacheSkillUUID(&pmain(env), env);
            // L1302
            if cachedData(env.mode, uuid).is_none() || env.mode == Mode::CALCULATOR {
                // L1303
                buildActiveSkill(env, env.mode, &pmain(env), Some(uuid), &[uuid]);
            }
            // L1305
            let Duration = cache_req(env, uuid)
                .Env
                .player
                .output
                .get()
                .get(n!("Duration"));
            pmain(env).skillData.get().set(
                n!("triggerRateCapOverride"),
                1.0 / Duration.opt_num().unwrap_or_else(|| {
                    lua_error!("attempt to perform arithmetic on field 'Duration' (a nil value)")
                }),
            );
            // L1306
            if let Some(breakdown) = env.player.breakdown.get() {
                // L1307
                breakdown.t.set(
                    n!("SkillTriggerRate"),
                    tbl![
                        lfmt!("Shattershard uses duration as pseudo cooldown"),
                        lfmt!(
                            "1 / %.2f ^8(Shattershard duration)",
                            cache_req(env, uuid)
                                .Env
                                .player
                                .output
                                .get()
                                .get(n!("Duration"))
                                .opt_num()
                        ),
                        lfmt!(
                            "= %.2f ^8per second",
                            pmain(env)
                                .skillData
                                .get()
                                .get(n!("triggerRateCapOverride"))
                                .opt_num()
                        ),
                    ],
                );
            }
            // L1313
            Some(TriggerConfig {
                source: Some(pmain(env)),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1315-1325 h=4683bb4a
        "battlemage's cry" => {
            // L1316
            if ge_name(&pmain(env)) != "Battlemage's Cry" {
                // L1317
                let mut config = TriggerConfig {
                    triggerSkillCond: cond(|env, skill| has(skill, SkillType::Melee)),
                    ..cfg()
                };
                config.comparer = Some(comparer(|env, uuid, source, triggerRate| {
                    // Skills with no uptime ratio are not exerted by battlemage so should not be considered.
                    let uptimeRatio = cache_req(env, uuid)
                        .Env
                        .player
                        .output
                        .get()
                        .get(n!("BattlemageUpTimeRatio"));
                    defaultComparer(env, uuid, source, triggerRate) && uptimeRatio.truthy()
                }));
                config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                    skill.skillData.get().truthy(n!("triggeredByBattleMageCry"))
                        && slotMatch(env, skill)
                }));
                return Some(config);
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1326-1346 h=897c45ac
        "arcanist brand" => {
            // L1327
            if ge_name(&pmain(env)) != "Arcanist Brand" {
                // L1328
                pmain(env)
                    .skillData
                    .get()
                    .set(n!("sourceRateIsFinal"), true);
                // L1329
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L1330
                    if ge_name(&skill) == "Arcanist Brand" {
                        // L1331
                        trig_by(&pmain(env))
                            .extra
                            .set(n!("mainSkill"), Val::Obj(skill.clone() as Rc<dyn std::any::Any>));
                        // L1332
                        break;
                    }
                });

                // L1336
                let brand = trig_by_main(&pmain(env));
                let activationFreqInc = (100.0
                    + brand.skillModList.get().Sum(
                        ModType::INC,
                        Some(&brand.skillCfg.get()),
                        &[n!("Speed"), n!("BrandActivationFrequency")],
                    ))
                    / 100.0;
                // L1337
                let brand = trig_by_main(&pmain(env));
                let activationFreqMore = brand.skillModList.get().More(
                    Some(&brand.skillCfg.get()),
                    &[n!("BrandActivationFrequency")],
                );
                // L1338
                trig_by(&pmain(env))
                    .extra
                    .set(n!("activationFreqInc"), activationFreqInc);
                // L1339
                trig_by(&pmain(env))
                    .extra
                    .set(n!("activationFreqMore"), activationFreqMore);
                // L1340
                let attachedBrandCount = trig_by_main(&pmain(env))
                    .skillData
                    .get()
                    .get(n!("attachedBrandCount"));
                trig_by(&pmain(env))
                    .extra
                    .set(n!("attachedBrandCount"), attachedBrandCount);
                // L1341
                trig_by(&pmain(env)).extra.set(n!("ignoresTickRate"), true);
                // L1342
                let repeatFrequency = trig_by_main(&pmain(env))
                    .skillData
                    .get()
                    .get(n!("repeatFrequency"));
                let attachedBrandCount = trig_by(&pmain(env)).extra.get(n!("attachedBrandCount"));
                let trigRate = repeatFrequency.num()
                    * activationFreqInc
                    * activationFreqMore
                    * attachedBrandCount.num();
                let mut config = TriggerConfig {
                    source: Some(trig_by_main(&pmain(env))),
                    ..cfg()
                };
                config.extra.set(n!("trigRate"), trigRate);
                config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                    skill.skillData.get().truthy(n!("triggeredByBrand")) && slotMatch(env, skill)
                }));
                return Some(config);
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1347-1351 h=80e8a335
        "cast on death" => {
            // L1348
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1349
            pmain(env).skillData.get().set(n!("triggered"), true);
            // L1350
            let message = format!("{}{}", ge_name(&pmain(env)), " Triggered on Death");
            pmain(env)
                .infoMessage
                .set(Some(LStr::from(message.as_str())));
            None
        }
        // @pob Modules/CalcTriggers.lua:1352-1359 h=b4580bfd
        "combust" => {
            // L1353
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| has(skill, SkillType::Melee)),
                ..cfg()
            };
            config.comparer = Some(comparer(|env, uuid, source, triggerRate| {
                // Skills with no uptime ratio are not exerted by infernal cry so should not be considered.
                let uptimeRatio = cache_req(env, uuid)
                    .Env
                    .player
                    .output
                    .get()
                    .get(n!("InfernalUpTimeRatio"));
                defaultComparer(env, uuid, source, triggerRate) && uptimeRatio.truthy()
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1360-1362 h=f7cb89a9
        "prismatic burst" => {
            // L1361
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1363-1365 h=36a7d957
        "voidstorm" => {
            // L1364
            Some(TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack)
                        && has(skill, SkillType::Rain)
                        && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1366-1368 h=61ba00e5
        "shockwave" => {
            // L1367
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Melee) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1369-1371 h=1abb793e
        "void shockwave" => {
            // L1370
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Melee) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1372-1374 h=197a8fc7
        "falling crystal" => {
            // L1373
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Melee) && slotMatch(env, skill)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1375-1382 h=d47244dd
        "call the pyre" => {
            // L1376
            if env.enemy.modDB.get().Flag(None, &[n!("Condition:Ignited")]) {
                // L1377
                return Some(TriggerConfig {
                    triggerChance: Some(50.0), // too much of a pain to pull this from the triggering skill
                    triggerSkillCond: cond(|env, skill| {
                        has(skill, SkillType::Melee) && slotMatch(env, skill)
                    }),
                    ..cfg()
                });
            } else {
                // L1380
                pmain(env)
                    .infoMessage
                    .set(Some(LStr::from("Call the Pyre requires Ignited enemies")));
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1383-1387 h=21a51b77
        "manaforged arrows" => {
            // L1384
            Some(TriggerConfig {
                triggerOnUse: true,
                triggerName: Some(n!("Manaforged Arrows")),
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Attack) && flags(skill) & ModFlag::Bow > 0
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1388-1397 h=506dc6cc
        "doom blast" => {
            // L1389
            if env
                .build
                .configTab
                .input
                .get(n!("doomBlastSource"))
                .str()
                .as_deref()
                == Some("replacement")
            {
                // L1390
                env.player.modDB.get().NewMod(
                    n!("UsesCurseOverlaps"),
                    ModType::FLAG,
                    true,
                    "Config",
                    0,
                    0,
                    &[],
                );
            }
            // L1392
            pmain(env).skillData.get().set(n!("ignoresTickRate"), true);
            // L1393
            let overlaps = if env
                .player
                .modDB
                .get()
                .Tabulate(Some(ModType::BASE), None, &[n!("Multiplier:CurseOverlaps")])
                .len()
                > 0
            {
                Val::Num(m_max(
                    env.player.modDB.get().Sum(
                        ModType::BASE,
                        None,
                        &[n!("Multiplier:CurseOverlaps")],
                    ),
                    1.0,
                ))
            } else {
                Val::Bool(false)
            };
            let config = TriggerConfig {
                useCastRate: true,
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Hex) && slotMatch(env, skill)
                }),
                ..cfg()
            };
            config.extra.set(n!("overlaps"), overlaps);
            config
                .extra
                .set(n!("customTriggerName"), "Doom Blast triggering Hex: ");
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1398-1400 h=38dec1cc
        "cast while channelling" => {
            // L1399
            Some(TriggerConfig {
                customHandler: Some(CWCHandler),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1401-1403 h=62b587f0
        "focus" => {
            // L1402
            Some(TriggerConfig {
                customHandler: Some(helmetFocusHandler),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1404-1479 h=4ff335b3
        "snipe" => snipe(env),
        // @pob Modules/CalcTriggers.lua:1480-1487 h=ef4cebd0
        "avenging flame" => {
            // L1481
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    skill.skillFlags.get().truthy(n!("totem")) && slotMatch(env, skill)
                }),
                ..cfg()
            };
            config.comparer = Some(comparer(|env, uuid, source, currentTotemLife| {
                let totemLife = cache_req(env, uuid)
                    .Env
                    .player
                    .output
                    .get()
                    .get(n!("TotemLife"));
                (source.is_none() && totemLife.truthy())
                    || (totemLife.truthy() && totemLife.num() > currentTotemLife.unwrap_or(0.0))
            }));
            config.extra.set(n!("ignoreSourceRate"), true);
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1488-1502 h=a0b36362
        "intuitive link" => {
            // L1489
            if ge_name(&pmain(env)) != "Intuitive Link" {
                // L1490
                for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                    // L1491
                    if ge_name(&skill) == "Intuitive Link" {
                        // L1492
                        trig_by(&pmain(env))
                            .extra
                            .set(n!("mainSkill"), Val::Obj(skill.clone() as Rc<dyn std::any::Any>));
                        // L1493
                        break;
                    }
                });
                // L1496
                let mut config = cfg();
                config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                    has(skill, SkillType::Spell)
                        && slotMatch(env, skill)
                        && !trig_by(&pmain(env))
                            .extra
                            .obj::<ActiveSkill>(n!("mainSkill"))
                            .is_some_and(|m| Rc::ptr_eq(&m, skill))
                }));
                config.extra.set(
                    n!("trigRate"),
                    env.modDB
                        .get()
                        .Sum(ModType::BASE, None, &[n!("IntuitiveLinkSourceRate")]),
                );
                config.source = trig_by(&pmain(env))
                    .extra
                    .obj::<ActiveSkill>(n!("mainSkill"));
                config.extra.set(n!("sourceName"), "Custom source");
                config.useCastRate = true;
                return Some(config);
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1503-1509 h=6c30fa77
        // Svalinn Girded Tower Shield
        "svalinn cast on block" => {
            // L1504
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1505
            let mut config = TriggerConfig {
                source: Some(pmain(env)),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                slotMatch(env, skill)
                    && skill.triggeredBy.get().is_some_and(|tb| {
                        calcLib::canGrantedEffectSupportActiveSkill(
                            &tb.grantedEffect.get(),
                            skill,
                            None,
                            false,
                        )
                    })
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1510-1516 h=c9cad98c
        // Festering Resentment Demon Dagger
        "festering resentment cast on block" => {
            // L1511
            pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
            // L1512
            let mut config = TriggerConfig {
                source: Some(pmain(env)),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                slotMatch(env, skill)
                    && skill.triggeredBy.get().is_some_and(|tb| {
                        calcLib::canGrantedEffectSupportActiveSkill(
                            &tb.grantedEffect.get(),
                            skill,
                            None,
                            false,
                        )
                    })
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1517-1525 h=a119af7c
        // Hand of the Lords Carnal Mitts
        "hex on trap" => {
            // L1518
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    // Hex is triggered only when a trap is triggered
                    has(skill, SkillType::Trapped)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill.skillData.get().truthy(n!("triggeredByTrapTrigger")) && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1526-1534 h=cdc35e69
        "supporttriggerfirespellonhit" => {
            // L1527
            let mut config = TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    // Skill is triggered only when the weapon with the enchant on it hits
                    has(skill, SkillType::Melee)
                }),
                ..cfg()
            };
            config.triggeredSkillCond = Some(Rc::new(|env, skill| {
                skill
                    .skillData
                    .get()
                    .truthy(n!("triggeredBySettlersEnchantTrigger"))
                    && slotMatch(env, skill)
            }));
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1535-1543 h=ea28e383
        "ghostly artillery" => {
            // L1536
            let config = TriggerConfig {
                triggerOnUse: true,
                triggerSkillCond: cond(|env, skill| has(skill, SkillType::Attack)),
                // CCR-0087 §3: `sourceWeapon = true`
                sourceWeapon: true,
                ..cfg()
            };
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1544-1550 h=13d083d0
        "replica gifts from above" => {
            // L1545
            Some(TriggerConfig {
                triggerSkillCond: cond(|env, skill| {
                    !has(skill, SkillType::SummonsTotem) && has(skill, SkillType::Attack)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1551-1555 h=1ea2e967
        "BloodShrineUniqueTriggeredExplodingToad" => {
            // L1552
            let srcInstance = pmain(env)
                .activeEffect
                .get()
                .srcInstance
                .get()
                .unwrap_or_else(|| {
                    lua_error!("attempt to index field 'srcInstance' (a nil value)")
                });
            let triggerChance = srcInstance
                .extra
                .get(n!("triggerChance"))
                .opt_num()
                .unwrap_or_else(|| {
                    lua_error!(
                        "attempt to perform arithmetic on field 'triggerChance' (a nil value)"
                    )
                })
                + env.player.modDB.get().Sum(
                    ModType::BASE,
                    None,
                    &[n!("BloodShrineExplodingToadTriggerChance")],
                );
            // L1553
            Some(TriggerConfig {
                assumingEveryHitKills: true,
                triggerChance: Some(m_min(triggerChance, 100.0)),
                triggerSkillCond: cond(|env, skill| {
                    has(skill, SkillType::Damage) || has(skill, SkillType::Attack)
                }),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1556-1575 h=1c159e89
        "bursting toad" => {
            // L1557
            let mut triggerInterval = m_huge;
            // L1558 All gems in the socket group should return the same HexToadCooldown even when there are multiple hextoad support gems slotted
            // L1559
            for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                // L1560
                if skill.skillData.get().truthy(n!("hextoadTriggerInterval")) {
                    // L1561
                    triggerInterval = m_min(
                        triggerInterval,
                        skill.skillData.get().get(n!("hextoadTriggerInterval")).num(),
                    );
                }
            });
            // L1564
            if triggerInterval < m_huge {
                // L1565
                pmain(env).skillFlags.get().set(n!("globalTrigger"), true);
                // L1566
                pmain(env)
                    .skillData
                    .get()
                    .set(n!("triggerRateCapOverride"), 1.0 / triggerInterval);
                // L1567
                if let Some(breakdown) = env.player.breakdown.get() {
                    // L1568
                    breakdown.t.set(
                        n!("TriggerRateCap"),
                        tbl![
                            lfmt!("1 / %.2f ^8(Hextoad trigger interval)", triggerInterval),
                            lfmt!(
                                "= %.2f",
                                pmain(env)
                                    .skillData
                                    .get()
                                    .get(n!("triggerRateCapOverride"))
                                    .opt_num()
                            ),
                        ],
                    );
                }
                // L1573
                return Some(TriggerConfig {
                    source: Some(pmain(env)),
                    ..cfg()
                });
            }
            None
        }
        // @pob Modules/CalcTriggers.lua:1576-1578 h=e92890d0
        "TriggeredMoltenStrike" => {
            // L1577
            Some(TriggerConfig {
                triggerSkillCond: melee_or_attack(),
                ..cfg()
            })
        }
        // @pob Modules/CalcTriggers.lua:1579-1581 h=872f9ee5
        "FieryImpactHeistMaceImplicit" => {
            // L1580
            let config = TriggerConfig {
                triggerSkillCond: melee_or_attack(),
                // CCR-0087 §3: `sourceWeapon = true`
                sourceWeapon: true,
                ..cfg()
            };
            Some(config)
        }
        // @pob Modules/CalcTriggers.lua:1582-1582 h=c2b7df62
        _ => None,
    }
}

/// The `["snipe"]` arm (CalcTriggers.lua 1404-1479; the marker is on the match arm).
fn snipe(env: &EnvRef) -> Option<TriggerConfig> {
    let playerModDB = env.player.modDB.get();
    // L1405
    let mut snipeStages = m_min(
        playerModDB.Sum(ModType::BASE, None, &[n!("Multiplier:SnipeStage")]),
        playerModDB.Sum(ModType::BASE, None, &[n!("Multiplier:SnipeStagesMax")]),
    );
    // L1406
    let snipeHitMulti = pmain(env).skillModList.get().Sum(
        ModType::BASE,
        Some(&pmain(env).skillCfg.get()),
        &[n!("snipeHitMulti")],
    );
    // L1407
    let snipeAilmentMulti = pmain(env).skillModList.get().Sum(
        ModType::BASE,
        Some(&pmain(env).skillCfg.get()),
        &[n!("snipeAilmentMulti")],
    );
    // L1408
    let mut triggeredSkills: Vec<ActiveSkillRef> = Vec::new();

    // L1410
    for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
        // L1411
        if skill.skillData.get().truthy(n!("triggeredBySnipe"))
            && skill.socketGroup.get().is_some()
            && slot(&skill) == slot(&pmain(env))
        {
            // L1412
            triggeredSkills.push(skill.clone());
        }
    });

    // L1416
    if ge_name(&pmain(env)) == "Snipe" {
        // L1417
        if env
            .limitedSkills
            .get()
            .is_some_and(|l| l.truthy(cacheSkillUUID(&pmain(env), env)))
        {
            // L1418 Snipe is being used by some other skill. In this case snipe does not get more damage mods
            // L1419
            snipeStages = 0.0;
        } else {
            // L1421 max(1, snipeStages) makes it behave consistently with other channeled ranged skills (scourge arrow)
            // L1422 First stage takes 0.5x time to channel compared to subsequent stages
            pmain(env)
                .skillData
                .get()
                .set(n!("hitTimeMultiplier"), m_max(1.0, snipeStages) - 0.5);
        }
        // L1424
        if triggeredSkills.len() < 1 {
            // L1425 Snipe is being used as a standalone skill
            // L1426 (`snipeStages` is a number: always truthy)
            {
                let skillModList = pmain(env).skillModList.get();
                // L1427
                skillModList.NewMod(
                    n!("Multiplier:SnipeStages"),
                    ModType::BASE,
                    snipeStages,
                    "Snipe",
                    0,
                    0,
                    &[],
                );
                // L1428
                skillModList.NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    snipeHitMulti,
                    "Snipe",
                    ModFlag::Hit,
                    0,
                    &[tag!({ type = Multiplier, var = "SnipeStages" })],
                );
                // L1429
                skillModList.NewMod(
                    n!("Damage"),
                    ModType::MORE,
                    snipeAilmentMulti,
                    "Snipe",
                    ModFlag::Ailment,
                    0,
                    &[tag!({ type = Multiplier, var = "SnipeStages" })],
                );
            }
        } else {
            // L1432 Snipe is being used as a trigger source, it triggers other skills but does no damage it self
            let skillModList = pmain(env).skillModList.get();
            // L1433
            skillModList.NewMod(
                n!("DealNoLightning"),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Snipe", includeTransfigured = true })],
            );
            // L1434
            skillModList.NewMod(
                n!("DealNoCold"),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Snipe", includeTransfigured = true })],
            );
            // L1435
            skillModList.NewMod(
                n!("DealNoFire"),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Snipe", includeTransfigured = true })],
            );
            // L1436
            skillModList.NewMod(
                n!("DealNoChaos"),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Snipe", includeTransfigured = true })],
            );
            // L1437
            skillModList.NewMod(
                n!("DealNoPhysical"),
                ModType::FLAG,
                true,
                None,
                0,
                0,
                &[tag!({ type = SkillName, skillName = "Snipe", includeTransfigured = true })],
            );
        }
        None
    } else {
        // L1440
        let mut currentSkillSnipeIndex: Option<usize> = None;
        // L1441
        for (index, skill) in triggeredSkills.iter().enumerate() {
            // L1442
            if Rc::ptr_eq(skill, &pmain(env)) {
                // L1443
                currentSkillSnipeIndex = Some(index + 1);
                // L1444
                break;
            }
        }

        // L1448 Does snipe have enough stages to trigger this skill?
        // L1449
        if currentSkillSnipeIndex.is_some_and(|i| i as f64 <= snipeStages) {
            // L1450
            let mut source: Option<ActiveSkillRef> = None;
            // L1451
            let mut trigRate = Val::Nil;
            let skillModList = pmain(env).skillModList.get();
            // L1452
            skillModList.NewMod(
                n!("Multiplier:SnipeStages"),
                ModType::BASE,
                snipeStages,
                "Snipe",
                0,
                0,
                &[],
            );
            // L1453
            skillModList.NewMod(
                n!("Damage"),
                ModType::MORE,
                snipeAilmentMulti,
                "Snipe",
                ModFlag::Ailment,
                0,
                &[tag!({ type = Multiplier, var = "SnipeStages" })],
            );
            // L1454
            skillModList.NewMod(
                n!("Damage"),
                ModType::MORE,
                snipeHitMulti,
                "Snipe",
                ModFlag::Hit,
                0,
                &[tag!({ type = Multiplier, var = "SnipeStages" })],
            );
            // L1455
            for_ipairs!((_, skill) in env.player.activeSkillList.get() => {
                // L1456
                if ge_name(&skill) == "Snipe"
                    && skill.socketGroup.get().is_some()
                    && slot(&skill) == slot(&pmain(env))
                {
                    // L1457
                    skill
                        .skillData
                        .get()
                        .set(n!("hitTimeMultiplier"), snipeStages - 0.5);
                    // L1458
                    let uuid = cacheSkillUUID(&skill, env);
                    // L1459
                    if cachedData(env.mode, uuid).is_none() || env.mode == Mode::CALCULATOR {
                        // L1460
                        buildActiveSkill(env, env.mode, &skill, Some(uuid), &[]);
                    }
                    // L1462
                    let cachedSpeed = cache_req(env, uuid)
                        .Env
                        .player
                        .output
                        .get()
                        .get(n!("HitSpeed"));
                    // L1463 (`skill.skillFlags` / `skill.skillCfg` are never nil on an active skill)
                    if !skill.skillFlags.get().truthy(n!("disable"))
                        && !skill
                            .skillCfg
                            .get()
                            .skillCond
                            .get()
                            .unwrap_or_else(|| {
                                lua_error!("attempt to index field 'skillCond' (a nil value)")
                            })
                            .truthy(n!("usedByMirage"))
                        && !has(&skill, SkillType::OtherThingUsesSkill)
                        && ((source.is_none() && cachedSpeed.truthy())
                            || (cachedSpeed.truthy()
                                && cachedSpeed.num() > lor(trigRate.clone(), || Val::Num(0.0)).num()))
                    {
                        // L1464
                        trigRate = cachedSpeed;
                        // L1465
                        let HitTime = cache_req(env, uuid)
                            .Env
                            .player
                            .output
                            .get()
                            .get(n!("HitTime"));
                        env.player
                            .output
                            .get()
                            .set(n!("ChannelTimeToTrigger"), HitTime);
                        // L1466
                        source = Some(skill.clone());
                    }
                }
            });

            // L1471
            let config = TriggerConfig { source, ..cfg() };
            config.extra.set(n!("trigRate"), trigRate);
            Some(config)
        } else {
            // L1473
            pmain(env).skillData.get().set(n!("triggered"), Val::Nil);
            // L1474
            pmain(env)
                .infoMessage2
                .set(Some(LStr::from("DPS reported assuming Self-Cast")));
            // L1475
            pmain(env).infoMessage.set(Some(LStr::from(
                "Not enough Snipe stages to trigger this skill",
            )));
            // L1476
            pmain(env).infoTrigger.set(Some(LStr::from("")));
            None
        }
    }
}

// L1584 Find unique item trigger name

/// @pob Modules/CalcTriggers.lua:1585-1600 h=d4ca6f88
pub(crate) fn getUniqueItemTriggerName(skill: &ActiveSkillRef) -> Option<Name> {
    // L1586
    let triggerSource = skill.skillData.get().get(n!("triggerSource"));
    if triggerSource.truthy() {
        // L1587 (`triggerSource` is a skill name written by ModParser)
        return Some(match triggerSource.str() {
            Some(s) => Name::from(&*s),
            None => lua_error!(
                "attempt to index local 'uniqueName' (a {} value)",
                triggerSource.lua_type()
            ),
        });
    // L1588 (`supportList` is never nil on an active skill)
    } else if skill.supportList.get().with(|l| l.len()) >= 1 {
        // L1589
        for_ipairs!((_, gemInstance) in skill.supportList.get() => {
            let grantedEffect = gemInstance.grantedEffect.get();
            // L1590
            if grantedEffect_fromItem(&grantedEffect) && !grantedEffect.support {
                // L1591
                return Some(grantedEffect.name);
            }
        });
    }

    // L1596
    if let Some(source) = skill.socketGroup.get().and_then(|g| g.source.get()) {
        // L1597
        let uniqueTriggerName = pob_lua::find(&source, ".*:.*:(.*),.*", None, false)
            .map(|(_, _, caps)| Name::from(caps[0].as_str()));
        // L1598
        return uniqueTriggerName;
    }
    None
}

/// @pob Modules/CalcTriggers.lua:1602-1627 h=4f18f39d
pub fn triggers(env: &EnvRef, actor: Option<&ActorRef>) {
    let Some(actor) = actor else {
        return;
    };
    let mainSkill = |actor: &ActorRef| {
        actor
            .mainSkill
            .get()
            .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
    };
    // L1603
    if !mainSkill(actor).skillFlags.get().truthy(n!("disable"))
        && !env
            .limitedSkills
            .get()
            .is_some_and(|l| l.truthy(cacheSkillUUID(&mainSkill(actor), env)))
    {
        // L1604
        let skillName = ge_name(&mainSkill(actor));
        // L1605
        let triggerName = mainSkill(actor).triggeredBy.get().map(|tb| tb.grantedEffect.get().name);
        // L1606
        let uniqueName = if isTriggered(&mainSkill(actor)) {
            getUniqueItemTriggerName(&mainSkill(actor))
        } else {
            None
        };
        // L1607
        let skillNameLower = Name::from(pob_lua::string::lower(skillName.as_str()).as_str());
        // L1608
        let triggerNameLower =
            triggerName.map(|t| Name::from(pob_lua::string::lower(t.as_str()).as_str()));
        // L1609
        let awakenedTriggerNameLower = triggerNameLower
            .map(|t| Name::from(pob_lua::gsub(t.as_str(), "^awakened ", "", None).0.as_str()));
        // L1610
        let uniqueNameLower =
            uniqueName.map(|u| Name::from(pob_lua::string::lower(u.as_str()).as_str()));
        // L1611
        let skillId = mainSkill(actor).activeEffect.get().grantedEffect.get().id;
        // L1612
        let mut config = configTable(skillId, env);
        // L1613
        if config.is_none() {
            config = configTable(skillNameLower, env);
        }
        // L1614
        if config.is_none() {
            if let Some(key) = triggerNameLower {
                config = configTable(key, env);
            }
        }
        // L1615
        if config.is_none() {
            if let Some(key) = awakenedTriggerNameLower {
                config = configTable(key, env);
            }
        }
        // L1616
        if config.is_none() {
            if let Some(key) = uniqueNameLower {
                config = configTable(key, env);
            }
        }
        // L1617
        if let Some(mut config) = config {
            // L1618
            config.actor = config.actor.or_else(|| Some(actor.clone()));
            // L1619
            config.triggerName = config
                .triggerName
                .or(triggerName)
                .or(Some(skillName))
                .or(uniqueName);
            // L1620
            config.triggerChance = config.triggerChance.or_else(|| {
                mainSkill(actor)
                    .activeEffect
                    .get()
                    .srcInstance
                    .get()
                    .and_then(|s| s.extra.get(n!("triggerChance")).opt_num())
            });
            // L1621
            let triggerHandler = config.customHandler.unwrap_or(defaultTriggerHandler);
            // L1622
            triggerHandler(env, &mut config);
        } else {
            // L1624
            mainSkill(actor)
                .skillData
                .get()
                .set(n!("triggered"), Val::Nil);
        }
    }
}
