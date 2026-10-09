//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/UndoHandler.lua 1-58 (ui doc §6.12).
//! Owner: U4. Verified against the real UndoHandler.lua by `tests/tier0_undo.rs` (T0).
//!
//! @pob-file Classes/UndoHandler.lua @16de4b82d
//!
//! `self.undo`/`self.redo` are Lua arrays used as stacks with the top at index 1 (`t_insert(t, 1,
//! v)`, `t_remove(t, 1)`); here `VecDeque` fronts. They never have holes, so the Lua `#`/index
//! semantics are the deque's. The two required methods (`CreateUndoState`,
//! `RestoreUndoState`) and the place the stack lives are the `UndoOps` "class".

use std::collections::VecDeque;

use crate::cx::Cx;
use crate::ids::CtrlId;

/// `self.undo`, `self.redo`, `self.modFlag` of one UndoHandler object.
pub struct UndoStack<S> {
    pub undo: VecDeque<S>,
    pub redo: VecDeque<S>,
    pub mod_flag: bool,
}

impl<S> Default for UndoStack<S> {
    /// lua: UndoHandler.lua:17-21 UndoHandler() (`modFlag` starts nil, i.e. false)
    /// @pob Classes/UndoHandler.lua:17-21 h=e4197241
    fn default() -> Self {
        UndoStack {
            undo: VecDeque::default(),
            redo: VecDeque::default(),
            mod_flag: false,
        }
    }
}

/// One UndoHandler "class": where the stack lives and the two required methods.
pub struct UndoOps<S: 'static> {
    pub stack: for<'a> fn(&'a mut Cx, CtrlId) -> &'a mut UndoStack<S>,
    /// CreateUndoState (may mutate, e.g. EditControl.lastUndoState)
    pub create: fn(&mut Cx, CtrlId) -> S,
    /// RestoreUndoState
    pub restore: fn(&mut Cx, CtrlId, S),
}

impl<S: 'static> UndoOps<S> {
    /// lua: UndoHandler.lua:25-29 ResetUndo
    /// @pob Classes/UndoHandler.lua:25-29 h=a623c54a
    pub fn reset_undo(&self, cx: &mut Cx, me: CtrlId) {
        {
            let st = (self.stack)(cx, me);
            // self.undo = wipeTable(self.undo); self.redo = wipeTable(self.redo)
            st.undo.clear();
            st.redo.clear();
        }
        // self.undo[1] = self:CreateUndoState()  (evaluated after the wipes)
        let s = (self.create)(cx, me);
        (self.stack)(cx, me).undo.push_front(s);
    }
    /// lua: UndoHandler.lua:34-41 AddUndoState
    /// @pob Classes/UndoHandler.lua:34-41 h=9c2937e4
    pub fn add_undo_state(&self, cx: &mut Cx, me: CtrlId, no_clear_redo: bool) {
        // t_insert(self.undo, 1, self:CreateUndoState())
        let s = (self.create)(cx, me);
        let st = (self.stack)(cx, me);
        st.undo.push_front(s);
        // self.undo[102] = nil
        st.undo.truncate(101);
        st.mod_flag = true;
        if !no_clear_redo {
            st.redo.clear();
        }
    }
    /// lua: UndoHandler.lua:44-50 Undo
    /// @pob Classes/UndoHandler.lua:44-50 h=f2b3b108
    pub fn undo(&self, cx: &mut Cx, me: CtrlId) {
        let prev = {
            let st = (self.stack)(cx, me);
            // if self.undo[2] then
            if st.undo.len() < 2 {
                return;
            }
            // t_insert(self.redo, 1, t_remove(self.undo, 1))
            let top = st.undo.pop_front().expect("undo[1]");
            st.redo.push_front(top);
            // t_remove(self.undo, 1) (the argument of RestoreUndoState)
            st.undo.pop_front().expect("undo[2]")
        };
        (self.restore)(cx, me, prev);
        self.add_undo_state(cx, me, true);
    }
    /// lua: UndoHandler.lua:53-58 Redo
    /// @pob Classes/UndoHandler.lua:53-58 h=5043fecf
    pub fn redo(&self, cx: &mut Cx, me: CtrlId) {
        let Some(s) = (self.stack)(cx, me).redo.pop_front() else {
            return;
        };
        (self.restore)(cx, me, s);
        self.add_undo_state(cx, me, true);
    }
}
