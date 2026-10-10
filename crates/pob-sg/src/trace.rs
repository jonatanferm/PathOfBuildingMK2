//! API-level draw traces (ui doc §4.6 trace.rs, §11.1 T3, §11.2). Types and the recorder are
//! complete in UI-U0; the bodies of `normalize` and `diff` are U1's (owner of this file after
//! UI-U0).
//!
//! [`normalize`] replays SimpleGraphic's draw state over a frame's recorded calls (semantics of
//! PathOfBuilding-SimpleGraphic `3b1a346`, ui doc §2, re-verified in
//! `docs/port/ui/sg-semantics.md`):
//! - `SetDrawLayer` (`ui_api.cpp:759-781`, `r_main.cpp:1675-1704`) selects the bucket;
//!   `(nil, s)` keeps the layer; `(l)` means sub 0;
//! - the viewport and the draw colour are **global** renderer state (`curViewport`,
//!   `drawColor`): a layer switch re-emits the current viewport into the target queue
//!   (`r_main.cpp:1697`) and every draw records the current colour (`:1784`), so the state a
//!   draw is issued under is a snapshot of the globals at the call ("Consequence" row of §2);
//! - each frame starts with layer (0,0), the full-screen viewport and white
//!   (`r_main.cpp:1199-1203`);
//! - `DrawString` passes `col = NULL`, so its escapes overwrite the draw colour with alpha 1
//!   (`r_main.cpp:1793-1809`, `r_font.cpp:321-338, 410-417`): after the call the colour is the
//!   last escape in the text (up to its first NUL, the C string SG sees), else unchanged.
//!
//! Viewports are kept in **Lua (virtual) units**, as the Lua code passed them, so `diff`
//! compares them with the same `abs_eps` as other arguments. `SetViewport()` and a height that
//! SG turns into 0 (`ceil(h * dpi) == 0`, `r_main.cpp:1713`) become `[x, y, fb_w / dpi,
//! fb_h / dpi]` with `dpi = screen.dpi_scale` (`x, y` are 0 for `SetViewport()`). The trace does
//! not see `SetDPIScaleOverridePercent`; both sides of a T3 comparison share it, so this only
//! affects how the full-screen viewport is spelled, never whether two traces agree.

use std::collections::BTreeSet;

use crate::color::{is_color_escape, parse_escape_color, read_color_escape};
use crate::image::ImageFlags;
use crate::types::{Align, Font, ScreenInfo};

/// An image argument, identified by what it was loaded from.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct ImgRef {
    pub path: String,
    pub flags: ImageFlags,
}

/// One recorded SG call with its Lua-level arguments.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum ApiCall {
    SetDrawLayer {
        layer: Option<i32>,
        sub: Option<i32>,
    },
    SetViewport(Option<[f64; 4]>),
    SetDrawColor([f64; 4]),
    SetDrawColorEsc(String),
    DrawImage {
        img: Option<ImgRef>,
        rect: [f64; 4],
        tc: Option<[f64; 4]>,
    },
    DrawImageQuad {
        img: Option<ImgRef>,
        p: [f64; 8],
        tc: Option<[f64; 8]>,
    },
    DrawString {
        x: f64,
        y: f64,
        align: Align,
        height: f64,
        font: Font,
        text: String,
    },
}

impl ApiCall {
    /// `DrawImage(img, x, y, w, h[, l, t, r, b])`.
    pub fn draw_image(img: Option<ImgRef>, rect: [f64; 4], tc: Option<[f64; 4]>) -> ApiCall {
        ApiCall::DrawImage { img, rect, tc }
    }
}

/// Observable side effects compared alongside draws.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub enum SideEffect {
    ImageLoad(ImgRef),
    WindowTitle(String),
    Copy(String),
    OpenUrl(String),
    FileWrite { path: String, xxh3: u64 },
    Screenshot,
    Exit,
}

/// One call with its control path and source location.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct CallRec {
    pub call: ApiCall,
    pub ctl: Option<String>,
    pub src: Option<String>,
}

/// Everything recorded in one frame.
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct FrameTrace {
    pub index: u32,
    pub screen: Option<ScreenInfo>,
    pub calls: Vec<CallRec>,
    pub effects: Vec<SideEffect>,
}

/// One draw with the state it was issued under: the unit of comparison.
/// `ctl` (control path, e.g. "BuildMode/NotesTab/toggleColorCodes") and `src` are excluded from equality.
#[derive(Clone, Debug, serde::Serialize, serde::Deserialize)]
pub struct DrawOp {
    pub viewport: [f64; 4],
    pub color: [f32; 4],
    pub call: ApiCall,
    pub ctl: Option<String>,
    pub src: Option<String>,
}

impl PartialEq for DrawOp {
    fn eq(&self, o: &Self) -> bool {
        self.viewport == o.viewport && self.color == o.color && self.call == o.call
    }
}

pub type Buckets = std::collections::BTreeMap<(i32, i32), Vec<DrawOp>>;

/// The text SG sees: `lua_tostring` handed on as a C string stops at the first NUL.
fn c_str(s: &str) -> &str {
    match s.find('\0') {
        Some(i) => &s[..i],
        None => s,
    }
}

/// The draw colour after `DrawString(.., text)`: every escape overwrites RGB and forces alpha
/// to 1, on visible and invisible lines alike (`r_font.cpp:326-336`, `:410-417`).
fn after_draw_string(mut col: [f32; 4], text: &str) -> [f32; 4] {
    let b = c_str(text).as_bytes();
    let mut i = 0;
    while i < b.len() {
        let n = is_color_escape(&b[i..]);
        if n > 0 {
            read_color_escape(&b[i..], n, &mut col);
            col[3] = 1.0;
            i += n;
        } else {
            i += 1;
        }
    }
    col
}

