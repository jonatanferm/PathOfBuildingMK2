//! Boot real PoB under LuaJIT on our pob-sg (ui doc §11.2 "Boot sequence"). Owner: U8.
//!
//! 1. Create the mlua LuaJIT VM (the interpreter: `jit.off()` first, AMENDMENTS A6).
//! 2. Install pob-oracle's natives (`pob_oracle::jit::{PRELUDE, install_natives}`, made `pub` by
//!    CCR-0028): file access rooted at PoB's `src/`, `Inflate`/`Deflate`/`NewFileSearch`.
//! 3. Load the HeadlessWrapper prefix (everything before `dofile("Launch.lua")`).
//! 4. Override every SG global with [`crate::sg_bridge`] (tracing on; this also replaces the
//!    natives' `OpenURL`, `LaunchSubScript` and `NewFileSearch`), install the markers and
//!    the determinism patches (`lua/markers.lua`, `lua/determinism.lua`).
//! 5. `debug.getinfo(2, "Sl")` sets the trace `src` before each draw call (sg_bridge).
//! 6. `dofile("Launch.lua")` (or the gallery mini-launch `lua/gallery_launch.lua`); the runner then
//!    drives `launch:OnInit`, the key callbacks and `launch:OnFrame` from a `.pobscript`.
//!
//! Every PoB file loads through the prelude's `loadfile`, which calls the source hook installed
//! here: [`crate::patches::apply`] runs on the files that have patches, so a missing anchor fails
//! the load (upstream drift fails loudly).
//!
//! This module also builds the oracle's `Sg`/`Sys` ([`oracle_parts`]): PoB's fonts, the
//! synchronous headless image loader (patch 5), the virtual clock, a virtual clipboard and the
//! fixture user dir (patch 4).

use std::cell::{Cell, RefCell};
use std::path::{Path, PathBuf};
use std::rc::Rc;

use mlua::{IntoLuaMulti, Lua};
use pob_sg::{AssetSource, FileEntry, FontSet, ImageRegistry, Sg, Sys};

/// A booted PoB: the VM plus the Sg/Sys it draws through (tracing on).
pub struct LuaPob {
    pub lua: mlua::Lua,
    pub sg: Rc<RefCell<pob_sg::Sg>>,
    pub sys: Rc<RefCell<pob_sg::Sys>>,
}

impl Drop for LuaPob {
    fn drop(&mut self) {
        crate::sg_bridge::uninstall(&self.lua);
    }
}

/// What the VM runs after the SG globals are bound.
#[derive(Clone, Debug, Default)]
pub enum Entry {
    /// `dofile("Launch.lua")`: PoB itself.
    #[default]
    Launch,
    /// The gallery mini-launch running `lua/gallery/<name>.lua` (this path).
    Gallery(PathBuf),
}

/// Boot options beyond [`boot`]'s defaults.
#[derive(Clone, Debug, Default)]
pub struct BootOptions {
    pub entry: Entry,
    /// Audit mode (§11.2): patch 6's canonical order is disabled and draw order is recorded.
    pub audit: bool,
    /// A live session (`pob-lua-gui`): `lua/determinism.lua` (patches 1 and 2: virtual `os`
    /// clock, fixed random seed) is not loaded. The text patches still apply.
    pub live: bool,
}

/// The crate's `lua/` directory.
pub fn lua_dir() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("lua")
}

/// `.PathOfBuilding` of the main checkout (or `$POB_ROOT`), found from this crate's directory or the
/// working directory (a worktree's ancestors include the main checkout).
pub fn locate_pob_root() -> Option<PathBuf> {
    let mut starts = Vec::new();
    if let Some(p) = std::env::var_os("POB_ROOT") {
        let p = PathBuf::from(p);
        if p.join("src/Launch.lua").is_file() {
            return Some(p);
        }
        starts.push(p);
    }
    starts.push(PathBuf::from(env!("CARGO_MANIFEST_DIR")));
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    for s in starts {
        for d in s.ancestors() {
            let root = d.join(".PathOfBuilding");
            if root.join("src/Launch.lua").is_file()
                && root
                    .join("runtime/SimpleGraphic/Fonts/Liberation Sans.tgf")
                    .is_file()
            {
                return Some(root);
            }
        }
    }
    None
}

