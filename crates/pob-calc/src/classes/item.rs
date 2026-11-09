//! @pob-file Classes/Item.lua @16de4b82d
//! @pob-file Modules/ItemSlotHelper.lua @16de4b82d
//! @pob-file Modules/Utils.lua @16de4b82d
//! Classes/Item.lua 1-423, 1646-2191 (Item core), Modules/ItemSlotHelper.lua, Modules/Utils.lua.
//!
//! Owner: IT-B. Signatures were frozen by U0; the deviations are CCR-0032
//! (`GetVariantGroupOptions` returns variant ids, `MutateMod` takes a boolean,
//! `getTagBasedModifiers` returns nothing, `DrawViewer` is UI and skipped).
//!
//! **Representation of the untyped Item fields** (they live in `Item.extra`, keyed by
//! their Lua name, with their Lua shape; CCR-0032 §2 records the convention for IT-A/IT-C):
//! strings are `Val::Str`, numbers `Val::Num`, booleans `Val::Bool`; Lua tables are
//! [`Table`]s (`versionList` = list of strings, `variantGroups[groupId][variantId][versionId]`,
//! `variantGroupSelections[groupId] = variantId`, `prefixes`/`suffixes` = lists of
//! `{ modId, range, fractured }` with an optional `limit`, `baseLines[baseName] = { line,
//! variantList, versionList, variantGroupList }`); `affixes` is a `Val::Obj` holding an
//! [`ItemAffixes`]. Influence flags (`self.shaper`, …) are in `Item.influences`.
//! Mod lines are `Table`s with their Lua keys (`line`, `modTags`, `variantList`, …).
#![allow(non_snake_case, non_upper_case_globals, clippy::all, clippy::pedantic)]

use std::cell::RefCell;

use pob_gamedata::schema::affixes::{AffixTable, ItemMod};
use pob_lua::pattern;

use crate::classes::item_tools as itemLib;
use crate::prelude::*;

// ---- plumbing (Rust-only helpers; snake_case) -------------------------------------------

/// Lua `..` on a value: strings and numbers concatenate, anything else raises.
fn cs(v: &Val) -> String {
    match v {
        Val::Str(s) => s.to_string(),
        Val::Num(n) => tostring_num(*n),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// `t[x]` for a number key: a table cannot hold a non-integral key, so such a read is nil.
fn tget(t: &Table, x: f64) -> Val {
    if x == x.trunc() && x.is_finite() && x.abs() < 9.007_199_254_740_992e15 {
        t.get(Key::I(x as i64))
    } else {
        Val::Nil
    }
}

/// `v[k]` where `v` must be indexable (`nil[k]` raises) and `k` may be nil (a nil key reads nil).
fn vget(v: &Val, k: &Val) -> Val {
    match v {
        Val::Table(t) => match k {
            Val::Nil => Val::Nil,
            Val::Num(x) => tget(t, *x),
            Val::Str(s) => t.get(Name::from(&**s)),
            Val::Bool(b) => t.get(Key::B(*b)),
            _ => Val::Nil,
        },
        Val::Data(_) | Val::Mod(_) => match k {
            Val::Num(x) if *x == x.trunc() && *x >= 1.0 => v.get(Key::I(*x as i64)),
            Val::Str(s) => v.get(Name::from(&**s)),
            _ => Val::Nil,
        },
        other => lua_error!("attempt to index a {} value", other.lua_type()),
    }
}

/// `ipairs(v)` over a calc table or a data list (values snapshot).
fn ipairs_vals(v: &Val) -> Vec<Val> {
    match v {
        Val::Table(t) => t.ipairs().into_iter().map(|(_, v)| v).collect(),
        Val::Data(Value::Table(t)) => t
            .array
            .iter()
            .map(Val::from_value)
            .take_while(|v| !v.is_nil())
            .collect(),
        Val::Nil => lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"),
        other => lua_error!(
            "bad argument #1 to 'ipairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `#v` on a table value (`#nil` raises).
fn vlen(v: &Val) -> usize {
    match v {
        Val::Table(t) => t.len(),
        Val::Data(Value::Table(t)) => t.array.len(),
        Val::Str(s) => s.len(),
        other => lua_error!("attempt to get length of a {} value", other.lua_type()),
    }
}

/// `table.concat(v, sep)` over a list of strings/numbers.
fn tconcat(v: &Val, sep: &str) -> String {
    let parts: Vec<String> = ipairs_vals(v)
        .iter()
        .map(|x| match x {
            Val::Str(_) | Val::Num(_) => cs(x),
            other => lua_error!(
                "invalid value (at index 1) in table for 'concat' ({})",
                other.lua_type()
            ),
        })
        .collect();
    parts.join(sep)
}

/// Lua `<` on two table keys (numbers numerically, strings bytewise; mixed raises).
fn key_lt(a: &Key, b: &Key) -> bool {
    match (a, b) {
        (Key::I(x), Key::I(y)) => x < y,
        (Key::S(x), Key::S(y)) => x.as_str().as_bytes() < y.as_str().as_bytes(),
        (Key::B(_), Key::B(_)) => lua_error!("attempt to compare two boolean values"),
        _ => lua_error!("attempt to compare number with string"),
    }
}

/// The keys `pairsSortByKey(t)` (Modules/Common.lua:657-670) visits, in order:
/// `pairs` keys sorted with `table.sort` and Lua's default `<`.
fn pairsSortByKey(t: &Table) -> Vec<Key> {
    let mut sortedKeys: Vec<Key> = t.pairs().map(|(k, _)| k).collect();
    pob_lua::sort::sort(&mut sortedKeys, key_lt);
    sortedKeys
}

/// `pairsSortByKey` over a table value (`pairs(nil)` raises).
fn pairsSortByKey_val(v: &Val) -> Vec<Key> {
    match v {
        Val::Table(t) => pairsSortByKey(t),
        other => lua_error!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            other.lua_type()
        ),
    }
}

/// `ConPrintf(fmt, ...)`: PoB's console.
fn ConPrintf(s: String) {
    eprintln!("{s}");
}

/// `s:lower()`.
fn lower(s: &str) -> String {
    pob_lua::string::lower(s)
}

/// `s:find(p)` (a Lua pattern; malformed patterns raise as in Lua).
fn lfind(s: &str, p: &str) -> bool {
    pattern::find(s, p, None, false).is_some()
}

/// The Lua value of an optional number field.
fn onum(x: Option<f64>) -> Val {
    x.map_or(Val::Nil, Val::Num)
}

/// The data table an item draws affixes from (`self.affixes`, Item.lua:1032-1039): one of
/// `data.itemMods[...]` or `data.rareLikeUniques[...].affixes`. Stored in `Item.extra`
/// under `affixes` as `Val::Obj(Rc<ItemAffixes>)` (CCR-0032 §2).
pub struct ItemAffixes(pub &'static AffixTable);

impl Item {
    /// `self.<k>` for an untyped field.
    fn ex(&self, k: Name) -> Val {
        self.extra.get(k)
    }

    /// `self.affixes`.
    pub fn affixes_table(&self) -> Option<&'static AffixTable> {
        self.extra.obj::<ItemAffixes>(n!("affixes")).map(|a| a.0)
    }

    /// `self.variantList` as a Lua value (`#`/index reads).
    fn variantList_len(&self) -> usize {
        match self.variantList.get() {
            Some(l) => l.len(),
            None => lua_error!("attempt to get length of field 'variantList' (a nil value)"),
        }
    }
}

// ---- module level --------------------------------------------------------------------------

// @pob Classes/Item.lua:6-29 h=690d5d30
// (module-level statements: ipairs,t_insert,t_remove,m_min,m_max,m_floor,dmgTypeList,catalystList,catalystDescriptorList,catalys…)
// L13
pub const dmgTypeList: [&str; 5] = ["Physical", "Lightning", "Cold", "Fire", "Chaos"];
// L14
pub const catalystList: [&str; 12] = [
    "Abrasive",
    "Accelerating",
    "Dextral",
    "Fertile",
    "Imbued",
    "Intrinsic",
    "Noxious",
    "Prismatic",
    "Sinistral",
    "Tempering",
    "Turbulent",
    "Unstable",
];
// L15
pub const catalystDescriptorList: [&str; 12] = [
    "Attack",
    "Speed",
    "Suffix",
    "Life and Mana",
    "Caster",
    "Attribute",
    "Physical and Chaos",
    "Resistance",
    "Prefix",
    "Defence",
    "Elemental",
    "Critical",
];
// L16
pub const catalystTags: [&[&str]; 12] = [
    &["attack"],
    &["speed"],
    &["suffix"],
    &["life", "mana", "resource"],
    &["caster"],
    &["jewellery_attribute", "attribute"],
    &["physical_damage", "chaos_damage"],
    &["jewellery_resistance", "resistance"],
    &["prefix"],
    &[
        "jewellery_defense",
        "defences",
        "armour",
        "evasion",
        "energyshield",
    ],
    &["jewellery_elemental", "elemental_damage"],
    &["critical"],
];

/// `catalystList[i]` (nil outside 1..12 or for a non-integral index).
pub fn catalystList_get(i: f64) -> Option<&'static str> {
    if i == i.trunc() && i >= 1.0 && i <= catalystList.len() as f64 {
        Some(catalystList[i as usize - 1])
    } else {
        None
    }
}

/// @pob Classes/Item.lua:31-62 h=93b4170f
pub fn getCatalystScalar(catalystId: Option<f64>, r#mod: &Table, quality: Option<f64>) -> f64 {
    // L32
    if r#mod.truthy(n!("unscalable")) {
        return 1.0;
    }
    // L35
    let tags = r#mod.get(n!("modTags"));
    // L36
    let catalystTag: Option<&[&str]> = catalystId.and_then(|c| {
        (c == c.trunc() && c >= 1.0 && c <= catalystTags.len() as f64)
            .then(|| catalystTags[c as usize - 1])
    });
    let Some(catalystTagList) = catalystTag else {
        return 1.0;
    };
    if tags.is_nil() || tags.lua_type() != "table" || vlen(&tags) == 0 {
        return 1.0;
    }
    // L39
    let mut quality = quality;
    if quality.is_none() {
        quality = Some(20.0);
    }
    let quality = quality.unwrap_or(20.0);

    // Create a fast lookup table for all provided tags
    // L44
    let mut tagLookup: Vec<Val> = Vec::new();
    // L45
    for curTag in ipairs_vals(&tags) {
        tagLookup.push(curTag);
    }
    // these aren't actual mod tags but do sinistral/dextral catalyst
    // L49
    for lineFlag in ["prefix", "suffix"] {
        // L50
        if r#mod.truthy(Name::from(lineFlag)) {
            tagLookup.push(Val::from(lineFlag));
        }
    }

    // Find if any of the catalyst's tags match the provided tags
    // L56
    for catalystTag in catalystTagList {
        // L57
        if tagLookup
            .iter()
            .any(|t| matches!(t, Val::Str(s) if &**s == *catalystTag))
        {
            return (100.0 + quality) / 100.0;
        }
    }
    // L61
    1.0
}

/// @pob Classes/Item.lua:64-68 h=fef9a806
pub fn normaliseModLine(line: &str) -> String {
    // L65
    let (s, _) = pattern::gsub(line, "%d+%.?%d*", "#", None);
    let (s, _) = pattern::gsub(&s, "%(%-?#%-#%)", "#", None);
    let s = lower(&s);
    pattern::gsub(&s, "\n", " ", None).0
}

// @pob Classes/Item.lua:70-70 h=1dd79fe5
// (module-level statements: uniqueModStatOrder)
thread_local! {
    /// `local uniqueModStatOrder` (Item.lua:70): `{ exact = { }, normalised = { } }`, built
    /// lazily by ParseRaw (Item.lua:1361-1371, IT-A); module-level state (D17).
    pub static uniqueModStatOrder: RefCell<Option<Table>> = const { RefCell::new(None) };
}

/// @pob Classes/Item.lua:72-87 h=a231bebc
pub fn sortCraftedModLines(modLines: &Shared<Vec<Table>>) {
    // L73
    let mut sourceOrder: Vec<(*const (), usize)> = Vec::new();
    // L74
    for (index, modLine) in modLines.snapshot().iter().enumerate() {
        let p = modLine.as_ptr();
        match sourceOrder.iter_mut().find(|(q, _)| *q == p) {
            Some(e) => e.1 = index + 1,
            None => sourceOrder.push((p, index + 1)),
        }
    }
    let srcOrder = |t: &Table| -> f64 {
        let p = t.as_ptr();
        match sourceOrder.iter().find(|(q, _)| *q == p) {
            Some((_, i)) => *i as f64,
            None => lua_error!("attempt to compare nil with number"),
        }
    };
    let group = |t: &Table| -> f64 {
        if t.truthy(n!("crafted")) || t.truthy(n!("custom")) {
            3.0
        } else if t.truthy(n!("fractured")) {
            1.0
        } else {
            2.0
        }
    };
    // L77
    let mut v = modLines.snapshot();
    pob_lua::sort::sort(&mut v, |a, b| {
        // L78
        let aGroup = group(a);
        // L79
        let bGroup = group(b);
        // L80
        if aGroup != bGroup {
            return aGroup < bGroup;
        } else if aGroup < 3.0 && !a.get(n!("order")).raw_eq(&b.get(n!("order"))) {
            return a.get(n!("order")).num_or(f64::INFINITY)
                < b.get(n!("order")).num_or(f64::INFINITY);
        }
        // L85
        srcOrder(a) < srcOrder(b)
    });
    modLines.with_mut(|m| *m = v);
}

// @pob Classes/Item.lua:89-92 h=0eda4292
// (module-level statements: influenceInfo,ItemClass)

/// One entry of `itemLib.influenceInfo.all` (Modules/ItemTools.lua:16-25, IT-C's module
/// table; the colours are UI-only).
pub struct InfluenceInfo {
    pub key: &'static str,
    pub display: &'static str,
}

/// `local influenceInfo = itemLib.influenceInfo.all` (Item.lua:89).
pub const influenceInfo: [InfluenceInfo; 8] = [
    InfluenceInfo {
        key: "shaper",
        display: "Shaper",
    },
    InfluenceInfo {
        key: "elder",
        display: "Elder",
    },
    InfluenceInfo {
        key: "adjudicator",
        display: "Warlord",
    },
    InfluenceInfo {
        key: "basilisk",
        display: "Hunter",
    },
    InfluenceInfo {
        key: "crusader",
        display: "Crusader",
    },
    InfluenceInfo {
        key: "eyrie",
        display: "Redeemer",
    },
    InfluenceInfo {
        key: "cleansing",
        display: "Searing Exarch",
    },
    InfluenceInfo {
        key: "tangle",
        display: "Eater of Worlds",
    },
];

/// @pob Classes/Item.lua:94-99 h=d86edc1c
/// `new("Item", raw, rarity, highQuality)`.
pub fn new_Item(raw: Option<&str>, rarity: Option<&str>, highQuality: bool) -> ItemRef {
    let item: ItemRef = Rc::new(Item::default());
    // L95
    if let Some(raw) = raw {
        item.ParseRaw(&pob_lua::string::sanitiseText(raw), rarity, highQuality);
    }
    // L98
    item
}

/// @pob Classes/Item.lua:102-106 h=cfd1f19c
impl Item {
    // Reset all influence keys to false
    pub fn ResetInfluence(&self) {
        // L103
        for curInfluenceInfo in &influenceInfo {
            self.influences.set(Name::from(curInfluenceInfo.key), false);
        }
    }
}

// @pob Classes/Item.lua:108-141 h=f99be248
// (module-level statements: influenceItemMap,GenFor,lineFlags,specialModifierFoundList,inverseModifierFoundList)

/// `influenceItemMap[line]` (Item.lua:108-111): `"<display> Item"` → influence key.
pub fn influenceItemMap(line: &str) -> Option<&'static str> {
    // L109
    influenceInfo.iter().find_map(|curInfluenceInfo| {
        (line.len() == curInfluenceInfo.display.len() + 5
            && line.starts_with(curInfluenceInfo.display)
            && line.ends_with(" Item"))
        .then_some(curInfluenceInfo.key)
    })
}

