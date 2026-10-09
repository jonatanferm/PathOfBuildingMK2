//! Image handles (`NewImageHandle` / `imgHandle:Load` ...) and the texture
//! registry shared with the GPU backend.
//!
//! Port of SimpleGraphic (master @ 8694a07):
//! - `ui_api.cpp` `l_NewImageHandle`, `imgHandleLoad`, `l_imgHandleUnload`,
//!   `l_imgHandleIsValid`, `l_imgHandleIsLoading`, `l_imgHandleImageSize`,
//!   `l_imgHandleSetLoadingPriority`, `l_GetAsyncCount`;
//! - `engine/render/r_main.cpp` `RegisterShader` (textures are shared between
//!   handles that load the same path, compared case-insensitively, with the
//!   same flags), `GetShaderImageSize`;
//! - `engine/render/r_texture.cpp` `r_tex_c` / `t_manager_c` (4 loader
//!   threads, highest `loadPri` first, ties in queue order; a failed load
//!   uploads the 8x8 `t_defaultTexture` and leaves the size at 0x0;
//!   `BuildMipSet` builds a full mip chain only when `MIPMAP` was given,
//!   since `Load` starts from `TF_NOMIPMAP`).
//!
//! Divergences (documented, deliberate):
//! - `ImageSize()` in SimpleGraphic *blocks* until the loader thread has read
//!   the header. [`Images::image_size`] never blocks and returns `(0, 0)`
//!   until the image is decoded (PoB tolerates this; `PassiveTreeView` checks
//!   for 0). [`Images::image_size_blocking`] gives the SimpleGraphic behaviour.
//! - The size becomes known when decoding finishes rather than after the
//!   header is read.
//! - Mip levels are built with a 2x2 alpha-weighted box filter in linear
//!   light; SimpleGraphic uses `stbir_resize_uint8_srgb_edgemode` (also
//!   sRGB-correct and alpha-weighted, but with stb's default Mitchell filter).
//! - On wasm32 there are no threads; `ASYNC` loads are performed synchronously.

use std::path::{Path, PathBuf};
use std::sync::{Arc, Condvar, Mutex, MutexGuard};

/// Load flags (`TF_*` in r_texture.h; parsed in `imgHandleLoad`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, Default)]
pub struct LoadFlags(u8);

impl LoadFlags {
    pub const NONE: LoadFlags = LoadFlags(0);
    pub const ASYNC: LoadFlags = LoadFlags(1);
    pub const CLAMP: LoadFlags = LoadFlags(2);
    pub const MIPMAP: LoadFlags = LoadFlags(4);
    pub const NEAREST: LoadFlags = LoadFlags(8);

    pub fn contains(self, o: LoadFlags) -> bool {
        self.0 & o.0 == o.0
    }

    /// Parse Lua flag strings (`"ASYNC"`, `"CLAMP"`, `"MIPMAP"`, `"NEAREST"`).
    /// Unknown flags are an error in SimpleGraphic.
    pub fn parse<'a>(flags: impl IntoIterator<Item = &'a str>) -> Result<LoadFlags, String> {
        let mut f = LoadFlags::NONE;
        for s in flags {
            f = f | match s {
                "ASYNC" => LoadFlags::ASYNC,
                "CLAMP" => LoadFlags::CLAMP,
                "MIPMAP" => LoadFlags::MIPMAP,
                "NEAREST" => LoadFlags::NEAREST,
                other => return Err(format!("imgHandle:Load(): unrecognised flag '{other}'")),
            };
        }
        Ok(f)
    }
}

impl std::ops::BitOr for LoadFlags {
    type Output = LoadFlags;
    fn bitor(self, o: LoadFlags) -> LoadFlags {
        LoadFlags(self.0 | o.0)
    }
}

/// A Lua-style image handle (`NewImageHandle()`); cheap to copy.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct ImageHandle(pub u32);

/// A shared texture in the registry (SimpleGraphic `r_shader_c` + `r_tex_c`).
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, PartialOrd, Ord)]
pub struct TextureId(pub u32);

