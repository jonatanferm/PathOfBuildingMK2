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
//! 3. the first `.PathOfBuilding/src` in a search root: the ancestors of the working directory, of
//!    the executable and of this crate's source directory, then the main checkout of the linked
//!    git worktree any of them is in (its `.git` file names it), the order `pob-testpaths` uses. A
//!    worktree outside the main checkout (`/tmp/…`) finds the gitignored inputs this way.
//!
//! **Game data**: `--data DIR`, else `POB_DUMP_DIR`, else `<assets>/data.bin`'s directory, else the
//! first `data/dump/data.bin` in a search root (as above).
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

/// The directories the search starts from: the working directory, the executable's directory and
/// this crate's source directory (when it still exists).
fn starts() -> Vec<PathBuf> {
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
    let built = PathBuf::from(env!("CARGO_MANIFEST_DIR"));
    if built.is_dir() {
        starts.push(built);
    }
    starts
}

/// `<main>` for a linked git worktree whose `.git` file (in `start` or an ancestor) says
/// `gitdir: <main>/.git/worktrees/<name>` (as `pob-testpaths`).
fn linked_main_checkout(start: &Path) -> Option<PathBuf> {
    let dot_git = start
        .ancestors()
        .map(|a| a.join(".git"))
        .find(|p| p.exists())?;
    let text = std::fs::read_to_string(&dot_git).ok()?; // a directory in the main checkout
    let gitdir = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
    Some(gitdir.parent()?.parent()?.parent()?.to_path_buf())
}

/// The search roots, in order: every ancestor of each start, then the main checkouts of the linked
/// worktrees the starts are in.
pub fn search_roots() -> Vec<PathBuf> {
    let starts = starts();
    let mut out: Vec<PathBuf> = Vec::new();
    for s in &starts {
        out.extend(s.ancestors().map(Path::to_path_buf));
    }
    out.extend(starts.iter().filter_map(|s| linked_main_checkout(s)));
    let mut seen = std::collections::BTreeSet::new();
    out.retain(|p| seen.insert(p.clone()));
    out
}

/// The first `<root>/<rel>` that exists, over [`search_roots`].
pub fn find_up(rel: &str) -> Option<PathBuf> {
    search_roots()
        .into_iter()
        .map(|r| r.join(rel))
        .find(|p| p.exists())
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

#[cfg(test)]
mod tests {
    use super::*;

    /// A linked worktree outside the main checkout reaches it through its `.git` file.
    #[test]
    fn linked_worktree_names_its_main_checkout() {
        let d = std::env::temp_dir().join(format!("pob-prof-paths-{}", std::process::id()));
        let sub = d.join("crates/x");
        std::fs::create_dir_all(&sub).unwrap();
        std::fs::write(d.join(".git"), "gitdir: /some/main/.git/worktrees/wt\n").unwrap();
        let got = linked_main_checkout(&sub);
        std::fs::remove_dir_all(&d).unwrap();
        assert_eq!(got, Some(PathBuf::from("/some/main")));
        // this crate's source directory is a search start
        let r = search_roots();
        assert!(
            r.iter()
                .any(|p| p.join("crates/pob-prof/Cargo.toml").is_file()),
            "{r:?}"
        );
    }
}
