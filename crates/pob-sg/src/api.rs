//! `Sg`: the SimpleGraphic drawing API as a command recorder (ui doc §4.2). Owner: U1.
//!
//! Port of PathOfBuilding-SimpleGraphic **`3b1a346`** (the commit the shipped
//! `SimpleGraphic.dll` identifies as, `SimpleGraphic v2.5-3b1a346`): `ui_api.cpp:722-1257`
//! (argument handling, DPI conversion), `engine/render/r_main.cpp:128-276` (`r_layer_c`
//! command storage), `:1161-1224` (`BeginFrame`, `layerCompFunc`), `:1298-1302`, `:1464-1466`
//! (`EndFrame` sort and discard), `:1675-1860` (renderer draw state, DrawImage/DrawString
//! entry points) and `:1866-1899` (virtual screen); `ui_main.cpp:394-432` (`renderEnable`
//! around `OnFrame`). Every row of ui doc §2 is re-verified at that commit in
//! `docs/port/ui/sg-semantics.md`, which also lists the differences from the research copy
//! (master @ 8694a07) this file was first written against.
//!
//! Model: one persistent queue of VIEWPORT/BIND/COLOR/QUAD commands per `(layer, subLayer)`
//! (`r_layer_c`), switching layer re-emits the current viewport, every draw is
//! BIND+COLOR+QUAD, and `end_frame` sorts the queues by `(layer, subLayer)`.
//!
//! Moved from the pre-port `pob-sg/src/sg.rs` (flat `DrawCmd` list) by UI-U0 and rebuilt on the
//! §4.2 reference bodies; the moved tests are adapted to the queue model.
//!
//! DIVERGENCE notes (all invisible in PoB; details in `sg-semantics.md`):
//! - DIVERGENCE(blend): no `SetBlendMode` (PoB never calls it); the BLEND command that
//!   `3b1a346` queues on every layer switch and at frame start is only counted against the
//!   layer's command storage, not recorded (see `frame.rs`).
//! - DIVERGENCE(stack-mask): `DrawImage`/`DrawImageQuad` take no `stackIdx`/`mask` arguments
//!   (argument counts 6/7/10/11 and 10/11/18/19 at `3b1a346`); PoB never passes them.
//! - DIVERGENCE(console): `ui_main.cpp:426-427` switches to layer 10000 after `OnFrame` to draw
//!   SG's console, which PoB never opens; `end_frame` does not emulate it.
//! - ORDER: `layers` is a `Vec` in creation order, exactly SG's `layerList`; `end_frame` sorts
//!   a copy by the unique `(layer, subLayer)` key, so the result does not depend on `qsort`'s
//!   instability.

use std::rc::Rc;

use pob_lua::lua_error;

use crate::color::parse_escape_color;
use crate::font::FontSet;
use crate::frame::{digest, Cmd, Frame, LayerQueue, Quad, QuadSink};
use crate::image::{ImageHandle, ImageRegistry, TexId};
use crate::trace::{ApiCall, ImgRef, TraceRecorder};
use crate::types::{Align, Font, IRect, Num, ScreenInfo};

/// `r_layer_c::cmdStorage.resize(1ull << 23)` (r_main.cpp:172 @3b1a346): the byte capacity of
/// one layer's command storage per frame.
const CMD_STORAGE: usize = 1 << 23;
/// `sizeof(r_layerCmdViewport_s)`: `Command` (int) + `r_viewport_s` (4 ints).
const SIZE_VIEWPORT: usize = 20;
/// `sizeof(r_layerCmdBlend_s)`: `Command` + `int blendMode`.
const SIZE_BLEND: usize = 8;
/// `sizeof(r_layerCmdBind_s)`: `Command` + padding + `r_tex_c*` (x64).
const SIZE_BIND: usize = 16;
/// `sizeof(r_layerCmdColor_s)`: `Command` + `col4_t`.
const SIZE_COLOR: usize = 20;
/// `sizeof(r_layerCmdQuad_s)`: `Command` + 16 floats + `stackLayer, maskLayer`.
const SIZE_QUAD: usize = 76;

fn cmd_size(c: &Cmd) -> usize {
    match c {
        Cmd::Viewport(_) => SIZE_VIEWPORT,
        Cmd::Bind(_) => SIZE_BIND,
        Cmd::Color(_) => SIZE_COLOR,
        Cmd::Quad(_) => SIZE_QUAD,
    }
}

/// `r_layer_c::NewCommand`: a command that does not fit (`cmdEnd >= cmdStorage.size()`) is
/// silently dropped; later, smaller commands may still fit.
///
/// sg: r_main.cpp:218-228 @3b1a346
fn new_command(used: &mut usize, size: usize) -> bool {
    let cmd_end = *used + size;
    if cmd_end >= CMD_STORAGE {
        return false;
    }
    *used = cmd_end;
    true
}

/// `r_layer_c::SetViewport/Bind/Color/Quad` on one queue.
///
/// sg: r_main.cpp:230-276 @3b1a346
fn push_cmd(q: &mut LayerQueue, used: &mut usize, c: Cmd) {
    if new_command(used, cmd_size(&c)) {
        q.cmds.push(c);
    }
}

/// `r_layer_c::SetBlendMode`: only the storage it takes is modelled (DIVERGENCE(blend)).
///
/// sg: r_main.cpp:241-247 @3b1a346
fn push_blend(used: &mut usize) {
    new_command(used, SIZE_BLEND);
}

/// The font's sink over the current queue, with `NewCommand`'s capacity rule.
struct CappedSink<'a> {
    q: &'a mut LayerQueue,
    used: &'a mut usize,
}

impl QuadSink for CappedSink<'_> {
    fn bind(&mut self, t: TexId) {
        push_cmd(self.q, self.used, Cmd::Bind(t));
    }
    fn color(&mut self, c: [f32; 4]) {
        push_cmd(self.q, self.used, Cmd::Color(c));
    }
    fn quad(&mut self, q: Quad) {
        push_cmd(self.q, self.used, Cmd::Quad(q));
    }
}

/// `lua_tostring(L, i)` handed to C as `const char*`: SG sees the text only up to the first
/// NUL byte.
fn c_str(s: &str) -> &str {
    match s.find('\0') {
        Some(i) => &s[..i],
        None => s,
    }
}

/// The recording renderer. One per window.
pub struct Sg {
    screen: ScreenInfo,
    dpi_override_percent: i32,
    /// SG layerList: creation order, persistent; [0] == (0,0)
    layers: Vec<LayerQueue>,
    /// `r_layer_c::cmdCursor` of each entry of `layers` (bytes used this frame).
    layer_bytes: Vec<usize>,
    cur: usize,
    /// physical (curViewport)
    viewport: IRect,
    /// renderer drawColor (escape leak target)
    draw_color: [f32; 4],
    /// ui->lastColor (GetDrawColor)
    last_color: [f32; 4],
    /// SetDrawLayer/Viewport/Draw* outside OnFrame => lua_error
    render_enable: bool,
    pub fonts: Rc<FontSet>,
    pub images: ImageRegistry,
    pub trace: Option<TraceRecorder>,
}

/// StripEscapes(string): removes every `IsColorEscape` match, scanning bytes up to the first
/// NUL (the C string SG receives).
///
/// sg: ui_api.cpp:1259-1281 @3b1a346
pub fn strip_escapes(s: &str) -> String {
    crate::color::strip_escapes(c_str(s))
}

