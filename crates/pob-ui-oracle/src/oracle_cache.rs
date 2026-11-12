//! Content-addressed cache of the Lua side of the UI oracle tests, and the bounded worker pool
//! they run their independent cases on (TP2; docs/port/testing.md "pob-ui-oracle: Lua-side cache
//! and parallel cases"). Infrastructure, not a port of PoB code.
//!
//! What is cached is always a pure function of the key: the bytes the Lua side produced (a
//! `LuaCalcBridge` session's dump documents, a scenario's Lua traces and the test's queries on the
//! VM afterwards). The key hashes every input that can change those bytes (see [`KeyBuilder`] and
//! the two key makers, `lua_calc::session_key` and [`ui_key`]), so a hit is exactly what a fresh
//! run would return, up to PoB's own per-process nondeterminism (LuaJIT's seeded string hashing:
//! `pairs` order over string keys, docs/port/STYLE.md §3.4), which the tests already tolerate.
//!
//! Layout: `<root>/<kind>/<k[0..2]>/<k>.zst`, `k` = 32 hex digits of BLAKE3. Each entry is
//! `MAGIC`, the BLAKE3 of the payload and the zstd payload; an entry that fails the check reads as
//! a miss and is rewritten. Entries are written to a private temp name in the same directory and
//! renamed into place, so readers see a whole entry or none, and concurrent writers of one key
//! (threads, test processes, worktrees) each publish a valid copy.
//!
//! Environment:
//! * `POB_ORACLE_NOCACHE=1`: neither read nor write (every Lua side runs, as before TP2).
//! * `POB_ORACLE_CACHE_VERIFY=1`: run the Lua side on every hit too and fail on a difference
//!   (the mismatching pair is kept under `<root>/verify-mismatch/`); misses are written.
//! * `POB_UI_ORACLE_CACHE=DIR`: the cache root (default `$XDG_CACHE_HOME/pob-ui-oracle`, else
//!   `~/.cache/pob-ui-oracle`: machine-wide, shared by every worktree and both corpora).
//! * `POB_TEST_PAR=N`: the most cases [`par_map`] runs at once (default [`DEFAULT_PAR`] = 3).
//! * `POB_TEST_MEM_MB=N`: the process RSS a new case may be expected to reach (default
//!   [`DEFAULT_MEM_MB`]); fewer cases run at once when they are heavy (see `CasePool`).
//!   `POB_TEST_POOL_LOG=1` logs each admission to stderr.
//! * `POB_TEST_LUA_PAR=N`: LuaJIT PoBs booted at once on cache misses (Lua UI sides and bridge
//!   sessions; default 1; [`LuaVmPermit`]).
//! * `POB_TEST_MALLOC_ARENAS=0`: keep glibc's default malloc arena count (see
//!   `limit_malloc_arenas`; an explicit `MALLOC_ARENA_MAX` is always kept).

use std::io::Write as _;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, AtomicUsize, Ordering};
use std::sync::{Condvar, Mutex, OnceLock};

/// What the environment allows.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Mode {
    On,
    Off,
    Verify,
}

fn env_on(name: &str) -> bool {
    std::env::var(name).is_ok_and(|v| !v.is_empty() && v != "0")
}

/// The mode for this process.
#[must_use]
pub fn mode() -> Mode {
    if env_on("POB_ORACLE_NOCACHE") {
        Mode::Off
    } else if env_on("POB_ORACLE_CACHE_VERIFY") {
        Mode::Verify
    } else {
        Mode::On
    }
}

/// The cache root (see the module doc).
#[must_use]
pub fn root() -> PathBuf {
    if let Some(p) = std::env::var_os("POB_UI_ORACLE_CACHE").filter(|p| !p.is_empty()) {
        return PathBuf::from(p);
    }
    if let Some(p) = std::env::var_os("XDG_CACHE_HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(p).join("pob-ui-oracle");
    }
    if let Some(h) = std::env::var_os("HOME").filter(|p| !p.is_empty()) {
        return PathBuf::from(h).join(".cache/pob-ui-oracle");
    }
    std::env::temp_dir().join("pob-ui-oracle-cache")
}

/// Bumped to retire every entry when something the keys do not see changes the Lua side.
pub const EPOCH: u32 = 1;

/// A finished key: the entry kind (a directory) and the hash.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Key {
    pub kind: String,
    pub hex: String,
}

impl Key {
    /// The entry's path under `root`.
    #[must_use]
    pub fn path(&self, root: &Path) -> PathBuf {
        root.join(&self.kind)
            .join(&self.hex[..2])
            .join(format!("{}.zst", self.hex))
    }
}

/// BLAKE3 over length-prefixed parts (part boundaries count), domain-separated by the kind and
/// [`EPOCH`].
pub struct KeyBuilder {
    kind: String,
    h: blake3::Hasher,
}

impl KeyBuilder {
    #[must_use]
    pub fn new(kind: &str) -> KeyBuilder {
        let mut h = blake3::Hasher::new();
        h.update(b"pob-ui-oracle cache\0");
        h.update(&EPOCH.to_le_bytes());
        h.update(kind.as_bytes());
        h.update(&[0]);
        KeyBuilder {
            kind: kind.to_owned(),
            h,
        }
    }

    /// Add one part.
    pub fn part(&mut self, bytes: impl AsRef<[u8]>) -> &mut KeyBuilder {
        let b = bytes.as_ref();
        self.h.update(&(b.len() as u64).to_le_bytes());
        self.h.update(b);
        self
    }

    #[must_use]
    pub fn finish(&self) -> Key {
        Key {
            kind: self.kind.clone(),
            hex: self.h.finalize().to_hex()[..32].to_owned(),
        }
    }
}

const MAGIC: &[u8; 8] = b"PUOC\x01\0\0\0";

/// Statistics of this process (for the tests' summaries).
static HITS: AtomicUsize = AtomicUsize::new(0);
static MISSES: AtomicUsize = AtomicUsize::new(0);
static BYPASSED: AtomicUsize = AtomicUsize::new(0);
/// Milliseconds spent computing misses / bypassed entries.
static COMPUTE_MS: AtomicU64 = AtomicU64::new(0);

/// One line: `oracle cache (mode, root): H hits, M misses (T s computing), B uncached`.
#[must_use]
pub fn summary() -> String {
    format!(
        "pob-ui-oracle cache ({:?}, {}): {} hits, {} misses, {} uncached runs ({:.1}s in Lua runs)",
        mode(),
        root().display(),
        HITS.load(Ordering::Relaxed),
        MISSES.load(Ordering::Relaxed),
        BYPASSED.load(Ordering::Relaxed),
        COMPUTE_MS.load(Ordering::Relaxed) as f64 / 1000.0
    )
}

