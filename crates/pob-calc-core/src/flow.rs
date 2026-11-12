//! Segment control flow (calc doc §5.7, §7.2).

/// What a segment of a split giant function tells its orchestrator.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub enum Flow {
    /// Fall through to the next segment.
    #[default]
    Normal,
    /// A top-level `return` (CalcOffence.lua:371).
    Return,
    /// `return calcs.initEnv(build, mode, override, specEnv)` (initEnv only,
    /// CalcSetup.lua:1433, 1923).
    Restart,
}

/// Run a segment and propagate a top-level `return`.
#[macro_export]
macro_rules! seg {
    ($e:expr) => {
        match $e {
            $crate::flow::Flow::Normal => {}
            $crate::flow::Flow::Return => return,
            $crate::flow::Flow::Restart => unreachable!("Restart outside initEnv"),
        }
    };
}
