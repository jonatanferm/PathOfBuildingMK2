//! The golden runner (GOLDEN-PLAN §3.3, §3.5, §3.6, §4.4).
//!
//! [`run`] walks MANIFEST.json for one tier (`e2e`, `fn`, `fuzz`; one test binary each), and for
//! every file, on the bounded pool ([`crate::par`], `$POB_GOLDEN_JOBS` workers, default 3):
//!
//! 1. checks the file against MANIFEST (BLAKE3, set, key, line count) and the build XML its
//!    header names against the header's BLAKE3: no silent drift;
//! 2. applies its overlay ([`crate::overlay`]); a stale overlay line fails;
//! 3. asks the set's [`Adapter`] for Rust's answers, and for the lines the recorder elided;
//! 4. compares every line ([`crate::compare`]); a key Rust produces and the file lacks fails;
//! 5. checks the file's pins: `compared` and `bitExact` may not drop, `known` may not grow,
//!    `requiredKeys` must be compared.
//!
//! A missing file, a set without adapter or an adapter error fails: nothing is skipped. Results
//! are reported in MANIFEST order, failures sorted by key, so the report is the same at any job
//! count.
//!
//! Environment:
//! * `POB_GOLDEN_JOBS`: workers (default 3).
//! * `POB_GOLDEN_FILTER=<set glob>[:<key glob>]`: run only the files of matching sets; a bless
//!   writes only matching keys.
//! * `POB_GOLDEN_DIFFS=<file>`: every mismatch as TSV (as `CA_L6_DIFFS` today).
//! * `POB_GOLDEN_SELFTEST=1`: the negative control (§4.4): one expected value per set is changed
//!   and each set must report exactly one failure, on that key.
//! * `POB_GOLDEN_BLESS=1` / `rebase` with `POB_GOLDEN_REASON=…` and `POB_GOLDEN_BLESS_MAX`
//!   (default 200): accept Rust's values for the failing lines (§3.6).

use std::collections::{BTreeMap, BTreeSet};
use std::fmt::Write as _;
use std::path::{Path, PathBuf};

use serde_json::Value as J;

use crate::adapter::{Adapter, Answers};
use crate::compare::{compare_line, mutate, Kind, LineResult, Mismatch, MutateKind, Status};
use crate::encode::{absent, canonical, marker};
use crate::format::{GoldenFile, Line};
use crate::manifest::{FileEntry, Manifest, OverlayEntry};
use crate::overlay::{overlay_path, Filter, Overlay, OverlayLine};
use crate::{Error, Result};

/// Default worker count (the machine's memory rule: bounded, 3).
pub const DEFAULT_JOBS: usize = 3;

/// Default most values one bless may write (§3.6).
pub const DEFAULT_BLESS_MAX: usize = 200;

/// Failures printed per file.
pub const PRINT_FAILURES: usize = 20;

/// How a bless writes.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum BlessMode {
    /// `POB_GOLDEN_BLESS=1`: overlay lines; the PoB recording stays untouched.
    Overlay,
    /// `POB_GOLDEN_BLESS=rebase`: rewrite the base files; MANIFEST records `origin: rust@<sha>`.
    Rebase,
}

/// A run's settings.
#[derive(Debug, Clone)]
pub struct Config {
    /// The goldens directory.
    pub root: PathBuf,
    /// `e2e`, `fn` or `fuzz`.
    pub tier: String,
    pub jobs: usize,
    pub filter: Option<Filter>,
    pub diffs: Option<PathBuf>,
    pub selftest: bool,
    pub bless: Option<BlessMode>,
    pub reason: Option<String>,
    pub bless_max: usize,
}

fn env(name: &str) -> Option<String> {
    std::env::var(name).ok().filter(|v| !v.trim().is_empty())
}

impl Config {
    /// The settings of `tier` from the environment (module docs).
    #[must_use]
    pub fn from_env(tier: &str) -> Self {
        let bless = match env("POB_GOLDEN_BLESS").as_deref() {
            None | Some("0") => None,
            Some("rebase") => Some(BlessMode::Rebase),
            Some(_) => Some(BlessMode::Overlay),
        };
        Self {
            root: crate::golden_root(),
            tier: tier.to_owned(),
            jobs: env("POB_GOLDEN_JOBS")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(DEFAULT_JOBS)
                .max(1),
            filter: env("POB_GOLDEN_FILTER").map(|s| Filter::parse(&s)),
            diffs: env("POB_GOLDEN_DIFFS").map(PathBuf::from),
            selftest: env("POB_GOLDEN_SELFTEST").is_some_and(|v| v != "0"),
            bless,
            reason: env("POB_GOLDEN_REASON"),
            bless_max: env("POB_GOLDEN_BLESS_MAX")
                .and_then(|s| s.trim().parse().ok())
                .unwrap_or(DEFAULT_BLESS_MAX),
        }
    }

    /// A plain run of `tier` over `root` (tests).
    #[must_use]
    pub fn new(root: &Path, tier: &str) -> Self {
        Self {
            root: root.to_path_buf(),
            tier: tier.to_owned(),
            jobs: DEFAULT_JOBS,
            filter: None,
            diffs: None,
            selftest: false,
            bless: None,
            reason: None,
            bless_max: DEFAULT_BLESS_MAX,
        }
    }
}

/// A failed line.
#[derive(Debug, Clone, PartialEq)]
pub struct Failure {
    pub key: String,
    /// PoB's value as compared (after the overlay); `None` for a Rust-only key.
    pub expected: Option<J>,
    /// The base file's value (before the overlay); `None` when the base file lacks the key.
    pub base: Option<J>,
    /// Rust's answer.
    pub actual: Option<crate::compare::Answer>,
    pub mismatches: Vec<Mismatch>,
}

/// One file's outcome.
#[derive(Debug, Clone, Default)]
pub struct FileReport {
    pub path: String,
    pub set: String,
    /// Lines compared (exact, `$anyOf`, `$host` and failed; regenerated lines included).
    pub compared: usize,
    pub exact: usize,
    pub any_of: usize,
    pub host: usize,
    /// `$known` values per id.
    pub known: BTreeMap<String, usize>,
    pub regenerated: usize,
    pub bit_exact: usize,
    /// Mod lists that matched in another order (reported, not failed).
    pub order_notes: usize,
    pub failures: Vec<Failure>,
    /// File-level problems: drift, no adapter, an adapter error, a broken pin.
    pub problems: Vec<String>,
    /// The negative control's change, when this file carries it: (key, kind).
    pub mutated: Option<(String, MutateKind)>,
}

