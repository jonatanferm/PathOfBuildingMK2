//! Issue #204: workspace-wide undo / redo.
//!
//! Generic snapshot-based undo / redo stack. Callers clone the slice of
//! state they want to be able to undo *before* mutating it and push that
//! clone via [`UndoStack::record`]. On [`UndoStack::undo`] the recorded
//! snapshot comes back and the (now-current) state is parked on the
//! redo stack so the mutation can be re-applied via
//! [`UndoStack::redo`]. Mirrors `PathOfBuilding/src/Classes/UndoHandler.lua`,
//! the tiny shared mixin every PoB tab piggybacks on for cmd+Z.
//!
//! The stack is intentionally state-agnostic: this slice (#204 slice 1)
//! wires the Tree tab to snapshot [`pob_engine::Character`]; later
//! slices can route the Items / Skills / Config tabs through the same
//! primitive (or use a per-tab stack with the same shape).

use std::collections::VecDeque;

/// Default per-stack snapshot depth. PoB's `UndoHandler` doesn't bound
/// itself (LuaJIT's GC reaps the unreferenced tail), but in Rust an
/// unbounded `VecDeque<Character>` would drift through a long editing
/// session. 100 covers a comfortable working set without obvious memory
/// growth — most editing bursts are well under 50 mutations.
pub const DEFAULT_CAPACITY: usize = 100;

#[derive(Debug, Clone)]
pub struct UndoStack<T: Clone> {
    past: VecDeque<T>,
    future: Vec<T>,
    capacity: usize,
}

impl<T: Clone> Default for UndoStack<T> {
    fn default() -> Self {
        Self::with_capacity(DEFAULT_CAPACITY)
    }
}

impl<T: Clone> UndoStack<T> {
    /// Create an empty stack with the given per-side capacity. A
    /// capacity of zero is silently bumped to 1 — a stack that would
    /// drop every snapshot is never what the caller meant.
    #[must_use]
    pub fn with_capacity(capacity: usize) -> Self {
        Self {
            past: VecDeque::new(),
            future: Vec::new(),
            capacity: capacity.max(1),
        }
    }

    /// Record a snapshot of the state *before* a mutation. Clears the
    /// redo stack — once the user diverges from a redoable timeline,
    /// the redo branch is gone (matches every text editor).
    pub fn record(&mut self, snapshot: T) {
        if self.past.len() == self.capacity {
            self.past.pop_front();
        }
        self.past.push_back(snapshot);
        self.future.clear();
    }

    /// Pop the most recent past snapshot. The caller hands `current` in
    /// so it can be parked on the redo stack — the stack itself never
    /// reads or mutates the live state. Returns `None` when there is
    /// nothing to undo.
    pub fn undo(&mut self, current: T) -> Option<T> {
        let prev = self.past.pop_back()?;
        self.future.push(current);
        Some(prev)
    }

    /// Pop the most recent future snapshot. The caller hands `current`
    /// in so it can be parked on the past stack. Returns `None` when
    /// there is nothing to redo.
    pub fn redo(&mut self, current: T) -> Option<T> {
        let next = self.future.pop()?;
        if self.past.len() == self.capacity {
            self.past.pop_front();
        }
        self.past.push_back(current);
        Some(next)
    }

    // `can_undo` / `can_redo` are used by tests and exist for the
    // menu / status-bar greying affordances that follow in later #204
    // slices. Allowed dead-code until the Edit menu lands.
    #[allow(dead_code)]
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    #[allow(dead_code)]
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Drop both past and future. Used on Build New / Open / Demo so a
    /// freshly-loaded build can't be undone back into the previous one.
    pub fn clear(&mut self) {
        self.past.clear();
        self.future.clear();
    }

    /// Convenience over `record(current.clone())` for the common
    /// "snapshot before mutation" call site. Keeps the `.clone()` noise
    /// out of the Tree-tab event handlers.
    pub fn snapshot(&mut self, current: &T) {
        self.record(current.clone());
    }

    /// Pop the last snapshot, swap it into `current`, and park the
    /// previous `current` on the redo stack. Returns `true` when the
    /// state changed — call sites use it to gate `recompute`.
    pub fn apply_undo(&mut self, current: &mut T) -> bool {
        match self.undo(current.clone()) {
            Some(prev) => {
                *current = prev;
                true
            }
            None => false,
        }
    }

