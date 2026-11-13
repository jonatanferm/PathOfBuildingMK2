//! Records PoB's answers as the committed goldens (docs/standalone/GOLDEN-PLAN.md §3.1).
//!
//! ```text
//! pob_golden_record <set|tier|all> [--corpus all57|fresh|special|derived] [--builds a,b]
//!                   [--runs K] [--out tests/golden] [--verify] [--list] [--pob SRC]
//! ```
//!
//! * `<set>` is a set name (`e2e.ca`), a tier (`e2e`: every `e2e.*` set) or `all`.
//! * `--corpus` / `--builds` restrict the builds (chunked, resumable runs); `--runs K` overrides
//!   the set's default number of recording runs (E2E 3, growing to 6 while a run adds a value;
//!   function-level 2).
//! * `--verify` records into a temporary directory, always uncached (`POB_QUERY_CACHE=off`), and
//!   compares every file's text with the committed one (§4.2): the gate's determinism check.
//!   Nothing is written to `--out`.
//! * The process holds the machine-wide oracle lock for its whole run (`runner::OracleLock`),
//!   and sets `POB_ORACLE_LOCK_HELD=1` for its children.
//!
//! The recorders themselves are added by the GOLD units (GOLD3-GOLD12), one `record_<set>`
//! function each, registered in [`RECORDERS`]: the same Lua and the same inputs as the live
//! test, writing goldens instead of comparing. They write through `pob_golden::format` and the
//! `pob_golden::encode` encodings, so the recorder and the runner share one schema, and they use
//! [`RecordCtx::cache_allowed`]: only run 1 of a non-verify recording may read the probe cache
//! (to resume an interrupted first run); runs 2..K and every `--verify` run start a fresh,
//! uncached PoB, or the nondeterminism capture would quietly repeat run 1.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};
use std::process::ExitCode;

use pob_golden::format::GoldenFile;
use pob_golden::manifest::{entry_for, Manifest, POB_VERSION};
use pob_oracle::oracle::find_pob_src;
use pob_oracle::runner::{OracleLock, LOCK_HELD_ENV};
use serde_json::{json, Value as J};

const USAGE: &str =
    "usage: pob_golden_record <set|tier|all> [--corpus all57|fresh|special|derived] \
[--builds a,b] [--runs K] [--out DIR] [--verify] [--list] [--pob SRC]";

/// The build corpora of §2.1.
const CORPORA: [&str; 4] = ["all57", "fresh", "special", "derived"];

/// What a recorder gets.
#[derive(Debug, Clone)]
pub struct RecordCtx {
    /// Where to write (the goldens directory, or the verify directory).
    pub out: PathBuf,
    /// The committed goldens directory (fixtures are read from here).
    pub golden: PathBuf,
    /// PoB's `src/`.
    pub pob_src: PathBuf,
    pub corpus: Option<String>,
    pub builds: Option<Vec<String>>,
    /// `--runs`, when given.
    pub runs: Option<usize>,
    pub verify: bool,
    /// `rust-port@<sha>`.
    pub recorder: String,
}

impl RecordCtx {
    /// Whether recording run `run` (1-based) may read the probe cache: only run 1, and never
    /// in `--verify` (§3.1).
    #[must_use]
    pub fn cache_allowed(&self, run: usize) -> bool {
        run == 1 && !self.verify
    }

    /// The number of runs for a set whose default is `default`.
    #[must_use]
    pub fn runs_or(&self, default: usize) -> usize {
        self.runs.unwrap_or(default).max(1)
    }

    /// Whether a build of `corpus` named `stem` is selected by `--corpus` / `--builds`.
    #[must_use]
    pub fn keep_build(&self, corpus: &str, stem: &str) -> bool {
        self.corpus.as_deref().map_or(true, |c| c == corpus)
            && self.builds.as_ref().map_or(true, |b| {
                b.iter()
                    .any(|x| x == stem || x == &format!("{corpus}/{stem}"))
            })
    }

    /// The header's `origin` (§3.2): PoB's version, the LuaJIT host and the number of runs.
    #[must_use]
    pub fn origin(&self, runs: usize) -> J {
        json!({"pob": POB_VERSION, "luajit": luajit_host(), "runs": runs})
    }
}

