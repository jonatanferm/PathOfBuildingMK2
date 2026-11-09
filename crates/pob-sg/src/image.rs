//! Image handles and the texture registry (ui doc §4.5). Owner: U3.
//!
//! Port of PathOfBuilding-SimpleGraphic **`3b1a346`** (the commit the shipped DLL identifies as,
//! ui doc §2; re-verified by U3, the research copy was master):
//! - `ui_api.cpp:362-482` `imgHandle_s`, `l_NewImageHandle`, `GetImgHandle`, `imgHandleLoad`
//!   (flag parsing, `scriptWorkDir` join), `l_imgHandleUnload`, `l_imgHandleIsValid`,
//!   `l_imgHandleIsLoading`, `l_imgHandleSetLoadingPriority`, `l_imgHandleImageSize`;
//! - `engine/render/r_main.cpp:45-100` `r_shader_c`, `r_shaderHnd_c` (refcount);
//!   `:1574-1659` `PurgeShaders`, `RegisterShader` (dedup), `GetShaderImageSize` (blocks until
//!   `SIZE_KNOWN`), `SetShaderLoadingPriority`; `:1150-1159` `PumpShaders` (EndFrame);
//! - `engine/render/r_texture.cpp:21-43` built-in textures, `:298-338` `r_tex_c` ctor/`Init`,
//!   `:365-384` `StartLoad`/`AbortLoad`/`ForceLoad`, `:386-464` `BuildMipSet` (mips through
//!   `stbir_resize_uint8_srgb_edgemode`), `:553-608` `LoadFile`;
//! - `engine/core/core_image.cpp:38-76` `image_c::CopyRaw` (32768 limit), `:93-141`
//!   `LoaderForFile` (format detection), and the per-format loaders (`targa_c`, `jpeg_c`, `png_c`,
//!   `webp_c`: channel counts and where the size callback fires);
//! - `dep/stb/stb_image_resize.h` v0.97 (the subset `stbir_resize_uint8_srgb_edgemode` with
//!   `STBIR_EDGE_CLAMP` reaches when downsampling: Mitchell filter, sRGB tables, alpha
//!   premultiplication with `STBIR_ALPHA_EPSILON`), in `mod stbir`.
//!
//! Moved from the pre-port `pob-sg/src/images.rs` by UI-U0 (single-threaded registry; decoding
//! goes through the [`ImageLoader`] trait, whose native/web/headless implementations live in
//! `pob-host`).
//!
//! Decisions and divergences (U3; CCR-0023 records the contract side):
//! - **`ImageHandle::unload(&self)` keeps its frozen signature; textures live until exit** (ui doc
//!   §13 item 10). SG refcounts shaders (`r_shaderHnd_c`) and `PurgeShaders` frees a texture at
//!   EndFrame once no handle references it, but a Lua handle only lets go when the Lua GC collects
//!   it (`l_imgHandleGC`), at a nondeterministic time. Rust drops are immediate, so eager purging
//!   would re-decode images every frame at PoB sites that create a handle per draw
//!   (`ExtBuildListControl.lua:195`, `Tooltip.lua:508-511`). PoB never calls `Unload`. The only
//!   visible difference is memory (and `AbortLoad` of an ASYNC load whose last handle went away
//!   is not modelled: the load completes). `Frame::releases` therefore stays empty.
//!   // DIVERGENCE(texture lifetime: kept until exit instead of purged when unreferenced)
//! - **Dedup key** (`RegisterShader`, r_main.cpp:1585-1614): SG matches an existing shader when the
//!   16-bit `StringHash` of the full name (case-SENSITIVE, common.cpp:345), `_stricmp` and the
//!   flags all agree, so a name that differs only in letter case is a different texture unless the
//!   two hashes happen to collide, which depends on the absolute `scriptWorkDir` prefix. The
//!   registry keys on `(path with '\\' -> '/', flags)`, compared exactly (the UI-U0 code folded
//!   case, which was wrong). Paths are keyed as passed, not joined with `scriptWorkDir`: an
//!   absolute path naming the same file as a relative one gets its own texture in the port. PoB
//!   only passes relative paths. // DIVERGENCE(dedup: exact compare, no 16-bit hash collisions;
//!   key is the path as passed)
//! - **Re-registration** runs `ForceLoad` (r_texture.cpp:376-384): a texture whose size is still
//!   unknown (a failed load, or an ASYNC load not yet past its header) is loaded again
//!   synchronously, on the calling thread. A failed texture is therefore retried on every `Load`.
//! - **Empty path**: `imgHandleLoad` joins `scriptWorkDir / ""` (always non-empty in PoB), so
//!   `Load("")` registers a texture for the script directory itself, whose load fails: a valid
//!   handle, the default texture, `ImageSize() == (0, 0)`, `IsLoading() == true`. The UI-U0 code
//!   cleared the handle instead (that is `RegisterShader("")` without a work dir, never PoB's case).
//! - **Unknown flag**: SG deletes the old shader handle *before* it validates the flags
//!   (ui_api.cpp:406-427), leaving a dangling pointer in the handle. [`ImageFlags::from_lua`] runs
//!   before [`ImageRegistry::load`], so a caller (the oracle bridge) that wants SG's state after
//!   the error clears the handle (`unload`) before raising it.
//! - **Mips** are `BuildMipSet`'s: each level is `stbir_resize_uint8_srgb_edgemode` of the previous
//!   level (sRGB-correct, alpha-weighted when the image has an alpha channel, Mitchell filter,
//!   clamp edges), level extents `max(1, e >> level)`. Ui doc §4.5 asked for a 2x2 box filter on
//!   gamma values; the pinned SG source says otherwise (CCR-0023). PoB itself never passes
//!   `"MIPMAP"` (PassiveTree.lua:263, :308 have it commented out).
//! - **Decoders**: PNG, JPEG and WebP decode through the `image` crate, not SG's stb_image v2.26
//!   (`dep/stb`) and libwebp. Measured by U3 on the 131 images under `src/Assets` and
//!   `src/TreeData/3_27` against stb_image v2.26 and libwebp 1.6 (`WebPDecodeRGBA`): all 99 PNGs
//!   and both WebPs are byte-identical (16-bit PNG channels are reduced with `>> 8` like
//!   `stbi__convert_16_to_8`); the 30 JPEGs differ by at most 4 levels per channel, by more than
//!   2 levels on at most 0.1% of a file's pixels (different IDCT/chroma upsampling rounding).
//!   // DIVERGENCE(JPEG decoder rounding, <= 4/255)
//!   GIF and DDS (`.dds`, `.dds.zst`) are detected like `LoaderForFile` but not decoded (the
//!   `image` crate is built without them and PoB ships none): they fail like a corrupt file.
//!   // DIVERGENCE(no GIF/DDS decoding)
//! - **Upload timing**: completions are applied at `pump` (Sg::begin_frame) and again before the
//!   frame's uploads are taken ([`ImageRegistry::take_uploads`] = `PumpShaders` in EndFrame), so
//!   an ASYNC texture that finishes during a frame is drawn in that frame, as in SG.
//! - Texture ids are never reused (SG reuses free `shaderList` slots; ids are internal).

use std::cell::Cell;
use std::collections::HashMap;
use std::rc::Rc;

use crate::api::Sg;

bitflags::bitflags! {
    /// Load flags (`TF_*`; parsed in `imgHandleLoad`). `MIPMAP` is the absence of SG's
    /// `TF_NOMIPMAP`.
    #[derive(Clone, Copy, Debug, PartialEq, Eq, Hash, serde::Serialize, serde::Deserialize)]
    pub struct ImageFlags: u8 { const ASYNC = 1; const CLAMP = 2; const MIPMAP = 4; const NEAREST = 8; }
}

impl ImageFlags {
    /// Lua flag strings (`"ASYNC"`, `"CLAMP"`, `"MIPMAP"`, `"NEAREST"`, case-sensitive); an
    /// unknown flag is a Lua error in SG (`Err` with SG's message). Non-string arguments are
    /// skipped by SG (`if (!lua_isstring(L, f)) continue;`): the caller does not pass them.
    ///
    /// lua: ui_api.cpp:406-428 (imgHandleLoad flag loop)
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

/// `r_tex_c::Status` (r_texture.h), in SG's order.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum TexStatus {
    Init,
    InQueue,
    Processing,
    SizeKnown,
    PendingUpload,
    Done,
}

/// `status >= SIZE_KNOWN` (the `GetShaderImageSize` wait condition).
fn size_known(s: TexStatus) -> bool {
    matches!(
        s,
        TexStatus::SizeKnown | TexStatus::PendingUpload | TexStatus::Done
    )
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
    /// The header was parsed (the image loader's size callback, `SIZE_KNOWN`).
    Size(TexId, u32, u32),
    /// Decoded (and mipmapped when asked).
    Done(TexId, ImageData),
    /// Missing, unsupported or corrupt: the default texture is used.
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
    /// `Some` exactly when SG's per-format loader would have called its size callback (see
    /// [`read_image_size`]).
    fn read_size(&mut self, id: TexId, path: &str) -> Option<(u32, u32)>;
    fn set_priority(&mut self, id: TexId, priority: i32);
    fn poll(&mut self) -> Vec<LoadEvent>;
}

