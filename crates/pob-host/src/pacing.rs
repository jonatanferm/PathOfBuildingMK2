//! Frame pacing (ui doc §2 "Frame loop", §5.2 "Pacing"; `ui_main.cpp:380-445`). Owner: U7.
//!
//! Frames run continuously while the window is focused or hovered or the app wants frames
//! (`hasActiveCoroutine || hasSubscript`); otherwise the host sleeps 100 ms between frames. The
//! first 10 frames after boot always run.

use std::time::Duration;

/// Frames that always run after boot.
pub const BOOT_FRAMES: u32 = 10;
/// The idle sleep between frames.
pub const IDLE_WAIT: Duration = Duration::from_millis(100);

#[derive(Default, Debug)]
pub struct Pacing {
    frames: u32,
}

impl Pacing {
    /// Count a finished frame.
    pub fn frame_done(&mut self) {
        self.frames = self.frames.saturating_add(1);
    }

    /// `None`: run the next frame immediately (ControlFlow::Poll); `Some(d)`: wait `d` first.
    pub fn next_wait(&self, focused: bool, hovered: bool, wants_frames: bool) -> Option<Duration> {
        if self.frames < BOOT_FRAMES || focused || hovered || wants_frames {
            None
        } else {
            Some(IDLE_WAIT)
        }
    }
}
