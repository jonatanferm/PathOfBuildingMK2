//! Image handles and the texture registry (ui doc §4.5). Owner: U3.
//!
//! Ports (SimpleGraphic research copy master @ 8694a07; U3 re-verifies at `3b1a346`):
//! - `ui_api.cpp:377-485` `l_NewImageHandle`, `imgHandleLoad` (flag parsing), `l_imgHandleUnload`,
//!   `l_imgHandleIsValid`, `l_imgHandleIsLoading`, `l_imgHandleImageSize`,
//!   `l_imgHandleSetLoadingPriority`;
//! - `engine/render/r_main.cpp:1366-1433` `RegisterShader` (textures are shared between loads of
//!   the same path, compared case-insensitively, with the same flags) and `GetShaderImageSize`
//!   (blocks until the size is known);
//! - `engine/render/r_texture.cpp` `t_defaultTexture`, `@white`, `@black`, `BuildMipSet`.
//!
//! Moved from the pre-port `pob-sg/src/images.rs` (Arc/Mutex registry with built-in loader
//! threads) by UI-U0: the registry is now single-threaded and the decoding work goes through
//! the [`ImageLoader`] trait (native thread pool, web VFS and headless loaders live in
//! `pob-host`). `decode_image` / `build_mips` / `default_texture` are the moved code.
//!
//! Deltas U3 closes (ui doc §2, §4.5): mips are still built with the moved sRGB-correct,
//! alpha-weighted 2x2 filter, while §4.5 asks for a box filter on raw (gamma) values;
//! `unload` does not release the texture (the frozen `ImageHandle` has no registry access,
//! see CCR-0013).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::api::Sg;

bitflags::bitflags! {
    /// Load flags (`TF_*`; parsed in `imgHandleLoad`).
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub struct ImageFlags: u8 { const ASYNC = 1; const CLAMP = 2; const MIPMAP = 4; const NEAREST = 8; }
}

impl ImageFlags {
    /// Lua flag strings (`"ASYNC"`, `"CLAMP"`, `"MIPMAP"`, `"NEAREST"`); an unknown flag is a Lua
    /// error in SG (`Err`).
    pub fn from_lua(flags: &[&str]) -> Result<ImageFlags, String> {
        let mut f = ImageFlags::empty();
        for s in flags {
            f |= match *s {
                "ASYNC" => ImageFlags::ASYNC,
                "CLAMP" => ImageFlags::CLAMP,
                "MIPMAP" => ImageFlags::MIPMAP,
                "NEAREST" => ImageFlags::NEAREST,
                other => return Err(format!("imgHandle:Load(): unrecognised flag '{other}'")),
            };
        }
        Ok(f)
    }
}

/// A registered texture (SG `r_shader_c` + `r_tex_c`).
#[derive(
    Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord, serde::Serialize, serde::Deserialize,
)]
pub struct TexId(pub u32);

impl TexId {
    /// `@white`: 8x8 grey 0xFF (`DrawImage(nil, ...)`).
    pub const WHITE: TexId = TexId(0);
    /// `@black`: 8x8 RGBA all zero, bound for textures that are not uploaded yet.
    pub const BLACK: TexId = TexId(1);
    /// `t_defaultTexture`: the 8x8 grey pattern a failed load gets.
    pub const DEFAULT: TexId = TexId(2);
}

/// `r_tex_c` status.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexStatus {
    Init,
    InQueue,
    Processing,
    SizeKnown,
    PendingUpload,
    Done,
}

/// Decoded pixels ready for upload: RGBA8, top-down; `levels[0]` is the full image, then mips.
#[derive(Clone, Debug, PartialEq)]
pub struct ImageData {
    pub width: u32,
    pub height: u32,
    /// RGBA8, mip 0..
    pub levels: Vec<Vec<u8>>,
    /// CCR-0013: the sampler state (CLAMP / NEAREST) the backend needs travels with the data.
    pub flags: ImageFlags,
}

/// What a loader reports.
pub enum LoadEvent {
    Size(TexId, u32, u32),
    Done(TexId, ImageData),
    Failed(TexId),
}

