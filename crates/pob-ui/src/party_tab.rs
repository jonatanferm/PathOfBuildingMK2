//! Party tab — group-play teammates whose auras / curses / banners
//! project onto the player. Each member carries a free-form list of
//! mod lines; the engine parses them through `mod_parser` at compute
//! time and tags them with `Party:<member_name>` as the source.
//!
//! This MVP doesn't yet import a teammate's PoB code and extract their
//! actual auras — the user pastes the relevant mod lines directly.
//! Future work: link to a `pob_engine::import_pob_code` call that
//! pulls auras out of a teammate's tree + skill bar automatically.

use eframe::egui;
use pob_engine::{character::PartyMember, Character};

#[derive(Debug, Clone, Default)]
pub struct PartyTabState {
    /// Index of the member currently shown in the right-pane editor.
    /// `None` means "no member selected".
    pub selected: Option<usize>,
    /// Buffer for the "add new member" name input.
    pub new_name: String,
}

/// Returns true if any member field changed (so the caller can recompute).
pub fn ui(ui: &mut egui::Ui, state: &mut PartyTabState, character: &mut Character) -> bool {
    let mut changed = false;

    ui.horizontal(|ui| {
        ui.heading("Party");
        ui.separator();
        ui.add(
            egui::TextEdit::singleline(&mut state.new_name)
                .desired_width(160.0)
                .hint_text("Teammate name…"),
        );
        let add_enabled = !state.new_name.trim().is_empty();
        if ui
            .add_enabled(add_enabled, egui::Button::new("Add member"))
            .clicked()
        {
            character.party_members.push(PartyMember {
                name: state.new_name.trim().to_owned(),
                mod_lines: String::new(),
                enabled: true,
            });
            state.selected = Some(character.party_members.len() - 1);
            state.new_name.clear();
            changed = true;
        }
    });
    ui.separator();

    if character.party_members.is_empty() {
        ui.label(
            "No party members yet. Add one above and paste the mod lines they project (auras, \
             banners, curses, etc.) into the right pane.\n\
             \n\
             Example: a teammate running Hatred at 50% effect would have a line like\n\
             `+50% to Cold Damage` in their mod box; the player's Calcs tab then\n\
             shows the buff sourced from `Party:<their name>`.",
        );
        return changed;
    }

    ui.horizontal(|ui| {
        // Left pane: member list with enable toggle and select / delete.
        ui.vertical(|ui| {
            ui.set_min_width(200.0);
            // Snapshot so we don't borrow during mutation inside the loop.
            let entries: Vec<(usize, String, bool)> = character
                .party_members
                .iter()
                .enumerate()
                .map(|(i, m)| (i, m.name.clone(), m.enabled))
                .collect();
            let mut delete_idx: Option<usize> = None;
            let mut toggle_idx: Option<usize> = None;
            for (idx, name, enabled) in entries {
                ui.horizontal(|ui| {
                    let label = if enabled { name.clone() } else { format!("{name} (off)") };
                    let selected = state.selected == Some(idx);
                    if ui.selectable_label(selected, label).clicked() {
                        state.selected = Some(idx);
                    }
                    if ui
                        .small_button(if enabled { "✓" } else { "—" })
                        .on_hover_text("Toggle this member's contribution")
                        .clicked()
                    {
                        toggle_idx = Some(idx);
                    }
                    if ui.small_button("✕").on_hover_text("Remove member").clicked() {
                        delete_idx = Some(idx);
                    }
                });
            }
            if let Some(idx) = toggle_idx {
                if let Some(member) = character.party_members.get_mut(idx) {
                    member.enabled = !member.enabled;
                    changed = true;
                }
            }
            if let Some(idx) = delete_idx {
                character.party_members.remove(idx);
                if state.selected == Some(idx) {
                    state.selected = None;
                } else if let Some(sel) = state.selected {
                    if sel > idx {
                        state.selected = Some(sel - 1);
                    }
                }
                changed = true;
            }
        });

        ui.separator();

        // Right pane: edit selected member's mod lines.
        ui.vertical(|ui| {
            let Some(idx) = state.selected else {
                ui.weak("Select a member on the left to edit their mods.");
                return;
            };
            let total_members = character.party_members.len();
            let Some(member) = character.party_members.get_mut(idx) else {
                state.selected = None;
                return;
            };
            ui.horizontal(|ui| {
                ui.label("Member:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut member.name).desired_width(220.0),
                );
                if resp.changed() {
                    changed = true;
                }
                ui.weak(format!("({}/{})", idx + 1, total_members));
            });
            ui.checkbox(&mut member.enabled, "Active in calc")
                .on_hover_text("Untick to exclude this member from the next compute pass");
            ui.label(
                "Mod lines (one per line, same syntax as Custom Modifiers / item mods):",
            );
            let resp = ui.add(
                egui::TextEdit::multiline(&mut member.mod_lines)
                    .desired_width(f32::INFINITY)
                    .desired_rows(10)
                    .hint_text("e.g. +30% to Cold Damage\n+15% increased Attack Speed")
                    .font(egui::TextStyle::Monospace),
            );
            if resp.changed() {
                changed = true;
            }
            // Live parse-status summary so users can spot bad lines at
            // a glance — same UX as Custom Modifiers.
            let total = member
                .mod_lines
                .lines()
                .filter(|l| !l.trim().is_empty())
                .count();
            if total > 0 {
                let parsed = member
                    .mod_lines
                    .lines()
                    .filter(|l| !l.trim().is_empty())
                    .filter(|l| pob_engine::mod_parser::parse_mod_line(l.trim()).is_some())
                    .count();
                let color = if parsed == total {
                    egui::Color32::from_rgb(0x33, 0xFF, 0x77)
                } else {
                    egui::Color32::from_rgb(0xFF, 0x99, 0x22)
                };
                ui.colored_label(color, format!("{parsed} / {total} lines parse"));
            }
        });
    });

    changed
}
