//! Issue #213 (slice 1): pure data layer for the Builds tab folder
//! browser.
//!
//! Mirrors PoB's `Classes/FolderListControl.lua` — builds can live in
//! nested folders, and the UI renders an expand/collapse tree.
//! `crates/pob-ui/src/builds_tab.rs` previously showed a single-level
//! grouping by [`BuildEntry::category`]; this module turns that flat
//! category string (which may contain `/`-separated path segments,
//! mirroring on-disk directory nesting) into a hierarchical
//! [`FolderNode`] tree.
//!
//! Pure helper only: no egui, no filesystem, deterministically sorted
//! output so the renderer (next slice) can diff frames without
//! reshuffling. The renderer is intentionally out of scope here — see
//! issue #213 follow-ups.
//!
//! Ordering rules (matches `build_store_disk::rescan`):
//! * Within each level, subfolders sort before builds (PoB convention).
//! * Folder names sort case-insensitively, ASCII fold.
//! * Build labels sort case-insensitively within their folder.
//! * Empty / whitespace-only path segments are stripped — defensive
//!   against malformed `category` strings.
//!
//! Slice 2 (issue #213): the renderer in `builds_tab` consumes
//! [`build_folder_tree`] + [`folder_path_key`] to drive a recursive
//! `egui::CollapsingHeader` view with persistent expand/collapse
//! state.

use crate::builds_tab::BuildEntry;

/// Node in the builds folder hierarchy. The root node carries an
/// empty `name`; all other nodes name a directory segment.
#[derive(Debug, Clone, Default)]
pub struct FolderNode {
    /// Directory segment for this node. Empty for the root.
    pub name: String,
    /// Subfolders, sorted case-insensitively by `name`.
    pub children: Vec<FolderNode>,
    /// Builds that live directly in this folder, sorted
    /// case-insensitively by `label`.
    pub builds: Vec<BuildEntry>,
}

/// Issue #213 follow-up: how the builds list orders within each folder.
/// `Name` is the historical alphabetical order; `RecentFirst` orders by
/// filesystem mtime descending (entries without an mtime sort to the
/// bottom and fall back to label order so the result is deterministic).
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum BuildsSortMode {
    /// Case-insensitive alphabetical by label. Default — preserves the
    /// pre-issue-#213 ordering.
    #[default]
    Name,
    /// Most-recently-modified first. Entries without a known mtime
    /// fall to the bottom of their folder, ordered alphabetically
    /// among themselves so the tail is stable.
    RecentFirst,
}

impl BuildsSortMode {
    /// Human-readable label for the UI selector.
    #[must_use]
    pub fn label(self) -> &'static str {
        match self {
            Self::Name => "Name",
            Self::RecentFirst => "Recent",
        }
    }
}

/// Group a flat list of [`BuildEntry`]s into a folder tree, sorted
/// alphabetically by label within each folder. Convenience wrapper for
/// the historical default — new callers should prefer
/// [`build_folder_tree_sorted`] so the sort mode is explicit.
///
/// Each entry's [`BuildEntry::category`] is interpreted as a
/// `/`-separated path. `None` (or an empty / whitespace-only category)
/// places the build at the root.
#[must_use]
pub fn build_folder_tree(builds: &[BuildEntry]) -> FolderNode {
    build_folder_tree_sorted(builds, BuildsSortMode::Name)
}

/// Group a flat list of [`BuildEntry`]s into a folder tree, ordering
/// each folder's builds by `mode`.
#[must_use]
pub fn build_folder_tree_sorted(builds: &[BuildEntry], mode: BuildsSortMode) -> FolderNode {
    let mut root = FolderNode::default();
    for entry in builds {
        let segments = split_category(entry.category.as_deref());
        insert_build(&mut root, &segments, entry.clone());
    }
    sort_node(&mut root, mode);
    root
}

fn split_category(category: Option<&str>) -> Vec<String> {
    let Some(raw) = category else {
        return Vec::new();
    };
    raw.split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(str::to_owned)
        .collect()
}

fn insert_build(node: &mut FolderNode, segments: &[String], entry: BuildEntry) {
    let Some((head, tail)) = segments.split_first() else {
        node.builds.push(entry);
        return;
    };
    if let Some(idx) = node.children.iter().position(|c| c.name == *head) {
        insert_build(&mut node.children[idx], tail, entry);
    } else {
        let mut child = FolderNode {
            name: head.clone(),
            ..FolderNode::default()
        };
        insert_build(&mut child, tail, entry);
        node.children.push(child);
    }
}

