//! `pob-vendor`: write the asset bundle (`assets/`, VENDOR) from a PoB checkout and the data dump.
//!
//! ```text
//! pob-vendor --pob <.PathOfBuilding> --dump <data/dump> --out <assets>
//!            [--inventory tools/pob-vendor/inventory.txt]
//! ```
//!
//! This is the one tool that reads PoB; it runs once per snapshot while a checkout exists, and
//! the shipped apps only ever read what it wrote. It writes, under `--out`:
//!
//! * the inventory's files (`inventory.txt`: every file the app reads, by logical path) in the
//!   bundle's own layout: `ui/` (PoB's `src/Assets/`), `tree/` (`src/TreeData/`), `timeless/`
//!   (`src/Data/TimelessJewelData/`), `text/` (the release `changelog.txt`, `help.txt`) and
//!   `fonts/` (`runtime/SimpleGraphic/Fonts/`). Identical files are stored once: the first
//!   logical path (in sorted order) that has the content places it, the others point to it;
//! * `gamedata/`: the dump's compiled `data.bin` and `trees/<ver>.bin`, plus the tree UI files
//!   (`trees/<ver>.sprites.json`, `trees/3_19.assets.json`) compiled the same way
//!   (`pob_gamedata::data::compile_tree_json`);
//! * `text/manifest.xml`, the logical `manifest.xml` (written, not copied): a release-style PoB
//!   manifest, `<Version number="<pobVersion>" branch="master" platform="win32"/>`, which
//!   `launch:OnInit` (Launch.lua:46-63) reads for `launch.versionNumber` (the HTTP user agent, the
//!   build-site and archive requests, the error prompt). PoB's checkout manifest is not copied:
//!   it has no branch or platform, which makes Launch.lua:59-62 turn on dev mode, and dev mode
//!   puts the user data in the script path (Main.lua:89-91), the read-only bundle. With branch
//!   and platform set the app runs as an installed release (dev mode off, user data in the
//!   user's data directory). Nothing reads `branch` and `platform` but the update check, which
//!   the port does not have (DIVERGENCE(update-deferred)). DIVERGENCE(manifest-platform):
//!   `platform="win32"` on every OS, the value of PoB's own (Windows-only) release manifests;
//!   only that absent update check would read it. The bundle is read-only to the app
//!   (`pob_host::bundle::Bundle::contains`), so Options > Save (`main:SetManifestBranch`) cannot
//!   rewrite this file;
//! * `licenses/PathOfBuilding-LICENSE.md` (PoB's `LICENSE.md`);
//! * `index.json`: format 1, the snapshot (`source`: PoB version and commit from the dump's
//!   manifest), the logical → bundle maps (`script`, `runtime`; `pob_host::bundle`) and every
//!   written file with its size and sha256 (`files`);
//! * `web-prefetch.txt`: what the web host fetches before `OnInit` (bundle paths): `ui/`, the
//!   top-level and legion tree images, the latest tree version's images and `text/` (the manifest
//!   `launch:OnInit` reads, the About popup's changelog and help).
//!
//! The hand-written `README.md` and `THIRD_PARTY_NOTICES.md` are left as they are; everything
//! else the tool owns is replaced.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use anyhow::{bail, ensure, Context, Result};
use sha2::{Digest, Sha256};

/// What the tool owns under `--out` (removed before writing).
const OWNED: [&str; 9] = [
    "ui",
    "tree",
    "timeless",
    "text",
    "fonts",
    "gamedata",
    "licenses",
    "index.json",
    "web-prefetch.txt",
];

struct Entry {
    kind: Kind,
    logical: String,
    source: PathBuf,
}

#[derive(Clone, Copy, PartialEq, Eq, PartialOrd, Ord)]
enum Kind {
    Script,
    Runtime,
}

/// The bundle path of a logical path (the layout, see the module docs).
fn place(kind: Kind, logical: &str) -> Result<String> {
    let rules: &[(&str, &str)] = match kind {
        Kind::Script => &[
            ("Assets/", "ui/"),
            ("TreeData/", "tree/"),
            ("Data/TimelessJewelData/", "timeless/"),
            ("changelog.txt", "text/changelog.txt"),
            ("help.txt", "text/help.txt"),
        ],
        Kind::Runtime => &[("Fonts/", "fonts/")],
    };
    for (from, to) in rules {
        if let Some(rest) = logical.strip_prefix(from) {
            return Ok(format!("{to}{rest}"));
        }
    }
    bail!("no bundle place for {logical}")
}

/// The logical path of the manifest `launch:OnInit` reads (Launch.lua:46).
const MANIFEST_LOGICAL: &str = "manifest.xml";
/// Where the bundle holds it.
const MANIFEST_FILE: &str = "text/manifest.xml";