struct TexInfo {
    /// The path as passed to `Load` (trace `ImgRef`, loader requests).
    path: String,
    flags: ImageFlags,
    status: TexStatus,
    /// `fileWidth`/`fileHeight`: 0 until the size callback fires.
    width: u32,
    height: u32,
}

/// The texture registry (SG `shaderList` + `t_manager_c`): `by_key` dedups `(path, flags)` like
/// `RegisterShader`.
pub struct ImageRegistry {
    infos: Vec<TexInfo>,
    by_key: HashMap<(String, ImageFlags), TexId>,
    loader: Box<dyn ImageLoader>,
    uploads: Vec<(TexId, Rc<ImageData>)>,
    releases: Vec<TexId>,
}

/// The registry key of a path: `std::filesystem::path::generic_u8string` turns Windows
/// separators into `/`; the comparison is otherwise exact (see the module header).
fn shader_key(path: &str) -> String {
    path.replace('\\', "/")
}

impl ImageRegistry {
    /// A registry over `loader`, with `@white`, `@black` and the default texture registered
    /// (their pixels are the first uploads).
    ///
    /// lua: r_main.cpp:1080-1081 (whiteImage/blackImage), r_texture.cpp:21-43, :553-570
    pub fn new(loader: Box<dyn ImageLoader>) -> ImageRegistry {
        let mut r = ImageRegistry {
            infos: Vec::new(),
            by_key: HashMap::new(),
            loader,
            uploads: Vec::new(),
            releases: Vec::new(),
        };
        // t_whiteImage is IMGTYPE_GRAY 0xFF (sampled as opaque white).
        let white = ImageData {
            width: 8,
            height: 8,
            levels: vec![vec![255u8; 8 * 8 * 4]],
            flags: ImageFlags::empty(),
        };
        // t_blackImage is IMGTYPE_RGBA all zero (transparent).
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
    /// Not part of the `(path, flags)` dedup (SG's font atlases are separate `r_tex_c`s).
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

    /// `imgHandle:Load(fileName, flags...)`: the handle drops its old shader reference, then
    /// `RegisterShader` (dedup + `ForceLoad`, or a new texture whose load starts: synchronously
    /// unless ASYNC).
    ///
    /// lua: ui_api.cpp:394-433 (imgHandleLoad), r_main.cpp:1585-1614 (RegisterShader)
    pub fn load(&mut self, handle: &ImageHandle, path: &str, flags: ImageFlags) {
        // delete imgHandle->hnd;
        handle.0.set(None);
        // imgHandle->hnd = ui->renderer->RegisterShader(fileName.generic_u8string(), flags);
        let id = self.register_shader(path, flags);
        handle.0.set(Some(id));
    }

    /// lua: r_main.cpp:1585-1614
    fn register_shader(&mut self, path: &str, flags: ImageFlags) -> TexId {
        let key = (shader_key(path), flags);
        if let Some(&id) = self.by_key.get(&key) {
            // Shader already exists, return a new handle for it.
            // Ensure texture is loaded as soon as possible.
            self.force_load(id);
            return id;
        }
        let id = TexId(self.infos.len() as u32);
        // r_tex_c::Init: status = INIT, fileWidth = fileHeight = 0.
        self.infos.push(TexInfo {
            path: path.to_owned(),
            flags,
            status: TexStatus::Init,
            width: 0,
            height: 0,
        });
        self.by_key.insert(key, id);
        // r_tex_c ctor: StartLoad() (ASYNC -> AsyncAdd: IN_QUEUE); if (status == INIT) LoadFile().
        let sync = !flags.contains(ImageFlags::ASYNC);
        if !sync {
            self.infos[id.0 as usize].status = TexStatus::InQueue;
        }
        if let Some(ev) = self.loader.request(id, path, flags, sync, 0) {
            self.apply(ev);
        }
        id
    }

    /// `r_tex_c::ForceLoad`: a texture still at INIT or whose size is unknown (`fileWidth == 0`:
    /// failed, or queued/processing before its header) is loaded now, on the calling thread.
    ///
    /// lua: r_texture.cpp:376-384
    fn force_load(&mut self, id: TexId) {
        let i = &self.infos[id.0 as usize];
        if i.status == TexStatus::Init || i.width == 0 {
            let (path, flags) = (i.path.clone(), i.flags);
            if let Some(ev) = self.loader.request(id, &path, flags, true, 0) {
                self.apply(ev);
            }
        }
    }

    fn apply(&mut self, ev: LoadEvent) {
        match ev {
            LoadEvent::Size(id, w, h) => {
                let Some(i) = self.infos.get_mut(id.0 as usize) else {
                    return;
                };
                if !size_known(i.status) {
                    i.width = w;
                    i.height = h;
                    i.status = TexStatus::SizeKnown;
                }
            }
            LoadEvent::Done(id, data) => {
                let Some(i) = self.infos.get_mut(id.0 as usize) else {
                    return;
                };
                i.width = data.width;
                i.height = data.height;
                i.status = TexStatus::Done;
                self.uploads.push((id, Rc::new(data)));
            }
            LoadEvent::Failed(id) => {
                let Some(i) = self.infos.get(id.0 as usize) else {
                    return;
                };
                // LoadFile's fallback: the default texture is uploaded with TF_NOMIPMAP (so the
                // requested CLAMP/NEAREST do not apply) and status = DONE. fileWidth/fileHeight
                // keep whatever the size callback set before the error (a readable header in a
                // corrupt file), else 0.
                let path = i.path.clone();
                let size = self.loader.read_size(id, &path).unwrap_or((0, 0));
                let i = &mut self.infos[id.0 as usize];
                i.width = size.0;
                i.height = size.1;
                i.status = TexStatus::Done;
                self.uploads.push((id, Rc::new(default_texture())));
            }
        }
    }

    /// Poll the loader (`Sg::begin_frame`; also `PumpShaders`, see [`Self::take_uploads`]).
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

    /// `GetShaderImageSize`: SG sleeps until the texture reaches `SIZE_KNOWN`; the port reads the
    /// header through the loader in place of waiting for the worker's size callback. `(0, 0)` for
    /// a missing or unreadable file.
    ///
    /// lua: r_main.cpp:1638-1652
    pub fn image_size(&mut self, id: TexId) -> (i32, i32) {
        self.pump();
        let Some(i) = self.infos.get(id.0 as usize) else {
            return (0, 0);
        };
        if size_known(i.status) {
            return (i.width as i32, i.height as i32);
        }
        let path = i.path.clone();
        match self.loader.read_size(id, &path) {
            Some((w, h)) => {
                self.apply(LoadEvent::Size(id, w, h));
                (w as i32, h as i32)
            }
            // The load is going to fail: SG waits for it and reads fileWidth == 0.
            None => (0, 0),
        }
    }

    /// `SetShaderLoadingPriority`: only textures not yet done.
    ///
    /// lua: r_main.cpp:1654-1659
    pub fn set_priority(&mut self, id: TexId, pri: i32) {
        if self.status(id) != TexStatus::Done {
            self.loader.set_priority(id, pri);
        }
    }

    /// Pixels that became ready since the last call. Polls the loader first: SG's EndFrame runs
    /// `PumpShaders` (pending uploads become DONE) before it renders.
    ///
    /// lua: r_main.cpp:1150-1159 (PumpShaders), :1264-1267
    pub fn take_uploads(&mut self) -> Vec<(TexId, Rc<ImageData>)> {
        self.pump();
        std::mem::take(&mut self.uploads)
    }

    /// Textures released since the last call. Always empty: textures live until exit (module
    /// header, ui doc §13 item 10).
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
    /// `GetImgHandle(L, ui, method, true)`: the method needs a loaded handle.
    ///
    /// lua: ui_api.cpp:375-384
    #[track_caller]
    fn loaded(&self, method: &str) -> TexId {
        self.0.get().unwrap_or_else(|| {
            pob_lua::lua_error!("imgHandle:{method}(): image handle has no image loaded")
        })
    }

    /// `imgHandle:Load(fileName, ...)`: relative paths resolve against the script dir (src/)
    /// in the loader.
    ///
    /// lua: ui_api.cpp:394-433
    pub fn load(&self, sg: &mut Sg, path: &str, flags: ImageFlags) {
        sg.images.load(self, path, flags);
    }

    /// `imgHandle:Unload()`: `delete hnd; hnd = NULL`. The texture itself stays registered until
    /// exit (module header; ui doc §13 item 10).
    ///
    /// lua: ui_api.cpp:435-442
    pub fn unload(&self) {
        self.0.set(None);
    }

    /// `imgHandle:IsValid()`: `hnd != NULL`.
    ///
    /// lua: ui_api.cpp:444-450
    pub fn is_valid(&self) -> bool {
        self.0.get().is_some()
    }

    /// `imgHandle:IsLoading()`: `GetShaderImageSize(...); width == 0` (so it blocks like
    /// `ImageSize`, and a missing file stays "loading" forever). Unloaded handle: Lua error.
    ///
    /// lua: ui_api.cpp:452-460
    #[track_caller]
    pub fn is_loading(&self, sg: &mut Sg) -> bool {
        let id = self.loaded("IsLoading");
        sg.images.image_size(id).0 == 0
    }

    /// Blocks (reads the header) until the size is known, like SG; (0,0) for a missing file.
    /// Calling it on an unloaded handle is a Lua error.
    ///
    /// lua: ui_api.cpp:473-482
    #[track_caller]
    pub fn image_size(&self, sg: &mut Sg) -> (i32, i32) {
        let id = self.loaded("ImageSize");
        sg.images.image_size(id)
    }

    /// `imgHandle:SetLoadingPriority(pri)` (the caller converts with `(int)lua_tointeger`).
    /// Unloaded handle: Lua error.
    ///
    /// lua: ui_api.cpp:462-471
    #[track_caller]
    pub fn set_loading_priority(&self, sg: &mut Sg, pri: i32) {
        let id = self.loaded("SetLoadingPriority");
        sg.images.set_priority(id, pri);
    }

    pub fn tex(&self) -> Option<TexId> {
        self.0.get()
    }
}

/// `t_defaultTexture` (r_texture.cpp:34-43): 8x8 grey pattern used for failed loads.
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

/// The default texture's pixels (IMGTYPE_GRAY expanded to RGBA).
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

/// The `image_c` subclass `LoaderForFile` picks.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum ImgFormat {
    Targa,
    Jpeg,
    Png,
    Gif,
    Dds,
    Webp,
}

