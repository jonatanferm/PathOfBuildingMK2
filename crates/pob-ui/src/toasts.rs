//! Transient toast-notification queue for [#225](https://github.com/jonatanferm/games/PathOfBuildingMK2/issues/225).
//!
//! Inline `status_message` already shows a single persistent banner at
//! the bottom of the menu bar. That's fine for "here's the latest
//! status" but loses earlier messages — open a build, save it, change
//! a few things, save again, and the user only sees the last save.
//!
//! Toasts fix that by queuing a few short-lived overlay messages.
//! Each carries an expiry timestamp; the render loop sweeps expired
//! entries every frame. Pure state + sweep logic lives here and is
//! unit-tested; the egui-side render is a thin paint over the result.
//!
//! Time source is `egui::Context::input(|i| i.time)` — a monotonic
//! `f64` seconds counter that works on both native and wasm without
//! the `std::time::Instant` cfg-gating the rest of `lib.rs` uses.

use crate::StatusKind;

/// One toast notification. `expires_at` is in egui's frame-time
/// seconds (monotonic f64). The render loop drops entries where
/// `expires_at <= now`.
#[derive(Debug, Clone, PartialEq)]
pub struct Toast {
    pub kind: StatusKind,
    pub message: String,
    pub expires_at: f64,
}

impl Toast {
    /// Whether this toast should still render at the given clock
    /// time. Strict `>` so an entry inserted at exactly `t` with
    /// zero-second lifetime is treated as already expired.
    #[must_use]
    pub fn is_visible(&self, now: f64) -> bool {
        self.expires_at > now
    }
}

/// Lifetime of a default toast push, in seconds. Five seconds is the
/// PoB convention — long enough to read a one-line message at a
/// glance, short enough that a burst of saves doesn't pile up a
/// permanent overlay.
pub const DEFAULT_LIFETIME_SECS: f64 = 5.0;

/// Maximum number of simultaneously-visible toasts. New pushes past
/// the cap drop the oldest entry so the overlay can't grow unbounded
/// during a high-volume status burst (e.g. an import that pushes
/// dozens of "Equipped <slot>" messages).
pub const MAX_TOASTS: usize = 6;

/// FIFO queue of live toasts. Owned by `LoadedApp` and mutated from
/// the existing status_message call sites via [`ToastQueue::push`].
#[derive(Debug, Clone, Default)]
pub struct ToastQueue {
    pub entries: Vec<Toast>,
}

impl ToastQueue {
    /// Append a toast with the default 5-second lifetime starting at
    /// `now`. If the queue is already at [`MAX_TOASTS`], drops the
    /// oldest entry before pushing.
    ///
    /// The production renderer uses [`Self::push_with_lifetime`] so
    /// the user-configurable
    /// [`crate::settings::UserSettings::toast_lifetime_secs`] is
    /// honoured per-push; tests + ad-hoc callers that want the
    /// default constant use this shorter form.
    #[allow(dead_code)]
    pub fn push(&mut self, kind: StatusKind, message: impl Into<String>, now: f64) {
        self.push_with_lifetime(kind, message, now, DEFAULT_LIFETIME_SECS);
    }

    /// Like [`Self::push`] but with a caller-chosen lifetime. Used by
    /// tests; production callers should stick to the default so the
    /// overlay's pacing stays consistent.
    pub fn push_with_lifetime(
        &mut self,
        kind: StatusKind,
        message: impl Into<String>,
        now: f64,
        lifetime_secs: f64,
    ) {
        if self.entries.len() >= MAX_TOASTS {
            self.entries.remove(0);
        }
        self.entries.push(Toast {
            kind,
            message: message.into(),
            expires_at: now + lifetime_secs,
        });
    }

    /// Drop expired entries. Called once per frame from the render
    /// path so `entries.iter()` always yields visible toasts.
    pub fn sweep(&mut self, now: f64) {
        self.entries.retain(|t| t.is_visible(now));
    }

    /// Issue #225 follow-up: dismiss the toast at `index`. Out-of-
    /// range indices are a no-op so a stale click after a sweep can't
    /// panic. Returns `true` when a toast was actually removed — the
    /// renderer reads this to short-circuit the rest of the click
    /// handler.
    pub fn dismiss(&mut self, index: usize) -> bool {
        if index < self.entries.len() {
            self.entries.remove(index);
            true
        } else {
            false
        }
    }

    /// Iterate currently-visible toasts (after the last `sweep`).
    /// Order is insertion-order so the most recent message is at the
    /// bottom of the stack — matches the convention most desktop OS
    /// toast surfaces use.
    pub fn iter(&self) -> impl Iterator<Item = &Toast> {
        self.entries.iter()
    }

    /// Number of currently-live entries. Convenience for tests +
    /// any future caller that wants to gate UI on the queue's
    /// occupancy.
    #[must_use]
    #[allow(dead_code)]
    pub fn len(&self) -> usize {
        self.entries.len()
    }