/// Reads PoB's `src/` and `runtime/SimpleGraphic/` from disk.
pub struct DirAssets {
    pub src: PathBuf,
    pub runtime: PathBuf,
}

impl AssetSource for DirAssets {
    fn read_src(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.src.join(path))
    }
    fn read_runtime(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.runtime.join(path))
    }
}

/// The oracle's process services: a virtual clock (patch 1), a virtual clipboard, the fixture
/// user dir (patch 4); window requests are recorded as trace side effects by sg_bridge.
pub struct OraclePlatform {
    pub clock: Rc<Cell<i64>>,
    pub clipboard: Rc<RefCell<Option<String>>>,
    pub script_path: String,
    pub runtime_path: String,
    pub user_path: String,
    /// Echo ConPrintf to stderr (`POB_UI_ORACLE_CONSOLE=1`).
    pub echo_console: bool,
}

impl pob_sg::Platform for OraclePlatform {
    fn time_ms(&self) -> i64 {
        self.clock.get()
    }
    fn copy(&mut self, text: &str) {
        *self.clipboard.borrow_mut() = Some(text.to_owned());
    }
    fn paste(&mut self) -> Option<String> {
        self.clipboard.borrow().clone()
    }
    fn open_url(&mut self, _url: &str) {}
    fn set_window_title(&mut self, _title: &str) {}
    fn take_screenshot(&mut self) {}
    fn restart(&mut self) {}
    fn exit(&mut self, _msg: Option<&str>) {}
    fn script_path(&self) -> String {
        self.script_path.clone()
    }
    fn runtime_path(&self) -> String {
        self.runtime_path.clone()
    }
    fn user_path(&self) -> Result<String, (String, String)> {
        Ok(self.user_path.clone())
    }
    fn con_print(&mut self, s: &str) {
        if self.echo_console {
            eprintln!("[pob] {s}");
        }
    }
}

/// The Rust side's file access (and `MakeDir`/`RemoveDir` of the Lua side; Lua's `io` is the
/// prelude's, rooted at `src/`). Searches follow the bridge's `NewFileSearch` (SG's glob, NTFS
/// name order) and U7's headless modification times ([`crate::sg_bridge::Mtimes::Virtual`]:
/// the virtual write time for files written through [`pob_sg::Fs::write`], 0 otherwise).
pub struct OracleFs {
    /// PoB's `src/` (relative paths resolve here, like the prelude's `io.open`).
    pub root: PathBuf,
    /// The virtual clock (write times).
    pub clock: Rc<Cell<i64>>,
    /// Files written during the run (normalised absolute path -> virtual clock).
    pub writes: std::collections::HashMap<PathBuf, i64>,
}

impl OracleFs {
    /// File access rooted at `root` on `clock`.
    pub fn new(root: PathBuf, clock: Rc<Cell<i64>>) -> OracleFs {
        OracleFs {
            root,
            clock,
            writes: std::collections::HashMap::new(),
        }
    }

    fn abs(&self, path: &str) -> PathBuf {
        let p = Path::new(path);
        if p.is_absolute() {
            p.to_path_buf()
        } else {
            self.root.join(p)
        }
    }
}

