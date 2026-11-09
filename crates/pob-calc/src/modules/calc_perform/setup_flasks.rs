//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! perform segment PF-C (CalcPerform.lua 1289-1942): flasks, tinctures, minion setup, keystones.
//!
//! Owner: PF-C. Signatures of `seg`, `mergeFlasks` and `mergeTinctures` are U0's (frozen).
//!
//! The segment declares the local functions `mergeFlasks` (1691) and `mergeTinctures` (1862),
//! which later segments call (PF-F 3121, PF-D 2095; calc doc §7.1). Their Lua upvalues are
//! segment locals the frozen `PerformCtx` does not carry (the flask `effectInc`… and
//! `flaskDurInc`, the tincture `effectInc`, `effectIncMagic` and the mutable `tinctureLimit`):
//! CCR-0078 §1. Until it is applied the segment records them per env in a thread-local side
//! table ([`MERGE_UPVALUES`]) where the closures are created, and the frozen entry points read
//! them from there.
//!
//! Callees that are not ported yet (createMinionSkills [AS], initMinionModDB [PF-B],
//! applyEnemyModifiers and mergeBuff [PF-A]) are called through [`callee`]: with the `probe`
//! feature, a call that only hit an `unported!` stub is handed to the L4 test's replay hook
//! ([`callee_replay`]; CCR-0078 §2). Without the feature `callee` is the plain call.
#![allow(non_snake_case, dead_code, clippy::all, clippy::pedantic)]

use std::cell::RefCell;

use pob_gamedata::{mod_tools, tag};

use super::actor_a::{applyEnemyModifiers, mergeBuff};
use super::actor_b::initMinionModDB;
use super::PerformCtx;
use crate::modules::calc_active_skill;
use crate::modules::calc_tools as calcLib;
use crate::modules::mod_tools_env;
use crate::prelude::*;

// ---- Rust-only plumbing ----------------------------------------------------------------------

/// CCR-0078 §1: the upvalues of the local function `mergeFlasks` (CalcPerform.lua 1691),
/// captured when the closure is created (the locals declared at 1655-1664).
struct FlaskUpvalues {
    effectInc: f64,
    effectIncMagic: f64,
    effectIncMagicNoAdjacent: f64,
    effectIncNonPlayer: f64,
    effectIncMagicNonPlayer: f64,
    flasksApplyToMinion: bool,
    quickSilverAppliesToAllies: bool,
    flaskTotalRateInc: f64,
    flaskDurInc: f64,
}

/// CCR-0078 §1: the upvalues of the local function `mergeTinctures` (CalcPerform.lua 1862):
/// the tincture `effectInc`/`effectIncMagic` (1851-1852, shadowing the flask ones) and
/// `tinctureLimit` (1853), which `mergeTinctures` decrements (1909), so it is shared.
struct TinctureUpvalues {
    effectInc: f64,
    effectIncMagic: f64,
    tinctureLimit: Cell<f64>,
}

/// One `calcs.perform` call's closures, by env (CCR-0078 §1).
struct MergeUpvalues {
    env: Weak<Env>,
    flasks: Option<Rc<FlaskUpvalues>>,
    tinctures: Option<Rc<TinctureUpvalues>>,
}

thread_local! {
    /// CCR-0078 §1: the upvalues of the latest `mergeFlasks` / `mergeTinctures` closures of
    /// each live env (one perform at a time per env; a new perform replaces the entry).
    static MERGE_UPVALUES: RefCell<Vec<MergeUpvalues>> = const { RefCell::new(Vec::new()) };
}

fn record_upvalues(env: &EnvRef, f: impl FnOnce(&mut MergeUpvalues)) {
    MERGE_UPVALUES.with(|m| {
        let mut m = m.borrow_mut();
        m.retain(|e| e.env.strong_count() > 0);
        let i = match m
            .iter()
            .position(|e| std::ptr::eq(e.env.as_ptr(), Rc::as_ptr(env)))
        {
            Some(i) => i,
            None => {
                m.push(MergeUpvalues {
                    env: Rc::downgrade(env),
                    flasks: None,
                    tinctures: None,
                });
                m.len() - 1
            }
        };
        f(&mut m[i]);
    });
}

fn flask_upvalues(env: &EnvRef) -> Rc<FlaskUpvalues> {
    MERGE_UPVALUES
        .with(|m| {
            m.borrow()
                .iter()
                .find(|e| std::ptr::eq(e.env.as_ptr(), Rc::as_ptr(env)))
                .and_then(|e| e.flasks.clone())
        })
        .unwrap_or_else(|| {
            lua_error!("CCR-0078: mergeFlasks called before its closure was created (upvalues of CalcPerform.lua 1655-1664 not captured for this env)")
        })
}

fn tincture_upvalues(env: &EnvRef) -> Rc<TinctureUpvalues> {
    MERGE_UPVALUES
        .with(|m| {
            m.borrow()
                .iter()
                .find(|e| std::ptr::eq(e.env.as_ptr(), Rc::as_ptr(env)))
                .and_then(|e| e.tinctures.clone())
        })
        .unwrap_or_else(|| {
            lua_error!("CCR-0078: mergeTinctures called before its closure was created (upvalues of CalcPerform.lua 1851-1853 not captured for this env)")
        })
}

/// CCR-0078 §2: callee replay for the L4 test (feature `probe` only). A call of a callee
/// whose body is still an `unported!` stub is reported to the installed hook with its
/// arguments, so the test can apply the callee's recorded effect in its place.
#[cfg(feature = "probe")]
pub mod callee_replay {
    use super::*;

    /// `hook(callee, args)`: called after a callee call that only hit its stub.
    pub type Hook = Box<dyn FnMut(&'static str, &[Val])>;

    thread_local! {
        static HOOK: RefCell<Option<Hook>> = const { RefCell::new(None) };
    }

    /// Install (or with `None` remove) the replay hook of this thread.
    pub fn set_hook(h: Option<Hook>) {
        HOOK.with(|s| *s.borrow_mut() = h);
    }

    pub(super) fn stub_hits(site: &str) -> u64 {
        crate::stub::stub_report()
            .into_iter()
            .find(|(s, _)| *s == site)
            .map_or(0, |(_, n)| n)
    }

    /// Report a finished callee call to the hook: always for `site == None`, else only when
    /// the call hit the stub `site` (its count moved from `before`).
    pub(super) fn after(
        site: Option<&'static str>,
        name: &'static str,
        before: u64,
        args: impl FnOnce() -> Vec<Val>,
    ) {
        if site.is_some_and(|site| stub_hits(site) == before) {
            return;
        }
        let Some(mut h) = HOOK.with(|s| s.borrow_mut().take()) else {
            return;
        };
        h(name, &args());
        HOOK.with(|s| {
            let mut s = s.borrow_mut();
            if s.is_none() {
                *s = Some(h);
            }
        });
    }

    /// `key` of the object `ptr` is set in Lua now (the exporter leaves out a default-valued
    /// typed field that the injected dump had as nil; CCR-0078 §5).
    pub(super) fn present(ptr: usize, key: &str) {
        crate::probe::unmark_absent(ptr, key);
    }
}

/// A call of a callee from another unit (see the module doc): `f()`, plus, with the `probe`
/// feature, the replay hook when the call only hit the callee's stub `site`.
#[inline]
fn callee(
    site: &'static str,
    name: &'static str,
    args: impl FnOnce() -> Vec<Val>,
    f: impl FnOnce(),
) {
    #[cfg(feature = "probe")]
    let before = callee_replay::stub_hits(site);
    f();
    #[cfg(feature = "probe")]
    callee_replay::after(Some(site), name, before, args);
    #[cfg(not(feature = "probe"))]
    let _ = (site, name, args);
}

/// `modLib.mergeKeystones(env, env.modDB)` (CT). With the `probe` feature the call is also
/// reported to the replay hook: the injected L4 graph has no passive tree to resolve
/// `env.spec.tree.keystoneMap` against until PT/BM-A land (CCR-0078 §4).
fn merge_keystones(env: &EnvRef) {
    mod_tools_env::mergeKeystones(env, &env.modDB.get());
    #[cfg(feature = "probe")]
    callee_replay::after(None, "mergeKeystones", 0, || vec![obj(env)]);
}

/// The Lua key `key` of the object behind `rc` was assigned (see [`callee_replay::present`]).
#[inline]
fn present<T: ?Sized>(rc: &Rc<T>, key: &str) {
    #[cfg(feature = "probe")]
    callee_replay::present(crate::probe::rc_ptr(rc), key);
    #[cfg(not(feature = "probe"))]
    let _ = (rc, key);
}

fn obj<T: 'static>(v: &Rc<T>) -> Val {
    Val::Obj(v.clone() as Rc<dyn std::any::Any>)
}

/// `cfg = { actor = "player" }`.
fn cfg_actor_player() -> Cfg {
    let c = ModCfg::new();
    c.actor.set(Some(n!("player")));
    c
}

/// `cfg = { source = "Skill" }`.
fn cfg_source_skill() -> Cfg {
    let c = ModCfg::new();
    c.source.set(Some(n!("Skill")));
    c
}

/// `modDB:NewMod(name, type, value)` without a source (`modLib.createMod` with no varargs:
/// `source` nil, flags 0).
fn new_mod_nosrc(store: &ModStore, name: Name, ty: ModType, value: impl Into<Value>) {
    store.AddMod(Arc::new(mod_tools::createMod(
        name,
        ty,
        value,
        None,
        0,
        0,
        &[],
    )));
}

