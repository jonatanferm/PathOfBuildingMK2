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

use pob_engine::Character;
use std::collections::VecDeque;

/// Issue #204 (slice 4): per-app undo snapshot for paths that mutate
/// more than `Character`. The tree-version-swap drops orphaned
/// allocations *and* swaps `LoadedApp.tree_version` (and the loaded
/// `tree` / `tree_view` alongside it). Snapshotting `Character` alone
/// would resurrect the dropped allocations against the *new* tree on
/// undo — the previous slice's commit message called this out as
/// needing its own slice.
///
/// Packaging both halves into a single `Clone` value lets the existing
/// `UndoStack<T>` primitive carry the whole thing through past / future
/// without per-stack ordering games. `tree_version` is the *string* the
/// in-memory `tree` was loaded from — restoring it tells the caller
/// which `passive-tree-<v>.json` to reload (the heavy `PassiveTree`
/// itself stays out of the snapshot).
#[derive(Debug, Clone)]
pub struct BuildSnapshot {
    pub character: Character,
    pub tree_version: String,
}

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

    /// Whether at least one snapshot is parked on the past stack —
    /// drives the Edit menu's "Undo" enabled state.
    pub fn can_undo(&self) -> bool {
        !self.past.is_empty()
    }

    /// Mirror of [`Self::can_undo`] for the future stack.
    pub fn can_redo(&self) -> bool {
        !self.future.is_empty()
    }

    /// Issue #204 follow-up: how many snapshots are sitting on the
    /// past stack. The Edit-menu hover-text shows "Undo (N available)"
    /// so a user about to chain undos knows how deep they can go
    /// without spamming Cmd+Z.
    #[must_use]
    pub fn undo_depth(&self) -> usize {
        self.past.len()
    }

    /// Mirror of [`Self::undo_depth`] for the future stack.
    #[must_use]
    pub fn redo_depth(&self) -> usize {
        self.future.len()
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

/// Speculative-clone guard for the "snapshot before tab::ui()" pattern.
///
/// Each tab's `ui` function takes `&mut Character` and returns a `bool`
/// indicating whether the user mutated state this frame. We need the
/// snapshot to be the *pre-mutation* state, but we only want to record
/// it when the tab signals change — otherwise every idle frame would
/// burn an undo slot. `PendingSnapshot` captures the clone up-front,
/// then either [`Self::commit`]s it onto the stack or is dropped to
/// throw it away. The clone-once-up-front cost is fine because it
/// only fires for the active tab.
pub struct PendingSnapshot<T: Clone> {
    snap: T,
}

impl<T: Clone> PendingSnapshot<T> {
    pub fn capture(current: &T) -> Self {
        Self {
            snap: current.clone(),
        }
    }

    /// Promote the captured snapshot to a real undo entry. Only call
    /// this when the tab returned `changed = true`.
    pub fn commit(self, stack: &mut UndoStack<T>) {
        stack.record(self.snap);
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn pending_snapshot_commit_pushes_captured_state() {
        // PendingSnapshot is the speculative-clone helper used at every
        // tab-call site that returns a `changed` bool. capture() clones
        // the current state up-front; commit() promotes that clone to
        // an undo entry only when the tab signals state changed. The
        // ergonomic win is that the call site doesn't have to thread
        // the clone-or-not decision through its own `changed` branch.
        let mut stack: UndoStack<i32> = UndoStack::default();
        let mut state = 5;
        let pending = PendingSnapshot::capture(&state);
        state = 10;
        pending.commit(&mut stack);
        assert_eq!(stack.undo(state), Some(5));
    }

    #[test]
    fn pending_snapshot_dropped_without_commit_does_not_record() {
        // Tabs commonly return `changed = false` (no mutation). The
        // captured snapshot must NOT land on the undo stack — otherwise
        // every idle frame would burn a slot.
        let stack: UndoStack<i32> = UndoStack::default();
        let state = 5;
        let pending = PendingSnapshot::capture(&state);
        drop(pending);
        assert!(!stack.can_undo());
    }

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
    fn pending_snapshot_round_trip_restores_config_state() {
        // Mirrors the Config-tab call site: capture pre-tab, run a
        // ConfigState mutation, commit, undo. The undo restores the
        // pre-mutation config without touching anything else.
        use pob_engine::character::ClassRef;
        use pob_engine::Character;

        let mut character = Character::new(ClassRef::marauder(), 1);
        let mut stack: UndoStack<Character> = UndoStack::default();
        character.config.enemy_fire_resist = 25;

        let pending = PendingSnapshot::capture(&character);
        character.config.enemy_fire_resist = 75;
        pending.commit(&mut stack);

        assert!(stack.apply_undo(&mut character));
        assert_eq!(character.config.enemy_fire_resist, 25);
        assert!(stack.apply_redo(&mut character));
        assert_eq!(character.config.enemy_fire_resist, 75);
    }

    #[test]
    fn pending_snapshot_round_trip_restores_skill_groups() {
        // Mirrors the Skills-tab call site: snapshot before any gem
        // mutation, commit on change, undo restores the prior gem list.
        use pob_engine::character::{ClassRef, SocketGroup};
        use pob_engine::{Character, MainSkill, QualityId};

        let mut character = Character::new(ClassRef::witch(), 1);
        let mut stack: UndoStack<Character> = UndoStack::default();

        let pending = PendingSnapshot::capture(&character);
        character.skill_groups.push(SocketGroup {
            label: "Main".into(),
            gems: vec![MainSkill {
                skill_id: "Arc".into(),
                level: 20,
                quality: 0,
                quality_id: QualityId::Default,
                enabled: true,
            }],
            main_active_skill_index: 1,
            enabled: true,
        });
        assert_eq!(character.skill_groups.len(), 1);
        pending.commit(&mut stack);

        assert!(stack.apply_undo(&mut character));
        assert_eq!(character.skill_groups.len(), 0);
        assert!(stack.apply_redo(&mut character));
        assert_eq!(character.skill_groups.len(), 1);
        assert_eq!(character.skill_groups[0].gems[0].skill_id, "Arc");
    }

    #[test]
    fn snapshot_then_undo_restores_tattoo_overrides() {
        // Issue #204 (slice 3): Tree-tab "Remove all tattoos" wipes
        // every tattoo override on the spec. The undo wired into the
        // confirm modal must restore the full map — verifying via the
        // same snapshot/undo round-trip the destructive action uses.
        use pob_engine::character::ClassRef;
        use pob_engine::Character;

        let mut character = Character::new(ClassRef::marauder(), 1);
        character
            .tattoo_overrides
            .insert(7, "+10 to Strength".into());
        character
            .tattoo_overrides
            .insert(11, "+15 to Dexterity".into());
        let mut stack: UndoStack<Character> = UndoStack::default();

        stack.snapshot(&character);
        character.tattoo_overrides.clear();
        assert!(character.tattoo_overrides.is_empty());

        assert!(stack.apply_undo(&mut character));
        assert_eq!(character.tattoo_overrides.len(), 2);
        assert_eq!(
            character.tattoo_overrides.get(&7).map(String::as_str),
            Some("+10 to Strength")
        );
        assert_eq!(
            character.tattoo_overrides.get(&11).map(String::as_str),
            Some("+15 to Dexterity")
        );
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

    #[test]
    fn build_snapshot_round_trip_restores_character_and_tree_version() {
        // Issue #204 (slice 4): the tree-version-swap path mutates
        // both `Character.allocated` (orphaned nodes are dropped) AND
        // the in-memory `tree_version` string. Snapshotting Character
        // alone — the slice-1/2/3 contract — would resurrect the
        // dropped allocations against the *new* tree on undo, which
        // is exactly the confusion slice 3's commit message called
        // out as needing its own slice.
        //
        // `BuildSnapshot` packages the two pieces of state that the
        // version-swap touches into one clone-able value so the
        // existing UndoStack<T> primitive carries the whole thing
        // through the past / future deques together.
        use pob_engine::character::ClassRef;
        use pob_engine::Character;

        let mut character = Character::new(ClassRef::marauder(), 1);
        character.allocated.insert(42);
        let mut tree_version = "3_25".to_owned();
        let mut stack: UndoStack<BuildSnapshot> = UndoStack::default();

        // Pre-swap snapshot: capture both halves.
        stack.snapshot(&BuildSnapshot {
            character: character.clone(),
            tree_version: tree_version.clone(),
        });

        // Simulate the swap: drop node 42 (not in the new tree) and
        // bump the version string.
        character.allocated.remove(&42);
        tree_version = "3_24".to_owned();

        // Undo should hand the pre-swap pair back so the caller can
        // restore both halves of LoadedApp in one go.
        let restored = stack
            .undo(BuildSnapshot {
                character: character.clone(),
                tree_version: tree_version.clone(),
            })
            .expect("snapshot was just recorded");
        assert_eq!(restored.tree_version, "3_25");
        assert!(restored.character.allocated.contains(&42));
    }

    // ─── can_undo / can_redo / undo_depth / redo_depth ───────────────────

    #[test]
    fn depth_helpers_track_stack_sizes() {
        // The Edit menu hover-text reads "(N available)" so the
        // depth helpers have to mirror the live stack sizes —
        // record/undo/redo cycles flip them deterministically.
        let mut stack: UndoStack<i32> = UndoStack::with_capacity(5);
        assert_eq!(stack.undo_depth(), 0);
        assert_eq!(stack.redo_depth(), 0);
        assert!(!stack.can_undo());
        assert!(!stack.can_redo());

        stack.record(1);
        stack.record(2);
        stack.record(3);
        assert_eq!(stack.undo_depth(), 3);
        assert_eq!(stack.redo_depth(), 0);
        assert!(stack.can_undo());
        assert!(!stack.can_redo());

        let _ = stack.undo(99).expect("3 was last past");
        // Undo pops one off past and pushes the current onto future.
        assert_eq!(stack.undo_depth(), 2);
        assert_eq!(stack.redo_depth(), 1);
        assert!(stack.can_undo());
        assert!(stack.can_redo());
    }

    #[test]
    fn record_clears_redo_depth() {
        // A divergent record (the "branch off a redoable timeline" rule)
        // must reset `redo_depth` to zero so the Edit menu doesn't
        // promise redo operations that are no longer reachable.
        let mut stack: UndoStack<i32> = UndoStack::with_capacity(5);
        stack.record(1);
        stack.record(2);
        let _ = stack.undo(3); // future now has [3]
        assert_eq!(stack.redo_depth(), 1);
        stack.record(10); // branch — must wipe future
        assert_eq!(stack.redo_depth(), 0);
        assert!(!stack.can_redo());
    }

    #[test]
    fn depth_caps_at_capacity_on_overflow() {
        // The hover-text contract is "N undos available", not "N
        // mutations recorded". Once the capacity-cap kicks in, depth
        // should reflect the live stack length, not the original push
        // count.
        let mut stack: UndoStack<i32> = UndoStack::with_capacity(2);
        stack.record(1);
        stack.record(2);
        stack.record(3); // pushes out the oldest
        assert_eq!(stack.undo_depth(), 2);
    }
}