/// Decoded texture data ready for upload: RGBA8, top-down, `levels[0]` is the
/// full-size image, further entries are mip levels (each half size, min 1).
#[derive(Clone, Debug)]
pub struct TextureData {
    pub width: u32,
    pub height: u32,
    pub levels: Vec<Vec<u8>>,
    pub flags: LoadFlags,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Status {
    Queued,
    Processing,
    Done,
}

struct Tex {
    name: String,
    path: PathBuf,
    flags: LoadFlags,
    refcount: u32,
    status: Status,
    width: u32,
    height: u32,
    pri: i32,
    /// Decoded data not yet taken by the GPU backend.
    pending: Option<TextureData>,
    /// In-memory source bytes (for `load_from_memory`).
    bytes: Option<Arc<Vec<u8>>>,
}

#[derive(Default)]
struct Registry {
    base_dir: Option<PathBuf>,
    /// `None` = freed slot, `Some(None)` = handle without image.
    handles: Vec<Option<Option<TextureId>>>,
    textures: Vec<Option<Tex>>,
    queue: Vec<TextureId>,
    freed: Vec<TextureId>,
    #[cfg_attr(target_arch = "wasm32", allow(dead_code))]
    workers_started: bool,
}

struct Shared {
    reg: Mutex<Registry>,
    cv: Condvar,
}

/// The image/texture registry. Clone is cheap (shared).
#[derive(Clone)]
pub struct Images {
    sh: Arc<Shared>,
}

impl Default for Images {
    fn default() -> Self {
        Self::new()
    }
}

impl std::fmt::Debug for Images {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.debug_struct("Images").finish_non_exhaustive()
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

fn default_texture() -> TextureData {
    let mut px = Vec::with_capacity(256);
    for &v in &DEFAULT_TEXTURE {
        px.extend_from_slice(&[v, v, v, 255]);
    }
    TextureData { width: 8, height: 8, levels: vec![px], flags: LoadFlags::NONE }
}

/// Decode an encoded image (png/jpg/webp, and tga via our decoder) into
/// texture data, building mips when requested. `None` on failure.
pub fn decode_image(bytes: &[u8], name: &str, flags: LoadFlags) -> Option<TextureData> {
    let lower = name.to_ascii_lowercase();
    let (w, h, px) = if lower.ends_with(".tga") {
        let img = crate::tga::decode(bytes).ok()?;
        (img.width, img.height, img.pixels)
    } else {
        let img = image::load_from_memory(bytes).ok()?.to_rgba8();
        let (w, h) = img.dimensions();
        (w, h, img.into_raw())
    };
    if w == 0 || h == 0 {
        return None;
    }
    let levels = if flags.contains(LoadFlags::MIPMAP) { build_mips(w, h, px) } else { vec![px] };
    Some(TextureData { width: w, height: h, levels, flags })
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
    let c = if l <= 0.003_130_8 { l * 12.92 } else { 1.055 * l.powf(1.0 / 2.4) - 0.055 };
    (c * 255.0).round().clamp(0.0, 255.0) as u8
}

/// Full mip chain (gli `texture2d_array(format, extent, layers)` allocates
/// `log2(max(w,h)) + 1` levels).
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
                    dst[o + c] = if a_acc > 0.0 { linear_to_srgb(acc[c] / a_acc) } else { 0 };
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

impl Images {
    pub fn new() -> Images {
        Images { sh: Arc::new(Shared { reg: Mutex::new(Registry::default()), cv: Condvar::new() }) }
    }

    fn lock(&self) -> MutexGuard<'_, Registry> {
        self.sh.reg.lock().unwrap_or_else(std::sync::PoisonError::into_inner)
    }

    /// Directory relative paths are resolved against (SimpleGraphic uses the
    /// script working directory, i.e. PoB's `src`).
    pub fn set_base_dir(&self, dir: impl Into<PathBuf>) {
        self.lock().base_dir = Some(dir.into());
    }

    pub fn base_dir(&self) -> Option<PathBuf> {
        self.lock().base_dir.clone()
    }

    /// `NewImageHandle()`.
    pub fn new_handle(&self) -> ImageHandle {
        let mut r = self.lock();
        if let Some(i) = r.handles.iter().position(Option::is_none) {
            r.handles[i] = Some(None);
            ImageHandle(i as u32)
        } else {
            r.handles.push(Some(None));
            ImageHandle(r.handles.len() as u32 - 1)
        }
    }

    fn release_tex(r: &mut Registry, t: TextureId) {
        if let Some(Some(tex)) = r.textures.get_mut(t.0 as usize) {
            tex.refcount = tex.refcount.saturating_sub(1);
            if tex.refcount == 0 {
                r.textures[t.0 as usize] = None;
                r.queue.retain(|&q| q != t);
                r.freed.push(t);
            }
        }
    }