/// `---@enum (key) LineFlags` (Item.lua:114-135).
pub const lineFlags: [&str; 17] = [
    "crafted",
    "crucible",
    "custom",
    "disabled",
    "eater",
    "enchant",
    "exarch",
    "fractured",
    "implicit",
    "scourge",
    "synthesis",
    "mutated",
    "unscalable",
    "prefix",
    "suffix",
    // this is not actually present in advanced copy strings unlike desecration
    // in poe2. this is currently only added as a hack for cane of kulemak and
    // should be added based on mod tags after matching a mod in the future
    "unveiled",
    "vestigial",
];

/// `lineFlags[k]`.
pub fn is_lineFlag(k: &str) -> bool {
    lineFlags.contains(&k)
}

thread_local! {
    // Special function to store unique instances of modifier on specific item slots
    // that require special handling for ItemConditions. Only called if line #224 is
    // uncommented
    // L140
    static specialModifierFoundList: RefCell<ahash::AHashSet<Name>> = RefCell::new(ahash::AHashSet::default());
    // L141
    static inverseModifierFoundList: RefCell<ahash::AHashSet<Name>> = RefCell::new(ahash::AHashSet::default());
}

/// `data.itemTagSpecial[tagName][itemSlotName]` (or the exclusion table).
fn tagSpecialList(
    t: &'static pob_gamedata::schema::item_data::ItemTagSpecial,
    tagName: &str,
    itemSlotName: &str,
) -> Option<&'static Vec<AStr>> {
    t.get(&Name::from(tagName))?.get(&Name::from(itemSlotName))
}

