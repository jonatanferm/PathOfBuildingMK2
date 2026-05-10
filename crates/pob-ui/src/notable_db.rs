//! Tree-tab notable / keystone DB browser.
//!
//! Issue [#215](https://github.com/jonatanferm/PathOfBuildingMK2/issues/215).
//! Mirrors PoB's `NotableDBControl.lua` — a side panel cataloguing every
//! "interesting" node on the tree (notables, keystones, masteries, jewel
//! sockets) so the user can find a node by name without panning. The panel
//! reuses the existing tree-tab `search` text (so typing in the search box
//! also narrows the catalogue) and adds a node-kind pill row to filter the
//! list down to a single bucket.
//!
//! Click a row → the tree viewport recentres on the node's tree-space
//! position. The panel doesn't allocate or otherwise mutate `Character`;
//! the user still has to click the node on the tree itself to add it to
//! their build (this matches PoB's behaviour).

use eframe::egui;
use pob_data::{NodeId, NodeKind, PassiveTree};

use crate::tree_view::TreeView;

/// Per-session UI state for the DB browser. The panel starts collapsed —
/// the user opens it via the toggle in the search row.
#[derive(Default)]
pub struct NotableDbState {
    /// Whether the side panel is rendered at all.
    pub open: bool,
    /// Active node-kind filter. `None` means "every kind in
    /// [`NodeKindFilter::all`]" — no per-kind narrowing.
    pub kind_filter: Option<NodeKindFilter>,
}

/// Node kinds the DB browser exposes as a filter pill. The full
/// [`NodeKind`] enum has more variants (Normal / Root / ClassStart /
/// AscendancyStart / Tattoo / Blighted) but they aren't useful here —
/// Normals are too numerous to browse, the start / synthetic kinds aren't
/// individually addressable, and tattoos are placed via right-click on
/// allocated nodes rather than by lookup.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum NodeKindFilter {
    Notable,
    Keystone,
    Mastery,
    JewelSocket,
}

impl NodeKindFilter {
    pub fn all() -> &'static [NodeKindFilter] {
        &[
            NodeKindFilter::Notable,
            NodeKindFilter::Keystone,
            NodeKindFilter::Mastery,
            NodeKindFilter::JewelSocket,
        ]
    }

    pub fn label(self) -> &'static str {
        match self {
            NodeKindFilter::Notable => "Notable",
            NodeKindFilter::Keystone => "Keystone",
            NodeKindFilter::Mastery => "Mastery",
            NodeKindFilter::JewelSocket => "Jewel Socket",
        }
    }

    fn matches(self, kind: NodeKind) -> bool {
        matches!(
            (self, kind),
            (NodeKindFilter::Notable, NodeKind::Notable)
                | (NodeKindFilter::Keystone, NodeKind::Keystone)
                | (NodeKindFilter::Mastery, NodeKind::Mastery)
                | (NodeKindFilter::JewelSocket, NodeKind::JewelSocket)
        )
    }
}

/// Predicate: is this node kind eligible for the DB browser at all? Used
/// by [`filter_entries`] when no per-kind filter is active so we still
/// exclude the always-uninteresting kinds (Normal nodes, synthetic
/// root / class-start, tattoos, blighted).
fn kind_eligible(kind: NodeKind) -> bool {
    matches!(
        kind,
        NodeKind::Notable | NodeKind::Keystone | NodeKind::Mastery | NodeKind::JewelSocket
    )
}

/// Pull every catalogue-eligible node out of `tree` that matches both the
/// case-insensitive substring `query` (against `name` and any `stats`
/// line) and the optional `kind_filter`. Empty `query` is treated as
/// "match everything" — unlike the highlight-only `compute_search_matches`
/// in `lib.rs`, the DB browser shows the whole catalogue when nothing has
/// been typed so the user can scroll. The returned vector is sorted by
/// `(name, id)` so panel order is deterministic.
pub fn filter_entries(
    tree: &PassiveTree,
    query: &str,
    kind_filter: Option<NodeKindFilter>,
) -> Vec<NodeId> {
    let q = query.trim().to_lowercase();
    let mut out: Vec<NodeId> = tree
        .nodes
        .iter()
        .filter(|(_, node)| match kind_filter {
            Some(f) => f.matches(node.kind),
            None => kind_eligible(node.kind),
        })
        .filter(|(_, node)| {
            if q.is_empty() {
                return true;
            }
            let name_match = node
                .name
                .as_deref()
                .map(|s| s.to_lowercase().contains(&q))
                .unwrap_or(false);
            let stat_match = node.stats.iter().any(|s| s.to_lowercase().contains(&q));
            name_match || stat_match
        })
        .map(|(id, _)| *id)
        .collect();
    out.sort_unstable_by(|a, b| {
        let na = tree
            .nodes
            .get(a)
            .and_then(|n| n.name.as_deref())
            .unwrap_or("");
        let nb = tree
            .nodes
            .get(b)
            .and_then(|n| n.name.as_deref())
            .unwrap_or("");
        na.to_ascii_lowercase()
            .cmp(&nb.to_ascii_lowercase())
            .then_with(|| a.cmp(b))
    });
    out
}