impl pob_sg::Fs for OracleFs {
    fn read(&self, path: &str) -> std::io::Result<Vec<u8>> {
        std::fs::read(self.abs(path))
    }
    fn write(&mut self, path: &str, data: &[u8]) -> std::io::Result<()> {
        let p = self.abs(path);
        std::fs::write(&p, data)?;
        self.writes
            .insert(crate::sg_bridge::normalize_path(&p), self.clock.get());
        Ok(())
    }
    fn search(&self, spec: &str, find_dirs: bool) -> Vec<FileEntry> {
        crate::sg_bridge::find_files(
            &self.abs(spec),
            find_dirs,
            crate::sg_bridge::Mtimes::Virtual(&self.writes),
        )
        .into_iter()
        .map(|f| FileEntry {
            name: f.name,
            is_dir: find_dirs,
            size: f.size,
            modified: f.modified as i64,
        })
        .collect()
    }
    fn make_dir(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::create_dir_all(self.abs(path))
    }
    fn remove_dir(&mut self, path: &str, recursive: bool) -> std::io::Result<()> {
        if recursive {
            std::fs::remove_dir_all(self.abs(path))
        } else {
            std::fs::remove_dir(self.abs(path))
        }
    }
    fn rename(&mut self, from: &str, to: &str) -> std::io::Result<()> {
        std::fs::rename(self.abs(from), self.abs(to))
    }
    fn remove(&mut self, path: &str) -> std::io::Result<()> {
        std::fs::remove_file(self.abs(path))
    }
}

/// No network: `launch:DownloadPage` goes through the scripted sub-script fixtures (patch 8).
pub struct NoNet;

impl pob_sg::Net for NoNet {
    fn download_page(&mut self, _req: pob_sg::HttpRequest) -> u64 {
        0
    }
    fn poll(&mut self) -> Vec<(u64, pob_sg::HttpResponse)> {
        Vec::new()
    }
    fn busy(&self) -> bool {
        false
    }
}

/// The Rust side's `Net` over the same fixtures as the Lua side's `LaunchSubScript` (patch 8),
/// with U7's headless rule (`pob_host::headless::ScriptedNet` on `port/U7`, CCR-0028 item 10):
/// every request made since the last `poll` completes at the next one, which the driver runs at
/// the start of the next frame, before `OnFrame`. TRANSITIONAL: replaced by U7's `ScriptedNet`
/// when the oracle switches to `pob_host::run_headless`.
pub struct ScriptedNet {
    fixtures: crate::sg_bridge::NetFixtures,
    pending: Vec<(u64, pob_sg::HttpResponse)>,
    next_id: u64,
}

impl ScriptedNet {
    pub fn new(fixtures: crate::sg_bridge::NetFixtures) -> ScriptedNet {
        ScriptedNet {
            fixtures,
            pending: Vec::new(),
            next_id: 0,
        }
    }
}

impl pob_sg::Net for ScriptedNet {
    fn download_page(&mut self, req: pob_sg::HttpRequest) -> u64 {
        let r = self.fixtures.take(&req.url);
        self.next_id += 1;
        self.pending.push((
            self.next_id,
            pob_sg::HttpResponse {
                body: r.body,
                error: r.error,
                headers: r.headers,
            },
        ));
        self.next_id
    }
    fn poll(&mut self) -> Vec<(u64, pob_sg::HttpResponse)> {
        std::mem::take(&mut self.pending)
    }
    fn busy(&self) -> bool {
        !self.pending.is_empty()
    }
}

/// Shared handles to the oracle platform's state.
#[derive(Clone, Default)]
pub struct OracleHandles {
    pub clock: Rc<Cell<i64>>,
    pub clipboard: Rc<RefCell<Option<String>>>,
}