/// @pob Classes/Item.lua:142-286 h=119add6b
/// A GGPK-update debugging helper (only called from a commented-out line in
/// FindModifierSubstring). Lua returns nothing (CCR-0032 §1: the U0 stub returned `Vec<Table>`).
pub fn getTagBasedModifiers(tagName: &str, itemSlotName: &str) {
    let d = data();
    // L143
    let tag_name = lower(tagName);
    // L144
    let slot_name = pattern::gsub(&lower(itemSlotName), " ", "_", None).0;
    let Some(itemModsItem) = d.itemMods.get(&n!("Item")) else {
        lua_error!("bad argument #1 to 'pairs' (table expected, got nil)")
    };
    // iterate all the item modifiers
    // L146 LUA-ORDER: only the order of the console prints depends on it
    for (k, v) in &itemModsItem.0 {
        let k = *k;
        // iterate across the modifier tags for each modifier
        // L148
        for tag in &v.modTags {
            // if tag matches the tag_name we are investigating
            // L150
            if lower(tag.as_str()) == tag_name {
                // L151
                let mut found = false;
                // if there is a valid weightKey table
                // L153
                if !v.weightKey.is_empty() {
                    // L154
                    for wk in &v.weightKey {
                        // and it matches the slot_name of the item we are investigating
                        // L156
                        if wk.as_str() == slot_name {
                            // L157
                            for dv in &v.lines {
                                // and the modifier description contains the tag_name keyword
                                // L159
                                if lfind(&lower(dv), &tag_name) {
                                    found = true;
                                    break;
                                } else {
                                    // L163
                                    let mut excluded = false;
                                    // L164
                                    if let Some(list) =
                                        tagSpecialList(&d.itemTagSpecial, tagName, itemSlotName)
                                    {
                                        // L165
                                        for specialMod in list {
                                            // L166
                                            if lfind(&lower(dv), &lower(specialMod)) {
                                                excluded = true;
                                                break;
                                            }
                                        }
                                    }
                                    // L172
                                    if excluded {
                                        found = true;
                                        break;
                                    }
                                }
                            }
                            // L178
                            if !found && !specialModifierFoundList.with(|s| s.borrow().contains(&k))
                            {
                                specialModifierFoundList.with(|s| s.borrow_mut().insert(k));
                                ConPrintf(lfmt!(
                                    "[%s] [%s] ENTRY: %s",
                                    tagName,
                                    itemSlotName,
                                    k.as_str()
                                ));
                            }
                        }
                    }
                } else {
                    // L185
                    for dv in &v.lines {
                        // L186
                        if lfind(&lower(dv), &tag_name) {
                            found = true;
                            break;
                        } else {
                            // L190
                            let mut excluded = false;
                            // L191
                            if let Some(list) =
                                tagSpecialList(&d.itemTagSpecial, tagName, itemSlotName)
                            {
                                // L192
                                for specialMod in list {
                                    // L193
                                    if lfind(&lower(dv), &lower(specialMod)) {
                                        excluded = true;
                                        break;
                                    }
                                }
                            }
                            // L199
                            if excluded {
                                found = true;
                                break;
                            }
                        }
                    }
                    // L205
                    if !found && !specialModifierFoundList.with(|s| s.borrow().contains(&k)) {
                        specialModifierFoundList.with(|s| s.borrow_mut().insert(k));
                        ConPrintf(lfmt!("[%s] ENTRY: %s", tagName, k.as_str()));
                    }
                }
            }
        }
        // L212
        for dv in &v.lines {
            // L213
            if lfind(&lower(dv), &tag_name) {
                // L214
                let mut found_2 = false;
                // L215
                if !v.weightKey.is_empty() {
                    // L216
                    for wk in &v.weightKey {
                        // L217
                        if wk.as_str() == slot_name {
                            // this is useless if the modTags = { } (is empty)
                            // L219
                            if !v.modTags.is_empty() {
                                // L220
                                for tag in &v.modTags {
                                    // L221
                                    if lower(tag.as_str()) == tag_name {
                                        found_2 = true;
                                        break;
                                    } else {
                                        // L225
                                        let mut excluded = false;
                                        // if we have an exclusion pattern list for that tagName and itemSlotName
                                        // L227
                                        if let Some(list) = tagSpecialList(
                                            &d.itemTagSpecialExclusionPattern,
                                            tagName,
                                            itemSlotName,
                                        ) {
                                            // iterate across the exclusion patterns
                                            // L229
                                            for specialMod in list {
                                                // and if the description matches pattern exclude it
                                                // L231
                                                if lfind(&lower(dv), &lower(specialMod)) {
                                                    excluded = true;
                                                    break;
                                                }
                                            }
                                        }
                                        // L237
                                        if excluded {
                                            found_2 = true;
                                            break;
                                        }
                                    }
                                }
                                // L243
                                if !found_2
                                    && !inverseModifierFoundList.with(|s| s.borrow().contains(&k))
                                {
                                    inverseModifierFoundList.with(|s| s.borrow_mut().insert(k));
                                    ConPrintf(lfmt!(
                                        "[%s] appears in desc but not in tags. [%s] %s",
                                        tag_name.as_str(),
                                        k.as_str(),
                                        &**dv
                                    ));
                                    break;
                                }
                            }
                        }
                    }
                } else {
                    // this is useless if the modTags = { } (is empty)
                    // L253
                    if !v.modTags.is_empty() {
                        // L254
                        for tag in &v.modTags {
                            // L255
                            if lower(tag.as_str()) == tag_name {
                                found_2 = true;
                                break;
                            } else {
                                // L259
                                let mut excluded = false;
                                // if we have an exclusion pattern list for that tagName and itemSlotName
                                // L261
                                if let Some(list) = tagSpecialList(
                                    &d.itemTagSpecialExclusionPattern,
                                    tagName,
                                    itemSlotName,
                                ) {
                                    // iterate across the exclusion patterns
                                    // L263
                                    for specialMod in list {
                                        // and if the description matches pattern exclude it
                                        // L265
                                        if lfind(&lower(dv), &lower(specialMod)) {
                                            excluded = true;
                                            break;
                                        }
                                    }
                                }
                                // L271
                                if excluded {
                                    found_2 = true;
                                    break;
                                }
                            }
                        }
                        // L277
                        if !found_2 && !inverseModifierFoundList.with(|s| s.borrow().contains(&k)) {
                            inverseModifierFoundList.with(|s| s.borrow_mut().insert(k));
                            ConPrintf(lfmt!(
                                "[%s] appears in desc but not in tags. [%s] %s",
                                tag_name.as_str(),
                                k.as_str(),
                                &**dv
                            ));
                        }
                    }
                }
            }
        }
    }
}

/// @pob Classes/Item.lua:289-333 h=767b0498
impl Item {
    // Iterate over modifiers to see if specific substring is found (for conditional checking)
    pub fn FindModifierSubstring(&self, substring: &str, itemSlotName: &str) -> bool {
        let d = data();
        // L290
        let mut modLines: Vec<Table> = Vec::new();
        // L291
        let (substring, explicit) = pattern::gsub(substring, "explicit ", "", None);

        // The commented out line below is used at GGPK updates to check if any new modifiers
        // have been identified that need to be added to the manually maintained special modifier
        // pool in Data.lua (data.itemTagSpecial and data.itemTagSpecialExclusionPattern tables)
        //getTagBasedModifiers(substring, itemSlotName)

        // merge various modifier lines into one table
        // L299
        for v in self.explicitModLines.get().snapshot() {
            modLines.push(v);
        }
        // L300
        if explicit < 1 {
            // L301
            for v in self.enchantModLines.get().snapshot() {
                modLines.push(v);
            }
            // L302
            for v in self.scourgeModLines.get().snapshot() {
                modLines.push(v);
            }
            // L303
            for v in self.implicitModLines.get().snapshot() {
                modLines.push(v);
            }
            // L304
            for v in self.crucibleModLines.get().snapshot() {
                modLines.push(v);
            }
        }

        // L307
        for v in &modLines {
            // L308
            if !v.truthy(n!("disabled")) && self.CheckModLineVariant(v) {
                let line = cs(&v.get(n!("line")));
                // L309
                if lfind(&lower(&line), &substring)
                    && !lfind(&lower(&line), &format!("{substring} modifier"))
                {
                    // L310
                    let mut excluded = false;
                    // L311
                    if let Some(list) =
                        tagSpecialList(&d.itemTagSpecialExclusionPattern, &substring, itemSlotName)
                    {
                        // L312
                        for specialMod in list {
                            // L313
                            if lfind(&lower(&line), &lower(specialMod)) {
                                excluded = true;
                                break;
                            }
                        }
                    }
                    // L319
                    if !excluded {
                        return true;
                    }
                }
                // L323
                if let Some(list) = tagSpecialList(&d.itemTagSpecial, &substring, itemSlotName) {
                    // L324
                    for specialMod in list {
                        // L325
                        if lfind(&lower(&line), &lower(specialMod)) {
                            return true;
                        }
                    }
                }
            }
        }
        // L332
        false
    }
}

/// @pob Classes/Item.lua:335-338 h=4ca5c800
pub fn specToNumber(s: &str) -> Option<f64> {
    // L336
    let n = pattern::r#match(s, "^([%+%-]?[%d%.]+)", None);
    // L337
    n.and_then(|c| c[0].num())
}

/// @pob Classes/Item.lua:340-344 h=6f595d12
impl Item {
    pub fn IsVariantGroupOptionEligible(&self, groupId: f64, variantId: f64) -> bool {
        // L341
        let variantGroups = self.ex(n!("variantGroups"));
        let group = if variantGroups.truthy() {
            vget(&variantGroups, &Val::Num(groupId))
        } else {
            variantGroups
        };
        // L342
        let versions = if group.truthy() {
            vget(&group, &Val::Num(variantId))
        } else {
            group
        };
        // L343
        if !versions.truthy() {
            return false;
        }
        if vget(&versions, &Val::Num(0.0)).truthy() {
            return true;
        }
        let selectedVersion = self.ex(n!("selectedVersion"));
        selectedVersion.truthy() && vget(&versions, &selectedVersion).truthy()
    }
}

/// @pob Classes/Item.lua:346-368 h=f208a3c7
impl Item {
    /// The eligible variant ids of a variant group (CCR-0032 §1: U0's stub returned `Vec<Table>`).
    pub fn GetVariantGroupOptions(&self, groupId: f64, excludeSelected: bool) -> Vec<f64> {
        // L347
        let mut options: Vec<f64> = Vec::new();
        // L348
        let variantGroups = self.ex(n!("variantGroups"));
        if !variantGroups.truthy() || !vget(&variantGroups, &Val::Num(groupId)).truthy() {
            return options;
        }
        // L351
        let mut used: Vec<f64> = Vec::new();
        // L352
        if excludeSelected {
            // L353
            for otherGroupId in pairsSortByKey_val(&variantGroups) {
                let otherGroupId = otherGroupId.to_val();
                // L354
                if !otherGroupId.raw_eq(&Val::Num(groupId)) {
                    // L355
                    let variantId = vget(&self.ex(n!("variantGroupSelections")), &otherGroupId);
                    // L356
                    if variantId.truthy()
                        && self.IsVariantGroupOptionEligible(otherGroupId.num(), variantId.num())
                    {
                        used.push(variantId.num());
                    }
                }
            }
        }
        // L362
        let n = self.variantList_len();
        for variantId in 1..=n {
            let variantId = variantId as f64;
            // L363
            if self.IsVariantGroupOptionEligible(groupId, variantId) && !used.contains(&variantId) {
                options.push(variantId);
            }
        }
        // L367
        options
    }
}

