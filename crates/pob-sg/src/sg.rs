//! The SimpleGraphic drawing API as a command recorder (no GPU dependencies).
//!
//! Every behaviour below is a port of SimpleGraphic (master @ 8694a07,
//! `ui_api.cpp` for the Lua-facing argument handling and DPI conversion,
//! `engine/render/r_main.cpp` for renderer state). Summary of the model:
//!
//! - SimpleGraphic keeps one command buffer per `(layer, subLayer)` pair
//!   (`r_layer_c`), appends VIEWPORT/BIND/COLOR/QUAD commands to the current
//!   one, and at `EndFrame` sorts the layers by `(layer, subLayer)`
//!   (`layerCompFunc`) and replays each buffer in submission order. Switching
//!   layer re-emits the current viewport into the new buffer
//!   (`r_renderer_c::SetDrawLayer`). Recording one flat list of quads that each
//!   snapshot `(layer, subLayer, viewport, blend, colour, texture)` and stably
//!   sorting by `(layer, subLayer)` produces exactly the same draw sequence.
//! - Coordinates passed by Lua are *virtual* (logical) pixels; SimpleGraphic
//!   multiplies them by `VirtualScreenScaleFactor()` before recording, so
//!   [`DrawCmd`] positions are physical pixels relative to the viewport origin.
//!   PoB calls `RenderInit("DPI_AWARE")` (Launch.lua:69), so the
//!   "virtual screen" of the renderer is the physical framebuffer.

use std::sync::Arc;

use crate::color::{self, Rgba};
use crate::font::{index_utf8, Align, Font, FontSet};
use crate::images::{ImageHandle, Images, TextureId};

/// Blend modes of the shipped SimpleGraphic (`SetBlendMode("ALPHA" |
/// "PREALPHA" | "ADDITIVE")`, removed upstream on 2026-09-09; unused by PoB).
/// ALPHA = `(SRC_ALPHA, ONE_MINUS_SRC_ALPHA)`, PREALPHA = `(ONE,
/// ONE_MINUS_SRC_ALPHA)`, ADDITIVE = `(ONE, ONE)`.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub enum BlendMode {
    #[default]
    Alpha,
    PreAlpha,
    Additive,
}

/// A viewport in physical pixels (`r_viewport_s`: `lo`, `extent`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct Viewport {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

/// What a quad samples.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum TexRef {
    /// `whiteImage` (8x8 white): solid colour quads (`DrawImage(nil, ...)`).
    White,
    /// A registered image texture.
    Image(TextureId),
    /// A font atlas (face, index into its heights).
    Font { font: Font, height_idx: u8 },
}

/// One recorded quad with the state that was current when it was submitted.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct DrawCmd {
    pub layer: i32,
    pub sublayer: i32,
    pub viewport: Viewport,
    pub blend: BlendMode,
    pub color: Rgba,
    pub tex: TexRef,
    /// TL, TR, BR, BL as x,y pairs, physical pixels relative to the viewport.
    pub pos: [f32; 8],
    /// Texture coordinates for the four corners.
    pub uv: [f32; 8],
}

impl DrawCmd {
    /// SimpleGraphic's draw culling (`AdjacentMergeStrategy::ProcessCommand`,
    /// `r_drawCull` defaults to 1): a quad whose bounding box does not
    /// intersect `(0, 0)..extent` of its viewport is dropped.
    pub fn is_culled(&self) -> bool {
        let xs = [self.pos[0], self.pos[2], self.pos[4], self.pos[6]];
        let ys = [self.pos[1], self.pos[3], self.pos[5], self.pos[7]];
        let min_x = xs.iter().copied().fold(f32::INFINITY, f32::min);
        let max_x = xs.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let min_y = ys.iter().copied().fold(f32::INFINITY, f32::min);
        let max_y = ys.iter().copied().fold(f32::NEG_INFINITY, f32::max);
        let vp = self.viewport;
        !(min_x < vp.width as f32 && max_x > 0.0 && min_y < vp.height as f32 && max_y > 0.0)
    }
}

