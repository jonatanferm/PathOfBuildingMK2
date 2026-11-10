//! @pob-file Modules/CalcSetup.lua @16de4b82d
//! CalcSetup.lua 121-304: node mod lists and jewel functions.
//!
//! Owner: CS0.
//!
//! Representation of an `env.radiusJewelList` entry (CalcSetup.lua:886-894 / 914-922, CS2
//! builds them): a dynamic [`Table`] with
//! * `type`: `Val::Str` (`"Self"`, `"SelfUnalloc"`, `"Threshold"`, `"Other"`);
//! * `nodes`: `node.nodesInRadius[radiusIndex]`, i.e. `Val::Obj(Rc<LjIntMap<NodeRef>>)`
//!   (CCR-0024 §3), or the `{ }` fallback as an empty `Val::Table` (a `Table` of
//!   `id -> Val::Obj(NodeRef)` is accepted too);
//! * `func`: the jewel func, `Val::Data(Value::JewelFunc(spec))` (`func.func` of a
//!   `JewelFunc`/`ExtraJewelFunc` mod value `{ type = …, func = … }`, whose `func` the ModParser
//!   emits as `Value::JewelFunc`; the CalcSetup.lua:877-884 default func is
//!   `JewelFuncSpec { key: "$default", .. }`, CCR-0039 §4). It is run by MP7's `callJewelFunc`;
//! * `item`: `Val::Obj(ItemRef)`; `nodeId`: `Val::Num`; `data`: `Val::Table`;
//!   `attributes`: not read here.
//!
//! The two functions return Lua's second value too (CCR-0074): `buildModListForNode` the
//! explode source (`nil` or the node, as a `Val`; never `false`, since `ModList:Flag` returns
//! `true` or nothing), `buildModListForNodeList` the `explodeSources` list (only
//! `Val::Obj(NodeRef)` entries). `reuse` is the scratch
//! `ModList` the Lua passes, not a flag (CCR-0074).
#![allow(non_snake_case)]

use std::any::Any;

use crate::classes::passive_tree::node_id_val;
use crate::modules::calc_tools;
use crate::modules::jewel_funcs::callJewelFunc;
use crate::prelude::*;

// ---- Rust-only plumbing ---------------------------------------------------------------------

/// `t[node.id]` for a node-id map key: integral ids only (a NaN / string id is never a key of
/// an `LjIntMap`, and `t[nan]` reads nil in Lua).
fn int_key(id: f64) -> Option<i64> {
    (id.is_finite() && id == id.trunc()).then_some(id as i64)
}

/// `env.allocNodes[node.id]` (truthy when present).
fn isAlloc(env: &Env, node: &Node) -> bool {
    match int_key(node.id) {
        Some(k) => env.allocNodes.get().with(|m| m.contains_key(k)),
        None => false,
    }
}

/// Lua `v == "<s>"` for a string literal.
fn is_str(v: &Val, s: &str) -> bool {
    matches!(v, Val::Str(x) if &**x == s)
}