/// A release-style PoB manifest (module docs): what PoB's `UpdateCheck.lua:291` writes for an
/// installed release, without the `Source`/`File` entries only the update check reads (and
/// without an XML comment, which PoB's `xml.lua` would not take for the root's first node).
fn release_manifest(pob_version: &str) -> String {
    format!(
        "<?xml version='1.0' encoding='UTF-8'?>\n\
         <PoBVersion>\n\
         \t<Version number=\"{pob_version}\" branch=\"master\" platform=\"win32\" />\n\
         </PoBVersion>\n"
    )
}

fn read_inventory(path: &Path, pob: &Path) -> Result<Vec<Entry>> {
    let text = std::fs::read_to_string(path).with_context(|| path.display().to_string())?;
    let mut out = Vec::new();
    for (n, line) in text.lines().enumerate() {
        if line.trim().is_empty() || line.starts_with('#') {
            continue;
        }
        let cols: Vec<&str> = line.split('\t').collect();
        let (kind, base) = match cols[0] {
            "script" => (Kind::Script, pob.join("src")),
            "runtime" => (Kind::Runtime, pob.join("runtime/SimpleGraphic")),
            k => bail!("{}:{}: unknown kind {k}", path.display(), n + 1),
        };
        let logical = cols
            .get(1)
            .with_context(|| format!("{}:{}: no path", path.display(), n + 1))?;
        let source = match cols.get(2) {
            Some(s) => pob.join(s),
            None => base.join(logical),
        };
        out.push(Entry {
            kind,
            logical: (*logical).to_owned(),
            source,
        });
    }
    out.sort_by(|a, b| (a.kind, &a.logical).cmp(&(b.kind, &b.logical)));
    Ok(out)
}

fn hex(d: &[u8]) -> String {
    use std::fmt::Write as _;
    d.iter().fold(String::new(), |mut s, b| {
        let _ = write!(s, "{b:02x}");
        s
    })
}

#[derive(Default)]
struct Out {
    root: PathBuf,
    /// bundle path → (bytes, sha256)
    files: BTreeMap<String, (u64, String)>,
}

impl Out {
    fn write(&mut self, rel: &str, bytes: &[u8]) -> Result<String> {
        let sha = hex(&Sha256::digest(bytes));
        if let Some((_, old)) = self.files.get(rel) {
            ensure!(*old == sha, "{rel} written twice with different contents");
            return Ok(sha);
        }
        let p = self.root.join(rel);
        if let Some(d) = p.parent() {
            std::fs::create_dir_all(d)?;
        }
        std::fs::write(&p, bytes).with_context(|| p.display().to_string())?;
        self.files
            .insert(rel.to_owned(), (bytes.len() as u64, sha.clone()));
        Ok(sha)
    }
}

fn arg(args: &[String], name: &str) -> Option<String> {
    args.iter()
        .position(|a| a == name)
        .and_then(|i| args.get(i + 1))
        .cloned()
}