impl FileReport {
    #[must_use]
    pub fn ok(&self) -> bool {
        self.failures.is_empty() && self.problems.is_empty()
    }

    fn tally(&mut self, r: &LineResult) {
        match r.status {
            Status::Known => {
                let id = r.known.clone().unwrap_or_default();
                *self.known.entry(id).or_insert(0) += 1;
                return;
            }
            Status::Exact => self.exact += 1,
            Status::AnyOf => self.any_of += 1,
            Status::Host => self.host += 1,
            Status::Failed => {}
        }
        self.compared += 1;
        if !r.failed() && r.bit_exact {
            self.bit_exact += 1;
        }
        if !r.order_notes.is_empty() {
            self.order_notes += 1;
        }
    }
}

/// Totals of one set.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct SetStats {
    pub files: usize,
    pub compared: usize,
    pub exact: usize,
    pub any_of: usize,
    pub host: usize,
    pub known: usize,
    pub regenerated: usize,
    pub bit_exact: usize,
    pub order_notes: usize,
    pub failed: usize,
    /// The sums of the files' pins.
    pub compared_pin: usize,
    pub bit_exact_pin: usize,
}

/// The negative control's outcome for one set.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SelftestResult {
    pub set: String,
    /// (file, key, kind) of the change, or `None` when the set has no candidate value.
    pub target: Option<(String, String, MutateKind)>,
    pub ok: bool,
    pub message: String,
}

/// A whole run.
#[derive(Debug, Clone, Default)]
pub struct Report {
    pub tier: String,
    pub files: Vec<FileReport>,
    /// Run-level problems (MANIFEST unreadable, unlisted files, …).
    pub problems: Vec<String>,
    pub selftest: Vec<SelftestResult>,
    /// Files left out by `POB_GOLDEN_FILTER`.
    pub filtered_out: usize,
}

impl Report {
    #[must_use]
    pub fn sets(&self) -> BTreeMap<String, SetStats> {
        let mut out: BTreeMap<String, SetStats> = BTreeMap::new();
        for f in &self.files {
            let s = out.entry(f.set.clone()).or_default();
            s.files += 1;
            s.compared += f.compared;
            s.exact += f.exact;
            s.any_of += f.any_of;
            s.host += f.host;
            s.known += f.known.values().sum::<usize>();
            s.regenerated += f.regenerated;
            s.bit_exact += f.bit_exact;
            s.order_notes += f.order_notes;
            s.failed += f.failures.len();
        }
        out
    }

    /// No failure and no problem (in a self-test run: every set failed exactly as planned).
    #[must_use]
    pub fn ok(&self) -> bool {
        if !self.problems.is_empty() {
            return false;
        }
        if !self.selftest.is_empty() {
            return self.selftest.iter().all(|s| s.ok);
        }
        self.files.iter().all(FileReport::ok)
    }

    /// The human report: one `golden <set>: …` line per set, then the first failures and the
    /// problems of each file.
    #[must_use]
    pub fn render(&self, pins: &BTreeMap<String, (usize, usize)>) -> String {
        let mut s = String::new();
        let sets = self.sets();
        let _ = writeln!(
            s,
            "golden {}: {} files in {} sets{}",
            self.tier,
            self.files.len(),
            sets.len(),
            if self.filtered_out > 0 {
                format!(
                    " ({} files left out by POB_GOLDEN_FILTER)",
                    self.filtered_out
                )
            } else {
                String::new()
            }
        );
        for (name, st) in &sets {
            let (cp, bp) = pins.get(name).copied().unwrap_or_default();
            let _ = writeln!(
                s,
                "golden {name}: compared {} (pin {cp}), exact {}, anyOf {}, host {}, known {}, \
                 regenerated {}, bitExact {} (pin {bp}), order-only {}, failed {}, files {}",
                st.compared,
                st.exact,
                st.any_of,
                st.host,
                st.known,
                st.regenerated,
                st.bit_exact,
                st.order_notes,
                st.failed,
                st.files
            );
        }
        for f in &self.files {
            for p in &f.problems {
                let _ = writeln!(s, "  FAIL {}: {p}", f.path);
            }
            for x in f.failures.iter().take(PRINT_FAILURES) {
                let m = x.mismatches.first();
                let _ = writeln!(
                    s,
                    "  FAIL {} {}: {} at {:?}: PoB {} / Rust {}",
                    f.path,
                    x.key,
                    m.map_or("value", |m| m.kind.name()),
                    m.map_or("", |m| m.path.as_str()),
                    m.and_then(|m| m.expected.as_ref())
                        .map_or_else(|| "-".to_owned(), short),
                    m.and_then(|m| m.actual.as_ref())
                        .map_or_else(|| "-".to_owned(), short),
                );
            }
            if f.failures.len() > PRINT_FAILURES {
                let _ = writeln!(
                    s,
                    "  … {} more failures in {}",
                    f.failures.len() - PRINT_FAILURES,
                    f.path
                );
            }
        }
        for p in &self.problems {
            let _ = writeln!(s, "  FAIL: {p}");
        }
        for t in &self.selftest {
            let _ = writeln!(
                s,
                "selftest {}: {} ({})",
                t.set,
                if t.ok { "ok" } else { "FAIL" },
                t.message
            );
        }
        s
    }

    /// Every mismatch as TSV: `set file key path kind pob rust`.
    #[must_use]
    pub fn diffs_tsv(&self) -> String {
        let mut s = String::from("set\tfile\tkey\tpath\tkind\tpob\trust\n");
        let cell = |v: Option<&J>| {
            v.map_or_else(|| "-".to_owned(), canonical)
                .replace('\t', " ")
        };
        for f in &self.files {
            for x in &f.failures {
                for m in &x.mismatches {
                    let _ = writeln!(
                        s,
                        "{}\t{}\t{}\t{}\t{}\t{}\t{}",
                        f.set,
                        f.path,
                        x.key,
                        m.path,
                        m.kind.name(),
                        cell(m.expected.as_ref()),
                        cell(m.actual.as_ref())
                    );
                }
            }
        }
        s
    }
}

fn short(v: &J) -> String {
    let t = canonical(v);
    if t.chars().count() > 120 {
        let cut: String = t.chars().take(117).collect();
        format!("{cut}...")
    } else {
        t
    }
}

