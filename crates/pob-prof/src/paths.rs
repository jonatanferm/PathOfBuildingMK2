//! Where the suite finds its inputs. Owner: PROF1.
//!
//! The app reads everything from the asset bundle (VENDOR, `assets/`, `pob_host::bundle`), as the
//! desktop app does: the script and SimpleGraphic runtime files through the bundle's
//! `index.json`, the timeless jewel files through `pob_desktop::BundleJewelFs`, and the compiled
//! game data (`data.bin`, `trees/`) from `<bundle>/gamedata`. The user directory is the suite's
//! own fixture (`crates/pob-prof/fixtures/user`). Every location is resolved here, in [`locate`]:
//!
//! **Asset bundle** (first match wins; a directory holding `index.json`):
//! 1. `--assets DIR`, else `POB_PROF_ASSETS`;
//! 2. `pob_desktop::locate_assets()`, the desktop app's own search: `POB_ASSETS` (the same
//!    meaning as for the desktop: a bundle directory), `assets/` next to the executable, then this
//!    repository's `assets/` (compile-time).
//!
//! **Game data**: `--data DIR` (a directory with `data.bin`), else `<bundle>/gamedata`.
//!
//! Nothing reads a PoB checkout or the repository's `data/` directory, and no Lua is read but
//! the files the product itself reads from the bundle.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::{anyhow, Result};
use pob_host::bundle::{Bundle, INDEX_FILE};

/// The resolved inputs.
#[derive(Clone, Debug)]
pub struct Inputs {
    /// The asset bundle directory: `GetScriptPath()`, and SimpleGraphic's runtime files (fonts)
    /// through its index, as `pob_desktop::host_config` sets them.
    pub assets: PathBuf,
    /// The bundle's index.
    pub bundle: Arc<Bundle>,
    /// The compiled game data (`data.bin`, `trees/`).
    pub dump: PathBuf,
    /// How the bundle was found (reported in the results).
    pub how: String,
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn canon(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// Resolve the inputs (module docs).
pub fn locate(assets: Option<&Path>, data: Option<&Path>) -> Result<Inputs> {
    let (dir, how) = if let Some(d) = assets.map(Path::to_path_buf) {
        let how = format!("--assets {}", d.display());
        (d, how)
    } else if let Some(d) = env_path("POB_PROF_ASSETS") {
        let how = format!("POB_PROF_ASSETS {}", d.display());
        (d, how)
    } else {
        let d = pob_desktop::locate_assets()?;
        let how = format!("pob_desktop::locate_assets {}", d.display());
        (d, how)
    };
    if !dir.join(INDEX_FILE).is_file() {
        return Err(anyhow!(
            "{}: no asset bundle there (no {INDEX_FILE}; docs/perf/PROFILING.md \"Inputs\")",
            dir.display()
        ));
    }
    let dir = canon(dir);
    let bundle = Bundle::detect(&dir)
        .ok_or_else(|| anyhow!("{}: unreadable asset bundle index", dir.display()))?;
    let dump = data.map_or_else(|| pob_desktop::gamedata_dir(&dir), Path::to_path_buf);
    if !dump.join("data.bin").is_file() {
        return Err(anyhow!("{}: no data.bin", dump.display()));
    }
    Ok(Inputs {
        assets: dir,
        bundle,
        dump: canon(dump),
        how,
    })
}

/// This crate's directory (scenarios and fixtures): `POB_PROF_DIR`, else the build's
/// (`CARGO_MANIFEST_DIR`, when it still exists), else the first `crates/pob-prof` above the
/// working directory or the executable.
pub fn crate_dir() -> PathBuf {
    if let Some(d) = env_path("POB_PROF_DIR") {
        return d;
    }
    let built = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if built.join("scenarios").is_dir() {
        return built;
    }
    let starts = [
        std::env::current_dir().ok(),
        std::env::current_exe()
            .ok()
            .and_then(|p| p.parent().map(Path::to_path_buf)),
    ];
    starts
        .iter()
        .flatten()
        .flat_map(|s| s.ancestors())
        .map(|a| a.join("crates/pob-prof"))
        .find(|p| p.join("scenarios").is_dir())
        .unwrap_or(built)
}

/// `crates/pob-prof/scenarios`.
pub fn scenario_dir() -> PathBuf {
    crate_dir().join("scenarios")
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The default inputs are the repository's bundle, and a directory without an index is
    /// refused.
    #[test]
    fn locates_the_repository_bundle() {
        let i = locate(None, None).expect("the repository's asset bundle");
        assert!(i.assets.join(INDEX_FILE).is_file(), "{i:?}");
        assert!(i.dump.ends_with("gamedata") && i.dump.join("data.bin").is_file());
        assert!(!i.bundle.root.is_empty());
        let e = locate(Some(Path::new(env!("CARGO_MANIFEST_DIR"))), None).unwrap_err();
        assert!(e.to_string().contains("no asset bundle there"), "{e}");
        assert!(scenario_dir().is_dir());
    }
}