/// `std::filesystem::path::extension()` of the last component (case-sensitive, includes the dot;
/// a leading dot alone is not an extension).
fn extension(name: &str) -> &str {
    let file = name.rsplit(['/', '\\']).next().unwrap_or(name);
    if file == ".." {
        return "";
    }
    match file.rfind('.') {
        Some(0) | None => "",
        Some(i) => &file[i..],
    }
}

/// `image_c::LoaderForFile`: by extension first (`.dds`, `.dds.zst`, `.webp`), then by the first
/// four bytes. `None`: SG warns and the load fails.
///
/// lua: core_image.cpp:93-141
fn loader_for_file(bytes: &[u8], name: &str) -> Option<ImgFormat> {
    let ext = extension(name);
    if ext == ".zst" {
        let inner = &name[..name.len() - 4];
        if extension(inner) == ".dds" {
            return Some(ImgFormat::Dds);
        }
    }
    if ext == ".dds" {
        return Some(ImgFormat::Dds);
    }
    if ext == ".webp" {
        return Some(ImgFormat::Webp);
    }
    // Attempt to detect image file type from first 4 bytes of file
    let dat: [u8; 4] = bytes.get(..4)?.try_into().ok()?;
    let word = u32::from_le_bytes(dat);
    if dat[0] == 0xFF && dat[1] == 0xD8 {
        Some(ImgFormat::Jpeg)
    } else if word == 0x474E_5089 {
        Some(ImgFormat::Png)
    } else if word == 0x3846_4947 {
        Some(ImgFormat::Gif)
    } else if word == 0x2053_4444 {
        Some(ImgFormat::Dds)
    } else if word == 0x4646_4952 {
        Some(ImgFormat::Webp)
    } else if (dat[1] == 0 && matches!(dat[2], 2 | 3 | 10 | 11))
        || (dat[1] == 1 && matches!(dat[2], 1 | 9))
    {
        Some(ImgFormat::Targa)
    } else {
        None
    }
}

/// Pixels as SG's loader leaves them (`CopyRaw`): `comp` is 1 (L8), 3 (RGB8) or 4 (RGBA8),
/// expanded here to RGBA8.
struct Raw {
    width: u32,
    height: u32,
    comp: u8,
    rgba: Vec<u8>,
}

fn image_format(f: ImgFormat) -> Option<image::ImageFormat> {
    match f {
        ImgFormat::Png => Some(image::ImageFormat::Png),
        ImgFormat::Jpeg => Some(image::ImageFormat::Jpeg),
        ImgFormat::Webp => Some(image::ImageFormat::WebP),
        ImgFormat::Targa | ImgFormat::Gif | ImgFormat::Dds => None,
    }
}

fn decode_with_image(bytes: &[u8], fmt: image::ImageFormat) -> Option<image::DynamicImage> {
    let mut r = image::ImageReader::with_format(std::io::Cursor::new(bytes), fmt);
    r.no_limits();
    r.decode().ok()
}

/// 16-bit channels to 8 bits the way `stbi__convert_16_to_8` does (`>> 8`).
fn hi8(v: &[u16]) -> Vec<u8> {
    v.iter().map(|&c| (c >> 8) as u8).collect()
}

/// Expand `comp`-channel 8-bit pixels to RGBA8 (grey -> v,v,v; missing alpha -> 255).
fn expand(px: &[u8], comp: usize) -> Vec<u8> {
    let mut out = Vec::with_capacity(px.len() / comp * 4);
    for p in px.chunks_exact(comp) {
        match comp {
            1 => out.extend_from_slice(&[p[0], p[0], p[0], 255]),
            2 => out.extend_from_slice(&[p[0], p[0], p[0], p[1]]),
            3 => out.extend_from_slice(&[p[0], p[1], p[2], 255]),
            _ => out.extend_from_slice(&p[..4]),
        }
    }
    out
}

/// `png_c::Load` (core_image.cpp:353-389): 1/3-channel files load as RGB, 2/4-channel as RGBA.
fn decode_png(bytes: &[u8]) -> Option<Raw> {
    use image::DynamicImage as D;
    let img = decode_with_image(bytes, image::ImageFormat::Png)?;
    let (width, height) = (img.width(), img.height());
    let (comp, rgba) = match &img {
        D::ImageLuma8(b) => (3, expand(b.as_raw(), 1)),
        D::ImageLumaA8(b) => (4, expand(b.as_raw(), 2)),
        D::ImageRgb8(b) => (3, expand(b.as_raw(), 3)),
        D::ImageRgba8(b) => (4, b.as_raw().clone()),
        D::ImageLuma16(b) => (3, expand(&hi8(b.as_raw()), 1)),
        D::ImageLumaA16(b) => (4, expand(&hi8(b.as_raw()), 2)),
        D::ImageRgb16(b) => (3, expand(&hi8(b.as_raw()), 3)),
        D::ImageRgba16(b) => (4, hi8(b.as_raw())),
        other => (4, other.to_rgba8().into_raw()),
    };
    Some(Raw {
        width,
        height,
        comp,
        rgba,
    })
}

/// `jpeg_c::Load` (core_image.cpp:285-322): grey loads as L8, colour as RGB.
fn decode_jpeg(bytes: &[u8]) -> Option<Raw> {
    use image::DynamicImage as D;
    let img = decode_with_image(bytes, image::ImageFormat::Jpeg)?;
    let (width, height) = (img.width(), img.height());
    let (comp, rgba) = match &img {
        D::ImageLuma8(b) => (1, expand(b.as_raw(), 1)),
        D::ImageRgb8(b) => (3, expand(b.as_raw(), 3)),
        other => (3, expand(other.to_rgb8().as_raw(), 3)),
    };
    Some(Raw {
        width,
        height,
        comp,
        rgba,
    })
}

/// `webp_c::Load` (core_image.cpp:496-521): `WebPDecodeRGBA`, always 4 channels.
fn decode_webp(bytes: &[u8]) -> Option<Raw> {
    let img = decode_with_image(bytes, image::ImageFormat::WebP)?;
    Some(Raw {
        width: img.width(),
        height: img.height(),
        comp: 4,
        rgba: img.to_rgba8().into_raw(),
    })
}

/// `targa_c::Load` (core_image.cpp:162-254) through the font module's decoder (same port).
fn decode_tga(bytes: &[u8]) -> Option<Raw> {
    let hdr = crate::font::tga::read_header(bytes).ok()?;
    let img = crate::font::tga::decode(bytes).ok()?;
    Some(Raw {
        width: img.width,
        height: img.height,
        comp: hdr.depth >> 3,
        rgba: img.pixels,
    })
}

