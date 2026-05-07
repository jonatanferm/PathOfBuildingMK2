//! Import / Export tab — generate or paste an MK2 share code.

use eframe::egui;
use pob_engine::{export_code, import_code, Character};

pub struct ImportExportTabState {
    pub paste: String,
    pub generated: String,
    pub last_message: Option<(bool, String)>,
}

impl Default for ImportExportTabState {
    fn default() -> Self {
        Self {
            paste: String::new(),
            generated: String::new(),
            last_message: None,
        }
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut ImportExportTabState, character: &mut Character) -> bool {
    let mut changed = false;

    ui.horizontal_top(|ui| {
        ui.vertical(|ui| {
            ui.set_min_width(360.0);
            ui.heading("Export current build");
            ui.separator();
            if ui.button("Generate code").clicked() {
                match export_code(character) {
                    Ok(code) => {
                        state.generated = code;
                        state.last_message = Some((true, "Generated.".into()));
                    }
                    Err(e) => {
                        state.last_message = Some((false, format!("Export failed: {e}")));
                    }
                }
            }
            ui.add(
                egui::TextEdit::multiline(&mut state.generated)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .font(egui::TextStyle::Monospace),
            );
        });

        ui.separator();

        ui.vertical(|ui| {
            ui.set_min_width(360.0);
            ui.heading("Import build");
            ui.separator();
            ui.label("Paste an MK2 build code:");
            ui.add(
                egui::TextEdit::multiline(&mut state.paste)
                    .desired_width(f32::INFINITY)
                    .desired_rows(8)
                    .font(egui::TextStyle::Monospace)
                    .hint_text("MK2|..."),
            );
            ui.horizontal(|ui| {
                if ui.button("Import").clicked() {
                    match import_code(&state.paste) {
                        Ok(c) => {
                            *character = c;
                            state.last_message = Some((true, "Imported.".into()));
                            state.paste.clear();
                            changed = true;
                        }
                        Err(e) => {
                            state.last_message = Some((false, format!("Import failed: {e}")));
                        }
                    }
                }
                if ui.button("Clear").clicked() {
                    state.paste.clear();
                    state.last_message = None;
                }
            });
        });
    });

    if let Some((ok, msg)) = &state.last_message {
        ui.add_space(4.0);
        let colour = if *ok {
            egui::Color32::LIGHT_GREEN
        } else {
            egui::Color32::LIGHT_RED
        };
        ui.colored_label(colour, msg);
    }

    changed
}
