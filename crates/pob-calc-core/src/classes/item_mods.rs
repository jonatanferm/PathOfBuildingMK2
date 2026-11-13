//! @pob-file Classes/Item.lua @16de4b82d
//! Item mod lists (Classes/Item.lua 2192-2736): `calcLocal`, `Item:BuildModListForSlotNum`,
//! `getRangedModList`, `Item:BuildModList`, `Item:CanHaveMod`.
//!
//! Owner: IT-C. Field shapes follow CCR-0032 §2 / CCR-0025 §2 (untyped fields in `Item.extra`
//! under their Lua names; mod lines are `Table`s; a mod line's `modList` is a list `Table` of
//! `Val::Mod`). Workarounds for gaps in the frozen contract (CCR-0061):
//! * `t_remove(modList, i)` (calcLocal) and `mod[1] = …` / `t_insert(mod, …)` on the slot list's
//!   own copies (Item.lua 2336-2338, contracts.md#mod-mutation) need "remove/replace the mod at
//!   index i" on a ModList, which MS-A's API does not offer: the list is rebuilt
//!   ([`replace_list`]) with the same mods in the same order;
//! * `self.sockets.colourAlwaysMatches` (a string key on the sockets list) is
//!   `Item.extra.colourAlwaysMatches` (CCR-0019 §7); it is cleared where Lua replaces or wipes
//!   the sockets table.
#![allow(non_snake_case, non_upper_case_globals, clippy::all, clippy::pedantic)]

use pob_gamedata::mod_tools::{createMod, setSource};
use pob_gamedata::schema::affixes::ItemMod;
use pob_gamedata::tag;
use pob_lua::pattern;

use crate::classes::item::dmgTypeList;
use crate::classes::item_tools as itemLib;
use crate::prelude::*;

// ---- plumbing (Rust-only helpers; snake_case) ------------------------------------------------

/// Replace the contents of a (fresh, unaliased) ModList with `mods`, in order (CCR-0061:
/// `t_remove(modList, i)` / `modList[i] = mod`). `wipe` also drops `parent`/`actor`, which are
/// restored; the lists rebuilt here have empty `conditions`/`multipliers`.
fn replace_list(modList: &ModList, mods: Vec<ModRef>) {
    let parent = modList.parent.get();
    let actor = modList.actor.get();
    modList.wipe();
    modList.parent.set(parent);
    modList.actor.set(actor);
    modList.AddList(&mods);
}

