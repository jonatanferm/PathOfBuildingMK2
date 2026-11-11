//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/ConfigVisibility.lua 1-160.
//! Owner: U42.
//!
//! @pob-file Modules/ConfigVisibility.lua @16de4b82d
//!
//! Shared helpers that decide which entries in ConfigOptions should be visible for a given
//! build (the Config tab's "Show All Configurations" exclusion list, and the Compare tab's
//! Config view, U46).
//!
//! The module is pure model code: `build` is the model build (`model::BuildRef`), so the
//! Compare tab can pass its compared builds.
//!
//! Lua semantics:
//! * Option keys absent from the data at 16de4b82d (the dump schema `ConfigOptionDef` has no
//!   field for them): `ifEnemyStat` (a SIMPLE_PREDICATES entry), `ifNode`, `implyMinionCond`,
//!   `implyEnemyCond`. Their branches are ported against the value nil ([`IF_ENEMY_STAT`],
//!   [`IF_NODE`], [`IMPLY_MINION_COND`], [`IMPLY_ENEMY_COND`]), as U41's `VarExtras`
//!   (ctor.rs) does for the same keys.
//! * `anyIfValue(ifOption, predicate)` with a table `ifOption` loops over its `ipairs`; a
//!   single value is `predicate(v) and true or false`.
//! * `mainEnv[p.env] or {}`: a nil `*Used` table reads as empty.
//! * `labelLower:find(kw)` is a pattern find (the keywords hold no magic characters).
//! * `s[field][opt]` (L133) indexes `skillFlags`/`skillData`, which every active skill has.
//!
//! ORDER: `pairs(skillsUsed)` (L120): an any-match (c).
//!
//! Props with side effects (ui doc §7.3): none (no controls).
#![allow(non_snake_case)]

use pob_gamedata::schema::config_defs::{ConfigOptionDef, StrOrList};
use pob_lua::lua_error;

use crate::model::{self, Name, Table};

// @pob Modules/ConfigVisibility.lua:1-22 h=5792cdc2
// (the module header, EXCLUDE_KEYWORDS and SIMPLE_PREDICATES)

/// Labels containing any of these keywords stay hidden even when "Show All Configurations" is
/// on.
pub const EXCLUDE_KEYWORDS: [&str; 6] = [
    "recently",
    "in the last",
    "in the past",
    "in last",
    "in past",
    "pvp",
];

/// `varData.ifEnemyStat`: no option of the data has one (module notes).
const IF_ENEMY_STAT: Option<StrOrList> = None;
/// `varData.ifNode`: no option of the data has one (module notes).
const IF_NODE: Option<StrOrList> = None;
/// `varData.implyMinionCond`: no option of the data has one (module notes).
const IMPLY_MINION_COND: Option<Name> = None;
/// `varData.implyEnemyCond`: no option of the data has one (module notes).
const IMPLY_ENEMY_COND: Option<Name> = None;

/// The `mainEnv` table one SIMPLE_PREDICATES entry reads.
#[derive(Clone, Copy, Debug)]
#[allow(clippy::enum_variant_names)] // the Lua `mainEnv` field names
enum UsedEnv {
    ConditionsUsed,
    MinionConditionsUsed,
    EnemyConditionsUsed,
    MultipliersUsed,
    EnemyMultipliersUsed,
    EnemyPerStatsUsed,
    TagTypesUsed,
    ModsUsed,
}

/// One SIMPLE_PREDICATES entry: `{ key = …, env = …, canImply = … }`.
struct SimplePredicate {
    /// `varData[p.key]`
    key: fn(&ConfigOptionDef) -> Option<StrOrList>,
    env: UsedEnv,
    can_imply: bool,
}

fn one(n: Option<Name>) -> Option<StrOrList> {
    n.map(StrOrList::Str)
}