/// `(hits, misses, uncached)` so far in this process.
#[must_use]
pub fn stats() -> (usize, usize, usize) {
    (
        HITS.load(Ordering::Relaxed),
        MISSES.load(Ordering::Relaxed),
        BYPASSED.load(Ordering::Relaxed),
    )
}

/// The payload of the entry at `path`, if it exists and is whole.
fn read_entry(path: &Path) -> Option<Vec<u8>> {
    let raw = std::fs::read(path).ok()?;
    if raw.len() < MAGIC.len() + 32 || &raw[..MAGIC.len()] != MAGIC {
        return None;
    }
    let (sum, body) = raw[MAGIC.len()..].split_at(32);
    let data = zstd::decode_all(body).ok()?;
    (blake3::hash(&data).as_bytes() == sum).then_some(data)
}

/// A temp name next to `dst` that no other writer uses.
fn private_tmp(dst: &Path) -> PathBuf {
    static SEQ: AtomicU64 = AtomicU64::new(0);
    let nanos = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| d.subsec_nanos());
    dst.with_file_name(format!(
        ".tmp.{}.{}.{nanos}",
        std::process::id(),
        SEQ.fetch_add(1, Ordering::Relaxed)
    ))
}

/// Write `data` to `path` atomically (temp file + rename). Errors are reported, not fatal: the
/// cache is an optimisation.
fn write_entry(path: &Path, data: &[u8]) {
    let r = (|| -> std::io::Result<()> {
        let dir = path.parent().expect("entry dir");
        std::fs::create_dir_all(dir)?;
        let tmp = private_tmp(path);
        let body = zstd::encode_all(data, 3)?;
        let res = (|| {
            let mut f = std::fs::File::create(&tmp)?;
            f.write_all(MAGIC)?;
            f.write_all(blake3::hash(data).as_bytes())?;
            f.write_all(&body)?;
            f.sync_data()?;
            drop(f);
            std::fs::rename(&tmp, path)
        })();
        if res.is_err() {
            let _ = std::fs::remove_file(&tmp);
        }
        res
    })();
    if let Err(e) = r {
        eprintln!("pob-ui-oracle cache: cannot write {}: {e}", path.display());
    }
}

fn timed<T>(f: impl FnOnce() -> T) -> T {
    let t = std::time::Instant::now();
    let r = f();
    COMPUTE_MS.fetch_add(
        u64::try_from(t.elapsed().as_millis()).unwrap_or(u64::MAX),
        Ordering::Relaxed,
    );
    r
}

/// The cached bytes for `key`, else `compute()`'s (stored when it succeeds). Errors are never
/// cached. Under [`Mode::Verify`] a hit is recomputed and compared byte for byte.
pub fn get_or_compute(
    key: &Key,
    compute: impl FnOnce() -> Result<Vec<u8>, String>,
) -> Result<Vec<u8>, String> {
    get_or_compute_with(key, compute, |a, b| a == b)
}

/// [`get_or_compute`] with the comparison [`Mode::Verify`] uses (`same(cached, fresh)`), for
/// payloads PoB itself does not reproduce byte for byte (`pairs` order over string keys).
pub fn get_or_compute_with(
    key: &Key,
    compute: impl FnOnce() -> Result<Vec<u8>, String>,
    same: impl FnOnce(&[u8], &[u8]) -> bool,
) -> Result<Vec<u8>, String> {
    let mode = mode();
    if mode == Mode::Off {
        BYPASSED.fetch_add(1, Ordering::Relaxed);
        return timed(compute);
    }
    let root = root();
    let path = key.path(&root);
    if let Some(cached) = read_entry(&path) {
        HITS.fetch_add(1, Ordering::Relaxed);
        if mode == Mode::Verify {
            let fresh = timed(compute)?;
            if !same(&cached, &fresh) {
                let dir = root.join("verify-mismatch");
                let _ = std::fs::create_dir_all(&dir);
                let _ = std::fs::write(dir.join(format!("{}.cached", key.hex)), &cached);
                let _ = std::fs::write(dir.join(format!("{}.fresh", key.hex)), &fresh);
                let at = cached
                    .iter()
                    .zip(&fresh)
                    .position(|(a, b)| a != b)
                    .unwrap_or(cached.len().min(fresh.len()));
                return Err(format!(
                    "pob-ui-oracle cache verify: {}/{} differs from a fresh Lua run ({} cached bytes, {} fresh, first byte difference at {at}; both kept in {})",
                    key.kind,
                    key.hex,
                    cached.len(),
                    fresh.len(),
                    dir.display()
                ));
            }
        }
        return Ok(cached);
    }
    MISSES.fetch_add(1, Ordering::Relaxed);
    let data = timed(compute)?;
    write_entry(&path, &data);
    Ok(data)
}

/// [`get_or_compute`] for a serde value: stored as JSON, and only when the JSON reads back as the
/// same JSON (a NaN would come back as `null`); such a value is returned but not cached.
pub fn get_or_compute_json<T: serde::Serialize + serde::de::DeserializeOwned>(
    key: &Key,
    compute: impl FnOnce() -> Result<T, String>,
) -> Result<T, String> {
    get_or_compute_json_with(key, compute, |a: &T, b: &T| {
        serde_json::to_vec(a).ok() == serde_json::to_vec(b).ok()
    })
}

/// [`get_or_compute_json`] with the comparison [`Mode::Verify`] uses (`same(cached, fresh)`).
pub fn get_or_compute_json_with<T: serde::Serialize + serde::de::DeserializeOwned>(
    key: &Key,
    compute: impl FnOnce() -> Result<T, String>,
    same: impl FnOnce(&T, &T) -> bool,
) -> Result<T, String> {
    let mut direct: Option<T> = None;
    let decode = |b: &[u8]| serde_json::from_slice::<T>(b).ok();
    let same_bytes = |a: &[u8], b: &[u8]| {
        a == b
            || match (decode(a), decode(b)) {
                (Some(x), Some(y)) => same(&x, &y),
                _ => false,
            }
    };
    let bytes = get_or_compute_with(
        key,
        || {
            let v = compute()?;
            let s = serde_json::to_vec(&v).map_err(|e| format!("cache encode: {e}"))?;
            let round = serde_json::from_slice::<T>(&s)
                .ok()
                .and_then(|r| serde_json::to_vec(&r).ok());
            if round.as_deref() == Some(&s[..]) {
                Ok(s)
            } else {
                direct = Some(v);
                Err(String::from(NOT_CACHEABLE))
            }
        },
        same_bytes,
    );
    match bytes {
        Ok(b) => serde_json::from_slice(&b).map_err(|e| format!("cache decode {}: {e}", key.hex)),
        Err(e) if e == NOT_CACHEABLE && direct.is_some() => Ok(direct.expect("computed value")),
        Err(e) => Err(e),
    }
}

