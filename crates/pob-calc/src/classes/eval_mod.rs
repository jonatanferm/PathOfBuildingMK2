//! @pob-file Classes/ModStore.lua @16de4b82d
//! ModStore:EvalMod and getActor (Classes/ModStore.lua 48-54, 363-971; calc doc §5.3).
//! EvalMod MUST NOT mutate a tag; it returns a new Value wherever Lua copies and edits.
//!
//! Owner: MS-B.
//!
//! Notes on the translation (calc doc §3):
//! * `tag.div = GetMultiplier(self, tag.divVar, cfg)` (ModStore.lua:401, 499) writes into the
//!   (shared, data-owned) tag. The port computes it into the local `div` and reads that where Lua
//!   reads `tag.div`. Within one call the two are identical (the write always precedes the read
//!   and is recomputed on every call). The other readers of a tag's `div` are listed in
//!   docs/port/ccr/0015-ms-b-evalmod-notes.md §1 together with the oracle proof
//!   (crates/pob-oracle/tests/MS-B.rs, `tag_div_no_reader`).
//! * `value = copyTable(value)` + `value.mod.value = …` (ModStore.lua 422-440, 511-523, 561-573)
//!   builds a new [`Value`] (and a new `Mod` for `value.mod`); the stored value is untouched.
//! * A nil `var`/`stat` argument: `GetMultiplier(target, nil, cfg)` and `GetCondition(target, nil,
//!   cfg)` raise in Lua (`multiplierName[nil]` / `conditionName[nil]` concatenate nil);
//!   `GetStat(target, nil, cfg)` reaches GetStat's guarded final branch (ModStore.lua:355), reads
//!   with a nil key (no error) and returns 0. See [`gm`], [`gc`], [`stat_key`].
//! * `GetStat` on a store without an actor (`self.actor = { }`) does not raise in Lua: its final
//!   branch is `(self.actor.output and self.actor.output[stat]) or (cfg and cfg.skillStats and
//!   cfg.skillStats[stat]) or 0` (ModStore.lua:355). Only `ManaReservedPercent`,
//!   `LifeReservedPercent` and `ManaUnreserved` (L320, L334, L347) index the missing `output` and
//!   raise; MS-A's `GetStat` does the same, so EvalMod's PerStat / PercentStat / StatThreshold
//!   tags need nothing special there.
#![allow(non_snake_case)]
// Lua statement shapes are kept (`local x` then assigned per branch; `if … then return end`).
#![allow(
    clippy::needless_late_init,
    clippy::redundant_else,
    clippy::collapsible_else_if,
    clippy::collapsible_if,
    clippy::float_cmp,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::manual_range_contains,
    clippy::nonminimal_bool
)]

use std::cell::RefCell;

use crate::modules::calc_tools::{gemIsType, getGameIdFromGemName};
use crate::prelude::*;
use pob_gamedata::{Ramp, Sockets};
use smallvec::SmallVec;

use super::mod_store::{arith, value_type};

/// @pob Classes/ModStore.lua:48-54 h=ae416723
impl ModStore {
    /// ModStore.lua:48-54. `self.actor` is `{ }` (a dangling Weak) for a store made without a
    /// parent: every field is nil, so the result is nil.
    pub(crate) fn getActor(&self, actorType: Option<Name>) -> Option<ActorRef> {
        let actor = self.actor.get().upgrade()?;
        // L49
        if actorType == Some(n!("player")) {
            // L50
            actor
                .player()
                .or_else(|| actor.parent().and_then(|p| p.player()))
                .or_else(|| actor.enemy().and_then(|e| e.player()))
        } else {
            // L52
            actor_field(&actor, actorType)
        }
    }
}

/// `actor[actorType]` for the actor-valued keys of an actor table (CalcSetup.lua 514-527,
/// CalcActiveSkill.lua 705-715, CalcPerform.lua 1311). `t[nil]` is nil.
///
/// LUA-QUIRK: any other key yields nil here. In Lua it yields that field's value (`level`,
/// `output`, …), which no tag uses: the census of `actor`/`limitActor`/`thresholdActor` values in
/// PoB's data and ModParser is exactly {enemy, parent, partyMembers, player}.
fn actor_field(actor: &Actor, actorType: Option<Name>) -> Option<ActorRef> {
    let k = actorType?;
    if k == n!("enemy") {
        actor.enemy()
    } else if k == n!("parent") {
        actor.parent()
    } else if k == n!("player") {
        actor.player()
    } else if k == n!("minion") {
        actor.minion.get()
    } else if k == n!("partyMembers") {
        actor.partyMembers.get().upgrade()
    } else {
        None
    }
}

// ---- Rust-only plumbing for EvalMod ----

/// `GetMultiplier(target, var, cfg)` with a possibly nil `var`: `multiplierName[nil]`
/// concatenates nil (ModStore.lua:21-24) before anything else happens.
#[inline]
fn gm(target: &ModStore, var: Option<Name>, cfg: Option<&ModCfg>) -> f64 {
    match var {
        Some(var) => target.GetMultiplier(var, cfg, false),
        None => lua_error!("attempt to concatenate local 'var' (a nil value)"),
    }
}

/// `GetCondition(target, var, cfg)` with a possibly nil `var`: `self.conditions[nil]` is nil,
/// the parent walk passes `noMod = true`, then `conditionName[nil]` concatenates nil
/// (ModStore.lua:25-28, 294), so a nil `var` always raises.
#[inline]
fn gc(target: &ModStore, var: Option<Name>, cfg: Option<&ModCfg>) -> bool {
    match var {
        Some(var) => target.GetCondition(var, cfg, false),
        None => lua_error!("attempt to concatenate local 'var' (a nil value)"),
    }
}

/// The key `GetStat` uses for a nil `stat` (`tag.stat` / `tag.thresholdStat` absent). Lua's final
/// branch (ModStore.lua:355) reads `self.actor.output[nil]` (only when `output` exists) and
/// `cfg.skillStats[nil]`, both nil without an error, so the result is 0; this reserved name is
/// never a key of either table, so the Rust `GetStat` (which takes a `Name`) behaves the same.
#[inline]
fn stat_key(stat: Option<Name>) -> Name {
    stat.unwrap_or_else(|| n!("\u{0}nil-stat"))
}

/// `type(value) == "table"`.
#[inline]
fn is_table(v: &Value) -> bool {
    matches!(v, Value::Table(_) | Value::Mod(_) | Value::JewelFunc(_))
}

/// `v[k]` on a mod value (read).
fn index(v: &Value, k: Name, what: &str) -> Value {
    match v {
        Value::Table(t) => t.get(k).clone(),
        Value::Mod(m) => {
            if k == n!("value") {
                m.value.clone()
            } else {
                m.extra_get(k).cloned().unwrap_or(Value::Nil)
            }
        }
        // `{ type = …, func = … }` jewel closures have neither `mod` nor `value`.
        Value::JewelFunc(_) => Value::Nil,
        // string methods are not table data
        Value::Str(_) => Value::Nil,
        other => lua_error!("attempt to index {what} (a {} value)", value_type(other)),
    }
}

/// A copy of the table value `v` with `v[k] = x` (`copyTable(value)` then the write).
fn with_field(v: &Value, k: Name, x: Value) -> Value {
    match v {
        Value::Table(t) => Value::Table(Arc::new(t.with(k, x))),
        Value::Mod(m) => {
            let mut c = (**m).clone();
            if k == n!("value") {
                c.value = x;
            } else {
                let mut ex = c.extra.take().map(|b| *b).unwrap_or_default();
                ex.set(k, x);
                c.extra = Some(Box::new(ex));
            }
            Value::Mod(Arc::new(c))
        }
        other => lua_error!("attempt to index a {} value", value_type(other)),
    }
}