/// The full-screen viewport in Lua units at `screen.dpi_scale`.
fn full_viewport(x: f64, y: f64, screen: &ScreenInfo) -> [f64; 4] {
    let s = f64::from(screen.dpi_scale);
    [x, y, f64::from(screen.fb_w) / s, f64::from(screen.fb_h) / s]
}

/// Replays SetDrawLayer/SetViewport/SetDrawColor semantics, including the DrawString escape leak
/// (the colour after a DrawString is the last escape in the text with alpha 1, else unchanged).
///
/// Calls SG rejects with a Lua error (`SetDrawLayer(nil)`, an invalid escape string) change no
/// state. Every draw call becomes one [`DrawOp`] in its `(layer, sub)` bucket, carrying the
/// call's `ctl`/`src`.
///
/// sg: r_main.cpp:1199-1203, :1675-1723, :1731-1742, :1774-1809, ui_api.cpp:790-874 @3b1a346
pub fn normalize(t: &FrameTrace, screen: &ScreenInfo) -> Buckets {
    let mut out = Buckets::new();
    // BeginFrame: curLayer = layerList[0]; SetViewport(); DrawColor();
    let mut layer: (i32, i32) = (0, 0);
    let mut viewport = full_viewport(0.0, 0.0, screen);
    let mut color: [f32; 4] = [1.0; 4];
    for rec in &t.calls {
        let push = |out: &mut Buckets, layer: (i32, i32), viewport, color| {
            out.entry(layer).or_default().push(DrawOp {
                viewport,
                color,
                call: rec.call.clone(),
                ctl: rec.ctl.clone(),
                src: rec.src.clone(),
            });
        };
        match &rec.call {
            ApiCall::SetDrawLayer { layer: l, sub } => match (l, sub) {
                (None, Some(s)) => layer.1 = *s,
                (Some(l), s) => layer = (*l, s.unwrap_or(0)),
                (None, None) => {} // "must provide subLayer if layer is nil"
            },
            ApiCall::SetViewport(Some([x, y, w, h])) => {
                // r_renderer_c::SetViewport: height 0 => full screen, x and y kept
                let s = f64::from(screen.dpi_scale);
                viewport = if (h * s).ceil() as i32 == 0 {
                    full_viewport(*x, *y, screen)
                } else {
                    [*x, *y, *w, *h]
                };
            }
            ApiCall::SetViewport(None) => viewport = full_viewport(0.0, 0.0, screen),
            ApiCall::SetDrawColor(c) => color = c.map(|v| v as f32),
            ApiCall::SetDrawColorEsc(s) => {
                if let Some(c) = parse_escape_color(c_str(s)) {
                    color = c;
                }
            }
            ApiCall::DrawImage { .. } | ApiCall::DrawImageQuad { .. } => {
                push(&mut out, layer, viewport, color);
            }
            ApiCall::DrawString { text, .. } => {
                push(&mut out, layer, viewport, color);
                color = after_draw_string(color, text);
            }
        }
    }
    out
}

/// Comparison settings.
#[derive(Clone, Debug)]
pub struct DiffConfig {
    /// default 1e-6 on f64 args; colours exact
    pub abs_eps: f64,
    /// bucket ranges compared order-insensitively
    pub multiset: Vec<((i32, i32), (i32, i32))>,
    /// ignore DrawString text in a rect (open calc issues only)
    pub masks: Vec<TextMask>,
}

impl Default for DiffConfig {
    fn default() -> Self {
        DiffConfig {
            abs_eps: 1e-6,
            multiset: Vec::new(),
            masks: Vec::new(),
        }
    }
}

/// A DrawString text mask (ui doc §11.3: only after switching to the Rust calc, naming an issue).
///
/// A Lua-side `DrawString` whose `(x, y)` lies in `rect` (`[x, y, w, h]`, half-open) and whose
/// text matches the Lua pattern `pattern` on both sides is compared without its text.
#[derive(Clone, Debug)]
pub struct TextMask {
    pub rect: [f64; 4],
    pub pattern: String,
    pub issue: String,
}

/// One difference between the Lua and the Rust trace.
///
/// `idx` is the op's 0-based position in its side's bucket (`Changed`: the Lua side's).
#[derive(Clone, Debug)]
pub enum Divergence {
    Missing {
        bucket: (i32, i32),
        idx: usize,
        op: DrawOp,
    },
    Extra {
        bucket: (i32, i32),
        idx: usize,
        op: DrawOp,
    },
    Changed {
        bucket: (i32, i32),
        idx: usize,
        lua: DrawOp,
        rust: DrawOp,
    },
    Effect {
        lua: Option<SideEffect>,
        rust: Option<SideEffect>,
    },
}

/// `|a - b| <= eps`, with equal infinities and NaN == NaN counted as equal.
fn num_eq(a: f64, b: f64, eps: f64) -> bool {
    a == b || (a.is_nan() && b.is_nan()) || (a - b).abs() <= eps
}

fn nums_eq(a: &[f64], b: &[f64], eps: f64) -> bool {
    a.len() == b.len() && a.iter().zip(b).all(|(x, y)| num_eq(*x, *y, eps))
}

