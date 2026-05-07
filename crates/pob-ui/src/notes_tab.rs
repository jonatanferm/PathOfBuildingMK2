//! Notes tab — free-form text editor.

use eframe::egui;

pub fn ui(ui: &mut egui::Ui, notes: &mut String) {
    ui.heading("Notes");
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