/// Decode an encoded image into texture data (`LoadFile`: `LoaderForFile`, the format's `Load`,
/// `CopyRaw`, then `BuildMipSet` when `MIPMAP` was asked). `None` on failure (missing loader,
/// decode error, a side of 32768 px or more).
///
/// lua: r_texture.cpp:553-608 (LoadFile), :386-464 (BuildMipSet); core_image.cpp:38-76 (CopyRaw)
pub fn decode_image(bytes: &[u8], name: &str, flags: ImageFlags) -> Option<ImageData> {
    let raw = match loader_for_file(bytes, name)? {
        ImgFormat::Png => decode_png(bytes)?,
        ImgFormat::Jpeg => decode_jpeg(bytes)?,
        ImgFormat::Webp => decode_webp(bytes)?,
        ImgFormat::Targa => decode_tga(bytes)?,
        // DIVERGENCE(no GIF/DDS decoding): see the module header.
        ImgFormat::Gif | ImgFormat::Dds => return None,
    };
    // CopyRaw: if (inWidth >= (1 << 15) || inHeight >= (1 << 15)) return false;
    if raw.width >= (1 << 15) || raw.height >= (1 << 15) || raw.width == 0 || raw.height == 0 {
        return None;
    }
    let levels = if flags.contains(ImageFlags::MIPMAP) {
        build_mip_chain(raw.width, raw.height, raw.rgba, raw.comp == 4)
    } else {
        vec![raw.rgba]
    };
    Some(ImageData {
        width: raw.width,
        height: raw.height,
        levels,
        flags,
    })
}

/// Header-only size of an encoded image: `Some` exactly when SG's loader for this file would call
/// its size callback (`SIZE_KNOWN`): PNG/JPEG after `stbi_info`, WebP after `WebPGetInfo`, TGA
/// after a header without a colour map; GIF only after a full decode and DDS after loading (both
/// unsupported here, so `None`); an unrecognised format never.
pub fn read_image_size(bytes: &[u8], name: &str) -> Option<(u32, u32)> {
    read_image_size_from(std::io::Cursor::new(bytes), name)
}

/// [`read_image_size`] over a reader (the native loader reads only the header from disk).
pub fn read_image_size_from<R: std::io::BufRead + std::io::Seek>(
    mut r: R,
    name: &str,
) -> Option<(u32, u32)> {
    let mut head = [0u8; 18];
    let n = read_up_to(&mut r, &mut head);
    r.seek(std::io::SeekFrom::Start(0)).ok()?;
    let fmt = loader_for_file(&head[..n], name)?;
    if fmt == ImgFormat::Targa {
        // targa_c::Load: header read, colour-mapped images rejected, then the size callback.
        let hdr = crate::font::tga::read_header(&head[..n]).ok()?;
        if hdr.color_map_type != 0 {
            return None;
        }
        return Some((u32::from(hdr.width), u32::from(hdr.height)));
    }
    let f = image_format(fmt)?;
    let mut rd = image::ImageReader::with_format(r, f);
    rd.no_limits();
    rd.into_dimensions().ok()
}

fn read_up_to(r: &mut impl std::io::Read, buf: &mut [u8]) -> usize {
    let mut n = 0;
    while n < buf.len() {
        match r.read(&mut buf[n..]) {
            Ok(0) | Err(_) => break,
            Ok(k) => n += k,
        }
    }
    n
}

/// Full mip chain of an RGBA8 image with an alpha channel (`BuildMipSet` for a 4-channel
/// texture). The pob-sg-wgpu backend uses it for downscaled oversize textures.
pub fn build_mips(w: u32, h: u32, base: Vec<u8>) -> Vec<Vec<u8>> {
    build_mip_chain(w, h, base, true)
}

/// `BuildMipSet`'s generation branch: gli allocates `floor(log2(max(w, h))) + 1` levels with
/// extents `max(1, e >> level)`, and each level is `stbir_resize_uint8_srgb_edgemode` of the
/// previous one, with alpha channel 3 when the texture has 4 components, else none.
/// `base` is RGBA8; without alpha its A channel is 255 and stays 255 (SG has no alpha channel to
/// filter; the colour channels are filtered independently, so 4-channel processing with
/// `STBIR_ALPHA_CHANNEL_NONE` gives the 1- and 3-channel results).
///
/// lua: r_texture.cpp:433-461
fn build_mip_chain(w: u32, h: u32, base: Vec<u8>, has_alpha: bool) -> Vec<Vec<u8>> {
    let num_levels = 32 - w.max(h).leading_zeros();
    let mut levels = vec![base];
    for level in 1..num_levels {
        let (sw, sh) = ((w >> (level - 1)).max(1), (h >> (level - 1)).max(1));
        let (dw, dh) = ((w >> level).max(1), (h >> level).max(1));
        let src = levels.last().expect("level 0");
        let mut dst = vec![0u8; (dw * dh * 4) as usize];
        stbir::resize_uint8_srgb_clamp(
            src,
            sw as usize,
            sh as usize,
            &mut dst,
            dw as usize,
            dh as usize,
            4,
            if has_alpha { Some(3) } else { None },
        );
        if !has_alpha {
            for px in dst.chunks_exact_mut(4) {
                px[3] = 255;
            }
        }
        levels.push(dst);
    }
    levels
}

/// The part of `stb_image_resize.h` v0.97 (SG `dep/stb`) that
/// `stbir_resize_uint8_srgb_edgemode(..., STBIR_EDGE_CLAMP)` runs when neither axis is enlarged
/// (`scale <= 1`, so both axes use the downsampling path and the default Mitchell filter).
/// All arithmetic is `f32` in the C statement order (SG builds with MSVC x64, SSE scalar floats,
/// no contraction); `+ 0.5` against a `double` literal is done in `f64` like the C promotion.
pub(crate) mod stbir {
    /// `STBIR_ALPHA_EPSILON`: `(float)1 / (1 << 20) / (1 << 20) / (1 << 20) / (1 << 20)` = 2^-80.
    const ALPHA_EPSILON: f32 = 1.0 / 1_048_576.0 / 1_048_576.0 / 1_048_576.0 / 1_048_576.0;

    #[rustfmt::skip]
    #[allow(clippy::unreadable_literal, clippy::excessive_precision)]
    const SRGB_UCHAR_TO_LINEAR_FLOAT: [f32; 256] = [
        0.000000, 0.000304, 0.000607, 0.000911, 0.001214, 0.001518, 0.001821, 0.002125,
        0.002428, 0.002732, 0.003035, 0.003347, 0.003677, 0.004025, 0.004391, 0.004777,
        0.005182, 0.005605, 0.006049, 0.006512, 0.006995, 0.007499, 0.008023, 0.008568,
        0.009134, 0.009721, 0.010330, 0.010960, 0.011612, 0.012286, 0.012983, 0.013702,
        0.014444, 0.015209, 0.015996, 0.016807, 0.017642, 0.018500, 0.019382, 0.020289,
        0.021219, 0.022174, 0.023153, 0.024158, 0.025187, 0.026241, 0.027321, 0.028426,
        0.029557, 0.030713, 0.031896, 0.033105, 0.034340, 0.035601, 0.036889, 0.038204,
        0.039546, 0.040915, 0.042311, 0.043735, 0.045186, 0.046665, 0.048172, 0.049707,
        0.051269, 0.052861, 0.054480, 0.056128, 0.057805, 0.059511, 0.061246, 0.063010,
        0.064803, 0.066626, 0.068478, 0.070360, 0.072272, 0.074214, 0.076185, 0.078187,
        0.080220, 0.082283, 0.084376, 0.086500, 0.088656, 0.090842, 0.093059, 0.095307,
        0.097587, 0.099899, 0.102242, 0.104616, 0.107023, 0.109462, 0.111932, 0.114435,
        0.116971, 0.119538, 0.122139, 0.124772, 0.127438, 0.130136, 0.132868, 0.135633,
        0.138432, 0.141263, 0.144128, 0.147027, 0.149960, 0.152926, 0.155926, 0.158961,
        0.162029, 0.165132, 0.168269, 0.171441, 0.174647, 0.177888, 0.181164, 0.184475,
        0.187821, 0.191202, 0.194618, 0.198069, 0.201556, 0.205079, 0.208637, 0.212231,
        0.215861, 0.219526, 0.223228, 0.226966, 0.230740, 0.234551, 0.238398, 0.242281,
        0.246201, 0.250158, 0.254152, 0.258183, 0.262251, 0.266356, 0.270498, 0.274677,
        0.278894, 0.283149, 0.287441, 0.291771, 0.296138, 0.300544, 0.304987, 0.309469,
        0.313989, 0.318547, 0.323143, 0.327778, 0.332452, 0.337164, 0.341914, 0.346704,
        0.351533, 0.356400, 0.361307, 0.366253, 0.371238, 0.376262, 0.381326, 0.386430,
        0.391573, 0.396755, 0.401978, 0.407240, 0.412543, 0.417885, 0.423268, 0.428691,
        0.434154, 0.439657, 0.445201, 0.450786, 0.456411, 0.462077, 0.467784, 0.473532,
        0.479320, 0.485150, 0.491021, 0.496933, 0.502887, 0.508881, 0.514918, 0.520996,
        0.527115, 0.533276, 0.539480, 0.545725, 0.552011, 0.558340, 0.564712, 0.571125,
        0.577581, 0.584078, 0.590619, 0.597202, 0.603827, 0.610496, 0.617207, 0.623960,
        0.630757, 0.637597, 0.644480, 0.651406, 0.658375, 0.665387, 0.672443, 0.679543,
        0.686685, 0.693872, 0.701102, 0.708376, 0.715694, 0.723055, 0.730461, 0.737911,
        0.745404, 0.752942, 0.760525, 0.768151, 0.775822, 0.783538, 0.791298, 0.799103,
        0.806952, 0.814847, 0.822786, 0.830770, 0.838799, 0.846873, 0.854993, 0.863157,
        0.871367, 0.879622, 0.887923, 0.896269, 0.904661, 0.913099, 0.921582, 0.930111,
        0.938686, 0.947307, 0.955974, 0.964686, 0.973445, 0.982251, 0.991102, 1.0,
    ];