/// @pob Classes/Item.lua:370-408 h=b690a873
impl Item {
    pub fn NormaliseVariantSelections(&self) {
        // L371
        let versionList = self.ex(n!("versionList"));
        if versionList.truthy() && vlen(&versionList) > 0 {
            // L372
            let n = vlen(&versionList) as f64;
            let sel = self.ex(n!("selectedVersion")).num_or(n);
            self.extra
                .set(n!("selectedVersion"), m_max(1.0, m_min(n, sel)));
        } else {
            // L374
            self.extra.set(n!("selectedVersion"), Val::Nil);
        }
        // L376
        let mut variantGroupSelections = self.ex(n!("variantGroupSelections"));
        if !variantGroupSelections.truthy() {
            variantGroupSelections = Val::Table(Table::new());
        }
        self.extra
            .set(n!("variantGroupSelections"), variantGroupSelections.clone());
        let Val::Table(selections) = variantGroupSelections else {
            lua_error!(
                "bad argument #1 to 'pairs' (table expected, got {})",
                variantGroupSelections.lua_type()
            )
        };
        let variantGroups = self.ex(n!("variantGroups"));
        // L377 (int keys; the loop only deletes, so its order is not observable)
        for (groupId, _) in selections.pairs() {
            // L378
            if !vget(&variantGroups, &groupId.to_val()).truthy() {
                selections.set(groupId, Val::Nil);
            }
        }

        // L383
        let mut used: Vec<Val> = Vec::new();
        // L384
        let mut needsSelection: Vec<Key> = Vec::new();
        // L385
        for groupId in pairsSortByKey_val(&variantGroups) {
            let groupIdV = groupId.to_val();
            // L386
            if !self
                .GetVariantGroupOptions(groupIdV.num(), false)
                .is_empty()
            {
                // L387
                let selected = selections.get(groupId);
                // L388
                if selected.truthy()
                    && self.IsVariantGroupOptionEligible(groupIdV.num(), selected.num())
                    && !used.iter().any(|u| u.raw_eq(&selected))
                {
                    used.push(selected);
                } else {
                    // L391
                    needsSelection.push(groupId);
                }
            }
        }
        // L395
        for groupId in needsSelection {
            // L396
            let mut selected: Val = Val::Nil;
            // L397
            for variantId in self.GetVariantGroupOptions(groupId.to_val().num(), false) {
                // L398
                if !used.iter().any(|u| u.raw_eq(&Val::Num(variantId))) {
                    selected = Val::Num(variantId);
                    break;
                }
            }
            // L403
            selections.set(groupId, selected.clone());
            // L404
            if selected.truthy() {
                used.push(selected);
            }
        }
    }
}

/// @pob Classes/Item.lua:1646-1682 h=dd18671b
impl Item {
    /// `mutatedValue` is a boolean in Lua (`not line.mutated`, ItemsTab.lua:1044/1105;
    /// CCR-0032 §1: U0's stub had `Option<f64>`).
    pub fn MutateMod(&self, modId: Name, newModId: Name, mutatedValue: bool) {
        let d = data();
        let category = |c: Name| match d.itemMods.get(&c) {
            Some(t) => t,
            None => lua_error!("attempt to index field '{}' (a nil value)", c.as_str()),
        };
        // L1647
        let newMod: Option<&Arc<ItemMod>> = (if mutatedValue {
            category(n!("Foulborn")).0.get(&newModId)
        } else {
            None
        })
        .or_else(|| category(n!("ItemExclusive")).0.get(&newModId));
        // L1648
        let Some(newMod) = newMod else {
            ConPrintf(lfmt!(
                "Invalid mod id given to MutateMod: %s, %s, %s",
                modId.as_str(),
                newModId.as_str(),
                mutatedValue
            ));
            return;
        };
        // L1652
        let mut i: usize = 1;
        // where we will reinsert the mod lines
        // L1654
        let mut insertIdx: Option<usize> = None;
        // L1655
        let mut variantList = Val::Nil;
        // L1656
        let mut versionList = Val::Nil;
        // L1657
        let mut variantGroupList = Val::Nil;
        let explicitModLines = self.explicitModLines.get();
        // L1658
        while let Some(modLine) = explicitModLines.get(i) {
            // L1659
            // L1660
            if matches!(modLine.get(n!("modId")), Val::Str(s) if &*s == modId.as_str())
                && self.CheckModLineVariant(&modLine)
            {
                // L1661
                if insertIdx.is_none() {
                    insertIdx = Some(i);
                    variantList = modLine.get(n!("variantList"));
                    versionList = modLine.get(n!("versionList"));
                    variantGroupList = modLine.get(n!("variantGroupList"));
                }
                // L1667
                explicitModLines.remove(Some(i));
            } else {
                // L1669
                i += 1;
            }
        }
        // L1672
        let Some(insertIdx) = insertIdx else {
            lua_error!("bad argument #2 to 'insert' (number expected, got nil)")
        };
        // Lua shares `newMod.modTags` (data) by reference; a copy is equivalent because nothing
        // mutates a mod line's tags in place: BuildAndParseRaw re-parses the line, and ParseRaw's
        // t_insert(modLine.modTags, tag) (Item.lua:897) acts on its fresh `{ modTags = {} }` (872).
        let modTags = Table::new();
        for t in &newMod.modTags {
            modTags.push(*t);
        }
        let newLine = Table::new();
        newLine.set(
            n!("line"),
            newMod
                .lines
                .iter()
                .map(|l| &**l)
                .collect::<Vec<&str>>()
                .join("\n"),
        );
        newLine.set(n!("modTags"), modTags);
        newLine.set(n!("variantList"), variantList);
        newLine.set(n!("versionList"), versionList);
        newLine.set(n!("variantGroupList"), variantGroupList);
        newLine.set(n!("mutated"), mutatedValue);
        newLine.set(n!("modGroup"), newModId);
        explicitModLines.insert(insertIdx, newLine);
        // L1681
        self.BuildAndParseRaw();
    }
}

/// @pob Classes/Item.lua:1683-1691 h=b3d94dcd
impl Item {
    pub fn NormaliseQuality(&self) {
        // L1684
        if let Some(base) = self.base.get() {
            if base.armour.is_some()
                || base.weapon.is_some()
                || base.flask.is_some()
                || base.tincture.is_some()
            {
                // L1685
                match self.quality.get() {
                    None => self.quality.set(Some(0.0)),
                    // L1687
                    Some(q) => {
                        if self.uniqueID.get().is_none()
                            && !self.corrupted.get()
                            && !self.split.get()
                            && !self.mirrored.get()
                            && q < 20.0
                        {
                            self.quality.set(Some(20.0));
                        }
                    }
                }
            }
        }
    }
}

/// @pob Classes/Item.lua:1693-1752 h=2ec74720
impl Item {
    pub fn GetModSpawnWeight(
        &self,
        r#mod: &pob_gamedata::schema::affixes::ItemMod,
        includeTags: Option<&Table>,
        excludeTags: Option<&Table>,
        baseTags: Option<&Table>,
    ) -> f64 {
        // L1694
        let mut weight = 0.0;
        // L1695
        if let Some(base) = self.base.get() {
            // L1696: baseTags = baseTags or self.base.tags
            let baseTag = |key: Name| -> bool {
                match baseTags {
                    Some(t) => t.truthy(key),
                    None => base.tags.contains(&key),
                }
            };
            // L1697
            let HasInfluenceTag = |key: Name| -> bool {
                // L1698
                if let Some(influenceTags) = &base.influenceTags {
                    // L1699
                    for curInfluenceInfo in &influenceInfo {
                        let k = Name::from(curInfluenceInfo.key);
                        // L1700
                        if self.influences.truthy(k) && influenceTags.get(&k) == Some(&key) {
                            return true;
                        }
                    }
                }
                // L1705
                false
            };

            // L1708
            let HasMavenInfluence = |modAffix: Option<&AStr>| -> bool {
                match modAffix {
                    Some(a) => lfind(a, "Elevated"),
                    None => lua_error!("attempt to index local 'modAffix' (a nil value)"),
                }
            };
            // L1711
            if self.ex(n!("restrictTag")).truthy() {
                // L1712
                for key in &r#mod.modTags {
                    // L1713
                    let flagName = format!(
                        "no{}",
                        pattern::gsub_fn(
                            key.as_str(),
                            "^%l",
                            |c| Some(c[0].to_str().to_uppercase()),
                            None
                        )
                        .0
                    );
                    // L1714
                    if self.ex(Name::from(flagName.as_str())).truthy() {
                        return 0.0;
                    }
                }
            }
            // L1719
            if self.ex(n!("restrictDamageType")).truthy() {
                // L1720
                let mut required = false;
                let mut restricted: Vec<&str> = Vec::new();
                // L1721
                for element in ["fire", "cold", "lightning", "chaos", "physical"] {
                    // L1722
                    let flagName = format!(
                        "only{}Damage",
                        pattern::gsub_fn(
                            element,
                            "^%l",
                            |c| Some(c[0].to_str().to_uppercase()),
                            None
                        )
                        .0
                    );
                    // L1723
                    if self.ex(Name::from(flagName.as_str())).truthy() {
                        required = true;
                    } else {
                        restricted.push(element);
                    }
                }
                // L1729
                if required {
                    // L1730
                    for key in &r#mod.modTags {
                        // L1731
                        if restricted.contains(&key.as_str()) {
                            return 0.0;
                        }
                    }
                }
            }

            // L1738
            for (i, key) in r#mod.weightKey.iter().enumerate() {
                let key = *key;
                // L1739
                if (baseTag(key)
                    || includeTags.is_some_and(|t| t.truthy(key))
                    || HasInfluenceTag(key))
                    && !excludeTags.is_some_and(|t| t.truthy(key))
                {
                    // L1740
                    weight = if HasInfluenceTag(key) && HasMavenInfluence(r#mod.affix.as_ref()) {
                        1000.0
                    } else {
                        match r#mod.weightVal.get(i) {
                            Some(w) => *w,
                            None => lua_error!("weightVal[{}] is nil", i + 1),
                        }
                    };
                    break;
                }
            }
            // L1744
            for (i, key) in r#mod.weightMultiplierKey.iter().enumerate() {
                let key = *key;
                // L1745
                if (baseTag(key)
                    || includeTags.is_some_and(|t| t.truthy(key))
                    || HasInfluenceTag(key))
                    && !excludeTags.is_some_and(|t| t.truthy(key))
                {
                    // L1746
                    let m = match r#mod.weightMultiplierVal.get(i) {
                        Some(w) => *w,
                        None => lua_error!("attempt to perform arithmetic on a nil value"),
                    };
                    weight = weight * m / 100.0;
                    break;
                }
            }
        }
        // L1751
        weight
    }
}

/// @pob Classes/Item.lua:1754-1765 h=99b93939
impl Item {
    pub fn GetNecropolisModSpawnWeight(
        &self,
        r#mod: &pob_gamedata::schema::affixes::ItemMod,
    ) -> f64 {
        // L1755
        let mut weight = 0.0;
        // L1756
        if let Some(base) = self.base.get() {
            // L1757
            for (i, key) in r#mod.weightKey.iter().enumerate() {
                // L1758
                let k = pattern::gsub(key.as_str(), "necropolis_", "", None).0;
                if base.tags.contains(&Name::from(k.as_str())) {
                    // L1759
                    weight = match r#mod.weightVal.get(i) {
                        Some(w) => *w,
                        None => lua_error!("weightVal[{}] is nil", i + 1),
                    };
                    break;
                }
            }
        }
        // L1764
        weight
    }
}