/// `value = copyTable(value)`, then `value.mod.value = f(value.mod.value)` when `value.mod`,
/// else `value.value = f(value.value)` (ModStore.lua 422-440, 511-523, 561-573). Returns the
/// new value; the mod's stored value is never touched.
fn copy_and_set(value: &Value, f: impl FnOnce(&Value) -> f64) -> Value {
    let inner = index(value, n!("mod"), "local 'value'");
    if inner.truthy() {
        let old = index(&inner, n!("value"), "field 'mod'");
        let new_inner = with_field(&inner, n!("value"), Value::Num(f(&old)));
        with_field(value, n!("mod"), new_inner)
    } else {
        let old = index(value, n!("value"), "local 'value'");
        with_field(value, n!("value"), Value::Num(f(&old)))
    }
}

/// `math.min(x, …)` argument check (`lj_lib_checknum`: numbers and numeric strings).
fn checknum(v: &Value, arg: u32) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Str(s) => pob_lua::tonumber(s).unwrap_or_else(|| {
            lua_error!("bad argument #{arg} to 'min' (number expected, got string)")
        }),
        other => lua_error!(
            "bad argument #{arg} to 'min' (number expected, got {})",
            if other.is_nil() {
                "nil"
            } else {
                value_type(other)
            }
        ),
    }
}

thread_local! {
    /// `s:lower()` of interned names (a pure function; memoised so repeated tag evaluation does
    /// not allocate).
    static LOWER: RefCell<ahash::AHashMap<Name, Name>> = RefCell::new(ahash::AHashMap::new());
    /// The ItemCondition slot-name normalisation of ModStore.lua:693 (pure; memoised).
    static ITEM_SLOT: RefCell<ahash::AHashMap<Name, Name>> = RefCell::new(ahash::AHashMap::new());
}

thread_local! {
    /// `calcLib.getGameIdFromGemName(name, true)` per name: a pure function of the immutable
    /// `data` (CalcTools.lua:279-292), memoised because it lower-cases its argument.
    static GAME_ID: RefCell<ahash::AHashMap<Name, Option<Name>>> = RefCell::new(ahash::AHashMap::new());
}

/// `calcLib.getGameIdFromGemName(name, true)` (nil name → nil).
fn game_id(name: Option<Name>) -> Option<Name> {
    let name = name?;
    if let Some(r) = GAME_ID.with(|m| m.borrow().get(&name).copied()) {
        return r;
    }
    let r = getGameIdFromGemName(Some(name.as_str()), true);
    GAME_ID.with(|m| m.borrow_mut().insert(name, r));
    r
}

/// `name:lower()`.
fn lower(s: Name) -> Name {
    if let Some(l) = LOWER.with(|m| m.borrow().get(&s).copied()) {
        return l;
    }
    let l = Name::from(pob_lua::string::lower(s.as_str()).as_str());
    LOWER.with(|m| m.borrow_mut().insert(s, l));
    l
}

/// ModStore.lua:693: `tag.itemSlot:lower():gsub("(%l)(%w*)", function(a,b) return
/// string.upper(a)..b end):gsub('^%s*(.-)%s*$', '%1')`.
fn item_slot_name(itemSlot: Name) -> Name {
    if let Some(n) = ITEM_SLOT.with(|m| m.borrow().get(&itemSlot).copied()) {
        return n;
    }
    let lowered = pob_lua::string::lower(itemSlot.as_str());
    let (s, _) = pob_lua::gsub_fn(
        &lowered,
        "(%l)(%w*)",
        |c| {
            let mut r = pob_lua::string::upper(&c[0].to_str());
            r.push_str(&c[1].to_str());
            Some(r)
        },
        None,
    );
    let (s, _) = pob_lua::gsub(&s, "^%s*(.-)%s*$", "%1", None);
    let n = Name::from(s.as_str());
    ITEM_SLOT.with(|m| m.borrow_mut().insert(itemSlot, n));
    n
}

/// `name:match("Kalandra's Touch")` (no magic characters: a plain substring test).
#[inline]
fn is_kalandra(item: &Item) -> bool {
    item.name.get().contains("Kalandra's Touch")
}

/// `tag.sockets` / `cfg.socketNum` membership (ModStore.lua 766-773, the local `isValidSocket`).
fn isValidSocket(sockets: &[f64], targetSocket: f64) -> bool {
    // L767
    for &val in sockets {
        // L768
        if val == targetSocket {
            // L769
            return true;
        }
    }
    // L772
    false
}