    #[rustfmt::skip]
    #[allow(clippy::unreadable_literal)]
    const FP32_TO_SRGB8_TAB4: [u32; 104] = [
        0x0073000d, 0x007a000d, 0x0080000d, 0x0087000d, 0x008d000d, 0x0094000d, 0x009a000d, 0x00a1000d,
        0x00a7001a, 0x00b4001a, 0x00c1001a, 0x00ce001a, 0x00da001a, 0x00e7001a, 0x00f4001a, 0x0101001a,
        0x010e0033, 0x01280033, 0x01410033, 0x015b0033, 0x01750033, 0x018f0033, 0x01a80033, 0x01c20033,
        0x01dc0067, 0x020f0067, 0x02430067, 0x02760067, 0x02aa0067, 0x02dd0067, 0x03110067, 0x03440067,
        0x037800ce, 0x03df00ce, 0x044600ce, 0x04ad00ce, 0x051400ce, 0x057b00c5, 0x05dd00bc, 0x063b00b5,
        0x06970158, 0x07420142, 0x07e30130, 0x087b0120, 0x090b0112, 0x09940106, 0x0a1700fc, 0x0a9500f2,
        0x0b0f01cb, 0x0bf401ae, 0x0ccb0195, 0x0d950180, 0x0e56016e, 0x0f0d015e, 0x0fbc0150, 0x10630143,
        0x11070264, 0x1238023e, 0x1357021d, 0x14660201, 0x156601e9, 0x165a01d3, 0x174401c0, 0x182401af,
        0x18fe0331, 0x1a9602fe, 0x1c1502d2, 0x1d7e02ad, 0x1ed4028d, 0x201a0270, 0x21520256, 0x227d0240,
        0x239f0443, 0x25c003fe, 0x27bf03c4, 0x29a10392, 0x2b6a0367, 0x2d1d0341, 0x2ebe031f, 0x304d0300,
        0x31d105b0, 0x34a80555, 0x37520507, 0x39d504c5, 0x3c37048b, 0x3e7c0458, 0x40a8042a, 0x42bd0401,
        0x44c20798, 0x488e071e, 0x4c1c06b6, 0x4f76065d, 0x52a50610, 0x55ac05cc, 0x5892058f, 0x5b590559,
        0x5e0c0a23, 0x631c0980, 0x67db08f6, 0x6c55087f, 0x70940818, 0x74a007bd, 0x787d076c, 0x7c330723,
    ];

    /// `stbir__linear_to_srgb_uchar` (the IEEE table version).
    #[allow(clippy::neg_cmp_op_on_partial_ord)] // `!(in > minval)` maps NaN to 0, as in C
    pub(crate) fn linear_to_srgb_uchar(mut v: f32) -> u8 {
        let almostone = f32::from_bits(0x3f7f_ffff); // 1-eps
        let minval = f32::from_bits((127 - 13) << 23);
        // Clamp to [2^(-13), 1-eps]; these two values map to 0 and 1, respectively.
        // The tests are carefully written so that NaNs map to 0.
        if !(v > minval) {
            v = minval;
        }
        if v > almostone {
            v = almostone;
        }
        let u = v.to_bits();
        let tab = FP32_TO_SRGB8_TAB4[((u - minval.to_bits()) >> 20) as usize];
        let bias = (tab >> 16) << 9;
        let scale = tab & 0xffff;
        let t = (u >> 12) & 0xff;
        ((bias + scale * t) >> 16) as u8
    }

    /// `stbir__srgb_uchar_to_linear_float[v]`.
    #[cfg(test)]
    pub(crate) fn decode_entry(v: u8) -> f32 {
        SRGB_UCHAR_TO_LINEAR_FLOAT[v as usize]
    }

    /// `stbir__filter_mitchell`.
    fn filter_mitchell(x: f32) -> f32 {
        let x = x.abs();
        if x < 1.0 {
            (16.0 + x * x * (21.0 * x - 36.0)) / 18.0
        } else if x < 2.0 {
            (32.0 + x * (-60.0 + x * (36.0 - 7.0 * x))) / 18.0
        } else {
            0.0
        }
    }

    /// `stbir__support_two` (Mitchell).
    const SUPPORT: f32 = 2.0;

    /// Contributors and coefficients of one axis (`stbir__calculate_filters`, downsampling).
    struct Axis {
        scale: f32,
        margin: i32,
        coefficient_width: usize,
        /// (n0, n1) per input pixel `-margin..input + margin` (index `n + margin`).
        contributors: Vec<(i32, i32)>,
        coefficients: Vec<f32>,
    }

    /// `stbir__calculate_sample_range_downsample`: (out_first, out_last, out_center_of_in).
    fn sample_range_downsample(n: i32, in_pixels_radius: f32, scale: f32) -> (i32, i32, f32) {
        let shift = 0.0f32; // s0 = t0 = 0
        let in_pixel_center = n as f32 + 0.5;
        let in_lb = in_pixel_center - in_pixels_radius;
        let in_ub = in_pixel_center + in_pixels_radius;
        let out_lb = in_lb * scale - shift;
        let out_ub = in_ub * scale - shift;
        let out_center_of_in = in_pixel_center * scale - shift;
        (
            (f64::from(out_lb) + 0.5).floor() as i32,
            (f64::from(out_ub) - 0.5).floor() as i32,
            out_center_of_in,
        )
    }

    impl Axis {
        /// `stbir__calculate_filters` + `stbir__normalize_downsample_coefficients` for
        /// `input_size -> output_size`, `scale = (float)output / input` (<= 1).
        fn new(input_size: usize, output_size: usize) -> Axis {
            let scale = output_size as f32 / input_size as f32;
            assert!(
                scale <= 1.0,
                "stbir port: only the downsampling path is ported"
            );
            // stbir__get_filter_pixel_width / _margin / _coefficient_width (downsampling)
            let filter_pixel_width = f64::from(SUPPORT * 2.0 / scale).ceil() as i32;
            let margin = filter_pixel_width / 2;
            let coefficient_width = f64::from(SUPPORT * 2.0).ceil() as usize;
            let num_contributors = input_size + margin as usize * 2;
            let mut contributors = vec![(0i32, 0i32); num_contributors];
            let mut coefficients = vec![0f32; num_contributors * coefficient_width];

            let in_pixels_radius = SUPPORT / scale;
            for n in 0..num_contributors {
                let n_adjusted = n as i32 - margin;
                let (out_first, out_last, out_center_of_in) =
                    sample_range_downsample(n_adjusted, in_pixels_radius, scale);
                // stbir__calculate_coefficients_downsample
                let group = &mut coefficients[n * coefficient_width..];
                let c = &mut contributors[n];
                c.0 = out_first;
                c.1 = out_last;
                for i in 0..=(out_last - out_first) {
                    let out_pixel_center = (i + out_first) as f32 + 0.5;
                    let x = out_pixel_center - out_center_of_in;
                    group[i as usize] = filter_mitchell(x) * scale;
                }
                let mut i = out_last - out_first;
                while i >= 0 {
                    if group[i as usize] != 0.0 {
                        break;
                    }
                    // This line has no weight. We can skip it.
                    c.1 = c.0 + i - 1;
                    i -= 1;
                }
            }

            let mut axis = Axis {
                scale,
                margin,
                coefficient_width,
                contributors,
                coefficients,
            };
            axis.normalize(output_size);
            axis
        }