/// `ui_api.cpp` `l_DrawString` / `l_DrawStringWidth` / `l_DrawStringCursorIndex`:
/// the logical height is scaled by DPI, rounded (`lround`), then forced to be
/// at least 1 and otherwise rounded *up to an even number*.
pub fn scaled_font_height(logical: f64, dpi_scale: f32) -> i32 {
    let h = (logical * f64::from(dpi_scale)).round() as i32;
    if h <= 1 {
        1
    } else {
        (h + 1) & !1
    }
}

/// The recording renderer. One per window.
pub struct Sg {
    images: Images,
    fonts: Option<Arc<FontSet>>,
    screen_w: i32,
    screen_h: i32,
    dpi_scale: f32,
    dpi_override_percent: i32,
    layer: i32,
    sublayer: i32,
    viewport: Viewport,
    blend: BlendMode,
    color: Rgba,
    last_color: Rgba,
    clear_color: Rgba,
    cmds: Vec<DrawCmd>,
    glyph_scratch: Vec<crate::font::GlyphQuad>,
}

impl std::fmt::Debug for Sg {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Sg")
            .field("screen", &(self.screen_w, self.screen_h))
            .field("scale", &self.screen_scale())
            .field("layer", &(self.layer, self.sublayer))
            .field("viewport", &self.viewport)
            .field("cmds", &self.cmds.len())
            .finish_non_exhaustive()
    }
}

impl Sg {
    pub fn new(images: Images, fonts: Option<Arc<FontSet>>) -> Sg {
        Sg {
            images,
            fonts,
            screen_w: 1920,
            screen_h: 1080,
            dpi_scale: 1.0,
            dpi_override_percent: 0,
            layer: 0,
            sublayer: 0,
            viewport: Viewport { x: 0, y: 0, width: 1920, height: 1080 },
            blend: BlendMode::Alpha,
            color: [1.0; 4],
            last_color: [1.0; 4],
            clear_color: [0.0, 0.0, 0.0, 1.0],
            cmds: Vec::new(),
            glyph_scratch: Vec::new(),
        }
    }

    pub fn images(&self) -> &Images {
        &self.images
    }

    pub fn fonts(&self) -> Option<&Arc<FontSet>> {
        self.fonts.as_ref()
    }

    /// `r_renderer_c::BeginFrame`: layer (0, 0), full-screen viewport,
    /// white draw colour (`DrawColor()` with no argument), ALPHA blending.
    /// `width`/`height` are the framebuffer size in physical pixels and
    /// `dpi_scale` the window's content scale.
    pub fn begin_frame(&mut self, width: u32, height: u32, dpi_scale: f32) {
        self.screen_w = width as i32;
        self.screen_h = height as i32;
        self.dpi_scale = if dpi_scale > 0.0 { dpi_scale } else { 1.0 };
        self.cmds.clear();
        self.layer = 0;
        self.sublayer = 0;
        self.blend = BlendMode::Alpha;
        self.set_viewport_px(0, 0, 0, 0);
        self.color = [1.0; 4];
    }

    /// Finish the frame: the recorded quads, stably sorted by
    /// `(layer, subLayer)` (`r_renderer_c::EndFrame` / `layerCompFunc`).
    pub fn end_frame(&mut self) -> Vec<DrawCmd> {
        let mut cmds = std::mem::take(&mut self.cmds);
        cmds.sort_by_key(|c| (c.layer, c.sublayer));
        cmds
    }

    /// Commands recorded so far this frame, in submission order.
    pub fn commands(&self) -> &[DrawCmd] {
        &self.cmds
    }

    // ---- screen / DPI -------------------------------------------------

    /// `GetScreenSize()`: the renderer's virtual screen, which with
    /// `DPI_AWARE` is the physical framebuffer size (`VirtualScreenWidth`).
    pub fn screen_size(&self) -> (i32, i32) {
        (self.screen_w, self.screen_h)
    }