/// @pob Classes/Item.lua:1767-1769 h=f6c5dd40
impl Item {
    pub fn CheckIfModIsDelve(&self, r#mod: &pob_gamedata::schema::affixes::ItemMod) -> bool {
        // L1768
        matches!(
            r#mod.affix.as_deref(),
            Some("Subterranean") | Some("of the Underground")
        )
    }
}

/// `writeModLine(modLine)` (Item.lua:1842-1927), a closure of BuildRaw.
fn writeModLine(rawLines: &mut Vec<String>, modLine: &Table) {
    // L1843
    let mut line = cs(&modLine.get(n!("line")));
    // L1844
    fn prependToAllLines(line: &mut String, prefix: &str) {
        // L1845
        let rest = pattern::gsub(line, "\n", &format!("\n{prefix}"), None).0;
        *line = format!("{prefix}{rest}");
    }
    // L1847
    fn makeIdSpec(idList: &Val) -> String {
        // L1848
        let mut ids: Vec<String> = Vec::new();
        // L1849
        for id in pairsSortByKey_val(idList) {
            ids.push(cs(&id.to_val()));
        }
        // L1852
        ids.join(",")
    }
    // L1854
    let range = modLine.get(n!("range"));
    if range.truthy() && pattern::r#match(&line, "%(%-?[%d%.]+%-%-?[%d%.]+%)", None).is_some() {
        line = format!(
            "{{range:{}}}{line}",
            tostring_num(round(range.num(), Some(6)))
        );
    }
    // L1857
    let corruptedRange = modLine.get(n!("corruptedRange"));
    if corruptedRange.truthy() {
        line = format!(
            "{{corruptedRange:{}}}{line}",
            tostring_num(round(corruptedRange.num(), Some(2)))
        );
    }
    // L1860
    if modLine.truthy(n!("disabled")) {
        line = format!("{{disabled}}{line}");
    }
    // L1863
    if modLine.truthy(n!("crafted")) {
        line = format!("{{crafted}}{line}");
    }
    // L1866
    if modLine.truthy(n!("enchant")) {
        line = format!("{{enchant}}{line}");
    }
    // L1869
    if modLine.truthy(n!("custom")) {
        line = format!("{{custom}}{line}");
    }
    // L1872
    if modLine.truthy(n!("scourge")) {
        line = format!("{{scourge}}{line}");
    }
    // L1875
    if modLine.truthy(n!("crucible")) {
        line = format!("{{crucible}}{line}");
    }
    // ggg tag for cultivated/foulborn mod
    // L1879
    if modLine.truthy(n!("mutated")) {
        line = format!("{{mutated}}{line}");
    }
    // L1882
    let modGroup = modLine.get(n!("modGroup"));
    if modGroup.truthy() {
        // L1883
        let modGroup = format!("{{modGroup:{}}}", cs(&modGroup));
        // L1884
        let rest = pattern::gsub(&line, "\n", &format!("\n{modGroup}"), None).0;
        line = format!("{modGroup}{rest}");
    }
    // L1886
    if modLine.truthy(n!("fractured")) {
        line = format!("{{fractured}}{line}");
    }
    // L1889
    if modLine.truthy(n!("prefix")) {
        line = format!("{{prefix}}{line}");
    }
    // L1892
    if modLine.truthy(n!("suffix")) {
        line = format!("{{suffix}}{line}");
    }
    // L1895
    if modLine.truthy(n!("exarch")) {
        line = format!("{{exarch}}{line}");
    }
    // L1898
    if modLine.truthy(n!("eater")) {
        line = format!("{{eater}}{line}");
    }
    // L1901
    if modLine.truthy(n!("synthesis")) {
        line = format!("{{synthesis}}{line}");
    }
    // L1904
    if modLine.truthy(n!("unscalable")) {
        line = format!("{{unscalable}}{line}");
    }
    // L1907
    if modLine.truthy(n!("vestigial")) {
        line = format!("{{vestigial}}{line}");
    }
    // L1910
    let versionList = modLine.get(n!("versionList"));
    let variantGroupList = modLine.get(n!("variantGroupList"));
    let hasNewSelection = versionList.truthy() || variantGroupList.truthy();
    let modTags = modLine.get(n!("modTags"));
    // L1911
    if hasNewSelection && modTags.truthy() && vlen(&modTags) > 0 {
        line = format!("{{tags:{}}}{line}", tconcat(&modTags, ","));
    }
    // L1914
    if variantGroupList.truthy() {
        prependToAllLines(
            &mut line,
            &format!("{{group:{}}}", makeIdSpec(&variantGroupList)),
        );
    }
    // L1917
    let variantList = modLine.get(n!("variantList"));
    if variantList.truthy() {
        prependToAllLines(
            &mut line,
            &format!("{{variant:{}}}", makeIdSpec(&variantList)),
        );
    }
    // L1920
    if versionList.truthy() {
        prependToAllLines(
            &mut line,
            &format!("{{version:{}}}", makeIdSpec(&versionList)),
        );
    }
    // L1923
    if !hasNewSelection && modTags.truthy() && vlen(&modTags) > 0 {
        line = format!("{{tags:{}}}{line}", tconcat(&modTags, ","));
    }
    // L1926
    rawLines.push(line);
}

