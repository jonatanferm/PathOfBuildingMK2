//! Right-click Timeless-jewel socket picker for the Tree tab.
//!
//! Issue [#216](https://github.com/jonatanferm/PathOfBuildingMK2/issues/216).
//! When the user right-clicks a regular Jewel Socket that holds a Timeless
//! jewel, this picker pops up and lets them change the jewel's **conqueror**
//! (dropdown) and **seed** (numeric field + ◀/▶ rotate buttons). Both inputs
//! live in the socketed jewel `Item`'s marker mod line
//! (`"Bathed in the blood of 6500 sacrificed in the name of Doryani"`), so the
//! edit is a rewrite of that one line via `pob_engine::set_timeless_marker` —
//! persistence + save/load round-trip are automatic because the `Item` already
//! serialises.
//!
//! There is no separate "rotation" choice for Glorious Vanity (Might vs Legacy
//! of the Vaal): PoB derives it deterministically from the seed, so the only
//! user inputs are conqueror + seed.
//!
//! The replacement preview rows are computed by the caller (it owns the LUT
//! context + node index) and passed in read-only; see
//! [`crate::LoadedApp`]'s render path.

use eframe::egui;
use pob_data::{NodeId, PassiveTree, TimelessJewelData};
use pob_engine::Character;

/// Picker state. `node_id` is the jewel socket currently being edited; `None`
/// collapses the window.
#[derive(Default)]
pub struct TimelessSocketPickerState {
    pub node_id: Option<NodeId>,
}

impl TimelessSocketPickerState {
    pub fn open_for(&mut self, node_id: NodeId) {
        self.node_id = Some(node_id);
    }
    pub fn close(&mut self) {
        self.node_id = None;
    }
}

/// One read-only row in the "nodes this jewel replaces" preview. Built by the
/// caller from `compute_keystone_replacements` / `compute_notable_replacements`
/// for the socket being edited.
pub struct PreviewRow {
    /// `"Keystone"` / `"Notable"` / `"Addition"` — what kind of replacement.
    pub kind: &'static str,
    /// Display name of the allocated tree node being conquered.
    pub target: String,
    /// What it becomes — the replacement keystone name, or the (roll-substituted)
    /// stat lines for a notable/small-node replacement.
    pub replacement: String,
}