/// `Sg` (PoB's fonts, the synchronous headless loader) and `Sys` (virtual clock, clipboard,
/// `GetUserPath() == user_dir`) for one side of a run.
pub fn oracle_parts(
    pob_root: &Path,
    user_dir: &Path,
    handles: &OracleHandles,
) -> anyhow::Result<(Sg, Sys)> {
    let src = pob_root.join("src");
    let runtime = pob_root.join("runtime/SimpleGraphic");
    let assets = DirAssets {
        src: src.clone(),
        runtime: runtime.clone(),
    };
    let mut images = ImageRegistry::new(Box::new(pob_host::loader_headless::HeadlessLoader::new(
        Box::new(DirAssets {
            src: src.clone(),
            runtime,
        }),
    )));
    let fonts = FontSet::load(&assets, &mut images)
        .map_err(|e| anyhow::anyhow!("loading fonts under {}: {e}", pob_root.display()))?;
    let sg = Sg::new(Rc::new(fonts), images);
    let canon = std::fs::canonicalize(&src).unwrap_or(src);
    let platform = OraclePlatform {
        clock: handles.clock.clone(),
        clipboard: handles.clipboard.clone(),
        script_path: canon.to_string_lossy().into_owned(),
        runtime_path: std::fs::canonicalize(pob_root.join("runtime"))
            .unwrap_or_else(|_| pob_root.join("runtime"))
            .to_string_lossy()
            .into_owned(),
        user_path: user_dir.to_string_lossy().into_owned(),
        echo_console: std::env::var_os("POB_UI_ORACLE_CONSOLE").is_some(),
    };
    let sys = Sys::new(
        Box::new(platform),
        Box::new(OracleFs::new(canon, handles.clock.clone())),
        Box::new(NoNet),
    );
    Ok((sg, sys))
}

/// The prelude's `loadfile` with the patch hook: the hook sees each file's raw text first.
fn hooked_prelude() -> anyhow::Result<String> {
    const ANCHOR: &str = "    local src = f:read(\"*a\")\n    f:close()\n";
    const HOOK: &str = "    if __mk2_source_hook then src = __mk2_source_hook(full, src) end\n";
    let p = pob_oracle::jit::PRELUDE;
    anyhow::ensure!(
        p.matches(ANCHOR).count() == 1,
        "pob-oracle PRELUDE changed: the loadfile anchor for the patch hook is missing"
    );
    Ok(p.replacen(ANCHOR, &format!("{ANCHOR}{HOOK}"), 1))
}

fn lerr(e: mlua::Error) -> anyhow::Error {
    anyhow::anyhow!("{e}")
}

/// Boot PoB from `pob_src` (PoB's `src/`) with the patches applied.
pub fn boot(pob_src: &str, sg: pob_sg::Sg, sys: pob_sg::Sys) -> anyhow::Result<LuaPob> {
    boot_with(pob_src, sg, sys, &BootOptions::default())
}