fn main() -> Result<()> {
    let args: Vec<String> = std::env::args().collect();
    let need = |n: &str| arg(&args, n).with_context(|| format!("missing {n} (see the docs)"));
    let pob = PathBuf::from(need("--pob")?);
    let dump = PathBuf::from(need("--dump")?);
    let out_dir = PathBuf::from(need("--out")?);
    let inventory = arg(&args, "--inventory").map_or_else(
        || PathBuf::from(env!("CARGO_MANIFEST_DIR")).join("inventory.txt"),
        PathBuf::from,
    );

    let manifest: serde_json::Value = serde_json::from_str(
        &std::fs::read_to_string(dump.join("manifest.json")).context("dump manifest.json")?,
    )?;
    let pob_version = manifest["pobVersion"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    let pob_commit = manifest["pobCommit"]
        .as_str()
        .unwrap_or_default()
        .to_owned();
    ensure!(!pob_commit.is_empty(), "dump manifest without pobCommit");

    let entries = read_inventory(&inventory, &pob)?;
    std::fs::create_dir_all(&out_dir)?;
    for o in OWNED {
        let p = out_dir.join(o);
        if p.is_dir() {
            std::fs::remove_dir_all(&p)?;
        } else if p.exists() {
            std::fs::remove_file(&p)?;
        }
    }
    let mut out = Out {
        root: out_dir.clone(),
        ..Out::default()
    };

    // the inventory, deduplicated by content
    let mut by_sha: BTreeMap<String, String> = BTreeMap::new();
    let mut script: BTreeMap<String, String> = BTreeMap::new();
    let mut runtime: BTreeMap<String, String> = BTreeMap::new();
    let (mut logical_bytes, mut shared) = (0u64, 0usize);
    for e in &entries {
        let bytes = std::fs::read(&e.source).with_context(|| e.source.display().to_string())?;
        logical_bytes += bytes.len() as u64;
        let sha = hex(&Sha256::digest(&bytes));
        let file = if let Some(f) = by_sha.get(&sha) {
            shared += 1;
            f.clone()
        } else {
            let f = place(e.kind, &e.logical)?;
            out.write(&f, &bytes)?;
            by_sha.insert(sha, f.clone());
            f
        };
        match e.kind {
            Kind::Script => script.insert(e.logical.clone(), file),
            Kind::Runtime => runtime.insert(e.logical.clone(), file),
        };
    }

    // the game data
    let data_bin = std::fs::read(dump.join("data.bin")).context("dump data.bin")?;
    out.write("gamedata/data.bin", &data_bin)?;
    let mut tree_names = Vec::new();
    for e in std::fs::read_dir(dump.join("trees"))?.flatten() {
        tree_names.push(e.file_name().to_string_lossy().into_owned());
    }
    tree_names.sort();
    for n in &tree_names {
        let src = dump.join("trees").join(n);
        if Path::new(n).extension().is_some_and(|e| e == "bin") {
            out.write(&format!("gamedata/trees/{n}"), &std::fs::read(&src)?)?;
        } else if let Some(name) = n
            .strip_suffix(".json")
            .filter(|s| s.ends_with(".sprites") || s.ends_with(".assets"))
        {
            let bytes =
                pob_gamedata::data::compile_tree_json(name, &std::fs::read_to_string(&src)?)
                    .map_err(|e| anyhow::anyhow!("{n}: {e}"))?;
            out.write(&format!("gamedata/trees/{name}.bin"), &bytes)?;
        }
    }
    // the release-style manifest (module docs)
    ensure!(!pob_version.is_empty(), "dump manifest without pobVersion");
    ensure!(
        !script.contains_key(MANIFEST_LOGICAL),
        "the inventory must not list {MANIFEST_LOGICAL}: pob-vendor writes it"
    );
    out.write(MANIFEST_FILE, release_manifest(&pob_version).as_bytes())?;
    script.insert(MANIFEST_LOGICAL.to_owned(), MANIFEST_FILE.to_owned());

    out.write(
        "licenses/PathOfBuilding-LICENSE.md",
        &std::fs::read(pob.join("LICENSE.md")).context("PoB LICENSE.md")?,
    )?;

    // the web prefetch list
    pob_gamedata::init(&data_bin).map_err(|e| anyhow::anyhow!("data.bin: {e}"))?;
    let latest = pob_gamedata::data()
        .gameVersions
        .latestTreeVersion
        .to_string();
    let mut prefetch: Vec<&str> = script
        .iter()
        .filter(|(l, f)| {
            f.starts_with("text/")
                || l.starts_with("Assets/")
                || l.starts_with("TreeData/legion/")
                || l.starts_with(&format!("TreeData/{latest}/"))
                || (l.starts_with("TreeData/") && l.matches('/').count() == 1)
        })
        .map(|(_, f)| f.as_str())
        .collect();
    prefetch.sort_unstable();
    prefetch.dedup();
    let mut list = format!(
        "# The files the web host fetches before OnInit (VENDOR, pob_host::web): bundle paths.\n\
         # ui/, the top-level and legion tree images, tree version {latest}, text/. Written by pob-vendor.\n"
    );
    for f in &prefetch {
        list.push_str(f);
        list.push('\n');
    }
    out.write("web-prefetch.txt", list.as_bytes())?;

    // the index (keys sorted)
    let obj = |m: &BTreeMap<String, String>| {
        serde_json::Value::Object(
            m.iter()
                .map(|(k, v)| (k.clone(), serde_json::Value::String(v.clone())))
                .collect(),
        )
    };
    let files = serde_json::Value::Object(
        out.files
            .iter()
            .map(|(k, (b, s))| (k.clone(), serde_json::json!({ "bytes": b, "sha256": s })))
            .collect(),
    );
    let index = serde_json::json!({
        "format": 1,
        "source": {
            "pobVersion": pob_version,
            "pobCommit": pob_commit,
            "tool": "tools/pob-vendor (VENDOR)",
        },
        "script": obj(&script),
        "runtime": obj(&runtime),
        "files": files,
    });
    let text = serde_json::to_string_pretty(&index)? + "\n";
    std::fs::write(out_dir.join("index.json"), text)?;

    let total: u64 = out.files.values().map(|(b, _)| b).sum();
    println!(
        "pob-vendor: {} logical files ({:.1} MB) in {} bundle files, {shared} shared; {} files written, {:.1} MB; latest tree {latest}",
        entries.len(),
        logical_bytes as f64 / 1e6,
        by_sha.len(),
        out.files.len(),
        total as f64 / 1e6
    );
    Ok(())
}