/// Verify comparison of pob-state/1 documents (JSON): equal after dropping `$id` and the `$ref`
/// targets (both follow encounter order), with every list compared as a multiset and numbers to
/// `pob_oracle::diff::REL_TOL` (`pob_oracle::diff::tree_eq_unordered`, the u15 gate's order-only
/// rule): PoB's own runs differ in mod and list order (seeded string hashing).
#[must_use]
pub fn same_state_doc(a: &str, b: &str) -> bool {
    fn norm(v: &mut serde_json::Value) {
        match v {
            serde_json::Value::Object(m) => {
                m.remove("$id");
                if let Some(r) = m.get_mut("$ref") {
                    *r = serde_json::Value::Null;
                }
                m.values_mut().for_each(norm);
            }
            serde_json::Value::Array(a) => a.iter_mut().for_each(norm),
            _ => {}
        }
    }
    let (Ok(mut x), Ok(mut y)) = (
        serde_json::from_str::<serde_json::Value>(a),
        serde_json::from_str::<serde_json::Value>(b),
    ) else {
        return a == b;
    };
    norm(&mut x);
    norm(&mut y);
    x == y || pob_oracle::diff::tree_eq_unordered(&x, &y, pob_oracle::diff::REL_TOL)
}

const NOT_STORED: &str = "\0pob-ui-oracle cache: a run that failed is not stored";
const NOT_CACHEABLE: &str = "\0pob-ui-oracle cache: value does not round-trip through JSON";

// ---- source fingerprints -------------------------------------------------------------------------

/// The workspace root (this crate is `crates/pob-ui-oracle`).
#[must_use]
pub fn workspace_root() -> PathBuf {
    PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("../..")
}

fn walk(dir: &Path, out: &mut Vec<PathBuf>) {
    if dir.is_file() {
        out.push(dir.to_path_buf());
        return;
    }
    let Ok(rd) = std::fs::read_dir(dir) else {
        return;
    };
    let mut entries: Vec<_> = rd.filter_map(Result::ok).collect();
    entries.sort_by_key(std::fs::DirEntry::file_name);
    for e in entries {
        let p = e.path();
        match e.file_type() {
            Ok(t) if t.is_dir() => walk(&p, out),
            Ok(_) => out.push(p),
            Err(_) => {}
        }
    }
}

/// BLAKE3 of the contents of every file under `dirs` (names relative to `base` included), in name
/// order. A missing directory hashes as empty. Only relative names go in, never `base` itself:
/// the same tree hashes the same in every worktree.
#[must_use]
pub fn tree_hash(base: &Path, dirs: &[&str]) -> String {
    tree_hash_except(base, dirs, &[])
}

/// [`tree_hash`] without the files (or whole directories) named in `skip` (relative to `base`).
#[must_use]
pub fn tree_hash_except(base: &Path, dirs: &[&str], skip: &[&str]) -> String {
    let mut h = blake3::Hasher::new();
    for d in dirs {
        h.update(d.as_bytes());
        h.update(&[0]);
        let mut files = Vec::new();
        walk(&base.join(d), &mut files);
        for f in files {
            let rel = f.strip_prefix(base).unwrap_or(&f).to_string_lossy();
            if skip
                .iter()
                .any(|s| rel == *s || rel.strip_prefix(s).is_some_and(|r| r.starts_with('/')))
            {
                continue;
            }
            h.update(&(rel.len() as u64).to_le_bytes());
            h.update(rel.as_bytes());
            let data = std::fs::read(&f).unwrap_or_default();
            h.update(&(data.len() as u64).to_le_bytes());
            h.update(&data);
        }
    }
    h.finalize().to_hex()[..32].to_owned()
}

/// The third-party packages of `Cargo.lock` (every `[[package]]` block with a `source`: registry
/// or git), in file order. The workspace's own entries (no `source`) are left out: they change
/// with every workspace dependency edit, and the workspace code that runs is hashed by content.
#[must_use]
pub fn third_party_lock(lock: &str) -> String {
    let mut out = String::new();
    for block in lock.split("[[package]]").skip(1) {
        if block.lines().any(|l| l.starts_with("source = ")) {
            out.push_str("[[package]]");
            out.push_str(block.trim_end());
            out.push('\n');
        }
    }
    out
}

/// The workspace code a UI oracle run's Lua side executes or draws through, hashed by content:
/// this crate's `src/` (boot, runner, the SG bridge, patches, markers, this cache's value types)
/// but not the parts that run only outside a Lua UI side (`lua_calc.rs`: the bridge, keyed and
/// cached on its own and never started inside [`cached_lua_run`]; `diff_report.rs`: comparison
/// after both sides ran; `bin/`: the CLI tools), its `lua/`, `probe/`, `build.rs` and
/// `Cargo.toml` (features); pob-sg (the `Sg` the Lua side draws through, its fonts and trace
/// recorder), pob-host (scripts, the headless image loader, key delivery) and pob-lua; of
/// pob-oracle what `boot.rs` takes from it (`jit`: the prelude, the natives, the JIT switch; and
/// what `jit.rs` uses: `headless.rs`, the `lua/` module, `lib.rs`) and its `Cargo.toml`; and the
/// third-party packages of `Cargo.lock` ([`third_party_lock`]; mlua/LuaJIT included).
///
/// TP4: the TP2 set hashed all of pob-oracle's `src/` and the whole `Cargo.lock`, so every merge
/// that touched the calc oracle (diff, inject, patcher, a new bin) or added a workspace crate or
/// dependency retired every UI entry: 9 of the 49 rust-port merges of 2026-11-11/12, about one
/// full-suite run in two.
#[must_use]
pub fn lua_side_code_hash() -> &'static str {
    static H: OnceLock<String> = OnceLock::new();
    H.get_or_init(|| {
        let ws = workspace_root();
        let mut k = KeyBuilder::new("lua-side-code");
        k.part(tree_hash_except(
            &ws,
            &[
                "crates/pob-ui-oracle/src",
                "crates/pob-ui-oracle/lua",
                "crates/pob-ui-oracle/probe",
                "crates/pob-ui-oracle/build.rs",
                "crates/pob-ui-oracle/Cargo.toml",
                "crates/pob-sg/src",
                "crates/pob-sg/Cargo.toml",
                "crates/pob-host/src",
                "crates/pob-host/Cargo.toml",
                "crates/pob-lua/src",
                "crates/pob-lua/Cargo.toml",
                "crates/pob-oracle/src/lib.rs",
                "crates/pob-oracle/src/jit.rs",
                "crates/pob-oracle/src/headless.rs",
                "crates/pob-oracle/src/lua",
                "crates/pob-oracle/Cargo.toml",
            ],
            &[
                "crates/pob-ui-oracle/src/lua_calc.rs",
                "crates/pob-ui-oracle/src/diff_report.rs",
                "crates/pob-ui-oracle/src/bin",
            ],
        ));
        k.part(third_party_lock(
            &std::fs::read_to_string(ws.join("Cargo.lock")).unwrap_or_default(),
        ));
        k.finish().hex
    })
}

