//! Notes tab — free-form text editor with PoB-style color escape rendering.

use eframe::egui;

use crate::color_codes;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum NotesMode {
    #[default]
    Edit,
    Render,
}

#[derive(Debug, Clone, Default)]
pub struct NotesTabState {
    pub mode: NotesMode,
}

pub fn ui(ui: &mut egui::Ui, notes: &mut String, state: &mut NotesTabState) {
    ui.horizontal(|ui| {
        ui.heading("Notes");
        ui.separator();
        ui.selectable_value(&mut state.mode, NotesMode::Edit, "Edit");
        ui.selectable_value(&mut state.mode, NotesMode::Render, "Render");
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
            let chars = notes.chars().count();
            let lines = notes
                .lines()
                .count()
                .max(if notes.is_empty() { 0 } else { 1 });
            let words = notes.split_whitespace().count();
            ui.weak(format!("{chars} chars · {words} words · {lines} lines"));
        });
    });
    ui.separator();
    egui::ScrollArea::vertical()
        .auto_shrink([false, false])
        .show(ui, |ui| match state.mode {
            NotesMode::Edit => {
                ui.add(
                    egui::TextEdit::multiline(notes)
                        .desired_width(f32::INFINITY)
                        .desired_rows(40)
                        .font(egui::TextStyle::Body),
                );
            }
            NotesMode::Render => {
                let default_color = ui.style().visuals.text_color();
                let font = egui::TextStyle::Body.resolve(ui.style());
                let job = color_codes::to_layout_job(notes, default_color, font);
                ui.label(job);
            }
        });
}
