//! Calcs tab — flat dump of every computed output stat. Phase 5 minimum: a sortable,
//! filterable table view. The proper "click-to-breakdown" panel comes later — it needs
//! the engine to expose the modifier list contributing to a given stat (which we have
//! the data for via ModDB::iter_named, just not surfaced yet).

use eframe::egui;
use pob_engine::Output;

pub struct CalcsTabState {
    pub filter: String,
}

impl Default for CalcsTabState {
    fn default() -> Self {
        Self {
            filter: String::new(),
        }
    }
}

pub fn ui(ui: &mut egui::Ui, state: &mut CalcsTabState, output: &Output) {
    ui.horizontal(|ui| {
        ui.label("Filter:");
        ui.add(
            egui::TextEdit::singleline(&mut state.filter)
                .desired_width(220.0)
                .hint_text("Life, FireResist, MainSkill, …"),
        );
        ui.separator();
        ui.label(format!("{} stats", output.len()));
    });
    ui.separator();

    let q = state.filter.trim().to_lowercase();
    let mut entries: Vec<(&str, f64)> = output.iter().collect();
    entries.sort_by(|a, b| a.0.cmp(b.0));

    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            egui::Grid::new("calcs_grid")
                .num_columns(2)
                .striped(true)
                .show(ui, |ui| {
                    for (k, v) in entries {
                        if !q.is_empty() && !k.to_lowercase().contains(&q) {
                            continue;
                        }
                        ui.monospace(k);
                        ui.with_layout(
                            egui::Layout::right_to_left(egui::Align::Min),
                            |ui| {
                                let formatted = if v.fract().abs() < 1e-9 {
                                    format!("{v:>12.0}")
                                } else if v.abs() < 100.0 {
                                    format!("{v:>12.4}")
                                } else {
                                    format!("{v:>12.2}")
                                };
                                ui.monospace(formatted);
                            },
                        );
                        ui.end_row();
                    }
                });
        });
}