/// Simple predicates of the form `varData.ifX` → `mainEnv.YUsed[opt]`, with imply-cond
/// fallback when `canImply` is true.
const SIMPLE_PREDICATES: [SimplePredicate; 8] = [
    SimplePredicate {
        key: |v| v.ifCond.clone(),
        env: UsedEnv::ConditionsUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |v| one(v.ifMinionCond),
        env: UsedEnv::MinionConditionsUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |v| one(v.ifEnemyCond),
        env: UsedEnv::EnemyConditionsUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |v| one(v.ifMult),
        env: UsedEnv::MultipliersUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |v| one(v.ifEnemyMult),
        env: UsedEnv::EnemyMultipliersUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |_| IF_ENEMY_STAT,
        env: UsedEnv::EnemyPerStatsUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |v| one(v.ifTagType),
        env: UsedEnv::TagTypesUsed,
        can_imply: true,
    },
    SimplePredicate {
        key: |v| v.ifMod.clone(),
        env: UsedEnv::ModsUsed,
        can_imply: true,
    },
];

/// `mainEnv[p.env]` (nil when the env has not filled it).
fn env_table(env: &model::EnvRef, which: UsedEnv) -> Option<Table> {
    match which {
        UsedEnv::ConditionsUsed => env.conditionsUsed.get(),
        UsedEnv::MinionConditionsUsed => env.minionConditionsUsed.get(),
        UsedEnv::EnemyConditionsUsed => env.enemyConditionsUsed.get(),
        UsedEnv::MultipliersUsed => env.multipliersUsed.get(),
        UsedEnv::EnemyMultipliersUsed => env.enemyMultipliersUsed.get(),
        UsedEnv::EnemyPerStatsUsed => env.enemyPerStatsUsed.get(),
        UsedEnv::TagTypesUsed => env.tagTypesUsed.get(),
        UsedEnv::ModsUsed => env.modsUsed.get(),
    }
}

/// `mainEnv.<name>[opt]` for a table Lua indexes directly (nil: the Lua index error).
#[track_caller]
fn used(t: Option<Table>, name: &str) -> Table {
    t.unwrap_or_else(|| lua_error!("attempt to index field '{name}' (a nil value)"))
}

/// Run `predicate` against either a single value or a list of values.
/// lua: ConfigVisibility.lua:25-33 anyIfValue(ifOption, predicate)
/// @pob Modules/ConfigVisibility.lua:24-33 h=ce23b6e0
fn anyIfValue(ifOption: &StrOrList, mut predicate: impl FnMut(Name) -> bool) -> bool {
    match ifOption {
        // L26
        StrOrList::List(list) => {
            // L27
            for opt in list {
                // L28
                if predicate(*opt) {
                    return true;
                }
            }
            // L30
            false
        }
        // L32
        StrOrList::Str(o) => predicate(*o),
    }
}

/// `configTab.configSets[configTab.activeConfigSetId]` of `build`.
fn active_set(build: &model::BuildRef) -> Option<Table> {
    let ct = &build.configTab;
    ct.configSet(ct.activeConfigSetId.get())
}

/// When the option has an input value and one of its implied conditions is currently used,
/// treat gated predicates as passing.
/// lua: ConfigVisibility.lua:36-52 implyCondActive(varData, build)
/// @pob Modules/ConfigVisibility.lua:35-52 h=aecc7203
fn implyCondActive(varData: &ConfigOptionDef, build: Option<&model::BuildRef>) -> bool {
    // L37-38 (`build.configTab` always exists on a model build)
    let Some(build) = build else { return false };
    // L39-40
    let Some(activeSet) = active_set(build) else {
        return false;
    };
    let input = activeSet
        .table(pob_lua::n!("input"))
        .unwrap_or_else(|| lua_error!("attempt to index field 'input' (a nil value)"));
    if !varData.var.is_some_and(|v| input.truthy(v)) {
        return false;
    }
    // L41-42
    let Some(mainEnv) = build.calcsTab.mainEnv.get() else {
        return false;
    };
    // L43 (`varData.implyCondList` is `{}` when absent: an empty loop)
    for implyCond in &varData.implyCondList {
        // L45
        if used(mainEnv.conditionsUsed.get(), "conditionsUsed").truthy(*implyCond) {
            return true;
        }
    }
    // L48-51
    varData
        .implyCond
        .is_some_and(|c| used(mainEnv.conditionsUsed.get(), "conditionsUsed").truthy(c))
        || IMPLY_MINION_COND.is_some_and(|c| {
            used(mainEnv.minionConditionsUsed.get(), "minionConditionsUsed").truthy(c)
        })
        || IMPLY_ENEMY_COND.is_some_and(|c| {
            used(mainEnv.enemyConditionsUsed.get(), "enemyConditionsUsed").truthy(c)
        })
}