/// The content key of a PoB tree (`src/` or `runtime/`): every file's path relative to `dir` and
/// the BLAKE3 of its content, except PoB's derived caches (`pob_oracle::patcher::is_derived_cache`:
/// the timeless jewel `.bin`s PoB inflates and rewrites on any run), plus what decides how the
/// tree runs: the oracle runtime `JitPob` boots it with (`jit.rs`, `headless.rs`, `oracle.rs`,
/// `oracle.lua`, compiled in), the mlua/LuaJIT versions and the JIT mode. Memoised per process.
///
/// TP4: content-addressed. TP2 used `pob_oracle::runner::source_key`, which hashes the canonical
/// path and every file's mtime: a PoB checkout at another path, or the same files touched (a
/// `git checkout`, a copy), missed every entry. The per-file content hashes are kept in a stat
/// index under the cache root ([`file_content_hashes`]), so only changed files are read again.
pub fn pob_tree_key(dir: &Path) -> Result<String, String> {
    use std::collections::HashMap;
    static KEYS: OnceLock<Mutex<HashMap<PathBuf, String>>> = OnceLock::new();
    let canon = std::fs::canonicalize(dir).map_err(|e| format!("{}: {e}", dir.display()))?;
    // held while a key is computed: the cases of a `par_map` ask at once, and one walk is enough
    let mut keys = KEYS
        .get_or_init(Mutex::default)
        .lock()
        .unwrap_or_else(std::sync::PoisonError::into_inner);
    if let Some(k) = keys.get(&canon) {
        return Ok(k.clone());
    }
    let key = tree_content_key(&canon, &root().join("stat-index"))?;
    keys.insert(canon, key.clone());
    Ok(key)
}

/// [`pob_tree_key`] of the canonical `canon`, not memoised, with the stat index in `index_dir`.
fn tree_content_key(canon: &Path, index_dir: &Path) -> Result<String, String> {
    const ORACLE_RUNTIME: &[&str] = &[
        include_str!("../../pob-oracle/src/jit.rs"),
        include_str!("../../pob-oracle/src/headless.rs"),
        include_str!("../../pob-oracle/src/oracle.rs"),
        include_str!("../../pob-oracle/src/oracle.lua"),
    ];
    let mut k = KeyBuilder::new("pob-tree");
    for code in ORACLE_RUNTIME {
        k.part(code);
    }
    k.part(pob_oracle::runner::lua_runtime_versions());
    k.part(if pob_oracle::jit::oracle_jit_off() {
        "jit.off"
    } else {
        "jit.on"
    });
    let files = file_content_hashes(canon, index_dir)?;
    k.part((files.len() as u64).to_le_bytes());
    for (rel, sum) in &files {
        k.part(rel).part(sum);
    }
    Ok(k.finish().hex)
}

/// One file of a stat index: what `stat` said when its content was hashed, and when.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
struct StatRecord {
    len: u64,
    mtime_ns: u64,
    ino: u64,
    /// When the content was read (ns since the epoch).
    hashed_ns: u64,
    blake3: String,
}

/// A record is trusted only when the file's mtime is this much older than the time its content
/// was read: a write in the same timestamp tick as the read (racy git) is hashed again.
const RACY_NS: u64 = 2_000_000_000;

/// Nanoseconds since the epoch.
fn epoch_ns(t: std::time::SystemTime) -> u64 {
    t.duration_since(std::time::UNIX_EPOCH)
        .map_or(0, |d| u64::try_from(d.as_nanos()).unwrap_or(u64::MAX))
}

/// `(path relative to the canonical `dir`, BLAKE3 hex of the content)` of every file under `dir`
/// but PoB's derived caches, in name order. The content hashes are reused from a stat index
/// (`<root>/stat-index/<hash of dir>.json`: size, mtime and inode of each file at the time it was
/// read) when the file's stat is unchanged and its mtime was older than that read by
/// [`RACY_NS`]; any other file is read and hashed. The index only saves reads: the result is the
/// content, whatever the index says about other files or paths.
fn file_content_hashes(dir: &Path, index_dir: &Path) -> Result<Vec<(String, String)>, String> {
    use std::collections::BTreeMap;
    let mut files = Vec::new();
    walk(dir, &mut files);
    let index_path = index_dir.join(format!(
        "{}.json",
        &blake3::hash(dir.to_string_lossy().as_bytes()).to_hex()[..32]
    ));
    let old: BTreeMap<String, StatRecord> = std::fs::read(&index_path)
        .ok()
        .and_then(|b| serde_json::from_slice(&b).ok())
        .unwrap_or_default();
    let mut new: BTreeMap<String, StatRecord> = BTreeMap::new();
    let mut out = Vec::with_capacity(files.len());
    let mut changed = false;
    for f in files {
        let rel = f
            .strip_prefix(dir)
            .unwrap_or(&f)
            .to_string_lossy()
            .replace('\\', "/");
        if pob_oracle::patcher::is_derived_cache(&rel) {
            continue;
        }
        let meta = std::fs::metadata(&f).map_err(|e| format!("{}: {e}", f.display()))?;
        let mtime_ns = meta.modified().map_or(0, epoch_ns);
        #[cfg(unix)]
        let ino = std::os::unix::fs::MetadataExt::ino(&meta);
        #[cfg(not(unix))]
        let ino = 0;
        let reuse = old.get(&rel).filter(|r| {
            r.len == meta.len()
                && r.mtime_ns == mtime_ns
                && r.ino == ino
                && mtime_ns.saturating_add(RACY_NS) < r.hashed_ns
        });
        let rec = if let Some(r) = reuse {
            r.clone()
        } else {
            changed = true;
            let hashed_ns = epoch_ns(std::time::SystemTime::now());
            let data = std::fs::read(&f).map_err(|e| format!("{}: {e}", f.display()))?;
            StatRecord {
                len: meta.len(),
                mtime_ns,
                ino,
                hashed_ns,
                blake3: blake3::hash(&data).to_hex().to_string(),
            }
        };
        out.push((rel.clone(), rec.blake3.clone()));
        new.insert(rel, rec);
    }
    if (changed || new.len() != old.len()) && mode() != Mode::Off {
        if let Ok(json) = serde_json::to_vec(&new) {
            write_plain(&index_path, &json);
        }
    }
    Ok(out)
}

