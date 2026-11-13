//! `tests/golden/MANIFEST.json` (GOLDEN-PLAN §2.1, §2.3, §2.4, §3.5).
//!
//! It lists every golden file with its set, BLAKE3, line count, size on disk, origin and pins,
//! every committed build XML with its BLAKE3 (and the paths that alias it), the size caps and
//! the measured sizes. The runner walks it: a file it lists must exist with that hash (no
//! silent drift), and a file it does not list is not run.
//!
//! Pins (§2.4, §3.5): `compared` (the number of compared values may not drop below it),
//! `bitExact` (the number of bit-identical lines may not drop below it), `known` (the number of
//! `$known` values per id may not grow above it), `requiredKeys` (keys that must be in the file
//! and compared). A set may add its own pins as extra fields (`orderDependent`, …); they are
//! kept as written.

use std::collections::BTreeMap;
use std::path::Path;

use serde::{Deserialize, Serialize};
use serde_json::Value as J;

use crate::{bail, Error, Result};

/// The manifest's file name in the goldens directory.
pub const MANIFEST_FILE: &str = "MANIFEST.json";

/// The manifest format version.
pub const MANIFEST_VERSION: u64 = 1;

/// The PoB the goldens were recorded from.
pub const POB_VERSION: &str = "v2.67.2@16de4b82d";

/// The size caps of §2.3, in bytes.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Caps {
    /// The `git gc --aggressive` pack of `tests/golden/` alone.
    pub packed: u64,
    /// The checked-out bytes of `tests/golden/`.
    pub working_tree: u64,
    /// Any one file on disk (plain or zstd).
    pub per_file: u64,
}

impl Default for Caps {
    fn default() -> Self {
        Self {
            packed: 80_000_000,
            working_tree: 250_000_000,
            per_file: 8_000_000,
        }
    }
}

/// The measured sizes (`scripts/golden/size.sh`).
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Sizes {
    pub working_tree: u64,
    pub packed: u64,
    /// The largest file on disk, and its path.
    pub largest_file: u64,
    #[serde(default, skip_serializing_if = "String::is_empty")]
    pub largest_path: String,
    /// What was measured: `<commit> <date>`.
    pub measured: String,
}

/// A committed build XML (GOLD2 fills these).
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct BuildEntry {
    /// Relative to the goldens directory (`builds/all57/allflame_exp_000.xml`).
    pub path: String,
    pub blake3: String,
    pub bytes: u64,
    /// Other paths with the same content (stored once, §2.3).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub aliases: Vec<String>,
    /// `<build key> + <transform>` for a derived fixture (§1.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub derived_from: Option<String>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, J>,
}

/// An overlay file (§3.6) of a golden file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
pub struct OverlayEntry {
    pub path: String,
    pub blake3: String,
    pub lines: usize,
}

/// A golden file.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct FileEntry {
    /// Relative to the goldens directory (`e2e/ca/all57/allflame_exp_000.jsonl`).
    pub path: String,
    pub set: String,
    /// The header's key.
    pub key: String,
    /// BLAKE3 of the bytes on disk.
    pub blake3: String,
    /// Lines after the header.
    pub lines: usize,
    /// Bytes on disk.
    pub bytes: u64,
    /// `pob` (recorded from PoB), or `rust@<sha>` after a rebase bless (§3.6).
    pub origin: String,
    /// Pin: the compared count may not drop below it.
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub compared: Option<usize>,
    /// Pin: the bit-exact line count may not drop below it (§3.5).
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub bit_exact: Option<usize>,
    /// Pin: the most `$known` values per id.
    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub known: BTreeMap<String, usize>,
    /// Keys that must be present and compared (OF-I's dps30 keys, §1.2).
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub required_keys: Vec<String>,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub overlay: Option<OverlayEntry>,
    /// Set-specific pins and notes.
    #[serde(flatten)]
    pub extra: BTreeMap<String, J>,
}

impl FileEntry {
    /// The tier of the file's set: the part before the first `.` (`e2e`, `fn`, `fuzz`).
    #[must_use]
    pub fn tier(&self) -> &str {
        tier_of(&self.set)
    }
}

/// The tier of a set name.
#[must_use]
pub fn tier_of(set: &str) -> &str {
    set.split('.').next().unwrap_or(set)
}

/// The whole manifest.
#[derive(Debug, Clone, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Manifest {
    pub manifest: u64,
    pub pob: String,
    pub caps: Caps,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub sizes: Option<Sizes>,
    /// Sorted by path.
    #[serde(default)]
    pub builds: Vec<BuildEntry>,
    /// Sorted by path.
    #[serde(default)]
    pub files: Vec<FileEntry>,
    #[serde(flatten)]
    pub extra: BTreeMap<String, J>,
}