fn opt_nums_eq<const N: usize>(a: Option<&[f64; N]>, b: Option<&[f64; N]>, eps: f64) -> bool {
    match (a, b) {
        (None, None) => true,
        (Some(a), Some(b)) => nums_eq(a, b, eps),
        _ => false,
    }
}

/// Colours are compared exactly (bit patterns, so NaN == NaN and -0 != +0 do not surprise).
fn color_eq(a: &[f32; 4], b: &[f32; 4]) -> bool {
    a.iter()
        .zip(b)
        .all(|(x, y)| x == y || x.to_bits() == y.to_bits())
}

impl DiffConfig {
    fn is_multiset(&self, bucket: (i32, i32)) -> bool {
        self.multiset
            .iter()
            .any(|(lo, hi)| *lo <= bucket && bucket <= *hi)
    }

    /// Whether the text of this `DrawString` pair is masked.
    fn text_masked(&self, x: f64, y: f64, lua_text: &str, rust_text: &str) -> bool {
        self.masks.iter().any(|m| {
            let [rx, ry, rw, rh] = m.rect;
            x >= rx
                && x < rx + rw
                && y >= ry
                && y < ry + rh
                && pob_lua::pattern::find(lua_text, &m.pattern, None, false).is_some()
                && pob_lua::pattern::find(rust_text, &m.pattern, None, false).is_some()
        })
    }

    fn call_eq(&self, a: &ApiCall, b: &ApiCall) -> bool {
        let eps = self.abs_eps;
        match (a, b) {
            (
                ApiCall::DrawImage { img, rect, tc },
                ApiCall::DrawImage {
                    img: img2,
                    rect: rect2,
                    tc: tc2,
                },
            ) => {
                img == img2
                    && nums_eq(rect, rect2, eps)
                    && opt_nums_eq(tc.as_ref(), tc2.as_ref(), eps)
            }
            (
                ApiCall::DrawImageQuad { img, p, tc },
                ApiCall::DrawImageQuad {
                    img: img2,
                    p: p2,
                    tc: tc2,
                },
            ) => img == img2 && nums_eq(p, p2, eps) && opt_nums_eq(tc.as_ref(), tc2.as_ref(), eps),
            (
                ApiCall::DrawString {
                    x,
                    y,
                    align,
                    height,
                    font,
                    text,
                },
                ApiCall::DrawString {
                    x: x2,
                    y: y2,
                    align: align2,
                    height: height2,
                    font: font2,
                    text: text2,
                },
            ) => {
                num_eq(*x, *x2, eps)
                    && num_eq(*y, *y2, eps)
                    && align == align2
                    && num_eq(*height, *height2, eps)
                    && font == font2
                    && (text == text2 || self.text_masked(*x, *y, text, text2))
            }
            (ApiCall::SetViewport(a), ApiCall::SetViewport(b)) => {
                opt_nums_eq(a.as_ref(), b.as_ref(), eps)
            }
            (ApiCall::SetDrawColor(a), ApiCall::SetDrawColor(b)) => nums_eq(a, b, eps),
            _ => a == b,
        }
    }

    /// Two ops are the same draw: viewport within `abs_eps`, colour exact, call within
    /// `abs_eps` (strings, images, fonts and alignments exact; masked texts skipped).
    fn op_eq(&self, a: &DrawOp, b: &DrawOp) -> bool {
        nums_eq(&a.viewport, &b.viewport, self.abs_eps)
            && color_eq(&a.color, &b.color)
            && self.call_eq(&a.call, &b.call)
    }
}

/// Largest middle section (after trimming the common prefix and suffix) aligned with an exact
/// LCS; beyond it the middle is compared position by position.
const LCS_MAX_CELLS: usize = 4_000_000;

enum Edit {
    Same,
    Del,
    Ins,
}

/// An edit script turning `a` into `b` (LCS-based when small enough).
fn edit_script(a: &[DrawOp], b: &[DrawOp], cfg: &DiffConfig) -> Vec<Edit> {
    let mut pre = 0;
    while pre < a.len() && pre < b.len() && cfg.op_eq(&a[pre], &b[pre]) {
        pre += 1;
    }
    let mut suf = 0;
    while suf < a.len() - pre
        && suf < b.len() - pre
        && cfg.op_eq(&a[a.len() - 1 - suf], &b[b.len() - 1 - suf])
    {
        suf += 1;
    }
    let (ma, mb) = (&a[pre..a.len() - suf], &b[pre..b.len() - suf]);
    let mut script: Vec<Edit> = (0..pre).map(|_| Edit::Same).collect();
    let (n, m) = (ma.len(), mb.len());
    if n > 0 && m > 0 && (n + 1) * (m + 1) <= LCS_MAX_CELLS {
        // lcs[i][j] = LCS length of ma[i..], mb[j..]
        let w = m + 1;
        let mut lcs = vec![0u32; (n + 1) * w];
        for i in (0..n).rev() {
            for j in (0..m).rev() {
                lcs[i * w + j] = if cfg.op_eq(&ma[i], &mb[j]) {
                    lcs[(i + 1) * w + j + 1] + 1
                } else {
                    lcs[(i + 1) * w + j].max(lcs[i * w + j + 1])
                };
            }
        }
        let (mut i, mut j) = (0, 0);
        while i < n && j < m {
            if cfg.op_eq(&ma[i], &mb[j]) && lcs[i * w + j] == lcs[(i + 1) * w + j + 1] + 1 {
                script.push(Edit::Same);
                i += 1;
                j += 1;
            } else if lcs[(i + 1) * w + j] >= lcs[i * w + j + 1] {
                script.push(Edit::Del);
                i += 1;
            } else {
                script.push(Edit::Ins);
                j += 1;
            }
        }
        script.extend((i..n).map(|_| Edit::Del));
        script.extend((j..m).map(|_| Edit::Ins));
    } else {
        // Position by position: a run of n deletions and m insertions (paired below).
        script.extend((0..n).map(|_| Edit::Del));
        script.extend((0..m).map(|_| Edit::Ins));
    }
    script.extend((0..suf).map(|_| Edit::Same));
    script
}

