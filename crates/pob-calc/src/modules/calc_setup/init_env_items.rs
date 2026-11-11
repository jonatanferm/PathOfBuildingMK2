//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! initEnv segment CS2 (CalcSetup.lua 800-1450): items, jewels, nodes; restarts at 1433 (extraJewelFuncs).
//!
//! Owner: CS2.
//!
//! Port notes (representation; each site is marked):
//! * The segment's own locals (`items`, `jewelLimits`, `itemDisabled`, `itemDisablers`,
//!   `visited`, `trueDisabled`, `matchedName`) are string-keyed Lua tables used only through
//!   `t[k]` reads and writes, plus `pairs(itemDisablers)` / `pairs(trueDisabled)`, whose PoB
//!   order is random per process (string hashing, D8): insertion-ordered `IndexMap`s, the
//!   `pairs` marked `LUA-ORDER`.
//! * `env.radiusJewelList` entries are the dynamic tables CS0's `node_mods.rs` documents
//!   (`nodes`, `func`, `type`, `item`, `nodeId`, `attributes`, `data`). A `JewelFunc` mod value
//!   is the table `{ type = …, func = Value::JewelFunc }`; an `ExtraJewelFunc` mod value is the
//!   `Value::JewelFunc` itself (it carries `type` and `radius`, ModParser.lua 2622/2630 as MP
//!   emits them), so `funcData.func` is that value ([`jewel_func`]).
//! * The CalcSetup.lua:877-884 default func is `JewelFuncSpec { key: "$default" }` (CCR-0039;
//!   its body is MP7's `jewel_funcs::default_func`).
//! * `override.removeNodes[node.id]` (1393, 1408): PoB's set mixes node keys (power calcs) and
//!   node-id keys (anointed nodes, PassiveTreeView.lua:1706); the frozen id → node map is the
//!   node-keyed set (CS1, CCR-0082 §2), so id-keyed entries are read from
//!   `override.extra["$removeNodeIds"]` (CCR-0105).
#![allow(non_snake_case)]
// Lua statement shapes are kept (locals assigned per branch, `if … then … else … end`).
#![allow(
    clippy::needless_late_init,
    clippy::collapsible_if,
    clippy::collapsible_else_if,
    clippy::too_many_lines,
    clippy::cognitive_complexity,
    clippy::float_cmp,
    clippy::explicit_auto_deref,
    clippy::len_zero,
    clippy::no_effect_underscore_binding,
    clippy::assign_op_pattern,
    clippy::redundant_else,
    clippy::neg_cmp_op_on_partial_ord,
    clippy::blocks_in_conditions
)]

use std::any::Any;

use indexmap::IndexMap;
use pob_gamedata::mod_tools as modLib;
use pob_gamedata::{JewelFuncKind, JewelFuncSpec};
use pob_lua::pattern;

use super::InitEnvCtx;
use crate::classes::item::new_Item;
use crate::model::items_tab::id_key;
use crate::modules::calc_tools as calcLib;
use crate::modules::common::mergeDB;
use crate::prelude::*;

// ---- Rust-only plumbing ---------------------------------------------------------------------

/// `s:match(p)` is truthy.
fn lmatch(s: &str, p: &str) -> bool {
    pattern::r#match(s, p, None).is_some()
}

/// The first value of `s:gsub(p, repl)`.
fn gsub(s: &str, p: &str, repl: &str) -> String {
    pattern::gsub(s, p, repl, None).0
}

/// A calc object as a Lua value.
fn obj<T: 'static>(rc: &Rc<T>) -> Val {
    Val::Obj(rc.clone() as Rc<dyn Any>)
}

/// `item.base` (indexing a nil base raises, as in Lua).
fn item_base(item: &Item) -> ItemBaseRef {
    match item.base.get() {
        Some(b) => b,
        None => lua_error!("attempt to index field 'base' (a nil value)"),
    }
}

/// `item.base.type`.
fn base_type(item: &Item) -> Name {
    item_base(item).r#type
}

/// `item.base.subType` (nil → None).
fn base_subType(item: &Item) -> Option<Name> {
    item_base(item).subType
}

/// `item.baseName` as a string operand (a nil one raises on the method call).
fn item_baseName(item: &Item) -> Name {
    match item.baseName.get() {
        Some(b) => b,
        None => lua_error!("attempt to index field 'baseName' (a nil value)"),
    }
}

/// `item.jewelData` (a table or nil).
fn jewelData(item: &Item) -> Option<Table> {
    item.jewelData.get()
}

/// `item.flaskData` indexed (a nil one raises).
fn flaskData(item: &Item) -> Table {
    match item.flaskData.get() {
        Some(t) => t,
        None => lua_error!("attempt to index field 'flaskData' (a nil value)"),
    }
}

/// `item.slotModList[slotNum]`.
fn slotModList_at(item: &Item, slotNum: &Val) -> Option<ModList> {
    match slotNum {
        Val::Num(x) if *x == 1.0 => item.slotModList[0].get(),
        Val::Num(x) if *x == 2.0 => item.slotModList[1].get(),
        Val::Num(x) if *x == 3.0 => item.slotModList[2].get(),
        _ => None,
    }
}

/// `ipairs(list)` on a mod list that may be nil (raises like Lua).
fn ipairs_list(list: Option<ModList>) -> Vec<ModRef> {
    match list {
        Some(l) => l.ipairs(),
        None => lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)"),
    }
}

/// `item[property]` for the influence / corruption keys of the 1272 and 1364 loops:
/// `corrupted` is typed, the influences live in `item.influences` (contracts#class-census).
fn item_property(item: &Item, property: &str) -> bool {
    if property == "corrupted" {
        item.corrupted.get()
    } else {
        item.influences.truthy(property)
    }
}

/// `item.shaper or item.elder`.
fn shaper_or_elder(item: &Item) -> bool {
    item.influences.truthy(n!("shaper")) || item.influences.truthy(n!("elder"))
}

/// `item.foulborn` (untyped, `Item.extra`).
fn item_foulborn(item: &Item) -> bool {
    item.extra.truthy(n!("foulborn"))
}

/// `{ ["CorruptedItem"] = "corrupted", … }` (CalcSetup.lua:1272 and 1364), in source order.
/// LUA-ORDER: PoB's `pairs` over this string-keyed constructor is in hash order; every step
/// adds to its own multiplier keys, so only the keys' insertion order in
/// `env.itemModDB.multipliers` can differ.
const ITEM_PROPERTIES: [(&str, &str); 7] = [
    ("CorruptedItem", "corrupted"),
    ("ShaperItem", "shaper"),
    ("ElderItem", "elder"),
    ("WarlordItem", "adjudicator"),
    ("HunterItem", "basilisk"),
    ("CrusaderItem", "crusader"),
    ("RedeemerItem", "eyrie"),
];

/// `t[k] = (t[k] or 0) + d` on a multipliers table.
fn add_mult(t: &Table, k: &str, d: f64) {
    t.add(Name::from(k), d);
}

/// Probe builds: `<obj>.<key>` is set in Lua now. The exporter leaves out a typed field that was
/// nil in the injected dump while it holds a default (A8 needs-code item 5; CCR-0079 §5), and
/// `env.flaskSlotMap` holds only object keys (registry entries), which the default check does not
/// see.
fn field_present<T>(obj: &T, key: &str) {
    #[cfg(feature = "probe")]
    crate::probe::unmark_absent(std::ptr::from_ref(obj).cast::<()>() as usize, key);
    #[cfg(not(feature = "probe"))]
    let _ = (obj, key);
}

/// `override.removeNodes[node.id]` (1393, 1408; CCR-0105): the node-id keys of PoB's mixed set
/// (PassiveTreeView.lua:1706 `{ [node.id] = true }`), kept in `override.extra["$removeNodeIds"]`
/// (a table of id → true). The frozen `removeNodes` map is the node-keyed half (CCR-0082 §2),
/// which a number key never hits in PoB. So `not override.removeNodes or not
/// override.removeNodes[node.id]` is `!removeNodes_byId(..)` alone: with neither half set it is
/// Lua's `not override.removeNodes`, and the typed map's presence must not short-circuit it (an
/// id-only caller leaves `removeNodes` as None).
fn removeNodes_byId(r#override: &Override, id: f64) -> bool {
    match r#override.extra.table(Name::from("$removeNodeIds")) {
        Some(t) => t.truthy(id),
        None => false,
    }
}

/// `env.allocNodes[k]`-style int key of a node id (a non-integral id is never a key here).
fn int_key(id: f64) -> Option<i64> {
    (id.is_finite() && id == id.trunc()).then_some(id as i64)
}