/// The scaled text height of DrawString / DrawStringWidth / DrawStringCursorIndex:
/// `lround(height * dpiScale)`, then 1 if `<= 1`, else rounded up to even.
///
/// sg: ui_api.cpp:1150-1157, 1190-1197, 1222-1228 @3b1a346
pub fn scaled_text_height(h: Num, dpi: f32) -> i32 {
    let v = (h * dpi as f64).round() as i32; // std::lround: half away from zero == f64::round
    if v <= 1 {
        1
    } else {
        (v + 1) & !1
    }
}

impl Sg {
    pub fn new(fonts: Rc<FontSet>, images: ImageRegistry) -> Sg {
        let screen = ScreenInfo {
            fb_w: 1920,
            fb_h: 1080,
            win_h: 1080,
            dpi_scale: 1.0,
        };
        Sg {
            screen,
            dpi_override_percent: 0,
            layers: vec![LayerQueue::new(0, 0)],
            layer_bytes: vec![0],
            cur: 0,
            viewport: IRect {
                x: 0,
                y: 0,
                w: screen.fb_w,
                h: screen.fb_h,
            },
            draw_color: [1.0; 4],
            // ui_main.h:55 `float lastColor[4] = {1.0f, 1.0f, 1.0f, 1.0f};`
            last_color: [1.0; 4],
            render_enable: false,
            fonts,
            images,
            trace: None,
        }
    }

    // ---- host-only lifecycle ----