/// @pob Classes/Item.lua:1772-2047 h=ec27f0b6
impl Item {
    pub fn BuildRaw(&self) -> String {
        // L1773
        let mut rawLines: Vec<String> = Vec::new();
        // L1774
        rawLines.push(format!("Rarity: {}", self.rarity.get().as_str()));
        // L1775
        if let Some(title) = self.title.get() {
            // L1776
            rawLines.push(title.to_string());
            // L1777: t_insert(rawLines, nil) is a no-op
            if let Some(baseName) = self.baseName.get() {
                rawLines.push(baseName.as_str().to_owned());
            }
        } else {
            // L1779
            let namePrefix = lor(self.ex(n!("namePrefix")), || Val::from(""));
            let Some(baseName) = self.baseName.get() else {
                lua_error!("attempt to concatenate field 'baseName' (a nil value)")
            };
            let nameSuffix = lor(self.ex(n!("nameSuffix")), || Val::from(""));
            rawLines.push(format!(
                "{}{}{}",
                cs(&namePrefix),
                baseName.as_str(),
                cs(&nameSuffix)
            ));
        }
        // L1781
        if let Some(armourData) = self.armourData.get() {
            // L1782
            for r#type in ["Armour", "Evasion", "EnergyShield", "Ward"] {
                let v = armourData.get(Name::from(r#type));
                // L1783
                if v.truthy() && v.num() > 0.0 {
                    // L1784
                    rawLines.push(format!(
                        "{}: {}",
                        pattern::gsub(r#type, "EnergyShield", "Energy Shield", None).0,
                        cs(&v)
                    ));
                    // L1785
                    let pk = format!("{type}BasePercentile", type = r#type);
                    let p = armourData.get(Name::from(pk.as_str()));
                    if p.truthy() {
                        // L1786
                        rawLines.push(format!("{pk}: {}", cs(&p)));
                    }
                }
            }
        }
        // L1791
        let intangibility = self.ex(n!("intangibility"));
        if intangibility.truthy() {
            // L1792
            rawLines.push(lfmt!("Intangibility: %d%%", intangibility.num()));
        }
        // L1794
        if let Some(uniqueID) = self.uniqueID.get() {
            // L1795
            rawLines.push(format!("Unique ID: {uniqueID}"));
        }
        // L1797
        let league = self.ex(n!("league"));
        if league.truthy() {
            // L1798
            rawLines.push(format!("League: {}", cs(&league)));
        }
        // L1800
        if self.ex(n!("unreleased")).truthy() {
            // L1801
            rawLines.push("Unreleased: true".to_owned());
        }
        // L1803
        for curInfluenceInfo in &influenceInfo {
            // L1804
            if self.influences.truthy(Name::from(curInfluenceInfo.key)) {
                // L1805
                rawLines.push(format!("{} Item", curInfluenceInfo.display));
            }
        }
        // L1808
        if self.ex(n!("crafted")).truthy() {
            // L1809
            rawLines.push("Crafted: true".to_owned());
            // L1810
            for (label, list) in [("Prefix", n!("prefixes")), ("Suffix", n!("suffixes"))] {
                let list = self.ex(list);
                let affixes = if list.truthy() {
                    ipairs_vals(&list)
                } else {
                    Vec::new()
                };
                // L1810 / L1814
                for affix in affixes {
                    // L1811 / L1815
                    let r = vget(&affix, &Val::from("range"));
                    let range = if r.truthy() {
                        let inner = if r.lua_type() == "table" {
                            tconcat(&r, ",")
                        } else {
                            tostring_num(round(r.num(), Some(3)))
                        };
                        format!("{{range:{inner}}}")
                    } else {
                        String::new()
                    };
                    // L1812 / L1816
                    let fractured = if vget(&affix, &Val::from("fractured")).truthy() {
                        "{fractured}"
                    } else {
                        ""
                    };
                    rawLines.push(format!(
                        "{label}: {fractured}{range}{}",
                        cs(&vget(&affix, &Val::from("modId")))
                    ));
                }
            }
        }
        // L1819
        if let Some(catalyst) = self.catalyst.get() {
            if catalyst > 0.0 {
                // L1820
                match catalystList_get(catalyst) {
                    Some(c) => rawLines.push(format!("Catalyst: {c}")),
                    None => lua_error!("attempt to concatenate a nil value"),
                }
            }
        }
        // L1822
        if let Some(catalystQuality) = self.catalystQuality.get() {
            // L1823
            rawLines.push(format!(
                "CatalystQuality: {}",
                tostring_num(catalystQuality)
            ));
        }
        // L1825
        if self.clusterJewel.get().is_some() {
            // L1826
            if let Some(clusterJewelSkill) = self.clusterJewelSkill.get() {
                // L1827
                rawLines.push(format!(
                    "Cluster Jewel Skill: {}",
                    clusterJewelSkill.as_str()
                ));
            }
            // L1829
            if let Some(clusterJewelNodeCount) = self.clusterJewelNodeCount.get() {
                // L1830
                rawLines.push(format!(
                    "Cluster Jewel Node Count: {}",
                    tostring_num(clusterJewelNodeCount)
                ));
            }
        }
        // L1833
        if let Some(talismanTier) = self.talismanTier.get() {
            // L1834
            rawLines.push(format!("Talisman Tier: {}", tostring_num(talismanTier)));
        }
        // L1836
        if let Some(itemLevel) = self.itemLevel.get() {
            // L1837
            rawLines.push(format!("Item Level: {}", tostring_num(itemLevel)));
        }
        // L1839
        let memoryStrands = self.ex(n!("memoryStrands"));
        if memoryStrands.truthy() {
            // L1840
            rawLines.push(format!("Memory Strands: {}", cs(&memoryStrands)));
        }
        // L1842: writeModLine (above)
        // L1928
        let versionList = self.ex(n!("versionList"));
        if versionList.truthy() {
            // L1929
            for versionName in ipairs_vals(&versionList) {
                // L1930
                rawLines.push(format!("Version: {}", cs(&versionName)));
            }
            // L1932
            let selectedVersion = self.ex(n!("selectedVersion"));
            if selectedVersion.truthy() {
                // L1933
                rawLines.push(format!("Selected Version: {}", cs(&selectedVersion)));
            }
        }
        // L1936
        let usesVariantGroups = self.ex(n!("usesVariantGroups")).truthy();
        if let Some(variantList) = self.variantList.get() {
            // L1937
            for variantName in variantList.snapshot() {
                // L1938
                rawLines.push(format!("Variant: {variantName}"));
            }
            // L1940
            if usesVariantGroups {
                // L1941
                let selections = self.ex(n!("variantGroupSelections"));
                for groupId in pairsSortByKey_val(&self.ex(n!("variantGroups"))) {
                    let groupId = groupId.to_val();
                    // L1942
                    let variantId = vget(&selections, &groupId);
                    // L1943
                    if variantId.truthy() {
                        // L1944
                        rawLines.push(format!(
                            "Selected Variant Group: {}={}",
                            cs(&groupId),
                            cs(&variantId)
                        ));
                    }
                }
            } else {
                // L1948
                rawLines.push(format!(
                    "Selected Variant: {}",
                    cs(&onum(self.variant.get()))
                ));
            }

            // L1951 LUA-ORDER: baseLines is keyed by base name (insertion order here, random in PoB)
            let baseLines = lor(self.ex(n!("baseLines")), || Val::Table(Table::new()));
            if let Val::Table(baseLines) = &baseLines {
                for (_, baseLine) in baseLines.pairs() {
                    let Val::Table(baseLine) = baseLine else {
                        lua_error!(
                            "attempt to index local 'baseLine' (a {} value)",
                            baseLine.lua_type()
                        )
                    };
                    // L1952
                    if baseLine.truthy(n!("variantList"))
                        || baseLine.truthy(n!("versionList"))
                        || baseLine.truthy(n!("variantGroupList"))
                    {
                        // L1953
                        writeModLine(&mut rawLines, &baseLine);
                    }
                }
            }
            // L1956
            let alts = [
                (
                    n!("hasAltVariant"),
                    &self.variantAlt,
                    "Has Alt Variant: true",
                    "Selected Alt Variant: ",
                ),
                (
                    n!("hasAltVariant2"),
                    &self.variantAlt2,
                    "Has Alt Variant Two: true",
                    "Selected Alt Variant Two: ",
                ),
                (
                    n!("hasAltVariant3"),
                    &self.variantAlt3,
                    "Has Alt Variant Three: true",
                    "Selected Alt Variant Three: ",
                ),
                (
                    n!("hasAltVariant4"),
                    &self.variantAlt4,
                    "Has Alt Variant Four: true",
                    "Selected Alt Variant Four: ",
                ),
                (
                    n!("hasAltVariant5"),
                    &self.variantAlt5,
                    "Has Alt Variant Five: true",
                    "Selected Alt Variant Five: ",
                ),
            ];
            // L1956 / L1960 / L1964 / L1968 / L1972
            for (has, variantAlt, hasLine, selLine) in alts {
                if !usesVariantGroups && self.ex(has).truthy() {
                    rawLines.push(hasLine.to_owned());
                    rawLines.push(format!("{selLine}{}", cs(&onum(variantAlt.get()))));
                }
            }
            // L1976
            if self.ex(n!("allowDuplicateVariants")).truthy() {
                // L1977
                rawLines.push("Allow Duplicate Variants: true".to_owned());
            }
        }
        // L1980
        if self.variantList.get().is_none() {
            // L1981 LUA-ORDER: see L1951
            let baseLines = lor(self.ex(n!("baseLines")), || Val::Table(Table::new()));
            if let Val::Table(baseLines) = &baseLines {
                for (_, baseLine) in baseLines.pairs() {
                    let Val::Table(baseLine) = baseLine else {
                        lua_error!(
                            "attempt to index local 'baseLine' (a {} value)",
                            baseLine.lua_type()
                        )
                    };
                    // L1982
                    if baseLine.truthy(n!("versionList")) || baseLine.truthy(n!("variantGroupList"))
                    {
                        // L1983
                        writeModLine(&mut rawLines, &baseLine);
                    }
                }
            }
        }
        // L1987
        if let Some(quality) = self.quality.get() {
            // L1988
            rawLines.push(format!("Quality: {}", tostring_num(quality)));
        }
        // L1990
        let sockets = self.sockets.get().snapshot();
        if !sockets.is_empty() {
            // L1991
            let mut line = String::from("Sockets: ");
            // L1992
            for (i, socket) in sockets.iter().enumerate() {
                // L1993
                line.push_str(&cs(&socket.get(n!("color"))));
                // L1994
                if let Some(next) = sockets.get(i + 1) {
                    // L1995
                    line.push_str(if socket.get(n!("group")).raw_eq(&next.get(n!("group"))) {
                        "-"
                    } else {
                        " "
                    });
                }
            }
            // L1998
            rawLines.push(line);
        }
        // L2000
        let level = self.requirements.get(n!("level"));
        if level.truthy() {
            // L2001
            rawLines.push(format!("LevelReq: {}", cs(&level)));
        }
        // L2003
        if let Some(jewelRadiusLabel) = self.jewelRadiusLabel.get() {
            // L2004
            rawLines.push(format!("Radius: {jewelRadiusLabel}"));
        }
        // L2006
        if let Some(limit) = self.limit.get() {
            // L2007
            rawLines.push(format!("Limited to: {}", tostring_num(limit)));
        }
        // L2009
        if let Some(classRestriction) = self.classRestriction.get() {
            // L2010
            rawLines.push(format!("Requires Class {}", classRestriction.as_str()));
        }
        // L2012
        rawLines.push(format!(
            "Implicits: {}",
            tostring_num(
                self.enchantModLines.get().len() as f64
                    + self.implicitModLines.get().len() as f64
                    + self.scourgeModLines.get().len() as f64
            )
        ));
        // L2013
        for modLine in self.enchantModLines.get().snapshot() {
            writeModLine(&mut rawLines, &modLine);
        }
        // L2016
        for modLine in self.scourgeModLines.get().snapshot() {
            writeModLine(&mut rawLines, &modLine);
        }
        // L2019
        for modLine in self.classRequirementModLines.get().snapshot() {
            writeModLine(&mut rawLines, &modLine);
        }
        // L2022
        for modLine in self.implicitModLines.get().snapshot() {
            writeModLine(&mut rawLines, &modLine);
        }
        // L2025
        for modLine in self.explicitModLines.get().snapshot() {
            writeModLine(&mut rawLines, &modLine);
        }
        // L2028
        for modLine in self.crucibleModLines.get().snapshot() {
            writeModLine(&mut rawLines, &modLine);
        }
        // L2031
        if self.split.get() {
            rawLines.push("Split".to_owned());
        }
        // L2034
        if self.mirrored.get() {
            rawLines.push("Mirrored".to_owned());
        }
        // L2037
        if self.fractured.get() {
            rawLines.push("Fractured Item".to_owned());
        }
        // L2040
        if self.corrupted.get() || self.ex(n!("scourge")).truthy() {
            rawLines.push("Corrupted".to_owned());
        }
        // L2043
        let foilType = self.ex(n!("foilType"));
        if foilType.truthy() {
            // L2044
            rawLines.push(format!("Foil Unique ({})", cs(&foilType)));
        }
        // L2046
        rawLines.join("\n")
    }
}

/// @pob Classes/Item.lua:2049-2052 h=4d709cd8
impl Item {
    pub fn BuildAndParseRaw(&self) {
        // L2050
        let raw = self.BuildRaw();
        // L2051
        self.ParseRaw(&raw, None, false);
    }
}

/// @pob Classes/Item.lua:2055-2118 h=cef182f1
impl Item {
    // Rebuild explicit modifiers using the item's affixes
    pub fn Craft(&self) {
        // Save off any crafted or custom mods so they can be re-added at the end
        // L2057
        let mut savedMods: Vec<Table> = Vec::new();
        let explicitModLines = self.explicitModLines.get();
        // L2058
        for r#mod in explicitModLines.snapshot() {
            // L2059
            if r#mod.truthy(n!("crafted"))
                || r#mod.truthy(n!("custom"))
                || (self.ex(n!("rareLikeUnique")).truthy()
                    && !(r#mod.truthy(n!("prefix")) || r#mod.truthy(n!("suffix"))))
            {
                savedMods.push(r#mod);
            }
        }

        // L2064
        explicitModLines.wipe();
        // L2065
        self.extra.set(n!("namePrefix"), "");
        // L2066
        self.extra.set(n!("nameSuffix"), "");
        // L2067
        let Some(base) = self.base.get() else {
            lua_error!("attempt to index field 'base' (a nil value)")
        };
        self.requirements.set(n!("level"), onum(base.req.level));
        // L2068
        let statOrder = Table::new();
        // L2069
        // ipairs({ self.prefixes, self.suffixes }) stops at the first nil
        for listName in [n!("prefixes"), n!("suffixes")] {
            let list = match self.ex(listName) {
                Val::Nil => break,
                Val::Table(list) => list,
                other => lua_error!(
                    "attempt to index local 'list' (a {} value)",
                    other.lua_type()
                ),
            };
            // L2070
            let limit = lor(list.get(n!("limit")), || {
                Val::Num(self.ex(n!("affixLimit")).num() / 2.0)
            })
            .num();
            let mut i = 1.0;
            while i <= limit {
                // L2071
                let affix = tget(&list, i);
                // L2072
                if !affix.truthy() {
                    list.set(Key::from(i), tbl! { modId = "None" });
                }
                // L2075: `affix` is still the nil read above
                let modId = vget(&affix, &Val::from("modId"));
                let Some(affixes) = self.affixes_table() else {
                    lua_error!("attempt to index field 'affixes' (a nil value)")
                };
                let r#mod = match &modId {
                    Val::Str(s) => affixes.0.get(&Name::from(&**s)),
                    _ => None,
                };
                // L2076
                if let Some(r#mod) = r#mod {
                    // L2077
                    let affixName = || match &r#mod.affix {
                        Some(a) => a.to_string(),
                        None => lua_error!("attempt to concatenate field 'affix' (a nil value)"),
                    };
                    if r#mod.r#type == Some(n!("Prefix")) {
                        // L2078
                        let p = cs(&self.ex(n!("namePrefix")));
                        self.extra
                            .set(n!("namePrefix"), format!("{} {p}", affixName()));
                    } else if r#mod.r#type == Some(n!("Suffix")) {
                        // L2080
                        let s = cs(&self.ex(n!("nameSuffix")));
                        self.extra
                            .set(n!("nameSuffix"), format!("{s} {}", affixName()));
                    }
                    // L2082
                    let Some(modLevel) = r#mod.level else {
                        lua_error!("attempt to perform arithmetic on field 'level' (a nil value)")
                    };
                    self.requirements.set(
                        n!("level"),
                        m_max(
                            self.requirements.num_or(n!("level"), 0.0),
                            m_floor(modLevel * 0.8),
                        ),
                    );
                    // L2083
                    for (i, line) in r#mod.lines.iter().enumerate() {
                        // L2084
                        // `affix.range` is a number or a list (Item.lua:803-809), passed as is
                        // (CCR-0032 §4)
                        let range = lor(vget(&affix, &Val::from("range")), || Val::Num(0.5));
                        let line = itemLib::applyRange(line, &range, None, None);
                        // L2085
                        let order = onum(r#mod.statOrder.get(i).copied());
                        // L2086
                        let existing = vget(&Val::Table(statOrder.clone()), &order);
                        if let Val::Table(existing) = existing {
                            // Combine stats
                            // L2088
                            let mut start: i64 = 1;
                            // L2089
                            let combined = pattern::gsub_fn(
                                &cs(&existing.get(n!("line"))),
                                "%d+",
                                |num| {
                                    // L2090
                                    let Some((_, e, other)) =
                                        pattern::find(&line, "(%d+)", Some(start), false)
                                    else {
                                        lua_error!("attempt to perform arithmetic on local 'e' (a nil value)")
                                    };
                                    // L2091
                                    start = e as i64 + 1;
                                    // L2092
                                    let a = num[0].num().unwrap_or(f64::NAN);
                                    let b = other[0].num().unwrap_or(f64::NAN);
                                    Some(tostring_num(a + b))
                                },
                                None,
                            )
                            .0;
                            existing.set(n!("line"), combined);
                        } else {
                            // L2095 (`mod.modTags` copied, not shared: see MutateMod)
                            let modTags = Table::new();
                            for t in &r#mod.modTags {
                                modTags.push(*t);
                            }
                            let modLine = Table::new();
                            modLine.set(n!("line"), line.as_str());
                            modLine.set(n!("order"), order.clone());
                            modLine.set(n!("type"), r#mod.r#type.map_or(Val::Nil, Val::from));
                            modLine.set(n!("modTags"), modTags);
                            modLine.set(n!("fractured"), vget(&affix, &Val::from("fractured")));
                            // L2096
                            if r#mod.r#type == Some(n!("Prefix")) {
                                modLine.set(n!("prefix"), true);
                            } else if r#mod.r#type == Some(n!("Suffix")) {
                                // L2099
                                modLine.set(n!("suffix"), true);
                            }
                            // L2101
                            explicitModLines.push(modLine.clone());
                            // L2102
                            match order {
                                Val::Num(o) => statOrder.set(Key::from(o), modLine),
                                _ => lua_error!("table index is nil"),
                            }
                        }
                    }
                }
                i += 1.0;
            }
        }

        // Restore the crafted and custom mods
        // L2110
        for r#mod in savedMods {
            explicitModLines.push(r#mod);
        }
        // L2113
        if explicitModLines.len() > 1 {
            sortCraftedModLines(&explicitModLines);
        }

        // L2117
        self.BuildAndParseRaw();
    }
}

