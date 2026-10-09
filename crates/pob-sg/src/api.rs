//! `Sg`: the SimpleGraphic drawing API as a command recorder (ui doc §4.2). Owner: U1.
//!
//! Ports (SimpleGraphic research copy master @ 8694a07; U1 re-verifies every row of ui doc §2 at
//! `3b1a346`): `ui_api.cpp:725-1230` (argument handling, DPI conversion) and
//! `engine/render/r_main.cpp:1440-1680` (renderer state). Model: one persistent queue of
//! VIEWPORT/BIND/COLOR/QUAD commands per `(layer, subLayer)` (`r_layer_c`), switching layer
//! re-emits the current viewport, every draw is BIND+COLOR+QUAD, and `end_frame` sorts the
//! queues by `(layer, subLayer)`.
//!
//! Moved from the pre-port `pob-sg/src/sg.rs` (flat `DrawCmd` list) by UI-U0 and rebuilt on the
//! §4.2 reference bodies; the moved tests are adapted to the queue model.

use std::rc::Rc;

use pob_lua::lua_error;

use crate::color::parse_escape_color;
use crate::font::FontSet;
use crate::frame::{digest, Cmd, Frame, LayerQueue, LayerSink, Quad};
use crate::image::{ImageHandle, ImageRegistry, TexId};
use crate::trace::{ApiCall, ImgRef, TraceRecorder};
use crate::types::{Align, Font, IRect, Num, ScreenInfo};