fn kind_glyph(kind: NodeKind) -> &'static str {
    match kind {
        NodeKind::Notable => "◆",
        NodeKind::Keystone => "★",
        NodeKind::Mastery => "○",
        NodeKind::JewelSocket => "◇",
        _ => "·",
    }
}

/// Side-panel UI. Renders a header, the kind-filter pill row, a count
/// label, and a scrollable list of every matching node. Clicking a row
/// recentres `tree_view` on the node and returns its id so the caller can
/// optionally cycle search focus alongside.
///
/// The panel intentionally doesn't carry its own search input — it shares
/// the tree-tab `search` text driven by the existing search row, so the
/// two stay in lock-step. The `query` argument is borrowed read-only for
/// that reason.
pub fn render_panel(
    ui: &mut egui::Ui,
    state: &mut NotableDbState,
    tree: &PassiveTree,
    tree_view: &mut TreeView,
    query: &str,
) -> Option<NodeId> {
    let mut clicked: Option<NodeId> = None;
    ui.heading("Browse nodes");
    ui.weak("Click a row to recentre the tree on that node.");
    ui.separator();

    ui.horizontal_wrapped(|ui| {
        ui.label("Kind:");
        if ui
            .selectable_label(state.kind_filter.is_none(), "All")
            .on_hover_text("Show notables, keystones, masteries, and jewel sockets together.")
            .clicked()
        {
            state.kind_filter = None;
        }
        for k in NodeKindFilter::all() {
            let active = state.kind_filter == Some(*k);
            if ui.selectable_label(active, k.label()).clicked() {
                state.kind_filter = if active { None } else { Some(*k) };
            }
        }
    });
    ui.add_space(2.0);

    let entries = filter_entries(tree, query, state.kind_filter);
    let total = catalogue_size(tree, state.kind_filter);
    ui.label(format!("{} of {} nodes", entries.len(), total));
    ui.add_space(2.0);

    egui::ScrollArea::vertical().show(ui, |ui| {
        for id in entries {
            let Some(node) = tree.nodes.get(&id) else {
                continue;
            };
            let glyph = kind_glyph(node.kind);
            let name = node.name.as_deref().unwrap_or("(unnamed)");
            let stats_preview = node
                .stats
                .iter()
                .take(2)
                .cloned()
                .collect::<Vec<_>>()
                .join(" • ");
            let body = if stats_preview.is_empty() {
                format!("{glyph} {name}")
            } else {
                format!("{glyph} {name}\n    {stats_preview}")
            };
            let row = ui
                .add(egui::Label::new(body).sense(egui::Sense::click()))
                .on_hover_text("Click to recentre the tree on this node.");
            if row.clicked() {
                if let Some(p) = tree_view.position_of(id) {
                    tree_view.focus(p.x, p.y);
                }
                clicked = Some(id);
            }
            ui.separator();
        }
    });
    clicked
}