/// Render the picker window if a socket is selected. Returns `true` when the
/// jewel's marker line actually changed (so the caller recomputes).
pub fn ui(
    ctx: &egui::Context,
    state: &mut TimelessSocketPickerState,
    character: &mut Character,
    timeless_jewels: Option<&TimelessJewelData>,
    tree: &PassiveTree,
    preview: &[PreviewRow],
) -> bool {
    let Some(node_id) = state.node_id else {
        return false;
    };
    // Locate the socketed jewel for this socket.
    let Some(idx) = character
        .socketed_jewels
        .entries
        .iter()
        .position(|(n, _)| *n == node_id)
    else {
        state.close();
        return false;
    };
    // Read the current conqueror + seed off the item's marker line.
    let Some(jewel) =
        pob_engine::identify_timeless_jewel(&character.socketed_jewels.entries[idx].1)
    else {
        state.close();
        return false;
    };
    let base_name = jewel.base_name.clone();
    let cur_conqueror = jewel.conqueror_name.clone();
    let cur_seed = jewel.seed;
    let Some(jewel_type) = pob_data::jewel_type_for_base(&base_name) else {
        state.close();
        return false;
    };
    let bounds = pob_data::stored_seed_bounds(jewel_type);

    // Conqueror options for this jewel base (data file's variant rows).
    let conquerors: Vec<String> = timeless_jewels
        .and_then(|d| d.jewels.get(&base_name))
        .map(|cfg| cfg.conquerors.iter().map(|c| c.name.clone()).collect())
        .unwrap_or_default();

    let socket_name = tree
        .nodes
        .get(&node_id)
        .and_then(|n| n.name.clone())
        .unwrap_or_else(|| format!("Jewel Socket #{node_id}"));

    let mut new_conqueror = cur_conqueror.clone();
    let mut new_seed = cur_seed;
    let mut should_close = false;

    let mut window_open = true;
    egui::Window::new(format!("Timeless jewel — {base_name}"))
        .id(egui::Id::new(("timeless-socket-picker", node_id)))
        .open(&mut window_open)
        .resizable(true)
        .default_width(420.0)
        .default_height(460.0)
        .show(ctx, |ui| {
            ui.label(
                egui::RichText::new(format!("Socket: {socket_name}"))
                    .weak()
                    .italics(),
            );
            ui.add_space(4.0);

            // --- Conqueror dropdown ---
            ui.horizontal(|ui| {
                ui.label("Conqueror:");
                if conquerors.is_empty() {
                    ui.label(egui::RichText::new(&cur_conqueror).strong());
                } else {
                    egui::ComboBox::from_id_salt(("timeless-conqueror", node_id))
                        .selected_text(&new_conqueror)
                        .show_ui(ui, |ui| {
                            for name in &conquerors {
                                ui.selectable_value(&mut new_conqueror, name.clone(), name);
                            }
                        });
                }
            });

            // --- Seed entry + rotate buttons ---
            ui.horizontal(|ui| {
                ui.label("Seed:");
                let step = bounds.map_or(1, |(_, _, s)| s);
                if ui.button("◀").on_hover_text("Previous seed").clicked() {
                    new_seed = new_seed.saturating_sub(step);
                }
                ui.add(
                    egui::DragValue::new(&mut new_seed)
                        .speed(step as f32)
                        .range(bounds.map_or(0..=u32::MAX, |(min, max, _)| min..=max)),
                );
                if ui.button("▶").on_hover_text("Next seed").clicked() {
                    new_seed = new_seed.saturating_add(step);
                }
            });
            if let Some((min, max, step)) = bounds {
                let hint = if step > 1 {
                    format!("Valid seeds: {min}–{max} (steps of {step})")
                } else {
                    format!("Valid seeds: {min}–{max}")
                };
                ui.label(egui::RichText::new(hint).weak().small());
            }

            ui.separator();

            // --- Replacement preview (read-only) ---
            ui.label(
                egui::RichText::new("Allocated nodes in radius this jewel conquers:").strong(),
            );
            if preview.is_empty() {
                ui.label(
                    egui::RichText::new(
                        "None — allocate notable / keystone / small nodes inside the jewel's \
                         radius to see replacements here.",
                    )
                    .weak(),
                );
            } else {
                egui::ScrollArea::vertical()
                    .auto_shrink([false, false])
                    .max_height(220.0)
                    .show(ui, |ui| {
                        for row in preview {
                            egui::CollapsingHeader::new(format!("[{}] {}", row.kind, row.target))
                                .id_salt(egui::Id::new((
                                    "timeless-preview",
                                    node_id,
                                    &row.target,
                                    row.kind,
                                )))
                                .default_open(false)
                                .show(ui, |ui| {
                                    ui.label(&row.replacement);
                                });
                        }
                    });
            }

            ui.add_space(4.0);
            if ui.button("Close").clicked() {
                should_close = true;
            }
        });

    if !window_open {
        should_close = true;
    }

    // Apply edits: clamp/snap the seed, then rewrite the marker line if anything
    // changed. `set_timeless_marker` returns true only on a real text change.
    new_seed = pob_data::snap_stored_seed(jewel_type, new_seed);
    let mut changed = false;
    if new_conqueror != cur_conqueror || new_seed != cur_seed {
        changed = pob_engine::set_timeless_marker(
            &mut character.socketed_jewels.entries[idx].1,
            &new_conqueror,
            new_seed,
        );
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
    fn picker_state_open_close() {
        let mut state = TimelessSocketPickerState::default();
        assert!(state.node_id.is_none());
        state.open_for(123);
        assert_eq!(state.node_id, Some(123));
        state.close();
        assert!(state.node_id.is_none());
    }
}