/// True if every `ifX` predicate on `varData` currently passes for `build`.
/// lua: ConfigVisibility.lua:55-141 isRelevantForBuild(varData, build)
/// @pob Modules/ConfigVisibility.lua:54-141 h=9d347521
pub fn isRelevantForBuild(varData: &ConfigOptionDef, build: Option<&model::BuildRef>) -> bool {
    // L56
    let Some(b) = build else { return false };
    // L57-58
    let Some(mainEnv) = b.calcsTab.mainEnv.get() else {
        return false;
    };
    // L59
    let player = &mainEnv.player;
    // L60
    let mainSkill = player.mainSkill.get();
    // L61
    let spec = b.spec.get();
    // L62-66
    let activeInput = active_set(b)
        .and_then(|s| s.table(pob_lua::n!("input")))
        .unwrap_or_default();

    // L68-74
    let mut impliedCache: Option<bool> = None;
    let mut implied = || -> bool {
        // L70
        if impliedCache.is_none() {
            // L71
            impliedCache = Some(implyCondActive(varData, build));
        }
        // L73
        impliedCache.unwrap_or(false)
    };

    // L76
    for p in &SIMPLE_PREDICATES {
        // L77
        let ifVal = (p.key)(varData);
        // L78
        if let Some(ifVal) = ifVal {
            // L79
            let envTable = env_table(&mainEnv, p.env).unwrap_or_default();
            // L80-82
            if !anyIfValue(&ifVal, |opt| {
                envTable.truthy(opt) || (p.can_imply && implied())
            }) {
                return false;
            }
        }
    }

    // L86 (`varData.ifNode` is never set: module notes)
    if let Some(ifNode) = &IF_NODE {
        // L87-94
        if !anyIfValue(ifNode, |opt| {
            let Some(id) = pob_lua::tonumber(opt.as_str()) else {
                return false;
            };
            // L88
            if spec.allocNodes.with(|m| m.contains_key(id as i64)) {
                return true;
            }
            // L89
            let node = spec.nodes.with(|m| m.get(id as i64));
            // L90
            if let Some(node) = node.filter(|n| n.r#type.get().as_str() == "Keystone") {
                // L91
                return mainEnv
                    .keystonesAdded
                    .get()
                    .truthy(Name::from(&node.dn.get()));
            }
            // L93
            false
        }) {
            return false;
        }
    }
    // L96
    if let Some(ifOption) = varData.ifOption {
        // L97
        if !anyIfValue(&StrOrList::Str(ifOption), |opt| activeInput.truthy(opt)) {
            return false;
        }
    }
    // L99
    if let Some(ifCondTrue) = varData.ifCondTrue {
        // L100
        let conditions = player.modDB.get().conditions.clone();
        if !anyIfValue(&StrOrList::Str(ifCondTrue), |opt| conditions.truthy(opt)) {
            return false;
        }
    }
    // L102
    if let Some(ifStat) = &varData.ifStat {
        // L103-105
        if !anyIfValue(ifStat, |opt| {
            used(mainEnv.perStatsUsed.get(), "perStatsUsed").truthy(opt)
                || used(mainEnv.enemyMultipliersUsed.get(), "enemyMultipliersUsed").truthy(opt)
                || implied()
        }) {
            return false;
        }
    }
    // L107
    if let Some(ifFlag) = varData.ifFlag {
        // L108
        let Some(mainSkill) = &mainSkill else {
            return false;
        };
        // L109 (`mainSkill.skillFlags or {}`: every active skill has one)
        let skillFlags = mainSkill.skillFlags.get();
        // L110
        let skillModList = mainSkill.skillModList.get();
        // L111-113
        if !anyIfValue(&StrOrList::Str(ifFlag), |opt| {
            skillFlags.truthy(opt) || skillModList.Flag(None, &[opt])
        }) {
            return false;
        }
    }
    // L115
    if let Some(ifSkill) = &varData.ifSkill {
        // L116
        let skillsUsed = mainEnv.skillsUsed.get().unwrap_or_default();
        // L117
        if varData.includeTransfigured {
            // L118-124
            if !anyIfValue(ifSkill, |opt| {
                // L119
                if pob_calc::modules::calc_tools::getGameIdFromGemName(Some(opt.as_str()), true)
                    .is_none()
                {
                    return false;
                }
                // L120 ORDER: an any-match (module notes)
                for (skill, _) in skillsUsed.pairs() {
                    let Some(skill) = skill.name() else { continue };
                    // L121
                    if pob_calc::modules::calc_tools::isGemIdSame(
                        skill.as_str(),
                        opt.as_str(),
                        true,
                    ) {
                        return true;
                    }
                }
                // L123
                false
            }) {
                return false;
            }
        } else {
            // L126
            if !anyIfValue(ifSkill, |opt| skillsUsed.truthy(opt)) {
                return false;
            }
        }
    }
    // L129
    if varData.ifSkillFlag.is_some() || varData.ifSkillData.is_some() {
        // L130
        let skillList = player.activeSkillList.get().with(Clone::clone);
        // L131-136
        let anySkillHas =
            |field: fn(&pob_calc::active_skill::ActiveSkill) -> Table, opt: Name| -> bool {
                for s in &skillList {
                    // L133
                    if field(s).truthy(opt) {
                        return true;
                    }
                }
                false
            };
        // L137
        if let Some(f) = varData.ifSkillFlag {
            if !anyIfValue(&StrOrList::Str(f), |opt| {
                anySkillHas(|a| a.skillFlags.get(), opt)
            }) {
                return false;
            }
        }
        // L138
        if let Some(f) = varData.ifSkillData {
            if !anyIfValue(&StrOrList::Str(f), |opt| {
                anySkillHas(|a| a.skillData.get(), opt)
            }) {
                return false;
            }
        }
    }
    // L140
    true
}