        /// `stbir__normalize_downsample_coefficients`.
        fn normalize(&mut self, output_size: usize) {
            let cw = self.coefficient_width;
            let num_contributors = self.contributors.len();
            for i in 0..output_size as i32 {
                let mut total = 0f32;
                for j in 0..num_contributors {
                    let (n0, n1) = self.contributors[j];
                    if i >= n0 && i <= n1 {
                        total += self.coefficients[cw * j + (i - n0) as usize];
                    } else if i < n0 {
                        break;
                    }
                }
                let scale = 1.0 / total;
                for j in 0..num_contributors {
                    let (n0, n1) = self.contributors[j];
                    if i >= n0 && i <= n1 {
                        self.coefficients[cw * j + (i - n0) as usize] *= scale;
                    } else if i < n0 {
                        break;
                    }
                }
            }
            // Optimize: Skip zero coefficients and contributions outside of image bounds.
            for j in 0..num_contributors {
                let mut skip = 0usize;
                // C reads the flat coefficient array here (past the group when all are zero).
                while self
                    .coefficients
                    .get(cw * j + skip)
                    .is_some_and(|&c| c == 0.0)
                {
                    skip += 1;
                }
                self.contributors[j].0 += skip as i32;
                while self.contributors[j].0 < 0 {
                    self.contributors[j].0 += 1;
                    skip += 1;
                }
                let range = self.contributors[j].1 - self.contributors[j].0 + 1;
                let max = (cw as i32).min(range);
                for i in 0..max.max(0) as usize {
                    if i + skip >= cw {
                        break;
                    }
                    self.coefficients[cw * j + i] = self.coefficients[cw * j + i + skip];
                }
            }
            // Using min to avoid writing into invalid pixels.
            for c in &mut self.contributors {
                c.1 = c.1.min(output_size as i32 - 1);
            }
        }
    }

    /// `stbir__edge_wrap(STBIR_EDGE_CLAMP, n, max)`.
    fn clamp_edge(n: i32, max: usize) -> usize {
        n.clamp(0, max as i32 - 1) as usize
    }

