//! Where the suite finds its inputs. Owner: PROF1.
//!
//! The app needs three things at run time: the compiled game data (`data.bin` and `trees/`), the
//! image and font assets (today PoB's `src/` and `runtime/SimpleGraphic/`), and a user directory
//! (the suite's own fixture, `crates/pob-prof/fixtures/user`). The asset location is moving (the
//! VENDOR unit bundles the assets in the repository and adds a `POB_ASSETS` override), so every
//! location is configurable and resolved in one place, [`locate`]:
//!
//! **Assets** (first match wins):
//! 1. `--assets DIR` (or `POB_PROF_ASSETS`, then `POB_ASSETS`): `DIR/src` + `DIR/runtime/SimpleGraphic`
//!    when `DIR/src` exists (a PoB-shaped tree), otherwise `DIR` itself holds the image paths the
//!    app asks for and the fonts are in the first of `DIR/runtime/SimpleGraphic`, `DIR/runtime`,
//!    `DIR/fonts/..`, `DIR`;
//! 2. `POB_ROOT`: `$POB_ROOT/src` + `$POB_ROOT/runtime/SimpleGraphic`;
//! 3. the first `.PathOfBuilding/src` in an ancestor of the working directory or the executable.
//!
//! **Game data**: `--data DIR`, else `POB_DUMP_DIR`, else `<assets>/data.bin`'s directory, else the
//! first `data/dump/data.bin` in an ancestor of the working directory or the executable.
//!
//! The suite never reads PoB's Lua: only the files the product reads.

use std::path::{Path, PathBuf};

use anyhow::{anyhow, Result};

/// The resolved inputs.
#[derive(Clone, Debug)]
pub struct Inputs {
    /// `GetScriptPath()`: relative image paths (`Assets/…`, `TreeData/…`) resolve here; the
    /// timeless jewel data is read from `Data/TimelessJewelData` below it.
    pub src: PathBuf,
    /// SimpleGraphic's runtime directory (`Fonts/` below it).
    pub runtime: PathBuf,
    /// The compiled game data (`data.bin`).
    pub dump: PathBuf,
    /// How the assets were found (reported in the results).
    pub how: String,
}

fn ancestors_of_cwd_and_exe() -> Vec<PathBuf> {
    let mut starts = Vec::new();
    if let Ok(d) = std::env::current_dir() {
        starts.push(d);
    }
    if let Some(d) = std::env::current_exe()
        .ok()
        .and_then(|p| p.parent().map(Path::to_path_buf))
    {
        starts.push(d);
    }
    starts
}

/// The first `rel` that exists in an ancestor of the working directory or of the executable.
pub fn find_up(rel: &str) -> Option<PathBuf> {
    for s in ancestors_of_cwd_and_exe() {
        for d in s.ancestors() {
            let p = d.join(rel);
            if p.exists() {
                return Some(p);
            }
        }
    }
    None
}

fn env_path(k: &str) -> Option<PathBuf> {
    std::env::var_os(k)
        .filter(|v| !v.is_empty())
        .map(PathBuf::from)
}

fn canon(p: PathBuf) -> PathBuf {
    std::fs::canonicalize(&p).unwrap_or(p)
}

fn from_asset_dir(dir: &Path) -> (PathBuf, PathBuf) {
    if dir.join("src").is_dir() {
        return (dir.join("src"), dir.join("runtime/SimpleGraphic"));
    }
    let runtime = [
        dir.join("runtime/SimpleGraphic"),
        dir.join("runtime"),
        dir.to_path_buf(),
    ]
    .into_iter()
    .find(|d| d.join("Fonts").is_dir())
    .unwrap_or_else(|| dir.to_path_buf());
    (dir.to_path_buf(), runtime)
}

/// Resolve the inputs (module docs).
pub fn locate(assets: Option<&Path>, data: Option<&Path>) -> Result<Inputs> {
    let (src, runtime, how) = if let Some(d) = assets
        .map(Path::to_path_buf)
        .or_else(|| env_path("POB_PROF_ASSETS"))
        .or_else(|| env_path("POB_ASSETS"))
    {
        let (s, r) = from_asset_dir(&d);
        (s, r, format!("assets dir {}", d.display()))
    } else if let Some(root) = env_path("POB_ROOT") {
        (
            root.join("src"),
            root.join("runtime/SimpleGraphic"),
            format!("POB_ROOT {}", root.display()),
        )
    } else if let Some(src) = find_up(".PathOfBuilding/src") {
        let root = src.parent().map(Path::to_path_buf).unwrap_or_default();
        (
            src,
            root.join("runtime/SimpleGraphic"),
            format!("found {}", root.display()),
        )
    } else {
        return Err(anyhow!(
            "assets not found: pass --assets DIR or set POB_ASSETS / POB_ROOT (docs/perf/PROFILING.md \"Inputs\")"
        ));
    };
    let dump = if let Some(d) = data
        .map(Path::to_path_buf)
        .or_else(|| env_path("POB_DUMP_DIR"))
    {
        d
    } else if src.join("data.bin").is_file() {
        src.clone()
    } else if let Some(p) = find_up("data/dump/data.bin") {
        p.parent().map(Path::to_path_buf).unwrap_or_default()
    } else {
        return Err(anyhow!(
            "game data not found: pass --data DIR or set POB_DUMP_DIR (a directory with data.bin)"
        ));
    };
    if !dump.join("data.bin").is_file() {
        return Err(anyhow!("{}: no data.bin", dump.display()));
    }
    Ok(Inputs {
        src: canon(src),
        runtime: canon(runtime),
        dump: canon(dump),
        how,
    })
}

/// This crate's directory (scenarios and fixtures), from the build or the working directory.
pub fn crate_dir() -> PathBuf {
    if let Some(d) = env_path("POB_PROF_DIR") {
        return d;
    }
    let built = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if built.join("scenarios").is_dir() {
        return built;
    }
    find_up("crates/pob-prof").unwrap_or(built)
}

/// `crates/pob-prof/scenarios`.
pub fn scenario_dir() -> PathBuf {
    crate_dir().join("scenarios")
}
