//! Test inputs and the test-skip policy (unit HYG1). Dev-dependency only.
//!
//! The inputs the port's tests read are gitignored (AMENDMENTS A5), so a git
//! worktree does not contain them: PoB's checkout (`.PathOfBuilding`), `data/`
//! (`data/dump` from `pob-dump`) and `.parity` (build corpus, oracle dumps).
//! Every lookup here searches, in order:
//!
//! 1. an explicit environment override (`POB_SRC`, `POB_ROOT`, `POB_DATA_DIR`,
//!    `POB_DUMP_DIR`, `POB_PARITY_DIR`; an override that does not hold the
//!    input is an error, never silently replaced),
//! 2. this workspace and its ancestors (a worktree nested in the main
//!    checkout reaches the main checkout this way),
//! 3. the main checkout of the linked git worktree (its `.git` file),
//! 4. the main checkout's absolute path, [`MAIN_CHECKOUT`].
//!
//! **Policy.** A test whose input is missing FAILS. With `POB_ALLOW_SKIP=1` it
//! prints `SKIPPED: …` and returns instead. Use [`require`]:
//!
//! ```no_run
//! # fn t() {
//! let Some(src) = pob_testpaths::require(pob_testpaths::pob_src(), "PoB checkout (.PathOfBuilding/src)") else {
//!     return; // only with POB_ALLOW_SKIP=1
//! };
//! # let _ = src; }
//! ```

use std::path::{Path, PathBuf};

/// The main checkout (AMENDMENTS A5: "use absolute paths into the main checkout").
pub const MAIN_CHECKOUT: &str = "/home/ferm/Work/projects/PathOfBuildingMK2";

/// `POB_ALLOW_SKIP=1`: tests may skip when an input is missing.
pub fn allow_skip() -> bool {
    std::env::var_os("POB_ALLOW_SKIP").is_some_and(|v| v == "1")
}

/// The test-skip policy: `Some(v)` passes through. `None` panics (the default),
/// or with `POB_ALLOW_SKIP=1` prints `SKIPPED: <what>` and returns `None`, and
/// the caller returns from the test.
#[track_caller]
pub fn require<T>(v: Option<T>, what: &str) -> Option<T> {
    if v.is_some() {
        return v;
    }
    if allow_skip() {
        eprintln!("SKIPPED (POB_ALLOW_SKIP=1): missing test input: {what}");
        return None;
    }
    let roots: Vec<String> = roots().iter().map(|r| r.display().to_string()).collect();
    panic!(
        "missing test input: {what}\n  searched the environment overrides and: {}\n  \
         (set the override, create the input, or run with POB_ALLOW_SKIP=1 to skip)",
        roots.join(", ")
    );
}

/// The roots searched after the environment: this workspace and its
/// ancestors, the linked worktree's main checkout, then [`MAIN_CHECKOUT`].
pub fn roots() -> Vec<PathBuf> {
    let here = Path::new(env!("CARGO_MANIFEST_DIR"));
    let mut out: Vec<PathBuf> = here
        .ancestors()
        .skip(2) // crates/pob-testpaths -> workspace root first
        .map(Path::to_path_buf)
        .collect();
    if let Some(m) = linked_main_checkout(here) {
        out.push(m);
    }
    out.push(PathBuf::from(MAIN_CHECKOUT));
    let mut seen = Vec::new();
    out.retain(|p| {
        let key = p.canonicalize().unwrap_or_else(|_| p.clone());
        if seen.contains(&key) {
            false
        } else {
            seen.push(key);
            true
        }
    });
    out
}

/// `<main>` for a linked worktree whose `.git` file says
/// `gitdir: <main>/.git/worktrees/<name>`.
fn linked_main_checkout(start: &Path) -> Option<PathBuf> {
    let dot_git = start
        .ancestors()
        .map(|a| a.join(".git"))
        .find(|p| p.exists())?;
    let text = std::fs::read_to_string(&dot_git).ok()?; // a directory in the main checkout
    let gitdir = PathBuf::from(text.strip_prefix("gitdir:")?.trim());
    Some(gitdir.parent()?.parent()?.parent()?.to_path_buf())
}

/// The first `<root>/<rel>` (over [`roots`]) for which `ok` holds.
pub fn find_where(rel: &str, ok: impl Fn(&Path) -> bool) -> Option<PathBuf> {
    roots().into_iter().map(|r| r.join(rel)).find(|p| ok(p))
}

/// The first existing `<root>/<rel>`, e.g. `data/lua/modparser_env.lua` or
/// `.parity/builds/upstream_OccVortex.xml`.
pub fn find(rel: &str) -> Option<PathBuf> {
    find_where(rel, Path::exists)
}

/// The path an environment override names, when it is set.
fn env_dir(var: &str) -> Option<PathBuf> {
    std::env::var_os(var).map(PathBuf::from)
}

fn is_pob_root(p: &Path) -> bool {
    p.join("src/HeadlessWrapper.lua").is_file()
}

