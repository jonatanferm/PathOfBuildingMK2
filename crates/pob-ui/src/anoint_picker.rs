//! Amulet anointment picker for [#221](https://github.com/jonatanferm/PathOfBuildingMK2/issues/221).
//!
//! Anointing an amulet copies one passive-tree notable's stats onto
//! the amulet as enchant-section mod lines. Unlike the lab enchant
//! catalogues, there's no separate data file — the notable list is a
//! derived view over the live `PassiveTree`.
//!
//! This module owns:
//!
//! - The pure filter helper that produces the picker's row list
//!   ([`anointable_notables`]).
//! - The egui popup that renders the list with search + commit
//!   ([`render_picker_popup`]).
//!
//! Apply path rides on [`pob_data::Item::apply_enchant`] — anoint mods
//! land as `ModSection::Enchant` lines on the amulet, which is what
//! the existing engine path expects (PoB's parser categorises any
//! `(enchant)`-suffixed line as an enchant regardless of section
//! order).

use eframe::egui;
use pob_data::{NodeId, NodeKind, PassiveTree, Slot};
use pob_engine::Character;

/// A notable that a user can anoint onto their amulet. Pulled out of
/// the live tree by [`anointable_notables`]. The picker UI walks the
/// returned vec; the apply path commits `stats` via
/// [`Item::apply_enchant`].
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct AnointableNotable {
    pub id: NodeId,
    pub name: String,
    pub stats: Vec<String>,
}

/// Issue #221: collect every anointable notable from `tree`. Pure /
/// no allocation beyond the returned vec — the picker calls this per
/// open frame so a future caching layer can reach for it later.
///
/// Eligibility rules:
///
/// - `kind == Notable` (not Normal, Keystone, Mastery, Socket, etc.).
/// - Has a non-empty `name` (synthetic nodes can be Notable-kinded but
///   lack display names — they wouldn't be useful anoint picks).
/// - No `ascendancy_name` — ascendancy notables aren't anointable.
/// - At least one `stats` line — a notable with zero stat text would
///   be a no-op anoint and just confuses the user.
///
/// The output is sorted alphabetically by `name` (case-insensitive)
/// so picker rows have a stable order independent of the
/// `tree.nodes` HashMap iteration.
#[must_use]
pub fn anointable_notables(tree: &PassiveTree) -> Vec<AnointableNotable> {
    let mut out: Vec<AnointableNotable> = tree
        .nodes
        .values()
        .filter(|n| n.kind == NodeKind::Notable)
        .filter(|n| n.ascendancy_name.is_none())
        .filter_map(|n| {
            let name = n.name.as_deref()?;
            if name.is_empty() {
                return None;
            }
            if n.stats.is_empty() {
                return None;
            }
            Some(AnointableNotable {
                id: n.id,
                name: name.to_owned(),
                stats: n.stats.clone(),
            })
        })
        .collect();
    out.sort_by(|a, b| {
        a.name
            .to_ascii_lowercase()
            .cmp(&b.name.to_ascii_lowercase())
    });
    out
}

/// Render the amulet anointment popup. Returns `true` when the user
/// commits a pick so the caller can dirty-flag the build + recompute.
///
/// Gated on `state.open` — the caller (items_tab) toggles this from a
/// button on the equipped Amulet slot.
pub fn render_picker_popup(
    ui: &mut egui::Ui,
    character: &mut Character,
    state: &mut AnointPickerState,
    tree: &PassiveTree,
) -> bool {
    if !state.open {
        return false;
    }
    let mut window_open = true;
    let mut committed = false;
    let mut chosen: Option<AnointableNotable> = None;
    let mut clear_anoint = false;
    egui::Window::new("Apply Amulet Anointment")
        .id(egui::Id::new("amulet-anoint-picker"))
        .open(&mut window_open)
        .resizable(true)
        .collapsible(false)
        .default_width(460.0)
        .default_height(420.0)
        .show(ui.ctx(), |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.filter)
                        .desired_width(260.0)
                        .hint_text("notable name or stat text"),
                );
                if ui.small_button("✕").on_hover_text("Clear search").clicked() {
                    state.filter.clear();
                }
            });
            ui.separator();
            let notables = anointable_notables(tree);
            let filter_lc = state.filter.to_ascii_lowercase();
            egui::ScrollArea::vertical()
                .id_salt("anoint-list")
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for entry in &notables {
                        if !filter_lc.is_empty() && !matches_anoint(entry, &filter_lc) {
                            continue;
                        }
                        ui.group(|ui| {
                            let header_resp = ui.add(
                                egui::Label::new(egui::RichText::new(&entry.name).strong())
                                    .sense(egui::Sense::click()),
                            );
                            for stat in &entry.stats {
                                ui.label(format!("  • {stat}"));
                            }
                            if header_resp.clicked() || ui.button("Apply").clicked() {
                                chosen = Some(entry.clone());
                            }
                        });
                    }
                });
            ui.horizontal(|ui| {
                if ui
                    .button("Remove anoint")
                    .on_hover_text("Strip any existing anointment from the amulet.")
                    .clicked()
                {
                    clear_anoint = true;
                }
                if ui.button("Cancel").clicked() {
                    state.open = false;
                }
            });
        });
    if let Some(entry) = chosen {
        if let Some(item) = character.items.get_mut(Slot::Amulet) {
            item.apply_enchant(&entry.stats);
            committed = true;
        }
        state.open = false;
    } else if clear_anoint {
        if let Some(item) = character.items.get_mut(Slot::Amulet) {
            item.apply_enchant(&[]);
            committed = true;
        }
        state.open = false;
    } else if !window_open {
        state.open = false;
    }
    committed
}

