//! Shared helpers for the `font_*` integration tests (U2, ui doc T2). Included by the other test
//! files with `#[path = "font_util.rs"] mod font_util;`; as its own test target it has no tests.
#![allow(dead_code)]

use pob_sg::font::FontFace;
use pob_sg::image::{ImageLoader, LoadEvent};
use pob_sg::{AssetSource, Font, FontSet, ImageFlags, ImageRegistry, Quad, QuadSink, TexId};
use std::fmt::Write as _;
use std::path::PathBuf;

/// File-system AssetSource over a PoB checkout (pob-sg itself has no std::fs).
pub struct DirSource {
    pub src: PathBuf,
    pub runtime: PathBuf,
}

impl AssetSource for DirSource {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.src.join(path))
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.runtime.join(path))
    }
}

/// `.PathOfBuilding` of the main checkout (or `$POB_ROOT`), found from this crate's directory.
pub fn locate() -> Option<DirSource> {
    let mut starts = Vec::new();
    if let Some(p) = std::env::var_os("POB_ROOT") {
        starts.push(PathBuf::from(p));
    }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    for s in starts {
        for d in s.ancestors() {
            for root in [d.to_path_buf(), d.join(".PathOfBuilding")] {
                if root
                    .join("runtime/SimpleGraphic/Fonts/Liberation Sans.tgf")
                    .is_file()
                {
                    return Some(DirSource {
                        src: root.join("src"),
                        runtime: root.join("runtime/SimpleGraphic"),
                    });
                }
            }
        }
    }
    None
}

/// The font atlases are registered with `insert_ready`; nothing goes through a loader.
struct NoLoader;

impl ImageLoader for NoLoader {
    fn request(&mut self, _: TexId, _: &str, _: ImageFlags, _: bool, _: i32) -> Option<LoadEvent> {
        None
    }
    fn read_size(&mut self, _: TexId, _: &str) -> Option<(u32, u32)> {
        None
    }
    fn set_priority(&mut self, _: TexId, _: i32) {}
    fn poll(&mut self) -> Vec<LoadEvent> {
        Vec::new()
    }
}

/// The 7 faces and the registry holding their 112 atlases. The PoB runtime assets are required:
/// a missing checkout fails the test (set `POB_ROOT`).
pub fn fonts_and_images() -> (FontSet, ImageRegistry) {
    let src =
        locate().expect("PoB runtime fonts not found: set POB_ROOT to a PathOfBuilding checkout");
    let mut images = ImageRegistry::new(Box::new(NoLoader));
    let fs = FontSet::load(&src, &mut images).expect("load fonts");
    (fs, images)
}

pub fn fonts() -> FontSet {
    fonts_and_images().0
}

/// The face and atlas height a texture belongs to.
pub fn atlas_of(fs: &FontSet, t: TexId) -> (Font, i32) {
    for f in Font::ALL {
        for h in fs.face(f).heights() {
            if h.tex == t {
                return (f, h.height);
            }
        }
    }
    panic!("{t:?} is not a font atlas")
}

/// One recorded SG layer command.
#[derive(Clone, Debug, PartialEq)]
pub enum Ev {
    Bind(TexId),
    Color([f32; 4]),
    Quad(Quad),
}

/// Records the BIND / COLOR / QUAD stream a draw emits.
#[derive(Default)]
pub struct Rec(pub Vec<Ev>);

impl QuadSink for Rec {
    fn bind(&mut self, t: TexId) {
        self.0.push(Ev::Bind(t));
    }
    fn color(&mut self, c: [f32; 4]) {
        self.0.push(Ev::Color(c));
    }
    fn quad(&mut self, q: Quad) {
        self.0.push(Ev::Quad(q));
    }
}

/// `fontMap` names (ui_api.cpp) to faces.
pub fn font(name: &str) -> Font {
    Font::from_lua(name).unwrap_or_else(|| panic!("font {name}"))
}

/// `rfont_ref.py` `canon`: the canonical text form of a command stream (f32 bit patterns).
pub fn canon(fs: &FontSet, evs: &[Ev]) -> String {
    let mut s = String::new();
    for e in evs {
        match e {
            Ev::Color(c) => {
                s.push('C');
                for v in c {
                    let _ = write!(s, "{:08x}", v.to_bits());
                }
                s.push(';');
            }
            Ev::Bind(t) => {
                let (f, h) = atlas_of(fs, *t);
                let _ = write!(s, "B{}.{h};", f.index());
            }
            Ev::Quad(q) => {
                s.push('Q');
                for a in [q.s, q.t, q.x, q.y] {
                    for v in a {
                        let _ = write!(s, "{:08x}", v.to_bits());
                    }
                }
                s.push(';');
            }
        }
    }
    s
}

/// FNV-1a 64 (`rfont_ref.py` `fnv1a64`).
pub fn fnv1a64(s: &str) -> String {
    let mut h: u64 = 0xcbf2_9ce4_8422_2325;
    for b in s.bytes() {
        h ^= u64::from(b);
        h = h.wrapping_mul(0x0000_0100_0000_01b3);
    }
    format!("{h:016x}")
}

/// The glyph of an atlas (face, height) for a codepoint.
pub fn glyph(face: &FontFace, height: i32, cp: u32) -> pob_sg::font::Glyph {
    let fh = face
        .heights()
        .iter()
        .find(|h| h.height == height)
        .unwrap_or_else(|| panic!("no atlas {height}"));
    *fh.glyph(cp)
}
