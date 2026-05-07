//! Items tab — slot grid + paste-to-equip.

use eframe::egui;
use pob_data::{Item, ItemSet, Rarity, Slot};
use pob_engine::parse_item;

pub struct ItemsTabState {
    /// Slot the user is currently editing (paste / clear / view).
    pub selected_slot: Option<Slot>,
    /// Buffer for the textarea input.
    pub paste_buffer: String,
    /// Last parse error, if any, shown next to the textarea.
    pub last_error: Option<String>,
}

impl Default for ItemsTabState {
    fn default() -> Self {
        Self {
            selected_slot: Some(Slot::Amulet),
            paste_buffer: String::new(),
            last_error: None,
        }
    }
}

/// Returns true if the equipped items changed (so the caller can recompute).
pub fn ui(ui: &mut egui::Ui, state: &mut ItemsTabState, items: &mut ItemSet) -> bool {
    let mut changed = false;
    ui.horizontal(|ui| {
        // Left: slot grid
        ui.vertical(|ui| {
            ui.set_min_width(180.0);
            ui.heading("Slots");
            ui.separator();
            for slot in Slot::all() {
                let equipped = items.get(*slot);
                let label = if let Some(item) = equipped {
                    let rarity_glyph = rarity_glyph(item.rarity);
                    if item.name.is_empty() {
                        format!("{rarity_glyph} {} — {}", slot.label(), item.base_name)
                    } else {
                        format!("{rarity_glyph} {} — {}", slot.label(), item.name)
                    }
                } else {
                    format!("· {} — (empty)", slot.label())
                };
                let selected = state.selected_slot == Some(*slot);
                if ui.selectable_label(selected, label).clicked() {
                    state.selected_slot = Some(*slot);
                    state.paste_buffer.clear();
                    state.last_error = None;
                }
            }
        });

        ui.separator();

        // Right: editor
        ui.vertical(|ui| {
            ui.heading(
                state
                    .selected_slot
                    .map(|s| s.label().to_string())
                    .unwrap_or_else(|| "(no slot selected)".to_owned()),
            );
            ui.separator();

            if let Some(slot) = state.selected_slot {
                if let Some(item) = items.get(slot) {
                    egui::ScrollArea::vertical()
                        .max_height(220.0)
                        .show(ui, |ui| {
                            render_item_summary(ui, item);
                        });
                    ui.add_space(4.0);
                    if ui.button("Unequip").clicked() {
                        items.unequip(slot);
                        changed = true;
                    }
                    ui.separator();
                }
                ui.label("Paste an item from PoE / PoB:");
                egui::ScrollArea::vertical()
                    .max_height(220.0)
                    .show(ui, |ui| {
                        ui.add(
                            egui::TextEdit::multiline(&mut state.paste_buffer)
                                .desired_width(f32::INFINITY)
                                .desired_rows(10)
                                .font(egui::TextStyle::Monospace),
                        );
                    });
                ui.horizontal(|ui| {
                    if ui.button("Equip from paste").clicked() {
                        match parse_item(&state.paste_buffer) {
                            Ok(item) => {
                                items.equip(slot, item);
                                state.last_error = None;
                                state.paste_buffer.clear();
                                changed = true;
                            }
                            Err(e) => {
                                state.last_error = Some(e.to_string());
                            }
                        }
                    }
                    // Note: egui doesn't expose synchronous clipboard reads in this
                    // version, so we just instruct the user to paste manually with
                    // the system shortcut.
                    if ui.button("Clear paste").clicked() {
                        state.paste_buffer.clear();
                        state.last_error = None;
                    }
                });
                if let Some(err) = &state.last_error {
                    ui.colored_label(egui::Color32::LIGHT_RED, err);
                }
            } else {
                ui.label("Pick a slot on the left.");
            }
        });
    });
    changed
}

fn rarity_glyph(r: Rarity) -> &'static str {
    match r {
        Rarity::Normal => "·",
        Rarity::Magic => "M",
        Rarity::Rare => "R",
        Rarity::Unique => "U",
        Rarity::Relic => "L",
    }
}

fn render_item_summary(ui: &mut egui::Ui, item: &Item) {
    ui.label(egui::RichText::new(&item.name).strong());
    ui.label(&item.base_name);
    ui.label(format!(
        "{:?} • iLvl {} • Q{}{}",
        item.rarity,
        item.item_level,
        item.quality,
        if item.corrupted { " • Corrupted" } else { "" }
    ));
    if !item.sockets.is_empty() {
        ui.label(format!("Sockets: {}", item.sockets));
    }
    ui.add_space(4.0);
    for ml in &item.mod_lines {
        let colour = match ml.section {
            pob_data::ModSection::Implicit => egui::Color32::from_rgb(200, 200, 255),
            pob_data::ModSection::Crafted => egui::Color32::from_rgb(180, 230, 255),
            pob_data::ModSection::Enchant => egui::Color32::from_rgb(180, 230, 180),
            pob_data::ModSection::Fractured => egui::Color32::from_rgb(220, 200, 130),
            pob_data::ModSection::Corrupted => egui::Color32::from_rgb(220, 100, 220),
            pob_data::ModSection::Veiled => egui::Color32::from_rgb(180, 180, 180),
            pob_data::ModSection::Explicit => egui::Color32::from_rgb(220, 220, 100),
        };
        ui.colored_label(colour, &ml.line);
    }
}
