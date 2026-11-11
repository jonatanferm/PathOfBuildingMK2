//! @pob-file Modules/Common.lua @16de4b82d
//! Modules/Common.lua 477-566 and 897-962: table helpers, the skill UUID and the GlobalCache.
//! `copyTable` / `wipeTable` are `Table::copyTable` / `Table::wipe` (U0, lua_table.rs).
//! Owner: CT.
#![allow(non_snake_case)]

use crate::global_cache::{clear_all, set_cachedData};
use crate::prelude::*;

/// @pob Modules/Common.lua:488-510 h=22168b4d
/// `copyTableSafe(tbl, noRecurse, preserveMeta, isSubTable)` (inside a `do` block with its
/// `subTableMap` memo: cycles map to the same copy). The memo is wiped at the end of every
/// top-level recursive call (L505-507), so a per-call map is the same thing.
pub fn copyTableSafe(tbl: &Table, noRecurse: bool, preserveMeta: bool) -> Table {
    let mut subTableMap: Vec<(Table, Table)> = Vec::new();
    copy_table_safe(tbl, noRecurse, preserveMeta, false, &mut subTableMap)
}

fn copy_table_safe(
    tbl: &Table,
    noRecurse: bool,
    preserveMeta: bool,
    isSubTable: bool,
    subTableMap: &mut Vec<(Table, Table)>,
) -> Table {
    // L491
    let out = Table::new();
    // L492
    if !noRecurse {
        // L493
        subTableMap.push((tbl.clone(), out.clone()));
    }
    // L495
    if preserveMeta {
        // L496
        out.set_meta(tbl.meta());
    }
    // L498 LUA-ORDER: pairs; the copy's insertion order follows it
    for (k, v) in tbl.pairs() {
        match v {
            // L499
            Val::Table(t) if !noRecurse => {
                // L500
                let known = subTableMap
                    .iter()
                    .find(|(s, _)| s.ptr_eq(&t))
                    .map(|(_, c)| c.clone());
                let c = match known {
                    Some(c) => c,
                    None => copy_table_safe(&t, false, preserveMeta, true, subTableMap),
                };
                out.set(k, c);
            }
            // L502 (immutable data values are shared: equal to a deep copy)
            v => out.set(k, v),
        }
    }
    // L505
    if !noRecurse && !isSubTable {
        // L506
        subTableMap.clear();
    }
    // L508
    out
}

/// @pob Modules/Common.lua:512-522 h=bf7c004f
pub fn mergeDB(srcDB: &ModDB, modDB: Option<&ModDB>) {
    // L513
    if let Some(modDB) = modDB {
        // L514
        srcDB.AddDB(modDB);
        // L515
        for (k, v) in modDB.conditions.pairs() {
            // L516
            srcDB.conditions.set(k, v);
        }
        // L518
        for (k, v) in modDB.multipliers.pairs() {
            // L519
            srcDB.multipliers.set(k, v);
        }
    }
}

/// `store.conditions = copyTable(src)` on a fresh store: fill its (unaliased) table.
fn fill_copy(dst: &Table, src: &Table) {
    for (k, v) in src.copyTable(false).pairs() {
        dst.set(k, v);
    }
}

/// @pob Modules/Common.lua:524-543 h=222b93ee
/// → (modDB, enemyDB, minionDB). Lua reassigns the fresh stores' `conditions`/`multipliers` to
/// copies; the port fills the fresh stores' (unaliased) tables, which is equivalent.
pub fn specCopy(env: &EnvRef) -> (ModDB, ModDB, Option<ModDB>) {
    // L525
    let modDB = ModStore::ModDB(None);
    // L526
    modDB.AddDB(&env.modDB.get());
    // L527
    fill_copy(&modDB.conditions, &env.modDB.get().conditions);
    // L528
    fill_copy(&modDB.multipliers, &env.modDB.get().multipliers);
    // L529
    let enemyDB = ModStore::ModDB(None);
    // L530 (`env.enemyDB` is always set when specCopy runs, CalcSetup.lua:687)
    {
        let src = env.enemyDB.get();
        // L531
        enemyDB.AddDB(&src);
        // L532
        fill_copy(&enemyDB.conditions, &src.conditions);
        // L533
        fill_copy(&enemyDB.multipliers, &src.multipliers);
    }
    // L535
    let mut minionDB = None;
    // L536
    if let Some(minion) = env.minion.get() {
        // L537
        let db = ModStore::ModDB(None);
        // L538
        db.AddDB(&minion.modDB.get());
        // L539
        fill_copy(&db.conditions, &minion.modDB.get().conditions);
        // L540
        fill_copy(&db.multipliers, &minion.modDB.get().multipliers);
        minionDB = Some(db);
    }
    // L542
    (modDB, enemyDB, minionDB)
}

