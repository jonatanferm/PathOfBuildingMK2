//! @pob-file Modules/CalcPerform.lua @16de4b82d
//! CalcPerform.lua 1-635: module header and actor helpers A.
//!
//! Owner: PF-A. Signatures are U0's (frozen); two additions are marked
//! `// CCR-0085` (docs/port/ccr/0085-pf-a-perform-helpers.md):
//! [`mergeBuff_v`] takes the `src` argument as the Lua value it is (a ModList
//! object or a plain list of mods; the frozen `mergeBuff` takes a `Table`),
//! and the `actor.appliedEnemyModifiers` mod-identity cache is kept beside the
//! frozen `Table` field (CCR-0030 #6).
//!
//! [`probe_call`] (feature `probe`) runs one of these functions on injected
//! function-probe locals, for the PF-A acceptance test.
#![allow(non_snake_case)]
// The callers are the PF-C..PF-G segments (still stubs until those units land).
#![allow(dead_code)]
// Lua statement shapes are kept (`local x` then assigned per branch, `if … then … end` chains).
#![allow(
    clippy::needless_late_init,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::nonminimal_bool,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::float_cmp
)]

use crate::modules::{calc_tools as calcLib, common::cacheSkillUUID};
use crate::prelude::*;
use pob_gamedata::{mod_tools as modLib, ValueTable};

// @pob Modules/CalcPerform.lua:7-22 h=d1d8c60a
// (module-level statements: calcs,pairs,ipairs,t_insert,t_remove,m_min,m_max,m_ceil,m_floor,m_modf,s_format,m_huge,bor,band,bnot)

// ---- Rust-only plumbing -----------------------------------------------------------------------

/// `actor.mainSkill` where Lua indexes it (nil raises).
fn mainSkill_of(actor: &Actor) -> ActiveSkillRef {
    actor
        .mainSkill
        .get()
        .unwrap_or_else(|| lua_error!("attempt to index field 'mainSkill' (a nil value)"))
}

/// `actor.itemList[slot]`.
fn item_in(actor: &Actor, slot: Name) -> Option<ItemRef> {
    actor.itemList.with(|l| l.get(&slot).cloned())
}

/// `env.data.weaponTypeInfo[t]` where Lua indexes the result (nil raises).
fn weaponTypeInfo(t: &Val) -> &'static pob_gamedata::WeaponTypeInfo {
    let info = match t {
        Val::Str(s) => data().weaponTypeInfo.get(&Name::from(s)),
        _ => None,
    };
    info.unwrap_or_else(|| lua_error!("attempt to index local 'info' (a nil value)"))
}

/// A Lua string value as `&str` for a method call (`s:match(…)`; nil raises).
fn str_of(v: &Val, what: &str) -> LStr {
    v.str()
        .unwrap_or_else(|| lua_error!("attempt to index field '{what}' (a {} value)", v.lua_type()))
}

/// Probe builds only: a typed actor field Lua just assigned. Injection marks the typed
/// fields that were nil in Lua as absent, and the exporter hides an absent field that holds
/// its default (`strDmgBonus = 0` on a minion, an empty `appliedEnemyModifiers`), so a
/// write must clear the mark. A no-op without the `probe` feature.
#[inline]
fn probe_written(actor: &ActorRef, key: &str) {
    #[cfg(feature = "probe")]
    crate::probe::unmark_absent(crate::probe::rc_ptr(actor), key);
    #[cfg(not(feature = "probe"))]
    let _ = (actor, key);
}

/// `{ n = select("#", …), … }`: the `ret` local a function probe's `:out` dumps.
#[cfg_attr(not(feature = "probe"), allow(dead_code))]
fn ret_table(vals: &[Val]) -> Table {
    let t = Table::new();
    t.set(n!("n"), vals.len() as f64);
    for (i, v) in vals.iter().enumerate() {
        t.set(i + 1, v.clone());
    }
    t
}

/// @pob Modules/CalcPerform.lua:31-42 h=1053e69a
/// `getCachedOutputValue(env, activeSkill, ...)`: the cached outputs named, as values.
pub(crate) fn getCachedOutputValue(
    env: &EnvRef,
    activeSkill: &ActiveSkillRef,
    names: &[Name],
) -> Vec<Val> {
    // L32
    let uuid = cacheSkillUUID(activeSkill, env);
    // L33
    if cachedData(env.mode, uuid).is_none() || env.mode == Mode::CALCULATOR {
        // L34
        crate::calcs::buildActiveSkill(env, env.mode, activeSkill, Some(uuid), &[uuid]);
    }

    // L37
    let tempValues = Table::new();
    // L38 (`{...}`: the names; ipairs stops at none of them)
    for (i, v) in names.iter().enumerate() {
        // L39
        let entry = cachedData(env.mode, uuid)
            .unwrap_or_else(|| lua_error!("attempt to index a nil value"));
        let val = entry.Env.player.output.get().get(*v);
        tempValues.set(i + 1, val);
    }
    // L41 `unpack(tempValues)`: values 1..#tempValues
    (1..=tempValues.len()).map(|i| tempValues.get(i)).collect()
}

/// @pob Modules/CalcPerform.lua:45-67 h=ff615531
/// `mergeBuff(src, destTable, destKey)`: `src` and the merged buff are buff mod tables.
/// Merge an instance of a buff, taking the highest value of each modifier
pub(crate) fn mergeBuff(src: &Table, destTable: &Table, destKey: Name) {
    mergeBuff_v(&Val::Table(src.clone()), destTable, destKey);
}

/// `mergeBuff` with `src` as the Lua value it is: a ModList object (`srcList`, every
/// caller but two) or a plain list of mods (`buff.modList`, CalcPerform.lua:2808/2832;
/// `guard.modList`, 3339). CCR-0085: the frozen signature takes a `Table` only.
/// (Body of CalcPerform.lua:45-67; marker on [`mergeBuff`].)
pub(crate) fn mergeBuff_v(src: &Val, destTable: &Table, destKey: Name) {
    // L46
    if !destTable.truthy(destKey) {
        // L47
        let l: Rc<dyn std::any::Any> = ModStore::ModList(None);
        destTable.set(destKey, Val::Obj(l));
    }
    // L49
    let dest = destTable.get(destKey);
    // L50 `ipairs(src)`: src is not written during the loop (a snapshot is the live read)
    let srcMods: Vec<ModRef> = match src {
        Val::Obj(_) => match src.obj::<ModStore>() {
            Some(l) => l.ipairs(),
            None => lua_error!("bad argument #1 to 'ipairs' (table expected, got userdata)"),
        },
        Val::Table(t) => {
            let mut v = Vec::new();
            for_ipairs!((_, m) in t => {
                match m {
                    Val::Mod(m) => v.push(m),
                    other => lua_error!("mergeBuff: src entry is a {} (a mod expected)", other.lua_type()),
                }
            });
            v
        }
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    };
    for r#mod in srcMods {
        // L51
        let mut r#match = false;
        // L52
        if r#mod.r#type != ModType::LIST {
            // L53
            let destMods = dest_ipairs(&dest);
            for (index, destMod) in destMods.iter().enumerate() {
                // L54
                if modLib::compareModParams(&r#mod, destMod) {
                    // L55
                    if let Value::Num(d) = destMod.value {
                        let m = match r#mod.value {
                            Value::Num(m) => m,
                            ref other => {
                                lua_error!("attempt to compare number with {}", value_type(other))
                            }
                        };
                        if m > d {
                            // L56
                            dest_set(&dest, index + 1, r#mod.clone());
                        }
                    }
                    // L58
                    r#match = true;
                    // L59
                    break;
                }
            }
        }
        // L63
        if !r#match {
            // L64
            dest_push(&dest, r#mod.clone());
        }
    }
}

/// Lua `type(v)` of a data value.
fn value_type(v: &Value) -> &'static str {
    match v {
        Value::Nil => "nil",
        Value::Bool(_) => "boolean",
        Value::Num(_) => "number",
        Value::Str(_) => "string",
        _ => "table",
    }
}

