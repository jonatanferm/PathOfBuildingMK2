//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! CalcSetup.lua 305-474: wipeEnv and the support helpers.
//!
//! Owner: CS0. Signatures of the three file-local helpers follow the Lua arguments (CCR-0074):
//! `applyGemMods` takes Tabulate rows, `applySocketMods` the gem's data (`gemInstance.gemData`).
#![allow(non_snake_case)]

use pob_gamedata::mod_tools as modLib;
use pob_gamedata::GemData;

use crate::modules::calc_tools as calcLib;
use crate::prelude::*;

/// @pob Modules/CalcSetup.lua:306-380 h=67ad3f26
/// Global `wipeEnv(env, accelerate)`.
pub fn wipeEnv(env: &EnvRef, accelerate: &Accelerate) {
    // Always wipe the below as we will be pushing in the modifiers,
    // multipliers and conditions for player and enemy DBs via `parent`
    // extensions of those DBs later which allow us to do a table-pointer
    // link and save time on having to do a copyTable() function.
    // L311 (`wipeTable(modDB.mods)`: a ModDB's wipe clears only its mods)
    env.modDB.get().wipe();
    // L312
    env.modDB.get().conditions.wipe();
    // L313
    env.modDB.get().multipliers.wipe();
    // L314
    env.enemyDB.get().wipe();
    // L315
    env.enemyDB.get().conditions.wipe();
    // L316
    env.enemyDB.get().multipliers.wipe();
    // L317
    if let Some(minion) = env.minion.get() {
        // L318
        minion.modDB.get().wipe();
        // L319
        minion.modDB.get().conditions.wipe();
        // L320
        minion.modDB.get().multipliers.wipe();
    }

    // L323
    if accelerate.everything {
        // L324
        return;
    }

    // Passive tree node allocations
    // Also in a further pass tracks Legion influenced mods
    // L329
    if !accelerate.nodeAlloc {
        // L330
        env.allocNodes.get().with_mut(|m| m.wipe());
        // Usually states: `Allocates <NAME>` (e.g., amulet anointment)
        // L332
        env.grantedPassives.get().with_mut(|m| m.wipe());
        // L333
        env.grantedSkillsNodes.get().wipe();
    }

    // L336
    if !accelerate.requirementsItems {
        // Item-related tables
        // L338
        env.itemModDB.get().wipe();
        // L339
        env.itemModDB.get().conditions.wipe();
        // L340
        env.itemModDB.get().multipliers.wipe();
        // 1) Jewels and Jewel-Radius related node modifications
        // 2) Player items
        // 3) Granted Skill from items (e.g., Curse on Hit rings)
        // 4) Flasks and Tinctures
        // L345
        env.radiusJewelList.get().wipe();
        // L346
        env.extraRadiusNodeList.get().with_mut(|m| m.wipe());
        // L347
        env.player.itemList.with_mut(|m| m.clear());
        // L348
        env.grantedSkillsItems.get().wipe();
        // L349
        env.flasks.get().with_mut(|m| m.wipe());
        // L350
        env.tinctures.get().with_mut(|m| m.wipe());

        // Special / Unique Items that have their own ModDB()
        // L353
        if let Some(l) = env.aegisModList.get() {
            // L354
            l.wipe();
        }
        // L356
        if let Some(l) = env.theIronMass.get() {
            // L357
            l.wipe();
        }
        // L359
        if let Some(l) = env.weaponModList1.get() {
            // L360
            l.wipe();
        }

        // Requirements from Items (Str, Dex, Int)
        // L364
        env.requirementsTableItems.get().wipe();
    }

    // Requirements from Gems (Str, Dex, Int)
    // L368
    if !accelerate.requirementsGems {
        // L369
        env.requirementsTableGems.get().wipe();
    }

    // L372
    if !accelerate.skills {
        // Player Active Skills generation
        // L374
        env.player.activeSkillList.get().wipe();

        // Enhances Active Skills with skill ModFlags, KeywordFlags
        // and modifiers that affect skill scaling (e.g., global buffs/effects)
        // L378
        env.auxSkillList.get().wipe();
    }
}

// ---- Rust-only plumbing ---------------------------------------------------------------------