    /// Whether there are any live toasts to render. The renderer
    /// skips the overlay entirely when empty.
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn push_appends_with_correct_expiry() {
        let mut q = ToastQueue::default();
        q.push(StatusKind::Info, "hello", 10.0);
        assert_eq!(q.len(), 1);
        assert_eq!(q.entries[0].message, "hello");
        assert_eq!(q.entries[0].kind, StatusKind::Info);
        // Default lifetime + insert time.
        assert!((q.entries[0].expires_at - 15.0).abs() < f64::EPSILON);
    }

    #[test]
    fn sweep_drops_expired_keeps_visible() {
        let mut q = ToastQueue::default();
        q.push_with_lifetime(StatusKind::Info, "old", 0.0, 1.0); // expires at 1.0
        q.push_with_lifetime(StatusKind::Info, "fresh", 0.5, 10.0); // expires at 10.5
        q.sweep(5.0);
        assert_eq!(q.len(), 1);
        assert_eq!(q.entries[0].message, "fresh");
    }

    #[test]
    fn push_past_max_drops_oldest() {
        // MAX_TOASTS = 6. Push 7 and confirm the first one fell off.
        let mut q = ToastQueue::default();
        for i in 0..7 {
            q.push(StatusKind::Info, format!("msg {i}"), 0.0);
        }
        assert_eq!(q.len(), MAX_TOASTS);
        // First message dropped; second message is now the oldest.
        assert_eq!(q.entries[0].message, "msg 1");
        // Most recent push retained.
        assert_eq!(q.entries[MAX_TOASTS - 1].message, "msg 6");
    }

    #[test]
    fn is_visible_strict_greater_than() {
        // A toast pushed at t=0 with 5s lifetime expires at exactly
        // t=5. At now=5.0 it should *not* be visible — strict `>`
        // matches "5 seconds elapsed, time's up". Without strict
        // ordering an off-by-one would keep the toast for one extra
        // frame at the boundary.
        let t = Toast {
            kind: StatusKind::Info,
            message: "x".into(),
            expires_at: 5.0,
        };
        assert!(t.is_visible(4.999));
        assert!(!t.is_visible(5.0));
        assert!(!t.is_visible(5.0001));
    }

    #[test]
    fn sweep_on_empty_queue_is_noop() {
        let mut q = ToastQueue::default();
        q.sweep(1.0);
        assert!(q.is_empty());
    }

    #[test]
    fn dismiss_removes_entry_at_index_and_reports_true() {
        // Issue #225 follow-up: clicking a toast removes that toast
        // immediately so the user doesn't have to wait for its
        // expiry. Confirms the surviving entries shift up so a
        // subsequent dismiss still indexes correctly.
        let mut q = ToastQueue::default();
        q.push_with_lifetime(StatusKind::Info, "a", 0.0, 10.0);
        q.push_with_lifetime(StatusKind::Info, "b", 0.0, 10.0);
        q.push_with_lifetime(StatusKind::Info, "c", 0.0, 10.0);
        assert!(q.dismiss(1));
        let msgs: Vec<&str> = q.iter().map(|t| t.message.as_str()).collect();
        assert_eq!(msgs, vec!["a", "c"]);
        // Dismiss the now-second entry; `c` survives.
        // Wait — after removal we have [a, c], so dismissing index 0
        // drops `a` and `c` becomes the only entry.
        assert!(q.dismiss(0));
        let msgs: Vec<&str> = q.iter().map(|t| t.message.as_str()).collect();
        assert_eq!(msgs, vec!["c"]);
    }

    #[test]
    fn dismiss_out_of_range_is_noop_and_reports_false() {
        // Defensive: a stale click index after a sweep / dismiss
        // shouldn't panic. The renderer relies on the bool return
        // to decide whether to short-circuit further hit-testing.
        let mut q = ToastQueue::default();
        q.push_with_lifetime(StatusKind::Info, "only", 0.0, 10.0);
        assert!(!q.dismiss(7));
        assert!(!q.dismiss(1)); // exactly one past the end
        assert_eq!(q.len(), 1, "no entry was removed");
        // Empty queue: any index is out of range.
        let mut empty = ToastQueue::default();
        assert!(!empty.dismiss(0));
    }

    #[test]
    fn iter_yields_insertion_order() {
        // Render expects oldest-first so the stack reads
        // chronologically top-down. After a sweep removes the
        // first entry, iteration of the surviving entries still
        // preserves order.
        let mut q = ToastQueue::default();
        q.push_with_lifetime(StatusKind::Info, "a", 0.0, 1.0);
        q.push_with_lifetime(StatusKind::Info, "b", 0.0, 10.0);
        q.push_with_lifetime(StatusKind::Info, "c", 0.0, 10.0);
        q.sweep(2.0);
        let msgs: Vec<&str> = q.iter().map(|t| t.message.as_str()).collect();
        assert_eq!(msgs, vec!["b", "c"]);
    }
}
