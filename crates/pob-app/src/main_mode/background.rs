//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1486-1496 (DrawBackground).
//! Owner: U17.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main.tree[latestTreeVersion].assets.Background2/Background1` are image handles that
//! PassiveTree.lua:263-266 (`self:LoadImage(name..".png", ..., data, "ASYNC")`) creates. The UI
//! half of the tree (`tree_view/assets.rs`, `TreeAssets`) is U23's and has no fields yet, and
//! `main:LoadTree` is a no-op until U23 lands (CCR-0091 B), so [`tree_background`] stands in
//! (CCR-0099 A): it loads the image the Lua loads for the latest tree. Every tree version from
//! 3_19 on (`latestTreeVersion` included) has no `assets` of its own and copies
//! `TreeData/3_19/Assets.lua` (PassiveTree.lua:234-236), which has `Background2`; LoadImage finds
//! `TreeData/Background2.png` first (PassiveTree.lua:910-913), and the name does not match
//! `[OL][ri][bn][ie][tC]`, so the flags are `ASYNC`. Registration is deduplicated by
//! `(path, flags)` (ui doc §4.5), so loading it per call is a lookup; the draw ops are the Lua's.
//!
//! Props with side effects (ui doc §7.3): none.

use pob_sg::{ImageFlags, ImageHandle};
use pob_ui_core::prelude::*;

/// `main.tree[latestTreeVersion].assets.Background2` and `.Background1` (nil when absent).
///
/// CCR-0099 A: U23 replaces the body with the lookup in `cx.app().trees` (`TreeAssets`); until
/// then it is the handle PassiveTree.lua:263-266 would make for the latest tree (module notes).
pub fn tree_background(cx: &mut Cx) -> (Option<ImageHandle>, Option<ImageHandle>) {
    // CCR-0099
    let handle = ImageHandle::default();
    handle.load(&mut cx.sg, "TreeData/Background2.png", ImageFlags::ASYNC);
    (Some(handle), None)
}

/// lua: Main.lua:1486-1496 main:DrawBackground(viewPort) (sublayer -100, the tree `Background` asset).
/// @pob Modules/Main.lua:1486-1495 h=96a9b68e
pub fn draw_background(cx: &mut Cx, vp: &ViewPort) {
    // L1487
    cx.sg.set_draw_layer(None, Some(-100));
    // L1488
    cx.sg.set_draw_color(0.5, 0.5, 0.5);
    let (background2, background1) = tree_background(cx);
    // L1489
    if let Some(bg2) = background2 {
        // L1490
        cx.sg.draw_image_tc(
            Some(&bg2),
            vp.x,
            vp.y,
            vp.width,
            vp.height,
            0.0,
            0.0,
            vp.width / 100.0,
            vp.height / 100.0,
        );
    } else {
        // L1492 (`assets.Background1.handle` on a nil Background1 is the Lua index error)
        let bg1 = background1
            .unwrap_or_else(|| lua_error!("attempt to index field 'Background1' (a nil value)"));
        cx.sg.draw_image_tc(
            Some(&bg1),
            vp.x,
            vp.y,
            vp.width,
            vp.height,
            0.0,
            0.0,
            vp.width / 100.0,
            vp.height / 100.0,
        );
    }
    // L1494
    cx.sg.set_draw_layer(None, Some(0));
}
