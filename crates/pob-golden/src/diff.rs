//! `golden diff` (GOLDEN-PLAN §2.4): two states of the goldens joined by key.
//!
//! Each side is read through its own MANIFEST: every listed file, decompressed, with its overlay
//! applied, so the diff shows what the runner will compare with. Old and new values are joined
//! by (file, line key); a value counts as changed unless the two are equal under `num_eq`
//! (structurally: markers compare as values). The summary gives, per set and per key, the
//! counts of changed, added and removed values, the 20 largest relative changes of numbers, and
//! the files (builds) affected.
//!
//! A side is a directory ([`Side::from_dir`]) or a git revision ([`Side::from_git`], read with
//! one `git ls-tree` and one `git cat-file --batch`). A revision without goldens is an empty
//! side, so the first recording shows as all added.

use std::collections::BTreeMap;
use std::fmt::Write as _;
use std::io::{BufRead, BufReader, Read, Write};
use std::path::{Path, PathBuf};
use std::process::{Command, Stdio};

use serde_json::Value as J;

use crate::compare::num_close;
use crate::encode::{canonical, decode_num};
use crate::format::{text_of, GoldenFile};
use crate::manifest::{Manifest, MANIFEST_FILE};
use crate::overlay::Overlay;
use crate::{bail, Error, Result};

/// Whether a path names a golden or overlay file (`.jsonl`, `.jsonl.zst`).
fn is_golden_name(p: &str) -> bool {
    let p = Path::new(p);
    let ext = |e: &str| p.extension().is_some_and(|x| x.eq_ignore_ascii_case(e));
    ext("jsonl") || (ext("zst") && is_golden_name(&p.with_extension("").to_string_lossy()))
}

/// How many relative changes the summary lists.
pub const TOP_CHANGES: usize = 20;

/// The raw files of one side, by path relative to the goldens directory.
#[derive(Debug, Clone, Default)]
pub struct Side {
    pub label: String,
    pub files: BTreeMap<String, Vec<u8>>,
}

impl Side {
    /// The goldens directory `root` as it is on disk (MANIFEST, and the files it lists).
    pub fn from_dir(root: &Path) -> Result<Self> {
        let mut side = Self {
            label: root.display().to_string(),
            files: BTreeMap::new(),
        };
        let mp = root.join(MANIFEST_FILE);
        if !mp.exists() {
            return Ok(side);
        }
        let text = crate::read_bytes(&mp)?;
        let m = Manifest::parse(&String::from_utf8_lossy(&text))?;
        side.files.insert(MANIFEST_FILE.to_owned(), text);
        for f in &m.files {
            side.files
                .insert(f.path.clone(), crate::read_bytes(&root.join(&f.path))?);
            if let Some(o) = &f.overlay {
                side.files
                    .insert(o.path.clone(), crate::read_bytes(&root.join(&o.path))?);
            }
        }
        Ok(side)
    }