/// `{arch, numberMode, jit}` of the oracle's LuaJIT (§3.1 host artefacts): arm64 runs in
/// dual-number mode, x64 in single-number mode; the oracle boots with the JIT off unless
/// `POB_ORACLE_JIT=on`.
#[must_use]
pub fn luajit_host() -> J {
    let arch = match std::env::consts::ARCH {
        "aarch64" => "arm64",
        "x86_64" => "x64",
        a => a,
    };
    let mode = if arch == "arm64" { "dual" } else { "single" };
    let jit = if pob_oracle::jit::oracle_jit_off() {
        "off"
    } else {
        "on"
    };
    json!({"arch": arch, "numberMode": mode, "jit": jit})
}

/// A recorder: the golden files of its set, as (path relative to the goldens directory, file).
pub type RecordFn = fn(&RecordCtx) -> Result<Vec<(String, GoldenFile)>, String>;

/// The registered recorders: (set, function). The GOLD units add their `record_<set>` here.
pub const RECORDERS: &[(&str, RecordFn)] = &[];

#[derive(Debug, Default)]
struct Args {
    target: String,
    corpus: Option<String>,
    builds: Option<Vec<String>>,
    runs: Option<usize>,
    out: Option<PathBuf>,
    verify: bool,
    list: bool,
    pob: Option<PathBuf>,
}

fn parse_args(argv: &[String]) -> Result<Args, String> {
    let mut a = Args::default();
    let mut it = argv.iter();
    while let Some(x) = it.next() {
        let mut val = |name: &str| {
            it.next()
                .cloned()
                .ok_or_else(|| format!("{name} needs a value"))
        };
        match x.as_str() {
            "--corpus" => {
                let c = val("--corpus")?;
                if !CORPORA.contains(&c.as_str()) {
                    return Err(format!("--corpus {c}: one of {CORPORA:?}"));
                }
                a.corpus = Some(c);
            }
            "--builds" => {
                a.builds = Some(val("--builds")?.split(',').map(str::to_owned).collect());
            }
            "--runs" => {
                let r = val("--runs")?;
                a.runs = Some(r.parse().map_err(|_| format!("--runs {r}: not a number"))?);
            }
            "--out" => a.out = Some(PathBuf::from(val("--out")?)),
            "--pob" => a.pob = Some(PathBuf::from(val("--pob")?)),
            "--verify" => a.verify = true,
            "--list" => a.list = true,
            s if s.starts_with("--") => return Err(format!("unknown option {s}")),
            s if a.target.is_empty() => s.clone_into(&mut a.target),
            s => return Err(format!("unexpected argument {s}")),
        }
    }
    if a.target.is_empty() && !a.list {
        return Err("no set given".into());
    }
    Ok(a)
}

/// The recorders `target` names: a set, a tier (`e2e`) or `all`.
fn select(target: &str) -> Result<Vec<(&'static str, RecordFn)>, String> {
    let picked: Vec<_> = RECORDERS
        .iter()
        .copied()
        .filter(|(s, _)| {
            target == "all" || *s == target || pob_golden::manifest::tier_of(s) == target
        })
        .collect();
    if picked.is_empty() && target != "all" {
        let known: Vec<&str> = RECORDERS.iter().map(|(s, _)| *s).collect();
        return Err(format!(
            "no recorder for {target:?} (registered: {known:?})"
        ));
    }
    Ok(picked)
}

/// `rust-port@<short sha>` of the working tree.
fn recorder_id(dir: &Path) -> String {
    format!("rust-port@{}", pob_golden::runner::head_commit(dir))
}

/// Write the recorded files under `out` and their MANIFEST entries (pins of an existing entry
/// are kept).
fn write_files(
    out: &Path,
    files: &[(String, GoldenFile)],
    manifest: &mut Manifest,
) -> Result<(), String> {
    for (rel, gf) in files {
        let bytes = gf
            .write(&out.join(rel))
            .map_err(|e| format!("{rel}: {e}"))?;
        let mut e = entry_for(rel, gf, &bytes);
        if let Some(old) = manifest.file(rel) {
            e.compared = old.compared;
            e.bit_exact = old.bit_exact;
            e.known.clone_from(&old.known);
            e.required_keys.clone_from(&old.required_keys);
            e.overlay.clone_from(&old.overlay);
            e.extra.clone_from(&old.extra);
        }
        manifest.upsert_file(e);
    }
    Ok(())
}

