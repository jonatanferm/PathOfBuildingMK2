//! The SimpleGraphic globals as Rust functions over `Rc<RefCell<Sg>>` / `Rc<RefCell<Sys>>` (ui doc
//! §11.2 step 4), with `ui_api.cpp`'s exact argument-count dispatch. Owner: U8. UI-U0 stub.
//!
//! Covered: SetDrawLayer, GetDrawLayer, SetViewport, SetDrawColor, GetDrawColor,
//! DrawImage/DrawImageQuad, DrawString, DrawStringWidth, DrawStringCursorIndex, StripEscapes,
//! NewImageHandle (Load/Unload/IsValid/IsLoading/ImageSize/SetLoadingPriority), GetScreenSize,
//! GetScreenScale, Get/SetDPIScaleOverridePercent, GetCursorPos, IsKeyDown, GetTime, Copy, Paste,
//! SetWindowTitle, OpenURL, TakeScreenshot, ConPrintf.

use std::cell::RefCell;
use std::rc::Rc;

/// Register the SG globals in `lua`.
pub fn install(
    lua: &mlua::Lua,
    sg: Rc<RefCell<pob_sg::Sg>>,
    sys: Rc<RefCell<pob_sg::Sys>>,
) -> mlua::Result<()> {
    let _ = (lua, sg, sys);
    todo!("U8: SG globals bridge")
}