/// @pob Modules/Common.lua:560-566 h=3d9ff30f
/// Search a table for a value, and return the corresponding key
pub fn isValueInTable(tbl: &Table, val: &Val) -> Option<Key> {
    // L561 LUA-ORDER: the first matching key in pairs order
    for (k, v) in tbl.pairs() {
        // L562
        if val.raw_eq(&v) {
            // L563
            return Some(k);
        }
    }
    None
}

/// @pob Modules/Common.lua:569-575 h=b0314d73
/// Search an array for a value, and return the corresponding array index
/// (`ipairs(tbl)`: the 1-based index of the first `v` with `val == v`, else nil).
pub fn isValueInArray<T: PartialEq + ?Sized, E: std::borrow::Borrow<T>>(
    tbl: &[E],
    val: &T,
) -> Option<usize> {
    // L570
    for (i, v) in tbl.iter().enumerate() {
        // L571
        if val == v.borrow() {
            // L572
            return Some(i + 1);
        }
    }
    None
}

/// @pob Modules/Common.lua:897-921 h=e90b67af
/// Generate a UUID for a skill
pub fn cacheSkillUUID(skill: &ActiveSkillRef, env: &EnvRef) -> Name {
    let socketGroup = skill.socketGroup.get();
    // L898 strip spaces
    let strName = pob_lua::gsub(
        &skill.activeEffect.get().grantedEffect.get().name,
        "%s+",
        "",
        None,
    )
    .0;
    // L899 strip spaces
    let slot = match socketGroup.as_ref().and_then(|g| g.slot.get()) {
        Some(s) => pob_lua::string::upper(&s),
        None => "NO_SLOT".to_owned(),
    };
    let strSlotName = pob_lua::gsub(&slot, "%s+", "", None).0;
    // L900
    let mut slotIndx = 1.0;
    // L901
    let mut groupIdx = 1.0;
    // L902
    if let (Some(group), Some(src)) = (&socketGroup, skill.activeEffect.get().srcInstance.get()) {
        // L903
        for_ipairs!((idx, gem) in group.gemList.get() => {
            // L904 we compare table addresses rather than names since two of the same gem
            // L905 can be socketed in the same slot
            // L906
            if Rc::ptr_eq(&gem, &src) {
                // L907
                slotIndx = idx as f64;
                // L908
                break;
            }
        });
    }

    // L913
    for_ipairs!((i, group) in env.build.skillsTab.socketGroupList.get() => {
        // L914
        if socketGroup.as_ref().is_some_and(|g| Rc::ptr_eq(g, &group)) {
            // L915
            groupIdx = i as f64;
            // L916
            break;
        }
    });

    // L920
    cat!(
        strName.as_str(),
        "_",
        strSlotName.as_str(),
        "_",
        tostring_num(slotIndx).as_str(),
        "_",
        tostring_num(groupIdx).as_str()
    )
}

/// @pob Modules/Common.lua:924-941 h=6ebae9ec
/// Global Cache related
pub fn cacheData(uuid: Name, env: &EnvRef) {
    let output = env.player.output.get();
    let mainSkill = env
        .player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
    // L925
    let entry = CachedData {
        Name: mainSkill.activeEffect.get().grantedEffect.get().name,
        Speed: output.get(n!("Speed")),
        HitSpeed: output.get(n!("HitSpeed")),
        ManaCost: output.get(n!("ManaCost")),
        LifeCost: output.get(n!("LifeCost")),
        ESCost: output.get(n!("ESCost")),
        RageCost: output.get(n!("RageCost")),
        HitChance: output.get(n!("HitChance")),
        AccuracyHitChance: output.get(n!("AccuracyHitChance")),
        PreEffectiveCritChance: output.get(n!("PreEffectiveCritChance")),
        CritChance: output.get(n!("CritChance")),
        TotalDPS: output.get(n!("TotalDPS")),
        ActiveSkill: mainSkill,
        Env: env.clone(),
    };
    set_cachedData(env.mode, uuid, Some(Rc::new(entry)));
}

/// @pob Modules/Common.lua:944-948 h=56bb634a
/// Wipe all the tables associated with Global Cache
pub fn wipeGlobalCache() {
    // L945-947 (wipeTable of MAIN, CALCS and CALCULATOR)
    clear_all();
}

/// @pob Modules/Common.lua:951-958 h=f2a0525b
/// Check if a specific named gem is enabled in a socket group belonging to a skill
pub fn supportEnabled(skillName: Name, activeSkill: &ActiveSkillRef) -> bool {
    let socketGroup = activeSkill
        .socketGroup
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'socketGroup' (a nil value)"));
    // L952
    let mut found: Option<bool> = None;
    for_ipairs!((_, gemInstance) in socketGroup.gemList.get() => {
        // L953
        if gemInstance.skillId.get() == Some(skillName) {
            // L954
            found = Some(gemInstance.enabled.get());
            break;
        }
    });
    // L957
    found.unwrap_or(true)
}