    /// The goldens directory `root` at git revision `rev`.
    pub fn from_git(root: &Path, rev: &str) -> Result<Self> {
        let git = |args: &[&str]| -> Result<Vec<u8>> {
            let o = Command::new("git")
                .args(args)
                .current_dir(root.parent().unwrap_or(root))
                .output()
                .map_err(|e| Error(format!("git: {e}")))?;
            if !o.status.success() {
                bail!(
                    "git {}: {}",
                    args.join(" "),
                    String::from_utf8_lossy(&o.stderr).trim()
                );
            }
            Ok(o.stdout)
        };
        let top = String::from_utf8_lossy(&git(&["rev-parse", "--show-toplevel"])?)
            .trim()
            .to_owned();
        let canon = |p: &Path| std::fs::canonicalize(p).unwrap_or_else(|_| p.to_path_buf());
        let rel = canon(root)
            .strip_prefix(canon(Path::new(&top)))
            .map(|p| p.to_string_lossy().replace('\\', "/"))
            .map_err(|_| {
                Error(format!(
                    "{} is not inside the repository {top}",
                    root.display()
                ))
            })?;
        // --full-tree: the pathspec and the listed paths are relative to the top, not the cwd
        let tree = git(&[
            "ls-tree",
            "--full-tree",
            "-r",
            "-z",
            rev,
            "--",
            &format!("{rel}/"),
        ])?;
        let mut entries: Vec<(String, String)> = Vec::new();
        for rec in tree.split(|&b| b == 0).filter(|r| !r.is_empty()) {
            let rec = String::from_utf8_lossy(rec);
            let Some((meta, path)) = rec.split_once('\t') else {
                continue;
            };
            let mut it = meta.split_whitespace();
            let (_, kind, hash) = (it.next(), it.next(), it.next());
            if kind != Some("blob") {
                continue;
            }
            let Some(hash) = hash else { continue };
            let Some(p) = path.strip_prefix(&format!("{rel}/")) else {
                continue;
            };
            if p == MANIFEST_FILE || is_golden_name(p) {
                entries.push((p.to_owned(), hash.to_owned()));
            }
        }
        let mut side = Self {
            label: rev.to_owned(),
            files: BTreeMap::new(),
        };
        if entries.is_empty() {
            return Ok(side);
        }
        let mut child = Command::new("git")
            .args(["cat-file", "--batch"])
            .current_dir(&top)
            .stdin(Stdio::piped())
            .stdout(Stdio::piped())
            .spawn()
            .map_err(|e| Error(format!("git cat-file: {e}")))?;
        let mut stdin = child
            .stdin
            .take()
            .ok_or_else(|| Error("git cat-file: no stdin".into()))?;
        let hashes: Vec<String> = entries.iter().map(|(_, h)| h.clone()).collect();
        let writer = std::thread::spawn(move || {
            for h in hashes {
                if writeln!(stdin, "{h}").is_err() {
                    break;
                }
            }
        });
        let mut out = BufReader::new(
            child
                .stdout
                .take()
                .ok_or_else(|| Error("git cat-file: no stdout".into()))?,
        );
        for (path, _) in &entries {
            let mut head = String::new();
            out.read_line(&mut head)
                .map_err(|e| Error(format!("git cat-file: {e}")))?;
            let size: usize = head
                .split_whitespace()
                .nth(2)
                .and_then(|s| s.parse().ok())
                .ok_or_else(|| Error(format!("git cat-file: {head:?}")))?;
            let mut buf = vec![0; size + 1];
            out.read_exact(&mut buf)
                .map_err(|e| Error(format!("git cat-file: {e}")))?;
            buf.pop();
            side.files.insert(path.clone(), buf);
        }
        let _ = writer.join();
        let _ = child.wait();
        Ok(side)
    }

    /// Every listed golden file of this side: path -> (set, file key, effective values).
    pub fn effective(&self) -> Result<BTreeMap<String, EffectiveFile>> {
        let mut out = BTreeMap::new();
        let Some(mtext) = self.files.get(MANIFEST_FILE) else {
            return Ok(out);
        };
        let m = Manifest::parse(&String::from_utf8_lossy(mtext))
            .map_err(|e| Error(format!("{}: MANIFEST: {e}", self.label)))?;
        for f in &m.files {
            let bytes = self.files.get(&f.path).ok_or_else(|| {
                Error(format!("{}: {} is listed but missing", self.label, f.path))
            })?;
            let base = GoldenFile::parse(&text_of(Path::new(&f.path), bytes)?)
                .map_err(|e| Error(format!("{}: {}: {e}", self.label, f.path)))?;
            let gf = match &f.overlay {
                Some(o) => {
                    let ob = self.files.get(&o.path).ok_or_else(|| {
                        Error(format!(
                            "{}: overlay {} is listed but missing",
                            self.label, o.path
                        ))
                    })?;
                    Overlay::parse(&String::from_utf8_lossy(ob))?.apply(&base).0
                }
                None => base,
            };
            out.insert(
                f.path.clone(),
                EffectiveFile {
                    set: f.set.clone(),
                    key: f.key.clone(),
                    values: gf.lines.into_iter().map(|l| (l.key, l.value)).collect(),
                },
            );
        }
        Ok(out)
    }
}

/// One golden file's values as the runner compares them.
#[derive(Debug, Clone, PartialEq)]
pub struct EffectiveFile {
    pub set: String,
    pub key: String,
    pub values: BTreeMap<String, J>,
}

/// Two values the same under `num_eq`, structurally (markers as values).
#[must_use]
pub fn same(a: &J, b: &J) -> bool {
    if let (Some(x), Some(y)) = (decode_num(a), decode_num(b)) {
        return num_close(x, y, crate::compare::REL_TOL);
    }
    match (a, b) {
        (J::Array(x), J::Array(y)) => {
            x.len() == y.len() && x.iter().zip(y).all(|(p, q)| same(p, q))
        }
        (J::Object(x), J::Object(y)) => {
            x.len() == y.len() && x.iter().all(|(k, p)| y.get(k).is_some_and(|q| same(p, q)))
        }
        _ => a == b,
    }
}

/// Counts of one set, one key or one file.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq)]
pub struct Counts {
    pub changed: usize,
    pub added: usize,
    pub removed: usize,
}