/// A node-id map key from a number id (Lua would store any other key in the hash part; the
/// node-id maps of the port hold integral ids only).
fn node_key(id: f64, what: &str) -> i64 {
    match int_key(id) {
        Some(k) => k,
        None => lua_error!(
            "{what}: node id {} is not an integer key (calc doc §3.4)",
            tostring_num(id)
        ),
    }
}

/// `func.func` of a jewel func entry: a `JewelFunc` value table's `func`, or an
/// `ExtraJewelFunc` value, which is the `Value::JewelFunc` itself (module docs).
fn jewel_func(funcData: &Val) -> Val {
    match funcData {
        Val::Data(Value::JewelFunc(_)) => funcData.clone(),
        other => other.get(n!("func")),
    }
}

/// `{ { type = "Self", func = function(node, out, data) … end } }` (CalcSetup.lua:877-884).
fn default_funcList() -> Table {
    let entry = Table::new();
    entry.set(n!("type"), "Self");
    entry.set(
        n!("func"),
        Val::Data(Value::JewelFunc(Arc::new(JewelFuncSpec {
            key: n!("$default"),
            kind: JewelFuncKind::Self_,
            args: smallvec::SmallVec::new(),
            r#type: Some(n!("Self")),
            radius: None,
        }))),
    );
    let t = Table::new();
    t.push(entry);
    t
}

/// `node.nodesInRadius and node.nodesInRadius[index] or { }` (and `attributesInRadius`).
fn in_radius(t: Option<Table>, index: &Val) -> Val {
    let v = match (t, index) {
        (Some(t), Val::Num(x)) => t.get(*x),
        _ => Val::Nil,
    };
    if v.truthy() {
        v
    } else {
        Val::Table(Table::new())
    }
}

/// `for nodeId, node in pairs(node.nodesInRadius[index]) do if not env.allocNodes[nodeId] then
/// env.extraRadiusNodeList[nodeId] = env.spec.nodes[nodeId] end end` (CalcSetup.lua 899-904 and
/// 927-932).
fn add_extra_radius_nodes(env: &Env, nodesInRadius: Option<Table>, index: &Val) {
    let set = match (nodesInRadius, index) {
        (Some(t), Val::Num(x)) => t.get(*x),
        (Some(_), _) => Val::Nil,
        (None, _) => lua_error!("attempt to index field 'nodesInRadius' (a nil value)"),
    };
    let ids: Vec<i64> = match &set {
        Val::Obj(o) => match o.downcast_ref::<LjIntMap<NodeRef>>() {
            Some(m) => m.keys(),
            None => lua_error!("bad argument #1 to 'pairs' (table expected, got userdata)"),
        },
        Val::Table(t) => t
            .pairs()
            .filter_map(|(k, _)| match k {
                Key::I(i) => Some(i),
                _ => None,
            })
            .collect(),
        other => lua_error!(
            "bad argument #1 to 'pairs' (table expected, got {})",
            other.lua_type()
        ),
    };
    // LUA-ORDER: a node-id map (LjIntMap, LuaJIT `next` order)
    for nodeId in ids {
        if !env.allocNodes.get().with(|m| m.contains_key(nodeId)) {
            let specNode = env.spec.get().nodes.with(|m| m.get(nodeId));
            env.extraRadiusNodeList
                .get()
                .with_mut(|m| m.set(nodeId, specNode));
        }
    }
}

/// `{ nodes = …, func = …, type = …, item = item, nodeId = …, attributes = …, data = { } }`.
fn radius_entry(nodes: Val, func: Val, r#type: Val, item: &ItemRef, nodeId: f64, attributes: Val) -> Table {
    let t = Table::new();
    t.set(n!("nodes"), nodes);
    t.set(n!("func"), func);
    t.set(n!("type"), r#type);
    t.set(n!("item"), obj(item));
    t.set(n!("nodeId"), nodeId);
    t.set(n!("attributes"), attributes);
    t.set(n!("data"), Table::new());
    t
}

/// `item.rarity:gsub("(%a)(%u*)", function(a, b) return a..string.lower(b) end)`.
fn rarity_title(rarity: &str) -> String {
    pattern::gsub_fn(
        rarity,
        "(%a)(%u*)",
        |c| {
            Some(format!(
                "{}{}",
                c[0].as_str(),
                pob_lua::string::lower(c[1].as_str())
            ))
        },
        None,
    )
    .0
}

/// `copyTable(skill)` of a granted-skill table (deep).
fn copy_skill(skill: &Table) -> Table {
    skill.copyTable(false)
}

/// `for _, tag in ipairs(mod) do if tag.type == "SocketedIn" then … end end`: the filter loops
/// of 1105-1111, 1158-1164, 1176-1182, 1194-1198.
fn has_socketedIn(r#mod: &Mod) -> bool {
    r#mod.tags.iter().any(|t| t.r#type == TagType::SocketedIn)
}

/// `new("ModList"):ModList()` with `MergeMod` of every mod of `srcList`.
fn combined(srcList: &[ModRef]) -> ModList {
    let combinedList = ModStore::ModList(None);
    for r#mod in srcList {
        combinedList.MergeMod(r#mod.clone(), false);
    }
    combinedList
}


/// `slot.slotName` (a string).
fn slot_name(slot: &Table) -> Name {
    match slot.get(n!("slotName")) {
        Val::Str(s) => Name::from(&*s),
        other => lua_error!("slot.slotName is a {} value", other.lua_type()),
    }
}

/// `build.itemsTab.activeItemSet.useSecondWeaponSet`.
fn useSecondWeaponSet(build: &Build) -> bool {
    match build.itemsTab.activeItemSet.get() {
        Some(s) => s.truthy(n!("useSecondWeaponSet")),
        None => lua_error!("attempt to index field 'activeItemSet' (a nil value)"),
    }
}

/// `env.initialNodeModDB` indexed (nil raises).
fn initialNodeModDB(env: &Env) -> ModDB {
    match env.initialNodeModDB.get() {
        Some(db) => db,
        None => lua_error!("attempt to index field 'initialNodeModDB' (a nil value)"),
    }
}

/// `env.spec.nodes[slot.nodeId]` (nil → None).
fn spec_node(env: &Env, nodeId: f64) -> Option<NodeRef> {
    int_key(nodeId).and_then(|k| env.spec.get().nodes.with(|m| m.get(k)))
}

/// `t[k] = v` on a string-keyed local table: a nil key raises, a nil value removes the key.
fn set_kv(t: &mut IndexMap<LStr, Val>, k: &Val, v: Val) {
    let k = match k {
        Val::Str(s) => s.clone(),
        Val::Nil => lua_error!("table index is nil"),
        other => lua_error!("item disabler key is a {} value", other.lua_type()),
    };
    if v.is_nil() {
        t.shift_remove(&k);
    } else {
        t.insert(k, v);
    }
}

/// `t[k]` on a string-keyed local table.
fn get_kv(t: &IndexMap<LStr, Val>, k: &Val) -> Val {
    match k {
        Val::Str(s) => t.get(s).cloned().unwrap_or(Val::Nil),
        _ => Val::Nil,
    }
}

/// `modLib.setSource(copyTable(mod), nil)` (ModTools.lua:239-245 with a nil `source`, CalcSetup.lua
/// 1200-1201 when `item.modSource` is nil): clears `mod.source` and, for a `{ mod = … }` value,
/// the inner mod's `source` too (`mod_tools::setSource` takes a string).
fn setSource_nil(m: &Mod) -> Mod {
    let mut r#mod = m.clone();
    r#mod.source = None;
    if let Some(inner) = r#mod.value.inner_mod() {
        let mut inner = (**inner).clone();
        inner.source = None;
        if let Value::Table(t) = &r#mod.value {
            r#mod.value = Value::Table(Arc::new(t.with(n!("mod"), Value::Mod(Arc::new(inner)))));
        }
    }
    r#mod
}

/// `override.extra` key that keeps the env player alive which `override.extraJewelFuncs.actor`
/// points at (CalcSetup.lua:1428). Lua's table reference keeps the first pass's `env.player`
/// alive across the restart; `ModStore.actor` is `Weak` (§3.3 rule 4), so the override holds
/// the strong reference (Rust-only plumbing). EvalMod's `ItemCondition` needs a live actor
/// (ModStore.lua:697) for the `ExtraJewelFunc` mods' tags.
const EXTRA_JEWEL_FUNCS_ACTOR: &str = "$extraJewelFuncsActor";

