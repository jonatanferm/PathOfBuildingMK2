//! Native-only loading of the extracted `data/` directory.
//!
//! One place that knows every file the calc needs, so the desktop app and the
//! `pob_diff` parity harness feed the engine identical inputs. Missing or
//! unparsable optional files load as `None` (the calc degrades gracefully),
//! matching the app's historical behaviour.

use std::path::{Path, PathBuf};

use crate::{
    bases::ItemBaseSet, cluster_jewel_mods::ClusterModSet, ClusterJewelData, LegionPool,
    MinionData, PassiveTree, SkillSet, TimelessJewelData, TimelessNodeIndex,
};

/// Everything under `data/` that the calc engine consumes. Passive trees are
/// loaded separately per version via [`load_tree`].
#[derive(Default)]
pub struct GameData {
    pub root: PathBuf,
    pub skill_sets: Vec<SkillSet>,
    pub bases: Option<ItemBaseSet>,
    pub cluster_jewels: Option<ClusterJewelData>,
    pub cluster_jewel_mods: Option<ClusterModSet>,
    pub timeless_jewels: Option<TimelessJewelData>,
    pub timeless_node_index: Option<TimelessNodeIndex>,
    pub legion_pool: Option<LegionPool>,
    pub minions: Option<MinionData>,
}

fn read(root: &Path, name: &str) -> Option<String> {
    std::fs::read_to_string(root.join(name)).ok()
}

impl GameData {
    /// Load every calc input from `root` (normally `data/`).
    #[must_use]
    pub fn load(root: &Path) -> Self {
        let mut skill_sets = Vec::new();
        if let Ok(entries) = std::fs::read_dir(root.join("skills")) {
            let mut paths: Vec<PathBuf> = entries.flatten().map(|e| e.path()).collect();
            paths.sort();
            for p in paths {
                if p.extension().and_then(|e| e.to_str()) != Some("json")
                    || p.file_stem().and_then(|s| s.to_str()) == Some("index")
                {
                    continue;
                }
                if let Some(set) = std::fs::read_to_string(&p)
                    .ok()
                    .and_then(|j| crate::load_skill_file(&j).ok())
                {
                    skill_sets.push(set);
                }
            }
        }
        let legion_pool = (|| {
            Some(LegionPool {
                additions: crate::load_legion_passives(&read(root, "legion_passives.json")?)
                    .ok()?,
                nodes: crate::load_legion_nodes(&read(root, "legion_nodes.json")?).ok()?,
            })
        })();
        Self {
            root: root.to_path_buf(),
            skill_sets,
            bases: read(root, "bases.json").and_then(|j| crate::load_bases(&j).ok()),
            cluster_jewels: read(root, "cluster_jewels.json")
                .and_then(|j| crate::load_cluster_jewels(&j).ok()),
            cluster_jewel_mods: read(root, "cluster_jewel_mods.json")
                .and_then(|j| crate::load_cluster_jewel_mods(&j).ok()),
            timeless_jewels: read(root, "timeless_jewels.json")
                .and_then(|j| crate::load_timeless_jewels(&j).ok()),
            timeless_node_index: read(root, "timeless_node_index.json")
                .and_then(|j| crate::load_timeless_node_index(&j).ok()),
            legion_pool,
            minions: read(root, "minions.json").and_then(|j| crate::load_minions(&j).ok()),
        }
    }

    /// Load `data/trees/<version>.json`.
    pub fn load_tree(&self, version: &str) -> Result<PassiveTree, String> {
        load_tree(&self.root, version)
    }

    /// Compressed bytes of the timeless LUT for a 1-based jewel type (see
    /// [`crate::jewel_type_for_base`]); split `.zip.partN` files are
    /// concatenated. Inflate with `pob_engine::inflate_timeless_lut`.
    #[must_use]
    pub fn timeless_lut_compressed(&self, jewel_type: u32) -> Option<Vec<u8>> {
        let stem = timeless_lut_stem(jewel_type)?;
        let dir = self.root.join("timeless");
        if let Ok(bytes) = std::fs::read(dir.join(format!("{stem}.zip"))) {
            return Some(bytes);
        }
        let mut buf = Vec::new();
        for part in 0.. {
            match std::fs::read(dir.join(format!("{stem}.zip.part{part}"))) {
                Ok(bytes) => buf.extend_from_slice(&bytes),
                Err(_) => break,
            }
        }
        (!buf.is_empty()).then_some(buf)
    }
}

/// Load `<root>/trees/<version>.json`.
pub fn load_tree(root: &Path, version: &str) -> Result<PassiveTree, String> {
    let path = root.join("trees").join(format!("{version}.json"));
    let json = std::fs::read_to_string(&path).map_err(|e| format!("reading {path:?}: {e}"))?;
    crate::load_passive_tree(&json).map_err(|e| format!("parsing {path:?}: {e}"))
}

/// `data/timeless/<stem>` filename stem for a 1-based jewel type, in PoB's
/// `data.timelessJewelTypes` order.
#[must_use]
pub fn timeless_lut_stem(jewel_type: u32) -> Option<&'static str> {
    Some(match jewel_type {
        1 => "GloriousVanity",
        2 => "LethalPride",
        3 => "BrutalRestraint",
        4 => "MilitantFaith",
        5 => "ElegantHubris",
        6 => "HeroicTragedy",
        7 => "AbyssTecrod",
        8 => "AbyssUlaman",
        9 => "AbyssKurgal",
        10 => "AbyssAmanamu",
        11 => "AbyssZorath",
        _ => return None,
    })
}