/// `calcLib.gemIsType(gem, type, true)` for `effect.gemData` (nil when the effect has no gem) and
/// a keyword value. `type == "all"` is tested before `gem` is indexed (CalcTools.lua:165), so a
/// nil gem only raises for other keywords, as in Lua. A non-string keyword (e.g. nil) matches no
/// clause of CalcTools.lua:165-175: every `type == "<s>"` test and `type == gem.name:lower()` are
/// false, `isGemIdSame` returns nil for a non-string (CalcTools.lua:280-281) and `gem.tags[type]`
/// reads nil, so the result is falsy once `gem.name` (L172) has been indexed.
fn gemIsTypeVal(gem: Option<&GemData>, r#type: &Val) -> bool {
    if matches!(r#type, Val::Str(t) if &**t == "all") {
        return true;
    }
    let Some(g) = gem else {
        lua_error!("attempt to index local 'gem' (a nil value)")
    };
    match r#type {
        Val::Str(t) => calcLib::gemIsType(g, t, true),
        _ => false,
    }
}

/// Lua `a + b` operand (numbers; numeric strings coerce).
fn arith(v: &Val, what: &str) -> f64 {
    match v {
        Val::Num(x) => *x,
        Val::Str(s) => match pob_lua::tonumber(s) {
            Some(x) => x,
            None => lua_error!("attempt to perform arithmetic on {} (a string value)", what),
        },
        other => lua_error!(
            "attempt to perform arithmetic on {} (a {} value)",
            what,
            other.lua_type()
        ),
    }
}

/// `effect[key]` as a number or nil (the typed numeric fields, else `effect.extra`).
fn effect_num(effect: &SkillEffect, key: &str) -> Option<f64> {
    match key {
        "level" => Some(effect.level.get()),
        "quality" => Some(effect.quality.get()),
        "globalQuality" => effect.globalQuality.get(),
        "itemQuality" => effect.itemQuality.get(),
        "supportQuality" => effect.supportQuality.get(),
        "actorLevel" => effect.actorLevel.get(),
        // CCR-0074: `effect.req` is a number here (GemProperty `key = "req"`, ModParser.lua
        // 2374-2375; read as `gemInstance.reqOverride = effect.req`, CalcSetup.lua:1705/1776),
        // which the frozen `req: Field<Option<Table>>` cannot hold: it lives in `extra`.
        _ => match effect.extra.get(key) {
            Val::Nil => None,
            other => Some(arith(&other, "field (effect[value.key])")),
        },
    }
}

/// `effect[key] = v`.
fn set_effect_num(effect: &SkillEffect, key: &str, v: f64) {
    match key {
        "level" => effect.level.set(v),
        "quality" => effect.quality.set(v),
        "globalQuality" => effect.globalQuality.set(Some(v)),
        "itemQuality" => effect.itemQuality.set(Some(v)),
        "supportQuality" => effect.supportQuality.set(Some(v)),
        "actorLevel" => effect.actorLevel.set(Some(v)),
        // CCR-0074 (`req`, see effect_num)
        _ => effect.extra.set(key, v),
    }
}

// ---- CalcSetup.lua 382-457 --------------------------------------------------------------------

/// @pob Modules/CalcSetup.lua:382-417 h=6cf90c88
/// `applyGemMods(effect, modList)`: `modList` holds the GemProperty Tabulate rows
/// (`{ value = …, mod = … }`, CCR-0074).
pub fn applyGemMods(effect: &SkillEffectRef, modList: &[TabRow]) {
    // L383
    for r#mod in modList {
        // L384
        let mut r#match = true;
        // L385
        let value = Val::from_value(&r#mod.value);
        // L386
        let keywordList = value.get(n!("keywordList"));
        if keywordList.truthy() {
            // L387
            let mut i = 1i64;
            loop {
                let keyword = keywordList.get(i);
                if keyword.is_nil() {
                    break;
                }
                // L388
                if !gemIsTypeVal(effect.gemData.get().as_deref(), &keyword) {
                    // L389
                    r#match = false;
                    // L390
                    break;
                }
                i += 1;
            }
        } else if !gemIsTypeVal(effect.gemData.get().as_deref(), &value.get(n!("keyword"))) {
            // L394
            r#match = false;
        }
        // L396
        if r#match {
            // save quality increases for use in tooltips
            let key = value.get(n!("key"));
            // L398
            if is_quality(&key) {
                // L399
                let mut isSocketed = false;
                // L400
                for tag in r#mod.r#mod.tags.iter() {
                    // L401
                    if tag.r#type == TagType::SocketedIn {
                        // L402
                        isSocketed = true;
                        // L403
                        break;
                    }
                }
                // L406
                if isSocketed {
                    // L407
                    let v = effect.itemQuality.get().unwrap_or(0.0)
                        + arith(&value.get(n!("value")), "field 'value'");
                    effect.itemQuality.set(Some(v));
                } else {
                    // L409
                    let v = effect.globalQuality.get().unwrap_or(0.0)
                        + arith(&value.get(n!("value")), "field 'value'");
                    effect.globalQuality.set(Some(v));
                }
            }
            // L412
            let k = match &key {
                Val::Str(s) => s.clone(),
                Val::Nil => lua_error!("table index is nil"),
                other => lua_error!("effect[value.key]: key is a {} value", other.lua_type()),
            };
            let v = effect_num(effect, &k).unwrap_or(0.0)
                + arith(&value.get(n!("value")), "field 'value'");
            set_effect_num(effect, &k, v);
            // L413
            let gemPropertyInfo = match effect.gemPropertyInfo.get() {
                Some(t) => t,
                None => {
                    let t = Table::new();
                    effect.gemPropertyInfo.set(Some(t.clone()));
                    t
                }
            };
            // L414 (the row as a `{ value = …, mod = … }` table, CCR-0040 §6)
            let row = Table::new();
            row.set(n!("value"), value.clone());
            row.set(n!("mod"), Val::Mod(r#mod.r#mod.clone()));
            gemPropertyInfo.push(row);
        }
    }
}

