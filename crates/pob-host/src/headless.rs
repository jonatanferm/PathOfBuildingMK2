//! The deterministic, windowless host (ui doc §5.2 `run_headless`). Owner: U7.
//!
//! Virtual clock (+16 ms per frame), virtual clipboard, fixture Fs, scripted Net, synchronous
//! image loads (`loader_headless`). Used by tests and by `ui-parity`.

use crate::script::Script;
use crate::App;

/// Run `script` against `app` and return one trace per frame (`app.sg().trace` is installed by
/// the host).
pub fn run_headless<A: App>(app: &mut A, script: &Script) -> Vec<pob_sg::trace::FrameTrace> {
    let _ = (app, script);
    todo!("U7: headless host")
}
