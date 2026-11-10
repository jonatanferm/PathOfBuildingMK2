//! Unported bodies (calc doc §5.7, D9). One registry for the whole port (the
//! counters live in `pob_gamedata::stub`, so pob-gamedata and pob-modparser
//! stubs show up in the same report).

pub use pob_gamedata::stub::{hit, replay, set_replay_hook, stub_report, stub_reset, ReplayHook};

/// Unported body: logs once per site, counts the hit (`stub_report()`),
/// returns `Default` (D9), or the value of the test replay hook
/// (`set_replay_hook`, CCR-0044). `let () = unported!(…);` in statement position.
#[macro_export]
macro_rules! unported {
    ($site:literal) => {{
        $crate::stub::hit($site);
        $crate::stub::replay($site).unwrap_or_default()
    }};
}