/// The mods of a merged buff (`destTable[destKey]` of mergeBuff): a ModList object, or an
/// array table of mods (PF-A's representation of `new("ModList"):ModList()` in a table).
fn buff_mods(v: &Val) -> Vec<ModRef> {
    if let Some(store) = v.obj::<ModStore>() {
        return store.ipairs();
    }
    match v {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .map(|(_, m)| match m {
                Val::Mod(m) => m,
                other => lua_error!(
                    "merged buff entry is a {} value, not a mod",
                    other.lua_type()
                ),
            })
            .collect(),
        other => lua_error!(
            "merged buff is a {} value, not a mod list",
            other.lua_type()
        ),
    }
}

/// A ModList as the frozen `mergeBuff(src: &Table, …)` takes it: the array of its mods
/// (`ipairs(src)` is all mergeBuff reads; CCR-0078 §3).
fn mods_table(list: &ModList) -> Table {
    let t = Table::new();
    for m in list.ipairs() {
        t.push(Val::Mod(m));
    }
    t
}

/// `mergeBuff(src, destTable, destKey)` (PF-A), through [`callee`].
fn merge_buff(src: &Table, destTable: &Table, destKey: Name) {
    callee(
        "Modules/CalcPerform.lua:45-67",
        "mergeBuff",
        || {
            vec![
                Val::Table(src.clone()),
                Val::Table(destTable.clone()),
                Val::from(destKey),
            ]
        },
        || mergeBuff(src, destTable, destKey),
    );
}

/// `destTable[k]` for a table `mergeBuff` will index (`flaskBuffsPerBase[item.baseName]`).
fn sub_table(t: &Table, k: Name) -> Table {
    match t.get(k) {
        Val::Table(x) => x,
        other => lua_error!("attempt to index a {} value", other.lua_type()),
    }
}

fn item_baseName(item: &Item) -> Name {
    item.baseName
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'baseName' (a nil value)"))
}

fn item_flaskData(item: &Item) -> Table {
    item.flaskData
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'flaskData' (a nil value)"))
}

fn item_flask_base(item: &Item) -> pob_gamedata::schema::bases::FlaskBase {
    let base = item
        .base
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'base' (a nil value)"));
    base.flask
        .clone()
        .unwrap_or_else(|| lua_error!("attempt to index field 'flask' (a nil value)"))
}

fn item_list_mods(l: Option<ModList>, field: &str) -> Vec<ModRef> {
    match l {
        Some(l) => l.ipairs(),
        None => lua_error!("attempt to index field '{field}' (a nil value)"),
    }
}

/// `"Using"..item.baseName:gsub("%s+", "")`.
fn using_name(baseName: Name) -> Name {
    let (s, _) = pob_lua::gsub(baseName.as_str(), "%s+", "", None);
    cat!("Using", s)
}

/// `item.title` as a table key (a nil key raises in mergeBuff's assignment).
fn title_key(item: &Item) -> Name {
    match item.title.get() {
        Some(t) => Name::from(&*t),
        None => lua_error!("table index is nil"),
    }
}

// ---- the segment ----------------------------------------------------------------------------

