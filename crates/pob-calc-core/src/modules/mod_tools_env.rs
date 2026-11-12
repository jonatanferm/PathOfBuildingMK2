//! @pob-file Modules/ModTools.lua @16de4b82d
//! `modLib.mergeKeystones` (Modules/ModTools.lua 246-259; the rest of modLib is `pob_gamedata::mod_tools`).
//! Owner: CT.
#![allow(non_snake_case, clippy::explicit_auto_deref)]

use crate::prelude::*;
use pob_gamedata::mod_tools::setSource;

/// `tree.keystoneMap[name]` as a node (PT stores nodes in the map as `Val::Obj(NodeRef)`).
fn keystone_node(v: &Val) -> NodeRef {
    v.obj::<Node>()
        .unwrap_or_else(|| lua_error!("keystoneMap entry is a {} value, not a node", v.lua_type()))
}

/// @pob Modules/ModTools.lua:246-259 h=d4e85c65
/// Merge keystone modifiers
///
/// CCR-0091: Lua's `modLib.setSource(mod, …)` (L255) re-sources the keystone node's own mod in
/// place, so the tree's keystone mod keeps the item source for the rest of the PoB process.
/// `ModRef` is immutable (§3.6): the port adds a re-sourced copy and leaves the node's mod alone.
pub fn mergeKeystones(env: &EnvRef, modDB: &ModDB) {
    // L249 (`env.keystonesAdded` always holds a table in the port)
    let keystonesAdded = env.keystonesAdded.get();
    // L250
    for modObj in modDB.Tabulate(Some(ModType::LIST), None, &[n!("Keystone")]) {
        let value = Val::from_value(&modObj.value);
        let key: Option<Key> = match &value {
            Val::Nil => None,
            Val::Str(s) => Some(Key::S(Name::from(&**s))),
            Val::Num(x) => Some(Key::from(*x)),
            Val::Bool(b) => Some(Key::B(*b)),
            other => lua_error!("Keystone LIST value is a {} value", other.lua_type()),
        };
        let keystoneMap = env.spec.get().tree.get().keystoneMap.clone();
        // L251
        let Some(key) = key else { continue };
        if !keystonesAdded.truthy(key) && keystoneMap.truthy(key) {
            // L252
            keystonesAdded.set(key, true);
            // L253
            let fromTree = modObj.r#mod.source.as_deref().is_some_and(|s| {
                pob_lua::lmatch(&pob_lua::string::lower(s), "tree", None).is_none()
            });
            // L254
            let node = keystone_node(&keystoneMap.get(key));
            for r#mod in node.modList.get().ipairs() {
                // L255
                if fromTree {
                    let source = modObj.r#mod.source.as_deref().expect("fromTree");
                    modDB.AddMod(Arc::new(setSource(&r#mod, source)));
                } else {
                    modDB.AddMod(r#mod);
                }
            }
        }
    }
}
