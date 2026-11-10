//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! perform segment PF-E (CalcPerform.lua 2308-2996): the skill buff loop. Every active skill's
//! `buffList` (buffs, guards, warcries, auras, debuffs, curses, links), wither and Combustion,
//! then the buffs of the skills of its minion (spectre auras, minion curses and debuffs).
//!
//! Owner: PF-E.
//!
//! Shared-mod writes (CCR-0094): Lua writes into mods it does not own at two sites. The port
//! builds a new mod and puts it where Lua's write is visible to the code that follows:
//! - 2380-2384 `warcryBuff[1].warcryPowerBonus = …` (CCR-0014 §3): the rebuilt mod replaces
//!   the entry of `buff.modList` (every later read in the segment goes through that list).
//! - 2780-2792 `setSpectreSource` (`modList[i].source = source`): the rebuilt mod replaces
//!   the entry of the list being re-sourced and every alias of the old mod in `buff.modList`
//!   and `extraAuraModList` (the lists the minion aura branch reads afterwards).
#![allow(non_snake_case)]
// Lua statement shapes are kept (`local x` then assigned, `if a then … elseif …` chains).
#![allow(
    clippy::needless_late_init,
    clippy::nonminimal_bool,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::if_same_then_else,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::len_zero
)]

use super::actor_a::{determineCursePriority, mergeBuff};
use super::PerformCtx;
use crate::modules::calc_offence::helpers_setup::calcSkillDuration;
use crate::modules::calc_tools as calcLib;
use crate::prelude::*;
use pob_gamedata::mod_tools as modLib;
use pob_gamedata::tag;

// ---- Rust-only plumbing ---------------------------------------------------------------------

/// `s:gsub(" ", "")`.
fn ns(s: &str) -> String {
    pob_lua::gsub(s, " ", "", None).0
}

/// A string field of a buff table as a `Name` (`buff.name`, `buff.cond`, …).
fn name_of(v: &Val, what: &str) -> Name {
    match v {
        Val::Str(s) => Name::from(&s[..]),
        other => lua_error!("{what} is a {} value, not a string", other.lua_type()),
    }
}