/// `ipairs(dest)` over the merged buff: a ModList (`new("ModList")`, L47) or a plain list.
fn dest_ipairs(dest: &Val) -> Vec<ModRef> {
    if let Some(l) = dest.obj::<ModStore>() {
        return l.ipairs();
    }
    match dest {
        Val::Table(t) => t
            .ipairs()
            .into_iter()
            .filter_map(|(_, v)| match v {
                Val::Mod(m) => Some(m),
                _ => None,
            })
            .collect(),
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `dest[index] = mod` (1-based; `index` is an existing slot whose mod has the same
/// name, `compareModParams` checked it). CCR-0085: ModList has no public `self[i] = mod`;
/// the name → positions index is unchanged because the names are equal.
fn dest_set(dest: &Val, index: usize, m: ModRef) {
    if let Some(l) = dest.obj::<ModStore>() {
        match &mut *l.mods.borrow_mut() {
            crate::classes::StoreMods::List { list, .. } => {
                debug_assert_eq!(list[index - 1].name, m.name);
                list[index - 1] = m;
            }
            crate::classes::StoreMods::DB(_) => unreachable!("ipairs over a ModDB is empty"),
        }
        return;
    }
    if let Val::Table(t) = dest {
        t.set(index, Val::Mod(m));
    }
}

/// `t_insert(dest, mod)`.
fn dest_push(dest: &Val, m: ModRef) {
    if let Some(l) = dest.obj::<ModStore>() {
        if l.kind == StoreKind::ModList {
            l.AddMod(m);
            return;
        }
        // LUA-QUIRK: t_insert on a ModDB object puts the mod in its array part, which no
        // query reads (never happens in PoB: every dest is a `new("ModList")`).
        return;
    }
    match dest {
        Val::Table(t) => t.push(Val::Mod(m)),
        other => lua_error!(
            "bad argument #1 to 'insert' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// @pob Modules/CalcPerform.lua:69-133 h=b6d0f139
/// Global `doActorLifeMana(actor)`.
pub fn doActorLifeMana(actor: &ActorRef) {
    // L70
    let modDB = actor.modDB.get();
    // L72
    let output = actor.output.get();
    // L74
    let breakdown = actor.breakdown.get();
    // L75
    let condList = modDB.conditions.clone();

    // L77
    let lowLifePerc = modDB.Sum(ModType::BASE, None, nm!["LowLifePercentage"]);
    // L78
    output.set(
        n!("LowLifePercentage"),
        100.0
            * (if lowLifePerc > 0.0 {
                lowLifePerc
            } else {
                data().misc.LowPoolThreshold
            }),
    );
    // L79
    let fullLifePerc = modDB.Sum(ModType::BASE, None, nm!["FullLifePercentage"]);
    // L80
    output.set(
        n!("FullLifePercentage"),
        100.0
            * (if fullLifePerc > 0.0 {
                fullLifePerc
            } else {
                1.0
            }),
    );

    // L82 (Flag returns true or nil)
    let ci = modDB.Flag(None, nm!["ChaosInoculation"]);
    output.set(
        n!("ChaosInoculation"),
        if ci { Val::Bool(true) } else { Val::Nil },
    );
    // -- Life/mana pools
    // L84
    if output.truthy(n!("ChaosInoculation")) {
        // L85
        output.set(n!("Life"), 1.0);
        // L86
        condList.set(n!("FullLife"), true);
    } else {
        // L88
        let base = modDB.Sum(ModType::BASE, None, nm!["Life"]);
        // L89
        let inc = modDB.Sum(ModType::INC, None, nm!["Life"]);
        // L90
        let more = modDB.More(None, nm!["Life"]);
        // L91
        let r#override = Val::from_value(&modDB.Override(None, nm!["Life"]));
        // L92
        let conv = modDB.Sum(ModType::BASE, None, nm!["LifeConvertToEnergyShield"]);
        // L93
        let life = lor(r#override.clone(), || {
            Val::Num(m_max(
                round(
                    base * (1.0 + inc / 100.0) * more * (1.0 - conv / 100.0),
                    None,
                ),
                1.0,
            ))
        });
        output.set(n!("Life"), life);
        // L94
        if let Some(breakdown) = &breakdown {
            // L95
            if inc != 0.0 || more != 1.0 || conv != 0.0 || r#override.truthy() {
                // L96
                let t = Table::new();
                breakdown.set(n!("Life"), t.clone());
                // L97
                t.set(1_i64, lfmt!("%g ^8(base)", base));
                // L98
                if inc != 0.0 {
                    // L99
                    t.push(lfmt!("x %.2f ^8(increased/reduced)", 1.0 + inc / 100.0));
                }
                // L101
                if more != 1.0 {
                    // L102
                    t.push(lfmt!("x %.2f ^8(more/less)", more));
                }
                // L104
                if r#override.truthy() {
                    // L105
                    t.push(fmt_val("= %g ^8(life override)", &r#override));
                }
                // L107
                t.push(fmt_val("= %g", &output.get(n!("Life"))));
            }
        }
    }
    // L111
    let manaConv = modDB.Sum(ModType::BASE, None, nm!["ManaConvertToArmour"]);
    // L112
    output.set(
        n!("Mana"),
        round(
            calcLib::val(&modDB, n!("Mana"), None) * (1.0 - manaConv / 100.0),
            None,
        ),
    );
    // L113
    let base = modDB.Sum(ModType::BASE, None, nm!["Mana"]);
    // L114
    let inc = modDB.Sum(ModType::INC, None, nm!["Mana"]);
    // L115
    let more = modDB.More(None, nm!["Mana"]);
    // L116
    if let Some(breakdown) = &breakdown {
        // L117
        if inc != 0.0 || more != 1.0 || manaConv != 0.0 {
            // L118
            let t = Table::new();
            breakdown.set(n!("Mana"), t.clone());
            // L119
            t.set(1_i64, lfmt!("%g ^8(base)", base));
            // L120
            if inc != 0.0 {
                // L121
                t.push(lfmt!("x %.2f ^8(increased/reduced)", 1.0 + inc / 100.0));
            }
            // L123
            if more != 1.0 {
                // L124
                t.push(lfmt!("x %.2f ^8(more/less)", more));
            }
            // L126
            if manaConv != 0.0 {
                // L127
                t.push(lfmt!(
                    "x %.2f ^8(converted to Armour)",
                    1.0 - manaConv / 100.0
                ));
            }
            // L129
            t.push(lfmt!("= %g", output.num(n!("Mana"))));
        }
    }
    // L132
    let lowest = m_min(output.num(n!("Life")), output.num(n!("Mana")));
    output.set(n!("LowestOfMaximumLifeAndMaximumMana"), lowest);
}

/// `s_format(fmt, v)` with one Lua value (a number, or a string Lua coerces).
fn fmt_val(fmt: &str, v: &Val) -> String {
    match v {
        Val::Num(n) => lfmt!(fmt, *n),
        Val::Str(s) => lfmt!(fmt, &**s),
        other => lua_error!(
            "bad argument #2 to 'format' (number expected, got {})",
            other.lua_type()
        ),
    }
}

/// @pob Modules/CalcPerform.lua:138-547 h=a56d79fa
/// Calculate attributes, and set conditions
pub(crate) fn doActorAttribsConditions(env: &EnvRef, actor: &ActorRef) {
    // L139
    let modDB = actor.modDB.get();
    // L141
    let output = actor.output.get();
    // L143
    let breakdown = actor.breakdown.get();
    // L144
    let condList = modDB.conditions.clone();

    // -- Set conditions
    // L147
    let weapon2 = item_in(actor, n!("Weapon 2"));
    if (weapon2
        .as_ref()
        .is_some_and(|i| i.r#type.get() == n!("Shield")))
        || (Rc::ptr_eq(actor, &env.player) && env.aegisModList.get().is_some())
    {
        // L148
        condList.set(n!("UsingShield"), true);
    } else if weapon2.is_none() {
        // L150
        condList.set(n!("OffHandIsEmpty"), true);
    }
    let weaponData1 = actor.weaponData1.get();
    // L152
    if weaponData1.get(n!("type")).str().as_deref() == Some("None") {
        // L153
        condList.set(n!("Unarmed"), true);
        // L154
        if item_in(actor, n!("Weapon 2")).is_none() && item_in(actor, n!("Gloves")).is_none() {
            // L155
            condList.set(n!("Unencumbered"), true);
        }
    } else {
        // L158
        let info = weaponTypeInfo(&weaponData1.get(n!("type")));
        // L159
        condList.set(cat!("Using", info.flag), true);
        // L160
        if weaponData1.truthy(n!("countsAsAll1H")) {
            countsAsAll1H(&weaponData1, &condList);
        }
        // L174
        if info.melee {
            // L175
            condList.set(n!("UsingMeleeWeapon"), true);
        }
        // L177
        if info.oneHand {
            // L178
            condList.set(n!("UsingOneHandedWeapon"), true);
        } else {
            // L180
            condList.set(n!("UsingTwoHandedWeapon"), true);
        }
    }
    // L183
    let armourSlots = [n!("Helmet"), n!("Body Armour"), n!("Gloves"), n!("Boots")];
    // L184
    for slotName in armourSlots {
        // L185
        if item_in(actor, slotName).is_some() {
            // L186
            condList.set(cat!("Using", slotName), true);
        }
    }
    // L189-194
    for slot in [n!("Graft 1"), n!("Graft 2")] {
        if let Some(baseName) = item_in(actor, slot).and_then(|i| i.baseName.get()) {
            let cap = pob_lua::lmatch(&baseName, "%s(%S+)", None);
            let suffix = cap
                .as_ref()
                .and_then(|c| c.first())
                .map(|c| c.to_str().into_owned())
                .unwrap_or_else(|| lua_error!("attempt to concatenate a nil value"));
            condList.set(cat!("Using", suffix.as_str()), true);
        }
    }
    let weaponData2 = actor.weaponData2.get();
    // L195
    if weaponData2.truthy(n!("type")) {
        // L196
        let info = weaponTypeInfo(&weaponData2.get(n!("type")));
        // L197
        condList.set(cat!("Using", info.flag), true);
        // L198
        if weaponData2.truthy(n!("countsAsAll1H")) {
            countsAsAll1H(&weaponData2, &condList);
        }
        // L212
        if info.melee {
            // L213
            condList.set(n!("UsingMeleeWeapon"), true);
        }
        // L215
        if info.oneHand {
            // L216
            condList.set(n!("UsingOneHandedWeapon"), true);
        } else {
            // L218
            condList.set(n!("UsingTwoHandedWeapon"), true);
        }
    }
    // L221
    if weaponData1.truthy(n!("type")) && weaponData2.truthy(n!("type")) {
        // L222
        condList.set(n!("DualWielding"), true);
        let is = |wd: &Table, t: &str| {
            wd.get(n!("type")).str().as_deref() == Some(t) || wd.truthy(n!("countsAsAll1H"))
        };
        // L223
        if is(&weaponData1, "Claw") && is(&weaponData2, "Claw") {
            // L224
            condList.set(n!("DualWieldingClaws"), true);
        }
        // L226
        if is(&weaponData1, "Dagger") && is(&weaponData2, "Dagger") {
            // L227
            condList.set(n!("DualWieldingDaggers"), true);
        }
        // L229
        let getWeaponType = |weaponData: &Table| -> Val {
            // -- GGG treats thrusting one handed swords as the same class as one handed swords
            // L231
            if weaponData.get(n!("type")).str().as_deref() == Some("One Handed Sword")
                && weaponData.get(n!("subType")).str().as_deref() == Some("Thrusting")
            {
                // L232
                return Val::from("One Handed Sword");
            }
            // L234
            let label = weaponTypeInfo(&weaponData.get(n!("type"))).label;
            lor(label.map_or(Val::Nil, Val::from), || {
                lor(weaponData.get(n!("subType")), || weaponData.get(n!("type")))
            })
        };
        // L236
        if !getWeaponType(&weaponData1).raw_eq(&getWeaponType(&weaponData2)) {
            // L237
            let info1 = weaponTypeInfo(&weaponData1.get(n!("type")));
            // L238
            let info2 = weaponTypeInfo(&weaponData2.get(n!("type")));
            // L239
            if info1.oneHand && info2.oneHand {
                // L240
                condList.set(n!("WieldingDifferentWeaponTypes"), true);
            }
        }
    }
    // L244
    if env.mode_combat.get() {
        let mainSkill = mainSkill_of(actor);
        let skillFlags = mainSkill.skillFlags.get();
        let skillTypes = mainSkill.skillTypes.get();
        let flag = |k: Name| skillFlags.truthy(k);
        // L245
        if !mainSkill.skillData.get().truthy(n!("triggered"))
            && !flag(n!("trap"))
            && !flag(n!("mine"))
            && !flag(n!("totem"))
        {
            // L246
            if flag(n!("attack")) {
                // L247
                condList.set(n!("AttackedRecently"), true);
            } else if flag(n!("spell")) {
                // L249
                condList.set(n!("CastSpellRecently"), true);
            }
            // L251
            if skillTypes.with(|s| s.has(SkillType::Movement)) {
                // L252
                condList.set(n!("UsedMovementSkillRecently"), true);
            }
            // L254
            if flag(n!("minion")) && !flag(n!("permanentMinion")) {
                // L255
                condList.set(n!("UsedMinionSkillRecently"), true);
            }
            // L257
            if skillTypes.with(|s| s.has(SkillType::Vaal)) {
                // L258
                condList.set(n!("UsedVaalSkillRecently"), true);
            }
            // L260
            if skillTypes.with(|s| s.has(SkillType::Channel)) {
                // L261
                condList.set(n!("Channelling"), true);
            }
        }
        // L264
        if flag(n!("hit")) && !flag(n!("trap")) && !flag(n!("mine")) && !flag(n!("totem")) {
            // L265
            condList.set(n!("HitRecently"), true);
            // L266
            if flag(n!("spell")) {
                // L267
                condList.set(n!("HitSpellRecently"), true);
            }
        }
        // L270
        if flag(n!("totem")) {
            // L271
            condList.set(n!("HaveTotem"), true);
            // L272
            condList.set(n!("SummonedTotemRecently"), true);
            // L273
            if flag(n!("hit")) {
                // L274
                condList.set(n!("TotemsHitRecently"), true);
                // L275
                if flag(n!("spell")) {
                    // L276
                    condList.set(n!("TotemsSpellHitRecently"), true);
                }
            }
        }
        // L280
        if flag(n!("mine")) {
            // L281
            condList.set(n!("DetonatedMinesRecently"), true);
        }
        // L283
        if flag(n!("trap")) {
            // L284
            condList.set(n!("TriggeredTrapsRecently"), true);
        }
        // `modDB:Flag(nil, "CritAlwaysAltAilments") and not modDB:Flag(env.player.mainSkill.skillCfg, "NeverCrit")`
        let critAlt = || {
            modDB.Flag(None, nm!["CritAlwaysAltAilments"]) && {
                let cfg = mainSkill_of(&env.player).skillCfg.get();
                !modDB.Flag(Some(&cfg), nm!["NeverCrit"])
            }
        };
        // L286
        if modDB.Sum(ModType::BASE, None, nm!["EnemyScorchChance"]) > 0.0
            || critAlt()
            || modDB.Flag(None, nm!["IgniteCanScorch"])
        {
            // L287
            condList.set(n!("CanInflictScorch"), true);
        }
        // L289
        if modDB.Sum(ModType::BASE, None, nm!["EnemyBrittleChance"]) > 0.0 || critAlt() {
            // L290
            condList.set(n!("CanInflictBrittle"), true);
        }
        // L292
        if modDB.Sum(ModType::BASE, None, nm!["EnemySapChance"]) > 0.0 || critAlt() {
            // L293
            condList.set(n!("CanInflictSap"), true);
        }
        // -- Shrine Buffs: Must be done before life pool calculated for massive shrine
        // L296
        let shrineEffectMod = 1.0
            + modDB.Sum(
                ModType::INC,
                None,
                nm!["BuffEffectOnSelf", "ShrineBuffEffect"],
            ) / 100.0;
        let new =
            |name: Name, ty: ModType, v: f64, src: &str| modDB.NewMod(name, ty, v, src, 0, 0, &[]);
        let flagged = |f: Name| modDB.Flag(None, &[f]);
        use ModType::{BASE, INC, MORE, OVERRIDE};
        // L297
        if flagged(n!("AccelerationShrine")) {
            // L298
            new(
                n!("ActionSpeed"),
                INC,
                m_floor(50.0 * shrineEffectMod),
                "Acceleration Shrine",
            );
            // L299
            new(
                n!("ProjectileSpeed"),
                INC,
                m_floor(80.0 * shrineEffectMod),
                "Acceleration Shrine",
            );
        }
        // L301
        if flagged(n!("BrutalShrine")) {
            // L302
            new(
                n!("Damage"),
                INC,
                m_floor(50.0 * shrineEffectMod),
                "Brutal Shrine",
            );
            // L303
            new(
                n!("EnemyStunDuration"),
                INC,
                m_floor(30.0 * shrineEffectMod),
                "Brutal Shrine",
            );
            // L304
            new(n!("EnemyKnockbackChance"), INC, 100.0, "Brutal Shrine");
        }
        // L306
        if flagged(n!("DiamondShrine")) {
            // L307
            new(n!("CritChance"), OVERRIDE, 100.0, "Diamond Shrine");
        }
        // L309
        if flagged(n!("DivineShrine")) {
            // L310
            new(n!("DamageTaken"), MORE, -100.0, "Divine Shrine");
        }
        // L312
        if flagged(n!("EchoingShrine")) {
            // L313
            modDB.NewMod(
                n!("Speed"),
                MORE,
                m_floor(100.0 * shrineEffectMod),
                "Echoing Shrine",
                ModFlag::Attack,
                0,
                &[],
            );
            // L314
            modDB.NewMod(
                n!("Speed"),
                MORE,
                m_floor(100.0 * shrineEffectMod),
                "Echoing Shrine",
                ModFlag::Cast,
                0,
                &[],
            );
            // L315
            new(
                n!("RepeatCount"),
                BASE,
                m_floor(1.0 * shrineEffectMod),
                "Echoing Shrine",
            );
        }
        // L317
        if flagged(n!("GloomShrine")) {
            // L318
            new(
                n!("NonChaosDamageGainAsChaos"),
                BASE,
                m_floor(10.0 * shrineEffectMod),
                "Gloom Shrine",
            );
        }
        // L320
        if flagged(n!("GreaterFreezingShrine")) {
            // L321
            new(
                n!("PhysicalDamageGainAsCold"),
                BASE,
                m_floor(30.0 * shrineEffectMod),
                "Greater Freezing Shrine",
            );
        }
        // L323
        if flagged(n!("GreaterShockingShrine")) {
            // L324
            new(
                n!("PhysicalDamageGainAsLightning"),
                BASE,
                m_floor(30.0 * shrineEffectMod),
                "Greater Shocking Shrine",
            );
        }
        // L326
        if flagged(n!("GreaterSkeletalShrine")) {
            // L327
            new(
                n!("PhysicalDamageGainAsChaos"),
                BASE,
                m_floor(30.0 * shrineEffectMod),
                "Greater Skeletal Shrine",
            );
        }
        // L329
        if flagged(n!("ImpenetrableShrine")) {
            // L330
            new(
                n!("Armour"),
                INC,
                m_floor(100.0 * shrineEffectMod),
                "Impenetrable Shrine",
            );
            // L331
            new(
                n!("Evasion"),
                INC,
                m_floor(100.0 * shrineEffectMod),
                "Impenetrable Shrine",
            );
            // L332
            new(
                n!("EnergyShield"),
                INC,
                m_floor(100.0 * shrineEffectMod),
                "Impenetrable Shrine",
            );
        }
        // L334
        if flagged(n!("MassiveShrine")) {
            // L335
            new(
                n!("Life"),
                INC,
                m_floor(40.0 * shrineEffectMod),
                "Massive Shrine",
            );
            // L336
            new(
                n!("AreaOfEffect"),
                INC,
                m_floor(40.0 * shrineEffectMod),
                "Massive Shrine",
            );
        }
        // L338
        if flagged(n!("ReplenishingShrine")) {
            // L339
            new(
                n!("ManaRegen"),
                INC,
                200.0 * shrineEffectMod,
                "Replenishing Shrine",
            );
            // L340
            new(
                n!("LifeRegenPercent"),
                BASE,
                6.7 * shrineEffectMod,
                "Replenishing Shrine",
            );
        }
        // L342
        if flagged(n!("ResistanceShrine")) {
            // L343
            new(
                n!("ElementalResist"),
                BASE,
                m_floor(50.0 * shrineEffectMod),
                "Resistance Shrine",
            );
            // L344
            new(
                n!("ElementalResistMax"),
                BASE,
                m_floor(10.0 * shrineEffectMod),
                "Resistance Shrine",
            );
            // L345
            new(
                n!("ChaosResistMax"),
                BASE,
                m_floor(10.0 * shrineEffectMod),
                "Resistance Shrine",
            );
        }
        // L347
        if flagged(n!("ResonatingShrine")) {
            // L348
            new(
                n!("PowerChargesMax"),
                BASE,
                m_floor(1.0 * shrineEffectMod),
                "Resonating Shrine",
            );
            // L349
            new(
                n!("FrenzyChargesMax"),
                BASE,
                m_floor(1.0 * shrineEffectMod),
                "Resonating Shrine",
            );
            // L350
            new(
                n!("EnduranceChargesMax"),
                BASE,
                m_floor(1.0 * shrineEffectMod),
                "Resonating Shrine",
            );
        }
        // L352
        if flagged(n!("LesserAccelerationShrine")) && !flagged(n!("AccelerationShrine")) {
            // L353
            new(
                n!("ActionSpeed"),
                INC,
                m_floor(10.0 * shrineEffectMod),
                "Lesser Acceleration Shrine",
            );
            // L354
            new(
                n!("ProjectileSpeed"),
                INC,
                m_floor(30.0 * shrineEffectMod),
                "Lesser Acceleration Shrine",
            );
        }
        // L356
        if flagged(n!("LesserBrutalShrine")) {
            // L357
            new(
                n!("Damage"),
                INC,
                m_floor(20.0 * shrineEffectMod),
                "Lesser Brutal Shrine",
            );
            // L358
            new(
                n!("EnemyStunDuration"),
                INC,
                m_floor(20.0 * shrineEffectMod),
                "Lesser Brutal Shrine",
            );
            // L359
            new(
                n!("EnemyKnockbackChance"),
                INC,
                100.0,
                "Lesser Brutal Shrine",
            );
        }
        // L361
        if flagged(n!("LesserImpenetrableShrine")) {
            // L362
            new(
                n!("Armour"),
                INC,
                m_floor(50.0 * shrineEffectMod),
                "Lesser Impenetrable Shrine",
            );
            // L363
            new(
                n!("Evasion"),
                INC,
                m_floor(50.0 * shrineEffectMod),
                "Lesser Impenetrable Shrine",
            );
            // L364
            new(
                n!("EnergyShield"),
                INC,
                m_floor(50.0 * shrineEffectMod),
                "Lesser Impenetrable Shrine",
            );
        }
        // L366
        if flagged(n!("LesserMassiveShrine")) {
            // L367
            new(
                n!("Life"),
                INC,
                m_floor(20.0 * shrineEffectMod),
                "Lesser Massive Shrine",
            );
            // L368
            new(
                n!("AreaOfEffect"),
                INC,
                m_floor(20.0 * shrineEffectMod),
                "Lesser Massive Shrine",
            );
        }
        // L370
        if flagged(n!("LesserReplenishingShrine")) {
            // L371
            new(
                n!("ManaRegen"),
                INC,
                100.0 * shrineEffectMod,
                "Lesser Replenishing Shrine",
            );
            // L372
            new(
                n!("LifeRegenPercent"),
                BASE,
                3.3 * shrineEffectMod,
                "Lesser Replenishing Shrine",
            );
        }
        // L374
        if flagged(n!("LesserResistanceShrine")) {
            // L375
            new(
                n!("ElementalResist"),
                BASE,
                m_floor(25.0 * shrineEffectMod),
                "Lesser Resistance Shrine",
            );
            // L376
            new(
                n!("ElementalResistMax"),
                BASE,
                m_floor(2.0 * shrineEffectMod),
                "Lesser Resistance Shrine",
            );
            // L377
            new(
                n!("ChaosResistMax"),
                BASE,
                m_floor(2.0 * shrineEffectMod),
                "Lesser Resistance Shrine",
            );
        }
        // L379
        if flagged(n!("BloodShrineOfRats")) {
            // L380
            new(
                n!("ElementalDamageGainAsChaos"),
                BASE,
                m_floor(30.0 * shrineEffectMod),
                "Blood Shrine of Rats",
            );
        }
        // L382
        if flagged(n!("BloodShrineOfLocusts")) {
            // L383
            new(
                n!("DotMultiplier"),
                BASE,
                m_floor(30.0 * shrineEffectMod),
                "Blood Shrine of Locusts",
            );
        }
        // L385
        if flagged(n!("BloodShrineOfToads")) {
            // L386
            new(
                n!("BloodShrineExplodingToadTriggerChance"),
                BASE,
                m_floor(25.0 * shrineEffectMod),
                "Blood Shrine of Toads",
            );
        }
        // L388
        if flagged(n!("BloodShrineOfCrows")) {
            // L389
            new(
                n!("ChaosResist"),
                BASE,
                m_floor(50.0 * shrineEffectMod),
                "Blood Shrine of Crows",
            );
            // L390
            new(
                n!("ElementalDamageFromHitsTakenAsChaos"),
                BASE,
                m_floor(10.0 * shrineEffectMod),
                "Blood Shrine of Crows",
            );
            // L391
            new(
                n!("PhysicalDamageFromHitsTakenAsChaos"),
                BASE,
                m_floor(10.0 * shrineEffectMod),
                "Blood Shrine of Crows",
            );
        }
        // L393
        if flagged(n!("BloodShrineOfBats")) {
            // L394
            new(
                n!("EnemyCurseLimit"),
                BASE,
                m_floor(1.0 * shrineEffectMod),
                "Blood Shrine of Bats",
            );
            // L395
            let curse = Value::Table(Arc::new(ValueTable {
                array: Vec::new(),
                fields: vec![
                    (n!("skillId"), Value::from("Enfeeble")),
                    (n!("level"), Value::Num(1.0)),
                ],
            }));
            modDB.NewMod(
                n!("ExtraCurse"),
                ModType::LIST,
                curse,
                "Blood Shrine of Bats",
                0,
                0,
                &[],
            );
        }
    }
    // L398
    if env.mode_effective.get() {
        let mainSkill = mainSkill_of(&env.player);
        let skillModList = mainSkill.skillModList.get();
        let skillCfg = mainSkill.skillCfg.get();
        // L399
        if skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["FireExposureChance"]) > 0.0
            || modDB.Sum(ModType::BASE, None, nm!["FireExposureChance"]) > 0.0
        {
            // L400
            condList.set(n!("CanApplyFireExposure"), true);
        }
        // L402
        if skillModList.Sum(ModType::BASE, Some(&skillCfg), nm!["ColdExposureChance"]) > 0.0
            || modDB.Sum(ModType::BASE, None, nm!["ColdExposureChance"]) > 0.0
        {
            // L403
            condList.set(n!("CanApplyColdExposure"), true);
        }
        // L405
        if skillModList.Sum(
            ModType::BASE,
            Some(&skillCfg),
            nm!["LightningExposureChance"],
        ) > 0.0
            || modDB.Sum(ModType::BASE, None, nm!["LightningExposureChance"]) > 0.0
        {
            // L406
            condList.set(n!("CanApplyLightningExposure"), true);
        }
    }

    // -- Calculate attributes
    // L411 `calculateAttributes` (a closure that runs where it is defined, D7)
    let calculateAttributes = || {
        // L412 -- Calculate twice because of circular dependency (X attribute higher than Y attribute)
        for _pass in 1..=2 {
            // L413
            for stat in [n!("Str"), n!("Dex"), n!("Int")] {
                // L414
                output.set(
                    stat,
                    m_max(round(calcLib::val(&modDB, stat, None), None), 0.0),
                );
                // L415
                if let Some(breakdown) = &breakdown {
                    // L416
                    let b = breakdown.simple(None, None, output.num(stat), &[stat]);
                    breakdown.set(stat, b);
                }
            }
            setAttributeConditions(&output, &condList);
        }
    };

    // L440 `calculateOmniscience(convert)` (`convert` is unused)
    let calculateOmniscience = || {
        // L441
        let tree = env.spec.get().tree.get();
        let characterData = tree.extra.get(n!("characterData"));
        let classId = env.classId.get();
        let classStats = lor(
            land(characterData.clone(), || characterData.get(classId)),
            || tree.classes.get(classId),
        );
        let base_of = |stat: Name| {
            classStats
                .get(cat!("base_", pob_lua::string::lower(&stat).as_str()))
                .num()
        };

        // L443 -- Calculate twice because of circular dependency (X attribute higher than Y attribute)
        for pass in 1..=2 {
            // L444
            if pass != 1 {
                // L445
                for stat in [n!("Str"), n!("Dex"), n!("Int")] {
                    // L446
                    let base = base_of(stat);
                    // L447
                    output.set(
                        stat,
                        m_min(round(calcLib::val(&modDB, stat, None), None), base),
                    );
                    // L448
                    if let Some(breakdown) = &breakdown {
                        // L449
                        let b = breakdown.simple(None, None, output.num(stat), &[stat]);
                        breakdown.set(stat, b);
                    }

                    // L452
                    modDB.NewMod(
                        n!("Omni"),
                        ModType::BASE,
                        modDB.Sum(ModType::BASE, None, &[stat]) - base,
                        &cat!(stat, " conversion Omniscience"),
                        0,
                        0,
                        &[],
                    );
                    // L453
                    modDB.NewMod(
                        n!("Omni"),
                        ModType::INC,
                        modDB.Sum(ModType::INC, None, &[stat]),
                        "Omniscience",
                        0,
                        0,
                        &[],
                    );
                    // L454
                    modDB.NewMod(
                        n!("Omni"),
                        ModType::MORE,
                        modDB.Sum(ModType::MORE, None, &[stat]),
                        "Omniscience",
                        0,
                        0,
                        &[],
                    );
                }
            }

            // L458
            if pass != 2 {
                // -- Subtract out double and triple dips
                // L460-461 (`conversion`, `reduction`)
                let mut reduction = [0.0_f64; 3];
                // L462
                for (ti, ty) in [ModType::BASE, ModType::INC, ModType::MORE]
                    .into_iter()
                    .enumerate()
                {
                    // L463-466 (`conversion[type][stat] = modDB:Sum(type, nil, stat) or 0`)
                    let c = |s: Name| modDB.Sum(ty, None, &[s]);
                    let StrDex = c(n!("StrDex"));
                    let StrInt = c(n!("StrInt"));
                    let DexInt = c(n!("DexInt"));
                    let All = c(n!("All"));
                    // L467
                    reduction[ti] = StrDex + StrInt + DexInt + 2.0 * All;
                }
                let src = "Reduction from Double/Triple Dipped attributes to Omniscience";
                // L469
                modDB.NewMod(n!("Omni"), ModType::BASE, -reduction[0], src, 0, 0, &[]);
                // L470
                modDB.NewMod(n!("Omni"), ModType::INC, -reduction[1], src, 0, 0, &[]);
                // L471
                modDB.NewMod(n!("Omni"), ModType::MORE, -reduction[2], src, 0, 0, &[]);
            }

            // L474
            for stat in [n!("Str"), n!("Dex"), n!("Int")] {
                // L475
                let base = base_of(stat);
                // L476
                output.set(stat, base);
            }

            // L479
            output.set(
                n!("Omni"),
                m_max(round(calcLib::val(&modDB, n!("Omni"), None), None), 0.0),
            );
            // L480
            if let Some(breakdown) = &breakdown {
                // L481
                let b = breakdown.simple(None, None, output.num(n!("Omni")), nm!["Omni"]);
                breakdown.set(n!("Omni"), b);
            }

            setAttributeConditions(&output, &condList);
        }
    };

    // L504
    if modDB.Flag(None, nm!["Omniscience"]) {
        // L505
        calculateOmniscience();
    } else {
        // L507
        calculateAttributes();
    }

    // -- Calculate total attributes
    // L511
    let total = output.num(n!("Str")) + output.num(n!("Dex")) + output.num(n!("Int"));
    output.set(n!("TotalAttr"), total);

    // -- Special case for Devotion
    // L514
    output.set(
        n!("Devotion"),
        modDB.Sum(ModType::BASE, None, nm!["Devotion"]),
    );

    // -- Add attribute bonuses
    // L517
    if !modDB.Flag(None, nm!["NoAttributeBonuses"]) {
        // L518
        if !modDB.Flag(None, nm!["NoStrengthAttributeBonuses"]) {
            // L519
            if !modDB.Flag(None, nm!["NoStrBonusToLife"]) {
                // L520
                modDB.NewMod(
                    n!("Life"),
                    ModType::BASE,
                    m_floor(output.num(n!("Str")) / 2.0),
                    "Strength",
                    0,
                    0,
                    &[],
                );
            }
            // L522
            let strDmgBonusRatioOverride =
                modDB.Sum(ModType::BASE, None, nm!["StrDmgBonusRatioOverride"]);
            // L523
            if strDmgBonusRatioOverride > 0.0 {
                // L524
                actor.strDmgBonus.set(m_floor(
                    (output.num(n!("Str"))
                        + modDB.Sum(ModType::BASE, None, nm!["DexIntToMeleeBonus"]))
                        * strDmgBonusRatioOverride,
                ));
            } else {
                // L526
                actor.strDmgBonus.set(m_floor(
                    (output.num(n!("Str"))
                        + modDB.Sum(ModType::BASE, None, nm!["DexIntToMeleeBonus"]))
                        / 5.0,
                ));
            }
            probe_written(actor, "strDmgBonus");
            // L528
            modDB.NewMod(
                n!("PhysicalDamage"),
                ModType::INC,
                actor.strDmgBonus.get(),
                "Strength",
                ModFlag::Melee,
                0,
                &[],
            );
        }
        // L530
        if !modDB.Flag(None, nm!["NoDexterityAttributeBonuses"]) {
            // L531
            let perDex = lor(
                Val::from_value(&modDB.Override(None, nm!["DexAccBonusOverride"])),
                || Val::Num(data().misc.AccuracyPerDexBase),
            );
            modDB.NewMod(
                n!("Accuracy"),
                ModType::BASE,
                output.num(n!("Dex")) * perDex.num(),
                "Dexterity",
                0,
                0,
                &[],
            );
            // L532
            if !modDB.Flag(None, nm!["NoDexBonusToEvasion"]) {
                // L533
                modDB.NewMod(
                    n!("Evasion"),
                    ModType::INC,
                    m_floor(output.num(n!("Dex")) / 5.0),
                    "Dexterity",
                    0,
                    0,
                    &[],
                );
            }
        }
        // L536
        if !modDB.Flag(None, nm!["NoIntelligenceAttributeBonuses"]) {
            // L537
            if !modDB.Flag(None, nm!["NoIntBonusToMana"]) {
                // L538
                modDB.NewMod(
                    n!("Mana"),
                    ModType::BASE,
                    m_floor(output.num(n!("Int")) / 2.0),
                    "Intelligence",
                    0,
                    0,
                    &[],
                );
            }
            // L540
            if !modDB.Flag(None, nm!["NoIntBonusToES"]) {
                // L541
                modDB.NewMod(
                    n!("EnergyShield"),
                    ModType::INC,
                    m_floor(output.num(n!("Int")) / 10.0),
                    "Intelligence",
                    0,
                    0,
                    &[],
                );
            }
        }
    }

    // L546
    doActorLifeMana(actor);
}

/// CalcPerform.lua 160-172 / 198-210 (the same block for `weaponData1` and `weaponData2`).
fn countsAsAll1H(wd: &Table, condList: &Table) {
    // L161
    wd.set(n!("AddedUsingAxe"), !condList.truthy(n!("UsingAxe")));
    // L162
    condList.set(n!("UsingAxe"), true);
    // L163 --Varunastra is a sword
    let name = str_of(&wd.get(n!("name")), "name");
    let added = match pob_lua::lmatch(&name, "Varunastra", None) {
        Some(c) => Val::from(
            c.first()
                .map(|c| c.to_str().into_owned())
                .unwrap_or_default(),
        ),
        None => Val::Bool(!condList.truthy(n!("UsingSword"))),
    };
    wd.set(n!("AddedUsingSword"), added);
    // L164
    condList.set(n!("UsingSword"), true);
    // L165
    wd.set(n!("AddedUsingDagger"), !condList.truthy(n!("UsingDagger")));
    // L166
    condList.set(n!("UsingDagger"), true);
    // L167
    wd.set(n!("AddedUsingMace"), !condList.truthy(n!("UsingMace")));
    // L168
    condList.set(n!("UsingMace"), true);
    // L169
    wd.set(n!("AddedUsingClaw"), !condList.truthy(n!("UsingClaw")));
    // L170
    condList.set(n!("UsingClaw"), true);
    // -- GGG stated that a single Varunastra satisfied requirement for wielding two different weapons
    // L172
    condList.set(n!("WieldingDifferentWeaponTypes"), true);
}

/// CalcPerform.lua 420-436 / 485-501 (the same statements end both attribute closures).
fn setAttributeConditions(output: &Output, condList: &Table) {
    let (Str, Dex, Int) = (
        output.num(n!("Str")),
        output.num(n!("Dex")),
        output.num(n!("Int")),
    );
    // L420
    let mut stats = [Str, Dex, Int];
    // L421
    pob_lua::sort(&mut stats, |a, b| a < b);
    // L422
    output.set(n!("LowestAttribute"), stats[0]);
    // L423
    condList.set(n!("TwoHighestAttributesEqual"), stats[1] == stats[2]);

    // L425
    condList.set(n!("DexHigherThanInt"), Dex > Int);
    // L426
    condList.set(n!("StrHigherThanInt"), Str > Int);
    // L427
    condList.set(n!("IntHigherThanDex"), Int > Dex);
    // L428
    condList.set(n!("StrHigherThanDex"), Str > Dex);
    // L429
    condList.set(n!("IntHigherThanStr"), Int > Str);
    // L430
    condList.set(n!("DexHigherThanStr"), Dex > Str);

    // L432
    condList.set(n!("StrHighestAttribute"), Str >= Dex && Str >= Int);
    // L433
    condList.set(n!("IntHighestAttribute"), Int >= Str && Int >= Dex);
    // L434
    condList.set(n!("DexHighestAttribute"), Dex >= Str && Dex >= Int);
    // L435
    condList.set(n!("IntSingleHighestAttribute"), Int > Str && Int > Dex);
    // L436
    condList.set(n!("DexSingleHighestAttribute"), Dex > Str && Dex > Int);
}

/// @pob Modules/CalcPerform.lua:551-584 h=88ae145e
/// Global `doActorLifeManaReservation(actor, addAura)`.
/// Calculate life/mana reservation
pub fn doActorLifeManaReservation(actor: &ActorRef, addAura: bool) {
    // L552
    let modDB = actor.modDB.get();
    // L554
    let output = actor.output.get();
    // L555
    let condList = modDB.conditions.clone();

    // L557
    for pool in [n!("Life"), n!("Mana")] {
        // L558
        let max = output.get(pool);
        // L559
        let reserved: f64;
        // L560
        if lua_lt(0.0, &max) {
            let max = max.num();
            // L561
            let lowPerc = modDB.Sum(ModType::BASE, None, &[cat!("Low", pool, "Percentage")]);
            // L562
            reserved = actor.extra.get(cat!("reserved_", pool, "Base")).num_or(0.0)
                + m_ceil(
                    max * actor
                        .extra
                        .get(cat!("reserved_", pool, "Percent"))
                        .num_or(0.0)
                        / 100.0,
                );
            // L563 (an implicit global in Lua, kept as a local: no read before this write, §7.3)
            let uncancellableReservation = actor
                .extra
                .get(cat!("uncancellable_", pool, "Reservation"))
                .num_or(0.0);
            // L564
            output.set(cat!(pool, "Reserved"), m_min(reserved, max));
            // L565
            output.set(
                cat!(pool, "ReservedPercent"),
                m_min(reserved / max * 100.0, 100.0),
            );
            // L566
            output.set(cat!(pool, "Unreserved"), max - reserved);
            // L567
            output.set(
                cat!(pool, "UnreservedPercent"),
                (max - reserved) / max * 100.0,
            );
            // L568
            output.set(
                cat!(pool, "UncancellableReservation"),
                m_min(uncancellableReservation, 0.0),
            );
            // L569
            output.set(
                cat!(pool, "CancellableReservation"),
                100.0 - uncancellableReservation,
            );
            // L570
            if (max - reserved) / max
                <= (if lowPerc > 0.0 {
                    lowPerc
                } else {
                    data().misc.LowPoolThreshold
                })
            {
                // L571
                condList.set(cat!("Low", pool), true);
            }
        } else {
            // L574
            reserved = 0.0;
        }
        // L576
        if addAura {
            // L577
            for value in modDB.List(None, &[cat!("GrantReserved", pool, "AsAura")]) {
                // L578
                let mut auraMod: Mod = match value.inner_mod() {
                    Some(m) => (**m).clone(),
                    None => lua_error!("bad argument #1 to 'copyTable' (table expected, got nil)"),
                };
                // L579
                let v = match auraMod.value {
                    Value::Num(v) => v,
                    ref other => lua_error!(
                        "attempt to perform arithmetic on field 'value' (a {} value)",
                        value_type(other)
                    ),
                };
                auraMod.value = Value::Num(m_floor(v * m_min(reserved, max.num())));
                // L580 (no source: `NewMod(name, type, value)`)
                let payload = Value::Table(Arc::new(ValueTable {
                    array: Vec::new(),
                    fields: vec![(n!("mod"), Value::Mod(Arc::new(auraMod)))],
                }));
                modDB.AddMod(Arc::new(modLib::createMod(
                    n!("ExtraAura"),
                    ModType::LIST,
                    payload,
                    None,
                    0,
                    0,
                    &[],
                )));
            }
        }
    }
}

/// Lua `a < b` with a number `a` and the value `b` (non-numbers raise).
fn lua_lt(a: f64, b: &Val) -> bool {
    match b {
        Val::Num(b) => a < *b,
        other => lua_error!("attempt to compare number with {}", other.lua_type()),
    }
}

/// @pob Modules/CalcPerform.lua:587-617 h=cc53e1d0
/// Helper function to determine curse priority when processing curses beyond the curse limit
pub(crate) fn determineCursePriority(curseName: Name, activeSkill: Option<&ActiveSkillRef>) -> f64 {
    let cursePriority = &data().cursePriority;
    let prio = |k: &str| cursePriority.get(&Name::from(k)).copied();
    // L588 `curseName or ""`: a `Name` is never nil (callers pass "" for nil)
    // L589
    let mut source: LStr = LStr::from("");
    // L590
    let mut slot: LStr = LStr::from("");
    // L591
    let mut socket = 1.0;
    // L592
    let socketGroup = activeSkill.and_then(|s| s.socketGroup.get());
    if let Some(socketGroup) = &socketGroup {
        // L593
        source = socketGroup.source.get().unwrap_or_else(|| LStr::from(""));
        // L594
        slot = socketGroup
            .slot
            .get()
            .map_or_else(|| LStr::from(""), |s| LStr::from(s.as_str()));
        // L595
        for_ipairs!((k, v) in socketGroup.gemList.get() => {
            // L596
            if v.gemData.get().is_some_and(|g| g.name == curseName) {
                // -- We need to enforce a limit of 8 here to avoid collision with data.cursePriority["CurseFromEquipment"]
                // L598
                socket = m_min(k as f64, 8.0);
                // L599
                break;
            }
        });
    }
    // L603
    let basePriority = cursePriority.get(&curseName).copied().unwrap_or(0.0);
    // L604
    let socketPriority = socket
        * prio("SocketPriorityBase")
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
    // L605 (`slot:gsub(…)` as an index: its first result)
    let mut slotPriority = prio(&pob_lua::gsub(&slot, " (Swap)", "", None).0).unwrap_or(0.0);
    // L606
    let mut sourcePriority: Option<f64> = Some(0.0);
    // L607
    if activeSkill.is_some_and(|s| s.skillTypes.get().with(|t| t.has(SkillType::Aura))) {
        // L608
        sourcePriority = prio("CurseFromAura");
    } else if !source.is_empty() {
        // L610
        sourcePriority = prio("CurseFromEquipment");
    }
    // L612
    if !source.is_empty()
        && (Some(slotPriority) == prio("Ring 2") || Some(slotPriority) == prio("Ring 3"))
    {
        // -- Implicit and explicit curses from rings have equal priority; only curses from socketed skill gems care about which ring slot they're equipped in
        // L614
        slotPriority = prio("Ring 1")
            .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
    }
    // L616
    let sourcePriority = sourcePriority
        .unwrap_or_else(|| lua_error!("attempt to perform arithmetic on a nil value"));
    basePriority + socketPriority + slotPriority + sourcePriority
}

/// @pob Modules/CalcPerform.lua:619-633 h=7a9a1545
pub(crate) fn applyEnemyModifiers(actor: &ActorRef, clearCache: bool) {
    // L620
    if clearCache || actor.appliedEnemyModifiers.get().is_none() {
        // L621
        let t = Table::new();
        enemy_cache::reset(actor, &t);
        actor.appliedEnemyModifiers.set(Some(t));
    }
    // L623
    let cache = actor.appliedEnemyModifiers.get().expect("set above");
    // L624
    let enemyDB = actor
        .enemy()
        .unwrap_or_else(|| lua_error!("attempt to index field 'enemy' (a nil value)"))
        .modDB
        .get();
    // L625
    for value in actor.modDB.get().Tabulate(None, None, nm!["EnemyModifier"]) {
        // L626 `value.value and value.value.mod`
        let r#mod = value.value.inner_mod().cloned();
        // L627
        if let Some(r#mod) = r#mod {
            if !enemy_cache::has(actor, &cache, &r#mod) {
                // L628
                let source: Option<AStr> =
                    r#mod.source.clone().or_else(|| value.r#mod.source.clone());
                // L629 `modLib.setSource(mod, source)` mutates `mod` in place in Lua. It is the
                // inner mod of the actor's EnemyModifier mod (shared): CCR-0085 §3. When the
                // assignment changes nothing the very mod is added (identity as in Lua);
                // otherwise a re-sourced copy.
                let added = setSource_shared(&r#mod, source.as_deref());
                enemyDB.AddMod(added.clone());
                // L630
                enemy_cache::set(actor, &cache, &r#mod, &added);
            }
        }
    }
}

/// `modLib.setSource(mod, source)` on a mod Lua shares (CCR-0085 §3): the mod itself when
/// setting the source changes nothing, else the re-sourced copy (`ModTools.lua:239-246`).
fn setSource_shared(m: &ModRef, source: Option<&str>) -> ModRef {
    let same = |x: &Mod| x.source.as_deref() == source;
    let inner_same = m.value.inner_mod().map_or(true, |i| same(i));
    if same(m) && inner_same {
        return m.clone();
    }
    match source {
        Some(s) => Arc::new(modLib::setSource(m, s)),
        // `mod.source = nil` (both sources nil): drop the inner mod's source too
        None => {
            let mut c = (**m).clone();
            c.source = None;
            if let (Some(inner), Value::Table(t)) = (m.value.inner_mod(), &m.value) {
                let mut i = (**inner).clone();
                i.source = None;
                c.value = Value::Table(Arc::new(t.with(n!("mod"), Value::Mod(Arc::new(i)))));
            }
            Arc::new(c)
        }
    }
}

/// `actor.appliedEnemyModifiers`: a Lua table keyed by mod identity (`cache[mod] = true`).
/// The frozen field is a `Table`, which has no identity keys (CCR-0030 #6), so the set of
/// mods lives beside it, per actor, tied to the table it belongs to. CCR-0085.
mod enemy_cache {
    use super::*;
    use std::cell::RefCell;

    struct Entry {
        actor: Weak<Actor>,
        /// The `appliedEnemyModifiers` table this set belongs to (kept alive so its
        /// address is not reused while the entry exists).
        table: Table,
        /// The keys, by `Arc` identity.
        mods: Vec<ModRef>,
    }

    thread_local! {
        static CACHES: RefCell<Vec<Entry>> = const { RefCell::new(Vec::new()) };
    }

    /// A fresh `{ }` for `actor` (L621).
    pub(super) fn reset(actor: &ActorRef, t: &Table) {
        CACHES.with(|c| {
            let mut c = c.borrow_mut();
            c.retain(|e| e.actor.strong_count() > 0 && !e.actor.ptr_eq(&Rc::downgrade(actor)));
            c.push(Entry {
                actor: Rc::downgrade(actor),
                table: t.clone(),
                mods: Vec::new(),
            });
        });
        // probe builds: an injected actor whose field was nil in Lua has it marked absent,
        // and the exporter would hide the new table (its mod keys live in the registry)
        probe_written(actor, "appliedEnemyModifiers");
    }

    /// `cache[mod]` (truthy).
    pub(super) fn has(actor: &ActorRef, t: &Table, m: &ModRef) -> bool {
        let own = CACHES.with(|c| {
            c.borrow().iter().find_map(|e| {
                (e.actor.ptr_eq(&Rc::downgrade(actor)) && e.table.ptr_eq(t))
                    .then(|| e.mods.iter().any(|x| Arc::ptr_eq(x, m)))
            })
        });
        match own {
            Some(hit) => hit,
            // a table this module did not create: an injected one (probe builds)
            None => injected_has(t, m),
        }
    }

    /// `cache[mod] = true`; `exported` is the mod as Lua's key table looks afterwards
    /// (re-sourced by `setSource`), for the state export.
    pub(super) fn set(actor: &ActorRef, t: &Table, m: &ModRef, exported: &ModRef) {
        let found = CACHES.with(|c| {
            let mut c = c.borrow_mut();
            if let Some(e) = c
                .iter_mut()
                .find(|e| e.actor.ptr_eq(&Rc::downgrade(actor)) && e.table.ptr_eq(t))
            {
                e.mods.push(m.clone());
                true
            } else {
                false
            }
        });
        if !found {
            // an injected table: start tracking it, keeping its injected keys
            CACHES.with(|c| {
                c.borrow_mut().push(Entry {
                    actor: Rc::downgrade(actor),
                    table: t.clone(),
                    mods: vec![m.clone()],
                });
            });
        }
        exported_set(t, exported);
    }

    #[cfg(feature = "probe")]
    fn injected_has(t: &Table, m: &ModRef) -> bool {
        // Injection decodes the dumped key mods afresh (their identity with the actor's
        // EnemyModifier mods is lost), so an injected key matches by value.
        crate::probe::with_registry(|r| {
            r.obj_keys(t).iter().any(|(k, v)| {
                v.truthy() && matches!(k, Val::Mod(x) if Arc::ptr_eq(x, m) || (**x == **m))
            })
        })
    }

    #[cfg(not(feature = "probe"))]
    fn injected_has(_t: &Table, _m: &ModRef) -> bool {
        false
    }

    #[cfg(feature = "probe")]
    fn exported_set(t: &Table, exported: &ModRef) {
        crate::probe::with_registry(|r| {
            r.push_obj_key(t, Val::Mod(exported.clone()), Val::Bool(true))
        });
    }

    #[cfg(not(feature = "probe"))]
    fn exported_set(_t: &Table, _exported: &ModRef) {}
}

// ---- function probes (PF-A acceptance) --------------------------------------------------------

/// Run the PF-A function named by a function probe (`<id>:in`) on its injected locals
/// (the Lua parameter names) and return the locals afterwards plus `ret`, the
/// `{ n = …, … }` return table the `<id>:out` dump holds. Feature `probe`.
#[cfg(feature = "probe")]
pub fn probe_call(id: &str, locals: &[(String, Val)]) -> Result<Vec<(String, Val)>, String> {
    let get = |k: &str| {
        locals
            .iter()
            .find(|(n, _)| n == k)
            .map_or(Val::Nil, |(_, v)| v.clone())
    };
    let actor = || {
        get("actor")
            .obj::<Actor>()
            .ok_or("local actor is not an actor")
    };
    let env = || get("env").obj::<Env>().ok_or("local env is not an env");
    let skill = |k: &str| get(k).obj::<ActiveSkill>();
    let id = id.split_once(':').map_or(id, |(a, _)| a);
    let ret: Vec<Val> = match id {
        "getCachedOutputValue" => {
            let s = skill("activeSkill").ok_or("local activeSkill is not an active skill")?;
            let names: Vec<Name> = get("varargs")
                .table()
                .map(|t| {
                    t.ipairs()
                        .into_iter()
                        .filter_map(|(_, v)| v.str().map(|s| Name::from(&s)))
                        .collect()
                })
                .unwrap_or_default();
            getCachedOutputValue(&env()?, &s, &names)
        }
        "mergeBuff" => {
            let destTable = get("destTable")
                .table()
                .ok_or("local destTable is not a table")?;
            let destKey = get("destKey")
                .str()
                .ok_or("local destKey is not a string")?;
            mergeBuff_v(&get("src"), &destTable, Name::from(&destKey));
            vec![]
        }
        "doActorLifeMana" => {
            doActorLifeMana(&actor()?);
            vec![]
        }
        "doActorAttribsConditions" => {
            doActorAttribsConditions(&env()?, &actor()?);
            vec![]
        }
        "doActorLifeManaReservation" => {
            doActorLifeManaReservation(&actor()?, get("addAura").truthy());
            vec![]
        }
        "determineCursePriority" => {
            let name = get("curseName")
                .str()
                .map_or_else(|| Name::from(""), |s| Name::from(&s));
            let s = skill("activeSkill");
            vec![Val::Num(determineCursePriority(name, s.as_ref()))]
        }
        "applyEnemyModifiers" => {
            applyEnemyModifiers(&actor()?, get("clearCache").truthy());
            vec![]
        }
        other => return Err(format!("no PF-A function {other}")),
    };
    let mut out: Vec<(String, Val)> = locals.to_vec();
    out.push(("ret".to_owned(), Val::Table(ret_table(&ret))));
    Ok(out)
}