/// Write `data` to `path` through a private temp file and a rename (an optimisation: errors are
/// ignored).
fn write_plain(path: &Path, data: &[u8]) {
    let Some(dir) = path.parent() else { return };
    if std::fs::create_dir_all(dir).is_err() {
        return;
    }
    let tmp = private_tmp(path);
    if std::fs::write(&tmp, data).is_ok() && std::fs::rename(&tmp, path).is_ok() {
        return;
    }
    let _ = std::fs::remove_file(&tmp);
}

/// A test file's name (`file!()`, relative to the workspace root) and the source the test binary
/// was compiled from ([`test_source!`](crate::test_source)).
#[derive(Clone, Copy, Debug)]
pub struct TestSource {
    pub file: &'static str,
    pub text: &'static str,
}

/// The calling test file's [`TestSource`]: its `file!()` and its text, compiled in
/// (`include_str!`), so a key always describes the code that runs, even when the file is edited
/// while the binary runs (TP2 read the file at run time).
#[macro_export]
macro_rules! test_source {
    () => {
        $crate::oracle_cache::TestSource {
            file: file!(),
            text: include_str!(concat!(env!("CARGO_MANIFEST_DIR"), "/../../", file!())),
        }
    };
}

/// The key of one UI oracle Lua-side run of a test: `test` is the test file
/// ([`test_source!`](crate::test_source); its source holds the Lua stubs and the Lua-side
/// wrapper, so it is hashed whole), `pob_root` the PoB checkout (`src/` and `runtime/` content
/// keys, [`pob_tree_key`]), `fixtures` the directories and files the run reads (hashed by content:
/// a directory's files by their names relative to it, so never by where the checkout is), and
/// `params` everything else that decides the run (script text, screen, mode, query chunks). The
/// caller adds nothing else: everything goes through here.
///
/// TP4: no absolute path goes in. TP2 hashed each fixture's path, which is under the worktree
/// (`CARGO_MANIFEST_DIR`), so the full suite's `../pob-fullsuite` worktree, the main checkout and
/// every agent worktree each had keys of their own.
pub fn ui_key(
    kind: &str,
    test: TestSource,
    pob_root: &Path,
    fixtures: &[&Path],
    params: &[&str],
) -> Result<Key, String> {
    let mut k = KeyBuilder::new(kind);
    k.part(lua_side_code_hash());
    k.part(test.file).part(test.text);
    k.part(pob_tree_key(&pob_root.join("src"))?);
    k.part(pob_tree_key(&pob_root.join("runtime"))?);
    for f in fixtures {
        if f.is_dir() {
            k.part("dir").part(tree_hash(f, &[""]));
        } else {
            k.part("file").part(std::fs::read(f).unwrap_or_default());
        }
    }
    for p in params {
        k.part(p);
    }
    Ok(k.finish())
}

// ---- the Lua side of a UI scenario ---------------------------------------------------------------

/// What a test keeps of one Lua-side run: the traces, the run's error, the `snapshot` XMLs and
/// whatever it read from the VM or the user dir afterwards (`extra`, the test's own encoding).
#[derive(Clone, Debug, Default, serde::Serialize, serde::Deserialize)]
pub struct LuaRun {
    pub frames: Vec<pob_sg::trace::FrameTrace>,
    pub error: Option<String>,
    pub snapshots: Vec<(String, String)>,
    pub extra: Vec<String>,
}

/// `run()` through the cache under `key` (a [`ui_key`]). A run that ends in an error (`error` set,
/// or `run` failing) is returned but not stored: only complete runs are reused. `key` failing
/// (e.g. an unreadable input) runs uncached. `run` (a miss) holds a [`LuaVmPermit`], so it must
/// not itself start a `LuaCalcBridge` session (that takes one too).
pub fn cached_lua_run(
    key: Result<Key, String>,
    run: impl FnOnce() -> anyhow::Result<LuaRun>,
) -> anyhow::Result<LuaRun> {
    let key = match key {
        Ok(k) => k,
        Err(e) => {
            eprintln!("pob-ui-oracle cache: no key ({e}); running uncached");
            BYPASSED.fetch_add(1, Ordering::Relaxed);
            return timed(|| LuaVmPermit::scope(run));
        }
    };
    let mut uncacheable: Option<anyhow::Result<LuaRun>> = None;
    let r = get_or_compute_json_with(
        &key,
        || match LuaVmPermit::scope(run) {
            Ok(r) if r.error.is_none() => Ok(r),
            other => {
                uncacheable = Some(other);
                Err(String::from(NOT_STORED))
            }
        },
        same_lua_run,
    );
    match r {
        Ok(v) => Ok(v),
        Err(e) if e == NOT_STORED => uncacheable.expect("the run's result"),
        Err(e) => Err(anyhow::anyhow!("{e}")),
    }
}

/// Verify comparison of two Lua-side runs: the same error, traces equal up to PoB's own
/// per-process `pairs` order (`diff_report::order_only_difference`: every frame has the same
/// calls and side effects as multisets), and the same snapshots and `extra` up to that order
/// ([`same_text_unordered`]: PoB's `SaveDB` writes attributes and some sections in `pairs` order).
#[must_use]
pub fn same_lua_run(a: &LuaRun, b: &LuaRun) -> bool {
    a.error == b.error
        && a.snapshots.len() == b.snapshots.len()
        && a.snapshots
            .iter()
            .zip(&b.snapshots)
            .all(|(x, y)| x.0 == y.0 && same_text_unordered(&x.1, &y.1))
        && a.extra.len() == b.extra.len()
        && a.extra
            .iter()
            .zip(&b.extra)
            .all(|(x, y)| same_text_unordered(x, y))
        && crate::diff_report::order_only_difference(&a.frames, &b.frames).is_ok()
}

/// `a == b`, or the same multiset of lines once the `name="value"` attributes of each line are
/// sorted (XML written in `pairs` order; JSON-escaped newlines count as line breaks). A verify
/// check: it catches a stale or wrong entry, not an order difference.
#[must_use]
pub fn same_text_unordered(a: &str, b: &str) -> bool {
    fn norm(t: &str) -> Vec<String> {
        let mut lines: Vec<String> = t
            .split('\n')
            .flat_map(|l| l.split("\\n"))
            .map(|l| {
                let l = l.trim();
                // split at `"` boundaries: odd parts are values; a part before a value ending in
                // `name=` names an attribute
                let parts: Vec<&str> = l.split('"').collect();
                if parts.len() < 3 {
                    return l.to_owned();
                }
                let mut head = String::new();
                let mut attrs = Vec::new();
                let mut i = 0;
                while i < parts.len() {
                    let p = parts[i];
                    if i % 2 == 0 && i + 1 < parts.len() && p.trim_end().ends_with('=') {
                        let cut = p.rfind(char::is_whitespace).map_or(0, |c| c + 1);
                        head.push_str(&p[..cut]);
                        attrs.push(format!("{}\"{}\"", p[cut..].trim(), parts[i + 1]));
                        i += 2;
                    } else {
                        head.push_str(p);
                        if i + 1 < parts.len() {
                            head.push('"');
                        }
                        i += 1;
                    }
                }
                attrs.sort();
                format!("{} {}", head.trim(), attrs.join(" "))
            })
            .collect();
        lines.sort();
        lines
    }
    a == b || norm(a) == norm(b)
}

