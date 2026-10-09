//! @pob-file Modules/ModTools.lua @16de4b82d
//! `modLib` (Modules/ModTools.lua 1-245; `mergeKeystones` 246-259 is
//! `pob_calc::modules::mod_tools_env`). Owner: CT. Signatures frozen by U0
//! (calc doc §4.3); bodies are CT's.
#![allow(non_snake_case, unused_variables)]

use crate::modifier::{Mod, ModType, Tag};
use crate::unported;
use crate::value::Value;
use crate::Name;

/// @pob Modules/ModTools.lua:1-32 h=5b927ece
/// Module header: stdlib locals and `modLib = { }` (no Rust counterpart).
const _MODULE_HEADER: () = ();

/// @pob Modules/ModTools.lua:33-67 h=106d7eaa
/// `modLib.createMod(modName, modType, modVal, ...)`: the varargs
/// `source, flags, keywordFlags, tags…` are already split by the caller.
pub fn createMod(
    modName: Name,
    modType: ModType,
    modVal: impl Into<Value>,
    source: Option<&str>,
    flags: u32,
    keywordFlags: u32,
    tags: &[Tag],
) -> Mod {
    let _ = (
        modName,
        modType,
        modVal.into(),
        source,
        flags,
        keywordFlags,
        tags,
    );
    let () = unported!("Modules/ModTools.lua:33-67");
    Mod {
        name: modName,
        r#type: modType,
        value: Value::Nil,
        flags,
        keywordFlags,
        source: None,
        tags: smallvec::SmallVec::new(),
        extra: None,
    }
}

/// @pob Modules/ModTools.lua:68-70 h=f47504bd
/// `modLib.parseMod = modParserModule.parseMod` (pob-modparser `parseMod`).
const _PARSE_MOD_ALIAS: () = ();

/// @pob Modules/ModTools.lua:72-98 h=155ea718
pub fn parseTags(line: &str) -> Vec<Tag> {
    unported!("Modules/ModTools.lua:72-98")
}

/// @pob Modules/ModTools.lua:100-119 h=161a8bac
pub fn parseFormattedSourceMod(line: &str) -> Option<Mod> {
    unported!("Modules/ModTools.lua:100-119")
}

/// @pob Modules/ModTools.lua:121-134 h=881ee494
pub fn compareModParams(a: &Mod, b: &Mod) -> bool {
    unported!("Modules/ModTools.lua:121-134")
}

/// @pob Modules/ModTools.lua:136-149 h=7408756c
/// `src` is `ModFlag` / `KeywordFlag` as `(name, value)` entries
/// (`ModFlag::ENTRIES`); Lua iterates it with `pairs` and sorts the names.
pub fn formatFlags(flags: u32, src: &[(&str, u32)]) -> String {
    unported!("Modules/ModTools.lua:136-149")
}

/// @pob Modules/ModTools.lua:151-186 h=f30d60db
pub fn formatTag(t: &Tag) -> String {
    unported!("Modules/ModTools.lua:151-186")
}

/// @pob Modules/ModTools.lua:188-194 h=954b43a1
pub fn formatTags(t: &[Tag]) -> String {
    unported!("Modules/ModTools.lua:188-194")
}

/// @pob Modules/ModTools.lua:196-225 h=27b487f0
pub fn formatValue(v: &Value) -> String {
    unported!("Modules/ModTools.lua:196-225")
}

/// @pob Modules/ModTools.lua:227-229 h=aba0488a
pub fn formatModParams(m: &Mod) -> String {
    unported!("Modules/ModTools.lua:227-229")
}

/// @pob Modules/ModTools.lua:231-233 h=22f8d9af
pub fn formatMod(m: &Mod) -> String {
    unported!("Modules/ModTools.lua:231-233")
}

/// @pob Modules/ModTools.lua:235-237 h=0322ee29
pub fn formatSourceMod(m: &Mod) -> String {
    unported!("Modules/ModTools.lua:235-237")
}

/// @pob Modules/ModTools.lua:239-245 h=4822ac80
/// Returns the re-sourced copy (ModTools.lua:239-246 mutates; §3.6: callers
/// pass a fresh copy or a mod under construction).
pub fn setSource(m: &Mod, source: &str) -> Mod {
    let _ = source;
    let () = unported!("Modules/ModTools.lua:239-245");
    m.clone()
}