/// @pob Classes/Item.lua:2120-2146 h=c1722bca
impl Item {
    pub fn CheckModLineVariant(&self, modLine: &Table) -> bool {
        let variantList = modLine.get(n!("variantList"));
        // L2121
        if self.ex(n!("usesVariantGroups")).truthy() {
            // L2122
            let versionList = modLine.get(n!("versionList"));
            if versionList.truthy() {
                let selectedVersion = self.ex(n!("selectedVersion"));
                if !selectedVersion.truthy() || !vget(&versionList, &selectedVersion).truthy() {
                    return false;
                }
            }
            // L2125
            let variantGroupList = modLine.get(n!("variantGroupList"));
            if variantGroupList.truthy() {
                // L2126
                if !variantList.truthy() {
                    return false;
                }
                let Val::Table(groups) = &variantGroupList else {
                    lua_error!(
                        "bad argument #1 to 'pairs' (table expected, got {})",
                        variantGroupList.lua_type()
                    )
                };
                let selections = self.ex(n!("variantGroupSelections"));
                // L2129 (int keys; the boolean result does not depend on the order)
                for (groupId, _) in groups.pairs() {
                    // L2130
                    let selectedVariant = vget(&selections, &groupId.to_val());
                    // L2131
                    if selectedVariant.truthy() && vget(&variantList, &selectedVariant).truthy() {
                        return true;
                    }
                }
                // L2135
                return false;
            }
            // L2137
            return !variantList.truthy();
        }
        // L2139
        if !variantList.truthy() {
            return true;
        }
        if vget(&variantList, &onum(self.variant.get())).truthy() {
            return true;
        }
        let alts = [
            (n!("hasAltVariant"), &self.variantAlt),
            (n!("hasAltVariant2"), &self.variantAlt2),
            (n!("hasAltVariant3"), &self.variantAlt3),
            (n!("hasAltVariant4"), &self.variantAlt4),
            (n!("hasAltVariant5"), &self.variantAlt5),
        ];
        // L2141-2145
        for (has, variantAlt) in alts {
            if self.ex(has).truthy() && vget(&variantList, &onum(variantAlt.get())).truthy() {
                return true;
            }
        }
        false
    }
}

/// @pob Classes/Item.lua:2148-2164 h=35fea79e
impl Item {
    pub fn GetModLineVariantCount(&self, modLine: &Table) -> f64 {
        // L2149
        if !self.ex(n!("allowDuplicateVariants")).truthy() || !modLine.truthy(n!("variantList")) {
            // L2150
            return if self.CheckModLineVariant(modLine) {
                1.0
            } else {
                0.0
            };
        }

        // Mageblood can intentionally select the same variant more than once.
        // L2154
        let variantList = modLine.get(n!("variantList"));
        // L2155
        let mut count = if vget(&variantList, &onum(self.variant.get())).truthy() {
            1.0
        } else {
            0.0
        };
        let alts = [
            (n!("hasAltVariant"), &self.variantAlt),
            (n!("hasAltVariant2"), &self.variantAlt2),
            (n!("hasAltVariant3"), &self.variantAlt3),
            (n!("hasAltVariant4"), &self.variantAlt4),
            (n!("hasAltVariant5"), &self.variantAlt5),
        ];
        // L2156: suffix = i == 1 and "" or i
        for (has, variantAlt) in alts {
            // L2158
            let variant = variantAlt.get();
            // L2159
            if self.ex(has).truthy()
                && variant.is_some()
                && vget(&variantList, &onum(variant)).truthy()
            {
                count += 1.0;
            }
        }
        // L2163
        count
    }
}

/// @pob Classes/Item.lua:2166-2182 h=45e1b16b
impl Item {
    // Return the name of the slot this item is equipped in
    pub fn GetPrimarySlot(&self) -> Name {
        let Some(base) = self.base.get() else {
            lua_error!("attempt to index field 'base' (a nil value)")
        };
        let r#type = self.r#type.get();
        // L2167
        if base.weapon.is_some() {
            n!("Weapon 1")
        } else if r#type == n!("Quiver") || r#type == n!("Shield") {
            n!("Weapon 2")
        } else if r#type == n!("Ring") {
            n!("Ring 1")
        } else if r#type == n!("Flask") {
            n!("Flask 1")
        } else if r#type == n!("Tincture") {
            n!("Flask 1")
        } else if r#type == n!("Graft") {
            n!("Graft 1")
        } else {
            r#type
        }
    }
}

// ---- Modules/ItemSlotHelper.lua -------------------------------------------------------------

// @pob Modules/ItemSlotHelper.lua:1-1 h=5ce3829c
// (module-level statements: M)

// @pob-skip Modules/ItemSlotHelper.lua:10-38 reason="M.DrawViewer: UI drawing (SetDrawLayer/DrawImage/SetViewport and the tree viewer); the UI port owns it (CCR-0032 §1)"

// @pob Modules/ItemSlotHelper.lua:40-40 h=85c34a63
// (module-level statements: Return)

// ---- Modules/Utils.lua ----------------------------------------------------------------------