/// @pob Modules/CalcPerform.lua:1289-1942 h=f67a66ce
/// PF-C: CalcPerform.lua 1289-1942 (flasks, tinctures, minion setup, keystones).
pub(crate) fn seg(ctx: &mut PerformCtx) -> Flow {
    let env = ctx.env.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();

    // Merge keystone modifiers
    // L1291
    env.keystonesAdded.set(Table::new());
    present(&env, "keystonesAdded");
    // L1292
    merge_keystones(&env);

    // Build minion skills
    // L1295
    for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
        // L1296
        activeSkill
            .skillModList
            .set(ModStore::ModList(Some(activeSkill.baseSkillModList.get())));
        present(&activeSkill, "skillModList");
        // L1297
        if let Some(minion) = activeSkill.minion.get() {
            // L1298
            minion.modDB.set(ModStore::ModDB(None));
            present(&minion, "modDB");
            // L1299
            minion.modDB.get().actor.set(Rc::downgrade(&minion));
            present(&minion.modDB.get(), "actor");
            // L1300
            callee(
                "Modules/CalcActiveSkill.lua:878-934",
                "createMinionSkills",
                || vec![obj(&env), obj(&activeSkill)],
                || calc_active_skill::createMinionSkills(&env, &activeSkill),
            );
            // L1301 (`activeSkill.minion` is read again)
            let mainSkill = activeSkill
                .minion
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'minion' (a nil value)"))
                .mainSkill
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
            let name = mainSkill.activeEffect.get().grantedEffect.get().name;
            activeSkill.skillPartName.set(Some(LStr::from(name.as_str())));
        }
    });

    // L1305
    env.player.output.set(Table::new());
    present(&env.player, "output");
    // L1306
    env.enemy.output.set(Table::new());
    present(&env.enemy, "output");
    // L1308
    ctx.output = env.player.output.get();
    let output = ctx.output.clone();

    // L1310
    env.partyMembers.set(Some(env.build.partyTab.actor.clone()));
    // L1311
    env.player.partyMembers.set(
        env.partyMembers
            .get()
            .as_ref()
            .map_or_else(Weak::new, Rc::downgrade),
    );
    present(&env.player, "partyMembers");
    // L1312
    ctx.partyTabEnableExportBuffs =
        env.build.partyTab.enableExportBuffs.get() && env.mode != Mode::CALCULATOR;

    // Calculator passes reuse the environment after the player's shield has been removed.
    // L1315
    let aegisItem = if env.aegisModList.get().is_some() {
        env.player
            .itemList
            .with(|l| l.get(&n!("Weapon 2")).cloned())
            .or_else(|| env.aegisItem.get())
    } else {
        None
    };
    env.aegisItem.set(aegisItem);
    // L1316
    let mainSkill = env
        .player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
    env.minion.set(mainSkill.minion.get());
    // L1317
    if env.minion.get().is_some() {
        // Initialise minion modifier database
        // L1319
        output.set(n!("Minion"), Table::new());
        // L1320
        let minionOutput = output.table(n!("Minion"));
        callee(
            "Modules/CalcPerform.lua:1154-1244",
            "initMinionModDB",
            || {
                vec![
                    obj(&env),
                    obj(&mainSkill),
                    minionOutput.clone().map_or(Val::Nil, Val::Table),
                ]
            },
            || initMinionModDB(&env, &mainSkill, minionOutput.as_ref()),
        );
    }
    // L1322
    if env.aegisModList.get().is_some() {
        // L1323
        env.player.itemList.with_mut(|l| {
            l.shift_remove(&n!("Weapon 2"));
        });
    }
    // L1325
    if modDB.Flag(None, nm!["AlchemistsGenius"]) {
        // L1326
        let effectMod = 1.0 + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]) / 100.0;
        // L1327
        modDB.NewMod(
            n!("FlaskEffect"),
            ModType::INC,
            m_floor(10.0 * effectMod),
            "Alchemist's Genius",
            0,
            0,
            &[],
        );
        // L1328
        modDB.NewMod(
            n!("FlaskChargesGained"),
            ModType::INC,
            m_floor(20.0 * effectMod),
            "Alchemist's Genius",
            0,
            0,
            &[],
        );
    }

    // L1331
    ctx.hasGuaranteedBonechill = false;
    // L1332
    ctx.guaranteedChillSkills = Table::new();

    // Banners
    // L1335
    if modDB.Flag(None, nm!["Condition:BannerPlanted"]) {
        // L1336
        let max = modDB.Sum(ModType::BASE, None, nm!["MaximumValour"]);
        // L1337
        let stacks = modDB.Sum(ModType::BASE, None, nm!["Multiplier:ValourStacks"]);
        // L1338
        modDB.NewMod(
            n!("Multiplier:BannerValour"),
            ModType::BASE,
            m_min(stacks, max),
            "Base",
            0,
            0,
            &[],
        );
    }

    // L1341
    if modDB.Flag(None, nm!["CryWolfMinimumPower"])
        && modDB.Sum(ModType::BASE, None, nm!["WarcryPower"]) < 10.0
    {
        // L1342
        modDB.NewMod(
            n!("WarcryPower"),
            ModType::OVERRIDE,
            10.0,
            "Minimum Warcry Power from CryWolf",
            0,
            0,
            &[],
        );
    }
    // L1344
    if modDB.Flag(None, nm!["WarcryInfinitePower"]) {
        // L1345
        modDB.NewMod(
            n!("WarcryPower"),
            ModType::OVERRIDE,
            999999.0,
            "Warcries have infinite power",
            0,
            0,
            &[],
        );
    }
    // L1347 (`Sum` is a number, so the trailing `or 0` never applies)
    let warcryPower = lor(
        Val::from_value(&modDB.Override(None, nm!["WarcryPower"])),
        || Val::Num(modDB.Sum(ModType::BASE, None, nm!["WarcryPower"])),
    );
    output.set(n!("WarcryPower"), warcryPower);
    // L1348
    modDB
        .multipliers
        .set(n!("WarcryPower"), output.get(n!("WarcryPower")));

    // L1350
    callee(
        "Modules/CalcPerform.lua:619-633",
        "applyEnemyModifiers",
        || vec![obj(&env.player), Val::Bool(true)],
        || applyEnemyModifiers(&env.player, true),
    );
    // L1351
    if let Some(minion) = env.minion.get() {
        // L1352
        callee(
            "Modules/CalcPerform.lua:619-633",
            "applyEnemyModifiers",
            || vec![obj(&minion), Val::Bool(true)],
            || applyEnemyModifiers(&minion, true),
        );
    }
    // L1354
    callee(
        "Modules/CalcPerform.lua:619-633",
        "applyEnemyModifiers",
        || vec![obj(&env.enemy), Val::Bool(true)],
        || applyEnemyModifiers(&env.enemy, true),
    );
    // L1355 (string keys; `pairs` below visits them in insertion order, PoB in hash order)
    let mut minionCounts: indexmap::IndexMap<Name, MinionCounts> = indexmap::IndexMap::new();

    // L1357
    for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
        let skillTypes = activeSkill.skillTypes.get();
        let has = |t: SkillType::T| skillTypes.with(|s| s.has(t));
        let skillModList = activeSkill.skillModList.get();
        let skillCfg = activeSkill.skillCfg.get();
        let skillFlags = activeSkill.skillFlags.get();
        let skillData = activeSkill.skillData.get();
        let grantedEffect = activeSkill.activeEffect.get().grantedEffect.get();
        // L1358
        if has(SkillType::Brand) {
            // L1359
            let attachLimit = m_min(
                skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["BrandsAttachedLimit"]),
                skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["ActiveBrandLimit"]),
            );
            // L1360
            let configured = modDB.Sum(ModType::BASE, None, nm!["Multiplier:ConfigBrandsAttachedToEnemy"]);
            // L1361
            let attached = if configured > 0.0 { m_min(configured, attachLimit) } else { attachLimit };
            // L1362
            skillData.set(n!("attachedBrandCount"), attached);
            // L1363
            let activeBrands = modDB.Sum(ModType::BASE, None, nm!["Multiplier:ConfigActiveBrands"]);
            // Cap the number of active brands by the limit, which is 3 by default
            // Also consider increase to number of active brands from other sources (e.g. Foulgrasp Support)
            // L1366
            let v = m_max(
                m_min(activeBrands, skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["ActiveBrandLimit"])),
                modDB.multipliers.num_or(n!("ActiveBrand"), 0.0),
            );
            modDB.multipliers.set(n!("ActiveBrand"), v);
            // L1367
            let v = m_max(attached, modDB.multipliers.num_or(n!("BrandsAttachedToEnemy"), 0.0));
            modDB.multipliers.set(n!("BrandsAttachedToEnemy"), v);
            // L1368
            let v = m_max(attached, enemyDB.multipliers.num_or(n!("BrandsAttached"), 0.0));
            enemyDB.multipliers.set(n!("BrandsAttached"), v);
        }
        // L1370
        if skillFlags.truthy(n!("totem")) {
            // L1371
            let main = env
                .player
                .mainSkill
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"));
            let limit = main.skillModList.get().Sum(
                ModType::BASE,
                Some(&main.skillCfg.get()),
                nm!["ActiveTotemLimit", "ActiveBallistaLimit"],
            );
            // L1372
            let v = m_max(limit, output.num_or(n!("ActiveTotemLimit"), 0.0));
            output.set(n!("ActiveTotemLimit"), v);
            // L1373
            let v = lor(Val::from_value(&modDB.Override(None, nm!["TotemsSummoned"])), || {
                output.get(n!("ActiveTotemLimit"))
            });
            output.set(n!("TotemsSummoned"), v);
            // L1374
            let v = m_max(
                output.num_or(n!("TotemsSummoned"), 0.0),
                enemyDB.multipliers.num_or(n!("TotemsSummoned"), 0.0),
            );
            enemyDB.multipliers.set(n!("TotemsSummoned"), v);
        }
        // The actual hexes as opposed to hex related skills all have the curse flag. TotemCastsWhenNotDetached is to remove blasphemy
        // Note that this doesn't work for triggers yet, insufficient support
        // L1378 (`Sum` is a number, so its own truth test always passes; it is still evaluated)
        if skillFlags.truthy(n!("hex"))
            && skillFlags.truthy(n!("curse"))
            && !has(SkillType::TotemCastsWhenNotDetached)
            && {
                let _ = skillModList.Sum(ModType::BASE, None, nm!["MaxDoom"]);
                true
            }
        {
            // L1379
            let hexDoom = modDB.Sum(ModType::BASE, None, nm!["Multiplier:HexDoomStack"]);
            // L1380
            let maxDoom = skillModList.Sum(ModType::BASE, None, nm!["MaxDoom"]);
            // L1381
            let doomEffect = skillModList.More(None, nm!["DoomEffect"]);
            // Update the max doom limit
            // L1383
            let v = m_max(maxDoom, output.num_or(n!("HexDoomLimit"), 0.0));
            output.set(n!("HexDoomLimit"), v);
            // Update the Hex Doom to apply
            // L1385
            skillModList.NewMod(n!("CurseEffect"), ModType::INC, m_min(hexDoom, maxDoom) * doomEffect, "Doom", 0, 0, &[]);
            // L1386
            let v = m_min(
                m_max(hexDoom, modDB.multipliers.num_or(n!("HexDoom"), 0.0)),
                output.num(n!("HexDoomLimit")),
            );
            modDB.multipliers.set(n!("HexDoom"), v);
        }
        // L1388
        if grantedEffect.name.as_str() == "Vaal Lightning Trap" || grantedEffect.name.as_str() == "Shock Ground" {
            // Shock effect applies to shocked ground
            // L1390
            let effect = skillModList.Sum(ModType::BASE, None, nm!["ShockedGroundEffect"])
                * (1.0 + skillModList.Sum(ModType::INC, None, nm!["EnemyShockEffect"]) / 100.0);
            // L1391
            modDB.NewMod(
                n!("ShockOverride"),
                ModType::BASE,
                effect,
                "Shocked Ground",
                0,
                0,
                &[tag!({ type = ActorCondition, actor = "enemy", var = "OnShockedGround" })],
            );
        }
        // L1393
        if skillData.truthy(n!("supportBonechill"))
            && (has(SkillType::ChillingArea)
                || has(SkillType::NonHitChill)
                || !skillModList.Flag(None, nm!["CannotChill"]))
        {
            // L1394
            output.set(n!("HasBonechill"), true);
        }
        // L1396
        if grantedEffect.name.as_str() == "Summon Skitterbots" {
            let src = grantedEffect.name.as_str();
            // L1397
            let skitterbotAilmentEffect = skillModList.Sum(ModType::INC, None, nm!["SkitterbotAilmentEffect"]);
            // L1398
            if !skillModList.Flag(None, nm!["SkitterbotsCannotShock"]) {
                // L1399
                let effect = ailment_default(n!("Shock"))
                    * (1.0
                        + (skillModList.Sum(ModType::INC, Some(&cfg_source_skill()), nm!["EnemyShockEffect"])
                            + skitterbotAilmentEffect)
                            / 100.0);
                // L1400
                modDB.NewMod(n!("ShockOverride"), ModType::BASE, effect, src, 0, 0, &[]);
                // L1401
                enemyDB.NewMod(n!("Condition:Shocked"), ModType::FLAG, true, src, 0, 0, &[]);
                // L1402
                if skillModList.Flag(None, nm!["SkitterbotAffectPlayer"]) {
                    // L1403
                    modDB.NewMod(n!("Shock"), ModType::FLAG, true, src, 0, 0, &[]);
                    // L1404
                    modDB.NewMod(n!("SelfShockOverride"), ModType::BASE, effect, src, 0, 0, &[]);
                }
            }
            // L1407
            if !skillModList.Flag(None, nm!["SkitterbotsCannotChill"]) {
                // L1408
                let effect = ailment_default(n!("Chill"))
                    * (1.0
                        + (skillModList.Sum(ModType::INC, Some(&cfg_source_skill()), nm!["EnemyChillEffect"])
                            + skitterbotAilmentEffect)
                            / 100.0);
                // L1409
                modDB.NewMod(n!("ChillOverride"), ModType::BASE, effect, src, 0, 0, &[]);
                // L1410
                enemyDB.NewMod(n!("Condition:Chilled"), ModType::FLAG, true, src, 0, 0, &[]);
                // L1411
                if skillModList.Flag(None, nm!["SkitterbotAffectPlayer"]) {
                    // L1412
                    modDB.NewMod(n!("Chill"), ModType::FLAG, true, src, 0, 0, &[]);
                    // L1413
                    modDB.NewMod(n!("SelfChillOverride"), ModType::BASE, effect, src, 0, 0, &[]);
                }
                // L1415
                if skillData.truthy(n!("supportBonechill")) {
                    // L1416
                    ctx.hasGuaranteedBonechill = true;
                    // L1417
                    modDB.NewMod(n!("SkitterbotBonechill"), ModType::FLAG, true, src, 0, 0, &[]);
                }
            }
            // L1420
            if skillModList.Flag(None, nm!["ScorchingSkitterbot"]) {
                // L1421
                let effect = ailment_default(n!("Scorch"))
                    * (1.0
                        + (skillModList.Sum(ModType::INC, Some(&cfg_source_skill()), nm!["EnemyScorchEffect"])
                            + skitterbotAilmentEffect)
                            / 100.0);
                // L1422
                modDB.NewMod(n!("ScorchOverride"), ModType::BASE, effect, src, 0, 0, &[]);
                // L1423
                enemyDB.NewMod(n!("Condition:Scorched"), ModType::FLAG, true, src, 0, 0, &[]);
                // L1424
                if skillModList.Flag(None, nm!["SkitterbotAffectPlayer"]) {
                    // L1425
                    modDB.NewMod(n!("Scorch"), ModType::FLAG, true, src, 0, 0, &[]);
                    // L1426
                    modDB.NewMod(n!("SelfScorchOverride"), ModType::BASE, effect, src, 0, 0, &[]);
                }
            }
        } else if skillFlags.truthy(n!("guaranteedChill"))
            || has(SkillType::ChillingArea)
            || (has(SkillType::NonHitChill) && !skillModList.Flag(None, nm!["CannotChill"]))
        {
            // L1429
            // L1430
            ctx.guaranteedChillSkills.push(obj(&activeSkill));
            // L1431
            enemyDB.NewMod(n!("Condition:Chilled"), ModType::FLAG, true, grantedEffect.name.as_str(), 0, 0, &[]);
            // L1432
            if skillData.truthy(n!("supportBonechill")) {
                // L1433
                ctx.hasGuaranteedBonechill = true;
            }
        }
        // Count active minions. Skills without a limit contribute one minion.
        // L1437
        let minionList = activeSkill.minionList.get();
        // L1438
        if !skillFlags.truthy(n!("disable")) && minionList.len() >= 1 {
            // L1439
            let grantedEffect = activeSkill.activeEffect.get().grantedEffect.get();
            // L1440
            for_ipairs!((_, minionType) in minionList => {
                // L1441
                let minionData = env.data.minions.get(&minionType).cloned();
                // L1442
                if let Some(minionData) = minionData.filter(|d| !d.hostile) {
                    // L1443
                    let key = match minionData.limit {
                        Some(limit) => limit,
                        None => cat!(grantedEffect.id, ":", minionType),
                    };
                    // L1444
                    let mut count = 1.0;
                    // L1445
                    if let Some(limit) = minionData.limit {
                        // L1446
                        count = m_floor(
                            lor(Val::from_value(&modDB.Override(None, &[limit])), || {
                                Val::Num(
                                    calcLib::val(&skillModList, limit, None)
                                        * skillModList.More(Some(&skillCfg), nm!["ActiveMinionLimit"]),
                                )
                            })
                            .num(),
                        );
                        // L1447
                        let v = m_max(count, output.num_or(limit, 0.0));
                        output.set(limit, v);
                    }
                    // L1449
                    let mut counts = minionCounts.get(&key).cloned().unwrap_or_default();
                    // L1450
                    counts.total = Some(m_max(count, counts.total.unwrap_or(0.0)));
                    // L1451
                    if !has(SkillType::Vaal) {
                        // L1452
                        counts.nonVaal = Some(m_max(count, counts.nonVaal.unwrap_or(0.0)));
                    }
                    // L1454
                    if skillFlags.truthy(n!("permanentMinion")) {
                        // L1455
                        counts.permanent = Some(m_max(count, counts.permanent.unwrap_or(0.0)));
                    }
                    // L1457
                    minionCounts.insert(key, counts);
                }
            });
        }
        // L1461
        if has(SkillType::CreatesMinion) && !has(SkillType::MinionsAreUndamagable) {
            // L1462
            new_mod_nosrc(&modDB, n!("Condition:HaveDamageableMinion"), ModType::FLAG, true);
        }
        // L1464
        if env.mode_buffs.get() && skillFlags.truthy(n!("warcry")) {
            let mainCfg = || {
                env.player
                    .mainSkill
                    .get()
                    .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
                    .skillCfg
                    .get()
            };
            // L1465
            if grantedEffect.name.as_str() == "Rallying Cry"
                && !skillModList.Flag(None, nm!["CannotShareWarcryBuffs"])
                && !modDB.Flag(None, nm!["RallyingActive"])
            {
                // L1466
                let v = skillModList.Sum(ModType::BASE, Some(&mainCfg()), nm!["RallyingCryExertDamageBonus"]);
                new_mod_nosrc(&env.player.modDB.get(), n!("RallyingExertMoreDamagePerAlly"), ModType::BASE, v);
                // L1467 -- Prevents effect from applying multiple times
                new_mod_nosrc(&modDB, n!("RallyingActive"), ModType::FLAG, true);
            } else if grantedEffect.name.as_str() == "Seismic Cry" && !modDB.Flag(None, nm!["SeismicActive"]) {
                // L1468
                // L1469
                let v = skillModList.Sum(ModType::BASE, Some(&mainCfg()), nm!["SeismicAoEMoreMultiplier"]);
                new_mod_nosrc(&env.player.modDB.get(), n!("SeismicMoreAoE"), ModType::BASE, v);
                // L1470 -- Prevents effect from applying multiple times
                new_mod_nosrc(&modDB, n!("SeismicActive"), ModType::FLAG, true);
            }
        }
        // L1473
        if skillData.truthy(n!("triggeredOnDeath")) && !skillFlags.truthy(n!("minion")) {
            // L1474
            skillData.set(n!("triggered"), true);
            let mainCfg = env
                .player
                .mainSkill
                .get()
                .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
                .skillCfg
                .get();
            // L1475
            for value in skillModList.Tabulate(Some(ModType::INC), Some(&mainCfg), nm!["TriggeredDamage"]) {
                // L1476
                let m = &value.r#mod;
                skillModList.AddMod(Arc::new(mod_tools::createMod(
                    n!("Damage"),
                    ModType::INC,
                    m.value.clone(),
                    m.source.as_deref(),
                    m.flags,
                    m.keywordFlags,
                    &m.tags,
                )));
            }
            // L1478
            for value in skillModList.Tabulate(Some(ModType::MORE), Some(&mainCfg), nm!["TriggeredDamage"]) {
                // L1479
                let m = &value.r#mod;
                skillModList.AddMod(Arc::new(mod_tools::createMod(
                    n!("Damage"),
                    ModType::MORE,
                    m.value.clone(),
                    m.source.as_deref(),
                    m.flags,
                    m.keywordFlags,
                    &m.tags,
                )));
            }
            // Set trigger time to 1 min in ms ( == 6000 ). Technically any large value would do.
            // L1482
            skillData.set(n!("triggerTime"), 60.0 * 1000.0);
        }
        // The Saviour
        // L1485
        if grantedEffect.name.as_str() == "Reflection" || skillData.truthy(n!("triggeredBySaviour")) {
            // L1486
            activeSkill
                .infoMessage
                .set(Some(LStr::from("Triggered by a Crit from The Saviour")));
            // L1487
            activeSkill.infoTrigger.set(Some(LStr::from("Saviour")));
        }
    });

    // L1491
    let mut summonedMinions = 0.0;
    // L1492 LUA-ORDER: string-keyed `pairs` (PoB: hash order; here insertion order). Only the
    // order of the NewMods below depends on it (the stores compare as multisets).
    for (_, counts) in &minionCounts {
        let total = counts.total.expect("counts.total is set at 1450");
        // L1493
        summonedMinions = summonedMinions + total;
        // L1494
        modDB.NewMod(
            n!("Multiplier:SummonedMinion"),
            ModType::BASE,
            total,
            "Config",
            0,
            0,
            &[tag!({ type = Condition, var = "Combat" })],
        );
        // L1495
        if let Some(nonVaal) = counts.nonVaal {
            // L1496
            modDB.NewMod(
                n!("Multiplier:NonVaalSummonedMinion"),
                ModType::BASE,
                nonVaal,
                "Config",
                0,
                0,
                &[tag!({ type = Condition, var = "Combat" })],
            );
        }
        // L1498
        if let Some(permanent) = counts.permanent {
            // L1499
            modDB.NewMod(
                n!("Multiplier:PermanentMinion"),
                ModType::BASE,
                permanent,
                "Config",
                0,
                0,
                &[tag!({ type = Condition, var = "Combat" })],
            );
        }
    }

    // Companionship only works while exactly one Minion is summoned.
    // L1504
    let summonedMinionOverride = env.configInput.get(n!("multiplierSummonedMinion"));
    // L1505 (`x ~= 0 and x or nil`; a non-number config value cannot be held by the frozen
    // `Option<f64>` local: it is read as nil)
    let summonedMinionOverride = match summonedMinionOverride {
        Val::Num(x) if x != 0.0 => Some(x),
        _ => None,
    };
    ctx.summonedMinionOverride = summonedMinionOverride;
    // L1506
    modDB.conditions.set(
        n!("OnlyMinion"),
        summonedMinionOverride.unwrap_or(summonedMinions) == 1.0,
    );

    // Special Rarity / Quantity Calc for Bisco's
    // L1509
    let lootQuantityNormalEnemies = modDB.Sum(ModType::INC, None, nm!["LootQuantityNormalEnemies"]);
    // L1510
    let v = if lootQuantityNormalEnemies > 0.0 {
        lootQuantityNormalEnemies + modDB.Sum(ModType::INC, None, nm!["LootQuantity"])
    } else {
        0.0
    };
    output.set(n!("LootQuantityNormalEnemies"), v);
    // L1511
    let lootRarityMagicEnemies = modDB.Sum(ModType::INC, None, nm!["LootRarityMagicEnemies"]);
    // L1512
    let v = if lootRarityMagicEnemies > 0.0 {
        lootRarityMagicEnemies + modDB.Sum(ModType::INC, None, nm!["LootRarity"])
    } else {
        0.0
    };
    output.set(n!("LootRarityMagicEnemies"), v);

    // L1514
    ctx.breakdown = None;
    // L1515
    if env.buildBreakdown.get() {
        // Initialise breakdown module
        // L1517
        let breakdown = newBreakdown(&modDB, &output, &env.player);
        ctx.breakdown = Some(breakdown.clone());
        // L1518
        env.player.breakdown.set(Some(breakdown));
        // L1519
        if let Some(minion) = env.minion.get() {
            // L1520
            let b = newBreakdown(&minion.modDB.get(), &minion.output.get(), &minion);
            minion.breakdown.set(Some(b));
        }
    }

    // L1524
    if modDB.Flag(None, nm!["ConvertArmourESToLife"]) {
        // L1525
        let mut energyShieldBase: f64;
        // L1526-1527 (`tempTable1` is a module-level scratch table, a local here: D17)
        let slotCfg = ModCfg::new();
        // L1528 (`pairs` over an array: index order)
        for slot in [
            "Helmet",
            "Gloves",
            "Boots",
            "Body Armour",
            "Weapon 2",
            "Weapon 3",
        ] {
            // L1529
            let armourData = env
                .player
                .itemList
                .with(|l| l.get(&Name::from(slot)).cloned())
                .and_then(|i| i.armourData.get());
            // L1530
            if let Some(armourData) = armourData {
                // L1531
                slotCfg.slotName.set(Some(Name::from(slot)));
                // L1532
                energyShieldBase = armourData.num_or(n!("EnergyShield"), 0.0);
                // L1533
                if energyShieldBase > 0.0 {
                    // L1534
                    modDB.NewMod(
                        n!("Life"),
                        ModType::BASE,
                        energyShieldBase,
                        &format!("{slot} ES to Life Conversion"),
                        0,
                        0,
                        &[],
                    );
                }
            }
        }
    }

    // L1540
    for element in ["Lightning", "Fire", "Cold", "Chaos", "Physical"] {
        // L1541
        if modDB.Flag(
            None,
            &[cat!(element, "DamageAppliesTo", element, "AuraEffect")],
        ) {
            // Damage to Aura Effect conversion from Breach rings
            // L1543 (`Sum` is a number, so `or 100` never applies)
            let multiplier = modDB.Sum(
                ModType::BASE,
                None,
                &[cat!(
                    "Improved",
                    element,
                    "DamageAppliesTo",
                    element,
                    "AuraEffect"
                )],
            ) / 100.0;
            // L1544
            let limit = modDB.Max(
                None,
                &[cat!(element, "DamageAppliesTo", element, "AuraEffectLimit")],
            );
            // L1545
            let mut totalConverted = 0.0;
            // L1546
            for value in modDB.Tabulate(
                Some(ModType::INC),
                Some(&ModCfg::new()),
                &[cat!(element, "Damage")],
            ) {
                // L1547
                let r#mod = &value.r#mod;
                // L1548
                let modifiers = calcLib::getConvertedModTags(r#mod, multiplier, true);
                // L1549
                let mut converted = m_floor(Val::from_value(&r#mod.value).num() * multiplier);
                // L1550
                if let Some(limit) = limit.filter(|_| converted > 0.0) {
                    // L1551
                    let remaining = limit - totalConverted;
                    // L1552
                    if remaining <= 0.0 {
                        // L1553
                        converted = 0.0;
                    } else {
                        // L1555
                        converted = m_min(converted, remaining);
                        // L1556
                        totalConverted = totalConverted + converted;
                    }
                }
                // L1559
                if converted != 0.0 {
                    // L1560
                    modDB.AddMod(Arc::new(mod_tools::createMod(
                        n!("AuraEffect"),
                        r#mod.r#type,
                        converted,
                        r#mod.source.as_deref(),
                        r#mod.flags,
                        KeywordFlag::by_name(element).expect("KeywordFlag[element]"),
                        &modifiers,
                    )));
                }
            }
        }
    }

    // L1566
    if modDB.Flag(None, nm!["MinionLifeAppliesToPlayer"]) {
        // Minion Life conversion from Rigwald's Hunt
        // L1568
        let multiplier = modDB
            .Max(None, nm!["ImprovedMinionLifeAppliesToPlayer"])
            .unwrap_or(100.0)
            / 100.0;
        // L1569
        for value in modDB.List(None, nm!["MinionModifier"]) {
            let value = Val::from_value(&value);
            // L1570
            let r#mod = match value.get(n!("mod")) {
                Val::Mod(m) => m,
                other => lua_error!(
                    "attempt to index field 'mod' (a {} value)",
                    other.lua_type()
                ),
            };
            if r#mod.name.as_str() == "Life" && r#mod.r#type == ModType::INC {
                // L1571
                // L1572
                let modifiers = calcLib::getConvertedModTags(&r#mod, multiplier, true);
                // L1573
                modDB.AddMod(Arc::new(mod_tools::createMod(
                    n!("Life"),
                    ModType::INC,
                    m_floor(Val::from_value(&r#mod.value).num() * multiplier),
                    r#mod.source.as_deref(),
                    r#mod.flags,
                    r#mod.keywordFlags,
                    &modifiers,
                )));
            }
        }
    }

    // Special handling of Mageblood
    // L1579
    let maxLeftActiveMagicUtilityCount =
        modDB.Sum(ModType::BASE, None, nm!["LeftActiveMagicUtilityFlasks"]);
    // L1580
    let maxRightActiveMagicUtilityCount =
        modDB.Sum(ModType::BASE, None, nm!["RightActiveMagicUtilityFlasks"]);
    // L1581
    if maxLeftActiveMagicUtilityCount > 0.0 || maxRightActiveMagicUtilityCount > 0.0 {
        // L1582
        let mut magicUtilityFlasks: Vec<ItemRef> = Vec::new();
        // L1583
        for_ipairs!((_, slot) in env.build.itemsTab.orderedSlots => {
            // L1584
            let item = match slot.get(n!("selItemId")) {
                Val::Num(id) if id >= 0.0 && id.fract() == 0.0 => env
                    .build
                    .itemsTab
                    .items
                    .with(|items| items.get(&(id as u32)).cloned()),
                _ => None,
            };
            // L1585-1586
            if let Some(item) = item {
                if item.r#type.get().as_str() == "Flask" && item.rarity.get().as_str() == "MAGIC" && {
                    let baseName = item_baseName(&item);
                    let b = baseName.as_str();
                    !(pob_lua::lmatch(b, "Life Flask", None).is_some()
                        || pob_lua::lmatch(b, "Mana Flask", None).is_some()
                        || pob_lua::lmatch(b, "Hybrid Flask", None).is_some())
                } {
                    // L1587
                    magicUtilityFlasks.push(item);
                }
            }
        });
        let n = magicUtilityFlasks.len() as f64;
        // L1590
        if maxLeftActiveMagicUtilityCount > 0.0 {
            // L1591
            let mut i = 1.0;
            let limit = m_min(maxLeftActiveMagicUtilityCount, n);
            while i <= limit {
                // L1592
                let item = magicUtilityFlasks[i as usize - 1].clone();
                env.flasks.get().with_mut(|f| f.set(&item, Some(true)));
                i += 1.0;
            }
        }
        // L1595
        if maxRightActiveMagicUtilityCount > 0.0 {
            // L1596
            let mut i = n;
            let limit = m_max(n - maxRightActiveMagicUtilityCount + 1.0, 1.0);
            while i >= limit {
                // L1597
                let item = magicUtilityFlasks
                    .get(i as usize - 1)
                    .cloned()
                    .unwrap_or_else(|| lua_error!("table index is nil"));
                env.flasks.get().with_mut(|f| f.set(&item, Some(true)));
                i -= 1.0;
            }
        }
    }

    // L1602
    let effectInc = modDB.Sum(ModType::INC, Some(&cfg_actor_player()), nm!["FlaskEffect"]);
    // L1603
    let effectIncMagic = modDB.Sum(
        ModType::INC,
        Some(&cfg_actor_player()),
        nm!["MagicUtilityFlaskEffect"],
    );
    // L1604
    let effectIncMagicNoAdjacent = modDB.Sum(
        ModType::INC,
        Some(&cfg_actor_player()),
        nm!["MagicFlaskNoAdjacentEffect"],
    );
    // L1605
    let effectIncNonPlayer = modDB.Sum(ModType::INC, None, nm!["FlaskEffect"]);
    // L1606
    let effectIncMagicNonPlayer = modDB.Sum(ModType::INC, None, nm!["MagicUtilityFlaskEffect"]);
    // L1607
    let flasksApplyToMinion = env.minion.get().is_some()
        && modDB.Flag(Some(&mainSkill.skillCfg.get()), nm!["FlasksApplyToMinion"]);
    // L1608
    let quickSilverAppliesToAllies = env.minion.get().is_some()
        && modDB.Flag(
            Some(&mainSkill.skillCfg.get()),
            nm!["QuickSilverAppliesToAllies"],
        );
    // L1609
    ctx.nonUniqueFlasksApplyToMinion = match env.minion.get() {
        Some(minion) => minion
            .modDB
            .get()
            .Flag(None, nm!["ParentNonUniqueFlasksAppliedToYou"]),
        None => false,
    };
    // L1610
    let flaskTotalRateInc = modDB.Sum(ModType::INC, None, nm!["FlaskRecoveryRate"]);
    // L1611
    let flaskDurInc = modDB.Sum(ModType::INC, None, nm!["FlaskDuration"]);

    // flask breakdown
    // L1614
    if ctx.breakdown.is_some() {
        // L1615
        let chargesGenerated = modDB.Sum(ModType::BASE, None, nm!["FlaskChargesGenerated"]);
        // L1616
        let mut usedFlasks = 0.0;
        // L1617
        for (_, v) in env.flasks.get().with(|f| f.pairs()) {
            // L1618
            if v {
                // L1619
                usedFlasks = usedFlasks + 1.0;
            }
        }

        // L1623
        let chargesGeneratedPerFlask = modDB.Sum(
            ModType::BASE,
            None,
            nm!["FlaskChargesGeneratedPerEmptyFlask"],
        ) * (5.0 - usedFlasks);
        // L1624
        let totalChargesGenerated = chargesGenerated + chargesGeneratedPerFlask;
        // L1625
        let utilityChargesGenerated =
            modDB.Sum(ModType::BASE, None, nm!["UtilityFlaskChargesGenerated"]);
        // L1626
        let lifeChargesGenerated = modDB.Sum(ModType::BASE, None, nm!["LifeFlaskChargesGenerated"]);
        // L1627
        let manaChargesGenerated = modDB.Sum(ModType::BASE, None, nm!["ManaFlaskChargesGenerated"]);

        // L1629
        output.set(n!("FlaskEffect"), effectInc);
        // L1630
        output.set(n!("FlaskChargeGen"), totalChargesGenerated);
        // L1631
        output.set(
            n!("LifeFlaskChargeGen"),
            totalChargesGenerated + lifeChargesGenerated,
        );
        // L1632
        output.set(
            n!("ManaFlaskChargeGen"),
            totalChargesGenerated + manaChargesGenerated,
        );
        // L1633
        output.set(
            n!("UtilityFlaskChargeGen"),
            totalChargesGenerated + utilityChargesGenerated,
        );
        // L1634
        output.set(
            n!("FlaskChargeOnCritChance"),
            m_min(
                100.0,
                modDB.Sum(ModType::BASE, None, nm!["FlaskChargeOnCritChance"]),
            ),
        );
    }

    // Merge flask modifiers
    // L1638-1650 getFlaskInstantRecovery, L1652-1689 calcFlaskRecovery: private fns below.
    // L1691 local function mergeFlasks: its upvalues (CCR-0078 §1)
    let flaskUp = Rc::new(FlaskUpvalues {
        effectInc,
        effectIncMagic,
        effectIncMagicNoAdjacent,
        effectIncNonPlayer,
        effectIncMagicNonPlayer,
        flasksApplyToMinion,
        quickSilverAppliesToAllies,
        flaskTotalRateInc,
        flaskDurInc,
    });
    record_upvalues(&env, |e| e.flasks = Some(flaskUp.clone()));

    // L1851
    let effectInc = modDB.Sum(
        ModType::INC,
        Some(&cfg_actor_player()),
        nm!["TinctureEffect"],
    );
    // L1852
    let effectIncMagic = modDB.Sum(
        ModType::INC,
        Some(&cfg_actor_player()),
        nm!["MagicTinctureEffect"],
    );
    // L1853
    let tinctureLimit = modDB.Sum(ModType::BASE, None, nm!["TinctureLimit"]);

    // tincture breakdown
    // L1856
    if ctx.breakdown.is_some() {
        // L1857
        output.set(n!("TinctureEffect"), effectInc);
        // L1858
        output.set(n!("TinctureLimit"), tinctureLimit);
    }

    // L1862 local function mergeTinctures: its upvalues (CCR-0078 §1)
    let tinctureUp = Rc::new(TinctureUpvalues {
        effectInc,
        effectIncMagic,
        tinctureLimit: Cell::new(tinctureLimit),
    });
    record_upvalues(&env, |e| e.tinctures = Some(tinctureUp.clone()));

    // L1934
    if env.mode_combat.get() {
        // This needs to be done in 2 steps to account for effects affecting life recovery from flasks
        // For example Sorrow of the Divine and buffs (like flask recovery watchers eye)
        // L1937
        let flasks = env.flasks.get().with(|f| f.clone());
        mergeFlasks_up(ctx, &flaskUp, &flasks, false, true);

        // Merge keystones again to catch any that were added by flasks
        // L1940
        merge_keystones(&env);
    }
    Flow::Normal
}

/// `minionCounts[key]` (CalcPerform.lua 1449-1457).
#[derive(Clone, Default)]
struct MinionCounts {
    total: Option<f64>,
    nonVaal: Option<f64>,
    permanent: Option<f64>,
}

/// `data.nonDamagingAilment[name].default` in arithmetic (nil raises).
fn ailment_default(name: Name) -> f64 {
    data()
        .nonDamagingAilment
        .get(&name)
        .and_then(|a| a.default)
        .unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on field 'default' (a nil value)")
        })
}