/// @pob Classes/ModStore.lua:363-971 h=e55ec1f4
impl ModStore {
    /// `context` is the store the query started on (passed down the parent chain exactly as Lua does).
    ///
    /// Returns `Value::Nil` where Lua returns nothing (a tag that does not match).
    pub fn EvalMod(
        &self,
        m: &Mod,
        cfg: Option<&ModCfg>,
        globalLimits: Option<&mut GlobalLimits>,
    ) -> Value {
        // L364
        let mut value = m.value.clone();
        // L365-367: `local GetStat = self.GetStat` etc. are the ModStore methods (no override exists).
        // L368
        for tag in &m.tags {
            match &tag.r#type {
                // L369
                TagType::Multiplier => {
                    // L370-371
                    let target_db;
                    let mut target: &ModStore = self;
                    let limit_db;
                    let mut limitTarget: &ModStore = self;

                    // Allow limiting a self multiplier on a parent multiplier (eg. Agony Crawler on player virulence)
                    // This explicit target is necessary because even though the GetMultiplier method does call self.parent.GetMultiplier, it does so with noMod = true,
                    // disabling the summation (3rd part): (not noMod and self:Sum("BASE", cfg, multiplierName[var]) or 0)
                    // L376
                    if tag.limitActor.is_some() {
                        // L377
                        let limitActor = self.getActor(tag.limitActor);
                        // L378
                        if let Some(limitActor) = limitActor {
                            // L379
                            limit_db = limitActor.modDB.get();
                            limitTarget = &limit_db;
                        } else {
                            // L381
                            return Value::Nil;
                        }
                    }
                    // L384
                    if tag.actor.is_some() {
                        // L385
                        let actor = self.getActor(tag.actor);
                        // L386
                        if let Some(actor) = actor {
                            // L387
                            target_db = actor.modDB.get();
                            target = &target_db;
                        } else {
                            // L389
                            return Value::Nil;
                        }
                    }
                    // L392
                    let mut base = 0.0;
                    // L393
                    if let Some(varList) = &tag.varList {
                        // L394 (pairs over a sequence: index order)
                        for &var in varList.iter() {
                            // L395
                            base += target.GetMultiplier(var, cfg, false);
                        }
                    } else {
                        // L398
                        base = gm(target, tag.var, cfg);
                    }
                    // L400 `tag.div = GetMultiplier(self, tag.divVar, cfg)` (L401) is computed
                    // into a local: EvalMod never mutates a tag (calc doc §5.3).
                    let div = match tag.divVar {
                        Some(divVar) => Some(self.GetMultiplier(divVar, cfg, false)),
                        None => tag.div,
                    };
                    // L403
                    let mut mult = m_floor(base / div.unwrap_or(1.0) + 0.0001);
                    // L404
                    if tag.noFloor {
                        // L405
                        mult = base / div.unwrap_or(1.0);
                    }
                    // L407
                    let mut limitTotal: Option<f64> = None;
                    // L408
                    let mut limitNegTotal: Option<f64> = None;
                    // L409
                    if tag.limit.is_some() || tag.limitVar.is_some() || tag.limitStat.is_some() {
                        // L410
                        let limit = if let Some(l) = tag.limit {
                            l
                        } else if let Some(limitVar) = tag.limitVar {
                            limitTarget.GetMultiplier(limitVar, cfg, false)
                        } else {
                            limitTarget.GetStat(stat_key(tag.limitStat), cfg)
                        };
                        // L411
                        if tag.limitTotal {
                            // L412
                            limitTotal = Some(limit);
                        } else if tag.limitNegTotal {
                            // L414
                            limitNegTotal = Some(limit);
                        } else {
                            // L416
                            mult = m_min(mult, limit);
                        }
                    }
                    // L419
                    if tag.invert && mult != 0.0 {
                        // L420
                        mult = 1.0 / mult;
                    }
                    let tagBase = tag.base.unwrap_or(0.0);
                    let apply = |old: &Value, what: &str| -> f64 {
                        // L425 / L433 / L442
                        let mut v = arith(old, what) * mult + tagBase;
                        // L426 / L434 / L443
                        if let Some(limitTotal) = limitTotal {
                            // L427 / L435 / L444
                            v = m_min(v, limitTotal);
                        }
                        // L429 / L437 / L446
                        if let Some(limitNegTotal) = limitNegTotal {
                            // L430 / L438 / L447
                            v = m_max(v, limitNegTotal);
                        }
                        v
                    };
                    // L422
                    if is_table(&value) {
                        // L423-440
                        value = copy_and_set(&value, |old| apply(old, "field 'value'"));
                    } else {
                        // L442-448
                        value = Value::Num(apply(&value, "local 'value'"));
                    }
                }
                // L450
                TagType::MultiplierThreshold => {
                    // L451-452
                    let target_db;
                    let mut target: &ModStore = self;
                    let threshold_db;
                    let mut thresholdTarget: &ModStore = self;
                    // L453
                    if tag.thresholdActor.is_some() {
                        // L454
                        let thresholdActor = self.getActor(tag.thresholdActor);
                        // L455
                        if let Some(thresholdActor) = thresholdActor {
                            // L456
                            threshold_db = thresholdActor.modDB.get();
                            thresholdTarget = &threshold_db;
                        } else {
                            // L458
                            return Value::Nil;
                        }
                    }
                    // L461
                    if tag.actor.is_some() {
                        // L462
                        let actor = self.getActor(tag.actor);
                        // L463
                        if let Some(actor) = actor {
                            // L464
                            target_db = actor.modDB.get();
                            target = &target_db;
                        } else {
                            // L466
                            return Value::Nil;
                        }
                    }
                    // L469
                    let mut mult = 0.0;
                    // L470
                    if let Some(varList) = &tag.varList {
                        // L471
                        for &var in varList.iter() {
                            // L472
                            mult += target.GetMultiplier(var, cfg, false);
                        }
                    } else {
                        // L475
                        mult = gm(target, tag.var, cfg);
                    }
                    // L477
                    let threshold = match tag.threshold {
                        Some(t) => t,
                        None => gm(
                            if tag.thresholdActor.is_some() {
                                thresholdTarget
                            } else {
                                target
                            },
                            tag.thresholdVar,
                            cfg,
                        ),
                    };
                    // L478
                    if (tag.upper && mult > threshold)
                        || (tag.equals && mult != threshold)
                        || (!tag.upper && mult < threshold)
                    {
                        // L479
                        return Value::Nil;
                    }
                }
                // L481
                TagType::PerStat => {
                    // L482
                    let mut base: f64;
                    // L483
                    let target_db;
                    let mut target: &ModStore = self;
                    // This functions similar to the above tagTypes in regard to which actor to use, but for PerStat
                    // if the actor is 'parent', we don't want to return if we're already using 'parent', just keep using 'self'
                    // L486
                    let actor = self.getActor(tag.actor);
                    // L487
                    if let Some(actor) = actor {
                        // L488
                        target_db = actor.modDB.get();
                        target = &target_db;
                    }
                    // L490
                    if let Some(statList) = &tag.statList {
                        // L491
                        base = 0.0;
                        // L492
                        for &stat in statList.iter() {
                            // L493
                            base += target.GetStat(stat, cfg);
                        }
                    } else {
                        // L496
                        base = target.GetStat(stat_key(tag.stat), cfg);
                    }
                    // L498 `tag.div = GetMultiplier(self, tag.divVar, cfg)` (L499) as a local.
                    let div = match tag.divVar {
                        Some(divVar) => Some(self.GetMultiplier(divVar, cfg, false)),
                        None => tag.div,
                    };
                    // L501
                    let mut mult = m_floor(base / div.unwrap_or(1.0) + 0.0001);
                    // L502
                    let mut limitTotal: Option<f64> = None;
                    // L503
                    if tag.limit.is_some() || tag.limitVar.is_some() {
                        // L504
                        let limit = match tag.limit {
                            Some(l) => l,
                            None => gm(self, tag.limitVar, cfg),
                        };
                        // L505
                        if tag.limitTotal {
                            // L506
                            limitTotal = Some(limit);
                        } else {
                            // L508
                            mult = m_min(mult, limit);
                        }
                    }
                    let tagBase = tag.base.unwrap_or(0.0);
                    let apply = |old: &Value, what: &str| -> f64 {
                        // L514 / L519 / L525
                        let mut v = arith(old, what) * mult + tagBase;
                        // L515 / L520 / L526
                        if let Some(limitTotal) = limitTotal {
                            // L516 / L521 / L527
                            v = m_min(v, limitTotal);
                        }
                        v
                    };
                    // L511
                    if is_table(&value) {
                        // L512-523
                        value = copy_and_set(&value, |old| apply(old, "field 'value'"));
                    } else {
                        // L525-528
                        value = Value::Num(apply(&value, "local 'value'"));
                    }
                }
                // L530
                TagType::PercentStat => {
                    // L531
                    let mut base: f64;
                    // L532
                    let target_db;
                    let mut target: &ModStore = self;
                    // This functions similar to the above tagTypes in regard to which actor to use, but for PercentStat
                    // if the actor is 'parent', we don't want to return if we're already using 'parent', just keep using 'self'
                    // L535
                    let actor = self.getActor(tag.actor);
                    // L536
                    if let Some(actor) = actor {
                        // L537
                        target_db = actor.modDB.get();
                        target = &target_db;
                    }
                    // L539
                    if let Some(statList) = &tag.statList {
                        // L540
                        base = 0.0;
                        // L541
                        for &stat in statList.iter() {
                            // L542
                            base += target.GetStat(stat, cfg);
                        }
                    } else {
                        // L545
                        base = target.GetStat(stat_key(tag.stat), cfg);
                    }
                    // L547
                    let percent = match tag.percent {
                        Some(p) => p,
                        None => gm(self, tag.percentVar, cfg),
                    };
                    // L548 (`percent` is a number here, so `percent and percent / 100 or 1` is `percent / 100`)
                    let mut mult = base * (percent / 100.0);
                    // L549
                    if tag.floor {
                        // L550
                        mult = m_floor(mult);
                    }
                    // L552
                    let mut limitTotal: Option<f64> = None;
                    // L553
                    if tag.limit.is_some() || tag.limitVar.is_some() {
                        // L554
                        let limit = match tag.limit {
                            Some(l) => l,
                            None => gm(self, tag.limitVar, cfg),
                        };
                        // L555
                        if tag.limitTotal {
                            // L556
                            limitTotal = Some(limit);
                        } else {
                            // L558
                            mult = m_min(mult, limit);
                        }
                    }
                    let tagBase = tag.base.unwrap_or(0.0);
                    let apply = |old: &Value, what: &str| -> f64 {
                        // L564 / L569 / L575
                        let mut v = m_ceil(arith(old, what) * mult + tagBase);
                        // L565 / L570 / L576
                        if let Some(limitTotal) = limitTotal {
                            // L566 / L571 / L577
                            v = m_min(v, limitTotal);
                        }
                        v
                    };
                    // L561
                    if is_table(&value) {
                        // L562-573
                        value = copy_and_set(&value, |old| apply(old, "field 'value'"));
                    } else {
                        // L575-578
                        value = Value::Num(apply(&value, "local 'value'"));
                    }
                }
                // L580
                TagType::StatThreshold => {
                    // L581
                    let stat: f64;
                    // L582
                    if let Some(statList) = &tag.statList {
                        // L583
                        stat = 0.0;
                        // L584
                        for &statName in statList.iter() {
                            // L585 LUA-QUIRK: the loop variable `stat` shadows the outer `stat`, so
                            // `stat + GetStat(self, stat, cfg)` adds the stat *name* (a string) to
                            // the stat value and assigns the loop variable; the outer `stat` stays
                            // 0. The string operand raises unless it is numeric.
                            let g = self.GetStat(statName, cfg);
                            match pob_lua::tonumber(statName.as_str()) {
                                Some(x) => {
                                    let _ = x + g;
                                }
                                None => lua_error!(
                                    "attempt to perform arithmetic on local 'stat' (a string value)"
                                ),
                            }
                        }
                    } else {
                        // L588
                        stat = self.GetStat(stat_key(tag.stat), cfg);
                    }
                    // L590
                    let mut threshold = match tag.threshold {
                        Some(t) => t,
                        None => self.GetStat(stat_key(tag.thresholdStat), cfg),
                    };
                    // L591
                    if tag.thresholdPercent.is_some() || tag.thresholdPercentVar.is_some() {
                        // L592
                        let thresholdPercent = match tag.thresholdPercent {
                            Some(p) => p,
                            None => gm(self, tag.thresholdPercentVar, cfg),
                        };
                        // L593
                        threshold *= thresholdPercent / 100.0;
                    }
                    // L595
                    if (tag.upper && stat > threshold) || (!tag.upper && stat < threshold) {
                        // L596
                        return Value::Nil;
                    }
                }
                // L598
                TagType::DistanceRamp => {
                    // L599
                    let Some(skillDist) = cfg.and_then(|c| c.skillDist.get()) else {
                        // L600
                        return Value::Nil;
                    };
                    let ramp: &[[f64; 2]] = match &tag.ramp {
                        Some(Ramp::Points(p)) => p,
                        Some(Ramp::Flat(_)) => {
                            lua_error!("attempt to index field '?' (a number value)")
                        }
                        None => lua_error!("attempt to index field 'ramp' (a nil value)"),
                    };
                    let Some(first) = ramp.first() else {
                        lua_error!("attempt to index field '?' (a nil value)")
                    };
                    let last = ramp[ramp.len() - 1];
                    // L602
                    if skillDist <= first[0] {
                        // L603
                        value = Value::Num(arith(&value, "local 'value'") * first[1]);
                    } else if skillDist >= last[0] {
                        // L605
                        value = Value::Num(arith(&value, "local 'value'") * last[1]);
                    } else {
                        // L607
                        for (i, dat) in ramp.iter().enumerate() {
                            // L608
                            let Some(next) = ramp.get(i + 1) else {
                                lua_error!("attempt to index local 'next' (a nil value)")
                            };
                            // L609
                            if skillDist <= next[0] {
                                // L610
                                value = Value::Num(
                                    arith(&value, "local 'value'")
                                        * (dat[1]
                                            + (next[1] - dat[1]) * (skillDist - dat[0])
                                                / (next[0] - dat[0])),
                                );
                                // L611
                                break;
                            }
                        }
                    }
                    // Syntax: { type = "MeleeProximity", ramp = {MaxBonusPct,MinBonusPct} }
                    // 			Both MaxBonusPct and MinBonusPct are percent in decimal form (1.0 = 100%)
                    // Example: { type = "MeleeProximity", ramp = {1,0} }   ## Duelist-Slayer: Impact
                }
                // L618
                TagType::MeleeProximity => {
                    // L619
                    let Some(skillDist) = cfg.and_then(|c| c.skillDist.get()) else {
                        // L620
                        return Value::Nil;
                    };
                    // `tag.ramp[1]` (read only by the branches that use it)
                    let ramp1 = || -> f64 {
                        match &tag.ramp {
                            Some(Ramp::Flat(f)) => match f.first() {
                                Some(&x) => x,
                                None => lua_error!(
                                    "attempt to perform arithmetic on field '?' (a nil value)"
                                ),
                            },
                            Some(Ramp::Points(_)) => lua_error!(
                                "attempt to perform arithmetic on field '?' (a table value)"
                            ),
                            None => lua_error!("attempt to index field 'ramp' (a nil value)"),
                        }
                    };
                    // Max potency is 0-15 units of distance
                    // L623
                    if skillDist <= 15.0 {
                        // L624
                        let r = ramp1();
                        value = Value::Num(arith(&value, "local 'value'") * r);
                        // Reduced potency (linear) until 40 units
                    } else if skillDist >= 16.0 && skillDist <= 39.0 {
                        // LUA-QUIRK: a skillDist strictly between 15 and 16 or 39 and 40 matches
                        // no branch and leaves the value unchanged.
                        // L627
                        let r = ramp1() - ((ramp1() / 25.0) * (skillDist - 15.0));
                        value = Value::Num(arith(&value, "local 'value'") * r);
                    } else if skillDist >= 40.0 {
                        // L629
                        value = Value::Num(0.0);
                    }
                }
                // L631
                TagType::Limit => {
                    // L632
                    let limit = match tag.limit {
                        Some(l) => l,
                        None => gm(self, tag.limitVar, cfg),
                    };
                    value = Value::Num(m_min(checknum(&value, 1), limit));
                }
                // L633
                TagType::Condition => {
                    // L634
                    let mut r#match = false;
                    // L635
                    let allOneH: Option<Table> = self.actor.get().upgrade().and_then(|a| {
                        let w1 = a.weaponData1.get();
                        if w1.truthy(n!("countsAsAll1H")) {
                            return Some(w1);
                        }
                        let w2 = a.weaponData2.get();
                        if w2.truthy(n!("countsAsAll1H")) {
                            return Some(w2);
                        }
                        None
                    });
                    // L636
                    if let Some(varList) = &tag.varList {
                        // L637
                        for &var in varList.iter() {
                            // L638
                            let added = match &allOneH {
                                Some(t) if tag.neg => t.get(cat!("Added", var)),
                                _ => Val::Nil,
                            };
                            if tag.neg && allOneH.is_some() && !added.is_nil() {
                                // Varunastra adds all using weapon conditions and that causes this condition to fail when it shouldn't
                                // if the condition was added by Varunastra then ignore, otherwise return as the tag condition is not satisfied
                                // L641
                                if !added.truthy() {
                                    // L642
                                    return Value::Nil;
                                }
                            } else if self.GetCondition(var, cfg, false)
                                || cfg.is_some_and(|c| {
                                    c.skillCond.get().is_some_and(|sc| sc.truthy(var))
                                })
                            {
                                // L645
                                r#match = true;
                                // L646
                                break;
                            }
                        }
                    } else {
                        // L650
                        let added = match &allOneH {
                            Some(t) if tag.neg => match tag.var {
                                Some(var) => t.get(cat!("Added", var)),
                                None => {
                                    lua_error!("attempt to concatenate field 'var' (a nil value)")
                                }
                            },
                            _ => Val::Nil,
                        };
                        if tag.neg && allOneH.is_some() && !added.is_nil() {
                            // L651
                            if !added.truthy() {
                                // L652
                                return Value::Nil;
                            }
                        } else {
                            // L655
                            r#match = gc(self, tag.var, cfg)
                                || cfg.is_some_and(|c| {
                                    c.skillCond.get().is_some_and(|sc| match tag.var {
                                        Some(var) => sc.truthy(var),
                                        None => false,
                                    })
                                });
                        }
                    }
                    // L658
                    if tag.neg {
                        // L659
                        r#match = !r#match;
                    }
                    // L661
                    if !r#match {
                        // L662
                        return Value::Nil;
                    }
                }
                // L664
                TagType::ActorCondition => {
                    // L665
                    let mut r#match = false;
                    // L666
                    let target_db;
                    let mut target: Option<&ModStore> = Some(self);
                    // L667
                    if tag.actor.is_some() {
                        // L668
                        let actor = self.getActor(tag.actor);
                        // L669
                        match actor {
                            Some(actor) => {
                                target_db = actor.modDB.get();
                                target = Some(&target_db);
                            }
                            None => target = None,
                        }
                    }
                    // L671
                    if let (Some(target), true) =
                        (target, tag.var.is_some() || tag.varList.is_some())
                    {
                        // L672
                        if let Some(varList) = &tag.varList {
                            // L673
                            for &var in varList.iter() {
                                // L674
                                if target.GetCondition(var, cfg, false) {
                                    // L675
                                    r#match = true;
                                    // L676
                                    break;
                                }
                            }
                        } else {
                            // L680
                            r#match = gc(target, tag.var, cfg);
                        }
                    } else if tag.actor.is_some() && cfg.is_some_and(|c| tag.actor == c.actor.get())
                    {
                        // L683
                        r#match = true;
                    }
                    // L685
                    if tag.neg {
                        // L686
                        r#match = !r#match;
                    }
                    // L688
                    if !r#match {
                        // L689
                        return Value::Nil;
                    }
                }
                // L691
                TagType::ItemCondition => {
                    if !self.ItemCondition(tag, cfg) {
                        return Value::Nil;
                    }
                }
                // L762
                TagType::SocketedIn => {
                    // L763
                    let Some(cfg) = cfg else {
                        // L764
                        return Value::Nil;
                    };
                    if tag.slotName.is_none() && tag.keyword.is_none() && tag.socketColor.is_none()
                    {
                        // L764
                        return Value::Nil;
                    } else {
                        // L775: `match` holds at most these four keys; any failing entry returns,
                        // so pairs order does not matter.
                        let mut r#match: SmallVec<[bool; 4]> = SmallVec::new();
                        // L776
                        if tag.slotName.is_some() {
                            // L777
                            r#match.push(tag.slotName == cfg.slotName.get());
                        }
                        // L779
                        if let Some(keyword) = tag.keyword {
                            // L780
                            r#match.push(
                                cfg.skillGem
                                    .get()
                                    .is_some_and(|gem| gemIsType(&gem, keyword.as_str(), false)),
                            );
                        } else if tag.socketColor.is_some() && tag.sockets != Some(Sockets::All) {
                            // the all socket tag inherently checks for the correct color
                            // L782
                            r#match.push(tag.socketColor == cfg.socketColor.get());
                        }
                        // L784
                        if let Some(sockets) = &tag.sockets {
                            // L785
                            let sc = tag.socketColor;
                            let targetAtrColor = if sc == Some(n!("R")) {
                                Some(cfg.strengthGems.get())
                            } else if sc == Some(n!("G")) {
                                Some(cfg.dexterityGems.get())
                            } else if sc == Some(n!("B")) {
                                Some(cfg.intelligenceGems.get())
                            } else {
                                None // `cfg[false]` is nil
                            };
                            // L786
                            let count = targetAtrColor.flatten().unwrap_or(0.0);
                            match sockets {
                                // L787
                                Sockets::All => {
                                    // L788
                                    let total = cfg.intelligenceGems.get().unwrap_or(0.0)
                                        + cfg.dexterityGems.get().unwrap_or(0.0)
                                        + cfg.strengthGems.get().unwrap_or(0.0);
                                    // L789
                                    r#match.push((total == count) && (total > 0.0));
                                }
                                // L790
                                Sockets::List(list) if cfg.socketNum.get().is_some() => {
                                    // L791
                                    r#match.push(isValidSocket(
                                        list,
                                        cfg.socketNum.get().unwrap_or_default(),
                                    ));
                                }
                                // L792
                                Sockets::Count(n) => {
                                    // L793
                                    r#match.push(count < *n);
                                }
                                Sockets::List(_) => {
                                    // L795
                                    return Value::Nil;
                                }
                            }
                        }
                        // L798 LUA-ORDER: pairs over the string-keyed `match`; any failing entry
                        // returns, so the result does not depend on the order.
                        for v in r#match {
                            // L799
                            if (!tag.neg && !v) || (tag.neg && v) {
                                // L800
                                return Value::Nil;
                            }
                        }
                    }
                }
                // L804
                TagType::SkillName => {
                    // L805
                    let mut r#match = false;
                    // L806
                    if tag.includeTransfigured {
                        // L807: `cfg and getGameId(…) or ""` (a nil result becomes "")
                        let gid = game_id;
                        let matchGameId: Option<Name> = if tag.summonSkill {
                            Some(
                                cfg.and_then(|c| gid(c.summonSkillName.get()))
                                    .unwrap_or_else(|| n!("")),
                            )
                        } else {
                            Some(
                                cfg.and_then(|c| c.skillName.get().and_then(|s| gid(Some(s))))
                                    .unwrap_or_else(|| n!("")),
                            )
                        };
                        // L808
                        if let Some(skillNameList) = &tag.skillNameList {
                            // L809
                            for &name in skillNameList.iter() {
                                // L810
                                if matchGameId == gid(Some(name)) {
                                    // L811
                                    r#match = true;
                                    // L812
                                    break;
                                }
                            }
                        } else {
                            // L816
                            r#match = tag.skillName.is_some() && matchGameId == gid(tag.skillName);
                        }
                    } else {
                        // L819
                        let matchName = if tag.summonSkill {
                            cfg.and_then(|c| c.summonSkillName.get())
                                .unwrap_or_else(|| n!(""))
                        } else {
                            cfg.and_then(|c| c.skillName.get())
                                .unwrap_or_else(|| n!(""))
                        };
                        // L820
                        let matchName = lower(matchName);
                        // L821
                        if let Some(skillNameList) = &tag.skillNameList {
                            // L822
                            for &name in skillNameList.iter() {
                                // L823
                                if lower(name) == matchName {
                                    // L824
                                    r#match = true;
                                    // L825
                                    break;
                                }
                            }
                        } else {
                            // L829
                            r#match = tag.skillName.is_some_and(|s| lower(s) == matchName);
                        }
                    }
                    // L832
                    if tag.neg {
                        // L833
                        r#match = !r#match;
                    }
                    // L835
                    if !r#match {
                        // L836
                        return Value::Nil;
                    }
                }
                // L838
                TagType::SkillId => {
                    // L839
                    let ok = cfg.is_some_and(|c| {
                        c.skillGrantedEffect
                            .get()
                            .is_some_and(|ge| Some(ge.id) == tag.skillId)
                    });
                    if !ok {
                        // L840
                        return Value::Nil;
                    }
                }
                // L842
                TagType::SkillPart => {
                    // L843
                    let Some(cfg) = cfg else {
                        // L844
                        return Value::Nil;
                    };
                    // L846
                    let mut r#match = false;
                    let skillPart = cfg.skillPart.get();
                    // L847
                    if let Some(skillPartList) = &tag.skillPartList {
                        // L848
                        for &part in skillPartList.iter() {
                            // L849
                            if Some(part) == skillPart {
                                // L850
                                r#match = true;
                                // L851
                                break;
                            }
                        }
                    } else {
                        // L855
                        r#match = tag.skillPart == skillPart;
                    }
                    // L857
                    if tag.neg {
                        // L858
                        r#match = !r#match;
                    }
                    // L860
                    if !r#match {
                        // L861
                        return Value::Nil;
                    }
                }
                // L863
                TagType::SkillType => {
                    // L864
                    let mut r#match = false;
                    let skillTypes = cfg.and_then(|c| c.skillTypes.get());
                    // L865
                    if let Some(skillTypeList) = &tag.skillTypeList {
                        // L866
                        for &ty in skillTypeList.iter() {
                            // L867
                            if skillTypes.as_ref().is_some_and(|s| s.with(|s| s.has(ty))) {
                                // L868
                                r#match = true;
                                // L869
                                break;
                            }
                        }
                    } else {
                        // L873
                        r#match = match (tag.skillType, &skillTypes) {
                            (Some(ty), Some(s)) => s.with(|s| s.has(ty)),
                            _ => false,
                        };
                    }
                    // L875
                    if tag.neg {
                        // L876
                        r#match = !r#match;
                    }
                    // L878
                    if !r#match {
                        // L879
                        return Value::Nil;
                    }
                }
                // L881
                TagType::BaseFlag => {
                    // L882: grantedEffect.baseFlags is never nil in the typed data (an absent
                    // table loads as the empty set); `cfg.baseFlags` is set by no cfg at the pin.
                    let baseFlags = cfg.and_then(|c| {
                        c.skillGrantedEffect
                            .get()
                            .map(|ge| ge.baseFlags.clone())
                            .or_else(|| c.baseFlags.get())
                    });
                    // L883
                    let mut r#match = match (&baseFlags, tag.baseFlag) {
                        (Some(b), Some(f)) => b.contains(&f),
                        _ => false,
                    };
                    // L884
                    if tag.neg {
                        // L885
                        r#match = !r#match;
                    }
                    // L887
                    if !r#match {
                        // L888
                        return Value::Nil;
                    }
                }
                // L890
                TagType::SlotName => {
                    // L891
                    let Some(cfg) = cfg else {
                        // L892
                        return Value::Nil;
                    };
                    // L894
                    let mut r#match = false;
                    let slotName = cfg.slotName.get();
                    // L895
                    if let Some(slotNameList) = &tag.slotNameList {
                        // L896
                        for &slot in slotNameList.iter() {
                            // L897
                            if Some(slot) == slotName {
                                // L898
                                r#match = true;
                                // L899
                                break;
                            }
                        }
                    } else {
                        // L903
                        r#match = tag.slotName == slotName;
                    }
                    // L905
                    if tag.neg {
                        // L906
                        r#match = !r#match;
                    }
                    // L908
                    if !r#match {
                        // L909
                        return Value::Nil;
                    }
                }
                // L911
                TagType::ModFlagOr => {
                    // L912 (`cfg.flags` nil and 0 both return: band(0, x) == 0)
                    let Some(cfg) = cfg else {
                        // L913
                        return Value::Nil;
                    };
                    // L915
                    if pob_lua::bit::band(cfg.flags.get(), tag.modFlags) == 0 {
                        // L916
                        return Value::Nil;
                    }
                }
                // L918
                TagType::KeywordFlagAnd => {
                    // L919
                    let Some(cfg) = cfg else {
                        // L920
                        return Value::Nil;
                    };
                    // L922
                    if pob_lua::bit::band(cfg.keywordFlags.get(), tag.keywordFlags)
                        != tag.keywordFlags
                    {
                        // L923
                        return Value::Nil;
                    }
                }
                // L925
                TagType::MonsterTag => {
                    // actor should be a minion to apply
                    // L927
                    let Some(minionData) =
                        self.actor.get().upgrade().and_then(|a| a.minionData.get())
                    else {
                        // L928
                        return Value::Nil;
                    };

                    // L931
                    let mut r#match = false;

                    // validate for actor and minionData
                    // L934
                    for &tagList in &minionData.monsterTags {
                        // L935
                        let matchName = lower(tagList);
                        // L936
                        if let Some(monsterTagList) = &tag.monsterTagList {
                            // L937
                            for &name in monsterTagList.iter() {
                                // L938
                                if lower(name) == matchName {
                                    // L939
                                    r#match = true;
                                    // L940
                                    break;
                                }
                            }
                        } else {
                            // L944
                            r#match = tag.monsterTag.is_some_and(|t| lower(t) == matchName);
                        }
                        // L946
                        if r#match {
                            // L947
                            break;
                        }
                    }
                    // L950
                    if tag.neg {
                        // L951
                        r#match = !r#match;
                    }
                    // L953
                    if !r#match {
                        // L954
                        return Value::Nil;
                    }
                }
                // A non-table entry in the mod's sequence: `tag.type` indexes it. A string has
                // no `type` field (string methods), anything else raises.
                TagType::LuaNonTable => match tag.extra_get(n!("$value")) {
                    Some(Value::Str(_)) => {}
                    other => lua_error!(
                        "attempt to index local 'tag' (a {} value)",
                        other.map_or("nil", value_type)
                    ),
                },
                // GlobalEffect, InSlot, SlotNumber, DisablesItem, IgnoreCond, Global, Skill, …:
                // no EvalMod branch.
                _ => {}
            }
        }

        // Apply global limits
        // L960
        if let Some(globalLimits) = globalLimits {
            for tag in &m.tags {
                // L961
                if let (Some(globalLimit), Some(globalLimitKey)) =
                    (tag.globalLimit, tag.globalLimitKey)
                {
                    // L962
                    if !value.truthy() {
                        value = Value::Num(0.0);
                    }
                    // L963
                    let cur = globalLimits.get(&globalLimitKey).copied().unwrap_or(0.0);
                    globalLimits.insert(globalLimitKey, cur);
                    // L964
                    let v = arith(&value, "local 'value'");
                    if cur + v > globalLimit {
                        // L965
                        value = Value::Num(globalLimit - cur);
                    }
                    // L967
                    let v = arith(&value, "local 'value'");
                    globalLimits.insert(globalLimitKey, cur + v);
                }
            }
        }
        // L970
        value
    }

    /// The `ItemCondition` branch of EvalMod (ModStore.lua 691-761); `false` = Lua's `return`.
    fn ItemCondition(&self, tag: &Tag, cfg: Option<&ModCfg>) -> bool {
        // L692
        let mut matches: SmallVec<[bool; 64]> = SmallVec::new();
        // L693
        let Some(rawSlot) = tag.itemSlot else {
            lua_error!("attempt to index field 'itemSlot' (a nil value)")
        };
        let itemSlot = item_slot_name(rawSlot);
        let actor = self.actor.get().upgrade();
        let cfgItem = || cfg.and_then(|c| c.item.get());
        // L694 LUA-ORDER (pairs over `items` at 715, 722, 727, 732, 737, 742, 747): `items` as
        // (slot, item) pairs in insertion order. Every use below only collects
        // booleans whose order does not matter, so PoB's random `pairs` order is not observable.
        let mut items: SmallVec<[(Name, ItemRef); 64]> = SmallVec::new();
        // L695
        if tag.allSlots {
            // L696: `self.actor.itemList` (nil for a store without an actor: the `pairs(items)`
            // below raises before anything else happens).
            let Some(actor) = &actor else {
                lua_error!("bad argument #1 to 'for iterator' (table expected, got nil)")
            };
            actor.itemList.with(|l| {
                for (k, v) in l {
                    items.push((*k, v.clone()));
                }
            });
        } else if let Some(actor) = &actor {
            // L697
            let itemList = |k: Name| actor.itemList.with(|l| l.get(&k).cloned());
            // L698
            if tag.bothSlots {
                // L699
                let k1 = cat!(itemSlot, " 1");
                let mut itemSlot1 = itemList(k1);
                // L700
                let k2 = cat!(itemSlot, " 2");
                let mut itemSlot2 = itemList(k2);
                // L701
                if itemSlot1.as_ref().is_some_and(|i| is_kalandra(i)) {
                    itemSlot1 = itemSlot2.clone();
                }
                // L702
                if itemSlot2.as_ref().is_some_and(|i| is_kalandra(i)) {
                    itemSlot2 = itemSlot1.clone();
                }
                // L703
                if let (Some(i1), Some(i2)) = (itemSlot1, itemSlot2) {
                    // L704
                    items.push((k1, i1));
                    items.push((k2, i2));
                }
            } else {
                // L707
                let mut item = itemList(itemSlot).or_else(cfgItem);
                // L708
                if item.as_ref().is_some_and(|i| is_kalandra(i)) {
                    // L709
                    let (other, _) = pob_lua::gsub_table(
                        itemSlot.as_str(),
                        "%d$",
                        |c| match &*c.to_str() {
                            "1" => Some("2".to_owned()),
                            "2" => Some("1".to_owned()),
                            _ => None,
                        },
                        None,
                    );
                    item = itemList(Name::from(other.as_str()));
                }
                // L711
                if let Some(item) = item.or_else(cfgItem) {
                    items.push((itemSlot, item));
                }
            }
        }
        // L714
        if let Some(searchCond) = &tag.searchCond {
            let searchCond = lower(Name::from(searchCond.as_ref()));
            // L715
            for (slot, item) in &items {
                // L716
                if ((!tag.allSlots
                    || (tag.allSlots
                        && (item.r#type.get() != n!("Jewel") && item.r#type.get() != n!("Graft"))))
                    && *slot != itemSlot)
                    || !tag.excludeSelf
                {
                    // L717
                    matches.push(
                        item.FindModifierSubstring(searchCond.as_str(), lower(*slot).as_str()),
                    );
                }
            }
        }
        // L721
        if let Some(rarityCond) = tag.rarityCond {
            // L722
            for (_, item) in &items {
                // L723
                matches.push(item.rarity.get() == rarityCond);
            }
        }
        // L726 (Lua's `item.corrupted` is `true` or nil: Item.lua:551)
        if let Some(corruptedCond) = tag.corruptedCond.filter(|c| *c) {
            // L727
            for (_, item) in &items {
                // L728
                matches.push(item.corrupted.get() == corruptedCond);
            }
        }
        // L731 (`item.shaper` / `item.elder`: the influence keys, Item.lua:104/560/1234)
        if let Some(shaperCond) = tag.shaperCond.filter(|c| *c) {
            // L732
            for (_, item) in &items {
                // L733
                matches.push(
                    item.influences
                        .get(n!("shaper"))
                        .raw_eq(&Val::Bool(shaperCond)),
                );
            }
        }
        // L736
        if let Some(elderCond) = tag.elderCond.filter(|c| *c) {
            // L737
            for (_, item) in &items {
                // L738
                matches.push(
                    item.influences
                        .get(n!("elder"))
                        .raw_eq(&Val::Bool(elderCond)),
                );
            }
        }
        // L741
        if let Some(nameCond) = &tag.nameCond {
            let nameCond = lower(Name::from(nameCond.as_ref()));
            // L742
            for (_, item) in &items {
                // L743
                matches.push(lower(Name::from(item.name.get().as_ref())) == nameCond);
            }
        }
        // L746-750
        let hasItems = !items.is_empty();

        // L752
        let mut r#match = true;
        // L753
        for b in matches {
            // L754
            if b == tag.neg {
                // L755
                r#match = false;
                // L756
                break;
            }
        }
        // L759
        if !r#match || (!hasItems && !tag.neg) {
            // L760
            return false;
        }
        true
    }
}

// ---- unit tests (MS-B; the oracle comparison is crates/pob-oracle/tests/MS-B.rs) ----
#[cfg(test)]
mod tests {
    use super::*;
    use pob_gamedata::tag;

    fn md(name: &str, ty: ModType, value: impl Into<Value>, tags: Vec<Tag>) -> Mod {
        Mod {
            name: Name::from(name),
            r#type: ty,
            value: value.into(),
            flags: 0,
            keywordFlags: 0,
            source: Some(AStr::from("Test")),
            tags: tags.into_iter().collect(),
            extra: None,
        }
    }

    fn mult(db: &ModStore, var: &str, v: f64) {
        db.multipliers.set(Name::from(var), v);
    }

    #[test]
    fn untagged_mod_returns_its_value() {
        let db = ModStore::ModDB(None);
        let m = md("X", ModType::BASE, 7.0, vec![]);
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(7.0));
    }

    #[test]
    fn multiplier_floor_div_limit_base_invert() {
        let db = ModStore::ModDB(None);
        mult(&db, "A", 7.0);
        let m = md(
            "X",
            ModType::BASE,
            2.0,
            vec![tag!({ type = Multiplier, var = "A", div = 2.0 })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(6.0)); // floor(7/2)=3 → 2*3
        let m = md(
            "X",
            ModType::BASE,
            2.0,
            vec![tag!({ type = Multiplier, var = "A", div = 2.0, noFloor = true })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(7.0));
        let m = md(
            "X",
            ModType::BASE,
            2.0,
            vec![tag!({ type = Multiplier, var = "A", limit = 2.0, base = 1.0 })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(5.0));
        let m = md(
            "X",
            ModType::BASE,
            2.0,
            vec![tag!({ type = Multiplier, var = "A", limit = 9.0, limitTotal = true })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(9.0));
        let m = md(
            "X",
            ModType::BASE,
            14.0,
            vec![tag!({ type = Multiplier, var = "A", invert = true })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(2.0));
    }

    /// calc doc §5.3: EvalMod never mutates a tag. Lua writes `tag.div` (ModStore.lua:401); the
    /// port reads the local, so evaluating the same mod in two stores with different `divVar`
    /// multipliers gives independent results and leaves the tag as it was.
    #[test]
    fn div_var_is_local_and_tag_unchanged() {
        let a = ModStore::ModDB(None);
        let b = ModStore::ModDB(None);
        mult(&a, "Rage", 12.0);
        mult(&b, "Rage", 12.0);
        mult(&a, "Per", 3.0);
        mult(&b, "Per", 4.0);
        let m = md(
            "X",
            ModType::BASE,
            1.0,
            vec![tag!({ type = Multiplier, var = "Rage", divVar = "Per" })],
        );
        let before = m.clone();
        assert_eq!(a.EvalMod(&m, None, None), Value::Num(4.0));
        assert_eq!(b.EvalMod(&m, None, None), Value::Num(3.0));
        assert_eq!(a.EvalMod(&m, None, None), Value::Num(4.0));
        assert_eq!(m, before);
        assert_eq!(m.tags[0].div, None);
    }

    #[test]
    fn table_values_are_copied_not_mutated() {
        let db = ModStore::ModDB(None);
        mult(&db, "A", 3.0);
        let inner = Arc::new(md("Life", ModType::BASE, 10.0, vec![]));
        let mut t = ValueTable::new();
        t.set(n!("mod"), Value::Mod(inner.clone()));
        let m = md(
            "MinionModifier",
            ModType::LIST,
            t,
            vec![tag!({ type = Multiplier, var = "A" })],
        );
        let v = db.EvalMod(&m, None, None);
        let got = v.inner_mod().unwrap();
        assert_eq!(got.value, Value::Num(30.0));
        assert_eq!(inner.value, Value::Num(10.0));
        let mut t = ValueTable::new();
        t.set(n!("key"), Value::from("K"));
        t.set(n!("value"), Value::Num(2.0));
        let m = md(
            "Misc",
            ModType::LIST,
            t,
            vec![tag!({ type = PercentStat, stat = "Life", percent = 50.0 })],
        );
        // no actor: GetStat reads nothing (MS-A) → base 0 → ceil(2*0) = 0
        let v = db.EvalMod(&m, None, None);
        assert_eq!(v.get(n!("value")), &Value::Num(0.0));
        assert_eq!(v.get(n!("key")), &Value::from("K"));
    }

    #[test]
    fn conditions_and_negation() {
        let db = ModStore::ModDB(None);
        db.conditions.set(n!("Yes"), true);
        let m = md(
            "X",
            ModType::BASE,
            1.0,
            vec![tag!({ type = Condition, var = "Yes" })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(1.0));
        let m = md(
            "X",
            ModType::BASE,
            1.0,
            vec![tag!({ type = Condition, var = "No" })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Nil);
        let m = md(
            "X",
            ModType::BASE,
            1.0,
            vec![tag!({ type = Condition, var = "No", neg = true })],
        );
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(1.0));
        // cfg.skillCond
        let cfg = ModCfg::new();
        let sc = Table::new();
        sc.set(n!("No"), true);
        cfg.skillCond.set(Some(sc));
        let m = md(
            "X",
            ModType::BASE,
            1.0,
            vec![tag!({ type = Condition, var = "No" })],
        );
        assert_eq!(db.EvalMod(&m, Some(&cfg), None), Value::Num(1.0));
        // a nil var raises (conditionName[nil])
        let m = md("X", ModType::BASE, 1.0, vec![Tag::new(TagType::Condition)]);
        assert!(pob_lua::error::catch(|| db.EvalMod(&m, None, None)).is_err());
    }

    #[test]
    fn actor_condition_and_get_actor() {
        let player = Actor::new();
        let enemy = Actor::new();
        let pdb = ModStore::ModDB(None);
        let edb = ModStore::ModDB(None);
        pdb.actor.set(Rc::downgrade(&player));
        edb.actor.set(Rc::downgrade(&enemy));
        player.modDB.set(pdb.clone());
        enemy.modDB.set(edb.clone());
        player.enemy.set(Rc::downgrade(&enemy));
        enemy.enemy.set(Rc::downgrade(&player));
        player.player.set(Rc::downgrade(&player));
        enemy.player.set(Rc::downgrade(&player));
        edb.conditions.set(n!("Shocked"), true);
        let m = md(
            "X",
            ModType::INC,
            5.0,
            vec![tag!({ type = ActorCondition, actor = "enemy", var = "Shocked" })],
        );
        assert_eq!(pdb.EvalMod(&m, None, None), Value::Num(5.0));
        assert_eq!(edb.EvalMod(&m, None, None), Value::Nil); // enemy.enemy = player, not shocked
        assert!(Rc::ptr_eq(
            &edb.getActor(Some(n!("player"))).unwrap(),
            &player
        ));
        // `tag.actor == cfg.actor` without var
        let m = md(
            "X",
            ModType::INC,
            5.0,
            vec![tag!({ type = ActorCondition, actor = "nobody" })],
        );
        let cfg = ModCfg::new();
        cfg.actor.set(Some(n!("nobody")));
        assert_eq!(pdb.EvalMod(&m, Some(&cfg), None), Value::Num(5.0));
        assert_eq!(pdb.EvalMod(&m, None, None), Value::Nil);
        // a store without an actor: getActor is nil → Multiplier with an actor returns nil
        let lone = ModStore::ModList(None);
        let m = md(
            "X",
            ModType::BASE,
            1.0,
            vec![tag!({ type = Multiplier, var = "A", actor = "enemy" })],
        );
        assert_eq!(lone.EvalMod(&m, None, None), Value::Nil);
    }

    #[test]
    fn global_limits_cap_the_running_total() {
        let db = ModStore::ModDB(None);
        let mut t = Tag::new(TagType::Global);
        t.globalLimit = Some(10.0);
        t.globalLimitKey = Some(n!("K"));
        let m = md("X", ModType::BASE, 6.0, vec![t]);
        let mut gl = GlobalLimits::default();
        assert_eq!(db.EvalMod(&m, None, Some(&mut gl)), Value::Num(6.0));
        assert_eq!(db.EvalMod(&m, None, Some(&mut gl)), Value::Num(4.0));
        assert_eq!(db.EvalMod(&m, None, Some(&mut gl)), Value::Num(0.0));
        assert_eq!(gl.get(&n!("K")).copied(), Some(10.0));
        // without globalLimits the value is untouched
        assert_eq!(db.EvalMod(&m, None, None), Value::Num(6.0));
    }

    #[test]
    fn skill_type_part_slot_flags() {
        let db = ModStore::ModDB(None);
        let cfg = ModCfg::new();
        let st = Shared::new(SkillTypeSet::default());
        st.with_mut(|s| s.set(SkillType::Attack, true));
        cfg.skillTypes.set(Some(st));
        cfg.skillPart.set(Some(2.0));
        cfg.slotName.set(Some(n!("Weapon 1")));
        cfg.flags.set(ModFlag::Attack);
        cfg.keywordFlags.set(KeywordFlag::Hit | KeywordFlag::Fire);
        let ok = |t: Tag| {
            db.EvalMod(&md("X", ModType::BASE, 1.0, vec![t]), Some(&cfg), None)
                .truthy()
        };
        assert!(ok(
            tag!({ type = SkillType, skillType = SkillType::Attack })
        ));
        assert!(!ok(
            tag!({ type = SkillType, skillType = SkillType::Spell })
        ));
        assert!(ok(tag!({ type = SkillPart, skillPart = 2.0 })));
        assert!(!ok(tag!({ type = SkillPart, skillPart = 1.0 })));
        assert!(ok(tag!({ type = SlotName, slotName = "Weapon 1" })));
        assert!(ok(
            tag!({ type = SlotName, slotName = "Weapon 2", neg = true })
        ));
        assert!(ok(
            tag!({ type = ModFlagOr, modFlags = ModFlag::Attack | ModFlag::Spell })
        ));
        assert!(!ok(tag!({ type = ModFlagOr, modFlags = ModFlag::Spell })));
        assert!(ok(
            tag!({ type = KeywordFlagAnd, keywordFlags = KeywordFlag::Hit | KeywordFlag::Fire })
        ));
        assert!(!ok(
            tag!({ type = KeywordFlagAnd, keywordFlags = KeywordFlag::Hit | KeywordFlag::Cold })
        ));
        // no cfg
        let none = |t: Tag| {
            db.EvalMod(&md("X", ModType::BASE, 1.0, vec![t]), None, None)
                .truthy()
        };
        assert!(!none(tag!({ type = SkillPart, skillPart = 2.0 })));
        assert!(none(
            tag!({ type = SkillType, skillType = SkillType::Attack, neg = true })
        ));
    }

    #[test]
    fn distance_ramp_and_melee_proximity() {
        let db = ModStore::ModDB(None);
        let cfg = ModCfg::new();
        let ramp = |sd: f64, t: Tag| {
            cfg.skillDist.set(Some(sd));
            db.EvalMod(&md("X", ModType::MORE, 10.0, vec![t]), Some(&cfg), None)
        };
        let pts = || {
            let mut t = Tag::new(TagType::DistanceRamp);
            t.ramp = Some(Ramp::Points(Arc::from(vec![[10.0, 1.0], [35.0, 0.0]])));
            t
        };
        assert_eq!(ramp(5.0, pts()), Value::Num(10.0));
        assert_eq!(ramp(40.0, pts()), Value::Num(0.0));
        assert_eq!(
            ramp(22.5, pts()),
            Value::Num(10.0 * (1.0 + (0.0 - 1.0) * (22.5 - 10.0) / (35.0 - 10.0)))
        );
        let flat = || {
            let mut t = Tag::new(TagType::MeleeProximity);
            t.ramp = Some(Ramp::Flat(Arc::from(vec![1.0, 0.0])));
            t
        };
        assert_eq!(ramp(10.0, flat()), Value::Num(10.0));
        assert_eq!(ramp(15.5, flat()), Value::Num(10.0)); // LUA-QUIRK gap between 15 and 16
        assert_eq!(
            ramp(20.0, flat()),
            Value::Num(10.0 * (1.0 - ((1.0 / 25.0) * 5.0)))
        );
        assert_eq!(ramp(45.0, flat()), Value::Num(0.0));
        assert_eq!(
            db.EvalMod(&md("X", ModType::MORE, 10.0, vec![flat()]), None, None),
            Value::Nil
        );
    }

    #[test]
    fn stat_threshold_stat_list_quirk_raises() {
        let db = ModStore::ModDB(None);
        let mut t = Tag::new(TagType::StatThreshold);
        t.statList = Some(Arc::from(vec![n!("Str"), n!("Dex")]));
        t.threshold = Some(0.0);
        let m = md("X", ModType::BASE, 1.0, vec![t]);
        assert!(pob_lua::error::catch(|| db.EvalMod(&m, None, None)).is_err());
    }

    #[test]
    fn item_slot_name_normalisation() {
        assert_eq!(
            item_slot_name(n!("ring {OtherSlotNum}")).as_str(),
            "Ring {Otherslotnum}"
        );
        assert_eq!(item_slot_name(n!("  body armour ")).as_str(), "Body Armour");
        assert_eq!(item_slot_name(n!("WEAPON 1")).as_str(), "Weapon 1");
    }
}
