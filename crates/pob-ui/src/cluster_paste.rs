//! Right-click "Paste cluster jewel" picker for the Tree tab.
//!
//! Slice A of [#197](https://github.com/jonatanferm/genericpathofbuildingMK2/issues/197).
//! When the user right-clicks a Large jewel socket on the tree, the picker offers
//! a paste box (PoE / PoB clipboard format) plus a "Clear jewel" entry when a
//! cluster jewel is already socketed. The pasted text goes through `parse_item`,
//! and we validate that the result looks like a cluster jewel before storing
//! it in `Character::jewels[socket_id]`. The compute path picks up the new
//! jewel automatically via `compute_full_with_clusters`.

use eframe::egui;
use pob_data::{NodeId, NodeKind, PassiveTree};
use pob_engine::{parse_item, Character};

/// Picker state. `node_id` is the Large jewel socket currently being edited;
/// `paste` holds the live textarea contents. `None` collapses the window.
#[derive(Default)]
pub struct ClusterPasteState {
    pub node_id: Option<NodeId>,
    pub paste: String,
    pub last_error: Option<String>,
}

impl ClusterPasteState {
    pub fn open_for(&mut self, node_id: NodeId) {
        self.node_id = Some(node_id);
        self.paste.clear();
        self.last_error = None;
    }
    pub fn close(&mut self) {
        self.node_id = None;
        self.paste.clear();
        self.last_error = None;
    }
}

/// Returns `true` if a cluster jewel was equipped or cleared (caller must
/// recompute). A right-click on a non-Large socket (or on any non-socket node)
/// is a no-op and returns `false` immediately.
pub fn ui(
    ctx: &egui::Context,
    state: &mut ClusterPasteState,
    tree: &PassiveTree,
    character: &mut Character,
) -> bool {
    let Some(node_id) = state.node_id else {
        return false;
    };
    let Some(node) = tree.nodes.get(&node_id) else {
        state.close();
        return false;
    };
    // Only Large jewel sockets host cluster jewels (`expansion_jewel_size == 2`
    // mirrors PoB's gate at `PassiveSpec.lua:1717`). Right-clicking any other
    // node falls back to the existing tattoo-picker / no-op path.
    if !matches!(node.kind, NodeKind::JewelSocket) || node.expansion_jewel_size != Some(2) {
        state.close();
        return false;
    }

    let mut changed = false;
    let mut should_close = false;

    let title = node
        .name
        .clone()
        .unwrap_or_else(|| format!("Large Jewel Socket #{node_id}"));
    let header = format!("Cluster jewel: {title}");
    let already_equipped = character.jewels.contains_key(&node_id);

    let mut window_open = true;
    egui::Window::new(header)
        .id(egui::Id::new(("cluster-paste-window", node_id)))
        .open(&mut window_open)
        .resizable(true)
        .default_width(420.0)
        .default_height(360.0)
        .show(ctx, |ui| {
            if already_equipped {
                ui.label("A cluster jewel is socketed here.");
                let summary = character
                    .jewels
                    .get(&node_id)
                    .map(|it| {
                        if it.name.is_empty() {
                            it.base_name.clone()
                        } else {
                            it.name.clone()
                        }
                    })
                    .unwrap_or_default();
                if !summary.is_empty() {
                    ui.label(egui::RichText::new(summary).strong());
                }
                ui.separator();
                if ui.button("Clear jewel").clicked() {
                    character.jewels.remove(&node_id);
                    changed = true;
                    should_close = true;
                }
                ui.separator();
                ui.label("Paste a different cluster jewel to replace it:");
            } else {
                ui.label("Paste a Cluster Jewel from PoE / PoB clipboard:");
            }

            egui::ScrollArea::vertical()
                .max_height(200.0)
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    ui.add(
                        egui::TextEdit::multiline(&mut state.paste)
                            .desired_width(f32::INFINITY)
                            .desired_rows(10)
                            .font(egui::TextStyle::Monospace),
                    );
                });

            ui.horizontal(|ui| {
                if ui.button("Equip from paste").clicked() {
                    match parse_item(&state.paste) {
                        Ok(item) => {
                            if !is_cluster_jewel(&item) {
                                state.last_error = Some(
                                    "That doesn't look like a Cluster Jewel — \
                                     expected a base name containing \"Cluster Jewel\" \
                                     or an `Adds N Passive Skills` mod line."
                                        .to_owned(),
                                );
                            } else {
                                character.jewels.insert(node_id, item);
                                state.last_error = None;
                                state.paste.clear();
                                changed = true;
                                should_close = true;
                            }
                        }
                        Err(e) => {
                            state.last_error = Some(e.to_string());
                        }
                    }
                }
                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
            });
            if let Some(err) = &state.last_error {
                ui.colored_label(egui::Color32::LIGHT_RED, err);
            }
        });

    if !window_open {
        should_close = true;
    }
    if should_close {
        state.close();
    }
    changed
}

/// Validate that an `Item` looks like a cluster jewel. We accept anything whose
/// base name contains `"Cluster Jewel"` or that has at least one `Adds N
/// Passive Skills` mod line — `parse_cluster_jewel` already does the same fuzzy
/// match upstream so we mirror its tolerance here.
fn is_cluster_jewel(item: &pob_data::Item) -> bool {
    if item.base_name.contains("Cluster Jewel") {
        return true;
    }
    item.mod_lines.iter().any(|m| {
        let t = m.line.trim_start();
        t.starts_with("Adds ") && (t.contains(" Passive Skills") || t.contains(" Passive Skill"))
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use pob_data::{Item, ModLine, ModSection, Rarity};

    fn empty_item() -> Item {
        Item {
            name: String::new(),
            base_name: String::new(),
            rarity: Rarity::Magic,
            item_level: 0,
            quality: 0,
            tags: ahash::HashSet::default(),
            mod_lines: Vec::new(),
            sockets: String::new(),
            raw: String::new(),
            corrupted: false,
            mirrored: false,
            variants: Vec::new(),
            variant: None,
        }
    }

    #[test]
    fn cluster_jewel_recognised_by_base_name() {
        let mut item = empty_item();
        item.base_name = "Large Cluster Jewel".to_owned();
        assert!(is_cluster_jewel(&item));
    }

    #[test]
    fn cluster_jewel_recognised_by_added_passives_mod() {
        // No base-name match — simulate an oddly-formatted paste that drops
        // the base header but keeps the enchant. We still accept it because
        // parse_cluster_jewel's fallback path handles missing base names.
        let mut item = empty_item();
        item.base_name = "Some Strange Jewel".to_owned();
        item.mod_lines.push(ModLine {
            line: "Adds 8 Passive Skills".into(),
            section: ModSection::Enchant,
            variant_list: None,
        });
        assert!(is_cluster_jewel(&item));
    }

    #[test]
    fn non_cluster_item_rejected() {
        let mut item = empty_item();
        item.base_name = "Onyx Amulet".to_owned();
        item.mod_lines.push(ModLine {
            line: "+30 to Strength".into(),
            section: ModSection::Explicit,
            variant_list: None,
        });
        assert!(!is_cluster_jewel(&item));
    }

    #[test]
    fn picker_state_open_close_round_trip() {
        let mut state = ClusterPasteState::default();
        assert!(state.node_id.is_none());
        state.open_for(1234);
        assert_eq!(state.node_id, Some(1234));
        state.paste = "noise".into();
        state.last_error = Some("err".into());
        state.close();
        assert!(state.node_id.is_none());
        assert!(state.paste.is_empty());
        assert!(state.last_error.is_none());
    }
}
