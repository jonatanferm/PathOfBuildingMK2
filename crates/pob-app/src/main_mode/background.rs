//! Port of PathOfBuilding v2.67.2 (16de4b82d) src/Modules/Main.lua:1486-1496 (DrawBackground).
//! Owner: U17.
//!
//! @pob-file Modules/Main.lua @16de4b82d
//!
//! `main.tree[latestTreeVersion].assets.Background2/Background1` are image handles that
//! PassiveTree.lua:263-266 (`self:LoadImage(name..".png", ..., data, "ASYNC")`) creates; the UI
//! half of the tree is U23's `tree_view/assets.rs` (`TreeAssets`), which `main:LoadTree` fills
//! (CCR-0099 A, applied by U23: [`tree_background`] is the lookup).
//!
//! Props with side effects (ui doc §7.3): none.

use pob_sg::ImageHandle;
use pob_ui_core::prelude::*;

/// `main.tree[latestTreeVersion].assets.Background2` and `.Background1` (nil when absent).
///
/// CCR-0099 A: the lookup in `cx.app().trees` (`TreeAssets`, U23): `main:LoadTree` built
/// `main.tree[latestTreeVersion]` in `main:Init` (Main.lua:152).
pub fn tree_background(cx: &mut Cx) -> (Option<ImageHandle>, Option<ImageHandle>) {
    let Some(assets) = crate::tree_view::assets::latest_tree_assets(cx) else {
        lua_error!("attempt to index field '?' (a nil value)")
    };
    (
        assets.asset_handle("Background2"),
        assets.asset_handle("Background1"),
    )
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