/// The adapter registry the runner uses ([`crate::adapter::lookup`] in the test binaries).
pub type Lookup<'a> = &'a (dyn Fn(&str) -> Option<&'static dyn Adapter> + Sync);

/// A golden file as the runner compares it: the base file, the effective file (the overlay
/// applied) and the stale overlay keys.
struct Loaded {
    base: GoldenFile,
    effective: GoldenFile,
    stale: Vec<String>,
}

/// Read and check one listed file and its overlay (steps 1-2).
fn load(root: &Path, e: &FileEntry) -> Result<Loaded> {
    let bytes = crate::read_bytes(&root.join(&e.path))?;
    let h = crate::blake3_hex(&bytes);
    if h != e.blake3 {
        return Err(Error(format!(
            "BLAKE3 {h}, MANIFEST says {} (the file changed and MANIFEST did not)",
            e.blake3
        )));
    }
    let base = GoldenFile::parse(&crate::format::text_of(Path::new(&e.path), &bytes)?)?;
    if base.header.set != e.set || base.header.key != e.key {
        return Err(Error(format!(
            "header says set {:?} key {:?}, MANIFEST says {:?} {:?}",
            base.header.set, base.header.key, e.set, e.key
        )));
    }
    if base.lines.len() != e.lines {
        return Err(Error(format!(
            "{} lines, MANIFEST says {}",
            base.lines.len(),
            e.lines
        )));
    }
    if let Some((xml, Some(want))) = base.header.input_xml() {
        let have = crate::blake3_hex(&crate::read_bytes(&root.join(xml))?);
        if have != want {
            return Err(Error(format!(
                "input {xml}: BLAKE3 {have}, the header says {want} (the XML changed and the golden did not)"
            )));
        }
    }
    let unlisted = overlay_path(&e.path);
    let (effective, stale) = match &e.overlay {
        Some(o) => {
            let ob = crate::read_bytes(&root.join(&o.path))?;
            if crate::blake3_hex(&ob) != o.blake3 {
                return Err(Error(format!(
                    "overlay {}: BLAKE3 differs from MANIFEST",
                    o.path
                )));
            }
            let text = String::from_utf8(ob).map_err(|e| Error(e.to_string()))?;
            Overlay::parse(&text)?.apply(&base)
        }
        None if root.join(&unlisted).exists() => {
            return Err(Error(format!(
                "overlay {unlisted} exists but MANIFEST does not list it"
            )));
        }
        None => (base.clone(), Vec::new()),
    };
    Ok(Loaded {
        base,
        effective,
        stale,
    })
}

/// Run one file (steps 3-5).
fn run_file(
    root: &Path,
    e: &FileEntry,
    lookup: Lookup<'_>,
    mutation: Option<&(String, String, MutateKind)>,
) -> FileReport {
    let mut rep = FileReport {
        path: e.path.clone(),
        set: e.set.clone(),
        ..FileReport::default()
    };
    let loaded = match load(root, e) {
        Ok(l) => l,
        Err(err) => {
            rep.problems.push(err.0);
            return rep;
        }
    };
    let Some(adapter) = lookup(&e.set) else {
        rep.problems.push(format!("no adapter for set {}", e.set));
        return rep;
    };
    let ctx = crate::adapter::ctx(root);
    let mut effective = loaded.effective;
    // the negative control changes the expected value before the adapter sees the file, so an
    // adapter that echoes the golden passes the change through and the control catches it
    if let Some((_, key, kind)) = mutation.filter(|m| m.0 == e.path) {
        if let Ok(i) = effective
            .lines
            .binary_search_by(|l| l.key.as_str().cmp(key))
        {
            if let Some(v) = mutate(&effective.lines[i].value, *kind) {
                effective.lines[i].value = v;
                rep.mutated = Some((key.clone(), *kind));
            }
        }
    }
    let answers: Answers = match adapter.answer(&effective, &ctx) {
        Ok(a) => a,
        Err(err) => {
            rep.problems.push(format!("adapter: {err}"));
            return rep;
        }
    };
    let elided = match adapter.elided(&effective, &ctx) {
        Ok(v) => v,
        Err(err) => {
            rep.problems.push(format!("adapter (elided lines): {err}"));
            return rep;
        }
    };
    let stale: BTreeSet<&String> = loaded.stale.iter().collect();
    let mut seen: BTreeSet<&str> = BTreeSet::new();
    let regenerated: BTreeSet<&str> = elided.iter().map(|l| l.key.as_str()).collect();
    for line in effective.lines.iter().chain(&elided) {
        if !seen.insert(line.key.as_str()) {
            rep.problems
                .push(format!("key {:?} both stored and elided", line.key));
            continue;
        }
        let base = loaded.base.get(&line.key).map(|l| l.value.clone());
        if stale.contains(&line.key) {
            rep.failures.push(Failure {
                key: line.key.clone(),
                expected: Some(line.value.clone()),
                base,
                actual: answers.get(&line.key).cloned(),
                mismatches: vec![Mismatch {
                    path: String::new(),
                    kind: Kind::StaleOverlay,
                    expected: Some(line.value.clone()),
                    actual: None,
                }],
            });
            rep.compared += 1;
            continue;
        }
        let actual = answers.get(&line.key);
        let r = compare_line(&line.value, actual);
        rep.tally(&r);
        if regenerated.contains(line.key.as_str()) {
            rep.regenerated += 1;
        }
        if r.failed() {
            rep.failures.push(Failure {
                key: line.key.clone(),
                expected: Some(line.value.clone()),
                base,
                actual: actual.cloned(),
                mismatches: r.mismatches,
            });
        }
    }
    // stale overlay lines whose key is in neither the base nor the effective file
    for k in &loaded.stale {
        if !seen.contains(k.as_str()) {
            rep.failures.push(Failure {
                key: k.clone(),
                expected: None,
                base: None,
                actual: answers.get(k).cloned(),
                mismatches: vec![Mismatch {
                    path: String::new(),
                    kind: Kind::StaleOverlay,
                    expected: None,
                    actual: None,
                }],
            });
        }
    }
    // Rust-only keys
    for (k, a) in &answers {
        if !seen.contains(k.as_str()) {
            let shown = a.clone().unwrap_or_else(|m| marker("$error", J::String(m)));
            rep.failures.push(Failure {
                key: k.clone(),
                expected: None,
                base: None,
                actual: Some(a.clone()),
                mismatches: vec![Mismatch {
                    path: String::new(),
                    kind: Kind::Extra,
                    expected: None,
                    actual: Some(shown),
                }],
            });
        }
    }
    rep.failures.sort_by(|a, b| a.key.cmp(&b.key));
    check_pins(e, &effective, &mut rep);
    rep
}

/// Step 5: the file's pins.
fn check_pins(e: &FileEntry, file: &GoldenFile, rep: &mut FileReport) {
    if let Some(pin) = e.compared {
        if rep.compared < pin {
            rep.problems.push(format!(
                "compared {} values, below the pin {pin}",
                rep.compared
            ));
        }
    }
    if let Some(pin) = e.bit_exact {
        if rep.bit_exact < pin {
            rep.problems.push(format!(
                "{} bit-exact lines, below the pin {pin} (a lowered pin needs a bless with a reason)",
                rep.bit_exact
            ));
        }
    }
    for (id, n) in &rep.known {
        let bound = e.known.get(id).copied().unwrap_or(0);
        if *n > bound {
            rep.problems
                .push(format!("{n} `$known` {id} values, above the bound {bound}"));
        }
    }
    for k in &e.required_keys {
        let compared = file
            .get(k)
            .is_some_and(|l| l.value != absent() && l.value.get("$known").is_none());
        if !compared {
            rep.problems
                .push(format!("required key {k:?} is not compared"));
        }
    }
}

/// Files under `<root>/<tier>/` and `<root>/overlay/<tier>/` that MANIFEST does not list.
fn unlisted_files(root: &Path, manifest: &Manifest, tier: &str) -> Vec<String> {
    fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
        let Ok(rd) = std::fs::read_dir(dir) else {
            return;
        };
        for e in rd.flatten() {
            let p = e.path();
            if p.is_dir() {
                walk(&p, out);
            } else {
                out.push(p);
            }
        }
    }
    let listed: BTreeSet<String> = manifest
        .files
        .iter()
        .flat_map(|f| {
            std::iter::once(f.path.clone()).chain(f.overlay.as_ref().map(|o| o.path.clone()))
        })
        .collect();
    let mut found = Vec::new();
    walk(&root.join(tier), &mut found);
    walk(&root.join("overlay").join(tier), &mut found);
    let mut out: Vec<String> = found
        .iter()
        .filter_map(|p| p.strip_prefix(root).ok())
        .map(|p| p.to_string_lossy().replace('\\', "/"))
        .filter(|p| {
            let name = p.rsplit('/').next().unwrap_or(p);
            !name.starts_with('.') && !listed.contains(p)
        })
        .collect();
    out.sort();
    out
}