fn matches_anoint(entry: &AnointableNotable, filter_lc: &str) -> bool {
    if entry.name.to_ascii_lowercase().contains(filter_lc) {
        return true;
    }
    entry
        .stats
        .iter()
        .any(|s| s.to_ascii_lowercase().contains(filter_lc))
}

/// UI state for the anoint picker. Owned by the items-tab state so
/// the popup survives across frames.
#[derive(Debug, Clone, Default)]
pub struct AnointPickerState {
    pub open: bool,
    pub filter: String,
}

#[cfg(test)]
mod tests {
    use super::*;
    use ahash::HashMap;
    use pob_data::tree::{Node, TreeConstants};

    fn notable(id: NodeId, name: Option<&str>, stats: &[&str], asc: Option<&str>) -> Node {
        let v = serde_json::json!({
            "id": id,
            "name": name,
            "stats": stats,
            "kind": "notable",
            "ascendancy_name": asc,
        });
        serde_json::from_value(v).expect("valid notable")
    }

    fn other(id: NodeId, kind: &str, name: Option<&str>) -> Node {
        let v = serde_json::json!({
            "id": id,
            "name": name,
            "stats": ["+5 to Stat"],
            "kind": kind,
        });
        serde_json::from_value(v).expect("valid node")
    }

    fn mk_tree(nodes: Vec<Node>) -> PassiveTree {
        let mut map: HashMap<NodeId, Node> = HashMap::default();
        for n in nodes {
            map.insert(n.id, n);
        }
        let constants: TreeConstants = serde_json::from_value(serde_json::json!({
            "skills_per_orbit": [],
            "orbit_radii": [],
        }))
        .expect("constants");
        PassiveTree {
            version: "test".into(),
            tree: "Default".into(),
            classes: Vec::new(),
            groups: ahash::HashMap::default(),
            nodes: map,
            jewel_slots: Vec::new(),
            min_x: 0,
            min_y: 0,
            max_x: 0,
            max_y: 0,
            constants,
            points: Default::default(),
        }
    }

    #[test]
    fn anointable_notables_filters_by_kind_and_sorts_alphabetically() {
        let tree = mk_tree(vec![
            notable(1, Some("Bandit's Hideout"), &["+5% Attack Speed"], None),
            notable(2, Some("Alchemist's Mark"), &["+10% Cast Speed"], None),
            other(3, "keystone", Some("Resolute Technique")),
            other(4, "normal", Some("+10 Strength")),
        ]);
        let list = anointable_notables(&tree);
        let names: Vec<_> = list.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Alchemist's Mark", "Bandit's Hideout"]);
    }

    #[test]
    fn anointable_notables_skips_ascendancy_notables() {
        let tree = mk_tree(vec![
            notable(1, Some("Tree Notable"), &["+5%"], None),
            notable(2, Some("Asc Notable"), &["+10%"], Some("Slayer")),
        ]);
        let list = anointable_notables(&tree);
        let names: Vec<_> = list.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Tree Notable"]);
    }

    #[test]
    fn anointable_notables_skips_nameless_and_statless() {
        let tree = mk_tree(vec![
            notable(1, Some(""), &["+5%"], None),
            notable(2, Some("No-Stats Notable"), &[], None),
            notable(3, None, &["+5%"], None),
            notable(4, Some("Real Notable"), &["+5%"], None),
        ]);
        let list = anointable_notables(&tree);
        let names: Vec<_> = list.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["Real Notable"]);
    }

    #[test]
    fn anointable_notables_sort_is_case_insensitive() {
        // ASCII-lowercase comparison: lowercase 'a' should sort
        // before uppercase 'B' (which would be wrong without
        // case-folding).
        let tree = mk_tree(vec![
            notable(1, Some("Beacon"), &["+5%"], None),
            notable(2, Some("alchemy"), &["+5%"], None),
            notable(3, Some("Crystal Skin"), &["+5%"], None),
        ]);
        let list = anointable_notables(&tree);
        let names: Vec<_> = list.iter().map(|n| n.name.as_str()).collect();
        assert_eq!(names, vec!["alchemy", "Beacon", "Crystal Skin"]);
    }

    #[test]
    fn matches_anoint_matches_name_or_stat_text() {
        let entry = AnointableNotable {
            id: 1,
            name: "Heart of Flame".into(),
            stats: vec!["10% increased Fire Damage".into()],
        };
        assert!(matches_anoint(&entry, "flame"));
        // Stat text match too — useful for "show me all fire-related
        // anoints" without typing every name.
        assert!(matches_anoint(&entry, "fire"));
        assert!(!matches_anoint(&entry, "cold"));
    }
}
