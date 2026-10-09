//! Determinism and order patches (ui doc §11.2). Owner: U8. UI-U0 stub.
//!
//! Each patch is a text substitution applied when the module loads; it asserts that the original
//! text is present (so upstream drift fails loudly) and states its reason. The planned set: virtual
//! clock, `math.randomseed(0)`, no update checks, fixture `GetUserPath`, synchronous image loads,
//! creation-order `__canonPairs` at ControlHost.lua:32/:110 and Main.lua:1298/:1308 (D7), control
//! markers in DrawControls, scripted Net/LaunchSubScript.

/// One source substitution.
#[derive(Clone, Copy, Debug)]
pub struct Patch {
    /// Path relative to PoB's `src/`.
    pub file: &'static str,
    /// Text that must be present exactly once.
    pub find: &'static str,
    pub replace: &'static str,
    pub reason: &'static str,
}

/// Every registered patch (U8 fills it; `// ORDER: (a)` resolutions add theirs here).
pub const PATCHES: &[Patch] = &[];

/// Apply the patches registered for `file` to its source; a missing anchor is an error.
pub fn apply(file: &str, src: &str) -> Result<String, String> {
    let _ = (file, src);
    todo!("U8: patch application")
}