/// Options with these properties or label keywords stay hidden even when "Show All
/// Configurations" is on.
/// lua: ConfigVisibility.lua:144-155 isShowAllExcluded(varData)
/// @pob Modules/ConfigVisibility.lua:143-155 h=9345a078
pub fn isShowAllExcluded(varData: &ConfigOptionDef) -> bool {
    // L145
    if varData.ifOption.is_some()
        || varData.ifSkill.is_some()
        || varData.ifSkillData.is_some()
        || varData.ifSkillFlag.is_some()
        || varData.legacy
    {
        // L146
        return true;
    }
    // L148
    if let Some(label) = &varData.label {
        // L149
        let labelLower = pob_lua::string::lower(label);
        // L150
        for kw in EXCLUDE_KEYWORDS {
            // L151
            if pob_lua::pattern::find(&labelLower, kw, None, false).is_some() {
                return true;
            }
        }
    }
    // L154
    false
}

// @pob Modules/ConfigVisibility.lua:157-160 h=cc61de38
// (the module's return table: the two pub functions above)

#[cfg(test)]
mod tests {
    use super::*;

    fn opt(label: &str) -> ConfigOptionDef {
        let v = serde_json::json!({ "label": label });
        serde_json::from_value(v).expect("ConfigOptionDef from json")
    }

    #[test]
    fn show_all_excluded_by_keyword_and_keys() {
        assert!(isShowAllExcluded(&opt("Have you Killed Recently?")));
        assert!(isShowAllExcluded(&opt("# of hits in the past 8 seconds")));
        assert!(isShowAllExcluded(&opt("PvP damage scaling")));
        assert!(!isShowAllExcluded(&opt("Is the enemy a Boss?")));
        let mut o = opt("Plain");
        o.legacy = true;
        assert!(isShowAllExcluded(&o));
        let mut o = opt("Plain");
        o.ifOption = Some(Name::from("x"));
        assert!(isShowAllExcluded(&o));
    }

    #[test]
    fn any_if_value_list_and_single() {
        let l = StrOrList::List(vec![Name::from("a"), Name::from("b")]);
        assert!(anyIfValue(&l, |o| o.as_str() == "b"));
        assert!(!anyIfValue(&l, |o| o.as_str() == "c"));
        assert!(anyIfValue(&StrOrList::Str(Name::from("a")), |o| o.as_str() == "a"));
    }

    #[test]
    fn no_build_is_irrelevant() {
        assert!(!isRelevantForBuild(&opt("x"), None));
    }
}