/// The negative control's target per set (§4.4): in MANIFEST order and then key order, the
/// first compared finite non-zero number; failing that the first compared string, then the
/// first compared boolean.
fn selftest_targets(
    root: &Path,
    files: &[&FileEntry],
) -> BTreeMap<String, Option<(String, String, MutateKind)>> {
    let mut by_set: BTreeMap<String, Vec<&FileEntry>> = BTreeMap::new();
    for f in files {
        by_set.entry(f.set.clone()).or_default().push(f);
    }
    let mut out = BTreeMap::new();
    for (set, fs) in by_set {
        let loaded: Vec<(&FileEntry, GoldenFile)> = fs
            .iter()
            .filter_map(|f| load(root, f).ok().map(|l| (*f, l.effective)))
            .collect();
        let mut target = None;
        'kinds: for kind in [MutateKind::Number, MutateKind::String, MutateKind::Bool] {
            for (f, gf) in &loaded {
                for l in &gf.lines {
                    if mutate(&l.value, kind).is_some() {
                        target = Some((f.path.clone(), l.key.clone(), kind));
                        break 'kinds;
                    }
                }
            }
        }
        out.insert(set, target);
    }
    out
}

/// Run one tier (module docs).
#[must_use]
pub fn run(cfg: &Config, lookup: Lookup<'_>) -> Report {
    let mut report = Report {
        tier: cfg.tier.clone(),
        ..Report::default()
    };
    let manifest = match Manifest::load(&cfg.root) {
        Ok(m) => m,
        Err(e) => {
            report.problems.push(e.0);
            return report;
        }
    };
    report.problems.extend(
        unlisted_files(&cfg.root, &manifest, &cfg.tier)
            .into_iter()
            .map(|p| format!("{p}: not in MANIFEST (a file MANIFEST does not list is never run)")),
    );
    let all = manifest.tier_files(&cfg.tier);
    let files: Vec<&FileEntry> = all
        .iter()
        .copied()
        .filter(|f| cfg.filter.as_ref().map_or(true, |x| x.matches_set(&f.set)))
        .collect();
    report.filtered_out = all.len() - files.len();
    let targets = if cfg.selftest {
        selftest_targets(&cfg.root, &files)
    } else {
        BTreeMap::new()
    };
    let mutations: BTreeMap<&str, &(String, String, MutateKind)> = targets
        .values()
        .flatten()
        .map(|t| (t.0.as_str(), t))
        .collect();
    let mut out = Vec::with_capacity(files.len());
    crate::par::par_for_each_init_with(
        cfg.jobs,
        &files,
        || (),
        |(), _, e| {
            run_file(
                &cfg.root,
                e,
                lookup,
                mutations.get(e.path.as_str()).copied(),
            )
        },
        |_, _, r| out.push(r),
    );
    report.files = out;
    if cfg.selftest {
        report.selftest = judge_selftest(&report, &targets);
    }
    report
}

/// Each set must have failed exactly once, on the changed key, with no file-level problem.
fn judge_selftest(
    report: &Report,
    targets: &BTreeMap<String, Option<(String, String, MutateKind)>>,
) -> Vec<SelftestResult> {
    let mut out = Vec::new();
    for (set, target) in targets {
        let files: Vec<&FileReport> = report.files.iter().filter(|f| &f.set == set).collect();
        let problems: Vec<&String> = files.iter().flat_map(|f| &f.problems).collect();
        let failures: Vec<(&str, &str)> = files
            .iter()
            .flat_map(|f| {
                f.failures
                    .iter()
                    .map(move |x| (f.path.as_str(), x.key.as_str()))
            })
            .collect();
        let (ok, message) = match target {
            None => (
                false,
                "no compared number, string or boolean to change: the set would pass vacuously"
                    .to_owned(),
            ),
            Some((file, key, kind)) => {
                let want = [(file.as_str(), key.as_str())];
                if !problems.is_empty() {
                    (false, format!("file problems: {problems:?}"))
                } else if failures == want {
                    (
                        true,
                        format!("changed {file} {key:?} ({kind:?}); 1 failure, on that key"),
                    )
                } else {
                    (
                        false,
                        format!(
                            "changed {file} {key:?} ({kind:?}); {} failures: {:?}",
                            failures.len(),
                            failures.iter().take(5).collect::<Vec<_>>()
                        ),
                    )
                }
            }
        };
        out.push(SelftestResult {
            set: set.clone(),
            target: target.clone(),
            ok,
            message,
        });
    }
    out
}