/// `value.key == "quality"`.
fn is_quality(key: &Val) -> bool {
    matches!(key, Val::Str(s) if &**s == "quality")
}

/// @pob Modules/CalcSetup.lua:419-426 h=091f9eb8
/// `applySocketMods(env, gem, groupCfg, socketNum, modSource)`: `gem` is `gemInstance.gemData`
/// (CalcSetup.lua:1708/1779; CCR-0074).
pub fn applySocketMods(
    env: &EnvRef,
    gem: &GemDataRef,
    groupCfg: &Cfg,
    socketNum: f64,
    modSource: Option<&str>,
) {
    // L420
    let socketCfg = groupCfg.copyTable(true);
    // L421
    socketCfg.skillGem.set(Some(gem.clone()));
    // L422
    socketCfg.socketNum.set(Some(socketNum));
    // L423
    for value in env
        .modDB
        .get()
        .List(Some(&*socketCfg), nm!["SocketProperty"])
    {
        // L424
        let inner = match Val::from_value(&value).get(n!("value")) {
            Val::Mod(m) => m,
            other => lua_error!(
                "SocketProperty value: value is a {} value",
                other.lua_type()
            ),
        };
        let slotName = groupCfg.slotName.get();
        let source: &str = match modSource {
            Some(s) => s,
            None => slotName.as_ref().map_or("", |s| s.as_str()),
        };
        // LUA-ALIAS: Lua's setSource rewrites the shared SocketProperty mod in place
        // (ModTools.lua:239-246), so the stored value's mod also carries the new source; the
        // port adds a re-sourced copy (§3.6, CCR-0074).
        env.player
            .modDB
            .get()
            .AddMod(Arc::new(modLib::setSource(&inner, source)));
    }
}

/// @pob Modules/CalcSetup.lua:428-457 h=40b82627
pub fn addBestSupport(
    supportEffect: &SkillEffectRef,
    appliedSupportList: &Shared<Vec<SkillEffectRef>>,
    mode: Mode,
) {
    // L429
    let mut add = true;
    // L430 (ipairs: reads `appliedSupportList[index]` live, stops at the first nil)
    let mut index = 1usize;
    while let Some(otherSupport) = appliedSupportList.get(index) {
        let ge = supportEffect.grantedEffect.get();
        let otherGe = otherSupport.grantedEffect.get();
        // Check if there's another better support already present
        // L432
        if Arc::ptr_eq(&ge, &otherGe) {
            // L433
            add = false;
            // L434
            if supportEffect.level.get() > otherSupport.level.get()
                || (supportEffect.level.get() == otherSupport.level.get()
                    && supportEffect.quality.get() > otherSupport.quality.get())
            {
                // L435
                if mode == Mode::MAIN {
                    // L436
                    otherSupport.superseded.set(true);
                }
                // L438
                appliedSupportList.set(index, supportEffect.clone());
            } else {
                // L440
                supportEffect.superseded.set(true);
            }
            // L442
            break;
        } else if ge.plusVersionOf.is_some_and(|p| p == otherGe.id) {
            // L444
            add = false;
            // L445
            if mode == Mode::MAIN {
                // L446
                otherSupport.superseded.set(true);
            }
            // L448
            appliedSupportList.set(index, supportEffect.clone());
        } else if otherGe.plusVersionOf.is_some_and(|p| p == ge.id) {
            // L450
            add = false;
            // L451
            supportEffect.superseded.set(true);
        }
        index += 1;
    }
    // L454
    if add {
        // L455
        appliedSupportList.push(supportEffect.clone());
    }
}