    fn set_handle(r: &mut Registry, h: ImageHandle, t: Option<TextureId>) {
        if let Some(slot) = r.handles.get_mut(h.0 as usize) {
            let old = slot.replace(t).flatten();
            if let Some(old) = old {
                Self::release_tex(r, old);
            }
        }
    }

    /// `imgHandle:Load(fileName, flags...)`. Relative paths are resolved
    /// against the base dir. Without `ASYNC` the image is decoded now.
    pub fn load(&self, h: ImageHandle, path: &str, flags: LoadFlags) {
        let resolved = {
            let r = self.lock();
            let p = Path::new(path);
            match &r.base_dir {
                Some(b) if !p.is_absolute() => b.join(p),
                _ => p.to_path_buf(),
            }
        };
        self.register(h, resolved.to_string_lossy().replace('\\', "/"), resolved, None, flags);
    }

    /// Like [`Images::load`] but from bytes already in memory (wasm, tests).
    /// `name` is used for de-duplication and format sniffing (`.tga`).
    pub fn load_from_memory(&self, h: ImageHandle, name: &str, bytes: Vec<u8>, flags: LoadFlags) {
        self.register(h, name.to_owned(), PathBuf::from(name), Some(Arc::new(bytes)), flags);
    }

    fn register(&self, h: ImageHandle, name: String, path: PathBuf, bytes: Option<Arc<Vec<u8>>>, flags: LoadFlags) {
        if name.is_empty() {
            let mut r = self.lock();
            Self::set_handle(&mut r, h, None);
            return;
        }
        let tid;
        let needs_sync;
        {
            let mut r = self.lock();
            // RegisterShader: reuse an existing texture with the same name
            // (case-insensitive) and flags.
            let existing = r.textures.iter().position(|t| {
                t.as_ref().is_some_and(|t| t.flags == flags && t.name.eq_ignore_ascii_case(&name))
            });
            if let Some(i) = existing {
                let t = TextureId(i as u32);
                r.textures[i].as_mut().unwrap().refcount += 1;
                Self::set_handle(&mut r, h, Some(t));
                return;
            }
            let slot = r.textures.iter().position(Option::is_none).unwrap_or(r.textures.len());
            let tex = Tex {
                name,
                path,
                flags,
                refcount: 1,
                status: Status::Queued,
                width: 0,
                height: 0,
                pri: 0,
                pending: None,
                bytes,
            };
            if slot == r.textures.len() {
                r.textures.push(Some(tex));
            } else {
                r.textures[slot] = Some(tex);
            }
            tid = TextureId(slot as u32);
            Self::set_handle(&mut r, h, Some(tid));
            needs_sync = !flags.contains(LoadFlags::ASYNC) || cfg!(target_arch = "wasm32");
            if !needs_sync {
                r.queue.push(tid);
                self.ensure_workers(&mut r);
            } else {
                r.textures[slot].as_mut().unwrap().status = Status::Processing;
            }
        }
        if needs_sync {
            self.process(tid);
        } else {
            self.sh.cv.notify_one();
        }
    }

    /// Decode texture `t` (status must already be `Processing`).
    fn process(&self, t: TextureId) {
        let (path, name, flags, bytes) = {
            let r = self.lock();
            let Some(Some(tex)) = r.textures.get(t.0 as usize) else { return };
            (tex.path.clone(), tex.name.clone(), tex.flags, tex.bytes.clone())
        };
        let data = match bytes {
            Some(b) => decode_image(&b, &name, flags),
            None => std::fs::read(&path).ok().and_then(|b| decode_image(&b, &name, flags)),
        };
        let mut r = self.lock();
        if let Some(Some(tex)) = r.textures.get_mut(t.0 as usize) {
            if tex.name != name {
                return; // slot was reused while we were decoding
            }
            if let Some(d) = data {
                tex.width = d.width;
                tex.height = d.height;
                tex.pending = Some(d);
            } else {
                tex.width = 0;
                tex.height = 0;
                tex.pending = Some(default_texture());
            }
            tex.status = Status::Done;
            tex.bytes = None;
        }
        drop(r);
        self.sh.cv.notify_all();
    }

