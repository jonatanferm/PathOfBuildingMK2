//! The desktop app's start-up (VENDOR): find the asset bundle (`assets/`, `pob_host::bundle`),
//! load the game data from it and register its timeless jewel files. `main.rs` and `sg-demo` use
//! it, and so do the standalone tests (`tests/standalone.rs`).
//!
//! Where the bundle is found, first hit wins:
//! 1. `$POB_ASSETS` (a directory holding the bundle's `index.json`);
//! 2. `assets/` next to the executable (an installed copy);
//! 3. the compile-time default, this repository's `assets/` ([`COMPILED_ASSETS_DIR`]).
//!
//! Nothing searches the working directory or its parents, and no PoB checkout is read.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use pob_gamedata::timeless_lut::{JewelFileAttr, JewelFs};
use pob_host::bundle::{Bundle, INDEX_FILE};

/// The repository's asset bundle, the compile-time default.
pub const COMPILED_ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/../../assets");

/// The environment variable that overrides where the bundle is.
pub const ASSETS_ENV: &str = "POB_ASSETS";

fn is_bundle(dir: &Path) -> bool {
    dir.join(INDEX_FILE).is_file()
}

/// The asset bundle directory (module docs), canonicalised.
pub fn locate_assets() -> anyhow::Result<PathBuf> {
    let found = if let Some(p) = std::env::var_os(ASSETS_ENV) {
        let p = PathBuf::from(p);
        anyhow::ensure!(
            is_bundle(&p),
            "{ASSETS_ENV}={}: no asset bundle there (no {INDEX_FILE})",
            p.display()
        );
        p
    } else {
        let next_to_exe = std::env::current_exe()
            .ok()
            .and_then(|e| e.parent().map(|d| d.join("assets")))
            .filter(|d| is_bundle(d));
        if let Some(d) = next_to_exe {
            d
        } else {
            let d = PathBuf::from(COMPILED_ASSETS_DIR);
            anyhow::ensure!(
                is_bundle(&d),
                "asset bundle not found: no assets/ next to the executable and none at {} \
                 (set {ASSETS_ENV})",
                d.display()
            );
            d
        }
    };
    Ok(std::fs::canonicalize(&found).unwrap_or(found))
}

/// The bundle's game data directory (`data.bin`, `trees/`).
pub fn gamedata_dir(assets: &Path) -> PathBuf {
    assets.join("gamedata")
}

/// Load the game data and register the timeless jewel files of the bundle at `assets`, with
/// the decompressed-LUT cache in [`lut_cache_dir`].
pub fn init_bundle(assets: &Path) -> anyhow::Result<Arc<Bundle>> {
    let bundle = Bundle::detect(assets)
        .ok_or_else(|| anyhow::anyhow!("{}: unreadable asset bundle index", assets.display()))?;
    let data = gamedata_dir(assets);
    pob_gamedata::init_from_dir(&data)
        .map_err(|e| anyhow::anyhow!("{}: {e}", data.join("data.bin").display()))?;
    pob_gamedata::timeless_lut::set_jewel_fs(Arc::new(BundleJewelFs::new(
        bundle.clone(),
        lut_cache_dir(&bundle),
    )));
    Ok(bundle)
}

/// Where decompressed timeless jewel tables are cached (PoB writes its `.bin` next to the
/// `.zip`, which the read-only bundle cannot hold): `$XDG_CACHE_HOME` (else `~/.cache`; Windows
/// `%LOCALAPPDATA%`, macOS `~/Library/Caches`) `/pob-rust-port/timeless/<snapshot>`. `None`:
/// no cache, the tables are inflated on every load.
pub fn lut_cache_dir(bundle: &Bundle) -> Option<PathBuf> {
    let base = if cfg!(windows) {
        std::env::var_os("LOCALAPPDATA").map(PathBuf::from)
    } else if cfg!(target_os = "macos") {
        std::env::var_os("HOME").map(|h| PathBuf::from(h).join("Library/Caches"))
    } else {
        std::env::var_os("XDG_CACHE_HOME")
            .map(PathBuf::from)
            .or_else(|| std::env::var_os("HOME").map(|h| PathBuf::from(h).join(".cache")))
    }?;
    let snapshot = if bundle.source_id.is_empty() {
        "unknown"
    } else {
        &bundle.source_id
    };
    Some(base.join("pob-rust-port/timeless").join(snapshot))
}

