//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Classes/UndoHandler.lua 1-58 (ui doc §6.12).
//! Owner: U4. UI-U0 stub: the §6.12 reference bodies; U4 verifies them with T0 (`tier0_undo.rs`).

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
    /// lua: UndoHandler.lua:25-29
    pub fn reset_undo(&self, cx: &mut Cx, me: CtrlId) {
        let s = (self.create)(cx, me);
        let st = (self.stack)(cx, me);
        st.undo.clear();
        st.redo.clear();
        st.undo.push_front(s);
    }
    /// lua: UndoHandler.lua:34-41
    pub fn add_undo_state(&self, cx: &mut Cx, me: CtrlId, no_clear_redo: bool) {
        let s = (self.create)(cx, me);
        let st = (self.stack)(cx, me);
        st.undo.push_front(s);
        st.undo.truncate(101);
        st.mod_flag = true;
        if !no_clear_redo {
            st.redo.clear();
        }
    }
    /// lua: UndoHandler.lua:44-50
    pub fn undo(&self, cx: &mut Cx, me: CtrlId) {
        let prev = {
            let st = (self.stack)(cx, me);
            if st.undo.len() < 2 {
                return;
            }
            let top = st.undo.pop_front().unwrap();
            st.redo.push_front(top);
            st.undo.pop_front().unwrap()
        };
        (self.restore)(cx, me, prev);
        self.add_undo_state(cx, me, true);
    }
    /// lua: UndoHandler.lua:53-58
    pub fn redo(&self, cx: &mut Cx, me: CtrlId) {
        let Some(s) = (self.stack)(cx, me).redo.pop_front() else {
            return;
        };
        (self.restore)(cx, me, s);
        self.add_undo_state(cx, me, true);
    }
}