/// Ordered comparison of one bucket: unmatched runs between matches are paired up as
/// `Changed` (in order), the rest are `Missing` / `Extra`.
fn diff_ordered(
    bucket: (i32, i32),
    a: &[DrawOp],
    b: &[DrawOp],
    cfg: &DiffConfig,
    out: &mut Vec<Divergence>,
) {
    let script = edit_script(a, b, cfg);
    let (mut i, mut j) = (0usize, 0usize);
    let mut dels: Vec<usize> = Vec::new();
    let mut inss: Vec<usize> = Vec::new();
    let flush = |dels: &mut Vec<usize>, inss: &mut Vec<usize>, out: &mut Vec<Divergence>| {
        let k = dels.len().min(inss.len());
        for p in 0..k {
            out.push(Divergence::Changed {
                bucket,
                idx: dels[p],
                lua: a[dels[p]].clone(),
                rust: b[inss[p]].clone(),
            });
        }
        for &d in &dels[k..] {
            out.push(Divergence::Missing {
                bucket,
                idx: d,
                op: a[d].clone(),
            });
        }
        for &e in &inss[k..] {
            out.push(Divergence::Extra {
                bucket,
                idx: e,
                op: b[e].clone(),
            });
        }
        dels.clear();
        inss.clear();
    };
    for e in script {
        match e {
            Edit::Same => {
                flush(&mut dels, &mut inss, out);
                i += 1;
                j += 1;
            }
            Edit::Del => {
                dels.push(i);
                i += 1;
            }
            Edit::Ins => {
                inss.push(j);
                j += 1;
            }
        }
    }
    flush(&mut dels, &mut inss, out);
}

/// Order-insensitive comparison of one bucket (ui doc §8 rule 6 (b)): each Lua op is matched
/// to the first unmatched equal Rust op.
fn diff_multiset(
    bucket: (i32, i32),
    a: &[DrawOp],
    b: &[DrawOp],
    cfg: &DiffConfig,
    out: &mut Vec<Divergence>,
) {
    let mut used = vec![false; b.len()];
    for (i, op) in a.iter().enumerate() {
        match (0..b.len()).find(|&j| !used[j] && cfg.op_eq(op, &b[j])) {
            Some(j) => used[j] = true,
            None => out.push(Divergence::Missing {
                bucket,
                idx: i,
                op: op.clone(),
            }),
        }
    }
    for (j, op) in b.iter().enumerate() {
        if !used[j] {
            out.push(Divergence::Extra {
                bucket,
                idx: j,
                op: op.clone(),
            });
        }
    }
}

/// Compares two normalised frames bucket by bucket, in `(layer, sub)` order. Buckets inside a
/// `cfg.multiset` range (inclusive, lexicographic) are compared as multisets; all others in
/// order, aligned by a longest common subsequence so that one inserted op reports one
/// divergence rather than shifting the rest of the bucket.
pub fn diff(lua: &Buckets, rust: &Buckets, cfg: &DiffConfig) -> Vec<Divergence> {
    let keys: BTreeSet<(i32, i32)> = lua.keys().chain(rust.keys()).copied().collect();
    let mut out = Vec::new();
    for k in keys {
        let a = lua.get(&k).map_or(&[][..], Vec::as_slice);
        let b = rust.get(&k).map_or(&[][..], Vec::as_slice);
        if cfg.is_multiset(k) {
            diff_multiset(k, a, b, cfg, &mut out);
        } else {
            diff_ordered(k, a, b, cfg, &mut out);
        }
    }
    out
}

/// Side effects are compared in order, position by position.
pub fn diff_effects(lua: &[SideEffect], rust: &[SideEffect]) -> Vec<Divergence> {
    let n = lua.len().max(rust.len());
    (0..n)
        .filter_map(|i| {
            let (a, b) = (lua.get(i), rust.get(i));
            (a != b).then(|| Divergence::Effect {
                lua: a.cloned(),
                rust: b.cloned(),
            })
        })
        .collect()
}

/// One frame of T3: [`normalize`] both sides (each with its own recorded screen, falling back
/// to the other side's), then [`diff`] the draws and [`diff_effects`] the side effects.
pub fn diff_frames(lua: &FrameTrace, rust: &FrameTrace, cfg: &DiffConfig) -> Vec<Divergence> {
    const FALLBACK: ScreenInfo = ScreenInfo {
        fb_w: 1920,
        fb_h: 1080,
        win_h: 1080,
        dpi_scale: 1.0,
    };
    let sl = lua.screen.or(rust.screen).unwrap_or(FALLBACK);
    let sr = rust.screen.or(lua.screen).unwrap_or(FALLBACK);
    let mut out = diff(&normalize(lua, &sl), &normalize(rust, &sr), cfg);
    out.extend(diff_effects(&lua.effects, &rust.effects));
    out
}

/// Records `FrameTrace`s for `Sg` (installed in `Sg::trace` by tests and the oracle).
#[derive(Default)]
pub struct TraceRecorder {
    /// Finished frames.
    pub frames: Vec<FrameTrace>,
    /// The frame being recorded.
    pub cur: FrameTrace,
    ctl: Vec<String>,
    src: Option<String>,
}

