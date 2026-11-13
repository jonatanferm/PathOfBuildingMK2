//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! initEnv segment CS3 (CalcSetup.lua 1451-1975): skills, supports, minions; restarts at 1923; final merge.
//! The final `return env, cachedPlayerDB, …` (1974) is the orchestrator's `InitEnvCtx::finish`.
//!
//! Owner: CS3.
//!
//! The segment's locals that never cross a boundary stay Rust locals here. Those that Lua keys
//! by table identity (`markList[socketGroup]`, `supportLists[group]`,
//! `groupCfgList[slot][group]`, `socketGroupSkillListList[slot][group]`,
//! `processedSockets[gemInstance]`) are insertion-ordered `PtrMap`/`PtrSet`s (calc doc §3.4;
//! the `pairs` over them is marked `LUA-ORDER`). A `groupCfg` is both a ModStore cfg and the
//! carrier of `propertyModList` and the gem colour counts: [`GroupCfg`] keeps the cfg (a
//! `ModCfg` whose `extra.propertyModList` is the rows as a Lua list, so `copyTable(groupCfg,
//! true)` carries it into every `gemCfg` as in Lua) and the rows for `applyGemMods`.
//!
//! Contract gaps (CCR-0083, docs/port/ccr/0083-cs3-init-env-skills.md), each site marked:
//! * `grantedEffect.fromItem = true` (CalcSetup.lua:1637) writes into PoB's global `data`;
//!   the immutable Rust data cannot take it, so the write goes to a process-wide set
//!   ([`grantedEffect_fromItem`]) that readers must consult (CCR-0083 §1);
//! * a nil `socketGroup.mainActiveSkill` / `mainActiveSkillCalcs` (groups CS3 creates) is NaN,
//!   the CCR-0019 convention for nil numbers in typed model fields (CCR-0083 §3).
#![allow(non_snake_case)]
// Lua statement shapes are kept (locals assigned per branch, `if … then … else … end`).
#![allow(
    clippy::needless_late_init,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::float_cmp,
    clippy::len_zero,
    clippy::explicit_auto_deref,
    clippy::no_effect_underscore_binding
)]

use std::cell::RefCell;

use indexmap::IndexMap;
use pob_gamedata::mod_tools as modLib;
use pob_gamedata::GemData;

use super::support::{addBestSupport, applyGemMods, applySocketMods};
use super::InitEnvCtx;
use crate::model::skills_tab::socketGroup_label;
use crate::modules::calc_active_skill::{buildActiveSkillModList, createActiveSkill};
use crate::modules::calc_tools::{self as calcLib, GemLevelTarget};
use crate::prelude::*;

// ---- Rust-only plumbing ---------------------------------------------------------------------

thread_local! {
    /// CCR-0083 §1: granted effects CalcSetup.lua:1637 flagged `fromItem = true`. PoB writes the
    /// flag into its global `data.skills[...]` table, so it persists for the rest of the process
    /// (every later env sees it). Keyed by the granted effect's id.
    static GRANTED_EFFECT_FROM_ITEM: RefCell<ahash::AHashSet<Name>> = RefCell::new(ahash::AHashSet::new());
}

/// `grantedEffect.fromItem`: the data's own flag, or CalcSetup.lua:1637's runtime write
/// (CCR-0083 §1).
pub fn grantedEffect_fromItem(grantedEffect: &GrantedEffect) -> bool {
    grantedEffect.fromItem
        || GRANTED_EFFECT_FROM_ITEM.with(|s| s.borrow().contains(&grantedEffect.id))
}

/// Forget the CalcSetup.lua:1637 writes (a freshly booted PoB process; tests).
pub fn reset_grantedEffect_fromItem() {
    GRANTED_EFFECT_FROM_ITEM.with(|s| s.borrow_mut().clear());
}

/// A `groupCfg` table (`{ slotName = …, propertyModList = … }` plus the gem colour counts).
struct GroupCfg {
    /// The table as a ModStore cfg: `slotName`, `intelligenceGems`, … and
    /// `extra.propertyModList` (the rows as a Lua list).
    cfg: Cfg,
    /// `groupCfg.propertyModList` as the rows `applyGemMods` takes.
    propertyModList: Rc<Vec<TabRow>>,
}