/// The timeless jewel files (`DataJewelFileLoader.lua`'s `scriptPath.."/Data/TimelessJewelData/…"`)
/// of the asset bundle: `GetScriptPath()` is the bundle root, the `.zip`/`.zip.partN` files
/// resolve through the bundle's index, and the uncompressed `.bin` cache lives in a cache
/// directory of its own ([`lut_cache_dir`]).
pub struct BundleJewelFs {
    bundle: Arc<Bundle>,
    cache: Option<PathBuf>,
}

impl BundleJewelFs {
    pub fn new(bundle: Arc<Bundle>, cache: Option<PathBuf>) -> BundleJewelFs {
        BundleJewelFs { bundle, cache }
    }

    /// The logical script path of `path` (`<root>/Data/…` → `Data/…`).
    fn logical(&self, path: &str) -> Option<String> {
        self.bundle
            .relative(path)
            .map(|r| pob_host::bundle::logical(&r))
    }

    /// The cache file of a `.bin` path.
    fn cache_file(&self, logical: &str) -> Option<PathBuf> {
        let name = logical.rsplit('/').next()?;
        let dir = self.cache.as_ref()?;
        Path::new(name)
            .extension()
            .is_some_and(|e| e == "bin")
            .then(|| dir.join(name))
    }

    /// Where `path` is on disk.
    fn file(&self, path: &str) -> Option<PathBuf> {
        let l = self.logical(path)?;
        if let Some(c) = self.cache_file(&l) {
            return Some(c);
        }
        self.bundle
            .script_file(&l)
            .map(|f| Path::new(&self.bundle.root).join(f))
    }
}

fn modified(p: &Path) -> f64 {
    std::fs::metadata(p)
        .and_then(|m| m.modified())
        .ok()
        .and_then(|t| t.duration_since(std::time::UNIX_EPOCH).ok())
        .map_or(0.0, |d| d.as_secs_f64())
}

impl JewelFs for BundleJewelFs {
    fn script_path(&self) -> String {
        self.bundle.root.clone()
    }
    fn exists(&self, path: &str) -> bool {
        self.file(path).is_some_and(|p| p.is_file())
    }
    fn read(&self, path: &str) -> Option<Vec<u8>> {
        std::fs::read(self.file(path)?).ok()
    }
    fn write(&self, path: &str, data: &[u8]) -> bool {
        let Some(l) = self.logical(path) else {
            return false;
        };
        // only the `.bin` cache is writable
        let Some(c) = self.cache_file(&l) else {
            return false;
        };
        c.parent()
            .is_some_and(|d| std::fs::create_dir_all(d).is_ok())
            && std::fs::write(&c, data).is_ok()
    }
    /// `NewFileSearch(spec)`: the first match by name (a trailing `*` matches any suffix), in
    /// the cache for `.bin` specs, else in the bundle's logical directory.
    fn search(&self, spec: &str) -> Option<JewelFileAttr> {
        let (pattern, exact) = match spec.strip_suffix('*') {
            Some(p) => (p, false),
            None => (spec, true),
        };
        let l = self.logical(pattern)?;
        let (dir, prefix) = l.rsplit_once('/').unwrap_or(("", l.as_str()));
        let hit = |name: &str| {
            if exact {
                name == prefix
            } else {
                name.starts_with(prefix)
            }
        };
        if let Some(c) = self.cache_file(&l) {
            let p = c.with_file_name(prefix);
            return (exact && p.is_file()).then(|| JewelFileAttr {
                fileName: prefix.to_owned(),
                modified: modified(&p),
            });
        }
        let entries = self.bundle.script_dir(dir)?;
        // `script_dir` lists by name, as PoB's sorted search does
        let e = entries.into_iter().find(|e| !e.is_dir && hit(&e.name))?;
        let file = Path::new(&self.bundle.root).join(e.file.as_deref()?);
        Some(JewelFileAttr {
            fileName: e.name,
            modified: modified(&file),
        })
    }
}