impl Counts {
    #[must_use]
    pub fn total(&self) -> usize {
        self.changed + self.added + self.removed
    }
}

/// A numeric change.
#[derive(Debug, Clone, PartialEq)]
pub struct NumChange {
    pub rel: f64,
    pub set: String,
    pub file_key: String,
    pub key: String,
    pub old: f64,
    pub new: f64,
}

/// The joined diff.
#[derive(Debug, Clone, Default)]
pub struct Summary {
    pub old: String,
    pub new: String,
    pub sets: BTreeMap<String, Counts>,
    /// (set, line key).
    pub keys: BTreeMap<(String, String), Counts>,
    /// path -> (set, file key, counts).
    pub files: BTreeMap<String, (String, String, Counts)>,
    pub files_added: usize,
    pub files_removed: usize,
    /// Sorted by relative change, largest first, at most [`TOP_CHANGES`].
    pub largest: Vec<NumChange>,
}

impl Summary {
    #[must_use]
    pub fn is_empty(&self) -> bool {
        self.files.is_empty()
    }

    #[must_use]
    pub fn totals(&self) -> Counts {
        let mut t = Counts::default();
        for c in self.sets.values() {
            t.changed += c.changed;
            t.added += c.added;
            t.removed += c.removed;
        }
        t
    }

    /// The printed summary.
    #[must_use]
    pub fn render(&self) -> String {
        let mut s = String::new();
        let t = self.totals();
        let _ = writeln!(
            s,
            "golden diff {} -> {}: {} files differ ({} added, {} removed); values: {} changed, {} added, {} removed",
            self.old,
            self.new,
            self.files.len(),
            self.files_added,
            self.files_removed,
            t.changed,
            t.added,
            t.removed
        );
        if self.is_empty() {
            return s;
        }
        let _ = writeln!(s, "per set:");
        for (set, c) in &self.sets {
            let n = self.files.values().filter(|(fs, _, _)| fs == set).count();
            let _ = writeln!(
                s,
                "  {set}: {} changed, {} added, {} removed, in {n} files",
                c.changed, c.added, c.removed
            );
        }
        let _ = writeln!(s, "per key:");
        let mut keys: Vec<_> = self.keys.iter().collect();
        keys.sort_by(|a, b| b.1.total().cmp(&a.1.total()).then_with(|| a.0.cmp(b.0)));
        for ((set, key), c) in keys.iter().take(50) {
            let _ = writeln!(
                s,
                "  {set} {key}: {} changed, {} added, {} removed",
                c.changed, c.added, c.removed
            );
        }
        if keys.len() > 50 {
            let _ = writeln!(s, "  … {} more keys", keys.len() - 50);
        }
        if !self.largest.is_empty() {
            let _ = writeln!(s, "largest relative changes:");
            for c in &self.largest {
                let _ = writeln!(
                    s,
                    "  {:.3e}  {} {} {}: {} -> {}",
                    c.rel, c.set, c.file_key, c.key, c.old, c.new
                );
            }
        }
        let _ = writeln!(s, "affected files:");
        for (path, (set, key, c)) in &self.files {
            let _ = writeln!(
                s,
                "  {path} ({set} {key}): {} changed, {} added, {} removed",
                c.changed, c.added, c.removed
            );
        }
        s
    }
}

