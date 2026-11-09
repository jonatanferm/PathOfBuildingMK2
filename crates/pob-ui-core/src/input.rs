//! `main.inputEvents`: a Lua array with holes (ui doc §6.7, D9). Owner: U4.
//!
//! lua: Main.lua:494-505 (`t_insert(self.inputEvents, { type = "KeyDown", key = key, doubleClick = ... })`),
//! Main.lua:404/:487 (`wipeTable(self.inputEvents)`), Launch.lua:367 (`self.main.inputEvents = { }`:
//! `*ev = InputEvents::default()`), ControlHost.lua:49-107 (`for id, event in ipairs(inputEvents)`,
//! `inputEvents[id] = nil`), Build.lua:1216 (`#inputEvents`).
//!
//! The table is `pob_lua::LjIntMap`, the calc plan's exact model of LuaJIT 2.1's `lj_tab.c`
//! integer-key paths (array part sizing, hash part, rehash, dead keys), so `#t` (`lj_tab_len`), the
//! key `t_insert` picks and the holes behave as in PoB, including after `wipeTable` (the array
//! part never shrinks: the nodes stay, nil). Property-tested against LuaJIT in
//! `tests/tier0_len.rs`.

use pob_lua::LjIntMap;
use pob_sg::Key;

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum EventKind {
    KeyDown { double_click: bool },
    KeyUp,
    Char,
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub struct InputEvent {
    pub kind: EventKind,
    pub key: Key,
}

/// main.inputEvents: a Lua array that gets holes when events are consumed.
#[derive(Default)]
pub struct InputEvents {
    /// The Lua table (`{}` when created).
    t: LjIntMap<InputEvent>,
}

impl InputEvents {
    /// t_insert(t, e) (appends at #t + 1)
    pub fn push(&mut self, e: InputEvent) {
        // lib_table.c table.insert: i = lj_tab_len(t) + 1; t[i] = e
        let i = self.t.len() as i64 + 1;
        self.t.set(i, Some(e));
    }
    /// t[i], 1-based
    pub fn get(&self, i: usize) -> Option<InputEvent> {
        self.t.get(i as i64)
    }
    /// t[i] = nil
    ///
    /// On a key that is not in the table this inserts a dead key, as LuaJIT's `lj_meta_tset`
    /// does (it can trigger a rehash, which moves the `#t` border). PoB only consumes events it
    /// has just read.
    pub fn consume(&mut self, i: usize) {
        self.t.set(i as i64, None);
    }
    /// `for i, e in ipairs(t)`: i += 1, then t[i]; stops at the first nil. Lazy, so consuming t[i]
    /// does not stop the current loop, but a LATER loop stops at that hole.
    pub fn next_ipairs(&self, i: &mut usize) -> Option<InputEvent> {
        *i += 1;
        self.get(*i)
    }
    /// `#t` with LuaJIT lj_tab_len border semantics over the modelled array part (asize never shrinks across
    /// wipe). Property-tested against LuaJIT (U4).
    pub fn lua_len(&self) -> usize {
        self.t.len()
    }
    /// wipeTable
    /// lua: Common.lua:549-557 wipeTable (the @pob marker lives on pob-calc lua_table.rs)
    pub fn wipe(&mut self) {
        self.t.wipe();
    }
}