/// PoB's checkout root (`.PathOfBuilding`, holding `src/`, `runtime/`,
/// `spec/`). `$POB_ROOT` (the root) or `$POB_SRC` (`src/` or the root)
/// override.
pub fn pob_root() -> Option<PathBuf> {
    if let Some(p) = env_dir("POB_ROOT") {
        return Some(p).filter(|p| is_pob_root(p));
    }
    if let Some(p) = env_dir("POB_SRC") {
        return [
            p.clone(),
            p.parent().map(Path::to_path_buf).unwrap_or_default(),
        ]
        .into_iter()
        .find(|c| is_pob_root(c));
    }
    find_where(".PathOfBuilding", is_pob_root)
}

/// PoB's `src/` (`HeadlessWrapper.lua`, `Modules/`, …).
pub fn pob_src() -> Option<PathBuf> {
    pob_root().map(|r| r.join("src"))
}

/// The repository's `data/` (`$POB_DATA_DIR`), the one that holds `data/dump`
/// when any does.
pub fn data_dir() -> Option<PathBuf> {
    if let Some(p) = env_dir("POB_DATA_DIR") {
        return Some(p).filter(|p| p.is_dir());
    }
    dump_dir()
        .and_then(|d| d.parent().map(Path::to_path_buf))
        .or_else(|| find_where("data", Path::is_dir))
}

/// `data/dump` as `pob-dump` writes it (`manifest.json`; `$POB_DUMP_DIR`).
pub fn dump_dir() -> Option<PathBuf> {
    if let Some(p) = env_dir("POB_DUMP_DIR") {
        return Some(p)
            .filter(|p| p.join("manifest.json").is_file() || p.join("data.bin").is_file());
    }
    find_where("data/dump", |p| p.join("manifest.json").is_file())
}

/// `data/dump/data.bin` (`pob-dump --compile`).
pub fn data_bin() -> Option<PathBuf> {
    if let Some(p) = env_dir("POB_DUMP_DIR") {
        return Some(p.join("data.bin")).filter(|p| p.is_file());
    }
    find_where("data/dump/data.bin", |p| p.is_file())
}

/// `.parity` (`builds/` corpus, oracle dumps; `$POB_PARITY_DIR`). Under [`quick_corpus`]
/// (`POB_QUICK_CORPUS=1`) without `$POB_PARITY_DIR`, the quick corpus `.parity-quick`, the
/// directory `scripts/port/cargo.sh` points `$POB_PARITY_DIR` at for merge-time runs (CK2,
/// CCR-0147 §5: set alone, the variable used to keep the full corpus).
pub fn parity_dir() -> Option<PathBuf> {
    if let Some(p) = env_dir("POB_PARITY_DIR") {
        return Some(p).filter(|p| p.join("builds").is_dir());
    }
    let name = if quick_corpus() { ".parity-quick" } else { ".parity" };
    find_where(name, |p| p.join("builds").is_dir())
}

/// `.parity/builds/<stem>.xml`.
pub fn parity_build(stem: &str) -> Option<PathBuf> {
    parity_dir()
        .map(|d| d.join("builds").join(format!("{stem}.xml")))
        .filter(|p| p.is_file())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn roots_include_the_main_checkout() {
        let r = roots();
        assert!(r.contains(&PathBuf::from(MAIN_CHECKOUT)), "{r:?}");
        // The first root is this workspace (the dir holding crates/pob-testpaths).
        assert!(
            r[0].join("crates/pob-testpaths/Cargo.toml").is_file(),
            "{r:?}"
        );
    }

    /// The inputs this machine's tests need (the policy: fail, not skip).
    #[test]
    fn inputs_found() {
        let Some(src) = require(pob_src(), "PoB checkout (.PathOfBuilding/src)") else {
            return;
        };
        assert!(src.join("Modules/Data.lua").is_file());
        if let Some(d) = require(dump_dir(), "data/dump (pob-dump)") {
            assert!(d.join("manifest.json").is_file());
            assert_eq!(data_dir().as_deref(), d.parent());
        }
        if let Some(p) = require(parity_dir(), ".parity (build corpus)") {
            assert!(p.join("builds").is_dir());
        }
        assert!(find("Cargo.toml").is_some());
    }

    #[test]
    fn require_passes_values_through() {
        assert_eq!(require(Some(3), "x"), Some(3));
    }

    #[test]
    #[should_panic(expected = "missing test input: nothing")]
    fn require_fails_without_the_opt_out() {
        assert!(
            !allow_skip(),
            "missing test input: nothing (POB_ALLOW_SKIP=1 is set for this run)"
        );
        let _ = require(None::<()>, "nothing");
    }
}

/// `POB_QUICK_CORPUS=1` (set by `scripts/port/cargo.sh` when it points `$POB_PARITY_DIR` at the
/// quick corpus for merge-time runs): the build set is a deliberate subset, so tests treat the run
/// like a filtered one and skip whole-corpus coverage assertions (build counts, query floors).
/// Reviewers and the periodic full suite run without it and keep those assertions.
pub fn quick_corpus() -> bool {
    std::env::var_os("POB_QUICK_CORPUS").is_some_and(|v| v == "1")
}