/// The pins per set (compared, bitExact) from MANIFEST, for the report.
#[must_use]
pub fn set_pins(manifest: &Manifest) -> BTreeMap<String, (usize, usize)> {
    let mut out: BTreeMap<String, (usize, usize)> = BTreeMap::new();
    for f in &manifest.files {
        let e = out.entry(f.set.clone()).or_default();
        e.0 += f.compared.unwrap_or(0);
        e.1 += f.bit_exact.unwrap_or(0);
    }
    out
}

/// The current commit (`git rev-parse --short=12 HEAD` in `dir`), or `unknown`.
#[must_use]
pub fn head_commit(dir: &Path) -> String {
    std::process::Command::new("git")
        .args(["rev-parse", "--short=12", "HEAD"])
        .current_dir(dir)
        .output()
        .ok()
        .filter(|o| o.status.success())
        .map_or_else(
            || "unknown".to_owned(),
            |o| String::from_utf8_lossy(&o.stdout).trim().to_owned(),
        )
}

/// What a bless wrote.
#[derive(Debug, Clone, Default, PartialEq, Eq)]
pub struct BlessOutcome {
    /// Values written, per file.
    pub written: BTreeMap<String, usize>,
    /// `bitExact` pins lowered: file -> (old, new).
    pub lowered: BTreeMap<String, (usize, usize)>,
}

/// The value a bless records as Rust's new expectation of a failed line.
fn blessed_value(f: &Failure) -> J {
    let v = match &f.actual {
        None => return absent(),
        Some(Err(m)) => return marker("$error", J::String(m.clone())),
        Some(Ok(v)) => v.clone(),
    };
    // a `$perOrder` line: Rust's answer is the list of results, the golden keeps the marker
    match &f.expected {
        Some(J::Object(m)) if m.len() == 1 && m.contains_key("$perOrder") => marker("$perOrder", v),
        _ => v,
    }
}

/// Accept Rust's values for the failing lines of `report` (§3.6). Refuses without a reason, and
/// when the filter matches more than `bless_max` values. Updates the overlays (or, for a
/// rebase, the base files) and MANIFEST.
pub fn bless(cfg: &Config, report: &Report) -> Result<BlessOutcome> {
    let mode = cfg
        .bless
        .ok_or_else(|| Error("bless: POB_GOLDEN_BLESS is not set".into()))?;
    let reason = cfg.reason.clone().ok_or_else(|| {
        Error(
            "bless: POB_GOLDEN_REASON is required (the reason goes into every overlay line)".into(),
        )
    })?;
    if !report.problems.is_empty() {
        return Err(Error(format!(
            "bless: the run has problems: {:?}",
            report.problems
        )));
    }
    let mut picked: Vec<(&FileReport, Vec<&Failure>)> = Vec::new();
    let mut total = 0;
    for f in &report.files {
        if !f.problems.is_empty() && f.problems.iter().any(|p| !p.contains("below the pin")) {
            return Err(Error(format!(
                "bless: {} has problems: {:?}",
                f.path, f.problems
            )));
        }
        let fs: Vec<&Failure> = f
            .failures
            .iter()
            .filter(|x| {
                cfg.filter
                    .as_ref()
                    .map_or(true, |flt| flt.matches(&f.set, &x.key))
            })
            .collect();
        total += fs.len();
        if !fs.is_empty() {
            picked.push((f, fs));
        }
    }
    if total > cfg.bless_max {
        return Err(Error(format!(
            "bless: {total} values match, over POB_GOLDEN_BLESS_MAX={} (split the bless by key, or raise the limit on purpose)",
            cfg.bless_max
        )));
    }
    let commit = head_commit(&cfg.root);
    let mut manifest = Manifest::load(&cfg.root)?;
    let mut outcome = BlessOutcome::default();
    for (f, fails) in picked {
        let Some(entry) = manifest.file(&f.path).cloned() else {
            return Err(Error(format!("bless: {} is not in MANIFEST", f.path)));
        };
        match mode {
            BlessMode::Overlay => {
                let rel = entry
                    .overlay
                    .as_ref()
                    .map_or_else(|| overlay_path(&entry.path), |o| o.path.clone());
                let path = cfg.root.join(&rel);
                let mut ov = Overlay::read(&path)?;
                for x in &fails {
                    let pob = x.base.clone().unwrap_or_else(absent);
                    let new = blessed_value(x);
                    if canonical(&pob) == canonical(&new) {
                        // the base value already holds: drop the overlay line
                        ov.lines.remove(&x.key);
                        continue;
                    }
                    ov.lines.insert(
                        x.key.clone(),
                        OverlayLine {
                            key: x.key.clone(),
                            pob,
                            new,
                            reason: reason.clone(),
                            commit: commit.clone(),
                        },
                    );
                }
                let e = manifest.file_mut(&f.path).expect("listed");
                if ov.lines.is_empty() {
                    if path.exists() {
                        std::fs::remove_file(&path)
                            .map_err(|x| Error(format!("{}: {x}", path.display())))?;
                    }
                    e.overlay = None;
                } else {
                    ov.write(&path)?;
                    e.overlay = Some(OverlayEntry {
                        path: rel,
                        blake3: String::new(),
                        lines: 0,
                    });
                }
            }
            BlessMode::Rebase => {
                let full = cfg.root.join(&entry.path);
                let mut base = GoldenFile::read(&full)?;
                // the overlay's values first: they are what the run compared with
                if let Some(o) = &entry.overlay {
                    let ov = Overlay::read(&cfg.root.join(&o.path))?;
                    base = ov.apply(&base).0;
                    let _ = std::fs::remove_file(cfg.root.join(&o.path));
                }
                let query = base.lines.first().is_some_and(|l| l.input.is_some());
                for x in &fails {
                    let new = blessed_value(x);
                    match base.lines.binary_search_by(|l| l.key.as_str().cmp(&x.key)) {
                        Ok(i) if new == absent() => {
                            base.lines.remove(i);
                        }
                        Ok(i) => base.lines[i].value = new,
                        Err(_) if new == absent() => {}
                        Err(i) => base.lines.insert(
                            i,
                            Line {
                                key: x.key.clone(),
                                input: query.then_some(J::Null),
                                value: new,
                            },
                        ),
                    }
                }
                base.write(&full)?;
                let e = manifest.file_mut(&f.path).expect("listed");
                e.overlay = None;
                e.origin = format!("rust@{commit}");
            }
        }
        manifest.refresh_file(&cfg.root, &f.path)?;
        let e = manifest.file_mut(&f.path).expect("listed");
        if let Some(pin) = e.bit_exact {
            if f.bit_exact < pin {
                e.bit_exact = Some(f.bit_exact);
                outcome.lowered.insert(f.path.clone(), (pin, f.bit_exact));
            }
        }
        outcome.written.insert(f.path.clone(), fails.len());
    }
    manifest.save(&cfg.root)?;
    Ok(outcome)
}