/// Implemented in pob-host: native thread pool, web (prefetched VFS + lazy fetch), headless.
pub trait ImageLoader {
    /// Non-ASYNC (`sync == true`): must return Done/Failed immediately. ASYNC: may return nothing.
    fn request(
        &mut self,
        id: TexId,
        path: &str,
        flags: ImageFlags,
        sync: bool,
        priority: i32,
    ) -> Option<LoadEvent>;
    /// Header-only size read; used by `image_size` to emulate SG's blocking ImageSize (§2).
    fn read_size(&mut self, id: TexId, path: &str) -> Option<(u32, u32)>;
    fn set_priority(&mut self, id: TexId, priority: i32);
    fn poll(&mut self) -> Vec<LoadEvent>;
}

struct TexInfo {
    path: String,
    flags: ImageFlags,
    status: TexStatus,
    width: u32,
    height: u32,
}

/// The texture registry: `by_key` dedups `(path, flags)` like `RegisterShader`.
pub struct ImageRegistry {
    infos: Vec<TexInfo>,
    by_key: HashMap<(String, ImageFlags), TexId>,
    loader: Box<dyn ImageLoader>,
    uploads: Vec<(TexId, Rc<ImageData>)>,
    releases: Vec<TexId>,
}

impl ImageRegistry {
    /// A registry over `loader`, with `@white`, `@black` and the default texture registered
    /// (their pixels are the first uploads).
    pub fn new(loader: Box<dyn ImageLoader>) -> ImageRegistry {
        let mut r = ImageRegistry {
            infos: Vec::new(),
            by_key: HashMap::new(),
            loader,
            uploads: Vec::new(),
            releases: Vec::new(),
        };
        let white = ImageData {
            width: 8,
            height: 8,
            levels: vec![vec![255u8; 8 * 8 * 4]],
            flags: ImageFlags::empty(),
        };
        let black = ImageData {
            width: 8,
            height: 8,
            levels: vec![vec![0u8; 8 * 8 * 4]],
            flags: ImageFlags::empty(),
        };
        assert_eq!(r.insert_ready("@white", white), TexId::WHITE);
        assert_eq!(r.insert_ready("@black", black), TexId::BLACK);
        assert_eq!(
            r.insert_ready("@default", default_texture()),
            TexId::DEFAULT
        );
        r
    }

    /// Register already-decoded pixels (font atlases, built-ins): status Done, upload pending.
    pub fn insert_ready(&mut self, name: &str, data: ImageData) -> TexId {
        let id = TexId(self.infos.len() as u32);
        self.infos.push(TexInfo {
            path: name.to_owned(),
            flags: data.flags,
            status: TexStatus::Done,
            width: data.width,
            height: data.height,
        });
        self.uploads.push((id, Rc::new(data)));
        id
    }

    /// `imgHandle:Load(fileName, flags...)`: `RegisterShader` dedup, then a load request
    /// (synchronous unless ASYNC).
    pub fn load(&mut self, handle: &ImageHandle, path: &str, flags: ImageFlags) {
        if path.is_empty() {
            handle.0.set(None);
            return;
        }
        let key = (path.replace('\\', "/").to_ascii_lowercase(), flags);
        if let Some(&id) = self.by_key.get(&key) {
            handle.0.set(Some(id));
            return;
        }
        let id = TexId(self.infos.len() as u32);
        self.infos.push(TexInfo {
            path: path.to_owned(),
            flags,
            status: TexStatus::InQueue,
            width: 0,
            height: 0,
        });
        self.by_key.insert(key, id);
        handle.0.set(Some(id));
        let sync = !flags.contains(ImageFlags::ASYNC);
        if let Some(ev) = self.loader.request(id, path, flags, sync, 0) {
            self.apply(ev);
        }
    }

