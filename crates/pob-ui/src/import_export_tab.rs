//! Import / Export tab — generate or paste an MK2 share code.

use eframe::egui;
use pob_engine::{export_code, import_code, import_pob_code, import_pob_xml, Character};

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
                if ui.button("Import (auto)").clicked() {
                    match auto_import(&state.paste) {
                        Ok((c, kind)) => {
                            *character = c;
                            state.last_message = Some((true, format!("Imported as {kind}.")));
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
            ui.weak("Auto-detects MK2 codes, raw PoB XML, or PoB share codes (zlib+base64).");
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

/// Try the formats in order of specificity.
fn auto_import(input: &str) -> Result<(Character, &'static str), String> {
    let trimmed = input.trim();
    if trimmed.starts_with("MK2|") {
        return import_code(trimmed)
            .map(|c| (c, "MK2 code"))
            .map_err(|e| e.to_string());
    }
    if trimmed.starts_with('<') {
        return import_pob_xml(trimmed)
            .map(|c| (c, "PoB XML"))
            .map_err(|e| e.to_string());
    }
    // Fall through to PoB share code (zlib+base64).
    import_pob_code(trimmed)
        .map(|c| (c, "PoB share code"))
        .map_err(|e| e.to_string())
}