/// Total catalogue size for the current `kind_filter`, regardless of the
/// search query. Used by [`render_panel`] to render the "X of Y" denominator.
fn catalogue_size(tree: &PassiveTree, kind_filter: Option<NodeKindFilter>) -> usize {
    tree.nodes
        .values()
        .filter(|n| match kind_filter {
            Some(f) => f.matches(n.kind),
            None => kind_eligible(n.kind),
        })
        .count()
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::{HashMap, HashMapExt};
    use pob_data::{Node, TreeConstants};

    fn mk_node(id: NodeId, name: &str, kind: NodeKind, stats: Vec<&str>) -> Node {
        Node {
            id,
            name: Some(name.into()),
            icon: None,
            ascendancy_name: None,
            stats: stats.into_iter().map(String::from).collect(),
            reminder_text: Vec::new(),
            kind,
            class_start_index: None,
            group: None,
            orbit: None,
            orbit_index: None,
            out_edges: Default::default(),
            in_edges: Default::default(),
            mastery_effects: Vec::new(),
            expansion_jewel_size: None,
            jewel_radius: None,
        }
    }

    fn fixture_tree() -> PassiveTree {
        let mut nodes = HashMap::new();
        nodes.insert(
            1,
            mk_node(
                1,
                "Frenzy Resonance",
                NodeKind::Notable,
                vec!["Gain a Frenzy Charge on Hit"],
            ),
        );
        nodes.insert(
            2,
            mk_node(
                2,
                "Acrobatics",
                NodeKind::Keystone,
                vec!["+30% chance to Dodge"],
            ),
        );
        nodes.insert(
            3,
            mk_node(
                3,
                "Life Mastery",
                NodeKind::Mastery,
                vec!["+50 to maximum Life"],
            ),
        );
        nodes.insert(
            4,
            mk_node(4, "Medium Jewel Socket", NodeKind::JewelSocket, vec![]),
        );
        nodes.insert(
            5,
            mk_node(
                5,
                "Plain +10 Strength",
                NodeKind::Normal,
                vec!["+10 to Strength"],
            ),
        );
        nodes.insert(
            6,
            mk_node(
                6,
                "Frenzy Cluster",
                NodeKind::Notable,
                vec!["Frenzy Charges last 50% longer"],
            ),
        );
        PassiveTree {
            version: "test".into(),
            tree: "Default".into(),
            classes: Vec::new(),
            groups: HashMap::new(),
            nodes,
            jewel_slots: Vec::new(),
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants: TreeConstants {
                skills_per_orbit: Vec::new(),
                orbit_radii: Vec::new(),
                classes: Default::default(),
                character_attributes: Default::default(),
                pss_centre_inner_radius: None,
            },
            points: Default::default(),
        }
    }

    #[test]
    fn empty_query_returns_every_eligible_node() {
        let tree = fixture_tree();
        let entries = filter_entries(&tree, "", None);
        // Notable + Notable + Keystone + Mastery + JewelSocket = 5; Normal excluded.
        assert_eq!(entries.len(), 5);
        assert!(!entries.contains(&5));
    }

    #[test]
    fn substring_query_matches_name_or_stats_case_insensitive() {
        let tree = fixture_tree();
        // Name match
        let by_name = filter_entries(&tree, "Acrobatics", None);
        assert_eq!(by_name, vec![2]);
        // Stat match (case-insensitive)
        let by_stat = filter_entries(&tree, "DODGE", None);
        assert_eq!(by_stat, vec![2]);
        // Substring across multiple notables — sorted alphabetically.
        let multi = filter_entries(&tree, "frenzy", None);
        assert_eq!(multi, vec![6, 1]); // "Frenzy Cluster" before "Frenzy Resonance"
    }

    #[test]
    fn kind_filter_narrows_results() {
        let tree = fixture_tree();
        assert_eq!(
            filter_entries(&tree, "", Some(NodeKindFilter::Keystone)),
            vec![2]
        );
        assert_eq!(
            filter_entries(&tree, "", Some(NodeKindFilter::Mastery)),
            vec![3]
        );
        assert_eq!(
            filter_entries(&tree, "", Some(NodeKindFilter::JewelSocket)),
            vec![4]
        );
        let notables = filter_entries(&tree, "", Some(NodeKindFilter::Notable));
        assert_eq!(notables.len(), 2);
    }

    #[test]
    fn kind_filter_combines_with_query() {
        let tree = fixture_tree();
        // Two notables match "frenzy"; keystone-only filter rejects both.
        assert!(filter_entries(&tree, "frenzy", Some(NodeKindFilter::Keystone)).is_empty());
        // Notable filter keeps them.
        let entries = filter_entries(&tree, "frenzy", Some(NodeKindFilter::Notable));
        assert_eq!(entries.len(), 2);
    }

    #[test]
    fn kind_filter_excludes_other_eligible_kinds_even_without_query() {
        let tree = fixture_tree();
        // Mastery filter shouldn't accidentally include the keystone or jewel socket.
        let entries = filter_entries(&tree, "", Some(NodeKindFilter::Mastery));
        assert_eq!(entries, vec![3]);
    }

    #[test]
    fn whitespace_only_query_treated_as_empty() {
        let tree = fixture_tree();
        let trimmed = filter_entries(&tree, "   ", None);
        let empty = filter_entries(&tree, "", None);
        assert_eq!(trimmed, empty);
    }

    #[test]
    fn unnamed_eligible_nodes_still_appear() {
        // Real trees include unnamed jewel sockets ("Medium Jewel Socket" is a
        // common synthesised name; some sockets just have no name field). The
        // filter should still surface them so the user can find them — just
        // sorted last because empty names sort before others alphabetically.
        let mut tree = fixture_tree();
        tree.nodes.insert(7, {
            let mut n = mk_node(7, "", NodeKind::JewelSocket, vec![]);
            n.name = None;
            n
        });
        let entries = filter_entries(&tree, "", Some(NodeKindFilter::JewelSocket));
        assert_eq!(entries.len(), 2);
        assert!(entries.contains(&4));
        assert!(entries.contains(&7));
    }

    #[test]
    fn catalogue_size_counts_eligible_nodes_only() {
        let tree = fixture_tree();
        // Excludes Normal node #5.
        assert_eq!(catalogue_size(&tree, None), 5);
        assert_eq!(catalogue_size(&tree, Some(NodeKindFilter::Notable)), 2);
        assert_eq!(catalogue_size(&tree, Some(NodeKindFilter::Keystone)), 1);
    }
}