impl Default for Manifest {
    fn default() -> Self {
        Self {
            manifest: MANIFEST_VERSION,
            pob: POB_VERSION.to_owned(),
            caps: Caps::default(),
            sizes: None,
            builds: Vec::new(),
            files: Vec::new(),
            extra: BTreeMap::new(),
        }
    }
}

impl Manifest {
    /// Read `<root>/MANIFEST.json`. A missing manifest is an error: the goldens are committed.
    pub fn load(root: &Path) -> Result<Self> {
        let p = root.join(MANIFEST_FILE);
        let text =
            std::fs::read_to_string(&p).map_err(|e| Error(format!("{}: {e}", p.display())))?;
        Self::parse(&text).map_err(|e| Error(format!("{}: {e}", p.display())))
    }

    pub fn parse(text: &str) -> Result<Self> {
        let m: Self = serde_json::from_str(text).map_err(|e| Error(e.to_string()))?;
        if m.manifest != MANIFEST_VERSION {
            bail!(
                "manifest version {} (this reader is {MANIFEST_VERSION})",
                m.manifest
            );
        }
        Ok(m)
    }

    /// The canonical text: pretty JSON, files and builds sorted by path, a final newline.
    #[must_use]
    pub fn to_text(&self) -> String {
        let mut m = self.clone();
        m.files.sort_by(|a, b| a.path.cmp(&b.path));
        m.builds.sort_by(|a, b| a.path.cmp(&b.path));
        let mut s = serde_json::to_string_pretty(&m).unwrap_or_default();
        s.push('\n');
        s
    }

    /// Write `<root>/MANIFEST.json` atomically.
    pub fn save(&self, root: &Path) -> Result<()> {
        crate::write_atomic(&root.join(MANIFEST_FILE), self.to_text().as_bytes())
    }

    #[must_use]
    pub fn file(&self, path: &str) -> Option<&FileEntry> {
        self.files.iter().find(|f| f.path == path)
    }

    pub fn file_mut(&mut self, path: &str) -> Option<&mut FileEntry> {
        self.files.iter_mut().find(|f| f.path == path)
    }

    /// Insert or replace a file entry (by path).
    pub fn upsert_file(&mut self, e: FileEntry) {
        if let Some(x) = self.file_mut(&e.path) {
            *x = e;
        } else {
            self.files.push(e);
            self.files.sort_by(|a, b| a.path.cmp(&b.path));
        }
    }

    /// The build entry whose path or one of whose aliases is `path`.
    #[must_use]
    pub fn build(&self, path: &str) -> Option<&BuildEntry> {
        self.builds
            .iter()
            .find(|b| b.path == path || b.aliases.iter().any(|a| a == path))
    }

    /// The files of one tier (`e2e`, `fn`, `fuzz`), in path order.
    #[must_use]
    pub fn tier_files(&self, tier: &str) -> Vec<&FileEntry> {
        self.files.iter().filter(|f| f.tier() == tier).collect()
    }

    /// Every file and overlay checked against the disk: present, with the listed hash and line
    /// count, within the per-file cap. Returns one message per problem.
    #[must_use]
    pub fn check(&self, root: &Path) -> Vec<String> {
        let mut out = Vec::new();
        for f in &self.files {
            match crate::read_bytes(&root.join(&f.path)) {
                Err(e) => out.push(e.0),
                Ok(bytes) => {
                    let h = crate::blake3_hex(&bytes);
                    if h != f.blake3 {
                        out.push(format!(
                            "{}: BLAKE3 {h}, MANIFEST says {} (the file changed and MANIFEST did not)",
                            f.path, f.blake3
                        ));
                    }
                    if bytes.len() as u64 != f.bytes {
                        out.push(format!(
                            "{}: {} bytes, MANIFEST says {}",
                            f.path,
                            bytes.len(),
                            f.bytes
                        ));
                    }
                    if bytes.len() as u64 > self.caps.per_file {
                        out.push(format!(
                            "{}: {} bytes on disk, over the per-file cap of {}",
                            f.path,
                            bytes.len(),
                            self.caps.per_file
                        ));
                    }
                }
            }
            if let Some(o) = &f.overlay {
                match crate::read_bytes(&root.join(&o.path)) {
                    Err(e) => out.push(e.0),
                    Ok(bytes) if crate::blake3_hex(&bytes) != o.blake3 => out.push(format!(
                        "{}: BLAKE3 differs from MANIFEST (overlay of {})",
                        o.path, f.path
                    )),
                    Ok(_) => {}
                }
            }
        }
        for b in &self.builds {
            match crate::read_bytes(&root.join(&b.path)) {
                Err(e) => out.push(e.0),
                Ok(bytes) if crate::blake3_hex(&bytes) != b.blake3 => {
                    out.push(format!("{}: BLAKE3 differs from MANIFEST", b.path));
                }
                Ok(_) => {}
            }
        }
        if let Some(s) = &self.sizes {
            if s.packed > self.caps.packed {
                out.push(format!(
                    "packed size {} over the cap {}",
                    s.packed, self.caps.packed
                ));
            }
            if s.working_tree > self.caps.working_tree {
                out.push(format!(
                    "working-tree size {} over the cap {}",
                    s.working_tree, self.caps.working_tree
                ));
            }
        }
        out
    }