    fn apply(&mut self, ev: LoadEvent) {
        match ev {
            LoadEvent::Size(id, w, h) => {
                let i = &mut self.infos[id.0 as usize];
                if !matches!(i.status, TexStatus::Done | TexStatus::PendingUpload) {
                    i.width = w;
                    i.height = h;
                    i.status = TexStatus::SizeKnown;
                }
            }
            LoadEvent::Done(id, data) => {
                let i = &mut self.infos[id.0 as usize];
                i.width = data.width;
                i.height = data.height;
                i.status = TexStatus::Done;
                self.uploads.push((id, Rc::new(data)));
            }
            LoadEvent::Failed(id) => {
                // r_texture.cpp: the default texture is uploaded, fileWidth stays 0.
                let i = &mut self.infos[id.0 as usize];
                i.width = 0;
                i.height = 0;
                i.status = TexStatus::Done;
                self.uploads.push((id, Rc::new(default_texture())));
            }
        }
    }

    /// Poll the loader (`Sg::begin_frame`).
    pub fn pump(&mut self) {
        for ev in self.loader.poll() {
            self.apply(ev);
        }
    }

    pub fn status(&self, id: TexId) -> TexStatus {
        self.infos
            .get(id.0 as usize)
            .map_or(TexStatus::Init, |i| i.status)
    }

    /// What `id` was loaded from (trace `ImgRef`).
    pub fn source(&self, id: TexId) -> Option<(&str, ImageFlags)> {
        self.infos
            .get(id.0 as usize)
            .map(|i| (i.path.as_str(), i.flags))
    }

    /// The size the registry knows (no blocking).
    pub fn tex_size(&self, id: TexId) -> Option<(u32, u32)> {
        self.infos.get(id.0 as usize).map(|i| (i.width, i.height))
    }

    /// `GetShaderImageSize`: blocks (reads the header through the loader) until the size is
    /// known; `(0, 0)` for a missing file.
    pub fn image_size(&mut self, id: TexId) -> (i32, i32) {
        self.pump();
        let Some(i) = self.infos.get(id.0 as usize) else {
            return (0, 0);
        };
        if matches!(
            i.status,
            TexStatus::SizeKnown | TexStatus::PendingUpload | TexStatus::Done
        ) {
            return (i.width as i32, i.height as i32);
        }
        let path = i.path.clone();
        match self.loader.read_size(id, &path) {
            Some((w, h)) => {
                self.apply(LoadEvent::Size(id, w, h));
                (w as i32, h as i32)
            }
            None => (0, 0),
        }
    }

    /// `SetLoadingPriority`: only textures not yet done.
    pub fn set_priority(&mut self, id: TexId, pri: i32) {
        if self.status(id) != TexStatus::Done {
            self.loader.set_priority(id, pri);
        }
    }

    /// Pixels that became ready since the last call.
    pub fn take_uploads(&mut self) -> Vec<(TexId, Rc<ImageData>)> {
        std::mem::take(&mut self.uploads)
    }

    /// Textures released since the last call.
    pub fn take_releases(&mut self) -> Vec<TexId> {
        std::mem::take(&mut self.releases)
    }
}

/// imgHandle userdata. Clone shares the slot (Lua aliasing): Load/Unload through any alias is seen by all.
#[derive(Clone, Default)]
pub struct ImageHandle(pub(crate) Rc<Cell<Option<TexId>>>);

impl std::fmt::Debug for ImageHandle {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "ImageHandle({:?})", self.0.get())
    }
}

impl ImageHandle {
    /// `imgHandle:Load(fileName, ...)`: relative paths resolve against the script dir (src/).
    pub fn load(&self, sg: &mut Sg, path: &str, flags: ImageFlags) {
        sg.images.load(self, path, flags);
    }
    /// `imgHandle:Unload()`.
    pub fn unload(&self) {
        self.0.set(None);
    }
    /// `imgHandle:IsValid()`: `hnd != NULL`.
    pub fn is_valid(&self) -> bool {
        self.0.get().is_some()
    }
    /// `imgHandle:IsLoading()`: `image_size().0 == 0`.
    pub fn is_loading(&self, sg: &mut Sg) -> bool {
        self.image_size(sg).0 == 0
    }
    /// Blocks (reads the header) until the size is known, like SG; (0,0) for a missing file.
    /// Calling it on an unloaded handle is a Lua error.
    pub fn image_size(&self, sg: &mut Sg) -> (i32, i32) {
        let id = self.0.get().unwrap_or_else(|| {
            pob_lua::lua_error!("imgHandle:ImageSize(): image handle has no image loaded")
        });
        sg.images.image_size(id)
    }
    /// `imgHandle:SetLoadingPriority(pri)`.
    pub fn set_loading_priority(&self, sg: &mut Sg, pri: i32) {
        if let Some(id) = self.0.get() {
            sg.images.set_priority(id, pri);
        }
    }
    pub fn tex(&self) -> Option<TexId> {
        self.0.get()
    }
}

