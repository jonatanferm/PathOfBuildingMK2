//! egui UI for Path of Building MK2. Phase 4a: passive tree screen with live stats.

use std::collections::HashSet;
use std::path::PathBuf;

use eframe::egui;
use pob_data::{NodeId, PassiveTree};
use pob_engine::{character::ClassRef, Character, Output, SkillRegistry};

mod calcs_tab;
mod config_tab;
mod import_export_tab;
mod items_tab;
mod notes_tab;
mod pathfind;
mod skills_tab;
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
    search: String,
    active_tab: Tab,
    items_state: items_tab::ItemsTabState,
    skills_state: skills_tab::SkillsTabState,
    calcs_state: calcs_tab::CalcsTabState,
    import_export_state: import_export_tab::ImportExportTabState,
    skills: SkillRegistry,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Tab {
    Tree,
    Items,
    Skills,
    Config,
    Calcs,
    Notes,
    ImportExport,
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
            PathBuf::from("data"),
            PathBuf::from("../data"),
            PathBuf::from("../../data"),
        ];
        let data_root = candidates
            .iter()
            .find(|p| p.join("trees/3_25.json").is_file())
            .cloned()
            .ok_or_else(|| {
                "could not find data/trees/3_25.json — run `cargo run -p pob-extract --release` from the workspace root".to_owned()
            })?;
        let tree_json = std::fs::read_to_string(data_root.join("trees/3_25.json"))
            .map_err(|e| format!("reading tree: {e}"))?;
        let tree = pob_data::load_passive_tree(&tree_json)
            .map_err(|e| format!("parsing tree: {e}"))?;

        let mut skill_sets = Vec::new();
        if let Ok(entries) = std::fs::read_dir(data_root.join("skills")) {
            for entry in entries.flatten() {
                let p = entry.path();
                if p.extension().and_then(|e| e.to_str()) != Some("json") {
                    continue;
                }
                let stem = p.file_stem().and_then(|s| s.to_str()).unwrap_or("");
                if stem == "index" {
                    continue;
                }
                if let Ok(json) = std::fs::read_to_string(&p) {
                    if let Ok(set) = pob_data::load_skill_file(&json) {
                        skill_sets.push(set);
                    }
                }
            }
        }
        let skills = SkillRegistry::from_files(skill_sets);

        let tree_view = TreeView::new(&tree);
        let character = Character::new(ClassRef::marauder(), 1);
        let output = pob_engine::perform::compute_with_skills(&character, &tree, Some(&skills));

        Ok(LoadedApp {
            tree,
            tree_view,
            character,
            output,
            search: String::new(),
            active_tab: Tab::Tree,
            items_state: items_tab::ItemsTabState::default(),
            skills_state: skills_tab::SkillsTabState::default(),
            calcs_state: calcs_tab::CalcsTabState::default(),
            import_export_state: import_export_tab::ImportExportTabState::default(),
            skills,
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

    egui::TopBottomPanel::top("tab_bar").show(ctx, |ui| {
        ui.horizontal(|ui| {
            ui.selectable_value(&mut app.active_tab, Tab::Tree, "Tree");
            ui.selectable_value(&mut app.active_tab, Tab::Items, "Items");
            ui.selectable_value(&mut app.active_tab, Tab::Skills, "Skills");
            ui.selectable_value(&mut app.active_tab, Tab::Config, "Config");
            ui.selectable_value(&mut app.active_tab, Tab::Calcs, "Calcs");
            ui.selectable_value(&mut app.active_tab, Tab::Notes, "Notes");
            ui.selectable_value(&mut app.active_tab, Tab::ImportExport, "Import / Export");
        });
    });

    if app.active_tab == Tab::Tree {
        egui::TopBottomPanel::top("search_panel").show(ctx, |ui| {
            ui.horizontal(|ui| {
                ui.label("Search:");
                let resp = ui.add(
                    egui::TextEdit::singleline(&mut app.search)
                        .desired_width(220.0)
                        .hint_text("notable name, keyword, stat..."),
                );
                if resp.changed() {
                    update_search(app);
                }
                if ui.button("Clear").clicked() {
                    app.search.clear();
                    update_search(app);
                }
                ui.separator();
                ui.label(format!("{} matches", app.tree_view.search_matches.len()));
                if ui.button("Focus first match").clicked() {
                    if let Some(&id) = app.tree_view.search_matches.iter().next() {
                        if let Some(p) = app.tree_view.position_of(id) {
                            app.tree_view.focus(p.x, p.y);
                        }
                    }
                }
            });
        });
    }

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

            ui.add_space(8.0);
            ui.heading("Main skill");
            ui.separator();
            if app.character.main_skill.is_some() {
                stat_row_decimal(ui, "Avg hit", &app.output, "MainSkillAverageHit");
                stat_row_decimal(ui, "Avg w/ crit", &app.output, "MainSkillAverageHitWithCrit");
                stat_row_decimal(ui, "Speed (cps)", &app.output, "MainSkillSpeed");
                stat_row_decimal(ui, "DPS", &app.output, "MainSkillDPS");
            } else {
                ui.weak("(pick one in the Skills tab)");
            }
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

    egui::CentralPanel::default().show(ctx, |ui| match app.active_tab {
        Tab::Tree => {
            let allocated: HashSet<NodeId> = app.character.allocated.iter().copied().collect();
            let interaction = app.tree_view.ui(ui, &app.tree, &allocated);

            // Path-overlay preview: when the user hovers an unallocated node, plot the
            // shortest path from any allocated node to it.
            app.tree_view.path_overlay.clear();
            if let Some(hover) = interaction.hovered {
                if !allocated.contains(&hover) && !allocated.is_empty() {
                    if let Some(path) = pathfind::shortest_path_from_allocated(&app.tree, &allocated, hover) {
                        app.tree_view.path_overlay = path;
                    }
                }
            }

            if let Some(id) = interaction.clicked {
                if app.character.allocated.contains(&id) {
                    app.character.allocated.remove(&id);
                } else {
                    app.character.allocated.insert(id);
                }
                recompute = true;
            }
        }
        Tab::Items => {
            if items_tab::ui(ui, &mut app.items_state, &mut app.character.items) {
                recompute = true;
            }
        }
        Tab::Skills => {
            if skills_tab::ui(
                ui,
                &mut app.skills_state,
                &mut app.character.main_skill,
                &app.skills,
            ) {
                recompute = true;
            }
        }
        Tab::Config => {
            if config_tab::ui(ui, &mut app.character.config) {
                recompute = true;
            }
        }
        Tab::Calcs => {
            calcs_tab::ui(ui, &mut app.calcs_state, &app.output);
        }
        Tab::Notes => {
            notes_tab::ui(ui, &mut app.character.notes);
        }
        Tab::ImportExport => {
            if import_export_tab::ui(
                ui,
                &mut app.import_export_state,
                &mut app.character,
            ) {
                // Imported character: rebind the tree view (positions stay valid since
                // the tree didn't change) and force recompute.
                app.tree_view.rebind(&app.tree);
                recompute = true;
            }
        }
    });

    // Recompute every frame for now — ~3k nodes is sub-millisecond. We can move to
    // dirty-flagging in Phase 6 polish if profiling shows it matters.
    let _ = recompute;
    app.output = pob_engine::perform::compute_with_skills(&app.character, &app.tree, Some(&app.skills));
}

fn update_search(app: &mut LoadedApp) {
    app.tree_view.search_matches.clear();
    let q = app.search.trim().to_lowercase();
    if q.is_empty() {
        return;
    }
    for (id, node) in &app.tree.nodes {
        let name_match = node
            .name
            .as_deref()
            .map(|s| s.to_lowercase().contains(&q))
            .unwrap_or(false);
        let stat_match = node.stats.iter().any(|s| s.to_lowercase().contains(&q));
        if name_match || stat_match {
            app.tree_view.search_matches.insert(*id);
        }
    }
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

fn stat_row_decimal(ui: &mut egui::Ui, label: &str, out: &Output, key: &str) {
    let v = out.get(key);
    ui.horizontal(|ui| {
        ui.label(label);
        ui.with_layout(egui::Layout::right_to_left(egui::Align::Min), |ui| {
            ui.monospace(format!("{:>9.2}", v));
        });
    });
}