// ---- the segment's local functions ----------------------------------------------------------

/// CalcPerform.lua 1638-1650 `getFlaskInstantRecovery(item)` (local function; segment marker).
fn getFlaskInstantRecovery(modDB: &ModDB, item: &Item) -> f64 {
    let flaskData = item_flaskData(item);
    let flask = item_flask_base(item);
    // L1639
    let mut instantPerc = flaskData.num_or(n!("instantPerc"), 0.0);
    // L1640
    if modDB.Flag(None, nm!["Condition:LowLife"]) {
        // L1641
        instantPerc = instantPerc + flaskData.num_or(n!("instantLowLifePerc"), 0.0);
    }
    // L1643
    if flask.life.is_some() {
        // L1644
        instantPerc = instantPerc + modDB.Sum(ModType::BASE, None, nm!["LifeFlaskInstantRecovery"]);
    }
    // L1646
    if flask.mana.is_some() {
        // L1647
        instantPerc = instantPerc + modDB.Sum(ModType::BASE, None, nm!["ManaFlaskInstantRecovery"]);
    }
    // L1649
    m_min(100.0, instantPerc)
}

/// CalcPerform.lua 1652-1689 `calcFlaskRecovery(type, item)` (local function; segment marker).
fn calcFlaskRecovery(modDB: &ModDB, up: &FlaskUpvalues, r#type: &str, item: &Item) -> Vec<ModRef> {
    let flaskData = item_flaskData(item);
    // L1653
    let mut out: Vec<ModRef> = Vec::new();
    // L1654
    let lType = pob_lua::string::lower(r#type);

    // L1656
    if !flaskData.truthy(cat!(lType, "EffectNotRemoved"))
        && !modDB.Flag(None, &[cat!(r#type, "FlaskEffectNotRemoved")])
    {
        // L1657
        return out;
    }

    // L1660
    let name = item.name.get();
    // L1661
    let base = flaskData.num(cat!(lType, "Base"));
    // L1662
    let dur = flaskData.num(n!("duration"));
    // L1663
    let instPerc = getFlaskInstantRecovery(modDB, item);
    // L1664
    let flaskRecInc = modDB.Sum(ModType::INC, None, &[cat!("Flask", r#type, "Recovery")]);
    // L1665
    let flaskRecMore = modDB.More(None, &[cat!("Flask", r#type, "Recovery")]);
    // L1666
    let flaskRateInc = modDB.Sum(ModType::INC, None, &[cat!("Flask", r#type, "RecoveryRate")]);
    // L1667
    let mut flaskTotal = base
        * (1.0 - instPerc / 100.0)
        * (1.0 + flaskRecInc / 100.0)
        * flaskRecMore
        * (1.0 + up.flaskDurInc / 100.0);
    // L1668
    let flaskDur = dur * (1.0 + up.flaskDurInc / 100.0)
        / (1.0 + up.flaskTotalRateInc / 100.0)
        / (1.0 + flaskRateInc / 100.0);

    // More life recovery while on low life is not affected by flask effect (verified ingame).
    // Since this will be multiplied by the flask effect value below we have to counteract this by removing the flask effect from the value beforehand.
    // This is also the reason why this value needs a separate multiplier and cannot just be calculated into FlaskLifeRecovery.
    // L1673
    let lowLifeFlaskRecMore = modDB.More(None, nm!["FlaskLifeRecoveryLowLife"]);
    // L1674
    if lowLifeFlaskRecMore > 1.0 {
        // L1675
        flaskTotal = flaskTotal
            * if &*lType == "life" {
                ((lowLifeFlaskRecMore - 1.0) / (1.0 + (up.effectInc) / 100.0)) + 1.0
            } else {
                1.0
            };
    }

    // L1678
    out.push(Arc::new(mod_tools::createMod(
        cat!(r#type, "Recovery"),
        ModType::BASE,
        flaskTotal / flaskDur,
        Some(&name),
        0,
        0,
        &[],
    )));

    // L1680
    if modDB.Flag(None, &[cat!(r#type, "FlaskAppliesToEnergyShield")]) {
        // L1681
        out.push(Arc::new(mod_tools::createMod(
            n!("EnergyShieldRecovery"),
            ModType::BASE,
            flaskTotal / flaskDur,
            Some(&name),
            0,
            0,
            &[],
        )));
    }

    // L1684
    if modDB.Flag(None, &[cat!(r#type, "FlaskAppliesToLife")]) {
        // L1685
        out.push(Arc::new(mod_tools::createMod(
            n!("LifeRecovery"),
            ModType::BASE,
            flaskTotal / flaskDur,
            Some(&name),
            0,
            0,
            &[],
        )));
    }

    // L1688
    out
}

/// The locals of one `mergeFlasks` call (CalcPerform.lua 1692-1699), shared with its local
/// function `calcFlaskMods`.
struct FlaskMerge {
    flaskBuffs: Table,
    flaskConditions: Table,
    flaskConditionsNonUtility: Table,
    flaskBuffsPerBase: Table,
    flaskBuffsNonPlayer: Table,
    flaskBuffsPerBaseNonPlayer: Table,
    flaskBuffsNonUtility: Table,
}

/// CalcPerform.lua 1701-1771 `calcFlaskMods(item, baseName, buffModList, modList, onlyMinion)`
/// (local function of mergeFlasks; segment marker).
fn calcFlaskMods(
    ctx: &PerformCtx,
    up: &FlaskUpvalues,
    m: &FlaskMerge,
    onlyRecovery: bool,
    checkNonRecoveryFlasksForMinions: bool,
    item: &ItemRef,
    baseName: Name,
    buffModList: &[ModRef],
    modList: &[ModRef],
    onlyMinion: bool,
) {
    let env = &ctx.env;
    let flaskData = item_flaskData(item);
    let flask = item_flask_base(item);
    let rarity = item.rarity.get();
    let itemBaseName = item_baseName(item);
    // L1702
    let mut flaskEffectInc = up.effectInc + flaskData.num(n!("effectInc"));
    // L1703
    let mut flaskEffectIncNonPlayer = up.effectIncNonPlayer + flaskData.num(n!("effectInc"));
    // L1704
    if rarity.as_str() == "MAGIC" && !(flask.life.is_some() || flask.mana.is_some()) {
        // L1705
        flaskEffectInc = flaskEffectInc + up.effectIncMagic;
        // L1706
        flaskEffectIncNonPlayer = flaskEffectIncNonPlayer + up.effectIncMagicNonPlayer;
    }
    // Essence of Desolation belt mod: bonus for magic flasks with no flask in an adjacent slot (1-5)
    // L1709
    if rarity.as_str() == "MAGIC" && up.effectIncMagicNoAdjacent != 0.0 {
        // L1710 CCR-0030 #5: `env.flaskSlotMap` is keyed by items, which the frozen
        // `Field<Table>` cannot hold (no object keys); the read cannot be ported until then.
        let _ = env.flaskSlotMap.get();
        lua_error!(
            "CCR-0030 #5: env.flaskSlotMap[item] (CalcPerform.lua:1710) needs an item-keyed map"
        );
    }
    // L1718
    let effectMod = 1.0 + (flaskEffectInc) / 100.0;
    // L1719
    let effectModNonPlayer = 1.0 + (flaskEffectIncNonPlayer) / 100.0;

    // Life and Mana flasks are not Utility flasks, so their effects are kept when Utility flasks are disabled
    // L1722
    let isUtilityFlask = !(flask.life.is_some() || flask.mana.is_some());
    let notUnique = rarity.as_str() != "UNIQUE" && rarity.as_str() != "RELIC";
    let toMinion = (!onlyRecovery || checkNonRecoveryFlasksForMinions)
        && (up.flasksApplyToMinion
            || up.quickSilverAppliesToAllies
            || (ctx.nonUniqueFlasksApplyToMinion && notUnique));

    // Avert thine eyes, lest they be forever scarred
    // I have no idea how to determine which buff is applied by a given flask,
    // so utility flasks are grouped by base, unique flasks are grouped by name, and magic flasks by their modifiers
    // L1727
    if !buffModList.is_empty() {
        // L1728
        if !onlyMinion {
            // L1729
            let srcList = ModStore::ModList(None);
            // L1730
            srcList.ScaleAddList(buffModList, effectMod, false);
            let src = mods_table(&srcList);
            // L1731
            merge_buff(&src, &m.flaskBuffs, baseName);
            // L1732
            merge_buff(
                &src,
                &sub_table(&m.flaskBuffsPerBase, itemBaseName),
                baseName,
            );
            // L1733
            if !isUtilityFlask {
                // L1734
                merge_buff(&src, &m.flaskBuffsNonUtility, baseName);
            }
        }
        // L1737 (`srcList` at 1738 is an implicit global when `onlyMinion`: a local here, §7.3)
        if toMinion {
            // L1738
            let srcList = ModStore::ModList(None);
            // L1739
            srcList.ScaleAddList(buffModList, effectModNonPlayer, false);
            let src = mods_table(&srcList);
            // L1740
            merge_buff(&src, &m.flaskBuffsNonPlayer, baseName);
            // L1741
            merge_buff(
                &src,
                &sub_table(&m.flaskBuffsPerBaseNonPlayer, itemBaseName),
                baseName,
            );
        }
    }

    // L1745
    if !modList.is_empty() {
        // L1746
        let srcList = ModStore::ModList(None);
        // L1747
        srcList.ScaleAddList(modList, effectMod, false);
        // L1748
        let key: Name;
        // L1749
        if !notUnique {
            // L1750
            key = title_key(item);
        } else {
            // L1752
            let mut k = String::new();
            // L1753
            for r#mod in modList {
                // L1754
                k = k + &mod_tools::formatModParams(r#mod) + "&";
            }
            key = Name::from(k.as_str());
        }
        let src = mods_table(&srcList);
        // L1757
        if !onlyRecovery {
            // L1758
            merge_buff(&src, &m.flaskBuffs, key);
            // L1759
            merge_buff(&src, &sub_table(&m.flaskBuffsPerBase, itemBaseName), key);
            // L1760
            if !isUtilityFlask {
                // L1761
                merge_buff(&src, &m.flaskBuffsNonUtility, key);
            }
        }
        // L1764
        if toMinion {
            // L1765
            let srcList = ModStore::ModList(None);
            // L1766
            srcList.ScaleAddList(modList, effectModNonPlayer, false);
            let src = mods_table(&srcList);
            // L1767
            merge_buff(&src, &m.flaskBuffsNonPlayer, key);
            // L1768
            merge_buff(
                &src,
                &sub_table(&m.flaskBuffsPerBaseNonPlayer, itemBaseName),
                key,
            );
        }
    }
}

/// `mergeFlasks` with its upvalues (CalcPerform.lua 1691-1849; segment marker).
fn mergeFlasks_up(
    ctx: &PerformCtx,
    up: &FlaskUpvalues,
    flasks: &PtrMap<Item, bool>,
    onlyRecovery: bool,
    checkNonRecoveryFlasksForMinions: bool,
) {
    let env = &ctx.env;
    let modDB = &ctx.modDB;
    // L1692-1699
    let m = FlaskMerge {
        flaskBuffs: Table::new(),
        flaskConditions: Table::new(),
        flaskConditionsNonUtility: Table::new(),
        flaskBuffsPerBase: Table::new(),
        flaskBuffsNonPlayer: Table::new(),
        flaskBuffsPerBaseNonPlayer: Table::new(),
        flaskBuffsNonUtility: Table::new(),
    };

    // L1773 LUA-ORDER: item-keyed `pairs` (PoB: address order; here insertion order). It
    // decides which flask's buff wins a mergeBuff tie and the order of the merged mods.
    for (item, _) in flasks.pairs() {
        let baseName = item_baseName(&item);
        let flask = item_flask_base(&item);
        // L1774
        if !m.flaskBuffsPerBase.truthy(baseName) {
            m.flaskBuffsPerBase.set(baseName, Table::new());
        }
        // L1775
        if !m.flaskBuffsPerBaseNonPlayer.truthy(baseName) {
            m.flaskBuffsPerBaseNonPlayer.set(baseName, Table::new());
        }
        // L1776
        let instantPerc = getFlaskInstantRecovery(modDB, &item);
        // L1777
        if instantPerc < 100.0 {
            // L1778
            m.flaskConditions.set(n!("UsingFlask"), true);
            // L1779
            m.flaskConditions.set(using_name(baseName), true);
            // L1780
            if flask.life.is_some() || flask.mana.is_some() {
                // L1781
                m.flaskConditionsNonUtility.set(n!("UsingFlask"), true);
                // L1782
                m.flaskConditionsNonUtility.set(using_name(baseName), true);
            }
            // L1784
            if flask.life.is_some() && !modDB.Flag(None, nm!["CannotRecoverLifeOutsideLeech"]) {
                // L1785
                m.flaskConditions.set(n!("UsingLifeFlask"), true);
                // L1786
                m.flaskConditionsNonUtility.set(n!("UsingLifeFlask"), true);
            }
            // L1788
            if flask.mana.is_some() {
                // L1789
                m.flaskConditions.set(n!("UsingManaFlask"), true);
                // L1790
                m.flaskConditionsNonUtility.set(n!("UsingManaFlask"), true);
            }
        }
        // L1793
        if baseName.as_str() == "Iron Flask" {
            // L1794
            let chargesGeneratedOnWardBreak = modDB.Sum(
                ModType::BASE,
                None,
                nm!["IronFlaskChargesGeneratedOnWardBreak"],
            );
            // L1795
            if chargesGeneratedOnWardBreak > 0.0 {
                let flaskData = item_flaskData(&item);
                // L1796
                let gainMod = flaskData.num(n!("gainMod"))
                    * (1.0 + modDB.Sum(ModType::INC, None, nm!["FlaskChargesGained"]) / 100.0);
                // L1797
                let chargesUsed = flaskData.num(n!("chargesUsed"))
                    * (1.0 + modDB.Sum(ModType::INC, None, nm!["FlaskChargesUsed"]) / 100.0);
                // L1798
                if chargesGeneratedOnWardBreak * gainMod > chargesUsed {
                    // L1799
                    m.flaskConditions.set(n!("UnbrokenWard"), true);
                }
            }
        }

        // L1804
        if onlyRecovery {
            // L1805
            if flask.life.is_some() && !modDB.Flag(None, nm!["CannotRecoverLifeOutsideLeech"]) {
                // L1806
                let rec = calcFlaskRecovery(modDB, up, "Life", &item);
                calcFlaskMods(
                    ctx,
                    up,
                    &m,
                    onlyRecovery,
                    checkNonRecoveryFlasksForMinions,
                    &item,
                    n!("LifeFlask"),
                    &rec,
                    &[],
                    false,
                );
            }
            // L1808
            if flask.mana.is_some() {
                // L1809
                let rec = calcFlaskRecovery(modDB, up, "Mana", &item);
                calcFlaskMods(
                    ctx,
                    up,
                    &m,
                    onlyRecovery,
                    checkNonRecoveryFlasksForMinions,
                    &item,
                    n!("ManaFlask"),
                    &rec,
                    &[],
                    false,
                );
            }
            // L1811
            if checkNonRecoveryFlasksForMinions {
                // L1812
                let buffModList = item_list_mods(item.buffModList.get(), "buffModList");
                let modList = item_list_mods(item.modList.get(), "modList");
                calcFlaskMods(
                    ctx,
                    up,
                    &m,
                    onlyRecovery,
                    checkNonRecoveryFlasksForMinions,
                    &item,
                    baseName,
                    &buffModList,
                    &modList,
                    true,
                );
            }
        } else {
            // L1815
            let buffModList = item_list_mods(item.buffModList.get(), "buffModList");
            let modList = item_list_mods(item.modList.get(), "modList");
            calcFlaskMods(
                ctx,
                up,
                &m,
                onlyRecovery,
                checkNonRecoveryFlasksForMinions,
                &item,
                baseName,
                &buffModList,
                &modList,
                false,
            );
        }
    }
    // L1818
    if !modDB.Flag(None, nm!["FlasksDoNotApplyToPlayer"]) {
        // L1819
        let utilityFlasksDisabled = modDB.Flag(None, nm!["UtilityFlasksDoNotApplyToPlayer"]);
        // L1820
        let appliedFlaskConditions = if utilityFlasksDisabled {
            m.flaskConditionsNonUtility.clone()
        } else {
            m.flaskConditions.clone()
        };
        // L1821
        let appliedFlaskBuffs = if utilityFlasksDisabled {
            m.flaskBuffsNonUtility.clone()
        } else {
            m.flaskBuffs.clone()
        };
        // L1822
        for (flaskCond, status) in appliedFlaskConditions.pairs() {
            // L1823
            modDB.conditions.set(flaskCond, status);
        }
        // L1825 LUA-ORDER: string-keyed `pairs` (mod order within the store only)
        for (_, buffModList) in appliedFlaskBuffs.pairs() {
            // L1826
            modDB.AddList(&buff_mods(&buffModList));
        }
    }
    // L1829
    if let Some(minion) = env.minion.get() {
        // L1830
        if up.flasksApplyToMinion || ctx.nonUniqueFlasksApplyToMinion {
            // nonUniqueAlreadyFiltered
            // L1831
            let minionModDB = minion.modDB.get();
            // L1832
            for (flaskCond, status) in m.flaskConditions.pairs() {
                // L1833
                minionModDB.conditions.set(flaskCond, status);
            }
            // L1835 LUA-ORDER: string-keyed `pairs` (mod order within the store only)
            for (_, buffModList) in m.flaskBuffsNonPlayer.pairs() {
                // L1836
                minionModDB.AddList(&buff_mods(&buffModList));
            }
        } else {
            // Not all flasks apply to minions. Check if some flasks need to be selectively applied
            // L1839
            if up.quickSilverAppliesToAllies
                && m.flaskBuffsPerBaseNonPlayer.truthy(n!("Quicksilver Flask"))
            {
                // L1840
                let minionModDB = minion.modDB.get();
                // L1841
                minionModDB.conditions.set(
                    n!("UsingQuicksilverFlask"),
                    m.flaskConditions.get(n!("UsingQuicksilverFlask")),
                );
                // L1842
                minionModDB
                    .conditions
                    .set(n!("UsingFlask"), m.flaskConditions.get(n!("UsingFlask")));
                // L1843 LUA-ORDER: string-keyed `pairs` (mod order within the store only)
                let perBase = sub_table(&m.flaskBuffsPerBaseNonPlayer, n!("Quicksilver Flask"));
                for (_, buffModList) in perBase.pairs() {
                    // L1844
                    minionModDB.AddList(&buff_mods(&buffModList));
                }
            }
        }
    }
}

/// `mergeTinctures` with its upvalues (CalcPerform.lua 1862-1932; segment marker).
fn mergeTinctures_up(ctx: &PerformCtx, up: &TinctureUpvalues, tinctures: &PtrMap<Item, bool>) {
    let modDB = &ctx.modDB;
    let output = &ctx.output;
    // L1863
    let tincturesNotInflictManaBurn = m_min(
        modDB.Sum(ModType::BASE, None, nm!["TincturesNotInflictManaBurn"]),
        100.0,
    );
    // L1864
    let canGainRequiredBurn = modDB.Flag(None, nm!["Condition:WeepingWoundsInsteadOfManaBurn"])
        || (tincturesNotInflictManaBurn < 100.0 && output.num_or(n!("ManaUnreserved"), 0.0) > 0.0);
    // L1865
    if !canGainRequiredBurn {
        // L1866
        return;
    }
    // L1868
    let tinctureBuffs = Table::new();
    // L1869
    let tinctureConditions = Table::new();
    // L1870
    let tinctureBuffsPerBase = Table::new();

    // L1872-1903 local function calcTinctureMods(item, baseName, buffModList, modList)
    let calcTinctureMods =
        |item: &ItemRef, baseName: Name, buffModList: &[ModRef], modList: &[ModRef]| {
            let tinctureData = item.tinctureData.get().unwrap_or_else(|| {
                lua_error!("attempt to index field 'tinctureData' (a nil value)")
            });
            let rarity = item.rarity.get();
            let itemBaseName = item_baseName(item);
            // L1873
            let mut tinctureEffectInc = up.effectInc + tinctureData.num(n!("effectInc"));
            // L1874
            if rarity.as_str() == "MAGIC" {
                // L1875
                tinctureEffectInc = tinctureEffectInc + up.effectIncMagic;
            }
            // Compute tincture effect multiplier.
            // Tincture effect multiplier is rounded to 2 decimal places before applying it.
            // L1879
            let effectMod = m_floor(
                (1.0 + (tinctureEffectInc) / 100.0)
                    * (1.0 + item.quality.get().unwrap_or(0.0) / 100.0)
                    * 100.0,
            ) / 100.0;

            // same deal as flasks, go look at the comment there
            // L1882
            if !buffModList.is_empty() {
                // L1883
                let srcList = ModStore::ModList(None);
                // L1884
                srcList.ScaleAddList(buffModList, effectMod, false);
                let src = mods_table(&srcList);
                // L1885
                merge_buff(&src, &tinctureBuffs, baseName);
                // L1886
                merge_buff(
                    &src,
                    &sub_table(&tinctureBuffsPerBase, itemBaseName),
                    baseName,
                );
            }

            // L1889
            if !modList.is_empty() {
                // L1890
                let srcList = ModStore::ModList(None);
                // L1891
                srcList.ScaleAddList(modList, effectMod, false);
                // L1892
                let key: Name;
                // L1893
                if rarity.as_str() == "UNIQUE" || rarity.as_str() == "RELIC" {
                    // L1894
                    key = title_key(item);
                } else {
                    // L1896
                    let mut k = String::new();
                    // L1897
                    for r#mod in modList {
                        // L1898
                        k = k + &mod_tools::formatModParams(r#mod) + "&";
                    }
                    key = Name::from(k.as_str());
                }
                let src = mods_table(&srcList);
                // L1901
                merge_buff(&src, &tinctureBuffs, key);
                // L1902
                merge_buff(&src, &sub_table(&tinctureBuffsPerBase, itemBaseName), key);
            }
        };
    // L1905 LUA-ORDER: item-keyed `pairs` (PoB: address order; here insertion order). With a
    // tincture limit below the number of tinctures it decides which tinctures apply.
    for (item, _) in tinctures.pairs() {
        // L1906
        if up.tinctureLimit.get() <= 0.0 {
            // L1907
            break;
        }
        // L1909
        up.tinctureLimit.set(up.tinctureLimit.get() - 1.0);
        let baseName = item_baseName(&item);
        // L1910
        if !tinctureBuffsPerBase.truthy(baseName) {
            tinctureBuffsPerBase.set(baseName, Table::new());
        }
        // L1911
        tinctureConditions.set(n!("UsingTincture"), true);
        // L1912
        tinctureConditions.set(using_name(baseName), true);
        // L1913
        let buffModList = item_list_mods(item.buffModList.get(), "buffModList");
        let modList = item_list_mods(item.modList.get(), "modList");
        calcTinctureMods(&item, baseName, &buffModList, &modList);
    }
    // L1915
    for (tinctureCond, status) in tinctureConditions.pairs() {
        // L1916
        modDB.conditions.set(tinctureCond, status);
    }
    // L1918 LUA-ORDER: string-keyed `pairs` (mod order within the store only)
    for (_, buffModList) in tinctureBuffs.pairs() {
        // L1919
        modDB.AddList(&buff_mods(&buffModList));
    }
    // L1921
    if modDB.Flag(None, nm!["TinctureRangedWeapons"]) {
        // L1922 LUA-ORDER: string-keyed `pairs` (mod order within the store only)
        for (_key, buffModList) in tinctureBuffs.pairs() {
            // L1923
            for buff in buff_mods(&buffModList) {
                // L1924
                if (buff.flags & ModFlag::WeaponMelee) == ModFlag::WeaponMelee {
                    // L1925 (`newMod` is an implicit global, CCR-0004 note: a local here, §7.3;
                    // `copyTable(buff, true)` is shallow, so the copy shares the tags)
                    let mut newMod: Mod = (*buff).clone();
                    // L1926
                    newMod.flags = (newMod.flags & !ModFlag::WeaponMelee) | ModFlag::WeaponRanged;
                    // L1927
                    modDB.AddList(&[Arc::new(newMod)]);
                }
            }
        }
    }
}

// ---- the frozen entry points of the crossing local functions --------------------------------

// The local functions `mergeFlasks` (CalcPerform.lua:1691) and `mergeTinctures` (1862) are
// declared inside the segment and cross into PF-F (3121) / PF-D (2095) (§7.1); their
// provenance is part of the segment marker above.
pub(super) fn mergeFlasks(
    ctx: &PerformCtx,
    flasks: &PtrMap<Item, bool>,
    onlyRecovery: bool,
    checkNonRecoveryFlasksForMinions: bool,
) {
    let up = flask_upvalues(&ctx.env); // CCR-0078 §1
    mergeFlasks_up(
        ctx,
        &up,
        flasks,
        onlyRecovery,
        checkNonRecoveryFlasksForMinions,
    );
}

pub(super) fn mergeTinctures(ctx: &PerformCtx, tinctures: &PtrMap<Item, bool>) {
    let up = tincture_upvalues(&ctx.env); // CCR-0078 §1
    mergeTinctures_up(ctx, &up, tinctures);
}