/// `t_defaultTexture` (r_texture.cpp): 8x8 grey pattern used for failed loads.
const DEFAULT_TEXTURE: [u8; 64] = [
    0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, //
    0x7F, 0x7F, 0x7F, 0x00, 0x00, 0x00, 0x7F, 0x7F, //
    0x7F, 0x7F, 0x7F, 0x00, 0x00, 0x00, 0x00, 0x7F, //
    0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, //
    0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, //
    0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, //
    0x7F, 0x00, 0x00, 0x00, 0x00, 0x00, 0x00, 0x7F, //
    0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, 0x7F, //
];

/// The default texture's pixels (grey expanded to RGBA).
pub fn default_texture() -> ImageData {
    let mut px = Vec::with_capacity(256);
    for &v in &DEFAULT_TEXTURE {
        px.extend_from_slice(&[v, v, v, 255]);
    }
    ImageData {
        width: 8,
        height: 8,
        levels: vec![px],
        flags: ImageFlags::empty(),
    }
}

/// Decode an encoded image (png/jpg/webp, and tga via our decoder) into texture data, building
/// mips when requested. `None` on failure.
pub fn decode_image(bytes: &[u8], name: &str, flags: ImageFlags) -> Option<ImageData> {
    let lower = name.to_ascii_lowercase();
    let (w, h, px) = if lower.ends_with(".tga") {
        let img = crate::font::tga::decode(bytes).ok()?;
        (img.width, img.height, img.pixels)
    } else {
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        (w, h, img.into_raw())
    };
    if w == 0 || h == 0 {
        return None;
    }
    let levels = if flags.contains(ImageFlags::MIPMAP) {
        build_mips(w, h, px)
    } else {
        vec![px]
    };
    Some(ImageData {
        width: w,
        height: h,
        levels,
        flags,
    })
}

/// Header-only size of an encoded image (`None` if it is not a readable image).
pub fn read_image_size(bytes: &[u8], name: &str) -> Option<(u32, u32)> {
    if name.to_ascii_lowercase().ends_with(".tga") {
        let hdr = crate::font::tga::read_header(bytes).ok()?;
        return Some((u32::from(hdr.width), u32::from(hdr.height)));
    }
    image::ImageReader::new(std::io::Cursor::new(bytes))
        .with_guessed_format()
        .ok()?
        .into_dimensions()
        .ok()
}

fn srgb_to_linear(v: u8) -> f32 {
    let c = f32::from(v) / 255.0;
    if c <= 0.04045 {
        c / 12.92
    } else {
        ((c + 0.055) / 1.055).powf(2.4)
    }
}