    /// Clears queues; cur = layer (0,0); SetViewport(); DrawColor(white); pumps the image loader.
    ///
    /// sg: r_main.cpp:1199-1203 (BeginFrame), :1464-1466 (the previous EndFrame's Discard),
    /// ui_main.cpp:396-401 (`renderEnable = true` after BeginFrame) @3b1a346
    pub fn begin_frame(&mut self, screen: ScreenInfo) {
        self.screen = screen;
        for q in &mut self.layers {
            q.cmds.clear();
        }
        self.layer_bytes.fill(0);
        // curLayer = layerList[0];
        self.cur = 0;
        // SetViewport();
        self.set_viewport_phys(IRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
        // SetBlendMode(RB_ALPHA);
        push_blend(&mut self.layer_bytes[self.cur]);
        // DrawColor();
        self.draw_color = [1.0; 4];
        self.render_enable = true;
        self.images.pump();
        if let Some(t) = &mut self.trace {
            t.begin_frame(screen);
        }
    }

    /// Sorted non-empty queues + texture uploads/releases + XXH3 digest.
    ///
    /// sg: ui_main.cpp:421 (`renderEnable = false`), r_main.cpp:1208-1224 (layerCompFunc),
    /// :1298-1302 (qsort) @3b1a346
    pub fn end_frame(&mut self) -> Frame {
        self.render_enable = false;
        let mut layers: Vec<LayerQueue> = self
            .layers
            .iter_mut()
            .filter(|q| !q.cmds.is_empty())
            .map(|q| LayerQueue {
                layer: q.layer,
                sub_layer: q.sub_layer,
                cmds: std::mem::take(&mut q.cmds),
            })
            .collect();
        // r_main.cpp:1302 qsort by (layer, subLayer); keys are unique, so the order is deterministic.
        layers.sort_by_key(|q| (q.layer, q.sub_layer));
        let digest = digest(&layers);
        if let Some(t) = &mut self.trace {
            t.end_frame();
        }
        Frame {
            screen: self.screen,
            layers,
            uploads: self.images.take_uploads(),
            releases: self.images.take_releases(),
            digest,
        }
    }

    // ---- screen / DPI ----

    /// VirtualScreenScaleFactor: the override percentage when set, else the OS content scale
    /// (PoB always runs `RenderInit("DPI_AWARE")`, Launch.lua:69).
    ///
    /// sg: r_main.cpp:1876-1884 @3b1a346
    #[inline]
    pub fn scale(&self) -> f32 {
        if self.dpi_override_percent > 0 {
            self.dpi_override_percent as f32 / 100.0
        } else {
            self.screen.dpi_scale
        }
    }
    /// VirtualScreenWidth (DPI_AWARE: `vid.fbSize[0]`). sg: r_main.cpp:1866-1869 @3b1a346
    #[inline]
    fn vsw(&self) -> i32 {
        self.screen.fb_w
    }
    /// VirtualScreenHeight (DPI_AWARE: `vid.fbSize[1]`). sg: r_main.cpp:1871-1874 @3b1a346
    #[inline]
    fn vsh(&self) -> i32 {
        self.screen.fb_h
    }

    /// GetScreenSize: (fb_w, fb_h).
    ///
    /// sg: ui_api.cpp:722-728 @3b1a346
    pub fn get_screen_size(&self) -> (i32, i32) {
        (self.vsw(), self.vsh())
    }
    /// GetScreenScale: VirtualScreenScaleFactor.
    ///
    /// sg: ui_api.cpp:730-735 @3b1a346
    pub fn get_screen_scale(&self) -> f32 {
        self.scale()
    }
    /// SetDPIScaleOverridePercent(percent).
    ///
    /// sg: ui_api.cpp:1239-1249, r_main.cpp:1886-1888 @3b1a346
    pub fn set_dpi_scale_override_percent(&mut self, percent: i32) {
        self.dpi_override_percent = percent;
    }
    /// GetDPIScaleOverridePercent().
    ///
    /// sg: ui_api.cpp:1251-1257, r_main.cpp:1890-1892 @3b1a346
    pub fn get_dpi_scale_override_percent(&self) -> i32 {
        self.dpi_override_percent
    }
    /// Common.lua:1118 GetVirtualScreenSize (Lua helper, here for convenience)
    ///
    /// lua: Common.lua:1118-1126
    pub fn get_virtual_screen_size(&self) -> (Num, Num) {
        let (mut width, mut height) = {
            let (w, h) = self.get_screen_size();
            (w as f64, h as f64)
        };
        // local scale = GetScreenScale and GetScreenScale() or 1.0 (lua_pushnumber of a float)
        let scale = self.get_screen_scale() as f64;
        if scale != 1.0 {
            width = (width / scale).floor();
            height = (height / scale).floor();
        }
        (width, height)
    }

    /// The current physical viewport (`curViewport`).
    pub fn viewport(&self) -> IRect {
        self.viewport
    }

    /// The renderer's live draw colour (`drawColor`).
    pub fn current_draw_color(&self) -> [f32; 4] {
        self.draw_color
    }

    // ---- trace ----

    #[track_caller]
    fn rec(&mut self, f: impl FnOnce() -> ApiCall) {
        if let Some(t) = &mut self.trace {
            t.record(f(), std::panic::Location::caller());
        }
    }

    fn img_ref(&self, img: Option<&ImageHandle>) -> Option<ImgRef> {
        let t = img?.tex()?;
        self.images.source(t).map(|(p, f)| ImgRef {
            path: p.to_owned(),
            flags: f,
        })
    }

    /// `ui->LAssert(L, ui->renderEnable, "<func>() called outside of OnFrame")`.
    #[track_caller]
    fn check_render(&self, func: &str) {
        if !self.render_enable {
            lua_error!("{func}() called outside of OnFrame");
        }
    }

    // ---- draw state ----

    /// SetDrawLayer({layer|nil}[, subLayer]): `SetDrawLayer(l)` means sub 0; `SetDrawLayer(nil, s)`
    /// keeps the layer (`SetDrawSubLayer`); a switch to another `(l, s)` finds or appends a
    /// persistent queue and re-emits the current viewport (and blend mode) into it.
    ///
    /// sg: ui_api.cpp:759-781, r_main.cpp:1675-1704 @3b1a346
    #[track_caller]
    pub fn set_draw_layer(&mut self, layer: Option<i32>, sub: Option<i32>) {
        self.check_render("SetDrawLayer");
        self.rec(|| ApiCall::SetDrawLayer { layer, sub });
        let (l, s) = match (layer, sub) {
            (None, Some(s)) => (self.layers[self.cur].layer, s),
            (Some(l), s) => (l, s.unwrap_or(0)),
            (None, None) => lua_error!("SetDrawLayer(): must provide subLayer if layer is nil"),
        };
        let c = &self.layers[self.cur];
        if c.layer == l && c.sub_layer == s {
            return;
        }
        let idx = self
            .layers
            .iter()
            .position(|q| q.layer == l && q.sub_layer == s)
            .unwrap_or_else(|| {
                self.layers.push(LayerQueue::new(l, s));
                self.layer_bytes.push(0);
                self.layers.len() - 1
            });
        self.cur = idx;
        // curLayer->SetViewport(&curViewport);
        let vp = self.viewport;
        push_cmd(
            &mut self.layers[idx],
            &mut self.layer_bytes[idx],
            Cmd::Viewport(vp),
        );
        // curLayer->SetBlendMode(curBlendMode);
        push_blend(&mut self.layer_bytes[idx]);
    }

    /// GetDrawLayer: returns subLayer (SG quirk)
    ///
    /// sg: ui_api.cpp:783-788, r_main.cpp:1706-1709 @3b1a346
    pub fn get_draw_layer(&self) -> i32 {
        self.layers[self.cur].sub_layer
    }

    /// SetViewport(x, y, width, height): `x, y = lround(v * dpi)`, `w, h = ceil(v * dpi)` with
    /// `v` a double and `dpi` a float (the product is a double).
    ///
    /// sg: ui_api.cpp:790-812 @3b1a346
    #[track_caller]
    pub fn set_viewport(&mut self, x: Num, y: Num, w: Num, h: Num) {
        self.check_render("SetViewport");
        self.rec(|| ApiCall::SetViewport(Some([x, y, w, h])));
        let s = self.scale() as f64; // lua_Number * float -> double
        self.set_viewport_phys(IRect {
            x: (x * s).round() as i32,
            y: (y * s).round() as i32,
            w: (w * s).ceil() as i32,
            h: (h * s).ceil() as i32,
        });
    }

    /// SetViewport()
    ///
    /// sg: ui_api.cpp:808-810 @3b1a346
    #[track_caller]
    pub fn reset_viewport(&mut self) {
        self.check_render("SetViewport");
        self.rec(|| ApiCall::SetViewport(None));
        self.set_viewport_phys(IRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
    }

    /// `r_renderer_c::SetViewport`: a zero height means full screen (x and y are kept); the
    /// command goes into the current layer only.
    ///
    /// sg: r_main.cpp:1711-1723 @3b1a346
    fn set_viewport_phys(&mut self, mut vp: IRect) {
        if vp.h == 0 {
            vp.w = self.vsw();
            vp.h = self.vsh();
        }
        self.viewport = vp;
        push_cmd(
            &mut self.layers[self.cur],
            &mut self.layer_bytes[self.cur],
            Cmd::Viewport(vp),
        );
    }

    /// SetDrawColor(r, g, b): a = 1
    ///
    /// sg: ui_api.cpp:851-861 (`color[3] = 1.0` when alpha is absent or nil) @3b1a346
    #[track_caller]
    pub fn set_draw_color(&mut self, r: Num, g: Num, b: Num) {
        self.set_draw_color_a(r, g, b, 1.0);
    }

    /// SetDrawColor(r, g, b, a): `(float)` of each component; updates `drawColor` and
    /// `lastColor`.
    ///
    /// sg: ui_api.cpp:826-874, r_main.cpp:1731-1742, :1752-1758 @3b1a346
    #[track_caller]
    pub fn set_draw_color_a(&mut self, r: Num, g: Num, b: Num, a: Num) {
        self.check_render("SetDrawColor");
        self.rec(|| ApiCall::SetDrawColor([r, g, b, a]));
        let c = [r as f32, g as f32, b as f32, a as f32];
        self.draw_color = c;
        self.last_color = c;
    }

    /// SetDrawColor("^7") / ("^xRRGGBB"): alpha forced to 1; invalid => lua_error.
    ///
    /// sg: ui_api.cpp:834-840, common.cpp:201-218 (IsColorEscape), :256-274 (ReadColorEscape)
    /// @3b1a346
    #[track_caller]
    pub fn set_draw_color_esc(&mut self, esc: &str) {
        self.check_render("SetDrawColor");
        self.rec(|| ApiCall::SetDrawColorEsc(esc.to_owned()));
        let c = parse_escape_color(c_str(esc)).unwrap_or_else(|| {
            lua_error!("SetDrawColor() argument 1: invalid color escape sequence")
        });
        self.draw_color = c;
        self.last_color = c;
    }

    /// GetDrawColor: `lastColor`, updated by SetDrawColor and after every DrawString.
    ///
    /// sg: ui_api.cpp:876-887 @3b1a346
    pub fn get_draw_color(&self) -> [f32; 4] {
        self.last_color
    }

    // ---- drawing ----

    /// `r_renderer_c::DrawImageQuad`: BIND (the image or `@white`) + COLOR(drawColor) + QUAD,
    /// every call, no deduplication. A handle with nothing loaded is the Lua error SG raises
    /// while reading argument 1.
    ///
    /// sg: ui_api.cpp:928-935 / :1047-1054 (the handle check), r_main.cpp:1774-1791 @3b1a346
    #[track_caller]
    fn quad_phys(&mut self, img: Option<&ImageHandle>, func: &str, p: [f32; 8], uv: [f32; 8]) {
        let tex = match img {
            None => TexId::WHITE,
            Some(h) => h
                .tex()
                .unwrap_or_else(|| lua_error!("{func}(): image handle has no image loaded")),
        };
        let col = self.draw_color;
        let q = &mut self.layers[self.cur];
        let used = &mut self.layer_bytes[self.cur];
        push_cmd(q, used, Cmd::Bind(tex));
        push_cmd(q, used, Cmd::Color(col));
        push_cmd(
            q,
            used,
            Cmd::Quad(Quad {
                s: [uv[0], uv[2], uv[4], uv[6]],
                t: [uv[1], uv[3], uv[5], uv[7]],
                x: [p[0], p[2], p[4], p[6]],
                y: [p[1], p[3], p[5], p[7]],
            }),
        );
    }

    /// DrawImage(img, x, y, w, h): tex coords (0,0)-(1,1).
    ///
    /// sg: ui_api.cpp:889-1007 (argument count 5), r_main.cpp:1760-1772 @3b1a346
    #[track_caller]
    pub fn draw_image(&mut self, img: Option<&ImageHandle>, x: Num, y: Num, w: Num, h: Num) {
        self.check_render("DrawImage");
        if self.trace.is_some() {
            let r = self.img_ref(img);
            self.rec(|| ApiCall::draw_image(r, [x, y, w, h], None));
        }
        self.draw_image_phys(img, x, y, w, h, 0.0, 0.0, 1.0, 1.0);
    }

    /// DrawImage(img, x, y, w, h, tcLeft, tcTop, tcRight, tcBottom).
    ///
    /// sg: ui_api.cpp:889-1007 (argument count 9), r_main.cpp:1760-1772 @3b1a346
    #[track_caller]
    pub fn draw_image_tc(
        &mut self,
        img: Option<&ImageHandle>,
        x: Num,
        y: Num,
        w: Num,
        h: Num,
        l: Num,
        t: Num,
        r: Num,
        b: Num,
    ) {
        self.check_render("DrawImage");
        if self.trace.is_some() {
            let ir = self.img_ref(img);
            self.rec(|| ApiCall::draw_image(ir, [x, y, w, h], Some([l, t, r, b])));
        }
        self.draw_image_phys(img, x, y, w, h, l, t, r, b);
    }

    /// `xys[i] = (float)val * dpiScale` (float arithmetic), `uvs[i] = (float)val`; corners
    /// TL, TR, BR, BL with UVs `(l,t),(r,t),(r,b),(l,b)`.
    ///
    /// sg: ui_api.cpp:937-968, r_main.cpp:1760-1772 @3b1a346
    #[track_caller]
    fn draw_image_phys(
        &mut self,
        img: Option<&ImageHandle>,
        x: Num,
        y: Num,
        w: Num,
        h: Num,
        l: Num,
        t: Num,
        r: Num,
        b: Num,
    ) {
        let s = self.scale();
        let (x, y, w, h) = (x as f32 * s, y as f32 * s, w as f32 * s, h as f32 * s); // (float)val * dpiScale
        let (l, t, r, b) = (l as f32, t as f32, r as f32, b as f32);
        self.quad_phys(
            img,
            "DrawImage",
            [x, y, x + w, y, x + w, y + h, x, y + h],
            [l, t, r, t, r, b, l, b],
        );
    }

    /// DrawImageQuad(img, x1, y1, .. x4, y4): default UVs (0,0),(1,0),(1,1),(0,1).
    ///
    /// sg: ui_api.cpp:1009-1130 (argument count 9), r_main.cpp:1774-1791 @3b1a346
    #[track_caller]
    pub fn draw_image_quad(&mut self, img: Option<&ImageHandle>, p: [Num; 8]) {
        self.check_render("DrawImageQuad");
        if self.trace.is_some() {
            let r = self.img_ref(img);
            self.rec(|| ApiCall::DrawImageQuad {
                img: r,
                p,
                tc: None,
            });
        }
        self.draw_image_quad_phys(img, p, [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
    }

    /// DrawImageQuad(img, x1, y1, .. x4, y4, s1, t1, .. s4, t4).
    ///
    /// sg: ui_api.cpp:1009-1130 (argument count 17), r_main.cpp:1774-1791 @3b1a346
    #[track_caller]
    pub fn draw_image_quad_tc(&mut self, img: Option<&ImageHandle>, p: [Num; 8], tc: [Num; 8]) {
        self.check_render("DrawImageQuad");
        if self.trace.is_some() {
            let r = self.img_ref(img);
            self.rec(|| ApiCall::DrawImageQuad {
                img: r,
                p,
                tc: Some(tc),
            });
        }
        self.draw_image_quad_phys(img, p, tc);
    }

    /// sg: ui_api.cpp:1056-1089 @3b1a346
    #[track_caller]
    fn draw_image_quad_phys(&mut self, img: Option<&ImageHandle>, p: [Num; 8], tc: [Num; 8]) {
        let s = self.scale();
        let p = p.map(|v| v as f32 * s);
        let uv = tc.map(|v| v as f32);
        self.quad_phys(img, "DrawImageQuad", p, uv);
    }

    /// DrawString(left, top, align, height, font, text): position `(float)(v * dpi)` (double
    /// product), height via [`scaled_text_height`], `col = NULL` so the font draws with (and
    /// its escapes overwrite) the renderer's persistent `drawColor`; `lastColor` is read back
    /// afterwards.
    ///
    /// sg: ui_api.cpp:1132-1177, r_main.cpp:1793-1809 @3b1a346
    #[track_caller]
    pub fn draw_string(
        &mut self,
        x: Num,
        y: Num,
        align: Align,
        height: Num,
        font: Font,
        text: &str,
    ) {
        self.check_render("DrawString");
        self.rec(|| ApiCall::DrawString {
            x,
            y,
            align,
            height,
            font,
            text: text.to_owned(),
        });
        let s = self.scale();
        let pos = [(x * s as f64) as f32, (y * s as f64) as f32]; // double * float -> double -> float
        let h = scaled_text_height(height, s);
        let mut col = self.draw_color; // SG passes drawColor *by pointer*
        let fonts = self.fonts.clone();
        let (vsw, win_h) = (self.vsw(), self.screen.win_h);
        fonts.face(font).draw(
            &mut CappedSink {
                q: &mut self.layers[self.cur],
                used: &mut self.layer_bytes[self.cur],
            },
            pos,
            align,
            h,
            &mut col,
            c_str(text),
            vsw,
            win_h,
        );
        self.draw_color = col; // escapes leak (alpha forced to 1)
        self.last_color = col;
    }

    /// DrawStringWidth(height, font, text): physical width / scale (double / float).
    ///
    /// sg: ui_api.cpp:1179-1204, r_main.cpp:1833-1843 @3b1a346
    pub fn draw_string_width(&self, height: Num, font: Font, text: &str) -> Num {
        let s = self.scale();
        let h = scaled_text_height(height, s);
        let text = c_str(text);
        // r_renderer_c::DrawStringWidth: if (!*str) return 0;
        let physical_width = if text.is_empty() {
            0
        } else {
            self.fonts.face(font).string_width(h, text)
        };
        physical_width as f64 / s as f64
    }

    /// DrawStringCursorIndex(height, font, text, cursorX, cursorY): 1-based byte index into
    /// `text`, exactly what Lua receives. The cursor is `lround(v * dpi)`.
    ///
    /// sg: ui_api.cpp:1206-1237, r_main.cpp:1845-1860 @3b1a346
    pub fn draw_string_cursor_index(
        &self,
        height: Num,
        font: Font,
        text: &str,
        cx: Num,
        cy: Num,
    ) -> usize {
        let s = self.scale();
        let h = scaled_text_height(height, s);
        let cx = (cx * s as f64).round() as i32;
        let cy = (cy * s as f64).round() as i32;
        let text = c_str(text);
        // r_renderer_c::DrawStringCursorIndex: if (!*str) return 0;
        let index = if text.is_empty() {
            0
        } else {
            self.fonts.face(font).cursor_index(h, text, cx, cy)
        };
        index + 1
    }

    // ---- images ----

    /// NewImageHandle()
    pub fn new_image_handle(&mut self) -> ImageHandle {
        ImageHandle::default()
    }

    // ---- trace (oracle/tests) ----

    /// control-path markers (§11.2, patch 7)
    pub fn trace_ctl_push(&mut self, path_elem: &str) {
        if let Some(t) = &mut self.trace {
            t.ctl_push(path_elem);
        }
    }
    pub fn trace_ctl_pop(&mut self) {
        if let Some(t) = &mut self.trace {
            t.ctl_pop();
        }
    }
    /// oracle: Lua "Classes/X.lua:54" for the next call
    pub fn set_trace_src(&mut self, src: Option<String>) {
        if let Some(t) = &mut self.trace {
            t.set_src(src);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::image::tests::MemLoader;

    /// One replayed quad with the state it was drawn under (the pre-port `DrawCmd`).
    #[derive(Debug)]
    struct Q {
        layer: (i32, i32),
        vp: IRect,
        color: [f32; 4],
        tex: TexId,
        quad: Quad,
    }

    fn flat(f: &Frame) -> Vec<Q> {
        let mut out = Vec::new();
        for l in &f.layers {
            let (mut vp, mut col, mut tex) = (IRect::default(), [1.0; 4], TexId::WHITE);
            for c in &l.cmds {
                match c {
                    Cmd::Viewport(v) => vp = *v,
                    Cmd::Bind(t) => tex = *t,
                    Cmd::Color(c) => col = *c,
                    Cmd::Quad(q) => out.push(Q {
                        layer: (l.layer, l.sub_layer),
                        vp,
                        color: col,
                        tex,
                        quad: *q,
                    }),
                }
            }
        }
        out
    }

    fn screen(w: i32, h: i32, dpi: f32) -> ScreenInfo {
        ScreenInfo {
            fb_w: w,
            fb_h: h,
            win_h: h,
            dpi_scale: dpi,
        }
    }

    fn sg() -> Sg {
        let mut sg = Sg::new(
            Rc::new(FontSet::empty()),
            ImageRegistry::new(Box::new(MemLoader::default())),
        );
        sg.begin_frame(screen(800, 600, 1.0));
        sg
    }

    fn panics(f: impl FnOnce()) -> String {
        pob_lua::error::catch(f)
            .expect_err("expected a Lua error")
            .0
    }

    fn vp(x: i32, y: i32, w: i32, h: i32) -> IRect {
        IRect { x, y, w, h }
    }

    fn queue(f: &Frame, l: i32, s: i32) -> &LayerQueue {
        f.layers
            .iter()
            .find(|q| q.layer == l && q.sub_layer == s)
            .expect("layer present")
    }

    #[test]
    fn layer_sorting_is_stable() {
        let mut sg = sg();
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0); // (0,0) #0
        sg.set_draw_layer(Some(5), None);
        sg.draw_image(None, 1.0, 0.0, 1.0, 1.0); // (5,0) #1
        sg.set_draw_layer(None, Some(-100));
        sg.draw_image(None, 2.0, 0.0, 1.0, 1.0); // (5,-100) #2
        sg.set_draw_layer(Some(0), Some(0));
        sg.draw_image(None, 3.0, 0.0, 1.0, 1.0); // (0,0) #3
        sg.set_draw_layer(Some(0), Some(-10));
        sg.draw_image(None, 4.0, 0.0, 1.0, 1.0); // (0,-10) #4
        sg.set_draw_layer(Some(5), None); // sublayer resets to 0
        assert_eq!(sg.get_draw_layer(), 0);
        sg.draw_image(None, 5.0, 0.0, 1.0, 1.0); // (5,0) #5
        let f = sg.end_frame();
        let qs = flat(&f);
        let order: Vec<f32> = qs.iter().map(|c| c.quad.x[0]).collect();
        assert_eq!(order, vec![4.0, 0.0, 3.0, 2.0, 1.0, 5.0]);
        let keys: Vec<(i32, i32)> = qs.iter().map(|c| c.layer).collect();
        assert_eq!(
            keys,
            vec![(0, -10), (0, 0), (0, 0), (5, -100), (5, 0), (5, 0)]
        );
    }

    /// §2 "Layer queues": a switch re-emits the current viewport into the target queue, every
    /// time (also when returning to an existing queue); a switch to the current layer is a
    /// no-op that emits nothing.
    #[test]
    fn layer_switch_reemits_viewport() {
        let mut sg = sg();
        sg.set_viewport(10.0, 10.0, 50.0, 50.0);
        sg.set_draw_layer(Some(3), Some(1));
        sg.draw_image(None, 0.0, 0.0, 5.0, 5.0);
        sg.set_draw_layer(Some(3), Some(1)); // same layer: nothing
        sg.set_draw_layer(Some(0), None);
        sg.set_viewport(1.0, 2.0, 3.0, 4.0); // only into (0,0)
        sg.set_draw_layer(Some(3), Some(1)); // back: re-emits (1,2,3,4)
        sg.draw_image(None, 0.0, 0.0, 5.0, 5.0);
        let f = sg.end_frame();
        let q31 = queue(&f, 3, 1);
        assert_eq!(
            q31.cmds
                .iter()
                .filter(|c| matches!(c, Cmd::Viewport(_)))
                .count(),
            2
        );
        assert_eq!(q31.cmds[0], Cmd::Viewport(vp(10, 10, 50, 50)));
        assert_eq!(q31.cmds[4], Cmd::Viewport(vp(1, 2, 3, 4)));
        let q00 = queue(&f, 0, 0);
        // BeginFrame's SetViewport(), the explicit (10,10,50,50), the switch back, (1,2,3,4).
        assert_eq!(
            q00.cmds,
            vec![
                Cmd::Viewport(vp(0, 0, 800, 600)),
                Cmd::Viewport(vp(10, 10, 50, 50)),
                Cmd::Viewport(vp(10, 10, 50, 50)),
                Cmd::Viewport(vp(1, 2, 3, 4)),
            ]
        );
        let qs = flat(&f);
        assert_eq!(qs[0].vp, vp(10, 10, 50, 50));
        assert_eq!(qs[1].vp, vp(1, 2, 3, 4));
    }

    /// §2 "Layer queues": `SetDrawLayer(nil, s)` keeps the layer; `SetDrawLayer(nil)` is a Lua
    /// error; `SetDrawLayer(l)` means sub 0; queues persist in creation order across frames.
    #[test]
    fn nil_layer() {
        let mut sg = sg();
        sg.set_draw_layer(Some(7), Some(3));
        sg.set_draw_layer(None, Some(9));
        assert_eq!(sg.get_draw_layer(), 9);
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0);
        assert!(panics(|| sg.set_draw_layer(None, None))
            .contains("SetDrawLayer(): must provide subLayer if layer is nil"));
        sg.set_draw_layer(Some(7), None);
        assert_eq!(sg.get_draw_layer(), 0);
        let f = sg.end_frame();
        assert_eq!(flat(&f)[0].layer, (7, 9));
        // Persistent creation-order list: (0,0), (7,3), (7,9), (7,0).
        let keys: Vec<(i32, i32)> = sg.layers.iter().map(|q| (q.layer, q.sub_layer)).collect();
        assert_eq!(keys, vec![(0, 0), (7, 3), (7, 9), (7, 0)]);
        sg.begin_frame(screen(800, 600, 1.0));
        assert_eq!(sg.get_draw_layer(), 0); // curLayer = layerList[0]
        assert!(sg.layers.iter().skip(1).all(|q| q.cmds.is_empty()));
    }

    /// §2 "Frame start": layer (0,0), SetViewport() recorded into layer 0, white draw colour,
    /// whatever the previous frame left behind.
    #[test]
    fn frame_start() {
        let mut sg = sg();
        sg.set_draw_layer(Some(4), Some(2));
        sg.set_viewport(1.0, 1.0, 2.0, 2.0);
        sg.set_draw_color_a(0.1, 0.2, 0.3, 0.4);
        sg.end_frame();
        sg.begin_frame(screen(640, 480, 1.0));
        assert_eq!(sg.get_draw_layer(), 0);
        assert_eq!(sg.layers[sg.cur].layer, 0);
        assert_eq!(sg.viewport(), vp(0, 0, 640, 480));
        assert_eq!(sg.current_draw_color(), [1.0; 4]);
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0);
        let f = sg.end_frame();
        assert_eq!(f.layers.len(), 1);
        assert_eq!(
            f.layers[0].cmds[..2],
            [Cmd::Viewport(vp(0, 0, 640, 480)), Cmd::Bind(TexId::WHITE)]
        );
        assert_eq!(flat(&f)[0].color, [1.0; 4]);
        // VIEWPORT 20 + BLEND 8 at frame start, then one draw (112).
        assert_eq!(sg.layer_bytes[0], 28 + 112);
    }

    /// §2 "GetDrawLayer": only the subLayer.
    #[test]
    fn get_draw_layer_is_sub() {
        let mut sg = sg();
        sg.set_draw_layer(Some(42), Some(-7));
        assert_eq!(sg.get_draw_layer(), -7);
        sg.set_draw_layer(Some(-3), Some(5));
        assert_eq!(sg.get_draw_layer(), 5);
        sg.set_draw_layer(Some(100), None);
        assert_eq!(sg.get_draw_layer(), 0);
    }

    /// §2 "Viewport": `height == 0` after scaling means full screen with x and y kept;
    /// `SetViewport()` is (0,0,full,full); the command goes into the current layer only.
    #[test]
    fn viewport_height_zero() {
        let mut sg = sg();
        sg.begin_frame(screen(1000, 500, 1.5));
        assert_eq!(sg.viewport(), vp(0, 0, 1000, 500));
        sg.set_viewport(4.0, 4.0, 10.0, 0.0);
        assert_eq!(sg.viewport(), vp(6, 6, 1000, 500));
        // ceil(-0.4 * 1.5) = -0.0 -> 0 -> full screen
        sg.set_viewport(2.0, 3.0, 7.0, -0.4);
        assert_eq!(sg.viewport(), vp(3, 5, 1000, 500)); // lround(4.5)=5 (half away from zero)
                                                        // ceil(-0.7 * 1.5) = ceil(-1.05) = -1: not zero, kept
        sg.set_viewport(0.0, 0.0, 7.0, -0.7);
        assert_eq!(sg.viewport(), vp(0, 0, 11, -1));
        // a zero width alone is kept
        sg.set_viewport(1.0, 1.0, 0.0, 2.0);
        assert_eq!(sg.viewport(), vp(2, 2, 0, 3));
        sg.set_viewport(1.0, 1.0, 9.0, 9.0);
        sg.reset_viewport();
        assert_eq!(sg.viewport(), vp(0, 0, 1000, 500));
        let f = sg.end_frame();
        assert_eq!(f.layers.len(), 1);
        assert_eq!(f.layers[0].cmds.len(), 7); // begin + 6 SetViewport calls
    }

    #[test]
    fn viewport_snapshot_and_scaling() {
        let mut sg = sg();
        sg.begin_frame(screen(1000, 500, 1.5));
        sg.set_viewport(10.3, 20.5, 100.1, 50.0);
        // lround(15.45)=15, lround(30.75)=31, ceil(150.15)=151, ceil(75)=75
        assert_eq!(sg.viewport(), vp(15, 31, 151, 75));
        sg.set_draw_color_a(1.0, 0.0, 0.0, 0.5);
        sg.draw_image(None, 2.0, 4.0, 10.0, 10.0);
        sg.reset_viewport();
        sg.set_draw_color(0.0, 1.0, 0.0);
        sg.draw_image_tc(None, 2.0, 4.0, 10.0, 10.0, 0.0, 0.0, 2.0, 3.0);
        let qs = flat(&sg.end_frame());
        assert_eq!(qs[0].vp, vp(15, 31, 151, 75));
        assert_eq!(qs[0].color, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(
            (qs[0].quad.x, qs[0].quad.y),
            ([3.0, 18.0, 18.0, 3.0], [6.0, 6.0, 21.0, 21.0])
        );
        assert_eq!(
            (qs[0].quad.s, qs[0].quad.t),
            ([0.0, 1.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0])
        );
        assert_eq!(qs[1].vp, vp(0, 0, 1000, 500));
        assert_eq!(qs[1].color, [0.0, 1.0, 0.0, 1.0]);
        assert_eq!(
            (qs[1].quad.s, qs[1].quad.t),
            ([0.0, 2.0, 2.0, 0.0], [0.0, 0.0, 3.0, 3.0])
        );
        assert_eq!(qs[1].tex, TexId::WHITE);
    }

    /// §2 "DrawImage": every call appends BIND + COLOR(drawColor) + QUAD, with no
    /// deduplication of identical state; a nil image binds `@white`.
    #[test]
    fn bind_color_quad_every_draw() {
        let mut sg = sg();
        sg.set_draw_layer(Some(1), None);
        sg.set_draw_color(0.25, 0.5, 0.75);
        sg.draw_image(None, 1.0, 2.0, 3.0, 4.0);
        sg.draw_image(None, 1.0, 2.0, 3.0, 4.0);
        sg.draw_image_quad(None, [0.0, 0.0, 1.0, 0.0, 1.0, 1.0, 0.0, 1.0]);
        let f = sg.end_frame();
        let q = queue(&f, 1, 0);
        let col = [0.25, 0.5, 0.75, 1.0];
        let quad = Quad {
            s: [0.0, 1.0, 1.0, 0.0],
            t: [0.0, 0.0, 1.0, 1.0],
            x: [1.0, 4.0, 4.0, 1.0],
            y: [2.0, 2.0, 6.0, 6.0],
        };
        let unit = Quad {
            s: [0.0, 1.0, 1.0, 0.0],
            t: [0.0, 0.0, 1.0, 1.0],
            x: [0.0, 1.0, 1.0, 0.0],
            y: [0.0, 0.0, 1.0, 1.0],
        };
        assert_eq!(
            q.cmds,
            vec![
                Cmd::Viewport(vp(0, 0, 800, 600)),
                Cmd::Bind(TexId::WHITE),
                Cmd::Color(col),
                Cmd::Quad(quad),
                Cmd::Bind(TexId::WHITE),
                Cmd::Color(col),
                Cmd::Quad(quad),
                Cmd::Bind(TexId::WHITE),
                Cmd::Color(col),
                Cmd::Quad(unit),
            ]
        );
    }

    /// §2 "Escape leak": DrawString writes each escape into the persistent draw colour with
    /// alpha 1; later DrawImage calls use it; GetDrawColor follows; an empty string emits
    /// nothing and changes nothing.
    #[test]
    fn escape_leak_and_alpha_one() {
        let mut sg = sg();
        sg.set_draw_color_a(0.2, 0.3, 0.4, 0.5);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "");
        assert_eq!(sg.get_draw_color(), [0.2, 0.3, 0.4, 0.5]);
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "a^1b^xFF8000c");
        assert_eq!(sg.current_draw_color(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        assert_eq!(sg.get_draw_color(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0);
        // Text without escapes keeps the colour (alpha too).
        sg.set_draw_color_a(0.1, 0.1, 0.1, 0.25);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "plain");
        assert_eq!(sg.get_draw_color(), [0.1, 0.1, 0.1, 0.25]);
        // A line far off screen still applies its escapes.
        sg.draw_string(0.0, 5000.0, Align::Left, 16.0, Font::Var, "x^2y");
        assert_eq!(sg.current_draw_color(), [0.0, 1.0, 0.0, 1.0]);
        let f = sg.end_frame();
        let q = &f.layers[0].cmds;
        // (empty string: no COLOR) BIND, COLOR(0.2..0.5), QUAD, then the string's COLORs
        assert_eq!(q[1], Cmd::Bind(TexId::WHITE));
        assert_eq!(q[2], Cmd::Color([0.2, 0.3, 0.4, 0.5]));
        assert_eq!(q[4], Cmd::Color([0.2, 0.3, 0.4, 0.5])); // Draw(): Color(col) up front
        assert_eq!(q[5], Cmd::Color([1.0, 0.0, 0.0, 1.0]));
        assert_eq!(q[6], Cmd::Color([1.0, 128.0 / 255.0, 0.0, 1.0]));
        let qs = flat(&f);
        assert_eq!(qs[0].color, [0.2, 0.3, 0.4, 0.5]);
        assert_eq!(qs[1].color, [1.0, 128.0 / 255.0, 0.0, 1.0]);
    }

    /// §2 "GetDrawColor": `lastColor` starts white, follows SetDrawColor and every DrawString,
    /// and survives BeginFrame (which resets only `drawColor`).
    #[test]
    fn get_draw_color_after_draw_string() {
        let mut sg = Sg::new(
            Rc::new(FontSet::empty()),
            ImageRegistry::new(Box::new(MemLoader::default())),
        );
        assert_eq!(sg.get_draw_color(), [1.0; 4]); // ui_main.h:55
        sg.begin_frame(screen(800, 600, 1.0));
        sg.set_draw_color_esc("^xFF8000");
        assert_eq!(sg.get_draw_color(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        assert!(panics(|| sg.set_draw_color_esc("nope"))
            .contains("SetDrawColor() argument 1: invalid color escape sequence"));
        sg.set_draw_color_esc("^8trailing ignored");
        assert_eq!(sg.get_draw_color(), [0.7, 0.7, 0.7, 1.0]);
        sg.set_draw_color_a(0.2, 0.3, 0.4, 0.5);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "a^3b");
        assert_eq!(sg.current_draw_color(), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(sg.get_draw_color(), [0.0, 0.0, 1.0, 1.0]);
        sg.begin_frame(screen(800, 600, 1.0));
        assert_eq!(sg.current_draw_color(), [1.0; 4]);
        assert_eq!(sg.get_draw_color(), [0.0, 0.0, 1.0, 1.0]);
    }

    /// §2 "Viewport"/"DrawImage"/"Text height"/"Screen / DPI": rounding at dpi 1.0, 1.25, 1.5.
    #[test]
    fn rounding_at_dpi() {
        let mut sg = sg();
        // (dpi, SetViewport args, expected physical viewport)
        let cases: [(f32, [f64; 4], IRect); 6] = [
            (1.0, [10.5, 20.5, 30.2, 40.0], vp(11, 21, 31, 40)),
            (1.0, [-0.5, -1.5, 0.0, 1.0], vp(-1, -2, 0, 1)),
            (1.25, [10.0, 2.0, 30.0, 41.0], vp(13, 3, 38, 52)), // 12.5->13, 2.5->3, 37.5->38, 51.25->52
            (1.25, [3.0, 5.0, 8.0, 0.8], vp(4, 6, 10, 1)),      // 3.75->4, 6.25->6, 10, 1.0
            (1.5, [3.0, 5.0, 7.0, 9.0], vp(5, 8, 11, 14)), // 4.5->5, 7.5->8, 10.5->11, 13.5->14
            (1.5, [0.3, 0.1, 0.1, 0.1], vp(0, 0, 1, 1)),
        ];
        for (dpi, a, want) in cases {
            sg.begin_frame(screen(1920, 1080, dpi));
            sg.set_viewport(a[0], a[1], a[2], a[3]);
            assert_eq!(sg.viewport(), want, "dpi {dpi} {a:?}");
            sg.end_frame();
        }
        // DrawImage positions are (float)v * (float)dpi in float arithmetic.
        sg.begin_frame(screen(1920, 1080, 1.25));
        sg.draw_image(None, 0.1, 10.3, 3.3, 7.7);
        let qs = flat(&sg.end_frame());
        let (x, y, w, h) = (
            0.1f32 * 1.25f32,
            10.3f32 * 1.25f32,
            3.3f32 * 1.25f32,
            7.7f32 * 1.25f32,
        );
        assert_eq!(qs[0].quad.x, [x, x + w, x + w, x]);
        assert_eq!(qs[0].quad.y, [y, y, y + h, y + h]);
        // DPI override wins over the OS scale.
        sg.begin_frame(screen(1920, 1080, 1.0));
        sg.set_dpi_scale_override_percent(150);
        assert_eq!(sg.get_screen_scale(), 1.5);
        assert_eq!(sg.get_dpi_scale_override_percent(), 150);
        sg.set_viewport(3.0, 5.0, 7.0, 9.0);
        assert_eq!(sg.viewport(), vp(5, 8, 11, 14));
        assert_eq!(sg.get_virtual_screen_size(), (1280.0, 720.0));
        sg.set_dpi_scale_override_percent(125);
        assert_eq!(sg.get_virtual_screen_size(), (1536.0, 864.0));
        sg.set_dpi_scale_override_percent(0);
        assert_eq!(sg.get_virtual_screen_size(), (1920.0, 1080.0));
        sg.end_frame();
        // GetVirtualScreenSize floors (Common.lua:1122-1123).
        sg.begin_frame(screen(1001, 801, 1.5));
        assert_eq!(sg.get_screen_size(), (1001, 801));
        assert_eq!(sg.get_virtual_screen_size(), (667.0, 534.0));
        sg.end_frame();
    }

    #[test]
    fn font_height_rounding() {
        assert_eq!(scaled_text_height(0.4, 1.0), 1);
        assert_eq!(scaled_text_height(1.0, 1.0), 1);
        assert_eq!(scaled_text_height(-3.0, 1.0), 1);
        assert_eq!(scaled_text_height(13.0, 1.0), 14);
        assert_eq!(scaled_text_height(14.0, 1.0), 14);
        assert_eq!(scaled_text_height(14.0, 1.25), 18); // lround(17.5)=18
        assert_eq!(scaled_text_height(12.0, 1.25), 16); // 15 -> 16
        assert_eq!(scaled_text_height(16.0, 1.25), 20);
        assert_eq!(scaled_text_height(16.0, 1.5), 24);
        assert_eq!(scaled_text_height(15.0, 1.5), 24); // lround(22.5)=23 -> 24
        assert_eq!(scaled_text_height(1.0, 1.5), 2); // lround(1.5)=2
    }

    /// `renderEnable`: draw-state calls outside OnFrame (before begin_frame / after end_frame)
    /// are Lua errors; measuring and GetDrawLayer are not.
    #[test]
    fn render_enable() {
        let mut sg = Sg::new(
            Rc::new(FontSet::empty()),
            ImageRegistry::new(Box::new(MemLoader::default())),
        );
        assert!(panics(|| sg.set_draw_layer(Some(1), None))
            .contains("SetDrawLayer() called outside of OnFrame"));
        assert!(panics(|| sg.set_viewport(0.0, 0.0, 1.0, 1.0))
            .contains("SetViewport() called outside of OnFrame"));
        assert!(panics(|| sg.set_draw_color(1.0, 1.0, 1.0))
            .contains("SetDrawColor() called outside of OnFrame"));
        assert!(panics(|| sg.draw_image(None, 0.0, 0.0, 1.0, 1.0))
            .contains("DrawImage() called outside of OnFrame"));
        assert!(panics(|| sg.draw_image_quad(None, [0.0; 8]))
            .contains("DrawImageQuad() called outside of OnFrame"));
        assert!(
            panics(|| sg.draw_string(0.0, 0.0, Align::Left, 12.0, Font::Var, "x"))
                .contains("DrawString() called outside of OnFrame")
        );
        assert_eq!(sg.draw_string_width(12.0, Font::Var, "x"), 0.0);
        assert_eq!(sg.get_draw_layer(), 0);
        sg.begin_frame(screen(800, 600, 1.0));
        sg.set_draw_layer(Some(1), None);
        sg.end_frame();
        assert!(panics(|| sg.reset_viewport()).contains("SetViewport() called outside of OnFrame"));
    }

    #[test]
    fn quad_default_uvs_and_culling() {
        let mut sg = sg();
        sg.draw_image_quad(None, [0.0, 0.0, 10.0, 0.0, 10.0, 10.0, 0.0, 10.0]);
        sg.draw_image_quad(None, [-20.0, 0.0, -10.0, 0.0, -10.0, 10.0, -20.0, 10.0]);
        let qs = flat(&sg.end_frame());
        assert_eq!(
            (qs[0].quad.s, qs[0].quad.t),
            ([0.0, 1.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0])
        );
        assert!(!qs[0].quad.is_culled(qs[0].vp));
        assert!(qs[1].quad.is_culled(qs[1].vp));
    }

    #[test]
    fn unloaded_handle_is_a_lua_error() {
        let mut sg = sg();
        let h = sg.new_image_handle();
        // SG: "DrawImage(): image handle has no image loaded" (the pre-port recorder skipped the quad).
        assert!(panics(|| sg.draw_image(Some(&h), 0.0, 0.0, 1.0, 1.0))
            .contains("DrawImage(): image handle has no image loaded"));
        assert!(panics(|| sg.draw_image_quad(Some(&h), [0.0; 8]))
            .contains("DrawImageQuad(): image handle has no image loaded"));
    }

    /// `r_layer_c::NewCommand`: 8 MiB of command storage per layer per frame; a command that
    /// does not fit is dropped, and the layer recovers next frame.
    #[test]
    fn layer_command_storage_cap() {
        let mut sg = sg();
        sg.set_draw_layer(Some(1), None); // VIEWPORT 20 + BLEND 8
        sg.set_viewport(0.0, 0.0, 10.0, 10.0); // VIEWPORT 20
        let per_draw = SIZE_BIND + SIZE_COLOR + SIZE_QUAD; // 112
        let usable = CMD_STORAGE - 1; // `cmdEnd >= size` fails, so 8388607 bytes are usable
        let fit = (usable - 48) / per_draw; // whole draws that fit: 74897
        assert_eq!(fit, 74897);
        assert_eq!(usable - 48 - fit * per_draw, 95); // bytes left after them
        for _ in 0..fit + 3 {
            sg.draw_image(None, 0.0, 0.0, 1.0, 1.0);
        }
        // 95 left: draw +1 keeps BIND (79) and COLOR (59), drops QUAD; draw +2 keeps BIND (43)
        // and COLOR (23), drops QUAD; draw +3 keeps BIND (7), drops COLOR and QUAD.
        assert_eq!(sg.layer_bytes[sg.cur], usable - 7);
        let f = sg.end_frame();
        let q = queue(&f, 1, 0);
        let quads = q.cmds.iter().filter(|c| matches!(c, Cmd::Quad(_))).count();
        assert_eq!(quads, fit);
        assert_eq!(q.cmds.len(), 2 + fit * 3 + 2 + 2 + 1);
        assert!(matches!(q.cmds[q.cmds.len() - 1], Cmd::Bind(_)));
        assert!(matches!(q.cmds[q.cmds.len() - 2], Cmd::Color(_)));
        // Next frame starts empty again.
        sg.begin_frame(screen(800, 600, 1.0));
        sg.set_draw_layer(Some(1), None);
        sg.draw_image(None, 0.0, 0.0, 1.0, 1.0);
        let f = sg.end_frame();
        assert_eq!(queue(&f, 1, 0).cmds.len(), 4);
    }

    /// Strings reach SG as C strings: everything after a NUL byte is ignored.
    #[test]
    fn strings_stop_at_nul() {
        let mut sg = sg();
        assert_eq!(strip_escapes("a^1b\0^2c"), "ab");
        sg.set_draw_color(0.5, 0.5, 0.5);
        sg.draw_string(0.0, 0.0, Align::Left, 12.0, Font::Var, "x\0^1y");
        assert_eq!(sg.get_draw_color(), [0.5, 0.5, 0.5, 1.0]);
        sg.draw_string(0.0, 0.0, Align::Left, 12.0, Font::Var, "\0^1y");
        assert_eq!(sg.get_draw_color(), [0.5, 0.5, 0.5, 1.0]);
        assert_eq!(
            sg.draw_string_cursor_index(12.0, Font::Var, "\0abc", 100.0, 0.0),
            1
        );
        // A trace records the Lua argument unchanged.
        sg.trace = Some(TraceRecorder::new());
        sg.draw_string(0.0, 0.0, Align::Left, 12.0, Font::Var, "x\0^1y");
        let t = sg.trace.as_ref().unwrap();
        assert!(
            matches!(&t.cur.calls[0].call, ApiCall::DrawString { text, .. } if text == "x\0^1y")
        );
    }

    #[test]
    fn strings_with_fonts() {
        let Some((fonts, images)) = crate::font::tests::fonts_and_images() else {
            return;
        };
        let mut sg = Sg::new(Rc::new(fonts), images);
        sg.begin_frame(screen(800, 600, 1.0));
        assert_eq!(sg.draw_string_width(16.0, Font::Var, "Hello"), 31.0);
        assert_eq!(sg.draw_string_width(15.0, Font::Var, "Hello"), 31.0); // 15 -> 16
        assert_eq!(sg.draw_string_width(16.0, Font::Var, ""), 0.0);
        // Cursor index is 1-based bytes: 'é' is 2 bytes.
        assert_eq!(
            sg.draw_string_cursor_index(16.0, Font::Var, "Hello", 10.0, 0.0),
            2
        );
        assert_eq!(
            sg.draw_string_cursor_index(16.0, Font::Var, "Hello", 999.0, 0.0),
            6
        );
        assert_eq!(
            sg.draw_string_cursor_index(16.0, Font::Var, "éa", 999.0, 0.0),
            4
        );
        assert_eq!(
            sg.draw_string_cursor_index(16.0, Font::Var, "", 5.0, 0.0),
            1
        );
        // DPI 2: width is physical / scale.
        sg.begin_frame(screen(1600, 1200, 2.0));
        let w32 = sg.fonts.face(Font::Var).string_width(32, "Hello") as f64;
        assert_eq!(sg.draw_string_width(16.0, Font::Var, "Hello"), w32 / 2.0);
        // DPI 1.25: height lround(20)=20, width physical / 1.25 (double / float).
        sg.begin_frame(screen(1600, 1200, 1.25));
        let w20 = sg.fonts.face(Font::Var).string_width(20, "Hello") as f64;
        assert_eq!(
            sg.draw_string_width(16.0, Font::Var, "Hello"),
            w20 / 1.25f32 as f64
        );
        // DrawString emits one quad per visible glyph with the layer state.
        sg.begin_frame(screen(1600, 1200, 2.0));
        sg.set_draw_layer(Some(2), Some(7));
        sg.set_viewport(10.0, 10.0, 100.0, 100.0);
        sg.set_draw_color_a(0.5, 0.5, 0.5, 0.5);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "A ^1B");
        let qs = flat(&sg.end_frame());
        assert_eq!(qs.len(), 3); // 'A', ' ' (1px wide glyph), 'B'
        assert!(qs.iter().all(|c| c.layer == (2, 7)));
        assert_eq!(qs[0].vp, vp(20, 20, 200, 200));
        assert_eq!(qs[0].color, [0.5, 0.5, 0.5, 0.5]);
        assert_eq!(qs[2].color, [1.0, 0.0, 0.0, 1.0]);
        let var32 = sg
            .fonts
            .face(Font::Var)
            .heights()
            .iter()
            .find(|h| h.height == 32)
            .unwrap()
            .tex;
        assert_eq!(qs[0].tex, var32);
    }
}