/// The test binaries' entry point: run `tier` with the environment's settings and the real
/// adapters, print the report, write the diffs, bless when asked, and panic on any failure.
pub fn test_main(tier: &str) {
    let cfg = Config::from_env(tier);
    let report = run(&cfg, &crate::adapter::lookup);
    let pins = Manifest::load(&cfg.root)
        .map(|m| set_pins(&m))
        .unwrap_or_default();
    eprint!("{}", report.render(&pins));
    if let Some(p) = &cfg.diffs {
        if let Err(e) = crate::write_atomic(p, report.diffs_tsv().as_bytes()) {
            eprintln!("POB_GOLDEN_DIFFS: {e}");
        }
    }
    if cfg.bless.is_some() && !cfg.selftest {
        match bless(&cfg, &report) {
            Ok(o) => {
                for (f, n) in &o.written {
                    eprintln!("bless: {n} values in {f}");
                }
                for (f, (a, b)) in &o.lowered {
                    eprintln!("bless: bitExact pin of {f} lowered {a} -> {b}");
                }
                eprintln!(
                    "bless: {} values written; run the suite again (and its negative control) before committing",
                    o.written.values().sum::<usize>()
                );
                return;
            }
            Err(e) => panic!("{e}"),
        }
    }
    assert!(
        report.ok(),
        "golden {tier}: {} failed lines, {} file problems, {} run problems{}",
        report.files.iter().map(|f| f.failures.len()).sum::<usize>(),
        report.files.iter().map(|f| f.problems.len()).sum::<usize>(),
        report.problems.len(),
        if cfg.selftest {
            " (negative control)"
        } else {
            ""
        }
    );
}

#[cfg(test)]
mod tests {
    //! The runner on a temporary goldens directory with a test adapter.
    #![allow(clippy::many_single_char_names)]
    use super::*;
    use crate::format::Header;
    use crate::manifest::entry_for;
    use serde_json::json;

    /// Answers each line from a table of Rust values held in the header (`extra.rust`): a
    /// stand-in for a calc adapter. A `null` there means Rust produced nothing; a string
    /// starting with `!` is a Rust error.
    struct TableAdapter;

    impl Adapter for TableAdapter {
        fn answer(
            &self,
            file: &GoldenFile,
            _ctx: &crate::adapter::Ctx,
        ) -> std::result::Result<Answers, String> {
            let t = file
                .header
                .extra
                .get("rust")
                .and_then(J::as_object)
                .ok_or("no rust table")?;
            let mut out = Answers::new();
            for (k, v) in t {
                match v {
                    J::Null => {}
                    J::String(s) if s.starts_with('!') => {
                        out.insert(k.clone(), Err(s[1..].to_owned()));
                    }
                    v => {
                        out.insert(k.clone(), Ok(v.clone()));
                    }
                }
            }
            Ok(out)
        }

        fn elided(
            &self,
            file: &GoldenFile,
            _ctx: &crate::adapter::Ctx,
        ) -> std::result::Result<Vec<Line>, String> {
            Ok(file
                .header
                .extra
                .get("elided")
                .and_then(J::as_array)
                .map(|a| {
                    a.iter()
                        .filter_map(|k| k.as_str())
                        .map(|k| Line::e2e(k, json!(0)))
                        .collect()
                })
                .unwrap_or_default())
        }
    }

    static TABLE: TableAdapter = TableAdapter;