fn linear_to_srgb(l: f32) -> u8 {
    let c = if l <= 0.003_130_8 {
        l * 12.92
    } else {
        1.055 * l.powf(1.0 / 2.4) - 0.055
    };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Full mip chain (gli `texture2d_array(format, extent, layers)` allocates
/// `log2(max(w,h)) + 1` levels). Moved code: sRGB-correct alpha-weighted 2x2 filter (U3 delta).
pub fn build_mips(w: u32, h: u32, base: Vec<u8>) -> Vec<Vec<u8>> {
    let lut: Vec<f32> = (0..=255u8).map(srgb_to_linear).collect();
    let mut levels = vec![base];
    let (mut cw, mut ch) = (w, h);
    while cw > 1 || ch > 1 {
        let (nw, nh) = ((cw / 2).max(1), (ch / 2).max(1));
        let src = levels.last().unwrap();
        let mut dst = vec![0u8; (nw * nh * 4) as usize];
        for y in 0..nh {
            for x in 0..nw {
                let mut acc = [0.0f32; 3];
                let mut a_acc = 0.0f32;
                let mut n = 0.0f32;
                for dy in 0..2 {
                    for dx in 0..2 {
                        let sx = (x * 2 + dx).min(cw - 1);
                        let sy = (y * 2 + dy).min(ch - 1);
                        let i = ((sy * cw + sx) * 4) as usize;
                        let a = f32::from(src[i + 3]) / 255.0;
                        for c in 0..3 {
                            acc[c] += lut[src[i + c] as usize] * a;
                        }
                        a_acc += a;
                        n += 1.0;
                    }
                }
                let o = ((y * nw + x) * 4) as usize;
                for c in 0..3 {
                    dst[o + c] = if a_acc > 0.0 {
                        linear_to_srgb(acc[c] / a_acc)
                    } else {
                        0
                    };
                }
                dst[o + 3] = (a_acc / n * 255.0).round() as u8;
            }
        }
        levels.push(dst);
        cw = nw;
        ch = nh;
    }
    levels
}

#[cfg(test)]
pub(crate) mod tests {
    use super::*;

    /// Synchronous in-memory loader for tests (deterministic mode).
    #[derive(Default)]
    pub(crate) struct MemLoader(pub HashMap<String, Vec<u8>>);

    impl ImageLoader for MemLoader {
        fn request(
            &mut self,
            id: TexId,
            path: &str,
            flags: ImageFlags,
            _sync: bool,
            _pri: i32,
        ) -> Option<LoadEvent> {
            Some(
                match self.0.get(path).and_then(|b| decode_image(b, path, flags)) {
                    Some(d) => LoadEvent::Done(id, d),
                    None => LoadEvent::Failed(id),
                },
            )
        }
        fn read_size(&mut self, _id: TexId, path: &str) -> Option<(u32, u32)> {
            self.0.get(path).and_then(|b| read_image_size(b, path))
        }
        fn set_priority(&mut self, _id: TexId, _priority: i32) {}
        fn poll(&mut self) -> Vec<LoadEvent> {
            Vec::new()
        }
    }

    pub(crate) fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn sync_load_and_dedup() {
        let mut files = HashMap::new();
        files.insert("x.png".to_owned(), png_bytes(5, 3));
        files.insert("X.PNG".to_owned(), png_bytes(5, 3));
        let mut images = ImageRegistry::new(Box::new(MemLoader(files)));
        assert_eq!(images.take_uploads().len(), 3); // @white, @black, default
        let a = ImageHandle::default();
        assert!(!a.is_valid());
        images.load(&a, "x.png", ImageFlags::empty());
        assert!(a.is_valid());
        assert_eq!(images.image_size(a.tex().unwrap()), (5, 3));
        let b = ImageHandle::default();
        images.load(&b, "X.PNG", ImageFlags::empty());
        assert_eq!(a.tex(), b.tex()); // RegisterShader: case-insensitive name + same flags
        let c = ImageHandle::default();
        images.load(&c, "x.png", ImageFlags::CLAMP);
        assert_ne!(a.tex(), c.tex());
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 2);
        assert_eq!(ups[1].1.flags, ImageFlags::CLAMP);
        // Aliases share the slot (Lua userdata): Unload through one is seen by the other.
        let alias = a.clone();
        alias.unload();
        assert!(!a.is_valid());
    }

    #[test]
    fn failed_load_is_zero_size_default_texture() {
        let mut images = ImageRegistry::new(Box::new(MemLoader::default()));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "/definitely/not/here.png", ImageFlags::empty());
        assert!(a.is_valid());
        assert_eq!(images.image_size(a.tex().unwrap()), (0, 0));
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 1);
        assert_eq!((ups[0].1.width, ups[0].1.height), (8, 8));
        assert_eq!(*ups[0].1, default_texture());
    }

    #[test]
    fn flags_parse() {
        let f = ImageFlags::from_lua(&["CLAMP", "MIPMAP"]).unwrap();
        assert!(
            f.contains(ImageFlags::CLAMP)
                && f.contains(ImageFlags::MIPMAP)
                && !f.contains(ImageFlags::ASYNC)
        );
        assert!(ImageFlags::from_lua(&["BOGUS"]).is_err());
    }
}