    #[cfg(not(target_arch = "wasm32"))]
    fn ensure_workers(&self, r: &mut Registry) {
        if r.workers_started {
            return;
        }
        r.workers_started = true;
        // t_manager_c: runnersWanted = 4.
        for _ in 0..4 {
            let weak = Arc::downgrade(&self.sh);
            std::thread::Builder::new()
                .name("pob-sg-image-loader".into())
                .spawn(move || worker(&weak))
                .ok();
        }
    }

    #[cfg(target_arch = "wasm32")]
    fn ensure_workers(&self, _r: &mut Registry) {}

    /// `imgHandle:Unload()`.
    pub fn unload(&self, h: ImageHandle) {
        let mut r = self.lock();
        Self::set_handle(&mut r, h, None);
    }

    /// Drop a handle entirely (the Lua `__gc`).
    pub fn free_handle(&self, h: ImageHandle) {
        let mut r = self.lock();
        Self::set_handle(&mut r, h, None);
        if let Some(slot) = r.handles.get_mut(h.0 as usize) {
            *slot = None;
        }
    }

    /// `imgHandle:IsValid()`: a texture has been registered (even if loading
    /// or failed).
    pub fn is_valid(&self, h: ImageHandle) -> bool {
        self.texture_of(h).is_some()
    }

    pub fn texture_of(&self, h: ImageHandle) -> Option<TextureId> {
        self.lock().handles.get(h.0 as usize).copied().flatten().flatten()
    }

    /// `imgHandle:IsLoading()`: `width == 0` (also true for failed loads, as in
    /// SimpleGraphic).
    pub fn is_loading(&self, h: ImageHandle) -> bool {
        self.image_size(h).0 == 0
    }

    /// `imgHandle:ImageSize()`, non-blocking: `(0, 0)` until decoded or when
    /// the load failed.
    pub fn image_size(&self, h: ImageHandle) -> (u32, u32) {
        let r = self.lock();
        let Some(t) = r.handles.get(h.0 as usize).copied().flatten().flatten() else { return (0, 0) };
        match r.textures.get(t.0 as usize) {
            Some(Some(tex)) if tex.status == Status::Done => (tex.width, tex.height),
            _ => (0, 0),
        }
    }