    fn lookup(set: &str) -> Option<&'static dyn Adapter> {
        set.starts_with("e2e.t").then_some(&TABLE as &dyn Adapter)
    }

    struct Dir(PathBuf);

    impl Drop for Dir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn tmp(name: &str) -> Dir {
        let d = std::env::temp_dir().join(format!("pob-golden-run-{name}-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        std::fs::create_dir_all(&d).unwrap();
        Dir(d)
    }

    /// Write a golden of `set` / `key` with PoB's `lines` and Rust's `rust` table; list it.
    fn add(
        root: &Path,
        m: &mut Manifest,
        set: &str,
        key: &str,
        lines: &[(&str, J)],
        rust: J,
    ) -> String {
        let mut h = Header::new(set, key);
        h.extra.insert("rust".into(), rust);
        let gf = GoldenFile::new(
            h,
            lines
                .iter()
                .map(|(k, v)| Line::e2e(*k, v.clone()))
                .collect(),
        );
        let path = format!("e2e/{}/{key}.jsonl", set.trim_start_matches("e2e."));
        let bytes = gf.write(&root.join(&path)).unwrap();
        m.upsert_file(entry_for(&path, &gf, &bytes));
        path
    }

    fn set_rust(root: &Path, m: &mut Manifest, path: &str, rust: J) {
        let mut gf = GoldenFile::read(&root.join(path)).unwrap();
        gf.header.extra.insert("rust".into(), rust);
        gf.write(&root.join(path)).unwrap();
        m.refresh_file(root, path).unwrap();
        m.save(root).unwrap();
    }

    #[test]
    fn pass_fail_pins_and_drift() {
        let d = tmp("basic");
        let root = &d.0;
        let mut m = Manifest::default();
        let a = add(
            root,
            &mut m,
            "e2e.ta",
            "b1",
            &[
                ("x", json!(1.5)),
                ("y", json!({"$anyOf": [1, 2]})),
                ("z", json!({"$known": "CCR-0014", "pob": 3})),
            ],
            json!({"x": 1.5, "y": 2, "z": 9}),
        );
        let b = add(
            root,
            &mut m,
            "e2e.ta",
            "b2",
            &[("x", json!(10)), ("w", json!({"$error": "e"}))],
            json!({"x": 10.000_000_001, "w": "!boom"}),
        );
        m.file_mut(&a).unwrap().compared = Some(2);
        m.file_mut(&a).unwrap().known.insert("CCR-0014".into(), 1);
        m.file_mut(&b).unwrap().bit_exact = Some(1);
        m.save(root).unwrap();
        let cfg = Config::new(root, "e2e");
        let r = run(&cfg, &lookup);
        assert!(r.ok(), "{}", r.render(&BTreeMap::new()));
        let st = &r.sets()["e2e.ta"];
        assert_eq!(
            (st.compared, st.exact, st.any_of, st.known, st.bit_exact),
            (4, 3, 1, 1, 3)
        );

        // a changed Rust value, a Rust-only key and a missing one
        set_rust(root, &mut m, &b, json!({"x": 11, "w": "!boom", "v": 1}));
        set_rust(root, &mut m, &a, json!({"y": 2, "z": 9}));
        let r = run(&cfg, &lookup);
        assert!(!r.ok());
        let fa: Vec<_> = r
            .files
            .iter()
            .flat_map(|f| {
                f.failures
                    .iter()
                    .map(|x| (x.key.clone(), x.mismatches[0].kind))
            })
            .collect();
        assert_eq!(
            fa,
            [
                ("x".to_owned(), Kind::Missing),
                ("v".to_owned(), Kind::Extra),
                ("x".to_owned(), Kind::Value)
            ]
        );
        // the pins: a's compared count fell to 2 (still the pin), b's bit-exact count to 1 (w)
        assert!(
            r.files.iter().all(|f| f.problems.is_empty()),
            "{:?}",
            r.files
        );
        m.file_mut(&a).unwrap().compared = Some(3);
        m.file_mut(&a).unwrap().known.clear();
        m.save(root).unwrap();
        let r = run(&cfg, &lookup);
        let p = &r.files.iter().find(|f| f.path == a).unwrap().problems;
        assert!(p.iter().any(|x| x.contains("below the pin 3")), "{p:?}");
        assert!(p.iter().any(|x| x.contains("above the bound 0")), "{p:?}");

        // drift: the file changes, MANIFEST does not
        let mut gf = GoldenFile::read(&root.join(&a)).unwrap();
        gf.lines[0].value = json!(1.25);
        gf.write(&root.join(&a)).unwrap();
        let r = run(&cfg, &lookup);
        assert!(
            r.files[0].problems[0].contains("BLAKE3"),
            "{:?}",
            r.files[0].problems
        );

        // unlisted files and sets without an adapter fail
        std::fs::write(root.join("e2e/ta/stray.jsonl"), "x").unwrap();
        add(root, &mut m, "e2e.zz", "c", &[("k", json!(1))], json!({}));
        m.save(root).unwrap();
        let r = run(&cfg, &lookup);
        assert!(r.problems.iter().any(|p| p.contains("stray.jsonl")));
        assert!(r.files.iter().any(|f| f
            .problems
            .iter()
            .any(|p| p.contains("no adapter for set e2e.zz"))));
        // a filter leaves sets out
        let mut c2 = cfg.clone();
        c2.filter = Some(Filter::parse("e2e.zz"));
        assert_eq!(run(&c2, &lookup).filtered_out, 2);
        // another tier sees nothing
        let r = run(&Config::new(root, "fn"), &lookup);
        assert!(r.files.is_empty() && r.problems.is_empty());
    }

    #[test]
    fn missing_manifest_fails() {
        let d = tmp("nomanifest");
        let r = run(&Config::new(&d.0, "e2e"), &lookup);
        assert!(!r.ok());
        assert!(r.problems[0].contains("MANIFEST.json"));
    }

    #[test]
    fn input_xml_hash_and_elided_lines() {
        let d = tmp("xml");
        let root = &d.0;
        std::fs::create_dir_all(root.join("builds/all57")).unwrap();
        std::fs::write(root.join("builds/all57/b.xml"), "<PathOfBuilding/>").unwrap();
        let mut h = Header::new("e2e.ta", "all57/b");
        h.input = Some(
            json!({"xml": "builds/all57/b.xml", "blake3": crate::blake3_hex(b"<PathOfBuilding/>"), "rounds": 1}),
        );
        h.extra
            .insert("rust".into(), json!({"a": 1, "e1": 0, "e2": 0}));
        h.extra.insert("elided".into(), json!(["e1", "e2"]));
        let gf = GoldenFile::new(h, vec![Line::e2e("a", json!(1))]);
        let path = "e2e/ta/b.jsonl";
        let bytes = gf.write(&root.join(path)).unwrap();
        let mut m = Manifest::default();
        m.upsert_file(entry_for(path, &gf, &bytes));
        m.save(root).unwrap();
        let cfg = Config::new(root, "e2e");
        let r = run(&cfg, &lookup);
        assert!(r.ok(), "{}", r.render(&BTreeMap::new()));
        assert_eq!((r.files[0].compared, r.files[0].regenerated), (3, 2));
        std::fs::write(root.join("builds/all57/b.xml"), "<PathOfBuilding />").unwrap();
        let r = run(&cfg, &lookup);
        assert!(
            r.files[0].problems[0].contains("the XML changed"),
            "{:?}",
            r.files[0].problems
        );
    }

    #[test]
    fn negative_control() {
        let d = tmp("selftest");
        let root = &d.0;
        let mut m = Manifest::default();
        // set ta: the first number in key order is b1's "b" (a is a zero, k is $known)
        add(
            root,
            &mut m,
            "e2e.ta",
            "b1",
            &[
                ("a", json!(0)),
                ("b", json!([3, 4])),
                ("k", json!({"$known": "X", "pob": 1})),
            ],
            json!({"a": 0, "b": [3, 4], "k": 5}),
        );
        m.file_mut("e2e/ta/b1.jsonl")
            .unwrap()
            .known
            .insert("X".into(), 1);
        add(
            root,
            &mut m,
            "e2e.ta",
            "b2",
            &[("a", json!(7))],
            json!({"a": 7}),
        );
        // set tb has no number: its first string changes
        add(
            root,
            &mut m,
            "e2e.tb",
            "s",
            &[("n", json!(true)), ("s", json!("Onslaught"))],
            json!({"n": true, "s": "Onslaught"}),
        );
        // set tc has nothing to change
        add(
            root,
            &mut m,
            "e2e.tc",
            "z",
            &[("z", json!(0))],
            json!({"z": 0}),
        );
        m.save(root).unwrap();
        let mut cfg = Config::new(root, "e2e");
        cfg.selftest = true;
        let r = run(&cfg, &lookup);
        let st: BTreeMap<_, _> = r.selftest.iter().map(|s| (s.set.as_str(), s)).collect();
        assert!(st["e2e.ta"].ok, "{}", st["e2e.ta"].message);
        assert_eq!(
            st["e2e.ta"].target,
            Some(("e2e/ta/b1.jsonl".into(), "b".into(), MutateKind::Number))
        );
        assert!(st["e2e.tb"].ok, "{}", st["e2e.tb"].message);
        assert_eq!(st["e2e.tb"].target.as_ref().unwrap().2, MutateKind::String);
        assert!(
            !st["e2e.tc"].ok,
            "a set with nothing to change fails the control"
        );
        assert!(!r.ok());
        // a set that passes vacuously (its adapter echoes whatever it is asked) fails the control
        struct Echo;
        impl Adapter for Echo {
            fn answer(
                &self,
                f: &GoldenFile,
                _: &crate::adapter::Ctx,
            ) -> std::result::Result<Answers, String> {
                Ok(f.lines
                    .iter()
                    .map(|l| (l.key.clone(), Ok(l.value.clone())))
                    .collect())
            }
        }
        static ECHO: Echo = Echo;
        let echo = |s: &str| (s == "e2e.ta").then_some(&ECHO as &dyn Adapter);
        cfg.filter = Some(Filter::parse("e2e.ta"));
        let r = run(&cfg, &echo);
        assert!(!r.selftest[0].ok);
        assert!(
            r.selftest[0].message.contains("0 failures"),
            "{}",
            r.selftest[0].message
        );
    }

    #[test]
    fn bless_overlay_and_rebase() {
        let d = tmp("bless");
        let root = &d.0;
        let mut m = Manifest::default();
        let p = add(
            root,
            &mut m,
            "e2e.ta",
            "b",
            &[("a", json!(1)), ("b", json!(2)), ("c", json!(3))],
            json!({"a": 1, "b": 2, "c": 3}),
        );
        m.file_mut(&p).unwrap().bit_exact = Some(3);
        m.save(root).unwrap();
        // Rust now says b = 2.5, c is gone, d is new
        set_rust(root, &mut m, &p, json!({"a": 1, "b": 2.5, "d": "x"}));
        let mut cfg = Config::new(root, "e2e");
        let r = run(&cfg, &lookup);
        assert_eq!(r.files[0].failures.len(), 3);
        cfg.bless = Some(BlessMode::Overlay);
        assert!(bless(&cfg, &r).unwrap_err().0.contains("POB_GOLDEN_REASON"));
        cfg.reason = Some("test: b follows the new rounding".into());
        cfg.bless_max = 2;
        assert!(bless(&cfg, &r)
            .unwrap_err()
            .0
            .contains("over POB_GOLDEN_BLESS_MAX=2"));
        cfg.bless_max = 200;
        cfg.filter = Some(Filter::parse("e2e.ta:b"));
        let o = bless(&cfg, &r).unwrap();
        assert_eq!(o.written[&p], 1);
        assert_eq!(o.lowered[&p], (3, 1));
        let ov = Overlay::read(&root.join("overlay/e2e/ta/b.bless.jsonl")).unwrap();
        assert_eq!(ov.lines["b"].pob, json!(2));
        assert_eq!(ov.lines["b"].new, json!(2.5));
        assert_eq!(ov.lines["b"].reason, "test: b follows the new rounding");
        // the base file is untouched; the run now fails only on c and d
        let m2 = Manifest::load(root).unwrap();
        assert!(m2.check(root).is_empty(), "{:?}", m2.check(root));
        cfg.filter = None;
        cfg.bless = None;
        let r = run(&cfg, &lookup);
        assert_eq!(
            r.files[0]
                .failures
                .iter()
                .map(|x| x.key.as_str())
                .collect::<Vec<_>>(),
            ["c", "d"]
        );
        // bless the rest: c becomes $absent, d is added
        cfg.bless = Some(BlessMode::Overlay);
        bless(&cfg, &r).unwrap();
        cfg.bless = None;
        let r = run(&cfg, &lookup);
        assert!(r.ok(), "{}", r.render(&BTreeMap::new()));
        let ov = Overlay::read(&root.join("overlay/e2e/ta/b.bless.jsonl")).unwrap();
        assert_eq!(ov.lines["c"].new, absent());
        assert_eq!(ov.lines["d"].pob, absent());
        // an unlisted overlay fails
        let mut m3 = Manifest::load(root).unwrap();
        let saved = m3.file(&p).unwrap().overlay.clone();
        m3.file_mut(&p).unwrap().overlay = None;
        m3.save(root).unwrap();
        let r = run(&cfg, &lookup);
        assert!(r.files[0].problems[0].contains("does not list it"));
        m3.file_mut(&p).unwrap().overlay = saved;
        m3.save(root).unwrap();

        // a rebase writes Rust's values into the base file and drops the overlay
        set_rust(
            root,
            &mut Manifest::load(root).unwrap(),
            &p,
            json!({"a": 4, "b": 2.5, "d": "x"}),
        );
        let r = run(&cfg, &lookup);
        assert_eq!(r.files[0].failures.len(), 1);
        cfg.bless = Some(BlessMode::Rebase);
        bless(&cfg, &r).unwrap();
        cfg.bless = None;
        let m4 = Manifest::load(root).unwrap();
        assert!(m4.file(&p).unwrap().origin.starts_with("rust@"));
        assert!(m4.file(&p).unwrap().overlay.is_none());
        assert!(!root.join("overlay/e2e/ta/b.bless.jsonl").exists());
        let base = GoldenFile::read(&root.join(&p)).unwrap();
        assert_eq!(
            base.lines
                .iter()
                .map(|l| (l.key.as_str(), l.value.clone()))
                .collect::<Vec<_>>(),
            [
                ("a", json!(4)),
                ("b", json!(2.5)),
                ("c", absent()),
                ("d", json!("x"))
            ]
        );
        assert!(run(&cfg, &lookup).ok());
    }

    #[test]
    fn report_is_independent_of_jobs() {
        let d = tmp("jobs");
        let root = &d.0;
        let mut m = Manifest::default();
        for i in 0..12 {
            let k = format!("b{i:02}");
            add(
                root,
                &mut m,
                "e2e.ta",
                &k,
                &[("x", json!(i)), ("y", json!(i + 1))],
                json!({"x": i, "y": 0}),
            );
        }
        m.save(root).unwrap();
        let mut texts = BTreeSet::new();
        for jobs in [1, 3, 8] {
            let mut cfg = Config::new(root, "e2e");
            cfg.jobs = jobs;
            let r = run(&cfg, &lookup);
            texts.insert(r.render(&BTreeMap::new()) + &r.diffs_tsv());
        }
        assert_eq!(texts.len(), 1);
        assert!(texts.iter().next().unwrap().contains("failed 12"));
    }
}
