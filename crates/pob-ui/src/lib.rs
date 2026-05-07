//! egui UI for Path of Building MK2.
//!
//! Exposes [`PobApp`] which implements [`eframe::App`]. The desktop binary just wraps it.

use eframe::egui;

#[derive(Default)]
pub struct PobApp;

impl PobApp {
    #[must_use]
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        Self
    }
}

impl eframe::App for PobApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        egui::CentralPanel::default().show(ctx, |ui| {
            ui.heading("Path of Building MK2");
            ui.label("Phase 0 done. Workspace skeleton up.");
        });
    }
}
