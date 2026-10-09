//! Identity of a Lua closure stored in data (calc doc §4.4 `LuaFnRef`, §8, D7).
//!
//! The dump replaces every function value by
//! `{"$fn": "<canonical path>", "src": "<file>:<a>-<b>", "hash": "<sha256>"}`
//! (crates/pob-dump/FORMAT.md, AMENDMENTS A2: the hash is the sha256 of the
//! source lines, not the 8-hex SHA-1 of calc doc §9.1). The implementation of
//! each closure lives in a Rust registry (`pob_calc::data_fns`,
//! `config_options`, `mod_map`) keyed by `id` and checked against `hash`.

use crate::Name;

/// A Lua closure from the data, by identity. `id` is the dotted path from the
/// dump root (`data.skills.Barrage.preDamageFunc`, `ConfigOptions[6].apply`,
/// `data.mapMods.AffixData.Armoured.apply`).
#[derive(Debug, Clone, serde::Deserialize, serde::Serialize, PartialEq, Eq, Hash)]
#[serde(deny_unknown_fields)]
pub struct LuaFnRef {
    #[serde(rename = "$fn")]
    pub id: Name,
    pub src: String,
    pub hash: String,
}

impl LuaFnRef {
    /// `(file, first line, last line)` of `src`, or `None` for C functions (`"[C]"`).
    pub fn src_range(&self) -> Option<(&str, u32, u32)> {
        let (file, range) = self.src.rsplit_once(':')?;
        let (a, b) = range.split_once('-')?;
        Some((file, a.parse().ok()?, b.parse().ok()?))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn src_range() {
        let f = LuaFnRef {
            id: Name::from("data.skills.X.preDamageFunc"),
            src: "Data/Skills/act_dex.lua:6699-6778".into(),
            hash: String::new(),
        };
        assert_eq!(f.src_range(), Some(("Data/Skills/act_dex.lua", 6699, 6778)));
    }
}
