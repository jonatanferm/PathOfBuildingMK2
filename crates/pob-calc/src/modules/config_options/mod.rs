//! The ConfigOptions `apply` registry (calc doc §8.1). Owner: U0 (frozen
//! frame); the bodies are CO-A (`apply_a`, ConfigOptions.lua 1-1188) and CO-B
//! (`apply_b`, 1189-2380).
#![allow(non_snake_case)]

use std::sync::LazyLock;

use crate::prelude::*;

pub mod apply_a;
pub mod apply_b;

pub use crate::data_fns::FnRow;

/// `apply(val, modList, enemyModList, build)` (ConfigOptions.lua).
pub type ConfigApply = fn(val: &Val, modList: &ModList, enemyModList: &ModList, build: &BuildRef);

/// Every `ConfigOptions[i].apply` closure by id (`apply_a::ROWS ++ apply_b::ROWS`).
pub static CONFIG_APPLY: LazyLock<ahash::AHashMap<&'static str, &'static FnRow<ConfigApply>>> =
    LazyLock::new(|| {
        apply_a::ROWS
            .iter()
            .chain(apply_b::ROWS.iter())
            .map(|r| (r.id, r))
            .collect()
    });

/// The body of a config option's `apply` (unknown id → `unported!` no-op).
pub fn configApply(r: &LuaFnRef) -> ConfigApply {
    fn missing(_: &Val, _: &ModList, _: &ModList, _: &BuildRef) {
        let () = unported!("Modules/ConfigOptions.lua (apply not in the registry)");
    }
    CONFIG_APPLY
        .get(r.id.as_str())
        .map_or(missing as ConfigApply, |row| row.f)
}