/// [`boot`] with an entry point (PoB or a gallery) and the audit switch.
pub fn boot_with(
    pob_src: &str,
    sg: pob_sg::Sg,
    sys: pob_sg::Sys,
    opts: &BootOptions,
) -> anyhow::Result<LuaPob> {
    let root = std::fs::canonicalize(pob_src).map_err(|e| anyhow::anyhow!("{pob_src}: {e}"))?;
    let mut root_s = root.to_string_lossy().replace('\\', "/");
    if !root_s.ends_with('/') {
        root_s.push('/');
    }
    let pob = LuaPob {
        lua: Lua::new(),
        sg: Rc::new(RefCell::new(sg)),
        sys: Rc::new(RefCell::new(sys)),
    };
    let lua = &pob.lua;
    // 1. The interpreter (A6), before anything else runs.
    if pob_oracle::jit::oracle_jit_off() {
        lua.load(pob_oracle::jit::ORACLE_JIT_OFF)
            .set_name("jit_off")
            .exec()
            .map_err(lerr)?;
    }
    // 2. File access rooted at src/, plus the patch hook.
    lua.globals()
        .set("__mk2_root", root_s.clone())
        .map_err(lerr)?;
    lua.load(hooked_prelude()?)
        .set_name("prelude")
        .exec()
        .map_err(lerr)?;
    let patched = crate::patches::patched_files();
    let hook_root = root_s.clone();
    lua.globals()
        .set(
            "__mk2_source_hook",
            lua.create_function(move |lua, (full, src): (String, mlua::String)| {
                let rel = full.strip_prefix(&hook_root).unwrap_or(&full).to_owned();
                if !patched.contains(&rel.as_str()) {
                    return Ok(src);
                }
                let text: String = (*src.to_str()?).to_owned();
                let out = crate::patches::apply(&rel, &text).map_err(mlua::Error::RuntimeError)?;
                lua.create_string(out)
            })
            .map_err(lerr)?,
        )
        .map_err(lerr)?;
    // 3. HeadlessWrapper.lua up to `dofile("Launch.lua")`: SG stubs, callbacks, the require hack.
    let wrapper = std::fs::read_to_string(root.join("HeadlessWrapper.lua"))
        .map_err(|e| anyhow::anyhow!("HeadlessWrapper.lua: {e}"))?;
    let prefix = wrapper
        .find("dofile(\"Launch.lua\")")
        .map(|i| &wrapper[..i])
        .ok_or_else(|| anyhow::anyhow!("HeadlessWrapper.lua: dofile(\"Launch.lua\") not found"))?;
    lua.load(prefix)
        .set_name(format!("@{root_s}HeadlessWrapper.lua"))
        .exec()
        .map_err(lerr)?;
    pob_oracle::jit::install_natives(lua).map_err(lerr)?;
    // 4. Our SG globals (they replace the def.lua dummies and the natives' OpenURL /
    //    LaunchSubScript), the markers and the determinism patches.
    crate::sg_bridge::install(lua, pob.sg.clone(), pob.sys.clone()).map_err(lerr)?;
    *crate::sg_bridge::bridge(lua)
        .map_err(lerr)?
        .chunk_root
        .borrow_mut() = format!("@{root_s}");
    crate::markers::install(lua, pob.sg.clone()).map_err(lerr)?;
    if opts.audit {
        crate::markers::enable_audit(lua).map_err(lerr)?;
    }
    crate::sg_bridge::bridge(lua)
        .map_err(lerr)?
        .virtual_mtimes
        .set(!opts.live);
    if !opts.live {
        lua.load(include_str!("../lua/determinism.lua"))
            .set_name("@pob-ui-oracle/lua/determinism.lua")
            .exec()
            .map_err(lerr)?;
    }
    // 6. The entry point.
    match &opts.entry {
        Entry::Launch => lua
            .load("dofile('Launch.lua')")
            .set_name("boot")
            .exec()
            .map_err(lerr)?,
        Entry::Gallery(path) => {
            lua.globals()
                .set("__galleryPath", path.to_string_lossy().into_owned())
                .map_err(lerr)?;
            let launch = lua_dir().join("gallery_launch.lua");
            lua.load(
                std::fs::read_to_string(&launch)
                    .map_err(|e| anyhow::anyhow!("{}: {e}", launch.display()))?,
            )
            .set_name("@pob-ui-oracle/lua/gallery_launch.lua")
            .exec()
            .map_err(lerr)?;
        }
    }
    Ok(pob)
}

impl LuaPob {
    /// `runCallback(name, ...)` (HeadlessWrapper.lua: `__mainObject__[name](__mainObject__, ...)`).
    pub fn run_callback(&self, name: &str, args: impl IntoLuaMulti) -> mlua::Result<()> {
        let f: mlua::Function = self.lua.globals().get("runCallback")?;
        let mut a = args.into_lua_multi(&self.lua)?;
        a.push_front(mlua::Value::String(self.lua.create_string(name)?));
        f.call::<()>(a)
    }

    /// Evaluate a Lua chunk (tests and the runner's `load-build` / `snapshot`).
    pub fn eval<R: mlua::FromLuaMulti>(&self, code: &str) -> mlua::Result<R> {
        self.lua.load(code).set_name("=oracle").eval()
    }
}