/// `--verify`: every recorded file's text equals the committed file's (§4.2). Returns the
/// paths that differ, with a reason.
fn verify(golden: &Path, files: &[(String, GoldenFile)]) -> Vec<String> {
    let mut bad = Vec::new();
    for (rel, gf) in files {
        let new = match gf.to_text() {
            Ok(t) => t,
            Err(e) => {
                bad.push(format!("{rel}: {e}"));
                continue;
            }
        };
        match pob_golden::format::read_text(&golden.join(rel)) {
            Ok(old) if old == new => {}
            Ok(old) => {
                let (o, n): (BTreeMap<&str, &str>, BTreeMap<&str, &str>) =
                    (key_lines(&old), key_lines(&new));
                let changed = o
                    .iter()
                    .filter(|(k, v)| n.get(*k).is_some_and(|w| w != *v))
                    .count();
                let added = n.keys().filter(|k| !o.contains_key(*k)).count();
                let removed = o.keys().filter(|k| !n.contains_key(*k)).count();
                bad.push(format!(
                    "{rel}: differs ({changed} lines changed, {added} added, {removed} removed; \
                     a grown $anyOf/$unordered/orders list is folded in and the verify repeated)"
                ));
            }
            Err(e) => bad.push(format!("{rel}: not committed ({e})")),
        }
    }
    bad
}

/// A file's lines by key (the key is the first JSON string of the line; the header is `""`).
fn key_lines(text: &str) -> BTreeMap<&str, &str> {
    text.lines()
        .enumerate()
        .map(|(i, l)| {
            let key = if i == 0 {
                ""
            } else {
                l.get(2..).and_then(|r| r.split("\",").next()).unwrap_or(l)
            };
            (key, l)
        })
        .collect()
}

fn run(a: &Args) -> Result<bool, String> {
    if a.list {
        for (s, _) in RECORDERS {
            println!("{s}");
        }
        if a.target.is_empty() {
            return Ok(true);
        }
    }
    let recorders = select(&a.target)?;
    let golden = a.out.clone().unwrap_or_else(pob_golden::golden_root);
    let pob_src = a
        .pob
        .clone()
        .or_else(find_pob_src)
        .ok_or("PoB checkout not found (.PathOfBuilding/src, $POB_SRC or --pob)")?;
    if recorders.is_empty() {
        println!("pob_golden_record: no recorders registered yet (the GOLD units add them); nothing to do");
        return Ok(true);
    }
    // one PoB at a time machine-wide, for the whole run
    let _lock = OracleLock::acquire().map_err(|e| e.0)?;
    std::env::set_var(LOCK_HELD_ENV, "1");
    let out = if a.verify {
        // §4.2: a fresh, uncached PoB for every run
        std::env::set_var("POB_QUERY_CACHE", "off");
        let d = std::env::temp_dir().join(format!("pob-golden-verify-{}", std::process::id()));
        let _ = std::fs::remove_dir_all(&d);
        d
    } else {
        golden.clone()
    };
    let ctx = RecordCtx {
        out: out.clone(),
        golden: golden.clone(),
        pob_src,
        corpus: a.corpus.clone(),
        builds: a.builds.clone(),
        runs: a.runs,
        verify: a.verify,
        recorder: recorder_id(&golden),
    };
    let mut manifest = Manifest::load(&golden).unwrap_or_default();
    let mut all_ok = true;
    for (set, f) in recorders {
        let t = std::time::Instant::now();
        let files = f(&ctx).map_err(|e| format!("{set}: {e}"))?;
        if a.verify {
            let bad = verify(&golden, &files);
            for b in &bad {
                println!("VERIFY {set}: {b}");
            }
            all_ok &= bad.is_empty();
            println!(
                "verify {set}: {} files, {} differ ({:.1}s)",
                files.len(),
                bad.len(),
                t.elapsed().as_secs_f64()
            );
        } else {
            write_files(&out, &files, &mut manifest)?;
            manifest.save(&golden).map_err(|e| e.0)?;
            println!(
                "record {set}: {} files, {} lines ({:.1}s)",
                files.len(),
                files.iter().map(|(_, g)| g.lines.len()).sum::<usize>(),
                t.elapsed().as_secs_f64()
            );
        }
    }
    if a.verify {
        let _ = std::fs::remove_dir_all(&out);
    }
    Ok(all_ok)
}

fn main() -> ExitCode {
    let argv: Vec<String> = std::env::args().skip(1).collect();
    let a = match parse_args(&argv) {
        Ok(a) => a,
        Err(e) => {
            eprintln!("pob_golden_record: {e}\n{USAGE}");
            return ExitCode::from(2);
        }
    };
    match run(&a) {
        Ok(true) => ExitCode::SUCCESS,
        Ok(false) => ExitCode::from(1),
        Err(e) => {
            eprintln!("pob_golden_record: {e}");
            ExitCode::from(1)
        }
    }
}