/// The Tabulate rows as the Lua list `{ { value = …, mod = … }, … }` (CCR-0040 §6 / CCR-0074 §3).
fn rows_table(rows: &[TabRow]) -> Table {
    let t = Table::new();
    for r in rows {
        let row = Table::new();
        row.set(n!("value"), Val::from_value(&r.value));
        row.set(n!("mod"), Val::Mod(r.r#mod.clone()));
        t.push(row);
    }
    t
}

/// `{ slotName = slotName, propertyModList = env.modDB:Tabulate("LIST", {slotName = slotName}, "GemProperty") }`.
fn newGroupCfg(env: &EnvRef, slotName: Option<Name>) -> Rc<GroupCfg> {
    let q = ModCfg::default();
    q.slotName.set(slotName);
    let rows = env
        .modDB
        .get()
        .Tabulate(Some(ModType::LIST), Some(&q), nm!["GemProperty"]);
    let cfg = ModCfg::default();
    cfg.slotName.set(slotName);
    cfg.extra.set(n!("propertyModList"), rows_table(&rows));
    Rc::new(GroupCfg {
        cfg: Rc::new(cfg),
        propertyModList: Rc::new(rows),
    })
}

/// `t[slotName or "noSlot"][group]` of the slot-then-group keyed tables.
type BySlot<V> = IndexMap<Name, PtrMap<SocketGroup, V>>;

fn slot_key(slotName: Option<Name>) -> Name {
    slotName.unwrap_or(n!("noSlot"))
}

/// `supportLists`: string keys (`supportLists[slotName][group]`) and table keys
/// (`supportLists[group]`) of one Lua table.
#[derive(Default)]
struct SupportLists {
    bySlot: BySlot<Shared<Vec<SkillEffectRef>>>,
    byGroup: PtrMap<SocketGroup, Shared<Vec<SkillEffectRef>>>,
}

/// `isTreeSkill` and `slotName` of a socket group (CalcSetup.lua 1609-1610, 1737-1738, 1864-1865):
/// `group.slot and group.slot:gsub(" Swap", "") or (isTreeSkill and "Passive Tree") or nil`.
fn group_slotName(group: &SocketGroup) -> Option<Name> {
    let slot = group.slot.get();
    let isTreeSkill = slot.is_none()
        && group.source.get().is_some_and(|s| {
            pob_lua::pattern::find(&pob_lua::string::lower(&s), "tree", None, false).is_some()
        });
    match slot {
        Some(s) => Some(Name::from(
            pob_lua::pattern::gsub(s.as_str(), " Swap", "", None)
                .0
                .as_str(),
        )),
        None => isTreeSkill.then(|| n!("Passive Tree")),
    }
}

/// `t[k] or 1` of a model number whose nil is NaN (CCR-0019 / CCR-0083 §3).
fn or1(v: f64) -> f64 {
    if v.is_nan() {
        1.0
    } else {
        v
    }
}

/// `explodeSource.modSource or explodeSource.id` (an item, a passive node or a table).
fn explode_key(src: &Val) -> Val {
    if let Some(item) = src.obj::<Item>() {
        return lor(item.modSource.get().map_or(Val::Nil, Val::Str), || {
            item.id.get().map_or(Val::Nil, Val::Num)
        });
    }
    if let Some(node) = src.obj::<Node>() {
        // FUP2: `node.id` as Lua has it (the string id of a legion / tattoo node, CCR-0024 §1;
        // the typed `Node.id` is NaN there)
        return crate::classes::passive_tree::node_id_val(&node);
    }
    lor(src.get(n!("modSource")), || src.get(n!("id")))
}

/// A Lua table key from a value to store at (`gemsBySource[k] = gem`): strings and numbers; a
/// nil or NaN key raises.
fn val_key(v: &Val) -> Key {
    match v {
        Val::Str(s) => Key::S(Name::from(&**s)),
        Val::Num(x) => Key::from(*x),
        Val::Bool(b) => Key::B(*b),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!("gemsBySource: unsupported key type {}", other.lua_type()),
    }
}

/// The key `gemsBySource[k]` reads at (FUP2, FOLLOWUPS "FUP1" item 6): `None` for a nil or NaN
/// key, which reads nil (`lj_tab_get` finds nothing) where a store at it raises ([`val_key`]).
fn read_key(v: &Val) -> Option<Key> {
    match v {
        Val::Nil => None,
        Val::Num(x) => Key::try_num(*x),
        other => Some(val_key(other)),
    }
}

/// `gemInstance.matchesSocket and data.misc.MatchingSocketQualityBonus or 0`.
fn socketQuality(gemInstance: &GemInstance) -> f64 {
    if gemInstance.extra.truthy(n!("matchesSocket")) {
        data().misc.MatchingSocketQualityBonus
    } else {
        0.0
    }
}

/// `(groupCfg.<x>Gems or 0) + (gemInstance.gemData.tags.<tag> and 1 or 0)`.
fn count_gem(c: &Cell<Option<f64>>, gemData: &GemData, tag: Name) {
    let v = c.get().unwrap_or(0.0)
        + if gemData.tags.contains(&tag) {
            1.0
        } else {
            0.0
        };
    c.set(Some(v));
}

/// `playerItems[groupCfg.slotName] and playerItems[groupCfg.slotName].sockets and
/// playerItems[groupCfg.slotName].sockets[gemIndex]`.
fn socketedIn(env: &Env, slotName: Option<Name>, gemIndex: usize) -> Option<Table> {
    let item = slotName.and_then(|s| env.player.item_in(s))?;
    item.sockets.get().get(gemIndex)
}

/// `playerItems[groupCfg.slotName] and playerItems[groupCfg.slotName].name`.
fn slotItemName(env: &Env, slotName: Option<Name>) -> Option<LStr> {
    let item = slotName.and_then(|s| env.player.item_in(s))?;
    Some(item.name.get())
}

/// `gemCfg = copyTable(groupCfg, true)` with the socket fields (1701-1703 / 1772-1774).
fn gemCfg(groupCfg: &GroupCfg, socketedIn: Option<&Table>, gemIndex: usize) -> Cfg {
    let c = groupCfg.cfg.copyTable(true);
    c.socketColor.set(
        socketedIn
            .and_then(|s| s.str(n!("color")))
            .map(|s| Name::from(&*s)),
    );
    c.socketNum.set(Some(gemIndex as f64));
    c
}

/// The once-per-gem socket processing of 1706-1713 / 1777-1784.
fn processSocket(
    env: &EnvRef,
    processedSockets: &RefCell<PtrSet<GemInstance>>,
    gemInstance: &GemInstanceRef,
    gemData: &GemDataRef,
    groupCfg: &GroupCfg,
    gemIndex: usize,
) {
    // L1706 / L1777
    if !processedSockets.borrow().contains(gemInstance) {
        // L1707
        processedSockets.borrow_mut().insert(gemInstance);
        // L1708
        let slotName = groupCfg.cfg.slotName.get();
        let itemName = slotItemName(env, slotName);
        applySocketMods(
            env,
            gemData,
            &groupCfg.cfg,
            gemIndex as f64,
            itemName.as_deref(),
        );
        // Keep track of the gem count for each color socketed in this group
        // L1710
        count_gem(&groupCfg.cfg.intelligenceGems, gemData, n!("intelligence"));
        // L1711
        count_gem(&groupCfg.cfg.dexterityGems, gemData, n!("dexterity"));
        // L1712
        count_gem(&groupCfg.cfg.strengthGems, gemData, n!("strength"));
    }
}

/// `normalizedGrantedSkill` of `getNormalizedSkillLevel` (1456-1459).
struct NormalizedGrantedSkill {
    grantedEffect: Option<GrantedEffectRef>,
    level: Cell<f64>,
}

impl GemLevelTarget for NormalizedGrantedSkill {
    fn lua_grantedEffect(&self) -> Option<GrantedEffectRef> {
        self.grantedEffect.clone()
    }
    fn lua_gemData(&self) -> Option<GemDataRef> {
        None
    }
    fn lua_level(&self) -> f64 {
        self.level.get()
    }
    fn set_lua_level(&self, v: f64) {
        self.level.set(v);
    }
}

/// `local function getNormalizedSkillLevel(grantedSkill)` (CalcSetup.lua 1453-1462).
fn getNormalizedSkillLevel(grantedSkill: &Table) -> f64 {
    // Levels in socketGroup.gemList[1].level are normalized
    // grantedSkill.level is not causing group match miss which causes all things that rely on group order to fail
    // L1456
    let normalizedGrantedSkill = NormalizedGrantedSkill {
        grantedEffect: grantedSkill
            .str(n!("skillId"))
            .and_then(|id| data().skills.get(&Name::from(&*id)).cloned()),
        level: Cell::new(grantedSkill.opt_num(n!("level")).unwrap_or(f64::NAN)),
    };
    // L1460
    calcLib::validateGemLevel(&normalizedGrantedSkill);
    // L1461
    normalizedGrantedSkill.level.get()
}

/// A skill effect table literal: `grantedEffect`, `level`, `quality` plus the given fields.
fn newEffect(grantedEffect: GrantedEffectRef, level: f64, quality: f64) -> SkillEffectRef {
    SkillEffect::new(grantedEffect, level, quality)
}

/// `env.data.gems[env.data.gemForBaseName[name:lower()] or env.data.gemForBaseName[(name .. " Support"):lower()]]`.
fn gemDataForSupportName(name: &str) -> Option<GemDataRef> {
    let d = data();
    let id = d
        .gemForBaseName
        .get(pob_lua::string::lower(name).as_str())
        .or_else(|| {
            d.gemForBaseName
                .get(pob_lua::string::lower(&format!("{name} Support")).as_str())
        })?;
    d.gems.get(id).cloned()
}

/// `local function addExtraSupports(value, grantedEffect, level)` (CalcSetup.lua 1628-1651).
fn addExtraSupports(
    env: &EnvRef,
    targetListList: &[Shared<Vec<SkillEffectRef>>],
    value: Option<&Val>,
    grantedEffect: Option<GrantedEffectRef>,
    level: Option<f64>,
) {
    let d = env.data;
    // L1629
    let mut grantedEffect = match grantedEffect {
        Some(ge) => Some(ge),
        None => {
            let Some(value) = value else {
                lua_error!("attempt to index local 'value' (a nil value)")
            };
            match value.get(n!("skillId")) {
                Val::Str(id) => d.skills.get(&Name::from(&*id)).cloned(),
                _ => None,
            }
        }
    };
    // Some skill gems share the same name as support gems, e.g. Barrage.
    // Since a support gem is expected here, if the first lookup returns a skill, then
    // prepending "Support" to the skillId will find the support version of the gem.
    // L1633
    if let (Some(value), Some(ge)) = (value, grantedEffect.as_ref()) {
        if !ge.support {
            // L1634
            let id = match value.get(n!("skillId")) {
                Val::Str(s) => s,
                Val::Num(x) => LStr::from(tostring_num(x)),
                other => lua_error!(
                    "attempt to concatenate field 'skillId' (a {} value)",
                    other.lua_type()
                ),
            };
            grantedEffect = d.skills.get(&Name::from(&*format!("Support{id}"))).cloned();
        }
    }
    // L1636
    if let (Some(_), Some(ge)) = (value, grantedEffect.as_ref()) {
        // Only item ExtraSupport gems should be flagged as fromItem. Imbued gems do not pass this check
        // L1637 (a write into PoB's global data: CCR-0083 §1)
        GRANTED_EFFECT_FROM_ITEM.with(|s| s.borrow_mut().insert(ge.id));
    }
    // L1639
    if let Some(grantedEffect) = grantedEffect {
        // L1640
        for targetList in targetListList {
            // L1641
            let lvl = match level {
                Some(l) => l,
                None => value
                    .map(|v| v.get(n!("level")))
                    .and_then(|v| v.opt_num())
                    .unwrap_or(f64::NAN),
            };
            let e = newEffect(grantedEffect.clone(), lvl, 0.0);
            // L1643
            e.gemData
                .set(gemDataForSupportName(grantedEffect.name.as_str()));
            // L1645
            e.appliesToGrantedSkills
                .set(value.is_some_and(|v| v.get(n!("appliesToGrantedSkills")).truthy()));
            // L1647
            e.extra.set(n!("enabled"), true);
            addBestSupport(&e, targetList, env.mode);
        }
    }
}

/// The literal `{ grantedEffect, level, quality = actualQuality, globalQuality = 0, itemQuality = 0,
/// supportQuality = 0, socketQuality = …, srcInstance = gemInstance, gemData = gemInstance.gemData }`
/// (1681-1691 / 1758-1768).
fn gemEffect(grantedEffect: &GrantedEffectRef, gemInstance: &GemInstanceRef) -> SkillEffectRef {
    // L1680 / L1757
    let actualQuality = gemInstance.quality.get() + socketQuality(gemInstance);
    let e = newEffect(
        grantedEffect.clone(),
        gemInstance.level.get(),
        actualQuality,
    );
    e.globalQuality.set(Some(0.0));
    e.itemQuality.set(Some(0.0));
    e.supportQuality.set(Some(0.0));
    e.extra.set(n!("socketQuality"), socketQuality(gemInstance));
    e.srcInstance.set(Some(gemInstance.clone()));
    e.gemData.set(gemInstance.gemData.get());
    e
}

/// `gemInstance.reqOverride = effect.req`.
fn setReqOverride(gemInstance: &GemInstance, effect: &SkillEffect) {
    gemInstance.extra.set(
        n!("reqOverride"),
        effect.req.get().map_or(Val::Nil, Val::Num),
    );
}

/// `gemInstance.gemData and gemInstance.gemData.grantedEffect or gemInstance.grantedEffect`.
fn gemGrantedEffect(gemInstance: &GemInstance) -> Option<GrantedEffectRef> {
    gemInstance
        .gemData
        .get()
        .map(|g| g.grantedEffect.clone())
        .or_else(|| gemInstance.grantedEffect.get())
}

/// `gemInstance["enableGlobal"..index]`.
fn enableGlobal(gemInstance: &GemInstance, index: usize) -> bool {
    match index {
        1 => gemInstance.enableGlobal1.get(),
        2 => gemInstance.enableGlobal2.get(),
        i => gemInstance
            .extra
            .truthy(format!("enableGlobal{i}").as_str()),
    }
}

/// `imbuedSupportBySlot[slot]`: a granted effect (`Val::Obj`, SkillsTab.lua:334).
fn as_grantedEffect(v: &Val) -> Option<GrantedEffectRef> {
    v.obj::<GrantedEffectRef>().map(|g| (*g).clone())
}

/// `tableConcat(t1, t2)` (Common.lua 610-619).
fn tableConcat(t1: &Table, t2: &Table) -> Table {
    let t3 = Table::new();
    for (_, v) in t1.ipairs() {
        t3.push(v);
    }
    for (_, v) in t2.ipairs() {
        t3.push(v);
    }
    t3
}

/// `copyTable(env.data.unarmedWeaponData[env.classId])`.
fn unarmedWeaponData(env: &Env) -> Table {
    let classId = env.classId.get();
    let Some(w) = (classId.fract() == 0.0)
        .then(|| env.data.unarmedWeaponData.get(&(classId as i64)))
        .flatten()
    else {
        lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
    };
    let t = Table::new();
    t.set(n!("type"), w.r#type);
    t.set(n!("AttackRate"), w.AttackRate);
    t.set(n!("CritChance"), w.CritChance);
    t.set(n!("PhysicalMin"), w.PhysicalMin);
    t.set(n!("PhysicalMax"), w.PhysicalMax);
    t
}

/// `item.weaponData and item.weaponData[i]` (a table, or nil).
fn itemWeaponData(item: &Item, i: i64) -> Option<Table> {
    item.weaponData.get().and_then(|w| w.table(i))
}

// ---- the segment ------------------------------------------------------------------------------

/// @pob Modules/CalcSetup.lua:1451-1973 h=952365eb
/// CS3: CalcSetup.lua 1451-1973 (`Flow::Restart` at 1923).
pub fn seg(ctx: &mut InitEnvCtx) -> Flow {
    let build = ctx.build.clone();
    let accelerate = ctx.accelerate;
    let env = match &ctx.env {
        Some(e) => e.clone(),
        None => lua_error!("attempt to index upvalue 'env' (a nil value)"),
    };
    let modDB = match &ctx.modDB {
        Some(m) => m.clone(),
        None => lua_error!("attempt to index local 'modDB' (a nil value)"),
    };
    let d = env.data;
    let skillsTab = &build.skillsTab;

    // L1451
    if !accelerate.skills {
        // L1452
        if env.mode == Mode::MAIN {
            // L1453-1462 getNormalizedSkillLevel (above)

            // Process extra skills granted by items or tree nodes
            // L1465 (`wipeTable(tempTable1)`: a module-level scratch set, empty here)
            let mut markList: PtrSet<SocketGroup> = PtrSet::new();
            // L1466
            for_ipairs!((_i, grantedSkill) in env.grantedSkills.get() => {
                // Check if a matching group already exists
                // L1468
                let mut group: Option<SocketGroupRef> = None;
                // L1469 (`pairs` over the list: its array part, index order)
                for socketGroup in skillsTab.socketGroupList.get().snapshot() {
                    // L1470
                    let gsSource = grantedSkill.str(n!("source"));
                    let gsSlot = grantedSkill.str(n!("slotName"));
                    if socketGroup.source.get().as_deref() == gsSource.as_deref()
                        && socketGroup.slot.get().as_ref().map(Name::as_str) == gsSlot.as_deref()
                    {
                        // L1471
                        if let Some(first) = socketGroup.gemList.get().get(1) {
                            let gsSkillId = grantedSkill.str(n!("skillId"));
                            if first.skillId.get().as_ref().map(Name::as_str) == gsSkillId.as_deref()
                                && (Val::Num(first.level.get()).raw_eq(&grantedSkill.get(n!("level")))
                                    || first.level.get() == getNormalizedSkillLevel(&grantedSkill))
                            {
                                // L1472
                                group = Some(socketGroup.clone());
                                // L1473
                                markList.insert(&socketGroup);
                                // L1474
                                break;
                            }
                        }
                    }
                }
                // L1478
                let group = match group {
                    Some(g) => g,
                    None => {
                        // Create a new group for this skill
                        // L1480
                        let g: SocketGroupRef = Rc::new(SocketGroup::default());
                        g.label.set(LStr::from(""));
                        g.enabled.set(true);
                        g.gemList.set(Shared::new(Vec::new()));
                        g.source.set(grantedSkill.str(n!("source")));
                        g.slot.set(grantedSkill.str(n!("slotName")).map(|s| Name::from(&*s)));
                        // nil fields of the literal (CCR-0019 / CCR-0083 §3)
                        g.extra.set(n!("$includeInFullDPSNil"), true);
                        g.mainActiveSkill.set(f64::NAN);
                        g.mainActiveSkillCalcs.set(f64::NAN);
                        // L1481
                        skillsTab.socketGroupList.get().push(g.clone());
                        // L1482
                        markList.insert(&g);
                        g
                    }
                };

                // Update the group
                // L1486
                group.sourceItem.set(grantedSkill.obj::<Item>(n!("sourceItem")));
                // L1487
                group.extra.set(n!("sourceNode"), grantedSkill.get(n!("sourceNode")));
                // L1488
                let activeGemInstance: GemInstanceRef = match group.gemList.get().get(1) {
                    Some(g) => g,
                    None => {
                        let g: GemInstanceRef = Rc::new(GemInstance::default());
                        g.skillId.set(grantedSkill.str(n!("skillId")).map(|s| Name::from(&*s)));
                        // `nameSpec = grantedSkill.nameSpec` (nil is "" in the typed field)
                        g.nameSpec.set(grantedSkill.str(n!("nameSpec")).unwrap_or_else(|| LStr::from("")));
                        g.quality.set(0.0);
                        g.enabled.set(true);
                        // CCR-0145 §4: the literal's nil enableGlobal1/2 (L1497 sets the first)
                        g.extra.set(n!("$enableGlobal1Nil"), true);
                        g.extra.set(n!("$enableGlobal2Nil"), true);
                        g
                    }
                };
                // L1494
                activeGemInstance.fromItem.set(!grantedSkill.get(n!("sourceItem")).is_nil());
                // L1495
                activeGemInstance.gemId.set(None);
                // L1496
                activeGemInstance.level.set(grantedSkill.opt_num(n!("level")).unwrap_or(f64::NAN));
                // L1497
                activeGemInstance.enableGlobal1.set(true);
                activeGemInstance.extra.set(n!("$enableGlobal1Nil"), Val::Nil); // CCR-0145 §4
                // L1498
                activeGemInstance.noSupports.set(grantedSkill.truthy(n!("noSupports")));
                // L1499
                group.noSupports.set(grantedSkill.truthy(n!("noSupports")));
                // L1500
                activeGemInstance.triggered.set(grantedSkill.truthy(n!("triggered")));
                // L1501
                activeGemInstance.extra.set(n!("triggerChance"), grantedSkill.get(n!("triggerChance")));
                // L1502
                group.gemList.get().wipe();
                // L1503
                group.gemList.get().push(activeGemInstance);
                // L1504
                skillsTab.ProcessSocketGroup(&build, &group);
            });

            // L1507
            let explodeSources = env.explodeSources.get();
            if explodeSources.len() != 0 {
                // Check if a matching group already exists
                // L1509
                let mut group: Option<SocketGroupRef> = None;
                // L1510 (`pairs` over the list: index order)
                for socketGroup in skillsTab.socketGroupList.get().snapshot() {
                    // L1511
                    if socketGroup.source.get().as_deref() == Some("Explode") {
                        // L1512
                        group = Some(socketGroup);
                        // L1513
                        break;
                    }
                }
                // L1516
                let group = match group {
                    Some(g) => g,
                    None => {
                        // Create a new group for this skill
                        // L1518
                        let g: SocketGroupRef = Rc::new(SocketGroup::default());
                        g.label.set(LStr::from("On Kill Monster Explosion"));
                        g.enabled.set(true);
                        g.gemList.set(Shared::new(Vec::new()));
                        g.source.set(Some(LStr::from("Explode")));
                        g.noSupports.set(true);
                        g.extra.set(n!("$includeInFullDPSNil"), true);
                        g.mainActiveSkill.set(f64::NAN);
                        g.mainActiveSkillCalcs.set(f64::NAN);
                        // L1519
                        skillsTab.socketGroupList.get().push(g.clone());
                        g
                    }
                };
                // Update the group
                // L1522
                group.explodeSources.set(Some(explodeSources.clone()));
                // L1523
                let gemsBySource = Table::new();
                // L1524
                for_ipairs!((_i, gem) in group.gemList.get() => {
                    // L1525
                    let src = gem.extra.get(n!("explodeSource"));
                    if src.truthy() {
                        // L1526
                        gemsBySource.set(val_key(&explode_key(&src)), Val::Obj(gem.clone()));
                    }
                });
                // L1529
                group.gemList.get().wipe();
                // L1530
                for explodeSource in explodeSources.snapshot() {
                    let explodeSource = explodeSource.to_val();
                    // L1531
                    let activeGemInstance: GemInstanceRef;
                    // L1532
                    // (a nil or NaN key reads nil)
                    let key = explode_key(&explodeSource);
                    let found = read_key(&key).and_then(|k| gemsBySource.obj::<GemInstance>(k));
                    if let Some(g) = found {
                        // L1533
                        activeGemInstance = g;
                    } else {
                        // L1535
                        let g: GemInstanceRef = Rc::new(GemInstance::default());
                        g.skillId.set(Some(n!("EnemyExplode")));
                        g.quality.set(0.0);
                        g.enabled.set(true);
                        g.level.set(1.0);
                        g.triggered.set(true);
                        g.extra.set(n!("explodeSource"), explodeSource.clone());
                        // CCR-0145 §4: the literal has no enableGlobal1/2 (nil)
                        g.extra.set(n!("$enableGlobal1Nil"), true);
                        g.extra.set(n!("$enableGlobal2Nil"), true);
                        activeGemInstance = g;
                    }
                    // L1544
                    group.gemList.get().push(activeGemInstance);
                }
                // L1546
                markList.insert(&group);
                // L1547
                skillsTab.ProcessSocketGroup(&build, &group);
            }

            // Remove any socket groups that no longer have a matching item
            // L1551
            let mut i = 1usize;
            // L1552
            while let Some(socketGroup) = skillsTab.socketGroupList.get().get(i) {
                // L1554
                if socketGroup.source.get().is_some() && !markList.contains(&socketGroup) {
                    // L1555
                    skillsTab.socketGroupList.get().remove(Some(i));
                    // L1556
                    if skillsTab
                        .displayGroup
                        .get()
                        .is_some_and(|g| Rc::ptr_eq(&g, &socketGroup))
                    {
                        // L1557
                        skillsTab.displayGroup.set(None);
                    }
                } else {
                    // L1560
                    i += 1;
                }
            }
        }

        // Get the weapon data tables for the equipped weapons
        // L1566
        let weapon1 = env
            .player
            .itemList()
            .with(|m| m.get(&n!("Weapon 1")).cloned());
        let weapon2 = env
            .player
            .itemList()
            .with(|m| m.get(&n!("Weapon 2")).cloned());
        let gloves = env
            .player
            .itemList()
            .with(|m| m.get(&n!("Gloves")).cloned());
        let wd1 = match weapon1.as_ref().and_then(|w| itemWeaponData(w, 1)) {
            Some(t) => t,
            None => unarmedWeaponData(&env),
        };
        env.player.weaponData1.set(wd1);
        // L1567
        if env
            .player
            .weaponData1
            .get()
            .truthy(n!("countsAsDualWielding"))
        {
            // L1568 (nil would raise on the next read; the typed field holds a table)
            let w2 = match weapon1.as_ref() {
                Some(w) => itemWeaponData(w, 2).unwrap_or_default(),
                None => lua_error!("attempt to index field 'Weapon 1' (a nil value)"),
            };
            env.player.weaponData2.set(w2);
        } else if weapon2.is_none() {
            // Hollow Palm Technique
            // L1571 (`env.modDB.mods.Keystone`: the name's bucket exists)
            if weapon1.is_none()
                && gloves.is_none()
                && env.modDB.get().mod_names().contains(&n!("Keystone"))
            {
                // L1572
                for keystone in env.modDB.get().mods_named(n!("Keystone")) {
                    // L1573
                    if matches!(&keystone.value, Value::Str(s) if &**s == "Hollow Palm Technique") {
                        // L1574
                        env.player.weaponData2.set(unarmedWeaponData(&env));
                        // L1575
                        break;
                    }
                }
            }
            // L1579 (`weaponData2 or { }`: the typed field always holds a table; a fresh actor's
            // is the empty table)
        } else {
            // L1581
            let w2 = weapon2
                .as_ref()
                .and_then(|w| itemWeaponData(w, 2))
                .unwrap_or_default();
            env.player.weaponData2.set(w2);
        }

        // Determine main skill group
        // L1585
        if env.mode == Mode::CALCS {
            // L1586
            let n = skillsTab.socketGroupList.get().len() as f64;
            let v = m_min(
                m_max(n, 1.0),
                env.calcsInput.opt_num(n!("skill_number")).unwrap_or(1.0),
            );
            env.calcsInput.set(n!("skill_number"), v);
            // L1587
            env.mainSocketGroup
                .set(env.calcsInput.opt_num(n!("skill_number")));
        } else {
            // L1589
            let n = skillsTab.socketGroupList.get().len() as f64;
            build
                .mainSocketGroup
                .set(m_min(m_max(n, 1.0), or1(build.mainSocketGroup.get())));
            // L1590
            env.mainSocketGroup.set(Some(build.mainSocketGroup.get()));
        }
        let mainSocketGroup = env.mainSocketGroup.get();
        let isMain = |index: usize| mainSocketGroup == Some(index as f64);

        // Process supports and put them into the correct buckets
        // L1594
        env.crossLinkedSupportGroups.set(Table::new());
        // L1595
        for r#mod in env
            .modDB
            .get()
            .Tabulate(Some(ModType::LIST), None, nm!["LinkedSupport"])
        {
            let cl = env.crossLinkedSupportGroups.get();
            let sourceSlot = match r#mod.r#mod.extra_get(n!("sourceSlot")) {
                Some(Value::Str(s)) => Name::from(&**s),
                Some(Value::Num(x)) => Name::from(tostring_num(*x).as_str()),
                _ => lua_error!("table index is nil"),
            };
            // L1596
            let list = match cl.table(sourceSlot) {
                Some(t) => t,
                None => {
                    let t = Table::new();
                    cl.set(sourceSlot, t.clone());
                    t
                }
            };
            // L1597
            list.push(Val::from_value(&r#mod.value).get(n!("targetSlotName")));
        }

        // L1600
        let mut supportLists = SupportLists::default();
        // L1601
        let mut groupCfgList: BySlot<Rc<GroupCfg>> = IndexMap::new();
        // L1602
        let processedSockets: RefCell<PtrSet<GemInstance>> = RefCell::new(PtrSet::new());
        // Process support gems adding them to applicable support lists
        // L1604
        for_ipairs!((index, group) in skillsTab.socketGroupList.get() => {
            // L1605
            let slot = group
                .slot
                .get()
                .and_then(|s| build.itemsTab.slots.with(|m| m.get(&s).cloned()));
            // L1606
            let weaponSet = slot.as_ref().map(|s| s.get(n!("weaponSet")));
            group.slotEnabled.set(match &weaponSet {
                None => true,
                Some(ws) if !ws.truthy() => true,
                Some(ws) => {
                    // (`activeItemSet` is only read when the slot has a weapon set)
                    let useSecond = build
                        .itemsTab
                        .activeItemSet
                        .get()
                        .unwrap_or_else(|| lua_error!("attempt to index field 'activeItemSet' (a nil value)"))
                        .truthy(n!("useSecondWeaponSet"));
                    ws.raw_eq(&Val::Num(if useSecond { 2.0 } else { 1.0 }))
                }
            });
            // if group is main skill or group is enabled
            // L1608
            if isMain(index) || (group.enabled.get() && group.slotEnabled.get()) {
                // L1609-1610
                let slotName = group_slotName(&group);
                // L1611-1615
                let groupCfg = groupCfgList
                    .entry(slot_key(slotName))
                    .or_default()
                    .get(&group)
                    .unwrap_or_else(|| {
                        newGroupCfg(&env, slotName)
                    });
                groupCfgList
                    .entry(slot_key(slotName))
                    .or_default()
                    .set(&group, Some(groupCfg.clone()));
                // L1616-1617
                let propertyModList = groupCfg.propertyModList.clone();
                // L1618
                let mut targetListList: Vec<Shared<Vec<SkillEffectRef>>> = Vec::new();
                // L1619
                if let Some(cfgSlot) = groupCfg.cfg.slotName.get() {
                    // L1620-1621
                    let bySlot = supportLists.bySlot.entry(cfgSlot).or_default();
                    let list = match bySlot.get(&group) {
                        Some(l) => l,
                        None => {
                            let l = Shared::new(Vec::new());
                            bySlot.set(&group, Some(l.clone()));
                            l
                        }
                    };
                    // L1622
                    targetListList.push(list);
                } else {
                    // L1624
                    let list = match supportLists.byGroup.get(&group) {
                        Some(l) => l,
                        None => {
                            let l = Shared::new(Vec::new());
                            supportLists.byGroup.set(&group, Some(l.clone()));
                            l
                        }
                    };
                    // L1625
                    targetListList.push(list);
                }

                // L1628-1651 addExtraSupports (above)

                // Add extra supports from the item this group is socketed in
                // L1654
                for value in env.modDB.get().List(Some(&*groupCfg.cfg), nm!["ExtraSupport"]) {
                    let value = Val::from_value(&value);
                    // L1655
                    if group.source.get().is_none() || value.get(n!("appliesToGrantedSkills")).truthy() {
                        // L1656
                        addExtraSupports(&env, &targetListList, Some(&value), None, None);
                    }
                }
                // if the slot has an imbued support, add it as an ExtraSupport
                // L1660
                let imbued = slotName
                    .map(|s| skillsTab.imbuedSupportBySlot.get(s))
                    .unwrap_or(Val::Nil);
                if imbued.truthy() && group.extra.truthy(n!("imbuedSupport")) {
                    // L1661
                    let imbuedSupport = as_grantedEffect(&imbued).unwrap_or_else(|| {
                        lua_error!("imbuedSupportBySlot: not a granted effect ({})", imbued.lua_type())
                    });
                    // L1662
                    addExtraSupports(&env, &targetListList, None, Some(imbuedSupport.clone()), Some(1.0));
                    // L1663
                    let imbuedGemData = d
                        .gemForSkill
                        .get(&imbuedSupport.id)
                        .and_then(|id| d.gems.get(id))
                        .cloned();
                    // L1664
                    if let Some(sec) = imbuedGemData
                        .as_ref()
                        .and_then(|g| g.secondaryGrantedEffect.clone())
                        .filter(|s| s.support)
                    {
                        // L1665
                        addExtraSupports(&env, &targetListList, None, Some(sec), Some(1.0));
                    }
                }

                // L1669
                for_ipairs!((gemIndex, gemInstance) in group.gemList.get() => {
                    // Add support gems from this group
                    // L1671
                    if env.mode == Mode::MAIN {
                        // L1672
                        gemInstance.displayEffect.set(None);
                        // L1673
                        gemInstance.supportEffect.set(None);
                    }
                    // L1675
                    if gemInstance.enabled.get() {
                        // L1676 local function processGrantedEffect(grantedEffect)
                        let processGrantedEffect = |grantedEffect: Option<GrantedEffectRef>| {
                            // L1677
                            let Some(grantedEffect) = grantedEffect.filter(|g| g.support) else {
                                // L1678
                                return;
                            };
                            // L1680-1693
                            let supportEffect = gemEffect(&grantedEffect, &gemInstance);
                            supportEffect.superseded.set(false);
                            supportEffect.isSupporting.set(Some(Shared::new(PtrSet::new())));
                            // L1694
                            if env.mode == Mode::MAIN {
                                // L1695
                                gemInstance.displayEffect.set(Some(supportEffect.clone()));
                                // L1696
                                gemInstance.supportEffect.set(Some(supportEffect.clone()));
                            }
                            // L1698
                            if let Some(gemData) = gemInstance.gemData.get() {
                                // L1699-1700
                                let socketedIn = socketedIn(&env, groupCfg.cfg.slotName.get(), gemIndex);
                                // L1701-1703
                                let gc = gemCfg(&groupCfg, socketedIn.as_ref(), gemIndex);
                                supportEffect.gemCfg.set(Some(gc.clone()));
                                // L1704
                                let rows = if socketedIn.is_some() {
                                    env.modDB.get().Tabulate(Some(ModType::LIST), Some(&*gc), nm!["GemProperty"])
                                } else {
                                    (*propertyModList).clone()
                                };
                                applyGemMods(&supportEffect, &rows);
                                // L1705
                                setReqOverride(&gemInstance, &supportEffect);
                                // L1706-1713
                                processSocket(&env, &processedSockets, &gemInstance, &gemData, &groupCfg, gemIndex);
                            }
                            // Validate support gem level in case there is no active skill (and no full calculation)
                            // L1716
                            calcLib::validateGemLevel(&*supportEffect);

                            // L1718
                            for targetList in &targetListList {
                                // L1719
                                addBestSupport(&supportEffect, targetList, env.mode);
                            }
                        };
                        // L1722
                        if let Some(gemData) = gemInstance.gemData.get() {
                            // L1723
                            processGrantedEffect(Some(gemData.grantedEffect.clone()));
                            // L1724
                            processGrantedEffect(gemData.secondaryGrantedEffect.clone());
                        } else {
                            // L1726
                            processGrantedEffect(gemInstance.grantedEffect.get());
                        }
                    }
                });
            }
        });

        // Process active skills adding the applicable supports
        // L1734
        let mut socketGroupSkillListList: BySlot<Shared<Vec<ActiveSkillRef>>> = IndexMap::new();
        // L1735
        for_ipairs!((index, group) in skillsTab.socketGroupList.get() => {
            // L1736
            if isMain(index) || (group.enabled.get() && group.slotEnabled.get()) {
                // L1737-1738
                let slotName = group_slotName(&group);
                // L1739-1742 (`groupCfgList[slotName or "noSlot"]` exists: 1611 ran for this group)
                let Some(byGroup) = groupCfgList.get_mut(&slot_key(slotName)) else {
                    lua_error!("attempt to index field '?' (a nil value)")
                };
                let groupCfg = match byGroup.get(&group) {
                    Some(c) => c,
                    None => {
                        let c = newGroupCfg(&env, slotName);
                        byGroup.set(&group, Some(c.clone()));
                        c
                    }
                };
                // L1744
                let propertyModList = groupCfg.propertyModList.clone();
                // L1745-1747
                let socketGroupSkillList = {
                    let m = socketGroupSkillListList.entry(slot_key(slotName)).or_default();
                    match m.get(&group) {
                        Some(l) => l,
                        None => {
                            let l = Shared::new(Vec::new());
                            m.set(&group, Some(l.clone()));
                            l
                        }
                    }
                };
                // L1748
                let mut slotHasActiveSkill = false;

                // Create active skills
                // L1751
                for_ipairs!((gemIndex, gemInstance) in group.gemList.get() => {
                    // L1752
                    if gemInstance.enabled.get() && (gemInstance.gemData.get().is_some() || gemInstance.grantedEffect.get().is_some()) {
                        // L1753
                        let grantedEffectList: Vec<GrantedEffectRef> = match gemInstance.gemData.get() {
                            Some(g) => g.grantedEffectList.clone(),
                            None => gemInstance.grantedEffect.get().into_iter().collect(),
                        };
                        // L1754
                        for_ipairs!((index, grantedEffect) in grantedEffectList => {
                            // L1755 (`grantedEffect.unsupported`: no granted effect has the key)
                            if !grantedEffect.support && (!grantedEffect.hasGlobalEffect || enableGlobal(&gemInstance, index)) {
                                // L1756
                                slotHasActiveSkill = true;
                                // L1757-1768
                                let activeEffect = gemEffect(&grantedEffect, &gemInstance);
                                // L1769
                                if let Some(gemData) = gemInstance.gemData.get() {
                                    // L1770-1771
                                    let socketedIn = socketedIn(&env, groupCfg.cfg.slotName.get(), gemIndex);
                                    // L1772-1774
                                    let gc = gemCfg(&groupCfg, socketedIn.as_ref(), gemIndex);
                                    activeEffect.gemCfg.set(Some(gc.clone()));
                                    // L1775
                                    let rows = if socketedIn.is_some() {
                                        env.modDB.get().Tabulate(Some(ModType::LIST), Some(&*gc), nm!["GemProperty"])
                                    } else {
                                        (*propertyModList).clone()
                                    };
                                    applyGemMods(&activeEffect, &rows);
                                    // L1776
                                    setReqOverride(&gemInstance, &activeEffect);
                                    // L1777-1784
                                    processSocket(&env, &processedSockets, &gemInstance, &gemData, &groupCfg, gemIndex);
                                }
                                // L1786
                                if env.mode == Mode::MAIN {
                                    // L1787
                                    gemInstance.displayEffect.set(Some(activeEffect.clone()));
                                }
                                // L1789
                                let mut appliedSupportList: Shared<Vec<SkillEffectRef>> = Shared::new(Vec::new());
                                // L1790
                                if !group.noSupports.get() {
                                    // L1791
                                    let src = match supportLists.byGroup.get(&group) {
                                        Some(l) => l,
                                        None => match slotName.and_then(|s| supportLists.bySlot.get(&s)) {
                                            Some(m) => m.get(&group).unwrap_or_else(|| {
                                                lua_error!("bad argument #1 to 'copyTable' (table expected, got nil)")
                                            }),
                                            None => lua_error!("attempt to index field '?' (a nil value)"),
                                        },
                                    };
                                    appliedSupportList = Shared::new(src.snapshot());
                                    // add displayGemList for tooltip to display all gems linked to active skills
                                    // L1793
                                    let displayGemList = Table::new();
                                    for g in group.gemList.get().snapshot() {
                                        displayGemList.push(Val::Obj(g));
                                    }
                                    group.extra.set(n!("displayGemList"), displayGemList.clone());
                                    // if skill granted by unique item, go through all support groups in slot
                                    // L1795
                                    if group.source.get().is_some() {
                                        // L1796
                                        if let Some(bySlot) = slotName.and_then(|s| supportLists.bySlot.get(&s)) {
                                            // add socketed supports from other socketGroups
                                            // L1798
                                            for_ipairs!((_j, otherSocketGroup) in skillsTab.socketGroupList.get() => {
                                                // L1799
                                                let other_slot = otherSocketGroup.slot.get();
                                                if other_slot.is_some() && other_slot == group.slot.get()
                                                    && !(otherSocketGroup.source.get().is_some() && otherSocketGroup.source.get() == group.source.get())
                                                {
                                                    // L1800
                                                    for_ipairs!((_k, gem) in otherSocketGroup.gemList.get() => {
                                                        // L1801
                                                        if gem.gemData.get().is_some_and(|g| g.grantedEffect.support) {
                                                            // L1802
                                                            displayGemList.push(Val::Obj(gem.clone()));
                                                        }
                                                    });
                                                }
                                            });
                                            // L1807 LUA-ORDER: `pairs` over a group-keyed table (PoB: address order)
                                            for (_g, supportGroup) in bySlot.pairs() {
                                                // L1808
                                                for_ipairs!((_k, supportEffect) in supportGroup => {
                                                    // L1809
                                                    addBestSupport(&supportEffect, &appliedSupportList, env.mode);
                                                });
                                            }
                                        }
                                    }
                                    // then add supports from crossLinked socketGroups
                                    // L1815 LUA-ORDER: `pairs` over a slot-name-keyed table
                                    for (crossLinkedSupportSlot, crossLinkedSupportGroup) in env.crossLinkedSupportGroups.get().pairs() {
                                        let Some(crossLinkedSupportSlot) = crossLinkedSupportSlot.name() else {
                                            continue;
                                        };
                                        let Some(crossLinkedSupportGroup) = crossLinkedSupportGroup.table() else {
                                            lua_error!("bad argument #1 to 'ipairs' (table expected)")
                                        };
                                        // L1816
                                        for (_k, crossLinkedSupportedSlot) in crossLinkedSupportGroup.ipairs() {
                                            // L1817
                                            let matches = match (&crossLinkedSupportedSlot, slotName) {
                                                (Val::Str(s), Some(n)) => &**s == n.as_str(),
                                                (Val::Nil, None) => true,
                                                _ => false,
                                            };
                                            if matches && supportLists.bySlot.contains_key(&crossLinkedSupportSlot) {
                                                // L1818
                                                for_ipairs!((_j, otherSocketGroup) in skillsTab.socketGroupList.get() => {
                                                    // L1819
                                                    if otherSocketGroup.slot.get() == Some(crossLinkedSupportSlot) {
                                                        // L1820
                                                        for_ipairs!((_l, gem) in otherSocketGroup.gemList.get() => {
                                                            // L1821
                                                            if gem.gemData.get().is_some_and(|g| g.grantedEffect.support) {
                                                                // L1822
                                                                displayGemList.push(Val::Obj(gem.clone()));
                                                            }
                                                        });
                                                    }
                                                });
                                                // L1827 LUA-ORDER: `pairs` over a group-keyed table (PoB: address order)
                                                let groups = supportLists.bySlot.get(&crossLinkedSupportSlot).map(PtrMap::pairs).unwrap_or_default();
                                                for (_g, supportGroup) in groups {
                                                    // L1828
                                                    for_ipairs!((_l, supportEffect) in supportGroup => {
                                                        // L1829
                                                        addBestSupport(&supportEffect, &appliedSupportList, env.mode);
                                                    });
                                                }
                                            }
                                        }
                                    }
                                }
                                // L1836
                                let activeSkill = createActiveSkill(
                                    activeEffect.clone(),
                                    &appliedSupportList,
                                    Rc::downgrade(&env.player),
                                    Some(group.clone()),
                                    None,
                                );
                                // L1837
                                if gemInstance.gemData.get().is_some() {
                                    // L1838
                                    activeSkill.slotName.set(groupCfg.cfg.slotName.get());
                                }
                                // L1840
                                socketGroupSkillList.push(activeSkill.clone());
                                // L1841
                                env.player.activeSkillList.get().push(activeSkill);
                            }
                        });
                        // L1844
                        if gemInstance.gemData.get().is_some() && !accelerate.requirementsGems {
                            // L1845
                            let req = Table::new();
                            req.set(n!("source"), "Gem");
                            req.set(n!("sourceGem"), Val::Obj(gemInstance.clone()));
                            let reqOverride = gemInstance.extra.get(n!("reqOverride"));
                            req.set(n!("Str"), lor(reqOverride.clone(), || gemInstance.extra.get(n!("reqStr"))));
                            req.set(n!("Dex"), lor(reqOverride.clone(), || gemInstance.extra.get(n!("reqDex"))));
                            req.set(n!("Int"), lor(reqOverride, || gemInstance.extra.get(n!("reqInt"))));
                            env.requirementsTableGems.get().push(req);
                        }
                    }
                });

                // L1856
                if !slotHasActiveSkill && group.extra.truthy(n!("displayGemList")) {
                    // L1857
                    group.extra.set(n!("displayGemList"), Val::Nil);
                }
            }
        });

        // Process calculated active skill lists
        // L1863
        for_ipairs!((index, group) in skillsTab.socketGroupList.get() => {
            // L1864-1865
            let slotName = group_slotName(&group);
            // L1866-1868
            let socketGroupSkillList = {
                let m = socketGroupSkillListList.entry(slot_key(slotName)).or_default();
                match m.get(&group) {
                    Some(l) => l,
                    None => {
                        let l = Shared::new(Vec::new());
                        m.set(&group, Some(l.clone()));
                        l
                    }
                }
            };
            // L1869
            if isMain(index) || (group.enabled.get() && group.slotEnabled.get()) {
                // L1870-1874
                let Some(byGroup) = groupCfgList.get_mut(&slot_key(slotName)) else {
                    lua_error!("attempt to index field '?' (a nil value)")
                };
                let groupCfg = match byGroup.get(&group) {
                    Some(c) => c,
                    None => {
                        let c = newGroupCfg(&env, slotName);
                        byGroup.set(&group, Some(c.clone()));
                        c
                    }
                };
                // L1875
                for value in env.modDB.get().List(Some(&*groupCfg.cfg), nm!["GroupProperty"]) {
                    // L1876 LUA-ALIAS: Lua's setSource rewrites the shared mod in place
                    // (ModTools.lua:239-246); the port adds a re-sourced copy (§3.6, as CCR-0074 §4)
                    let inner = match Val::from_value(&value).get(n!("value")) {
                        Val::Mod(m) => m,
                        other => lua_error!("GroupProperty value: value is a {} value", other.lua_type()),
                    };
                    let src = groupCfg.cfg.slotName.get();
                    env.player.modDB.get().AddMod(Arc::new(modLib::setSource(
                        &inner,
                        src.as_ref().map_or("", Name::as_str),
                    )));
                }

                // L1879
                if isMain(index) && socketGroupSkillList.len() > 0 {
                    // Select the main skill from this socket group
                    // L1881
                    let activeSkillIndex: f64;
                    // L1882
                    if env.mode == Mode::CALCS {
                        // L1883
                        group.mainActiveSkillCalcs.set(m_min(
                            socketGroupSkillList.len() as f64,
                            or1(group.mainActiveSkillCalcs.get()),
                        ));
                        // L1884
                        activeSkillIndex = group.mainActiveSkillCalcs.get();
                    } else {
                        // L1886
                        activeSkillIndex = m_min(
                            socketGroupSkillList.len() as f64,
                            or1(group.mainActiveSkill.get()),
                        );
                        // L1887
                        if env.mode == Mode::MAIN {
                            // L1888
                            group.mainActiveSkill.set(activeSkillIndex);
                        }
                    }
                    // L1891 (`list[i]` for a non-integral or out-of-range i is nil)
                    let mainSkill = (activeSkillIndex.fract() == 0.0 && activeSkillIndex >= 1.0)
                        .then(|| socketGroupSkillList.get(activeSkillIndex as usize))
                        .flatten();
                    env.player.mainSkill.set(mainSkill);
                }
            }

            // L1895
            if env.mode == Mode::MAIN {
                // Create display label for the socket group if the user didn't specify one
                // L1897
                let label = socketGroup_label(&group);
                if let Some(label) = label.filter(|l| pob_lua::pattern::r#match(l, "%S", None).is_some()) {
                    // L1898
                    group.displayLabel.set(Some(label));
                } else {
                    // L1900
                    group.displayLabel.set(None);
                    // L1901
                    for_ipairs!((_j, gemInstance) in group.gemList.get() => {
                        // L1902
                        let grantedEffect = gemGrantedEffect(&gemInstance);
                        // L1903
                        if let Some(grantedEffect) = grantedEffect.filter(|g| !g.support && gemInstance.enabled.get()) {
                            // L1904
                            let prev = group.displayLabel.get().map(|l| format!("{l}, ")).unwrap_or_default();
                            group.displayLabel.set(Some(LStr::from(format!("{prev}{}", grantedEffect.name))));
                        }
                    });
                    // L1907
                    if group.displayLabel.get().is_none() {
                        group.displayLabel.set(Some(LStr::from("<No active skills>")));
                    }
                }

                // Save the active skill list for display in the socket group tooltip
                // L1911
                group.displaySkillList.set(Some(socketGroupSkillList));
            } else if env.mode == Mode::CALCS {
                // L1913
                group.displaySkillListCalcs.set(Some(socketGroupSkillList));
            }

            // Check for enabled energy blade to see if we need to regenerate everything.
            // L1917
            if !modDB.conditions.truthy(n!("AffectedByEnergyBlade")) && group.enabled.get() && group.slotEnabled.get() {
                // L1918
                for gemInstance in group.gemList.get().snapshot() {
                    // L1919
                    let grantedEffect = gemGrantedEffect(&gemInstance);
                    // L1920
                    if grantedEffect.is_some_and(|g| !g.support && gemInstance.enabled.get() && g.name.as_str() == "Energy Blade") {
                        // L1921
                        let conditions = match ctx.r#override.conditions.get() {
                            Some(c) => c,
                            None => {
                                let c = Shared::new(Vec::new());
                                ctx.r#override.conditions.set(Some(c.clone()));
                                c
                            }
                        };
                        // L1922
                        conditions.push(n!("AffectedByEnergyBlade"));
                        // L1923
                        return Flow::Restart;
                    }
                }
            }
        });

        // L1929
        if env.player.mainSkill.get().is_none() {
            // Add a default main skill if none are specified
            // L1931
            let melee = d
                .skills
                .get(&n!("Melee"))
                .cloned()
                .unwrap_or_else(|| lua_error!("data.skills.Melee is nil"));
            let defaultEffect = newEffect(melee, 1.0, 0.0);
            defaultEffect.extra.set(n!("enabled"), true);
            // L1937
            let mainSkill = createActiveSkill(
                defaultEffect,
                &Shared::new(Vec::new()),
                Rc::downgrade(&env.player),
                None,
                None,
            );
            env.player.mainSkill.set(Some(mainSkill.clone()));
            // L1938
            env.player.activeSkillList.get().push(mainSkill);
        }

        // Build skill modifier lists
        // L1942 (`pairs` over the list: index order)
        for activeSkill in env.player.activeSkillList.get().snapshot() {
            // L1943
            buildActiveSkillModList(&env, &activeSkill);
        }
    } else {
        // Wipe skillData and readd required data the rest of the data will be added by the rest of code this stops iterative calculations on skillData not being reset
        // L1947 (`pairs` over the list: index order)
        for activeSkill in env.player.activeSkillList.get().snapshot() {
            // L1948
            let skillData = activeSkill.skillData.get().copyTable(true);
            // L1949
            activeSkill.skillData.set(Table::new());
            // L1950
            for value in env
                .modDB
                .get()
                .List(Some(&*activeSkill.skillCfg.get()), nm!["SkillData"])
            {
                // L1951
                let value = Val::from_value(&value);
                skillData_set(&activeSkill, &value);
            }
            // L1953
            for value in activeSkill
                .skillModList
                .get()
                .List(Some(&*activeSkill.skillCfg.get()), nm!["SkillData"])
            {
                // L1954
                let value = Val::from_value(&value);
                skillData_set(&activeSkill, &value);
            }
            // These mods were modified with special expressions in buildActiveSkillModList() use old one to avoid more calculations
            let sd = activeSkill.skillData.get();
            // L1957
            sd.set(
                n!("manaReservationPercent"),
                skillData.get(n!("manaReservationPercent")),
            );
            // L1958
            sd.set(n!("cooldown"), skillData.get(n!("cooldown")));
            // L1959
            sd.set(n!("storedUses"), skillData.get(n!("storedUses")));
            // L1960
            sd.set(n!("CritChance"), skillData.get(n!("CritChance")));
            // L1961
            sd.set(n!("attackTime"), skillData.get(n!("attackTime")));
            // L1962
            sd.set(
                n!("attackSpeedMultiplier"),
                skillData.get(n!("attackSpeedMultiplier")),
            );
            // L1963
            sd.set(
                n!("soulPreventionDuration"),
                activeSkill.extra.get(n!("soulPreventionDuration")),
            );
            // L1964
            sd.set(n!("totemLevel"), skillData.get(n!("totemLevel")));
            // L1965
            sd.set(
                n!("damageEffectiveness"),
                skillData.get(n!("damageEffectiveness")),
            );
            // L1966
            sd.set(n!("stagesMax"), skillData.get(n!("stagesMax")));
            // L1967
            sd.set(
                n!("manaReservationPercent"),
                skillData.get(n!("manaReservationPercent")),
            );
        }
    }

    // Merge Requirements Tables
    // L1972
    env.requirementsTable.set(tableConcat(
        &env.requirementsTableItems.get(),
        &env.requirementsTableGems.get(),
    ));

    Flow::Normal
}

/// `activeSkill.skillData[value.key] = value.value`.
fn skillData_set(activeSkill: &ActiveSkill, value: &Val) {
    let key = match value.get(n!("key")) {
        Val::Str(s) => Key::S(Name::from(&*s)),
        Val::Num(x) => Key::from(x),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!("skillData key is a {} value", other.lua_type()),
    };
    activeSkill.skillData.get().set(key, value.get(n!("value")));
}

#[cfg(test)]
mod fup2_key_tests {
    use super::*;

    /// CalcSetup.lua:1526/1532 `gemsBySource[explodeSource.modSource or explodeSource.id]`: a
    /// passive node's key is its Lua `id`, the string id for a node whose typed `id` is NaN
    /// (CCR-0024 §1); a NaN or nil key reads nil (FOLLOWUPS "FUP1" item 6).
    #[test]
    fn explode_key_and_nan_reads() {
        let node = Rc::new(crate::classes::passive_tree::new_node_table(f64::NAN));
        node.extra.set(n!("id"), "tattoo_1");
        let src = Val::Obj(node);
        assert_eq!(explode_key(&src).str().as_deref(), Some("tattoo_1"));
        let numbered = Rc::new(crate::classes::passive_tree::new_node_table(26725.0));
        assert_eq!(explode_key(&Val::Obj(numbered)).opt_num(), Some(26725.0));
        assert!(read_key(&Val::Num(f64::NAN)).is_none());
        assert!(read_key(&Val::Nil).is_none());
        assert_eq!(read_key(&Val::Num(2.0)), Some(Key::I(2)));
        let e = pob_lua::error::catch(|| val_key(&Val::Num(f64::NAN))).unwrap_err();
        assert!(e.0.contains("table index is NaN"), "{}", e.0);
    }
}
