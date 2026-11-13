//! The calc goldens (docs/standalone/GOLDEN-PLAN.md §2-§3).
//!
//! PoB's answers were recorded once, while PoB still ran under LuaJIT (the `pob_golden_record`
//! bin of pob-oracle), and are committed under `tests/golden/` at the workspace root. This crate
//! reads them and checks the Rust calc against them, with no PoB, no `mlua` and no pob-oracle.
//!
//! * [`format`]: the `golden/1` file grammar (§3.2): a header line, then sorted `[key, value]`
//!   (E2E) or `[key, input, value]` (query) lines; plain `.jsonl` or `.jsonl.zst` shards.
//! * [`encode`]: the shared encodings of §1.1 (numbers, flag names, mod/1) and the merge of K
//!   recording runs into `$anyOf` / `$unordered` (§3.1). The recorder and the adapters use the
//!   same functions, so both sides write one schema.
//! * [`compare`]: the compare engine (§3.5): `num_eq`, the value markers, mod lists as a
//!   multiset per name with an order report, bit-exact counting.
//! * [`manifest`]: `tests/golden/MANIFEST.json`: every file with its BLAKE3, line count, pins
//!   and sizes (§2.1, §2.4).
//! * [`overlay`]: the bless overlays (`overlay/*.bless.jsonl`) and the bless / rebase workflow
//!   (§3.6).
//! * [`runner`]: walks MANIFEST for a tier (`e2e`, `fn`, `fuzz`), runs each file's adapter on the
//!   bounded pool ([`par`]), compares, checks the pins, blesses, and runs the negative control
//!   (`POB_GOLDEN_SELFTEST=1`, §4.4).
//! * [`diff`]: `golden diff` (§2.4): old and new goldens joined by key, with a summary.
//! * [`adapter`]: the only code that names `pob_calc` internals (§3.4). The refactor re-points
//!   only this layer.

pub mod adapter;
pub mod compare;
pub mod diff;
pub mod encode;
pub mod format;
pub mod manifest;
pub mod overlay;
pub mod par;
pub mod runner;

use std::path::{Path, PathBuf};

/// An error of the golden machinery (I/O, a malformed file, a broken pin): a message.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Error(pub String);

impl std::fmt::Display for Error {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(&self.0)
    }
}

impl std::error::Error for Error {}

impl From<std::io::Error> for Error {
    fn from(e: std::io::Error) -> Self {
        Self(e.to_string())
    }
}

pub type Result<T> = std::result::Result<T, Error>;

/// `Err(Error(format!(...)))`.
#[macro_export]
macro_rules! bail {
    ($($t:tt)*) => {
        return Err($crate::Error(format!($($t)*)))
    };
}

/// The variable that points the runner and `golden diff` at another goldens directory.
pub const GOLDEN_DIR_ENV: &str = "POB_GOLDEN_DIR";

/// The workspace root (two levels above this crate).
#[must_use]
pub fn workspace_root() -> PathBuf {
    let p = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    std::fs::canonicalize(&p).unwrap_or(p)
}

/// `$POB_GOLDEN_DIR`, else `<workspace>/tests/golden` (§2.1: the goldens live at the workspace
/// root, not in a crate).
#[must_use]
pub fn golden_root() -> PathBuf {
    std::env::var_os(GOLDEN_DIR_ENV)
        .filter(|v| !v.is_empty())
        .map_or_else(|| workspace_root().join("tests/golden"), PathBuf::from)
}

/// BLAKE3 of `bytes`, as lower-case hex.
#[must_use]
pub fn blake3_hex(bytes: &[u8]) -> String {
    blake3::hash(bytes).to_hex().to_string()
}

/// Write `bytes` to `path` atomically (a temp file in the same directory, then a rename), making
/// the parent directories.
pub fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let dir = path
        .parent()
        .ok_or_else(|| Error(format!("{}: no parent directory", path.display())))?;
    std::fs::create_dir_all(dir).map_err(|e| Error(format!("{}: {e}", dir.display())))?;
    let name = path
        .file_name()
        .ok_or_else(|| Error(format!("{}: no file name", path.display())))?
        .to_string_lossy();
    let tmp = dir.join(format!(".{name}.tmp{}", std::process::id()));
    std::fs::write(&tmp, bytes).map_err(|e| Error(format!("{}: {e}", tmp.display())))?;
    std::fs::rename(&tmp, path).map_err(|e| {
        let _ = std::fs::remove_file(&tmp);
        Error(format!("{}: {e}", path.display()))
    })
}

/// Read a file, naming it in the error.
pub fn read_bytes(path: &Path) -> Result<Vec<u8>> {
    std::fs::read(path).map_err(|e| Error(format!("{}: {e}", path.display())))
}