    /// `stbir_resize_uint8_srgb_edgemode(input, in_w, in_h, 0, output, out_w, out_h, 0,
    /// channels, alpha_channel, 0, STBIR_EDGE_CLAMP)` for `out_w <= in_w`, `out_h <= in_h`.
    /// `alpha_channel == None` is `STBIR_ALPHA_CHANNEL_NONE` (every channel sRGB, no
    /// premultiplication).
    pub(crate) fn resize_uint8_srgb_clamp(
        input: &[u8],
        in_w: usize,
        in_h: usize,
        output: &mut [u8],
        out_w: usize,
        out_h: usize,
        channels: usize,
        alpha_channel: Option<usize>,
    ) {
        let hz = Axis::new(in_w, out_w);
        let vt = Axis::new(in_h, out_h);
        let _ = (hz.scale, vt.scale);
        let ch = channels;
        let hm = hz.margin;
        let decode_w = in_w + 2 * hm as usize;
        let mut decode = vec![0f32; decode_w * ch];
        let mut horizontal = vec![0f32; out_w * ch];
        // The ring buffer of stbir__buffer_loop_downsample holds each output scanline from its
        // first contribution until it is encoded; contributions arrive in input-row order, so a
        // full-height accumulator sums in the same order.
        let mut acc = vec![0f32; out_h * out_w * ch];

        let vm = vt.margin;
        let in_pixels_radius = SUPPORT / vt.scale;
        for y in -vm..(in_h as i32 + vm) {
            let (out_first, out_last, _) = sample_range_downsample(y, in_pixels_radius, vt.scale);
            if out_last < 0 || out_first >= out_h as i32 {
                continue;
            }
            // stbir__decode_scanline (UINT8, SRGB, EDGE_CLAMP)
            let row = &input[clamp_edge(y, in_h) * in_w * ch..];
            for x in -hm..(in_w as i32 + hm) {
                let d = (x + hm) as usize * ch;
                let s = clamp_edge(x, in_w) * ch;
                for c in 0..ch {
                    decode[d + c] = SRGB_UCHAR_TO_LINEAR_FLOAT[row[s + c] as usize];
                }
                if let Some(a) = alpha_channel {
                    decode[d + a] = f32::from(row[s + a]) / 255.0;
                }
            }
            if let Some(a) = alpha_channel {
                for x in 0..decode_w {
                    let d = x * ch;
                    // If the alpha value is 0 it will clobber the color values. Make sure it's not.
                    let alpha = decode[d + a] + ALPHA_EPSILON;
                    decode[d + a] = alpha;
                    for c in 0..ch {
                        if c != a {
                            decode[d + c] *= alpha;
                        }
                    }
                }
            }
            // stbir__resample_horizontal_downsample into the zeroed horizontal buffer
            horizontal.fill(0.0);
            for x in 0..decode_w {
                let (n0, n1) = hz.contributors[x];
                let group = hz.coefficient_width * x;
                for k in n0..=n1 {
                    let coefficient = hz.coefficients[group + (k - n0) as usize];
                    let o = k as usize * ch;
                    for c in 0..ch {
                        horizontal[o + c] += decode[x * ch + c] * coefficient;
                    }
                }
            }
            // stbir__resample_vertical_downsample
            let contributor = (y + vm) as usize;
            let (n0, n1) = vt.contributors[contributor];
            for k in n0..=n1 {
                let coefficient =
                    vt.coefficients[vt.coefficient_width * contributor + (k - n0) as usize];
                let line = &mut acc[k as usize * out_w * ch..(k as usize + 1) * out_w * ch];
                for (o, h) in line.iter_mut().zip(&horizontal) {
                    *o += h * coefficient;
                }
            }
        }

        // stbir__encode_scanline (UINT8, SRGB) for every output row
        for (px_out, px) in output
            .chunks_exact_mut(ch)
            .zip(acc.chunks_exact_mut(ch))
            .take(out_w * out_h)
        {
            if let Some(a) = alpha_channel {
                let alpha = px[a];
                let reciprocal_alpha = if alpha != 0.0 { 1.0 / alpha } else { 0.0 };
                for c in 0..ch {
                    if c != a {
                        px[c] *= reciprocal_alpha;
                    }
                }
            }
            for c in 0..ch {
                if Some(c) != alpha_channel {
                    px_out[c] = linear_to_srgb_uchar(px[c]);
                }
            }
            if let Some(a) = alpha_channel {
                // STBIR__ENCODE_LINEAR8: (unsigned char)(int)(saturate(f) * 255.0f + 0.5)
                let f = px[a].clamp(0.0, 1.0);
                px_out[a] = (f64::from(f * 255.0) + 0.5) as i32 as u8;
            }
        }
    }
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
        // RegisterShader: same name and flags => the same shader.
        let a2 = ImageHandle::default();
        images.load(&a2, "x.png", ImageFlags::empty());
        assert_eq!(a.tex(), a2.tex());
        // StringHash is case-sensitive, so a name differing in case is another texture
        // (CCR-0023; the UI-U0 test asserted case-insensitive dedup).
        let b = ImageHandle::default();
        images.load(&b, "X.PNG", ImageFlags::empty());
        assert_ne!(a.tex(), b.tex());
        // generic_u8string: '\\' and '/' name the same shader.
        let s = ImageHandle::default();
        images.load(&s, "dir\\x.png", ImageFlags::empty());
        let s2 = ImageHandle::default();
        images.load(&s2, "dir/x.png", ImageFlags::empty());
        assert_eq!(s.tex(), s2.tex());
        let c = ImageHandle::default();
        images.load(&c, "x.png", ImageFlags::CLAMP);
        assert_ne!(a.tex(), c.tex());
        let ups = images.take_uploads();
        // x.png, X.PNG, dir\x.png (failed: default texture), dir/x.png (same shader, size unknown:
        // ForceLoad loads it again), x.png CLAMP
        assert_eq!(ups.len(), 5);
        assert_eq!(ups[4].1.flags, ImageFlags::CLAMP);
        // Aliases share the slot (Lua userdata): Unload through one is seen by the other.
        let alias = a.clone();
        alias.unload();
        assert!(!a.is_valid());
        // The texture stays registered (textures live until exit): reloading reuses it.
        images.load(&a, "x.png", ImageFlags::empty());
        assert_eq!(a.tex(), a2.tex());
        assert!(images.take_uploads().is_empty());
        assert!(images.take_releases().is_empty());
    }

    #[test]
    fn failed_load_is_zero_size_default_texture() {
        let mut images = ImageRegistry::new(Box::new(MemLoader::default()));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "/definitely/not/here.png", ImageFlags::CLAMP);
        assert!(a.is_valid());
        assert_eq!(images.image_size(a.tex().unwrap()), (0, 0));
        let ups = images.take_uploads();
        assert_eq!(ups.len(), 1);
        assert_eq!((ups[0].1.width, ups[0].1.height), (8, 8));
        // Uploaded with TF_NOMIPMAP only: the requested CLAMP does not reach the sampler.
        assert_eq!(*ups[0].1, default_texture());
        // Re-registering a texture whose size is unknown runs ForceLoad -> LoadFile again.
        let b = ImageHandle::default();
        images.load(&b, "/definitely/not/here.png", ImageFlags::CLAMP);
        assert_eq!(a.tex(), b.tex());
        assert_eq!(images.take_uploads().len(), 1);
    }

    #[test]
    fn force_load_retries_a_file_that_appeared() {
        let files: Rc<std::cell::RefCell<HashMap<String, Vec<u8>>>> = Rc::default();
        struct Shared(Rc<std::cell::RefCell<HashMap<String, Vec<u8>>>>);
        impl ImageLoader for Shared {
            fn request(
                &mut self,
                id: TexId,
                path: &str,
                flags: ImageFlags,
                s: bool,
                p: i32,
            ) -> Option<LoadEvent> {
                MemLoader(self.0.borrow().clone()).request(id, path, flags, s, p)
            }
            fn read_size(&mut self, id: TexId, path: &str) -> Option<(u32, u32)> {
                MemLoader(self.0.borrow().clone()).read_size(id, path)
            }
            fn set_priority(&mut self, _: TexId, _: i32) {}
            fn poll(&mut self) -> Vec<LoadEvent> {
                Vec::new()
            }
        }
        let mut images = ImageRegistry::new(Box::new(Shared(files.clone())));
        let a = ImageHandle::default();
        images.load(&a, "TreeData/new.png", ImageFlags::empty());
        assert_eq!(images.image_size(a.tex().unwrap()), (0, 0));
        files
            .borrow_mut()
            .insert("TreeData/new.png".into(), png_bytes(3, 2));
        // Same shader, size still unknown => loaded again (PassiveTree:LoadImage after a download).
        images.load(&a, "TreeData/new.png", ImageFlags::empty());
        assert_eq!(images.image_size(a.tex().unwrap()), (3, 2));
        // Once the size is known a re-registration does not reload.
        let n = images.take_uploads().len();
        images.load(&a, "TreeData/new.png", ImageFlags::empty());
        assert_eq!(images.take_uploads().len(), 0);
        assert!(n >= 2);
    }

    #[test]
    fn empty_path_is_a_valid_failed_texture() {
        let mut images = ImageRegistry::new(Box::new(MemLoader::default()));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "", ImageFlags::empty());
        assert!(a.is_valid());
        assert_eq!(images.image_size(a.tex().unwrap()), (0, 0));
        assert_eq!(
            images.source(a.tex().unwrap()),
            Some(("", ImageFlags::empty()))
        );
    }

    #[test]
    fn corrupt_file_keeps_the_header_size() {
        // A PNG whose header parses but whose data does not: SG's size callback fired before the
        // decode error, so ImageSize reports the header size while the pixels are the default.
        let mut bytes = png_bytes(6, 4);
        let n = bytes.len();
        bytes.truncate(n - 20);
        bytes.extend_from_slice(&[0u8; 8]);
        let mut files = HashMap::new();
        files.insert("c.png".to_owned(), bytes);
        let mut images = ImageRegistry::new(Box::new(MemLoader(files)));
        images.take_uploads();
        let a = ImageHandle::default();
        images.load(&a, "c.png", ImageFlags::empty());
        let ups = images.take_uploads();
        assert_eq!(*ups[0].1, default_texture());
        assert_eq!(images.image_size(a.tex().unwrap()), (6, 4));
    }

    fn sg() -> Sg {
        let mut files = HashMap::new();
        files.insert("x.png".to_owned(), png_bytes(5, 3));
        Sg::new(
            Rc::new(crate::font::FontSet::empty()),
            ImageRegistry::new(Box::new(MemLoader(files))),
        )
    }

    fn lua_err(f: impl FnOnce()) -> String {
        pob_lua::error::catch(f)
            .expect_err("expected a Lua error")
            .0
    }

    #[test]
    fn unloaded_handle_methods_are_lua_errors() {
        let mut sg = sg();
        let h = ImageHandle::default();
        assert_eq!(
            lua_err(|| {
                h.image_size(&mut sg);
            }),
            "imgHandle:ImageSize(): image handle has no image loaded"
        );
        assert_eq!(
            lua_err(|| {
                h.is_loading(&mut sg);
            }),
            "imgHandle:IsLoading(): image handle has no image loaded"
        );
        assert_eq!(
            lua_err(|| h.set_loading_priority(&mut sg, 3)),
            "imgHandle:SetLoadingPriority(): image handle has no image loaded"
        );
        // Unload / IsValid accept an unloaded handle (GetImgHandle(..., false)).
        h.unload();
        assert!(!h.is_valid());
        h.load(&mut sg, "x.png", ImageFlags::empty());
        assert!(!h.is_loading(&mut sg));
        assert_eq!(h.image_size(&mut sg), (5, 3));
        h.set_loading_priority(&mut sg, 3);
        let m = ImageHandle::default();
        m.load(&mut sg, "missing.png", ImageFlags::ASYNC);
        assert!(m.is_loading(&mut sg));
        assert_eq!(m.image_size(&mut sg), (0, 0));
    }

    #[test]
    fn flags_parse() {
        let f = ImageFlags::from_lua(&["CLAMP", "MIPMAP"]).unwrap();
        assert!(
            f.contains(ImageFlags::CLAMP)
                && f.contains(ImageFlags::MIPMAP)
                && !f.contains(ImageFlags::ASYNC)
        );
        assert_eq!(
            ImageFlags::from_lua(&["NEAREST", "BOGUS"]).unwrap_err(),
            "imgHandle:Load(): unrecognised flag 'BOGUS'"
        );
        // Flag names are case-sensitive (std::string ==).
        assert!(ImageFlags::from_lua(&["clamp"]).is_err());
        assert_eq!(ImageFlags::from_lua(&[]).unwrap(), ImageFlags::empty());
    }

    #[test]
    fn loader_for_file_detection() {
        assert_eq!(extension("a/b.png"), ".png");
        assert_eq!(extension("a.b/c"), "");
        assert_eq!(extension("x.dds.zst"), ".zst");
        assert_eq!(extension(".hidden"), "");
        assert_eq!(loader_for_file(b"\x89PNGxx", "a.bin"), Some(ImgFormat::Png));
        assert_eq!(
            loader_for_file(b"\xFF\xD8\xFF\xE0", "a.png"),
            Some(ImgFormat::Jpeg)
        );
        assert_eq!(loader_for_file(b"GIF89a", "a"), Some(ImgFormat::Gif));
        assert_eq!(loader_for_file(b"DDS |", "a"), Some(ImgFormat::Dds));
        assert_eq!(loader_for_file(b"RIFFxxxx", "a"), Some(ImgFormat::Webp));
        assert_eq!(loader_for_file(b"", "a.webp"), Some(ImgFormat::Webp));
        assert_eq!(loader_for_file(b"", "a.dds.zst"), Some(ImgFormat::Dds));
        assert_eq!(loader_for_file(b"\0\0\x02\0", "a"), Some(ImgFormat::Targa));
        assert_eq!(
            loader_for_file(b"\0\x01\x09\0", "a"),
            Some(ImgFormat::Targa)
        );
        assert_eq!(loader_for_file(b"\0\0\x04\0", "a"), None);
        assert_eq!(loader_for_file(b"abc", "a.png"), None); // fewer than 4 bytes
                                                            // Extension tests are case-sensitive (path::extension() == ".webp").
        assert_eq!(loader_for_file(b"\x89PNG", "a.WEBP"), Some(ImgFormat::Png));
    }

    #[test]
    fn decode_channel_layouts() {
        // Grey PNG -> RGB (comp 3): v,v,v,255; grey+alpha -> RGBA.
        let g = image::GrayImage::from_fn(2, 1, |x, _| image::Luma([10 + x as u8]));
        let mut out = std::io::Cursor::new(Vec::new());
        g.write_to(&mut out, image::ImageFormat::Png).unwrap();
        let d = decode_image(out.get_ref(), "g.png", ImageFlags::empty()).unwrap();
        assert_eq!(d.levels[0], [10, 10, 10, 255, 11, 11, 11, 255]);
        let ga = image::GrayAlphaImage::from_fn(1, 1, |_, _| image::LumaA([9, 100]));
        let mut out = std::io::Cursor::new(Vec::new());
        ga.write_to(&mut out, image::ImageFormat::Png).unwrap();
        let d = decode_image(out.get_ref(), "ga.png", ImageFlags::empty()).unwrap();
        assert_eq!(d.levels[0], [9, 9, 9, 100]);
        // 16-bit -> >> 8 (stbi__convert_16_to_8).
        let w = image::ImageBuffer::<image::Rgba<u16>, Vec<u16>>::from_fn(1, 1, |_, _| {
            image::Rgba([0x12ff, 0x0080, 0xffff, 0x7f80])
        });
        let mut out = std::io::Cursor::new(Vec::new());
        image::DynamicImage::ImageRgba16(w)
            .write_to(&mut out, image::ImageFormat::Png)
            .unwrap();
        let d = decode_image(out.get_ref(), "w.png", ImageFlags::empty()).unwrap();
        assert_eq!(d.levels[0], [0x12, 0x00, 0xff, 0x7f]);
        // TGA 32-bit RLE through the font decoder; size from the header.
        let mut t = vec![0u8; 18];
        t[2] = 10;
        t[12] = 2;
        t[14] = 1;
        t[16] = 32;
        t[17] = 0x20;
        t.extend_from_slice(&[0x81, 1, 2, 3, 4]);
        let d = decode_image(&t, "t.tga", ImageFlags::empty()).unwrap();
        assert_eq!(d.levels[0], [3, 2, 1, 4, 3, 2, 1, 4]);
        assert_eq!(read_image_size(&t, "t.tga"), Some((2, 1)));
        t[1] = 1; // colour-mapped: rejected before the size callback
        assert_eq!(read_image_size(&t, "t.tga"), None);
        // Unknown signature: no loader.
        assert!(decode_image(b"hello world", "x.png", ImageFlags::empty()).is_none());
        assert_eq!(read_image_size(b"hello world", "x.png"), None);
    }

    #[test]
    fn mip_chain_shape() {
        let d = decode_image(&png_bytes(8, 4), "m.png", ImageFlags::MIPMAP).unwrap();
        let sizes: Vec<usize> = d.levels.iter().map(Vec::len).collect();
        assert_eq!(sizes, [8 * 4 * 4, 4 * 2 * 4, 2 * 4, 4]); // 8x4, 4x2, 2x1, 1x1
        let d = decode_image(&png_bytes(5, 1), "m.png", ImageFlags::MIPMAP).unwrap();
        let sizes: Vec<usize> = d.levels.iter().map(Vec::len).collect();
        assert_eq!(sizes, [5 * 4, 2 * 4, 4]); // 5x1, 2x1, 1x1
                                              // A constant opaque image stays constant (normalised coefficients).
        let c = image::RgbaImage::from_pixel(16, 16, image::Rgba([200, 100, 50, 255]));
        let mips = build_mips(16, 16, c.into_raw());
        assert_eq!(mips.len(), 5);
        for l in &mips[1..] {
            for px in l.chunks_exact(4) {
                assert_eq!(px, [200, 100, 50, 255]);
            }
        }
    }

    #[test]
    fn linear_to_srgb_table_round_trips() {
        // The sRGB decode table followed by the encoder is the identity on every byte.
        for v in 0..=255u8 {
            let lin = stbir::linear_to_srgb_uchar(stbir::decode_entry(v));
            assert_eq!(lin, v, "byte {v}");
        }
        assert_eq!(stbir::linear_to_srgb_uchar(f32::NAN), 0);
        assert_eq!(stbir::linear_to_srgb_uchar(2.0), 255);
        assert_eq!(stbir::linear_to_srgb_uchar(-1.0), 0);
    }

    /// FNV-1a 64 (the C golden generator uses the same).
    fn fnv(levels: &[Vec<u8>], comp: usize) -> u64 {
        let mut h: u64 = 0xcbf2_9ce4_8422_2325;
        for l in levels {
            for px in l.chunks_exact(4) {
                for &b in &px[..comp] {
                    h ^= u64::from(b);
                    h = h.wrapping_mul(0x0100_0000_01b3);
                }
            }
        }
        h
    }

    /// The synthetic image of the golden generator: an LCG, with every 7th pixel fully
    /// transparent so that the alpha epsilon matters.
    fn synth(w: u32, h: u32, seed: u64) -> Vec<u8> {
        let mut s = seed;
        let mut out = Vec::with_capacity((w * h * 4) as usize);
        for i in 0..w * h {
            let mut px = [0u8; 4];
            for p in &mut px {
                s = s
                    .wrapping_mul(6_364_136_223_846_793_005)
                    .wrapping_add(1_442_695_040_888_963_407);
                *p = (s >> 33) as u8;
            }
            if i % 7 == 3 {
                px[3] = 0;
            }
            out.extend_from_slice(&px);
        }
        out
    }

    /// Mip chains against goldens produced by compiling SG's `dep/stb/stb_image_resize.h` v0.97
    /// with `-ffp-contract=off` and running `stbir_resize_uint8_srgb_edgemode(..., 4, 3, 0,
    /// STBIR_EDGE_CLAMP)` (RGBA) and `(..., 3, STBIR_ALPHA_CHANNEL_NONE, ...)` (RGB) level by level
    /// like `BuildMipSet` (CCR-0023 lists the generator).
    #[test]
    fn mips_match_stb_image_resize_goldens() {
        for &(w, h, seed, rgba_hash, rgb_hash) in MIP_GOLDENS {
            let base = synth(w, h, seed);
            let with_alpha = build_mip_chain(w, h, base.clone(), true);
            assert_eq!(fnv(&with_alpha, 4), rgba_hash, "RGBA {w}x{h} seed {seed}");
            let no_alpha = build_mip_chain(w, h, base, false);
            assert_eq!(fnv(&no_alpha, 3), rgb_hash, "RGB {w}x{h} seed {seed}");
            assert!(no_alpha[1..]
                .iter()
                .all(|l| l.chunks_exact(4).all(|p| p[3] == 255)));
        }
    }

    /// (w, h, seed, FNV of the RGBA chain, FNV of the RGB chain); level 0 included.
    #[allow(clippy::unreadable_literal)]
    const MIP_GOLDENS: &[(u32, u32, u64, u64, u64)] = &[
        (8, 8, 1, 0x9c3485a7aa740c03, 0xbb3a801f9786445c),
        (16, 4, 2, 0x57be38e470700f86, 0x5ad8f5195eeff465),
        (5, 3, 3, 0xba15848d2db3b78d, 0x847bc7e1c11d5d2b),
        (7, 1, 4, 0x6bf45703bfeb7050, 0x033a56490f5ce6d9),
        (1, 9, 5, 0x194c8d80f92c788b, 0xe56acd8b1fcb5f35),
        (33, 17, 6, 0xcedc28139e37e453, 0x022256ca83658a62),
        (64, 64, 7, 0xe486116a3b35c058, 0x9d0a3013e357d0c3),
        (100, 37, 8, 0x2968350f3c88d0e2, 0xe71e04a836b9b7ad),
        (3, 3, 9, 0x0fbb71b30e54d460, 0x145b69ff036ac19f),
        (2, 2, 10, 0xd9cd71d243ab5375, 0xc4343177655cb227),
        (257, 3, 11, 0x9dd1e7cbb8001630, 0x92db2bd43ece3078),
        (12, 40, 12, 0x0caec96c059da11e, 0x3495cef635955c28),
    ];

    /// Every image PoB ships under `src/Assets` and `src/TreeData/3_27` decodes, and the header
    /// size equals the decoded size.
    #[test]
    fn pob_assets_decode() {
        decode_dirs(&["Assets", "TreeData/3_27"], 130);
    }

    /// Every tree version's images (slow in debug builds: `cargo test -- --ignored`).
    #[test]
    #[ignore = "slow in debug builds: decodes all 769 tree images"]
    fn pob_all_tree_versions_decode() {
        decode_dirs(&["TreeData"], 700);
    }

    fn decode_dirs(dirs: &[&str], min: usize) {
        let Some(src) = crate::font::tests::locate() else {
            eprintln!("SKIP: PoB sources not found (set POB_ROOT)");
            return;
        };
        let mut files = Vec::new();
        for dir in dirs {
            collect(&src.src.join(dir), &mut files);
        }
        let mut n = 0;
        for f in &files {
            let name = f.to_string_lossy();
            let lower = name.to_ascii_lowercase();
            if !(lower.ends_with(".png")
                || lower.ends_with(".jpg")
                || lower.ends_with(".jpeg")
                || lower.ends_with(".webp")
                || lower.ends_with(".tga"))
            {
                continue;
            }
            let bytes = std::fs::read(f).unwrap();
            let size = read_image_size(&bytes, &name);
            let d = decode_image(&bytes, &name, ImageFlags::empty())
                .unwrap_or_else(|| panic!("{name} does not decode"));
            assert_eq!(size, Some((d.width, d.height)), "{name}");
            assert_eq!(d.levels[0].len(), (d.width * d.height * 4) as usize);
            n += 1;
        }
        eprintln!("decoded {n} PoB images under {dirs:?}");
        assert!(n >= min, "only {n} images found");
    }

    fn collect(dir: &std::path::Path, out: &mut Vec<std::path::PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        let mut v: Vec<_> = rd.flatten().map(|e| e.path()).collect();
        v.sort();
        for p in v {
            if p.is_dir() {
                collect(&p, out);
            } else {
                out.push(p);
            }
        }
    }
}