/// The recording renderer. One per window.
pub struct Sg {
    screen: ScreenInfo,
    dpi_override_percent: i32,
    /// SG layerList: creation order, persistent; [0] == (0,0)
    layers: Vec<LayerQueue>,
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

/// StripEscapes (IsColorEscape-based, ui_api.cpp:1206)
pub fn strip_escapes(s: &str) -> String {
    crate::color::strip_escapes(s)
}

/// ui_api.cpp:1095-1104
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
            cur: 0,
            viewport: IRect {
                x: 0,
                y: 0,
                w: screen.fb_w,
                h: screen.fb_h,
            },
            draw_color: [1.0; 4],
            last_color: [1.0; 4],
            render_enable: false,
            fonts,
            images,
            trace: None,
        }
    }

    // ---- host-only lifecycle ----

    /// Clears queues; cur = layer (0,0); SetViewport(); DrawColor(white); pumps the image loader.
    pub fn begin_frame(&mut self, screen: ScreenInfo) {
        self.screen = screen;
        for q in &mut self.layers {
            q.cmds.clear();
        }
        self.cur = 0;
        self.render_enable = true;
        self.set_viewport_phys(IRect {
            x: 0,
            y: 0,
            w: 0,
            h: 0,
        });
        self.draw_color = [1.0; 4];
        self.images.pump();
        if let Some(t) = &mut self.trace {
            t.begin_frame(screen);
        }
    }

    /// Sorted non-empty queues + texture uploads/releases + XXH3 digest.
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
        // r_main.cpp:1028-1042 qsort by (layer, subLayer); keys are unique, so the order is deterministic.
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

    /// VirtualScreenScaleFactor: the override percentage when set, else the OS content scale.
    #[inline]
    pub fn scale(&self) -> f32 {
        if self.dpi_override_percent > 0 {
            self.dpi_override_percent as f32 / 100.0
        } else {
            self.screen.dpi_scale
        }
    }
    #[inline]
    fn vsw(&self) -> i32 {
        self.screen.fb_w
    }
    #[inline]
    fn vsh(&self) -> i32 {
        self.screen.fb_h
    }

    /// (fb_w, fb_h)
    pub fn get_screen_size(&self) -> (i32, i32) {
        (self.screen.fb_w, self.screen.fb_h)
    }
    /// VirtualScreenScaleFactor
    pub fn get_screen_scale(&self) -> f32 {
        self.scale()
    }
    pub fn set_dpi_scale_override_percent(&mut self, percent: i32) {
        self.dpi_override_percent = percent;
    }
    pub fn get_dpi_scale_override_percent(&self) -> i32 {
        self.dpi_override_percent
    }
    /// Common.lua:1118 GetVirtualScreenSize (Lua helper, here for convenience)
    pub fn get_virtual_screen_size(&self) -> (Num, Num) {
        let (w, h) = self.get_screen_size();
        let scale = self.get_screen_scale() as f64;
        if scale != 1.0 {
            ((w as f64 / scale).floor(), (h as f64 / scale).floor())
        } else {
            (w as f64, h as f64)
        }
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

    #[track_caller]
    fn check_render(&self, func: &str) {
        if !self.render_enable {
            lua_error!("{func}() called outside of OnFrame");
        }
    }

    // ---- draw state ----

    /// SetDrawLayer(layer, subLayer) (ui_api.cpp:762-784, r_main.cpp:1456-1484).
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
                self.layers.len() - 1
            });
        self.cur = idx;
        let vp = self.viewport;
        self.layers[idx].cmds.push(Cmd::Viewport(vp));
    }

    /// GetDrawLayer: returns subLayer (SG quirk, r_main.cpp:1486)
    pub fn get_draw_layer(&self) -> i32 {
        self.layers[self.cur].sub_layer
    }

    /// SetViewport(x, y, w, h) (ui_api.cpp:793-815).
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

    fn set_viewport_phys(&mut self, mut vp: IRect) {
        if vp.h == 0 {
            // r_main.cpp:1493 (x, y kept)
            vp.w = self.vsw();
            vp.h = self.vsh();
        }
        self.viewport = vp;
        self.layers[self.cur].cmds.push(Cmd::Viewport(vp));
    }

    /// SetDrawColor(r, g, b): a = 1
    #[track_caller]
    pub fn set_draw_color(&mut self, r: Num, g: Num, b: Num) {
        self.set_draw_color_a(r, g, b, 1.0);
    }

    /// SetDrawColor(r, g, b, a) (ui_api.cpp:817-865).
    #[track_caller]
    pub fn set_draw_color_a(&mut self, r: Num, g: Num, b: Num, a: Num) {
        self.check_render("SetDrawColor");
        self.rec(|| ApiCall::SetDrawColor([r, g, b, a]));
        let c = [r as f32, g as f32, b as f32, a as f32];
        self.draw_color = c;
        self.last_color = c;
    }

    /// SetDrawColor("^7") / ("^xRRGGBB"): alpha forced to 1; invalid => lua_error.
    #[track_caller]
    pub fn set_draw_color_esc(&mut self, esc: &str) {
        self.check_render("SetDrawColor");
        self.rec(|| ApiCall::SetDrawColorEsc(esc.to_owned()));
        let c = parse_escape_color(esc)
            .unwrap_or_else(|| lua_error!("SetDrawColor(): invalid color escape sequence"));
        self.draw_color = c;
        self.last_color = c;
    }

    /// lastColor
    pub fn get_draw_color(&self) -> [f32; 4] {
        self.last_color
    }

    // ---- drawing ----

    fn quad_phys(&mut self, img: Option<&ImageHandle>, p: [f32; 8], uv: [f32; 8]) {
        let tex = match img {
            None => TexId::WHITE,
            Some(h) => h
                .tex()
                .unwrap_or_else(|| lua_error!("DrawImage(): image handle has no image loaded")),
        };
        let col = self.draw_color;
        let q = &mut self.layers[self.cur].cmds;
        q.push(Cmd::Bind(tex));
        q.push(Cmd::Color(col));
        q.push(Cmd::Quad(Quad {
            s: [uv[0], uv[2], uv[4], uv[6]],
            t: [uv[1], uv[3], uv[5], uv[7]],
            x: [p[0], p[2], p[4], p[6]],
            y: [p[1], p[3], p[5], p[7]],
        }));
    }

    /// DrawImage(img, x, y, w, h): tex coords (0,0)-(1,1).
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
            [x, y, x + w, y, x + w, y + h, x, y + h],
            [l, t, r, t, r, b, l, b],
        );
    }

    /// DrawImageQuad(img, x1, y1, .. x4, y4): default UVs (0,0),(1,0),(1,1),(0,1).
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

    fn draw_image_quad_phys(&mut self, img: Option<&ImageHandle>, p: [Num; 8], tc: [Num; 8]) {
        let s = self.scale();
        let p = p.map(|v| v as f32 * s);
        let uv = tc.map(|v| v as f32);
        self.quad_phys(img, p, uv);
    }

    /// DrawString(left, top, align, height, font, text) (ui_api.cpp:1092-1110, r_main.cpp:1565-1581).
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
            &mut LayerSink(&mut self.layers[self.cur]),
            pos,
            align,
            h,
            &mut col,
            text,
            vsw,
            win_h,
        );
        self.draw_color = col; // escapes leak (alpha forced to 1)
        self.last_color = col;
    }

    /// DrawStringWidth(height, font, text): physical width / scale.
    pub fn draw_string_width(&self, height: Num, font: Font, text: &str) -> Num {
        if text.is_empty() {
            return 0.0;
        }
        let s = self.scale();
        self.fonts
            .face(font)
            .string_width(scaled_text_height(height, s), text) as f64
            / s as f64
    }

    /// 1-based byte index into `text`, exactly what Lua receives.
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
        self.fonts.face(font).cursor_index(h, text, cx, cy) + 1
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
                                                 // SetDrawLayer(nil) is a Lua error in SG (the pre-port recorder ignored it).
        assert!(panics(|| sg.set_draw_layer(None, None)).contains("must provide subLayer"));
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

    #[test]
    fn viewport_snapshot_and_scaling() {
        let mut sg = sg();
        sg.begin_frame(screen(1000, 500, 1.5));
        assert_eq!(
            sg.viewport(),
            IRect {
                x: 0,
                y: 0,
                w: 1000,
                h: 500
            }
        );
        sg.set_viewport(10.3, 20.5, 100.1, 50.0);
        // lround(15.45)=15, lround(30.75)=31, ceil(150.15)=151, ceil(75)=75
        assert_eq!(
            sg.viewport(),
            IRect {
                x: 15,
                y: 31,
                w: 151,
                h: 75
            }
        );
        sg.set_draw_color_a(1.0, 0.0, 0.0, 0.5);
        sg.draw_image(None, 2.0, 4.0, 10.0, 10.0);
        sg.reset_viewport();
        sg.set_draw_color(0.0, 1.0, 0.0);
        sg.draw_image_tc(None, 2.0, 4.0, 10.0, 10.0, 0.0, 0.0, 2.0, 3.0);
        // Height 0 keeps x/y but uses the full screen size.
        sg.set_viewport(4.0, 4.0, 10.0, 0.0);
        assert_eq!(
            sg.viewport(),
            IRect {
                x: 6,
                y: 6,
                w: 1000,
                h: 500
            }
        );
        let qs = flat(&sg.end_frame());
        assert_eq!(
            qs[0].vp,
            IRect {
                x: 15,
                y: 31,
                w: 151,
                h: 75
            }
        );
        assert_eq!(qs[0].color, [1.0, 0.0, 0.0, 0.5]);
        assert_eq!(
            (qs[0].quad.x, qs[0].quad.y),
            ([3.0, 18.0, 18.0, 3.0], [6.0, 6.0, 21.0, 21.0])
        );
        assert_eq!(
            (qs[0].quad.s, qs[0].quad.t),
            ([0.0, 1.0, 1.0, 0.0], [0.0, 0.0, 1.0, 1.0])
        );
        assert_eq!(
            qs[1].vp,
            IRect {
                x: 0,
                y: 0,
                w: 1000,
                h: 500
            }
        );
        assert_eq!(
            (qs[1].quad.s, qs[1].quad.t),
            ([0.0, 2.0, 2.0, 0.0], [0.0, 0.0, 3.0, 3.0])
        );
        assert_eq!(qs[1].tex, TexId::WHITE);
    }

    #[test]
    fn layer_switch_keeps_viewport() {
        let mut sg = sg();
        sg.set_viewport(10.0, 10.0, 50.0, 50.0);
        sg.set_draw_layer(Some(3), Some(1));
        sg.draw_image(None, 0.0, 0.0, 5.0, 5.0);
        let f = sg.end_frame();
        // The switch re-emitted the viewport into the new queue.
        assert_eq!(
            f.layers[1].cmds[0],
            Cmd::Viewport(IRect {
                x: 10,
                y: 10,
                w: 50,
                h: 50
            })
        );
        assert_eq!(
            flat(&f)[0].vp,
            IRect {
                x: 10,
                y: 10,
                w: 50,
                h: 50
            }
        );
    }

    #[test]
    fn draw_color_escape_and_get() {
        let mut sg = sg();
        sg.set_draw_color_esc("^xFF8000");
        assert_eq!(sg.get_draw_color(), [1.0, 128.0 / 255.0, 0.0, 1.0]);
        assert!(panics(|| sg.set_draw_color_esc("nope")).contains("invalid color escape"));
        sg.set_draw_color_a(0.2, 0.3, 0.4, 0.5);
        // DrawString escapes persist into the draw colour (no fonts loaded: escapes only).
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "a^3b");
        assert_eq!(sg.current_draw_color(), [0.0, 0.0, 1.0, 1.0]);
        assert_eq!(sg.get_draw_color(), [0.0, 0.0, 1.0, 1.0]);
        // BeginFrame resets drawColor but not lastColor.
        sg.begin_frame(screen(800, 600, 1.0));
        assert_eq!(sg.current_draw_color(), [1.0; 4]);
        assert_eq!(sg.get_draw_color(), [0.0, 0.0, 1.0, 1.0]);
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
    fn font_height_rounding() {
        assert_eq!(scaled_text_height(0.4, 1.0), 1);
        assert_eq!(scaled_text_height(1.0, 1.0), 1);
        assert_eq!(scaled_text_height(13.0, 1.0), 14);
        assert_eq!(scaled_text_height(14.0, 1.0), 14);
        assert_eq!(scaled_text_height(14.0, 1.25), 18); // lround(17.5)=18
        assert_eq!(scaled_text_height(16.0, 1.5), 24);
        assert_eq!(scaled_text_height(15.0, 1.5), 24); // lround(22.5)=23 -> 24
    }

    #[test]
    fn unloaded_handle_is_a_lua_error() {
        let mut sg = sg();
        let h = sg.new_image_handle();
        // SG: "DrawImage(): image handle has no image loaded" (the pre-port recorder skipped the quad).
        assert!(panics(|| sg.draw_image(Some(&h), 0.0, 0.0, 1.0, 1.0)).contains("no image loaded"));
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
        // DrawString emits one quad per visible glyph with the layer state.
        sg.set_draw_layer(Some(2), Some(7));
        sg.set_viewport(10.0, 10.0, 100.0, 100.0);
        sg.set_draw_color_a(0.5, 0.5, 0.5, 0.5);
        sg.draw_string(0.0, 0.0, Align::Left, 16.0, Font::Var, "A ^1B");
        let qs = flat(&sg.end_frame());
        assert_eq!(qs.len(), 3); // 'A', ' ' (1px wide glyph), 'B'
        assert!(qs.iter().all(|c| c.layer == (2, 7)));
        assert_eq!(
            qs[0].vp,
            IRect {
                x: 20,
                y: 20,
                w: 200,
                h: 200
            }
        );
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