// @pob Modules/Utils.lua:1-5 h=8afe2e80
// (module-level statements: t_insert,M,indentCache)
thread_local! {
    // L5
    static indentCache: RefCell<ahash::AHashMap<i64, String>> = RefCell::new(ahash::AHashMap::default());
}

/// @pob Modules/Utils.lua:6-14 h=233c3a4e
pub fn indentFor(k: &Val) -> String {
    let kn = match k {
        Val::Num(x) => *x,
        other => lua_error!("attempt to compare {} with number", other.lua_type()),
    };
    // L7
    if kn < 1.0 {
        return String::new();
    }
    // L10: indentCache[k] (a memo; k is a whole number of tabs in every call)
    let key = kn as i64;
    if !indentCache.with(|c| c.borrow().contains_key(&key)) {
        // L11: string.rep("\t", k)
        let s = pob_lua::string::rep("\t", kn as usize);
        indentCache.with(|c| c.borrow_mut().insert(key, s));
    }
    // L13
    indentCache.with(|c| c.borrow().get(&key).cloned().unwrap_or_default())
}

/// Lua `tostring(v)` for the values `strSort` sees (table keys).
fn tostring_val(v: &Val) -> String {
    match v {
        Val::Nil => "nil".to_owned(),
        Val::Bool(b) => b.to_string(),
        Val::Num(n) => tostring_num(*n),
        Val::Str(s) => s.to_string(),
        Val::Table(t) => format!("table: {:p}", t.as_ptr()),
        Val::Data(_) | Val::Mod(_) => format!("table: {:p}", v as *const Val),
        Val::Obj(o) => format!("userdata: {:p}", Rc::as_ptr(o)),
    }
}

/// @pob Modules/Utils.lua:15-17 h=0b041ef6
pub fn strSort(a: &Val, b: &Val) -> bool {
    // L16
    tostring_val(a).as_bytes() < tostring_val(b).as_bytes()
}

/// `pairs(value)` keys and `value[k]` over a calc table or a data table.
fn table_entries(value: &Val) -> (Vec<Val>, Vec<(Val, Val)>) {
    match value {
        Val::Table(t) => {
            let arr = t.ipairs().into_iter().map(|(_, v)| v).collect();
            let all = t.pairs().map(|(k, v)| (k.to_val(), v)).collect();
            (arr, all)
        }
        Val::Data(Value::Table(t)) => {
            let arr: Vec<Val> = t
                .array
                .iter()
                .map(Val::from_value)
                .take_while(|v| !v.is_nil())
                .collect();
            let mut all: Vec<(Val, Val)> = t
                .array
                .iter()
                .enumerate()
                .filter(|(_, v)| !matches!(v, Value::Nil))
                .map(|(i, v)| (Val::Num((i + 1) as f64), Val::from_value(v)))
                .collect();
            all.extend(
                t.fields
                    .iter()
                    .map(|(k, v)| (Val::from(*k), Val::from_value(v))),
            );
            (arr, all)
        }
        _ => (Vec::new(), Vec::new()),
    }
}

/// @pob Modules/Utils.lua:19-62 h=6bc36f7a
/// `tabs` is Lua's `tabs or 1` (L21-23): callers pass 1 where Lua passes nil.
pub fn writeStringify(
    buf: &mut String,
    value: &Val,
    allowNewlines: bool,
    tabs: f64,
    singleLine: bool,
) {
    // L20
    let valType = value.lua_type();
    // L21: if not tabs then tabs = 1 (see the doc comment)
    // L24
    if valType == "string" {
        let value = cs(value);
        // L25
        let str = if allowNewlines {
            value
        } else {
            let (s, _) = pattern::gsub(&value, "\r\n", " ", None);
            pattern::gsub(&s, "[\r\n]", " ", None).0
        };
        // L26
        buf.push_str(&lfmt!("%q", str.as_str()));
    } else if valType == "boolean" || valType == "nil" || valType == "number" {
        // L28
        buf.push_str(&tostring_val(value));
    } else if valType == "table" && !matches!(value, Val::Mod(_)) {
        // L30
        let newLine = if singleLine { " " } else { "\n" };
        // L31
        buf.push('{');
        buf.push_str(newLine);
        // ipairs compatible keys are done first so we can use the array syntax for them
        // L33
        let (arr, all) = table_entries(value);
        let mut arrayKeys: Vec<f64> = Vec::new();
        // L34
        let indent = if singleLine {
            String::new()
        } else {
            indentFor(&Val::Num(tabs))
        };
        // L35
        for (k, v) in arr.iter().enumerate() {
            // L36
            arrayKeys.push((k + 1) as f64);
            // L37
            buf.push_str(&indent);
            // L38
            writeStringify(buf, v, allowNewlines, tabs + 1.0, singleLine);
            // L39
            buf.push(',');
            buf.push_str(newLine);
        }

        // L42
        let mut mapKeys: Vec<Val> = Vec::new();
        // L43 LUA-ORDER: the keys are sorted below; only keys with equal tostring (1 and "1") keep the pairs order
        for (key, _) in &all {
            // avoid printing array-style items twice
            // L45
            let isArray = matches!(key, Val::Num(x) if arrayKeys.contains(x));
            if !isArray {
                mapKeys.push(key.clone());
            }
        }
        // L49
        pob_lua::sort::sort(&mut mapKeys, strSort);

        // L51
        for k in &mapKeys {
            // L52
            buf.push_str(&indent);
            buf.push('[');
            // L53
            writeStringify(buf, k, allowNewlines, 1.0, singleLine);
            // L54
            buf.push_str("] = ");
            // L55
            let v = all
                .iter()
                .find(|(kk, _)| kk.raw_eq(k))
                .map(|(_, v)| v.clone())
                .unwrap_or(Val::Nil);
            writeStringify(buf, &v, allowNewlines, tabs + 1.0, singleLine);
            // L56
            buf.push(',');
            buf.push_str(newLine);
        }
        // L58
        if !singleLine {
            buf.push_str(&indentFor(&Val::Num(tabs - 1.0)));
        }
        buf.push('}');
    } else {
        // L60 (a mod table is not stringified: CCR-0032 §3)
        lua_error!("Disallowed stringify type {valType}")
    }
}

/// @pob Modules/Utils.lua:69-73 h=8c4f2ac1
// Converts a table to a string which will be valid Lua. The result will ipairs to utilise the array
// syntax when applicable, and will sort other keys.
pub fn stringify(value: &Val, allowNewlines: bool, tabs: Option<f64>) -> String {
    // L70
    let mut buf = String::new();
    // L71
    writeStringify(&mut buf, value, allowNewlines, tabs.unwrap_or(1.0), false);
    // L72
    buf
}

/// @pob Modules/Utils.lua:79-83 h=37e4525d
// Converts a table to a string which will be valid Lua. The result will ipairs to utilise the array
// syntax when applicable, and will sort other keys. This is similar to stringify(), but does not produce a multi line string.
pub fn stringifyInline(value: &Val) -> String {
    // L80
    let mut buf = String::new();
    // L81
    writeStringify(&mut buf, value, false, 1.0, true);
    // L82
    buf
}

/// The file contents `saveTableToFile` writes (Utils.lua:95-114).
fn saveTableContents(table: &Val, description: Option<&str>, allowMultiLine: bool) -> String {
    // L95
    let mut descFormatted = String::new();
    // L96
    if let Some(description) = description {
        let mut description = description.to_owned();
        // add newline at the end
        // L98
        if pob_lua::string::sub(&description, -1, None) != "\n" {
            description.push('\n');
        }
        // prepend lines with comments
        // L102
        for line in pattern::gmatch(&description, "(.-)\n") {
            descFormatted.push_str("-- ");
            descFormatted.push_str(&line[0].to_str());
            descFormatted.push('\n');
        }
    }
    // L106
    let template = "-- This file is automatically generated, do not edit!\n-- Game data (c) Grinding Gear Games\n\n%s\n-- spell-checker: disable\nreturn %s\n";

    // L114
    lfmt!(
        template,
        descFormatted.as_str(),
        stringify(table, allowMultiLine, Some(1.0)).as_str()
    )
}

/// The C `strerror` text LuaJIT's `io.open` / `f:write` report: Rust's `io::Error` display of
/// an OS error is that text plus ` (os error N)`, which Lua does not have.
#[cfg(not(target_arch = "wasm32"))]
fn lua_io_strerror(e: &std::io::Error) -> String {
    let s = e.to_string();
    match e.raw_os_error() {
        Some(code) => s
            .strip_suffix(&format!(" (os error {code})"))
            .map_or_else(|| s.clone(), str::to_owned),
        None => s,
    }
}

/// @pob Modules/Utils.lua:90-123 h=7297461d
pub fn saveTableToFile(
    fileName: &str,
    table: &Val,
    description: Option<&str>,
    allowMultiLine: bool,
) -> bool {
    #[cfg(not(target_arch = "wasm32"))]
    {
        use std::io::Write;
        // L91
        let f = std::fs::File::create(fileName);
        // L92
        let mut f = match f {
            Ok(f) => f,
            Err(openErr) => lua_error!(
                "{}",
                lfmt!(
                    "Could not open %s for writing table: %s",
                    fileName,
                    format!("{fileName}: {}", lua_io_strerror(&openErr)).as_str()
                )
            ),
        };
        let contents = saveTableContents(table, description, allowMultiLine);
        // L116
        let writeErr = f.write_all(contents.as_bytes()).err();
        // L118
        if let Some(writeErr) = writeErr {
            lua_error!(
                "{}",
                lfmt!(
                    "Failed to write to %s: %s",
                    fileName,
                    lua_io_strerror(&writeErr).as_str()
                )
            );
        }
        // L122
        f.flush().is_ok()
    }
    #[cfg(target_arch = "wasm32")]
    {
        // no `io` library in the browser build
        let _ = saveTableContents(table, description, allowMultiLine);
        lua_error!("Could not open {fileName} for writing table: no file system")
    }
}

/// `saveTableToFile`'s generated text (for tests and callers that do not write a file).
pub fn saveTableToString(table: &Val, description: Option<&str>, allowMultiLine: bool) -> String {
    saveTableContents(table, description, allowMultiLine)
}

// @pob Modules/Utils.lua:125-125 h=85c34a63
// (module-level statements: Return)