/// A mod value in Lua arithmetic.
fn mod_num(v: &Value) -> f64 {
    match v {
        Value::Num(n) => *n,
        Value::Str(s) => pob_lua::tonumber(s).unwrap_or_else(|| {
            lua_error!("attempt to perform arithmetic on field 'value' (a string value)")
        }),
        other => lua_error!(
            "attempt to perform arithmetic on field 'value' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `calcLocal(...)` as an arithmetic operand.
fn cl(modList: &ModList, name: &str, r#type: ModType, flags: u32) -> f64 {
    calcLocal(modList, Name::from(name), r#type, flags).num()
}

/// A Lua value as a table key to store at (`t[v] = …`): a nil or NaN key raises.
fn key_of(v: &Val) -> Key {
    match v {
        Val::Str(s) => Key::S(Name::from(&**s)),
        Val::Num(x) => Key::from(*x),
        Val::Bool(b) => Key::B(*b),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!(
            "a {} value as a table key is not supported by Table",
            other.lua_type()
        ),
    }
}

/// `t[v]` read (FUP2, FOLLOWUPS "FUP1" item 6): a nil or NaN key reads nil (`lj_tab_get` finds
/// nothing), where a store at it raises ([`key_of`]); the same rule as item.rs `tget`.
fn read_at(t: &Table, v: &Val) -> Val {
    match v {
        Val::Nil => Val::Nil,
        Val::Num(x) => Key::try_num(*x).map_or(Val::Nil, |k| t.get(k)),
        other => t.get(key_of(other)),
    }
}

/// `value.<k>` of a LIST value (`nil` for a non-table raises, as indexing would).
fn vfield(v: &Value, k: &str) -> Val {
    match v {
        Value::Table(t) => Val::from_value(t.get(Name::from(k))),
        Value::Mod(m) => match k {
            "name" => Val::from(m.name),
            "value" => Val::from_value(&m.value),
            _ => Val::from_value(m.extra_get(Name::from(k)).unwrap_or(&Value::Nil)),
        },
        Value::JewelFunc(j) => match k {
            "type" => j.r#type.map_or(Val::Nil, Val::from),
            _ => Val::Nil,
        },
        other => lua_error!(
            "attempt to index local 'value' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `for _, value in ipairs(modList:List(nil, name)) do data[value.key] = value.value end`.
fn apply_data_list(t: &Table, modList: &ModList, name: &str) {
    for value in modList.List(None, &[Name::from(name)]) {
        let k = vfield(&value, "key");
        t.set(key_of(&k), vfield(&value, "value"));
    }
}

/// `list:NewMod(name, type, value, source, tags…)` with a possibly nil source.
fn new_mod(
    list: &ModList,
    name: Name,
    r#type: ModType,
    value: impl Into<Value>,
    source: Option<&str>,
    tags: &[Tag],
) {
    list.AddMod(Arc::new(createMod(name, r#type, value, source, 0, 0, tags)));
}

/// The value of a nil-able number field in arithmetic.
fn need(x: Option<f64>, what: &str) -> f64 {
    x.unwrap_or_else(|| lua_error!("attempt to perform arithmetic on {what} (a nil value)"))
}

/// `t.<k>` in arithmetic.
fn tnum(t: &Table, k: &str) -> f64 {
    match t.get(Name::from(k)) {
        Val::Num(n) => n,
        Val::Nil => lua_error!("attempt to perform arithmetic on field '{k}' (a nil value)"),
        other => other.num(),
    }
}

/// `t.<k> > 0` (Lua compares numbers only).
fn tgt0(t: &Table, k: &str) -> bool {
    match t.get(Name::from(k)) {
        Val::Num(n) => n > 0.0,
        other => lua_error!(
            "attempt to compare {} with number",
            if other.is_nil() {
                "nil"
            } else {
                other.lua_type()
            }
        ),
    }
}

/// `v:gsub(...)` for the three slot placeholders of a tag string (Item.lua:2240-2242).
fn slot_gsub(v: &str, slotName: &str, slotNum: Option<f64>) -> String {
    if !v.contains('{') {
        return v.to_owned();
    }
    let s = pattern::gsub(v, "{SlotName}", slotName, None).0;
    let s = pattern::gsub(
        &s,
        "{Hand}",
        if slotNum == Some(1.0) {
            "MainHand"
        } else {
            "OffHand"
        },
        None,
    )
    .0;
    pattern::gsub(
        &s,
        "{OtherSlotNum}",
        if slotNum == Some(1.0) { "2" } else { "1" },
        None,
    )
    .0
}

/// `for k, v in pairs(tag) do if type(v) == "string" then tag[k] = v:gsub(…) end end`:
/// every string-valued key of a tag (lists such as `varList` are tables and are skipped).
fn tag_gsub(tag: &mut Tag, slotName: &str, slotNum: Option<f64>) {
    let f = |v: &mut Option<Name>| {
        if let Some(s) = v {
            if s.as_str().contains('{') {
                *v = Some(Name::from(
                    slot_gsub(s.as_str(), slotName, slotNum).as_str(),
                ));
            }
        }
    };
    let g = |v: &mut Option<AStr>| {
        if let Some(s) = v {
            if s.contains('{') {
                *v = Some(AStr::from(slot_gsub(s, slotName, slotNum).as_str()));
            }
        }
    };
    f(&mut tag.var);
    f(&mut tag.actor);
    f(&mut tag.stat);
    f(&mut tag.divVar);
    f(&mut tag.limitVar);
    f(&mut tag.limitStat);
    f(&mut tag.limitActor);
    f(&mut tag.percentVar);
    f(&mut tag.thresholdVar);
    f(&mut tag.thresholdStat);
    f(&mut tag.thresholdActor);
    f(&mut tag.thresholdPercentVar);
    f(&mut tag.itemSlot);
    f(&mut tag.rarityCond);
    f(&mut tag.slotName);
    f(&mut tag.keyword);
    f(&mut tag.socketColor);
    f(&mut tag.skillName);
    f(&mut tag.skillId);
    f(&mut tag.baseFlag);
    f(&mut tag.monsterTag);
    f(&mut tag.globalLimitKey);
    f(&mut tag.effectType);
    f(&mut tag.effectName);
    f(&mut tag.effectCond);
    f(&mut tag.effectEnemyCond);
    f(&mut tag.effectStackVar);
    f(&mut tag.modCond);
    f(&mut tag.excludeItemType);
    g(&mut tag.searchCond);
    g(&mut tag.nameCond);
    if let TagType::Other(n) = tag.r#type {
        if n.as_str().contains('{') {
            tag.r#type = TagType::Other(Name::from(
                slot_gsub(n.as_str(), slotName, slotNum).as_str(),
            ));
        }
    }
    if let Some(ex) = tag.extra.as_mut() {
        for (_, v) in ex.fields.iter_mut() {
            if let Value::Str(s) = v {
                if s.contains('{') {
                    *v = Value::Str(AStr::from(slot_gsub(s, slotName, slotNum).as_str()));
                }
            }
        }
    }
}

/// The mods of a mod line's `modList` (`ipairs(modLine.modList)`; nil raises).
fn line_mods(modLine: &Table) -> (Table, Vec<(i64, ModRef)>) {
    let Val::Table(list) = modLine.get(n!("modList")) else {
        lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
    };
    let mods = list
        .ipairs()
        .into_iter()
        .map(|(i, v)| match v {
            Val::Mod(m) => (i as i64, m),
            other => lua_error!(
                "attempt to index local 'mod' (a {} value)",
                other.lua_type()
            ),
        })
        .collect();
    (list, mods)
}

/// A list `Table` of `Val::Mod` (a mod line's `modList`, CCR-0025 §2).
fn mods_table(mods: Vec<Mod>) -> Table {
    let t = Table::new();
    for m in mods {
        t.push(Val::Mod(Arc::new(m)));
    }
    t
}

// ---- Item.lua ------------------------------------------------------------------------------

// To be considered local, a modifier must be an exact flag match, and cannot have any tags (e.g. conditions, multipliers)
// Only the InSlot tag is allowed (for Adds x to x X Damage in X Hand modifiers)
// ---@param modList any
// ---@param name string
// ---@param type "FLAG"|"MORE"|"BASE"|"INC" other mod types not handled
// ---@param flags integer
// ---@return boolean|number
/// @pob Classes/Item.lua:2192-2219 h=1dfbd9ce
/// `calcLocal(modList, name, type, flags)` (local to Item.lua).
pub fn calcLocal(modList: &ModList, name: Name, r#type: ModType, flags: u32) -> Val {
    // L2193
    let mut result;
    // L2194
    if r#type == ModType::FLAG {
        // L2195
        result = Val::Bool(false);
    } else if r#type == ModType::MORE {
        // L2197
        result = Val::Num(1.0);
    } else {
        // L2199
        result = Val::Num(0.0);
    }
    // L2201
    let mods = modList.ipairs();
    let mut kept: Vec<ModRef> = Vec::with_capacity(mods.len());
    let mut removed = false;
    // L2202
    for r#mod in mods {
        // L2203: local mod = modList[i]
        // L2204
        if r#mod.name == name
            && r#mod.r#type == r#type
            && r#mod.flags == flags
            && r#mod.keywordFlags == 0
            && (r#mod.tags.is_empty() || r#mod.tags[0].r#type == TagType::InSlot)
        {
            // L2205
            if r#type == ModType::FLAG {
                // L2206
                result = lor(result, || Val::from_value(&r#mod.value));
            // convert MORE to times multiplier, e.g. 50% more = 1.5x, result = 1.5
            } else if r#type == ModType::MORE {
                // L2209
                result = Val::Num(result.num() * ((100.0 + mod_num(&r#mod.value)) / 100.0));
            } else {
                // L2211
                result = Val::Num(result.num() + mod_num(&r#mod.value));
            }
            // L2213: t_remove(modList, i)
            removed = true;
        } else {
            // L2215: i = i + 1
            kept.push(r#mod);
        }
    }
    if removed {
        replace_list(modList, kept); // CCR-0061
    }
    // L2218
    result
}

/// @pob Classes/Item.lua:2222-2520 h=169596ce
impl Item {
    // Build list of modifiers in a given slot number while applying local modifiers and adding quality
    pub fn BuildModListForSlotNum(&self, baseList: &ModList, slotNum: Option<f64>) -> ModList {
        let Some(base) = self.base.get() else {
            lua_error!("attempt to index field 'base' (a nil value)")
        };
        // L2223
        let mut slotName: String = self.GetPrimarySlot().as_str().to_owned();
        // L2224
        if slotNum != Some(1.0) {
            // L2225: tostring(slotNum) ("nil" when the item has one slot)
            let r = slotNum.map_or_else(|| "nil".to_owned(), tostring_num);
            slotName = pattern::gsub(&slotName, "1", &r, None).0;
        }
        // L2227
        let modList = ModStore::ModList(None);
        // L2228
        for baseMod in baseList.ipairs() {
            // L2229
            let mut r#mod: Mod = (*baseMod).clone();
            // L2230
            let mut add = true;
            // L2231
            for tag in r#mod.tags.iter_mut() {
                if tag.r#type == TagType::LuaNonTable {
                    lua_error!("attempt to index local 'tag' (a non-table value)");
                }
                // L2232
                if tag.r#type == TagType::SlotNumber || tag.r#type == TagType::InSlot {
                    // L2233
                    if tag.num != slotNum {
                        // L2234
                        add = false;
                        // L2235
                        break;
                    }
                }
                // L2238-2244
                tag_gsub(tag, &slotName, slotNum);
            }
            // L2246
            if add {
                // L2247
                let ex = r#mod.extra.get_or_insert_with(Default::default);
                ex.set(n!("sourceSlot"), Value::Str(AStr::from(slotName.as_str())));
                // L2248
                modList.AddMod(Arc::new(r#mod));
            }
        }
        // L2251
        let sockets = self.sockets.get();
        if sockets.len() > 0 {
            // L2252
            let multiName = |color: &Val| -> Option<Name> {
                let c = match color {
                    Val::Str(s) => s.clone(),
                    _ => return None,
                };
                let p = match &*c {
                    "R" => "Multiplier:RedSocketIn",
                    "G" => "Multiplier:GreenSocketIn",
                    "B" => "Multiplier:BlueSocketIn",
                    "W" => "Multiplier:WhiteSocketIn",
                    _ => return None,
                };
                Some(Name::from(format!("{p}{slotName}").as_str()))
            };
            // L2258
            let groupCounts = Table::new();
            // L2259
            for socket in sockets.snapshot() {
                // L2260
                let group = socket.get(n!("group"));
                // L2261
                let count = read_at(&groupCounts, &group).num_or(0.0) + 1.0;
                groupCounts.set(key_of(&group), Val::Num(count));
                // L2262
                if let Some(name) = multiName(&socket.get(n!("color"))) {
                    // L2263
                    modList.NewMod(name, ModType::BASE, 1.0, "Item Sockets", 0, 0, &[]);
                }
            }
            // L2266
            let mut unlinkedSockets = 0.0;
            // L2267 (the count does not depend on the order)
            for (_, count) in groupCounts.pairs() {
                // L2268
                if count.opt_num() == Some(1.0) {
                    // L2269
                    unlinkedSockets = unlinkedSockets + 1.0;
                }
            }
            // L2272
            modList.NewMod(
                Name::from(format!("Multiplier:UnlinkedSocketIn{slotName}").as_str()),
                ModType::BASE,
                unlinkedSockets,
                "Unlinked Item Sockets",
                0,
                0,
                &[],
            );
        }
        // L2274
        let craftedQuality = calcLocal(&modList, n!("Quality"), ModType::BASE, 0).num_or(0.0);
        // L2275
        let selfCraftedQuality = self.extra.get(n!("craftedQuality"));
        if selfCraftedQuality.opt_num() != Some(craftedQuality) {
            // L2276
            if selfCraftedQuality.truthy() {
                // L2277
                let q = self.quality.get().unwrap_or(0.0);
                self.quality
                    .set(Some(q - selfCraftedQuality.num() + craftedQuality));
            }
            // L2279
            self.extra.set(n!("craftedQuality"), craftedQuality);
        }
        // L2281
        let corruptImplicitCount =
            calcLocal(&modList, n!("CorruptImplicitCount"), ModType::BASE, 0);
        self.extra
            .set(n!("corruptImplicitCount"), corruptImplicitCount.clone());
        // L2282
        if corruptImplicitCount.opt_num() == Some(0.0) {
            // L2283
            self.extra.set(n!("corruptImplicitCount"), Val::Nil);
        }
        // L2285
        if let Some(quality) = self.quality.get() {
            // L2286
            modList.NewMod(
                Name::from(format!("Multiplier:QualityOn{slotName}").as_str()),
                ModType::BASE,
                quality,
                "Quality",
                0,
                0,
                &[],
            );
        }
        let quality = || need(self.quality.get(), "field 'quality'");
        // L2288
        if let Some(w) = &base.weapon {
            // L2289
            let weaponData = Table::new();
            // L2290
            let Some(selfWeaponData) = self.weaponData.get() else {
                lua_error!("attempt to index field 'weaponData' (a nil value)")
            };
            let Some(slotNumV) = slotNum else {
                lua_error!("table index is nil")
            };
            selfWeaponData.set(Key::from(slotNumV), weaponData.clone());
            // L2291
            weaponData.set(n!("type"), base.r#type);
            // L2292
            weaponData.set(n!("subType"), base.subType.map_or(Val::Nil, Val::from));
            // L2293
            weaponData.set(n!("name"), Val::Str(self.name.get()));
            // L2294
            let AttackSpeedInc = cl(&modList, "Speed", ModType::INC, ModFlag::Attack)
                + m_floor(
                    quality() / 8.0
                        * cl(
                            &modList,
                            "AlternateQualityLocalAttackSpeedPer8Quality",
                            ModType::INC,
                            0,
                        ),
                );
            weaponData.set(n!("AttackSpeedInc"), AttackSpeedInc);
            // L2295
            let AttackRate = round(w.AttackRateBase * (1.0 + AttackSpeedInc / 100.0), Some(2));
            weaponData.set(n!("AttackRate"), AttackRate);
            // L2296
            let rangeBonus = cl(&modList, "WeaponRange", ModType::BASE, 0)
                + 10.0 * cl(&modList, "WeaponRangeMetre", ModType::BASE, 0)
                + m_floor(
                    quality() / 10.0
                        * cl(
                            &modList,
                            "AlternateQualityLocalWeaponRangePer10Quality",
                            ModType::BASE,
                            0,
                        ),
                );
            weaponData.set(n!("rangeBonus"), rangeBonus);
            // L2297
            weaponData.set(n!("range"), w.Range + rangeBonus);
            // L2298
            let LocalIncEle = cl(&modList, "LocalElementalDamage", ModType::INC, 0);
            // L2299
            for dmgType in dmgTypeList {
                let (baseMin, baseMax) = if dmgType == "Physical" {
                    (Some(w.PhysicalMin), Some(w.PhysicalMax))
                } else {
                    (None, None) // self.base.weapon[dmgType.."Min"] is nil
                };
                // L2300
                let mut min = baseMin.unwrap_or(0.0)
                    + cl(&modList, &format!("{dmgType}Min"), ModType::BASE, 0);
                // L2301
                let mut max = baseMax.unwrap_or(0.0)
                    + cl(&modList, &format!("{dmgType}Max"), ModType::BASE, 0);
                // L2302
                if dmgType == "Physical" {
                    // L2303
                    let physInc = cl(&modList, "PhysicalDamage", ModType::INC, 0);
                    // L2304
                    let mut qualityScalar = self.quality.get();
                    // L2305
                    if cl(&modList, "AlternateQualityWeapon", ModType::BASE, 0) > 0.0 {
                        // L2306
                        qualityScalar = Some(0.0);
                    }
                    let qs = need(qualityScalar, "local 'qualityScalar'");
                    // L2308
                    min = round(min * (1.0 + physInc / 100.0) * (1.0 + qs / 100.0), None);
                    // L2309
                    max = round(max * (1.0 + physInc / 100.0) * (1.0 + qs / 100.0), None);
                } else if dmgType != "Physical" && dmgType != "Chaos" {
                    // L2311
                    let localInc = cl(&modList, &format!("Local{dmgType}Damage"), ModType::INC, 0)
                        + LocalIncEle;
                    // L2312
                    min = round(min * (1.0 + localInc / 100.0), None);
                    // L2313
                    max = round(max * (1.0 + localInc / 100.0), None);
                }
                // L2315
                if min > 0.0 && max > 0.0 {
                    // L2316
                    weaponData.set(Name::from(format!("{dmgType}Min").as_str()), min);
                    // L2317
                    weaponData.set(Name::from(format!("{dmgType}Max").as_str()), max);
                    // L2318
                    let dps = (min + max) / 2.0 * AttackRate;
                    // L2319
                    weaponData.set(Name::from(format!("{dmgType}DPS").as_str()), dps);
                    // L2320
                    if dmgType != "Physical" && dmgType != "Chaos" {
                        // L2321
                        let e = weaponData.num_or(n!("ElementalDPS"), 0.0) + dps;
                        weaponData.set(n!("ElementalDPS"), e);
                    }
                }
            }
            // L2325
            let critBase = w.CritChanceBase + cl(&modList, "CritChance", ModType::BASE, 0);
            let critInc = cl(&modList, "CritChance", ModType::INC, 0)
                + m_floor(
                    quality() / 4.0
                        * cl(
                            &modList,
                            "AlternateQualityLocalCritChancePer4Quality",
                            ModType::INC,
                            0,
                        ),
                );
            weaponData.set(
                n!("CritChance"),
                round(critBase * (1.0 + critInc / 100.0), Some(2)),
            );
            // L2326
            apply_data_list(&weaponData, &modList, "WeaponData");
            // L2329: the slot list's own copies (contracts.md#mod-mutation): replaced at their index
            let hand = if slotNum == Some(1.0) {
                "MainHandAttack"
            } else {
                "OffHandAttack"
            };
            let mut out: Vec<ModRef> = Vec::new();
            let mut changed = false;
            for r#mod in modList.ipairs() {
                // Convert accuracy, L/MGoH and PAD Leech modifiers to local
                // L2331-2335
                if ((r#mod.name == n!("Accuracy") && r#mod.flags == 0)
                    || (r#mod.name == n!("ImpaleChance") && r#mod.flags != ModFlag::Spell)
                    || ((r#mod.name == n!("LifeOnHit") || r#mod.name == n!("ManaOnHit"))
                        && r#mod.flags == ModFlag::Attack)
                    || ((r#mod.name == n!("PhysicalDamageLifeLeech")
                        || r#mod.name == n!("PhysicalDamageManaLeech"))
                        && r#mod.flags == ModFlag::Attack))
                    && (r#mod.keywordFlags == 0 || r#mod.keywordFlags == KeywordFlag::Attack)
                    && r#mod.tags.is_empty()
                {
                    // L2336: mod[1] = { … } (no mod[1]: the first tag)
                    let mut m = (*r#mod).clone();
                    m.tags.push(tag!({ type = Condition, var = hand }));
                    out.push(Arc::new(m));
                    changed = true;
                } else if (r#mod.name == n!("PoisonChance") || r#mod.name == n!("BleedChance"))
                    && r#mod.flags != ModFlag::Spell
                    && (r#mod.tags.is_empty()
                        || (r#mod.tags[0].r#type == TagType::Condition
                            && r#mod.tags[0].var == Some(n!("CriticalStrike"))
                            && r#mod.tags.len() < 2))
                {
                    // L2338
                    let mut m = (*r#mod).clone();
                    m.tags.push(tag!({ type = Condition, var = hand }));
                    out.push(Arc::new(m));
                    changed = true;
                } else {
                    out.push(r#mod);
                }
            }
            if changed {
                replace_list(&modList, out); // CCR-0061
            }
            // L2341
            weaponData.set(n!("TotalDPS"), 0.0);
            // L2342
            for dmgType in dmgTypeList {
                // L2343
                let t = weaponData.num(n!("TotalDPS"))
                    + weaponData.num_or(Name::from(format!("{dmgType}DPS").as_str()), 0.0);
                weaponData.set(n!("TotalDPS"), t);
            }
        } else if let Some(a) = &base.armour {
            // L2346
            let Some(armourData) = self.armourData.get() else {
                lua_error!("attempt to index local 'armourData' (a nil value)")
            };
            // L2347
            let armourBase =
                cl(&modList, "Armour", ModType::BASE, 0) + a.ArmourBaseMin.unwrap_or(0.0);
            // L2348
            let armourVariance = a.ArmourBaseMax.unwrap_or(0.0) - a.ArmourBaseMin.unwrap_or(0.0);
            // L2349
            let armourEvasionBase = cl(&modList, "ArmourAndEvasion", ModType::BASE, 0);
            // L2350
            let evasionBase =
                cl(&modList, "Evasion", ModType::BASE, 0) + a.EvasionBaseMin.unwrap_or(0.0);
            // L2351
            let evasionVariance = a.EvasionBaseMax.unwrap_or(0.0) - a.EvasionBaseMin.unwrap_or(0.0);
            // L2352
            let evasionEnergyShieldBase = cl(&modList, "EvasionAndEnergyShield", ModType::BASE, 0);
            // L2353
            let energyShieldBase = cl(&modList, "EnergyShield", ModType::BASE, 0)
                + a.EnergyShieldBaseMin.unwrap_or(0.0);
            // L2354
            let energyShieldVariance =
                a.EnergyShieldBaseMax.unwrap_or(0.0) - a.EnergyShieldBaseMin.unwrap_or(0.0);
            // L2355
            let armourEnergyShieldBase = cl(&modList, "ArmourAndEnergyShield", ModType::BASE, 0);
            // L2356
            let wardBase = cl(&modList, "Ward", ModType::BASE, 0) + a.WardBaseMin.unwrap_or(0.0);
            // L2357
            let wardVariance = a.WardBaseMax.unwrap_or(0.0) - a.WardBaseMin.unwrap_or(0.0);
            // L2358
            let armourInc = cl(&modList, "Armour", ModType::INC, 0);
            // L2359
            let armourEvasionInc = cl(&modList, "ArmourAndEvasion", ModType::INC, 0);
            // L2360
            let evasionInc = cl(&modList, "Evasion", ModType::INC, 0);
            // L2361
            let evasionEnergyShieldInc = cl(&modList, "EvasionAndEnergyShield", ModType::INC, 0);
            // L2362
            let energyShieldInc = cl(&modList, "EnergyShield", ModType::INC, 0);
            // L2363
            let wardInc = cl(&modList, "Ward", ModType::INC, 0);
            // L2364
            let armourEnergyShieldInc = cl(&modList, "ArmourAndEnergyShield", ModType::INC, 0);
            // L2365
            let defencesInc = cl(&modList, "Defences", ModType::INC, 0);
            // L2366
            let mut qualityScalar = self.quality.get();
            // L2367
            if cl(&modList, "AlternateQualityArmour", ModType::BASE, 0) > 0.0 {
                // L2368
                qualityScalar = Some(0.0);
            }
            let qs = || need(qualityScalar, "local 'qualityScalar'");
            let pct = |k: &str| armourData.get(Name::from(k)).num_or(1.0);
            // base percentiles need to differ for each armour type, as they're weighted differently
            // L2371
            if armourData.truthy(n!("Armour"))
                && tgt0(&armourData, "Armour")
                && !armourData.truthy(n!("ArmourBasePercentile"))
            {
                // L2372
                let p = (tnum(&armourData, "Armour")
                    / ((1.0
                        + (armourInc + armourEvasionInc + armourEnergyShieldInc + defencesInc)
                            / 100.0)
                        * (1.0 + (qs() / 100.0)))
                    - armourBase)
                    / armourVariance;
                // L2373
                armourData.set(
                    n!("ArmourBasePercentile"),
                    round(m_max(m_min(p, 1.0), 0.0), Some(4)),
                );
            }
            // L2375
            if armourData.truthy(n!("Evasion"))
                && tgt0(&armourData, "Evasion")
                && !armourData.truthy(n!("EvasionBasePercentile"))
            {
                // L2376
                let p = (tnum(&armourData, "Evasion")
                    / ((1.0
                        + (evasionInc + armourEvasionInc + evasionEnergyShieldInc + defencesInc)
                            / 100.0)
                        * (1.0 + (qs() / 100.0)))
                    - evasionBase)
                    / evasionVariance;
                // L2377
                armourData.set(
                    n!("EvasionBasePercentile"),
                    round(m_max(m_min(p, 1.0), 0.0), Some(4)),
                );
            }
            // L2379
            if armourData.truthy(n!("EnergyShield"))
                && tgt0(&armourData, "EnergyShield")
                && !armourData.truthy(n!("EnergyShieldBasePercentile"))
            {
                // L2380
                let p = (tnum(&armourData, "EnergyShield")
                    / ((1.0
                        + (energyShieldInc
                            + armourEnergyShieldInc
                            + evasionEnergyShieldInc
                            + defencesInc)
                            / 100.0)
                        * (1.0 + (qs() / 100.0)))
                    - energyShieldBase)
                    / energyShieldVariance;
                // L2381
                armourData.set(
                    n!("EnergyShieldBasePercentile"),
                    round(m_max(m_min(p, 1.0), 0.0), Some(4)),
                );
            }
            // L2383
            if armourData.truthy(n!("Ward"))
                && tgt0(&armourData, "Ward")
                && !armourData.truthy(n!("WardBasePercentile"))
            {
                // L2384
                let p = (tnum(&armourData, "Ward")
                    / ((1.0 + (wardInc + defencesInc) / 100.0) * (1.0 + (qs() / 100.0)))
                    - wardBase)
                    / wardVariance;
                // L2385
                armourData.set(
                    n!("WardBasePercentile"),
                    round(m_max(m_min(p, 1.0), 0.0), Some(4)),
                );
            }

            // L2388
            armourData.set(
                n!("ArmourBase"),
                round(
                    a.ArmourBaseMin.unwrap_or(0.0) + armourVariance * pct("ArmourBasePercentile"),
                    None,
                ),
            );
            // L2389
            armourData.set(
                n!("Armour"),
                round(
                    (armourBase
                        + armourEvasionBase
                        + armourEnergyShieldBase
                        + armourVariance * pct("ArmourBasePercentile"))
                        * (1.0
                            + (armourInc + armourEvasionInc + armourEnergyShieldInc + defencesInc)
                                / 100.0)
                        * (1.0 + (qs() / 100.0)),
                    None,
                ),
            );
            // L2390
            armourData.set(
                n!("EvasionBase"),
                round(
                    a.EvasionBaseMin.unwrap_or(0.0)
                        + evasionVariance * pct("EvasionBasePercentile"),
                    None,
                ),
            );
            // L2391
            armourData.set(
                n!("Evasion"),
                round(
                    (evasionBase
                        + armourEvasionBase
                        + evasionEnergyShieldBase
                        + evasionVariance * pct("EvasionBasePercentile"))
                        * (1.0
                            + (evasionInc
                                + armourEvasionInc
                                + evasionEnergyShieldInc
                                + defencesInc)
                                / 100.0)
                        * (1.0 + (qs() / 100.0)),
                    None,
                ),
            );
            // L2392
            armourData.set(
                n!("EnergyShieldBase"),
                round(
                    a.EnergyShieldBaseMin.unwrap_or(0.0)
                        + energyShieldVariance * pct("EnergyShieldBasePercentile"),
                    None,
                ),
            );
            // L2393
            armourData.set(
                n!("EnergyShield"),
                round(
                    (energyShieldBase
                        + evasionEnergyShieldBase
                        + armourEnergyShieldBase
                        + energyShieldVariance * pct("EnergyShieldBasePercentile"))
                        * (1.0
                            + (energyShieldInc
                                + armourEnergyShieldInc
                                + evasionEnergyShieldInc
                                + defencesInc)
                                / 100.0)
                        * (1.0 + (qs() / 100.0)),
                    None,
                ),
            );
            // L2394
            armourData.set(
                n!("WardBase"),
                round(
                    a.WardBaseMin.unwrap_or(0.0) + wardVariance * pct("WardBasePercentile"),
                    None,
                ),
            );
            // L2395
            armourData.set(
                n!("Ward"),
                round(
                    (wardBase + wardVariance * pct("WardBasePercentile"))
                        * (1.0 + (wardInc + defencesInc) / 100.0)
                        * (1.0 + (qs() / 100.0)),
                    None,
                ),
            );

            // L2397
            if !armourData.truthy(n!("ArmourBasePercentile")) && tgt0(&armourData, "Armour") {
                // L2398
                armourData.set(n!("ArmourBasePercentile"), 1.0);
            }
            // L2400
            if !armourData.truthy(n!("EvasionBasePercentile")) && tgt0(&armourData, "Evasion") {
                // L2401
                armourData.set(n!("EvasionBasePercentile"), 1.0);
            }
            // L2403
            if !armourData.truthy(n!("EnergyShieldBasePercentile"))
                && tgt0(&armourData, "EnergyShield")
            {
                // L2404
                armourData.set(n!("EnergyShieldBasePercentile"), 1.0);
            }
            // L2406
            if !armourData.truthy(n!("WardBasePercentile")) && tgt0(&armourData, "Ward") {
                // L2407
                armourData.set(n!("WardBasePercentile"), 1.0);
            }

            // L2410
            if let Some(bc) = a.BlockChance {
                // L2411
                let v = m_floor(
                    (bc + cl(&modList, "BlockChance", ModType::BASE, 0))
                        * (1.0 + cl(&modList, "BlockChance", ModType::INC, 0) / 100.0),
                );
                armourData.set(n!("BlockChance"), v);
            }
            // L2413
            if let Some(mp) = a.MovementPenalty {
                // L2414
                let src = self.modSource.get();
                new_mod(
                    &modList,
                    n!("MovementSpeed"),
                    ModType::INC,
                    -mp,
                    src.as_deref(),
                    &[tag!({ type = Condition, var = "IgnoreMovementPenalties", neg = true })],
                );
            }
            // L2416
            apply_data_list(&armourData, &modList, "ArmourData");
        } else if let Some(f) = &base.flask {
            // L2420
            let Some(flaskData) = self.flaskData.get() else {
                lua_error!("attempt to index local 'flaskData' (a nil value)")
            };
            // L2421
            let durationInc = cl(&modList, "Duration", ModType::INC, 0);
            // L2422
            let durationMore = cl(&modList, "Duration", ModType::MORE, 0);
            // L2423
            if f.life.is_some() || f.mana.is_some() {
                // Recovery flask
                // L2425
                let instantPerc = cl(&modList, "FlaskInstantRecovery", ModType::BASE, 0);
                flaskData.set(n!("instantPerc"), instantPerc);
                // L2426
                flaskData.set(
                    n!("instantLowLifePerc"),
                    cl(&modList, "FlaskLowLifeInstantRecovery", ModType::BASE, 0),
                );
                // L2427
                let recoveryMod = 1.0 + cl(&modList, "FlaskRecovery", ModType::INC, 0) / 100.0;
                // L2428
                let rateMod = 1.0 + cl(&modList, "FlaskRecoveryRate", ModType::INC, 0) / 100.0;
                // L2429
                flaskData.set(
                    n!("duration"),
                    round(
                        f.duration * (1.0 + durationInc / 100.0) / rateMod * durationMore,
                        Some(1),
                    ),
                );
                // L2430
                if let Some(life) = f.life {
                    // L2431
                    let lifeBase = life * (1.0 + quality() / 100.0) * recoveryMod;
                    flaskData.set(n!("lifeBase"), lifeBase);
                    // L2432
                    let lifeInstant = lifeBase * instantPerc / 100.0;
                    flaskData.set(n!("lifeInstant"), lifeInstant);
                    // L2433
                    let lifeGradual = lifeBase * (1.0 - instantPerc / 100.0);
                    flaskData.set(n!("lifeGradual"), lifeGradual);
                    // L2434
                    flaskData.set(n!("lifeTotal"), lifeInstant + lifeGradual);
                    // L2435
                    flaskData.set(
                        n!("lifeAdditional"),
                        cl(&modList, "FlaskAdditionalLifeRecovery", ModType::BASE, 0),
                    );
                    // L2436
                    flaskData.set(
                        n!("lifeEffectNotRemoved"),
                        calcLocal(baseList, n!("LifeFlaskEffectNotRemoved"), ModType::FLAG, 0),
                    );
                }
                // L2438
                if let Some(mana) = f.mana {
                    // L2439
                    let manaBase = mana * (1.0 + quality() / 100.0) * recoveryMod;
                    flaskData.set(n!("manaBase"), manaBase);
                    // L2440
                    let manaInstant = manaBase * instantPerc / 100.0;
                    flaskData.set(n!("manaInstant"), manaInstant);
                    // L2441
                    let manaGradual = manaBase * (1.0 - instantPerc / 100.0);
                    flaskData.set(n!("manaGradual"), manaGradual);
                    // L2442
                    flaskData.set(n!("manaTotal"), manaInstant + manaGradual);
                    // L2443
                    flaskData.set(
                        n!("manaEffectNotRemoved"),
                        calcLocal(baseList, n!("ManaFlaskEffectNotRemoved"), ModType::FLAG, 0),
                    );
                }
            } else {
                // Utility flask
                // L2447
                flaskData.set(
                    n!("duration"),
                    round(
                        f.duration
                            * (1.0 + durationInc / 100.0)
                            * (1.0 + quality() / 100.0)
                            * durationMore,
                        Some(1),
                    ),
                );
            }
            // L2449
            flaskData.set(
                n!("chargesMax"),
                f.chargesMax + cl(&modList, "FlaskCharges", ModType::BASE, 0),
            );
            // L2450
            flaskData.set(
                n!("chargesUsed"),
                m_floor(
                    f.chargesUsed
                        * (1.0 + cl(&modList, "FlaskChargesUsed", ModType::INC, 0) / 100.0),
                ),
            );
            // L2451
            flaskData.set(
                n!("gainMod"),
                1.0 + cl(&modList, "FlaskChargeRecovery", ModType::INC, 0) / 100.0,
            );
            // L2452
            flaskData.set(
                n!("effectInc"),
                cl(&modList, "FlaskEffect", ModType::INC, 0)
                    + cl(&modList, "LocalEffect", ModType::INC, 0),
            );
            // L2453
            apply_data_list(&flaskData, &modList, "FlaskData");
        } else if let Some(t) = &base.tincture {
            // L2457
            let Some(tinctureData) = self.tinctureData.get() else {
                lua_error!("attempt to index local 'tinctureData' (a nil value)")
            };
            // L2458
            tinctureData.set(
                n!("manaBurn"),
                (t.manaBurn + 0.01)
                    / (1.0 + cl(&modList, "TinctureManaBurnRate", ModType::INC, 0) / 100.0)
                    / (1.0 + cl(&modList, "TinctureManaBurnRate", ModType::MORE, 0) / 100.0),
            );
            // L2459
            let cooldownInc = cl(&modList, "TinctureCooldownRecovery", ModType::INC, 0)
                + cl(&modList, "CooldownRecovery", ModType::INC, 0);
            tinctureData.set(n!("cooldownInc"), cooldownInc);
            // L2460
            tinctureData.set(n!("cooldown"), t.cooldown / (1.0 + cooldownInc / 100.0));
            // L2461
            tinctureData.set(
                n!("effectInc"),
                cl(&modList, "TinctureEffect", ModType::INC, 0)
                    + cl(&modList, "LocalEffect", ModType::INC, 0),
            );
            // L2462
            apply_data_list(&tinctureData, &modList, "TinctureData");
        } else if self.r#type.get() == n!("Jewel") {
            let name = self.name.get();
            // L2466
            if pattern::find(&name, "Grand Spectrum", None, false).is_some() {
                // L2467
                let spectrumMod: ModRef = Arc::new(createMod(
                    n!("Multiplier:GrandSpectrum"),
                    ModType::BASE,
                    1.0,
                    Some(&name),
                    0,
                    0,
                    &[],
                ));
                // L2468
                modList.AddMod(spectrumMod.clone());
                // L2469
                let mut v = ValueTable::new();
                v.set(n!("mod"), Value::Mod(spectrumMod));
                modList.NewMod(
                    n!("MinionModifier"),
                    ModType::LIST,
                    Value::Table(Arc::new(v)),
                    &name,
                    0,
                    0,
                    &[],
                );
            }

            // L2472
            let Some(jewelData) = self.jewelData.get() else {
                lua_error!("attempt to index local 'jewelData' (a nil value)")
            };
            // L2473
            for func in modList.List(None, &[n!("JewelFunc")]) {
                // L2474
                let funcList = match jewelData.get(n!("funcList")) {
                    Val::Table(t) => t,
                    Val::Nil | Val::Bool(false) => {
                        let t = Table::new();
                        jewelData.set(n!("funcList"), t.clone());
                        t
                    }
                    other => lua_error!(
                        "bad argument #1 to 'insert' (table expected, got {})",
                        other.lua_type()
                    ),
                };
                // L2475
                funcList.push(Val::from_value(&func));
            }
            // L2477
            apply_data_list(&jewelData, &modList, "JewelData");
            // L2480: modList:List(...) is a table, always truthy
            {
                // L2481
                let t = Table::new();
                jewelData.set(n!("impossibleEscapeKeystones"), t.clone());
                // L2482
                apply_data_list(&t, &modList, "ImpossibleEscapeKeystones");
            }
            // L2486
            if let Some(clusterJewel) = self.clusterJewel.get() {
                // L2487
                let notables = Table::new();
                jewelData.set(n!("clusterJewelNotables"), notables.clone());
                // L2488
                for name in modList.List(None, &[n!("ClusterJewelNotable")]) {
                    // L2489
                    notables.push(Val::from_value(&name));
                }
                // L2491
                let added = Table::new();
                jewelData.set(n!("clusterJewelAddedMods"), added.clone());
                // L2492
                for line in modList.List(None, &[n!("AddToClusterJewelNode")]) {
                    // L2493
                    added.push(Val::from_value(&line));
                }

                // Small and Medium Curse Cluster Jewel passive mods are parsed the same so the medium cluster data overwrites small and the skills differ
                // This changes small curse clusters to have the correct clusterJewelSkill so it passes validation below and works as expected in the tree
                let size = clusterJewel.get(n!("size"));
                let skill = jewelData.get(n!("clusterJewelSkill"));
                let is = |v: &Val, s: &str| matches!(v, Val::Str(x) if &**x == s);
                // L2498
                if is(&size, "Small") && is(&skill, "affliction_curse_effect") {
                    // L2499
                    jewelData.set(n!("clusterJewelSkill"), "affliction_curse_effect_small");
                } else if is(&size, "Medium") && is(&skill, "affliction_curse_effect_small") {
                    // L2501
                    jewelData.set(n!("clusterJewelSkill"), "affliction_curse_effect");
                }

                // Validation
                // L2505
                if jewelData.truthy(n!("clusterJewelNodeCount")) {
                    // L2506
                    let c = m_min(
                        m_max(
                            tnum(&jewelData, "clusterJewelNodeCount"),
                            tnum(&clusterJewel, "minNodes"),
                        ),
                        tnum(&clusterJewel, "maxNodes"),
                    );
                    jewelData.set(n!("clusterJewelNodeCount"), c);
                }
                // L2508
                let skill = jewelData.get(n!("clusterJewelSkill"));
                if skill.truthy() {
                    let skills = clusterJewel.get(n!("skills"));
                    let known = match &skills {
                        Val::Table(s) => read_at(s, &skill).truthy(),
                        other => lua_error!(
                            "attempt to index field 'skills' (a {} value)",
                            other.lua_type()
                        ),
                    };
                    if !known {
                        // L2509
                        jewelData.set(n!("clusterJewelSkill"), Val::Nil);
                    }
                }
                // Set missing crafting fields when using advanced copy
                // L2512
                if self.clusterJewelSkill.get().is_none() {
                    match jewelData.get(n!("clusterJewelSkill")) {
                        Val::Str(s) => self.clusterJewelSkill.set(Some(Name::from(&*s))),
                        Val::Nil | Val::Bool(false) => {}
                        other => lua_error!(
                            "clusterJewelSkill: a {} value is not a name",
                            other.lua_type()
                        ),
                    }
                }
                // L2513
                if self.clusterJewelNodeCount.get().is_none() {
                    self.clusterJewelNodeCount
                        .set(jewelData.get(n!("clusterJewelNodeCount")).opt_num());
                }
                // L2514-2516
                let valid = lor(jewelData.get(n!("clusterJewelKeystone")), || {
                    lor(
                        land(
                            lor(jewelData.get(n!("clusterJewelSkill")), || {
                                jewelData.get(n!("clusterJewelSmallsAreNothingness"))
                            }),
                            || jewelData.get(n!("clusterJewelNodeCount")),
                        ),
                        || {
                            land(jewelData.get(n!("clusterJewelSocketCountOverride")), || {
                                jewelData.get(n!("clusterJewelNothingnessCount"))
                            })
                        },
                    )
                });
                jewelData.set(n!("clusterJewelValid"), valid);
            }
        }
        // L2519: return { unpack(modList) }
        let out = ModStore::ModList(None);
        out.AddList(&modList.ipairs());
        out
    }
}

/// @pob Classes/Item.lua:2522-2532 h=29c57056
pub fn getRangedModList(item: &Item, modLine: &Table) -> Option<Vec<Mod>> {
    let _ = item;
    let line = || match modLine.get(n!("line")) {
        Val::Str(s) => s,
        other => lua_error!(
            "attempt to index field 'line' (a {} value)",
            other.lua_type()
        ),
    };
    // L2523
    if !modLine.truthy(n!("range"))
        || pattern::find(&line(), "%((%-?%d+%.?%d*)%-(%-?%d+%.?%d*)%)", None, false).is_none()
    {
        // L2524
        return None;
    }
    // L2526
    let line = itemLib::applyRange(
        &pattern::gsub(&line(), "\n", " ", None).0,
        &modLine.get(n!("range")),
        modLine.opt_num(n!("valueScalar")),
        modLine.opt_num(n!("corruptedRange")),
    );
    // L2527
    let (list, extra) = itemLib::parseMod(&line, false);
    // L2528
    if itemLib::isZeroValueLine(&line) {
        // L2529
        return Some(Vec::new());
    }
    // L2531: not extra and list
    if extra.is_some() {
        None
    } else {
        list
    }
}

/// @pob Classes/Item.lua:2534-2704 h=cd449c3b
impl Item {
    // Build lists of modifiers for each slot the item can occupy
    pub fn BuildModList(&self) {
        // L2535
        let Some(base) = self.base.get() else {
            // L2536
            return;
        };
        // L2538
        let baseList = ModStore::ModList(None);
        // L2539
        if base.weapon.is_some() {
            // L2540
            self.weaponData.set(Some(Table::new()));
        } else if base.armour.is_some() {
            // L2542
            if self.armourData.get().is_none() {
                self.armourData.set(Some(Table::new()));
            }
        } else if base.flask.is_some() {
            // L2544
            self.flaskData.set(Some(Table::new()));
            // L2545
            self.buffModList.set(Some(ModStore::ModList(None)));
        } else if base.tincture.is_some() {
            // L2547
            self.tinctureData.set(Some(Table::new()));
            // L2548
            self.buffModList.set(Some(ModStore::ModList(None)));
        } else if self.r#type.get() == n!("Jewel") {
            // L2550
            self.jewelData.set(Some(Table::new()));
        }
        // L2552
        self.baseModList.set(Some(baseList.clone()));
        // L2553
        self.extra.set(n!("rangeLineList"), Table::new());
        let rangeLineList = || match self.extra.get(n!("rangeLineList")) {
            Val::Table(t) => t,
            _ => unreachable!("set above"),
        };
        // L2554
        let id = self.id.get().unwrap_or(-1.0);
        let modSource: String = format!("Item:{}:{}", tostring_num(id), self.name.get());
        self.modSource.set(Some(LStr::from(modSource.as_str())));
        // L2555
        for modLine in self.buffModLines.get().snapshot() {
            // L2556
            if !modLine.truthy(n!("extra")) && self.CheckModLineVariant(&modLine) {
                // L2557
                let (list, mods) = line_mods(&modLine);
                for (i, r#mod) in mods {
                    // L2558: the line's fresh mods (contracts.md#mod-mutation): replaced in place
                    let mut m = (*r#mod).clone();
                    m.source = Some(AStr::from(modSource.as_str()));
                    let m: ModRef = Arc::new(m);
                    list.set(Key::I(i), Val::Mod(m.clone()));
                    // L2559
                    match self.buffModList.get() {
                        Some(b) => b.AddMod(m),
                        None => lua_error!("bad argument #1 to 'insert' (table expected, got nil)"),
                    }
                }
            }
        }
        // L2563
        let processModLine = |modLine: &Table| {
            // L2564
            if modLine.truthy(n!("disabled")) {
                // L2565
                return;
            }
            // L2567
            let variantCount = self.GetModLineVariantCount(modLine);
            // L2568
            if variantCount > 0.0 {
                // special section for variant over-ride of pre-modifier item parameters
                let line = match modLine.get(n!("line")) {
                    Val::Str(s) => s,
                    other => lua_error!(
                        "attempt to index field 'line' (a {} value)",
                        other.lua_type()
                    ),
                };
                // L2570
                if pattern::find(&line, "Requires Class", None, false).is_some() {
                    // L2571
                    let stripped = pattern::gsub(&line, "{variant:([%d,]+)}", "", None).0;
                    let cr = pattern::r#match(&stripped, "Requires Class (.+)", None)
                        .map(|c| Name::from(&*c[0].to_str()));
                    self.classRestriction.set(cr);
                }
                // handle understood modifier variable properties
                // L2574
                let rangedModList = if !modLine.truthy(n!("extra")) {
                    getRangedModList(self, modLine)
                } else {
                    None
                };
                // L2575
                if let Some(rangedModList) = rangedModList {
                    // L2576
                    modLine.set(n!("modList"), mods_table(rangedModList));
                    // L2577
                    modLine.set(n!("showSlider"), true);
                    // L2578
                    rangeLineList().push(modLine.clone());
                } else if modLine.truthy(n!("modId")) && modLine.truthy(n!("newModId")) {
                    // mutated mod transformation available
                    // L2581
                    rangeLineList().push(modLine.clone());
                }
                // L2583
                if !modLine.truthy(n!("extra")) {
                    // L2584
                    let (list, mods) = line_mods(modLine);
                    for (i, r#mod) in mods {
                        // L2585-2586: modLib.setSource mutates the line's fresh mod and returns it
                        let m: ModRef = Arc::new(setSource(&r#mod, &modSource));
                        list.set(Key::I(i), Val::Mod(m.clone()));
                        let mut k = 1.0;
                        while k <= variantCount {
                            baseList.AddMod(m.clone());
                            k += 1.0;
                        }
                    }
                    // L2589
                    let modTags = modLine.get(n!("modTags"));
                    let n = match &modTags {
                        Val::Nil | Val::Bool(false) => 0,
                        Val::Table(t) => t.len(),
                        other => lua_error!(
                            "attempt to get length of field 'modTags' (a {} value)",
                            other.lua_type()
                        ),
                    };
                    if n > 0 {
                        // L2590
                        self.extra.set(n!("hasModTags"), true);
                    }
                }
            }
        };
        // L2595
        for modLine in self.enchantModLines.get().snapshot() {
            // L2596
            processModLine(&modLine);
        }
        // L2598
        for modLine in self.scourgeModLines.get().snapshot() {
            // L2599
            processModLine(&modLine);
        }
        // L2601
        for modLine in self.classRequirementModLines.get().snapshot() {
            // L2602
            processModLine(&modLine);
        }
        // L2604
        for modLine in self.implicitModLines.get().snapshot() {
            // L2605
            processModLine(&modLine);
        }
        // L2607
        for modLine in self.explicitModLines.get().snapshot() {
            // L2608
            processModLine(&modLine);
        }
        // L2610
        for modLine in self.crucibleModLines.get().snapshot() {
            // L2611
            processModLine(&modLine);
        }
        let name = self.name.get();
        // L2613
        if &*name == "Tabula Rasa, Simple Robe"
            || &*name == "Skin of the Loyal, Simple Robe"
            || &*name == "Skin of the Lords, Simple Robe"
            || &*name == "The Apostate, Cabalist Regalia"
        {
            // Hack to remove the energy shield and base int requirement
            // L2615: NewMod with no source
            let mut v = ValueTable::new();
            v.set(n!("key"), Value::Str(AStr::from("EnergyShield")));
            v.set(n!("value"), Value::Num(0.0));
            new_mod(
                &baseList,
                n!("ArmourData"),
                ModType::LIST,
                Value::Table(Arc::new(v)),
                None,
                &[],
            );
            // L2616
            self.requirements.set(n!("int"), 0.0);
        }
        // L2618
        if calcLocal(&baseList, n!("NoAttributeRequirements"), ModType::FLAG, 0).truthy() {
            // L2619
            self.requirements.set(n!("strMod"), 0.0);
            // L2620
            self.requirements.set(n!("dexMod"), 0.0);
            // L2621
            self.requirements.set(n!("intMod"), 0.0);
        } else {
            // L2623
            let v = m_floor(
                (tnum(&self.requirements, "str")
                    + cl(&baseList, "StrRequirement", ModType::BASE, 0))
                    * (1.0 + cl(&baseList, "StrRequirement", ModType::INC, 0) / 100.0),
            );
            self.requirements.set(n!("strMod"), v);
            // L2624
            let v = m_floor(
                (tnum(&self.requirements, "dex")
                    + cl(&baseList, "DexRequirement", ModType::BASE, 0))
                    * (1.0 + cl(&baseList, "DexRequirement", ModType::INC, 0) / 100.0),
            );
            self.requirements.set(n!("dexMod"), v);
            // L2625
            let v = m_floor(
                (tnum(&self.requirements, "int")
                    + cl(&baseList, "IntRequirement", ModType::BASE, 0))
                    * (1.0 + cl(&baseList, "IntRequirement", ModType::INC, 0) / 100.0),
            );
            self.requirements.set(n!("intMod"), v);
        }
        // L2627
        let grantedSkills: Shared<Vec<Table>> = Shared::new(Vec::new());
        self.grantedSkills.set(grantedSkills.clone());
        // L2628
        for skill in baseList.List(None, &[n!("ExtraSkill")]) {
            // L2629
            if !matches!(vfield(&skill, "name"), Val::Str(ref s) if &**s == "Unknown") {
                // L2630
                let t = Table::new();
                t.set(n!("skillId"), vfield(&skill, "skillId"));
                t.set(n!("level"), vfield(&skill, "level"));
                t.set(n!("noSupports"), vfield(&skill, "noSupports"));
                t.set(n!("source"), modSource.as_str());
                t.set(n!("triggered"), vfield(&skill, "triggered"));
                t.set(n!("triggerChance"), vfield(&skill, "triggerChance"));
                grantedSkills.push(t);
            }
        }
        // L2640
        let socketCount = cl(&baseList, "SocketCount", ModType::BASE, 0);
        // L2641
        self.abyssalSocketCount
            .set(cl(&baseList, "AbyssalSocketCount", ModType::BASE, 0));
        // L2642
        let sockets = self.sockets.get();
        self.extra.set(
            n!("selectableSocketCount"),
            m_max(base.socketLimit.unwrap_or(0.0), sockets.len() as f64)
                - self.abyssalSocketCount.get(),
        );
        // L2643
        if calcLocal(&baseList, n!("NoSockets"), ModType::FLAG, 0).truthy() {
            // Remove all sockets
            // L2645: wipeTable(self.sockets) also drops `colourAlwaysMatches` (CCR-0019 §7)
            sockets.wipe();
            self.extra.set(n!("colourAlwaysMatches"), Val::Nil);
            // L2646
            self.extra.set(n!("selectableSocketCount"), 0.0);
            // L2647
            self.abyssalSocketCount.set(0.0);
        } else if socketCount > 0.0 {
            // Force the socket count to be equal to the stated number
            // L2650
            self.extra.set(n!("selectableSocketCount"), socketCount);
            // L2651
            let mut group = Val::Num(0.0);
            // L2652
            let last = m_max(socketCount, sockets.len() as f64);
            let mut i = 1.0;
            while i <= last {
                // L2653
                if i > socketCount {
                    // L2654: self.sockets[i] = nil (every index past socketCount; truncated below)
                } else if sockets.get(i as usize).is_none() {
                    // L2656
                    let s = Table::new();
                    s.set(n!("color"), self.extra.get(n!("defaultSocketColor")));
                    s.set(n!("group"), group.clone());
                    sockets.push(s);
                } else {
                    // L2661
                    group = sockets.get(i as usize).expect("checked").get(n!("group"));
                }
                i += 1.0;
            }
            if (sockets.len() as f64) > socketCount {
                sockets.with_mut(|v| v.truncate(socketCount as usize));
            }
        } else if self.abyssalSocketCount.get() > 0.0 {
            // Ensure that there are the correct number of abyssal sockets present
            // L2666
            let newSockets: Shared<Vec<Table>> = Shared::new(Vec::new());
            // L2667
            let mut group = Val::Num(0.0);
            // L2668: self.sockets is always a table
            // L2669
            let selectable = self.extra.get(n!("selectableSocketCount")).num();
            for socket in sockets.snapshot() {
                // L2670
                if !matches!(socket.get(n!("color")), Val::Str(ref c) if &**c == "A") {
                    // L2671
                    if newSockets.len() as f64 >= selectable {
                        // L2672
                        break;
                    }
                    // L2674
                    newSockets.push(socket.clone());
                    // L2675
                    group = socket.get(n!("group"));
                }
            }
            // L2679
            let mut i = 1.0;
            while i <= self.abyssalSocketCount.get() {
                // L2680
                group = Val::Num(group.num() + 1.0);
                // L2681
                let s = Table::new();
                s.set(n!("color"), "A");
                s.set(n!("group"), group.clone());
                newSockets.push(s);
                i += 1.0;
            }
            // L2686: a new table (no `colourAlwaysMatches`, CCR-0019 §7)
            self.sockets.set(newSockets);
            self.extra.set(n!("colourAlwaysMatches"), Val::Nil);
        }
        // L2688
        if calcLocal(&baseList, n!("SocketAlwaysMatches"), ModType::FLAG, 0).truthy() {
            // L2689 (CCR-0019 §7)
            self.extra.set(n!("colourAlwaysMatches"), true);
        }
        // L2691
        self.extra.set(
            n!("socketedJewelEffectModifier"),
            1.0 + cl(&baseList, "SocketedJewelEffect", ModType::INC, 0) / 100.0,
        );
        // L2692
        if base.weapon.is_some() || self.r#type.get() == n!("Ring") {
            // L2693: self.slotModList = { }
            for l in &self.slotModList {
                l.set(None);
            }
            // L2694
            for i in 1..=2usize {
                // L2695
                let l = self.BuildModListForSlotNum(&baseList, Some(i as f64));
                self.slotModList[i - 1].set(Some(l));
            }
            // Ring 3
            // L2698
            if self.r#type.get() == n!("Ring") {
                // L2699
                let l = self.BuildModListForSlotNum(&baseList, Some(3.0));
                self.slotModList[2].set(Some(l));
            }
        } else {
            // L2702
            let l = self.BuildModListForSlotNum(&baseList, None);
            self.modList.set(Some(l));
        }
    }
}

/// @pob Classes/Item.lua:2706-2736 h=3cbbe245
impl Item {
    pub fn CanHaveMod(&self, r#mod: &ItemMod, includeTags: Option<&Table>) -> bool {
        let d = data();
        // L2707
        let mut keyMap: ahash::AHashMap<Name, usize> = ahash::AHashMap::new();
        // L2708
        let includeTags: Table = includeTags.cloned().unwrap_or_else(Table::new);
        // L2709
        for (index, key) in r#mod.weightKey.iter().enumerate() {
            // L2710
            keyMap.insert(*key, index + 1);
        }
        // check for uniques with off-tag mods
        let title = self.title.get().map(|t| Name::from(&*t));
        // L2713
        if title.is_some_and(|t| d.casterTagCrucibleUniques.contains(&t)) {
            // L2714
            includeTags.set(n!("caster_unique_weapon"), true);
        }
        // L2716
        if title.is_some_and(|t| d.minionTagCrucibleUniques.contains(&t)) {
            // L2717
            includeTags.set(n!("minion_unique_weapon"), true);
        }
        // L2719
        if let Some(validBases) = self.rareLikeUnique_ref().and_then(|r| {
            r.validBases
                .as_ref()
                .map(|v| v.iter().map(|b| b.base.clone()).collect::<Vec<_>>())
        }) {
            // Some uniques use modifiers from another item type.
            // L2721
            for base in validBases {
                let tags = Table::new();
                for t in base.tags.iter() {
                    tags.set(*t, true);
                }
                // L2722
                if self.GetModSpawnWeight(r#mod, Some(&includeTags), None, Some(&tags)) > 0.0 {
                    // L2723
                    return true;
                }
            }
            // L2726
            return false;
        }
        // L2728
        if self
            .extra
            .truthy(n!("canHaveOnlySupportSkillsCrucibleTree"))
        {
            // L2729
            return match keyMap.get(&n!("crucible_unique_staff")) {
                Some(&i) => r#mod.weightVal.get(i - 1) != Some(&0.0),
                None => false,
            };
        } else if self.extra.truthy(n!("canHaveShieldCrucibleTree")) {
            // L2731
            let inc = Table::new();
            inc.set(n!("crucible_unique_helmet"), true);
            inc.set(n!("shield"), true);
            return self.GetModSpawnWeight(r#mod, Some(&inc), None, None) > 0.0;
        } else if self.extra.truthy(n!("canHaveTwoHandedSwordCrucibleTree")) {
            // L2733
            let inc = Table::new();
            inc.set(n!("two_hand_weapon"), true);
            let exc = Table::new();
            exc.set(n!("one_hand_weapon"), true);
            return self.GetModSpawnWeight(r#mod, Some(&inc), Some(&exc), None) > 0.0;
        }
        // L2735
        self.GetModSpawnWeight(r#mod, Some(&includeTags), None, None) > 0.0
    }
}

#[cfg(test)]
mod fup2_key_tests {
    use super::*;

    /// FOLLOWUPS "FUP1" item 6: `t[0/0]` and `t[nil]` read nil in Lua; a store at NaN raises.
    /// L2261 (`groupCounts[socket.group] = (groupCounts[socket.group] or 0) + 1`) with a NaN
    /// group: the read gives 0 + 1, the store raises "table index is NaN".
    #[test]
    fn nan_and_nil_keys_read_nil() {
        let t = Table::new();
        t.set(n!("Tribal Fury"), true);
        assert!(read_at(&t, &Val::Num(f64::NAN)).is_nil());
        assert!(read_at(&t, &Val::Nil).is_nil());
        assert!(read_at(&t, &Val::from("Tribal Fury")).truthy());
        let group = Val::Num(f64::NAN);
        let count = read_at(&t, &group).num_or(0.0) + 1.0;
        assert_eq!(count, 1.0);
        let e = pob_lua::error::catch(|| t.set(key_of(&group), count)).unwrap_err();
        assert!(e.0.contains("table index is NaN"), "{}", e.0);
    }
}
