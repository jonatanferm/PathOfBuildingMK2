//! @pob-file Modules/ModParser.lua @16de4b82d
//! ModParser.lua 6994-7021: `modLib.parseMod` (the two-order pass + memo cache, seeded from
//! `data().modCache`) and `parseModCache`.
//!
//! Owner: MP0.
//!
//! The memo is per thread (one PoB Lua state per thread, like pob-calc). Its seed is
//! `Data/ModCache.lua`, which `Main.lua:136-138` copies into `modLib.parseModCache` at startup;
//! the port reads `data().modCache` as the bottom layer instead of copying 23k entries
//! (entries are never removed, so the two are the same table).
#![allow(non_snake_case, clippy::all, clippy::pedantic)]

use crate::scan::parseModPass;
use pob_gamedata::{data, Mod};
use std::cell::RefCell;

/// One `cache[line] = { modList, extra }` entry.
type CacheEntry = (Option<Vec<Mod>>, Option<String>);

thread_local! {
    /// `local cache = { }` (6994): lines parsed natively in this Lua state.
    static cache: RefCell<ahash::AHashMap<String, CacheEntry>> = RefCell::new(ahash::AHashMap::new());
}

/// The cached entry for `line`, if any (`cache[line]`): the natively parsed memo first, then
/// the ModCache seed. Returns a fresh copy (`unpack(copyTable(cache[line]))`).
fn cache_get(line: &str) -> Option<CacheEntry> {
    if let Some(e) = cache.with(|c| c.borrow().get(line).cloned()) {
        return Some(e);
    }
    let e = data().modCache.get(line)?;
    Some((
        e.mods
            .as_ref()
            .map(|v| v.iter().map(|m| (**m).clone()).collect()),
        e.extra.as_deref().map(str::to_owned),
    ))
}

/// The two passes of `parseMod` (7002-7005).
fn parse_two_orders(line: &str) -> CacheEntry {
    // L7002
    let (mut modList, mut extra) = parseModPass(line, 1);
    // L7003
    if modList.is_some() && extra.is_some() {
        // L7004
        (modList, extra) = parseModPass(line, 2);
    }
    (modList, extra)
}

/// @pob Modules/ModParser.lua:6994-7021 h=f5001218
/// `local cache, unsupported, count` and the returned module table (`modLib.parseMod`,
/// ModParser.lua 6994-7021): the two-order pass + memo cache, seeded from `data().modCache`.
/// Returns fresh copies on each call (Lua: `unpack(copyTable(cache[line]))`).
pub fn parseMod(line: &str, isComb: bool) -> (Option<Vec<Mod>>, Option<String>) {
    // L6995-6996: `unsupported`, `count` only serve the debug dump below
    let _ = isComb;
    // L7001
    if let Some(e) = cache_get(line) {
        // L7018
        return e;
    }
    // L7002-7005
    let (modList, extra) = parse_two_orders(line);
    // L7006
    cache.with(|c| {
        c.borrow_mut()
            .insert(line.to_owned(), (modList.clone(), extra.clone()));
    });
    // L7007: `if foo and …` — `foo` is an undeclared global (its assignments at 6997-6998 are
    // commented out): LUA-GLOBAL-NIL, the unsupported-line dump (7007-7016) never runs
    // L7018: return unpack(copyTable(cache[line]))
    (modList, extra)
}

/// The ModCache tier bypassed (L1 tests): the two passes of `parseMod` on `line`, without the
/// seed or the memo.
pub fn parseModNative(line: &str) -> (Option<Vec<Mod>>, Option<String>) {
    parse_two_orders(line)
}

/// `parseModCache` (7020): whether `line` is in the cache (the memo or the ModCache seed).
pub fn parseModCacheContains(line: &str) -> bool {
    cache.with(|c| c.borrow().contains_key(line)) || data().modCache.contains_key(line)
}