/// @pob Modules/CalcSetup.lua:800-1450 h=d03eb08e
/// CS2: CalcSetup.lua 800-1450 (`Flow::Restart` at 1433).
#[rustfmt::skip]
pub(crate) fn seg(ctx: &mut InitEnvCtx) -> Flow {
    let build = ctx.build.clone();
    let accelerate = ctx.accelerate;
    let r#override = ctx.r#override.clone();
    let env: EnvRef = match ctx.env.clone() {
        Some(e) => e,
        None => lua_error!("attempt to index local 'env' (a nil value)"),
    };
    let modDB: ModDB = match ctx.modDB.clone() {
        Some(m) => m,
        None => lua_error!("attempt to index local 'modDB' (a nil value)"),
    };

    // Build and merge item modifiers, and create list of radius jewels
    // L800
    if !accelerate.requirementsItems {
        // L801
        let mut items: IndexMap<Name, ItemRef> = IndexMap::new();
        // L802
        let mut jewelLimits: IndexMap<LStr, f64> = IndexMap::new();
        let orderedSlots = build.itemsTab.orderedSlots.snapshot();
        // L803
        for slot in &orderedSlots {
            // L804
            let mut slotName = slot_name(slot);
            // L805
            if slotName == n!("Graft 1") || slotName == n!("Graft 2") {
                // L806
                if pattern::find(build.spec.get().treeVersion.get().as_str(), "3_27", None, false).is_none() {
                    // L807
                    continue;
                }
            }
            // ignore item in Ring 3 if The Unseen Hand is not allocated
            // L811
            if slotName == n!("Ring 3") && !initialNodeModDB(&env).Flag(None, nm!["AdditionalRingSlot"]) {
                // L812
                continue;
            }
            // L814
            let item: Option<ItemRef>;
            let repSlotName = r#override.repSlotName.get();
            let repItem = r#override.repItem.get();
            // L815
            if repSlotName == Some(slotName) {
                // L816
                item = repItem.clone();
            } else if let Some(rep) = repItem.as_ref().filter(|_| {
                (match repSlotName {
                    Some(s) => lmatch(s.as_str(), "^Weapon 1"),
                    None => lua_error!("attempt to index field 'repSlotName' (a nil value)"),
                }) && lmatch(slotName.as_str(), "^Weapon 2")
            }).filter(|rep| {
                let t = base_type(rep);
                // LUA-QUIRK: `item` is the local declared at 814, still nil here, so the Bow
                // clause `(… == "Bow" and item and item.base.type ~= "Quiver")` is always nil
                t == n!("Staff") || t == n!("Two Handed Sword") || t == n!("Two Handed Axe") || t == n!("Two Handed Mace")
            }) {
                let _ = rep;
                // L820
                continue;
            } else if slot.truthy(n!("nodeId")) && r#override.spec.get().is_some() {
                // L822
                let nodeId = slot.get(n!("nodeId")).num();
                let id = int_key(nodeId).and_then(|k| env.spec.get().jewels.with(|m| m.get(k)));
                item = id.and_then(id_key).and_then(|k| build.itemsTab.items.with(|m| m.get(&k).cloned()));
            } else {
                // L824
                item = match slot.get(n!("selItemId")) {
                    Val::Num(x) => id_key(x).and_then(|k| build.itemsTab.items.with(|m| m.get(&k).cloned())),
                    _ => None,
                };
            }
            // L826 (`item.grantedSkills` is always a table)
            if let Some(it) = &item {
                // Find skills granted by this item
                // L828
                for_ipairs!((_i, skill) in it.grantedSkills.get() => {
                    // L829
                    let skillData = match skill.get(n!("skillId")) {
                        Val::Str(s) => env.data.skills.get(&Name::from(&*s)).cloned(),
                        _ => None,
                    };
                    // L830
                    let grantedSkill = copy_skill(&skill);
                    // L831
                    grantedSkill.set(n!("nameSpec"), skillData.as_ref().map_or(Val::Nil, |d| Val::from(d.name)));
                    // L832
                    grantedSkill.set(n!("sourceItem"), obj(it));
                    // L833
                    grantedSkill.set(n!("slotName"), slotName);
                    // L834
                    env.grantedSkillsItems.get().push(grantedSkill);
                });
            }
            // L837
            if let Some(it) = item.as_ref().filter(|i| i.baseModList.get().is_some_and(|l| l.Flag(None, nm!["CanExplode"]))) {
                // L838
                env.explodeSources.get().push(ExplodeSource::Item(it.clone()));
            }
            let weaponSet = slot.get(n!("weaponSet"));
            // L840
            if weaponSet.truthy() && !weaponSet.raw_eq(&Val::Num(if useSecondWeaponSet(&build) { 2.0 } else { 1.0 })) {
                // L841
                continue;
            }
            // L843
            if weaponSet.raw_eq(&Val::Num(2.0)) && useSecondWeaponSet(&build) {
                // L844
                slotName = Name::from(gsub(slotName.as_str(), " Swap", "").as_str());
            }
            // L846
            if slot.truthy(n!("nodeId")) {
                let nodeId = slot.get(n!("nodeId")).num();
                // Slot is a jewel socket, check if socket is allocated
                // L848
                if !int_key(nodeId).is_some_and(|k| env.allocNodes.get().with(|m| m.contains_key(k))) {
                    // L849
                    continue;
                } else if let Some(it) = &item {
                    // L851
                    if let Some(jd) = jewelData(it) {
                        // L852
                        jd.set(n!("limitDisabled"), Val::Nil);
                    }
                    // L854
                    if it.r#type.get() == n!("Jewel") && lmatch(&it.name.get(), "The Adorned, Crimson Jewel") {
                        let Some(jd) = jewelData(it) else {
                            lua_error!("attempt to index field 'jewelData' (a nil value)")
                        };
                        // L855
                        if jd.truthy(n!("corruptedMagicJewelIncEffect")) {
                            // L856
                            let v = jd.get(n!("corruptedMagicJewelIncEffect")).num() / 100.0;
                            env.modDB.get().multipliers.set(n!("CorruptedMagicJewelEffect"), v);
                        }
                        // L858
                        if jd.truthy(n!("corruptedRareJewelIncEffect")) {
                            // L859
                            let v = jd.get(n!("corruptedRareJewelIncEffect")).num() / 100.0;
                            env.modDB.get().multipliers.set(n!("CorruptedRareJewelEffect"), v);
                        }
                    }
                    // L862
                    if let Some(limit) = it.limit.get().filter(|_| !env.configInput.truthy(n!("ignoreJewelLimits"))) {
                        // L863
                        let limitKey: Option<LStr> = if base_subType(it) == Some(n!("Timeless")) {
                            Some(LStr::from("Historic"))
                        } else {
                            it.title.get()
                        };
                        // L864
                        let cur = limitKey.as_ref().and_then(|k| jewelLimits.get(k).copied());
                        if cur.is_some_and(|c| c >= limit) {
                            // L865
                            if let Some(jd) = jewelData(it) {
                                // L866
                                jd.set(n!("limitDisabled"), true);
                            }
                            // L868
                            let w = env.itemWarnings.get().get_or_new_table(n!("jewelLimitWarning"));
                            // L869
                            w.push(limitKey.map_or(Val::Nil, Val::Str));
                            // L870
                            continue;
                        } else {
                            // L872
                            match limitKey {
                                Some(k) => {
                                    let v = cur.unwrap_or(0.0) + 1.0;
                                    jewelLimits.insert(k, v);
                                }
                                None => lua_error!("table index is nil"),
                            }
                        }
                    }
                    // L875
                    let extraJewelFuncs = r#override.extraJewelFuncs.get();
                    if it.jewelRadiusIndex.get().is_some() || extraJewelFuncs.as_ref().is_some_and(|l| l.len() > 0) {
                        let radiusIndex = it.jewelRadiusIndex.get().map_or(Val::Nil, Val::Num);
                        // Jewel has a radius, add it to the list
                        // L877
                        let funcList = match jewelData(it).map(|jd| jd.get(n!("funcList"))) {
                            Some(Val::Table(t)) => t,
                            Some(v) if v.truthy() => lua_error!("bad argument #1 to 'ipairs' (table expected, got {})", v.lua_type()),
                            _ => default_funcList(),
                        };
                        // L885
                        for_ipairs!((_i, func) in funcList => {
                            // L886
                            let Some(node) = spec_node(&env, nodeId) else {
                                lua_error!("attempt to index local 'node' (a nil value)")
                            };
                            // L887
                            env.radiusJewelList.get().push(radius_entry(
                                in_radius(node.nodesInRadius.get(), &radiusIndex),
                                jewel_func(&func),
                                func.get(n!("type")),
                                it,
                                nodeId,
                                in_radius(node.attributesInRadius.get(), &radiusIndex),
                            ));
                            // L896
                            if !matches!(func.get(n!("type")), Val::Str(s) if &*s == "Self") && node.nodesInRadius.get().is_some() {
                                // Add nearby unallocated nodes to the extra node list
                                // L898-L902
                                add_extra_radius_nodes(&env, node.nodesInRadius.get(), &radiusIndex);
                            }
                        });
                        // L905
                        let funcDatas: Vec<Val> = match &extraJewelFuncs {
                            Some(l) => {
                                let cfg = ModCfg::default();
                                cfg.item.set(Some(it.clone()));
                                l.List(Some(&cfg), nm!["ExtraJewelFunc"]).iter().map(Val::from_value).collect()
                            }
                            None => Vec::new(),
                        };
                        for funcData in &funcDatas {
                            // L906
                            let Some(node) = spec_node(&env, nodeId) else {
                                lua_error!("attempt to index local 'node' (a nil value)")
                            };
                            // L907
                            let mut radius: Val = Val::Nil;
                            // L908 (`data.jewelRadius` is a list: index order)
                            let radii = pob_gamedata::jewelRadius();
                            for (i0, data) in radii.jewelRadius.iter().enumerate() {
                                // L909
                                if matches!(funcData.get(n!("radius")), Val::Str(s) if *s == *data.label.as_str()) {
                                    // L910
                                    radius = Val::Num((i0 + 1) as f64);
                                    // L911
                                    break;
                                }
                            }
                            // L914
                            env.radiusJewelList.get().push(radius_entry(
                                in_radius(node.nodesInRadius.get(), &radius),
                                jewel_func(funcData),
                                funcData.get(n!("type")),
                                it,
                                nodeId,
                                in_radius(node.attributesInRadius.get(), &radius),
                            ));
                            // L923
                            if !matches!(funcData.get(n!("type")), Val::Str(s) if &*s == "Self") && node.nodesInRadius.get().is_some() {
                                // Add nearby unallocated nodes to the extra node list
                                // L925-L929
                                add_extra_radius_nodes(&env, node.nodesInRadius.get(), &radius);
                            }
                        }
                    }
                }
            }
            // L935
            if let Some(it) = item.as_ref().filter(|i| i.r#type.get() == n!("Flask") && base_subType(i) == Some(n!("Life")) && i.flaskData.get().is_some()) {
                // Keep highest life flask recovery even if this slot is later disabled (e.g. Poisonous Concoction).
                // L937
                let mults = env.itemModDB.get().multipliers.clone();
                let v = m_max(mults.num_or(n!("LifeFlaskRecovery"), 0.0), flaskData(it).num_or(n!("lifeTotal"), 0.0));
                mults.set(n!("LifeFlaskRecovery"), v);
            }
            // L939
            match item {
                Some(it) => {
                    items.insert(slotName, it);
                }
                None => {
                    items.shift_remove(&slotName);
                }
            }
        }

        // L943
        if !env.configInput.truthy(n!("ignoreItemDisablers")) {
            // L944
            let mut itemDisabled: IndexMap<LStr, Val> = IndexMap::new();
            // L945
            let mut itemDisablers: IndexMap<LStr, Val> = IndexMap::new();
            // First check tree nodes for disabled items.  This will break if there's ever an anointable node that disables items
            let treeCfg = ModCfg::default();
            treeCfg.source.set(Some(n!("Tree")));
            // L947
            for row in initialNodeModDB(&env).Tabulate(Some(ModType::FLAG), Some(&treeCfg), nm!["CanNotUseItem"]) {
                // L948
                let r#mod = row.r#mod.clone();
                // checks if it disables another slot
                // L950
                for tag in &r#mod.tags {
                    // L951
                    if tag.r#type == TagType::DisablesItem {
                        // L952
                        if tag.excludeItemType.is_some() && tag.slotName.and_then(|s| items.get(&s)).is_some_and(|i| Some(i.r#type.get()) == tag.excludeItemType) {
                            // L953
                            break;
                        }
                        let tagSlotName = tag.slotName.map_or(Val::Nil, Val::from);
                        let source = r#mod.source.as_ref().map_or(Val::Nil, |s| Val::Str(LStr::from(&**s)));
                        // L955
                        set_kv(&mut itemDisablers, &source, tagSlotName.clone());
                        // L956
                        set_kv(&mut itemDisabled, &tagSlotName, source);
                        // L957
                        break;
                    }
                }
            }
            // L961
            for slot in &orderedSlots {
                // L962
                let slotName = slot_name(slot);
                // L963
                if let Some(it) = items.get(&slotName).cloned() {
                    // L964
                    let srcList = match it.modList.get() {
                        Some(l) => Some(l),
                        None => slotModList_at(&it, &slot.get(n!("slotNum"))),
                    };
                    // L965
                    for r#mod in ipairs_list(srcList) {
                        // checks if it disables another slot
                        // L967
                        for tag in &r#mod.tags {
                            // L968
                            if tag.r#type == TagType::DisablesItem {
                                // e.g. Tincture in Flask 5 while using a Micro-Distillery Belt
                                // L970
                                if tag.excludeItemType.is_some() && tag.slotName.and_then(|s| items.get(&s)).is_some_and(|i| Some(i.r#type.get()) == tag.excludeItemType) {
                                    // L971
                                    break;
                                }
                                let tagSlotName = tag.slotName.map_or(Val::Nil, Val::from);
                                // L973
                                set_kv(&mut itemDisablers, &Val::from(slotName), tagSlotName.clone());
                                // L974
                                set_kv(&mut itemDisabled, &tagSlotName, Val::from(slotName));
                                // L975
                                break;
                            }
                        }
                    }
                }
            }
            // L981
            let mut visited: IndexMap<LStr, Val> = IndexMap::new();
            // L982
            let mut trueDisabled: IndexMap<LStr, Val> = IndexMap::new();
            // L983 LUA-ORDER: string keys (PoB: hash order, D8)
            let disablerKeys: Vec<LStr> = itemDisablers.keys().cloned().collect();
            for k in disablerKeys {
                let mut slot: Val = Val::Str(k);
                // L984
                if !get_kv(&visited, &slot).truthy() {
                    // find chain start
                    // L986 LUA-QUIRK: `{ slot = true }` is the string key "slot", not the variable
                    let mut curChain: IndexMap<LStr, Val> = IndexMap::new();
                    curChain.insert(LStr::from("slot"), Val::Bool(true));
                    // L987
                    while get_kv(&itemDisabled, &slot).truthy() {
                        // L988
                        slot = get_kv(&itemDisabled, &slot);
                        // L989
                        if get_kv(&curChain, &slot).truthy() { break; } // detect cycles
                        // L990
                        set_kv(&mut curChain, &slot, Val::Bool(true));
                    }

                    // step through the chain of disabled items, disabling every other one
                    // L994
                    loop {
                        // L995
                        set_kv(&mut visited, &slot, Val::Bool(true));
                        // L996
                        slot = get_kv(&itemDisablers, &slot);
                        // L997
                        if !slot.truthy() { break; }
                        // L998
                        set_kv(&mut visited, &slot, Val::Bool(true));
                        // L999
                        set_kv(&mut trueDisabled, &slot, Val::Bool(true));
                        // L1000
                        slot = get_kv(&itemDisablers, &slot);
                        // L1001
                        if !slot.truthy() || get_kv(&visited, &slot).truthy() { break; }
                    }
                }
            }
            // L1004 (removals: order is not observable)
            for slot in trueDisabled.keys() {
                // L1005
                items.shift_remove(&Name::from(&**slot));
            }
        }

        // L1009
        for slot in &orderedSlots {
            // L1010
            let item = items.get(&slot_name(slot)).cloned();
            // L1011 (`isAnointable(nil)` is false: 0)
            let missingAnoints = match &item {
                Some(it) => build.itemsTab.getMissingAnointCount(it),
                None => 0.0,
            };
            // L1012
            if missingAnoints > 0.0 {
                // L1013
                let mut slotLabel: String = match slot.get(n!("label")) {
                    Val::Str(s) => s.to_string(),
                    Val::Num(x) => tostring_num(x),
                    other => lua_error!("attempt to concatenate local 'slotLabel' (a {} value)", other.lua_type()),
                };
                // L1014
                if missingAnoints > 1.0 {
                    // L1015
                    slotLabel = format!("{slotLabel} ({} missing)", tostring_num(missingAnoints));
                }
                // L1017
                let w = env.itemWarnings.get().get_or_new_table(n!("missingAnointWarning"));
                // L1018
                w.push(slotLabel.as_str());
            }
        }

        // Track which flask slot (1-5) each flask is in, for adjacency checks
        // L1023
        env.flaskSlotMap.set(Shared::new(PtrMap::new()));
        field_present(&*env, "flaskSlotMap");
        // L1024
        env.flaskSlotOccupied.set(Table::new());
        field_present(&*env, "flaskSlotOccupied");
        // L1025
        for slot in &orderedSlots {
            // L1026
            let slotName = slot_name(slot);
            // L1027
            let mut item = items.get(&slotName).cloned();
            // L1028
            if let Some(it) = item.clone().filter(|i| i.r#type.get() == n!("Flask")) {
                // L1029
                env.itemModDB.get().conditions.set(
                    Name::from(format!("Have{}", gsub(item_baseName(&it).as_str(), "%s+", "")).as_str()),
                    true,
                );
                // L1030
                if slot.truthy(n!("active")) {
                    // L1031
                    env.flasks.get().with_mut(|m| m.set(&it, Some(true)));
                }
                // L1033
                let flaskNum = pattern::r#match(slotName.as_str(), "Flask (%d+)", None)
                    .and_then(|c| c.first().and_then(pattern::Capture::num));
                // L1034
                if let Some(flaskNum) = flaskNum {
                    // L1035
                    env.flaskSlotMap.get().with_mut(|m| m.set(&it, Some(flaskNum)));
                    // L1036
                    env.flaskSlotOccupied.get().set(flaskNum, true);
                }
                // L1038
                if base_subType(&it) == Some(n!("Life")) {
                    // L1039
                    let highestCharges = env.itemModDB.get().multipliers.num_or(n!("LifeFlaskCharges"), 0.0);
                    // L1040
                    let chargesMax = flaskData(&it).num(n!("chargesMax"));
                    if chargesMax > highestCharges {
                        // L1041
                        env.itemModDB.get().multipliers.set(n!("LifeFlaskCharges"), chargesMax);
                    }
                }
                // L1044
                item = None;
            } else if let Some(it) = item.clone().filter(|i| i.r#type.get() == n!("Tincture")) {
                // L1046
                if slot.truthy(n!("active")) {
                    // L1047
                    env.tinctures.get().with_mut(|m| m.set(&it, Some(true)));
                }
                // L1049
                item = None;
            }
            // L1051
            let mut scale: f64 = 1.0;
            // L1052
            if item.as_ref().is_some_and(|i| i.r#type.get() == n!("Jewel") && base_subType(i) == Some(n!("Abyss"))) && slot.truthy(n!("parentSlot")) {
                // Check if the item in the parent slot has enough Abyssal Sockets
                // L1054
                let parentItem = match slot.get(n!("parentSlot")).get(n!("slotName")) {
                    Val::Str(s) => env.player.itemList().with(|m| m.get(&Name::from(&*s)).cloned()),
                    _ => None,
                };
                // L1055
                match parentItem {
                    Some(p) if !(p.abyssalSocketCount.get() < slot.get(n!("slotNum")).num()) => {
                        // L1058. Item.lua:2691 (IT-C, item_mods.rs:1453) always sets the field to a number;
                        // a nil would reach Lua's ScaleAddMod arithmetic and raise there (only an
                        // empty or all-`unscalable` list would get through), so it raises here.
                        scale = match p.extra.get(n!("socketedJewelEffectModifier")) {
                            Val::Num(x) => x,
                            other => lua_error!("parentItem.socketedJewelEffectModifier is a {} value", other.lua_type()),
                        };
                    }
                    _ => {
                        // L1056
                        item = None;
                    }
                }
            }
            // L1061
            if let Some(it) = item.as_ref().filter(|i| {
                slot.truthy(n!("nodeId")) && i.r#type.get() == n!("Jewel")
                    && jewelData(i).is_some_and(|jd| jd.truthy(n!("jewelIncEffectFromClassStart")))
            }) {
                // L1062
                let node = spec_node(&env, slot.get(n!("nodeId")).num());
                // L1063
                if let Some(d) = node.and_then(|n| n.distanceToClassStart.get()) {
                    // L1064
                    let inc = jewelData(it).map_or(0.0, |jd| jd.num(n!("jewelIncEffectFromClassStart")));
                    scale = scale + d * (inc / 100.0);
                }
            }
            // L1067
            if let Some(item) = item {
                // L1068
                env.player.itemList().with_mut(|m| m.insert(slotName, item.clone()));
                // Merge mods for this item
                // L1070
                let srcList: Vec<ModRef> = match item.modList.get() {
                    Some(l) => l.ipairs(),
                    None => slotModList_at(&item, &slot.get(n!("slotNum"))).map(|l| l.ipairs()).unwrap_or_default(),
                };
                // L1071 (`item.requirements` is always a table, contracts#class-census)
                if !accelerate.requirementsItems {
                    // L1072-L1079
                    let t = Table::new();
                    t.set(n!("source"), "Item");
                    t.set(n!("sourceItem"), obj(&item));
                    t.set(n!("sourceSlot"), slotName);
                    t.set(n!("Str"), item.requirements.get(n!("strMod")));
                    t.set(n!("Dex"), item.requirements.get(n!("dexMod")));
                    t.set(n!("Int"), item.requirements.get(n!("intMod")));
                    env.requirementsTableItems.get().push(t);
                }
                let itemType = item.r#type.get();
                // L1081
                if itemType == n!("Jewel") && base_subType(&item) == Some(n!("Abyss")) {
                    let itemModDB = env.itemModDB.get();
                    let baseName = item_baseName(&item);
                    // Update Abyss Jewel conditions/multipliers
                    // L1083
                    let cond = format!("Have{}", gsub(baseName.as_str(), " ", ""));
                    // L1084
                    let mult = gsub(baseName.as_str(), " ", "");
                    // L1085
                    if !itemModDB.conditions.truthy(Name::from(cond.as_str())) {
                        // L1086
                        itemModDB.conditions.set(Name::from(cond.as_str()), true);
                        // L1087
                        add_mult(&itemModDB.multipliers, "AbyssJewelType", 1.0);
                    }
                    // L1089
                    if slot.truthy(n!("parentSlot")) {
                        let parentName = match slot.get(n!("parentSlot")).get(n!("slotName")) {
                            Val::Str(s) => s.to_string(),
                            other => lua_error!("attempt to concatenate field 'slotName' (a {} value)", other.lua_type()),
                        };
                        // L1090
                        itemModDB.conditions.set(Name::from(format!("{cond}In{parentName}").as_str()), true);
                        // L1091
                        add_mult(&itemModDB.multipliers, &format!("{mult}In{parentName}"), 1.0);
                    }
                    // L1093
                    add_mult(&itemModDB.multipliers, "AbyssJewel", 1.0);
                    let rarity = item.rarity.get();
                    // L1094
                    if rarity == n!("NORMAL") { add_mult(&itemModDB.multipliers, "NormalAbyssJewels", 1.0); }
                    // L1095
                    if rarity == n!("MAGIC") { add_mult(&itemModDB.multipliers, "MagicAbyssJewels", 1.0); }
                    // L1096
                    if rarity == n!("RARE") { add_mult(&itemModDB.multipliers, "RareAbyssJewels", 1.0); }
                    // L1097
                    if rarity == n!("UNIQUE") || rarity == n!("RELIC") { add_mult(&itemModDB.multipliers, "UniqueAbyssJewels", 1.0); }
                    // L1098
                    add_mult(&itemModDB.multipliers, &gsub(baseName.as_str(), " ", ""), 1.0);
                }
                // L1100
                if itemType == n!("Shield") && env.allocNodes.get().with(|m| m.get(45175)).is_some_and(|n| &*n.dn.get() == "Necromantic Aegis") {
                    // Special handling for Necromantic Aegis
                    // L1102
                    let aegisModList = ModStore::ModList(None);
                    env.aegisModList.set(Some(aegisModList.clone()));
                    // L1103
                    for r#mod in &srcList {
                        // Filter out mods that apply to socketed gems, or which add supports
                        // L1105-L1111
                        let add = !has_socketedIn(r#mod);
                        // L1112
                        if add {
                            // L1113
                            aegisModList.ScaleAddMod(r#mod, scale, false);
                        } else {
                            // L1115
                            env.itemModDB.get().ScaleAddMod(r#mod, scale, false);
                        }
                    }
                } else if (slotName == n!("Weapon 1") || slotName == n!("Weapon 2")) && modDB.conditions.truthy(n!("AffectedByEnergyBlade")) {
                    // L1119
                    let previousItem = env.player.item_in(slotName);
                    // L1120
                    let r#type: Val = match previousItem.as_ref().and_then(|p| p.weaponData.get()) {
                        Some(wd) => match wd.get(1.0) {
                            Val::Nil => lua_error!("attempt to index field '?' (a nil value)"),
                            w => w.get(n!("type")),
                        },
                        None => Val::Nil,
                    };
                    // L1121
                    let info = match &r#type {
                        Val::Str(s) => env.data.weaponTypeInfo.get(&Name::from(&**s)).cloned(),
                        _ => None,
                    };
                    // L1122
                    match info.filter(|_| !matches!(&r#type, Val::Str(s) if &**s == "Bow")) {
                        Some(info) => {
                            let Some(previousItem) = previousItem else { unreachable!("info implies previousItem") };
                            // L1123
                            let name = if info.oneHand { "Energy Blade One Handed" } else { "Energy Blade Two Handed" };
                            // L1124
                            let item = new_Item(None, None, false);
                            // L1125
                            item.name.set(LStr::from(name));
                            // L1126
                            item.base.set(data().itemBases.get(name).cloned());
                            // L1127
                            item.baseName.set(Some(Name::from(name)));
                            // L1128
                            item.classRequirementModLines.set(Shared::new(Vec::new()));
                            // L1129
                            item.buffModLines.set(Shared::new(Vec::new()));
                            // L1130
                            item.enchantModLines.set(Shared::new(Vec::new()));
                            // L1131
                            item.scourgeModLines.set(Shared::new(Vec::new()));
                            // L1132
                            item.implicitModLines.set(Shared::new(Vec::new()));
                            // L1133
                            item.explicitModLines.set(Shared::new(Vec::new()));
                            // L1134
                            item.crucibleModLines.set(Shared::new(Vec::new()));
                            // L1135
                            item.quality.set(Some(0.0));
                            // L1136
                            item.rarity.set(n!("NORMAL"));
                            // L1137 LUA-QUIRK: `item.baseName` is a string, so `item.baseName.implicit`
                            // indexes the string library (nil): the implicit lines (1138-1143) never run
                            // L1145
                            item.NormaliseQuality();
                            // L1146
                            item.BuildAndParseRaw();
                            // L1147
                            item.sockets.set(previousItem.sockets.get());
                            // L1148
                            item.abyssalSocketCount.set(previousItem.abyssalSocketCount.get());
                            // L1149
                            env.player.itemList().with_mut(|m| m.insert(slotName, item.clone()));
                        }
                        None => {
                            // L1151
                            env.itemModDB.get().ScaleAddList(&srcList, scale, false);
                        }
                    }
                } else if slotName == n!("Weapon 1") && &*item.name.get() == "The Iron Mass, Gladius" {
                    // Special handling for The Iron Mass
                    // L1155
                    let theIronMass = ModStore::ModList(None);
                    env.theIronMass.set(Some(theIronMass.clone()));
                    // L1156
                    for r#mod in &srcList {
                        // Filter out mods that apply to socketed gems, or which add supports
                        // L1158-L1164
                        let add = !has_socketedIn(r#mod);
                        // L1165
                        if add {
                            // L1166
                            theIronMass.ScaleAddMod(r#mod, scale, false);
                        }
                        // Add all the stats to player as well
                        // L1169
                        env.itemModDB.get().ScaleAddMod(r#mod, scale, false);
                    }
                } else if slotName == n!("Weapon 1") && item.grantedSkills.get().get(1).is_some_and(|s| matches!(s.get(n!("skillId")), Val::Str(x) if &*x == "UniqueAnimateWeapon")) {
                    // Special handling for The Dancing Dervish
                    // L1173
                    let weaponModList1 = ModStore::ModList(None);
                    env.weaponModList1.set(Some(weaponModList1.clone()));
                    // L1174
                    for r#mod in &srcList {
                        // Filter out mods that apply to socketed gems, or which add supports
                        // L1176-L1182
                        let add = !has_socketedIn(r#mod);
                        // L1183
                        if add {
                            // L1184
                            weaponModList1.ScaleAddMod(r#mod, scale, false);
                        } else {
                            // L1186
                            env.itemModDB.get().ScaleAddMod(r#mod, scale, false);
                        }
                    }
                } else if lmatch(&item.name.get(), "Kalandra's Touch") {
                    // L1190
                    let otherKey = if slotName == n!("Ring 1") { Some(n!("Ring 2")) } else if slotName == n!("Ring 2") { Some(n!("Ring 1")) } else { None };
                    let otherRing = otherKey.and_then(|k| items.get(&k).cloned());
                    // L1191
                    if let Some(otherRing) = otherRing.filter(|o| !lmatch(&o.name.get(), "Kalandra's Touch")) {
                        let otherList = match otherRing.modList.get() {
                            Some(l) => Some(l),
                            None => slotModList_at(&otherRing, &slot.get(n!("slotNum"))),
                        };
                        // L1192
                        for r#mod in otherList.map(|l| l.ipairs()).unwrap_or_default() {
                            // Filter out SocketedIn type mods
                            // L1194-L1198 (`goto skip_mod`)
                            if has_socketedIn(&r#mod) {
                                continue;
                            }

                            // L1200-L1201 (`copyTable(mod)` then `setSource`: a fresh mod)
                            let modCopy: Mod = match item.modSource.get() {
                                Some(src) => modLib::setSource(&r#mod, &src),
                                None => setSource_nil(&r#mod),
                            };
                            // L1202
                            env.itemModDB.get().ScaleAddMod(&Arc::new(modCopy), scale, false);
                        }
                        // Adjust multipliers based on other ring
                        let mults = env.itemModDB.get().multipliers.clone();
                        // L1207 LUA-ORDER: see ITEM_PROPERTIES
                        for (mult, property) in ITEM_PROPERTIES {
                            // L1208
                            if item_property(&otherRing, property) {
                                // L1209
                                add_mult(&mults, mult, 1.0);
                                // L1210
                                add_mult(&mults, &format!("Non{mult}"), -1.0);
                            }
                        }
                        // L1213
                        if shaper_or_elder(&otherRing) {
                            // L1214
                            add_mult(&mults, "ShaperOrElderItem", 1.0);
                        }
                        // Esh of the Storm, Tul of the Blizzard
                        // L1217
                        let otherRingKey = format!("{}Equipped", gsub(item_baseName(&otherRing).as_str(), " ", ""));
                        // L1218 (a string: always truthy)
                        // L1219
                        add_mult(&mults, &otherRingKey, 1.0);
                    }
                    // Only ExtraSkill implicit mods work (none should but this is likely an in game bug)
                    // L1223
                    for r#mod in &srcList {
                        // L1224
                        if r#mod.name == n!("ExtraSkill") {
                            // L1225
                            env.itemModDB.get().ScaleAddMod(r#mod, scale, false);
                        }
                    }
                } else if itemType == n!("Quiver") && (items.get(&n!("Weapon 1")).is_some_and(|w| lmatch(&w.name.get(), "Widowhail"))
                    || initialNodeModDB(&env).Sum(ModType::INC, None, nm!["EffectOfBonusesFromQuiver"]) > 0.0)
                {
                    // L1229
                    let bonus = match items.get(&n!("Weapon 1")) {
                        Some(w) => {
                            let weaponInc = match w.baseModList.get() {
                                Some(l) => l.Sum(ModType::INC, None, nm!["EffectOfBonusesFromQuiver"]),
                                None => lua_error!("attempt to index field 'baseModList' (a nil value)"),
                            };
                            weaponInc + initialNodeModDB(&env).Sum(ModType::INC, None, nm!["EffectOfBonusesFromQuiver"])
                        }
                        None => 100.0,
                    };
                    let widowHailMod = 1.0 + bonus / 100.0;
                    // L1230
                    scale = scale * widowHailMod;
                    // L1231
                    env.modDB.get().NewMod(n!("WidowHailMultiplier"), ModType::BASE, widowHailMod, "Widowhail", 0, 0, &[]);
                    // L1232-L1235
                    let combinedList = combined(&srcList);
                    // L1236
                    env.itemModDB.get().ScaleAddList(&combinedList.ipairs(), scale, false);
                } else if {
                    let key = Name::from(format!("Corrupted{}JewelEffect", rarity_title(item.rarity.get().as_str())).as_str());
                    env.modDB.get().multipliers.truthy(key)
                        && itemType == n!("Jewel")
                        && item.corrupted.get()
                        && slot.truthy(n!("nodeId"))
                        && base_subType(&item) != Some(n!("Charm"))
                        && !match spec_node(&env, slot.get(n!("nodeId")).num()) {
                            Some(n) => n.extra.truthy(n!("containJewelSocket")),
                            None => lua_error!("attempt to index field '?' (a nil value)"),
                        }
                } {
                    // L1238
                    let key = Name::from(format!("Corrupted{}JewelEffect", rarity_title(item.rarity.get().as_str())).as_str());
                    scale = scale + env.modDB.get().multipliers.num(key);
                    // L1239-L1242
                    let combinedList = combined(&srcList);
                    // L1243
                    env.itemModDB.get().ScaleAddList(&combinedList.ipairs(), scale, false);
                } else if itemType == n!("Gloves") && calcLib::r#mod(&initialNodeModDB(&env), None, nm!["EffectOfBonusesFromGloves"]) != 1.0 {
                    // L1245
                    scale = calcLib::r#mod(&initialNodeModDB(&env), None, nm!["EffectOfBonusesFromGloves"]) - 1.0;
                    // L1246-L1249
                    let combinedList = combined(&srcList);
                    // L1250
                    let scaledList = ModStore::ModList(None);
                    // L1251
                    scaledList.ScaleAddList(&combinedList.ipairs(), scale, false);
                    // L1252
                    for r#mod in scaledList.ipairs() {
                        // L1253
                        combinedList.MergeMod(r#mod, true);
                    }
                    // L1255
                    env.itemModDB.get().AddList(&combinedList.ipairs());
                } else if itemType == n!("Boots") && calcLib::r#mod(&initialNodeModDB(&env), None, nm!["EffectOfBonusesFromBoots"]) != 1.0 {
                    // L1257
                    scale = calcLib::r#mod(&initialNodeModDB(&env), None, nm!["EffectOfBonusesFromBoots"]) - 1.0;
                    // L1258-L1261
                    let combinedList = combined(&srcList);
                    // L1262
                    let scaledList = ModStore::ModList(None);
                    // L1263
                    scaledList.ScaleAddList(&combinedList.ipairs(), scale, false);
                    // L1264
                    for r#mod in scaledList.ipairs() {
                        // L1265
                        combinedList.MergeMod(r#mod, true);
                    }
                    // L1267
                    env.itemModDB.get().AddList(&combinedList.ipairs());
                } else {
                    // L1269
                    env.itemModDB.get().ScaleAddList(&srcList, scale, false);
                }
                // set conditions on restricted items
                // L1272
                if let Some(classRestriction) = item.classRestriction.get() {
                    let Some(title) = item.title.get() else {
                        lua_error!("attempt to index field 'title' (a nil value)")
                    };
                    // L1273
                    env.itemModDB.get().conditions.set(Name::from(gsub(&title, " ", "").as_str()), classRestriction);
                }
                // L1275
                if itemType != n!("Jewel") && itemType != n!("Flask") && itemType != n!("Tincture") && itemType != n!("Graft") {
                    let itemModDB = env.itemModDB.get();
                    let mults = itemModDB.multipliers.clone();
                    // Update item counts
                    // L1277
                    let key: &str;
                    let rarity = item.rarity.get();
                    // L1278
                    if rarity == n!("UNIQUE") || rarity == n!("RELIC") {
                        // L1279
                        if item_foulborn(&item) {
                            // L1280
                            add_mult(&mults, "FoulbornUniqueItem", 1.0);
                        }
                        // L1282
                        key = "UniqueItem";
                    } else if rarity == n!("RARE") {
                        // L1284
                        key = "RareItem";
                    } else if rarity == n!("MAGIC") {
                        // L1286
                        key = "MagicItem";
                    } else {
                        // L1288
                        key = "NormalItem";
                    }
                    // L1290
                    add_mult(&mults, key, 1.0);
                    // L1291
                    itemModDB.conditions.set(Name::from(format!("{key}In{}", slotName.as_str()).as_str()), true);
                    // L1292 LUA-ORDER: see ITEM_PROPERTIES
                    for (mult, property) in ITEM_PROPERTIES {
                        // L1293
                        if item_property(&item, property) {
                            // L1294
                            add_mult(&mults, mult, 1.0);
                        } else {
                            // L1296
                            add_mult(&mults, &format!("Non{mult}"), 1.0);
                        }
                    }
                    // L1299
                    if shaper_or_elder(&item) {
                        // L1300
                        add_mult(&mults, "ShaperOrElderItem", 1.0);
                    }
                    // L1302
                    add_mult(&mults, &format!("{}Item", gsub(&gsub(itemType.as_str(), " ", ""), ".+Handed", "")), 1.0);
                    // base ring count, e.g. Cryonic, Synaptic for Breachlord Esh of the Storm, Tul of the Blizzard
                    // L1304
                    if itemType == n!("Ring") {
                        // L1305
                        let key = format!("{}Equipped", gsub(item_baseName(&item).as_str(), " ", ""));
                        // L1306
                        add_mult(&mults, &key, 1.0);
                    }
                    // Calculate socket counts
                    // L1309 (`{ R = 0, G = 0, B = 0, W = 0 }`)
                    let mut slotEmptySocketsCount: [f64; 4] = [0.0, 0.0, 0.0, 0.0];
                    let colour_index = |c: &str| match c { "R" => Some(0), "G" => Some(1), "B" => Some(2), "W" => Some(3), _ => None };
                    // L1310
                    let mut slotGemSocketsCount: f64 = 0.0;
                    // L1311
                    let mut socketedGems: Vec<GemInstanceRef> = Vec::new();
                    // Loop through socket groups to calculate number of socketed gems
                    // L1313
                    for_ipairs!((_i, socketGroup) in env.build.skillsTab.socketGroupList.get() => {
                        // L1314 (`socketGroup.gemList` is always a list)
                        if socketGroup.source.get().is_none() && socketGroup.enabled.get() && socketGroup.slot.get() == Some(slotName) {
                            // L1315
                            for_ipairs!((_j, gem) in socketGroup.gemList.get() => {
                                // L1316
                                if gem.gemData.get().is_some() && gem.enabled.get() {
                                    // L1317
                                    socketedGems.push(gem.clone());
                                }
                            });
                        }
                    });
                    // L1322
                    for_ipairs!((i, socket) in item.sockets.get() => {
                        // Check socket color to ignore abyssal sockets
                        let color = socket.get(n!("color"));
                        // L1324
                        if let Some(c) = color.str().and_then(|s| colour_index(&s)) {
                            // L1325
                            slotGemSocketsCount = slotGemSocketsCount + 1.0;
                            // loop through sockets indexes that are greater than number of socketed gems
                            // L1327
                            if i > socketedGems.len() {
                                // L1328
                                slotEmptySocketsCount[c] = slotEmptySocketsCount[c] + 1.0;
                            }
                        }
                    });
                    // L1332 (`{ R = 0, G = 0, B = 0 }`)
                    let mut socketedColours: [f64; 3] = [0.0, 0.0, 0.0];
                    let nSocketedGems = socketedGems.len() as f64;
                    // Only gems that fit in the item's sockets contribute to multipliers
                    // L1334
                    let last = m_min(slotGemSocketsCount, nSocketedGems);
                    let mut i: f64 = 1.0;
                    while i <= last {
                        // L1335
                        let Some(gemData) = socketedGems[i as usize - 1].gemData.get() else { unreachable!("filtered at 1316") };
                        let tags = &gemData.tags;
                        // L1336
                        if tags.contains(&n!("strength")) {
                            // L1337
                            socketedColours[0] = socketedColours[0] + 1.0;
                        }
                        // L1339
                        if tags.contains(&n!("dexterity")) {
                            // L1340
                            socketedColours[1] = socketedColours[1] + 1.0;
                        }
                        // L1342
                        if tags.contains(&n!("intelligence")) {
                            // L1343
                            socketedColours[2] = socketedColours[2] + 1.0;
                        }
                        i = i + 1.0;
                    }
                    let sn = slotName.as_str();
                    // L1346
                    mults.set(Name::from(format!("SocketedGemsIn{sn}").as_str()), m_min(slotGemSocketsCount, nSocketedGems));
                    // L1347
                    mults.set(Name::from(format!("SocketedRedGemsIn{sn}").as_str()), socketedColours[0]);
                    // L1348
                    mults.set(Name::from(format!("SocketedGreenGemsIn{sn}").as_str()), socketedColours[1]);
                    // L1349
                    mults.set(Name::from(format!("SocketedBlueGemsIn{sn}").as_str()), socketedColours[2]);
                    // L1350
                    mults.set(
                        Name::from(format!("EmptySocketIn{sn}").as_str()),
                        m_min(slotGemSocketsCount, slotEmptySocketsCount[0] + slotEmptySocketsCount[1] + slotEmptySocketsCount[2] + slotEmptySocketsCount[3]),
                    );
                    // L1351
                    add_mult(&mults, "EmptyRedSocketsInAnySlot", slotEmptySocketsCount[0]);
                    // L1352
                    add_mult(&mults, "EmptyGreenSocketsInAnySlot", slotEmptySocketsCount[1]);
                    // L1353
                    add_mult(&mults, "EmptyBlueSocketsInAnySlot", slotEmptySocketsCount[2]);
                    // L1354
                    add_mult(&mults, "EmptyWhiteSocketsInAnySlot", slotEmptySocketsCount[3]);
                    // Warn if socketed gems over socket limit
                    // L1356
                    if nSocketedGems > slotGemSocketsCount {
                        // L1357
                        let w = env.itemWarnings.get().get_or_new_table(n!("socketLimitWarning"));
                        // L1358
                        w.push(slotName);
                    }
                }
            }
        }
        // Override empty socket calculation if set in config
        let mults = env.itemModDB.get().multipliers.clone();
        for (k, cfgKey) in [
            // L1364
            ("EmptyRedSocketsInAnySlot", "overrideEmptyRedSockets"),
            // L1365
            ("EmptyGreenSocketsInAnySlot", "overrideEmptyGreenSockets"),
            // L1366
            ("EmptyBlueSocketsInAnySlot", "overrideEmptyBlueSockets"),
            // L1367
            ("EmptyWhiteSocketsInAnySlot", "overrideEmptyWhiteSockets"),
        ] {
            let v = lor(env.configInput.get(Name::from(cfgKey)), || mults.get(Name::from(k)));
            mults.set(Name::from(k), v);
        }
        // L1368
        if let Some(f) = r#override.toggleFlask.get() {
            // L1369
            if env.flasks.get().with(|m| m.get(&f)).is_some_and(|v| v) {
                // L1370
                env.flasks.get().with_mut(|m| m.set(&f, None));
            } else {
                // L1372
                env.flasks.get().with_mut(|m| m.set(&f, Some(true)));
            }
        }
        // L1375
        if let Some(t) = r#override.toggleTincture.get() {
            // L1376
            if env.tinctures.get().with(|m| m.get(&t)).is_some_and(|v| v) {
                // L1377
                env.tinctures.get().with_mut(|m| m.set(&t, None));
            } else {
                // L1379
                env.tinctures.get().with_mut(|m| m.set(&t, Some(true)));
            }
        }
    }

    // Merge env.itemModDB with env.ModDB
    // L1385
    mergeDB(&env.modDB.get(), Some(&env.itemModDB.get()));

    // Add granted passives (e.g., amulet anoints)
    // L1388
    if !accelerate.nodeAlloc {
        let spec = env.spec.get();
        let tree = spec.tree.get();
        // L1389 (`pairs` over a list: index order)
        for passive in env.modDB.get().List(None, nm!["GrantedPassive"]) {
            let passive = Val::from_value(&passive);
            let lookup = |t: &Table| -> Option<NodeRef> {
                match &passive {
                    Val::Str(s) => t.obj::<Node>(&**s),
                    Val::Num(x) => t.obj::<Node>(*x),
                    _ => None,
                }
            };
            // L1390
            let mut node = lookup(&tree.notableMap).or_else(|| lookup(&tree.ascendancyMap));
            // L1391
            let specNode = node.as_ref().and_then(|n| int_key(n.id)).and_then(|k| spec.nodes.with(|m| m.get(k))); // use the conquered node data, if available
            // L1392
            if node.is_none() {
                node = match build.latestTree.get() {
                    Some(t) => lookup(&t.ascendancyMap),
                    None => lua_error!("attempt to index field 'latestTree' (a nil value)"),
                };
            }
            // L1393 (CCR-0105: the node-id keys of `override.removeNodes`; the node-keyed half never
            // matches a number key, so `not override.removeNodes or not …[node.id]` is this alone)
            if let Some(node) = node.filter(|n| !removeNodes_byId(&r#override, n.id)) {
                let k = node_key(node.id, "env.allocNodes");
                // L1394
                let v = specNode.unwrap_or_else(|| node.clone());
                env.allocNodes.get().with_mut(|m| m.set(k, Some(v)));
                // L1395
                env.grantedPassives.get().with_mut(|m| m.set(k, Some(true)));
                // L1396
                env.extraRadiusNodeList.get().with_mut(|m| m.set(k, None));
            }
        }
    }

    // Add granted ascendancy node (e.g., Forbidden Flame/Flesh combo)
    // L1402 (`{ side = …, matched = … }` per name)
    let mut matchedName: IndexMap<LStr, (Val, bool)> = IndexMap::new();
    // L1403 (`pairs` over a list: index order)
    for ascTbl in env.modDB.get().List(None, nm!["GrantedAscendancyNode"]) {
        let ascTbl = Val::from_value(&ascTbl);
        // L1404
        let name = ascTbl.get(n!("name")).str();
        let side = ascTbl.get(n!("side"));
        // L1405
        let entry = name.as_ref().and_then(|k| matchedName.get(k).cloned());
        if let (Some(name), Some((_, false))) = (name.clone(), entry.filter(|(s, _)| !s.raw_eq(&side))) {
            // L1406
            if let Some(e) = matchedName.get_mut(&name) {
                e.1 = true;
            }
            // L1407
            let node = env.spec.get().tree.get().ascendancyMap.obj::<Node>(&*name).or_else(|| match build.latestTree.get() {
                Some(t) => t.ascendancyMap.obj::<Node>(&*name),
                None => lua_error!("attempt to index field 'latestTree' (a nil value)"),
            });
            // L1408 (CCR-0105; as L1393)
            if let Some(node) = node.filter(|n| !removeNodes_byId(&r#override, n.id)) {
                let curClassName = env.spec.get().curClassName.get().map_or(Val::Nil, Val::from);
                let conditions = env.itemModDB.get().conditions.clone();
                // L1409
                if conditions.get(n!("ForbiddenFlesh")).raw_eq(&curClassName) && conditions.get(n!("ForbiddenFlame")).raw_eq(&curClassName) {
                    let k = node_key(node.id, "env.allocNodes");
                    // L1410
                    env.allocNodes.get().with_mut(|m| m.set(k, Some(node.clone())));
                    // L1411
                    env.grantedPassives.get().with_mut(|m| m.set(k, Some(true)));
                }
            }
        } else {
            // L1415
            match name {
                Some(k) => {
                    matchedName.insert(k, (side, false));
                }
                None => lua_error!("table index is nil"),
            }
        }
    }

    // Merge modifiers for allocated passives
    // L1420
    {
        // L1421
        let (modList, explodeSources) = super::buildModListForNodeList(&env, &env.allocNodes.get(), true);
        // L1422
        env.modDB.get().AddList(&modList.ipairs());
        // L1423 (`tableConcat`, Common.lua 610-619: a new list, the node sources first)
        let mut t = explodeSources;
        t.extend(env.explodeSources.get().snapshot());
        env.explodeSources.set(Shared::new(t));
    }
    // L1425 (`override` is never nil here: CalcSetup.lua:485)
    if r#override.extraJewelFuncs.get().is_none() {
        // L1426 (a table already)
        // L1427
        let extraJewelFuncs = ModStore::ModList(None);
        r#override.extraJewelFuncs.set(Some(extraJewelFuncs.clone()));
        field_present(&*r#override, "extraJewelFuncs");
        // L1428
        extraJewelFuncs.actor.set(Rc::downgrade(&env.player));
        r#override
            .extra
            .set(Name::from(EXTRA_JEWEL_FUNCS_ACTOR), obj(&env.player));
        // L1429
        for row in env.modDB.get().Tabulate(Some(ModType::LIST), None, nm!["ExtraJewelFunc"]) {
            // L1430
            extraJewelFuncs.AddMod(row.r#mod.clone());
        }
        // L1432
        if extraJewelFuncs.len() > 0 {
            // L1433
            return Flow::Restart;
        }
    }

    // Find skills granted by tree nodes
    // L1438
    if !accelerate.nodeAlloc {
        // L1439 LUA-ORDER: node-id map (LjIntMap, LuaJIT `next` order)
        for (_, node) in env.allocNodes.get().with(|m| m.pairs()) {
            let Some(grantedSkills) = node.grantedSkills.get() else {
                lua_error!("bad argument #1 to 'ipairs' (table expected, got nil)")
            };
            // L1440
            for_ipairs!((_i, skill) in grantedSkills => {
                // L1441
                let grantedSkill = copy_skill(&skill);
                // L1442
                grantedSkill.set(n!("sourceNode"), obj(&node));
                // L1443
                env.grantedSkillsNodes.get().push(grantedSkill);
            });
        }
    }

    // Merge Granted Skills Tables
    // L1449 (`tableConcat`, Common.lua 610-619)
    let grantedSkills: Vec<Table> = env
        .grantedSkillsNodes
        .get()
        .snapshot()
        .into_iter()
        .chain(env.grantedSkillsItems.get().snapshot())
        .collect();
    env.grantedSkills.set(Shared::new(grantedSkills));

    Flow::Normal
}