    /// `GetScreenScale()` = `VirtualScreenScaleFactor()`: the DPI override
    /// percentage when set (> 0), else the window content scale.
    pub fn screen_scale(&self) -> f32 {
        if self.dpi_override_percent > 0 {
            self.dpi_override_percent as f32 / 100.0
        } else {
            self.dpi_scale
        }
    }

    /// PoB's `GetVirtualScreenSize()` (Modules/Common.lua:1118).
    pub fn virtual_screen_size(&self) -> (i32, i32) {
        let s = self.screen_scale();
        if s == 1.0 {
            (self.screen_w, self.screen_h)
        } else {
            ((self.screen_w as f32 / s).floor() as i32, (self.screen_h as f32 / s).floor() as i32)
        }
    }

    pub fn set_dpi_scale_override_percent(&mut self, percent: i32) {
        self.dpi_override_percent = percent;
    }

    pub fn dpi_scale_override_percent(&self) -> i32 {
        self.dpi_override_percent
    }

    /// `SetClearColor(r, g, b[, a])`.
    pub fn set_clear_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.clear_color = [r, g, b, a];
    }

    pub fn clear_color(&self) -> Rgba {
        self.clear_color
    }

    // ---- state ----------------------------------------------------------

    /// `SetDrawLayer(layer, subLayer)` (`l_SetDrawLayer`):
    /// - `(nil, sub)` keeps the layer and sets the sublayer;
    /// - `(layer)` sets the layer with sublayer 0 (the C++ default argument);
    /// - `(layer, sub)` sets both.
    ///
    /// `(nil)` alone is a Lua error in SimpleGraphic and is ignored here.
    pub fn set_draw_layer(&mut self, layer: Option<i32>, sub: Option<i32>) {
        match (layer, sub) {
            (None, Some(s)) => self.sublayer = s,
            (Some(l), s) => {
                self.layer = l;
                self.sublayer = s.unwrap_or(0);
            }
            (None, None) => {}
        }
    }

    /// `GetDrawLayer()`: SimpleGraphic returns the current *sublayer*
    /// (`r_renderer_c::GetDrawLayer` returns `curLayer->subLayer`).
    pub fn get_draw_layer(&self) -> i32 {
        self.sublayer
    }

    /// Current `(layer, subLayer)`.
    pub fn draw_layer(&self) -> (i32, i32) {
        (self.layer, self.sublayer)
    }

    /// `SetViewport(x, y, w, h)` / `SetViewport()` (`l_SetViewport`): x/y are
    /// scaled and `lround`ed, w/h scaled and `ceil`ed; then
    /// `r_renderer_c::SetViewport` replaces width/height by the full screen
    /// when the height is 0 (x/y are kept). `None` is `SetViewport()`, i.e.
    /// `(0, 0)` and the full screen. While set, draw coordinates are relative
    /// to the viewport origin and clipped to its rectangle.
    pub fn set_viewport(&mut self, rect: Option<(f32, f32, f32, f32)>) {
        match rect {
            Some((x, y, w, h)) => {
                let s = f64::from(self.screen_scale());
                let vx = (f64::from(x) * s).round() as i32;
                let vy = (f64::from(y) * s).round() as i32;
                let vw = (f64::from(w) * s).ceil() as i32;
                let vh = (f64::from(h) * s).ceil() as i32;
                self.set_viewport_px(vx, vy, vw, vh);
            }
            None => self.set_viewport_px(0, 0, 0, 0),
        }
    }

    fn set_viewport_px(&mut self, x: i32, y: i32, mut w: i32, mut h: i32) {
        if h == 0 {
            w = self.screen_w;
            h = self.screen_h;
        }
        self.viewport = Viewport { x, y, width: w, height: h };
    }

    pub fn viewport(&self) -> Viewport {
        self.viewport
    }

    /// `SetBlendMode(mode)` of the shipped DLL (unused by PoB).
    pub fn set_blend_mode(&mut self, mode: BlendMode) {
        self.blend = mode;
    }

    /// `SetDrawColor(r, g, b[, a])`; pass `a = 1.0` for the Lua default.
    pub fn set_draw_color(&mut self, r: f32, g: f32, b: f32, a: f32) {
        self.color = [r, g, b, a];
        self.last_color = self.color;
    }

    /// `SetDrawColor("^7")` / `SetDrawColor("^xRRGGBB")`: alpha becomes 1.
    /// Returns false (and leaves the colour unchanged) where SimpleGraphic
    /// raises "invalid color escape sequence".
    pub fn set_draw_color_escape(&mut self, escape: &str) -> bool {
        match color::parse_escape_color(escape) {
            Some(c) => {
                self.color = c;
                self.last_color = c;
                true
            }
            None => false,
        }
    }

    /// `GetDrawColor()`: SimpleGraphic returns `ui->lastColor`, which is only
    /// refreshed by `SetDrawColor` and `DrawString` (not by `BeginFrame`).
    pub fn get_draw_color(&self) -> Rgba {
        self.last_color
    }

    /// The renderer's live draw colour (`drawColor`).
    pub fn current_color(&self) -> Rgba {
        self.color
    }

    // ---- drawing --------------------------------------------------------

    fn tex_for(&self, img: Option<ImageHandle>, what: &str) -> Option<TexRef> {
        match img {
            None => Some(TexRef::White),
            Some(h) => if let Some(t) = self.images.texture_of(h) { Some(TexRef::Image(t)) } else {
                // SimpleGraphic: "<what>(): image handle has no image loaded" (Lua error).
                eprintln!("pob-sg: {what}(): image handle has no image loaded");
                None
            },
        }
    }

    fn push_quad(&mut self, tex: TexRef, pos: [f32; 8], uv: [f32; 8], color: Rgba) {
        self.cmds.push(DrawCmd {
            layer: self.layer,
            sublayer: self.sublayer,
            viewport: self.viewport,
            blend: self.blend,
            color,
            tex,
            pos,
            uv,
        });
    }

    /// `DrawImage(img|nil, left, top, width, height[, tcLeft, tcTop, tcRight,
    /// tcBottom])`. Without tex coords the full image `(0,0)-(1,1)` is used.
    /// `r_renderer_c::DrawImage` turns this into the quad TL, TR, BR, BL with
    /// uvs `uv1, (uv2.s, uv1.t), uv2, (uv1.s, uv2.t)`.
    pub fn draw_image(&mut self, img: Option<ImageHandle>, x: f32, y: f32, w: f32, h: f32, tc: Option<[f32; 4]>) {
        let Some(tex) = self.tex_for(img, "DrawImage") else { return };
        let s = self.screen_scale();
        let (x0, y0, ex, ey) = (x * s, y * s, w * s, h * s);
        let [l, t, r, b] = tc.unwrap_or([0.0, 0.0, 1.0, 1.0]);
        self.push_quad(
            tex,
            [x0, y0, x0 + ex, y0, x0 + ex, y0 + ey, x0, y0 + ey],
            [l, t, r, t, r, b, l, b],
            self.color,
        );
    }

    /// `DrawImageQuad(img|nil, x1, y1, x2, y2, x3, y3, x4, y4[, s1, t1, ...
    /// s4, t4])`. Without uvs: `(0,0), (1,0), (1,1), (0,1)`.
    pub fn draw_image_quad(&mut self, img: Option<ImageHandle>, pos: [f32; 8], uv: Option<[f32; 8]>) {
        let Some(tex) = self.tex_for(img, "DrawImageQuad") else { return };
        let s = self.screen_scale();
        let p = pos.map(|v| v * s);
        let uv = uv.unwrap_or([0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        self.push_quad(tex, p, uv, self.color);
    }

    /// `DrawString(left, top, align, height, font, text)`.
    ///
    /// Inline escapes recolour the following glyphs and force alpha to 1;
    /// because SimpleGraphic passes its live `drawColor` to the font renderer,
    /// the colour after the last escape *remains the draw colour* for later
    /// draws (and is what `GetDrawColor` reports). `\n` starts a new line
    /// `height` lower. See `font::FontFace::draw_text_line` for the layout.
    pub fn draw_string(&mut self, x: f32, y: f32, align: Align, height: f32, font: Font, text: &str) {
        let s = self.screen_scale();
        let h = scaled_font_height(f64::from(height), s);
        let Some(fonts) = self.fonts.clone() else {
            // No fonts: still apply escape colours so state matches.
            apply_escapes_only(&mut self.color, text);
            self.last_color = self.color;
            return;
        };
        let (cps, _) = index_utf8(text);
        let mut col = self.color;
        let mut quads = std::mem::take(&mut self.glyph_scratch);
        quads.clear();
        fonts.face(font).draw((x * s, y * s), align, h, &mut col, &cps, (self.screen_w, self.screen_h), &mut quads);
        for q in &quads {
            self.push_quad(TexRef::Font { font, height_idx: q.height_idx as u8 }, q.pos, q.uv, q.color);
        }
        self.glyph_scratch = quads;
        self.color = col;
        self.last_color = col;
    }

    /// `DrawStringWidth(height, font, text)`: widest line in *logical* pixels
    /// (physical integer width divided by the scale; 0 for "").
    pub fn draw_string_width(&self, height: f32, font: Font, text: &str) -> f32 {
        if text.is_empty() {
            return 0.0;
        }
        let Some(fonts) = &self.fonts else { return 0.0 };
        let s = self.screen_scale();
        let h = scaled_font_height(f64::from(height), s);
        let (cps, _) = index_utf8(text);
        (f64::from(fonts.face(font).string_width(h, &cps)) / f64::from(s)) as f32
    }

    /// `DrawStringCursorIndex(height, font, text, cursorX, cursorY)`.
    ///
    /// Returns the **1-based byte index** exactly like the Lua API (PoB's
    /// EditControl etc. use it directly with `string.sub`): the renderer finds
    /// the codepoint under the point (relative to the text origin), maps it
    /// back to its UTF-8 byte offset (or the byte length when past the end),
    /// and `l_DrawStringCursorIndex` adds 1. Empty text gives 1.
    pub fn draw_string_cursor_index(&self, height: f32, font: Font, text: &str, cursor_x: f32, cursor_y: f32) -> usize {
        if text.is_empty() {
            return 1;
        }
        let Some(fonts) = &self.fonts else { return 1 };
        let s = self.screen_scale();
        let h = scaled_font_height(f64::from(height), s);
        let cx = (f64::from(cursor_x) * f64::from(s)).round() as i32;
        let cy = (f64::from(cursor_y) * f64::from(s)).round() as i32;
        let (cps, offs) = index_utf8(text);
        let idx = fonts.face(font).string_cursor_index(h, &cps, cx, cy);
        let byte = if idx < offs.len() { offs[idx] } else { text.len() };
        byte + 1
    }
}

fn apply_escapes_only(col: &mut Rgba, text: &str) {
    let b = text.as_bytes();
    let mut i = 0;
    while i < b.len() {
        let n = color::is_color_escape(&b[i..]);
        if n > 0 {
            color::read_color_escape(&b[i..], n, col);
            col[3] = 1.0;
            i += n;
        } else {
            i += 1;
        }
    }
}

/// `StripEscapes(text)`.
pub fn strip_escapes(text: &str) -> String {
    color::strip_escapes(text)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn sg() -> Sg {
        let mut sg = Sg::new(Images::new(), None);
        sg.begin_frame(800, 600, 1.0);
        sg
    }

    #[test]
    fn layer_sorting_is_stable() {
        let mut sg = sg();
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0, None); // (0,0) #0
        sg.set_draw_layer(Some(5), None);
        sg.draw_image(None, 1.0, 0.0, 1.0, 1.0, None); // (5,0) #1
        sg.set_draw_layer(None, Some(-100));
        sg.draw_image(None, 2.0, 0.0, 1.0, 1.0, None); // (5,-100) #2
        sg.set_draw_layer(Some(0), Some(0));
        sg.draw_image(None, 3.0, 0.0, 1.0, 1.0, None); // (0,0) #3
        sg.set_draw_layer(Some(0), Some(-10));
        sg.draw_image(None, 4.0, 0.0, 1.0, 1.0, None); // (0,-10) #4
        sg.set_draw_layer(Some(5), None); // sublayer resets to 0
        assert_eq!(sg.draw_layer(), (5, 0));
        sg.draw_image(None, 5.0, 0.0, 1.0, 1.0, None); // (5,0) #5
        sg.set_draw_layer(None, None); // ignored
        assert_eq!(sg.get_draw_layer(), 0);
        let cmds = sg.end_frame();
        let order: Vec<f32> = cmds.iter().map(|c| c.pos[0]).collect();
        assert_eq!(order, vec![4.0, 0.0, 3.0, 2.0, 1.0, 5.0]);
        let keys: Vec<(i32, i32)> = cmds.iter().map(|c| (c.layer, c.sublayer)).collect();
        assert_eq!(keys, vec![(0, -10), (0, 0), (0, 0), (5, -100), (5, 0), (5, 0)]);
    }

    #[test]
    fn viewport_snapshot_and_scaling() {
        let mut sg = sg();
        sg.begin_frame(1000, 500, 1.5);
        assert_eq!(sg.viewport(), Viewport { x: 0, y: 0, width: 1000, height: 500 });
        sg.set_viewport(Some((10.3, 20.5, 100.1, 50.0)));
        // lround(15.45)=15, lround(30.75)=31, ceil(150.15)=151, ceil(75)=75
        assert_eq!(sg.viewport(), Viewport { x: 15, y: 31, width: 151, height: 75 });
        sg.set_draw_color(1.0, 0.0, 0.0, 0.5);
        sg.draw_image(None, 2.0, 4.0, 10.0, 10.0, None);
        sg.set_viewport(None);
        sg.set_draw_color(0.0, 1.0, 0.0, 1.0);
        sg.draw_image(None, 2.0, 4.0, 10.0, 10.0, Some([0.0, 0.0, 2.0, 3.0]));
        // Height 0 keeps x/y but uses the full screen size.
        sg.set_viewport(Some((4.0, 4.0, 10.0, 0.0)));
        assert_eq!(sg.viewport(), Viewport { x: 6, y: 6, width: 1000, height: 500 });
        let cmds = sg.end_frame();
        assert_eq!(cmds[0].viewport, Viewport { x: 15, y: 31, width: 151, height: 75 });
        assert_eq!(cmds[0].color, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(cmds[0].pos, [3.0, 6.0, 18.0, 6.0, 18.0, 21.0, 3.0, 21.0]);
        assert_eq!(cmds[0].uv, [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        assert_eq!(cmds[1].viewport, Viewport { x: 0, y: 0, width: 1000, height: 500 });
        assert_eq!(cmds[1].uv, [0.0, 0.0, 2.0, 0.0, 2.0, 3.0, 0.0, 3.0]);
        assert_eq!(cmds[1].tex, TexRef::White);
    }

    #[test]
    fn layer_switch_keeps_viewport() {
        let mut sg = sg();
        sg.set_viewport(Some((10.0, 10.0, 50.0, 50.0)));
        sg.set_draw_layer(Some(3), Some(1));
        sg.draw_image(None, 0.0, 0.0, 5.0, 5.0, None);
        let cmds = sg.end_frame();
        assert_eq!(cmds[0].viewport, Viewport { x: 10, y: 10, width: 50, height: 50 });
    }

    #[test]
    fn draw_color_escape_and_get() {
        let mut sg = sg();
        assert!(sg.set_draw_color_escape("^xFF8000"));
        assert_eq!(sg.get_draw_color(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        assert!(!sg.set_draw_color_escape("nope"));
        sg.set_draw_color(0.2, 0.3, 0.4, 0.5);
        // DrawString escapes persist into the draw colour (fonts absent: state only).
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "a^3b");
        assert_eq!(sg.current_color(), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(sg.get_draw_color(), [0.0, 0.0, 1.0, 1.0]);
        // BeginFrame resets drawColor but not lastColor.
        sg.begin_frame(800, 600, 1.0);
        assert_eq!(sg.current_color(), [1.0; 4]);
        assert_eq!(sg.get_draw_color(), [0.0, 0.0, 1.0, 1.0]);
    }

    #[test]
    fn quad_default_uvs_and_culling() {
        let mut sg = sg();
        sg.draw_image_quad(None, [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0], None);
        sg.draw_image_quad(None, [-20.0, 0.0, -10.0, 0.0, -10.0, 10.0, -20.0, 10.0], None);
        let cmds = sg.end_frame();
        assert_eq!(cmds[0].uv, [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        assert!(!cmds[0].is_culled());
        assert!(cmds[1].is_culled());
    }

    #[test]
    fn font_height_rounding() {
        assert_eq!(scaled_font_height(0.4, 1.0), 1);
        assert_eq!(scaled_font_height(1.0, 1.0), 1);
        assert_eq!(scaled_font_height(13.0, 1.0), 14);
        assert_eq!(scaled_font_height(14.0, 1.0), 14);
        assert_eq!(scaled_font_height(14.0, 1.25), 18); // lround(17.5)=18
        assert_eq!(scaled_font_height(16.0, 1.5), 24);
        assert_eq!(scaled_font_height(15.0, 1.5), 24); // lround(22.5)=23 -> 24
    }

    #[test]
    fn unloaded_handle_draws_nothing() {
        let mut sg = sg();
        let h = sg.images().new_handle();
        sg.draw_image(Some(h), 0.0, 0.0, 1.0, 1.0, None);
        assert!(sg.end_frame().is_empty());
    }

    #[test]
    fn strings_with_fonts() {
        let Some(dir) = FontSet::locate() else {
            eprintln!("SKIP: fonts not found");
            return;
        };
        let fonts = Arc::new(FontSet::load(dir).unwrap());
        let mut sg = Sg::new(Images::new(), Some(fonts));
        sg.begin_frame(800, 600, 1.0);
        assert_eq!(sg.draw_string_width(16.0, Font::Var, "Hello"), 31.0);
        assert_eq!(sg.draw_string_width(15.0, Font::Var, "Hello"), 31.0); // 15 -> 16
        assert_eq!(sg.draw_string_width(16.0, Font::Var, ""), 0.0);
        // Cursor index is 1-based bytes: 'é' is 2 bytes.
        assert_eq!(sg.draw_string_cursor_index(16.0, Font::Var, "Hello", 10.0, 0.0), 2);
        assert_eq!(sg.draw_string_cursor_index(16.0, Font::Var, "Hello", 999.0, 0.0), 6);
        assert_eq!(sg.draw_string_cursor_index(16.0, Font::Var, "éa", 999.0, 0.0), 4);
        assert_eq!(sg.draw_string_cursor_index(16.0, Font::Var, "", 5.0, 0.0), 1);
        // DPI 2: width is physical / scale.
        sg.begin_frame(1600, 1200, 2.0);
        let w32 = sg.fonts().unwrap().face(Font::Var).string_width(32, &index_utf8("Hello").0) as f32;
        assert_eq!(sg.draw_string_width(16.0, Font::Var, "Hello"), w32 / 2.0);
        // DrawString emits one quad per visible glyph with the layer state.
        sg.set_draw_layer(Some(2), Some(7));
        sg.set_viewport(Some((10.0, 10.0, 100.0, 100.0)));
        sg.set_draw_color(0.5, 0.5, 0.5, 0.5);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "A ^1B");
        let cmds = sg.end_frame();
        assert_eq!(cmds.len(), 3); // 'A', ' ' (1px wide glyph), 'B'
        assert!(cmds.iter().all(|c| c.layer == 2 && c.sublayer == 7));
        assert_eq!(cmds[0].viewport, Viewport { x: 20, y: 20, width: 200, height: 200 });
        assert_eq!(cmds[0].color, [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(cmds[2].color, [1.0, 0.0, 0.0, 1.0]);
        assert!(matches!(cmds[0].tex, TexRef::Font { font: Font::Var, .. }));
    }
}
