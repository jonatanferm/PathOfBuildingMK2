//! Frame pacing (ui doc §2 "Frame loop", §5.2 "Pacing"). Owner: U7.
//!
//! Port of PathOfBuilding-SimpleGraphic `3b1a346` `ui_main.cpp:380-445` `ui_main_c::Frame`, the
//! parts that decide whether a frame runs and whether the loop sleeps afterwards:
//!
//! ```text
//! if (!IsVisible() || conWin->IsVisible() || restartFlag || didExit) framesSinceWindowHidden = 0;
//! else if (framesSinceWindowHidden <= 10) framesSinceWindowHidden++;
//! else if (!IsActive() && !IsCursorOverWindow() && !hasActiveCoroutine && !hasSubscript) {
//!     Sleep(100); return;                                   // frame skipped
//! }
//! ... BeginFrame, subscripts, OnFrame, EndFrame ...
//! if (!IsActive() && !hasActiveCoroutine && !hasSubscript) Sleep(100);
//! ```
//!
//! So the first 11 frames after boot always run (the counter is tested with `<= 10` before the
//! increment; ui doc §2 says "10"), and an unfocused window sleeps 100 ms after every frame even
//! while hovered. The window is always visible for PoB (`GLFW_VISIBLE` is not affected by
//! minimising), so the reset branch is not modelled.

use std::time::Duration;

/// `framesSinceWindowHidden <= 10`: frames that always run after boot (11 of them).
pub const BOOT_FRAMES: u32 = 10;
/// The idle sleep (`sys->Sleep(100)`).
pub const IDLE_WAIT: Duration = Duration::from_millis(100);

/// `framesSinceWindowHidden`.
#[derive(Default, Debug)]
pub struct Pacing {
    frames: u32,
}

impl Pacing {
    /// lua: ui_main.cpp:384-399 (head of `Frame`). `true`: run this frame; `false`: skip it and
    /// sleep [`IDLE_WAIT`]. `active` = focused, `cursor_over` = cursor in the window, `wants` =
    /// `hasActiveCoroutine || hasSubscript`.
    pub fn begin_frame(&mut self, active: bool, cursor_over: bool, wants: bool) -> bool {
        if self.frames <= BOOT_FRAMES {
            self.frames += 1;
            true
        } else {
            active || cursor_over || wants
        }
    }

    /// lua: ui_main.cpp:437-440 (tail of `Frame`): the sleep after a frame that ran.
    pub fn sleep_after(active: bool, wants: bool) -> Option<Duration> {
        if !active && !wants {
            Some(IDLE_WAIT)
        } else {
            None
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn boot_frames_always_run() {
        let mut p = Pacing::default();
        for i in 0..11 {
            assert!(p.begin_frame(false, false, false), "boot frame {i}");
        }
        // the 12th frame is subject to the idle test
        assert!(!p.begin_frame(false, false, false));
        assert!(!p.begin_frame(false, false, false));
    }

    #[test]
    fn idle_rules() {
        let mut p = Pacing::default();
        for _ in 0..11 {
            p.begin_frame(true, true, false);
        }
        assert!(p.begin_frame(true, false, false)); // focused
        assert!(p.begin_frame(false, true, false)); // hovered
        assert!(p.begin_frame(false, false, true)); // coroutine / subscript
        assert!(!p.begin_frame(false, false, false));
        // the post-frame sleep ignores the hover state
        assert_eq!(Pacing::sleep_after(false, false), Some(IDLE_WAIT));
        assert_eq!(Pacing::sleep_after(true, false), None);
        assert_eq!(Pacing::sleep_after(false, true), None);
        assert_eq!(IDLE_WAIT, Duration::from_millis(100));
    }
}
