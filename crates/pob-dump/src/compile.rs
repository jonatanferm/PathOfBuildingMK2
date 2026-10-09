//! `pob-dump --compile` (calc doc §9.1 step 4, unit U0): turn the JSON dump
//! into the runtime binary.
//!
//! 1. Load every dump file into a [`DumpTree`] and deserialise
//!    [`pob_gamedata::Data`] from it. Every schema struct is
//!    `deny_unknown_fields`, and every dump root must be typed or listed as
//!    dynamic, so a successful compile means **zero unknown keys**.
//! 2. Write `data.bin` (postcard of the tree) and `trees/<ver>.bin` (one per
//!    tree version, loaded lazily through `pob_gamedata::tree_bytes`).
//! 3. Reload `data.bin` exactly as the runtime does and time it.

use std::path::{Path, PathBuf};
use std::time::Instant;

use anyhow::{Context, Result};
use pob_gamedata::loader::{self, DumpTree};
use pob_gamedata::Data;

/// What a compile produced.
#[derive(Debug, Clone, Default)]
pub struct CompileReport {
    pub data_bin: PathBuf,
    pub data_bin_bytes: u64,
    pub roots: usize,
    pub typed_roots: usize,
    pub dynamic_roots: usize,
    pub ref_targets: usize,
    pub trees: Vec<(String, u64)>,
    pub skills: usize,
    pub gems: usize,
    pub mod_cache: usize,
    pub config_options: usize,
    /// Seconds: JSON load + typed deserialise, encode, runtime reload.
    pub t_json: f64,
    pub t_typed: f64,
    pub t_reload: f64,
}

/// Compile `dump_dir` (`data/dump`) into `out_dir` (normally the same
/// directory): `data.bin` and `trees/<ver>.bin`.
pub fn compile(dump_dir: &Path, out_dir: &Path) -> Result<CompileReport> {
    let mut r = CompileReport::default();
    let t0 = Instant::now();
    let tree: DumpTree = pob_gamedata::load_dump_dir(dump_dir)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("load dump")?;
    r.t_json = t0.elapsed().as_secs_f64();
    let t1 = Instant::now();
    let data = Data::from_tree(&tree)
        .map_err(|e| anyhow::anyhow!("{e}"))
        .context("typed schema (unknown key or unmodelled root)")?;
    r.t_typed = t1.elapsed().as_secs_f64();
    r.roots = tree.roots.len();
    r.dynamic_roots = data.other.len();
    r.typed_roots = r.roots - r.dynamic_roots - data.statDescriptions.0.len();
    r.ref_targets = tree.index.len();
    r.skills = data.skills.len();
    r.gems = data.gems.len();
    r.mod_cache = data.modCache.len();
    r.config_options = data.configOptions.len();
    drop(data);

    std::fs::create_dir_all(out_dir).with_context(|| format!("create {}", out_dir.display()))?;
    let bytes = loader::encode_bin(&tree).map_err(|e| anyhow::anyhow!("{e}"))?;
    r.data_bin = out_dir.join("data.bin");
    write_atomic(&r.data_bin, &bytes)?;
    r.data_bin_bytes = bytes.len() as u64;
    drop(tree);

    // trees: one file per version
    let tdir = out_dir.join("trees");
    std::fs::create_dir_all(&tdir)?;
    for (ver, root, path) in
        pob_gamedata::data::dump_tree_files(dump_dir).map_err(|e| anyhow::anyhow!("{e}"))?
    {
        let text =
            std::fs::read_to_string(&path).with_context(|| format!("read {}", path.display()))?;
        let v =
            loader::parse_json(&text).map_err(|e| anyhow::anyhow!("{}: {e}", path.display()))?;
        let mut t = DumpTree::default();
        t.add_file(&[root], v).map_err(|e| anyhow::anyhow!("{e}"))?;
        t.build_index()
            .map_err(|e| anyhow::anyhow!("tree {ver}: {e}"))?;
        let b = loader::encode_bin(&t).map_err(|e| anyhow::anyhow!("{e}"))?;
        write_atomic(&tdir.join(format!("{ver}.bin")), &b)?;
        r.trees.push((ver, b.len() as u64));
    }

    // reload as the runtime does
    let t2 = Instant::now();
    let bytes = std::fs::read(&r.data_bin)?;
    let tree = loader::decode_bin(&bytes).map_err(|e| anyhow::anyhow!("{e}"))?;
    let data = Data::from_tree(&tree).map_err(|e| anyhow::anyhow!("reload: {e}"))?;
    r.t_reload = t2.elapsed().as_secs_f64();
    anyhow::ensure!(
        data.skills.len() == r.skills && data.gems.len() == r.gems,
        "reload differs"
    );
    Ok(r)
}

fn write_atomic(path: &Path, bytes: &[u8]) -> Result<()> {
    let tmp = path.with_extension("bin.tmp");
    std::fs::write(&tmp, bytes).with_context(|| format!("write {}", tmp.display()))?;
    std::fs::rename(&tmp, path).with_context(|| format!("rename to {}", path.display()))?;
    Ok(())
}

impl CompileReport {
    pub fn print(&self) {
        println!(
            "pob-dump --compile: {} ({:.1} MB), zero unknown keys",
            self.data_bin.display(),
            self.data_bin_bytes as f64 / 1e6
        );
        println!(
            "  roots: {} ({} typed, {} dynamic, {} stat-description scopes); ref targets: {}",
            self.roots,
            self.typed_roots,
            self.dynamic_roots,
            self.roots - self.typed_roots - self.dynamic_roots,
            self.ref_targets
        );
        println!(
            "  skills {}, gems {}, ModCache lines {}, config options {}",
            self.skills, self.gems, self.mod_cache, self.config_options
        );
        println!("  trees: {} versions", self.trees.len());
        println!(
            "  time: json load {:.2}s, typed {:.2}s, runtime reload of data.bin {:.3}s",
            self.t_json, self.t_typed, self.t_reload
        );
    }
}
