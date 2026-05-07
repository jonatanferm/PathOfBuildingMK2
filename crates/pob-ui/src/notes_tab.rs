//! Notes tab — free-form text editor.

use eframe::egui;

pub fn ui(ui: &mut egui::Ui, notes: &mut String) {
    ui.horizontal(|ui| {
        ui.heading("Notes");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let chars = notes.chars().count();
            let lines = notes.lines().count().max(if notes.is_empty() { 0 } else { 1 });
            let words = notes.split_whitespace().count();
            ui.weak(format!("{chars} chars · {words} words · {lines} lines"));
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| {
            ui.add(
                egui::TextEdit::multiline(notes)
                    .desired_width(f32::INFINITY)
                    .desired_rows(40)
                    .font(egui::TextStyle::Body),
            );
        });
}