    /// `imgHandle:ImageSize()` with SimpleGraphic's blocking semantics.
    pub fn image_size_blocking(&self, h: ImageHandle) -> (u32, u32) {
        let mut r = self.lock();
        loop {
            let Some(t) = r.handles.get(h.0 as usize).copied().flatten().flatten() else { return (0, 0) };
            match r.textures.get(t.0 as usize) {
                Some(Some(tex)) if tex.status == Status::Done => return (tex.width, tex.height),
                Some(Some(_)) => {}
                _ => return (0, 0),
            }
            if cfg!(target_arch = "wasm32") {
                return (0, 0);
            }
            r = self.sh.cv.wait(r).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// `imgHandle:SetLoadingPriority(pri)`: only affects textures not yet done.
    pub fn set_loading_priority(&self, h: ImageHandle, pri: i32) {
        let mut r = self.lock();
        let Some(t) = r.handles.get(h.0 as usize).copied().flatten().flatten() else { return };
        if let Some(Some(tex)) = r.textures.get_mut(t.0 as usize) {
            if tex.status != Status::Done {
                tex.pri = pri;
            }
        }
    }

    /// `GetAsyncCount()`: textures waiting in the loader queue.
    pub fn async_count(&self) -> usize {
        self.lock().queue.len()
    }

    /// Wait until no texture is queued or being decoded (tests, screenshots).
    pub fn wait_idle(&self) {
        let mut r = self.lock();
        loop {
            let busy = !r.queue.is_empty()
                || r.textures.iter().flatten().any(|t| t.status != Status::Done);
            if !busy || cfg!(target_arch = "wasm32") {
                return;
            }
            r = self.sh.cv.wait(r).unwrap_or_else(std::sync::PoisonError::into_inner);
        }
    }

    /// Decoded textures not yet handed to the GPU backend.
    pub fn take_uploads(&self) -> Vec<(TextureId, TextureData)> {
        let mut r = self.lock();
        let mut out = Vec::new();
        for (i, t) in r.textures.iter_mut().enumerate() {
            if let Some(t) = t {
                if let Some(d) = t.pending.take() {
                    out.push((TextureId(i as u32), d));
                }
            }
        }
        out
    }

    /// Texture ids released since the last call (GPU resources can be freed).
    pub fn drain_freed(&self) -> Vec<TextureId> {
        std::mem::take(&mut self.lock().freed)
    }

    /// Whether `t` has finished loading (SimpleGraphic binds a transparent
    /// black texture for textures that are not `DONE`).
    pub fn is_done(&self, t: TextureId) -> bool {
        matches!(self.lock().textures.get(t.0 as usize), Some(Some(tex)) if tex.status == Status::Done)
    }
}

#[cfg(not(target_arch = "wasm32"))]
fn worker(weak: &std::sync::Weak<Shared>) {
    loop {
        let Some(sh) = weak.upgrade() else { return };
        let job = {
            let mut r = sh.reg.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
            if r.queue.is_empty() {
                let (g, _) = sh
                    .cv
                    .wait_timeout(r, std::time::Duration::from_millis(100))
                    .unwrap_or_else(std::sync::PoisonError::into_inner);
                r = g;
            }
            // ThreadProc: pick the highest loadPri; ties keep queue order.
            let mut best: Option<(usize, i32)> = None;
            for (qi, t) in r.queue.iter().enumerate() {
                let pri = r.textures.get(t.0 as usize).and_then(Option::as_ref).map_or(0, |t| t.pri);
                if best.map_or(true, |(_, bp)| pri > bp) {
                    best = Some((qi, pri));
                }
            }
            best.map(|(qi, _)| {
                let t = r.queue.remove(qi);
                if let Some(Some(tex)) = r.textures.get_mut(t.0 as usize) {
                    tex.status = Status::Processing;
                }
                t
            })
        };
        if let Some(t) = job {
            let images = Images { sh };
            images.process(t);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn png_bytes(w: u32, h: u32) -> Vec<u8> {
        let img = image::RgbaImage::from_fn(w, h, |x, y| image::Rgba([x as u8, y as u8, 7, 255]));
        let mut out = std::io::Cursor::new(Vec::new());
        img.write_to(&mut out, image::ImageFormat::Png).unwrap();
        out.into_inner()
    }

    #[test]
    fn sync_load_and_dedup() {
        let images = Images::new();
        let a = images.new_handle();
        assert!(!images.is_valid(a));
        assert_eq!(images.image_size(a), (0, 0));
        images.load_from_memory(a, "x.png", png_bytes(5, 3), LoadFlags::NONE);
        assert!(images.is_valid(a));
        assert_eq!(images.image_size(a), (5, 3));
        assert!(!images.is_loading(a));
        let b = images.new_handle();
        images.load_from_memory(b, "X.PNG", png_bytes(5, 3), LoadFlags::NONE);
        assert_eq!(images.texture_of(a), images.texture_of(b));
        let c = images.new_handle();
        images.load_from_memory(c, "x.png", png_bytes(5, 3), LoadFlags::CLAMP);
        assert_ne!(images.texture_of(a), images.texture_of(c));
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 2);
        images.unload(a);
        assert!(images.drain_freed().is_empty());
        images.unload(b);
        assert_eq!(images.drain_freed().len(), 1);
    }

    #[test]
    fn failed_load_is_zero_size_default_texture() {
        let images = Images::new();
        let a = images.new_handle();
        images.load(a, "/definitely/not/here.png", LoadFlags::NONE);
        assert!(images.is_valid(a));
        assert_eq!(images.image_size(a), (0, 0));
        assert!(images.is_loading(a));
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 1);
        assert_eq!((ups[0].1.width, ups[0].1.height), (8, 8));
    }

    #[test]
    fn async_load_and_mips() {
        let images = Images::new();
        let a = images.new_handle();
        images.load_from_memory(a, "m.png", png_bytes(8, 4), LoadFlags::ASYNC | LoadFlags::MIPMAP);
        let size = images.image_size_blocking(a);
        assert_eq!(size, (8, 4));
        images.wait_idle();
        let ups = images.take_uploads();
        assert_eq!(ups[0].1.levels.len(), 4); // 8x4, 4x2, 2x1, 1x1
        assert_eq!(ups[0].1.levels[3].len(), 4);
    }

    #[test]
    fn flags_parse() {
        let f = LoadFlags::parse(["CLAMP", "MIPMAP"]).unwrap();
        assert!(f.contains(LoadFlags::CLAMP) && f.contains(LoadFlags::MIPMAP) && !f.contains(LoadFlags::ASYNC));
        assert!(LoadFlags::parse(["BOGUS"]).is_err());
    }
}
