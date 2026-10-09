//! `main.inputEvents`: a Lua array with holes (ui doc §6.7, D9). Owner: U4. UI-U0 stub.
//!
//! lua: Main.lua (`t_insert(self.inputEvents, { type = "KeyDown", key = key, doubleClick = ... })`,
//! `wipeTable(self.inputEvents)`), ControlHost.lua:49-107 (`for id, event in ipairs(inputEvents)`,
//! `inputEvents[id] = nil`), Build.lua:1216 (`#inputEvents`).

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
    slots: Vec<Option<InputEvent>>,
    /// modelled LuaJIT array-part size
    asize: usize,
}

impl InputEvents {
    /// t_insert(t, e) (appends at #t + 1)
    pub fn push(&mut self, e: InputEvent) {
        let _ = (e, &self.asize);
        todo!("U4: t_insert at #t + 1 (lj_tab_len border)")
    }
    /// t[i], 1-based
    pub fn get(&self, i: usize) -> Option<InputEvent> {
        if i == 0 {
            return None;
        }
        self.slots.get(i - 1).copied().flatten()
    }
    /// t[i] = nil
    pub fn consume(&mut self, i: usize) {
        if let Some(s) = i.checked_sub(1).and_then(|j| self.slots.get_mut(j)) {
            *s = None;
        }
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
        todo!("U4: lj_tab_len border semantics")
    }
    /// wipeTable
    pub fn wipe(&mut self) {
        todo!("U4: wipeTable (asize kept)")
    }
}
