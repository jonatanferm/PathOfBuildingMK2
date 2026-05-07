//! egui UI for Path of Building MK2. Phase 4a: passive tree screen with live stats.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;
use pob_data::{NodeId, PassiveTree};
use pob_engine::{character::ClassRef, Character, Output};

mod tree_layout;
mod tree_view;

pub use tree_view::TreeView;

pub struct PobApp {
    state: AppState,
}

enum AppState {
    Loaded(LoadedApp),
    Error(String),
}

struct LoadedApp {
    tree: PassiveTree,
    tree_view: TreeView,
    character: Character,
    output: Output,
}

impl PobApp {
    #[must_use]
    pub fn new(_cc: &eframe::CreationContext<'_>) -> Self {
        let state = match Self::load_initial() {
            Ok(loaded) => AppState::Loaded(loaded),
            Err(e) => AppState::Error(e),
        };
        Self { state }
    }

    fn load_initial() -> Result<LoadedApp, String> {
        // Find data/ relative to the workspace root. pob-desktop runs from there.
        let candidates = [
            PathBuf::from("data/trees/3_25.json"),
            PathBuf::from("../data/trees/3_25.json"),
            PathBuf::from("../../data/trees/3_25.json"),
        ];
        let path = candidates
            .iter()
            .find(|p| p.is_file())
            .ok_or_else(|| {
                format!(
                    "could not find data/trees/3_25.json — run `cargo run -p pob-extract --release` from the workspace root\n(searched: {:?})",
                    candidates
                )
            })?;
        let json = std::fs::read_to_string(path)
            .map_err(|e| format!("reading {}: {e}", path.display()))?;
        let tree = pob_data::load_passive_tree(&json)
            .map_err(|e| format!("parsing tree: {e}"))?;

        let tree_view = TreeView::new(&tree);
        let character = Character::new(ClassRef::marauder(), 1);
        let output = pob_engine::compute(&character, &tree);

        Ok(LoadedApp {
            tree,
            tree_view,
            character,
            output,
        })
    }
}

impl eframe::App for PobApp {
    fn update(&mut self, ctx: &egui::Context, _frame: &mut eframe::Frame) {
        match &mut self.state {
            AppState::Error(e) => {
                egui::CentralPanel::default().show(ctx, |ui| {
                    ui.heading("Path of Building MK2");
                    ui.colored_label(egui::Color32::LIGHT_RED, e.as_str());
                });
            }
            AppState::Loaded(app) => render_loaded(ctx, app),
        }
    }
}

fn render_loaded(ctx: &egui::Context, app: &mut LoadedApp) {
    let mut recompute = false;

    egui::SidePanel::left("class_panel")
        .resizable(true)
        .default_width(220.0)
        .show(ctx, |ui| {
            ui.heading("Character");
            ui.separator();

            egui::ComboBox::from_label("Class")
                .selected_text(app.character.class.0.clone())
                .show_ui(ui, |ui| {
                    for c in &app.tree.classes {
                        if ui.selectable_label(app.character.class.0 == c.name, &c.name).clicked() {
                            app.character.class = ClassRef(c.name.clone());
                            recompute = true;
                        }
                    }
                });
            ui.add(
                egui::Slider::new(&mut app.character.level, 1..=100)
                    .text("Level")
                    .step_by(1.0),
            );
            if ui.button("Reset allocation").clicked() {
                app.character.allocated.clear();
                recompute = true;
            }
            ui.label(format!(
                "Allocated: {} nodes",
                app.character.allocated.len()
            ));

            ui.add_space(10.0);
            ui.heading("Stats");
            ui.separator();
            stat_row(ui, "Strength", &app.output, "Strength");
            stat_row(ui, "Dexterity", &app.output, "Dexterity");
            stat_row(ui, "Intelligence", &app.output, "Intelligence");
            ui.add_space(4.0);
            stat_row(ui, "Life", &app.output, "Life");
            stat_row(ui, "Mana", &app.output, "Mana");
            stat_row(ui, "Energy Shield", &app.output, "EnergyShield");
            ui.add_space(4.0);
            stat_row(ui, "Fire Res", &app.output, "FireResist");
            stat_row(ui, "Cold Res", &app.output, "ColdResist");
            stat_row(ui, "Lightning Res", &app.output, "LightningResist");
            stat_row(ui, "Chaos Res", &app.output, "ChaosResist");
        });

    egui::TopBottomPanel::bottom("status_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.label(format!("Tree: {}", app.tree.version));
            ui.separator();
            ui.label(format!(
                "{} nodes ({} groups)",
                app.tree.nodes.len(),
                app.tree.groups.len()
            ));
            ui.separator();
            ui.label(format!("Zoom: {:.3}", app.tree_view.zoom()));
            ui.separator();
            ui.label("Drag to pan, scroll to zoom, click a node to allocate.");
        });
    });

    egui::CentralPanel::default().show(ctx, |ui| {
        // Build a HashSet ref for the view's allocated query without cloning.
        let allocated: HashSet<NodeId> = app.character.allocated.iter().copied().collect();
        if let Some(id) = app.tree_view.ui(ui, &app.tree, &allocated) {
            if app.character.allocated.contains(&id) {
                app.character.allocated.remove(&id);
            } else {
                app.character.allocated.insert(id);
            }
            recompute = true;
        }
    });

    // Recompute every frame for now — ~3k nodes is sub-millisecond. We can move to
    // dirty-flagging in Phase 6 polish if profiling shows it matters.
    let _ = recompute;
    app.output = pob_engine::compute(&app.character, &app.tree);
}

fn stat_row(ui: &mut egui::Ui, label: &str, out: &Output, key: &str) {
    let v = out.get(key);
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.monospace(format!("{:>7.0}", v));
        });
    });
}
