//! Boot real PoB under LuaJIT on our pob-sg (ui doc §11.2 "Boot sequence"). Owner: U8. UI-U0 stub.
//!
//! 1. Create the mlua LuaJIT VM.
//! 2. Install pob-oracle's natives (CCR for `pub fn install_natives`).
//! 3. Load the HeadlessWrapper prefix.
//! 4. Override every SG global with [`crate::sg_bridge`].
//! 5. `debug.getinfo(2, "Sl")` sets the trace `src` before each draw call.
//! 6. `dofile("Launch.lua")`, then drive launch:OnInit / key callbacks / OnFrame from a .pobscript.

use std::cell::RefCell;
use std::rc::Rc;

/// A booted PoB: the VM plus the Sg/Sys it draws through (tracing on).
pub struct LuaPob {
    pub lua: mlua::Lua,
    pub sg: Rc<RefCell<pob_sg::Sg>>,
    pub sys: Rc<RefCell<pob_sg::Sys>>,
}

/// Boot PoB from `pob_src` (PoB's `src/`) with the patches applied.
pub fn boot(pob_src: &str, sg: pob_sg::Sg, sys: pob_sg::Sys) -> anyhow::Result<LuaPob> {
    let _ = (pob_src, sg, sys);
    todo!("U8: oracle boot")
}