// ---- the bounded pool ---------------------------------------------------------------------------

/// The default upper bound on concurrent cases (`POB_TEST_PAR`).
pub const DEFAULT_PAR: usize = 3;

/// The default memory budget of a test process in MB (`POB_TEST_MEM_MB`).
pub const DEFAULT_MEM_MB: u64 = 3000;

/// `POB_TEST_PAR` (default [`DEFAULT_PAR`], at least 1): the most cases at once, lowered by
/// [`limit_par`].
#[must_use]
pub fn test_par() -> usize {
    std::env::var("POB_TEST_PAR")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(DEFAULT_PAR)
        .max(1)
        .min(PAR_LIMIT.load(Ordering::Relaxed))
}

static PAR_LIMIT: AtomicUsize = AtomicUsize::new(usize::MAX);

/// Lower [`test_par`] to at most `n` (at least 1) for the rest of the process: for a test binary
/// whose cases keep their memory (every one of its tests calls it before its first [`par_map`],
/// so it holds before any of them admits a case). u21 does: its process kept the RSS of every case
/// it ran (1 GB more at 3 cases at once early on than serially: 3.8-4.1 GB against 3.0 GB).
pub fn limit_par(n: usize) {
    PAR_LIMIT.fetch_min(n.max(1), Ordering::Relaxed);
}

/// `POB_TEST_MEM_MB` (default [`DEFAULT_MEM_MB`]): the RSS a new case may take the process to.
#[must_use]
pub fn mem_budget_kb() -> u64 {
    std::env::var("POB_TEST_MEM_MB")
        .ok()
        .and_then(|v| v.trim().parse::<u64>().ok())
        .unwrap_or(DEFAULT_MEM_MB)
        .saturating_mul(1024)
}

/// `(VmRSS, VmHWM)` of this process in kB (`/proc/self/status`), when readable.
fn rss_hwm_kb() -> Option<(u64, u64)> {
    let s = std::fs::read_to_string("/proc/self/status").ok()?;
    let field = |name: &str| {
        s.lines()
            .find_map(|l| l.strip_prefix(name))
            .and_then(|v| v.split_whitespace().next())
            .and_then(|v| v.parse::<u64>().ok())
    };
    Some((field("VmRSS:")?, field("VmHWM:")?))
}

/// A counting semaphore.
struct Permits {
    free: Mutex<usize>,
    cv: Condvar,
}

impl Permits {
    fn new(n: usize) -> Permits {
        Permits {
            free: Mutex::new(n),
            cv: Condvar::new(),
        }
    }
    fn take(&self) {
        let mut free = self
            .free
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        while *free == 0 {
            free = self
                .cv
                .wait(free)
                .unwrap_or_else(std::sync::PoisonError::into_inner);
        }
        *free -= 1;
    }
    fn give(&self) {
        *self
            .free
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) += 1;
        self.cv.notify_one();
    }
}

/// `POB_TEST_LUA_PAR` (default 1, at least 1): LuaJIT PoBs booted at once on cache misses.
#[must_use]
pub fn lua_par() -> usize {
    std::env::var("POB_TEST_LUA_PAR")
        .ok()
        .and_then(|v| v.trim().parse::<usize>().ok())
        .unwrap_or(1)
        .max(1)
}

/// Holding one of the process-wide Lua VM permits ([`lua_par`]): every cache miss that boots a
/// LuaJIT PoB (0.6-1 GB in a debug test process) holds one while the VM lives: a Lua UI side of
/// [`cached_lua_run`] and a `LuaCalcBridge` session (`lua_calc::LuaSession::cached_*`), so a
/// cold run keeps the old "one LuaJIT PoB at a time" per process while the other cases' Rust
/// sides keep running. Taken inside a case that already holds a [`Permit`], always before the
/// machine-wide oracle lock (bridge sessions), and its holder waits for nothing else in this
/// process, so the locks cannot deadlock.
pub struct LuaVmPermit(());

impl LuaVmPermit {
    fn sem() -> &'static Permits {
        static P: OnceLock<Permits> = OnceLock::new();
        P.get_or_init(|| Permits::new(lua_par()))
    }
    /// Block until a permit is free.
    #[must_use]
    pub fn acquire() -> LuaVmPermit {
        Self::sem().take();
        LuaVmPermit(())
    }
    /// `f()` holding a permit.
    pub fn scope<R>(f: impl FnOnce() -> R) -> R {
        let _p = Self::acquire();
        f()
    }
}

impl Drop for LuaVmPermit {
    fn drop(&mut self) {
        Self::sem().give();
    }
}

/// The process-wide case pool, shared by every test thread and every [`par_map`] call.
///
/// A case of a [`Site`] (one `par_map` call, or one [`Permit::acquire`] holder) is admitted when
/// no case runs, or when fewer than [`test_par`] run, the site's cases have been measured, and
/// the process's RSS plus the largest growth one of them caused, once for this case and once for
/// each running one (which may not have reached its peak yet), stays within [`mem_budget_kb`].
/// So the first case of every site runs alone, and a site whose cases are heavy for the budget
/// keeps running one at a time. Growth of a case = RSS (or the part of VmHWM above its start) at
/// its end minus RSS at its start, which also counts what concurrent cases allocated meanwhile:
/// conservative. A LuaJIT VM a case boots and frees in between is not in it; at most
/// [`lua_par`] of those exist at once ([`LuaVmPermit`]).
///
/// A test process starts at ~2 GB (data dump, timeless jewel tables, debug build) and a u21/u29
/// case adds 0.6-1 GB (a Rust `PobApp` with a build loaded and the bridge's injected envs), so
/// those run one at a time within the default budget, lighter ones (u32, u41, galleries) several
/// at once. Results do not depend on it ([`par_map`] keeps item order).
struct CasePool {
    running: Mutex<usize>,
    cv: Condvar,
}

/// The cases of one [`par_map`] call (or one [`Permit::acquire`] holder): the largest RSS growth
/// (kB) one of them was measured to cause, once one finished.
#[derive(Default)]
pub struct Site {
    case_kb: Mutex<Option<u64>>,
}