/// Join `old` and `new` (module docs).
pub fn diff(old: &Side, new: &Side) -> Result<Summary> {
    let (a, b) = (old.effective()?, new.effective()?);
    let mut sum = Summary {
        old: old.label.clone(),
        new: new.label.clone(),
        ..Summary::default()
    };
    let empty = BTreeMap::new();
    let paths: std::collections::BTreeSet<&String> = a.keys().chain(b.keys()).collect();
    let mut changes: Vec<NumChange> = Vec::new();
    for path in paths {
        let (fa, fb) = (a.get(path), b.get(path));
        let meta = fb.or(fa).expect("in one side");
        let (set, file_key) = (meta.set.clone(), meta.key.clone());
        let va = fa.map_or(&empty, |f| &f.values);
        let vb = fb.map_or(&empty, |f| &f.values);
        let mut fc = Counts::default();
        let keys: std::collections::BTreeSet<&String> = va.keys().chain(vb.keys()).collect();
        for k in keys {
            // 0 changed, 1 added, 2 removed
            let what = match (va.get(k), vb.get(k)) {
                (Some(x), Some(y)) if same(x, y) => continue,
                (Some(x), Some(y)) => {
                    if let (Some(p), Some(q)) = (decode_num(x), decode_num(y)) {
                        let rel = if p.is_finite() && q.is_finite() {
                            (q - p).abs() / p.abs().max(1.0)
                        } else {
                            f64::INFINITY
                        };
                        changes.push(NumChange {
                            rel,
                            set: set.clone(),
                            file_key: file_key.clone(),
                            key: k.clone(),
                            old: p,
                            new: q,
                        });
                    }
                    0
                }
                (None, Some(_)) => 1,
                (Some(_), None) => 2,
                (None, None) => continue,
            };
            let kc = sum.keys.entry((set.clone(), k.clone())).or_default();
            for c in [&mut fc, kc] {
                match what {
                    0 => c.changed += 1,
                    1 => c.added += 1,
                    _ => c.removed += 1,
                }
            }
        }
        if fc.total() > 0 || fa.is_none() || fb.is_none() {
            if fa.is_none() {
                sum.files_added += 1;
            }
            if fb.is_none() {
                sum.files_removed += 1;
            }
            let sc = sum.sets.entry(set.clone()).or_default();
            sc.changed += fc.changed;
            sc.added += fc.added;
            sc.removed += fc.removed;
            sum.files.insert(path.clone(), (set, file_key, fc));
        }
    }
    changes.sort_by(|x, y| {
        y.rel
            .partial_cmp(&x.rel)
            .unwrap_or(std::cmp::Ordering::Equal)
            .then_with(|| (&x.set, &x.file_key, &x.key).cmp(&(&y.set, &y.file_key, &y.key)))
    });
    changes.truncate(TOP_CHANGES);
    sum.largest = changes;
    Ok(sum)
}

/// `golden diff <rev>`: the goldens at `rev` against the working tree.
pub fn diff_rev(root: &Path, rev: &str) -> Result<Summary> {
    diff(&Side::from_git(root, rev)?, &Side::from_dir(root)?)
}

/// `golden diff --old <dir> --new <dir>`.
pub fn diff_dirs(old: &Path, new: &Path) -> Result<Summary> {
    diff(&Side::from_dir(old)?, &Side::from_dir(new)?)
}

/// For callers that want a canonical text of a value (the bin's `--values` listing).
#[must_use]
pub fn show(v: Option<&J>) -> String {
    v.map_or_else(|| "-".to_owned(), canonical)
}