    /// Mirror of [`Self::apply_undo`] for the redo branch.
    pub fn apply_redo(&mut self, current: &mut T) -> bool {
        match self.redo(current.clone()) {
            Some(next) => {
                *current = next;
                true
            }
            None => false,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn undo_after_record_returns_recorded_snapshot() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        stack.record(1);
        // Caller hands the *current* state in so the stack can park it
        // for redo. Undo returns the snapshot that was recorded.
        assert_eq!(stack.undo(2), Some(1));
    }

    #[test]
    fn undo_returns_snapshots_in_lifo_order() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        stack.record(1);
        stack.record(2);
        stack.record(3);
        assert_eq!(stack.undo(99), Some(3));
        assert_eq!(stack.undo(99), Some(2));
        assert_eq!(stack.undo(99), Some(1));
    }

    #[test]
    fn empty_stack_undo_and_redo_return_none() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        // Handing in a current state should not silently park it on
        // the redo stack — returning None means "nothing happened".
        assert_eq!(stack.undo(7), None);
        assert!(!stack.can_redo());
        assert_eq!(stack.redo(7), None);
        assert!(!stack.can_undo());
    }

    #[test]
    fn clear_drops_both_past_and_future() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        stack.record(1);
        stack.record(2);
        stack.undo(99);
        // Past has [1], future has [99]. After clear both must be empty.
        assert!(stack.can_undo());
        assert!(stack.can_redo());
        stack.clear();
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());
        assert_eq!(stack.undo(0), None);
        assert_eq!(stack.redo(0), None);
    }

    #[test]
    fn capacity_bound_drops_oldest_when_overflowing() {
        // Two-slot stack: recording three snapshots should evict the
        // first. Undoing twice yields the most recent two; a third undo
        // returns None because the oldest was dropped.
        let mut stack: UndoStack<i32> = UndoStack::with_capacity(2);
        stack.record(1);
        stack.record(2);
        stack.record(3);
        assert_eq!(stack.undo(99), Some(3));
        assert_eq!(stack.undo(99), Some(2));
        assert_eq!(stack.undo(99), None);
    }

    #[test]
    fn record_after_undo_discards_redo_branch() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        stack.record(1);
        // Undo back to before record(1) — original state was 2.
        assert_eq!(stack.undo(2), Some(1));
        // Now diverge: record a new snapshot. The previously-undone
        // 2 must no longer be reachable via redo.
        stack.record(99);
        assert_eq!(stack.redo(0), None);
    }

    #[test]
    fn apply_undo_swaps_current_into_redo_branch() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        let mut state = 1;
        stack.snapshot(&state);
        state = 2;
        // apply_undo restores the snapshot (1) and parks the current
        // state (2) on the redo branch so apply_redo can re-apply it.
        assert!(stack.apply_undo(&mut state));
        assert_eq!(state, 1);
        assert!(stack.apply_redo(&mut state));
        assert_eq!(state, 2);
    }

    #[test]
    fn apply_undo_on_empty_stack_returns_false_and_leaves_state() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        let mut state = 42;
        assert!(!stack.apply_undo(&mut state));
        assert_eq!(state, 42);
        assert!(!stack.apply_redo(&mut state));
        assert_eq!(state, 42);
    }

    #[test]
    fn snapshot_then_undo_restores_character_allocation_set() {
        // End-to-end shape: snapshot the live `Character`, mutate the
        // allocation set the way the Tree tab's click handler does,
        // then undo. The character must be byte-identical to the
        // pre-mutation state.
        use pob_engine::character::ClassRef;
        use pob_engine::Character;

        let mut character = Character::new(ClassRef::marauder(), 1);
        let mut stack: UndoStack<Character> = UndoStack::default();

        let before = character.clone();
        stack.snapshot(&character);
        character.allocated.insert(42);
        character.allocated.insert(99);
        assert!(character.allocated.contains(&42));

        assert!(stack.apply_undo(&mut character));
        assert_eq!(character.allocated, before.allocated);
        assert!(!character.allocated.contains(&42));

        // And redo restores the post-mutation state.
        assert!(stack.apply_redo(&mut character));
        assert!(character.allocated.contains(&42));
        assert!(character.allocated.contains(&99));
    }

    #[test]
    fn redo_returns_state_undone_past() {
        let mut stack: UndoStack<i32> = UndoStack::default();
        stack.record(10);
        // Mutation moved state from 10 → 20. Undo restores 10 and parks
        // 20 on the redo stack. Redo (with 10 as current) should give
        // 20 back, restoring the mutation.
        let restored = stack.undo(20);
        assert_eq!(restored, Some(10));
        assert_eq!(stack.redo(10), Some(20));
    }
}
