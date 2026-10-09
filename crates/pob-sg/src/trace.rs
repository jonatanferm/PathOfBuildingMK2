//! API-level draw traces (ui doc §4.6 trace.rs, §11.1 T3, §11.2). Types and the recorder are
//! complete in UI-U0; the bodies of `normalize` and `diff` are U1's.

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

/// Replays SetDrawLayer/SetViewport/SetDrawColor semantics, including the DrawString escape leak
/// (the colour after a DrawString is the last escape in the text with alpha 1, else unchanged).
pub fn normalize(t: &FrameTrace, screen: &ScreenInfo) -> Buckets {
    let _ = (t, screen);
    todo!("U1: trace normalize (ui doc §4.6)")
}

/// Comparison settings.
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
pub struct TextMask {
    pub rect: [f64; 4],
    pub pattern: String,
    pub issue: String,
}

/// One difference between the Lua and the Rust trace.
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

pub fn diff(lua: &Buckets, rust: &Buckets, cfg: &DiffConfig) -> Vec<Divergence> {
    let _ = (lua, rust, cfg);
    todo!("U1: trace diff (ui doc §4.6)")
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