/// `rad.nodes[node.id]` (nil → None). Raises like Lua when `rad.nodes` cannot be indexed.
fn radNodesAt(rad: &Table, node: &Node) -> Option<NodeRef> {
    match rad.get(n!("nodes")) {
        Val::Obj(o) => {
            if let Some(m) = o.downcast_ref::<LjIntMap<NodeRef>>() {
                return int_key(node.id).and_then(|k| m.get(k));
            }
            lua_error!("attempt to index field 'nodes' (a userdata value)")
        }
        Val::Table(t) => {
            let v = match node_id_val(node) {
                Val::Num(x) => match int_key(x) {
                    Some(k) => t.get(k),
                    None => Val::Nil,
                },
                Val::Str(s) => t.get(&*s),
                _ => Val::Nil,
            };
            match v {
                Val::Nil => None,
                other => match other.obj::<Node>() {
                    Some(n) => Some(n),
                    None => lua_error!(
                        "rad.nodes[{}] is a {} value, not a node",
                        tostring_num(node.id),
                        other.lua_type()
                    ),
                },
            }
        }
        other => lua_error!(
            "attempt to index field 'nodes' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `rad.data` (a table; anything else raises when the func or the caller indexes it).
fn radData(rad: &Table) -> Table {
    match rad.get(n!("data")) {
        Val::Table(t) => t,
        other => lua_error!(
            "attempt to index field 'data' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `rad.func(node, out, data)`.
fn callRadFunc(rad: &Table, node: Option<&NodeRef>, out: &ModList, data: &Table) {
    match rad.get(n!("func")) {
        Val::Data(Value::JewelFunc(spec)) => callJewelFunc(&spec, node, out, data),
        other => lua_error!(
            "attempt to call field 'func' (a {} value)",
            other.lua_type()
        ),
    }
}

/// `"Tree:" .. id` (number ids as `%.14g`, legion / tattoo string ids verbatim).
fn treeSource(id: &Val) -> LStr {
    match id {
        Val::Num(x) => LStr::from(format!("Tree:{}", tostring_num(*x))),
        Val::Str(s) => LStr::from(format!("Tree:{s}")),
        other => lua_error!("attempt to concatenate a {} value", other.lua_type()),
    }
}

/// The mod names of a ModList, in list order (`modList[i].name`).
fn listNames(modList: &ModList) -> Vec<Name> {
    modList.ipairs().iter().map(|m| m.name).collect()
}

// ---- CalcSetup.lua 121-304 --------------------------------------------------------------------

/// `calcs.buildModListForNode(env, node, reuse)` → `modList[, explode]`.
/// `reuse`: a ModList to recycle instead of allocating. Only safe when the caller discards the
/// result. The second value is `Val::Nil` unless the list has a `CanExplode` mod, then
/// `modList:Flag(nil, "CanExplode") and node` (`Val::Obj(node)`, or `Val::Nil` when the flag is
/// off: `Flag` returns `true` or nothing, never `false`).
/// Signature: CCR-0074 §1.
/// @pob Modules/CalcSetup.lua:123-256 h=610f22bb
pub fn buildModListForNode(
    env: &EnvRef,
    node: &NodeRef,
    reuse: Option<&ModList>,
) -> (ModList, Val) {
    // L124
    let mut modList: ModList;
    // L125
    if let Some(reuse) = reuse {
        // Reset the scratch list so non-MAIN calculations can reuse it for each node
        // L127
        modList = reuse.clone();
        // L128-134: `modList[i] = nil` for every i, `multipliers`/`conditions` wiped,
        // `actor = wipeTable(actor)`, `parent = false`. `ModStore::wipe` is exactly these
        // five writes on a ModList; the actor becomes the dangling `{ }` (the scratch list
        // never has a real actor: buildModListForNodeList creates it without a parent).
        modList.wipe();
    } else {
        // L136
        modList = ModStore::ModList(None);
    }
    // L138
    if node.r#type.get() == n!("Keystone") {
        // L139 (`AddMod(nil)` is `t_insert(self, nil)`, a no-op)
        if let Some(m) = node.keystoneMod.get() {
            modList.AddMod(m);
        }
    } else {
        // L141
        modList.AddList(&node.modList.get().ipairs());
    }

    // Run first pass radius jewels
    let radiusJewelList = env.radiusJewelList.get();
    // L145
    for i in 1..=radiusJewelList.len() {
        // L146
        let Some(rad) = radiusJewelList.get(i) else {
            lua_error!("attempt to index local 'rad' (a nil value)")
        };
        // L147
        if is_str(&rad.get(n!("type")), "Other") {
            // L148
            let radNode = radNodesAt(&rad, node);
            // L149
            if let Some(radNode) = radNode {
                if radNode.r#type.get() != n!("Mastery") {
                    // L150
                    callRadFunc(&rad, Some(node), &modList, &radData(&rad));
                }
            }
        }
    }

    // prefilter the modlist so that every :Flag() call does not have to go through the entire mod list
    // L156
    let (
        mut hasNoEffect,
        mut hasAllocNoEffect,
        mut hasScale,
        mut hasOtherEffect,
        mut hasExtraSkill,
        mut hasExplode,
    ) = (false, false, false, false, false, false);
    // L157
    for name in listNames(&modList) {
        // L158
        // L159
        if name == n!("PassiveSkillHasNoEffect") {
            // L160
            hasNoEffect = true;
        } else if name == n!("AllocatedPassiveSkillHasNoEffect") {
            // L162
            hasAllocNoEffect = true;
        } else if name == n!("PassiveSkillEffect") {
            // L164
            hasScale = true;
        } else if name == n!("PassiveSkillHasOtherEffect") {
            // L166
            hasOtherEffect = true;
        } else if name == n!("ExtraSkill") {
            // L168
            hasExtraSkill = true;
        } else if name == n!("CanExplode") {
            // L170
            hasExplode = true;
        }
    }

    // L174
    if (hasNoEffect && modList.Flag(None, nm!["PassiveSkillHasNoEffect"]))
        || (isAlloc(env, node)
            && (hasAllocNoEffect && modList.Flag(None, nm!["AllocatedPassiveSkillHasNoEffect"])))
    {
        // L175
        modList.wipe();
        // L176
        hasScale = false;
        // L177
        hasOtherEffect = false;
        // L178
        hasExtraSkill = false;
        // L179
        hasExplode = false;
    }

    // Apply effect scaling
    // L183
    if hasScale {
        // L184
        let scale = calc_tools::r#mod(&modList, None, nm!["PassiveSkillEffect"]);
        // L185
        if scale != 1.0 {
            // L186
            let scaledList = ModStore::ModList(None);
            // L187
            scaledList.ScaleAddList(&modList.ipairs(), scale, false);
            // L188
            modList = scaledList;
        }
    }

    // Run second pass radius jewels
    // L193
    let mut rescan = false;
    // L194
    for i in 1..=radiusJewelList.len() {
        // L195
        let Some(rad) = radiusJewelList.get(i) else {
            lua_error!("attempt to index local 'rad' (a nil value)")
        };
        // L196
        let inRadius = radNodesAt(&rad, node).is_some_and(|radNode| {
            radNode.r#type.get() != n!("Mastery")
                && (is_str(&rad.get(n!("type")), "Threshold")
                    || (is_str(&rad.get(n!("type")), "Self") && isAlloc(env, node))
                    || (is_str(&rad.get(n!("type")), "SelfUnalloc") && !isAlloc(env, node)))
        });
        if inRadius {
            // L197
            callRadFunc(&rad, Some(node), &modList, &radData(&rad));
            // L198
            rescan = true;
            // L199
            hasOtherEffect = false;
            // L200
            hasExtraSkill = false;
            // L201
            hasExplode = false;
        }
    }

    // L205
    if rescan {
        // L206
        for name in listNames(&modList) {
            // L207
            // L208
            if name == n!("PassiveSkillHasOtherEffect") {
                // L209
                hasOtherEffect = true;
            } else if name == n!("ExtraSkill") {
                // L211
                hasExtraSkill = true;
            } else if name == n!("CanExplode") {
                // L213
                hasExplode = true;
            }
        }
    }

    // L219
    if hasOtherEffect && modList.Flag(None, nm!["PassiveSkillHasOtherEffect"]) {
        // L220
        let newMods = modList.List(None, nm!["NodeModifier"]);
        // L221
        for (i0, v) in newMods.iter().enumerate() {
            // L222
            let r#mod = match Val::from_value(v).get(n!("mod")) {
                Val::Mod(m) => m,
                other => lua_error!("NodeModifier value: mod is a {} value", other.lua_type()),
            };
            // L223
            if i0 == 0 {
                // L224
                modList.wipe();
                // L225
                hasExtraSkill = false;
                // L226
                hasExplode = false;
            }
            // L228
            if r#mod.name == n!("ExtraSkill") {
                // L229
                hasExtraSkill = true;
            } else if r#mod.name == n!("CanExplode") {
                // L231
                hasExplode = true;
            }
            // L233
            modList.AddMod(r#mod);
        }
    }

    // L237
    let grantedSkills = match node.grantedSkills.get() {
        Some(g) => {
            g.wipe();
            g
        }
        None => Shared::new(Vec::new()),
    };
    node.grantedSkills.set(Some(grantedSkills.clone()));
    // L238
    if hasExtraSkill {
        // L239
        let list = modList.List(None, nm!["ExtraSkill"]);
        // L240
        for v in &list {
            // L241
            let skill = Val::from_value(v);
            // L242
            if !is_str(&skill.get(n!("name")), "Unknown") {
                // L243-247
                let t = Table::new();
                t.set(n!("skillId"), skill.get(n!("skillId")));
                t.set(n!("level"), skill.get(n!("level")));
                t.set(n!("source"), treeSource(&node_id_val(node)));
                grantedSkills.push(t);
            }
        }
    }

    // L252
    if hasExplode {
        // L253
        // `ModList:Flag` returns `true` or nothing (ModList.lua:156-175), so `Flag and node` is
        // the node or nil, never `false`.
        let explode = if modList.Flag(None, nm!["CanExplode"]) {
            Val::Obj(node.clone() as Rc<dyn Any>)
        } else {
            Val::Nil
        };
        (modList, explode)
    } else {
        // L255
        (modList, Val::Nil)
    }
}

/// Build list of modifiers from the listed tree nodes
/// `calcs.buildModListForNodeList(env, nodeList, finishJewels)` → `modList, explodeSources`.
/// `nodeList` is a node-id map (LuaJIT order, D8); `finishJewels` runs the jewel funcs' "finish" call (node = nil).
/// Signature: CCR-0074 §2.
/// @pob Modules/CalcSetup.lua:259-304 h=6c6f8915
pub fn buildModListForNodeList(
    env: &EnvRef,
    nodeList: &Shared<LjIntMap<NodeRef>>,
    finishJewels: bool,
) -> (ModList, Vec<Val>) {
    // Initialise radius jewels
    // L261 (a list built with t_insert: pairs = index order)
    for rad in env.radiusJewelList.get().snapshot() {
        // L262
        let data = radData(&rad);
        data.wipe();
        // L263
        data.set(n!("modSource"), treeSource(&rad.get(n!("nodeId"))));
    }

    // Add node modifiers
    // L267
    let modList = ModStore::ModList(None);
    // L268
    let mut explodeSources: Vec<Val> = Vec::new();
    // Outside MAIN mode the per-node list is merged into modList and then
    // dropped, so a single list can be recycled for every node instead of
    // allocating one each time.
    // L272
    let scratch = if env.mode != Mode::MAIN {
        Some(ModStore::ModList(None))
    } else {
        None
    };
    // L273 LUA-ORDER: node-id map, LuaJIT `next` order (LjIntMap)
    for (_, node) in nodeList.with(|m| m.pairs()) {
        // L274
        let (nodeModList, explode) = buildModListForNode(env, &node, scratch.as_ref());
        // L275 (`t_insert(t, nil)` appends nothing)
        if !explode.is_nil() {
            explodeSources.push(explode);
        }
        // L276
        modList.AddList(&nodeModList.ipairs());
        // L277
        if env.mode == Mode::MAIN {
            // L278
            node.finalModList.set(Some(nodeModList));
        }
    }

    // L282
    if finishJewels {
        // Process extra radius nodes; these are unallocated nodes near conversion or threshold jewels that need to be processed
        // L284 LUA-ORDER: node-id map
        for (_, node) in env.extraRadiusNodeList.get().with(|m| m.pairs()) {
            // L285
            let (nodeModList, _) = buildModListForNode(env, &node, scratch.as_ref());
            // L286
            if env.mode == Mode::MAIN {
                // L287
                node.finalModList.set(Some(nodeModList));
            }
        }

        // Finalise radius jewels
        // L292 (a list: index order)
        for rad in env.radiusJewelList.get().snapshot() {
            // L293
            callRadFunc(&rad, None, &modList, &radData(&rad));
            // L294
            if env.mode == Mode::MAIN {
                let item = match rad.get(n!("item")).obj::<Item>() {
                    Some(it) => it,
                    None => lua_error!("attempt to index field 'item' (not an item)"),
                };
                // L295
                let jewelRadiusData = match item.jewelRadiusData.get() {
                    Some(t) => t,
                    None => {
                        // L296
                        let t = Table::new();
                        item.jewelRadiusData.set(Some(t.clone()));
                        t
                    }
                };
                // L298
                let nodeId = rad.get(n!("nodeId"));
                let key = match nodeId {
                    Val::Num(x) => Key::from(x),
                    other => lua_error!("rad.nodeId is a {} value", other.lua_type()),
                };
                jewelRadiusData.set(key, radData(&rad));
            }
        }
    }

    // L303
    (modList, explodeSources)
}
