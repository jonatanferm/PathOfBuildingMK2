//! Click-to-pick mastery effect dialog for the Tree tab.
//!
//! Issue [#210](https://github.com/jonatanferm/PathOfBuildingMK2/issues/210).
//! When the user clicks an allocated mastery node the picker pops up listing
//! every `mastery_effects` entry on that node. Selecting one writes the
//! effect id into `Character::mastery_selections`, which the engine already
//! consumes during compute (`perform.rs`). Right-clicking an allocated mastery
//! node clears the current selection.
//!
//! The window is rendered with `collapsible(false)` and `movable(false)`; egui
//! has no true modal, but this matches the convention used elsewhere (see
//! `tattoo_picker.rs`). Esc closes the picker via the standard `open(&mut bool)`
//! pattern combined with an explicit `key_pressed(Escape)` check.
//!
//! Per-option DPS / EHP deltas are intentionally deferred — the issue gates them
//! on power scoring landing, and this slice ships the MVP picker only.

use eframe::egui;
use pob_data::{NodeId, NodeKind, PassiveTree};
use pob_engine::Character;

/// Picker state. `node_id` is the mastery node currently being edited; `None`
/// collapses the window.
#[derive(Default)]
pub struct MasteryPickerState {
    pub node_id: Option<NodeId>,
}

impl MasteryPickerState {
    pub fn open_for(&mut self, node_id: NodeId) {
        self.node_id = Some(node_id);
    }
    pub fn close(&mut self) {
        self.node_id = None;
    }
}

/// Render the picker window if a mastery node is selected. Returns `true`
/// when the picker mutated `character.mastery_selections` (so the caller knows
/// to recompute). A click on a non-mastery or unallocated node is a no-op and
/// returns `false` immediately.
pub fn ui(
    ctx: &egui::Context,
    state: &mut MasteryPickerState,
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
    if !matches!(node.kind, NodeKind::Mastery) {
        state.close();
        return false;
    }
    if !character.allocated.contains(&node_id) {
        state.close();
        return false;
    }
    if node.mastery_effects.is_empty() {
        // Nothing to pick — pretend the picker never opened.
        state.close();
        return false;
    }

    let mut changed = false;
    let mut should_close = false;

    let title = node
        .name
        .clone()
        .unwrap_or_else(|| format!("Mastery #{node_id}"));
    let header = format!("Choose mastery effect — {title}");

    let current = character.mastery_selections.get(&node_id).copied();

    let mut window_open = true;
    egui::Window::new(header)
        .id(egui::Id::new(("mastery-picker-window", node_id)))
        .open(&mut window_open)
        .collapsible(false)
        .movable(false)
        .resizable(true)
        .default_width(400.0)
        .default_height(360.0)
        .anchor(egui::Align2::CENTER_CENTER, egui::vec2(0.0, 0.0))
        .show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label(format!(
                    "{} effect{} available",
                    node.mastery_effects.len(),
                    if node.mastery_effects.len() == 1 {
                        ""
                    } else {
                        "s"
                    }
                ));
                ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                    if ui.button("Cancel").clicked() {
                        should_close = true;
                    }
                    if current.is_some() && ui.button("Clear selection").clicked() {
                        character.mastery_selections.remove(&node_id);
                        changed = true;
                        should_close = true;
                    }
                });
            });
            ui.separator();

            egui::ScrollArea::vertical()
                .auto_shrink([false, false])
                .show(ui, |ui| {
                    for effect in &node.mastery_effects {
                        let selected = current == Some(effect.effect);
                        let body = if effect.stats.is_empty() {
                            format!("(effect #{} — no stats)", effect.effect)
                        } else {
                            effect.stats.join("\n")
                        };
                        let resp = ui.add(egui::SelectableLabel::new(selected, body));
                        if resp.clicked() {
                            character.mastery_selections.insert(node_id, effect.effect);
                            changed = true;
                            should_close = true;
                        }
                        ui.add_space(2.0);
                    }
                });
        });

    if !window_open {
        should_close = true;
    }
    // Esc closes the picker. Match the convention used by the search bar in
    // lib.rs: poll the input layer once per frame.
    if ctx.input(|i| i.key_pressed(egui::Key::Escape)) {
        should_close = true;
    }
    if should_close {
        state.close();
    }
    changed
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn picker_state_open_close_round_trip() {
        let mut state = MasteryPickerState::default();
        assert!(state.node_id.is_none());
        state.open_for(42);
        assert_eq!(state.node_id, Some(42));
        state.close();
        assert!(state.node_id.is_none());
    }
}