fn pool() -> &'static CasePool {
    static P: OnceLock<CasePool> = OnceLock::new();
    P.get_or_init(|| {
        limit_malloc_arenas(test_par().max(2));
        CasePool {
            running: Mutex::new(0),
            cv: Condvar::new(),
        }
    })
}

fn pool_log() -> bool {
    static L: OnceLock<bool> = OnceLock::new();
    *L.get_or_init(|| env_on("POB_TEST_POOL_LOG"))
}

impl CasePool {
    /// Block until a case of `site` may start; the `(RSS, VmHWM)` at its start.
    fn take(&self, site: &Site) -> Option<(u64, u64)> {
        let par = test_par();
        let budget = mem_budget_kb();
        let mut running = self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner);
        loop {
            let est = *site
                .case_kb
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            // room for this case and, conservatively, for the full growth of every running one
            // (they may not have reached their peak yet)
            let fits = *running == 0
                || (*running < par
                    && est.is_some_and(|c| match rss_hwm_kb() {
                        Some((rss, _)) => {
                            rss.saturating_add(c.saturating_mul(*running as u64 + 1)) <= budget
                        }
                        None => true,
                    }));
            if fits {
                break;
            }
            // RSS also falls without a notification (a case freeing memory): poll
            running = self
                .cv
                .wait_timeout(running, std::time::Duration::from_millis(500))
                .unwrap_or_else(std::sync::PoisonError::into_inner)
                .0;
        }
        *running += 1;
        let at = rss_hwm_kb();
        if pool_log() {
            let est = *site
                .case_kb
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            eprintln!(
                "pob-ui-oracle pool: admit ({} running), rss {:?} MB, case estimate {:?} MB",
                *running,
                at.map(|(r, _)| r / 1024),
                est.map(|c| c / 1024)
            );
        }
        at
    }

    fn give(&self, site: &Site, start: Option<(u64, u64)>) {
        if let (Some((r0, h0)), Some((r1, h1))) = (start, rss_hwm_kb()) {
            let grew = r1.saturating_sub(r0).max(h1.saturating_sub(h0.max(r0)));
            let mut c = site
                .case_kb
                .lock()
                .unwrap_or_else(std::sync::PoisonError::into_inner);
            *c = Some(c.map_or(grew, |c| c.max(grew)));
        }
        *self
            .running
            .lock()
            .unwrap_or_else(std::sync::PoisonError::into_inner) -= 1;
        self.cv.notify_all();
    }
}

/// Cap glibc's malloc arenas at `n` for the rest of the process (first pool use), unless
/// `MALLOC_ARENA_MAX` is set (glibc read it at start) or `POB_TEST_MALLOC_ARENAS=0`.
///
/// Every case thread gets a glibc arena of its own (up to 8 per core), and an arena keeps its
/// peak heap after the case is gone, so a test process's RSS grew with the number of threads it
/// ever ran rather than with the cases alive at once. Measured on u29 (warm, 2 workers): 3343 MB
/// peak RSS without the cap, 2746 MB with `MALLOC_ARENA_MAX=2`, same wall time.
#[cfg(all(target_os = "linux", target_env = "gnu"))]
#[allow(unsafe_code)]
fn limit_malloc_arenas(n: usize) {
    if std::env::var_os("MALLOC_ARENA_MAX").is_some()
        || std::env::var("POB_TEST_MALLOC_ARENAS").is_ok_and(|v| v == "0")
    {
        return;
    }
    extern "C" {
        fn mallopt(param: std::ffi::c_int, value: std::ffi::c_int) -> std::ffi::c_int;
    }
    /// glibc `malloc.h`.
    const M_ARENA_MAX: std::ffi::c_int = -8;
    let n = std::ffi::c_int::try_from(n).unwrap_or(std::ffi::c_int::MAX);
    // SAFETY: `mallopt` is glibc's (this cfg); M_ARENA_MAX only stores the limit (under the main
    // arena's lock) that later arena creation reads; plain ints in and out, no Rust state.
    unsafe {
        mallopt(M_ARENA_MAX, n);
    }
}

#[cfg(not(all(target_os = "linux", target_env = "gnu")))]
fn limit_malloc_arenas(_n: usize) {}

/// A running case of the process-wide pool (released on drop; see `CasePool`).
pub struct Permit {
    site: std::sync::Arc<Site>,
    start: Option<(u64, u64)>,
}

impl Permit {
    /// Block until the pool admits a case of a site of its own (a heavy case run outside
    /// [`par_map`], e.g. on a test thread): it starts when no other case runs.
    #[must_use]
    pub fn acquire() -> Permit {
        Permit::acquire_in(&std::sync::Arc::new(Site::default()))
    }

    /// Block until the pool admits one more case of `site`.
    #[must_use]
    pub fn acquire_in(site: &std::sync::Arc<Site>) -> Permit {
        let start = pool().take(site);
        Permit {
            site: site.clone(),
            start,
        }
    }
}

impl Drop for Permit {
    fn drop(&mut self) {
        pool().give(&self.site, self.start);
    }
}

/// Stack size of a case thread (the Rust UI side recurses deeply in debug builds).
pub const CASE_STACK: usize = 256 << 20;