    /// Re-read a listed file (and its overlay) from disk: BLAKE3, bytes, line count. The pins
    /// are kept. Used after the recorder or a bless rewrote it.
    pub fn refresh_file(&mut self, root: &Path, path: &str) -> Result<()> {
        let bytes = crate::read_bytes(&root.join(path))?;
        let gf =
            crate::format::GoldenFile::parse(&crate::format::text_of(Path::new(path), &bytes)?)
                .map_err(|e| Error(format!("{path}: {e}")))?;
        let Some(f) = self.file_mut(path) else {
            bail!("{path}: not in MANIFEST");
        };
        f.blake3 = crate::blake3_hex(&bytes);
        f.bytes = bytes.len() as u64;
        f.lines = gf.lines.len();
        f.set.clone_from(&gf.header.set);
        f.key.clone_from(&gf.header.key);
        if let Some(o) = &mut f.overlay {
            let ob = crate::read_bytes(&root.join(&o.path))?;
            o.blake3 = crate::blake3_hex(&ob);
            o.lines = ob.split(|&b| b == b'\n').filter(|l| !l.is_empty()).count();
        }
        Ok(())
    }
}

/// A new entry for a golden file written at `<root>/<path>` (the recorder's call): hash, size
/// and lines from the bytes, origin `pob`, no pins.
#[must_use]
pub fn entry_for(path: &str, file: &crate::format::GoldenFile, bytes: &[u8]) -> FileEntry {
    FileEntry {
        path: path.to_owned(),
        set: file.header.set.clone(),
        key: file.header.key.clone(),
        blake3: crate::blake3_hex(bytes),
        lines: file.lines.len(),
        bytes: bytes.len() as u64,
        origin: "pob".to_owned(),
        compared: None,
        bit_exact: None,
        known: BTreeMap::new(),
        required_keys: Vec::new(),
        overlay: None,
        extra: BTreeMap::new(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{GoldenFile, Header, Line};
    use serde_json::json;

    #[test]
    fn round_trip_and_check() {
        let dir = std::env::temp_dir().join(format!("pob-golden-man-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&dir);
        let gf = GoldenFile::new(
            Header::new("e2e.ca", "all57/x"),
            vec![Line::e2e("a", json!(1))],
        );
        let path = "e2e/ca/all57/x.jsonl";
        let bytes = gf.write(&dir.join(path)).unwrap();
        let mut m = Manifest::default();
        let mut e = entry_for(path, &gf, &bytes);
        e.compared = Some(1);
        e.extra.insert("orderDependent".into(), json!({"gems": 10}));
        m.upsert_file(e);
        assert!(m.check(&dir).is_empty(), "{:?}", m.check(&dir));
        m.save(&dir).unwrap();
        let back = Manifest::load(&dir).unwrap();
        assert_eq!(back, m);
        assert_eq!(back.to_text(), m.to_text());
        assert!(m.to_text().contains("\"orderDependent\""));
        assert_eq!(back.tier_files("e2e").len(), 1);
        assert!(back.tier_files("fn").is_empty());

        // drift: the file changes, MANIFEST does not
        let gf2 = GoldenFile::new(
            Header::new("e2e.ca", "all57/x"),
            vec![Line::e2e("a", json!(2))],
        );
        gf2.write(&dir.join(path)).unwrap();
        let problems = m.check(&dir);
        assert!(
            problems.iter().any(|p| p.contains("BLAKE3")),
            "{problems:?}"
        );
        m.refresh_file(&dir, path).unwrap();
        assert!(m.check(&dir).is_empty());
        assert_eq!(m.files[0].compared, Some(1), "pins survive a refresh");
        // the per-file cap
        m.caps.per_file = 10;
        assert!(m.check(&dir).iter().any(|p| p.contains("per-file cap")));
        let _ = std::fs::remove_dir_all(&dir);
    }
}
