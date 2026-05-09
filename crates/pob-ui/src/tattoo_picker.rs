//! Right-click tattoo picker for the Tree tab.
//!
//! Slice 2 of [#98](https://github.com/jonatanferm/PathOfBuildingMK2/issues/98).
//! When the user right-clicks an allocated normal / notable / keystone node, the picker
//! pops up filtered to tattoos whose `target_type` matches the node kind. Selecting one
//! writes its `stat_lines` (joined with newlines) into `Character::tattoo_overrides`,
//! which the engine already consumes during compute.
//!
//! Visual badge on tattooed nodes is a follow-up slice — the override is already
//! reflected in the side panel's stat output, so functionally a tattoo applies the moment
//! the user clicks it.

use eframe::egui;
use pob_data::{NodeId, NodeKind, PassiveTree, TattooSet};
use pob_engine::Character;

/// Picker state. `node_id` is the node currently being edited; `search` is the live
/// filter text. `None` collapses the window.
#[derive(Default)]
pub struct TattooPickerState {
    pub node_id: Option<NodeId>,
    pub search: String,
}

impl TattooPickerState {
    pub fn open_for(&mut self, node_id: NodeId) {
        self.node_id = Some(node_id);
        self.search.clear();
    }
    pub fn close(&mut self) {
        self.node_id = None;
        self.search.clear();
    }
}

/// Render the picker window if a node is selected. Returns `true` when the picker
/// applied or removed a tattoo (so the caller knows to recompute).
pub fn ui(
    ctx: &egui::Context,
    state: &mut TattooPickerState,
    tattoos: Option<&TattooSet>,
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
    let Some(target_type) = node_kind_to_target_type(node.kind) else {
        // Mastery / root: nothing to do.
        state.close();
        return false;
    };
    if !character.allocated.contains(&node_id) {
        state.close();
        return false;
    }

    let mut changed = false;
    let mut should_close = false;

    let title = node
        .name
        .clone()
        .unwrap_or_else(|| format!("Node #{node_id}"));
    let header = format!("Apply tattoo to {title}");

    let mut window_open = true;
    egui::Window::new(header)
        .id(egui::Id::new(("tattoo-picker-window", node_id)))
        .open(&mut window_open)
        .resizable(true)
        .default_width(380.0)
        .default_height(440.0)
        .show(ctx, |ui| {
            let Some(tattoos) = tattoos else {
                ui.label(
                    "No tattoo catalogue loaded. Run `cargo run -p pob-extract --release` to \
                     populate `data/tattoos.json` and reopen the build.",
                );
                return;
            };

            let already_tattooed = character.tattoo_overrides.contains_key(&node_id);

            ui.horizontal(|ui| {
                ui.label("Search:");
                ui.add(
                    egui::TextEdit::singleline(&mut state.search)
                        .hint_text("Acrobatics, Hex Master, …")
                        .desired_width(220.0),
                );
                if already_tattooed && ui.button("Remove tattoo").clicked() {
                    character.tattoo_overrides.remove(&node_id);
                    changed = true;
                    should_close = true;
                }
                if ui.button("Cancel").clicked() {
                    should_close = true;
                }
            });
            ui.separator();

            let q = state.search.trim().to_lowercase();
            let mut matches: Vec<(&str, &pob_data::Tattoo)> = tattoos
                .nodes
                .iter()
                .filter(|(_, t)| t.target_type == target_type)
                .filter(|(_, t)| {
                    q.is_empty()
                        || t.display_name.to_lowercase().contains(&q)
                        || t.stat_lines.iter().any(|s| s.to_lowercase().contains(&q))
                })
                .map(|(name, t)| (name.as_str(), t))
                .collect();
            matches.sort_unstable_by_key(|(_, t)| t.display_name.clone());

            ui.label(format!(
                "{} {} tattoo{} available",
                matches.len(),
                target_type.to_lowercase(),
                if matches.len() == 1 { "" } else { "s" }
            ));
            ui.add_space(2.0);

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for (key, t) in matches {
                        let id = egui::Id::new(("tattoo-row", node_id, key));
                        egui::CollapsingHeader::new(&t.display_name)
                            .id_salt(id)
                            .default_open(false)
                            .show(ui, |ui| {
                                for line in &t.stat_lines {
                                    ui.label(line);
                                }
                                ui.add_space(2.0);
                                if ui.button("Apply").clicked() {
                                    let body = t.stat_lines.join("\n");
                                    character.tattoo_overrides.insert(node_id, body);
                                    changed = true;
                                    should_close = true;
                                }
                            });
                    }
                });
        });

    if !window_open {
        should_close = true;
    }
    if should_close {
        state.close();
    }
    changed
}

/// Map MK2's `NodeKind` to PoB's `target_type` string used by the tattoo catalogue.
/// Tattoos use `"Small"` for normal nodes — preserve that mapping.
fn node_kind_to_target_type(kind: NodeKind) -> Option<&'static str> {
    match kind {
        NodeKind::Keystone => Some("Keystone"),
        NodeKind::Notable => Some("Notable"),
        NodeKind::Normal => Some("Small"),
        // Every other node kind is unsupported as a tattoo target. Use a catch-all
        // so future PoB additions to NodeKind don't fail the build before we have a
        // chance to consider them.
        _ => None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn node_kind_maps_to_pob_target_type() {
        assert_eq!(
            node_kind_to_target_type(NodeKind::Keystone),
            Some("Keystone")
        );
        assert_eq!(node_kind_to_target_type(NodeKind::Notable), Some("Notable"));
        assert_eq!(node_kind_to_target_type(NodeKind::Normal), Some("Small"));
        assert!(node_kind_to_target_type(NodeKind::Mastery).is_none());
        assert!(node_kind_to_target_type(NodeKind::JewelSocket).is_none());
        assert!(node_kind_to_target_type(NodeKind::Root).is_none());
        assert!(node_kind_to_target_type(NodeKind::ClassStart).is_none());
    }

    #[test]
    fn picker_state_open_close_round_trip() {
        let mut state = TattooPickerState::default();
        assert!(state.node_id.is_none());
        state.open_for(42);
        assert_eq!(state.node_id, Some(42));
        state.search = "abc".into();
        state.close();
        assert!(state.node_id.is_none());
        assert!(state.search.is_empty());
    }
}