/// Stable key for a folder's position in the tree, suitable for use
/// as a `HashMap` key in expand/collapse state and as an egui id_salt.
///
/// Built from the path of folder names from the root down to this
/// node. The root itself maps to `""` (empty key). Joined with `/`
/// because folder names from [`split_category`] are already
/// `/`-stripped — there is no escaping concern.
///
/// Slice 2 (issue #213) callers persist these keys on
/// [`crate::builds_tab::BuildsTabState::expanded`] so collapse state
/// survives tab switches and refresh cycles.
#[must_use]
pub fn folder_path_key(path: &[&str]) -> String {
    path.join("/")
}

/// Issue #213 (folder-isolation slice): clone the subtree at
/// `selected_path` out of `root`, or return `None` when the path
/// doesn't resolve to a folder. Pure / no egui — the renderer uses
/// this to drop every sibling branch when the user picks
/// "Show only this folder".
///
/// `selected_path` is the slash-joined key produced by
/// [`folder_path_key`]. An empty string means "the root" and yields
/// the whole tree unchanged (the renderer treats this as
/// "no filter active" and bypasses the helper). Whitespace / empty
/// segments inside `selected_path` are stripped so a hand-built key
/// loosely.
#[must_use]
pub fn filter_folder_to_subtree(root: &FolderNode, selected_path: &str) -> Option<FolderNode> {
    let segments: Vec<&str> = selected_path
        .split('/')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .collect();
    if segments.is_empty() {
        return Some(root.clone());
    }
    let mut node = root;
    for seg in &segments {
        node = node.children.iter().find(|c| c.name == *seg)?;
    }
    Some(node.clone())
}

