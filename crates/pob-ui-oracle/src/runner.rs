//! Runs one scenario or gallery on both sides and collects their traces (ui doc §11.2, §11.3).
//! Owner: U8. UI-U0 stub.

use pob_host::script::Script;
use pob_sg::trace::FrameTrace;

/// Both sides' per-frame traces for one run.
pub struct RunResult {
    pub lua: Vec<FrameTrace>,
    pub rust: Vec<FrameTrace>,
}

/// What to run.
pub enum Target {
    Scenario(Script),
    Gallery(String),
}

/// Run `target` at `screen` (1280x800@1.0 and 1920x1080@1.5 for acceptance).
pub fn run(target: &Target, screen: pob_sg::ScreenInfo) -> anyhow::Result<RunResult> {
    let _ = (target, screen);
    todo!("U8: run both sides")
}