impl TraceRecorder {
    pub fn new() -> TraceRecorder {
        TraceRecorder::default()
    }

    /// Start a frame (indices count from 0).
    pub fn begin_frame(&mut self, screen: ScreenInfo) {
        self.cur = FrameTrace {
            index: self.frames.len() as u32,
            screen: Some(screen),
            calls: Vec::new(),
            effects: Vec::new(),
        };
    }

    /// Close the current frame.
    pub fn end_frame(&mut self) {
        let done = std::mem::take(&mut self.cur);
        self.frames.push(done);
    }

    /// Record one call; `src` is the oracle's Lua location if set (consumed), else `caller`.
    pub fn record(&mut self, call: ApiCall, caller: &std::panic::Location<'_>) {
        let ctl = if self.ctl.is_empty() {
            None
        } else {
            Some(self.ctl.join("/"))
        };
        let src = self
            .src
            .take()
            .or_else(|| Some(format!("{}:{}", caller.file(), caller.line())));
        self.cur.calls.push(CallRec { call, ctl, src });
    }

    pub fn effect(&mut self, e: SideEffect) {
        self.cur.effects.push(e);
    }

    /// Control-path markers (ui doc §11.2 patch 7).
    pub fn ctl_push(&mut self, path_elem: &str) {
        self.ctl.push(path_elem.to_owned());
    }

    pub fn ctl_pop(&mut self) {
        self.ctl.pop();
    }