/// `t.k` where Lua indexes the result further (nil raises "attempt to index").
fn sub(t: &Table, k: &str) -> Table {
    match t.get(Name::from(k)) {
        Val::Table(x) => x,
        other => lua_error!(
            "attempt to index field '{k}' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `allyBuffs` where Lua indexes it (`env.partyMembers["Aura"]`, nil raises).
fn ally(allyBuffs: &Option<Table>) -> Table {
    match allyBuffs {
        Some(t) => t.clone(),
        None => lua_error!("attempt to index upvalue 'allyBuffs' (a nil value)"),
    }
}

/// `ipairs(list)` over a plain list of mods (`buff.modList`, `{ mod, … }`) or a ModList.
fn mods_of(v: &Val) -> Vec<ModRef> {
    match v {
        Val::Table(t) => {
            let mut out = Vec::new();
            for_ipairs!((_, m) in t => {
                match m {
                    Val::Mod(m) => out.push(m),
                    Val::Data(Value::Mod(m)) => out.push(m),
                    other => lua_error!("a mod list holds a {} value", other.lua_type()),
                }
            });
            out
        }
        Val::Obj(_) => match v.obj::<ModStore>() {
            Some(s) => s.ipairs(),
            None => lua_error!("bad argument to ipairs (table expected, got userdata)"),
        },
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `buff.modList` itself (a plain list, CalcActiveSkill.lua:838), where Lua passes or writes it.
fn buff_mod_list(buff: &Table) -> Table {
    match buff.get(n!("modList")) {
        Val::Table(t) => t,
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `mergeBuff` takes its source as a table it walks with `ipairs` (frozen signature
/// `src: &Table`); a ModList source is passed as the list of its mods (`ipairs(modList)`).
fn list_table(l: &ModList) -> Table {
    let t = Table::new();
    for m in l.ipairs() {
        t.push(Val::Mod(m));
    }
    t
}

/// A list of mods as a fresh plain table (`local newModList = {}` + `t_insert`).
fn plain_table(mods: &[ModRef]) -> Table {
    let t = Table::new();
    for m in mods {
        t.push(Val::Mod(m.clone()));
    }
    t
}

fn obj(s: &ModList) -> Val {
    Val::Obj(s.clone())
}

/// `x` used in arithmetic (a mod value, a tag field).
fn num(v: &Value) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Str(s) => pob_lua::tonumber(s)
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a string value")),
        other => lua_error!(
            "attempt to perform arithmetic on a {} value",
            other.lua_type()
        ),
    }
}

/// A calc value used as a mod value (`NewMod(…, value, …)`).
fn to_value(v: &Val) -> Value {
    match v {
        Val::Nil => Value::Nil,
        Val::Bool(b) => Value::Bool(*b),
        Val::Num(n) => Value::Num(*n),
        Val::Str(s) => Value::Str(AStr::from(&**s)),
        Val::Mod(m) => Value::Mod(m.clone()),
        Val::Data(d) => d.clone(),
        other => lua_error!("a {} value cannot be a mod value", other.lua_type()),
    }
}

/// The Lua `true`/nil of `modDB:Flag(…)` as a table value.
fn flag_val(b: bool) -> Val {
    if b {
        Val::Bool(true)
    } else {
        Val::Nil
    }
}

/// `activeSkill.skillTypes[SkillType.X]`.
fn has_type(skill: &ActiveSkillRef, t: SkillType::T) -> bool {
    skill.skillTypes.get().with(|s| s.has(t))
}

/// `a == b` on two actor values (nil == nil is true).
fn same_actor(a: &Option<ActorRef>, b: &Option<ActorRef>) -> bool {
    match (a, b) {
        (Some(x), Some(y)) => Rc::ptr_eq(x, y),
        (None, None) => true,
        _ => false,
    }
}

/// `env.player.mainSkill` where Lua indexes it.
fn main_skill(env: &EnvRef) -> ActiveSkillRef {
    env.player
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `buffs[name].notBuff = true` (CalcPerform.lua 2331, 2810, 2834): `buffs[name]` is the
/// ModList `mergeBuff` created (`ModStore.notBuff`, CCR-0030 #7 / CCR-0076).
fn set_notBuff(buffs: &Table, name: Name) {
    let Some(store) = buffs.obj::<ModStore>(name) else {
        lua_error!("attempt to index field '?' (a nil value)")
    };
    store.notBuff.set(true);
}

/// The merge loop that builds `extraAuraModList` (CalcPerform.lua 2448-2460; the same Lua at
/// 2560-2572, 2606-2618, 2714-2730 (Link, with its `onAdd` lines 2725-2728) and 2842-2854):
/// ```lua
/// for _, value in ipairs(<store>:List(skillCfg, "<name>")) do
///     local add = true
///     for _, mod in ipairs(extraAuraModList) do
///         if modLib.compareModParams(mod, value.mod) then
///             mod.value = mod.value + value.mod.value
///             add = false
///             break
///         end
///     end
///     if add then
///         t_insert(extraAuraModList, copyTable(value.mod, true))
///     end
/// end
/// ```
/// The entries are fresh copies (`copyTable(value.mod, true)`) and only reach a store after
/// the loop (contracts.md#mod-mutation), so they are built as owned mods and frozen after.
fn merge_extra_mods(values: Vec<Value>, mut onAdd: impl FnMut(&ModRef)) -> Vec<ModRef> {
    let mut extra: Vec<Mod> = Vec::new();
    for value in values {
        let Some(vmod) = value.inner_mod().cloned() else {
            lua_error!("attempt to index field 'mod' (a nil value)")
        };
        // local add = true
        let mut add = true;
        for m in &mut extra {
            if modLib::compareModParams(m, &vmod) {
                // mod.value = mod.value + value.mod.value
                m.value = Value::Num(num(&m.value) + num(&vmod.value));
                add = false;
                break;
            }
        }
        if add {
            // t_insert(extraAuraModList, copyTable(value.mod, true))
            extra.push((*vmod).clone());
            onAdd(&vmod);
        }
    }
    extra.into_iter().map(Arc::new).collect()
}

/// `(m_floor(v) == v) and m_modf(round(v * scale, 2)) or v * scale` (2537, 2539, 2919, 2921;
/// `m_modf`'s first result, which is truthy even when 0).
fn totem_scale(v: f64, scale: f64) -> f64 {
    if m_floor(v) == v {
        m_modf(round(v * scale, Some(2))).0
    } else {
        v * scale
    }
}

/// The Totem copies of the defence mods (CalcPerform.lua 2530-2545; the same Lua at
/// 2912-2927):
/// ```lua
/// for _, modList in ipairs(lists) do
///     for _, mod in ipairs(modList) do
///         if mod.name == "EnergyShield" or … or mod.name:match("Resist?M?a?x?$") then
///             local totemMod = copyTable(mod)
///             totemMod.name = "Totem"..totemMod.name
///             if scale ~= 1 then … end
///             srcList:AddMod(totemMod)
/// ```
fn add_totem_mods(srcList: &ModList, lists: [&[ModRef]; 2], scale: f64) {
    for modList in lists {
        for m in modList {
            let name = m.name.as_str();
            if name == "EnergyShield"
                || name == "Armour"
                || name == "Evasion"
                || pob_lua::lmatch(name, "Resist?M?a?x?$", None).is_some()
            {
                // local totemMod = copyTable(mod) (deep: the value table and its mod are fresh)
                let mut totemMod: Mod = (**m).clone();
                totemMod.name = cat!("Totem", totemMod.name);
                if scale != 1.0 {
                    if let Value::Num(v) = totemMod.value {
                        totemMod.value = Value::Num(totem_scale(v, scale));
                    } else if let Some(inner) = totemMod.value.inner_mod().cloned() {
                        let mut innerMod: Mod = (*inner).clone();
                        let v = match innerMod.value {
                            Value::Num(v) => v,
                            ref other => lua_error!(
                                "bad argument #1 to 'floor' (number expected, got {})",
                                other.lua_type()
                            ),
                        };
                        innerMod.value = Value::Num(totem_scale(v, scale));
                        let Value::Table(t) = &totemMod.value else {
                            unreachable!("inner_mod is a table value")
                        };
                        totemMod.value = Value::Table(Arc::new(
                            t.with(n!("mod"), Value::Mod(Arc::new(innerMod))),
                        ));
                    }
                }
                srcList.AddMod(Arc::new(totemMod));
            }
        }
    }
}

/// `warcryBuff[1].warcryPowerBonus` (nil when the tag has none; no first tag raises).
fn warcry_power_bonus(m: &Mod) -> Option<f64> {
    let Some(tag) = m.tags.first() else {
        lua_error!("attempt to index field '?' (a nil value)")
    };
    tag.extra_get(n!("warcryPowerBonus")).map(num)
}

/// `setSpectreSource(modList, sourceSkill)` (CalcPerform.lua 2780-2792, a closure over
/// `activeSkill` and `castingMinion`): `modList[i].source = source` for every mod of the list.
/// The rebuilt mods replace their originals in `modList` and in the lists that alias them
/// (`buff.modList`, `extraAuraModList`; CCR-0094 §2).
fn setSpectreSource(
    activeSkill: &ActiveSkillRef,
    castingMinion: &ActorRef,
    modList: &ModList,
    sourceSkill: Option<Name>,
    buffModList: &Table,
    extraAuraModList: &mut [ModRef],
) {
    // L2781
    if activeSkill.skillFlags.get().truthy(n!("spectre")) {
        let minionName = match castingMinion.minionData.get() {
            Some(d) => d.name,
            None => lua_error!("attempt to index field 'minionData' (a nil value)"),
        };
        // L2782
        let mut source = String::from("Spectre:");
        // L2783
        if let Some(sourceSkill) = sourceSkill {
            // L2784
            source = format!("{source}{sourceSkill} - {minionName}");
        } else {
            // L2786
            source = format!("{source}{minionName}");
        }
        // L2788
        let old = modList.ipairs();
        let mut new: Vec<ModRef> = Vec::with_capacity(old.len());
        for m in &old {
            // L2789
            let mut c: Mod = (**m).clone();
            c.source = Some(AStr::from(source.as_str()));
            let c = Arc::new(c);
            // the same table seen through its other lists
            for_ipairs!((i, v) in buffModList => {
                if let Val::Mod(x) = &v {
                    if Arc::ptr_eq(x, m) {
                        buffModList.set(i as i64, Val::Mod(c.clone()));
                    }
                }
            });
            for x in extraAuraModList.iter_mut() {
                if Arc::ptr_eq(x, m) {
                    *x = c.clone();
                }
            }
            new.push(c);
        }
        // the same list object, its entries replaced in place
        modList.wipe();
        modList.AddList(&new);
    }
}

// ---- the segment ----------------------------------------------------------------------------

/// @pob Modules/CalcPerform.lua:2308-2996 h=f1215178
/// PF-E: CalcPerform.lua 2308-2996 (auras, buffs, curses setup, warcries).
pub(crate) fn seg(ctx: &mut PerformCtx) -> Flow {
    let env = ctx.env.clone();
    let modDB = ctx.modDB.clone();
    let enemyDB = ctx.enemyDB.clone();
    let partyTabEnableExportBuffs = ctx.partyTabEnableExportBuffs;
    // L2308
    for_ipairs!((_, activeSkill) in env.player.activeSkillList.get() => {
        // L2309
        let skillModList = activeSkill.skillModList.get();
        // L2310
        let skillCfg = activeSkill.skillCfg.get();
        // L2311
        for_ipairs!((_, buff) in activeSkill.buffList.get() => {
            let btype = buff.str(n!("type"));
            let is = |t: &str| btype.as_deref() == Some(t);
            // L2312
            if buff.truthy(n!("cond"))
                && !skillModList.GetCondition(name_of(&buff.get(n!("cond")), "buff.cond"), Some(&skillCfg), false)
            {
                // L2313 Nothing!
            } else if buff.truthy(n!("enemyCond"))
                && !enemyDB.GetCondition(name_of(&buff.get(n!("enemyCond")), "buff.enemyCond"), None, false)
            {
                // L2314
                // L2315 Also nothing :/
            } else if is("GlobalDB") {
                // L2316
                // L2317 Allows a skill mod to affect other skills through modDB
                modDB.AddList(&mods_of(&buff.get(n!("modList"))));
            } else if is("Buff") {
                // L2318
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2319
                if env.mode_buffs.get()
                    && (!activeSkill.skillFlags.get().truthy(n!("totem")) || buff.truthy(n!("allowTotemBuff")))
                {
                    // L2320
                    let skillCfg = if buff.truthy(n!("activeSkillBuff")) { Some(skillCfg.clone()) } else { None };
                    let skillCfg = skillCfg.as_deref();
                    // L2321
                    let modStore = if buff.truthy(n!("activeSkillBuff")) { skillModList.clone() } else { modDB.clone() };
                    // L2322
                    if !buff.truthy(n!("applyNotPlayer")) {
                        // L2323
                        activeSkill.buffSkill.set(true);
                        // L2324
                        modDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                        // L2325
                        let srcList = ModStore::ModList(None);
                        // L2326
                        let inc = modStore.Sum(ModType::INC, skillCfg, nm!["BuffEffect", "BuffEffectOnSelf", "BuffEffectOnPlayer"])
                            + skillModList.Sum(ModType::INC, skillCfg, &[cat!(ns(&buffName), "Effect")]);
                        // L2327
                        let more = modStore.More(skillCfg, nm!["BuffEffect", "BuffEffectOnSelf"]);
                        // L2328
                        srcList.ScaleAddList(&mods_of(&buff.get(n!("modList"))), (1.0 + inc / 100.0) * more, false);
                        // L2329
                        mergeBuff(&list_table(&srcList), &ctx.buffs, buffName);
                        // L2330
                        if activeSkill.skillData.get().truthy(n!("thisIsNotABuff")) {
                            // L2331
                            set_notBuff(&ctx.buffs, buffName);
                        }
                    }
                    // L2334
                    let minion = env.minion.get();
                    if let Some(minion) = minion.as_ref().filter(|m| {
                        !m.hostile.get()
                            && (buff.truthy(n!("applyMinions"))
                                || buff.truthy(n!("applyAllies"))
                                || skillModList.Flag(None, nm!["BuffAppliesToAllies"]))
                    }) {
                        // L2335
                        activeSkill.minionBuffSkill.set(true);
                        // L2336
                        minion.modDB.get().conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                        // L2337
                        let srcList = ModStore::ModList(None);
                        // L2338
                        let inc = modStore.Sum(ModType::INC, skillCfg, nm!["BuffEffect"])
                            + minion.modDB.get().Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]);
                        // L2339
                        let more = modStore.More(skillCfg, nm!["BuffEffect"])
                            * minion.modDB.get().More(None, nm!["BuffEffectOnSelf"]);
                        // L2340
                        srcList.ScaleAddList(&mods_of(&buff.get(n!("modList"))), (1.0 + inc / 100.0) * more, false);
                        // L2341
                        mergeBuff(&list_table(&srcList), &ctx.minionBuffs, buffName);
                    }
                    // L2343
                    if partyTabEnableExportBuffs
                        && (buff.truthy(n!("applyAllies"))
                            || skillModList.Flag(None, nm!["BuffAppliesToAllies"])
                            || skillModList.Flag(None, nm!["BuffAppliesToPartyMembers"]))
                    {
                        // L2344
                        let inc = modStore.Sum(ModType::INC, skillCfg, nm!["BuffEffect"])
                            + skillModList.Sum(ModType::INC, skillCfg, &[cat!(ns(&buffName), "Effect")]);
                        // L2345
                        let more = modStore.More(skillCfg, nm!["BuffEffect"]);
                        // L2346
                        let aura = sub(&ctx.buffExports, "Aura");
                        let other = aura.get(n!("otherEffects"));
                        let other = if other.truthy() { other } else { Val::Table(Table::new()) };
                        aura.set(n!("otherEffects"), other);
                        // L2347
                        let effectMult = (1.0 + inc / 100.0) * more;
                        sub(&aura, "otherEffects").set(
                            buffName,
                            tbl! { effectMult = effectMult, modList = buff.get(n!("modList")) },
                        );
                    }
                }
            } else if is("Guard") {
                // L2350
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2351
                if env.mode_buffs.get()
                    && (!activeSkill.skillFlags.get().truthy(n!("totem")) || buff.truthy(n!("allowTotemBuff")))
                {
                    // L2352
                    let skillCfg = if buff.truthy(n!("activeSkillBuff")) { Some(skillCfg.clone()) } else { None };
                    let skillCfg = skillCfg.as_deref();
                    // L2353
                    let modStore = if buff.truthy(n!("activeSkillBuff")) { skillModList.clone() } else { modDB.clone() };
                    // L2354
                    if !buff.truthy(n!("applyNotPlayer")) {
                        // L2355
                        activeSkill.buffSkill.set(true);
                        // L2356
                        let srcList = ModStore::ModList(None);
                        // L2357
                        let inc = modStore.Sum(ModType::INC, skillCfg, nm!["BuffEffect", "BuffEffectOnSelf", "BuffEffectOnPlayer"]);
                        // L2358
                        let more = modStore.More(skillCfg, nm!["BuffEffect", "BuffEffectOnSelf"]);
                        // L2359
                        srcList.ScaleAddList(&mods_of(&buff.get(n!("modList"))), (1.0 + inc / 100.0) * more, false);
                        // L2360
                        mergeBuff(&list_table(&srcList), &ctx.guards, buffName);
                    }
                }
            } else if is("Warcry") {
                // L2363
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2364
                if env.mode_buffs.get() {
                    // L2365
                    let skillCfg: &ModCfg = &skillCfg;
                    // L2366 (`skillModList or modDB`: skillModList is never nil)
                    let modStore = skillModList.clone();
                    // L2367
                    let warcryName = {
                        let s = pob_lua::gsub(&buffName, " Cry", "", None).0;
                        let s = pob_lua::gsub(&s, "'s", "", None).0;
                        pob_lua::gsub(&s, " ", "", None).0
                    };
                    // L2368
                    let baseExerts = modStore.Sum(
                        ModType::BASE,
                        Some(&main_skill(&env).skillCfg.get()),
                        &[cat!(warcryName, "ExertedAttacks")],
                    );
                    // L2369
                    if baseExerts > 0.0 {
                        // L2370 (`Sum` never returns nil, so `or 0` never applies)
                        let extraExertions = modStore.Sum(ModType::BASE, None, nm!["ExtraExertedAttacks"]);
                        // L2371
                        let exertMultiplier = modStore.More(None, nm!["ExtraExertedAttacks"]);
                        // L2372
                        env.player.modDB.get().NewMod(cat!("Num", warcryName, "Exerts"), ModType::BASE, m_floor((baseExerts + extraExertions) * exertMultiplier), None, 0, 0, &[]);
                        // L2373
                        if !ctx.warcryList.truthy(buffName) {
                            // L2374
                            env.player.modDB.get().NewMod(
                                n!("Multiplier:ExertingWarcryCount"),
                                ModType::BASE,
                                1.0,
                                &buffName,
                                0,
                                0,
                                &[],
                            );
                            // L2375
                            ctx.warcryList.set(buffName, true);
                        }
                    }
                    // L2378
                    if !activeSkill.skillModList.get().Flag(None, nm!["CannotShareWarcryBuffs"]) {
                        // L2379
                        let warcryPower = {
                            let o = modDB.Override(None, nm!["WarcryPower"]);
                            if o.truthy() {
                                num(&o)
                            } else {
                                m_max(
                                    modDB.Sum(ModType::BASE, None, nm!["WarcryPower"])
                                        * (1.0 + modDB.Sum(ModType::INC, None, nm!["WarcryPower"]) / 100.0),
                                    modDB.Sum(ModType::BASE, None, nm!["MinimumWarcryPower"]),
                                )
                            }
                        };
                        // L2380
                        let modList = buff.table(n!("modList"));
                        let buffModListVal = buff.get(n!("modList"));
                        for (i, warcryBuff) in mods_of(&buffModListVal).into_iter().enumerate() {
                            // L2381
                            let Some(tag1) = warcryBuff.tags.first() else { continue };
                            if tag1.effectType.is_some_and(|e| e.as_str() == "Warcry") && tag1.div.is_some() {
                                // L2382 (a write into the mod's first tag: CCR-0014 §3, CCR-0094 §1)
                                let div = tag1.div.unwrap_or_default();
                                let bonus = m_floor(
                                    (match tag1.limit {
                                        Some(limit) => m_min(warcryPower, limit),
                                        None => warcryPower,
                                    }) / div,
                                );
                                let mut m: Mod = (*warcryBuff).clone();
                                let mut ex = m.tags[0].extra.take().map(|b| *b).unwrap_or_default();
                                ex.set(n!("warcryPowerBonus"), Value::Num(bonus));
                                m.tags[0].extra = Some(Box::new(ex));
                                match &modList {
                                    Some(t) => t.set(i as i64 + 1, Val::Mod(Arc::new(m))),
                                    None => lua_error!("buff.modList of {buffName} is not a plain list"),
                                }
                            }
                        }
                        // L2385
                        let full_duration = calcSkillDuration(&modStore, Some(skillCfg), &activeSkill.skillData.get(), Some(&env), Some(&enemyDB));
                        // L2386
                        let cooldownOverride = modStore.Override(Some(skillCfg), nm!["CooldownRecovery"]);
                        // L2387
                        let actual_cooldown = if cooldownOverride.truthy() {
                            num(&cooldownOverride)
                        } else {
                            (activeSkill.skillData.get().num(n!("cooldown"))
                                + modStore.Sum(ModType::BASE, Some(skillCfg), nm!["CooldownRecovery"]))
                                / calcLib::r#mod(&modStore, Some(skillCfg), nm!["CooldownRecovery"])
                        };
                        // L2388
                        let uptime = if modDB.Flag(None, nm!["Condition:WarcryMaxHit"]) {
                            1.0
                        } else {
                            m_min(full_duration / actual_cooldown, 1.0)
                        };
                        // L2389 (`{}` when not Rallying Cry: nothing is ever added to it)
                        let extraWarcryModList: Option<ModList> =
                            if activeSkill.activeEffect.get().grantedEffect.get().name.as_str() == "Rallying Cry" {
                                Some(ModStore::ModList(None))
                            } else {
                                None
                            };
                        // L2390
                        if !modDB.Flag(None, nm!["CannotGainWarcryBuffs"]) {
                            // L2391
                            if !buff.truthy(n!("applyNotPlayer")) {
                                // L2392
                                activeSkill.buffSkill.set(true);
                                // L2393
                                modDB.conditions.set(cat!("AffectedBy", warcryName), true);
                                // L2394
                                let srcList = ModStore::ModList(None);
                                // L2395
                                let inc = modStore.Sum(ModType::INC, Some(skillCfg), nm!["BuffEffect", "BuffEffectOnSelf", "BuffEffectOnPlayer"]);
                                // L2396
                                let more = modStore.More(Some(skillCfg), nm!["BuffEffect", "BuffEffectOnSelf"]);
                                // L2397
                                for warcryBuff in mods_of(&buff.get(n!("modList"))) {
                                    // L2398
                                    let mult = (1.0 + inc / 100.0) * more * warcry_power_bonus(&warcryBuff).unwrap_or(1.0) * uptime;
                                    // L2399
                                    srcList.ScaleAddList(&[warcryBuff], mult, false);
                                }
                                // L2401
                                mergeBuff(&list_table(&srcList), &ctx.buffs, buffName);
                            }
                        }
                        // L2404
                        if let Some(minion) = env.minion.get() {
                            // L2405
                            activeSkill.minionBuffSkill.set(true);
                            // L2406
                            minion.modDB.get().conditions.set(cat!("AffectedBy", warcryName), true);
                            // L2407
                            let srcList = ModStore::ModList(None);
                            // L2408
                            let inc = skillModList.Sum(ModType::INC, Some(skillCfg), nm!["BuffEffect"])
                                + minion.modDB.get().Sum(ModType::INC, Some(skillCfg), nm!["BuffEffectOnSelf"]);
                            // L2409
                            let more = skillModList.More(Some(skillCfg), nm!["BuffEffect"])
                                * minion.modDB.get().More(Some(skillCfg), nm!["BuffEffectOnSelf"]);
                            // L2410
                            for warcryBuff in mods_of(&buff.get(n!("modList"))) {
                                // L2411
                                let mult = (1.0 + inc / 100.0) * more * warcry_power_bonus(&warcryBuff).unwrap_or(1.0) * uptime;
                                // L2412
                                srcList.ScaleAddList(&[warcryBuff], mult, false);
                            }
                            // L2414 Special handling for the minion side to add the flat damage bonus
                            // L2415
                            if activeSkill.activeEffect.get().grantedEffect.get().name.as_str() == "Rallying Cry" {
                                let extraWarcryModList = extraWarcryModList
                                    .as_ref()
                                    .expect("a ModList for Rallying Cry (2389)");
                                // L2416
                                let warcryPowerBonus = m_floor(m_min(warcryPower, 30.0) / 5.0);
                                // L2417
                                let rallyingWeaponEffect = m_floor(
                                    activeSkill.skillModList.get().Sum(
                                        ModType::BASE,
                                        Some(&main_skill(&env).skillCfg.get()),
                                        nm!["RallyingCryAllyDamageBonusPer5Power"],
                                    ) * warcryPowerBonus,
                                );
                                // L2418
                                let inc = modStore.Sum(ModType::INC, Some(skillCfg), nm!["BuffEffect"])
                                    + minion.modDB.get().Sum(ModType::INC, Some(skillCfg), nm!["BuffEffectOnSelf"]);
                                // L2419
                                let rallyingBonusMoreMultiplier = 1.0
                                    + activeSkill.skillModList.get().Sum(
                                        ModType::BASE,
                                        Some(&main_skill(&env).skillCfg.get()),
                                        nm!["RallyingCryMinionDamageBonusMultiplier"],
                                    );
                                // L2420 Add all damage types
                                // L2421
                                let dmgTypeList = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
                                // L2422
                                for damageType in dmgTypeList {
                                    let weaponData1 = env.player.weaponData1.get();
                                    // L2423
                                    if weaponData1.truthy(cat!(damageType, "Min")) {
                                        // L2424
                                        extraWarcryModList.NewMod(
                                            cat!(damageType, "Min"),
                                            ModType::BASE,
                                            weaponData1.num(cat!(damageType, "Min")) * rallyingWeaponEffect / 100.0,
                                            "Rallying Cry",
                                            0,
                                            KeywordFlag::Attack,
                                            &[tag!({ type = GlobalEffect, effectType = "Warcry", div = 5.0, limit = 30.0 })],
                                        );
                                    }
                                    // L2426
                                    if weaponData1.truthy(cat!(damageType, "Max")) {
                                        // L2427
                                        extraWarcryModList.NewMod(
                                            cat!(damageType, "Max"),
                                            ModType::BASE,
                                            weaponData1.num(cat!(damageType, "Max")) * rallyingWeaponEffect / 100.0,
                                            "Rallying Cry",
                                            0,
                                            KeywordFlag::Attack,
                                            &[tag!({ type = GlobalEffect, effectType = "Warcry", div = 5.0, limit = 30.0 })],
                                        );
                                    }
                                }
                                // L2430
                                srcList.ScaleAddList(
                                    &extraWarcryModList.ipairs(),
                                    (1.0 + inc / 100.0) * rallyingBonusMoreMultiplier * uptime,
                                    false,
                                );
                            }
                            // L2432
                            mergeBuff(&list_table(&srcList), &ctx.minionBuffs, buffName);
                        }
                        // L2434
                        if partyTabEnableExportBuffs {
                            // L2435
                            let newModList = ModStore::ModList(None);
                            // L2436
                            let inc = skillModList.Sum(ModType::INC, Some(skillCfg), nm!["BuffEffect"]);
                            // L2437
                            let more = skillModList.More(Some(skillCfg), nm!["BuffEffect"]);
                            // L2438
                            newModList.AddList(&mods_of(&buff.get(n!("modList"))));
                            // L2439
                            if let Some(l) = &extraWarcryModList {
                                newModList.AddList(&l.ipairs());
                            }
                            // L2440
                            let effectMult = (1.0 + inc / 100.0) * more * uptime;
                            sub(&ctx.buffExports, "Warcry").set(
                                buffName,
                                tbl! { effectMult = effectMult, modList = obj(&newModList) },
                            );
                        }
                    }
                }
            } else if is("Aura") {
                // L2444
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2445
                if env.mode_buffs.get() {
                    // L2446 Check for extra modifiers to apply to aura skills
                    // L2447-2460
                    let extraAuraModList =
                        merge_extra_mods(modDB.List(Some(&skillCfg), nm!["ExtraAuraEffect"]), |_| {});
                    let buffMods = mods_of(&buff.get(n!("modList")));
                    // L2461
                    if !activeSkill.skillData.get().truthy(n!("auraCannotAffectSelf")) {
                        // L2462
                        let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "BuffEffectOnSelf", "AuraEffectOnSelf", "AuraBuffEffect", "SkillAuraEffectOnSelf"]);
                        // L2463
                        let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "BuffEffectOnSelf", "AuraEffectOnSelf", "AuraBuffEffect", "SkillAuraEffectOnSelf"]);
                        // L2464
                        let mult = (1.0 + inc / 100.0) * more;
                        // L2465
                        if modDB.Flag(None, nm!["AlliesAurasCannotAffectSelf"])
                            || !ally(&ctx.allyBuffs).truthy(n!("Aura"))
                            || !sub(&ally(&ctx.allyBuffs), "Aura").truthy(buffName)
                            || sub(&sub(&ally(&ctx.allyBuffs), "Aura"), &buffName).num(n!("effectMult")) / 100.0 <= mult
                        {
                            // L2466
                            activeSkill.buffSkill.set(true);
                            // L2467
                            modDB.conditions.set(n!("AffectedByAura"), true);
                            // L2468
                            if pob_lua::string::sub(&buffName, 1, Some(4)) == "Vaal" {
                                // L2469
                                modDB.conditions.set(
                                    cat!("AffectedBy", ns(&pob_lua::string::sub(&buffName, 6, None))),
                                    true,
                                );
                            }
                            // L2471
                            modDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                            // L2472
                            let srcList = ModStore::ModList(None);
                            // L2473
                            srcList.ScaleAddList(&buffMods, mult, false);
                            // L2474
                            srcList.ScaleAddList(&extraAuraModList, mult, false);
                            // L2475
                            mergeBuff(&list_table(&srcList), &ctx.buffs, buffName);
                        }
                    }
                    // L2478
                    if !(modDB.Flag(None, nm!["SelfAurasCannotAffectAllies"])
                        || modDB.Flag(None, nm!["SelfAurasOnlyAffectYou"])
                        || modDB.Flag(None, nm!["SelfAuraSkillsCannotAffectAllies"]))
                    {
                        // L2479
                        let minion = env.minion.get();
                        if let Some(minion) = minion.as_ref().filter(|m| {
                            !m.hostile.get() || modDB.Flag(None, nm!["AurasAffectEnemies"])
                        }) {
                            let minionDB = minion.modDB.get();
                            // L2480
                            let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect"])
                                + minionDB.Sum(ModType::INC, Some(&skillCfg), nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                            // L2481
                            let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect"])
                                * minionDB.More(Some(&skillCfg), nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                            // L2482
                            let mult = (1.0 + inc / 100.0) * more;
                            // L2483
                            if !ally(&ctx.allyBuffs).truthy(n!("Aura"))
                                || !sub(&ally(&ctx.allyBuffs), "Aura").truthy(buffName)
                                || sub(&sub(&ally(&ctx.allyBuffs), "Aura"), &buffName).num(n!("effectMult")) / 100.0 <= mult
                            {
                                // L2484
                                activeSkill.minionBuffSkill.set(true);
                                // L2485
                                minionDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                                // L2486
                                minionDB.conditions.set(n!("AffectedByAura"), true);
                                // L2487
                                let srcList = ModStore::ModList(None);
                                // L2488
                                srcList.ScaleAddList(&buffMods, mult, false);
                                // L2489
                                srcList.ScaleAddList(&extraAuraModList, mult, false);
                                // L2490
                                mergeBuff(&list_table(&srcList), &ctx.minionBuffs, buffName);
                            }
                        }
                        // L2493
                        let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect"]);
                        // L2494
                        let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect"]);
                        // L2495
                        let mult = (1.0 + inc / 100.0) * more;
                        // L2496
                        let newModList = ModStore::ModList(None);
                        // L2497
                        newModList.AddList(&buffMods);
                        // L2498
                        newModList.AddList(&extraAuraModList);
                        let auraExports = sub(&ctx.buffExports, "Aura");
                        // L2499
                        if auraExports.truthy(buffName) {
                            // L2500
                            auraExports.set(cat!(buffName, "_Debuff"), auraExports.get(buffName));
                        }
                        // L2502
                        auraExports.set(buffName, tbl! { effectMult = mult, modList = obj(&newModList) });
                        // L2503
                        if modDB.Flag(None, nm!["AurasAffectEnemies"])
                            && !activeSkill.skillModList.get().Flag(Some(&skillCfg), nm!["SelfAurasAffectYouAndLinkedTarget"])
                        {
                            // L2504-2511
                            let mut newModList: Vec<ModRef> = Vec::new();
                            // L2505
                            let srcList = ModStore::ModList(None);
                            // L2506
                            for m in &buffMods {
                                // L2507
                                newModList.push(m.clone());
                            }
                            // L2509
                            for m in &extraAuraModList {
                                // L2510
                                newModList.push(m.clone());
                            }
                            // L2512
                            let key = if auraExports.truthy(buffName) { cat!(buffName, "_Debuff") } else { buffName };
                            auraExports.set(key, tbl! { effectMult = mult, modList = plain_table(&newModList) });
                            // L2513
                            srcList.ScaleAddList(&buffMods, mult, false);
                            // L2514
                            srcList.ScaleAddList(&extraAuraModList, mult, false);
                            // L2515
                            mergeBuff(&list_table(&srcList), &ctx.debuffs, buffName);
                        }
                    }
                    // L2518
                    let mainSkill = main_skill(&env);
                    if mainSkill.skillFlags.get().truthy(n!("totem"))
                        && !(modDB.Flag(None, nm!["SelfAurasCannotAffectAllies"])
                            || modDB.Flag(None, nm!["SelfAuraSkillsCannotAffectAllies"]))
                    {
                        // L2519
                        activeSkill.totemBuffSkill.set(true);
                        let mainSkillModList = mainSkill.skillModList.get();
                        // L2520
                        mainSkillModList.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                        // L2521
                        mainSkillModList.conditions.set(n!("AffectedByAura"), true);
                        // L2523
                        let srcList = ModStore::ModList(None);
                        // L2524
                        let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "AuraBuffEffect"]);
                        // L2525
                        let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "AuraBuffEffect"]);
                        // L2526 (lists = {extraAuraModList, buff.modList})
                        // L2527
                        let mut scale = (1.0 + inc / 100.0) * more;
                        // L2528
                        scale = m_max(scale, 0.0);
                        // L2530-2545
                        add_totem_mods(&srcList, [&extraAuraModList, &buffMods], scale);
                        // L2546
                        mergeBuff(&list_table(&srcList), &ctx.buffs, cat!("Totem ", buffName));
                    }
                    // L2548 check if aura has a debuff added to it, but does not have an auraDebuff
                    // L2549
                    if env.mode_effective.get()
                        && !modDB.List(Some(&skillCfg), nm!["ExtraAuraDebuffEffect"]).is_empty()
                        && !(modDB.Flag(None, nm!["SelfAurasOnlyAffectYou"])
                            || activeSkill.skillModList.get().Flag(Some(&skillCfg), nm!["SelfAurasAffectYouAndLinkedTarget"]))
                    {
                        // L2550
                        let mut auraDebuffFound = false;
                        // L2551
                        for_ipairs!((_, buff) in activeSkill.buffList.get() => {
                            // L2552
                            if buff.str(n!("type")).as_deref() == Some("AuraDebuff") {
                                // L2553
                                auraDebuffFound = true;
                                // L2554
                                break;
                            }
                        });
                        // L2557
                        if !auraDebuffFound {
                            // L2558
                            activeSkill.debuffSkill.set(true);
                            // L2559-2572
                            let extraAuraModList =
                                merge_extra_mods(modDB.List(Some(&skillCfg), nm!["ExtraAuraDebuffEffect"]), |_| {});
                            // L2573
                            let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "DebuffEffect"]);
                            // L2574
                            let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "DebuffEffect"]);
                            // L2575 (`mult` is an implicit global here, CCR-0004; never read before)
                            #[allow(unused_assignments)]
                            let mut mult = (1.0 + inc / 100.0) * more;
                            // L2576
                            let newModList = ModStore::ModList(None);
                            // L2577
                            newModList.AddList(&extraAuraModList);
                            // L2578
                            let auraExports = sub(&ctx.buffExports, "Aura");
                            let key = if auraExports.truthy(buffName) { cat!(buffName, "_Debuff") } else { buffName };
                            auraExports.set(key, tbl! { effectMult = mult, modList = obj(&newModList) });
                            // L2579
                            let allyBuffs = ally(&ctx.allyBuffs);
                            if allyBuffs.truthy(n!("AuraDebuff"))
                                && sub(&allyBuffs, "AuraDebuff").truthy(buffName)
                                && sub(&sub(&allyBuffs, "AuraDebuff"), &buffName).num(n!("effectMult")) / 100.0 > mult
                            {
                                // L2580
                                mult = 0.0;
                            }
                            let _ = mult;
                            // L2582
                            mergeBuff(&list_table(&newModList), &ctx.debuffs, buffName);
                        }
                    }
                }
            } else if is("Debuff") || is("AuraDebuff") {
                // L2586
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2587
                let mut stackCount: f64;
                // L2588
                if buff.truthy(n!("stackVar")) {
                    let stackVar = name_of(&buff.get(n!("stackVar")), "buff.stackVar");
                    // L2589
                    stackCount = skillModList.Sum(ModType::BASE, Some(&skillCfg), &[cat!("Multiplier:", stackVar)]);
                    // L2590
                    if buff.truthy(n!("stackLimit")) {
                        // L2591
                        stackCount = m_min(stackCount, buff.num(n!("stackLimit")));
                    } else if buff.truthy(n!("stackLimitVar")) {
                        // L2592
                        let stackLimitVar = name_of(&buff.get(n!("stackLimitVar")), "buff.stackLimitVar");
                        // L2593
                        stackCount = m_min(
                            stackCount,
                            skillModList.Sum(ModType::BASE, Some(&skillCfg), &[cat!("Multiplier:", stackLimitVar)]),
                        );
                    }
                } else {
                    // L2596
                    stackCount = activeSkill.skillData.get().num_or(n!("stackCount"), 1.0);
                }
                // L2598
                if env.mode_effective.get() && stackCount > 0.0 {
                    // L2599
                    activeSkill.debuffSkill.set(true);
                    // L2600
                    enemyDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                    // L2601
                    modDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                    // L2602
                    let srcList = ModStore::ModList(None);
                    // L2603
                    let mut mult = 1.0;
                    // L2604
                    let mut extraAuraModList: Vec<ModRef> = Vec::new();
                    let buffMods = mods_of(&buff.get(n!("modList")));
                    // L2605
                    if is("AuraDebuff") {
                        // L2606-2618
                        extraAuraModList =
                            merge_extra_mods(modDB.List(Some(&skillCfg), nm!["ExtraAuraDebuffEffect"]), |_| {});
                        // L2619
                        mult = 0.0;
                        // L2620
                        if !modDB.Flag(None, nm!["SelfAurasOnlyAffectYou"]) {
                            // L2621
                            let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "DebuffEffect"]);
                            // L2622
                            let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "DebuffEffect"]);
                            // L2623
                            mult = (1.0 + inc / 100.0) * more;
                            // L2624
                            let mut newModList: Vec<ModRef> = Vec::new();
                            // L2625
                            for m in &buffMods {
                                // L2626
                                newModList.push(m.clone());
                            }
                            // L2628
                            for m in &extraAuraModList {
                                // L2629
                                newModList.push(m.clone());
                            }
                            // L2631 A full modlist causes issues with copy table for mine auras
                            // L2632 --local newModList = new("ModList"):ModList()
                            // L2633 --newModList:AddList(buff.modList)
                            // L2634 --newModList:AddList(extraAuraModList)
                            // L2635
                            let auraExports = sub(&ctx.buffExports, "Aura");
                            let key = if auraExports.truthy(buffName) { cat!(buffName, "_Debuff") } else { buffName };
                            auraExports.set(key, tbl! { effectMult = mult, modList = plain_table(&newModList) });
                            // L2636
                            let allyBuffs = ally(&ctx.allyBuffs);
                            if allyBuffs.truthy(n!("AuraDebuff"))
                                && sub(&allyBuffs, "AuraDebuff").truthy(buffName)
                                && sub(&sub(&allyBuffs, "AuraDebuff"), &buffName).num(n!("effectMult")) / 100.0 > mult
                            {
                                // L2637
                                mult = 0.0;
                            }
                        }
                    }
                    // L2641
                    if is("Debuff") {
                        // L2642
                        let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["DebuffEffect"]);
                        // L2643
                        let more = skillModList.More(Some(&skillCfg), nm!["DebuffEffect"]);
                        // L2644
                        mult = (1.0 + inc / 100.0) * more;
                    }
                    // L2646
                    srcList.ScaleAddList(&buffMods, mult * stackCount, false);
                    // L2647
                    srcList.ScaleAddList(&extraAuraModList, mult * stackCount, false);
                    // L2648
                    if activeSkill.skillData.get().truthy(n!("stackCount")) || buff.truthy(n!("stackVar")) {
                        // L2649
                        srcList.NewMod(cat!("Multiplier:", buffName, "Stack"), ModType::BASE, stackCount, &buffName, 0, 0, &[]);
                    }
                    // L2651
                    mergeBuff(&list_table(&srcList), &ctx.debuffs, buffName);
                }
            } else if is("Curse") || is("CurseBuff") {
                // L2653
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2654
                let mark = has_type(&activeSkill, SkillType::Mark);
                // L2655
                modDB.conditions.set(
                    cat!("SelfCast", ns(&buffName)),
                    !(has_type(&activeSkill, SkillType::Triggered) || has_type(&activeSkill, SkillType::Aura)),
                );
                // L2656
                let mut skipCurse = false;
                // L2657
                let configVar = cat!("balanceOfTerrorSelfCast", ns(&buffName));
                // L2658
                if env.configInput.truthy(configVar) && !mark {
                    // L2659 User flagged this curse as Balance of Terror self-only, so don't apply it to enemies.
                    // L2660
                    skipCurse = true;
                }
                // L2662
                let skillData = activeSkill.skillData.get();
                if !skipCurse
                    && ((env.mode_effective.get()
                        && (!enemyDB.Flag(None, nm!["Hexproof"])
                            || modDB.Flag(None, nm!["CursesIgnoreHexproof"])
                            || skillData.truthy(n!("ignoreHexLimit"))
                            || skillData.truthy(n!("ignoreHexproof"))))
                        || mark)
                {
                    // L2663-2670
                    let priority = determineCursePriority(buffName, Some(&activeSkill));
                    let ignoreHexLimit = (modDB.Flag(Some(&activeSkill.skillCfg.get()), nm!["CursesIgnoreHexLimit"])
                        || skillData.truthy(n!("ignoreHexLimit")))
                        && !mark;
                    let socketedCursesHexLimit =
                        flag_val(modDB.Flag(Some(&activeSkill.skillCfg.get()), nm!["SocketedCursesAdditionalLimit"]));
                    let curse = tbl! {
                        name = buffName,
                        fromPlayer = true,
                        priority = priority,
                        isMark = flag_val(mark),
                        ignoreHexLimit = ignoreHexLimit,
                        socketedCursesHexLimit = socketedCursesHexLimit,
                    };
                    // L2671
                    let mut inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["CurseEffect"])
                        + enemyDB.Sum(ModType::INC, None, nm!["CurseEffectOnSelf"]);
                    // L2672
                    if has_type(&activeSkill, SkillType::Aura) && !has_type(&activeSkill, SkillType::RemoteMined) {
                        // L2673
                        inc += skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect"]);
                    }
                    // L2675
                    let mut more = skillModList.More(Some(&skillCfg), nm!["CurseEffect"]);
                    // L2676
                    let moreMark = more;
                    // L2677 This is non-ideal, but the only More for enemy is the boss effect
                    // L2678
                    if !curse.truthy(n!("isMark")) {
                        // L2679
                        more *= enemyDB.More(None, nm!["CurseEffectOnSelf"]);
                    }
                    // L2681
                    let mut mult = 0.0;
                    // L2682 If your aura only effect you blasphemy does nothing
                    if !((modDB.Flag(None, nm!["SelfAurasOnlyAffectYou"])
                        || activeSkill.skillModList.get().Flag(Some(&skillCfg), nm!["SelfAurasAffectYouAndLinkedTarget"]))
                        && has_type(&activeSkill, SkillType::Aura))
                    {
                        // L2683
                        mult = (1.0 + inc / 100.0) * more;
                    }
                    let buffMods = mods_of(&buff.get(n!("modList")));
                    // L2685
                    if is("Curse") {
                        // L2686
                        let modList = ModStore::ModList(None);
                        curse.set(n!("modList"), obj(&modList));
                        // L2687
                        modList.ScaleAddList(&buffMods, mult, false);
                        // L2688
                        if partyTabEnableExportBuffs {
                            // L2689
                            let isMark = curse.get(n!("isMark"));
                            let effectMult = if isMark.truthy() { mult } else { (1.0 + inc / 100.0) * moreMark };
                            sub(&ctx.buffExports, "Curse").set(
                                buffName,
                                tbl! { isMark = isMark, effectMult = effectMult, modList = buff.get(n!("modList")) },
                            );
                        }
                    } else {
                        // L2692 Curse applies a buff; scale by curse effect, then buff effect
                        // L2693
                        let temp = ModStore::ModList(None);
                        // L2694
                        temp.ScaleAddList(&buffMods, mult, false);
                        // L2695
                        let buffModList = ModStore::ModList(None);
                        curse.set(n!("buffModList"), obj(&buffModList));
                        // L2696
                        let buffInc = modDB.Sum(ModType::INC, Some(&skillCfg), nm!["BuffEffectOnSelf"]);
                        // L2697
                        let buffMore = modDB.More(Some(&skillCfg), nm!["BuffEffectOnSelf"]);
                        // L2698
                        buffModList.ScaleAddList(&temp.ipairs(), (1.0 + buffInc / 100.0) * buffMore, false);
                        // L2699
                        if let Some(minion) = env.minion.get() {
                            // L2700
                            let minionBuffModList = ModStore::ModList(None);
                            curse.set(n!("minionBuffModList"), obj(&minionBuffModList));
                            // L2701
                            let buffInc = minion.modDB.get().Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]);
                            // L2702
                            let buffMore = minion.modDB.get().More(None, nm!["BuffEffectOnSelf"]);
                            // L2703
                            minionBuffModList.ScaleAddList(&temp.ipairs(), (1.0 + buffInc / 100.0) * buffMore, false);
                        }
                    }
                    // L2706
                    ctx.curses.push(curse);
                }
            } else if is("Link") {
                // L2708
                let buffName = name_of(&buff.get(n!("name")), "buff.name");
                // L2709-2710
                let minion = env.minion.get();
                let linksApplyToMinions = minion.is_some()
                    && modDB.Flag(None, nm!["Condition:CanLinkToMinions"])
                    && modDB.Flag(None, nm!["Condition:LinkedToMinion"])
                    && !minion.as_ref().is_some_and(|m| m.modDB.get().Flag(None, nm!["Condition:CannotBeDamaged"]))
                    && !minion.as_ref().is_some_and(|m| {
                        let mainSkill = m.mainSkill.get().unwrap_or_else(|| {
                            lua_error!("attempt to index field 'mainSkill' (a nil value)")
                        });
                        let summonSkill = mainSkill.summonSkill.get().unwrap_or_else(|| {
                            lua_error!("attempt to index field 'summonSkill' (a nil value)")
                        });
                        has_type(&summonSkill, SkillType::MinionsAreUndamagable)
                    });
                // L2711
                if env.mode_buffs.get()
                    && ctx.linkSkills.len() < 1
                    && (partyTabEnableExportBuffs || linksApplyToMinions)
                {
                    // L2712 Check for extra modifiers to apply to link skills
                    // L2713-2730
                    let mut nonUniqueFlasks = false;
                    let extraLinkModList =
                        merge_extra_mods(modDB.List(Some(&skillCfg), nm!["ExtraLinkEffect"]), |m| {
                            // L2725 special handling to add this early
                            // L2726
                            if m.name.as_str() == "ParentNonUniqueFlasksAppliedToYou" {
                                // L2727
                                nonUniqueFlasks = true;
                            }
                        });
                    if nonUniqueFlasks {
                        ctx.nonUniqueFlasksApplyToMinion = true;
                    }
                    let buffMods = mods_of(&buff.get(n!("modList")));
                    // L2731
                    let mut inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["LinkEffect", "BuffEffect"]);
                    // L2732
                    let mut more = skillModList.More(Some(&skillCfg), nm!["LinkEffect", "BuffEffect"]);
                    // L2733
                    let mut mult = (1.0 + inc / 100.0) * more;
                    // L2734
                    if partyTabEnableExportBuffs {
                        // L2735
                        let newModList = ModStore::ModList(None);
                        // L2736
                        newModList.AddList(&buffMods);
                        // L2737
                        newModList.AddList(&extraLinkModList);
                        // L2738
                        sub(&ctx.buffExports, "Link").set(buffName, tbl! { effectMult = mult, modList = obj(&newModList) });
                    }
                    // L2740
                    if linksApplyToMinions {
                        let minion = minion.expect("linksApplyToMinions needs env.minion");
                        let minionDB = minion.modDB.get();
                        // L2741
                        activeSkill.minionBuffSkill.set(true);
                        // L2742
                        minionDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                        // L2743
                        minionDB.conditions.set(n!("AffectedByLink"), true);
                        // L2744
                        let srcList = ModStore::ModList(None);
                        // L2745
                        inc += minionDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf", "LinkEffectOnSelf"]);
                        // L2746
                        more *= minionDB.More(None, nm!["BuffEffectOnSelf", "LinkEffectOnSelf"]);
                        // L2747
                        mult = (1.0 + inc / 100.0) * more;
                        // L2748
                        srcList.ScaleAddList(&buffMods, mult, false);
                        // L2749
                        srcList.ScaleAddList(&extraLinkModList, mult, false);
                        // L2750
                        mergeBuff(&list_table(&srcList), &ctx.minionBuffs, buffName);
                        // L2751
                        ctx.linkSkills.set(1, Val::Str(LStr::from(buffName.as_str())));
                    }
                }
            }
        });
        // L2756
        let minionCanWither = || {
            activeSkill.minion.get().is_some()
                && env.minion.get().is_some_and(|m| m.modDB.get().Flag(None, nm!["Condition:CanWither"]))
        };
        if activeSkill.skillModList.get().Flag(None, nm!["Condition:CanWither"]) || minionCanWither() {
            // L2757
            let mut witherEffect = modDB.Sum(
                ModType::INC,
                None,
                &[if activeSkill.minion.get().is_some() { n!("MinionWitherEffect") } else { n!("WitherEffect") }],
            );
            // L2758
            if activeSkill.minion.get().is_none() && modDB.Flag(None, nm!["UnholyMight"]) {
                // L2759
                witherEffect += modDB.Sum(ModType::INC, None, nm!["UnholyMightAlsoWitherEffect"]);
            }
            // L2761
            let effect = m_floor(6.0 * (1.0 + witherEffect / 100.0));
            // L2762
            modDB.NewMod(n!("WitherEffectStack"), ModType::MAX, effect, None, 0, 0, &[]);
        }
        // L2764 Handle combustion
        // L2765
        if enemyDB.Flag(None, nm!["Condition:Ignited"])
            && (has_type(&activeSkill, SkillType::Damage) || has_type(&activeSkill, SkillType::Attack))
            && !ctx.appliedCombustion
        {
            // L2766
            for_ipairs!((_, support) in activeSkill.supportList.get() => {
                // L2767
                if support.grantedEffect.get().name.as_str() == "Combustion" {
                    // L2768
                    if !activeSkill.skillModList.get().Flag(Some(&activeSkill.skillCfg.get()), nm!["CannotIgnite"]) {
                        // L2769
                        let value = activeSkill.skillModList.get().Sum(
                            ModType::BASE,
                            Some(&activeSkill.skillCfg.get()),
                            nm!["CombustionFireResist"],
                        );
                        // L2770
                        enemyDB.NewMod(
                            n!("FireResist"),
                            ModType::BASE,
                            value,
                            "Combustion",
                            0,
                            0,
                            &[
                                tag!({ type = GlobalEffect, effectType = "Debuff", effectName = "Combustion" }),
                                tag!({ type = Condition, var = "Ignited" }),
                            ],
                        );
                        // L2771
                        ctx.appliedCombustion = true;
                    }
                    // L2773
                    break;
                }
            });
        }
        // L2777
        if let Some(castingMinion) = activeSkill.minion.get() {
            // L2778 (`activeSkill.minion.activeSkillList`: an actor's list is never nil here)
            // L2779
            for_ipairs!((_, activeMinionSkill) in castingMinion.activeSkillList.get() => {
                // L2780-2792: `local function setSpectreSource` (see [`setSpectreSource`])
                // L2793
                let skillModList = activeMinionSkill.skillModList.get();
                // L2794
                let skillCfg = activeMinionSkill.skillCfg.get();
                // L2795
                for_ipairs!((_, buff) in activeMinionSkill.buffList.get() => {
                    let btype = buff.str(n!("type"));
                    let is = |t: &str| btype.as_deref() == Some(t);
                    // L2796
                    if is("Buff") {
                        let buffName = name_of(&buff.get(n!("name")), "buff.name");
                        // L2797
                        if env.mode_buffs.get() && activeMinionSkill.skillData.get().truthy(n!("enable")) {
                            // L2798
                            let skillCfg = if buff.truthy(n!("activeSkillBuff")) { Some(skillCfg.clone()) } else { None };
                            let skillCfg = skillCfg.as_deref();
                            // L2799
                            let modStore = if buff.truthy(n!("activeSkillBuff")) {
                                skillModList.clone()
                            } else {
                                castingMinion.modDB.get()
                            };
                            let buffMods = mods_of(&buff.get(n!("modList")));
                            // L2800
                            if buff.truthy(n!("applyAllies")) {
                                // L2801
                                activeMinionSkill.buffSkill.set(true);
                                // L2802
                                modDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                                // L2803
                                let srcList = ModStore::ModList(None);
                                // L2804
                                let inc = modStore.Sum(ModType::INC, skillCfg, nm!["BuffEffect", "BuffEffectOnPlayer"])
                                    + modDB.Sum(ModType::INC, None, nm!["BuffEffectOnSelf"]);
                                // L2805
                                let more = modStore.More(skillCfg, nm!["BuffEffect", "BuffEffectOnPlayer"])
                                    * modDB.More(None, nm!["BuffEffectOnSelf"]);
                                // L2806
                                srcList.ScaleAddList(&buffMods, (1.0 + inc / 100.0) * more, false);
                                // L2807
                                mergeBuff(&list_table(&srcList), &ctx.buffs, buffName);
                                // L2808
                                mergeBuff(&buff_mod_list(&buff), &ctx.buffs, buffName);
                                // L2809
                                if activeMinionSkill.skillData.get().truthy(n!("thisIsNotABuff")) {
                                    // L2810
                                    set_notBuff(&ctx.buffs, buffName);
                                }
                                // L2812
                                if partyTabEnableExportBuffs {
                                    // L2813
                                    let inc = modStore.Sum(ModType::INC, skillCfg, nm!["BuffEffect"]);
                                    // L2814
                                    let more = modStore.More(skillCfg, nm!["BuffEffect"]);
                                    // L2815
                                    let aura = sub(&ctx.buffExports, "Aura");
                                    let other = aura.get(n!("otherEffects"));
                                    let other = if other.truthy() { other } else { Val::Table(Table::new()) };
                                    aura.set(n!("otherEffects"), other);
                                    // L2816
                                    let effectMult = (1.0 + inc / 100.0) * more;
                                    sub(&aura, "otherEffects").set(
                                        buffName,
                                        tbl! { effectMult = effectMult, modList = buff.get(n!("modList")) },
                                    );
                                }
                            }
                            // L2819
                            let envMinion = env.minion.get();
                            let isCasting = envMinion.as_ref().is_some_and(|m| Rc::ptr_eq(m, &castingMinion));
                            let envMinionCheck = envMinion.is_some() && (isCasting || buff.truthy(n!("applyAllies")));
                            // L2820
                            if buff.truthy(n!("applyMinions")) || envMinionCheck {
                                // L2821
                                activeMinionSkill.minionBuffSkill.set(true);
                                // L2822
                                if envMinionCheck {
                                    // L2823
                                    envMinion
                                        .as_ref()
                                        .expect("envMinionCheck needs env.minion")
                                        .modDB
                                        .get()
                                        .conditions
                                        .set(cat!("AffectedBy", ns(&buffName)), true);
                                } else {
                                    // L2825
                                    castingMinion.modDB.get().conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                                }
                                // L2827
                                let srcList = ModStore::ModList(None);
                                // L2828
                                let inc = modStore.Sum(
                                    ModType::INC,
                                    skillCfg,
                                    &names([Some(n!("BuffEffect")), isCasting.then(|| n!("BuffEffectOnSelf"))]),
                                );
                                // L2829
                                let more = modStore.More(
                                    skillCfg,
                                    &names([Some(n!("BuffEffect")), isCasting.then(|| n!("BuffEffectOnSelf"))]),
                                );
                                // L2830
                                srcList.ScaleAddList(&buffMods, (1.0 + inc / 100.0) * more, false);
                                // L2831
                                mergeBuff(&list_table(&srcList), &ctx.minionBuffs, buffName);
                                // L2832
                                mergeBuff(&buff_mod_list(&buff), &ctx.minionBuffs, buffName);
                                // L2833
                                if activeMinionSkill.skillData.get().truthy(n!("thisIsNotABuff")) {
                                    // L2834 (`buffs`, not `minionBuffs`, as in Lua)
                                    set_notBuff(&ctx.buffs, buffName);
                                }
                            }
                        }
                    } else if is("Aura") {
                        // L2838
                        let buffName = name_of(&buff.get(n!("name")), "buff.name");
                        // L2839
                        if env.mode_buffs.get() && activeMinionSkill.skillData.get().truthy(n!("enable")) {
                            // L2840 Check for extra modifiers to apply to aura skills
                            // L2841-2854
                            let mut extraAuraModList = merge_extra_mods(
                                castingMinion.modDB.get().List(Some(&skillCfg), nm!["ExtraAuraEffect"]),
                                |_| {},
                            );
                            let buffModList = buff_mod_list(&buff);
                            let castingDB = castingMinion.modDB.get();
                            // L2855
                            if !(castingDB.Flag(None, nm!["SelfAurasCannotAffectAllies"])
                                || castingDB.Flag(None, nm!["SelfAurasOnlyAffectYou"])
                                || castingDB.Flag(None, nm!["SelfAuraSkillsCannotAffectAllies"])
                                || skillModList.Flag(Some(&skillCfg), nm!["SelfAurasAffectYouAndLinkedTarget"]))
                            {
                                // L2856
                                if !modDB.Flag(None, nm!["AlliesAurasCannotAffectSelf"])
                                    && !modDB.conditions.truthy(cat!("AffectedBy", ns(&buffName)))
                                {
                                    // L2857
                                    let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "BuffEffectOnPlayer", "AuraBuffEffect"])
                                        + modDB.Sum(ModType::INC, Some(&skillCfg), nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                                    // L2858
                                    let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "AuraBuffEffect"])
                                        * modDB.More(Some(&skillCfg), nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                                    // L2859
                                    let mult = (1.0 + inc / 100.0) * more;
                                    // L2860
                                    if !ally(&ctx.allyBuffs).truthy(n!("Aura"))
                                        || !sub(&ally(&ctx.allyBuffs), "Aura").truthy(buffName)
                                        || sub(&sub(&ally(&ctx.allyBuffs), "Aura"), &buffName).num(n!("effectMult")) / 100.0 <= mult
                                    {
                                        // L2861
                                        activeMinionSkill.buffSkill.set(true);
                                        // L2862
                                        modDB.conditions.set(n!("AffectedByAura"), true);
                                        // L2863
                                        if pob_lua::string::sub(&buffName, 1, Some(4)) == "Vaal" {
                                            // L2864
                                            modDB.conditions.set(
                                                cat!("AffectedBy", ns(&pob_lua::string::sub(&buffName, 6, None))),
                                                true,
                                            );
                                        }
                                        // L2866
                                        modDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                                        // L2867
                                        let srcList = ModStore::ModList(None);
                                        // L2868
                                        srcList.ScaleAddList(&mods_of(&Val::Table(buffModList.clone())), mult, false);
                                        // L2869
                                        srcList.ScaleAddList(&extraAuraModList, mult, false);
                                        // L2870
                                        setSpectreSource(&activeSkill, &castingMinion, &srcList, Some(buffName), &buffModList, &mut extraAuraModList);
                                        // L2871
                                        mergeBuff(&list_table(&srcList), &ctx.buffs, buffName);
                                    }
                                }
                                // L2874
                                let envMinion = env.minion.get();
                                if let Some(envMinion) = envMinion.as_ref().filter(|m| {
                                    !m.modDB.get().conditions.truthy(cat!("AffectedBy", ns(&buffName)))
                                        && (!same_actor(&Some((*m).clone()), &activeSkill.minion.get())
                                            || !activeSkill.skillData.get().truthy(n!("auraCannotAffectSelf")))
                                }) {
                                    let minionDB = envMinion.modDB.get();
                                    // L2875
                                    let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect"])
                                        + minionDB.Sum(ModType::INC, Some(&skillCfg), nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                                    // L2876
                                    let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect"])
                                        * minionDB.More(Some(&skillCfg), nm!["BuffEffectOnSelf", "AuraEffectOnSelf"]);
                                    // L2877
                                    let mult = (1.0 + inc / 100.0) * more;
                                    // L2878
                                    if !ally(&ctx.allyBuffs).truthy(n!("Aura"))
                                        || !sub(&ally(&ctx.allyBuffs), "Aura").truthy(buffName)
                                        || sub(&sub(&ally(&ctx.allyBuffs), "Aura"), &buffName).num(n!("effectMult")) / 100.0 <= mult
                                    {
                                        // L2879
                                        activeMinionSkill.minionBuffSkill.set(true);
                                        // L2880
                                        minionDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                                        // L2881
                                        minionDB.conditions.set(n!("AffectedByAura"), true);
                                        // L2882
                                        let srcList = ModStore::ModList(None);
                                        // L2883
                                        srcList.ScaleAddList(&mods_of(&Val::Table(buffModList.clone())), mult, false);
                                        // L2884
                                        srcList.ScaleAddList(&extraAuraModList, mult, false);
                                        // L2885
                                        setSpectreSource(&activeSkill, &castingMinion, &srcList, Some(buffName), &buffModList, &mut extraAuraModList);
                                        // L2886
                                        mergeBuff(&list_table(&srcList), &ctx.minionBuffs, buffName);
                                    }
                                }
                                // L2889
                                let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect"]);
                                // L2890
                                let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect"]);
                                // L2891
                                let mult = (1.0 + inc / 100.0) * more;
                                // L2892
                                let newModList = ModStore::ModList(None);
                                // L2893
                                newModList.AddList(&mods_of(&Val::Table(buffModList.clone())));
                                // L2894
                                newModList.AddList(&extraAuraModList);
                                // L2895
                                setSpectreSource(&activeSkill, &castingMinion, &newModList, Some(buffName), &buffModList, &mut extraAuraModList);
                                let auraExports = sub(&ctx.buffExports, "Aura");
                                // L2896
                                if auraExports.truthy(buffName) {
                                    // L2897
                                    auraExports.set(cat!(buffName, "_Debuff"), auraExports.get(buffName));
                                }
                                // L2899
                                auraExports.set(buffName, tbl! { effectMult = mult, modList = obj(&newModList) });
                                // L2900
                                let mainSkill = main_skill(&env);
                                let mainSkillModList = mainSkill.skillModList.get();
                                if mainSkill.skillFlags.get().truthy(n!("totem"))
                                    && !mainSkillModList.conditions.truthy(cat!("AffectedBy", ns(&buffName)))
                                {
                                    // L2901
                                    activeMinionSkill.totemBuffSkill.set(true);
                                    // L2902
                                    mainSkillModList.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                                    // L2903
                                    mainSkillModList.conditions.set(n!("AffectedByAura"), true);
                                    // L2905
                                    let srcList = ModStore::ModList(None);
                                    // L2906
                                    let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "AuraBuffEffect"]);
                                    // L2907
                                    let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "AuraBuffEffect"]);
                                    // L2908 (lists = {extraAuraModList, buff.modList})
                                    // L2909
                                    let mut scale = (1.0 + inc / 100.0) * more;
                                    // L2910
                                    scale = m_max(scale, 0.0);
                                    // L2912-2927
                                    let buffMods = mods_of(&Val::Table(buffModList.clone()));
                                    add_totem_mods(&srcList, [&extraAuraModList, &buffMods], scale);
                                    // L2928
                                    setSpectreSource(&activeSkill, &castingMinion, &srcList, None, &buffModList, &mut extraAuraModList);
                                    // L2929
                                    mergeBuff(&list_table(&srcList), &ctx.buffs, cat!("Totem ", buffName));
                                }
                            }
                        }
                    } else if is("Curse") {
                        // L2933
                        let buffName = name_of(&buff.get(n!("name")), "buff.name");
                        // L2934
                        if env.mode_effective.get()
                            && activeMinionSkill.skillData.get().truthy(n!("enable"))
                            && (!enemyDB.Flag(None, nm!["Hexproof"]) || has_type(&activeMinionSkill, SkillType::Mark))
                        {
                            // L2935-2938
                            let priority = determineCursePriority(buffName, Some(&activeMinionSkill));
                            let curse = tbl! { name = buffName, priority = priority };
                            // L2939
                            let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["CurseEffect"])
                                + enemyDB.Sum(ModType::INC, None, nm!["CurseEffectOnSelf"]);
                            // L2940
                            let more = skillModList.More(Some(&skillCfg), nm!["CurseEffect"])
                                * enemyDB.More(None, nm!["CurseEffectOnSelf"]);
                            // L2941
                            let modList = ModStore::ModList(None);
                            curse.set(n!("modList"), obj(&modList));
                            // L2942
                            modList.ScaleAddList(&mods_of(&buff.get(n!("modList"))), (1.0 + inc / 100.0) * more, false);
                            // L2943
                            ctx.minionCurses.push(curse);
                        }
                    } else if is("Debuff") || is("AuraDebuff") {
                        // L2945
                        let buffName = name_of(&buff.get(n!("name")), "buff.name");
                        // L2946
                        let mut stackCount: f64;
                        // L2947
                        if buff.truthy(n!("stackVar")) {
                            let stackVar = name_of(&buff.get(n!("stackVar")), "buff.stackVar");
                            // L2948
                            stackCount = skillModList.Sum(ModType::BASE, Some(&skillCfg), &[cat!("Multiplier:", stackVar)]);
                            // L2949
                            if buff.truthy(n!("stackLimit")) {
                                // L2950
                                stackCount = m_min(stackCount, buff.num(n!("stackLimit")));
                            } else if buff.truthy(n!("stackLimitVar")) {
                                // L2951
                                let stackLimitVar = name_of(&buff.get(n!("stackLimitVar")), "buff.stackLimitVar");
                                // L2952
                                stackCount = m_min(
                                    stackCount,
                                    skillModList.Sum(ModType::BASE, Some(&skillCfg), &[cat!("Multiplier:", stackLimitVar)]),
                                );
                            }
                        } else {
                            // L2955
                            stackCount = activeMinionSkill.skillData.get().num_or(n!("stackCount"), 1.0);
                        }
                        // L2957
                        if env.mode_effective.get() && stackCount > 0.0 {
                            // L2958
                            activeMinionSkill.debuffSkill.set(true);
                            // L2959
                            let srcList = ModStore::ModList(None);
                            // L2960
                            let mut mult = 1.0;
                            // L2961
                            if is("AuraDebuff") {
                                // L2962
                                mult = 0.0;
                                // L2963
                                if !skillModList.Flag(None, nm!["SelfAurasOnlyAffectYou"])
                                    || skillModList.Flag(Some(&skillCfg), nm!["SelfAurasAffectYouAndLinkedTarget"])
                                {
                                    // L2964
                                    let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "DebuffEffect"]);
                                    // L2965
                                    let more = skillModList.More(Some(&skillCfg), nm!["AuraEffect", "BuffEffect", "DebuffEffect"]);
                                    // L2966
                                    mult = (1.0 + inc / 100.0) * more;
                                    // L2967
                                    if !enemyDB.conditions.truthy(cat!("AffectedBy", ns(&buffName))) {
                                        // L2968
                                        let auraExports = sub(&ctx.buffExports, "Aura");
                                        let key = if auraExports.truthy(buffName) { cat!(buffName, "_Debuff") } else { buffName };
                                        auraExports.set(key, tbl! { effectMult = mult, modList = buff.get(n!("modList")) });
                                        // L2969
                                        let allyBuffs = ally(&ctx.allyBuffs);
                                        if allyBuffs.truthy(n!("AuraDebuff"))
                                            && sub(&allyBuffs, "AuraDebuff").truthy(buffName)
                                            && sub(&sub(&allyBuffs, "AuraDebuff"), &buffName).num(n!("effectMult")) / 100.0 > mult
                                        {
                                            // L2970
                                            mult = 0.0;
                                        }
                                    } else {
                                        // L2973
                                        mult = 0.0;
                                    }
                                }
                            }
                            // L2977
                            enemyDB.conditions.set(cat!("AffectedBy", ns(&buffName)), true);
                            // L2978
                            if env.minion.get().is_some() && same_actor(&env.minion.get(), &activeSkill.minion.get()) {
                                // L2979
                                env.minion
                                    .get()
                                    .expect("checked above")
                                    .modDB
                                    .get()
                                    .conditions
                                    .set(cat!("AffectedBy", ns(&buffName)), true);
                            }
                            // L2981
                            if is("Debuff") {
                                // L2982
                                let inc = skillModList.Sum(ModType::INC, Some(&skillCfg), nm!["DebuffEffect"]);
                                // L2983
                                let more = skillModList.More(Some(&skillCfg), nm!["DebuffEffect"]);
                                // L2984
                                mult = (1.0 + inc / 100.0) * more;
                            }
                            // L2986
                            srcList.ScaleAddList(&mods_of(&buff.get(n!("modList"))), mult * stackCount, false);
                            // L2987
                            if activeMinionSkill.skillData.get().truthy(n!("stackCount")) || buff.truthy(n!("stackVar")) {
                                // L2988
                                srcList.NewMod(
                                    cat!("Multiplier:", buffName, "Stack"),
                                    ModType::BASE,
                                    to_value(&activeMinionSkill.skillData.get().get(n!("stackCount"))),
                                    &buffName,
                                    0,
                                    0,
                                    &[],
                                );
                            }
                            // L2990
                            mergeBuff(&list_table(&srcList), &ctx.debuffs, buffName);
                        }
                    }
                });
            });
        }
    });
    Flow::Normal
}