/// `f` over `items` on fresh threads (one per item, so thread-local state never leaks between
/// cases), as many at a time as the process-wide pool admits ([`Permit`]: at most [`test_par`],
/// fewer when a case is heavy for [`mem_budget_kb`]); the results in `items`' order, whatever order the cases finish in. A panicking case is re-raised on the caller after
/// every case has finished (the first one in item order, so the failure is deterministic).
pub fn par_map<I, R, F>(items: Vec<I>, f: F) -> Vec<R>
where
    I: Send,
    R: Send,
    F: Fn(I) -> R + Sync,
{
    let n = items.len();
    let mut out: Vec<Option<std::thread::Result<R>>> = (0..n).map(|_| None).collect();
    let site = std::sync::Arc::new(Site::default());
    std::thread::scope(|s| {
        let f = &f;
        let mut handles = Vec::with_capacity(n);
        for (i, item) in items.into_iter().enumerate() {
            // admitted before spawning: only admitted cases have a thread
            let permit = Permit::acquire_in(&site);
            let h = std::thread::Builder::new()
                .name(format!("case-{i}"))
                .stack_size(CASE_STACK)
                .spawn_scoped(s, move || {
                    let _permit = permit;
                    f(item)
                })
                .expect("spawn case thread");
            handles.push((i, h));
        }
        for (i, h) in handles {
            out[i] = Some(h.join());
        }
    });
    let mut res = Vec::with_capacity(n);
    let mut panic = None;
    for r in out {
        match r.expect("case joined") {
            Ok(v) => res.push(v),
            Err(p) => {
                if panic.is_none() {
                    panic = Some(p);
                }
            }
        }
    }
    if let Some(p) = panic {
        std::panic::resume_unwind(p);
    }
    res
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn par_map_keeps_order_and_bounds_concurrency() {
        let live = AtomicUsize::new(0);
        let peak = AtomicUsize::new(0);
        let items: Vec<usize> = (0..12).collect();
        let out = par_map(items, |i| {
            let now = live.fetch_add(1, Ordering::SeqCst) + 1;
            peak.fetch_max(now, Ordering::SeqCst);
            std::thread::sleep(std::time::Duration::from_millis(5 * (12 - i as u64)));
            live.fetch_sub(1, Ordering::SeqCst);
            i * 10
        });
        assert_eq!(out, (0..12).map(|i| i * 10).collect::<Vec<_>>());
        assert!(peak.load(Ordering::SeqCst) <= test_par());
    }

    #[test]
    fn entries_round_trip_and_reject_corruption() {
        let dir = std::env::temp_dir().join(format!("puoc-test-{}", std::process::id()));
        let key = KeyBuilder::new("t").part("a").finish();
        let p = key.path(&dir);
        write_entry(&p, b"hello");
        assert_eq!(read_entry(&p).as_deref(), Some(&b"hello"[..]));
        let mut raw = std::fs::read(&p).unwrap();
        let last = raw.len() - 1;
        raw[last] ^= 0xff;
        std::fs::write(&p, raw).unwrap();
        assert_eq!(read_entry(&p), None);
        let _ = std::fs::remove_dir_all(&dir);
    }

    #[test]
    fn unordered_text_ignores_attribute_and_line_order_only() {
        let a = "<A x=\"1\" y=\"2\"/>\n<B z=\"3\"/>";
        let b = "<B z=\"3\"/>\n<A y=\"2\" x=\"1\"/>";
        assert!(same_text_unordered(a, b));
        assert!(!same_text_unordered(
            a,
            "<A x=\"1\" y=\"3\"/>\n<B z=\"3\"/>"
        ));
        assert!(!same_text_unordered(a, "<A x=\"1\" y=\"2\"/>"));
    }

    /// TP4: a fresh scratch directory holding `files`.
    fn scratch_tree(tag: &str, files: &[(&str, &str)]) -> PathBuf {
        static N: AtomicUsize = AtomicUsize::new(0);
        let d = std::env::temp_dir().join(format!(
            "puoc-{tag}-{}-{}",
            std::process::id(),
            N.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&d).unwrap();
        for (rel, text) in files {
            let p = d.join(rel);
            std::fs::create_dir_all(p.parent().unwrap()).unwrap();
            std::fs::write(p, text).unwrap();
        }
        std::fs::canonicalize(d).unwrap()
    }

    /// The PoB tree key and the fixture hash of `ui_key` depend on content and relative names
    /// only: two copies at different paths, written at different times, key the same; an edit or
    /// a rename does not; PoB's jewel `.bin` caches do not count.
    #[test]
    fn keys_are_content_addressed() {
        let index = scratch_tree("index", &[]);
        let files = [
            ("Modules/A.lua", "return 1"),
            ("Data/TimelessJewelData/X.zip", "zip"),
        ];
        let a = scratch_tree("pa", &files);
        std::thread::sleep(std::time::Duration::from_millis(20));
        let b = scratch_tree("pb", &files);
        std::fs::write(b.join("Data/TimelessJewelData/X.bin"), "inflated").unwrap();
        let ka = tree_content_key(&a, &index).unwrap();
        assert_eq!(
            ka,
            tree_content_key(&b, &index).unwrap(),
            "same content, other path/mtime"
        );
        assert_eq!(
            ka,
            tree_content_key(&a, &index).unwrap(),
            "from the stat index"
        );
        let c = scratch_tree("pc", &[("Modules/A.lua", "return 2"), files[1]]);
        assert_ne!(ka, tree_content_key(&c, &index).unwrap(), "content edit");
        let d = scratch_tree("pd", &[("Modules/B.lua", "return 1"), files[1]]);
        assert_ne!(ka, tree_content_key(&d, &index).unwrap(), "rename");
        // fixture hashes: relative names and content only
        assert_eq!(
            tree_hash(&a, &[""]),
            tree_hash(&c.join("../").join(a.file_name().unwrap()), &[""])
        );
        assert_eq!(
            tree_hash_except(&b, &[""], &["Data/TimelessJewelData/X.bin"]),
            tree_hash(&a, &[""])
        );
        assert_ne!(tree_hash(&a, &[""]), tree_hash(&c, &[""]));
        for p in [&a, &b, &c, &d, &index] {
            let _ = std::fs::remove_dir_all(p);
        }
    }

    /// A file rewritten with the same size and mtime right after its content was hashed (inside
    /// the racy window) is read again, not taken from the stat index.
    #[test]
    fn stat_index_rehashes_racy_files() {
        let index = scratch_tree("index", &[]);
        let a = scratch_tree("racy", &[("A.lua", "return 1")]);
        let f = a.join("A.lua");
        let mtime = std::fs::metadata(&f).unwrap().modified().unwrap();
        let k1 = tree_content_key(&a, &index).unwrap();
        std::fs::write(&f, "return 2").unwrap();
        std::fs::File::options()
            .write(true)
            .open(&f)
            .unwrap()
            .set_modified(mtime)
            .unwrap();
        assert_ne!(k1, tree_content_key(&a, &index).unwrap());
        for p in [&a, &index] {
            let _ = std::fs::remove_dir_all(p);
        }
    }

    #[test]
    fn third_party_lock_keeps_registry_packages_only() {
        let lock = "version = 4\n\n[[package]]\nname = \"pob-app\"\nversion = \"0.0.1\"\ndependencies = [\n \"mlua\",\n]\n\n[[package]]\nname = \"mlua\"\nversion = \"0.10.0\"\nsource = \"registry+https://github.com/rust-lang/crates.io-index\"\nchecksum = \"ab\"\n";
        let t = third_party_lock(lock);
        assert!(
            t.contains("name = \"mlua\"") && !t.contains("pob-app"),
            "{t}"
        );
        let edited = lock.replace(" \"mlua\",\n", " \"mlua\",\n \"serde\",\n");
        assert_eq!(t, third_party_lock(&edited), "a workspace dependency edit");
        assert_ne!(t, third_party_lock(&lock.replace("0.10.0", "0.10.1")));
    }

    #[test]
    fn key_parts_are_length_prefixed() {
        let a = KeyBuilder::new("t").part("ab").part("c").finish();
        let b = KeyBuilder::new("t").part("a").part("bc").finish();
        assert_ne!(a, b);
        assert_ne!(
            KeyBuilder::new("t").part("x").finish(),
            KeyBuilder::new("u").part("x").finish()
        );
    }
}