    /// Oracle: the Lua "Classes/X.lua:54" location of the next call.
    pub fn set_src(&mut self, src: Option<String>) {
        self.src = src;
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::font::FontSet;
    use crate::frame::Cmd;
    use crate::image::tests::MemLoader;
    use crate::image::ImageRegistry;
    use crate::types::IRect;
    use crate::Sg;
    use std::rc::Rc;

    /// xorshift64*: a deterministic generator for the property tests (no rand dependency).
    struct Rng(u64);
    impl Rng {
        fn next(&mut self) -> u64 {
            let mut x = self.0;
            x ^= x >> 12;
            x ^= x << 25;
            x ^= x >> 27;
            self.0 = x;
            x.wrapping_mul(0x2545_F491_4F6C_DD1D)
        }
        fn below(&mut self, n: u64) -> u64 {
            self.next() % n
        }
        fn f(&mut self, lo: f64, hi: f64) -> f64 {
            lo + (self.next() >> 11) as f64 / (1u64 << 53) as f64 * (hi - lo)
        }
    }

    const SCREEN: ScreenInfo = ScreenInfo {
        fb_w: 800,
        fb_h: 600,
        win_h: 600,
        dpi_scale: 1.0,
    };

    fn rec(call: ApiCall) -> CallRec {
        CallRec {
            call,
            ctl: None,
            src: None,
        }
    }

    fn frame(calls: Vec<ApiCall>) -> FrameTrace {
        FrameTrace {
            index: 0,
            screen: Some(SCREEN),
            calls: calls.into_iter().map(rec).collect(),
            effects: Vec::new(),
        }
    }

    fn img(x: f64) -> ApiCall {
        ApiCall::draw_image(None, [x, 0.0, 1.0, 1.0], None)
    }

    fn text(s: &str) -> ApiCall {
        ApiCall::DrawString {
            x: 0.0,
            y: 0.0,
            align: Align::Left,
            height: 14.0,
            font: Font::Var,
            text: s.to_owned(),
        }
    }

    #[test]
    fn normalize_replays_state() {
        let t = frame(vec![
            img(0.0),
            ApiCall::SetDrawLayer {
                layer: Some(5),
                sub: Some(2),
            },
            ApiCall::SetViewport(Some([1.0, 2.0, 3.0, 4.0])),
            ApiCall::SetDrawColor([0.5, 0.25, 0.125, 0.5]),
            img(1.0),
            ApiCall::SetDrawLayer {
                layer: None,
                sub: Some(7),
            },
            img(2.0),
            ApiCall::SetDrawLayer {
                layer: None,
                sub: None,
            }, // a Lua error: no effect
            text("a^1b"),
            img(3.0),
            text("^xFF0000\0^2"), // the NUL ends the C string
            ApiCall::SetDrawColorEsc("^4".into()),
            ApiCall::SetDrawLayer {
                layer: Some(5),
                sub: None,
            },
            ApiCall::SetViewport(None),
            img(4.0),
            ApiCall::SetViewport(Some([3.0, 4.0, 5.0, 0.0])), // height 0: full screen, x/y kept
            ApiCall::SetDrawColorEsc("bad".into()),           // a Lua error: no effect
            img(5.0),
        ]);
        let b = normalize(&t, &SCREEN);
        let keys: Vec<_> = b.keys().copied().collect();
        assert_eq!(keys, vec![(0, 0), (5, 0), (5, 2), (5, 7)]);
        assert_eq!(b[&(0, 0)][0].viewport, [0.0, 0.0, 800.0, 600.0]);
        assert_eq!(b[&(0, 0)][0].color, [1.0; 4]);
        assert_eq!(b[&(5, 2)][0].viewport, [1.0, 2.0, 3.0, 4.0]);
        assert_eq!(b[&(5, 2)][0].color, [0.5, 0.25, 0.125, 0.5]);
        let s7 = &b[&(5, 7)];
        assert_eq!(s7.len(), 4); // img 2, text, img 3, text
        assert_eq!(s7[0].color, [0.5, 0.25, 0.125, 0.5]);
        assert_eq!(s7[1].color, [0.5, 0.25, 0.125, 0.5]); // the string draws with the old colour
        assert_eq!(s7[2].color, [1.0, 0.0, 0.0, 1.0]); // leaked, alpha 1
        assert_eq!(s7[3].color, [1.0, 0.0, 0.0, 1.0]);
        let s0 = &b[&(5, 0)];
        assert_eq!(s0[0].viewport, [0.0, 0.0, 800.0, 600.0]);
        assert_eq!(s0[0].color, [1.0, 1.0, 0.0, 1.0]); // ^4
        assert_eq!(s0[1].viewport, [3.0, 4.0, 800.0, 600.0]);
        assert_eq!(s0[1].color, [1.0, 1.0, 0.0, 1.0]);
        // At dpi 1.25 the full screen is the virtual size.
        let hi = ScreenInfo {
            dpi_scale: 1.25,
            ..SCREEN
        };
        assert_eq!(
            normalize(&t, &hi)[&(0, 0)][0].viewport,
            [0.0, 0.0, 640.0, 480.0]
        );
    }

    #[test]
    fn normalize_carries_ctl_and_src() {
        let mut t = frame(vec![img(0.0)]);
        t.calls[0].ctl = Some("Main/button".into());
        t.calls[0].src = Some("Classes/ButtonControl.lua:54".into());
        let b = normalize(&t, &SCREEN);
        let op = &b[&(0, 0)][0];
        assert_eq!(op.ctl.as_deref(), Some("Main/button"));
        assert_eq!(op.src.as_deref(), Some("Classes/ButtonControl.lua:54"));
        // ctl/src never make two ops differ.
        let mut u = t.clone();
        u.calls[0].ctl = None;
        u.calls[0].src = Some("crates/x.rs:1".into());
        assert!(diff(&b, &normalize(&u, &SCREEN), &DiffConfig::default()).is_empty());
    }

    /// One op of a random bucket plan: full explicit state, then a draw.
    #[derive(Clone)]
    struct PlanOp {
        vp: [f64; 4],
        color: [f64; 4],
        call: ApiCall,
    }

    fn random_call(r: &mut Rng) -> ApiCall {
        match r.below(3) {
            0 => ApiCall::draw_image(
                None,
                [
                    r.f(-50.0, 900.0),
                    r.f(-50.0, 700.0),
                    r.f(0.0, 64.0),
                    r.f(0.0, 64.0),
                ],
                if r.below(2) == 0 {
                    None
                } else {
                    Some([0.0, 0.0, r.f(0.0, 2.0), r.f(0.0, 2.0)])
                },
            ),
            1 => ApiCall::DrawImageQuad {
                img: None,
                p: [0.0; 8].map(|_| r.f(-20.0, 820.0)),
                tc: None,
            },
            _ => ApiCall::DrawString {
                x: r.f(0.0, 800.0),
                y: r.f(0.0, 600.0),
                align: [Align::Left, Align::Center, Align::RightX][r.below(3) as usize],
                height: [12.0, 14.0, 16.0][r.below(3) as usize],
                font: Font::Var,
                text: ["Life", "^7Mana", "^x33FF77+12%^7 x", "ES"][r.below(4) as usize].into(),
            },
        }
    }

    fn random_plan(r: &mut Rng) -> Vec<((i32, i32), Vec<PlanOp>)> {
        let keys = [(0, 0), (1, 0), (1, 5), (10, -3), (-2, 4)];
        keys.iter()
            .map(|&k| {
                let n = 1 + r.below(12) as usize;
                let ops = (0..n)
                    .map(|_| PlanOp {
                        vp: [
                            r.f(0.0, 100.0),
                            r.f(0.0, 100.0),
                            r.f(1.0, 700.0),
                            r.f(1.0, 500.0),
                        ],
                        color: [r.f(0.0, 1.0), r.f(0.0, 1.0), r.f(0.0, 1.0), r.f(0.0, 1.0)],
                        call: random_call(r),
                    })
                    .collect();
                (k, ops)
            })
            .collect()
    }

    /// Emits the plan as one frame, interleaving buckets in a random order that keeps each
    /// bucket's own sequence.
    fn emit(plan: &[((i32, i32), Vec<PlanOp>)], r: &mut Rng) -> FrameTrace {
        let mut next = vec![0usize; plan.len()];
        let mut calls = Vec::new();
        loop {
            let open: Vec<usize> = (0..plan.len())
                .filter(|&i| next[i] < plan[i].1.len())
                .collect();
            if open.is_empty() {
                break;
            }
            let b = open[r.below(open.len() as u64) as usize];
            let ((l, s), ops) = &plan[b];
            let op = &ops[next[b]];
            next[b] += 1;
            calls.push(ApiCall::SetDrawLayer {
                layer: Some(*l),
                sub: Some(*s),
            });
            calls.push(ApiCall::SetViewport(Some(op.vp)));
            calls.push(ApiCall::SetDrawColor(op.color));
            calls.push(op.call.clone());
        }
        frame(calls)
    }

    /// Acceptance (ui doc §12.3 U1): a shuffle across buckets gives no diff; a shuffle within a
    /// bucket gives a diff (unless the bucket is configured as a multiset).
    #[test]
    fn shuffle_property() {
        let cfg = DiffConfig::default();
        let mut r = Rng(0x9E37_79B9_7F4A_7C15);
        for round in 0..200 {
            let plan = random_plan(&mut r);
            let a = normalize(&emit(&plan, &mut r), &SCREEN);
            let b = normalize(&emit(&plan, &mut r), &SCREEN);
            assert!(
                diff(&a, &b, &cfg).is_empty(),
                "round {round}: cross-bucket shuffle diverged"
            );
            assert!(diff(&b, &a, &cfg).is_empty());

            // Swap two different ops inside one bucket.
            let mut plan2 = plan.clone();
            let Some(bi) = (0..plan2.len()).find(|&i| plan2[i].1.len() >= 2) else {
                continue;
            };
            let n = plan2[bi].1.len();
            let i = r.below(n as u64) as usize;
            let j = (i + 1 + r.below(n as u64 - 1) as usize) % n;
            plan2[bi].1.swap(i, j);
            let c = normalize(&emit(&plan2, &mut r), &SCREEN);
            let d = diff(&a, &c, &cfg);
            assert!(
                !d.is_empty(),
                "round {round}: in-bucket shuffle not detected"
            );
            let key = plan2[bi].0;
            assert!(d.iter().all(|x| match x {
                Divergence::Missing { bucket, .. }
                | Divergence::Extra { bucket, .. }
                | Divergence::Changed { bucket, .. } => *bucket == key,
                Divergence::Effect { .. } => false,
            }));
            // The same shuffle in a multiset bucket is accepted.
            let ms = DiffConfig {
                multiset: vec![(key, key)],
                ..DiffConfig::default()
            };
            assert!(
                diff(&a, &c, &ms).is_empty(),
                "round {round}: multiset bucket diverged"
            );
        }
    }

    #[test]
    fn diff_reports_missing_extra_changed() {
        let cfg = DiffConfig::default();
        let a = normalize(
            &frame(vec![img(0.0), img(1.0), img(2.0), img(3.0)]),
            &SCREEN,
        );
        // One op inserted in the middle: a single Extra, nothing shifted.
        let b = normalize(
            &frame(vec![img(0.0), img(1.0), img(9.0), img(2.0), img(3.0)]),
            &SCREEN,
        );
        let d = diff(&a, &b, &cfg);
        assert_eq!(d.len(), 1);
        assert!(matches!(
            d[0],
            Divergence::Extra {
                bucket: (0, 0),
                idx: 2,
                ..
            }
        ));
        // The reverse is a single Missing.
        let d = diff(&b, &a, &cfg);
        assert_eq!(d.len(), 1);
        assert!(matches!(
            d[0],
            Divergence::Missing {
                bucket: (0, 0),
                idx: 2,
                ..
            }
        ));
        // One op changed: a single Changed at its index.
        let c = normalize(
            &frame(vec![img(0.0), img(1.5), img(2.0), img(3.0)]),
            &SCREEN,
        );
        let d = diff(&a, &c, &cfg);
        assert_eq!(d.len(), 1);
        match &d[0] {
            Divergence::Changed {
                bucket,
                idx,
                lua,
                rust,
            } => {
                assert_eq!((*bucket, *idx), ((0, 0), 1));
                assert_eq!(lua.call, img(1.0));
                assert_eq!(rust.call, img(1.5));
            }
            other => panic!("{other:?}"),
        }
        // A whole bucket missing on one side.
        let e = normalize(
            &frame(vec![
                img(0.0),
                img(1.0),
                img(2.0),
                img(3.0),
                ApiCall::SetDrawLayer {
                    layer: Some(3),
                    sub: None,
                },
                img(7.0),
            ]),
            &SCREEN,
        );
        let d = diff(&a, &e, &cfg);
        assert_eq!(d.len(), 1);
        assert!(matches!(
            d[0],
            Divergence::Extra {
                bucket: (3, 0),
                idx: 0,
                ..
            }
        ));
        // A colour difference is never within eps.
        let f1 = normalize(
            &frame(vec![ApiCall::SetDrawColor([0.5, 0.5, 0.5, 1.0]), img(0.0)]),
            &SCREEN,
        );
        let f2 = normalize(
            &frame(vec![
                ApiCall::SetDrawColor([0.5 + 1e-7, 0.5, 0.5, 1.0]),
                img(0.0),
            ]),
            &SCREEN,
        );
        assert_eq!(diff(&f1, &f2, &cfg).len(), 1);
    }

    #[test]
    fn diff_eps_and_masks() {
        let cfg = DiffConfig::default();
        let a = normalize(&frame(vec![img(10.0), text("DPS: 1,234")]), &SCREEN);
        let b = normalize(&frame(vec![img(10.0 + 5e-7), text("DPS: 1,234")]), &SCREEN);
        assert!(diff(&a, &b, &cfg).is_empty());
        let c = normalize(&frame(vec![img(10.0 + 1e-3), text("DPS: 1,234")]), &SCREEN);
        assert_eq!(diff(&a, &c, &cfg).len(), 1);
        let d = normalize(&frame(vec![img(10.0), text("DPS: 1,235")]), &SCREEN);
        assert_eq!(diff(&a, &d, &cfg).len(), 1);
        let masked = DiffConfig {
            masks: vec![TextMask {
                rect: [0.0, 0.0, 10.0, 10.0],
                pattern: "^DPS: [%d,]+$".into(),
                issue: "calc#1".into(),
            }],
            ..DiffConfig::default()
        };
        assert!(diff(&a, &d, &masked).is_empty());
        // Outside the rect, or not matching the pattern: still compared.
        let off = DiffConfig {
            masks: vec![TextMask {
                rect: [5.0, 5.0, 10.0, 10.0],
                pattern: "^DPS".into(),
                issue: "calc#1".into(),
            }],
            ..DiffConfig::default()
        };
        assert_eq!(diff(&a, &d, &off).len(), 1);
        let e = normalize(&frame(vec![img(10.0), text("Life: 1,235")]), &SCREEN);
        assert_eq!(diff(&a, &e, &masked).len(), 1);
    }

    #[test]
    fn diff_effects_in_order() {
        let a = vec![
            SideEffect::WindowTitle("PoB".into()),
            SideEffect::Copy("x".into()),
        ];
        let b = vec![SideEffect::WindowTitle("PoB".into())];
        let d = diff_effects(&a, &b);
        assert_eq!(d.len(), 1);
        assert!(matches!(
            &d[0],
            Divergence::Effect { lua: Some(SideEffect::Copy(s)), rust: None } if s == "x"
        ));
        assert!(diff_effects(&a, &a).is_empty());
        let mut fa = frame(vec![img(0.0)]);
        fa.effects = a.clone();
        let mut fb = frame(vec![img(0.0)]);
        fb.effects = a;
        assert!(diff_frames(&fa, &fb, &DiffConfig::default()).is_empty());
        fb.effects.clear();
        assert_eq!(diff_frames(&fa, &fb, &DiffConfig::default()).len(), 2);
    }

    /// Cross-check against `Sg` itself: random API sequences recorded by `Sg`'s trace normalise
    /// to ops whose colour and viewport are exactly the state `Sg` queued for the same draws.
    #[test]
    fn normalize_agrees_with_sg_queues() {
        let mut r = Rng(42);
        for (fb_w, fb_h, dpi) in [(800, 600, 1.0f32), (800, 600, 1.25), (1600, 1200, 2.0)] {
            let screen = ScreenInfo {
                fb_w,
                fb_h,
                win_h: fb_h,
                dpi_scale: dpi,
            };
            for _ in 0..50 {
                let mut sg = Sg::new(
                    Rc::new(FontSet::empty()),
                    ImageRegistry::new(Box::new(MemLoader::default())),
                );
                sg.trace = Some(TraceRecorder::new());
                sg.begin_frame(screen);
                for _ in 0..60 {
                    match r.below(9) {
                        0 => sg.set_draw_layer(Some(r.below(3) as i32), None),
                        1 => sg.set_draw_layer(None, Some(r.below(4) as i32 - 1)),
                        2 => sg.set_viewport(
                            r.f(0.0, 50.0),
                            r.f(0.0, 50.0),
                            r.f(0.0, 400.0),
                            if r.below(4) == 0 {
                                0.0
                            } else {
                                r.f(1.0, 300.0)
                            },
                        ),
                        3 => sg.reset_viewport(),
                        4 => sg.set_draw_color_a(
                            r.f(0.0, 1.0),
                            r.f(0.0, 1.0),
                            r.f(0.0, 1.0),
                            r.f(0.0, 1.0),
                        ),
                        5 => sg.set_draw_color_esc(["^1", "^xA0B0C0", "^9"][r.below(3) as usize]),
                        6 => sg.draw_string(
                            0.0,
                            0.0,
                            Align::Left,
                            14.0,
                            Font::Var,
                            ["", "x", "^2y", "a^x102030b"][r.below(4) as usize],
                        ),
                        7 => sg.draw_image_quad(None, [0.0, 0.0, 5.0, 0.0, 5.0, 5.0, 0.0, 5.0]),
                        _ => sg.draw_image(None, r.f(0.0, 100.0), r.f(0.0, 100.0), 3.0, 3.0),
                    }
                }
                let frame = sg.end_frame();
                let t = &sg.trace.as_ref().unwrap().frames[0];
                let b = normalize(t, &screen);
                for q in &frame.layers {
                    // The state each QUAD was queued under.
                    let (mut vp, mut col) = (IRect::default(), [0.0f32; 4]);
                    let mut states = Vec::new();
                    for c in &q.cmds {
                        match c {
                            Cmd::Viewport(v) => vp = *v,
                            Cmd::Color(c) => col = *c,
                            Cmd::Bind(_) => {}
                            Cmd::Quad(_) => states.push((vp, col)),
                        }
                    }
                    let ops: Vec<&DrawOp> = b
                        .get(&(q.layer, q.sub_layer))
                        .map(|v| {
                            v.iter()
                                .filter(|o| !matches!(o.call, ApiCall::DrawString { .. }))
                                .collect()
                        })
                        .unwrap_or_default();
                    assert_eq!(ops.len(), states.len());
                    let s = f64::from(dpi);
                    for (op, (vp, col)) in ops.iter().zip(&states) {
                        assert_eq!(op.color, *col);
                        let [x, y, w, h] = op.viewport;
                        let phys = IRect {
                            x: (x * s).round() as i32,
                            y: (y * s).round() as i32,
                            w: (w * s).ceil() as i32,
                            h: (h * s).ceil() as i32,
                        };
                        assert_eq!(phys, *vp);
                    }
                }
                // Every normalised image op has a queue.
                let n_img: usize = b
                    .values()
                    .map(|v| {
                        v.iter()
                            .filter(|o| !matches!(o.call, ApiCall::DrawString { .. }))
                            .count()
                    })
                    .sum();
                let n_quads: usize = frame
                    .layers
                    .iter()
                    .map(|q| q.cmds.iter().filter(|c| matches!(c, Cmd::Quad(_))).count())
                    .sum();
                assert_eq!(n_img, n_quads);
            }
        }
    }
}