fn sort_node(node: &mut FolderNode, mode: BuildsSortMode) {
    // Folders always sort alphabetically — the mode only affects the
    // build leaves. (PoB doesn't expose a per-folder mtime, and a
    // shuffling folder list reads poorly when the user expects a
    // stable layout.)
    node.children.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    match mode {
        BuildsSortMode::Name => {
            node.builds.sort_by(|a, b| {
                a.label
                    .to_ascii_lowercase()
                    .cmp(&b.label.to_ascii_lowercase())
            });
        }
        BuildsSortMode::RecentFirst => {
            // Sort by mtime descending. Entries without an mtime sink
            // to the bottom; ties (including all-None) fall back to
            // alphabetical so the order is deterministic.
            node.builds.sort_by(|a, b| match (a.modified, b.modified) {
                (Some(am), Some(bm)) => bm.cmp(&am).then_with(|| {
                    a.label
                        .to_ascii_lowercase()
                        .cmp(&b.label.to_ascii_lowercase())
                }),
                (Some(_), None) => std::cmp::Ordering::Less,
                (None, Some(_)) => std::cmp::Ordering::Greater,
                (None, None) => a
                    .label
                    .to_ascii_lowercase()
                    .cmp(&b.label.to_ascii_lowercase()),
            });
        }
    }
    for child in &mut node.children {
        sort_node(child, mode);
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::builds_tab::BuildId;
    use std::path::PathBuf;

    fn mk(label: &str, category: Option<&str>) -> BuildEntry {
        BuildEntry {
            label: label.to_owned(),
            id: BuildId::Disk(PathBuf::from(format!("/tmp/{label}.mk2"))),
            ext: "mk2".to_owned(),
            category: category.map(str::to_owned),
            modified: None,
        }
    }

    fn mk_at(label: &str, category: Option<&str>, mtime_secs: u64) -> BuildEntry {
        BuildEntry {
            label: label.to_owned(),
            id: BuildId::Disk(PathBuf::from(format!("/tmp/{label}.mk2"))),
            ext: "mk2".to_owned(),
            category: category.map(str::to_owned),
            modified: Some(
                std::time::SystemTime::UNIX_EPOCH + std::time::Duration::from_secs(mtime_secs),
            ),
        }
    }

    #[test]
    fn name_mode_sorts_alphabetically_within_folder() {
        // Historical default — labels in case-insensitive order, mtime
        // is irrelevant.
        let entries = vec![
            mk_at("Gamma", None, 100),
            mk_at("alpha", None, 999),
            mk_at("Beta", None, 1),
        ];
        let tree = build_folder_tree_sorted(&entries, BuildsSortMode::Name);
        let labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["alpha", "Beta", "Gamma"]);
    }

    #[test]
    fn recent_first_mode_orders_newest_at_top() {
        // mtime descending — the freshest build comes first.
        let entries = vec![
            mk_at("Old", None, 100),
            mk_at("New", None, 999),
            mk_at("Middle", None, 500),
        ];
        let tree = build_folder_tree_sorted(&entries, BuildsSortMode::RecentFirst);
        let labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["New", "Middle", "Old"]);
    }

    #[test]
    fn recent_first_mode_sinks_none_mtime_to_bottom() {
        // Entries that lack an mtime (wasm IDB, FSA folder backends)
        // fall to the bottom of their folder so the timed entries
        // dominate the readable top of the list.
        let entries = vec![
            mk("NoMtime1", None),
            mk_at("Old", None, 100),
            mk("NoMtime2", None),
            mk_at("New", None, 999),
        ];
        let tree = build_folder_tree_sorted(&entries, BuildsSortMode::RecentFirst);
        let labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        // Timed entries first (newest → oldest), then no-mtime entries
        // in case-insensitive alphabetical order.
        assert_eq!(labels, vec!["New", "Old", "NoMtime1", "NoMtime2"]);
    }

    #[test]
    fn recent_first_mode_breaks_mtime_ties_with_label() {
        // Two builds touched at exactly the same second — the tail
        // sort falls through to label-alphabetical for determinism.
        let entries = vec![
            mk_at("Zeta", None, 500),
            mk_at("alpha", None, 500),
            mk_at("Mu", None, 500),
        ];
        let tree = build_folder_tree_sorted(&entries, BuildsSortMode::RecentFirst);
        let labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["alpha", "Mu", "Zeta"]);
    }

    #[test]
    fn build_folder_tree_defaults_to_name_mode() {
        // The wrapper preserves the historical default so existing
        // call sites don't drift in behaviour.
        let entries = vec![mk_at("Newer", None, 999), mk_at("Older", None, 100)];
        let tree = build_folder_tree(&entries);
        let labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        // Alphabetical — `Newer` (N) sorts after `Older` (O)... wait,
        // 'N' < 'O' so `Newer` comes first.
        assert_eq!(labels, vec!["Newer", "Older"]);
    }

    #[test]
    fn empty_input_yields_empty_root() {
        let tree = build_folder_tree(&[]);
        assert_eq!(tree.name, "");
        assert!(tree.children.is_empty());
        assert!(tree.builds.is_empty());
    }

    #[test]
    fn single_root_build_lands_in_root() {
        let entries = vec![mk("Solo", None)];
        let tree = build_folder_tree(&entries);
        assert!(tree.children.is_empty());
        assert_eq!(tree.builds.len(), 1);
        assert_eq!(tree.builds[0].label, "Solo");
    }

    #[test]
    fn nested_folder_structure_mirrors_path_segments() {
        let entries = vec![
            mk("PhysRT", Some("Levelling/Marauder")),
            mk("CI", Some("Levelling/Witch")),
            mk("HCDD", Some("Bossing")),
        ];
        let tree = build_folder_tree(&entries);
        assert_eq!(tree.children.len(), 2);
        // Bossing sorts before Levelling.
        assert_eq!(tree.children[0].name, "Bossing");
        assert_eq!(tree.children[0].builds.len(), 1);
        assert_eq!(tree.children[0].builds[0].label, "HCDD");

        let levelling = &tree.children[1];
        assert_eq!(levelling.name, "Levelling");
        assert!(levelling.builds.is_empty());
        assert_eq!(levelling.children.len(), 2);
        assert_eq!(levelling.children[0].name, "Marauder");
        assert_eq!(levelling.children[0].builds[0].label, "PhysRT");
        assert_eq!(levelling.children[1].name, "Witch");
        assert_eq!(levelling.children[1].builds[0].label, "CI");
    }

    #[test]
    fn entries_sort_alphabetically_case_insensitive_within_each_level() {
        let entries = vec![
            mk("Zeta", None),
            mk("alpha", None),
            mk("MIDDLE", None),
            mk("beta", Some("Group")),
            mk("Alpha", Some("Group")),
        ];
        let tree = build_folder_tree(&entries);
        let labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(labels, vec!["alpha", "MIDDLE", "Zeta"]);
        assert_eq!(tree.children.len(), 1);
        let group_labels: Vec<&str> = tree.children[0]
            .builds
            .iter()
            .map(|e| e.label.as_str())
            .collect();
        assert_eq!(group_labels, vec!["Alpha", "beta"]);
    }

    #[test]
    fn mixed_builds_and_folders_coexist_at_same_level() {
        let entries = vec![
            mk("scratch", None),
            mk("notes", None),
            mk("PhysRT", Some("Levelling")),
            mk("HCDD", Some("Bossing")),
        ];
        let tree = build_folder_tree(&entries);
        // Both root-level subfolders present.
        let folder_names: Vec<&str> = tree.children.iter().map(|c| c.name.as_str()).collect();
        assert_eq!(folder_names, vec!["Bossing", "Levelling"]);
        // Both root-level builds present and sorted.
        let root_labels: Vec<&str> = tree.builds.iter().map(|e| e.label.as_str()).collect();
        assert_eq!(root_labels, vec!["notes", "scratch"]);
    }

    #[test]
    fn folder_path_key_root_is_empty() {
        assert_eq!(folder_path_key(&[]), "");
    }

    #[test]
    fn folder_path_key_joins_segments_with_slash() {
        assert_eq!(folder_path_key(&["Levelling"]), "Levelling");
        assert_eq!(
            folder_path_key(&["Levelling", "Marauder"]),
            "Levelling/Marauder"
        );
    }

    #[test]
    fn folder_path_key_distinguishes_distinct_paths() {
        // Two folders with the same leaf name at different depths
        // must produce different keys so expand-state for one doesn't
        // bleed into the other.
        let a = folder_path_key(&["Bossing", "HC"]);
        let b = folder_path_key(&["Levelling", "HC"]);
        let c = folder_path_key(&["HC"]);
        assert_ne!(a, b);
        assert_ne!(a, c);
        assert_ne!(b, c);
    }

    #[test]
    fn filter_folder_to_subtree_returns_whole_tree_for_empty_path() {
        // Empty filter path means "no filter" — the helper short-
        // circuits and returns a clone of the whole tree so the
        // renderer can treat empty as a no-op.
        let entries = vec![
            mk("A", None),
            mk("B", Some("Levelling")),
            mk("C", Some("Bossing")),
        ];
        let tree = build_folder_tree(&entries);
        let filtered = filter_folder_to_subtree(&tree, "").expect("root");
        assert_eq!(filtered.children.len(), 2);
        assert_eq!(filtered.builds.len(), 1);
    }

    #[test]
    fn filter_folder_to_subtree_extracts_subtree_by_path() {
        let entries = vec![
            mk("a", Some("Bossing")),
            mk("b", Some("Bossing/Sirus")),
            mk("c", Some("Levelling")),
        ];
        let tree = build_folder_tree(&entries);
        let bossing = filter_folder_to_subtree(&tree, "Bossing").expect("Bossing");
        // Sibling "Levelling" dropped — only the Bossing subtree
        // survives. The Sirus child + its build come along.
        assert_eq!(bossing.children.len(), 1);
        assert_eq!(bossing.children[0].name, "Sirus");
        assert_eq!(bossing.children[0].builds[0].label, "b");
        // Direct builds on Bossing also survive.
        assert_eq!(bossing.builds.len(), 1);
        assert_eq!(bossing.builds[0].label, "a");
    }

    #[test]
    fn filter_folder_to_subtree_extracts_deeply_nested_subtree() {
        let entries = vec![mk("deep", Some("L1/L2/L3"))];
        let tree = build_folder_tree(&entries);
        let l3 = filter_folder_to_subtree(&tree, "L1/L2/L3").expect("L3");
        assert!(l3.children.is_empty());
        assert_eq!(l3.builds[0].label, "deep");
    }

    #[test]
    fn filter_folder_to_subtree_returns_none_for_unknown_path() {
        let entries = vec![mk("a", Some("Real"))];
        let tree = build_folder_tree(&entries);
        assert!(filter_folder_to_subtree(&tree, "Real/Nope").is_none());
        assert!(filter_folder_to_subtree(&tree, "Phantom").is_none());
    }

    #[test]
    fn filter_folder_to_subtree_strips_empty_segments_in_path() {
        // Defensive: a hand-built key like `"Real//Sub/"` resolves
        // the same as `"Real/Sub"` — the loose-path tolerance lives
        // here so the renderer doesn't have to scrub keys before
        // calling.
        let entries = vec![mk("x", Some("Real/Sub"))];
        let tree = build_folder_tree(&entries);
        let resolved = filter_folder_to_subtree(&tree, "Real//Sub/").expect("Real/Sub");
        assert_eq!(resolved.builds[0].label, "x");
    }

    #[test]
    fn empty_and_whitespace_segments_are_stripped() {
        // Defensive: a `/Levelling/` category or `  /  ` shouldn't
        // produce empty-named folders in the tree.
        let entries = vec![
            mk("A", Some("/Levelling/")),
            mk("B", Some("  ")),
            mk("C", Some("Bossing//HC")),
        ];
        let tree = build_folder_tree(&entries);
        // "B" with whitespace-only category lands at the root.
        assert_eq!(tree.builds.len(), 1);
        assert_eq!(tree.builds[0].label, "B");
        assert_eq!(tree.children.len(), 2);
        assert_eq!(tree.children[0].name, "Bossing");
        assert_eq!(tree.children[0].children.len(), 1);
        assert_eq!(tree.children[0].children[0].name, "HC");
        assert_eq!(tree.children[0].children[0].builds[0].label, "C");
        assert_eq!(tree.children[1].name, "Levelling");
        assert_eq!(tree.children[1].builds[0].label, "A");
    }
}
