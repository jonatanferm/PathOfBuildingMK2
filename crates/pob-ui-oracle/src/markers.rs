//! Control-path markers (ui doc §11.2 patch 7): `__tracePush(<HostClass>.<key>)` before
//! `control:Draw` and `__tracePop()` after, inside the patched DrawControls loop; Rust emits the
//! same path from `Cx::draw_controls`. Owner: U8. UI-U0 stub.

/// Register `__tracePush` / `__tracePop` / `__canonPairs` in `lua`.
pub fn install(
    lua: &mlua::Lua,
    sg: std::rc::Rc<std::cell::RefCell<pob_sg::Sg>>,
) -> mlua::Result<()> {
    let _ = (lua, sg);
    todo!("U8: trace markers")
}