/// The default goldens directory as a path (the bin's default).
#[must_use]
pub fn default_root() -> PathBuf {
    crate::golden_root()
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::format::{Header, Line};
    use crate::manifest::entry_for;
    use crate::overlay::OverlayLine;
    use serde_json::json;

    type Files<'a> = &'a [(&'a str, &'a str, &'a [(&'a str, J)])];

    fn write(root: &Path, files: Files<'_>) -> Manifest {
        let mut m = Manifest::default();
        for (path, key, lines) in files {
            let gf = GoldenFile::new(
                Header::new("e2e.ca", key),
                lines
                    .iter()
                    .map(|(k, v)| Line::e2e(*k, v.clone()))
                    .collect(),
            );
            let bytes = gf.write(&root.join(path)).unwrap();
            m.upsert_file(entry_for(path, &gf, &bytes));
        }
        m.save(root).unwrap();
        m
    }

    #[test]
    fn joins_by_key() {
        let base = std::env::temp_dir().join(format!("pob-golden-diff-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let (a, b) = (base.join("a"), base.join("b"));
        write(
            &a,
            &[
                (
                    "e2e/ca/x.jsonl",
                    "all57/x",
                    &[
                        ("main.Life", json!(100)),
                        ("main.Mana", json!(50)),
                        ("main.Gone", json!(1)),
                    ],
                ),
                ("e2e/ca/y.jsonl", "all57/y", &[("main.Life", json!(7))]),
                (
                    "e2e/ca/old.jsonl.zst",
                    "all57/old",
                    &[("main.Life", json!(1))],
                ),
            ],
        );
        let mut m = write(
            &b,
            &[
                (
                    "e2e/ca/x.jsonl",
                    "all57/x",
                    &[
                        ("main.Life", json!(110)),
                        ("main.Mana", json!(50.000_000_000_01)),
                        ("main.New", json!("s")),
                    ],
                ),
                ("e2e/ca/y.jsonl", "all57/y", &[("main.Life", json!(7))]),
            ],
        );
        let s = diff_dirs(&a, &b).unwrap();
        assert_eq!(
            s.totals(),
            Counts {
                changed: 1,
                added: 1,
                removed: 2
            }
        );
        assert_eq!((s.files_added, s.files_removed), (0, 1));
        assert_eq!(s.files.len(), 2);
        assert_eq!(s.largest.len(), 1);
        assert!((s.largest[0].rel - 0.1).abs() < 1e-12);
        assert_eq!(
            s.keys[&("e2e.ca".to_owned(), "main.Life".to_owned())],
            Counts {
                changed: 1,
                added: 0,
                removed: 1
            }
        );
        let text = s.render();
        assert!(
            text.contains("e2e.ca main.Life: 1 changed, 0 added, 1 removed"),
            "{text}"
        );
        assert!(
            text.contains("e2e/ca/old.jsonl.zst (e2e.ca all57/old)"),
            "{text}"
        );
        assert!(diff_dirs(&b, &b).unwrap().is_empty());

        // an overlay counts: the diff shows what the runner compares with
        let ov = Overlay {
            lines: [(
                "main.Life".to_owned(),
                OverlayLine {
                    key: "main.Life".into(),
                    pob: json!(7),
                    new: json!(8),
                    reason: "r".into(),
                    commit: "c".into(),
                },
            )]
            .into(),
        };
        ov.write(&b.join("overlay/e2e/ca/y.bless.jsonl")).unwrap();
        m.file_mut("e2e/ca/y.jsonl").unwrap().overlay = Some(crate::manifest::OverlayEntry {
            path: "overlay/e2e/ca/y.bless.jsonl".into(),
            blake3: String::new(),
            lines: 0,
        });
        m.refresh_file(&b, "e2e/ca/y.jsonl").unwrap();
        m.save(&b).unwrap();
        let s = diff_dirs(&a, &b).unwrap();
        assert_eq!(s.totals().changed, 2);
        // a side without goldens is empty: everything is added
        let s = diff_dirs(&base.join("none"), &a).unwrap();
        assert_eq!((s.files_added, s.totals().added), (3, 5));
        let _ = std::fs::remove_dir_all(&base);
    }

    /// A side read from a git revision (`ls-tree` + `cat-file --batch`) equals the directory it
    /// was committed from, shards included.
    #[test]
    fn git_revision_side() {
        let base = std::env::temp_dir().join(format!("pob-golden-diffgit-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&base);
        let root = base.join("repo/tests/golden");
        write(
            &root,
            &[
                ("e2e/ca/x.jsonl", "all57/x", &[("main.Life", json!(100))]),
                (
                    "fn/lut/lut.00.jsonl.zst",
                    "00",
                    &[("a", json!(1)), ("b", json!("s"))],
                ),
            ],
        );
        let git = |args: &[&str]| {
            let o = std::process::Command::new("git")
                .args(args)
                .current_dir(base.join("repo"))
                .env("GIT_AUTHOR_NAME", "t")
                .env("GIT_AUTHOR_EMAIL", "t@t")
                .env("GIT_COMMITTER_NAME", "t")
                .env("GIT_COMMITTER_EMAIL", "t@t")
                .output()
                .unwrap();
            assert!(
                o.status.success(),
                "git {args:?}: {}",
                String::from_utf8_lossy(&o.stderr)
            );
        };
        git(&["init", "-q"]);
        git(&["add", "-A"]);
        git(&["commit", "-q", "-m", "goldens"]);
        let s = diff_rev(&root, "HEAD").unwrap();
        assert!(s.is_empty(), "{}", s.render());
        // a changed value in the working tree shows against HEAD
        write(
            &root,
            &[
                ("e2e/ca/x.jsonl", "all57/x", &[("main.Life", json!(101))]),
                (
                    "fn/lut/lut.00.jsonl.zst",
                    "00",
                    &[("a", json!(1)), ("b", json!("s"))],
                ),
            ],
        );
        let s = diff_rev(&root, "HEAD").unwrap();
        assert_eq!(
            s.totals(),
            Counts {
                changed: 1,
                added: 0,
                removed: 0
            }
        );
        assert_eq!(s.largest[0].key, "main.Life");
        assert!(Side::from_git(&root, "no-such-rev").is_err());
        let _ = std::fs::remove_dir_all(&base);
    }

    #[test]
    fn structural_equality() {
        assert!(same(
            &json!({"$anyOf": [1, 2.000_000_000_001]}),
            &json!({"$anyOf": [1, 2]})
        ));
        assert!(!same(
            &json!({"$anyOf": [1, 2]}),
            &json!({"$anyOf": [2, 1]})
        ));
        assert!(same(&json!({"$num": "nan"}), &json!({"$num": "nan"})));
        assert!(!same(&json!({"a": 1}), &json!({"a": 1, "b": 2})));
    }
}
